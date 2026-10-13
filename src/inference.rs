//! Flow-insensitive type inference over a prepared program: Phase 2 step 3 of
//! docs/advanced-optimization.md (light-cljs-210).
//!
//! [`Inference::new`] gives every expression an [`Inferred`] value: a [`Type`]
//! of the core operation table's lattice ([`crate::core_operations`]) and, when
//! one holds, a [`Known`] representation that refines one of its classes.
//! `Advanced` emission reads it to drop `truth()` and to lower core calls to
//! JavaScript operators and field reads; later passes read the same result
//! (see "Phase 2 type inference extension points" in the design document).
//!
//! # What it infers
//!
//! - Literals, collection literals, `fn`, `new` and constructor calls of
//!   program types, and core values have their own types.
//! - A call of a core operation has the table's result type for its argument
//!   types ([`crate::core_operations::Operation::call`]). Calls that dispatch
//!   or invoke are `any` when the program extends a core protocol beyond its
//!   own types, and every call of an operation the program may replace is.
//! - A call of a known function (the [`Known::Function`] of its callee) has the
//!   union of its method bodies' types; a call of a global tagged `^boolean`
//!   and every `BooleanHint` are `boolean`, as upstream trusts these tags.
//! - A local bound by `let`, `loop` or `letfn` has the union of its
//!   initializer and, for a loop, of every `recur` argument for it. Parameters,
//!   `catch` and `this-as` bindings, type fields and locals the program
//!   assigns are `any`.
//! - A global defined exactly once (and never assigned, rebound or passed to
//!   `js*`) by `def`, `deftype`/`defrecord` or `defprotocol` has its
//!   initializer's type. A read can see `nil` before the definition runs,
//!   unless the definition is a top-level form and the read is in a later
//!   top-level form.
//! - In the branches of an `if`, a local the test examines is narrowed
//!   (upstream's `infer-if`): by its truthiness, by `nil?`, `some?`,
//!   `string?`, `number?`, `keyword?`, `symbol?`, `boolean?`, `true?`,
//!   `false?` and `array?`, by `vector?`, `map?`, `set?`, `seq?` and
//!   `record?` while core protocols are intact, by `instance?` of a program
//!   type and by `satisfies?` of a program protocol. Tests combine through
//!   `not`, through `and`/`or` (whose expansion binds the test to a
//!   temporary local) and through a local bound to a test. Only the then
//!   branch is narrowed by tests that a value of another class may pass.
//!
//! Types are computed to a fixed point: locals start at `never` and grow
//! with every binding, so loops and mutually recursive functions converge to
//! the least solution. Everything is monotone over a finite lattice; after
//! [`ROUND_LIMIT`] rounds the result degrades to `any` everywhere.
//!
//! # Soundness
//!
//! An expression's inferred value contains every value it can produce at run
//! time, under the program-wide conditions of `crate::core_operations` that
//! [`ProgramFacts`] checks, and with two trusted inputs: `^boolean` tags (as
//! upstream) and the table's assumption that protocol implementations return
//! what their protocol requires. Code that never runs may be typed `never`.

use std::collections::HashMap;

use crate::analyzer::{
    CoreFnId, ExprId, ExprKind, FnMethod, GlobalId, LocalId, MethodDispatch, Program,
};
use crate::compiler_data::CompilerLiteralKind;
use crate::core_operations::{Calls, Type};

/// The fixed point gives up after this many rounds and infers `any`
/// everywhere. Real programs converge in a handful.
pub const ROUND_LIMIT: usize = 64;

/// What is known about a value beyond its [`Type`]: a representation that
/// refines the values of one class. Values of the type's other classes are
/// unconstrained.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Known {
    /// Nothing beyond the type.
    Unknown,
    /// `fn` values made by this `Fn` expression.
    Function(ExprId),
    /// `record` or `object` values that are instances of this program type:
    /// the global a single `deftype` or `defrecord` defines.
    Instance(GlobalId),
    /// `vector` values whose payload is a flat array (vector literals and
    /// `vector`): never a map entry or subvector.
    Vector,
    /// `host` values that are JavaScript arrays.
    Array,
}

impl Known {
    /// The classes whose values this refines.
    pub const fn classes(self) -> Type {
        match self {
            Self::Unknown => Type::NEVER,
            Self::Function(_) => Type::FN,
            Self::Instance(_) => Type::RECORD.union(Type::OBJECT),
            Self::Vector => Type::VECTOR,
            Self::Array => Type::HOST,
        }
    }
}

/// An inferred value: its type and what else is known. Values join (`union`)
/// as types do; a [`Known`] survives a join with a value that has none of its
/// classes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inferred {
    pub ty: Type,
    pub known: Known,
}

impl Inferred {
    pub const NEVER: Self = Self::of(Type::NEVER);
    pub const ANY: Self = Self::of(Type::ANY);

    pub const fn of(ty: Type) -> Self {
        Self {
            ty,
            known: Known::Unknown,
        }
    }

    /// A value of type `ty` refined by `known`.
    pub fn known(ty: Type, known: Known) -> Self {
        Self { ty, known }.normalized()
    }

    fn normalized(self) -> Self {
        if self.ty.intersection(self.known.classes()).is_never() {
            Self::of(self.ty)
        } else {
            self
        }
    }

    /// The join: values of either.
    pub fn union(self, other: Self) -> Self {
        if self.ty.is_never() {
            return other;
        }
        if other.ty.is_never() {
            return self;
        }
        let known = if self.known == other.known {
            self.known
        } else if self.known == Known::Unknown
            && self.ty.intersection(other.known.classes()).is_never()
        {
            other.known
        } else if other.known == Known::Unknown
            && other.ty.intersection(self.known.classes()).is_never()
        {
            self.known
        } else {
            Known::Unknown
        };
        Self::known(self.ty.union(other.ty), known)
    }

    /// The values of classes `classes`.
    pub fn keep(self, classes: Type) -> Self {
        Self::known(self.ty.intersection(classes), self.known)
    }

    /// The values of classes other than `classes`.
    pub fn remove(self, classes: Type) -> Self {
        Self::known(self.ty.difference(classes), self.known)
    }

    /// Whether JavaScript truthiness agrees with Clojure's (only `nil` and
    /// `false` are false) for every value: no number (`0`, `-0`, `NaN`),
    /// string (`""`) or host value (`0n`, `document.all`).
    pub fn truth_is_js(self) -> bool {
        self.ty
            .intersection(Type::NUMBER.union(Type::STRING).union(Type::HOST))
            .is_never()
    }
}

