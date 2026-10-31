//! What a namespace compilation did and how long each phase took: the
//! numbers `--stats` prints (docs/compiler-cache.md, "Implementation
//! sequence", step 1). A compilation collects them only when its caller
//! passes a [`CompileStats`]; without one it reads no clock and records
//! nothing.
//!
//! The report is one `key value` line per counter, then the table of the
//! namespaces with the most macro calls under a `[macro-calls-by-namespace]`
//! line, one `NAMESPACE COUNT` line each. Times are milliseconds with one
//! decimal. Later counters, such as the cache's, are added as further
//! `key value` lines ahead of the table; the `memory.*` lines come last.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// The version of the report's format, its first line.
pub const REPORT_VERSION: u32 = 1;

/// How many namespaces the report's table lists.
pub const REPORT_NAMESPACES: usize = 20;

/// Native form bridge work and collection diagnostics for one macro expansion.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NativeBridgeCall {
    pub imported: usize,
    pub exported: usize,
    pub reused: usize,
    /// Bridge time, excluding JavaScript execution.
    pub nanos: u128,
    pub collections: u64,
    pub gc_pause_nanos: u128,
    /// Runtime high-water pause through this call, not a per-call delta.
    pub gc_pause_max_nanos: u128,
    pub gc_live_objects: usize,
    /// Accounted bytes under the memory limit after this call, runtime-wide
    /// and including unswept garbage (docs/native-macros.md, "Memory limit").
    pub gc_managed_bytes: usize,
    /// Runtime high-water accounted bytes through this call.
    pub gc_peak_managed_bytes: usize,
    /// Accounted bytes left after the runtime's last collection.
    pub gc_live_bytes: usize,
    /// Bytes admitted during this call, by every task of the runtime.
    pub gc_admitted_bytes: usize,
    /// The runtime's accounted-byte limit; 0 when it has none.
    pub gc_byte_limit: usize,
    pub helper_compiled: usize,
    pub helper_published: usize,
    pub helper_per_task: usize,
    pub helper_waits: usize,
}

/// Actual native-runtime work across all tasks, counted once per event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NativeHelperCounts {
    /// One snapshot of the shared heap, including pooled tasks. Per-call
    /// samples alone omit calls whose worker-local host has been dropped.
    pub heap: Option<NativeHeapStats>,
    pub compiled: usize,
    pub published: usize,
    pub per_task: usize,
    pub waits: usize,
    pub steps: usize,
    pub migrations: usize,
    /// Task contexts booted on the runtime: the coordinator's, each
    /// worker's and each nested producer's.
    pub contexts: usize,
    /// Distinct unit sources compiled once into a shared factory.
    pub units_compiled: usize,
    /// Unit evaluations served by a shared factory.
    pub units_shared: usize,
    /// Unit evaluations compiled by the evaluating task itself.
    pub units_evaluated: usize,
    /// Task contexts pool slots parked between jobs for others to borrow.
    pub contexts_lent: usize,
    /// Parked task contexts a pool slot borrowed instead of booting one.
    pub contexts_borrowed: usize,
    /// Parked task steps a completion queued on the pool from its wakeups.
    pub woken: usize,
    /// The bridge's prune walk and `&env` cache, counted only while a
    /// trace records (`--stats`).
    pub bridge: NativeBridgeCounters,
    /// Task steps after which the host read the task's dependency log, the
    /// edges those reads copied and the edges they handed to the dependency
    /// graph, counted only while a trace records (`--stats`).
    pub log_reads: usize,
    pub log_edges_copied: usize,
    pub log_edges_recorded: usize,
    /// Catalog installs into a task (each applies the registrations after
    /// its task's frontier, or a producer adopts its parent's catalog), the
    /// registrations they applied, including those producers applied on
    /// first use of a name or protocol, and the installs' time, counted only
    /// while a trace records (`--stats`).
    pub catalog_uses: usize,
    pub catalog_entries: usize,
    pub catalog_nanos: usize,
    /// Helper producer tasks set up, the standalone registrations their
    /// setup replayed, the catalog registrations they applied (on first use
    /// of a name or protocol of the catalog they adopted, light-cljs-590),
    /// and the setup's time, counted only while a trace records.
    pub producers: usize,
    pub producer_replayed: usize,
    pub producer_catalog_entries: usize,
    pub producer_nanos: usize,
    /// The real cljs.core of `--macro-core real` (light-cljs-648); none
    /// with the port.
    pub real_core: Option<RealCoreCounts>,
}

/// The real cljs.core one compilation booted (`--macro-core real`;
/// `src/bbp_evaluator/real_core.rs`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RealCoreCounts {
    /// Members of the profile's hub (its per-task defaults included,
    /// the evaluator's compile-time members not), and those that published.
    pub members: usize,
    pub published: usize,
    /// Tasks that booted a private copy of the profile for a member that
    /// did not publish (the per-task fallback).
    pub private_boots: usize,
    /// The boot task's time, from spawn to the published hub.
    pub boot_nanos: u64,
}

/// What the Pants build's form bridge did to keep its import memo and its
/// one `&env` envelope, runtime-wide (`src/bbp_evaluator.rs`, `Bridge`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NativeBridgeCounters {
    /// Prune walks: one on the first import after each collection.
    pub prune_passes: usize,
    /// Imports examined following reclamation notifications, and those dropped.
    pub prune_imports_visited: usize,
    pub prune_imports_removed: usize,
    /// Datum nodes traversed to rediscover reachability. Cached child links
    /// eliminate this walk; keep the counter for comparison with older builds.
    pub prune_nodes_walked: usize,
    /// Imported roots and cached child links released. Children are examined
    /// only when the last reference to their parent dies.
    pub prune_references_released: usize,
    /// Form, cost and native-object entries the walks dropped.
    pub prune_entries_removed: usize,
    pub prune_nanos: u128,
    /// `&env` imports served by the envelope as it is (the same Datum).
    pub env_hits: usize,
    /// `&env` imports that rebuilt the envelope from the previous one, and
    /// those with no previous envelope (a full import).
    pub env_misses: usize,
    pub env_full: usize,
    /// Rebuilds that replaced a different envelope.
    pub env_evictions: usize,
    /// Envelope keys the rebuilds assoc'd or dissoc'd.
    pub env_entries_rebuilt: usize,
    /// Bytes the engine admitted during `&env` imports (runtime-wide, so
    /// exact only on one thread) and the time those imports took.
    pub env_bytes_rebuilt: usize,
    pub env_nanos: u128,
}

