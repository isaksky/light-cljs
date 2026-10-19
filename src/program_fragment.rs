//! Append independently analyzed arenas while preserving global identities.

use crate::analyzer::*;
use std::collections::HashMap;

/// How many leading entries of each table a fragment shares with the program
/// it is appended to, at the same IDs. A deferred unit is analyzed from a
/// snapshot of that program, whose tables only grow, so [`append`] copies
/// only the entries after them.
#[derive(Clone, Debug, Default)]
pub(crate) struct SharedPrefix {
    exprs: usize,
    strings: usize,
    named_literals: usize,
    locals: usize,
    recur_targets: usize,
    globals: usize,
    js_globals: usize,
    namespaces: usize,
}

impl SharedPrefix {
    /// How many globals the fragment shares.
    pub(crate) fn globals(&self) -> usize {
        self.globals
    }

    /// How many expressions the fragment shares.
    pub(crate) fn exprs(&self) -> usize {
        self.exprs
    }

    /// Check, in every build, what [`append`] relies on: each shared table is
    /// a prefix of both programs, however much the target grew since the
    /// snapshot (a unit's fragment may be appended after later namespaces'
    /// top-level passes, docs/compiler-concurrency.md, "Stage A"), and the
    /// last shared global names the same binding in both. The full
    /// comparison is a debug assertion.
    fn check(&self, target: &Program, fragment: &Program) {
        let tables = [
            (
                "expressions",
                self.exprs,
                target.exprs.len(),
                fragment.exprs.len(),
            ),
            (
                "strings",
                self.strings,
                target.strings.len(),
                fragment.strings.len(),
            ),
            (
                "named literals",
                self.named_literals,
                target.named_literals.len(),
                fragment.named_literals.len(),
            ),
            (
                "locals",
                self.locals,
                target.locals.len(),
                fragment.locals.len(),
            ),
            (
                "recur targets",
                self.recur_targets,
                target.recur_targets.len(),
                fragment.recur_targets.len(),
            ),
            (
                "globals",
                self.globals,
                target.globals.len(),
                fragment.globals.len(),
            ),
            (
                "host globals",
                self.js_globals,
                target.js_globals.len(),
                fragment.js_globals.len(),
            ),
            (
                "namespaces",
                self.namespaces,
                target.namespaces.len(),
                fragment.namespaces.len(),
            ),
        ];
        for (table, shared, target, fragment) in tables {
            assert!(
                shared <= target && shared <= fragment,
                "a fragment shares {shared} {table} with a program holding {target}, from a snapshot holding {fragment}: it was not analyzed from a prefix of that program"
            );
        }
        if let Some(last) = self.globals.checked_sub(1) {
            let (a, b) = (&target.globals[last], &fragment.globals[last]);
            assert!(
                a.namespace == b.namespace && a.name == b.name,
                "a fragment's shared global {}/{} is {}/{} in its program",
                b.namespace,
                b.name,
                a.namespace,
                a.name
            );
        }
    }

    /// All of `snapshot` at the same IDs, for a fragment appended to another
    /// program analyzed from the same snapshot.
    pub(crate) fn same(snapshot: &Program) -> Self {
        Self {
            exprs: snapshot.exprs.len(),
            strings: snapshot.strings.len(),
            named_literals: snapshot.named_literals.len(),
            locals: snapshot.locals.len(),
            recur_targets: snapshot.recur_targets.len(),
            globals: snapshot.globals.len(),
            js_globals: snapshot.js_globals.len(),
            namespaces: snapshot.namespaces.len(),
        }
    }

    /// An estimate of the bytes `fragment` adds to what it shares, for
    /// `--stats`: each table's entries after the shared ones times the
    /// entry's size, and the added strings' UTF-16 code units. Entries' own
    /// heap data (such as an expression's argument vector) is not counted.
    pub(crate) fn added_bytes(&self, fragment: &Program) -> usize {
        use std::mem::size_of;
        let added = |len: usize, shared: usize, size: usize| len.saturating_sub(shared) * size;
        let strings: usize = fragment
            .strings
            .range(self.strings.min(fragment.strings.len())..fragment.strings.len())
            .map(|string| 2 * string.len())
            .sum();
        added(fragment.exprs.len(), self.exprs, size_of::<Expr>())
            + added(fragment.strings.len(), self.strings, size_of::<Vec<u16>>())
            + strings
            + added(
                fragment.named_literals.len(),
                self.named_literals,
                size_of::<NamedLiteral>(),
            )
            + added(
                fragment.locals.len(),
                self.locals,
                size_of::<LocalBinding>(),
            )
            + added(
                fragment.recur_targets.len(),
                self.recur_targets,
                size_of::<RecurTarget>(),
            )
            + added(
                fragment.globals.len(),
                self.globals,
                size_of::<GlobalBinding>(),
            )
            + added(
                fragment.js_globals.len(),
                self.js_globals,
                size_of::<JsGlobal>(),
            )
            + added(
                fragment.namespaces.len(),
                self.namespaces,
                size_of::<String>(),
            )
    }
}

/// Maps one fragment table's IDs to the target's: the first `shared` keep
/// theirs, and the fragment's own entries take their appended positions from
/// `start`. The default is the identity used by read-only traversals.
#[derive(Clone, Copy, Default)]
struct Ids {
    shared: usize,
    start: usize,
}

impl Ids {
    fn new(shared: usize, start: usize) -> Self {
        Self { shared, start }
    }
    fn map(self, id: usize) -> usize {
        if id < self.shared {
            id
        } else {
            id - self.shared + self.start
        }
    }
}

