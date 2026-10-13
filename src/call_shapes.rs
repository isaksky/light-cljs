//! Call shapes: Phase 2 step 5 of docs/advanced-optimization.md
//! (light-cljs-212, and light-cljs-228 for local and named functions), at
//! `Advanced` only.
//!
//! Below `Advanced` every call of a program function is `(f).call(null, a)`,
//! and a function with several methods or rest arguments is an arrow IIFE that
//! builds an `arguments.length` dispatcher and its arity properties. [`analyze`]
//! finds the calls whose target is known at compile time, so emission can call
//! the method itself, and the functions whose value nothing else can see, so
//! emission can leave their dispatcher out.
//!
//! # Known functions
//!
//! A global's function is *known* when the program defines the global once,
//! with a `def` whose initializer is a `fn` (possibly in a one-form `do`),
//! and never assigns it otherwise: no second `def` (a `def` without a value,
//! such as `declare`, assigns nothing and does not count), `defonce`, `set!`,
//! `binding`, `with-redefs` or `js*` hole, no `:dynamic` metadata, no `:test`
//! metadata, and no name with a dot (a host path). Macro functions are left
//! alone. A named function with a dispatcher is known only when its `def` is a
//! top-level form and its name is read only as the callee of calls whose
//! argument count selects a method (light-cljs-228): the name is the function
//! of one evaluation, which the shared entries below are only when the `def`
//! runs once, and a method that reads the name as a value needs the
//! dispatcher.
//!
//! A known function *escapes* when its value can reach code the analysis does
//! not see: a read of the global anywhere but as the callee of a call (`apply`,
//! `var`, `exists?`, a higher-order argument), the value of its `def` being
//! used, an explicit export, or a call whose argument count selects no method
//! at compile time (a count with no fixed method below the variadic method's
//! fixed parameters, or no method at all), which must go through the
//! dispatcher to throw or fill in its arguments.
//!
//! # Shapes
//!
//! - A function with one fixed method is the method itself ([`FunctionShape::
//!   Single`]); every call of the global is the plain call `g3(a, b)`.
//! - Any other known function binds each method to its own top-level entry,
//!   `g3_0`, `g3_1`, ... (by method index), at its `def`
//!   ([`FunctionShape::Entries`]). Calls choose the method at compile time:
//!   `g3_1(a)`, or for a variadic method `g3_2(a, b, rt$fnRest([c, d], 0))`
//!   (`null` without rest arguments), exactly the values its dispatcher would
//!   pass. Calls through the name of a named function are calls of the same
//!   entries. The global itself, its dispatcher and its arity properties exist
//!   only when the function escapes. The dispatcher of a top-level `def`, which
//!   runs once, calls the shared entries; one of a `def` that may run again
//!   holds the entries of its own evaluation (`rerun`), since the next
//!   evaluation rebinds them along with the global, and direct calls read the
//!   current ones as calls of the global did.
//! - Calls of a local bound to a `fn` with one fixed method, or of such a `fn`
//!   literal, which inference knows
//!   ([`crate::inference::Inference::callee`]), are plain calls too, when the
//!   callee cannot be nil: calling nil keeps `.call(null, ...)` and its
//!   `TypeError`.
//!
//! # Local functions
//!
//! A `fn` with several methods or rest arguments bound by `let` or `letfn`
//! ([`LocalFunction`], light-cljs-228) has no dispatcher when its value goes
//! only to that binding (through one-form `do`s, each with one parent), and
//! its local and its name are read only as the callee of calls whose argument
//! count selects a method (as for a global): not captured by a `reify`, whose
//! fields expose every visible local, nor by a method of a type, `reify` or
//! `extend-type`, whose `IFn` adapter may copy its captures into an object.
//! The calls name the entry of the method they select. Each method is bound
//! where its captures permit ([`Placement`]):
//!
//! - a method that reads no local from outside itself, except as the callee
//!   of a call of another hoisted method (including its own), and holds no
//!   `js*` and no expression with two parents, is hoisted to a function entry
//!   of the `Advanced` constant table, created once when the output loads
//!   ([`Entry::Hoisted`]). Its function value never escapes, so nothing can
//!   tell one closure from many;
//! - any other method is a `const` of its own next to the binding,
//!   `const l5_0 = function (l6) {...};` ([`Entry::Scoped`]), created once per
//!   evaluation of the scope, as the dispatcher was;
//! - a method that no call selects is not emitted.
//!
//! Locals bound by `loop` (which `recur` rebinds), functions whose local or
//! name escapes and functions called with a count that selects no method keep
//! the dispatcher. So do `fn` literals called in place.
//!
//! # Soundness
//!
//! A direct call passes the arguments the dispatcher would pass to the same
//! method, so the method's `arguments`, parameters and `recur` behave alike.
//! Calls read the bindings at call time, as before, so a call that runs before
//! the `def` still throws a `TypeError`, whose engine message now names the
//! output binding instead of `call` (see "Advanced-mode contract"). A plain
//! call passes `undefined` as `this` where `.call(null, ...)` passed `null`,
//! which only strict-mode raw JavaScript can tell apart (`this-as` maps both to
//! the global object), so a call of a method whose body holds `js*` outside a
//! nested `fn` keeps `.call(null, ...)` on its entry.
//!
//! A local function's calls are in the scope of its binding, which `let` and
//! `letfn` evaluate before their bodies (`letfn` initializers are `fn`
//! expressions, which call nothing), so no call of a local entry runs before
//! the entry is bound.
//!
//! # For parameter types
//!
//! [`CallShapes::closed`] says when the direct calls are every call of a
//! function: then each value its parameters receive is an argument of one of
//! [`CallShapes::calls_of`].
//!
//! Inference joins those arguments into the parameters (light-cljs-229), so it
//! needs the calls before it runs, while [`analyze`] reads inference for the
//! calls of locals. [`closed_functions`] is the part that needs no inference:
//! the same escape analysis, run before inference over the program that rapid
//! type analysis then rewrites, extended to `fn` expressions whose value only
//! a local holds or a call receives. A `fn` is *closed* when every place its
//! value goes calls it:
//!
//! - it is the function of a known global that does not escape;
//! - it is bound by `let` or `letfn` to a local that is read only as the
//!   callee of calls (a `reify` in its scope reads it, since its object holds
//!   every visible local);
//! - it is the callee of a call (`((fn [x] ...) 1)`);
//!
//! and, for a named `fn`, its name is read only as the callee of calls. The
//! `fn` expression and the one-form `do`s around it must have one parent each,
//! since an expression reached twice is evaluated twice. Every call of a
//! closed `fn` is then one of [`ClosedFunctions::call`], each running the
//! method its argument count selects, as the dispatcher would (a count that
//! selects none throws before any method runs). Rapid type analysis only
//! removes code, so the calls of a function closed before it are a superset
//! of its calls after it: joining over them stays sound, and a function that
//! only those removals close keeps `any` parameters.
use crate::analyzer::{ExprId, ExprKind, FnMethod, GlobalId, LocalId, Program};
use crate::core_operations::Type;
use crate::inference::Inference;
use crate::program_fragment::children;

