//! Transfer a namespace's top-level result without advancing its deferred
//! drain point. Execution and publication may happen on different threads.
use super::*;
use crate::program_fragment::{Publication, SharedPrefix};
use std::sync::Arc;

pub(crate) struct NamespaceJob {
    top: NamespaceTop,
    shared: SharedPrefix,
    globals_before: crate::arena::Arena<GlobalBinding>,
}

pub(crate) struct NamespacePublication {
    program: Program,
    shared: SharedPrefix,
    root: ExprId,
    drained: DrainedNamespace,
    requires: Vec<String>,
    charges: Option<crate::compilation_session::UnitCharges>,
    changed_globals: Vec<usize>,
    warnings: Vec<AnalysisWarning>,
    mutated_foreign_ns: bool,
}

/// Reusable frozen prefix snapshot shared across all ready namespaces in a
/// single parallel dispatch wave (O(1) structural sharing per namespace).
pub(crate) struct WaveSnapshot {
    program: Program,
    globals: crate::name_index::NameIndex,
    record_layouts: RecordLayouts,
    shared: SharedPrefix,
    globals_before: crate::arena::Arena<GlobalBinding>,
}

impl WaveSnapshot {
    pub(crate) fn job(&self, source_index: usize, slots: crate::scheduler::Slots) -> NamespaceJob {
        NamespaceJob {
            shared: self.shared.clone(),
            globals_before: self.globals_before.clone(),
            top: NamespaceTop {
                program: self.program.clone(),
                globals: self.globals.clone(),
                members: Default::default(),
                record_layouts: self.record_layouts.clone(),
                source_index,
                unit_slots: slots,
                main_isolate: false,
                deferred_time: None,
            },
        }
    }
}

impl NamespaceAnalysis {
    /// Freeze the program once so every ready namespace in a parallel wave can
    /// clone an O(1) [`NamespaceJob`] from the same snapshot.
    pub(crate) fn wave_snapshot(&mut self) -> WaveSnapshot {
        self.program.freeze();
        WaveSnapshot {
            shared: SharedPrefix::same(&self.program),
            globals_before: self.program.globals.clone(),
            program: self.program.clone(),
            globals: self.globals.clone(),
            record_layouts: self.record_layouts.clone(),
        }
    }

    /// Publish a parallel overlay namespace pass in canonical order: relocates
    /// local unit IDs to canonical unit IDs, appends top-level warnings in
    /// serial order (so `finish_capture` and `hold_warnings` match `--jobs 1`),
    /// appends top-level IR, and queues deferred units for `merge_front`.
    pub(crate) fn publish_overlay_namespace(
        &mut self,
        mut published: NamespacePublication,
        base_unit_id: usize,
        pooled: Vec<usize>,
        warnings: &mut Vec<AnalysisWarning>,
    ) {
        assert_eq!(published.drained.source_index, self.roots.len());
        let warnings_start = warnings.len();
        warnings.extend(published.warnings);
        published.drained.warnings = HeldWarnings::Reported(warnings_start);
        published.drained.relocate_unit_ids(base_unit_id, pooled);
        let start = self.program.exprs.len();
        let first_lazy = self.program.lazy_targets.len();
        published.program.root = published.root;
        let (root, ids) = crate::program_fragment::append_published(
            &mut self.program,
            &published.program,
            &published.shared,
            &published.drained.namespace,
            &mut self.globals,
            &published.changed_globals,
        );
        published.drained.relocate(ids);
        let end = self.program.exprs.len();
        for i in start..end {
            if let ExprKind::DefinitionTest { unit, .. } = &mut self.program.exprs[i].kind
                && *unit >= crate::compilation_session::LOCAL_UNIT_BASE
            {
                *unit = base_unit_id + (*unit - crate::compilation_session::LOCAL_UNIT_BASE);
            }
        }
        published.drained.top = start..end;
        self.record_layouts.extend(&self.program.exprs);
        self.last_lazy = self.lazy_namespaces_since(first_lazy);
        self.roots.push(root);
        self.requires.push(published.requires);
        self.pending.push_back(Pending::Analyzed(published.drained));
    }
}

