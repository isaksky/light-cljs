//! Proxy reads, calls, presence and enumeration. Intermediate values live in traced native
//! state; no Rust frame survives a trap, getter, budget yield or dependency wait.
//! Semantics follow QuickJS quickjs.c js_proxy_get_own_property_names and
//! js_proxy_get_own_property, revision 7d1c13c5d8f8 (fresh Rust implementation).
//! HasProperty follows ECMA-262 10.5.7, including target descriptor and
//! extensibility callbacks only after a falsy trap result.
use super::dispatch::{self, Fault};
use super::native::{Action, Input};
use super::{descriptor, integrity, keys, property, prototype};
use crate::heap::budget::{ByteReservation, GrowthCharge};
use crate::heap::gc::{Trace, TraceEdge};
use crate::heap::{Heap, HeapObject, ObjectId};
use crate::ids::TaskId;
use crate::object::{Property, PropertyDescriptor, validate_and_apply};
use crate::value::{JsString, Value};
use property::Key;

#[derive(Debug)]
pub(crate) struct Proxy {
    pub(crate) private: super::private::Elements,
    pub(crate) owner: TaskId,
    pub(crate) target: ObjectId,
    handler: ObjectId,
}

/// Public Proxy operations snapshot only immutable routing metadata. Private
/// slots stay on the receiver and are never copied into trap continuations.
#[derive(Clone, Debug)]
pub(crate) struct Info {
    pub(crate) target: ObjectId,
    handler: ObjectId,
}
impl Trace for Info {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        visit(self.target.into());
        visit(self.handler.into());
    }
}
impl Trace for Proxy {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        self.private.trace(visit);
        visit(self.target.into());
        visit(self.handler.into());
    }
}
pub(crate) fn construct(heap: &Heap, task: TaskId, args: &[Value]) -> Result<Value, Fault> {
    let (Some(Value::Object(target)), Some(Value::Object(handler))) = (args.first(), args.get(1))
    else {
        return Err(error("target and handler must be objects"));
    };
    heap.alloc(HeapObject::Proxy(Proxy {
        private: Default::default(),
        owner: task,
        target: *target,
        handler: *handler,
    }))
    .map(Value::Object)
    .ok_or(Fault::OutOfMemory("heap object table exhausted"))
}
pub(crate) fn inspect(heap: &Heap, task: TaskId, id: ObjectId) -> Result<Option<Info>, Fault> {
    // Immutable realm views are authoritative. Intrinsic property reads must
    // retain their lock-free path, including while another worker holds the
    // object table for collection or allocation.
    if heap
        .realm()
        .get()
        .is_some_and(|realm| realm.view(id).is_some())
    {
        return Ok(None);
    }
    heap.read(id, |o| match o {
        HeapObject::Proxy(p) if p.owner != task => Err(Fault::ForeignObject {
            owner: Some(p.owner),
            caller: task,
        }),
        HeapObject::Proxy(p) => Ok(Some(Info {
            target: p.target,
            handler: p.handler,
        })),
        _ => Ok(None),
    })
    .ok_or(Fault::Internal("Proxy was reclaimed"))?
}
fn error(message: &str) -> Fault {
    Fault::TypeError(format!("Proxy: {message}"))
}

/// Leaf operations forward only when their trap is absent. Supported traps use
/// Operations below instead. Null is absent, as in GetMethod.
pub(crate) fn forward(
    heap: &Heap,
    task: TaskId,
    id: ObjectId,
    trap: &str,
) -> Result<ObjectId, Fault> {
    if inspect(heap, task, id)?.is_none() {
        return Ok(id);
    }
    forward_cursor(heap, task, id, trap)
}

