//! Typed embedding configuration and the explicit sharing boundary for forks.
use super::*;
use std::collections::HashSet;

/// Name access policy. Unqualified entries mean `clojure.core/name`; aliases
/// are never policy identities. Deny wins over allow. Language-created Vars
/// are exempt from the allow list, but are still subject to deny.
#[derive(Clone, Debug, Default)]
pub struct NamePolicy {
    pub allow: Option<HashSet<Name>>,
    pub deny: HashSet<Name>,
    /// Disable name checks only. This grants no host capabilities and does not
    /// disable owner checks, resource limits, or builtin write protection.
    pub unrestricted: bool,
}
impl NamePolicy {
    fn canonical(name: &Name) -> Name {
        if name.namespace().is_some() {
            name.clone()
        } else {
            Name::from_utf16(
                Some(&"clojure.core".encode_utf16().collect::<Vec<_>>()),
                name.name(),
            )
        }
    }
    fn normalized(mut self) -> Self {
        self.allow = self
            .allow
            .map(|names| names.iter().map(Self::canonical).collect());
        self.deny = self.deny.iter().map(Self::canonical).collect();
        self
    }
    fn active(&self) -> bool {
        !self.unrestricted && (self.allow.is_some() || !self.deny.is_empty())
    }
    fn permits(&self, name: &Name, exposed: bool) -> bool {
        self.unrestricted
            || (!self.deny.contains(name)
                && (!exposed || self.allow.as_ref().is_none_or(|names| names.contains(name))))
    }
}

/// One namespace alias to install or replace explicitly during option merging.
pub struct NamespaceAlias {
    pub namespace: Name,
    pub alias: Name,
    pub target: Name,
}

/// A patch, not a replacement context. Omitted fields preserve existing state.
/// Supplied policy/features/limits replace those settings; lists merge by name.
/// `Some(None)` removes an optional host capability. Values must belong to this
/// context (or its fork family); create values after `Context::new`, then merge.
/// Unsupported SCI options (classes, reflection, async, etc.) are intentionally
/// unrepresentable in this typed API.
///
/// ```compile_fail
/// use light_cljs::interpreter::ContextOptions;
/// let options = ContextOptions { classes: true, ..Default::default() };
/// ```
#[derive(Default)]
pub struct ContextOptions {
    pub natives: Vec<&'static NativeDefinition>,
    pub macros: Vec<&'static NativeDefinition>,
    /// Qualified Var names, or unqualified names in `user`.
    pub bindings: Vec<(Name, RootedValue)>,
    pub namespaces: Vec<Name>,
    pub aliases: Vec<NamespaceAlias>,
    pub current_namespace: Option<Name>,
    pub policy: Option<NamePolicy>,
    pub features: Option<Vec<Name>>,
    pub data_readers: Vec<(Name, Option<&'static NativeDefinition>)>,
    pub default_data_reader: Option<Option<&'static NativeDefinition>>,
    pub source_provider: Option<Option<Rc<dyn SourceProvider>>>,
    /// Explicit stream roots, or nil to disconnect a channel.
    pub input: Option<RootedValue>,
    pub output: Option<RootedValue>,
    pub error_output: Option<RootedValue>,
    pub limits: Option<ResourceLimits>,
    pub cancellation: Option<Option<CancellationToken>>,
    pub execution_poll: Option<Option<ExecutionPoll>>,
    pub allow_builtin_var_mutation: Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OptionsError {
    WrongOwner,
    InvalidName,
    InvalidArity,
    ExpectedInput,
    ExpectedOutput,
    Namespace(NamespaceError),
    /// Options and fork are host boundaries, not callback operations. This also
    /// prevents a callback from replacing its active permission policy.
    ActiveEvaluation,
}
impl fmt::Display for OptionsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for OptionsError {}
impl From<NamespaceError> for OptionsError {
    fn from(value: NamespaceError) -> Self {
        Self::Namespace(value)
    }
}

/// The last value and final namespace, including an empty input's nil value.
#[derive(Clone, Debug)]
pub struct Evaluation {
    pub value: RootedValue,
    pub namespace: RootedValue,
}
/// Preserve the namespace reached before failure as well as owned diagnostics.
#[derive(Clone, Debug)]
pub struct EvaluationError {
    pub diagnostic: Box<EvalError>,
    pub namespace: RootedValue,
}
impl fmt::Display for EvaluationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.diagnostic.fmt(f)
    }
}
impl std::error::Error for EvaluationError {}

impl Context {
    pub fn init(options: ContextOptions) -> Result<Self, OptionsError> {
        let mut context = Self::new();
        context.merge_opts(options)?;
        Ok(context)
    }

    pub fn name_policy(&self) -> NamePolicy {
        self.policy.borrow().clone()
    }

