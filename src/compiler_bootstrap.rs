//! First-party bootstrap compiled by the same Rust analyzer/emitter as applications.
//! Only immutable bootstrap artifacts are cached; every compilation has a fresh JS heap.
#[cfg(test)]
use crate::evaluator::Evaluator;
use crate::{
    MacroHost,
    compiler_data::{DataBuilder, Datum, ExpansionOrigin, Form, Name, Source},
    evaluator::ValueId,
    form_bridge::ConversionLimits,
};
use std::{collections::HashMap, rc::Rc};

const CORE: &str = include_str!("compiler_core_macros.cljs");
const SEQUENCES: &str = include_str!("compiler_sequence_macros.cljs");
const MACROS: &[(&str, &str)] = &[
    ("defmulti", "defmulti"),
    ("defmethod", "defmethod"),
    ("and", "and"),
    ("or", "or"),
    ("js-fn?", "js-fn?"),
    ("if-not", "if-not"),
    ("when", "when"),
    ("when-not", "when-not"),
    ("when-first", "when-first"),
    ("if-let", "if-let"),
    ("when-let", "when-let"),
    ("if-some", "if-some"),
    ("when-some", "when-some"),
    ("cond", "cond"),
    ("condp", "condp"),
    ("case", "case"),
    ("->", "thread-first"),
    ("->>", "thread-last"),
    ("as->", "as-thread"),
    ("cond->", "cond-thread-first"),
    ("cond->>", "cond-thread-last"),
    ("some->", "some-thread-first"),
    ("some->>", "some-thread-last"),
    ("while", "while"),
    ("dotimes", "dotimes"),
    ("vswap!", "vswap!"),
    ("doto", "doto"),
    ("js-obj", "js-obj"),
    ("hash-map", "hash-map"),
    ("memfn", "memfn"),
    ("amap", "amap"),
    ("areduce", "areduce"),
    ("..", "dot-dot"),
    ("lazy-cat", "lazy-cat"),
    ("selector", "selector"),
    ("comment", "comment"),
    ("with-in-str", "with-in-str"),
    ("js-debugger", "js-debugger"),
    ("js-comment", "js-comment"),
    ("locking", "locking"),
    ("time", "time"),
    ("simple-benchmark", "simple-benchmark"),
    ("unchecked-max", "unchecked-max"),
    ("unchecked-min", "unchecked-min"),
];
#[derive(Clone)]
pub(crate) struct Bootstrap {
    pub(crate) evaluator: Rc<dyn crate::compiler_evaluator::CompilerEvaluator>,
    functions: HashMap<String, CompiledMacro>,
    /// The source macro functions evaluated in `evaluator` whose last
    /// declaration is gone, to release there ([`Self::release_dropped`]).
    dropped: DroppedValues,
}
/// Values whose owners ([`MacroValue`]) were dropped, on whatever thread,
/// for their engine to release between operations.
type DroppedValues = std::sync::Arc<std::sync::Mutex<Vec<ValueId>>>;
impl Bootstrap {
    /// An owner of `function`, evaluated in `engine`, that has this
    /// bootstrap's engine release it once the last declaration holding it is
    /// dropped. `engine` may be the bootstrap's evaluator or a handle to its
    /// heap active in a callback, such as a deferred unit's analysis; a
    /// function of another engine is never released, as before.
    fn own(&self, engine: &dyn crate::evaluator::MacroEvaluator, function: ValueId) -> MacroValue {
        let same = std::ptr::addr_eq(
            engine as *const dyn crate::evaluator::MacroEvaluator,
            Rc::as_ptr(&self.evaluator),
        ) || (engine.heap() != 0 && engine.heap() == self.evaluator.heap());
        MacroValue {
            id: function,
            dropped: same.then(|| self.dropped.clone()),
        }
    }
    /// Release in the engine the macro functions no declaration holds any
    /// more. Outside the engine only: between operations, or where a
    /// declaration or expansion is about to enter it.
    pub(crate) fn release_dropped(&self) {
        let dropped = std::mem::take(&mut *self.dropped.lock().unwrap_or_else(|e| e.into_inner()));
        for function in dropped {
            // A cancelled or failed engine refuses; its values go with it.
            let _ = self.evaluator.release(function);
        }
    }
}
/// A macro's compile-time function in the engine that evaluated it, which
/// that engine releases once every declaration holding it, in any frame or
/// layer of the tables, has been dropped (light-cljs-381). A redefinition
/// replaces the declaration, so a long watch or REPL session roots one
/// function per live macro, not one per definition.
struct MacroValue {
    id: ValueId,
    dropped: Option<DroppedValues>,
}
impl Drop for MacroValue {
    fn drop(&mut self) {
        if let Some(dropped) = &self.dropped {
            dropped
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(self.id);
        }
    }
}

#[derive(Clone)]
struct BootstrapUnit {
    source: String,
    environment_reads: HashMap<String, bool>,
}
impl std::ops::Deref for BootstrapUnit {
    type Target = str;
    fn deref(&self) -> &str {
        &self.source
    }
}
// Only bootstrap definitions use this proof. The compiler supplies their first
// two parameters; inspect resolved local IDs, including nested captures, so
// shadowing and quoted symbols do not look like reads of the hidden &env.
fn reads_macro_environment(program: &crate::Program, root: crate::ExprId) -> bool {
    use crate::ExprKind;
    let ExprKind::Fn { methods, .. } = &program.exprs[root.0].kind else {
        return true;
    };
    let mut environments = Vec::new();
    for method in methods {
        let Some(environment) = method.params.get(1) else {
            return true;
        };
        environments.push(*environment);
    }
    let mut pending: Vec<_> = methods.iter().map(|method| method.body).collect();
    let mut seen = std::collections::HashSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        match &program.exprs[id.0].kind {
            ExprKind::Local(local) if environments.contains(local) => return true,
            // Raw JavaScript and argument reflection can observe otherwise
            // unused formals. Stay conservative about type/method captures.
            ExprKind::JsArguments
            | ExprKind::CopyArguments(_)
            | ExprKind::JsTemplate { .. }
            | ExprKind::Reify { .. }
            | ExprKind::DefType { .. }
            | ExprKind::ExtendType { .. } => return true,
            ExprKind::JsGlobal(global) if program.js_globals[global.0].path[0] == "arguments" => {
                return true;
            }
            ExprKind::Fn { methods, .. }
                if methods
                    .iter()
                    .any(|m| m.captures.iter().any(|local| environments.contains(local))) =>
            {
                return true;
            }
            _ => {}
        }
        pending.extend(crate::program_fragment::children(program, id));
    }
    false
}
// Skipping an unread formal must not hide a conversion-limit failure or a
// literal constructor's behavior. For plain immutable data, the canonical JS
// snapshot visits at most twice the input nodes (list traversal adds a step
// per cell). If that bound does not fit, retain the original import path.
fn elidable_environment(
    data: &Datum,
    remaining: &mut usize,
    depth: usize,
    control: &crate::evaluator::ExecutionControl,
) -> bool {
    if depth == 0 || *remaining == 0 {
        return false;
    }
    if *remaining & 1023 == 0 && control.poll().is_err() {
        return false;
    }
    *remaining -= 1;
    if let crate::compiler_data::DatumKind::CompilerLiteral { .. } = data.kind() {
        return false;
    }
    if let Some(metadata) = data.metadata()
        && !elidable_environment(&metadata, remaining, depth - 1, control)
    {
        return false;
    }
    match data.kind() {
        crate::compiler_data::DatumKind::Collection { children, .. } => children
            .iter()
            .all(|child| elidable_environment(child, remaining, depth - 1, control)),
        _ => true,
    }
}

