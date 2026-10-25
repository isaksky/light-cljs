//! Standard Promise capabilities, combinators and finally. Every user property
//! read, call and construction is a trampoline action. Private closure captures
//! are ordinary task-owned environments, traced through their built-in function;
//! publication therefore rejects these closures transitively. Intrinsic
//! resolve/reject functions retain the concurrent settlement path in `promise`.
use super::*;
use crate::vm::instr::Instr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Callback {
    Capability,
    Element(bool),
    Finally(bool),
    Thunk(bool),
}
impl Callback {
    pub(crate) fn length(self) -> u32 {
        match self {
            Self::Capability => 2,
            Self::Element(_) | Self::Finally(_) => 1,
            Self::Thunk(_) => 0,
        }
    }
}

fn result(input: Input) -> Result<Value, Fault> {
    match input {
        Input::CallResult(value) => Ok(value),
        _ => Err(Fault::Internal("missing Promise protocol result")),
    }
}
fn get(base: Value, name: &str) -> Action {
    Action::Instruction {
        instr: Instr::GetElem,
        operands: vec![base, Value::String(name.into())],
    }
}
fn call(callee: Value, this: Value, args: Vec<Value>) -> Action {
    Action::Call { callee, this, args }
}
fn intrinsic(heap: &Heap) -> Result<Value, Fault> {
    Ok(crate::realm::intrinsics(heap)
        .ok_or(Fault::OutOfMemory("realm exhausted"))?
        .global(crate::realm::Global::Promise))
}
fn environment(heap: &Heap, task: TaskId, fields: &[(&str, Value)]) -> Result<Value, Fault> {
    let prototype = crate::realm::intrinsics(heap)
        .ok_or(Fault::OutOfMemory("realm exhausted"))?
        .object_prototype;
    let mut object = OrdinaryObject::with_prototype(task, prototype);
    for (name, value) in fields {
        let key = heap
            .keys()
            .intern(&JsString::from(*name))
            .ok_or(Fault::OutOfMemory("Promise closure key"))?;
        object.define_with(key, value.clone(), PropertyFlags::DATA);
    }
    alloc(heap, HeapObject::Plain(object)).map(Value::Object)
}
fn field(heap: &Heap, task: TaskId, env: &Value, name: &str) -> Result<Value, Fault> {
    property::get_field(heap, task, env, &name.into())
}
fn put(heap: &Heap, task: TaskId, env: &Value, name: &str, value: Value) -> Result<(), Fault> {
    property::define_field(heap, task, env, &name.into(), value)
}
fn closure(heap: &Heap, env: &Value, callback: Callback) -> Result<Value, Fault> {
    let Value::Object(id) = env else {
        return Err(Fault::Internal("missing Promise environment"));
    };
    function(heap, Builtin::PromiseCallback(*id, callback))
}

/// Engine observers must receive a local rejection for an inaccessible result:
/// rejecting only then's discarded derived promise would strand a combinator
/// or skip finally's cleanup. No foreign value is delivered to either closure.
pub(super) fn foreign_rejection_handler(
    heap: &Heap,
    fulfilled: &Value,
    rejected: &Value,
) -> Option<Value> {
    let observer = |value: &Value| {
        let Value::Object(id) = value else {
            return false;
        };
        heap.read(*id, |o| {
            matches!(
                o,
                HeapObject::Builtin(BuiltinFunction {
                    which: Builtin::PromiseCallback(_, Callback::Element(_) | Callback::Finally(_)),
                    ..
                })
            )
        })
        .unwrap_or(false)
    };
    (observer(fulfilled) || observer(rejected)).then(|| rejected.clone())
}

#[derive(Clone, Debug)]
pub(crate) struct Capability {
    pub(super) promise: Value,
    resolve: Value,
    reject: Value,
}
impl Trace for Capability {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        self.promise.trace(visit);
        self.resolve.trace(visit);
        self.reject.trace(visit);
    }
}

