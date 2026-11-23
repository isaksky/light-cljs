//! Owned source requests and provider results, independent of evaluation.
use crate::compiler_data::Name;
use std::{
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
};
/// Whether a request reloads: not a loaded namespace, the namespace itself,
/// or it and every namespace it requires, directly or not, each once. They
/// are ordered by what they reload.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, PartialOrd, Ord)]
pub enum Reload {
    #[default]
    None,
    Namespace,
    All,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LoadError {
    InvalidLibspec(String),
    MissingNamespace(Name),
    MissingVar(Name),
    Cycle(Vec<Name>),
    AmbiguousSources(Vec<PathBuf>),
    Provider(String),
}
impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for LoadError {}
/// Owned request information. The callback never receives implicit capabilities.
#[derive(Clone, Debug)]
pub struct LoadRequest {
    pub namespace: Name,
    pub requesting_namespace: Name,
    pub requesting_file: Option<String>,
    pub reload: Reload,
}
#[derive(Clone, Debug)]
pub struct SourceInput {
    pub file: String,
    pub source: String,
}
impl SourceInput {
    pub fn new(file: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            file: file.into(),
            source: source.into(),
        }
    }
}
pub trait SourceProvider {
    fn load(&self, request: &LoadRequest) -> Result<Option<SourceInput>, LoadError>;
    /// [`Self::load`], also appending to `tried` each file considered, in
    /// order, present or absent, for a dependency manifest
    /// ([`crate::dependency_manifest`]). A provider without files to report
    /// keeps this default, which reports none.
    fn load_traced(
        &self,
        request: &LoadRequest,
        tried: &mut Vec<crate::dependency_manifest::Candidate>,
    ) -> Result<Option<SourceInput>, LoadError> {
        let _ = tried;
        self.load(request)
    }
    /// [`Self::load_traced`], returning only the resolved file identity
    /// (`SourceInput::file`) without reading the source text. Used when
    /// revalidating resolution of required namespaces so unchanged dependency
    /// files do not need to be opened and read again.
    fn locate_traced(
        &self,
        request: &LoadRequest,
        tried: &mut Vec<crate::dependency_manifest::Candidate>,
    ) -> Result<Option<String>, LoadError> {
        self.load_traced(request, tried)
            .map(|loaded| loaded.map(|input| input.file))
    }
    /// The resources `light.build/resource` reads while this provider's
    /// namespaces compile (docs/compiler.md, "Compile-time services:
    /// light.build"). The default has none, and every resource request is
    /// then an error.
    fn resources(&self) -> Option<std::rc::Rc<dyn ResourceProvider>> {
        None
    }
    /// This provider as one the loader may call from worker threads, several
    /// lookups at once, or `None`, the default, to be called on the
    /// compiling thread alone, in the loader's depth-first order
    /// (docs/compiler-concurrency.md, "Source discovery and readiness"). A
    /// provider that opts in is asked once per namespace a build loads, from
    /// whichever require the parse queue reached it by first, so its answers
    /// must not depend on [`LoadRequest::requesting_namespace`] or on the
    /// order of the requests; the dependency manifest still records the
    /// depth-first requester.
    fn concurrent(&self) -> Option<&(dyn SourceProvider + Send + Sync)> {
        None
    }
    /// An owned thread-safe handle to this provider that background workers
    /// (such as parallel cache publishing) may keep across the call boundary,
    /// or `None` when the provider cannot be moved to worker threads.
    fn owned_concurrent(&self) -> Option<Arc<dyn SourceProvider + Send + Sync>> {
        None
    }
}

/// A resource a [`ResourceProvider`] answered.
#[derive(Clone, Debug)]
pub struct ResourceInput {
    /// Where it was found, for diagnostics.
    pub file: String,
    /// The stable identity of this resource, which a cache may key on: the
    /// canonical file for source roots. `None` makes the work that read it
    /// non-cacheable.
    pub identity: Option<String>,
    pub text: String,
}

/// Compile-time resources by logical path: `app/data.txt` with `/`
/// separators, already normalized, never absolute. Answers are recorded in
/// the dependency manifest, so a provider must not consult anything a later
/// build could not observe again.
pub trait ResourceProvider {
    /// The resource at `path`, or `None` when it is absent, appending to
    /// `tried` each location considered, in order, present or absent. A path
    /// present in two places is an error.
    fn load_resource(
        &self,
        path: &str,
        tried: &mut Vec<crate::dependency_manifest::Candidate>,
    ) -> Result<Option<ResourceInput>, LoadError>;
    /// Whether absence and the identities answered are stable across
    /// builds. The default is false: the consuming work is non-cacheable.
    fn stable(&self) -> bool {
        false
    }
    /// The logical directory, `/`-separated and relative to the roots, of
    /// the source file this provider named `file` (`SourceInput::file`),
    /// against which `./` and `../` resolve. `None`, the default, is a file
    /// it cannot place, and a relative path then resolves against the
    /// directory the namespace's name maps to.
    fn directory(&self, file: &str) -> Option<String> {
        let _ = file;
        None
    }
}
impl<F> SourceProvider for F
where
    F: Fn(&LoadRequest) -> Result<Option<SourceInput>, LoadError>,
{
    fn load(&self, request: &LoadRequest) -> Result<Option<SourceInput>, LoadError> {
        self(request)
    }
}