/// How the definition and the calls of one global are emitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FunctionShape {
    /// As below `Advanced`.
    Generic,
    /// The global's sole definition binds the `fn` expression `function`,
    /// which has one fixed method; its calls are plain calls of the global.
    /// `closed` when the function does not escape.
    Single { function: ExprId, closed: bool },
    /// The global's sole definition binds each method of the `fn` expression
    /// `function` to its own entry, and the global to the dispatcher only
    /// when `dispatcher` (the function escapes). With `rerun` the definition
    /// may run more than once, so its dispatcher holds the entries of its own
    /// evaluation instead of reading their shared bindings.
    Entries {
        function: ExprId,
        dispatcher: bool,
        rerun: bool,
    },
}

impl FunctionShape {
    /// The `fn` expression of a known function.
    pub fn function(self) -> Option<ExprId> {
        match self {
            Self::Generic => None,
            Self::Single { function, .. } | Self::Entries { function, .. } => Some(function),
        }
    }
}

/// The binding of the method entry that a direct call names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    /// `g<global>_<method>`, bound at the definition of a global of
    /// [`FunctionShape::Entries`].
    Global { global: GlobalId, method: usize },
    /// `l<local>_<method>`, a `const` next to the `let` or `letfn` binding of
    /// the [`LocalFunction`] `local` ([`Placement::Scoped`]).
    Scoped { local: LocalId, method: usize },
    /// A function entry of the `Advanced` constant table, `k<n>`, which holds
    /// method `method` of the `fn` expression `function`
    /// ([`Placement::Hoisted`]).
    Hoisted { function: ExprId, method: usize },
}

/// Where a method of a [`LocalFunction`] is bound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// To a function entry of the constant table, created once: the method
    /// captures nothing but other hoisted methods.
    Hoisted,
    /// To a `const` next to the binding, created at each evaluation of the
    /// binding's scope.
    Scoped,
    /// Nowhere: no call selects the method.
    Unused,
}

/// A function bound by `let` or `letfn` whose methods are bound to entries of
/// their own, with no dispatcher (see "Local functions" in the module
/// documentation).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalFunction {
    /// The local that the `let` or `letfn` binds.
    pub local: LocalId,
    /// The `fn` expression that it binds, possibly through one-form `do`s.
    pub function: ExprId,
    /// Where each method is bound, by method index.
    pub placements: Box<[Placement]>,
}

/// A call whose method is chosen at compile time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirectCall {
    /// The `fn` expression whose method the call runs.
    pub function: ExprId,
    /// The index of that method.
    pub method: usize,
    /// The entry the call names, for a function of [`FunctionShape::Entries`]
    /// or a [`LocalFunction`]; `None` when the call names its callee as
    /// written (a global of [`FunctionShape::Single`], a local or a `fn`
    /// literal).
    pub entry: Option<Entry>,
    /// For a variadic method, its number of fixed parameters: the arguments
    /// after them become the rest sequence.
    pub rest: Option<usize>,
    /// Whether the method may read `this` in raw JavaScript, so that the call
    /// passes `null` with `.call(null, ...)` as before.
    pub receiver: bool,
}

/// The result of [`analyze`].
#[derive(Clone, Debug, Default)]
pub struct CallShapes {
    functions: Vec<FunctionShape>,
    /// By global: the function is known and its value never escapes.
    closed: Vec<bool>,
    /// By `ExprId`: the index in `direct` of the call's shape, or `u32::MAX`.
    slots: Vec<u32>,
    direct: Vec<(ExprId, DirectCall)>,
    /// By `LocalId`: the index in `local_functions` of the function the local
    /// binds, or `u32::MAX`.
    locals: Vec<u32>,
    local_functions: Vec<LocalFunction>,
}

