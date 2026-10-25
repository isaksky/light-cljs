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
//! ([`sync_gensym_scope`]), with the scope's count kept here, which a core
//! macro expanded natively numbers from too ([`gensym`]), so that units
//! whose only macros expand natively need no runtime (light-cljs-432); a
//! custom data reader's runtime numbers in the scope of the namespace it
//! reads ([`sync_reader_gensym_scope`]), with its count in that runtime.
//! The compiler's bootstrap macros read in a scope of their own
//! ([`BOOTSTRAP_SCOPE`]).
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
//! | the compile-time runtime's `gensym` | `G__D_N`, `prefixD_N` | `G__N` | per scope, counted here; counter and spellings reserved |
//! | a custom data reader evaluator's `gensym` | `G__D_0_N` | `G__N` | per scope (light-cljs-350); not reserved |
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
//! symbols, which adds them there. In a scope a runtime takes each number
//! from the scope's count here (`__lightGensym`, [`serve_gensym`]), as a
//! core macro expanded natively does without a runtime ([`gensym`]): the
//! two interleave on one counter, so a name is the same whichever side
//! allocates it, and while a capturing compilation logs what `gensym`
//! returns, both log here, on the analyzing thread, in the order they
//! returned the names ([`start_log`]). The reader
//! generation names the compilation whose declarations a macro environment's
//! reader sees; artifacts record reader contexts without it, and an
//! installed namespace's macros read in the installing compilation's
//! generation. Source IDs identify loaded source text inside one process;
//! artifacts refer to sources by index. Unit IDs are relocated by
//! [`crate::artifact_install`].
//!
//! A custom data reader runs in an evaluator of its own, which a cache hit
//! never starts, so its counter in a miss after a hit is not where it is in
//! an uncached build. In a scope its `gensym` spells `D_0_` and a count of
//! the scope's own in that evaluator, which no unit's prefix `D_K` begins:
//! its names in a namespace are the same whichever namespaces hit, so they
//! need no reservation, and differ from the compile-time runtime's. Its
//! function is evaluated in a scope of the reader's own
//! ([`enter_reader_definition`]), so a `gensym` its definition calls gives
//! one name whichever namespace first reads with it.
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
    /// The next number the compile-time runtime's `gensym` appends in the
    /// scope, whichever side allocates it: a core macro expanded natively
    /// ([`gensym`]) or a runtime put in the scope ([`serve_gensym`]), so
    /// that the two interleave as one counter (light-cljs-432).
    gensym: Cell<u64>,
    /// Whether a compile-time runtime was put in the scope, whose `gensym`
    /// may still number in it after it ends ([`ENDED`]).
    installed: Cell<bool>,
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

    /// What the scope's `gensym` appends to its prefix argument, `D_N`,
    /// taking the next number of the scope's count.
    fn gensym_suffix(&self) -> String {
        format!("{}{}", self.gensym_prefix(), Self::next(&self.gensym))
    }

    /// What a custom data reader's `gensym` appends in the scope: `D_0_`,
    /// which no unit's prefix `D_K` begins, K being from 1, so its names,
    /// `G__D_0_N`, are never the compile-time runtime's.
    fn reader_gensym_prefix(&self) -> String {
        format!("{}_0_", self.prefix)
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
        let popped = SCOPES.with(|scopes| scopes.borrow_mut().pop());
        debug_assert_eq!(popped.as_ref().map(|scope| scope.serial), Some(self.0));
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
        // A runtime put in it keeps it until the next runtime put in a
        // scope: its `gensym` goes on with the scope's count meanwhile.
        if let Some(scope) = popped.filter(|scope| scope.installed.get()) {
            ENDED.with(|ended| {
                let mut ended = ended.borrow_mut();
                if ended.len() >= ENDED_BOUND {
                    ended.remove(0);
                }
                ended.push(scope);
            });
        }
        READER_RETIRED.with(|retired| {
            if let Some(retired) = retired.borrow_mut().as_mut() {
                if retired.len() >= RETIRED_BOUND {
                    retired.clear();
                }
                retired.push(self.0);
            }
        });
    }
}

/// How many ended scopes a thread remembers to retire in a runtime.
const RETIRED_BOUND: usize = 1 << 16;

thread_local! {
    /// The serials of the scopes that ended on this thread since a runtime
    /// was last put in a scope.
    static RETIRED: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
    /// The same for the custom data readers' runtimes, kept once one was
    /// put in a scope on this thread ([`sync_reader_gensym_scope`]).
    static READER_RETIRED: RefCell<Option<Vec<u64>>> = const { RefCell::new(None) };
    /// The latest scopes that ended on this thread after a compile-time
    /// runtime was put in them, whose `gensym` numbers in such a scope
    /// until it is put in another one ([`serve_gensym`]).
    static ENDED: RefCell<Vec<Rc<Scope>>> = const { RefCell::new(Vec::new()) };
    /// While the compilation on this thread logs them, what the compile-time
    /// runtime's `gensym` returned, in the order it returned them, whichever
    /// side allocated them ([`start_log`]).
    static LOG: RefCell<Option<Vec<Vec<u16>>>> = const { RefCell::new(None) };
}

