//! Incremental compiler metadata for an explicit application evaluation session.
//! The macro host owns a separate compilation heap; no application value enters it.
use crate::analyzer::{AnalysisOptions, AnalysisWarning, NamespaceResolution};
use crate::compiler_data::{DatumKind, Form, Name, Source};
use crate::source_provider::SourceInput;
use crate::{AnalysisError, AnalysisErrorKind, ExprContext, MacroHost, Program};

type NamespaceContext = (Vec<(String, String)>, Vec<String>, NamespaceResolution);
type NamespaceDirective = (
    Vec<crate::source_provider::LoadRequest>,
    Option<crate::compiler::SessionNamespaceDirective>,
);

pub(crate) struct InteractiveCompiler {
    pub(crate) host: MacroHost,
    program: Option<Program>,
    namespace: String,
    aliases: Vec<(String, String)>,
    requires: Vec<String>,
    resolution: NamespaceResolution,
    pub(crate) options: AnalysisOptions,
    pub(crate) warnings: Vec<AnalysisWarning>,
    pub(crate) policy: NamePolicy,
    exposed: std::collections::HashSet<Name>,
    contexts: std::collections::HashMap<String, NamespaceContext>,
}
impl InteractiveCompiler {
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::with_host(MacroHost::new())
    }
    pub(crate) fn with_host(mut host: MacroHost) -> Self {
        host.begin_compilation();
        host.interactive = true;
        Self {
            host,
            program: None,
            namespace: "cljs.user".into(),
            aliases: vec![],
            requires: vec![],
            resolution: NamespaceResolution::default(),
            options: AnalysisOptions::default(),
            warnings: vec![],
            policy: NamePolicy::default(),
            exposed: Default::default(),
            contexts: Default::default(),
        }
    }
    pub(crate) fn namespace(&self) -> &str {
        &self.namespace
    }
    pub(crate) fn set_namespace(&mut self, namespace: &str) {
        if self.namespace == namespace {
            return;
        }
        self.contexts.insert(
            self.namespace.clone(),
            (
                self.aliases.clone(),
                self.requires.clone(),
                self.resolution.clone(),
            ),
        );
        self.namespace = namespace.into();
        let (aliases, requires, resolution) =
            self.contexts.get(namespace).cloned().unwrap_or_default();
        self.aliases = aliases;
        self.requires = requires;
        self.resolution = resolution;
    }
    pub(crate) fn set_alias(&mut self, alias: &str, namespace: &str) {
        self.aliases.retain(|(a, _)| a != alias);
        self.resolution.js_refers.retain(|(local, target)| {
            local != alias || !matches!(target, crate::analyzer::JsRefer::Package(..))
        });
        self.aliases.push((alias.into(), namespace.into()));
    }
    pub(crate) fn set_imports(&mut self, imports: Vec<crate::MacroImport>) -> Result<(), String> {
        for import in &imports {
            if let Some(alias) = &import.alias {
                self.set_alias(alias, &import.namespace);
            }
            for name in &import.refers {
                self.resolution
                    .refers
                    .push((name.clone(), import.namespace.clone(), name.clone()));
            }
            self.requires.push(import.namespace.clone());
        }
        self.host.set_interactive_imports(&self.namespace, imports)
    }
    pub(crate) fn validate_imports(&self) -> Result<(), String> {
        for (_, namespace, name) in &self.resolution.refers {
            let exists = self
                .globals()
                .iter()
                .any(|g| g.namespace == *namespace && g.name == *name && g.declared)
                || self
                    .host
                    .javascript_macros
                    .contains_key(&format!("{namespace}/{name}"))
                || (matches!(namespace.as_str(), "cljs.core" | "clojure.core")
                    && (crate::CoreFnId::resolve(name).is_some()
                        || crate::compiler_names::core_macro(name)));
            if !exists {
                return Err(format!(
                    "{namespace}/{name} (:refer does not name a public Var or macro)"
                ));
            }
            if namespace != &self.namespace && self.host.source_binding_private(namespace, name) {
                return Err(format!("{namespace}/{name} is private"));
            }
        }
        Ok(())
    }
    pub(crate) fn globals(&self) -> &[crate::GlobalBinding] {
        self.program.as_ref().map_or(&[], |p| p.globals.as_slice())
    }
    /// Whether the session has defined or declared `namespace/name`.
    pub(crate) fn defines(&self, namespace: &str, name: &str) -> bool {
        self.globals()
            .iter()
            .any(|g| g.declared && g.namespace == namespace && g.name == name)
    }
    pub(crate) fn read_forms(
        &self,
        input: &SourceInput,
    ) -> Result<Vec<Form>, crate::compiler_data::ReadError> {
        let mut cursor = 0;
        let mut forms = Vec::new();
        while let Some(form) = self.next_form(input, &mut cursor)? {
            forms.push(form);
        }
        Ok(forms)
    }
    pub(crate) fn next_form(
        &self,
        input: &SourceInput,
        cursor: &mut usize,
    ) -> Result<Option<Form>, crate::compiler_data::ReadError> {
        let source = Source::new(&input.file, &input.source);
        self.next_form_in_source(input, cursor, source)
    }
    pub(crate) fn next_form_in_source(
        &self,
        input: &SourceInput,
        cursor: &mut usize,
        source: Source,
    ) -> Result<Option<Form>, crate::compiler_data::ReadError> {
        loop {
            let mut parser = crate::Parser::at_offset(&input.source, *cursor).retaining_discards();
            let root = parser.read_form().map_err(|e| {
                let mut message = format!("{} (bytes {}..{})", e, e.span.start, e.span.end);
                if let Some(open) = &e.related {
                    let (line, column) = source.line_column(open.start).unwrap();
                    message.push_str(&format!("; opened at {}:{line}:{column}", source.name()));
                }
                crate::compiler_data::ReadError {
                    message,
                    span: source.span(e.span).unwrap(),
                    related: e.related.and_then(|span| source.span(span)),
                }
            })?;
            let Some(root) = root else {
                *cursor = parser.position();
                if input.file.ends_with(".cljc") {
                    let mut discards = parser.discarded().to_vec();
                    let mut syntax = crate::SyntaxTree {
                        nodes: parser.into_nodes(),
                        roots: vec![],
                    };
                    crate::compiler_reader::select_cljs(&input.source, &mut syntax, &mut discards)
                        .map_err(|e| crate::compiler_data::ReadError {
                            message: e.to_string(),
                            span: source.span(e.span).unwrap(),
                            related: None,
                        })?;
                }
                return Ok(None);
            };
            let (syntax, discards, next_cursor) = if input.file.ends_with(".cljc") {
                let mut roots = vec![root];
                loop {
                    let mut discards = parser.discarded().to_vec();
                    let mut syntax = crate::SyntaxTree {
                        nodes: parser.nodes().to_vec(),
                        roots: roots.clone(),
                    };
                    if let Err(e) = crate::compiler_reader::select_cljs(
                        &input.source,
                        &mut syntax,
                        &mut discards,
                    ) {
                        // An inactive top-level conditional under a prefix may
                        // require the next syntactic root. Do not cross a closed
                        // collection boundary or eagerly read later source forms.
                        if matches!(
                            e.kind,
                            crate::AnalysisErrorKind::InvalidForm(
                                "reader prefix requires one selected form"
                            )
                        ) && e.span.end == parser.nodes()[roots.last().unwrap().0].span.end
                            && let Some(next) =
                                parser
                                    .read_form()
                                    .map_err(|e| crate::compiler_data::ReadError {
                                        message: e.to_string(),
                                        span: source.span(e.span).unwrap(),
                                        related: e.related.and_then(|span| source.span(span)),
                                    })?
                        {
                            roots.push(next);
                            continue;
                        }
                        return Err(crate::compiler_data::ReadError {
                            message: e.to_string(),
                            span: source.span(e.span).unwrap(),
                            related: None,
                        });
                    }
                    break (syntax, discards, parser.position());
                }
            } else {
                let position = parser.position();
                let discards = parser.discarded().to_vec();
                (
                    crate::SyntaxTree {
                        nodes: parser.into_nodes(),
                        roots: vec![root],
                    },
                    discards,
                    position,
                )
            };
            *cursor = next_cursor;
            let Some(root) = syntax.roots.first().copied() else {
                continue;
            };
            let mut view = crate::compiler_reader::NamespaceView::new(&self.namespace, &source);
            for (alias, target) in &self.aliases {
                view.aliases
                    .insert(Name::new(None, alias), Name::new(None, target));
            }
            for (local, ns, name) in &self.resolution.refers {
                view.refers
                    .insert(Name::new(None, local), Name::new(Some(ns), name));
            }
            for name in &self.resolution.excludes {
                view.excludes.insert(Name::new(None, name));
            }
            for (index, global) in self
                .globals()
                .iter()
                .enumerate()
                .filter(|(_, g)| g.declared)
            {
                let qualified = Name::new(Some(&global.namespace), &global.name);
                let binding = crate::compiler_reader::ResolvedBinding {
                    id: crate::compiler_reader::BindingId(index),
                    kind: crate::compiler_reader::BindingKind::Value,
                    name: qualified.clone(),
                };
                view.bindings.insert(qualified.clone(), binding.clone());
                if global.namespace == self.namespace {
                    view.bindings
                        .insert(Name::new(None, &global.name), binding.clone());
                }
                for (local, target) in &view.refers {
                    if target == &qualified {
                        view.bindings.insert(local.clone(), binding.clone());
                    }
                }
                view.declaration_frontier = index + 1;
            }
            self.host.register_macro_view(&mut view);
            return self
                .host
                .lower_form_in_view(&source, &syntax, root, &view, &discards)
                .map(Some);
        }
    }
    pub(crate) fn analyze_form(&mut self, form: &Form) -> Result<Program, AnalysisError> {
        self.host.name_policy = Some(self.policy.clone());
        let mut resolution = self.resolution.clone();
        resolution.source_name = Some(form.span().source().name().into());
        // Previously entered namespaces remain addressable in an explicit session.
        let mut requires = self.requires.clone();
        for ns in self
            .contexts
            .keys()
            .chain(self.globals().iter().map(|g| &g.namespace))
        {
            if ns != &self.namespace && !requires.contains(ns) {
                requires.push(ns.clone());
            }
        }
        let program = crate::analyzer::analyze_interactive_form(
            form,
            &mut self.host,
            self.program.as_ref(),
            crate::analyzer::NamespaceAnalysisView {
                namespace: &self.namespace,
                aliases: &self.aliases,
                requires: &requires,
                resolution,
            },
            self.options,
            &mut self.warnings,
        )?;
        self.check_policy(&program, self.program.as_ref().map_or(0, |p| p.exprs.len()))?;
        self.program = Some(crate::program_fragment::interactive_resolution_snapshot(
            &program,
        ));
        Ok(program)
    }
    pub(crate) fn declare_binding(
        &mut self,
        namespace: &str,
        name: &str,
    ) -> Result<(), AnalysisError> {
        let old = self.namespace.clone();
        self.namespace = namespace.into();
        let source = Source::new("<host-binding>", name);
        let c = crate::compiler_data::DataBuilder;
        let data = c
            .list(&[
                c.symbol(Name::new(None, "declare")),
                c.symbol(Name::new(None, name)),
            ])
            .unwrap();
        let form = Form::from_data(
            data,
            source.span(0..name.len()).unwrap(),
            crate::compiler_data::Origin::SuppliedData,
        );
        let result = self.analyze_form(&form).map(|_| ());
        self.namespace = old;
        if result.is_ok() {
            self.exposed.insert(Name::new(Some(namespace), name));
            if namespace != self.namespace {
                self.set_imports(vec![crate::MacroImport {
                    namespace: namespace.into(),
                    ..Default::default()
                }])
                .map_err(|message| AnalysisError {
                    kind: AnalysisErrorKind::MacroExpansion(message),
                    span: 0..0,
                })?;
            }
        }
        result
    }
    pub(crate) fn macroexpand(&mut self, form: &Form, once: bool) -> Result<Form, String> {
        let mut form = form.clone();
        for _ in 0..self.host.limits.expansion_depth {
            if !matches!(
                form.data().kind(),
                DatumKind::Collection {
                    kind: crate::CollectionKind::List,
                    ..
                }
            ) {
                return Ok(form);
            }
            let children = form.children();
            let Some(head) = children.first() else {
                return Ok(form);
            };
            let data = head.data();
            let DatumKind::Symbol(name) = data.kind() else {
                return Ok(form);
            };
            let target = if let Some(ns) = name.namespace() {
                format!(
                    "{}/{}",
                    String::from_utf16_lossy(ns),
                    String::from_utf16_lossy(name.name())
                )
            } else {
                String::from_utf16_lossy(name.name())
            };
            self.host.name_policy = Some(self.policy.clone());
            let shadow = self.host.has_source_value(&self.namespace, &target)
                || self.resolution.excludes.contains(&target);
            self.host
                .check_macro_policy(&self.namespace, &target, shadow)?;
            let env = self.host.compiler_environment(
                &self.namespace,
                ExprContext::Value,
                &[],
                &self.aliases,
                &self.requires,
                (1, 1),
            )?;
            let env = self.host.reader_environment(
                env,
                &self.namespace,
                &self.resolution.refers,
                &self.resolution.excludes,
                self.globals(),
            )?;
            let next = if self.host.has_javascript_macro(&self.namespace, &target) {
                Some(self.host.expand_source_javascript(
                    &self.namespace,
                    &target,
                    &form,
                    &env,
                    None,
                )?)
            } else {
                match self.host.macro_name(&self.namespace, &target, shadow) {
                    Some((ns, name)) if ns == "cljs.core" || ns == "clojure.core" => {
                        if let Some(expanded) = self
                            .host
                            .expand_compiler_seed(&form, &Name::new(Some("cljs.core"), &name))?
                        {
                            Some(expanded)
                        } else {
                            Some(self.host.expand_core_javascript(&name, &form, &env, None)?)
                        }
                    }
                    _ => None,
                }
            };
            let Some(next) = next else { return Ok(form) };
            form = next;
            if once {
                return Ok(form);
            }
        }
        Err("compiler macro expansion depth limit exceeded".into())
    }
}