impl CallShapes {
    /// How `global` is defined and called.
    pub fn function(&self, global: GlobalId) -> FunctionShape {
        self.functions
            .get(global.0)
            .copied()
            .unwrap_or(FunctionShape::Generic)
    }

    /// The direct shape of the call `call`, if it has one.
    pub fn call(&self, call: ExprId) -> Option<DirectCall> {
        let slot = *self.slots.get(call.0)?;
        self.direct.get(slot as usize).map(|(_, direct)| *direct)
    }

    /// How many calls are direct.
    pub fn direct_calls(&self) -> usize {
        self.direct.len()
    }

    fn insert(&mut self, call: ExprId, direct: DirectCall) {
        self.slots[call.0] = u32::try_from(self.direct.len()).expect("fewer than 2^32 calls");
        self.direct.push((call, direct));
    }

    /// Whether `global`'s function is known and does not escape, so that its
    /// direct calls ([`CallShapes::calls_of`]) are all its calls: nothing reads
    /// the global or the `fn`'s name other than as a callee, and the `fn`
    /// expression is evaluated only by the `def`.
    pub fn closed(&self, global: GlobalId) -> bool {
        self.closed.get(global.0).copied().unwrap_or(false)
    }

    /// The direct calls of the `fn` expression `function`, with the method
    /// each runs.
    pub fn calls_of(&self, function: ExprId) -> impl Iterator<Item = (ExprId, usize)> + '_ {
        self.direct
            .iter()
            .filter(move |(_, direct)| direct.function == function)
            .map(|(call, direct)| (*call, direct.method))
    }

    /// The function that the `let` or `letfn` local `local` binds, when its
    /// methods are bound to entries without a dispatcher.
    pub fn local_function(&self, local: LocalId) -> Option<&LocalFunction> {
        let index = *self.locals.get(local.0)?;
        self.local_functions.get(index as usize)
    }

    /// The local functions whose methods are bound to entries.
    pub fn local_functions(&self) -> &[LocalFunction] {
        &self.local_functions
    }

    /// How many methods of local functions are hoisted to the constant table.
    pub fn hoisted_methods(&self) -> usize {
        self.local_functions
            .iter()
            .flat_map(|function| &function.placements)
            .filter(|placement| **placement == Placement::Hoisted)
            .count()
    }
}

/// Decide the call shapes of `program`, the prepared program emission reads,
/// whose types are `inference`. `exports` escape. Without `inference` (as
/// [`crate::inlining`] asks, before types are inferred) calls of locals and
/// `fn` literals are not direct, and no local function has entries.
pub fn analyze(
    program: &Program,
    inference: Option<&Inference>,
    exports: &[GlobalId],
) -> CallShapes {
    let scan = Scan::new(program, exports);
    let local = match inference {
        Some(_) => LocalFunctions::new(program, &scan),
        None => LocalFunctions::default(),
    };
    let Scan {
        functions,
        closed,
        resolved,
        name_resolved,
        inferred_sites,
        ..
    } = scan;
    let count = program.exprs.len();
    let mut shapes = CallShapes {
        functions,
        closed,
        slots: vec![u32::MAX; count],
        direct: Vec::new(),
        locals: local.locals,
        local_functions: local.functions,
    };
    let mut this = ReceiverCache {
        methods: vec![None; count],
    };
    for (call, global, method) in resolved.into_iter().chain(name_resolved) {
        let (function, entry) = match shapes.functions[global.0] {
            FunctionShape::Generic => continue,
            FunctionShape::Single { function, .. } => (function, None),
            FunctionShape::Entries { function, .. } => {
                (function, Some(Entry::Global { global, method }))
            }
        };
        let direct = this.direct(program, function, method, entry);
        shapes.insert(call, direct);
    }
    for (call, index, method) in local.calls {
        let LocalFunction {
            local,
            function,
            ref placements,
        } = shapes.local_functions[index as usize];
        let entry = match placements[method] {
            Placement::Hoisted => Entry::Hoisted { function, method },
            Placement::Scoped | Placement::Unused => Entry::Scoped { local, method },
        };
        let direct = this.direct(program, function, method, Some(entry));
        shapes.insert(call, direct);
    }
    let Some(inference) = inference else {
        return shapes;
    };
    for call in inferred_sites {
        if shapes.slots[call.0] != u32::MAX {
            continue;
        }
        let Some(function) = inference.callee(program, call) else {
            continue;
        };
        // A callee that may be nil keeps `.call(null, ...)`, so that calling
        // nil throws the `TypeError` it throws below `Advanced`.
        let ExprKind::Call { callee, .. } = program.exprs[call.0].kind else {
            continue;
        };
        if inference.ty(callee) == Type::FN
            && single(methods_of(program, function))
            && !program.macro_functions.contains(&function)
        {
            let direct = this.direct(program, function, 0, None);
            shapes.insert(call, direct);
        }
    }
    shapes
}

