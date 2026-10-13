//! Budgeted inlining over the prepared IR: the inlining half of Phase 2 step 8
//! of docs/advanced-optimization.md (light-cljs-214), at `Advanced` only.
//! [`crate::folding`] is the other half; `prepare_application` alternates the
//! two with definition removal (see "Phase 2 folding and inlining extension
//! points" in the design document).
//!
//! [`inline`] replaces a call whose target is known at compile time
//! ([`crate::call_shapes`]) with the body of the method it runs when:
//!
//! - the callee is *single-use*: its value never escapes
//!   ([`CallShapes::closed`]) and this is its only call, so the definition
//!   goes away once nothing reads it and the body moves rather than grows; or
//! - the body costs at most [`INLINE_BUDGET`] expressions, is *pure* and
//!   inlining it lowers the program's cost ([`lowers_cost`]). A pure body only
//!   reads its parameters, locals, globals and constants, branches, binds
//!   locals, builds literal collections, looks up keywords and calls core
//!   operations whose row in `src/core_operations.tsv` is `pure` or
//!   `allocates` and calls no function argument, so it can run only the code
//!   its arguments supply (protocol implementations), as the call would. Its
//!   copies spend the program's growth budget ([`growth_budget`]).
//!
//! The cost walk stops as soon as a body exceeds its limit (rule 4 of
//! docs/dos-and-donts.md), [`SINGLE_USE_LIMIT`] for single-use callees.
//! Upstream's Closure Compiler weighs inlining the same way, by the size of the
//! body against the calls it replaces and the definition it removes
//! (`FunctionInjector.java:910-1080` in the research clone).
//!
//! # What is inlined
//!
//! The callee is a global whose sole definition
//! ([`crate::folding::sole_definitions`]) is a direct top-level `def`, and the
//! call runs after it: in a later top-level form, or inside the function
//! itself. Its argument count matches the method's fixed parameters (a
//! variadic method may receive more, when its body never reads the rest
//! parameter), and the call has one parent. The method captures nothing, is no
//! `recur` target, and its body holds only literals, local, global and host
//! reads, `if`, `do`, `let`, `^boolean` hints, calls, collection literals,
//! property reads, method calls, `new`, `instance?` and metadata: no nested
//! `fn`, loop, `try`, `throw`, `case`, assignment, definition, `js*` raw text,
//! `js-arguments` or `this-as`, whose meaning depends on the function they are
//! in. A callee tagged `^boolean` keeps its calls, whose tests trust the tag
//! (see "Advanced-mode contract"). Calls are visited children first, in
//! source order, so a function's own calls are inlined before it is copied.
//!
//! # Arguments
//!
//! A parameter whose argument is a literal, a core value, an immutable local
//! or a constant collection the pool holds reads a copy of the argument
//! ([`trivial`]): those have no effect and keep their value while the body
//! runs. An unread parameter drops an argument without effect; an argument
//! without effect that reads no global or mutable field moves to its
//! parameter's only read ([`stable`]); and one argument with an effect moves
//! to its parameter's only read when the body reaches that read before
//! anything with an effect, unconditionally ([`first_read`], Closure's direct
//! inlining). Any other argument is bound to a fresh local by a `let` around
//! the body, in argument order, so it is evaluated once and before the body,
//! as the call did; arguments for an unread rest parameter are bound too when
//! they may have an effect. A `let` in value position would be emitted as a
//! function call, so a call there is inlined only when it needs no binding and
//! its body has no `let` where it produces its value. Locals of the body get
//! fresh identities, so a body inlined twice in one scope does not clash.
use crate::analyzer::{Expr, ExprId, ExprKind, LocalBinding, LocalId, Program};
use crate::call_shapes::CallShapes;
use crate::core_operations::{Calls, Purity};
use crate::folding::{NOWHERE, constant_like};
use crate::inference::ProgramFacts;
use crate::program_fragment::children;

/// The most expressions a body inlined at every call may have.
pub const INLINE_BUDGET: usize = 10;

