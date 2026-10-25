//! Function objects, lexical closures and binding cells: the heap kinds a
//! `Call` invokes and a closure's environment is made of.
//!
//! A function object is a heap value (`HeapObject::Function`) holding an `Arc`
//! of shared, immutable function [`Code`] and the values it captured when it
//! was created. Creating one (`Function`, `CurrentFunction`) never copies the
//! code, and every task that calls it runs that one copy in its own
//! task-owned frames (`crate::vm::frames`).
//!
//! Identity is per object: each `Function` evaluation allocates a new object,
//! as each evaluation of a JavaScript function expression does, and `===`
//! compares objects, not code.
//!
//! # Lexical environments (clj-quickjs-20)
//!
//! A closure's lexical environment is the array of values it captured,
//! fixed when `Function` creates it (the opcodes are in [`crate::vm::instr`]).
//! The code declares how many it carries ([`Code::captures`]); `GetCapture i`
//! reads entry `i` of the running frame's function object. Each entry is
//! either
//!
//! * **an immutable value**, captured by value: correct for a binding that is
//!   initialized before the closure is created and never assigned afterwards
//!   (a `const`, or a parameter or `let` the compiler proves is never
//!   reassigned). Copying it loses nothing: no one can observe a change; or
//! * **a binding cell** ([`BindingCell`], `HeapObject::Cell`), the storage of
//!   a mutable binding (or of any binding a closure may read before its
//!   initialization). The creating frame keeps the cell in the binding's local
//!   slot and every closure that captures the binding holds the *same* cell,
//!   so they all observe one binding (QuickJS's `JSVarRef`, which
//!   `get_var_ref` shares between closures of one frame), and the binding
//!   outlives the frame because the cell is an ordinary heap object reached
//!   from the closures. A closure created inside a closure re-captures with
//!   `GetCapture`, passing the same cell (or value) on, as QuickJS's
//!   `js_closure2` copies a parent's `var_refs`.
//!
//! Unlike QuickJS there is no "open" variable reference pointing into a live
//! frame and no `close_var_refs` copy when the frame exits: a captured mutable
//! binding lives in its cell from its declaration on, so frames stay plain
//! task-owned storage that may move when the value stack grows, and nothing
//! has to be fixed up when a frame is popped, yielded or collected. Each
//! execution of a declaration's `NewCell` makes a fresh binding (a
//! `let` in a loop body gets one per iteration).
//!
//! **Temporal dead zone.** A cell starts uninitialized; `CellInit` gives it
//! its first value, and `CellGet`/`CellSet` on an uninitialized cell fail with
//! `ReferenceError`. This is the run-time check a compiler needs once a
//! closure can run before a `let`/`const` declaration has executed (the
//! static decision of `crate::frontend::lower` is only sound without
//! closures).
//!
//! # Ownership and publication
//!
//! Executable code is immutable `Arc` storage shared by every task; closure
//! environments are heap objects and follow the publication rules of
//! [`crate::value::share`], which are separate from code sharing:
//!
//! * a function object never changes its code or captured values after
//!   allocation, so it is [`Persistent`](crate::value::share::Shareability::Persistent):
//!   shareable exactly when every captured value is and it has no own
//!   properties (see "Own properties" below). A closure capturing only
//!   immutable shareable values (primitives, persistent collections, other
//!   such closures) may be published and called from another task;
//! * a binding cell is mutable, so it is
//!   [`TaskOwned`](crate::value::share::Shareability::TaskOwned) by the task
//!   that created it. A closure that captures one is therefore rejected by the
//!   publication check, with a path ending in `.capture(i)`. Cell operations
//!   from any other task fail with
//!   [`VmErrorKind::ForeignBinding`](crate::VmErrorKind::ForeignBinding), so
//!   even a closure handed over without the check (`Runtime::spawn` arguments
//!   are not checked) cannot read or write another task's bindings.
//!
//! # Tracing
//!
//! A function object's `Trace` reports its code's constants (primitives in
//! this slice) and its captured values; a cell's reports its value. Cycles
//! (a closure stored in a binding it captures, as in
//! `let f = function () { return f; }`) are ordinary heap cycles: the
//! collector traces through them while they are reachable from a task frame
//! or a root and reclaims them once they are not. A function's property
//! record (below) is one more traced edge.
//!
//! # Own properties (clj-quickjs-125)
//!
//! The decision record (and the alternatives considered) is
//! `docs/architecture.md`, "Function own properties".
//!
//! A function object's JavaScript own properties live in a separate
//! **property record**: an ordinary object (`HeapObject::Plain`) owned by
//! the task that created the function, never itself a JavaScript value,
//! whose `[[Prototype]]` is the function's (normally `Function.prototype`,
//! or the superclass for derived constructors, clj-quickjs-137). A function starts without one; the
//! record is created by the first write of a property (`f.x = 1`) or the
//! first observation of a lazy `prototype`, and from then on
//! [`FunctionObject::properties`] names it. Property operations on the
//! function are the ordinary-object operations on its record
//! (`crate::vm::property`), so attributes, extensibility and the
//! prototype-chain write check are those of `crate::object`.
//!
//! * **`prototype`.** Code from a `function` declaration or expression
//!   ([`Code::constructor_kind`]) has an own `prototype` (writable, not
//!   enumerable, not configurable) whose value is created on first
//!   observation: a read, or a write of `prototype` itself (QuickJS
//!   `js_instantiate_prototype`, revision 7d1c13c5d8f8, does the same on
//!   first access). The value is a new ordinary object owned by the same
//!   task, inheriting from `Object.prototype`, with a non-enumerable
//!   `constructor` that is the function; it is stored in the record, so
//!   later reads give the same object. Arrows and concise methods have no
//!   `prototype`.
//! * **`length` and `name`** (clj-quickjs-101). Before a record exists,
//!   immutable code metadata supplies these non-writable, non-enumerable,
//!   configurable own properties. Reads, descriptors and key enumeration
//!   do not attach a record. Its first creation copies both properties and
//!   materializes a constructor's `prototype` before adding user properties,
//!   preserving creation order. From then on the record is authoritative:
//!   deletion never reveals the code default again. Publication makes the
//!   virtual metadata non-configurable, as for bound functions.
//! * **Inherited reads.** Ordinary functions inherit from `Function.prototype`
//!   (clj-quickjs-102); derived classes inherit from their superclass and
//!   bound functions retain their target's internal prototype (clj-quickjs-137).
//!   These links cannot be changed explicitly yet (`crate::vm::prototype`). A name of
//!   [`FUNCTION_PROTOTYPE_NAMES`] (the standard
//!   `Function.prototype` methods that do not exist yet:
//!   `toString`, ...) fails with `UnsupportedOperand` only if lookup reaches
//!   that intrinsic without finding an own or inherited definition. Other
//!   names continue through the constructor chain to `Function.prototype`
//!   (`call`, `apply`, `bind`, `constructor`) and then `Object.prototype`, so `f.x`
//!   is `undefined` until assigned.
//! * **Ownership.** Only the creating task ([`FunctionObject::owner`])
//!   writes a function's properties or creates its `prototype`; another
//!   task gets `ForeignObject`, as for an ordinary object, and the record
//!   is owner-checked on reads like any ordinary object.
//! * **Publication** (`crate::value::share`). The function object stays
//!   `Persistent`: its code and captures never change, and its one other
//!   mutable edge, the record, is written at most once. A function with a record is
//!   rejected at it (`.properties` in the path; the record is a task-owned
//!   `object`); one without publishes as before. A function that a
//!   publication check has proved shareable (its known-shareable bit is
//!   set) is shared, and like a realm intrinsic it is then immutable: its
//!   record can no longer be created, so a write is a catchable `TypeError`
//!   and the first read of its `prototype` fails with `UnsupportedOperand`.
//!   Attaching a record and setting the bit both happen under the
//!   function's access word and each refuses the other
//!   ([`Heap::mark_shareable_unless`]), so no known-shareable function ever
//!   has a record; for the same reason a function is never known shareable
//!   at allocation, only after a check.
//!
//! # Concise methods (clj-quickjs-126)
//!
//! `Method` allocates a non-constructor closure whose immutable
//! `home_object` is the defining literal. `Trace` visits it separately from
//! captures and the property record. The persistent publication walk follows
//! it with `.home_object`: because literals are task-owned, even detached
//! methods with no captures cannot publish. Super properties resolve through
//! the home object's current prototype, independently of their receiver.
//!
//! Base classes (clj-quickjs-136) eagerly materialize a non-writable
//! `prototype` and retain it as the constructor's home object. Their own
//! property records and method home objects use the same tracing and
//! publication rules; class constructors cannot publish even without captures.

