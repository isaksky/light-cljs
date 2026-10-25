//! Stable, budgeted sort over a task-owned snapshot. Every comparison and
//! property operation is resumed by the native trampoline, never Rust's stack.
use super::*;
use crate::value::string::StringFlatten;

#[derive(Debug)]
enum Stage {
    Start,
    Length,
    LengthNumber,
    Scan,
    Element,
    Merge,
    Compared,
    Number,
    LeftString,
    RightString,
    FlattenLeft,
    FlattenRight,
    StringCompare,
    Write,
    Written,
    Delete,
}

#[derive(Debug)]
pub(super) struct State {
    stage: Stage,
    this: Value,
    comparator: Value,
    copy: bool,
    result: Value,
    source: Vec<Value>,
    destination: Vec<Value>,
    length: u64,
    index: u64,
    width: usize,
    start: usize,
    left: usize,
    middle: usize,
    right: usize,
    end: usize,
    output: usize,
    left_string: Option<JsString>,
    right_string: Option<JsString>,
    flatten: Option<StringFlatten>,
    unit: usize,
}

impl Trace for State {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        self.this.trace(visit);
        self.comparator.trace(visit);
        self.result.trace(visit);
        for value in self.source.iter().chain(&self.destination) {
            value.trace(visit);
        }
    }
}

impl State {
    pub(super) fn new(this: Value, args: &[Value], copy: bool) -> Self {
        Self {
            stage: Stage::Start,
            this,
            comparator: args.first().cloned().unwrap_or(Value::Undefined),
            copy,
            result: Value::Undefined,
            source: Vec::new(),
            destination: Vec::new(),
            length: 0,
            index: 0,
            width: 1,
            start: 0,
            left: 0,
            middle: 0,
            right: 0,
            end: 0,
            output: 0,
            left_string: None,
            right_string: None,
            flatten: None,
            unit: 0,
        }
    }

    fn value(input: Input) -> Result<Value, Fault> {
        match input {
            Input::CallResult(v) => Ok(v),
            _ => Err(Fault::Internal("sort expected a call result")),
        }
    }

    fn read(&mut self, heap: &Heap, task: TaskId, key: property::Key) -> Result<Action, Fault> {
        match property::read(heap, task, &self.this, &key)? {
            Some(property::Read::Getter(callee)) => Ok(Action::Call {
                callee,
                this: self.this.clone(),
                args: vec![],
            }),
            Some(property::Read::Value(v)) => self.step(heap, task, Input::CallResult(v)),
            None => self.step(heap, task, Input::CallResult(Value::Undefined)),
        }
    }

    fn select(&mut self, left: bool) -> Action {
        let index = if left {
            &mut self.left
        } else {
            &mut self.right
        };
        let value = std::mem::replace(&mut self.source[*index], Value::Undefined);
        *index += 1;
        if self.output == self.destination.len() {
            self.destination.push(value);
        } else {
            self.destination[self.output] = value;
        }
        self.output += 1;
        self.left_string = None;
        self.right_string = None;
        self.stage = Stage::Merge;
        Action::Continue
    }

    fn string(&mut self, input: Input, next: Stage) -> Result<Action, Fault> {
        let Value::String(text) = Self::value(input)? else {
            return Err(Fault::Internal("sort expected a string"));
        };
        self.flatten = Some(StringFlatten::new(text).ok_or(Fault::OutOfMemory("sort string"))?);
        self.stage = next;
        Ok(Action::Continue)
    }

