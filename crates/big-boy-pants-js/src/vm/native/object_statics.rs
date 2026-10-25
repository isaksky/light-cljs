//! Object statics compose the budgeted copy and descriptor protocols. Descriptor
//! conversion finishes for every enumerable key before the first target write.
use super::*;
use crate::object::PropertyDescriptor;

#[inline(never)]
pub(super) fn start(
    heap: &Heap,
    state: &mut VmState,
    which: builtin::Builtin,
    args: &[Value],
    popped: usize,
) -> Result<(), Fault> {
    use builtin::Builtin;
    let task = state.diag.task;
    let arg = |i| args.get(i).cloned().unwrap_or(Value::Undefined);
    let native = match which {
        Builtin::ObjectEntries | Builtin::ObjectValues => {
            if matches!(arg(0), Value::Null | Value::Undefined) {
                return Err(Fault::TypeError(
                    "cannot convert null or undefined to object".into(),
                ));
            }
            NativeState::CopyProperties(copy_properties::State::with_mode(
                property::new_array(heap, task)?,
                arg(0),
                if which == Builtin::ObjectEntries {
                    copy_properties::Mode::Entries
                } else {
                    copy_properties::Mode::Values
                },
            ))
        }
        Builtin::ObjectAssign => NativeState::ObjectStatics(Box::new(State::Assign {
            target: builtin::array_string_receiver(heap, task, &arg(0))?,
            sources: args.iter().skip(1).cloned().collect(),
            next: 0,
            copy: None,
        })),
        Builtin::ObjectFromEntries => NativeState::AddEntries(Box::new(
            add_entries::State::from_entries(builtin::new_object(heap, task)?, arg(0)),
        )),
        Builtin::ObjectDefineProperties => {
            if !matches!(arg(0), Value::Object(_)) {
                return Err(Fault::TypeError(
                    "Object.defineProperties target must be an object".into(),
                ));
            }
            NativeState::ObjectStatics(Box::new(State::Define(Box::new(Define {
                target: arg(0),
                properties: arg(1),
                keys: Value::Undefined,
                key: Value::Undefined,
                index: 0,
                length: 0,
                stage: Stage::Start,
                reader: None,
                descriptors: vec![],
            }))))
        }
        _ => return Err(Fault::Internal("not an Object static")),
    };
    state.truncate_operands(state.stack_len() - popped)?;
    push_native(state, native)
}

#[derive(Debug)]
pub(super) enum State {
    Assign {
        target: Value,
        sources: Vec<Value>,
        next: usize,
        copy: Option<copy_properties::State>,
    },
    Define(Box<Define>),
}

impl Trace for State {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        match self {
            Self::Assign {
                target,
                sources,
                copy,
                ..
            } => {
                target.trace(visit);
                for source in sources {
                    source.trace(visit);
                }
                if let Some(copy) = copy {
                    copy.trace(visit);
                }
            }
            Self::Define(define) => define.trace(visit),
        }
    }
}

impl State {
    pub(super) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
    ) -> Result<Action, Fault> {
        match self {
            Self::Assign {
                target,
                sources,
                next,
                copy,
            } => {
                if let Some(active) = copy {
                    let action = active.step(heap, task, input)?;
                    if matches!(action, Action::Return(_)) {
                        *copy = None;
                        return Ok(Action::Continue);
                    }
                    return Ok(action);
                }
                let Some(source) = sources.get_mut(*next) else {
                    return Ok(Action::Return(target.clone()));
                };
                *copy = Some(copy_properties::State::with_mode(
                    target.clone(),
                    std::mem::replace(source, Value::Undefined),
                    copy_properties::Mode::Assign,
                ));
                *next += 1;
                Ok(Action::Continue)
            }
            Self::Define(define) => define.step(heap, task, input),
        }
    }
}

#[derive(Debug)]
enum Stage {
    Start,
    Keys,
    Next,
    Enumerable,
    Attributes,
    Convert,
    Apply,
    Applied,
}