use std::sync::Arc;

use crate::heap::gc::Trace;
use crate::heap::{Heap, HeapObject, ObjectId};
use crate::ids::TaskId;
use crate::object::{OrdinaryObject, Property, PropertyFlags, PropertyKey};
use crate::realm;
use crate::value::{JsString, Value};
use crate::vm::dispatch::Fault;
use crate::vm::executable::Code;
use crate::vm::property::Key;

/// Storage of a function object. See the module docs.
#[derive(Clone, Debug)]
pub(crate) struct FunctionObject {
    pub(crate) code: Arc<Code>,
    /// The closure's environment: `code.captures` values, never changed
    /// after allocation.
    pub(crate) captures: Box<[Value]>,
    /// The task whose run created the function: the only one that may
    /// change its properties (module docs, "Own properties").
    pub(crate) owner: TaskId,
    /// The own-property record, once created: an ordinary object owned by
    /// `owner`. Set at most once, and never on a known-shareable function.
    pub(crate) properties: Option<ObjectId>,
    /// Literal that defined a concise method. Fixed before allocation;
    /// ordinary functions and arrows have no home object.
    pub(crate) home_object: Option<ObjectId>,
    /// Set once by a computed literal definition before the closure escapes.
    /// Per-closure metadata; executable code remains shared and immutable.
    pub(crate) computed_name: Option<JsString>,
}

impl FunctionObject {
    /// A function over `code` with its captured values, created by `owner`,
    /// without own properties yet.
    pub(crate) fn new(code: Arc<Code>, captures: Box<[Value]>, owner: TaskId) -> FunctionObject {
        FunctionObject {
            code,
            captures,
            owner,
            properties: None,
            home_object: None,
            computed_name: None,
        }
    }
}

impl Trace for FunctionObject {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        self.code.trace(visit);
        self.captures.trace(visit);
        if let Some(home) = self.home_object {
            visit(home.into());
        }
        if let Some(record) = self.properties {
            visit(record.into());
        }
    }
}

/// A bound callable has immutable call operands and initial metadata. Own
/// property changes use the same optional, task-owned record as closures.
#[derive(Clone, Debug)]
pub(crate) struct BoundFunction {
    pub(crate) target: Value,
    pub(crate) this: Value,
    pub(crate) args: Box<[Value]>,
    pub(crate) owner: TaskId,
    pub(crate) metadata: ObjectId,
    pub(crate) properties: Option<ObjectId>,
}

impl Trace for BoundFunction {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        self.target.trace(visit);
        self.this.trace(visit);
        self.args.trace(visit);
        visit(self.metadata.into());
        if let Some(properties) = self.properties {
            visit(properties.into());
        }
    }
}

/// Bind snapshots the target's own length (or zero) and its current name.
#[cfg(test)]
pub(crate) fn bind(
    heap: &Heap,
    task: TaskId,
    target: &Value,
    args: &[Value],
) -> Result<Value, Fault> {
    crate::vm::dispatch::callee(heap, target)?;
    let Value::Object(id) = *target else {
        unreachable!()
    };
    let length_key = Key::Name(JsString::from("length"));
    let length = crate::vm::property::get_own_property(heap, task, id, &length_key)?
        .map_or(Value::Undefined, |p| p.value);
    let name = crate::vm::property::get_field(heap, task, target, &JsString::from("name"))?;
    bind_with_metadata(heap, task, target, args, &length, &name)
}