    /// Validate the entire patch before installing anything. Named replacement
    /// preserves Var identity and unrelated namespace state; no code executes.
    pub fn merge_opts(&mut self, options: ContextOptions) -> Result<(), OptionsError> {
        if self.accounting.is_active() {
            return Err(OptionsError::ActiveEvaluation);
        }
        let bare = |n: &Name| n.namespace().is_none() && !n.name().is_empty();
        let named = |n: &Name| !n.name().is_empty() && n.namespace().is_none_or(|s| !s.is_empty());
        if options.namespaces.iter().any(|n| !bare(n))
            || options.current_namespace.as_ref().is_some_and(|n| !bare(n))
            || options
                .aliases
                .iter()
                .any(|a| !bare(&a.namespace) || !bare(&a.alias) || !bare(&a.target))
            || options.bindings.iter().any(|(n, _)| !named(n))
            || options.data_readers.iter().any(|(n, _)| !named(n))
            || options
                .policy
                .as_ref()
                .is_some_and(|p| p.allow.iter().flatten().chain(&p.deny).any(|n| !named(n)))
        {
            return Err(OptionsError::InvalidName);
        }
        for definition in options
            .natives
            .iter()
            .chain(&options.macros)
            .copied()
            .chain(options.data_readers.iter().filter_map(|(_, d)| *d))
            .chain(options.default_data_reader.flatten())
        {
            if definition.metadata.namespace.is_empty() || definition.metadata.name.is_empty() {
                return Err(OptionsError::InvalidName);
            }
            if matches!(definition.metadata.arity, Arity::Range { min, max } if min > max) {
                return Err(OptionsError::InvalidArity);
            }
        }
        for value in options
            .bindings
            .iter()
            .map(|(_, v)| v)
            .chain(options.input.iter())
            .chain(options.output.iter())
            .chain(options.error_output.iter())
        {
            if !Rc::ptr_eq(&value.owner, &self.owner) {
                return Err(OptionsError::WrongOwner);
            }
        }
        if options
            .input
            .as_ref()
            .is_some_and(|v| !matches!(v.inspect(), ValueView::Nil | ValueView::Input))
        {
            return Err(OptionsError::ExpectedInput);
        }
        if options
            .output
            .iter()
            .chain(options.error_output.iter())
            .any(|v| !matches!(v.inspect(), ValueView::Nil | ValueView::Output))
        {
            return Err(OptionsError::ExpectedOutput);
        }
        for name in options.namespaces {
            self.create_namespace(name)?;
        }
        for definition in options.natives {
            self.register_native(definition);
        }
        for definition in options.macros {
            self.register_macro(definition);
        }
        for (name, value) in options.bindings {
            let namespace = name.namespace().map_or_else(
                || Name::new(None, "user"),
                |namespace| Name::from_utf16(None, namespace),
            );
            let ns = self.create_namespace(namespace)?;
            let var = self
                .intern_var(
                    ns.namespace_id().unwrap(),
                    Name::from_utf16(None, name.name()),
                )?
                .var_id()
                .unwrap();
            self.set_var_root(var, &value).expect("validated owner");
            self.owner.store.borrow_mut().vars.get_mut(var.0).exposed = true;
        }
        for alias in options.aliases {
            let ns = self
                .create_namespace(alias.namespace)?
                .namespace_id()
                .unwrap();
            let target = self.create_namespace(alias.target)?.namespace_id().unwrap();
            self.unalias_namespace(ns, &alias.alias)?;
            self.alias_namespace(ns, alias.alias, target)?;
        }
        if let Some(name) = options.current_namespace {
            let id = self.create_namespace(name)?.namespace_id().unwrap();
            self.set_current_namespace(id)?;
        }
        if let Some(features) = options.features {
            self.set_reader_features(features);
        }
        for (name, definition) in options.data_readers {
            if let Some(definition) = definition {
                self.register_data_reader(name, definition);
            } else {
                self.remove_data_reader(&name);
            }
        }
        if let Some(reader) = options.default_data_reader {
            self.set_default_data_reader(reader);
        }
        if let Some(provider) = options.source_provider {
            self.loading.borrow_mut().provider = provider;
        }
        for (name, value) in [
            ("*in*", options.input),
            ("*out*", options.output),
            ("*err*", options.error_output),
        ] {
            if let Some(value) = value {
                let var = self.core_var(&Name::new(None, name)).unwrap();
                self.set_var_root(var, &value).expect("validated owner");
            }
        }
        if let Some(limits) = options.limits {
            self.set_resource_limits(limits);
        }
        if let Some(token) = options.cancellation {
            self.set_cancellation_token(token);
        }
        if let Some(poll) = options.execution_poll {
            self.set_execution_poll(poll);
        }
        if let Some(allow) = options.allow_builtin_var_mutation {
            self.allow_builtin_var_mutation(allow);
        }
        self.invalidate_analysis();
        if let Some(policy) = options.policy {
            *self.policy.borrow_mut() = policy.normalized();
        }
        Ok(())
    }