/// How a global is defined, when the program defines it exactly once and
/// never assigns it (`set!`, `binding`, `with-redefs`, a `js*` hole).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Definition {
    /// Defined more than once, assigned, or not defined by the program.
    Unknown,
    /// `(def g ...)`: this `Def` expression.
    Def(ExprId),
    /// The type, positional factory (`->T`) or map factory (`map->T`) that this
    /// `DefType` expression defines, when its type global is sole too.
    Type {
        definition: ExprId,
        type_global: GlobalId,
        role: TypeRole,
        record: bool,
    },
    /// A protocol or protocol method that this `DefProtocol` defines.
    Protocol(ExprId),
}

/// Which global of a type definition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeRole {
    Constructor,
    Factory,
    MapFactory,
}

/// The classes of the values that satisfy a program protocol: every type the
/// program implements it on, and the single program type when there is one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Implementors {
    pub classes: Type,
    /// The only implementing program type, when nothing else implements it.
    pub sole: Option<GlobalId>,
}

/// Program-wide facts that the core operation table and inference rely on
/// (`crate::core_operations`, "What a pass may assume"), from one scan of
/// every expression, including code that may never run.
#[derive(Clone, Debug)]
pub struct ProgramFacts {
    /// Core operations the program may replace, by table row.
    replaced: Vec<bool>,
    /// Core protocols are extended only to the program's own types.
    core_protocols: bool,
    /// Some program type implements `IEquiv`.
    equiv: bool,
    globals: Vec<Definition>,
    /// Type globals the program implements `ILookup` on (in the definition
    /// or with `extend-type`).
    lookups: Vec<bool>,
    /// Top-level position of each direct top-level form, by expression.
    top: Vec<Option<usize>>,
    /// Program protocols by marker key (`ns/Name`).
    protocols: HashMap<String, Implementors>,
}

