//! AddDisposableResource. Getter calls use ordinary VM frames;
//! source and progress remain traced across suspension and collection.
use super::*;
use crate::vm::builtin::Builtin;

/// A private async function activation around the shared disposal continuation.
/// The entry calls a captured intrinsic, so global/prototype mutation cannot
/// replace the operation. AsyncStart establishes the ordinary task-owned async
/// boundary before receiver validation or the synchronous first disposer call.
pub(super) fn start_async_stack(
    heap: &Heap,
    state: &mut VmState,
    this: Value,
    popped: usize,
) -> Result<(), Fault> {
    use crate::vm::{Assembler, Opcode};
    use std::sync::OnceLock;
    static DRIVER: OnceLock<crate::vm::Executable> = OnceLock::new();
    let code = DRIVER.get_or_init(|| {
        let mut asm = Assembler::new();
        asm.captures(1)
            .op(Opcode::AsyncStart)
            .op(Opcode::GetThis)
            .get_capture(0)
            .op(Opcode::CallMethod)
            .raw(0)
            .raw(0)
            .op(Opcode::Return);
        asm.finish("<AsyncDisposableStack.disposeAsync>", 0, 0)
            .expect("internal async disposal driver")
    });
    let body = crate::realm::intrinsics(heap)
        .ok_or_else(|| Fault::OutOfMemory("heap object table exhausted"))?
        .async_dispose_body;
    let function = dispatch::new_function(
        heap,
        code.0.clone(),
        &[Value::Object(body)],
        state.diag.task,
        None,
    )?;
    let Value::Object(id) = function else {
        unreachable!()
    };
    state.truncate_operands(state.stack_len() - popped)?;
    state.push(function);
    state.enter(state.code_cache.get(&code.0), id, this, 0)
}

#[derive(Debug)]
pub(super) struct State {
    source: Value,
    reading: bool,
    asynchronous: bool,
    fallback: bool,
}

impl Trace for State {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        self.source.trace(visit);
    }
}

impl State {
    pub(super) fn new(source: Value) -> Self {
        Self {
            source,
            reading: false,
            asynchronous: false,
            fallback: false,
        }
    }

    pub(super) fn asynchronous(source: Value) -> Self {
        Self {
            asynchronous: true,
            ..Self::new(source)
        }
    }

    fn result(&self, method: Value) -> Action {
        if self.asynchronous {
            Action::ReturnPair(method, Value::Number(if self.fallback { 2.0 } else { 1.0 }))
        } else {
            Action::Return(method)
        }
    }

    fn checked(&mut self, heap: &Heap, method: Value) -> Result<Action, Fault> {
        if matches!(method, Value::Undefined | Value::Null) {
            if self.asynchronous && !self.fallback {
                self.fallback = true;
                self.reading = false;
                return Ok(Action::Continue);
            }
            return Err(Fault::TypeError("value is not disposable".into()));
        }
        match dispatch::callee(heap, &method) {
            Err(Fault::NotCallable(_)) => {
                return Err(Fault::TypeError("dispose method is not a function".into()));
            }
            result => {
                result?;
            }
        }
        Ok(self.result(method))
    }

    pub(super) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
    ) -> Result<Action, Fault> {
        if self.reading {
            let Input::CallResult(method) = input else {
                return Err(Fault::Internal("dispose check expected getter result"));
            };
            return self.checked(heap, method);
        }
        if matches!(self.source, Value::Null | Value::Undefined) {
            return Ok(self.result(Value::Undefined));
        }
        if !matches!(self.source, Value::Object(_)) {
            return Err(Fault::TypeError("value is not disposable".into()));
        }
        let key = property::Key::Symbol(crate::value::symbol::well_known(
            if self.asynchronous && !self.fallback {
                "asyncDispose"
            } else {
                "dispose"
            },
        ));
        self.reading = true;
        match property::read(heap, task, &self.source, &key)? {
            Some(property::Read::Deferred(callee)) => Ok(callee.action(self.source.clone())),
            Some(property::Read::Value(method)) => self.checked(heap, method),
            None => self.checked(heap, Value::Undefined),
        }
    }
}

use crate::object::disposable_stack::{self as stack, Hint as ResourceHint, Kind, Resource};

