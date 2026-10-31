//! Sealing boot-time state for sharing (light-cljs-646): `__rt.sealBindings`
//! and `__rt.seal`.
//!
//! A program that builds a runtime in one task and shares it with others
//! (the real cljs.core booted once, `docs/real-core-design.md`) leaves two
//! kinds of state behind that never change again but that the publication
//! check (`crate::value::share`) cannot prove immutable on its own:
//!
//! * **Bindings reassigned while loading** (`seq = wrap(seq)` at the top
//!   level of the factory). The compiler gives a binding that only its own
//!   function's code assigns a *sealable* cell (`NewSealableCell`;
//!   `crate::vm::function`, [`CellMode`]). `__rt.sealBindings(v)` seals
//!   every initialized sealable cell of the running task reachable from `v`
//!   and evaluates to how many it sealed. A sealed cell is persistent and
//!   readable by every task; assigning it throws a `TypeError`. Cells that a
//!   nested function assigns (a counter, a cache) are mutable and stay
//!   task-owned: the publication check still rejects them with their path.
//!   A binding that nothing assigns needs no seal: its cell is *fixed*, and
//!   immutable once initialized (`NewFixedCell`).
//! * **Registries and prototypes complete after boot** (an object of hook
//!   functions, a sentinel, an array of method keys, a constructor's
//!   `prototype`). `__rt.seal(o)` seals the one object `o` in place and
//!   evaluates to it: an ordinary object or array is frozen and becomes
//!   shared and immutable, as a realm intrinsic is (`crate::object`,
//!   [`OrdinaryObject::seal`]); a function's own-property record and a
//!   concise method's home object are sealed the same way (a function
//!   without either is returned unchanged: it already publishes). Identity is preserved, so code that captured the object
//!   keeps reading it. Sealing is shallow: the values the object holds are
//!   not sealed, and publication still rejects one that is task-owned, with
//!   its path. Writes, deletions and prototype changes of a sealed object
//!   throw a `TypeError` in every task.
//!
//! Only the running task's own state is sealed: `sealBindings` passes over
//! other tasks' cells, and `seal` of another task's object is the
//! `ForeignObject` error. JavaScript Maps and Sets, WeakMaps and other
//! exotic objects do not seal (a `TypeError`); the real core's Maps hold
//! dispatch state that changes after boot (light-cljs-649).
//!
//! Both operations run to completion inside their instruction: they call no
//! JavaScript and allocate nothing on the heap. `sealBindings` reads each
//! reachable object once under its own access word, so its cost is the
//! size of the graph it walks.

use std::collections::{HashSet, VecDeque};

use crate::collections::object_name;
use crate::heap::{Heap, HeapObject, ObjectId};
use crate::ids::TaskId;
use crate::value::Value;
use crate::vm::dispatch::Fault;
use crate::vm::function::CellMode;

#[cfg(doc)]
use crate::object::OrdinaryObject;

/// `__rt.sealBindings(value)`: seal every initialized sealable binding cell
/// of `task` reachable from `value`; the number sealed.
pub(crate) fn seal_bindings(heap: &Heap, task: TaskId, value: &Value) -> Result<usize, Fault> {
    let Value::Object(root) = *value else {
        return Ok(0);
    };
    let mut seen = HashSet::from([root]);
    let mut queue = VecDeque::from([root]);
    let mut cells = Vec::new();
    while let Some(id) = queue.pop_front() {
        let edges = heap.read(id, |object| {
            if let HeapObject::Cell(c) = object
                && c.owner == task
                && c.mode == CellMode::Sealable
                && c.value.is_some()
            {
                cells.push(id);
            }
            let mut edges = Vec::new();
            crate::heap::gc::Trace::trace(object, &mut |edge| {
                if let crate::heap::gc::TraceEdge::Object(child) = edge {
                    edges.push(child);
                }
            });
            edges
        });
        for child in edges.into_iter().flatten() {
            if seen.insert(child) {
                queue.push_back(child);
            }
        }
    }
    let mut sealed = 0;
    for id in cells {
        // Only this task assigns its cells, and it is running this
        // instruction: the mode read above still holds.
        let done = heap.with_object_mut(id, |object| match object {
            HeapObject::Cell(c) if c.mode == CellMode::Sealable => {
                c.mode = CellMode::Sealed;
                true
            }
            _ => false,
        });
        sealed += usize::from(done == Some(true));
    }
    Ok(sealed)
}

/// `__rt.seal(value)`: seal one ordinary object, array or function's
/// own-property record of `task` for sharing; evaluates to `value`.
pub(crate) fn seal(heap: &Heap, task: TaskId, value: &Value) -> Result<Value, Fault> {
    let Value::Object(id) = *value else {
        return Ok(value.clone());
    };
    seal_object(heap, task, id)?;
    Ok(value.clone())
}

/// Seal `id`. A function is sealed through its two task-owned edges: its
/// own-property record and a concise method's home object (an object, or
/// for a static class method its class, a function, sealed the same way;
/// home objects never form a cycle through functions). A function with
/// neither already publishes; a known-shareable one can no longer gain a
/// record.
fn seal_object(heap: &Heap, task: TaskId, id: ObjectId) -> Result<(), Fault> {
    let parts = heap
        .read(id, |object| match object {
            HeapObject::Function(f) => Some((f.properties, f.home_object)),
            HeapObject::BoundFunction(f) => Some((f.properties, None)),
            _ => None,
        })
        .ok_or(Fault::Internal("sealed value was reclaimed"))?;
    let Some((record, home)) = parts else {
        return seal_one(heap, task, id);
    };
    if let Some(record) = record {
        seal_one(heap, task, record)?;
    }
    if let Some(home) = home {
        seal_object(heap, task, home)?;
    }
    Ok(())
}

/// Seal the one ordinary object or array `target` (module docs).
fn seal_one(heap: &Heap, task: TaskId, target: ObjectId) -> Result<(), Fault> {
    let unsupported = |kind: &str| {
        Fault::TypeError(format!(
            "cannot seal {kind}: only ordinary objects, arrays and functions are sealed for sharing"
        ))
    };
    let foreign = |owner: Option<TaskId>| Fault::ForeignObject {
        owner,
        caller: task,
    };
    heap.with_object_mut(target, |object| match object {
        HeapObject::Plain(o) if o.is_intrinsic() => Ok(()),
        HeapObject::Plain(o) if o.owner() != Some(task) => Err(foreign(o.owner())),
        HeapObject::Plain(o) => {
            if o.seal() {
                Ok(())
            } else {
                Err(unsupported("an object with internal slots"))
            }
        }
        HeapObject::Array(a) if a.object().is_intrinsic() => Ok(()),
        HeapObject::Array(a) if a.object().owner() != Some(task) => {
            Err(foreign(a.object().owner()))
        }
        HeapObject::Array(a) => {
            a.freeze();
            if a.object_mut().seal() {
                Ok(())
            } else {
                Err(unsupported("an array with internal slots"))
            }
        }
        // Already immutable and shared: persistent values and built-ins.
        HeapObject::Vector(_)
        | HeapObject::Map(_)
        | HeapObject::Occurrence(_)
        | HeapObject::Form(_)
        | HeapObject::Builtin(_) => Ok(()),
        other => Err(unsupported(&format!("a {}", object_name(other)))),
    })
    .unwrap_or(Err(Fault::Internal("sealed value was reclaimed")))
}