impl ProgramFacts {
    /// Scan `program`, whose flattened top-level forms are `top`.
    pub fn new<'p>(program: &'p Program, top: &[ExprId]) -> Self {
        let mut definitions = vec![0usize; program.globals.len()];
        let mut assigned = vec![false; program.globals.len()];
        let mut replaced = vec![false; CoreFnId::ALL.len()];
        let mut extended = Vec::new();
        let mut lookup_targets = Vec::new();
        // Extensions name core protocols and their methods by `Builtin` keys,
        // and also mark the program's own protocols with arity-0 `ns/Name` ones.
        let markers: Vec<String> = program
            .protocols
            .keys()
            .map(|protocol| {
                let binding = &program.globals[protocol.0];
                format!("{}/{}", binding.namespace, binding.name)
            })
            .collect();
        fn marker_key<'m>(
            markers: &[String],
            (dispatch, method): &'m (MethodDispatch, FnMethod),
        ) -> Option<&'m str> {
            match dispatch {
                MethodDispatch::Builtin(key)
                    if method.params.is_empty()
                        && method.rest.is_none()
                        && markers.contains(key) =>
                {
                    Some(key.as_str())
                }
                _ => None,
            }
        }
        let marker = |method: &'p (MethodDispatch, FnMethod)| marker_key(&markers, method);
        let core_protocol = |method: &(MethodDispatch, FnMethod)| {
            matches!(method.0, MethodDispatch::Builtin(_)) && marker_key(&markers, method).is_none()
        };
        let implements = |protocol: &'static str| {
            move |(dispatch, _): &(MethodDispatch, FnMethod)| {
                matches!(dispatch, MethodDispatch::Builtin(key)
                    if key == protocol
                        || key.strip_prefix(protocol).is_some_and(|rest| rest.starts_with('/')))
            }
        };
        let lookup = implements("ILookup");
        let mut equiv = false;
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
                } => {
                    if methods.iter().any(core_protocol) {
                        extended.push(*target);
                    }
                    if methods.iter().any(lookup) {
                        lookup_targets.push(*target);
                    }
                    equiv |= methods.iter().any(implements("IEquiv"));
                }
                ExprKind::Reify { methods, .. } => {
                    equiv |= methods.iter().any(implements("IEquiv"));
                }
                ExprKind::Global(_) => (),
                kind => {
                    if let ExprKind::DefType { methods, .. } = kind {
                        equiv |= methods.iter().any(implements("IEquiv"));
                    }
                    for global in crate::application::globals(kind) {
                        definitions[global.0] += 1;
                    }
                }
            }
        }
        let sole = |global: GlobalId| definitions[global.0] == 1 && !assigned[global.0];
        let mut globals = vec![Definition::Unknown; program.globals.len()];
        let mut lookups = vec![false; program.globals.len()];
        for (index, expr) in program.exprs.iter().enumerate() {
            let id = ExprId(index);
            match &expr.kind {
                ExprKind::Def { global, .. } if sole(*global) => {
                    globals[global.0] = Definition::Def(id);
                }
                ExprKind::DefType {
                    record,
                    global,
                    constructor,
                    map_constructor,
                    methods,
                    ..
                } if sole(*global) => {
                    let role = |role| Definition::Type {
                        definition: id,
                        type_global: *global,
                        role,
                        record: *record,
                    };
                    globals[global.0] = role(TypeRole::Constructor);
                    if let Some(factory) = constructor.filter(|factory| sole(*factory)) {
                        globals[factory.0] = role(TypeRole::Factory);
                    }
                    if let Some(factory) = map_constructor.filter(|factory| sole(*factory)) {
                        globals[factory.0] = role(TypeRole::MapFactory);
                    }
                    lookups[global.0] = methods.iter().any(lookup);
                }
                ExprKind::DefProtocol {
                    global, methods, ..
                } => {
                    for global in std::iter::once(*global).chain(methods.iter().map(|m| m.global)) {
                        if sole(global) {
                            globals[global.0] = Definition::Protocol(id);
                        }
                    }
                }
                _ => (),
            }
        }
        let program_type = |target: ExprId| match program.exprs[target.0].kind {
            ExprKind::Global(global) => match globals[global.0] {
                Definition::Type {
                    role: TypeRole::Constructor,
                    record,
                    ..
                } => Some((global, record)),
                _ => None,
            },
            _ => None,
        };
        // An extension to anything else is a core protocol extension beyond the
        // program's types, which `core_protocols` records.
        for target in lookup_targets {
            if let Some((global, _)) = program_type(target) {
                lookups[global.0] = true;
            }
        }
        let mut top_positions = vec![None; program.exprs.len()];
        for (position, id) in top.iter().enumerate() {
            top_positions[id.0] = Some(position);
        }
        // The core protocol condition holds when every extension of a core
        // protocol targets a program type defined once at top level.
        let core_protocols = extended.iter().all(|target| {
            matches!(program.exprs[target.0].kind, ExprKind::Global(global)
                if matches!(globals[global.0], Definition::Type {
                    role: TypeRole::Constructor,
                    definition,
                    ..
                } if top_positions[definition.0].is_some()))
        });
        // Who implements each program protocol: the classes, the program types,
        // and whether anything else does (`reify`, native types, `specify!`).
        let mut implementations: HashMap<&str, (Type, Vec<GlobalId>, bool)> = HashMap::new();
        let mut implement = |key: &'p str, classes: Type, program_type: Option<GlobalId>| {
            let entry = implementations
                .entry(key)
                .or_insert((Type::NEVER, Vec::new(), false));
            entry.0 = entry.0.union(classes);
            match program_type {
                Some(global) if !entry.1.contains(&global) => entry.1.push(global),
                Some(_) => (),
                None => entry.2 = true,
            }
        };
        for (index, expr) in program.exprs.iter().enumerate() {
            let (classes, program_type, methods) = match &expr.kind {
                ExprKind::DefType {
                    record,
                    global,
                    methods,
                    ..
                } => {
                    let own = matches!(globals[global.0], Definition::Type {
                        definition,
                        role: TypeRole::Constructor,
                        ..
                    } if definition.0 == index);
                    let class = if *record { Type::RECORD } else { Type::OBJECT };
                    (class, own.then_some(*global), methods)
                }
                ExprKind::Reify { methods, .. } => (Type::OBJECT, None, methods),
                ExprKind::ExtendType {
                    instance: false,
                    target,
                    methods,
                    ..
                } => match program_type(*target) {
                    Some((global, record)) => {
                        let class = if record { Type::RECORD } else { Type::OBJECT };
                        (class, Some(global), methods)
                    }
                    None => (native_classes(program, *target), None, methods),
                },
                ExprKind::ExtendType { methods, .. } => (Type::ANY, None, methods),
                _ => continue,
            };
            for key in methods.iter().filter_map(marker) {
                implement(key, classes, program_type);
            }
        }
        // A protocol extendable through metadata may be satisfied by any value
        // with metadata.
        let via_metadata: Vec<GlobalId> = program
            .exprs
            .iter()
            .filter_map(|expr| match expr.kind {
                ExprKind::DefProtocol {
                    global,
                    via_metadata: true,
                    ..
                } => Some(global),
                _ => None,
            })
            .collect();
        let mut protocols: HashMap<String, Implementors> = HashMap::new();
        for protocol in program.protocols.keys() {
            let binding = &program.globals[protocol.0];
            let key = format!("{}/{}", binding.namespace, binding.name);
            let implementors = match implementations.get(key.as_str()) {
                _ if via_metadata.contains(protocol) => Implementors {
                    classes: Type::ANY,
                    sole: None,
                },
                Some((classes, types, other)) => Implementors {
                    classes: *classes,
                    sole: match types.as_slice() {
                        [global] if !other => Some(*global),
                        _ => None,
                    },
                },
                None => Implementors {
                    classes: Type::NEVER,
                    sole: None,
                },
            };
            protocols.insert(key, implementors);
        }
        Self {
            replaced,
            core_protocols,
            equiv,
            globals,
            lookups,
            top: top_positions,
            protocols,
        }
    }

    /// Whether the program may replace this core operation: assigned with
    /// `set!`, `binding` or `with-redefs` (directly or through a property) or
    /// passed to a `js*` hole, which raw text could assign.
    pub fn replaced(&self, core: CoreFnId) -> bool {
        self.replaced[core.table_row()]
    }

    /// The runtime names of the core operations the program may replace,
    /// sorted and without duplicates (operations can share a runtime name).
    pub fn replaced_names(&self) -> Vec<&'static str> {
        let mut names = CoreFnId::ALL
            .into_iter()
            .filter(|core| self.replaced(*core))
            .map(CoreFnId::runtime_name)
            .collect::<Vec<_>>();
        names.sort_unstable();
        names.dedup();
        names
    }

    /// Whether core protocols are extended only to the program's own types
    /// (each defined once at top level), so that `dispatch` operations run only
    /// the runtime's implementations on runtime and host values.
    pub fn core_protocols_intact(&self) -> bool {
        self.core_protocols
    }

    /// Whether a `deftype`, `defrecord`, `reify` or `extend-type` of the program
    /// implements `IEquiv`. Otherwise `=` runs only the runtime's
    /// implementations (while core protocols are intact), and a record's
    /// compares only with records of its type.
    pub fn implements_equiv(&self) -> bool {
        self.equiv
    }

    /// Whether the table's description of calls of `core` holds: the program
    /// does not replace it, and it does not dispatch or core protocols are
    /// intact. `calls` is the call's [`Calls`].
    pub fn trusted(&self, core: CoreFnId, calls: Calls) -> bool {
        !self.replaced(core) && (calls == Calls::None || self.core_protocols)
    }

    /// How `global` is defined.
    pub fn definition(&self, global: GlobalId) -> Definition {
        self.globals
            .get(global.0)
            .copied()
            .unwrap_or(Definition::Unknown)
    }

    /// The program type whose constructor is `global`, and whether it is a
    /// record.
    pub fn program_type(&self, global: GlobalId) -> Option<(ExprId, bool)> {
        match self.definition(global) {
            Definition::Type {
                definition,
                role: TypeRole::Constructor,
                record,
                ..
            } => Some((definition, record)),
            _ => None,
        }
    }

    /// Whether the program implements `ILookup` on the program type `global`,
    /// so that keyword lookup may not read its fields.
    pub fn implements_lookup(&self, global: GlobalId) -> bool {
        self.lookups.get(global.0).copied().unwrap_or(true)
    }

    /// The position of `id` among the flattened top-level forms, when it is
    /// one.
    pub fn top_position(&self, id: ExprId) -> Option<usize> {
        self.top.get(id.0).copied().flatten()
    }

    /// The classes satisfying the program protocol with marker key `key`
    /// (`ns/Name`); `None` for core protocols and unknown keys.
    pub fn implementors(&self, key: &str) -> Option<Implementors> {
        self.protocols.get(key).copied()
    }
}

