//! Owned source requests and provider results, independent of evaluation.
use crate::compiler_data::Name;
use std::{fmt, path::PathBuf};
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
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
        let name = String::from_utf16_lossy(request.namespace.name());
        if !crate::compiler::valid_namespace(&name) {
            return Err(LoadError::Provider(format!("invalid namespace {name}")));
        }
        let Some(path) = crate::compiler::locate_source(&self.roots, &name, &["cljs", "cljc"])
            .map_err(|e| LoadError::Provider(e.to_string()))?
        else {
            return Ok(None);
        };
        let source = std::fs::read_to_string(&path)
            .map_err(|e| LoadError::Provider(format!("{}: {e}", path.display())))?;
        Ok(Some(SourceInput::new(path.to_string_lossy(), source)))
    }
}