/// How many ended scopes [`ENDED`] keeps.
const ENDED_BOUND: usize = 64;

/// The scope with `serial` that a compile-time runtime of this thread was
/// put in: an entered one, innermost first, or one that ended since.
fn installed_scope(serial: u64) -> Option<Rc<Scope>> {
    SCOPES
        .with(|scopes| {
            scopes
                .borrow()
                .iter()
                .rev()
                .find(|scope| scope.serial == serial)
                .cloned()
        })
        .or_else(|| {
            ENDED.with(|ended| {
                ended
                    .borrow()
                    .iter()
                    .rev()
                    .find(|scope| scope.serial == serial)
                    .cloned()
            })
        })
}

/// The name the compile-time runtime's `gensym` returns for the prefix
/// argument `base` in this thread's innermost scope, allocated here rather
/// than in a runtime, so that a core macro expanded natively needs none
/// ([`crate::native_core_macros`]); `None` outside every scope, where the
/// runtime's own counter numbers. It is the name a runtime put in the
/// scope would return there, the next in the scope's count, and is logged
/// as it would be ([`start_log`]).
pub(crate) fn gensym(base: &str) -> Option<String> {
    let name = format!("{base}{}", innermost()?.gensym_suffix());
    log(|| name.encode_utf16().collect());
    Some(name)
}

thread_local! {
    /// The count of [`stand_in`]'s names on this thread.
    static STAND_INS: Cell<u64> = const { Cell::new(0) };
}

/// A name that stands in for one [`gensym`] would allocate for `base`,
/// without taking a number of the scope's count: distinct from every other
/// stand-in and, carrying the innermost scope's digest and an `s` no
/// allocated name spells, from every symbol a source spells but by a digest
/// collision and from every allocated name, so that an expansion built with
/// stand-ins has the shape of the one built with names
/// ([`crate::native_core_macros::native_expansion`]).
pub(crate) fn stand_in(base: &str) -> String {
    let number = STAND_INS.get() + 1;
    STAND_INS.set(number);
    let prefix = innermost().map_or_else(|| "0".into(), |scope| scope.prefix.clone());
    format!("{base}{prefix}_s{number}")
}

/// Answer a compile-time runtime's `gensym` (`__lightGensym`, build.rs
/// `compiler_runtime`): `g` and a scope's serial take the next number of
/// that scope's count, and answer what the name appends to the prefix
/// argument, or nothing for a scope this thread no longer knows, which the
/// runtime then counts itself, from one, as it counted every scope before
/// light-cljs-432; `L` and a name, or `U` and its UTF-16 code units, log a
/// name the runtime returned, while the compilation logs them.
pub(crate) fn serve_gensym(request: &str) -> String {
    match request.split_at_checked(1) {
        Some(("g", serial)) => serial
            .parse()
            .ok()
            .and_then(installed_scope)
            .map(|scope| scope.gensym_suffix())
            .unwrap_or_default(),
        Some(("L", name)) => {
            log(|| name.encode_utf16().collect());
            String::new()
        }
        Some(("U", units)) => {
            log(|| {
                units
                    .split(',')
                    .filter_map(|unit| unit.parse::<u16>().ok())
                    .collect()
            });
            String::new()
        }
        _ => String::new(),
    }
}

/// Add a name to this thread's log, if the compilation logs them.
fn log(name: impl FnOnce() -> Vec<u16>) {
    LOG.with(|log| {
        if let Some(log) = log.borrow_mut().as_mut() {
            log.push(name());
        }
    });
}

/// Log, from now on and on this thread, what the compile-time runtime's
/// `gensym` returns, whichever side allocates it, forgetting what was
/// logged before: a capturing compilation records the names its
/// namespaces' analyses generated ([`GeneratedNames`]). A runtime logs its
/// own once the session has read its log (`takeGensyms`).
pub(crate) fn start_log() {
    LOG.with(|log| *log.borrow_mut() = Some(Vec::new()));
}

/// What was logged on this thread since the log started or was last
/// taken, in order.
pub(crate) fn take_log() -> Vec<Vec<u16>> {
    LOG.with(|log| log.borrow_mut().as_mut().map(std::mem::take))
        .unwrap_or_default()
}

/// While alive, this thread logs nothing ([`unlogged`]).
pub(crate) struct Unlogged(Option<Vec<Vec<u16>>>);