/// The result of [`Inference::new`]; see the module documentation.
pub struct Inference {
    values: Vec<Inferred>,
    locals: Vec<Inferred>,
    uses: Vec<u32>,
    facts: ProgramFacts,
    rounds: usize,
}

impl Inference {
    /// Infer every expression reachable from `program.root`, under `facts`
    /// (scanned from the same program).
    pub fn new(program: &Program, facts: ProgramFacts) -> Self {
        let mut walker = Walker::new(program, &facts);
        let mut rounds = 0;
        loop {
            walker.changed = false;
            walker.position = 0;
            walker.top(program.root);
            walker.first_round = false;
            rounds += 1;
            if !walker.changed {
                break;
            }
            if rounds == ROUND_LIMIT {
                walker.values.fill(Inferred::ANY);
                walker.locals.fill(Inferred::ANY);
                break;
            }
        }
        // Expressions the walk never reached are not emitted; claim nothing.
        for (value, visited) in walker.values.iter_mut().zip(&walker.visited) {
            if !visited {
                *value = Inferred::ANY;
            }
        }
        let Walker {
            values,
            mut locals,
            pinned,
            uses,
            ..
        } = walker;
        for (local, pinned) in locals.iter_mut().zip(pinned) {
            if pinned {
                *local = Inferred::ANY;
            }
        }
        Self {
            values,
            locals,
            uses,
            facts,
            rounds,
        }
    }

    /// The inferred value of `id`, in the context it is evaluated in
    /// (narrowed by the tests of enclosing `if`s).
    pub fn value(&self, id: ExprId) -> Inferred {
        self.values.get(id.0).copied().unwrap_or(Inferred::ANY)
    }

    /// The inferred type of `id`.
    pub fn ty(&self, id: ExprId) -> Type {
        self.value(id).ty
    }

    /// Every value `local` is bound to, before any narrowing.
    pub fn local(&self, local: LocalId) -> Inferred {
        self.locals.get(local.0).copied().unwrap_or(Inferred::ANY)
    }

    /// How many `Local` expressions reachable from the root read `local`.
    pub fn uses(&self, local: LocalId) -> usize {
        self.uses.get(local.0).map_or(0, |uses| *uses as usize)
    }

    /// What a call of the `Fn` expression `function` returns: the union of its
    /// method bodies.
    pub fn returns(&self, program: &Program, function: ExprId) -> Inferred {
        match &program.exprs[function.0].kind {
            ExprKind::Fn { methods, .. } => {
                methods.iter().fold(Inferred::NEVER, |value, method| {
                    value.union(self.value(method.body))
                })
            }
            _ => Inferred::ANY,
        }
    }

    /// The `Fn` expression that the callee of a call evaluates to, when known.
    pub fn callee(&self, program: &Program, call: ExprId) -> Option<ExprId> {
        let ExprKind::Call { callee, .. } = program.exprs[call.0].kind else {
            return None;
        };
        let value = self.value(callee);
        match value.known {
            Known::Function(function) if Type::FN.union(Type::NIL).contains(value.ty) => {
                Some(function)
            }
            _ => None,
        }
    }

    /// The program-wide facts inference relied on.
    pub fn facts(&self) -> &ProgramFacts {
        &self.facts
    }

    /// How many rounds the fixed point took.
    pub fn rounds(&self) -> usize {
        self.rounds
    }
}

/// A narrowing of one local in one branch.
#[derive(Clone, Copy)]
enum Filter {
    Keep(Type),
    Remove(Type),
    Instance(GlobalId, Type),
}

impl Filter {
    fn apply(self, value: Inferred) -> Inferred {
        match self {
            Self::Keep(classes) => value.keep(classes),
            Self::Remove(classes) => value.remove(classes),
            Self::Instance(global, class) => {
                Inferred::known(value.ty.intersection(class), Known::Instance(global))
            }
        }
    }
}

/// What a test tells about locals when it is truthy and when it is not.
#[derive(Default)]
struct Narrowing {
    yes: Vec<(LocalId, Filter)>,
    no: Vec<(LocalId, Filter)>,
}

impl Narrowing {
    fn swap(self) -> Self {
        Self {
            yes: self.no,
            no: self.yes,
        }
    }
}

struct Walker<'p> {
    program: &'p Program,
    facts: &'p ProgramFacts,
    values: Vec<Inferred>,
    visited: Vec<bool>,
    locals: Vec<Inferred>,
    /// Locals that are `any` and never narrowed.
    pinned: Vec<bool>,
    /// The `let` initializer of each local, to narrow through a local bound to
    /// a test.
    initializers: Vec<Option<ExprId>>,
    uses: Vec<u32>,
    narrowed: Vec<(LocalId, Inferred)>,
    /// Position of the top-level form being walked.
    position: usize,
    changed: bool,
    /// The first round also counts local reads.
    first_round: bool,
}

