//! Shareable regex values (light-cljs-563): the RegExp behaviour of a
//! `regex` compiler-literal form (`crate::collections::form`).
//!
//! A native ClojureScript regex value is one immutable form, shared by every
//! task, that JavaScript also uses as a RegExp: `exec`, `test`, the flag and
//! `source` getters, `Symbol.split`/`replace`/`match`, `instanceof RegExp`
//! and the RegExp constructor's brand check all take it as it is, because
//! [`crate::regexp::object::data`] answers its compiled program and property
//! reads continue at `RegExp.prototype` (`crate::vm::property`). Nothing
//! converts it, so `identical?`, map keys and `this === r` hold whatever
//! code it passes through.
//!
//! # `lastIndex`
//!
//! A shared value has no per-task state: **only a regex without the `g` and
//! `y` flags is a shareable value, and its `lastIndex` is always 0**. Such a
//! regex's matching never moves `lastIndex` (ECMAScript RegExpBuiltinExec
//! reads it and writes it only for global or sticky regexes; `search`,
//! `split`, `matchAll` and `replace` write it only when it is not 0 or on a
//! copy), so a value that cannot change is exactly what legacy code observes.
//! Writing 0 is accepted and changes nothing; writing anything else is a
//! `TypeError`, the documented unsupported case (a legacy RegExp would keep
//! the value for its readers). A global or sticky regex stays an ordinary
//! task-owned RegExp (`formRegex` refuses it), so an atom or a published
//! helper rejects it with the usual publication error, and a helper whose
//! value holds one is initialized per task, as before.
//!
//! The one exception is a `regex` form the bridge makes from a quoted global
//! or sticky literal in macro data (`'#"(?g)a"`), which no task creates: its
//! `lastIndex` is **task-local, starting at 0**, and holds numbers only (any
//! other value is the unsupported `TypeError`), so it matches statefully
//! within a task and never carries state to another. Each task entry is
//! charged to the byte budget and released, with its charge, when the task
//! terminates (completes, fails, is cancelled or is dropped); a denied write
//! fails with the heap limit and keeps the previous value. The heap's
//! registration of an entry, charged too, is also removed by the first
//! collection after the cell dies, so a task that keeps running does not
//! accumulate them.
//!
//! The first use compiles the program, one task at a time: concurrent first
//! users wait for it and share it. A syntax error is kept; a heap-limit
//! failure is not, so a later use compiles again. The cell is charged to the
//! heap's byte budget.
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use super::object::{CompileError, Compiled, compile_accounted};
use crate::heap::Heap;
use crate::heap::budget::ByteReservation;
use crate::ids::TaskId;
use crate::value::{JsString, Value};
use crate::vm::dispatch::Fault;

/// The RegExp state of one `regex` compiler-literal form.
#[derive(Debug)]
pub(crate) struct RegexCell {
    source: JsString,
    flags: JsString,
    /// A program or a syntax error (module docs).
    compiled: OnceLock<Result<Arc<Compiled>, CompileError>>,
    /// Held while compiling, so concurrent first uses compile once.
    compiling: Mutex<()>,
    /// Global or sticky (only a quoted literal's form): task-local
    /// `lastIndex` values, with their byte charges (module docs).
    stateful: bool,
    tasks: Mutex<HashMap<TaskId, (f64, ByteReservation)>>,
    _charge: ByteReservation,
}

/// The bytes a task entry is charged: its key, value and table overhead.
const ENTRY_BYTES: usize = 2 * std::mem::size_of::<(TaskId, (f64, ByteReservation))>();

/// The bytes the heap's registration of a task entry is charged: its slot,
/// with growth overhead, and the cell allocation its `Weak` keeps after
/// the cell dies, until a collection prunes it or the task terminates.
pub(crate) const REGISTRATION_BYTES: usize =
    2 * std::mem::size_of::<(std::sync::Weak<RegexCell>, ByteReservation)>()
        + std::mem::size_of::<RegexCell>()
        + 2 * std::mem::size_of::<usize>();

/// The flag letters a `(?flags)` payload prefix may spell, as the runtimes'
/// `re-pattern` reads them.
const PREFIX_FLAGS: &[u16] = &[
    b'd' as u16,
    b'g' as u16,
    b'i' as u16,
    b'm' as u16,
    b's' as u16,
    b'u' as u16,
    b'x' as u16,
    b'y' as u16,
];

impl RegexCell {
    /// The cell of a form with the exact `source` and `flags`, already
    /// compiled when the caller validated them; `None` when the heap's byte
    /// budget denies it.
    pub(crate) fn new(
        heap: &Heap,
        source: JsString,
        flags: JsString,
        compiled: Option<Arc<Compiled>>,
    ) -> Option<Self> {
        let Some(charge) = heap.reserve_bytes(std::mem::size_of::<RegexCell>()) else {
            heap.byte_budget().note_denial();
            return None;
        };
        let cell = OnceLock::new();
        if let Some(compiled) = compiled {
            let _ = cell.set(Ok(compiled));
        }
        let stateful = flags
            .code_units()
            .iter()
            .any(|&c| c == u16::from(b'g') || c == u16::from(b'y'));
        Some(RegexCell {
            source,
            flags,
            compiled: cell,
            compiling: Mutex::new(()),
            stateful,
            tasks: Mutex::new(HashMap::new()),
            _charge: charge,
        })
    }

