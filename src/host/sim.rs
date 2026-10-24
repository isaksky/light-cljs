//! A small simulated compiler host (`docs/design.md` §§9, 12): immutable
//! worlds of synthetic definitions, separate result kinds per definition, and
//! registered producers scheduled through the dependency-cell protocol.
//!
//! This is not a compiler. A definition is a host-assigned [`DependencyId`]
//! with a name and one [`Producer`] per [`ResultKind`]; there is no parser,
//! analyzer, namespace or module system behind it.
//!
//! # Worlds and result kinds
//!
//! A [`WorldSpec`] is a plain description of definitions. Registering it
//! ([`SimHost::add_world`]) freezes it under a fresh [`WorldId`]
//! ([`Runtime::new_world`]): a registered world never changes. A changed
//! program is a new world registered from an edited copy of the spec; the old
//! world keeps its definitions and its completed cells, which are never reset
//! (no invalidation in this slice). Since the world is part of every
//! [`DependencyKey`], a world-2 result never satisfies a world-1 need.
//!
//! Each definition has separate single-assignment results, one cell per kind:
//! [`BINDING`] (binding discovery: the name exists), [`CALLABLE`] (a callable
//! is available), [`INITIALIZED`] (initializer completion) and [`ANALYSIS`].
//! None of them is a coarse "ready" flag, and producing one does not imply
//! producing another: a callable that refers to another definition needs
//! only that definition's binding, not its callable or initializer, so
//! recursive and mutually recursive functions become callable without
//! waiting for any initializer (and without a wait cycle).
//!
//! # Producers
//!
//! A [`Producer`] declares the results it needs (`needing`), and a body:
//!
//! * [`Producer::rust`]: Rust work over the needed values, run exactly once
//!   after every need is ready (a producer is never restarted, so its work
//!   is never replayed);
//! * [`Producer::callable`]: publishes a VM function compiled by this runtime
//!   (for example by [`frontend::compile_function`](crate::frontend::compile_function));
//! * [`Producer::invoke`]: an initializer whose value is the result of
//!   calling a definition's callable through the VM.
//!
//! A definition without an explicit [`BINDING`] producer answers binding
//! discovery with `true`.
//!
//! # Scheduling through the dependency protocol
//!
//! Every unit of host work is a *job* with its own runtime [`Task`] in the
//! job's world: the task is the job's consumer identity and wake handle, and
//! its [`DependencyLog`] records what the job used. A job needs a result with
//! [`RunGuard::need`](crate::RunGuard::need) (standing in for the `__rt.need`
//! VM operation, pellet clj-quickjs-23). When the cell is pending the job
//! parks; when the need was the first for that cell it carries the cell's one
//! [`ProducerRequest`], and the host schedules a
//! producer job for exactly that key. So there is one producer per result key
//! and the producer is discovered through the cell, not by the asker. A
//! completion returns the wake handles of the parked jobs, which the host
//! moves back to its ready queue; nothing runs inline inside completion.
//!
//! The driver ([`SimHost::run_until_idle`]) is a deterministic, single-thread
//! reference loop over a FIFO queue; an executor with worker threads is
//! pellet clj-quickjs-8.
//!
//! # Failures
//!
//! Every requested key settles: absent results fail deterministically,
//! decided from the world's immutable description when the producer request
//! is scheduled, never by a timeout. A key whose world is not registered
//! fails with [`SimError::UnknownWorld`], an unknown definition with
//! [`SimError::MissingDefinition`], a known definition without a producer for
//! the kind with [`SimError::MissingResult`]. A Rust producer returning an
//! error (or a result that fails the publication check) settles its cell as
//! `Failed` ([`SimError::ProducerFailed`]); a producer whose input failed or
//! was cancelled fails with [`SimError::InputFailed`] /
//! [`SimError::InputCancelled`]. The published error value is the number
//! [`SimError::code`]; the full error is kept by the host
//! ([`SimHost::failure`]). When the queue runs dry while a top-level request
//! is still waiting, the request reports [`SimError::Stalled`] instead of
//! hanging; reporting the wait cycle itself is pellet clj-quickjs-26.
//!
//! # Calls (stand-in)
//!
//! The VM has no call instruction or function values yet (pellets
//! clj-quickjs-4, -20, -21, -23). Until then a published callable is a host
//! token (a persistent vector holding the callable's index in the host's
//! table, unforgeable because the host checks its identity) and calls are
//! dispatched by the host. An activation is a new VM task running the
//! callable's executable with one argument. A callable may declare one
//! `tail_callee`: when an activation returns a **number**, the host needs
//! the tail callee's [`CALLABLE`] result (recording the edge) and calls it
//! with that number; any other returned value is the result. The control
//! flow (base case, recursion step, result) is the JavaScript's: the host
//! only performs the call that the function requested. Activations per
//! invocation are bounded ([`SimHost::set_activation_limit`]).

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt;
use std::sync::Arc;

