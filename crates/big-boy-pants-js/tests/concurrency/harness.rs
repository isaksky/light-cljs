//! The bounded scenario harness of the concurrency suite.
//!
//! A scenario runs the production runtime under the two-worker reference
//! executor (`exec::Executor`) with a recording driver and recording hosts:
//!
//! * **Bounded.** [`scenario`] runs the body on its own thread and waits for
//!   it at most [`Config::scenario`] (default [`SCENARIO`]); every wait inside
//!   the body (on executor events, on harness notes, for a terminal outcome,
//!   for shutdown) is bounded by [`Config::step`] (default [`STEP`]), and a
//!   run blocked at a gate by [`Config::gate`]. A bound that expires fails
//!   the test with the full trace; nothing ever sleeps, and elapsed time
//!   never decides an order, it only turns a hang into a failure.
//! * **Traced.** The driver and hosts append [`Note`]s to one totally
//!   ordered log (run start and end per worker, every leaf effect, gate
//!   blocks, claimed producer requests, Rust-side needs). On any failure
//!   (assertion, hang, invariant) the notes and the executor's event record
//!   are dumped.
//! * **Gates.** Host operation [`GATE`]`(g)` blocks while the test holds gate
//!   `g` ([`Harness::arm`]) and the Rust-need driver can block between its
//!   subscription and its park on a gate too: the test acts inside a known
//!   window, then opens the gate. A gate left closed times out (bounded),
//!   failing the run.
//! * **Invariants checked after every scenario** ([`Harness::finish`]): no
//!   task ran in two drivers at once (checked by the driver itself, not
//!   just from the record), dispatches and releases alternate on one worker
//!   per task, execution numbers increase, nothing is dispatched after a
//!   terminal release, every effect happened inside a run of its task on
//!   that worker, and the executor ends with no queued or parked task
//!   unless the scenario declared it (a lost wakeup leaves one).

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};
use std::thread;
use std::time::{Duration, Instant};

use big_boy_pants_js::exec::{
    CollectionEvent, EnqueueReason, Event, EventKind, ExecutorConfig, OutcomeKind, Placement,
    RunContext, WorkerId,
};
use big_boy_pants_js::frontend::compile_function;
use big_boy_pants_js::{
    DependencyId, DependencyKey, Executable, Executor, Host, HostCall, Need, NeedError, Primitive,
    ProducerRequest, ResultKind, Rooted, RunGuard, RunOutcome, Runtime, Task, TaskId, TaskStatus,
    WakeHandle, Wakeups, WorldId,
};

/// Bound of one wait inside a scenario (an event, a note, a gate, an
/// outcome, shutdown).
pub const STEP: Duration = Duration::from_secs(20);
/// Bound of a whole scenario, including its invariant checks.
pub const SCENARIO: Duration = Duration::from_secs(90);

/// Host operations of the harness hosts. `PRE` and `POST` record and return
/// their argument; `GATE(g)` also blocks while gate `g` is armed.
pub const PRE: u16 = 1;
pub const GATE: u16 = 2;
pub const POST: u16 = 3;
pub const NOTE: u16 = 4;