/// The single disposal loop for lexical using scopes and DisposableStack.
/// Resources are popped before calling JS; pending errors and all remaining
/// records live here across waits, collection, migration and re-entrant calls.
#[derive(Debug)]
pub(super) struct Run {
    resources: Vec<Resource>,
    error: Option<Value>,
    failure: Option<Value>,
    changed: bool,
    cancelling: bool,
    calling: bool,
    // A disposer call must be followed by await; fallback discards its result.
    await_call: Option<bool>,
    // Async stacks await normal adopt/defer results too.
    await_all: bool,
    await_failure: bool,
    // DisposeResources coalesces nullish async records with a later async
    // call, or awaits once before the next sync record / the end of the run.
    needs_await: bool,
    has_awaited: bool,
    stack: JsString,
    reservation: crate::heap::budget::ByteReservation,
}
impl Trace for Run {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        for resource in &self.resources {
            resource.trace(visit);
        }
        if let Some(value) = &self.error {
            value.trace(visit);
        }
        if let Some(value) = &self.failure {
            value.trace(visit);
        }
    }
}
impl Run {
    pub(super) fn new(
        resources: Vec<Resource>,
        error: Option<Value>,
        cancelling: bool,
        stack: JsString,
    ) -> Self {
        Self {
            resources,
            error,
            failure: None,
            changed: false,
            cancelling,
            calling: false,
            await_call: None,
            await_all: false,
            await_failure: false,
            needs_await: false,
            has_awaited: false,
            stack,
            reservation: Default::default(),
        }
    }
    pub(super) fn catches(&self) -> bool {
        self.calling
    }
    pub(super) fn failed(&mut self, exception: super::super::exception::Exception) {
        self.failure = Some(exception.value);
        self.calling = false;
        // GetDisposeMethod's sync fallback wrapper turns a throw into a
        // rejected promise. Direct calls that throw have no result to await.
        self.await_failure = self.await_call.take() == Some(true);
    }
    pub(super) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
    ) -> Result<Action, Fault> {
        self.calling = false;
        if std::mem::take(&mut self.await_failure) {
            self.has_awaited = true;
            self.calling = true;
            return Ok(Action::Await(Value::Undefined));
        }
        if let Some(fallback) = self.await_call.take() {
            let Input::CallResult(value) = input else {
                return Err(Fault::Internal("async disposer expected call result"));
            };
            self.has_awaited = true;
            self.calling = true;
            return Ok(Action::Await(if fallback {
                Value::Undefined
            } else {
                value
            }));
        }
        if let Some(new_error) = self.failure.take() {
            self.error = Some(match self.error.take() {
                Some(previous) => {
                    error::suppressed_error(heap, task, new_error, previous, self.stack.clone())?
                }
                None => new_error,
            });
            self.changed = true;
        }
        if self.needs_await
            && !self.has_awaited
            && !self.await_all
            && self.resources.last().is_some_and(|resource| {
                !matches!(
                    resource.hint,
                    ResourceHint::Async | ResourceHint::AsyncFallback
                )
            })
        {
            // Keep the pending sync resource traced until the boundary resumes.
            self.needs_await = false;
            self.calling = true;
            return Ok(Action::Await(Value::Undefined));
        }
        if let Some(resource) = self.resources.pop() {
            if matches!(resource.method, Value::Undefined) {
                if self.await_all
                    || matches!(
                        resource.hint,
                        ResourceHint::Async | ResourceHint::AsyncFallback
                    )
                {
                    self.needs_await = true;
                }
                return Ok(Action::Continue);
            }
            let (this, args) = match resource.hint {
                ResourceHint::Sync => (resource.value, vec![]),
                ResourceHint::Adopt => (Value::Undefined, vec![resource.value]),
                ResourceHint::Defer => (Value::Undefined, vec![]),
                ResourceHint::Async | ResourceHint::AsyncFallback => {
                    self.await_call = Some(matches!(resource.hint, ResourceHint::AsyncFallback));
                    (resource.value, vec![])
                }
            };
            if self.await_all && self.await_call.is_none() {
                self.await_call = Some(false);
            }
            self.calling = true;
            return Ok(Action::Call {
                callee: resource.method,
                this,
                args,
            });
        }
        if self.needs_await && !self.has_awaited {
            self.needs_await = false;
            self.calling = true;
            return Ok(Action::Await(Value::Undefined));
        }
        if self.changed && !self.cancelling {
            return Err(Fault::Thrown(
                self.error
                    .take()
                    .ok_or_else(|| Fault::Internal("missing disposal error"))?,
            ));
        }
        Ok(Action::Return(Value::Undefined))
    }
}

