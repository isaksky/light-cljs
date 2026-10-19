//! The compiler's generated-name allocators, how they give the same names
//! whatever ran before and on whichever thread (docs/compiler-concurrency.md,
//! "Generated names"), and how cached namespaces keep them hygienic
//! (docs/compiler-cache.md, "Generated names").
//!
//! A namespace's analysis, and each unit it registers, allocates in a
//! [`Scope`] of its own (light-cljs-332). A name spells its scope's prefix
//! and a count of the scope's own: a namespace's prefix is a digest of its
//! name and source ([`namespace_discriminator`]), a unit's is its
//! registering scope's prefix and that scope's count. So a scope's names
//! depend on its source and where in it the unit was registered, never on
//! the thread, on what the process compiled before or on the order in which
//! units run, and no two scopes spell a name alike. The compile-time
//! runtime's `gensym` numbers in the scope whose macro it expands
//! ([`sync_gensym_scope`]), with its count in that runtime. The compiler's
//! bootstrap macros read in a scope of their own ([`BOOTSTRAP_SCOPE`]).
//! Since a namespace keeps its names wherever it loads and whatever the
//! cache holds, a namespace installed from its artifact retains exactly the
//! names a fresh analysis of its source gives, and a fresh namespace next to
//! hits gives those of an uncached build.
//!
//! Analysis outside every scope (a single source, a REPL form, an
//! evaluation session) draws from the process-wide counters below, whose
//! spellings are one number, so they never meet a scope's. Within one
//! process every such allocation is new. A namespace artifact can come from
//! another process, or from an
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
//! | Allocator | Names in a scope D | Outside every scope | Treatment |
//! | --- | --- | --- | --- |
//! | [`next_syntax_quote`] | `x__D_N__auto__` | `x__N__auto__` | per scope; window reserved |
//! | [`next_anonymous`] | `pK__D_N#` | `pK__N#` | per scope; window reserved |
//! | [`next_binding`] | `binding__D_0_N__auto__` | `binding__N__auto__` | per scope; window reserved |
//! | [`next_destructure`] | `compiler_destructure__D_0_N__auto__` | `compiler_destructure__N__auto__` | per scope; window reserved |
//! | the compile-time runtime's `gensym` | `G__D_N`, `prefixD_N` | `G__N` | per scope; counter and spellings reserved |
//! | a custom data reader evaluator's `gensym` | in reader output | the same | not scoped, not reserved (light-cljs-350) |
//! | `READER_GENERATION` (macro_host.rs) | a compilation's reader generation | | never spelled in output, never retained |
//! | `NEXT_SOURCE` (compiler_data.rs) | [`crate::compiler_data::SourceId`] | | never spelled, never retained |
//! | the session's unit IDs | `declaration#N`, `:test#N` | | renumbered on install |
//!
//! The reader's allocations (`D_N`) count apart from expansion's (`D_0_N`)
//! and the units a scope registers (`D_K`), so the names a namespace's
//! source reads, which its macros carry into other namespaces, depend on
//! that source alone, not on what its requirements' macros expand to.
//!
//! Outside every scope, `gensym` skips every name in the runtime's
//! `generatedNames`: its own results and every unqualified symbol the form
//! bridge decoded. In a scope it does not, so a scope's names do not depend
//! on what the heap decoded before; its spellings carry a 64-bit digest that
//! source does not spell by chance. Reserving spellings imports them as
//! symbols, which adds them there. The reader
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

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

pub use crate::namespace_artifact::{Counters, GeneratedNames};

static SYNTAX_QUOTE: AtomicU64 = AtomicU64::new(1);
static ANONYMOUS: AtomicU64 = AtomicU64::new(1);
static BINDING: AtomicU64 = AtomicU64::new(0);
static DESTRUCTURE: AtomicU64 = AtomicU64::new(0);
/// Identifies each [`Scope`] of the process, so that a compile-time runtime
/// keeps the `gensym` counter of each apart. Never spelled.
static SCOPE_SERIAL: AtomicU64 = AtomicU64::new(1);

