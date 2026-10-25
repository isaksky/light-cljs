//! Built-in GetPrototypeFromConstructor (clj-quickjs-309). Fresh implementation
//! of the allocation rule illustrated by QuickJS JS_CreateFromConstructor,
//! js_array_constructor and js_error_constructor (7d1c13c5d8f8).
use super::{Action, Fault, Input};
use crate::heap::gc::Trace;
use crate::heap::{Heap, ObjectId};
use crate::ids::TaskId;
use crate::value::{JsString, Value};
use crate::vm::{builtin::Builtin, instr::Instr, prototype};

/// Boxed so ordinary constructor entry does not enlarge every VM action.
#[derive(Debug)]
pub(crate) struct Invocation {
    pub(super) callee: Value,
    pub(super) target: Value,
    pub(super) this: Value,
    pub(super) args: Vec<Value>,
}

#[derive(Debug)]
pub(super) struct State {
    which: Builtin,
    ordinary: Option<Value>,
    target: Value,
    args: Vec<Value>,
    prototype: Option<ObjectId>,
    result: Value,
    stage: Stage,
    late: bool,
    regexp: Option<Box<super::regexp::State>>,
}

#[derive(Debug)]
enum Stage {
    Start,
    TypedLength,
    Prototype,
    Initialize,
    Result,
    Finish,
}

impl Trace for State {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        self.target.trace(visit);
        if let Some(callee) = &self.ordinary {
            callee.trace(visit);
        }
        for arg in &self.args {
            arg.trace(visit);
        }
        if let Some(id) = self.prototype {
            visit(id.into());
        }
        self.result.trace(visit);
        if let Some(regexp) = &self.regexp {
            regexp.trace(visit);
        }
    }
}

impl State {
    pub(super) fn new(which: Builtin, target: Value, args: Vec<Value>) -> Self {
        // These constructors convert/validate their arguments before selecting
        // the prototype. No partially initialized result is exposed to JS.
        let late = matches!(
            which,
            Builtin::Number
                | Builtin::String
                | Builtin::Date
                | Builtin::ArrayBuffer
                | Builtin::Function
                | Builtin::WeakRef
                | Builtin::FinalizationRegistry
        );
        Self {
            which,
            ordinary: None,
            target,
            args,
            prototype: None,
            result: Value::Undefined,
            stage: Stage::Start,
            late,
            regexp: None,
        }
    }

    pub(super) fn ordinary(callee: Value, target: Value, args: Vec<Value>) -> Self {
        let mut state = Self::new(Builtin::Object, target, args);
        state.ordinary = Some(callee);
        state
    }

    fn lookup(&mut self) -> Action {
        self.stage = Stage::Prototype;
        Action::Instruction {
            instr: Instr::GetElem,
            operands: vec![
                self.target.clone(),
                Value::String(JsString::from("prototype")),
            ],
        }
    }

    pub(super) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
    ) -> Result<Action, Fault> {
        if let Some(regexp) = &mut self.regexp {
            return regexp.step(heap, task, input);
        }
        match self.stage {
            Stage::Start => {
                if self.which == Builtin::RegExp {
                    // RegExp observes the pattern before prototype selection,
                    // but coerces source/flags afterwards. Keep all three phases
                    // in its traced, resumable continuation.
                    let mut regexp = super::regexp::start(
                        heap,
                        task,
                        self.which,
                        &Value::Undefined,
                        &self.args,
                        true,
                    )?
                    .ok_or(Fault::Internal("RegExp constructor missing"))?;
                    regexp.set_new_target(self.target.clone());
                    self.regexp = Some(Box::new(regexp));
                    self.args.clear();
                    return self.regexp.as_mut().expect("RegExp state installed").step(
                        heap,
                        task,
                        Input::Start,
                    );
                }
                if matches!(self.which, Builtin::IntegerTypedArray(_))
                    && !matches!(self.args.first(), Some(Value::Object(_)))
                {
                    self.stage = Stage::TypedLength;
                    return Ok(Action::ToNumber(
                        self.args.first().cloned().unwrap_or(Value::Undefined),
                    ));
                }
                if self.which == Builtin::Promise {
                    crate::vm::dispatch::callee(
                        heap,
                        self.args.first().unwrap_or(&Value::Undefined),
                    )?;
                }
                if !self.late {
                    return Ok(self.lookup());
                }
                self.stage = Stage::Initialize;
            }
            Stage::TypedLength => {
                let Input::CallResult(value) = input else {
                    return Err(Fault::Internal("typed-array length missing"));
                };
                let length = crate::object::typed_array::to_index(&value)?;
                if self.args.is_empty() {
                    self.args.push(Value::Undefined);
                }
                self.args[0] = Value::Number(length as f64);
                return Ok(self.lookup());
            }
            Stage::Prototype => {
                let Input::CallResult(value) = input else {
                    return Err(Fault::Internal("constructor missing prototype"));
                };
                self.prototype = Some(match value {
                    Value::Object(id) => id,
                    _ => default_prototype(heap, self.which)?,
                });
                self.stage = if self.late {
                    Stage::Finish
                } else {
                    Stage::Initialize
                };
            }
            Stage::Result => {
                let Input::CallResult(value) = input else {
                    return Err(Fault::Internal("constructor missing result"));
                };
                self.result = value;
                if self.ordinary.is_some() {
                    return Ok(Action::Return(self.result.clone()));
                }
                if self.late {
                    return Ok(self.lookup());
                }
                self.stage = Stage::Finish;
            }
            Stage::Initialize | Stage::Finish => {}
        }
        if matches!(self.stage, Stage::Initialize) {
            self.stage = Stage::Result;
            if let Some(callee) = &self.ordinary {
                let prototype = self
                    .prototype
                    .ok_or(Fault::Internal("ordinary constructor prototype missing"))?;
                let receiver = heap
                    .alloc(crate::heap::HeapObject::Plain(
                        crate::object::OrdinaryObject::with_prototype(heap, task, prototype),
                    ))
                    .ok_or(Fault::OutOfMemory("heap object table exhausted"))?;
                return Ok(Action::EnterConstructor(Box::new(Invocation {
                    callee: callee.clone(),
                    target: self.target.clone(),
                    this: Value::Object(receiver),
                    args: std::mem::take(&mut self.args),
                })));
            }
            return Ok(Action::InitializeBuiltin {
                which: self.which,
                args: std::mem::take(&mut self.args),
                prototype: self.prototype,
            });
        }
        if !matches!(self.stage, Stage::Finish) {
            return Err(Fault::Internal("invalid constructor continuation"));
        }
        // Collection adders receive the object during initialization and may
        // legitimately change its prototype or prevent extensions.
        if !matches!(
            self.which,
            Builtin::Map | Builtin::WeakMap | Builtin::Set | Builtin::WeakSet
        ) {
            initialize_prototype(
                heap,
                task,
                &self.result,
                self.prototype
                    .ok_or(Fault::Internal("constructor prototype missing"))?,
            )?;
        }
        Ok(Action::Return(self.result.clone()))
    }
}