use crate::dep::{CompleteError, Completion, DependencyLog, Need, ProducerRequest};
use crate::heap::Rooted;
use crate::ids::{DependencyId, DependencyKey, ProducerId, ResultKind, TaskId, WorldId};
use crate::runtime::Runtime;
use crate::task::{RunOutcome, Task};
use crate::value::{Primitive, ValueView};
use crate::vm::Executable;

/// Binding discovery: the definition's name exists in the world.
pub const BINDING: ResultKind = ResultKind::new(1);
/// Callable availability: a function value for the definition.
pub const CALLABLE: ResultKind = ResultKind::new(2);
/// Initializer completion: the definition's initialized value.
pub const INITIALIZED: ResultKind = ResultKind::new(3);
/// An analysis result about the definition.
pub const ANALYSIS: ResultKind = ResultKind::new(4);

/// Rust producer work: given the runtime and the needed values (in
/// `needing` order), return the result or an error message.
pub type RustWork = dyn Fn(&Runtime, &[Rooted]) -> Result<Rooted, String> + Send + Sync;

/// How one result of a definition is produced. See the module docs.
#[derive(Clone)]
pub struct Producer {
    needs: Vec<(DependencyId, ResultKind)>,
    body: Body,
}

#[derive(Clone)]
enum Body {
    Rust(Arc<RustWork>),
    Callable {
        function: Executable,
        tail_callee: Option<DependencyId>,
    },
    Invoke {
        callee: DependencyId,
        arg: Primitive,
    },
}

impl Producer {
    /// Rust work, run once when every need is ready.
    pub fn rust(
        work: impl Fn(&Runtime, &[Rooted]) -> Result<Rooted, String> + Send + Sync + 'static,
    ) -> Producer {
        Producer {
            needs: Vec::new(),
            body: Body::Rust(Arc::new(work)),
        }
    }

    /// Publish `function`, a VM executable taking one argument, as a
    /// callable. With `tail_callee`, a number returned by an activation is a
    /// tail call of that definition (module docs, "Calls").
    pub fn callable(function: Executable, tail_callee: Option<DependencyId>) -> Producer {
        Producer {
            needs: Vec::new(),
            body: Body::Callable {
                function,
                tail_callee,
            },
        }
    }

    /// The result of calling `callee`'s callable with `arg`.
    pub fn invoke(callee: DependencyId, arg: Primitive) -> Producer {
        Producer {
            needs: Vec::new(),
            body: Body::Invoke { callee, arg },
        }
    }

    /// Also need result `kind` of `definition` (in the producer's world)
    /// before the body runs. Needs are taken in the order added.
    pub fn needing(mut self, definition: DependencyId, kind: ResultKind) -> Producer {
        self.needs.push((definition, kind));
        self
    }
}

impl fmt::Debug for Producer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let body = match &self.body {
            Body::Rust(_) => "rust",
            Body::Callable { .. } => "callable",
            Body::Invoke { .. } => "invoke",
        };
        f.debug_struct("Producer")
            .field("needs", &self.needs)
            .field("body", &body)
            .finish()
    }
}

#[derive(Clone, Debug)]
struct Definition {
    name: String,
    producers: BTreeMap<ResultKind, Producer>,
}