    pub(super) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
    ) -> Result<Action, Fault> {
        match self.stage {
            Stage::Start => {
                // Validate even for empty/singleton receivers, before length or elements.
                if !matches!(self.comparator, Value::Undefined) {
                    dispatch::callee(heap, &self.comparator)?;
                }
                self.this = builtin::array_string_receiver(heap, task, &self.this)?;
                self.stage = Stage::Length;
                self.read(heap, task, property::Key::Name(JsString::from("length")))
            }
            Stage::Length => {
                self.stage = Stage::LengthNumber;
                Ok(Action::ToNumber(Self::value(input)?))
            }
            Stage::LengthNumber => {
                self.length = to_length(&Self::value(input)?)?;
                if self.copy {
                    self.result = builtin::array_constructor(
                        heap,
                        task,
                        &[Value::Number(self.length as f64)],
                    )?;
                }
                self.stage = Stage::Scan;
                Ok(Action::Continue)
            }
            Stage::Scan => {
                if self.index == self.length {
                    self.stage = Stage::Merge;
                    return Ok(Action::Continue);
                }
                let key = index_key(self.index);
                self.index += 1;
                if self.copy || property::has_property(heap, task, &self.this, &key)? {
                    self.stage = Stage::Element;
                    self.read(heap, task, key)
                } else {
                    Ok(Action::Continue)
                }
            }
            Stage::Element => {
                self.source.push(Self::value(input)?);
                self.stage = Stage::Scan;
                Ok(Action::Continue)
            }
            Stage::Merge => {
                let n = self.source.len();
                if self.width >= n {
                    self.index = 0;
                    self.stage = Stage::Write;
                    return Ok(Action::Continue);
                }
                if self.left == self.middle && self.right == self.end {
                    if self.end == n {
                        std::mem::swap(&mut self.source, &mut self.destination);
                        self.width = self.width.saturating_mul(2);
                        self.start = 0;
                        self.end = 0;
                        self.output = 0;
                        // Set up the first pair on the next budgeted step.
                        self.left = 0;
                        self.middle = 0;
                        self.right = 0;
                        return Ok(Action::Continue);
                    }
                    self.start = self.end;
                    self.left = self.start;
                    self.middle = self.start.saturating_add(self.width).min(n);
                    self.right = self.middle;
                    self.end = self.middle.saturating_add(self.width).min(n);
                }
                if self.left == self.middle {
                    return Ok(self.select(false));
                }
                if self.right == self.end {
                    return Ok(self.select(true));
                }
                let a = self.source[self.left].clone();
                let b = self.source[self.right].clone();
                // Undefined never reaches the comparator or ToString.
                if matches!(a, Value::Undefined) || matches!(b, Value::Undefined) {
                    return Ok(self.select(matches!(b, Value::Undefined)));
                }
                if matches!(self.comparator, Value::Undefined) {
                    self.stage = Stage::LeftString;
                    Ok(Action::ToString(a))
                } else {
                    self.stage = Stage::Compared;
                    Ok(Action::Call {
                        callee: self.comparator.clone(),
                        this: Value::Undefined,
                        args: vec![a, b],
                    })
                }
            }
            Stage::Compared => {
                self.stage = Stage::Number;
                Ok(Action::ToNumber(Self::value(input)?))
            }
            Stage::Number => {
                let Value::Number(number) = Self::value(input)? else {
                    return Err(Fault::Internal("sort expected a number"));
                };
                Ok(self.select(number.is_nan() || number <= 0.0))
            }
            Stage::LeftString => self.string(input, Stage::FlattenLeft),
            Stage::FlattenLeft => {
                if let Some(text) = self.flatten.as_mut().unwrap().advance(256) {
                    self.left_string = Some(text);
                    self.flatten = None;
                    self.stage = Stage::RightString;
                    return Ok(Action::ToString(self.source[self.right].clone()));
                }
                Ok(Action::Continue)
            }
            Stage::RightString => self.string(input, Stage::FlattenRight),
            Stage::FlattenRight => {
                if let Some(text) = self.flatten.as_mut().unwrap().advance(256) {
                    self.right_string = Some(text);
                    self.flatten = None;
                    self.unit = 0;
                    self.stage = Stage::StringCompare;
                }
                Ok(Action::Continue)
            }
            Stage::StringCompare => {
                // Both strings are flat. Compare at most 256 UTF-16 units.
                let a = self.left_string.as_ref().unwrap().code_units();
                let b = self.right_string.as_ref().unwrap().code_units();
                for _ in 0..256 {
                    match (a.get(self.unit), b.get(self.unit)) {
                        (Some(a), Some(b)) if a == b => self.unit += 1,
                        (Some(a), Some(b)) => return Ok(self.select(a < b)),
                        (None, _) => return Ok(self.select(true)),
                        (_, None) => return Ok(self.select(false)),
                    }
                }
                Ok(Action::Continue)
            }
            Stage::Write => {
                if self.index == self.source.len() as u64 {
                    self.stage = Stage::Delete;
                    return Ok(Action::Continue);
                }
                self.stage = Stage::Written;
                if self.copy {
                    return Ok(Action::CreateDataProperty {
                        this: self.result.clone(),
                        key: Value::Number(self.index as f64),
                        value: self.source[self.index as usize].clone(),
                    });
                }
                Ok(Action::SetProperty {
                    this: self.this.clone(),
                    key: index_key(self.index),
                    value: self.source[self.index as usize].clone(),
                    strict: true,
                })
            }
            Stage::Written => {
                self.index += 1;
                self.stage = Stage::Write;
                Ok(Action::Continue)
            }
            Stage::Delete => {
                if self.copy {
                    return Ok(Action::Return(self.result.clone()));
                }
                if self.index == self.length {
                    return Ok(Action::Return(self.this.clone()));
                }
                property::delete(
                    heap,
                    task,
                    &self.this,
                    &Value::Number(self.index as f64),
                    true,
                )?;
                self.index += 1;
                Ok(Action::Continue)
            }
        }
    }
}
