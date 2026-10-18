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
use std::{cell::RefCell, collections::HashMap, rc::Rc};

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

thread_local! {
    // Generated in memory from these exact include_str! inputs by this binary.
    // No disk artifact, source hash ambiguity or mutable JS state is retained.
    static UNITS: RefCell<Option<Vec<BootstrapUnit>>> = const { RefCell::new(None) };
}
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
                function,
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
fn build(owner: &MacroHost, cache: bool) -> Result<Bootstrap, String> {
    let cached = if cache {
        UNITS.with(|u| u.borrow().clone())
    } else {
        None
    };
    let mut bootstrap = Bootstrap {
        evaluator: owner.new_compiler_evaluator().map_err(|e| e.to_string())?,
        functions: HashMap::new(),
    };
    let mut host = MacroHost::new();
    host.bootstrap_seed = true;
    let core = match &cached {
        Some(units) => units[0].clone(),
        None => compile(&core_source(), &mut host)?,
    };
    install(
        &core,
        &mut bootstrap,
        &MACROS.iter().map(|p| p.0).collect::<Vec<_>>(),
    )?;
    host.bootstrap = Some(bootstrap.clone());
    let sequence = match &cached {
        Some(units) => units[1].clone(),
        None => compile(
            &format!("{SEQUENCES}\n{{\"for\" sequence-for \"doseq\" sequence-doseq}}"),
            &mut host,
        )?,
    };
    install(&sequence, &mut bootstrap, &["for", "doseq"])?;
    if cache && cached.is_none() {
        UNITS.with(|u| *u.borrow_mut() = Some(vec![core, sequence]));
    }
    Ok(bootstrap)
}
impl MacroHost {
    pub(crate) fn expand_core_javascript(
        &mut self,
        name: &str,
        call: &Form,
        env: &Datum,
        engine: Option<&dyn crate::evaluator::MacroEvaluator>,
    ) -> Result<Form, String> {
        if self.bootstrap.is_none() {
            if self.bootstrap_seed && self.session_handle().is_none() {
                return Err(format!("bootstrap primitive seed cannot expand {name}"));
            }
            self.bootstrap = self
                .session_handle()
                .and_then(|session| session.bootstrap());
            if self.bootstrap.is_none() {
                self.bootstrap = Some(build(self, true)?);
            }
        }
        let bootstrap = self.bootstrap.as_ref().unwrap().clone();
        if engine.is_none()
            && let Some(session) = self.session_handle()
        {
            session.attach(&bootstrap)?;
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
            definition: Some(declaration.definition),
            helpers: vec![],
        };
        let engine = engine.unwrap_or(bootstrap.evaluator.as_ref());
        // Core macros were defined before user declarations. Their ordinary
        // collection operations must not demand the very extension whose body
        // they are expanding, or observe later user protocol replacements.
        // Keep source macros on their normal declaration frontier, and restore
        // any installed extensions (including executed mutations) on return.
        let _frontier = self
            .session_handle()
            .filter(|session| session.extension_frontier() != 0)
            .map(|_| crate::compilation_session::FrontierGuard::bootstrap(engine))
            .transpose()?;
        // Only unwrapped bootstrap functions with an unused &env qualify.
        // Core macros execute at frontier zero, where user protocol extensions
        // cannot observe the compiler-owned map during construction. Source
        // macros and reader wrappers continue to receive the complete view.
        let unused = DataBuilder.nil();
        let mut remaining = self
            .limits
            .generated_nodes
            .saturating_sub(self.generated_nodes.get())
            / 2;
        let env = if !declaration.reads_environment
            && elidable_environment(env, &mut remaining, 128, &engine.control())
        {
            &unused
        } else {
            env
        };
        let result = self.expand_javascript(engine, declaration.function, call, env, origin);
        if result.is_err() {
            self.bootstrap = None;
        }
        result
    }
}

