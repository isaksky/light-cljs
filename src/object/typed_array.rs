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
    Float16,
    Float32,
    Float64,
}

impl ElementType {
    pub(crate) const ALL: [Self; 10] = [
        Self::Int8,
        Self::Uint8,
        Self::Uint8Clamped,
        Self::Int16,
        Self::Uint16,
        Self::Int32,
        Self::Uint32,
        Self::Float16,
        Self::Float32,
        Self::Float64,
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
            Self::Float16 => "Float16Array",
            Self::Float32 => "Float32Array",
            Self::Float64 => "Float64Array",
        }
    }

    pub(crate) fn size(self) -> usize {
        match self {
            Self::Int8 | Self::Uint8 | Self::Uint8Clamped => 1,
            Self::Int16 | Self::Uint16 | Self::Float16 => 2,
            Self::Int32 | Self::Uint32 | Self::Float32 => 4,
            Self::Float64 => 8,
        }
    }
}

/// Round directly from binary64 to binary16; an intermediate f32 could round
/// a value onto a binary16 midpoint and give the wrong ties-to-even result.
fn to_binary16(number: f64) -> u16 {
    let bits = number.to_bits();
    let sign = ((bits >> 48) & 0x8000) as u16;
    let exponent = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & ((1_u64 << 52) - 1);
    if exponent == 0x7ff {
        // Store every NaN as the same positive quiet NaN.
        return if fraction != 0 { 0x7e00 } else { sign | 0x7c00 };
    }
    let exponent = exponent - 1023;
    if exponent < -25 {
        return sign;
    }
    if exponent > 15 {
        return sign | 0x7c00;
    }
    let significand = (1_u64 << 52) | fraction;
    // Subnormals have a fixed 2^-24 quantum; normals keep eleven significant
    // bits. All shifts here are in 42..=53, including the zero/subnormal tie.
    let shift = if exponent < -14 { 28 - exponent } else { 42 } as u32;
    let truncated = significand >> shift;
    let remainder = significand & ((1_u64 << shift) - 1);
    let halfway = 1_u64 << (shift - 1);
    let rounded =
        truncated + u64::from(remainder > halfway || (remainder == halfway && truncated & 1 != 0));
    let magnitude = if exponent < -14 {
        rounded as u16
    } else {
        // Addition carries into the exponent, including normal/subnormal and
        // maximum-finite/infinity boundaries.
        (((exponent + 14) as u16) << 10) + rounded as u16
    };
    sign | magnitude
}

/// Every finite binary16 value widens exactly to binary64.
fn from_binary16(bits: u16) -> f64 {
    let sign = u64::from(bits & 0x8000) << 48;
    let exponent = (bits >> 10) & 0x1f;
    let fraction = bits & 0x03ff;
    match exponent {
        0 => {
            let magnitude = f64::from(fraction) / 16777216.0; // 2^-24
            if sign != 0 { -magnitude } else { magnitude }
        }
        31 if fraction != 0 => f64::NAN,
        31 => f64::from_bits(sign | f64::INFINITY.to_bits()),
        _ => {
            f64::from_bits(sign | (u64::from(exponent + 1008) << 52) | (u64::from(fraction) << 42))
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
                    ElementType::Float16 => from_binary16(u16::from_le_bytes([bytes[0], bytes[1]])),
                    ElementType::Float32 => {
                        f64::from(f32::from_le_bytes(bytes.try_into().unwrap()))
                    }
                    ElementType::Float64 => f64::from_le_bytes(bytes.try_into().unwrap()),
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
        let mut bytes = [0; 8];
        match self.kind {
            ElementType::Float16 => bytes[..2].copy_from_slice(&to_binary16(n).to_le_bytes()),
            ElementType::Float32 => {
                let rounded = if n.is_nan() {
                    f32::from_bits(0x7fc0_0000)
                } else {
                    n as f32
                };
                bytes[..4].copy_from_slice(&rounded.to_le_bytes());
            }
            ElementType::Float64 => {
                let canonical = if n.is_nan() {
                    f64::from_bits(0x7ff8_0000_0000_0000)
                } else {
                    n
                };
                bytes.copy_from_slice(&canonical.to_le_bytes());
            }
            _ => {
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
                bytes[..4].copy_from_slice(&integer.to_le_bytes());
            }
        }
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

#[cfg(test)]
mod tests {
    use super::{from_binary16, to_binary16};

    #[test]
    fn binary16_rounding() {
        for (number, bits) in [
            (1.0, 0x3c00),
            (1.0 + 2_f64.powi(-11), 0x3c00),
            (1.0 + 3.0 * 2_f64.powi(-11), 0x3c02),
            (1.0 + 2_f64.powi(-11) + 2_f64.powi(-40), 0x3c01),
            (65504.0, 0x7bff),
            (65520.0_f64.next_down(), 0x7bff),
            (65520.0, 0x7c00),
            (f64::MAX, 0x7c00),
            (f64::INFINITY, 0x7c00),
            (2_f64.powi(-24), 0x0001),
            (2_f64.powi(-25), 0x0000),
            (2_f64.powi(-25).next_up(), 0x0001),
            (2_f64.powi(-14) - 2_f64.powi(-25), 0x0400),
            ((2_f64.powi(-14) - 2_f64.powi(-25)).next_down(), 0x03ff),
            (0.1, 0x2e66),
            (0.0, 0x0000),
            (f64::from_bits(1), 0x0000),
        ] {
            assert_eq!(to_binary16(number), bits, "{number:?}");
            assert_eq!(to_binary16(-number), bits | 0x8000, "-{number:?}");
        }
        for bits in [
            0x7ff0_0000_0000_0001,
            0x7ff8_0000_0000_0000,
            0xfff8_1234_5678_9abc,
        ] {
            assert_eq!(to_binary16(f64::from_bits(bits)), 0x7e00);
        }
        assert_eq!(from_binary16(0x8000).to_bits(), (-0.0_f64).to_bits());
        assert_eq!(from_binary16(0x0001), 2_f64.powi(-24));
        assert_eq!(from_binary16(0x2e66), 0.0999755859375);
    }

    #[test]
    fn binary16_encodings_and_midpoints() {
        for bits in 0..=u16::MAX {
            let number = from_binary16(bits);
            if bits & 0x7fff > 0x7c00 {
                assert!(number.is_nan());
                assert_eq!(to_binary16(number), 0x7e00);
            } else {
                assert_eq!(to_binary16(number), bits);
            }
        }
        for lower in 0..0x7bff {
            let midpoint = (from_binary16(lower) + from_binary16(lower + 1)) / 2.0;
            assert_eq!(to_binary16(midpoint), lower + (lower & 1));
            assert_eq!(to_binary16(midpoint.next_down()), lower);
            assert_eq!(to_binary16(midpoint.next_up()), lower + 1);
            assert_eq!(to_binary16(-midpoint), 0x8000 | (lower + (lower & 1)));
        }
    }
}