/// A description of a world's definitions; frozen by
/// [`SimHost::add_world`]. Cloning and editing a spec is how a new world
/// version is made.
#[derive(Clone, Debug, Default)]
pub struct WorldSpec {
    definitions: BTreeMap<DependencyId, Definition>,
}

impl WorldSpec {
    pub fn new() -> WorldSpec {
        WorldSpec::default()
    }

    /// Define (or redefine, dropping its producers) `id` named `name`.
    pub fn define(&mut self, id: DependencyId, name: &str) -> &mut WorldSpec {
        self.definitions.insert(
            id,
            Definition {
                name: name.to_owned(),
                producers: BTreeMap::new(),
            },
        );
        self
    }

    /// Set the producer of result `kind` of `id`.
    ///
    /// # Panics
    ///
    /// If `id` has not been defined.
    pub fn produce(&mut self, id: DependencyId, kind: ResultKind, producer: Producer) -> &mut Self {
        let Some(definition) = self.definitions.get_mut(&id) else {
            panic!("{id} must be defined before its {kind} producer is set");
        };
        definition.producers.insert(kind, producer);
        self
    }

    /// Remove definition `id`.
    pub fn remove(&mut self, id: DependencyId) -> &mut WorldSpec {
        self.definitions.remove(&id);
        self
    }

    /// The name of definition `id`, if defined.
    pub fn name(&self, id: DependencyId) -> Option<&str> {
        self.definitions.get(&id).map(|d| d.name.as_str())
    }
}

/// Why a simulated-host result failed. See the module docs, "Failures".
#[derive(Clone, Debug, PartialEq)]
pub enum SimError {
    /// `key`'s world is not registered with this host.
    UnknownWorld { key: DependencyKey },
    /// `key`'s definition does not exist in its world.
    MissingDefinition { key: DependencyKey },
    /// The definition exists but has no producer for `key`'s result kind.
    MissingResult { key: DependencyKey },
    /// `key`'s producer returned an error (or an unshareable result).
    ProducerFailed { key: DependencyKey, message: String },
    /// Producing `key` needed `input`, whose cell failed.
    InputFailed {
        key: DependencyKey,
        input: DependencyKey,
    },
    /// Producing `key` needed `input`, whose cell was cancelled.
    InputCancelled {
        key: DependencyKey,
        input: DependencyKey,
    },
    /// The `CALLABLE` cell `callable` holds no callable of this host.
    NotCallable { callable: DependencyKey },
    /// An activation of `callable` failed in the VM.
    ActivationFailed {
        callable: DependencyKey,
        message: String,
    },
    /// An invocation exceeded the activation limit while calling `callable`.
    ActivationLimit {
        callable: DependencyKey,
        limit: usize,
    },
    /// The host ran out of work while the request still waits on `waiting`.
    Stalled { waiting: DependencyKey },
    /// A requested cell failed with an error this host did not publish
    /// (it was completed by someone else).
    Failed { key: DependencyKey },
    /// A requested cell was cancelled.
    Cancelled { key: DependencyKey },
    /// A runtime API refused a host operation (a host bug).
    Internal { message: String },
}

impl SimError {
    /// The number published as the cell's `Failed` value.
    pub fn code(&self) -> f64 {
        match self {
            SimError::UnknownWorld { .. } => 1.0,
            SimError::MissingDefinition { .. } => 2.0,
            SimError::MissingResult { .. } => 3.0,
            SimError::ProducerFailed { .. } => 4.0,
            SimError::InputFailed { .. } => 5.0,
            SimError::InputCancelled { .. } => 6.0,
            SimError::NotCallable { .. } => 7.0,
            SimError::ActivationFailed { .. } => 8.0,
            SimError::ActivationLimit { .. } => 9.0,
            SimError::Stalled { .. } => 10.0,
            SimError::Failed { .. } => 11.0,
            SimError::Cancelled { .. } => 12.0,
            SimError::Internal { .. } => 13.0,
        }
    }
}