#[derive(Debug)]
struct NewCapability {
    constructor: Value,
    env: Value,
    executor: Value,
    called: bool,
    ready: Option<Capability>,
}
impl Trace for NewCapability {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        self.constructor.trace(visit);
        self.env.trace(visit);
        self.executor.trace(visit);
        if let Some(cap) = &self.ready {
            cap.trace(visit);
        }
    }
}
impl NewCapability {
    fn new(heap: &Heap, task: TaskId, constructor: Value) -> Result<Self, Fault> {
        // Realm constructors are immutable, so the intrinsic construction has
        // no observable new.target/prototype work or user executor to invoke.
        let (env, executor, ready) = if constructor == intrinsic(heap)? {
            let (promise, resolve, reject) = capability(heap)?;
            (
                Value::Undefined,
                Value::Undefined,
                Some(Capability {
                    promise: Value::Object(promise),
                    resolve,
                    reject,
                }),
            )
        } else {
            let env = environment(
                heap,
                task,
                &[("resolve", Value::Undefined), ("reject", Value::Undefined)],
            )?;
            let executor = closure(heap, &env, Callback::Capability)?;
            (env, executor, None)
        };
        Ok(Self {
            constructor,
            env,
            executor,
            called: false,
            ready,
        })
    }
    fn step(&mut self, heap: &Heap, task: TaskId, input: Input) -> Result<Option<Action>, Fault> {
        if self.ready.is_some() {
            return Ok(None);
        }
        if !self.called {
            self.called = true;
            return Ok(Some(Action::Construct {
                callee: self.constructor.clone(),
                args: vec![self.executor.clone()],
            }));
        }
        let promise = result(input)?;
        let resolve = field(heap, task, &self.env, "resolve")?;
        let reject = field(heap, task, &self.env, "reject")?;
        dispatch::callee(heap, &resolve)?;
        dispatch::callee(heap, &reject)?;
        self.ready = Some(Capability {
            promise,
            resolve,
            reject,
        });
        Ok(None)
    }
    fn cap(&self) -> &Capability {
        self.ready.as_ref().expect("completed capability")
    }
}

#[derive(Debug)]
struct Species {
    this: Value,
    stage: u8,
    limit: usize,
}
impl Species {
    fn step(&mut self, heap: &Heap, input: Input) -> Result<Action, Fault> {
        match self.stage {
            0 => {
                if !matches!(self.this, Value::Object(_)) {
                    return Err(Fault::TypeError(
                        "Promise species requires an object".into(),
                    ));
                }
                self.stage = 1;
                Ok(get(self.this.clone(), "constructor"))
            }
            1 => {
                let constructor = result(input)?;
                if constructor == Value::Undefined {
                    return Ok(Action::Return(intrinsic(heap)?));
                }
                if !matches!(constructor, Value::Object(_)) {
                    return Err(Fault::TypeError(
                        "Promise constructor must be an object".into(),
                    ));
                }
                self.stage = 2;
                Ok(Action::Instruction {
                    instr: Instr::GetElem,
                    operands: vec![
                        constructor,
                        Value::Symbol(crate::value::symbol::well_known("species")),
                    ],
                })
            }
            2 => {
                let constructor = result(input)?;
                if matches!(constructor, Value::Undefined | Value::Null) {
                    return Ok(Action::Return(intrinsic(heap)?));
                }
                if !dispatch::is_constructor(heap, &constructor, self.limit)? {
                    return Err(Fault::TypeError(
                        "Promise species is not a constructor".into(),
                    ));
                }
                Ok(Action::Return(constructor))
            }
            _ => Err(Fault::Internal("invalid Promise species stage")),
        }
    }
}

