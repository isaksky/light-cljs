//! Lazy sequence realization (light-cljs-479; `crate::collections::lazy`).
//!
//! A `__rt.clj*` operation that meets an unrealized lazy sequence records
//! it; its instruction then starts this native frame with its operands
//! instead of completing ([`start`]). The frame claims the sequence's cell
//! and calls its thunk as an ordinary callback in this task, or parks the
//! task while another task realizes it ([`Action::WaitLazy`]), then
//! executes the original instruction again with the same operands
//! ([`Action::Resume`], or [`Action::FormFallback`] if its struct protocol
//! already resolved, so that resolution is not replayed). The claim is given up if the frame is dropped
//! unfinished (a cancellation unwinding through it, or the task's state
//! being dropped), so the cell's other readers are never left waiting.
//!
//! `cljRealizeAll` does not start over when it executes again: the frame
//! keeps its walk ([`clj::RealizeAll`]) and gives it back to the next
//! execution, which continues at the sequence it stopped at, so realizing a
//! macro's result is linear in its size (light-cljs-622).

use std::sync::Arc;

use super::{Action, Input};
use crate::collections::clj;
use crate::collections::form;
use crate::collections::lazy::{Claim, LazyCell, LazyFailure};
use crate::heap::Heap;
use crate::heap::gc::{Trace, TraceEdge};
use crate::ids::TaskId;
use crate::value::{JsString, Value, share};
use crate::vm::dispatch::Fault;
use crate::vm::exception::{Exception, describe_thrown};
use crate::vm::instr::Instr;
use crate::vm::property;

/// The state of one realization. See the module docs.
#[derive(Debug)]
pub(crate) struct Realize {
    /// The instruction to execute again, and its operands.
    instr: Instr,
    operands: Vec<Value>,
    /// This leaf already resolved its struct protocol before realization.
    pub(super) core_resolved: bool,
    /// The task runtime's answers to the instruction's sorted-map-by
    /// comparisons so far, given back when it executes again.
    answers: form::SortedAnswers,
    /// The `cljRealizeAll` walk the instruction stopped at this sequence,
    /// which its next execution continues (light-cljs-622).
    walk: Option<Box<clj::RealizeAll>>,
    /// The lazy form, which keeps the cell alive.
    lazy: Value,
    cell: Arc<LazyCell>,
    /// The attempt this task claimed and has not settled.
    attempt: Option<u64>,
    /// While it has one, the runtime's note that this task realizes the
    /// cell ([`Heap::realizing`]).
    realizing: Option<crate::collections::lazy::Realization>,
    /// The attempt this task parked on, and the task.
    waited: Option<u64>,
    task: Option<TaskId>,
    /// What the thunk threw, delivered by the native catch.
    failed: Option<Exception>,
}

impl Realize {
    /// The state for `instr` (popped `operands`) needing `lazy`; `None` if
    /// `lazy` is not a lazy form.
    pub(crate) fn new(
        heap: &Heap,
        instr: Instr,
        operands: Vec<Value>,
        lazy: Value,
        answers: form::SortedAnswers,
        walk: Option<Box<clj::RealizeAll>>,
    ) -> Option<Realize> {
        let cell = form::form_object(heap, &lazy)?.lazy_cell()?.clone();
        Some(Realize {
            instr,
            operands,
            core_resolved: false,
            answers,
            walk,
            lazy,
            cell,
            attempt: None,
            realizing: None,
            waited: None,
            task: None,
            failed: None,
        })
    }

    /// The cell this frame waits for (for the run guard's subscription).
    pub(crate) fn waiting(&self) -> Option<(Arc<LazyCell>, u64)> {
        self.waited.map(|attempt| (self.cell.clone(), attempt))
    }

    /// The thunk threw (the native catch delivers it).
    pub(crate) fn failed(&mut self, exception: Exception) {
        self.failed = Some(exception);
    }

    /// Whether a native catch must be installed around the thunk call.
    pub(crate) fn catches(&self) -> bool {
        self.attempt.is_some()
    }

    fn resume(&mut self) -> Action {
        Action::Retry {
            instr: self.instr,
            operands: std::mem::take(&mut self.operands),
            answers: std::mem::take(&mut self.answers),
            fallback: self.core_resolved,
            walk: self.walk.take(),
        }
    }