/// Explicit compiler source roots, with `.cljs` taking precedence over `.cljc`.
#[derive(Clone, Debug)]
pub struct FileSourceProvider {
    roots: Arc<[PathBuf]>,
}
impl FileSourceProvider {
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self {
            roots: roots.into(),
        }
    }
}
impl SourceProvider for FileSourceProvider {
    fn load(&self, request: &LoadRequest) -> Result<Option<SourceInput>, LoadError> {
        self.locate(request, None)
    }
    fn load_traced(
        &self,
        request: &LoadRequest,
        tried: &mut Vec<crate::dependency_manifest::Candidate>,
    ) -> Result<Option<SourceInput>, LoadError> {
        self.locate(request, Some(tried))
    }
    fn locate_traced(
        &self,
        request: &LoadRequest,
        tried: &mut Vec<crate::dependency_manifest::Candidate>,
    ) -> Result<Option<String>, LoadError> {
        Ok(self
            .resolve(request, Some(tried))?
            .map(|path| path.to_string_lossy().into_owned()))
    }
    fn resources(&self) -> Option<std::rc::Rc<dyn ResourceProvider>> {
        Some(std::rc::Rc::new(self.clone()))
    }
    /// Source roots answer by name alone, from any thread.
    fn concurrent(&self) -> Option<&(dyn SourceProvider + Send + Sync)> {
        Some(self)
    }
    fn owned_concurrent(&self) -> Option<Arc<dyn SourceProvider + Send + Sync>> {
        Some(Arc::new(self.clone()))
    }
}
/// Resources across every root, like namespaces: a path in two roots is an
/// error.
impl ResourceProvider for FileSourceProvider {
    fn load_resource(
        &self,
        path: &str,
        tried: &mut Vec<crate::dependency_manifest::Candidate>,
    ) -> Result<Option<ResourceInput>, LoadError> {
        let mut found: Vec<PathBuf> = Vec::new();
        for root in self.roots.iter() {
            let candidate = root.join(path);
            let metadata = std::fs::metadata(&candidate);
            let present = metadata.as_ref().is_ok_and(|m| m.is_file());
            tried.push(crate::dependency_manifest::Candidate {
                path: candidate.clone(),
                present,
            });
            match metadata {
                Ok(_) if present => {
                    let canonical =
                        crate::canonical_path::canonicalize(&candidate).map_err(|e| {
                            LoadError::Provider(format!(
                                "cannot resolve {}: {e}",
                                candidate.display()
                            ))
                        })?;
                    if !found.contains(&canonical) {
                        found.push(canonical);
                    }
                }
                Ok(_) => {
                    return Err(LoadError::Provider(format!(
                        "resource is not a file: {}",
                        candidate.display()
                    )));
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(LoadError::Provider(format!(
                        "cannot inspect {}: {e}",
                        candidate.display()
                    )));
                }
            }
        }
        if found.len() > 1 {
            return Err(LoadError::AmbiguousSources(found));
        }
        let Some(file) = found.pop() else {
            return Ok(None);
        };
        let text = std::fs::read_to_string(&file)
            .map_err(|e| LoadError::Provider(format!("{}: {e}", file.display())))?;
        let file = file.to_string_lossy().into_owned();
        Ok(Some(ResourceInput {
            identity: Some(file.clone()),
            file,
            text,
        }))
    }
    fn stable(&self) -> bool {
        true
    }
    /// The directory of `file` under the first root that holds it, as the
    /// provider named it (a symbolic link stays where it is), or else once
    /// its directory and the roots are canonical; `None` for a file outside
    /// every root.
    fn directory(&self, file: &str) -> Option<String> {
        let logical = |parent: &Path, root: &Path| {
            parent
                .strip_prefix(root)
                .ok()?
                .components()
                .map(|component| match component {
                    std::path::Component::Normal(name) => name.to_str(),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()
                .map(|segments| segments.join("/"))
        };
        let parent = Path::new(file).parent()?;
        if let Some(found) = self.roots.iter().find_map(|root| logical(parent, root)) {
            return Some(found);
        }
        let parent = crate::canonical_path::canonicalize(parent).ok()?;
        self.roots.iter().find_map(|root| {
            let root = crate::canonical_path::canonicalize(root).ok()?;
            logical(&parent, &root)
        })
    }
}
impl FileSourceProvider {
    fn resolve(
        &self,
        request: &LoadRequest,
        tried: Option<&mut Vec<crate::dependency_manifest::Candidate>>,
    ) -> Result<Option<PathBuf>, LoadError> {
        let name = String::from_utf16_lossy(request.namespace.name());
        if !crate::compiler::valid_namespace(&name) {
            return Err(LoadError::Provider(format!("invalid namespace {name}")));
        }
        crate::compiler::locate_source_traced(&self.roots, &name, &["cljs", "cljc"], tried)
            .map_err(|e| LoadError::Provider(e.to_string()))
    }

    fn locate(
        &self,
        request: &LoadRequest,
        tried: Option<&mut Vec<crate::dependency_manifest::Candidate>>,
    ) -> Result<Option<SourceInput>, LoadError> {
        let Some(path) = self.resolve(request, tried)? else {
            return Ok(None);
        };
        let source = std::fs::read_to_string(&path)
            .map_err(|e| LoadError::Provider(format!("{}: {e}", path.display())))?;
        Ok(Some(SourceInput::new(path.to_string_lossy(), source)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let dir = std::env::temp_dir().join(format!(
            "light-cljs-source-provider-{tag}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn req(ns: &str) -> LoadRequest {
        LoadRequest {
            namespace: Name::new(None, ns),
            requesting_namespace: Name::new(None, "app.main"),
            requesting_file: None,
            reload: Reload::None,
        }
    }

    #[test]
    fn file_source_provider_locate_traced_matches_load_traced() {
        let base = temp_dir("locate-traced");
        let root_a = base.join("root-a");
        let root_b = base.join("root-b");
        fs::create_dir_all(root_a.join("app")).unwrap();
        fs::create_dir_all(root_b.join("app")).unwrap();

        // Present .cljs only in root_a.
        fs::write(root_a.join("app/only_cljs.cljs"), "(ns app.only-cljs)").unwrap();
        // Present .cljc only in root_b.
        fs::write(root_b.join("app/only_cljc.cljc"), "(ns app.only-cljc)").unwrap();
        // Shadowed: .cljs in root_b shadows .cljc in root_a (extension precedence).
        fs::write(root_a.join("app/shadowed.cljc"), "(ns app.shadowed)").unwrap();
        fs::write(root_b.join("app/shadowed.cljs"), "(ns app.shadowed)").unwrap();
        // Ambiguous: .cljs present in both roots.
        fs::write(root_a.join("app/ambig.cljs"), "(ns app.ambig)").unwrap();
        fs::write(root_b.join("app/ambig.cljs"), "(ns app.ambig)").unwrap();
        // Directory collision at candidate path.
        fs::create_dir_all(root_a.join("app/dir_hit.cljs")).unwrap();
        // Non-UTF-8 file: proves locate_traced inspects metadata without reading contents.
        fs::write(root_a.join("app/raw_bytes.cljs"), [0xff, 0xfe, 0xfd]).unwrap();

        let provider = FileSourceProvider::new(vec![root_a.clone(), root_b.clone()]);

        for ns in [
            "app.only-cljs",
            "app.only-cljc",
            "app.shadowed",
            "app.missing",
            "app.ambig",
            "app.dir-hit",
            "invalid..ns",
        ] {
            let request = req(ns);
            let mut load_tried = Vec::new();
            let loaded = provider.load_traced(&request, &mut load_tried);
            let mut locate_tried = Vec::new();
            let located = provider.locate_traced(&request, &mut locate_tried);
            assert_eq!(locate_tried, load_tried, "candidates for {ns}");
            assert_eq!(
                located,
                loaded.map(|opt| opt.map(|input| input.file)),
                "result for {ns}"
            );
        }

        // Non-UTF-8 file: load_traced fails reading UTF-8, while locate_traced
        // succeeds because it never opens/reads the file.
        let raw_req = req("app.raw-bytes");
        let mut load_tried = Vec::new();
        assert!(provider.load_traced(&raw_req, &mut load_tried).is_err());
        let mut locate_tried = Vec::new();
        let located = provider.locate_traced(&raw_req, &mut locate_tried).unwrap();
        assert!(located.is_some());
        assert_eq!(locate_tried, load_tried);

        let _ = fs::remove_dir_all(&base);
    }
}
