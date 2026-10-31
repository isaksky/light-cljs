//! Budgeted Array copying, concatenation and flattening. All observable reads,
//! conversions, construction and definitions resume through the VM trampoline.
use super::builtin::Builtin;
use super::*;

const MAX_LENGTH: u64 = (1_u64 << 53) - 1;

#[derive(Clone, Copy, Debug)]
enum Stage {
    Start,
    Length,
    LengthNumber,
    Argument,
    ArgumentNumber,
    SpeciesStart,
    Constructor,
    Species,
    Create,
    Created,
    Ready,
    CopyScan,
    CopyPresent,
    CopyElement,
    ConcatItem,
    Spread,
    SpreadLength,
    SpreadLengthNumber,
    FlatScan,
    FlatPresent,
    FlatElement,
    InnerLength,
    InnerLengthNumber,
    OfConstructor,
    OfScan,
    SetLength,
    Done,
}

#[derive(Debug)]
struct Source {
    value: Value,
    length: u64,
    index: u64,
    depth: f64,
}

#[derive(Debug)]
pub(super) struct State {
    which: Builtin,
    stage: Stage,
    this: Value,
    args: Vec<Value>,
    result: Value,
    source: Value,
    element: Value,
    probe: Value,
    read_value: Option<Value>,
    frames: Vec<Source>,
    limit: usize,
    hops: usize,
    length: u64,
    start: u64,
    end: u64,
    index: u64,
    output: u64,
    item: usize,
    argument: usize,
    depth: f64,
    result_length: u64,
}

impl Trace for State {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        for value in [
            &self.this,
            &self.result,
            &self.source,
            &self.element,
            &self.probe,
        ]
        .into_iter()
        .chain(&self.args)
        {
            value.trace(visit);
        }
        if let Some(value) = &self.read_value {
            value.trace(visit);
        }
        for frame in &self.frames {
            frame.value.trace(visit);
        }
    }
}

impl State {
    pub(super) fn new(which: Builtin, this: Value, args: &[Value], limit: usize) -> Self {
        Self {
            which,
            stage: Stage::Start,
            this,
            args: args.to_vec(),
            result: Value::Undefined,
            source: Value::Undefined,
            element: Value::Undefined,
            probe: Value::Undefined,
            read_value: None,
            frames: Vec::new(),
            limit,
            hops: 0,
            length: 0,
            start: 0,
            end: 0,
            index: 0,
            output: 0,
            item: 0,
            argument: 0,
            depth: 1.0,
            result_length: 0,
        }
    }