/// Allocate the immutable bound-function metadata after the resumable
/// length/name reads have completed.
pub(crate) fn bind_with_metadata(
    heap: &Heap,
    task: TaskId,
    target: &Value,
    args: &[Value],
    length: &Value,
    name: &Value,
) -> Result<Value, Fault> {
    let length = match length {
        Value::Number(n) if !n.is_nan() => {
            (n.trunc() - args.len().saturating_sub(1) as f64).max(0.0)
        }
        _ => 0.0,
    };
    let name = match name {
        Value::String(name) => JsString::from("bound ")
            .concat(name)
            .ok_or(Fault::StringTooLong)?,
        _ => JsString::from("bound "),
    };
    let Value::Object(target_id) = target else {
        return Err(Fault::Internal("bound target is not an object"));
    };
    let parent = super::prototype::get_prototype_of(heap, task, *target_id)?;
    let mut store = OrdinaryObject::intrinsic(heap, parent);
    store.define_with(
        intern(heap, "length")?,
        Value::Number(length),
        PropertyFlags::FUNCTION_INFO,
    );
    store.define_with(
        intern(heap, "name")?,
        Value::String(name),
        PropertyFlags::FUNCTION_INFO,
    );
    let metadata = heap.alloc(HeapObject::Plain(store)).ok_or_else(exhausted)?;
    heap.alloc(HeapObject::BoundFunction(BoundFunction {
        target: target.clone(),
        this: args.first().cloned().unwrap_or(Value::Undefined),
        args: args.get(1..).unwrap_or(&[]).into(),
        owner: task,
        metadata,
        properties: None,
    }))
    .map(Value::Object)
    .ok_or_else(exhausted)
}

/// The standard properties of `Function.prototype` (ECMAScript §20.2.3)
/// that do not exist yet. Until they exist, reading or writing
/// one on a function that lacks its own fails with `UnsupportedOperand`
/// instead of answering from `Object.prototype` (module docs, "Inherited
/// reads"); so does a lookup that reaches `Function.prototype` itself
/// without finding one ([`lacks_inherited`]). `call`, `apply` and
/// `constructor` are real properties of `Function.prototype`
/// (clj-quickjs-102).
pub(crate) const FUNCTION_PROTOTYPE_NAMES: [&str; 3] = ["toString", "arguments", "caller"];

/// True when `key` names a property `Function.prototype` should have but
/// does not yet (see [`FUNCTION_PROTOTYPE_NAMES`]): a lookup that reaches
/// `Function.prototype` without finding it fails (`crate::vm::property`).
pub(crate) fn lacks_inherited(key: &Key) -> bool {
    matches!(key, Key::Name(name) if FUNCTION_PROTOTYPE_NAMES.iter().any(|n| *name == **n))
}

/// The attributes of an ordinary function's `prototype`: writable, not
/// enumerable, not configurable.
const PROTOTYPE: PropertyFlags = PropertyFlags {
    writable: true,
    enumerable: false,
    configurable: false,
};

/// What a function contributes to a property read
/// ([`crate::vm::property::get`]).
pub(crate) enum FunctionRead {
    /// The value of the property.
    Found(Value),
    /// Continue the ordinary read at this object: the function's record,
    /// or `Function.prototype` for a function without one.
    Continue(ObjectId),
}

/// A snapshot of a function's property state.
struct Info {
    owner: TaskId,
    record: Option<ObjectId>,
    constructor: bool,
    /// Known shareable: published, so its properties are immutable.
    shared: bool,
    defaults: Option<ObjectId>,
    /// Needed only for virtual metadata before a property record exists.
    /// Repeated constructor prototype reads must not clone shared code.
    code: Option<Arc<Code>>,
    computed_name: Option<JsString>,
}

fn info(heap: &Heap, function: ObjectId) -> Result<Info, Fault> {
    heap.read_shareable(function, |object, known| match object {
        HeapObject::Function(f) => Ok(Info {
            owner: f.owner,
            record: f.properties,
            constructor: f.code.constructor_kind.is_constructor() || f.code.is_generator(),
            shared: known,
            defaults: None,
            code: f.properties.is_none().then(|| f.code.clone()),
            computed_name: f.computed_name.clone(),
        }),
        HeapObject::BoundFunction(f) => Ok(Info {
            owner: f.owner,
            record: f.properties,
            constructor: false,
            shared: known,
            defaults: Some(f.metadata),
            code: None,
            computed_name: None,
        }),
        _ => Err(Fault::Internal("not a function object")),
    })
    // The frame keeps the function alive; a stale id is reported.
    .unwrap_or(Err(Fault::Internal("function object was reclaimed")))
}

/// The function's property record, if it has one (own-key enumeration,
/// `crate::vm::keys`, clj-quickjs-123).
pub(crate) fn property_record(heap: &Heap, function: ObjectId) -> Result<Option<ObjectId>, Fault> {
    info(heap, function).map(|info| info.record.or(info.defaults))
}

/// Private elements use the same task-owned record, but ignore public integrity flags.
pub(crate) fn private_record(
    heap: &Heap,
    task: TaskId,
    function: ObjectId,
    create: bool,
) -> Result<Option<ObjectId>, Fault> {
    let info = info(heap, function)?;
    if info.owner != task && !info.shared {
        return Err(Fault::TypeError("foreign private receiver".into()));
    }
    if create {
        writable_record(heap, task, function, &info, &Change::Integrity).map(Some)
    } else {
        Ok(info.record)
    }
}

