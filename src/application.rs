//! Conservative, optional application preparation over Rust semantic IR.
//! Compile-time unit emission never enters this boundary.
use crate::analyzer::{CoreFnId, ExprId, ExprKind, GlobalId, MethodDispatch, Program};
use crate::core_operations::{Calls, Purity, Type};
use crate::program_fragment::children;
use std::collections::HashMap;

#[cfg(test)]
thread_local! { static PREPARATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

/// How much whole-program work application preparation does before emission.
/// Levels are ordered: each one includes everything the previous one does.
/// See docs/advanced-optimization.md for the roadmap behind `Advanced`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OptimizationLevel {
    /// Emit every definition and call as analyzed. This output is the
    /// differential oracle that every other level must agree with.
    None,
    /// Conservative preparation: remove discarded definitions with inert
    /// initializers and specialize eager top-level calls. The default.
    #[default]
    Prepare,
    /// Release mode. It accepts the deliberate semantic differences listed
    /// under "Advanced-mode contract" in docs/advanced-optimization.md. So far
    /// it prepares like `Prepare` except that it also removes definitions whose
    /// initializers only call pure core operations, branch, bind locals or
    /// construct program types, and does not treat retained `js*` as an
    /// escape. Emission embeds just the runtime units the program reaches
    /// (`crate::runtime_slice`). Later passes of that design hook in through
    /// [`OptimizationLevel::advanced`].
    Advanced,
}
impl OptimizationLevel {
    /// Every level from weakest to strongest, for differential tests.
    pub const ALL: [Self; 3] = [Self::None, Self::Prepare, Self::Advanced];

    /// The CLI spelling accepted by `--optimize` and [`str::parse`].
    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Prepare => "prepare",
            Self::Advanced => "advanced",
        }
    }
    /// Whether the conservative removal and specialization passes run.
    pub fn prepares(self) -> bool {
        self >= Self::Prepare
    }
    /// Whether release-only passes may run. They must preserve results
    /// relative to `None` except under the advanced-mode contract.
    pub fn advanced(self) -> bool {
        self >= Self::Advanced
    }
}
/// `false` is [`OptimizationLevel::None`] and `true` is
/// [`OptimizationLevel::Prepare`], the meaning of the former boolean option.
impl From<bool> for OptimizationLevel {
    fn from(optimize: bool) -> Self {
        if optimize { Self::Prepare } else { Self::None }
    }
}
impl std::fmt::Display for OptimizationLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}
/// An optimization level name other than `none`, `prepare` or `advanced`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownOptimizationLevel(pub String);
impl std::fmt::Display for UnknownOptimizationLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "unknown optimization level {:?}: expected none, prepare or advanced",
            self.0
        )
    }
}
impl std::error::Error for UnknownOptimizationLevel {}
impl std::str::FromStr for OptimizationLevel {
    type Err = UnknownOptimizationLevel;
    fn from_str(name: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|level| level.name() == name)
            .ok_or_else(|| UnknownOptimizationLevel(name.to_owned()))
    }
}

