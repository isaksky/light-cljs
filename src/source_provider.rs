//! Owned source requests and provider results, independent of evaluation.
use crate::compiler_data::Name;
use std::{
    fmt,
    path::{Path, PathBuf},
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
    /// The resources `light.build/resource` reads while this provider's
    /// namespaces compile (docs/compiler.md, "Compile-time services:
    /// light.build"). The default has none, and every resource request is
    /// then an error.
    fn resources(&self) -> Option<std::rc::Rc<dyn ResourceProvider>> {
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
    roots: Vec<PathBuf>,
}
impl FileSourceProvider {
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self { roots }
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
    fn resources(&self) -> Option<std::rc::Rc<dyn ResourceProvider>> {
        Some(std::rc::Rc::new(self.clone()))
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
        for root in &self.roots {
            let candidate = root.join(path);
            let metadata = std::fs::metadata(&candidate);
            let present = metadata.as_ref().is_ok_and(|m| m.is_file());
            tried.push(crate::dependency_manifest::Candidate {
                path: candidate.clone(),
                present,
            });
            match metadata {
                Ok(_) if present => {
                    let canonical = std::fs::canonicalize(&candidate).map_err(|e| {
                        LoadError::Provider(format!("cannot resolve {}: {e}", candidate.display()))
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
        let parent = std::fs::canonicalize(parent).ok()?;
        self.roots.iter().find_map(|root| {
            let root = std::fs::canonicalize(root).ok()?;
            logical(&parent, &root)
        })
    }
}
impl FileSourceProvider {
    fn locate(
        &self,
        request: &LoadRequest,
        tried: Option<&mut Vec<crate::dependency_manifest::Candidate>>,
    ) -> Result<Option<SourceInput>, LoadError> {
        let name = String::from_utf16_lossy(request.namespace.name());
        if !crate::compiler::valid_namespace(&name) {
            return Err(LoadError::Provider(format!("invalid namespace {name}")));
        }
        let Some(path) =
            crate::compiler::locate_source_traced(&self.roots, &name, &["cljs", "cljc"], tried)
                .map_err(|e| LoadError::Provider(e.to_string()))?
        else {
            return Ok(None);
        };
        let source = std::fs::read_to_string(&path)
            .map_err(|e| LoadError::Provider(format!("{}: {e}", path.display())))?;
        Ok(Some(SourceInput::new(path.to_string_lossy(), source)))
    }
}
