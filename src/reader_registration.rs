//! What a compile-time engine has of the program's namespaces.
//!
//! A macro declaration and every compile-time unit an engine evaluates first
//! register the namespaces of their program with the engine's runtime: each
//! one's reader context and definition names (`registerReaderNamespace`).
//! Registering every namespace each time made a declaration cost the
//! program's size, so that a project's macros cost its macros times its
//! definitions (light-cljs-423). An engine keeps [`ReaderNamespaces`], what
//! it has registered, and a registration lists only the namespaces whose
//! reader context changed since, fully, and of the others only the
//! definitions added since.
//!
//! An engine's definitions of a namespace only grow: a unit's program is a
//! snapshot of the compilation's taken where the unit was defined, and a
//! snapshot older than what the engine has registers nothing it lacks. Its
//! reader contexts are those the last registration named, as before.
//!
//! A unit compiled while JavaScript runs (`__lightCompileUnit`) may run
//! inside a macro's reader view of a namespace (`withMacroReaderContext`),
//! which restores the namespace's earlier context when the macro returns
//! and is no context to add definitions to. Such a registration names each
//! namespace it registers fully, as every registration did before, and the
//! next registration names those namespaces fully again.

use crate::analyzer::{GlobalBinding, GlobalId, Program, RuntimeReaderNamespace};
use crate::arena::Arena;
use crate::compile_trace::RegistrationCounts;
use crate::evaluator::{EvalError, MacroEvaluator};
use crate::layered::Layered;
use crate::shared::Shared;
use std::collections::{HashMap, HashSet};

/// A program's reader contexts by namespace.
pub(crate) type ReaderContexts = Shared<Layered<String, Shared<RuntimeReaderNamespace>>>;

/// The tables of a program that registering its namespaces reads.
#[derive(Clone, Default)]
pub(crate) struct ReaderTables {
    pub namespaces: Arena<String>,
    pub globals: Arena<GlobalBinding>,
    pub reader_contexts: ReaderContexts,
}

impl ReaderTables {
    pub(crate) fn of(program: &Program) -> Self {
        Self {
            namespaces: program.namespaces.clone(),
            globals: program.globals.clone(),
            reader_contexts: program.reader_contexts.clone(),
        }
    }

    /// The tables of `program`, which no longer has them.
    pub(crate) fn take(program: &mut Program) -> Self {
        Self {
            namespaces: std::mem::take(&mut program.namespaces),
            globals: std::mem::take(&mut program.globals),
            reader_contexts: std::mem::take(&mut program.reader_contexts),
        }
    }
}

/// One namespace of a registration, in its program's namespace order.
pub(crate) struct Planned<'t> {
    pub namespace: &'t str,
    /// Whether the statement replaces the namespace's reader context, with
    /// the definitions `ids` and its macros; otherwise it adds the
    /// definitions `ids` to the context the engine has.
    pub full: bool,
    pub ids: Vec<GlobalId>,
}

/// What an engine has registered of one namespace.
struct Registered {
    /// Its position in the registered namespaces.
    position: usize,
    /// The reader context registered: `None` for a namespace without one,
    /// or before the namespace was registered at all (`registered`).
    context: Option<Shared<RuntimeReaderNamespace>>,
    registered: bool,
    /// How many of `ReaderNamespaces::members` of the namespace the
    /// engine has.
    bindings: usize,
}

/// What an engine has registered of a compilation's namespaces.
#[derive(Default)]
pub struct ReaderNamespaces {
    /// The longest globals table registered, which the others are
    /// prefixes of, and its namespaces.
    globals: Arena<GlobalBinding>,
    namespaces: Arena<String>,
    /// The reader contexts the last registration named.
    contexts: Option<ReaderContexts>,
    /// The globals of each namespace of `globals`, in ID order.
    members: HashMap<String, Arena<GlobalId>>,
    state: HashMap<String, Registered>,
    /// The namespaces whose registration may lag what a later program that
    /// adds nothing to the last one names: with fewer definitions than
    /// `members`, or a reader context other than the last program's.
    pending: HashSet<String>,
    /// How many compile-unit callbacks of the engine are running
    /// ([`in_callback`]), and the namespaces registered inside one, which
    /// a macro's view may have restored since.
    callbacks: usize,
    touched: HashSet<String>,
}

