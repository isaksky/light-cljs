//! CopyDataProperties for object literals. Each descriptor check, read and
//! definition is a separate budgeted instruction; Proxy traps and getters
//! return through the ordinary trampoline. All intermediate values are traced.
use super::*;

#[derive(Debug)]
enum Stage {
    Start,
    Keys,
    Next,
    Enumerable,
    Value,
    Defined,
}

#[derive(Debug)]
pub(super) struct State {
    target: Value,
    source: Value,
    keys: Value,
    key: Value,
    index: u32,
    length: u32,
    stage: Stage,
}

impl Trace for State {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        for value in [&self.target, &self.source, &self.keys, &self.key] {
            value.trace(visit);
        }
    }
}

impl State {
    pub(super) fn new(target: Value, source: Value) -> Self {
        Self {
            target,
            source,
            keys: Value::Undefined,
            key: Value::Undefined,
            index: 0,
            length: 0,
            stage: Stage::Start,
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
                self.stage = Stage::Enumerable;
                Ok(Action::Instruction {
                    instr: Instr::IsEnumerable,
                    operands: vec![self.source.clone(), self.key.clone()],
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
                Ok(Action::CreateDataProperty {
                    this: self.target.clone(),
                    key: self.key.clone(),
                    value: result()?,
                })
            }
            Stage::Defined => {
                self.stage = Stage::Next;
                Ok(Action::Continue)
            }
        }
    }
}