/// The `fn` expressions whose every call the program makes itself, and those
/// calls; see "For parameter types" in the module documentation. The result
/// of [`closed_functions`].
#[derive(Clone, Debug, Default)]
pub struct ClosedFunctions {
    /// By `ExprId`: whether this `fn` expression is closed.
    functions: Vec<bool>,
    /// By `ExprId`: the index in `calls` of the call, or `u32::MAX`.
    slots: Vec<u32>,
    /// Each call of a closed `fn`, with the `fn` and the method it runs.
    calls: Vec<(ExprId, ExprId, usize)>,
}

impl ClosedFunctions {
    /// Whether every call of the `fn` expression `function` is one of
    /// [`ClosedFunctions::call`].
    pub fn closed(&self, function: ExprId) -> bool {
        self.functions.get(function.0).copied().unwrap_or(false)
    }

    /// The closed `fn` that `call` calls and the index of the method it runs.
    pub fn call(&self, call: ExprId) -> Option<(ExprId, usize)> {
        let slot = *self.slots.get(call.0)?;
        self.calls
            .get(slot as usize)
            .map(|&(_, function, method)| (function, method))
    }

    /// How many `fn` expressions are closed.
    pub fn len(&self) -> usize {
        self.functions.iter().filter(|closed| **closed).count()
    }

