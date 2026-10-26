//! Generic array searches. Each scan advances through budgeted native steps;
//! getters, conversions and predicates use ordinary resumable VM calls.
use super::builtin::Builtin;
use super::*;

#[derive(Clone, Copy, Debug)]
enum Stage {
    Start,
    Length,
    LengthNumber,
    IndexNumber,
    Scan,
    Present,
    Element,
    Predicate,
}

#[derive(Debug)]
pub(super) struct State {
    which: Builtin,
    stage: Stage,
    this: Value,
    argument: Value,
    second: Value,
    has_second: bool,
    element: Value,
    read_value: Option<Value>,
    length: i64,
    index: i64,
}

impl Trace for State {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        for value in [&self.this, &self.argument, &self.second, &self.element] {
            value.trace(visit);
        }
        if let Some(value) = &self.read_value {
            value.trace(visit);
        }
    }
}

impl State {
    pub(super) fn new(which: Builtin, this: Value, args: &[Value]) -> Self {
        Self {
            which,
            stage: Stage::Start,
            this,
            argument: args.first().cloned().unwrap_or(Value::Undefined),
            second: args.get(1).cloned().unwrap_or(Value::Undefined),
            has_second: args.len() > 1,
            element: Value::Undefined,
            read_value: None,
            length: 0,
            index: 0,
        }
    }

    fn predicate(&self) -> bool {
        matches!(
            self.which,
            Builtin::ArrayPrototypeFind
                | Builtin::ArrayPrototypeFindIndex
                | Builtin::ArrayPrototypeFindLast
                | Builtin::ArrayPrototypeFindLastIndex
        )
    }

    fn backward(&self) -> bool {
        matches!(
            self.which,
            Builtin::ArrayPrototypeLastIndexOf
                | Builtin::ArrayPrototypeFindLast
                | Builtin::ArrayPrototypeFindLastIndex
        )
    }

    fn value(&mut self, input: Input) -> Result<Value, Fault> {
        match input {
            Input::CallResult(value) => Ok(value),
            Input::Start => self
                .read_value
                .take()
                .ok_or(Fault::Internal("array search expected a saved read")),
            _ => Err(Fault::Internal("array search expected a call result")),
        }
    }

    fn read(
        &mut self,
        heap: &Heap,
        task: TaskId,
        key: property::Key,
        next: Stage,
    ) -> Result<Action, Fault> {
        self.stage = next;
        match property::read(heap, task, &self.this, &key)? {
            Some(property::Read::Getter(callee)) => Ok(Action::Call {
                callee,
                this: self.this.clone(),
                args: vec![],
            }),
            read => {
                self.read_value = Some(match read {
                    Some(property::Read::Value(value)) => value,
                    None => Value::Undefined,
                    Some(property::Read::Getter(_)) => unreachable!(),
                });
                Ok(Action::Continue)
            }
        }
    }

    fn missing(&self) -> Action {
        use Builtin::*;
        Action::Return(match self.which {
            ArrayPrototypeIncludes => Value::Bool(false),
            ArrayPrototypeFind | ArrayPrototypeFindLast | ArrayPrototypeAt => Value::Undefined,
            _ => Value::Number(-1.0),
        })
    }

    fn found(&self) -> Action {
        use Builtin::*;
        Action::Return(match self.which {
            ArrayPrototypeIncludes => Value::Bool(true),
            ArrayPrototypeFind | ArrayPrototypeFindLast | ArrayPrototypeAt => self.element.clone(),
            _ => Value::Number(self.index as f64),
        })
    }

    fn advance(&mut self) -> Action {
        self.index += if self.backward() { -1 } else { 1 };
        self.element = Value::Undefined;
        self.stage = Stage::Scan;
        Action::Continue
    }

    pub(super) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
    ) -> Result<Action, Fault> {
        use Builtin::*;
        match self.stage {
            Stage::Start => {
                self.this = builtin::array_string_receiver(heap, task, &self.this)?;
                self.read(
                    heap,
                    task,
                    property::Key::Name("length".into()),
                    Stage::Length,
                )
            }
            Stage::Length => {
                self.stage = Stage::LengthNumber;
                Ok(Action::ToNumber(self.value(input)?))
            }
            Stage::LengthNumber => {
                self.length = to_length(&self.value(input)?)? as i64;
                if self.predicate() {
                    // Validate even on an empty receiver, after length conversion.
                    dispatch::callee(heap, &self.argument)?;
                    self.index = if self.backward() { self.length - 1 } else { 0 };
                    self.stage = Stage::Scan;
                    return Ok(Action::Continue);
                }
                // at always coerces its index, including for an empty receiver.
                if self.which != ArrayPrototypeAt && self.length == 0 {
                    return Ok(self.missing());
                }
                self.stage = Stage::IndexNumber;
                let value = if self.which == ArrayPrototypeAt {
                    self.argument.clone()
                } else if self.which == ArrayPrototypeLastIndexOf && !self.has_second {
                    Value::Number((self.length - 1) as f64)
                } else {
                    self.second.clone()
                };
                Ok(Action::ToNumber(value))
            }
            Stage::IndexNumber => {
                let Value::Number(number) = self.value(input)? else {
                    return Err(Fault::Internal("array search expected a number"));
                };
                let integer = if number.is_nan() { 0.0 } else { number.trunc() };
                let relative = if integer < 0.0 {
                    self.length as f64 + integer
                } else {
                    integer
                };
                // Clamp before integer conversion, preserving infinities and the
                // distinction between omitted and explicit undefined fromIndex.
                if self.which == ArrayPrototypeAt {
                    if relative < 0.0 || relative >= self.length as f64 {
                        return Ok(self.missing());
                    }
                    self.index = relative as i64;
                } else if self.backward() {
                    if relative < 0.0 {
                        return Ok(self.missing());
                    }
                    self.index = relative.min((self.length - 1) as f64) as i64;
                } else {
                    if relative >= self.length as f64 {
                        return Ok(self.missing());
                    }
                    self.index = relative.max(0.0) as i64;
                }
                self.stage = Stage::Scan;
                Ok(Action::Continue)
            }
            Stage::Scan => {
                if self.index < 0 || self.index >= self.length {
                    return Ok(self.missing());
                }
                let key = index_key(self.index as u64);
                if matches!(
                    self.which,
                    ArrayPrototypeIndexOf | ArrayPrototypeLastIndexOf
                ) {
                    self.stage = Stage::Present;
                    return Ok(has_property(self.this.clone(), key));
                }
                self.read(heap, task, key, Stage::Element)
            }
            Stage::Present => {
                if !self.value(input)?.truthy() {
                    return Ok(self.advance());
                }
                self.read(heap, task, index_key(self.index as u64), Stage::Element)
            }
            Stage::Element => {
                self.element = self.value(input)?;
                if self.which == ArrayPrototypeAt {
                    return Ok(self.found());
                }
                if self.predicate() {
                    self.stage = Stage::Predicate;
                    return Ok(Action::Call {
                        callee: self.argument.clone(),
                        this: self.second.clone(),
                        args: vec![
                            self.element.clone(),
                            Value::Number(self.index as f64),
                            self.this.clone(),
                        ],
                    });
                }
                let equal = if self.which == ArrayPrototypeIncludes {
                    crate::collections::map::key_eq(&self.element, &self.argument)
                } else {
                    self.element.strict_equals(&self.argument)
                };
                Ok(if equal { self.found() } else { self.advance() })
            }
            Stage::Predicate => {
                let result = self.value(input)?;
                Ok(if result.truthy() {
                    self.found()
                } else {
                    self.advance()
                })
            }
        }
    }
}