impl<'p> Walker<'p> {
    fn new(program: &'p Program, facts: &'p ProgramFacts) -> Self {
        let mut bound = vec![false; program.locals.len()];
        let mut pinned = vec![false; program.locals.len()];
        let mut initializers = vec![None; program.locals.len()];
        let assigned = |target: ExprId, pinned: &mut Vec<bool>| {
            if let ExprKind::Local(local) = program.exprs[target.0].kind {
                pinned[local.0] = true;
            }
        };
        for expr in &program.exprs {
            match &expr.kind {
                ExprKind::Let { bindings, .. } => {
                    for &(local, init) in bindings {
                        bound[local.0] = true;
                        initializers[local.0] = Some(init);
                    }
                }
                ExprKind::Loop { bindings, .. } | ExprKind::LetFn { bindings, .. } => {
                    for &(local, _) in bindings {
                        bound[local.0] = true;
                    }
                }
                ExprKind::Fn {
                    name: Some(name), ..
                } => bound[name.0] = true,
                ExprKind::SetGlobal { target, .. } => assigned(*target, &mut pinned),
                ExprKind::Binding { bindings, .. } => {
                    for (target, _) in bindings {
                        assigned(*target, &mut pinned);
                    }
                }
                // Raw text may assign its holes, and parameters are assignable.
                ExprKind::JsTemplate { args, .. } => {
                    for arg in args {
                        assigned(*arg, &mut pinned);
                    }
                }
                _ => (),
            }
        }
        // `(js-arguments)` escapes the arguments object, which aliases the
        // parameters of the enclosing (non-strict) functions; any expression
        // that reads it may change them.
        if program
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::JsArguments))
        {
            for expr in &program.exprs {
                let methods: Vec<&FnMethod> = match &expr.kind {
                    ExprKind::Fn { methods, .. } => methods.iter().collect(),
                    ExprKind::DefType { methods, .. }
                    | ExprKind::Reify { methods, .. }
                    | ExprKind::ExtendType { methods, .. } => {
                        methods.iter().map(|(_, method)| method).collect()
                    }
                    _ => Vec::new(),
                };
                for method in methods {
                    if reads_arguments(program, method.body) {
                        for param in method.params.iter().chain(&method.rest) {
                            pinned[param.0] = true;
                        }
                    }
                }
            }
        }
        // Type fields may be mutable.
        for local in program.field_bindings.keys() {
            pinned[local.0] = true;
        }
        // Parameters and `catch` and `this-as` bindings are `any`, but their
        // tests narrow them; locals bound by `let`, `loop` and `letfn` grow
        // from `never`.
        let locals = bound
            .iter()
            .zip(&pinned)
            .map(|(bound, pinned)| {
                if *bound && !*pinned {
                    Inferred::NEVER
                } else {
                    Inferred::ANY
                }
            })
            .collect();
        Self {
            program,
            facts,
            values: vec![Inferred::NEVER; program.exprs.len()],
            visited: vec![false; program.exprs.len()],
            locals,
            pinned,
            initializers,
            uses: vec![0; program.locals.len()],
            narrowed: Vec::new(),
            position: 0,
            changed: false,
            first_round: true,
        }
    }

    /// Walk the top level, numbering its forms as `crate::application`'s
    /// flattening does.
    fn top(&mut self, id: ExprId) -> Inferred {
        let value = if let ExprKind::Do(items) = &self.program.exprs[id.0].kind {
            let mut last = Inferred::of(Type::NIL);
            for &item in items {
                last = self.top(item);
            }
            self.visited[id.0] = true;
            last
        } else {
            let value = self.walk(id);
            self.position += 1;
            value
        };
        self.record(id, value)
    }

    fn record(&mut self, id: ExprId, value: Inferred) -> Inferred {
        self.visited[id.0] = true;
        let joined = self.values[id.0].union(value);
        if joined != self.values[id.0] {
            self.values[id.0] = joined;
            self.changed = true;
        }
        joined
    }

    fn bind(&mut self, local: LocalId, value: Inferred) {
        if self.pinned[local.0] {
            return;
        }
        let joined = self.locals[local.0].union(value);
        if joined != self.locals[local.0] {
            self.locals[local.0] = joined;
            self.changed = true;
        }
    }

    fn local(&self, local: LocalId) -> Inferred {
        if self.pinned[local.0] {
            return Inferred::ANY;
        }
        self.narrowed
            .iter()
            .rev()
            .find(|(narrowed, _)| *narrowed == local)
            .map_or(self.locals[local.0], |(_, value)| *value)
    }

    /// Walk `id` with `filters` applied to their locals.
    fn walk_narrowed(&mut self, id: ExprId, filters: &[(LocalId, Filter)]) -> Inferred {
        let depth = self.narrowed.len();
        for &(local, filter) in filters {
            if !self.pinned[local.0] {
                let value = filter.apply(self.local(local));
                self.narrowed.push((local, value));
            }
        }
        let value = self.walk(id);
        self.narrowed.truncate(depth);
        value
    }

    fn walk_all(&mut self, ids: &[ExprId]) -> Vec<Inferred> {
        ids.iter().map(|id| self.walk(*id)).collect()
    }

    fn methods(&mut self, methods: &[FnMethod]) -> Inferred {
        let mut value = Inferred::NEVER;
        for method in methods {
            value = value.union(self.walk(method.body));
        }
        value
    }

    fn walk(&mut self, id: ExprId) -> Inferred {
        use ExprKind as K;
        let program = self.program;
        let value = match &program.exprs[id.0].kind {
            K::Nil => Inferred::of(Type::NIL),
            K::Bool(_) => Inferred::of(Type::BOOLEAN),
            K::Number(_) => Inferred::of(Type::NUMBER),
            K::String(_) => Inferred::of(Type::STRING),
            K::Keyword(_) => Inferred::of(Type::KEYWORD),
            K::Symbol(_) => Inferred::of(Type::SYMBOL),
            K::ReaderLiteral {
                kind,
                items,
                quoted,
            } => {
                self.walk_all(items);
                match (kind, quoted) {
                    (CompilerLiteralKind::JsArray, false) => {
                        Inferred::known(Type::HOST, Known::Array)
                    }
                    (
                        CompilerLiteralKind::JsObject
                        | CompilerLiteralKind::Regex
                        | CompilerLiteralKind::Instant,
                        false,
                    ) => Inferred::of(Type::HOST),
                    (CompilerLiteralKind::Uuid | CompilerLiteralKind::Queue, false) => {
                        Inferred::of(Type::OBJECT)
                    }
                    _ => Inferred::ANY,
                }
            }
            K::DefinitionTest { value, .. } => self.walk(*value),
            K::WithMeta { value, metadata } => {
                let arguments = [self.walk(*value).ty, self.walk(*metadata).ty];
                self.core_call(CoreFnId::WithMeta, &arguments)
            }
            K::Var {
                value, metadata, ..
            } => {
                self.walk(*value);
                if let Some(metadata) = metadata {
                    self.walk(*metadata);
                }
                Inferred::of(Type::OBJECT)
            }
            K::List(items) => {
                self.walk_all(items);
                Inferred::of(Type::LIST)
            }
            K::Vector(items) => {
                self.walk_all(items);
                Inferred::known(Type::VECTOR, Known::Vector)
            }
            K::Map(items) | K::LiteralMap(items) => {
                self.walk_all(items);
                Inferred::of(Type::MAP)
            }
            K::Set(items) | K::LiteralSet(items) => {
                self.walk_all(items);
                Inferred::of(Type::SET)
            }
            K::CoreFn(core) => {
                if self.facts.replaced(*core) {
                    Inferred::ANY
                } else {
                    Inferred::of(core.operation().value)
                }
            }
            K::Local(local) => {
                if self.first_round {
                    self.uses[local.0] += 1;
                }
                self.local(*local)
            }
            K::Global(global) => self.global(*global),
            K::JsGlobal(_) => Inferred::ANY,
            K::JsTemplate { args, .. } => {
                self.walk_all(args);
                Inferred::ANY
            }
            K::JsIndex { receiver, indices } => {
                self.walk(*receiver);
                self.walk_all(indices);
                Inferred::ANY
            }
            K::ThisAs { body, .. } => self.walk(*body),
            K::JsArguments => Inferred::of(Type::HOST),
            K::JsDelete { receiver, key } => {
                self.walk(*receiver);
                self.walk(*key);
                Inferred::of(Type::BOOLEAN)
            }
            K::QualifiedMethod { class, .. } => {
                self.walk(*class);
                Inferred::of(Type::FN)
            }
            K::CopyArguments(dest) => {
                self.walk(*dest);
                Inferred::of(Type::NIL)
            }
            K::DefProtocol { .. } => Inferred::of(Type::NIL),
            K::ProtocolPredicate {
                protocol, value, ..
            } => {
                self.walk(*protocol);
                self.walk(*value);
                Inferred::of(Type::BOOLEAN)
            }
            K::ExtendType {
                target, methods, ..
            } => {
                self.walk(*target);
                for (_, method) in methods {
                    self.walk(method.body);
                }
                Inferred::ANY
            }
            K::Reify {
                metadata, methods, ..
            } => {
                if let Some(metadata) = metadata {
                    self.walk(*metadata);
                }
                for (_, method) in methods {
                    self.walk(method.body);
                }
                Inferred::of(Type::OBJECT)
            }
            K::DefType {
                body,
                masks,
                methods,
                ..
            } => {
                if let Some(masks) = masks {
                    self.walk(*masks);
                }
                for (_, method) in methods {
                    self.walk(method.body);
                }
                match body {
                    Some(body) => self.walk(*body),
                    None => Inferred::of(Type::FN),
                }
            }
            K::Construct { callee, args } => {
                self.walk(*callee);
                self.walk_all(args);
                match program.exprs[callee.0].kind {
                    K::Global(global) => self.instance(global, true).unwrap_or(Inferred::ANY),
                    _ => Inferred::ANY,
                }
            }
            K::Assert { test, message, .. } => {
                self.walk(*test);
                if let Some(message) = message {
                    self.walk(*message);
                }
                Inferred::of(Type::NIL)
            }
            K::Def {
                init, test, once, ..
            } => {
                let value = match init {
                    Some(init) => self.walk(*init),
                    None => Inferred::of(Type::NIL),
                };
                if let Some(test) = test {
                    self.walk(*test);
                }
                if *once {
                    value.union(Inferred::of(Type::NIL))
                } else {
                    value
                }
            }
            K::SetGlobal { target, value } => {
                self.walk(*target);
                self.walk(*value)
            }
            K::Binding { bindings, body } => {
                for &(target, init) in bindings {
                    self.walk(target);
                    self.walk(init);
                }
                self.walk(*body)
            }
            K::WithOutStr(body) => {
                self.walk(*body);
                Inferred::of(Type::STRING)
            }
            K::Exists(value) | K::BindingExists(value) => {
                self.walk(*value);
                Inferred::of(Type::BOOLEAN)
            }
            K::BooleanHint(value) => {
                self.walk(*value);
                Inferred::of(Type::BOOLEAN)
            }
            K::Property { receiver, .. } => {
                self.walk(*receiver);
                Inferred::ANY
            }
            K::MethodCall { receiver, args, .. } => {
                self.walk(*receiver);
                self.walk_all(args);
                Inferred::ANY
            }
            K::If { test, yes, no } => {
                self.walk(*test);
                let narrowing = self.narrowing(*test, 0);
                let yes = self.walk_narrowed(*yes, &narrowing.yes);
                let no = self.walk_narrowed(*no, &narrowing.no);
                if constant_truthy(&program.exprs[test.0].kind) {
                    yes
                } else {
                    yes.union(no)
                }
            }
            K::Do(items) => self
                .walk_all(items)
                .last()
                .copied()
                .unwrap_or(Inferred::of(Type::NIL)),
            K::Let { bindings, body } | K::Loop { bindings, body, .. } => {
                for &(local, init) in bindings {
                    let value = self.walk(init);
                    self.bind(local, value);
                }
                self.walk(*body)
            }
            K::LetFn { bindings, body } => {
                for &(local, init) in bindings {
                    let value = self.walk(init);
                    self.bind(local, value);
                }
                self.walk(*body)
            }
            K::Recur { target, args } => {
                let params = &program.recur_targets[target.0].params;
                for (index, &arg) in args.iter().enumerate() {
                    let value = self.walk(arg);
                    if let Some(&param) = params.get(index) {
                        self.bind(param, value);
                    }
                }
                Inferred::NEVER
            }
            K::Throw(value) => {
                self.walk(*value);
                Inferred::NEVER
            }
            K::Try {
                body,
                catches,
                finally,
            } => {
                let mut value = self.walk(*body);
                for catch in catches {
                    if let Some(class) = catch.class {
                        self.walk(class);
                    }
                    value = value.union(self.walk(catch.body));
                }
                if let Some(finally) = finally {
                    self.walk(*finally);
                }
                value
            }
            K::Case {
                test,
                branches,
                default,
                ..
            } => {
                self.walk(*test);
                let mut value = Inferred::NEVER;
                for &(constant, branch) in branches {
                    self.walk(constant);
                    value = value.union(self.walk(branch));
                }
                if let Some(default) = default {
                    value = value.union(self.walk(*default));
                }
                value
            }
            K::Fn { name, methods } => {
                let function = Inferred::known(Type::FN, Known::Function(id));
                if let Some(name) = name {
                    self.bind(*name, function);
                }
                self.methods(methods);
                if program.macro_functions.contains(&id) {
                    Inferred::ANY
                } else {
                    function
                }
            }
            K::Call { callee, args } => {
                let callee_value = self.walk(*callee);
                let arguments = self.walk_all(args);
                self.call(*callee, callee_value, &arguments)
            }
            K::InstanceOf { class, value } => {
                self.walk(*class);
                self.walk(*value);
                Inferred::of(Type::BOOLEAN)
            }
        };
        self.record(id, value)
    }

    /// A read of `global`.
    fn global(&self, global: GlobalId) -> Inferred {
        let program = self.program;
        if program.globals[global.0].name.contains('.') {
            return Inferred::ANY;
        }
        let (definition, value) = match self.facts.definition(global) {
            Definition::Unknown => return Inferred::ANY,
            Definition::Def(definition) => {
                let ExprKind::Def { init, once, .. } = program.exprs[definition.0].kind else {
                    return Inferred::ANY;
                };
                let value = match init {
                    Some(init) => self.values[init.0],
                    None => Inferred::of(Type::NIL),
                };
                if once {
                    return value.union(Inferred::of(Type::NIL));
                }
                (definition, value)
            }
            Definition::Type { definition, .. } => (definition, Inferred::of(Type::FN)),
            Definition::Protocol(definition) => {
                let ExprKind::DefProtocol {
                    global: protocol, ..
                } = program.exprs[definition.0].kind
                else {
                    return Inferred::ANY;
                };
                if protocol == global {
                    return Inferred::ANY;
                }
                (definition, Inferred::of(Type::FN))
            }
        };
        // Before its definition runs, a global is undefined. A read in a later
        // top-level form than a top-level definition runs after it: an earlier
        // form that throws ends the program.
        let defined_before = self
            .facts
            .top_position(definition)
            .is_some_and(|position| position < self.position);
        if defined_before {
            value
        } else {
            value.union(Inferred::of(Type::NIL))
        }
    }

    /// What `new global` (`construct`) or a call of `global` returns when
    /// `global` is a program type's constructor, or its positional or map
    /// factory: an instance of that type.
    fn instance(&self, global: GlobalId, construct: bool) -> Option<Inferred> {
        let Definition::Type {
            type_global,
            role,
            record,
            ..
        } = self.facts.definition(global)
        else {
            return None;
        };
        if construct != (role == TypeRole::Constructor) {
            return None;
        }
        let class = if record { Type::RECORD } else { Type::OBJECT };
        Some(Inferred::known(class, Known::Instance(type_global)))
    }

    fn core_call(&self, core: CoreFnId, arguments: &[Type]) -> Inferred {
        let operation = core.operation();
        let call = operation.call(arguments);
        if !self.facts.trusted(core, call.calls) {
            return Inferred::ANY;
        }
        // Joining `returns` keeps the result monotone in the arguments: a call
        // outside the domain may also return what one inside does.
        let returns = call.returns.union(operation.returns);
        let known = match core {
            CoreFnId::Vector => Known::Vector,
            CoreFnId::Array => Known::Array,
            _ => Known::Unknown,
        };
        Inferred::known(returns, known)
    }

    fn call(&self, callee: ExprId, value: Inferred, arguments: &[Inferred]) -> Inferred {
        let program = self.program;
        match &program.exprs[callee.0].kind {
            ExprKind::CoreFn(core) => {
                let types: Vec<Type> = arguments.iter().map(|value| value.ty).collect();
                return self.core_call(*core, &types);
            }
            ExprKind::Global(global) => {
                if let Some(instance) = self.instance(*global, false) {
                    return instance;
                }
            }
            ExprKind::BooleanHint(inner) => {
                if let ExprKind::Global(global) = program.exprs[inner.0].kind
                    && program.globals[global.0].boolean_tag
                {
                    return Inferred::of(Type::BOOLEAN);
                }
            }
            _ => (),
        }
        // Calling nil throws. Staying `never` until the callee is known keeps
        // the fixed point from starting at `any` for recursive functions.
        let callable = value.ty.difference(Type::NIL);
        if callable.is_never() {
            return Inferred::NEVER;
        }
        match value.known {
            Known::Function(function) if callable == Type::FN => {
                match &program.exprs[function.0].kind {
                    ExprKind::Fn { methods, .. } => {
                        methods.iter().fold(Inferred::NEVER, |result, method| {
                            result.union(self.values[method.body.0])
                        })
                    }
                    _ => Inferred::ANY,
                }
            }
            _ => Inferred::ANY,
        }
    }

    /// What `test` tells about locals in the branches of an `if`.
    fn narrowing(&self, test: ExprId, depth: usize) -> Narrowing {
        use ExprKind as K;
        let program = self.program;
        if depth > 8 {
            return Narrowing::default();
        }
        match &program.exprs[test.0].kind {
            K::Local(local) if !self.pinned[local.0] => {
                let mut narrowing = match self.initializers[local.0] {
                    Some(init) => self.narrowing(init, depth + 1),
                    None => Narrowing::default(),
                };
                narrowing.yes.push((*local, Filter::Remove(Type::NIL)));
                narrowing
                    .no
                    .push((*local, Filter::Keep(Type::NIL.union(Type::BOOLEAN))));
                narrowing
            }
            K::BooleanHint(inner) | K::DefinitionTest { value: inner, .. } => {
                self.narrowing(*inner, depth + 1)
            }
            K::Do(items) if items.len() == 1 => self.narrowing(items[0], depth + 1),
            K::Call { callee, args } => {
                let K::CoreFn(core) = program.exprs[callee.0].kind else {
                    return Narrowing::default();
                };
                if self.facts.replaced(core) {
                    return Narrowing::default();
                }
                if core == CoreFnId::Not && args.len() == 1 {
                    return self.narrowing(args[0], depth + 1).swap();
                }
                let [arg] = args.as_slice() else {
                    return Narrowing::default();
                };
                let K::Local(local) = program.exprs[arg.0].kind else {
                    return Narrowing::default();
                };
                let user = Type::RECORD.union(Type::OBJECT);
                let intact = self.facts.core_protocols_intact();
                let (yes, no) = match core {
                    CoreFnId::IsNil => (Filter::Keep(Type::NIL), Some(Filter::Remove(Type::NIL))),
                    CoreFnId::IsSome => (Filter::Remove(Type::NIL), Some(Filter::Keep(Type::NIL))),
                    CoreFnId::IsString => (
                        Filter::Keep(Type::STRING),
                        Some(Filter::Remove(Type::STRING)),
                    ),
                    CoreFnId::IsNumber => (
                        Filter::Keep(Type::NUMBER),
                        Some(Filter::Remove(Type::NUMBER)),
                    ),
                    CoreFnId::IsKeyword => (
                        Filter::Keep(Type::KEYWORD),
                        Some(Filter::Remove(Type::KEYWORD)),
                    ),
                    CoreFnId::IsSymbol => (
                        Filter::Keep(Type::SYMBOL),
                        Some(Filter::Remove(Type::SYMBOL)),
                    ),
                    CoreFnId::IsBoolean => (
                        Filter::Keep(Type::BOOLEAN),
                        Some(Filter::Remove(Type::BOOLEAN)),
                    ),
                    CoreFnId::IsTrue | CoreFnId::IsFalse => (Filter::Keep(Type::BOOLEAN), None),
                    CoreFnId::IsArray => (Filter::Keep(Type::HOST), None),
                    CoreFnId::IsVector if intact => (Filter::Keep(Type::VECTOR.union(user)), None),
                    CoreFnId::IsMap if intact => (Filter::Keep(Type::MAP.union(user)), None),
                    CoreFnId::IsSet if intact => (Filter::Keep(Type::SET.union(user)), None),
                    CoreFnId::IsSeq if intact => {
                        (Filter::Keep(Type::SEQ.union(Type::LIST).union(user)), None)
                    }
                    CoreFnId::IsRecord if intact => (Filter::Keep(user), None),
                    _ => return Narrowing::default(),
                };
                Narrowing {
                    yes: vec![(local, yes)],
                    no: no.map(|no| vec![(local, no)]).unwrap_or_default(),
                }
            }
            K::InstanceOf { class, value } => {
                let (K::Global(global), K::Local(local)) =
                    (&program.exprs[class.0].kind, &program.exprs[value.0].kind)
                else {
                    return Narrowing::default();
                };
                match self.facts.program_type(*global) {
                    Some((_, record)) => {
                        let class = if record { Type::RECORD } else { Type::OBJECT };
                        Narrowing {
                            yes: vec![(*local, Filter::Instance(*global, class))],
                            no: Vec::new(),
                        }
                    }
                    None => Narrowing::default(),
                }
            }
            K::ProtocolPredicate { value, key, .. } => {
                let K::Local(local) = program.exprs[value.0].kind else {
                    return Narrowing::default();
                };
                match self.facts.implementors(key) {
                    Some(Implementors {
                        sole: Some(global),
                        classes,
                    }) => Narrowing {
                        yes: vec![(local, Filter::Instance(global, classes))],
                        no: Vec::new(),
                    },
                    Some(Implementors { classes, .. }) => Narrowing {
                        yes: vec![(local, Filter::Keep(classes))],
                        no: Vec::new(),
                    },
                    None => Narrowing::default(),
                }
            }
            // `(and a b)` is `(let [t a] (if t b t))` and `(or a b)` is
            // `(let [t a] (if t t b))`, with `t` read nowhere else.
            K::Let { bindings, body } => {
                let Some((_, init, other, and)) = short_circuit(program, bindings, *body) else {
                    return Narrowing::default();
                };
                let (first, second) = (
                    self.narrowing(init, depth + 1),
                    self.narrowing(other, depth + 1),
                );
                if and {
                    Narrowing {
                        yes: first.yes.into_iter().chain(second.yes).collect(),
                        no: Vec::new(),
                    }
                } else {
                    Narrowing {
                        yes: Vec::new(),
                        no: first.no.into_iter().chain(second.no).collect(),
                    }
                }
            }
            _ => Narrowing::default(),
        }
    }
}

