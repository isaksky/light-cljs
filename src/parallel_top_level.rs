//! Stage C parallel top-level namespace analysis (`light-cljs-1090`).
//!
//! Schedules ready namespaces ahead of the coordinator's canonical publication
//! frontier onto the build's unified `UnitPool` (reusing the Phase 1 capped
//! V8/QuickJS worker isolates and engine-less Rust workers with zero extra
//! isolates), using an incremental dependency DAG over `loader.ordered`.
//!
//! Correctness & determinism invariants:
//! - Strict require-closure & symbol-reference visibility: namespace `j` never
//!   starts until every namespace `i < j` it requires, aliases, imports, or
//!   references via a qualified symbol has been published canonically.
//! - Private per-worker `NamespaceOverlay`: workers buffer local units and
//!   registrations privately and never pollute `SharedCatalog` before canonical
//!   publication at `index == j`.
//! - Freshness validation at canonical publication: checks foreign-namespace
//!   mutations, environment-extension bundles, data readers, namespace
//!   reflection (`all-ns` / `find-ns`), and newly reserved globals for `j`.
//!   Any stale result or engine mismatch falls back cleanly to serial analysis.
//! - Canonical publication order `0, 1, 2, ...`: preserves exact byte-identical
//!   IR, unit IDs, warnings, gensym/macro charges, error precedence, and JS
//!   output across `--jobs 1, 2, 4, 8`.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::analyzer::namespace_publication::NamespacePublication;
use crate::analyzer::{AnalysisOptions, NamespaceAnalysis, NamespaceInput};
use crate::build_services::BuildGuard;
use crate::compilation_session::{
    CompilationSession, NamespaceOverlayOutput, PoolPolicy, SharedCatalog,
};
use crate::macro_host::{MacroHost, NamespaceMetadata, UnitRecording};
use crate::{AnalysisError, CollectionKind, NodeId, NodeKind};

use super::{NamespaceSource, prepare_source};

/// How far ahead of the canonical publication frontier `index` to inspect for
/// ready namespaces. Wide enough to keep all workers saturated on sparse DFS
/// trees (such as Re-frame's `(events, subs, views)` triples) without unbounded
/// lookahead memory.
const LOOKAHEAD: usize = 256;

/// Cap on completed-but-unconsumed parallel top-level results held in memory
/// while waiting for canonical `index` to reach them.
const MAX_BUFFERED: usize = 64;

/// Near-frontier lookahead window that stays eligible even when distant
/// lookahead namespaces fill `MAX_BUFFERED`, so newly-unblocked immediate
/// successors never fall back to running serially on the coordinator.
const NEAR_LOOKAHEAD: usize = 32;

/// Maximum times a namespace whose worker isolate cursor moved past its
/// snapshot `prefix_cursor` is re-dispatched with a fresh snapshot before
/// falling back to the coordinator.
const MAX_RETRIES: u8 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SlotState {
    Pending,
    /// Reserved for the coordinator to run inline in `warning_mode`, so the
    /// coordinator shares lookahead waves with pool workers instead of
    /// blocking on every namespace in `take_or_wait`.
    Coordinator,
    Running {
        needs_engine: bool,
    },
    Done,
}

pub(super) struct ReadyNamespace {
    pub(super) publication: NamespacePublication,
    pub(super) metadata: NamespaceMetadata,
    pub(super) overlay: NamespaceOverlayOutput,
    pub(super) resolution: crate::analyzer::NamespaceResolution,
    pub(super) snap_globals_len: usize,
}

struct CompletedNamespace {
    ready: ReadyNamespace,
    snap_index: usize,
    snap_globals_len: usize,
    snap_env_bundles: usize,
    snap_data_readers: usize,
    snap_foreign_epoch: u64,
    entered_engine: bool,
    reflected_namespaces: Vec<String>,
}

