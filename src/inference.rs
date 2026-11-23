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

use std::collections::{HashMap, HashSet};

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

const KNOWN_UNKNOWN: u8 = 0;
const KNOWN_FUNCTION: u8 = 1;
const KNOWN_INSTANCE: u8 = 2;
const KNOWN_VECTOR: u8 = 3;
const KNOWN_ARRAY: u8 = 4;
const KNOWN_UNVISITED: u8 = u8::MAX;

#[inline]
const fn known_classes(tag: u8) -> Type {
    match tag {
        KNOWN_FUNCTION => Type::FN,
        KNOWN_INSTANCE => Type::RECORD.union(Type::OBJECT),
        KNOWN_VECTOR => Type::VECTOR,
        KNOWN_ARRAY => Type::HOST,
        _ => Type::NEVER,
    }
}

#[inline]
fn decode_known(tag: u8, id: u32) -> Known {
    match tag {
        KNOWN_FUNCTION => Known::Function(ExprId(id as usize)),
        KNOWN_INSTANCE => Known::Instance(GlobalId(id as usize)),
        KNOWN_VECTOR => Known::Vector,
        KNOWN_ARRAY => Known::Array,
        _ => Known::Unknown,
    }
}

#[inline]
fn join_pair(
    a_ty: Type,
    a_tag: u8,
    a_id: u32,
    b_ty: Type,
    b_tag: u8,
    b_id: u32,
) -> (Type, u8, u32) {
    if a_ty.is_never() {
        return (b_ty, b_tag, b_id);
    }
    if b_ty.is_never() {
        return (a_ty, a_tag, a_id);
    }
    let (tag, id) = if a_tag == b_tag && a_id == b_id {
        (a_tag, a_id)
    } else if a_tag == KNOWN_UNKNOWN && a_ty.intersection(known_classes(b_tag)).is_never() {
        (b_tag, b_id)
    } else if b_tag == KNOWN_UNKNOWN && b_ty.intersection(known_classes(a_tag)).is_never() {
        (a_tag, a_id)
    } else {
        (KNOWN_UNKNOWN, 0)
    };
    let ty = a_ty.union(b_ty);
    if tag != KNOWN_UNKNOWN && ty.intersection(known_classes(tag)).is_never() {
        (ty, KNOWN_UNKNOWN, 0)
    } else {
        (ty, tag, id)
    }
}

/// Compact 16-byte internal representation of [`Inferred`] used throughout
/// inference storage and fixed-point solving (light-cljs-1100).
///
/// Invariants for every normalized value:
/// - `known_id == 0` whenever `known_tag` is not `KNOWN_FUNCTION` or `KNOWN_INSTANCE`.
/// - `elem_id == 0` whenever `elem_tag` is not `KNOWN_FUNCTION` or `KNOWN_INSTANCE`.
/// - `known_tag == KNOWN_UNKNOWN` whenever `ty.intersection(known_classes(known_tag)).is_never()`.
/// - `elem_ty == Type::NEVER` and `elem_tag == KNOWN_UNKNOWN` whenever
///   `ty.intersection(SEQUENTIAL).is_never()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CompactInferred {
    ty: Type,
    elem_ty: Type,
    known_tag: u8,
    elem_tag: u8,
    known_id: u32,
    elem_id: u32,
}

const _: () = assert!(std::mem::size_of::<CompactInferred>() == 16);

impl CompactInferred {
    const NEVER: Self = Self::of(Type::NEVER);
    const ANY: Self = Self::of(Type::ANY);
    const UNVISITED: Self = Self {
        ty: Type::ANY,
        elem_ty: Type::ANY,
        known_tag: KNOWN_UNVISITED,
        elem_tag: KNOWN_UNKNOWN,
        known_id: 0,
        elem_id: 0,
    };

    #[inline]
    const fn of(ty: Type) -> Self {
        Self {
            ty,
            elem_ty: if ty.intersection(SEQUENTIAL).is_never() {
                Type::NEVER
            } else {
                Type::ANY
            },
            known_tag: KNOWN_UNKNOWN,
            elem_tag: KNOWN_UNKNOWN,
            known_id: 0,
            elem_id: 0,
        }
    }

    #[inline]
    fn union(self, other: Self) -> Self {
        if self == other || other.ty.is_never() {
            return self;
        }
        if self.ty.is_never() {
            return other;
        }
        let (ty, known_tag, known_id) = join_pair(
            self.ty,
            self.known_tag,
            self.known_id,
            other.ty,
            other.known_tag,
            other.known_id,
        );
        let (elem_ty, elem_tag, elem_id) = if self.elem_ty == other.elem_ty
            && self.elem_tag == other.elem_tag
            && self.elem_id == other.elem_id
        {
            (self.elem_ty, self.elem_tag, self.elem_id)
        } else {
            join_pair(
                self.elem_ty,
                self.elem_tag,
                self.elem_id,
                other.elem_ty,
                other.elem_tag,
                other.elem_id,
            )
        };
        Self {
            ty,
            elem_ty,
            known_tag,
            elem_tag,
            known_id,
            elem_id,
        }
    }

    #[inline]
    fn to_inferred(self) -> Inferred {
        Inferred {
            ty: self.ty,
            known: decode_known(self.known_tag, self.known_id),
            elements: Element {
                ty: self.elem_ty,
                known: decode_known(self.elem_tag, self.elem_id),
            },
        }
    }
}

const REF_VECTOR: u8 = 1 << 0;
const REF_ARRAY: u8 = 1 << 1;
const NO_ID: u32 = u32::MAX;
const INSTANCE_CLASSES: Type = Type::RECORD.union(Type::OBJECT);

/// Associative, commutative, idempotent refinement of a [`Type`] used inside
/// the fixed-point solver (light-cljs-1100): each of the four refinable
/// classes (`FN`, `RECORD|OBJECT`, `VECTOR`, `HOST`) tracks its refinement
/// independently as a height-2 flat lattice (`Bot` when the class is absent
/// from `ty`, `Exact` when refined, `Top` when unrefined).
///
/// Projecting to [`CompactInferred`] / [`Known`] happens only at query
/// boundaries (`to_compact`), so parallel BSP slice merges and serial sweeps
/// converge to the exact same state regardless of join order or grouping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Refined {
    ty: Type,
    /// `REF_VECTOR`: every `VECTOR` value is a flat vector (`Known::Vector`).
    /// `REF_ARRAY`: every `HOST` value is a JS array (`Known::Array`).
    flags: u8,
    /// Single `Fn` [`ExprId`] for `FN` values, or `NO_ID` when absent/widened.
    fn_id: u32,
    /// Single program type [`GlobalId`] for `RECORD|OBJECT` values, or `NO_ID`.
    inst_id: u32,
}

const _: () = assert!(std::mem::size_of::<Refined>() == 12);

impl Refined {
    const NEVER: Self = Self::of(Type::NEVER);
    const ANY: Self = Self::of(Type::ANY);

    #[inline]
    const fn of(ty: Type) -> Self {
        Self {
            ty,
            flags: 0,
            fn_id: NO_ID,
            inst_id: NO_ID,
        }
    }

    #[inline]
    fn normalized(mut self) -> Self {
        if self.fn_id != NO_ID && self.ty.intersection(Type::FN).is_never() {
            self.fn_id = NO_ID;
        }
        if self.inst_id != NO_ID && self.ty.intersection(INSTANCE_CLASSES).is_never() {
            self.inst_id = NO_ID;
        }
        if (self.flags & REF_VECTOR) != 0 && self.ty.intersection(Type::VECTOR).is_never() {
            self.flags &= !REF_VECTOR;
        }
        if (self.flags & REF_ARRAY) != 0 && self.ty.intersection(Type::HOST).is_never() {
            self.flags &= !REF_ARRAY;
        }
        self
    }

    #[inline]
    fn union(self, other: Self) -> Self {
        if self == other || other.ty.is_never() {
            return self;
        }
        if self.ty.is_never() {
            return other;
        }
        let ty = self.ty.union(other.ty);

        let a_has_vec = !self.ty.intersection(Type::VECTOR).is_never();
        let b_has_vec = !other.ty.intersection(Type::VECTOR).is_never();
        let vec_flag = if (a_has_vec || b_has_vec)
            && (!a_has_vec || (self.flags & REF_VECTOR) != 0)
            && (!b_has_vec || (other.flags & REF_VECTOR) != 0)
        {
            REF_VECTOR
        } else {
            0
        };

        let a_has_host = !self.ty.intersection(Type::HOST).is_never();
        let b_has_host = !other.ty.intersection(Type::HOST).is_never();
        let array_flag = if (a_has_host || b_has_host)
            && (!a_has_host || (self.flags & REF_ARRAY) != 0)
            && (!b_has_host || (other.flags & REF_ARRAY) != 0)
        {
            REF_ARRAY
        } else {
            0
        };

        let a_has_fn = !self.ty.intersection(Type::FN).is_never();
        let b_has_fn = !other.ty.intersection(Type::FN).is_never();
        let fn_id = match (a_has_fn, b_has_fn) {
            (false, false) => NO_ID,
            (true, false) => self.fn_id,
            (false, true) => other.fn_id,
            (true, true) => {
                if self.fn_id == other.fn_id {
                    self.fn_id
                } else {
                    NO_ID
                }
            }
        };

        let a_has_inst = !self.ty.intersection(INSTANCE_CLASSES).is_never();
        let b_has_inst = !other.ty.intersection(INSTANCE_CLASSES).is_never();
        let inst_id = match (a_has_inst, b_has_inst) {
            (false, false) => NO_ID,
            (true, false) => self.inst_id,
            (false, true) => other.inst_id,
            (true, true) => {
                if self.inst_id == other.inst_id {
                    self.inst_id
                } else {
                    NO_ID
                }
            }
        };

        Self {
            ty,
            flags: vec_flag | array_flag,
            fn_id,
            inst_id,
        }
    }

    #[inline]
    fn project_known(self) -> (u8, u32) {
        let has_fn = self.fn_id != NO_ID;
        let has_inst = self.inst_id != NO_ID;
        let has_vec = (self.flags & REF_VECTOR) != 0;
        let has_array = (self.flags & REF_ARRAY) != 0;
        let count = (has_fn as u8) + (has_inst as u8) + (has_vec as u8) + (has_array as u8);
        if count != 1 {
            return (KNOWN_UNKNOWN, 0);
        }
        if has_fn {
            (KNOWN_FUNCTION, self.fn_id)
        } else if has_inst {
            (KNOWN_INSTANCE, self.inst_id)
        } else if has_vec {
            (KNOWN_VECTOR, 0)
        } else {
            (KNOWN_ARRAY, 0)
        }
    }
}

/// Associative solver lattice value (value + sequential elements), projected to
/// 16-byte [`CompactInferred`] when stored in [`Inference`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LatticeValue {
    value: Refined,
    elements: Refined,
}

const _: () = assert!(std::mem::size_of::<LatticeValue>() == 24);

impl LatticeValue {
    const NEVER: Self = Self::of(Type::NEVER);
    const ANY: Self = Self::of(Type::ANY);

    #[inline]
    const fn of(ty: Type) -> Self {
        Self {
            value: Refined::of(ty),
            elements: if ty.intersection(SEQUENTIAL).is_never() {
                Refined::NEVER
            } else {
                Refined::ANY
            },
        }
    }

    #[inline]
    fn from_refined(value: Refined) -> Self {
        Self {
            value,
            elements: if value.ty.intersection(SEQUENTIAL).is_never() {
                Refined::NEVER
            } else {
                Refined::ANY
            },
        }
        .normalized()
    }

    #[inline]
    const fn ty(self) -> Type {
        self.value.ty
    }

    #[inline]
    fn known_function(id: ExprId) -> Self {
        let raw = u32::try_from(id.0).expect("expression id fits in u32");
        assert!(raw != NO_ID, "expression id reserved sentinel");
        Self::from_refined(Refined {
            ty: Type::FN,
            flags: 0,
            fn_id: raw,
            inst_id: NO_ID,
        })
    }

    #[inline]
    fn known_instance(ty: Type, global: GlobalId) -> Self {
        let raw = u32::try_from(global.0).expect("global id fits in u32");
        assert!(raw != NO_ID, "global id reserved sentinel");
        Self::known_instance_raw(ty, raw)
    }

    #[inline]
    fn known_instance_raw(ty: Type, global: u32) -> Self {
        Self::from_refined(Refined {
            ty,
            flags: 0,
            fn_id: NO_ID,
            inst_id: global,
        })
    }

    #[inline]
    fn known_vector(ty: Type) -> Self {
        Self::from_refined(Refined {
            ty,
            flags: REF_VECTOR,
            fn_id: NO_ID,
            inst_id: NO_ID,
        })
    }

    #[inline]
    fn known_array(ty: Type) -> Self {
        Self::from_refined(Refined {
            ty,
            flags: REF_ARRAY,
            fn_id: NO_ID,
            inst_id: NO_ID,
        })
    }

    #[inline]
    fn with_elements(mut self, element: Self) -> Self {
        self.elements = element.value.normalized();
        self.normalized()
    }

    #[inline]
    fn normalized(mut self) -> Self {
        self.value = self.value.normalized();
        if self.value.ty.intersection(SEQUENTIAL).is_never() {
            self.elements = Refined::NEVER;
        }
        self
    }

    #[inline]
    fn union(self, other: Self) -> Self {
        if self == other || other.value.ty.is_never() {
            return self;
        }
        if self.value.ty.is_never() {
            return other;
        }
        Self {
            value: self.value.union(other.value),
            elements: self.elements.union(other.elements),
        }
    }

    #[inline]
    fn keep(mut self, classes: Type) -> Self {
        self.value.ty = self.value.ty.intersection(classes);
        self.normalized()
    }

    #[inline]
    fn remove(mut self, classes: Type) -> Self {
        self.value.ty = self.value.ty.difference(classes);
        self.normalized()
    }

