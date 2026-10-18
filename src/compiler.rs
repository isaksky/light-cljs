//! Small end-to-end driver. Analysis and emission remain independently usable.

use std::{
    collections::HashMap,
    fmt, fs,
    ops::Range,
    path::{Path, PathBuf},
    time::Duration,
};

use crate::analyzer::{
    AnalysisOptions, AnalysisWarning, JsRefer, LAZY_NAMESPACE, NamespaceAnalysis, NamespaceInput,
    analyze_with_macros,
};
use crate::compile_stats::{self, CompileStats};
use crate::dependency_manifest::DependencyManifest;
use crate::macro_host::{MacroHost, MacroImport};
use crate::source_provider::{FileSourceProvider, LoadError, LoadRequest, Reload, SourceProvider};
use crate::{
    AnalysisError, CollectionKind, LineIndex, NodeId, NodeKind, ParseError, PrefixKind, SyntaxTree,
    analyze, emit_js, parse,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompileError {
    Parse(ParseError),
    Analysis(AnalysisError),
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => error.fmt(f),
            Self::Analysis(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for CompileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Parse(error) => error,
            Self::Analysis(error) => error,
        })
    }
}

pub fn compile(source: &str) -> Result<String, CompileError> {
    let syntax = parse(source).map_err(CompileError::Parse)?;
    let program = analyze(source, &syntax).map_err(CompileError::Analysis)?;
    Ok(emit_js(&program))
}

/// Compile using explicit macro configuration and a fresh JavaScript session.
pub fn compile_with_macros(source: &str, macros: &mut MacroHost) -> Result<String, CompileError> {
    let syntax = parse(source).map_err(CompileError::Parse)?;
    let program = analyze_with_macros(source, &syntax, macros).map_err(CompileError::Analysis)?;
    Ok(emit_js(&program))
}

/// A loader or analysis diagnostic, located in its originating file when known.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NamespaceCompileError {
    pub message: String,
    pub path: Option<PathBuf>,
    pub span: Option<Range<usize>>,
    pub line_column: Option<(usize, usize)>,
}

impl NamespaceCompileError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            path: None,
            span: None,
            line_column: None,
        }
    }

    fn at(mut self, path: &Path, source: &str, span: Range<usize>) -> Self {
        self.path = Some(path.to_owned());
        self.line_column = LineIndex::new(source).line_column(span.start);
        self.span = Some(span);
        self
    }
}

impl fmt::Display for NamespaceCompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(path) = &self.path {
            write!(f, "{}:", path.display())?;
            if let Some((line, column)) = self.line_column {
                write!(f, "{line}:{column}:")?;
            }
            f.write_str(" ")?;
        }
        f.write_str(&self.message)?;
        if let Some(span) = &self.span {
            write!(f, " (bytes {}..{})", span.start, span.end)?;
        }
        Ok(())
    }
}

impl std::error::Error for NamespaceCompileError {}

#[derive(Clone)]
struct RequireSpec {
    /// For a JavaScript package, `namespace` holds the package name.
    import: MacroImport,
    renames: Vec<(String, String)>,
    alias_only: bool,
    alias_node: Option<NodeId>,
    span: Range<usize>,
    package: bool,
    /// The local bound to a package's default export by `:default`.
    default: Option<String>,
}

struct NamespaceSource {
    name: String,
    name_span: Range<usize>,
    path: PathBuf,
    source: String,
    syntax: SyntaxTree,
    aliases: Vec<(String, String)>,
    requires: Vec<String>,
    require_spans: Vec<Range<usize>>,
    runtime_specs: Vec<RequireSpec>,
    resolution: crate::analyzer::NamespaceResolution,
    namespace_imports: Vec<(String, String)>,
    warning_mode: bool,
    /// Package libspecs without :as, :refer or :default; they bind nothing.
    inert_packages: Vec<(String, Range<usize>)>,
}