impl fmt::Display for SimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SimError::UnknownWorld { key } => write!(f, "{key}: unknown world"),
            SimError::MissingDefinition { key } => write!(f, "{key}: no such definition"),
            SimError::MissingResult { key } => {
                write!(f, "{key}: the definition has no producer for this result")
            }
            SimError::ProducerFailed { key, message } => write!(f, "{key}: {message}"),
            SimError::InputFailed { key, input } => write!(f, "{key}: input {input} failed"),
            SimError::InputCancelled { key, input } => {
                write!(f, "{key}: input {input} was cancelled")
            }
            SimError::NotCallable { callable } => write!(f, "{callable} is not callable"),
            SimError::ActivationFailed { callable, message } => {
                write!(f, "call of {callable} failed: {message}")
            }
            SimError::ActivationLimit { callable, limit } => {
                write!(f, "call of {callable} exceeded {limit} activations")
            }
            SimError::Stalled { waiting } => {
                write!(f, "no runnable work while waiting on {waiting}")
            }
            SimError::Failed { key } => write!(f, "{key} failed"),
            SimError::Cancelled { key } => write!(f, "{key} was cancelled"),
            SimError::Internal { message } => write!(f, "simulated host error: {message}"),
        }
    }
}

impl std::error::Error for SimError {}

/// One recorded host event, in order ([`SimHost::events`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SimEvent {
    /// A producer was scheduled for `key` (once per key).
    Scheduled {
        key: DependencyKey,
        producer: ProducerId,
    },
    /// Job task `task` parked on pending `waiting`.
    Parked {
        task: TaskId,
        waiting: DependencyKey,
    },
    /// Parked job task `task` was woken by a completion.
    Woken { task: TaskId },
    /// VM task `task` ran an activation of `callable`.
    Activation {
        callable: DependencyKey,
        task: TaskId,
    },
    /// `key`'s cell was completed by this host; `ok` unless `Failed`.
    Settled { key: DependencyKey, ok: bool },
}

/// The answer to a top-level request ([`SimHost::need`], [`SimHost::invoke`]).
#[derive(Debug)]
pub struct Answer {
    /// The value, or why there is none.
    pub result: Result<Rooted, SimError>,
    /// The requesting job's task (its consumer identity).
    pub task: TaskId,
    /// What the request used: recorded edges and any unresolved wait.
    pub log: DependencyLog,
}

/// A published callable (module docs, "Calls").
#[derive(Debug)]
struct CallableEntry {
    token: Rooted,
    function: Executable,
    tail_callee: Option<DependencyId>,
}

/// Where a call's final result goes.
enum Sink {
    Cell(ProducerRequest),
    Answer(u64),
}

enum Work {
    Produce {
        request: ProducerRequest,
        producer: Producer,
        inputs: Vec<Rooted>,
    },
    Need {
        answer: u64,
        definition: DependencyId,
        kind: ResultKind,
    },
    Call {
        sink: Sink,
        callee: DependencyId,
        arg: Rooted,
        activations: usize,
    },
}

struct Job {
    task: Task,
    work: Work,
}

/// What one need found.
enum Got {
    Value(Rooted),
    Failed(DependencyKey),
    Cancelled(DependencyKey),
    /// Pending: park, then schedule the request if this need claimed one.
    Pending(DependencyKey, Option<ProducerRequest>),
    Error(SimError),
}

/// The simulated host. See the module docs.
pub struct SimHost {
    rt: Runtime,
    worlds: HashMap<WorldId, Arc<WorldSpec>>,
    /// Never-run executable of job tasks, which exist for their identity and
    /// dependency log.
    job_code: Executable,
    ready: VecDeque<Job>,
    parked: HashMap<TaskId, Job>,
    callables: Vec<CallableEntry>,
    failures: HashMap<DependencyKey, SimError>,
    producer_logs: HashMap<DependencyKey, DependencyLog>,
    answers: HashMap<u64, (Result<Rooted, SimError>, DependencyLog)>,
    next_answer: u64,
    events: Vec<SimEvent>,
    activation_limit: usize,
}

impl fmt::Debug for SimHost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SimHost")
            .field("worlds", &self.worlds.len())
            .field("ready", &self.ready.len())
            .field("parked", &self.parked.len())
            .field("callables", &self.callables.len())
            .finish_non_exhaustive()
    }
}

