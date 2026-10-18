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
    EdgeKinds, ExternalInput, InputRecord, ObservationLog, ResolvedResource, ResourceDependency,
    content_hash,
};
use crate::source_provider::{LoadError, ResourceProvider};
use std::{cell::RefCell, rc::Rc};

/// The compilation `light.build` serves: set while [`crate::compiler`]
/// analyzes namespaces and for the life of an interactive compiler
/// ([`crate::eval_compiler`]), `None` otherwise.
pub(crate) struct BuildContext {
    resources: Option<Rc<dyn ResourceProvider>>,
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
    pub(crate) fn enter(build: &SharedBuild, resources: Option<Rc<dyn ResourceProvider>>) -> Self {
        let previous = build.borrow_mut().replace(BuildContext {
            resources,
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
    let (resources, own, site) = {
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
        (context.resources.clone(), context.window as u32, site)
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
                    hash: content_hash(found.text.as_bytes()),
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
        let guard = BuildGuard::enter(&build, Some(Rc::new(Anonymous)));
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
        let outer = BuildGuard::enter(&build, Some(Rc::new(Anonymous)));
        // Indices are names, whatever order analysis reaches them in.
        outer.analyze(2, "app.later", None);
        outer.analyze(0, "app.core", None);
        assert_eq!(ask("e\0\0resource?\0./x.txt\0"), "n");
        {
            let inner = BuildGuard::enter(&build, None);
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
        let guard = BuildGuard::enter(&build, Some(Rc::new(Generated)));
        guard.analyze(0, "app.core", Some("gen/core.cljs"));
        assert_eq!(ask("e\0\0resource\0./x.txt\0"), "vgen/x.txt");
        // A file the provider cannot place resolves by the namespace's name.
        guard.analyze(0, "app.core", Some("<eval>.cljs"));
        assert_eq!(ask("e\0\0resource\0./x.txt\0"), "vapp/x.txt");
        guard.analyze(0, "app.core", None);
        assert_eq!(ask("e\0\0resource\0./x.txt\0"), "vapp/x.txt");
    }
}