impl ReaderNamespaces {
    /// Forget what the engine has, so that the next registration names
    /// every namespace of its program.
    pub(crate) fn forget(&mut self) {
        *self = Self {
            callbacks: self.callbacks,
            ..Self::default()
        };
    }

    /// The namespaces of `tables` to register: those whose reader context
    /// differs from the one the engine has, or that a registration inside a
    /// callback named, fully, and of the others the definitions it lacks,
    /// which inside a callback are full too. Records them as registered.
    pub(crate) fn plan<'t>(
        &mut self,
        tables: &'t ReaderTables,
        counts: &mut RegistrationCounts,
    ) -> Vec<Planned<'t>> {
        counts.registrations += 1;
        if self.diverged(tables) {
            counts.restarts += 1;
            self.forget();
        }
        // The namespaces to look at, each once.
        let mut candidates: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut push = |name: &str, candidates: &mut Vec<String>| {
            if !seen.contains(name) {
                seen.insert(name.to_owned());
                candidates.push(name.to_owned());
            }
        };
        // Namespaces new to the engine.
        if tables.namespaces.len() > self.namespaces.len() {
            for (offset, namespace) in tables
                .namespaces
                .tail_from(self.namespaces.len())
                .enumerate()
            {
                let position = self.namespaces.len() + offset;
                self.state
                    .entry(namespace.clone())
                    .or_insert_with(|| Registered {
                        position,
                        context: None,
                        registered: false,
                        bindings: 0,
                    });
                push(namespace, &mut candidates);
            }
            self.namespaces = tables.namespaces.clone();
        }
        // Definitions new to the engine.
        if tables.globals.len() > self.globals.len() {
            for (offset, global) in tables.globals.tail_from(self.globals.len()).enumerate() {
                let id = GlobalId(self.globals.len() + offset);
                match self.members.get_mut(&global.namespace) {
                    Some(ids) => ids.push(id),
                    None => {
                        let mut ids = Arena::default();
                        ids.push(id);
                        self.members.insert(global.namespace.clone(), ids);
                    }
                }
                push(&global.namespace, &mut candidates);
            }
            self.globals = tables.globals.clone();
        }
        // Namespaces whose reader context changed since the last program.
        match &self.contexts {
            Some(before) if Shared::ptr_eq(before, &tables.reader_contexts) => {}
            Some(before) => {
                for namespace in tables.reader_contexts.changed_keys(before) {
                    push(namespace, &mut candidates);
                }
            }
            None => {
                for namespace in tables.reader_contexts.keys() {
                    push(namespace, &mut candidates);
                }
            }
        }
        for namespace in self.pending.iter().chain(&self.touched) {
            push(namespace, &mut candidates);
        }
        let inside = self.callbacks > 0;
        self.contexts = Some(tables.reader_contexts.clone());
        let mut plan = Vec::new();
        for namespace in candidates {
            let members = self.members.get(&namespace);
            let Some(state) = self.state.get_mut(&namespace) else {
                // Not a namespace of the program.
                continue;
            };
            let context = tables.reader_contexts.get(&namespace);
            if state.position >= tables.namespaces.len() {
                // A namespace of a later program than this one.
                if !same_context(&state.context, context) {
                    self.pending.insert(namespace);
                }
                continue;
            }
            // Of the namespace's definitions, those in this program.
            let available = members.map_or(0, |ids| below(ids, tables.globals.len()));
            let touched = self.touched.contains(&namespace);
            let full = !state.registered
                || !same_context(&state.context, context)
                || touched
                || (inside && available > state.bindings);
            let from = if full { 0 } else { state.bindings };
            if full || available > from {
                let ids = members.map_or_else(Vec::new, |ids| {
                    ids.range(from..available).copied().collect()
                });
                if full {
                    counts.full += 1;
                } else {
                    counts.added += 1;
                }
                plan.push((
                    state.position,
                    Planned {
                        namespace: &tables.namespaces[state.position],
                        full,
                        ids,
                    },
                ));
                state.bindings = available;
            }
            state.registered = true;
            state.context = context.cloned();
            if inside && full {
                self.touched.insert(namespace.clone());
            } else if touched && !inside {
                self.touched.remove(&namespace);
            }
            if state.bindings < members.map_or(0, |ids| ids.len()) {
                self.pending.insert(namespace);
            } else {
                self.pending.remove(&namespace);
            }
        }
        if plan.is_empty() {
            counts.empty += 1;
        }
        plan.sort_unstable_by_key(|(position, _)| *position);
        plan.into_iter().map(|(_, planned)| planned).collect()
    }

    /// Whether `tables` is not of the program the engine registered: a
    /// namespace or definition the two share by position has another name.
    fn diverged(&self, tables: &ReaderTables) -> bool {
        let len = tables.namespaces.len().min(self.namespaces.len());
        if !tables
            .namespaces
            .changed_entries(&self.namespaces, len)
            .is_empty()
        {
            return true;
        }
        let len = tables.globals.len().min(self.globals.len());
        tables
            .globals
            .changed_entries(&self.globals, len)
            .into_iter()
            .any(|id| {
                let (a, b) = (&tables.globals[id], &self.globals[id]);
                a.namespace != b.namespace || a.name != b.name
            })
    }
}