impl NamespaceJob {
    /// Analyze a namespace's top-level pass on a pool worker backed by a
    /// private [`crate::compilation_session::CompilationSession`] overlay
    /// (Stage C, light-cljs-1090). Deferred units stay unanalyzed in the
    /// overlay (except in `warning_mode`) until canonical publication.
    pub(crate) fn analyze_overlay(
        mut self,
        input: &NamespaceInput<'_>,
        resolution: NamespaceResolution,
        result: bool,
        host: &mut MacroHost,
        options: AnalysisOptions,
        recording_cfg: crate::macro_host::UnitRecording,
    ) -> Result<NamespacePublication, AnalysisError> {
        let mut warnings = Vec::new();
        let evaluator = host.engine.bootstrap.as_ref().map(|b| b.evaluator.clone());
        let engine = evaluator
            .as_ref()
            .map(|e| e.as_ref() as &dyn crate::evaluator::MacroEvaluator);
        let logs_capture = recording_cfg.capture;
        if logs_capture {
            crate::generated_names::start_log();
        }
        let captures_namespaces = logs_capture && engine.is_some();
        let pending_capture =
            (engine.is_some() && (logs_capture || captures_namespaces)).then(|| {
                crate::macro_host::PendingCaptureGuard::install(
                    host.engine.work_cache.clone(),
                    crate::macro_host::PendingUnitCapture {
                        on_worker: true,
                        logs_gensyms: logs_capture,
                        logs_namespaces: captures_namespaces,
                        pooled: false,
                        session: None,
                        gensym_error: false,
                        namespace_error: false,
                        entered: false,
                    },
                )
            });
        let recording = crate::macro_host::MacroFreeUnit::enter(
            input.name.to_owned(),
            false,
            recording_cfg,
            true,
        );
        let outcome = self
            .top
            .analyze(input, resolution, result, host, options, &mut warnings);
        let recorded = recording.finish();
        let capture = pending_capture.and_then(|guard| guard.finish());
        let entered = capture.as_ref().is_some_and(|c| c.entered);
        let started_gensyms = if capture.as_ref().is_some_and(|c| c.gensym_error) {
            Err(String::new())
        } else {
            Ok(())
        };
        let started_namespaces = if capture.as_ref().is_some_and(|c| c.namespace_error) {
            Err(String::new())
        } else {
            Ok(())
        };
        let trace_rows = crate::compile_trace::take_worker_rows();
        let mut gensyms = if logs_capture {
            let logged = crate::generated_names::stop_log();
            started_gensyms.map(|()| logged)
        } else {
            Ok(Vec::new())
        };
        let namespaces = match engine {
            Some(engine) if captures_namespaces && entered => {
                match started_namespaces.and_then(|()| {
                    crate::compilation_session::CompilationSession::take_namespaces_from_cached(
                        engine,
                        false,
                        host.engine.work_cache.take_namespaces_handle(false),
                    )
                }) {
                    Ok(entries) => {
                        host.engine.work_cache.namespace_pool_mode.set(Some(false));
                        if !entries.is_empty() {
                            host.engine.work_cache.encode_peak_clean.set(false);
                        }
                        Ok(entries)
                    }
                    Err(error) => {
                        host.engine.work_cache.namespace_pool_mode.set(None);
                        host.engine.work_cache.encode_peak_clean.set(false);
                        Err(error)
                    }
                }
            }
            _ => Ok(Vec::new()),
        };
        let mut recorded = recorded;
        if gensyms.is_err() {
            recorded
                .events
                .push(crate::macro_host::UnitEvent::Unmeasured);
            gensyms = Ok(Vec::new());
        }
        let namespaces = namespaces.unwrap_or_else(|_| {
            recorded
                .events
                .push(crate::macro_host::UnitEvent::Unmeasured);
            Vec::new()
        });
        let charges = crate::compilation_session::UnitCharges::namespace(
            recorded,
            host,
            input.name,
            gensyms.unwrap_or_default(),
            namespaces,
            trace_rows,
        );
        let (root, drained) = outcome?;
        let changed_globals = self
            .top
            .program
            .globals
            .changed_entries(&self.globals_before, self.globals_before.len());
        let mutated_foreign_ns = changed_globals
            .iter()
            .any(|&idx| self.top.program.globals[idx].namespace != input.name)
            || self
                .top
                .program
                .globals
                .range(self.globals_before.len()..self.top.program.globals.len())
                .any(|g| g.declared && g.namespace != input.name);
        Ok(NamespacePublication {
            program: self.top.program,
            shared: self.shared,
            root,
            drained,
            requires: input.requires.to_vec(),
            charges: Some(charges),
            changed_globals,
            warnings,
            mutated_foreign_ns,
        })
    }
}

impl NamespacePublication {
    pub(crate) fn charge(&mut self, host: &MacroHost) -> Result<(), AnalysisError> {
        let charges = self.charges.take().expect("namespace charges publish once");
        host.session_handle()
            .unwrap()
            .charge_namespace(charges, host)
    }
    pub(crate) fn dispatched_local_ids(&self) -> Vec<usize> {
        self.drained
            .dispatched
            .iter()
            .map(|unit| unit.unit)
            .collect()
    }
    pub(crate) fn any_may_enter_engine(&self) -> bool {
        self.drained.dispatched.iter().any(|u| u.may_enter_engine)
    }
    pub(crate) fn mutated_foreign_ns(&self) -> bool {
        self.mutated_foreign_ns
    }
    pub(crate) fn absent_namespaces(&self) -> impl Iterator<Item = &str> + '_ {
        self.charges.iter().flat_map(|c| c.absent_namespaces())
    }
    pub(crate) fn added_global_keys(
        &self,
        since: usize,
    ) -> impl Iterator<Item = (&str, &str)> + '_ {
        self.program
            .globals
            .range(since..self.program.globals.len())
            .map(|g| (g.namespace.as_str(), g.name.as_str()))
    }
}

impl DrainedNamespace {
    fn relocate(&mut self, ids: Arc<Publication>) {
        for unit in &mut self.units {
            unit.placeholder = ids.expr(unit.placeholder);
            unit.shared = unit.shared.clone().published(ids.clone());
        }
        for unit in &mut self.dispatched {
            unit.placeholder = ids.expr(unit.placeholder);
            unit.shared = unit.shared.clone().published(ids.clone());
        }
        self.checked_array_exprs = self
            .checked_array_exprs
            .iter()
            .map(|id| ids.expr(*id))
            .collect();
    }

    fn relocate_unit_ids(&mut self, base: usize, pooled: Vec<usize>) {
        let map = |id: usize| {
            if id >= crate::compilation_session::LOCAL_UNIT_BASE {
                base + (id - crate::compilation_session::LOCAL_UNIT_BASE)
            } else {
                id
            }
        };
        for unit in &mut self.units {
            unit.unit = map(unit.unit);
        }
        for unit in &mut self.dispatched {
            unit.unit = map(unit.unit);
        }
        self.pooled = pooled;
    }
}