impl NamespaceSource {
    fn text(&self, node: NodeId) -> &str {
        &self.source[self.syntax.nodes[node.0].span.clone()]
    }
    fn error(&self, node: NodeId, message: impl Into<String>) -> NamespaceCompileError {
        NamespaceCompileError::new(message).at(
            &self.path,
            &self.source,
            self.syntax.nodes[node.0].span.clone(),
        )
    }
    fn symbol(&self, node: NodeId) -> Option<&str> {
        matches!(self.syntax.nodes[node.0].kind, NodeKind::Symbol).then(|| self.text(node))
    }
    /// The span of the first `light.lazy/loadable` symbol among the forms the
    /// source compiles: not in quoted data, and for `.cljc` only in the
    /// selected branches. Syntax quote counts, since a macro writes code with it.
    fn spells_loadable(&self) -> Option<Range<usize>> {
        let mut pending = self.syntax.roots.iter().rev().copied().collect::<Vec<_>>();
        while let Some(node) = pending.pop() {
            match &self.syntax.nodes[node.0].kind {
                NodeKind::Symbol if self.text(node) == "light.lazy/loadable" => {
                    return Some(self.syntax.nodes[node.0].span.clone());
                }
                NodeKind::Collection { kind, children } => {
                    let quoted = *kind == CollectionKind::List
                        && children.first().and_then(|first| self.symbol(*first)) == Some("quote");
                    if !quoted {
                        pending.extend(children.iter().rev());
                    }
                }
                NodeKind::Prefix { kind, form } if *kind != PrefixKind::Quote => {
                    pending.push(*form);
                }
                NodeKind::Metadata { metadata, target } => {
                    pending.extend([*target, *metadata]);
                }
                NodeKind::Tagged { form, .. } => pending.push(*form),
                NodeKind::NamespacedMap { map, .. } => pending.push(*map),
                _ => {}
            }
        }
        None
    }
    fn collection(&self, node: NodeId, expected: CollectionKind) -> Option<&[NodeId]> {
        match &self.syntax.nodes[node.0].kind {
            NodeKind::Collection { kind, children } if *kind == expected => Some(children),
            _ => None,
        }
    }
    fn simple_symbol(&self, node: NodeId) -> Result<String, NamespaceCompileError> {
        self.symbol(node)
            .filter(|s| !s.contains('.') && (!s.contains('/') || *s == "/") && *s != "&")
            .map(str::to_owned)
            .ok_or_else(|| self.error(node, "expected a simple symbol"))
    }
    fn names(&self, node: NodeId) -> Result<Vec<String>, NamespaceCompileError> {
        self.collection(node, CollectionKind::Vector)
            .or_else(|| self.collection(node, CollectionKind::List))
            .ok_or_else(|| self.error(node, ":refer/:exclude needs a sequence of simple symbols"))?
            .iter()
            .map(|&n| self.simple_symbol(n))
            .collect()
    }
    fn renames(&self, node: NodeId) -> Result<Vec<(String, String)>, NamespaceCompileError> {
        self.collection(node, CollectionKind::Map)
            .ok_or_else(|| self.error(node, ":rename needs a map of simple symbols"))?
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| Ok((self.simple_symbol(p[0])?, self.simple_symbol(p[1])?)))
            .collect()
    }
    /// A string library names a JavaScript package that the host supplies.
    fn package(&self, node: NodeId) -> Option<Result<String, NamespaceCompileError>> {
        let NodeKind::String(value) = &self.syntax.nodes[node.0].kind else {
            return None;
        };
        Some(
            String::from_utf16(&value.to_utf16(&self.source))
                .ok()
                .filter(|name| {
                    !name.is_empty() && !name.chars().any(|c| c.is_whitespace() || c.is_control())
                })
                .ok_or_else(|| {
                    self.error(
                        node,
                        "JavaScript package name must be a nonempty string without whitespace",
                    )
                }),
        )
    }
    fn read_spec(&self, spec: NodeId, only: bool) -> Result<RequireSpec, NamespaceCompileError> {
        let items = if self.symbol(spec).is_some()
            || matches!(self.syntax.nodes[spec.0].kind, NodeKind::String(_))
        {
            vec![spec]
        } else {
            self.collection(spec, CollectionKind::Vector)
                .or_else(|| self.collection(spec, CollectionKind::List))
                .ok_or_else(|| {
                    self.error(
                        spec,
                        "require expects a namespace symbol, a package string or [library options]",
                    )
                })?
                .to_vec()
        };
        let dependency = *items
            .first()
            .ok_or_else(|| self.error(spec, "require specification cannot be empty"))?;
        let package = self.package(dependency).transpose()?;
        if only && package.is_some() {
            return Err(self.error(
                dependency,
                ":use requires a namespace symbol; require JavaScript packages with :require",
            ));
        }
        let namespace = match &package {
            Some(name) => name.clone(),
            None => self
                .symbol(dependency)
                .filter(|n| valid_namespace(n))
                .ok_or_else(|| {
                    self.error(
                        dependency,
                        "require library must be a supported namespace symbol or a JavaScript package string",
                    )
                })?
                .to_owned(),
        };
        let mut result = RequireSpec {
            import: MacroImport {
                namespace,
                ..MacroImport::default()
            },
            renames: vec![],
            alias_only: false,
            alias_node: None,
            span: self.syntax.nodes[dependency.0].span.clone(),
            package: package.is_some(),
            default: None,
        };
        if items.len().is_multiple_of(2) {
            return Err(self.error(spec, "require options need values"));
        }
        let mut seen = std::collections::HashSet::new();
        for pair in items[1..].as_chunks::<2>().0.iter() {
            let option = self.text(pair[0]);
            if !seen.insert(option) {
                return Err(self.error(spec, "duplicate require option; only one :as alias and one :refer/:rename option are allowed"));
            }
            if only && !matches!(option, ":only" | ":rename") {
                return Err(self.error(spec, ":use expects :only and optional :rename"));
            }
            match option {
                ":only" if only => result.import.refers = self.names(pair[1])?,
                ":as" | ":as-alias" => {
                    if result.import.alias.is_some() {
                        return Err(self.error(spec, "only one :as alias is allowed"));
                    }
                    result.alias_only = option == ":as-alias";
                    result.alias_node = Some(pair[1]);
                    result.import.alias = Some(self.symbol(pair[1]).filter(|n| valid_namespace(n) || (self.warning_mode && *n == "js")).ok_or_else(|| self.error(pair[1], ":as alias must be a supported namespace symbol; js is reserved"))?.to_owned());
                }
                ":refer" => result.import.refers = self.names(pair[1])?,
                ":rename" => result.renames = self.renames(pair[1])?,
                ":default" if result.package => {
                    result.default = Some(self.simple_symbol(pair[1])?);
                }
                ":default" => {
                    return Err(
                        self.error(pair[0], ":default requires a JavaScript package string")
                    );
                }
                ":refer-macros" | ":include-macros" => {
                    return Err(self.error(pair[0], format!("{option} is retired; import macros and functions with ordinary :require and :refer from .cljs/.cljc sources")));
                }
                _ => return Err(self.error(spec, "unsupported require options")),
            }
        }
        if only && !seen.contains(":only") {
            return Err(self.error(spec, ":use requires :only"));
        }
        for (original, _) in &result.renames {
            if !result.import.refers.contains(original) {
                return Err(self.error(spec, format!(":rename symbol {original} is not referred")));
            }
        }
        if result.alias_only && (items.len() != 3 || !result.import.refers.is_empty()) {
            return Err(self.error(spec, ":as-alias cannot be combined with other options"));
        }
        Ok(result)
    }
    fn quoted_argument(
        &self,
        node: NodeId,
        operator: &str,
    ) -> Result<NodeId, NamespaceCompileError> {
        if let NodeKind::Prefix {
            kind: PrefixKind::Quote,
            form,
        } = self.syntax.nodes[node.0].kind
        {
            return Ok(form);
        }
        if let Some(parts) = self.collection(node, CollectionKind::List)
            && parts.len() == 2
            && self.symbol(parts[0]) == Some("quote")
        {
            return Ok(parts[1]);
        }
        Err(self.error(node, format!("Arguments to {operator} must be quoted")))
    }
    fn top_level_clauses(
        &mut self,
        node: NodeId,
        clauses: &mut Vec<(NodeId, String, Vec<NodeId>)>,
    ) -> Result<(), NamespaceCompileError> {
        let Some(parts) = self
            .collection(node, CollectionKind::List)
            .map(<[NodeId]>::to_vec)
        else {
            return Ok(());
        };
        let Some(operator) = parts.first().and_then(|&head| self.symbol(head)) else {
            return Ok(());
        };
        let operator = operator
            .strip_prefix("cljs.core/")
            .or_else(|| operator.strip_prefix("clojure.core/"))
            .unwrap_or(operator)
            .to_owned();
        if operator == "do" {
            for &child in &parts[1..] {
                self.top_level_clauses(child, clauses)?;
            }
            return Ok(());
        }
        if operator == "ns*" {
            if parts.len() != 2 {
                return Err(self.error(node, "ns* requires one namespace directive"));
            }
            let directive = self
                .collection(parts[1], CollectionKind::List)
                .ok_or_else(|| self.error(parts[1], "ns* requires a literal namespace directive"))?
                .to_vec();
            let kind = directive.first().map(|&head| self.text(head)).unwrap_or("");
            if !matches!(
                kind,
                ":require"
                    | ":use"
                    | ":import"
                    | ":refer-clojure"
                    | ":refer-global"
                    | ":require-global"
                    | ":require-macros"
                    | ":use-macros"
            ) {
                return Err(self.error(parts[1], "unsupported ns* namespace directive"));
            }
            let kind = kind.to_owned();
            let mut arguments = Vec::new();
            for &arg in &directive[1..] {
                if matches!(self.syntax.nodes[arg.0].kind, NodeKind::Keyword { .. })
                    && matches!(kind.as_str(), ":refer-clojure" | ":refer-global")
                {
                    arguments.push(arg);
                } else {
                    arguments.push(self.quoted_argument(arg, kind.trim_start_matches(':'))?);
                }
            }
            clauses.push((parts[1], kind, arguments));
            self.syntax.nodes[node.0].kind = NodeKind::Nil;
            return Ok(());
        }
        if !matches!(
            operator.as_str(),
            "require"
                | "require-macros"
                | "use"
                | "use-macros"
                | "import"
                | "refer-clojure"
                | "refer-global"
                | "require-global"
        ) {
            return Ok(());
        }
        let mut arguments = Vec::new();
        for &arg in &parts[1..] {
            if matches!(self.syntax.nodes[arg.0].kind, NodeKind::Keyword { .. })
                && matches!(operator.as_str(), "refer-clojure" | "refer-global")
            {
                arguments.push(arg);
            } else {
                arguments.push(self.quoted_argument(arg, &operator)?);
            }
        }
        clauses.push((node, format!(":{operator}"), arguments));
        // Namespace loading is a compile-time operation. Preserve its original
        // span and nil result, including when it appears in a top-level do.
        self.syntax.nodes[node.0].kind = NodeKind::Nil;
        Ok(())
    }
    fn add_js_refer(
        &mut self,
        node: NodeId,
        local: String,
        path: String,
    ) -> Result<(), NamespaceCompileError> {
        self.add_js_target(node, local, JsRefer::Global(path))
    }
    fn add_js_target(
        &mut self,
        node: NodeId,
        local: String,
        target: JsRefer,
    ) -> Result<(), NamespaceCompileError> {
        if let Some((_, previous)) = self
            .resolution
            .js_refers
            .iter()
            .find(|(name, _)| name == &local)
        {
            if previous == &target {
                return Ok(());
            }
            return Err(self.error(node, format!("conflicting global refer: {local}")));
        }
        self.resolution.js_refers.push((local, target));
        Ok(())
    }
    /// Bind a package's alias, referred exports and default export. The host
    /// loads the package; nothing is added to the namespace dependency graph.
    fn add_package(
        &mut self,
        node: NodeId,
        spec: RequireSpec,
    ) -> Result<(), NamespaceCompileError> {
        let package = spec.import.namespace;
        if spec.import.alias.is_none() && spec.import.refers.is_empty() && spec.default.is_none() {
            self.inert_packages.push((package, spec.span));
            return Ok(());
        }
        if let Some(alias) = spec.import.alias {
            if self.aliases.iter().any(|(existing, _)| existing == &alias) {
                return Err(self.error(
                    spec.alias_node.unwrap_or(node),
                    format!("duplicate or conflicting namespace alias: {alias}"),
                ));
            }
            self.add_js_target(node, alias, JsRefer::Package(package.clone(), None))?;
        }
        if let Some(local) = spec.default {
            self.add_js_target(
                node,
                local,
                JsRefer::Package(package.clone(), Some("default".into())),
            )?;
        }
        for original in spec.import.refers {
            let local = spec
                .renames
                .iter()
                .find(|(from, _)| from == &original)
                .map_or_else(|| original.clone(), |(_, to)| to.clone());
            let member = crate::analyzer::munge_js_member(&original);
            self.add_js_target(node, local, JsRefer::Package(package.clone(), Some(member)))?;
        }
        Ok(())
    }
    fn read_header(&mut self) -> Result<(), NamespaceCompileError> {
        let Some(&root) = self.syntax.roots.first() else {
            return Err(NamespaceCompileError::new(
                "namespace source must begin with (ns NAME ...)",
            )
            .at(&self.path, &self.source, 0..0));
        };
        let forms = self
            .collection(root, CollectionKind::List)
            .filter(|xs| xs.first().and_then(|&n| self.symbol(n)) == Some("ns"))
            .ok_or_else(|| self.error(root, "namespace source must begin with (ns NAME ...)"))?
            .to_vec();
        let mut declared = *forms
            .get(1)
            .ok_or_else(|| self.error(root, "ns requires a namespace name"))?;
        while let NodeKind::Metadata { target, .. } = self.syntax.nodes[declared.0].kind {
            declared = target;
        }
        let name = self
            .symbol(declared)
            .filter(|n| valid_namespace(n))
            .ok_or_else(|| {
                self.error(
                    declared,
                    "namespace must use dot-separated ASCII names; js is reserved",
                )
            })?;
        if name != self.name {
            return Err(self.error(
                declared,
                format!(
                    "declared namespace {name} does not match requested namespace {}",
                    self.name
                ),
            ));
        }
        self.name_span = self.syntax.nodes[declared.0].span.clone();
        let mut clauses = self.header_clauses(&forms)?;
        for &node in &self.syntax.roots[1..].to_vec() {
            self.top_level_clauses(node, &mut clauses)?;
        }
        self.read_clauses(root, clauses)
    }
    /// The clauses of the `ns` form `forms` after its name, docstring and
    /// attribute map, each kind at most once.
    #[allow(clippy::type_complexity)]
    fn header_clauses(
        &self,
        forms: &[NodeId],
    ) -> Result<Vec<(NodeId, String, Vec<NodeId>)>, NamespaceCompileError> {
        let mut offset = 2;
        if forms
            .get(offset)
            .is_some_and(|id| matches!(self.syntax.nodes[id.0].kind, NodeKind::String(_)))
        {
            offset += 1;
        }
        if forms
            .get(offset)
            .is_some_and(|&id| self.collection(id, CollectionKind::Map).is_some())
        {
            offset += 1;
        }
        let mut seen = std::collections::HashSet::new();
        let mut clauses = Vec::new();
        for &clause in forms.iter().skip(offset) {
            let parts = self
                .collection(clause, CollectionKind::List)
                .ok_or_else(|| self.error(clause, "unsupported namespace clause"))?
                .to_vec();
            let kind = parts
                .first()
                .map(|&id| self.text(id))
                .unwrap_or("")
                .to_owned();
            if !seen.insert(kind.clone()) {
                return Err(self.error(
                    clause,
                    format!("only one {kind} clause is allowed per namespace"),
                ));
            }
            clauses.push((clause, kind, parts.get(1..).unwrap_or_default().to_vec()));
        }
        Ok(clauses)
    }
    fn read_clauses(
        &mut self,
        root: NodeId,
        clauses: Vec<(NodeId, String, Vec<NodeId>)>,
    ) -> Result<(), NamespaceCompileError> {
        for (clause, kind, arguments) in clauses {
            // Existing readers use a head slot; its spelling is irrelevant here.
            let mut parts = vec![clause];
            parts.extend(arguments);
            match kind.as_str() {
                ":require-global" => {
                    for spec in &parts[1..] {
                        let parsed = self.read_spec(*spec, false)?;
                        if parsed.package {
                            return Err(self.error(
                                *spec,
                                "require-global expects a global name symbol; require JavaScript packages with :require",
                            ));
                        }
                        if parsed.alias_only {
                            return Err(self
                                .error(*spec, "require-global supports :as, :refer, and :rename"));
                        }
                        let namespace = parsed.import.namespace;
                        self.add_js_refer(*spec, namespace.clone(), namespace.clone())?;
                        if let Some(alias) = parsed.import.alias {
                            self.add_js_refer(*spec, alias, namespace.clone())?;
                        }
                        for original in parsed.import.refers {
                            let local = parsed
                                .renames
                                .iter()
                                .find(|(from, _)| from == &original)
                                .map_or_else(|| original.clone(), |(_, to)| to.clone());
                            self.add_js_refer(*spec, local, format!("{namespace}.{original}"))?;
                        }
                    }
                }
                ":import" => {
                    for &spec in &parts[1..] {
                        let imported = if let Some(name) = self.symbol(spec) {
                            vec![name.to_owned()]
                        } else {
                            let names = self
                                .collection(spec, CollectionKind::Vector)
                                .or_else(|| self.collection(spec, CollectionKind::List))
                                .filter(|names| !names.is_empty())
                                .ok_or_else(|| {
                                    self.error(
                                        spec,
                                        "import expects a symbol or [namespace constructors]",
                                    )
                                })?;
                            let namespace = self
                                .symbol(names[0])
                                .filter(|name| valid_namespace(name))
                                .ok_or_else(|| {
                                    self.error(names[0], "import namespace must be a symbol")
                                })?;
                            names[1..]
                                .iter()
                                .map(|&name| {
                                    Ok(format!("{namespace}.{}", self.simple_symbol(name)?))
                                })
                                .collect::<Result<Vec<_>, NamespaceCompileError>>()?
                        };
                        for qualified in imported {
                            if !valid_namespace(&qualified) {
                                return Err(self.error(spec, "import name must be a dotted symbol"));
                            }
                            // Upstream ignores a bare namespace import such as goog.
                            let Some((_, local)) = qualified.rsplit_once('.') else {
                                continue;
                            };
                            self.add_js_refer(spec, local.to_owned(), qualified.clone())?;
                            self.add_js_refer(spec, qualified.clone(), qualified.clone())?;
                            self.namespace_imports.push((local.to_owned(), qualified));
                        }
                    }
                }
                ":refer-global" => {
                    if parts.len().is_multiple_of(2) {
                        return Err(self.error(clause, ":refer-global options need values"));
                    }
                    let mut names = None;
                    let mut renames = Vec::new();
                    let mut options = std::collections::HashSet::new();
                    for pair in parts[1..].as_chunks::<2>().0 {
                        let option = self.text(pair[0]);
                        if !options.insert(option.to_owned()) {
                            return Err(self.error(pair[0], "duplicate :refer-global option"));
                        }
                        match option {
                            ":only" => names = Some(self.names(pair[1])?),
                            ":rename" => renames = self.renames(pair[1])?,
                            _ => {
                                return Err(
                                    self.error(pair[0], ":refer-global supports :only and :rename")
                                );
                            }
                        }
                    }
                    let names =
                        names.ok_or_else(|| self.error(clause, ":refer-global requires :only"))?;
                    if renames
                        .iter()
                        .any(|(original, _)| !names.contains(original))
                    {
                        return Err(
                            self.error(clause, ":refer-global rename requires a referred name")
                        );
                    }
                    for original in names {
                        let local = renames
                            .iter()
                            .find(|(name, _)| name == &original)
                            .map_or_else(|| original.clone(), |(_, local)| local.clone());
                        if self
                            .resolution
                            .js_refers
                            .iter()
                            .any(|(name, _)| name == &local)
                        {
                            return Err(self.error(clause, "duplicate global refer"));
                        }
                        self.resolution
                            .js_refers
                            .push((local, JsRefer::Global(original)));
                    }
                }
                ":refer-clojure" => {
                    if parts.len().is_multiple_of(2) {
                        return Err(self.error(clause, ":refer-clojure options need values"));
                    }
                    let mut options = std::collections::HashSet::new();
                    for pair in parts[1..].as_chunks::<2>().0.iter() {
                        let option = self.text(pair[0]);
                        if !options.insert(option.to_owned()) {
                            return Err(self.error(clause, "duplicate :refer-clojure option"));
                        }
                        match option {
                            ":exclude" => self.resolution.excludes.extend(self.names(pair[1])?),
                            ":rename" => {
                                for (original, local) in self.renames(pair[1])? {
                                    self.resolution.excludes.push(original.clone());
                                    self.resolution.refers.push((
                                        local,
                                        "cljs.core".into(),
                                        original,
                                    ));
                                }
                            }
                            _ => {
                                return Err(self.error(
                                    clause,
                                    "unsupported :refer-clojure option; use :exclude or :rename",
                                ));
                            }
                        }
                    }
                }
                ":require-macros" | ":use-macros" => {
                    return Err(self.error(clause, format!("{kind} is retired; import macros and functions with ordinary :require from .cljs/.cljc sources")));
                }
                ":require" | ":use" => {
                    for spec in parts.into_iter().skip(1) {
                        let parsed = self.read_spec(spec, kind == ":use")?;
                        if parsed.package && !parsed.alias_only {
                            self.add_package(spec, parsed)?;
                            continue;
                        }
                        if let Some(alias) = &parsed.import.alias {
                            if self.resolution.js_refers.iter().any(|(local, target)| {
                                local == alias && matches!(target, JsRefer::Package(..))
                            }) {
                                return Err(self.error(
                                    parsed.alias_node.unwrap_or(spec),
                                    format!("duplicate or conflicting namespace alias: {alias}"),
                                ));
                            }
                            if let Some((_, previous)) =
                                self.aliases.iter().find(|(existing, _)| existing == alias)
                            {
                                if previous != &parsed.import.namespace {
                                    return Err(self.error(
                                        parsed.alias_node.unwrap_or(spec),
                                        format!(
                                            "duplicate or conflicting namespace alias: {alias}"
                                        ),
                                    ));
                                }
                            } else {
                                self.aliases
                                    .push((alias.clone(), parsed.import.namespace.clone()));
                            }
                        }
                        if !parsed.alias_only {
                            if self.runtime_specs.iter().any(|existing| {
                                existing.import == parsed.import
                                    && existing.renames == parsed.renames
                            }) {
                                continue;
                            }
                            self.requires.push(parsed.import.namespace.clone());
                            self.require_spans.push(parsed.span.clone());
                            self.runtime_specs.push(parsed);
                        }
                    }
                }
                _ => return Err(self.error(clause, "unsupported namespace clause")),
            }
        }
        for (alias, target) in &self.aliases {
            if alias != target
                && (alias == &self.name
                    || self.requires.contains(alias)
                    || matches!(alias.as_str(), "cljs.core" | "clojure.core"))
            {
                let node = self
                    .syntax
                    .nodes
                    .iter()
                    .enumerate()
                    .rev()
                    .find(|(_, n)| {
                        n.span.end <= self.syntax.nodes[root.0].span.end
                            && matches!(n.kind, NodeKind::Symbol)
                            && &self.source[n.span.clone()] == alias
                    })
                    .map(|(i, _)| NodeId(i))
                    .unwrap_or(root);
                return Err(self.error(
                    node,
                    format!("namespace alias {alias} conflicts with namespace qualifier {alias}"),
                ));
            }
        }
        Ok(())
    }
}