    #[inline]
    fn element(self) -> Self {
        let mut element = Self::NEVER;
        if !self.value.ty.intersection(SEQUENTIAL).is_never() {
            element = Self::from_refined(self.elements);
        }
        if !self.value.ty.intersection(Type::MAP).is_never() {
            element = element.union(Self::of(Type::VECTOR));
        }
        if !self.value.ty.intersection(Type::STRING).is_never() {
            element = element.union(Self::of(Type::STRING));
        }
        let other = self.value.ty.difference(
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

    #[inline]
    fn to_compact(self) -> CompactInferred {
        let (known_tag, known_id) = self.value.project_known();
        let (elem_tag, elem_id) = self.elements.project_known();
        CompactInferred {
            ty: self.value.ty,
            elem_ty: self.elements.ty,
            known_tag,
            elem_tag,
            known_id,
            elem_id,
        }
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
    /// The program writes a core protocol's keyed member by name, or raw
    /// text spells one.
    core_slots_by_name: bool,
    /// The first expression that writes a core protocol method's keyed
    /// member by name, or raw text that spells one.
    core_method_write: Option<ExprId>,
    /// The program reads or writes a core protocol's keyed member or a
    /// protocol mask by name, or raw text spells one.
    protocol_members_named: bool,
    /// Some program type implements `IEquiv`.
    equiv: bool,
    /// The program writes `IEquiv`'s method member by name, or raw text
    /// spells it.
    equiv_by_name: bool,
    /// Some program type implements a core protocol whose methods receive the
    /// function an operation is given: `IReduce`, `IKVReduce` or `ISwap`.
    receives_functions: bool,
    globals: Vec<Definition>,
    /// Type globals the program implements `ILookup` on (in the definition
    /// or with `extend-type`).
    lookups: Vec<bool>,
    /// Top-level position of each direct top-level form, by expression
    /// (`u32::MAX` when not a direct top-level form).
    top: Vec<u32>,
    /// Program protocols by marker key (`ns/Name`).
    protocols: HashMap<String, Implementors>,
    /// Expressions that set a pessimistic summary flag (`replaced`, `extended`,
    /// `equiv`, `equiv_by_name`, or writes to core-extension targets), populated
    /// only in lightweight scans to skip post-compaction refreshes when none of
    /// them were compacted away (light-cljs-1098).
    pessimistic_exprs: Vec<ExprId>,
}

impl ProgramFacts {
    /// Scan `program`, whose flattened top-level forms are `top`.
    pub fn new(program: &Program, top: &[ExprId]) -> Self {
        Self::build(program, top, true)
    }

    /// Lightweight scan that skips program-protocol implementor tables
    /// (`protocols`), `lookups`, and emission-only protocol member name scans,
    /// for `retain`, `folding`, and `inlining` (light-cljs-1098).
    pub(crate) fn new_light(program: &Program, top: &[ExprId]) -> Self {
        Self::build(program, top, false)
    }

    /// Release per-expression and per-global lookup tables once `Inert` has
    /// read them; `folding` and `inlining` only read summary flags and
    /// `replaced`.
    pub(crate) fn discard_tables(&mut self) {
        self.globals = Vec::new();
        self.lookups = Vec::new();
        self.top = Vec::new();
        self.protocols = HashMap::new();
    }

    /// Release compaction invalidation tracking once compaction has run.
    pub(crate) fn clear_pessimistic(&mut self) {
        self.pessimistic_exprs = Vec::new();
    }

    /// Whether any expression that contributed a pessimistic summary flag
    /// (`replaced`, `core_protocols`, `equiv`, `equiv_by_name`) was dropped by
    /// `compact_expressions` (`ids[old] == ExprId(usize::MAX)`).
    pub(crate) fn any_pessimistic_dropped(&self, ids: &[ExprId]) -> bool {
        self.pessimistic_exprs
            .iter()
            .any(|id| ids.get(id.0).copied().unwrap_or(ExprId(usize::MAX)) == ExprId(usize::MAX))
    }

    fn build<'p>(program: &'p Program, top: &[ExprId], full: bool) -> Self {
        let mut definitions = vec![0usize; program.globals.len()];
        let mut assigned = vec![false; program.globals.len()];
        let mut replaced = vec![false; CoreFnId::ALL.len()];
        let mut extended = Vec::new();
        let mut lookup_targets = Vec::new();
        // Extensions name core protocols and their methods by `Builtin` keys,
        // and also mark the program's own protocols with arity-0 `ns/Name` ones,
        // looked up for every method of every type, so by hash (light-cljs-417).
        let markers: HashSet<String> = program
            .protocols
            .keys()
            .map(|protocol| {
                let binding = &program.globals[protocol.0];
                format!("{}/{}", binding.namespace, binding.name)
            })
            .collect();
        fn marker_key<'m>(
            markers: &HashSet<String>,
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
        // counts as replacing it. Returns whether a core operation was marked
        // and, in lightweight mode, records global assignments so compaction
        // invalidation sees dropped type assignments too.
        let mut assigned_exprs: Vec<(GlobalId, ExprId)> = Vec::new();
        let mut assign = |expr_index: usize, target: &ExprId| -> bool {
            let target = match program.exprs[target.0].kind {
                ExprKind::Property { receiver, .. } | ExprKind::JsIndex { receiver, .. } => {
                    receiver
                }
                _ => *target,
            };
            match program.exprs[target.0].kind {
                ExprKind::CoreFn(core) => {
                    replaced[core.table_row()] = true;
                    true
                }
                ExprKind::Global(global) => {
                    assigned[global.0] = true;
                    if !full {
                        assigned_exprs.push((global, ExprId(expr_index)));
                    }
                    false
                }
                _ => false,
            }
        };
        // Definitions count where emission can reach them: analysis leaves
        // scaffolding in the arena, such as the `def` without a value that
        // `defprotocol` makes for each Var, which no output contains.
        let mut emitted = vec![false; program.exprs.len()];
        let mut pending = vec![program.root];
        while let Some(id) = pending.pop() {
            if !std::mem::replace(&mut emitted[id.0], true) {
                crate::program_fragment::visit_children(&program.exprs[id.0].kind, |child| {
                    if !emitted[child.0] {
                        pending.push(child);
                    }
                });
            }
        }
        // Whether the program writes a core protocol's keyed member by its
        // name (`(set! (.-cljs$core$ICounted$_count$arity$1 o) f)`), or raw
        // text spells one with the key's placeholder, which can give any
        // object, a host object too, a core protocol slot.
        // A target names one as a property (`(.-cljs$core$... o)`) or as a
        // component of a `js/` path (`js/o.cljs$core$...`), which the
        // analyzer keys alike; `set!`, `binding` and a raw text's hole may
        // assign it.
        let mut core_slots_by_name = false;
        // ... and the first such write of a protocol method's member (a name
        // holding `$arity$`), which can give a runtime class's value a method
        // of its own (`core_method_write`); a marker written by name declares
        // nothing, as it is no protocol sentinel.
        let mut core_method_write = None;
        // ... and whether any expression names a core protocol's keyed member
        // or a protocol mask at all, to read or write it
        // (`protocol_members_named`).
        let mut protocol_members_named = false;
        let protocol_member_name = |name: &str| {
            (name.starts_with("cljs$core$") || name.starts_with("cljs$lang$protocol_mask$"))
                && crate::analyzer::keyed_name(name)
        };
        let core_member =
            |name: &str| name.starts_with("cljs$core$") && crate::analyzer::keyed_name(name);
        let core_slot_target = |target: &ExprId| match &program.exprs[target.0].kind {
            ExprKind::Property { property, .. } => core_member(property),
            ExprKind::JsGlobal(global) => program.js_globals[global.0]
                .path
                .iter()
                .any(|component| core_member(component)),
            _ => false,
        };
        let core_method_target = |target: &ExprId| match &program.exprs[target.0].kind {
            ExprKind::Property { property, .. } => {
                core_member(property) && property.contains("$arity$")
            }
            ExprKind::JsGlobal(global) => program.js_globals[global.0]
                .path
                .iter()
                .any(|component| core_member(component) && component.contains("$arity$")),
            _ => false,
        };
        let spelled = |segment: &String, method: bool| {
            segment.contains("cljs$core$")
                && segment.contains(crate::rta::RUNTIME_KEY)
                && (!method || segment.contains("$arity$"))
        };
        // ... and whether one of those writes is `IEquiv`'s method, which can
        // give any value, a keyword constant or a string's prototype too, a
        // `-equiv` method of its own that `=` calls (`equiv_by_name`).
        let mut equiv_by_name = false;
        let equiv_member = |name: &str| {
            name.strip_suffix(crate::rta::RUNTIME_KEY) == Some("cljs$core$IEquiv$_equiv$arity$2")
        };
        let equiv_target = |target: &ExprId| match &program.exprs[target.0].kind {
            ExprKind::Property { property, .. } => equiv_member(property),
            ExprKind::JsGlobal(global) => program.js_globals[global.0]
                .path
                .iter()
                .any(|component| equiv_member(component)),
            _ => false,
        };
        let mut def_exprs: Vec<u32> = Vec::new();
        let mut protocol_impl_exprs: Vec<u32> = Vec::new();
        let mut via_metadata = Vec::new();
        let mut pessimistic_exprs = Vec::new();
        for (index, expr) in program.exprs.iter().enumerate() {
            let mut pessimistic = false;
            if full {
                protocol_members_named |= match &expr.kind {
                    ExprKind::Property { property, .. } => protocol_member_name(property),
                    ExprKind::JsGlobal(global) => program.js_globals[global.0]
                        .path
                        .iter()
                        .any(|component| protocol_member_name(component)),
                    ExprKind::JsTemplate { segments, .. } => segments.iter().any(|segment| {
                        (segment.contains("cljs$core$")
                            || segment.contains("cljs$lang$protocol_mask$"))
                            && segment.contains(crate::rta::RUNTIME_KEY)
                    }),
                    _ => false,
                };
            }
            let method_write = match &expr.kind {
                ExprKind::SetGlobal { target, .. } => core_method_target(target),
                ExprKind::Binding { bindings, .. } => bindings
                    .iter()
                    .any(|(target, _)| core_method_target(target)),
                ExprKind::JsTemplate { segments, args } => {
                    args.iter().any(core_method_target)
                        || segments.iter().any(|segment| spelled(segment, true))
                }
                _ => false,
            };
            if full && method_write && core_method_write.is_none() {
                core_method_write = Some(ExprId(index));
            }
            if method_write
                && match &expr.kind {
                    ExprKind::SetGlobal { target, .. } => equiv_target(target),
                    ExprKind::Binding { bindings, .. } => {
                        bindings.iter().any(|(target, _)| equiv_target(target))
                    }
                    ExprKind::JsTemplate { segments, args } => {
                        args.iter().any(equiv_target)
                            || segments.iter().any(|segment| {
                                segment.contains("cljs$core$IEquiv$_equiv$")
                                    && segment.contains(crate::rta::RUNTIME_KEY)
                            })
                    }
                    _ => false,
                }
            {
                equiv_by_name = true;
                pessimistic = true;
            }
            match &expr.kind {
                ExprKind::SetGlobal { target, .. } => {
                    if full {
                        core_slots_by_name |= core_slot_target(target);
                    }
                    pessimistic |= assign(index, target);
                }
                ExprKind::Binding { bindings, .. } => {
                    if full {
                        core_slots_by_name |=
                            bindings.iter().any(|(target, _)| core_slot_target(target));
                    }
                    for (target, _) in bindings {
                        pessimistic |= assign(index, target);
                    }
                }
                // Raw text may assign its holes.
                ExprKind::JsTemplate { segments, args } => {
                    if full {
                        core_slots_by_name |= args.iter().any(core_slot_target)
                            || segments.iter().any(|segment| spelled(segment, false));
                    }
                    for arg in args {
                        pessimistic |= assign(index, arg);
                    }
                }
                ExprKind::ExtendType {
                    target, methods, ..
                } => {
                    if full {
                        protocol_impl_exprs.push(index as u32);
                    }
                    if methods.iter().any(core_protocol) {
                        extended.push(*target);
                        pessimistic = true;
                    }
                    if full && methods.iter().any(lookup) {
                        lookup_targets.push(*target);
                    }
                    if methods.iter().any(implements("IEquiv")) {
                        equiv = true;
                        pessimistic = true;
                    }
                    if full {
                        receives_functions |= methods.iter().any(receives);
                    }
                }
                ExprKind::Reify { methods, .. } => {
                    if full {
                        protocol_impl_exprs.push(index as u32);
                    }
                    if methods.iter().any(implements("IEquiv")) {
                        equiv = true;
                        pessimistic = true;
                    }
                    if full {
                        receives_functions |= methods.iter().any(receives);
                    }
                }
                ExprKind::Global(_) => (),
                kind => {
                    match kind {
                        ExprKind::Def { .. } => {
                            if emitted[index] {
                                def_exprs.push(index as u32);
                            }
                        }
                        ExprKind::DefType { methods, .. } => {
                            if full {
                                protocol_impl_exprs.push(index as u32);
                            }
                            if methods.iter().any(implements("IEquiv")) {
                                equiv = true;
                                pessimistic = true;
                            }
                            if full {
                                receives_functions |= methods.iter().any(receives);
                            }
                            if emitted[index] {
                                def_exprs.push(index as u32);
                            }
                        }
                        ExprKind::DefProtocol {
                            global,
                            via_metadata: is_via_metadata,
                            ..
                        } => {
                            if full && *is_via_metadata {
                                via_metadata.push(*global);
                            }
                            if emitted[index] {
                                def_exprs.push(index as u32);
                            }
                        }
                        _ => (),
                    }
                    if emitted[index] {
                        crate::application::visit_globals(kind, |global| {
                            definitions[global.0] += 1;
                        });
                    }
                }
            }
            if !full && pessimistic {
                pessimistic_exprs.push(ExprId(index));
            }
        }
        drop(emitted);
        let mut globals = vec![Definition::Unknown; program.globals.len()];
        let mut lookups = if full {
            vec![false; program.globals.len()]
        } else {
            Vec::new()
        };
        {
            let sole = |global: GlobalId| definitions[global.0] == 1 && !assigned[global.0];
            for index in def_exprs {
                let index = index as usize;
                let id = ExprId(index);
                match &program.exprs[index].kind {
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
                        if let Some(factory) =
                            layout.map_constructor.filter(|factory| sole(*factory))
                        {
                            globals[factory.0] = role(TypeRole::MapFactory);
                        }
                        if full {
                            lookups[global.0] = methods.iter().any(lookup);
                        }
                    }
                    ExprKind::DefProtocol {
                        global, methods, ..
                    } => {
                        for global in
                            std::iter::once(*global).chain(methods.iter().map(|m| m.global))
                        {
                            if sole(global) {
                                globals[global.0] = Definition::Protocol(id);
                            }
                        }
                    }
                    _ => (),
                }
            }
        }
        drop(definitions);
        drop(assigned);
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
        if full {
            // An extension to anything else is a core protocol extension beyond the
            // program's types, which `core_protocols` records.
            for target in lookup_targets {
                if let Some((global, _)) = program_type(target) {
                    lookups[global.0] = true;
                }
            }
        }
        let mut top_positions = vec![u32::MAX; program.exprs.len()];
        for (position, id) in top.iter().enumerate() {
            top_positions[id.0] = position as u32;
        }
        // The core protocol condition holds when every extension of a core
        // protocol targets a program type defined once at top level.
        let core_protocols = extended.iter().all(|target| {
            matches!(program.exprs[target.0].kind, ExprKind::Global(global)
                if matches!(globals[global.0], Definition::Type {
                    role: TypeRole::Constructor,
                    definition,
                    ..
                } if top_positions[definition.0] != u32::MAX))
        });
        if !full && !core_protocols && !assigned_exprs.is_empty() {
            let extended_globals: HashSet<GlobalId> = extended
                .iter()
                .filter_map(|target| match program.exprs[target.0].kind {
                    ExprKind::Global(global) => Some(global),
                    _ => None,
                })
                .collect();
            for (global, expr_id) in assigned_exprs {
                if extended_globals.contains(&global) {
                    pessimistic_exprs.push(expr_id);
                }
            }
        }
        let mut protocols: HashMap<String, Implementors> = HashMap::new();
        if full {
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
            for index in protocol_impl_exprs {
                let index = index as usize;
                let (classes, program_type, methods) = match &program.exprs[index].kind {
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
        }
        Self {
            replaced,
            core_protocols,
            core_slots_by_name,
            core_method_write,
            protocol_members_named,
            equiv,
            equiv_by_name,
            receives_functions,
            globals,
            lookups,
            top: top_positions,
            protocols,
            pessimistic_exprs,
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

    /// Whether only the runtime's classes, the program's types and `reify`
    /// values, which all have a keyed kind, can hold a core protocol's keyed
    /// slot: the core protocols are intact, and the program neither writes a
    /// core protocol member by name nor spells one in raw text. Then a
    /// value's slot is what an operation's slot-first wrapper would call
    /// first, without reading the value's kind (light-cljs-1016).
    pub fn core_slots_runtime_only(&self) -> bool {
        self.core_protocols && !self.core_slots_by_name
    }

    /// The first expression that writes a core protocol method's keyed
    /// member by name (`(set! (.-cljs$core$IMeta$_meta$arity$1 v) f)`), or
    /// raw text that spells one. Such a write can give a runtime class's
    /// value a method of its own without `extend-type` or `specify!`, which
    /// only the lookup that `extend-type` installs reads (`extendedMethod`),
    /// so an `Advanced` slice keeps `extendType` for it (light-cljs-1056).
    pub fn core_method_write(&self) -> Option<ExprId> {
        self.core_method_write
    }

    /// Whether the program reads or writes a core protocol's keyed member or
    /// a protocol mask by name (`(.-cljs$core$IRecord$ r)`), or raw text
    /// spells one: then a record type at `Advanced` keeps its markers, which
    /// it otherwise takes as their masks alone (`defineType`,
    /// light-cljs-1069), so that what the program reads is what it read.
    pub fn protocol_members_named(&self) -> bool {
        self.protocol_members_named
    }

    /// Whether a `deftype`, `defrecord`, `reify` or `extend-type` of the program
    /// implements `IEquiv`, or the program writes its method by name
    /// ([`ProgramFacts::equiv_by_name`]). Otherwise `=` runs only the
    /// runtime's implementations (while core protocols are intact), and a
    /// record's compares only with records of its type.
    pub fn implements_equiv(&self) -> bool {
        self.equiv || self.equiv_by_name
    }

    /// Whether the program writes `IEquiv`'s method member by name (`(set!
    /// (.-cljs$core$IEquiv$_equiv$arity$2 k) f)`), or raw text spells it.
    /// Such a write can give any value a `-equiv` method of its own without
    /// `extend-type` or `specify!`, which keep core protocols from being
    /// intact: a keyword, a keyword constant, a host object, or a string's
    /// prototype. `=` calls it at every level, as upstream's `=` calls a
    /// value's `-equiv` (light-cljs-1061), so no comparison may be decided
    /// from the operands' types or spellings.
    pub fn equiv_by_name(&self) -> bool {
        self.equiv_by_name
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
        self.top
            .get(id.0)
            .copied()
            .filter(|&pos| pos != u32::MAX)
            .map(|pos| pos as usize)
    }

    /// The classes satisfying the program protocol with marker key `key`
    /// (`ns/Name`); `None` for core protocols and unknown keys.
    pub fn implementors(&self, key: &str) -> Option<Implementors> {
        self.protocols.get(key).copied()
    }
}

/// The result of [`Inference::new`]; see the module documentation.
pub struct Inference {
    values: Vec<CompactInferred>,
    locals: Vec<CompactInferred>,
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
        Self::new_with_slots(program, facts, closed, crate::scheduler::Slots::ONE)
    }

    /// [`Inference::new`] with a work-slot budget for parallel dirty-form BSP
    /// waves and final value population on [`crate::scheduler::map`]
    /// (light-cljs-1100). With [`crate::scheduler::Slots::ONE`] everything runs
    /// inline with zero thread overhead.
    pub fn new_with_slots(
        program: &Program,
        facts: ProgramFacts,
        closed: &ClosedFunctions,
        slots: crate::scheduler::Slots,
    ) -> Self {
        Self::build(
            program,
            facts,
            closed,
            slots,
            PARALLEL_DIRTY_MIN_NODES,
            PARALLEL_FINAL_MIN_NODES,
        )
    }

    fn build(
        program: &Program,
        facts: ProgramFacts,
        closed: &ClosedFunctions,
        slots: crate::scheduler::Slots,
        parallel_dirty_min_nodes: usize,
        parallel_final_min_nodes: usize,
    ) -> Self {
        let mut solver = Solver::new(program, &facts, closed);
        let mut solved = solver.solve(slots, parallel_dirty_min_nodes);
        // If BSP snapshot lag caused the multi-slot solve to exhaust
        // `ROUND_LIMIT`, retry serially with the original `ClosedFunctions`
        // before discarding parameter types.
        if solved.is_none() && slots != crate::scheduler::Slots::ONE {
            solver = Solver::new(program, &facts, closed);
            solved = solver.solve(crate::scheduler::Slots::ONE, parallel_dirty_min_nodes);
        }
        // Parameter types that do not converge (a call chain longer than the
        // round limit runs one round per link) must not cost the program the
        // types it has without them.
        let without = ClosedFunctions::default();
        if solved.is_none() && !closed.is_empty() {
            solver = Solver::new(program, &facts, &without);
            solved = solver.solve(slots, parallel_dirty_min_nodes);
            if solved.is_none() && slots != crate::scheduler::Slots::ONE {
                solver = Solver::new(program, &facts, &without);
                solved = solver.solve(crate::scheduler::Slots::ONE, parallel_dirty_min_nodes);
            }
        }
        let (values, locals, uses, rounds, parameters) = match solved {
            Some((rounds, form_nodes)) => {
                let values = solver.populate_values(slots, &form_nodes, parallel_final_min_nodes);
                let Solver {
                    ctx, state, uses, ..
                } = solver;
                let locals = state
                    .locals
                    .into_iter()
                    .zip(ctx.pinned)
                    .map(|(value, pinned)| {
                        if pinned {
                            CompactInferred::ANY
                        } else {
                            value.to_compact()
                        }
                    })
                    .collect();
                (values, locals, uses, rounds, ctx.parameters)
            }
            None => {
                let Solver { ctx, uses, .. } = solver;
                (
                    vec![CompactInferred::ANY; program.exprs.len()],
                    vec![CompactInferred::ANY; program.locals.len()],
                    uses,
                    ROUND_LIMIT,
                    ctx.parameters,
                )
            }
        };
        let typed_parameters = parameters
            .iter()
            .filter(|parameter| locals[parameter.0] != CompactInferred::ANY)
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
        self.values
            .get(id.0)
            .map_or(Inferred::ANY, |value| value.to_inferred())
    }

    /// The inferred type of `id`.
    pub fn ty(&self, id: ExprId) -> Type {
        self.values.get(id.0).map_or(Type::ANY, |value| value.ty)
    }

    /// Every value `local` is bound to, before any narrowing.
    pub fn local(&self, local: LocalId) -> Inferred {
        self.locals
            .get(local.0)
            .map_or(Inferred::ANY, |value| value.to_inferred())
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
            ExprKind::Fn { methods, .. } => methods
                .iter()
                .fold(CompactInferred::NEVER, |value, method| {
                    value.union(if method.asynchronous {
                        CompactInferred::ANY
                    } else {
                        self.values
                            .get(method.body.0)
                            .copied()
                            .unwrap_or(CompactInferred::ANY)
                    })
                })
                .to_inferred(),
            _ => Inferred::ANY,
        }
    }

    /// The `Fn` expression that the callee of a call evaluates to, when known.
    pub fn callee(&self, program: &Program, call: ExprId) -> Option<ExprId> {
        let ExprKind::Call { callee, .. } = program.exprs[call.0].kind else {
            return None;
        };
        let value = self
            .values
            .get(callee.0)
            .copied()
            .unwrap_or(CompactInferred::ANY);
        if value.known_tag == KNOWN_FUNCTION && Type::FN.union(Type::NIL).contains(value.ty) {
            Some(ExprId(value.known_id as usize))
        } else {
            None
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

/// Minimum AST nodes in the dirty set to run a parallel BSP wave.
const PARALLEL_DIRTY_MIN_NODES: usize = 16_384;
/// Minimum total AST nodes to run the final `values` pass in parallel.
const PARALLEL_FINAL_MIN_NODES: usize = 32_768;
/// Switch remaining rounds to serial after this many parallel BSP waves so deep
/// cross-slice dependency chains always converge within `ROUND_LIMIT`.
const PARALLEL_ROUND_LIMIT: usize = ROUND_LIMIT / 2;
/// Sequential mini-batches for large parallel final passes, bounding transient
/// `(ExprId, CompactInferred)` slice buffers.
const FINAL_PASS_BATCHES: usize = 4;
const LOCAL_UNASSIGNED: u32 = u32::MAX;
const LOCAL_MULTI_FORM: u32 = u32::MAX - 1;

const INLINE_ARGS: usize = 8;
const INLINE_FILTERS: usize = 4;

enum ArgBuf {
    Inline([LatticeValue; INLINE_ARGS], usize),
    Heap(Vec<LatticeValue>),
}

impl ArgBuf {
    #[inline]
    fn as_slice(&self) -> &[LatticeValue] {
        match self {
            Self::Inline(buf, len) => &buf[..*len],
            Self::Heap(vec) => vec.as_slice(),
        }
    }

    #[inline]
    fn push(&mut self, value: LatticeValue) {
        match self {
            Self::Inline(buf, len) => {
                if *len < INLINE_ARGS {
                    buf[*len] = value;
                    *len += 1;
                } else {
                    let mut vec = Vec::with_capacity(INLINE_ARGS * 2);
                    vec.extend_from_slice(buf);
                    vec.push(value);
                    *self = Self::Heap(vec);
                }
            }
            Self::Heap(vec) => vec.push(value),
        }
    }
}

enum TypeBuf {
    Inline([Type; INLINE_ARGS], usize),
    Heap(Vec<Type>),
}

impl TypeBuf {
    #[inline]
    fn from_args(arguments: &[LatticeValue]) -> Self {
        if arguments.len() <= INLINE_ARGS {
            let mut buf = [Type::NEVER; INLINE_ARGS];
            for (slot, arg) in buf.iter_mut().zip(arguments) {
                *slot = arg.ty();
            }
            Self::Inline(buf, arguments.len())
        } else {
            Self::Heap(arguments.iter().map(|value| value.ty()).collect())
        }
    }

    #[inline]
    fn as_slice(&self) -> &[Type] {
        match self {
            Self::Inline(buf, len) => &buf[..*len],
            Self::Heap(vec) => vec.as_slice(),
        }
    }
}

/// A narrowing of one local in one branch.
#[derive(Clone, Copy)]
enum Filter {
    Keep(Type),
    Remove(Type),
    Instance(u32, Type),
}

impl Filter {
    #[inline]
    fn apply(self, value: LatticeValue) -> LatticeValue {
        match self {
            Self::Keep(classes) => value.keep(classes),
            Self::Remove(classes) => value.remove(classes),
            Self::Instance(global, class) => {
                LatticeValue::known_instance_raw(value.ty().intersection(class), global)
            }
        }
    }
}

enum FilterBuf {
    Inline([(LocalId, Filter); INLINE_FILTERS], usize),
    Heap(Vec<(LocalId, Filter)>),
}

impl Default for FilterBuf {
    #[inline]
    fn default() -> Self {
        Self::Inline([(LocalId(0), Filter::Keep(Type::NEVER)); INLINE_FILTERS], 0)
    }
}

impl FilterBuf {
    #[inline]
    fn one(entry: (LocalId, Filter)) -> Self {
        let mut buf = [(LocalId(0), Filter::Keep(Type::NEVER)); INLINE_FILTERS];
        buf[0] = entry;
        Self::Inline(buf, 1)
    }

    #[inline]
    fn as_slice(&self) -> &[(LocalId, Filter)] {
        match self {
            Self::Inline(buf, len) => &buf[..*len],
            Self::Heap(vec) => vec.as_slice(),
        }
    }

    #[inline]
    fn push(&mut self, entry: (LocalId, Filter)) {
        match self {
            Self::Inline(buf, len) => {
                if *len < INLINE_FILTERS {
                    buf[*len] = entry;
                    *len += 1;
                } else {
                    let mut vec = Vec::with_capacity(INLINE_FILTERS * 2);
                    vec.extend_from_slice(buf);
                    vec.push(entry);
                    *self = Self::Heap(vec);
                }
            }
            Self::Heap(vec) => vec.push(entry),
        }
    }

    fn extend_from(&mut self, other: &Self) {
        for &entry in other.as_slice() {
            self.push(entry);
        }
    }
}

/// What a test tells about locals when it is truthy and when it is not.
#[derive(Default)]
struct Narrowing {
    yes: FilterBuf,
    no: FilterBuf,
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
    Type(u32, Type),
    /// Objects of a `reify`, which are no program type's instances.
    Reify,
    /// Any receiver: the target of `specify!` may be any object.
    Any,
}

impl Receivers {
    /// Whether a call with receiver `receiver` may run the implementation.
    #[inline]
    fn reach(self, receiver: LatticeValue, inherits: bool) -> bool {
        let ty = receiver.ty();
        let inst = receiver.value.inst_id;
        match self {
            _ if inherits || !ty.intersection(Type::HOST).is_never() => !ty.is_never(),
            Self::Type(global, class) => {
                !ty.intersection(class).is_never() && !(inst != NO_ID && inst != global)
            }
            Self::Reify => !ty.intersection(Type::OBJECT).is_never() && inst == NO_ID,
            Self::Any => true,
        }
    }
}

/// Compact bitset supporting fast forward/reverse sweeps over dirty forms.
struct BitSet {
    words: Vec<u64>,
}

impl BitSet {
    fn new(bits: usize) -> Self {
        Self {
            words: vec![0; bits.div_ceil(64)],
        }
    }

    #[inline]
    fn set(&mut self, index: usize) {
        self.words[index >> 6] |= 1u64 << (index & 63);
    }

    #[inline]
    fn get(&self, index: usize) -> bool {
        (self.words[index >> 6] & (1u64 << (index & 63))) != 0
    }

    #[inline]
    fn clear(&mut self) {
        self.words.fill(0);
    }

    fn is_empty(&self) -> bool {
        self.words.iter().all(|&word| word == 0)
    }

    /// Remove and return the smallest set bit `>= from`.
    #[inline]
    fn pop_next(&mut self, from: usize) -> Option<usize> {
        let mut word_idx = from >> 6;
        if word_idx >= self.words.len() {
            return None;
        }
        let mut word = self.words[word_idx] & (!0u64 << (from & 63));
        while word == 0 {
            word_idx += 1;
            if word_idx >= self.words.len() {
                return None;
            }
            word = self.words[word_idx];
        }
        let bit = word.trailing_zeros() as usize;
        self.words[word_idx] &= !(1u64 << bit);
        Some((word_idx << 6) | bit)
    }

    /// Remove and return the largest set bit `<= from`.
    #[inline]
    fn pop_prev(&mut self, from: usize) -> Option<usize> {
        if self.words.is_empty() {
            return None;
        }
        let mut word_idx = (from >> 6).min(self.words.len() - 1);
        let mask = if (from >> 6) < self.words.len() {
            !0u64 >> (63 - (from & 63))
        } else {
            !0u64
        };
        let mut word = self.words[word_idx] & mask;
        while word == 0 {
            if word_idx == 0 {
                return None;
            }
            word_idx -= 1;
            word = self.words[word_idx];
        }
        let bit = 63 - word.leading_zeros() as usize;
        self.words[word_idx] &= !(1u64 << bit);
        Some((word_idx << 6) | bit)
    }
}

/// Unified dense numbering of inter-form summaries:
/// `[0..fn_base)`: `def_values` by `GlobalId`
/// `[fn_base..param_base)`: `fn_returns` by dense `fn_slot`
/// `[param_base..atom_base)`: closed parameter `locals` by dense `param_slot`
/// `[atom_base..total)`: closed atom `contents` by atom index
#[derive(Clone, Copy)]
struct SummaryLayout {
    fn_base: usize,
    param_base: usize,
    atom_base: usize,
    total: usize,
}

impl SummaryLayout {
    #[inline]
    fn def_dep(&self, global: GlobalId) -> usize {
        global.0
    }

    #[inline]
    fn fn_dep(&self, slot: u32) -> usize {
        self.fn_base + slot as usize
    }

    #[inline]
    fn param_dep(&self, slot: u32) -> usize {
        self.param_base + slot as usize
    }

    #[inline]
    fn atom_dep(&self, atom: usize) -> usize {
        self.atom_base + atom
    }
}

/// Inverted subscriber graph mapping each inter-form summary `dep` to the
/// sorted list of top-level form indices that read it.
struct SubscriberGraph {
    offsets: Vec<u32>,
    forms: Vec<u32>,
    extra: HashMap<u32, Vec<u32>>,
}

impl SubscriberGraph {
    fn from_round1(total_deps: usize, edges: &[(u32, u32)]) -> Self {
        let mut counts = vec![0u32; total_deps];
        for &(dep, _) in edges {
            counts[dep as usize] += 1;
        }
        let mut offsets = Vec::with_capacity(total_deps + 1);
        let mut sum = 0u32;
        for &count in &counts {
            offsets.push(sum);
            sum = sum
                .checked_add(count)
                .expect("subscriber graph fits in u32");
        }
        offsets.push(sum);
        let mut forms = vec![0u32; sum as usize];
        let mut cursor = offsets[..total_deps].to_vec();
        for &(dep, form) in edges {
            let slot = &mut cursor[dep as usize];
            forms[*slot as usize] = form;
            *slot += 1;
        }
        Self {
            offsets,
            forms,
            extra: HashMap::new(),
        }
    }

    #[inline]
    fn csr_slice(&self, dep: usize) -> &[u32] {
        let start = self.offsets[dep] as usize;
        let end = self.offsets[dep + 1] as usize;
        &self.forms[start..end]
    }

    #[inline]
    fn contains(&self, dep: usize, form: u32) -> bool {
        if self.csr_slice(dep).binary_search(&form).is_ok() {
            return true;
        }
        self.extra
            .get(&(dep as u32))
            .is_some_and(|list| list.binary_search(&form).is_ok())
    }

    fn insert_extra(&mut self, dep: u32, form: u32) {
        if self.csr_slice(dep as usize).binary_search(&form).is_ok() {
            return;
        }
        let list = self.extra.entry(dep).or_default();
        if let Err(pos) = list.binary_search(&form) {
            list.insert(pos, form);
        }
    }

    #[inline]
    fn for_each(&self, dep: usize, mut f: impl FnMut(usize)) {
        for &form in self.csr_slice(dep) {
            f(form as usize);
        }
        if let Some(extra) = self.extra.get(&(dep as u32)) {
            for &form in extra {
                f(form as usize);
            }
        }
    }

    /// Invoke `f` for every subscriber `form` with `lo <= form < hi`.
    #[inline]
    fn for_each_in_range(&self, dep: usize, lo: usize, hi: usize, mut f: impl FnMut(usize)) {
        if lo >= hi {
            return;
        }
        let lo32 = lo as u32;
        let hi32 = hi as u32;
        let slice = self.csr_slice(dep);
        let start = slice.partition_point(|&form| form < lo32);
        for &form in &slice[start..] {
            if form >= hi32 {
                break;
            }
            f(form as usize);
        }
        if let Some(extra) = self.extra.get(&(dep as u32)) {
            let start = extra.partition_point(|&form| form < lo32);
            for &form in &extra[start..] {
                if form >= hi32 {
                    break;
                }
                f(form as usize);
            }
        }
    }
}

/// Read-only per-program tables shared across all solver phases and workers.
struct StaticCtx<'p> {
    program: &'p Program,
    facts: &'p ProgramFacts,
    closed: &'p ClosedFunctions,
    parameters: Vec<LocalId>,
    receivers: Vec<Option<Receivers>>,
    pinned: Vec<bool>,
    initializers: Vec<u32>,
    fn_slot: Vec<u32>,
    param_slot: Vec<u32>,
    forms: Vec<ExprId>,
    dos: Vec<ExprId>,
    layout: SummaryLayout,
}

/// Mutable fixed-point summary tables owned by the coordinator (and read as an
/// immutable snapshot during parallel BSP waves and the final pass).
struct SolverState {
    def_values: Vec<LatticeValue>,
    fn_returns: Vec<LatticeValue>,
    locals: Vec<LatticeValue>,
    contents: Vec<LatticeValue>,
}

impl SolverState {
    #[inline]
    fn read_dep(&self, ctx: &StaticCtx<'_>, dep: usize) -> LatticeValue {
        let layout = ctx.layout;
        if dep < layout.fn_base {
            self.def_values[dep]
        } else if dep < layout.param_base {
            self.fn_returns[dep - layout.fn_base]
        } else if dep < layout.atom_base {
            let local = ctx.parameters[dep - layout.param_base];
            self.locals[local.0]
        } else {
            self.contents[dep - layout.atom_base]
        }
    }

    #[inline]
    fn join_dep(&mut self, ctx: &StaticCtx<'_>, dep: usize, value: LatticeValue) -> bool {
        let layout = ctx.layout;
        let slot = if dep < layout.fn_base {
            &mut self.def_values[dep]
        } else if dep < layout.param_base {
            &mut self.fn_returns[dep - layout.fn_base]
        } else if dep < layout.atom_base {
            let local = ctx.parameters[dep - layout.param_base];
            &mut self.locals[local.0]
        } else {
            &mut self.contents[dep - layout.atom_base]
        };
        let joined = slot.union(value);
        if joined != *slot {
            *slot = joined;
            true
        } else {
            false
        }
    }
}

/// Per-form metadata discovered during Round 1.
struct FormMeta {
    form_nodes: Vec<u32>,
    local_owner: Vec<u32>,
    local_in_form: Vec<u32>,
    form_locals_flat: Vec<LocalId>,
    form_locals_offsets: Vec<u32>,
}

struct Solver<'p> {
    ctx: StaticCtx<'p>,
    state: SolverState,
    uses: Vec<u32>,
    bsp_waves: usize,
}

impl<'p> Solver<'p> {
    fn new(program: &'p Program, facts: &'p ProgramFacts, closed: &'p ClosedFunctions) -> Self {
        let expr_count = program.exprs.len();
        let local_count = program.locals.len();
        assert!(
            expr_count < u32::MAX as usize
                && program.globals.len() < u32::MAX as usize
                && local_count < u32::MAX as usize,
            "program tables fit in u32"
        );
        let mut bound = vec![false; local_count];
        let mut pinned = vec![false; local_count];
        let mut initializers = vec![u32::MAX; local_count];
        let mut fn_slot = vec![u32::MAX; expr_count];
        let mut fn_returns = Vec::new();
        let mut closed_fns = Vec::new();
        let mut has_js_arguments = false;
        let assigned = |target: ExprId, pinned: &mut Vec<bool>| {
            if let ExprKind::Local(local) = program.exprs[target.0].kind {
                pinned[local.0] = true;
            }
        };
        for (index, expr) in program.exprs.iter().enumerate() {
            match &expr.kind {
                ExprKind::Let { bindings, .. } => {
                    for &(local, init) in bindings {
                        bound[local.0] = true;
                        initializers[local.0] = init.0 as u32;
                    }
                }
                ExprKind::Loop { bindings, .. } | ExprKind::LetFn { bindings, .. } => {
                    for &(local, _) in bindings {
                        bound[local.0] = true;
                    }
                }
                ExprKind::Fn { name, methods } => {
                    if let Some(name) = name {
                        bound[name.0] = true;
                    }
                    let slot = u32::try_from(fn_returns.len()).expect("fn count fits in u32");
                    fn_slot[index] = slot;
                    fn_returns.push(if methods.iter().any(|method| method.asynchronous) {
                        LatticeValue::ANY
                    } else {
                        LatticeValue::NEVER
                    });
                    if closed.closed(ExprId(index)) {
                        closed_fns.push(ExprId(index));
                    }
                }
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
                ExprKind::JsArguments => has_js_arguments = true,
                _ => (),
            }
        }
        // `(js-arguments)` escapes the arguments object, which aliases the
        // parameters of the enclosing (non-strict) functions; any expression
        // that reads it may change them.
        if has_js_arguments {
            for expr in &program.exprs {
                match &expr.kind {
                    ExprKind::Fn { methods, .. } => {
                        for method in methods {
                            if reads_arguments(program, method.body) {
                                for param in method.params.iter().chain(&method.rest) {
                                    pinned[param.0] = true;
                                }
                            }
                        }
                    }
                    ExprKind::DefType { methods, .. }
                    | ExprKind::Reify { methods, .. }
                    | ExprKind::ExtendType { methods, .. } => {
                        for (_, method) in methods {
                            if reads_arguments(program, method.body) {
                                for param in method.params.iter().chain(&method.rest) {
                                    pinned[param.0] = true;
                                }
                            }
                        }
                    }
                    _ => (),
                }
            }
        }
        // Type fields may be mutable.
        for local in program.field_bindings.keys() {
            pinned[local.0] = true;
        }
        // The parameters of closed functions grow from their calls.
        let mut parameters = Vec::new();
        let mut param_slot = vec![u32::MAX; local_count];
        for function in closed_fns {
            if let ExprKind::Fn { methods, .. } = &program.exprs[function.0].kind {
                for method in methods {
                    for &param in method.params.iter().chain(&method.rest) {
                        if !pinned[param.0] && param_slot[param.0] == u32::MAX {
                            bound[param.0] = true;
                            param_slot[param.0] =
                                u32::try_from(parameters.len()).expect("param count fits in u32");
                            parameters.push(param);
                        }
                    }
                }
            }
        }
        // So do the parameters of the implementations of closed protocol
        // methods (light-cljs-253) that the program defines once and never
        // assigns.
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
                        Some(Receivers::Type(global.0 as u32, class(*record)))
                    }
                    ExprKind::Reify { .. } => Some(Receivers::Reify),
                    ExprKind::ExtendType { instance: true, .. } => Some(Receivers::Any),
                    ExprKind::ExtendType { target, .. } => match program.exprs[target.0].kind {
                        ExprKind::Global(global) => facts
                            .program_type(global)
                            .map(|(_, record)| Receivers::Type(global.0 as u32, class(record))),
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
                if !pinned[param.0] && param_slot[param.0] == u32::MAX {
                    bound[param.0] = true;
                    param_slot[param.0] =
                        u32::try_from(parameters.len()).expect("param count fits in u32");
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
                    LatticeValue::NEVER
                } else {
                    LatticeValue::ANY
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
        let fn_base = program.globals.len();
        let param_base = fn_base + fn_returns.len();
        let atom_base = param_base + parameters.len();
        let total = atom_base + closed.atoms();
        assert!(u32::try_from(total).is_ok(), "summary count fits in u32");
        let layout = SummaryLayout {
            fn_base,
            param_base,
            atom_base,
            total,
        };
        let state = SolverState {
            def_values: vec![LatticeValue::NEVER; program.globals.len()],
            fn_returns,
            locals,
            contents: vec![LatticeValue::NEVER; closed.atoms()],
        };
        let ctx = StaticCtx {
            program,
            facts,
            closed,
            parameters,
            receivers,
            pinned,
            initializers,
            fn_slot,
            param_slot,
            forms,
            dos,
            layout,
        };
        Self {
            ctx,
            state,
            uses: vec![0; local_count],
            bsp_waves: 0,
        }
    }

    /// Run Round 1 (forward, collecting exact read dependencies and local
    /// ownership), then iterate dirty-form fixed-point rounds alternating
    /// reverse/forward until convergence or [`ROUND_LIMIT`].
    fn solve(
        &mut self,
        slots: crate::scheduler::Slots,
        parallel_dirty_min_nodes: usize,
    ) -> Option<(usize, Vec<u32>)> {
        let form_count = self.ctx.forms.len();
        let (round1_changed, mut subs, meta, mut cur_dirty) = self.run_round1();
        if !round1_changed && cur_dirty.is_empty() {
            return Some((1, meta.form_nodes));
        }
        let mut next_dirty = BitSet::new(form_count);
        let use_parallel = slots != crate::scheduler::Slots::ONE;
        let mut narrowed = Vec::new();
        let mut local_read_stamp = Vec::new();
        for round in 2..=ROUND_LIMIT {
            if cur_dirty.is_empty() {
                return Some((round, meta.form_nodes));
            }
            let reverse = round % 2 == 0;
            let changed = if use_parallel && round <= PARALLEL_ROUND_LIMIT {
                let (dirty_count, dirty_nodes) = dirty_stats(&cur_dirty, &meta.form_nodes);
                if dirty_nodes >= parallel_dirty_min_nodes && slots.get() <= dirty_count / 2 {
                    let ranges = partition_dirty_ranges(
                        &cur_dirty,
                        &meta.form_nodes,
                        dirty_nodes,
                        slots.get(),
                    );
                    self.run_parallel_wave_bsp(
                        slots,
                        reverse,
                        &mut subs,
                        &meta,
                        &cur_dirty,
                        &ranges,
                        &mut next_dirty,
                    )
                } else {
                    self.run_serial_wave(
                        reverse,
                        &mut subs,
                        &mut cur_dirty,
                        &mut next_dirty,
                        &mut narrowed,
                        &mut local_read_stamp,
                    )
                }
            } else {
                self.run_serial_wave(
                    reverse,
                    &mut subs,
                    &mut cur_dirty,
                    &mut next_dirty,
                    &mut narrowed,
                    &mut local_read_stamp,
                )
            };
            if !changed && next_dirty.is_empty() {
                return Some((round, meta.form_nodes));
            }
            std::mem::swap(&mut cur_dirty, &mut next_dirty);
            next_dirty.clear();
        }
        None
    }

    fn run_round1(&mut self) -> (bool, SubscriberGraph, FormMeta, BitSet) {
        let form_count = self.ctx.forms.len();
        let total_deps = self.ctx.layout.total;
        let local_count = self.ctx.program.locals.len();
        let mut env = Round1Env {
            ctx: &self.ctx,
            state: &mut self.state,
            uses: &mut self.uses,
            narrowed: Vec::new(),
            position: 0,
            changed: false,
            dep_seen_stamp: vec![u32::MAX; total_deps],
            last_changed_pos: vec![u32::MAX; total_deps],
            dep_edges: Vec::with_capacity(form_count.saturating_mul(4)),
            local_read_stamp: vec![u32::MAX; local_count],
            local_owner: vec![LOCAL_UNASSIGNED; local_count],
            local_in_form: vec![u32::MAX; local_count],
            form_locals_flat: Vec::with_capacity(local_count / 2),
            form_locals_offsets: Vec::with_capacity(form_count + 1),
            cur_form_locals: 0,
            cur_form_nodes: 0,
            form_nodes: Vec::with_capacity(form_count),
            form_local_changed: false,
            self_dirty: Vec::new(),
        };
        env.form_locals_offsets.push(0);
        for pos in 0..form_count {
            env.position = pos;
            env.cur_form_locals = 0;
            env.cur_form_nodes = 0;
            env.form_local_changed = false;
            let form_id = env.ctx.forms[pos];
            walk_expr(&mut env, form_id);
            env.form_nodes.push(env.cur_form_nodes);
            env.form_locals_offsets
                .push(u32::try_from(env.form_locals_flat.len()).expect("locals fit in u32"));
            if env.form_local_changed {
                env.self_dirty.push(pos as u32);
            }
        }
        let Round1Env {
            changed,
            last_changed_pos,
            dep_edges,
            local_owner,
            local_in_form,
            form_locals_flat,
            form_locals_offsets,
            form_nodes,
            self_dirty,
            ..
        } = env;
        let subs = SubscriberGraph::from_round1(total_deps, &dep_edges);
        drop(dep_edges);
        let mut dirty = BitSet::new(form_count);
        for (dep, &last_pos) in last_changed_pos.iter().enumerate() {
            if last_pos == u32::MAX {
                continue;
            }
            for &form in subs.csr_slice(dep) {
                if form > last_pos {
                    break;
                }
                dirty.set(form as usize);
            }
        }
        for form in self_dirty {
            dirty.set(form as usize);
        }
        let meta = FormMeta {
            form_nodes,
            local_owner,
            local_in_form,
            form_locals_flat,
            form_locals_offsets,
        };
        (changed, subs, meta, dirty)
    }

    fn run_serial_wave(
        &mut self,
        reverse: bool,
        subs: &mut SubscriberGraph,
        cur_dirty: &mut BitSet,
        next_dirty: &mut BitSet,
        narrowed: &mut Vec<(LocalId, LatticeValue)>,
        local_read_stamp: &mut Vec<u32>,
    ) -> bool {
        let form_count = self.ctx.forms.len();
        if local_read_stamp.len() != self.ctx.program.locals.len() {
            local_read_stamp.resize(self.ctx.program.locals.len(), u32::MAX);
        } else {
            local_read_stamp.fill(u32::MAX);
        }
        let mut env = SerialDirtyEnv {
            ctx: &self.ctx,
            state: &mut self.state,
            subs,
            cur_dirty,
            next_dirty,
            narrowed,
            local_read_stamp: local_read_stamp.as_mut_slice(),
            position: 0,
            reverse,
            changed: false,
            form_local_changed: false,
        };
        if reverse {
            if form_count > 0 {
                let mut cursor = form_count - 1;
                while let Some(pos) = env.cur_dirty.pop_prev(cursor) {
                    env.position = pos;
                    env.form_local_changed = false;
                    let form_id = env.ctx.forms[pos];
                    walk_expr(&mut env, form_id);
                    if env.form_local_changed {
                        env.next_dirty.set(pos);
                    }
                    if pos == 0 {
                        break;
                    }
                    cursor = pos - 1;
                }
            }
        } else {
            let mut cursor = 0;
            while let Some(pos) = env.cur_dirty.pop_next(cursor) {
                env.position = pos;
                env.form_local_changed = false;
                let form_id = env.ctx.forms[pos];
                walk_expr(&mut env, form_id);
                if env.form_local_changed {
                    env.next_dirty.set(pos);
                }
                cursor = pos + 1;
            }
        }
        env.changed
    }

    #[allow(clippy::too_many_arguments)]
    fn run_parallel_wave_bsp(
        &mut self,
        slots: crate::scheduler::Slots,
        reverse: bool,
        subs: &mut SubscriberGraph,
        meta: &FormMeta,
        cur_dirty: &BitSet,
        ranges: &[(usize, usize, usize)],
        next_dirty: &mut BitSet,
    ) -> bool {
        self.bsp_waves += 1;
        let jobs: Vec<(u64, (usize, usize, usize))> = ranges
            .iter()
            .enumerate()
            .map(|(slice_idx, &(lo, hi, weight))| (weight as u64, (slice_idx, lo, hi)))
            .collect();
        let ctx = &self.ctx;
        let snapshot = &self.state;
        let subs_ref: &SubscriberGraph = subs;
        let total_deps = ctx.layout.total;
        let outputs: Vec<SliceWaveOutput> = crate::scheduler::map(slots, jobs, |(_, lo, hi)| {
            let slice_len = hi - lo;
            let mut slice_dirty = BitSet::new(slice_len);
            for pos in lo..hi {
                if cur_dirty.get(pos) {
                    slice_dirty.set(pos - lo);
                }
            }
            let mut env = ParallelSliceEnv {
                ctx,
                snapshot,
                subs: subs_ref,
                meta,
                lo,
                hi,
                reverse,
                position: lo,
                narrowed: Vec::new(),
                slice_dirty,
                touched: BitSet::new(total_deps),
                dep_deltas: HashMap::new(),
                cur_locals: Vec::new(),
                cur_local_read: Vec::new(),
                local_deltas: Vec::new(),
                extra_local_deltas: HashMap::new(),
                new_deps: Vec::new(),
                self_dirty: Vec::new(),
                form_local_changed: false,
            };
            if reverse {
                if slice_len > 0 {
                    let mut cursor = slice_len - 1;
                    while let Some(rel) = env.slice_dirty.pop_prev(cursor) {
                        let pos = lo + rel;
                        env.walk_form(pos);
                        if rel == 0 {
                            break;
                        }
                        cursor = rel - 1;
                    }
                }
            } else {
                let mut cursor = 0;
                while let Some(rel) = env.slice_dirty.pop_next(cursor) {
                    let pos = lo + rel;
                    env.walk_form(pos);
                    cursor = rel + 1;
                }
            }
            let mut deltas: Vec<(u32, LatticeValue, u32)> = env
                .dep_deltas
                .into_iter()
                .map(|(dep, (val, last_pos))| (dep, val, last_pos))
                .collect();
            deltas.sort_unstable_by_key(|&(dep, _, _)| dep);
            for (local, val) in env.extra_local_deltas {
                env.local_deltas.push((local, val));
            }
            SliceWaveOutput {
                lo,
                hi,
                deltas,
                local_deltas: env.local_deltas,
                new_deps: env.new_deps,
                self_dirty: env.self_dirty,
            }
        });

        // 1. Apply newly discovered dynamic dependencies BEFORE marking
        //    subscribers of changed summaries dirty.
        for output in &outputs {
            for &(dep, form) in &output.new_deps {
                subs.insert_extra(dep, form);
            }
        }

        // 2. Merge intra-form local deltas and self-dirty forms.
        let mut any_changed = false;
        for output in &outputs {
            for &(local, val) in &output.local_deltas {
                let slot = &mut self.state.locals[local.0];
                let joined = slot.union(val);
                if joined != *slot {
                    *slot = joined;
                    any_changed = true;
                }
            }
            for &form in &output.self_dirty {
                next_dirty.set(form as usize);
                any_changed = true;
            }
        }

        // 3. Merge inter-form summary deltas in slice order `0..P`, tracking
        //    whether a single slice wrote each changed summary so already-run
        //    intra-slice subscribers need not be needlessly re-queued.
        let mut changed_deps: HashMap<u32, (u32, usize, usize, u32)> = HashMap::new();
        for output in &outputs {
            for &(dep, val, last_pos) in &output.deltas {
                if self.state.join_dep(&self.ctx, dep as usize, val) {
                    any_changed = true;
                }
                changed_deps
                    .entry(dep)
                    .and_modify(|entry| {
                        entry.0 = entry.0.saturating_add(1);
                    })
                    .or_insert((1, output.lo, output.hi, last_pos));
            }
        }

        for (dep, (writers, lo, hi, last_pos)) in changed_deps {
            let last_pos = last_pos as usize;
            if writers == 1 {
                // Subscribers strictly ahead of `last_pos` inside `[lo..hi)`
                // were already woken and evaluated after `last_pos` wrote the
                // sole changed value of `dep`.
                if reverse {
                    subs.for_each(dep as usize, |sub| {
                        if sub < lo || sub >= last_pos {
                            next_dirty.set(sub);
                        }
                    });
                } else {
                    subs.for_each(dep as usize, |sub| {
                        if sub <= last_pos || sub >= hi {
                            next_dirty.set(sub);
                        }
                    });
                }
            } else {
                subs.for_each(dep as usize, |sub| {
                    next_dirty.set(sub);
                });
            }
        }

        any_changed
    }

    /// Populate `values` once all summaries and locals have converged.
    fn populate_values(
        &self,
        slots: crate::scheduler::Slots,
        form_nodes: &[u32],
        parallel_final_min_nodes: usize,
    ) -> Vec<CompactInferred> {
        let expr_count = self.ctx.program.exprs.len();
        let form_count = self.ctx.forms.len();
        let total_nodes: usize = form_nodes.iter().map(|&n| n as usize).sum();
        let mut values = vec![CompactInferred::UNVISITED; expr_count];
        if slots == crate::scheduler::Slots::ONE
            || total_nodes < parallel_final_min_nodes
            || slots.get() > form_count / 2
        {
            let mut env = FinalDirectEnv {
                ctx: &self.ctx,
                state: &self.state,
                values: &mut values,
                narrowed: Vec::new(),
                position: 0,
            };
            for pos in 0..form_count {
                env.position = pos;
                let form_id = env.ctx.forms[pos];
                walk_expr(&mut env, form_id);
            }
        } else {
            let batches = if total_nodes >= 200_000 {
                FINAL_PASS_BATCHES
            } else {
                1
            };
            let batch_ranges =
                partition_node_ranges(form_nodes, 0, form_count, total_nodes, batches);
            let ctx = &self.ctx;
            let state = &self.state;
            for (batch_lo, batch_hi, batch_nodes) in batch_ranges {
                let slice_ranges =
                    partition_node_ranges(form_nodes, batch_lo, batch_hi, batch_nodes, slots.get());
                let jobs: Vec<(u64, (usize, usize, usize))> = slice_ranges
                    .into_iter()
                    .map(|(lo, hi, nodes)| (nodes as u64, (lo, hi, nodes)))
                    .collect();
                let chunks: Vec<Vec<(u32, CompactInferred)>> =
                    crate::scheduler::map(slots, jobs, |(lo, hi, nodes)| {
                        let mut env = FinalSliceEnv {
                            ctx,
                            state,
                            entries: Vec::with_capacity(nodes),
                            narrowed: Vec::new(),
                            position: lo,
                        };
                        for pos in lo..hi {
                            env.position = pos;
                            let form_id = ctx.forms[pos];
                            walk_expr(&mut env, form_id);
                        }
                        env.entries
                    });
                for chunk in chunks {
                    for (id, value) in chunk {
                        record_value(&mut values, ExprId(id as usize), value);
                    }
                }
            }
        }
        // Each `do` evaluates to its last form; inner `do`s come first.
        for &id in &self.ctx.dos {
            let ExprKind::Do(items) = &self.ctx.program.exprs[id.0].kind else {
                continue;
            };
            let value = items.last().map_or(CompactInferred::of(Type::NIL), |last| {
                let last_val = values[last.0];
                if last_val.known_tag == KNOWN_UNVISITED {
                    CompactInferred::NEVER
                } else {
                    last_val
                }
            });
            record_value(&mut values, id, value);
        }
        // Expressions the walk never reached are not emitted; claim nothing.
        for slot in &mut values {
            if slot.known_tag == KNOWN_UNVISITED {
                slot.known_tag = KNOWN_UNKNOWN;
            }
        }
        values
    }
}

#[inline]
fn record_value(values: &mut [CompactInferred], id: ExprId, value: CompactInferred) {
    let slot = &mut values[id.0];
    if slot.known_tag == KNOWN_UNVISITED {
        *slot = value;
    } else {
        *slot = slot.union(value);
    }
}

fn dirty_stats(dirty: &BitSet, form_nodes: &[u32]) -> (usize, usize) {
    let mut count = 0usize;
    let mut nodes = 0usize;
    for (word_idx, &word) in dirty.words.iter().enumerate() {
        let mut w = word;
        let base = word_idx << 6;
        while w != 0 {
            let bit = w.trailing_zeros() as usize;
            w &= w - 1;
            let pos = base | bit;
            if pos < form_nodes.len() {
                count += 1;
                nodes += form_nodes[pos] as usize;
            }
        }
    }
    (count, nodes)
}

/// Partition `0..form_nodes.len()` into up to `parts` contiguous ranges
/// `(lo, hi, dirty_weight)` balanced by dirty AST node weight.
fn partition_dirty_ranges(
    dirty: &BitSet,
    form_nodes: &[u32],
    total_dirty_nodes: usize,
    parts: usize,
) -> Vec<(usize, usize, usize)> {
    let form_count = form_nodes.len();
    if form_count == 0 || parts <= 1 {
        return vec![(0, form_count, total_dirty_nodes)];
    }
    let target = (total_dirty_nodes / parts).max(1);
    let mut ranges = Vec::with_capacity(parts);
    let mut lo = 0usize;
    let mut acc = 0usize;
    for (word_idx, &word) in dirty.words.iter().enumerate() {
        let mut w = word;
        let base = word_idx << 6;
        while w != 0 {
            let bit = w.trailing_zeros() as usize;
            w &= w - 1;
            let pos = base | bit;
            if pos >= form_count {
                break;
            }
            acc += form_nodes[pos] as usize;
            if acc >= target && ranges.len() + 1 < parts {
                let hi = pos + 1;
                ranges.push((lo, hi, acc));
                lo = hi;
                acc = 0;
            }
        }
    }
    if lo < form_count && acc > 0 {
        ranges.push((lo, form_count, acc));
    } else if let Some(last) = ranges.last_mut() {
        last.1 = form_count;
    }
    ranges
}

/// Partition `start..end` into up to `parts` contiguous ranges `(lo, hi, nodes)`
/// balanced by `form_nodes`.
fn partition_node_ranges(
    form_nodes: &[u32],
    start: usize,
    end: usize,
    total_nodes: usize,
    parts: usize,
) -> Vec<(usize, usize, usize)> {
    if start >= end {
        return Vec::new();
    }
    if parts <= 1 {
        return vec![(start, end, total_nodes)];
    }
    let target = (total_nodes / parts).max(1);
    let mut ranges = Vec::with_capacity(parts);
    let mut lo = start;
    let mut acc = 0usize;
    for (pos, &nodes) in form_nodes[start..end].iter().enumerate() {
        let abs = start + pos;
        acc += nodes as usize;
        if acc >= target && ranges.len() + 1 < parts && abs + 1 < end {
            ranges.push((lo, abs + 1, acc));
            lo = abs + 1;
            acc = 0;
        }
    }
    if lo < end {
        ranges.push((lo, end, acc));
    }
    ranges
}

struct SliceWaveOutput {
    lo: usize,
    hi: usize,
    deltas: Vec<(u32, LatticeValue, u32)>,
    local_deltas: Vec<(LocalId, LatticeValue)>,
    new_deps: Vec<(u32, u32)>,
    self_dirty: Vec<u32>,
}

trait WalkEnv<'p: 'c, 'c> {
    const COUNT_USES: bool = false;
    const FINAL_PASS: bool = false;

    fn ctx(&self) -> &'c StaticCtx<'p>;
    fn position(&self) -> usize;
    fn narrowed(&self) -> &[(LocalId, LatticeValue)];
    fn narrowed_mut(&mut self) -> &mut Vec<(LocalId, LatticeValue)>;

    #[inline]
    fn on_enter_expr(&mut self) {}
    #[inline]
    fn on_record(&mut self, _id: ExprId, value: LatticeValue) -> LatticeValue {
        value
    }
    #[inline]
    fn on_local_read_count(&mut self, _local: LocalId) {}
    #[inline]
    fn on_declare_local(&mut self, _local: LocalId) {}

    fn read_local_raw(&mut self, local: LocalId) -> LatticeValue;
    fn bind_local(&mut self, local: LocalId, value: LatticeValue);

    fn read_def(&mut self, global: GlobalId) -> LatticeValue;
    fn join_def(&mut self, global: GlobalId, value: LatticeValue);

    fn read_fn_return(&mut self, slot: u32) -> LatticeValue;
    fn join_fn_return(&mut self, slot: u32, value: LatticeValue);

    fn read_atom(&mut self, atom: usize) -> LatticeValue;
    fn join_atom(&mut self, atom: usize, value: LatticeValue);
}

struct Round1Env<'p, 'c> {
    ctx: &'c StaticCtx<'p>,
    state: &'c mut SolverState,
    uses: &'c mut [u32],
    narrowed: Vec<(LocalId, LatticeValue)>,
    position: usize,
    changed: bool,
    dep_seen_stamp: Vec<u32>,
    last_changed_pos: Vec<u32>,
    dep_edges: Vec<(u32, u32)>,
    local_read_stamp: Vec<u32>,
    local_owner: Vec<u32>,
    local_in_form: Vec<u32>,
    form_locals_flat: Vec<LocalId>,
    form_locals_offsets: Vec<u32>,
    cur_form_locals: usize,
    cur_form_nodes: u32,
    form_nodes: Vec<u32>,
    form_local_changed: bool,
    self_dirty: Vec<u32>,
}

impl<'p, 'c> Round1Env<'p, 'c> {
    #[inline]
    fn record_dep(&mut self, dep: usize) {
        let pos = self.position as u32;
        if self.dep_seen_stamp[dep] != pos {
            self.dep_seen_stamp[dep] = pos;
            self.dep_edges.push((dep as u32, pos));
        }
    }

    #[inline]
    fn mark_dep_changed(&mut self, dep: usize) {
        self.last_changed_pos[dep] = self.position as u32;
        self.changed = true;
    }
}

impl<'p, 'c> WalkEnv<'p, 'c> for Round1Env<'p, 'c> {
    const COUNT_USES: bool = true;

    #[inline]
    fn ctx(&self) -> &'c StaticCtx<'p> {
        self.ctx
    }

    #[inline]
    fn position(&self) -> usize {
        self.position
    }

    #[inline]
    fn narrowed(&self) -> &[(LocalId, LatticeValue)] {
        &self.narrowed
    }

    #[inline]
    fn narrowed_mut(&mut self) -> &mut Vec<(LocalId, LatticeValue)> {
        &mut self.narrowed
    }

    #[inline]
    fn on_enter_expr(&mut self) {
        self.cur_form_nodes = self.cur_form_nodes.saturating_add(1);
    }

    #[inline]
    fn on_local_read_count(&mut self, local: LocalId) {
        self.uses[local.0] += 1;
    }

    #[inline]
    fn on_declare_local(&mut self, local: LocalId) {
        if self.ctx.pinned[local.0] || self.ctx.param_slot[local.0] != u32::MAX {
            return;
        }
        let pos = self.position as u32;
        let owner = &mut self.local_owner[local.0];
        if *owner == LOCAL_UNASSIGNED {
            *owner = pos;
            self.local_in_form[local.0] =
                u32::try_from(self.cur_form_locals).expect("form locals fit in u32");
            self.form_locals_flat.push(local);
            self.cur_form_locals += 1;
        } else if *owner != pos {
            *owner = LOCAL_MULTI_FORM;
        }
    }

    #[inline]
    fn read_local_raw(&mut self, local: LocalId) -> LatticeValue {
        let param = self.ctx.param_slot[local.0];
        if param != u32::MAX {
            self.record_dep(self.ctx.layout.param_dep(param));
        } else {
            self.local_read_stamp[local.0] = self.position as u32;
        }
        self.state.locals[local.0]
    }

    #[inline]
    fn bind_local(&mut self, local: LocalId, value: LatticeValue) {
        if self.ctx.pinned[local.0] {
            return;
        }
        let param = self.ctx.param_slot[local.0];
        if param == u32::MAX {
            self.on_declare_local(local);
        }
        let slot = &mut self.state.locals[local.0];
        let joined = slot.union(value);
        if joined != *slot {
            *slot = joined;
            if param != u32::MAX {
                self.mark_dep_changed(self.ctx.layout.param_dep(param));
            } else {
                self.changed = true;
                if self.local_read_stamp[local.0] == self.position as u32 {
                    self.form_local_changed = true;
                }
            }
        }
    }

    #[inline]
    fn read_def(&mut self, global: GlobalId) -> LatticeValue {
        self.record_dep(self.ctx.layout.def_dep(global));
        self.state.def_values[global.0]
    }

    #[inline]
    fn join_def(&mut self, global: GlobalId, value: LatticeValue) {
        let slot = &mut self.state.def_values[global.0];
        let joined = slot.union(value);
        if joined != *slot {
            *slot = joined;
            self.mark_dep_changed(self.ctx.layout.def_dep(global));
        }
    }

    #[inline]
    fn read_fn_return(&mut self, slot: u32) -> LatticeValue {
        self.record_dep(self.ctx.layout.fn_dep(slot));
        self.state.fn_returns[slot as usize]
    }

    #[inline]
    fn join_fn_return(&mut self, slot: u32, value: LatticeValue) {
        let entry = &mut self.state.fn_returns[slot as usize];
        let joined = entry.union(value);
        if joined != *entry {
            *entry = joined;
            self.mark_dep_changed(self.ctx.layout.fn_dep(slot));
        }
    }

    #[inline]
    fn read_atom(&mut self, atom: usize) -> LatticeValue {
        self.record_dep(self.ctx.layout.atom_dep(atom));
        self.state.contents[atom]
    }

    #[inline]
    fn join_atom(&mut self, atom: usize, value: LatticeValue) {
        let entry = &mut self.state.contents[atom];
        let joined = entry.union(value);
        if joined != *entry {
            *entry = joined;
            self.mark_dep_changed(self.ctx.layout.atom_dep(atom));
        }
    }
}

struct SerialDirtyEnv<'p, 'c> {
    ctx: &'c StaticCtx<'p>,
    state: &'c mut SolverState,
    subs: &'c mut SubscriberGraph,
    cur_dirty: &'c mut BitSet,
    next_dirty: &'c mut BitSet,
    narrowed: &'c mut Vec<(LocalId, LatticeValue)>,
    local_read_stamp: &'c mut [u32],
    position: usize,
    reverse: bool,
    changed: bool,
    form_local_changed: bool,
}

impl<'p, 'c> SerialDirtyEnv<'p, 'c> {
    #[inline]
    fn notify_dep(&mut self, dep: usize) {
        self.changed = true;
        let pos = self.position;
        if self.reverse {
            self.subs.for_each(dep, |sub| {
                if sub < pos {
                    self.cur_dirty.set(sub);
                } else {
                    self.next_dirty.set(sub);
                }
            });
        } else {
            self.subs.for_each(dep, |sub| {
                if sub > pos {
                    self.cur_dirty.set(sub);
                } else {
                    self.next_dirty.set(sub);
                }
            });
        }
    }

    #[inline]
    fn ensure_dynamic_dep(&mut self, dep: usize) {
        let form = self.position as u32;
        if !self.subs.contains(dep, form) {
            self.subs.insert_extra(dep as u32, form);
        }
    }
}

impl<'p, 'c> WalkEnv<'p, 'c> for SerialDirtyEnv<'p, 'c> {
    #[inline]
    fn ctx(&self) -> &'c StaticCtx<'p> {
        self.ctx
    }

    #[inline]
    fn position(&self) -> usize {
        self.position
    }

    #[inline]
    fn narrowed(&self) -> &[(LocalId, LatticeValue)] {
        self.narrowed
    }

    #[inline]
    fn narrowed_mut(&mut self) -> &mut Vec<(LocalId, LatticeValue)> {
        self.narrowed
    }

    #[inline]
    fn read_local_raw(&mut self, local: LocalId) -> LatticeValue {
        if self.ctx.param_slot[local.0] == u32::MAX {
            self.local_read_stamp[local.0] = self.position as u32;
        }
        self.state.locals[local.0]
    }

    #[inline]
    fn bind_local(&mut self, local: LocalId, value: LatticeValue) {
        if self.ctx.pinned[local.0] {
            return;
        }
        let slot = &mut self.state.locals[local.0];
        let joined = slot.union(value);
        if joined != *slot {
            *slot = joined;
            let param = self.ctx.param_slot[local.0];
            if param != u32::MAX {
                self.notify_dep(self.ctx.layout.param_dep(param));
            } else {
                self.changed = true;
                if self.local_read_stamp[local.0] == self.position as u32 {
                    self.form_local_changed = true;
                }
            }
        }
    }

    #[inline]
    fn read_def(&mut self, global: GlobalId) -> LatticeValue {
        self.state.def_values[global.0]
    }

    #[inline]
    fn join_def(&mut self, global: GlobalId, value: LatticeValue) {
        let slot = &mut self.state.def_values[global.0];
        let joined = slot.union(value);
        if joined != *slot {
            *slot = joined;
            self.notify_dep(self.ctx.layout.def_dep(global));
        }
    }

    #[inline]
    fn read_fn_return(&mut self, slot: u32) -> LatticeValue {
        self.ensure_dynamic_dep(self.ctx.layout.fn_dep(slot));
        self.state.fn_returns[slot as usize]
    }

    #[inline]
    fn join_fn_return(&mut self, slot: u32, value: LatticeValue) {
        let entry = &mut self.state.fn_returns[slot as usize];
        let joined = entry.union(value);
        if joined != *entry {
            *entry = joined;
            self.notify_dep(self.ctx.layout.fn_dep(slot));
        }
    }

    #[inline]
    fn read_atom(&mut self, atom: usize) -> LatticeValue {
        self.ensure_dynamic_dep(self.ctx.layout.atom_dep(atom));
        self.state.contents[atom]
    }

    #[inline]
    fn join_atom(&mut self, atom: usize, value: LatticeValue) {
        let entry = &mut self.state.contents[atom];
        let joined = entry.union(value);
        if joined != *entry {
            *entry = joined;
            self.notify_dep(self.ctx.layout.atom_dep(atom));
        }
    }
}

struct ParallelSliceEnv<'p, 'c> {
    ctx: &'c StaticCtx<'p>,
    snapshot: &'c SolverState,
    subs: &'c SubscriberGraph,
    meta: &'c FormMeta,
    lo: usize,
    hi: usize,
    reverse: bool,
    position: usize,
    narrowed: Vec<(LocalId, LatticeValue)>,
    slice_dirty: BitSet,
    touched: BitSet,
    dep_deltas: HashMap<u32, (LatticeValue, u32)>,
    cur_locals: Vec<LatticeValue>,
    cur_local_read: Vec<bool>,
    local_deltas: Vec<(LocalId, LatticeValue)>,
    extra_local_deltas: HashMap<LocalId, LatticeValue>,
    new_deps: Vec<(u32, u32)>,
    self_dirty: Vec<u32>,
    form_local_changed: bool,
}

impl<'p, 'c> ParallelSliceEnv<'p, 'c> {
    fn walk_form(&mut self, pos: usize) {
        self.position = pos;
        self.form_local_changed = false;
        let start = self.meta.form_locals_offsets[pos] as usize;
        let end = self.meta.form_locals_offsets[pos + 1] as usize;
        self.cur_locals.clear();
        self.cur_local_read.clear();
        for &local in &self.meta.form_locals_flat[start..end] {
            self.cur_locals.push(self.snapshot.locals[local.0]);
            self.cur_local_read.push(false);
        }
        let form_id = self.ctx.forms[pos];
        walk_expr(self, form_id);
        for (idx, &local) in self.meta.form_locals_flat[start..end].iter().enumerate() {
            let val = self.cur_locals[idx];
            if val != self.snapshot.locals[local.0] {
                self.local_deltas.push((local, val));
            }
        }
        if self.form_local_changed {
            self.self_dirty.push(pos as u32);
        }
    }

    #[inline]
    fn read_summary(&self, dep: usize) -> LatticeValue {
        if self.touched.get(dep)
            && let Some(&(val, _)) = self.dep_deltas.get(&(dep as u32))
        {
            return val;
        }
        self.snapshot.read_dep(self.ctx, dep)
    }

    #[inline]
    fn write_summary(&mut self, dep: usize, value: LatticeValue) {
        let cur = self.read_summary(dep);
        let joined = cur.union(value);
        if joined == cur {
            return;
        }
        self.touched.set(dep);
        self.dep_deltas
            .insert(dep as u32, (joined, self.position as u32));
        // Propagate immediately to subscribers strictly ahead within `[lo..hi)`.
        let (range_lo, range_hi) = if self.reverse {
            (self.lo, self.position)
        } else {
            (self.position + 1, self.hi)
        };
        let lo = self.lo;
        self.subs.for_each_in_range(dep, range_lo, range_hi, |sub| {
            self.slice_dirty.set(sub - lo);
        });
    }

    #[inline]
    fn record_dynamic_dep(&mut self, dep: usize) {
        let form = self.position as u32;
        if !self.subs.contains(dep, form) {
            self.new_deps.push((dep as u32, form));
        }
    }
}

impl<'p, 'c> WalkEnv<'p, 'c> for ParallelSliceEnv<'p, 'c> {
    #[inline]
    fn ctx(&self) -> &'c StaticCtx<'p> {
        self.ctx
    }

    #[inline]
    fn position(&self) -> usize {
        self.position
    }

    #[inline]
    fn narrowed(&self) -> &[(LocalId, LatticeValue)] {
        &self.narrowed
    }

    #[inline]
    fn narrowed_mut(&mut self) -> &mut Vec<(LocalId, LatticeValue)> {
        &mut self.narrowed
    }

    #[inline]
    fn read_local_raw(&mut self, local: LocalId) -> LatticeValue {
        let owner = self.meta.local_owner[local.0];
        if owner == self.position as u32 {
            let slot = self.meta.local_in_form[local.0] as usize;
            self.cur_local_read[slot] = true;
            return self.cur_locals[slot];
        }
        let param = self.ctx.param_slot[local.0];
        if param != u32::MAX {
            return self.read_summary(self.ctx.layout.param_dep(param));
        }
        if owner == LOCAL_MULTI_FORM
            && let Some(&val) = self.extra_local_deltas.get(&local)
        {
            return val;
        }
        self.snapshot.locals[local.0]
    }

    #[inline]
    fn bind_local(&mut self, local: LocalId, value: LatticeValue) {
        if self.ctx.pinned[local.0] {
            return;
        }
        let owner = self.meta.local_owner[local.0];
        if owner == self.position as u32 {
            let slot = self.meta.local_in_form[local.0] as usize;
            let joined = self.cur_locals[slot].union(value);
            if joined != self.cur_locals[slot] {
                self.cur_locals[slot] = joined;
                if self.cur_local_read[slot] {
                    self.form_local_changed = true;
                }
            }
            return;
        }
        let param = self.ctx.param_slot[local.0];
        if param != u32::MAX {
            self.write_summary(self.ctx.layout.param_dep(param), value);
            return;
        }
        if owner == LOCAL_MULTI_FORM {
            let cur = self
                .extra_local_deltas
                .get(&local)
                .copied()
                .unwrap_or(self.snapshot.locals[local.0]);
            let joined = cur.union(value);
            if joined != cur {
                self.extra_local_deltas.insert(local, joined);
                self.form_local_changed = true;
            }
        }
    }

    #[inline]
    fn read_def(&mut self, global: GlobalId) -> LatticeValue {
        self.read_summary(self.ctx.layout.def_dep(global))
    }

    #[inline]
    fn join_def(&mut self, global: GlobalId, value: LatticeValue) {
        self.write_summary(self.ctx.layout.def_dep(global), value);
    }

    #[inline]
    fn read_fn_return(&mut self, slot: u32) -> LatticeValue {
        let dep = self.ctx.layout.fn_dep(slot);
        self.record_dynamic_dep(dep);
        self.read_summary(dep)
    }

    #[inline]
    fn join_fn_return(&mut self, slot: u32, value: LatticeValue) {
        self.write_summary(self.ctx.layout.fn_dep(slot), value);
    }

    #[inline]
    fn read_atom(&mut self, atom: usize) -> LatticeValue {
        let dep = self.ctx.layout.atom_dep(atom);
        self.record_dynamic_dep(dep);
        self.read_summary(dep)
    }

    #[inline]
    fn join_atom(&mut self, atom: usize, value: LatticeValue) {
        self.write_summary(self.ctx.layout.atom_dep(atom), value);
    }
}

struct FinalDirectEnv<'p, 'c> {
    ctx: &'c StaticCtx<'p>,
    state: &'c SolverState,
    values: &'c mut [CompactInferred],
    narrowed: Vec<(LocalId, LatticeValue)>,
    position: usize,
}

impl<'p, 'c> WalkEnv<'p, 'c> for FinalDirectEnv<'p, 'c> {
    const FINAL_PASS: bool = true;

    #[inline]
    fn ctx(&self) -> &'c StaticCtx<'p> {
        self.ctx
    }

    #[inline]
    fn position(&self) -> usize {
        self.position
    }

    #[inline]
    fn narrowed(&self) -> &[(LocalId, LatticeValue)] {
        &self.narrowed
    }

    #[inline]
    fn narrowed_mut(&mut self) -> &mut Vec<(LocalId, LatticeValue)> {
        &mut self.narrowed
    }

    #[inline]
    fn on_record(&mut self, id: ExprId, value: LatticeValue) -> LatticeValue {
        record_value(self.values, id, value.to_compact());
        value
    }

    #[inline]
    fn read_local_raw(&mut self, local: LocalId) -> LatticeValue {
        self.state.locals[local.0]
    }

    #[inline]
    fn bind_local(&mut self, _local: LocalId, _value: LatticeValue) {}

    #[inline]
    fn read_def(&mut self, global: GlobalId) -> LatticeValue {
        self.state.def_values[global.0]
    }

    #[inline]
    fn join_def(&mut self, _global: GlobalId, _value: LatticeValue) {}

    #[inline]
    fn read_fn_return(&mut self, slot: u32) -> LatticeValue {
        self.state.fn_returns[slot as usize]
    }

    #[inline]
    fn join_fn_return(&mut self, _slot: u32, _value: LatticeValue) {}

    #[inline]
    fn read_atom(&mut self, atom: usize) -> LatticeValue {
        self.state.contents[atom]
    }

    #[inline]
    fn join_atom(&mut self, _atom: usize, _value: LatticeValue) {}
}

struct FinalSliceEnv<'p, 'c> {
    ctx: &'c StaticCtx<'p>,
    state: &'c SolverState,
    entries: Vec<(u32, CompactInferred)>,
    narrowed: Vec<(LocalId, LatticeValue)>,
    position: usize,
}

impl<'p, 'c> WalkEnv<'p, 'c> for FinalSliceEnv<'p, 'c> {
    const FINAL_PASS: bool = true;

    #[inline]
    fn ctx(&self) -> &'c StaticCtx<'p> {
        self.ctx
    }

    #[inline]
    fn position(&self) -> usize {
        self.position
    }

    #[inline]
    fn narrowed(&self) -> &[(LocalId, LatticeValue)] {
        &self.narrowed
    }

    #[inline]
    fn narrowed_mut(&mut self) -> &mut Vec<(LocalId, LatticeValue)> {
        &mut self.narrowed
    }

    #[inline]
    fn on_record(&mut self, id: ExprId, value: LatticeValue) -> LatticeValue {
        self.entries.push((id.0 as u32, value.to_compact()));
        value
    }

    #[inline]
    fn read_local_raw(&mut self, local: LocalId) -> LatticeValue {
        self.state.locals[local.0]
    }

    #[inline]
    fn bind_local(&mut self, _local: LocalId, _value: LatticeValue) {}

    #[inline]
    fn read_def(&mut self, global: GlobalId) -> LatticeValue {
        self.state.def_values[global.0]
    }

    #[inline]
    fn join_def(&mut self, _global: GlobalId, _value: LatticeValue) {}

    #[inline]
    fn read_fn_return(&mut self, slot: u32) -> LatticeValue {
        self.state.fn_returns[slot as usize]
    }

    #[inline]
    fn join_fn_return(&mut self, _slot: u32, _value: LatticeValue) {}

    #[inline]
    fn read_atom(&mut self, atom: usize) -> LatticeValue {
        self.state.contents[atom]
    }

    #[inline]
    fn join_atom(&mut self, _atom: usize, _value: LatticeValue) {}
}

#[inline]
fn read_local<'p: 'c, 'c, E: WalkEnv<'p, 'c>>(env: &mut E, local: LocalId) -> LatticeValue {
    if env.ctx().pinned[local.0] {
        return LatticeValue::ANY;
    }
    if let Some(&(_, value)) = env
        .narrowed()
        .iter()
        .rev()
        .find(|(narrowed, _)| *narrowed == local)
    {
        return value;
    }
    env.read_local_raw(local)
}

/// Walk `id` with `filters` applied to their locals.
fn walk_narrowed<'p: 'c, 'c, E: WalkEnv<'p, 'c>>(
    env: &mut E,
    id: ExprId,
    filters: &[(LocalId, Filter)],
) -> LatticeValue {
    if filters.is_empty() {
        return walk_expr(env, id);
    }
    let ctx = env.ctx();
    let depth = env.narrowed().len();
    for &(local, filter) in filters {
        if !ctx.pinned[local.0] {
            let current = read_local(env, local);
            let value = filter.apply(current);
            env.narrowed_mut().push((local, value));
        }
    }
    let value = walk_expr(env, id);
    env.narrowed_mut().truncate(depth);
    value
}

#[inline]
fn walk_discard<'p: 'c, 'c, E: WalkEnv<'p, 'c>>(env: &mut E, ids: &[ExprId]) {
    for &id in ids {
        walk_expr(env, id);
    }
}

#[inline]
fn walk_last<'p: 'c, 'c, E: WalkEnv<'p, 'c>>(env: &mut E, ids: &[ExprId]) -> LatticeValue {
    let mut last = LatticeValue::of(Type::NIL);
    for &id in ids {
        last = walk_expr(env, id);
    }
    last
}