    pub(crate) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
    ) -> Result<Action, Fault> {
        if let Some(exception) = self.failed.take() {
            self.realizing = None;
            if let Some(attempt) = self.attempt.take() {
                let failure = failure(heap, task, &exception.value);
                let _ = self.cell.settle(attempt, Some(Err(failure)), false);
            }
            return Err(Fault::Rethrow(exception));
        }
        let mut produced = match input {
            Input::CallResult(value) => Some(value),
            _ => None,
        };
        // The lazy `concat` cells this step realized through: one met again
        // is a `concat` that contains itself.
        let mut concats: Vec<usize> = Vec::new();
        loop {
            if let Some(value) = produced.take()
                && let Some(action) = self.realized(heap, value)?
            {
                return Ok(action);
            }
            match self.cell.claim(task, self.waited) {
                Claim::Realized => return Ok(self.resume()),
                Claim::Run { thunk, attempt } => {
                    self.attempt = Some(attempt);
                    self.realizing = Some(heap.realizing.begin(task, &self.cell));
                    self.waited = None;
                    if !self.cell.is_concat() {
                        return Ok(Action::Call {
                            callee: thunk,
                            this: Value::Undefined,
                            args: vec![],
                        });
                    }
                    // A lazy `concat`: realized here, without JavaScript, once
                    // its first collection with elements is realized.
                    match clj::concat_step(heap, &thunk) {
                        Ok(value) => produced = Some(value),
                        Err(clj::CljError::Realize(inner)) => {
                            concats.push(Arc::as_ptr(&self.cell) as usize);
                            let again = form::form_object(heap, &inner)
                                .and_then(|f| f.lazy_cell().cloned())
                                .is_some_and(|c| concats.contains(&(Arc::as_ptr(&c) as usize)));
                            if again {
                                return Err(self.fail(Fault::RangeError(
                                    "infinite lazy sequence: a concat contains itself".to_string(),
                                )));
                            }
                            self.abandon();
                            if !self.retarget(heap, inner) {
                                return Err(Fault::Internal(
                                    "concat requested a value that is not lazy",
                                ));
                            }
                        }
                        Err(e) => return Err(self.fail(e.into())),
                    }
                }
                Claim::Busy { attempt } => {
                    self.waited = Some(attempt);
                    self.task = Some(task);
                    return Ok(Action::WaitLazy);
                }
                Claim::Failed(LazyFailure::Thrown(value)) => return Err(Fault::Thrown(value)),
                Claim::Failed(LazyFailure::Message(message)) => {
                    return Err(Fault::Error(message.to_string_lossy()));
                }
                Claim::Failed(LazyFailure::Type(message)) => {
                    return Err(Fault::TypeError(message.to_string_lossy()));
                }
                Claim::Reentered => {
                    return Err(Fault::RangeError(
                        "lazy sequence realization reentered its own thunk".to_string(),
                    ));
                }
            }
        }
    }

    /// Install the value this frame's attempt produced; then the action
    /// that ends the step, or `None` to realize the next cell of the chain.
    fn realized(&mut self, heap: &Heap, value: Value) -> Result<Option<Action>, Fault> {
        {
            let Some(attempt) = self.attempt else {
                return Err(Fault::Internal("lazy realization result without a claim"));
            };
            let record = form::form_object(heap, &value)
                .is_some_and(|f| f.record().is_some())
                .then(|| value.clone());
            let value = match clj::realized_seq(heap, &value) {
                Ok(value) => value,
                Err(e) => {
                    let fault: Fault = e.into();
                    return Err(self.fail(fault));
                }
            };
            // A value the cell cannot hold fails this attempt for every reader
            // parked on it, as a throw would.
            let mut checked = false;
            loop {
                if self.cell.is_shared() && !checked {
                    let check = share::check(heap, &value).and_then(|_| match &record {
                        Some(record) => share::check(heap, record),
                        None => Ok(()),
                    });
                    if let Err(e) = check {
                        return Err(self.fail(Fault::TypeError(format!(
                            "a shared lazy sequence realized a value it cannot share: {e}"
                        ))));
                    }
                    checked = true;
                }
                match self.cell.settle_record(
                    attempt,
                    Some(Ok(value.clone())),
                    checked,
                    record.clone(),
                ) {
                    Ok(()) => break,
                    // Published meanwhile: check, then install.
                    Err(_) => checked = false,
                }
            }
            self.attempt = None;
            self.realizing = None;
            // An operation that reads the whole sequence would meet the
            // next unrealized cell of this chain at once: realize it now
            // instead of reading the realized prefix again for each cell.
            let next = self.next_in_chain(heap, &value);
            if next.is_some_and(|next| self.retarget(heap, next)) {
                return Ok(None);
            }
            Ok(Some(self.resume()))
        }
    }

    /// For an instruction that reads its sequence to the end (`count`,
    /// `cljList`, `cljRealizeAll`), the first unrealized lazy form in the
    /// chain that `value`, just realized, starts: the cell the instruction
    /// would request next. Realizing the chain here keeps such an operation
    /// linear in its length; the cells are realized in the order the
    /// instruction would realize them, as it reads the chain in order.
    fn next_in_chain(&self, heap: &Heap, value: &Value) -> Option<Value> {
        use crate::vm::instr::FormOp;
        if !matches!(
            self.instr,
            Instr::FormOp(FormOp::CljCount | FormOp::CljList | FormOp::CljRealizeAll)
        ) {
            return None;
        }
        let result = if self.instr == Instr::FormOp(FormOp::CljCount) {
            form::sequence_count(heap, value).map(|_| ())
        } else {
            form::sequence_elements(heap, value).map(|_| ())
        };
        match result {
            Err(form::SeqError::Pending(next)) => Some(next),
            _ => None,
        }
    }

    /// Realize the lazy form `next` instead (a cell of the same chain).
    fn retarget(&mut self, heap: &Heap, next: Value) -> bool {
        let Some(cell) = form::form_object(heap, &next).and_then(|f| f.lazy_cell().cloned()) else {
            return false;
        };
        self.lazy = next;
        self.cell = cell;
        self.waited = None;
        true
    }

    /// Fail this frame's attempt with `fault` (raised in this task), so that
    /// the readers parked on it observe the failure rather than run the
    /// thunk again.
    fn fail(&mut self, fault: Fault) -> Fault {
        self.realizing = None;
        if let Some(attempt) = self.attempt.take() {
            let failure = match &fault {
                Fault::TypeError(m) => LazyFailure::Type(JsString::from(m.as_str())),
                Fault::RangeError(m) | Fault::Error(m) => {
                    LazyFailure::Message(JsString::from(m.as_str()))
                }
                _ => LazyFailure::Message(JsString::from("lazy sequence realization failed")),
            };
            let _ = self.cell.settle(attempt, Some(Err(failure)), false);
        }
        fault
    }

    /// Give up an unsettled claim: the cell's waiters wake and one of them
    /// claims it again.
    fn abandon(&mut self) {
        self.realizing = None;
        if let Some(attempt) = self.attempt.take() {
            let _ = self.cell.settle(attempt, None, false);
        }
    }
}