/// The most expressions a single-use body may have.
pub const SINGLE_USE_LIMIT: usize = 4096;

/// How many expressions inlining may add to a program of `nodes` reachable
/// expressions over all rounds: inlined calls of small pure functions copy
/// their bodies, and the budget bounds the copies.
pub fn growth_budget(nodes: usize) -> usize {
    nodes / 2 + 64
}

/// What a body walk found: its size, whether it is pure, and whether it has a
/// `let` where it produces its value.
#[derive(Clone)]
struct Body {
    nodes: usize,
    pure: bool,
    binds: bool,
    /// The local of every local read, once per read.
    reads: Vec<LocalId>,
}

/// How an argument reaches its parameter.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Use {
    /// Read in place at every read ([`trivial`]).
    Value,
    /// Read in place at its only read ([`stable`]).
    Moved,
    /// The parameter is never read and the argument has no effect.
    Dropped,
    /// Bound to a fresh local before the body.
    Bound,
}

/// How a parameter of an inlined method reads.
#[derive(Clone)]
enum Parameter {
    /// A copy of the argument expression at each read ([`trivial`]).
    Value(ExprId),
    /// A fresh local the argument is bound to.
    Local(LocalId),
    /// The argument expression, moved to the parameter's only read.
    Moved(ExprId),
    /// A parameter whose argument was dropped, or the rest parameter: a read
    /// aborts inlining.
    Unread,
}