/// Metadata has no heap store until the first property change. Once a
/// record exists it is authoritative, including the absence of deleted keys.
pub(crate) fn own_metadata(
    heap: &Heap,
    task: TaskId,
    function: ObjectId,
    key: &Key,
) -> Result<Option<Property>, Fault> {
    let info = info(heap, function)?;
    if info.record.is_some() {
        return Ok(None);
    }
    let mut property = initial_metadata(heap, task, &info, key)?;
    if info.shared
        && let Some(p) = &mut property
    {
        p.flags.configurable = false;
    }
    Ok(property)
}

/// Flags-only observation: virtual metadata and a constructor's lazy
/// prototype are non-enumerable. Do not materialize their values, which
/// would attach task-owned state and prevent publication.
pub(crate) fn own_enumerable(
    heap: &Heap,
    task: TaskId,
    function: ObjectId,
    key: &Key,
) -> Result<bool, Fault> {
    let info = info(heap, function)?;
    if !info.shared && info.owner != task {
        return Err(Fault::ForeignObject {
            owner: Some(info.owner),
            caller: task,
        });
    }
    match info.record.or(info.defaults) {
        Some(record) => Ok(super::property::get_own_property(heap, task, record, key)?
            .is_some_and(|p| p.flags.enumerable)),
        None => Ok(false),
    }
}

fn initial_metadata(
    heap: &Heap,
    task: TaskId,
    info: &Info,
    key: &Key,
) -> Result<Option<Property>, Fault> {
    let value = match special(key) {
        Some("length") => info
            .code
            .as_ref()
            .map(|c| Value::Number(f64::from(c.function_length))),
        Some("name") => info.code.as_ref().map(|c| {
            Value::String(
                info.computed_name
                    .clone()
                    .unwrap_or_else(|| c.function_name.clone()),
            )
        }),
        _ => return Ok(None),
    };
    let value = match (value, info.defaults) {
        (Some(v), _) => Some(v),
        (_, Some(defaults)) => record_own(heap, task, defaults, special(key).unwrap())?,
        _ => None,
    };
    Ok(value.map(|value| Property {
        accessor: None,
        value,
        flags: PropertyFlags::FUNCTION_INFO,
    }))
}

/// Names that exist without a property record. No prototype value is
/// created by enumeration, so observations preserve publication eligibility.
pub(crate) fn initial_keys(heap: &Heap, function: ObjectId) -> Result<Vec<&'static str>, Fault> {
    let info = info(heap, function)?;
    Ok(if info.record.is_some() {
        Vec::new()
    } else if info.constructor {
        vec!["length", "name", "prototype"]
    } else {
        vec!["length", "name"]
    })
}

/// Where a `delete` of a function's property applies
/// ([`delete_target`], clj-quickjs-124).
pub(crate) enum FunctionDelete {
    /// The property record: the ordinary `[[Delete]]` runs on it.
    Record(ObjectId),
    /// The function has no such own property (no record, so no own
    /// property besides a constructor's `prototype`).
    Absent,
    /// A constructor's `prototype`, which is not configurable, whether or
    /// not it has been created yet.
    NonConfigurable,
}

/// The function's part of `delete f[key]` for `task` (module docs, "Own
/// properties"). A lazy `prototype` is non-configurable. Deleting initial
/// metadata creates a record first; an absent property creates nothing.
/// Another task's function is `ForeignObject` unless it is published, in
/// which case metadata is non-configurable.
pub(crate) fn delete_target(
    heap: &Heap,
    task: TaskId,
    function: ObjectId,
    key: &Key,
) -> Result<FunctionDelete, Fault> {
    let info = info(heap, function)?;
    if !info.shared && info.owner != task {
        return Err(Fault::ForeignObject {
            owner: Some(info.owner),
            caller: task,
        });
    }
    if info.record.is_none() && initial_metadata(heap, task, &info, key)?.is_some() {
        if info.shared {
            return Ok(FunctionDelete::NonConfigurable);
        }
        let record = writable_record(heap, task, function, &info, &Change::Write(key))?;
        return Ok(FunctionDelete::Record(record));
    }
    match special(key) {
        Some("prototype") if info.constructor => return Ok(FunctionDelete::NonConfigurable),
        _ => {}
    }
    Ok(match info.record {
        Some(record) => FunctionDelete::Record(record),
        None => FunctionDelete::Absent,
    })
}

fn exhausted() -> Fault {
    Fault::OutOfMemory("heap object table exhausted")
}

fn object_prototype(heap: &Heap) -> Result<ObjectId, Fault> {
    realm::intrinsics(heap)
        .map(|r| r.object_prototype)
        .ok_or_else(exhausted)
}

/// The default function prototype intrinsic (clj-quickjs-102).
pub(crate) fn function_prototype(heap: &Heap) -> Result<ObjectId, Fault> {
    realm::intrinsics(heap)
        .map(|r| r.function_prototype)
        .ok_or_else(exhausted)
}

fn intern(heap: &Heap, name: &str) -> Result<PropertyKey, Fault> {
    heap.keys()
        .intern(&JsString::from(name))
        .ok_or(Fault::OutOfMemory("property key table exhausted"))
}

/// The name of `key` if it needs the function's own handling: `prototype`
/// or one of [`FUNCTION_PROTOTYPE_NAMES`].
fn special(key: &Key) -> Option<&'static str> {
    let Key::Name(name) = key else {
        return None;
    };
    // Avoid iterator-adapter frames on the metadata read path: name reads
    // also run on 64 KiB execution workers in debug builds.
    for special in &[
        &["prototype", "length", "name"][..],
        &FUNCTION_PROTOTYPE_NAMES[..],
    ] {
        for &special in *special {
            if *name == special {
                return Some(special);
            }
        }
    }
    None
}

/// The own property `name` of the record, owner-checked like any ordinary
/// object read.
fn record_own(
    heap: &Heap,
    task: TaskId,
    record: ObjectId,
    name: &str,
) -> Result<Option<Value>, Fault> {
    record_property(heap, task, record, name).map(|p| p.map(|p| p.value))
}