// Generated in memory from these exact include_str! inputs by this binary,
// once per process for every thread: they are read and analyzed in the
// bootstrap's own scope of generated names, so every thread, and every
// compilation before or after another, gets the same text and the same
// suffixes (crate::generated_names::BOOTSTRAP_SCOPE). No disk artifact,
// source hash ambiguity or mutable JS state is retained.
static UNITS: std::sync::OnceLock<[BootstrapUnit; 2]> = std::sync::OnceLock::new();
fn compile(source: &str, host: &mut MacroHost) -> Result<BootstrapUnit, String> {
    let syntax = crate::parse(source).map_err(|e| e.to_string())?;
    let program = crate::analyze_with_macros(source, &syntax, host).map_err(|e| {
        let loc = crate::LineIndex::new(source).line_column(e.span.start);
        format!("bootstrap {loc:?}: {e}")
    })?;
    let environment_reads = program
        .exprs
        .iter()
        .filter_map(|expr| {
            if let crate::ExprKind::Def {
                global,
                init: Some(init),
                ..
            } = &expr.kind
            {
                Some((
                    program.globals[global.0].name.clone(),
                    program.macro_functions.contains(init)
                        || reads_macro_environment(&program, *init),
                ))
            } else {
                None
            }
        })
        .collect();
    Ok(BootstrapUnit {
        source: crate::emitter::emit_js_unit(&program),
        environment_reads,
    })
}
fn core_source() -> String {
    let mut source = CORE.to_owned();
    source.push_str("\n{");
    for (name, function) in MACROS {
        source.push_str(&format!("\"{name}\" macro-{function} "));
    }
    source.push('}');
    source
}
fn install(unit: &BootstrapUnit, bootstrap: &mut Bootstrap, names: &[&str]) -> Result<(), String> {
    let (file, text) = if names.first() == Some(&"for") {
        ("compiler_sequence_macros.cljs", SEQUENCES)
    } else {
        ("compiler_core_macros.cljs", CORE)
    };
    let source = Source::new(file, text);
    let syntax = crate::parse(text).map_err(|e| e.to_string())?;
    let mut definitions = HashMap::new();
    for root in &syntax.roots {
        if let crate::NodeKind::Collection { children, .. } = &syntax.nodes[root.0].kind
            && children.len() > 1
            && &text[syntax.nodes[children[0].0].span.clone()] == "defmacro"
        {
            definitions.insert(
                text[syntax.nodes[children[1].0].span.clone()].to_owned(),
                source.span(syntax.nodes[root.0].span.clone()).unwrap(),
            );
        }
    }
    let e = &bootstrap.evaluator;
    let values = e.evaluate_unit(unit).map_err(|e| e.to_string())?;
    // Registry access is compiler-emitted code too, sharing canonical get.
    let mut seed = MacroHost::new();
    let getter = e
        .evaluate_unit(&compile("(fn* [m k] (get m k))", &mut seed)?)
        .map_err(|e| e.to_string())?;
    for name in names {
        let key = e
            .import_data(
                &DataBuilder.string(name.encode_utf16().collect::<Vec<_>>()),
                ConversionLimits::default(),
            )
            .map_err(|e| e.to_string())?;
        let function = e.call(getter, &[values, key]).map_err(|e| e.to_string())?;
        e.release(key).map_err(|e| e.to_string())?;
        let declared = if matches!(*name, "for" | "doseq") {
            format!("sequence-{name}")
        } else {
            format!("macro-{}", MACROS.iter().find(|p| p.0 == *name).unwrap().1)
        };
        let definition = definitions
            .get(&declared)
            .ok_or_else(|| format!("missing bootstrap declaration {declared}"))?
            .clone();
        bootstrap.functions.insert(
            (*name).into(),
            CompiledMacro {
                // The bootstrap's own, for as long as its engine.
                function: MacroFunction::Ready(
                    std::sync::Arc::new(MacroValue {
                        id: function,
                        dropped: None,
                    }),
                    None,
                ),
                definition,
                metadata: None,
                generation: 0,
                reads_environment: unit
                    .environment_reads
                    .get(&declared)
                    .copied()
                    .unwrap_or(true),
            },
        );
    }
    e.release(getter).map_err(|e| e.to_string())?;
    e.release(values).map_err(|e| e.to_string())?;
    Ok(())
}
/// The bootstrap heap is `owner`'s compile-time evaluator, so its settings
/// (including the `*assert*` source macros read) reach every macro it runs.
pub(crate) fn build(owner: &MacroHost, cache: bool) -> Result<Bootstrap, String> {
    let evaluator = owner.new_compiler_evaluator().map_err(|e| e.to_string())?;
    build_in(evaluator, cache)
}
/// The bootstrap of a host bound to `evaluator` on its own thread
/// ([`MacroHost::bind`]): the process-wide units installed in it, compiled
/// only if no thread has compiled them yet.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn bind(
    evaluator: Rc<dyn crate::compiler_evaluator::CompilerEvaluator>,
) -> Result<Bootstrap, String> {
    build_in(evaluator, true)
}
/// Compile the process-wide bootstrap units now, unless a thread has, in a
/// short-lived engine of the kind `owner` makes, with the default limits:
/// a build with several isolates does this before it starts the deadline
/// they share, which no isolate's own exemption stops (light-cljs-378).
pub(crate) fn prepare(owner: &MacroHost) -> Result<(), String> {
    if UNITS.get().is_some() {
        return Ok(());
    }
    let mut host = MacroHost::new();
    host.engine.script_host = owner.engine.script_host.clone();
    let evaluator = host.new_compiler_evaluator().map_err(|e| e.to_string())?;
    build_in(evaluator, true).map(drop)
}
fn build_in(
    evaluator: Rc<dyn crate::compiler_evaluator::CompilerEvaluator>,
    cache: bool,
) -> Result<Bootstrap, String> {
    let cached = if cache { UNITS.get() } else { None };
    // Compiling the units is the process's work, not the build's: the
    // engine's deadline and poll budget stand still meanwhile, so a build
    // is charged only for installing them, the same whether or not the
    // process compiled them before (light-cljs-378). Their text depends on
    // no engine or setting.
    let control = evaluator.control();
    let _names = crate::generated_names::enter_namespace(crate::generated_names::BOOTSTRAP_SCOPE);
    let mut bootstrap = Bootstrap {
        evaluator,
        functions: HashMap::new(),
        dropped: Default::default(),
    };
    let mut host = MacroHost::new();
    host.tables.bootstrap_seed = true;
    let compiled_core;
    let core = match cached {
        Some(units) => &units[0],
        None => {
            compiled_core = control.exempt(|| compile(&core_source(), &mut host))?;
            &compiled_core
        }
    };
    install(
        core,
        &mut bootstrap,
        &MACROS.iter().map(|p| p.0).collect::<Vec<_>>(),
    )?;
    host.engine.bootstrap = Some(bootstrap.clone());
    let compiled_sequence;
    let sequence = match cached {
        Some(units) => &units[1],
        None => {
            compiled_sequence = control.exempt(|| {
                compile(
                    &format!("{SEQUENCES}\n{{\"for\" sequence-for \"doseq\" sequence-doseq}}"),
                    &mut host,
                )
            })?;
            &compiled_sequence
        }
    };
    install(sequence, &mut bootstrap, &["for", "doseq"])?;
    if cache && cached.is_none() {
        // Another thread may have set them first, to the same text.
        let _ = UNITS.set([core.clone(), sequence.clone()]);
    }
    Ok(bootstrap)
}
impl MacroHost {
    /// Start the compilation's compile-time engine now, as the first
    /// JavaScript macro it expands would: at a namespace's dispatch whose
    /// units may need it, so that it starts at the same point whichever
    /// thread analyzes them (light-cljs-391). Nothing without a session or
    /// once it has an engine. A start that fails is left to the expansion
    /// that needs it, which fails as it would have.
    pub(crate) fn start_session_engine(&mut self) {
        let Some(session) = self.session_handle() else {
            return;
        };
        if session.start_dispatch_engine(self).is_ok() && self.engine.bootstrap.is_none() {
            self.engine.bootstrap = session.bootstrap();
        }
    }
    pub(crate) fn expand_core_javascript(
        &mut self,
        name: &str,
        call: &Form,
        env: &Datum,
        engine: Option<&dyn crate::evaluator::MacroEvaluator>,
    ) -> Result<Form, String> {
        crate::macro_host::forbid_engine_entry(name);
        if self.engine.bootstrap.is_none() {
            if self.tables.bootstrap_seed && self.session_handle().is_none() {
                return Err(format!("bootstrap primitive seed cannot expand {name}"));
            }
            self.engine.bootstrap = self
                .session_handle()
                .and_then(|session| session.bootstrap());
            if self.engine.bootstrap.is_none() {
                self.engine.bootstrap = Some(build(self, true)?);
            }
        }
        let bootstrap = self.engine.bootstrap.as_ref().unwrap().clone();
        if engine.is_none()
            && let Some(session) = self.session_handle()
        {
            session.attach(&bootstrap, self)?;
        }
        let declaration = bootstrap
            .functions
            .get(name)
            .ok_or_else(|| {
                format!("core macro {name} is not a compiler declaration or bootstrapped function")
            })?
            .clone();
        let origin = ExpansionOrigin {
            macro_name: Name::new(Some("cljs.core"), name),
            call: call.span(),
            definition: Some(declaration.definition.clone()),
            helpers: vec![],
        };
        let engine = engine.unwrap_or(bootstrap.evaluator.as_ref());
        // Core macros were defined before user declarations. Their ordinary
        // collection operations must not demand the very extension whose body
        // they are expanding, or observe later user protocol replacements.
        // Keep source macros on their normal declaration frontier, and restore
        // any installed extensions (including executed mutations) on return.
        let _frontier = (self.engine.extension_frontier() != 0)
            .then(|| crate::compilation_session::FrontierGuard::bootstrap(engine))
            .transpose()?;
        // Only unwrapped bootstrap functions with an unused &env qualify.
        // Core macros execute at frontier zero, where user protocol extensions
        // cannot observe the compiler-owned map during construction. Source
        // macros and reader wrappers continue to receive the complete view.
        let unused = DataBuilder.nil();
        // Half the node limit, as each conversion takes at most the
        // limit ([`MacroHost::expand_javascript`]).
        let mut remaining = self.limits.generated_nodes / 2;
        let budget = remaining;
        let env = if !declaration.reads_environment
            && elidable_environment(env, &mut remaining, 128, &engine.control())
        {
            // With less budget the environment is converted instead, which
            // can cost more, so a namespace needs the budget that elision
            // needed: twice the environment's nodes
            // ([`crate::macro_host::WorkMarks`]).
            if self.measures_work() {
                let declared = crate::macro_host::WorkMarks::declared_elsewhere(env);
                self.note_node_room(2 * (budget - remaining + declared));
            }
            &unused
        } else {
            env
        };
        let result = self.expand_javascript(
            engine,
            declaration.function(engine, &self.engine)?,
            call,
            env,
            origin,
        );
        if result.is_err() {
            self.engine.bootstrap = None;
        }
        result
    }
}