/// Append an analyzed fragment without changing `target.root` and return its new
/// root ID. Diagnostic spans/source indices retain the fragment's provenance.
/// Existing global IDs remain stable; a fragment definition updates its flags,
/// and supplied metadata replaces metadata on the matching global. Entries in
/// `shared` are the target's own: they keep their target IDs and are not copied.
/// With `own_namespace`, the fragment's reader context of that namespace is
/// the only one it may change (one absent from the target is still added):
/// a unit appended after later namespaces' analyses has a snapshot's view of
/// the others, which they may have refreshed since.
pub(crate) fn append(
    target: &mut Program,
    fragment: &Program,
    shared: &SharedPrefix,
    update_shared_globals: bool,
    own_namespace: Option<&str>,
) -> ExprId {
    shared.check(target, fragment);
    let exprs = Ids::new(shared.exprs, target.exprs.len());
    // Shared globals keep their IDs and, unless the caller wants the
    // fragment's view of them, their live flags and metadata.
    let first_global = if update_shared_globals {
        0
    } else {
        shared.globals
    };
    let mut remap = Remap {
        map_expr: move |id: ExprId| ExprId(exprs.map(id.0)),
        local: Ids::new(shared.locals, target.locals.len()),
        recur: Ids::new(shared.recur_targets, target.recur_targets.len()),
        string: Ids::new(shared.strings, target.strings.len()),
        named: Ids::new(shared.named_literals, target.named_literals.len()),
        first_global,
        globals: Vec::with_capacity(fragment.globals.len() - first_global),
        js: Vec::with_capacity(fragment.js_globals.len()),
        lazy: Vec::with_capacity(fragment.lazy_targets.len()),
    };
    // A lazy target whose site is shared is the target's own; the others are
    // appended after the target's, in fragment order.
    let mut appended_lazy = Vec::new();
    for (index, lazy) in fragment.lazy_targets.iter().enumerate() {
        let site = (remap.map_expr)(lazy.site);
        let existing = (lazy.site.0 < shared.exprs)
            .then(|| target.lazy_targets.iter().position(|t| t.site == site))
            .flatten();
        remap.lazy.push(LazyTargetId(existing.unwrap_or_else(|| {
            appended_lazy.push(index);
            target.lazy_targets.len() + appended_lazy.len() - 1
        })));
    }
    debug_assert!(
        remap.shares_prefix(target, fragment, shared),
        "a fragment's shared entries must be unchanged in its target"
    );
    for namespace in fragment.namespaces.iter().skip(shared.namespaces) {
        if !target.namespaces.contains(namespace) {
            target.namespaces.push(namespace.clone());
        }
    }
    for (namespace, context) in fragment.reader_contexts.iter() {
        let current = target.reader_contexts.get(namespace);
        let owned = own_namespace.is_none_or(|own| own == namespace) || current.is_none();
        if owned && current != Some(context) {
            target
                .reader_contexts
                .insert(namespace.clone(), context.clone());
        }
    }
    target.namespace_entries.extend(
        fragment
            .namespace_entries
            .iter()
            .filter(|(id, _)| id.0 >= shared.exprs)
            .map(|(id, ns)| ((remap.map_expr)(*id), ns.clone())),
    );
    for (index, global) in fragment.globals.iter().enumerate().skip(first_global) {
        let existing = if index < shared.globals {
            Some(index)
        } else {
            target.globals.iter().position(|existing| {
                existing.namespace == global.namespace && existing.name == global.name
            })
        };
        let id = if let Some(index) = existing {
            let existing = &mut target.globals[index];
            if global.declared {
                existing.declared = true;
                existing.type_descriptor = global.type_descriptor;
                existing.constant = global.constant;
                existing.define = global.define;
                existing.boolean_tag = global.boolean_tag;
            }
            if let Some(metadata) = global.metadata {
                existing.metadata = Some((remap.map_expr)(metadata));
            }
            GlobalId(index)
        } else {
            let id = GlobalId(target.globals.len());
            let mut global = global.clone();
            remap.optional(&mut global.metadata);
            target.globals.push(global);
            id
        };
        remap.globals.push(id);
    }
    for (index, global) in fragment.js_globals.iter().enumerate() {
        let existing = if index < shared.js_globals {
            Some(index)
        } else {
            target
                .js_globals
                .iter()
                .position(|existing| existing == global)
        };
        let id = match existing {
            Some(index) => JsGlobalId(index),
            None => {
                let id = JsGlobalId(target.js_globals.len());
                target.js_globals.push(global.clone());
                id
            }
        };
        remap.js.push(id);
    }
    for index in appended_lazy {
        let lazy = fragment.lazy_targets[index];
        let name = &fragment.lazy_namespaces[lazy.namespace];
        let namespace = match target.lazy_namespaces.iter().position(|n| n == name) {
            Some(namespace) => namespace,
            None => {
                target.lazy_namespaces.push(name.clone());
                target.lazy_namespaces.len() - 1
            }
        };
        let mut global = lazy.global;
        if let Some(global) = &mut global {
            remap.global(global);
        }
        target.lazy_targets.push(LazyTarget {
            site: (remap.map_expr)(lazy.site),
            namespace,
            global,
        });
    }
    target
        .strings
        .extend(fragment.strings.tail_from(shared.strings).cloned());
    target.named_literals.extend(
        fragment
            .named_literals
            .tail_from(shared.named_literals)
            .cloned()
            .map(|mut literal| {
                remap.literal(&mut literal);
                literal
            }),
    );
    target
        .locals
        .extend(fragment.locals.tail_from(shared.locals).cloned());
    target.recur_targets.extend(
        fragment
            .recur_targets
            .tail_from(shared.recur_targets)
            .cloned()
            .map(|mut t| {
                remap.locals(&mut t.params);
                t
            }),
    );
    for (global, methods) in &fragment.protocols {
        if global.0 < first_global {
            continue;
        }
        let mut methods = methods.clone();
        for method in &mut methods {
            remap.global(&mut method.global);
        }
        target
            .protocols
            .insert(remap.mapped_global(*global), methods);
    }
    target.record_constructors.extend(
        fragment
            .record_constructors
            .iter()
            .filter(|g| g.0 >= first_global)
            .map(|g| remap.mapped_global(*g)),
    );
    target.field_bindings.extend(
        fragment
            .field_bindings
            .iter()
            .filter(|(local, _)| local.0 >= shared.locals)
            .map(|(local, (target, b))| {
                let (mut local, mut target) = (*local, *target);
                remap.local(&mut local);
                remap.recur(&mut target);
                (local, (target, *b))
            }),
    );
    target.type_methods.extend(
        fragment
            .type_methods
            .iter()
            .filter(|target| target.0 >= shared.recur_targets)
            .map(|target| {
                let mut target = *target;
                remap.recur(&mut target);
                target
            }),
    );
    target.macro_functions.extend(
        fragment
            .macro_functions
            .iter()
            .filter(|e| e.0 >= shared.exprs)
            .map(|e| (remap.map_expr)(*e)),
    );
    target.exprs.extend(
        fragment
            .exprs
            .tail_from(shared.exprs)
            .cloned()
            .map(|mut e| {
                remap.kind(&mut e.kind);
                e
            }),
    );
    (remap.map_expr)(fragment.root)
}

