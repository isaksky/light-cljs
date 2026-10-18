//! The compile-time services behind the bundled `light.build` namespace
//! (docs/compiler.md, "Compile-time services: light.build"): reading a
//! resource and declaring a dependency, from macros, the helper code they
//! run and custom data readers. Reading and recording are one operation:
//! the dependency manifest, when one is recorded, gets the exact text
//! returned, absence and every location tried ([`crate::dependency_manifest`]).
//!
//! Both compile-time engines run the same JavaScript object, `lightCljsBuild`
//! ([`BUILD_OBJECT`]), over one string service, so V8 and QuickJS behave
//! identically. A request is `CALLER \0 UNITS \0 OPERATION \0 FIRST \0
//! SECOND`, and an answer is `v` and a text, `n` for nil, or `e` and an error
//! message, which the object throws.

use crate::dependency_manifest::{
    BroadView, EdgeKinds, ExternalInput, InputRecord, ObservationLog, ResolvedResource,
    ResourceDependency,
};
use crate::source_provider::{LoadError, ResourceProvider};
use std::{cell::RefCell, rc::Rc};

/// The compilation `light.build` serves: set while [`crate::compiler`]
/// analyzes namespaces and for the life of an interactive compiler
/// ([`crate::eval_compiler`]), `None` otherwise.
pub(crate) struct BuildContext {
    resources: Option<Rc<dyn ResourceProvider>>,
    /// What `light.build/config` answers.
    configuration: Rc<BuildConfiguration>,
    /// Each source by source index, as analysis reaches it.
    sources: Vec<Option<Site>>,
    /// The source being analyzed.
    window: usize,
}

/// A source analysis reached: its namespace and the file its provider
/// named, against whose directory `./` resolves.
#[derive(Clone, PartialEq)]
struct Site {
    namespace: String,
    file: Option<String>,
}

pub(crate) type SharedBuild = Rc<RefCell<Option<BuildContext>>>;
pub(crate) type SharedLog = Rc<RefCell<Option<ObservationLog>>>;

/// Serves one compilation while it lives, then restores the context it
/// replaced: a compilation nested in another serves only while it runs.
/// Guards on one [`SharedBuild`] drop in the reverse order they entered.
pub(crate) struct BuildGuard {
    build: SharedBuild,
    previous: Option<BuildContext>,
}

impl BuildGuard {
    pub(crate) fn enter(
        build: &SharedBuild,
        resources: Option<Rc<dyn ResourceProvider>>,
        configuration: Rc<BuildConfiguration>,
    ) -> Self {
        let previous = build.borrow_mut().replace(BuildContext {
            resources,
            configuration,
            sources: Vec::new(),
            window: 0,
        });
        Self {
            build: build.clone(),
            previous,
        }
    }
    /// Serve `resources` from now on.
    pub(crate) fn provide(&self, resources: Option<Rc<dyn ResourceProvider>>) {
        if let Some(context) = self.build.borrow_mut().as_mut() {
            context.resources = resources;
        }
    }
    /// Source `index`, namespace `name` from the provider's `file`, is
    /// analyzed next: the expansion site of what runs until the next call,
    /// and the namespace on which the units of that source record what they
    /// read. An interactive compiler analyzes every form as source 0, in its
    /// current namespace, from the form's file.
    pub(crate) fn analyze(&self, index: usize, name: &str, file: Option<&str>) {
        if let Some(context) = self.build.borrow_mut().as_mut() {
            context.window = index;
            if context.sources.len() <= index {
                context.sources.resize(index + 1, None);
            }
            let slot = &mut context.sources[index];
            if slot
                .as_ref()
                .is_none_or(|site| site.namespace != name || site.file.as_deref() != file)
            {
                *slot = Some(Site {
                    namespace: name.to_owned(),
                    file: file.map(str::to_owned),
                });
            }
        }
    }
}

impl Drop for BuildGuard {
    fn drop(&mut self) {
        *self.build.borrow_mut() = self.previous.take();
    }
}

