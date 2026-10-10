//! Native compile-time execution, independent of runtime globals and JS paths.
use std::collections::{HashMap, HashSet};

use crate::analyzer::ExprContext;
use crate::interpreter::{
    BindingGuard, Context, Form, Name, NativeDefinition, Reload, ResourceLimits, RootedValue,
    ValueView,
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
    namespace_symbols: HashMap<String, RootedValue>,
    namespace_docs: HashMap<String, RootedValue>,
    runtime_vars: HashMap<String, HashMap<String, bool>>,
    namespace_imports: HashMap<String, Vec<(String, String)>>,
    reflection_macros_loaded: bool,
    pub limits: MacroLimits,
    assertions_disabled: bool,
    interop_macros_loaded: bool,
    sequence_macros_loaded: bool,
    runtime_defmacro_loaded: bool,
    expansions: usize,
    generated_nodes: usize,
}

impl MacroHost {
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
        symbol: RootedValue,
        doc: Option<RootedValue>,
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
            && matches!(name, "ns-publics" | "ns-interns" | "ns-imports")
            && !self.reflection_macros_loaded
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
            // Host-only marker Vars. Expansion below reads this compilation's
            // actually registered definitions/imports and emits real Var forms.
            c.eval_string("(defmacro compiler-ns-publics [quoted-ns] nil) (defmacro compiler-ns-interns [quoted-ns] nil) (defmacro compiler-ns-imports [quoted-ns] nil)").map_err(|e| e.to_string())?;
            self.reflection_macros_loaded = true;
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
        } else if namespace == "clojure.core"
            && matches!(name, "ns-publics" | "ns-interns" | "ns-imports")
        {
            match name {
                "ns-publics" => "compiler-ns-publics",
                "ns-interns" => "compiler-ns-interns",
                _ => "compiler-ns-imports",
            }
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
    pub(crate) fn resolve(
        &mut self,
        caller: &str,
        operator: &str,
        shadow_core: bool,
    ) -> Result<Option<RootedValue>, String> {
        let imports = self.imports.get(caller).map(Vec::as_slice).unwrap_or(&[]);
        let target = if let Some((qualifier, name)) = operator.split_once('/') {
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
        };
        match target {
            Some((namespace, name)) => self.find_macro(&namespace, &name),
            None => Ok(None),
        }
    }
    pub(crate) fn expand(
        &mut self,
        form: &Form,
        var: &RootedValue,
        env: &RootedValue,
    ) -> Result<Form, String> {
        if self.expansions >= self.limits.expansions {
            return Err("compiler macro expansion limit exceeded".into());
        }
        self.expansions += 1;
        let reflection = self.expand_namespace_reflection(form, var)?;
        let expanded = match reflection.or(compiler_destructuring::expand(
            self.context_mut(),
            form,
            var,
        )?) {
            Some(expanded) => expanded,
            None => self
                .context_mut()
                .expand_macro_call(form, var, env)
                .map_err(|e| e.to_string())?,
        };
        self.generated_nodes = self.generated_nodes.saturating_add(expanded.nodes().len());
        if self.generated_nodes > self.limits.generated_nodes {
            return Err("compiler macro generated-node limit exceeded".into());
        }
        Ok(expanded)
    }
    fn expand_namespace_reflection(
        &mut self,
        form: &Form,
        var: &RootedValue,
    ) -> Result<Option<Form>, String> {
        let ValueView::Var(name) = var.inspect() else {
            return Ok(None);
        };
        if name.namespace() != Some(Name::new(None, "clojure.core").name()) {
            return Ok(None);
        }
        let operation = String::from_utf16_lossy(name.name());
        let Some(operation) = operation.strip_prefix("compiler-") else {
            return Ok(None);
        };
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
                || !matches!(quoted[0].inspect(), ValueView::Symbol(n) if n == Name::new(None,"quote"))
                || !matches!(quoted[1].inspect(), ValueView::Symbol(_))
            {
                return Err(format!("Argument to {operation} must be a quoted symbol").into());
            }
            let ValueView::Symbol(namespace) = quoted[1].inspect() else {
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
            let c = self.context_mut();
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
            Ok(c.macro_form(&c.map(&entries)?, form, name)?)
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

// Portable native expanders whose primitive output can be checked by the JS
// analyzer. Binding destructuring, pre/post assertions and recurrence lower to
// emitted JavaScript primitives; other macros keep explicit capability checks.
fn core_macro(name: &str) -> bool {
    matches!(
        name,
        "let"
            | "doto"
            | "js-obj"
            | "memfn"
            | "amap"
            | "areduce"
            | ".."
            | "fn"
            | "defn"
            | "defn-"
            | "defmacro"
            | "if-not"
            | "when"
            | "when-not"
            | "cond"
            | "assert"
            | "condp"
            | "while"
            | "dotimes"
            | "for"
            | "lazy-cat"
            | "selector"
            | "ns-publics"
            | "ns-interns"
            | "ns-imports"
            | "doseq"
            | "when-first"
            | "and"
            | "or"
            | "->"
            | "->>"
            | "as->"
            | "some->"
            | "some->>"
            | "cond->"
            | "cond->>"
            | "if-let"
            | "when-let"
            | "if-some"
            | "when-some"
            | "loop"
            | "case"
            | "letfn"
            | "vswap!"
    )
}

// These forms are lowered directly by the runtime analyzer; the native host's
// similarly named forms have interpreter-specific semantics.
pub(crate) fn is_definition_macro(namespace: &str, name: &str) -> bool {
    matches!(namespace, "cljs.core" | "clojure.core")
        && matches!(
            name,
            "delay"
                | "lazy-seq"
                | "defonce"
                | "declare"
                | "binding"
                | "with-redefs"
                | "resolve"
                | "exists?"
                | "with-out-str"
                | "this-as"
                | "js-arguments"
                | "copy-arguments"
        )
}

// Checkout ClojureScript extends map patterns beyond the native interpreter's
// ordinary binding macros. Keep this lowering in the compiler host: expanded
// forms use existing primitives and never install new interpreter semantics.
mod compiler_destructuring {
    use super::*;
    use crate::interpreter::{FormKind, HeapId};
    use crate::parser::CollectionKind;
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicU64, Ordering};

    type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
    type Entries = Vec<(RootedValue, RootedValue)>;

    fn named(v: &RootedValue, name: &str) -> bool {
        matches!(v.inspect(), ValueView::Symbol(n) if n == Name::new(None, name))
    }
    fn ident(v: &RootedValue) -> bool {
        matches!(v.inspect(), ValueView::Symbol(_) | ValueView::Keyword(_))
    }
    fn truth(v: &RootedValue) -> bool {
        !matches!(v.inspect(), ValueView::Nil | ValueView::Boolean(false))
    }
    fn extended(pattern: &RootedValue) -> Result<bool> {
        match pattern.inspect() {
            ValueView::Map(_) => {
                for (binding, value) in pattern.entries()? {
                    if let ValueView::Keyword(n) = binding.inspect() {
                        let name = String::from_utf16_lossy(n.name());
                        if matches!(
                            name.as_str(),
                            "keys!"
                                | "syms!"
                                | "strs!"
                                | "select"
                                | "defaults"
                                | "all"
                                | "excess"
                                | "missing"
                        ) {
                            return Ok(true);
                        }
                        if matches!(name.as_str(), "keys" | "syms" | "strs")
                            && value.elements()?.iter().any(|v| named(v, "&"))
                        {
                            return Ok(true);
                        }
                    } else if extended(&binding)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            ValueView::Vector(_) => {
                for value in pattern.elements()? {
                    if extended(&value)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    pub(super) fn expand(
        c: &Context,
        form: &Form,
        var: &RootedValue,
    ) -> std::result::Result<Option<Form>, String> {
        (|| -> Result<Option<Form>> {
            let ValueView::Var(name) = var.inspect() else {
                return Ok(None);
            };
            if name.namespace() != Some(Name::new(None, "clojure.core").name()) {
                return Ok(None);
            }
            let name = String::from_utf16_lossy(name.name());
            if !matches!(name.as_str(), "let" | "fn" | "loop" | "letfn") {
                return Ok(None);
            }
            let parts = form.data().elements()?;
            let args = &parts[1..];
            let mut expander = Expander {
                c,
                maps: HashMap::new(),
                derived_entries: RefCell::new(HashMap::new()),
            };
            let output = if name == "letfn" {
                let Some(specs) = args
                    .first()
                    .filter(|v| matches!(v.inspect(), ValueView::Vector(_)))
                else {
                    return Ok(None);
                };
                let mut bindings = vec![];
                let mut found = false;
                for spec in specs.elements()? {
                    if !matches!(spec.inspect(), ValueView::List(_)) {
                        return Ok(None);
                    }
                    let parts = spec.elements()?;
                    let Some(function_name) = parts
                        .first()
                        .filter(|v| matches!(v.inspect(), ValueView::Symbol(_)))
                    else {
                        return Ok(None);
                    };
                    let declarations = &parts[1..];
                    let Some(first) = declarations.first() else {
                        return Ok(None);
                    };
                    let methods = if matches!(first.inspect(), ValueView::Vector(_)) {
                        vec![declarations.to_vec()]
                    } else {
                        if declarations
                            .iter()
                            .any(|value| !matches!(value.inspect(), ValueView::List(_)))
                        {
                            return Ok(None);
                        }
                        declarations
                            .iter()
                            .map(RootedValue::elements)
                            .collect::<std::result::Result<Vec<_>, _>>()?
                    };
                    for method in methods {
                        if let Some(params) = method.first() {
                            found |= extended(params)?;
                        }
                    }
                    let mut function_args = vec![function_name.clone()];
                    function_args.extend_from_slice(declarations);
                    bindings.extend([function_name.clone(), expander.call("fn", &function_args)?]);
                }
                if !found {
                    return Ok(None);
                }
                expander.body("letfn*", bindings, &args[1..])?
            } else if name == "fn" {
                let start = usize::from(
                    args.first()
                        .is_some_and(|v| matches!(v.inspect(), ValueView::Symbol(_))),
                );
                let declarations = &args[start..];
                let Some(first) = declarations.first() else {
                    return Ok(None);
                };
                let methods = if matches!(first.inspect(), ValueView::Vector(_)) {
                    vec![declarations.to_vec()]
                } else {
                    if declarations
                        .iter()
                        .any(|value| !matches!(value.inspect(), ValueView::List(_)))
                    {
                        return Ok(None);
                    }
                    declarations
                        .iter()
                        .map(RootedValue::elements)
                        .collect::<std::result::Result<Vec<_>, _>>()?
                };
                let mut found = false;
                for method in &methods {
                    if let Some(params) = method.first() {
                        found |= extended(params)?;
                    }
                }
                if !found {
                    return Ok(None);
                }
                expander.remember(form);
                expander.function(&args[..start], methods)?
            } else {
                let Some(bindings) = args.first() else {
                    return Ok(None);
                };
                if !matches!(bindings.inspect(), ValueView::Vector(_)) {
                    return Ok(None);
                }
                let pairs = bindings.elements()?;
                let mut found = false;
                for pattern in pairs.iter().step_by(2) {
                    found |= extended(pattern)?;
                }
                if !found {
                    return Ok(None);
                }
                if !pairs.len().is_multiple_of(2) {
                    return Err("binding vector requires binding/initializer pairs".into());
                }
                expander.remember(form);
                if name == "let" {
                    let mut bindings = vec![];
                    for pair in pairs.as_chunks::<2>().0 {
                        expander.bind(&mut bindings, &pair[0], pair[1].clone(), 0)?;
                    }
                    expander.body("let*", bindings, &args[1..])?
                } else {
                    let mut outer = vec![];
                    let mut targets = vec![];
                    let mut inner = vec![];
                    for pair in pairs.as_chunks::<2>().0 {
                        if matches!(pair[0].inspect(), ValueView::Keyword(_)) {
                            return Err("unsupported binding key".into());
                        }
                        let simple = matches!(pair[0].inspect(), ValueView::Symbol(_));
                        let temp = if simple {
                            pair[0].clone()
                        } else {
                            expander.fresh()
                        };
                        outer.extend([temp.clone(), pair[1].clone()]);
                        if !simple {
                            expander.bind(&mut outer, &pair[0], temp.clone(), 0)?;
                        }
                        targets.extend([temp.clone(), temp.clone()]);
                        expander.bind(&mut inner, &pair[0], temp, 0)?;
                    }
                    let body = expander.body("let*", inner, &args[1..])?;
                    let body = expander.body("loop*", targets, &[body])?;
                    expander.body("let*", outer, &[body])?
                }
            };
            Ok(Some(c.macro_form(
                &output,
                form,
                match var.inspect() {
                    ValueView::Var(name) => name,
                    _ => unreachable!(),
                },
            )?))
        })()
        .map_err(|e| e.to_string())
    }

    struct Expander<'a> {
        c: &'a Context,
        maps: HashMap<HeapId, Form>,
        derived_entries: RefCell<HashMap<HeapId, Entries>>,
    }
    impl Expander<'_> {
        fn remember(&mut self, form: &Form) {
            let mut pending = vec![form.id()];
            while let Some(id) = pending.pop() {
                let child = form.at(id).unwrap();
                if let FormKind::Collection { kind, children } = &child.node().kind {
                    pending.extend(children.iter().copied());
                    if *kind == CollectionKind::Map {
                        self.maps
                            .insert(child.data().heap_id().unwrap(), child.clone());
                    }
                }
            }
        }

        fn sym(&self, name: &str) -> RootedValue {
            self.c.symbol(Name::new(None, name))
        }
        fn kw(&self, name: &str) -> RootedValue {
            self.c.keyword(Name::new(None, name))
        }
        fn fresh(&self) -> RootedValue {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            self.sym(&format!(
                "compiler_destructure__{}__auto__",
                NEXT.fetch_add(1, Ordering::Relaxed)
            ))
        }
        fn call(&self, name: &str, args: &[RootedValue]) -> Result<RootedValue> {
            let op = if matches!(
                name,
                "let*" | "letfn*" | "loop*" | "fn*" | "if" | "do" | "quote" | "throw" | "new"
            ) {
                self.sym(name)
            } else {
                self.c.symbol(Name::new(Some("cljs.core"), name))
            };
            let mut forms = vec![op];
            forms.extend_from_slice(args);
            Ok(self.c.list(&forms)?)
        }
        fn body(
            &self,
            op: &str,
            bindings: Vec<RootedValue>,
            body: &[RootedValue],
        ) -> Result<RootedValue> {
            let mut args = vec![self.c.vector(&bindings)?];
            args.extend_from_slice(body);
            self.call(op, &args)
        }
        fn entries(&self, value: &RootedValue) -> Result<Entries> {
            if let Some(entries) = value
                .heap_id()
                .and_then(|id| self.derived_entries.borrow().get(&id).cloned())
            {
                return Ok(entries);
            }
            if let Some(form) = value.heap_id().and_then(|id| self.maps.get(&id)) {
                let FormKind::Collection { children, .. } = &form.node().kind else {
                    unreachable!()
                };
                Ok(children
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|p| (form.at(p[0]).unwrap().data(), form.at(p[1]).unwrap().data()))
                    .collect())
            } else {
                Ok(value.entries()?)
            }
        }
        fn assoc_pattern(
            &self,
            pattern: &RootedValue,
            key: RootedValue,
            value: RootedValue,
        ) -> Result<RootedValue> {
            let mut entries = self.entries(pattern)?;
            if let Some((_, old_value)) = entries
                .iter_mut()
                .find(|(old_key, _)| old_key.value_eq(&key))
            {
                *old_value = value.clone();
            } else {
                entries.push((key.clone(), value.clone()));
            }
            let pattern = self.c.assoc(pattern, &key, &value)?;
            self.derived_entries
                .borrow_mut()
                .insert(pattern.heap_id().unwrap(), entries);
            Ok(pattern)
        }
        fn local(&self, binding: &RootedValue) -> Result<RootedValue> {
            let name = match binding.inspect() {
                ValueView::Keyword(n) | ValueView::Symbol(n) => n,
                _ => return Err("binding must be named".into()),
            };
            let local = self.c.symbol(Name::from_utf16(None, name.name()));
            Ok(if let Some(meta) = binding.metadata() {
                self.c.with_meta(&local, &meta)?
            } else {
                local
            })
        }
        fn function(
            &self,
            name: &[RootedValue],
            methods: Vec<Vec<RootedValue>>,
        ) -> Result<RootedValue> {
            let mut output = name.to_vec();
            for method in methods {
                let Some(params) = method
                    .first()
                    .filter(|v| matches!(v.inspect(), ValueView::Vector(_)))
                else {
                    return Err("function parameters must be a vector".into());
                };
                let mut body = method[1..].to_vec();
                let conditions = if body.len() > 1 && matches!(body[0].inspect(), ValueView::Map(_))
                {
                    Some(body.remove(0))
                } else {
                    params.metadata()
                };
                if let Some(conditions) = conditions {
                    if let Some(post) = conditions.get(&self.kw("post"))?.filter(truth) {
                        let mut tail = vec![];
                        for condition in post.elements()? {
                            tail.push(self.call("assert", &[condition])?);
                        }
                        tail.push(self.sym("%"));
                        body = vec![self.body(
                            "let*",
                            vec![self.sym("%"), self.call("do", &body)?],
                            &tail,
                        )?];
                    }
                    if let Some(pre) = conditions.get(&self.kw("pre"))?.filter(truth) {
                        let mut before = vec![];
                        for condition in pre.elements()? {
                            before.push(self.call("assert", &[condition])?);
                        }
                        before.extend(body);
                        body = before;
                    }
                }
                let mut parameters = vec![];
                let mut bindings = vec![];
                let parts = params.elements()?;
                let mut rest = false;
                for (index, param) in parts.iter().enumerate() {
                    if named(param, "&") {
                        if rest || index + 2 != parts.len() {
                            return Err("& requires one final function parameter".into());
                        }
                        rest = true;
                        parameters.push(param.clone());
                    } else if matches!(param.inspect(), ValueView::Symbol(_)) {
                        parameters.push(param.clone());
                    } else {
                        if matches!(param.inspect(), ValueView::Keyword(_)) {
                            return Err("unsupported function binding key".into());
                        }
                        let temp = self.fresh();
                        parameters.push(temp.clone());
                        self.bind(&mut bindings, param, temp, 0)?;
                    }
                }
                let mut method = vec![self.c.vector(&parameters)?];
                method.push(self.body("let*", bindings, &body)?);
                output.push(self.c.list(&method)?);
            }
            self.call("fn*", &output)
        }
        fn bind(
            &self,
            out: &mut Vec<RootedValue>,
            pattern: &RootedValue,
            value: RootedValue,
            depth: usize,
        ) -> Result<()> {
            if depth > 128 {
                return Err("compiler destructuring nesting limit exceeded".into());
            }
            match pattern.inspect() {
                ValueView::Symbol(_) | ValueView::Keyword(_) => {
                    out.extend([self.local(pattern)?, value])
                }
                ValueView::Vector(_) => {
                    let whole = self.fresh();
                    out.extend([whole.clone(), value]);
                    let parts = pattern.elements()?;
                    let has_rest = parts.iter().any(|v| named(v, "&"));
                    let seq = self.fresh();
                    if has_rest {
                        out.extend([seq.clone(), self.call("seq", std::slice::from_ref(&whole))?]);
                    }
                    let mut index = 0;
                    let mut rest = false;
                    while index < parts.len() {
                        let part = &parts[index];
                        if named(part, "&") {
                            if rest || index + 1 == parts.len() {
                                return Err("& requires one rest binding".into());
                            }
                            self.bind(out, &parts[index + 1], seq.clone(), depth + 1)?;
                            rest = true;
                            index += 2;
                        } else if part.value_eq(&self.kw("as")) {
                            if index + 2 != parts.len() {
                                return Err(":as requires one final binding".into());
                            }
                            self.bind(out, &parts[index + 1], whole.clone(), depth + 1)?;
                            break;
                        } else {
                            if rest {
                                return Err("only :as may follow a rest binding".into());
                            }
                            let item = if has_rest {
                                let first = self.fresh();
                                out.extend([
                                    first.clone(),
                                    self.call("first", std::slice::from_ref(&seq))?,
                                    seq.clone(),
                                    self.call("next", std::slice::from_ref(&seq))?,
                                ]);
                                first
                            } else {
                                self.call(
                                    "nth",
                                    &[whole.clone(), self.c.integer(index as i64), self.c.nil()],
                                )?
                            };
                            self.bind(out, part, item, depth + 1)?;
                            index += 1;
                        }
                    }
                }
                ValueView::Map(_) => self.map(out, pattern, value, depth + 1)?,
                _ => return Err("unsupported binding form".into()),
            }
            Ok(())
        }
        // Building the map before filtering preserves evaluation of selector
        // key expressions even when a nested selection has a nil result.
        fn some_vals(&self, entries: &Entries) -> Result<RootedValue> {
            self.call("some-vals", &[self.c.map(entries)?])
        }
        fn map(
            &self,
            out: &mut Vec<RootedValue>,
            pattern: &RootedValue,
            value: RootedValue,
            depth: usize,
        ) -> Result<()> {
            let c = self.c;
            let directive = |name: &str| -> Result<Option<RootedValue>> {
                Ok(pattern.get(&self.kw(name))?.filter(truth))
            };
            let defaults = directive("or")?;
            let defaults_as = directive("defaults")?;
            if defaults_as.is_some() && defaults.is_none() {
                return Err("Can't specify :defaults without :or".into());
            }
            let mut default_vars = vec![];
            if let Some(defaults) = &defaults {
                if !matches!(defaults.inspect(), ValueView::Map(_)) {
                    return Err(":or defaults must be a map".into());
                }
                for (key, expression) in self.entries(defaults)? {
                    let temp = self.fresh();
                    out.extend([temp.clone(), expression]);
                    default_vars.push((key, temp));
                }
            }
            let whole = self.fresh();
            out.extend([
                whole.clone(),
                value,
                whole.clone(),
                self.call(
                    "if",
                    &[
                        self.call("seq?", std::slice::from_ref(&whole))?,
                        self.call("seq-to-map-for-destructuring", std::slice::from_ref(&whole))?,
                        whole.clone(),
                    ],
                )?,
            ]);
            if let Some(alias) = directive("as")? {
                out.extend([alias, whole.clone()]);
            }
            let select = directive("select")?;
            let all = directive("all")?;
            let excess = directive("excess")?;
            let missing = directive("missing")?;
            let not_found = missing.as_ref().map(|_| self.fresh());
            if let Some(missing) = &missing {
                out.extend([
                    missing.clone(),
                    c.nil(),
                    not_found.as_ref().unwrap().clone(),
                    self.call("new", &[c.symbol(Name::new(Some("js"), "Object"))])?,
                ]);
            }
            let mut selected: Vec<RootedValue> = vec![];
            let mut binding_keys: Entries = vec![];
            let mut subs: [Entries; 4] = std::array::from_fn(|_| vec![]);
            let mut pending = vec![];
            for (binding, key) in self.entries(pattern)? {
                if let ValueView::Keyword(dir) = binding.inspect() {
                    let which = String::from_utf16_lossy(dir.name());
                    if dir.namespace().is_none()
                        && matches!(
                            which.as_str(),
                            "as" | "or" | "defaults" | "select" | "all" | "excess" | "missing"
                        )
                    {
                        continue;
                    }
                    let required = which.ends_with('!');
                    let which = which.trim_end_matches('!');
                    if !matches!(which, "keys" | "syms" | "strs") {
                        return Err("unsupported map destructuring directive".into());
                    }
                    if !matches!(key.inspect(), ValueView::Vector(_)) {
                        return Err("map directive requires a vector".into());
                    }
                    let mut before_amp = true;
                    for binding in key.elements()? {
                        if named(&binding, "&") {
                            if !before_amp {
                                return Err("& can only appear once in map directive".into());
                            }
                            before_amp = false;
                            continue;
                        }
                        let key = if before_amp {
                            let name = match binding.inspect() {
                                ValueView::Symbol(n) | ValueView::Keyword(n) => n,
                                _ => return Err("map directive requires named bindings".into()),
                            };
                            let key_name =
                                Name::from_utf16(dir.namespace().or(name.namespace()), name.name());
                            match which {
                                "keys" => c.keyword(key_name),
                                "syms" => self.call("quote", &[c.symbol(key_name)])?,
                                _ => {
                                    let mut text = String::new();
                                    if matches!(binding.inspect(), ValueView::Keyword(_)) {
                                        text.push(':');
                                    }
                                    if let Some(ns) = name.namespace() {
                                        text.push_str(&String::from_utf16_lossy(ns));
                                        text.push('/');
                                    }
                                    text.push_str(&String::from_utf16_lossy(name.name()));
                                    c.string(text.encode_utf16().collect::<Vec<_>>())
                                }
                            }
                        } else {
                            if matches!(binding.inspect(), ValueView::Symbol(_)) {
                                return Err(
                                    "binding symbols can only appear before &, use keys after"
                                        .into(),
                                );
                            }
                            binding.clone()
                        };
                        if !selected.iter().any(|v| v.value_eq(&key)) {
                            selected.push(key.clone());
                        }
                        if before_amp {
                            binding_keys.push((self.local(&binding)?, key.clone()));
                            pending.push((binding, key, required));
                        } else if required {
                            pending.push((self.fresh(), key, true));
                        }
                    }
                } else {
                    if !selected.iter().any(|v| v.value_eq(&key)) {
                        selected.push(key.clone());
                    }
                    let mut binding = binding;
                    if matches!(binding.inspect(), ValueView::Map(_)) {
                        for (index, (name, enabled)) in [
                            ("select", &select),
                            ("all", &all),
                            ("excess", &excess),
                            ("missing", &missing),
                        ]
                        .iter()
                        .enumerate()
                        {
                            if enabled.is_some() {
                                let sub = binding
                                    .get(&self.kw(name))?
                                    .filter(truth)
                                    .unwrap_or_else(|| self.fresh());
                                binding =
                                    self.assoc_pattern(&binding, self.kw(name), sub.clone())?;
                                subs[index].push((key.clone(), sub));
                            }
                        }
                    } else if matches!(binding.inspect(), ValueView::Symbol(_)) {
                        binding_keys.push((binding.clone(), key.clone()));
                    }
                    pending.push((binding, key, false));
                }
            }
            for (binding, key, required) in pending {
                let local = if ident(&binding) {
                    self.local(&binding)?
                } else {
                    binding.clone()
                };
                let local_default = default_vars.iter().find(|(k, _)| k.value_eq(&local));
                let key_default = default_vars.iter().find(|(k, _)| k.value_eq(&key));
                if local_default.is_some() && key_default.is_some() {
                    return Err("Multiple :or defaults for same key".into());
                }
                let default = local_default.or(key_default).map(|(_, v)| v.clone());
                if required && default.is_some() {
                    return Err("Can't supply default value for required key".into());
                }
                let mut get_args = vec![whole.clone(), key.clone()];
                if let Some(default) = default {
                    get_args.push(default);
                }
                let get = self.call("get", &get_args)?;
                let value = if required {
                    if let Some(missing) = &missing {
                        let sentinel = not_found.as_ref().unwrap().clone();
                        let found = self.fresh();
                        let absent = self.fresh();
                        out.extend([
                            found.clone(),
                            self.call("get", &[whole.clone(), key.clone(), sentinel.clone()])?,
                            absent.clone(),
                            self.call("identical?", &[found.clone(), sentinel])?,
                            missing.clone(),
                            self.call(
                                "if",
                                &[
                                    absent.clone(),
                                    self.call("assoc", &[missing.clone(), key.clone(), c.nil()])?,
                                    missing.clone(),
                                ],
                            )?,
                        ]);
                        self.call("if", &[absent, c.nil(), found])?
                    } else {
                        self.call("req!", &[whole.clone(), key.clone()])?
                    }
                } else {
                    get
                };
                self.bind(out, &local, value, depth)?;
            }
            let new_defaults =
                defaults.is_some() && (defaults_as.is_some() || select.is_some() || all.is_some());
            let mut default_map = vec![];
            for (key, value) in &default_vars {
                let key = if matches!(key.inspect(), ValueView::Symbol(_)) {
                    match binding_keys.iter().find(|(b, _)| b.value_eq(key)) {
                        Some((_, key)) => key.clone(),
                        None if new_defaults => {
                            return Err("symbol in :or does not refer to a binding".into());
                        }
                        None => c.nil(),
                    }
                } else {
                    key.clone()
                };
                if new_defaults && !selected.iter().any(|v| v.value_eq(&key)) {
                    return Err("keys appear only in :or".into());
                }
                default_map.push((key, value.clone()));
            }
            let dm = if default_map.is_empty() {
                c.nil()
            } else {
                c.map(&default_map)?
            };
            let selected = c.vector(&selected)?;
            if let Some(select) = select {
                let merged = self.fresh();
                out.extend([
                    select,
                    self.body(
                        "let*",
                        vec![
                            merged.clone(),
                            self.call(
                                "merge",
                                &[dm.clone(), whole.clone(), self.some_vals(&subs[0])?],
                            )?,
                        ],
                        &[self.call(
                            "if",
                            &[
                                merged.clone(),
                                self.call("select-keys", &[merged, selected.clone()])?,
                                c.nil(),
                            ],
                        )?],
                    )?,
                ]);
            }
            if let Some(all) = all {
                out.extend([
                    all,
                    self.call(
                        "merge",
                        &[dm.clone(), whole.clone(), self.some_vals(&subs[1])?],
                    )?,
                ]);
            }
            if let Some(excess) = excess {
                out.extend([
                    excess,
                    self.call(
                        "merge",
                        &[
                            self.call(
                                "not-empty",
                                &[self.call(
                                    "apply",
                                    &[
                                        c.symbol(Name::new(Some("cljs.core"), "dissoc")),
                                        whole,
                                        selected,
                                    ],
                                )?],
                            )?,
                            self.some_vals(&subs[2])?,
                        ],
                    )?,
                ]);
            }
            if let Some(missing) = missing {
                out.extend([
                    missing.clone(),
                    self.call("merge", &[missing, self.some_vals(&subs[3])?])?,
                ]);
            }
            if let Some(defaults_as) = defaults_as {
                out.extend([defaults_as, dm]);
            }
            Ok(())
        }
    }
}