// Non-proxy probes made by the cursor must not reserve another cursor's
// temporary Rust stack area on small executor stacks.
#[inline(never)]
fn forward_cursor(heap: &Heap, task: TaskId, id: ObjectId, trap: &str) -> Result<ObjectId, Fault> {
    // Handler lookups may themselves encounter proxies. Keep the entire leaf
    // lookup in one bounded cursor instead of re-entering property::lookup.
    enum Work<'a> {
        Target(ObjectId, &'a str),
        Lookup(ObjectId, Key),
        Trap(ObjectId, &'a str),
        Forwarded(Key),
    }
    enum Found {
        Target(ObjectId),
        Read(Option<property::Read>),
    }
    let mut work = vec![Work::Target(id, trap)];
    let mut found = None;
    for _ in 0..4096 {
        let Some(next) = work.pop() else {
            return match found {
                Some(Found::Target(id)) => Ok(id),
                _ => Err(Fault::Internal("Proxy forwarding result missing")),
            };
        };
        match next {
            Work::Target(id, trap) => match inspect(heap, task, id)? {
                None => found = Some(Found::Target(id)),
                Some(p) => {
                    work.push(Work::Trap(p.target, trap));
                    work.push(Work::Lookup(p.handler, Key::Name(JsString::from(trap))));
                }
            },
            Work::Trap(target, trap) => {
                if !matches!(
                    found.take(),
                    Some(Found::Read(
                        None | Some(property::Read::Value(Value::Undefined | Value::Null))
                    ))
                ) {
                    return Err(Fault::TypeError(format!("unsupported Proxy trap: {trap}")));
                }
                work.push(Work::Target(target, trap));
            }
            Work::Lookup(id, key) => {
                if inspect(heap, task, id)?.is_some() {
                    work.push(Work::Forwarded(key));
                    work.push(Work::Target(id, "get"));
                } else if let Some(p) = property::get_own_property(heap, task, id, &key)? {
                    found = Some(Found::Read(Some(property::Read::property(&p, false))));
                } else if let Some(proto) = prototype::get_prototype_of(heap, task, id)? {
                    work.push(Work::Lookup(proto, key));
                } else {
                    found = Some(Found::Read(None));
                }
            }
            Work::Forwarded(key) => {
                let Some(Found::Target(target)) = found.take() else {
                    return Err(Fault::Internal("Proxy handler target missing"));
                };
                work.push(Work::Lookup(target, key));
            }
        }
    }
    Err(Fault::StackOverflow(
        "maximum Proxy forwarding depth exceeded".into(),
    ))
}

/// Classification alone observes no traps (IsCallable / IsConstructor).
pub(crate) fn unwrapped(heap: &Heap, mut id: ObjectId) -> Result<ObjectId, Fault> {
    for _ in 0..512 {
        let target = heap
            .read(id, |o| match o {
                HeapObject::Proxy(p) => Some(p.target),
                _ => None,
            })
            .ok_or(Fault::Internal("Proxy target was reclaimed"))?;
        match target {
            Some(target) => id = target,
            None => return Ok(id),
        }
    }
    Err(error("classification depth exceeded"))
}

#[derive(Debug)]
pub(crate) enum Output {
    Value(Value),
    Keys(keys::ChargedKeys),
    Descriptor(Option<Property>),
    Boolean(bool),
}
impl Trace for Output {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        match self {
            Self::Value(v) => v.trace(visit),
            Self::Keys((keys, _)) => trace_keys(keys, visit),
            Self::Descriptor(Some(p)) => {
                p.value.trace(visit);
                if let Some((get, set)) = &p.accessor {
                    get.trace(visit);
                    set.trace(visit);
                }
            }
            _ => {}
        }
    }
}
fn trace_keys(keys: &[keys::OwnKey], visit: &mut dyn FnMut(TraceEdge)) {
    for k in keys {
        if let Key::Symbol(s) = &k.key {
            visit(TraceEdge::Symbol(s.identity()));
        }
    }
}
#[derive(Debug)]
enum Stage {
    Start,
    TrapGetter,
    Trap,
    Forward,
    TrapResult,
    GetResult,
    Length,
    LengthValue,
    Element,
    ElementValue,
    ValidateKeys,
    TargetKeys,
    CheckKeys,
    TargetKeyDescriptor,
    ReadDescriptor,
    TargetDescriptor,
    TargetDescriptorResult,
    CheckDescriptor,
    TargetExtensible,
    CheckExtensible,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Get,
    Keys,
    Descriptor,
    Has,
    Extensible,
}
#[derive(Debug)]
struct Operation {
    object: ObjectId,
    key: Option<Key>,
    kind: Kind,
    phase: Stage,
    proxy: Option<Info>,
    value: Value,
    receiver: Value,
    conversion: bool,
    keys: Vec<keys::OwnKey>,
    target_keys: Vec<keys::OwnKey>,
    /// Charges of `keys` (grown per trap-result element) and `target_keys`.
    keys_charge: GrowthCharge,
    target_charge: ByteReservation,
    index: usize,
    length: usize,
    reader: Option<descriptor::DefineProperty>,
    reported: Option<Property>,
    target_descriptor: Option<Property>,
    extensible: bool,
    missing_nonconfig: bool,
}
impl Operation {
    fn get(object: ObjectId, key: Key, receiver: Value, conversion: bool) -> Self {
        let mut op = Self::new(object, Some(key));
        op.kind = Kind::Get;
        op.receiver = receiver;
        op.conversion = conversion;
        op
    }
    fn keys(object: ObjectId) -> Self {
        Self::new(object, None)
    }
    fn descriptor(object: ObjectId, key: Key) -> Self {
        Self::new(object, Some(key))
    }
    fn has(object: ObjectId, key: Key) -> Self {
        let mut op = Self::new(object, Some(key));
        op.kind = Kind::Has;
        op
    }
    fn extensible(object: ObjectId) -> Self {
        let mut op = Self::new(object, None);
        op.kind = Kind::Extensible;
        op
    }
    fn new(object: ObjectId, key: Option<Key>) -> Self {
        Self {
            object,
            kind: if key.is_some() {
                Kind::Descriptor
            } else {
                Kind::Keys
            },
            key,
            phase: Stage::Start,
            proxy: None,
            value: Value::Undefined,
            receiver: Value::Undefined,
            conversion: false,
            keys: vec![],
            target_keys: vec![],
            keys_charge: GrowthCharge::default(),
            target_charge: ByteReservation::default(),
            index: 0,
            length: 0,
            reader: None,
            reported: None,
            target_descriptor: None,
            extensible: true,
            missing_nonconfig: false,
        }
    }
    fn trap(&self) -> &'static str {
        match self.kind {
            Kind::Get => "get",
            Kind::Keys => "ownKeys",
            Kind::Descriptor => "getOwnPropertyDescriptor",
            Kind::Has => "has",
            Kind::Extensible => "isExtensible",
        }
    }
    fn key_value(&self, heap: &Heap) -> Result<Value, Fault> {
        key_value(heap, self.key.as_ref().unwrap())
    }
}
/// A key as the value JavaScript sees, which a trap may keep: a fresh index
/// spelling is charged, and a name is imported, so a name this heap has not
/// charged (a converted primitive key's fresh spelling, a key-table or
/// constant body) reaches the trap as a charged copy.
fn key_value(heap: &Heap, key: &Key) -> Result<Value, Fault> {
    Ok(match key {
        Key::Index(i) => Value::String(keys::index_name(heap, *i)?),
        Key::Name(n) => Value::String(n.import(heap).ok_or(Fault::HeapLimit)?),
        Key::Symbol(s) => Value::Symbol(s.clone()),
    })
}
impl Trace for Operation {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        visit(self.object.into());
        if let Some(p) = &self.proxy {
            p.trace(visit);
        }
        if let Some(Key::Symbol(s)) = &self.key {
            visit(TraceEdge::Symbol(s.identity()));
        }
        self.value.trace(visit);
        self.receiver.trace(visit);
        trace_keys(&self.keys, visit);
        trace_keys(&self.target_keys, visit);
        if let Some(r) = &self.reader {
            r.trace(visit);
        }
        if let Some(p) = &self.reported {
            Output::Descriptor(Some(p.clone())).trace(visit);
        }
        if let Some(p) = &self.target_descriptor {
            Output::Descriptor(Some(p.clone())).trace(visit);
        }
    }
}
#[derive(Debug, Default)]
pub(crate) struct Operations {
    stack: Vec<Operation>,
    output: Option<Output>,
    ready: Option<Value>,
    json_keys: bool,
}
pub(crate) enum Progress {
    Action(Action),
    Done(Output),
    Ready(Value),
}
impl Trace for Operations {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        for op in &self.stack {
            op.trace(visit);
        }
        if let Some(out) = &self.output {
            out.trace(visit);
        }
        if let Some(v) = &self.ready {
            v.trace(visit);
        }
    }
}
impl Operations {
    pub(crate) fn descriptor(object: ObjectId, key: Key) -> Self {
        let mut ops = Self::default();
        ops.start(Operation::descriptor(object, key));
        ops
    }
    fn start(&mut self, op: Operation) {
        self.stack.push(op);
    }
    fn finish(&mut self, output: Output) -> Result<Progress, Fault> {
        self.stack.pop();
        self.output = Some(output);
        Ok(Progress::Action(Action::Continue))
    }
    pub(crate) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
    ) -> Result<Progress, Fault> {
        let input = self.ready.take().map(Input::CallResult).unwrap_or(input);
        match self.advance(heap, task, input)? {
            Progress::Ready(v) => {
                self.ready = Some(v);
                Ok(Progress::Action(Action::Continue))
            }
            progress => Ok(progress),
        }
    }
    fn advance(&mut self, heap: &Heap, task: TaskId, input: Input) -> Result<Progress, Fault> {
        if self.stack.is_empty() {
            return Ok(Progress::Done(
                self.output
                    .take()
                    .ok_or(Fault::Internal("Proxy operation missing result"))?,
            ));
        }
        if self.stack.len() > 512 {
            return Err(error("operation depth exceeded"));
        }
        let op = self.stack.last_mut().unwrap();
        match op.phase {
            Stage::Start => {
                if op.kind == Kind::Get {
                    let read = if op.conversion {
                        property::read_conversion
                    } else {
                        property::read
                    };
                    match read(
                        heap,
                        task,
                        &Value::Object(op.object),
                        op.key.as_ref().unwrap(),
                    )? {
                        Some(property::Read::Deferred(property::Deferred::Proxy {
                            target,
                            ..
                        })) => op.object = target,
                        Some(property::Read::Deferred(read)) => {
                            op.phase = Stage::GetResult;
                            return Ok(Progress::Action(read.action(op.receiver.clone())));
                        }
                        Some(property::Read::Value(value)) => {
                            return self.finish(Output::Value(value));
                        }
                        None => return self.finish(Output::Value(Value::Undefined)),
                    }
                }
                if op.kind == Kind::Has {
                    match property::probe(
                        heap,
                        task,
                        &Value::Object(op.object),
                        op.key.as_ref().unwrap(),
                    )? {
                        property::Presence::Value(value) => {
                            return self.finish(Output::Boolean(value));
                        }
                        property::Presence::Proxy(id) => op.object = id,
                    }
                }
                let Some(p) = inspect(heap, task, op.object)? else {
                    let out = match op.kind {
                        Kind::Get => return Err(Fault::Internal("Get lost its Proxy")),
                        Kind::Has => return Err(Fault::Internal("HasProperty lost its Proxy")),
                        Kind::Extensible => Output::Boolean(extensible(heap, task, op.object)?),
                        Kind::Descriptor => Output::Descriptor(property::get_own_property(
                            heap,
                            task,
                            op.object,
                            op.key.as_ref().unwrap(),
                        )?),
                        Kind::Keys => {
                            if self.json_keys {
                                super::json::check_key_snapshot(heap, task, op.object)?;
                            }
                            Output::Keys(keys::own_property_keys_charged(
                                heap,
                                task,
                                op.object,
                                keys::KeyFilter::All,
                            )?)
                        }
                    };
                    return self.finish(out);
                };
                op.proxy = Some(p.clone());
                op.phase = Stage::TrapGetter;
                return match property::read(
                    heap,
                    task,
                    &Value::Object(p.handler),
                    &Key::Name(JsString::from(op.trap())),
                )? {
                    Some(property::Read::Deferred(callee)) => {
                        Ok(Progress::Action(callee.action(Value::Object(p.handler))))
                    }
                    value => {
                        op.value = match value {
                            Some(property::Read::Value(v)) => v,
                            _ => Value::Undefined,
                        };
                        op.phase = Stage::Trap;
                        Ok(Progress::Action(Action::Continue))
                    }
                };
            }
            Stage::TrapGetter => {
                op.value = result(input)?;
                op.phase = Stage::Trap;
            }
            Stage::Trap => {
                let p = op.proxy.as_ref().unwrap();
                if matches!(op.value, Value::Undefined | Value::Null) {
                    let mut sub = Operation::new(p.target, op.key.clone());
                    sub.kind = op.kind;
                    sub.receiver = op.receiver.clone();
                    sub.conversion = op.conversion;
                    op.phase = Stage::Forward;
                    self.stack.push(sub);
                } else {
                    dispatch::callee(heap, &op.value).map_err(|_| error("trap is not callable"))?;
                    let mut args = vec![Value::Object(p.target)];
                    if op.key.is_some() {
                        args.push(op.key_value(heap)?);
                    }
                    if op.kind == Kind::Get {
                        args.push(op.receiver.clone());
                    }
                    op.phase = Stage::TrapResult;
                    return Ok(Progress::Action(Action::Call {
                        callee: op.value.clone(),
                        this: Value::Object(p.handler),
                        args,
                    }));
                }
            }
            Stage::Forward => {
                let out = self
                    .output
                    .take()
                    .ok_or(Fault::Internal("Proxy forwarding result missing"))?;
                return self.finish(out);
            }
            Stage::TrapResult => {
                op.value = result(input)?;
                if op.kind == Kind::Get {
                    op.phase = Stage::TargetDescriptor;
                    return Ok(Progress::Action(Action::Continue));
                }
                if op.kind == Kind::Has {
                    if op.value.truthy() {
                        return self.finish(Output::Boolean(true));
                    }
                    op.phase = Stage::TargetDescriptor;
                    return Ok(Progress::Action(Action::Continue));
                }
                if op.kind == Kind::Extensible {
                    op.phase = Stage::TargetExtensible;
                    return Ok(Progress::Action(Action::Continue));
                }
                if op.key.is_some() {
                    match &op.value {
                        Value::Undefined => op.phase = Stage::TargetDescriptor,
                        Value::Object(_) => op.phase = Stage::TargetDescriptor,
                        _ => {
                            return Err(error(
                                "getOwnPropertyDescriptor must return an object or undefined",
                            ));
                        }
                    }
                } else {
                    if !matches!(op.value, Value::Object(_)) {
                        return Err(error("ownKeys must return an array-like object"));
                    }
                    op.phase = Stage::Length;
                    return read(heap, task, &op.value, Key::Name(JsString::from("length")));
                }
            }
            Stage::Length => {
                op.phase = Stage::LengthValue;
                return Ok(Progress::Action(Action::ToNumber(result(input)?)));
            }
            Stage::LengthValue => {
                let Value::Number(n) = result(input)? else {
                    return Err(Fault::Internal("Proxy length conversion"));
                };
                let n = if n.is_nan() || n < 0.0 {
                    0.0
                } else {
                    n.floor().min(9007199254740991.0)
                };
                if self.json_keys {
                    super::json::check_property_count(n.min(usize::MAX as f64) as usize)?;
                }
                if n > u32::MAX as f64 {
                    return Err(error("ownKeys result is too large"));
                }
                op.length = n as usize;
                op.phase = Stage::Element;
            }
            Stage::Element => {
                if op.index < op.length {
                    op.phase = Stage::ElementValue;
                    return read(heap, task, &op.value, Key::Index(op.index as u32));
                }
                op.index = 0;
                op.phase = Stage::ValidateKeys;
            }
            Stage::ValidateKeys => {
                // CreateListFromArrayLike finishes every observable element
                // read before the duplicate/invariant checks begin.
                if op.index < op.keys.len() {
                    if op.keys[..op.index]
                        .iter()
                        .any(|k| k.key == op.keys[op.index].key)
                    {
                        return Err(error("ownKeys returned duplicate keys"));
                    }
                    op.index += 1;
                    return Ok(Progress::Action(Action::Continue));
                }
                op.index = 0;
                op.phase = Stage::TargetExtensible;
            }
            Stage::ElementValue => {
                let v = result(input)?;
                let key = match v {
                    Value::String(s) => Key::from_name(&s),
                    Value::Symbol(s) => Key::Symbol(s),
                    _ => return Err(error("ownKeys entries must be strings or symbols")),
                };
                // A trap result can be an array-like with a huge `length`
                // whose getter repeats one string: charge every listed key.
                op.keys_charge
                    .admit(heap, op.keys.len(), keys::SNAPSHOT_KEY_BYTES)?;
                op.keys.push(keys::OwnKey {
                    key,
                    enumerable: true,
                });
                op.index += 1;
                op.phase = Stage::Element;
            }
            Stage::TargetKeys => {
                let Some(Output::Keys((keys, charge))) = self.output.take() else {
                    return Err(Fault::Internal("Proxy target keys missing"));
                };
                op.target_keys = keys;
                op.target_charge = charge;
                op.phase = Stage::CheckKeys;
            }
            Stage::CheckKeys => {
                let p = op.proxy.as_ref().unwrap();
                if op.index == op.target_keys.len() {
                    if op.missing_nonconfig {
                        return Err(error("ownKeys omitted a non-configurable key"));
                    }
                    if !op.extensible
                        && (op.keys.len() != op.target_keys.len()
                            || op
                                .keys
                                .iter()
                                .any(|k| !op.target_keys.iter().any(|t| t.key == k.key)))
                    {
                        return Err(error("ownKeys must exactly match a non-extensible target"));
                    }
                    let keys = std::mem::take(&mut op.keys);
                    let charge = std::mem::take(&mut op.keys_charge).into_reservation();
                    return self.finish(Output::Keys((keys, charge)));
                }
                let key = op.target_keys[op.index].key.clone();
                op.phase = Stage::TargetKeyDescriptor;
                let target = p.target;
                self.stack.push(Operation::descriptor(target, key));
            }
            Stage::TargetKeyDescriptor => {
                let Some(Output::Descriptor(desc)) = self.output.take() else {
                    return Err(Fault::Internal("Proxy target descriptor missing"));
                };
                if desc.is_some_and(|p| !p.flags.configurable)
                    && !op
                        .keys
                        .iter()
                        .any(|k| k.key == op.target_keys[op.index].key)
                {
                    op.missing_nonconfig = true;
                }
                op.index += 1;
                op.phase = Stage::CheckKeys;
            }
            Stage::ReadDescriptor => {
                let delivered = match input {
                    Input::CallResult(v) => Some(v),
                    _ => None,
                };
                let reader = op.reader.as_mut().unwrap();
                match reader.step(heap, task, delivered)? {
                    descriptor::DefineStep::Has { this, key } => {
                        return Ok(Progress::Action(super::native::has_property(this, key)));
                    }
                    descriptor::DefineStep::Getter { callee, this } => {
                        return Ok(Progress::Action(callee.action(this)));
                    }
                    descriptor::DefineStep::Done(_) => {
                        op.reported = Some(
                            validate_and_apply(None, reader.descriptor())
                                .map_err(|_| error("invalid descriptor"))?,
                        );
                        op.phase = Stage::CheckDescriptor;
                    }
                    _ => return Err(Fault::Internal("Proxy descriptor reader attempted a write")),
                }
            }
            Stage::TargetDescriptor => {
                let p = op.proxy.as_ref().unwrap();
                op.phase = Stage::TargetDescriptorResult;
                let sub = Operation::descriptor(p.target, op.key.clone().unwrap());
                self.stack.push(sub);
            }
            Stage::TargetDescriptorResult => {
                let Some(Output::Descriptor(target)) = self.output.take() else {
                    return Err(Fault::Internal("Proxy invariant descriptor missing"));
                };
                op.target_descriptor = target;
                if op.kind == Kind::Get {
                    if let Some(target) = &op.target_descriptor
                        && !target.flags.configurable
                    {
                        match &target.accessor {
                            None if !target.flags.writable
                                && !crate::atom::same_value(&op.value, &target.value) =>
                            {
                                return Err(error(
                                    "get must return the value of a non-writable, non-configurable target property",
                                ));
                            }
                            Some((Value::Undefined, _)) if op.value != Value::Undefined => {
                                return Err(error(
                                    "get must return undefined for a non-configurable target accessor without a getter",
                                ));
                            }
                            _ => {}
                        }
                    }
                    let value = op.value.clone();
                    return self.finish(Output::Value(value));
                }
                if op.kind == Kind::Has {
                    let Some(target) = &op.target_descriptor else {
                        return self.finish(Output::Boolean(false));
                    };
                    if !target.flags.configurable {
                        return Err(error("has cannot hide a non-configurable target property"));
                    }
                    op.phase = Stage::TargetExtensible;
                    return Ok(Progress::Action(Action::Continue));
                }
                if op.value == Value::Undefined && op.target_descriptor.is_none() {
                    return self.finish(Output::Descriptor(None));
                }
                if op.value == Value::Undefined
                    && op
                        .target_descriptor
                        .as_ref()
                        .is_some_and(|p| !p.flags.configurable)
                {
                    return Err(error(
                        "descriptor cannot hide a non-configurable target property",
                    ));
                }
                op.phase = Stage::TargetExtensible;
            }
            Stage::CheckDescriptor => {
                validate_descriptor(
                    op.target_descriptor.as_ref(),
                    op.reported.as_ref(),
                    op.extensible,
                )?;
                let out = op.reported.take();
                return self.finish(Output::Descriptor(out));
            }
            Stage::GetResult => return self.finish(Output::Value(result(input)?)),
            Stage::TargetExtensible => {
                op.phase = Stage::CheckExtensible;
                let target = op.proxy.as_ref().unwrap().target;
                self.stack.push(Operation::extensible(target));
            }
            Stage::CheckExtensible => {
                let Some(Output::Boolean(extensible)) = self.output.take() else {
                    return Err(Fault::Internal("Proxy target extensibility missing"));
                };
                op.extensible = extensible;
                if op.kind == Kind::Keys {
                    op.phase = Stage::TargetKeys;
                    let target = op.proxy.as_ref().unwrap().target;
                    self.stack.push(Operation::keys(target));
                    return Ok(Progress::Action(Action::Continue));
                }
                if op.kind == Kind::Descriptor {
                    if op.value == Value::Undefined {
                        op.phase = Stage::CheckDescriptor;
                    } else {
                        op.reader = Some(descriptor::DefineProperty::reader(op.value.clone())?);
                        op.phase = Stage::ReadDescriptor;
                    }
                    return Ok(Progress::Action(Action::Continue));
                }
                if op.kind == Kind::Has {
                    if !extensible {
                        return Err(error(
                            "has cannot hide an own property of a non-extensible target",
                        ));
                    }
                    return self.finish(Output::Boolean(false));
                }
                let reported = op.value.truthy();
                if reported != extensible {
                    return Err(error("isExtensible must match the target"));
                }
                return self.finish(Output::Boolean(reported));
            }
        }
        Ok(Progress::Action(Action::Continue))
    }
}
fn extensible(heap: &Heap, task: TaskId, target: ObjectId) -> Result<bool, Fault> {
    Ok(integrity::is_extensible(heap, task, &Value::Object(target))?.truthy())
}
fn validate_descriptor(
    target: Option<&Property>,
    reported: Option<&Property>,
    extensible: bool,
) -> Result<(), Fault> {
    let Some(p) = reported else {
        if target.is_some_and(|p| !p.flags.configurable) || target.is_some() && !extensible {
            return Err(error("descriptor cannot hide a protected target property"));
        }
        return Ok(());
    };
    if target.is_none() && !extensible {
        return Err(error(
            "descriptor cannot add a property to a non-extensible target",
        ));
    }
    let desc = PropertyDescriptor {
        enumerable: Some(p.flags.enumerable),
        configurable: Some(p.flags.configurable),
        writable: p.accessor.is_none().then_some(p.flags.writable),
        value: p.accessor.is_none().then(|| p.value.clone()),
        get: p.accessor.as_ref().map(|a| a.0.clone()),
        set: p.accessor.as_ref().map(|a| a.1.clone()),
    };
    if validate_and_apply(target, &desc).is_err() {
        return Err(error("descriptor is incompatible with target"));
    }
    if !p.flags.configurable {
        if target.is_none_or(|t| t.flags.configurable) {
            return Err(error(
                "descriptor cannot invent a non-configurable property",
            ));
        }
        if p.accessor.is_none() && !p.flags.writable && target.is_some_and(|t| t.flags.writable) {
            return Err(error(
                "descriptor cannot make a non-configurable writable property read-only",
            ));
        }
    }
    Ok(())
}
fn result(input: Input) -> Result<Value, Fault> {
    match input {
        Input::CallResult(v) => Ok(v),
        _ => Err(Fault::Internal("Proxy expected callback result")),
    }
}
/// A data read is delivered by a tiny built-in-free continuation step, using
/// the same input slot as an accessor callback.
fn read(heap: &Heap, task: TaskId, base: &Value, key: Key) -> Result<Progress, Fault> {
    match property::read(heap, task, base, &key)? {
        Some(property::Read::Deferred(callee)) => Ok(Progress::Action(callee.action(base.clone()))),
        v => Ok(Progress::Ready(match v {
            Some(property::Read::Value(v)) => v,
            _ => Value::Undefined,
        })),
    }
}