pub fn w(index: usize) -> WorkerId {
    WorkerId::new(index)
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Compile a synthetic source fixture (not light-cljs output).
pub fn compile(source: &str) -> Executable {
    compile_function(source).unwrap_or_else(|e| panic!("fixture does not compile: {e:?}"))
}

// ---------------------------------------------------------------------------
// The trace.

/// One harness observation. Notes are appended under one lock, so their
/// order is a true order across workers and the test thread.
#[derive(Clone, Debug)]
pub enum Note {
    /// The driver got the run guard of `task`'s run number `execution`.
    RunStart {
        task: TaskId,
        worker: WorkerId,
        execution: u64,
    },
    /// The run ended (claim returned), before the executor records it.
    RunEnd {
        task: TaskId,
        worker: WorkerId,
        outcome: OutcomeKind,
    },
    /// A leaf host effect executed inside a run.
    Effect {
        task: TaskId,
        worker: WorkerId,
        op: u16,
        arg: f64,
    },
    /// A run is blocked on a closed gate (in a GATE effect, or in the
    /// Rust-need driver between subscription and park).
    Gated { task: TaskId, gate: u32 },
    /// A closed gate was never opened within the gate bound.
    GateTimedOut { task: TaskId, gate: u32 },
    /// A run claimed `key`'s single producer request; the harness holds it.
    Requested {
        task: TaskId,
        key: DependencyKey,
        request: u64,
    },
    /// The Rust-need driver subscribed `handle` to pending `key`.
    Subscribed {
        task: TaskId,
        key: DependencyKey,
        handle: WakeHandle,
    },
    /// The Rust-need driver read a terminal outcome (`ready`, `failed`,
    /// `cancelled`) or observed cancellation (`cancel requested`).
    Read {
        task: TaskId,
        key: DependencyKey,
        what: &'static str,
    },
    /// Something the test did (publication, cancellation, wake).
    Test(String),
    /// A broken invariant noticed while the scenario ran.
    Violation(String),
}

impl fmt::Display for Note {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Note::RunStart {
                task,
                worker,
                execution,
            } => write!(f, "{task} run {execution} starts on {worker}"),
            Note::RunEnd {
                task,
                worker,
                outcome,
            } => write!(f, "{task} run ends on {worker}: {outcome:?}"),
            Note::Effect {
                task,
                worker,
                op,
                arg,
            } => {
                let name = match *op {
                    PRE => "PRE",
                    GATE => "GATE",
                    POST => "POST",
                    NOTE => "NOTE",
                    _ => "?",
                };
                write!(f, "{task} on {worker}: {name}({arg})")
            }
            Note::Gated { task, gate } => write!(f, "{task} blocked at gate {gate}"),
            Note::GateTimedOut { task, gate } => {
                write!(f, "{task}: gate {gate} never opened (bounded wait expired)")
            }
            Note::Requested { task, key, request } => {
                write!(f, "{task} claimed producer request #{request} for {key}")
            }
            Note::Subscribed { task, key, handle } => {
                write!(f, "{task} subscribed {handle} to {key}")
            }
            Note::Read { task, key, what } => write!(f, "{task} read {key}: {what}"),
            Note::Test(s) => write!(f, "test: {s}"),
            Note::Violation(s) => write!(f, "VIOLATION: {s}"),
        }
    }
}

/// A need the driver issues through [`RunGuard::need`] before running the
/// task's (trivial) VM code, for the window between subscription and park
/// that a VM `need` closes inside one instruction.
#[derive(Clone, Copy, Debug)]
pub struct RustNeed {
    pub def: DependencyId,
    pub kind: ResultKind,
    /// Block on this gate (if armed) after subscribing, before parking.
    pub park_gate: Option<u32>,
}

#[derive(Default)]
struct State {
    notes: Vec<Note>,
    tasks: HashMap<TaskId, Task>,
    plans: HashMap<TaskId, RustNeed>,
    /// Requests claimed by runs, held for the test.
    requests: Vec<(TaskId, ProducerRequest)>,
    /// Number of requests ever claimed.
    claimed: u64,
    /// Values the Rust-need driver read, per task.
    reads: HashMap<TaskId, Vec<Rooted>>,
    armed: HashSet<u32>,
    /// Tasks currently inside a driver call.
    inside: HashSet<TaskId>,
    /// Tasks allowed to end the scenario queued or parked.
    unfinished_ok: HashSet<TaskId>,
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    /// Bound of one wait ([`Config::step`]).
    step: Duration,
    /// Bound of one gate ([`Config::gate`]).
    gate: Duration,
}

impl Shared {
    fn note(&self, note: Note) {
        lock(&self.state).notes.push(note);
        self.changed.notify_all();
    }

    /// Block while `gate` is armed, bounded by the gate bound; false on
    /// timeout.
    fn pass_gate(&self, task: TaskId, gate: u32) -> bool {
        let mut state = lock(&self.state);
        if !state.armed.contains(&gate) {
            return true;
        }
        state.notes.push(Note::Gated { task, gate });
        self.changed.notify_all();
        let (mut state, _) = self
            .changed
            .wait_timeout_while(state, self.gate, |s| s.armed.contains(&gate))
            .unwrap_or_else(PoisonError::into_inner);
        if state.armed.contains(&gate) {
            state.notes.push(Note::GateTimedOut { task, gate });
            self.changed.notify_all();
            return false;
        }
        true
    }
}

/// Each worker's leaf-effect host.
struct HarnessHost {
    worker: WorkerId,
    shared: Arc<Shared>,
}

impl Host for HarnessHost {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        let Primitive::Number(arg) = call.arg else {
            return Err(format!("non-number host argument {:?}", call.arg));
        };
        self.shared.note(Note::Effect {
            task: call.task,
            worker: self.worker,
            op: call.op,
            arg,
        });
        match call.op {
            PRE | POST | NOTE => Ok(call.arg),
            GATE if self.shared.pass_gate(call.task, arg as u32) => Ok(call.arg),
            GATE => Err(format!("gate {arg} never opened")),
            op => Err(format!("unknown host operation {op}")),
        }
    }
}

fn kind_of(outcome: &RunOutcome) -> OutcomeKind {
    match outcome {
        RunOutcome::Complete(_) => OutcomeKind::Completed,
        RunOutcome::Failed(_) => OutcomeKind::Failed,
        RunOutcome::Waiting(key) => OutcomeKind::Waiting(*key),
        RunOutcome::Yielded => OutcomeKind::Yielded,
        RunOutcome::Cancelled => OutcomeKind::Cancelled,
    }
}