fn record_property(
    heap: &Heap,
    task: TaskId,
    record: ObjectId,
    name: &str,
) -> Result<Option<Property>, Fault> {
    let Some(key) = heap.keys().lookup(&JsString::from(name)) else {
        return Ok(None);
    };
    heap.read(record, |object| match object {
        HeapObject::Plain(o) => match o.owner() {
            Some(owner) if owner != task => Err(Fault::ForeignObject {
                owner: Some(owner),
                caller: task,
            }),
            _ => Ok(o.get_own(key).cloned()),
        },
        _ => Err(Fault::Internal(
            "a function's property record is not an object",
        )),
    })
    .unwrap_or(Err(Fault::Internal("property record was reclaimed")))
}

fn unsupported_inherited(name: &str, verb: &str) -> Fault {
    Fault::Unsupported(format!(
        "cannot {verb} property '{name}' of a function: Function.prototype and function \
         methods are not supported"
    ))
}

/// The function's part of reading `key` for `task` (module docs, "Own
/// properties").
pub(crate) fn read(
    heap: &Heap,
    task: TaskId,
    function: ObjectId,
    key: &Key,
) -> Result<FunctionRead, Fault> {
    read_or_probe(heap, task, function, key, false)
}

/// [`read`] for HasProperty (the `in` operator, clj-quickjs-74): a
/// constructor's lazy `prototype` is reported present (as
/// `Found(undefined)`, a presence marker) without being created, as QuickJS
/// `JS_HasProperty` leaves autoinit properties alone, so a published
/// function can be asked and an unpublished one gains no task-owned state.
pub(crate) fn probe(
    heap: &Heap,
    task: TaskId,
    function: ObjectId,
    key: &Key,
) -> Result<FunctionRead, Fault> {
    read_or_probe(heap, task, function, key, true)
}

fn read_or_probe(
    heap: &Heap,
    task: TaskId,
    function: ObjectId,
    key: &Key,
    probe: bool,
) -> Result<FunctionRead, Fault> {
    let info = info(heap, function)?;
    if info.record.is_none()
        && let Some(p) = initial_metadata(heap, task, &info, key)?
    {
        return Ok(FunctionRead::Found(p.value));
    }
    if info.defaults.is_some() {
        return Ok(FunctionRead::Continue(
            info.record.or(info.defaults).unwrap(),
        ));
    }
    if let Some(name) = special(key) {
        if let Some(record) = info.record
            && let Some(property) = record_property(heap, task, record, name)?
        {
            // The record may hold an accessor. Let the ordinary property
            // walk inspect its kind and retain the original function receiver.
            return Ok(if property.accessor.is_some() {
                FunctionRead::Continue(record)
            } else {
                FunctionRead::Found(property.value)
            });
        }
        if name == "prototype" && info.constructor {
            if probe {
                return Ok(FunctionRead::Found(Value::Undefined));
            }
            let record = writable_record(heap, task, function, &info, &Change::CreatePrototype)?;
            return Ok(FunctionRead::Found(materialize(
                heap, task, function, record,
            )?));
        }
    }
    match info.record {
        Some(record) => Ok(FunctionRead::Continue(record)),
        None => function_prototype(heap).map(FunctionRead::Continue),
    }
}

/// The function's part of writing `key` for `task`: the record the write
/// goes to (created if needed, with `prototype` materialized when that is
/// the key), on which the ordinary write then runs. A name of
/// [`FUNCTION_PROTOTYPE_NAMES`] the function lacks fails: the write would
/// consult `Function.prototype`.
pub(crate) fn write_target(
    heap: &Heap,
    task: TaskId,
    function: ObjectId,
    key: &Key,
) -> Result<ObjectId, Fault> {
    change_target(heap, task, function, key, &FUNCTION_PROTOTYPE_NAMES)
}

/// An untouched function's metadata rejects assignment without
/// changing its representation or publication eligibility. The caller
/// applies the ordinary strict/sloppy [[Set]] rejection rule.
pub(crate) fn rejects_metadata_write(
    heap: &Heap,
    task: TaskId,
    function: ObjectId,
    key: &Key,
) -> Result<bool, Fault> {
    let info = info(heap, function)?;
    if info.record.is_none() && matches!(special(key), Some("length" | "name")) {
        if !info.shared && info.owner != task {
            return Err(Fault::ForeignObject {
                owner: Some(info.owner),
                caller: task,
            });
        }
        return Ok(true);
    }
    Ok(false)
}

/// The function's part of `Object.defineProperty` (clj-quickjs-119): the
/// record the definition goes to, as [`write_target`], with `prototype`
/// materialized before a partial descriptor is validated against it.
/// A definition ignores the prototype chain.
pub(crate) fn define_target(
    heap: &Heap,
    task: TaskId,
    function: ObjectId,
    key: &Key,
) -> Result<ObjectId, Fault> {
    change_target(heap, task, function, key, &[])
}

/// The store holding the function's own property `key`, for
/// `[[GetOwnProperty]]` (`Object.getOwnPropertyDescriptor`): its record,
/// with `prototype` materialized when that is the key (a constructor's
/// `prototype` always exists), or `None` when it has no record (and so no
/// stored property). Unrecorded metadata uses [`own_metadata`] instead.
pub(crate) fn own_store(
    heap: &Heap,
    task: TaskId,
    function: ObjectId,
    key: &Key,
) -> Result<Option<ObjectId>, Fault> {
    let info = info(heap, function)?;
    if info.defaults.is_some() {
        return Ok(info.record.or(info.defaults));
    }
    match special(key) {
        Some("prototype") if info.constructor => {
            let record = writable_record(heap, task, function, &info, &Change::CreatePrototype)?;
            materialize(heap, task, function, record)?;
            return Ok(Some(record));
        }
        _ => {}
    }
    Ok(info.record)
}

/// Where a function's `[[Extensible]]` flag and integrity level live
/// (clj-quickjs-122; module docs, "Own properties").
pub(crate) enum IntegrityStore {
    /// Published: shared and immutable, so not extensible and frozen.
    Shared,
    /// No property record: extensible (a constructor's lazy `prototype`
    /// is a writable own property).
    Unrecorded,
    /// The property record, an ordinary object of the function's owner:
    /// its flag and attributes are the function's.
    Record(ObjectId),
}

