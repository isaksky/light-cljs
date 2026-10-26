//! Synchronous iterator helpers and built-in cursors (clj-quickjs-237).
//! GetIterator is reusable by native consumers; its result is a task-owned
//! record with `iterator` and cached `next`. Every observable read/call is a
//! trampoline action. Cursors are ordinary task-owned objects with a traced
//! internal slot, independently of their public properties/prototype.
use super::*;
use crate::heap::HeapObject;
use crate::object::{OrdinaryObject, PropertyFlags};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Array,
    TypedArray,
    String,
    ArrayKeys,
    ArrayEntries,
    SetValues,
    SetEntries,
    MapKeys,
    MapValues,
    MapEntries,
}

fn is_set(kind: Kind) -> bool {
    matches!(kind, Kind::SetValues | Kind::SetEntries)
}

fn is_map(kind: Kind) -> bool {
    matches!(kind, Kind::MapKeys | Kind::MapValues | Kind::MapEntries)
}

#[derive(Clone, Debug)]
pub(crate) struct Data {
    source: Value,
    index: u64,
    kind: Kind,
    done: bool,
}

impl Trace for Data {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        self.source.trace(visit);
    }
}

fn cursor<R>(
    heap: &Heap,
    task: TaskId,
    value: &Value,
    f: impl FnOnce(&mut Data) -> R,
) -> Result<R, Fault> {
    let invalid = || Fault::TypeError("next requires a built-in iterator".into());
    let Value::Object(id) = value else {
        return Err(invalid());
    };
    heap.with_object_mut(*id, |o| {
        let HeapObject::Plain(o) = o else {
            return Err(invalid());
        };
        if o.owner() != Some(task) {
            return Err(Fault::ForeignObject {
                owner: o.owner(),
                caller: task,
            });
        }
        o.iterator().map(f).ok_or_else(invalid)
    })
    .ok_or(Fault::Internal("iterator was reclaimed"))?
}

fn map<R>(
    heap: &Heap,
    task: TaskId,
    value: &Value,
    set: bool,
    f: impl FnOnce(&crate::object::map::MapObject) -> R,
) -> Result<R, Fault> {
    let invalid = || {
        Fault::TypeError(
            if set {
                "Set method requires a Set receiver"
            } else {
                "Map method requires a Map receiver"
            }
            .into(),
        )
    };
    let Value::Object(id) = value else {
        return Err(invalid());
    };
    heap.read(*id, |o| {
        let map = match o {
            HeapObject::JsSet(map) if set => map,
            HeapObject::JsMap(map) if !set => map,
            _ => return Err(invalid()),
        };
        if map.object().owner() != Some(task) {
            return Err(Fault::ForeignObject {
                owner: map.object().owner(),
                caller: task,
            });
        }
        Ok(f(map))
    })
    .ok_or(Fault::Internal("Map was reclaimed"))?
}

fn object(heap: &Heap, task: TaskId, fields: &[(&str, Value)]) -> Result<Value, Fault> {
    let realm = crate::realm::intrinsics(heap).ok_or(Fault::OutOfMemory("iterator realm"))?;
    let mut o = OrdinaryObject::with_prototype(heap, task, realm.object_prototype);
    for (name, value) in fields {
        let key = heap
            .keys()
            .intern(&JsString::from(*name))
            .ok_or(Fault::OutOfMemory("iterator key"))?;
        o.define_with(key, value.clone(), PropertyFlags::DATA);
    }
    heap.alloc(HeapObject::Plain(o))
        .map(Value::Object)
        .ok_or(Fault::OutOfMemory("iterator object"))
}

