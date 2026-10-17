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
//!   union of its method bodies' types, `any` for an `^:async` method, whose
//!   call is a Promise; `await` is `any`; a call of a global tagged `^boolean`
//!   and every `BooleanHint` are `boolean`, as upstream trusts these tags.
//! - A local bound by `let`, `loop` or `letfn` has the union of its
//!   initializer and, for a loop, of every `recur` argument for it.
//! - A parameter of a closed `fn` ([`crate::call_shapes::closed_functions`]:
//!   one whose every call the program makes itself) has the union of the
//!   arguments those calls pass it and of the method's `recur` arguments for
//!   it (light-cljs-229). A call with fewer arguments than parameters passes
//!   `nil` (`undefined`) for the others; the rest parameter is a `seq` when the
//!   call has arguments beyond the fixed parameters and `nil` otherwise, as
//!   `fnRest` builds it. A closed `fn` may also be the function argument of
//!   core operation calls with a callback shape (light-cljs-243): each such
//!   call binds its parameters as the calls the operation makes of it, from
//!   the call's arguments and their elements, when the table describes the
//!   call (within the domain, the operation not replaced, core protocols
//!   intact, and no program type that receives functions among the other
//!   arguments); otherwise it binds them to `any`.
//! - The state that `swap!` and `swap-vals!` pass their function (`sN` in
//!   their callback shapes) is what a closed atom holds
//!   ([`crate::call_shapes`], "Atoms", light-cljs-252): the join of the
//!   initial value of its `(atom x)`, the new states of the `reset!`,
//!   `reset-vals!` and `compare-and-set!` calls on it, and what the functions
//!   of the `swap!` and `swap-vals!` calls on it return, while the program
//!   replaces none of these operations and core protocols are intact. A
//!   closed atom is the runtime's, so it is no program type that receives
//!   functions. The state of any other atom is `any`.
//! - Under the same conditions, the calls on a closed atom return its states
//!   (light-cljs-256): `deref` what the atom holds, `reset!` its argument,
//!   `swap!` what its function returns, and `reset-vals!` and `swap-vals!`
//!   vectors of what it holds (`vector<s0>` in their rows). On any other atom
//!   they return what the table says.
//! - A parameter of an implementation of a closed protocol method
//!   ([`crate::call_shapes`], "Protocol methods") that the program defines
//!   once and never assigns has the union of what the method's calls pass it
//!   (light-cljs-253), as its dispatcher passes them: the receiver first, and
//!   `nil` for missing arguments. A call joins only into the implementations
//!   its receiver may run (`Receivers`): a `deftype`'s, `defrecord`'s or
//!   `extend-type`'s of a program type when the receiver may be an instance
//!   of that type (of class `object` or `record`, and not known to be an
//!   instance of another type), a `reify`'s when it may be an `object` that
//!   is not known to be a program type's instance, and a `specify!`'s always.
//!   A receiver that may be a host object, which may inherit any object's
//!   slots (`#js {"__proto__" t}`), reaches every implementation, and so does
//!   every receiver where the program names a prototype
//!   ([`ClosedFunctions::inherits`]).
//!   An `extend-type` of anything but a program type may be reached by the
//!   receiver's host type, through the dispatcher's own table, so its
//!   parameters stay `any`.
//! - The parameters of other functions, `catch` and `this-as` bindings, type
//!   fields and locals the program assigns are `any`.
//! - The elements of sequences, lists, vectors and sets
//!   ([`Inferred::elements`], light-cljs-243) come from collection literals,
//!   pass through bindings and joins as types do, and come from core calls
//!   whose row gives the elements of what they return (`map` returns what
//!   its function returns, `filter` the elements of its collection). Every
//!   other such collection holds `any`. These collections are immutable, so
//!   their elements do not change.
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
//! the least solution. Arguments may depend on other parameters, through
//! recursion too, and they join inside the same fixed point. Rounds walk the
//! top-level forms forward and backward in turn, since values flow forward
//! from definitions and parameter types backward from calls. Everything is
//! monotone over a finite lattice. After [`ROUND_LIMIT`] rounds inference
//! starts over without parameter types, and if that does not converge either,
//! the result degrades to `any` everywhere.
//!
//! # Soundness
//!
//! An expression's inferred value contains every value it can produce at run
//! time, under the program-wide conditions of `crate::core_operations` that
//! [`ProgramFacts`] checks, and with two trusted inputs: `^boolean` tags (as
//! upstream) and the table's assumption that protocol implementations return
//! what their protocol requires. Code that never runs may be typed `never`;
//! so is the body of a closed function that nothing calls.
//!
//! A parameter receives values only from calls, `recur` and assignments, and
//! assignable parameters (a `js*` hole, `set!`, a method that reads
//! `(js-arguments)`) stay `any`. A closed function's calls are all calls of it
//! (`crate::call_shapes`, "For parameter types"), every one of which the walk
//! visits, since it visits every expression reachable from the root. Calls
//! that a core operation makes of a closed function are those its callback
//! shape describes, which the `core_operations` suite and the
//! `type_inference` suite check against the runtime; a program type that
//! implements `IReduce`, `IKVReduce` or `ISwap` receives the function
//! itself, and the operations whose domain admits program types (`swap!`)
//! then bind `any`.
//!
//! A closed atom's state changes only through the calls on its reads that
//! `crate::call_shapes` ("Atoms") allows, since no other code gets the atom
//! and the program names no prototype it could patch: on the runtime's
//! atoms `reset!`, `reset-vals!` and `compare-and-set!` store their
//! argument, and `swap!` and `swap-vals!` what their function returns, with
//! no validator or watch (only options to `atom`, `set-validator!` and
//! `add-watch` add them). A replaced operation may store anything in the
//! atom it gets, and so may a program method that a core protocol extension
//! gives atoms. The runtime's `deref` returns the state, `reset!` and
//! `swap!` the new state, and the `-vals!` variants the old and the new
//! (light-cljs-256). A read of a global before its `def` runs is `nil`, on
//! which these operations throw.
//!
//! An implementation of a closed protocol method runs only from the calls of
//! the method (`crate::call_shapes`, "Protocol methods"), with the receiver
//! as `this` and first argument; a method's receiver parameter is bound to
//! `this`, which is that argument. An instance of a program type holds the
//! slots its `deftype` or `defrecord` and the `extend-type`s of that type
//! install on its prototype, and `specify!` may give any object more; a
//! `reify` object is no program type's instance. Other values hold those
//! slots only by inheriting them: host objects, whose prototype is any
//! object, and values whose prototype the program sets, which it can do only
//! by naming a prototype. A call whose receiver cannot hold an
//! implementation's slot therefore cannot run it. The body of an
//! implementation that no call reaches is typed `never`, as is the body of a
//! closed function nothing calls: only a slot read by name could run it.