/// A deferred unit's view of `program`: the program itself, frozen so the
/// snapshot shares its tables instead of copying them, and what it shares
/// with `program` for [`append`].
pub(crate) fn resolution_snapshot(program: &mut Program) -> (Program, SharedPrefix) {
    program.freeze();
    let snapshot = program.clone();
    let shared = SharedPrefix::same(&snapshot);
    (snapshot, shared)
}

/// Compact `program` to its resolution environment and metadata, renumbered.
fn compact_resolution_snapshot(program: &Program) -> Program {
    let (mut snapshot, _exprs) = retain(program, None);
    let (strings, named_literals) = compact_literals(program, &mut snapshot);
    // Declaration metadata normally contains only data. Executable metadata
    // retains its local arenas; ordinary snapshots need not copy every prior
    // function's locals and recur tables.
    if !snapshot.exprs.iter().all(|expr| is_data(&expr.kind)) {
        snapshot.locals = program.locals.clone();
        snapshot.recur_targets = program.recur_targets.clone();
        snapshot.field_bindings = program.field_bindings.clone();
        snapshot.type_methods = program.type_methods.clone();
    }
    let _ = (strings, named_literals);
    snapshot.root = ExprId(snapshot.exprs.len());
    snapshot.exprs.push(Expr {
        kind: ExprKind::Nil,
        span: 0..0,
        context: ExprContext::Value,
        source_index: 0,
    });
    snapshot
}

/// Retain a complete analyzed owning form, without expanding its macros again.
pub(crate) fn expression_snapshot(program: &mut Program, root: ExprId) -> Program {
    program.freeze();
    let mut snapshot = program.clone();
    snapshot.root = root;
    snapshot
}

/// Compact a persisted interactive catalog between executed forms. Only global
/// binding IDs survive into the next form; emitted closures already own code.
pub(crate) fn interactive_resolution_snapshot(program: &Program) -> Program {
    let mut snapshot = compact_resolution_snapshot(program);
    snapshot.freeze();
    snapshot
}