impl SimHost {
    /// A host with no worlds over runtime `rt`.
    pub fn new(rt: &Runtime) -> SimHost {
        let job_code = crate::frontend::compile_script("sim-job", "undefined")
            .expect("the job placeholder script compiles");
        SimHost {
            rt: rt.clone(),
            worlds: HashMap::new(),
            job_code,
            ready: VecDeque::new(),
            parked: HashMap::new(),
            callables: Vec::new(),
            failures: HashMap::new(),
            producer_logs: HashMap::new(),
            answers: HashMap::new(),
            next_answer: 0,
            events: Vec::new(),
            activation_limit: 10_000,
        }
    }

    /// Freeze `spec` as a new world and return its identity.
    pub fn add_world(&mut self, spec: WorldSpec) -> WorldId {
        let world = self.rt.new_world();
        self.worlds.insert(world, Arc::new(spec));
        world
    }

    /// The frozen description of `world`.
    pub fn world(&self, world: WorldId) -> Option<&WorldSpec> {
        self.worlds.get(&world).map(|w| &**w)
    }

    /// Bound the activations of one invocation (default 10 000).
    pub fn set_activation_limit(&mut self, limit: usize) {
        self.activation_limit = limit;
    }

    /// Recorded events, oldest first.
    pub fn events(&self) -> &[SimEvent] {
        &self.events
    }

    /// The error this host published for `key`, if it failed it.
    pub fn failure(&self, key: DependencyKey) -> Option<&SimError> {
        self.failures.get(&key)
    }

    /// What `key`'s producer job used, once it has settled the cell.
    pub fn producer_log(&self, key: DependencyKey) -> Option<&DependencyLog> {
        self.producer_logs.get(&key)
    }

    /// Number of parked jobs (waiting on a pending cell).
    pub fn parked(&self) -> usize {
        self.parked.len()
    }

    /// Request result `kind` of `definition` in `world` and run until it is
    /// answered or no work is left.
    pub fn need(&mut self, world: WorldId, definition: DependencyId, kind: ResultKind) -> Answer {
        let answer = self.new_answer();
        let work = Work::Need {
            answer,
            definition,
            kind,
        };
        self.request(world, answer, work)
    }

    /// Call `definition`'s callable in `world` with `arg` and run until it
    /// returns or no work is left.
    pub fn invoke(&mut self, world: WorldId, definition: DependencyId, arg: &Rooted) -> Answer {
        let answer = self.new_answer();
        let work = Work::Call {
            sink: Sink::Answer(answer),
            callee: definition,
            arg: arg.clone(),
            activations: 0,
        };
        self.request(world, answer, work)
    }

    /// Wake parked jobs whose tasks are among `tasks`: for cells completed
    /// outside the host ([`Runtime::complete`] returns the wake handles).
    /// Unknown or unparked task IDs are ignored.
    pub fn wake(&mut self, tasks: &[TaskId]) {
        for &task in tasks {
            if let Some(job) = self.parked.remove(&task) {
                self.events.push(SimEvent::Woken { task });
                self.ready.push_back(job);
            }
        }
    }

    /// Run queued jobs until none is ready; returns how many ran.
    pub fn run_until_idle(&mut self) -> usize {
        let mut steps = 0;
        while let Some(job) = self.ready.pop_front() {
            self.run_job(job);
            steps += 1;
        }
        steps
    }

    fn new_answer(&mut self) -> u64 {
        self.next_answer += 1;
        self.next_answer
    }

    fn request(&mut self, world: WorldId, answer: u64, work: Work) -> Answer {
        let task = match self.spawn_job(world) {
            Ok(task) => task,
            Err(error) => {
                return Answer {
                    result: Err(error),
                    task: TaskId::from_raw(0),
                    log: DependencyLog::default(),
                };
            }
        };
        let id = task.id();
        self.ready.push_back(Job { task, work });
        self.run_until_idle();
        let (result, log) = match self.answers.remove(&answer) {
            Some(answered) => answered,
            None => {
                // Still parked: no work is left that could complete its wait.
                let log = self
                    .parked
                    .get(&id)
                    .map(|j| j.task.dependency_log())
                    .unwrap_or_default();
                let result = match log.waiting() {
                    Some(waiting) => Err(SimError::Stalled { waiting }),
                    None => Err(SimError::Internal {
                        message: format!("{id} was neither answered nor parked"),
                    }),
                };
                (result, log)
            }
        };
        Answer {
            result,
            task: id,
            log,
        }
    }