/// Inline the calls of `program` that qualify, given its call shapes, and
/// return how many were inlined. `growth` is what is left of the program's
/// growth budget, which inlining pure functions at several calls spends.
pub fn inline(
    program: &mut Program,
    shapes: &CallShapes,
    growth: &mut usize,
    built: &mut Vec<bool>,
) -> usize {
    let count = program.exprs.len();
    let mut top = Vec::new();
    flatten(program, program.root, &mut top);
    let facts = ProgramFacts::new(program, &top);

    // Walk each top-level form in order, children first, recording each
    // expression's parent count, the first form that reaches it, the
    // innermost `fn` around it (`owner`) and whether emission writes it as a
    // statement or a returned value (`block`), where a `let` is a block
    // rather than a function call.
    let mut parents = vec![0u8; count];
    let mut owner = vec![None; count];
    let mut position = vec![NOWHERE; count];
    let mut block = vec![false; count];
    let mut visited = vec![false; count];
    let mut calls = Vec::new();
    let mut roles = Vec::new();
    for (index, &form) in top.iter().enumerate() {
        if visited[form.0] {
            continue;
        }
        // A top-level form's parent is the `do` around it.
        parents[form.0] = 1;
        block[form.0] = true;
        let mut pending = vec![(form, false)];
        while let Some((id, expanded)) = pending.pop() {
            if expanded {
                if matches!(program.exprs[id.0].kind, ExprKind::Call { .. }) {
                    calls.push(id);
                }
                continue;
            }
            if std::mem::replace(&mut visited[id.0], true) {
                continue;
            }
            position[id.0] = index as u32;
            pending.push((id, true));
            roles.clear();
            child_roles(program, id, block[id.0], &mut roles);
            let inner = if matches!(program.exprs[id.0].kind, ExprKind::Fn { .. }) {
                Some(id)
            } else {
                owner[id.0]
            };
            for &(child, role) in roles.iter().rev() {
                parents[child.0] = parents[child.0].saturating_add(1);
                if !visited[child.0] {
                    block[child.0] = role;
                    owner[child.0] = inner;
                    pending.push((child, false));
                }
            }
        }
    }

    let definitions = crate::folding::sole_definitions(program, &position);
    // Map keys and set members, which must not become constants.
    let fenced = crate::folding::fenced_positions(program);
    built.resize(count, false);
    let mut sites = vec![0u32; count];
    for &call in &calls {
        if let Some(direct) = shapes.call(call) {
            sites[direct.function.0] += 1;
        }
    }

    let mut inlined = 0;
    for call in calls {
        let Some(direct) = shapes.call(call) else {
            continue;
        };
        if parents[call.0] != 1 {
            continue;
        }
        let ExprKind::Call { callee, args } = &program.exprs[call.0].kind else {
            continue;
        };
        let ExprKind::Global(global) = program.exprs[callee.0].kind else {
            continue;
        };
        if program.globals[global.0].boolean_tag {
            continue;
        }
        // The definition is a direct top-level form that runs before the
        // form holding the call.
        let Some(definition) = definitions[global.0] else {
            continue;
        };
        let defined = position[definition.0];
        // A call inside the function itself runs once the function exists,
        // after its definition.
        let inside = || {
            let mut function = owner[call.0];
            while let Some(id) = function {
                if id == direct.function {
                    return true;
                }
                function = owner[id.0];
            }
            false
        };
        if top.get(defined as usize) != Some(&definition)
            || position[call.0] == NOWHERE
            || position[call.0] < defined
            || (position[call.0] == defined && !inside())
        {
            continue;
        }
        let ExprKind::Fn { methods, .. } = &program.exprs[direct.function.0].kind else {
            continue;
        };
        let method = methods[direct.method].clone();
        let args = args.clone();
        let fixed = method.params.len();
        if !method.captures.is_empty()
            || program.recur_targets[method.target.0].used
            || (method.rest.is_none() && args.len() != fixed)
            || args.len() < fixed
        {
            continue;
        }
        let closed = shapes.closed(global);
        let call_count = sites[direct.function.0] as usize;
        let single = closed && call_count == 1;
        let limit = if single {
            SINGLE_USE_LIMIT
        } else {
            INLINE_BUDGET
        };
        let Some(body) = measure(program, &facts, method.body, limit) else {
            continue;
        };
        if !single
            && (!body.pure
                || *growth < body.nodes
                || !lowers_cost(body.nodes, args.len(), fixed, closed.then_some(call_count)))
        {
            continue;
        }
        if body.binds && !block[call.0] {
            continue;
        }

        // How each parameter reads its argument. Arguments read in place or
        // dropped need no binding; the others are bound by a `let`, which only
        // a statement or returned value may hold.
        let mut uses: Vec<Use> = args
            .iter()
            .enumerate()
            .map(|(index, &arg)| {
                let kind = &program.exprs[arg.0].kind;
                let reads = method.params.get(index).map_or(0, |parameter| {
                    body.reads.iter().filter(|l| *l == parameter).count()
                });
                // A literal built at each evaluation is one value per call,
                // which copies at several reads would not be.
                if reads > 0 && trivial(program, kind) && !builds(program, built, arg) {
                    Use::Value
                } else if reads == 0 && crate::folding::effect_free(program, kind) {
                    Use::Dropped
                } else if reads == 1 && stable(program, kind) {
                    Use::Moved
                } else {
                    Use::Bound
                }
            })
            .collect();
        // One argument with an effect may move to its parameter's only read
        // when the body reads it before anything else it evaluates can have an
        // effect or throw: it is then evaluated once, at the same point
        // relative to every effect (Closure's direct inlining).
        if let [bound] = uses
            .iter()
            .enumerate()
            .filter(|(_, use_)| **use_ == Use::Bound)
            .map(|(index, _)| index)
            .collect::<Vec<_>>()[..]
            && let Some(&parameter) = method.params.get(bound)
            && !crate::folding::scoped(program, args[bound])
            && body.reads.iter().filter(|l| **l == parameter).count() == 1
            && first_read(program, method.body, parameter) == Scan::Found
        {
            uses[bound] = Use::Moved;
        }
        if uses.contains(&Use::Bound) && !block[call.0] {
            continue;
        }
        let (exprs, locals) = (program.exprs.len(), program.locals.len());
        let mut parameters: Vec<(LocalId, Parameter)> = Vec::with_capacity(args.len() + 1);
        let mut bindings = Vec::new();
        for (index, (&arg, use_)) in args.iter().zip(uses).enumerate() {
            let parameter = method.params.get(index).copied();
            let read = match use_ {
                Use::Value => Parameter::Value(arg),
                Use::Moved => Parameter::Moved(arg),
                Use::Dropped => Parameter::Unread,
                Use::Bound => {
                    let like = parameter.or(method.rest).expect("a parameter");
                    let local = fresh(program, like);
                    bindings.push((local, arg));
                    Parameter::Local(local)
                }
            };
            if let Some(parameter) = parameter {
                parameters.push((parameter, read));
            }
        }
        if let Some(rest) = method.rest {
            parameters.push((rest, Parameter::Unread));
        }
        let mut copier = Copier {
            program,
            built,
            call,
            parameters,
        };
        // A call that is a map key or set member must not become a constant.
        let Some(copy) = copier
            .copy(method.body)
            .filter(|copy| !fenced[call.0] || !constant_like(program, &program.exprs[copy.0].kind))
        else {
            program.exprs.truncate(exprs);
            program.locals.truncate(locals);
            built.truncate(exprs);
            continue;
        };
        let kind = if bindings.is_empty() {
            built[call.0] = built[copy.0];
            program.exprs[copy.0].kind.clone()
        } else {
            built[call.0] = false;
            ExprKind::Let {
                bindings,
                body: copy,
            }
        };
        program.exprs[call.0].kind = kind;
        if !single {
            *growth -= body.nodes;
        }
        inlined += 1;
    }
    inlined
}