/// The store `Object.isExtensible` and `Object.isFrozen` read for
/// `function`. Creates nothing.
pub(crate) fn integrity_store(heap: &Heap, function: ObjectId) -> Result<IntegrityStore, Fault> {
    let info = info(heap, function)?;
    Ok(match (info.shared, info.record) {
        (true, _) => IntegrityStore::Shared,
        (false, Some(record)) => IntegrityStore::Record(record),
        (false, None) => IntegrityStore::Unrecorded,
    })
}

/// The store `Object.preventExtensions` and `Object.freeze` change for
/// `task`: the property record, created if needed, with a constructor's
/// `prototype` materialized first (so it exists as an own property before
/// the record stops taking new ones), or [`IntegrityStore::Shared`] for a
/// published function, which is already frozen. Another task's function
/// is `ForeignObject`. Never [`IntegrityStore::Unrecorded`].
pub(crate) fn integrity_target(
    heap: &Heap,
    task: TaskId,
    function: ObjectId,
) -> Result<IntegrityStore, Fault> {
    integrity_target_from(heap, task, function, &info(heap, function)?)
}

/// [`integrity_target`] from the snapshot `info`, which a publication
/// check may have overtaken (tests).
fn integrity_target_from(
    heap: &Heap,
    task: TaskId,
    function: ObjectId,
    info: &Info,
) -> Result<IntegrityStore, Fault> {
    if info.shared {
        return Ok(IntegrityStore::Shared);
    }
    let record = match writable_record(heap, task, function, info, &Change::Integrity) {
        Ok(record) => record,
        // A publication check proved it shareable since `info` was read.
        Err(_) if self::info(heap, function)?.shared => return Ok(IntegrityStore::Shared),
        Err(fault) => return Err(fault),
    };
    if info.constructor {
        materialize(heap, task, function, record)?;
    }
    Ok(IntegrityStore::Record(record))
}

fn change_target(
    heap: &Heap,
    task: TaskId,
    function: ObjectId,
    key: &Key,
    blocked: &[&str],
) -> Result<ObjectId, Fault> {
    let info = info(heap, function)?;
    let name = special(key);
    if let Some(name) = name
        && blocked.contains(&name)
    {
        let own = match info.record {
            Some(record) => record_own(heap, task, record, name)?.is_some(),
            None => false,
        };
        if !own {
            // Probe the actual constructor chain without invoking accessors.
            // The property walker rejects a missing Function.prototype name
            // only when it reaches that intrinsic, after inherited statics.
            match super::property::has_property(heap, task, &Value::Object(function), key) {
                Err(Fault::Unsupported(_)) => return Err(unsupported_inherited(name, "set")),
                result => {
                    result?;
                }
            }
        }
    }
    let record = writable_record(heap, task, function, &info, &Change::Write(key))?;
    if name == Some("prototype") && info.constructor {
        materialize(heap, task, function, record)?;
    }
    Ok(record)
}

/// Why [`writable_record`] needs the record, which decides how it fails on
/// a published function.
enum Change<'k> {
    /// The first read of a constructor's `prototype` creates it: an
    /// explicit gap (`UnsupportedOperand`) on a published function, since a
    /// read has no JavaScript failure for it.
    CreatePrototype,
    /// A write or definition of this property: the catchable `TypeError`
    /// of an immutable object, as for a realm intrinsic.
    Write(&'k Key),
    /// `Object.preventExtensions` or `Object.freeze` (clj-quickjs-122). A
    /// published function is already frozen, so [`integrity_target`] turns
    /// this failure into success.
    Integrity,
}

impl Change<'_> {
    /// The failure of this change on a published function.
    fn on_shared(&self) -> Fault {
        match self {
            Change::CreatePrototype => Fault::Unsupported(
                "cannot create the prototype object of a published function: functions \
                 shared between tasks are immutable"
                    .to_string(),
            ),
            Change::Write(key) => Fault::TypeError(format!(
                "cannot set property '{key}' of a published function: functions shared \
                 between tasks are immutable"
            )),
            Change::Integrity => Fault::TypeError(
                "cannot change the extensibility of a published function: functions shared \
                 between tasks are immutable"
                    .to_string(),
            ),
        }
    }
}

/// The record of a function `task` may change, created if needed. Fails if
/// the function is shared (published), as `change` says, or belongs to
/// another task.
fn writable_record(
    heap: &Heap,
    task: TaskId,
    function: ObjectId,
    info: &Info,
    change: &Change<'_>,
) -> Result<ObjectId, Fault> {
    if info.shared {
        return Err(change.on_shared());
    }
    if info.owner != task {
        return Err(Fault::ForeignObject {
            owner: Some(info.owner),
            caller: task,
        });
    }
    if let Some(record) = info.record {
        return Ok(record);
    }
    // Allocated, then attached in the same instruction (no safepoint
    // between): reachable from the function, which the frame holds.
    let mut fresh = OrdinaryObject::new(heap, Some(task));
    fresh.set_prototype(inheritance_prototype(heap, function)?);
    for name in ["length", "name"] {
        let key = Key::Name(JsString::from(name));
        if let Some(p) = initial_metadata(heap, task, info, &key)? {
            fresh.define_with(intern(heap, name)?, p.value, p.flags);
        }
    }
    let fresh = heap.alloc(HeapObject::Plain(fresh)).ok_or_else(exhausted)?;
    let record = heap
        .with_unshared_object_mut(function, |object| {
            let properties = match object {
                HeapObject::Function(f) => &mut f.properties,
                HeapObject::BoundFunction(f) => &mut f.properties,
                _ => return Err(Fault::Internal("not a function object")),
            };
            Ok(*properties.get_or_insert(fresh))
        })
        .ok_or(Fault::Internal("function object was reclaimed"))?
        // A check proved it shareable since `info` was read.
        .unwrap_or_else(|| Err(change.on_shared()))?;
    // Complete initial property order before a user property can be added.
    // Untouched functions still answer metadata and keys without a record.
    if info.constructor {
        materialize(heap, task, function, record)?;
    }
    Ok(record)
}

