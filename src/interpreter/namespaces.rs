//! Context-owned namespace indexes and stable late-bound Vars. Registry roots
//! belong to Context; all graph edges between namespaces, Vars and code are IDs.
use super::*;

#[derive(Clone)]
pub(super) struct Namespace {
    pub(super) name: Name,
    pub(super) mappings: HashMap<SymbolId, VarId>,
    pub(super) aliases: HashMap<Name, NamespaceId>,
    pub(super) core_excludes: std::collections::HashSet<Name>,
}
#[derive(Clone)]
pub(super) struct Var {
    pub(super) namespace: NamespaceId,
    pub(super) name: SymbolId,
    pub(super) root: Option<Value>,
    pub(super) protocol: Option<protocols::ProtocolId>,
    pub(super) protocol_method: Option<(protocols::ProtocolId, usize)>,
    pub(super) metadata: Value,
    pub(super) metadata_set: bool,
    pub(super) builtin: bool,
    pub(super) exposed: bool,
    pub(super) bindings: Vec<(Rc<()>, Value)>,
}
pub(super) struct Registry {
    pub(super) entries: HashMap<Name, RootedValue>,
    pub(super) current: NamespaceId,
    pub(super) current_value: RootedValue,
    pub(super) core: NamespaceId,
    // Reuse only the immutable lookup key, never a mutable Var/mapping/binding.
    // Analysis-cache pruning calls current_namespace_id during collection.
    pub(super) current_var_name: Name,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NamespaceError {
    Handle(HandleError),
    Resource(ResourceError),
    PermissionDenied(Name),
    InvalidName,
    MissingNamespace(Name),
    ReferredVar(Name),
    ConflictingAlias(Name),
}
impl fmt::Display for NamespaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for NamespaceError {}
impl From<HandleError> for NamespaceError {
    fn from(error: HandleError) -> Self {
        Self::Handle(error)
    }
}

impl Store {
    pub(super) fn var_name(&self, id: VarId) -> Name {
        let var = self.vars.get(id.0).expect("live Var");
        let ns = &self
            .namespaces
            .get(var.namespace.0)
            .expect("live namespace")
            .name;
        Name::from_utf16(Some(ns.name()), self.names.get(var.name.0).unwrap().name())
    }
}

impl Context {
    pub(super) fn registry(&self) -> &RefCell<Registry> {
        self.namespaces.get_or_init(|| {
            let mut entries = HashMap::new();
            let mut make = |name: &str| {
                let name = Name::new(None, name);
                let _ = self.charge_allocation(1);
                let id = NamespaceId(self.owner.store.borrow_mut().namespaces.insert(Namespace {
                    name: name.clone(),
                    mappings: HashMap::new(),
                    aliases: HashMap::new(),
                    core_excludes: Default::default(),
                }));
                entries.insert(name, self.owner.value(Value::Namespace(id)));
                id
            };
            let core = make("clojure.core");
            let current = make("user");
            for name in [
                "clojure.string",
                "clojure.set",
                "clojure.walk",
                "clojure.edn",
            ] {
                make(name);
            }
            let current_value = entries[&Name::new(None, "user")].clone();
            RefCell::new(Registry {
                entries,
                current_value,
                current,
                core,
                current_var_name: Name::new(None, "*ns*"),
            })
        })
    }
    pub(super) fn current_namespace_id(&self) -> NamespaceId {
        let registry = self.registry().borrow();
        if let Some(id) = self.lookup_mapping(registry.core, &registry.current_var_name) {
            let store = self.owner.store.borrow();
            let var = store.vars.get(id.0).expect("namespace Var");
            if let Some(Value::Namespace(ns)) = var.bindings.last().map(|(_, v)| *v).or(var.root) {
                let name = &store.namespaces.get(ns.0).expect("namespace binding").name;
                return registry
                    .entries
                    .get(name)
                    .and_then(RootedValue::namespace_id)
                    .unwrap_or(registry.current);
            }
        }
        registry.current
    }
    pub fn current_namespace(&self) -> RootedValue {
        self.owner
            .value(Value::Namespace(self.current_namespace_id()))
    }
    pub fn find_namespace(&self, name: &Name) -> Option<RootedValue> {
        self.registry().borrow().entries.get(name).cloned()
    }
    pub fn create_namespace(&self, name: Name) -> Result<RootedValue, NamespaceError> {
        if name.namespace().is_some() || name.name().is_empty() {
            return Err(NamespaceError::InvalidName);
        }
        if let Some(ns) = self.find_namespace(&name) {
            return Ok(ns);
        }
        let _ = self.charge_allocation(1);
        let id = NamespaceId(self.owner.store.borrow_mut().namespaces.insert(Namespace {
            name: name.clone(),
            mappings: HashMap::new(),
            aliases: HashMap::new(),
            core_excludes: Default::default(),
        }));
        let ns = self.owner.value(Value::Namespace(id));
        self.registry()
            .borrow_mut()
            .entries
            .insert(name.clone(), ns.clone());
        if !self.take_alias_repair(&name) {
            return Ok(ns);
        }
        let count = self.registry().borrow().entries.len() as u64;
        self.accounting
            .work(count)
            .map_err(NamespaceError::Resource)?;
        self.charge_allocation(count)
            .map_err(NamespaceError::Resource)?;
        // SCI aliases name a namespace, so retry after a failed load must follow
        // its replacement identity and release the obsolete namespace graph.
        let namespace_ids: Vec<_> = self
            .registry()
            .borrow()
            .entries
            .values()
            .filter_map(RootedValue::namespace_id)
            .collect();
        let mut store = self.owner.store.borrow_mut();
        for namespace in namespace_ids {
            let count = store.namespaces.get(namespace.0)?.aliases.len() as u64;
            self.accounting
                .work(count)
                .map_err(NamespaceError::Resource)?;
            self.charge_allocation(count)
                .map_err(NamespaceError::Resource)?;
            let aliases: Vec<_> = store
                .namespaces
                .get(namespace.0)?
                .aliases
                .iter()
                .filter(|(_, target)| store.namespaces.get(target.0).unwrap().name == name)
                .map(|(alias, _)| alias.clone())
                .collect();
            for alias in aliases {
                store
                    .namespaces
                    .get_mut(namespace.0)
                    .aliases
                    .insert(alias, id);
            }
        }
        Ok(ns)
    }
    pub fn set_current_namespace(&mut self, id: NamespaceId) -> Result<(), NamespaceError> {
        let id = self.local_namespace(id)?;
        self.owner.store.borrow().namespaces.get(id.0)?;
        let core = self.registry().borrow().core;
        if let Some(var) = self.lookup_mapping(core, &Name::new(None, "*ns*")) {
            let mut store = self.owner.store.borrow_mut();
            let var = store.vars.get_mut(var.0);
            if let Some((_, value)) = var.bindings.last_mut() {
                *value = Value::Namespace(id);
                return Ok(());
            }
            var.root = Some(Value::Namespace(id));
        }
        let name = self.owner.store.borrow().namespaces.get(id.0)?.name.clone();
        let value = self
            .find_namespace(&name)
            .filter(|value| value.namespace_id() == Some(id))
            .unwrap_or_else(|| self.owner.value(Value::Namespace(id)));
        let mut registry = self.registry().borrow_mut();
        registry.current = id;
        registry.current_value = value;
        Ok(())
    }
    pub fn namespace_value(&self, id: NamespaceId) -> Result<RootedValue, HandleError> {
        self.owner.store.borrow().namespaces.get(id.0)?;
        Ok(self.owner.value(Value::Namespace(id)))
    }
    pub fn var_value(&self, id: VarId) -> Result<RootedValue, HandleError> {
        let id = self.context_var(id)?;
        self.owner.store.borrow().vars.get(id.0)?;
        Ok(self.owner.value(Value::Var(id)))
    }
    pub(super) fn lookup_mapping(&self, ns: NamespaceId, name: &Name) -> Option<VarId> {
        let store = self.owner.store.borrow();
        let symbol = store.interned.get(name)?;
        store
            .namespaces
            .get(ns.0)
            .ok()?
            .mappings
            .get(symbol)
            .copied()
    }
    /// Intern an unqualified name. Repeated calls preserve its Var identity;
    /// embedding intern replaces an existing refer with a new local Var.
    /// Language def separately rejects conflicts with explicit refers.
    pub fn intern_var(
        &self,
        namespace: NamespaceId,
        name: Name,
    ) -> Result<RootedValue, NamespaceError> {
        self.intern_var_inner(namespace, name, true)
    }
    pub(super) fn intern_language_var(
        &self,
        namespace: NamespaceId,
        name: Name,
    ) -> Result<RootedValue, NamespaceError> {
        self.intern_var_inner(namespace, name, false)
    }
    fn intern_var_inner(
        &self,
        namespace: NamespaceId,
        name: Name,
        exposed: bool,
    ) -> Result<RootedValue, NamespaceError> {
        let namespace = self.local_namespace(namespace)?;
        if name.namespace().is_some() || name.name().is_empty() {
            return Err(NamespaceError::InvalidName);
        }
        self.owner.store.borrow().namespaces.get(namespace.0)?;
        if let Some(id) = self.lookup_mapping(namespace, &name)
            && self.owner.store.borrow().var_belongs_to(id, namespace)
        {
            return Ok(self.owner.value(Value::Var(id)));
        }
        let symbol = self.intern(name);
        let mut store = self.owner.store.borrow_mut();
        let _ = self.charge_allocation(1);
        let id = VarId(store.vars.insert(Var {
            namespace,
            name: symbol,
            root: None,
            protocol: None,
            protocol_method: None,
            metadata: Value::Nil,
            metadata_set: false,
            builtin: false,
            exposed,
            bindings: Vec::new(),
        }));
        store
            .namespaces
            .get_mut(namespace.0)
            .mappings
            .insert(symbol, id);
        Ok(self.owner.value(Value::Var(id)))
    }
    /// Remove only the namespace mapping. Retained code/Var handles continue
    /// using the old identity, and a future intern allocates a fresh identity.
    pub fn unmap_var(&mut self, namespace: NamespaceId, name: &Name) -> Result<(), NamespaceError> {
        let namespace = self.local_namespace(namespace)?;
        if name.namespace().is_some() {
            return Err(NamespaceError::InvalidName);
        }
        let mut store = self.owner.store.borrow_mut();
        store.namespaces.get(namespace.0)?;
        if let Some(symbol) = store.interned.get(name).copied() {
            store
                .namespaces
                .get_mut(namespace.0)
                .mappings
                .remove(&symbol);
        }
        Ok(())
    }
    // Definition commits target the current mapping after effects have run.
    // Ordinary reads continue to use the Var ID selected during analysis.
    pub(super) fn definition_target(&self, analyzed: VarId) -> Result<VarId, NamespaceError> {
        let (ns, name) = {
            let store = self.owner.store.borrow();
            let var = store.vars.get(analyzed.0).expect("analyzed Var");
            (var.namespace, store.names.get(var.name.0).unwrap().clone())
        };
        let ns = self.local_namespace(ns)?;
        Ok(self.intern_language_var(ns, name)?.var_id().unwrap())
    }
    pub fn var_root(&self, id: VarId) -> Result<RootedValue, HandleError> {
        let id = self.context_var(id)?;
        let root = self
            .owner
            .store
            .borrow()
            .vars
            .get(id.0)?
            .root
            .unwrap_or(Value::Unbound(id));
        Ok(self.owner.value(root))
    }
    pub fn var_is_bound(&self, id: VarId) -> Result<bool, HandleError> {
        let id = self.context_var(id)?;
        let store = self.owner.store.borrow();
        let var = store.vars.get(id.0)?;
        Ok(var.root.is_some_and(|v| !matches!(v, Value::Unbound(_))) || !var.bindings.is_empty())
    }
    /// Trusted embedding installation, including replacement of builtins.
    /// Language mutation instead uses the context permission check.
    pub fn set_var_root(&mut self, id: VarId, value: &RootedValue) -> Result<(), HandleError> {
        self.set_var_root_internal(id, value)?;
        let id = self.context_var(id)?;
        self.owner.store.borrow_mut().vars.get_mut(id.0).exposed = true;
        Ok(())
    }
    pub(super) fn set_var_root_internal(
        &mut self,
        id: VarId,
        value: &RootedValue,
    ) -> Result<(), HandleError> {
        let id = self.context_var(id)?;
        if !Rc::ptr_eq(&self.owner, &value.owner) {
            return Err(HandleError::WrongOwner);
        }
        let mut store = self.owner.store.borrow_mut();
        store.vars.get(id.0)?;
        store.vars.get_mut(id.0).root = Some(value.value());
        Ok(())
    }
    /// Explicit embedding refer; loading code or discovering dependencies is
    /// deliberately outside namespace resolution.
    pub fn refer_var(
        &mut self,
        namespace: NamespaceId,
        name: Name,
        var: VarId,
    ) -> Result<(), NamespaceError> {
        let namespace = self.local_namespace(namespace)?;
        if name.namespace().is_some() || name.name().is_empty() {
            return Err(NamespaceError::InvalidName);
        }
        {
            let store = self.owner.store.borrow();
            store.namespaces.get(namespace.0)?;
            store.vars.get(var.0)?;
        }
        let symbol = self.intern(name);
        self.owner
            .store
            .borrow_mut()
            .namespaces
            .get_mut(namespace.0)
            .mappings
            .insert(symbol, var);
        Ok(())
    }
    /// Associate an existing namespace with a local qualifier. No loading occurs.
    pub fn alias_namespace(
        &mut self,
        namespace: NamespaceId,
        alias: Name,
        target: NamespaceId,
    ) -> Result<(), NamespaceError> {
        let namespace = self.local_namespace(namespace)?;
        let target = self.local_namespace(target)?;
        if alias.namespace().is_some() || alias.name().is_empty() {
            return Err(NamespaceError::InvalidName);
        }
        let mut store = self.owner.store.borrow_mut();
        store.namespaces.get(namespace.0)?;
        store.namespaces.get(target.0)?;
        if let Some(previous) = store.namespaces.get(namespace.0)?.aliases.get(&alias)
            && *previous != target
            && store.namespaces.get(previous.0)?.name != store.namespaces.get(target.0)?.name
        {
            return Err(NamespaceError::ConflictingAlias(alias));
        }
        let _ = self.charge_allocation(1);
        store
            .namespaces
            .get_mut(namespace.0)
            .aliases
            .insert(alias, target);
        Ok(())
    }
    pub fn unalias_namespace(
        &mut self,
        namespace: NamespaceId,
        alias: &Name,
    ) -> Result<(), NamespaceError> {
        let namespace = self.local_namespace(namespace)?;
        let mut store = self.owner.store.borrow_mut();
        store.namespaces.get(namespace.0)?;
        store.namespaces.get_mut(namespace.0).aliases.remove(alias);
        Ok(())
    }
    fn qualified_namespace(&self, namespace: NamespaceId, name: &Name) -> Option<NamespaceId> {
        // SCI syntax resolution gives real namespace names precedence over aliases.
        self.find_namespace(name)
            .and_then(|value| value.namespace_id())
            .or_else(|| {
                let store = self.owner.store.borrow();
                let target = store.namespaces.get(namespace.0).ok()?.aliases.get(name)?;
                let target_name = &store.namespaces.get(target.0).ok()?.name;
                self.find_namespace(target_name)
                    .and_then(|n| n.namespace_id())
            })
    }
    /// Auto-resolved reader syntax accepts aliases only, unlike symbol lookup.
    pub(super) fn reader_namespace(&self, alias: Option<&Name>) -> Option<Name> {
        let current = self.current_namespace_id();
        let store = self.owner.store.borrow();
        let id = match alias {
            Some(alias) => *store.namespaces.get(current.0).ok()?.aliases.get(alias)?,
            None => current,
        };
        Some(store.namespaces.get(id.0).ok()?.name.clone())
    }
    pub(super) fn syntax_quote_name(&self, name: &Name) -> Name {
        let current = self.current_namespace_id();
        if let Some(qualifier) = name.namespace() {
            return self
                .qualified_namespace(current, &Name::from_utf16(None, qualifier))
                .map(|id| {
                    Name::from_utf16(
                        Some(
                            self.owner
                                .store
                                .borrow()
                                .namespaces
                                .get(id.0)
                                .unwrap()
                                .name
                                .name(),
                        ),
                        name.name(),
                    )
                })
                .unwrap_or_else(|| name.clone());
        }
        if let Ok(Some(var)) = self.resolve_in(current, name) {
            return self.owner.store.borrow().var_name(var);
        }
        // Reader qualification also knows the reserved core macro surface; this
        // does not install executable Vars for features outside the current core.
        let text = String::from_utf16_lossy(name.name());
        if self.readers.borrow().compiler
            && matches!(text.as_ref(), "defonce" | "declare" | "exists?")
        {
            return Name::new(Some("clojure.core"), &text);
        }
        if [
            "let",
            "fn",
            "loop",
            "letfn",
            "defn",
            "and",
            "or",
            "when",
            "when-not",
            "cond",
            "->",
            "->>",
            "as->",
            "some->",
            "some->>",
            "cond->",
            "cond->>",
            "doto",
            "dotimes",
            "doseq",
            "for",
            "lazy-seq",
            "ns",
            "import",
            "binding",
            "with-open",
            "locking",
            "comment",
            "unquote",
            "expand-dot*",
            "expand-constructor",
        ]
        .contains(&text.as_str())
            && !self
                .owner
                .store
                .borrow()
                .namespaces
                .get(current.0)
                .unwrap()
                .core_excludes
                .contains(name)
        {
            return Name::from_utf16(
                Some(&"clojure.core".encode_utf16().collect::<Vec<_>>()),
                name.name(),
            );
        }
        if [
            "AssertionError",
            "Exception",
            "String",
            "ArithmeticException",
            "Integer",
            "Number",
            "Double",
            "Object",
        ]
        .contains(&text.as_str())
        {
            return Name::new(None, &format!("java.lang.{text}"));
        }
        if text.contains('.') {
            if !text.starts_with('.') && text.ends_with('.') {
                let base = self.syntax_quote_name(&Name::new(None, &text[..text.len() - 1]));
                let mut suffix = base.name().to_vec();
                suffix.push(u16::from(b'.'));
                return Name::from_utf16(base.namespace(), &suffix);
            }
            return name.clone();
        }
        let store = self.owner.store.borrow();
        Name::from_utf16(
            Some(store.namespaces.get(current.0).unwrap().name.name()),
            name.name(),
        )
    }
    pub(super) fn library_var(&self, namespace: NamespaceId, name: &Name) -> Option<VarId> {
        let namespace_name = self
            .owner
            .store
            .borrow()
            .namespaces
            .get(namespace.0)
            .ok()?
            .name
            .clone();
        let definition = library_definitions(&namespace_name)?
            .iter()
            .find(|d| name == &Name::new(None, d.metadata.name))?;
        if let Some(id) = self.lookup_mapping(namespace, name) {
            return Some(id);
        }
        let id = self.intern_var(namespace, name.clone()).ok()?.var_id()?;
        let metadata = self.var_base_metadata(id).and_then(|m| {
            self.assoc(
                &m,
                &self.keyword(Name::new(Some("sci"), "built-in")),
                &self.boolean(true),
            )
        });
        let Ok(metadata) = metadata else {
            let mut store = self.owner.store.borrow_mut();
            let symbol = store.vars.get(id.0).ok()?.name;
            store
                .namespaces
                .get_mut(namespace.0)
                .mappings
                .remove(&symbol);
            return None;
        };
        self.set_var_metadata(id, &metadata);
        let mut store = self.owner.store.borrow_mut();
        let var = store.vars.get_mut(id.0);
        var.root = Some(Value::Native(definition));
        var.builtin = true;
        var.exposed = true;
        Some(id)
    }
    pub(super) fn core_var(&self, name: &Name) -> Option<VarId> {
        let core = self.registry().borrow().core;
        if let Some(id) = self.lookup_mapping(core, name) {
            return Some(id);
        }
        if name == &Name::new(None, "unquote") {
            return self.intern_var(core, name.clone()).ok()?.var_id();
        }
        let definition = builtin_definitions()
            .iter()
            .chain(namespace_definitions())
            .chain(var_definitions())
            .chain(state_definitions())
            .chain(printing_definitions())
            .chain(io_definitions())
            .chain(string_core_definitions())
            .chain(loading::loading_definitions())
            .chain(exception_definitions())
            .chain(collection_definitions())
            .chain(sequence_definitions())
            .chain(deferred_definitions())
            .chain(lazy_definitions())
            .chain(macro_definitions())
            .chain(protocol_definitions())
            .chain(record_definitions())
            .chain(embedding::eval_definitions())
            .find(|d| {
                name.name()
                    .iter()
                    .copied()
                    .eq(d.metadata.name.encode_utf16())
            });
        let root = if name == &Name::new(None, "*ns*") {
            Value::Namespace(self.current_namespace_id())
        } else if name == &Name::new(None, "*file*") {
            self.loading
                .borrow()
                .source
                .as_ref()
                .map_or(Value::Nil, |s| {
                    self.string(s.name().encode_utf16().collect::<Vec<_>>())
                        .value()
                })
        } else if [
            "*in*",
            "*out*",
            "*err*",
            "*print-length*",
            "*print-level*",
            "*1",
            "*2",
            "*3",
            "*e",
        ]
        .iter()
        .any(|n| name == &Name::new(None, n))
        {
            Value::Nil
        } else if name == &Name::new(None, "*print-meta*") {
            Value::Boolean(false)
        } else if name == &Name::new(None, "*print-readably*")
            || name == &Name::new(None, "*flush-on-newline*")
        {
            Value::Boolean(true)
        } else {
            Value::Native(definition?)
        };
        let id = self
            .intern_var(core, name.clone())
            .unwrap()
            .var_id()
            .unwrap();
        let metadata = (|| -> Result<_, CollectionError> {
            let dynamic = [
                "*ns*",
                "*file*",
                "*print-length*",
                "*print-level*",
                "*print-meta*",
                "*print-readably*",
                "*flush-on-newline*",
                "*in*",
                "*out*",
                "*err*",
                "*1",
                "*2",
                "*3",
                "*e",
            ]
            .iter()
            .any(|n| name == &Name::new(None, n));
            let mut meta = self.var_base_metadata(id)?;
            meta = self.assoc(
                &meta,
                &self.keyword(Name::new(Some("sci"), "built-in")),
                &self.boolean(true),
            )?;
            if dynamic {
                meta = self.assoc(
                    &meta,
                    &self.keyword(Name::new(None, "dynamic")),
                    &self.boolean(true),
                )?;
            }
            if macros::core_macro(name) {
                meta = self.assoc(
                    &meta,
                    &self.keyword(Name::new(None, "macro")),
                    &self.boolean(true),
                )?;
            }
            self.accounting.check().map_err(CollectionError::Resource)?;
            Ok(meta)
        })();
        let Ok(metadata) = metadata else {
            // Never cache a half-initialized builtin after a resource failure.
            let mut store = self.owner.store.borrow_mut();
            let symbol = store.vars.get(id.0).unwrap().name;
            store.namespaces.get_mut(core.0).mappings.remove(&symbol);
            return None;
        };
        {
            let mut store = self.owner.store.borrow_mut();
            let var = store.vars.get_mut(id.0);
            var.root = Some(root);
            var.builtin = true;
            var.exposed = true;
        }
        self.set_var_metadata(id, &metadata);
        Some(id)
    }
    pub(super) fn resolve_in(
        &self,
        namespace: NamespaceId,
        name: &Name,
    ) -> Result<Option<VarId>, NamespaceError> {
        let namespace = self.local_namespace(namespace)?;
        self.owner.store.borrow().namespaces.get(namespace.0)?;
        let target = if let Some(ns) = name.namespace() {
            let Some(target) = self.qualified_namespace(namespace, &Name::from_utf16(None, ns))
            else {
                return Ok(None);
            };
            target
        } else {
            namespace
        };
        let bare = Name::from_utf16(None, name.name());
        let core = self.registry().borrow().core;
        let library_namespace =
            library_definitions(&self.owner.store.borrow().namespaces.get(target.0)?.name)
                .is_some();
        let id = self.lookup_mapping(target, &bare).or_else(|| {
            if library_namespace && let Some(id) = self.library_var(target, &bare) {
                return Some(id);
            }
            if target == core
                || (name.namespace().is_none()
                    && !self
                        .owner
                        .store
                        .borrow()
                        .namespaces
                        .get(target.0)
                        .unwrap()
                        .core_excludes
                        .contains(&bare))
            {
                self.core_var(&bare)
            } else {
                None
            }
        });
        if let Some(id) = id {
            self.check_var_access(id).map_err(|error| match error {
                EvalErrorKind::PermissionDenied(name) => NamespaceError::PermissionDenied(name),
                _ => NamespaceError::Handle(HandleError::WrongOwner),
            })?;
        }
        Ok(id)
    }
    /// Find an interned Var in an exact namespace, without caller aliases,
    /// refers or unqualified core fallback. Used by external macro resolvers.
    pub fn find_interned_var(
        &self,
        namespace: &Name,
        name: &Name,
    ) -> Result<Option<RootedValue>, NamespaceError> {
        if namespace.namespace().is_some() || name.namespace().is_some() {
            return Err(NamespaceError::InvalidName);
        }
        let Some(target) = self
            .find_namespace(namespace)
            .and_then(|n| n.namespace_id())
        else {
            return Ok(None);
        };
        let id = self
            .lookup_mapping(target, name)
            .filter(|id| {
                self.owner
                    .store
                    .borrow()
                    .vars
                    .get(id.0)
                    .is_ok_and(|v| v.namespace == target)
            })
            .or_else(|| {
                if namespace == &Name::new(None, "clojure.core") {
                    self.core_var(name)
                } else {
                    self.library_var(target, name)
                }
            });
        if let Some(id) = id {
            self.check_var_access(id).map_err(|error| match error {
                EvalErrorKind::PermissionDenied(name) => NamespaceError::PermissionDenied(name),
                _ => NamespaceError::Handle(HandleError::WrongOwner),
            })?;
        }
        Ok(id.map(|id| self.owner.value(Value::Var(id))))
    }
    pub fn resolve_var(&self, name: &Name) -> Result<Option<RootedValue>, NamespaceError> {
        let namespace = self.current_namespace_id();
        Ok(self
            .resolve_in(namespace, name)?
            .map(|id| self.owner.value(Value::Var(id))))
    }
    pub(super) fn var_private(&self, id: VarId) -> bool {
        let meta = self
            .owner
            .store
            .borrow()
            .vars
            .get(id.0)
            .expect("live Var")
            .metadata;
        let meta = self.owner.value(meta);
        let key = self.keyword(Name::new(None, "private"));
        meta.get(&key)
            .ok()
            .flatten()
            .is_some_and(|v| !matches!(v.inspect(), ValueView::Nil | ValueView::Boolean(false)))
    }
    pub(super) fn var_base_metadata(&self, id: VarId) -> Result<RootedValue, CollectionError> {
        let (namespace, name) = {
            let store = self.owner.store.borrow();
            let v = store.vars.get(id.0).unwrap();
            (v.namespace, v.name)
        };
        self.map(&[
            (
                self.keyword(Name::new(None, "name")),
                self.owner.value(Value::Symbol(name)),
            ),
            (
                self.keyword(Name::new(None, "ns")),
                self.owner.value(Value::Namespace(namespace)),
            ),
        ])
    }
    pub(super) fn set_var_metadata(&self, id: VarId, metadata: &RootedValue) {
        let mut store = self.owner.store.borrow_mut();
        let var = store.vars.get_mut(id.0);
        var.metadata = metadata.value();
        var.metadata_set = true;
    }
    pub(super) fn namespace_mappings(
        &self,
        ns: NamespaceId,
        operation: Operation,
    ) -> Result<RootedValue, CollectionError> {
        use Operation::*;
        let core = self.registry().borrow().core;
        let namespace_name = self
            .owner
            .store
            .borrow()
            .namespaces
            .get(ns.0)
            .unwrap()
            .name
            .clone();
        if let Some(definitions) = library_definitions(&namespace_name) {
            for d in definitions {
                self.library_var(ns, &Name::new(None, d.metadata.name));
            }
        }
        if matches!(operation, NsMap | NsRefers) || ns == core {
            for d in builtin_definitions()
                .iter()
                .chain(namespace_definitions())
                .chain(var_definitions())
                .chain(state_definitions())
                .chain(printing_definitions())
                .chain(io_definitions())
                .chain(string_core_definitions())
                .chain(loading::loading_definitions())
                .chain(exception_definitions())
                .chain(collection_definitions())
                .chain(sequence_definitions())
                .chain(deferred_definitions())
                .chain(lazy_definitions())
                .chain(macro_definitions())
                .chain(protocol_definitions())
                .chain(record_definitions())
                .chain(embedding::eval_definitions())
            {
                self.core_var(&Name::new(None, d.metadata.name));
            }
            for name in [
                "*ns*",
                "*print-length*",
                "*print-level*",
                "*print-meta*",
                "*print-readably*",
                "*flush-on-newline*",
                "*in*",
                "*out*",
                "*err*",
                "*1",
                "*2",
                "*3",
                "*e",
            ] {
                self.core_var(&Name::new(None, name));
            }
        }
        let store = self.owner.store.borrow();
        let size = store
            .namespaces
            .get(core.0)
            .unwrap()
            .mappings
            .len()
            .saturating_add(store.namespaces.get(ns.0).unwrap().mappings.len());
        self.charge_allocation(size as u64)
            .map_err(CollectionError::Resource)?;
        self.accounting
            .work(size as u64)
            .map_err(CollectionError::Resource)?;
        let mut mappings = if matches!(operation, NsMap | NsRefers) && ns != core {
            store
                .namespaces
                .get(core.0)
                .unwrap()
                .mappings
                .iter()
                .filter(|(name, _)| {
                    !store
                        .namespaces
                        .get(ns.0)
                        .unwrap()
                        .core_excludes
                        .contains(store.names.get(name.0).unwrap())
                })
                .map(|(name, var)| (*name, *var))
                .collect()
        } else {
            HashMap::new()
        };
        mappings.extend(
            store
                .namespaces
                .get(ns.0)
                .unwrap()
                .mappings
                .iter()
                .filter(|(_, v)| !matches!(operation, NsRefers) || !store.var_belongs_to(**v, ns))
                .map(|(k, v)| (*k, *v)),
        );
        drop(store);
        let mut entries = Vec::new();
        for (symbol, var) in mappings {
            let own = self.owner.store.borrow().var_belongs_to(var, ns);
            if (matches!(operation, NsInterns | NsPublics) && !own)
                || (matches!(operation, NsRefers) && own)
                || (matches!(operation, NsPublics) && self.var_private(var))
            {
                continue;
            }
            entries.push((
                self.owner.value(Value::Symbol(symbol)),
                self.owner.value(Value::Var(var)),
            ));
        }
        self.map(&entries)
    }
}

#[derive(Clone, Copy)]
pub(super) enum Operation {
    Deref,
    VarGet,
    VarP,
    BoundP,
    Resolve,
    NsResolve,
    InNs,
    NsName,
    FindNs,
    CreateNs,
    TheNs,
    AllNs,
    NsMap,
    NsInterns,
    NsPublics,
    NsRefers,
    Meta,
    NsUnmap,
    Alias,
    NsUnalias,
    WithMeta,
}
fn type_error(argument: usize, expected: &'static str) -> NativeError {
    NativeError::Type { argument, expected }
}
fn host(error: impl fmt::Display) -> NativeError {
    NativeError::Host(error.to_string())
}
fn namespace_failure(error: NamespaceError) -> NativeError {
    match error {
        NamespaceError::PermissionDenied(name) => {
            InvokeError::Dispatch(EvalErrorKind::PermissionDenied(name)).into()
        }
        other => host(other),
    }
}
fn symbol(args: &[RootedValue], i: usize) -> Result<Name, NativeError> {
    match args[i].inspect() {
        ValueView::Symbol(n) => Ok(n),
        _ => Err(type_error(i, "symbol")),
    }
}
fn namespace(context: &Context, value: &RootedValue, i: usize) -> Result<NamespaceId, NativeError> {
    if let Some(id) = value.namespace_id() {
        return context.local_namespace(id).map_err(host);
    }
    if let ValueView::Symbol(name) = value.inspect() {
        return context
            .find_namespace(&name)
            .and_then(|n| n.namespace_id())
            .ok_or_else(|| host(NamespaceError::MissingNamespace(name)));
    }
    Err(type_error(i, "namespace or symbol"))
}
fn call(context: &mut Context, args: &[RootedValue], operation: Operation) -> NativeResult {
    use Operation::*;
    match operation {
        Deref | VarGet => {
            if matches!(operation, Deref)
                && let Some(value) = args[0].state_value()
            {
                return Ok(value);
            }
            if matches!(operation, Deref) && matches!(args[0].inspect(), ValueView::Delay) {
                return context.force(&args[0]);
            }
            if matches!(operation, Deref)
                && let Some(value) = args[0].reduced_value()
            {
                return Ok(value);
            }
            let id = args[0].var_id().ok_or_else(|| type_error(0, "Var"))?;
            context
                .check_var_access(id)
                .map_err(InvokeError::Dispatch)?;
            context.var_get(id).map_err(host)
        }
        VarP => Ok(context.boolean(args[0].var_id().is_some())),
        BoundP => {
            let mut bound = true;
            for (i, value) in args.iter().enumerate() {
                bound &= context
                    .var_is_bound(value.var_id().ok_or_else(|| type_error(i, "Var"))?)
                    .map_err(host)?;
            }
            Ok(context.boolean(bound))
        }
        Resolve | NsResolve => {
            let (ns, env, index) = if matches!(operation, Resolve) {
                (
                    context.current_namespace_id(),
                    (args.len() == 2).then_some(0),
                    args.len() - 1,
                )
            } else {
                (
                    namespace(context, &args[0], 0)?,
                    (args.len() == 3).then_some(1),
                    args.len() - 1,
                )
            };
            let name = symbol(args, index)?;
            if let Some(env) = env
                && !matches!(args[env].inspect(), ValueView::Nil)
                && args[env].contains(&args[index]).map_err(host)?
            {
                return Ok(context.nil());
            }
            Ok(context
                .resolve_in(ns, &name)
                .map_err(namespace_failure)?
                .map_or_else(|| context.nil(), |id| context.owner.value(Value::Var(id))))
        }
        InNs | CreateNs => {
            let value = context.create_namespace(symbol(args, 0)?).map_err(host)?;
            if matches!(operation, InNs) {
                context
                    .set_current_namespace(value.namespace_id().unwrap())
                    .map_err(host)?;
            }
            Ok(value)
        }
        FindNs => Ok(match args[0].inspect() {
            ValueView::Symbol(name) => context
                .find_namespace(&name)
                .unwrap_or_else(|| context.nil()),
            _ => context.nil(),
        }),
        TheNs => context
            .namespace_value(namespace(context, &args[0], 0)?)
            .map_err(host),
        NsName => {
            let id = namespace(context, &args[0], 0)?;
            let name = context
                .owner
                .store
                .borrow()
                .namespaces
                .get(id.0)
                .unwrap()
                .name
                .clone();
            Ok(context.symbol(name))
        }
        AllNs => {
            let values: Vec<_> = context
                .registry()
                .borrow()
                .entries
                .values()
                .cloned()
                .collect();
            context.list(&values).map_err(host)
        }
        NsMap | NsInterns | NsPublics | NsRefers => {
            Ok(context.namespace_mappings(namespace(context, &args[0], 0)?, operation)?)
        }
        WithMeta => {
            if matches!(args[0].inspect(), ValueView::LazySeq)
                && args[0].deferred_status().is_some()
            {
                context.seq(&args[0])?;
            }
            Ok(context.with_meta(&args[0], &args[1])?)
        }
        Alias => {
            let alias = symbol(args, 0)?;
            let target = namespace(context, &args[1], 1)?;
            let current = context.current_namespace_id();
            context
                .alias_namespace(current, alias, target)
                .map_err(host)?;
            Ok(context.nil())
        }
        NsUnalias => {
            let ns = namespace(context, &args[0], 0)?;
            context
                .unalias_namespace(ns, &symbol(args, 1)?)
                .map_err(host)?;
            Ok(context.nil())
        }
        Meta => {
            let value = if let Some(id) = args[0].var_id() {
                context
                    .check_var_access(id)
                    .map_err(InvokeError::Dispatch)?;
                let id = context.context_var(id).map_err(host)?;
                if !context
                    .owner
                    .store
                    .borrow()
                    .vars
                    .get(id.0)
                    .unwrap()
                    .metadata_set
                {
                    return Ok(context.var_base_metadata(id)?);
                }
                context.var_value(id).map_err(host)?
            } else {
                args[0].clone()
            };
            Ok(value.metadata().unwrap_or_else(|| context.nil()))
        }
        NsUnmap => {
            let ns = namespace(context, &args[0], 0)?;
            context.unmap_var(ns, &symbol(args, 1)?).map_err(host)?;
            Ok(context.nil())
        }
    }
}
macro_rules! def {
    ($name:literal, $arity:expr, $op:ident) => {
        NativeDefinition {
            metadata: NativeMetadata {
                namespace: "clojure.core",
                name: $name,
                arity: $arity,
                effects: NativeEffects::Context,
                errors: &[
                    NativeErrorClass::Arity,
                    NativeErrorClass::Type,
                    NativeErrorClass::Host,
                ],
            },
            callback: |context, args| call(context, args, Operation::$op),
        }
    };
}
pub fn namespace_definitions() -> &'static [NativeDefinition] {
    use Arity::{Exact, Range};
    static DEFINITIONS: &[NativeDefinition] = &[
        def!("deref", Exact(1), Deref),
        def!("var-get", Exact(1), VarGet),
        def!("var?", Exact(1), VarP),
        def!("bound?", Exact(1), BoundP),
        def!("resolve", Range { min: 1, max: 2 }, Resolve),
        def!("ns-resolve", Range { min: 2, max: 3 }, NsResolve),
        def!("in-ns", Exact(1), InNs),
        def!("create-ns", Exact(1), CreateNs),
        def!("find-ns", Exact(1), FindNs),
        def!("the-ns", Exact(1), TheNs),
        def!("ns-name", Exact(1), NsName),
        def!("all-ns", Exact(0), AllNs),
        def!("ns-map", Exact(1), NsMap),
        def!("ns-interns", Exact(1), NsInterns),
        def!("ns-publics", Exact(1), NsPublics),
        def!("ns-refers", Exact(1), NsRefers),
        def!("meta", Exact(1), Meta),
        def!("with-meta", Exact(2), WithMeta),
        def!("alias", Exact(2), Alias),
        def!("ns-unalias", Exact(2), NsUnalias),
        def!("ns-unmap", Exact(2), NsUnmap),
    ];
    DEFINITIONS
}

/// Native libraries share lazy Var installation, introspection and require.
pub(super) fn library_definitions(name: &Name) -> Option<&'static [NativeDefinition]> {
    if name == &Name::new(None, "clojure.string") {
        Some(string_definitions())
    } else if name == &Name::new(None, "clojure.set") {
        Some(set_definitions())
    } else if name == &Name::new(None, "clojure.walk") {
        Some(walk_definitions())
    } else if name == &Name::new(None, "clojure.edn") {
        Some(io::edn_definitions())
    } else {
        None
    }
}
