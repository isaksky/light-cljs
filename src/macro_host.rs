//! Native compile-time execution, independent of runtime globals and JS paths.
use std::collections::{HashMap, HashSet};

use crate::analyzer::ExprContext;
use crate::compiler_data::{Form, Name};
use crate::compiler_names::{core_macro, is_definition_macro};
use crate::interpreter::{
    BindingGuard, Context, Form as NativeForm, NativeDefinition, Reload, ResourceLimits,
    RootedValue, ValueView,
};

/// An explicit compile-time namespace import. Runtime aliases are separate.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MacroImport {
    pub namespace: String,
    pub alias: Option<String>,
    pub refers: Vec<String>,
}

/// Per-compilation expansion bounds, in addition to interpreter resource limits.
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

/// A reusable native interpreter for compiler macros. Default compilation owns a
/// fresh host; explicitly reusing one retains macro Vars and namespace load state.
/// No runtime compiler globals or JavaScript capabilities enter this context.
#[derive(Default)]
pub struct MacroHost {
    context: Option<Context>,
    imports: HashMap<String, Vec<MacroImport>>,
    native_namespaces: HashSet<String>,
    renames: HashMap<String, Vec<(String, String, String)>>,
    namespace_symbols: HashMap<String, crate::compiler_data::Datum>,
    namespace_docs: HashMap<String, crate::compiler_data::Datum>,
    runtime_vars: HashMap<String, HashMap<String, bool>>,
    namespace_imports: HashMap<String, Vec<(String, String)>>,
    pub limits: MacroLimits,
    /// Configuration only: JS roots belong to each fresh compilation session.
    pub evaluator_options: crate::evaluator::EvaluatorOptions,
    assertions_disabled: bool,
    interop_macros_loaded: bool,
    sequence_macros_loaded: bool,
    runtime_defmacro_loaded: bool,
    expansions: usize,
    generated_nodes: usize,
}

impl MacroHost {
    /// Allocate a fresh synchronous heap for a clean compilation.
    /// The legacy native macro adapter is removed by light-cljs-115.
    pub fn new_evaluator(&self) -> Result<crate::evaluator::Evaluator, crate::evaluator::EvalError> {
        crate::evaluator::Evaluator::new(self.evaluator_options.clone())
    }