#[derive(Debug)]
pub(crate) enum State {
    Static(Static),
    Combinator(Combinator),
    Finally(Finally),
    Then(Then),
    Callback(CallbackState),
    Reaction(ReactionState),
}
impl State {
    pub(super) fn start(
        heap: &Heap,
        task: TaskId,
        which: Builtin,
        this: Value,
        args: &[Value],
        limit: usize,
    ) -> Result<Self, Fault> {
        let arg = |i| args.get(i).cloned().unwrap_or(Value::Undefined);
        Ok(match which {
            Builtin::PromiseResolve | Builtin::PromiseReject => {
                Self::Static(Static::new(this, arg(0), which == Builtin::PromiseReject))
            }
            Builtin::PromiseWithResolvers => Self::Static(Static {
                with_resolvers: true,
                ..Static::new(this, Value::Undefined, false)
            }),
            Builtin::PromiseAll
            | Builtin::PromiseRace
            | Builtin::PromiseAllSettled
            | Builtin::PromiseAny => Self::Combinator(Combinator::new(this, arg(0), which)),
            Builtin::PromiseFinally => Self::Finally(Finally {
                species: Species {
                    this: this.clone(),
                    stage: 0,
                    limit,
                },
                this,
                handler: arg(0),
                constructor: Value::Undefined,
                fulfilled: Value::Undefined,
                rejected: Value::Undefined,
                stage: 0,
            }),
            Builtin::PromiseThen => {
                let source = receiver(heap, &this)?;
                heap.read(source, |o| match o {
                    HeapObject::Promise(p) => p.check_owner(task),
                    _ => unreachable!(),
                })
                .ok_or(Fault::Internal("promise reclaimed"))??;
                Self::Then(Then {
                    source,
                    species: Species {
                        this,
                        stage: 0,
                        limit,
                    },
                    cap: None,
                    fulfilled: arg(0),
                    rejected: arg(1),
                })
            }
            Builtin::PromiseCallback(id, kind) => Self::Callback(CallbackState {
                env: Value::Object(id),
                kind,
                value: arg(0),
                second: arg(1),
                saved: Value::Undefined,
                resolve: None,
                stage: 0,
            }),
            _ => return Err(Fault::Internal("invalid standard Promise operation")),
        })
    }
    pub(super) fn reaction(
        cap: Capability,
        handler: Value,
        value: Value,
        rejected: bool,
        foreign: bool,
        forwarding: Option<ObjectId>,
        foreign_handler: Option<Value>,
    ) -> Self {
        Self::Reaction(ReactionState {
            cap,
            handler,
            value,
            rejected,
            foreign,
            forwarding,
            foreign_handler,
            stage: 0,
        })
    }
    pub(super) fn catches(&self) -> bool {
        match self {
            Self::Combinator(c) => {
                c.cap.as_ref().is_some_and(|c| c.ready.is_some())
                    && !matches!(c.stage, CombineStage::Reject | CombineStage::Return)
            }
            Self::Reaction(r) => r.stage == 1,
            _ => false,
        }
    }
    pub(super) fn failed(&mut self, value: Value) {
        match self {
            Self::Combinator(c) => {
                if c.pending.is_none() {
                    c.pending = Some(value);
                }
                c.stage = if c.close {
                    c.close = false;
                    CombineStage::Close
                } else {
                    CombineStage::Reject
                };
            }
            Self::Reaction(r) => {
                r.value = value;
                r.rejected = true;
                r.stage = 2;
            }
            _ => unreachable!(),
        }
    }
    pub(super) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        inbox: &Weak<JobInbox>,
        input: Input,
    ) -> Result<Action, Fault> {
        match self {
            Self::Static(s) => s.step(heap, task, input),
            Self::Combinator(c) => c.step(heap, task, input),
            Self::Finally(f) => f.step(heap, task, input),
            Self::Then(t) => t.step(heap, task, inbox, input),
            Self::Callback(c) => c.step(heap, task, input),
            Self::Reaction(r) => r.step(heap, task, inbox, input),
        }
    }
}
impl Trace for State {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        match self {
            Self::Static(s) => s.trace(visit),
            Self::Combinator(c) => c.trace(visit),
            Self::Finally(f) => f.trace(visit),
            Self::Then(t) => t.trace(visit),
            Self::Callback(c) => c.trace(visit),
            Self::Reaction(r) => r.trace(visit),
        }
    }
}