#[derive(Debug)]
pub(super) struct Define {
    target: Value,
    properties: Value,
    keys: Value,
    key: Value,
    index: u32,
    length: u32,
    stage: Stage,
    reader: Option<descriptor::DefineProperty>,
    descriptors: Vec<(Value, PropertyDescriptor)>,
}

impl Trace for Define {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        for value in [&self.target, &self.properties, &self.keys, &self.key] {
            value.trace(visit);
        }
        if let Some(reader) = &self.reader {
            reader.trace(visit);
        }
        for (key, desc) in &self.descriptors {
            key.trace(visit);
            for value in [&desc.value, &desc.get, &desc.set].into_iter().flatten() {
                value.trace(visit);
            }
        }
    }
}

impl Define {
    fn step(&mut self, heap: &Heap, task: TaskId, input: Input) -> Result<Action, Fault> {
        let result = || match &input {
            Input::CallResult(value) => Ok(value.clone()),
            _ => Err(Fault::Internal("Object.defineProperties expected result")),
        };
        match self.stage {
            Stage::Start => {
                self.stage = Stage::Keys;
                Ok(Action::Instruction {
                    instr: Instr::OwnKeys,
                    operands: vec![self.properties.clone()],
                })
            }
            Stage::Keys => {
                self.keys = result()?;
                let Value::Number(length) =
                    property::get_field(heap, task, &self.keys, &JsString::from("length"))?
                else {
                    return Err(Fault::Internal("own keys missing length"));
                };
                self.length = length as u32;
                self.stage = Stage::Next;
                Ok(Action::Continue)
            }
            Stage::Next => {
                if self.index == self.length {
                    self.index = 0;
                    self.stage = Stage::Apply;
                    return Ok(Action::Continue);
                }
                self.key =
                    property::get(heap, task, &self.keys, &property::Key::Index(self.index))?;
                self.index += 1;
                self.stage = Stage::Enumerable;
                Ok(Action::Instruction {
                    instr: Instr::IsEnumerable,
                    operands: vec![self.properties.clone(), self.key.clone()],
                })
            }
            Stage::Enumerable => {
                if !result()?.truthy() {
                    self.stage = Stage::Next;
                    return Ok(Action::Continue);
                }
                self.stage = Stage::Attributes;
                Ok(Action::Instruction {
                    instr: Instr::GetElem,
                    operands: vec![self.properties.clone(), self.key.clone()],
                })
            }
            Stage::Attributes => {
                self.reader = Some(descriptor::DefineProperty::reader(result()?)?);
                self.stage = Stage::Convert;
                Ok(Action::Continue)
            }
            Stage::Convert => {
                let reader = self
                    .reader
                    .as_mut()
                    .ok_or(Fault::Internal("missing descriptor reader"))?;
                let value = match input {
                    Input::CallResult(value) => Some(value),
                    _ => None,
                };
                match reader.step(heap, task, value)? {
                    descriptor::DefineStep::Getter { callee, this } => Ok(Action::Call {
                        callee,
                        this,
                        args: vec![],
                    }),
                    descriptor::DefineStep::Done(_) => {
                        self.descriptors
                            .push((self.key.clone(), reader.descriptor().clone()));
                        self.reader = None;
                        self.stage = Stage::Next;
                        Ok(Action::Continue)
                    }
                    _ => Err(Fault::Internal("descriptor reader attempted a write")),
                }
            }
            Stage::Apply => {
                let Some((key, desc)) = self.descriptors.get(self.index as usize) else {
                    return Ok(Action::Return(self.target.clone()));
                };
                self.index += 1;
                self.stage = Stage::Applied;
                Ok(Action::DefineProperty(Box::new(
                    descriptor::DefineProperty::prepared(
                        self.target.clone(),
                        property::to_property_key(key)?,
                        desc.clone(),
                    ),
                )))
            }
            Stage::Applied => {
                self.stage = Stage::Apply;
                Ok(Action::Continue)
            }
        }
    }
}