#[derive(Debug)]
pub(crate) enum Mode {
    Get {
        target: ObjectId,
        key: Key,
        receiver: Value,
        conversion: bool,
    },
    Has {
        target: ObjectId,
        key: Key,
    },
    ForIn(Value),
    Keys {
        target: ObjectId,
        enumerable: bool,
        symbols: bool,
    },
    Descriptor {
        target: ObjectId,
        key: Key,
    },
    Enumerable {
        target: ObjectId,
        key: Key,
    },
    Freeze(ObjectId),
    Frozen(ObjectId),
    Seal(ObjectId),
    Sealed(ObjectId),
    Set {
        target: ObjectId,
        key: Key,
        value: Value,
        strict: bool,
    },
}
#[derive(Debug)]
pub(crate) struct State {
    mode: Mode,
    ops: Operations,
    keys: Vec<keys::OwnKey>,
    keys_charge: ByteReservation,
    selected: Vec<Value>,
    selected_charge: GrowthCharge,
    result: Value,
    emitted: usize,
    index: usize,
    pending: Option<(ObjectId, Key)>,
    started: bool,
    setter: bool,
    coercing: Option<(ObjectId, PropertyDescriptor)>,
}
impl State {
    fn select(&mut self, heap: &Heap, value: Value) -> Result<(), Fault> {
        self.selected_charge
            .admit(heap, self.selected.len(), std::mem::size_of::<Value>())?;
        self.selected.push(value);
        Ok(())
    }