fn default_prototype(heap: &Heap, which: Builtin) -> Result<ObjectId, Fault> {
    let r = crate::realm::intrinsics(heap).ok_or(Fault::OutOfMemory("realm exhausted"))?;
    Ok(match which {
        Builtin::Object => r.object_prototype,
        Builtin::Array => r.array_prototype,
        Builtin::Boolean => r.boolean_prototype,
        Builtin::Number => r.number_prototype,
        Builtin::String => r.string_prototype,
        Builtin::Function => r.function_prototype,
        Builtin::Error => r.error_prototype,
        Builtin::NativeError(kind) => r.native_error_prototype(kind),
        Builtin::Map => r.map_prototype,
        Builtin::WeakMap => r.weak_map_prototype,
        Builtin::Set => r.set_prototype,
        Builtin::WeakSet => r.weak_set_prototype,
        Builtin::WeakRef => r.weak_ref_prototype,
        Builtin::FinalizationRegistry => r.finalization_registry_prototype,
        Builtin::Promise => r.promise_prototype,
        Builtin::RegExp => r.regexp_prototype,
        Builtin::Date => r.date_prototype,
        Builtin::ArrayBuffer => r.array_buffer_prototype,
        Builtin::IntegerTypedArray(kind) => r.typed_array_prototypes[kind as usize],
        Builtin::IntlLocale => r.intl_locale_prototype,
        Builtin::IntlNumberFormat => r.intl_number_format_prototype,
        Builtin::IntlPluralRules => r.intl_plural_rules_prototype,
        Builtin::IntlRelativeTimeFormat => r.intl_relative_time_format_prototype,
        Builtin::IntlDisplayNames => r.intl_display_names_prototype,
        Builtin::IntlDurationFormat => r.intl_duration_format_prototype,
        Builtin::IntlCollator => r.intl_collator_prototype,
        Builtin::IntlDateTimeFormat => r.intl_date_time_format_prototype,
        Builtin::IntlListFormat => r.intl_list_format_prototype,
        Builtin::IntlSegmenter => r.intl_segmenter_prototype,
        _ => return Err(Fault::Internal("no built-in constructor prototype")),
    })
}

/// Only freshly allocated, unpublished constructor results reach this helper.
/// The ordinary/exotic payload stays intact; no user setter is involved.
pub(super) fn initialize_prototype(
    heap: &Heap,
    task: TaskId,
    result: &Value,
    proto: ObjectId,
) -> Result<(), Fault> {
    let Value::Object(id) = result else {
        return Err(Fault::Internal("constructor result is not an object"));
    };
    if heap
        .read(*id, |o| matches!(o, crate::heap::HeapObject::Promise(_)))
        .unwrap_or(false)
    {
        // Promise's prototype and ownership were installed before its executor.
        return Ok(());
    }
    if heap
        .read(*id, |o| matches!(o, crate::heap::HeapObject::Function(_)))
        .unwrap_or(false)
    {
        return crate::vm::function::initialize_constructor_prototype(heap, task, *id, proto);
    }
    prototype::set_prototype_of(heap, task, *id, Some(proto))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonobject_new_target_prototype_uses_the_matching_intrinsic() {
        let runtime = crate::Runtime::new();
        let heap = runtime.heap();
        let task = TaskId::from_raw(1);
        for which in [
            Builtin::Array,
            Builtin::Error,
            Builtin::Map,
            Builtin::Promise,
            Builtin::RegExp,
            Builtin::Date,
            Builtin::Boolean,
            Builtin::Number,
            Builtin::String,
        ] {
            for value in [
                Value::Undefined,
                Value::Null,
                Value::Number(1.),
                Value::Bool(false),
            ] {
                let mut state = State::new(which, Value::Undefined, vec![]);
                state.stage = Stage::Prototype;
                // Exercise the allocation-time branch independently of argument conversion.
                state.late = false;
                let Action::InitializeBuiltin { prototype, .. } = state
                    .step(heap, task, Input::CallResult(value))
                    .ok()
                    .expect("prototype selection succeeds")
                else {
                    panic!("expected constructor initialization");
                };
                assert_eq!(
                    prototype,
                    Some(
                        default_prototype(heap, which)
                            .ok()
                            .expect("intrinsic exists")
                    )
                );
            }
        }
    }
}