#[derive(Clone)]
pub(crate) struct CompiledMacro {
    function: ValueId,
    definition: crate::compiler_data::SourceSpan,
    pub(crate) metadata: Option<Datum>,
    generation: usize,
    reads_environment: bool,
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
        if self.bootstrap.is_none() {
            self.bootstrap = self
                .session_handle()
                .and_then(|session| session.bootstrap());
            if self.bootstrap.is_none() {
                self.bootstrap = Some(build(self, true)?);
            }
        }
        let bootstrap = self.bootstrap.as_ref().unwrap().clone();
        if let Some(session) = self.session_handle() {
            session.attach(&bootstrap)?;
        }
        let mut frame = self.macro_analysis_frame();
        let (program, _) =
            crate::analyzer::analyze_deferred_form(&function, &mut frame, &snapshot, engine)
                .map_err(|e| e.to_string())?;
        let engine = engine.unwrap_or(bootstrap.evaluator.as_ref());
        let (registration, unit) = crate::emitter::emit_deferred_unit(&program);
        engine
            .evaluate_registration(&registration.source, &registration.chunks)
            .map_err(|e| e.to_string())?;
        let function = engine
            .evaluate_unit(&format!(
                "rt => rt.core.macroReaderFunction(({})(rt))",
                unit
            ))
            .map_err(|e| e.to_string())?;
        self.javascript_macros.insert(
            format!("{namespace}/{name}"),
            CompiledMacro {
                function,
                definition: form.span(),
                metadata: Some(metadata),
                generation,
                reads_environment: true,
            },
        );
        self.macro_environment = Default::default();
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
            let found = self.javascript_macros.contains_key(&key);
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
            .is_some_and(|key| self.javascript_macros.contains_key(&key))
    }
    pub(crate) fn expand_source_javascript(
        &mut self,
        namespace: &str,
        name: &str,
        call: &Form,
        env: &Datum,
        engine: Option<&dyn crate::evaluator::MacroEvaluator>,
    ) -> Result<Form, String> {
        let key = self
            .source_macro_key(namespace, name)
            .ok_or_else(|| format!("macro {name} is not visible in namespace {namespace}"))?;
        let declaration = self
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
        let bootstrap = self
            .bootstrap
            .as_ref()
            .ok_or("macro session failed")?
            .clone();
        let (ns, name) = key.split_once('/').unwrap();
        self.expand_javascript(
            engine.unwrap_or(bootstrap.evaluator.as_ref()),
            declaration.function,
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
            host.bootstrap_seed = true;
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
        host.bootstrap_seed = true;
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

    #[test]
    fn cancelled_demand_discards_roots_before_reusing_host() {
        let mut host = MacroHost::new();
        for source in [
            "(def hits (atom 0)) (defn h [] (swap! hits inc) (js/cancelDemand 0)) (defmacro m [] (h))",
            "(def hits (atom 0)) (let [] (def value (do (swap! hits inc) (js/cancelDemand 0)))) (defmacro m [] (try value (catch :default e 42)))",
        ] {
            crate::compile_with_macros(source, &mut host).unwrap();
            let evaluator = host.bootstrap.as_ref().unwrap().evaluator.clone();
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
            let function = host.javascript_macros["cljs.user/m"].function;
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
                &host.bootstrap.as_ref().unwrap().evaluator
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
        host.interactive = true;
        host.interactive_control = Some(control.clone());
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
        host.interactive_control = None;
        host.interactive = false;
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
        let evaluator = host.bootstrap.as_ref().unwrap().evaluator.clone();
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
        let evaluator = host.bootstrap.as_ref().unwrap().evaluator.clone();
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
        let function = host.javascript_macros["cljs.user/m"].function;
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
            &host.bootstrap.as_ref().unwrap().evaluator
        ));
    }

    #[test]
    fn regenerate_from_first_party_sources_and_collect_during_bootstrap() {
        let mut outputs = Vec::new();
        for collect in [false, true] {
            let mut host = MacroHost::new();
            host.bootstrap_seed = true;
            let evaluator = Rc::new(Evaluator::new(Default::default()).unwrap());
            evaluator.install_gc_callback("bootstrapCollect").unwrap();
            let mut bootstrap = Bootstrap {
                evaluator: evaluator.clone(),
                functions: HashMap::new(),
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
            host.bootstrap = Some(bootstrap.clone());
            let sequences = compile(
                &format!("{prefix}{SEQUENCES}\n{{\"for\" sequence-for \"doseq\" sequence-doseq}}"),
                &mut host,
            )
            .unwrap();
            install(&sequences, &mut bootstrap, &["for", "doseq"]).unwrap();
            evaluator.collect_garbage();
            host.bootstrap = Some(bootstrap);
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