    /// Whether no `fn` expression is closed.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Find the closed `fn` expressions of `program` and their calls, without
/// inference. `exports` escape.
pub fn closed_functions(program: &Program, exports: &[GlobalId]) -> ClosedFunctions {
    let scan = Scan::new(program, exports);
    let count = program.exprs.len();
    let mut closed = ClosedFunctions {
        functions: vec![false; count],
        slots: vec![u32::MAX; count],
        calls: Vec::new(),
    };
    let local_calls = &scan.local_calls;
    let mut global_calls: Vec<Vec<ExprId>> = vec![Vec::new(); program.globals.len()];
    for &(call, global, _) in &scan.resolved {
        global_calls[global.0].push(call);
    }
    for &function in &scan.order {
        let ExprKind::Fn { name, methods } = &program.exprs[function.0].kind else {
            continue;
        };
        if program.macro_functions.contains(&function) || !scan.name_closed(*name) {
            continue;
        }
        let Some((user, value)) = scan.sole_use(program, function) else {
            continue;
        };
        let mut calls = match &program.exprs[user.0].kind {
            ExprKind::Call { callee, .. } if *callee == value => vec![user],
            ExprKind::Let { bindings, .. } | ExprKind::LetFn { bindings, .. } => {
                match bindings.iter().find(|(_, init)| *init == value) {
                    Some((local, _)) if !scan.local_escapes[local.0] => {
                        local_calls[local.0].clone()
                    }
                    _ => continue,
                }
            }
            ExprKind::Def {
                global,
                init: Some(init),
                ..
            } if *init == value
                && scan.closed[global.0]
                && scan.functions[global.0].function() == Some(function) =>
            {
                global_calls[global.0].clone()
            }
            _ => continue,
        };
        if let Some(name) = name {
            calls.extend(&local_calls[name.0]);
        }
        closed.functions[function.0] = true;
        for call in calls {
            let ExprKind::Call { args, .. } = &program.exprs[call.0].kind else {
                continue;
            };
            if let Some(method) = dispatched_method(methods, args.len()) {
                closed.slots[call.0] =
                    u32::try_from(closed.calls.len()).expect("fewer than 2^32 calls");
                closed.calls.push((call, function, method));
            }
        }
    }
    closed
}

/// What [`analyze`] and [`closed_functions`] decide without inference: the
/// known functions and their shapes, which values escape, and the calls of
/// known globals and of locals.
struct Scan {
    parent: Vec<Option<ExprId>>,
    parents: Vec<u8>,
    /// Every expression emission can reach, each after its parent.
    order: Vec<ExprId>,
    functions: Vec<FunctionShape>,
    /// By global: the function is known and does not escape.
    closed: Vec<bool>,
    /// The calls of known globals whose argument count selects a method, with
    /// the global and the method.
    resolved: Vec<(ExprId, GlobalId, usize)>,
    /// The calls through the name of a known function of
    /// [`FunctionShape::Entries`], with its global and the method.
    name_resolved: Vec<(ExprId, GlobalId, usize)>,
    /// The calls whose callee is a local or a `fn` literal.
    inferred_sites: Vec<ExprId>,
    /// By `LocalId`: an expression reads the local other than as the callee
    /// of a call, or a `reify` captures it (as a field of its object).
    local_escapes: Vec<bool>,
    /// By `LocalId`: a method of a type, `reify` or `extend-type` captures the
    /// local, so its emission may copy the local's value into an object.
    object_captures: Vec<bool>,
    /// By `LocalId`: the calls whose callee is the local.
    local_calls: Vec<Vec<ExprId>>,
}

impl Scan {
    fn new(program: &Program, exports: &[GlobalId]) -> Self {
        let count = program.exprs.len();
        // Every expression emission can reach, each with its parent; an
        // expression reached from two places has no single parent and counts as
        // read.
        let mut parent: Vec<Option<ExprId>> = vec![None; count];
        let mut parents = vec![0u8; count];
        let mut order = Vec::new();
        let mut visited = vec![false; count];
        visited[program.root.0] = true;
        let mut pending = vec![program.root];
        while let Some(id) = pending.pop() {
            order.push(id);
            for child in children(program, id) {
                parents[child.0] = parents[child.0].saturating_add(1);
                parent[child.0] = Some(id);
                if !std::mem::replace(&mut visited[child.0], true) {
                    pending.push(child);
                }
            }
        }
        let parent_of = |id: ExprId| parent[id.0].filter(|_| parents[id.0] == 1);
        let callee_of = |id: ExprId| {
            parent_of(id).filter(|call| {
                matches!(program.exprs[call.0].kind, ExprKind::Call { callee, .. } if callee == id)
            })
        };
        // The direct top-level forms, which run once.
        let mut top = vec![false; count];
        let mut forms = vec![program.root];
        while let Some(id) = forms.pop() {
            match &program.exprs[id.0].kind {
                ExprKind::Do(items) => forms.extend(items),
                _ => top[id.0] = true,
            }
        }

        let globals = program.globals.len();
        let mut definitions = vec![0u32; globals];
        let mut definition = vec![None; globals];
        let mut fixed = vec![true; globals];
        let mut escaped = vec![false; globals];
        let mut sites = Vec::new();
        let mut inferred_sites = Vec::new();
        let mut local_escapes = vec![false; program.locals.len()];
        let mut object_captures = vec![false; program.locals.len()];
        let mut local_calls = vec![Vec::new(); program.locals.len()];
        for &id in &order {
            let mut assign = |target: &ExprId| {
                if let ExprKind::Global(global) = program.exprs[target.0].kind {
                    fixed[global.0] = false;
                }
            };
            // Reads of locals that are not `Local` expressions: a `reify`
            // object holds every visible local as a field, and an `IFn`
            // adapter of an object method may copy its captures into an
            // object (`captures3.l5`).
            let object_methods = match &program.exprs[id.0].kind {
                ExprKind::Reify {
                    captures, methods, ..
                } => {
                    captures
                        .iter()
                        .for_each(|local| local_escapes[local.0] = true);
                    methods.as_slice()
                }
                ExprKind::DefType { methods, .. } | ExprKind::ExtendType { methods, .. } => {
                    methods.as_slice()
                }
                _ => &[],
            };
            for (_, method) in object_methods {
                for local in &method.captures {
                    object_captures[local.0] = true;
                }
            }
            match &program.exprs[id.0].kind {
                ExprKind::Def {
                    global,
                    init,
                    test,
                    once,
                } => {
                    if init.is_some() || *once {
                        definitions[global.0] += 1;
                        definition[global.0] = Some(id);
                    }
                    if *once || test.is_some() {
                        fixed[global.0] = false;
                    }
                }
                ExprKind::SetGlobal { target, .. } => assign(target),
                ExprKind::Binding { bindings, .. } => {
                    bindings.iter().for_each(|(target, _)| assign(target))
                }
                // Raw text may assign its holes.
                ExprKind::JsTemplate { args, .. } => args.iter().for_each(assign),
                ExprKind::Global(global) => {
                    if callee_of(id).is_none() {
                        escaped[global.0] = true;
                    }
                }
                // A local that is assigned (`set!`, a `js*` hole) is read there.
                ExprKind::Local(local) => match callee_of(id) {
                    Some(call) => local_calls[local.0].push(call),
                    None => local_escapes[local.0] = true,
                },
                ExprKind::Call { callee, args } => match program.exprs[callee.0].kind {
                    ExprKind::Global(global) => sites.push((id, global, args.len())),
                    ExprKind::Local(_) | ExprKind::Fn { .. } => inferred_sites.push(id),
                    _ => (),
                },
                // Types and protocols define their globals some other way.
                kind => {
                    for global in crate::application::globals(kind) {
                        fixed[global.0] = false;
                    }
                }
            }
        }

        let mut scan = Self {
            parent,
            parents,
            order,
            functions: vec![FunctionShape::Generic; globals],
            closed: vec![false; globals],
            resolved: Vec::new(),
            name_resolved: Vec::new(),
            inferred_sites,
            local_escapes,
            object_captures,
            local_calls,
        };
        let mut known = vec![None; globals];
        for global in 0..globals {
            let binding = &program.globals[global];
            if definitions[global] != 1
                || !fixed[global]
                || binding.name.contains('.')
                || crate::application::dynamic(program, GlobalId(global))
            {
                continue;
            }
            let Some(def) = definition[global] else {
                continue;
            };
            let ExprKind::Def {
                init: Some(init), ..
            } = program.exprs[def.0].kind
            else {
                continue;
            };
            let function = crate::application::unwrap_do(program, init);
            let ExprKind::Fn { name, methods } = &program.exprs[function.0].kind else {
                continue;
            };
            if program.macro_functions.contains(&function) {
                continue;
            }
            // The name is the function of one evaluation of the `def`: its
            // calls may name the shared entries when the `def` runs once and
            // evaluates the `fn` once, and every read of the name is such a
            // call.
            if !single(methods)
                && let Some(name) = *name
            {
                let resolved: Option<Vec<_>> = scan.local_calls[name.0]
                    .iter()
                    .map(|&call| {
                        let ExprKind::Call { args, .. } = &program.exprs[call.0].kind else {
                            return None;
                        };
                        method_for(methods, args.len())
                            .map(|method| (call, GlobalId(global), method))
                    })
                    .collect();
                match resolved {
                    Some(calls)
                        if top[def.0]
                            && !scan.local_escapes[name.0]
                            && !scan.object_captures[name.0]
                            && scan
                                .sole_use(program, function)
                                .is_some_and(|(user, _)| user == def) =>
                    {
                        scan.name_resolved.extend(calls);
                    }
                    _ => continue,
                }
            }
            known[global] = Some(function);
            if !discarded(program, def, &|id| scan.parent_of(id)) {
                escaped[global] = true;
            }
        }
        for global in exports {
            if let Some(escaped) = escaped.get_mut(global.0) {
                *escaped = true;
            }
        }
        // Calls whose count selects no method go through the dispatcher.
        for (call, global, arguments) in sites {
            let Some(function) = known[global.0] else {
                continue;
            };
            match method_for(methods_of(program, function), arguments) {
                Some(method) => scan.resolved.push((call, global, method)),
                None => escaped[global.0] = true,
            }
        }
        for global in 0..globals {
            let Some(function) = known[global] else {
                continue;
            };
            // The value escapes through the `fn`'s name, or through a second
            // evaluation of the `fn` expression, as well as through the global.
            let ExprKind::Fn { name, .. } = program.exprs[function.0].kind else {
                continue;
            };
            let closed = !escaped[global]
                && scan.name_closed(name)
                && scan
                    .sole_use(program, function)
                    .is_some_and(|(user, _)| Some(user) == definition[global]);
            scan.closed[global] = closed;
            scan.functions[global] = if single(methods_of(program, function)) {
                FunctionShape::Single { function, closed }
            } else {
                FunctionShape::Entries {
                    function,
                    dispatcher: escaped[global],
                    rerun: escaped[global] && !definition[global].is_some_and(|def| top[def.0]),
                }
            };
        }
        scan
    }