/// The number a generated name is spelled with. In a [`Scope`], its prefix
/// and a count of its own: `D_N` for what the reader allocates (syntax-quote
/// gensyms and `#()` parameters) and `D_0_N` for what expansion allocates
/// (`binding` and destructuring locals), where `D` is the scope's prefix,
/// `3963015217_5` say for the fifth unit of a namespace; outside every
/// scope, a process-wide number, as the four allocators always gave.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedId {
    prefix: Option<Arc<str>>,
    expansion: bool,
    number: u64,
}

impl std::fmt::Display for GeneratedId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (&self.prefix, self.expansion) {
            (Some(prefix), false) => write!(f, "{prefix}_{}", self.number),
            (Some(prefix), true) => write!(f, "{prefix}_0_{}", self.number),
            (None, _) => write!(f, "{}", self.number),
        }
    }
}

/// A scope of generated names: the top-level forms of a namespace, or one
/// unit registered in a scope, whose analysis gives the same names whichever
/// thread runs it, whatever ran before it and whenever it runs
/// (docs/compiler-concurrency.md, "Generated names").
///
/// A namespace's prefix is its discriminator
/// ([`namespace_discriminator`]), a digest of its name and source, and a
/// unit's is its registering scope's prefix and that scope's count, `D_K`
/// with K from 1. A scope counts what its reader allocates apart from what
/// its expansions allocate, so the names a namespace's source reads, which
/// macros carry into other namespaces, depend on that source alone. Each
/// spelling has at least two numbers, unlike a process-wide one, and tells
/// its scope and count apart: `D_N`, `D_0_N` and a unit's `D_K_...`.
pub(crate) struct Scope {
    serial: u64,
    prefix: Arc<str>,
    /// The next number of the scope's reader allocations.
    read: Cell<u64>,
    /// The next number of the scope's expansion allocations and of the
    /// units it registers, which share it.
    expanded: Cell<u64>,
}

impl Scope {
    fn next(counter: &Cell<u64>) -> u64 {
        let number = counter.get();
        counter.set(
            number
                .checked_add(1)
                .expect("scoped name identities exhausted"),
        );
        number
    }

    /// What the scope's `gensym` appends before its own count in a
    /// compile-time runtime, which tells scopes apart by serial: `D_` for
    /// `D`. Its names, `G__D_N`, end neither in `__auto__` nor `#`.
    fn gensym_prefix(&self) -> String {
        format!("{}_", self.prefix)
    }
}

thread_local! {
    /// The scopes entered on this thread, innermost last.
    static SCOPES: RefCell<Vec<Rc<Scope>>> = const { RefCell::new(Vec::new()) };
}

/// While alive, the innermost scope of its thread ([`enter`]).
pub(crate) struct ScopeGuard(u64);

impl Drop for ScopeGuard {
    fn drop(&mut self) {
        SCOPES.with(|scopes| {
            let popped = scopes.borrow_mut().pop();
            debug_assert_eq!(popped.map(|scope| scope.serial), Some(self.0));
        });
        // No entry resumes it: the next runtime put in a scope forgets its
        // count. Past the bound, runtimes keep the counts, which costs only
        // their memory.
        RETIRED.with(|retired| {
            let mut retired = retired.borrow_mut();
            if retired.len() >= RETIRED_BOUND {
                retired.clear();
            }
            retired.push(self.0);
        });
    }
}

/// How many ended scopes a thread remembers to retire in a runtime.
const RETIRED_BOUND: usize = 1 << 16;