#[derive(Debug)]
pub(crate) struct Static {
    constructor: Value,
    value: Value,
    reject: bool,
    with_resolvers: bool,
    stage: u8,
    cap: Option<NewCapability>,
}
impl Static {
    fn new(constructor: Value, value: Value, reject: bool) -> Self {
        Self {
            constructor,
            value,
            reject,
            with_resolvers: false,
            stage: 0,
            cap: None,
        }
    }
    fn step(&mut self, heap: &Heap, task: TaskId, input: Input) -> Result<Action, Fault> {
        match self.stage {
            0 => {
                if !matches!(self.constructor, Value::Object(_)) {
                    return Err(Fault::TypeError(
                        "Promise constructor must be an object".into(),
                    ));
                }
                if !self.reject && !self.with_resolvers && receiver(heap, &self.value).is_ok() {
                    let id = receiver(heap, &self.value)?;
                    heap.read(id, |o| match o {
                        HeapObject::Promise(p) => p.check_owner(task),
                        _ => unreachable!(),
                    })
                    .ok_or(Fault::Internal("promise reclaimed"))??;
                    self.stage = 1;
                    return Ok(get(self.value.clone(), "constructor"));
                }
                self.stage = 2;
                Ok(Action::Continue)
            }
            1 => {
                if result(input)? == self.constructor {
                    return Ok(Action::Return(self.value.clone()));
                }
                self.stage = 2;
                Ok(Action::Continue)
            }
            2 => {
                self.cap = Some(NewCapability::new(heap, task, self.constructor.clone())?);
                self.stage = 3;
                Ok(Action::Continue)
            }
            3 => {
                let cap = self.cap.as_mut().unwrap();
                if let Some(action) = cap.step(heap, task, input)? {
                    return Ok(action);
                }
                let cap = cap.cap();
                if self.with_resolvers {
                    let record = environment(
                        heap,
                        task,
                        &[
                            ("promise", cap.promise.clone()),
                            ("resolve", cap.resolve.clone()),
                            ("reject", cap.reject.clone()),
                        ],
                    )?;
                    return Ok(Action::Return(record));
                }
                self.stage = 4;
                Ok(call(
                    if self.reject {
                        cap.reject.clone()
                    } else {
                        cap.resolve.clone()
                    },
                    Value::Undefined,
                    vec![self.value.clone()],
                ))
            }
            4 => Ok(Action::Return(
                self.cap.as_ref().unwrap().cap().promise.clone(),
            )),
            _ => Err(Fault::Internal("invalid Promise static stage")),
        }
    }
}
impl Trace for Static {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        self.constructor.trace(visit);
        self.value.trace(visit);
        if let Some(cap) = &self.cap {
            cap.trace(visit);
        }
    }
}

#[derive(Debug)]
pub(crate) struct Then {
    source: ObjectId,
    species: Species,
    cap: Option<NewCapability>,
    fulfilled: Value,
    rejected: Value,
}
impl Then {
    fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        inbox: &Weak<JobInbox>,
        input: Input,
    ) -> Result<Action, Fault> {
        if self.cap.is_none() {
            let action = self.species.step(heap, input)?;
            if let Action::Return(constructor) = action {
                self.cap = Some(NewCapability::new(heap, task, constructor)?);
                return Ok(Action::Continue);
            }
            return Ok(action);
        }
        let cap = self.cap.as_mut().unwrap();
        if let Some(action) = cap.step(heap, task, input)? {
            return Ok(action);
        }
        let cap = cap.cap().clone();
        let handler = |value: &Value| {
            if dispatch::callee(heap, value).is_ok() {
                value.clone()
            } else {
                Value::Undefined
            }
        };
        let fulfilled = handler(&self.fulfilled);
        let rejected = handler(&self.rejected);
        let forwarding = forwarding_guard(heap, &fulfilled, &rejected);
        let record = alloc(
            heap,
            HeapObject::PromiseReaction(Reaction {
                source: self.source,
                derived: self.source,
                fulfilled,
                rejected,
                forwarding,
                async_state: None,
                capability: Some(cap.clone()),
            }),
        )?;
        let callable = function(heap, Builtin::PromiseReaction(record))?;
        register(heap, inbox, self.source, callable)?;
        Ok(Action::Return(cap.promise))
    }
}
impl Trace for Then {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        visit(self.source.into());
        self.species.this.trace(visit);
        self.fulfilled.trace(visit);
        self.rejected.trace(visit);
        if let Some(cap) = &self.cap {
            cap.trace(visit);
        }
    }
}