    fn spawn_job(&self, world: WorldId) -> Result<Task, SimError> {
        self.rt
            .spawn_in(world, &self.job_code, &[])
            .map_err(|e| SimError::Internal {
                message: format!("cannot spawn a job task: {e}"),
            })
    }

    /// One need by job task `task`.
    fn need_for(&self, task: &Task, definition: DependencyId, kind: ResultKind) -> Got {
        let key = DependencyKey::new(task.world(), definition, kind);
        let need = match task.try_begin_run() {
            Ok(mut guard) => guard.need(definition, kind),
            Err(e) => {
                return Got::Error(SimError::Internal {
                    message: format!("job {} cannot need {key}: {e}", task.id()),
                });
            }
        };
        match need {
            Ok(Need::Ready(value)) => Got::Value(value),
            Ok(Need::Failed(_)) => Got::Failed(key),
            Ok(Need::Cancelled) => Got::Cancelled(key),
            Ok(Need::Waiting { key, produce }) => Got::Pending(key, produce),
            Err(e) => Got::Error(SimError::Internal {
                message: format!("job {} cannot need {key}: {e}", task.id()),
            }),
        }
    }

    /// Park `job` on `waiting`, then schedule `produce`. Parking first means
    /// a producer that settles immediately (a missing definition) finds the
    /// job among the parked wake handles.
    fn park(&mut self, job: Job, waiting: DependencyKey, produce: Option<ProducerRequest>) {
        let task = job.task.id();
        self.events.push(SimEvent::Parked { task, waiting });
        self.parked.insert(task, job);
        if let Some(request) = produce {
            self.schedule(request);
        }
    }

    /// Start the producer of `request`'s key, or fail the cell at once when
    /// the world has none.
    fn schedule(&mut self, request: ProducerRequest) {
        let key = request.key();
        self.events.push(SimEvent::Scheduled {
            key,
            producer: request.producer(),
        });
        let producer = match self.producer_of(key) {
            Ok(producer) => producer,
            Err(error) => return self.settle(request, Err(error), None),
        };
        match self.spawn_job(key.world()) {
            Ok(task) => self.ready.push_back(Job {
                task,
                work: Work::Produce {
                    request,
                    producer,
                    inputs: Vec::new(),
                },
            }),
            Err(error) => self.settle(request, Err(error), None),
        }
    }

    fn producer_of(&self, key: DependencyKey) -> Result<Producer, SimError> {
        let world = self
            .worlds
            .get(&key.world())
            .ok_or(SimError::UnknownWorld { key })?;
        let definition = world
            .definitions
            .get(&key.dependency())
            .ok_or(SimError::MissingDefinition { key })?;
        match definition.producers.get(&key.kind()) {
            Some(producer) => Ok(producer.clone()),
            None if key.kind() == BINDING => {
                Ok(Producer::rust(|rt, _| Ok(rt.root(Primitive::Bool(true)))))
            }
            None => Err(SimError::MissingResult { key }),
        }
    }

    /// Complete `request`'s cell with `result` and wake its waiters. Every
    /// outcome settles the cell: an unshareable value becomes a failure.
    fn settle(
        &mut self,
        request: ProducerRequest,
        result: Result<Rooted, SimError>,
        task: Option<&Task>,
    ) {
        let key = request.key();
        if let Some(task) = task {
            self.producer_logs.insert(key, task.dependency_log());
        }
        let mut result = result;
        loop {
            let completed = match &result {
                Ok(value) => self
                    .rt
                    .complete(key, request.producer(), Completion::Ready(value)),
                Err(error) => {
                    let code = self.rt.root(Primitive::Number(error.code()));
                    self.rt
                        .complete(key, request.producer(), Completion::Failed(&code))
                }
            };
            match completed {
                Ok(woken) => {
                    // Recorded before any woken job runs and reads it.
                    if let Err(error) = &result {
                        self.failures.insert(key, error.clone());
                    }
                    self.events.push(SimEvent::Settled {
                        key,
                        ok: result.is_ok(),
                    });
                    self.wake(&woken);
                    return;
                }
                Err(CompleteError::NotShareable(e)) if result.is_ok() => {
                    result = Err(SimError::ProducerFailed {
                        key,
                        message: format!("result is not shareable: {e}"),
                    });
                }
                Err(e) => {
                    // Completed by someone else, or a host bug: the cell is
                    // not ours to settle. Keep the reason for diagnostics.
                    self.failures.entry(key).or_insert(SimError::Internal {
                        message: format!("cannot settle: {e}"),
                    });
                    return;
                }
            }
        }
    }

