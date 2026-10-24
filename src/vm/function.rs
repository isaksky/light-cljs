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
//! whose `[[Prototype]]` is `Object.prototype` (standing in for
//! `Function.prototype`, see below). A function starts without one; the
//! record is created by the first write of a property (`f.x = 1`) or the
//! first observation of a lazy `prototype`, and from then on
//! [`FunctionObject::properties`] names it. Property operations on the
//! function are the ordinary-object operations on its record
//! (`crate::vm::property`), so attributes, extensibility and the
//! prototype-chain write check are those of `crate::object`.
//!
//! * **`prototype`.** Code from a `function` declaration or expression
//!   ([`Code::constructor`]) has an own `prototype` (writable, not
//!   enumerable, not configurable) whose value is created on first
//!   observation: a read, or a write of `prototype` itself (QuickJS
//!   `js_instantiate_prototype`, revision 7d1c13c5d8f8, does the same on
//!   first access). The value is a new ordinary object owned by the same
//!   task, inheriting from `Object.prototype`, with a non-enumerable
//!   `constructor` that is the function; it is stored in the record, so
//!   later reads give the same object. Arrow functions have no `prototype`.
//! * **Inherited reads.** A function's `[[Prototype]]` is
//!   `Function.prototype`, which does not exist yet (clj-quickjs-102). A
//!   name `Function.prototype` will have as its own
//!   ([`FUNCTION_PROTOTYPE_NAMES`]: `call`, `apply`, `bind`, `toString`,
//!   `constructor`, ..., and `length` / `name`, which are also every
//!   function's own properties, clj-quickjs-101) fails with
//!   `UnsupportedOperand` unless the function has it as its own; any other
//!   name continues at `Object.prototype`, where the chain continues after
//!   `Function.prototype`, so `f.x` is `undefined` until assigned.
//! * **Ownership.** Only the creating task ([`FunctionObject::owner`])
//!   writes a function's properties or creates its `prototype`; another
//!   task gets `ForeignObject`, as for an ordinary object, and the record
//!   is owner-checked on reads like any ordinary object.
//! * **Publication** (`crate::value::share`). The function object stays
//!   `Persistent`: its code and captures never change, and its one other
//!   edge, the record, is written at most once. A function with a record is
//!   rejected at it (`.properties` in the path; the record is a task-owned
//!   `object`); one without publishes as before. A function that a
//!   publication check has proved shareable (its known-shareable bit is
//!   set) is shared, and like a realm intrinsic it is then immutable: its
//!   record can no longer be created, so a write is a catchable `TypeError`
//!   and the first read of its `prototype` fails with `UnsupportedOperand`.
//!   Attaching a record and setting the bit both happen under the
//!   object-table lock and each refuses the other
//!   ([`Heap::mark_shareable_unless`]), so no known-shareable function ever
//!   has a record; for the same reason a function is never known shareable
//!   at allocation, only after a check.

use std::sync::Arc;

use crate::heap::gc::Trace;
use crate::heap::{Heap, HeapObject, ObjectId};
use crate::ids::TaskId;
use crate::object::{OrdinaryObject, PropertyFlags, PropertyKey};
use crate::realm;
use crate::value::{JsString, Value};
use crate::vm::dispatch::Fault;
use crate::vm::executable::Code;
use crate::vm::property::Key;

/// Storage of a function object. See the module docs.
#[derive(Debug)]
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
        }
    }
}

impl Trace for FunctionObject {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        self.code.trace(visit);
        self.captures.trace(visit);
        if let Some(record) = self.properties {
            visit(record);
        }
    }
}

/// The own properties `Function.prototype` will have (ECMAScript §20.2.3,
/// plus `length` and `name`, which every function also has as its own,
/// clj-quickjs-101). Until they exist, reading or writing one on a function
/// that lacks its own fails with `UnsupportedOperand` instead of answering
/// from `Object.prototype` (module docs, "Inherited reads").
pub(crate) const FUNCTION_PROTOTYPE_NAMES: [&str; 9] = [
    "length",
    "name",
    "constructor",
    "apply",
    "bind",
    "call",
    "toString",
    "arguments",
    "caller",
];

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
    /// or `Object.prototype` for a function without one.
    Continue(ObjectId),
}

/// A snapshot of a function's property state.
struct Info {
    owner: TaskId,
    record: Option<ObjectId>,
    constructor: bool,
    /// Known shareable: published, so its properties are immutable.
    shared: bool,
}

fn info(heap: &Heap, function: ObjectId) -> Result<Info, Fault> {
    heap.read_shareable(function, |object, known| match object {
        HeapObject::Function(f) => Ok(Info {
            owner: f.owner,
            record: f.properties,
            constructor: f.code.constructor,
            shared: known,
        }),
        _ => Err(Fault::Internal("not a function object")),
    })
    // The frame keeps the function alive; a stale id is reported.
    .unwrap_or(Err(Fault::Internal("function object was reclaimed")))
}

fn exhausted() -> Fault {
    Fault::OutOfMemory("heap object table exhausted")
}

fn object_prototype(heap: &Heap) -> Result<ObjectId, Fault> {
    realm::intrinsics(heap)
        .map(|r| r.object_prototype)
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
    std::iter::once("prototype")
        .chain(FUNCTION_PROTOTYPE_NAMES)
        .find(|special| *name == **special)
}