thread_local! {
    /// The serials of the scopes that ended on this thread since a runtime
    /// was last put in a scope.
    static RETIRED: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

/// Enter the scope `prefix`, counting from one, until the guard is dropped.
/// Analyzing the same scope again counts from one again, so it gives the
/// same names.
pub(crate) fn enter(prefix: Arc<str>) -> ScopeGuard {
    let serial = SCOPE_SERIAL.fetch_add(1, Ordering::Relaxed);
    SCOPES.with(|scopes| {
        scopes.borrow_mut().push(Rc::new(Scope {
            serial,
            prefix,
            read: Cell::new(1),
            expanded: Cell::new(1),
        }))
    });
    ScopeGuard(serial)
}

/// Enter the top-level scope of a namespace with discriminator
/// `discriminator` ([`namespace_discriminator`], or [`BOOTSTRAP_SCOPE`]).
pub(crate) fn enter_namespace(discriminator: u64) -> ScopeGuard {
    enter(discriminator.to_string().into())
}

/// The scope the compiler's bootstrap macros are read and analyzed in, the
/// same in every compilation and on every thread, and no namespace's.
pub(crate) const BOOTSTRAP_SCOPE: u64 = 0;

/// The discriminator of the namespace `name` with source `text`: a 64-bit
/// FNV-1a digest of both, never [`BOOTSTRAP_SCOPE`]. It does not depend on
/// where the namespace is loaded, what the build holds in its cache or what
/// the process compiled before, so a namespace installed from its artifact
/// keeps the names a fresh analysis of the same source gives, and a fresh
/// namespace compiled next to cache hits gives the names it gives in an
/// uncached build. A namespace whose source changed takes other names than
/// the ones hits captured from its earlier source. Two namespaces of a build
/// differ in name, so only a digest collision, about one in 2^64 per pair,
/// could give them one discriminator.
pub(crate) fn namespace_discriminator(name: &str, text: &str) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    for byte in name.bytes().chain([0xff]).chain(text.bytes()) {
        hash = (hash ^ u64::from(byte)).wrapping_mul(PRIME);
    }
    hash.max(1)
}

/// The prefix of a unit registered now in the innermost scope, numbered by
/// that scope's count, or `None` outside every scope.
pub(crate) fn child() -> Option<Arc<str>> {
    innermost().map(|scope| format!("{}_{}", scope.prefix, Scope::next(&scope.expanded)).into())
}

/// The innermost scope of this thread.
fn innermost() -> Option<Rc<Scope>> {
    SCOPES.with(|scopes| scopes.borrow().last().cloned())
}

thread_local! {
    /// The heap whose `gensym` was last put in a scope on this thread, and
    /// that scope's serial, so expanding again in the same one costs nothing.
    static INSTALLED: Cell<(usize, u64)> = const { Cell::new((0, 0)) };
}

/// Put `engine`'s `gensym` in this thread's innermost scope, or in none, as
/// a macro is about to expand there: the names it generates are then the
/// unit's, whichever runtime expands it.
pub(crate) fn sync_gensym_scope(
    engine: &dyn crate::evaluator::MacroEvaluator,
) -> Result<(), crate::evaluator::EvalError> {
    let scope = innermost();
    let serial = scope.as_ref().map_or(0, |scope| scope.serial);
    let heap = engine.heap();
    if heap != 0 && INSTALLED.get() == (heap, serial) {
        return Ok(());
    }
    let prefix = scope.map(|scope| scope.gensym_prefix()).unwrap_or_default();
    let retired = RETIRED.with(|retired| {
        let serials = retired.borrow_mut().drain(..).collect::<Vec<_>>();
        serials
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(",")
    });
    engine.gensym_scope(serial, &prefix, &retired)?;
    INSTALLED.set((heap, serial));
    Ok(())
}

/// When dropped, put the evaluator's `gensym` back in this thread's
/// innermost scope: after a unit analyzed while a macro of another scope
/// runs, as a helper the macro calls is, so the macro goes on numbering in
/// its own scope.
pub(crate) struct RestoreGensymScope<'a>(
    pub(crate) Option<&'a dyn crate::evaluator::MacroEvaluator>,
);

