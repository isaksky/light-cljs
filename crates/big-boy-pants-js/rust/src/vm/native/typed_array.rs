//! Budgeted typed-array construction, set and fill (clj-quickjs-168).
//! Each step reads/converts/writes at most one element. All callbacks use
//! ordinary VM frames, and all retained object edges are traced here.
use super::*;
use crate::object::typed_array::{self as ta, ElementType, View};

#[derive(Clone, Copy, Debug)]
enum Operation {
    Construct(ElementType),
    Set,
    Fill,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Source,
    IteratorMethod,
    IteratorObject,
    NextMethod,
    Next,
    NextResult,
    DoneValue,
    IteratorValue,
    Length,
    LengthValue,
    CollectValue,
    Offset,
    OffsetValue,
    Snapshot,
    Element,
    ElementValue,
    NumberValue,
    FillValue,
    FillStart,
    FillStartValue,
    FillEnd,
    FillEndValue,
    Fill,
}

#[derive(Debug)]
pub(super) struct TypedArray {
    op: Operation,
    stage: Stage,
    source: Value,
    target: Value,
    args: Vec<Value>,
    iterator: Value,
    next_method: Value,
    result: Value,
    values: Vec<Value>,
    raw: Vec<[u8; 8]>,
    source_view: Option<View>,
    target_view: Option<View>,
    len: usize,
    next: usize,
    offset: usize,
    target_len: usize,
    fill_value: Value,
}

impl Trace for TypedArray {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        for value in [
            &self.source,
            &self.target,
            &self.iterator,
            &self.next_method,
            &self.result,
            &self.fill_value,
        ] {
            value.trace(visit);
        }
        for value in self.args.iter().chain(&self.values) {
            value.trace(visit);
        }
    }
}

pub(super) fn receiver(heap: &Heap, task: TaskId, this: &Value) -> Result<View, Fault> {
    if let Value::Object(id) = this
        && let Some(view) = ta::view(heap, task, *id)?
    {
        return Ok(view);
    }
    Err(Fault::TypeError("method requires a typed array".into()))
}

fn attached(heap: &Heap, view: View) -> Result<(), Fault> {
    if view.is_detached(heap)? {
        Err(Fault::TypeError("ArrayBuffer is detached".into()))
    } else {
        Ok(())
    }
}

impl TypedArray {
    fn new(op: Operation, source: Value, target: Value, args: Vec<Value>, stage: Stage) -> Self {
        Self {
            op,
            source,
            target,
            args,
            stage,
            iterator: Value::Undefined,
            next_method: Value::Undefined,
            result: Value::Undefined,
            values: vec![],
            raw: vec![],
            source_view: None,
            target_view: None,
            len: 0,
            next: 0,
            offset: 0,
            target_len: 0,
            fill_value: Value::Undefined,
        }
    }

    pub(super) fn constructor(kind: ElementType, source: Value) -> Self {
        Self::new(
            Operation::Construct(kind),
            source,
            Value::Undefined,
            vec![],
            Stage::Source,
        )
    }

    pub(super) fn method(
        heap: &Heap,
        task: TaskId,
        which: builtin::Builtin,
        this: &Value,
        args: &[Value],
    ) -> Result<Self, Fault> {
        let view = receiver(heap, task, this)?;
        let fill = which == builtin::Builtin::TypedArrayFill;
        if fill {
            attached(heap, view)?;
        }
        let mut state = Self::new(
            if fill {
                Operation::Fill
            } else {
                Operation::Set
            },
            args.first().cloned().unwrap_or(Value::Undefined),
            this.clone(),
            args.iter().take(3).cloned().collect(),
            if fill {
                Stage::FillValue
            } else {
                Stage::Offset
            },
        );
        state.target_view = Some(view);
        state.target_len = view.length;
        Ok(state)
    }

    fn value(input: Input) -> Result<Value, Fault> {
        match input {
            Input::CallResult(v) => Ok(v),
            _ => Err(Fault::Internal("typed array expected call result")),
        }
    }

    fn arg(&self, index: usize) -> Value {
        self.args.get(index).cloned().unwrap_or(Value::Undefined)
    }