    /// JSON's snapshot bound applies before filtering keys and to every
    /// target snapshot, including targets mutated by an ownKeys callback.
    pub(crate) fn json_keys(target: ObjectId) -> Self {
        let mut state = Self::new(Mode::Keys {
            target,
            enumerable: true,
            symbols: false,
        });
        state.ops.json_keys = true;
        state
    }

    pub(crate) fn new(mode: Mode) -> Self {
        Self {
            mode,
            ops: Operations::default(),
            keys: vec![],
            keys_charge: ByteReservation::default(),
            selected: vec![],
            selected_charge: GrowthCharge::default(),
            result: Value::Undefined,
            emitted: 0,
            index: 0,
            pending: None,
            started: false,
            setter: false,
            coercing: None,
        }
    }
    pub(crate) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
    ) -> Result<Action, Fault> {
        if let Some((target, mut desc)) = self.coercing.take() {
            let Mode::Set { key, value, .. } = &self.mode else {
                unreachable!()
            };
            desc.value = Some(result(input)?);
            property::define_own_property(heap, task, target, key, &desc)?;
            return Ok(Action::Return(value.clone()));
        }
        if !self.started {
            self.started = true;
            match &self.mode {
                Mode::ForIn(_) => {}
                Mode::Get {
                    target,
                    key,
                    receiver,
                    conversion,
                } => self.ops.start(Operation::get(
                    *target,
                    key.clone(),
                    receiver.clone(),
                    *conversion,
                )),
                Mode::Has { target, key } => self.ops.start(Operation::has(*target, key.clone())),
                Mode::Keys { target, .. } => self.ops.start(Operation::keys(*target)),
                Mode::Set {
                    target,
                    key,
                    value,
                    strict,
                } => {
                    let lookup = forward(heap, task, *target, "set")?;
                    match property::super_write(heap, task, &Value::Object(lookup), key, *strict)? {
                        property::SuperWrite::Setter(callee) => {
                            self.setter = true;
                            return Ok(Action::Call {
                                callee,
                                this: Value::Object(*target),
                                args: vec![value.clone()],
                            });
                        }
                        property::SuperWrite::Done => return Ok(Action::Return(value.clone())),
                        property::SuperWrite::Receiver => {
                            self.ops.start(Operation::descriptor(*target, key.clone()))
                        }
                    }
                }
                Mode::Freeze(target) | Mode::Seal(target) => {
                    integrity::prevent_extensions(heap, task, &Value::Object(*target))?;
                    self.ops.start(Operation::keys(*target));
                }
                Mode::Frozen(target) | Mode::Sealed(target) => {
                    if extensible(heap, task, *target)? {
                        return Ok(Action::Return(Value::Bool(false)));
                    }
                    self.ops.start(Operation::keys(*target));
                }
                Mode::Descriptor { target, key } | Mode::Enumerable { target, key } => {
                    self.ops.start(Operation::descriptor(*target, key.clone()))
                }
            }
        }
        if self.setter {
            let Mode::Set { value, .. } = &self.mode else {
                unreachable!()
            };
            return Ok(Action::Return(value.clone()));
        }
        if !self.ops.stack.is_empty() || self.ops.output.is_some() {
            match self.ops.step(heap, task, input)? {
                Progress::Action(a) => return Ok(a),
                Progress::Ready(_) => unreachable!(),
                Progress::Done(out) => match &self.mode {
                    Mode::Get { .. } => match out {
                        Output::Value(value) => return Ok(Action::Return(value)),
                        _ => return Err(Fault::Internal("Proxy get result missing")),
                    },
                    Mode::Has { .. } => match out {
                        Output::Boolean(value) => return Ok(Action::Return(Value::Bool(value))),
                        _ => return Err(Fault::Internal("Proxy has result missing")),
                    },
                    Mode::Set {
                        target,
                        key,
                        value,
                        strict,
                    } => {
                        let Output::Descriptor(own) = out else {
                            return Err(Fault::Internal("Proxy set descriptor missing"));
                        };
                        if own
                            .as_ref()
                            .is_some_and(|p| p.accessor.is_some() || !p.flags.writable)
                        {
                            if *strict {
                                return Err(error("cannot assign to read-only receiver property"));
                            }
                        } else {
                            let desc = PropertyDescriptor {
                                value: Some(value.clone()),
                                writable: own.is_none().then_some(true),
                                enumerable: own.is_none().then_some(true),
                                configurable: own.is_none().then_some(true),
                                ..PropertyDescriptor::default()
                            };
                            let resolved = forward(heap, task, *target, "defineProperty")?;
                            if matches!(value, Value::Object(_))
                                && property::typed_array_descriptor(
                                    heap, task, resolved, key, &desc,
                                )?
                                .is_some()
                            {
                                self.coercing = Some((resolved, desc));
                                return Ok(Action::ToNumber(value.clone()));
                            }
                            match property::define_own_property(heap, task, *target, key, &desc) {
                                Err(Fault::TypeError(message))
                                    if !strict
                                        && !message.starts_with("unsupported Proxy trap:") => {}
                                outcome => outcome?,
                            }
                        }
                        return Ok(Action::Return(value.clone()));
                    }
                    Mode::Descriptor { .. } => {
                        return match out {
                            Output::Descriptor(Some(p)) => {
                                descriptor::from_property_descriptor(heap, task, p)
                                    .map(Action::Return)
                            }
                            Output::Descriptor(None) => Ok(Action::Return(Value::Undefined)),
                            _ => Err(Fault::Internal("Proxy reflection result")),
                        };
                    }
                    Mode::Enumerable { .. } => {
                        return match out {
                            Output::Descriptor(p) => Ok(Action::Return(Value::Bool(
                                p.is_some_and(|p| p.flags.enumerable),
                            ))),
                            _ => Err(Fault::Internal("Proxy enumerable result")),
                        };
                    }
                    Mode::ForIn(iterator) => match out {
                        Output::Boolean(_) | Output::Value(_) => {
                            return Err(Fault::Internal("Proxy for-in boolean result"));
                        }
                        Output::Keys(keys) => keys::for_in_set_keys(heap, iterator, keys)?,
                        Output::Descriptor(p) => {
                            let (_, key) = self.pending.take().unwrap();
                            if p.is_none() {
                                keys::for_in_missing_key(heap, iterator)?;
                            }
                            if p.is_some_and(|p| p.flags.enumerable) {
                                return Ok(Action::ReturnPair(
                                    key_value(heap, &key)?,
                                    Value::Bool(true),
                                ));
                            }
                        }
                    },
                    Mode::Keys { .. } => match out {
                        Output::Boolean(_) | Output::Value(_) => {
                            return Err(Fault::Internal("Proxy keys boolean result"));
                        }
                        Output::Keys((keys, charge)) => {
                            self.keys = keys;
                            self.keys_charge = charge;
                        }
                        Output::Descriptor(p) => {
                            if p.is_some_and(|p| p.flags.enumerable) {
                                let value = key_value(heap, &self.keys[self.index - 1].key)?;
                                self.select(heap, value)?;
                            }
                        }
                    },
                    Mode::Freeze(target)
                    | Mode::Frozen(target)
                    | Mode::Seal(target)
                    | Mode::Sealed(target) => match out {
                        Output::Boolean(_) | Output::Value(_) => {
                            return Err(Fault::Internal("Proxy integrity boolean result"));
                        }
                        Output::Keys((keys, charge)) => {
                            self.keys = keys;
                            self.keys_charge = charge;
                        }
                        Output::Descriptor(Some(p)) => {
                            if matches!(self.mode, Mode::Frozen(_) | Mode::Sealed(_)) {
                                if p.flags.configurable
                                    || matches!(self.mode, Mode::Frozen(_))
                                        && p.accessor.is_none()
                                        && p.flags.writable
                                {
                                    return Ok(Action::Return(Value::Bool(false)));
                                }
                            } else {
                                property::define_own_property(
                                    heap,
                                    task,
                                    *target,
                                    &self.keys[self.index - 1].key,
                                    &PropertyDescriptor {
                                        configurable: Some(false),
                                        writable: (matches!(self.mode, Mode::Freeze(_))
                                            && p.accessor.is_none())
                                        .then_some(false),
                                        ..PropertyDescriptor::default()
                                    },
                                )?;
                            }
                        }
                        Output::Descriptor(None) => {}
                    },
                },
            }
        }
        match &self.mode {
            Mode::ForIn(iterator) => match keys::for_in_request(heap, task, iterator)? {
                keys::ForInRequest::Keys(object) => self.ops.start(Operation::keys(object)),
                keys::ForInRequest::Descriptor(object, key) => {
                    self.pending = Some((object, key.clone()));
                    self.ops.start(Operation::descriptor(object, key));
                }
                keys::ForInRequest::Key(key) => {
                    return Ok(Action::ReturnPair(
                        key_value(heap, &key)?,
                        Value::Bool(true),
                    ));
                }
                keys::ForInRequest::Done => {
                    return Ok(Action::ReturnPair(Value::Undefined, Value::Bool(false)));
                }
            },
            Mode::Keys {
                target,
                enumerable,
                symbols,
            } => {
                if let Some(key) = self.keys.get(self.index) {
                    self.index += 1;
                    if !symbols && matches!(key.key, Key::Symbol(_)) {
                        return Ok(Action::Continue);
                    }
                    if *enumerable {
                        self.ops
                            .start(Operation::descriptor(*target, key.key.clone()));
                        return Ok(Action::Continue);
                    }
                    let value = key_value(heap, &key.key)?;
                    self.select(heap, value)?;
                    return Ok(Action::Continue);
                }
                if self.result == Value::Undefined {
                    self.result = property::new_array(heap, task)?;
                    return Ok(Action::Continue);
                }
                if let Some(value) = self.selected.get(self.emitted) {
                    property::append(heap, task, &self.result, Some(value.clone()))?;
                    self.emitted += 1;
                    return Ok(Action::Continue);
                }
                return Ok(Action::Return(self.result.clone()));
            }
            Mode::Freeze(target)
            | Mode::Frozen(target)
            | Mode::Seal(target)
            | Mode::Sealed(target) => {
                if let Some(key) = self.keys.get(self.index) {
                    self.index += 1;
                    // SetIntegrityLevel(sealed) defines configurable:false
                    // directly. Unlike frozen, it must not observe a Proxy's
                    // getOwnPropertyDescriptor trap first.
                    if matches!(self.mode, Mode::Seal(_)) {
                        property::define_own_property(
                            heap,
                            task,
                            *target,
                            &key.key,
                            &PropertyDescriptor {
                                configurable: Some(false),
                                ..PropertyDescriptor::default()
                            },
                        )?;
                        return Ok(Action::Continue);
                    }
                    self.ops
                        .start(Operation::descriptor(*target, key.key.clone()));
                } else {
                    return Ok(Action::Return(
                        if matches!(self.mode, Mode::Freeze(_) | Mode::Seal(_)) {
                            Value::Object(*target)
                        } else {
                            Value::Bool(true)
                        },
                    ));
                }
            }
            _ => return Err(Fault::Internal("completed Proxy operation was resumed")),
        }
        Ok(Action::Continue)
    }
}
impl Trace for State {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        self.result.trace(visit);
        if let Some((target, desc)) = &self.coercing {
            visit((*target).into());
            for v in [&desc.value, &desc.get, &desc.set].into_iter().flatten() {
                v.trace(visit);
            }
        }
        match &self.mode {
            Mode::ForIn(it) => it.trace(visit),
            Mode::Keys { target, .. }
            | Mode::Get { target, .. }
            | Mode::Has { target, .. }
            | Mode::Set { target, .. }
            | Mode::Descriptor { target, .. }
            | Mode::Enumerable { target, .. } => visit((*target).into()),
            Mode::Freeze(target)
            | Mode::Frozen(target)
            | Mode::Seal(target)
            | Mode::Sealed(target) => visit((*target).into()),
        }
        match &self.mode {
            Mode::Get {
                key: Key::Symbol(s),
                ..
            } => visit(TraceEdge::Symbol(s.identity())),
            Mode::Has {
                key: Key::Symbol(s),
                ..
            }
            | Mode::Set {
                key: Key::Symbol(s),
                ..
            }
            | Mode::Descriptor {
                key: Key::Symbol(s),
                ..
            }
            | Mode::Enumerable {
                key: Key::Symbol(s),
                ..
            } => visit(TraceEdge::Symbol(s.identity())),
            _ => {}
        }
        self.ops.trace(visit);
        if let Mode::Get { receiver, .. } = &self.mode {
            receiver.trace(visit);
        }
        if let Mode::Set { value, .. } = &self.mode {
            value.trace(visit);
        }
        trace_keys(&self.keys, visit);
        for v in &self.selected {
            v.trace(visit);
        }
        if let Some((object, key)) = &self.pending {
            visit((*object).into());
            if let Key::Symbol(s) = key {
                visit(TraceEdge::Symbol(s.identity()));
            }
        }
    }
}