    pub fn eval_string_plus(&mut self, text: &str) -> Result<Evaluation, EvaluationError> {
        let result = self.eval_string(text);
        self.evaluation_outcome(result)
    }
    pub fn eval_form_plus(&mut self, form: &Form) -> Result<Evaluation, EvaluationError> {
        let result = self.eval_form(form);
        self.evaluation_outcome(result)
    }
    fn evaluation_outcome(
        &self,
        result: Result<RootedValue, EvalError>,
    ) -> Result<Evaluation, EvaluationError> {
        let namespace = self.current_namespace();
        result
            .map(|value| Evaluation {
                value,
                namespace: namespace.clone(),
            })
            .map_err(|diagnostic| EvaluationError {
                diagnostic: Box::new(diagnostic),
                namespace,
            })
    }

    pub(super) fn check_name(&self, name: &Name, exposed: bool) -> Result<(), EvalErrorKind> {
        if self.policy.borrow().permits(name, exposed) {
            Ok(())
        } else {
            Err(EvalErrorKind::PermissionDenied(name.clone()))
        }
    }
    pub(super) fn check_var_access(&self, id: VarId) -> Result<(), EvalErrorKind> {
        if !self.policy.borrow().active() {
            return Ok(());
        }
        let store = self.owner.store.borrow();
        let var = store
            .vars
            .get(id.0)
            .map_err(|_| EvalErrorKind::WrongOwner)?;
        self.check_name(&store.var_name(id), var.exposed)
    }
    pub(super) fn check_native(&self, definition: &NativeDefinition) -> Result<(), EvalErrorKind> {
        if !self.policy.borrow().active() {
            return Ok(());
        }
        self.check_name(&definition.metadata.qualified_name(), true)
    }
    pub(super) fn check_primitive(&self, name: &Name) -> Result<(), EvalErrorKind> {
        if !self.policy.borrow().active() {
            return Ok(());
        }
        if macros::primitive(name)
            || ["fn*", "defonce", "declare"]
                .iter()
                .any(|n| name == &Name::new(None, n))
        {
            self.check_name(&NamePolicy::canonical(name), true)?;
        }
        Ok(())
    }