/// A validated source directive, staged until the session has loaded its dependencies.
pub(crate) struct SessionNamespaceDirective {
    pub aliases: Vec<(String, String)>,
    pub requires: Vec<String>,
    pub resolution: crate::analyzer::NamespaceResolution,
    pub imports: Vec<MacroImport>,
    pub namespace_imports: Vec<(String, String)>,
    /// Whether the directive replaces the namespace's context, as an `ns`
    /// form does, instead of adding to it, as `ns*` does.
    pub replaces: bool,
    /// The warnings about the directive's header and libspecs, as
    /// `--compile` reports them before a namespace's body.
    pub warnings: Vec<AnalysisWarning>,
    /// The namespace's symbol with its metadata (name metadata, docstring
    /// and attribute map) and its docstring, which macros read in `&env`,
    /// for an `ns` form that has any; the session sets it.
    pub symbol: Option<(
        crate::compiler_data::Datum,
        Option<crate::compiler_data::Datum>,
    )>,
}

/// The directive `form` as a namespace source's syntax tree and root, and
/// whether its positions are the source's. A form read from source text is
/// that text read again, the reader conditionals of a `.cljc` source
/// selected and metadata on the form skipped, once it reads as the same data
/// in `namespace`. Any other form, as one built from data, is its data
/// written as text: positions in it are not the source's.
fn session_syntax(
    form: &crate::compiler_data::Form,
    namespace: &str,
) -> Result<(NamespaceSource, NodeId, bool), NamespaceCompileError> {
    let span = form.span();
    let name = span.source().name();
    let read = (form.node().origin == crate::compiler_data::Origin::Source)
        .then(|| {
            let source = span.source().text();
            let mut parser =
                crate::Parser::at_offset(source, span.bytes().start).retaining_discards();
            let root = parser.read_form().ok()??;
            let mut discards = parser.discarded().to_vec();
            let mut syntax = SyntaxTree {
                nodes: parser.into_nodes(),
                roots: vec![root],
            };
            if syntax.nodes[root.0].span != span.bytes() {
                return None;
            }
            if name.ends_with(".cljc") {
                crate::compiler_reader::select_cljs(source, &mut syntax, &mut discards).ok()?;
            }
            let view = crate::compiler_reader::NamespaceView::new(namespace, span.source());
            let lowered =
                crate::compiler_reader::lower_form(span.source(), &syntax, root, &view, &discards)
                    .ok()?;
            lowered
                .data()
                .value_eq(&form.data())
                .then(|| (source.to_owned(), syntax, root))
        })
        .flatten();
    let from_source = read.is_some();
    let (text, syntax, mut root) = match read {
        Some(read) => read,
        None => {
            let text =
                String::from_utf16_lossy(&crate::compiler_data::diagnostic_text(&form.data()));
            let mut parser = crate::Parser::new(&text);
            let root = parser.read_form().ok().flatten().ok_or_else(|| {
                NamespaceCompileError::new("the directive's data cannot be read as source text")
            })?;
            let syntax = SyntaxTree {
                nodes: parser.into_nodes(),
                roots: vec![root],
            };
            (text, syntax, root)
        }
    };
    while let NodeKind::Metadata { target, .. } = syntax.nodes[root.0].kind {
        root = target;
    }
    let file = NamespaceSource {
        name: String::new(),
        name_span: 0..0,
        path: name.into(),
        source: text,
        syntax,
        aliases: vec![],
        requires: vec![],
        require_spans: vec![],
        runtime_specs: vec![],
        namespace_imports: vec![],
        resolution: crate::analyzer::NamespaceResolution {
            source_name: Some(name.into()),
            cljc: name.ends_with(".cljc"),
            ..Default::default()
        },
        warning_mode: false,
        inert_packages: vec![],
    };
    Ok((file, root, from_source))
}

/// What is reported about a directive read by [`session_syntax`], located
/// at the whole form where its positions are not the source's: an error
/// without a position, and warnings at the form's span.
fn session_located<T>(
    form: &crate::compiler_data::Form,
    from_source: bool,
    result: Result<(T, SessionNamespaceDirective), NamespaceCompileError>,
) -> Result<(T, SessionNamespaceDirective), NamespaceCompileError> {
    if from_source {
        return result;
    }
    let (value, mut staged) = result.map_err(|error| NamespaceCompileError::new(error.message))?;
    for warning in &mut staged.warnings {
        warning.span = form.span().bytes();
    }
    Ok((value, staged))
}

