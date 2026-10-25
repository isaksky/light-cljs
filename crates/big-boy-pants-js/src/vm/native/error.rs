//! Error's callbacks are ordinary resumable VM calls. This follows the
//! semantics of QuickJS js_error_constructor/js_error_toString at revision
//! 7d1c13c5d8f8, implemented over task-owned ordinary objects.
use super::{Action, Fault, Input};
use crate::heap::gc::Trace;
use crate::heap::{Heap, HeapObject};
use crate::ids::TaskId;
use crate::object::{OrdinaryObject, PropertyFlags};
use crate::value::{JsString, Value};
use crate::vm::builtin::NativeErrorKind;
use crate::vm::{dispatch, property, prototype};

#[derive(Debug)]
pub(super) enum ErrorState {
    Constructor {
        object: Value,
        message: Value,
        options: Value,
        suppressed: Option<(Value, Value)>,
        errors: Option<Value>,
        iterator: Value,
        next: Value,
        current: Value,
        list: Value,
        stack: JsString,
        stage: u8,
    },
    ToString {
        this: Value,
        name: Value,
        stage: u8,
    },
}

impl Trace for ErrorState {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        match self {
            Self::Constructor {
                object,
                message,
                options,
                suppressed,
                errors,
                iterator,
                next,
                current,
                list,
                ..
            } => {
                object.trace(visit);
                message.trace(visit);
                options.trace(visit);
                for value in [iterator, next, current, list] {
                    value.trace(visit);
                }
                if let Some(value) = errors {
                    value.trace(visit);
                }
                if let Some((error, suppressed)) = suppressed {
                    error.trace(visit);
                    suppressed.trace(visit);
                }
            }
            Self::ToString { this, name, .. } => {
                this.trace(visit);
                name.trace(visit);
            }
        }
    }
}