    fn parent_of(&self, id: ExprId) -> Option<ExprId> {
        self.parent[id.0].filter(|_| self.parents[id.0] == 1)
    }

    /// Where the value of `id` goes: the expression that receives it and its
    /// child that holds it, through one-form `do`s, when each has one parent.
    fn sole_use(&self, program: &Program, id: ExprId) -> Option<(ExprId, ExprId)> {
        let mut child = id;
        loop {
            let parent = self.parent_of(child)?;
            match &program.exprs[parent.0].kind {
                ExprKind::Do(items) if items.len() == 1 => child = parent,
                _ => return Some((parent, child)),
            }
        }
    }

    /// Whether a `fn` with this name, if any, has its name read only as the
    /// callee of calls.
    fn name_closed(&self, name: Option<LocalId>) -> bool {
        name.is_none_or(|name| !self.local_escapes[name.0])
    }
}

/// The local functions of a program and the calls of their entries; see
/// "Local functions" in the module documentation.
#[derive(Default)]
struct LocalFunctions {
    /// By `LocalId`: the index in `functions` of the function the local binds,
    /// or `u32::MAX`.
    locals: Vec<u32>,
    functions: Vec<LocalFunction>,
    /// The calls through each function's local or name, with the function's
    /// index and the method the call selects.
    calls: Vec<(ExprId, u32, usize)>,
}

impl LocalFunctions {
    fn new(program: &Program, scan: &Scan) -> Self {
        let mut local = Self {
            locals: Vec::new(),
            functions: Vec::new(),
            calls: Vec::new(),
        };
        for &id in &scan.order {
            let (ExprKind::Let { bindings, .. } | ExprKind::LetFn { bindings, .. }) =
                &program.exprs[id.0].kind
            else {
                continue;
            };
            'binding: for &(binding, init) in bindings {
                let function = crate::application::unwrap_do(program, init);
                let ExprKind::Fn { name, methods } = &program.exprs[function.0].kind else {
                    continue;
                };
                if single(methods)
                    || program.macro_functions.contains(&function)
                    || scan.sole_use(program, function) != Some((id, init))
                {
                    continue;
                }
                let index = u32::try_from(local.functions.len()).expect("fewer than 2^32 locals");
                let start = local.calls.len();
                let mut placements = vec![Placement::Unused; methods.len()];
                for reader in std::iter::once(binding).chain(*name) {
                    if scan.local_escapes[reader.0] || scan.object_captures[reader.0] {
                        local.calls.truncate(start);
                        continue 'binding;
                    }
                    for &call in &scan.local_calls[reader.0] {
                        let ExprKind::Call { args, .. } = &program.exprs[call.0].kind else {
                            unreachable!("a local call")
                        };
                        let Some(method) = method_for(methods, args.len()) else {
                            local.calls.truncate(start);
                            continue 'binding;
                        };
                        placements[method] = Placement::Hoisted;
                        local.calls.push((call, index, method));
                    }
                }
                local.functions.push(LocalFunction {
                    local: binding,
                    function,
                    placements: placements.into(),
                });
            }
        }
        if local.functions.is_empty() {
            return local;
        }
        local.locals = vec![u32::MAX; program.locals.len()];
        for (index, function) in local.functions.iter().enumerate() {
            local.locals[function.local.0] = index as u32;
        }
        local.place(program, scan);
        local
    }