/// Whether inlining a body of `body` expressions at a call with `arguments`
/// arguments shrinks the program, in expressions: the call costs itself, its
/// callee and its arguments, and a function of `parameters` parameters costs
/// its body and about three expressions more. A function that escapes keeps
/// its definition, so each inlined call must cost no more than the call. A
/// closed one with `sites` calls loses its definition once every call is
/// inlined, which pays for the copies (Closure's `inliningLowersCost`).
fn lowers_cost(body: usize, arguments: usize, parameters: usize, sites: Option<usize>) -> bool {
    let call = 2 + arguments;
    match sites {
        None => body <= call,
        Some(sites) => sites * body <= sites * call + body + 3 + parameters,
    }
}

/// The direct top-level forms of the program.
fn flatten(program: &Program, id: ExprId, top: &mut Vec<ExprId>) {
    if let ExprKind::Do(items) = &program.exprs[id.0].kind {
        for item in items {
            flatten(program, *item, top);
        }
    } else {
        top.push(id);
    }
}

/// The children of `id` and whether emission writes each as a statement or a
/// returned value, given whether it writes `id` so (`block`). A `let`, `loop`
/// or `letfn` body and a function body are always one: in value position the
/// form becomes a function call that returns it. Everything else not listed
/// is a value.
fn child_roles(program: &Program, id: ExprId, block: bool, out: &mut Vec<(ExprId, bool)>) {
    match &program.exprs[id.0].kind {
        ExprKind::Do(items) => out.extend(items.iter().map(|item| (*item, block))),
        ExprKind::If { test, yes, no } => out.extend([(*test, false), (*yes, block), (*no, block)]),
        ExprKind::Let { bindings, body }
        | ExprKind::LetFn { bindings, body }
        | ExprKind::Loop { bindings, body, .. } => {
            out.extend(bindings.iter().map(|(_, init)| (*init, false)));
            out.push((*body, true));
        }
        ExprKind::Fn { methods, .. } => out.extend(methods.iter().map(|m| (m.body, true))),
        _ => out.extend(
            children(program, id)
                .into_iter()
                .map(|child| (child, false)),
        ),
    }
}

