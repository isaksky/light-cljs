//! Explicit host loading, separate from the compiler's `.cljs` search contract.
use super::*;
use std::{collections::HashSet, path::PathBuf};

pub use crate::source_provider::{LoadError, LoadRequest, Reload, SourceInput, SourceProvider};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoadState {
    Unloaded,
    Loading,
    Loaded,
    Failed,
}
/// Opt-in CLI/embedding adapter. Construction performs no filesystem access.
/// Every configured root is searched for `.clj` and `.cljc`; more than one
/// distinct canonical candidate is an error, independent of root ordering.
#[derive(Clone, Debug)]
pub struct FileSourceProvider {
    roots: Vec<PathBuf>,
}
impl FileSourceProvider {
    pub fn new(roots: impl IntoIterator<Item = PathBuf>) -> Self {
        Self {
            roots: roots.into_iter().collect(),
        }
    }
}
impl SourceProvider for FileSourceProvider {
    fn load(&self, request: &LoadRequest) -> Result<Option<SourceInput>, LoadError> {
        let name = String::from_utf16(request.namespace.name()).map_err(|_| {
            LoadError::InvalidLibspec("file namespace must be valid Unicode".into())
        })?;
        if request.namespace.namespace().is_some()
            || name.split('.').any(str::is_empty)
            || name.contains(['/', '\\', ':', '\0'])
        {
            return Err(LoadError::InvalidLibspec("invalid file namespace".into()));
        }
        let relative = name.replace('.', "/").replace('-', "_");
        let mut candidates = Vec::new();
        for root in &self.roots {
            for extension in ["clj", "cljc"] {
                let path = root.join(format!("{relative}.{extension}"));
                match std::fs::metadata(&path) {
                    Ok(meta) if meta.is_file() => {
                        let path = std::fs::canonicalize(&path)
                            .map_err(|e| LoadError::Provider(e.to_string()))?;
                        if !candidates.contains(&path) {
                            candidates.push(path);
                        }
                    }
                    Ok(_) => (),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                    Err(e) => return Err(LoadError::Provider(e.to_string())),
                }
            }
        }
        if candidates.len() > 1 {
            return Err(LoadError::AmbiguousSources(candidates));
        }
        let Some(path) = candidates.pop() else {
            return Ok(None);
        };
        let source =
            std::fs::read_to_string(&path).map_err(|e| LoadError::Provider(e.to_string()))?;
        Ok(Some(SourceInput::new(path.to_string_lossy(), source)))
    }
}
#[derive(Clone)]
struct Record {
    state: LoadState,
    // SCI allows a back-reference after the target's ns clauses have completed.
    declared: bool,
}
#[derive(Clone, Default)]
pub(super) struct Loader {
    pub(super) provider: Option<Rc<dyn SourceProvider>>,
    records: HashMap<Name, Record>,
    discarded: HashSet<Name>,
    stack: Vec<Name>,
    reload_all: bool,
    pub(super) source: Option<Source>,
}
pub(super) struct SourceContext {
    previous: Option<Source>,
    bindings: BindingGuard,
    persist_namespace: bool,
}
fn failure(error: LoadError) -> NativeError {
    InvokeError::Dispatch(EvalErrorKind::Load(error)).into()
}
fn invalid(message: &str) -> NativeError {
    failure(LoadError::InvalidLibspec(message.into()))
}
fn symbol(value: &RootedValue) -> Result<Name, NativeError> {
    match value.inspect() {
        ValueView::Symbol(name) if name.namespace().is_none() && !name.name().is_empty() => {
            Ok(name)
        }
        _ => Err(invalid("expected an unqualified symbol")),
    }
}
fn keyword(value: &RootedValue) -> Option<String> {
    match value.inspect() {
        ValueView::Keyword(name) if name.namespace().is_none() => {
            Some(String::from_utf16_lossy(name.name()))
        }
        _ => None,
    }
}
fn symbols(value: &RootedValue) -> Result<Vec<Name>, NativeError> {
    value
        .owner
        .accounting
        .work(value.count().unwrap_or(0) as u64)?;
    if !matches!(
        value.inspect(),
        ValueView::List(_) | ValueView::Vector(_) | ValueView::Seq(_)
    ) {
        return Err(invalid("expected a sequential collection of symbols"));
    }
    value.elements()?.iter().map(symbol).collect()
}
#[derive(Default)]
struct Options {
    alias: Option<Name>,
    alias_only: Option<Name>,
    refer: Option<Vec<Name>>, // None without all means no refers.
    all: bool,
    rename: HashMap<Name, Name>,
    exclude: HashSet<Name>,
    reload: Reload,
}
fn options(values: &[RootedValue], refer_options: bool) -> Result<Options, NativeError> {
    if !values.len().is_multiple_of(2) {
        return Err(invalid("libspec options require key/value pairs"));
    }
    if let Some(value) = values.first() {
        value.owner.accounting.work(values.len() as u64)?;
    }
    let mut result = Options::default();
    let mut seen = HashSet::new();
    for pair in values.as_chunks::<2>().0 {
        let key = keyword(&pair[0]).ok_or_else(|| invalid("option names must be keywords"))?;
        if !seen.insert(key.clone()) {
            return Err(invalid("duplicate option"));
        }
        match key.as_str() {
            "as" if !refer_options => result.alias = Some(symbol(&pair[1])?),
            "as-alias" if !refer_options => result.alias_only = Some(symbol(&pair[1])?),
            "refer" | "only" if key == "refer" || refer_options => {
                if keyword(&pair[1]).as_deref() == Some("all") {
                    result.all = true;
                } else {
                    result.refer = Some(symbols(&pair[1])?);
                }
            }
            "exclude" => result.exclude.extend(symbols(&pair[1])?),
            "rename" => {
                if !matches!(pair[1].inspect(), ValueView::Map(_)) {
                    return Err(invalid(":rename requires a symbol map"));
                }
                pair[1]
                    .owner
                    .accounting
                    .work(pair[1].count().unwrap_or(0) as u64)?;
                for (from, to) in pair[1].entries()? {
                    result.rename.insert(symbol(&from)?, symbol(&to)?);
                }
            }
            "reload" | "reload-all" if !refer_options => {
                if !matches!(pair[1].inspect(), ValueView::Boolean(_)) {
                    return Err(invalid("reload option requires a boolean"));
                }
                if matches!(pair[1].inspect(), ValueView::Boolean(true)) {
                    result.reload = if key == "reload-all" {
                        Reload::All
                    } else {
                        Reload::Namespace
                    };
                }
            }
            _ => return Err(invalid("unsupported libspec option")),
        }
    }
    if result.alias.is_some() && result.alias_only.is_some() {
        return Err(invalid(":as and :as-alias are mutually exclusive"));
    }
    Ok(result)
}
fn libspec(
    value: &RootedValue,
    prefix: Option<&Name>,
    specs: &mut Vec<(Name, Options)>,
) -> Result<(), NativeError> {
    value
        .owner
        .accounting
        .work(value.count().unwrap_or(1) as u64)?;
    let (name, opts) = match value.inspect() {
        ValueView::Symbol(_) => (symbol(value)?, Options::default()),
        ValueView::List(_) | ValueView::Vector(_) => {
            let values = value.elements()?;
            let Some(first) = values.first() else {
                return Err(invalid("empty libspec"));
            };
            let name = symbol(first)?;
            if values.len() > 1 && keyword(&values[1]).is_none() {
                if prefix.is_some() {
                    return Err(invalid("nested prefix lists are unsupported"));
                }
                for value in &values[1..] {
                    libspec(value, Some(&name), specs)?;
                }
                return Ok(());
            }
            (name, options(&values[1..], false)?)
        }
        _ => {
            return Err(invalid(
                "require expects symbol, vector libspec, or prefix list",
            ));
        }
    };
    let name = if let Some(prefix) = prefix {
        if name.name().contains(&u16::from(b'.')) {
            return Err(invalid(
                "lib names inside prefix lists must not contain periods",
            ));
        }
        let mut units = prefix.name().to_vec();
        units.push(u16::from(b'.'));
        units.extend_from_slice(name.name());
        Name::from_utf16(None, &units)
    } else {
        name
    };
    value
        .owner
        .accounting
        .charge(ResourceLimit::Allocation, 1)?;
    specs.push((name, opts));
    Ok(())
}
impl Context {
    pub fn set_source_provider(&mut self, provider: impl SourceProvider + 'static) {
        self.invalidate_analysis();
        self.loading.borrow_mut().provider = Some(Rc::new(provider));
    }
    pub(super) fn take_alias_repair(&self, name: &Name) -> bool {
        self.loading.borrow_mut().discarded.remove(name)
    }
    pub fn clear_source_provider(&mut self) {
        self.invalidate_analysis();
        self.loading.borrow_mut().provider = None;
    }
    pub(crate) fn compiler_assertions(&mut self, enabled: bool) {
        let namespace = self
            .find_namespace(&Name::new(None, "clojure.core"))
            .unwrap();
        let var = self
            .intern_var(
                namespace.namespace_id().unwrap(),
                Name::new(None, "*assert*"),
            )
            .unwrap();
        let metadata = self
            .map(&[(self.keyword(Name::new(None, "dynamic")), self.boolean(true))])
            .unwrap();
        self.set_var_metadata(var.var_id().unwrap(), &metadata);
        self.set_var_root(var.var_id().unwrap(), &self.boolean(enabled))
            .unwrap();
    }
    pub(crate) fn initialize_compiler_host(&self) {
        self.readers.borrow_mut().compiler = true;
        // The default `user` identity is ambient reader state, not loaded user.clj.
        self.loading.borrow_mut().records.insert(
            Name::new(None, "user"),
            Record {
                state: LoadState::Unloaded,
                declared: false,
            },
        );
    }
    // A CLJS caller needs *ns* identity but has not loaded same-named .clj code.
    // Keep this explicit state so normal require (including transitive requires)
    // never mistakes the compiler's caller namespace for a preloaded library.
    pub(crate) fn compiler_namespace(&self, name: Name) -> Result<RootedValue, NamespaceError> {
        let existed = self.find_namespace(&name).is_some();
        let namespace = self.create_namespace(name.clone())?;
        if !existed {
            self.loading.borrow_mut().records.insert(
                name,
                Record {
                    state: LoadState::Unloaded,
                    declared: false,
                },
            );
        }
        Ok(namespace)
    }
    pub fn namespace_load_state(&self, namespace: &Name) -> LoadState {
        self.loading.borrow().records.get(namespace).map_or_else(
            || {
                if self.find_namespace(namespace).is_some() {
                    LoadState::Loaded
                } else {
                    LoadState::Unloaded
                }
            },
            |record| record.state,
        )
    }
    pub(super) fn enter_source_context(
        &mut self,
        source: &Source,
    ) -> Result<SourceContext, VarError> {
        let file = self.core_var(&Name::new(None, "*file*"));
        let ns = self.core_var(&Name::new(None, "*ns*"));
        self.accounting.check()?;
        let file = file.expect("core file Var");
        let ns = ns.expect("core namespace Var");
        // A string reentry receives its own namespace slot, while inherited file
        // slots remain shared. Only a named source establishes a new file slot.
        let persist_namespace =
            self.loading.borrow().source.is_none() && !self.var_is_thread_bound(ns)?;
        let mut values = vec![(ns, self.var_get(ns)?)];
        if source.name() != "<eval>" {
            values.push((
                file,
                self.string(source.name().encode_utf16().collect::<Vec<_>>()),
            ));
        }
        let bindings = self.bind_vars(&values)?;
        let previous = self.loading.borrow_mut().source.replace(source.clone());
        Ok(SourceContext {
            previous,
            bindings,
            persist_namespace,
        })
    }
    pub(super) fn restore_source_context(&mut self, previous: SourceContext) {
        let namespace = self.current_namespace_id();
        self.loading.borrow_mut().source = previous.previous;
        drop(previous.bindings);
        // The embedding Context retains top-level REPL namespace selection;
        // recursive evaluations and externally bound namespaces restore instead.
        if previous.persist_namespace {
            self.set_current_namespace(namespace)
                .expect("retained namespace");
        }
    }
    fn current_ns_name(&self) -> Name {
        let id = self.current_namespace_id();
        self.owner
            .store
            .borrow()
            .namespaces
            .get(id.0)
            .unwrap()
            .name
            .clone()
    }
    fn apply_refers(&mut self, target: NamespaceId, opts: &Options) -> NativeResult {
        let current = self.current_namespace_id();
        let names = if opts.all {
            self.namespace_mappings(target, namespaces::Operation::NsInterns)?
                .entries()?
                .iter()
                .map(|(name, _)| symbol(name))
                .collect::<Result<Vec<_>, _>>()?
        } else {
            opts.refer.clone().unwrap_or_default()
        };
        // Validate every requested Var before changing mappings. Explicit private
        // refers and :refer :all include private interns, matching pinned SCI.
        let mut mappings = Vec::new();
        for name in names {
            self.accounting.work(1)?;
            if opts.exclude.contains(&name) {
                continue;
            }
            let core = self.registry().borrow().core;
            let var = if target == core {
                self.core_var(&name)
            } else {
                self.lookup_mapping(target, &name)
                    .or_else(|| self.library_var(target, &name))
            }
            .ok_or_else(|| failure(LoadError::MissingVar(name.clone())))?;
            mappings.push((opts.rename.get(&name).unwrap_or(&name).clone(), var));
        }
        for (name, var) in mappings {
            self.refer_var(current, name, var)
                .map_err(|e| NativeError::Host(e.to_string()))?;
        }
        Ok(self.nil())
    }
    fn require_one(&mut self, name: Name, opts: Options) -> NativeResult {
        self.accounting.work(1)?;
        let current = self.current_namespace_id();
        if let Some(alias) = opts.alias_only {
            let existed = self.find_namespace(&name).is_some();
            let target = self
                .create_namespace(name.clone())
                .map_err(|e| NativeError::Host(e.to_string()))?
                .namespace_id()
                .unwrap();
            if !existed {
                self.loading.borrow_mut().records.insert(
                    name,
                    Record {
                        state: LoadState::Unloaded,
                        declared: false,
                    },
                );
            }
            self.alias_namespace(current, alias, target)
                .map_err(|e| NativeError::Host(e.to_string()))?;
            return Ok(self.nil());
        }
        let mut reload = opts.reload;
        if self.loading.borrow().reload_all {
            reload = Reload::All;
        }
        let state = self.namespace_load_state(&name);
        if state == LoadState::Loading {
            let loading = self.loading.borrow();
            let record = &loading.records[&name];
            if name != self.current_ns_name() && !record.declared {
                let mut path = loading.stack.clone();
                path.push(name);
                return Err(failure(LoadError::Cycle(path)));
            }
        } else if name != Name::new(None, "clojure.core")
            && namespaces::library_definitions(&name).is_none()
            && (state != LoadState::Loaded || reload != Reload::None)
        {
            self.load_namespace(&name, reload)?;
        }
        let target = self
            .find_namespace(&name)
            .and_then(|n| n.namespace_id())
            .ok_or_else(|| failure(LoadError::MissingNamespace(name.clone())))?;
        if let Some(alias) = &opts.alias {
            let current = self.current_namespace_id();
            self.alias_namespace(current, alias.clone(), target)
                .map_err(|e| NativeError::Host(e.to_string()))?;
        }
        self.apply_refers(target, &opts)
    }
    fn load_namespace(&mut self, name: &Name, reload: Reload) -> NativeResult {
        self.invalidate_analysis();
        let provider = self.loading.borrow().provider.clone();
        let request = LoadRequest {
            namespace: name.clone(),
            requesting_namespace: self.current_ns_name(),
            requesting_file: self
                .loading
                .borrow()
                .source
                .as_ref()
                .map(|s| s.name().to_owned()),
            reload,
        };
        self.loading.borrow_mut().records.insert(
            name.clone(),
            Record {
                state: LoadState::Loading,
                declared: false,
            },
        );
        let previous_ns = self.current_namespace();
        let previous_reload = {
            let mut loading = self.loading.borrow_mut();
            loading.stack.push(name.clone());
            std::mem::replace(&mut loading.reload_all, reload == Reload::All)
        };
        let result = (|| {
            self.poll_execution(PollPoint::Host)
                .map_err(InvokeError::Dispatch)?;
            let input = provider
                .as_ref()
                .map_or(Ok(None), |p| p.load(&request))
                .map_err(failure)?;
            self.accounting.check()?;
            let Some(input) = input else {
                // Pinned SCI keeps a preloaded namespace when reload has no source.
                if reload != Reload::None && self.find_namespace(name).is_some() {
                    return Ok(self.nil());
                }
                return Err(failure(LoadError::MissingNamespace(name.clone())));
            };
            let source = self.source(input.file, input.source);
            self.eval_source(&source)
                .map_err(|e| NativeError::from(InvokeError::Execution(Box::new(e))))?;
            if self.find_namespace(name).is_none() {
                return Err(failure(LoadError::MissingNamespace(name.clone())));
            }
            Ok(self.nil())
        })();
        if !Rc::ptr_eq(&previous_ns.owner, &self.owner) {
            return Err(NativeError::WrongOwner);
        }
        let caller = self
            .find_namespace(&request.requesting_namespace)
            .unwrap_or(previous_ns);
        self.set_current_namespace(caller.namespace_id().unwrap())
            .expect("retained caller namespace");
        let mut loading = self.loading.borrow_mut();
        loading.stack.pop();
        loading.reload_all = previous_reload;
        let record = loading.records.get_mut(name).unwrap();
        record.state = if result.is_ok() {
            LoadState::Loaded
        } else {
            LoadState::Failed
        };
        if result.is_err() && self.registry().borrow_mut().entries.remove(name).is_some() {
            loading.discarded.insert(name.clone());
        }
        result
    }
}
fn require(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let mut reload = Reload::None;
    let mut specs = Vec::new();
    for arg in args {
        c.accounting.work(1)?;
        if let Some(flag) = keyword(arg) {
            match flag.as_str() {
                "reload-all" => reload = Reload::All,
                "reload" if reload != Reload::All => reload = Reload::Namespace,
                "reload" => (),
                _ => return Err(invalid("unsupported require flag")),
            }
        } else {
            libspec(arg, None, &mut specs)?;
        }
    }
    if specs.is_empty() {
        return Err(invalid("nothing specified to load"));
    }
    for (name, mut opts) in specs {
        if reload != Reload::None {
            opts.reload = reload;
        }
        c.require_one(name, opts)?;
    }
    Ok(c.nil())
}
fn refer(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let name = symbol(&args[0])?;
    let mut opts = options(&args[1..], true)?;
    if name == Name::new(None, "clojure.core") {
        return refer_core(c, &args[1..]);
    }
    let ns = c
        .find_namespace(&name)
        .and_then(|n| n.namespace_id())
        .ok_or_else(|| failure(LoadError::MissingNamespace(name)))?;
    if opts.refer.is_none() {
        opts.all = true;
    }
    c.apply_refers(ns, &opts)
}
fn refer_core(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let opts = options(args, true)?;
    let current = c.current_namespace_id();
    let core = c.registry().borrow().core;
    // As in SCI, :only adds explicit refers while the default core fallback remains.
    c.apply_refers(core, &opts)?;
    for (from, to) in &opts.rename {
        let var = c
            .core_var(from)
            .ok_or_else(|| failure(LoadError::MissingVar(from.clone())))?;
        c.refer_var(current, to.clone(), var)
            .map_err(|e| NativeError::Host(e.to_string()))?;
    }
    let mut store = c.owner.store.borrow_mut();
    let ns = store.namespaces.get_mut(current.0);
    ns.core_excludes.extend(opts.exclude);
    ns.core_excludes.extend(opts.rename.into_keys());
    Ok(c.nil())
}
fn quoted(c: &Context, value: RootedValue) -> NativeResult {
    c.list(&[c.symbol(Name::new(None, "quote")), value])
        .map_err(Into::into)
}
fn call(c: &Context, name: &str, args: &[RootedValue]) -> NativeResult {
    let mut forms = vec![c.symbol(Name::new(Some("clojure.core"), name))];
    forms.extend_from_slice(args);
    c.list(&forms).map_err(Into::into)
}
fn ns_macro(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    symbol(&args[2])?;
    let mut forms = vec![
        c.symbol(Name::new(None, "do")),
        call(c, "in-ns", &[quoted(c, args[2].clone())?])?,
    ];
    let mut clauses = &args[3..];
    if clauses
        .first()
        .is_some_and(|v| matches!(v.inspect(), ValueView::String(_)))
    {
        clauses = &clauses[1..];
    }
    if clauses
        .first()
        .is_some_and(|v| matches!(v.inspect(), ValueView::Map(_)))
    {
        return Err(invalid("namespace metadata maps are not supported"));
    }
    for clause in clauses {
        c.accounting.work(1)?;
        if !matches!(clause.inspect(), ValueView::List(_)) {
            return Err(invalid("ns clauses must be lists"));
        }
        let values = clause.elements()?;
        let name = values
            .first()
            .and_then(keyword)
            .ok_or_else(|| invalid("ns clause requires a keyword"))?;
        if !matches!(name.as_str(), "require" | "refer-clojure") {
            return Err(invalid("unsupported ns clause"));
        }
        let quoted = values[1..]
            .iter()
            .map(|v| quoted(c, v.clone()))
            .collect::<Result<Vec<_>, _>>()?;
        forms.push(call(
            c,
            if name == "refer-clojure" {
                "-refer-clojure"
            } else {
                "require"
            },
            &quoted,
        )?);
    }
    forms.push(call(c, "-finish-ns", &[quoted(c, args[2].clone())?])?);
    c.list(&forms).map_err(Into::into)
}
fn finish_ns(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let name = symbol(&args[0])?;
    let mut loading = c.loading.borrow_mut();
    let record = loading.records.entry(name).or_insert(Record {
        state: LoadState::Loaded,
        declared: true,
    });
    record.declared = true;
    if record.state != LoadState::Loading {
        record.state = LoadState::Loaded;
    }
    Ok(c.nil())
}
pub(super) fn loading_definitions() -> &'static [NativeDefinition] {
    macro_rules! definition {
        ($name:literal, $arity:expr, $callback:expr) => {
            NativeDefinition {
                metadata: NativeMetadata {
                    namespace: "clojure.core",
                    name: $name,
                    arity: $arity,
                    effects: NativeEffects::Context,
                    errors: &[NativeErrorClass::Invocation, NativeErrorClass::Arity],
                },
                callback: $callback,
            }
        };
    }
    static DEFINITIONS: &[NativeDefinition] = &[
        definition!("require", Arity::AtLeast(0), require),
        definition!("refer", Arity::AtLeast(1), refer),
        definition!("ns", Arity::AtLeast(3), ns_macro),
        definition!("refer-clojure", Arity::AtLeast(2), |c, args| {
            call(c, "-refer-clojure", &args[2..])
        }),
        definition!("-refer-clojure", Arity::AtLeast(0), refer_core),
        definition!("-finish-ns", Arity::Exact(1), finish_ns),
    ];
    DEFINITIONS
}