#[inline]
fn walk_args<'p: 'c, 'c, E: WalkEnv<'p, 'c>>(env: &mut E, ids: &[ExprId]) -> ArgBuf {
    if ids.len() <= INLINE_ARGS {
        let mut buf = [LatticeValue::NEVER; INLINE_ARGS];
        for (slot, &id) in buf.iter_mut().zip(ids) {
            *slot = walk_expr(env, id);
        }
        ArgBuf::Inline(buf, ids.len())
    } else {
        let mut vec = Vec::with_capacity(ids.len());
        for &id in ids {
            vec.push(walk_expr(env, id));
        }
        ArgBuf::Heap(vec)
    }
}

/// Walk `ids` and join their values.
#[inline]
fn join_all<'p: 'c, 'c, E: WalkEnv<'p, 'c>>(env: &mut E, ids: &[ExprId]) -> LatticeValue {
    let mut acc = LatticeValue::NEVER;
    for &id in ids {
        acc = acc.union(walk_expr(env, id));
    }
    acc
}

fn walk_expr<'p: 'c, 'c, E: WalkEnv<'p, 'c>>(env: &mut E, id: ExprId) -> LatticeValue {
    use ExprKind as K;
    env.on_enter_expr();
    let ctx = env.ctx();
    let program = ctx.program;
    let value = match &program.exprs[id.0].kind {
        K::Nil => LatticeValue::of(Type::NIL),
        // The runtime's `Loadable` class, like a core type's (`AtomType`).
        K::LoadableType => LatticeValue::of(Type::FN),
        // `new Loadable(...)`, as `K::Construct` below.
        K::Loadable { class, .. } => {
            walk_expr(env, *class);
            match program.exprs[class.0].kind {
                K::Global(global) => instance(ctx, global, true).unwrap_or(LatticeValue::ANY),
                _ => LatticeValue::ANY,
            }
        }
        K::Bool(_) => LatticeValue::of(Type::BOOLEAN),
        K::Number(_) => LatticeValue::of(Type::NUMBER),
        K::String(_) => LatticeValue::of(Type::STRING),
        K::Keyword(_) => LatticeValue::of(Type::KEYWORD),
        K::Symbol(_) => LatticeValue::of(Type::SYMBOL),
        K::ReaderLiteral {
            kind,
            items,
            quoted,
        } => {
            walk_discard(env, items);
            match (kind, quoted) {
                (CompilerLiteralKind::JsArray, false) => LatticeValue::known_array(Type::HOST),
                (
                    CompilerLiteralKind::JsObject
                    | CompilerLiteralKind::Regex
                    | CompilerLiteralKind::Instant,
                    false,
                ) => LatticeValue::of(Type::HOST),
                (CompilerLiteralKind::Uuid | CompilerLiteralKind::Queue, false) => {
                    LatticeValue::of(Type::OBJECT)
                }
                _ => LatticeValue::ANY,
            }
        }
        K::DefinitionTest { value, .. } | K::NamespaceView { value, .. } => walk_expr(env, *value),
        K::WithMeta { value, metadata } => {
            let arguments = [walk_expr(env, *value).ty(), walk_expr(env, *metadata).ty()];
            core_call(ctx, CoreFnId::WithMeta, &arguments)
        }
        K::Var {
            value, metadata, ..
        } => {
            walk_expr(env, *value);
            if let Some(metadata) = metadata {
                walk_expr(env, *metadata);
            }
            LatticeValue::of(Type::OBJECT)
        }
        K::List(items) => {
            let element = join_all(env, items);
            LatticeValue::of(Type::LIST).with_elements(element)
        }
        K::Vector(items) => {
            let element = join_all(env, items);
            LatticeValue::known_vector(Type::VECTOR).with_elements(element)
        }
        K::Map(items) | K::LiteralMap(items) => {
            walk_discard(env, items);
            LatticeValue::of(Type::MAP)
        }
        K::Set(items) | K::LiteralSet(items) => {
            let element = join_all(env, items);
            LatticeValue::of(Type::SET).with_elements(element)
        }
        K::CoreFn(core) => {
            if ctx.facts.replaced(*core) {
                LatticeValue::ANY
            } else {
                LatticeValue::of(core.operation().value)
            }
        }
        K::Local(local) => {
            if E::COUNT_USES {
                env.on_local_read_count(*local);
            }
            read_local(env, *local)
        }
        K::Global(global) => read_global(env, *global),
        K::JsGlobal(_) => LatticeValue::ANY,
        K::JsTemplate { args, .. } => {
            walk_discard(env, args);
            LatticeValue::ANY
        }
        K::JsIndex { receiver, indices } => {
            walk_expr(env, *receiver);
            walk_discard(env, indices);
            LatticeValue::ANY
        }
        K::ThisAs { body, .. } => walk_expr(env, *body),
        K::JsArguments => LatticeValue::of(Type::HOST),
        K::JsDelete { receiver, key } => {
            walk_expr(env, *receiver);
            walk_expr(env, *key);
            LatticeValue::of(Type::BOOLEAN)
        }
        K::QualifiedMethod { class, .. } => {
            walk_expr(env, *class);
            LatticeValue::of(Type::FN)
        }
        K::CopyArguments(dest) => {
            walk_expr(env, *dest);
            LatticeValue::of(Type::NIL)
        }
        K::DefProtocol { .. } => LatticeValue::of(Type::NIL),
        K::ProtocolPredicate {
            protocol, value, ..
        } => {
            walk_expr(env, *protocol);
            walk_expr(env, *value);
            LatticeValue::of(Type::BOOLEAN)
        }
        K::ExtendType {
            target, methods, ..
        } => {
            walk_expr(env, *target);
            for (_, method) in methods {
                walk_expr(env, method.body);
            }
            LatticeValue::ANY
        }
        K::Reify {
            metadata, methods, ..
        } => {
            if let Some(metadata) = metadata {
                walk_expr(env, *metadata);
            }
            for (_, method) in methods {
                walk_expr(env, method.body);
            }
            LatticeValue::of(Type::OBJECT)
        }
        K::DefType {
            layout, methods, ..
        } => {
            if let Some(masks) = layout.masks {
                walk_expr(env, masks);
            }
            for (_, method) in methods {
                walk_expr(env, method.body);
            }
            match layout.body {
                Some(body) => walk_expr(env, body),
                None => LatticeValue::of(Type::FN),
            }
        }
        K::Construct { callee, args } => {
            walk_expr(env, *callee);
            walk_discard(env, args);
            match program.exprs[callee.0].kind {
                K::Global(global) => instance(ctx, global, true).unwrap_or(LatticeValue::ANY),
                _ => LatticeValue::ANY,
            }
        }
        K::Assert { test, message, .. } => {
            walk_expr(env, *test);
            if let Some(message) = message {
                walk_expr(env, *message);
            }
            LatticeValue::of(Type::NIL)
        }
        K::Def {
            global,
            init,
            test,
            once,
            ..
        } => {
            let value = match init {
                Some(init) => {
                    let init_val = walk_expr(env, *init);
                    if !E::FINAL_PASS
                        && matches!(ctx.facts.definition(*global), Definition::Def(def_id) if def_id == id)
                    {
                        env.join_def(*global, init_val);
                    }
                    init_val
                }
                None => LatticeValue::of(Type::NIL),
            };
            if let Some(test) = test {
                walk_expr(env, *test);
            }
            if *once {
                value.union(LatticeValue::of(Type::NIL))
            } else {
                value
            }
        }
        K::SetGlobal { target, value } => {
            walk_expr(env, *target);
            walk_expr(env, *value)
        }
        K::Binding { bindings, body } => {
            for &(target, init) in bindings {
                walk_expr(env, target);
                walk_expr(env, init);
            }
            walk_expr(env, *body)
        }
        K::WithOutStr(body) => {
            walk_expr(env, *body);
            LatticeValue::of(Type::STRING)
        }
        K::Exists(value) | K::BindingExists(value) => {
            walk_expr(env, *value);
            LatticeValue::of(Type::BOOLEAN)
        }
        K::BooleanHint(value) => {
            walk_expr(env, *value);
            LatticeValue::of(Type::BOOLEAN)
        }
        K::Property { receiver, .. } => {
            walk_expr(env, *receiver);
            LatticeValue::ANY
        }
        K::MethodCall { receiver, args, .. } => {
            walk_expr(env, *receiver);
            walk_discard(env, args);
            LatticeValue::ANY
        }
        K::If { test, yes, no } => {
            walk_expr(env, *test);
            let narrowing = narrowing(ctx, *test, 0);
            let yes = walk_narrowed(env, *yes, narrowing.yes.as_slice());
            let no = walk_narrowed(env, *no, narrowing.no.as_slice());
            if constant_truthy(&program.exprs[test.0].kind) {
                yes
            } else {
                yes.union(no)
            }
        }
        K::Do(items) => walk_last(env, items),
        K::Let { bindings, body } | K::Loop { bindings, body, .. } => {
            for &(local, init) in bindings {
                let value = walk_expr(env, init);
                if !E::FINAL_PASS {
                    env.bind_local(local, value);
                }
            }
            walk_expr(env, *body)
        }
        K::LetFn { bindings, body } => {
            if !E::FINAL_PASS {
                for &(local, _) in bindings {
                    env.on_declare_local(local);
                }
            }
            for &(local, init) in bindings {
                let value = walk_expr(env, init);
                if !E::FINAL_PASS {
                    env.bind_local(local, value);
                }
            }
            walk_expr(env, *body)
        }
        K::Recur { target, args } => {
            let params = &program.recur_targets[target.0].params;
            for (index, &arg) in args.iter().enumerate() {
                let value = walk_expr(env, arg);
                if !E::FINAL_PASS
                    && let Some(&param) = params.get(index)
                {
                    env.bind_local(param, value);
                }
            }
            LatticeValue::NEVER
        }
        K::Throw(value) => {
            walk_expr(env, *value);
            LatticeValue::NEVER
        }
        // What a promise settles to, or the value itself, is not known.
        K::Await(value) => {
            walk_expr(env, *value);
            LatticeValue::ANY
        }
        K::Try {
            body,
            catches,
            finally,
        } => {
            let mut value = walk_expr(env, *body);
            for catch in catches {
                if let Some(class) = catch.class {
                    walk_expr(env, class);
                }
                value = value.union(walk_expr(env, catch.body));
            }
            if let Some(finally) = finally {
                walk_expr(env, *finally);
            }
            value
        }
        K::Case {
            test,
            branches,
            default,
            ..
        } => {
            walk_expr(env, *test);
            let mut value = LatticeValue::NEVER;
            for &(constant, branch) in branches {
                walk_expr(env, constant);
                value = value.union(walk_expr(env, branch));
            }
            if let Some(default) = default {
                value = value.union(walk_expr(env, *default));
            }
            value
        }
        K::Fn { name, methods } => {
            let function = LatticeValue::known_function(id);
            if !E::FINAL_PASS
                && let Some(name) = name
            {
                env.bind_local(*name, function);
            }
            let mut ret = LatticeValue::NEVER;
            for method in methods {
                let body_val = walk_expr(env, method.body);
                if !E::FINAL_PASS {
                    ret = ret.union(if method.asynchronous {
                        LatticeValue::ANY
                    } else {
                        body_val
                    });
                }
            }
            if !E::FINAL_PASS {
                let slot = ctx.fn_slot[id.0];
                if slot != u32::MAX {
                    env.join_fn_return(slot, ret);
                }
            }
            if program.macro_functions.contains(&id) {
                LatticeValue::ANY
            } else {
                function
            }
        }
        K::Call { callee, args } => {
            let callee_value = walk_expr(env, *callee);
            let arguments = walk_args(env, args);
            let args_slice = arguments.as_slice();
            if !E::FINAL_PASS {
                if let Some((function, method)) = ctx.closed.call(id) {
                    pass(env, function, method, args_slice);
                }
                dispatch(env, id, args_slice);
                if let K::CoreFn(core) = program.exprs[callee.0].kind {
                    for (index, function) in ctx.closed.callbacks(id) {
                        pass_callback(env, core, args, args_slice, index, function);
                    }
                    store(env, id, core, args, args_slice);
                }
            }
            call(env, *callee, callee_value, args, args_slice)
        }
        K::InstanceOf { class, value } => {
            walk_expr(env, *class);
            walk_expr(env, *value);
            LatticeValue::of(Type::BOOLEAN)
        }
    };
    env.on_record(id, value)
}

