//! Budgeted Array.from. Iterator acquisition uses the shared GetIterator
//! operation; every observable getter and callback is a trampoline action.
use super::*;
use crate::vm::exception::Exception;

#[derive(Debug)]
enum Stage {
    Start,
    Method,
    Create,
    Iterator,
    Next,
    NextResult,
    Done,
    Value,
    Mapped,
    Append,
    Length,
    LengthValue,
    Element,
    Close,
    CloseMethod,
    CloseResult,
    Rethrow,
}

#[derive(Debug)]
pub(super) struct State {
    stage: Stage,
    items: Value,
    constructor: Value,
    mapper: Value,
    this_arg: Value,
    out: Value,
    iterator: Value,
    next: Value,
    result: Value,
    acquisition: Option<iterator::State>,
    index: u64,
    length: u64,
    iterable: bool,
    bound_receiver: bool,
    pending: Option<Exception>,
}

impl Trace for State {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        for value in [
            &self.items,
            &self.constructor,
            &self.mapper,
            &self.this_arg,
            &self.out,
            &self.iterator,
            &self.next,
            &self.result,
        ] {
            value.trace(visit);
        }
        if let Some(acquisition) = &self.acquisition {
            acquisition.trace(visit);
        }
        if let Some(exception) = &self.pending {
            exception.value.trace(visit);
        }
    }
}

impl State {
    pub(super) fn new(constructor: Value, args: &[Value]) -> Self {
        let arg = |i| args.get(i).cloned().unwrap_or(Value::Undefined);
        Self {
            stage: Stage::Start,
            items: arg(0),
            constructor,
            mapper: arg(1),
            this_arg: arg(2),
            out: Value::Undefined,
            iterator: Value::Undefined,
            next: Value::Undefined,
            result: Value::Undefined,
            acquisition: None,
            index: 0,
            length: 0,
            iterable: false,
            bound_receiver: false,
            pending: None,
        }
    }

    // Only mapping/element creation triggers IteratorClose. Failures from
    // IteratorStep/IteratorValue propagate without calling return.
    pub(super) fn catches(&self) -> bool {
        self.iterable
            && matches!(
                self.stage,
                Stage::Mapped
                    | Stage::Append
                    | Stage::Close
                    | Stage::CloseMethod
                    | Stage::CloseResult
            )
    }

    pub(super) fn catches_step(&self) -> bool {
        self.iterable && matches!(self.stage, Stage::Append | Stage::Close)
    }

    pub(super) fn failed(&mut self, exception: Exception) {
        if self.pending.is_none() {
            self.pending = Some(exception);
            self.stage = Stage::Close;
        } else {
            self.stage = Stage::Rethrow;
        }
    }

    fn value(input: Input) -> Result<Value, Fault> {
        match input {
            Input::CallResult(v) => Ok(v),
            _ => Err(Fault::Internal("Array.from expected a call result")),
        }
    }

    fn read(
        &mut self,
        heap: &Heap,
        task: TaskId,
        base: Value,
        key: property::Key,
        stage: Stage,
    ) -> Result<Action, Fault> {
        self.stage = stage;
        let read = property::read(heap, task, &base, &key)?;
        match read {
            Some(property::Read::Deferred(callee)) => Ok(callee.action(base)),
            Some(property::Read::Value(v)) => self.step(heap, task, Input::CallResult(v)),
            None => self.step(heap, task, Input::CallResult(Value::Undefined)),
        }
    }

    fn create(&mut self, heap: &Heap, task: TaskId) -> Result<Action, Fault> {
        // Classes/subclassing are unavailable. Reject custom constructors
        // explicitly rather than silently returning an array of the wrong brand.
        let custom = match dispatch::callee(heap, &self.constructor) {
            Ok(Callee::Code(_, kind)) => kind.is_constructor(),
            Ok(Callee::Bound(bound)) => {
                // One bound target per step; bound nonconstructors still
                // select the ordinary array fallback.
                self.constructor = bound.callee;
                self.bound_receiver = true;
                return Ok(Action::Continue);
            }
            Ok(Callee::Builtin(builtin::Builtin::Array)) => self.bound_receiver,
            Ok(Callee::Builtin(builtin::Builtin::BigInt | builtin::Builtin::Symbol)) => false,
            Ok(Callee::Builtin(_)) => {
                // Realm constructors expose fixed own prototype metadata.
                // Inspect the descriptor without invoking a getter, and
                // avoid duplicating the registry of built-in constructors.
                let Value::Object(id) = self.constructor else {
                    return Err(Fault::Internal("built-in constructor is not an object"));
                };
                property::get_own_property(
                    heap,
                    task,
                    id,
                    &property::Key::Name(JsString::from("prototype")),
                )?
                .is_some()
            }
            Err(Fault::NotCallable(_)) => false,
            Err(fault) => return Err(fault),
        };
        if custom {
            return Err(Fault::Unsupported(
                "Array.from custom constructors are not supported".into(),
            ));
        }
        self.out = property::new_array(heap, task)?;
        self.stage = if self.iterable {
            Stage::Iterator
        } else {
            Stage::Element
        };
        Ok(Action::Continue)
    }