pub(crate) fn create(
    heap: &Heap,
    task: TaskId,
    source: &Value,
    kind: Kind,
) -> Result<Value, Fault> {
    if matches!(source, Value::Null | Value::Undefined) {
        return Err(Fault::TypeError("iterator requires a receiver".into()));
    }
    if kind == Kind::TypedArray {
        let view = typed_array::receiver(heap, task, source)?;
        if view.is_detached(heap)? {
            return Err(Fault::TypeError("ArrayBuffer is detached".into()));
        }
    }
    if is_set(kind) || is_map(kind) {
        map(heap, task, source, is_set(kind), |_| ())?;
    }
    let realm = crate::realm::intrinsics(heap).ok_or(Fault::OutOfMemory("iterator realm"))?;
    let prototype = if is_set(kind) {
        realm.set_iterator_prototype
    } else if is_map(kind) {
        realm.map_iterator_prototype
    } else if kind == Kind::String {
        realm.string_iterator_prototype
    } else {
        realm.array_iterator_prototype
    };
    let o = OrdinaryObject::with_prototype(heap, task, prototype).with_iterator(Data {
        source: source.clone(),
        index: 0,
        kind,
        done: false,
    });
    heap.alloc(HeapObject::Plain(o))
        .map(Value::Object)
        .ok_or(Fault::OutOfMemory("iterator cursor"))
}

#[derive(Debug)]
enum Stage {
    AsyncMethod,
    AsyncInvoke,
    Method,
    KnownMethod,
    Invoke,
    NextMethod,
    Record,
    StringConvert,
    StringCreate,
    Length,
    LengthValue,
    Element,
    ElementValue,
    MapEntry,
}

#[derive(Debug)]
pub(crate) struct State {
    fallback: bool,
    stage: Stage,
    source: Value,
    iterator: Value,
    index: u64,
    kind: Kind,
    done: bool,
    callback: Value,
    this_arg: Value,
}

impl Trace for State {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        self.source.trace(visit);
        self.iterator.trace(visit);
        self.callback.trace(visit);
        self.this_arg.trace(visit);
    }
}

impl State {
    pub(crate) fn get_async_iterator(source: Value) -> Self {
        Self {
            stage: Stage::AsyncMethod,
            ..Self::get_iterator(source)
        }
    }
    /// GetIterator with an already observed @@iterator method. Consumers
    /// such as Array.from must select their fallback without reading twice.
    pub(crate) fn with_method(source: Value, method: Value) -> Self {
        Self {
            stage: Stage::KnownMethod,
            iterator: method,
            ..Self::get_iterator(source)
        }
    }
    /// GetIterator's reusable resumable native operation. The caller can
    /// cache the returned record, then call its `next` with `iterator` as this.
    pub(crate) fn get_iterator(source: Value) -> Self {
        Self {
            fallback: false,
            stage: Stage::Method,
            source,
            iterator: Value::Undefined,
            index: 0,
            kind: Kind::Array,
            done: false,
            callback: Value::Undefined,
            this_arg: Value::Undefined,
        }
    }

    pub(super) fn map_for_each(
        heap: &Heap,
        task: TaskId,
        source: Value,
        callback: Value,
        this_arg: Value,
    ) -> Result<Self, Fault> {
        map(heap, task, &source, false, |_| ())?;
        dispatch::callee(heap, &callback)?;
        Ok(Self {
            stage: Stage::MapEntry,
            callback,
            this_arg,
            ..Self::get_iterator(source)
        })
    }

    pub(super) fn set_for_each(
        heap: &Heap,
        task: TaskId,
        source: Value,
        callback: Value,
        this_arg: Value,
    ) -> Result<Self, Fault> {
        map(heap, task, &source, true, |_| ())?;
        dispatch::callee(heap, &callback)?;
        Ok(Self {
            stage: Stage::MapEntry,
            kind: Kind::SetValues,
            callback,
            this_arg,
            ..Self::get_iterator(source)
        })
    }

    pub(super) fn string(source: Value) -> Result<Self, Fault> {
        if matches!(source, Value::Null | Value::Undefined) {
            return Err(Fault::TypeError(
                "string iterator requires a receiver".into(),
            ));
        }
        Ok(Self {
            stage: Stage::StringConvert,
            ..Self::get_iterator(source)
        })
    }

    pub(super) fn next(
        heap: &Heap,
        task: TaskId,
        iterator: &Value,
        expected: Kind,
    ) -> Result<Self, Fault> {
        let data = cursor(heap, task, iterator, |data| data.clone())?;
        if (data.kind == Kind::String) != (expected == Kind::String)
            || is_set(data.kind) != is_set(expected)
            || is_map(data.kind) != is_map(expected)
        {
            return Err(Fault::TypeError("incompatible iterator receiver".into()));
        }
        Ok(Self {
            fallback: false,
            stage: Stage::Length,
            source: data.source,
            iterator: iterator.clone(),
            index: data.index,
            kind: data.kind,
            done: data.done,
            callback: Value::Undefined,
            this_arg: Value::Undefined,
        })
    }