/// Join what the call `call` of `core` with argument expressions `args` of
/// values `arguments` stores in a closed atom (light-cljs-252).
fn store<'p: 'c, 'c, E: WalkEnv<'p, 'c>>(
    env: &mut E,
    call_id: ExprId,
    core: CoreFnId,
    args: &[ExprId],
    arguments: &[LatticeValue],
) {
    let ctx = env.ctx();
    let target = if core == CoreFnId::Atom {
        call_id
    } else {
        match args.first() {
            Some(&target) => target,
            None => return,
        }
    };
    let Some(atom) = ctx.closed.atom(target) else {
        return;
    };
    let argument = |index: usize| arguments.get(index).copied().unwrap_or(LatticeValue::ANY);
    let stored = match core {
        _ if !ctx.facts.trusted(core, Calls::Invoke) => LatticeValue::ANY,
        CoreFnId::Atom => argument(0),
        CoreFnId::Reset | CoreFnId::ResetVals => argument(1),
        CoreFnId::CompareAndSet => argument(2),
        CoreFnId::Swap | CoreFnId::SwapVals => callback_returns(env, core, 1, args, arguments),
        _ => return,
    };
    env.join_atom(atom, stored);
}

/// What the closed atom read by `arg` holds (`sN`), else `any`.
#[inline]
fn state<'p: 'c, 'c, E: WalkEnv<'p, 'c>>(env: &mut E, arg: Option<&ExprId>) -> LatticeValue {
    match arg.and_then(|arg| env.ctx().closed.atom(*arg)) {
        Some(atom) => env.read_atom(atom),
        None => LatticeValue::ANY,
    }
}

