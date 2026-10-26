//! Native macro tasks on a shared runtime. Mutable registries belong to a
//! task; compiler helper factories cross explicit dependency boundaries.
use crate::{
    CollectionKind as C,
    compiler_data::{Datum, DatumKind as D, ExpansionOrigin, Form, Name},
    compiler_evaluator::{
        CompilerEvaluator, NativeRunner, StringService, UnitService, WaitService,
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
    #[cfg(test)]
    on_wait: Mutex<Option<WaitHook>>,
    #[cfg(test)]
    on_initializer: Mutex<Option<WaitHook>>,
}
#[derive(Default)]
struct NativeCounters {
    compiled: AtomicUsize,
    published: AtomicUsize,
    per_task: AtomicUsize,
    waits: AtomicUsize,
    steps: AtomicUsize,
    migrations: AtomicUsize,
    #[cfg(test)]
    cleanups: AtomicUsize,
}
#[cfg(test)]
type WaitHook = Arc<dyn Fn(&Task, DependencyKey) + Send + Sync>;

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
        }
    }
    #[cfg(test)]
    pub(crate) fn on_wait(&self, hook: WaitHook) {
        *self.0.on_wait.lock().unwrap() = Some(hook);
    }
}

pub struct BbpEvaluator {
    shared: BbpRuntime,
    runtime: Runtime,
    task: Task,
    control: ExecutionControl,
    command_kind: ResultKind,
    begin_kind: ResultKind,
    finish_kind: ResultKind,
    initializers: RefCell<HashMap<DependencyKey, ProducerRequest>>,
    service_depth: usize,
    service: RefCell<Option<UnitService>>,
    help: RefCell<Option<WaitService>>,
    runner: RefCell<Option<NativeRunner>>,
    resumable: Cell<bool>,
    per_task: Arc<AtomicUsize>,
    last_thread: Arc<Mutex<std::thread::ThreadId>>,
    registrations: RefCell<Vec<String>>,
    session: u64,
    next: Cell<u64>,
    bridge: Arc<Mutex<Bridge>>,
    stats: Cell<BridgeStats>,
    values: RefCell<HashMap<u64, Rooted>>,
}
impl BbpEvaluator {
    pub fn new(options: EvaluatorOptions) -> Result<Self, EvalError> {
        BbpRuntime::new(&options)?.evaluator()
    }
    fn in_runtime(shared: BbpRuntime, service_depth: usize) -> Result<Self, EvalError> {
        let runtime = shared.0.runtime.clone();
        let kind = shared
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
            .spawn(&shared.0.executable, &[&argument, &begin, &finish])
            .map_err(error)?;
        let control = shared.0.control.clone();
        let bridge = shared.0.bridge.clone();
        let this = Self {
            shared,
            runtime,
            task,
            command_kind: ResultKind::new(kind),
            begin_kind: ResultKind::new(kind + 1),
            finish_kind: ResultKind::new(kind + 2),
            initializers: RefCell::new(HashMap::new()),
            service_depth,
            service: RefCell::new(None),
            help: RefCell::new(None),
            runner: RefCell::new(None),
            resumable: Cell::new(false),
            per_task: Arc::new(AtomicUsize::new(0)),
            last_thread: Arc::new(Mutex::new(std::thread::current().id())),
            registrations: RefCell::new(Vec::new()),
            control: control.clone(),
            session: NEXT_SESSION.fetch_add(1, Ordering::Relaxed),
            next: Cell::new(1),
            bridge,
            stats: Cell::new(BridgeStats::default()),
            values: RefCell::new(HashMap::new()),
        };
        this.run()?;
        Ok(this)
    }
    pub fn bridge_stats(&self) -> BridgeStats {
        let mut stats = self.stats.get();
        stats.helper_per_task += self.per_task.load(Ordering::Relaxed);
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
        let result = self.run_inner();
        if let Err(failure) = &result {
            self.shared.0.graph.lock().unwrap().withdraw(self.task.id());
            self.fail_initializers(&failure.0);
            let _cancel = self.task.cancel();
            // The VM bounds cancellation cleanup independently of the spent
            // aggregate fuel. Finally blocks finish on saved task frames.
            while matches!(
                self.task.run_with(
                    Budget::instructions(10_000),
                    &mut NativeHost(&self.per_task, &self.shared.0.counters)
                ),
                Ok(RunOutcome::Yielded)
            ) {}
        }
        result
    }
    fn run_inner(&self) -> Result<(), EvalError> {
        loop {
            self.control.poll()?;
            let step = NativeStep {
                task: self.task.clone(),
                per_task: self.per_task.clone(),
                counters: self.shared.0.counters.clone(),
                last_thread: self.last_thread.clone(),
                admitted: Arc::new(Mutex::new(true)),
            };
            // Before the first helper wait, the worker already owns this
            // runnable task. Only suspended continuations need pool handoff.
            let runner = self
                .resumable
                .get()
                .then(|| self.runner.borrow().clone())
                .flatten();
            let outcome = match runner {
                Some(run) => run(step),
                None => step.run(),
            }
            .map_err(error)?;
            {
                let mut graph = self.shared.0.graph.lock().unwrap();
                for key in self.task.dependency_log().edges() {
                    if [BINDING, HELPER, INITIALIZER].contains(&key.kind()) {
                        graph.record_use(self.task.id(), *key);
                    }
                }
                graph.withdraw(self.task.id());
                if let RunOutcome::Waiting(key) = outcome
                    && [HELPER, INITIALIZER].contains(&key.kind())
                {
                    graph
                        .wait(
                            self.task.id(),
                            key,
                            big_boy_pants_js::DemandSite::new("native compiler helper"),
                        )
                        .map_err(error)?;
                }
            }
            match outcome {
                RunOutcome::Waiting(key) if key.kind() == self.command_kind => return Ok(()),
                RunOutcome::Waiting(key) => self.demand(key)?,
                RunOutcome::Yielded => {}
                RunOutcome::Failed(e) => return Err(error(e)),
                RunOutcome::Cancelled => return Err(error("macro task cancelled")),
                other => return Err(error(format!("unexpected session outcome: {other:?}"))),
            }
        }
    }
    fn demand(&self, key: DependencyKey) -> Result<(), EvalError> {
        if [HELPER, INITIALIZER].contains(&key.kind()) {
            self.resumable.set(true);
        }
        #[cfg(test)]
        let hook = self.shared.0.on_wait.lock().unwrap().clone();
        #[cfg(test)]
        if let Some(hook) = hook {
            hook(&self.task, key);
        }
        if key.kind() == HELPER || key.kind() == INITIALIZER {
            self.note(|s| s.helper_waits += 1);
        }
        let shared = &self.shared.0;
        let requests = self.task.take_producer_requests();
        for request in requests {
            let result = self.produce(&request);
            if let Err(failure) = &result {
                self.fail_initializers(&failure.0);
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
            let _runnable = self
                .runtime
                .complete(request.key(), request.producer(), completion)
                .map_err(error)?
                .into_runnable();
            *shared.generation.lock().unwrap() += 1;
            shared.changed.notify_all();
        }
        let mut generation = shared.generation.lock().unwrap();
        while matches!(
            self.runtime.cell_status(key),
            big_boy_pants_js::CellStatus::Pending { .. }
        ) {
            self.control.poll()?;
            if self.task.cancel_requested() {
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
        Ok(())
    }
    fn produce(&self, request: &ProducerRequest) -> Result<Rooted, EvalError> {
        let key = request.key();
        if key.kind() == self.begin_kind || key.kind() == self.finish_kind {
            return self.initialize(key.kind() == self.begin_kind);
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
        self.shared.0.graph.lock().unwrap().bind_producer(
            key,
            request.producer(),
            active.task.id(),
        );
        *active.service.borrow_mut() = Some(callback.clone());
        *active.help.borrow_mut() = self.help.borrow().clone();
        *active.runner.borrow_mut() = self.runner.borrow().clone();
        let registrations = self.registrations.borrow().clone();
        for source in &registrations {
            let value = active.evaluate_unit(source)?;
            active.release(value)?;
        }
        *active.registrations.borrow_mut() = registrations;
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
    fn initialize(&self, begin: bool) -> Result<Rooted, EvalError> {
        let unit = self
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
                        self.task.id(),
                    );
                    self.initializers.borrow_mut().insert(key, request);
                    #[cfg(test)]
                    {
                        let hook = self.shared.0.on_initializer.lock().unwrap().clone();
                        if let Some(hook) = hook {
                            hook(&self.task, key);
                        }
                    }
                    true
                }
                Err(_) => false,
            };
            return Ok(self.runtime.root(Primitive::Bool(claimed)));
        }
        let producer = self
            .initializers
            .borrow()
            .get(&key)
            .ok_or_else(|| error("initializer completed without its producer claim"))?
            .producer();
        let failed = self
            .task
            .global_value("__lightInitializerFailure")
            .map_err(error)?
            .ok_or_else(|| error("missing initializer outcome"))?;
        let value = self
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
        let _runnable = self
            .runtime
            .complete(key, producer, completion)
            .map_err(error)?
            .into_runnable();
        self.initializers.borrow_mut().remove(&key);
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
    fn fail_initializers(&self, message: &str) {
        let requests = std::mem::take(&mut *self.initializers.borrow_mut());
        if requests.is_empty() {
            return;
        }
        let failure = self.failure_root(message);
        for (key, request) in requests {
            self.shared.0.graph.lock().unwrap().resolve(key);
            let _wakeups =
                self.runtime
                    .complete(key, request.producer(), Completion::Failed(&failure));
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
    fn id(&self) -> ValueId {
        let index = self.next.get();
        self.next.set(index + 1);
        ValueId {
            session: self.session,
            index,
        }
    }
    fn check(&self, id: ValueId) -> Result<(), EvalError> {
        if id.session != self.session || !self.values.borrow().contains_key(&id.index) {
            Err(error("invalid value handle"))
        } else {
            Ok(())
        }
    }
    fn request(&self, op: u32, id: ValueId, input: Rooted) -> Result<(), EvalError> {
        self.control.check()?;
        // Built before the boundary is taken: a denied message leaves the
        // task waiting at the same boundary for the next request.
        let message = self.host_scope(|s| {
            let op = s.primitive(Primitive::Number(op as f64));
            let id = s.primitive(Primitive::Number(id.index as f64));
            let message = s.new_vector(&[op, id, s.local(&input).map_err(error)?]);
            Ok::<_, EvalError>(s.persist(message.map_err(error)?))
        })?;
        let request = self
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
        let failed = self
            .task
            .global_value("__lightFailure")
            .map_err(error)?
            .ok_or_else(|| error("missing result boundary"))?;
        if let Some(message) = failed.string() {
            return Err(error(message.to_string_lossy()));
        }
        if op != 3 {
            let value = self
                .task
                .global_value("__lightResult")
                .map_err(error)?
                .ok_or_else(|| error("missing macro result"))?;
            self.values.borrow_mut().insert(id.index, value);
        }
        Ok(())
    }
    fn evaluate(&self, source: &str) -> Result<ValueId, EvalError> {
        let source = self.host_scope(|s| {
            s.try_primitive(Primitive::string(source))
                .map(|local| s.persist(local))
                .ok_or_else(|| error("heap byte limit exceeded"))
        })?;
        let id = self.id();
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
        let id = self.id();
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
        if let Some(unit) = unit {
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
                let _wakeups = self
                    .runtime
                    .complete(binding, request.producer(), Completion::Ready(&value))
                    .map_err(error)?;
            }
        }
        let source = crate::evaluator::compiler_root_source(name, unit, once, location, test);
        let value = self.evaluate_unit(&source)?;
        self.release(value)?;
        self.registrations.borrow_mut().push(source);
        Ok(())
    }
    fn native_bridge_stats(&self) -> Option<BridgeStats> {
        Some(self.bridge_stats())
    }
    fn control(&self) -> ExecutionControl {
        self.control.clone()
    }
    fn heap(&self) -> usize {
        self.session as usize
    }
    fn evaluate_unit(&self, source: &str) -> Result<ValueId, EvalError> {
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
        let id = self.id();
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
        let id = self.id();
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
        let data = self.host_scope(|s| {
            let v = self.values.borrow().get(&id.index).unwrap().clone();
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
        self.values.borrow_mut().remove(&id.index);
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
        Ok(self.values.borrow().len() + self.bridge.lock().unwrap().imports.len())
    }
    fn collect_garbage(&self) {
        let _ = self.runtime.collect();
    }
    #[cfg(all(test, light_v8))]
    fn number(&self, id: ValueId) -> Result<f64, EvalError> {
        self.check(id)?;
        match self.values.borrow()[&id.index].view() {
            ValueView::Number(n) => Ok(n),
            _ => Err(error("not a number")),
        }
    }
}
impl BbpEvaluator {
    fn unsupported_service(&self, name: &str) -> Result<(), EvalError> {
        if !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return Err(error("invalid service name"));
        }
        let source = format!(
            "rt => {{ globalThis.{name} = () => {{ throw new Error('unsupported operation: native compiler service {name}'); }}; return null; }}"
        );
        let value = self.evaluate_unit(&source)?;
        self.release(value)?;
        self.registrations.borrow_mut().push(source);
        Ok(())
    }
}