#[derive(Clone, Debug, Default)]
pub struct ApplicationOptions {
    pub optimize: OptimizationLevel,
    /// Keep these runtime binding identities and all their source generations.
    pub exports: Vec<GlobalId>,
}
impl ApplicationOptions {
    /// Options at `optimize` without explicit exports. Accepts a level or the
    /// former boolean (`false` is `None`, `true` is `Prepare`).
    pub fn new(optimize: impl Into<OptimizationLevel>) -> Self {
        Self {
            optimize: optimize.into(),
            exports: Vec::new(),
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ApplicationStats {
    pub removed_definitions: usize,
    pub specialized_calls: usize,
    pub retained_globals: usize,
}
/// An owned semantic program plus emission decisions computed entirely in Rust.
/// Repeated emission does not rerun preparation or execute compiler callbacks.
pub struct PreparedApplication {
    level: OptimizationLevel,
    program: Program,
    globals: Vec<bool>,
    calls: HashMap<ExprId, usize>,
    stats: ApplicationStats,
}
impl PreparedApplication {
    fn emit(&self, module: bool) -> (String, crate::runtime_slice::EmittedRuntime) {
        crate::emitter::emit_application(
            &self.program,
            &self.globals,
            &self.calls,
            self.level,
            module,
        )
    }
    /// One JavaScript expression. At `Advanced` it embeds only the runtime
    /// units the program reaches; see [`PreparedApplication::retention`].
    pub fn emit_js(&self) -> String {
        self.emit(false).0
    }
    /// One ES module for a bundler; see [`crate::emitter::emit_esm`]. Its
    /// runtime is the one [`PreparedApplication::emit_js`] embeds.
    pub fn emit_esm(&self) -> String {
        self.emit(true).0
    }
    /// Which runtime units the emitted output embeds, each with its size and
    /// the runtime name and source expression that first demanded it. Below
    /// `Advanced` the output embeds the whole runtime and the report
    /// attributes what it can. `--size-report` prints this table.
    pub fn retention(&self) -> crate::RetentionReport {
        let (_, runtime) = self.emit(false);
        crate::RetentionReport::new(&self.program, self.level, &runtime)
    }
    pub fn stats(&self) -> &ApplicationStats {
        &self.stats
    }
    /// The level this application was prepared at. Level-specific emission
    /// (for example runtime slicing under `Advanced`) reads it here.
    pub fn level(&self) -> OptimizationLevel {
        self.level
    }
}

/// Preserve the entry result, explicit exports, and ordered observable effects.
/// Only discarded top-level definitions with provably inert initializers are
/// candidates for removal. Unknown calls, throws, metadata tests, declaration
/// bundles and standalone expressions stay in their original execution order.
pub fn prepare_application(program: &Program, options: ApplicationOptions) -> PreparedApplication {
    #[cfg(test)]
    PREPARATIONS.with(|count| count.set(count.get() + 1));
    let level = options.optimize;
    let mut prepared = PreparedApplication {
        level,
        program: program.clone(),
        globals: vec![false; program.globals.len()],
        calls: HashMap::new(),
        stats: ApplicationStats::default(),
    };
    let mut top = Vec::new();
    flatten(program, program.root, true, &mut top);
    let mut candidates = vec![false; program.exprs.len()];
    let mut definitions = vec![Vec::new(); program.globals.len()];
    let mut pending = Vec::new();
    let mut inert = Inert::new(program, &top, level.advanced());
    for (position, &(id, result)) in top.iter().enumerate() {
        if let ExprKind::Def {
            global, init, test, ..
        } = program.exprs[id.0].kind
        {
            definitions[global.0].push(id);
            if level.prepares()
                && !result
                && test.is_none()
                && init.is_none_or(|i| inert.initializer(i, position))
            {
                candidates[id.0] = true;
                continue;
            }
        }
        pending.push(id);
    }
    for g in &options.exports {
        if let Some(defs) = definitions.get(g.0) {
            pending.extend(defs);
        }
    }
    let mut visited = vec![false; program.exprs.len()];
    let mut raw = false;
    while let Some(id) = pending.pop() {
        if std::mem::replace(&mut visited[id.0], true) {
            continue;
        }
        // Below `Advanced`, retained raw JS is an application escape: every
        // top-level form becomes a root and nothing is specialized. A discarded
        // function containing JS is inert and must not turn its compile-time
        // uses into roots. `Advanced` treats `js*` as an opaque leaf whose holes
        // are ordinary children: hygiene renames every compiler-bound name away
        // from raw text, which can therefore name only its holes and host
        // globals (Phase 2 step 7 of docs/advanced-optimization.md).
        if !raw
            && !level.advanced()
            && matches!(program.exprs[id.0].kind, ExprKind::JsTemplate { .. })
        {
            raw = true;
            pending.extend(top.iter().map(|(id, _)| *id));
        }
        for global in globals(&program.exprs[id.0].kind) {
            if !std::mem::replace(&mut prepared.globals[global.0], true) {
                pending.extend(&definitions[global.0]);
            }
        }
        pending.extend(children(program, id));
    }
    for (index, candidate) in candidates.iter().enumerate() {
        if *candidate && !visited[index] {
            prepared.program.exprs[index].kind = ExprKind::Nil;
            prepared.stats.removed_definitions += 1;
        }
    }
    if level.prepares() && !raw {
        specialize(&mut prepared, &top);
    }
    if !level.prepares() {
        prepared.globals.fill(true);
    }
    // Release-only passes (docs/advanced-optimization.md, Phases 1-3) run here,
    // after conservative preparation, under `level.advanced()`. So far `Advanced`
    // prepares like `Prepare` except that more initializers are inert (`Inert`)
    // and retained `js*` is not an escape; emission also slices the runtime.
    prepared.stats.retained_globals = prepared.globals.iter().filter(|b| **b).count();
    prepared.stats.specialized_calls += prepared.calls.len();
    prepared
}

fn flatten(program: &Program, id: ExprId, result: bool, out: &mut Vec<(ExprId, bool)>) {
    if let ExprKind::Do(items) = &program.exprs[id.0].kind {
        for (index, child) in items.iter().enumerate() {
            flatten(program, *child, result && index + 1 == items.len(), out);
        }
    } else {
        out.push((id, result));
    }
}
/// What an inert expression evaluates to, as far as the calls that consume it
/// need to know: its type in the core operation lattice and whether it is
/// *plain data* (see `crate::core_operations`), which `dispatch` operations
/// hash, compare and traverse without running program code.
#[derive(Clone, Copy)]
struct Shape {
    ty: Type,
    plain: bool,
}
impl Shape {
    /// A value of type `ty`, plain when every value of that type is a scalar.
    fn of(ty: Type) -> Self {
        Self {
            ty,
            plain: SCALAR.contains(ty),
        }
    }
    fn opaque(ty: Type) -> Self {
        Self { ty, plain: false }
    }
}
const SCALAR: Type = Type::NIL
    .union(Type::BOOLEAN)
    .union(Type::NUMBER)
    .union(Type::STRING)
    .union(Type::KEYWORD)
    .union(Type::SYMBOL);

/// A constructor whose call only stores its arguments: a program type defined
/// once, by a top-level `deftype` or `defrecord` that never gets reassigned,
/// or that type's positional factory (`->T`).
#[derive(Clone, Copy)]
struct Constructor {
    /// Index in the flattened top level of the defining form.
    position: usize,
    factory: bool,
    instance: Type,
}

/// Decides which discarded top-level initializers are inert: evaluating them
/// has no observable effect, cannot throw and runs no program code, so removing
/// an unread definition is unobservable.
///
/// `Prepare` accepts primitive literals, functions, core values, lists and
/// vectors of inert values, and maps and sets with scalar keys. `Advanced`
/// (Phase 2 step 6 of docs/advanced-optimization.md) also accepts local reads,
/// `let`, `if` and `do` over inert parts, metadata on inert values through the
/// `with-meta` row, calls of core operations whose row in
/// `src/core_operations.tsv` is `pure` or `allocates` for the argument types
/// and whose arguments run no code (`-` calls, or `dispatch` over plain data),
/// and `new` or `->T` of a [`Constructor`] after its definition. `throws`,
/// `effect` and `invoke` rows stay effectful.
///
/// The core rows hold under the program-wide conditions of the
/// `crate::core_operations` module docs, checked here: a core operation that
/// the program may replace (`set!`, `with-redefs`, or a `js*` hole, which raw
/// text could assign) is not trusted, and when the program extends a core
/// protocol to anything but its own types (`extend-type number`, `default`, a
/// runtime collection, `specify!`), no core call is inert.
struct Inert<'p> {
    program: &'p Program,
    advanced: bool,
    core_calls: bool,
    /// Core operations the program may replace, by table row.
    replaced: Vec<bool>,
    constructors: Vec<Option<Constructor>>,
    /// The shape of each `let` binding examined so far; every local has one
    /// binding site.
    locals: Vec<Option<Shape>>,
    /// Index in the flattened top level of the definition being examined.
    position: usize,
}
impl<'p> Inert<'p> {
    fn new(program: &'p Program, top: &[(ExprId, bool)], advanced: bool) -> Self {
        let mut inert = Self {
            program,
            advanced,
            core_calls: false,
            replaced: Vec::new(),
            constructors: Vec::new(),
            locals: Vec::new(),
            position: 0,
        };
        if !advanced {
            return inert;
        }
        // Program-wide, including code that may never run: how often each
        // global is defined, which globals and core operations may be
        // reassigned, and which targets core protocols are extended to.
        let mut definitions = vec![0usize; program.globals.len()];
        let mut assigned = vec![false; program.globals.len()];
        let mut replaced = vec![false; CoreFnId::ALL.len()];
        let mut extended = Vec::new();
        // Extensions name core protocols and their methods by `Builtin` keys,
        // and also mark the program's own protocols with arity-0 `ns/Name` ones.
        let markers: Vec<_> = program
            .protocols
            .keys()
            .map(|protocol| {
                let binding = &program.globals[protocol.0];
                format!("{}/{}", binding.namespace, binding.name)
            })
            .collect();
        let core_protocol = |(dispatch, method): &(MethodDispatch, crate::analyzer::FnMethod)| {
            matches!(dispatch, MethodDispatch::Builtin(key)
                if !(method.params.is_empty() && method.rest.is_none() && markers.contains(key)))
        };
        // Assigning a property of a core value (`PersistentHashMap.fromArrays`)
        // counts as replacing it.
        let mut assign = |target: &ExprId| {
            let target = match program.exprs[target.0].kind {
                ExprKind::Property { receiver, .. } | ExprKind::JsIndex { receiver, .. } => {
                    receiver
                }
                _ => *target,
            };
            match program.exprs[target.0].kind {
                ExprKind::CoreFn(core) => replaced[core.table_row()] = true,
                ExprKind::Global(global) => assigned[global.0] = true,
                _ => (),
            }
        };
        for expr in &program.exprs {
            match &expr.kind {
                ExprKind::SetGlobal { target, .. } => assign(target),
                ExprKind::Binding { bindings, .. } => {
                    bindings.iter().for_each(|(target, _)| assign(target))
                }
                // Raw text may assign its holes.
                ExprKind::JsTemplate { args, .. } => args.iter().for_each(&mut assign),
                ExprKind::ExtendType {
                    target, methods, ..
                } if methods.iter().any(core_protocol) => extended.push(*target),
                ExprKind::Global(_) => (),
                kind => {
                    for global in globals(kind) {
                        definitions[global.0] += 1;
                    }
                }
            }
        }
        inert.replaced = replaced;
        inert.locals = vec![None; program.locals.len()];
        inert.constructors = vec![None; program.globals.len()];
        let sole = |global: GlobalId| definitions[global.0] == 1 && !assigned[global.0];
        for (position, &(id, _)) in top.iter().enumerate() {
            if let ExprKind::DefType {
                record,
                global,
                constructor,
                ..
            } = program.exprs[id.0].kind
                && sole(global)
            {
                let instance = if record { Type::RECORD } else { Type::OBJECT };
                inert.constructors[global.0] = Some(Constructor {
                    position,
                    factory: false,
                    instance,
                });
                if let Some(factory) = constructor.filter(|factory| sole(*factory)) {
                    inert.constructors[factory.0] = Some(Constructor {
                        position,
                        factory: true,
                        instance,
                    });
                }
            }
        }
        inert.core_calls = extended.iter().all(|target| {
            matches!(program.exprs[target.0].kind, ExprKind::Global(global)
                if inert.constructors[global.0].is_some_and(|c| !c.factory))
        });
        inert
    }

    /// Whether the initializer of the definition at `position` in the
    /// flattened top level is inert.
    fn initializer(&mut self, init: ExprId, position: usize) -> bool {
        self.position = position;
        self.shape(init).is_some()
    }

    /// The shape of `id` when it is inert.
    fn shape(&mut self, id: ExprId) -> Option<Shape> {
        use ExprKind::*;
        let program = self.program;
        Some(match &program.exprs[id.0].kind {
            Nil => Shape::of(Type::NIL),
            Bool(_) => Shape::of(Type::BOOLEAN),
            Number(_) => Shape::of(Type::NUMBER),
            String(_) => Shape::of(Type::STRING),
            Keyword(_) => Shape::of(Type::KEYWORD),
            Symbol(_) => Shape::of(Type::SYMBOL),
            CoreFn(core) => Shape::opaque(core.operation().value),
            Fn { .. } => Shape::opaque(Type::FN),
            Do(items) => {
                let mut last = Shape::of(Type::NIL);
                for item in items {
                    last = self.shape(*item)?;
                }
                last
            }
            List(items) | Vector(items) => {
                let mut plain = true;
                for item in items {
                    plain &= self.shape(*item)?.plain;
                }
                let ty = if matches!(program.exprs[id.0].kind, List(_)) {
                    Type::LIST
                } else {
                    Type::VECTOR
                };
                Shape { ty, plain }
            }
            Map(items) | LiteralMap(items) => {
                let mut plain = true;
                for pair in items.as_chunks::<2>().0 {
                    if !scalar(program, pair[0]) {
                        return None;
                    }
                    plain &= self.shape(pair[1])?.plain;
                }
                Shape {
                    ty: Type::MAP,
                    plain,
                }
            }
            Set(items) | LiteralSet(items) => {
                if !items.iter().all(|id| scalar(program, *id)) {
                    return None;
                }
                Shape {
                    ty: Type::SET,
                    plain: true,
                }
            }
            // Non-scalar hashing, reader literals, metadata and unknown operations
            // may throw or dispatch user code and are deliberately not pure here.
            _ if !self.advanced => return None,
            Local(local) => self.locals[local.0].unwrap_or(Shape::opaque(Type::ANY)),
            Let { bindings, body } => {
                for (local, init) in bindings {
                    self.locals[local.0] = Some(self.shape(*init)?);
                }
                self.shape(*body)?
            }
            If { test, yes, no } => {
                self.shape(*test)?;
                let (yes, no) = (self.shape(*yes)?, self.shape(*no)?);
                Shape {
                    ty: yes.ty.union(no.ty),
                    plain: yes.plain && no.plain,
                }
            }
            WithMeta { value, metadata } => {
                let arguments = [self.shape(*value)?, self.shape(*metadata)?];
                Shape::opaque(self.core_call(CoreFnId::WithMeta, &arguments)?)
            }
            Call { callee, args } => {
                if let Some((keys, values)) = hash_map_expansion(program, *callee, args) {
                    return self.hash_map(keys, values);
                }
                let arguments = args
                    .iter()
                    .map(|arg| self.shape(*arg))
                    .collect::<Option<Vec<_>>>()?;
                match program.exprs[callee.0].kind {
                    CoreFn(core) => Shape::of(self.core_call(core, &arguments)?),
                    Global(global) => Shape::opaque(self.construct(global, true)?),
                    _ => return None,
                }
            }
            Construct { callee, args } => {
                let Global(global) = program.exprs[callee.0].kind else {
                    return None;
                };
                let instance = self.construct(global, false)?;
                for arg in args {
                    self.shape(*arg)?;
                }
                Shape::opaque(instance)
            }
            _ => return None,
        })
    }

    /// The result type of an inert call of `core` on arguments of these shapes.
    fn core_call(&self, core: CoreFnId, arguments: &[Shape]) -> Option<Type> {
        if !self.core_calls || self.replaced[core.table_row()] {
            return None;
        }
        let types: Vec<_> = arguments.iter().map(|shape| shape.ty).collect();
        let call = core.operation().call(&types);
        let runs_no_code = match call.calls {
            Calls::None => true,
            Calls::Dispatch => arguments.iter().all(|shape| shape.plain),
            Calls::Invoke => false,
        };
        (call.purity <= Purity::Allocates && runs_no_code).then_some(call.returns)
    }

    /// A `hash-map` call that its compiler macro expanded: the `array` calls
    /// and the `hash-map` row on the interleaved operands, in source order.
    fn hash_map(&mut self, keys: &[ExprId], values: &[ExprId]) -> Option<Shape> {
        if self.replaced[CoreFnId::PersistentHashMap.table_row()] {
            return None;
        }
        let mut operands = [Vec::new(), Vec::new()];
        for (array, operands) in [keys, values].into_iter().zip(&mut operands) {
            for operand in array {
                operands.push(self.shape(*operand)?);
            }
            self.core_call(CoreFnId::Array, operands)?;
        }
        let [keys, values] = operands;
        let pairs: Vec<_> = keys
            .into_iter()
            .zip(values)
            .flat_map(|(k, v)| [k, v])
            .collect();
        Some(Shape::opaque(self.core_call(CoreFnId::HashMap, &pairs)?))
    }

    /// The instance type of `new global` (or of calling it, for a `factory`)
    /// when it only stores its arguments and its definition has run.
    fn construct(&self, global: GlobalId, factory: bool) -> Option<Type> {
        let constructor = self.constructors.get(global.0).copied().flatten()?;
        (constructor.factory == factory && constructor.position < self.position)
            .then_some(constructor.instance)
    }
}

/// The key and value operands of `(PersistentHashMap.fromArrays (array k ...)
/// (array v ...))`, the expansion of a `hash-map` call with arguments
/// (src/compiler_core_macros.cljs), when every key has a value.
fn hash_map_expansion<'p>(
    program: &'p Program,
    callee: ExprId,
    args: &'p [ExprId],
) -> Option<(&'p [ExprId], &'p [ExprId])> {
    let ExprKind::Property { receiver, property } = &program.exprs[callee.0].kind else {
        return None;
    };
    let receiver = &program.exprs[receiver.0].kind;
    if property != "fromArrays"
        || !matches!(receiver, ExprKind::CoreFn(CoreFnId::PersistentHashMap))
    {
        return None;
    }
    let array = |id: &ExprId| match &program.exprs[id.0].kind {
        ExprKind::Call { callee, args }
            if matches!(
                program.exprs[callee.0].kind,
                ExprKind::CoreFn(CoreFnId::Array)
            ) =>
        {
            Some(args.as_slice())
        }
        _ => None,
    };
    let [keys, values] = args else {
        return None;
    };
    let (keys, values) = (array(keys)?, array(values)?);
    (keys.len() == values.len()).then_some((keys, values))
}
fn scalar(program: &Program, id: ExprId) -> bool {
    matches!(
        program.exprs[id.0].kind,
        ExprKind::Nil
            | ExprKind::Bool(_)
            | ExprKind::Number(_)
            | ExprKind::String(_)
            | ExprKind::Keyword(_)
            | ExprKind::Symbol(_)
    )
}
fn globals(kind: &ExprKind) -> Vec<GlobalId> {
    match kind {
        ExprKind::Global(g) | ExprKind::Def { global: g, .. } => vec![*g],
        ExprKind::DefProtocol {
            global, methods, ..
        } => std::iter::once(*global)
            .chain(methods.iter().map(|m| m.global))
            .collect(),
        ExprKind::DefType {
            global,
            constructor,
            map_constructor,
            ..
        } => std::iter::once(*global)
            .chain(*constructor)
            .chain(*map_constructor)
            .collect(),
        _ => Vec::new(),
    }
}
fn reachable(program: &Program, root: ExprId) -> Vec<ExprId> {
    let mut pending = vec![root];
    let mut visited = vec![false; program.exprs.len()];
    let mut result = Vec::new();
    while let Some(id) = pending.pop() {
        if std::mem::replace(&mut visited[id.0], true) {
            continue;
        }
        result.push(id);
        pending.extend(children(program, id));
    }
    result
}
fn unwrap_do(program: &Program, mut id: ExprId) -> ExprId {
    while let ExprKind::Do(items) = &program.exprs[id.0].kind {
        if items.len() != 1 {
            break;
        }
        id = items[0];
    }
    id
}
fn dynamic(program: &Program, g: GlobalId) -> bool {
    let Some(id) = program.globals[g.0].metadata else {
        return false;
    };
    let ExprKind::Map(items) = &program.exprs[id.0].kind else {
        return true;
    };
    items.as_chunks::<2>().0.iter().any(|pair| {
        let ExprKind::Keyword(name) = program.exprs[pair[0].0].kind else {
            return false;
        };
        let name = &program.named_literals[name.0];
        name.namespace.is_none()
            && program.strings[name.name.0] == "dynamic".encode_utf16().collect::<Vec<_>>()
            && !matches!(
                program.exprs[pair[1].0].kind,
                ExprKind::Nil | ExprKind::Bool(false)
            )
    })
}
fn specialize(prepared: &mut PreparedApplication, top: &[(ExprId, bool)]) {
    let p = &prepared.program;
    let nodes = reachable(p, p.root);
    let mut defs = vec![Vec::new(); p.globals.len()];
    let mut escaped = vec![false; p.globals.len()];
    let mut callees = vec![false; p.exprs.len()];
    for &id in &nodes {
        if let ExprKind::Call { callee, .. } = p.exprs[id.0].kind {
            callees[callee.0] = true;
        }
    }
    for &id in &nodes {
        match &p.exprs[id.0].kind {
            ExprKind::Def {
                global,
                init,
                once: false,
                test: None,
            } => defs[global.0].push(*init),
            ExprKind::Def { global, .. } => escaped[global.0] = true,
            ExprKind::Global(g) if !callees[id.0] => escaped[g.0] = true,
            _ => (),
        }
    }
    // Specialize only eager top-level calls after the sole initialization. Calls
    // inside functions/branches retain late lookup: no initialization dominance
    // or interprocedural effect assumptions are smuggled into emission.
    let mut initialized = vec![false; p.globals.len()];
    let mut core_calls = Vec::new();
    for &(id, _) in top {
        if let ExprKind::Def { global, .. } = p.exprs[id.0].kind {
            initialized[global.0] = true;
        }
        let ExprKind::Call { callee, ref args } = p.exprs[id.0].kind else {
            continue;
        };
        let ExprKind::Global(g) = p.exprs[callee.0].kind else {
            continue;
        };
        if escaped[g.0] || !initialized[g.0] || dynamic(p, g) || defs[g.0].len() != 1 {
            continue;
        }
        let Some(init) = defs[g.0][0] else {
            continue;
        };
        let init = unwrap_do(p, init);
        if let ExprKind::Fn { methods, .. } = &p.exprs[init.0].kind {
            // Multi-arity wrappers publish these canonical fixed arity entries.
            // Keep .call(null) so this/arguments/recur and argument effects match.
            if methods.len() > 1
                && methods
                    .iter()
                    .filter(|m| m.rest.is_none() && m.params.len() == args.len())
                    .count()
                    == 1
            {
                prepared.calls.insert(id, args.len());
            }
        } else if let ExprKind::CoreFn(core) = p.exprs[init.0].kind {
            core_calls.push((id, callee, core));
        }
    }
    for (id, callee, core) in core_calls {
        let mut expr = prepared.program.exprs[callee.0].clone();
        expr.kind = ExprKind::CoreFn(core);
        let callee = ExprId(prepared.program.exprs.len());
        prepared.program.exprs.push(expr);
        if let ExprKind::Call { callee: target, .. } = &mut prepared.program.exprs[id.0].kind {
            *target = callee;
        }
        prepared.stats.specialized_calls += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn optimization_levels_parse_print_order_and_convert_from_the_former_boolean() {
        for level in OptimizationLevel::ALL {
            assert_eq!(level.name().parse(), Ok(level));
            assert_eq!(level.to_string(), level.name());
        }
        assert_eq!(
            "Advanced".parse::<OptimizationLevel>(),
            Err(UnknownOptimizationLevel("Advanced".into()))
        );
        assert!(OptimizationLevel::None < OptimizationLevel::Prepare);
        assert!(OptimizationLevel::Prepare < OptimizationLevel::Advanced);
        assert_eq!(
            OptimizationLevel::ALL.map(|l| (l.prepares(), l.advanced())),
            [(false, false), (true, false), (true, true)]
        );
        assert_eq!(OptimizationLevel::from(false), OptimizationLevel::None);
        assert_eq!(OptimizationLevel::from(true), OptimizationLevel::Prepare);
        assert_eq!(
            ApplicationOptions::default().optimize,
            OptimizationLevel::Prepare
        );
        let p = crate::analyze(
            "(def unused 1) 2",
            &crate::parse("(def unused 1) 2").unwrap(),
        )
        .unwrap();
        for level in OptimizationLevel::ALL {
            let prepared = prepare_application(&p, ApplicationOptions::new(level));
            assert_eq!(prepared.level(), level);
            assert_eq!(
                prepared.stats().removed_definitions,
                usize::from(level.prepares())
            );
        }
    }
    #[test]
    fn macro_calls_do_not_prepare_applications_or_recompile_unchanged_units() {
        let source = "(def factor 2) (defn helper [x] (* factor x)) (defmacro twice [x] (helper x)) (twice 21) (twice 21) (twice 21)";
        PREPARATIONS.with(|count| count.set(0));
        let p = crate::analyze(source, &crate::parse(source).unwrap()).unwrap();
        PREPARATIONS.with(|count| assert_eq!(count.get(), 0));
        for name in ["cljs.user/factor", "cljs.user/helper"] {
            let uses: Vec<_> = p
                .compilation_uses
                .iter()
                .filter(|u| u.name == name)
                .collect();
            assert_eq!(uses.len(), 1, "{name}: {:?}", p.compilation_uses);
            assert_eq!(uses[0].analyses, 1);
            assert_eq!(uses[0].emissions, 1);
        }
        let prepared = prepare_application(&p, Default::default());
        assert_eq!(prepared.stats().removed_definitions, 2);
        let js = prepared.emit_js();
        assert_eq!(prepared.emit_js(), js);
        PREPARATIONS.with(|count| assert_eq!(count.get(), 1));
        assert!(!js.contains("__lightCompileUnit"));
        assert!(!js.contains("__lightCompilerRoots"));
    }
    #[test]
    fn generation_counters_distinguish_compile_demand_from_application_analysis() {
        let source = "(def x 1) (defmacro read-x [] x) (read-x) (read-x) (def x 2) (read-x) (read-x) (def unused 3) 42";
        let p = crate::analyze(source, &crate::parse(source).unwrap()).unwrap();
        let uses: Vec<_> = p
            .compilation_uses
            .iter()
            .filter(|u| u.name == "cljs.user/x")
            .collect();
        assert_eq!(uses.len(), 2);
        assert_eq!(uses[0].generation, 1);
        assert_eq!(uses[1].generation, 2);
        for u in uses {
            assert_eq!((u.analyses, u.emissions), (1, 1));
        }
        let unused = p
            .compilation_uses
            .iter()
            .find(|u| u.name == "cljs.user/unused")
            .unwrap();
        assert_eq!((unused.analyses, unused.emissions), (1, 0));
    }
    /// Removed definitions at `Prepare` and `Advanced`.
    fn removed(source: &str) -> [usize; 2] {
        let p = crate::analyze(source, &crate::parse(source).unwrap()).unwrap();
        [OptimizationLevel::Prepare, OptimizationLevel::Advanced]
            .map(|level| prepare_application(&p, ApplicationOptions::new(level)))
            .map(|prepared| prepared.stats().removed_definitions)
    }
    #[test]
    fn advanced_inertness_checks_replaced_operations_extensions_and_constructors() {
        assert_eq!(removed("(def a (vector 1)) (def b [1]) 42"), [1, 2]);
        // Replaced core operations, including through a property or a raw hole
        // that raw text could assign, are not trusted; other operations are.
        assert_eq!(
            removed("(set! cljs.core/vector list) (def a (vector 1)) 42"),
            [0, 0]
        );
        assert_eq!(
            removed("(with-redefs [vector list] 1) (def a (vector 1)) 42"),
            [0, 0]
        );
        assert_eq!(
            removed("(js* \"~{}\" vector) (def a (vector 1)) 42"),
            [0, 0]
        );
        assert_eq!(removed("(js* \"~{}\" list) (def a (vector 1)) 42"), [0, 1]);
        assert_eq!(
            removed("(def a (hash-map 1 2)) (def b (hash-map 1)) 42"),
            [0, 1]
        );
        assert_eq!(
            removed("(set! (.-fromArrays PersistentHashMap) nil) (def a (hash-map 1 2)) 42"),
            [0, 0]
        );
        // Core protocols extended beyond the program's own types make every
        // core call suspect; the program's own protocols and types do not.
        assert_eq!(
            removed("(extend-type number IHash (-hash [_] 0)) (def a (vector 1)) (def b [1]) 42"),
            [1, 1]
        );
        assert_eq!(
            removed(
                "(defprotocol P (m [x])) (extend-type number P (m [x] x)) (def a (vector 1)) 42"
            ),
            [0, 1]
        );
        assert_eq!(
            removed("(deftype T [x]) (extend-type T IHash (-hash [_] 0)) (def a (vector 1)) 42"),
            [0, 1]
        );
        // Construction after a sole top-level definition only stores fields.
        assert_eq!(
            removed(
                "(defrecord R [x]) (deftype T [x]) (def a (->R 1)) (def b (R. 1)) (def c (T. 1)) 42"
            ),
            [0, 3]
        );
        assert_eq!(
            removed("(defrecord R [x]) (def a (map->R {:x 1})) 42"),
            [0, 0]
        );
        assert_eq!(
            removed("(declare T) (def a (T. 1)) (deftype T [x]) 42"),
            [0, 0]
        );
        assert_eq!(
            removed("(deftype T [x]) (set! T nil) (def a (T. 1)) 42"),
            [0, 0]
        );
        assert_eq!(
            removed("(defrecord R [x]) (set! ->R vector) (def a (->R 1)) (def b (R. 1)) 42"),
            [0, 1]
        );
        // Local reads, `let`, `if`, `do` and metadata over inert parts.
        assert_eq!(
            removed("(def a (let [x (inc 1)] (if (pos? x) (do x [x]) ^:m {:x x}))) 42"),
            [0, 1]
        );
        // `throws`, `effect` and `invoke` rows, calls outside a row's domain and
        // `dispatch` rows over data that is not plain stay.
        assert_eq!(
            removed(
                "(def a (into [] [1])) (def b (mapv inc [1])) (def c (inc \"x\")) \
                 (def d (count (vector 1))) (def e (count [(fn [] 1)])) 42"
            ),
            [0, 0]
        );
    }
}