/// The interactive snapshot of `program`, a form analyzed from `previous`
/// (an interactive snapshot, which it shares as a prefix), at a cost that
/// depends on what the form added and changed, not on `previous`'s size:
/// `previous`'s entries keep their IDs, and the expressions, strings, named
/// literals and lazy targets the form's new or changed metadata reaches are
/// appended. Unlike [`interactive_resolution_snapshot`], it keeps the
/// expressions that metadata the form replaced reached; the second value
/// counts them, with the previous root, so that the caller can compact
/// once they make up a large share; the third is whether the analysis
/// changed any of `previous`'s expressions in place.
pub(crate) fn extend_interactive_snapshot(
    previous: &Program,
    program: &Program,
) -> (Program, usize, bool) {
    use std::collections::HashSet;
    let old_exprs = previous.exprs.len();
    let old_globals = previous.globals.len();
    // Old entries a write changed: the arenas compare only the leaves they no
    // longer share with `previous`.
    let changed_exprs = program.exprs.changed_entries(&previous.exprs, old_exprs);
    let changed_globals = program
        .globals
        .changed_entries(&previous.globals, old_globals)
        .into_iter()
        .filter(|&i| program.globals[i].metadata != previous.globals[i].metadata)
        .collect::<Vec<_>>();
    // What replaced metadata reached, counted generously (shared trees count
    // once per root), and the previous root.
    let mut released = 1;
    for &i in &changed_globals {
        if let Some(root) = previous.globals[i].metadata {
            let seen = std::cell::RefCell::new(HashSet::new());
            let pending = std::cell::RefCell::new(vec![root]);
            let visitor = identity_remap(previous, |id| {
                pending.borrow_mut().push(id);
                id
            });
            loop {
                let Some(id) = pending.borrow_mut().pop() else {
                    break;
                };
                if seen.borrow_mut().insert(id.0) {
                    visitor.kind(&mut previous.exprs[id.0].kind.clone());
                }
            }
            released += seen.into_inner().len();
        }
    }
    // The new expressions reachable from new or changed metadata and from
    // changed old expressions, in their original order.
    let pending = std::cell::RefCell::new(
        changed_globals
            .iter()
            .copied()
            .chain(old_globals..program.globals.len())
            .filter_map(|i| program.globals[i].metadata)
            .filter(|id| id.0 >= old_exprs)
            .collect::<Vec<_>>(),
    );
    let discover = identity_remap(program, |id| {
        if id.0 >= old_exprs {
            pending.borrow_mut().push(id);
        }
        id
    });
    for &id in &changed_exprs {
        discover.kind(&mut program.exprs[id].kind.clone());
    }
    let mut fresh = HashSet::new();
    loop {
        let Some(id) = pending.borrow_mut().pop() else {
            break;
        };
        if fresh.insert(id.0) {
            discover.kind(&mut program.exprs[id.0].kind.clone());
        }
    }
    let mut origins = fresh.into_iter().collect::<Vec<_>>();
    origins.sort_unstable();
    let ids = origins
        .iter()
        .enumerate()
        .map(|(index, origin)| (*origin, ExprId(old_exprs + index)))
        .collect::<HashMap<_, _>>();
    // Lazy targets past `previous`'s that retained loadables name, appended
    // in order.
    let old_lazy = previous.lazy_targets.len();
    let mut lazy_targets = previous.lazy_targets.clone();
    let mut lazy = Vec::new();
    if program.lazy_targets.len() > old_lazy {
        lazy = (0..program.lazy_targets.len())
            .map(|i| LazyTargetId(if i < old_lazy { i } else { usize::MAX }))
            .collect::<Vec<_>>();
        for origin in changed_exprs.iter().chain(&origins) {
            if let ExprKind::Loadable { target, .. } = program.exprs[*origin].kind
                && lazy[target.0].0 == usize::MAX
            {
                let mut kept = program.lazy_targets[target.0];
                if kept.site.0 >= old_exprs {
                    let own = if *origin < old_exprs {
                        ExprId(*origin)
                    } else {
                        ids[origin]
                    };
                    kept.site = ids.get(&kept.site.0).copied().unwrap_or(own);
                }
                lazy[target.0] = LazyTargetId(lazy_targets.len());
                lazy_targets.push(kept);
            }
        }
    }
    let map = |id: ExprId| {
        if id.0 < old_exprs { id } else { ids[&id.0] }
    };
    let mut remap = identity_remap(program, map);
    remap.lazy = lazy;
    // Strings and named literals past `previous`'s that kept expressions use,
    // appended in first-use order.
    let (old_strings, old_named) = (previous.strings.len(), previous.named_literals.len());
    let mut strings = previous.strings.clone();
    let mut named_literals = previous.named_literals.clone();
    let mut string_ids = HashMap::new();
    let mut named_ids = HashMap::new();
    let mut string = |id: &mut usize, strings: &mut crate::arena::Arena<Vec<u16>>| {
        if *id >= old_strings {
            *id = *string_ids.entry(*id).or_insert_with(|| {
                strings.push(program.strings[*id].clone());
                strings.len() - 1
            });
        }
    };
    let mut literals =
        |kind: &mut ExprKind,
         strings: &mut crate::arena::Arena<Vec<u16>>,
         named_literals: &mut crate::arena::Arena<NamedLiteral>| {
            match kind {
                ExprKind::String(id) | ExprKind::Assert { form: id, .. } => {
                    string(&mut id.0, strings)
                }
                ExprKind::Keyword(id) | ExprKind::Symbol(id) | ExprKind::Var { symbol: id, .. }
                    if id.0 >= old_named =>
                {
                    id.0 = *named_ids.entry(id.0).or_insert_with(|| {
                        let mut literal = program.named_literals[id.0].clone();
                        for id in literal.namespace.iter_mut().chain([&mut literal.name]) {
                            string(&mut id.0, strings);
                        }
                        named_literals.push(literal);
                        named_literals.len() - 1
                    });
                }
                _ => {}
            }
        };
    // `previous`'s expressions, shared, with the ones the analysis changed.
    let mut exprs = previous.exprs.clone();
    let mut data = true;
    for &id in &changed_exprs {
        let expr = exprs.get_mut(id).expect("old expression");
        *expr = program.exprs[id].clone();
        remap.kind(&mut expr.kind);
        literals(&mut expr.kind, &mut strings, &mut named_literals);
        data &= is_data(&expr.kind);
    }
    for origin in &origins {
        let mut expr = program.exprs[*origin].clone();
        remap.kind(&mut expr.kind);
        literals(&mut expr.kind, &mut strings, &mut named_literals);
        data &= is_data(&expr.kind);
        exprs.push(expr);
    }
    let mut globals = program.globals.clone();
    for i in changed_globals
        .into_iter()
        .chain(old_globals..program.globals.len())
    {
        remap.optional(&mut globals.get_mut(i).expect("global").metadata);
    }
    let mut macro_functions = previous.macro_functions.clone();
    let added = origins
        .iter()
        .filter(|origin| program.macro_functions.contains(&ExprId(**origin)))
        .map(|origin| ids[origin])
        .collect::<Vec<_>>();
    if !added.is_empty() {
        macro_functions.extend(added);
    }
    let executable = !data
        || !previous.locals.is_empty()
        || !previous.recur_targets.is_empty()
        || !previous.field_bindings.is_empty()
        || !previous.type_methods.is_empty();
    let root = ExprId(exprs.len());
    exprs.push(Expr {
        kind: ExprKind::Nil,
        span: 0..0,
        context: ExprContext::Value,
        source_index: 0,
    });
    let mut snapshot = Program {
        namespaces: program.namespaces.clone(),
        reader_contexts: program.reader_contexts.clone(),
        namespace_entries: Default::default(),
        compilation_uses: Vec::new(),
        exprs,
        locals: Default::default(),
        recur_targets: Default::default(),
        protocols: program.protocols.clone(),
        record_constructors: program.record_constructors.clone(),
        field_bindings: Default::default(),
        type_methods: Default::default(),
        macro_functions,
        globals,
        js_globals: program.js_globals.clone(),
        strings,
        named_literals,
        lazy_targets,
        lazy_namespaces: program.lazy_namespaces.clone(),
        namespace_requires: program.namespace_requires.clone(),
        namespace_lazy_only: program.namespace_lazy_only.clone(),
        root,
    };
    if executable {
        snapshot.locals = program.locals.clone();
        snapshot.recur_targets = program.recur_targets.clone();
        snapshot.field_bindings = program.field_bindings.clone();
        snapshot.type_methods = program.type_methods.clone();
    }
    snapshot.freeze();
    (snapshot, released, !changed_exprs.is_empty())
}