use std::collections::HashMap;

use crate::analyzer::{
    CoreFnId, ExprId, ExprKind, FnMethod, GlobalId, LocalId, MethodDispatch, Program,
};
use crate::call_shapes::ClosedFunctions;
use crate::compiler_data::CompilerLiteralKind;
use crate::core_operations::{Calls, Descriptor, Term, Type};

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

/// The classes whose values hold the elements [`Inferred::elements`]
/// describes: the runtime's sequences, lists, vectors and sets, which are
/// immutable.
pub const SEQUENTIAL: Type = Type::SEQ
    .union(Type::LIST)
    .union(Type::VECTOR)
    .union(Type::SET);

/// The join of two values' types and representations: a [`Known`] survives
/// a join with a value that has none of its classes.
fn join(a: (Type, Known), b: (Type, Known)) -> (Type, Known) {
    let ((a_ty, a_known), (b_ty, b_known)) = (a, b);
    let known = if a_known == b_known {
        a_known
    } else if a_known == Known::Unknown && a_ty.intersection(b_known.classes()).is_never() {
        b_known
    } else if b_known == Known::Unknown && b_ty.intersection(a_known.classes()).is_never() {
        a_known
    } else {
        Known::Unknown
    };
    let ty = a_ty.union(b_ty);
    if ty.intersection(known.classes()).is_never() {
        (ty, Known::Unknown)
    } else {
        (ty, known)
    }
}

/// What the elements of a value's [`SEQUENTIAL`] values are: a type and a
/// representation, as for [`Inferred`], one level deep.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Element {
    pub ty: Type,
    pub known: Known,
}

impl Element {
    pub const NEVER: Self = Self {
        ty: Type::NEVER,
        known: Known::Unknown,
    };
    pub const ANY: Self = Self {
        ty: Type::ANY,
        known: Known::Unknown,
    };

    fn union(self, other: Self) -> Self {
        let (ty, known) = join((self.ty, self.known), (other.ty, other.known));
        Self { ty, known }
    }
}

/// An inferred value: its type, what else is known, and what the elements of
/// its sequential collections are. Values join (`union`) as types do; a
/// [`Known`] survives a join with a value that has none of its classes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inferred {
    pub ty: Type,
    pub known: Known,
    /// Every element of the values of `ty`'s [`SEQUENTIAL`] classes (since
    /// light-cljs-243): `never` when `ty` has none, `any` when nothing is
    /// known. Elements of other classes follow from the class
    /// ([`Inferred::element`]).
    pub elements: Element,
}

impl Inferred {
    pub const NEVER: Self = Self::of(Type::NEVER);
    pub const ANY: Self = Self::of(Type::ANY);

    /// A value of type `ty`, with nothing known about its elements.
    pub const fn of(ty: Type) -> Self {
        Self {
            ty,
            known: Known::Unknown,
            elements: if ty.intersection(SEQUENTIAL).is_never() {
                Element::NEVER
            } else {
                Element::ANY
            },
        }
    }