impl Drop for Realize {
    fn drop(&mut self) {
        self.abandon();
        // A reader that parked and will not read again gives up its place
        // and its claim on a failure kept for it.
        if let (Some(task), Some(attempt)) = (self.task, self.waited) {
            self.cell.withdraw(task, attempt);
        }
    }
}

impl Trace for Realize {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        self.lazy.trace(visit);
        for v in &self.operands {
            v.trace(visit);
        }
        self.answers.trace(visit);
        if let Some(walk) = &self.walk {
            walk.trace(visit);
        }
        if let Some(e) = &self.failed {
            e.value.trace(visit);
        }
    }
}

/// What the readers parked on a failed attempt observe: the thrown value
/// when it is shareable, otherwise a message describing it (an `Error`'s
/// own `message`).
fn failure(heap: &Heap, task: TaskId, thrown: &Value) -> LazyFailure {
    if share::check(heap, thrown).is_ok() {
        return LazyFailure::Thrown(thrown.clone());
    }
    let message = match property::read(
        heap,
        task,
        thrown,
        &property::Key::Name(JsString::from("message")),
    ) {
        Ok(Some(property::Read::Value(Value::String(s)))) => s.to_string_lossy(),
        _ => describe_thrown(thrown),
    };
    LazyFailure::Message(JsString::from(message.as_str()))
}

