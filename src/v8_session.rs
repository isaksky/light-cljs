//! The V8 heap of an interactive session (`EvalSession`, `--eval`, `--repl`):
//! host functions, an input provider, promise jobs and rejection tracking on
//! top of the macro evaluator's isolate, with QuickJS's interactive contract.
use super::{Active, State, V8Evaluator, bridge, invoke};
use crate::{
    compiler_data::{Datum, DatumKind},
    eval_session::{HostCallback, Input},
    evaluator::{
        EvalError, EvaluatorOptions, ExecutionControl, HostArity, HostCall, HostScope,
        MacroEvaluator, SessionEvaluator, ValueId,
    },
    form_bridge::ConversionLimits,
};
use std::{cell::RefCell, rc::Rc};
use v8::{Local, PinScope};

/// One installed host function.
#[derive(Clone)]
pub(super) struct HostEntry {
    name: String,
    arity: HostArity,
    callback: HostCallback,
}

/// `queueMicrotask` for the session's heap: bare V8 has none, and a job's
/// exception must reach `run_jobs` as QuickJS reports it, not be swallowed by
/// the microtask checkpoint.
const QUEUE_MICROTASK: &str = r#"rt => {
  const services = __lightEvaluator, resolved = Promise.resolve(), then = Promise.prototype.then;
  const queueMicrotask = function queueMicrotask(callback) {
    if (typeof callback !== 'function') throw new TypeError('queueMicrotask requires a function');
    Reflect.apply(then, resolved, [() => { try { callback(); } catch (error) { services.jobError(error); } }]);
  };
  Object.defineProperty(globalThis, 'queueMicrotask', {value: queueMicrotask, writable: true, configurable: true, enumerable: false});
  return null;
}"#;

impl V8Evaluator {
    /// A heap for an interactive session: language exceptions leave it
    /// usable, promise rejections with no handler are tracked, and the
    /// session's jobs run in [`SessionEvaluator::run_jobs`].
    pub(crate) fn interactive(options: EvaluatorOptions) -> Result<Self, EvalError> {
        let evaluator = Self::new(options)?;
        evaluator.state.interactive.set(true);
        evaluator
            .isolate
            .borrow_mut()
            .set_promise_reject_callback(promise_rejected);
        evaluator
            .isolate
            .borrow_mut()
            .set_promise_hook(promise_hook);
        let done = MacroEvaluator::evaluate_unit(&evaluator, QUEUE_MICROTASK)?;
        MacroEvaluator::release(&evaluator, done)?;
        Ok(evaluator)
    }
}

/// An argument of a `__lightEvaluator` service.
enum Arg<'x> {
    Number(f64),
    Text(&'x str),
    Bool(bool),
}
impl Active<'_, '_, '_> {
    /// Call the service `name` with `args` and read its result with `read`.
    fn service<T>(
        &self,
        name: &str,
        args: &[Arg<'_>],
        read: impl for<'s, 'i> FnOnce(
            &mut PinScope<'s, 'i>,
            Local<'s, v8::Value>,
        ) -> Result<T, EvalError>,
    ) -> Result<T, EvalError> {
        self.enter(|scope| {
            let mut values = Vec::with_capacity(args.len());
            for arg in args {
                values.push(match arg {
                    Arg::Number(n) => v8::Number::new(scope, *n).into(),
                    Arg::Text(text) => bridge::string(scope, text)?.into(),
                    Arg::Bool(b) => v8::Boolean::new(scope, *b).into(),
                });
            }
            let value = invoke(scope, self.state, name, &values)?;
            read(scope, value)
        })
    }
    fn id(&self, name: &str, args: &[Arg<'_>]) -> Result<ValueId, EvalError> {
        let index = self.service(name, args, |scope, value| bridge::number(scope, value))?;
        Ok(self.value(index as u64))
    }
    fn of(&self, id: ValueId) -> Result<Arg<'static>, EvalError> {
        Ok(Arg::Number(self.check(id)? as f64))
    }
    fn number(&self, id: ValueId) -> Result<f64, EvalError> {
        self.service("number", &[self.of(id)?], |scope, value| {
            bridge::number(scope, value)
        })
    }
    fn string(&self, id: ValueId) -> Result<String, EvalError> {
        // Rust text has no lone surrogates: reject them, as QuickJS does,
        // rather than replace them.
        self.service("string", &[self.of(id)?], |scope, value| {
            let text: Local<v8::String> = value
                .try_into()
                .map_err(|_| bridge::error("Expected string"))?;
            let mut units = vec![0; text.length()];
            text.write_v2(scope, 0, &mut units, v8::WriteFlags::empty());
            String::from_utf16(&units)
                .map_err(|_| bridge::error("string is not valid UTF-16: it has a lone surrogate"))
        })
    }
    fn print(&self, id: ValueId) -> Result<String, EvalError> {
        self.service("print", &[self.of(id)?], |scope, value| {
            Ok(value.to_rust_string_lossy(scope))
        })
    }
    fn export_data(&self, id: ValueId, limits: ConversionLimits) -> Result<Datum, EvalError> {
        let id = self.check(id)?;
        self.enter(|scope| {
            let args = [
                v8::Number::new(scope, id as f64).into(),
                v8::Number::new(scope, limits.nodes as f64).into(),
                v8::Number::new(scope, limits.depth as f64).into(),
            ];
            let descriptor = invoke(scope, self.state, "exportForm", &args)?;
            self.state.bridge.borrow_mut().decode(
                scope,
                bridge::array(descriptor)?,
                &self.state.control,
                limits,
                false,
            )
        })
    }
}

