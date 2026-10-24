//! Task-owned ArrayBuffer storage (clj-quickjs-165). Views must retain the
//! buffer's ObjectId and consult this state on every access, never copy it.

use super::OrdinaryObject;
use crate::heap::gc::Trace;
use crate::heap::{Heap, HeapObject, ObjectId};
use crate::ids::TaskId;
use crate::value::Value;
use crate::vm::dispatch::{self, Fault};

/// Per-buffer bound, checked before allocating or touching memory (16 MiB).
pub(crate) const MAX_BYTE_LENGTH: usize = 16 * 1024 * 1024;

/// Bytes have no heap edges. The ordinary property store traces its own
/// prototype and property edges; detachment preserves those properties.
#[derive(Debug)]
pub(crate) struct ArrayBuffer {
    pub(crate) object: OrdinaryObject,
    pub(crate) bytes: Vec<u8>,
    pub(crate) detached: bool,
}

impl Trace for ArrayBuffer {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        self.object.trace(visit);
    }
}

fn invalid_length() -> Fault {
    Fault::RangeError("invalid ArrayBuffer length (maximum 16777216 bytes)".into())
}

/// ToIndex, including truncation toward zero before checking the bound.
pub(crate) fn to_index(value: &Value) -> Result<usize, Fault> {
    let number = dispatch::to_number(value)?;
    let index = if number.is_nan() { 0.0 } else { number.trunc() };
    if !(0.0..=MAX_BYTE_LENGTH as f64).contains(&index) {
        return Err(invalid_length());
    }
    Ok(index as usize)
}

fn zeroed(length: usize) -> Result<Vec<u8>, Fault> {
    if length > MAX_BYTE_LENGTH {
        return Err(invalid_length());
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| invalid_length())?;
    bytes.resize(length, 0);
    Ok(bytes)
}

fn allocate(heap: &Heap, task: TaskId, bytes: Vec<u8>) -> Result<Value, Fault> {
    let prototype = crate::realm::intrinsics(heap)
        .ok_or_else(invalid_length)?
        .array_buffer_prototype;
    heap.alloc(HeapObject::ArrayBuffer(Box::new(ArrayBuffer {
        object: OrdinaryObject::with_prototype(task, prototype),
        bytes,
        detached: false,
    })))
    .map(Value::Object)
    .ok_or_else(invalid_length)
}

pub(crate) fn constructor(heap: &Heap, task: TaskId, length: &Value) -> Result<Value, Fault> {
    allocate(heap, task, zeroed(to_index(length)?)?)
}

/// Brand/ownership check precedes any observable argument coercion.
pub(crate) fn receiver(heap: &Heap, task: TaskId, value: &Value) -> Result<ObjectId, Fault> {
    let bad = || Fault::TypeError("ArrayBuffer method requires an ArrayBuffer receiver".into());
    let Value::Object(id) = value else {
        return Err(bad());
    };
    heap.read(*id, |o| match o {
        HeapObject::ArrayBuffer(b) if b.object.owner() == Some(task) => Ok(*id),
        HeapObject::ArrayBuffer(b) => Err(Fault::ForeignObject {
            owner: b.object.owner(),
            caller: task,
        }),
        _ => Err(bad()),
    })
    .unwrap_or(Err(Fault::Internal("ArrayBuffer was reclaimed")))
}

pub(crate) fn inspect(
    heap: &Heap,
    task: TaskId,
    value: &Value,
    detached: bool,
) -> Result<Value, Fault> {
    let id = receiver(heap, task, value)?;
    heap.read(id, |o| match o {
        HeapObject::ArrayBuffer(b) if detached => Ok(Value::Bool(b.detached)),
        HeapObject::ArrayBuffer(b) => Ok(Value::Number(b.bytes.len() as f64)),
        _ => Err(Fault::Internal("ArrayBuffer changed kind")),
    })
    .unwrap_or(Err(Fault::Internal("ArrayBuffer was reclaimed")))
}

fn attached(b: &ArrayBuffer) -> Result<(), Fault> {
    if b.detached {
        Err(Fault::TypeError("ArrayBuffer is detached".into()))
    } else {
        Ok(())
    }
}