/// Start realizing `lazy` for `instr`, whose `n` operands are on the
/// stack: they move into the new frame, and `instr` executes again with
/// them once `lazy` is realized.
pub(crate) fn start(
    heap: &Heap,
    state: &mut crate::vm::frames::VmState,
    instr: Instr,
    n: usize,
    lazy: Value,
    answers: form::SortedAnswers,
    walk: Option<Box<clj::RealizeAll>>,
) -> Result<(), Fault> {
    let ops = state.operands();
    let at = ops
        .len()
        .checked_sub(n)
        .ok_or(Fault::Internal("operand stack underflow"))?;
    let operands = ops[at..].to_vec();
    let realize = Realize::new(heap, instr, operands, lazy, answers, walk).ok_or(
        Fault::Internal("realization request for a value that is not lazy"),
    )?;
    let base = state.stack_len() - n;
    state.truncate_operands(base)?;
    super::push_native(state, super::NativeState::Realize(Box::new(realize)))
}

#[cfg(test)]
mod tests {
    use crate::collections::clj::realize_all_entered;
    use crate::frontend::compile_function;
    use crate::{RunOutcome, Runtime};

    /// `cljRealizeAll` on a list of `n` syntax-quote-shaped lazy sequences,
    /// `(seq (concat (list 'a) (list i) (list (lazy-seq (list 'b i)))))`,
    /// each holding another: the realization's report (the result's kind
    /// and count, whether it equals the same forms built without laziness,
    /// and the order the thunks ran in) and the values the walk entered.
    fn realize_nested(n: usize) -> (String, usize) {
        let rt = Runtime::new();
        let f = compile_function(&format!(
            "function test() {{
               const order = [];
               const item = (i) => __rt.lazySeq(() => {{
                 order.push(i);
                 const inner = __rt.lazySeq(() => {{ order.push(-i - 1); return __rt.formList('b', i); }});
                 return __rt.cljSeq(__rt.cljConcat(__rt.formList('a'),
                   __rt.cljConcat(__rt.formList(i), __rt.formList(inner))));
               }});
               let xs = null, expected = null;
               for (let i = {n} - 1; i >= 0; i--) {{
                 xs = __rt.cljCons(item(i), xs);
                 expected = __rt.cljCons(__rt.formList('a', i, __rt.formList('b', i)), expected);
               }}
               const r = __rt.cljRealizeAll(xs);
               return [__rt.formKind(r), __rt.cljCount(r), __rt.formEquals(r, expected),
                       order.join(',')].join(';');
             }}"
        ))
        .unwrap();
        let before = realize_all_entered();
        let RunOutcome::Complete(v) = rt.spawn(&f, &[]).unwrap().run().unwrap() else {
            panic!("realization failed");
        };
        let entered = realize_all_entered() - before;
        (v.string().unwrap().to_string_lossy(), entered)
    }

    /// Realizing a result is linear in its size (light-cljs-622): the walk
    /// continues at each pending sequence instead of entering the realized
    /// part of the result again, so twice the sequences take twice the
    /// steps, not four times. The thunks still run once each, in the order
    /// the walk meets them, and the result is the same.
    #[test]
    fn realizing_nested_lazy_sequences_is_linear_in_their_number() {
        let mut steps = Vec::new();
        for n in [50, 100, 200] {
            let (report, entered) = realize_nested(n);
            let order: Vec<String> = (0..n as i64)
                .flat_map(|i| [i.to_string(), (-i - 1).to_string()])
                .collect();
            assert_eq!(report, format!("list;{n};true;{}", order.join(",")));
            steps.push(entered);
        }
        // Eight a sequence (it is entered pending and again realized, its
        // three elements, and its inner sequence's second entry and two
        // elements) and the outer list. Restarting the walk instead entered
        // 15,351, 60,701 and 241,401 values.
        assert_eq!(steps, [401, 801, 1601], "values entered at 50, 100, 200");
    }
}