/// What a call of `core` returns when its first argument reads a closed atom
/// (light-cljs-256).
fn atom_result<'p: 'c, 'c, E: WalkEnv<'p, 'c>>(
    env: &mut E,
    core: CoreFnId,
    args: &[ExprId],
    arguments: &[LatticeValue],
) -> Option<LatticeValue> {
    let ctx = env.ctx();
    let atom = ctx.closed.atom(*args.first()?)?;
    if !ctx.facts.trusted(core, Calls::Invoke) || ctx.facts.replaced(CoreFnId::Atom) {
        return None;
    }
    match core {
        CoreFnId::Deref => Some(env.read_atom(atom)),
        CoreFnId::Reset => arguments.get(1).copied(),
        CoreFnId::Swap => Some(callback_returns(env, core, 1, args, arguments)),
        _ => None,
    }
}

/// Bind the parameters of method `method` of the closed `fn` `function` to
/// `arguments`.
fn pass<'p: 'c, 'c, E: WalkEnv<'p, 'c>>(
    env: &mut E,
    function: ExprId,
    method: usize,
    arguments: &[LatticeValue],
) {
    let ExprKind::Fn { methods, .. } = &env.ctx().program.exprs[function.0].kind else {
        return;
    };
    let Some(method) = methods.get(method) else {
        return;
    };
    for (index, &param) in method.params.iter().enumerate() {
        let argument = arguments
            .get(index)
            .copied()
            .unwrap_or(LatticeValue::of(Type::NIL));
        env.bind_local(param, argument);
    }
    if let Some(rest) = method.rest {
        let rest_type = if arguments.len() > method.params.len() {
            Type::SEQ
        } else {
            Type::NIL
        };
        env.bind_local(rest, LatticeValue::of(rest_type));
    }
}