#[derive(Debug)]
pub(crate) struct ReactionState {
    cap: Capability,
    handler: Value,
    value: Value,
    rejected: bool,
    foreign: bool,
    forwarding: Option<ObjectId>,
    foreign_handler: Option<Value>,
    stage: u8,
}
impl ReactionState {
    fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        inbox: &Weak<JobInbox>,
        input: Input,
    ) -> Result<Action, Fault> {
        match self.stage {
            0 => {
                if self.foreign {
                    self.stage = 3;
                    let callee = crate::realm::intrinsics(heap)
                        .unwrap()
                        .global(crate::realm::Global::TypeError);
                    return Ok(call(
                        callee,
                        Value::Undefined,
                        vec![Value::String(
                            "promise result belongs to another task".into(),
                        )],
                    ));
                }
                if dispatch::callee(heap, &self.handler).is_err() {
                    self.stage = 2;
                    return Ok(Action::Continue);
                }
                self.stage = 1;
                Ok(call(
                    self.handler.clone(),
                    Value::Undefined,
                    vec![self.value.clone()],
                ))
            }
            1 => {
                self.value = result(input)?;
                self.rejected = false;
                self.stage = 2;
                Ok(Action::Continue)
            }
            2 => {
                self.stage = 4;
                Ok(call(
                    if self.rejected {
                        self.cap.reject.clone()
                    } else {
                        self.cap.resolve.clone()
                    },
                    Value::Undefined,
                    vec![self.value.clone()],
                ))
            }
            3 => {
                self.value = result(input)?;
                self.rejected = true;
                if let Some(handler) = self.foreign_handler.take() {
                    self.handler = handler;
                    self.foreign = false;
                    self.stage = 0;
                    return Ok(Action::Continue);
                }
                if let Some(guard) = self.forwarding {
                    if let Some(target) = claim(heap, task, guard)? {
                        settle(heap, task, inbox, target, true, self.value.clone())?;
                    }
                    self.value = Value::Undefined;
                    self.rejected = false;
                }
                self.stage = 2;
                Ok(Action::Continue)
            }
            4 => Ok(Action::Return(Value::Undefined)),
            _ => Err(Fault::Internal("invalid Promise reaction stage")),
        }
    }
}
impl Trace for ReactionState {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        self.cap.trace(visit);
        self.handler.trace(visit);
        self.value.trace(visit);
        if let Some(handler) = &self.foreign_handler {
            handler.trace(visit);
        }
        if let Some(guard) = self.forwarding {
            visit(guard.into());
        }
    }
}

