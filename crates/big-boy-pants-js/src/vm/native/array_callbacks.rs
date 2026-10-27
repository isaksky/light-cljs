//! Array callback iteration. Saved values belong to the task and are traced;
//! each property read, callback and output element advances a budgeted step.
use super::builtin::Builtin;
use super::*;

#[derive(Clone, Copy, Debug)]
enum Stage {
    Start,
    Length,
    LengthNumber,
    Constructor,
    Species,
    Create,
    Created,
    Scan,
    Present,
    Element,
    Callback,
    Advance,
    InnerLength,
    InnerLengthNumber,
    InnerScan,
    InnerPresent,
    InnerElement,
}

#[derive(Debug)]
pub(super) struct State {
    which: Builtin,
    stage: Stage,
    this: Value,
    callback: Value,
    this_arg: Value,
    accumulator: Value,
    has_accumulator: bool,
    result: Value,
    element: Value,
    inner: Value,
    read_value: Option<Value>,
    length: i64,
    index: i64,
    output: u64,
    inner_length: u64,
    inner_index: u64,
}

impl Trace for State {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        for value in [
            &self.this,
            &self.callback,
            &self.this_arg,
            &self.accumulator,
            &self.result,
            &self.element,
            &self.inner,
        ] {
            value.trace(visit);
        }
        if let Some(value) = &self.read_value {
            value.trace(visit);
        }
    }
}

impl State {
    pub(super) fn new(which: Builtin, this: Value, args: &[Value]) -> Self {
        let reduce = matches!(
            which,
            Builtin::ArrayPrototypeReduce | Builtin::ArrayPrototypeReduceRight
        );
        Self {
            which,
            stage: Stage::Start,
            this,
            callback: args.first().cloned().unwrap_or(Value::Undefined),
            this_arg: if reduce {
                Value::Undefined
            } else {
                args.get(1).cloned().unwrap_or(Value::Undefined)
            },
            accumulator: if reduce {
                args.get(1).cloned().unwrap_or(Value::Undefined)
            } else {
                Value::Undefined
            },
            has_accumulator: reduce && args.len() > 1,
            result: Value::Undefined,
            element: Value::Undefined,
            inner: Value::Undefined,
            read_value: None,
            length: 0,
            index: 0,
            output: 0,
            inner_length: 0,
            inner_index: 0,
        }
    }

    fn reduce(&self) -> bool {
        matches!(
            self.which,
            Builtin::ArrayPrototypeReduce | Builtin::ArrayPrototypeReduceRight
        )
    }

    fn transforms(&self) -> bool {
        matches!(
            self.which,
            Builtin::ArrayPrototypeMap
                | Builtin::ArrayPrototypeFilter
                | Builtin::ArrayPrototypeFlatMap
        )
    }

    fn result_length(&self) -> u64 {
        if self.which == Builtin::ArrayPrototypeMap {
            self.length as u64
        } else {
            0
        }
    }