/// Whether the engine's reader context `registered` is `context`.
fn same_context(
    registered: &Option<Shared<RuntimeReaderNamespace>>,
    context: Option<&Shared<RuntimeReaderNamespace>>,
) -> bool {
    match (registered, context) {
        (Some(a), Some(b)) => Shared::ptr_eq(a, b) || **a == **b,
        (None, None) => true,
        _ => false,
    }
}

/// How many of `ids`, in increasing order, are below `len`.
fn below(ids: &Arena<GlobalId>, len: usize) -> usize {
    if ids.last().is_none_or(|last| last.0 < len) {
        return ids.len();
    }
    let (mut low, mut high) = (0, ids.len());
    while low < high {
        let middle = (low + high) / 2;
        if ids[middle].0 < len {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    low
}

/// The registration of every namespace of `tables`, as an engine that has
/// none of them registers it.
pub(crate) fn emit_every_namespace(tables: &ReaderTables) -> crate::emitter::NamespaceRegistration {
    let plan = ReaderNamespaces::default().plan(tables, &mut RegistrationCounts::default());
    crate::emitter::emit_reader_registration(tables, &plan)
}

/// Run `f`, a compile-unit callback of `engine` (`__lightCompileUnit`),
/// during which its registrations name each namespace they register fully
/// ([`ReaderNamespaces::plan`]).
pub(crate) fn in_callback<T>(engine: &dyn MacroEvaluator, f: impl FnOnce() -> T) -> T {
    struct Leave(std::rc::Rc<std::cell::RefCell<ReaderNamespaces>>);
    impl Drop for Leave {
        fn drop(&mut self) {
            self.0.borrow_mut().callbacks -= 1;
        }
    }
    let Some(registered) = engine.reader_namespaces() else {
        return f();
    };
    registered.borrow_mut().callbacks += 1;
    let _leave = Leave(registered);
    f()
}

/// Register the namespaces of `tables` with `engine`'s runtime: of an
/// engine that keeps [`ReaderNamespaces`] only what it lacks, otherwise
/// every namespace.
pub(crate) fn register(
    engine: &dyn MacroEvaluator,
    tables: &ReaderTables,
) -> Result<(), EvalError> {
    let Some(registered) = engine.reader_namespaces() else {
        let registration = emit_every_namespace(tables);
        return engine.evaluate_registration(&registration.source, &registration.chunks);
    };
    let registration = {
        let mut counts = RegistrationCounts::default();
        let plan = registered.borrow_mut().plan(tables, &mut counts);
        crate::compile_trace::registered(|total| {
            total.registrations += counts.registrations;
            total.empty += counts.empty;
            total.restarts += counts.restarts;
            total.full += counts.full;
            total.added += counts.added;
        });
        if plan.is_empty() {
            return Ok(());
        }
        crate::emitter::emit_reader_registration(tables, &plan)
    };
    let result = engine.evaluate_registration(&registration.source, &registration.chunks);
    if result.is_err() {
        registered.borrow_mut().forget();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn global(namespace: &str, name: &str) -> GlobalBinding {
        GlobalBinding {
            type_descriptor: false,
            declared: false,
            constant: false,
            define: false,
            boolean_tag: false,
            metadata: None,
            namespace: namespace.into(),
            name: name.into(),
            span: 0..0,
            source_index: 0,
        }
    }

    fn context(macros: &[&str]) -> Shared<RuntimeReaderNamespace> {
        Shared::new(RuntimeReaderNamespace {
            aliases: vec![("d".into(), "dep".into())],
            refers: Vec::new(),
            excludes: Vec::new(),
            macros: macros.iter().map(|m| (*m).into()).collect(),
        })
    }

    /// Each planned namespace, whether it is full, and its definitions.
    fn planned(
        registered: &mut ReaderNamespaces,
        tables: &ReaderTables,
        counts: &mut RegistrationCounts,
    ) -> Vec<(String, bool, Vec<String>)> {
        registered
            .plan(tables, counts)
            .into_iter()
            .map(|planned| {
                let names = planned
                    .ids
                    .iter()
                    .map(|id| tables.globals[id.0].name.clone())
                    .collect();
                (planned.namespace.to_owned(), planned.full, names)
            })
            .collect()
    }

    fn entry(namespace: &str, full: bool, names: &[&str]) -> (String, bool, Vec<String>) {
        (
            namespace.into(),
            full,
            names.iter().map(|name| (*name).into()).collect(),
        )
    }

    #[test]
    fn a_registration_names_only_what_the_engine_lacks() {
        let mut tables = ReaderTables::default();
        tables.namespaces.extend(["a".to_owned(), "b".to_owned()]);
        tables
            .globals
            .extend([global("a", "x"), global("b", "y"), global("a", "z")]);
        tables.reader_contexts.insert("a".into(), context(&[]));
        // A context of no namespace of the program is never registered.
        tables
            .reader_contexts
            .insert("elsewhere".into(), context(&[]));
        let mut registered = ReaderNamespaces::default();
        let mut counts = RegistrationCounts::default();
        assert_eq!(
            planned(&mut registered, &tables, &mut counts),
            [entry("a", true, &["x", "z"]), entry("b", true, &["y"])]
        );
        // The same program again, and a copy of it, register nothing.
        assert!(planned(&mut registered, &tables, &mut counts).is_empty());
        let copy = tables.clone();
        assert!(planned(&mut registered, &copy, &mut counts).is_empty());
        // A definition registers itself only.
        tables.globals.freeze();
        let older = tables.clone();
        tables.globals.push(global("b", "w"));
        assert_eq!(
            planned(&mut registered, &tables, &mut counts),
            [entry("b", false, &["w"])]
        );
        // A changed reader context registers its namespace again, fully,
        // and a new namespace registers itself.
        tables.reader_contexts.insert("a".into(), context(&["m"]));
        tables.namespaces.push("c".into());
        tables.globals.push(global("c", "v"));
        assert_eq!(
            planned(&mut registered, &tables, &mut counts),
            [entry("a", true, &["x", "z"]), entry("c", true, &["v"])]
        );
        // An older snapshot removes no definition, and registers its own
        // reader context of `a`, as every registration did before.
        assert_eq!(
            planned(&mut registered, &older, &mut counts),
            [entry("a", true, &["x", "z"])]
        );
        assert_eq!(
            planned(&mut registered, &tables, &mut counts),
            [entry("a", true, &["x", "z"])]
        );
        assert!(planned(&mut registered, &tables, &mut counts).is_empty());
        assert_eq!(
            counts,
            RegistrationCounts {
                registrations: 8,
                empty: 3,
                restarts: 0,
                full: 6,
                added: 1,
            }
        );
    }

    #[test]
    fn a_namespace_registered_from_an_older_snapshot_catches_up() {
        let mut tables = ReaderTables::default();
        tables.namespaces.push("a".into());
        tables.globals.push(global("a", "x"));
        tables.reader_contexts.insert("a".into(), context(&[]));
        tables.globals.freeze();
        let older = tables.clone();
        tables.globals.push(global("a", "y"));
        tables.reader_contexts.insert("a".into(), context(&["m"]));
        let mut registered = ReaderNamespaces::default();
        let mut counts = RegistrationCounts::default();
        assert_eq!(
            planned(&mut registered, &tables, &mut counts),
            [entry("a", true, &["x", "y"])]
        );
        // The older snapshot's context of `a`, with its definitions.
        assert_eq!(
            planned(&mut registered, &older, &mut counts),
            [entry("a", true, &["x"])]
        );
        // A later program with that context has `y` too, which the engine
        // then lacks, though nothing changed since the last registration.
        let mut later = older.clone();
        later.globals.push(global("a", "y"));
        assert_eq!(
            planned(&mut registered, &later, &mut counts),
            [entry("a", false, &["y"])]
        );
        assert!(planned(&mut registered, &later, &mut counts).is_empty());
    }

    #[test]
    fn a_registration_inside_a_callback_is_full_and_repeated_after_it() {
        let mut tables = ReaderTables::default();
        tables.namespaces.extend(["a".to_owned(), "b".to_owned()]);
        tables.globals.extend([global("a", "x"), global("b", "y")]);
        let mut registered = ReaderNamespaces::default();
        let mut counts = RegistrationCounts::default();
        planned(&mut registered, &tables, &mut counts);
        registered.callbacks += 1;
        tables.globals.push(global("a", "z"));
        assert_eq!(
            planned(&mut registered, &tables, &mut counts),
            [entry("a", true, &["x", "z"])]
        );
        // A macro's view of `a` may have restored its earlier context.
        assert_eq!(
            planned(&mut registered, &tables, &mut counts),
            [entry("a", true, &["x", "z"])]
        );
        registered.callbacks -= 1;
        assert_eq!(
            planned(&mut registered, &tables, &mut counts),
            [entry("a", true, &["x", "z"])]
        );
        assert!(planned(&mut registered, &tables, &mut counts).is_empty());
        // Forgetting keeps the running callbacks.
        registered.callbacks += 1;
        registered.forget();
        assert_eq!(registered.callbacks, 1);
    }

    #[test]
    fn another_program_starts_over() {
        let mut tables = ReaderTables::default();
        tables.namespaces.push("a".into());
        tables.globals.extend([global("a", "x"), global("a", "y")]);
        let mut registered = ReaderNamespaces::default();
        let mut counts = RegistrationCounts::default();
        planned(&mut registered, &tables, &mut counts);
        // A definition's other fields do not make another program.
        let mut same = tables.clone();
        same.globals[0].declared = true;
        assert!(planned(&mut registered, &same, &mut counts).is_empty());
        let mut other = tables.clone();
        other.globals[1].name = "renamed".into();
        assert_eq!(
            planned(&mut registered, &other, &mut counts),
            [entry("a", true, &["x", "renamed"])]
        );
        assert_eq!(counts.restarts, 1);
        // Forgetting starts over too, as a new compilation does.
        registered.forget();
        assert_eq!(
            planned(&mut registered, &other, &mut counts),
            [entry("a", true, &["x", "renamed"])]
        );
    }
}