    fn check_length(&mut self, heap: &Heap, task: TaskId, length: u64) -> Result<(), Fault> {
        self.len = usize::try_from(length)
            .map_err(|_| Fault::RangeError("typed array too long".into()))?;
        match self.op {
            Operation::Set => {
                if self.offset > self.target_len || self.len > self.target_len - self.offset {
                    return Err(Fault::RangeError("source exceeds typed array".into()));
                }
                self.stage = if self.source_view.is_some() {
                    Stage::Snapshot
                } else {
                    Stage::Element
                };
            }
            Operation::Construct(kind) => {
                self.target =
                    ta::constructor(heap, task, kind, &[Value::Number(self.len as f64)], false)?;
                self.target_view = Some(receiver(heap, task, &self.target)?);
                self.stage = if self.source_view.is_some() {
                    Stage::Snapshot
                } else {
                    Stage::Element
                };
            }
            Operation::Fill => unreachable!(),
        }
        Ok(())
    }

    fn integer(value: &Value) -> Result<f64, Fault> {
        let n = ta::to_number(value)?;
        Ok(if n.is_nan() { 0.0 } else { n.trunc() })
    }

    fn relative(&self, value: &Value) -> Result<usize, Fault> {
        let n = Self::integer(value)?;
        Ok(if n < 0.0 {
            (self.target_len as f64 + n).max(0.0)
        } else {
            n.min(self.target_len as f64)
        } as usize)
    }