    fn value(&mut self, input: Input) -> Result<Value, Fault> {
        match input {
            Input::CallResult(value) => Ok(value),
            Input::Start => self
                .read_value
                .take()
                .ok_or_else(|| Fault::Internal("array copy expected saved read")),
            _ => Err(Fault::Internal("array copy expected call result")),
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

    fn read_length(
        &mut self,
        heap: &Heap,
        task: TaskId,
        base: Value,
        next: Stage,
    ) -> Result<Action, Fault> {
        self.read(heap, task, base, property::Key::Name("length".into()), next)
    }

    fn define(&mut self, value: Value, next: Stage) -> Result<Action, Fault> {
        if self.output >= MAX_LENGTH {
            return Err(Fault::TypeError(
                "array result exceeds 2^53 - 1 elements".into(),
            ));
        }
        let index = self.output;
        self.output += 1;
        self.stage = next;
        Ok(Action::CreateDataProperty {
            this: self.result.clone(),
            key: Value::Number(index as f64),
            value,
        })
    }

    fn integer(value: Value) -> Result<f64, Fault> {
        let Value::Number(number) = value else {
            return Err(Fault::Internal("array copy expected number"));
        };
        Ok(if number.is_nan() { 0.0 } else { number.trunc() })
    }

    fn relative(number: f64, length: u64) -> u64 {
        if number < 0.0 {
            (length as f64 + number).max(0.0) as u64
        } else {
            number.min(length as f64) as u64
        }
    }

    pub(super) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
    ) -> Result<Action, Fault> {
        match self.stage {
            Stage::Start
            | Stage::Length
            | Stage::LengthNumber
            | Stage::Argument
            | Stage::ArgumentNumber => self.prepare(heap, task, input),
            Stage::SpeciesStart
            | Stage::Constructor
            | Stage::Species
            | Stage::Create
            | Stage::Created
            | Stage::Ready
            | Stage::OfConstructor => self.create(heap, task, input),
            Stage::FlatScan
            | Stage::FlatPresent
            | Stage::FlatElement
            | Stage::InnerLength
            | Stage::InnerLengthNumber => self.flatten(heap, task, input),
            _ => self.copy(heap, task, input),
        }
    }

    #[inline(never)]
    fn prepare(&mut self, heap: &Heap, task: TaskId, input: Input) -> Result<Action, Fault> {
        use Builtin::*;
        match self.stage {
            Stage::Start => {
                if self.which == ArrayOf {
                    self.result_length = self.args.len() as u64;
                    self.probe = self.this.clone();
                    self.stage = Stage::OfConstructor;
                    return Ok(Action::Continue);
                }
                self.this = builtin::array_string_receiver(heap, task, &self.this)?;
                if self.which == ArrayPrototypeConcat {
                    self.stage = Stage::SpeciesStart;
                    return Ok(Action::Continue);
                }
                self.read_length(heap, task, self.this.clone(), Stage::Length)
            }
            Stage::Length => {
                self.stage = Stage::LengthNumber;
                Ok(Action::ToNumber(self.value(input)?))
            }
            Stage::LengthNumber => {
                self.length = to_length(&self.value(input)?)?;
                self.end = self.length;
                self.stage = if self.which == ArrayPrototypeToReversed {
                    Stage::SpeciesStart
                } else {
                    Stage::Argument
                };
                self.result_length = self.length;
                Ok(Action::Continue)
            }
            Stage::Argument => {
                let arg = self
                    .args
                    .get(self.argument)
                    .cloned()
                    .unwrap_or(Value::Undefined);
                if matches!(arg, Value::Undefined) {
                    if self.which == ArrayPrototypeFlat {
                        self.result_length = 0;
                        self.stage = Stage::SpeciesStart;
                        return Ok(Action::Continue);
                    }
                    if self.which == ArrayPrototypeSlice && self.argument == 1 {
                        self.result_length = self.end.saturating_sub(self.start);
                        self.stage = Stage::SpeciesStart;
                        return Ok(Action::Continue);
                    }
                }
                self.stage = Stage::ArgumentNumber;
                Ok(Action::ToNumber(arg))
            }
            Stage::ArgumentNumber => {
                let number = Self::integer(self.value(input)?)?;
                match self.which {
                    ArrayPrototypeSlice => {
                        if self.argument == 0 {
                            self.start = Self::relative(number, self.length);
                            self.argument = 1;
                            self.stage = Stage::Argument;
                            return Ok(Action::Continue);
                        }
                        self.end = Self::relative(number, self.length);
                        self.result_length = self.end.saturating_sub(self.start);
                    }
                    ArrayPrototypeFlat => {
                        self.depth = number.max(0.0);
                        self.result_length = 0;
                    }
                    ArrayPrototypeWith => {
                        let index = if number < 0.0 {
                            self.length as f64 + number
                        } else {
                            number
                        };
                        if index < 0.0 || index >= self.length as f64 {
                            return Err(Fault::RangeError("invalid array index".into()));
                        }
                        self.start = index as u64;
                    }
                    _ => unreachable!(),
                }
                self.stage = Stage::SpeciesStart;
                Ok(Action::Continue)
            }
            _ => unreachable!(),
        }
    }

    #[inline(never)]
    fn create(&mut self, heap: &Heap, task: TaskId, input: Input) -> Result<Action, Fault> {
        use Builtin::*;
        match self.stage {
            Stage::SpeciesStart => {
                if matches!(
                    self.which,
                    ArrayPrototypeSlice | ArrayPrototypeConcat | ArrayPrototypeFlat
                ) && builtin::is_array(heap, &self.this)?
                {
                    return self.read(
                        heap,
                        task,
                        self.this.clone(),
                        property::Key::Name("constructor".into()),
                        Stage::Constructor,
                    );
                }
                self.stage = Stage::Create;
                Ok(Action::Continue)
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
                self.stage = Stage::Create;
                Ok(Action::Continue)
            }
            Stage::Species => {
                let species = self.value(input)?;
                if !matches!(species, Value::Null | Value::Undefined) {
                    self.stage = Stage::Created;
                    return Ok(Action::Construct {
                        callee: species,
                        args: vec![Value::Number(self.result_length as f64)],
                    });
                }
                self.stage = Stage::Create;
                Ok(Action::Continue)
            }
            Stage::OfConstructor => {
                // Probe one bound target per budgeted step, but construct the
                // original receiver so bound arguments and new.target survive.
                if let Ok(Callee::Bound(bound)) = dispatch::callee(heap, &self.probe) {
                    if self.hops >= self.limit {
                        return Err(Fault::StackOverflow(
                            "maximum bound constructor depth exceeded".into(),
                        ));
                    }
                    self.hops += 1;
                    self.probe = bound.callee;
                    return Ok(Action::Continue);
                }
                if dispatch::is_constructor(heap, &self.probe, 1)? {
                    self.stage = Stage::Created;
                    return Ok(Action::Construct {
                        callee: self.this.clone(),
                        args: vec![Value::Number(self.result_length as f64)],
                    });
                }
                self.stage = Stage::Create;
                Ok(Action::Continue)
            }
            Stage::Create => {
                self.result = builtin::array_constructor(
                    heap,
                    task,
                    &[Value::Number(self.result_length as f64)],
                )?;
                self.stage = Stage::Ready;
                Ok(Action::Continue)
            }
            Stage::Created => {
                self.result = self.value(input)?;
                self.stage = Stage::Ready;
                Ok(Action::Continue)
            }
            Stage::Ready => {
                self.source = self.this.clone();
                self.stage = match self.which {
                    ArrayOf => Stage::OfScan,
                    ArrayPrototypeConcat => Stage::ConcatItem,
                    ArrayPrototypeFlat => {
                        self.frames.push(Source {
                            value: self.this.clone(),
                            length: self.length,
                            index: 0,
                            depth: self.depth,
                        });
                        Stage::FlatScan
                    }
                    _ => {
                        self.index = if self.which == ArrayPrototypeSlice {
                            self.start
                        } else {
                            0
                        };
                        Stage::CopyScan
                    }
                };
                Ok(Action::Continue)
            }
            _ => unreachable!(),
        }
    }

    #[inline(never)]
    fn copy(&mut self, heap: &Heap, task: TaskId, input: Input) -> Result<Action, Fault> {
        use Builtin::*;
        match self.stage {
            Stage::ConcatItem => {
                if self.item > self.args.len() {
                    self.stage = Stage::SetLength;
                    return Ok(Action::Continue);
                }
                self.source = if self.item == 0 {
                    self.this.clone()
                } else {
                    self.args[self.item - 1].clone()
                };
                self.item += 1;
                if matches!(self.source, Value::Object(_)) {
                    return self.read(
                        heap,
                        task,
                        self.source.clone(),
                        property::Key::Symbol(crate::value::symbol::well_known(
                            "isConcatSpreadable",
                        )),
                        Stage::Spread,
                    );
                }
                self.define(self.source.clone(), Stage::ConcatItem)
            }
            Stage::Spread => {
                let value = self.value(input)?;
                let spread = if matches!(value, Value::Undefined) {
                    builtin::is_array(heap, &self.source)?
                } else {
                    value.truthy()
                };
                if spread {
                    self.read_length(heap, task, self.source.clone(), Stage::SpreadLength)
                } else {
                    self.define(self.source.clone(), Stage::ConcatItem)
                }
            }
            Stage::SpreadLength => {
                self.stage = Stage::SpreadLengthNumber;
                Ok(Action::ToNumber(self.value(input)?))
            }
            Stage::SpreadLengthNumber => {
                self.end = to_length(&self.value(input)?)?;
                if self.end > MAX_LENGTH - self.output {
                    return Err(Fault::TypeError(
                        "array result exceeds 2^53 - 1 elements".into(),
                    ));
                }
                self.index = 0;
                self.stage = Stage::CopyScan;
                Ok(Action::Continue)
            }
            Stage::CopyScan => {
                if self.index >= self.end {
                    self.stage = if self.which == ArrayPrototypeConcat {
                        Stage::ConcatItem
                    } else if self.which == ArrayPrototypeSlice {
                        Stage::SetLength
                    } else {
                        Stage::Done
                    };
                    return Ok(Action::Continue);
                }
                let index = self.index;
                self.index += 1;
                if self.which == ArrayPrototypeWith && index == self.start {
                    return self.define(
                        self.args.get(1).cloned().unwrap_or(Value::Undefined),
                        Stage::CopyScan,
                    );
                }
                let key = index_key(if self.which == ArrayPrototypeToReversed {
                    self.length - index - 1
                } else {
                    index
                });
                if matches!(self.which, ArrayPrototypeSlice | ArrayPrototypeConcat) {
                    self.stage = Stage::CopyPresent;
                    return Ok(has_property(self.source.clone(), key));
                }
                self.read(heap, task, self.source.clone(), key, Stage::CopyElement)
            }
            Stage::CopyPresent => {
                if !self.value(input)?.truthy() {
                    self.output += 1;
                    self.stage = Stage::CopyScan;
                    return Ok(Action::Continue);
                }
                self.read(
                    heap,
                    task,
                    self.source.clone(),
                    index_key(self.index - 1),
                    Stage::CopyElement,
                )
            }
            Stage::CopyElement => {
                let value = self.value(input)?;
                self.define(value, Stage::CopyScan)
            }
            Stage::OfScan => {
                if self.output == self.args.len() as u64 {
                    self.stage = Stage::SetLength;
                    return Ok(Action::Continue);
                }
                self.define(self.args[self.output as usize].clone(), Stage::OfScan)
            }
            Stage::SetLength => {
                self.stage = Stage::Done;
                Ok(Action::SetProperty {
                    this: self.result.clone(),
                    key: property::Key::Name("length".into()),
                    value: Value::Number(self.output as f64),
                    strict: true,
                })
            }
            Stage::Done => Ok(Action::Return(self.result.clone())),
            _ => unreachable!(),
        }
    }

    #[inline(never)]
    fn flatten(&mut self, heap: &Heap, task: TaskId, input: Input) -> Result<Action, Fault> {
        match self.stage {
            Stage::FlatScan => {
                let Some(frame) = self.frames.last_mut() else {
                    return Ok(Action::Return(self.result.clone()));
                };
                if frame.index == frame.length {
                    self.frames.pop();
                    return Ok(Action::Continue);
                }
                let key = index_key(frame.index);
                frame.index += 1;
                self.stage = Stage::FlatPresent;
                Ok(has_property(frame.value.clone(), key))
            }
            Stage::FlatPresent => {
                if !self.value(input)?.truthy() {
                    self.stage = Stage::FlatScan;
                    return Ok(Action::Continue);
                }
                let frame = self.frames.last().unwrap();
                self.read(
                    heap,
                    task,
                    frame.value.clone(),
                    index_key(frame.index - 1),
                    Stage::FlatElement,
                )
            }
            Stage::FlatElement => {
                let value = self.value(input)?;
                if self.frames.last().unwrap().depth > 0.0 && builtin::is_array(heap, &value)? {
                    self.element = value;
                    return self.read_length(heap, task, self.element.clone(), Stage::InnerLength);
                }
                self.define(value, Stage::FlatScan)
            }
            Stage::InnerLength => {
                self.stage = Stage::InnerLengthNumber;
                Ok(Action::ToNumber(self.value(input)?))
            }
            Stage::InnerLengthNumber => {
                let length = to_length(&self.value(input)?)?;
                if self.frames.len() >= self.limit {
                    return Err(Fault::StackOverflow(
                        "maximum array flatten depth exceeded".into(),
                    ));
                }
                let depth = self.frames.last().unwrap().depth - 1.0;
                self.frames.push(Source {
                    value: std::mem::replace(&mut self.element, Value::Undefined),
                    length,
                    index: 0,
                    depth,
                });
                self.stage = Stage::FlatScan;
                Ok(Action::Continue)
            }
            _ => unreachable!(),
        }
    }
}