impl SessionEvaluator for V8Evaluator {
    fn control(&self) -> ExecutionControl {
        self.state.control.clone()
    }
    /// Begin an operation, first emptying V8's compilation caches when the
    /// heap holds more than half the limit. A session evaluates a new source
    /// per form, and V8 keeps what `eval` compiles until a low-memory
    /// collection: 6,000 REPL forms reached the 64 MB limit with 2.6 MB live.
    fn begin_operation(&self, options: &EvaluatorOptions) -> Result<(), EvalError> {
        self.state.control.begin(options)?;
        let mut isolate = self.isolate.borrow_mut();
        let entered = super::Entered::new(&mut isolate);
        let used = entered.0.get_heap_statistics().used_heap_size();
        if used.saturating_add(self.state.buffers.used()) > self.memory.maximum / 2 {
            entered.0.low_memory_notification();
        }
        Ok(())
    }
    fn evaluate_unit(&self, source: &str) -> Result<ValueId, EvalError> {
        MacroEvaluator::evaluate_unit(self, source)
    }
    fn call(&self, function: ValueId, arguments: &[ValueId]) -> Result<ValueId, EvalError> {
        MacroEvaluator::call(self, function, arguments)
    }
    fn number(&self, id: ValueId) -> Result<f64, EvalError> {
        self.with_active(|e| e.number(id))
    }
    fn string(&self, id: ValueId) -> Result<String, EvalError> {
        self.with_active(|e| e.string(id))
    }
    fn print(&self, id: ValueId) -> Result<String, EvalError> {
        self.with_active(|e| e.print(id))
    }
    fn release(&self, id: ValueId) -> Result<(), EvalError> {
        MacroEvaluator::release(self, id)
    }
    fn import_data(&self, data: &Datum, limits: ConversionLimits) -> Result<ValueId, EvalError> {
        MacroEvaluator::import_data(self, data, limits)
    }
    fn export_data(&self, id: ValueId, limits: ConversionLimits) -> Result<Datum, EvalError> {
        self.with_active(|e| e.export_data(id, limits))
    }
    fn validate(&self, id: ValueId) -> Result<(), EvalError> {
        self.with_active(|e| e.service("validate", &[e.of(id)?], |_, _| Ok(())))
    }
    fn bind(&self, name: &str, id: ValueId, core: bool) -> Result<(), EvalError> {
        let service = if core { "coreBinding" } else { "bind" };
        self.with_active(|e| e.service(service, &[Arg::Text(name), e.of(id)?], |_, _| Ok(())))
    }
    fn lookup(&self, name: &str, core: bool) -> Result<ValueId, EvalError> {
        let service = if core { "coreLookup" } else { "lookup" };
        self.with_active(|e| e.id(service, &[Arg::Text(name)]))
    }
    fn error_data(&self) -> Option<Datum> {
        self.with_active(|e| {
            let id = e.id("errorData", &[])?;
            let result = e.export_data(id, ConversionLimits::default());
            let _ = MacroEvaluator::release(e, id);
            result
        })
        .ok()
        .filter(|data| !matches!(data.kind(), DatumKind::Nil))
    }
    fn last_exception(&self) -> Result<ValueId, EvalError> {
        self.with_active(|e| e.id("lastError", &[]))
    }
    /// Run the microtask queue to its end under the operation's limits: the
    /// watchdog's interrupts end a job that loops or an endless chain of them.
    /// A `.then` reaction's throw rejects its promise; an exception that
    /// leaves a `queueMicrotask` callback is recorded by its wrapper.
    fn run_jobs(&self, keep: bool) -> Result<(), EvalError> {
        self.with_active(|e| {
            e.service("holdError", &[], |_, _| Ok(()))?;
            e.enter(|scope| {
                scope.perform_microtask_checkpoint();
                Ok(())
            })?;
            e.service("releaseError", &[Arg::Bool(keep)], |_, _| Ok(()))
        })
    }
    fn unhandled_rejections(&self, limit: u32, raise: bool) -> Result<Vec<String>, EvalError> {
        if !self.state.rejected.replace(false) {
            return Ok(vec![]);
        }
        self.with_active(|e| {
            e.service(
                "takeRejections",
                &[Arg::Number(f64::from(limit)), Arg::Bool(raise)],
                |scope, value| {
                    let reasons = bridge::array(value)?;
                    (0..reasons.length())
                        .map(|i| Ok(bridge::get(scope, reasons, i)?.to_rust_string_lossy(scope)))
                        .collect()
                },
            )
        })
    }
    fn input_provider(&self, name: &str, provider: Input) -> Result<ValueId, EvalError> {
        let index = self.state.input_providers.borrow().len();
        self.state.input_providers.borrow_mut().push(provider);
        self.with_active(|e| {
            let index = e.enter(|scope| {
                let function = native(scope, input_chunk, index)?;
                let name = bridge::string(scope, name)?;
                let id = invoke(
                    scope,
                    e.state,
                    "inputProvider",
                    &[name.into(), function.into()],
                )?;
                bridge::number(scope, id)
            })?;
            Ok(e.value(index as u64))
        })
    }
    fn input_window(&self, more: bool) -> Result<(String, usize, bool, usize), EvalError> {
        self.with_active(|e| {
            e.service("inputWindow", &[Arg::Bool(more)], |scope, value| {
                let window = bridge::array(value)?;
                let text = bridge::get(scope, window, 0)?.to_rust_string_lossy(scope);
                let units = bridge::get(scope, window, 1)?;
                let units = bridge::number(scope, units)? as usize;
                let eof = bridge::get(scope, window, 2)?.is_true();
                let line = bridge::get(scope, window, 3)?;
                let line = bridge::number(scope, line)? as usize;
                Ok((text, units, eof, line))
            })
        })
    }
    fn input_action(&self, action: &str, units: usize) -> Result<(), EvalError> {
        self.with_active(|e| e.service(action, &[Arg::Number(units as f64)], |_, _| Ok(())))
    }
    fn install_host_function(
        &self,
        name: &str,
        arity: HostArity,
        callback: HostCallback,
    ) -> Result<ValueId, EvalError> {
        let index = self.state.host_functions.borrow().len();
        self.state.host_functions.borrow_mut().push(HostEntry {
            name: name.to_owned(),
            arity,
            callback,
        });
        self.with_active(|e| {
            let index = e.enter(|scope| {
                let callback = native(scope, host_function, index)?;
                let function = invoke(scope, e.state, "hostFunction", &[callback.into()])?;
                let id = invoke(scope, e.state, "save", &[function])?;
                bridge::number(scope, id)
            })?;
            Ok(e.value(index as u64))
        })
    }
    fn memory_usage_bytes(&self) -> i64 {
        let used = self
            .isolate
            .borrow_mut()
            .get_heap_statistics()
            .used_heap_size();
        i64::try_from(used.saturating_add(self.state.buffers.used())).unwrap_or(i64::MAX)
    }
}