impl Drop for RestoreGensymScope<'_> {
    fn drop(&mut self) {
        if let Some(engine) = self.0 {
            // An engine that failed reports it on its next operation.
            let _ = sync_gensym_scope(engine);
        }
    }
}

/// Forget which heap was put in which scope: a heap created now can take
/// the place of one that went away.
pub(crate) fn forget_installed_scopes() {
    INSTALLED.set((0, 0));
}

fn allocate(process: &AtomicU64, what: &str, expansion: bool) -> GeneratedId {
    match innermost() {
        Some(scope) => GeneratedId {
            number: Scope::next(if expansion {
                &scope.expanded
            } else {
                &scope.read
            }),
            prefix: Some(scope.prefix.clone()),
            expansion,
        },
        None => GeneratedId {
            prefix: None,
            expansion,
            number: process
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
                .unwrap_or_else(|_| panic!("{what} gensym identities exhausted")),
        },
    }
}

/// The number of the next syntax-quote gensym, `x#` read as `x__N__auto__`.
pub(crate) fn next_syntax_quote() -> GeneratedId {
    allocate(&SYNTAX_QUOTE, "syntax quote", false)
}

/// The identity of the next `#()` shorthand, whose parameters read as `pK__N#`.
pub(crate) fn next_anonymous() -> GeneratedId {
    allocate(&ANONYMOUS, "anonymous", false)
}

/// The number of the next `binding__N__auto__` local of `binding`.
pub(crate) fn next_binding() -> GeneratedId {
    allocate(&BINDING, "binding", true)
}

/// The number of the next `compiler_destructure__N__auto__` local.
pub(crate) fn next_destructure() -> GeneratedId {
    allocate(&DESTRUCTURE, "destructure", true)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evaluator::Evaluator;

    fn evaluate(engine: &Evaluator, source: &str) -> String {
        let value = engine.evaluate_unit(source).unwrap();
        engine.print(value).unwrap()
    }

    fn gensym(engine: &Evaluator) -> String {
        sync_gensym_scope(engine).unwrap();
        evaluate(engine, "rt => rt.core.gensym(\"g\")")
    }

    /// A runtime numbers in each scope it is put in, a unit's apart from its
    /// namespace's, goes on in the namespace's after the unit, and forgets
    /// the counts of scopes that ended; entering the same scope again counts
    /// from one again.
    #[test]
    fn completed_gensym_scopes_are_released() {
        let engine = Evaluator::new(Default::default()).unwrap();
        for _ in 0..3 {
            let namespace = enter_namespace(7);
            assert_eq!(gensym(&engine), "g7_1");
            {
                let _unit = enter(child().unwrap());
                assert_eq!(gensym(&engine), "g7_1_1");
                assert_eq!(gensym(&engine), "g7_1_2");
            }
            assert_eq!(gensym(&engine), "g7_2");
            drop(namespace);
        }
        assert_eq!(gensym(&engine), "g1");
        assert_eq!(
            evaluate(&engine, "rt => rt.compilerForms.gensymScopeCount()"),
            "0"
        );
    }

    /// The reader's names and expansion's count apart in one scope, and a
    /// unit's prefix takes the expansion count.
    #[test]
    fn reader_and_expansion_names_count_apart() {
        let _namespace = enter_namespace(5);
        assert_eq!(next_syntax_quote().to_string(), "5_1");
        assert_eq!(next_binding().to_string(), "5_0_1");
        assert_eq!(child().as_deref(), Some("5_2"));
        assert_eq!(next_anonymous().to_string(), "5_2");
        assert_eq!(next_destructure().to_string(), "5_0_3");
        assert_ne!(
            namespace_discriminator("a", "x"),
            namespace_discriminator("b", "x")
        );
        assert_ne!(namespace_discriminator("a", "x"), BOOTSTRAP_SCOPE);
    }
}
