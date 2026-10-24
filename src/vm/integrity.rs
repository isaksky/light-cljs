//! Extensibility and the frozen integrity level (clj-quickjs-122):
//! `Object.isExtensible`, `Object.preventExtensions`, `Object.freeze` and
//! `Object.isFrozen`, after QuickJS's `js_object_isExtensible`,
//! `js_object_preventExtensions`, `JS_PreventExtensions`, `js_object_seal`
//! and `js_object_isSealed` with their freeze flag
//! (`research/quickjs/quickjs.c`, revision 7d1c13c5d8f8); written fresh over
//! this runtime's object model (`crate::object`, `crate::object::array`,
//! `crate::vm::function`).
//!
//! # What each value reports
//!
//! | Argument | `isExtensible` | `isFrozen` | `preventExtensions` / `freeze` |
//! |----------|----------------|------------|--------------------------------|
//! | a primitive | `false` | `true` | returns it unchanged |
//! | ordinary object or array of the running task | its `[[Extensible]]` flag | not extensible and no own property (elements and `length` included) writable or configurable | clears the flag / also makes every own property non-writable and non-configurable; returns the object |
//! | ordinary object created by the embedder (no owner) | as above | as above | `ForeignObject` (no task changes it) |
//! | another task's object or array | `ForeignObject` | `ForeignObject` | `ForeignObject` |
//! | realm intrinsic (`Object.prototype`, `Object`, ...) | its flag: `true` | `false` (its descriptors say writable/configurable) | catchable `TypeError`: built-in objects are shared and immutable |
//! | unpublished function of the running task | its property record's flag (`true` without a record) | the record's level (`false` without one) | on the property record, created if needed (a constructor's `prototype` is created first), so the function can no longer be published |
//! | published function | `false` | `true` | returns it: already frozen |
//! | another task's unpublished function | `true` without a record, else `ForeignObject` | likewise (`false`) | `ForeignObject` |
//! | persistent vector, map, occurrence; builder, atom, binding cell; native payload | `false` | `true` | returns it: already frozen |
//! Error instances, including engine errors, follow the ordinary object row.
//!
//! The runtime's own heap values behave as objects with no own properties
//! and no extensibility (`crate::vm::property`, "Which values have
//! properties"), so they are already frozen. A published function is
//! shared and immutable (`crate::vm::function`, "Own properties"), so it
//! is frozen too. A realm intrinsic is immutable because it is shared, not
//! because of its attributes, and reports its attributes as
//! `Object.getOwnPropertyDescriptor` does (`docs/architecture.md`, "Realm
//! intrinsics"); `Object.preventExtensions` on it is the `TypeError` of a
//! `[[PreventExtensions]]` that returns false.
//!
//! **Freezing does not change ownership.** A frozen object or array stays
//! task-owned (`Shareability::TaskOwned`), so the publication check still
//! rejects it, and only its owner reads it. Freezing a function gives it a
//! property record, which makes it unpublishable like any property write.
//! A deeply frozen object is not turned into a shareable value.
//!
//! # Calls
//!
//! The four run to completion inside their call instruction
//! (`crate::vm::builtin`, "Calls"): they call no JavaScript (there are no
//! proxies yet) and allocate only a function's property record
//! and lazy `prototype`, which are reachable from the function, an operand
//! of the running frame, before the next safepoint.

use crate::heap::{Heap, HeapObject, ObjectId};
use crate::ids::TaskId;
use crate::value::Value;
use crate::vm::dispatch::Fault;
use crate::vm::function::{self, IntegrityStore};

/// Which integrity operation runs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Level {
    /// `[[PreventExtensions]]`.
    NonExtensible,
    /// SetIntegrityLevel(frozen).
    Frozen,
}

/// `Object.isExtensible(v)`.
pub(crate) fn is_extensible(heap: &Heap, task: TaskId, v: &Value) -> Result<Value, Fault> {
    query(heap, task, v, Level::NonExtensible).map(|level| Value::Bool(!level))
}

