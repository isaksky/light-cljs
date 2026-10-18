//! The compiler's generated-name allocators, and how cached namespaces keep
//! them hygienic (docs/compiler-cache.md, "Generated names").
//!
//! Names the compiler makes up for code that never spelled them are drawn
//! from process-wide counters, so within one process every allocation is
//! new. A namespace artifact can come from another process, or from an
//! earlier compilation of this one, so the names it retains can be ones this
//! compilation would allocate again. Each artifact therefore records the
//! names its namespace's analysis generated ([`GeneratedNames`]): the window
//! of each process-wide counter it drew from, and the compile-time runtime's
//! `gensym` counter and the spellings it returned. Every generated name the
//! artifact retains, its own or one an earlier namespace's code made, was
//! numbered below those marks. Installing it reserves them ([`reserve`], and
//! the session's `reserve_gensyms`), so no later allocation of the
//! compilation takes one of the numbers or spellings, which keeps names a
//! macro derives from them, such as munged ones, apart as in an uncached
//! build; and installation refuses an artifact when a window overlaps
//! numbers the compilation already allocated or another installed
//! namespace's window ([`Taken`]). A [`Reservation`] of every artifact a
//! compilation may install, made before its first namespace, keeps the
//! namespaces compiled fresh before a hit apart from it as well. Retained
//! names are never rewritten: a gensym that escapes into a constant prints
//! what it printed when it was captured.
//!
//! The allocators, explicitly:
//!
//! | Allocator | Names | Treatment |
//! | --- | --- | --- |
//! | [`next_syntax_quote`] | syntax-quote `x#` as `x__N__auto__` | window reserved |
//! | [`next_anonymous`] | `#()` parameters `pK__N#` | window reserved |
//! | [`next_binding`] | `binding__N__auto__` of `binding` | window reserved |
//! | [`next_destructure`] | `compiler_destructure__N__auto__` | window reserved |
//! | the compile-time runtime's `gensym` | `G__N` and prefixed names | counter and spellings reserved |
//! | a custom data reader evaluator's `gensym` | the same, in reader output | not reserved (light-cljs-350) |
//! | `READER_GENERATION` (macro_host.rs) | a compilation's reader generation | never retained |
//! | `NEXT_SOURCE` (compiler_data.rs) | [`crate::compiler_data::SourceId`] | never retained |
//! | the session's unit IDs | `declaration#N`, `:test#N` | renumbered on install |
//!
//! `gensym` skips every name in the runtime's `generatedNames`: its own
//! results and every unqualified symbol the form bridge decoded. Reserving
//! spellings imports them as symbols, which adds them there. The reader
//! generation names the compilation whose declarations a macro environment's
//! reader sees; artifacts record reader contexts without it, and an
//! installed namespace's macros read in the installing compilation's
//! generation. Source IDs identify loaded source text inside one process;
//! artifacts refer to sources by index. Unit IDs are relocated by
//! [`crate::artifact_install`]. A custom data reader runs in an evaluator of
//! its own, whose counter is independent of the session's, so its names are
//! not kept apart from the session's, in uncached builds either.
//!
//! A macro that needs the allocation sequence itself to repeat, rather than
//! fresh names, does not keep that across a cache hit: its namespace opts
//! out with `^:light-cljs/no-cache`, or the build names it with
//! `--cache-blocker`.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

pub use crate::namespace_artifact::{Counters, GeneratedNames};

static SYNTAX_QUOTE: AtomicU64 = AtomicU64::new(1);
static ANONYMOUS: AtomicU64 = AtomicU64::new(1);
static BINDING: AtomicU64 = AtomicU64::new(0);
static DESTRUCTURE: AtomicU64 = AtomicU64::new(0);

/// The number of the next syntax-quote gensym, `x#` read as `x__N__auto__`.
pub(crate) fn next_syntax_quote() -> u64 {
    SYNTAX_QUOTE
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
        .expect("syntax quote gensym identities exhausted")
}

/// The identity of the next `#()` shorthand, whose parameters read as `pK__N#`.
pub(crate) fn next_anonymous() -> u64 {
    ANONYMOUS.fetch_add(1, Ordering::Relaxed)
}

