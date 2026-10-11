//! Small end-to-end driver. Analysis and emission remain independently usable.

use std::{
    collections::HashMap,
    fmt, fs,
    ops::Range,
    path::{Path, PathBuf},
};

use crate::analyzer::{
    AnalysisOptions, AnalysisWarning, NamespaceInput, analyze_namespaces_prepared_with_warnings,
    analyze_with_macros,
};
use crate::macro_host::{MacroHost, MacroImport};
use crate::source_provider::{LoadError, LoadRequest, Reload, SourceInput, SourceProvider};
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

/// Compile using an explicitly configured native macro context.
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
    import: MacroImport,
    renames: Vec<(String, String)>,
    macro_refers: Vec<String>,
    include_macros: bool,
    alias_only: bool,
    alias_node: Option<NodeId>,
    span: Range<usize>,
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
    macro_specs: Vec<RequireSpec>,
    resolution: crate::analyzer::NamespaceResolution,
    namespace_imports: Vec<(String, String)>,
    warning_mode: bool,
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
    fn read_spec(
        &self,
        spec: NodeId,
        macros: bool,
        only: bool,
    ) -> Result<RequireSpec, NamespaceCompileError> {
        let items = if self.symbol(spec).is_some() {
            vec![spec]
        } else {
            self.collection(spec, CollectionKind::Vector).or_else(|| self.collection(spec, CollectionKind::List))
                .ok_or_else(|| self.error(spec, "require expects a namespace or [namespace options]; JavaScript package imports are unsupported"))?.to_vec()
        };
        let dependency = *items
            .first()
            .ok_or_else(|| self.error(spec, "require specification cannot be empty"))?;
        let namespace = self.symbol(dependency).filter(|n| valid_namespace(n)).ok_or_else(|| self.error(dependency, "require library must be a supported namespace symbol; JavaScript package imports are unsupported"))?.to_owned();
        let mut result = RequireSpec {
            import: MacroImport {
                namespace,
                ..MacroImport::default()
            },
            renames: vec![],
            macro_refers: vec![],
            include_macros: false,
            alias_only: false,
            alias_node: None,
            span: self.syntax.nodes[dependency.0].span.clone(),
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
                return Err(self.error(spec, ":use/:use-macros expects :only and optional :rename"));
            }
            match option {
                ":only" if only => result.import.refers = self.names(pair[1])?,
                ":as" | ":as-alias" => {
                    if result.import.alias.is_some() {
                        return Err(self.error(spec, "only one :as alias is allowed"));
                    }
                    if macros && option == ":as-alias" {
                        return Err(self.error(spec, "unsupported macro require option :as-alias"));
                    }
                    result.alias_only = option == ":as-alias";
                    result.alias_node = Some(pair[1]);
                    result.import.alias = Some(self.symbol(pair[1]).filter(|n| valid_namespace(n) || (self.warning_mode && *n == "js")).ok_or_else(|| self.error(pair[1], ":as alias must be a supported namespace symbol; js is reserved"))?.to_owned());
                }
                ":refer" => result.import.refers = self.names(pair[1])?,
                ":rename" => result.renames = self.renames(pair[1])?,
                ":refer-macros" if !macros => {
                    result.macro_refers = self.names(pair[1])?;
                    result.include_macros = true;
                }
                ":include-macros" if !macros => {
                    if !matches!(self.syntax.nodes[pair[1].0].kind, NodeKind::Boolean(_)) {
                        return Err(self.error(pair[1], ":include-macros needs a boolean"));
                    }
                    // CLJS desugars by the presence of this option, including false.
                    result.include_macros = true;
                }
                _ => return Err(self.error(spec, "unsupported require options")),
            }
        }
        if only && !seen.contains(":only") {
            return Err(self.error(spec, ":use/:use-macros requires :only"));
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
        if let Some((_, previous)) = self
            .resolution
            .js_refers
            .iter()
            .find(|(name, _)| name == &local)
        {
            if previous == &path {
                return Ok(());
            }
            return Err(self.error(node, format!("conflicting global refer: {local}")));
        }
        self.resolution.js_refers.push((local, path));
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
        for clause in forms.into_iter().skip(offset) {
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
            clauses.push((clause, kind, parts[1..].to_vec()));
        }
        for &node in &self.syntax.roots[1..].to_vec() {
            self.top_level_clauses(node, &mut clauses)?;
        }
        for (clause, kind, arguments) in clauses {
            // Existing readers use a head slot; its spelling is irrelevant here.
            let mut parts = vec![clause];
            parts.extend(arguments);
            match kind.as_str() {
                ":require-global" => {
                    for spec in &parts[1..] {
                        let parsed = self.read_spec(*spec, false, false)?;
                        if parsed.include_macros
                            || parsed.alias_only
                            || !parsed.macro_refers.is_empty()
                        {
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
                        self.resolution.js_refers.push((local, original));
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
                ":require" | ":require-macros" | ":use" | ":use-macros" => {
                    for spec in parts.into_iter().skip(1) {
                        let is_macro = kind.ends_with("-macros");
                        let parsed = self.read_spec(spec, is_macro, kind.starts_with(":use"))?;
                        if is_macro {
                            if let Some(alias) = &parsed.import.alias
                                && let Some(previous) = self.macro_specs.iter().find(|previous| {
                                    previous.import.alias.as_ref() == Some(alias)
                                        && previous.import.namespace != parsed.import.namespace
                                })
                            {
                                return Err(self.error(
                                    parsed.alias_node.unwrap_or(spec),
                                    format!(
                                        "Alias {alias} already exists in namespace {}, aliasing {}",
                                        self.name, previous.import.namespace
                                    ),
                                ));
                            }
                            if self.macro_specs.iter().any(|existing| {
                                existing.import == parsed.import
                                    && existing.renames == parsed.renames
                            }) {
                                continue;
                            }
                            self.macro_specs.push(parsed);
                            continue;
                        }
                        if let Some(alias) = &parsed.import.alias {
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
                                    && existing.macro_refers == parsed.macro_refers
                                    && existing.include_macros == parsed.include_macros
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

fn valid_namespace(name: &str) -> bool {
    name != "js"
        && name.split('.').all(|part| {
            let mut bytes = part.bytes();
            matches!(bytes.next(), Some(b'a'..=b'z' | b'A'..=b'Z' | b'_'))
                && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        })
}

fn locate_source(
    roots: &[PathBuf],
    name: &str,
    extensions: &[&str],
) -> Result<Option<PathBuf>, NamespaceCompileError> {
    let relative = name.replace('.', "/").replace('-', "_");
    for extension in extensions {
        let mut candidates = Vec::new();
        for root in roots {
            let path = root.join(format!("{relative}.{extension}"));
            match fs::metadata(&path) {
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
struct LegacyMacroSourceProvider {
    roots: Vec<PathBuf>,
}
impl SourceProvider for LegacyMacroSourceProvider {
    fn load(&self, request: &LoadRequest) -> Result<Option<SourceInput>, LoadError> {
        let name = String::from_utf16(request.namespace.name())
            .map_err(|e| LoadError::Provider(e.to_string()))?;
        if request.namespace.namespace().is_some() || !valid_namespace(&name) {
            return Err(LoadError::InvalidLibspec(name));
        }
        let Some(path) = locate_source(&self.roots, &name, &["clj", "cljc"])
            .map_err(|e| LoadError::Provider(e.to_string()))?
        else {
            return Ok(None);
        };
        let source = fs::read_to_string(&path).map_err(|e| LoadError::Provider(e.to_string()))?;
        Ok(Some(SourceInput::new(path.to_string_lossy(), source)))
    }
}

#[derive(Clone, Copy)]
enum LoadState {
    Unseen,
    Loading,
    Loaded,
}

struct Loader {
    roots: Vec<PathBuf>,
    states: HashMap<String, LoadState>,
    stack: Vec<String>,
    ordered: Vec<NamespaceSource>,
    warning_mode: bool,
}

impl Loader {
    fn locate(&self, name: &str) -> Result<PathBuf, NamespaceCompileError> {
        locate_source(&self.roots, name, &["cljs", "cljc"])?.ok_or_else(|| {
            NamespaceCompileError::new(format!(
                "namespace {name} not found: expected {}.cljs or .cljc in explicit source roots",
                name.replace('.', "/").replace('-', "_")
            ))
        })
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
        let path = self.locate(name)?;
        let source = fs::read_to_string(&path).map_err(|error| {
            NamespaceCompileError::new(format!("cannot read {}: {error}", path.display()))
        })?;
        let mut syntax = parse(&source).map_err(|error| {
            NamespaceCompileError::new(error.to_string()).at(&path, &source, error.span)
        })?;
        let cljc = path.extension().is_some_and(|ext| ext == "cljc");
        if cljc {
            crate::compiler_reader::select_cljs(&source, &mut syntax, &mut vec![]).map_err(
                |error| {
                    NamespaceCompileError::new(error.to_string()).at(&path, &source, error.span)
                },
            )?;
        }
        let mut file = NamespaceSource {
            name: name.to_owned(),
            name_span: 0..0,
            path,
            source,
            syntax,
            aliases: Vec::new(),
            requires: Vec::new(),
            require_spans: Vec::new(),
            runtime_specs: Vec::new(),
            macro_specs: Vec::new(),
            namespace_imports: Vec::new(),
            warning_mode: self.warning_mode,
            resolution: crate::analyzer::NamespaceResolution {
                cljc,
                ..Default::default()
            },
        };
        file.read_header()?;
        // Closure libraries are JavaScript host namespaces. A source namespace
        // provided explicitly by the caller still takes precedence.
        let mut host_namespaces = Vec::new();
        for spec in file.runtime_specs.clone() {
            let namespace = &spec.import.namespace;
            if namespace.starts_with("goog.")
                && locate_source(&self.roots, namespace, &["cljs", "cljc"])?.is_none()
            {
                let node = spec.alias_node.unwrap_or(file.syntax.roots[0]);
                if spec.include_macros {
                    return Err(file.error(node, "Closure host namespaces do not provide macros"));
                }
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
        for dependency in &mut file.requires {
            if let Some(suffix) = dependency.strip_prefix("clojure.")
                && dependency != "clojure.core"
                && locate_source(&self.roots, dependency, &["cljs", "cljc"])?.is_none()
            {
                let rewritten = format!("cljs.{suffix}");
                if locate_source(&self.roots, &rewritten, &["cljs", "cljc"])?.is_some() {
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
                    for spec in &mut file.macro_specs {
                        if spec.import.namespace == *dependency {
                            spec.import.namespace = rewritten.clone();
                        }
                    }
                    *dependency = rewritten;
                }
            }
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
        self.ordered.push(file);
        Ok(())
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

/// Reuse an explicit macro host. Sources are loaded once per host; callers use
/// `MacroHost::load_namespace` with `Reload` to invalidate macro namespace code.
/// This installs a filesystem source provider restricted to the supplied roots.
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
    let mut loader = Loader {
        roots: Vec::new(),
        states: HashMap::new(),
        stack: Vec::new(),
        ordered: Vec::new(),
        warning_mode: options.warning_mode,
    };
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
        if !loader.roots.contains(&canonical) {
            loader.roots.push(canonical);
        }
    }
    loader.visit(entry)?;
    if options.warning_mode {
        for (source_index, file) in loader.ordered.iter().enumerate() {
            if !file.name.contains('.') {
                warnings.push(AnalysisWarning {
                    kind: "single-segment-namespace",
                    message: format!("{} is a single segment namespace", file.name),
                    span: file.name_span.clone(),
                    source_index,
                });
            }
            for spec in file.runtime_specs.iter().chain(&file.macro_specs) {
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
    if loader.ordered.iter().any(|file| {
        !file.macro_specs.is_empty() || file.runtime_specs.iter().any(|spec| spec.include_macros)
    }) {
        macros
            .context_mut()
            .set_source_provider(LegacyMacroSourceProvider {
                roots: loader.roots.clone(),
            });
    }
    let inputs = loader
        .ordered
        .iter()
        .map(|file| NamespaceInput {
            name: &file.name,
            source: &file.source,
            syntax: &file.syntax,
            aliases: &file.aliases,
            requires: &file.requires,
        })
        .collect::<Vec<_>>();
    let program = analyze_namespaces_prepared_with_warnings(
        &inputs,
        macros,
        options,
        warnings,
        |macros, index| {
            let file = &loader.ordered[index];
            macros.set_namespace_imports(&file.name, file.namespace_imports.clone());
            let error = |span: Range<usize>, message: String| AnalysisError {
                kind: crate::AnalysisErrorKind::MacroExpansion(message),
                span,
            };
            let header_span = file.syntax.nodes[file.syntax.roots[0].0].span.clone();
            let mut specs = file.macro_specs.clone();
            // Inference follows the dependency's self-required macro namespace.
            for spec in &file.runtime_specs {
                if spec.include_macros
                    || loader.ordered.iter().any(|dep| {
                        dep.name == spec.import.namespace
                            && dep
                                .macro_specs
                                .iter()
                                .any(|m| m.import.namespace == dep.name)
                    })
                {
                    let mut inferred = spec.clone();
                    inferred.import.refers = spec.macro_refers.clone();
                    inferred.renames.clear();
                    specs.push(inferred);
                }
            }
            let mut imports = Vec::new();
            let mut renamed_macros = Vec::new();
            let mut macro_names = std::collections::HashSet::new();
            for spec in &specs {
                macros
                    .load_namespace(&spec.import.namespace, Reload::None)
                    .map_err(|message| {
                        error(
                            spec.span.clone(),
                            format!("macro namespace {}: {message}", spec.import.namespace),
                        )
                    })?;
                let mut import = spec.import.clone();
                for original in &spec.import.refers {
                    let local = spec
                        .renames
                        .iter()
                        .find(|(from, _)| from == original)
                        .map(|(_, to)| to)
                        .unwrap_or(original);
                    if !macro_names.insert(local.clone()) {
                        return Err(error(
                            spec.span.clone(),
                            format!("duplicate macro refer: {local}"),
                        ));
                    }
                    if macros
                        .find_macro(&import.namespace, original)
                        .map_err(|m| error(spec.span.clone(), m))?
                        .is_none()
                        && !crate::compiler_names::is_definition_macro(&import.namespace, original)
                    {
                        return Err(error(
                            spec.span.clone(),
                            format!(
                                "macro import {}/{original} does not name a public macro",
                                import.namespace
                            ),
                        ));
                    }
                    if local != original {
                        import.refers.retain(|n| n != original);
                        renamed_macros.push((
                            local.clone(),
                            import.namespace.clone(),
                            original.clone(),
                        ));
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
                    if resolution.refers.iter().any(|(name, _, _)| name == local) {
                        return Err(error(
                            spec.span.clone(),
                            format!("duplicate or conflicting refer: {local}"),
                        ));
                    }
                    resolution.refers.push((
                        local.clone(),
                        spec.import.namespace.clone(),
                        original.clone(),
                    ));
                }
            }
            for (local, namespace, original) in &resolution.refers {
                if (matches!(namespace.as_str(), "cljs.core" | "clojure.core")
                    || imports.iter().any(|i| &i.namespace == namespace))
                    && macros
                        .find_macro(namespace, original)
                        .map_err(|m| error(header_span.clone(), m))?
                        .is_some()
                {
                    if !macro_names.insert(local.clone()) {
                        return Err(error(
                            header_span.clone(),
                            format!("duplicate macro refer: {local}"),
                        ));
                    }
                    renamed_macros.push((local.clone(), namespace.clone(), original.clone()));
                }
            }
            macros
                .set_imports(&file.name, imports)
                .map_err(|m| error(header_span.clone(), m))?;
            macros.set_renames(&file.name, renamed_macros);
            let NodeKind::Collection { children, .. } =
                &file.syntax.nodes[file.syntax.roots[0].0].kind
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
                let source =
                    crate::compiler_data::Source::new(file.path.to_string_lossy(), &file.source);
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
        },
    )
    .map_err(|(index, error)| {
        let file = &loader.ordered[index];
        NamespaceCompileError::new(error.to_string()).at(&file.path, &file.source, error.span)
    })?;
    Ok(emit_js(&program))
}