/// A session's `ns` form, read as `--compile` reads a namespace source's
/// header: its name (with metadata), docstring, attribute map and clauses,
/// each clause once, from a fresh context, since an `ns` form declares all
/// of its namespace's requires, as upstream's replaces them. The name may be
/// any symbol without a namespace, as `in-ns` takes, where a source file's
/// is ASCII. Returns the declared namespace and the staged directive. With
/// `warning_mode`, `[lib :as js]` is a warning, and so is a single-segment
/// name, as in `--compile --warnings`. `namespace` is the current one.
pub(crate) fn session_ns(
    form: &crate::compiler_data::Form,
    namespace: &str,
    warning_mode: bool,
) -> Result<(String, SessionNamespaceDirective), NamespaceCompileError> {
    let (mut file, root, from_source) = session_syntax(form, namespace)?;
    file.warning_mode = warning_mode;
    let mut read = || {
        let forms = file
            .collection(root, CollectionKind::List)
            .ok_or_else(|| file.error(root, "ns requires a list"))?
            .to_vec();
        let mut declared = *forms
            .get(1)
            .ok_or_else(|| file.error(root, "ns requires a namespace name"))?;
        while let NodeKind::Metadata { target, .. } = file.syntax.nodes[declared.0].kind {
            declared = target;
        }
        file.name = file
            .symbol(declared)
            .filter(|name| !name.contains('/') && *name != "js")
            .ok_or_else(|| file.error(declared, "ns requires a namespace symbol; js is reserved"))?
            .to_owned();
        file.name_span = file.syntax.nodes[declared.0].span.clone();
        let clauses = file.header_clauses(&forms)?;
        file.read_clauses(root, clauses)?;
        Ok(file.name.clone())
    };
    let result = read().and_then(|name| Ok((name, session_directive(file, root, true)?)));
    session_located(form, from_source, result)
}

/// A session's `ns*` form, in `namespace` with its current context, which the
/// directive adds to. With `warning_mode`, `[lib :as js]` is a warning.
pub(crate) fn session_nsstar(
    form: &crate::compiler_data::Form,
    namespace: &str,
    aliases: &[(String, String)],
    requires: &[String],
    resolution: &crate::analyzer::NamespaceResolution,
    warning_mode: bool,
) -> Result<SessionNamespaceDirective, NamespaceCompileError> {
    let (mut file, root, from_source) = session_syntax(form, namespace)?;
    file.name = namespace.into();
    file.aliases = aliases.to_vec();
    file.requires = requires.to_vec();
    file.resolution = resolution.clone();
    file.warning_mode = warning_mode;
    let mut clauses = vec![];
    let result = file
        .top_level_clauses(root, &mut clauses)
        .and_then(|()| file.read_clauses(root, clauses))
        .and_then(|()| Ok(((), session_directive(file, root, false)?)));
    Ok(session_located(form, from_source, result)?.1)
}

/// The staged directive of a read `file`: its imports with the refers they
/// bind, and its warnings.
fn session_directive(
    mut file: NamespaceSource,
    root: NodeId,
    replaces: bool,
) -> Result<SessionNamespaceDirective, NamespaceCompileError> {
    let mut imports = vec![];
    for spec in &file.runtime_specs {
        let mut import = spec.import.clone();
        import.refers.clear();
        for original in &spec.import.refers {
            let local = spec
                .renames
                .iter()
                .find(|(from, _)| from == original)
                .map_or(original, |(_, to)| to);
            let refer = (local.clone(), import.namespace.clone(), original.clone());
            if !file.resolution.refers.contains(&refer) {
                if file
                    .resolution
                    .refers
                    .iter()
                    .any(|(name, _, _)| name == local)
                {
                    return Err(
                        file.error(root, format!("duplicate or conflicting refer: {local}"))
                    );
                }
                file.resolution.refers.push(refer);
            }
            if local == original {
                import.refers.push(original.clone());
            }
        }
        imports.push(import);
    }
    let mut warnings = vec![];
    let options = AnalysisOptions {
        warning_mode: file.warning_mode,
        ..Default::default()
    };
    source_warnings(std::slice::from_ref(&file), 0, options, &mut warnings);
    // An `ns*` names no namespace.
    warnings.retain(|warning| replaces || warning.kind != "single-segment-namespace");
    Ok(SessionNamespaceDirective {
        aliases: file.aliases,
        requires: file.requires,
        resolution: file.resolution,
        imports,
        namespace_imports: file.namespace_imports,
        replaces,
        warnings,
        symbol: None,
    })
}
pub(crate) fn valid_namespace(name: &str) -> bool {
    name != "js"
        && name.split('.').all(|part| {
            let mut bytes = part.bytes();
            matches!(bytes.next(), Some(b'a'..=b'z' | b'A'..=b'Z' | b'_'))
                && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        })
}

