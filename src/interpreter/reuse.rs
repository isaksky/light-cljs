//! Bounded, context-local reuse of immutable analysis, never expansion or effects.
use super::*;
use std::collections::VecDeque;

const MAX_ENTRIES: usize = 32;
const MAX_NODES: usize = 8_192;
const MAX_SOURCE_BYTES: usize = 256 * 1_024;
const MAX_ENTRY_SOURCE_BYTES: usize = 64 * 1_024;

/// Cumulative analysis reuse counters and current retained cache size.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReuseStats {
    pub hits: u64,
    pub misses: u64,
    pub entries: usize,
    pub retained_nodes: usize,
    /// Conservative sum of complete source lengths (shared sources count again).
    pub retained_source_bytes: usize,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) struct Environment {
    namespace: Option<NamespaceId>,
    namespaces: u64,
    vars: u64,
    configuration: u64,
}
struct Entry {
    identity: (UnitId, FormId),
    forms: Weak<reader::Forms>,
    environment: Environment,
    code: AnalyzedForm,
    nodes: usize,
    source_bytes: usize,
}
#[derive(Default)]
pub(super) struct AnalysisReuse {
    entries: VecDeque<Entry>,
    stats: ReuseStats,
}
impl AnalysisReuse {
    fn remove_front(&mut self) {
        if let Some(entry) = self.entries.pop_front() {
            self.stats.retained_nodes -= entry.nodes;
            self.stats.retained_source_bytes -= entry.source_bytes;
        }
    }
    fn prune(&mut self, environment: Environment) {
        self.entries
            .retain(|entry| entry.forms.strong_count() != 0 && entry.environment == environment);
        self.stats.retained_nodes = self.entries.iter().map(|e| e.nodes).sum();
        self.stats.retained_source_bytes = self.entries.iter().map(|e| e.source_bytes).sum();
    }
}
impl Context {
    pub fn analysis_reuse_stats(&self) -> ReuseStats {
        ReuseStats {
            entries: self.reuse.entries.len(),
            ..self.reuse.stats
        }
    }
    /// Drop optional retained analysis immediately; existing code handles survive.
    pub fn clear_analysis_reuse(&mut self) {
        self.reuse = AnalysisReuse::default();
    }
    pub(super) fn invalidate_analysis(&self) {
        self.reuse_revision.set(
            self.reuse_revision
                .get()
                .checked_add(1)
                .expect("analysis configuration revisions exhausted"),
        );
    }
    fn analysis_environment(&self) -> Environment {
        let namespace = self.namespaces.get().map(|_| self.current_namespace_id());
        let store = self.owner.store.borrow();
        Environment {
            namespace,
            namespaces: store.namespaces.revision(),
            vars: store.vars.revision(),
            configuration: self.reuse_revision.get(),
        }
    }
    pub(super) fn prune_analysis_reuse(&mut self) {
        if !self.reuse.entries.is_empty() {
            let environment = self.analysis_environment();
            self.reuse.prune(environment);
        }
    }
    pub(super) fn reused_analysis(&mut self, form: &Form) -> Option<AnalyzedForm> {
        if !self.accounting.allows_analysis_reuse() {
            return None;
        }
        let environment = self.analysis_environment();
        self.reuse.prune(environment);
        let identity = form.reuse_identity();
        if let Some(entry) = self.reuse.entries.iter().find(|e| e.identity == identity) {
            self.reuse.stats.hits += 1;
            return Some(entry.code.clone());
        }
        self.reuse.stats.misses += 1;
        None
    }
    pub(super) fn retain_analysis(&mut self, form: &Form, code: &AnalyzedForm) {
        if !self.accounting.allows_analysis_reuse() {
            return;
        }
        // Count both semantic and executable records, including bodies. Source
        // identity includes exact text and provenance through the owned code.
        let nodes = form.nodes().len().saturating_add(code.expression_count());
        let source_bytes = form.source_length();
        if nodes > MAX_NODES || source_bytes > MAX_ENTRY_SOURCE_BYTES {
            return;
        }
        let environment = self.analysis_environment();
        self.reuse.prune(environment);
        while self.reuse.entries.len() >= MAX_ENTRIES
            || self.reuse.stats.retained_nodes + nodes > MAX_NODES
            || self.reuse.stats.retained_source_bytes + source_bytes > MAX_SOURCE_BYTES
        {
            self.reuse.remove_front();
        }
        self.reuse.entries.push_back(Entry {
            identity: form.reuse_identity(),
            forms: form.weak_forms(),
            environment,
            code: code.clone(),
            nodes,
            source_bytes,
        });
        self.reuse.stats.retained_nodes += nodes;
        self.reuse.stats.retained_source_bytes += source_bytes;
    }
}
