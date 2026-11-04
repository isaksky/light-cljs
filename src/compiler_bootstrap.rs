//! First-party bootstrap compiled by the same Rust analyzer/emitter as applications.
//! Only immutable bootstrap artifacts are cached; every compilation has a fresh JS heap.
#[cfg(all(test, light_quickjs))]
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
/// The names a compile-time runtime's `gensym` returned, one per prefix
/// ([`MacroHost::unscoped_gensyms`]).
#[cfg(feature = "bbp-macros")]
type Names = Result<Vec<String>, String>;
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
        crate::program_fragment::visit_children(&program.exprs[id.0].kind, |child| {
            pending.push(child);
        });
    }
    false
}
/// The keys of its `&env` a source macro's analyzed function, the root of
/// its deferred program, can read, or `None` when it may read the whole
/// environment (light-cljs-427). As [`reads_macro_environment`], over
/// resolved locals, but a read of the hidden `&env` as the map of a keyword
/// invocation, `(:ns &env)` or `(:ns &env default)`, reads that key alone.
/// Every other read, a capture by a nested function among them, may see
/// all of it.
fn source_environment_keys(program: &crate::Program) -> Option<Vec<Datum>> {
    use crate::ExprKind;
    let mut root = program.root;
    while let ExprKind::Do(items) = &program.exprs[root.0].kind
        && let [item] = items.as_slice()
    {
        root = *item;
    }
    if program.macro_functions.contains(&root) {
        return None;
    }
    let ExprKind::Fn { methods, .. } = &program.exprs[root.0].kind else {
        return None;
    };
    let environments = methods
        .iter()
        .map(|method| method.params.get(1).copied())
        .collect::<Option<Vec<_>>>()?;
    let environment = |id: crate::ExprId| {
        matches!(&program.exprs[id.0].kind,
            ExprKind::Local(local) if environments.contains(local))
    };
    let keyword = |id: crate::ExprId| match &program.exprs[id.0].kind {
        ExprKind::Keyword(literal) => {
            let literal = &program.named_literals[literal.0];
            let namespace = literal.namespace.map(|s| program.strings[s.0].clone());
            let name = &program.strings[literal.name.0];
            Some(DataBuilder.keyword(Name::from_utf16(namespace.as_deref(), name)))
        }
        _ => None,
    };
    let mut keys: Vec<Datum> = Vec::new();
    let mut pending: Vec<_> = methods.iter().map(|method| method.body).collect();
    let mut seen = std::collections::HashSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        let kind = &program.exprs[id.0].kind;
        if let ExprKind::Call { callee, args } = kind
            && let [map, rest @ ..] = args.as_slice()
            && environment(*map)
        {
            // Not core `get`, whose binding a program can replace for the
            // call (`with-redefs`) with one that reads the whole map.
            let key = match (&program.exprs[callee.0].kind, rest) {
                (ExprKind::Keyword(_), [] | [_]) => keyword(*callee).map(|key| (key, rest)),
                _ => None,
            };
            if let Some((key, rest)) = key {
                if !keys
                    .iter()
                    .any(|known| crate::form_bridge::exact(known, &key))
                {
                    keys.push(key);
                }
                pending.extend(rest.iter().copied());
                continue;
            }
        }
        match kind {
            ExprKind::Local(local) if environments.contains(local) => return None,
            // Raw JavaScript and argument reflection can observe otherwise
            // unused formals. Stay conservative about type/method captures.
            ExprKind::JsArguments
            | ExprKind::CopyArguments(_)
            | ExprKind::JsTemplate { .. }
            | ExprKind::Reify { .. }
            | ExprKind::DefType { .. }
            | ExprKind::ExtendType { .. } => return None,
            ExprKind::JsGlobal(global) if program.js_globals[global.0].path[0] == "arguments" => {
                return None;
            }
            ExprKind::Fn { methods, .. }
                if methods
                    .iter()
                    .any(|m| m.captures.iter().any(|local| environments.contains(local))) =>
            {
                return None;
            }
            _ => {}
        }
        crate::program_fragment::visit_children(&program.exprs[id.0].kind, |child| {
            pending.push(child);
        });
    }
    Some(keys)
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
/// The node count and height of `data` when it is elidable at all, as
/// [`elidable_environment`] would visit it: `None` when it holds a compiler
/// literal, or has more than `nodes` nodes or a greater height than `depth`,
/// or when `control` stops the walk, which it is polled for every 1024
/// nodes `visited` counts.
fn elidable_size(
    data: &Datum,
    nodes: usize,
    depth: usize,
    control: &crate::evaluator::ExecutionControl,
    visited: &mut usize,
) -> Option<(usize, usize)> {
    if depth == 0 || nodes == 0 {
        return None;
    }
    *visited += 1;
    if *visited & 1023 == 0 && control.poll().is_err() {
        return None;
    }
    if let crate::compiler_data::DatumKind::CompilerLiteral { .. } = data.kind() {
        return None;
    }
    let (mut count, mut height) = (1, 1);
    let mut include = |child: &Datum, count: &mut usize| {
        let (n, h) = elidable_size(child, nodes - *count, depth - 1, control, visited)?;
        *count += n;
        height = height.max(h + 1);
        Some(())
    };
    if let Some(metadata) = data.metadata() {
        include(&metadata, &mut count)?;
    }
    if let crate::compiler_data::DatumKind::Collection { children, .. } = data.kind() {
        for child in children.iter() {
            include(child, &mut count)?;
        }
    }
    Some((count, height))
}
/// How many environments' collections [`elidable_map_environment`] keeps the
/// sizes of.
const ELIDABLE_SIZES: usize = 8;
/// The node counts and heights [`elidable_map_environment`] keeps, by the
/// collection measured, most recently used last.
pub(crate) type ElidableSizes = std::cell::RefCell<Vec<(Datum, (usize, usize))>>;
/// [`elidable_environment`] for a macro's environment, the same answer with
/// the same budget left: the size of each collection the map holds is kept
/// by its identity in `sizes`, most recently used last, so that one shared
/// by every expansion, such as the map of every macro the compilation
/// declared, is walked once rather than at each expansion.
fn elidable_map_environment(
    data: &Datum,
    remaining: &mut usize,
    depth: usize,
    control: &crate::evaluator::ExecutionControl,
    sizes: &ElidableSizes,
) -> bool {
    let crate::compiler_data::DatumKind::Collection { children, .. } = data.kind() else {
        return elidable_environment(data, remaining, depth, control);
    };
    if data.metadata().is_some() || depth < 2 || *remaining == 0 {
        return elidable_environment(data, remaining, depth, control);
    }
    if *remaining & 1023 == 0 && control.poll().is_err() {
        return false;
    }
    *remaining -= 1;
    children.iter().all(|child| {
        if !matches!(
            child.kind(),
            crate::compiler_data::DatumKind::Collection { .. }
        ) {
            return elidable_environment(child, remaining, depth - 1, control);
        }
        let identity = child.identity();
        let known = {
            let mut sizes = sizes.borrow_mut();
            let position = sizes
                .iter()
                .position(|(data, _)| data.identity() == identity);
            position.map(|position| {
                let entry = sizes.remove(position);
                let size = entry.1;
                sizes.push(entry);
                size
            })
        };
        let size = known.or_else(|| {
            let size = elidable_size(child, *remaining, depth - 1, control, &mut 0)?;
            let mut sizes = sizes.borrow_mut();
            if sizes.len() == ELIDABLE_SIZES {
                sizes.remove(0);
            }
            sizes.push((child.clone(), size));
            Some(size)
        });
        match size {
            Some((nodes, height)) if nodes <= *remaining && height < depth => {
                *remaining -= nodes;
                true
            }
            _ => false,
        }
    })
}