/// Aggregate native heap accounting; this is not process RSS.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NativeHeapStats {
    pub collections: u64,
    /// Edges every collection so far traced: the total marking work.
    pub traced_edges: u64,
    pub live_objects: usize,
    pub managed_bytes: usize,
    pub peak_managed_bytes: usize,
    pub live_bytes: usize,
    pub admitted_bytes: usize,
    pub byte_limit: usize,
}

/// Wall time per phase of a compilation. The phases do not overlap; work a
/// phase demands of another (such as a deferred unit a macro needs while a
/// namespace's top level is analyzed) counts in the phase that demanded it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PhaseTimes {
    /// Locating, reading and parsing sources and their `ns` forms.
    pub resolve_parse: Duration,
    /// Analyzing each namespace's forms with macro expansion, and closing
    /// the program, except draining its deferred units.
    pub analysis: Duration,
    /// Analyzing and preparing the deferred units each namespace leaves
    /// when its top level is done (the drain before the namespace returns).
    pub deferred_units: Duration,
    /// `prepare_application`.
    pub prepare_application: Duration,
    /// Emitting JavaScript and source maps.
    pub emission: Duration,
    /// Writing the output to stdout or files.
    pub write: Duration,
}

impl PhaseTimes {
    /// The sum of the phases.
    pub fn total(&self) -> Duration {
        self.resolve_parse
            + self.analysis
            + self.deferred_units
            + self.prepare_application
            + self.emission
            + self.write
    }
}

/// One namespace of a compilation, in the order of analysis.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NamespaceStats {
    pub name: String,
    /// Macro calls executed while the namespace was analyzed, its deferred
    /// units included: native, bootstrap and source macros alike, the
    /// expansions `--macro-expansions` limits.
    pub macro_calls: usize,
}

/// The deferred compilation units of a compilation
/// (`src/compilation_session.rs`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeferredUnitCounts {
    /// Units registered: compile-time definitions, each analyzed only on
    /// demand or when its namespace drains.
    pub registered: usize,
    /// Units analyzed. A build that fails before its final analysis
    /// counts those up to the last unit a drain point took or merged after
    /// an early warning-mode drain, in unit order, which no schedule changes.
    pub analyzed: usize,
    /// Analyzed units whose compile-time JavaScript was prepared and whose
    /// IR moved into the program, with the same failed-build cutoff as `analyzed`.
    pub prepared: usize,
    /// Units a macro or another unit executed at compile time.
    pub executed: usize,
    /// Units found macro-free at registration, which `--jobs N` analyzes
    /// on pool workers (docs/compiler-concurrency.md, "As built
    /// (light-cljs-338)").
    pub macro_free: usize,
    /// The time spent analyzing macro-free units, on whichever thread.
    pub macro_free_time: std::time::Duration,
    /// With `--jobs N`, what the build's unit pool did
    /// (docs/compiler-concurrency.md, "As built (light-cljs-339)"): the
    /// units its drains gave it, the isolates its workers made and the
    /// time that took, the time its workers spent analyzing their jobs,
    /// and the compile-time units its workers' engines initialized, each
    /// once per isolate that demanded it.
    pub pooled: usize,
    /// Pooled units the Pants coordinator claimed before a worker started.
    pub coordinator_units: usize,
    pub worker_isolates: usize,
    pub worker_isolate_startup: std::time::Duration,
    pub worker_unit_time: std::time::Duration,
    pub worker_initializations: usize,
    /// The pool's worker threads, how long it ran, what its scheduler did
    /// (claims, steals, parks, queue depth and lock contention), and how
    /// often the unit catalog's lock was found held, on any thread
    /// (light-cljs-343).
    pub pool_workers: usize,
    /// Of the pool's worker threads, those with a compile-time engine, and
    /// the units the others analyzed, macro-free ones (light-cljs-437).
    pub pool_engine_workers: usize,
    /// Distinct pool slots that actually made an engine view or ran a
    /// shared-runtime continuation, rather than configured engine capacity.
    pub pool_engine_participants: usize,
    pub engine_less_units: usize,
    pub pool_time: std::time::Duration,
    pub pool: crate::scheduler::PoolCounts,
    pub catalog_contended: u64,
}

