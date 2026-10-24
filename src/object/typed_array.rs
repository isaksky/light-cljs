//! Task-owned integer-indexed views. Byte order is always little-endian.

use super::{OrdinaryObject, array_buffer};
use crate::heap::gc::Trace;
use crate::heap::{Heap, HeapObject, ObjectId};
use crate::ids::TaskId;
use crate::value::Value;
use crate::vm::dispatch::{self, Fault};
use crate::vm::property::Key;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ElementType {
    Int8,
    Uint8,
    Uint8Clamped,
    Int16,
    Uint16,
    Int32,
    Uint32,
}

impl ElementType {
    pub(crate) const ALL: [Self; 7] = [
        Self::Int8,
        Self::Uint8,
        Self::Uint8Clamped,
        Self::Int16,
        Self::Uint16,
        Self::Int32,
        Self::Uint32,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Int8 => "Int8Array",
            Self::Uint8 => "Uint8Array",
            Self::Uint8Clamped => "Uint8ClampedArray",
            Self::Int16 => "Int16Array",
            Self::Uint16 => "Uint16Array",
            Self::Int32 => "Int32Array",
            Self::Uint32 => "Uint32Array",
        }
    }

    pub(crate) fn size(self) -> usize {
        match self {
            Self::Int8 | Self::Uint8 | Self::Uint8Clamped => 1,
            Self::Int16 | Self::Uint16 => 2,
            Self::Int32 | Self::Uint32 => 4,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct View {
    pub(crate) kind: ElementType,
    pub(crate) buffer: ObjectId,
    pub(crate) offset: usize,
    pub(crate) length: usize,
}

#[derive(Debug)]
pub(crate) struct TypedArray {
    pub(crate) object: OrdinaryObject,
    pub(crate) view: View,
}

impl Trace for TypedArray {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        self.object.trace(visit);
        visit(self.view.buffer);
    }
}

pub(crate) fn view(heap: &Heap, task: TaskId, id: ObjectId) -> Result<Option<View>, Fault> {
    heap.read(id, |o| match o {
        HeapObject::TypedArray(a) if a.object.owner() == Some(task) => Ok(Some(a.view)),
        HeapObject::TypedArray(a) => Err(Fault::ForeignObject {
            owner: a.object.owner(),
            caller: task,
        }),
        _ => Ok(None),
    })
    .unwrap_or(Err(Fault::Internal("typed array was reclaimed")))
}

pub(crate) fn is_buffer(heap: &Heap, value: &Value) -> bool {
    matches!(value, Value::Object(id) if heap.read(*id, |o| matches!(o, HeapObject::ArrayBuffer(_))).unwrap_or(false))
}

pub(crate) fn buffer_length(heap: &Heap, task: TaskId, value: &Value) -> Result<usize, Fault> {
    let id = array_buffer::receiver(heap, task, value)?;
    heap.read(id, |o| match o {
        HeapObject::ArrayBuffer(b) if !b.detached => Ok(b.bytes.len()),
        _ => Err(Fault::TypeError("ArrayBuffer is detached".into())),
    })
    .unwrap_or(Err(Fault::Internal("buffer was reclaimed")))
}

pub(crate) fn validate_offset(kind: ElementType, value: &Value) -> Result<usize, Fault> {
    let offset = to_index(value)?;
    if !offset.is_multiple_of(kind.size()) {
        return Err(Fault::RangeError("unaligned typed array byteOffset".into()));
    }
    Ok(offset)
}

pub(crate) fn to_number(value: &Value) -> Result<f64, Fault> {
    if matches!(value, Value::BigInt(_)) {
        return Err(Fault::TypeError(
            "cannot convert BigInt to a typed array Number".into(),
        ));
    }
    dispatch::to_number(value)
}

pub(crate) fn to_index(value: &Value) -> Result<usize, Fault> {
    let number = to_number(value)?;
    let index = if number.is_nan() { 0.0 } else { number.trunc() };
    if !(0.0..=9007199254740991.0).contains(&index) {
        return Err(Fault::RangeError("invalid typed array index".into()));
    }
    usize::try_from(index as u64)
        .map_err(|_| Fault::RangeError("typed array index exceeds platform limit".into()))
}

/// Arguments have completed resumable ToIndex conversion. Non-buffer object
/// construction (iterables, array-like objects and copies) is a separate feature.
pub(crate) fn constructor(
    heap: &Heap,
    task: TaskId,
    kind: ElementType,
    args: &[Value],
    from_buffer: bool,
) -> Result<Value, Fault> {
    let undefined = Value::Undefined;
    let (buffer, offset, length) = if from_buffer {
        let offset = validate_offset(kind, args.get(1).unwrap_or(&undefined))?;
        let requested = args
            .get(2)
            .filter(|v| !matches!(v, Value::Undefined))
            .map(to_index)
            .transpose()?;
        let bytes = buffer_length(heap, task, &args[0])?;
        if offset > bytes {
            return Err(Fault::RangeError(
                "typed array offset exceeds buffer".into(),
            ));
        }
        let length = if let Some(length) = requested {
            if length > (bytes - offset) / kind.size() {
                return Err(Fault::RangeError("typed array exceeds buffer".into()));
            }
            length
        } else {
            if !(bytes - offset).is_multiple_of(kind.size()) {
                return Err(Fault::RangeError("unaligned typed array byteLength".into()));
            }
            (bytes - offset) / kind.size()
        };
        (args[0].clone(), offset, length)
    } else {
        let length = to_index(&args[0])?;
        let bytes = length
            .checked_mul(kind.size())
            .ok_or_else(|| Fault::RangeError("typed array length overflow".into()))?;
        let buffer = array_buffer::constructor(heap, task, &Value::Number(bytes as f64))?;
        (buffer, 0, length)
    };
    let Value::Object(buffer) = buffer else {
        return Err(Fault::Internal("buffer constructor returned a primitive"));
    };
    let prototype = crate::realm::intrinsics(heap)
        .ok_or(Fault::OutOfMemory("typed array realm"))?
        .typed_array_prototypes[kind as usize];
    heap.alloc(HeapObject::TypedArray(Box::new(TypedArray {
        object: OrdinaryObject::with_prototype(task, prototype),
        view: View {
            kind,
            buffer,
            offset,
            length,
        },
    })))
    .map(Value::Object)
    .ok_or(Fault::OutOfMemory("typed array allocation"))
}

/// CanonicalNumericIndexString. Noncanonical spellings remain ordinary keys.
pub(crate) fn numeric_key(key: &Key) -> Option<f64> {
    match key {
        Key::Index(i) => Some(f64::from(*i)),
        Key::Name(s) if *s == "-0" => Some(-0.0),
        Key::Name(s) => {
            let n = dispatch::to_number(&Value::String(s.clone())).ok()?;
            (dispatch::to_js_string(&Value::Number(n)).ok()? == *s).then_some(n)
        }
        Key::Symbol(_) => None,
    }
}

impl View {
    fn is_detached(self, heap: &Heap) -> Result<bool, Fault> {
        heap.read(self.buffer, |o| match o {
            HeapObject::ArrayBuffer(b) => Ok(b.detached),
            _ => Err(Fault::Internal("typed array buffer changed kind")),
        })
        .unwrap_or(Err(Fault::Internal("typed array buffer was reclaimed")))
    }

    pub(crate) fn live_length(self, heap: &Heap) -> Result<usize, Fault> {
        Ok(if self.is_detached(heap)? {
            0
        } else {
            self.length
        })
    }

    fn position(self, n: f64) -> Option<usize> {
        if !n.is_finite()
            || n < 0.0
            || (n == 0.0 && n.is_sign_negative())
            || n.fract() != 0.0
            || n >= self.length as f64
        {
            None
        } else {
            Some(self.offset + n as usize * self.kind.size())
        }
    }

    pub(crate) fn get(self, heap: &Heap, n: f64) -> Result<Option<Value>, Fault> {
        let Some(at) = self.position(n) else {
            return Ok(None);
        };
        heap.read(self.buffer, |o| match o {
            HeapObject::ArrayBuffer(b) if !b.detached => {
                let bytes = &b.bytes[at..at + self.kind.size()];
                let n = match self.kind {
                    ElementType::Int8 => f64::from(bytes[0] as i8),
                    ElementType::Uint8 | ElementType::Uint8Clamped => f64::from(bytes[0]),
                    ElementType::Int16 => f64::from(i16::from_le_bytes([bytes[0], bytes[1]])),
                    ElementType::Uint16 => f64::from(u16::from_le_bytes([bytes[0], bytes[1]])),
                    ElementType::Int32 => f64::from(i32::from_le_bytes(bytes.try_into().unwrap())),
                    ElementType::Uint32 => f64::from(u32::from_le_bytes(bytes.try_into().unwrap())),
                };
                Ok(Some(Value::Number(n)))
            }
            HeapObject::ArrayBuffer(_) => Ok(None),
            _ => Err(Fault::Internal("typed array buffer changed kind")),
        })
        .unwrap_or(Err(Fault::Internal("typed array buffer was reclaimed")))
    }

    /// ToNumber happens even for invalid indices and detached buffers.
    pub(crate) fn set(self, heap: &Heap, index: f64, value: &Value) -> Result<(), Fault> {
        let n = to_number(value)?;
        let Some(at) = self.position(index) else {
            return Ok(());
        };
        let integer = if self.kind == ElementType::Uint8Clamped {
            if n.is_nan() {
                0
            } else {
                n.clamp(0.0, 255.0).round_ties_even() as u32
            }
        } else if !n.is_finite() || n == 0.0 {
            0
        } else {
            n.trunc().rem_euclid(4294967296.0) as u32
        };
        let bytes = integer.to_le_bytes();
        heap.with_object_mut(self.buffer, |o| match o {
            HeapObject::ArrayBuffer(b) if !b.detached => {
                b.bytes[at..at + self.kind.size()].copy_from_slice(&bytes[..self.kind.size()]);
                Ok(())
            }
            HeapObject::ArrayBuffer(_) => Ok(()),
            _ => Err(Fault::Internal("typed array buffer changed kind")),
        })
        .unwrap_or(Err(Fault::Internal("typed array buffer was reclaimed")))
    }
}

pub(crate) fn inspect(
    heap: &Heap,
    task: TaskId,
    this: &Value,
    which: crate::vm::builtin::Builtin,
) -> Result<Value, Fault> {
    use crate::vm::builtin::Builtin;
    let Value::Object(id) = this else {
        return Err(Fault::TypeError(
            "typed array getter requires a typed array".into(),
        ));
    };
    let view = view(heap, task, *id)?
        .ok_or_else(|| Fault::TypeError("typed array getter requires a typed array".into()))?;
    if which == Builtin::TypedArrayBuffer {
        return Ok(Value::Object(view.buffer));
    }
    let detached = view.is_detached(heap)?;
    let length = if detached { 0 } else { view.length };
    Ok(Value::Number(match which {
        Builtin::TypedArrayLength => length,
        Builtin::TypedArrayByteLength => length * view.kind.size(),
        Builtin::TypedArrayByteOffset => {
            if detached {
                0
            } else {
                view.offset
            }
        }
        _ => return Err(Fault::Internal("unknown typed array getter")),
    } as f64))
}