/// Who called a service.
pub(crate) struct Caller {
    /// A deferred unit's initializer is the innermost code running, which
    /// has no expansion site to resolve `./` against.
    pub initializing: bool,
    /// The sources of the deferred units being initialized or analyzed.
    pub units: Vec<u32>,
    /// The source of the innermost unit being analyzed, whose macro
    /// expansion is the site when no initializer is innermost.
    pub analyzing: Option<u32>,
    /// A custom data reader, recorded on the source being read.
    pub reader: bool,
}

/// The `lightCljsBuild` object, a JavaScript function of the string service
/// and of a function that describes the caller (`CALLER \0 UNITS`).
pub(crate) const BUILD_OBJECT: &str = include_str!("build_services.js");

/// The caller part of a request from a data reader's heap.
pub(crate) const READER_CALLER: &str = "() => 'r\\u0000'";

/// Parse and serve one request. `units` maps the deferred units a request
/// names to their sources, and `analysis` gives the sources of the units
/// being analyzed, innermost last.
pub(crate) fn serve_request(
    build: &SharedBuild,
    log: &SharedLog,
    request: &str,
    units: impl Fn(usize) -> Option<u32>,
    analysis: Vec<u32>,
) -> String {
    let mut parts = request.splitn(5, '\0');
    let (Some(caller), Some(initializing), Some(operation), Some(first), Some(second)) = (
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
    ) else {
        return "elight.build: malformed service request".into();
    };
    let mut sources: Vec<u32> = initializing
        .split(',')
        .filter_map(|unit| unit.parse::<usize>().ok())
        .filter_map(&units)
        .collect();
    let analyzing = analysis.last().copied();
    sources.extend(analysis);
    let caller = Caller {
        initializing: caller == "i",
        units: sources,
        analyzing,
        reader: caller == "r",
    };
    match serve(build, log, &caller, operation, first, second) {
        Ok(Some(text)) => format!("v{text}"),
        Ok(None) => "n".into(),
        Err(message) => format!("e{message}"),
    }
}

fn public_name(operation: &str) -> &str {
    match operation {
        "depend-resource" | "depend-namespace" | "depend-provider" => "depend!",
        "depend-on" => "depend-on!",
        other => other,
    }
}