    /// The cell of a payload in the legacy export text, `(?flags)source`;
    /// a pattern that does not compile fails when first used.
    pub(crate) fn from_payload(heap: &Heap, text: &JsString) -> Option<Self> {
        let units = text.code_units();
        let prefix =
            (units.len() >= 3 && units[0] == u16::from(b'(') && units[1] == u16::from(b'?'))
                .then(|| {
                    let end = units[2..].iter().position(|c| !PREFIX_FLAGS.contains(c))? + 2;
                    (units[end] == u16::from(b')')).then_some(end)
                })
                .flatten();
        let (source, flags) = match prefix {
            Some(end) => (
                JsString::copy_units(heap, &units[end + 1..])?,
                JsString::copy_units(heap, &units[2..end])?,
            ),
            None => (text.clone(), JsString::fixed("")),
        };
        Self::new(heap, source, flags, None)
    }

    /// The compiled program, compiling it on first use (module docs).
    pub(crate) fn compiled(&self, heap: &Heap) -> Result<Arc<Compiled>, Fault> {
        if let Some(result) = self.compiled.get() {
            return result.clone().map_err(|e| e.fault());
        }
        let _compiling = self
            .compiling
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(result) = self.compiled.get() {
            return result.clone().map_err(|e| e.fault());
        }
        match compile_accounted(heap, &self.source, &self.flags) {
            Err(CompileError::Limit) => Err(CompileError::Limit.fault()),
            result => {
                let _ = self.compiled.set(result.clone());
                result.map_err(|e| e.fault())
            }
        }
    }

    /// `lastIndex` as `task` sees it: always 0, or the task's value for a
    /// quoted global or sticky literal (module docs).
    pub(crate) fn last_index(&self, task: TaskId) -> Value {
        if !self.stateful {
            return Value::Number(0.0);
        }
        let tasks = self.tasks.lock().unwrap_or_else(PoisonError::into_inner);
        Value::Number(tasks.get(&task).map_or(0.0, |(n, _)| *n))
    }

    /// `lastIndex = value` for `task` (module docs).
    pub(crate) fn set_last_index(
        self: &Arc<Self>,
        heap: &Heap,
        task: TaskId,
        value: &Value,
    ) -> Result<(), Fault> {
        let unsupported = || {
            Fault::TypeError(
                "unsupported operation: a shared regex value's lastIndex stays 0 \
                 (global and sticky regexes are not shared)"
                    .into(),
            )
        };
        let n = match value {
            Value::Number(n) => *n,
            _ => return Err(unsupported()),
        };
        if !self.stateful {
            return if n == 0.0 { Ok(()) } else { Err(unsupported()) };
        }
        if let Some(entry) = self
            .tasks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_mut(&task)
        {
            entry.0 = n;
            return Ok(());
        }
        // Only `task` adds its own entry, so it is still absent below.
        let (Some(charge), Some(registration)) = (
            heap.reserve_bytes(ENTRY_BYTES),
            heap.reserve_bytes(REGISTRATION_BYTES),
        ) else {
            heap.byte_budget().note_denial();
            return Err(Fault::HeapLimit);
        };
        self.tasks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(task, (n, charge));
        heap.note_regex_task_state(task, Arc::downgrade(self), registration);
        Ok(())
    }

    /// Drop `task`'s entry and its charge: the task has terminated.
    pub(crate) fn release_task(&self, task: TaskId) {
        self.tasks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&task);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_index_stays_zero() {
        let heap = Heap::default();
        let task = TaskId::from_raw(1);
        let cell = Arc::new(RegexCell::from_payload(&heap, &JsString::from("(?i)o")).unwrap());
        assert!(matches!(cell.last_index(task), Value::Number(n) if n == 0.0));
        assert!(
            cell.set_last_index(&heap, task, &Value::Number(0.0))
                .is_ok()
        );
        assert!(
            cell.set_last_index(&heap, task, &Value::Number(-0.0))
                .is_ok()
        );
        for value in [
            Value::Number(2.0),
            Value::String("0".into()),
            Value::Undefined,
        ] {
            assert!(matches!(
                cell.set_last_index(&heap, task, &value),
                Err(Fault::TypeError(_))
            ));
        }
        assert!(matches!(cell.last_index(task), Value::Number(n) if n == 0.0));
    }

