//! Reflect operations (clj-quickjs-320). Fresh implementation after quickjs.c
//! js_reflect_funcs / js_reflect_construct, revision 7d1c13c5d8f8.
//! User code runs only through actions; all saved values are traced below.
use super::{Action, Fault, Input};
use crate::heap::gc::{Trace, TraceEdge};
use crate::heap::{Heap, HeapObject};
use crate::ids::TaskId;
use crate::object::PropertyDescriptor;
use crate::value::{JsString, Value};
use crate::vm::{
    builtin::Builtin, descriptor, dispatch, instr::Instr, integrity, property, prototype, proxy,
};

pub(super) fn is_method(which: Builtin) -> bool {
    matches!(
        which,
        Builtin::ReflectConstruct
            | Builtin::ReflectDefineProperty
            | Builtin::ReflectDeleteProperty
            | Builtin::ReflectGet
            | Builtin::ReflectGetOwnPropertyDescriptor
            | Builtin::ReflectGetPrototypeOf
            | Builtin::ReflectHas
            | Builtin::ReflectIsExtensible
            | Builtin::ReflectOwnKeys
            | Builtin::ReflectPreventExtensions
            | Builtin::ReflectSet
            | Builtin::ReflectSetPrototypeOf
    )
}
#[derive(Debug)]
enum Stage {
    Start,
    Key,
    Operate,
    Descriptor,
    Receiver,
    Define,
    Number,
    ArrayLength,
    ArrayLengthNumber,
    Length,
    LengthNumber,
    Element,
    Arguments,
    Return,
    Setter,
}
#[derive(Debug)]
pub(super) struct State {
    which: Builtin,
    args: Vec<Value>,
    key: Value,
    reader: Option<descriptor::DefineProperty>,
    desc: PropertyDescriptor,
    proxy: Option<proxy::State>,
    receiver: Value,
    ops: Option<proxy::Operations>,
    stage: Stage,
    list: Vec<Value>,
    length: usize,
    max_args: usize,
    max_hops: usize,
    typed_write: bool,
    array_length: u32,
}
impl Trace for State {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        for v in self.args.iter().chain(self.list.iter()) {
            v.trace(visit);
        }
        self.key.trace(visit);
        self.receiver.trace(visit);
        for v in [&self.desc.value, &self.desc.get, &self.desc.set]
            .into_iter()
            .flatten()
        {
            v.trace(visit);
        }
        if let Some(reader) = &self.reader {
            reader.trace(visit);
        }
        if let Some(proxy) = &self.proxy {
            proxy.trace(visit);
        }
        if let Some(ops) = &self.ops {
            ops.trace(visit);
        }
    }
}
impl State {
    pub(super) fn new(which: Builtin, args: Vec<Value>, max_args: usize, max_hops: usize) -> Self {
        Self {
            which,
            args,
            key: Value::Undefined,
            reader: None,
            desc: PropertyDescriptor::default(),
            proxy: None,
            receiver: Value::Undefined,
            ops: None,
            stage: Stage::Start,
            list: vec![],
            length: 0,
            max_args: max_args.min(usize::from(u16::MAX)),
            max_hops,
            typed_write: false,
            array_length: 0,
        }
    }
    fn arg(&self, i: usize) -> Value {
        self.args.get(i).cloned().unwrap_or(Value::Undefined)
    }
    fn key(&self) -> Result<property::Key, Fault> {
        property::to_property_key(&self.key)
    }
    fn target(&self) -> Result<crate::heap::ObjectId, Fault> {
        match self.arg(0) {
            Value::Object(id) => Ok(id),
            _ => Err(Fault::TypeError("Reflect target must be an object".into())),
        }
    }
    #[inline(never)]
    pub(super) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
    ) -> Result<Action, Fault> {
        if let Some(proxy) = &mut self.proxy {
            return proxy.step(heap, task, input);
        }
        self.step_operation(heap, task, input)
    }

    #[inline(never)]
    fn step_operation(&mut self, heap: &Heap, task: TaskId, input: Input) -> Result<Action, Fault> {
        match self.stage {
            Stage::Start => {
                if self.which == Builtin::ReflectConstruct {
                    for value in [
                        self.arg(0),
                        self.args.get(2).cloned().unwrap_or_else(|| self.arg(0)),
                    ] {
                        if !dispatch::is_constructor(heap, &value, self.max_hops)? {
                            return Err(Fault::TypeError(
                                "Reflect.construct needs a constructor".into(),
                            ));
                        }
                    }
                    if !matches!(self.arg(1), Value::Object(_)) {
                        return Err(Fault::TypeError(
                            "an argument list must be an object".into(),
                        ));
                    }
                    self.stage = Stage::Length;
                    return Ok(get(self.arg(1), Value::String(JsString::from("length"))));
                }
                self.target()?;
                if matches!(
                    self.which,
                    Builtin::ReflectDefineProperty
                        | Builtin::ReflectDeleteProperty
                        | Builtin::ReflectGet
                        | Builtin::ReflectGetOwnPropertyDescriptor
                        | Builtin::ReflectHas
                        | Builtin::ReflectSet
                ) {
                    self.stage = Stage::Key;
                    return Ok(Action::Instruction {
                        instr: Instr::ToSuperKey,
                        operands: vec![self.arg(1)],
                    });
                }
                self.stage = Stage::Operate;
            }
            Stage::Key => {
                self.key = result(input)?;
                self.stage = Stage::Operate;
            }
            Stage::Return => return Ok(Action::Return(result(input)?)),
            Stage::Setter => return Ok(Action::Return(Value::Bool(true))),
            Stage::Descriptor => {
                let reader = self
                    .reader
                    .as_mut()
                    .ok_or(Fault::Internal("Reflect descriptor missing"))?;
                let value = match input {
                    Input::Start => None,
                    Input::CallResult(v) => Some(v),
                    _ => return Err(Fault::Internal("Reflect descriptor input")),
                };
                match reader.step(heap, task, value)? {
                    descriptor::DefineStep::Has { this, key } => {
                        return Ok(super::has_property(this, key));
                    }
                    descriptor::DefineStep::Getter { callee, this } => {
                        return Ok(callee.action(this));
                    }
                    descriptor::DefineStep::Done(_) => {
                        self.desc = reader.descriptor().clone();
                        self.receiver = self.arg(0);
                        self.stage = Stage::Define;
                    }
                    _ => {
                        return Err(Fault::Internal(
                            "Reflect descriptor reader attempted a write",
                        ));
                    }
                }
            }
            Stage::Receiver => {
                match self
                    .ops
                    .as_mut()
                    .ok_or(Fault::Internal("Reflect receiver operation missing"))?
                    .step(heap, task, input)?
                {
                    proxy::Progress::Action(a) => return Ok(a),
                    proxy::Progress::Done(proxy::Output::Descriptor(own)) => {
                        if own
                            .as_ref()
                            .is_some_and(|p| p.accessor.is_some() || !p.flags.writable)
                        {
                            return boolean(false);
                        }
                        self.desc = PropertyDescriptor {
                            value: Some(self.arg(2)),
                            writable: own.is_none().then_some(true),
                            enumerable: own.is_none().then_some(true),
                            configurable: own.is_none().then_some(true),
                            ..Default::default()
                        };
                        self.stage = Stage::Define;
                    }
                    _ => return Err(Fault::Internal("Reflect receiver descriptor result")),
                }
            }
            Stage::Number => {
                self.desc.value = Some(result(input)?);
                return self.define(heap, task);
            }
            Stage::ArrayLength => {
                let Value::Number(number) = result(input)? else {
                    return Err(Fault::Internal("Reflect array length conversion"));
                };
                self.array_length = if number.is_finite() {
                    number.trunc().rem_euclid(4294967296.0) as u32
                } else {
                    0
                };
                self.stage = Stage::ArrayLengthNumber;
                return Ok(Action::ToNumber(self.desc.value.clone().unwrap()));
            }
            Stage::ArrayLengthNumber => {
                let Value::Number(number) = result(input)? else {
                    return Err(Fault::Internal("Reflect array length number"));
                };
                if number != f64::from(self.array_length) {
                    return Err(Fault::RangeError("invalid array length".into()));
                }
                self.desc.value = Some(Value::Number(number));
                return self.define(heap, task);
            }
            Stage::Length => {
                self.stage = Stage::LengthNumber;
                return Ok(Action::ToNumber(result(input)?));
            }
            Stage::LengthNumber => {
                let length = super::to_length(&result(input)?)?;
                if length > self.max_args as u64 {
                    return Err(Fault::StackOverflow(format!(
                        "Reflect.construct argument list exceeds limit of {}",
                        self.max_args
                    )));
                }
                self.length = length as usize;
                self.stage = Stage::Arguments;
            }
            Stage::Element => {
                self.list.push(result(input)?);
                self.stage = Stage::Arguments;
            }
            Stage::Operate | Stage::Define | Stage::Arguments => {}
        }
        if matches!(self.stage, Stage::Arguments) {
            if self.list.len() < self.length {
                self.stage = Stage::Element;
                return Ok(get(self.arg(1), Value::Number(self.list.len() as f64)));
            }
            let argc = u16::try_from(self.list.len())
                .map_err(|_| Fault::StackOverflow("Reflect.construct arguments".into()))?;
            let mut operands = vec![
                self.arg(0),
                self.args.get(2).cloned().unwrap_or_else(|| self.arg(0)),
            ];
            operands.append(&mut self.list);
            self.stage = Stage::Return;
            return Ok(Action::Instruction {
                instr: Instr::ConstructSuper(argc),
                operands,
            });
        }
        if matches!(self.stage, Stage::Define) {
            let Value::Object(id) = self.receiver else {
                return boolean(false);
            };
            let id = proxy::forward(heap, task, id, "defineProperty")?;
            // Forwarding has completed. A value conversion may mutate the
            // original Proxy handler, so finish on this traced target.
            self.receiver = Value::Object(id);
            let key = self.key()?;
            // Validate the typed-array descriptor before coercing its value.
            if !self.typed_write {
                match property::typed_array_descriptor(heap, task, id, &key, &self.desc) {
                    Ok(Some(_)) if self.desc.value.is_none() => return boolean(true),
                    // Once validated, an element definition finishes as a
                    // write, even if conversion detaches its buffer.
                    Ok(Some(_)) => self.typed_write = true,
                    Ok(None) => {}
                    Err(fault) => return boolean(ordinary(Err(fault))?),
                }
            }
            if let Some(value) = &self.desc.value {
                let typed = crate::object::typed_array::numeric_key(&key).is_some()
                    && crate::object::typed_array::view(heap, task, id)?.is_some();
                let length = key.is_length()
                    && heap
                        .read(id, |o| matches!(o, HeapObject::Array(_)))
                        .unwrap_or(false);
                if typed || length {
                    self.stage = if length {
                        Stage::ArrayLength
                    } else {
                        Stage::Number
                    };
                    return Ok(Action::ToNumber(value.clone()));
                }
            }
            return self.define(heap, task);
        }
        let target = self.target()?;
        match self.which {
            Builtin::ReflectGet => {
                self.stage = Stage::Return;
                match property::read(heap, task, &self.arg(0), &self.key()?)? {
                    Some(property::Read::Deferred(callee)) => {
                        Ok(callee.action(self.args.get(2).cloned().unwrap_or_else(|| self.arg(0))))
                    }
                    Some(property::Read::Value(v)) => Ok(Action::Return(v)),
                    None => Ok(Action::Return(Value::Undefined)),
                }
            }
            Builtin::ReflectHas => {
                self.stage = Stage::Return;
                Ok(Action::Instruction {
                    instr: Instr::In,
                    operands: vec![self.key.clone(), self.arg(0)],
                })
            }
            Builtin::ReflectDeleteProperty => boolean(property::delete(
                heap,
                task,
                &self.arg(0),
                &self.key,
                false,
            )?),
            Builtin::ReflectGetPrototypeOf => prototype::get_prototype_of(heap, task, target)
                .map(|p| Action::Return(p.map_or(Value::Null, Value::Object))),
            Builtin::ReflectIsExtensible => {
                integrity::is_extensible(heap, task, &self.arg(0)).map(Action::Return)
            }
            Builtin::ReflectPreventExtensions => boolean(ordinary(
                integrity::prevent_extensions(heap, task, &self.arg(0)).map(|_| ()),
            )?),
            Builtin::ReflectSetPrototypeOf => {
                let proto = match self.arg(1) {
                    Value::Null => None,
                    Value::Object(id) => Some(id),
                    _ => {
                        return Err(Fault::TypeError(
                            "an object's prototype must be an object or null".into(),
                        ));
                    }
                };
                boolean(ordinary(prototype::set_prototype_of(
                    heap, task, target, proto,
                ))?)
            }
            Builtin::ReflectOwnKeys | Builtin::ReflectGetOwnPropertyDescriptor => {
                let mode = if self.which == Builtin::ReflectOwnKeys {
                    proxy::Mode::Keys {
                        target,
                        enumerable: false,
                        symbols: true,
                    }
                } else {
                    proxy::Mode::Descriptor {
                        target,
                        key: self.key()?,
                    }
                };
                self.proxy = Some(proxy::State::new(mode));
                Ok(Action::Continue)
            }
            Builtin::ReflectDefineProperty => {
                self.reader = Some(descriptor::DefineProperty::reader(self.arg(2))?);
                self.stage = Stage::Descriptor;
                Ok(Action::Continue)
            }
            Builtin::ReflectSet => {
                let target = proxy::forward(heap, task, target, "set")?;
                self.receiver = self.args.get(3).cloned().unwrap_or_else(|| self.arg(0));
                let key = self.key()?;
                if self.receiver.same_object(&Value::Object(target))
                    && crate::object::typed_array::numeric_key(&key).is_some()
                    && crate::object::typed_array::view(heap, task, target)?.is_some()
                {
                    self.desc.value = Some(self.arg(2));
                    self.typed_write = true;
                    self.stage = Stage::Define;
                    return Ok(Action::Continue);
                }
                match property::super_write(heap, task, &Value::Object(target), &key, true) {
                    Ok(property::SuperWrite::Setter(callee)) => {
                        self.stage = Stage::Setter;
                        Ok(Action::Call {
                            callee,
                            this: self.receiver.clone(),
                            args: vec![self.arg(2)],
                        })
                    }
                    Ok(property::SuperWrite::Done) => boolean(true),
                    Ok(property::SuperWrite::Receiver) => {
                        let Value::Object(id) = self.receiver else {
                            return boolean(false);
                        };
                        self.ops = Some(proxy::Operations::descriptor(id, key));
                        self.stage = Stage::Receiver;
                        Ok(Action::Continue)
                    }
                    Err(fault) => boolean(ordinary(Err(fault))?),
                }
            }
            _ => Err(Fault::Internal("invalid Reflect operation")),
        }
    }
    fn define(&mut self, heap: &Heap, task: TaskId) -> Result<Action, Fault> {
        let Value::Object(id) = self.receiver else {
            return boolean(false);
        };
        let key = self.key()?;
        if self.typed_write {
            let index = crate::object::typed_array::numeric_key(&key)
                .ok_or(Fault::Internal("Reflect typed index"))?;
            let view = crate::object::typed_array::view(heap, task, id)?
                .ok_or(Fault::Internal("Reflect typed receiver"))?;
            view.set(heap, index, self.desc.value.as_ref().unwrap())?;
            return boolean(true);
        }
        boolean(ordinary(property::define_own_property(
            heap, task, id, &key, &self.desc,
        ))?)
    }
}
fn get(base: Value, key: Value) -> Action {
    Action::Instruction {
        instr: Instr::GetElem,
        operands: vec![base, key],
    }
}
fn result(input: Input) -> Result<Value, Fault> {
    match input {
        Input::CallResult(v) => Ok(v),
        _ => Err(Fault::Internal("Reflect expected callback result")),
    }
}
fn boolean(value: bool) -> Result<Action, Fault> {
    Ok(Action::Return(Value::Bool(value)))
}
/// Only ordinary internal-method rejection becomes false. Runtime ownership,
/// immutable-realm policy and unsupported Proxy traps keep their errors.
fn ordinary(result: Result<(), Fault>) -> Result<bool, Fault> {
    match result {
        Ok(()) => Ok(true),
        Err(Fault::TypeError(message))
            if !message.starts_with("unsupported Proxy trap:")
                && !message.starts_with("Proxy:")
                && !message.ends_with("built-in objects are shared by all tasks and immutable")
                && !message.ends_with("functions shared between tasks are immutable") =>
        {
            Ok(false)
        }
        Err(fault) => Err(fault),
    }
}