/// Whether `id` or a literal in it is marked built at each evaluation
/// ([`crate::folding::built_literals`]): folding made its items constants,
/// but each copy would still be a value of its own.
fn builds(program: &Program, built: &[bool], id: ExprId) -> bool {
    let mut pending = vec![id];
    while let Some(id) = pending.pop() {
        if built.get(id.0).copied().unwrap_or(false) {
            return true;
        }
        if matches!(
            program.exprs[id.0].kind,
            ExprKind::Vector(_)
                | ExprKind::List(_)
                | ExprKind::Map(_)
                | ExprKind::Set(_)
                | ExprKind::LiteralMap(_)
                | ExprKind::LiteralSet(_)
                | ExprKind::WithMeta { .. }
        ) {
            pending.extend(children(program, id));
        }
    }
    false
}

/// Whether an argument of this kind may be read in place of its parameter:
/// it has no effect and its value cannot change while the body runs. A
/// constant collection literal qualifies because `Advanced` declares it once
/// in the constant pool, so every read is the one value the call would pass;
/// not one that [`builds`] at each evaluation.
fn trivial(program: &Program, kind: &ExprKind) -> bool {
    match kind {
        ExprKind::Nil
        | ExprKind::Bool(_)
        | ExprKind::Number(_)
        | ExprKind::String(_)
        | ExprKind::Keyword(_)
        | ExprKind::CoreFn(_) => true,
        // A mutable field may be assigned while the body runs.
        ExprKind::Local(local) => !program
            .field_bindings
            .get(local)
            .is_some_and(|(_, mutable)| *mutable),
        _ => pooled(program, kind),
    }
}

/// Whether a literal of this kind is a non-empty constant collection that the
/// constant pool holds (see "Constant pool extension points" in
/// docs/advanced-optimization.md): vectors and lists of constants, and maps
/// and sets whose keys and members are scalars. Empty literals read their
/// type's `EMPTY` where they are evaluated.
fn pooled(program: &Program, kind: &ExprKind) -> bool {
    let scalar = |id: &ExprId| {
        matches!(
            program.exprs[id.0].kind,
            ExprKind::Nil
                | ExprKind::Bool(_)
                | ExprKind::Number(_)
                | ExprKind::String(_)
                | ExprKind::Keyword(_)
                | ExprKind::Symbol(_)
        )
    };
    let constant = |id: &ExprId| scalar(id) || pooled(program, &program.exprs[id.0].kind);
    match kind {
        ExprKind::Vector(items) | ExprKind::List(items) => {
            !items.is_empty() && items.iter().all(constant)
        }
        ExprKind::Map(items) | ExprKind::LiteralMap(items) => {
            !items.is_empty()
                && items
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .all(|[key, value]| scalar(key) && constant(value))
        }
        ExprKind::Set(items) | ExprKind::LiteralSet(items) => {
            !items.is_empty() && items.iter().all(scalar)
        }
        _ => false,
    }
}

/// Whether an argument of this kind may be read in place of its parameter at
/// its one read: it has no effect ([`crate::folding::effect_free`]) and reads
/// no global or mutable field, whose value the body might change before that
/// read.
fn stable(program: &Program, kind: &ExprKind) -> bool {
    if !crate::folding::effect_free(program, kind) {
        return false;
    }
    match kind {
        ExprKind::Global(_) => false,
        ExprKind::Local(_) => trivial(program, kind),
        ExprKind::Vector(items)
        | ExprKind::List(items)
        | ExprKind::Map(items)
        | ExprKind::LiteralMap(items)
        | ExprKind::Set(items)
        | ExprKind::LiteralSet(items) => items
            .iter()
            .all(|item| stable(program, &program.exprs[item.0].kind)),
        _ => true,
    }
}

/// What [`first_read`] found in an expression, scanning in evaluation order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Scan {
    /// The read of the parameter, reached before anything with an effect.
    Found,
    /// Evaluated completely without an effect or a throw, and no read.
    Clean,
    /// Something that may have an effect or throw, or a conditional or
    /// repeated evaluation, comes first.
    Dirty,
}

