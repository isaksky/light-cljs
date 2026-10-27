//! Bounded builtin calls borrowing the caller's rooted operands. A miss has
//! no observable effects and enters the usual resumable native instead.
use crate::heap::{Heap, HeapObject, ObjectId};
use crate::ids::TaskId;
use crate::object::{DefineOutcome, DeleteOutcome, PropertyDescriptor, SetOutcome};
use crate::value::Value;
use crate::vm::builtin::{self, Builtin};
use crate::vm::dispatch::Fault;
use crate::vm::property;

// Longer scans and argument lists retain native budget/cancellation points.
const LIMIT: usize = 64;

pub(crate) fn try_builtin_leaf(
    heap: &Heap,
    task: TaskId,
    which: Builtin,
    this: &Value,
    args: &[Value],
) -> Result<Option<Value>, Fault> {
    use Builtin::*;
    match which {
        ArrayPrototypePush | ArrayPrototypePop => mutate(heap, task, which, this, args),
        ArrayPrototypeAt
        | ArrayPrototypeIndexOf
        | ArrayPrototypeLastIndexOf
        | ArrayPrototypeIncludes => search(heap, task, which, this, args),
        ArrayPrototypeSlice => slice(heap, task, this, args),
        // These methods never coerce an operand or invoke JavaScript. Keep
        // forwarding, constructors and state-dependent leaves on their own
        // dispatch path. Result allocations (e.g. iterator objects) remain.
        ArrayIsArray
        | ObjectIs
        | NumberIsNaN
        | NumberIsFinite
        | NumberIsInteger
        | NumberIsSafeInteger
        | NumberPrototypeValueOf
        | StringPrototypeValueOf
        | StringPrototypeToString
        | BooleanPrototypeValueOf
        | BooleanPrototypeToString
        | BigIntPrototypeValueOf
        | ArraySpecies
        | MapSet
        | MapGet
        | MapHas
        | MapDelete
        | MapClear
        | MapSize
        | MapKeys
        | MapValues
        | MapEntries
        | WeakMapSet
        | WeakMapGet
        | WeakMapHas
        | WeakMapDelete
        | SetAdd
        | SetHas
        | SetDelete
        | SetClear
        | SetSize
        | SetValues
        | SetEntries
        | WeakSetAdd
        | WeakSetHas
        | WeakSetDelete => builtin::call(heap, task, which, this, args).map(Some),
        _ => Ok(None),
    }
}

fn simple_array(
    prototype: Option<ObjectId>,
    task: TaskId,
    array: &crate::object::array::ArrayObject,
) -> bool {
    array.object().owner() == Some(task)
        && (array.object().prototype().is_none() || array.object().prototype() == prototype)
}

fn mutate(
    heap: &Heap,
    task: TaskId,
    which: Builtin,
    this: &Value,
    args: &[Value],
) -> Result<Option<Value>, Fault> {
    let Value::Object(id) = this else {
        return Ok(None);
    };
    // with_object_mut must not copy frozen or foreign objects just to probe
    // a leaf. Check task ownership through a read first.
    let prototype = heap.realm().get().map(|r| r.array_prototype);
    if !heap
        .read(
            *id,
            |o| matches!(o, HeapObject::Array(a) if simple_array(prototype, task, a)),
        )
        .unwrap_or(false)
    {
        return Ok(None);
    }
    heap.with_object_mut(*id, |o| {
        let HeapObject::Array(array) = o else {
            return Err(Fault::Internal("array receiver changed kind"));
        };
        if !array.length_writable() {
            return Ok(None);
        }
        let length = array.length();
        if which == Builtin::ArrayPrototypePush {
            if args.len() > LIMIT
                || args.len() > (u32::MAX - length) as usize
                || (!args.is_empty() && !array.object().extensible())
            {
                return Ok(None);
            }
            // Appended indices cannot have own descriptors. The only allowed
            // prototypes are null or the immutable, index-free realm chain.
            for value in args {
                match array.add(array.length(), value.clone()) {
                    SetOutcome::Done => {}
                    SetOutcome::HeapLimit => return Err(Fault::HeapLimit),
                    _ => return Err(Fault::Internal("plain array append was rejected")),
                }
            }
            return Ok(Some(Value::Number(f64::from(array.length()))));
        }
        let value = if length == 0 {
            Value::Undefined
        } else {
            match array.element(length - 1) {
                Some(property) if property.accessor.is_some() || !property.flags.configurable => {
                    return Ok(None);
                }
                Some(property) => property.value,
                None => Value::Undefined,
            }
        };
        if length > 0 {
            match array.delete_element(length - 1) {
                DeleteOutcome::Deleted | DeleteOutcome::Absent => {}
                _ => return Err(Fault::Internal("plain array pop was rejected")),
            }
        }
        let outcome = array.define_length(&PropertyDescriptor {
            value: Some(Value::Number(f64::from(length.saturating_sub(1)))),
            ..Default::default()
        });
        match outcome {
            DefineOutcome::Done => Ok(Some(value)),
            _ => Err(Fault::Internal("plain array length was rejected")),
        }
    })
    .unwrap_or(Err(Fault::Internal("array receiver was reclaimed")))
}

