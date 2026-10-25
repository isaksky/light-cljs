//! Private elements: lexical fresh symbols and owner-checked, traced storage.
//! No public key conversion, prototype walk, integrity check or Proxy trap.
//! Semantics reference: QuickJS quickjs.c JS_*PrivateField / js_operator_private_in,
//! revision 7d1c13c5d8f8; independently implemented over task-owned objects.
use std::collections::HashMap;

use super::dispatch::Fault;
use super::instr::Instr;
use super::property::Read;
use crate::heap::gc::{Trace, TraceEdge};
use crate::heap::{Heap, HeapObject};
use crate::ids::TaskId;
use crate::object::{OrdinaryObject, Property, PropertyFlags};
use crate::value::{JsSymbol, Value};

#[derive(Debug, Default)]
pub(crate) struct Elements(HashMap<JsSymbol, Property>);

impl Trace for Elements {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        for (key, property) in &self.0 {
            visit(TraceEdge::Symbol(key.identity()));
            property.value.trace(visit);
            if let Some((get, set)) = &property.accessor {
                get.trace(visit);
                set.trace(visit);
            }
        }
    }
}

fn error() -> Fault {
    Fault::TypeError("invalid private member receiver or operation".into())
}

/// The callback is a leaf under the heap lock. Function records are attached
/// under the existing publication exclusion; persistent/intrinsic objects
/// cannot acquire mutable private state. Proxy storage belongs to the proxy.
fn storage<R>(
    heap: &Heap,
    task: TaskId,
    base: &Value,
    create: bool,
    f: impl FnOnce(Option<&mut Elements>) -> Result<R, Fault>,
) -> Result<R, Fault> {
    let &Value::Object(mut id) = base else {
        return Err(error());
    };
    if heap
        .read(id, |o| {
            matches!(o, HeapObject::Function(_) | HeapObject::BoundFunction(_))
        })
        .unwrap_or(false)
    {
        match super::function::private_record(heap, task, id, create)? {
            Some(record) => id = record,
            None => return f(None),
        }
    }
    heap.with_object_mut(id, |object| {
        let ordinary: Option<&mut OrdinaryObject> = match object {
            HeapObject::Plain(o) => Some(o),
            HeapObject::Array(a) => Some(a.object_mut()),
            HeapObject::JsMap(m) => Some(m.object_mut()),
            HeapObject::WeakMap(m) => Some(m.object_mut()),
            HeapObject::WeakRef(m) => Some(m.object_mut()),
            HeapObject::FinalizationRegistry(m) => Some(m.object_mut()),
            HeapObject::DisposableStack(m) => Some(m.object_mut()),
            HeapObject::Generator(g) => Some(&mut g.object),
            HeapObject::ArrayBuffer(b) => Some(&mut b.object),
            HeapObject::TypedArray(a) => Some(&mut a.object),
            HeapObject::Proxy(p) => {
                if p.owner != task {
                    return Err(error());
                }
                return f(Some(&mut p.private));
            }
            _ => None,
        };
        match ordinary {
            Some(o) if o.owner().is_some_and(|owner| owner != task) => Err(error()),
            Some(o) if o.owner() == Some(task) && !o.is_intrinsic() => {
                f(o.private_elements(create))
            }
            _ if create => Err(error()),
            _ => f(None),
        }
    })
    .ok_or(Fault::Internal("private receiver was reclaimed"))?
}

fn symbol(key: &Value) -> Result<&JsSymbol, Fault> {
    match key {
        Value::Symbol(s) => Ok(s),
        _ => Err(Fault::TypeError("invalid private name".into())),
    }
}

pub(crate) fn has(heap: &Heap, task: TaskId, base: &Value, key: &Value) -> Result<bool, Fault> {
    let key = symbol(key)?;
    storage(heap, task, base, false, |s| {
        Ok(s.is_some_and(|s| s.0.contains_key(key)))
    })
}

pub(crate) fn read(heap: &Heap, task: TaskId, base: &Value, key: &Value) -> Result<Read, Fault> {
    let key = symbol(key)?;
    storage(heap, task, base, false, |s| {
        let p = s.and_then(|s| s.0.get(key)).ok_or_else(error)?;
        // PrivateGet differs from ordinary [[Get]]: a setter-only private
        // accessor cannot be read, even for a skipped logical assignment.
        if matches!(&p.accessor, Some((Value::Undefined, _))) {
            return Err(Fault::TypeError("private accessor has no getter".into()));
        }
        Ok(Read::property(p, false))
    })
}

pub(crate) fn write(
    heap: &Heap,
    task: TaskId,
    base: &Value,
    key: &Value,
    value: Value,
) -> Result<Option<Value>, Fault> {
    let key = symbol(key)?;
    storage(heap, task, base, false, |s| {
        let p = s.and_then(|s| s.0.get_mut(key)).ok_or_else(error)?;
        if let Some((_, set)) = &p.accessor {
            return if matches!(set, Value::Undefined) {
                Err(error())
            } else {
                Ok(Some(set.clone()))
            };
        }
        if !p.flags.writable {
            return Err(error());
        }
        p.value = value;
        Ok(None)
    })
}

pub(crate) fn define(
    heap: &Heap,
    task: TaskId,
    base: &Value,
    key: &Value,
    value: Value,
    instr: Instr,
) -> Result<(), Fault> {
    let key = symbol(key)?;
    storage(heap, task, base, true, |s| {
        let s = s.ok_or_else(error)?;
        let accessor = matches!(
            instr,
            Instr::DefinePrivateGetter | Instr::DefinePrivateSetter
        );
        if let Some(old) = s.0.get_mut(key) {
            if accessor && let Some((get, set)) = &mut old.accessor {
                let slot = if matches!(instr, Instr::DefinePrivateGetter) {
                    get
                } else {
                    set
                };
                if matches!(slot, Value::Undefined) {
                    *slot = value;
                    return Ok(());
                }
            }
            return Err(Fault::TypeError(
                "private member already initialized".into(),
            ));
        }
        let mut property = Property {
            value: Value::Undefined,
            accessor: None,
            flags: PropertyFlags {
                writable: matches!(instr, Instr::DefinePrivateField),
                enumerable: false,
                configurable: false,
            },
        };
        if accessor {
            property.accessor = Some(if matches!(instr, Instr::DefinePrivateGetter) {
                (value, Value::Undefined)
            } else {
                (Value::Undefined, value)
            });
        } else {
            property.value = value;
        }
        s.0.insert(key.clone(), property);
        Ok(())
    })
}