#[derive(Clone)]
pub(crate) struct CompiledMacro {
    function: MacroFunction,
    definition: crate::compiler_data::SourceSpan,
    pub(crate) metadata: Option<Datum>,
    generation: usize,
    reads_environment: bool,
}
/// A macro's compile-time function: evaluated where it was declared, or, for
/// a macro installed from its namespace's artifact, on its first expansion.
#[derive(Clone)]
enum MacroFunction {
    /// Evaluated in the engine that declared it; a source macro's also with
    /// the code that declared it, which a worker's engine evaluates itself.
    Ready(
        std::sync::Arc<MacroValue>,
        Option<std::sync::Arc<MacroCode>>,
    ),
    /// Shared by every frame's copy of the declaration, so the function is
    /// evaluated once.
    Installed(std::sync::Arc<std::sync::Mutex<InstalledFunction>>),
}
/// A source macro's compile-time function as declaring it emitted it. The
/// declaring engine's value is not callable in another engine, so a
/// worker's engine evaluates this once for itself (docs/compiler-concurrency.md,
/// "As built (light-cljs-339)").
pub(crate) struct MacroCode {
    registration: crate::emitter::NamespaceRegistration,
    unit: String,
    /// The worker engines' key for the function they evaluate from it.
    identity: CodeIdentity,
}
struct InstalledFunction {
    unit: crate::emitter::DeferredUnit,
    function: Option<MacroValue>,
    /// The worker engines' key for the function they evaluate from it.
    identity: CodeIdentity,
}
/// A macro's compile-time code, unique in the process and shared by every
/// clone of its declaration: a worker engine's key for its own function
/// of it (`EngineBinding::foreign_macros`), which outlives the
/// declaration. An address would not identify it: the allocation of a
/// released generation can be reused by a later declaration
/// (light-cljs-385).
#[derive(Clone, Copy)]
struct CodeIdentity(usize);
impl CodeIdentity {
    fn next() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        Self(NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }
}
impl CompiledMacro {
    /// A macro of a namespace installed from its artifact
    /// (`crate::artifact_install`): its prepared compile-time function and
    /// the declarations it saw, evaluated when a later expansion first needs
    /// it.
    pub(crate) fn installed(
        unit: crate::emitter::DeferredUnit,
        definition: crate::compiler_data::SourceSpan,
        metadata: Datum,
        generation: usize,
    ) -> Self {
        Self {
            function: MacroFunction::Installed(std::sync::Arc::new(std::sync::Mutex::new(
                InstalledFunction {
                    unit,
                    function: None,
                    identity: CodeIdentity::next(),
                },
            ))),
            definition,
            metadata: Some(metadata),
            generation,
            reads_environment: true,
        }
    }
    /// The function to call, evaluating an installed macro's on `engine` the
    /// first time, as declaring it would have. In a worker's engine
    /// (`binding` is foreign), a source macro is evaluated once from its
    /// code, into the binding's own table.
    fn function(
        &self,
        engine: &dyn crate::evaluator::MacroEvaluator,
        binding: &crate::macro_host::EngineBinding,
    ) -> Result<ValueId, String> {
        if binding.foreign {
            return self.foreign_function(engine, binding);
        }
        let installed = match &self.function {
            MacroFunction::Ready(function, _) => return Ok(function.id),
            MacroFunction::Installed(installed) => installed,
        };
        if let Some(function) = &installed.lock().expect("installed macro").function {
            return Ok(function.id);
        }
        let (registration, unit) = installed.lock().expect("installed macro").unit.emit();
        engine
            .evaluate_registration(&registration.source, &registration.chunks)
            .map_err(|e| e.to_string())?;
        let function = engine
            .evaluate_unit(&format!(
                "rt => rt.core.macroReaderFunction(({})(rt))",
                unit
            ))
            .map_err(|e| e.to_string())?;
        let owner = match &binding.bootstrap {
            Some(bootstrap) => bootstrap.own(engine, function),
            None => MacroValue {
                id: function,
                dropped: None,
            },
        };
        installed.lock().expect("installed macro").function = Some(owner);
        Ok(function)
    }
    /// [`Self::function`] in a worker's engine: the declaring engine's
    /// value, or the installed macro's shared one, belongs to another.
    fn foreign_function(
        &self,
        engine: &dyn crate::evaluator::MacroEvaluator,
        binding: &crate::macro_host::EngineBinding,
    ) -> Result<ValueId, String> {
        let key = match &self.function {
            MacroFunction::Ready(_, Some(code)) => code.identity.0,
            // A bootstrap macro, which the worker's own bootstrap holds.
            MacroFunction::Ready(function, None) => return Ok(function.id),
            MacroFunction::Installed(installed) => {
                installed.lock().expect("installed macro").identity.0
            }
        };
        if let Some(&function) = binding.foreign_macros.borrow().get(&key) {
            return Ok(function);
        }
        let emitted;
        let (registration, unit) = match &self.function {
            MacroFunction::Ready(_, Some(code)) => (&code.registration, &code.unit),
            MacroFunction::Installed(installed) => {
                emitted = installed.lock().expect("installed macro").unit.emit();
                (&emitted.0, &emitted.1)
            }
            MacroFunction::Ready(_, None) => unreachable!("bootstrap macros returned above"),
        };
        engine
            .evaluate_registration(&registration.source, &registration.chunks)
            .map_err(|e| e.to_string())?;
        let function = engine
            .evaluate_unit(&format!(
                "rt => rt.core.macroReaderFunction(({})(rt))",
                unit
            ))
            .map_err(|e| e.to_string())?;
        binding.foreign_macros.borrow_mut().insert(key, function);
        Ok(function)
    }
    /// The `defmacro` form's place.
    pub(crate) fn definition(&self) -> &crate::compiler_data::SourceSpan {
        &self.definition
    }
    /// One more than the definition it replaced; also `:light-cljs/generation`.
    pub(crate) fn generation(&self) -> usize {
        self.generation
    }
}