#[derive(Debug)]
enum CombineStage {
    Start,
    Capability,
    ResolveMethod,
    Iterator,
    IteratorRecord,
    Next,
    NextResult,
    Done,
    Value,
    Resolved,
    ThenMethod,
    ThenResult,
    Finish,
    Aggregate,
    Settle,
    Close,
    CloseMethod,
    CloseResult,
    Reject,
    Return,
}
#[derive(Debug)]
pub(crate) struct Combinator {
    constructor: Value,
    items: Value,
    which: Builtin,
    cap: Option<NewCapability>,
    resolve: Value,
    iterator: Value,
    next: Value,
    current: Value,
    group: Value,
    fulfilled: Value,
    rejected: Value,
    index: u64,
    close: bool,
    pending: Option<Value>,
    stage: CombineStage,
}
impl Combinator {
    fn new(constructor: Value, items: Value, which: Builtin) -> Self {
        Self {
            constructor,
            items,
            which,
            cap: None,
            resolve: Value::Undefined,
            iterator: Value::Undefined,
            next: Value::Undefined,
            current: Value::Undefined,
            group: Value::Undefined,
            fulfilled: Value::Undefined,
            rejected: Value::Undefined,
            index: 0,
            close: false,
            pending: None,
            stage: CombineStage::Start,
        }
    }
    fn cap(&self) -> &Capability {
        self.cap.as_ref().unwrap().cap()
    }
    fn step(&mut self, heap: &Heap, task: TaskId, input: Input) -> Result<Action, Fault> {
        match self.stage {
            CombineStage::Start => {
                self.cap = Some(NewCapability::new(heap, task, self.constructor.clone())?);
                self.stage = CombineStage::Capability;
                Ok(Action::Continue)
            }
            CombineStage::Capability => {
                if let Some(action) = self.cap.as_mut().unwrap().step(heap, task, input)? {
                    return Ok(action);
                }
                self.stage = CombineStage::ResolveMethod;
                Ok(get(self.constructor.clone(), "resolve"))
            }
            CombineStage::ResolveMethod => {
                self.resolve = result(input)?;
                dispatch::callee(heap, &self.resolve)?;
                self.stage = CombineStage::Iterator;
                Ok(Action::Instruction {
                    instr: Instr::GetIterator,
                    operands: vec![self.items.clone()],
                })
            }
            CombineStage::Iterator => {
                let record = result(input)?;
                self.iterator = field(heap, task, &record, "iterator")?;
                self.next = field(heap, task, &record, "next")?;
                self.stage = CombineStage::IteratorRecord;
                Ok(Action::Continue)
            }
            CombineStage::IteratorRecord => {
                let values = property::new_array(heap, task)?;
                let kind = match self.which {
                    Builtin::PromiseAllSettled => 1.,
                    Builtin::PromiseAny => 2.,
                    _ => 0.,
                };
                self.group = environment(
                    heap,
                    task,
                    &[
                        ("values", values),
                        ("remaining", Value::Number(1.)),
                        ("resolve", self.cap().resolve.clone()),
                        ("reject", self.cap().reject.clone()),
                        ("kind", Value::Number(kind)),
                    ],
                )?;
                self.stage = CombineStage::Next;
                Ok(Action::Continue)
            }
            CombineStage::Next => {
                self.close = false;
                self.stage = CombineStage::NextResult;
                Ok(call(self.next.clone(), self.iterator.clone(), vec![]))
            }
            CombineStage::NextResult => {
                self.current = result(input)?;
                if !matches!(self.current, Value::Object(_)) {
                    return Err(Fault::TypeError("iterator result must be an object".into()));
                }
                self.stage = CombineStage::Done;
                Ok(get(self.current.clone(), "done"))
            }
            CombineStage::Done => {
                if result(input)?.truthy() {
                    self.stage = CombineStage::Finish;
                    return Ok(Action::Continue);
                }
                self.stage = CombineStage::Value;
                Ok(get(self.current.clone(), "value"))
            }
            CombineStage::Value => {
                let value = result(input)?;
                self.close = true;
                if self.which != Builtin::PromiseRace {
                    if self.index >= u64::from(u32::MAX) {
                        return Err(Fault::RangeError(
                            "invalid Promise result array length".into(),
                        ));
                    }
                    let values = field(heap, task, &self.group, "values")?;
                    property::append(heap, task, &values, Some(Value::Undefined))?;
                }
                self.stage = CombineStage::Resolved;
                Ok(call(
                    self.resolve.clone(),
                    self.constructor.clone(),
                    vec![value],
                ))
            }
            CombineStage::Resolved => {
                self.current = result(input)?;
                if self.which == Builtin::PromiseRace {
                    self.fulfilled = self.cap().resolve.clone();
                    self.rejected = self.cap().reject.clone();
                } else {
                    let env = environment(
                        heap,
                        task,
                        &[
                            ("group", self.group.clone()),
                            ("index", Value::Number(self.index as f64)),
                            ("called", Value::Bool(false)),
                        ],
                    )?;
                    let reject = self.which == Builtin::PromiseAny;
                    let element = closure(heap, &env, Callback::Element(reject))?;
                    self.fulfilled = if reject {
                        self.cap().resolve.clone()
                    } else {
                        element.clone()
                    };
                    self.rejected = if self.which == Builtin::PromiseAllSettled {
                        closure(heap, &env, Callback::Element(true))?
                    } else if reject {
                        element
                    } else {
                        self.cap().reject.clone()
                    };
                    remaining(heap, task, &self.group, 1.)?;
                }
                self.stage = CombineStage::ThenMethod;
                Ok(get(self.current.clone(), "then"))
            }
            CombineStage::ThenMethod => {
                let then = result(input)?;
                self.stage = CombineStage::ThenResult;
                Ok(call(
                    then,
                    self.current.clone(),
                    vec![self.fulfilled.clone(), self.rejected.clone()],
                ))
            }
            CombineStage::ThenResult => {
                if self.which != Builtin::PromiseRace {
                    self.index += 1;
                }
                self.current = Value::Undefined;
                self.fulfilled = Value::Undefined;
                self.rejected = Value::Undefined;
                self.close = false;
                self.stage = CombineStage::Next;
                Ok(Action::Continue)
            }
            CombineStage::Finish => {
                self.close = false;
                if self.which == Builtin::PromiseRace
                    || remaining(heap, task, &self.group, -1.)? != 0.
                {
                    return Ok(Action::Return(self.cap().promise.clone()));
                }
                self.current = field(heap, task, &self.group, "values")?;
                if self.which == Builtin::PromiseAny {
                    self.stage = CombineStage::Aggregate;
                    Ok(aggregate(heap, self.current.clone())?)
                } else {
                    self.stage = CombineStage::Settle;
                    Ok(call(
                        self.cap().resolve.clone(),
                        Value::Undefined,
                        vec![self.current.clone()],
                    ))
                }
            }
            CombineStage::Aggregate => {
                self.current = result(input)?;
                self.stage = CombineStage::Settle;
                Ok(call(
                    self.cap().reject.clone(),
                    Value::Undefined,
                    vec![self.current.clone()],
                ))
            }
            CombineStage::Settle => Ok(Action::Return(self.cap().promise.clone())),
            CombineStage::Close => {
                self.stage = CombineStage::CloseMethod;
                Ok(get(self.iterator.clone(), "return"))
            }
            CombineStage::CloseMethod => {
                let method = result(input)?;
                if matches!(method, Value::Undefined | Value::Null) {
                    self.stage = CombineStage::Reject;
                    Ok(Action::Continue)
                } else {
                    self.stage = CombineStage::CloseResult;
                    Ok(call(method, self.iterator.clone(), vec![]))
                }
            }
            CombineStage::CloseResult => {
                self.stage = CombineStage::Reject;
                Ok(Action::Continue)
            }
            CombineStage::Reject => {
                self.stage = CombineStage::Return;
                Ok(call(
                    self.cap().reject.clone(),
                    Value::Undefined,
                    vec![self.pending.take().unwrap()],
                ))
            }
            CombineStage::Return => Ok(Action::Return(self.cap().promise.clone())),
        }
    }
}
impl Trace for Combinator {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        for value in [
            &self.constructor,
            &self.items,
            &self.resolve,
            &self.iterator,
            &self.next,
            &self.current,
            &self.group,
            &self.fulfilled,
            &self.rejected,
        ] {
            value.trace(visit);
        }
        if let Some(cap) = &self.cap {
            cap.trace(visit);
        }
        if let Some(value) = &self.pending {
            value.trace(visit);
        }
    }
}
fn remaining(heap: &Heap, task: TaskId, group: &Value, delta: f64) -> Result<f64, Fault> {
    let Value::Number(n) = field(heap, task, group, "remaining")? else {
        return Err(Fault::Internal("invalid remaining count"));
    };
    put(heap, task, group, "remaining", Value::Number(n + delta))?;
    Ok(n + delta)
}
fn aggregate(heap: &Heap, errors: Value) -> Result<Action, Fault> {
    Ok(call(
        crate::realm::intrinsics(heap)
            .ok_or(Fault::OutOfMemory("realm exhausted"))?
            .global(crate::realm::Global::AggregateError),
        Value::Undefined,
        vec![errors],
    ))
}