/// The value of the function's `prototype`, creating it if the record does
/// not have it yet (module docs).
fn materialize(
    heap: &Heap,
    task: TaskId,
    function: ObjectId,
    record: ObjectId,
) -> Result<Value, Fault> {
    if let Some(value) = record_own(heap, task, record, "prototype")? {
        return Ok(value);
    }
    let constructor = intern(heap, "constructor")?;
    let prototype_key = intern(heap, "prototype")?;
    let (generator, class, asynchronous) = heap
        .read(function, |object| match object {
            HeapObject::Function(f) => (
                f.code.is_generator(),
                f.code.constructor_kind.is_class(),
                f.code.is_async_generator(),
            ),
            _ => (false, false, false),
        })
        .ok_or(Fault::Internal("function object was reclaimed"))?;
    let parent = if asynchronous {
        super::async_generator::prototype(heap)?
    } else if generator {
        super::generator::prototype(heap)?
    } else {
        object_prototype(heap)?
    };
    let mut prototype = OrdinaryObject::with_prototype(heap, task, parent);
    if !generator {
        prototype.define_with(constructor, Value::Object(function), PropertyFlags::METHOD);
    }
    let prototype = heap
        .alloc(HeapObject::Plain(prototype))
        .map(Value::Object)
        .ok_or_else(exhausted)?;
    let defined = heap
        .with_object_mut(record, |object| match object {
            HeapObject::Plain(o) => o.define_with(
                prototype_key,
                prototype.clone(),
                PropertyFlags {
                    writable: !class,
                    ..PROTOTYPE
                },
            ),
            _ => false,
        })
        .unwrap_or(false);
    if !defined {
        return Err(Fault::TypeError(
            "cannot define property 'prototype': the function is not extensible".to_string(),
        ));
    }
    Ok(prototype)
}

/// Classes eagerly materialize their prototype. Its task-owned property
/// record and home object participate in existing tracing/publication checks.
pub(crate) fn initialize_class(heap: &Heap, task: TaskId, function: ObjectId) -> Result<(), Fault> {
    let key = Key::Name(JsString::from("prototype"));
    let record = own_store(heap, task, function, &key)?
        .ok_or(Fault::Internal("class prototype record is missing"))?;
    let Value::Object(prototype) = materialize(heap, task, function, record)? else {
        return Err(Fault::Internal("class prototype is not an object"));
    };
    heap.with_object_mut(function, |o| {
        if let HeapObject::Function(f) = o {
            f.home_object = Some(prototype);
        }
    })
    .ok_or(Fault::Internal("class constructor was reclaimed"))?;
    Ok(())
}

/// Install both inheritance links on a newly created class. Function
/// inheritance lives in the already traced own-property record.
pub(crate) fn inherit_class(
    heap: &Heap,
    task: TaskId,
    function: ObjectId,
    parent: &Value,
    parent_prototype: &Value,
) -> Result<(), Fault> {
    let proto = match parent_prototype {
        Value::Object(id) => Some(*id),
        Value::Null => None,
        _ if matches!(parent, Value::Null) => None,
        _ => {
            return Err(Fault::TypeError(
                "superclass prototype must be an object or null".into(),
            ));
        }
    };
    let (record, home) = heap
        .read(function, |o| match o {
            HeapObject::Function(f) => f.properties.zip(f.home_object),
            _ => None,
        })
        .flatten()
        .ok_or(Fault::Internal("class inheritance target is not a class"))?;
    if let Value::Object(parent) = parent {
        heap.with_object_mut(record, |o| {
            if let HeapObject::Plain(o) = o {
                o.set_prototype(Some(*parent));
            }
        })
        .ok_or(Fault::Internal("class record was reclaimed"))?;
    }
    super::prototype::set_prototype_of(heap, task, home, proto)
}

pub(crate) fn inheritance_prototype(
    heap: &Heap,
    function: ObjectId,
) -> Result<Option<ObjectId>, Fault> {
    let info = info(heap, function)?;
    let record = info.record.or(info.defaults);
    if let Some(record) = record {
        return heap
            .read(record, |o| match o {
                HeapObject::Plain(o) => Ok(o.prototype()),
                _ => Err(Fault::Internal("function record is not an object")),
            })
            .ok_or(Fault::Internal("function record was reclaimed"))?;
    }
    function_prototype(heap).map(Some)
}

/// Install [[Prototype]] on a freshly compiled Function constructor result.
pub(crate) fn initialize_constructor_prototype(
    heap: &Heap,
    task: TaskId,
    function: ObjectId,
    prototype: ObjectId,
) -> Result<(), Fault> {
    if inheritance_prototype(heap, function)? == Some(prototype) {
        return Ok(());
    }
    let record = define_target(
        heap,
        task,
        function,
        &Key::Name(JsString::from("prototype")),
    )?;
    super::prototype::set_prototype_of(heap, task, record, Some(prototype))
}

/// Storage of a mutable captured binding. See the module docs.
#[derive(Debug)]
pub(crate) struct BindingCell {
    /// The task whose run created the cell; the only one that may use it.
    pub(crate) owner: TaskId,
    /// `None` until `CellInit` runs (temporal dead zone).
    pub(crate) value: Option<Value>,
}

impl Trace for BindingCell {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        self.value.iter().for_each(|v| v.trace(visit));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::Scope;
    use crate::value::share;

    const OWNER: TaskId = TaskId::from_raw(1);

    fn function(heap: &Heap, source: &str) -> ObjectId {
        let code = crate::frontend::compile_function(source).unwrap().0;
        let f = FunctionObject::new(code, Box::new([]), OWNER);
        heap.alloc(HeapObject::Function(f)).unwrap()
    }