impl Drop for Unlogged {
    fn drop(&mut self) {
        let log = self.0.take();
        LOG.with(|current| *current.borrow_mut() = log);
    }
}

/// Log nothing on this thread until the guard is dropped, then go on with
/// the log as it was: the compiler's bootstrap allocates names that are
/// its own, as its engine, which does not log, did before light-cljs-432.
pub(crate) fn unlogged() -> Unlogged {
    Unlogged(LOG.with(|log| log.borrow_mut().take()))
}

/// While alive, this thread logs into a log of its own if it logged, and
/// the log as it was goes on after it ([`NestedLog::finish`]): a unit
/// recorded on its own inside another unit's recording keeps its own names
/// (light-cljs-442).
pub(crate) struct NestedLog(Unlogged);

impl NestedLog {
    /// What this thread logged since [`nest_log`], or `None` if it did not
    /// log then; the outer log goes on.
    pub(crate) fn finish(self) -> Option<Vec<Vec<u16>>> {
        self.0.0.is_some().then(stop_log)
    }
}

/// Set this thread's log aside until the guard finishes or drops, logging
/// meanwhile into a new one if it logged.
pub(crate) fn nest_log() -> NestedLog {
    let outer = unlogged();
    if outer.0.is_some() {
        start_log();
    }
    NestedLog(outer)
}