/// Compiler binding policy. It bounds named capabilities, not arbitrary JavaScript.
#[derive(Clone, Debug, Default)]
pub struct NamePolicy {
    pub allow: Option<std::collections::HashSet<Name>>,
    pub deny: std::collections::HashSet<Name>,
    pub unrestricted: bool,
}
impl NamePolicy {
    fn canonical(name: &Name) -> Name {
        let ns = name.namespace().map(String::from_utf16_lossy);
        let ns = match ns.as_deref() {
            None | Some("clojure.core") => "cljs.core",
            Some(ns) => ns,
        };
        Name::new(Some(ns), &String::from_utf16_lossy(name.name()))
    }
    pub(crate) fn permits(&self, name: &Name, exposed: bool) -> bool {
        let name = Self::canonical(name);
        self.unrestricted
            || (!self.deny.iter().any(|n| Self::canonical(n) == name)
                && (!exposed
                    || self
                        .allow
                        .as_ref()
                        .is_none_or(|allow| allow.iter().any(|n| Self::canonical(n) == name))))
    }
    fn permits_core(&self, core: crate::CoreFnId) -> bool {
        let matches = |name: &Name| {
            let name = Self::canonical(name);
            name.namespace() == Some(Name::new(None, "cljs.core").name())
                && crate::CoreFnId::resolve(&String::from_utf16_lossy(name.name())) == Some(core)
        };
        self.unrestricted
            || (!self.deny.iter().any(matches)
                && self
                    .allow
                    .as_ref()
                    .is_none_or(|allow| allow.iter().any(matches)))
    }
}
impl InteractiveCompiler {
    fn check_policy(&self, program: &Program, start: usize) -> Result<(), AnalysisError> {
        for expression in &program.exprs[start..] {
            let denied = match &expression.kind {
                crate::ExprKind::CoreFn(core) if !self.policy.permits_core(*core) => {
                    Some(format!("cljs.core/{}", core.runtime_name()))
                }
                crate::ExprKind::Global(id) => {
                    let binding = &program.globals[id.0];
                    let name = Name::new(Some(&binding.namespace), &binding.name);
                    (!self.policy.permits(&name, self.exposed.contains(&name)))
                        .then(|| format!("{}/{}", binding.namespace, binding.name))
                }
                crate::ExprKind::JsGlobal(id) => {
                    let global = &program.js_globals[id.0];
                    // Every package lookup reads one host capability, the
                    // registry; the host decides which packages it contains.
                    let name = if global.package().is_some() {
                        crate::analyzer::PACKAGE_REGISTRY.to_owned()
                    } else {
                        global.path.join(".")
                    };
                    (!self.policy.permits(&Name::new(Some("js"), &name), true))
                        .then(|| format!("js/{name}"))
                }
                _ => None,
            };
            if let Some(name) = denied {
                return Err(AnalysisError {
                    kind: AnalysisErrorKind::Unsupported(format!("name policy denies {name}")),
                    span: expression.span.clone(),
                });
            }
        }
        Ok(())
    }
    pub(crate) fn finish_directive(
        &mut self,
        staged: Option<crate::compiler::SessionNamespaceDirective>,
    ) -> Result<(), String> {
        let Some(staged) = staged else {
            return self.validate_imports();
        };
        // Check against freshly loaded declarations before publishing any aliases.
        let previous = std::mem::replace(&mut self.resolution, staged.resolution);
        if let Err(error) = self.validate_imports() {
            self.resolution = previous;
            return Err(error);
        }
        if let Err(error) = self
            .host
            .set_interactive_imports(&self.namespace, staged.imports)
        {
            self.resolution = previous;
            return Err(error);
        }
        self.aliases = staged.aliases;
        self.requires = staged.requires;
        self.host
            .set_renames(&self.namespace, self.resolution.refers.clone());
        self.host
            .add_namespace_imports(&self.namespace, staged.namespace_imports);
        Ok(())
    }
    /// Apply a namespace directive and return explicit source-provider requests.
    /// `None` is an executable form; `Some` has the value nil after its loads finish.
    pub(crate) fn directive(
        &mut self,
        form: &Form,
    ) -> Result<Option<NamespaceDirective>, crate::EvalDiagnostic> {
        let children = form.children();
        if matches!(
            form.data().kind(),
            DatumKind::Collection {
                kind: crate::CollectionKind::List,
                ..
            }
        ) && matches!(children.first().map(|f| f.data().kind().clone()),
            Some(DatumKind::Symbol(name)) if name == Name::new(None, "ns*")
                || name == Name::new(Some("cljs.core"), "ns*")
                || name == Name::new(Some("clojure.core"), "ns*"))
        {
            let staged = crate::compiler::session_nsstar(
                form,
                &self.namespace,
                &self.aliases,
                &self.requires,
                &self.resolution,
            )
            .map_err(|error| crate::EvalDiagnostic {
                message: error.message,
                location: error
                    .span
                    .and_then(|bytes| form.span().source().span(bytes)),
                data: None,
            })?;
            let requests = staged
                .imports
                .iter()
                .map(|import| crate::source_provider::LoadRequest {
                    namespace: Name::new(None, &import.namespace),
                    requesting_namespace: Name::new(None, &self.namespace),
                    requesting_file: Some(form.span().source().name().into()),
                    reload: crate::source_provider::Reload::None,
                })
                .collect();
            return Ok(Some((requests, Some(staged))));
        }
        self.directive_inner(form)
            .map_err(|message| crate::EvalDiagnostic {
                message,
                location: Some(form.span()),
                data: None,
            })
    }
    fn directive_inner(&mut self, form: &Form) -> Result<Option<NamespaceDirective>, String> {
        use crate::source_provider::{LoadRequest, Reload};
        fn text(form: &Form) -> Option<String> {
            match form.data().kind() {
                DatumKind::Symbol(n) | DatumKind::Keyword(n) => Some(match n.namespace() {
                    Some(ns) => format!(
                        "{}/{}",
                        String::from_utf16_lossy(ns),
                        String::from_utf16_lossy(n.name())
                    ),
                    None => String::from_utf16_lossy(n.name()),
                }),
                _ => None,
            }
        }
        fn unquote(form: &Form) -> Form {
            let children = form.children();
            if children.len() == 2 && text(&children[0]).as_deref() == Some("quote") {
                children[1].clone()
            } else {
                form.clone()
            }
        }
        if !matches!(
            form.data().kind(),
            DatumKind::Collection {
                kind: crate::CollectionKind::List,
                ..
            }
        ) {
            return Ok(None);
        }
        let children = form.children();
        let Some(head) = children.first().and_then(text) else {
            return Ok(None);
        };
        if !matches!(
            head.as_str(),
            "ns" | "in-ns" | "require" | "cljs.core/require" | "clojure.core/require"
        ) {
            return Ok(None);
        }
        if head == "in-ns" {
            let name = children
                .get(1)
                .map(unquote)
                .as_ref()
                .and_then(text)
                .ok_or("in-ns requires a quoted namespace symbol")?;
            self.set_namespace(&name);
            return Ok(Some((vec![], None)));
        }
        let mut specs = Vec::new();
        if head == "ns" {
            let namespace = children
                .get(1)
                .and_then(text)
                .ok_or("ns requires a namespace symbol")?;
            self.set_namespace(&namespace);
            for clause in children.iter().skip(2) {
                let parts = clause.children();
                match parts.first().and_then(text).as_deref() {
                    Some("require") => specs.extend(parts.into_iter().skip(1)),
                    Some("refer-clojure") => {
                        let mut i = 1;
                        while i + 1 < parts.len() {
                            if text(&parts[i]).as_deref() == Some("exclude") {
                                self.resolution
                                    .excludes
                                    .extend(parts[i + 1].children().iter().filter_map(text));
                            } else {
                                return Err("unsupported :refer-clojure option".into());
                            }
                            i += 2;
                        }
                    }
                    Some("require-macros" | "use-macros") => {
                        return Err(
                            "macros use ordinary :require from .cljs/.cljc namespaces".into()
                        );
                    }
                    Some(other) => return Err(format!("unsupported namespace clause :{other}")),
                    None => {}
                }
            }
        } else {
            specs.extend(children.into_iter().skip(1).map(|f| unquote(&f)));
        }
        let mut imports = Vec::new();
        let mut requests = Vec::new();
        let mut reload = Reload::None;
        for spec in specs {
            if matches!(spec.data().kind(), DatumKind::Keyword(_)) {
                match text(&spec).as_deref() {
                    Some("reload") => reload = Reload::Namespace,
                    Some("reload-all") => reload = Reload::All,
                    _ => return Err("unsupported require flag".into()),
                }
                continue;
            }
            let parts = spec.children();
            let library = parts.first().unwrap_or(&spec);
            if let DatumKind::String(name) = library.data().kind() {
                self.require_package(&spec, name, &parts)?;
                continue;
            }
            let ns = if parts.is_empty() {
                text(&spec)
            } else {
                parts.first().and_then(text)
            }
            .ok_or("require expects a namespace symbol, a package string or libspec vector")?;
            let mut import = crate::MacroImport {
                namespace: ns.clone(),
                ..Default::default()
            };
            let mut i = 1;
            while i < parts.len() {
                let key = text(&parts[i]).ok_or("libspec option must be a keyword")?;
                let value = parts.get(i + 1).ok_or("libspec option requires a value")?;
                match key.as_str() {
                    "as" => import.alias = Some(text(value).ok_or(":as requires a symbol")?),
                    "refer" => {
                        import.refers = value
                            .children()
                            .iter()
                            .map(|f| text(f).ok_or(":refer requires symbols"))
                            .collect::<Result<_, _>>()?
                    }
                    "refer-macros" | "include-macros" => {
                        return Err("macros use ordinary :require and :refer".into());
                    }
                    _ => return Err(format!("unsupported libspec option :{key}")),
                }
                i += 2;
            }
            requests.push(LoadRequest {
                namespace: Name::new(None, &ns),
                requesting_namespace: Name::new(None, &self.namespace),
                requesting_file: Some(form.span().source().name().into()),
                reload,
            });
            imports.push(import);
        }
        for request in &mut requests {
            request.reload = reload;
        }
        self.set_imports(imports)?;
        Ok(Some((requests, None)))
    }
    /// Bind `["package" :as a :refer [x] :rename {x y} :default d]` to registry
    /// lookups. The host supplies packages, so nothing is loaded. As with
    /// interactive namespace aliases, a later binding of the same name wins.
    fn require_package(&mut self, spec: &Form, name: &[u16], parts: &[Form]) -> Result<(), String> {
        use crate::analyzer::JsRefer;
        let package = String::from_utf16(name)
            .ok()
            .filter(|name| {
                !name.is_empty() && !name.chars().any(|c| c.is_whitespace() || c.is_control())
            })
            .ok_or("JavaScript package name must be a nonempty string without whitespace")?;
        let simple = |form: &Form| match form.data().kind() {
            DatumKind::Symbol(name) if name.namespace().is_none() => {
                let name = String::from_utf16_lossy(name.name());
                (!name.contains('.')).then_some(name)
            }
            _ => None,
        };
        let keyword = |form: &Form| match form.data().kind() {
            DatumKind::Keyword(name) if name.namespace().is_none() => {
                Some(String::from_utf16_lossy(name.name()))
            }
            _ => None,
        };
        let mut bindings = Vec::new();
        let mut refers = Vec::new();
        let mut renames = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for pair in parts.get(1..).unwrap_or_default().chunks(2) {
            let key = keyword(&pair[0]).ok_or("libspec option must be a keyword")?;
            let value = pair.get(1).ok_or("libspec option requires a value")?;
            if !seen.insert(key.clone()) {
                return Err(format!("duplicate libspec option :{key}"));
            }
            match key.as_str() {
                "as" => bindings.push((
                    simple(value).ok_or(":as requires a simple symbol")?,
                    JsRefer::Package(package.clone(), None),
                )),
                "default" => bindings.push((
                    simple(value).ok_or(":default requires a simple symbol")?,
                    JsRefer::Package(package.clone(), Some("default".into())),
                )),
                "refer" => {
                    refers = value
                        .children()
                        .iter()
                        .map(|f| simple(f).ok_or(":refer requires simple symbols"))
                        .collect::<Result<_, _>>()?;
                }
                "rename" => {
                    renames = value
                        .children()
                        .chunks(2)
                        .map(|p| match (simple(&p[0]), p.get(1).and_then(simple)) {
                            (Some(from), Some(to)) => Ok((from, to)),
                            _ => Err(":rename requires a map of simple symbols"),
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                }
                _ => return Err(format!("unsupported libspec option :{key}")),
            }
        }
        if let Some((original, _)) = renames.iter().find(|(from, _)| !refers.contains(from)) {
            return Err(format!(":rename symbol {original} is not referred"));
        }
        for original in refers {
            let local = renames
                .iter()
                .find(|(from, _)| *from == original)
                .map_or_else(|| original.clone(), |(_, to)| to.clone());
            let member = crate::analyzer::munge_js_member(&original);
            bindings.push((local, JsRefer::Package(package.clone(), Some(member))));
        }
        if bindings.is_empty() {
            self.warnings.push(AnalysisWarning {
                kind: "package-require-no-effect",
                message: format!(
                    "requiring JavaScript package \"{package}\" without :as, :refer or :default has no effect; the host loads packages"
                ),
                span: spec.span().bytes(),
                source_index: 0,
            });
        }
        for (local, target) in bindings {
            self.aliases.retain(|(alias, _)| *alias != local);
            self.resolution.js_refers.retain(|(name, _)| *name != local);
            self.resolution.js_refers.push((local, target));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run(
        compiler: &mut InteractiveCompiler,
        engine: &crate::evaluator::Evaluator,
        source: &str,
    ) -> String {
        let input = SourceInput::new("test.cljs", source);
        let mut cursor = 0;
        let mut result = "nil".to_owned();
        while let Some(form) = compiler.next_form(&input, &mut cursor).unwrap() {
            if compiler.directive(&form).unwrap().is_some() {
                continue;
            }
            let p = compiler.analyze_form(&form).unwrap();
            let id = engine
                .evaluate_unit(&crate::emitter::emit_interactive_unit(&p))
                .unwrap();
            result = engine.print(id).unwrap();
            engine.release(id).unwrap();
        }
        result
    }
    #[test]
    fn incremental_roots_closures_and_macros_have_separate_heaps() {
        let mut compiler = InteractiveCompiler::new();
        let engine = crate::evaluator::Evaluator::new(Default::default()).unwrap();
        run(
            &mut compiler,
            &engine,
            "(def hits (atom 0)) (defn next-id [] (swap! hits inc))",
        );
        assert_eq!(run(&mut compiler, &engine, "(next-id)"), "1");
        run(&mut compiler, &engine, "(defmacro id [] (next-id))");
        assert_eq!(run(&mut compiler, &engine, "[(id) (id) @hits]"), "[1 2 1]");
        assert_eq!(
            run(
                &mut compiler,
                &engine,
                "(def n 1) (defn f [] n) (def n 3) (f)"
            ),
            "3"
        );
    }
    #[test]
    fn failed_macro_call_preserves_interactive_compiler() {
        let mut compiler = InteractiveCompiler::new();
        let engine = crate::evaluator::Evaluator::new(Default::default()).unwrap();
        run(
            &mut compiler,
            &engine,
            "(defmacro bad [] (throw (js/Error. \"broken\"))) (defmacro good [] 42)",
        );
        let form = compiler
            .read_forms(&SourceInput::new("bad.cljs", "(bad)"))
            .unwrap()
            .remove(0);
        assert!(compiler.analyze_form(&form).is_err());
        assert_eq!(run(&mut compiler, &engine, "(good)"), "42");
    }
    #[test]
    fn incremental_catalog_does_not_duplicate_literal_or_local_arenas() {
        let mut compiler = InteractiveCompiler::new();
        let engine = crate::evaluator::Evaluator::new(Default::default()).unwrap();
        for i in 0..16 {
            run(
                &mut compiler,
                &engine,
                &format!("(defn f{i} [x] (+ x {i}))"),
            );
        }
        let program = compiler.program.as_ref().unwrap();
        assert!(program.strings.len() < 2000, "{}", program.strings.len());
        assert!(program.locals.is_empty());
        assert_eq!(run(&mut compiler, &engine, "(f15 1)"), "16");
    }
    #[test]
    fn namespace_reader_and_policy_are_incremental() {
        let mut compiler = InteractiveCompiler::new();
        let engine = crate::evaluator::Evaluator::new(Default::default()).unwrap();
        assert_eq!(
            run(
                &mut compiler,
                &engine,
                "(ns other) (def n 5) (ns app (:require [other :as o])) o/n"
            ),
            "5"
        );
        compiler.policy.deny.insert(Name::new(None, "+"));
        let form = compiler
            .read_forms(&SourceInput::new("policy.cljs", "(+ 1 2)"))
            .unwrap()
            .remove(0);
        assert!(
            compiler
                .analyze_form(&form)
                .unwrap_err()
                .to_string()
                .contains("policy")
        );
    }
}