    pub fn new() -> Self {
        Self::default()
    }
    /// Control assertion emission for this host (enabled by default). Disabled
    /// assertions omit both condition and message, including function pre/post checks.
    pub fn set_assertions(&mut self, enabled: bool) {
        self.assertions_disabled = !enabled;
        if let Some(context) = &mut self.context {
            context.compiler_assertions(enabled);
        }
    }
    pub fn assertions_enabled(&self) -> bool {
        !self.assertions_disabled
    }
    /// Interpreter effects, limits and source providers are explicit embedding APIs.
    pub fn context_mut(&mut self) -> &mut Context {
        let assertions = self.assertions_enabled();
        self.context.get_or_insert_with(|| {
            let mut context = Context::new();
            context.initialize_compiler_host();
            context.compiler_assertions(assertions);
            context.set_resource_limits(ResourceLimits {
                fuel: Some(1_000_000),
                allocation: Some(2_000_000),
                ..ResourceLimits::default()
            });
            context
        })
    }
    /// Read immutable compiler-owned data without initializing an evaluator.
    /// Explicit native data-reader callbacks use the temporary adapter.
    pub fn read_compiler_form(
        &mut self,
        source: &crate::compiler_data::Source,
    ) -> Result<Option<Form>, crate::compiler_data::ReadError> {
        if let Some(context) = &self.context {
            if context.has_custom_data_readers() {
                return crate::native_form_adapter::read_form(context, source);
            }
        }
        crate::compiler_reader::read_form(
            source,
            &crate::compiler_reader::NamespaceView::new("cljs.user", source),
        )
    }
    pub(crate) fn lower_form_in_view(
        &self,
        source: &crate::compiler_data::Source,
        tree: &crate::SyntaxTree,
        root: crate::NodeId,
        view: &crate::compiler_reader::NamespaceView,
        discards: &[crate::NodeId],
    ) -> Result<Form, crate::compiler_data::ReadError> {
        if let Some(context) = &self.context {
            if context.has_custom_data_readers() {
                let aliases = view
                    .aliases
                    .iter()
                    .map(|(a, n)| {
                        (
                            String::from_utf16_lossy(a.name()),
                            String::from_utf16_lossy(n.name()),
                        )
                    })
                    .collect::<Vec<_>>();
                let globals = view
                    .refers
                    .iter()
                    .map(|(a, n)| {
                        (
                            String::from_utf16_lossy(a.name()),
                            String::from_utf16_lossy(n.namespace().unwrap()),
                            String::from_utf16_lossy(n.name()),
                        )
                    })
                    .collect::<Vec<_>>();
                return crate::native_form_adapter::lower_form(
                    context,
                    &view.namespace,
                    source,
                    tree,
                    root,
                    &aliases,
                    discards,
                    &globals,
                );
            }
        }
        crate::compiler_reader::lower_form(source, tree, root, view, discards)
    }
    pub fn register_macro(&mut self, definition: &'static NativeDefinition) -> RootedValue {
        self.native_namespaces
            .insert(definition.metadata.namespace.to_owned());
        self.context_mut().register_macro(definition)
    }
    /// Load `.clj`/`.cljc` through the interpreter's explicitly supplied provider.
    /// Reload observes updated Var roots; expanded compiler forms are never cached.
    pub fn load_namespace(&mut self, namespace: &str, reload: Reload) -> Result<(), String> {
        if matches!(namespace, "cljs.core" | "clojure.core")
            || (reload == Reload::None && self.native_namespaces.contains(namespace))
        {
            return Ok(());
        }
        let c = self.context_mut();
        let require = c
            .find_interned_var(
                &Name::new(None, "clojure.core"),
                &Name::new(None, "require"),
            )
            .map_err(|e| e.to_string())?
            .expect("core require");
        let mut args = vec![c.symbol(Name::new(None, namespace))];
        if reload != Reload::None {
            args.push(c.keyword(Name::new(
                None,
                if reload == Reload::All {
                    "reload-all"
                } else {
                    "reload"
                },
            )));
        }
        c.invoke(&require, &args).map_err(|e| e.to_string())?;
        Ok(())
    }
    /// Configure imports without loading or evaluating code. Filesystem compilation
    /// loads them separately; embedders may pre-register native/interpreted macros.
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
                if aliases
                    .insert(alias.clone(), import.namespace.clone())
                    .is_some()
                {
                    return Err(format!("duplicate macro alias: {alias}"));
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
                if self.find_macro(&import.namespace, name)?.is_none() {
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
    pub(crate) fn begin_compilation(&mut self) {
        self.expansions = 0;
        self.generated_nodes = 0;
        self.runtime_vars.clear();
        self.namespace_imports.clear();
    }
    pub(crate) fn register_runtime_var(&mut self, namespace: &str, name: &str, private: bool) {
        self.runtime_vars
            .entry(namespace.to_owned())
            .or_default()
            .insert(name.to_owned(), private);
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

    pub(crate) fn find_macro(
        &mut self,
        namespace: &str,
        name: &str,
    ) -> Result<Option<RootedValue>, String> {
        let namespace = if matches!(namespace, "cljs.core" | "clojure.core") {
            if !core_macro(name) {
                return Ok(None);
            }
            "clojure.core"
        } else {
            namespace
        };
        if namespace == "clojure.core" && name == "defmacro" && !self.runtime_defmacro_loaded {
            let c = self.context_mut();
            let ns_var = c
                .find_interned_var(&Name::new(None, "clojure.core"), &Name::new(None, "*ns*"))
                .map_err(|e| e.to_string())?
                .unwrap();
            let ns = c.find_namespace(&Name::new(None, "clojure.core")).unwrap();
            let _guard = c
                .bind_vars(&[(ns_var.var_id().unwrap(), ns)])
                .map_err(|e| e.to_string())?;
            // Runtime CLJS defmacro adds the compiler arguments to every method;
            // it does not register this runtime function in the native macro host.
            c.eval_string(r#"
              (defmacro compiler-runtime-defmacro [name & args]
                (let [doc (when (string? (first args)) (first args))
                      args (if doc (next args) args)
                      attributes (if (map? (first args)) (first args) {})
                      fdecl (if (map? (first args)) (next args) args)
                      fdecl (if (vector? (first fdecl)) (list fdecl) fdecl)
                      arglists (list 'quote (apply list (map first (filter seq? fdecl))))
                      attributes (if (contains? attributes :arglists) attributes
                                   (assoc attributes :arglists arglists))
                      macro-name (with-meta name (assoc (meta name) :macro true))
                      methods (map (fn [method]
                                     (if (map? method) method
                                       (cons (vec (cons '&form (cons '&env (first method)))) (next method)))) fdecl)
                      declaration (concat (list macro-name) (when doc (list doc)) (list attributes) methods)
                      result (gensym "macro-result")]
                  (list 'cljs.core/let [result (cons 'cljs.core/defn declaration)]
                    (list 'set! (list '. name '-cljs$lang$macro) true) result)))
            "#).map_err(|e| e.to_string())?;
            self.runtime_defmacro_loaded = true;
        }
        if namespace == "clojure.core"
            && matches!(
                name,
                "memfn" | "amap" | "areduce" | ".." | "js-obj" | "doto"
            )
            && !self.interop_macros_loaded
        {
            let c = self.context_mut();
            let ns = c
                .find_namespace(&Name::new(None, "clojure.core"))
                .expect("core namespace");
            let ns_var = c
                .find_interned_var(&Name::new(None, "clojure.core"), &Name::new(None, "*ns*"))
                .map_err(|e| e.to_string())?
                .expect("core ns Var");
            let _guard = c
                .bind_vars(&[(ns_var.var_id().unwrap(), ns)])
                .map_err(|e| e.to_string())?;
            c.eval_string(r#"
              (defmacro doto [x & forms]
                (let [target (gensym "target")]
                  (list* 'let [target x]
                    (concat
                      (map (fn [form]
                             (if (seq? form)
                               (list* (first form) target (next form))
                               (list form target))) forms)
                      [target]))))
              (defmacro js-obj [& entries]
                (let [raw-pairs (map vec (partition 2 entries))
                      ;; JVM small maps retain insertion order; the native
                      ;; interpreter's hash-map iteration must not reorder js-keys.
                      pairs (if (<= (count raw-pairs) 8)
                              (reduce (fn [pairs pair]
                                (if (some #(= (first %) (first pair)) pairs)
                                  (mapv #(if (= (first %) (first pair)) pair %) pairs)
                                  (conj pairs pair))) [] raw-pairs)
                              (into {} raw-pairs))
                      strings (filter (fn [p] (string? (first p))) pairs)
                      symbols (filter (fn [p] (symbol? (first p))) pairs)
                      expressions (filter (fn [p] (not (or (string? (first p)) (symbol? (first p))))) pairs)
                      bound (mapv (fn [p] [(gensym "key") (first p) (second p)]) expressions)
                      obj (gensym "object")]
                  (list* 'let
                    (vec (concat (mapcat (fn [p] [(first p) (second p)]) bound)
                           [obj (cons 'js* (cons
                             (str "({" (apply str (interpose "," (repeat (count strings) "~{}:~{}"))) "})")
                             (mapcat identity strings)))]))
                    (concat
                      (map (fn [p] (list 'cljs.core/unchecked-set obj (first p) (second p))) symbols)
                      (map (fn [p] (list 'cljs.core/unchecked-set obj (first p) (nth p 2))) bound)
                      [obj]))))
              (defmacro memfn [name & args]
                (let [target (gensym "target")]
                  (list 'fn (vec (cons target args)) (list '. target (cons name args)))))
              (defmacro amap [a idx ret expr]
                (let [arr (gensym "array") len (gensym "length")]
                  (list 'let [arr a len (list 'cljs.core/alength arr) ret (list 'cljs.core/aclone arr)]
                    (list 'loop [idx 0]
                      (list 'if (list 'cljs.core/< idx len)
                        (list 'do (list 'cljs.core/aset ret idx expr) (list 'recur (list 'cljs.core/inc idx))) ret)))))
              (defmacro areduce [a idx ret init expr]
                (let [arr (gensym "array") len (gensym "length")]
                  (list 'let [arr a len (list 'cljs.core/alength arr)]
                    (list 'loop [idx 0 ret init]
                      (list 'if (list 'cljs.core/< idx len)
                        (list 'recur (list 'cljs.core/inc idx) expr) ret)))))
              (defmacro .. [x form & more]
                (let [next (list '. x form)]
                  (if (seq more) (cons 'clojure.core/.. (cons next more)) next)))
            "#).map_err(|e| e.to_string())?;
            self.interop_macros_loaded = true;
        }
        if namespace == "clojure.core"
            && matches!(name, "for" | "doseq" | "lazy-cat" | "selector")
            && !self.sequence_macros_loaded
        {
            let c = self.context_mut();
            let ns_var = c
                .find_interned_var(&Name::new(None, "clojure.core"), &Name::new(None, "*ns*"))
                .map_err(|e| e.to_string())?
                .unwrap();
            let ns = c.find_namespace(&Name::new(None, "clojure.core")).unwrap();
            let _guard = c
                .bind_vars(&[(ns_var.var_id().unwrap(), ns)])
                .map_err(|e| e.to_string())?;
            c.eval_string(include_str!("compiler_sequence_macros.clj"))
                .map_err(|e| e.to_string())?;
            c.eval_string(
                "(defmacro lazy-cat [& colls] (cons 'cljs.core/concat (map (fn [coll] (list 'cljs.core/lazy-seq coll)) colls)))",
            ).map_err(|e| e.to_string())?;
            // Same directive selection and result contract as checkout
            // cljs.core/selector-impl, using the compiler destructuring path.
            c.eval_string(r#"
              (defmacro selector [m]
                (when-not (map? m)
                  (throw (ex-info "expected a map" {})))
                (let [dirs [:select :excess :missing :all]
                      names (into {} (map (fn [dir] [dir (gensym)]) (filter m dirs)))]
                  (when (empty? names)
                    (throw (ex-info "form must contain at least one of :select :excess :missing :all" {})))
                  (let [argument (gensym "map")]
                    (list 'cljs.core/fn (gensym "selector") [argument]
                      (list 'cljs.core/let [(merge m names) argument]
                        (if (= 1 (count names))
                          (second (first names))
                          (list 'cljs.core/some-vals names)))))))
            "#).map_err(|e| e.to_string())?;
            self.sequence_macros_loaded = true;
        }
        let name = if namespace == "clojure.core" && name == "defmacro" {
            "compiler-runtime-defmacro"
        } else if namespace == "clojure.core" && matches!(name, "for" | "doseq") {
            if name == "for" {
                "sequence-for"
            } else {
                "sequence-doseq"
            }
        } else {
            name
        };
        let c = self.context_mut();
        let var = c
            .find_interned_var(&Name::new(None, namespace), &Name::new(None, name))
            .map_err(|e| e.to_string())?;
        let Some(var) = var else { return Ok(None) };
        let Some(meta) = var.metadata() else {
            return Ok(None);
        };
        let truth = |key| {
            meta.get(&c.keyword(Name::new(None, key)))
                .ok()
                .flatten()
                .is_some_and(|v| !matches!(v.inspect(), ValueView::Nil | ValueView::Boolean(false)))
        };
        Ok((truth("macro") && !truth("private")).then_some(var))
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
    pub(crate) fn resolve(
        &mut self,
        caller: &str,
        operator: &str,
        shadow_core: bool,
    ) -> Result<Option<RootedValue>, String> {
        match self.macro_name(caller, operator, shadow_core) {
            Some((namespace, name)) => self.find_macro(&namespace, &name),
            None => Ok(None),
        }
    }
    pub(crate) fn expand_compiler_seed(
        &mut self,
        form: &Form,
        name: &Name,
    ) -> Result<Option<Form>, String> {
        let started = self
            .context
            .as_ref()
            .is_some_and(Context::compiler_diagnostics_enabled)
            .then(std::time::Instant::now);
        let builder = crate::compiler_data::DataBuilder;
        let extended = crate::compiler_destructuring::expand(&builder, form, &name)?;
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
            if self.expansions >= self.limits.expansions {
                return Err("compiler macro expansion limit exceeded".into());
            }
            self.expansions += 1;
            self.generated_nodes = self.generated_nodes.saturating_add(expanded.nodes().len());
            if self.generated_nodes > self.limits.generated_nodes {
                return Err("compiler macro generated-node limit exceeded".into());
            }
        }
        if let (Some(started), Some(expanded), Some(context)) =
            (started, &expanded, &mut self.context)
        {
            context.record_compiler_expansion(
                Name::from_utf16(Some(Name::new(None, "clojure.core").name()), name.name()),
                expanded.nodes().len(),
                started.elapsed(),
            );
        }
        Ok(expanded)
    }
    pub(crate) fn expand(
        &mut self,
        form: &Form,
        var: &RootedValue,
        env: &RootedValue,
    ) -> Result<Form, String> {
        if let ValueView::Var(name) = var.inspect() {
            if name.namespace() == Some(Name::new(None, "clojure.core").name()) {
                let name = Name::from_utf16(Some(Name::new(None, "cljs.core").name()), name.name());
                if let Some(expanded) = self.expand_compiler_seed(form, &name)? {
                    return Ok(expanded);
                }
            }
        }
        let (native, sources) =
            crate::native_form_adapter::native_form_with_sources(self.context_mut(), form)?;
        let expanded = self.expand_native(&native, var, env)?;
        crate::native_form_adapter::own_form_with_sources(&expanded, sources)
    }
    // Temporary native execution seam; deleted with native_form_adapter by
    // light-cljs-115 after macro bootstrap/source execution migration.
    fn expand_native(
        &mut self,
        form: &NativeForm,
        var: &RootedValue,
        env: &RootedValue,
    ) -> Result<NativeForm, String> {
        if self.expansions >= self.limits.expansions {
            return Err("compiler macro expansion limit exceeded".into());
        }
        self.expansions += 1;
        let expanded = self
            .context_mut()
            .expand_macro_call(form, var, env)
            .map_err(|e| e.to_string())?;
        self.generated_nodes = self.generated_nodes.saturating_add(expanded.nodes().len());
        if self.generated_nodes > self.limits.generated_nodes {
            return Err("compiler macro generated-node limit exceeded".into());
        }
        Ok(expanded)
    }
    fn expand_namespace_reflection(
        &self,
        form: &Form,
        name: &Name,
    ) -> Result<Option<Form>, String> {
        let operation = String::from_utf16_lossy(name.name());
        let operation = operation.strip_prefix("compiler-").unwrap_or(&operation);
        if !matches!(operation, "ns-publics" | "ns-interns" | "ns-imports") {
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
            let mut declarations = if operation == "ns-imports" {
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
            declarations.sort();
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
    pub(crate) fn bind_namespace(&mut self, namespace: &str) -> Result<BindingGuard, String> {
        let c = self.context_mut();
        let ns = c
            .compiler_namespace(Name::new(None, namespace))
            .map_err(|e| e.to_string())?;
        let var = c
            .find_interned_var(&Name::new(None, "clojure.core"), &Name::new(None, "*ns*"))
            .map_err(|e| e.to_string())?
            .expect("core *ns*");
        c.bind_vars(&[(var.var_id().unwrap(), ns)])
            .map_err(|e| e.to_string())
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
    pub(crate) fn environment(
        &mut self,
        namespace: &str,
        context: ExprContext,
        locals: &[(String, usize, &'static str, Option<usize>)],
        aliases: &[(String, String)],
        requires: &[String],
        location: (usize, usize),
    ) -> Result<RootedValue, String> {
        let imports = self.imports.get(namespace).cloned().unwrap_or_default();
        let namespace_symbol = self.namespace_symbols.get(namespace).cloned();
        let namespace_doc = self.namespace_docs.get(namespace).cloned();
        let namespace_imports = self
            .namespace_imports
            .get(namespace)
            .cloned()
            .unwrap_or_default();
        let c = self.context_mut();
        let namespace_symbol = namespace_symbol
            .as_ref()
            .map(|d| crate::native_form_adapter::native_datum(c, d))
            .transpose()?;
        let namespace_doc = namespace_doc
            .as_ref()
            .map(|d| crate::native_form_adapter::native_datum(c, d))
            .transpose()?;
        let key = |s| c.keyword(Name::new(None, s));
        let sym = |s: &str| c.symbol(Name::new(None, s));
        let build = || -> Result<RootedValue, crate::interpreter::CollectionError> {
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
            let mut runtime = requires
                .iter()
                .map(|n| (sym(n), sym(n)))
                .collect::<Vec<_>>();
            runtime.extend(aliases.iter().map(|(a, n)| (sym(a), sym(n))));
            let import_names = namespace_imports
                .iter()
                .map(|(a, n)| (sym(a), sym(n)))
                .collect::<Vec<_>>();
            runtime.extend(import_names.iter().cloned());
            c.map(&[
                (key("locals"), local_map),
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
                        (key("require-macros"), c.map(&macro_aliases)?),
                        (key("use-macros"), c.map(&macro_refers)?),
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
mod compiler_owned_origin_tests {
    use super::*;
    use crate::compiler_data::{DiagnosticLocation, Origin, Source};
    #[test]
    fn nested_expansion_preserves_origins_sources_and_metadata_after_host_drop() {
        let mut host = MacroHost::new();
        host.context_mut().eval_string("(ns macros) (defmacro outer [x] (list 'macros/inner x)) (defmacro inner [x] (list 'vector &form x))").unwrap();
        let source = Source::new("nested.cljs", "\n(macros/outer ^:kept [1])");
        let call = host.read_compiler_form(&source).unwrap().unwrap();
        let outer = host
            .context_mut()
            .resolve_var(&Name::new(Some("macros"), "outer"))
            .unwrap()
            .unwrap();
        let inner = host
            .context_mut()
            .resolve_var(&Name::new(Some("macros"), "inner"))
            .unwrap()
            .unwrap();
        let env = host.context_mut().nil();
        let first = host.expand(&call, &outer, &env).unwrap();
        let second = host.expand(&first, &inner, &env).unwrap();
        drop(env);
        drop(outer);
        drop(inner);
        drop(call);
        drop(first);
        drop(host);
        let children = second.children();
        assert_eq!(
            second.node().origin,
            Origin::MacroExpansion(std::rc::Rc::new(Name::new(Some("macros"), "inner")))
        );
        assert_eq!(
            children[1].node().origin,
            Origin::MacroExpansion(std::rc::Rc::new(Name::new(Some("macros"), "outer")))
        );
        assert_eq!(second.span().source().id(), source.id());
        let location = DiagnosticLocation::from_span(&second.span());
        assert_eq!((location.line, location.column), (2, 1));
        assert!(children[2].metadata().is_some());
        drop(source);
        assert_eq!(second.span().source().name(), "nested.cljs");
    }
}

#[cfg(test)]
mod compiler_pure_tests {
    use super::*;
    #[test]
    fn reader_seed_declarations_and_metadata_never_initialize_native_context() {
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
            assert!(host.context.is_none(), "{text} initialized native context");
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
        assert!(host.context.is_none());
        let source = crate::compiler_data::Source::new("reader.cljs", "^:kept #:x{:z [1] :a 2}");
        let form = host.read_compiler_form(&source).unwrap().unwrap();
        assert!(host.context.is_none());
        assert_eq!(form.children()[0].span().text(), ":z");
        assert!(form.metadata().is_some());
    }
}