/// Bind the parameters of each implementation that the protocol method call
/// `call` with `arguments` may run (light-cljs-253).
fn dispatch<'p: 'c, 'c, E: WalkEnv<'p, 'c>>(
    env: &mut E,
    call_id: ExprId,
    arguments: &[LatticeValue],
) {
    let ctx = env.ctx();
    let receiver = arguments
        .first()
        .copied()
        .unwrap_or(LatticeValue::of(Type::NIL));
    let inherits = ctx.closed.inherits();
    for (index, implementation) in ctx.closed.dispatch(call_id) {
        if !ctx.receivers[index].is_some_and(|receivers| receivers.reach(receiver, inherits)) {
            continue;
        }
        let Some(method) = implementation.method(ctx.program) else {
            continue;
        };
        for (position, &param) in method.params.iter().enumerate() {
            let argument = arguments
                .get(position)
                .copied()
                .unwrap_or(LatticeValue::of(Type::NIL));
            env.bind_local(param, argument);
        }
    }
}

/// A read of `global`.
fn read_global<'p: 'c, 'c, E: WalkEnv<'p, 'c>>(env: &mut E, global: GlobalId) -> LatticeValue {
    let ctx = env.ctx();
    let program = ctx.program;
    if program.globals[global.0].name.contains('.') {
        return LatticeValue::ANY;
    }
    let (definition, value) = match ctx.facts.definition(global) {
        Definition::Unknown => return LatticeValue::ANY,
        Definition::Def(definition) => {
            let ExprKind::Def { init, once, .. } = program.exprs[definition.0].kind else {
                return LatticeValue::ANY;
            };
            let value = match init {
                Some(_) => env.read_def(global),
                None => LatticeValue::of(Type::NIL),
            };
            if once {
                return value.union(LatticeValue::of(Type::NIL));
            }
            (definition, value)
        }
        Definition::Type { definition, .. } => (definition, LatticeValue::of(Type::FN)),
        Definition::Protocol(definition) => {
            let ExprKind::DefProtocol {
                global: protocol, ..
            } = program.exprs[definition.0].kind
            else {
                return LatticeValue::ANY;
            };
            if protocol == global {
                return LatticeValue::ANY;
            }
            (definition, LatticeValue::of(Type::FN))
        }
    };
    // Before its definition runs, a global is undefined. A read in a later
    // top-level form than a top-level definition runs after it: an earlier
    // form that throws ends the program.
    let defined_before = ctx
        .facts
        .top_position(definition)
        .is_some_and(|position| position < env.position());
    if defined_before {
        value
    } else {
        value.union(LatticeValue::of(Type::NIL))
    }
}