    /// Copy namespace maps and configuration. Existing ordinary Vars, mutable
    /// values, code, streams, provider objects, and accounting remain shared.
    /// Builtin dynamic Vars get independent slots, so namespace and I/O selection
    /// are local. New namespaces/Vars are visible only in the creating branch.
    /// Fork is permitted only when the shared owner has no active evaluation.
    pub fn fork(&self) -> Result<Self, OptionsError> {
        if self.accounting.is_active() {
            return Err(OptionsError::ActiveEvaluation);
        }
        let registry = self.registry().borrow();
        let parent_current = self.current_namespace_id();
        let mut store = self.owner.store.borrow_mut();
        let mut remap = HashMap::new();
        for value in registry.entries.values() {
            let old = value.namespace_id().unwrap();
            let copy = store.namespaces.get(old.0).unwrap().clone();
            let new = NamespaceId(store.namespaces.insert(copy));
            remap.insert(old, new);
        }
        // A failed reload may leave the selected namespace intentionally
        // detached from the registry, while its current-value root stays live.
        remap.entry(parent_current).or_insert_with(|| {
            let copy = store.namespaces.get(parent_current.0).unwrap().clone();
            NamespaceId(store.namespaces.insert(copy))
        });
        for new in remap.values() {
            for target in store.namespaces.get_mut(new.0).aliases.values_mut() {
                if let Some(new) = remap.get(target) {
                    *target = *new;
                }
            }
        }
        let core = remap[&registry.core];
        let current = remap[&parent_current];
        let mut copied_vars = HashMap::new();
        let core_vars: Vec<_> = store
            .namespaces
            .get(core.0)
            .unwrap()
            .mappings
            .iter()
            .map(|(s, v)| (*s, *v))
            .collect();
        for (symbol, id) in core_vars {
            if !store.is_context_var(id) {
                continue;
            }
            let mut copy = store.vars.get(id.0).unwrap().clone();
            // Forking with a host BindingGuard snapshots its visible value but
            // does not share guard tokens or later restoration with the parent.
            copy.root = copy.bindings.last().map(|(_, v)| *v).or(copy.root);
            copy.bindings.clear();
            copy.namespace = core;
            if let Some(Value::Namespace(id)) = copy.root {
                copy.root = remap.get(&id).map(|n| Value::Namespace(*n));
            }
            let new = VarId(store.vars.insert(copy));
            store
                .namespaces
                .get_mut(core.0)
                .mappings
                .insert(symbol, new);
            copied_vars.insert(id, new);
        }
        drop(store);
        let mut var_remap = self.var_remap.borrow().clone();
        for value in var_remap.values_mut() {
            if let Some(id) = copied_vars.get(&value.var_id().unwrap()) {
                *value = self.owner.value(Value::Var(*id));
            }
        }
        var_remap.extend(
            copied_vars
                .into_iter()
                .map(|(old, new)| (old, self.owner.value(Value::Var(new)))),
        );
        let entries = registry
            .entries
            .iter()
            .map(|(name, value)| {
                (
                    name.clone(),
                    self.owner
                        .value(Value::Namespace(remap[&value.namespace_id().unwrap()])),
                )
            })
            .collect();
        let namespaces = OnceCell::new();
        let _ = namespaces.set(RefCell::new(namespaces::Registry {
            entries,
            current,
            core,
            current_value: self.owner.value(Value::Namespace(current)),
        }));
        Ok(Self {
            owner: self.owner.clone(),
            evaluation: evaluation::Scratch::default(),
            analysis_scratch: evaluation::AnalysisScratch::default(),
            reuse: reuse::AnalysisReuse::default(),
            reuse_revision: Rc::new(std::cell::Cell::new(0)),
            active_calls: 0,
            active_expressions: 0,
            accounting: self.accounting.clone(),
            namespaces: Rc::new(namespaces),
            readers: Rc::new(RefCell::new(self.readers.borrow().clone())),
            macros: macros::MacroState::default(),
            loading: Rc::new(RefCell::new(self.loading.borrow().clone())),
            builtin_var_mutation: Rc::new(std::cell::Cell::new(self.builtin_var_mutation.get())),
            policy: Rc::new(RefCell::new(self.policy.borrow().clone())),
            var_remap: Rc::new(RefCell::new(var_remap)),
            #[cfg(test)]
            evaluation_trace: Vec::new(),
        })
    }
    pub(super) fn local_namespace(&self, id: NamespaceId) -> Result<NamespaceId, NamespaceError> {
        let name = self.owner.store.borrow().namespaces.get(id.0)?.name.clone();
        self.find_namespace(&name)
            .and_then(|v| v.namespace_id())
            .or_else(|| (self.registry().borrow().current == id).then_some(id))
            .ok_or(NamespaceError::MissingNamespace(name))
    }
    pub(super) fn context_var(&self, id: VarId) -> Result<VarId, HandleError> {
        self.owner.store.borrow().vars.get(id.0)?;
        Ok(self
            .var_remap
            .borrow()
            .get(&id)
            .and_then(RootedValue::var_id)
            .unwrap_or(id))
    }
}

impl Store {
    pub(super) fn var_belongs_to(&self, var: VarId, namespace: NamespaceId) -> bool {
        let original = self.vars.get(var.0).unwrap().namespace;
        self.namespaces.get(original.0).unwrap().name
            == self.namespaces.get(namespace.0).unwrap().name
    }
    fn is_context_var(&self, id: VarId) -> bool {
        let var = self.vars.get(id.0).unwrap();
        var.builtin
            && [
                "*ns*",
                "*file*",
                "*in*",
                "*out*",
                "*err*",
                "*1",
                "*2",
                "*3",
                "*e",
                "*print-length*",
                "*print-level*",
                "*print-meta*",
                "*print-readably*",
                "*flush-on-newline*",
            ]
            .iter()
            .any(|name| {
                self.names
                    .get(var.name.0)
                    .unwrap()
                    .name()
                    .iter()
                    .copied()
                    .eq(name.encode_utf16())
            })
    }
}

pub(super) fn eval_definitions() -> &'static [NativeDefinition] {
    static DEFINITIONS: &[NativeDefinition] = &[NativeDefinition {
        metadata: NativeMetadata {
            namespace: "clojure.core",
            name: "eval",
            arity: Arity::Exact(1),
            effects: NativeEffects::Context,
            errors: &[NativeErrorClass::Invocation],
        },
        callback: |context, args| {
            let source = context.source("<eval-data>", "");
            let form = context
                .form_from_data(&args[0], &source.span(0..0).unwrap())
                .map_err(|error| {
                    NativeError::from(InvokeError::Execution(Box::new(error.into())))
                })?;
            context
                .eval_form(&form)
                .map_err(|error| InvokeError::Execution(Box::new(error)).into())
        },
    }];
    DEFINITIONS
}