/// Whether evaluating `id` reaches a read of `parameter` before anything
/// that may have an effect or throw, unconditionally: through calls,
/// `do`, `let` and the test of an `if`, never into a branch.
fn first_read(program: &Program, id: ExprId, parameter: LocalId) -> Scan {
    // Children in evaluation order: the first that is not clean decides.
    let sequence = |items: &mut dyn Iterator<Item = ExprId>| {
        for item in items {
            match first_read(program, item, parameter) {
                Scan::Clean => (),
                scan => return scan,
            }
        }
        Scan::Clean
    };
    // A form that runs code of its own after its operands.
    let operation = |scan: Scan| {
        if scan == Scan::Found {
            Scan::Found
        } else {
            Scan::Dirty
        }
    };
    match &program.exprs[id.0].kind {
        ExprKind::Local(local) if *local == parameter => Scan::Found,
        ExprKind::Nil
        | ExprKind::Bool(_)
        | ExprKind::Number(_)
        | ExprKind::String(_)
        | ExprKind::Keyword(_)
        | ExprKind::Symbol(_)
        | ExprKind::Local(_)
        | ExprKind::Global(_)
        | ExprKind::CoreFn(_) => Scan::Clean,
        ExprKind::Do(items) => sequence(&mut items.iter().copied()),
        ExprKind::Let { bindings, body } => sequence(
            &mut bindings
                .iter()
                .map(|(_, init)| *init)
                .chain(std::iter::once(*body)),
        ),
        ExprKind::BooleanHint(inner) => first_read(program, *inner, parameter),
        ExprKind::If { test, .. } => operation(first_read(program, *test, parameter)),
        // Building a vector or list has no effect and cannot throw.
        ExprKind::Vector(items) | ExprKind::List(items) => sequence(&mut items.iter().copied()),
        ExprKind::Call { callee, args } => operation(sequence(
            &mut std::iter::once(*callee).chain(args.iter().copied()),
        )),
        ExprKind::MethodCall { receiver, args, .. } => operation(sequence(
            &mut std::iter::once(*receiver).chain(args.iter().copied()),
        )),
        ExprKind::Property { receiver, .. } => operation(first_read(program, *receiver, parameter)),
        ExprKind::Construct { callee, args } => operation(sequence(
            &mut std::iter::once(*callee).chain(args.iter().copied()),
        )),
        _ => Scan::Dirty,
    }
}

/// A fresh local named like `like`.
fn fresh(program: &mut Program, like: LocalId) -> LocalId {
    let binding: LocalBinding = program.locals[like.0].clone();
    program.locals.push(binding);
    LocalId(program.locals.len() - 1)
}