/// The parts of the expansion of `(and a b)`, `(let [t a] (if t b t))`, or of
/// `(or a b)`, `(let [t a] (if t t b))`: the temporary `t`, `a`, `b`, and
/// whether it is `and`. The caller checks that `t` is read only by the `if`.
pub fn short_circuit(
    program: &Program,
    bindings: &[(LocalId, ExprId)],
    body: ExprId,
) -> Option<(LocalId, ExprId, ExprId, bool)> {
    let [(temporary, init)] = bindings else {
        return None;
    };
    let mut body = body;
    while let ExprKind::Do(items) = &program.exprs[body.0].kind {
        let [item] = items.as_slice() else {
            return None;
        };
        body = *item;
    }
    let ExprKind::If { test, yes, no } = program.exprs[body.0].kind else {
        return None;
    };
    let reads =
        |id: ExprId| matches!(program.exprs[id.0].kind, ExprKind::Local(l) if l == *temporary);
    if !reads(test) {
        return None;
    }
    if reads(no) {
        Some((*temporary, *init, yes, true))
    } else if reads(yes) {
        Some((*temporary, *init, no, false))
    } else {
        None
    }
}

/// The classes an `extend-type` of a target other than a program type covers:
/// `nil`, `boolean`, `number` and `string` by name, `function` for functions
/// with and without metadata, and anything else (`object`, `default`, host
/// constructors, runtime types) for every class.
fn native_classes(program: &Program, target: ExprId) -> Type {
    let ExprKind::String(name) = &program.exprs[target.0].kind else {
        return Type::ANY;
    };
    match String::from_utf16_lossy(&program.strings[name.0]).as_str() {
        "nil" => Type::NIL,
        "boolean" => Type::BOOLEAN,
        "number" => Type::NUMBER,
        "string" => Type::STRING,
        "function" => Type::FN.union(Type::OBJECT),
        _ => Type::ANY,
    }
}