fn serve(
    build: &SharedBuild,
    log: &SharedLog,
    caller: &Caller,
    operation: &str,
    first: &str,
    second: &str,
) -> Result<Option<String>, String> {
    let name = format!("light.build/{}", public_name(operation));
    let (resources, configuration, own, site) = {
        let context = build.borrow();
        let Some(context) = context.as_ref() else {
            return Err(format!(
                "{name} is only available at compile time, while namespaces compile: \
                 in macros, the helper code they run and custom data readers"
            ));
        };
        let site = if caller.initializing {
            None
        } else {
            let source = caller.analyzing.map_or(context.window, |s| s as usize);
            context.sources.get(source).cloned().flatten()
        };
        (
            context.resources.clone(),
            context.configuration.clone(),
            context.window as u32,
            site,
        )
    };
    let mut helpers = caller.units.clone();
    if caller.reader {
        helpers.push(own);
    }
    helpers.sort_unstable();
    helpers.dedup();
    let record = |input: ExternalInput| {
        if let Some(log) = log.borrow_mut().as_mut() {
            log.inputs.push(InputRecord {
                own,
                helpers: helpers.clone(),
                input,
            });
        }
    };
    match operation {
        "resource" | "resource?" | "depend-resource" => {
            let directory = || {
                let site = site.as_ref()?;
                Some(site_directory(site, resources.as_deref()))
            };
            let path = resolve_path(first, directory).map_err(|e| format!("{name}: {e}"))?;
            let Some(resources) = resources else {
                // A macro can catch the error, and a later provider may
                // supply the resource: the lookup is non-cacheable.
                record(ExternalInput::Resource(ResourceDependency {
                    path,
                    candidates: Vec::new(),
                    found: None,
                    stable: false,
                }));
                return Err(format!(
                    "{name}: the source provider of this compilation supplies no resources"
                ));
            };
            let mut tried = Vec::new();
            let answer = match resources.load_resource(&path, &mut tried) {
                Ok(answer) => answer,
                Err(error) => {
                    // A macro can catch the error, so the lookup is still a
                    // dependency: an ambiguity is keyed by its candidates,
                    // which change when a copy goes, and any other provider
                    // failure cannot be keyed at all.
                    let ambiguous = matches!(error, LoadError::AmbiguousSources(_));
                    record(ExternalInput::Resource(ResourceDependency {
                        path: path.clone(),
                        candidates: tried,
                        found: None,
                        stable: ambiguous && resources.stable(),
                    }));
                    return Err(match error {
                        LoadError::AmbiguousSources(files) => format!(
                            "{name}: ambiguous resource {path}: {}",
                            files
                                .iter()
                                .map(|file| file.display().to_string())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        LoadError::Provider(message) => format!("{name}: {path}: {message}"),
                        other => format!("{name}: {path}: {other}"),
                    });
                }
            };
            let stable =
                resources.stable() && answer.as_ref().is_none_or(|found| found.identity.is_some());
            let tried_text = (operation == "resource" && answer.is_none()).then(|| {
                tried
                    .iter()
                    .map(|candidate| candidate.path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            });
            record(ExternalInput::Resource(ResourceDependency {
                path: path.clone(),
                candidates: tried,
                found: answer.as_ref().map(|found| ResolvedResource {
                    identity: found.identity.clone(),
                    sha256: crate::sha256::digest(found.text.as_bytes()),
                    bytes: found.text.len() as u64,
                }),
                stable,
            }));
            match (operation, answer) {
                ("depend-resource", _) => Ok(None),
                (_, Some(found)) => Ok(Some(found.text)),
                ("resource?", None) => Ok(None),
                (_, None) => Err(format!(
                    "{name}: resource {path} not found in any source root (tried {})",
                    tried_text.unwrap_or_default()
                )),
            }
        }
        "depend-namespace" => {
            if !crate::compiler::valid_namespace(first) {
                return Err(format!("{name}: invalid namespace {first}"));
            }
            record(ExternalInput::Namespace(first.to_owned()));
            if let Some(log) = log.borrow_mut().as_mut() {
                log.named(first, EdgeKinds::OBSERVED);
            }
            Ok(None)
        }
        "depend-provider" => {
            if first.is_empty() || second.is_empty() {
                return Err(format!(
                    "{name}: a provider dependency needs a token and a nonempty fingerprint"
                ));
            }
            record(ExternalInput::Provider {
                token: first.to_owned(),
                fingerprint: second.to_owned(),
            });
            Ok(None)
        }
        // Every namespace depends on the whole configuration's
        // fingerprint already ([`ObservationLog::configuration`]), so a read
        // records nothing more.
        "config" => Ok(configuration.get(first).map(str::to_owned)),
        "depend-on" => {
            let view = match first {
                "namespaces" => BroadView::Namespaces,
                "declarations" => BroadView::Declarations,
                "config" => BroadView::Config,
                other => {
                    return Err(format!(
                        "{name}: unknown view {other}; expected :namespaces, :declarations or :config"
                    ));
                }
            };
            record(ExternalInput::View(view));
            Ok(None)
        }
        _ => Err(format!("light.build: unknown service {operation}")),
    }
}

/// The logical directory of `site`'s file, as segments: where its provider
/// says the file is under the roots ([`ResourceProvider::directory`]), or,
/// for a file it cannot place (REPL input, a file outside every root), the
/// directory the namespace's name maps to.
fn site_directory(site: &Site, resources: Option<&dyn ResourceProvider>) -> Vec<String> {
    if let (Some(file), Some(resources)) = (&site.file, resources)
        && let Some(directory) = resources.directory(file)
    {
        return directory
            .split('/')
            .filter(|segment| !segment.is_empty())
            .map(str::to_owned)
            .collect();
    }
    let mut segments: Vec<String> = site
        .namespace
        .replace('-', "_")
        .split('.')
        .map(str::to_owned)
        .collect();
    segments.pop();
    segments
}

/// The logical path `path` names: root-relative, or, when it starts with
/// `./` or `../`, relative to the expansion site's `directory`, which is
/// None when there is no site.
fn resolve_path(
    path: &str,
    directory: impl FnOnce() -> Option<Vec<String>>,
) -> Result<String, String> {
    if path.is_empty() || path.contains(['\\', '\0']) {
        return Err(format!("invalid resource path {path:?}"));
    }
    if path.starts_with('/') {
        return Err(format!(
            "resource path {path} is absolute; paths are relative to the source roots"
        ));
    }
    let mut segments: Vec<String> = Vec::new();
    if path.starts_with("./") || path.starts_with("../") {
        let Some(directory) = directory() else {
            return Err(format!(
                "relative resource path {path} has no expansion site: code that runs \
                 outside any macro expansion, such as a def's initializer, must use a \
                 path relative to the source roots"
            ));
        };
        segments = directory;
    }
    for segment in path.split('/') {
        match segment {
            "" => return Err(format!("invalid resource path {path}: empty segment")),
            "." => {}
            ".." => {
                if segments.pop().is_none() {
                    return Err(format!("resource path {path} leaves the source roots"));
                }
            }
            segment => segments.push(segment.to_owned()),
        }
    }
    if segments.is_empty() {
        return Err(format!("resource path {path} names no file"));
    }
    Ok(segments.join("/"))
}

/// The build configuration `light.build/config` answers: each `--config
/// KEY=EDN` of a build, or what an embedder inserts. A key is a keyword
/// written without its colon (`app/flavor`), and a value is EDN data read
/// with the compiler's reader: keywords, numbers, strings, booleans, nil and
/// lists, vectors, maps and sets of them. Values are kept as canonical text,
/// with single spaces between the elements of collections, which macros
/// read back as data.
///
/// The first implementation keys every namespace of a build on
/// [`Self::fingerprint`], the hash of the whole map, whether or not it reads
/// a key (docs/compiler-cache.md, "Explicit dependency APIs").
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BuildConfiguration {
    entries: std::collections::BTreeMap<String, String>,
}

impl BuildConfiguration {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add `key` (a keyword without its colon) with the EDN text `value`. A
    /// malformed key or value, or a key already present, is an error.
    pub fn insert(&mut self, key: &str, value: &str) -> Result<(), String> {
        let key = configuration_key(key)?;
        if self.entries.contains_key(&key) {
            return Err(format!("the key {key} is given twice"));
        }
        let value = configuration_value(value)?;
        self.entries.insert(key, value);
        Ok(())
    }

    /// Add one `KEY=EDN` text, as `--config` gives it: KEY is everything
    /// before the first `=`.
    pub fn insert_text(&mut self, text: &str) -> Result<(), String> {
        let (key, value) = text
            .split_once('=')
            .ok_or("expected KEY=EDN, such as app/flavor=:dev")?;
        self.insert(key, value)
    }

    /// The canonical EDN text of `key`'s value, or `None` when unset.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The keys, ascending, with their canonical EDN text.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> + '_ {
        self.entries
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
    }

    /// The whole map as canonical EDN text, keys ascending: `{}` when empty.
    pub fn text(&self) -> String {
        let mut text = String::from("{");
        for (index, (key, value)) in self.entries.iter().enumerate() {
            if index > 0 {
                text.push(' ');
            }
            text.push(':');
            text.push_str(key);
            text.push(' ');
            text.push_str(value);
        }
        text.push('}');
        text
    }

    /// The SHA-256 of [`Self::text`], which every namespace of a build
    /// depends on: any change to any key changes it, and the empty map has
    /// its own.
    pub fn fingerprint(&self) -> [u8; 32] {
        crate::sha256::digest(self.text().as_bytes())
    }
}

/// `key` as the name of a keyword: the text after its colon.
fn configuration_key(key: &str) -> Result<String, String> {
    if key.starts_with(':') {
        return Err(format!(
            "the key {key} is written without its colon, as {}",
            key.trim_start_matches(':')
        ));
    }
    let text = format!(":{key}");
    let tree = crate::parser::parse(&text).ok();
    let single = tree.as_ref().and_then(|tree| match &tree.roots[..] {
        [root] => Some(&tree.nodes[root.0]),
        _ => None,
    });
    match single {
        Some(node)
            if matches!(
                node.kind,
                crate::parser::NodeKind::Keyword {
                    auto_resolve: false
                }
            ) && node.span == (0..text.len()) =>
        {
            Ok(key.to_owned())
        }
        _ => Err(format!(
            "invalid key {key:?}: expected a keyword name without its colon, such as app/flavor"
        )),
    }
}

/// The canonical text of the EDN value `value`.
fn configuration_value(value: &str) -> Result<String, String> {
    let tree = crate::parser::parse(value)
        .map_err(|error| format!("cannot read the value {value:?}: {error}"))?;
    let [root] = tree.roots[..] else {
        return Err(if tree.roots.is_empty() {
            "the value is empty: write nil for no value".into()
        } else {
            format!("the value {value:?} holds more than one form")
        });
    };
    let mut text = String::new();
    canonical_value(&tree, value, root, &mut text)?;
    Ok(text)
}

/// Append the canonical text of node `id` to `out`, and return its value's
/// identity as the EDN reader compares values: the canonical text of a
/// scalar, lists and vectors alike, and maps and sets in sorted order.
/// Scalars are canonical, so equal values spelled differently (`1` and
/// `1.0`, `"a"` and `"a"`) have one text.
fn canonical_value(
    tree: &crate::parser::SyntaxTree,
    source: &str,
    id: crate::parser::NodeId,
    out: &mut String,
) -> Result<String, String> {
    use crate::parser::{CollectionKind, NodeKind};
    let node = &tree.nodes[id.0];
    let spelled = &source[node.span.clone()];
    let start = out.len();
    match &node.kind {
        NodeKind::Nil | NodeKind::Boolean(_) => out.push_str(spelled),
        NodeKind::Keyword {
            auto_resolve: false,
        } => out.push_str(spelled),
        NodeKind::Number(_) => out.push_str(&canonical_number(spelled)?),
        NodeKind::String(value) => canonical_string(&value.to_utf16(source), out),
        NodeKind::Collection { kind, children } if *kind != CollectionKind::AnonymousFn => {
            let (open, close) = match kind {
                CollectionKind::List => ("(", ")"),
                CollectionKind::Vector => ("[", "]"),
                CollectionKind::Map => ("{", "}"),
                _ => ("#{", "}"),
            };
            out.push_str(open);
            let mut identities = Vec::with_capacity(children.len());
            for (index, &child) in children.iter().enumerate() {
                if index > 0 {
                    out.push(' ');
                }
                identities.push(canonical_value(tree, source, child, out)?);
            }
            out.push_str(close);
            // Map keys and set elements are distinct, as their reader requires.
            let identity = match kind {
                CollectionKind::Map => {
                    let mut entries: Vec<(String, String)> = Vec::new();
                    let mut pairs = identities.into_iter();
                    while let (Some(key), Some(value)) = (pairs.next(), pairs.next()) {
                        entries.push((key, value));
                    }
                    entries.sort();
                    if let Some(pair) = entries.windows(2).find(|pair| pair[0].0 == pair[1].0) {
                        return Err(repeated(source, "key", &pair[0].0));
                    }
                    let entries: Vec<String> = entries
                        .into_iter()
                        .map(|(k, v)| format!("{k} {v}"))
                        .collect();
                    format!("{{{}}}", entries.join(" "))
                }
                CollectionKind::Set => {
                    identities.sort();
                    if let Some(pair) = identities.windows(2).find(|pair| pair[0] == pair[1]) {
                        return Err(repeated(source, "element", &pair[0]));
                    }
                    format!("#{{{}}}", identities.join(" "))
                }
                _ => format!("[{}]", identities.join(" ")),
            };
            return Ok(identity);
        }
        _ => {
            return Err(format!(
                "{spelled:?} is not a configuration value: use keywords, numbers, strings, \
                 booleans, nil and lists, vectors, maps and sets of them"
            ));
        }
    }
    Ok(out[start..].to_owned())
}

fn repeated(source: &str, what: &str, identity: &str) -> String {
    format!(
        "the value {:?} repeats the {what} {identity}",
        source.trim()
    )
}

/// A decimal integer or floating-point number, which the EDN reader reads
/// as a JavaScript number, printed as that number: integral values without
/// a fraction, others as Rust's shortest round-trip decimal.
fn canonical_number(spelled: &str) -> Result<String, String> {
    let bytes = spelled.as_bytes();
    let digits = |from: usize| {
        bytes[from..]
            .iter()
            .position(|b| !b.is_ascii_digit())
            .map_or(bytes.len(), |n| from + n)
    };
    let mut at = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let integer = digits(at);
    let valid_integer = integer > at && (bytes[at] != b'0' || integer == at + 1);
    at = integer;
    if bytes.get(at) == Some(&b'.') {
        at = digits(at + 1);
    }
    if matches!(bytes.get(at), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(bytes.get(at + 1), Some(b'+' | b'-')));
        let end = digits(at + 1 + sign);
        at = if end > at + 1 + sign { end } else { 0 };
    }
    let number = (valid_integer && at == bytes.len())
        .then(|| spelled.parse::<f64>().ok())
        .flatten()
        .filter(|number| number.is_finite())
        .ok_or_else(|| {
            format!(
                "{spelled:?} is not a configuration number: write a decimal integer or \
                 floating-point number, such as 3, -2.5 or 1e3"
            )
        })?;
    // -0 equals 0.
    let number = if number == 0.0 { 0.0 } else { number };
    Ok(if number.fract() == 0.0 && number.abs() < 1e21 {
        format!("{number:.0}")
    } else {
        format!("{number}")
    })
}

