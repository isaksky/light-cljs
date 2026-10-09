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
    pub limits: MacroLimits,
    expansions: usize,
    generated_nodes: usize,
}

impl MacroHost {
    pub fn new() -> Self {
        Self::default()
    }
    /// Interpreter effects, limits and source providers are explicit embedding APIs.
    pub fn context_mut(&mut self) -> &mut Context {
        self.context.get_or_insert_with(|| {
            let mut context = Context::new();
            context.initialize_compiler_host();
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
        Ok(())
    }
    pub(crate) fn begin_compilation(&mut self) {
        self.expansions = 0;
        self.generated_nodes = 0;
    }
    fn find_macro(&mut self, namespace: &str, name: &str) -> Result<Option<RootedValue>, String> {
        let namespace = if matches!(namespace, "cljs.core" | "clojure.core") {
            if !core_macro(name) {
                return Ok(None);
            }
            "clojure.core"
        } else {
            namespace
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
                        (key("name"), sym(namespace)),
                        (key("requires"), c.map(&runtime)?),
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
// analyzer. Destructuring/prepost/loop/etc may still require unsupported runtime
// forms and then produce an explicit generated-form diagnostic.
fn core_macro(name: &str) -> bool {
    matches!(
        name,
        "let"
            | "fn"
            | "defn"
            | "defn-"
            | "when"
            | "when-not"
            | "cond"
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
            | "letfn"
    )
}
