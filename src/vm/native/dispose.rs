//! AddDisposableResource (sync). Getter calls use ordinary VM frames;
//! source and progress remain traced across suspension and collection.
use super::*;
use crate::vm::builtin::Builtin;

#[derive(Debug)]
pub(super) struct State {
    source: Value,
    reading: bool,
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
        }
    }

    fn checked(heap: &Heap, method: Value) -> Result<Action, Fault> {
        if matches!(method, Value::Undefined | Value::Null) {
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
        Ok(Action::Return(method))
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
            return Self::checked(heap, method);
        }
        if matches!(self.source, Value::Null | Value::Undefined) {
            return Ok(Action::Return(Value::Undefined));
        }
        if !matches!(self.source, Value::Object(_)) {
            return Err(Fault::TypeError("value is not disposable".into()));
        }
        let key = property::Key::Symbol(crate::value::symbol::well_known("dispose"));
        self.reading = true;
        match property::read(heap, task, &self.source, &key)? {
            Some(property::Read::Getter(callee)) => Ok(Action::Call {
                callee,
                this: self.source.clone(),
                args: vec![],
            }),
            Some(property::Read::Value(method)) => Self::checked(heap, method),
            None => Self::checked(heap, Value::Undefined),
        }
    }
}

use crate::object::disposable_stack::{self as stack, Hint as ResourceHint, Resource};

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
    stack: JsString,
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
            stack,
        }
    }
    pub(super) fn catches(&self) -> bool {
        self.calling
    }
    pub(super) fn failed(&mut self, exception: super::super::exception::Exception) {
        self.failure = Some(exception.value);
        self.calling = false;
    }
    pub(super) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        _input: Input,
    ) -> Result<Action, Fault> {
        self.calling = false;
        if let Some(new_error) = self.failure.take() {
            self.error = Some(match self.error.take() {
                Some(previous) => {
                    error::suppressed_error(heap, task, new_error, previous, self.stack.clone())?
                }
                None => new_error,
            });
            self.changed = true;
        }
        if let Some(resource) = self.resources.pop() {
            if matches!(resource.method, Value::Undefined) {
                return Ok(Action::Continue);
            }
            let (this, args) = match resource.hint {
                ResourceHint::Sync => (resource.value, vec![]),
                ResourceHint::Adopt => (Value::Undefined, vec![resource.value]),
                ResourceHint::Defer => (Value::Undefined, vec![]),
            };
            self.calling = true;
            return Ok(Action::Call {
                callee: resource.method,
                this,
                args,
            });
        }
        if self.changed && !self.cancelling {
            return Err(Fault::Thrown(
                self.error
                    .take()
                    .ok_or(Fault::Internal("missing disposal error"))?,
            ));
        }
        Ok(Action::Return(Value::Undefined))
    }
}

#[derive(Debug)]
pub(super) enum Stack {
    Construct { target: Value, reading: bool },
    Use { this: Value, check: State },
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
            Builtin::DisposableStackUse => {
                stack::access(heap, task, this, true, |_| ())?;
                Ok(Self::Use {
                    this: this.clone(),
                    check: State::new(args.first().cloned().unwrap_or(Value::Undefined)),
                })
            }
            Builtin::DisposableStackDispose => {
                let resources = stack::access(heap, task, this, false, |s| {
                    if s.disposed {
                        vec![]
                    } else {
                        s.disposed = true;
                        std::mem::take(&mut s.resources)
                    }
                })?;
                Ok(Self::Run(Run::new(resources, None, false, diagnostic)))
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
                if let Action::Return(method) = action {
                    // AddDisposableResource checks the receiver before invoking
                    // the getter. Re-entrant disposal in that getter does not
                    // repeat the disposed check (ECMAScript use steps 2-6).
                    if !matches!(method, Value::Undefined) {
                        stack::access(heap, task, this, false, |s| {
                            s.resources.push(Resource {
                                value: check.source.clone(),
                                method,
                                hint: ResourceHint::Sync,
                            })
                        })?;
                    }
                    Ok(Action::Return(check.source.clone()))
                } else {
                    Ok(action)
                }
            }
            Self::Construct { target, reading } => {
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
                        Some(property::Read::Getter(callee)) => {
                            return Ok(Action::Call {
                                callee,
                                this: target.clone(),
                                args: vec![],
                            });
                        }
                        Some(property::Read::Value(value)) => value,
                        None => Value::Undefined,
                    }
                };
                let prototype = match prototype {
                    Value::Object(id) => id,
                    _ => {
                        crate::realm::intrinsics(heap)
                            .ok_or(Fault::OutOfMemory("heap object table exhausted"))?
                            .disposable_stack_prototype
                    }
                };
                let value = heap
                    .alloc(crate::heap::HeapObject::DisposableStack(Box::new(
                        stack::DisposableStack::new(task, prototype),
                    )))
                    .map(Value::Object)
                    .ok_or(Fault::OutOfMemory("heap object table exhausted"))?;
                Ok(Action::Return(value))
            }
        }
    }
}