    #[test]
    fn quoted_global_literals_keep_task_local_numbers() {
        let heap = Heap::with_byte_limit(1024 + 4 * ENTRY_BYTES);
        let (a, b) = (TaskId::from_raw(1), TaskId::from_raw(2));
        let cell = Arc::new(RegexCell::from_payload(&heap, &JsString::from("(?g)o")).unwrap());
        assert!(cell.set_last_index(&heap, a, &Value::Number(2.0)).is_ok());
        assert!(matches!(cell.last_index(a), Value::Number(n) if n == 2.0));
        assert!(matches!(cell.last_index(b), Value::Number(n) if n == 0.0));
        assert!(matches!(
            cell.set_last_index(&heap, b, &Value::String("1".into())),
            Err(Fault::TypeError(_))
        ));
        // Entries are charged: the budget admits a bounded number of tasks,
        // and a denied task keeps reading 0.
        let before = heap.byte_budget().used();
        let denied = (3..64)
            .find(|&n| {
                cell.set_last_index(&heap, TaskId::from_raw(n), &Value::Number(1.0))
                    .is_err()
            })
            .expect("bounded");
        assert!(heap.byte_budget().used() > before);
        assert!(matches!(cell.last_index(TaskId::from_raw(denied)), Value::Number(n) if n == 0.0));
    }

    /// A task's registration stays charged after its cell dies, for the
    /// allocation its `Weak` keeps, until pruning removes it
    /// (light-cljs-584).
    #[test]
    fn dead_cell_registrations_stay_charged_until_pruned() {
        let heap = Heap::with_byte_limit(1 << 20);
        let task = TaskId::from_raw(1);
        let start = heap.byte_budget().used();
        let cell = Arc::new(RegexCell::from_payload(&heap, &JsString::from("(?g)o")).unwrap());
        let with_cell = heap.byte_budget().used();
        assert!(
            cell.set_last_index(&heap, task, &Value::Number(1.0))
                .is_ok()
        );
        assert_eq!(
            heap.byte_budget().used(),
            with_cell + ENTRY_BYTES + REGISTRATION_BYTES
        );
        heap.prune_regex_task_state();
        assert_eq!(heap.regex_task_registrations(), 1, "a live cell stays");
        drop(cell);
        assert_eq!(heap.byte_budget().used(), start + REGISTRATION_BYTES);
        assert_eq!(heap.regex_task_registrations(), 1);
        heap.prune_regex_task_state();
        assert_eq!(heap.regex_task_registrations(), 0);
        assert_eq!(heap.byte_budget().used(), start);
    }

    #[test]
    fn payload_prefixes_follow_re_pattern() {
        let heap = Heap::default();
        for (text, source, flags) in [
            ("(?im)a", "a", "im"),
            ("(?:a)", "(?:a)", ""),
            ("(?)a", "a", ""),
            ("(?v)a", "(?v)a", ""),
            ("(?i", "(?i", ""),
            ("", "", ""),
        ] {
            let cell = RegexCell::from_payload(&heap, &JsString::from(text)).unwrap();
            assert_eq!(cell.source, JsString::from(source), "{text}");
            assert_eq!(cell.flags, JsString::from(flags), "{text}");
        }
    }

    #[test]
    fn compilation_retries_after_heap_limit_recovery() {
        let heap = Heap::with_byte_limit(64 * 1024);
        let cell = RegexCell::new(&heap, JsString::from("a+b"), JsString::from(""), None).unwrap();
        let headroom = heap.byte_budget().limit().unwrap() - heap.byte_budget().used();
        let pressure = heap.reserve_bytes(headroom - 64).unwrap();
        assert!(matches!(cell.compiled(&heap), Err(Fault::RangeError(_))));
        drop(pressure);
        heap.byte_budget().take_denial();
        assert!(cell.compiled(&heap).is_ok());
        // A syntax error is final.
        let bad = RegexCell::new(&heap, JsString::from("("), JsString::from(""), None).unwrap();
        assert!(matches!(bad.compiled(&heap), Err(Fault::SyntaxError(_))));
        assert!(matches!(bad.compiled(&heap), Err(Fault::SyntaxError(_))));
    }

    #[test]
    fn concurrent_first_use_compiles_once_with_one_program_budget() {
        // What one compiled program keeps, measured on a roomy heap.
        let roomy = Heap::with_byte_limit(1 << 20);
        let probe =
            RegexCell::new(&roomy, JsString::from("a+b"), JsString::from(""), None).unwrap();
        let before = roomy.byte_budget().used();
        let program = probe.compiled(&roomy).map_err(|_| "limit").unwrap();
        let kept = roomy.byte_budget().used() - before;
        drop(program);
        // Headroom for one compilation but not two at once.
        let reservation = (3 + 1) * 1024;
        let heap = Heap::with_byte_limit(kept.max(reservation) + reservation / 2);
        let cell = RegexCell::new(&heap, JsString::from("a+b"), JsString::from(""), None).unwrap();
        let start = std::sync::Barrier::new(4);
        let programs: Vec<_> = std::thread::scope(|scope| {
            let threads: Vec<_> = (0..4)
                .map(|_| {
                    scope.spawn(|| {
                        start.wait();
                        cell.compiled(&heap).map_err(|_| "limit")
                    })
                })
                .collect();
            threads.into_iter().map(|t| t.join().unwrap()).collect()
        });
        let programs: Vec<_> = programs.into_iter().map(Result::unwrap).collect();
        assert!(programs.windows(2).all(|p| Arc::ptr_eq(&p[0], &p[1])));
    }
}
