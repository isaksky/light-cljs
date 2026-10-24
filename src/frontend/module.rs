//! Immutable static module records. No host resolution or task state lives here.

use std::sync::Arc;

use super::Span;
use super::ast::ModuleItem;
use crate::value::JsString;

/// The binding requested from another module. `default` is an ordinary name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportName {
    Name(String),
    Namespace,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportEntry {
    pub specifier: JsString,
    pub import_name: ImportName,
    pub local_name: String,
    /// The local binding's source position.
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalExportEntry {
    pub export_name: String,
    /// `*default*` denotes the inaccessible binding of a default expression
    /// or an anonymous default declaration (ECMA-262 BoundNames).
    pub local_name: String,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndirectExportEntry {
    pub specifier: JsString,
    pub import_name: String,
    pub export_name: String,
    pub span: Span,
}

/// ParseModule's shareable, immutable result, also named [`super::ast::Module`].
///
/// A clone shares the complete parsed body and static tables. These contain
/// Rust-owned syntax and strings, with no heap handles, instances, dependency
/// edges, resolver, world/version, or task-owned state. An embedder can later
/// cache records per world/version and instantiate them separately per task.
#[derive(Clone, Debug, PartialEq)]
pub struct ModuleRecord(Arc<ModuleData>);

#[derive(Debug, PartialEq)]
struct ModuleData {
    body: Vec<ModuleItem>,
    span: Span,
    requested_modules: Vec<JsString>,
    import_entries: Vec<ImportEntry>,
    local_export_entries: Vec<LocalExportEntry>,
    indirect_export_entries: Vec<IndirectExportEntry>,
    star_export_entries: Vec<JsString>,
}

impl ModuleRecord {
    pub(super) fn new(
        body: Vec<ModuleItem>,
        span: Span,
        requested_modules: Vec<JsString>,
        import_entries: Vec<ImportEntry>,
        local_export_entries: Vec<LocalExportEntry>,
        indirect_export_entries: Vec<IndirectExportEntry>,
        star_export_entries: Vec<JsString>,
    ) -> Self {
        Self(Arc::new(ModuleData {
            body,
            span,
            requested_modules,
            import_entries,
            local_export_entries,
            indirect_export_entries,
            star_export_entries,
        }))
    }

    /// Executable syntax in source order. Import declarations and export
    /// lists contribute only table entries; exported declarations remain here.
    pub fn body(&self) -> &[ModuleItem] {
        &self.0.body
    }

    pub fn span(&self) -> Span {
        self.0.span
    }

    pub fn is_strict(&self) -> bool {
        true
    }

    /// Cooked UTF-16 specifiers, deduplicated in first-request source order.
    /// In particular, lone surrogates are preserved without lossy conversion.
    pub fn requested_modules(&self) -> &[JsString] {
        &self.0.requested_modules
    }

    pub fn import_entries(&self) -> &[ImportEntry] {
        &self.0.import_entries
    }

    pub fn local_export_entries(&self) -> &[LocalExportEntry] {
        &self.0.local_export_entries
    }

    pub fn indirect_export_entries(&self) -> &[IndirectExportEntry] {
        &self.0.indirect_export_entries
    }

    /// Source specifiers of export-star entries, in source order.
    pub fn star_export_entries(&self) -> &[JsString] {
        &self.0.star_export_entries
    }
}
