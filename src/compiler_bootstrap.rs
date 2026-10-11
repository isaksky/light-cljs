//! First-party bootstrap compiled by the same Rust analyzer/emitter as applications.
//! Only immutable emitted source is cached; every compilation has a fresh JS heap.
use crate::{
    MacroHost,
    compiler_data::{DataBuilder, Datum, ExpansionOrigin, Form, Name, Source},
    evaluator::{Evaluator, ValueId},
    form_bridge::ConversionLimits,
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

const CORE: &str = include_str!("compiler_core_macros.cljs");
const SEQUENCES: &str = include_str!("compiler_sequence_macros.cljs");
const MACROS: &[(&str, &str)] = &[
    ("and", "and"),
    ("or", "or"),
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
    ("memfn", "memfn"),
    ("amap", "amap"),
    ("areduce", "areduce"),
    ("..", "dot-dot"),
    ("lazy-cat", "lazy-cat"),
    ("selector", "selector"),
    ("comment", "comment"),
];
#[derive(Clone)]
pub(crate) struct Bootstrap {
    pub(crate) evaluator: Rc<Evaluator>,
    functions: HashMap<String, CompiledMacro>,
}
thread_local! {
    // Generated in memory from these exact include_str! inputs by this binary.
    // No disk artifact, source hash ambiguity or mutable JS state is retained.
    static UNITS: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
}
fn compile(source: &str, host: &mut MacroHost) -> Result<String, String> {
    let syntax = crate::parse(source).map_err(|e| e.to_string())?;
    let program = crate::analyze_with_macros(source, &syntax, host).map_err(|e| {
        let loc = crate::LineIndex::new(source).line_column(e.span.start);
        format!("bootstrap {loc:?}: {e}")
    })?;
    Ok(crate::emitter::emit_js_unit(&program))
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
fn install(unit: &str, bootstrap: &mut Bootstrap, names: &[&str]) -> Result<(), String> {
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
    seed.disable_native_evaluation();
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
            },
        );
    }
    e.release(getter).map_err(|e| e.to_string())?;
    e.release(values).map_err(|e| e.to_string())?;
    Ok(())
}
fn build(options: crate::evaluator::EvaluatorOptions, cache: bool) -> Result<Bootstrap, String> {
    let cached = if cache {
        UNITS.with(|u| u.borrow().clone())
    } else {
        None
    };
    let mut bootstrap = Bootstrap {
        evaluator: Rc::new(Evaluator::new(options).map_err(|e| e.to_string())?),
        functions: HashMap::new(),
    };
    let mut host = MacroHost::new();
    host.disable_native_evaluation();
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
    ) -> Result<Form, String> {
        if self.bootstrap.is_none() {
            if self.bootstrap_seed {
                return Err(format!("bootstrap primitive seed cannot expand {name}"));
            }
            self.bootstrap = Some(build(self.evaluator_options.clone(), true)?);
        }
        let bootstrap = self.bootstrap.as_ref().unwrap().clone();
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
        let result = self.expand_javascript(
            &bootstrap.evaluator,
            declaration.function,
            call,
            env,
            origin,
        );
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
    ) -> Result<(), String> {
        let (name, _, function) = lower_declaration(form)?;
        if self.bootstrap.is_none() {
            self.bootstrap = Some(build(self.evaluator_options.clone(), true)?);
        }
        let bootstrap = self.bootstrap.as_ref().unwrap().clone();
        // Isolated analysis frame. The full deferred helper/catalog contract is
        // introduced by light-cljs-101/109, rather than consulting native roots.
        let mut frame = MacroHost::new();
        frame.disable_native_evaluation();
        frame.bootstrap_seed = true;
        frame.bootstrap = Some(bootstrap.clone());
        frame.javascript_macros = self.javascript_macros.clone();
        let program = crate::analyzer::analyze_compiler_form_with_host(&function, &mut frame)
            .map_err(|e| e.to_string())?;
        let function = bootstrap
            .evaluator
            .evaluate(&program)
            .map_err(|e| e.to_string())?;
        self.javascript_macros.insert(
            format!("{namespace}/{name}"),
            CompiledMacro {
                function,
                definition: form.span(),
            },
        );
        Ok(())
    }
    fn source_macro_key(namespace: &str, name: &str) -> String {
        if name.contains('/') {
            name.into()
        } else {
            format!("{namespace}/{name}")
        }
    }
    pub(crate) fn has_javascript_macro(&self, namespace: &str, name: &str) -> bool {
        self.javascript_macros
            .contains_key(&Self::source_macro_key(namespace, name))
    }
    pub(crate) fn expand_source_javascript(
        &mut self,
        namespace: &str,
        name: &str,
        call: &Form,
        env: &Datum,
    ) -> Result<Form, String> {
        let key = Self::source_macro_key(namespace, name);
        let declaration = self.javascript_macros.get(&key).unwrap().clone();
        let bootstrap = self
            .bootstrap
            .as_ref()
            .ok_or("macro session failed")?
            .clone();
        let (ns, name) = key.split_once('/').unwrap();
        self.expand_javascript(
            &bootstrap.evaluator,
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
    fn regenerate_from_first_party_sources_and_collect_during_bootstrap() {
        let mut outputs = Vec::new();
        for collect in [false, true] {
            let mut host = MacroHost::new();
            host.disable_native_evaluation();
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
                .expand_core_javascript("->", &call, &DataBuilder.nil())
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