/// Called after resumable argument conversion. Allocate the destination
/// before detaching so a failed allocation leaves the receiver intact.
pub(crate) fn transfer(
    heap: &Heap,
    task: TaskId,
    value: &Value,
    length: &Value,
) -> Result<Value, Fault> {
    let id = receiver(heap, task, value)?;
    let length = if matches!(length, Value::Undefined) {
        None
    } else {
        Some(to_index(length)?)
    };
    let bytes = heap
        .read(id, |o| match o {
            HeapObject::ArrayBuffer(b) => {
                attached(b)?;
                let mut bytes = zeroed(length.unwrap_or(b.bytes.len()))?;
                let copied = bytes.len().min(b.bytes.len());
                bytes[..copied].copy_from_slice(&b.bytes[..copied]);
                Ok(bytes)
            }
            _ => Err(Fault::Internal("ArrayBuffer changed kind")),
        })
        .unwrap_or(Err(Fault::Internal("ArrayBuffer was reclaimed")))?;
    let result = allocate(heap, task, bytes)?;
    heap.with_object_mut(id, |o| {
        if let HeapObject::ArrayBuffer(b) = o {
            b.bytes = Vec::new();
            b.detached = true;
        }
    })
    .ok_or(Fault::Internal("ArrayBuffer was reclaimed"))?;
    Ok(result)
}

pub(crate) fn slice(
    heap: &Heap,
    task: TaskId,
    value: &Value,
    start: &Value,
    end: &Value,
) -> Result<Value, Fault> {
    let id = receiver(heap, task, value)?;
    let start = dispatch::to_number(start)?;
    let end = if matches!(end, Value::Undefined) {
        None
    } else {
        Some(dispatch::to_number(end)?)
    };
    // Custom species constructors need a separate resumable construction
    // protocol. Reject them explicitly instead of silently ignoring them.
    let constructor = crate::vm::property::get(
        heap,
        task,
        value,
        &crate::vm::property::Key::Name("constructor".into()),
    )?;
    let intrinsic = crate::realm::intrinsics(heap)
        .ok_or_else(invalid_length)?
        .global(crate::realm::Global::ArrayBuffer);
    if !matches!(constructor, Value::Undefined)
        && !crate::atom::same_value(&constructor, &intrinsic)
    {
        return Err(Fault::Unsupported(
            "custom ArrayBuffer slice constructors are not supported".into(),
        ));
    }
    let bytes = heap
        .read(id, |o| match o {
            HeapObject::ArrayBuffer(b) => {
                attached(b)?;
                let relative = |n: f64| {
                    if n.is_nan() {
                        0
                    } else if n < 0.0 {
                        (b.bytes.len() as f64 + n.trunc()).max(0.0) as usize
                    } else {
                        n.trunc().min(b.bytes.len() as f64) as usize
                    }
                };
                let first = relative(start);
                let last = end.map_or(b.bytes.len(), relative).max(first);
                let mut bytes = zeroed(last - first)?;
                bytes.copy_from_slice(&b.bytes[first..last]);
                Ok(bytes)
            }
            _ => Err(Fault::Internal("ArrayBuffer changed kind")),
        })
        .unwrap_or(Err(Fault::Internal("ArrayBuffer was reclaimed")))?;
    allocate(heap, task, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Scope;
    use std::sync::Arc;

    #[test]
    fn byte_contents_transfer_slice_and_accounting() {
        let heap = Arc::new(Heap::default());
        let _scope = Scope::new(&heap);
        let task = TaskId::from_raw(1);
        let buffer = constructor(&heap, task, &Value::Number(4.0))
            .ok()
            .expect("constructor");
        let id = receiver(&heap, task, &buffer).ok().expect("receiver");
        heap.with_object_mut(id, |o| {
            let HeapObject::ArrayBuffer(b) = o else {
                panic!("buffer")
            };
            assert_eq!(b.bytes, [0, 0, 0, 0]);
            b.bytes.copy_from_slice(&[1, 2, 3, 4]);
        })
        .unwrap();
        let slice = slice(
            &heap,
            task,
            &buffer,
            &Value::Number(1.0),
            &Value::Number(3.0),
        )
        .ok()
        .expect("slice");
        let moved = transfer(&heap, task, &buffer, &Value::Number(6.0))
            .ok()
            .expect("transfer");
        let contents = |value: &Value| {
            heap.read(receiver(&heap, task, value).ok().expect("receiver"), |o| {
                let HeapObject::ArrayBuffer(b) = o else {
                    panic!("buffer")
                };
                b.bytes.clone()
            })
            .unwrap()
        };
        assert_eq!(contents(&slice), [2, 3]);
        assert_eq!(contents(&moved), [1, 2, 3, 4, 0, 0]);
        assert!(contents(&buffer).is_empty());
        assert_eq!(heap.usage().array_buffer_bytes, 8);
        let short = transfer(&heap, task, &moved, &Value::Number(2.0))
            .ok()
            .expect("transfer");
        assert_eq!(contents(&short), [1, 2]);
        assert_eq!(heap.usage().array_buffer_bytes, 4);
        assert!(matches!(
            transfer(&heap, task, &moved, &Value::Undefined),
            Err(Fault::TypeError(_))
        ));
    }
}