/// The numbers of one compilation; see the module documentation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompileStats {
    /// The Pants build's selected core, even when no macro task executes
    /// (a macro-free compilation or a rebuild using cached artifacts).
    /// Other macro engines have no core selection.
    pub macro_core: Option<crate::evaluator::MacroCore>,
    pub native_macro_calls: Vec<NativeBridgeCall>,
    pub native_helpers: NativeHelperCounts,
    /// The native backend's effect lanes (light-cljs-480).
    pub lanes: crate::effect_lanes::LaneCounts,
    pub phases: PhaseTimes,
    /// From the start of the compilation to the end of its last recorded
    /// phase.
    pub wall: Duration,
    /// The namespaces compiled, by source index.
    pub namespaces: Vec<NamespaceStats>,
    /// Macro calls executed in the whole compilation. The expansions
    /// installed namespaces charge against `--macro-expansions` are not
    /// executed and not counted.
    pub macro_calls: usize,
    /// The most form nodes macro expansions and compile-time data readers
    /// had generated, the count `--macro-nodes` limits, including the nodes
    /// installed namespaces charged.
    pub peak_macro_nodes: usize,
    pub deferred_units: DeferredUnitCounts,
    /// What the parse queue's pool did, when it had workers.
    pub parse_pool: Option<crate::scheduler::PoolCounts>,
    /// `--watch`: loading the program into the watch's development session
    /// after the startup build.
    pub session_load: Option<Duration>,
    /// When namespace artifacts were captured
    /// ([`crate::compiler::analyze_namespace_entry_with_artifacts`]).
    pub capture: Option<CaptureCounts>,
    /// When namespaces were installed from artifacts
    /// ([`crate::compiler::analyze_namespace_entry_recorded`]).
    pub install: Option<InstallCounts>,
    /// When captured artifacts were also encoded and decoded
    /// ([`crate::artifact_codec`]), to measure the binary format.
    pub codec: Option<CodecCounts>,
    /// When every namespace's cache key was computed and its inputs
    /// re-validated ([`crate::cache_key`]), to measure them.
    pub keys: Option<KeyCounts>,
    /// What the persistent cache did (`--cache-dir`,
    /// [`crate::compile_cache`]).
    pub cache: Option<CacheCounts>,
    /// The exclusive-stage trace of the analysis ([`crate::compile_trace`]).
    pub trace: Option<crate::compile_trace::Trace>,
    /// What the compilation held in memory at its peaks.
    pub memory: MemoryCounts,
}

/// What a compilation held in memory: the process's peak resident set and,
/// estimated from table lengths times element sizes, the most parsed
/// syntax trees and the most analyzed unit IR waiting to merge it held at
/// once (README `--stats`; docs/compiler-concurrency.md, "As built
/// (light-cljs-376)").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemoryCounts {
    /// The process's peak resident set size so far, in bytes
    /// ([`peak_rss`]), where the platform reports one: set by the caller
    /// that reports the compilation, as the CLI does.
    pub peak_rss: Option<u64>,
    /// The loader's parsed sources: each from when it is parsed until its
    /// tree is released after its namespace's analysis.
    pub parsed_trees: ParsedTreePeak,
    pub queued_ir: QueuedIrPeak,
    /// What the process still held when the report was made
    /// ([`live_counts`]): set by the caller that reports the compilation, as
    /// the CLI does once the build's macro host is dropped.
    pub live: Option<LiveCounts>,
}

/// The most parsed syntax trees the loader held at once: their files, and
/// the bytes of their source text, nodes, roots and child lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ParsedTreePeak {
    pub bytes: usize,
    pub files: usize,
}

/// The most analyzed deferred-unit IR waiting to merge into the program at
/// once, and the most units: from when a unit's analysis settles, on
/// whichever thread, while the catalog holds it for its namespace's drain
/// and while the namespace holds the drained unit until its units merge
/// after the next namespace's top-level pass. Each unit counts the table
/// entries its fragment adds to its snapshot's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QueuedIrPeak {
    pub bytes: usize,
    pub units: usize,
}

/// The process's peak resident set size so far in bytes, from
/// `getrusage(RUSAGE_SELF)`, on macOS and Linux.
pub fn peak_rss() -> Option<u64> {
    #[cfg(all(
        any(target_os = "macos", target_os = "linux"),
        target_pointer_width = "64"
    ))]
    {
        /// `struct rusage` of 64-bit macOS and Linux: two `timeval`s, then
        /// fourteen `long`s, `ru_maxrss` first.
        #[repr(C)]
        struct Usage {
            times: [i64; 4],
            maxrss: i64,
            rest: [i64; 13],
        }
        unsafe extern "C" {
            fn getrusage(who: i32, usage: *mut Usage) -> i32;
        }
        let mut usage = Usage {
            times: [0; 4],
            maxrss: 0,
            rest: [0; 13],
        };
        // SAFETY: `usage` is a writable `struct rusage`; RUSAGE_SELF is 0.
        if unsafe { getrusage(0, &mut usage) } != 0 {
            return None;
        }
        let maxrss = u64::try_from(usage.maxrss).ok()?;
        // Bytes on macOS, kilobytes on Linux.
        if cfg!(target_os = "macos") {
            Some(maxrss)
        } else {
            Some(maxrss * 1024)
        }
    }
    #[cfg(not(all(
        any(target_os = "macos", target_os = "linux"),
        target_pointer_width = "64"
    )))]
    None
}

/// What the process holds now that a finished compilation must have
/// released, counted as each is made and as it is dropped
/// (docs/compiler-cache.md, "Validation and performance gates", the row
/// "Repeated builds and teardown"). Once a build's program, host and
/// warnings are dropped, every count is zero again; a development session
/// keeps its engine, and what that engine still holds, until it is dropped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LiveCounts {
    /// [`crate::namespace_artifact::NamespaceArtifact`]s captured, decoded
    /// or cloned and not yet dropped.
    pub artifacts: usize,
    /// JavaScript engines (a shared Pants runtime, V8 isolate or QuickJS runtime)
    /// made for macros, deferred units or a session and not yet dropped.
    pub engines: usize,
    /// Persistent handles Rust holds into an engine's heap: V8 `Global`s (a
    /// context, its services, cached namespace functions, the bridge's
    /// provenance template, a failed unit's exception). QuickJS keeps none.
    pub engine_handles: usize,
    /// Rust data an engine's objects own through the form bridge: the
    /// provenance of each object a macro call imported, freed when the
    /// engine collects the object or is torn down.
    pub bridge_payloads: usize,
}

/// The counters of [`LiveCounts`] besides the artifacts', which
/// `crate::namespace_artifact` keeps.
pub(crate) static LIVE_ENGINES: AtomicUsize = AtomicUsize::new(0);
pub(crate) static LIVE_ENGINE_HANDLES: AtomicUsize = AtomicUsize::new(0);
pub(crate) static LIVE_BRIDGE_PAYLOADS: AtomicUsize = AtomicUsize::new(0);