/// A native function whose data is `index`.
fn native<'s>(
    scope: &mut PinScope<'s, '_>,
    callback: impl v8::MapFnTo<v8::FunctionCallback>,
    index: usize,
) -> Result<Local<'s, v8::Function>, EvalError> {
    let data = v8::Number::new(scope, index as f64);
    v8::Function::builder(callback)
        .data(data.into())
        .build(scope)
        .ok_or_else(|| bridge::error("V8 callback allocation failed"))
}

/// Throw an `Error` with `message`, or end the operation when even that
/// cannot be allocated.
fn throw(scope: &mut PinScope<'_, '_>, message: &str) {
    match v8::String::new(scope, message) {
        Some(message) => {
            let error = v8::Exception::error(scope, message);
            scope.throw_exception(error);
        }
        None => {
            scope.terminate_execution();
        }
    }
}

/// The session state and the index in the function's data.
fn entry(scope: &mut PinScope<'_, '_>, data: Local<'_, v8::Value>) -> Option<(Rc<State>, usize)> {
    let state = scope.get_slot::<Rc<State>>().cloned()?;
    let index = bridge::number(scope, data).ok()?;
    Some((state, index as usize))
}

/// A host callback's view of the active isolate.
struct V8Host<'a, 'b, 's, 'i> {
    active: &'a Active<'b, 's, 'i>,
    unit_exception: RefCell<Option<v8::Global<v8::Value>>>,
}
impl HostScope for V8Host<'_, '_, '_, '_> {
    fn control(&self) -> ExecutionControl {
        self.active.state.control.clone()
    }
    fn check(&self, id: ValueId) -> Result<u64, EvalError> {
        self.active.check(id)
    }
    fn number(&self, value: ValueId) -> Result<f64, EvalError> {
        self.active.number(value)
    }
    fn string(&self, value: ValueId) -> Result<String, EvalError> {
        self.active.string(value)
    }
    fn print(&self, value: ValueId) -> Result<String, EvalError> {
        self.active.print(value)
    }
    fn data(&self, value: ValueId) -> Result<Datum, EvalError> {
        self.active.export_data(value, ConversionLimits::default())
    }
    fn value(&self, data: &Datum) -> Result<ValueId, EvalError> {
        self.active.import_data(data, ConversionLimits::default())
    }
    fn invoke(&self, function: ValueId, args: &[ValueId]) -> Result<ValueId, EvalError> {
        self.active.call(function, args)
    }
    fn release(&self, value: ValueId) -> Result<(), EvalError> {
        MacroEvaluator::release(self.active, value)
    }
    fn unit(&self, source: &str) -> Result<ValueId, EvalError> {
        self.unit_exception.borrow_mut().take();
        let result = self.active.evaluate_unit(source);
        if result.is_err()
            && let Ok(exception) = self.active.enter(|scope| {
                let value = invoke(scope, self.active.state, "errorValue", &[])?;
                Ok(v8::Global::new(scope, value))
            })
        {
            *self.unit_exception.borrow_mut() = Some(exception);
        }
        result
    }
}