/// The file of namespace `name` in `roots`: each root at the first
/// extension, then each at the next, until one has a file. With `tried`,
/// also append each path considered, in that order, and whether it was
/// present, for a dependency manifest.
pub(crate) fn locate_source_traced(
    roots: &[PathBuf],
    name: &str,
    extensions: &[&str],
    mut tried: Option<&mut Vec<crate::dependency_manifest::Candidate>>,
) -> Result<Option<PathBuf>, NamespaceCompileError> {
    let relative = name.replace('.', "/").replace('-', "_");
    for extension in extensions {
        let mut candidates = Vec::new();
        for root in roots {
            let path = root.join(format!("{relative}.{extension}"));
            let metadata = fs::metadata(&path);
            if let Some(tried) = tried.as_deref_mut() {
                tried.push(crate::dependency_manifest::Candidate {
                    path: path.clone(),
                    present: metadata.as_ref().is_ok_and(|m| m.is_file()),
                });
            }
            match metadata {
                Ok(metadata) if metadata.is_file() => {
                    let canonical = fs::canonicalize(&path).map_err(|e| {
                        NamespaceCompileError::new(format!(
                            "cannot resolve {}: {e}",
                            path.display()
                        ))
                    })?;
                    if !candidates.contains(&canonical) {
                        candidates.push(canonical);
                    }
                }
                Ok(_) => {
                    return Err(NamespaceCompileError::new(format!(
                        "namespace source is not a file: {}",
                        path.display()
                    )));
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => {
                    return Err(NamespaceCompileError::new(format!(
                        "cannot inspect {}: {e}",
                        path.display()
                    )));
                }
            }
        }
        if candidates.len() > 1 {
            return Err(NamespaceCompileError::new(format!(
                "ambiguous namespace {name}: {}",
                candidates
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
        if let Some(path) = candidates.pop() {
            return Ok(Some(path));
        }
    }
    Ok(None)
}
#[derive(Clone, Copy)]
enum LoadState {
    Unseen,
    Loading,
    Loaded,
}

struct Loader<'a> {
    /// Where namespace sources come from: explicit roots on disk for the CLI
    /// (`FileSourceProvider`), or anything else the caller supplies.
    sources: &'a dyn SourceProvider,
    entry: String,
    states: HashMap<String, LoadState>,
    stack: Vec<String>,
    ordered: Vec<NamespaceSource>,
    warning_mode: bool,
    /// Lookups and requires for a dependency manifest, when one is asked for.
    record: Option<crate::dependency_manifest::LoaderRecord>,
}

impl Loader<'_> {
    /// Ask the provider for `name`: its file name and text, or `None` when the
    /// provider has no such namespace.
    /// With a manifest record, the lookup also becomes one, whose id
    /// [`Self::last_lookup`] answers next.
    fn find(&mut self, name: &str) -> Result<Option<(PathBuf, String)>, NamespaceCompileError> {
        let requesting = self
            .stack
            .last()
            .map_or(self.entry.as_str(), String::as_str);
        let request = LoadRequest {
            namespace: crate::compiler_data::Name::new(None, name),
            requesting_namespace: crate::compiler_data::Name::new(None, requesting),
            requesting_file: None,
            reload: Reload::None,
        };
        let loaded = match &mut self.record {
            Some(record) => {
                let mut candidates = Vec::new();
                let loaded = self.sources.load_traced(&request, &mut candidates);
                record.lookups.push(crate::dependency_manifest::Lookup {
                    namespace: name.to_owned(),
                    requester: requesting.to_owned(),
                    candidates,
                    found: loaded
                        .as_ref()
                        .ok()
                        .and_then(|input| input.as_ref().map(|input| input.file.clone())),
                });
                loaded
            }
            None => self.sources.load(&request),
        };
        match loaded {
            Ok(Some(input)) => Ok(Some((PathBuf::from(input.file), input.source))),
            Ok(None) => Ok(None),
            Err(LoadError::Provider(message)) => Err(NamespaceCompileError::new(message)),
            Err(error) => Err(NamespaceCompileError::new(error.to_string())),
        }
    }

    /// The id of the last lookup [`Self::find`] recorded.
    fn last_lookup(&self) -> crate::dependency_manifest::LookupId {
        let count = self
            .record
            .as_ref()
            .map_or(0, |record| record.lookups.len());
        crate::dependency_manifest::LookupId(count.saturating_sub(1) as u32)
    }

    fn visit(&mut self, name: &str) -> Result<(), NamespaceCompileError> {
        // The bounded first-party core is emitted inside the unit. A require
        // cannot replace it with a file found on a source root.
        if matches!(name, "cljs.core" | "clojure.core") {
            return Ok(());
        }
        match self.states.get(name).copied().unwrap_or(LoadState::Unseen) {
            LoadState::Loaded => return Ok(()),
            LoadState::Loading => {
                let start = self
                    .stack
                    .iter()
                    .position(|entry| entry == name)
                    .expect("loading namespace in stack");
                let mut cycle = self.stack[start..].to_vec();
                cycle.push(name.to_owned());
                return Err(NamespaceCompileError::new(format!(
                    "namespace dependency cycle: {}",
                    cycle.join(" -> ")
                )));
            }
            LoadState::Unseen => {}
        }
        use crate::dependency_manifest::{RequireForm, SourceOrigin};
        let found = self.find(name)?;
        let source_lookup = self.last_lookup();
        let mut origin = SourceOrigin::Provider;
        let (path, source) = if let Some(found) = found {
            found
        } else if let Some(source) = crate::compiler_libraries::source(name) {
            origin = SourceOrigin::Bundled;
            (
                PathBuf::from(format!("<first-party>/{}.cljs", name.replace('.', "/"))),
                source.to_owned(),
            )
        } else {
            return Err(NamespaceCompileError::new(format!(
                "namespace {name} not found: expected {}.cljs or .cljc in explicit source roots",
                name.replace('.', "/").replace('-', "_")
            )));
        };
        let cljc = path.extension().is_some_and(|ext| ext == "cljc");
        let mut parser = crate::Parser::new(&source).retaining_discards();
        let mut roots = Vec::new();
        while let Some(root) = parser.read_form().map_err(|error| {
            NamespaceCompileError::new(error.to_string()).at(&path, &source, error.span)
        })? {
            roots.push(root);
        }
        let mut discards = parser.discarded().to_vec();
        let mut syntax = SyntaxTree {
            nodes: parser.into_nodes(),
            roots,
        };
        if cljc {
            crate::compiler_reader::select_cljs(&source, &mut syntax, &mut discards).map_err(
                |error| {
                    NamespaceCompileError::new(error.to_string()).at(&path, &source, error.span)
                },
            )?;
        }
        let mut file = NamespaceSource {
            name: name.to_owned(),
            name_span: 0..0,
            path: path.clone(),
            source,
            syntax,
            aliases: Vec::new(),
            requires: Vec::new(),
            require_spans: Vec::new(),
            runtime_specs: Vec::new(),
            namespace_imports: Vec::new(),
            warning_mode: self.warning_mode,
            inert_packages: Vec::new(),
            resolution: crate::analyzer::NamespaceResolution {
                source_name: Some(path.to_string_lossy().into_owned()),
                cljc,
                ..Default::default()
            },
        };
        file.read_header()?;
        // Closure libraries are JavaScript host namespaces. A source namespace
        // provided explicitly by the caller still takes precedence.
        let mut host_namespaces = Vec::new();
        // For a manifest: each `goog.*` source check, and the host requires.
        let mut goog_checks = Vec::new();
        let mut recorded_requires = Vec::new();
        for spec in file.runtime_specs.clone() {
            let namespace = &spec.import.namespace;
            if !namespace.starts_with("goog.") {
                continue;
            }
            let provided = self.find(namespace)?.is_some();
            goog_checks.push((namespace.clone(), self.last_lookup()));
            if !provided {
                let node = spec.alias_node.unwrap_or(file.syntax.roots[0]);
                file.add_js_refer(node, "goog".into(), "goog".into())?;
                file.add_js_refer(node, namespace.clone(), namespace.clone())?;
                if let Some(alias) = &spec.import.alias {
                    file.add_js_refer(node, alias.clone(), namespace.clone())?;
                }
                for original in &spec.import.refers {
                    let local = spec
                        .renames
                        .iter()
                        .find(|(from, _)| from == original)
                        .map_or_else(|| original.clone(), |(_, to)| to.clone());
                    file.add_js_refer(node, local, format!("{namespace}.{original}"))?;
                }
                recorded_requires.push((
                    namespace.clone(),
                    namespace.clone(),
                    RequireForm::Declared,
                    vec![self.last_lookup()],
                ));
                host_namespaces.push(namespace.clone());
            }
        }
        let (requires, spans) = file
            .requires
            .into_iter()
            .zip(file.require_spans)
            .filter(|(name, _)| !host_namespaces.contains(name))
            .unzip();
        file.requires = requires;
        file.require_spans = spans;
        file.aliases
            .retain(|(_, target)| !host_namespaces.contains(target));
        file.runtime_specs
            .retain(|spec| !host_namespaces.contains(&spec.import.namespace));
        let mut forms: Vec<(String, RequireForm, Vec<_>)> = file
            .requires
            .iter()
            .map(|dependency| {
                let checks = goog_checks
                    .iter()
                    .filter(|(namespace, _)| namespace == dependency)
                    .map(|(_, lookup)| *lookup)
                    .collect();
                (dependency.clone(), RequireForm::Declared, checks)
            })
            .collect();
        for (index, dependency) in file.requires.iter_mut().enumerate() {
            if let Some(suffix) = dependency.strip_prefix("clojure.")
                && dependency != "clojure.core"
                && {
                    let absent = self.find(dependency)?.is_none();
                    forms[index].2.push(self.last_lookup());
                    absent
                }
            {
                let rewritten = format!("cljs.{suffix}");
                let provided = self.find(&rewritten)?.is_some();
                forms[index].2.push(self.last_lookup());
                if provided {
                    forms[index].1 = RequireForm::ClojureRewrite;
                    for (_, target) in &mut file.aliases {
                        if target == dependency {
                            *target = rewritten.clone();
                        }
                    }
                    file.aliases.push((dependency.clone(), rewritten.clone()));
                    for spec in &mut file.runtime_specs {
                        if spec.import.namespace == *dependency {
                            spec.import.namespace = rewritten.clone();
                        }
                    }
                    *dependency = rewritten;
                }
            }
        }
        // `light.lazy/loadable` is a special form that needs no require, but
        // its sites construct a `light.lazy/Loadable` when the source runs: a
        // source spelling it requires `light.lazy` after its own requires.
        if name != LAZY_NAMESPACE
            && !file
                .requires
                .iter()
                .any(|required| required == LAZY_NAMESPACE)
            && let Some(span) = file.spells_loadable()
        {
            file.requires.push(LAZY_NAMESPACE.to_owned());
            file.require_spans.push(span);
            forms.push((
                LAZY_NAMESPACE.to_owned(),
                RequireForm::ImplicitLazy,
                Vec::new(),
            ));
        }
        self.states.insert(name.to_owned(), LoadState::Loading);
        self.stack.push(name.to_owned());
        for (dependency, span) in file.requires.iter().zip(&file.require_spans) {
            self.visit(dependency).map_err(|error| {
                if error.path.is_none() {
                    error.at(&file.path, &file.source, span.clone())
                } else {
                    error
                }
            })?;
        }
        self.stack.pop();
        self.states.insert(name.to_owned(), LoadState::Loaded);
        if let Some(record) = &mut self.record {
            let mut requires: Vec<_> = forms
                .into_iter()
                .zip(&file.requires)
                .map(|((written, form, lookups), target)| (written, target.clone(), form, lookups))
                .collect();
            requires.append(&mut recorded_requires);
            record
                .sources
                .push(crate::dependency_manifest::LoadedSource {
                    name: name.to_owned(),
                    file: path.to_string_lossy().into_owned(),
                    kind: if cljc {
                        crate::dependency_manifest::SourceKind::Cljc
                    } else {
                        crate::dependency_manifest::SourceKind::Cljs
                    },
                    origin,
                    lookup: source_lookup,
                    requires,
                });
        }
        self.ordered.push(file);
        Ok(())
    }

    /// Visit `name`, the target namespace of a lazy reference in source
    /// `requester`. A namespace already loaded is resident. Otherwise this
    /// appends the namespace's unvisited static dependencies and then the
    /// namespace itself; a dependency on `requester` or on another loaded
    /// namespace is no cycle.
    fn visit_lazy(&mut self, name: &str, requester: usize) -> Result<(), NamespaceCompileError> {
        if matches!(self.states.get(name), Some(LoadState::Loaded)) {
            return Ok(());
        }
        // The requester is the namespace asking the provider for the target.
        self.stack.push(self.ordered[requester].name.clone());
        let result = match self.find(name) {
            Ok(None) if crate::compiler_libraries::source(name).is_none() => {
                Err(NamespaceCompileError::new(format!(
                    "lazy target namespace {name} not found: expected {}.cljs or .cljc in explicit source roots",
                    name.replace('.', "/").replace('-', "_")
                )))
            }
            Ok(_) => self.visit(name),
            Err(error) => Err(error),
        };
        self.stack.pop();
        result
    }
}

/// Load an entry namespace and its dependencies from explicit roots, then emit
/// one JavaScript expression. Dependencies initialize once, in depth-first order.
/// Namespace dots become directories and hyphens become underscores. `.cljs`
/// takes precedence over `.cljc`; distinct matches at that extension are ambiguous.
pub fn compile_namespace(
    entry: &str,
    source_roots: &[PathBuf],
) -> Result<String, NamespaceCompileError> {
    let mut macros = MacroHost::new();
    compile_namespace_with_macros(entry, source_roots, &mut macros)
}

/// Reuse explicit macro configuration with fresh source definitions and evaluator
/// roots for each compilation. Source loading is restricted to the supplied roots.
pub fn compile_namespace_with_macros(
    entry: &str,
    source_roots: &[PathBuf],
    macros: &mut MacroHost,
) -> Result<String, NamespaceCompileError> {
    compile_namespace_with_macro_warnings(
        entry,
        source_roots,
        macros,
        AnalysisOptions::default(),
        &mut Vec::new(),
    )
}

/// Compile namespace programs with explicit CLJS-compatible warning diagnostics.
pub fn compile_namespace_with_warnings(
    entry: &str,
    source_roots: &[PathBuf],
    options: AnalysisOptions,
    warnings: &mut Vec<AnalysisWarning>,
) -> Result<String, NamespaceCompileError> {
    compile_namespace_with_macro_warnings(
        entry,
        source_roots,
        &mut MacroHost::new(),
        options,
        warnings,
    )
}

pub fn compile_namespace_with_macro_warnings(
    entry: &str,
    source_roots: &[PathBuf],
    macros: &mut MacroHost,
    options: AnalysisOptions,
    warnings: &mut Vec<AnalysisWarning>,
) -> Result<String, NamespaceCompileError> {
    analyze_namespace_entry(entry, source_roots, macros, options, warnings).map(|p| emit_js(&p))
}

/// Compile an entry namespace into one ES module for a bundler. JavaScript
/// package references become static `import` declarations instead of
/// [`crate::analyzer::PACKAGE_REGISTRY`] lookups, and the entry value is the
/// default export. See [`crate::emitter::emit_esm`].
pub fn compile_namespace_esm(
    entry: &str,
    source_roots: &[PathBuf],
) -> Result<String, NamespaceCompileError> {
    compile_namespace_esm_with_warnings(
        entry,
        source_roots,
        AnalysisOptions::default(),
        &mut Vec::new(),
    )
}

/// [`compile_namespace_esm`] with explicit warning-compatible analysis options.
pub fn compile_namespace_esm_with_warnings(
    entry: &str,
    source_roots: &[PathBuf],
    options: AnalysisOptions,
    warnings: &mut Vec<AnalysisWarning>,
) -> Result<String, NamespaceCompileError> {
    analyze_namespace_entry(
        entry,
        source_roots,
        &mut MacroHost::new(),
        options,
        warnings,
    )
    .map(|p| crate::emitter::emit_esm(&p))
}

/// Load and analyze an entry namespace and its dependencies like
/// [`compile_namespace`], but return the combined program instead of emitting
/// it, so the caller picks the optimization level and output format:
/// `prepare_application(&program, ApplicationOptions::new(level))` followed by
/// `emit_js()` or `emit_esm()`. The `--compile --entry` CLI uses this.
pub fn analyze_namespace_entry(
    entry: &str,
    source_roots: &[PathBuf],
    macros: &mut MacroHost,
    options: AnalysisOptions,
    warnings: &mut Vec<AnalysisWarning>,
) -> Result<crate::Program, NamespaceCompileError> {
    analyze_namespace_entry_with_sources(entry, source_roots, macros, options, warnings)
        .map(|(program, _)| program)
}

/// [`analyze_namespace_entry`], also returning the path and text of each
/// source the program was analyzed from, by source index (the order of
/// loading, dependencies first): the sources a source map names
/// ([`crate::source_map`]).
pub fn analyze_namespace_entry_with_sources(
    entry: &str,
    source_roots: &[PathBuf],
    macros: &mut MacroHost,
    options: AnalysisOptions,
    warnings: &mut Vec<AnalysisWarning>,
) -> Result<(crate::Program, Vec<(PathBuf, String)>), NamespaceCompileError> {
    analyze_from_roots(
        entry,
        source_roots,
        macros,
        options,
        warnings,
        EntryRecorders::default(),
    )
}

/// [`analyze_namespace_entry_with_sources`], also recording in `stats` the
/// times of source resolution and parsing, analysis and deferred units, the
/// namespaces compiled and the macro calls each executed
/// ([`crate::compile_stats`]). A caller that prepares, emits and writes the
/// program adds those phases to `stats.phases` and `stats.wall` itself, as
/// the `--stats` CLI flag does.
pub fn analyze_namespace_entry_with_stats(
    entry: &str,
    source_roots: &[PathBuf],
    macros: &mut MacroHost,
    options: AnalysisOptions,
    warnings: &mut Vec<AnalysisWarning>,
    stats: &mut CompileStats,
) -> Result<(crate::Program, Vec<(PathBuf, String)>), NamespaceCompileError> {
    // The numbers describe this compilation alone, whatever `stats` held.
    *stats = CompileStats::default();
    analyze_from_roots(
        entry,
        source_roots,
        macros,
        options,
        warnings,
        EntryRecorders {
            stats: Some(stats),
            ..EntryRecorders::default()
        },
    )
}

/// [`analyze_namespace_entry_with_sources`] with the persistent compiler
/// cache in `cache.dir` (`--cache-dir`, docs/compiler-cache.md): each
/// namespace whose artifact matches its current inputs and policy is
/// installed instead of analyzed, each other one is analyzed and, unless
/// the policy bypasses it, published. After a successful analysis the
/// directory is trimmed to its retention bounds. With `stats`, it is filled
/// as [`analyze_namespace_entry_with_stats`] fills it, with
/// [`CompileStats::cache`] too.
pub fn analyze_namespace_entry_cached(
    entry: &str,
    source_roots: &[PathBuf],
    macros: &mut MacroHost,
    options: AnalysisOptions,
    warnings: &mut Vec<AnalysisWarning>,
    cache: &crate::compile_cache::CacheOptions,
    stats: Option<&mut CompileStats>,
) -> Result<(crate::Program, SourceFiles), NamespaceCompileError> {
    let mut store = crate::compile_cache::CompileCache::open(cache, macros, options, entry);
    let mut stats = stats;
    if let Some(stats) = stats.as_deref_mut() {
        *stats = CompileStats::default();
    }
    let analyzed = analyze_from_roots(
        entry,
        source_roots,
        macros,
        options,
        warnings,
        EntryRecorders {
            stats: stats.as_deref_mut(),
            cache: Some(&mut store),
            ..EntryRecorders::default()
        },
    );
    if analyzed.is_ok() {
        store.finish();
    }
    if let Some(stats) = stats {
        stats.cache = Some(store.take_counts());
    }
    analyzed
}

/// [`analyze_namespace_entry_with_sources`], also capturing each namespace
/// as a [`crate::namespace_artifact::NamespaceArtifact`] where its analysis
/// returns, and handing it to `capture` in source order before the next
/// namespace begins (docs/compiler-cache.md, "Binary artifacts"). A
/// namespace that cannot be captured is handed over with the reason. With
/// `stats`, also record what [`analyze_namespace_entry_with_stats`] does and
/// the capture's counts. Without this call nothing is captured, and analysis
/// keeps nothing for it.
pub fn analyze_namespace_entry_with_artifacts(
    entry: &str,
    source_roots: &[PathBuf],
    macros: &mut MacroHost,
    options: AnalysisOptions,
    warnings: &mut Vec<AnalysisWarning>,
    stats: Option<&mut CompileStats>,
    capture: &mut dyn FnMut(crate::namespace_artifact::NamespaceCapture),
) -> Result<(crate::Program, SourceFiles), NamespaceCompileError> {
    let stats = stats.map(|stats| {
        *stats = CompileStats::default();
        stats
    });
    analyze_from_roots(
        entry,
        source_roots,
        macros,
        options,
        warnings,
        EntryRecorders {
            stats,
            capture: Some(capture),
            ..EntryRecorders::default()
        },
    )
}

/// The path and text of each source a program was analyzed from, by source
/// index.
pub type SourceFiles = Vec<(PathBuf, String)>;

/// [`analyze_namespace_entry_with_sources`], also returning the
/// [`DependencyManifest`] of the compilation: each namespace's dependencies
/// by kind, with their reverse edges, and how each of its requires resolved,
/// every candidate file tried included ([`crate::dependency_manifest`]).
/// Without this call no manifest is recorded.
pub fn analyze_namespace_entry_with_dependencies(
    entry: &str,
    source_roots: &[PathBuf],
    macros: &mut MacroHost,
    options: AnalysisOptions,
    warnings: &mut Vec<AnalysisWarning>,
) -> Result<(crate::Program, SourceFiles, DependencyManifest), NamespaceCompileError> {
    let mut manifest = DependencyManifest::default();
    let (program, files) = analyze_from_roots(
        entry,
        source_roots,
        macros,
        options,
        warnings,
        EntryRecorders {
            manifest: Some(&mut manifest),
            ..EntryRecorders::default()
        },
    )?;
    Ok((program, files, manifest))
}

/// What an entry compilation records besides its program, when asked:
/// `--stats` numbers, the dependency manifest and namespace artifacts; and
/// the artifacts it installs in place of analysis.
#[derive(Default)]
pub struct EntryRecorders<'a> {
    /// Reset, then filled as [`analyze_namespace_entry_with_stats`] fills it.
    pub stats: Option<&'a mut CompileStats>,
    /// Filled as [`analyze_namespace_entry_with_dependencies`] fills it.
    pub manifest: Option<&'a mut DependencyManifest>,
    /// Handed each analyzed namespace's artifact, as
    /// [`analyze_namespace_entry_with_artifacts`] does.
    pub capture: Option<&'a mut dyn FnMut(crate::namespace_artifact::NamespaceCapture)>,
    /// Asked for each namespace, in source order, whether to install an
    /// artifact in its place (docs/compiler-cache.md, "Cache contract"). The
    /// caller decides eligibility; an artifact that does not fit the
    /// compilation is refused, with its reason in the stats, and its
    /// namespace analyzed instead. Installed namespaces execute no macro
    /// calls, are not captured, and add no observations to the manifest.
    pub install: Option<ArtifactLookup<'a>>,
    /// The generated names of every artifact `install` may hand over
    /// ([`crate::generated_names::Reservation::of`]), reserved before the
    /// first namespace is read, so that no namespace analyzed before a later
    /// hit allocates a name the hit retains. Without it, such a hit is
    /// refused where its window overlaps numbers the compilation allocated,
    /// and nothing keeps its `gensym` spellings from earlier misses. Each
    /// installed artifact reserves its names in any case, which keeps the
    /// namespaces after it apart from it.
    pub reserve: Option<&'a crate::generated_names::Reservation>,
    /// The execution control that cancels this compilation, such as a
    /// development session's. Installation checks it, and the compile-time
    /// session's own, while it runs: a cancelled or terminal control stops
    /// the compilation rather than letting a hit stand in for work that
    /// could not run.
    pub control: Option<&'a crate::evaluator::ExecutionControl>,
    /// The persistent cache (`--cache-dir`, [`crate::compile_cache`]):
    /// asked for each namespace, in source order, for an artifact to
    /// install, and handed each analyzed namespace's capture to publish.
    /// It reserves its candidates' generated names unless `reserve` is
    /// given. Without it nothing reads or writes a cache.
    pub cache: Option<&'a mut crate::compile_cache::CompileCache>,
}

/// The artifact to install for a namespace, if any ([`EntryRecorders::install`]).
pub type ArtifactLookup<'a> =
    &'a dyn Fn(&str) -> Option<&'a crate::namespace_artifact::NamespaceArtifact>;

/// [`analyze_namespace_entry_with_sources`] with any of the recorders
/// [`EntryRecorders`] holds, including installing namespace artifacts in place of
/// analysis. Installing reads no artifact the caller does not hand over:
/// there is no store yet.
pub fn analyze_namespace_entry_recorded(
    entry: &str,
    source_roots: &[PathBuf],
    macros: &mut MacroHost,
    options: AnalysisOptions,
    warnings: &mut Vec<AnalysisWarning>,
    mut recorders: EntryRecorders<'_>,
) -> Result<(crate::Program, SourceFiles), NamespaceCompileError> {
    if let Some(stats) = recorders.stats.as_deref_mut() {
        *stats = CompileStats::default();
    }
    analyze_from_roots(entry, source_roots, macros, options, warnings, recorders)
}

/// [`analyze_namespace_entry_recorded`] with sources from a
/// [`SourceProvider`], as [`analyze_namespace_entry_with_provider`] takes
/// them.
pub fn analyze_namespace_entry_recorded_with_provider(
    entry: &str,
    sources: &dyn SourceProvider,
    macros: &mut MacroHost,
    options: AnalysisOptions,
    warnings: &mut Vec<AnalysisWarning>,
    mut recorders: EntryRecorders<'_>,
) -> Result<(crate::Program, SourceFiles), NamespaceCompileError> {
    if let Some(stats) = recorders.stats.as_deref_mut() {
        *stats = CompileStats::default();
    }
    analyze_entry(entry, sources, macros, options, warnings, recorders)
}

fn analyze_from_roots(
    entry: &str,
    source_roots: &[PathBuf],
    macros: &mut MacroHost,
    options: AnalysisOptions,
    warnings: &mut Vec<AnalysisWarning>,
    recorders: EntryRecorders<'_>,
) -> Result<(crate::Program, Vec<(PathBuf, String)>), NamespaceCompileError> {
    if !valid_namespace(entry) {
        return Err(NamespaceCompileError::new(
            "invalid entry namespace: use dot-separated ASCII names; js is reserved",
        ));
    }
    if source_roots.is_empty() {
        return Err(NamespaceCompileError::new(
            "namespace compilation requires at least one explicit source root",
        ));
    }
    let mut roots: Vec<PathBuf> = Vec::new();
    for root in source_roots {
        let canonical = fs::canonicalize(root).map_err(|error| {
            NamespaceCompileError::new(format!(
                "cannot open source root {}: {error}",
                root.display()
            ))
        })?;
        if !canonical.is_dir() {
            return Err(NamespaceCompileError::new(format!(
                "source root is not a directory: {}",
                root.display()
            )));
        }
        if !roots.contains(&canonical) {
            roots.push(canonical);
        }
    }
    analyze_entry(
        entry,
        &FileSourceProvider::new(roots),
        macros,
        options,
        warnings,
        recorders,
    )
}

/// [`analyze_namespace_entry`] with sources from any [`SourceProvider`]
/// instead of directories on disk, for hosts without a filesystem such as the
/// browser playground. The provider answers each namespace with its file name
/// (whose extension decides `.cljc` reading) and text; a `clojure.*`
/// namespace it does not have falls back to the bundled first-party library
/// or to the `cljs.*` spelling, as with roots.
pub fn analyze_namespace_entry_with_provider(
    entry: &str,
    sources: &dyn SourceProvider,
    macros: &mut MacroHost,
    options: AnalysisOptions,
    warnings: &mut Vec<AnalysisWarning>,
) -> Result<crate::Program, NamespaceCompileError> {
    analyze_entry(
        entry,
        sources,
        macros,
        options,
        warnings,
        EntryRecorders::default(),
    )
    .map(|(program, _)| program)
}

/// The program of an entry namespace and the path and text of each source,
/// by source index.
///
/// The loader first reaches the entry and its static dependencies, which are
/// analyzed in that order. A `light.lazy/loadable` site is not a dependency:
/// after a source is analyzed, the loader visits each namespace its lazy
/// references name, which appends that namespace's unvisited dependencies and
/// then the namespace itself, to be analyzed in turn. A target already loaded
/// is resident, and a target may require the namespace that loads it, which
/// is loaded by then (docs/code-splitting.md, "Roots, edges and reach").
///
/// With `stats`, also time the phases and count each namespace's macro
/// calls; without, no clock is read.
fn analyze_entry(
    entry: &str,
    sources: &dyn SourceProvider,
    macros: &mut MacroHost,
    options: AnalysisOptions,
    warnings: &mut Vec<AnalysisWarning>,
    recorders: EntryRecorders<'_>,
) -> Result<(crate::Program, Vec<(PathBuf, String)>), NamespaceCompileError> {
    let EntryRecorders {
        mut stats,
        manifest,
        mut capture,
        install,
        reserve,
        control,
        mut cache,
    } = recorders;
    let cache_reservation = cache
        .as_deref()
        .filter(|_| reserve.is_none())
        .map(|cache| cache.reservation().clone());
    let reserve = reserve.or(cache_reservation.as_ref());
    // The cache captures each namespace it analyzes, to publish it.
    let capturing = capture.is_some() || cache.is_some();
    if let Some(reservation) = reserve {
        crate::generated_names::reserve(&reservation.to);
    }
    // Every number the allocators give from here on is this compilation's.
    let names_start = crate::generated_names::current();
    let mut install_counts = crate::compile_stats::InstallCounts::default();
    let timed = stats.is_some();
    let begun = compile_stats::start(timed);
    if !valid_namespace(entry) {
        return Err(NamespaceCompileError::new(
            "invalid entry namespace: use dot-separated ASCII names; js is reserved",
        ));
    }
    let mut loader = Loader {
        sources,
        entry: entry.to_owned(),
        states: HashMap::new(),
        stack: Vec::new(),
        ordered: Vec::new(),
        warning_mode: options.warning_mode,
        record: (manifest.is_some() || cache.is_some()).then(Default::default),
    };
    let mut resolve_parse = Duration::ZERO;
    let started = compile_stats::start(timed);
    loader.visit(entry)?;
    compile_stats::add(&mut resolve_parse, started);
    let entry_index = loader.ordered.len() - 1;
    source_warnings(&loader.ordered, 0, options, warnings);
    let analysis_started = compile_stats::start(timed);
    // Lazily reached sources are resolved and parsed between analyses.
    let mut lazy_parse = Duration::ZERO;
    // The persistent cache's lookups and publications, outside `analysis`.
    let mut cache_time = Duration::ZERO;
    let mut analysis = NamespaceAnalysis::new(macros);
    analysis.begin_generated_names(names_start);
    // The compilation's session exists from here on.
    if let Some(reservation) = reserve
        && let Some(session) = macros.session_handle()
    {
        session
            .reserve_gensyms(&reservation.gensyms, reservation.gensym_to)
            .map_err(NamespaceCompileError::new)?;
    }
    if timed {
        analysis.deferred_time = Some(Duration::ZERO);
    }
    let mut capture_counts = crate::compile_stats::CaptureCounts::default();
    if capturing && let Some(session) = macros.session_handle() {
        session.begin_capture();
        session.log_gensyms().map_err(NamespaceCompileError::new)?;
    }
    // Analysis logs what it observes beyond the `ns` forms only while a
    // manifest is recorded, and stops when this compilation ends, however.
    let observations = (manifest.is_some() || cache.is_some()).then(|| {
        *macros.dependency_log.borrow_mut() = Some(crate::dependency_manifest::ObservationLog {
            configuration: Some(macros.configuration.fingerprint()),
            ..Default::default()
        });
        ObservationGuard(macros.dependency_log.clone())
    });
    // `light.build` serves this compilation, with the provider's resources.
    let build = crate::build_services::BuildGuard::enter(
        &macros.build,
        sources.resources(),
        macros.configuration.clone(),
    );
    let mut index = 0;
    while index < loader.ordered.len() {
        let file = &loader.ordered[index];
        let calls_before = macros.executed_expansions();
        let at = |error: AnalysisError| {
            NamespaceCompileError::new(error.to_string()).at(&file.path, &file.source, error.span)
        };
        let first_lazy = analysis.lazy_target_count();
        macros.observe(|log| log.window = index as u32);
        build.analyze(index, &file.name, file.path.to_str());
        // A cancelled compilation stops at the next namespace.
        if let Some(reason) = control.and_then(|control| control.check().err()) {
            return Err(NamespaceCompileError::new(format!(
                "compiling {}: {}",
                file.name, reason.0
            )));
        }
        // An artifact installs at the frontier its analysis would have run.
        let started = compile_stats::start(timed);
        let cached = match (cache.as_deref_mut(), &loader.record) {
            (Some(cache), Some(record)) => {
                let cancelled = || macros.halted(control).is_some();
                cache
                    .lookup(record, &file.source, sources, &cancelled)
                    .map_err(|message| {
                        NamespaceCompileError::new(format!("loading {}: {message}", file.name))
                    })?
            }
            _ => None,
        };
        compile_stats::add(&mut cache_time, started);
        let installed = match cached
            .as_ref()
            .or_else(|| install.and_then(|lookup| lookup(&file.name)))
        {
            Some(artifact) => {
                let started = compile_stats::start(timed);
                let path = file.path.to_string_lossy();
                let outcome = analysis.install(
                    artifact,
                    &crate::artifact_install::InstallSource {
                        namespace: &file.name,
                        path: &path,
                        text: &file.source,
                        entry: index == entry_index,
                        options,
                        control,
                    },
                    macros,
                    warnings,
                );
                compile_stats::add(&mut install_counts.time, started);
                match outcome {
                    Ok(()) => {
                        if let (Some(cache), Some(_)) = (cache.as_deref_mut(), &cached) {
                            cache.installed(macros);
                        }
                        install_counts.installed.push(file.name.clone());
                        true
                    }
                    Err(crate::artifact_install::InstallError::Refused(reason)) => {
                        if let (Some(cache), Some(_)) = (cache.as_deref_mut(), &cached) {
                            cache.refused(&reason);
                        }
                        install_counts.refused.push((file.name.clone(), reason));
                        false
                    }
                    Err(crate::artifact_install::InstallError::Failed(message)) => {
                        return Err(NamespaceCompileError::new(format!(
                            "installing {}: {message}",
                            file.name
                        ))
                        .at(&file.path, &file.source, 0..0));
                    }
                }
            }
            None => false,
        };
        // A decoded artifact is released once installed.
        drop(cached);
        if !installed {
            let names_from = crate::generated_names::current();
            let work = capturing.then(|| macros.begin_namespace_work());
            let resolution = prepare_source(macros, file).map_err(at)?;
            let captured = capturing.then(|| {
                let started = compile_stats::start(timed);
                let start = analysis.begin_capture(warnings, macros);
                compile_stats::add(&mut capture_counts.time, started);
                (start, resolution.clone())
            });
            let input = NamespaceInput {
                name: &file.name,
                source: &file.source,
                syntax: &file.syntax,
                aliases: &file.aliases,
                requires: &file.requires,
            };
            analysis
                .analyze(
                    &input,
                    resolution,
                    index == entry_index,
                    macros,
                    options,
                    warnings,
                )
                .map_err(at)?;
            if let (Some((start, resolution)), Some(work)) = (&captured, work) {
                let started = compile_stats::start(timed);
                let path = file.path.to_string_lossy();
                let artifact = analysis.finish_capture(
                    start,
                    crate::artifact_capture::CaptureInput {
                        namespace: &file.name,
                        path: &path,
                        source_length: file.source.len(),
                        entry: index == entry_index,
                        resolution,
                        aliases: &file.aliases,
                        requires: &file.requires,
                        names_from,
                        options,
                        work,
                        control,
                    },
                    macros,
                    warnings,
                );
                match &artifact {
                    Ok(artifact) => {
                        capture_counts.captured += 1;
                        if timed {
                            capture_counts.bytes += artifact.approximate_bytes();
                        }
                    }
                    Err(_) => capture_counts.skipped += 1,
                }
                drop(captured);
                compile_stats::add(&mut capture_counts.time, started);
                let started = compile_stats::start(timed);
                match (cache.as_deref_mut(), &loader.record) {
                    (Some(cache), Some(record)) => cache.analyzed(
                        record,
                        &file.source,
                        sources,
                        macros,
                        analysis.program_namespaces(),
                        Some(artifact),
                    ),
                    _ => {
                        if let Some(sink) = capture.as_deref_mut() {
                            sink(crate::namespace_artifact::NamespaceCapture {
                                namespace: file.name.clone(),
                                artifact,
                            });
                        }
                    }
                }
                compile_stats::add(&mut cache_time, started);
            }
        }
        if let Some(stats) = stats.as_deref_mut() {
            stats.namespaces.push(compile_stats::NamespaceStats {
                name: file.name.clone(),
                macro_calls: macros.executed_expansions() - calls_before,
            });
        }
        // Deferred definitions own their forms and resolution snapshots now.
        // Keep source text for diagnostics, but release each parsed tree as
        // soon as analysis finishes instead of retaining all trees until the
        // last namespace (including lazily discovered ones) has completed.
        let syntax = &mut loader.ordered[index].syntax;
        syntax.nodes = Vec::new();
        syntax.roots = Vec::new();
        let loaded = loader.ordered.len();
        let started = compile_stats::start(timed);
        for (namespace, span) in analysis.lazy_namespaces_since(first_lazy) {
            if let Some(record) = &mut loader.record {
                record.lazy.push((index as u32, namespace.clone()));
            }
            loader.visit_lazy(&namespace, index).map_err(|error| {
                if error.path.is_none() {
                    let file = &loader.ordered[index];
                    error.at(&file.path, &file.source, span)
                } else {
                    error
                }
            })?;
        }
        compile_stats::add(&mut lazy_parse, started);
        source_warnings(&loader.ordered, loaded, options, warnings);
        index += 1;
    }
    let deferred_time = analysis.deferred_time;
    let program = analysis
        .finish(entry_index, macros)
        .map_err(|(index, error)| {
            let file = &loader.ordered[index];
            NamespaceCompileError::new(error.to_string()).at(&file.path, &file.source, error.span)
        })?;
    drop(build);
    if let (Some(stats), Some(analysis_started), Some(deferred)) =
        (stats, analysis_started, deferred_time)
    {
        let phases = &mut stats.phases;
        phases.resolve_parse += resolve_parse + lazy_parse;
        phases.deferred_units += deferred;
        phases.analysis += analysis_started.elapsed().saturating_sub(
            deferred + lazy_parse + capture_counts.time + install_counts.time + cache_time,
        );
        if capturing {
            stats.capture = Some(capture_counts);
        }
        if install.is_some() || cache.is_some() {
            stats.install = Some(install_counts);
        }
        stats.macro_calls = macros.executed_expansions();
        stats.peak_macro_nodes = macros.generated_nodes.get();
        if let Some(session) = macros.session_handle() {
            stats.deferred_units = session.unit_counts();
        }
        compile_stats::add(&mut stats.wall, begun);
    }
    if let (Some(manifest), Some(record), Some(observations)) =
        (manifest, loader.record.take(), observations)
    {
        let log = observations.0.borrow_mut().take().unwrap_or_default();
        // The macro host's inventory, not the program's reader contexts, which
        // a drained deferred unit can restore to a snapshot from before a
        // later `defmacro`.
        let owners: std::collections::HashSet<&str> = macros
            .javascript_macros
            .keys()
            .filter_map(|key| key.split_once('/').map(|(owner, _)| owner))
            .collect();
        *manifest = crate::dependency_manifest::build(record, log, &program.namespaces, |name| {
            owners.contains(name)
        });
    }
    let files = loader
        .ordered
        .into_iter()
        .map(|file| (file.path, file.source))
        .collect();
    Ok((program, files))
}

/// Stops a compilation's observation log when the compilation ends.
struct ObservationGuard(
    std::rc::Rc<std::cell::RefCell<Option<crate::dependency_manifest::ObservationLog>>>,
);

impl Drop for ObservationGuard {
    fn drop(&mut self) {
        self.0.borrow_mut().take();
    }
}

/// The warnings about the headers of `files[from..]`, which analysis reports
/// before the warnings of their bodies.
fn source_warnings(
    files: &[NamespaceSource],
    from: usize,
    options: AnalysisOptions,
    warnings: &mut Vec<AnalysisWarning>,
) {
    for (source_index, file) in files.iter().enumerate().skip(from) {
        for (package, span) in &file.inert_packages {
            warnings.push(AnalysisWarning {
                kind: "package-require-no-effect",
                message: format!(
                    "requiring JavaScript package \"{package}\" without :as, :refer or :default has no effect; the host loads packages"
                ),
                span: span.clone(),
                source_index,
            });
        }
    }
    if options.warning_mode {
        for (source_index, file) in files.iter().enumerate().skip(from) {
            if !file.name.contains('.') {
                warnings.push(AnalysisWarning {
                    kind: "single-segment-namespace",
                    message: format!("{} is a single segment namespace", file.name),
                    span: file.name_span.clone(),
                    source_index,
                });
            }
            for spec in file.runtime_specs.iter() {
                if spec.import.alias.as_deref() == Some("js") {
                    warnings.push(AnalysisWarning {
                        kind: "js-used-as-alias",
                        message: format!(
                            "In [{} :as js], the alias name js is reserved for JavaScript interop",
                            spec.import.namespace
                        ),
                        span: file.syntax.nodes[spec.alias_node.expect("alias node").0]
                            .span
                            .clone(),
                        source_index,
                    });
                }
            }
        }
    }
}

/// Configure the macro host for analyzing `file` and return how its names
/// resolve: its imports, refers, renamed macros and namespace metadata.
fn prepare_source(
    macros: &mut MacroHost,
    file: &NamespaceSource,
) -> Result<crate::analyzer::NamespaceResolution, AnalysisError> {
    macros.set_namespace_imports(&file.name, file.namespace_imports.clone());
    let error = |span: Range<usize>, message: String| AnalysisError {
        kind: crate::AnalysisErrorKind::MacroExpansion(message),
        span,
    };
    let header_span = file.syntax.nodes[file.syntax.roots[0].0].span.clone();
    let mut imports = Vec::new();
    let mut renamed_macros = Vec::new();
    for spec in &file.runtime_specs {
        let mut import = spec.import.clone();
        import.refers.clear();
        for original in &spec.import.refers {
            if macros.source_binding_private(&import.namespace, original) {
                return Err(error(
                    spec.span.clone(),
                    format!(
                        "{}/{original} is private; :refer requires a public binding",
                        import.namespace
                    ),
                ));
            }
            if macros
                .has_macro(&import.namespace, original)
                .map_err(|m| error(spec.span.clone(), m))?
            {
                let local = spec
                    .renames
                    .iter()
                    .find(|(from, _)| from == original)
                    .map_or(original, |(_, to)| to);
                if local == original {
                    import.refers.push(original.clone());
                } else {
                    renamed_macros.push((
                        local.clone(),
                        import.namespace.clone(),
                        original.clone(),
                    ));
                }
            }
        }
        imports.push(import);
    }
    let mut resolution = file.resolution.clone();
    for spec in &file.runtime_specs {
        for original in &spec.import.refers {
            let local = spec
                .renames
                .iter()
                .find(|(from, _)| from == original)
                .map(|(_, to)| to)
                .unwrap_or(original);
            let refer = (
                local.clone(),
                spec.import.namespace.clone(),
                original.clone(),
            );
            // The same refer twice is one refer, as upstream reads it.
            if resolution.refers.contains(&refer) {
                continue;
            }
            if resolution.refers.iter().any(|(name, _, _)| name == local) {
                return Err(error(
                    spec.span.clone(),
                    format!("duplicate or conflicting refer: {local}"),
                ));
            }
            resolution.refers.push(refer);
        }
    }
    for (local, namespace, original) in &file.resolution.refers {
        if matches!(namespace.as_str(), "cljs.core" | "clojure.core")
            && macros
                .has_macro(namespace, original)
                .map_err(|m| error(header_span.clone(), m))?
        {
            renamed_macros.push((local.clone(), namespace.clone(), original.clone()));
        }
    }
    macros
        .set_imports(&file.name, imports)
        .map_err(|m| error(header_span.clone(), m))?;
    macros.set_renames(&file.name, renamed_macros);
    let NodeKind::Collection { children, .. } = &file.syntax.nodes[file.syntax.roots[0].0].kind
    else {
        unreachable!()
    };
    let declared = children[1];
    if matches!(
        file.syntax.nodes[declared.0].kind,
        NodeKind::Metadata { .. }
    ) || children.get(2).is_some_and(|n| {
        matches!(
            file.syntax.nodes[n.0].kind,
            NodeKind::String(_)
                | NodeKind::Collection {
                    kind: CollectionKind::Map,
                    ..
                }
        )
    }) {
        let aliases = macros.reader_aliases(&file.name, &file.aliases, &file.requires);
        let source = crate::compiler_data::Source::new(file.path.to_string_lossy(), &file.source);
        let c = crate::compiler_data::DataBuilder;
        let mut view = crate::compiler_reader::NamespaceView::new(&file.name, &source);
        view.aliases = aliases
            .iter()
            .map(|(a, n)| {
                (
                    crate::compiler_data::Name::new(None, a),
                    crate::compiler_data::Name::new(None, n),
                )
            })
            .collect();
        let lower = |node| {
            crate::compiler_reader::lower_form(&source, &file.syntax, node, &view, &[])
                .map_err(|e| error(file.syntax.nodes[node.0].span.clone(), e.to_string()))
        };
        let name = lower(declared)?;
        let mut metadata = name.metadata().map(|f| f.data()).unwrap_or(
            c.map(&[])
                .map_err(|e| error(header_span.clone(), e.to_string()))?,
        );
        let mut docstring = metadata
            .get(&c.keyword(crate::compiler_data::Name::new(None, "doc")))
            .map_err(|e| error(header_span.clone(), e.to_string()))?;
        let mut offset = 2;
        if let Some(&doc) = children.get(offset)
            && matches!(file.syntax.nodes[doc.0].kind, NodeKind::String(_))
        {
            docstring = Some(lower(doc)?.data());
            offset += 1;
        }
        if let Some(&attrs) = children.get(offset)
            && matches!(
                file.syntax.nodes[attrs.0].kind,
                NodeKind::Collection {
                    kind: CollectionKind::Map,
                    ..
                }
            )
        {
            for (key, value) in lower(attrs)?
                .data()
                .entries()
                .map_err(|e| error(header_span.clone(), e.to_string()))?
            {
                metadata = c
                    .assoc(&metadata, &key, &value)
                    .map_err(|e| error(header_span.clone(), e.to_string()))?;
            }
        }
        let symbol = c
            .with_meta(&name.data(), &metadata)
            .map_err(|e| error(header_span.clone(), e.to_string()))?;
        macros.set_namespace_symbol(&file.name, symbol, docstring);
    }
    Ok(resolution)
}
