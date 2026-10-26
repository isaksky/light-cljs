//! Native macro tasks on a shared runtime. Mutable registries belong to a
//! task; compiler helper factories cross explicit dependency boundaries.
use crate::{
    CollectionKind as C,
    compiler_data::{Datum, DatumKind as D, ExpansionOrigin, Form, Name},
    compiler_evaluator::{
        CompilerEvaluator, NativeRunner, NativeSubmit, NativeSubmitter, StringService, UnitService,
        WaitService,
    },
    evaluator::{
        EvalError, EvaluatorOptions, ExecutionControl, MacroEvaluator, NEXT_SESSION, ValueId,
    },
    form_bridge::ConversionLimits,
};
use big_boy_pants_js::{
    Budget, Completion, DependencyId, DependencyKey, Executable, FormKind, JsString, Local,
    ObjectIdentity, Primitive, ProducerRequest, Provenance, ResultKind, Rooted, RunOutcome,
    Runtime, Scope, Task, ValueView,
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU32, AtomicUsize, Ordering},
    },
};

fn error(e: impl std::fmt::Display) -> EvalError {
    EvalError(format!("big-boy-pants-js: {e}"))
}
fn string(scope: &Scope<'_>, units: &[u16]) -> Result<JsString, EvalError> {
    let local = scope
        .try_string_units(units)
        .ok_or_else(|| error("heap byte or string limit exceeded"))?;
    scope
        .string(local)
        .ok_or_else(|| error("expected imported string"))
}

pub type BridgeStats = crate::compile_stats::NativeBridgeCall;
#[derive(Default)]
struct Bridge {
    control: Option<ExecutionControl>,
    // Retaining the Datum prevents allocator address reuse from aliasing a key.
    imports: HashMap<usize, (Datum, Rooted)>,
    forms: HashMap<usize, Form>,
    stats: BridgeStats,
    tokens: Vec<usize>,
    environment: Option<(Datum, Rooted)>,
    costs: HashMap<usize, (usize, usize)>,
    // Native metadata is a raw immutable map, so it can emerge without its
    // occurrence wrapper. Map that exact object back without traversing it.
    native_objects: HashMap<ObjectIdentity, usize>,
}
impl Bridge {
    fn poll(&self) -> Result<(), EvalError> {
        self.control.as_ref().map_or(Ok(()), ExecutionControl::poll)
    }
    fn charge(&self, data: &Datum, depth: usize, remaining: &mut usize) -> Result<(), EvalError> {
        let (nodes, height) = self.costs[&data.identity()];
        if depth < height || *remaining < nodes {
            return Err(error("form conversion limit exceeded"));
        }
        *remaining -= nodes;
        Ok(())
    }
    fn import<'s>(
        &mut self,
        s: &Scope<'s>,
        d: &Datum,
        depth: usize,
        remaining: &mut usize,
    ) -> Result<Local<'s>, EvalError> {
        self.poll()?;
        if let Some((_, value)) = self.imports.get(&d.identity()) {
            self.charge(d, depth, remaining)?;
            return s.local(value).map_err(error);
        }
        if depth == 0 || *remaining == 0 {
            return Err(error("form conversion limit exceeded"));
        }
        *remaining -= 1;
        let mut value = match d.kind() {
            D::Nil => s.primitive(Primitive::Null),
            D::Boolean(v) => s.primitive(Primitive::Bool(*v)),
            D::Integer(v) => s.primitive(Primitive::Number(*v as f64)),
            D::Double(v) => s.primitive(Primitive::Number(*v)),
            D::Ratio {
                numerator,
                denominator,
            } => s.primitive(Primitive::Number(*numerator as f64 / *denominator as f64)),
            D::Character(v) => s.primitive(Primitive::String(string(s, &[*v])?)),
            D::String(v) => s.primitive(Primitive::String(string(s, v)?)),
            D::Symbol(n) | D::Keyword(n) => {
                let ns = n.namespace().map(|units| string(s, units)).transpose()?;
                let name = string(s, n.name())?;
                if matches!(d.kind(), D::Symbol(_)) {
                    s.new_symbol(ns, name)
                } else {
                    s.new_keyword(ns, name)
                }
                .map_err(error)?
            }
            D::Collection { kind, children } => {
                let values = children
                    .iter()
                    .map(|v| self.import(s, v, depth - 1, remaining))
                    .collect::<Result<Vec<_>, _>>()?;
                match kind {
                    C::List | C::AnonymousFn => s.new_list(&values),
                    C::Vector => s.new_form_vector(&values),
                    C::Map => s.new_form_map(
                        &values
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|p| (p[0], p[1]))
                            .collect::<Vec<_>>(),
                    ),
                    C::Set => s.new_form_set(&values),
                }
                .map_err(error)?
            }
            D::CompilerLiteral { .. } => {
                return Err(error("unsupported operation: compiler literal"));
            }
        };
        if let Some(meta) = d.metadata() {
            let m = self.import(s, &meta, depth - 1, remaining)?;
            value = s
                .form_with_meta(value, Some(s.form_value(m)))
                .map_err(error)?;
        }
        // Every input node is an occurrence, including numbers and strings.
        // Returning it preserves exact integer/ratio/character representation.
        self.retain(s, d, value)
    }
    fn retain<'s>(
        &mut self,
        s: &Scope<'s>,
        d: &Datum,
        value: Local<'s>,
    ) -> Result<Local<'s>, EvalError> {
        if let Some(identity) = s.object_identity(value) {
            self.native_objects.insert(identity, d.identity());
        }
        let token = u32::try_from(self.tokens.len()).map_err(error)?;
        self.tokens.push(d.identity());
        let location = self
            .forms
            .get(&d.identity())
            .map(|f| crate::compiler_data::DiagnosticLocation::from_span(&f.span()));
        let value = s
            .new_occurrence(
                value,
                Provenance {
                    source: token,
                    line: location.as_ref().map_or(0, |l| l.line as u32),
                    column: location.as_ref().map_or(0, |l| l.column as u32),
                },
            )
            .map_err(error)?;
        self.imports
            .insert(d.identity(), (d.clone(), s.persist(value)));
        let mut cost = (1usize, 1usize);
        let children = match d.kind() {
            D::Collection { children, .. } => children.as_ref(),
            _ => &[],
        };
        for child in children.iter().cloned().chain(d.metadata()) {
            // An unchanged environment slot can have a freshly constructed
            // scalar keyword key whose native key was already present.
            let (nodes, height) = self.costs.get(&child.identity()).copied().unwrap_or((1, 1));
            cost.0 = cost.0.saturating_add(nodes);
            cost.1 = cost.1.max(height.saturating_add(1));
        }
        self.costs.insert(d.identity(), cost);
        self.stats.imported += 1;
        Ok(value)
    }
    fn environment<'s>(
        &mut self,
        s: &Scope<'s>,
        data: &Datum,
        limits: ConversionLimits,
    ) -> Result<Local<'s>, EvalError> {
        if let Some((_, value)) = self.imports.get(&data.identity()) {
            self.charge(data, limits.depth, &mut limits.nodes.clone())?;
            return s.local(value).map_err(error);
        }
        let mut remaining = limits.nodes;
        let Some((previous, root)) = self.environment.clone() else {
            let value = self.import(s, data, limits.depth, &mut remaining)?;
            self.environment = Some((data.clone(), s.persist(value)));
            return Ok(value);
        };
        let before = previous.entries().map_err(error)?;
        let after = data.entries().map_err(error)?;
        let mut value = s.form_value(s.local(&root).map_err(error)?);
        // The fixed environment envelope has a handful of keyword slots.
        // Namespace and reader tables are memoized Datum subtrees: their
        // contents are neither walked nor shipped on an unchanged slot.
        for (key, _) in &before {
            if !after.iter().any(|(k, _)| k.value_eq(key)) {
                let k = self.import(s, key, limits.depth, &mut remaining)?;
                value = s.form_dissoc(value, k).map_err(error)?;
            }
        }
        for (key, new) in &after {
            if before
                .iter()
                .any(|(k, v)| k.value_eq(key) && v.identity() == new.identity())
            {
                continue;
            }
            let k = self.import(s, key, limits.depth, &mut remaining)?;
            let v = self.import(s, new, limits.depth, &mut remaining)?;
            value = s.form_assoc(value, k, v).map_err(error)?;
        }
        let value = self.retain(s, data, value)?;
        self.charge(data, limits.depth, &mut limits.nodes.clone())?;
        self.environment = Some((data.clone(), s.persist(value)));
        Ok(value)
    }
    fn export<'s>(
        &mut self,
        s: &Scope<'s>,
        v: Local<'s>,
        depth: usize,
        remaining: &mut usize,
    ) -> Result<Datum, EvalError> {
        self.poll()?;
        if let Some(data) = s
            .object_identity(v)
            .and_then(|key| self.native_objects.get(&key))
            .and_then(|key| self.imports.get(key))
            .map(|(data, _)| data)
        {
            self.charge(data, depth, remaining)?;
            self.stats.reused += 1;
            return Ok(data.clone());
        }
        if let Ok(p) = s.provenance(v)
            && let Some((data, root)) = self
                .tokens
                .get(p.source as usize)
                .and_then(|key| self.imports.get(key))
            && s.same_object(v, s.local(root).map_err(error)?)
        {
            self.charge(data, depth, remaining)?;
            self.stats.reused += 1;
            return Ok(data.clone());
        }
        if depth == 0 || *remaining == 0 {
            return Err(error("form conversion limit exceeded"));
        }
        *remaining -= 1;
        let v = s.form_value(v);
        let kind = match s.form_kind(v) {
            Some(FormKind::Keyword | FormKind::Symbol) => {
                let name = s.form_name(v).map_err(error)?;
                let n = Name::from_utf16(
                    name.namespace.as_ref().map(JsString::code_units),
                    name.name.code_units(),
                );
                if s.form_kind(v) == Some(FormKind::Keyword) {
                    D::Keyword(n)
                } else {
                    D::Symbol(n)
                }
            }
            Some(k) => {
                let values = if k == FormKind::Map {
                    s.form_entries(v)
                        .map_err(error)?
                        .into_iter()
                        .flat_map(|(k, v)| [k, v])
                        .collect()
                } else {
                    s.form_items(v).map_err(error)?
                };
                let children = values
                    .into_iter()
                    .map(|x| self.export(s, x, depth - 1, remaining))
                    .collect::<Result<Vec<_>, _>>()?;
                D::Collection {
                    kind: match k {
                        FormKind::List => C::List,
                        FormKind::Vector => C::Vector,
                        FormKind::Set => C::Set,
                        _ => C::Map,
                    },
                    children: children.into(),
                }
            }
            None => match s.view(v) {
                ValueView::Null | ValueView::Undefined => D::Nil,
                ValueView::Bool(b) => D::Boolean(b),
                ValueView::Number(n) => D::Double(n),
                ValueView::String => D::String(s.string(v).unwrap().code_units().into()),
                _ => {
                    return Err(error(
                        "unsupported operation: macro returned a non-form value",
                    ));
                }
            },
        };
        let mut data = Datum::new(kind);
        if let Some(m) = s.form_meta(v) {
            data = data.with_metadata(self.export(s, m, depth - 1, remaining)?);
        }
        self.stats.exported += 1;
        Ok(data)
    }
    fn remember(
        &mut self,
        form: &Form,
        depth: usize,
        remaining: &mut usize,
    ) -> Result<(), EvalError> {
        self.poll()?;
        if self.forms.contains_key(&form.data().identity()) {
            return Ok(());
        }
        if depth == 0 || *remaining == 0 {
            return Err(error("form conversion limit exceeded"));
        }
        *remaining -= 1;
        for f in form.children() {
            self.remember(&f, depth - 1, remaining)?;
        }
        if let Some(f) = form.metadata() {
            self.remember(&f, depth - 1, remaining)?;
        }
        self.forms.insert(form.data().identity(), form.clone());
        Ok(())
    }
}