fn is_terminal(kind: OutcomeKind) -> bool {
    matches!(
        kind,
        OutcomeKind::Completed | OutcomeKind::Failed | OutcomeKind::Cancelled
    )
}

/// The recording driver: VM runs, or a Rust-side need first for tasks with
/// a [`RustNeed`] plan; producer requests the run claimed are held.
fn drive(shared: &Shared, mut guard: RunGuard<'_>, cx: &mut RunContext<'_>) -> RunOutcome {
    let id = guard.task_id();
    let execution = guard.wake_handle().execution();
    let plan = {
        let mut state = lock(&shared.state);
        if !state.inside.insert(id) {
            state.notes.push(Note::Violation(format!(
                "{id} entered a driver on {} while another driver ran it",
                cx.worker
            )));
        }
        state.notes.push(Note::RunStart {
            task: id,
            worker: cx.worker,
            execution,
        });
        shared.changed.notify_all();
        state.plans.get(&id).copied()
    };
    let outcome = match plan {
        None => guard.run_with(cx.budget, &mut *cx.host),
        Some(plan) => {
            let need = guard.need(plan.def, plan.kind);
            let key =
                DependencyKey::new(lock(&shared.state).tasks[&id].world(), plan.def, plan.kind);
            match need {
                Ok(Need::Ready(value)) => {
                    shared.note(Note::Read {
                        task: id,
                        key,
                        what: "ready",
                    });
                    lock(&shared.state).reads.entry(id).or_default().push(value);
                    guard.run_with(cx.budget, &mut *cx.host)
                }
                Ok(Need::Failed(_)) => {
                    shared.note(Note::Read {
                        task: id,
                        key,
                        what: "failed",
                    });
                    guard.run_with(cx.budget, &mut *cx.host)
                }
                Ok(Need::Cancelled) => {
                    shared.note(Note::Read {
                        task: id,
                        key,
                        what: "cancelled",
                    });
                    guard.run_with(cx.budget, &mut *cx.host)
                }
                Ok(Need::Waiting { key, produce }) => {
                    if let Some(request) = produce {
                        hold(shared, id, request);
                    }
                    shared.note(Note::Subscribed {
                        task: id,
                        key,
                        handle: guard.wake_handle(),
                    });
                    if let Some(gate) = plan.park_gate {
                        // A closed gate that times out still parks: the
                        // trace shows the timeout and the test fails on it.
                        shared.pass_gate(id, gate);
                    }
                    guard.park()
                }
                Err(NeedError::Cancelled { requested }) => {
                    shared.note(Note::Read {
                        task: id,
                        key: requested,
                        what: "cancel requested",
                    });
                    guard.park()
                }
                Err(e) => {
                    shared.note(Note::Violation(format!("{id}: unexpected need error {e}")));
                    guard.park()
                }
            }
        }
    };
    let task = lock(&shared.state).tasks.get(&id).cloned();
    if let Some(task) = task {
        for request in task.take_producer_requests() {
            hold(shared, id, request);
        }
    }
    let mut state = lock(&shared.state);
    state.inside.remove(&id);
    state.notes.push(Note::RunEnd {
        task: id,
        worker: cx.worker,
        outcome: kind_of(&outcome),
    });
    shared.changed.notify_all();
    outcome
}

fn hold(shared: &Shared, task: TaskId, request: ProducerRequest) {
    let mut state = lock(&shared.state);
    state.claimed += 1;
    let n = state.claimed;
    state.notes.push(Note::Requested {
        task,
        key: request.key(),
        request: n,
    });
    state.requests.push((task, request));
    shared.changed.notify_all();
}

// ---------------------------------------------------------------------------
// Placement.

/// Where enqueued tasks go.
#[derive(Clone, Copy, Debug)]
pub enum Route {
    /// Any idle worker.
    Any,
    /// A task woken, cancelled or yielded goes to a worker other than the
    /// one that ran it last: every resumption migrates.
    Migrate,
    /// Each enqueue picks a worker (or any) from a seeded generator.
    Seeded(u64),
}