/// Whether a test always passes: a literal other than `nil` and `false`.
fn constant_truthy(kind: &ExprKind) -> bool {
    matches!(
        kind,
        ExprKind::Bool(true)
            | ExprKind::Number(_)
            | ExprKind::String(_)
            | ExprKind::Keyword(_)
            | ExprKind::Symbol(_)
    )
}

/// Whether `id` reads `(js-arguments)`, outside nested functions or not.
fn reads_arguments(program: &Program, id: ExprId) -> bool {
    let mut pending = vec![id];
    while let Some(id) = pending.pop() {
        if matches!(program.exprs[id.0].kind, ExprKind::JsArguments) {
            return true;
        }
        pending.extend(crate::program_fragment::children(program, id));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_representations_survive_joins_with_values_outside_their_class() {
        let function = Inferred::known(Type::FN, Known::Function(ExprId(7)));
        let nil = Inferred::of(Type::NIL);
        assert_eq!(Inferred::NEVER.union(function), function);
        assert_eq!(
            function.union(nil),
            Inferred::known(Type::FN.union(Type::NIL), Known::Function(ExprId(7)))
        );
        // Another function, or a value of the same class that is not known, is
        // no longer that function.
        assert_eq!(
            function.union(Inferred::known(Type::FN, Known::Function(ExprId(8)))),
            Inferred::of(Type::FN)
        );
        assert_eq!(function.union(Inferred::of(Type::FN)).known, Known::Unknown);
        // Narrowing away the class drops the representation.
        let vector = Inferred::known(Type::VECTOR.union(Type::NIL), Known::Vector);
        assert_eq!(vector.remove(Type::NIL).known, Known::Vector);
        assert_eq!(vector.keep(Type::NIL), nil);
        assert!(Inferred::of(Type::BOOLEAN.union(Type::NIL).union(Type::SEQ)).truth_is_js());
        for falsy_in_js in [Type::NUMBER, Type::STRING, Type::HOST, Type::ANY] {
            assert!(!Inferred::of(falsy_in_js).truth_is_js(), "{falsy_in_js}");
        }
    }
}
