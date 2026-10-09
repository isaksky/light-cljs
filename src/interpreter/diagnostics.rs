//! Opt-in, owned snapshots: reports never keep interpreter roots alive.
use super::*;
use std::time::{Duration, Instant};

/// Exact source form identity, independent of recyclable arena IDs. Text is
/// included so changed definitions at the same filename/offset do not merge.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticLocation {
    pub source: String,
    pub bytes: Range<usize>,
    pub line: usize,
    pub column: usize,
    pub text: String,
}

impl DiagnosticLocation {
    pub(super) fn new(store: &Store, location: &Location) -> Self {
        // Read the provenance side table directly. Creating public Source/Unit
        // handles here would charge the program's semantic allocation budget.
        let file = store.sources.get(location.source.0).expect("rooted source");
        let (line, column) = file
            .lines
            .get_or_init(|| LineIndex::new(&file.text))
            .line_column(location.bytes.start)
            .expect("validated source location");
        Self {
            source: file.name.clone(),
            bytes: location.bytes.clone(),
            line: line + file.line_offset,
            column,
            text: file.text[location.bytes.clone()].to_owned(),
        }
    }
}

/// Successful analyzed units attributed to the original source form. Repeated
/// generations are repeated analysis, not proof that effectful macro output is
/// equivalent or safe to cache. Reusing analyzed code does not increment these.
#[derive(Clone, Debug)]
pub struct CodeGenerationStats {
    pub location: DiagnosticLocation,
    pub origin: Origin,
    pub generations: u64,
    pub generated_expressions: u64,
    pub generated_functions: u64,
    /// Inclusive analysis time, including any required macro expansion.
    pub elapsed: Duration,
}

impl Context {
    /// Enable both macro and analyzed-code attribution. Expensive clocks,
    /// source snapshots and counter searches are absent while disabled.
    pub fn set_diagnostics(&mut self, enabled: bool) {
        self.set_macro_diagnostics(enabled);
        self.macros.code_diagnostics = enabled;
    }

    pub fn code_generation_stats(&self) -> &[CodeGenerationStats] {
        &self.macros.code_stats
    }

    /// Release accumulated reports without changing the diagnostic switches.
    pub fn clear_diagnostics(&mut self) {
        self.macros.stats.clear();
        self.macros.code_stats.clear();
    }

    pub(super) fn record_code_generation(
        &mut self,
        form: &Form,
        analyzed: &AnalyzedForm,
        started: Option<Instant>,
    ) {
        let Some(started) = started else { return };
        let elapsed = started.elapsed();
        let location = form.diagnostic_location();
        let origin = form.node().origin.clone();
        let index = self
            .macros
            .code_stats
            .iter()
            .position(|stats| stats.location == location && stats.origin == origin)
            .unwrap_or_else(|| {
                self.macros.code_stats.push(CodeGenerationStats {
                    location,
                    origin,
                    generations: 0,
                    generated_expressions: 0,
                    generated_functions: 0,
                    elapsed: Duration::ZERO,
                });
                self.macros.code_stats.len() - 1
            });
        let stats = &mut self.macros.code_stats[index];
        stats.generations += 1;
        stats.generated_expressions += analyzed.expression_count() as u64;
        stats.generated_functions += analyzed.unit().code.functions.len() as u64;
        stats.elapsed += elapsed;
    }
}
