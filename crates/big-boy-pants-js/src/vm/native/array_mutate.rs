//! Generic array mutators. Each native step handles at most one element
//! move or swap phase; callback results and saved values are traced across waits.
use super::builtin::Builtin;
use super::*;

#[derive(Clone, Copy, Debug)]
enum Stage {
    Start,
    Length,
    LengthNumber,
    Arguments,
    ArgumentNumber,
    Prepare,
    Saved,
    Move,
    MovePresent,
    MoveValue,
    MoveAdvance,
    DeleteTail,
    Insert,
    SetLength,
    Done,
    Reverse,
    ReverseLowerPresent,
    ReverseLower,
    ReverseUpper,
    ReverseUpperPresent,
    ReverseUpperValue,
    ReverseWriteLower,
    ReverseWriteUpper,
    ReverseAdvance,
    Fill,
    Constructor,
    Species,
    CreateRemoved,
    CreatedRemoved,
    Removed,
    RemovedPresent,
    RemovedValue,
    SpliceMove,
}

#[derive(Debug)]
pub(super) struct State {
    which: Builtin,
    stage: Stage,
    this: Value,
    args: Vec<Value>,
    result: Value,
    lower: Value,
    upper: Value,
    read_value: Option<Value>,
    length: u64,
    new_length: u64,
    numbers: [f64; 3],
    argument: usize,
    start: u64,
    delete: u64,
    items: u64,
    index: u64,
    end: u64,
    from: u64,
    to: u64,
    remaining: u64,
    backward: bool,
    after_move: Stage,
    lower_present: bool,
    upper_present: bool,
}