/// Declaration seed: add the hidden ABI parameters without evaluating a macro.
/// The returned defn belongs to a compile-time unit, never an application unit.
pub(crate) fn lower_declaration(form: &Form) -> Result<(String, Form, Form), String> {
    use crate::compiler_data::DatumView;
    let c = DataBuilder;
    let parts = form.data().elements()?;
    let name = parts.get(1).ok_or("defmacro requires a name and methods")?;
    let DatumView::Symbol(symbol) = name.inspect() else {
        return Err("defmacro name must be a symbol".into());
    };
    if symbol.namespace().is_some() {
        return Err("defmacro name must be unqualified".into());
    }
    let mut start = 2;
    if parts
        .get(start)
        .is_some_and(|d| matches!(d.inspect(), DatumView::String(_)))
    {
        start += 1;
    }
    if parts
        .get(start)
        .is_some_and(|d| matches!(d.inspect(), DatumView::Map(_)))
    {
        start += 1;
    }
    let first = parts.get(start).ok_or("defmacro requires methods")?;
    let declarations = if matches!(first.inspect(), DatumView::Vector(_)) {
        vec![c.list(&parts[start..])?]
    } else {
        parts[start..].to_vec()
    };
    let mut methods = vec![];
    for declaration in declarations {
        // defn's trailing attributes map is metadata, not a method.
        if matches!(declaration.inspect(), DatumView::Map(_)) {
            continue;
        }
        let method = declaration.elements()?;
        let parameters = method
            .first()
            .ok_or("defmacro requires an argument vector")?;
        if !matches!(parameters.inspect(), DatumView::Vector(_)) {
            return Err("defmacro requires an argument vector".into());
        }
        let mut hidden = vec![
            c.symbol(Name::new(None, "&form")),
            c.symbol(Name::new(None, "&env")),
        ];
        hidden.extend(parameters.elements()?);
        let mut lowered = vec![c.vector(&hidden)?];
        lowered.extend_from_slice(&method[1..]);
        methods.push(c.list(&lowered)?);
    }
    let mut definition = vec![c.symbol(Name::new(Some("cljs.core"), "defn")), name.clone()];
    definition.extend_from_slice(&parts[2..start]);
    definition.extend(methods.clone());
    let mut function = vec![c.symbol(Name::new(Some("cljs.core"), "fn")), name.clone()];
    function.extend(methods);
    let origin = Name::new(Some("cljs.core"), "defmacro");
    Ok((
        String::from_utf16_lossy(symbol.name()),
        Form::from_expansion(c.list(&definition)?, form, origin.clone()),
        Form::from_expansion(c.list(&function)?, form, origin),
    ))
}
impl MacroHost {
    pub(crate) fn register_javascript_macro(
        &mut self,
        namespace: &str,
        form: &Form,
        snapshot: crate::compilation_session::DefinitionSnapshot,
        engine: Option<&dyn crate::evaluator::MacroEvaluator>,
    ) -> Result<(), String> {
        crate::macro_host::forbid_engine_entry("a macro declaration");
        let (name, _, function) = lower_declaration(form)?;
        // The declaration seed computes docs, user arglists and merged name,
        // leading and trailing metadata without executing any source body.
        let c = DataBuilder;
        let mut declaration_args = vec![c.nil(), c.nil()];
        declaration_args.extend(form.data().elements()?.into_iter().skip(1));
        let descriptor = crate::compiler_binding_macros::defn_macro(&mut { c }, &declaration_args)?;
        let mut metadata = descriptor.elements()?[1].metadata().unwrap_or(c.map(&[])?);
        metadata = c.assoc(
            &metadata,
            &c.keyword(Name::new(None, "macro")),
            &c.boolean(true),
        )?;
        if let Some(arglists) = metadata.get(&c.keyword(Name::new(None, "arglists")))?
            && let crate::compiler_data::DatumView::List(_) = arglists.inspect()
        {
            let quoted = arglists.elements()?;
            if quoted.len() == 2
                && matches!(quoted[0].inspect(), crate::compiler_data::DatumView::Symbol(ref name) if name.namespace().is_none() && name.name() == "quote".encode_utf16().collect::<Vec<_>>())
            {
                metadata = c.assoc(
                    &metadata,
                    &c.keyword(Name::new(None, "arglists")),
                    &quoted[1],
                )?;
            }
        }
        let generation = self
            .tables
            .javascript_macros
            .get(&format!("{namespace}/{name}"))
            .map_or(1, |previous| previous.generation + 1);
        for (key, value) in [
            (Name::new(None, "name"), c.symbol(Name::new(None, &name))),
            (Name::new(None, "ns"), c.symbol(Name::new(None, namespace))),
            (
                Name::new(Some("light-cljs"), "generation"),
                c.integer(generation as i64),
            ),
        ] {
            metadata = c.assoc(&metadata, &c.keyword(key), &value)?;
        }
        self.register_source_binding(
            namespace,
            &name,
            crate::macro_host::SourceBindingKind::Macro,
        )?;
        if self.engine.bootstrap.is_none() {
            self.engine.bootstrap = self
                .session_handle()
                .and_then(|session| session.bootstrap());
            if self.engine.bootstrap.is_none() {
                self.engine.bootstrap = Some(build(self, true)?);
            }
        }
        let bootstrap = self.engine.bootstrap.as_ref().unwrap().clone();
        bootstrap.release_dropped();
        if let Some(session) = self.session_handle() {
            session.attach(&bootstrap, self)?;
        }
        let mut frame = self.macro_analysis_frame();
        let (program, _) =
            crate::analyzer::analyze_deferred_form(&function, &mut frame, &snapshot, engine)
                .map_err(|e| e.to_string())?;
        let engine = engine.unwrap_or(bootstrap.evaluator.as_ref());
        let (registration, unit) = crate::emitter::emit_deferred_unit(&program);
        if let Some(session) = self.session_handle()
            && session.capturing()
        {
            session.capture_macro(
                format!("{namespace}/{name}"),
                unit.clone(),
                snapshot.resolution_program.globals.len(),
                program
                    .reader_contexts
                    .get(namespace)
                    .map(|c| (**c).clone()),
            );
        }
        engine
            .evaluate_registration(&registration.source, &registration.chunks)
            .map_err(|e| e.to_string())?;
        let function = engine
            .evaluate_unit(&format!(
                "rt => rt.core.macroReaderFunction(({})(rt))",
                unit
            ))
            .map_err(|e| e.to_string())?;
        let function = std::sync::Arc::new(bootstrap.own(engine, function));
        let code = std::sync::Arc::new(MacroCode {
            registration,
            unit,
            identity: CodeIdentity::next(),
        });
        self.insert_javascript_macro(
            format!("{namespace}/{name}"),
            CompiledMacro {
                function: MacroFunction::Ready(function, Some(code)),
                definition: form.span(),
                metadata: Some(metadata),
                generation,
                reads_environment: true,
            },
        );
        self.engine.macro_environment = Default::default();
        Ok(())
    }
    pub(crate) fn source_macro_key(&self, namespace: &str, name: &str) -> Option<String> {
        if let Some((qualifier, member)) = name.split_once('/') {
            if let Some(target) = self.source_namespace(namespace, qualifier) {
                return Some(format!("{target}/{member}"));
            }
            // A fully qualified macro of a namespace the program loaded: a macro
            // expansion names its own namespace's dependencies, as upstream.
            let key = format!("{qualifier}/{member}");
            let found = self.tables.javascript_macros.contains_key(&key);
            if found {
                self.observe(|log| {
                    log.named(
                        qualifier,
                        crate::dependency_manifest::EdgeKinds::MACRO_PROVIDER,
                    )
                });
            }
            return found.then_some(key);
        }
        if self.has_source_value(namespace, name)
            || self
                .tables
                .javascript_macros
                .contains_key(&format!("{namespace}/{name}"))
        {
            return Some(format!("{namespace}/{name}"));
        }
        self.macro_name(namespace, name, true)
            .map(|(ns, member)| format!("{ns}/{member}"))
    }
    pub(crate) fn has_javascript_macro(&self, namespace: &str, name: &str) -> bool {
        self.source_macro_key(namespace, name)
            .is_some_and(|key| self.tables.javascript_macros.contains_key(&key))
    }
    pub(crate) fn expand_source_javascript(
        &mut self,
        namespace: &str,
        name: &str,
        call: &Form,
        env: &Datum,
        engine: Option<&dyn crate::evaluator::MacroEvaluator>,
    ) -> Result<Form, String> {
        crate::macro_host::forbid_engine_entry(name);
        let key = self
            .source_macro_key(namespace, name)
            .ok_or_else(|| format!("macro {name} is not visible in namespace {namespace}"))?;
        let declaration = self
            .tables
            .javascript_macros
            .get(&key)
            .ok_or_else(|| format!("compile-time macro {key} is not declared"))?
            .clone();
        let (owner, member) = key.split_once('/').unwrap();
        if owner != namespace && self.source_binding_private(owner, member) {
            return Err(format!(
                "{key} is private; macros can only be imported from public bindings"
            ));
        }
        // A macro installed from an artifact may be the compilation's first:
        // its session starts as declaring one would have started it.
        if self.engine.bootstrap.is_none()
            && matches!(declaration.function, MacroFunction::Installed(_))
        {
            self.engine.bootstrap = self
                .session_handle()
                .and_then(|session| session.bootstrap());
            if self.engine.bootstrap.is_none() {
                self.engine.bootstrap = Some(build(self, true)?);
            }
            if let (Some(session), Some(bootstrap)) =
                (self.session_handle(), &self.engine.bootstrap)
            {
                session.attach(bootstrap, self)?;
            }
        }
        let bootstrap = self
            .engine
            .bootstrap
            .as_ref()
            .ok_or("macro session failed")?
            .clone();
        let (ns, name) = key.split_once('/').unwrap();
        let engine = engine.unwrap_or(bootstrap.evaluator.as_ref());
        let function = declaration.function(engine, &self.engine)?;
        self.expand_javascript(
            engine,
            function,
            call,
            env,
            ExpansionOrigin {
                macro_name: Name::new(Some(ns), name),
                call: call.span(),
                definition: Some(declaration.definition),
                helpers: vec![],
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compiling the bootstrap units, as a process's first build does,
    /// spends none of the build engine's poll budget (or deadline): the
    /// engine is charged what installing them costs, as when another build
    /// compiled them before (light-cljs-378). An interactive host's QuickJS
    /// engine counts its polls deterministically.
    #[cfg(light_quickjs)]
    #[test]
    fn compiling_the_units_charges_nothing_to_the_build_engine() {
        let mut owner = MacroHost::new();
        owner.tables.interactive = true;
        let remaining = |cache| {
            let bootstrap = build(&owner, cache).unwrap();
            bootstrap.evaluator.control().remaining_polls()
        };
        // The first build may compile the units, the second installs them.
        remaining(true);
        let installed = remaining(true);
        assert!(installed < owner.evaluator_options.poll_budget);
        assert_eq!(remaining(false), installed);
    }

    #[test]
    fn bootstrap_environment_reads_follow_resolved_locals_and_reflection() {
        for (body, expected) in [
            ("&form", false),
            ("'&env", false),
            ("(let* [&env 7] &env)", false),
            ("&env", true),
            ("(fn* [] &env)", true),
            ("js/arguments", true),
            ("(js-arguments)", true),
            ("(copy-arguments #js [])", true),
            ("(js* \"arguments[1]\")", true),
        ] {
            let mut host = MacroHost::new();
            host.tables.bootstrap_seed = true;
            let source = format!("(def sample (fn* [&form &env] {body}))");
            let syntax = crate::parse(&source).unwrap();
            let program = crate::analyze_with_macros(&source, &syntax, &mut host).unwrap();
            let init = program
                .exprs
                .iter()
                .find_map(|expr| match &expr.kind {
                    crate::ExprKind::Def { global, init, .. }
                        if program.globals[global.0].name == "sample" =>
                    {
                        *init
                    }
                    _ => None,
                })
                .unwrap();
            assert_eq!(reads_macro_environment(&program, init), expected, "{body}");
        }
        let mut host = MacroHost::new();
        host.tables.bootstrap_seed = true;
        let source = "(def ^:macro sample (fn* [&form &env] 42))";
        let syntax = crate::parse(source).unwrap();
        let wrapped = crate::analyze_with_macros(source, &syntax, &mut host).unwrap();
        assert!(!wrapped.macro_functions.is_empty());
    }

    #[test]
    fn unused_core_environment_keeps_conversion_limits_and_host_recovery() {
        let mut host = MacroHost::new();
        host.limits.generated_nodes = 200;
        let bindings = (0..20)
            .map(|i| format!("x{i} {i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let source = format!("(let* [{bindings}] (when true 42))");
        let error = crate::compile_with_macros(&source, &mut host)
            .unwrap_err()
            .to_string();
        assert!(error.contains("compiler input conversion limit"), "{error}");
        assert!(error.contains("macro cljs.core/when"), "{error}");
        host.limits.generated_nodes = 100_000;
        let js = crate::compile_with_macros("(when true 42)", &mut host).unwrap();
        let evaluator = Evaluator::new(Default::default()).unwrap();
        let value = evaluator.evaluate_js(&js).unwrap();
        assert_eq!(evaluator.number(value).unwrap(), 42.0);
    }

    #[test]
    fn unused_core_environment_keeps_literal_conversion_failures() {
        let mut host = MacroHost::new();
        let error = crate::compile_with_macros(
            r#"(defmacro ignored {:probe #"["} [] nil) (when true 42)"#,
            &mut host,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("macro cljs.core/when"), "{error}");
        assert!(
            error.contains("RegExp") || error.contains("regular expression"),
            "{error}"
        );
    }

    /// A function evaluated through the handle a callback receives, which is
    /// not the bootstrap's evaluator but the same heap, is owned too, and
    /// released only once its last owner is gone (light-cljs-381).
    #[test]
    fn a_function_evaluated_in_a_callback_is_released_after_its_last_owner() {
        let mut host = MacroHost::new();
        crate::compile_with_macros("(defmacro m [] (js/ownValue 0) 1)", &mut host).unwrap();
        let bootstrap = host.engine.bootstrap.clone().unwrap();
        let evaluator = bootstrap.evaluator.clone();
        let slot = Rc::new(std::cell::RefCell::new(Some(bootstrap.clone())));
        let owners = Rc::new(std::cell::RefCell::new(Vec::new()));
        let (service_slot, service_owners) = (slot.clone(), owners.clone());
        evaluator
            .clone()
            .install_compiler_service(
                "ownValue",
                Rc::new(move |active, _| {
                    let function = active
                        .evaluate_unit("rt => () => 1")
                        .map_err(|e| e.to_string())?;
                    let owner = service_slot
                        .borrow()
                        .as_ref()
                        .unwrap()
                        .own(active, function);
                    service_owners.borrow_mut().push((
                        std::ptr::addr_eq(
                            active as *const dyn crate::evaluator::MacroEvaluator,
                            Rc::as_ptr(&service_slot.borrow().as_ref().unwrap().evaluator),
                        ),
                        std::sync::Arc::new(owner),
                    ));
                    Ok("(rt) => 42".into())
                }),
            )
            .unwrap();
        let function = host.tables.javascript_macros["cljs.user/m"]
            .function(evaluator.as_ref(), &host.engine)
            .unwrap();
        let result = evaluator.call(function, &[]).unwrap();
        evaluator.release(result).unwrap();
        // The callback's slot held the bootstrap, and with it the engine.
        slot.borrow_mut().take();
        let (same_object, owner) = owners.borrow_mut().pop().unwrap();
        assert!(!same_object);
        assert!(owner.dropped.is_some());
        let roots = evaluator.value_roots().unwrap();
        let copy = owner.clone();
        drop(owner);
        bootstrap.release_dropped();
        assert_eq!(evaluator.value_roots().unwrap(), roots);
        drop(copy);
        bootstrap.release_dropped();
        assert_eq!(evaluator.value_roots().unwrap(), roots - 1);
    }

    #[test]
    fn cancelled_demand_discards_roots_before_reusing_host() {
        let mut host = MacroHost::new();
        for source in [
            "(def hits (atom 0)) (defn h [] (swap! hits inc) (js/cancelDemand 0)) (defmacro m [] (h))",
            "(def hits (atom 0)) (let [] (def value (do (swap! hits inc) (js/cancelDemand 0)))) (defmacro m [] (try value (catch :default e 42)))",
        ] {
            crate::compile_with_macros(source, &mut host).unwrap();
            let evaluator = host.engine.bootstrap.as_ref().unwrap().evaluator.clone();
            let control = evaluator.control();
            evaluator
                .clone()
                .install_compiler_service(
                    "cancelDemand",
                    Rc::new(move |_, _| {
                        control.cancel();
                        Ok("(rt) => 42".into())
                    }),
                )
                .unwrap();
            let function = host.tables.javascript_macros["cljs.user/m"]
                .function(evaluator.as_ref(), &host.engine)
                .unwrap();
            let error = evaluator.call(function, &[]).unwrap_err();
            assert!(error.0.contains("cancelled"), "{error}");
            assert!(evaluator.evaluate_js("1").is_err());
            let js = crate::compile_with_macros(
                "(def hits (atom 0)) (defmacro m [] (assert (= 0 @hits)) 42) (m)",
                &mut host,
            )
            .unwrap();
            assert!(js.contains("return 42"));
            assert!(!Rc::ptr_eq(
                &evaluator,
                &host.engine.bootstrap.as_ref().unwrap().evaluator
            ));
        }
    }

    #[test]
    fn asynchronous_bootstrap_cancellation_recovers_reused_host() {
        use std::time::Duration;

        // Cancellation arrives from another thread while the bootstrap imports
        // and expands its forms, the shape that produced the intermittent
        // engine crash (light-cljs-156).
        let mut host = MacroHost::new();
        let evaluator = Evaluator::new(crate::evaluator::EvaluatorOptions {
            poll_budget: u64::MAX,
            ..crate::evaluator::EvaluatorOptions::default()
        })
        .unwrap();
        let control = evaluator.control();
        host.tables.interactive = true;
        host.engine.interactive_control = Some(control.clone());
        host.evaluator_options.poll_budget = u64::MAX;
        host.evaluator_options.timeout = Duration::from_secs(10);
        let canceller = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            control.cancel();
        });
        let source = "(let [] (def value (loop [] (recur)))) (defmacro m [] (try value (catch :default e 42))) (m)";
        let syntax = crate::parse(source).unwrap();
        let error = crate::analyze_with_macros(source, &syntax, &mut host)
            .unwrap_err()
            .to_string();
        canceller.join().unwrap();
        assert!(error.contains("cancelled"), "{error}");

        // The cancelled session was discarded. The same host starts a clean
        // bootstrap and the recovered program executes to 42 in a fresh session.
        host.engine.interactive_control = None;
        host.tables.interactive = false;
        host.evaluator_options = crate::evaluator::EvaluatorOptions::default();
        let source = "(let [x 42] (defn value [] x)) (defmacro m [] (value)) (m)";
        let syntax = crate::parse(source).unwrap();
        let program = crate::analyze_with_macros(source, &syntax, &mut host).unwrap();
        let fresh = Evaluator::new(crate::evaluator::EvaluatorOptions::default()).unwrap();
        let value = fresh.evaluate(&program).unwrap();
        assert_eq!(fresh.number(value).unwrap(), 42.0);
    }

    #[test]
    fn declaration_products_are_atomic_and_unfinished_value_reads_cycle() {
        let mut host = MacroHost::new();
        crate::compile_with_macros("(defmacro ready [] 42)", &mut host).unwrap();
        let evaluator = host.engine.bootstrap.as_ref().unwrap().evaluator.clone();
        let value = evaluator.evaluate_unit(r#"rt => {
          const roots = globalThis.__lightCompilerRoots, products = globalThis.__lightCompilerBundleProducts;
          const names = ["test/T", "test/->T"];
          let effects = 0, first;
          globalThis.__lightRegisterBundle(names, rt => {
            effects++;
            products["test/T"] = rt.makeType([], [], false);
            products["test/->T"] = () => new (roots["test/T"])();
            throw new Error("after-product-installation");
          }, [], "bundle-test");
          try { roots["test/T"]; } catch (error) { first = error; }
          try { roots["test/->T"]; throw new Error("factory was published"); }
          catch (error) { if (error !== first) throw error; }
          if (effects !== 1) throw new Error("failed bundle retried");
          globalThis.__lightRegisterBundle(names, rt => {
            products["test/T"] = rt.makeType([], [], false);
            roots["test/T"];
          }, [], "cycle-test");
          try { roots["test/T"]; throw new Error("unfinished root was read"); }
          catch (error) { if (!error.message.includes("bundle initialization cycle")) throw error; }
          globalThis.__lightRegisterBundle(names, rt => {
            products["test/T"] = rt.makeType([], [], false);
            products["test/->T"] = () => new (roots["test/T"])();
          }, [], "replacement-test");
          const factory = roots["test/->T"], Type = roots["test/T"];
          if (!(factory() instanceof Type)) throw new Error("replacement products differ");
          return null;
        }"#).unwrap();
        evaluator.release(value).unwrap();
    }

    #[test]
    fn cancelled_extension_bundle_rolls_back_before_reusing_host() {
        let mut host = MacroHost::new();
        crate::compile_with_macros(
            r#"(defprotocol P (f [x]))
               (def target (js/cancelTarget 0))
               (extend-protocol P string (f [_] 1) target (f [_] 2))
               (defmacro m [] (f "s"))"#,
            &mut host,
        )
        .unwrap();
        let evaluator = host.engine.bootstrap.as_ref().unwrap().evaluator.clone();
        let control = evaluator.control();
        evaluator
            .clone()
            .install_compiler_service(
                "cancelTarget",
                Rc::new(move |_, _| {
                    control.cancel();
                    Ok("(rt) => 42".into())
                }),
            )
            .unwrap();
        let function = host.tables.javascript_macros["cljs.user/m"]
            .function(evaluator.as_ref(), &host.engine)
            .unwrap();
        let error = evaluator.call(function, &[]).unwrap_err();
        assert!(error.0.contains("cancelled"), "{error}");
        assert!(evaluator.evaluate_js("1").is_err());
        let js = crate::compile_with_macros(
            r#"(defprotocol P (f [x])) (extend-type string P (f [_] 42)) (defmacro m [] (f "s")) (m)"#,
            &mut host,
        ).unwrap();
        assert!(js.contains("return 42"));
        assert!(!Rc::ptr_eq(
            &evaluator,
            &host.engine.bootstrap.as_ref().unwrap().evaluator
        ));
    }

    #[test]
    fn regenerate_from_first_party_sources_and_collect_during_bootstrap() {
        let mut outputs = Vec::new();
        for collect in [false, true] {
            let mut host = MacroHost::new();
            host.tables.bootstrap_seed = true;
            let evaluator = Rc::new(Evaluator::new(Default::default()).unwrap());
            evaluator.install_gc_callback("bootstrapCollect").unwrap();
            let mut bootstrap = Bootstrap {
                evaluator: evaluator.clone(),
                functions: HashMap::new(),
                dropped: Default::default(),
            };
            let prefix = if collect {
                "(js/bootstrapCollect)\n"
            } else {
                ""
            };
            let core = compile(&format!("{prefix}{}", core_source()), &mut host).unwrap();
            install(
                &core,
                &mut bootstrap,
                &MACROS.iter().map(|p| p.0).collect::<Vec<_>>(),
            )
            .unwrap();
            host.engine.bootstrap = Some(bootstrap.clone());
            let sequences = compile(
                &format!("{prefix}{SEQUENCES}\n{{\"for\" sequence-for \"doseq\" sequence-doseq}}"),
                &mut host,
            )
            .unwrap();
            install(&sequences, &mut bootstrap, &["for", "doseq"]).unwrap();
            evaluator.collect_garbage();
            host.engine.bootstrap = Some(bootstrap);
            let program = crate::analyze_with_macros(
                "[(-> 2 inc (* 4)) (vec (for [x [1 2 3] :when (odd? x)] (* x x)))]",
                &crate::parse("[(-> 2 inc (* 4)) (vec (for [x [1 2 3] :when (odd? x)] (* x x)))]")
                    .unwrap(),
                &mut host,
            )
            .unwrap();
            let application = Evaluator::new(Default::default()).unwrap();
            outputs.push(
                application
                    .print(application.evaluate(&program).unwrap())
                    .unwrap(),
            );
            let source = Source::new("metadata.cljs", "(-> 1 ^:kept (inc))");
            let call = host.read_compiler_form(&source).unwrap().unwrap();
            let expanded = host
                .expand_core_javascript("->", &call, &DataBuilder.nil(), None)
                .unwrap();
            assert!(matches!(
                expanded
                    .data()
                    .metadata()
                    .unwrap()
                    .get(&DataBuilder.keyword(Name::new(None, "kept")))
                    .unwrap()
                    .unwrap()
                    .inspect(),
                crate::compiler_data::DatumView::Boolean(true)
            ));
        }
        assert_eq!(outputs, ["[12 [1 9]]", "[12 [1 9]]"]);
    }
}