#[derive(Debug)]
pub(crate) struct Finally {
    species: Species,
    this: Value,
    handler: Value,
    constructor: Value,
    fulfilled: Value,
    rejected: Value,
    stage: u8,
}
impl Finally {
    fn step(&mut self, heap: &Heap, task: TaskId, input: Input) -> Result<Action, Fault> {
        match self.stage {
            0 => {
                let action = self.species.step(heap, input)?;
                if let Action::Return(constructor) = action {
                    self.constructor = constructor;
                    if dispatch::callee(heap, &self.handler).is_ok() {
                        let env = environment(
                            heap,
                            task,
                            &[
                                ("constructor", self.constructor.clone()),
                                ("callback", self.handler.clone()),
                            ],
                        )?;
                        self.fulfilled = closure(heap, &env, Callback::Finally(false))?;
                        self.rejected = closure(heap, &env, Callback::Finally(true))?;
                    } else {
                        self.fulfilled = self.handler.clone();
                        self.rejected = self.handler.clone();
                    }
                    self.stage = 1;
                    return Ok(get(self.this.clone(), "then"));
                }
                Ok(action)
            }
            1 => {
                let then = result(input)?;
                self.stage = 2;
                Ok(call(
                    then,
                    self.this.clone(),
                    vec![self.fulfilled.clone(), self.rejected.clone()],
                ))
            }
            2 => Ok(Action::Return(result(input)?)),
            _ => Err(Fault::Internal("invalid Promise finally stage")),
        }
    }
}
impl Trace for Finally {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        for v in [
            &self.species.this,
            &self.this,
            &self.handler,
            &self.constructor,
            &self.fulfilled,
            &self.rejected,
        ] {
            v.trace(visit);
        }
    }
}