/// What `new global` (`construct`) or a call of `global` returns when `global`
/// is a program type's constructor, or its positional or map factory.
#[inline]
fn instance(ctx: &StaticCtx<'_>, global: GlobalId, construct: bool) -> Option<LatticeValue> {
    let Definition::Type {
        type_global,
        role,
        record,
        ..
    } = ctx.facts.definition(global)
    else {
        return None;
    };
    if construct != (role == TypeRole::Constructor) {
        return None;
    }
    let class = if record { Type::RECORD } else { Type::OBJECT };
    Some(LatticeValue::known_instance(class, type_global))
}

#[inline]
fn core_call(ctx: &StaticCtx<'_>, core: CoreFnId, arguments: &[Type]) -> LatticeValue {
    let operation = core.operation();
    let call = operation.call(arguments);
    if !ctx.facts.trusted(core, call.calls) {
        return LatticeValue::ANY;
    }
    // Joining `returns` keeps the result monotone in the arguments: a call
    // outside the domain may also return what one inside does.
    let returns = call.returns.union(operation.returns_for(arguments.len()));
    match core {
        CoreFnId::Vector => LatticeValue::known_vector(returns),
        CoreFnId::Array => LatticeValue::known_array(returns),
        _ => LatticeValue::of(returns),
    }
}

#[inline]
fn described(ctx: &StaticCtx<'_>, core: CoreFnId, types: &[Type]) -> bool {
    let operation = core.operation();
    operation
        .arity(types.len())
        .is_some_and(|arity| arity.admits(types))
        && ctx.facts.trusted(core, operation.calls)
        && ctx.facts.core_protocols_intact()
}

fn core_result<'p: 'c, 'c, E: WalkEnv<'p, 'c>>(
    env: &mut E,
    core: CoreFnId,
    args: &[ExprId],
    arguments: &[LatticeValue],
) -> LatticeValue {
    if let Some(result) = atom_result(env, core, args, arguments) {
        return result;
    }
    let ctx = env.ctx();
    let types = TypeBuf::from_args(arguments);
    let types_slice = types.as_slice();
    let result = core_call(ctx, core, types_slice);
    match &core.operation().elements {
        Some(elements) if described(ctx, core, types_slice) => {
            let elem = describe(env, core, elements, args, arguments, true);
            result.with_elements(elem)
        }
        _ => result,
    }
}

fn describe<'p: 'c, 'c, E: WalkEnv<'p, 'c>>(
    env: &mut E,
    core: CoreFnId,
    descriptor: &Descriptor,
    args: &[ExprId],
    arguments: &[LatticeValue],
    returns: bool,
) -> LatticeValue {
    let argument = |index: usize| arguments.get(index).copied().unwrap_or(LatticeValue::ANY);
    let mut value = LatticeValue::NEVER;
    for &term in &descriptor.terms {
        let part = match term {
            Term::Type(ty) => LatticeValue::of(ty),
            Term::Argument(index) => argument(index),
            Term::Element(index) => argument(index).element(),
            Term::Arguments(index) => arguments
                .get(index..)
                .unwrap_or(&[])
                .iter()
                .copied()
                .fold(LatticeValue::NEVER, LatticeValue::union),
            Term::Elements(index) => arguments
                .get(index..)
                .unwrap_or(&[])
                .iter()
                .copied()
                .fold(LatticeValue::NEVER, |acc, arg| acc.union(arg.element())),
            Term::State(index) => state(env, args.get(index)),
            Term::Returns(index) if returns => callback_returns(env, core, index, args, arguments),
            Term::Returns(_) => LatticeValue::ANY,
        };
        value = value.union(part);
    }
    value
}

fn callback_arguments<'p: 'c, 'c, E: WalkEnv<'p, 'c>>(
    env: &mut E,
    core: CoreFnId,
    call: &[Descriptor],
    args: &[ExprId],
    arguments: &[LatticeValue],
    returns: bool,
) -> ArgBuf {
    let mut values = ArgBuf::Inline([LatticeValue::NEVER; INLINE_ARGS], 0);
    for descriptor in call {
        match descriptor.spread() {
            Some(Term::Arguments(from)) => {
                for &arg in arguments.iter().skip(from) {
                    values.push(arg);
                }
            }
            Some(Term::Elements(from)) => {
                for arg in arguments.iter().skip(from) {
                    values.push(arg.element());
                }
            }
            _ => values.push(describe(env, core, descriptor, args, arguments, returns)),
        }
    }
    values
}

fn callback_returns<'p: 'c, 'c, E: WalkEnv<'p, 'c>>(
    env: &mut E,
    core: CoreFnId,
    index: usize,
    args: &[ExprId],
    arguments: &[LatticeValue],
) -> LatticeValue {
    let callback = core
        .operation()
        .arity(arguments.len())
        .and_then(|arity| arity.callback(index));
    let (Some(callback), Some(&callee), Some(&value)) =
        (callback, args.get(index), arguments.get(index))
    else {
        return LatticeValue::ANY;
    };
    let mut result = LatticeValue::NEVER;
    for call_desc in &callback.calls {
        let call_arguments = callback_arguments(env, core, call_desc, args, arguments, false);
        result = result.union(call(env, callee, value, &[], call_arguments.as_slice()));
    }
    result
}

fn pass_callback<'p: 'c, 'c, E: WalkEnv<'p, 'c>>(
    env: &mut E,
    core: CoreFnId,
    args: &[ExprId],
    arguments: &[LatticeValue],
    index: usize,
    function: ExprId,
) {
    let ctx = env.ctx();
    let types = TypeBuf::from_args(arguments);
    let types_slice = types.as_slice();
    let program_types = Type::RECORD.union(Type::OBJECT);
    let runtime_atom = |position: usize| {
        !ctx.facts.replaced(CoreFnId::Atom)
            && args
                .get(position)
                .is_some_and(|arg| ctx.closed.atom(*arg).is_some())
    };
    let reachable = ctx.facts.receives_functions()
        && types_slice.iter().enumerate().any(|(position, ty)| {
            position != index
                && !ty.intersection(program_types).is_never()
                && !runtime_atom(position)
        });
    let callback = core
        .operation()
        .arity(arguments.len())
        .and_then(|arity| arity.callback(index))
        .filter(|_| described(ctx, core, types_slice) && !reachable);
    let ExprKind::Fn { methods, .. } = &ctx.program.exprs[function.0].kind else {
        return;
    };
    let Some(callback) = callback else {
        for method in methods {
            for &param in method.params.iter().chain(&method.rest) {
                env.bind_local(param, LatticeValue::ANY);
            }
        }
        return;
    };
    for call_desc in &callback.calls {
        let call_arguments = callback_arguments(env, core, call_desc, args, arguments, true);
        let call_slice = call_arguments.as_slice();
        if let Some(method) = crate::call_shapes::dispatched_method(methods, call_slice.len()) {
            pass(env, function, method, call_slice);
        }
    }
}

fn call<'p: 'c, 'c, E: WalkEnv<'p, 'c>>(
    env: &mut E,
    callee: ExprId,
    value: LatticeValue,
    args: &[ExprId],
    arguments: &[LatticeValue],
) -> LatticeValue {
    let ctx = env.ctx();
    let program = ctx.program;
    match &program.exprs[callee.0].kind {
        ExprKind::CoreFn(core) => return core_result(env, *core, args, arguments),
        ExprKind::Global(global) => {
            if let Some(instance) = instance(ctx, *global, false) {
                return instance;
            }
        }
        ExprKind::BooleanHint(inner) => {
            if let ExprKind::Global(global) = program.exprs[inner.0].kind
                && program.globals[global.0].boolean_tag
            {
                return LatticeValue::of(Type::BOOLEAN);
            }
        }
        _ => (),
    }
    // Calling nil throws. Staying `never` until the callee is known keeps
    // the fixed point from starting at `any` for recursive functions.
    let callable = value.ty().difference(Type::NIL);
    if callable.is_never() {
        return LatticeValue::NEVER;
    }
    if value.value.fn_id != NO_ID && callable == Type::FN {
        let slot = ctx.fn_slot[value.value.fn_id as usize];
        if slot != u32::MAX {
            return env.read_fn_return(slot);
        }
    }
    LatticeValue::ANY
}