impl ErrorState {
    pub(super) fn constructor(
        heap: &Heap,
        task: TaskId,
        args: &[Value],
        kind: Option<NativeErrorKind>,
        stack: JsString,
    ) -> Result<Self, Fault> {
        // Calls use the intrinsic prototype; Construct installs its selected
        // new.target prototype before this continuation runs any callbacks.
        let intrinsics = crate::realm::intrinsics(heap)
            .ok_or(Fault::OutOfMemory("heap object table exhausted"))?;
        let proto = match kind {
            Some(kind) => intrinsics.native_error_prototype(kind),
            None => intrinsics.error_prototype,
        };
        let object = OrdinaryObject::with_prototype(task, proto).with_error_data();
        let id = heap
            .alloc(HeapObject::Plain(object))
            .ok_or(Fault::OutOfMemory("heap object table exhausted"))?;
        let suppressed = kind == Some(NativeErrorKind::Suppressed);
        let aggregate = kind == Some(NativeErrorKind::Aggregate);
        Ok(Self::Constructor {
            object: Value::Object(id),
            message: args
                .get(if suppressed {
                    2
                } else if aggregate {
                    1
                } else {
                    0
                })
                .cloned()
                .unwrap_or(Value::Undefined),
            // SuppressedError has no options argument (ECMAScript). In
            // particular, never read a fourth argument's cause getter.
            options: if suppressed {
                Value::Undefined
            } else {
                args.get(if aggregate { 2 } else { 1 })
                    .cloned()
                    .unwrap_or(Value::Undefined)
            },
            suppressed: suppressed.then(|| {
                (
                    args.first().cloned().unwrap_or(Value::Undefined),
                    args.get(1).cloned().unwrap_or(Value::Undefined),
                )
            }),
            errors: aggregate.then(|| args.first().cloned().unwrap_or(Value::Undefined)),
            iterator: Value::Undefined,
            next: Value::Undefined,
            current: Value::Undefined,
            list: Value::Undefined,
            stack,
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
                suppressed,
                errors,
                iterator,
                next,
                current,
                list,
                stack,
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
                        *options = Value::Undefined;
                    }
                    3 => {
                        *iterator =
                            property::get_field(heap, task, &delivered, &"iterator".into())?;
                        *next = property::get_field(heap, task, &delivered, &"next".into())?;
                        *list = property::new_array(heap, task)?;
                        *stage = 4;
                        return Ok(Action::Continue);
                    }
                    4 => {
                        *stage = 5;
                        return Ok(Action::Call {
                            callee: next.clone(),
                            this: iterator.clone(),
                            args: vec![],
                        });
                    }
                    5 => {
                        if !matches!(delivered, Value::Object(_)) {
                            return Err(Fault::TypeError(
                                "iterator result must be an object".into(),
                            ));
                        }
                        *current = delivered;
                        *stage = 6;
                        return Ok(Action::Instruction {
                            instr: crate::vm::instr::Instr::GetElem,
                            operands: vec![current.clone(), Value::String("done".into())],
                        });
                    }
                    6 => {
                        if delivered.truthy() {
                            define(heap, object, "errors", list.clone())?;
                            return Ok(Action::Return(object.clone()));
                        }
                        *stage = 7;
                        return Ok(Action::Instruction {
                            instr: crate::vm::instr::Instr::GetElem,
                            operands: vec![current.clone(), Value::String("value".into())],
                        });
                    }
                    7 => {
                        if property::get_field(heap, task, list, &"length".into())?
                            == Value::Number(f64::from(u32::MAX))
                        {
                            return Err(Fault::RangeError(
                                "invalid AggregateError errors array length".into(),
                            ));
                        }
                        property::append(heap, task, list, Some(delivered))?;
                        *stage = 4;
                        return Ok(Action::Continue);
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
                if let Some((error, suppressed)) = suppressed {
                    define(heap, object, "error", error.clone())?;
                    define(heap, object, "suppressed", suppressed.clone())?;
                }
                define(heap, object, "stack", Value::String(stack.clone()))?;
                if let Some(errors) = errors {
                    *stage = 3;
                    return Ok(Action::Instruction {
                        instr: crate::vm::instr::Instr::GetIterator,
                        operands: vec![errors.clone()],
                    });
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

impl crate::vm::frames::VmState {
    /// Engine-only wrapping for disposal failures. Never consults globals,
    /// constructor properties, getters or conversions. Like other VM
    /// allocations, this runs under the caller's mutator guard between
    /// safepoints. Both inputs must remain in traced task/native state;
    /// the caller must store the result there before the next poll, where
    /// allocation pressure may collect. Heap::alloc itself never polls.
    pub(crate) fn suppressed_error(
        &self,
        heap: &Heap,
        error: Value,
        suppressed: Value,
    ) -> Result<Value, Fault> {
        suppressed_error(
            heap,
            self.diag.task,
            error,
            suppressed,
            self.error_stack(self.top().pc.saturating_sub(1))?,
        )
    }
}

/// Intrinsic allocation shared by bytecode and resumable disposal loops.
pub(super) fn suppressed_error(
    heap: &Heap,
    task: TaskId,
    error: Value,
    suppressed: Value,
    stack: JsString,
) -> Result<Value, Fault> {
    let intrinsics =
        crate::realm::intrinsics(heap).ok_or(Fault::OutOfMemory("heap object table exhausted"))?;
    let prototype = intrinsics.native_error_prototype(NativeErrorKind::Suppressed);
    let mut object = OrdinaryObject::with_prototype(task, prototype).with_error_data();
    for (name, value) in [
        ("error", error),
        ("suppressed", suppressed),
        ("stack", Value::String(stack)),
    ] {
        let key = heap
            .keys()
            .intern(&JsString::from(name))
            .ok_or(Fault::OutOfMemory("property key table exhausted"))?;
        object.define_with(key, value, PropertyFlags::METHOD);
    }
    heap.alloc(HeapObject::Plain(object))
        .map(Value::Object)
        .ok_or(Fault::OutOfMemory("heap object table exhausted"))
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
        .ok_or(Fault::StringTooLong)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Runtime;
    use crate::frontend::compile_script;
    use crate::heap::Rooted;
    use crate::ids::WorldId;
    use crate::vm::frames::{DiagnosticContext, VmLimits, VmState};

    /// Synthetic fixture, not light-cljs output. User-amended acceptance:
    /// expect TypeError for the intrinsic write, then replace the global.
    /// Neither the rejected write nor the replacement can redirect wrapping.
    #[test]
    fn engine_suppressed_error_ignores_the_constructor_property() {
        let rt = Runtime::new();
        let code = compile_script(
            "engine-suppressed.js",
            r"
            let effects = {count: 0};
            let original = SuppressedError.prototype;
            let rejected = false;
            try {
                SuppressedError.prototype.constructor = function() {
                    effects.count++; throw 'intrinsic hijack';
                };
            } catch(e) { rejected = e instanceof TypeError; }
            SuppressedError = function() { effects.count++; throw 'global hijack'; };
            let a = {get message() { effects.count++; throw 'read a'; }};
            let b = {toString() { effects.count++; throw 'convert b'; }};
            ({a, b, original, effects, rejected, global: globalThis});
            ",
        )
        .unwrap();
        let task = rt.spawn(&code, &[]).unwrap();
        let owner = task.id();
        let crate::RunOutcome::Complete(payload) = task.run().unwrap() else {
            panic!("expected script result");
        };
        let heap = rt.heap();
        let raw = payload.value_in(heap).unwrap();
        let get = |object: &Value, field: &str| match read(heap, owner, object, field).ok().unwrap()
        {
            property::Read::Value(value) => value,
            property::Read::Getter(_) => panic!("unexpected getter: {field}"),
        };
        assert_eq!(get(&raw, "rejected"), Value::Bool(true));
        let a = get(&raw, "a");
        let b = get(&raw, "b");
        let original = get(&raw, "original");
        let effects = Rooted::new(heap, get(&raw, "effects"));
        let global = Rooted::new(heap, get(&raw, "global"));
        assert_ne!(
            get(&get(&raw, "global"), "SuppressedError"),
            get(&original, "constructor")
        );
        let mut state = VmState::new(
            &code.0,
            &[],
            DiagnosticContext {
                task: owner,
                world: WorldId::INITIAL,
            },
            VmLimits::DEFAULT,
        );
        // Give the engine call the script's replaced global environment.
        let Value::Object(global_id) = global.value_in(heap).unwrap() else {
            panic!("expected global object");
        };
        state.global_object = Some(global_id);
        let wrapped = rt.scope(|_| {
            let value = state
                .suppressed_error(heap, a.clone(), b.clone())
                .ok()
                .unwrap();
            Rooted::new(heap, value)
        });
        let value = wrapped.value_in(heap).unwrap();
        let Value::Object(id) = value else {
            panic!("expected Error object")
        };
        assert_eq!(
            prototype::get_prototype_of(heap, owner, id)
                .ok()
                .unwrap()
                .map(Value::Object),
            Some(original),
        );
        assert_eq!(get(&value, "error"), a);
        assert_eq!(get(&value, "suppressed"), b);
        assert_eq!(get(&value, "message"), Value::String(JsString::from("")));
        assert_eq!(
            get(&value, "name"),
            Value::String(JsString::from("SuppressedError"))
        );
        assert_eq!(
            get(&value, "stack"),
            Value::String(state.error_stack(0).ok().unwrap())
        );
        assert!(
            property::get_own_property(heap, owner, id, &key("message"))
                .ok()
                .unwrap()
                .is_none()
        );
        assert!(
            heap.read(id, |o| matches!(o, HeapObject::Plain(o) if o.is_error()))
                .unwrap()
        );
        for field in ["error", "suppressed", "stack"] {
            let prop = property::get_own_property(heap, owner, id, &key(field))
                .ok()
                .unwrap()
                .unwrap();
            assert_eq!(prop.flags, PropertyFlags::METHOD);
        }
        drop(payload);
        drop(task);
        rt.collect().unwrap();
        // Only the wrapped object's ordinary properties keep a and b alive.
        for field in ["error", "suppressed"] {
            let Value::Object(child) = get(&value, field) else {
                panic!("expected child")
            };
            assert!(heap.read(child, |_| ()).is_some());
        }
        assert_eq!(
            get(&effects.value_in(heap).unwrap(), "count"),
            Value::Number(0.0)
        );
        let chained = rt.scope(|_| {
            Rooted::new(
                heap,
                state
                    .suppressed_error(heap, b.clone(), value.clone())
                    .ok()
                    .unwrap(),
            )
        });
        drop(wrapped);
        rt.collect().unwrap();
        assert_eq!(get(&chained.value_in(heap).unwrap(), "suppressed"), value);
        assert_eq!(get(&value, "error"), a);
    }
}