/// The compilation's heap, immutable boot code and aggregate execution limit.
/// Spawning another evaluator does not create another heap or reset its budget.
#[derive(Clone)]
pub struct BbpRuntime(Arc<SharedRuntime>);
struct SharedRuntime {
    /// The aggregate budget this runtime was built for ([`heap_policy`]).
    memory_limit: usize,
    counters: Arc<NativeCounters>,
    runtime: Runtime,
    executable: Executable,
    bridge: Arc<Mutex<Bridge>>,
    control: ExecutionControl,
    next_command: AtomicU32,
    definitions: Mutex<HashMap<DependencyKey, u64>>,
    generation: Mutex<u64>,
    changed: Condvar,
    graph: Mutex<big_boy_pants_js::DependencyGraph>,
    /// Compiled unit factories by source ([`SHARED_UNIT_BYTES`]): each
    /// distinct source is compiled once for the whole runtime and every
    /// task calls the one published function. `None` records a source
    /// whose factory cannot be shared, which tasks evaluate themselves.
    factories: Mutex<HashMap<String, Arc<std::sync::OnceLock<Option<Rooted>>>>>,
    /// Task contexts that pool slots parked between jobs, for any slot to
    /// borrow ([`BbpEvaluator::park`], [`BbpEvaluator::adopt`]).
    idle: Mutex<Vec<TaskContext>>,
    /// Tasks parked on a dependency whose next step a completion queues on
    /// their pool ([`BbpRuntime::wake`]), by task.
    parked: Mutex<HashMap<big_boy_pants_js::TaskId, Parked>>,
    #[cfg(test)]
    on_wait: Mutex<Option<WaitHook>>,
    #[cfg(test)]
    on_initializer: Mutex<Option<WaitHook>>,
}
/// A task parked on a dependency: its next step, where that step's outcome
/// goes, and the pool that runs it once a completion wakes the task.
struct Parked {
    step: NativeStep,
    reply: std::sync::mpsc::SyncSender<Result<RunOutcome, String>>,
    submit: NativeSubmit,
}
/// Removes a parked task's entry when its owner stops waiting, so that a
/// later wakeup queues nothing for it.
struct Unpark<'a>(&'a SharedRuntime, big_boy_pants_js::TaskId);
impl Drop for Unpark<'_> {
    fn drop(&mut self) {
        self.0.parked.lock().unwrap().remove(&self.1);
    }
}
#[derive(Default)]
struct NativeCounters {
    compiled: AtomicUsize,
    published: AtomicUsize,
    per_task: AtomicUsize,
    waits: AtomicUsize,
    steps: AtomicUsize,
    migrations: AtomicUsize,
    contexts: AtomicUsize,
    units_compiled: AtomicUsize,
    units_shared: AtomicUsize,
    units_evaluated: AtomicUsize,
    lent: AtomicUsize,
    borrowed: AtomicUsize,
    woken: AtomicUsize,
    #[cfg(test)]
    cleanups: AtomicUsize,
}
#[cfg(test)]
type WaitHook = Arc<dyn Fn(&Task, DependencyKey) + Send + Sync>;

/// Unit sources at least this long are compiled once per runtime and
/// shared by its tasks ([`BbpRuntime::factory`]); a shorter one, such as a
/// root registration, is cheaper to evaluate in the task than to call.
const SHARED_UNIT_BYTES: usize = 512;

/// How many registrations a producer task replays per evaluation.
const REPLAY_BLOCK: usize = 64;

/// One unit that evaluates `registrations` in order, each as evaluating it
/// alone would: called with the task's native runtime.
fn replay_source(registrations: &[String]) -> String {
    let mut source = String::from("rt => { ");
    for registration in registrations {
        source.push('(');
        source.push_str(registration);
        source.push_str(")(rt); ");
    }
    source.push_str("return null; }");
    source
}

const BINDING: ResultKind = ResultKind::new(2);
const HELPER: ResultKind = ResultKind::new(3);
const INITIALIZER: ResultKind = ResultKind::new(4);

/// The runtime-wide collection and admission policy for `options`.
///
/// `options.memory_limit` is the compilation's one aggregate budget, shared
/// by every task and worker of the runtime; this is the only place it may
/// reach the engine. It becomes the engine's accounted-byte admission limit
/// (`HeapPolicy::hard_limit_bytes`): the booted realm, the runtime's boot
/// code, every task's objects, strings, forms and saved continuations count
/// against it, and allocation pressure triggers collections before an
/// allocation is denied. It is a managed-heap cap, not a process RSS cap
/// (docs/native-macros.md, "Memory limit").
fn heap_policy(options: &EvaluatorOptions) -> big_boy_pants_js::HeapPolicy {
    big_boy_pants_js::HeapPolicy {
        hard_limit_bytes: Some(options.memory_limit),
        ..Default::default()
    }
}