    fn run_job(&mut self, job: Job) {
        let Job { task, work } = job;
        match work {
            Work::Produce {
                request,
                producer,
                inputs,
            } => self.run_produce(task, request, producer, inputs),
            Work::Need {
                answer,
                definition,
                kind,
            } => match self.need_for(&task, definition, kind) {
                Got::Value(value) => self.finish_answer(answer, &task, Ok(value)),
                Got::Failed(key) => {
                    let error = self
                        .failures
                        .get(&key)
                        .cloned()
                        .unwrap_or(SimError::Failed { key });
                    self.finish_answer(answer, &task, Err(error));
                }
                Got::Cancelled(key) => {
                    self.finish_answer(answer, &task, Err(SimError::Cancelled { key }))
                }
                Got::Error(error) => self.finish_answer(answer, &task, Err(error)),
                Got::Pending(waiting, produce) => {
                    let work = Work::Need {
                        answer,
                        definition,
                        kind,
                    };
                    self.park(Job { task, work }, waiting, produce);
                }
            },
            Work::Call {
                sink,
                callee,
                arg,
                activations,
            } => self.run_call(task, sink, callee, arg, activations),
        }
    }

    fn run_produce(
        &mut self,
        task: Task,
        request: ProducerRequest,
        producer: Producer,
        mut inputs: Vec<Rooted>,
    ) {
        let key = request.key();
        while let Some(&(definition, kind)) = producer.needs.get(inputs.len()) {
            match self.need_for(&task, definition, kind) {
                Got::Value(value) => inputs.push(value),
                Got::Pending(waiting, produce) => {
                    let work = Work::Produce {
                        request,
                        producer,
                        inputs,
                    };
                    return self.park(Job { task, work }, waiting, produce);
                }
                Got::Failed(input) => {
                    let error = SimError::InputFailed { key, input };
                    return self.settle(request, Err(error), Some(&task));
                }
                Got::Cancelled(input) => {
                    let error = SimError::InputCancelled { key, input };
                    return self.settle(request, Err(error), Some(&task));
                }
                Got::Error(error) => return self.settle(request, Err(error), Some(&task)),
            }
        }
        match producer.body {
            Body::Rust(work) => {
                let result = work(&self.rt, &inputs)
                    .map_err(|message| SimError::ProducerFailed { key, message });
                self.settle(request, result, Some(&task));
            }
            Body::Callable {
                function,
                tail_callee,
            } => {
                let token = self.publish_callable(function, tail_callee);
                self.settle(request, token, Some(&task));
            }
            Body::Invoke { callee, arg } => {
                let arg = self.rt.root(arg);
                self.run_call(task, Sink::Cell(request), callee, arg, 0);
            }
        }
    }

    /// Register a callable and return its token: a fresh persistent vector
    /// `[index]`, so every published callable has its own identity.
    fn publish_callable(
        &mut self,
        function: Executable,
        tail_callee: Option<DependencyId>,
    ) -> Result<Rooted, SimError> {
        let index = self.callables.len();
        let token = self
            .rt
            .scope(|s| {
                let n = s.primitive(Primitive::Number(index as f64));
                s.new_vector(&[n]).map(|v| s.persist(v))
            })
            .map_err(|e| SimError::Internal {
                message: format!("cannot allocate a callable token: {e}"),
            })?;
        self.callables.push(CallableEntry {
            token: token.clone(),
            function,
            tail_callee,
        });
        Ok(token)
    }