/// An installed host function: `hostFunction` calls it with the array of its
/// arguments' IDs, and it answers the IDs of its result and of the values the
/// callback still owns, or throws.
fn host_function<'s>(
    scope: &mut PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    mut result: v8::ReturnValue<'s>,
) {
    let Some((state, index)) = entry(scope, args.data()) else {
        scope.terminate_execution();
        return;
    };
    let Some(HostEntry {
        name,
        arity,
        callback,
    }) = state.host_functions.borrow().get(index).cloned()
    else {
        scope.terminate_execution();
        return;
    };
    let ids = (|| {
        let ids = bridge::array(args.get(0))?;
        (0..ids.length())
            .map(|i| {
                let id = bridge::get(scope, ids, i)?;
                Ok(bridge::number(scope, id)? as u64)
            })
            .collect::<Result<Vec<_>, EvalError>>()
    })();
    let ids = match ids {
        Ok(ids) => ids,
        Err(error) => return throw(scope, &format!("{name}: {error}")),
    };
    if !arity.accepts(ids.len()) {
        return throw(scope, &format!("{name}: invalid arity {}", ids.len()));
    }
    let outcome = {
        let active = Active {
            scope: RefCell::new(&mut *scope),
            state: &state,
        };
        let host = V8Host {
            active: &active,
            unit_exception: RefCell::new(None),
        };
        let call = HostCall::new(&host);
        let args = ids.iter().map(|id| active.value(*id)).collect::<Vec<_>>();
        let outcome =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(&call, &args)));
        let outcome = match outcome {
            Ok(outcome) => outcome.and_then(|id| host.check(id)),
            Err(_) => {
                state.control.invalidate();
                Err(EvalError(format!("{name}: host callback panicked")))
            }
        }
        .and_then(|index| {
            state.control.poll()?;
            Ok(index)
        });
        let owned = call.into_owned();
        match outcome {
            Ok(index) => Ok(std::iter::once(index)
                .chain(owned.into_iter().map(|id| id.index))
                .collect::<Vec<_>>()),
            Err(error) => {
                for id in owned {
                    let _ = host.release(id);
                }
                Err((error, host.unit_exception.into_inner()))
            }
        }
    };
    match outcome {
        Ok(indices) => {
            let indices = indices
                .into_iter()
                .map(|index| v8::Number::new(scope, index as f64).into())
                .collect::<Vec<_>>();
            let indices = v8::Array::new_with_elements(scope, &indices);
            result.set(indices.into());
        }
        // A limit or cancellation ends the operation; no handler may catch it.
        // Keep the callback's own diagnostic (a panic, say) for the
        // operation's error, as termination carries no exception.
        Err((error, _)) if state.control.check().is_err() => {
            state.remember_error(&EvalError(format!("{name}: {error}")));
            scope.terminate_execution();
        }
        Err((error, Some(exception))) => {
            let exception = Local::new(scope, exception);
            match bridge::string(scope, &format!("{name}: {error}")) {
                // Throws the unit's own exception, which propagates.
                Ok(message) => {
                    let _ = invoke(scope, &state, "rethrowError", &[exception, message.into()]);
                }
                Err(_) => {
                    scope.terminate_execution();
                }
            }
        }
        Err((error, None)) => throw(scope, &format!("{name}: {error}")),
    }
}