/// Walk `body` and say what it is, or `None` when it holds something that is
/// never inlined or exceeds `limit` expressions; the walk stops there.
fn measure(program: &Program, facts: &ProgramFacts, body: ExprId, limit: usize) -> Option<Body> {
    let mut result = Body {
        nodes: 0,
        pure: true,
        binds: false,
        reads: Vec::new(),
    };
    // Each entry says whether the expression produces the body's value.
    let mut pending = vec![(body, true)];
    while let Some((id, produces)) = pending.pop() {
        result.nodes += 1;
        if result.nodes > limit {
            return None;
        }
        let kind = &program.exprs[id.0].kind;
        match kind {
            ExprKind::Local(local) => result.reads.push(*local),
            ExprKind::Nil
            | ExprKind::Bool(_)
            | ExprKind::Number(_)
            | ExprKind::String(_)
            | ExprKind::Keyword(_)
            | ExprKind::Symbol(_)
            | ExprKind::Global(_)
            | ExprKind::CoreFn(_) => (),
            // A constant collection is one read of its pool entry.
            kind if pooled(program, kind) => (),
            ExprKind::Do(items) => pending.extend(items.iter().map(|item| (*item, produces))),
            ExprKind::If { test, yes, no } => {
                pending.extend([(*test, false), (*yes, produces), (*no, produces)])
            }
            ExprKind::Let { bindings, body } => {
                result.binds |= produces;
                pending.extend(bindings.iter().map(|(_, init)| (*init, false)));
                pending.push((*body, produces));
            }
            ExprKind::BooleanHint(inner) => pending.push((*inner, produces)),
            ExprKind::List(items)
            | ExprKind::Vector(items)
            | ExprKind::Map(items)
            | ExprKind::Set(items)
            | ExprKind::LiteralMap(items)
            | ExprKind::LiteralSet(items) => {
                pending.extend(items.iter().map(|item| (*item, false)))
            }
            ExprKind::Call { callee, args } => {
                result.pure &= match program.exprs[callee.0].kind {
                    ExprKind::CoreFn(core) => {
                        let operation = core.operation();
                        !facts.replaced(core)
                            && operation.purity <= Purity::Allocates
                            && operation.calls != Calls::Invoke
                    }
                    // A keyword looks itself up.
                    ExprKind::Keyword(_) => true,
                    _ => false,
                };
                pending.push((*callee, false));
                pending.extend(args.iter().map(|arg| (*arg, false)));
            }
            ExprKind::JsGlobal(_)
            | ExprKind::Property { .. }
            | ExprKind::MethodCall { .. }
            | ExprKind::JsIndex { .. }
            | ExprKind::Construct { .. }
            | ExprKind::InstanceOf { .. }
            | ExprKind::WithMeta { .. } => {
                result.pure = false;
                pending.extend(
                    children(program, id)
                        .into_iter()
                        .map(|child| (child, false)),
                );
            }
            _ => return None,
        }
    }
    Some(result)
}

/// Copies a body for one call, mapping its parameters and giving its locals
/// fresh identities.
struct Copier<'p> {
    program: &'p mut Program,
    /// By expression: literals built at each evaluation
    /// ([`crate::folding::built_literals`]); copies carry the mark.
    built: &'p mut Vec<bool>,
    /// The call being replaced: a body that contains it calls itself.
    call: ExprId,
    parameters: Vec<(LocalId, Parameter)>,
}