/// Stop logging on this thread, and what was logged since the log started
/// or was last taken.
pub(crate) fn stop_log() -> Vec<Vec<u16>> {
    LOG.with(|log| log.borrow_mut().take()).unwrap_or_default()
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
            gensym: Cell::new(1),
            installed: Cell::new(false),
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

/// Enter the scope in which the custom data reader `key` (`#tag`, or `#`
/// for the default reader) evaluates its function `unit`: its prefix is a
/// digest of both, like a namespace's, which a namespace's never is but by
/// a digest collision, since no namespace name begins with `#`. A `gensym`
/// the definition calls thus gives the same name whichever reader is
/// evaluated first and whichever namespaces are cache hits.
pub(crate) fn enter_reader_definition(key: &str, unit: &str) -> ScopeGuard {
    enter_namespace(namespace_discriminator(key, unit))
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

/// Whether this thread is in a scope, where the compile-time runtime's
/// `gensym` numbers in the scope and skips no name the heap decoded.
pub(crate) fn scoped() -> bool {
    SCOPES.with(|scopes| !scopes.borrow().is_empty())
}

thread_local! {
    /// The heap whose `gensym` was last put in a scope on this thread, and
    /// that scope's serial, so expanding again in the same one costs nothing.
    static INSTALLED: Cell<(usize, u64)> = const { Cell::new((0, 0)) };
}

/// Put `engine`'s `gensym` in this thread's innermost scope, or in none, as
/// a macro is about to expand there: the names it generates are then the
/// unit's, whichever runtime expands it. In a scope it takes each number
/// from the scope's count here ([`serve_gensym`]), which a core macro
/// expanded natively takes from as well ([`gensym`]).
pub(crate) fn sync_gensym_scope(
    engine: &dyn crate::evaluator::MacroEvaluator,
) -> Result<(), crate::evaluator::EvalError> {
    let scope = innermost();
    if let Some(scope) = &scope {
        scope.installed.set(true);
    }
    sync(
        engine,
        scope.as_deref(),
        Scope::gensym_prefix,
        true,
        &RETIRED,
        |retired| retired.borrow_mut().drain(..).collect(),
    )
}

/// Put the `gensym` of a custom data reader's runtime `engine` in this
/// thread's innermost scope, or in none, as the reader is about to read
/// there or its function is evaluated ([`enter_reader_definition`]): the
/// names a reader returns in a namespace then spell that namespace's
/// scope, so they are what an uncached build gives whichever namespaces
/// are cache hits, and differ from every other namespace's and from those
/// of the compile-time runtime (light-cljs-350).
pub(crate) fn sync_reader_gensym_scope(
    engine: &dyn crate::evaluator::MacroEvaluator,
) -> Result<(), crate::evaluator::EvalError> {
    let scope = innermost();
    sync(
        engine,
        scope.as_deref(),
        Scope::reader_gensym_prefix,
        false,
        &READER_RETIRED,
        |retired| std::mem::take(retired.borrow_mut().get_or_insert_default()),
    )
}

/// Put `engine`'s `gensym` in `scope`, spelled with `prefix`, retiring in
/// it the scopes that `take` hands over from `retired`; `native` if it takes
/// the scope's numbers from here rather than counting them itself.
fn sync<T: 'static>(
    engine: &dyn crate::evaluator::MacroEvaluator,
    scope: Option<&Scope>,
    prefix: fn(&Scope) -> String,
    native: bool,
    retired: &'static std::thread::LocalKey<RefCell<T>>,
    take: fn(&RefCell<T>) -> Vec<u64>,
) -> Result<(), crate::evaluator::EvalError> {
    let serial = scope.map_or(0, |scope| scope.serial);
    let heap = engine.heap();
    if heap != 0 && INSTALLED.get() == (heap, serial) {
        return Ok(());
    }
    let prefix = scope.map(prefix).unwrap_or_default();
    let retired = retired.with(|retired| {
        take(retired)
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(",")
    });
    engine.gensym_scope(serial, &prefix, &retired, native)?;
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
#[cfg_attr(not(light_quickjs), allow(dead_code))]
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
    #[cfg(light_quickjs)]
    use crate::evaluator::Evaluator;

    #[cfg(light_quickjs)]
    fn evaluate(engine: &Evaluator, source: &str) -> String {
        let value = engine.evaluate_unit(source).unwrap();
        engine.print(value).unwrap()
    }

    #[cfg(light_quickjs)]
    fn gensym(engine: &Evaluator) -> String {
        sync_gensym_scope(engine).unwrap();
        evaluate(engine, "rt => rt.core.gensym(\"g\")")
    }

    /// A runtime numbers in each scope it is put in, a unit's apart from its
    /// namespace's, goes on in the namespace's after the unit, and forgets
    /// the counts of scopes that ended; entering the same scope again counts
    /// from one again.
    #[test]
    #[cfg(light_quickjs)]
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

    /// A custom data reader's runtime numbers in the scope apart from the
    /// compile-time runtime, in a scope of a reader's own while its
    /// definition is evaluated, which counts from one again each time, and
    /// forgets the counts of scopes that ended, which the compile-time
    /// runtime forgets as well.
    #[test]
    #[cfg(light_quickjs)]
    fn reader_gensyms_number_apart_in_each_scope() {
        let session = Evaluator::new(Default::default()).unwrap();
        let reader = Evaluator::new(Default::default()).unwrap();
        let read = || {
            sync_reader_gensym_scope(&reader).unwrap();
            evaluate(&reader, "rt => rt.core.gensym(\"g\")")
        };
        let definition = namespace_discriminator("#t", "unit");
        for round in 0..2 {
            let namespace = enter_namespace(7);
            assert_eq!(gensym(&session), "g7_1");
            assert_eq!(read(), "g7_0_1");
            {
                let _definition = enter_reader_definition("#t", "unit");
                assert_eq!(read(), format!("g{definition}_0_1"));
            }
            {
                let _unit = enter(child().unwrap());
                assert_eq!(read(), "g7_1_0_1");
                assert_eq!(gensym(&session), "g7_1_1");
            }
            assert_eq!(read(), "g7_0_2");
            assert_eq!(gensym(&session), "g7_2");
            drop(namespace);
            // Outside every scope, the runtime goes on with its counter.
            assert_eq!(read(), format!("g{}", round + 1));
            gensym(&session);
        }
        for engine in [&session, &reader] {
            assert_eq!(
                evaluate(engine, "rt => rt.compilerForms.gensymScopeCount()"),
                "0"
            );
        }
    }

    /// A core macro expanded natively and a runtime put in the scope take
    /// their numbers from one count, a unit's apart from its namespace's,
    /// and while the compilation logs, both log here, in the order the
    /// names were returned, once (light-cljs-432).
    #[test]
    #[cfg(light_quickjs)]
    fn native_and_runtime_gensyms_share_count_and_log() {
        let engine = Evaluator::new(Default::default()).unwrap();
        start_log();
        // The runtime logs once its log was read.
        evaluate(&engine, "rt => rt.compilerForms.takeGensyms()");
        let native = |base| super::gensym(base);
        let namespace = enter_namespace(7);
        assert_eq!(native("g").as_deref(), Some("g7_1"));
        assert_eq!(gensym(&engine), "g7_2");
        {
            let _unit = enter(child().unwrap());
            assert_eq!(gensym(&engine), "g7_1_1");
            assert_eq!(native("g").as_deref(), Some("g7_1_2"));
        }
        assert_eq!(gensym(&engine), "g7_3");
        assert_eq!(native("g").as_deref(), Some("g7_4"));
        let logged = |names: &[&str]| {
            names
                .iter()
                .map(|name| name.encode_utf16().collect::<Vec<_>>())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            take_log(),
            logged(&["g7_1", "g7_2", "g7_1_1", "g7_1_2", "g7_3", "g7_4"])
        );
        assert!(take_log().is_empty());
        // A stand-in takes no number.
        assert!(stand_in("g").starts_with("g7_s"));
        assert_eq!(native("g").as_deref(), Some("g7_5"));
        drop(namespace);
        assert_eq!(native("g"), None);
        assert_eq!(stop_log().len(), 1);
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