/// The session's input provider: the next chunk, or null at the end.
fn input_chunk(
    scope: &mut PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    let Some((state, index)) = entry(scope, args.data()) else {
        scope.terminate_execution();
        return;
    };
    let Some(provider) = state.input_providers.borrow().get(index).cloned() else {
        scope.terminate_execution();
        return;
    };
    if let Err(error) = state.control.poll() {
        return throw(scope, &error.0);
    }
    let chunk = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        provider(state.control.clone())
    }));
    let chunk = match chunk {
        Ok(Ok(chunk)) => chunk,
        Ok(Err(error)) => return throw(scope, &format!("input provider failed: {error}")),
        Err(_) => {
            state.control.invalidate();
            return throw(scope, "input provider panicked");
        }
    };
    if let Err(error) = state.control.poll() {
        return throw(scope, &error.0);
    }
    match chunk {
        Some(chunk) => match bridge::string(scope, &chunk) {
            Ok(chunk) => result.set(chunk.into()),
            Err(error) => throw(scope, &error.0),
        },
        None => result.set_null(),
    }
}

/// V8's report of a promise rejected with no handler, or of a handler
/// attached to one later, for `trackRejection`. A record it cannot make is
/// dropped, as QuickJS's tracker drops one.
extern "C" fn promise_rejected(message: v8::PromiseRejectMessage) {
    let handled = match message.get_event() {
        v8::PromiseRejectEvent::PromiseRejectWithNoHandler => false,
        v8::PromiseRejectEvent::PromiseHandlerAddedAfterReject => true,
        _ => return,
    };
    v8::callback_scope!(unsafe scope, &message);
    let Some(state) = scope.get_slot::<Rc<State>>().cloned() else {
        return;
    };
    // Nothing is recorded since the last take: no promise to drop.
    if handled && !state.rejected.get() {
        return;
    }
    state.rejected.set(true);
    v8::tc_scope!(let scope, scope);
    let promise = message.get_promise().into();
    let reason = message
        .get_value()
        .unwrap_or_else(|| v8::undefined(scope).into());
    let handled = v8::Boolean::new(scope, handled).into();
    let _ = invoke(scope, &state, "trackRejection", &[promise, reason, handled]);
}