    /// The callable a token names: its index must be in range and the entry
    /// must hold the very same token object.
    fn callable_of(&self, token: &Rooted) -> Option<(Executable, Option<DependencyId>)> {
        let index = self.rt.scope(|s| {
            let token = s.local(token).ok()?;
            let index = s.nth(token, 0).ok()?;
            match s.view(index) {
                ValueView::Number(n) if n >= 0.0 && n.fract() == 0.0 => Some(n as usize),
                _ => None,
            }
        })?;
        let entry = self.callables.get(index)?;
        entry
            .token
            .same_object(token)
            .then(|| (entry.function.clone(), entry.tail_callee))
    }

    /// Run call activations for job task `task` until the call returns,
    /// fails, or needs a pending callable (module docs, "Calls").
    fn run_call(
        &mut self,
        task: Task,
        sink: Sink,
        mut callee: DependencyId,
        mut arg: Rooted,
        mut activations: usize,
    ) {
        let result = loop {
            let callable = DependencyKey::new(task.world(), callee, CALLABLE);
            let token = match self.need_for(&task, callee, CALLABLE) {
                Got::Value(token) => token,
                Got::Pending(waiting, produce) => {
                    let work = Work::Call {
                        sink,
                        callee,
                        arg,
                        activations,
                    };
                    return self.park(Job { task, work }, waiting, produce);
                }
                Got::Failed(input) => break self.input_error(&sink, input, false),
                Got::Cancelled(input) => break self.input_error(&sink, input, true),
                Got::Error(error) => break Err(error),
            };
            let Some((function, tail_callee)) = self.callable_of(&token) else {
                break Err(SimError::NotCallable { callable });
            };
            if activations >= self.activation_limit {
                break Err(SimError::ActivationLimit {
                    callable,
                    limit: self.activation_limit,
                });
            }
            activations += 1;
            let activation = match self.rt.spawn_in(task.world(), &function, &[&arg]) {
                Ok(activation) => activation,
                Err(e) => {
                    break Err(SimError::Internal {
                        message: format!("cannot spawn an activation: {e}"),
                    });
                }
            };
            self.events.push(SimEvent::Activation {
                callable,
                task: activation.id(),
            });
            let value = match activation.run() {
                Ok(RunOutcome::Complete(value)) => value,
                Ok(RunOutcome::Failed(e)) => {
                    break Err(SimError::ActivationFailed {
                        callable,
                        message: e.to_string(),
                    });
                }
                Ok(other) => {
                    break Err(SimError::ActivationFailed {
                        callable,
                        message: format!("unexpected outcome {other:?}"),
                    });
                }
                Err(e) => {
                    break Err(SimError::Internal {
                        message: format!("cannot run an activation: {e}"),
                    });
                }
            };
            match (tail_callee, value.view()) {
                (Some(next), ValueView::Number(_)) => {
                    callee = next;
                    arg = value;
                }
                _ => break Ok(value),
            }
        };
        match sink {
            Sink::Cell(request) => self.settle(request, result, Some(&task)),
            Sink::Answer(answer) => self.finish_answer(answer, &task, result),
        }
    }

    /// The error of a call whose callable cell `input` failed or was
    /// cancelled: for a producer, wrapped as its input; for a top-level
    /// request, the input's own error.
    fn input_error(
        &self,
        sink: &Sink,
        input: DependencyKey,
        cancelled: bool,
    ) -> Result<Rooted, SimError> {
        Err(match (sink, cancelled) {
            (Sink::Cell(request), false) => SimError::InputFailed {
                key: request.key(),
                input,
            },
            (Sink::Cell(request), true) => SimError::InputCancelled {
                key: request.key(),
                input,
            },
            (Sink::Answer(_), false) => self
                .failures
                .get(&input)
                .cloned()
                .unwrap_or(SimError::Failed { key: input }),
            (Sink::Answer(_), true) => SimError::Cancelled { key: input },
        })
    }

    fn finish_answer(&mut self, answer: u64, task: &Task, result: Result<Rooted, SimError>) {
        self.answers.insert(answer, (result, task.dependency_log()));
    }
}