/// What `test` tells about locals in the branches of an `if`.
fn narrowing(ctx: &StaticCtx<'_>, test: ExprId, depth: usize) -> Narrowing {
    use ExprKind as K;
    let program = ctx.program;
    if depth > 8 {
        return Narrowing::default();
    }
    match &program.exprs[test.0].kind {
        K::Local(local) if !ctx.pinned[local.0] => {
            let init = ctx.initializers[local.0];
            let mut result = if init != u32::MAX {
                narrowing(ctx, ExprId(init as usize), depth + 1)
            } else {
                Narrowing::default()
            };
            result.yes.push((*local, Filter::Remove(Type::NIL)));
            result
                .no
                .push((*local, Filter::Keep(Type::NIL.union(Type::BOOLEAN))));
            result
        }
        K::BooleanHint(inner)
        | K::DefinitionTest { value: inner, .. }
        | K::NamespaceView { value: inner, .. } => narrowing(ctx, *inner, depth + 1),
        K::Do(items) if items.len() == 1 => narrowing(ctx, items[0], depth + 1),
        K::Call { callee, args } => {
            let K::CoreFn(core) = program.exprs[callee.0].kind else {
                return Narrowing::default();
            };
            if ctx.facts.replaced(core) {
                return Narrowing::default();
            }
            if core == CoreFnId::Not && args.len() == 1 {
                return narrowing(ctx, args[0], depth + 1).swap();
            }
            let [arg] = args.as_slice() else {
                return Narrowing::default();
            };
            let K::Local(local) = program.exprs[arg.0].kind else {
                return Narrowing::default();
            };
            let user = Type::RECORD.union(Type::OBJECT);
            let intact = ctx.facts.core_protocols_intact();
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
                yes: FilterBuf::one((local, yes)),
                no: no.map_or_else(FilterBuf::default, |filter| FilterBuf::one((local, filter))),
            }
        }
        K::InstanceOf { class, value } => {
            let (K::Global(global), K::Local(local)) =
                (&program.exprs[class.0].kind, &program.exprs[value.0].kind)
            else {
                return Narrowing::default();
            };
            match ctx.facts.program_type(*global) {
                Some((_, record)) => {
                    let class = if record { Type::RECORD } else { Type::OBJECT };
                    Narrowing {
                        yes: FilterBuf::one((*local, Filter::Instance(global.0 as u32, class))),
                        no: FilterBuf::default(),
                    }
                }
                None => Narrowing::default(),
            }
        }
        K::ProtocolPredicate { value, key, .. } => {
            let K::Local(local) = program.exprs[value.0].kind else {
                return Narrowing::default();
            };
            match ctx.facts.implementors(key) {
                Some(Implementors {
                    sole: Some(global),
                    classes,
                }) => Narrowing {
                    yes: FilterBuf::one((local, Filter::Instance(global.0 as u32, classes))),
                    no: FilterBuf::default(),
                },
                Some(Implementors { classes, .. }) => Narrowing {
                    yes: FilterBuf::one((local, Filter::Keep(classes))),
                    no: FilterBuf::default(),
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
                narrowing(ctx, init, depth + 1),
                narrowing(ctx, other, depth + 1),
            );
            if and {
                let mut yes = first.yes;
                yes.extend_from(&second.yes);
                Narrowing {
                    yes,
                    no: FilterBuf::default(),
                }
            } else {
                let mut no = first.no;
                no.extend_from(&second.no);
                Narrowing {
                    yes: FilterBuf::default(),
                    no,
                }
            }
        }
        _ => Narrowing::default(),
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
        crate::program_fragment::visit_children(&program.exprs[id.0].kind, |child| {
            pending.push(child);
        });
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

    #[test]
    fn compact_inferred_matches_public_inferred_lattice() {
        assert_eq!(std::mem::size_of::<CompactInferred>(), 16);
        assert_eq!(std::mem::size_of::<LatticeValue>(), 24);
        let pairs: Vec<(Inferred, LatticeValue)> = vec![
            (Inferred::NEVER, LatticeValue::NEVER),
            (Inferred::ANY, LatticeValue::ANY),
            (Inferred::of(Type::NIL), LatticeValue::of(Type::NIL)),
            (Inferred::of(Type::NUMBER), LatticeValue::of(Type::NUMBER)),
            (
                Inferred::known(Type::FN, Known::Function(ExprId(7))),
                LatticeValue::known_function(ExprId(7)),
            ),
            (
                Inferred::known(Type::FN, Known::Function(ExprId(8))),
                LatticeValue::known_function(ExprId(8)),
            ),
            (
                Inferred::known(Type::RECORD, Known::Instance(GlobalId(3))),
                LatticeValue::known_instance(Type::RECORD, GlobalId(3)),
            ),
            (
                Inferred::known(Type::VECTOR, Known::Vector)
                    .with_elements(Inferred::of(Type::NUMBER)),
                LatticeValue::known_vector(Type::VECTOR)
                    .with_elements(LatticeValue::of(Type::NUMBER)),
            ),
            (
                Inferred::known(Type::VECTOR, Known::Vector)
                    .with_elements(Inferred::known(Type::FN, Known::Function(ExprId(7)))),
                LatticeValue::known_vector(Type::VECTOR)
                    .with_elements(LatticeValue::known_function(ExprId(7))),
            ),
            (
                Inferred::known(Type::HOST, Known::Array),
                LatticeValue::known_array(Type::HOST),
            ),
            (Inferred::of(Type::HOST), LatticeValue::of(Type::HOST)),
            (Inferred::of(Type::MAP), LatticeValue::of(Type::MAP)),
            (Inferred::of(Type::STRING), LatticeValue::of(Type::STRING)),
        ];
        for (a_pub, a_lat) in &pairs {
            assert_eq!(a_lat.to_compact().to_inferred(), *a_pub);
            assert_eq!(a_lat.element().to_compact().to_inferred(), a_pub.element());
            assert_eq!(a_lat.union(*a_lat), *a_lat);
            for mask in [
                Type::NIL,
                Type::FN,
                Type::VECTOR,
                Type::RECORD,
                Type::NUMBER,
                Type::HOST,
            ] {
                assert_eq!(
                    a_lat.keep(mask).to_compact().to_inferred(),
                    a_pub.keep(mask)
                );
                assert_eq!(
                    a_lat.remove(mask).to_compact().to_inferred(),
                    a_pub.remove(mask)
                );
            }
            for (b_pub, b_lat) in &pairs {
                assert_eq!(a_lat.union(*b_lat), b_lat.union(*a_lat));
                assert_eq!(
                    a_lat.union(*b_lat).to_compact().to_inferred(),
                    a_pub.union(*b_pub)
                );
                assert_eq!(
                    a_lat.with_elements(*b_lat).to_compact().to_inferred(),
                    a_pub.with_elements(*b_pub)
                );
                for (_, c_lat) in &pairs {
                    assert_eq!(
                        a_lat.union(*b_lat).union(*c_lat),
                        a_lat.union(b_lat.union(*c_lat)),
                        "LatticeValue::union must be associative"
                    );
                }
            }
        }
    }

    fn flatten_top_forms(program: &Program) -> Vec<ExprId> {
        let mut top = Vec::new();
        let mut pending = vec![program.root];
        while let Some(id) = pending.pop() {
            match &program.exprs[id.0].kind {
                ExprKind::Do(items) => pending.extend(items.iter().rev().copied()),
                _ => top.push(id),
            }
        }
        top
    }

    fn assert_inference_parity(program: &Program, serial: &Inference, parallel: &Inference) {
        assert_eq!(parallel.typed_parameters(), serial.typed_parameters());
        for expr in 0..program.exprs.len() {
            let id = ExprId(expr);
            assert_eq!(
                parallel.value(id),
                serial.value(id),
                "value mismatch for {id:?}"
            );
        }
        for local in 0..program.locals.len() {
            let id = LocalId(local);
            assert_eq!(
                parallel.local(id),
                serial.local(id),
                "local mismatch for {id:?}"
            );
            assert_eq!(
                parallel.uses(id),
                serial.uses(id),
                "uses mismatch for {id:?}"
            );
        }
    }

    #[test]
    fn bitset_forward_and_reverse_sweeps_cover_word_boundaries() {
        let mut bits = BitSet::new(200);
        assert!(bits.is_empty());
        assert_eq!(bits.pop_next(0), None);
        assert_eq!(bits.pop_prev(199), None);
        let expected = [0usize, 1, 63, 64, 65, 127, 128, 199];
        for &i in &expected {
            bits.set(i);
        }
        let mut forward = Vec::new();
        let mut cursor = 0;
        while let Some(pos) = bits.pop_next(cursor) {
            forward.push(pos);
            cursor = pos + 1;
        }
        assert_eq!(forward, expected);
        assert!(bits.is_empty());

        for &i in &expected {
            bits.set(i);
        }
        let mut reverse = Vec::new();
        let mut cursor = 199;
        while let Some(pos) = bits.pop_prev(cursor) {
            reverse.push(pos);
            if pos == 0 {
                break;
            }
            cursor = pos - 1;
        }
        let mut expected_rev = expected.to_vec();
        expected_rev.reverse();
        assert_eq!(reverse, expected_rev);
        assert!(bits.is_empty());
    }

    #[test]
    fn parallel_bsp_matches_serial_exactly_across_chains_atoms_protocols_and_loops() {
        // Build a program with >40 top-level forms exercising forward/backward
        // call chains, mutual recursion, closed atoms, closed protocol methods,
        // loops, narrowing, and callbacks, then compare serial vs forced
        // parallel BSP + parallel final pass on 4 and 8 slots.
        let mut source = String::new();
        source.push_str("(defprotocol P (pm [this v]))\n");
        source.push_str("(defrecord R [x] P (pm [this v] (+ x v)))\n");
        source.push_str("(def state (atom 0))\n");
        source.push_str("(defn f0 [x] (swap! state + x) (pm (->R x) x))\n");
        for i in 1..=36 {
            source.push_str(&format!(
                "(defn f{i} [x] (let [y (if (number? x) (inc x) 0)] (f{} y)))\n",
                i - 1
            ));
        }
        source.push_str(
            "(defn looped [n] (loop [i 0 acc []] (if (< i n) (recur (inc i) (conj acc i)) acc)))\n",
        );
        source.push_str("(defn mapped [xs] (mapv (fn [z] (+ z 1)) xs))\n");
        source.push_str("(defn caller [] (+ (f36 1) @state (count (mapped (looped 4)))))\n");
        source.push_str("(caller)\n");

        let forms = crate::parse(&source).expect("parse");
        let program = crate::analyze(&source, &forms).expect("analyze");
        let top = flatten_top_forms(&program);
        let closed = crate::call_shapes::closed_functions(&program, &[]);
        let serial = Inference::new(&program, ProgramFacts::new(&program, &top), &closed);
        assert!(serial.typed_parameters() > 30);

        for slot_count in [2usize, 4, 8] {
            let slots = crate::scheduler::Slots::new(slot_count).unwrap();
            let parallel = Inference::build(
                &program,
                ProgramFacts::new(&program, &top),
                &closed,
                slots,
                1,
                1,
            );
            assert_inference_parity(&program, &serial, &parallel);
        }
    }

    #[test]
    fn parallel_bsp_preserves_mixed_representation_summary() {
        // Mixing `Known::Function`, `Known::Array`, and `Known::Vector` into
        // the same closed parameter across a forced BSP wave must agree with
        // serial solving on both the raw parameter and narrowed branches.
        let mut source = String::new();
        source.push_str("(defn sink [v] (if (vector? v) (count v) 0) (if (array? v) v nil) nil)\n");
        source.push_str("(defn feed [a b] (sink a) (sink b))\n");
        for i in 0..16 {
            source.push_str(&format!("(defn pad{i} [x] (+ x {i}))\n"));
        }
        source.push_str("(sink (fn [] nil))\n");
        source.push_str("(feed #js [] [1])\n");
        for i in 0..16 {
            source.push_str(&format!("(pad{i} {i})\n"));
        }
        let forms = crate::parse(&source).expect("parse");
        let program = crate::analyze(&source, &forms).expect("analyze");
        let top = flatten_top_forms(&program);
        let closed = crate::call_shapes::closed_functions(&program, &[]);
        let facts = ProgramFacts::new(&program, &top);
        let serial = Inference::new(&program, facts, &closed);

        let slots = crate::scheduler::Slots::new(4).unwrap();
        let raw_facts = ProgramFacts::new(&program, &top);
        let mut solver = Solver::new(&program, &raw_facts, &closed);
        assert!(solver.solve(slots, 1).is_some());
        assert!(solver.bsp_waves > 0, "expected at least one BSP wave");

        let parallel = Inference::build(
            &program,
            ProgramFacts::new(&program, &top),
            &closed,
            slots,
            1,
            1,
        );
        assert_inference_parity(&program, &serial, &parallel);
        let v_local = program
            .locals
            .iter()
            .enumerate()
            .find_map(|(idx, binding)| (binding.name == "v").then_some(LocalId(idx)))
            .expect("local v");
        assert_eq!(
            parallel.local(v_local),
            Inferred::of(Type::FN.union(Type::HOST).union(Type::VECTOR))
                .with_elements(Inferred::of(Type::NUMBER))
        );
    }

    #[test]
    fn parallel_bsp_switches_to_serial_and_converges_on_deep_chains() {
        let mut source = String::from("(defn f0 [x] x)\n");
        for i in 1..120 {
            source.push_str(&format!("(defn f{i} [x] (f{} x))\n", i - 1));
        }
        source.push_str("(f119 \"ok\")\n");
        let forms = crate::parse(&source).expect("parse");
        let program = crate::analyze(&source, &forms).expect("analyze");
        let top = flatten_top_forms(&program);
        let closed = crate::call_shapes::closed_functions(&program, &[]);
        let serial = Inference::new(&program, ProgramFacts::new(&program, &top), &closed);
        assert_eq!(serial.typed_parameters(), 120);
        assert_eq!(serial.value(program.root), Inferred::of(Type::STRING));

        let slots = crate::scheduler::Slots::new(8).unwrap();
        let parallel = Inference::build(
            &program,
            ProgramFacts::new(&program, &top),
            &closed,
            slots,
            1,
            1,
        );
        assert_eq!(parallel.typed_parameters(), 120);
        assert_eq!(parallel.value(program.root), Inferred::of(Type::STRING));
    }

    #[test]
    fn parallel_bsp_retries_serially_before_dropping_closed_functions() {
        // A loop that shifts a number through 62 locals one step per walk in
        // an earlier slice, fed by a closed atom written from a later slice.
        // Serial reverse sweep wakes the loop in round 2 and converges at round
        // 64; a BSP wave in round 2 delays waking it to round 3 so the mixed
        // solve exhausts ROUND_LIMIT, and `Inference::build` must retry
        // serially with the original `ClosedFunctions` instead of dropping
        // parameter types.
        let mut source = String::from("(def state (atom nil))\n");
        let mut bindings = String::new();
        let mut recur_args = String::new();
        for i in (1..63).rev() {
            bindings.push_str(&format!("l{i} nil "));
            recur_args.push_str(&format!("l{} ", i - 1));
        }
        bindings.push_str("l0 @state");
        recur_args.push_str("l0");
        source.push_str(&format!(
            "(defn stepper [flag] (loop [{bindings}] (if flag (recur {recur_args}) (do l62 nil))))\n"
        ));
        for i in 0..16 {
            source.push_str(&format!("(defn pad{i} [x] (+ x {i}))\n"));
        }
        source.push_str("(defn writer [x] (reset! state x) nil)\n");
        source.push_str("(stepper true)\n");
        for i in 0..16 {
            source.push_str(&format!("(pad{i} {i})\n"));
        }
        source.push_str("(writer 1)\n");

        let forms = crate::parse(&source).expect("parse");
        let program = crate::analyze(&source, &forms).expect("analyze");
        let top = flatten_top_forms(&program);
        let closed = crate::call_shapes::closed_functions(&program, &[]);

        let serial = Inference::new(&program, ProgramFacts::new(&program, &top), &closed);
        assert!(serial.typed_parameters() >= 18);

        let slots = crate::scheduler::Slots::new(4).unwrap();
        let raw_facts = ProgramFacts::new(&program, &top);
        let mut raw_solver = Solver::new(&program, &raw_facts, &closed);
        let raw_result = raw_solver.solve(slots, 1);
        assert!(raw_solver.bsp_waves > 0, "expected a BSP wave to run");
        assert!(
            raw_result.is_none(),
            "expected BSP snapshot lag to exhaust ROUND_LIMIT before serial retry"
        );

        let parallel = Inference::build(
            &program,
            ProgramFacts::new(&program, &top),
            &closed,
            slots,
            1,
            1,
        );
        assert_inference_parity(&program, &serial, &parallel);
    }

    #[test]
    fn oversized_slot_budget_keeps_inference_inline() {
        let mut source = String::from("(defn f [x] (+ x 1))\n");
        for i in 0..8 {
            source.push_str(&format!("(defn g{i} [y] (f (+ y {i})))\n"));
            source.push_str(&format!("(g{i} {i})\n"));
        }
        let forms = crate::parse(&source).expect("parse");
        let program = crate::analyze(&source, &forms).expect("analyze");
        let top = flatten_top_forms(&program);
        let closed = crate::call_shapes::closed_functions(&program, &[]);

        let serial = Inference::new(&program, ProgramFacts::new(&program, &top), &closed);
        let huge = crate::scheduler::Slots::new(usize::MAX / 2 + 1).unwrap();
        let parallel = Inference::build(
            &program,
            ProgramFacts::new(&program, &top),
            &closed,
            huge,
            1,
            1,
        );
        assert_inference_parity(&program, &serial, &parallel);
    }

    #[test]
    fn parallel_bsp_wakes_new_function_return_subscriber() {
        // `invoke` does not know `reader` as its callee until round 2 (when
        // its parameter `f` learns `Known::Function(reader)` during a BSP
        // wave). At the same time, a later slice widens `@state` from `number`
        // to `number|string`. The newly discovered `fn_returns` edge for
        // `reader` must be registered before changed summaries mark
        // subscribers dirty so `invoke` and `observe` pick up `number|string`.
        let mut source = String::new();
        source.push_str("(def state (atom 1))\n");
        source.push_str("(defn reader [] @state)\n");
        source.push_str("(defn invoke [f] (f))\n");
        source.push_str("(defn observe [] (invoke reader))\n");
        for i in 0..16 {
            source.push_str(&format!("(defn pad{i} [x] (+ x {i}))\n"));
        }
        source.push_str("(defn late-write [x] (reset! state x) nil)\n");
        for i in 0..16 {
            source.push_str(&format!("(pad{i} {i})\n"));
        }
        source.push_str("(late-write \"s\")\n");
        source.push_str("(observe)\n");

        let forms = crate::parse(&source).expect("parse");
        let program = crate::analyze(&source, &forms).expect("analyze");
        let top = flatten_top_forms(&program);
        let closed = crate::call_shapes::closed_functions(&program, &[]);
        let serial = Inference::new(&program, ProgramFacts::new(&program, &top), &closed);

        let slots = crate::scheduler::Slots::new(4).unwrap();
        let raw_facts = ProgramFacts::new(&program, &top);
        let mut raw_solver = Solver::new(&program, &raw_facts, &closed);
        assert!(raw_solver.solve(slots, 1).is_some());
        assert!(raw_solver.bsp_waves > 0, "expected a BSP wave to run");

        let parallel = Inference::build(
            &program,
            ProgramFacts::new(&program, &top),
            &closed,
            slots,
            1,
            1,
        );
        assert_inference_parity(&program, &serial, &parallel);
        assert_eq!(parallel.ty(program.root), Type::NUMBER.union(Type::STRING));
    }
}