/// xorshift64*: a tiny deterministic generator for seeded schedules.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn placement(
    route: Route,
) -> impl Fn(&big_boy_pants_js::exec::DispatchRequest) -> Placement + Send + Sync {
    let rng = Mutex::new(Rng::new(match route {
        Route::Seeded(seed) => seed,
        _ => 0,
    }));
    move |req| match route {
        Route::Any => Placement::Any,
        Route::Migrate => match (req.reason, req.last_worker) {
            (
                EnqueueReason::Woken | EnqueueReason::Cancelled | EnqueueReason::Yielded,
                Some(last),
            ) => Placement::Worker(w((last.index() + 1) % req.workers)),
            _ => Placement::Any,
        },
        Route::Seeded(_) => {
            let pick = lock(&rng).below(req.workers as u64 + 1) as usize;
            if pick == req.workers {
                Placement::Any
            } else {
                Placement::Worker(w(pick))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The harness.

struct Inner {
    name: String,
    rt: Runtime,
    exec: Executor,
    shared: Arc<Shared>,
}

impl Inner {
    fn dump(&self) -> String {
        let state = lock(&self.shared.state);
        let mut out = format!(
            "==== trace of scenario `{}` ====\n-- harness notes:\n",
            self.name
        );
        for (i, note) in state.notes.iter().enumerate() {
            out.push_str(&format!("{i:4} {note}\n"));
        }
        drop(state);
        out.push_str("-- executor events:\n");
        for e in self.exec.events() {
            let who = e.worker.map_or("-".to_string(), |w| w.to_string());
            let task = e.task.map_or("-".to_string(), |t| t.to_string());
            out.push_str(&format!("{:4} [{who}] {task}: {:?}\n", e.seq, e.kind));
        }
        out
    }
}

/// One running scenario: a runtime, a started executor and the trace.
pub struct Harness(Arc<Inner>);

/// Scenario settings.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub workers: usize,
    pub route: Route,
    /// Instructions per run.
    pub budget: u64,
    /// Bound of one wait (default [`STEP`]).
    pub step: Duration,
    /// Bound of a run blocked at a closed gate (default [`STEP`]).
    pub gate: Duration,
    /// Bound of the whole scenario (default [`SCENARIO`]).
    pub scenario: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            workers: 2,
            route: Route::Migrate,
            budget: 10_000,
            step: STEP,
            gate: STEP,
            scenario: SCENARIO,
        }
    }
}

impl Harness {
    fn start(name: &str, config: Config) -> Harness {
        let rt = Runtime::new();
        let shared = Arc::new(Shared {
            state: Mutex::default(),
            changed: Condvar::new(),
            step: config.step,
            gate: config.gate,
        });
        let hosts = Arc::clone(&shared);
        let drivers = Arc::clone(&shared);
        let exec = ExecutorConfig::new()
            .workers(config.workers)
            .budget(big_boy_pants_js::Budget::instructions(config.budget))
            .host(move |worker| {
                Box::new(HarnessHost {
                    worker,
                    shared: Arc::clone(&hosts),
                })
            })
            .driver(move |guard, cx| drive(&drivers, guard, cx))
            .placement(placement(config.route))
            .start(&rt)
            .expect("start the executor");
        Harness(Arc::new(Inner {
            name: name.to_string(),
            rt,
            exec,
            shared,
        }))
    }

    pub fn rt(&self) -> &Runtime {
        &self.0.rt
    }

    pub fn exec(&self) -> &Executor {
        &self.0.exec
    }

    fn step(&self) -> Duration {
        self.0.shared.step
    }

    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.0.shared.state)
    }

    /// The full trace: harness notes and executor events.
    pub fn dump(&self) -> String {
        self.0.dump()
    }

    /// Fail the scenario with `message` and the trace.
    pub fn fail(&self, message: impl fmt::Display) -> ! {
        panic!("{message}\n{}", self.dump())
    }

    pub fn note(&self, message: impl Into<String>) {
        self.0.shared.note(Note::Test(message.into()));
    }

    pub fn number(&self, x: f64) -> Rooted {
        self.0.rt.root(Primitive::Number(x))
    }

    /// A fresh persistent vector of numbers (a shareable object whose
    /// identity readers can check).
    pub fn vector(&self, items: &[f64]) -> Rooted {
        self.0.rt.scope(|s| {
            let items: Vec<_> = items
                .iter()
                .map(|&x| s.primitive(Primitive::Number(x)))
                .collect();
            let v = s.new_vector(&items).expect("new vector");
            s.persist(v)
        })
    }

    /// The error kind of `task`'s terminal outcome, which must be a failure.
    pub fn failure(&self, task: TaskId) -> big_boy_pants_js::VmErrorKind {
        match self.outcome(task) {
            RunOutcome::Failed(e) => e.kind(),
            other => self.fail(format!("{task} should fail, got {other:?}")),
        }
    }

    /// Spawn `exe` in `world` with numeric arguments.
    pub fn spawn(&self, world: WorldId, exe: &Executable, args: &[f64]) -> Task {
        let args: Vec<Rooted> = args.iter().map(|&x| self.number(x)).collect();
        let refs: Vec<&Rooted> = args.iter().collect();
        self.spawn_rooted(world, exe, &refs)
    }

    /// Spawn `exe` in `world` with rooted arguments.
    pub fn spawn_rooted(&self, world: WorldId, exe: &Executable, args: &[&Rooted]) -> Task {
        self.0
            .rt
            .spawn_in(world, exe, args)
            .unwrap_or_else(|e| self.fail(format!("spawn: {e:?}")))
    }

    /// Register and submit a task.
    pub fn submit(&self, task: &Task, placement: Placement) -> TaskId {
        let id = task.id();
        self.state().tasks.insert(id, task.clone());
        self.note(format!("submit {id} on {placement:?}"));
        self.0
            .exec
            .submit_with(task.clone(), placement)
            .unwrap_or_else(|e| self.fail(e));
        id
    }

    /// Submit a task whose runs first need `plan` through the run guard.
    pub fn submit_rust_need(&self, task: &Task, plan: RustNeed, placement: Placement) -> TaskId {
        self.state().plans.insert(task.id(), plan);
        self.submit(task, placement)
    }

    pub fn task(&self, id: TaskId) -> Task {
        self.state().tasks[&id].clone()
    }

    /// Allow `task` to end the scenario parked or queued.
    pub fn allow_unfinished(&self, task: TaskId) {
        self.state().unfinished_ok.insert(task);
    }

    pub fn arm(&self, gate: u32) {
        self.state().armed.insert(gate);
    }

    pub fn open(&self, gate: u32) {
        self.state().armed.remove(&gate);
        self.0.shared.changed.notify_all();
        self.note(format!("open gate {gate}"));
    }

    fn open_all(&self) {
        self.state().armed.clear();
        self.0.shared.changed.notify_all();
    }

    /// Notes so far.
    pub fn notes(&self) -> Vec<Note> {
        self.state().notes.clone()
    }

    pub fn events(&self) -> Vec<Event> {
        self.0.exec.events()
    }

    /// Wait (bounded) until `ready` holds over the harness notes.
    pub fn wait_notes(&self, what: &str, mut ready: impl FnMut(&[Note]) -> bool) {
        let deadline = Instant::now() + self.step();
        let mut state = self.state();
        loop {
            if ready(&state.notes) {
                return;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                drop(state);
                self.fail(format!("bounded wait expired: {what}"));
            }
            state = self
                .0
                .shared
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Wait (bounded) until `ready` holds over the executor's events.
    pub fn wait_events(&self, what: &str, ready: impl FnMut(&[Event]) -> bool) {
        if !self.0.exec.wait_for(self.step(), ready) {
            self.fail(format!("bounded wait expired: {what}"));
        }
    }

    /// Wait until `task` is blocked at `gate`.
    pub fn wait_gated(&self, task: TaskId, gate: u32) {
        self.wait_notes(&format!("{task} blocked at gate {gate}"), |n| {
            n.iter()
                .any(|n| matches!(n, Note::Gated { task: t, gate: g } if *t == task && *g == gate))
        });
    }

    /// Wait until the executor recorded the `nth` (from 1) release of `task`
    /// and return how that run ended.
    pub fn wait_release(&self, task: TaskId, nth: usize) -> OutcomeKind {
        let mut found = None;
        self.wait_events(&format!("release #{nth} of {task}"), |events| {
            found = releases(events, task).get(nth - 1).copied();
            found.is_some()
        });
        found.unwrap()
    }

    /// Wait until `task` is parked: its latest release is `Waiting`.
    pub fn wait_parked(&self, task: TaskId) -> DependencyKey {
        let mut key = None;
        self.wait_events(&format!("{task} parked"), |events| {
            key = match releases(events, task).last() {
                Some(OutcomeKind::Waiting(k)) => Some(*k),
                _ => None,
            };
            key.is_some()
        });
        key.unwrap()
    }

    /// The terminal outcome of `task` (bounded).
    pub fn outcome(&self, task: TaskId) -> RunOutcome {
        self.0
            .exec
            .take_outcome(task, self.step())
            .unwrap_or_else(|| self.fail(format!("{task} reached no terminal outcome")))
    }

    pub fn complete_value(&self, task: TaskId) -> Rooted {
        match self.outcome(task) {
            RunOutcome::Complete(v) => v,
            other => self.fail(format!("{task} should complete, got {other:?}")),
        }
    }

    /// Requests claimed and not yet taken.
    pub fn take_requests(&self) -> Vec<(TaskId, ProducerRequest)> {
        std::mem::take(&mut self.state().requests)
    }

    /// How many producer requests runs ever claimed.
    pub fn claimed(&self) -> u64 {
        self.state().claimed
    }

    /// The one request claimed for `key` (taken from the held ones).
    pub fn take_request(&self, key: DependencyKey) -> ProducerRequest {
        let mut state = self.state();
        let Some(i) = state.requests.iter().position(|(_, r)| r.key() == key) else {
            drop(state);
            self.fail(format!("no producer request held for {key}"));
        };
        state.requests.remove(i).1
    }

    /// Values the Rust-need driver read for `task`.
    pub fn reads(&self, task: TaskId) -> Vec<Rooted> {
        self.state().reads.get(&task).cloned().unwrap_or_default()
    }

    /// `(worker, arg)` of `task`'s effects with operation `op`, in order.
    pub fn effects(&self, task: TaskId, op: u16) -> Vec<(WorkerId, f64)> {
        self.state()
            .notes
            .iter()
            .filter_map(|n| match *n {
                Note::Effect {
                    task: t,
                    worker,
                    op: o,
                    arg,
                } if t == task && o == op => Some((worker, arg)),
                _ => None,
            })
            .collect()
    }

    /// The wake handles the Rust-need driver subscribed for `task`.
    pub fn subscriptions(&self, task: TaskId) -> Vec<WakeHandle> {
        self.state()
            .notes
            .iter()
            .filter_map(|n| match *n {
                Note::Subscribed {
                    task: t, handle, ..
                } if t == task => Some(handle),
                _ => None,
            })
            .collect()
    }

    /// Publish `completion` for `request` and enqueue its runnable wakes;
    /// returns the woken task ids (runnable deliveries only) and the full
    /// delivery list as `(handle, kind)` names.
    pub fn publish(
        &self,
        request: &ProducerRequest,
        completion: big_boy_pants_js::Completion<'_>,
    ) -> Delivered {
        let wakeups = self
            .0
            .rt
            .complete(request.key(), request.producer(), completion)
            .unwrap_or_else(|e| self.fail(format!("complete {}: {e}", request.key())));
        self.deliver(format!("publish {}", request.key()), wakeups)
    }

    /// Enqueue the runnable deliveries of `wakeups` and describe them.
    pub fn deliver(&self, what: String, wakeups: Wakeups) -> Delivered {
        let delivered = Delivered::of(&wakeups);
        self.note(format!("{what}: {delivered}"));
        let n = self.0.exec.wake(wakeups).unwrap_or_else(|e| self.fail(e));
        assert_eq!(n, delivered.runnable.len());
        delivered
    }

    /// [`Executor::cancel`], noted; true if it enqueued a parked task.
    pub fn cancel(&self, task: &Task) -> bool {
        let enqueued = self.0.exec.cancel(task).unwrap_or_else(|e| self.fail(e));
        self.note(format!("cancel {}: enqueued={enqueued}", task.id()));
        enqueued
    }

    /// Have a worker run a collection and wait (bounded) for its record.
    pub fn collect(&self) -> CollectionEvent {
        let before = collections(&self.events()).len();
        self.0.exec.schedule_collection(Placement::Any);
        let mut found = None;
        self.wait_events("a scheduled collection", |events| {
            found = collections(events).get(before).cloned();
            found.is_some()
        });
        let c = found.unwrap();
        self.note(format!(
            "collection #{}: {} live, {} reclaimed",
            c.number, c.stats.live, c.stats.reclaimed
        ));
        c
    }

    /// Pin one blocker task per worker inside a closed gate, so no queued
    /// task is dispatched until [`Occupied::release`]: a woken or cancelled
    /// task then stays queued while the test acts.
    pub fn occupy(&self, blocker: &Executable, first_gate: u32) -> Occupied {
        let mut ids = Vec::new();
        for i in 0..self.0.exec.workers() {
            let gate = first_gate + i as u32;
            self.arm(gate);
            let task = self.spawn(self.rt().world(), blocker, &[gate as f64]);
            let id = self.submit(&task, Placement::Worker(w(i)));
            self.wait_gated(id, gate);
            ids.push((id, gate));
        }
        Occupied { ids }
    }

    /// The invariant checks every scenario ends with (module docs).
    fn finish(self) {
        // A run still blocked at a gate the body never opened is a hang the
        // body walked away from: report it before releasing the gates.
        let blocked: Vec<String> = {
            let state = self.state();
            state
                .notes
                .iter()
                .filter_map(|n| match *n {
                    Note::Gated { task, gate } if state.armed.contains(&gate) => Some(format!(
                        "{task} still blocked at gate {gate} when the scenario body returned"
                    )),
                    _ => None,
                })
                .collect()
        };
        self.open_all();
        let violations: Vec<String> = blocked
            .into_iter()
            .chain(self.notes().into_iter().filter_map(|n| match n {
                Note::Violation(s) => Some(s),
                Note::GateTimedOut { task, gate } => {
                    Some(format!("{task} timed out at gate {gate}"))
                }
                _ => None,
            }))
            .collect();
        if !violations.is_empty() {
            self.fail(format!("violations: {violations:#?}"));
        }
        let inner = match Arc::try_unwrap(self.0) {
            Ok(inner) => inner,
            Err(_) => unreachable!("the harness is only shared weakly"),
        };
        let Inner {
            name,
            rt,
            exec,
            shared,
        } = inner;
        let shutdown = match exec.shutdown(shared.step) {
            Ok(s) => s,
            Err(e) => {
                let notes = lock(&shared.state).notes.clone();
                panic!(
                    "`{name}`: shutdown timed out ({:?}); notes: {notes:#?}",
                    e.busy
                )
            }
        };
        let state = lock(&shared.state);
        let report = |message: String| -> ! {
            let mut out = format!("`{name}`: {message}\n-- notes:\n");
            for (i, n) in state.notes.iter().enumerate() {
                out.push_str(&format!("{i:4} {n}\n"));
            }
            out.push_str("-- events:\n");
            for e in &shutdown.events {
                out.push_str(&format!(
                    "{:4} {:?} {:?}: {:?}\n",
                    e.seq, e.worker, e.task, e.kind
                ));
            }
            panic!("{out}")
        };
        if let Err(e) = check_record(&shutdown.events) {
            report(e);
        }
        if let Err(e) = check_effects_inside_runs(&state.notes) {
            report(e);
        }
        for task in shutdown.pending.iter().chain(&shutdown.parked) {
            if !state.unfinished_ok.contains(&task.id()) {
                report(format!(
                    "{} was left {:?} at shutdown (lost wakeup or missing settlement)",
                    task.id(),
                    task.status()
                ));
            }
        }
        for (id, task) in &state.tasks {
            let status = task.status();
            if !state.unfinished_ok.contains(id)
                && !matches!(status, TaskStatus::Finished | TaskStatus::Cancelled)
            {
                report(format!("{id} ended the scenario {status:?}"));
            }
        }
        drop(state);
        drop(rt);
    }
}

/// Blocker tasks holding every worker ([`Harness::occupy`]).
pub struct Occupied {
    ids: Vec<(TaskId, u32)>,
}

impl Occupied {
    pub fn release(self, h: &Harness) {
        for &(_, gate) in &self.ids {
            h.open(gate);
        }
        for (id, _) in self.ids {
            h.complete_value(id);
        }
    }
}

/// What one completion's wakes did.
#[derive(Clone, Debug, Default)]
pub struct Delivered {
    pub handles: Vec<WakeHandle>,
    pub runnable: Vec<TaskId>,
    pub remembered: Vec<TaskId>,
    pub stale: Vec<TaskId>,
}

impl Delivered {
    fn of(wakeups: &Wakeups) -> Delivered {
        let mut d = Delivered::default();
        for (handle, wake) in wakeups.deliveries() {
            d.handles.push(*handle);
            let list = match wake {
                big_boy_pants_js::Wake::Runnable(_) => &mut d.runnable,
                big_boy_pants_js::Wake::Remembered => &mut d.remembered,
                big_boy_pants_js::Wake::Stale => &mut d.stale,
            };
            list.push(handle.task());
        }
        d
    }
}

impl fmt::Display for Delivered {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "runnable {:?}, remembered {:?}, stale {:?}",
            self.runnable, self.remembered, self.stale
        )
    }
}

