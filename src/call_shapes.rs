//! Call shapes: Phase 2 step 5 of docs/advanced-optimization.md
//! (light-cljs-212), at `Advanced` only.
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
//! metadata, and no name with a dot (a host path). Macro functions and named
//! functions with a dispatcher, whose methods may read the dispatcher through
//! the name, are left alone.
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
//!   pass. The global itself, its dispatcher and its arity properties exist
//!   only when the function escapes; the dispatcher then calls the same
//!   entries. Because the entries are shared, a function that escapes keeps
//!   this shape only when its `def` is a top-level form, which runs once.
//!   Otherwise it stays [`FunctionShape::Generic`].
//! - Calls of a local bound to a `fn` with one fixed method, or of such a `fn`
//!   literal, which inference
//!   knows ([`crate::inference::Inference::callee`]), are plain calls too.
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
//! # For parameter types
//!
//! [`CallShapes::closed`] says when the direct calls are every call of a
//! function: then each value its parameters receive is an argument of one of
//! [`CallShapes::calls_of`], which a later inference pass can join.
use crate::analyzer::{ExprId, ExprKind, FnMethod, GlobalId, Program};
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
    /// when `dispatcher` (the function escapes).
    Entries { function: ExprId, dispatcher: bool },
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

/// A call whose method is chosen at compile time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirectCall {
    /// The `fn` expression whose method the call runs.
    pub function: ExprId,
    /// The index of that method.
    pub method: usize,
    /// The global and method index of the entry the call names, for a
    /// function of [`FunctionShape::Entries`]; `None` when the call names its
    /// callee as written (a global of [`FunctionShape::Single`], a local or a
    /// `fn` literal).
    pub entry: Option<(GlobalId, usize)>,
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
    /// By `ExprId`: the index in `direct` of the call's shape, or `u32::MAX`.
    slots: Vec<u32>,
    direct: Vec<(ExprId, DirectCall)>,
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
    /// direct calls ([`CallShapes::calls_of`]) are all its calls.
    pub fn closed(&self, global: GlobalId) -> bool {
        matches!(
            self.function(global),
            FunctionShape::Single { closed: true, .. }
                | FunctionShape::Entries {
                    dispatcher: false,
                    ..
                }
        )
    }

    /// The direct calls of the `fn` expression `function`, with the method
    /// each runs.
    pub fn calls_of(&self, function: ExprId) -> impl Iterator<Item = (ExprId, usize)> + '_ {
        self.direct
            .iter()
            .filter(move |(_, direct)| direct.function == function)
            .map(|(call, direct)| (*call, direct.method))
    }
}

/// Decide the call shapes of `program`, the prepared program emission reads,
/// whose types are `inference`. `exports` escape.
pub fn analyze(program: &Program, inference: &Inference, exports: &[GlobalId]) -> CallShapes {
    let count = program.exprs.len();
    // Every expression emission can reach, each with its parent; an expression
    // reached from two places has no single parent and counts as read.
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
    for &id in &order {
        let mut assign = |target: &ExprId| {
            if let ExprKind::Global(global) = program.exprs[target.0].kind {
                fixed[global.0] = false;
            }
        };
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
                let callee = parent_of(id).is_some_and(|call| {
                    matches!(program.exprs[call.0].kind, ExprKind::Call { callee, .. } if callee == id)
                });
                if !callee {
                    escaped[global.0] = true;
                }
            }
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

    let mut functions = vec![FunctionShape::Generic; globals];
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
        if program.macro_functions.contains(&function) || (!single(methods) && name.is_some()) {
            continue;
        }
        known[global] = Some(function);
        if !discarded(program, def, &parent_of) {
            escaped[global] = true;
        }
    }
    for global in exports {
        if let Some(escaped) = escaped.get_mut(global.0) {
            *escaped = true;
        }
    }
    // Calls whose count selects no method go through the dispatcher.
    let mut resolved = Vec::new();
    for (call, global, arguments) in sites {
        let Some(function) = known[global.0] else {
            continue;
        };
        match method_for(methods_of(program, function), arguments) {
            Some(method) => resolved.push((call, global, method)),
            None => escaped[global.0] = true,
        }
    }
    for global in 0..globals {
        let Some(function) = known[global] else {
            continue;
        };
        functions[global] = if single(methods_of(program, function)) {
            FunctionShape::Single {
                function,
                closed: !escaped[global],
            }
        } else if !escaped[global] || definition[global].is_some_and(|def| top[def.0]) {
            FunctionShape::Entries {
                function,
                dispatcher: escaped[global],
            }
        } else {
            FunctionShape::Generic
        };
    }

    let mut shapes = CallShapes {
        functions,
        slots: vec![u32::MAX; count],
        direct: Vec::new(),
    };
    let mut this = ReceiverCache {
        methods: vec![None; count],
    };
    for (call, global, method) in resolved {
        let (function, entry) = match shapes.functions[global.0] {
            FunctionShape::Generic => continue,
            FunctionShape::Single { function, .. } => (function, None),
            FunctionShape::Entries { function, .. } => (function, Some((global, method))),
        };
        let direct = this.direct(program, function, method, entry);
        shapes.insert(call, direct);
    }
    for call in inferred_sites {
        let Some(function) = inference.callee(program, call) else {
            continue;
        };
        if single(methods_of(program, function)) && !program.macro_functions.contains(&function) {
            let direct = this.direct(program, function, 0, None);
            shapes.insert(call, direct);
        }
    }
    shapes
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
        entry: Option<(GlobalId, usize)>,
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
