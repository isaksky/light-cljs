//! Compiler-owned macro requests and configuration. JavaScript expansion uses owned forms.
use std::collections::{HashMap, HashSet};

use crate::analyzer::ExprContext;
use crate::compiler_data::{Form, Name};
use crate::compiler_names::{core_macro, is_definition_macro};

/// Compiler import metadata. Filesystem imports share ordinary runtime aliases.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MacroImport {
    pub namespace: String,
    pub alias: Option<String>,
    pub refers: Vec<String>,
}

/// Per-compilation expansion bounds, in addition to evaluator resource limits.
#[derive(Clone, Copy, Debug)]
pub struct MacroLimits {
    pub expansions: usize,
    pub generated_nodes: usize,
    pub expansion_depth: usize,
}
impl Default for MacroLimits {
    fn default() -> Self {
        Self {
            expansions: 1024,
            generated_nodes: 100_000,
            expansion_depth: 64,
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum SourceBindingKind {
    Value,
    Macro,
}

/// Macro configuration and compiler metadata. Each clean compilation owns a fresh
/// QuickJS bootstrap session; reusing this host never reuses JavaScript macro roots.
#[derive(Default)]
pub struct MacroHost {
    compiler_data_readers: std::rc::Rc<std::cell::RefCell<CompilerDataReaders>>,
    imports: HashMap<String, Vec<MacroImport>>,
    renames: HashMap<String, Vec<(String, String, String)>>,
    namespace_symbols: HashMap<String, crate::compiler_data::Datum>,
    namespace_docs: HashMap<String, crate::compiler_data::Datum>,
    runtime_vars: HashMap<String, HashMap<String, bool>>,
    namespace_imports: HashMap<String, Vec<(String, String)>>,
    pub limits: MacroLimits,
    /// Configuration only: JS roots belong to each fresh compilation session.
    pub evaluator_options: crate::evaluator::EvaluatorOptions,
    assertions_disabled: bool,
    pub(crate) interactive: bool,
    pub(crate) interactive_control: Option<crate::evaluator::ExecutionControl>,
    pub(crate) name_policy: Option<crate::eval_compiler::NamePolicy>,
    pub(crate) bootstrap: Option<crate::compiler_bootstrap::Bootstrap>,
    pub(crate) bootstrap_seed: bool,
    pub(crate) javascript_macros: HashMap<String, crate::compiler_bootstrap::CompiledMacro>,
    pub(crate) source_bindings: HashMap<String, SourceBindingKind>,
    pub(crate) session: Option<std::rc::Rc<crate::compilation_session::CompilationSession>>,
    pub(crate) session_ref: std::rc::Weak<crate::compilation_session::CompilationSession>,
    pub(crate) expansions: std::rc::Rc<std::cell::Cell<usize>>,
    pub(crate) generated_nodes: std::rc::Rc<std::cell::Cell<usize>>,
}

#[derive(Default)]
struct CompilerDataReaders {
    units: HashMap<Name, String>,
    default_unit: Option<String>,
    default_function: Option<crate::evaluator::ValueId>,
    evaluator: Option<std::rc::Rc<crate::evaluator::Evaluator>>,
    functions: HashMap<Name, crate::evaluator::ValueId>,
}

impl MacroHost {
    /// Allocate a fresh synchronous heap for a clean compilation. Its dynamic
    /// `*assert*` follows [`Self::set_assertions`], so the macros, functions and
    /// data readers it runs read the setting, as upstream's `:elide-asserts`
    /// binds `clojure.core/*assert*` while compiling.
    pub fn new_evaluator(
        &self,
    ) -> Result<crate::evaluator::Evaluator, crate::evaluator::EvalError> {
        let evaluator = if let Some(control) = &self.interactive_control {
            crate::evaluator::Evaluator::interactive_with_control(
                self.evaluator_options.clone(),
                control.clone(),
            )
        } else if self.interactive {
            crate::evaluator::Evaluator::interactive(self.evaluator_options.clone())
        } else {
            crate::evaluator::Evaluator::new(self.evaluator_options.clone())
        }?;
        if self.assertions_disabled {
            let done =
                evaluator.evaluate_unit("rt => { rt.core.assertions = false; return null; }")?;
            evaluator.release(done)?;
        }
        Ok(evaluator)
    }

    /// Execute a compiler-emitted macro function with hidden &form/&env arguments.
    /// Function handles are generation-scoped IDs, never engine values in Program.
    pub fn expand_javascript(
        &mut self,
        evaluator: &dyn crate::evaluator::MacroEvaluator,
        function: crate::evaluator::ValueId,
        call: &Form,
        environment: &crate::compiler_data::Datum,
        origin: crate::compiler_data::ExpansionOrigin,
    ) -> Result<Form, String> {
        if self.expansions.get() >= self.limits.expansions {
            return Err("compiler macro expansion limit exceeded".into());
        }
        self.expansions.set(self.expansions.get() + 1);
        let limits = crate::form_bridge::ConversionLimits {
            nodes: self
                .limits
                .generated_nodes
                .saturating_sub(self.generated_nodes.get()),
            depth: 128,
        };
        let run = || -> Result<Form, crate::evaluator::EvalError> {
            let mut args = vec![
                evaluator.import_form(call, limits)?,
                evaluator.import_data(environment, limits)?,
            ];
            for argument in call.children().iter().skip(1) {
                args.push(evaluator.import_form(argument, limits)?);
            }
            let result = evaluator.call(function, &args)?;
            let expanded = evaluator.export_form(result, call, origin.clone(), limits);
            // Root release is best effort after terminal engine failure.
            let _ = evaluator.release(result);
            for id in args {
                let _ = evaluator.release(id);
            }
            expanded
        };
        let expanded = run().map_err(|e| {
            let location = crate::compiler_data::DiagnosticLocation::from_span(&origin.call);
            let mut message = format!(
                "{}:{}:{}: macro {}/{}: {}",
                location.source,
                location.line,
                location.column,
                String::from_utf16_lossy(origin.macro_name.namespace().unwrap_or(&[])),
                String::from_utf16_lossy(origin.macro_name.name()),
                e
            );
            if let Some(definition) = &origin.definition {
                let d = crate::compiler_data::DiagnosticLocation::from_span(definition);
                message.push_str(&format!(
                    "; definition {}:{}:{}",
                    d.source, d.line, d.column
                ));
            }
            for helper in &origin.helpers {
                message.push_str(&format!(
                    "; helper {}/{}",
                    String::from_utf16_lossy(helper.namespace().unwrap_or(&[])),
                    String::from_utf16_lossy(helper.name())
                ));
            }
            message
        })?;
        self.generated_nodes.set(
            self.generated_nodes
                .get()
                .saturating_add(expanded.nodes().len()),
        );
        if self.generated_nodes.get() > self.limits.generated_nodes {
            return Err("compiler macro generated-node limit exceeded".into());
        }
        Ok(expanded)
    }

    pub fn new() -> Self {
        Self::default()
    }
    /// Control assertion emission for this host (enabled by default). Disabled
    /// assertions omit both condition and message, including function pre/post checks.
    pub fn set_assertions(&mut self, enabled: bool) {
        self.assertions_disabled = !enabled;
    }
    pub fn assertions_enabled(&self) -> bool {
        !self.assertions_disabled
    }
    /// Read immutable compiler-owned data without initializing an evaluator.
    pub fn read_compiler_form(
        &mut self,
        source: &crate::compiler_data::Source,
    ) -> Result<Option<Form>, crate::compiler_data::ReadError> {
        let mut parser = crate::Parser::new(source.text()).retaining_discards();
        let root = parser
            .read_form()
            .map_err(|error| crate::compiler_data::ReadError {
                message: error.to_string(),
                span: source.span(error.span).unwrap(),
                related: error.related.map(|span| source.span(span).unwrap()),
            })?;
        let mut discards = parser.discarded().to_vec();
        let syntax = crate::SyntaxTree {
            nodes: parser.into_nodes(),
            roots: root.into_iter().collect(),
        };
        discards.sort_by_key(|id| syntax.nodes[id.0].span.start);
        let Some(root) = root else {
            // Discard-only input still executes explicit tag handlers once.
            let mut end = 0;
            for &discard in &discards {
                let span = &syntax.nodes[discard.0].span;
                if span.start < end {
                    continue;
                }
                self.lower_form_in_view(
                    source,
                    &syntax,
                    discard,
                    &crate::compiler_reader::NamespaceView::new("cljs.user", source),
                    &discards,
                )?;
                end = span.end;
            }
            return Ok(None);
        };
        self.lower_form_in_view(
            source,
            &syntax,
            root,
            &crate::compiler_reader::NamespaceView::new("cljs.user", source),
            &discards,
        )
        .map(Some)
    }
    /// Register an explicit compiler-emitted function over immutable reader data.
    /// It executes in canonical JavaScript; only finite form-domain results cross
    /// the typed bridge. Registration grants no stream or filesystem capability.
    pub fn register_compiler_data_reader(
        &mut self,
        tag: Name,
        function_source: &str,
    ) -> Result<(), String> {
        let syntax = crate::parse(function_source).map_err(|e| e.to_string())?;
        let program = crate::analyze(function_source, &syntax).map_err(|e| e.to_string())?;
        let unit = crate::emitter::emit_js_unit(&program);
        let mut readers = self.compiler_data_readers.borrow_mut();
        readers.units.insert(tag.clone(), unit);
        if let Some(id) = readers.functions.remove(&tag)
            && let Some(evaluator) = &readers.evaluator
        {
            let _ = evaluator.release(id);
        }
        Ok(())
    }
    /// Fallback for unknown tags, called with the tag symbol and immutable value.
    /// Named readers take precedence; built-in literal tags retain their reader.
    pub fn register_compiler_default_data_reader(
        &mut self,
        function_source: &str,
    ) -> Result<(), String> {
        let syntax = crate::parse(function_source).map_err(|e| e.to_string())?;
        let program = crate::analyze(function_source, &syntax).map_err(|e| e.to_string())?;
        let mut readers = self.compiler_data_readers.borrow_mut();
        readers.default_unit = Some(crate::emitter::emit_js_unit(&program));
        if let Some(id) = readers.default_function.take()
            && let Some(evaluator) = &readers.evaluator
        {
            let _ = evaluator.release(id);
        }
        Ok(())
    }
    fn call_compiler_data_reader(
        &self,
        tag: &Name,
        value: &crate::compiler_data::Datum,
    ) -> Result<Option<crate::compiler_data::Datum>, String> {
        let run = || -> Result<Option<crate::compiler_data::Datum>, String> {
            let (evaluator, function, fallback) = {
                let mut readers = self.compiler_data_readers.borrow_mut();
                let fallback = !readers.units.contains_key(tag);
                let unit = if fallback {
                    if tag.namespace().is_none()
                        && matches!(
                            String::from_utf16_lossy(tag.name()).as_ref(),
                            "inst" | "uuid" | "js" | "queue"
                        )
                    {
                        return Ok(None);
                    }
                    readers.default_unit.clone()
                } else {
                    readers.units.get(tag).cloned()
                };
                let Some(unit) = unit else {
                    return Ok(None);
                };
                if readers.evaluator.is_none() {
                    readers.evaluator = Some(std::rc::Rc::new(
                        self.new_evaluator().map_err(|e| e.to_string())?,
                    ));
                }
                let evaluator = readers.evaluator.as_ref().unwrap().clone();
                let cached = if fallback {
                    readers.default_function
                } else {
                    readers.functions.get(tag).copied()
                };
                let function = if let Some(function) = cached {
                    function
                } else {
                    let function = evaluator.evaluate_unit(&unit).map_err(|e| e.to_string())?;
                    if fallback {
                        readers.default_function = Some(function);
                    } else {
                        readers.functions.insert(tag.clone(), function);
                    }
                    function
                };
                (evaluator, function, fallback)
            };
            let limits = crate::form_bridge::ConversionLimits {
                nodes: self.limits.generated_nodes,
                depth: 128,
            };
            let argument = evaluator
                .import_data(value, limits)
                .map_err(|e| e.to_string())?;
            let tag_argument = if fallback {
                match evaluator.import_data(
                    &crate::compiler_data::DataBuilder.symbol(tag.clone()),
                    limits,
                ) {
                    Ok(value) => Some(value),
                    Err(error) => {
                        let _ = evaluator.release(argument);
                        return Err(error.to_string());
                    }
                }
            } else {
                None
            };
            let result = if let Some(tag_argument) = tag_argument {
                evaluator.call(function, &[tag_argument, argument])
            } else {
                evaluator.call(function, &[argument])
            };
            let _ = evaluator.release(argument);
            if let Some(tag_argument) = tag_argument {
                let _ = evaluator.release(tag_argument);
            }
            let result = result.map_err(|e| e.to_string())?;
            let data = evaluator
                .export_data(result, limits)
                .map_err(|e| e.to_string());
            let _ = evaluator.release(result);
            data.map(Some)
        };
        let result = run();
        if result.is_err() {
            let mut readers = self.compiler_data_readers.borrow_mut();
            readers.evaluator = None;
            readers.functions.clear();
            readers.default_function = None;
        }
        result
    }
    pub(crate) fn lower_form_in_view(
        &self,
        source: &crate::compiler_data::Source,
        tree: &crate::SyntaxTree,
        root: crate::NodeId,
        view: &crate::compiler_reader::NamespaceView,
        discards: &[crate::NodeId],
    ) -> Result<Form, crate::compiler_data::ReadError> {
        let reader = |tag: &Name,
                      value: &crate::compiler_data::Datum,
                      location: &crate::compiler_data::SourceSpan| {
            let value = self.call_compiler_data_reader(tag, value)?;
            if let Some(value) = &value {
                let size = Form::from_data(
                    value.clone(),
                    location.clone(),
                    crate::compiler_data::Origin::SuppliedData,
                )
                .nodes()
                .len();
                let generated = self.generated_nodes.get().saturating_add(size);
                if generated > self.limits.generated_nodes {
                    return Err("compiler reader generated-node limit exceeded".into());
                }
                self.generated_nodes.set(generated);
            }
            Ok(value)
        };
        crate::compiler_reader::lower_form_with_reader(
            source,
            tree,
            root,
            view,
            discards,
            Some(&reader),
        )
    }
    /// Configure imports without loading or evaluating code. Filesystem compilation
    /// registers source dependencies first.
    pub(crate) fn set_interactive_imports(
        &mut self,
        caller: &str,
        imports: Vec<MacroImport>,
    ) -> Result<(), String> {
        let current = self.imports.entry(caller.to_owned()).or_default();
        for import in imports {
            if let Some(alias) = &import.alias
                && current.iter().any(|old| {
                    old.alias.as_ref() == Some(alias) && old.namespace != import.namespace
                })
            {
                return Err(format!("conflicting namespace alias: {alias}"));
            }
            if let Some(old) = current
                .iter_mut()
                .find(|old| old.namespace == import.namespace && old.alias == import.alias)
            {
                for name in import.refers {
                    if !old.refers.contains(&name) {
                        old.refers.push(name);
                    }
                }
            } else {
                current.push(import);
            }
        }
        Ok(())
    }
    pub fn set_imports(&mut self, caller: &str, imports: Vec<MacroImport>) -> Result<(), String> {
        let mut aliases = HashMap::new();
        let mut refers = HashSet::new();
        for import in &imports {
            for name in &import.refers {
                if !refers.insert(name.clone()) {
                    return Err(format!("duplicate macro refer: {name}"));
                }
            }
            if let Some(alias) = &import.alias {
                if matches!(alias.as_str(), "cljs.core" | "clojure.core")
                    && alias != &import.namespace
                {
                    return Err(format!("reserved core macro alias: {alias}"));
                }
                if let Some(previous) = aliases.insert(alias.clone(), import.namespace.clone())
                    && previous != import.namespace
                {
                    return Err(format!("conflicting macro alias: {alias}"));
                }
                if alias != &import.namespace
                    && (alias == caller || imports.iter().any(|i| &i.namespace == alias))
                {
                    return Err(format!(
                        "macro alias {alias} conflicts with macro namespace qualifier"
                    ));
                }
            }
        }
        for import in &imports {
            for name in &import.refers {
                if matches!(import.namespace.as_str(), "cljs.core" | "clojure.core")
                    && core_macro(name)
                {
                    continue;
                }
                if !self.has_macro(&import.namespace, name)? {
                    return Err(format!(
                        "macro import {}/{name} does not name a public macro",
                        import.namespace
                    ));
                }
            }
        }
        self.imports.insert(caller.to_owned(), imports);
        self.renames.remove(caller);
        self.namespace_symbols.remove(caller);
        self.namespace_docs.remove(caller);
        Ok(())
    }
    pub(crate) fn set_namespace_symbol(
        &mut self,
        namespace: &str,
        symbol: crate::compiler_data::Datum,
        doc: Option<crate::compiler_data::Datum>,
    ) {
        self.namespace_symbols.insert(namespace.to_owned(), symbol);
        if let Some(doc) = doc {
            self.namespace_docs.insert(namespace.to_owned(), doc);
        }
    }
    pub(crate) fn set_renames(&mut self, caller: &str, renames: Vec<(String, String, String)>) {
        self.renames.insert(caller.to_owned(), renames);
    }
    pub(crate) fn macro_analysis_frame(&self) -> Self {
        Self {
            compiler_data_readers: self.compiler_data_readers.clone(),
            imports: self.imports.clone(),
            renames: self.renames.clone(),
            namespace_symbols: self.namespace_symbols.clone(),
            namespace_docs: self.namespace_docs.clone(),
            runtime_vars: self.runtime_vars.clone(),
            namespace_imports: self.namespace_imports.clone(),
            limits: self.limits,
            evaluator_options: self.evaluator_options.clone(),
            assertions_disabled: self.assertions_disabled,
            interactive: self.interactive,
            interactive_control: self.interactive_control.clone(),
            name_policy: self.name_policy.clone(),
            bootstrap: self.bootstrap.clone(),
            bootstrap_seed: true,
            javascript_macros: self.javascript_macros.clone(),
            source_bindings: self.source_bindings.clone(),
            session_ref: self
                .session_handle()
                .map_or_else(std::rc::Weak::new, |s| std::rc::Rc::downgrade(&s)),
            expansions: self.expansions.clone(),
            generated_nodes: self.generated_nodes.clone(),
            ..Self::default()
        }
    }
    pub(crate) fn defers_source_units(&self) -> bool {
        !self.bootstrap_seed
    }
    pub(crate) fn session_handle(
        &self,
    ) -> Option<std::rc::Rc<crate::compilation_session::CompilationSession>> {
        self.session.clone().or_else(|| self.session_ref.upgrade())
    }
    pub(crate) fn has_source_value(&self, namespace: &str, name: &str) -> bool {
        self.source_bindings.get(&format!("{namespace}/{name}")) == Some(&SourceBindingKind::Value)
    }
    pub(crate) fn begin_compilation(&mut self) {
        if !self.bootstrap_seed {
            let mut readers = self.compiler_data_readers.borrow_mut();
            readers.evaluator = None;
            readers.functions.clear();
            readers.default_function = None;
            drop(readers);
            self.bootstrap = None;
            self.javascript_macros.clear();
            self.source_bindings.clear();
            self.session = Some(crate::compilation_session::CompilationSession::new());
            self.session_ref = std::rc::Weak::new();
        }
        self.expansions = Default::default();
        self.generated_nodes = Default::default();
        self.runtime_vars.clear();
        self.namespace_imports.clear();
    }
    pub(crate) fn register_source_binding(
        &mut self,
        namespace: &str,
        name: &str,
        kind: SourceBindingKind,
    ) -> Result<(), String> {
        let key = format!("{namespace}/{name}");
        if self
            .source_bindings
            .get(&key)
            .is_some_and(|previous| *previous != kind)
        {
            return Err(format!(
                "binding {key} cannot change between an ordinary Var and a compile-time macro; use distinct names for the two phases"
            ));
        }
        self.source_bindings.insert(key, kind);
        Ok(())
    }
    pub(crate) fn register_runtime_var(
        &mut self,
        namespace: &str,
        name: &str,
        private: bool,
    ) -> Result<(), String> {
        self.register_source_binding(namespace, name, SourceBindingKind::Value)?;
        self.runtime_vars
            .entry(namespace.to_owned())
            .or_default()
            .insert(name.to_owned(), private);
        Ok(())
    }
    pub(crate) fn source_namespace<'a>(
        &'a self,
        caller: &'a str,
        qualifier: &'a str,
    ) -> Option<&'a str> {
        if caller == qualifier {
            return Some(caller);
        }
        self.imports
            .get(caller)
            .into_iter()
            .flatten()
            .find(|i| i.alias.as_deref() == Some(qualifier) || i.namespace == qualifier)
            .map(|i| i.namespace.as_str())
    }
    pub(crate) fn source_binding_private(&self, namespace: &str, name: &str) -> bool {
        if self
            .runtime_vars
            .get(namespace)
            .and_then(|vars| vars.get(name))
            .copied()
            .unwrap_or(false)
        {
            return true;
        }
        self.javascript_macros
            .get(&format!("{namespace}/{name}"))
            .and_then(|m| m.metadata.as_ref())
            .and_then(|m| {
                m.get(&crate::compiler_data::DataBuilder.keyword(Name::new(None, "private")))
                    .ok()
                    .flatten()
            })
            .is_some_and(|v| {
                !matches!(
                    v.inspect(),
                    crate::compiler_data::DatumView::Nil
                        | crate::compiler_data::DatumView::Boolean(false)
                )
            })
    }
    pub(crate) fn add_namespace_imports(
        &mut self,
        namespace: &str,
        imports: Vec<(String, String)>,
    ) {
        let current = self
            .namespace_imports
            .entry(namespace.to_owned())
            .or_default();
        for import in imports {
            if !current.contains(&import) {
                current.push(import);
            }
        }
    }
    pub(crate) fn set_namespace_imports(
        &mut self,
        namespace: &str,
        imports: Vec<(String, String)>,
    ) {
        self.namespace_imports.insert(namespace.to_owned(), imports);
    }
    pub(crate) fn definition_macro(&self, caller: &str, operator: &str) -> Option<String> {
        let imports = self.imports.get(caller).map(Vec::as_slice).unwrap_or(&[]);
        let target = if let Some((qualifier, name)) = operator.split_once('/') {
            imports
                .iter()
                .find(|i| i.namespace == qualifier || i.alias.as_deref() == Some(qualifier))
                .map(|i| (i.namespace.as_str(), name))
        } else if let Some((_, ns, name)) = self
            .renames
            .get(caller)
            .into_iter()
            .flatten()
            .find(|(local, _, _)| local == operator)
        {
            Some((ns.as_str(), name.as_str()))
        } else {
            imports
                .iter()
                .find(|i| i.refers.iter().any(|n| n == operator))
                .map(|i| (i.namespace.as_str(), operator))
        };
        target
            .filter(|(ns, name)| is_definition_macro(ns, name))
            .map(|(_, name)| name.into())
    }

    pub(crate) fn has_macro(&mut self, namespace: &str, name: &str) -> Result<bool, String> {
        if matches!(namespace, "cljs.core" | "clojure.core") {
            return Ok(core_macro(name));
        }
        if self
            .javascript_macros
            .contains_key(&format!("{namespace}/{name}"))
        {
            return Ok(!self.source_binding_private(namespace, name));
        }
        Ok(false)
    }
    pub(crate) fn resolves_macro(
        &mut self,
        caller: &str,
        operator: &str,
        shadow_core: bool,
    ) -> Result<bool, String> {
        match self.macro_name(caller, operator, shadow_core) {
            Some((namespace, name)) => self.has_macro(&namespace, &name),
            None => Ok(false),
        }
    }

    pub(crate) fn macro_name(
        &self,
        caller: &str,
        operator: &str,
        shadow_core: bool,
    ) -> Option<(String, String)> {
        let imports = self.imports.get(caller).map(Vec::as_slice).unwrap_or(&[]);
        if let Some((qualifier, name)) = operator.split_once('/') {
            if qualifier == "cljs.core" || qualifier == "clojure.core" {
                core_macro(name).then(|| ("clojure.core".to_owned(), name.to_owned()))
            } else {
                imports
                    .iter()
                    .find(|i| i.namespace == qualifier || i.alias.as_deref() == Some(qualifier))
                    .map(|i| (i.namespace.clone(), name.to_owned()))
            }
        } else if let Some((_, namespace, name)) = self
            .renames
            .get(caller)
            .into_iter()
            .flatten()
            .find(|(local, _, _)| local == operator)
        {
            Some((namespace.clone(), name.clone()))
        } else {
            imports
                .iter()
                .find(|i| i.refers.iter().any(|n| n == operator))
                .map(|i| (i.namespace.clone(), operator.to_owned()))
                .or_else(|| {
                    (!shadow_core && core_macro(operator))
                        .then(|| ("clojure.core".to_owned(), operator.to_owned()))
                })
        }
    }
    pub(crate) fn expand_compiler_seed(
        &mut self,
        form: &Form,
        name: &Name,
    ) -> Result<Option<Form>, String> {
        let builder = crate::compiler_data::DataBuilder;
        let extended = crate::compiler_destructuring::expand(&builder, form, name)?;
        let parts = form.data().elements()?;
        let operation = String::from_utf16_lossy(name.name());
        let minimum = match operation.as_ref() {
            "let" | "loop" | "letfn" | "fn" => 1,
            "defn" | "defn-" => 2,
            _ => 0,
        };
        if parts.len() < minimum + 1 {
            return Err(format!("{operation} requires arguments"));
        }
        let mut args = vec![builder.nil(), builder.nil()];
        args.extend_from_slice(&parts[1..]);
        let expanded = if let Some(expanded) = extended {
            Some(expanded)
        } else {
            use crate::compiler_binding_macros as seed;
            let data = match String::from_utf16_lossy(name.name()).as_str() {
                "let" => Some(seed::let_macro(&mut { builder }, &args)),
                "loop" => Some(seed::loop_macro(&mut { builder }, &args)),
                "fn" => Some(seed::fn_macro(&mut { builder }, &args)),
                "letfn" => Some(seed::letfn_macro(&mut { builder }, &args)),
                "defn" => Some(seed::defn_macro(&mut { builder }, &args)),
                "defn-" => Some(seed::private_defn(&mut { builder }, &args)),
                "assert" => {
                    if !(2..=3).contains(&parts.len()) {
                        return Err("assert requires a condition and optional message".into());
                    }
                    Some(seed::call(&builder, "compiler-assert*", &parts[1..]))
                }
                _ => None,
            };
            data.transpose()?
                .map(|data| Form::from_expansion(data, form, name.clone()))
        };
        let expanded = match expanded {
            Some(form) => Some(form),
            None => self.expand_namespace_reflection(form, name)?,
        };
        if let Some(expanded) = &expanded {
            if self.expansions.get() >= self.limits.expansions {
                return Err("compiler macro expansion limit exceeded".into());
            }
            self.expansions.set(self.expansions.get() + 1);
            self.generated_nodes.set(
                self.generated_nodes
                    .get()
                    .saturating_add(expanded.nodes().len()),
            );
            if self.generated_nodes.get() > self.limits.generated_nodes {
                return Err("compiler macro generated-node limit exceeded".into());
            }
        }
        Ok(expanded)
    }
    /// Macro identity/metadata is source reflection; its executable closure remains private.
    pub(crate) fn reflected_macro(
        &self,
        caller: &str,
        requested: &str,
    ) -> Option<(String, String, Option<crate::compiler_data::Datum>)> {
        if let Some(key) = self.source_macro_key(caller, requested)
            && let Some(declaration) = self.javascript_macros.get(&key)
        {
            let (namespace, name) = key.split_once('/')?;
            return Some((namespace.into(), name.into(), declaration.metadata.clone()));
        }
        let (namespace, name) = requested
            .split_once('/')
            .unwrap_or(("cljs.core", requested));
        if matches!(namespace, "cljs.core" | "clojure.core")
            && !self.has_source_value(caller, requested)
            && crate::CoreFnId::resolve(name).is_none()
            && (core_macro(name) || is_definition_macro(namespace, name))
        {
            return Some(("cljs.core".into(), name.into(), None));
        }
        None
    }
    fn expand_namespace_reflection(
        &self,
        form: &Form,
        name: &Name,
    ) -> Result<Option<Form>, String> {
        let operation = String::from_utf16_lossy(name.name());
        let operation = operation.strip_prefix("compiler-").unwrap_or(&operation);
        if !matches!(
            operation,
            "ns-publics" | "ns-interns" | "ns-imports" | "ns-refers"
        ) {
            return Ok(None);
        }
        let run = || -> Result<_, Box<dyn std::error::Error>> {
            let parts = form.data().elements()?;
            let quoted = if parts.len() == 2 {
                parts[1].elements().unwrap_or_default()
            } else {
                vec![]
            };
            if quoted.len() != 2
                || !matches!(quoted[0].inspect(), crate::compiler_data::DatumView::Symbol(n) if n == Name::new(None,"quote"))
                || !matches!(
                    quoted[1].inspect(),
                    crate::compiler_data::DatumView::Symbol(_)
                )
            {
                return Err(format!("Argument to {operation} must be a quoted symbol").into());
            }
            let crate::compiler_data::DatumView::Symbol(namespace) = quoted[1].inspect() else {
                unreachable!()
            };
            let namespace = String::from_utf16(namespace.name())?;
            let mut declarations = if operation == "ns-refers" {
                self.reader_refers(&namespace)
                    .into_iter()
                    .map(|(local, ns, original)| (local, format!("{ns}/{original}")))
                    .collect()
            } else if operation == "ns-imports" {
                self.namespace_imports
                    .get(&namespace)
                    .cloned()
                    .unwrap_or_default()
            } else {
                self.runtime_vars
                    .get(&namespace)
                    .into_iter()
                    .flat_map(|vars| vars.iter())
                    .filter(|(_, private)| operation == "ns-interns" || !**private)
                    .map(|(local, _)| (local.clone(), format!("{namespace}/{local}")))
                    .collect()
            };
            if matches!(operation, "ns-interns" | "ns-publics") {
                if matches!(namespace.as_str(), "cljs.core" | "clojure.core") {
                    declarations.extend(
                        crate::compiler_reader::core_public_bindings()
                            .filter(|(local, _)| {
                                crate::CoreFnId::resolve(local).is_some()
                                    || crate::compiler_names::core_macro(local)
                                    || crate::compiler_names::is_definition_macro(
                                        "cljs.core",
                                        local,
                                    )
                            })
                            .map(|(local, _)| (local.into(), format!("cljs.core/{local}"))),
                    );
                }
                declarations.extend(self.javascript_macros.keys().filter_map(|key| {
                    let (ns, local) = key.split_once('/')?;
                    (ns == namespace
                        && (operation == "ns-interns" || !self.source_binding_private(ns, local)))
                    .then(|| (local.into(), key.clone()))
                }));
            }
            declarations.sort();
            declarations.dedup();
            let c = crate::compiler_data::DataBuilder;
            let symbol = |s: &str| c.symbol(Name::new(None, s));
            let mut entries = Vec::new();
            for (local, qualified) in declarations {
                let key = c.list(&[symbol("quote"), symbol(&local)])?;
                let value = if operation == "ns-imports" {
                    c.symbol(Name::new(Some("js"), &qualified))
                } else {
                    let (namespace, local) = qualified.split_once('/').unwrap();
                    c.list(&[symbol("var"), c.symbol(Name::new(Some(namespace), local))])?
                };
                entries.push((key, value));
            }
            Ok(Form::from_expansion(c.map(&entries)?, form, name.clone()))
        };
        run().map(Some).map_err(|e| e.to_string())
    }
    pub(crate) fn register_macro_view(&self, view: &mut crate::compiler_reader::NamespaceView) {
        let mut names = self.javascript_macros.keys().collect::<Vec<_>>();
        names.sort();
        for (index, key) in names.into_iter().enumerate() {
            let (namespace, local) = key.split_once('/').unwrap();
            let name = Name::new(Some(namespace), local);
            let binding = crate::compiler_reader::ResolvedBinding {
                id: crate::compiler_reader::BindingId(usize::MAX / 2 - index),
                kind: crate::compiler_reader::BindingKind::Macro,
                name: name.clone(),
            };
            view.bindings.insert(name.clone(), binding.clone());
            if view.namespace == Name::new(None, namespace) {
                view.bindings
                    .insert(Name::new(None, local), binding.clone());
            }
            for (local, target) in &view.refers {
                if target == &name {
                    view.bindings.insert(local.clone(), binding.clone());
                }
            }
        }
    }
    pub(crate) fn reader_refers(&self, namespace: &str) -> Vec<(String, String, String)> {
        let mut names = self
            .imports
            .get(namespace)
            .into_iter()
            .flatten()
            .flat_map(|import| {
                import
                    .refers
                    .iter()
                    .map(|name| (name.clone(), import.namespace.clone(), name.clone()))
            })
            .collect::<Vec<_>>();
        names.extend(self.renames.get(namespace).into_iter().flatten().cloned());
        names
    }
    pub(crate) fn reader_aliases(
        &self,
        namespace: &str,
        runtime: &[(String, String)],
        requires: &[String],
    ) -> Vec<(String, String)> {
        let mut aliases = requires
            .iter()
            .map(|n| (n.clone(), n.clone()))
            .collect::<Vec<_>>();
        aliases.extend_from_slice(runtime);
        if let Some(imports) = self.imports.get(namespace) {
            for import in imports {
                aliases.push((import.namespace.clone(), import.namespace.clone()));
                if let Some(alias) = &import.alias {
                    aliases.push((alias.clone(), import.namespace.clone()));
                }
            }
        }
        aliases
    }
    pub(crate) fn source_macro_names(&self, namespace: &str) -> Vec<String> {
        let prefix = format!("{namespace}/");
        let mut names = self
            .javascript_macros
            .keys()
            .filter_map(|key| key.strip_prefix(&prefix).map(str::to_owned))
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    pub(crate) fn reader_environment(
        &self,
        environment: crate::compiler_data::Datum,
        namespace: &str,
        refers: &[(String, String, String)],
        excludes: &[String],
        globals: &[crate::GlobalBinding],
    ) -> Result<crate::compiler_data::Datum, String> {
        let c = crate::compiler_data::DataBuilder;
        let string = |s: &str| c.string(s.encode_utf16().collect::<Vec<_>>());
        let key = |s| c.keyword(Name::new(None, s));
        let context = c
            .map(&[
                (
                    key("refers"),
                    c.vector(
                        &refers
                            .iter()
                            .chain(self.reader_refers(namespace).iter())
                            .map(|(local, ns, name)| {
                                c.vector(&[string(local), string(ns), string(name)])
                            })
                            .collect::<Result<Vec<_>, _>>()?,
                    )
                    .map_err(|e| e.to_string())?,
                ),
                (
                    key("excludes"),
                    c.vector(&excludes.iter().map(|s| string(s)).collect::<Vec<_>>())
                        .map_err(|e| e.to_string())?,
                ),
                (
                    key("bindings"),
                    c.vector(
                        &globals
                            .iter()
                            .filter(|g| g.namespace == namespace)
                            .map(|g| string(&g.name))
                            .chain(
                                self.source_macro_names(namespace)
                                    .iter()
                                    .map(|name| string(name)),
                            )
                            .collect::<Vec<_>>(),
                    )
                    .map_err(|e| e.to_string())?,
                ),
            ])
            .map_err(|e| e.to_string())?;
        c.assoc(
            &environment,
            &c.keyword(Name::new(Some("light-cljs"), "reader-context")),
            &context,
        )
    }

    pub fn compiler_environment(
        &self,
        namespace: &str,
        context: ExprContext,
        locals: &[(String, usize, &'static str, Option<usize>)],
        aliases: &[(String, String)],
        requires: &[String],
        location: (usize, usize),
    ) -> Result<crate::compiler_data::Datum, String> {
        let imports = self.imports.get(namespace).cloned().unwrap_or_default();
        let namespace_symbol = self.namespace_symbols.get(namespace).cloned();
        let namespace_doc = self.namespace_docs.get(namespace).cloned();
        let namespace_imports = self
            .namespace_imports
            .get(namespace)
            .cloned()
            .unwrap_or_default();
        let c = crate::compiler_data::DataBuilder;
        let key = |s: &str| {
            let name = s.split_once('/').map_or_else(
                || Name::new(None, s),
                |(namespace, name)| Name::new(Some(namespace), name),
            );
            c.keyword(name)
        };
        let sym = |s: &str| c.symbol(Name::new(None, s));
        let build = || -> Result<crate::compiler_data::Datum, String> {
            let mut local_map = c.map(&[])?;
            for (name, id, kind, argument) in locals {
                let name = sym(name);
                let mut details = vec![
                    (key("name"), name.clone()),
                    (key("form"), name.clone()),
                    (key("op"), key("binding")),
                    (key("local"), key(kind)),
                    (key("light-cljs/id"), c.integer(*id as i64)),
                ];
                if *kind != "fn" {
                    details.push((key("binding-form?"), c.boolean(true)));
                }
                if let Some(argument) = argument {
                    details.push((key("arg-id"), c.integer(*argument as i64)));
                }
                if let Some(shadow) = local_map.get(&name)? {
                    details.push((key("shadow"), shadow));
                }
                local_map = c.assoc(&local_map, &name, &c.map(&details)?)?;
            }
            let mut macro_aliases = Vec::new();
            let mut macro_refers = Vec::new();
            for import in &imports {
                macro_aliases.push((sym(&import.namespace), sym(&import.namespace)));
                if let Some(alias) = &import.alias {
                    macro_aliases.push((sym(alias), sym(&import.namespace)));
                }
                for name in &import.refers {
                    macro_refers.push((sym(name), sym(&import.namespace)));
                }
            }
            for (local, target, _) in self.renames.get(namespace).into_iter().flatten() {
                macro_refers.push((sym(local), sym(target)));
            }
            let mut runtime = requires
                .iter()
                .map(|n| (sym(n), sym(n)))
                .collect::<Vec<_>>();
            runtime.extend(aliases.iter().map(|(a, n)| (sym(a), sym(n))));
            runtime.extend(macro_aliases);
            let import_names = namespace_imports
                .iter()
                .map(|(a, n)| (sym(a), sym(n)))
                .collect::<Vec<_>>();
            runtime.extend(import_names.iter().cloned());
            // Repeated ordinary requires and shared phase aliases have one
            // identity in the metadata map, just as in name resolution.
            let mut seen_names = HashSet::new();
            runtime.retain(|(key, _)| match key.inspect() {
                crate::compiler_data::DatumView::Symbol(name) => seen_names.insert(name),
                _ => unreachable!("namespace map keys are symbols"),
            });
            c.map(&[
                (key("locals"), local_map),
                // Descriptive compiler bindings only: no runtime Var or callable
                // macro value crosses this metadata service.
                (
                    c.keyword(Name::new(Some("light-cljs"), "macros")),
                    c.map(&{
                        let mut declarations = self.javascript_macros.iter().collect::<Vec<_>>();
                        declarations.sort_by(|a, b| a.0.cmp(b.0));
                        declarations
                            .into_iter()
                            .map(|(name, declaration)| {
                                let (ns, local) = name.split_once('/').unwrap();
                                (
                                    c.symbol(Name::new(Some(ns), local)),
                                    declaration.metadata.clone().unwrap_or_else(|| c.nil()),
                                )
                            })
                            .collect::<Vec<_>>()
                    })?,
                ),
                (
                    key("context"),
                    key(match context {
                        ExprContext::Value => "expr",
                        ExprContext::Discard => "statement",
                        ExprContext::Return => "return",
                    }),
                ),
                (
                    key("ns"),
                    c.map(&[
                        (
                            key("name"),
                            namespace_symbol.clone().unwrap_or_else(|| sym(namespace)),
                        ),
                        (key("doc"), namespace_doc.clone().unwrap_or_else(|| c.nil())),
                        (key("requires"), c.map(&runtime)?),
                        (key("imports"), c.map(&import_names)?),
                        (key("uses"), c.map(&macro_refers)?),
                        (key("require-macros"), c.map(&[])?),
                        (key("use-macros"), c.map(&[])?),
                    ])?,
                ),
                (key("line"), c.integer(location.0 as i64)),
                (key("column"), c.integer(location.1 as i64)),
            ])
        };
        build().map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod compiler_pure_tests {
    use super::*;
    #[test]
    fn reader_seed_declarations_and_metadata_use_owned_forms() {
        let mut host = MacroHost::new();
        for text in [
            "(let [{:keys [x] :or {x 4}} {}] (assert (= x 4)) x)",
            "(defn f [x] {:pre [(number? x)]} (+ x 1)) (f 2)",
            "(loop [x 2] (if (= x 0) x (recur (dec x))))",
            "(letfn [(f [x] (if (= x 0) x (f (dec x))))] (f 2))",
            "(defprotocol P (value [this])) (deftype T [] P (value [this] 3)) (value (T.))",
            "(defrecord R [x]) (:x (->R 3))",
            "(meta #'+)",
            "(def x 1) (ns-publics 'cljs.user)",
            "`[+ unknown ~(+ 1 2) ~@[3]]",
        ] {
            crate::compile_with_macros(text, &mut host).unwrap_or_else(|e| panic!("{text}: {e}"));
        }
        host.set_imports(
            "cljs.user",
            vec![MacroImport {
                namespace: "cljs.core".into(),
                alias: Some("c".into()),
                refers: vec!["let".into(), "ns-publics".into()],
            }],
        )
        .unwrap();
        crate::compile_with_macros("(c/let [x 3] x)", &mut host).unwrap();
        let source = crate::compiler_data::Source::new("reader.cljs", "^:kept #:x{:z [1] :a 2}");
        let form = host.read_compiler_form(&source).unwrap().unwrap();
        assert_eq!(form.children()[0].span().text(), ":z");
        assert!(form.metadata().is_some());
    }
}

impl MacroHost {
    pub(crate) fn check_macro_policy(
        &self,
        namespace: &str,
        target: &str,
        shadow_core: bool,
    ) -> Result<(), String> {
        let Some(policy) = &self.name_policy else {
            return Ok(());
        };
        let name = self.source_macro_key(namespace, target).or_else(|| {
            self.macro_name(namespace, target, shadow_core)
                .map(|(ns, name)| format!("{ns}/{name}"))
        });
        if let Some(name) = name {
            let (ns, member) = name.split_once('/').unwrap();
            if !policy.permits(&Name::new(Some(ns), member), false) {
                return Err(format!("name policy denies {name}"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod compiler_default_reader_tests {
    use super::*;
    use crate::compiler_data::{DatumKind, Source};

    #[test]
    fn default_reader_is_compiled_typed_and_preserves_reader_precedence() {
        fn number(form: &Form) -> f64 {
            match form.data().kind() {
                DatumKind::Integer(n) => *n as f64,
                DatumKind::Double(n) => *n,
                _ => panic!("expected number"),
            }
        }
        let mut host = MacroHost::new();
        host.register_compiler_default_data_reader("(fn [tag value] [(str tag) value])")
            .unwrap();
        host.register_compiler_data_reader(Name::new(None, "named"), "(fn [value] (+ value 1))")
            .unwrap();
        let read = |host: &mut MacroHost, text: &str| {
            host.read_compiler_form(&Source::new("reader.cljs", text))
                .unwrap()
                .unwrap()
        };
        let form = read(&mut host, "#demo/missing 7");
        let children = form.children();
        assert!(
            matches!(children[0].data().kind(), DatumKind::String(text) if String::from_utf16_lossy(text) == "demo/missing")
        );
        assert_eq!(number(&children[1]), 7.0);
        assert_eq!(number(&read(&mut host, "#named 7")), 8.0);
        assert!(matches!(
            read(&mut host, "#inst \"2020-01-01\"").data().kind(),
            DatumKind::CompilerLiteral { .. }
        ));
        host.begin_compilation();
        assert!(
            host.compiler_data_readers
                .borrow()
                .default_function
                .is_none()
        );
        assert_eq!(read(&mut host, "#unknown 9").children().len(), 2);
        host.register_compiler_default_data_reader("(fn [tag value] 42)")
            .unwrap();
        assert_eq!(number(&read(&mut host, "#unknown 9")), 42.0);
    }
}