    /// Bind to a `const` next to its function's binding ([`Placement::
    /// Scoped`]) each called method that may read that scope: it holds `js*`
    /// (raw text cannot move to the constant table) or an expression with two
    /// parents (which could see another scope), or reads a local bound outside
    /// it, other than as the callee of a call of a hoisted method.
    fn place(&mut self, program: &Program, scan: &Scan) {
        // The methods of the local functions, numbered in order.
        let mut first = Vec::with_capacity(self.functions.len());
        let mut methods = 0;
        for function in &self.functions {
            first.push(methods);
            methods += function.placements.len();
        }
        let count = program.exprs.len();
        let mut function_of = vec![u32::MAX; count];
        for (index, function) in self.functions.iter().enumerate() {
            function_of[function.function.0] = index as u32;
        }
        let mut walk = Walk {
            target: vec![u32::MAX; count],
            scoped: vec![false; methods],
            callers: vec![Vec::new(); methods],
            depth: vec![0; program.locals.len()],
            frames: Vec::new(),
        };
        for &(call, index, method) in &self.calls {
            walk.target[call.0] = (first[index as usize] + method) as u32;
        }
        enum Task<'p> {
            Visit(ExprId),
            Enter(usize, &'p FnMethod),
            Exit,
        }
        let mut visited = vec![false; count];
        // The functions in preorder, so each walk starts at an outermost one.
        for root in self.functions.iter().map(|function| function.function) {
            if std::mem::replace(&mut visited[root.0], true) {
                continue;
            }
            let mut tasks = vec![Task::Visit(root)];
            while let Some(task) = tasks.pop() {
                let id = match task {
                    Task::Visit(id) => id,
                    Task::Enter(number, method) => {
                        walk.frames.push(number);
                        walk.bind(method.params.iter().chain(&method.rest).copied());
                        continue;
                    }
                    Task::Exit => {
                        walk.frames.pop();
                        continue;
                    }
                };
                let object_methods = match &program.exprs[id.0].kind {
                    ExprKind::Local(local) => {
                        let call = scan.parent_of(id).filter(|call| {
                            matches!(program.exprs[call.0].kind,
                                ExprKind::Call { callee, .. } if callee == id)
                        });
                        walk.read(*local, call);
                        &[][..]
                    }
                    ExprKind::JsTemplate { .. } => {
                        walk.scope_frames();
                        &[]
                    }
                    ExprKind::Let { bindings, .. }
                    | ExprKind::LetFn { bindings, .. }
                    | ExprKind::Loop { bindings, .. } => {
                        walk.bind(bindings.iter().map(|(local, _)| *local));
                        &[]
                    }
                    ExprKind::ThisAs { binding, .. } => {
                        walk.bind([*binding]);
                        &[]
                    }
                    ExprKind::Try { catches, .. } => {
                        walk.bind(catches.iter().map(|catch| catch.binding));
                        &[]
                    }
                    ExprKind::Fn { name, methods } => {
                        walk.bind(*name);
                        let index = function_of[id.0];
                        if index != u32::MAX {
                            // A local function's methods are frames of their own.
                            for (offset, method) in methods.iter().enumerate().rev() {
                                let number = first[index as usize] + offset;
                                if std::mem::replace(&mut visited[method.body.0], true)
                                    || scan.parents[method.body.0] > 1
                                {
                                    walk.scoped[number] = true;
                                }
                                tasks.extend([
                                    Task::Exit,
                                    Task::Visit(method.body),
                                    Task::Enter(number, method),
                                ]);
                            }
                            continue;
                        }
                        for method in methods {
                            walk.bind(method.params.iter().chain(&method.rest).copied());
                        }
                        &[]
                    }
                    // A `reify` object holds every visible local.
                    ExprKind::Reify {
                        captures, methods, ..
                    } => {
                        for &local in captures {
                            walk.read(local, None);
                        }
                        methods.as_slice()
                    }
                    ExprKind::DefType { methods, .. } | ExprKind::ExtendType { methods, .. } => {
                        methods.as_slice()
                    }
                    _ => &[],
                };
                for (_, method) in object_methods {
                    walk.bind(method.params.iter().chain(&method.rest).copied());
                }
                for child in children(program, id) {
                    // An expression with two parents is emitted in both places.
                    if scan.parents[child.0] > 1 {
                        walk.scope_frames();
                    }
                    if !std::mem::replace(&mut visited[child.0], true) {
                        tasks.push(Task::Visit(child));
                    }
                }
            }
        }
        // A method that calls a scoped method reads the binding of its entry.
        let Walk {
            mut scoped,
            callers,
            ..
        } = walk;
        let mut pending: Vec<usize> = (0..methods).filter(|&method| scoped[method]).collect();
        while let Some(method) = pending.pop() {
            for &caller in &callers[method] {
                if !std::mem::replace(&mut scoped[caller], true) {
                    pending.push(caller);
                }
            }
        }
        for (function, first) in self.functions.iter_mut().zip(first) {
            for (offset, placement) in function.placements.iter_mut().enumerate() {
                if *placement == Placement::Hoisted && scoped[first + offset] {
                    *placement = Placement::Scoped;
                }
            }
        }
    }
}

/// The state of [`LocalFunctions::place`]'s walk.
struct Walk {
    /// By call: the number of the method it runs, when it names the entry of
    /// a local function's method, or `u32::MAX`.
    target: Vec<u32>,
    /// By method number: the method reads its scope.
    scoped: Vec<bool>,
    /// By method number: the methods that call it from outside its function's
    /// binding, which read that binding if it is scoped.
    callers: Vec<Vec<usize>>,
    /// By `LocalId`: how many frames enclose the local's binding. Locals whose
    /// binding the walk does not see are bound outside every frame.
    depth: Vec<usize>,
    /// The methods of local functions that enclose the expression visited,
    /// innermost last.
    frames: Vec<usize>,
}