/// `Object.isFrozen(v)`.
pub(crate) fn is_frozen(heap: &Heap, task: TaskId, v: &Value) -> Result<Value, Fault> {
    query(heap, task, v, Level::Frozen).map(Value::Bool)
}

/// `Object.preventExtensions(v)`: returns `v`.
pub(crate) fn prevent_extensions(heap: &Heap, task: TaskId, v: &Value) -> Result<Value, Fault> {
    change(heap, task, v, Level::NonExtensible).map(|()| v.clone())
}

/// `Object.freeze(v)`: returns `v`.
pub(crate) fn freeze(heap: &Heap, task: TaskId, v: &Value) -> Result<Value, Fault> {
    change(heap, task, v, Level::Frozen).map(|()| v.clone())
}

fn foreign(owner: Option<TaskId>, caller: TaskId) -> Fault {
    Fault::ForeignObject { owner, caller }
}

/// True if `object` has `level`: not extensible, or (`Frozen`) frozen.
fn has_level(object: &crate::object::OrdinaryObject, level: Level) -> bool {
    match level {
        Level::NonExtensible => !object.extensible(),
        Level::Frozen => object.is_frozen(),
    }
}

/// Whether `v` has `level` (module docs, table), for the running task.
fn query(heap: &Heap, task: TaskId, v: &Value, level: Level) -> Result<bool, Fault> {
    let Value::Object(id) = *v else {
        // A primitive is not extensible and is frozen.
        return Ok(true);
    };
    let id = match function_store(heap, id)? {
        Some(IntegrityStore::Shared) => return Ok(true),
        Some(IntegrityStore::Unrecorded) => return Ok(false),
        Some(IntegrityStore::Record(record)) => record,
        None => id,
    };
    let readable = |owner: Option<TaskId>| match owner {
        Some(owner) if owner != task => Err(foreign(Some(owner), task)),
        _ => Ok(()),
    };
    if let Some(view) = crate::object::typed_array::view(heap, task, id)?
        && matches!(level, Level::Frozen)
        && view.live_length(heap)? != 0
    {
        return Ok(false);
    }
    heap.read(id, |object| match object {
        HeapObject::Plain(o) => readable(o.owner()).map(|()| has_level(o, level)),
        HeapObject::JsMap(m) => readable(m.object().owner()).map(|()| has_level(m.object(), level)),
        HeapObject::WeakMap(m) => {
            readable(m.object().owner()).map(|()| has_level(m.object(), level))
        }
        HeapObject::WeakRef(m) => {
            readable(m.object().owner()).map(|()| has_level(m.object(), level))
        }
        HeapObject::FinalizationRegistry(m) => {
            readable(m.object().owner()).map(|()| has_level(m.object(), level))
        }
        HeapObject::ArrayBuffer(b) => {
            readable(b.object.owner()).map(|()| has_level(&b.object, level))
        }
        HeapObject::Generator(b) => {
            readable(b.object.owner()).map(|()| has_level(&b.object, level))
        }
        HeapObject::TypedArray(a) => {
            readable(a.object.owner()).map(|()| has_level(&a.object, level))
        }
        HeapObject::Array(a) => readable(a.object().owner()).map(|()| match level {
            Level::NonExtensible => !a.object().extensible(),
            Level::Frozen => a.is_frozen(),
        }),
        HeapObject::Builtin(f) => Ok(has_level(&f.properties, level)),
        HeapObject::Function(_) | HeapObject::BoundFunction(_) => {
            Err(Fault::Internal("a function reached the integrity read"))
        }
        // No properties and not extensible.
        _ => Ok(true),
    })
    // The argument is an operand of the running frame.
    .unwrap_or(Err(Fault::Internal("integrity argument was reclaimed")))
}