// Generated in memory from these exact include_str! inputs by this binary,
// once per process for every thread: they are read and analyzed in the
// bootstrap's own scope of generated names, so every thread, and every
// compilation before or after another, gets the same text and the same
// suffixes (crate::generated_names::BOOTSTRAP_SCOPE). No disk artifact,
// source hash ambiguity or mutable JS state is retained.
static UNITS: std::sync::OnceLock<[BootstrapUnit; 2]> = std::sync::OnceLock::new();
// Every bootstrap uses the same immutable builder code. A lent task keeps
// one cached handle even when a new host binds another bootstrap to it.
#[cfg(feature = "bbp-macros")]
static NATIVE_JS_OBJ: std::sync::OnceLock<CodeIdentity> = std::sync::OnceLock::new();
fn compile(source: &str, host: &mut MacroHost) -> Result<BootstrapUnit, String> {
    compile_with(source, host, crate::emitter::emit_js_unit)
}
/// [`compile`], emitting the program with `emit`.
fn compile_with(
    source: &str,
    host: &mut MacroHost,
    emit: fn(&crate::Program) -> String,
) -> Result<BootstrapUnit, String> {
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
        source: emit(&program),
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
fn definitions(
    names: &[&str],
) -> Result<HashMap<String, crate::compiler_data::SourceSpan>, String> {
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
    Ok(definitions)
}
/// The Pants build's core macro definitions: for each core macro, the code
/// identity under which a task keeps its function, the source a task
/// evaluates it from, and whether it reads `&env`. They are compiled once
/// per process, as the other engines' bootstrap units are, and emitted as
/// the Pants build emits source macros.
#[cfg(feature = "bbp-macros")]
type NativeDefinitions = HashMap<&'static str, (CodeIdentity, std::sync::Arc<str>, bool)>;
#[cfg(feature = "bbp-macros")]
fn native_definitions() -> Result<&'static NativeDefinitions, String> {
    static DEFINITIONS: std::sync::OnceLock<NativeDefinitions> = std::sync::OnceLock::new();
    if let Some(definitions) = DEFINITIONS.get() {
        return Ok(definitions);
    }
    let _names = crate::generated_names::enter_namespace(crate::generated_names::BOOTSTRAP_SCOPE);
    let _unlogged = crate::generated_names::unlogged();
    // As the Pants build compiles them, also beside another engine: the
    // core macros only it expands natively (`hash-map`) stay native.
    let mut host = MacroHost::new();
    host.use_native_macros();
    host.tables.bootstrap_seed = true;
    let compile = |source: &str, host: &mut MacroHost| {
        compile_with(source, host, |program| {
            crate::emitter::emit_native_macro_unit(program, false, &[])
        })
    };
    let getter = compile("(fn* [m k] (get m k))", &mut host)?;
    let core = compile(&native_core_source()?, &mut host)?;
    let sequence = compile(
        &format!("{SEQUENCES}\n{{\"for\" sequence-for \"doseq\" sequence-doseq}}"),
        &mut host,
    )?;
    let mut definitions = HashMap::new();
    for (name, unit) in MACROS
        .iter()
        .map(|(name, _)| (*name, &core))
        .chain([("for", &sequence), ("doseq", &sequence)])
    {
        let declared = if matches!(name, "for" | "doseq") {
            format!("sequence-{name}")
        } else {
            format!("macro-{}", MACROS.iter().find(|p| p.0 == name).unwrap().1)
        };
        let source = format!(
            "rt => ({})(rt)(({})(rt), {name:?})",
            getter.source, unit.source
        );
        definitions.insert(
            name,
            (
                CodeIdentity::next(),
                std::sync::Arc::from(source),
                unit.environment_reads
                    .get(&declared)
                    .copied()
                    .unwrap_or(true),
            ),
        );
    }
    // Another thread may have set them first, from the same text.
    let _ = DEFINITIONS.set(definitions);
    Ok(DEFINITIONS.get().unwrap())
}
/// The core macro definitions as the Pants build runs them. The V8 build's
/// engine decodes a macro's map and set arguments into its own maps and
/// sets: keys equal there merge, and more than eight entries take the
/// order of their hashes. The Pants build's runtime receives them as
/// literal forms in reader order. Where a definition computes from them
/// (`simple-benchmark` printing them, `selector` and `defmulti` merging
/// into them) it uses them decoded as the engine does (`macro-decoded`);
/// what it returns of them unchanged stays as it was.
#[cfg(feature = "bbp-macros")]
fn native_core_source() -> Result<String, String> {
    const DECODED: &str = r#"
(defn macro-decoded [x]
  (let [decoded (cond
                  (map? x) (into (if (> (count x) 8) (hash-map) {})
                             (map (fn [entry] [(macro-decoded (key entry)) (macro-decoded (val entry))]) x))
                  (set? x) (into #{} (map macro-decoded x))
                  (vector? x) (mapv macro-decoded x)
                  (seq? x) (apply list (map macro-decoded x))
                  :else x)
        metadata (meta x)]
    (if (some? metadata) (with-meta decoded (macro-decoded metadata)) decoded)))
"#;
    // Where a definition computes from its arguments' maps: what
    // `simple-benchmark` prints, the map `selector` merges its names into
    // and the metadata `defmulti` merges.
    const DECODE: [(&str, &str); 5] = [
        ("(pr-str bindings)", "(pr-str (macro-decoded bindings))"),
        ("(pr-str expr)", "(pr-str (macro-decoded expr))"),
        (
            "(pr-str (last options))",
            "(pr-str (macro-decoded (last options)))",
        ),
        ("(merge m names)", "(merge (macro-decoded m) names)"),
        (
            "(merge (meta name) attributes",
            "(merge (macro-decoded (meta name)) (macro-decoded attributes)",
        ),
    ];
    let mut core = core_source();
    for (from, to) in DECODE {
        if core.matches(from).count() != 1 {
            return Err(format!("bootstrap definitions no longer spell {from}"));
        }
        core = core.replace(from, to);
    }
    let start = core
        .find("(defmacro macro-simple-benchmark")
        .ok_or("missing bootstrap declaration macro-simple-benchmark")?;
    Ok(format!("{}{DECODED}{}", &core[..start], &core[start..]))
}
/// Where the core macro `name`'s bootstrap definition is.
#[cfg(feature = "bbp-macros")]
fn definition_span(name: &str) -> Result<crate::compiler_data::SourceSpan, String> {
    let (names, declared): (&[&str], _) = if matches!(name, "for" | "doseq") {
        (&["for"], format!("sequence-{name}"))
    } else {
        let declared = MACROS
            .iter()
            .find(|p| p.0 == name)
            .ok_or_else(|| format!("core macro {name} is not a compiler declaration"))?;
        (&[], format!("macro-{}", declared.1))
    };
    definitions(names)?
        .remove(&declared)
        .ok_or_else(|| format!("missing bootstrap declaration {declared}"))
}
fn install(unit: &BootstrapUnit, bootstrap: &mut Bootstrap, names: &[&str]) -> Result<(), String> {
    #[cfg(feature = "goose-macros")]
    let _probe = crate::compiler_probe::scope(|| format!("bootstrap/install/{}", names.join(",")));
    let definitions = definitions(names)?;
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
                environment_keys: None,
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
    #[cfg(feature = "goose-macros")]
    let _probe = crate::compiler_probe::scope(|| {
        format!(
            "bootstrap/build-in/cached={}",
            cache && UNITS.get().is_some()
        )
    });
    if evaluator.native_forms() {
        return Ok(Bootstrap {
            evaluator,
            functions: HashMap::new(),
            dropped: Default::default(),
        });
    }
    let cached = if cache { UNITS.get() } else { None };
    // Compiling the units is the process's work, not the build's: the
    // engine's deadline and poll budget stand still meanwhile, so a build
    // is charged only for installing them, the same whether or not the
    // process compiled them before (light-cljs-378). Their text depends on
    // no engine or setting.
    let control = evaluator.control();
    let _names = crate::generated_names::enter_namespace(crate::generated_names::BOOTSTRAP_SCOPE);
    let _unlogged = crate::generated_names::unlogged();
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
            #[cfg(feature = "goose-macros")]
            let _probe = crate::compiler_probe::scope(|| "bootstrap/compile-core".into());
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
            #[cfg(feature = "goose-macros")]
            let _probe = crate::compiler_probe::scope(|| "bootstrap/compile-sequences".into());
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
/// The V8 build's `RegExp` constructor's error for `pattern` (UTF-16 code
/// units) with `flags`, which the Pants build's parser refuses with
/// `error`: V8's wording of the reasons the two share, which for some
/// depends on where in the pattern the parser stopped, and the flags after
/// the pattern.
#[cfg(feature = "bbp-macros")]
fn v8_regex_error(
    pattern: &[u16],
    flags: &str,
    error: &big_boy_pants_js::regexp::parse::SyntaxError,
) -> String {
    let message = error.message.as_str();
    if matches!(
        message,
        "unknown regular expression flag" | "duplicate regular expression flag"
    ) {
        return format!("SyntaxError: Invalid flags supplied to RegExp constructor '{flags}'");
    }
    let at = error.offset.min(pattern.len());
    let before = &pattern[..at];
    let unit = |c: u8| u16::from(c);
    let escaped = |index: usize| {
        before[..index]
            .iter()
            .rev()
            .take_while(|&&u| u == unit(b'\\'))
            .count()
            % 2
            == 1
    };
    let ends = |text: &str| {
        let suffix = text.encode_utf16().collect::<Vec<_>>();
        before.ends_with(&suffix) && !escaped(at - suffix.len())
    };
    // Where an atom may start: at the start, after `(` or `|`, an assertion
    // that takes no quantifier, or a group's prefix, a capture's name
    // included. Escaped syntax characters are completed atoms, including
    // an escaped `(` followed by text that resembles a group prefix.
    let atom_start = before.last().is_none_or(|&last| {
        [b'(', b'|', b'^', b'$'].iter().any(|&c| last == unit(c)) && !escaped(at - 1)
    }) || before.split_last().is_some_and(|(&last, _)| {
        // `\b` or `\B`, not an escaped backslash and `b`.
        (last == unit(b'b') || last == unit(b'B')) && escaped(at - 1)
    }) || ["(?:", "(?=", "(?!", "(?<=", "(?<!"]
        .iter()
        .any(|prefix| ends(prefix))
        || before.last() == Some(&unit(b'>'))
            && before
                .iter()
                .rposition(|&u| u == unit(b'<'))
                .is_some_and(|open| {
                    before[..open].ends_with(&[unit(b'('), unit(b'?')])
                        && !escaped(open - 2)
                        && !before[open..].contains(&unit(b')'))
                });
    // After a completed quantifier (`a*`, `a*?`, `a{1}`, ...), a malformed
    // `{` starts a new term. Probe that position with another quantifier so the
    // parser distinguishes these from escaped characters (`\*`), classes,
    // and braced escapes (`\u{31}`) that still take a quantifier. Keep later
    // captures so forward references stay valid. Only this error path needs
    // a second parse.
    let quantified_term = || {
        let mut probe = pattern.to_vec();
        // An invalid quantifier's offset points at its opening `{`.
        probe[at] = unit(b'*');
        big_boy_pants_js::JsString::from_code_units(probe).is_some_and(|probe| {
            big_boy_pants_js::regexp::parse::validate_syntax(
                &probe,
                &big_boy_pants_js::JsString::from(flags),
            )
            .is_err_and(|error| error.offset == at && error.message == "nothing to repeat")
        })
    };
    // Whether the parser stopped inside a character class.
    let in_class = {
        let (mut class, mut index) = (false, 0);
        while index < at {
            match pattern[index] {
                u if u == unit(b'\\') => index += 1,
                u if u == unit(b'[') => class = true,
                u if u == unit(b']') => class = false,
                _ => {}
            }
            index += 1;
        }
        class
    };
    // A group of modifiers `(?ims-ims:`, from the `(` the parser stopped at.
    let modifiers = || {
        let group = &pattern[at..];
        let mut seen = Vec::new();
        let mut index = 2;
        let mut repeated = false;
        while let Some(&u) = group.get(index) {
            if u == unit(b'-') {
            } else if [b'i', b'm', b's'].iter().any(|&flag| u == unit(flag)) {
                repeated |= seen.contains(&u);
                seen.push(u);
            } else {
                break;
            }
            index += 1;
        }
        match group.get(index) {
            Some(&u) if u == unit(b':') && repeated => "Repeated flag in flag group",
            Some(&u) if u == unit(b':') => "Invalid flag group",
            _ => "Invalid group",
        }
    };
    let reason = match message {
        // `^*`, `$+`, `\b?`; a quantified lookaround.
        "assertion cannot be quantified" if before.last() == Some(&unit(b')')) => {
            "Invalid quantifier".to_owned()
        }
        "assertion cannot be quantified" => "Nothing to repeat".to_owned(),
        // A `{` that does not end a quantifier with the u flag: after an
        // atom, or at the start of a new term; or a lone `}` or `]`.
        "invalid quantifier" if !atom_start && !quantified_term() => {
            "Incomplete quantifier".to_owned()
        }
        "invalid quantifier" | "unexpected syntax character" => {
            "Lone quantifier brackets".to_owned()
        }
        "unmatched closing parenthesis" => "Unmatched ')'".to_owned(),
        "quantifier range out of order" => "numbers out of order in {} quantifier".to_owned(),
        "class range out of order" => "Range out of order in character class".to_owned(),
        "duplicate group name" => "Duplicate capture group name".to_owned(),
        "invalid group name" => "Invalid capture group name".to_owned(),
        "invalid group prefix" => "Invalid group".to_owned(),
        "duplicate group modifier" => "Repeated flag in flag group".to_owned(),
        "invalid group modifiers" => modifiers().to_owned(),
        "invalid named backreference" => "Invalid named reference".to_owned(),
        "unknown named backreference" => "Invalid named capture referenced".to_owned(),
        "invalid identity escape in u mode"
        | "invalid hexadecimal escape"
        | "invalid decimal backreference" => "Invalid escape".to_owned(),
        "invalid control escape" => "Invalid Unicode escape".to_owned(),
        "unknown Unicode property"
        | "unknown or unsupported Unicode property"
        | "invalid Unicode property escape"
            if in_class =>
        {
            "Invalid property name in character class".to_owned()
        }
        "unknown Unicode property"
        | "unknown or unsupported Unicode property"
        | "invalid Unicode property escape" => "Invalid property name".to_owned(),
        "class escape cannot be a range endpoint" => "Invalid character class".to_owned(),
        // `nothing to repeat`, `unterminated group` and the like.
        _ => {
            let mut chars = message.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect())
                .unwrap_or_default()
        }
    };
    let pattern = String::from_utf16_lossy(pattern);
    format!("SyntaxError: Invalid regular expression: /{pattern}/{flags}: {reason}")
}
/// The error of the first regular expression literal in `data` (in the
/// order the V8 build's engine decodes it: children, then metadata) whose
/// pattern does not compile, as that engine's `RegExp` constructor reports
/// it, or `None`. As `rePattern` does, a leading `(?flags)` group gives the
/// flags; the pattern is validated as its UTF-16 code units, lone
/// surrogates included.
#[cfg(feature = "bbp-macros")]
pub(crate) fn invalid_regex(data: &Datum) -> Option<String> {
    use crate::compiler_data::{CompilerLiteralKind, DatumKind};
    match data.kind() {
        DatumKind::CompilerLiteral {
            kind: CompilerLiteralKind::Regex,
            payload,
        } => {
            let DatumKind::String(text) = payload.kind() else {
                return None;
            };
            let (flags, pattern) = text
                .strip_prefix(&[u16::from(b'('), u16::from(b'?')][..])
                .and_then(|rest| {
                    let close = rest.iter().position(|&u| u == u16::from(b')'))?;
                    let flags = &rest[..close];
                    flags
                        .iter()
                        .all(|&u| u8::try_from(u).is_ok_and(|b| b"dgimsuxy".contains(&b)))
                        .then(|| (flags, &rest[close + 1..]))
                })
                .unwrap_or((&[][..], &text[..]));
            let js = |units: &[u16]| big_boy_pants_js::JsString::from_code_units(units.to_vec());
            big_boy_pants_js::regexp::parse::validate_syntax(&js(pattern)?, &js(flags)?)
                .err()
                .map(|error| v8_regex_error(pattern, &String::from_utf16_lossy(flags), &error))
        }
        DatumKind::CompilerLiteral { payload, .. } => invalid_regex(payload),
        DatumKind::Collection { children, .. } => children.iter().find_map(invalid_regex),
        _ => None,
    }
    .or_else(|| {
        data.metadata()
            .and_then(|metadata| invalid_regex(&metadata))
    })
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
    /// Give the host its bootstrap, the session's or a new one, as the
    /// first expansion of the core macro `name` that needs it does.
    fn ensure_bootstrap(&mut self, name: &str) -> Result<(), String> {
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
        Ok(())
    }
    /// The compile-time runtime in which the call `call` of the core macro
    /// `name`, expanded natively outside every scope of generated names,
    /// numbers its names: the one its bootstrap definition would run in
    /// ([`Self::expand_core_javascript`]), `engine` or the bootstrap's.
    /// Its `gensym` there skips the names that runtime decoded, which the
    /// call's own join first ([`crate::bbp_evaluator::BbpEvaluator::unscoped_gensyms`]),
    /// so the names are the ones the V8 build's engine gives (light-cljs-611).
    #[cfg(feature = "bbp-macros")]
    pub(crate) fn unscoped_gensyms<'e>(
        &mut self,
        name: &str,
        engine: Option<&'e dyn crate::evaluator::MacroEvaluator>,
    ) -> Result<impl FnMut(&Datum, &[&str]) -> Names + use<'e>, String> {
        self.ensure_bootstrap(name)?;
        let bootstrap = self.engine.bootstrap.as_ref().unwrap().clone();
        if engine.is_none()
            && let Some(session) = self.session_handle()
        {
            session.attach(&bootstrap, self)?;
        }
        let name = name.to_owned();
        Ok(move |call: &Datum, bases: &[&str]| {
            engine
                .unwrap_or(bootstrap.evaluator.as_ref())
                .native_shell()
                .ok_or_else(|| {
                    format!("core macro {name} outside a scope needs the native runtime")
                })?
                .unscoped_gensyms(call, bases)
                .map_err(|e| e.to_string())
        })
    }
    /// The Pants build's diagnostic for the call `call` of the core macro
    /// `name` that native expansion declines: the one the V8 build's engine
    /// gives running its definition
    /// ([`crate::native_core_macros::rejection`]), or, where that cannot
    /// be told, that the Pants build does not expand the call.
    #[cfg(feature = "bbp-macros")]
    fn reject_core_call(&self, name: &str, call: &Form) -> Option<String> {
        let message = crate::native_core_macros::rejection(
            name,
            &call.data(),
            "cljs.user",
            self.limits.generated_nodes,
        )?;
        let origin = ExpansionOrigin {
            macro_name: Name::new(Some("cljs.core"), name),
            call: call.span(),
            definition: definition_span(name).ok(),
            helpers: vec![],
        };
        Some(crate::macro_host::expansion_failure(&origin, message))
    }
    /// Outside every scope of generated names the V8 build's engine runs
    /// the core macro `name`'s definition, converting the call's
    /// environment `env` first unless the definition never reads it and it
    /// can be elided ([`elidable_map_environment`]); a regular expression
    /// literal there that does not compile fails the call. The Pants build,
    /// which expands the call natively without the environment, checks the
    /// same literals with its engine's parser.
    #[cfg(feature = "bbp-macros")]
    pub(crate) fn check_core_environment(
        &mut self,
        name: &str,
        call: &Form,
        env: &Datum,
    ) -> Result<(), String> {
        self.ensure_bootstrap(name)?;
        let control = self.engine.bootstrap.as_ref().unwrap().evaluator.control();
        let mut remaining = self.limits.generated_nodes / 2;
        // Only `defmulti`'s definition reads `&env`.
        if name != "defmulti"
            && elidable_map_environment(
                env,
                &mut remaining,
                128,
                &control,
                &self.engine.elidable_sizes,
            )
        {
            return Ok(());
        }
        match invalid_regex(env) {
            None => Ok(()),
            Some(message) => Err(crate::macro_host::expansion_failure(
                &ExpansionOrigin {
                    macro_name: Name::new(Some("cljs.core"), name),
                    call: call.span(),
                    definition: definition_span(name).ok(),
                    helpers: vec![],
                },
                message,
            )),
        }
    }
    pub(crate) fn expand_core_javascript(
        &mut self,
        name: &str,
        call: &Form,
        env: &Datum,
        engine: Option<&dyn crate::evaluator::MacroEvaluator>,
    ) -> Result<Form, String> {
        #[cfg(feature = "goose-macros")]
        let _probe = crate::compiler_probe::scope(|| format!("core/cljs.core/{name}"));
        #[cfg(feature = "goose-macros")]
        let engine = engine.filter(|engine| !engine.source_only());
        crate::macro_host::forbid_engine_entry(name);
        self.ensure_bootstrap(name)?;
        #[cfg(feature = "bbp-macros")]
        if name == "js-obj" {
            let bootstrap = self.engine.bootstrap.as_mut().unwrap();
            if bootstrap.evaluator.native_forms() && !bootstrap.functions.contains_key(name) {
                // The active helper producer may own a different task from
                // this bootstrap. Keep code identity here, and resolve its
                // function only in the selected task below.
                bootstrap.functions.insert(
                    name.into(),
                    CompiledMacro {
                        function: MacroFunction::NativeJsObj(
                            *NATIVE_JS_OBJ.get_or_init(CodeIdentity::next),
                        ),
                        definition: definitions(&[name])?
                            .remove("macro-js-obj")
                            .ok_or("missing bootstrap declaration macro-js-obj")?,
                        metadata: None,
                        generation: 0,
                        reads_environment: false,
                        environment_keys: None,
                    },
                );
            }
        }
        // On the Pants build a call native expansion declines that the
        // definition rejects fails as the V8 build's engine reports it, every
        // time, as the Pants runtime's conversions do not refuse what the
        // engine's do; any other runs the definition, installed in the
        // runtime on first use.
        #[cfg(feature = "bbp-macros")]
        if name != "js-obj"
            && self
                .engine
                .bootstrap
                .as_ref()
                .unwrap()
                .evaluator
                .native_forms()
        {
            if let Some(rejected) = self.reject_core_call(name, call) {
                return Err(rejected);
            }
            let bootstrap = self.engine.bootstrap.as_mut().unwrap();
            if !bootstrap.functions.contains_key(name) {
                let (identity, source, reads_environment) = native_definitions()?
                    .get(name)
                    .ok_or_else(|| format!("core macro {name} is not a compiler declaration"))?;
                let bootstrap = self.engine.bootstrap.as_mut().unwrap();
                bootstrap.functions.insert(
                    name.into(),
                    CompiledMacro {
                        function: MacroFunction::NativeDefinition(*identity, source.clone()),
                        definition: definition_span(name)?,
                        metadata: None,
                        generation: 0,
                        reads_environment: *reads_environment,
                        environment_keys: None,
                    },
                );
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
                if bootstrap.evaluator.native_forms() {
                    format!("big-boy-pants-js: unsupported operation: bootstrap macro {name}")
                } else {
                    format!(
                        "core macro {name} is not a compiler declaration or bootstrapped function"
                    )
                }
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
            && elidable_map_environment(
                env,
                &mut remaining,
                128,
                &engine.control(),
                &self.engine.elidable_sizes,
            ) {
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
        #[cfg(feature = "bbp-macros")]
        if name == "js-obj" && engine.has_native_bridge() {
            let result = self.expand_form_javascript(
                engine,
                declaration.function(engine, &self.engine)?,
                call,
                env,
                origin,
            );
            if result.is_err() {
                self.engine.bootstrap = None;
            }
            return result;
        }
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
    /// For a source macro, the keys of its `&env` it can read, or `None`
    /// when it may read all of it ([`source_environment_keys`]).
    environment_keys: Option<std::sync::Arc<[Datum]>>,
}
/// A macro's compile-time function: evaluated where it was declared, or, for
/// a macro installed from its namespace's artifact, on its first expansion.
#[derive(Clone)]
enum MacroFunction {
    /// Native bootstrap code whose callable handle belongs to the active
    /// task, including a producer analyzing while the bootstrap task waits.
    #[cfg(feature = "bbp-macros")]
    NativeJsObj(CodeIdentity),
    /// A core macro's bootstrap definition on the Pants build, which each
    /// task evaluates from its source (`rt => ...`) once, under the identity
    /// ([`native_definitions`]).
    #[cfg(feature = "bbp-macros")]
    NativeDefinition(CodeIdentity, std::sync::Arc<str>),
    /// Evaluated in the engine that declared it; a source macro's also with
    /// the code that declared it, which a worker's engine evaluates itself.
    Ready(
        std::sync::Arc<MacroValue>,
        Option<std::sync::Arc<MacroCode>>,
    ),
    /// Code published by another namespace task, without its engine handle.
    Published(std::sync::Arc<MacroCode>),
    /// Shared by every frame's copy of the declaration, so the function is
    /// evaluated once.
    Installed(std::sync::Arc<std::sync::Mutex<InstalledFunction>>),
}
/// A source macro's compile-time function as declaring it emitted it. The
/// declaring engine's value is not callable in another engine, so a
/// worker's engine evaluates this once for itself (docs/compiler-concurrency.md,
/// "As built (light-cljs-339)").
pub(crate) struct MacroCode {
    /// The tables its namespace registration reads.
    tables: crate::reader_registration::ReaderTables,
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
impl InstalledFunction {
    /// The tables its registration reads and its JavaScript, which an
    /// engine evaluates without holding the lock.
    fn parts(&self) -> (crate::reader_registration::ReaderTables, String) {
        (self.unit.tables().clone(), self.unit.code().to_owned())
    }
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
    pub(crate) fn published(&self) -> Self {
        let mut published = self.clone();
        if let MacroFunction::Ready(_, Some(code)) = &self.function {
            published.function = MacroFunction::Published(code.clone());
        }
        published
    }
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
            environment_keys: None,
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
        if engine.has_native_bridge() {
            // A helper's analysis runs in a fresh native task, even on the
            // coordinator. Only its declaring task may use the ready handle;
            // every other task materializes the declaration from its code.
            if let MacroFunction::Ready(function, _) = &self.function
                && function.id.session as usize == engine.heap()
            {
                return Ok(function.id);
            }
            return self.foreign_function(engine, binding);
        }
        if binding.foreign {
            return self.foreign_function(engine, binding);
        }
        let installed = match &self.function {
            #[cfg(feature = "bbp-macros")]
            MacroFunction::NativeJsObj(_) => return self.foreign_function(engine, binding),
            #[cfg(feature = "bbp-macros")]
            MacroFunction::NativeDefinition(..) => return self.foreign_function(engine, binding),
            MacroFunction::Ready(function, _) => return Ok(function.id),
            MacroFunction::Published(_) => return self.foreign_function(engine, binding),
            MacroFunction::Installed(installed) => installed,
        };
        if let Some(function) = &installed.lock().expect("installed macro").function {
            return Ok(function.id);
        }
        let (tables, unit) = installed.lock().expect("installed macro").parts();
        crate::reader_registration::register(engine, &tables).map_err(|e| e.to_string())?;
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
            #[cfg(feature = "bbp-macros")]
            MacroFunction::NativeJsObj(identity) => {
                return engine
                    .source_macro_function(identity.0, "rt => rt.compilerForms.jsObj")
                    .map_err(|e| e.to_string());
            }
            #[cfg(feature = "bbp-macros")]
            MacroFunction::NativeDefinition(identity, source) => {
                return engine
                    .source_macro_function(identity.0, source)
                    .map_err(|e| e.to_string());
            }
            MacroFunction::Ready(_, Some(code)) | MacroFunction::Published(code) => code.identity.0,
            // A bootstrap macro, which the worker's own bootstrap holds.
            MacroFunction::Ready(function, None) => return Ok(function.id),
            MacroFunction::Installed(installed) => {
                installed.lock().expect("installed macro").identity.0
            }
        };
        let native = engine.has_native_bridge();
        let cached = if native {
            engine.cached_source_macro_function(key)
        } else {
            binding.foreign_macros.borrow().get(&key).copied()
        };
        if let Some(function) = cached {
            return Ok(function);
        }
        let parts;
        let (tables, unit) = match &self.function {
            MacroFunction::Ready(_, Some(code)) | MacroFunction::Published(code) => {
                (&code.tables, &code.unit)
            }
            MacroFunction::Installed(installed) => {
                parts = installed.lock().expect("installed macro").parts();
                (&parts.0, &parts.1)
            }
            MacroFunction::Ready(_, None) => unreachable!("bootstrap macros returned above"),
            #[cfg(feature = "bbp-macros")]
            MacroFunction::NativeJsObj(_) => unreachable!("native bootstrap returned above"),
            #[cfg(feature = "bbp-macros")]
            MacroFunction::NativeDefinition(..) => unreachable!("native bootstrap returned above"),
        };
        crate::reader_registration::register(engine, tables).map_err(|e| e.to_string())?;
        let source = format!("rt => rt.core.macroReaderFunction(({})(rt))", unit);
        let function = engine
            .source_macro_function(key, &source)
            .map_err(|e| e.to_string())?;
        if !native {
            binding.foreign_macros.borrow_mut().insert(key, function);
        }
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
            .get_parts(namespace, &name)
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
        let (mut program, _) =
            crate::analyzer::analyze_deferred_form(&function, &mut frame, &snapshot, engine)
                .map_err(|e| e.to_string())?;
        #[cfg(feature = "goose-macros")]
        let goose = self.engine.goose.clone();
        #[cfg(feature = "goose-macros")]
        let engine = goose
            .as_deref()
            .map(|e| e as &dyn crate::evaluator::MacroEvaluator)
            .or(engine);
        let engine = engine.unwrap_or(bootstrap.evaluator.as_ref());
        let environment_keys = source_environment_keys(&program).map(std::sync::Arc::from);
        let unit = crate::emitter::emit_deferred_unit(&program);
        #[cfg(feature = "bbp-macros")]
        let unit = if bootstrap.evaluator.native_forms() || {
            #[cfg(feature = "goose-macros")]
            {
                engine.source_only()
            }
            #[cfg(not(feature = "goose-macros"))]
            {
                false
            }
        } {
            let replaced = self
                .session_handle()
                .map_or_else(Vec::new, |session| session.shared().native_replaced_core());
            crate::emitter::emit_native_macro_unit(&program, true, &replaced)
        } else {
            unit
        };
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
        let tables = crate::reader_registration::ReaderTables::take(&mut program);
        crate::reader_registration::register(engine, &tables).map_err(|e| e.to_string())?;
        let identity = CodeIdentity::next();
        let source = format!("rt => rt.core.macroReaderFunction(({})(rt))", unit);
        #[cfg(feature = "goose-macros")]
        let prepared = if engine.source_only() {
            engine.source_macro_function(identity.0, &source)
        } else {
            engine.evaluate_unit(&source)
        };
        #[cfg(not(feature = "goose-macros"))]
        let prepared = engine.evaluate_unit(&source);
        let function = prepared.map_err(|e| e.to_string())?;
        let function = std::sync::Arc::new(bootstrap.own(engine, function));
        let code = std::sync::Arc::new(MacroCode {
            tables,
            unit,
            identity,
        });
        self.insert_javascript_macro(
            format!("{namespace}/{name}"),
            CompiledMacro {
                function: MacroFunction::Ready(function, Some(code)),
                definition: form.span(),
                metadata: Some(metadata),
                generation,
                reads_environment: environment_keys.is_none(),
                environment_keys,
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
            let found = self
                .tables
                .javascript_macros
                .get_parts(qualifier, member)
                .is_some();
            if found {
                self.observe(|log| {
                    log.named(
                        qualifier,
                        crate::dependency_manifest::EdgeKinds::MACRO_PROVIDER,
                    )
                });
            }
            return found.then(|| format!("{qualifier}/{member}"));
        }
        if self.has_source_value(namespace, name)
            || self
                .tables
                .javascript_macros
                .get_parts(namespace, name)
                .is_some()
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
        if self.native_macro_backend()
            && self
                .session_handle()
                .is_some_and(|session| !session.shared().lanes().sees_here(owner))
        {
            return Err(format!(
                "compile-time namespace outside require closure: {key} (add a require of {owner})"
            ));
        }
        if owner != namespace && self.source_binding_private(owner, member) {
            return Err(format!(
                "{key} is private; macros can only be imported from public bindings"
            ));
        }
        // A macro installed from an artifact may be the compilation's first:
        // its session starts as declaring one would have started it.
        if self.engine.bootstrap.is_none()
            && matches!(
                declaration.function,
                MacroFunction::Installed(_) | MacroFunction::Published(_)
            )
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
        #[cfg(feature = "goose-macros")]
        let goose = self.engine.goose.clone();
        #[cfg(feature = "goose-macros")]
        if let (Some(goose), Some(session)) = (&goose, self.session_handle()) {
            session.prepare_goose(goose)?;
        }
        #[cfg(feature = "goose-macros")]
        if let (Some(goose), Some(worker)) = (
            &goose,
            self.engine.worker.as_ref().and_then(std::rc::Weak::upgrade),
        ) {
            worker.prepare_goose(goose)?;
        }
        #[cfg(feature = "goose-macros")]
        let engine = goose
            .as_deref()
            .map(|e| e as &dyn crate::evaluator::MacroEvaluator)
            .or(engine);
        let engine = engine.unwrap_or(bootstrap.evaluator.as_ref());
        let function = declaration.function(engine, &self.engine)?;
        let reader_view;
        let env = match self.reader_view(&declaration, env, engine) {
            Some(view) => {
                reader_view = view;
                &reader_view
            }
            None => env,
        };
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
    /// The environment a source macro that reads only some keys of its
    /// `&env`, or none, is given instead of `env` (light-cljs-427): only
    /// those entries and the `:ns` and `:light-cljs/reader-context` ones,
    /// the same data, from which the macro's reader wrapper
    /// (`withMacroReaderContext` in the runtime) binds `*ns*` and the reader
    /// context, so nothing it can observe changes, while the locals, the
    /// macro inventory and the rest are not converted
    /// ([`source_environment_keys`]). `None` keeps `env`: when the macro may
    /// read all of it, when it would not fit the conversion budget whole, so
    /// that leaving the rest out cannot hide a conversion failure
    /// ([`elidable_map_environment`]), or while the compilation measures
    /// each expansion's conversions for the cache
    /// ([`MacroHost::measures_work`]), whose work marks count what the whole
    /// environment's conversion charged, or once the compilation registered
    /// an extension bundle that converting the rest could demand or call
    /// ([`crate::compilation_session::reaches_environment_data`],
    /// light-cljs-433).
    fn reader_view(
        &self,
        declaration: &CompiledMacro,
        env: &Datum,
        engine: &dyn crate::evaluator::MacroEvaluator,
    ) -> Option<Datum> {
        let keys = declaration.environment_keys.as_ref()?;
        // Converting the entries left out constructs maps, which can demand
        // and call a user protocol extension at the source declaration
        // frontier, or one compile-time code installed outside a
        // declaration; while environment data can reach neither, nothing it
        // does is observable.
        if self.measures_work()
            || self.engine.environment_bundles() != 0
            || engine.environment_extensions().is_none_or(|tracking| {
                tracking.get() != crate::evaluator::EnvironmentExtensions::None
            })
        {
            return None;
        }
        // Half the node limit, as each conversion takes at most the limit
        // ([`MacroHost::expand_javascript`]).
        let mut remaining = self.limits.generated_nodes / 2;
        if !elidable_map_environment(
            env,
            &mut remaining,
            128,
            &engine.control(),
            &self.engine.elidable_sizes,
        ) {
            return None;
        }
        let c = DataBuilder;
        let mut entries: Vec<(Datum, Datum)> = Vec::with_capacity(keys.len() + 2);
        let wrapper = [
            c.keyword(Name::new(None, "ns")),
            c.keyword(Name::new(Some("light-cljs"), "reader-context")),
        ];
        for key in wrapper.into_iter().chain(keys.iter().cloned()) {
            if entries
                .iter()
                .any(|(k, _)| crate::form_bridge::exact(k, &key))
            {
                continue;
            }
            if let Some(value) = env.get(&key).ok()? {
                entries.push((key, value));
            }
        }
        c.map(&entries).ok()
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "bbp-macros")]
    #[test]
    fn native_js_obj_bootstrap_uses_active_producer_context() {
        use crate::evaluator::MacroEvaluator;
        let runtime = crate::bbp_evaluator::BbpRuntime::new(&Default::default()).unwrap();
        let owner = Rc::new(runtime.evaluator().unwrap());
        let mut seed = MacroHost::new();
        seed.use_native_macros();
        let mut host = MacroHost::bind(seed.environment(), Some(owner.clone())).unwrap();
        let producer = runtime.evaluator().unwrap();
        assert_ne!(owner.heap(), producer.heap());
        let call = host
            .read_compiler_form(&Source::new(
                "producer.cljs",
                r#"(js-obj "a" 1 "b" 2 "c" 3 "d" 4 "e" 5 "f" 6 "g" 7 "h" 8 "i" 9)"#,
            ))
            .unwrap()
            .unwrap();
        let environment = DataBuilder.map(&[]).unwrap();
        let expand = |host: &mut MacroHost, engine: Option<&dyn MacroEvaluator>| {
            let _names = crate::generated_names::enter_namespace(7);
            host.expand_core_javascript("js-obj", &call, &environment, engine)
                .unwrap()
        };
        // The first demand happens in a producer, without entering the
        // bootstrap's owner. Each task then retains its own callable handle.
        let expected = expand(&mut host, Some(&producer));
        let declaration = host.engine.bootstrap.as_ref().unwrap().functions["js-obj"].clone();
        let MacroFunction::NativeJsObj(identity) = declaration.function else {
            panic!("native bootstrap code identity");
        };
        assert!(owner.cached_source_macro_function(identity.0).is_none());
        let before = runtime.helper_counts().units_evaluated;
        let function = declaration.function(&producer, &host.engine).unwrap();
        assert_eq!(function.session as usize, producer.heap());
        assert_eq!(runtime.helper_counts().units_evaluated, before);
        let own_function = declaration.function(owner.as_ref(), &host.engine).unwrap();
        assert_ne!(own_function.session, function.session);
        assert_eq!(runtime.helper_counts().units_evaluated, before + 1);
        assert!(expand(&mut host, None).data().value_eq(&expected.data()));
        for _ in 0..2 {
            assert!(
                expand(&mut host, Some(&producer))
                    .data()
                    .value_eq(&expected.data())
            );
            assert_eq!(
                declaration.function(&producer, &host.engine).unwrap(),
                function
            );
        }
        // Lending preserves the task's cached function, while the host
        // still carries the bootstrap bound to its original owner.
        assert!(producer.park(Default::default()));
        let (borrowed, state) = runtime.lent_evaluator().unwrap();
        assert!(state.is_some());
        let before = runtime.helper_counts().units_evaluated;
        assert_eq!(
            declaration.function(&borrowed, &host.engine).unwrap(),
            function
        );
        assert_eq!(runtime.helper_counts().units_evaluated, before);
        assert!(
            expand(&mut host, Some(&borrowed))
                .data()
                .value_eq(&expected.data())
        );
    }

    /// The Pants build runs `doseq`'s bootstrap definition where native
    /// expansion declines a call, and the definition steps through its
    /// bindings two at a time with `nnext` (light-cljs-611): a call it
    /// expands gives the native expansion's data, in fresh scopes of
    /// generated names alike (light-cljs-621).
    #[cfg(feature = "bbp-macros")]
    #[test]
    fn native_doseq_definition_steps_bindings() {
        let mut host = MacroHost::new();
        host.use_native_macros();
        assert!(host.native_macro_backend());
        let environment = DataBuilder.map(&[]).unwrap();
        for source in [
            "(doseq [x xs y ys] (f x y))",
            "(doseq [x xs :let [a 1] y ys :when a] (f x) (g y))",
        ] {
            let call = host
                .read_compiler_form(&Source::new("doseq.cljs", source))
                .unwrap()
                .unwrap();
            let definition = {
                let _names = crate::generated_names::enter_namespace(7);
                host.expand_core_javascript("doseq", &call, &environment, None)
                    .unwrap_or_else(|error| panic!("{source}: {error}"))
            };
            let native = {
                let _names = crate::generated_names::enter_namespace(7);
                host.expand_core_native("doseq", &call, "user", ("doseq", &(0..1)), None)
                    .unwrap()
                    .unwrap()
            };
            assert!(
                definition.data().value_eq(&native.data()),
                "{source}: definition {:?}\nnative {:?}",
                definition.data(),
                native.data()
            );
        }
    }

    #[cfg(feature = "bbp-macros")]
    #[test]
    fn native_macro_cache_hits_skip_reader_registration() {
        let mut host = crate::MacroHost::new();
        host.use_native_macros();
        let source = "(defmacro one [] 1)";
        crate::analyze_with_macros(source, &crate::parse(source).unwrap(), &mut host).unwrap();
        let declaration = &host.tables.javascript_macros["cljs.user/one"];
        let MacroFunction::Ready(_, Some(code)) = &declaration.function else {
            panic!("source declaration code");
        };
        let installed = CompiledMacro::installed(
            crate::emitter::DeferredUnit::from_parts(
                code.unit.clone(),
                code.tables.namespaces.clone(),
                code.tables.globals.clone(),
                code.tables.reader_contexts.clone(),
            ),
            declaration.definition.clone(),
            declaration.metadata.clone().unwrap(),
            declaration.generation,
        );
        let runtime = crate::bbp_evaluator::BbpRuntime::new(&Default::default()).unwrap();
        let evaluations = || {
            let counts = runtime.helper_counts();
            counts.units_evaluated + counts.units_shared
        };
        // Each fresh task materializes its own function once. Native tasks
        // skip unused reader tables even on misses; hits also skip function
        // construction and evaluation.
        for declaration in [declaration, &installed] {
            let mut functions = Vec::new();
            for _ in 0..2 {
                let engine = runtime.evaluator().unwrap();
                let before = evaluations();
                let function = declaration.function(&engine, &host.engine).unwrap();
                assert_eq!(evaluations(), before + 1);
                for _ in 0..4 {
                    assert_eq!(
                        declaration.function(&engine, &host.engine).unwrap(),
                        function
                    );
                    assert_eq!(evaluations(), before + 1, "cache hit evaluated code");
                }
                functions.push(function);
            }
            assert_ne!(functions[0].session, functions[1].session);
        }
    }

    #[cfg(feature = "bbp-macros")]
    #[test]
    fn native_source_macro_preserves_datum_identity_across_expansions() {
        for (source, call_source) in [
            (
                "(defmacro same [x] x)",
                "(same ^{:m 1} [9007199254740993 \\a])",
            ),
            (
                "(defmacro same [x] (do (js/Math.max x 2) x))",
                "(same 9007199254740993)",
            ),
        ] {
            let mut host = crate::MacroHost::new();
            host.use_native_macros();
            crate::analyze_with_macros(source, &crate::parse(source).unwrap(), &mut host).unwrap();
            let call = host
                .read_compiler_form(&crate::compiler_data::Source::new(
                    "source-identity.cljs",
                    call_source,
                ))
                .unwrap()
                .unwrap();
            let original = call.children()[1].clone();
            let environment =
                crate::compiler_data::Datum::new(crate::compiler_data::DatumKind::Collection {
                    kind: crate::CollectionKind::Map,
                    children: Box::new([]),
                });
            for i in 0..2 {
                let expanded = host
                    .expand_source_javascript("cljs.user", "same", &call, &environment, None)
                    .unwrap();
                assert!(std::ptr::eq(expanded.data().kind(), original.data().kind()));
                assert_eq!(expanded.span().bytes(), original.span().bytes());
                assert_eq!(host.native_macro_calls()[i].exported, 0);
                assert_eq!(host.native_macro_calls()[i].reused, 1);
            }
            assert_eq!(host.native_macro_calls()[1].imported, 0);
        }
    }

    use super::*;

    fn collection(kind: crate::parser::CollectionKind, children: Vec<Datum>) -> Datum {
        Datum::new(crate::compiler_data::DatumKind::Collection {
            kind,
            children: children.into(),
        })
    }

    fn vector(children: Vec<Datum>) -> Datum {
        collection(crate::parser::CollectionKind::Vector, children)
    }

    fn integers(count: usize) -> Datum {
        vector(
            (0..count)
                .map(|i| Datum::new(crate::compiler_data::DatumKind::Integer(i as i64)))
                .collect(),
        )
    }

    fn environment(values: &[Datum]) -> Datum {
        let c = DataBuilder;
        collection(
            crate::parser::CollectionKind::Map,
            values
                .iter()
                .enumerate()
                .flat_map(|(i, value)| {
                    [c.keyword(Name::new(None, &format!("k{i}"))), value.clone()]
                })
                .collect(),
        )
    }

    fn control() -> crate::evaluator::ExecutionControl {
        crate::evaluator::ExecutionControl::new(&crate::evaluator::EvaluatorOptions {
            poll_budget: u64::MAX,
            ..Default::default()
        })
        .unwrap()
    }

    /// The elision check with kept collection sizes gives the complete
    /// walk's answer and leaves it the same budget, cold or warm, across
    /// node budgets, depths, metadata and compiler literals (light-cljs-415).
    #[test]
    fn cached_elision_matches_the_complete_walk() {
        let nested = (0..6).fold(integers(3), |inner, _| vector(vec![inner, integers(2)]));
        let literal = vector(vec![
            integers(2),
            Datum::new(crate::compiler_data::DatumKind::CompilerLiteral {
                kind: crate::compiler_data::CompilerLiteralKind::Regex,
                payload: DataBuilder.nil(),
            }),
        ]);
        let shared = integers(40).with_metadata(environment(&[integers(5)]));
        let environments = [
            environment(&[shared.clone(), integers(3)]),
            environment(&[shared.clone(), nested.clone(), DataBuilder.nil()]),
            environment(&[integers(10), shared.clone(), literal.clone()]),
            environment(&[nested.clone(), integers(2000)]),
            environment(&[]),
            vector(vec![shared.clone()]).with_metadata(environment(&[])),
        ];
        let control = control();
        let sizes = std::cell::RefCell::new(Vec::new());
        for _ in 0..2 {
            for env in &environments {
                for budget in [0, 1, 2, 5, 9, 40, 47, 48, 49, 60, 80, 2003, 2050, 4000] {
                    for depth in [0, 1, 2, 3, 7, 8, 9, 128] {
                        let (mut old, mut new) = (budget, budget);
                        let expected = elidable_environment(env, &mut old, depth, &control);
                        let actual =
                            elidable_map_environment(env, &mut new, depth, &control, &sizes);
                        assert_eq!(actual, expected, "budget {budget} depth {depth}");
                        if expected {
                            assert_eq!(new, old, "budget {budget} depth {depth}");
                        }
                    }
                }
            }
        }
        // A shared collection stays known while other ones come and go.
        let shared_identity = shared.identity();
        for i in 0..20 {
            let env = environment(&[shared.clone(), integers(i)]);
            elidable_map_environment(&env, &mut 1000, 128, &control, &sizes);
            assert!(
                sizes
                    .borrow()
                    .iter()
                    .any(|(data, _)| data.identity() == shared_identity)
            );
        }
    }

    /// Measuring a collection the sizes do not know yet stops when the
    /// compilation is cancelled, as the complete walk does.
    #[test]
    fn elision_size_misses_observe_cancellation() {
        let env = environment(&[integers(2048)]);
        let control = control();
        control.cancel();
        let sizes = std::cell::RefCell::new(Vec::new());
        assert!(!elidable_environment(&env, &mut 2051, 128, &control));
        assert!(!elidable_map_environment(
            &env, &mut 2051, 128, &control, &sizes
        ));
        assert!(sizes.borrow().is_empty());
    }

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
    fn source_environment_keys_follow_keyword_reads_of_resolved_locals() {
        for (body, expected) in [
            ("42", Some(vec![])),
            ("'&env", Some(vec![])),
            ("(let* [&env {}] (:ns &env))", Some(vec![])),
            ("(:ns &env)", Some(vec![":ns"])),
            (
                "(:light-cljs/reader-context &env :none)",
                Some(vec![":light-cljs/reader-context"]),
            ),
            ("(get &env :line 0)", None),
            (
                "[(:ns &env) (:ns &env) (:locals &env)]",
                Some(vec![":locals", ":ns"]),
            ),
            ("[(:ns &env) (get &env :ns)]", None),
            ("(:name (:ns &env))", Some(vec![":ns"])),
            ("(:ns &env (keys &env))", None),
            ("&env", None),
            ("(keys &env)", None),
            ("(get &env 'locals)", None),
            ("(get &env (:k &env))", None),
            ("(fn* [] (:ns &env))", None),
            ("(js* \"arguments[1]\")", None),
            ("(js-arguments)", None),
        ] {
            let mut host = MacroHost::new();
            host.tables.bootstrap_seed = true;
            let source = format!("(fn* [&form &env] {body})");
            let syntax = crate::parse(&source).unwrap();
            let program = crate::analyze_with_macros(&source, &syntax, &mut host).unwrap();
            let keys = source_environment_keys(&program).map(|keys| {
                keys.iter()
                    .map(|key| match key.kind() {
                        crate::compiler_data::DatumKind::Keyword(name) => {
                            let name_text = String::from_utf16_lossy(name.name());
                            match name.namespace() {
                                Some(ns) => {
                                    format!(":{}/{name_text}", String::from_utf16_lossy(ns))
                                }
                                None => format!(":{name_text}"),
                            }
                        }
                        _ => panic!("a key is a keyword"),
                    })
                    .collect::<Vec<_>>()
            });
            let expected = expected.map(|keys| keys.into_iter().map(String::from).collect());
            assert_eq!(keys, expected, "{body}");
        }
    }

    #[test]
    #[cfg(light_quickjs)]
    fn unused_core_environment_keeps_conversion_limits_and_host_recovery() {
        let mut host = MacroHost::new();
        host.limits.generated_nodes = 200;
        let bindings = (0..20)
            .map(|i| format!("x{i} {i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let source = format!("(let* [{bindings}] (doto 42))");
        let error = crate::compile_with_macros(&source, &mut host)
            .unwrap_err()
            .to_string();
        assert!(error.contains("compiler input conversion limit"), "{error}");
        assert!(error.contains("macro cljs.core/doto"), "{error}");
        host.limits.generated_nodes = 100_000;
        let js = crate::compile_with_macros("(doto 42)", &mut host).unwrap();
        let evaluator = Evaluator::new(Default::default()).unwrap();
        let value = evaluator.evaluate_js(&js).unwrap();
        assert_eq!(evaluator.number(value).unwrap(), 42.0);
    }

    #[test]
    fn unused_core_environment_keeps_literal_conversion_failures() {
        let check = |mut host: MacroHost| {
            let error = crate::compile_with_macros(
                r#"(defmacro ignored {:probe #"["} [] nil) (doto 42)"#,
                &mut host,
            )
            .unwrap_err()
            .to_string();
            assert!(error.contains("macro cljs.core/doto"), "{error}");
            assert!(
                error.contains("RegExp") || error.contains("regular expression"),
                "{error}"
            );
        };
        check(MacroHost::new());
        // The Pants build expands `doto` natively and checks the
        // environment's literals itself (light-cljs-611).
        #[cfg(feature = "bbp-macros")]
        {
            let mut host = MacroHost::new();
            host.use_native_macros();
            check(host);
        }
    }

    /// A function evaluated through the handle a callback receives, which is
    /// not the bootstrap's evaluator but the same heap, is owned too, and
    /// released only once its last owner is gone (light-cljs-381).
    // These engines lend callback views of one value-handle heap; Pants callbacks own separate tasks.
    #[cfg(any(feature = "quickjs", feature = "v8-macros"))]
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
    #[cfg(light_quickjs)]
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

    // Tests inline JavaScript declaration transactions; Pants executes compiler-owned catalog units.
    #[cfg(any(feature = "quickjs", feature = "v8-macros"))]
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
    #[cfg(light_quickjs)]
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

/// How long the Pants build takes to run the `doseq` bootstrap definition
/// through [`MacroHost::expand_core_javascript`] with raised limits, for
/// `(doseq [x0 xs ... xN-1 xs] (f x0))` at each N of
/// `LIGHT_CLJS_DOSEQ_BINDINGS` (default `4,5,6,7`): realizing the
/// definition's result is linear in its size (light-cljs-622), so the time
/// should about double per binding. A measurement, not a check:
/// `cargo test --no-default-features --features bbp-macros --lib
/// doseq_definition_timing -- --ignored --nocapture`.
#[cfg(all(test, feature = "bbp-macros"))]
mod doseq_timing {
    use super::*;

    #[test]
    #[ignore = "timing measurement"]
    fn doseq_definition_timing() {
        let counts = std::env::var("LIGHT_CLJS_DOSEQ_BINDINGS").unwrap_or("4,5,6,7".into());
        for n in counts
            .split(',')
            .map(|n| n.trim().parse::<usize>().unwrap())
        {
            let mut host = MacroHost::new();
            host.use_native_macros();
            host.limits.generated_nodes = 100_000_000;
            host.limits.expansions = 1 << 20;
            host.evaluator_options.timeout = std::time::Duration::from_secs(3600);
            host.evaluator_options.memory_limit = 4 << 30;
            let bindings = (0..n).map(|i| format!("x{i} xs")).collect::<Vec<_>>();
            let source = format!("(doseq [{}] (f x0))", bindings.join(" "));
            let call = host
                .read_compiler_form(&Source::new("doseq.cljs", &source))
                .unwrap()
                .unwrap();
            let environment = DataBuilder.map(&[]).unwrap();
            let _names = crate::generated_names::enter_namespace(7);
            // Install the definition first, so the time is the call's.
            let warm = host
                .read_compiler_form(&Source::new("warm.cljs", "(doseq [x0 xs] (f x0))"))
                .unwrap()
                .unwrap();
            host.expand_core_javascript("doseq", &warm, &environment, None)
                .unwrap();
            let start = std::time::Instant::now();
            let expanded = host
                .expand_core_javascript("doseq", &call, &environment, None)
                .unwrap();
            let elapsed = start.elapsed();
            let nodes = crate::macro_host::WorkMarks::nodes(&expanded.data());
            // A digest of the expansion, to compare builds' outputs.
            let digest = {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                format!("{:?}", expanded.data()).hash(&mut hasher);
                hasher.finish()
            };
            println!("doseq bindings={n} nodes={nodes} time={elapsed:.3?} digest={digest:016x}");
        }
    }
}

#[cfg(all(test, feature = "bbp-macros", light_v8))]
#[path = "../crates/big-boy-pants-js/prototypes/goose/capture.rs"]
pub(crate) mod goose_capture;