    fn callable(heap: &Heap, source: &str, bound: bool) -> ObjectId {
        let f = function(heap, source);
        if !bound {
            return f;
        }
        let Value::Object(f) = bind(heap, OWNER, &Value::Object(f), &[])
            .ok()
            .expect("bound function")
        else {
            panic!()
        };
        f
    }

    fn record(heap: &Heap, f: ObjectId) -> Option<ObjectId> {
        heap.read(f, |o| match o {
            HeapObject::Function(f) => f.properties,
            HeapObject::BoundFunction(f) => f.properties,
            _ => None,
        })
        .unwrap()
    }

    /// Publication wins the race against the owner's first write: the
    /// owner's snapshot says "not shared", a check then proves the function
    /// (and a vector holding it) shareable, and the attach under the lock
    /// refuses, so nothing mutable ends up under a cached shareable value.
    #[test]
    fn publication_wins_before_record_attachment() {
        for bound in [false, true] {
            let heap = Arc::new(Heap::default());
            let _scope = Scope::new(&heap);
            let f = callable(&heap, "function f() {}", bound);
            let stale = info(&heap, f).ok().expect("a live function");
            assert!(!stale.shared && stale.record.is_none());

            let holder = crate::collections::new_vector(&heap, &[Value::Object(f)]).unwrap();
            assert_eq!(share::check(&heap, &holder), Ok(()));
            assert_eq!(heap.known_shareable(f), Some(true));

            let key = Key::Name(JsString::from("x"));
            match writable_record(&heap, OWNER, f, &stale, &Change::Write(&key)) {
                Err(Fault::TypeError(m)) => assert!(m.contains("published function"), "{m}"),
                _ => panic!("expected the published-function TypeError"),
            }
            match writable_record(&heap, OWNER, f, &stale, &Change::CreatePrototype) {
                Err(Fault::Unsupported(m)) => assert!(m.contains("published function"), "{m}"),
                _ => panic!("expected UnsupportedOperand"),
            }
            assert_eq!(record(&heap, f), None, "nothing attached");
            // The cached verdicts still hold: republishing walks nothing new.
            assert_eq!(share::check(&heap, &holder), Ok(()));
            assert_eq!(heap.known_shareable(f), Some(true));
        }
    }

    /// Publication wins the race against `Object.freeze` /
    /// `Object.preventExtensions` (clj-quickjs-122): the snapshot says "not
    /// shared", a check then proves the function shareable, the attach
    /// refuses, and the change reports the function as already frozen
    /// (`Shared`) instead of failing; nothing is attached.
    #[test]
    fn publication_wins_before_integrity_change() {
        for bound in [false, true] {
            let heap = Arc::new(Heap::default());
            let _scope = Scope::new(&heap);
            let f = callable(&heap, "function f() {}", bound);
            let stale = info(&heap, f).ok().expect("a live function");
            assert!(!stale.shared && stale.record.is_none());

            let holder = crate::collections::new_vector(&heap, &[Value::Object(f)]).unwrap();
            assert_eq!(share::check(&heap, &holder), Ok(()));
            assert!(matches!(
                integrity_target_from(&heap, OWNER, f, &stale),
                Ok(IntegrityStore::Shared)
            ));
            assert!(matches!(
                integrity_store(&heap, f),
                Ok(IntegrityStore::Shared)
            ));
            assert_eq!(record(&heap, f), None, "nothing attached");
            assert_eq!(share::check(&heap, &holder), Ok(()));
            assert_eq!(heap.known_shareable(f), Some(true));

            // Without the race the owner gets a record with the prototype.
            let g = callable(&heap, "function g() {}", bound);
            let fresh = info(&heap, g).ok().expect("a live function");
            let Ok(IntegrityStore::Record(r)) = integrity_target_from(&heap, OWNER, g, &fresh)
            else {
                panic!("expected the property record");
            };
            assert_eq!(record(&heap, g), Some(r));
            assert!(
                record_own(&heap, OWNER, r, if bound { "length" } else { "prototype" })
                    .ok()
                    .flatten()
                    .is_some()
            );
        }
    }

    /// The lazy `prototype` through the production read path: one object,
    /// stored with the ordinary-function attributes, whose `constructor`
    /// has the built-in method attributes.
    #[test]
    fn lazy_prototype_has_ordinary_function_descriptors() {
        let heap = Arc::new(Heap::default());
        let _scope = Scope::new(&heap);
        let f = function(&heap, "function f() {}");
        let key = Key::Name(JsString::from("prototype"));
        let read_prototype = || match read(&heap, OWNER, f, &key).ok().expect("a prototype read") {
            FunctionRead::Found(v) => v,
            FunctionRead::Continue(_) => panic!("prototype not found"),
        };
        let prototype = read_prototype();
        assert!(prototype.same_object(&read_prototype()));
        let record = record(&heap, f).expect("the read attached a record");
        let own = |object: ObjectId, name: &str| {
            let k = heap.keys().lookup(&JsString::from(name)).unwrap();
            heap.read(object, |o| match o {
                HeapObject::Plain(o) => o.get_own(k).cloned(),
                _ => None,
            })
            .flatten()
            .unwrap()
        };
        let stored = own(record, "prototype");
        assert!(stored.value.same_object(&prototype));
        assert_eq!(stored.flags, PROTOTYPE);
        assert_eq!(
            (
                stored.flags.writable,
                stored.flags.enumerable,
                stored.flags.configurable
            ),
            (true, false, false)
        );
        let Value::Object(prototype) = prototype else {
            panic!("not an object")
        };
        let constructor = own(prototype, "constructor");
        assert!(constructor.value.same_object(&Value::Object(f)));
        assert_eq!(constructor.flags, PropertyFlags::METHOD);
        assert_eq!(
            (
                constructor.flags.writable,
                constructor.flags.enumerable,
                constructor.flags.configurable
            ),
            (true, false, true)
        );
    }
}
