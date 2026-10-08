//! Small end-to-end driver. Analysis and emission remain independently usable.

use std::{
    collections::HashMap,
    fmt, fs,
    ops::Range,
    path::{Path, PathBuf},
};

use crate::analyzer::{NamespaceInput, analyze_namespaces};
use crate::{
    AnalysisError, CollectionKind, LineIndex, NodeId, NodeKind, ParseError, SyntaxTree, analyze,
    emit_js, parse,
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

struct NamespaceSource {
    name: String,
    path: PathBuf,
    source: String,
    syntax: SyntaxTree,
    aliases: Vec<(String, String)>,
    requires: Vec<String>,
    require_spans: Vec<Range<usize>>,
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

    fn read_header(&mut self) -> Result<(), NamespaceCompileError> {
        let Some(&root) = self.syntax.roots.first() else {
            return Err(NamespaceCompileError::new(
                "namespace source must begin with (ns NAME ...)",
            )
            .at(&self.path, &self.source, 0..0));
        };
        let forms = self
            .collection(root, CollectionKind::List)
            .filter(|forms| forms.first().and_then(|&id| self.symbol(id)) == Some("ns"))
            .ok_or_else(|| self.error(root, "namespace source must begin with (ns NAME ...)"))?
            .to_vec();
        let declared = forms
            .get(1)
            .copied()
            .ok_or_else(|| self.error(root, "ns requires a namespace name"))?;
        let name = self.symbol(declared).filter(|name| valid_namespace(name))
            .ok_or_else(|| self.error(declared, "namespace must use dot-separated ASCII names (letters/underscore, then letters/digits/underscore/hyphen); js is reserved"))?;
        if name != self.name {
            return Err(self.error(
                declared,
                format!(
                    "declared namespace {name} does not match requested namespace {}",
                    self.name
                ),
            ));
        }
        let mut alias_nodes = Vec::new();
        let mut saw_require = false;
        for clause in forms.into_iter().skip(2) {
            let parts = self
                .collection(clause, CollectionKind::List)
                .ok_or_else(|| {
                    self.error(
                        clause,
                        "unsupported namespace clause; only (:require ...) is supported",
                    )
                })?
                .to_vec();
            if parts.first().map(|&id| self.text(id)) != Some(":require") {
                return Err(self.error(
                    clause,
                    "unsupported namespace clause; only (:require ...) is supported",
                ));
            }
            if saw_require {
                return Err(self.error(clause, "only one :require clause is allowed per namespace"));
            }
            saw_require = true;
            for spec in parts.into_iter().skip(1) {
                let items = if self.symbol(spec).is_some() {
                    vec![spec]
                } else {
                    self.collection(spec, CollectionKind::Vector)
                        .ok_or_else(|| self.error(spec, "require expects a namespace symbol or [namespace :as alias]; JavaScript package imports are unsupported"))?
                        .to_vec()
                };
                let dependency = items
                    .first()
                    .copied()
                    .ok_or_else(|| self.error(spec, "require specification cannot be empty"))?;
                let name = self.symbol(dependency).filter(|name| valid_namespace(name))
                    .ok_or_else(|| self.error(dependency, "require library must be a supported namespace symbol; JavaScript package imports are unsupported"))?
                    .to_owned();
                if items.len() != 1 {
                    if items.len() != 3 || self.text(items[1]) != ":as" {
                        return Err(self.error(spec, "unsupported require options; only one :as alias is supported (:refer, :rename, and macro imports are unsupported)"));
                    }
                    let alias_node = items[2];
                    let alias = self
                        .symbol(alias_node)
                        .filter(|name| valid_namespace(name))
                        .ok_or_else(|| {
                            self.error(
                                alias_node,
                                ":as alias must be a supported namespace symbol; js is reserved",
                            )
                        })?
                        .to_owned();
                    if self.aliases.iter().any(|(existing, _)| existing == &alias) {
                        return Err(self.error(
                            alias_node,
                            format!("duplicate or conflicting namespace alias: {alias}"),
                        ));
                    }
                    self.aliases.push((alias, name.clone()));
                    alias_nodes.push(alias_node);
                }
                self.requires.push(name);
                self.require_spans
                    .push(self.syntax.nodes[dependency.0].span.clone());
            }
        }
        // Full namespace spellings remain usable even when an alias is provided.
        // Reject an alias that would silently redirect one of those spellings.
        for ((alias, target), node) in self.aliases.iter().zip(alias_nodes) {
            if alias != target && (alias == &self.name || self.requires.contains(alias)) {
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
}

impl Loader {
    fn locate(&self, name: &str) -> Result<PathBuf, NamespaceCompileError> {
        let relative = format!("{}.cljs", name.replace('.', "/").replace('-', "_"));
        let mut matches = Vec::new();
        for root in &self.roots {
            let path = root.join(&relative);
            match fs::metadata(&path) {
                Ok(metadata) if metadata.is_file() => {
                    let canonical = fs::canonicalize(&path).map_err(|error| {
                        NamespaceCompileError::new(format!(
                            "cannot resolve {}: {error}",
                            path.display()
                        ))
                    })?;
                    if !matches.contains(&canonical) {
                        matches.push(canonical);
                    }
                }
                Ok(_) => {
                    return Err(NamespaceCompileError::new(format!(
                        "namespace source is not a file: {}",
                        path.display()
                    )));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(NamespaceCompileError::new(format!(
                        "cannot inspect {}: {error}",
                        path.display()
                    )));
                }
            }
        }
        match matches.len() {
            0 => Err(NamespaceCompileError::new(format!(
                "namespace {name} not found: expected {relative} in source roots {}",
                self.roots
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))),
            1 => Ok(matches.remove(0)),
            _ => Err(NamespaceCompileError::new(format!(
                "ambiguous namespace {name}: {}",
                matches
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))),
        }
    }

    fn visit(&mut self, name: &str) -> Result<(), NamespaceCompileError> {
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
        let syntax = parse(&source).map_err(|error| {
            NamespaceCompileError::new(error.to_string()).at(&path, &source, error.span)
        })?;
        let mut file = NamespaceSource {
            name: name.to_owned(),
            path,
            source,
            syntax,
            aliases: Vec::new(),
            requires: Vec::new(),
            require_spans: Vec::new(),
        };
        file.read_header()?;
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
/// Namespace dots become directories, hyphens become underscores, and the only
/// supported source extension is `.cljs`. Distinct matching files are ambiguous.
pub fn compile_namespace(
    entry: &str,
    source_roots: &[PathBuf],
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
    let program = analyze_namespaces(&inputs).map_err(|(index, error)| {
        let file = &loader.ordered[index];
        NamespaceCompileError::new(error.to_string()).at(&file.path, &file.source, error.span)
    })?;
    Ok(emit_js(&program))
}