    fn mapped(&mut self, value: Value) -> Result<Action, Fault> {
        self.stage = Stage::Mapped;
        if matches!(self.mapper, Value::Undefined) {
            self.result = value;
            self.stage = Stage::Append;
            Ok(Action::Continue)
        } else {
            Ok(Action::Call {
                callee: self.mapper.clone(),
                this: self.this_arg.clone(),
                args: vec![value, Value::Number(self.index as f64)],
            })
        }
    }

    pub(super) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
    ) -> Result<Action, Fault> {
        match self.stage {
            Stage::Start => {
                if !matches!(self.mapper, Value::Undefined) {
                    dispatch::callee(heap, &self.mapper)?;
                }
                self.read(
                    heap,
                    task,
                    self.items.clone(),
                    property::Key::Symbol(crate::value::symbol::well_known("iterator")),
                    Stage::Method,
                )
            }
            Stage::Method => {
                let method = Self::value(input)?;
                if matches!(method, Value::Undefined | Value::Null) {
                    self.stage = Stage::Length;
                } else {
                    dispatch::callee(heap, &method)?;
                    self.iterable = true;
                    self.acquisition =
                        Some(iterator::State::with_method(self.items.clone(), method));
                    self.stage = Stage::Create;
                }
                Ok(Action::Continue)
            }
            Stage::Create => self.create(heap, task),
            Stage::Iterator => {
                let action = self
                    .acquisition
                    .as_mut()
                    .ok_or(Fault::Internal("missing GetIterator"))?
                    .step(heap, task, input)?;
                if let Action::Return(record) = action {
                    self.iterator =
                        property::get_field(heap, task, &record, &JsString::from("iterator"))?;
                    self.next = property::get_field(heap, task, &record, &JsString::from("next"))?;
                    self.acquisition = None;
                    self.stage = Stage::Next;
                    Ok(Action::Continue)
                } else {
                    Ok(action)
                }
            }
            Stage::Next => {
                self.stage = Stage::NextResult;
                Ok(Action::Call {
                    callee: self.next.clone(),
                    this: self.iterator.clone(),
                    args: vec![],
                })
            }
            Stage::NextResult => {
                self.result = Self::value(input)?;
                if !matches!(self.result, Value::Object(_)) {
                    return Err(Fault::TypeError("iterator result must be an object".into()));
                }
                self.read(
                    heap,
                    task,
                    self.result.clone(),
                    property::Key::Name(JsString::from("done")),
                    Stage::Done,
                )
            }
            Stage::Done => {
                if Self::value(input)?.truthy() {
                    return Ok(Action::Return(self.out.clone()));
                }
                self.read(
                    heap,
                    task,
                    self.result.clone(),
                    property::Key::Name(JsString::from("value")),
                    Stage::Value,
                )
            }
            Stage::Value => self.mapped(Self::value(input)?),
            Stage::Mapped => {
                self.result = Self::value(input)?;
                self.stage = Stage::Append;
                Ok(Action::Continue)
            }
            Stage::Append => {
                if self.index >= u64::from(u32::MAX) {
                    return Err(Fault::RangeError("invalid array length".into()));
                }
                property::append(heap, task, &self.out, Some(self.result.clone()))?;
                self.index += 1;
                self.result = Value::Undefined;
                self.stage = if self.iterable {
                    Stage::Next
                } else {
                    Stage::Element
                };
                Ok(Action::Continue)
            }
            Stage::Length => self.read(
                heap,
                task,
                self.items.clone(),
                property::Key::Name(JsString::from("length")),
                Stage::LengthValue,
            ),
            Stage::LengthValue => {
                let value = Self::value(input)?;
                if matches!(value, Value::Object(_)) {
                    return Ok(Action::ToNumber(value));
                }
                self.length = to_length(&value)?;
                if self.length > u64::from(u32::MAX) {
                    return Err(Fault::RangeError("invalid array length".into()));
                }
                self.stage = Stage::Create;
                Ok(Action::Continue)
            }
            Stage::Element => {
                if self.index == self.length {
                    return Ok(Action::Return(self.out.clone()));
                }
                self.read(
                    heap,
                    task,
                    self.items.clone(),
                    index_key(self.index),
                    Stage::Value,
                )
            }
            Stage::Close => self.read(
                heap,
                task,
                self.iterator.clone(),
                property::Key::Name(JsString::from("return")),
                Stage::CloseMethod,
            ),
            Stage::CloseMethod => {
                let method = Self::value(input)?;
                if matches!(method, Value::Null | Value::Undefined) {
                    self.stage = Stage::Rethrow;
                    Ok(Action::Continue)
                } else {
                    self.stage = Stage::CloseResult;
                    Ok(Action::Call {
                        callee: method,
                        this: self.iterator.clone(),
                        args: vec![],
                    })
                }
            }
            Stage::CloseResult => {
                // A pending throw wins over every return result/error.
                self.stage = Stage::Rethrow;
                Ok(Action::Continue)
            }
            Stage::Rethrow => Err(Fault::Rethrow(
                self.pending
                    .take()
                    .ok_or(Fault::Internal("missing Array.from exception"))?,
            )),
        }
    }
}