    fn value(input: Input) -> Result<Value, Fault> {
        match input {
            Input::CallResult(v) => Ok(v),
            _ => Err(Fault::Internal("iterator expected call result")),
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
        match property::read(heap, task, &base, &key)? {
            Some(property::Read::Deferred(callee)) => Ok(callee.action(base)),
            Some(property::Read::Value(value)) => self.step(heap, task, Input::CallResult(value)),
            None => self.step(heap, task, Input::CallResult(Value::Undefined)),
        }
    }

    fn result(
        &mut self,
        heap: &Heap,
        task: TaskId,
        value: Value,
        done: bool,
    ) -> Result<Action, Fault> {
        if done {
            cursor(heap, task, &self.iterator, |data| {
                data.done = true;
                data.source = Value::Undefined;
            })?;
        }
        object(heap, task, &[("value", value), ("done", Value::Bool(done))]).map(Action::Return)
    }

    pub(super) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
    ) -> Result<Action, Fault> {
        match self.stage {
            Stage::AsyncMethod => self.read(
                heap,
                task,
                self.source.clone(),
                property::Key::Symbol(crate::value::symbol::well_known("asyncIterator")),
                Stage::AsyncInvoke,
            ),
            Stage::AsyncInvoke => {
                let method = Self::value(input)?;
                if matches!(method, Value::Null | Value::Undefined) {
                    self.fallback = true;
                    self.stage = Stage::Method;
                    Ok(Action::Continue)
                } else {
                    self.stage = Stage::Invoke;
                    self.step(heap, task, Input::CallResult(method))
                }
            }
            Stage::MapEntry => {
                // One live entry per budgeted step. The logical cursor
                // advances before reentry, and the heap read
                // ends before invoking JavaScript or reaching a safepoint.
                let entry = map(heap, task, &self.source, is_set(self.kind), |m| {
                    m.entry(self.index)
                        .map(|(next, entry)| (next, entry.clone()))
                })?;
                let Some((next, (key, value))) = entry else {
                    return Ok(Action::Return(Value::Undefined));
                };
                self.index = next;
                let value = if is_set(self.kind) {
                    key.clone()
                } else {
                    value
                };
                Ok(Action::Call {
                    callee: self.callback.clone(),
                    this: self.this_arg.clone(),
                    args: vec![value, key, self.source.clone()],
                })
            }
            Stage::KnownMethod => {
                let method = self.iterator.clone();
                self.stage = Stage::Invoke;
                self.step(heap, task, Input::CallResult(method))
            }
            Stage::Method => {
                if matches!(self.source, Value::Bool(_)) {
                    return Err(Fault::TypeError("value is not iterable".into()));
                }
                self.read(
                    heap,
                    task,
                    self.source.clone(),
                    property::Key::Symbol(crate::value::symbol::well_known("iterator")),
                    Stage::Invoke,
                )
            }
            Stage::Invoke => {
                let method = Self::value(input)?;
                dispatch::callee(heap, &method)?;
                self.stage = Stage::NextMethod;
                Ok(Action::Call {
                    callee: method,
                    this: self.source.clone(),
                    args: vec![],
                })
            }
            Stage::NextMethod => {
                self.iterator = Self::value(input)?;
                if !matches!(self.iterator, Value::Object(_)) {
                    return Err(Fault::TypeError("iterator must be an object".into()));
                }
                self.read(
                    heap,
                    task,
                    self.iterator.clone(),
                    property::Key::Name(JsString::from("next")),
                    Stage::Record,
                )
            }
            Stage::Record => {
                let mut next = Self::value(input)?;
                if self.fallback {
                    (self.iterator, next) = super::super::async_from_sync::create(
                        heap,
                        task,
                        self.iterator.clone(),
                        next,
                    )?;
                }
                object(
                    heap,
                    task,
                    &[("iterator", self.iterator.clone()), ("next", next)],
                )
                .map(Action::Return)
            }
            Stage::StringConvert => {
                self.stage = Stage::StringCreate;
                Ok(Action::ToString(self.source.clone()))
            }
            Stage::StringCreate => {
                create(heap, task, &Self::value(input)?, Kind::String).map(Action::Return)
            }
            Stage::Length => {
                if self.done {
                    return self.result(heap, task, Value::Undefined, true);
                }
                if is_set(self.kind) || is_map(self.kind) {
                    let entry = map(heap, task, &self.source, is_set(self.kind), |m| {
                        m.entry(self.index)
                            .map(|(next, entry)| (next, entry.clone()))
                    })?;
                    let Some((next, (key, value))) = entry else {
                        return self.result(heap, task, Value::Undefined, true);
                    };
                    cursor(heap, task, &self.iterator, |data| data.index = next)?;
                    let value = match self.kind {
                        Kind::SetEntries => {
                            builtin::array_constructor(heap, task, &[key.clone(), key])?
                        }
                        Kind::MapEntries => builtin::array_constructor(heap, task, &[key, value])?,
                        Kind::MapValues => value,
                        _ => key,
                    };
                    return self.result(heap, task, value, false);
                }
                if matches!(
                    self.kind,
                    Kind::Array | Kind::ArrayKeys | Kind::ArrayEntries
                ) && matches!(self.source, Value::Bool(_))
                {
                    return self.result(heap, task, Value::Undefined, true);
                }
                if self.kind == Kind::String {
                    let Value::String(text) = &self.source else {
                        return Err(Fault::Internal("invalid string cursor"));
                    };
                    let start = self.index as usize;
                    if start >= text.len() {
                        return self.result(heap, task, Value::Undefined, true);
                    }
                    let units = text.code_units();
                    let count = if (0xd800..=0xdbff).contains(&units[start])
                        && units
                            .get(start + 1)
                            .is_some_and(|u| (0xdc00..=0xdfff).contains(u))
                    {
                        2
                    } else {
                        1
                    };
                    let value = Value::String(
                        JsString::copy_units(heap, &units[start..start + count])
                            .ok_or(Fault::HeapLimit)?,
                    );
                    cursor(heap, task, &self.iterator, |data| {
                        data.index = self.index + count as u64
                    })?;
                    return self.result(heap, task, value, false);
                }
                // Array.prototype.values may be borrowed onto a typed array:
                // the receiver's brand, rather than the factory, selects
                // internal length and detachment validation at each next.
                if let Value::Object(id) = self.source
                    && let Some(view) = crate::object::typed_array::view(heap, task, id)?
                {
                    if view.is_detached(heap)? {
                        return Err(Fault::TypeError("ArrayBuffer is detached".into()));
                    }
                    self.stage = Stage::LengthValue;
                    return self.step(
                        heap,
                        task,
                        Input::CallResult(Value::Number(view.length as f64)),
                    );
                }
                self.read(
                    heap,
                    task,
                    self.source.clone(),
                    property::Key::Name(JsString::from("length")),
                    Stage::LengthValue,
                )
            }
            Stage::LengthValue => {
                let value = Self::value(input)?;
                if matches!(value, Value::Object(_)) {
                    return Ok(Action::ToNumber(value));
                }
                if self.index >= to_length(&value)? {
                    return self.result(heap, task, Value::Undefined, true);
                }
                cursor(heap, task, &self.iterator, |data| {
                    data.index = self.index + 1
                })?;
                if self.kind == Kind::ArrayKeys {
                    return self.result(heap, task, Value::Number(self.index as f64), false);
                }
                self.stage = Stage::Element;
                Ok(Action::Continue)
            }
            Stage::Element => self.read(
                heap,
                task,
                self.source.clone(),
                index_key(self.index),
                Stage::ElementValue,
            ),
            Stage::ElementValue => {
                let mut value = Self::value(input)?;
                if self.kind == Kind::ArrayEntries {
                    value = builtin::array_constructor(
                        heap,
                        task,
                        &[Value::Number(self.index as f64), value],
                    )?;
                }
                self.result(heap, task, value, false)
            }
        }
    }
}
