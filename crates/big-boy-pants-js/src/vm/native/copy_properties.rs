//! Own property traversal for object spread, assign, entries, values and
//! descriptors. Each descriptor check, read and write is a budgeted step; Proxy
//! traps, getters and setters return through the ordinary trampoline.
use super::*;

#[derive(Clone, Copy, Debug)]
pub(super) enum Mode {
    Spread,
    Assign,
    Values,
    Entries,
    Descriptors,
}

#[derive(Debug)]
enum Stage {
    Start,
    Keys,
    Next,
    Enumerable,
    Descriptor,
    Value,
    Defined,
}

#[derive(Debug)]
pub(super) struct State {
    mode: Mode,
    target: Value,
    source: Value,
    keys: Value,
    key: Value,
    index: u32,
    length: u32,
    stage: Stage,
    descriptor: Option<Box<super::super::proxy::State>>,
}

impl Trace for State {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        for value in [&self.target, &self.source, &self.keys, &self.key] {
            value.trace(visit);
        }
        if let Some(descriptor) = &self.descriptor {
            descriptor.trace(visit);
        }
    }
}

impl State {
    pub(super) fn new(target: Value, source: Value) -> Self {
        Self {
            mode: Mode::Spread,
            target,
            source,
            keys: Value::Undefined,
            key: Value::Undefined,
            index: 0,
            length: 0,
            stage: Stage::Start,
            descriptor: None,
        }
    }

    pub(super) fn with_mode(target: Value, source: Value, mode: Mode) -> Self {
        Self {
            mode,
            ..Self::new(target, source)
        }
    }

    pub(super) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
    ) -> Result<Action, Fault> {
        let result = || match &input {
            Input::CallResult(value) => Ok(value.clone()),
            _ => Err(Fault::Internal(
                "CopyDataProperties expected instruction result",
            )),
        };
        match self.stage {
            Stage::Start => {
                if matches!(self.source, Value::Null | Value::Undefined) {
                    return Ok(Action::Return(self.target.clone()));
                }
                self.stage = Stage::Keys;
                Ok(Action::Instruction {
                    instr: Instr::OwnKeys,
                    operands: vec![self.source.clone()],
                })
            }
            Stage::Keys => {
                self.keys = result()?;
                let Value::Number(length) = property::get(
                    heap,
                    task,
                    &self.keys,
                    &property::Key::Name(JsString::from("length")),
                )?
                else {
                    return Err(Fault::Internal("own keys missing length"));
                };
                self.length = length as u32;
                self.stage = Stage::Next;
                Ok(Action::Continue)
            }
            Stage::Next => {
                if self.index == self.length {
                    return Ok(Action::Return(self.target.clone()));
                }
                self.key =
                    property::get(heap, task, &self.keys, &property::Key::Index(self.index))?;
                self.index += 1;
                if matches!(self.mode, Mode::Descriptors) {
                    self.stage = Stage::Descriptor;
                    if let Value::Object(target) = self.source {
                        self.descriptor = Some(Box::new(super::super::proxy::State::new(
                            super::super::proxy::Mode::Descriptor {
                                target,
                                key: property::to_property_key(&self.key)?,
                            },
                        )));
                    }
                    return Ok(Action::Continue);
                }
                if matches!(self.mode, Mode::Values | Mode::Entries)
                    && matches!(self.key, Value::Symbol(_))
                {
                    return Ok(Action::Continue);
                }
                self.stage = Stage::Enumerable;
                Ok(Action::Instruction {
                    instr: Instr::IsEnumerable,
                    operands: vec![self.source.clone(), self.key.clone()],
                })
            }
            Stage::Descriptor => {
                let value = if let Some(descriptor) = &mut self.descriptor {
                    match descriptor.step(heap, task, input)? {
                        Action::Return(value) => value,
                        action => return Ok(action),
                    }
                } else {
                    descriptor::get_own_property_descriptor(
                        heap,
                        task,
                        &[self.source.clone(), self.key.clone()],
                    )?
                };
                self.descriptor = None;
                self.stage = Stage::Defined;
                if matches!(value, Value::Undefined) {
                    return Ok(Action::Continue);
                }
                Ok(Action::CreateDataProperty {
                    this: self.target.clone(),
                    key: self.key.clone(),
                    value,
                })
            }
            Stage::Enumerable => {
                if !result()?.truthy() {
                    self.stage = Stage::Next;
                    return Ok(Action::Continue);
                }
                self.stage = Stage::Value;
                Ok(Action::Instruction {
                    instr: Instr::GetElem,
                    operands: vec![self.source.clone(), self.key.clone()],
                })
            }
            Stage::Value => {
                self.stage = Stage::Defined;
                let value = result()?;
                if matches!(self.mode, Mode::Assign) {
                    return Ok(Action::SetProperty {
                        this: self.target.clone(),
                        key: property::to_property_key(&self.key)?,
                        value,
                        strict: true,
                    });
                }
                if matches!(self.mode, Mode::Values | Mode::Entries) {
                    let value = if matches!(self.mode, Mode::Entries) {
                        // OwnKeys uses numeric indices internally; entries exposes strings.
                        let key = match &self.key {
                            Value::Number(n) => {
                                Value::String(JsString::from(n.to_string().as_str()))
                            }
                            key => key.clone(),
                        };
                        let pair = property::new_array(heap, task)?;
                        property::append(heap, task, &pair, Some(key))?;
                        property::append(heap, task, &pair, Some(value))?;
                        pair
                    } else {
                        value
                    };
                    property::append(heap, task, &self.target, Some(value))?;
                    return Ok(Action::Continue);
                }
                Ok(Action::CreateDataProperty {
                    this: self.target.clone(),
                    key: self.key.clone(),
                    value,
                })
            }
            Stage::Defined => {
                self.stage = Stage::Next;
                Ok(Action::Continue)
            }
        }
    }
}
