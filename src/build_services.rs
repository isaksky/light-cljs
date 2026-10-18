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

/// The compilation `light.build` serves: set while
/// [`crate::compiler`] analyzes namespaces, `None` otherwise.
pub(crate) struct BuildContext {
    resources: Option<Rc<dyn ResourceProvider>>,
    /// Each source's namespace by source index, as analysis reaches it.
    namespaces: Vec<String>,
    /// The source being analyzed.
    window: usize,
}

pub(crate) type SharedBuild = Rc<RefCell<Option<BuildContext>>>;
pub(crate) type SharedLog = Rc<RefCell<Option<ObservationLog>>>;

/// Serves one compilation while it lives.
pub(crate) struct BuildGuard(SharedBuild);

impl BuildGuard {
    pub(crate) fn enter(build: &SharedBuild, resources: Option<Rc<dyn ResourceProvider>>) -> Self {
        *build.borrow_mut() = Some(BuildContext {
            resources,
            namespaces: Vec::new(),
            window: 0,
        });
        Self(build.clone())
    }
    /// Source `index`, namespace `name`, is analyzed next.
    pub(crate) fn analyze(&self, index: usize, name: &str) {
        if let Some(context) = self.0.borrow_mut().as_mut() {
            context.window = index;
            if context.namespaces.len() == index {
                context.namespaces.push(name.to_owned());
            }
        }
    }
}

impl Drop for BuildGuard {
    fn drop(&mut self) {
        self.0.borrow_mut().take();
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
            context.namespaces.get(source).cloned()
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
            let path = resolve_path(first, site.as_deref()).map_err(|e| format!("{name}: {e}"))?;
            let Some(resources) = resources else {
                return Err(format!(
                    "{name}: the source provider of this compilation supplies no resources"
                ));
            };
            let mut tried = Vec::new();
            let answer =
                resources
                    .load_resource(&path, &mut tried)
                    .map_err(|error| match error {
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
                    })?;
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

/// The logical path `path` names: root-relative, or relative to the
/// directory of namespace `site`'s file when it starts with `./` or `../`.
fn resolve_path(path: &str, site: Option<&str>) -> Result<String, String> {
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
        let Some(site) = site else {
            return Err(format!(
                "relative resource path {path} has no expansion site: code that runs \
                 outside any macro expansion, such as a def's initializer, must use a \
                 path relative to the source roots"
            ));
        };
        let file = site.replace('-', "_");
        segments.extend(file.split('.').map(str::to_owned));
        segments.pop();
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
        guard.analyze(0, "app.core");
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
    fn paths_resolve_against_roots_or_the_expansion_site() {
        assert_eq!(resolve_path("app/data.txt", None).unwrap(), "app/data.txt");
        assert_eq!(
            resolve_path("./data.txt", Some("my-app.core")).unwrap(),
            "my_app/data.txt"
        );
        assert_eq!(
            resolve_path("../shared/x.txt", Some("my-app.ui.core")).unwrap(),
            "my_app/shared/x.txt"
        );
        assert_eq!(resolve_path("./x.txt", Some("core")).unwrap(), "x.txt");
        assert!(resolve_path("../x.txt", Some("core")).is_err());
        assert!(resolve_path("./x.txt", None).is_err());
        assert!(resolve_path("/x.txt", None).is_err());
        assert!(resolve_path("a//b", None).is_err());
        assert!(resolve_path("a/../..", None).is_err());
        assert!(resolve_path("", None).is_err());
    }
}