/// What the process holds now ([`LiveCounts`]). Reading is one relaxed load
/// a counter; making and dropping what one counts is one relaxed add.
pub fn live_counts() -> LiveCounts {
    LiveCounts {
        artifacts: crate::namespace_artifact::live_artifacts(),
        engines: LIVE_ENGINES.load(Ordering::Relaxed),
        engine_handles: LIVE_ENGINE_HANDLES.load(Ordering::Relaxed),
        bridge_payloads: LIVE_BRIDGE_PAYLOADS.load(Ordering::Relaxed),
    }
}

impl LiveCounts {
    /// `artifacts=N engines=N engine-handles=N bridge-payloads=N`, as a
    /// `--watch --stats` step prints them.
    pub fn summary(&self) -> String {
        format!(
            "artifacts={} engines={} engine-handles={} bridge-payloads={}",
            self.artifacts, self.engines, self.engine_handles, self.bridge_payloads
        )
    }
}

/// Counts its owner in one of the [`LiveCounts`] counters from when it is
/// made until it is dropped. A build with neither engine (the wasm
/// playground) makes none.
#[cfg_attr(not(any(light_quickjs, light_v8)), allow(dead_code))]
pub(crate) struct Live(&'static AtomicUsize);

impl Live {
    #[cfg_attr(not(any(light_quickjs, light_v8)), allow(dead_code))]
    pub(crate) fn new(counter: &'static AtomicUsize) -> Self {
        counter.fetch_add(1, Ordering::Relaxed);
        Self(counter)
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// What a build's persistent cache did: each namespace is a hit, a miss
/// with the first reason its candidates could not be used, or a policy
/// bypass.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CacheCounts {
    pub hits: usize,
    /// `(namespace, kind, description)`, the kind being the first key
    /// component that differed (`source`, `dependencies`, ...) or
    /// `no-artifact`, `index`, `unreadable`, `undecodable`, `refused` or, on
    /// the Pants build, `core-write`.
    pub misses: Vec<(String, &'static str, String)>,
    /// `(namespace, reason, path)`, the path naming the namespace a
    /// propagated reason came from and those it came through.
    pub bypasses: Vec<(String, &'static str, String)>,
    /// Artifacts published, and their bytes.
    pub published: usize,
    pub bytes_written: u64,
    /// Missed namespaces that were not published, with the reason.
    pub unpublished: Vec<(String, String)>,
    /// Artifact bytes read, matching or not.
    pub bytes_read: u64,
    /// Reading indexes and artifacts, checking manifests and keys.
    pub validate_time: Duration,
    pub decode_time: Duration,
    /// Encoding and writing artifacts and indexes.
    pub write_time: Duration,
    /// What a `--compile --entry` build did with whole outputs, when it
    /// consulted them ([`crate::compile_cache::OutputCache`]).
    pub output: Option<OutputCounts>,
}

/// What one build did with whole outputs (light-cljs-418).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OutputCounts {
    /// It wrote a stored output instead of preparing and emitting.
    pub reused: bool,
    /// It stored the output it emitted.
    pub stored: bool,
    pub bytes_read: u64,
    pub bytes_written: u64,
    /// Reading, checking and writing stored outputs.
    pub time: Duration,
}

impl CacheCounts {
    /// The `key value` lines of the report.
    fn lines(&self, line: &mut dyn FnMut(&str, &dyn std::fmt::Display)) {
        let ms = |duration: Duration| format!("{:.1}", duration.as_secs_f64() * 1000.0);
        line("cache.hits", &self.hits);
        line("cache.misses", &self.misses.len());
        let mut kinds: Vec<&str> = self.misses.iter().map(|(_, kind, _)| *kind).collect();
        kinds.sort_unstable();
        for group in kinds.chunk_by(|a, b| a == b) {
            line(&format!("cache.miss.{}", group[0]), &group.len());
        }
        line("cache.bypasses", &self.bypasses.len());
        let mut reasons: Vec<&str> = self.bypasses.iter().map(|(_, reason, _)| *reason).collect();
        reasons.sort_unstable();
        for group in reasons.chunk_by(|a, b| a == b) {
            line(&format!("cache.bypass.{}", group[0]), &group.len());
        }
        line("cache.published", &self.published);
        line("cache.unpublished", &self.unpublished.len());
        line("cache.bytes-read", &self.bytes_read);
        line("cache.bytes-written", &self.bytes_written);
        line("cache.validate-ms", &ms(self.validate_time));
        line("cache.decode-ms", &ms(self.decode_time));
        line("cache.write-ms", &ms(self.write_time));
        if let Some(output) = &self.output {
            line("cache.output.reused", &u8::from(output.reused));
            line("cache.output.stored", &u8::from(output.stored));
            line("cache.output.bytes-read", &output.bytes_read);
            line("cache.output.bytes-written", &output.bytes_written);
            line("cache.output-ms", &ms(output.time));
        }
    }

    /// The `[cache-misses-and-bypasses]` rows: up to `limit` misses, then up
    /// to `limit` bypasses, each in source order, as `NAMESPACE miss KIND:
    /// DESCRIPTION` and `NAMESPACE bypass REASON PATH`.
    fn rows(&self, out: &mut String, limit: usize) {
        for (name, kind, description) in self.misses.iter().take(limit) {
            let _ = writeln!(out, "{name} miss {kind}: {description}");
        }
        for (name, reason, path) in self.bypasses.iter().take(limit) {
            if path.is_empty() {
                let _ = writeln!(out, "{name} bypass {reason}");
            } else {
                let _ = writeln!(out, "{name} bypass {reason} {path}");
            }
        }
    }
}

/// What computing a compilation's namespace keys cost.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyCounts {
    pub namespaces: usize,
    /// The source text hashed, in bytes.
    pub source_bytes: u64,
    pub time: Duration,
    /// Namespaces whose inputs [`crate::cache_key::revalidate`] found
    /// changed after the compilation read them.
    pub changed: usize,
    pub revalidate_time: Duration,
}

/// What encoding and decoding a compilation's captured artifacts cost.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodecCounts {
    /// Artifacts encoded.
    pub artifacts: usize,
    /// Artifacts that failed to encode or to decode again, which a store
    /// would neither write nor read.
    pub failed: usize,
    /// Their encoded size in all.
    pub bytes: u64,
    /// The largest encoded artifact, and its namespace.
    pub largest: (u64, String),
    /// The source text of the encoded namespaces, in bytes.
    pub source_bytes: u64,
    /// Encoded bytes by section, in [`crate::artifact_codec::Section::ALL`]
    /// order; the header and manifest are the rest of [`Self::bytes`].
    pub sections: [u64; 6],
    /// The longest expression chain of any artifact
    /// ([`crate::namespace_artifact::NamespaceArtifact::ir_depth`]).
    pub ir_depth: u32,
    pub encode_time: Duration,
    pub decode_time: Duration,
}

/// The namespace artifacts a compilation installed in place of analysis.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InstallCounts {
    /// The namespaces installed, in source order. Their macro calls are not
    /// executed, so [`CompileStats::macro_calls_in`] counts none for them.
    pub installed: Vec<String>,
    /// The namespaces whose artifact was refused, with the reason; each was
    /// analyzed instead.
    pub refused: Vec<(String, String)>,
    /// Time spent installing, outside `analysis`.
    pub time: Duration,
}