// Only numeric/undefined indices take the leaf; all other inputs preserve
// their existing conversion and exception ordering in the resumable native.
fn integer(value: Option<&Value>, default: f64) -> Option<f64> {
    match value {
        None | Some(Value::Undefined) => Some(default),
        Some(Value::Number(n)) => Some(if n.is_nan() { 0.0 } else { n.trunc() }),
        _ => None,
    }
}

fn search(
    heap: &Heap,
    task: TaskId,
    which: Builtin,
    this: &Value,
    args: &[Value],
) -> Result<Option<Value>, Fault> {
    let Value::Object(id) = this else {
        return Ok(None);
    };
    let prototype = heap.realm().get().map(|r| r.array_prototype);
    heap.read(*id, |o| {
        let HeapObject::Array(array) = o else {
            return None;
        };
        if !simple_array(prototype, task, array) {
            return None;
        }
        let length = f64::from(array.length());
        if which == Builtin::ArrayPrototypeAt {
            let n = integer(args.first(), 0.0)?;
            let index = if n < 0.0 { length + n } else { n };
            return if index >= 0.0 && index < length {
                match array.element(index as u32) {
                    Some(p) if p.accessor.is_some() => None,
                    Some(p) => Some(p.value),
                    None => Some(Value::Undefined),
                }
            } else {
                Some(Value::Undefined)
            };
        }
        if array.length() as usize > LIMIT {
            return None;
        }
        let backward = which == Builtin::ArrayPrototypeLastIndexOf;
        let includes = which == Builtin::ArrayPrototypeIncludes;
        let n = integer(args.get(1), if backward && args.len() < 2 { length - 1.0 } else { 0.0 })?;
        let start = if n < 0.0 { length + n } else { n };
        let mut index = if backward { start.min(length - 1.0) } else { start.max(0.0) };
        let needle = args.first().unwrap_or(&Value::Undefined);
        while index >= 0.0 && index < length {
            let value = match array.element(index as u32) {
                Some(p) if p.accessor.is_some() => return None,
                Some(p) => Some(p.value),
                None if includes => Some(Value::Undefined),
                None => None,
            };
            if let Some(value) = value
                && (value.strict_equals(needle)
                    || (includes && matches!((&value, needle), (Value::Number(a), Value::Number(b)) if a.is_nan() && b.is_nan())))
            {
                return Some(if includes { Value::Bool(true) } else { Value::Number(index) });
            }
            index += if backward { -1.0 } else { 1.0 };
        }
        Some(if includes { Value::Bool(false) } else { Value::Number(-1.0) })
    })
    .map(Ok)
    .unwrap_or(Err(Fault::Internal("array receiver was reclaimed")))
}

fn slice(heap: &Heap, task: TaskId, this: &Value, args: &[Value]) -> Result<Option<Value>, Fault> {
    let Value::Object(id) = this else {
        return Ok(None);
    };
    let prototype = heap.realm().get().map(|r| r.array_prototype);
    let bounds = heap
        .read(*id, |o| {
            let HeapObject::Array(array) = o else {
                return None;
            };
            // Any own named property could be a constructor/species override.
            if !simple_array(prototype, task, array) || array.object().properties().next().is_some()
            {
                return None;
            }
            let length = f64::from(array.length());
            let relative = |n: f64| {
                if n < 0.0 {
                    (length + n).max(0.0)
                } else {
                    n.min(length)
                }
            };
            let start = relative(integer(args.first(), 0.0)?) as u32;
            let end = relative(integer(args.get(1), length)?) as u32;
            if end.saturating_sub(start) as usize > LIMIT
                || (start..end).any(|i| array.element(i).is_some_and(|p| p.accessor.is_some()))
            {
                return None;
            }
            Some((start, end))
        })
        .ok_or(Fault::Internal("array receiver was reclaimed"))?;
    let Some((start, end)) = bounds else {
        return Ok(None);
    };
    // No user code or safepoint runs between validation and copying. Only
    // result storage is allocated, with the normal managed-byte admission.
    let result = property::new_array(heap, task)?;
    for i in start..end {
        let element = heap
            .read(*id, |o| match o {
                HeapObject::Array(a) => a.get(i).cloned(),
                _ => None,
            })
            .ok_or(Fault::Internal("array receiver was reclaimed"))?;
        property::append(heap, task, &result, element)?;
    }
    Ok(Some(result))
}