enum TopLevelOutcome {
    Ready(Box<CompletedNamespace>),
    /// Pure-Rust worker hit a JS macro or custom data reader; retry on an
    /// engine worker (or fall back to coordinator if none exist).
    NeedsEngine,
    /// Worker could not satisfy isolation (e.g. isolate cursor past
    /// `prefix_cursor`, data-reader mutation, namespace reflection in JS,
    /// error needing exact serial partial accounting, or foreign macro-table
    /// mutation); re-dispatch with fresh snapshot when `retry` is true, or
    /// fall straight back to the coordinator otherwise.
    NeedCoordinator {
        retry: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TopLevelClass {
    Plain,
    Engine,
    /// Namespaces that literally declare `defmacro` / `^:macro` run on the
    /// coordinator so `MacroCode.tables` captures exact canonical `ReaderTables`
    /// and registers them in the coordinator's engine.
    CoordinatorOnly,
}

pub(super) struct ParallelTopLevel {
    pool_policy: PoolPolicy,
    slots: crate::scheduler::Slots,
    max_in_flight: usize,
    max_in_flight_engine: usize,
    /// Namespace name -> canonical index in `loader.ordered`.
    ns_index: HashMap<String, usize>,
    /// How many entries of `loader.ordered` have been added to the DAG.
    indexed_len: usize,
    /// Forward edges: `dependents[i]` lists indices `j > i` that depend on `i`.
    dependents: Vec<Vec<usize>>,
    /// Count of unpublished dependencies `i < j` for each namespace `j`.
    unmet_deps: Vec<usize>,
    /// Scheduling state per namespace index.
    states: Vec<SlotState>,
    /// Namespaces discovered to require an engine after a pure-Rust bailout.
    force_engine: Vec<bool>,
    /// Retry counter per namespace for `NeedCoordinator` cursor misses.
    retries: Vec<u8>,
    /// Cached `classify_top_level` result keyed by `(macro_version, data_reader_count)`.
    needs_engine_cache: Vec<Option<(u64, usize, TopLevelClass)>>,
    /// Shared completion queue written by pool workers, reaped by coordinator.
    completed: Arc<Mutex<Vec<(usize, TopLevelOutcome)>>>,
    /// Coordinator-owned reaped outcomes awaiting canonical `index`.
    ready: HashMap<usize, TopLevelOutcome>,
    in_flight: usize,
    in_flight_engine: usize,
    /// Remaining worker dispatches before reserving one ready namespace for the
    /// coordinator in `warning_mode` (tracks engine and plain classes separately
    /// so mixed pools balance both).
    warn_engine_quota: usize,
    warn_plain_quota: usize,
    /// Incremented whenever any published namespace mutates a foreign namespace's
    /// globals or macro tables.
    foreign_mutation_epoch: u64,
    /// Names of canonically published namespaces in order `0..index`.
    published_names: Vec<String>,
}

impl ParallelTopLevel {
    pub(super) fn new(pool_policy: PoolPolicy, slots: crate::scheduler::Slots) -> Self {
        let workers = pool_policy.workers.max(1);
        let max_in_flight = workers * 3;
        let max_in_flight_engine = pool_policy.engine_workers * 3;
        Self {
            pool_policy,
            slots,
            max_in_flight,
            max_in_flight_engine,
            ns_index: HashMap::new(),
            indexed_len: 0,
            dependents: Vec::new(),
            unmet_deps: Vec::new(),
            states: Vec::new(),
            force_engine: Vec::new(),
            retries: Vec::new(),
            needs_engine_cache: Vec::new(),
            completed: Arc::new(Mutex::new(Vec::new())),
            ready: HashMap::new(),
            in_flight: 0,
            in_flight_engine: 0,
            warn_engine_quota: pool_policy.engine_workers,
            warn_plain_quota: workers,
            foreign_mutation_epoch: 0,
            published_names: Vec::new(),
        }
    }

    /// Incrementally index any newly loaded namespaces in `ordered` into the
    /// dependency DAG. All edges point strictly forward (`i < j`), so cycles in
    /// `:require` automatically preserve exact `--jobs 1` DFS postorder.
    pub(super) fn extend_graph(&mut self, ordered: &[NamespaceSource]) {
        if ordered.len() <= self.indexed_len {
            return;
        }
        let old_len = self.indexed_len;
        let new_len = ordered.len();
        self.dependents.resize_with(new_len, Vec::new);
        self.unmet_deps.resize(new_len, 0);
        self.states.resize(new_len, SlotState::Pending);
        self.force_engine.resize(new_len, false);
        self.retries.resize(new_len, 0);
        self.needs_engine_cache.resize(new_len, None);

        for (j, file) in ordered.iter().enumerate().skip(old_len) {
            self.ns_index.insert(file.name.clone(), j);
        }

        let published_count = self.published_names.len();
        let mut seen = HashSet::new();
        for (j, file) in ordered.iter().enumerate().skip(old_len) {
            seen.clear();
            let mut add_dep = |ns: &str,
                               ns_index: &HashMap<String, usize>,
                               dependents: &mut [Vec<usize>],
                               unmet_deps: &mut [usize]| {
                if let Some(&dep) = ns_index.get(ns)
                    && dep < j
                    && seen.insert(dep)
                    && dep >= published_count
                {
                    dependents[dep].push(j);
                    unmet_deps[j] += 1;
                }
            };
            for req in &file.requires {
                add_dep(
                    req,
                    &self.ns_index,
                    &mut self.dependents,
                    &mut self.unmet_deps,
                );
            }
            for (_, target) in &file.aliases {
                add_dep(
                    target,
                    &self.ns_index,
                    &mut self.dependents,
                    &mut self.unmet_deps,
                );
            }
            for (_, target) in &file.namespace_imports {
                add_dep(
                    target,
                    &self.ns_index,
                    &mut self.dependents,
                    &mut self.unmet_deps,
                );
            }
            // Also depend on any earlier namespace directly named by a
            // qualified symbol `ns/sym` or dotted symbol `ns.sym` in the file,
            // even without an explicit `:require` (allowed in ClojureScript
            // when loaded earlier).
            for node in &file.syntax.nodes {
                if matches!(node.kind, NodeKind::Symbol) {
                    let text = &file.source[node.span.clone()];
                    if let Some((prefix, _)) = text.split_once('/')
                        && !prefix.is_empty()
                    {
                        add_dep(
                            prefix,
                            &self.ns_index,
                            &mut self.dependents,
                            &mut self.unmet_deps,
                        );
                    } else if let Some((prefix, member)) = text.rsplit_once('.')
                        && !prefix.is_empty()
                        && !member.is_empty()
                    {
                        add_dep(
                            prefix,
                            &self.ns_index,
                            &mut self.dependents,
                            &mut self.unmet_deps,
                        );
                    }
                }
            }
        }
        self.indexed_len = new_len;
    }

    /// Move finished worker outcomes from the shared completion vector into
    /// coordinator-owned `self.ready`, updating in-flight counts and scheduling
    /// retries where appropriate.
    fn reap_completed(&mut self, current_index: usize) {
        let drained = {
            let mut guard = self
                .completed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if guard.is_empty() {
                return;
            }
            std::mem::take(&mut *guard)
        };
        for (idx, outcome) in drained {
            if let SlotState::Running { needs_engine } = self.states[idx] {
                self.in_flight = self.in_flight.saturating_sub(1);
                if needs_engine {
                    self.in_flight_engine = self.in_flight_engine.saturating_sub(1);
                }
                self.states[idx] = SlotState::Done;
            }
            match outcome {
                TopLevelOutcome::NeedsEngine => {
                    if idx > current_index && self.pool_policy.engine_workers > 0 {
                        self.force_engine[idx] = true;
                        self.states[idx] = SlotState::Pending;
                    } else {
                        self.ready.insert(idx, TopLevelOutcome::NeedsEngine);
                    }
                }
                TopLevelOutcome::NeedCoordinator { retry } => {
                    if retry && idx > current_index && self.retries[idx] < MAX_RETRIES {
                        self.retries[idx] += 1;
                        self.states[idx] = SlotState::Pending;
                    } else {
                        self.ready
                            .insert(idx, TopLevelOutcome::NeedCoordinator { retry: false });
                    }
                }
                other => {
                    self.ready.insert(idx, other);
                }
            }
        }
    }

    /// Dispatch ready lookahead namespaces `(current_index + 1)..` onto the
    /// unified `UnitPool`. Never dispatches `current_index` itself so the
    /// coordinator runs `current_index` inline in parallel with workers when it
    /// was not already started in an earlier wave.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn dispatch_ready(
        &mut self,
        current_index: usize,
        entry_index: usize,
        ordered: &[NamespaceSource],
        analysis: &mut NamespaceAnalysis,
        macros: &MacroHost,
        build: &BuildGuard,
        options: AnalysisOptions,
        recording_cfg: UnitRecording,
        skip_speculation: impl Fn(&str) -> bool,
    ) {
        self.reap_completed(current_index);
        if self.in_flight >= self.max_in_flight {
            return;
        }
        let Some(session) = macros.session_handle() else {
            return;
        };
        let buffered_full = self.ready.len() >= MAX_BUFFERED;
        let has_engine_room = self.pool_policy.engine_workers > 0
            && self.in_flight_engine < self.max_in_flight_engine;
        let end = if buffered_full && !has_engine_room {
            ordered.len().min(current_index + 1 + NEAR_LOOKAHEAD)
        } else {
            ordered.len().min(current_index + 1 + LOOKAHEAD)
        };
        if current_index + 1 >= end {
            return;
        }

        // Lazily construct one shared O(1) wave snapshot only if at least one
        // lookahead namespace is actually eligible for dispatch.
        let mut wave = None;

        let macro_version = macros.tables.macro_version;
        let data_readers_now = macros.data_reader_count();
        for (j, file) in ordered.iter().enumerate().take(end).skip(current_index + 1) {
            if self.in_flight >= self.max_in_flight {
                break;
            }
            if self.states[j] != SlotState::Pending || self.unmet_deps[j] != 0 {
                continue;
            }
            // Do not speculatively analyze warm cache candidates; they go
            // through cache lookup when canonical `index` reaches them.
            if skip_speculation(&file.name) {
                continue;
            }
            let class = if self.force_engine[j] {
                TopLevelClass::Engine
            } else if let Some((v, r, cached)) = self.needs_engine_cache[j]
                && v == macro_version
                && r == data_readers_now
            {
                cached
            } else {
                let computed = classify_top_level(file, macros, options.warning_mode);
                self.needs_engine_cache[j] = Some((macro_version, data_readers_now, computed));
                computed
            };
            let needs_engine = match class {
                TopLevelClass::CoordinatorOnly => continue,
                TopLevelClass::Engine => true,
                TopLevelClass::Plain => false,
            };
            if needs_engine {
                if self.pool_policy.engine_workers == 0
                    || self.in_flight_engine >= self.max_in_flight_engine
                {
                    continue;
                }
            } else if buffered_full && j >= current_index + 1 + NEAR_LOOKAHEAD {
                continue;
            }
            if self.ready.len() >= MAX_BUFFERED * 2 && j >= current_index + 1 + NEAR_LOOKAHEAD {
                continue;
            }
            // In `warning_mode`, each namespace drains all its units inline, so
            // pre-emptively dispatching every ready namespace to pool workers
            // starves the coordinator into blocking on `take_or_wait` (most
            // severely at `--jobs 2`, where 1 worker serializes the whole wave).
            // Reserve 1 ready namespace per `workers` worker dispatches so the
            // coordinator and workers share waves concurrently.
            if options.warning_mode {
                let (quota, refill) = if needs_engine {
                    (&mut self.warn_engine_quota, self.pool_policy.engine_workers)
                } else {
                    (&mut self.warn_plain_quota, self.pool_policy.workers.max(1))
                };
                if *quota == 0 {
                    self.states[j] = SlotState::Coordinator;
                    *quota = refill;
                    continue;
                }
            }

            let (snap, env, marks, globals_len, data_readers) = wave.get_or_insert_with(|| {
                let snap = analysis.wave_snapshot();
                let env = macros.environment();
                let marks = session.shared().overlay_snapshot_marks();
                let globals_len = analysis.globals_len();
                let data_readers = macros.data_reader_count();
                (snap, env, marks, globals_len, data_readers)
            });

            // Make `light.build` aware of `j`'s source metadata ahead of time
            // without moving the coordinator's current `window`.
            build.register_source(j, &file.name, file.path.to_str());

            let job = snap.job(j, self.slots);
            let file_clone = file.clone();
            let env_clone = env.clone();
            let (prefix_cursor, prefix_bundles, prefix_env_bundles) = *marks;
            let snap_globals_len = *globals_len;
            let snap_data_readers = *data_readers;
            let snap_foreign_epoch = self.foreign_mutation_epoch;
            let pool_policy = self.pool_policy;
            let is_entry = j == entry_index;
            let completed = self.completed.clone();
            let shared_for_wake: Arc<SharedCatalog> = session.shared().clone();

            let submitted = session.submit_namespace(
                j,
                needs_engine,
                Box::new(move |ctx| {
                    let Some(ctx) = ctx else {
                        completed
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .push((j, TopLevelOutcome::NeedCoordinator { retry: false }));
                        shared_for_wake.notify_settled();
                        return false;
                    };
                    if needs_engine && !ctx.has_engine {
                        completed
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .push((j, TopLevelOutcome::NeedCoordinator { retry: false }));
                        shared_for_wake.notify_settled();
                        return false;
                    }
                    let overlay_synced = if ctx.has_engine {
                        match ctx.view.install_up_to(prefix_cursor, prefix_bundles) {
                            Ok(Some(active)) => active,
                            Ok(None) | Err(_) => {
                                completed
                                    .lock()
                                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                                    .push((j, TopLevelOutcome::NeedCoordinator { retry: true }));
                                shared_for_wake.notify_settled();
                                return false;
                            }
                        }
                    } else {
                        false
                    };

                    let mut frame = ctx.view.engine_frame();
                    let bootstrap = frame.bootstrap.clone();
                    let (charged_exp, charged_nodes) = ctx.shared.charged();
                    frame.expansions = std::rc::Rc::new(std::cell::Cell::new(charged_exp));
                    frame.generated_nodes = std::rc::Rc::new(std::cell::Cell::new(charged_nodes));
                    frame.work = Default::default();
                    frame.forbid_engine_boot.set(!ctx.has_engine);
                    frame.entered_engine.set(false);
                    frame.reflected_namespaces.borrow_mut().clear();
                    frame.reader_declared = Default::default();

                    let ns_session = CompilationSession::new_namespace_overlay(
                        ctx.shared.clone(),
                        ctx.view.thread_id(),
                        bootstrap,
                        prefix_cursor,
                        prefix_bundles,
                        prefix_env_bundles,
                        pool_policy,
                        recording_cfg.capture,
                        overlay_synced,
                    );
                    frame.session = Some(ns_session.clone());
                    let _view_guard = ctx.view.enter_namespace(ns_session.clone(), j as u32);
                    let before_env = env_clone.clone();
                    let mut worker_host = MacroHost::from_parts(env_clone, frame);
                    // `MacroHost::environment()` sets `bootstrap_seed = true` for
                    // deferred unit frames; a top-level namespace pass must keep
                    // `defers_source_units() == true` just like the coordinator.
                    worker_host.tables.bootstrap_seed = false;

                    let outcome_res =
                        prepare_source(&mut worker_host, &file_clone).and_then(|resolution| {
                            let input = NamespaceInput {
                                name: &file_clone.name,
                                source: &file_clone.source,
                                syntax: &file_clone.syntax,
                                aliases: &file_clone.aliases,
                                requires: &file_clone.requires,
                            };
                            let captured_resolution = resolution.clone();
                            job.analyze_overlay(
                                &input,
                                resolution,
                                is_entry,
                                &mut worker_host,
                                options,
                                recording_cfg,
                            )
                            .map(|publication| (publication, captured_resolution))
                        });

                    let entered_engine = worker_host.engine.entered_engine.get();
                    let reflected_namespaces =
                        worker_host.engine.reflected_namespaces.borrow().clone();
                    let data_readers_after = worker_host.data_reader_count();
                    let clear_ok = ns_session.clear_engine_overlay().is_ok();
                    drop(_view_guard);
                    let overlay = ns_session.take_namespace_overlay();
                    let retire = overlay.touched_local_in_engine || !clear_ok;

                    let outcome = if overlay.reflected_in_engine {
                        TopLevelOutcome::NeedCoordinator { retry: false }
                    } else {
                        match outcome_res {
                            Ok((publication, resolution)) => {
                                if data_readers_after != snap_data_readers {
                                    TopLevelOutcome::NeedCoordinator { retry: false }
                                } else {
                                    let (metadata, mutated_foreign_tables) = worker_host
                                        .namespace_metadata_since(&file_clone.name, &before_env);
                                    if mutated_foreign_tables || metadata.defines_macros() {
                                        TopLevelOutcome::NeedCoordinator { retry: false }
                                    } else {
                                        TopLevelOutcome::Ready(Box::new(CompletedNamespace {
                                            ready: ReadyNamespace {
                                                publication,
                                                metadata,
                                                overlay,
                                                resolution,
                                                snap_globals_len,
                                            },
                                            snap_index: current_index,
                                            snap_globals_len,
                                            snap_env_bundles: prefix_env_bundles,
                                            snap_data_readers,
                                            snap_foreign_epoch,
                                            entered_engine,
                                            reflected_namespaces,
                                        }))
                                    }
                                }
                            }
                            Err(error) => {
                                if error
                                    .to_string()
                                    .contains(crate::macro_host::NEEDS_ENGINE_SENTINEL)
                                {
                                    TopLevelOutcome::NeedsEngine
                                } else {
                                    // Replay any failing namespace on the coordinator so
                                    // partial macro-call stats, node peaks, warnings, and
                                    // error precedence match `--jobs 1` exactly.
                                    TopLevelOutcome::NeedCoordinator { retry: false }
                                }
                            }
                        }
                    };
                    completed
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push((j, outcome));
                    shared_for_wake.notify_settled();
                    retire
                }),
            );

            if submitted {
                self.states[j] = SlotState::Running { needs_engine };
                self.in_flight += 1;
                if needs_engine {
                    self.in_flight_engine += 1;
                    if options.warning_mode {
                        self.warn_engine_quota = self.warn_engine_quota.saturating_sub(1);
                    }
                } else if options.warning_mode {
                    self.warn_plain_quota = self.warn_plain_quota.saturating_sub(1);
                }
            }
        }
    }

    /// Check whether canonical namespace `index` was analyzed in parallel:
    /// - If `Pending` (not yet started by a worker), return `Ok(None)` right
    ///   away so the coordinator analyzes it inline in parallel with workers.
    /// - If `Running`, block waiting for its worker (servicing any forwarded
    ///   `light.build` requests so workers never deadlock).
    /// - Once `Done`, validate freshness against everything published since its
    ///   snapshot; return `Ok(Some(Ok(ready)))`, `Ok(Some(Err(err)))`, or
    ///   `Ok(None)` if invalidated (clean serial fallback).
    pub(super) fn take_or_wait(
        &mut self,
        index: usize,
        file: &NamespaceSource,
        analysis: &NamespaceAnalysis,
        macros: &MacroHost,
    ) -> Result<Option<Result<ReadyNamespace, AnalysisError>>, String> {
        self.reap_completed(index);
        if matches!(self.states.get(index), Some(SlotState::Running { .. })) {
            let _wait_trace = crate::compile_trace::wait(index);
            let session = macros.session_handle().expect("compilation session");
            let completed = self.completed.clone();
            session.wait_for_namespace_ready(macros, || {
                let guard = completed
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard.iter().any(|(idx, _)| *idx == index)
            })?;
            drop(_wait_trace);
            self.reap_completed(index);
        }
        let Some(outcome) = self.ready.remove(&index) else {
            return Ok(None);
        };
        let session = macros.session_handle().expect("compilation session");
        match outcome {
            TopLevelOutcome::Ready(done) => {
                if self.is_fresh(
                    index,
                    &file.name,
                    done.snap_index,
                    done.snap_globals_len,
                    done.snap_env_bundles,
                    done.snap_data_readers,
                    done.snap_foreign_epoch,
                    done.entered_engine,
                    &done.reflected_namespaces,
                    &done.ready.publication,
                    analysis,
                    macros,
                    &session,
                ) {
                    Ok(Some(Ok(done.ready)))
                } else {
                    Ok(None)
                }
            }
            TopLevelOutcome::NeedsEngine | TopLevelOutcome::NeedCoordinator { .. } => Ok(None),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn is_fresh(
        &self,
        index: usize,
        namespace: &str,
        snap_index: usize,
        snap_globals_len: usize,
        snap_env_bundles: usize,
        snap_data_readers: usize,
        snap_foreign_epoch: u64,
        entered_engine: bool,
        reflected_namespaces: &[String],
        publication: &NamespacePublication,
        analysis: &NamespaceAnalysis,
        macros: &MacroHost,
        session: &CompilationSession,
    ) -> bool {
        if snap_foreign_epoch != self.foreign_mutation_epoch {
            return false;
        }
        if snap_data_readers != macros.data_reader_count() {
            return false;
        }
        if entered_engine && snap_env_bundles != session.environment_bundles() {
            return false;
        }
        if analysis.has_global_in_namespace_since(snap_globals_len, namespace) {
            return false;
        }
        if analysis.has_any_global_key_since(
            snap_globals_len,
            publication.added_global_keys(snap_globals_len),
        ) {
            return false;
        }
        if snap_index < index {
            let published_since = &self.published_names[snap_index..index];
            for reflected in reflected_namespaces {
                if reflected == "*" || published_since.iter().any(|p| p == reflected) {
                    return false;
                }
            }
            for absent in publication.absent_namespaces() {
                if published_since.iter().any(|p| p == absent) {
                    return false;
                }
            }
        }
        true
    }

    /// Record that canonical namespace `index` (`name`) has now been published
    /// (whether via parallel overlay or serial fallback), unlocking its direct
    /// dependents in the DAG.
    pub(super) fn note_published(&mut self, index: usize, name: &str, mutated_foreign_ns: bool) {
        debug_assert_eq!(self.published_names.len(), index);
        self.published_names.push(name.to_owned());
        if mutated_foreign_ns {
            self.foreign_mutation_epoch += 1;
        }
        if let Some(state) = self.states.get_mut(index) {
            *state = SlotState::Done;
        }
        if let Some(deps) = self.dependents.get(index) {
            for &dep in deps {
                self.unmet_deps[dep] = self.unmet_deps[dep].saturating_sub(1);
            }
        }
    }
}

/// Fast static classifier predicting whether `file`'s top-level pass will
/// invoke a compile-time JS/QuickJS engine (so macro-expanding top-level files
/// route straight to engine workers instead of bouncing off engine-less ones).
///
/// False negatives are 100% sound: an engine-less worker has
/// `forbid_engine_boot = true` and returns `NEEDS_ENGINE_SENTINEL`, cleanly
/// re-queuing the namespace to an engine worker (or falling back to the
/// coordinator).
fn classify_top_level(
    file: &NamespaceSource,
    macros: &MacroHost,
    warning_mode: bool,
) -> TopLevelClass {
    let mut needs_engine = macros.data_reader_count() > 0
        && file
            .syntax
            .nodes
            .iter()
            .any(|node| matches!(node.kind, NodeKind::Tagged { .. }));

    for &root in &file.syntax.roots {
        match classify_form(file, macros, root, warning_mode) {
            TopLevelClass::CoordinatorOnly => return TopLevelClass::CoordinatorOnly,
            TopLevelClass::Engine => needs_engine = true,
            TopLevelClass::Plain => {}
        }
    }
    if needs_engine {
        TopLevelClass::Engine
    } else {
        TopLevelClass::Plain
    }
}

fn metadata_has_macro(file: &NamespaceSource, node: NodeId) -> bool {
    match &file.syntax.nodes[node.0].kind {
        NodeKind::Keyword { .. } => {
            &file.source[file.syntax.nodes[node.0].span.clone()] == ":macro"
        }
        NodeKind::Collection {
            kind: CollectionKind::Map,
            children,
        } => children
            .as_chunks::<2>()
            .0
            .iter()
            .any(|[key, _]| &file.source[file.syntax.nodes[key.0].span.clone()] == ":macro"),
        _ => false,
    }
}

fn merge_class(a: TopLevelClass, b: TopLevelClass) -> TopLevelClass {
    match (a, b) {
        (TopLevelClass::CoordinatorOnly, _) | (_, TopLevelClass::CoordinatorOnly) => {
            TopLevelClass::CoordinatorOnly
        }
        (TopLevelClass::Engine, _) | (_, TopLevelClass::Engine) => TopLevelClass::Engine,
        (TopLevelClass::Plain, TopLevelClass::Plain) => TopLevelClass::Plain,
    }
}

fn classify_form(
    file: &NamespaceSource,
    macros: &MacroHost,
    mut node: NodeId,
    inspect_deferred_bodies: bool,
) -> TopLevelClass {
    while let NodeKind::Metadata { metadata, target } = file.syntax.nodes[node.0].kind {
        if metadata_has_macro(file, metadata) {
            return TopLevelClass::CoordinatorOnly;
        }
        node = target;
    }
    match &file.syntax.nodes[node.0].kind {
        NodeKind::Collection {
            kind: CollectionKind::List,
            children,
        } => {
            let Some(&head) = children.first() else {
                return TopLevelClass::Plain;
            };
            let head_unwrapped = unwrap_meta(file, head);
            if !matches!(file.syntax.nodes[head_unwrapped.0].kind, NodeKind::Symbol) {
                return children.iter().fold(TopLevelClass::Plain, |acc, &child| {
                    merge_class(
                        acc,
                        classify_form(file, macros, child, inspect_deferred_bodies),
                    )
                });
            }
            let op = &file.source[file.syntax.nodes[head_unwrapped.0].span.clone()];
            let unqualified = op
                .strip_prefix("cljs.core/")
                .or_else(|| op.strip_prefix("clojure.core/"))
                .unwrap_or(op);

            match unqualified {
                "ns" | "ns*" | "comment" | "quote" => return TopLevelClass::Plain,
                "do" => {
                    return children[1..]
                        .iter()
                        .fold(TopLevelClass::Plain, |acc, &child| {
                            merge_class(
                                acc,
                                classify_form(file, macros, child, inspect_deferred_bodies),
                            )
                        });
                }
                "defmacro" => return TopLevelClass::CoordinatorOnly,
                "def" | "defn" | "defn-" | "defonce" | "declare" | "defprotocol" | "defrecord"
                | "deftype" | "extend-type" | "extend-protocol" | "defmulti" | "defmethod" => {
                    if let Some(&name_node) = children.get(1)
                        && let NodeKind::Metadata { metadata, .. } =
                            file.syntax.nodes[name_node.0].kind
                        && metadata_has_macro(file, metadata)
                    {
                        return TopLevelClass::CoordinatorOnly;
                    }
                    if !inspect_deferred_bodies {
                        return TopLevelClass::Plain;
                    }
                    return children[1..]
                        .iter()
                        .fold(TopLevelClass::Plain, |acc, &child| {
                            merge_class(acc, classify_form(file, macros, child, true))
                        });
                }
                _ => {}
            }

            let mut acc = if operator_is_js_macro(file, macros, op) {
                TopLevelClass::Engine
            } else {
                TopLevelClass::Plain
            };

            for &child in &children[1..] {
                acc = merge_class(
                    acc,
                    classify_form(file, macros, child, inspect_deferred_bodies),
                );
                if acc == TopLevelClass::CoordinatorOnly {
                    return acc;
                }
            }
            acc
        }
        NodeKind::Collection { children, .. } => {
            children.iter().fold(TopLevelClass::Plain, |acc, &child| {
                merge_class(
                    acc,
                    classify_form(file, macros, child, inspect_deferred_bodies),
                )
            })
        }
        NodeKind::Prefix { kind, form } => {
            if *kind == crate::PrefixKind::Quote {
                TopLevelClass::Plain
            } else {
                classify_form(file, macros, *form, inspect_deferred_bodies)
            }
        }
        NodeKind::Tagged { form, .. } => {
            classify_form(file, macros, *form, inspect_deferred_bodies)
        }
        NodeKind::NamespacedMap { map, .. } => {
            classify_form(file, macros, *map, inspect_deferred_bodies)
        }
        _ => TopLevelClass::Plain,
    }
}

fn unwrap_meta(file: &NamespaceSource, mut node: NodeId) -> NodeId {
    while let NodeKind::Metadata { target, .. } = file.syntax.nodes[node.0].kind {
        node = target;
    }
    node
}

fn operator_is_js_macro(file: &NamespaceSource, macros: &MacroHost, op: &str) -> bool {
    if let Some((prefix, member)) = op.split_once('/') {
        let target_ns = file
            .aliases
            .iter()
            .find(|(alias, _)| alias == prefix)
            .map(|(_, ns)| ns.as_str())
            .unwrap_or(prefix);
        return macros.has_javascript_macro(target_ns, member);
    }
    // Check explicit `:refer` / `:rename` specs in this namespace header.
    for spec in &file.runtime_specs {
        for original in &spec.import.refers {
            let local = spec
                .renames
                .iter()
                .find(|(from, _)| from == original)
                .map(|(_, to)| to.as_str())
                .unwrap_or(original.as_str());
            if local == op {
                return macros.has_javascript_macro(&spec.import.namespace, original);
            }
        }
    }
    // Check `:refer-clojure :rename` / core refers.
    for (local, ns, original) in &file.resolution.refers {
        if local == op {
            return macros.has_javascript_macro(ns, original);
        }
    }
    macros.has_javascript_macro("cljs.core", op)
}