/// The namespace artifacts a compilation captured.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CaptureCounts {
    /// Namespaces captured as artifacts.
    pub captured: usize,
    /// Namespaces whose artifact capture refused, with a reason.
    pub skipped: usize,
    /// The artifacts' estimated size
    /// ([`crate::namespace_artifact::NamespaceArtifact::approximate_bytes`]).
    pub bytes: usize,
    /// Time spent capturing, outside `analysis`.
    pub time: Duration,
}

impl CompileStats {
    pub fn new() -> Self {
        Self::default()
    }

    /// The macro calls namespace `name` executed, if it was compiled.
    pub fn macro_calls_in(&self, name: &str) -> Option<usize> {
        self.namespaces
            .iter()
            .find(|namespace| namespace.name == name)
            .map(|namespace| namespace.macro_calls)
    }

    /// The `--stats-file` trace, with the phases after analysis, when the
    /// compilation was traced.
    pub fn trace_file(&self) -> Option<String> {
        let phases = &self.phases;
        self.trace.as_ref().map(|trace| {
            trace.tsv(&[
                ("prepare-application", phases.prepare_application),
                ("emission", phases.emission),
                ("write", phases.write),
                ("wall", self.wall),
            ])
        })
    }

    /// The `--stats` report.
    pub fn report(&self) -> String {
        let mut out = String::new();
        let mut line = |key: &str, value: &dyn std::fmt::Display| {
            let _ = writeln!(out, "{key} {value}");
        };
        let ms = |duration: Duration| format!("{:.1}", duration.as_secs_f64() * 1000.0);
        line("light-cljs-stats", &REPORT_VERSION);
        line("time.wall-ms", &ms(self.wall));
        line("time.resolve-parse-ms", &ms(self.phases.resolve_parse));
        line("time.analysis-ms", &ms(self.phases.analysis));
        line("time.deferred-units-ms", &ms(self.phases.deferred_units));
        line(
            "time.prepare-application-ms",
            &ms(self.phases.prepare_application),
        );
        line("time.emission-ms", &ms(self.phases.emission));
        line("time.write-ms", &ms(self.phases.write));
        if let Some(duration) = self.session_load {
            line("time.session-load-ms", &ms(duration));
        }
        line("namespaces", &self.namespaces.len());
        line("macro-calls", &self.macro_calls);
        for (i, call) in self.native_macro_calls.iter().enumerate() {
            line(&format!("macro.bridge.{i}.imported"), &call.imported);
            line(&format!("macro.bridge.{i}.exported"), &call.exported);
            line(&format!("macro.bridge.{i}.reused"), &call.reused);
            line(&format!("macro.bridge.{i}.ns"), &call.nanos);
            line(&format!("macro.gc.{i}.collections"), &call.collections);
            line(
                &format!("macro.gc.{i}.pause-total-ns"),
                &call.gc_pause_nanos,
            );
            line(
                &format!("macro.gc.{i}.pause-max-ns"),
                &call.gc_pause_max_nanos,
            );
            line(&format!("macro.gc.{i}.live-objects"), &call.gc_live_objects);
            if call.gc_byte_limit > 0 {
                line(
                    &format!("macro.gc.{i}.managed-bytes"),
                    &call.gc_managed_bytes,
                );
                line(
                    &format!("macro.gc.{i}.peak-managed-bytes"),
                    &call.gc_peak_managed_bytes,
                );
                line(&format!("macro.gc.{i}.live-bytes"), &call.gc_live_bytes);
                line(
                    &format!("macro.gc.{i}.admitted-bytes"),
                    &call.gc_admitted_bytes,
                );
                line(&format!("macro.gc.{i}.byte-limit"), &call.gc_byte_limit);
            }
            line(
                &format!("macro.helpers.{i}.compiled"),
                &call.helper_compiled,
            );
            line(
                &format!("macro.helpers.{i}.published"),
                &call.helper_published,
            );
            line(
                &format!("macro.helpers.{i}.per-task"),
                &call.helper_per_task,
            );
            line(&format!("macro.helpers.{i}.waits"), &call.helper_waits);
        }
        if let Some(core) = self.macro_core {
            line(
                "native.core.real",
                &usize::from(core == crate::evaluator::MacroCore::Real),
            );
        }
        if self.native_helpers.steps > 0 {
            let helpers = &self.native_helpers;
            if let Some(heap) = helpers.heap {
                line("macro.gc.collections", &heap.collections);
                line("macro.gc.traced-edges", &heap.traced_edges);
                line("macro.gc.live-objects", &heap.live_objects);
                line("macro.gc.managed-bytes", &heap.managed_bytes);
                line("macro.gc.peak-managed-bytes", &heap.peak_managed_bytes);
                line("macro.gc.live-bytes", &heap.live_bytes);
                line("macro.gc.admitted-bytes", &heap.admitted_bytes);
                line("macro.gc.byte-limit", &heap.byte_limit);
            }
            line("native.helpers.compiled", &helpers.compiled);
            line("native.helpers.published", &helpers.published);
            line("native.helpers.per-task", &helpers.per_task);
            line("native.helpers.waits", &helpers.waits);
            line("native.tasks.steps", &helpers.steps);
            line("native.tasks.migrations", &helpers.migrations);
            line("native.tasks.contexts", &helpers.contexts);
            line("native.units.compiled", &helpers.units_compiled);
            line("native.units.shared", &helpers.units_shared);
            line("native.units.evaluated", &helpers.units_evaluated);
            line("native.tasks.lent", &helpers.contexts_lent);
            line("native.tasks.borrowed", &helpers.contexts_borrowed);
            line("native.tasks.woken", &helpers.woken);
            line("native.log.reads", &helpers.log_reads);
            line("native.log.edges-copied", &helpers.log_edges_copied);
            line("native.log.edges-recorded", &helpers.log_edges_recorded);
            line("native.catalog.uses", &helpers.catalog_uses);
            line("native.catalog.entries", &helpers.catalog_entries);
            line(
                "native.catalog.ms",
                &ms(Duration::from_nanos(helpers.catalog_nanos as u64)),
            );
            line("native.producers.setups", &helpers.producers);
            line("native.producers.replayed", &helpers.producer_replayed);
            line(
                "native.producers.catalog-entries",
                &helpers.producer_catalog_entries,
            );
            line(
                "native.producers.setup-ms",
                &ms(Duration::from_nanos(helpers.producer_nanos as u64)),
            );
            if let Some(core) = &helpers.real_core {
                line("native.core.members", &core.members);
                line("native.core.published", &core.published);
                line("native.core.per-task", &(core.members - core.published));
                line("native.core.private-boots", &core.private_boots);
                line(
                    "native.core.boot-ms",
                    &ms(Duration::from_nanos(core.boot_nanos)),
                );
            }
            let bridge = &helpers.bridge;
            line("native.prune.passes", &bridge.prune_passes);
            line(
                "native.prune.imports-visited",
                &bridge.prune_imports_visited,
            );
            line(
                "native.prune.imports-removed",
                &bridge.prune_imports_removed,
            );
            line("native.prune.nodes-walked", &bridge.prune_nodes_walked);
            line(
                "native.prune.references-released",
                &bridge.prune_references_released,
            );
            line(
                "native.prune.entries-removed",
                &bridge.prune_entries_removed,
            );
            line(
                "native.prune.ms",
                &ms(Duration::from_nanos(bridge.prune_nanos as u64)),
            );
            line("native.env.hits", &bridge.env_hits);
            line("native.env.misses", &bridge.env_misses);
            line("native.env.full", &bridge.env_full);
            line("native.env.evictions", &bridge.env_evictions);
            line("native.env.entries-rebuilt", &bridge.env_entries_rebuilt);
            line("native.env.bytes-rebuilt", &bridge.env_bytes_rebuilt);
            line(
                "native.env.ms",
                &ms(Duration::from_nanos(bridge.env_nanos as u64)),
            );
            let lanes = &self.lanes;
            line("native.lanes.entered", &lanes.entered);
            line("native.lanes.waits", &lanes.waits);
            line("native.lanes.inherited", &lanes.inherited);
            line(
                "native.lanes.wait-ms",
                &ms(Duration::from_nanos(lanes.wait_nanos)),
            );
            line(
                "native.lanes.longest-wait-ms",
                &ms(Duration::from_nanos(lanes.longest_wait_nanos)),
            );
            line("native.lanes.rescued", &lanes.rescued);
        }
        line("macro-nodes.peak", &self.peak_macro_nodes);
        line("deferred-units.registered", &self.deferred_units.registered);
        line("deferred-units.analyzed", &self.deferred_units.analyzed);
        line("deferred-units.prepared", &self.deferred_units.prepared);
        line("deferred-units.executed", &self.deferred_units.executed);
        line("deferred-units.macro-free", &self.deferred_units.macro_free);
        line(
            "time.macro-free-units-ms",
            &ms(self.deferred_units.macro_free_time),
        );
        let units = &self.deferred_units;
        if units.pooled > 0 || units.worker_isolates > 0 {
            line("deferred-units.pooled", &units.pooled);
            line("pool.worker-isolates", &units.worker_isolates);
            line(
                "time.worker-isolate-startup-ms",
                &ms(units.worker_isolate_startup),
            );
            line("time.worker-units-ms", &ms(units.worker_unit_time));
            line("pool.worker-initializations", &units.worker_initializations);
            let pool = &units.pool;
            line("pool.workers", &units.pool_workers);
            line("pool.engine-workers", &units.pool_engine_workers);
            line("pool.engine-participants", &units.pool_engine_participants);
            line("pool.engine-less-units", &units.engine_less_units);
            if self.native_helpers.contexts > 0 || units.coordinator_units > 0 {
                line("pool.coordinator-units", &units.coordinator_units);
            }
            line("time.pool-ms", &ms(units.pool_time));
            // The share of the workers' time spent analyzing units.
            let capacity = units.pool_time.as_secs_f64() * units.pool_workers as f64;
            let busy = units.worker_unit_time.as_secs_f64();
            let utilization = if capacity > 0.0 { busy / capacity } else { 0.0 };
            line("pool.utilization", &format!("{utilization:.3}"));
            line("pool.jobs", &pool.started);
            line("pool.jobs.helped", &pool.helped);
            line("pool.help-depth.max", &pool.help_depth_max);
            line("pool.jobs.local", &pool.local);
            line("pool.claims", &pool.claims);
            line("pool.claimed", &pool.claimed);
            line("pool.steals", &pool.steals);
            line("pool.stolen", &pool.stolen);
            line("pool.parks", &pool.parks);
            line("pool.peak-admitted", &pool.peak_admitted);
            line("pool.peak-backlog", &pool.peak_backlog);
            line("time.pool.queue-wait-ms", &ms(pool.queue_wait));
            line("time.pool.queue-wait.max-ms", &ms(pool.queue_wait_max));
            line("pool.lock-contended", &pool.contended);
            line("catalog.lock-contended", &units.catalog_contended);
        }
        // The parse queue's pool, when it had workers (light-cljs-343).
        if let Some(pool) = &self.parse_pool {
            line("parse-pool.jobs", &pool.started);
            line("parse-pool.steals", &pool.steals);
            line("parse-pool.parks", &pool.parks);
            line("parse-pool.peak-admitted", &pool.peak_admitted);
            line("parse-pool.peak-backlog", &pool.peak_backlog);
            line("time.parse-pool.queue-wait-ms", &ms(pool.queue_wait));
            line(
                "time.parse-pool.queue-wait.max-ms",
                &ms(pool.queue_wait_max),
            );
            line("parse-pool.lock-contended", &pool.contended);
        }
        if let Some(capture) = &self.capture {
            line("cache.capture.namespaces", &capture.captured);
            line("cache.capture.skipped", &capture.skipped);
            line("cache.capture.bytes", &capture.bytes);
            line("cache.capture-ms", &ms(capture.time));
        }
        if let Some(install) = &self.install {
            line("cache.install.namespaces", &install.installed.len());
            line("cache.install.refused", &install.refused.len());
            line("cache.install-ms", &ms(install.time));
        }
        if let Some(codec) = &self.codec {
            line("cache.codec.artifacts", &codec.artifacts);
            line("cache.codec.failed", &codec.failed);
            line("cache.codec.bytes", &codec.bytes);
            line("cache.codec.largest", &codec.largest.0);
            line("cache.codec.source-bytes", &codec.source_bytes);
            line("cache.codec.ir-depth", &codec.ir_depth);
            for (section, bytes) in crate::artifact_codec::Section::ALL
                .iter()
                .zip(codec.sections)
            {
                line(&format!("cache.codec.section.{}", section.name()), &bytes);
            }
            line("cache.encode-ms", &ms(codec.encode_time));
            line("cache.decode-ms", &ms(codec.decode_time));
        }
        if let Some(keys) = &self.keys {
            line("cache.keys.namespaces", &keys.namespaces);
            line("cache.keys.source-bytes", &keys.source_bytes);
            line("cache.keys.changed", &keys.changed);
            line("cache.keys-ms", &ms(keys.time));
            line("cache.revalidate-ms", &ms(keys.revalidate_time));
        }
        if let Some(cache) = &self.cache {
            cache.lines(&mut line);
        }
        if let Some(trace) = &self.trace {
            trace.lines(&mut line);
        }
        let memory = &self.memory;
        if let Some(rss) = memory.peak_rss {
            line("memory.peak-rss-bytes", &rss);
        }
        line("memory.parsed-trees.peak-bytes", &memory.parsed_trees.bytes);
        line("memory.parsed-trees.peak-files", &memory.parsed_trees.files);
        line("memory.queued-ir.peak-bytes", &memory.queued_ir.bytes);
        line("memory.queued-ir.peak-units", &memory.queued_ir.units);
        if let Some(live) = memory.live {
            line("memory.live.artifacts", &live.artifacts);
            line("memory.live.engines", &live.engines);
            line("memory.live.engine-handles", &live.engine_handles);
            line("memory.live.bridge-payloads", &live.bridge_payloads);
        }
        out.push_str("[macro-calls-by-namespace]\n");
        let mut order = (0..self.namespaces.len()).collect::<Vec<_>>();
        // Most calls first; ties in source order.
        order.sort_by_key(|&index| std::cmp::Reverse(self.namespaces[index].macro_calls));
        for index in order.into_iter().take(REPORT_NAMESPACES) {
            let namespace = &self.namespaces[index];
            let _ = writeln!(out, "{} {}", namespace.name, namespace.macro_calls);
        }
        if let Some(cache) = &self.cache {
            out.push_str("[cache-misses-and-bypasses]\n");
            cache.rows(&mut out, REPORT_NAMESPACES);
        }
        out
    }
}