    fn value(&mut self, input: Input) -> Result<Value, Fault> {
        match input {
            Input::CallResult(value) => Ok(value),
            Input::Start => self
                .read_value
                .take()
                .ok_or(Fault::Internal("array callback expected a saved read")),
            _ => Err(Fault::Internal("array callback expected a call result")),
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

    fn define(&mut self, value: Value, next: Stage) -> Result<Action, Fault> {
        let index = if self.which == Builtin::ArrayPrototypeMap {
            self.index as u64
        } else {
            self.output
        };
        if index >= (1_u64 << 53) - 1 {
            return Err(Fault::TypeError(
                "array result exceeds 2^53 - 1 elements".into(),
            ));
        }
        self.output += 1;
        self.stage = next;
        Ok(Action::CreateDataProperty {
            this: self.result.clone(),
            key: Value::Number(index as f64),
            value,
        })
    }

    fn advance(&mut self) -> Action {
        self.index += if self.which == Builtin::ArrayPrototypeReduceRight {
            -1
        } else {
            1
        };
        self.element = Value::Undefined;
        self.inner = Value::Undefined;
        self.stage = Stage::Scan;
        Action::Continue
    }

    fn call(&mut self, element: Value) -> Action {
        self.element = element;
        if self.reduce() && !self.has_accumulator {
            self.accumulator = self.element.clone();
            self.has_accumulator = true;
            return self.advance();
        }
        let (args, argc) = if self.reduce() {
            (
                [
                    self.accumulator.clone(),
                    self.element.clone(),
                    Value::Number(self.index as f64),
                    self.this.clone(),
                ],
                4,
            )
        } else {
            (
                [
                    self.element.clone(),
                    Value::Number(self.index as f64),
                    self.this.clone(),
                    Value::Undefined,
                ],
                3,
            )
        };
        self.stage = Stage::Callback;
        Action::CallArray {
            callee: self.callback.clone(),
            this: self.this_arg.clone(),
            args,
            argc,
        }
    }

    pub(super) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
    ) -> Result<Action, Fault> {
        // Keep unrelated stage storage out of nested property/conversion paths
        // on small workers, as in the main native dispatcher.
        match self.stage {
            Stage::Start | Stage::Length | Stage::LengthNumber => self.prepare(heap, task, input),
            Stage::Constructor | Stage::Species | Stage::Create | Stage::Created => {
                self.create(heap, task, input)
            }
            Stage::InnerLength
            | Stage::InnerLengthNumber
            | Stage::InnerScan
            | Stage::InnerPresent
            | Stage::InnerElement => self.flatten(heap, task, input),
            _ => self.iterate(heap, task, input),
        }
    }

    #[inline(never)]
    fn prepare(&mut self, heap: &Heap, task: TaskId, input: Input) -> Result<Action, Fault> {
        use Builtin::*;
        match self.stage {
            Stage::Start => {
                self.this = builtin::array_string_receiver(heap, task, &self.this)?;
                self.stage = Stage::Length;
                match property::names::LENGTH.read(heap, task, &self.this)? {
                    Some(property::Read::Deferred(callee)) => Ok(callee.action(self.this.clone())),
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
            Stage::Length => {
                self.stage = Stage::LengthNumber;
                Ok(Action::ToNumber(self.value(input)?))
            }
            Stage::LengthNumber => {
                self.length = to_length(&self.value(input)?)? as i64;
                // Check even for an empty receiver, before consulting species.
                dispatch::callee(heap, &self.callback)?;
                self.index = if self.which == ArrayPrototypeReduceRight {
                    self.length - 1
                } else {
                    0
                };
                if self.transforms() {
                    if builtin::is_array(heap, &self.this)? {
                        return self.read(
                            heap,
                            task,
                            self.this.clone(),
                            property::Key::Name("constructor".into()),
                            Stage::Constructor,
                        );
                    }
                    self.stage = Stage::Create;
                } else {
                    self.stage = Stage::Scan;
                }
                Ok(Action::Continue)
            }
            _ => unreachable!("not an array callback preparation stage"),
        }
    }

    #[inline(never)]
    fn create(&mut self, heap: &Heap, task: TaskId, input: Input) -> Result<Action, Fault> {
        match self.stage {
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
                self.stage = Stage::Create;
                Ok(Action::Continue)
            }
            Stage::Species => {
                let species = self.value(input)?;
                if !matches!(species, Value::Undefined | Value::Null) {
                    self.stage = Stage::Created;
                    return Ok(Action::Construct {
                        callee: species,
                        args: vec![Value::Number(self.result_length() as f64)],
                    });
                }
                self.stage = Stage::Create;
                Ok(Action::Continue)
            }
            Stage::Create => {
                self.result = builtin::array_constructor(
                    heap,
                    task,
                    &[Value::Number(self.result_length() as f64)],
                )?;
                self.stage = Stage::Scan;
                Ok(Action::Continue)
            }
            Stage::Created => {
                self.result = self.value(input)?;
                self.stage = Stage::Scan;
                Ok(Action::Continue)
            }
            _ => unreachable!("not an array callback species stage"),
        }
    }

    fn scan(&mut self, heap: &Heap, task: TaskId) -> Result<Action, Fault> {
        use Builtin::*;
        if self.index < 0 || self.index >= self.length {
            return Ok(Action::Return(match self.which {
                ArrayPrototypeEvery => Value::Bool(true),
                ArrayPrototypeSome => Value::Bool(false),
                ArrayPrototypeReduce | ArrayPrototypeReduceRight => {
                    if !self.has_accumulator {
                        return Err(Fault::TypeError(
                            "reduce of empty array with no initial value".into(),
                        ));
                    }
                    self.accumulator.clone()
                }
                _ => self.result.clone(),
            }));
        }
        // A present own data element answers both HasProperty and Get
        // without user code. Re-read on every visit: callbacks may
        // delete elements, install accessors or truncate the array.
        // This step visits at most one element, retaining a poll and
        // budget boundary between callbacks, including skipped holes.
        if let Value::Object(id) = self.this
            && self.index < i64::from(u32::MAX)
            && let Some(value) = heap
                .read(id, |object| match object {
                    HeapObject::Array(array)
                        if array.object().owner().is_none_or(|owner| owner == task) =>
                    {
                        array
                            .element(self.index as u32)
                            .filter(|property| property.accessor.is_none())
                            .map(|property| property.value)
                    }
                    _ => None,
                })
                .flatten()
        {
            return Ok(self.call(value));
        }
        self.stage = Stage::Present;
        let key = index_key(self.index as u64);
        match property::probe(heap, task, &self.this, &key)? {
            property::Presence::Value(present) => {
                self.read_value = Some(Value::Bool(present));
                Ok(Action::Continue)
            }
            // HasProperty must still call traps on either the receiver
            // or its prototype chain, through the resumable In path.
            property::Presence::Proxy(_) => Ok(has_property(self.this.clone(), key)),
        }
    }

    #[inline(never)]
    fn iterate(&mut self, heap: &Heap, task: TaskId, input: Input) -> Result<Action, Fault> {
        use Builtin::*;
        match self.stage {
            Stage::Scan => self.scan(heap, task),
            Stage::Present => {
                if !self.value(input)?.truthy() {
                    return Ok(self.advance());
                }
                self.read(
                    heap,
                    task,
                    self.this.clone(),
                    index_key(self.index as u64),
                    Stage::Element,
                )
            }
            Stage::Element => {
                let element = self.value(input)?;
                Ok(self.call(element))
            }
            Stage::Callback => {
                let value = self.value(input)?;
                match self.which {
                    ArrayPrototypeSome if value.truthy() => {
                        return Ok(Action::Return(Value::Bool(true)));
                    }
                    ArrayPrototypeEvery if !value.truthy() => {
                        return Ok(Action::Return(Value::Bool(false)));
                    }
                    ArrayPrototypeMap => return self.define(value, Stage::Advance),
                    ArrayPrototypeFilter if value.truthy() => {
                        return self.define(self.element.clone(), Stage::Advance);
                    }
                    ArrayPrototypeReduce | ArrayPrototypeReduceRight => self.accumulator = value,
                    ArrayPrototypeFlatMap => {
                        if builtin::is_array(heap, &value)? {
                            self.inner = value;
                            return self.read(
                                heap,
                                task,
                                self.inner.clone(),
                                property::Key::Name("length".into()),
                                Stage::InnerLength,
                            );
                        }
                        return self.define(value, Stage::Advance);
                    }
                    _ => {}
                }
                self.advance();
                self.scan(heap, task)
            }
            Stage::Advance => {
                self.advance();
                self.scan(heap, task)
            }
            _ => unreachable!("not an array callback iteration stage"),
        }
    }

    #[inline(never)]
    fn flatten(&mut self, heap: &Heap, task: TaskId, input: Input) -> Result<Action, Fault> {
        match self.stage {
            Stage::InnerLength => {
                self.stage = Stage::InnerLengthNumber;
                Ok(Action::ToNumber(self.value(input)?))
            }
            Stage::InnerLengthNumber => {
                self.inner_length = to_length(&self.value(input)?)?;
                self.inner_index = 0;
                self.stage = Stage::InnerScan;
                Ok(Action::Continue)
            }
            Stage::InnerScan => {
                if self.inner_index == self.inner_length {
                    return Ok(self.advance());
                }
                let key = index_key(self.inner_index);
                self.inner_index += 1;
                self.stage = Stage::InnerPresent;
                Ok(has_property(self.inner.clone(), key))
            }
            Stage::InnerPresent => {
                if !self.value(input)?.truthy() {
                    self.stage = Stage::InnerScan;
                    return Ok(Action::Continue);
                }
                self.read(
                    heap,
                    task,
                    self.inner.clone(),
                    index_key(self.inner_index - 1),
                    Stage::InnerElement,
                )
            }
            Stage::InnerElement => {
                let value = self.value(input)?;
                self.define(value, Stage::InnerScan)
            }
            _ => unreachable!("not an array callback flattening stage"),
        }
    }
}