impl Copier<'_> {
    fn parameter(&self, local: LocalId) -> Option<&Parameter> {
        self.parameters
            .iter()
            .rev()
            .find(|(parameter, _)| *parameter == local)
            .map(|(_, parameter)| parameter)
    }

    fn all(&mut self, ids: &[ExprId]) -> Option<Vec<ExprId>> {
        ids.iter().map(|id| self.copy(*id)).collect()
    }

    /// A copy of `id`, or `None` when it reads a local the copy cannot map or
    /// contains the call being replaced.
    fn copy(&mut self, id: ExprId) -> Option<ExprId> {
        if id == self.call {
            return None;
        }
        let kind = match self.program.exprs[id.0].kind.clone() {
            kind @ (ExprKind::Nil
            | ExprKind::Bool(_)
            | ExprKind::Number(_)
            | ExprKind::String(_)
            | ExprKind::Keyword(_)
            | ExprKind::Symbol(_)
            | ExprKind::Global(_)
            | ExprKind::CoreFn(_)
            | ExprKind::JsGlobal(_)) => kind,
            ExprKind::Local(local) => match self.parameter(local)? {
                Parameter::Value(argument) => {
                    let argument = *argument;
                    return Some(self.duplicate(argument));
                }
                Parameter::Local(local) => ExprKind::Local(*local),
                Parameter::Moved(argument) => return Some(*argument),
                Parameter::Unread => return None,
            },
            ExprKind::Let { bindings, body } => {
                let mut copies = Vec::with_capacity(bindings.len());
                for (local, init) in bindings {
                    let init = self.copy(init)?;
                    let copy = fresh(self.program, local);
                    self.parameters.push((local, Parameter::Local(copy)));
                    copies.push((copy, init));
                }
                ExprKind::Let {
                    bindings: copies,
                    body: self.copy(body)?,
                }
            }
            ExprKind::If { test, yes, no } => ExprKind::If {
                test: self.copy(test)?,
                yes: self.copy(yes)?,
                no: self.copy(no)?,
            },
            ExprKind::Do(items) => ExprKind::Do(self.all(&items)?),
            ExprKind::BooleanHint(inner) => ExprKind::BooleanHint(self.copy(inner)?),
            ExprKind::List(items) => ExprKind::List(self.all(&items)?),
            ExprKind::Vector(items) => ExprKind::Vector(self.all(&items)?),
            ExprKind::Map(items) => ExprKind::Map(self.all(&items)?),
            ExprKind::Set(items) => ExprKind::Set(self.all(&items)?),
            ExprKind::LiteralMap(items) => ExprKind::LiteralMap(self.all(&items)?),
            ExprKind::LiteralSet(items) => ExprKind::LiteralSet(self.all(&items)?),
            ExprKind::Call { callee, args } => ExprKind::Call {
                callee: self.copy(callee)?,
                args: self.all(&args)?,
            },
            ExprKind::Property { receiver, property } => ExprKind::Property {
                receiver: self.copy(receiver)?,
                property,
            },
            ExprKind::MethodCall {
                receiver,
                method,
                args,
            } => ExprKind::MethodCall {
                receiver: self.copy(receiver)?,
                method,
                args: self.all(&args)?,
            },
            ExprKind::JsIndex { receiver, indices } => ExprKind::JsIndex {
                receiver: self.copy(receiver)?,
                indices: self.all(&indices)?,
            },
            ExprKind::Construct { callee, args } => ExprKind::Construct {
                callee: self.copy(callee)?,
                args: self.all(&args)?,
            },
            ExprKind::InstanceOf { class, value } => ExprKind::InstanceOf {
                class: self.copy(class)?,
                value: self.copy(value)?,
            },
            ExprKind::WithMeta { value, metadata } => ExprKind::WithMeta {
                value: self.copy(value)?,
                metadata: self.copy(metadata)?,
            },
            _ => return None,
        };
        // Map keys and set members that are not constants must not become
        // ones ([`crate::folding::fenced_positions`]).
        let program = &*self.program;
        let keys = |kind: &ExprKind| -> Vec<ExprId> {
            match kind {
                ExprKind::Map(items) | ExprKind::LiteralMap(items) => {
                    items.iter().step_by(2).copied().collect()
                }
                ExprKind::Set(items) | ExprKind::LiteralSet(items) => items.clone(),
                _ => Vec::new(),
            }
        };
        let constant = |id: &ExprId| constant_like(program, &program.exprs[id.0].kind);
        if keys(&kind)
            .iter()
            .zip(&keys(&program.exprs[id.0].kind))
            .any(|(copy, original)| constant(copy) && !constant(original))
        {
            return None;
        }
        Some(self.push(id, kind))
    }

    /// A copy of the trivial argument `id`, its collection items included.
    fn duplicate(&mut self, id: ExprId) -> ExprId {
        let mut kind = self.program.exprs[id.0].kind.clone();
        if let ExprKind::Vector(items)
        | ExprKind::List(items)
        | ExprKind::Map(items)
        | ExprKind::Set(items)
        | ExprKind::LiteralMap(items)
        | ExprKind::LiteralSet(items) = &mut kind
        {
            for item in items.iter_mut() {
                *item = self.duplicate(*item);
            }
        }
        self.push(id, kind)
    }

    /// A new expression of `kind` where `like` was.
    fn push(&mut self, like: ExprId, kind: ExprKind) -> ExprId {
        let original = &self.program.exprs[like.0];
        let expr = Expr {
            kind,
            span: original.span.clone(),
            context: original.context,
            source_index: original.source_index,
        };
        self.program.exprs.push(expr);
        let copy = ExprId(self.program.exprs.len() - 1);
        let mark = self.built.get(like.0).copied().unwrap_or(false);
        self.built.resize(self.program.exprs.len(), false);
        self.built[copy.0] = mark;
        copy
    }
}