/// V8's hook before each promise reaction job (a `.then` callback, the rest
/// of an async function after an `await`): each job costs a poll, as each
/// QuickJS job does, so the poll budget ends an endless chain of them.
extern "C" fn promise_hook(
    kind: v8::PromiseHookType,
    promise: Local<'_, v8::Promise>,
    _: Local<'_, v8::Value>,
) {
    if kind != v8::PromiseHookType::Before {
        return;
    }
    v8::callback_scope!(unsafe scope, promise);
    let Some(state) = scope.get_slot::<Rc<State>>().cloned() else {
        return;
    };
    if state.control.poll().is_err() {
        scope.terminate_execution();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXHAUST: &str = "(() => { try { const a = []; while (true) a.push(new Array(4096).fill(1)); } catch (_) { return 42; } })()";

    fn small() -> EvaluatorOptions {
        EvaluatorOptions {
            memory_limit: 32 * 1024 * 1024,
            ..Default::default()
        }
    }

    #[test]
    fn interactive_heap_exhaustion_is_terminal_even_when_javascript_catches() {
        let evaluator = V8Evaluator::interactive(small()).unwrap();
        let error = SessionEvaluator::evaluate_js(&evaluator, EXHAUST).unwrap_err();
        assert!(error.0.contains("memory limit exhausted"), "{error}");
        let error = SessionEvaluator::evaluate_js(&evaluator, "42").unwrap_err();
        assert!(error.0.contains("memory limit exhausted"), "{error}");
    }

    /// V8 keeps every source `eval` compiles until a low-memory collection;
    /// a long session's operations empty that cache before it fills the heap.
    #[test]
    fn many_distinct_units_stay_within_the_heap_limit() {
        let options = small();
        let evaluator = V8Evaluator::interactive(options.clone()).unwrap();
        let pad = "x".repeat(32 * 1024);
        for i in 0..2_000 {
            SessionEvaluator::begin_operation(&evaluator, &options).unwrap();
            let value =
                SessionEvaluator::evaluate_js(&evaluator, &format!("({i}, '{pad}').length"))
                    .unwrap_or_else(|error| panic!("unit {i}: {error}"));
            SessionEvaluator::release(&evaluator, value).unwrap();
        }
    }

    /// A loop that runs long enough for on-stack replacement, calling
    /// builtins, still ends at the deadline in either kind of heap.
    #[test]
    fn loops_that_call_builtins_stop_at_the_deadline() {
        let spin = "(() => { let a; while (true) a = new Array(4096).fill(1); })()";
        let options = EvaluatorOptions {
            timeout: std::time::Duration::from_millis(500),
            ..Default::default()
        };
        let evaluator = V8Evaluator::interactive(options.clone()).unwrap();
        let error = SessionEvaluator::evaluate_js(&evaluator, spin).unwrap_err();
        assert!(error.0.contains("deadline exceeded"), "{error}");
        let evaluator = V8Evaluator::new(options).unwrap();
        let error = crate::compiler_evaluator::CompilerEvaluator::evaluate_js(&evaluator, spin)
            .unwrap_err();
        assert!(error.0.contains("deadline exceeded"), "{error}");
    }

    /// A language exception is an ordinary error in an interactive heap,
    /// which goes on, and poisons a macro heap.
    #[test]
    fn language_exceptions_leave_only_the_interactive_heap_usable() {
        let throws = "(() => { throw new TypeError('no'); })()";
        let evaluator = V8Evaluator::interactive(EvaluatorOptions::default()).unwrap();
        let error = SessionEvaluator::evaluate_js(&evaluator, throws).unwrap_err();
        assert_eq!(error.0, "TypeError: no");
        let thrown = SessionEvaluator::last_exception(&evaluator).unwrap();
        assert_eq!(
            SessionEvaluator::print(&evaluator, thrown).unwrap(),
            "#object[TypeError TypeError: no]"
        );
        let value = SessionEvaluator::evaluate_js(&evaluator, "40 + 2").unwrap();
        assert_eq!(SessionEvaluator::number(&evaluator, value).unwrap(), 42.0);
        let evaluator = V8Evaluator::new(EvaluatorOptions::default()).unwrap();
        let compiler =
            |source| crate::compiler_evaluator::CompilerEvaluator::evaluate_js(&evaluator, source);
        assert!(compiler(throws).is_err());
        let error = compiler("42").unwrap_err();
        assert!(error.0.contains("invalidated"), "{error}");
    }
}