#[derive(Debug)]
pub(crate) struct CallbackState {
    env: Value,
    kind: Callback,
    value: Value,
    second: Value,
    saved: Value,
    resolve: Option<Static>,
    stage: u8,
}
impl CallbackState {
    fn step(&mut self, heap: &Heap, task: TaskId, input: Input) -> Result<Action, Fault> {
        match self.kind {
            Callback::Capability => {
                // The specified guard tests the stored values, rather than
                // whether the executor was called (undefined/undefined may repeat).
                if field(heap, task, &self.env, "resolve")? != Value::Undefined
                    || field(heap, task, &self.env, "reject")? != Value::Undefined
                {
                    return Err(Fault::TypeError(
                        "Promise capability executor called twice".into(),
                    ));
                }
                put(heap, task, &self.env, "resolve", self.value.clone())?;
                put(heap, task, &self.env, "reject", self.second.clone())?;
                Ok(Action::Return(Value::Undefined))
            }
            Callback::Thunk(rejected) => {
                let value = field(heap, task, &self.env, "value")?;
                if rejected {
                    Err(Fault::Thrown(value))
                } else {
                    Ok(Action::Return(value))
                }
            }
            Callback::Element(rejected) => self.element(heap, task, input, rejected),
            Callback::Finally(rejected) => match self.stage {
                0 => {
                    let callback = field(heap, task, &self.env, "callback")?;
                    self.stage = 1;
                    Ok(call(callback, Value::Undefined, vec![]))
                }
                1 => {
                    let constructor = field(heap, task, &self.env, "constructor")?;
                    self.resolve = Some(Static::new(constructor, result(input)?, false));
                    self.stage = 2;
                    Ok(Action::Continue)
                }
                2 => {
                    let action = self.resolve.as_mut().unwrap().step(heap, task, input)?;
                    if let Action::Return(promise) = action {
                        self.saved = promise;
                        self.stage = 3;
                        return Ok(get(self.saved.clone(), "then"));
                    }
                    Ok(action)
                }
                3 => {
                    let then = result(input)?;
                    let env = environment(heap, task, &[("value", self.value.clone())])?;
                    let thunk = closure(heap, &env, Callback::Thunk(rejected))?;
                    self.stage = 4;
                    Ok(call(then, self.saved.clone(), vec![thunk]))
                }
                4 => Ok(Action::Return(result(input)?)),
                _ => Err(Fault::Internal("invalid finally callback stage")),
            },
        }
    }
    fn element(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
        rejected: bool,
    ) -> Result<Action, Fault> {
        if self.stage == 2 {
            return Ok(Action::Return(Value::Undefined));
        }
        let group = field(heap, task, &self.env, "group")?;
        if self.stage == 1 {
            self.stage = 2;
            return Ok(call(
                field(heap, task, &group, "reject")?,
                Value::Undefined,
                vec![result(input)?],
            ));
        }
        if field(heap, task, &self.env, "called")?.truthy() {
            return Ok(Action::Return(Value::Undefined));
        }
        put(heap, task, &self.env, "called", Value::Bool(true))?;
        let values = field(heap, task, &group, "values")?;
        let index = field(heap, task, &self.env, "index")?;
        let kind = field(heap, task, &group, "kind")?;
        let value = if kind == Value::Number(1.) {
            environment(
                heap,
                task,
                &[
                    (
                        "status",
                        Value::String(if rejected { "rejected" } else { "fulfilled" }.into()),
                    ),
                    (
                        if rejected { "reason" } else { "value" },
                        self.value.clone(),
                    ),
                ],
            )?
        } else {
            self.value.clone()
        };
        let Value::Object(id) = values.clone() else {
            return Err(Fault::Internal("invalid Promise values array"));
        };
        let Value::Number(index) = index else {
            return Err(Fault::Internal("invalid Promise element index"));
        };
        property::define_own_property(
            heap,
            task,
            id,
            &Key::Index(index as u32),
            &crate::object::PropertyDescriptor {
                value: Some(value),
                writable: Some(true),
                enumerable: Some(true),
                configurable: Some(true),
                ..Default::default()
            },
        )?;
        if remaining(heap, task, &group, -1.)? != 0. {
            return Ok(Action::Return(Value::Undefined));
        }
        if kind == Value::Number(2.) {
            self.stage = 1;
            aggregate(heap, values)
        } else {
            self.stage = 2;
            Ok(call(
                field(heap, task, &group, "resolve")?,
                Value::Undefined,
                vec![values],
            ))
        }
    }
}
impl Trace for CallbackState {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        for v in [&self.env, &self.value, &self.second, &self.saved] {
            v.trace(visit);
        }
        if let Some(resolve) = &self.resolve {
            resolve.trace(visit);
        }
    }
}