    /// A value of type `ty` refined by `known`.
    pub fn known(ty: Type, known: Known) -> Self {
        Self {
            known,
            ..Self::of(ty)
        }
        .normalized()
    }

    /// This value, whose sequential collections hold only `element`.
    pub fn with_elements(self, element: Self) -> Self {
        let (ty, known) = join((element.ty, element.known), (Type::NEVER, Known::Unknown));
        Self {
            elements: Element { ty, known },
            ..self
        }
        .normalized()
    }

    fn normalized(self) -> Self {
        let known = if self.ty.intersection(self.known.classes()).is_never() {
            Known::Unknown
        } else {
            self.known
        };
        let elements = if self.ty.intersection(SEQUENTIAL).is_never() {
            Element::NEVER
        } else {
            self.elements
        };
        Self {
            ty: self.ty,
            known,
            elements,
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
        let (ty, known) = join((self.ty, self.known), (other.ty, other.known));
        Self {
            ty,
            known,
            elements: self.elements.union(other.elements),
        }
    }

    /// The values of classes `classes`.
    pub fn keep(self, classes: Type) -> Self {
        Self {
            ty: self.ty.intersection(classes),
            ..self
        }
        .normalized()
    }

    /// The values of classes other than `classes`.
    pub fn remove(self, classes: Type) -> Self {
        Self {
            ty: self.ty.difference(classes),
            ..self
        }
        .normalized()
    }

    /// What a sequence over this value yields (`seq`, `first`, `map`, ...):
    /// the elements of its sequential collections, the entries (vectors) of
    /// its maps and the characters (strings) of its strings; `nil` yields
    /// nothing, and every other class anything.
    pub fn element(self) -> Self {
        let mut element = Self::NEVER;
        if !self.ty.intersection(SEQUENTIAL).is_never() {
            element = Self::known(self.elements.ty, self.elements.known);
        }
        if !self.ty.intersection(Type::MAP).is_never() {
            element = element.union(Self::of(Type::VECTOR));
        }
        if !self.ty.intersection(Type::STRING).is_never() {
            element = element.union(Self::of(Type::STRING));
        }
        let other = self.ty.difference(
            SEQUENTIAL
                .union(Type::MAP)
                .union(Type::STRING)
                .union(Type::NIL),
        );
        if !other.is_never() {
            element = Self::ANY;
        }
        element
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
/// every expression, including code that may never run. Definitions count
/// only where the program's root reaches them, since analysis leaves
/// scaffolding in the arena that no output contains (`defprotocol` makes a
/// `def` without a value for each of its Vars).
#[derive(Clone, Debug)]
pub struct ProgramFacts {
    /// Core operations the program may replace, by table row.
    replaced: Vec<bool>,
    /// Core protocols are extended only to the program's own types.
    core_protocols: bool,
    /// Some program type implements `IEquiv`.
    equiv: bool,
    /// Some program type implements a core protocol whose methods receive the
    /// function an operation is given: `IReduce`, `IKVReduce` or `ISwap`.
    receives_functions: bool,
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
        let receives = |method: &(MethodDispatch, FnMethod)| {
            ["IReduce", "IKVReduce", "ISwap"]
                .into_iter()
                .any(|protocol| implements(protocol)(method))
        };
        let mut equiv = false;
        let mut receives_functions = false;
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
        // Definitions count where emission can reach them: analysis leaves
        // scaffolding in the arena, such as the `def` without a value that
        // `defprotocol` makes for each Var, which no output contains.
        let mut emitted = vec![false; program.exprs.len()];
        let mut pending = vec![program.root];
        while let Some(id) = pending.pop() {
            if !std::mem::replace(&mut emitted[id.0], true) {
                pending.extend(crate::program_fragment::children(program, id));
            }
        }
        for (index, expr) in program.exprs.iter().enumerate() {
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
                    receives_functions |= methods.iter().any(receives);
                }
                ExprKind::Reify { methods, .. } => {
                    equiv |= methods.iter().any(implements("IEquiv"));
                    receives_functions |= methods.iter().any(receives);
                }
                ExprKind::Global(_) => (),
                kind => {
                    if let ExprKind::DefType { methods, .. } = kind {
                        equiv |= methods.iter().any(implements("IEquiv"));
                        receives_functions |= methods.iter().any(receives);
                    }
                    if emitted[index] {
                        for global in crate::application::globals(kind) {
                            definitions[global.0] += 1;
                        }
                    }
                }
            }
        }
        let sole = |global: GlobalId| definitions[global.0] == 1 && !assigned[global.0];
        let mut globals = vec![Definition::Unknown; program.globals.len()];
        let mut lookups = vec![false; program.globals.len()];
        for (index, expr) in program.exprs.iter().enumerate() {
            let id = ExprId(index);
            if !emitted[index] {
                continue;
            }
            match &expr.kind {
                ExprKind::Def { global, .. } if sole(*global) => {
                    globals[global.0] = Definition::Def(id);
                }
                ExprKind::DefType {
                    record,
                    global,
                    layout,
                    methods,
                } if sole(*global) => {
                    let role = |role| Definition::Type {
                        definition: id,
                        type_global: *global,
                        role,
                        record: *record,
                    };
                    globals[global.0] = role(TypeRole::Constructor);
                    if let Some(factory) = layout.constructor.filter(|factory| sole(*factory)) {
                        globals[factory.0] = role(TypeRole::Factory);
                    }
                    if let Some(factory) = layout.map_constructor.filter(|factory| sole(*factory)) {
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
            receives_functions,
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

    /// Whether a `deftype`, `defrecord`, `reify` or `extend-type` of the program
    /// implements `IReduce`, `IKVReduce` or `ISwap`, whose methods receive the
    /// function that `reduce`, `reduce-kv` or `swap!` is given and may call it
    /// with anything. Otherwise the runtime makes every call of an
    /// operation's function argument, as its callback shape says.
    pub fn receives_functions(&self) -> bool {
        self.receives_functions
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
    typed_parameters: usize,
}

impl Inference {
    /// Infer every expression reachable from `program.root`, under `facts`
    /// (scanned from the same program), joining the parameters of the `fn`s
    /// that `closed` finds closed over their calls.
    pub fn new(program: &Program, facts: ProgramFacts, closed: &ClosedFunctions) -> Self {
        let mut walker = Walker::new(program, &facts, closed);
        let mut solved = walker.solve();
        // Parameter types that do not converge (a call chain longer than the
        // round limit runs one round per link) must not cost the program the
        // types it has without them.
        let without = ClosedFunctions::default();
        if solved.is_none() && !closed.is_empty() {
            walker = Walker::new(program, &facts, &without);
            solved = walker.solve();
        }
        let rounds = solved.unwrap_or_else(|| {
            walker.values.fill(Inferred::ANY);
            walker.locals.fill(Inferred::ANY);
            ROUND_LIMIT
        });
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
            parameters,
            ..
        } = walker;
        for (local, pinned) in locals.iter_mut().zip(pinned) {
            if pinned {
                *local = Inferred::ANY;
            }
        }
        let typed_parameters = parameters
            .iter()
            .filter(|parameter| locals[parameter.0] != Inferred::ANY)
            .count();
        Self {
            values,
            locals,
            uses,
            facts,
            rounds,
            typed_parameters,
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
    /// method bodies, or `any` for an `^:async` method, whose call returns a
    /// Promise of its body's value.
    pub fn returns(&self, program: &Program, function: ExprId) -> Inferred {
        match &program.exprs[function.0].kind {
            ExprKind::Fn { methods, .. } => {
                methods.iter().fold(Inferred::NEVER, |value, method| {
                    value.union(if method.asynchronous {
                        Inferred::ANY
                    } else {
                        self.value(method.body)
                    })
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

    /// How many parameters of closed functions have a type other than `any`.
    pub fn typed_parameters(&self) -> usize {
        self.typed_parameters
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

/// The receivers whose calls of a protocol method may run one implementation
/// of it (light-cljs-253).
#[derive(Clone, Copy)]
enum Receivers {
    /// Instances of the program type of this global, of class `record` or
    /// `object`: its `deftype` or `defrecord`, or an `extend-type` of it.
    Type(GlobalId, Type),
    /// Objects of a `reify`, which are no program type's instances.
    Reify,
    /// Any receiver: the target of `specify!` may be any object.
    Any,
}

impl Receivers {
    /// Whether a call with receiver `receiver` may run the implementation.
    /// Each instance of one program type has only that type's slots, so a
    /// receiver known to be an instance of another type ([`Known::Instance`])
    /// does not reach a type's or a `reify`'s method. A host object may
    /// inherit any object's slots (`#js {"__proto__" t}`), and where the
    /// program names a prototype (`inherits`) any value may.
    fn reach(self, receiver: Inferred, inherits: bool) -> bool {
        match self {
            _ if inherits || !receiver.ty.intersection(Type::HOST).is_never() => {
                !receiver.ty.is_never()
            }
            Self::Type(global, class) => {
                !receiver.ty.intersection(class).is_never()
                    && !matches!(receiver.known, Known::Instance(other) if other != global)
            }
            Self::Reify => {
                !receiver.ty.intersection(Type::OBJECT).is_never()
                    && !matches!(receiver.known, Known::Instance(_))
            }
            Self::Any => true,
        }
    }
}

struct Walker<'p> {
    program: &'p Program,
    facts: &'p ProgramFacts,
    closed: &'p ClosedFunctions,
    /// The parameters of closed functions and protocol method
    /// implementations that are not pinned.
    parameters: Vec<LocalId>,
    /// By index in [`ClosedFunctions::implementations`]: who may run the
    /// implementation, when its parameters join the calls of its protocol
    /// method.
    receivers: Vec<Option<Receivers>>,
    /// By index in [`ClosedFunctions::atom`]: every value the closed atom
    /// holds (light-cljs-252).
    contents: Vec<Inferred>,
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
    /// The top-level forms, numbered as `crate::application`'s flattening
    /// numbers them, and the `do`s that hold them, inner ones first.
    forms: Vec<ExprId>,
    dos: Vec<ExprId>,
    /// Position of the top-level form being walked.
    position: usize,
    changed: bool,
    /// The first round also counts local reads.
    first_round: bool,
}

impl<'p> Walker<'p> {
    fn new(program: &'p Program, facts: &'p ProgramFacts, closed: &'p ClosedFunctions) -> Self {
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
        // The parameters of closed functions grow from their calls.
        let mut parameters = Vec::new();
        for (index, expr) in program.exprs.iter().enumerate() {
            if let ExprKind::Fn { methods, .. } = &expr.kind
                && closed.closed(ExprId(index))
            {
                for method in methods {
                    for &param in method.params.iter().chain(&method.rest) {
                        if !pinned[param.0] {
                            bound[param.0] = true;
                            parameters.push(param);
                        }
                    }
                }
            }
        }
        // So do the parameters of the implementations of closed protocol
        // methods (light-cljs-253) that the program defines once and never
        // assigns. An `extend-type` installs its methods on the prototype of
        // a program type; on anything else the dispatcher may also reach it
        // by the receiver's host type.
        let receivers: Vec<Option<Receivers>> = closed
            .implementations()
            .iter()
            .map(|implementation| {
                if !matches!(
                    facts.definition(implementation.global),
                    Definition::Protocol(_)
                ) {
                    return None;
                }
                let class = |record: bool| if record { Type::RECORD } else { Type::OBJECT };
                match &program.exprs[implementation.owner.0].kind {
                    ExprKind::DefType { global, record, .. } => {
                        Some(Receivers::Type(*global, class(*record)))
                    }
                    ExprKind::Reify { .. } => Some(Receivers::Reify),
                    ExprKind::ExtendType { instance: true, .. } => Some(Receivers::Any),
                    ExprKind::ExtendType { target, .. } => match program.exprs[target.0].kind {
                        ExprKind::Global(global) => facts
                            .program_type(global)
                            .map(|(_, record)| Receivers::Type(global, class(record))),
                        _ => None,
                    },
                    _ => None,
                }
            })
            .collect();
        for (implementation, receivers) in closed.implementations().iter().zip(&receivers) {
            let Some(method) = implementation
                .method(program)
                .filter(|_| receivers.is_some())
            else {
                continue;
            };
            for &param in &method.params {
                if !pinned[param.0] {
                    bound[param.0] = true;
                    parameters.push(param);
                }
            }
        }
        // Other parameters and `catch` and `this-as` bindings are `any`, but
        // their tests narrow them; locals bound by `let`, `loop` and `letfn`
        // grow from `never`.
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
        let (mut forms, mut dos) = (Vec::new(), Vec::new());
        let mut pending = vec![(program.root, false)];
        while let Some((id, expanded)) = pending.pop() {
            match &program.exprs[id.0].kind {
                ExprKind::Do(_) if expanded => dos.push(id),
                ExprKind::Do(items) => {
                    pending.push((id, true));
                    pending.extend(items.iter().rev().map(|item| (*item, false)));
                }
                _ => forms.push(id),
            }
        }
        Self {
            program,
            facts,
            closed,
            parameters,
            receivers,
            contents: vec![Inferred::NEVER; closed.atoms()],
            values: vec![Inferred::NEVER; program.exprs.len()],
            visited: vec![false; program.exprs.len()],
            locals,
            pinned,
            initializers,
            uses: vec![0; program.locals.len()],
            narrowed: Vec::new(),
            forms,
            dos,
            position: 0,
            changed: false,
            first_round: true,
        }
    }

    /// Run rounds until one changes nothing: how many it took, or `None` when
    /// [`ROUND_LIMIT`] rounds do not converge.
    fn solve(&mut self) -> Option<usize> {
        for round in 1..=ROUND_LIMIT {
            self.changed = false;
            self.round(round % 2 == 0);
            self.first_round = false;
            if !self.changed {
                return Some(round);
            }
        }
        None
    }

    /// Walk every top-level form once, the last first when `reverse`, then the
    /// `do`s that hold them. Values flow forward from definitions to later
    /// reads, and parameter types backward from calls to the earlier
    /// definitions they call, so rounds alternate: either order reaches the
    /// same least fixed point, since every value only grows.
    fn round(&mut self, reverse: bool) {
        let forms = std::mem::take(&mut self.forms);
        let mut walk = |position: usize| {
            self.position = position;
            self.walk(forms[position]);
        };
        if reverse {
            (0..forms.len()).rev().for_each(&mut walk);
        } else {
            (0..forms.len()).for_each(&mut walk);
        }
        self.forms = forms;
        // Each `do` evaluates to its last form; inner `do`s come first.
        let dos = std::mem::take(&mut self.dos);
        for &id in &dos {
            let ExprKind::Do(items) = &self.program.exprs[id.0].kind else {
                continue;
            };
            let value = items
                .last()
                .map_or(Inferred::of(Type::NIL), |last| self.values[last.0]);
            self.record(id, value);
        }
        self.dos = dos;
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

    /// Walk `ids` and join their values.
    fn join_all(&mut self, ids: &[ExprId]) -> Inferred {
        ids.iter()
            .fold(Inferred::NEVER, |value, id| value.union(self.walk(*id)))
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
            // The runtime's `Loadable` class, like a core type's (`AtomType`).
            K::LoadableType => Inferred::of(Type::FN),
            // `new Loadable(...)`, as `K::Construct` below.
            K::Loadable { class, .. } => {
                self.walk(*class);
                match program.exprs[class.0].kind {
                    K::Global(global) => self.instance(global, true).unwrap_or(Inferred::ANY),
                    _ => Inferred::ANY,
                }
            }
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
                let element = self.join_all(items);
                Inferred::of(Type::LIST).with_elements(element)
            }
            K::Vector(items) => {
                let element = self.join_all(items);
                Inferred::known(Type::VECTOR, Known::Vector).with_elements(element)
            }
            K::Map(items) | K::LiteralMap(items) => {
                self.walk_all(items);
                Inferred::of(Type::MAP)
            }
            K::Set(items) | K::LiteralSet(items) => {
                let element = self.join_all(items);
                Inferred::of(Type::SET).with_elements(element)
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
                layout, methods, ..
            } => {
                if let Some(masks) = layout.masks {
                    self.walk(masks);
                }
                for (_, method) in methods {
                    self.walk(method.body);
                }
                match layout.body {
                    Some(body) => self.walk(body),
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
            // What a promise settles to, or the value itself, is not known.
            K::Await(value) => {
                self.walk(*value);
                Inferred::ANY
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
                if let Some((function, method)) = self.closed.call(id) {
                    self.pass(function, method, &arguments);
                }
                self.dispatch(id, &arguments);
                if let K::CoreFn(core) = program.exprs[callee.0].kind {
                    for (index, function) in self.closed.callbacks(id) {
                        self.pass_callback(core, args, &arguments, index, function);
                    }
                    self.store(id, core, args, &arguments);
                }
                self.call(*callee, callee_value, args, &arguments)
            }
            K::InstanceOf { class, value } => {
                self.walk(*class);
                self.walk(*value);
                Inferred::of(Type::BOOLEAN)
            }
        };
        self.record(id, value)
    }

    /// Join what the call `call` of `core` with argument expressions `args`
    /// of values `arguments` stores in a closed atom (`crate::call_shapes`,
    /// "Atoms", light-cljs-252) into what the atom holds: the initial value
    /// of its `(atom x)`, the new state of `reset!`, `reset-vals!` and
    /// `compare-and-set!`, and what the function given to `swap!` or
    /// `swap-vals!` returns when called with the state and the call's other
    /// arguments, as the runtime calls it on its own atoms. A replaced
    /// operation, including `deref`, may store anything in the atom it gets,
    /// and so may a protocol method that a core protocol extension adds to
    /// atoms.
    fn store(&mut self, call: ExprId, core: CoreFnId, args: &[ExprId], arguments: &[Inferred]) {
        let target = if core == CoreFnId::Atom {
            call
        } else {
            match args.first() {
                Some(&target) => target,
                None => return,
            }
        };
        let Some(atom) = self.closed.atom(target) else {
            return;
        };
        let argument = |index: usize| arguments.get(index).copied().unwrap_or(Inferred::ANY);
        let stored = match core {
            _ if !self.facts.trusted(core, Calls::Invoke) => Inferred::ANY,
            CoreFnId::Atom => argument(0),
            CoreFnId::Reset | CoreFnId::ResetVals => argument(1),
            CoreFnId::CompareAndSet => argument(2),
            CoreFnId::Swap | CoreFnId::SwapVals => self.callback_returns(core, 1, args, arguments),
            _ => return,
        };
        let joined = self.contents[atom].union(stored);
        if joined != self.contents[atom] {
            self.contents[atom] = joined;
            self.changed = true;
        }
    }

    /// What the closed atom read by `arg` holds (`sN`), else `any`.
    fn state(&self, arg: Option<&ExprId>) -> Inferred {
        arg.and_then(|arg| self.closed.atom(*arg))
            .map_or(Inferred::ANY, |atom| self.contents[atom])
    }

    /// What a call of `core` with argument expressions `args` of values
    /// `arguments` returns when its first argument reads a closed atom
    /// (light-cljs-256), under the conditions of [`Walker::store`]: `deref`
    /// returns what the atom holds, `reset!` its new state and `swap!` what
    /// its function returns, as the runtime's atoms do. A replaced `atom`
    /// may make anything, such as a program type whose `-reset!` returns
    /// another value. `None` for other calls, whose row applies:
    /// `reset-vals!` and `swap-vals!` return vectors of states (`sN`).
    fn atom_result(
        &self,
        core: CoreFnId,
        args: &[ExprId],
        arguments: &[Inferred],
    ) -> Option<Inferred> {
        let atom = self.closed.atom(*args.first()?)?;
        if !self.facts.trusted(core, Calls::Invoke) || self.facts.replaced(CoreFnId::Atom) {
            return None;
        }
        match core {
            CoreFnId::Deref => Some(self.contents[atom]),
            CoreFnId::Reset => arguments.get(1).copied(),
            CoreFnId::Swap => Some(self.callback_returns(core, 1, args, arguments)),
            _ => None,
        }
    }

    /// Bind the parameters of method `method` of the closed `fn` `function`
    /// to the `arguments` of a call, as the dispatcher or a direct call passes
    /// them: `nil` (`undefined`) for missing arguments, and a rest sequence
    /// (`fnRest`) or `nil` for the rest parameter.
    fn pass(&mut self, function: ExprId, method: usize, arguments: &[Inferred]) {
        let ExprKind::Fn { methods, .. } = &self.program.exprs[function.0].kind else {
            return;
        };
        let Some(method) = methods.get(method) else {
            return;
        };
        for (index, &param) in method.params.iter().enumerate() {
            let argument = arguments
                .get(index)
                .copied()
                .unwrap_or(Inferred::of(Type::NIL));
            self.bind(param, argument);
        }
        if let Some(rest) = method.rest {
            let rest_type = if arguments.len() > method.params.len() {
                Type::SEQ
            } else {
                Type::NIL
            };
            self.bind(rest, Inferred::of(rest_type));
        }
    }

    /// Bind the parameters of each implementation that the protocol method
    /// call `call` with `arguments` may run, as its dispatcher passes them:
    /// the receiver and the arguments, `nil` (`undefined`) for missing ones
    /// (light-cljs-253).
    fn dispatch(&mut self, call: ExprId, arguments: &[Inferred]) {
        let receiver = arguments
            .first()
            .copied()
            .unwrap_or(Inferred::of(Type::NIL));
        for (index, implementation) in self.closed.dispatch(call) {
            let inherits = self.closed.inherits();
            if !self.receivers[index].is_some_and(|receivers| receivers.reach(receiver, inherits)) {
                continue;
            }
            let Some(method) = implementation.method(self.program) else {
                continue;
            };
            for (position, &param) in method.params.iter().enumerate() {
                let argument = arguments
                    .get(position)
                    .copied()
                    .unwrap_or(Inferred::of(Type::NIL));
                self.bind(param, argument);
            }
        }
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
        let returns = call.returns.union(operation.returns_for(arguments.len()));
        let known = match core {
            CoreFnId::Vector => Known::Vector,
            CoreFnId::Array => Known::Array,
            _ => Known::Unknown,
        };
        Inferred::known(returns, known)
    }

    /// Whether the table describes a call of `core` with arguments of these
    /// types: the call is within the operation's domain and the program
    /// keeps the table's conditions for it. Elements and callback arguments
    /// come from traversing the arguments, which runs core protocols even in
    /// a lazy operation that runs nothing itself (`Calls::None`), so core
    /// protocols must be intact.
    fn described(&self, core: CoreFnId, types: &[Type]) -> bool {
        let operation = core.operation();
        operation
            .arity(types.len())
            .is_some_and(|arity| arity.admits(types))
            && self.facts.trusted(core, operation.calls)
            && self.facts.core_protocols_intact()
    }

    /// A call of `core` with argument expressions `args` of values
    /// `arguments`: [`Walker::core_call`], whose sequential collections hold
    /// what the row's element type says when the table describes the call
    /// (light-cljs-243), or [`Walker::atom_result`] on a closed atom. `args`
    /// may be empty for a call the operation makes, whose `rN` terms are then
    /// `any`.
    fn core_result(&self, core: CoreFnId, args: &[ExprId], arguments: &[Inferred]) -> Inferred {
        if let Some(result) = self.atom_result(core, args, arguments) {
            return result;
        }
        let types: Vec<Type> = arguments.iter().map(|value| value.ty).collect();
        let result = self.core_call(core, &types);
        match &core.operation().elements {
            Some(elements) if self.described(core, &types) => {
                result.with_elements(self.describe(core, elements, args, arguments, true))
            }
            _ => result,
        }
    }

    /// The value `descriptor` names for a call of `core` (see
    /// [`crate::core_operations::Descriptor`]), joining `aN+` and `eN+`
    /// terms. `rN` is `any` unless `returns`.
    fn describe(
        &self,
        core: CoreFnId,
        descriptor: &Descriptor,
        args: &[ExprId],
        arguments: &[Inferred],
        returns: bool,
    ) -> Inferred {
        let argument = |index: usize| arguments.get(index).copied().unwrap_or(Inferred::ANY);
        let from = |index: usize| arguments.get(index..).unwrap_or(&[]).iter().copied();
        descriptor
            .terms
            .iter()
            .fold(Inferred::NEVER, |value, term| {
                value.union(match *term {
                    Term::Type(ty) => Inferred::of(ty),
                    Term::Argument(index) => argument(index),
                    Term::Element(index) => argument(index).element(),
                    Term::Arguments(index) => from(index).fold(Inferred::NEVER, Inferred::union),
                    Term::Elements(index) => from(index)
                        .fold(Inferred::NEVER, |value, argument| {
                            value.union(argument.element())
                        }),
                    Term::State(index) => self.state(args.get(index)),
                    Term::Returns(index) if returns => {
                        self.callback_returns(core, index, args, arguments)
                    }
                    Term::Returns(_) => Inferred::ANY,
                })
            })
    }

    /// The arguments of one call that `core` makes of a function argument,
    /// as its callback shape describes them (`call`), with `rN` as in
    /// [`Walker::describe`].
    fn callback_arguments(
        &self,
        core: CoreFnId,
        call: &[Descriptor],
        args: &[ExprId],
        arguments: &[Inferred],
        returns: bool,
    ) -> Vec<Inferred> {
        let mut values = Vec::new();
        for descriptor in call {
            match descriptor.spread() {
                Some(Term::Arguments(from)) => {
                    values.extend(arguments.iter().skip(from).copied());
                }
                Some(Term::Elements(from)) => {
                    values.extend(arguments.iter().skip(from).map(|value| value.element()));
                }
                _ => values.push(self.describe(core, descriptor, args, arguments, returns)),
            }
        }
        values
    }

    /// What the function argument at `index` of a call of `core` returns when
    /// `core` calls it as its callback shape says, with its `rN` arguments
    /// `any`.
    fn callback_returns(
        &self,
        core: CoreFnId,
        index: usize,
        args: &[ExprId],
        arguments: &[Inferred],
    ) -> Inferred {
        let callback = core
            .operation()
            .arity(arguments.len())
            .and_then(|arity| arity.callback(index));
        let (Some(callback), Some(&callee), Some(&value)) =
            (callback, args.get(index), arguments.get(index))
        else {
            return Inferred::ANY;
        };
        callback.calls.iter().fold(Inferred::NEVER, |result, call| {
            let call_arguments = self.callback_arguments(core, call, args, arguments, false);
            result.union(self.call(callee, value, &[], &call_arguments))
        })
    }

    /// Bind the parameters of the closed `fn` `function`, the argument at
    /// `index` of a call of `core` with argument expressions `args` of values
    /// `arguments`, to the arguments of every call `core` makes of it
    /// (light-cljs-243). Where the table does not describe the call, or a
    /// program type that receives functions (`IReduce`, `IKVReduce`, `ISwap`)
    /// may get the function, any call can reach it. A closed atom that
    /// `atom` made (light-cljs-252) is the runtime's, not a program type.
    fn pass_callback(
        &mut self,
        core: CoreFnId,
        args: &[ExprId],
        arguments: &[Inferred],
        index: usize,
        function: ExprId,
    ) {
        let types: Vec<Type> = arguments.iter().map(|value| value.ty).collect();
        let program_types = Type::RECORD.union(Type::OBJECT);
        let runtime_atom = |position: usize| {
            !self.facts.replaced(CoreFnId::Atom)
                && args
                    .get(position)
                    .is_some_and(|arg| self.closed.atom(*arg).is_some())
        };
        let reachable = self.facts.receives_functions()
            && types.iter().enumerate().any(|(position, ty)| {
                position != index
                    && !ty.intersection(program_types).is_never()
                    && !runtime_atom(position)
            });
        let callback = core
            .operation()
            .arity(arguments.len())
            .and_then(|arity| arity.callback(index))
            .filter(|_| self.described(core, &types) && !reachable);
        let ExprKind::Fn { methods, .. } = &self.program.exprs[function.0].kind else {
            return;
        };
        let Some(callback) = callback else {
            for method in methods {
                for &param in method.params.iter().chain(&method.rest) {
                    self.bind(param, Inferred::ANY);
                }
            }
            return;
        };
        for call in &callback.calls {
            let call_arguments = self.callback_arguments(core, call, args, arguments, true);
            if let Some(method) =
                crate::call_shapes::dispatched_method(methods, call_arguments.len())
            {
                self.pass(function, method, &call_arguments);
            }
        }
    }

    fn call(
        &self,
        callee: ExprId,
        value: Inferred,
        args: &[ExprId],
        arguments: &[Inferred],
    ) -> Inferred {
        let program = self.program;
        match &program.exprs[callee.0].kind {
            ExprKind::CoreFn(core) => return self.core_result(*core, args, arguments),
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
                    // An `^:async` method returns a Promise.
                    ExprKind::Fn { methods, .. } => {
                        methods.iter().fold(Inferred::NEVER, |result, method| {
                            result.union(if method.asynchronous {
                                Inferred::ANY
                            } else {
                                self.values[method.body.0]
                            })
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