impl BbpRuntime {
    /// Build a compilation's native runtime. Every runtime-wide limit comes
    /// from `options` here, once: evaluators spawned from the result share
    /// its heap, policy and execution budget and never configure their own.
    pub fn new(options: &EvaluatorOptions) -> Result<Self, EvalError> {
        Self::with_heap_policy(options, heap_policy(options))
    }

    fn with_heap_policy(
        options: &EvaluatorOptions,
        policy: big_boy_pants_js::HeapPolicy,
    ) -> Result<Self, EvalError> {
        let runtime = Runtime::try_with_heap_policy(
            big_boy_pants_js::VmLimits {
                max_stack_values: (options.stack_limit / 16).max(1),
                ..Default::default()
            },
            policy,
        )
        .map_err(|e| match e {
            big_boy_pants_js::RuntimeStartError::HeapLimit => EvalError(format!(
                "macro memory limit of {} bytes cannot hold the native macro runtime's realm \
                 (about 1.2 MB of accounted heap before any macro runs); raise --macro-memory-limit",
                options.memory_limit
            )),
            e => error(e),
        })?;
        let keys = crate::CoreFnId::ALL
            .iter()
            .map(|k| format!("{:?}", k.runtime_key()))
            .collect::<Vec<_>>()
            .join(",");
        let source = include_str!("bbp_runtime.js").replace("/* CORE_KEYS */", &keys);
        let executable = big_boy_pants_js::frontend::compile_function(&format!(
            "function nativeMacroTask(commandKind, beginKind, finishKind) {{ {source} }}"
        ))
        .map_err(error)?;
        let control = ExecutionControl::new(options)?;
        Ok(Self(Arc::new(SharedRuntime {
            memory_limit: options.memory_limit,
            counters: Arc::new(NativeCounters::default()),
            runtime,
            executable,
            bridge: Arc::new(Mutex::new(Bridge {
                control: Some(control.clone()),
                ..Default::default()
            })),
            control,
            next_command: AtomicU32::new(16),
            definitions: Mutex::new(HashMap::new()),
            generation: Mutex::new(0),
            changed: Condvar::new(),
            graph: Mutex::new(big_boy_pants_js::DependencyGraph::new()),
            factories: Mutex::new(HashMap::new()),
            idle: Mutex::new(Vec::new()),
            parked: Mutex::new(HashMap::new()),
            #[cfg(test)]
            on_wait: Mutex::new(None),
            #[cfg(test)]
            on_initializer: Mutex::new(None),
        })))
    }

    /// The aggregate memory budget `new` received; spawned evaluators share it.
    pub fn memory_limit(&self) -> usize {
        self.0.memory_limit
    }
    pub fn evaluator(&self) -> Result<BbpEvaluator, EvalError> {
        BbpEvaluator::in_runtime(self.clone(), 0)
    }
    /// A pool slot's evaluator: on an idle task context another slot parked,
    /// with the state of the view that parked it, or, if none is idle, on a
    /// fresh one.
    pub(crate) fn lent_evaluator(&self) -> Result<(BbpEvaluator, Option<LentState>), EvalError> {
        let evaluator = BbpEvaluator::shell(self.clone(), 0);
        match evaluator.adopt() {
            Some(state) => Ok((evaluator, Some(state))),
            None => {
                evaluator.boot()?;
                Ok((evaluator, None))
            }
        }
    }
    /// Queue on its pool the next step of each task `wakeups` made runnable
    /// that parked there ([`BbpEvaluator::park_until_woken`]); a task its
    /// owner still runs itself is not parked and is left to it.
    fn wake(&self, wakeups: big_boy_pants_js::Wakeups) {
        for task in wakeups.into_runnable() {
            let parked = self.0.parked.lock().unwrap().remove(&task.id());
            if let Some(Parked {
                step,
                reply,
                submit,
            }) = parked
            {
                self.0.counters.woken.fetch_add(1, Ordering::Relaxed);
                submit(step, reply);
            }
        }
    }
    /// The idle task context with the most catalog registrations installed.
    fn take_idle(&self) -> Option<TaskContext> {
        let mut idle = self.0.idle.lock().unwrap();
        let best = (0..idle.len()).max_by_key(|&i| idle[i].lent.borrow().installed)?;
        let cx = idle.swap_remove(best);
        drop(idle);
        self.0.counters.borrowed.fetch_add(1, Ordering::Relaxed);
        Some(cx)
    }
    /// Drop the idle task contexts, once no pool slot will borrow one.
    pub(crate) fn clear_idle(&self) {
        let idle = std::mem::take(&mut *self.0.idle.lock().unwrap());
        drop(idle);
    }
    pub(crate) fn cancel(&self) {
        self.0.control.cancel();
        self.0.changed.notify_all();
    }
    pub fn helper_counts(&self) -> crate::compile_stats::NativeHelperCounts {
        let c = &self.0.counters;
        crate::compile_stats::NativeHelperCounts {
            compiled: c.compiled.load(Ordering::Relaxed),
            published: c.published.load(Ordering::Relaxed),
            per_task: c.per_task.load(Ordering::Relaxed),
            waits: c.waits.load(Ordering::Relaxed),
            steps: c.steps.load(Ordering::Relaxed),
            migrations: c.migrations.load(Ordering::Relaxed),
            contexts: c.contexts.load(Ordering::Relaxed),
            units_compiled: c.units_compiled.load(Ordering::Relaxed),
            units_shared: c.units_shared.load(Ordering::Relaxed),
            units_evaluated: c.units_evaluated.load(Ordering::Relaxed),
            contexts_lent: c.lent.load(Ordering::Relaxed),
            contexts_borrowed: c.borrowed.load(Ordering::Relaxed),
            woken: c.woken.load(Ordering::Relaxed),
        }
    }
    /// The runtime's one published factory for unit `source`, compiled now
    /// if no task has asked for it yet; `None` if it cannot be shared
    /// (a syntax error, or a value that does not publish), in which case
    /// the asking task evaluates the source itself and so reports exactly
    /// what evaluation reports. Concurrent first requests compile it once:
    /// the others wait for that compilation, which runs no task code that
    /// could wait in turn.
    fn factory(&self, source: &str) -> Result<Option<Rooted>, EvalError> {
        let cell = self
            .0
            .factories
            .lock()
            .unwrap()
            .entry(source.to_owned())
            .or_default()
            .clone();
        let mut failure = None;
        let factory = cell.get_or_init(|| match self.compile_factory(source) {
            Ok(factory) => {
                if factory.is_some() {
                    self.0
                        .counters
                        .units_compiled
                        .fetch_add(1, Ordering::Relaxed);
                }
                factory
            }
            Err(error) => {
                failure = Some(error);
                None
            }
        });
        match failure {
            // A cancelled or exhausted build: no task may evaluate it later.
            Some(error) => Err(error),
            None => Ok(factory.clone()),
        }
    }
    fn compile_factory(&self, source: &str) -> Result<Option<Rooted>, EvalError> {
        // As indirect eval does: sloppy global code, named `<eval>` in
        // diagnostics, compiled on a bounded helper stack.
        let text = format!("({source})");
        let compiled = std::thread::Builder::new()
            .name("bbp-unit-compiler".into())
            .stack_size(2 * 1024 * 1024)
            .spawn(move || {
                big_boy_pants_js::frontend::compile_script_with(
                    "<eval>",
                    &text,
                    big_boy_pants_js::frontend::ScriptOptions::SLOPPY,
                )
                .ok()
            })
            .map_err(error)?
            .join()
            .map_err(|_| error("unit compiler panicked"))?;
        let Some(executable) = compiled else {
            return Ok(None);
        };
        let producer = self.0.runtime.spawn(&executable, &[]).map_err(error)?;
        loop {
            self.0.control.poll()?;
            match producer
                .run_with(Budget::instructions(10_000), &mut RejectHost)
                .map_err(error)?
            {
                RunOutcome::Complete(value) => {
                    return Ok(self.0.runtime.publish(&value).is_ok().then_some(value));
                }
                RunOutcome::Yielded => {}
                _ => return Ok(None),
            }
        }
    }
    #[cfg(test)]
    pub(crate) fn on_wait(&self, hook: WaitHook) {
        *self.0.on_wait.lock().unwrap() = Some(hook);
    }
}