#[derive(Debug)]
pub(super) enum Stack {
    Construct {
        target: Value,
        reading: bool,
        kind: Kind,
    },
    Use {
        this: Value,
        check: State,
    },
    Run(Run),
}
impl Trace for Stack {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        match self {
            Self::Construct { target, .. } => target.trace(visit),
            Self::Use { this, check } => {
                this.trace(visit);
                check.trace(visit);
            }
            Self::Run(run) => run.trace(visit),
        }
    }
}
impl Stack {
    pub(super) fn method(
        heap: &Heap,
        task: TaskId,
        which: Builtin,
        this: &Value,
        args: &[Value],
        diagnostic: JsString,
    ) -> Result<Self, Fault> {
        match which {
            Builtin::DisposableStackUse | Builtin::AsyncDisposableStackUse => {
                let kind = if which == Builtin::AsyncDisposableStackUse {
                    Kind::Async
                } else {
                    Kind::Sync
                };
                stack::access(heap, task, this, kind, true, |_| ())?;
                Ok(Self::Use {
                    this: this.clone(),
                    check: if kind == Kind::Async {
                        State::asynchronous(args.first().cloned().unwrap_or(Value::Undefined))
                    } else {
                        State::new(args.first().cloned().unwrap_or(Value::Undefined))
                    },
                })
            }
            Builtin::DisposableStackDispose | Builtin::AsyncDisposableStackDisposeBody => {
                let kind = if which == Builtin::AsyncDisposableStackDisposeBody {
                    Kind::Async
                } else {
                    Kind::Sync
                };
                let (resources, reservation) = stack::access(heap, task, this, kind, false, |s| {
                    if s.disposed {
                        (vec![], Default::default())
                    } else {
                        s.disposed = true;
                        (
                            std::mem::take(&mut s.resources),
                            std::mem::take(&mut s.reservation),
                        )
                    }
                })?;
                let mut run = Run::new(resources, None, false, diagnostic);
                run.reservation = reservation;
                run.await_all = kind == Kind::Async;
                Ok(Self::Run(run))
            }
            _ => Err(Fault::Internal("invalid DisposableStack native")),
        }
    }
    pub(super) fn catches(&self) -> bool {
        matches!(self, Self::Run(run) if run.catches())
    }
    pub(super) fn failed(&mut self, exception: super::super::exception::Exception) {
        if let Self::Run(run) = self {
            run.failed(exception);
        }
    }
    pub(super) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
    ) -> Result<Action, Fault> {
        match self {
            Self::Run(run) => run.step(heap, task, input),
            Self::Use { this, check } => {
                let action = check.step(heap, task, input)?;
                let (method, kind, hint) = match action {
                    Action::Return(method) => (method, Kind::Sync, ResourceHint::Sync),
                    Action::ReturnPair(method, _) => (
                        method,
                        Kind::Async,
                        if check.fallback {
                            ResourceHint::AsyncFallback
                        } else {
                            ResourceHint::Async
                        },
                    ),
                    _ => return Ok(action),
                };
                // AddDisposableResource checks the receiver before invoking
                // the getter. Re-entrant disposal in that getter does not
                // repeat the disposed check (ECMAScript use steps 2-6).
                if kind == Kind::Async || !matches!(method, Value::Undefined) {
                    stack::access(heap, task, this, kind, false, |s| {
                        s.push(
                            heap,
                            Resource {
                                value: check.source.clone(),
                                method,
                                hint,
                            },
                        )
                    })??;
                }
                Ok(Action::Return(check.source.clone()))
            }
            Self::Construct {
                target,
                reading,
                kind,
            } => {
                let prototype = if *reading {
                    let Input::CallResult(value) = input else {
                        return Err(Fault::Internal(
                            "DisposableStack constructor expected prototype",
                        ));
                    };
                    value
                } else {
                    *reading = true;
                    match property::read(
                        heap,
                        task,
                        target,
                        &property::Key::Name(JsString::from("prototype")),
                    )? {
                        Some(property::Read::Deferred(callee)) => {
                            return Ok(callee.action(target.clone()));
                        }
                        Some(property::Read::Value(value)) => value,
                        None => Value::Undefined,
                    }
                };
                let prototype = match prototype {
                    Value::Object(id) => id,
                    _ => kind.prototype(heap)?,
                };
                let value = heap
                    .alloc(crate::heap::HeapObject::DisposableStack(Box::new(
                        stack::DisposableStack::new(heap, task, prototype, *kind),
                    )))
                    .map(Value::Object)
                    .ok_or_else(|| Fault::OutOfMemory("heap object table exhausted"))?;
                Ok(Action::Return(value))
            }
        }
    }
}
