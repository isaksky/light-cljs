//! AddDisposableResource (sync). Getter calls use ordinary VM frames;
//! source and progress remain traced across suspension and collection.
use super::*;

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