/// The own property `name` of the record, owner-checked like any ordinary
/// object read.
fn record_own(
    heap: &Heap,
    task: TaskId,
    record: ObjectId,
    name: &str,
) -> Result<Option<Value>, Fault> {
    let Some(key) = heap.keys().lookup(&JsString::from(name)) else {
        return Ok(None);
    };
    heap.read(record, |object| match object {
        HeapObject::Plain(o) => match o.owner() {
            Some(owner) if owner != task => Err(Fault::ForeignObject {
                owner: Some(owner),
                caller: task,
            }),
            _ => Ok(o.get_own(key).map(|p| p.value.clone())),
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
         length/name are not supported"
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
    let info = info(heap, function)?;
    if let Some(name) = special(key) {
        if let Some(record) = info.record
            && let Some(value) = record_own(heap, task, record, name)?
        {
            return Ok(FunctionRead::Found(value));
        }
        if name == "prototype" {
            if info.constructor {
                let record =
                    writable_record(heap, task, function, &info, &Change::CreatePrototype)?;
                return Ok(FunctionRead::Found(materialize(
                    heap, task, function, record,
                )?));
            }
        } else {
            return Err(unsupported_inherited(name, "read"));
        }
    }
    match info.record {
        Some(record) => Ok(FunctionRead::Continue(record)),
        None => object_prototype(heap).map(FunctionRead::Continue),
    }
}

/// The function's part of writing `key` for `task`: the record the write
/// goes to (created if needed, with `prototype` materialized when that is
/// the key), on which the ordinary write then runs. A name of
/// [`FUNCTION_PROTOTYPE_NAMES`] the function lacks fails: the write would
/// consult `Function.prototype` (or the function's own `length`/`name`).
pub(crate) fn write_target(
    heap: &Heap,
    task: TaskId,
    function: ObjectId,
    key: &Key,
) -> Result<ObjectId, Fault> {
    change_target(heap, task, function, key, &FUNCTION_PROTOTYPE_NAMES)
}

/// The function's part of `Object.defineProperty` (clj-quickjs-119): the
/// record the definition goes to, as [`write_target`], with `prototype`
/// materialized before a partial descriptor is validated against it. A
/// definition ignores the prototype chain, so only `length` and `name`,
/// which every function has as its own (clj-quickjs-101), fail.
pub(crate) fn define_target(
    heap: &Heap,
    task: TaskId,
    function: ObjectId,
    key: &Key,
) -> Result<ObjectId, Fault> {
    change_target(heap, task, function, key, &["length", "name"])
}

/// The store holding the function's own property `key`, for
/// `[[GetOwnProperty]]` (`Object.getOwnPropertyDescriptor`): its record,
/// with `prototype` materialized when that is the key (a constructor's
/// `prototype` always exists), or `None` when it has no record (and so no
/// own property). `length` and `name` fail until clj-quickjs-101.
pub(crate) fn own_store(
    heap: &Heap,
    task: TaskId,
    function: ObjectId,
    key: &Key,
) -> Result<Option<ObjectId>, Fault> {
    let info = info(heap, function)?;
    match special(key) {
        Some(name @ ("length" | "name")) => {
            let own = match info.record {
                Some(record) => record_own(heap, task, record, name)?.is_some(),
                None => false,
            };
            if !own {
                return Err(unsupported_inherited(name, "read"));
            }
        }
        Some("prototype") if info.constructor => {
            let record = writable_record(heap, task, function, &info, &Change::CreatePrototype)?;
            materialize(heap, task, function, record)?;
            return Ok(Some(record));
        }
        _ => {}
    }
    Ok(info.record)
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
            return Err(unsupported_inherited(name, "set"));
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
    let fresh = OrdinaryObject::with_prototype(task, object_prototype(heap)?);
    let fresh = heap.alloc(HeapObject::Plain(fresh)).ok_or_else(exhausted)?;
    heap.with_object_mut_known(function, |object, known| match object {
        HeapObject::Function(f) => match f.properties {
            Some(record) => Ok(record),
            // A check proved it shareable since `info` was read.
            None if known => Err(change.on_shared()),
            None => {
                f.properties = Some(fresh);
                Ok(fresh)
            }
        },
        _ => Err(Fault::Internal("not a function object")),
    })
    .unwrap_or(Err(Fault::Internal("function object was reclaimed")))
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
    let mut prototype = OrdinaryObject::with_prototype(task, object_prototype(heap)?);
    prototype.define_with(constructor, Value::Object(function), PropertyFlags::METHOD);
    let prototype = heap
        .alloc(HeapObject::Plain(prototype))
        .map(Value::Object)
        .ok_or_else(exhausted)?;
    let defined = heap
        .with_object_mut(record, |object| match object {
            HeapObject::Plain(o) => o.define_with(prototype_key, prototype.clone(), PROTOTYPE),
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

/// Storage of a mutable captured binding. See the module docs.
#[derive(Debug)]
pub(crate) struct BindingCell {
    /// The task whose run created the cell; the only one that may use it.
    pub(crate) owner: TaskId,
    /// `None` until `CellInit` runs (temporal dead zone).
    pub(crate) value: Option<Value>,
}

impl Trace for BindingCell {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
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

    fn record(heap: &Heap, f: ObjectId) -> Option<ObjectId> {
        heap.read(f, |o| match o {
            HeapObject::Function(f) => f.properties,
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
        let heap = Arc::new(Heap::default());
        let _scope = Scope::new(&heap);
        let f = function(&heap, "function f() {}");
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
