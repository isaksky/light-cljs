//! Opt-in task-local QuickJS test/host facilities. No wall clock or sleeps.
use crate::heap::gc::Trace;
use crate::heap::{Heap, HeapObject};
use crate::job::Job;
use crate::object::{OrdinaryObject, PropertyFlags};
use crate::value::{JsString, Value};
use crate::vm::builtin::{Builtin, BuiltinFunction};
use crate::vm::dispatch::{Fault, callee};
use crate::vm::frames::VmState;

/// Failure to install the optional test bindings on a runnable task.
#[derive(Debug)]
pub enum BindingError {
    Run(crate::RunError),
    OutOfMemory,
}
impl std::fmt::Display for BindingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Run(error) => error.fmt(f),
            Self::OutOfMemory => f.write_str("could not allocate simulated host bindings"),
        }
    }
}
impl std::error::Error for BindingError {}

#[derive(Debug, Default)]
pub(crate) struct TaskHost {
    now: u64,
    sequence: u64,
    timers: Vec<Timer>,
}
#[derive(Debug)]
struct Timer {
    due: u64,
    sequence: u64,
    callback: Value,
}
impl Trace for TaskHost {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        self.timers
            .iter()
            .for_each(|timer| timer.callback.trace(visit));
    }
}
impl TaskHost {
    /// Numeric delays are finite, truncated milliseconds. Negative delays
    /// become zero. Refuse overflow rather than wrap virtual time or IDs.
    pub(crate) fn timer(
        &mut self,
        callback: Value,
        delay: f64,
        clear: bool,
    ) -> Result<Value, Fault> {
        if clear {
            self.timers.retain(|timer| timer.sequence as f64 != delay);
            return Ok(Value::Undefined);
        }
        const MAX_SAFE: u64 = (1 << 53) - 1;
        if !delay.is_finite() || delay > MAX_SAFE as f64 {
            return Err(Fault::RangeError(
                "timer delay must be finite safe milliseconds".into(),
            ));
        }
        let delay = delay.max(0.0).trunc() as u64;
        let due = self
            .now
            .checked_add(delay)
            .filter(|v| *v <= MAX_SAFE)
            .ok_or_else(|| Fault::RangeError("virtual timer time overflow".into()))?;
        let sequence = self
            .sequence
            .checked_add(1)
            .filter(|v| *v <= MAX_SAFE)
            .ok_or_else(|| Fault::RangeError("virtual timer ID overflow".into()))?;
        self.sequence = sequence;
        self.timers.push(Timer {
            due,
            sequence,
            callback,
        });
        Ok(Value::Number(sequence as f64))
    }

    pub(crate) fn pending(&self) -> bool {
        !self.timers.is_empty()
    }

    pub(crate) fn next(&mut self) -> Option<Job> {
        let (index, _) = self
            .timers
            .iter()
            .enumerate()
            .min_by_key(|(_, timer)| (timer.due, timer.sequence))?;
        let timer = self.timers.swap_remove(index);
        self.now = timer.due;
        Some(Job {
            callable: timer.callback,
            args: vec![],
        })
    }
}

/// Called only under a run guard, with no safepoint until all new values
/// are attached to the task's traced global object.
pub(crate) fn install(heap: &Heap, state: &mut VmState) -> Option<()> {
    if state.sim_host.is_some() {
        return Some(());
    }
    let intrinsics = crate::realm::intrinsics(heap)?;
    let global = match state.global_object {
        Some(id) => id,
        None => crate::realm::new_global_object(heap, state.diag.task)?,
    };
    let mut bindings = Vec::new();
    for (name, methods) in [
        ("std", &[Builtin::SimGc][..]),
        (
            "os",
            &[Builtin::SimSetTimeout, Builtin::SimClearTimeout][..],
        ),
    ] {
        let mut namespace =
            OrdinaryObject::with_prototype(state.diag.task, intrinsics.object_prototype);
        for &which in methods {
            // Builtin objects have immutable property storage, matching the
            // engine's existing native-function representation.
            let mut properties = OrdinaryObject::intrinsic(Some(intrinsics.function_prototype));
            for (key, value) in [
                ("name", Value::String(JsString::from(which.name()))),
                ("length", Value::Number(f64::from(which.length()))),
            ] {
                properties.define_with(
                    heap.keys().intern(&JsString::from(key))?,
                    value,
                    PropertyFlags::FUNCTION_INFO,
                );
            }
            let function =
                heap.alloc(HeapObject::Builtin(BuiltinFunction { which, properties }))?;
            namespace.define_with(
                heap.keys().intern(&JsString::from(which.name()))?,
                Value::Object(function),
                PropertyFlags::METHOD,
            );
        }
        let namespace = heap.alloc(HeapObject::Plain(namespace))?;
        bindings.push((heap.keys().intern(&JsString::from(name))?, namespace));
    }
    let installed = heap.with_object_mut(global, |object| {
        let HeapObject::Plain(object) = object else {
            return false;
        };
        bindings.into_iter().all(|(key, value)| {
            object.define_with(key, Value::Object(value), PropertyFlags::METHOD)
        })
    })?;
    if !installed {
        return None;
    }
    state.global_object = Some(global);
    state.sim_host = Some(TaskHost::default());
    Some(())
}

pub(crate) fn validate_callback(heap: &Heap, callback: &Value) -> Result<(), Fault> {
    callee(heap, callback)
        .map(|_| ())
        .map_err(|_| Fault::TypeError("setTimeout callback must be callable".into()))
}