/// `units` as an EDN string: quotes, backslashes and control characters
/// escaped, and a lone surrogate as `\uXXXX`.
fn canonical_string(units: &[u16], out: &mut String) {
    out.push('"');
    for unit in char::decode_utf16(units.iter().copied()) {
        match unit {
            Ok('"') => out.push_str("\\\""),
            Ok('\\') => out.push_str("\\\\"),
            Ok('\n') => out.push_str("\\n"),
            Ok('\t') => out.push_str("\\t"),
            Ok('\r') => out.push_str("\\r"),
            Ok(c) if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            Ok(c) => out.push(c),
            Err(lone) => out.push_str(&format!("\\u{:04x}", lone.unpaired_surrogate())),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dependency_manifest::Candidate;
    use crate::source_provider::ResourceInput;

    /// An embedding provider that cannot say what a resource is.
    struct Anonymous;
    impl ResourceProvider for Anonymous {
        fn load_resource(
            &self,
            path: &str,
            _: &mut Vec<Candidate>,
        ) -> Result<Option<ResourceInput>, LoadError> {
            Ok((path == "x.txt").then(|| ResourceInput {
                file: "memory:x.txt".into(),
                identity: None,
                text: "text".into(),
            }))
        }
    }

    #[test]
    fn a_provider_without_stable_identities_makes_its_reads_non_cacheable() {
        let build: SharedBuild = Default::default();
        let log: SharedLog = Rc::new(RefCell::new(Some(ObservationLog::default())));
        let guard = BuildGuard::enter(&build, Some(Rc::new(Anonymous)), Default::default());
        guard.analyze(0, "app.core", None);
        let ask = |request: &str| serve_request(&build, &log, request, |_| None, Vec::new());
        assert_eq!(ask("e\0\0resource\0x.txt\0"), "vtext");
        assert_eq!(ask("e\0\0resource?\0y.txt\0"), "n");
        assert!(ask("i\u{0}3\0resource\0./x.txt\0").contains("has no expansion site"));
        let log = log.borrow_mut().take().unwrap();
        let stable: Vec<_> = log
            .inputs
            .iter()
            .map(|record| match &record.input {
                ExternalInput::Resource(resource) => resource.stable,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(stable, [false, false]);
        drop(guard);
        assert!(
            ask("e\0\0resource\0x.txt\0")
                .starts_with("elight.build/resource is only available at compile time")
        );
    }

    #[test]
    fn a_nested_guard_restores_the_context_it_replaced() {
        let build: SharedBuild = Default::default();
        let log: SharedLog = Default::default();
        let ask = |request: &str| serve_request(&build, &log, request, |_| None, Vec::new());
        let outer = BuildGuard::enter(&build, Some(Rc::new(Anonymous)), Default::default());
        // Indices are names, whatever order analysis reaches them in.
        outer.analyze(2, "app.later", None);
        outer.analyze(0, "app.core", None);
        assert_eq!(ask("e\0\0resource?\0./x.txt\0"), "n");
        {
            let inner = BuildGuard::enter(&build, None, Default::default());
            inner.analyze(0, "other.ns", None);
            assert!(ask("e\0\0resource\0x.txt\0").contains("supplies no resources"));
        }
        assert_eq!(ask("e\0\0resource\0x.txt\0"), "vtext");
        outer.analyze(2, "app.later", None);
        assert_eq!(ask("e\0\0resource?\0./x.txt\0"), "n");
        drop(outer);
        assert!(ask("e\0\0resource\0x.txt\0").contains("only available at compile time"));
    }

    #[test]
    fn paths_resolve_against_roots_or_the_expansion_site() {
        assert_eq!(at("app/data.txt", None).unwrap(), "app/data.txt");
        assert_eq!(
            at("./data.txt", Some("my-app.core")).unwrap(),
            "my_app/data.txt"
        );
        assert_eq!(
            at("../shared/x.txt", Some("my-app.ui.core")).unwrap(),
            "my_app/shared/x.txt"
        );
        assert_eq!(at("./x.txt", Some("core")).unwrap(), "x.txt");
        assert!(at("../x.txt", Some("core")).is_err());
        assert!(at("./x.txt", None).is_err());
        assert!(at("/x.txt", None).is_err());
        assert!(at("a//b", None).is_err());
        assert!(at("a/../..", None).is_err());
        assert!(at("", None).is_err());
    }

    /// `path` resolved at namespace `site`, a file its provider cannot place.
    fn at(path: &str, site: Option<&str>) -> Result<String, String> {
        resolve_path(path, || {
            let site = Site {
                namespace: site?.into(),
                file: None,
            };
            Some(site_directory(&site, None))
        })
    }

    /// A provider that places its files under `gen/`.
    struct Generated;
    impl ResourceProvider for Generated {
        fn load_resource(
            &self,
            path: &str,
            _: &mut Vec<Candidate>,
        ) -> Result<Option<ResourceInput>, LoadError> {
            Ok(Some(ResourceInput {
                file: path.into(),
                identity: None,
                text: path.into(),
            }))
        }
        fn directory(&self, file: &str) -> Option<String> {
            file.starts_with("gen/").then(|| "gen".into())
        }
    }

    #[test]
    fn a_relative_path_resolves_against_the_directory_the_provider_gives_the_file() {
        let build: SharedBuild = Default::default();
        let log: SharedLog = Default::default();
        let ask = |request: &str| serve_request(&build, &log, request, |_| None, Vec::new());
        let guard = BuildGuard::enter(&build, Some(Rc::new(Generated)), Default::default());
        guard.analyze(0, "app.core", Some("gen/core.cljs"));
        assert_eq!(ask("e\0\0resource\0./x.txt\0"), "vgen/x.txt");
        // A file the provider cannot place resolves by the namespace's name.
        guard.analyze(0, "app.core", Some("<eval>.cljs"));
        assert_eq!(ask("e\0\0resource\0./x.txt\0"), "vapp/x.txt");
        guard.analyze(0, "app.core", None);
        assert_eq!(ask("e\0\0resource\0./x.txt\0"), "vapp/x.txt");
    }
}