    pub(super) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        mut input: Input,
    ) -> Result<Action, Fault> {
        // Property reads fall through to their value stage. No loop contains
        // user code or visits more than one element in a native step.
        let read = match self.stage {
            Stage::Source => {
                if let Value::Object(id) = self.source
                    && let Some(view) = ta::view(heap, task, id)?
                {
                    attached(heap, view)?;
                    self.source_view = Some(view);
                    self.check_length(heap, task, view.length as u64)?;
                    return Ok(Action::Continue);
                }
                if matches!(self.source, Value::Null | Value::Undefined) {
                    return Err(Fault::TypeError("cannot convert source to object".into()));
                }
                if matches!(self.op, Operation::Construct(_)) {
                    self.stage = Stage::IteratorMethod;
                    Some((
                        self.source.clone(),
                        property::Key::Symbol(crate::value::symbol::well_known("iterator")),
                    ))
                } else {
                    self.stage = Stage::Length;
                    return Ok(Action::Continue);
                }
            }
            Stage::Length => {
                self.stage = Stage::LengthValue;
                Some((
                    self.source.clone(),
                    property::Key::Name(JsString::from("length")),
                ))
            }
            Stage::NextMethod => {
                self.stage = Stage::Next;
                Some((
                    self.iterator.clone(),
                    property::Key::Name(JsString::from("next")),
                ))
            }
            Stage::NextResult => {
                self.result = Self::value(input.clone())?;
                if !matches!(self.result, Value::Object(_)) {
                    return Err(Fault::TypeError("iterator result must be an object".into()));
                }
                self.stage = Stage::DoneValue;
                Some((
                    self.result.clone(),
                    property::Key::Name(JsString::from("done")),
                ))
            }
            Stage::IteratorValue => {
                self.stage = Stage::CollectValue;
                Some((
                    self.result.clone(),
                    property::Key::Name(JsString::from("value")),
                ))
            }
            Stage::Element
                if self.next < self.len && self.source_view.is_none() && self.values.is_empty() =>
            {
                self.stage = Stage::ElementValue;
                Some((self.source.clone(), index_key(self.next as u64)))
            }
            _ => None,
        };
        if let Some((this, key)) = read {
            input = match property::read(heap, task, &this, &key)? {
                Some(property::Read::Deferred(callee)) => {
                    return Ok(callee.action(this));
                }
                Some(property::Read::Value(value)) => Input::CallResult(value),
                None => Input::CallResult(Value::Undefined),
            };
        }
        match self.stage {
            Stage::IteratorMethod => {
                let method = Self::value(input)?;
                if matches!(method, Value::Null | Value::Undefined) {
                    self.stage = Stage::Length;
                    return Ok(Action::Continue);
                }
                self.stage = Stage::IteratorObject;
                Ok(Action::Call {
                    callee: method,
                    this: self.source.clone(),
                    args: vec![],
                })
            }
            Stage::IteratorObject => {
                self.iterator = Self::value(input)?;
                if !matches!(self.iterator, Value::Object(_)) {
                    return Err(Fault::TypeError("iterator must be an object".into()));
                }
                self.stage = Stage::NextMethod;
                Ok(Action::Continue)
            }
            Stage::Next => {
                if let Input::CallResult(value) = input {
                    self.next_method = value;
                }
                self.stage = Stage::NextResult;
                Ok(Action::Call {
                    callee: self.next_method.clone(),
                    this: self.iterator.clone(),
                    args: vec![],
                })
            }
            Stage::DoneValue => {
                if Self::value(input)?.truthy() {
                    self.check_length(heap, task, self.values.len() as u64)?;
                } else {
                    self.stage = Stage::IteratorValue;
                }
                Ok(Action::Continue)
            }
            Stage::CollectValue => {
                let value = Self::value(input)?;
                let Operation::Construct(kind) = self.op else {
                    unreachable!()
                };
                if self.values.len() >= crate::object::array_buffer::MAX_BYTE_LENGTH / kind.size() {
                    return Err(Fault::RangeError(
                        "typed array iterable exceeds buffer limit".into(),
                    ));
                }
                self.values.push(value);
                self.stage = Stage::Next;
                Ok(Action::Continue)
            }
            Stage::LengthValue => {
                let value = Self::value(input)?;
                if matches!(value, Value::Object(_)) {
                    return Ok(Action::ToNumber(value));
                }
                let length = to_length(&value)?;
                self.check_length(heap, task, length)?;
                Ok(Action::Continue)
            }
            Stage::Offset => {
                self.stage = Stage::OffsetValue;
                Ok(Action::ToNumber(self.arg(1)))
            }
            Stage::OffsetValue => {
                let offset = Self::integer(&Self::value(input)?)?;
                if offset < 0.0 {
                    return Err(Fault::RangeError("negative typed array offset".into()));
                }
                attached(heap, self.target_view.unwrap())?;
                // Keep positive infinity as an out-of-range sentinel; the
                // source length getter must still run before the range error.
                self.offset = if offset > usize::MAX as f64 {
                    usize::MAX
                } else {
                    offset as usize
                };
                self.stage = Stage::Source;
                Ok(Action::Continue)
            }
            Stage::Snapshot => {
                if self.next == self.len {
                    self.next = 0;
                    self.stage = Stage::Element;
                } else {
                    let source = self.source_view.unwrap();
                    if source.kind == self.target_view.unwrap().kind {
                        self.raw.push(source.raw(heap, self.next)?);
                    } else {
                        self.values.push(
                            source
                                .get(heap, self.next as f64)?
                                .unwrap_or(Value::Undefined),
                        );
                    }
                    self.next += 1;
                }
                Ok(Action::Continue)
            }
            Stage::Element => {
                if self.next == self.len {
                    return Ok(Action::Return(if matches!(self.op, Operation::Set) {
                        Value::Undefined
                    } else {
                        self.target.clone()
                    }));
                }
                if !self.raw.is_empty() {
                    self.target_view.unwrap().set_raw(
                        heap,
                        self.offset + self.next,
                        self.raw[self.next],
                    )?;
                    self.next += 1;
                    return Ok(Action::Continue);
                }
                self.stage = Stage::NumberValue;
                Ok(Action::ToNumber(self.values[self.next].clone()))
            }
            Stage::ElementValue => {
                self.stage = Stage::NumberValue;
                Ok(Action::ToNumber(Self::value(input)?))
            }
            Stage::NumberValue => {
                self.target_view.unwrap().set(
                    heap,
                    (self.offset + self.next) as f64,
                    &Self::value(input)?,
                )?;
                self.next += 1;
                self.stage = Stage::Element;
                Ok(Action::Continue)
            }
            Stage::FillValue => {
                self.stage = Stage::FillStart;
                Ok(Action::ToNumber(self.source.clone()))
            }
            Stage::FillStart => {
                self.fill_value = Self::value(input)?;
                self.stage = Stage::FillStartValue;
                Ok(Action::ToNumber(self.arg(1)))
            }
            Stage::FillStartValue => {
                self.next = self.relative(&Self::value(input)?)?;
                self.stage = Stage::FillEnd;
                Ok(Action::Continue)
            }
            Stage::FillEnd => {
                self.stage = Stage::FillEndValue;
                let end = self.arg(2);
                if matches!(end, Value::Undefined) {
                    self.len = self.target_len;
                    attached(heap, self.target_view.unwrap())?;
                    self.stage = Stage::Fill;
                    Ok(Action::Continue)
                } else {
                    Ok(Action::ToNumber(end))
                }
            }
            Stage::FillEndValue => {
                self.len = self.relative(&Self::value(input)?)?;
                attached(heap, self.target_view.unwrap())?;
                self.stage = Stage::Fill;
                Ok(Action::Continue)
            }
            Stage::Fill => {
                if self.next >= self.len {
                    return Ok(Action::Return(self.target.clone()));
                }
                self.target_view
                    .unwrap()
                    .set(heap, self.next as f64, &self.fill_value)?;
                self.next += 1;
                Ok(Action::Continue)
            }
            _ => Err(Fault::Internal("invalid typed array stage")),
        }
    }
}