impl Walk {
    fn bind(&mut self, locals: impl IntoIterator<Item = LocalId>) {
        for local in locals {
            self.depth[local.0] = self.frames.len();
        }
    }

    /// A read of `local`, as the callee of `call` if any: each frame inside
    /// the local's binding reads it from outside, which only a call of a
    /// hoisted method's entry does without reading the scope.
    fn read(&mut self, local: LocalId, call: Option<ExprId>) {
        let outside = &self.frames[self.depth[local.0].min(self.frames.len())..];
        match call.map(|call| self.target[call.0]) {
            Some(method) if method != u32::MAX => {
                self.callers[method as usize].extend(outside);
            }
            _ => outside.iter().for_each(|&frame| self.scoped[frame] = true),
        }
    }

    /// Every enclosing frame reads its scope.
    fn scope_frames(&mut self) {
        for &frame in &self.frames {
            self.scoped[frame] = true;
        }
    }
}

/// Whether a function with these methods is its one fixed method, with no
/// dispatcher.
fn single(methods: &[FnMethod]) -> bool {
    matches!(methods, [method] if method.rest.is_none())
}

fn methods_of(program: &Program, function: ExprId) -> &[FnMethod] {
    match &program.exprs[function.0].kind {
        ExprKind::Fn { methods, .. } => methods,
        _ => &[],
    }
}

/// The method the dispatcher runs for `arguments` arguments, when it calls one
/// with exactly those: the fixed method of that count, else the variadic
/// method when there are at least its fixed parameters. A function with one
/// fixed method has no dispatcher and runs it for every count.
fn method_for(methods: &[FnMethod], arguments: usize) -> Option<usize> {
    if single(methods) {
        return Some(0);
    }
    methods
        .iter()
        .position(|method| method.rest.is_none() && method.params.len() == arguments)
        .or_else(|| {
            methods
                .iter()
                .position(|method| method.rest.is_some() && method.params.len() <= arguments)
        })
}

/// The method a call with `arguments` arguments runs, through the dispatcher
/// or directly: the fixed method of that count, else the variadic method, which
/// the dispatcher also runs for fewer arguments than its fixed parameters
/// (they are `undefined`). `None` when the call throws. A call that
/// [`method_for`] resolves runs the same method.
fn dispatched_method(methods: &[FnMethod], arguments: usize) -> Option<usize> {
    if single(methods) {
        return Some(0);
    }
    methods
        .iter()
        .position(|method| method.rest.is_none() && method.params.len() == arguments)
        .or_else(|| methods.iter().position(|method| method.rest.is_some()))
}

/// Whether the value of `id` is discarded: it is a `do` form other than the
/// last, a `finally` or `with-out-str` body, or the value of a form whose
/// value is discarded (the last `do` form, an `if` branch, a `let`, `loop`,
/// `binding`, `this-as` or `try` body, a `catch` or `case` branch).
fn discarded(
    program: &Program,
    mut id: ExprId,
    parent_of: &impl Fn(ExprId) -> Option<ExprId>,
) -> bool {
    while let Some(parent) = parent_of(id) {
        let passes = match &program.exprs[parent.0].kind {
            ExprKind::Do(items) if items.last() != Some(&id) => return true,
            ExprKind::Do(_) => true,
            ExprKind::WithOutStr(_) => return true,
            ExprKind::Try { finally, .. } if *finally == Some(id) => return true,
            ExprKind::Try { body, catches, .. } => {
                *body == id || catches.iter().any(|catch| catch.body == id)
            }
            ExprKind::Let { body, .. }
            | ExprKind::LetFn { body, .. }
            | ExprKind::Loop { body, .. }
            | ExprKind::Binding { body, .. }
            | ExprKind::ThisAs { body, .. } => *body == id,
            ExprKind::If { yes, no, .. } => *yes == id || *no == id,
            ExprKind::Case {
                branches, default, ..
            } => *default == Some(id) || branches.iter().any(|(_, branch)| *branch == id),
            _ => false,
        };
        if !passes {
            return false;
        }
        id = parent;
    }
    false
}

/// Whether each method of each function may read `this` in raw JavaScript,
/// by the `fn` expression's `ExprId`, computed on first use.
struct ReceiverCache {
    methods: Vec<Option<Vec<bool>>>,
}

impl ReceiverCache {
    fn direct(
        &mut self,
        program: &Program,
        function: ExprId,
        method: usize,
        entry: Option<Entry>,
    ) -> DirectCall {
        let methods = methods_of(program, function);
        let receivers = self.methods[function.0].get_or_insert_with(|| {
            methods
                .iter()
                .map(|method| raw_javascript(program, method.body))
                .collect()
        });
        DirectCall {
            function,
            method,
            entry,
            rest: methods[method].rest.map(|_| methods[method].params.len()),
            receiver: receivers[method],
        }
    }
}

/// Whether `body` holds raw JavaScript outside nested functions, which may
/// read the method's `this`.
fn raw_javascript(program: &Program, body: ExprId) -> bool {
    let mut pending = vec![body];
    while let Some(id) = pending.pop() {
        match program.exprs[id.0].kind {
            ExprKind::JsTemplate { .. } => return true,
            ExprKind::Fn { .. } => (),
            _ => pending.extend(children(program, id)),
        }
    }
    false
}