// ---------------------------------------------------------------------------
// Record checks.

pub fn releases(events: &[Event], task: TaskId) -> Vec<OutcomeKind> {
    events
        .iter()
        .filter(|e| e.task == Some(task))
        .filter_map(|e| match e.kind {
            EventKind::Released(kind) => Some(kind),
            _ => None,
        })
        .collect()
}

pub fn enqueues(events: &[Event], task: TaskId) -> Vec<EnqueueReason> {
    events
        .iter()
        .filter(|e| e.task == Some(task))
        .filter_map(|e| match e.kind {
            EventKind::Enqueued { reason, .. } => Some(reason),
            _ => None,
        })
        .collect()
}

/// `(worker, execution)` of each dispatch of `task`.
pub fn dispatches(events: &[Event], task: TaskId) -> Vec<(WorkerId, u64)> {
    events
        .iter()
        .filter(|e| e.task == Some(task))
        .filter_map(|e| match e.kind {
            EventKind::Dispatched { execution } => e.worker.map(|w| (w, execution)),
            _ => None,
        })
        .collect()
}

pub fn collections(events: &[Event]) -> Vec<CollectionEvent> {
    events
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::Collected(c) => Some(c.clone()),
            _ => None,
        })
        .collect()
}

/// Per task: dispatch and release alternate on one worker, execution
/// numbers strictly increase, at most one terminal release, and nothing
/// is dispatched after it.
fn check_record(events: &[Event]) -> Result<(), String> {
    let tasks: HashSet<TaskId> = events.iter().filter_map(|e| e.task).collect();
    for task in tasks {
        let mut owner: Option<WorkerId> = None;
        let mut last_execution = 0;
        let mut terminal = None;
        for e in events.iter().filter(|e| e.task == Some(task)) {
            match e.kind {
                EventKind::Dispatched { execution } => {
                    if owner.is_some() {
                        return Err(format!("{task} dispatched twice at once (event {})", e.seq));
                    }
                    if let Some(t) = terminal {
                        return Err(format!(
                            "{task} dispatched (event {}) after its terminal release {t:?}",
                            e.seq
                        ));
                    }
                    if execution <= last_execution {
                        return Err(format!(
                            "{task} execution number {execution} after {last_execution}"
                        ));
                    }
                    last_execution = execution;
                    owner = e.worker;
                }
                EventKind::Released(kind) => {
                    if owner != e.worker || owner.is_none() {
                        return Err(format!("{task} released by a non-owner (event {})", e.seq));
                    }
                    owner = None;
                    if is_terminal(kind) {
                        if let Some(t) = terminal {
                            return Err(format!("{task} terminal twice: {t:?} then {kind:?}"));
                        }
                        terminal = Some(kind);
                    }
                }
                EventKind::Panicked => return Err(format!("{task}: the driver panicked")),
                _ => {}
            }
        }
        if owner.is_some() {
            return Err(format!("{task} still owned at shutdown"));
        }
    }
    Ok(())
}