impl Trace for State {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        for value in [&self.this, &self.result, &self.lower, &self.upper]
            .into_iter()
            .chain(&self.args)
        {
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
            args: args.to_vec(),
            result: Value::Undefined,
            lower: Value::Undefined,
            upper: Value::Undefined,
            read_value: None,
            length: 0,
            new_length: 0,
            numbers: [0.0; 3],
            argument: 0,
            start: 0,
            delete: 0,
            items: 0,
            index: 0,
            end: 0,
            from: 0,
            to: 0,
            remaining: 0,
            backward: false,
            after_move: Stage::Done,
            lower_present: false,
            upper_present: false,
        }
    }

    fn value(&mut self, input: Input) -> Result<Value, Fault> {
        match input {
            Input::CallResult(value) => Ok(value),
            Input::Start => self
                .read_value
                .take()
                .ok_or_else(|| Fault::Internal("array mutator expected a saved read")),
            _ => Err(Fault::Internal("array mutator expected a call result")),
        }
    }

    fn read(
        &mut self,
        heap: &Heap,
        task: TaskId,
        base: Value,
        key: property::Key,
        next: Stage,
    ) -> Result<Action, Fault> {
        self.stage = next;
        match property::read(heap, task, &base, &key)? {
            Some(property::Read::Deferred(callee)) => Ok(callee.action(base)),
            read => {
                self.read_value = Some(match read {
                    Some(property::Read::Value(value)) => value,
                    None => Value::Undefined,
                    Some(property::Read::Deferred(_)) => unreachable!(),
                });
                Ok(Action::Continue)
            }
        }
    }

    fn set(&self, key: property::Key, value: Value) -> Action {
        Action::SetProperty {
            this: self.this.clone(),
            key,
            value,
            strict: true,
        }
    }

    fn delete_at(&self, heap: &Heap, task: TaskId, index: u64) -> Result<(), Fault> {
        property::delete(heap, task, &self.this, &Value::Number(index as f64), true)?;
        Ok(())
    }

    // ToIntegerOrInfinity followed by relative-index clamping. All inputs
    // have already undergone resumable ToNumber, in argument order.
    fn relative(number: f64, length: u64) -> u64 {
        let integer = if number.is_nan() { 0.0 } else { number.trunc() };
        if integer < 0.0 {
            (length as f64 + integer).max(0.0) as u64
        } else {
            integer.min(length as f64) as u64
        }
    }

    fn new_length(&mut self) -> Result<(), Fault> {
        self.new_length = self.length - self.delete + self.items;
        if self.new_length > (1_u64 << 53) - 1 {
            return Err(Fault::TypeError(
                "array-like length exceeds 2^53 - 1".into(),
            ));
        }
        Ok(())
    }

    fn movement(&mut self, from: u64, to: u64, count: u64, backward: bool, next: Stage) {
        self.from = from;
        self.to = to;
        self.remaining = count;
        self.backward = backward;
        if backward && count != 0 {
            self.from += count - 1;
            self.to += count - 1;
        }
        self.after_move = next;
        self.stage = Stage::Move;
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
                    self.this.clone(),
                    property::Key::Name("length".into()),
                    Stage::Length,
                )
            }
            Stage::Length => {
                self.stage = Stage::LengthNumber;
                Ok(Action::ToNumber(self.value(input)?))
            }
            Stage::LengthNumber => {
                self.length = to_length(&self.value(input)?)?;
                self.new_length = self.length;
                self.stage = Stage::Arguments;
                Ok(Action::Continue)
            }
            Stage::Arguments => {
                let count = match self.which {
                    ArrayPrototypeFill => 2,
                    ArrayPrototypeCopyWithin => 3,
                    ArrayPrototypeSplice => {
                        if self.args.len() >= 2 {
                            2
                        } else {
                            1
                        }
                    }
                    _ => 0,
                };
                if self.argument == count {
                    self.stage = Stage::Prepare;
                    return Ok(Action::Continue);
                }
                let arg_index = self.argument + usize::from(self.which == ArrayPrototypeFill);
                let value = self
                    .args
                    .get(arg_index)
                    .cloned()
                    .unwrap_or(Value::Undefined);
                if arg_index == 2 && matches!(value, Value::Undefined) {
                    self.numbers[self.argument] = self.length as f64;
                    self.argument += 1;
                    return Ok(Action::Continue);
                }
                self.stage = Stage::ArgumentNumber;
                Ok(Action::ToNumber(value))
            }
            Stage::ArgumentNumber => {
                let Value::Number(number) = self.value(input)? else {
                    return Err(Fault::Internal("array mutator expected a number"));
                };
                self.numbers[self.argument] = number;
                self.argument += 1;
                self.stage = Stage::Arguments;
                Ok(Action::Continue)
            }
            Stage::Prepare => {
                match self.which {
                    ArrayPrototypePop | ArrayPrototypeShift => {
                        if self.length == 0 {
                            self.stage = Stage::SetLength;
                        } else {
                            self.new_length -= 1;
                            let index = if self.which == ArrayPrototypePop {
                                self.new_length
                            } else {
                                0
                            };
                            return self.read(
                                heap,
                                task,
                                self.this.clone(),
                                index_key(index),
                                Stage::Saved,
                            );
                        }
                    }
                    ArrayPrototypeUnshift => {
                        self.items = self.args.len() as u64;
                        self.new_length()?;
                        self.movement(
                            0,
                            self.items,
                            if self.items == 0 { 0 } else { self.length },
                            true,
                            Stage::Insert,
                        );
                    }
                    ArrayPrototypeSplice => {
                        self.start = Self::relative(self.numbers[0], self.length);
                        self.delete = match self.args.len() {
                            0 => 0,
                            1 => self.length - self.start,
                            _ => {
                                let n = self.numbers[1];
                                if n.is_nan() || n <= 0.0 {
                                    0
                                } else {
                                    n.trunc().min((self.length - self.start) as f64) as u64
                                }
                            }
                        };
                        self.items = self.args.len().saturating_sub(2) as u64;
                        self.new_length()?;
                        if builtin::is_array(heap, &self.this)? {
                            return self.read(
                                heap,
                                task,
                                self.this.clone(),
                                property::Key::Name("constructor".into()),
                                Stage::Constructor,
                            );
                        }
                        self.stage = Stage::CreateRemoved;
                    }
                    ArrayPrototypeReverse => {
                        self.end = self.length / 2;
                        self.stage = Stage::Reverse;
                    }
                    ArrayPrototypeFill => {
                        self.index = Self::relative(self.numbers[0], self.length);
                        self.end = Self::relative(self.numbers[1], self.length);
                        self.stage = Stage::Fill;
                    }
                    ArrayPrototypeCopyWithin => {
                        let to = Self::relative(self.numbers[0], self.length);
                        let from = Self::relative(self.numbers[1], self.length);
                        let end = Self::relative(self.numbers[2], self.length);
                        let count = end.saturating_sub(from).min(self.length - to);
                        self.movement(from, to, count, from < to && to < from + count, Stage::Done);
                    }
                    _ => return Err(Fault::Internal("not an array mutator")),
                }
                Ok(Action::Continue)
            }
            Stage::Saved => {
                self.result = self.value(input)?;
                self.index = self.length;
                if self.which == ArrayPrototypeShift {
                    self.movement(1, 0, self.new_length, false, Stage::DeleteTail);
                } else {
                    self.stage = Stage::DeleteTail;
                }
                Ok(Action::Continue)
            }
            Stage::Move => {
                if self.remaining == 0 {
                    self.stage = self.after_move;
                    return Ok(Action::Continue);
                }
                self.stage = Stage::MovePresent;
                Ok(has_property(self.this.clone(), index_key(self.from)))
            }
            Stage::MovePresent => {
                if self.value(input)?.truthy() {
                    self.read(
                        heap,
                        task,
                        self.this.clone(),
                        index_key(self.from),
                        Stage::MoveValue,
                    )
                } else {
                    self.delete_at(heap, task, self.to)?;
                    self.stage = Stage::MoveAdvance;
                    Ok(Action::Continue)
                }
            }
            Stage::MoveValue => {
                self.stage = Stage::MoveAdvance;
                let value = self.value(input)?;
                Ok(self.set(index_key(self.to), value))
            }
            Stage::MoveAdvance => {
                self.remaining -= 1;
                if self.remaining != 0 {
                    if self.backward {
                        self.from -= 1;
                        self.to -= 1;
                    } else {
                        self.from += 1;
                        self.to += 1;
                    }
                }
                self.stage = Stage::Move;
                Ok(Action::Continue)
            }
            Stage::DeleteTail => {
                if self.index > self.new_length {
                    self.index -= 1;
                    self.delete_at(heap, task, self.index)?;
                } else {
                    self.index = 0;
                    self.stage = Stage::Insert;
                }
                Ok(Action::Continue)
            }
            Stage::Insert => {
                if self.index == self.items {
                    self.stage = Stage::SetLength;
                    return Ok(Action::Continue);
                }
                let offset = if self.which == ArrayPrototypeSplice {
                    2
                } else {
                    0
                };
                let value = self.args[self.index as usize + offset].clone();
                let index = self.start + self.index;
                self.index += 1;
                Ok(self.set(index_key(index), value))
            }
            Stage::SetLength => {
                self.stage = Stage::Done;
                Ok(self.set(
                    property::Key::Name("length".into()),
                    Value::Number(self.new_length as f64),
                ))
            }
            Stage::Done => Ok(Action::Return(match self.which {
                ArrayPrototypePop | ArrayPrototypeShift | ArrayPrototypeSplice => {
                    self.result.clone()
                }
                ArrayPrototypeUnshift => Value::Number(self.new_length as f64),
                _ => self.this.clone(),
            })),
            Stage::Reverse => {
                if self.index == self.end {
                    return Ok(Action::Return(self.this.clone()));
                }
                self.lower = Value::Undefined;
                self.upper = Value::Undefined;
                self.stage = Stage::ReverseLowerPresent;
                Ok(has_property(self.this.clone(), index_key(self.index)))
            }
            Stage::ReverseLowerPresent => {
                self.lower_present = self.value(input)?.truthy();
                if self.lower_present {
                    self.read(
                        heap,
                        task,
                        self.this.clone(),
                        index_key(self.index),
                        Stage::ReverseLower,
                    )
                } else {
                    self.stage = Stage::ReverseUpper;
                    Ok(Action::Continue)
                }
            }
            Stage::ReverseLower => {
                self.lower = self.value(input)?;
                self.stage = Stage::ReverseUpper;
                Ok(Action::Continue)
            }
            Stage::ReverseUpper => {
                self.stage = Stage::ReverseUpperPresent;
                Ok(has_property(
                    self.this.clone(),
                    index_key(self.length - self.index - 1),
                ))
            }
            Stage::ReverseUpperPresent => {
                self.upper_present = self.value(input)?.truthy();
                if self.upper_present {
                    self.read(
                        heap,
                        task,
                        self.this.clone(),
                        index_key(self.length - self.index - 1),
                        Stage::ReverseUpperValue,
                    )
                } else {
                    self.stage = Stage::ReverseWriteLower;
                    Ok(Action::Continue)
                }
            }
            Stage::ReverseUpperValue => {
                self.upper = self.value(input)?;
                self.stage = Stage::ReverseWriteLower;
                Ok(Action::Continue)
            }
            Stage::ReverseWriteLower => {
                self.stage = Stage::ReverseWriteUpper;
                if self.upper_present {
                    Ok(self.set(index_key(self.index), self.upper.clone()))
                } else {
                    if self.lower_present {
                        self.delete_at(heap, task, self.index)?;
                    }
                    Ok(Action::Continue)
                }
            }
            Stage::ReverseWriteUpper => {
                self.stage = Stage::ReverseAdvance;
                let index = self.length - self.index - 1;
                if self.lower_present {
                    Ok(self.set(index_key(index), self.lower.clone()))
                } else {
                    if self.upper_present {
                        self.delete_at(heap, task, index)?;
                    }
                    Ok(Action::Continue)
                }
            }
            Stage::ReverseAdvance => {
                self.index += 1;
                self.stage = Stage::Reverse;
                Ok(Action::Continue)
            }
            Stage::Fill => {
                if self.index >= self.end {
                    return Ok(Action::Return(self.this.clone()));
                }
                let index = self.index;
                self.index += 1;
                Ok(self.set(
                    index_key(index),
                    self.args.first().cloned().unwrap_or(Value::Undefined),
                ))
            }
            Stage::Constructor => {
                let constructor = self.value(input)?;
                if matches!(constructor, Value::Object(_)) {
                    return self.read(
                        heap,
                        task,
                        constructor,
                        property::Key::Symbol(crate::value::symbol::well_known("species")),
                        Stage::Species,
                    );
                }
                if !matches!(constructor, Value::Undefined) {
                    return Err(Fault::TypeError(
                        "array constructor is not an object".into(),
                    ));
                }
                self.stage = Stage::CreateRemoved;
                Ok(Action::Continue)
            }
            Stage::Species => {
                let species = self.value(input)?;
                if !matches!(species, Value::Undefined | Value::Null) {
                    self.stage = Stage::CreatedRemoved;
                    return Ok(Action::Construct {
                        callee: species,
                        args: vec![Value::Number(self.delete as f64)],
                    });
                }
                self.stage = Stage::CreateRemoved;
                Ok(Action::Continue)
            }
            Stage::CreateRemoved => {
                self.result =
                    builtin::array_constructor(heap, task, &[Value::Number(self.delete as f64)])?;
                self.stage = Stage::Removed;
                Ok(Action::Continue)
            }
            Stage::CreatedRemoved => {
                self.result = self.value(input)?;
                self.stage = Stage::Removed;
                Ok(Action::Continue)
            }
            Stage::Removed => {
                if self.index == self.delete {
                    self.stage = Stage::SpliceMove;
                    return Ok(Action::SetProperty {
                        this: self.result.clone(),
                        key: property::Key::Name("length".into()),
                        value: Value::Number(self.delete as f64),
                        strict: true,
                    });
                }
                self.stage = Stage::RemovedPresent;
                Ok(has_property(
                    self.this.clone(),
                    index_key(self.start + self.index),
                ))
            }
            Stage::RemovedPresent => {
                if self.value(input)?.truthy() {
                    self.read(
                        heap,
                        task,
                        self.this.clone(),
                        index_key(self.start + self.index),
                        Stage::RemovedValue,
                    )
                } else {
                    self.index += 1;
                    self.stage = Stage::Removed;
                    Ok(Action::Continue)
                }
            }
            Stage::RemovedValue => {
                let key = Value::Number(self.index as f64);
                let value = self.value(input)?;
                self.index += 1;
                self.stage = Stage::Removed;
                Ok(Action::CreateDataProperty {
                    this: self.result.clone(),
                    key,
                    value,
                })
            }
            Stage::SpliceMove => {
                self.index = self.length;
                self.movement(
                    self.start + self.delete,
                    self.start + self.items,
                    if self.items == self.delete {
                        0
                    } else {
                        self.length - self.start - self.delete
                    },
                    self.items > self.delete,
                    Stage::DeleteTail,
                );
                Ok(Action::Continue)
            }
        }
    }
}