/// One task of the runtime with what the compiler keeps about it: its
/// boundaries, value handles and registrations, and, while it waits idle
/// for a pool slot to borrow it ([`BbpRuntime::lent_evaluator`]), the state
/// of the worker view that used it last. It is `Send`, so an idle context
/// moves between slots; one evaluator on one thread uses it at a time.
pub(crate) struct TaskContext {
    task: Task,
    command_kind: ResultKind,
    begin_kind: ResultKind,
    finish_kind: ResultKind,
    initializers: RefCell<HashMap<DependencyKey, ProducerRequest>>,
    resumable: Cell<bool>,
    per_task: Arc<AtomicUsize>,
    last_thread: Arc<Mutex<std::thread::ThreadId>>,
    registrations: RefCell<Vec<String>>,
    session: u64,
    next: Cell<u64>,
    values: RefCell<HashMap<u64, Rooted>>,
    /// This task's handle on its `globalThis.__lightNativeRuntime`, which
    /// shared unit factories are called with; made on first use.
    native_runtime: Cell<Option<ValueId>>,
    /// Whether a worker view prepared it (its settings and service stubs
    /// evaluated), which a view that borrows it then does not repeat.
    prepared: Cell<bool>,
    /// The borrowing view's state that names this task's values or
    /// registrations, kept here while the context is idle.
    lent: RefCell<LentState>,
}
const _: () = {
    const fn send<T: Send>() {}
    send::<TaskContext>();
};
/// What a worker view knows about the task it borrowed: how many catalog
/// registrations it installed, the source macros it evaluated in it by code
/// identity (`EngineBinding::foreign_macros`) and the reader declarations
/// it sent (`EngineBinding::reader_declared`).
#[derive(Default)]
pub(crate) struct LentState {
    pub(crate) installed: usize,
    pub(crate) foreign_macros: HashMap<usize, ValueId>,
    pub(crate) reader_declared: HashMap<String, (usize, usize)>,
}
/// A compile-time engine on a [`BbpRuntime`]: the services of the thread
/// that uses it, over one task context, which a pool slot's view returns to
/// the runtime between jobs and borrows again ([`Self::park`]).
pub struct BbpEvaluator {
    shared: BbpRuntime,
    runtime: Runtime,
    control: ExecutionControl,
    service_depth: usize,
    service: RefCell<Option<UnitService>>,
    help: RefCell<Option<WaitService>>,
    runner: RefCell<Option<NativeRunner>>,
    submitter: RefCell<Option<NativeSubmitter>>,
    bridge: Arc<Mutex<Bridge>>,
    stats: Cell<BridgeStats>,
    context: RefCell<Option<Rc<TaskContext>>>,
}
impl BbpEvaluator {
    pub fn new(options: EvaluatorOptions) -> Result<Self, EvalError> {
        BbpRuntime::new(&options)?.evaluator()
    }
    fn in_runtime(shared: BbpRuntime, service_depth: usize) -> Result<Self, EvalError> {
        let this = Self::shell(shared, service_depth);
        this.boot()?;
        Ok(this)
    }
    /// An evaluator without a task context yet.
    fn shell(shared: BbpRuntime, service_depth: usize) -> Self {
        Self {
            runtime: shared.0.runtime.clone(),
            control: shared.0.control.clone(),
            bridge: shared.0.bridge.clone(),
            shared,
            service_depth,
            service: RefCell::new(None),
            help: RefCell::new(None),
            runner: RefCell::new(None),
            submitter: RefCell::new(None),
            stats: Cell::new(BridgeStats::default()),
            context: RefCell::new(None),
        }
    }
    /// Give this evaluator a fresh task and run it to its first boundary.
    fn boot(&self) -> Result<(), EvalError> {
        let runtime = &self.runtime;
        let kind = self
            .shared
            .0
            .next_command
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |kind| {
                kind.checked_add(3)
            })
            .map_err(|_| error("native task identity exhausted"))?;
        let argument = runtime.root(Primitive::Number(kind as f64));
        let begin = runtime.root(Primitive::Number((kind + 1) as f64));
        let finish = runtime.root(Primitive::Number((kind + 2) as f64));
        let task = runtime
            .spawn(&self.shared.0.executable, &[&argument, &begin, &finish])
            .map_err(error)?;
        *self.context.borrow_mut() = Some(Rc::new(TaskContext {
            task,
            command_kind: ResultKind::new(kind),
            begin_kind: ResultKind::new(kind + 1),
            finish_kind: ResultKind::new(kind + 2),
            initializers: RefCell::new(HashMap::new()),
            resumable: Cell::new(false),
            per_task: Arc::new(AtomicUsize::new(0)),
            last_thread: Arc::new(Mutex::new(std::thread::current().id())),
            registrations: RefCell::new(Vec::new()),
            session: NEXT_SESSION.fetch_add(1, Ordering::Relaxed),
            next: Cell::new(1),
            values: RefCell::new(HashMap::new()),
            native_runtime: Cell::new(None),
            prepared: Cell::new(false),
            lent: RefCell::new(LentState::default()),
        }));
        self.shared
            .0
            .counters
            .contexts
            .fetch_add(1, Ordering::Relaxed);
        let booted = self.run();
        if booted.is_err() {
            self.context.borrow_mut().take();
        }
        booted
    }
    /// The task context this evaluator runs on.
    fn cx(&self) -> Result<Rc<TaskContext>, EvalError> {
        self.context
            .borrow()
            .clone()
            .ok_or_else(|| error("the evaluator has no task context"))
    }
    /// Whether it has a task context: a pool slot's evaluator has none
    /// between jobs once it parked its own ([`Self::park`]).
    pub(crate) fn has_context(&self) -> bool {
        self.context.borrow().is_some()
    }
    /// Whether its task context was prepared by a worker view
    /// ([`TaskContext::prepared`]): a view that borrows it does not
    /// evaluate its settings and services again.
    pub(crate) fn prepared(&self) -> bool {
        self.context
            .borrow()
            .as_ref()
            .is_some_and(|cx| cx.prepared.get())
    }
    /// Return the task context to the runtime's idle contexts with the state
    /// of the view that used it, for any pool slot to borrow
    /// ([`Self::adopt`]). The evaluator has no context afterwards. A context
    /// still referenced or mid-initialization is dropped instead; false then.
    pub(crate) fn park(&self, state: LentState) -> bool {
        let Some(cx) = self.context.borrow_mut().take() else {
            return false;
        };
        let Ok(cx) = Rc::try_unwrap(cx) else {
            return false;
        };
        if !cx.initializers.borrow().is_empty() || self.control.check().is_err() {
            return false;
        }
        cx.prepared.set(true);
        *cx.lent.borrow_mut() = state;
        self.shared.0.idle.lock().unwrap().push(cx);
        self.shared.0.counters.lent.fetch_add(1, Ordering::Relaxed);
        true
    }
    /// Borrow an idle task context of the runtime, the one with the most
    /// catalog registrations installed, if this evaluator has none and one
    /// is idle: the state of the view that parked it, for this one to take.
    pub(crate) fn adopt(&self) -> Option<LentState> {
        if self.has_context() {
            return None;
        }
        let cx = self.shared.take_idle()?;
        let state = std::mem::take(&mut *cx.lent.borrow_mut());
        *self.context.borrow_mut() = Some(Rc::new(cx));
        Some(state)
    }
    pub fn bridge_stats(&self) -> BridgeStats {
        let mut stats = self.stats.get();
        if let Some(cx) = self.context.borrow().as_ref() {
            stats.helper_per_task += cx.per_task.load(Ordering::Relaxed);
        }
        // Collections are runtime-wide: every worker's tasks share this heap.
        let gc = self.runtime.heap_stats();
        BridgeStats {
            collections: gc.collections,
            gc_pause_nanos: gc.pause_total.as_nanos(),
            gc_pause_max_nanos: gc.pause_max.as_nanos(),
            gc_live_objects: gc.last_live_objects,
            gc_managed_bytes: gc.managed_bytes,
            gc_peak_managed_bytes: gc.peak_managed_bytes,
            gc_live_bytes: gc.last_live_bytes,
            gc_admitted_bytes: gc.admitted_bytes_total,
            gc_byte_limit: gc.byte_limit.unwrap_or(0),
            ..stats
        }
    }
    fn note(&self, update: impl FnOnce(&mut BridgeStats)) {
        let mut stats = self.stats.get();
        let before = stats;
        update(&mut stats);
        let c = &self.shared.0.counters;
        for (counter, delta) in [
            (&c.compiled, stats.helper_compiled - before.helper_compiled),
            (
                &c.published,
                stats.helper_published - before.helper_published,
            ),
            (&c.per_task, stats.helper_per_task - before.helper_per_task),
            (&c.waits, stats.helper_waits - before.helper_waits),
        ] {
            if delta != 0 {
                counter.fetch_add(delta, Ordering::Relaxed);
            }
        }
        self.stats.set(stats);
    }
    fn with_bridge<T>(&self, work: impl FnOnce(&mut Bridge) -> T) -> T {
        let mut bridge = self.bridge.lock().unwrap();
        let before = bridge.stats;
        let result = work(&mut bridge);
        let after = bridge.stats;
        self.note(|s| {
            s.imported += after.imported - before.imported;
            s.exported += after.exported - before.exported;
            s.reused += after.reused - before.reused;
        });
        result
    }
    pub(crate) fn preload(&self, name: &str, source: &str) -> Result<(), EvalError> {
        let value = self.evaluate_unit(source)?;
        let escaped = name
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r");
        let install = self.evaluate_unit(&format!(
            "rt => f => {{
                Object.defineProperty(globalThis.__lightCompilerRoots, \"{escaped}\",
                    {{value:f,writable:true,configurable:true}});
                return null;
            }}"
        ))?;
        let result = self.call(install, &[value]);
        let _ = self.release(value);
        let _ = self.release(install);
        self.release(result?)
    }
    fn run(&self) -> Result<(), EvalError> {
        let cx = self.cx()?;
        let result = self.run_inner(&cx);
        if let Err(failure) = &result {
            self.shared.0.graph.lock().unwrap().withdraw(cx.task.id());
            self.fail_initializers(&cx, &failure.0);
            let _cancel = cx.task.cancel();
            // The VM bounds cancellation cleanup independently of the spent
            // aggregate fuel. Finally blocks finish on saved task frames.
            while matches!(
                cx.task.run_with(
                    Budget::instructions(10_000),
                    &mut NativeHost(&cx.per_task, &self.shared.0.counters)
                ),
                Ok(RunOutcome::Yielded)
            ) {}
        }
        result
    }
    fn step(&self, cx: &TaskContext) -> NativeStep {
        NativeStep {
            task: cx.task.clone(),
            per_task: cx.per_task.clone(),
            counters: self.shared.0.counters.clone(),
            last_thread: cx.last_thread.clone(),
            admitted: Arc::new(Mutex::new(true)),
        }
    }
    fn run_inner(&self, cx: &TaskContext) -> Result<(), EvalError> {
        // The step a pool worker already ran after a wakeup.
        let mut woken = None;
        loop {
            self.control.poll()?;
            let outcome = match woken.take() {
                Some(outcome) => outcome,
                None => {
                    let step = self.step(cx);
                    // Before the first helper wait, the worker already owns
                    // this runnable task. Only suspended continuations need
                    // pool handoff.
                    let runner = cx
                        .resumable
                        .get()
                        .then(|| self.runner.borrow().clone())
                        .flatten();
                    match runner {
                        Some(run) => run(step),
                        None => step.run(),
                    }
                    .map_err(error)?
                }
            };
            {
                let mut graph = self.shared.0.graph.lock().unwrap();
                for key in cx.task.dependency_log().edges() {
                    if [BINDING, HELPER, INITIALIZER].contains(&key.kind()) {
                        graph.record_use(cx.task.id(), *key);
                    }
                }
                graph.withdraw(cx.task.id());
                if let RunOutcome::Waiting(key) = outcome
                    && [HELPER, INITIALIZER].contains(&key.kind())
                {
                    graph
                        .wait(
                            cx.task.id(),
                            key,
                            big_boy_pants_js::DemandSite::new("native compiler helper"),
                        )
                        .map_err(error)?;
                }
            }
            match outcome {
                RunOutcome::Waiting(key) if key.kind() == cx.command_kind => return Ok(()),
                RunOutcome::Waiting(key) => woken = self.demand(cx, key)?,
                RunOutcome::Yielded => {}
                RunOutcome::Failed(e) => return Err(error(e)),
                RunOutcome::Cancelled => return Err(error("macro task cancelled")),
                other => return Err(error(format!("unexpected session outcome: {other:?}"))),
            }
        }
    }
    /// Wait for `key`, producing what this task was asked to produce first.
    /// In a pool, a task that waits for another's result is parked: the
    /// completion that wakes it queues its next step on the pool
    /// ([`BbpRuntime::wake`]), whose outcome this returns.
    fn demand(
        &self,
        cx: &TaskContext,
        key: DependencyKey,
    ) -> Result<Option<RunOutcome>, EvalError> {
        if [HELPER, INITIALIZER].contains(&key.kind()) {
            cx.resumable.set(true);
        }
        #[cfg(test)]
        let hook = self.shared.0.on_wait.lock().unwrap().clone();
        #[cfg(test)]
        if let Some(hook) = hook {
            hook(&cx.task, key);
        }
        if key.kind() == HELPER || key.kind() == INITIALIZER {
            self.note(|s| s.helper_waits += 1);
        }
        let shared = &self.shared.0;
        let requests = cx.task.take_producer_requests();
        for request in requests {
            let result = self.produce(cx, &request);
            if let Err(failure) = &result {
                self.fail_initializers(cx, &failure.0);
            }
            let value = match &result {
                Ok(value) => value.clone(),
                Err(error) => self.failure_root(&error.to_string()),
            };
            let completion = match result {
                Ok(_) => Completion::Ready(&value),
                Err(_) => Completion::Failed(&value),
            };
            // Completing wakes saved VM frames; it never executes JS inline.
            shared.graph.lock().unwrap().resolve(request.key());
            let wakeups = self
                .runtime
                .complete(request.key(), request.producer(), completion)
                .map_err(error)?;
            self.shared.wake(wakeups);
            *shared.generation.lock().unwrap() += 1;
            shared.changed.notify_all();
        }
        let runner = cx.resumable.get() && self.runner.borrow().is_some();
        if let Some(submit) = runner
            .then(|| self.submitter.borrow().clone())
            .flatten()
            .and_then(|submitter| submitter())
        {
            return self.park_until_woken(cx, key, submit);
        }
        let mut generation = shared.generation.lock().unwrap();
        while matches!(
            self.runtime.cell_status(key),
            big_boy_pants_js::CellStatus::Pending { .. }
        ) {
            self.control.poll()?;
            if cx.task.cancel_requested() {
                return Err(error("macro task cancelled"));
            }
            if let Some(help) = self.help.borrow().clone() {
                drop(generation);
                let helped = help().map_err(error)?;
                generation = shared.generation.lock().unwrap();
                if helped {
                    continue;
                }
                // Publication could have arrived while this thread helped.
                if !matches!(
                    self.runtime.cell_status(key),
                    big_boy_pants_js::CellStatus::Pending { .. }
                ) {
                    break;
                }
            }
            generation = shared
                .changed
                .wait_timeout(generation, std::time::Duration::from_millis(20))
                .unwrap()
                .0;
        }
        Ok(None)
    }
    /// Park this task until a completion of `key` wakes it, then return the
    /// outcome of the step that woke it, which `submit` queued on the pool
    /// for any worker, this one included while it helps. If `key` completed
    /// before the task was parked, nothing was queued and this returns
    /// `None`: the caller runs the step.
    fn park_until_woken(
        &self,
        cx: &TaskContext,
        key: DependencyKey,
        submit: NativeSubmit,
    ) -> Result<Option<RunOutcome>, EvalError> {
        let shared = &self.shared.0;
        let id = cx.task.id();
        let step = self.step(cx);
        // Until the woken step's outcome arrives, unwinding first takes the
        // task back from a queued or running step (`NativeHandoff`).
        let mut handoff = step.handoff();
        let (reply, outcome) = std::sync::mpsc::sync_channel(1);
        shared.parked.lock().unwrap().insert(
            id,
            Parked {
                step,
                reply,
                submit,
            },
        );
        let _unpark = Unpark(shared, id);
        let pending = |key| {
            matches!(
                self.runtime.cell_status(key),
                big_boy_pants_js::CellStatus::Pending { .. }
            )
        };
        // A completion before the task was parked found nothing to queue.
        if !pending(key) && shared.parked.lock().unwrap().remove(&id).is_some() {
            handoff.finish();
            return Ok(None);
        }
        loop {
            match outcome.try_recv() {
                Ok(result) => {
                    handoff.finish();
                    return result.map(Some).map_err(error);
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return Err(error("native task worker stopped"));
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
            self.control.poll()?;
            if cx.task.cancel_requested() {
                return Err(error("macro task cancelled"));
            }
            if let Some(help) = self.help.borrow().clone()
                && help().map_err(error)?
            {
                continue;
            }
            match outcome.recv_timeout(std::time::Duration::from_millis(20)) {
                Ok(result) => {
                    handoff.finish();
                    return result.map(Some).map_err(error);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(error("native task worker stopped"));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }
    fn produce(&self, cx: &TaskContext, request: &ProducerRequest) -> Result<Rooted, EvalError> {
        let key = request.key();
        if key.kind() == cx.begin_kind || key.kind() == cx.finish_kind {
            return self.initialize(cx, key.kind() == cx.begin_kind);
        }
        if key.kind() != HELPER {
            return Err(error(format!("unsupported compiler result: {key}")));
        }
        if self.service_depth >= 16 {
            return Err(error("compiler helper nesting limit exceeded"));
        }
        let id = *self
            .shared
            .0
            .definitions
            .lock()
            .unwrap()
            .get(&key)
            .ok_or_else(|| error("unregistered compiler helper"))?;
        let callback = self
            .service
            .borrow()
            .clone()
            .ok_or_else(|| error("compiler helper service is not installed"))?;
        // The requesting task is parked. Compiler registration and nested
        // macro expansion use independent mutable state on the same heap.
        let active = Self::in_runtime(self.shared.clone(), self.service_depth + 1)?;
        let producer_cx = active.cx()?;
        self.shared.0.graph.lock().unwrap().bind_producer(
            key,
            request.producer(),
            producer_cx.task.id(),
        );
        *active.service.borrow_mut() = Some(callback.clone());
        *active.help.borrow_mut() = self.help.borrow().clone();
        *active.runner.borrow_mut() = self.runner.borrow().clone();
        *active.submitter.borrow_mut() = self.submitter.borrow().clone();
        let registrations = cx.registrations.borrow().clone();
        // Replayed in fixed blocks from the first registration: a full
        // block's text never changes, so every producer task calls the one
        // factory compiled for it (`evaluate_unit`).
        for block in registrations.chunks(REPLAY_BLOCK) {
            let value = active.evaluate_unit(&replay_source(block))?;
            active.release(value)?;
        }
        *producer_cx.registrations.borrow_mut() = registrations;
        drop(producer_cx);
        let source = callback(&active, id).map_err(error)?;
        // Only the factory expression runs here. Its initializer runs in each
        // consumer's task, preserving task-owned mutable values.
        let executable =
            big_boy_pants_js::frontend::compile_script("compiler helper", &format!("({source})"))
                .map_err(error)?;
        let producer = self.runtime.spawn(&executable, &[]).map_err(error)?;
        loop {
            self.control.poll()?;
            match producer
                .run_with(Budget::instructions(10_000), &mut RejectHost)
                .map_err(error)?
            {
                RunOutcome::Complete(value) => {
                    self.runtime.publish(&value).map_err(error)?;
                    self.note(|s| s.helper_compiled += 1);
                    return Ok(value);
                }
                RunOutcome::Yielded => {}
                other => {
                    return Err(error(format!(
                        "unexpected helper factory outcome: {other:?}"
                    )));
                }
            }
        }
    }
    fn initialize(&self, cx: &TaskContext, begin: bool) -> Result<Rooted, EvalError> {
        let unit = cx
            .task
            .global_value("__lightInitializerUnit")
            .map_err(error)?
            .ok_or_else(|| error("missing initializer identity"))?;
        let ValueView::Number(unit) = unit.view() else {
            return Err(error("invalid initializer identity"));
        };
        let key = DependencyKey::new(
            self.runtime.world(),
            DependencyId::new(unit as u64),
            INITIALIZER,
        );
        if begin {
            let claimed = match self.runtime.claim(key) {
                Ok(request) => {
                    self.shared.0.graph.lock().unwrap().bind_producer(
                        key,
                        request.producer(),
                        cx.task.id(),
                    );
                    cx.initializers.borrow_mut().insert(key, request);
                    #[cfg(test)]
                    {
                        let hook = self.shared.0.on_initializer.lock().unwrap().clone();
                        if let Some(hook) = hook {
                            hook(&cx.task, key);
                        }
                    }
                    true
                }
                Err(_) => false,
            };
            return Ok(self.runtime.root(Primitive::Bool(claimed)));
        }
        let producer = cx
            .initializers
            .borrow()
            .get(&key)
            .ok_or_else(|| error("initializer completed without its producer claim"))?
            .producer();
        let failed = cx
            .task
            .global_value("__lightInitializerFailure")
            .map_err(error)?
            .ok_or_else(|| error("missing initializer outcome"))?;
        let value = cx
            .task
            .global_value("__lightInitializerResult")
            .map_err(error)?
            .ok_or_else(|| error("missing initializer result"))?;
        let failure = failed.string().is_some();
        let mut shared_value = false;
        let outcome = if failure {
            failed
        } else {
            let published = match self.runtime.publish(&value) {
                Ok(()) => true,
                Err(big_boy_pants_js::PublishError::NotShareable { .. }) => false,
                Err(e) => return Err(error(e)),
            };
            shared_value = published;
            self.host_scope(|s| {
                let status = s.primitive(Primitive::Bool(published));
                let value = if published {
                    s.local(&value).map_err(error)?
                } else {
                    s.primitive(Primitive::Null)
                };
                Ok::<_, EvalError>(s.persist(s.new_vector(&[status, value]).map_err(error)?))
            })?
        };
        let completion = if failure {
            Completion::Failed(&outcome)
        } else {
            Completion::Ready(&outcome)
        };
        self.shared.0.graph.lock().unwrap().resolve(key);
        let wakeups = self
            .runtime
            .complete(key, producer, completion)
            .map_err(error)?;
        self.shared.wake(wakeups);
        cx.initializers.borrow_mut().remove(&key);
        if !failure {
            self.note(|s| {
                if shared_value {
                    s.helper_published += 1;
                } else {
                    s.helper_per_task += 1;
                }
            });
        }
        *self.shared.0.generation.lock().unwrap() += 1;
        self.shared.0.changed.notify_all();
        Ok(self.runtime.root(Primitive::Null))
    }
    fn fail_initializers(&self, cx: &TaskContext, message: &str) {
        let requests = std::mem::take(&mut *cx.initializers.borrow_mut());
        if requests.is_empty() {
            return;
        }
        let failure = self.failure_root(message);
        for (key, request) in requests {
            self.shared.0.graph.lock().unwrap().resolve(key);
            if let Ok(wakeups) =
                self.runtime
                    .complete(key, request.producer(), Completion::Failed(&failure))
            {
                self.shared.wake(wakeups);
            }
        }
        *self.shared.0.generation.lock().unwrap() += 1;
        self.shared.0.changed.notify_all();
    }
    /// A host scope on this evaluator's thread, outside any run. After a
    /// byte-limit denial it first collects: the denial's own collection ran
    /// before the denied macro released its data, and only a run's start
    /// collects again (`Runtime::byte_starved`). Inside a run or with
    /// another scope open the request fails at once and the scope proceeds.
    fn host_scope<R>(&self, f: impl for<'s> FnOnce(&Scope<'s>) -> R) -> R {
        if self.runtime.byte_starved() {
            let _ = self.runtime.request_collection();
        }
        self.runtime.scope(f)
    }
    /// The failure value a producer or initializer completes its waiters
    /// with. It is charged to the memory limit when it fits; at the limit
    /// the bounded diagnostic is rooted uncharged instead, because dropping
    /// it would leave the waiters without their outcome.
    fn failure_root(&self, message: &str) -> Rooted {
        self.runtime
            .try_root(Primitive::string(message))
            .unwrap_or_else(|| self.runtime.root(Primitive::string(message)))
    }
    fn id(&self) -> Result<ValueId, EvalError> {
        let cx = self.cx()?;
        let index = cx.next.get();
        cx.next.set(index + 1);
        Ok(ValueId {
            session: cx.session,
            index,
        })
    }
    fn check(&self, id: ValueId) -> Result<(), EvalError> {
        let cx = self.cx()?;
        if id.session != cx.session || !cx.values.borrow().contains_key(&id.index) {
            Err(error("invalid value handle"))
        } else {
            Ok(())
        }
    }
    fn request(&self, op: u32, id: ValueId, input: Rooted) -> Result<(), EvalError> {
        self.control.check()?;
        let cx = self.cx()?;
        // Built before the boundary is taken: a denied message leaves the
        // task waiting at the same boundary for the next request.
        let message = self.host_scope(|s| {
            let op = s.primitive(Primitive::Number(op as f64));
            let id = s.primitive(Primitive::Number(id.index as f64));
            let message = s.new_vector(&[op, id, s.local(&input).map_err(error)?]);
            Ok::<_, EvalError>(s.persist(message.map_err(error)?))
        })?;
        let request = cx
            .task
            .take_producer_requests()
            .pop()
            .ok_or_else(|| error("macro task has no request boundary"))?;
        let _wakeups = self
            .runtime
            .complete(
                request.key(),
                request.producer(),
                Completion::Ready(&message),
            )
            .map_err(error)?;
        self.run()?;
        let failed = cx
            .task
            .global_value("__lightFailure")
            .map_err(error)?
            .ok_or_else(|| error("missing result boundary"))?;
        if let Some(message) = failed.string() {
            return Err(error(message.to_string_lossy()));
        }
        if op != 3 {
            let value = cx
                .task
                .global_value("__lightResult")
                .map_err(error)?
                .ok_or_else(|| error("missing macro result"))?;
            cx.values.borrow_mut().insert(id.index, value);
        }
        Ok(())
    }
    fn evaluate(&self, source: &str) -> Result<ValueId, EvalError> {
        let source = self.host_scope(|s| {
            s.try_primitive(Primitive::string(source))
                .map(|local| s.persist(local))
                .ok_or_else(|| error("heap byte limit exceeded"))
        })?;
        let id = self.id()?;
        self.request(0, id, source)?;
        Ok(id)
    }
    fn import(&self, data: &Datum, limits: ConversionLimits) -> Result<ValueId, EvalError> {
        self.control.poll()?;
        let started = std::time::Instant::now();
        let value = self.host_scope(|s| {
            let v =
                self.with_bridge(|b| b.import(s, data, limits.depth, &mut limits.nodes.clone()))?;
            Ok::<_, EvalError>(s.persist(v))
        })?;
        self.note(|s| s.nanos += started.elapsed().as_nanos());
        let id = self.id()?;
        self.request(1, id, value)?;
        Ok(id)
    }
}

#[cfg(test)]
mod tests;
struct RejectHost;
impl big_boy_pants_js::Host for RejectHost {
    fn effect(&mut self, call: big_boy_pants_js::HostCall) -> Result<Primitive, String> {
        Err(format!("unsupported operation: host effect {}", call.op))
    }
}
/// A bounded, owned VM step. It carries no analyzer or thread-local callback.
pub(crate) struct NativeStep {
    task: Task,
    per_task: Arc<AtomicUsize>,
    counters: Arc<NativeCounters>,
    last_thread: Arc<Mutex<std::thread::ThreadId>>,
    admitted: Arc<Mutex<bool>>,
}
impl NativeStep {
    pub(crate) fn handoff(&self) -> NativeHandoff {
        NativeHandoff {
            task: self.task.clone(),
            admitted: self.admitted.clone(),
            finished: false,
        }
    }
    #[cfg(test)]
    pub(crate) fn task_id(&self) -> big_boy_pants_js::TaskId {
        self.task.id()
    }
    pub(crate) fn run(self) -> Result<RunOutcome, String> {
        // The per-step permit also lets an abandoned wait reclaim ownership.
        // It never excludes another task, and spans only this bounded VM step.
        let admitted = self.admitted.lock().unwrap();
        if !*admitted {
            return Err("native task handoff cancelled".into());
        }
        let current = std::thread::current().id();
        let previous = std::mem::replace(&mut *self.last_thread.lock().unwrap(), current);
        self.counters.steps.fetch_add(1, Ordering::Relaxed);
        if previous != current {
            self.counters.migrations.fetch_add(1, Ordering::Relaxed);
        }
        self.task
            .run_with(
                Budget::instructions(10_000),
                &mut NativeHost(&self.per_task, &self.counters),
            )
            .map_err(|e| e.to_string())
    }
}
/// A waiting analyzer must regain exclusive ownership before unwinding its
/// saved task. A queued step is invalidated; an executing step finishes first.
pub(crate) struct NativeHandoff {
    task: Task,
    admitted: Arc<Mutex<bool>>,
    finished: bool,
}
impl NativeHandoff {
    pub(crate) fn finish(&mut self) {
        self.finished = true;
    }
}
impl Drop for NativeHandoff {
    fn drop(&mut self) {
        if !self.finished {
            *self.admitted.lock().unwrap() = false;
            let _cancel = self.task.cancel();
        }
    }
}
struct NativeHost<'a>(&'a AtomicUsize, &'a NativeCounters);
impl big_boy_pants_js::Host for NativeHost<'_> {
    fn effect(&mut self, call: big_boy_pants_js::HostCall) -> Result<Primitive, String> {
        #[cfg(test)]
        if call.op == 65534 {
            self.1.cleanups.fetch_add(1, Ordering::Relaxed);
            return Ok(Primitive::Null);
        }
        if call.op == 65535 {
            self.0.fetch_add(1, Ordering::Relaxed);
            self.1.per_task.fetch_add(1, Ordering::Relaxed);
            Ok(Primitive::Null)
        } else {
            Err(format!(
                "unsupported operation: native host effect {}",
                call.op
            ))
        }
    }
}
impl MacroEvaluator for BbpEvaluator {
    fn register_compiler_root(
        &self,
        name: &str,
        unit: Option<usize>,
        once: bool,
        location: &str,
        test: Option<usize>,
    ) -> Result<(), EvalError> {
        self.define_root(unit)?;
        let source = crate::evaluator::compiler_root_source(name, unit, once, location, test);
        let value = self.evaluate_unit(&source)?;
        self.release(value)?;
        self.cx()?.registrations.borrow_mut().push(source);
        Ok(())
    }
    /// One evaluation for all of `roots`, as a producer task replays them
    /// ([`replay_source`]): a full catalog block has the same text in every
    /// task, so its factory compiles once for the runtime.
    fn register_compiler_roots(
        &self,
        roots: &[crate::evaluator::CompilerRoot<'_>],
    ) -> Result<(), EvalError> {
        let mut sources = Vec::with_capacity(roots.len());
        for root in roots {
            self.define_root(root.unit)?;
            sources.push(crate::evaluator::compiler_root_source(
                root.name,
                root.unit,
                root.once,
                root.location,
                root.test,
            ));
        }
        let value = self.evaluate_unit(&replay_source(&sources))?;
        self.release(value)?;
        self.cx()?.registrations.borrow_mut().extend(sources);
        Ok(())
    }
    /// A declaration bundle's unit is a helper result like a root's: the
    /// runtime forces it with `__rt.need` when a product or an extension
    /// of a dispatched protocol is read (light-cljs-478). Nested producers
    /// replay the registration with the roots'.
    fn register_compiler_bundle(&self, source: &str, unit: usize) -> Result<(), EvalError> {
        self.define_root(Some(unit))?;
        let value = self.evaluate_unit(source)?;
        self.release(value)?;
        self.cx()?
            .registrations
            .borrow_mut()
            .push(source.to_owned());
        Ok(())
    }
    fn native_bridge_stats(&self) -> Option<BridgeStats> {
        Some(self.bridge_stats())
    }
    fn control(&self) -> ExecutionControl {
        self.control.clone()
    }
    fn heap(&self) -> usize {
        // The task's: a pool view's evaluator changes heaps as it borrows
        // contexts, which [`crate::generated_names`] is told of.
        self.context
            .borrow()
            .as_ref()
            .map_or(0, |cx| cx.session as usize)
    }
    fn evaluate_unit(&self, source: &str) -> Result<ValueId, EvalError> {
        if source.len() >= SHARED_UNIT_BYTES
            && let Some(factory) = self.shared.factory(source)?
        {
            return self.call_factory(factory);
        }
        self.shared
            .0
            .counters
            .units_evaluated
            .fetch_add(1, Ordering::Relaxed);
        self.evaluate(&format!("({source})(globalThis.__lightNativeRuntime)"))
    }
    fn call(&self, function: ValueId, arguments: &[ValueId]) -> Result<ValueId, EvalError> {
        self.check(function)?;
        for a in arguments {
            self.check(*a)?;
        }
        let input = self.host_scope(|s| {
            let values = std::iter::once(function)
                .chain(arguments.iter().copied())
                .map(|v| s.primitive(Primitive::Number(v.index as f64)))
                .collect::<Vec<_>>();
            Ok::<_, EvalError>(s.persist(s.new_vector(&values).map_err(error)?))
        })?;
        let id = self.id()?;
        self.request(2, id, input)?;
        Ok(id)
    }
    fn import_form(&self, form: &Form, limits: ConversionLimits) -> Result<ValueId, EvalError> {
        self.control.check()?;
        let started = std::time::Instant::now();
        self.with_bridge(|b| b.remember(form, limits.depth, &mut limits.nodes.clone()))?;
        self.note(|s| s.nanos += started.elapsed().as_nanos());
        self.import(&form.data(), limits)
    }
    fn import_data(&self, data: &Datum, limits: ConversionLimits) -> Result<ValueId, EvalError> {
        self.import(data, limits)
    }
    fn import_environment(
        &self,
        data: &Datum,
        limits: ConversionLimits,
    ) -> Result<ValueId, EvalError> {
        self.control.poll()?;
        let started = std::time::Instant::now();
        let value = self.host_scope(|s| {
            let v = self.with_bridge(|b| b.environment(s, data, limits))?;
            Ok::<_, EvalError>(s.persist(v))
        })?;
        self.note(|s| s.nanos += started.elapsed().as_nanos());
        let id = self.id()?;
        self.request(1, id, value)?;
        Ok(id)
    }
    fn export_form(
        &self,
        id: ValueId,
        call: &Form,
        origin: ExpansionOrigin,
        limits: ConversionLimits,
    ) -> Result<Form, EvalError> {
        self.check(id)?;
        let started = std::time::Instant::now();
        let v = self.cx()?.values.borrow().get(&id.index).unwrap().clone();
        let data = self.host_scope(|s| {
            self.with_bridge(|b| {
                b.export(
                    s,
                    s.local(&v).map_err(error)?,
                    limits.depth,
                    &mut limits.nodes.clone(),
                )
            })
        })?;
        let result =
            self.with_bridge(|b| Form::from_bridge_expansion(data, call, origin, &b.forms));
        self.note(|s| s.nanos += started.elapsed().as_nanos());
        Ok(result)
    }
    fn release(&self, id: ValueId) -> Result<(), EvalError> {
        self.check(id)?;
        self.request(3, id, self.runtime.root(Primitive::Null))?;
        self.cx()?.values.borrow_mut().remove(&id.index);
        Ok(())
    }
}
impl CompilerEvaluator for BbpEvaluator {
    fn install_wait_service(&self, callback: WaitService) {
        *self.help.borrow_mut() = Some(callback);
    }
    fn install_native_runner(&self, callback: NativeRunner) {
        *self.runner.borrow_mut() = Some(callback);
    }
    fn install_native_submitter(&self, callback: NativeSubmitter) {
        *self.submitter.borrow_mut() = Some(callback);
    }
    fn native_forms(&self) -> bool {
        true
    }
    fn evaluate_js(&self, expression: &str) -> Result<ValueId, EvalError> {
        self.evaluate(expression)
    }
    fn install_compiler_service(
        self: Rc<Self>,
        name: &str,
        callback: UnitService,
    ) -> Result<(), EvalError> {
        if name == "__lightCompileUnit" {
            *self.service.borrow_mut() = Some(callback);
        }
        // Helper demand calls the Rust service from a parked continuation.
        // Preserve explicit rejection of direct legacy JS service calls.
        self.unsupported_service(name)
    }
    fn install_string_service(
        &self,
        name: &str,
        _callback: StringService,
    ) -> Result<(), EvalError> {
        self.unsupported_service(name)
    }
    fn value_roots(&self) -> Result<usize, EvalError> {
        // The task's own runtime handle is not a value its callers hold.
        let held = self.context.borrow().as_ref().map_or(0, |cx| {
            cx.values.borrow().len() - usize::from(cx.native_runtime.get().is_some())
        });
        Ok(held + self.bridge.lock().unwrap().imports.len())
    }
    fn collect_garbage(&self) {
        let _ = self.runtime.collect();
    }
    #[cfg(feature = "bbp-macros")]
    fn native_shell(&self) -> Option<&BbpEvaluator> {
        Some(self)
    }
    #[cfg(all(test, light_v8))]
    fn number(&self, id: ValueId) -> Result<f64, EvalError> {
        self.check(id)?;
        match self.cx()?.values.borrow()[&id.index].view() {
            ValueView::Number(n) => Ok(n),
            _ => Err(error("not a number")),
        }
    }
}
impl BbpEvaluator {
    /// Record root `unit`'s helper definition and complete its binding cell.
    fn define_root(&self, unit: Option<usize>) -> Result<(), EvalError> {
        let Some(unit) = unit else {
            return Ok(());
        };
        let id = DependencyId::new(unit as u64);
        let key = DependencyKey::new(self.runtime.world(), id, HELPER);
        self.shared
            .0
            .definitions
            .lock()
            .unwrap()
            .insert(key, unit as u64);
        let binding = DependencyKey::new(self.runtime.world(), id, BINDING);
        if let Ok(request) = self.runtime.claim(binding) {
            let value = self.runtime.root(Primitive::Null);
            let wakeups = self
                .runtime
                .complete(binding, request.producer(), Completion::Ready(&value))
                .map_err(error)?;
            self.shared.wake(wakeups);
        }
        Ok(())
    }
    /// Evaluate a unit through its shared factory: the factory enters this
    /// task as a value and is called with this task's runtime, as the
    /// evaluated source would have called it.
    fn call_factory(&self, factory: Rooted) -> Result<ValueId, EvalError> {
        let cx = self.cx()?;
        let runtime = match cx.native_runtime.get() {
            Some(id) => id,
            None => {
                let id = self.evaluate("globalThis.__lightNativeRuntime")?;
                cx.native_runtime.set(Some(id));
                id
            }
        };
        drop(cx);
        let function = self.id()?;
        self.request(1, function, factory)?;
        let result = self.call(function, &[runtime]);
        let released = self.release(function);
        let result = result?;
        released?;
        self.shared
            .0
            .counters
            .units_shared
            .fetch_add(1, Ordering::Relaxed);
        Ok(result)
    }
    fn unsupported_service(&self, name: &str) -> Result<(), EvalError> {
        if !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return Err(error("invalid service name"));
        }
        let source = format!(
            "rt => {{ globalThis.{name} = () => {{ throw new Error('unsupported operation: native compiler service {name}'); }}; return null; }}"
        );
        // A borrowed context has its services' stubs from the view that
        // prepared it ([`TaskContext::prepared`]).
        let cx = self.cx()?;
        if cx.prepared.get() && cx.registrations.borrow().contains(&source) {
            return Ok(());
        }
        drop(cx);
        let value = self.evaluate_unit(&source)?;
        self.release(value)?;
        self.cx()?.registrations.borrow_mut().push(source);
        Ok(())
    }
}
