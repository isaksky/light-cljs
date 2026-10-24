//! Error's callbacks are ordinary resumable VM calls. This follows the
//! semantics of QuickJS js_error_constructor/js_error_toString at revision
//! 7d1c13c5d8f8, implemented over task-owned ordinary objects.
use super::{Action, Fault, Input};
use crate::heap::gc::Trace;
use crate::heap::{Heap, HeapObject, ObjectId};
use crate::ids::TaskId;
use crate::object::{OrdinaryObject, PropertyFlags};
use crate::value::{JsString, Value};
use crate::vm::{dispatch, property, prototype};

#[derive(Debug)]
pub(super) enum ErrorState {
    Constructor {
        object: Value,
        message: Value,
        options: Value,
        stage: u8,
    },
    ToString {
        this: Value,
        name: Value,
        stage: u8,
    },
}

impl Trace for ErrorState {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        match self {
            Self::Constructor {
                object,
                message,
                options,
                ..
            } => {
                object.trace(visit);
                message.trace(visit);
                options.trace(visit);
            }
            Self::ToString { this, name, .. } => {
                this.trace(visit);
                name.trace(visit);
            }
        }
    }
}

impl ErrorState {
    pub(super) fn constructor(heap: &Heap, task: TaskId, args: &[Value]) -> Result<Self, Fault> {
        // NewTarget is Error itself (or a bound Error, resolved by Construct).
        // Reflect.construct and subclass syntax are not supported yet. The
        // intrinsic's fixed prototype is Error.prototype in both call modes.
        let proto = crate::realm::intrinsics(heap)
            .ok_or(Fault::OutOfMemory("heap object table exhausted"))?
            .error_prototype;
        let object = OrdinaryObject::with_prototype(task, proto).with_error_data();
        let id = heap
            .alloc(HeapObject::Plain(object))
            .ok_or(Fault::OutOfMemory("heap object table exhausted"))?;
        Ok(Self::Constructor {
            object: Value::Object(id),
            message: args.first().cloned().unwrap_or(Value::Undefined),
            options: args.get(1).cloned().unwrap_or(Value::Undefined),
            stage: 0,
        })
    }

    pub(super) fn to_string(this: &Value) -> Result<Self, Fault> {
        if !matches!(this, Value::Object(_)) {
            return Err(Fault::TypeError(
                "Error.prototype.toString requires an object".into(),
            ));
        }
        Ok(Self::ToString {
            this: this.clone(),
            name: Value::Undefined,
            stage: 0,
        })
    }

    pub(super) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
    ) -> Result<Action, Fault> {
        match self {
            Self::Constructor {
                object,
                message,
                options,
                stage,
            } => {
                let delivered = result(input)?;
                match *stage {
                    0 => {
                        *stage = 1;
                        if !matches!(message, Value::Undefined) {
                            return Ok(Action::ToString(message.clone()));
                        }
                    }
                    1 => define(heap, object, "message", delivered)?,
                    2 => {
                        define(heap, object, "cause", delivered)?;
                        return Ok(Action::Return(object.clone()));
                    }
                    _ => return Err(Fault::Internal("invalid Error constructor stage")),
                }
                if matches!(options, Value::Object(_))
                    && property::has_property(heap, task, options, &key("cause"))?
                {
                    *stage = 2;
                    match read(heap, task, options, "cause")? {
                        property::Read::Getter(callee) => return Ok(call(callee, options)),
                        property::Read::Value(v) => define(heap, object, "cause", v)?,
                    }
                }
                Ok(Action::Return(object.clone()))
            }
            Self::ToString { this, name, stage } => {
                let mut value = result(input)?;
                loop {
                    match *stage {
                        0 | 3 => {
                            let field = if *stage == 0 { "name" } else { "message" };
                            *stage += 1;
                            match read(heap, task, this, field)? {
                                property::Read::Getter(callee) => return Ok(call(callee, this)),
                                property::Read::Value(v) => value = v,
                            }
                        }
                        1 | 4 => {
                            let default = if *stage == 1 { "Error" } else { "" };
                            *stage += 1;
                            if matches!(value, Value::Undefined) {
                                value = Value::String(JsString::from(default));
                            } else {
                                return Ok(Action::ToString(value));
                            }
                        }
                        2 => {
                            *name = value;
                            value = Value::Undefined;
                            *stage = 3;
                        }
                        5 => return join(name, &value).map(|s| Action::Return(Value::String(s))),
                        _ => return Err(Fault::Internal("invalid Error toString stage")),
                    }
                }
            }
        }
    }
}

fn result(input: Input) -> Result<Value, Fault> {
    match input {
        Input::Start => Ok(Value::Undefined),
        Input::CallResult(v) => Ok(v),
        Input::Dependency(_) => Err(Fault::Internal("Error native received a dependency")),
    }
}

fn key(name: &str) -> property::Key {
    property::Key::Name(JsString::from(name))
}

fn read(heap: &Heap, task: TaskId, this: &Value, name: &str) -> Result<property::Read, Fault> {
    Ok(property::read(heap, task, this, &key(name))?
        .unwrap_or(property::Read::Value(Value::Undefined)))
}

fn call(callee: Value, this: &Value) -> Action {
    Action::Call {
        callee,
        this: this.clone(),
        args: vec![],
    }
}

fn define(heap: &Heap, object: &Value, name: &str, value: Value) -> Result<(), Fault> {
    let Value::Object(id) = object else {
        return Err(Fault::Internal("Error object missing"));
    };
    let key = heap
        .keys()
        .intern(&JsString::from(name))
        .ok_or(Fault::OutOfMemory("property key table exhausted"))?;
    heap.with_object_mut(*id, |o| match o {
        HeapObject::Plain(o) => {
            o.define_with(key, value, PropertyFlags::METHOD);
        }
        _ => unreachable!("Error constructor allocates an ordinary object"),
    })
    .ok_or(Fault::Internal("Error object was reclaimed"))
}

fn join(name: &Value, message: &Value) -> Result<JsString, Fault> {
    let (Value::String(name), Value::String(message)) = (name, message) else {
        return Err(Fault::Internal("Error strings not converted"));
    };
    if name.is_empty() {
        return Ok(message.clone());
    }
    if message.is_empty() {
        return Ok(name.clone());
    }
    name.concat(&JsString::from(": "))
        .and_then(|s| s.concat(message))
        .ok_or_else(|| Fault::RangeError("string too long".into()))
}

/// Diagnostics must not run user code, including getters or conversions.
/// Unknown objects, accessors and object-valued fields use the old fallback.
pub(crate) fn diagnostic(heap: &Heap, task: TaskId, value: &Value) -> Option<String> {
    let Value::Object(id) = value else {
        return None;
    };
    if !heap.read(*id, |o| matches!(o, HeapObject::Plain(o) if o.is_error()))? {
        return None;
    }
    let data = |field: &str, default: &str| -> Option<Value> {
        let mut current = Some(*id);
        while let Some(id) = current {
            if let Some(p) = property::get_own_property(heap, task, id, &key(field)).ok()? {
                if p.accessor.is_some() {
                    return None;
                }
                return match p.value {
                    Value::Object(_) => None,
                    Value::Undefined => Some(Value::String(JsString::from(default))),
                    v => dispatch::to_js_string(&v).ok().map(Value::String),
                };
            }
            current = prototype::get_prototype_of(heap, task, id).ok()?;
        }
        Some(Value::String(JsString::from(default)))
    };
    join(&data("name", "Error")?, &data("message", "")?)
        .ok()
        .map(|s| s.to_string_lossy())
}