/// The start of a timed phase when `on`; without stats no clock is read.
pub(crate) fn start(on: bool) -> Option<Instant> {
    on.then(Instant::now)
}

/// Add the time since `started` to `phase`, when it was timed.
pub(crate) fn add(phase: &mut Duration, started: Option<Instant>) {
    if let Some(started) = started {
        *phase += started.elapsed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_lists_counters_then_the_busiest_namespaces() {
        let stats = CompileStats {
            phases: PhaseTimes {
                analysis: Duration::from_micros(1500),
                ..PhaseTimes::default()
            },
            namespaces: ["a", "b", "c"]
                .iter()
                .zip([1, 5, 1])
                .map(|(name, macro_calls)| NamespaceStats {
                    name: (*name).into(),
                    macro_calls,
                })
                .collect(),
            macro_calls: 7,
            ..CompileStats::default()
        };
        let report = stats.report();
        assert!(report.starts_with("light-cljs-stats 1\n"));
        assert!(report.contains("\ntime.analysis-ms 1.5\n"));
        assert!(report.contains("\nmacro-calls 7\n"));
        assert!(!report.contains("session-load"));
        assert!(report.ends_with("[macro-calls-by-namespace]\nb 5\na 1\nc 1\n"));
        assert!(!report.contains("cache.codec"));
    }

    #[test]
    fn report_includes_native_collection_pause_and_live_set_counters() {
        let stats = CompileStats {
            native_macro_calls: vec![NativeBridgeCall {
                collections: 3,
                gc_pause_nanos: 40,
                gc_pause_max_nanos: 20,
                gc_live_objects: 100,
                ..Default::default()
            }],
            ..Default::default()
        };
        let report = stats.report();
        assert!(report.contains("\nmacro.gc.0.collections 3\nmacro.gc.0.pause-total-ns 40\nmacro.gc.0.pause-max-ns 20\nmacro.gc.0.live-objects 100\n"), "{report}");
        assert!(!report.contains("macro.gc.0.managed-bytes"), "{report}");
    }

    #[test]
    fn report_includes_native_byte_accounting_under_a_memory_limit() {
        let stats = CompileStats {
            native_macro_calls: vec![NativeBridgeCall {
                gc_managed_bytes: 5,
                gc_peak_managed_bytes: 7,
                gc_live_bytes: 3,
                gc_admitted_bytes: 11,
                gc_byte_limit: 64,
                ..Default::default()
            }],
            ..Default::default()
        };
        let report = stats.report();
        assert!(report.contains("\nmacro.gc.0.live-objects 0\nmacro.gc.0.managed-bytes 5\nmacro.gc.0.peak-managed-bytes 7\nmacro.gc.0.live-bytes 3\nmacro.gc.0.admitted-bytes 11\nmacro.gc.0.byte-limit 64\n"), "{report}");
    }

    #[test]
    fn report_lists_memory_counters_last_with_the_resident_set_where_known() {
        let mut stats = CompileStats {
            memory: MemoryCounts {
                peak_rss: Some(5_000_000),
                parsed_trees: ParsedTreePeak {
                    bytes: 4096,
                    files: 3,
                },
                queued_ir: QueuedIrPeak {
                    bytes: 2048,
                    units: 7,
                },
                live: None,
            },
            ..CompileStats::default()
        };
        let lines = "memory.parsed-trees.peak-bytes 4096\n\
                     memory.parsed-trees.peak-files 3\n\
                     memory.queued-ir.peak-bytes 2048\n\
                     memory.queued-ir.peak-units 7\n\
                     [macro-calls-by-namespace]\n";
        let report = stats.report();
        assert!(
            report.ends_with(&format!("\nmemory.peak-rss-bytes 5000000\n{lines}")),
            "{report}"
        );
        stats.memory.peak_rss = None;
        let report = stats.report();
        assert!(report.ends_with(&format!("\ntime.macro-free-units-ms 0.0\n{lines}")));
        stats.memory.live = Some(LiveCounts {
            artifacts: 1,
            engines: 2,
            engine_handles: 3,
            bridge_payloads: 4,
        });
        let report = stats.report();
        assert!(
            report.ends_with(
                "\nmemory.queued-ir.peak-units 7\n\
                 memory.live.artifacts 1\n\
                 memory.live.engines 2\n\
                 memory.live.engine-handles 3\n\
                 memory.live.bridge-payloads 4\n\
                 [macro-calls-by-namespace]\n"
            ),
            "{report}"
        );
    }

    #[test]
    fn report_lists_codec_counters() {
        let stats = CompileStats {
            codec: Some(CodecCounts {
                artifacts: 3,
                failed: 1,
                bytes: 2100,
                largest: (1000, "app.big".into()),
                source_bytes: 700,
                sections: [600, 50, 400, 30, 20, 10],
                ir_depth: 42,
                encode_time: Duration::from_micros(2500),
                decode_time: Duration::from_millis(2),
            }),
            ..CompileStats::default()
        };
        let report = stats.report();
        let expected = "cache.codec.artifacts 3\n\
                        cache.codec.failed 1\n\
                        cache.codec.bytes 2100\n\
                        cache.codec.largest 1000\n\
                        cache.codec.source-bytes 700\n\
                        cache.codec.ir-depth 42\n\
                        cache.codec.section.strings 600\n\
                        cache.codec.section.declarations 50\n\
                        cache.codec.section.ir 400\n\
                        cache.codec.section.units 30\n\
                        cache.codec.section.diagnostics 20\n\
                        cache.codec.section.names 10\n\
                        cache.encode-ms 2.5\n\
                        cache.decode-ms 2.0\n";
        assert!(report.contains(expected), "{report}");
    }

    #[test]
    fn report_lists_only_the_twenty_namespaces_with_the_most_calls() {
        // ns0..ns24 make 0..24 calls, except that ns3 and ns4 tie with ns5
        // at the cutoff's edge: the table keeps the earlier of those.
        let calls = |index: usize| if (3..=5).contains(&index) { 5 } else { index };
        let stats = CompileStats {
            namespaces: (0..25)
                .map(|index| NamespaceStats {
                    name: format!("ns{index}"),
                    macro_calls: calls(index),
                })
                .collect(),
            macro_calls: (0..25).map(calls).sum(),
            ..CompileStats::default()
        };
        let report = stats.report();
        let (counters, table) = report.split_once("[macro-calls-by-namespace]\n").unwrap();
        assert!(counters.contains("\nnamespaces 25\n"));
        assert!(counters.contains(&format!("\nmacro-calls {}\n", stats.macro_calls)));
        let rows = table.lines().collect::<Vec<_>>();
        let mut expected = (6..25)
            .rev()
            .map(|index| format!("ns{index} {index}"))
            .collect::<Vec<_>>();
        expected.push("ns3 5".into());
        assert_eq!(rows, expected);
    }
}