/// Give `v` `level` (module docs, table), for the running task.
fn change(heap: &Heap, task: TaskId, v: &Value, level: Level) -> Result<(), Fault> {
    let Value::Object(id) = *v else {
        return Ok(());
    };
    let id = if is_function(heap, id) {
        match function::integrity_target(heap, task, id)? {
            IntegrityStore::Record(record) => record,
            IntegrityStore::Shared => return Ok(()),
            IntegrityStore::Unrecorded => {
                return Err(Fault::Internal("a function has no record to change"));
            }
        }
    } else {
        id
    };
    let immutable = || {
        Fault::TypeError(
            "cannot change the extensibility of a built-in object: built-in objects are shared \
             by all tasks and immutable"
                .to_string(),
        )
    };
    let writable = |owner: Option<TaskId>| match owner {
        Some(owner) if owner == task => Ok(()),
        owner => Err(foreign(owner, task)),
    };
    let nonempty_view = crate::object::typed_array::view(heap, task, id)?
        .map(|v| v.live_length(heap))
        .transpose()?
        .is_some_and(|n| n != 0);
    heap.with_object_mut(id, |object| match object {
        HeapObject::Plain(o) if o.is_intrinsic() => Err(immutable()),
        HeapObject::Builtin(_) => Err(immutable()),
        HeapObject::Plain(o) => {
            writable(o.owner())?;
            match level {
                Level::NonExtensible => o.prevent_extensions(),
                Level::Frozen => o.freeze(),
            }
            Ok(())
        }
        HeapObject::JsMap(m) => {
            writable(m.object().owner())?;
            match level {
                Level::NonExtensible => m.object_mut().prevent_extensions(),
                Level::Frozen => m.object_mut().freeze(),
            }
            Ok(())
        }
        HeapObject::WeakMap(m) => {
            writable(m.object().owner())?;
            match level {
                Level::NonExtensible => m.object_mut().prevent_extensions(),
                Level::Frozen => m.object_mut().freeze(),
            }
            Ok(())
        }
        HeapObject::WeakRef(m) => {
            writable(m.object().owner())?;
            match level {
                Level::NonExtensible => m.object_mut().prevent_extensions(),
                Level::Frozen => m.object_mut().freeze(),
            }
            Ok(())
        }
        HeapObject::FinalizationRegistry(m) => {
            writable(m.object().owner())?;
            match level {
                Level::NonExtensible => m.object_mut().prevent_extensions(),
                Level::Frozen => m.object_mut().freeze(),
            }
            Ok(())
        }
        HeapObject::Array(a) => {
            if a.object().is_intrinsic() {
                return Ok(());
            }
            writable(a.object().owner())?;
            match level {
                Level::NonExtensible => a.object_mut().prevent_extensions(),
                Level::Frozen => a.freeze(),
            }
            Ok(())
        }
        HeapObject::ArrayBuffer(b) => {
            writable(b.object.owner())?;
            match level {
                Level::NonExtensible => b.object.prevent_extensions(),
                Level::Frozen => b.object.freeze(),
            }
            Ok(())
        }
        HeapObject::Generator(b) => {
            writable(b.object.owner())?;
            match level {
                Level::NonExtensible => b.object.prevent_extensions(),
                Level::Frozen => b.object.freeze(),
            }
            Ok(())
        }
        HeapObject::TypedArray(a) => {
            writable(a.object.owner())?;
            a.object.prevent_extensions();
            if matches!(level, Level::Frozen) {
                if nonempty_view {
                    return Err(Fault::TypeError(
                        "cannot freeze a nonempty typed array".into(),
                    ));
                }
                a.object.freeze();
            }
            Ok(())
        }
        HeapObject::Function(_) | HeapObject::BoundFunction(_) => {
            Err(Fault::Internal("a function reached the integrity change"))
        }
        // Already frozen: no properties and not extensible.
        _ => Ok(()),
    })
    .unwrap_or(Err(Fault::Internal("integrity argument was reclaimed")))
}

fn is_function(heap: &Heap, id: ObjectId) -> bool {
    heap.read(id, |o| {
        matches!(o, HeapObject::Function(_) | HeapObject::BoundFunction(_))
    })
    .unwrap_or(false)
}

/// The integrity store of `id` if it is a function object.
fn function_store(heap: &Heap, id: ObjectId) -> Result<Option<IntegrityStore>, Fault> {
    if is_function(heap, id) {
        function::integrity_store(heap, id).map(Some)
    } else {
        Ok(None)
    }
}
