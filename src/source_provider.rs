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