/// A snapshot of `program`'s resolution tables and of the expressions reachable
/// from global metadata and `root`, in their original order with child IDs
/// renumbered, and the original ID of each retained expression. Strings, named
/// literals, locals and recur targets are left to the caller; without `root`,
/// so is the snapshot's root.
fn retain(program: &Program, root: Option<ExprId>) -> (Program, Vec<usize>) {
    use std::cell::RefCell;

    let pending = RefCell::new(
        program
            .globals
            .iter()
            .filter_map(|global| global.metadata)
            .collect::<Vec<_>>(),
    );
    pending.borrow_mut().extend(root);
    let mut ids = HashMap::new();
    let visitor = identity_remap(program, |id| {
        pending.borrow_mut().push(id);
        id
    });
    loop {
        let Some(id) = pending.borrow_mut().pop() else {
            break;
        };
        if ids.insert(id.0, id).is_some() {
            continue;
        }
        // The same exhaustive traversal used by append discovers every child,
        // including nested metadata and function bodies if metadata contains one.
        visitor.kind(&mut program.exprs[id.0].kind.clone());
    }
    let mut origins = ids.keys().copied().collect::<Vec<_>>();
    origins.sort_unstable();
    for (index, origin) in origins.iter().enumerate() {
        ids.insert(*origin, ExprId(index));
    }
    // The lazy targets the retained loadables name, renumbered in order.
    let mut lazy = vec![LazyTargetId(usize::MAX); program.lazy_targets.len()];
    let mut lazy_targets = Vec::new();
    for origin in &origins {
        if let ExprKind::Loadable { target, .. } = program.exprs[*origin].kind
            && lazy[target.0].0 == usize::MAX
        {
            let mut kept = program.lazy_targets[target.0];
            kept.site = ids.get(&kept.site.0).copied().unwrap_or(ids[origin]);
            lazy[target.0] = LazyTargetId(lazy_targets.len());
            lazy_targets.push(kept);
        }
    }
    let mut remap = identity_remap(program, |id| ids[&id.0]);
    remap.lazy = lazy;
    let exprs = origins
        .iter()
        .map(|origin| {
            let mut expr = program.exprs[*origin].clone();
            remap.kind(&mut expr.kind);
            expr
        })
        .collect();
    let mut globals = program.globals.to_vec();
    for global in &mut globals {
        remap.optional(&mut global.metadata);
    }
    let snapshot = Program {
        namespaces: program.namespaces.clone(),
        reader_contexts: program.reader_contexts.clone(),
        namespace_entries: Default::default(),
        compilation_uses: Vec::new(),
        exprs,
        locals: Default::default(),
        recur_targets: Default::default(),
        protocols: program.protocols.clone(),
        record_constructors: program.record_constructors.clone(),
        field_bindings: Default::default(),
        type_methods: Default::default(),
        macro_functions: program
            .macro_functions
            .iter()
            .filter_map(|id| ids.get(&id.0).copied())
            .collect(),
        globals: globals.into(),
        js_globals: program.js_globals.clone(),
        strings: Default::default(),
        named_literals: Default::default(),
        lazy_targets: lazy_targets.into(),
        lazy_namespaces: program.lazy_namespaces.clone(),
        namespace_requires: program.namespace_requires.clone(),
        namespace_lazy_only: program.namespace_lazy_only.clone(),
        root: root.map_or(ExprId(0), |root| ids[&root.0]),
    };
    (snapshot, origins)
}

/// Fill `snapshot`'s strings and named literals with the ones its expressions
/// use, renumbered, and return the original ID of each.
fn compact_literals(program: &Program, snapshot: &mut Program) -> (Vec<usize>, Vec<usize>) {
    fn renumber(id: &mut usize, ids: &mut HashMap<usize, usize>, origins: &mut Vec<usize>) {
        *id = *ids.entry(*id).or_insert_with(|| {
            origins.push(*id);
            origins.len() - 1
        });
    }
    let (mut string_ids, mut strings) = (HashMap::new(), Vec::new());
    let (mut named_ids, mut named) = (HashMap::new(), Vec::new());
    for expr in &mut snapshot.exprs {
        match &mut expr.kind {
            ExprKind::String(id) | ExprKind::Assert { form: id, .. } => {
                renumber(&mut id.0, &mut string_ids, &mut strings)
            }
            ExprKind::Keyword(id) | ExprKind::Symbol(id) | ExprKind::Var { symbol: id, .. } => {
                renumber(&mut id.0, &mut named_ids, &mut named)
            }
            _ => {}
        }
    }
    snapshot.named_literals = named
        .iter()
        .map(|origin| {
            let mut literal = program.named_literals[*origin].clone();
            for id in literal.namespace.iter_mut().chain([&mut literal.name]) {
                renumber(&mut id.0, &mut string_ids, &mut strings);
            }
            literal
        })
        .collect();
    snapshot.strings = strings
        .iter()
        .map(|origin| program.strings[*origin].clone())
        .collect();
    (strings, named)
}

/// Whether an expression is plain data, which owns no locals or recur targets.
fn is_data(kind: &ExprKind) -> bool {
    matches!(
        kind,
        ExprKind::Nil
            | ExprKind::Bool(_)
            | ExprKind::Number(_)
            | ExprKind::String(_)
            | ExprKind::ReaderLiteral { .. }
            | ExprKind::WithMeta { .. }
            | ExprKind::Keyword(_)
            | ExprKind::Symbol(_)
            | ExprKind::List(_)
            | ExprKind::Vector(_)
            | ExprKind::Map(_)
            | ExprKind::Set(_)
            | ExprKind::LiteralMap(_)
            | ExprKind::LiteralSet(_)
            | ExprKind::CoreFn(_)
            | ExprKind::Global(_)
            | ExprKind::JsGlobal(_)
            | ExprKind::Var { .. }
    )
}

/// Whether evaluating `id` may `await`: it holds an `await` outside the
/// functions it holds. Analysis allows one only in an `^:async` method's
/// body, and no other method can hold one.
pub(crate) fn awaits(program: &Program, id: ExprId) -> bool {
    let mut pending = vec![id];
    while let Some(id) = pending.pop() {
        match program.exprs[id.0].kind {
            ExprKind::Await(_) => return true,
            ExprKind::Fn { .. } => (),
            _ => pending.extend(children(program, id)),
        }
    }
    false
}

/// Visit semantic child IDs using the same exhaustive mapping as fragment linking.
pub(crate) fn children(program: &Program, id: ExprId) -> Vec<ExprId> {
    kind_children(program, &program.exprs[id.0].kind)
}

/// The child expressions of an expression of this kind, in [`children`]
/// order, for passes that hold a kind outside the arena while they rewrite it.
pub(crate) fn kind_children(program: &Program, kind: &ExprKind) -> Vec<ExprId> {
    let children = std::cell::RefCell::new(Vec::new());
    identity_remap(program, |child| {
        children.borrow_mut().push(child);
        child
    })
    .kind(&mut kind.clone());
    children.into_inner()
}

fn identity_remap<F: Fn(ExprId) -> ExprId>(_program: &Program, map_expr: F) -> Remap<F> {
    Remap {
        map_expr,
        local: Ids::default(),
        recur: Ids::default(),
        string: Ids::default(),
        named: Ids::default(),
        first_global: 0,
        globals: Vec::new(),
        js: Vec::new(),
        lazy: Vec::new(),
    }
}

struct Remap<F> {
    map_expr: F,
    local: Ids,
    recur: Ids,
    string: Ids,
    named: Ids,
    /// The fragment's first global that `globals` maps; the ones before it
    /// keep their IDs.
    first_global: usize,
    /// An empty table is the identity mapping used by read-only traversals.
    globals: Vec<GlobalId>,
    js: Vec<JsGlobalId>,
    /// [`Program::lazy_targets`] entries; empty is the identity, as above.
    lazy: Vec<LazyTargetId>,
}