/// Every effect of a task lies between a `RunStart` and `RunEnd` of that
/// task on the effect's worker, and no task has two runs open at once.
fn check_effects_inside_runs(notes: &[Note]) -> Result<(), String> {
    let mut open: HashMap<TaskId, WorkerId> = HashMap::new();
    for (i, note) in notes.iter().enumerate() {
        match *note {
            Note::RunStart { task, worker, .. } => {
                if let Some(other) = open.insert(task, worker) {
                    return Err(format!(
                        "note {i}: {task} started on {worker} while running on {other}"
                    ));
                }
            }
            // The guard closes the run it checks.
            Note::RunEnd { task, worker, .. } if open.remove(&task) != Some(worker) => {
                return Err(format!(
                    "note {i}: {task} ended a run it did not start on {worker}"
                ));
            }
            Note::Effect { task, worker, .. } if open.get(&task) != Some(&worker) => {
                return Err(format!(
                    "note {i}: effect of {task} on {worker} outside its run"
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The bounded runner.

/// Run `body` against a fresh harness under the hang bound [`SCENARIO`],
/// then the invariant checks. Any failure (assertion, panic, hang, broken
/// invariant) fails the test with the trace.
pub fn scenario(name: &str, config: Config, body: impl FnOnce(&Harness) + Send + 'static) {
    let slot: Arc<Mutex<Option<Weak<Inner>>>> = Arc::default();
    let (done_tx, done_rx) = mpsc::channel();
    let runner_slot = Arc::clone(&slot);
    let runner_name = name.to_string();
    thread::Builder::new()
        .name(format!("scenario {name}"))
        .spawn(move || {
            let result = panic::catch_unwind(AssertUnwindSafe(|| {
                let h = Harness::start(&runner_name, config);
                *lock(&runner_slot) = Some(Arc::downgrade(&h.0));
                if let Err(payload) = panic::catch_unwind(AssertUnwindSafe(|| body(&h))) {
                    eprintln!("{}", h.dump());
                    // Let gated runs go so the workers stop promptly.
                    h.open_all();
                    panic::resume_unwind(payload);
                }
                h.finish();
            }));
            let _ = done_tx.send(result);
        })
        .expect("spawn the scenario thread");
    match done_rx.recv_timeout(config.scenario) {
        Ok(Ok(())) => {}
        Ok(Err(payload)) => panic::resume_unwind(payload),
        Err(RecvTimeoutError::Disconnected) => panic!("scenario `{name}` vanished"),
        Err(RecvTimeoutError::Timeout) => {
            // Dump on a helper thread, itself bounded: a deadlocked executor
            // lock must not turn the hang report into a hang.
            let (dump_tx, dump_rx) = mpsc::channel();
            thread::spawn(move || {
                let dump = lock(&slot)
                    .as_ref()
                    .and_then(Weak::upgrade)
                    .map_or("(no harness yet)".to_string(), |inner| inner.dump());
                let _ = dump_tx.send(dump);
            });
            let dump = dump_rx
                .recv_timeout(config.step)
                .unwrap_or_else(|_| "(the trace could not be read: a lock is held)".into());
            panic!(
                "scenario `{name}` hung: no result within {:?}\n{dump}",
                config.scenario
            );
        }
    }
}