/// Proxy [[Call]] (ECMA-262 10.5.12). IsCallable is checked before entry;
/// GetMethod and the trap run in ordinary VM frames, never on a Rust stack.
#[derive(Debug)]
pub(crate) struct Call {
    proxy: Info,
    this: Value,
    args: Vec<Value>,
    trap: Value,
    array: Value,
    index: usize,
    stage: CallStage,
}
#[derive(Debug)]
enum CallStage {
    Start,
    Method,
    Arguments,
    Result,
}
impl Call {
    pub(crate) fn new(
        heap: &Heap,
        task: TaskId,
        call: super::builtin::Forward,
    ) -> Result<Self, Fault> {
        dispatch::callee(heap, &call.callee)?;
        let Value::Object(id) = call.callee else {
            return Err(error("call target must be an object"));
        };
        let proxy = inspect(heap, task, id)?.ok_or(Fault::Internal("Proxy call lost its Proxy"))?;
        Ok(Self {
            proxy,
            this: call.this,
            args: call.args,
            trap: Value::Undefined,
            array: Value::Undefined,
            index: 0,
            stage: CallStage::Start,
        })
    }
    pub(crate) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
    ) -> Result<Action, Fault> {
        match self.stage {
            CallStage::Start => {
                self.stage = CallStage::Method;
                match property::read(
                    heap,
                    task,
                    &Value::Object(self.proxy.handler),
                    &Key::Name("apply".into()),
                )? {
                    Some(property::Read::Deferred(read)) => {
                        return Ok(read.action(Value::Object(self.proxy.handler)));
                    }
                    Some(property::Read::Value(value)) => self.trap = value,
                    None => {}
                }
            }
            CallStage::Method => self.trap = result(input)?,
            CallStage::Arguments => {
                if let Some(value) = self.args.get(self.index) {
                    property::append(heap, task, &self.array, Some(value.clone()))?;
                    self.index += 1;
                    return Ok(Action::Continue);
                }
                self.stage = CallStage::Result;
                return Ok(Action::Call {
                    callee: self.trap.clone(),
                    this: Value::Object(self.proxy.handler),
                    args: vec![
                        Value::Object(self.proxy.target),
                        self.this.clone(),
                        self.array.clone(),
                    ],
                });
            }
            CallStage::Result => return Ok(Action::Return(result(input)?)),
        }
        if matches!(self.trap, Value::Undefined | Value::Null) {
            self.stage = CallStage::Result;
            return Ok(Action::Call {
                callee: Value::Object(self.proxy.target),
                this: self.this.clone(),
                args: self.args.clone(),
            });
        }
        dispatch::callee(heap, &self.trap).map_err(|_| error("apply trap is not callable"))?;
        self.array = property::new_array(heap, task)?;
        self.stage = CallStage::Arguments;
        Ok(Action::Continue)
    }
}
impl Trace for Call {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        self.proxy.trace(visit);
        self.this.trace(visit);
        self.trap.trace(visit);
        self.array.trace(visit);
        for arg in &self.args {
            arg.trace(visit);
        }
    }
}