/// The number of the next `binding__N__auto__` local of `binding`.
pub(crate) fn next_binding() -> u64 {
    BINDING
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
        .expect("binding gensym identities exhausted")
}

/// The number of the next `compiler_destructure__N__auto__` local.
pub(crate) fn next_destructure() -> u64 {
    DESTRUCTURE.fetch_add(1, Ordering::Relaxed)
}

/// The next number each process-wide allocator will allocate.
pub fn current() -> Counters {
    Counters {
        syntax_quote: SYNTAX_QUOTE.load(Ordering::Relaxed),
        anonymous: ANONYMOUS.load(Ordering::Relaxed),
        binding: BINDING.load(Ordering::Relaxed),
        destructure: DESTRUCTURE.load(Ordering::Relaxed),
    }
}

/// Move the process-wide allocators to at least `to`, so none of them
/// allocates a number below it again.
pub fn reserve(to: &Counters) {
    SYNTAX_QUOTE.fetch_max(to.syntax_quote, Ordering::Relaxed);
    ANONYMOUS.fetch_max(to.anonymous, Ordering::Relaxed);
    BINDING.fetch_max(to.binding, Ordering::Relaxed);
    DESTRUCTURE.fetch_max(to.destructure, Ordering::Relaxed);
}

/// Set the process-wide allocators back to `to`, as a new process would
/// start below the numbers an artifact captured elsewhere retains. Only for
/// tests that stand for such a process: every compilation still running in
/// this process can allocate its numbers again. Each nextest test is its own
/// process.
#[doc(hidden)]
pub fn rewind(to: &Counters) {
    SYNTAX_QUOTE.store(to.syntax_quote, Ordering::Relaxed);
    ANONYMOUS.store(to.anonymous, Ordering::Relaxed);
    BINDING.store(to.binding, Ordering::Relaxed);
    DESTRUCTURE.store(to.destructure, Ordering::Relaxed);
}

/// What a compilation reserves before its first namespace for the artifacts
/// it may install ([`crate::compiler::EntryRecorders::reserve`]): the end of
/// every window, the largest `gensym` mark and every `gensym` spelling.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reservation {
    pub to: Counters,
    pub gensym_to: u64,
    pub gensyms: Vec<Vec<u16>>,
}

impl Reservation {
    /// The reservation of every artifact whose names `names` yields, leaving
    /// out names that do not validate ([`GeneratedNames::validate`]), whose
    /// artifacts installation refuses.
    pub fn of<'a>(names: impl IntoIterator<Item = &'a GeneratedNames>) -> Self {
        let mut reservation = Self::default();
        for names in names.into_iter().filter(|names| names.validate().is_ok()) {
            reservation.to = reservation.to.max(names.to);
            reservation.gensym_to = reservation.gensym_to.max(names.gensym_to);
            reservation.gensyms.extend(names.gensyms.iter().cloned());
        }
        reservation
    }
}

/// The numbers of each process-wide allocator a compilation holds: those
/// it allocated itself and the windows of the namespaces it installed, as
/// disjoint intervals by start, so an overlap is found in logarithmic time.
#[derive(Default)]
pub(crate) struct Taken {
    intervals: [BTreeMap<u64, u64>; 4],
}

impl Taken {
    /// Hold `from..to` of each allocator.
    pub(crate) fn take(&mut self, from: &Counters, to: &Counters) {
        for (index, ((_, from), (_, to))) in from.each().into_iter().zip(to.each()).enumerate() {
            if from < to {
                self.intervals[index].insert(from, to);
            }
        }
    }

    /// The first allocator whose window `from..to` overlaps a number held.
    pub(crate) fn overlap(&self, from: &Counters, to: &Counters) -> Option<&'static str> {
        const NAMES: [&str; 4] = ["syntax-quote", "anonymous", "binding", "destructure"];
        from.each()
            .into_iter()
            .zip(to.each())
            .enumerate()
            .find(|(index, ((_, from), (_, to)))| {
                from < to
                    && self.intervals[*index]
                        .range(..*to)
                        .next_back()
                        .is_some_and(|(_, end)| end > from)
            })
            .map(|(index, _)| NAMES[index])
    }
}