impl<F: Fn(ExprId) -> ExprId> Remap<F> {
    /// Whether `fragment`'s shared entries are still the target's, as when
    /// both were analyzed from or snapshotted at the same program state.
    /// Expressions and literals compare after mapping their IDs; globals
    /// compare by name, since a later definition may update their flags.
    fn shares_prefix(&self, target: &Program, fragment: &Program, shared: &SharedPrefix) -> bool {
        // Shared expressions are checked by ID bounds only: the target may have
        // changed some since the fragment's snapshot was taken, such as a
        // deferred initializer's placeholder it has since filled in, or a
        // declaration's metadata it has since extended; the snapshot keeps its
        // own copy of those.
        let exprs = target.exprs.len() >= shared.exprs
            && (0..shared.exprs).all(|id| (self.map_expr)(ExprId(id)).0 < target.exprs.len());
        let strings = fragment
            .strings
            .shares_prefix(&target.strings, shared.strings)
            || (0..shared.strings)
                .all(|id| target.strings.get(self.string.map(id)) == Some(&fragment.strings[id]));
        let named = fragment
            .named_literals
            .shares_prefix(&target.named_literals, shared.named_literals)
            || (0..shared.named_literals).all(|id| {
                let mut literal = fragment.named_literals[id].clone();
                self.literal(&mut literal);
                target.named_literals.get(self.named.map(id)) == Some(&literal)
            });
        let locals = fragment.locals.shares_prefix(&target.locals, shared.locals)
            || fragment
                .locals
                .range(0..shared.locals)
                .enumerate()
                .all(|(id, local)| target.locals.get(self.local.map(id)) == Some(local));
        // Recur targets are marked used and given argument slots in place, so
        // they are checked by ID bounds as well.
        let recur_targets = target.recur_targets.len() >= shared.recur_targets
            && (0..shared.recur_targets).all(|id| self.recur.map(id) < target.recur_targets.len());
        let globals = target.globals.len() >= shared.globals
            && (fragment
                .globals
                .shares_prefix(&target.globals, shared.globals)
                || fragment
                    .globals
                    .range(0..shared.globals)
                    .zip(target.globals.iter())
                    .all(|(a, b)| a.namespace == b.namespace && a.name == b.name));
        let js_globals = target.js_globals.len() >= shared.js_globals
            && target
                .js_globals
                .range(0..shared.js_globals)
                .eq(fragment.js_globals.range(0..shared.js_globals));
        exprs && strings && named && locals && recur_targets && globals && js_globals
    }
    fn expr(&self, id: &mut ExprId) {
        *id = (self.map_expr)(*id);
    }
    fn local(&self, id: &mut LocalId) {
        id.0 = self.local.map(id.0);
    }
    fn recur(&self, id: &mut RecurTargetId) {
        id.0 = self.recur.map(id.0);
    }
    fn string(&self, id: &mut StringId) {
        id.0 = self.string.map(id.0);
    }
    fn named(&self, id: &mut NamedLiteralId) {
        id.0 = self.named.map(id.0);
    }
    fn literal(&self, literal: &mut NamedLiteral) {
        if let Some(namespace) = &mut literal.namespace {
            self.string(namespace);
        }
        self.string(&mut literal.name);
    }
    fn mapped_global(&self, id: GlobalId) -> GlobalId {
        if self.globals.is_empty() || id.0 < self.first_global {
            id
        } else {
            self.globals[id.0 - self.first_global]
        }
    }
    fn global(&self, id: &mut GlobalId) {
        *id = self.mapped_global(*id);
    }
    fn exprs(&self, ids: &mut [ExprId]) {
        for id in ids {
            self.expr(id);
        }
    }
    fn locals(&self, ids: &mut [LocalId]) {
        for id in ids {
            self.local(id);
        }
    }
    fn optional(&self, id: &mut Option<ExprId>) {
        if let Some(id) = id {
            self.expr(id);
        }
    }
    fn bindings(&self, bindings: &mut [(LocalId, ExprId)]) {
        for (local, expr) in bindings {
            self.local(local);
            self.expr(expr);
        }
    }
    fn pairs(&self, pairs: &mut [(ExprId, ExprId)]) {
        for (a, b) in pairs {
            self.expr(a);
            self.expr(b);
        }
    }
    fn method(&self, method: &mut FnMethod) {
        self.locals(&mut method.params);
        if let Some(rest) = &mut method.rest {
            self.local(rest);
        }
        self.expr(&mut method.body);
        self.recur(&mut method.target);
        self.locals(&mut method.captures);
    }
    fn methods(&self, methods: &mut [(MethodDispatch, FnMethod)]) {
        for (_, method) in methods {
            self.method(method);
        }
    }
    fn kind(&self, kind: &mut ExprKind) {
        use ExprKind::*;
        match kind {
            Nil | Bool(_) | Number(_) | CoreFn(_) | JsArguments | LoadableType => {}
            String(id) => self.string(id),
            Keyword(id) | Symbol(id) => self.named(id),
            Local(id) => self.local(id),
            Global(id) => self.global(id),
            JsGlobal(id) => {
                if !self.js.is_empty() {
                    *id = self.js[id.0];
                }
            }
            Loadable { target, class } => {
                if !self.lazy.is_empty() {
                    *target = self.lazy[target.0];
                }
                self.expr(class);
            }
            ReaderLiteral { items, .. }
            | List(items)
            | Vector(items)
            | Map(items)
            | Set(items)
            | LiteralMap(items)
            | LiteralSet(items)
            | Do(items) => self.exprs(items),
            DefinitionTest { value, .. } => self.expr(value),
            WithMeta { value, metadata } => {
                self.expr(value);
                self.expr(metadata);
            }
            Var {
                value,
                symbol,
                metadata,
                ..
            } => {
                self.expr(value);
                self.named(symbol);
                self.optional(metadata);
            }
            ThisAs { binding, body } => {
                self.local(binding);
                self.expr(body);
            }
            JsTemplate { args, .. } => self.exprs(args),
            JsIndex { receiver, indices } => {
                self.expr(receiver);
                self.exprs(indices);
            }
            JsDelete { receiver, key } => {
                self.expr(receiver);
                self.expr(key);
            }
            QualifiedMethod { class, .. } => self.expr(class),
            CopyArguments(id) | WithOutStr(id) | Exists(id) | BindingExists(id)
            | BooleanHint(id) | Throw(id) | Await(id) => self.expr(id),
            DefProtocol {
                global, methods, ..
            } => {
                self.global(global);
                for method in methods {
                    self.global(&mut method.global);
                }
            }
            ProtocolPredicate {
                protocol, value, ..
            } => {
                self.expr(protocol);
                self.expr(value);
            }
            ExtendType {
                target, methods, ..
            } => {
                self.expr(target);
                self.methods(methods);
            }
            Reify {
                metadata,
                captures,
                methods,
            } => {
                self.optional(metadata);
                self.locals(captures);
                self.methods(methods);
            }
            DefType {
                global,
                layout,
                methods,
                ..
            } => {
                self.global(global);
                if let Some(id) = &mut layout.constructor {
                    self.global(id);
                }
                if let Some(id) = &mut layout.map_constructor {
                    self.global(id);
                }
                self.optional(&mut layout.body);
                self.optional(&mut layout.masks);
                self.methods(methods);
            }
            Construct { callee, args } | Call { callee, args } => {
                self.expr(callee);
                self.exprs(args);
            }
            Assert {
                test,
                message,
                form,
            } => {
                self.expr(test);
                self.optional(message);
                self.string(form);
            }
            Def {
                global, init, test, ..
            } => {
                self.global(global);
                self.optional(init);
                self.optional(test);
            }
            SetGlobal { target, value } => {
                self.expr(target);
                self.expr(value);
            }
            Binding { bindings, body } => {
                self.pairs(bindings);
                self.expr(body);
            }
            Property { receiver, .. } => self.expr(receiver),
            MethodCall { receiver, args, .. } => {
                self.expr(receiver);
                self.exprs(args);
            }
            If { test, yes, no } => {
                self.expr(test);
                self.expr(yes);
                self.expr(no);
            }
            Let { bindings, body } | LetFn { bindings, body } => {
                self.bindings(bindings);
                self.expr(body);
            }
            Loop {
                target,
                bindings,
                body,
            } => {
                self.recur(target);
                self.bindings(bindings);
                self.expr(body);
            }
            Recur { target, args } => {
                self.recur(target);
                self.exprs(args);
            }
            Try {
                body,
                catches,
                finally,
            } => {
                self.expr(body);
                for catch in catches {
                    self.optional(&mut catch.class);
                    self.local(&mut catch.binding);
                    self.expr(&mut catch.body);
                }
                self.optional(finally);
            }
            Case {
                test,
                branches,
                default,
                ..
            } => {
                self.expr(test);
                self.pairs(branches);
                self.optional(default);
            }
            Fn { name, methods } => {
                if let Some(name) = name {
                    self.local(name);
                }
                for method in methods {
                    self.method(method);
                }
            }
            InstanceOf { class, value } => {
                self.expr(class);
                self.expr(value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analyze(source: &str) -> Program {
        crate::analyze(source, &crate::parse(source).unwrap()).unwrap()
    }

    #[test]
    fn appended_fragments_execute_with_shared_globals_and_distinct_arenas() {
        let mut target = analyze("(def shared (js/Math.max 0 10)) (let [x \"original\"] x)");
        let fragment = analyze(
            "(def shared 40) (def answer (loop [n 2 v shared] (if (zero? n) v (recur (dec n) (inc v))))) (pr-str [(js/Math.abs (- answer)) :sample/value \"fragment\"])",
        );
        let original_root = target.root;
        let original_global = target
            .globals
            .iter()
            .position(|g| g.name == "shared")
            .unwrap();
        let root = append(&mut target, &fragment, &SharedPrefix::default(), true, None);
        assert_eq!(target.root, original_root);
        assert_eq!(
            target.globals.iter().filter(|g| g.name == "shared").count(),
            1
        );
        assert_eq!(target.globals[original_global].name, "shared");
        target.exprs.push(Expr {
            kind: ExprKind::Do(vec![original_root, root]),
            span: 0..0,
            context: ExprContext::Value,
            source_index: 0,
        });
        target.root = ExprId(target.exprs.len() - 1);
        let output =
            std::process::Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
                .args([
                    "--eval",
                    &format!("console.log({});", crate::emit_js(&target)),
                ])
                .output()
                .expect("fragment execution requires Node");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert_eq!(stdout, "[42 :sample/value \"fragment\"]\n");
    }

    /// A unit's fragment appended after the target grew past its snapshot,
    /// as when a later namespace's top-level pass runs before an earlier
    /// namespace's units merge (docs/compiler-concurrency.md, "Stage A"): its
    /// IDs move past the target's newer entries, a global the body declares
    /// merges with the same-named one the target reserved meanwhile, and
    /// what the target changed on a shared global since is kept.
    #[test]
    fn a_fragment_appends_after_its_target_grew_past_the_snapshot() {
        let mut target = analyze("(def ^{:doc \"x\"} x 1)");
        let (snapshot, shared) = resolution_snapshot(&mut target);
        // The unit's analysis: its snapshot and what its body adds.
        let mut fragment = snapshot;
        fragment.root = append(
            &mut fragment,
            &analyze("(do (def later 1) (str later \"-body\"))"),
            &SharedPrefix::default(),
            true,
            None,
        );
        // The later namespace declares the same Var, adds its own entries
        // to every table, and drops the shared global's metadata.
        let grown = append(
            &mut target,
            &analyze("(def later 2) (let [n \"next\"] n)"),
            &SharedPrefix::default(),
            true,
            None,
        );
        target.globals[0].metadata = None;
        let (globals, strings, locals) = (
            target.globals.len(),
            target.strings.len(),
            target.locals.len(),
        );
        let root = append(&mut target, &fragment, &shared, false, None);
        assert_eq!(target.globals.len(), globals, "`later` merges");
        assert_eq!(
            target.globals[0].metadata, None,
            "the target's change stays"
        );
        assert!(target.strings.len() > strings && target.locals.len() == locals);
        target.exprs.push(Expr {
            kind: ExprKind::Do(vec![grown, root]),
            span: 0..0,
            context: ExprContext::Value,
            source_index: 0,
        });
        target.root = ExprId(target.exprs.len() - 1);
        let runtime = rquickjs::Runtime::new().unwrap();
        let context = rquickjs::Context::full(&runtime).unwrap();
        context.with(|ctx| {
            let value: String = ctx.eval(crate::emit_js(&target)).unwrap();
            assert_eq!(value, "1-body");
        });
    }

    #[test]
    #[should_panic(expected = "was not analyzed from a prefix of that program")]
    fn a_fragment_of_another_program_is_refused() {
        let mut target = analyze("(def x 1) (def y 2) (def z 3)");
        let (snapshot, shared) = resolution_snapshot(&mut target);
        append(&mut analyze("1"), &snapshot, &shared, false, None);
    }

    #[test]
    fn metadata_is_remapped_and_absent_metadata_preserves_existing_value() {
        let mut target = analyze("(def ^{:doc \"old\"} shared 1)");
        let fragment = analyze("(def ^{:doc \"new\"} shared 2)");
        let offset = target.exprs.len();
        let metadata = fragment.globals[0].metadata.unwrap();
        append(&mut target, &fragment, &SharedPrefix::default(), true, None);
        assert_eq!(
            target.globals[0].metadata,
            Some(ExprId(metadata.0 + offset))
        );
        let preserved = target.globals[0].metadata;
        let mut fragment = analyze("(def shared 3)");
        fragment.globals[0].metadata = None;
        append(&mut target, &fragment, &SharedPrefix::default(), true, None);
        assert_eq!(target.globals[0].metadata, preserved);
    }
    #[test]
    fn resolution_snapshot_retains_metadata_but_excludes_initializers() {
        let program = analyze(
            "(def ^{:doc \"kept\" :dynamic true} shared (let [x 1] (+ x 2))) (defprotocol P (f [this])) (defrecord R [x])",
        );
        let snapshot = compact_resolution_snapshot(&program);
        assert_eq!(snapshot.protocols, program.protocols);
        assert_eq!(snapshot.record_constructors, program.record_constructors);
        assert_eq!(snapshot.globals.len(), program.globals.len());
        assert!(matches!(
            snapshot.exprs[snapshot.root.0].kind,
            ExprKind::Nil
        ));
        assert!(snapshot.exprs.len() < program.exprs.len());
        assert!(!snapshot.exprs.iter().any(|expr| matches!(
            expr.kind,
            ExprKind::Def { .. }
                | ExprKind::Let { .. }
                | ExprKind::DefProtocol { .. }
                | ExprKind::DefType { .. }
        )));
        let old_metadata = program.globals[0].metadata.unwrap();
        let new_metadata = snapshot.globals[0].metadata.unwrap();
        let mut original = program.clone();
        original.root = old_metadata;
        let mut copied = snapshot.clone();
        copied.root = new_metadata;
        let runtime = rquickjs::Runtime::new().unwrap();
        let context = rquickjs::Context::full(&runtime).unwrap();
        context.with(|ctx| {
            let old: String = ctx
                .eval(format!("JSON.stringify({})", crate::emit_js(&original)))
                .unwrap();
            let new: String = ctx
                .eval(format!("JSON.stringify({})", crate::emit_js(&copied)))
                .unwrap();
            assert_eq!(old, new);
        });
    }

    /// Each top-level definition is a deferred unit analyzed from a snapshot of
    /// the program so far, and appending its analysis adds only what the unit
    /// added. So every further definition grows each table by the same amount,
    /// within a namespace and across namespaces (light-cljs-225: each unit
    /// re-appended its whole snapshot, and the tables grew quadratically).
    #[test]
    fn deferred_definitions_append_only_what_they_add() {
        fn definitions(namespace: &str, count: usize) -> String {
            let mut source = format!("(ns {namespace}) (def k (case 1 1 2 3))");
            for i in 0..count {
                source.push_str(&format!(
                    " (def ^{{:doc \"x\"}} x{i} {{:i {i}}}) (def v{i} (var x{i}))
                      (defn f{i} [a] (let [b (+ a (:i x{i}))] (loop [c b] (if (pos? c) (recur (dec c)) c))))"
                ));
            }
            source
        }
        let sizes = |count| {
            let sources = [definitions("a", count), definitions("b", count)];
            let syntax = sources
                .each_ref()
                .map(|source| crate::parse(source).unwrap());
            let program = crate::analyze_namespaces(&[
                NamespaceInput {
                    name: "a",
                    source: &sources[0],
                    syntax: &syntax[0],
                    aliases: &[],
                    requires: &[],
                },
                NamespaceInput {
                    name: "b",
                    source: &sources[1],
                    syntax: &syntax[1],
                    aliases: &[],
                    requires: &["a".into()],
                },
            ])
            .unwrap();
            [
                program.exprs.len(),
                program.strings.len(),
                program.named_literals.len(),
                program.locals.len(),
                program.recur_targets.len(),
            ]
        };
        let [ten, twenty, forty] = [10, 20, 40].map(sizes);
        let tables = [
            "exprs",
            "strings",
            "named literals",
            "locals",
            "recur targets",
        ];
        for (index, table) in tables.iter().enumerate() {
            assert_eq!(
                forty[index] - twenty[index],
                2 * (twenty[index] - ten[index]),
                "{table} for 10, 20 and 40 definitions per namespace: {} {} {}",
                ten[index],
                twenty[index],
                forty[index]
            );
        }
    }

    /// A unit's references to snapshot metadata resolve to the metadata the
    /// target already holds, and its own code and literals are appended.
    #[test]
    fn deferred_definitions_share_snapshot_metadata() {
        let program = analyze(
            "(def ^{:doc \"kept\"} x 1) (def k (case 1 1 2 3)) (def y (str (:doc (meta (var x))) \"-\" k)) y",
        );
        let runtime = rquickjs::Runtime::new().unwrap();
        let context = rquickjs::Context::full(&runtime).unwrap();
        context.with(|ctx| {
            let value: String = ctx.eval(crate::emit_js(&program)).unwrap();
            assert_eq!(value, "kept-2");
        });
    }
    #[test]
    fn deferred_prefix_allows_patched_initializers() {
        // Draining the first definition fills in the placeholder its snapshot
        // left for it, which the later definitions' snapshots still hold as it
        // was; appending them accepts that, with a declaration bundle between.
        let program =
            analyze("(def a 1) (defprotocol P (m [this])) (def b (fn [] (m a))) (def c b)");
        assert_eq!(
            program
                .globals
                .iter()
                .filter(|g| ["a", "b", "c"].contains(&g.name.as_str()))
                .count(),
            3
        );
    }
}
