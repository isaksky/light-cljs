//! The persistent compiler cache of a `--compile --entry` build
//! (`--cache-dir DIR`, docs/compiler-cache.md, "Loading, diagnostics, and
//! storage"): it joins the store ([`crate::artifact_store`]), the keys
//! ([`crate::cache_key`]), the policy ([`crate::cache_policy`]), the codec
//! and the in-memory install into the compilation's per-namespace loop
//! ([`crate::compiler::EntryRecorders::cache`]).
//!
//! For each namespace, in source order:
//!
//! 1. The loader has resolved its source and requires. A `--cache-blocker`
//!    namespace is not looked up.
//! 2. Otherwise each candidate of its index in the [`Snapshot`], newest
//!    first, is read and its manifest checked: the key it would have now,
//!    computed from the current sources and the keys of the namespaces
//!    before it with the edges, inputs and absent names the manifest
//!    stored, must be the key it was published under; its resources must
//!    read as they did; and no namespace it depends on may be bypassed.
//! 3. The matching candidate is decoded into an owned artifact, which the
//!    compilation installs at the namespace's frontier and then drops.
//! 4. Otherwise the namespace is analyzed and captured, its key computed
//!    from what its analysis observed ([`ManifestPrefix`]), and, unless the
//!    policy bypasses it, its artifact is published and dropped.
//!
//! Lookup reads files and hashes; it runs no macro and no application
//! code, and nothing it refuses has changed the compilation. Every failure
//! to use an artifact (missing, damaged, undecodable, another key, an
//! install refusal) is a miss with its reason, and every write failure
//! leaves the build's output alone.

use crate::artifact_codec::{self, DecodeLimits, ManifestFields};
use crate::artifact_store::{
    CommitCpu, EncodedArtifact, Inputs, MAX_OUTPUT_BYTES, Publish, Retention, Snapshot,
    StageAttempt, StagedArtifact, Store,
};
use crate::cache_key::{
    Digest, FrontEndKey, KeyEnvironment, Mismatch, NamespaceKeys, RevalidateInputs,
};
use crate::cache_policy::{Bypass, NamespaceMetadata};
use crate::compilation_session::PoolTaskSubmitter;
use crate::compile_stats::{CacheCounts, OutputCounts};
use crate::dependency_manifest::{BroadView, EdgeKinds, ExternalInput, LoaderRecord, NamespaceId};
use crate::macro_host::MacroHost;
use crate::manifest_prefix::{ManifestPrefix, NamespaceRecords};
use crate::namespace_artifact::NamespaceArtifact;
use crate::scheduler::Slots;
use crate::source_provider::SourceProvider;
use std::collections::{BTreeMap, VecDeque};
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Instant;

/// Where `--compile --entry`, `--reloadable` and `--watch` keep the cache
/// without `--cache-dir` or `--no-cache`: `.light-cljs/cache` under the
/// current directory (light-cljs-328).
pub const DEFAULT_DIR: &str = ".light-cljs/cache";

/// What a build asks of the persistent cache.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CacheOptions {
    /// `--cache-dir DIR`.
    pub dir: PathBuf,
    /// `--cache-blocker NS`: these namespaces and every compilation consumer
    /// of them neither read nor write the cache.
    pub blockers: Vec<String>,
    /// What trimming keeps after the build: `--cache-retention N` sets
    /// [`Retention::per_namespace`], three by default.
    pub retention: Retention,
}

/// The miss kind of a namespace the Pants build analyzed without a lookup
/// because it may write a core function ([`CompileCache::core_write`]).
pub(crate) const CORE_WRITE: &str = "core-write";

/// Why the current namespace was not installed, until it completes.
#[derive(Clone, Debug)]
enum Pending {
    /// The candidate whose key matched, being installed.
    Hit {
        key: Box<FrontEndKey>,
        artifact: Digest,
    },
    /// The first reason a candidate could not be used: the stats kind and
    /// its description.
    Miss(&'static str, String),
}

/// Kind of cache-publishing CPU work observed by test hooks.
#[cfg(any(debug_assertions, feature = "scheduling-stress"))]
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CacheCpuKind {
    ArtifactEncodeHash,
    IndexEncodeHash,
}

/// Upper bound on consecutive candidate-bearing namespaces prefetched in one
/// sliding-window cache lookup batch.
const LOOKUP_WINDOW_MAX_NS: usize = 64;
/// Upper bound on total encoded artifact bytes prefetched in one sliding-window
/// cache lookup batch (3 MiB), bounding in-flight decoded memory to keep peak
/// memory below the single-threaded/baseline budget.
const LOOKUP_WINDOW_MAX_BYTES: u64 = 3 * 1024 * 1024;

/// A candidate matched and verified/decoded ahead of time during a sliding-window
/// parallel cache lookup, waiting for serial deterministic install in Stage D.
struct PrefetchedHit {
    id: NamespaceId,
    fields: ManifestFields,
    key: Box<FrontEndKey>,
    artifact_digest: Digest,
    prefix_bytes_read: u64,
    body: PrefetchedBody,
    candidate_index: usize,
    prior_miss: Option<(&'static str, String)>,
}

enum PrefetchedBody {
    Decoded {
        artifact: NamespaceArtifact,
        body_bytes_read: u64,
    },
    ReadError {
        error: crate::artifact_store::ReadError,
        body_bytes_read: u64,
    },
    DecodeError {
        error: artifact_codec::DecodeError,
        body_bytes_read: u64,
    },
}

/// One compilation's use of a cache directory.
pub struct CompileCache {
    store: Store,
    snapshot: Snapshot,
    blockers: Vec<String>,
    environment: KeyEnvironment,
    entry: String,
    prefix: ManifestPrefix,
    keys: NamespaceKeys,
    /// Per namespace, the propagated bypass that reaches it and the
    /// namespace it came through (itself at the origin).
    propagated: Vec<Option<(Bypass, u32)>>,
    pending: Option<Pending>,
    /// The artifacts this build installed or published, which trimming
    /// keeps.
    in_use: Vec<Digest>,
    /// Per namespace, the artifact it was installed from or published as;
    /// `None` for one bypassed or not published.
    artifacts: Vec<Option<Digest>>,
    limits: DecodeLimits,
    counts: CacheCounts,
    retention: Retention,
    /// How many namespaces so far a later coarse view covers by their
    /// declaration key or whole key ([`CacheCounts::view_declaring`]).
    declaring: u64,
    /// True if any lookup observed a damaged index or unreadable/undecodable
    /// object, forcing [`Self::finish`] to run [`Store::trim`].
    saw_damage: bool,
    /// True once any earlier namespace has a propagated bypass (`Some(_)` in
    /// `self.propagated`), letting `reaching` skip scanning coarse edges when
    /// no bypass exists.
    any_propagated: bool,
    /// Incrementally maintained `namespace -> source index` table over
    /// `loader.sources` so sliding-window lookup avoids rebuilding it per batch.
    ns_positions: std::collections::HashMap<String, usize>,
    /// Prefetched decoded hits from the current sliding window, consumed in
    /// strict source order by [`Self::lookup_batched`].
    prefetched: VecDeque<PrefetchedHit>,
    /// CPU slots available to this compilation, detected from the session's
    /// unit pool policy when `--jobs > 1`.
    slots: Slots,
    /// `Send + Sync` submitter into the active compilation session's unit pool,
    /// keeping cache staging and index encoding/hashing within `--jobs` while
    /// preserving `CompileCache: Send`.
    pool: Option<PoolTaskSubmitter>,
    /// Staged artifacts awaiting a batched commit on the inline (`Slots::ONE`)
    /// path, flushed every `PUBLISH_BATCH_SIZE` namespaces and on flush/finish.
    inline_batch: Vec<(NamespaceId, String, StagedArtifact)>,
    /// Background publish pipeline when `slots > 1`, started lazily on the
    /// first publish so warm no-op builds spawn no threads.
    pipeline: Option<PublishPipeline>,
    /// True if spawning the background commit thread failed, so later publishes
    /// stay inline instead of retrying thread creation.
    pipeline_failed: bool,
    #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
    cpu_work_hook: Option<Arc<dyn Fn(CacheCpuKind) + Send + Sync>>,
}

impl CompileCache {
    /// The cache of a compilation by `host` with `options` of the entry
    /// namespace `entry`, reading every index of `options.dir` once.
    pub fn open(
        options: &CacheOptions,
        host: &MacroHost,
        analysis: crate::analyzer::AnalysisOptions,
        entry: &str,
    ) -> Self {
        Self::open_with_slots(options, host, analysis, entry, Slots::ONE)
    }

    /// [`Self::open`], using `slots` CPU slots for parallel index snapshotting,
    /// sliding-window lookup/decode, and background publishing.
    pub fn open_with_slots(
        options: &CacheOptions,
        host: &MacroHost,
        analysis: crate::analyzer::AnalysisOptions,
        entry: &str,
        slots: Slots,
    ) -> Self {
        let started = Instant::now();
        let store = Store::open(&options.dir);
        let snapshot = store.snapshot_all_with_slots(slots);
        let mut counts = CacheCounts::default();
        counts.validate_time += started.elapsed();
        Self {
            store,
            snapshot,
            blockers: options.blockers.clone(),
            environment: KeyEnvironment::new(host, analysis),
            entry: entry.to_owned(),
            prefix: ManifestPrefix::default(),
            keys: NamespaceKeys::default(),
            propagated: Vec::new(),
            pending: None,
            in_use: Vec::new(),
            artifacts: Vec::new(),
            limits: DecodeLimits::default(),
            counts,
            retention: options.retention,
            declaring: 0,
            saw_damage: false,
            any_propagated: false,
            ns_positions: std::collections::HashMap::new(),
            prefetched: VecDeque::new(),
            slots,
            pool: None,
            inline_batch: Vec::new(),
            pipeline: None,
            pipeline_failed: false,
            #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
            cpu_work_hook: None,
        }
    }

    #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
    #[doc(hidden)]
    pub fn set_cpu_work_hook(&mut self, hook: Option<Arc<dyn Fn(CacheCpuKind) + Send + Sync>>) {
        self.cpu_work_hook = hook;
    }

    #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
    #[doc(hidden)]
    pub fn set_write_temporary_hook(
        &self,
        hook: Option<crate::artifact_store::WriteTemporaryHook>,
    ) {
        self.store.set_write_temporary_hook(hook);
    }

    /// Configure the CPU slot budget for sliding-window lookup/decode and
    /// background publishing when opened via [`Self::open`].
    pub(crate) fn set_slots(&mut self, slots: Slots) {
        if slots.get() > self.slots.get() {
            self.slots = slots;
        }
    }

    /// The generated names of every candidate this compilation may install.
    pub fn reservation(&self) -> &crate::generated_names::Reservation {
        self.snapshot.reservation()
    }

    /// Whether this compilation's snapshot holds any candidate artifact for
    /// `namespace` (excluding cache blockers).
    pub(crate) fn has_candidates(&self, namespace: &str) -> bool {
        !self.blockers.iter().any(|blocker| blocker == namespace)
            && self
                .snapshot
                .candidates(namespace)
                .is_ok_and(|candidates| !candidates.is_empty())
    }

    /// What the cache did so far.
    pub fn counts(&self) -> &CacheCounts {
        &self.counts
    }

    pub fn take_counts(&mut self) -> CacheCounts {
        self.flush_publishes();
        std::mem::take(&mut self.counts)
    }

    /// Wait for any in-flight background publish work and commit any staged
    /// artifacts, applying their outcomes in deterministic submission order.
    pub fn flush_publishes(&mut self) {
        if let Some(pipeline) = &mut self.pipeline {
            let started = Instant::now();
            let outcomes = pipeline.flush();
            self.apply_finished(outcomes);
            self.counts.write_time += started.elapsed();
        }
        if !self.inline_batch.is_empty() {
            let started = Instant::now();
            self.commit_inline_batch();
            self.counts.write_time += started.elapsed();
        }
    }

    /// The dependency manifest of the compilation, once it has ended, from
    /// the records the cache keyed its namespaces with
    /// ([`ManifestPrefix::to_manifest`]).
    pub fn manifest(
        &self,
        lazy: &[(u32, String)],
    ) -> crate::dependency_manifest::DependencyManifest {
        self.prefix.to_manifest(lazy)
    }

    /// Bound the cache directory after the build succeeded, keeping what it
    /// used and published.
    pub fn finish(&mut self) {
        self.flush_publishes();
        if self.store.write_failure().is_some() {
            return;
        }
        if self.counts.published == 0
            && !self.saw_damage
            && self.snapshot.within_retention(&self.retention)
        {
            return;
        }
        if let Err(error) = self.store.trim(&self.retention, &self.in_use) {
            eprintln!(
                "warning: cannot trim the compiler cache in {}: {error}",
                self.store.root().display()
            );
        }
    }

    /// What reusing the output of this compilation needs, once it has
    /// ended: the store, and the digest of the assembled program when every
    /// namespace was installed from or published as an artifact.
    pub fn into_output(mut self) -> OutputCache {
        self.flush_publishes();
        self.pipeline = None;
        let complete =
            self.artifacts.len() == self.prefix.len() && self.keys.len() == self.prefix.len();
        let program = complete
            .then(|| {
                let mut fingerprint = crate::sha256::Fingerprint::new("light-cljs program");
                fingerprint.text(&self.entry).count(self.artifacts.len());
                for (at, artifact) in self.artifacts.iter().enumerate() {
                    let id = NamespaceId(at as u32);
                    fingerprint
                        .text(&self.prefix.record(id).name)
                        .digest(&self.keys.key(id).digest)
                        .digest(artifact.as_ref()?);
                }
                Some(fingerprint.finish())
            })
            .flatten();
        OutputCache {
            store: self.store.clone(),
            environment: self.environment,
            program,
            retention: self.retention,
            counts: OutputCounts::default(),
        }
    }

    fn miss(&mut self, kind: &'static str, description: String) {
        if matches!(kind, "unreadable" | "index" | "undecodable") {
            self.saw_damage = true;
        }
        if !matches!(self.pending, Some(Pending::Miss(..))) {
            self.pending = Some(Pending::Miss(kind, description));
        }
    }

    /// The rest of a candidate
    /// [`crate::artifact_store::Store::begin_read`] began, counted in the
    /// bytes read when it was read to the end.
    fn finish_read(
        &mut self,
        reading: crate::artifact_store::Reading,
    ) -> Result<Vec<u8>, crate::artifact_store::ReadError> {
        let rest = reading.length() - reading.prefix().len() as u64;
        let finished = reading.finish();
        if matches!(
            finished,
            Ok(_) | Err(crate::artifact_store::ReadError::Integrity)
        ) {
            self.counts.bytes_read += rest;
        }
        finished
    }

    /// The artifact to install for the next namespace, the last source of
    /// `loader`, whose text is `text`, if a candidate matches.
    /// Decoding polls `cancelled`, and a cancelled decode is the error.
    pub(crate) fn lookup(
        &mut self,
        loader: &LoaderRecord,
        text: &str,
        sources: &dyn SourceProvider,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Option<NamespaceArtifact>, String> {
        self.pending = None;
        let id = NamespaceId(self.prefix.len() as u32);
        let name = loader.sources[id.0 as usize].name.clone();
        if self.blockers.contains(&name) {
            return Ok(None);
        }
        let started = Instant::now();
        let candidates = match self.snapshot.candidates(&name) {
            Ok(candidates) => candidates.to_vec(),
            // The snapshot leaves out an index it could not read; read it
            // again only to name the reason, never for its entries.
            Err(crate::artifact_store::IndexError::Missing) => {
                match self.store.read_candidates(&name) {
                    Err(error @ crate::artifact_store::IndexError::Damaged)
                    | Err(error @ crate::artifact_store::IndexError::Io(_)) => {
                        self.miss("index", error.to_string())
                    }
                    _ => self.miss("no-artifact", "no artifact was published".into()),
                }
                Vec::new()
            }
            Err(error) => {
                self.miss("index", error.to_string());
                Vec::new()
            }
        };
        let mut found = None;
        for candidate in &candidates {
            // Only the manifest is read to compare the key; the rest is read
            // and the whole checked against its content hash only for the
            // candidate that matches, so a stale candidate costs its manifest.
            let reading = match self.store.begin_read(candidate, &self.limits) {
                Ok(reading) => reading,
                Err(error) => {
                    self.miss("unreadable", error.to_string());
                    continue;
                }
            };
            self.counts.bytes_read += reading.prefix().len() as u64;
            let manifest = match artifact_codec::read_manifest_prefix(
                reading.prefix(),
                reading.length(),
                &self.limits,
            ) {
                Ok(manifest) if manifest.namespace == name => manifest,
                Ok(manifest) => {
                    self.miss(
                        "undecodable",
                        format!("the artifact is {}'s", manifest.namespace),
                    );
                    continue;
                }
                // A manifest that does not read is rare (a damaged file, an
                // older schema): read it whole to name damage as the content
                // hash's failure, as a read of the whole file would.
                Err(error) => {
                    match self.finish_read(reading) {
                        Ok(_) => self.miss("undecodable", error.to_string()),
                        Err(read) => self.miss("unreadable", read.to_string()),
                    }
                    continue;
                }
            };
            let fields = &manifest.fields;
            self.prefix.push_stored(loader, fields, false);
            let key = self
                .keys
                .next(&self.environment, &self.prefix, id, text, &self.entry);
            let matched =
                if key.digest != candidate.key || fields.front_end_key.digest != key.digest {
                    match fields.front_end_key.mismatch(&key) {
                        // The key takes the inputs as stored, so only a
                        // re-read finds that they changed, and that comes
                        // before the view in the components' order: report
                        // the namespace's own input, not that earlier
                        // namespaces changed (a coarse namespace's view
                        // covers every earlier key).
                        Some(Mismatch::View) => {
                            crate::cache_key::inputs_unchanged(&fields.inputs, sources)
                                .and(Err(Mismatch::View))
                        }
                        mismatch => Err(mismatch.unwrap_or(Mismatch::Inconsistent)),
                    }
                } else {
                    crate::cache_key::inputs_unchanged(&fields.inputs, sources)
                };
            if let Err(mismatch) = matched {
                self.prefix.pop();
                self.miss(kind(&mismatch), mismatch.to_string());
                continue;
            }
            // The key matched, so these edges are the namespace's own.
            if fields.no_cache
                || fields.always
                || self.reaching(id).is_some()
                || self.origin(id, false, false).is_some()
            {
                self.prefix.pop();
                self.pending = None;
                break;
            }
            let bytes = match self.finish_read(reading) {
                Ok(bytes) => bytes,
                Err(error) => {
                    self.prefix.pop();
                    self.miss("unreadable", error.to_string());
                    continue;
                }
            };
            found = Some((key, candidate.artifact, bytes));
            break;
        }
        self.counts.validate_time += started.elapsed();
        let Some((key, artifact, bytes)) = found else {
            return Ok(None);
        };
        let started = Instant::now();
        let decoded = artifact_codec::decode_verified(&bytes, &self.limits, cancelled);
        drop(bytes);
        self.counts.decode_time += started.elapsed();
        match decoded {
            Ok((_, decoded)) => {
                self.pending = Some(Pending::Hit {
                    key: Box::new(key),
                    artifact,
                });
                Ok(Some(decoded))
            }
            Err(error) => {
                self.prefix.pop();
                self.pending = None;
                if error == artifact_codec::DecodeError::Cancelled {
                    return Err(error.to_string());
                }
                self.miss("undecodable", error.to_string());
                Ok(None)
            }
        }
    }

    /// Sliding-window parallel cache lookup for namespace `index`, prefetching
    /// up to `LOOKUP_WINDOW_MAX_NS` consecutive candidate-bearing namespaces
    /// (bounded by `LOOKUP_WINDOW_MAX_BYTES`) across `self.slots`:
    /// - Stage A (parallel): `begin_read` + `read_manifest_prefix` of each
    ///   namespace's newest candidate, plus `source_digest`, `resolution_digest`,
    ///   and concurrent `inputs_unchanged`.
    /// - Stage B (serial in-memory): chain-walk `ManifestPrefix` and
    ///   `NamespaceKeys` in topological order to confirm the contiguous hit
    ///   prefix, stopping at the first miss or bypass so stale candidates only
    ///   read their manifest prefix.
    /// - Stage C (parallel): `Reading::finish` (content-hash verify) and
    ///   `artifact_codec::decode_verified` (with fused `validate_with_depth`),
    ///   dropping raw bytes immediately on workers.
    /// - Stage D (delivered here one by one): return each decoded artifact in
    ///   order, falling back cleanly on corruption or cancellation with exact
    ///   `bytes_read` and miss reporting.
    pub(crate) fn lookup_batched<'a>(
        &mut self,
        loader: &LoaderRecord,
        index: usize,
        text_of: &(dyn Fn(usize) -> &'a str + Sync),
        sources: &dyn SourceProvider,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Option<NamespaceArtifact>, String> {
        self.pending = None;
        let id = NamespaceId(self.prefix.len() as u32);
        debug_assert_eq!(id.0 as usize, index);
        if self.slots == Slots::ONE {
            return self.lookup(loader, text_of(index), sources, cancelled);
        }
        if self.prefetched.front().is_some_and(|item| item.id != id) {
            self.prefetched.clear();
        }
        if self.prefetched.is_empty() {
            let mut window_end = index;
            let mut window_bytes = 0u64;
            while window_end < loader.sources.len()
                && window_end - index < LOOKUP_WINDOW_MAX_NS
                && self.has_candidates(&loader.sources[window_end].name)
            {
                let first_bytes = self
                    .snapshot
                    .candidates(&loader.sources[window_end].name)
                    .ok()
                    .and_then(|entries| entries.first())
                    .map_or(0, |entry| entry.bytes);
                if window_end > index
                    && window_bytes.saturating_add(first_bytes) > LOOKUP_WINDOW_MAX_BYTES
                {
                    break;
                }
                window_bytes = window_bytes.saturating_add(first_bytes);
                window_end += 1;
            }
            if window_end <= index + 1 {
                return self.lookup(loader, text_of(index), sources, cancelled);
            }
            if !self.fill_prefetch_window(loader, index, window_end, text_of, sources, cancelled) {
                return Ok(None);
            }
        }
        let item = self
            .prefetched
            .pop_front()
            .expect("prefetch window populated or returned early");
        debug_assert_eq!(item.id, id);
        self.counts.bytes_read += item.prefix_bytes_read;
        match item.body {
            PrefetchedBody::Decoded {
                artifact,
                body_bytes_read,
            } => {
                self.counts.bytes_read += body_bytes_read;
                self.prefix.push_stored(loader, &item.fields, false);
                self.pending = Some(Pending::Hit {
                    key: item.key,
                    artifact: item.artifact_digest,
                });
                Ok(Some(artifact))
            }
            PrefetchedBody::ReadError {
                error,
                body_bytes_read,
            } => {
                self.counts.bytes_read += body_bytes_read;
                self.prefetched.clear();
                if let Some((kind, description)) = item.prior_miss {
                    self.miss(kind, description);
                }
                self.miss("unreadable", error.to_string());
                self.lookup_remaining_candidates(
                    loader,
                    id,
                    text_of(index),
                    sources,
                    cancelled,
                    item.candidate_index + 1,
                )
            }
            PrefetchedBody::DecodeError {
                error,
                body_bytes_read,
            } => {
                self.counts.bytes_read += body_bytes_read;
                self.prefetched.clear();
                if error == artifact_codec::DecodeError::Cancelled {
                    return Err(error.to_string());
                }
                if let Some((kind, description)) = item.prior_miss {
                    self.miss(kind, description);
                }
                self.miss("undecodable", error.to_string());
                Ok(None)
            }
        }
    }

    /// Run Stages A, B, and C for `index..window_end`. Returns `true` when at
    /// least namespace `index` entered `self.prefetched`; returns `false` when
    /// namespace `index` itself missed or bypassed (having already recorded its
    /// `bytes_read` and miss reason).
    fn fill_prefetch_window<'a>(
        &mut self,
        loader: &LoaderRecord,
        index: usize,
        window_end: usize,
        text_of: &(dyn Fn(usize) -> &'a str + Sync),
        sources: &dyn SourceProvider,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> bool {
        enum FirstCandidateRead {
            BeginErr(crate::artifact_store::ReadError),
            ManifestErr {
                reading: crate::artifact_store::Reading,
                error: artifact_codec::DecodeError,
            },
            ManifestOk {
                reading: crate::artifact_store::Reading,
                manifest: artifact_codec::Manifest,
                inputs_check: Option<Result<(), Mismatch>>,
            },
        }
        struct StageAOutcome {
            source_digest: Digest,
            resolution_digest: Digest,
            first_read: FirstCandidateRead,
        }
        struct StageCJob {
            id: NamespaceId,
            reading: crate::artifact_store::Reading,
            fields: ManifestFields,
            key: Box<FrontEndKey>,
            artifact_digest: Digest,
            prefix_bytes_read: u64,
            candidate_index: usize,
            prior_miss: Option<(&'static str, String)>,
        }

        let validate_started = Instant::now();
        if self.ns_positions.len() < loader.sources.len() {
            let start = self.ns_positions.len();
            self.ns_positions.reserve(loader.sources.len() - start);
            for (pos, src) in loader.sources.iter().enumerate().skip(start) {
                self.ns_positions.insert(src.name.clone(), pos);
            }
        }
        let ns_positions = &self.ns_positions;
        let concurrent_sources = sources.concurrent();
        let store = &self.store;
        let snapshot = &self.snapshot;
        let limits = &self.limits;
        let entry = &self.entry;
        let stage_a_jobs: Vec<(u64, usize)> = (index..window_end)
            .map(|ns_idx| {
                let first_bytes = snapshot
                    .candidates(&loader.sources[ns_idx].name)
                    .ok()
                    .and_then(|c| c.first())
                    .map_or(0, |e| e.bytes);
                let cost = text_of(ns_idx).len() as u64 + first_bytes.min(65536);
                (cost, ns_idx)
            })
            .collect();

        // Stage A: parallel manifest prefix reads + source/resolution digests.
        let stage_a = crate::scheduler::map(self.slots, stage_a_jobs, |ns_idx| {
            let src = &loader.sources[ns_idx];
            let text = text_of(ns_idx);
            let source_digest = crate::cache_key::source_digest_from_loader(src, text, entry);
            let resolution_digest =
                crate::cache_key::resolution_digest_from_loader(loader, ns_idx, ns_positions);
            let first = &snapshot
                .candidates(&src.name)
                .expect("window entries have candidates")[0];
            let first_read = match store.begin_read(first, limits) {
                Err(error) => FirstCandidateRead::BeginErr(error),
                Ok(reading) => match artifact_codec::read_manifest_prefix(
                    reading.prefix(),
                    reading.length(),
                    limits,
                ) {
                    Err(error) => FirstCandidateRead::ManifestErr { reading, error },
                    Ok(manifest) => {
                        let inputs_check = if manifest.namespace == src.name {
                            let has_resources = manifest
                                .fields
                                .inputs
                                .iter()
                                .any(|input| matches!(input, ExternalInput::Resource(_)));
                            if !has_resources {
                                Some(Ok(()))
                            } else {
                                concurrent_sources.map(|cs| {
                                    crate::cache_key::inputs_unchanged(&manifest.fields.inputs, cs)
                                })
                            }
                        } else {
                            None
                        };
                        FirstCandidateRead::ManifestOk {
                            reading,
                            manifest,
                            inputs_check,
                        }
                    }
                },
            };
            StageAOutcome {
                source_digest,
                resolution_digest,
                first_read,
            }
        });

        // Stage B: fast in-memory serial chain walk in topological order without
        // mutating `ManifestPrefix` (avoiding duplicate coarse-edge expansion).
        let mut pushed_keys = 0usize;
        let mut stage_c_jobs: Vec<StageCJob> = Vec::with_capacity(stage_a.len());
        let mut first_ns_miss: Option<(&'static str, String)> = None;
        let mut first_ns_bypass = false;
        let mut first_ns_prefix_bytes = 0u64;

        'window: for (offset, outcome) in stage_a.into_iter().enumerate() {
            let ns_idx = index + offset;
            let ns_id = NamespaceId(ns_idx as u32);
            let src = &loader.sources[ns_idx];
            let name = src.name.as_str();
            let candidates = self
                .snapshot
                .candidates(name)
                .expect("window entries have candidates");
            let mut prefix_bytes_read = 0u64;
            let mut prior_miss: Option<(&'static str, String)> = None;
            let mut note_miss = |kind: &'static str, description: String| {
                if prior_miss.is_none() {
                    prior_miss = Some((kind, description));
                }
            };
            let mut first_read = Some(outcome.first_read);
            let mut matched_hit = false;

            for (cand_idx, candidate) in candidates.iter().enumerate() {
                let (reading, manifest, inputs_check) = if cand_idx == 0 {
                    match first_read.take().expect("cand 0 read once") {
                        FirstCandidateRead::BeginErr(error) => {
                            note_miss("unreadable", error.to_string());
                            continue;
                        }
                        FirstCandidateRead::ManifestErr { reading, error } => {
                            prefix_bytes_read += reading.prefix().len() as u64;
                            let rest = reading.length() - reading.prefix().len() as u64;
                            let finished = reading.finish();
                            if matches!(
                                finished,
                                Ok(_) | Err(crate::artifact_store::ReadError::Integrity)
                            ) {
                                prefix_bytes_read += rest;
                            }
                            match finished {
                                Ok(_) => note_miss("undecodable", error.to_string()),
                                Err(read) => note_miss("unreadable", read.to_string()),
                            }
                            continue;
                        }
                        FirstCandidateRead::ManifestOk {
                            reading,
                            manifest,
                            inputs_check,
                        } => {
                            prefix_bytes_read += reading.prefix().len() as u64;
                            (reading, manifest, inputs_check)
                        }
                    }
                } else {
                    let reading = match self.store.begin_read(candidate, &self.limits) {
                        Ok(reading) => reading,
                        Err(error) => {
                            note_miss("unreadable", error.to_string());
                            continue;
                        }
                    };
                    prefix_bytes_read += reading.prefix().len() as u64;
                    match artifact_codec::read_manifest_prefix(
                        reading.prefix(),
                        reading.length(),
                        &self.limits,
                    ) {
                        Ok(manifest) => (reading, manifest, None),
                        Err(error) => {
                            let rest = reading.length() - reading.prefix().len() as u64;
                            let finished = reading.finish();
                            if matches!(
                                finished,
                                Ok(_) | Err(crate::artifact_store::ReadError::Integrity)
                            ) {
                                prefix_bytes_read += rest;
                            }
                            match finished {
                                Ok(_) => note_miss("undecodable", error.to_string()),
                                Err(read) => note_miss("unreadable", read.to_string()),
                            }
                            continue;
                        }
                    }
                };
                if manifest.namespace != name {
                    note_miss(
                        "undecodable",
                        format!("the artifact is {}'s", manifest.namespace),
                    );
                    continue;
                }
                let fields = &manifest.fields;
                let key = self.keys.next_stored(
                    &self.environment,
                    loader,
                    fields,
                    &self.ns_positions,
                    ns_id,
                    outcome.source_digest,
                    outcome.resolution_digest,
                );
                let check_inputs = || match &inputs_check {
                    Some(res) => res.clone(),
                    None => crate::cache_key::inputs_unchanged(&fields.inputs, sources),
                };
                let matched =
                    if key.digest != candidate.key || fields.front_end_key.digest != key.digest {
                        match fields.front_end_key.mismatch(&key) {
                            Some(Mismatch::View) => check_inputs().and(Err(Mismatch::View)),
                            mismatch => Err(mismatch.unwrap_or(Mismatch::Inconsistent)),
                        }
                    } else {
                        check_inputs()
                    };
                if let Err(mismatch) = matched {
                    note_miss(kind(&mismatch), mismatch.to_string());
                    continue;
                }
                let reaches_bypass = self.any_propagated
                    && (fields.coarse
                        || fields.dependencies.iter().any(|dep| {
                            EdgeKinds(dep.kinds).intersects(EdgeKinds::COMPILATION)
                                && self.ns_positions.get(dep.namespace.as_str()).is_some_and(
                                    |&pos| {
                                        pos < self.propagated.len()
                                            && self.propagated[pos].is_some()
                                    },
                                )
                        }));
                if fields.no_cache
                    || fields.always
                    || reaches_bypass
                    || self.blockers.iter().any(|blocker| blocker == name)
                {
                    if offset == 0 {
                        first_ns_bypass = true;
                        first_ns_prefix_bytes = prefix_bytes_read;
                    }
                    break 'window;
                }
                self.keys.append_stored(src, fields, key.clone());
                pushed_keys += 1;
                stage_c_jobs.push(StageCJob {
                    id: ns_id,
                    reading,
                    fields: manifest.fields,
                    key: Box::new(key),
                    artifact_digest: candidate.artifact,
                    prefix_bytes_read,
                    candidate_index: cand_idx,
                    prior_miss: prior_miss.take(),
                });
                matched_hit = true;
                break;
            }
            if !matched_hit {
                if offset == 0 {
                    first_ns_miss = prior_miss;
                    first_ns_prefix_bytes = prefix_bytes_read;
                }
                break;
            }
        }

        // Unwind speculative key chain before Stage C/D so any mid-window
        // corruption or install refusal leaves `self.keys` untouched.
        for _ in 0..pushed_keys {
            self.keys.pop();
        }
        self.counts.validate_time += validate_started.elapsed();

        if stage_c_jobs.is_empty() {
            self.counts.bytes_read += first_ns_prefix_bytes;
            if !first_ns_bypass && let Some((kind, description)) = first_ns_miss {
                self.miss(kind, description);
            }
            return false;
        }

        // Stage C: parallel body read + full-file SHA-256 verify + decode_verified.
        let stage_c_started = Instant::now();
        let limits = &self.limits;
        let jobs: Vec<(u64, StageCJob)> = stage_c_jobs
            .into_iter()
            .map(|job| (job.reading.length(), job))
            .collect();
        let decoded = crate::scheduler::map(self.slots, jobs, |job| {
            let StageCJob {
                id,
                reading,
                fields,
                key,
                artifact_digest,
                prefix_bytes_read,
                candidate_index,
                prior_miss,
            } = job;
            if cancelled() {
                drop(reading);
                return (
                    PrefetchedHit {
                        id,
                        fields,
                        key,
                        artifact_digest,
                        prefix_bytes_read,
                        body: PrefetchedBody::DecodeError {
                            error: artifact_codec::DecodeError::Cancelled,
                            body_bytes_read: 0,
                        },
                        candidate_index,
                        prior_miss,
                    },
                    0u64,
                    0u64,
                );
            }
            let rest = reading.length() - reading.prefix().len() as u64;
            let t0 = Instant::now();
            let read_res = reading.finish();
            let read_ns = t0.elapsed().as_nanos() as u64;
            let (body, decode_ns) = match read_res {
                Err(error) => {
                    let body_bytes_read =
                        if matches!(error, crate::artifact_store::ReadError::Integrity) {
                            rest
                        } else {
                            0
                        };
                    (
                        PrefetchedBody::ReadError {
                            error,
                            body_bytes_read,
                        },
                        0u64,
                    )
                }
                Ok(bytes) => {
                    let t1 = Instant::now();
                    let decoded = artifact_codec::decode_verified(&bytes, limits, cancelled);
                    // Drop raw artifact bytes immediately on the worker thread.
                    drop(bytes);
                    let decode_ns = t1.elapsed().as_nanos() as u64;
                    let body = match decoded {
                        Ok((_, artifact)) => PrefetchedBody::Decoded {
                            artifact,
                            body_bytes_read: rest,
                        },
                        Err(error) => PrefetchedBody::DecodeError {
                            error,
                            body_bytes_read: rest,
                        },
                    };
                    (body, decode_ns)
                }
            };
            (
                PrefetchedHit {
                    id,
                    fields,
                    key,
                    artifact_digest,
                    prefix_bytes_read,
                    body,
                    candidate_index,
                    prior_miss,
                },
                read_ns,
                decode_ns,
            )
        });
        let stage_c_wall = stage_c_started.elapsed();
        let mut total_read_ns = 0u128;
        let mut total_decode_ns = 0u128;
        for (hit, read_ns, decode_ns) in decoded {
            total_read_ns += u128::from(read_ns);
            total_decode_ns += u128::from(decode_ns);
            self.prefetched.push_back(hit);
        }
        let total_cpu_ns = total_read_ns + total_decode_ns;
        if total_cpu_ns > 0 {
            let wall_ns = stage_c_wall.as_nanos();
            let read_wall =
                std::time::Duration::from_nanos((wall_ns * total_read_ns / total_cpu_ns) as u64);
            self.counts.validate_time += read_wall;
            self.counts.decode_time += stage_c_wall.saturating_sub(read_wall);
        } else {
            self.counts.decode_time += stage_c_wall;
        }
        true
    }

    /// Fallback for [`Self::lookup_batched`] when candidate `start_index - 1`
    /// failed `Reading::finish` in Stage C and the namespace has additional
    /// candidates to inspect.
    fn lookup_remaining_candidates(
        &mut self,
        loader: &LoaderRecord,
        id: NamespaceId,
        text: &str,
        sources: &dyn SourceProvider,
        cancelled: &dyn Fn() -> bool,
        start_index: usize,
    ) -> Result<Option<NamespaceArtifact>, String> {
        let name = loader.sources[id.0 as usize].name.clone();
        let candidates = match self.snapshot.candidates(&name) {
            Ok(candidates) if start_index < candidates.len() => candidates[start_index..].to_vec(),
            _ => return Ok(None),
        };
        let started = Instant::now();
        let mut found = None;
        for candidate in &candidates {
            let reading = match self.store.begin_read(candidate, &self.limits) {
                Ok(reading) => reading,
                Err(error) => {
                    self.miss("unreadable", error.to_string());
                    continue;
                }
            };
            self.counts.bytes_read += reading.prefix().len() as u64;
            let manifest = match artifact_codec::read_manifest_prefix(
                reading.prefix(),
                reading.length(),
                &self.limits,
            ) {
                Ok(manifest) if manifest.namespace == name => manifest,
                Ok(manifest) => {
                    self.miss(
                        "undecodable",
                        format!("the artifact is {}'s", manifest.namespace),
                    );
                    continue;
                }
                Err(error) => {
                    match self.finish_read(reading) {
                        Ok(_) => self.miss("undecodable", error.to_string()),
                        Err(read) => self.miss("unreadable", read.to_string()),
                    }
                    continue;
                }
            };
            let fields = &manifest.fields;
            self.prefix.push_stored(loader, fields, false);
            let key = self
                .keys
                .next(&self.environment, &self.prefix, id, text, &self.entry);
            let matched =
                if key.digest != candidate.key || fields.front_end_key.digest != key.digest {
                    match fields.front_end_key.mismatch(&key) {
                        Some(Mismatch::View) => {
                            crate::cache_key::inputs_unchanged(&fields.inputs, sources)
                                .and(Err(Mismatch::View))
                        }
                        mismatch => Err(mismatch.unwrap_or(Mismatch::Inconsistent)),
                    }
                } else {
                    crate::cache_key::inputs_unchanged(&fields.inputs, sources)
                };
            if let Err(mismatch) = matched {
                self.prefix.pop();
                self.miss(kind(&mismatch), mismatch.to_string());
                continue;
            }
            if fields.no_cache
                || fields.always
                || self.reaching(id).is_some()
                || self.origin(id, false, false).is_some()
            {
                self.prefix.pop();
                self.pending = None;
                break;
            }
            let bytes = match self.finish_read(reading) {
                Ok(bytes) => bytes,
                Err(error) => {
                    self.prefix.pop();
                    self.miss("unreadable", error.to_string());
                    continue;
                }
            };
            found = Some((key, candidate.artifact, bytes));
            break;
        }
        self.counts.validate_time += started.elapsed();
        let Some((key, artifact, bytes)) = found else {
            return Ok(None);
        };
        let started = Instant::now();
        let decoded = artifact_codec::decode_verified(&bytes, &self.limits, cancelled);
        drop(bytes);
        self.counts.decode_time += started.elapsed();
        match decoded {
            Ok((_, decoded)) => {
                self.pending = Some(Pending::Hit {
                    key: Box::new(key),
                    artifact,
                });
                Ok(Some(decoded))
            }
            Err(error) => {
                self.prefix.pop();
                self.pending = None;
                if error == artifact_codec::DecodeError::Cancelled {
                    return Err(error.to_string());
                }
                self.miss("undecodable", error.to_string());
                Ok(None)
            }
        }
    }

    /// The next namespace is analyzed without a lookup, because the
    /// Pants build may write a core function's value, which `description`
    /// names. It misses as [`CORE_WRITE`] and publishes nothing: no build
    /// that writes the function looks its artifact up, so publishing would
    /// only write what no identical rebuild reads.
    pub(crate) fn core_write(&mut self, description: String) {
        self.pending = Some(Pending::Miss(CORE_WRITE, description));
    }

    /// The artifact [`Self::lookup`] returned was installed.
    pub(crate) fn installed(&mut self, macros: &MacroHost) {
        let Some(Pending::Hit { key, artifact }) = self.pending.take() else {
            unreachable!("only a matched candidate is installed");
        };
        let id = NamespaceId(self.prefix.len() as u32 - 1);
        let name = self.prefix.record(id).name.clone();
        self.prefix.set_macros(id, declares_macros(macros, &name));
        if let Some(log) = macros.engine.dependency_log.borrow().as_ref() {
            self.prefix.skip_observations(log);
        }
        self.keys.append(self.prefix.record(id), *key);
        self.count_view(id);
        self.propagated.push(None);
        self.in_use.push(artifact);
        self.artifacts.push(Some(artifact));
        self.counts.hits += 1;
    }

    /// The artifact [`Self::lookup`] returned was refused; its namespace is
    /// analyzed instead.
    pub(crate) fn refused(&mut self, reason: &str) {
        self.prefetched.clear();
        self.prefix.pop();
        self.pending = None;
        self.miss("refused", reason.to_owned());
    }

    /// The namespace the last lookup did not install was analyzed: key it
    /// and decide its policy, and say whether its artifact, captured once
    /// its units merge after the next namespace's top-level pass, is to be
    /// published ([`Self::publish`]).
    pub(crate) fn analyzed(
        &mut self,
        loader: &LoaderRecord,
        text: &str,
        macros: &MacroHost,
        program_namespaces: &crate::arena::Arena<String>,
        declarations: Option<crate::dependency_manifest::DeclarationView>,
    ) -> Option<Publication> {
        if let Some(session) = macros.session_handle() {
            if self.slots == Slots::ONE
                && let Some(policy) = session.pool_policy()
                && let Some(slots) = Slots::new(policy.workers + 1)
            {
                self.slots = slots;
            }
            if self.pool.is_none() {
                self.pool = session.pool_task_submitter();
            }
        }
        let index = self.prefix.len();
        let name = loader.sources[index].name.clone();
        let id = {
            let log = macros.engine.dependency_log.borrow();
            let log = log.as_ref().expect("the cache records observations");
            self.prefix.push_analyzed(
                loader,
                log,
                program_namespaces,
                declares_macros(macros, &name),
            )
        };
        self.prefix.set_declarations(id, declarations);
        let key = self
            .keys
            .push(&self.environment, &self.prefix, id, text, &self.entry)
            .clone();
        self.count_view(id);
        let metadata = macros
            .namespace_symbol(&name)
            .map(NamespaceMetadata::of_symbol)
            .unwrap_or_default();
        let propagated = self
            .origin(id, metadata.always, true)
            .map(|reason| (reason, id.0))
            .or_else(|| self.reaching(id));
        if propagated.is_some() {
            self.any_propagated = true;
        }
        self.propagated.push(propagated);
        self.artifacts.push(None);
        let pending = self.pending.take();
        if metadata.no_cache {
            self.bypass(&name, Bypass::NoCache, String::new());
            return None;
        }
        if let Some((reason, _)) = propagated {
            let path = self.path(id);
            self.bypass(&name, reason, path);
            return None;
        }
        let (kind, description) = match pending {
            Some(Pending::Miss(kind, description)) => (kind, description),
            _ => ("no-artifact", "no artifact was published".to_owned()),
        };
        let core_write = kind == CORE_WRITE;
        self.counts.misses.push((name.clone(), kind, description));
        (!core_write).then_some(Publication { name, id, key })
    }

    /// Publish the artifact captured of the namespace [`Self::analyzed`]
    /// keyed as `publication`, read from `text`.
    pub(crate) fn publish(
        &mut self,
        publication: Publication,
        text: &str,
        sources: &dyn SourceProvider,
        artifact: Result<NamespaceArtifact, String>,
    ) {
        let started = Instant::now();
        let Publication { name, id, key } = publication;
        if self.slots.get() > 1 && self.pipeline.is_none() && !self.pipeline_failed {
            match PublishPipeline::start(
                self.store.clone(),
                self.slots,
                self.pool.clone(),
                #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
                self.cpu_work_hook.clone(),
            ) {
                Ok(pipeline) => self.pipeline = Some(pipeline),
                Err(_) => self.pipeline_failed = true,
            }
        }
        if self.pipeline.is_some() {
            let artifact = match artifact {
                Ok(artifact) => artifact,
                Err(reason) => {
                    if let Some(pipeline) = &mut self.pipeline {
                        pipeline.submit_capture_failed(id, name, reason);
                    }
                    self.counts.write_time += started.elapsed();
                    return;
                }
            };
            if self.store.write_failure().is_some() {
                drop(artifact);
                if let Some(pipeline) = &mut self.pipeline {
                    pipeline.submit_unwritable(id, name);
                }
                self.counts.write_time += started.elapsed();
                return;
            }
            let fields = self.manifest_fields(id, key);
            let owned_sources = sources.owned_concurrent();
            let rev_text_len = if owned_sources.is_some() {
                text.len()
            } else {
                0
            };
            let estimated_bytes = publish_estimated_bytes(&artifact, &fields, rev_text_len);
            if estimated_bytes <= PUBLISH_MAX_INFLIGHT_BYTES {
                let revalidate = match owned_sources {
                    Some(owned) => {
                        let inputs = RevalidateInputs::from_manifest(
                            &self.prefix,
                            id,
                            fields.inputs.clone(),
                        );
                        Some((inputs, text.to_owned(), owned))
                    }
                    None => {
                        if let Err(mismatch) =
                            crate::cache_key::revalidate(&self.prefix, id, text, sources)
                        {
                            drop(artifact);
                            if let Some(pipeline) = &mut self.pipeline {
                                pipeline.submit_refused(id, name, mismatch);
                            }
                            self.counts.write_time += started.elapsed();
                            return;
                        }
                        None
                    }
                };
                if let Some(pipeline) = &mut self.pipeline {
                    pipeline.enqueue(id, name, estimated_bytes, artifact, fields, revalidate);
                }
                self.counts.write_time += started.elapsed();
                return;
            }
            // Oversized artifact (> 32 MiB working set): drain background
            // staging/commits first so peak memory stays bounded and submission
            // order is preserved, then stage and commit inline.
            self.flush_publishes();
            self.publish_inline_artifact(id, name, text, sources, artifact, fields);
            self.commit_inline_batch();
            self.counts.write_time += started.elapsed();
            return;
        }
        let artifact = match artifact {
            Ok(artifact) => artifact,
            Err(reason) => {
                self.counts.unpublished.push((name, reason));
                return;
            }
        };
        let fields = self.manifest_fields(id, key);
        self.publish_inline_artifact(id, name, text, sources, artifact, fields);
        self.counts.write_time += started.elapsed();
    }

    fn publish_inline_artifact(
        &mut self,
        id: NamespaceId,
        name: String,
        text: &str,
        sources: &dyn SourceProvider,
        artifact: NamespaceArtifact,
        fields: ManifestFields,
    ) {
        let attempt = self.store.stage_attempt(
            &artifact,
            &fields,
            Inputs {
                manifest: &self.prefix,
                id,
                text,
                sources,
            },
        );
        drop(artifact);
        match attempt {
            StageAttempt::Staged(staged) => {
                self.inline_batch.push((id, name, staged));
                if self.inline_batch.len() >= PUBLISH_BATCH_SIZE {
                    self.commit_inline_batch();
                }
            }
            StageAttempt::Refused(mismatch) => {
                self.commit_inline_batch();
                if self.store.write_failure().is_some() {
                    self.counts
                        .unpublished
                        .push((name, "the cache cannot be written".into()));
                } else {
                    self.counts.unpublished.push((
                        name,
                        format!("its inputs changed while it compiled: {mismatch}"),
                    ));
                }
            }
            StageAttempt::Unencodable(reason) => {
                self.commit_inline_batch();
                if self.store.write_failure().is_some() {
                    self.counts
                        .unpublished
                        .push((name, "the cache cannot be written".into()));
                } else {
                    self.counts.unpublished.push((name, reason));
                }
            }
            StageAttempt::Io(error) => {
                // Commit earlier valid staged items before latching the store
                // failure so they are not discarded by a later staging error.
                self.commit_inline_batch();
                self.store.fail(&error);
                self.counts
                    .unpublished
                    .push((name, "the cache cannot be written".into()));
            }
            StageAttempt::Unwritable => self
                .counts
                .unpublished
                .push((name, "the cache cannot be written".into())),
        }
    }

    fn manifest_fields(&self, id: NamespaceId, key: FrontEndKey) -> ManifestFields {
        let record = self.prefix.record(id);
        ManifestFields {
            front_end_key: key,
            // One front-end artifact serves every emission mode, so it
            // records no emission key (docs/compiler-cache.md, "Conservative
            // keys and invalidation", light-cljs-324).
            emission_key: [0; 32],
            // The coarse fallback's edges are the `coarse` flag's.
            dependencies: self
                .prefix
                .row(id)
                .iter()
                .filter_map(|&(target, kinds)| Some((target, kinds.stored()?)))
                .map(|(target, kinds)| artifact_codec::Dependency {
                    namespace: self.prefix.record(target).name.clone(),
                    kinds: kinds.0,
                })
                .collect(),
            inputs: self.prefix.external_inputs(id).cloned().collect(),
            coarse: record.coarse,
            declarations: record.declarations.clone(),
            absent: record.absent.clone(),
            no_cache: false,
            always: false,
        }
    }

    fn commit_inline_batch(&mut self) {
        if self.inline_batch.is_empty() {
            return;
        }
        let batch = std::mem::take(&mut self.inline_batch);
        let mut meta = Vec::with_capacity(batch.len());
        let mut artifacts = Vec::with_capacity(batch.len());
        for (id, name, staged) in batch {
            meta.push((id, name));
            artifacts.push(staged);
        }
        let results = self.store.commit_batch(artifacts);
        for ((id, name), published) in meta.into_iter().zip(results) {
            match published {
                Publish::Published(entry) => {
                    self.counts.published += 1;
                    self.counts.bytes_written += entry.bytes;
                    self.in_use.push(entry.artifact);
                    self.artifacts[id.0 as usize] = Some(entry.artifact);
                }
                Publish::Refused(mismatch) => self.counts.unpublished.push((
                    name,
                    format!("its inputs changed while it compiled: {mismatch}"),
                )),
                Publish::Unencodable(reason) => self.counts.unpublished.push((name, reason)),
                Publish::Unwritable => self
                    .counts
                    .unpublished
                    .push((name, "the cache cannot be written".into())),
            }
        }
    }

    fn apply_finished(&mut self, outcomes: Vec<FinishedPublish>) {
        for item in outcomes {
            match item.result {
                FinishedResult::Published { artifact, bytes } => {
                    self.counts.published += 1;
                    self.counts.bytes_written += bytes;
                    self.in_use.push(artifact);
                    self.artifacts[item.id.0 as usize] = Some(artifact);
                }
                FinishedResult::Unpublished(reason) => {
                    self.counts.unpublished.push((item.name, reason));
                }
                FinishedResult::Unwritable => {
                    self.counts
                        .unpublished
                        .push((item.name, "the cache cannot be written".into()));
                }
            }
        }
    }

    /// Count what the view of namespace `id`, just keyed, covers.
    fn count_view(&mut self, id: NamespaceId) {
        let record = self.prefix.record(id);
        if record.coarse {
            self.counts.view_earlier += u64::from(id.0);
            self.counts.view_declaring += self.declaring;
        }
        if record
            .declarations
            .as_ref()
            .is_none_or(|view| view.prefix.is_some())
        {
            self.declaring += 1;
        }
    }

    fn bypass(&mut self, name: &str, reason: Bypass, path: String) {
        self.counts
            .bypasses
            .push((name.to_owned(), bypass_name(reason), path));
    }

    /// The propagated bypass namespace `id` sets itself: `^:dev/always`
    /// (`always`), a `--cache-blocker`, or, once analyzed (`analyzed`), an
    /// input that cannot be keyed.
    fn origin(&self, id: NamespaceId, always: bool, analyzed: bool) -> Option<Bypass> {
        let record = self.prefix.record(id);
        if always {
            return Some(Bypass::AlwaysCompile);
        }
        if self.blockers.contains(&record.name) {
            return Some(Bypass::Blocked);
        }
        let unkeyable = analyzed
            && self.prefix.external_inputs(id).any(|input| match input {
                ExternalInput::Resource(resource) => !resource.stable,
                // The prefix has only the namespaces so far.
                ExternalInput::View(BroadView::Namespaces) => true,
                _ => false,
            });
        unkeyable.then_some(Bypass::NonCacheableInput)
    }

    /// The propagated bypass of the first namespace `id` depends on through
    /// a compilation edge that one reaches, with that namespace.
    fn reaching(&self, id: NamespaceId) -> Option<(Bypass, u32)> {
        if !self.any_propagated {
            return None;
        }
        self.prefix
            .row(id)
            .iter()
            .filter(|(_, kinds)| kinds.intersects(EdgeKinds::COMPILATION))
            .find_map(|&(target, _)| {
                self.propagated[target.0 as usize].map(|(reason, _)| (reason, target.0))
            })
    }

    /// How the propagated bypass reached `id`: its origin first, then each
    /// namespace it came through, `a > b > c`.
    fn path(&self, id: NamespaceId) -> String {
        let mut path = vec![id.0];
        let mut at = id.0;
        while let Some((_, via)) = self.propagated[at as usize]
            && via != at
        {
            path.push(via);
            at = via;
        }
        path.iter()
            .rev()
            .map(|&id| self.prefix.record(NamespaceId(id)).name.as_str())
            .collect::<Vec<_>>()
            .join(" > ")
    }
}

impl Drop for CompileCache {
    fn drop(&mut self) {
        if !thread::panicking() {
            self.flush_publishes();
        }
    }
}

/// Number of staged artifacts committed per store lock acquisition.
const PUBLISH_BATCH_SIZE: usize = 64;
/// Tag added to staging job sequence numbers when dispatched on `UnitPool` so
/// scheduling-stress keys stay distinct from parallel top-level namespace keys.
const STAGE_JOB_INDEX_TAG: usize = 1usize << 30;
/// Tag added to commit-side CPU task sequence numbers when dispatched on
/// `UnitPool` so scheduling-stress keys stay distinct from top-level and
/// artifact staging keys.
const COMMIT_CPU_INDEX_TAG: usize = 1usize << 29;
/// Upper bound on in-flight publish jobs awaiting staging.
const PUBLISH_MAX_INFLIGHT_JOBS: usize = 16;
/// Upper bound on estimated bytes held by in-flight publish jobs (32 MiB).
const PUBLISH_MAX_INFLIGHT_BYTES: usize = 32 * 1024 * 1024;
/// Upper bound on uncommitted publish jobs waiting for commit.
const PUBLISH_MAX_PENDING_COMMIT_JOBS: usize = PUBLISH_BATCH_SIZE * 2;
/// Upper bound on estimated heap bytes retained by staged artifacts awaiting
/// commit (8 MiB).
const PUBLISH_MAX_PENDING_COMMIT_BYTES: usize = 8 * 1024 * 1024;

/// Conservative estimate of the peak memory working set held while staging one
/// artifact: `artifact.approximate_bytes()`, deferred `bodies` (which
/// `approximate_bytes()` omits), owned manifest metadata, the concurrent
/// revalidation source copy, and simultaneous encoder buffers.
fn publish_estimated_bytes(
    artifact: &NamespaceArtifact,
    fields: &ManifestFields,
    rev_text_len: usize,
) -> usize {
    use crate::namespace_artifact::{
        Expr, ExprRef, ExternalGlobal, Global, GlobalPatch, LocalBinding, NamedLiteral,
        RecurTarget, UnitCharge,
    };
    use std::mem::{size_of, size_of_val};

    let mut artifact_bytes = artifact.approximate_bytes();
    if let Some(bodies) = &artifact.bodies {
        let ir = &bodies.ir;
        artifact_bytes = artifact_bytes
            .saturating_add(ir.exprs.len().saturating_mul(size_of::<Expr>()))
            .saturating_add(
                ir.locals
                    .iter()
                    .map(|l| size_of::<LocalBinding>().saturating_add(l.name.len()))
                    .sum::<usize>(),
            )
            .saturating_add(
                ir.recur_targets
                    .len()
                    .saturating_mul(size_of::<RecurTarget>()),
            )
            .saturating_add(
                ir.strings
                    .iter()
                    .map(|s| size_of::<Vec<u16>>().saturating_add(s.len().saturating_mul(2)))
                    .sum::<usize>(),
            )
            .saturating_add(
                ir.named_literals
                    .len()
                    .saturating_mul(size_of::<NamedLiteral>()),
            )
            .saturating_add(size_of_val(ir.js_globals.as_slice()))
            .saturating_add(
                bodies
                    .declarations
                    .globals
                    .iter()
                    .map(|g| {
                        size_of::<Global>()
                            .saturating_add(g.namespace.len())
                            .saturating_add(g.name.len())
                    })
                    .sum::<usize>(),
            )
            .saturating_add(
                bodies
                    .declarations
                    .macros
                    .iter()
                    .map(|m| m.code.text.len().saturating_add(m.code.units.len() * 12))
                    .sum::<usize>(),
            )
            .saturating_add(
                bodies
                    .patches
                    .len()
                    .saturating_mul(size_of::<GlobalPatch>()),
            )
            .saturating_add(
                bodies
                    .references
                    .globals
                    .iter()
                    .map(|g| {
                        size_of::<ExternalGlobal>()
                            .saturating_add(g.namespace.len())
                            .saturating_add(g.name.len())
                    })
                    .sum::<usize>(),
            )
            .saturating_add(
                bodies
                    .references
                    .namespaces
                    .iter()
                    .map(String::len)
                    .sum::<usize>(),
            )
            .saturating_add(
                bodies
                    .placeholders
                    .len()
                    .saturating_mul(size_of::<(ExprRef, ExprRef)>()),
            )
            .saturating_add(bodies.charges.len().saturating_mul(size_of::<UnitCharge>()));
    }
    let fields_bytes = size_of::<ManifestFields>()
        .saturating_add(
            fields
                .dependencies
                .iter()
                .map(|d| size_of::<artifact_codec::Dependency>().saturating_add(d.namespace.len()))
                .sum::<usize>(),
        )
        .saturating_add(
            fields
                .front_end_key
                .dependencies
                .iter()
                .map(|d| 48usize.saturating_add(d.namespace.len()))
                .sum::<usize>(),
        )
        .saturating_add(fields.absent.iter().map(String::len).sum::<usize>())
        .saturating_add(fields.inputs.len().saturating_mul(96));
    // Factor of 2x on artifact_bytes accounts for the simultaneous Interner,
    // section buffers, and final encoded Vec<u8> during artifact_codec::encode;
    // 2x on rev_text_len accounts for the owned text and re-read copy during
    // revalidate.
    artifact_bytes
        .saturating_mul(2)
        .saturating_add(fields_bytes)
        .saturating_add(rev_text_len.saturating_mul(2))
}

struct StageJob {
    seq: usize,
    id: NamespaceId,
    name: String,
    estimated_bytes: usize,
    artifact: NamespaceArtifact,
    fields: ManifestFields,
    revalidate: Option<(
        RevalidateInputs,
        String,
        Arc<dyn SourceProvider + Send + Sync>,
    )>,
}

enum StageOutcome {
    Encoded(EncodedArtifact, usize),
    #[cfg(test)]
    Staged(StagedArtifact, usize),
    Refused(Mismatch),
    Unencodable(String),
    CaptureFailed(String),
    #[cfg(test)]
    Io(std::io::Error),
    Unwritable,
}

enum FinishedResult {
    Published { artifact: Digest, bytes: u64 },
    Unpublished(String),
    Unwritable,
}

struct FinishedPublish {
    seq: usize,
    id: NamespaceId,
    name: String,
    result: FinishedResult,
}

struct StageQueue {
    /// Priority queue for commit-side CPU work (index decode/encode/hash) so
    /// the I/O committer does not hold `v1/lock` longer than needed.
    commit_cpu_jobs: VecDeque<Box<dyn FnOnce() + Send>>,
    jobs: VecDeque<StageJob>,
    inflight_jobs: usize,
    inflight_bytes: usize,
    cancelled: bool,
}

struct CommitQueue {
    next_seq: usize,
    submitted: usize,
    pending: BTreeMap<usize, (NamespaceId, String, StageOutcome)>,
    staged_count: usize,
    staged_bytes: usize,
    /// Number of commit-side CPU tasks currently queued for execution by a
    /// pool worker or the coordinator. Updated under `commit_mu` before
    /// notifying `done_cv` so coordinator waits never miss a commit CPU task.
    pending_cpu: usize,
    finished: Vec<FinishedPublish>,
    completed: usize,
    flush_requested: bool,
    shutdown: bool,
    cancelled: bool,
    panic: Option<String>,
}

struct PublishShared {
    store: Store,
    pool: Option<PoolTaskSubmitter>,
    max_inflight_jobs: usize,
    max_inflight_bytes: usize,
    max_pending_commit_jobs: usize,
    max_pending_commit_bytes: usize,
    stage_mu: Mutex<StageQueue>,
    stage_space_cv: Condvar,
    commit_mu: Mutex<CommitQueue>,
    commit_cv: Condvar,
    done_cv: Condvar,
    #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
    cpu_work_hook: Option<Arc<dyn Fn(CacheCpuKind) + Send + Sync>>,
}

impl PublishShared {
    fn record_panic(&self, payload: Box<dyn std::any::Any + Send>) {
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "panic".to_owned());
        let dropped_cpu = {
            let mut stage = self.stage_mu.lock().unwrap_or_else(|e| e.into_inner());
            stage.cancelled = true;
            stage.jobs.clear();
            let dropped: Vec<_> = stage.commit_cpu_jobs.drain(..).collect();
            self.stage_space_cv.notify_all();
            dropped
        };
        drop(dropped_cpu);
        {
            let mut commit = self.commit_mu.lock().unwrap_or_else(|e| e.into_inner());
            commit.cancelled = true;
            commit.pending.clear();
            commit.pending_cpu = 0;
            if commit.panic.is_none() {
                commit.panic = Some(message);
            }
            self.commit_cv.notify_all();
            self.done_cv.notify_all();
        }
    }
}

struct PublishPipeline {
    shared: Arc<PublishShared>,
    submitted: usize,
    committer: Option<JoinHandle<()>>,
}

impl PublishPipeline {
    fn start(
        store: Store,
        slots: Slots,
        pool: Option<PoolTaskSubmitter>,
        #[cfg(any(debug_assertions, feature = "scheduling-stress"))] cpu_work_hook: Option<
            Arc<dyn Fn(CacheCpuKind) + Send + Sync>,
        >,
    ) -> std::io::Result<Self> {
        let max_inflight_jobs = (slots.get() * 2).min(PUBLISH_MAX_INFLIGHT_JOBS);
        Self::start_with_limits(
            store,
            pool,
            max_inflight_jobs,
            PUBLISH_MAX_INFLIGHT_BYTES,
            PUBLISH_MAX_PENDING_COMMIT_JOBS,
            PUBLISH_MAX_PENDING_COMMIT_BYTES,
            #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
            cpu_work_hook,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn start_with_limits(
        store: Store,
        pool: Option<PoolTaskSubmitter>,
        max_inflight_jobs: usize,
        max_inflight_bytes: usize,
        max_pending_commit_jobs: usize,
        max_pending_commit_bytes: usize,
        #[cfg(any(debug_assertions, feature = "scheduling-stress"))] cpu_work_hook: Option<
            Arc<dyn Fn(CacheCpuKind) + Send + Sync>,
        >,
    ) -> std::io::Result<Self> {
        let shared = Arc::new(PublishShared {
            store,
            pool,
            max_inflight_jobs: max_inflight_jobs.max(1),
            max_inflight_bytes: max_inflight_bytes.max(1),
            max_pending_commit_jobs: max_pending_commit_jobs.max(1),
            max_pending_commit_bytes: max_pending_commit_bytes.max(1),
            stage_mu: Mutex::new(StageQueue {
                commit_cpu_jobs: VecDeque::new(),
                jobs: VecDeque::new(),
                inflight_jobs: 0,
                inflight_bytes: 0,
                cancelled: false,
            }),
            stage_space_cv: Condvar::new(),
            commit_mu: Mutex::new(CommitQueue {
                next_seq: 0,
                submitted: 0,
                pending: BTreeMap::new(),
                staged_count: 0,
                staged_bytes: 0,
                pending_cpu: 0,
                finished: Vec::new(),
                completed: 0,
                flush_requested: false,
                shutdown: false,
                cancelled: false,
                panic: None,
            }),
            commit_cv: Condvar::new(),
            done_cv: Condvar::new(),
            #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
            cpu_work_hook,
        });

        // CPU-bound revalidation, artifact encoding/hashing, and commit-side
        // index decoding/encoding/hashing all run on the build's existing
        // `UnitPool` worker slots (or inline on the coordinator under
        // backpressure / flush), keeping CPU work within `--jobs N`. Only
        // blocking file I/O (temp writes, lock, renames) runs on the single
        // I/O committer thread.
        let shared_clone = Arc::clone(&shared);
        let committer = thread::Builder::new()
            .name("light-cljs-cache-commit".into())
            .spawn(move || commit_worker_loop(shared_clone))?;
        Ok(Self {
            shared,
            submitted: 0,
            committer: Some(committer),
        })
    }

    fn enqueue(
        &mut self,
        id: NamespaceId,
        name: String,
        estimated_bytes: usize,
        artifact: NamespaceArtifact,
        fields: ManifestFields,
        revalidate: Option<(
            RevalidateInputs,
            String,
            Arc<dyn SourceProvider + Send + Sync>,
        )>,
    ) {
        // Bound uncommitted staged work waiting on the committer (count and
        // bytes) so a blocked store lock cannot cause unbounded staged file
        // and heap accumulation.
        {
            let mut commit = self
                .shared
                .commit_mu
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            while !commit.cancelled
                && needs_commit_backpressure(&commit, self.submitted, &self.shared)
            {
                self.shared.commit_cv.notify_one();
                if commit.pending_cpu > 0 {
                    drop(commit);
                    pump_one_commit_cpu_job(&self.shared);
                    commit = self
                        .shared
                        .commit_mu
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    continue;
                }
                // Drain an unstarted staging job inline on the coordinator's
                // CPU slot before blocking on `done_cv`, so an unstarted job
                // holding `commit.next_seq` cannot deadlock if the pool worker
                // is waiting on the coordinator (e.g. `light.build`).
                drop(commit);
                if pump_one_stage_job(&self.shared) {
                    commit = self
                        .shared
                        .commit_mu
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    continue;
                }
                commit = self
                    .shared
                    .commit_mu
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                if commit.cancelled
                    || !needs_commit_backpressure(&commit, self.submitted, &self.shared)
                    || commit.pending_cpu > 0
                {
                    continue;
                }
                self.shared.commit_cv.notify_one();
                commit = self
                    .shared
                    .done_cv
                    .wait(commit)
                    .unwrap_or_else(|e| e.into_inner());
            }
            if commit.cancelled {
                drop(commit);
                drop(artifact);
                return;
            }
        }
        let seq = self.submitted;
        self.submitted += 1;
        loop {
            let mut stage = self
                .shared
                .stage_mu
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if stage.cancelled {
                drop(stage);
                drop(artifact);
                return;
            }
            let over_limit = stage.inflight_jobs > 0
                && (stage.inflight_jobs >= self.shared.max_inflight_jobs
                    || stage.inflight_bytes.saturating_add(estimated_bytes)
                        > self.shared.max_inflight_bytes);
            if !over_limit {
                stage.inflight_jobs += 1;
                stage.inflight_bytes = stage.inflight_bytes.saturating_add(estimated_bytes);
                stage.jobs.push_back(StageJob {
                    seq,
                    id,
                    name,
                    estimated_bytes,
                    artifact,
                    fields,
                    revalidate,
                });
                break;
            }
            // Service pending commit-side CPU work first (committer may hold
            // `v1/lock`), then the oldest unstarted staging job inline on the
            // coordinator's CPU slot so backpressure never deadlocks against a
            // pool worker waiting on the coordinator.
            if let Some(cpu_job) = stage.commit_cpu_jobs.pop_front() {
                drop(stage);
                decrement_pending_cpu(&self.shared);
                cpu_job();
                continue;
            }
            if let Some(job) = stage.jobs.pop_front() {
                drop(stage);
                execute_stage_job(&self.shared, job);
                continue;
            }
            // All in-flight jobs are already inside `run_stage` on pool
            // workers (which never wait on the coordinator), so waiting here
            // for one to finish is deadlock-free.
            let _stage = self
                .shared
                .stage_space_cv
                .wait(stage)
                .unwrap_or_else(|e| e.into_inner());
        }
        let dispatched = self.shared.pool.as_ref().is_some_and(|pool| {
            let shared = Arc::clone(&self.shared);
            pool.submit_plain(
                STAGE_JOB_INDEX_TAG | seq,
                Box::new(move || {
                    pump_one_cpu_job(&shared);
                }),
            )
        });
        if !dispatched {
            pump_one_cpu_job(&self.shared);
        }
    }

    fn submit_capture_failed(&mut self, id: NamespaceId, name: String, reason: String) {
        self.submit_immediate(id, name, StageOutcome::CaptureFailed(reason));
    }

    fn submit_refused(&mut self, id: NamespaceId, name: String, mismatch: Mismatch) {
        self.submit_immediate(id, name, StageOutcome::Refused(mismatch));
    }

    fn submit_unwritable(&mut self, id: NamespaceId, name: String) {
        self.submit_immediate(id, name, StageOutcome::Unwritable);
    }

    fn submit_immediate(&mut self, id: NamespaceId, name: String, outcome: StageOutcome) {
        let seq = self.submitted;
        self.submitted += 1;
        let mut commit = self
            .shared
            .commit_mu
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if commit.cancelled {
            return;
        }
        commit.pending.insert(seq, (id, name, outcome));
        if seq == commit.next_seq {
            self.shared.commit_cv.notify_one();
        }
    }

    fn wait_for_completed(&mut self, target: usize) {
        let mut commit = self
            .shared
            .commit_mu
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if commit.completed < target && !commit.cancelled {
            commit.submitted = target;
            commit.flush_requested = true;
            self.shared.commit_cv.notify_one();
            while commit.completed < target && !commit.cancelled {
                if commit.pending_cpu > 0 {
                    drop(commit);
                    pump_one_commit_cpu_job(&self.shared);
                    commit = self
                        .shared
                        .commit_mu
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    continue;
                }
                drop(commit);
                if pump_one_stage_job(&self.shared) {
                    commit = self
                        .shared
                        .commit_mu
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    continue;
                }
                commit = self
                    .shared
                    .commit_mu
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                if commit.completed >= target || commit.cancelled || commit.pending_cpu > 0 {
                    continue;
                }
                commit = self
                    .shared
                    .done_cv
                    .wait(commit)
                    .unwrap_or_else(|e| e.into_inner());
            }
        }
    }

    fn flush(&mut self) -> Vec<FinishedPublish> {
        let target = self.submitted;
        self.wait_for_completed(target);
        let mut commit = self
            .shared
            .commit_mu
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let panic = commit.panic.take();
        let mut finished = std::mem::take(&mut commit.finished);
        drop(commit);
        if let Some(message) = panic {
            panic!("cache publish worker panicked: {message}");
        }
        finished.sort_by_key(|item| item.seq);
        finished
    }
}

impl Drop for PublishPipeline {
    fn drop(&mut self) {
        let cancelling = thread::panicking();
        if !cancelling {
            self.wait_for_completed(self.submitted);
        }
        let dropped_cpu = {
            let mut stage = self
                .shared
                .stage_mu
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let dropped = if cancelling {
                stage.cancelled = true;
                stage.jobs.clear();
                stage.commit_cpu_jobs.drain(..).collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            self.shared.stage_space_cv.notify_all();
            dropped
        };
        drop(dropped_cpu);
        {
            let mut commit = self
                .shared
                .commit_mu
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            commit.submitted = self.submitted;
            if cancelling {
                commit.cancelled = true;
                commit.pending.clear();
                commit.pending_cpu = 0;
            } else {
                commit.shutdown = true;
            }
            self.shared.commit_cv.notify_all();
            self.shared.done_cv.notify_all();
        }
        if let Some(handle) = self.committer.take() {
            let _ = handle.join();
        }
    }
}

fn needs_commit_backpressure(
    commit: &CommitQueue,
    submitted: usize,
    shared: &PublishShared,
) -> bool {
    (submitted.saturating_sub(commit.completed) >= shared.max_pending_commit_jobs
        && submitted > commit.completed)
        || (commit.staged_count > 0 && commit.staged_bytes >= shared.max_pending_commit_bytes)
}

fn decrement_pending_cpu(shared: &PublishShared) {
    let mut commit = shared.commit_mu.lock().unwrap_or_else(|e| e.into_inner());
    commit.pending_cpu = commit.pending_cpu.saturating_sub(1);
}

fn pump_one_commit_cpu_job(shared: &Arc<PublishShared>) -> bool {
    let job = {
        let mut stage = shared.stage_mu.lock().unwrap_or_else(|e| e.into_inner());
        if stage.cancelled {
            return false;
        }
        stage.commit_cpu_jobs.pop_front()
    };
    let Some(job) = job else {
        return false;
    };
    decrement_pending_cpu(shared);
    job();
    true
}

fn pump_one_stage_job(shared: &Arc<PublishShared>) -> bool {
    let job = {
        let mut stage = shared.stage_mu.lock().unwrap_or_else(|e| e.into_inner());
        if stage.cancelled {
            return false;
        }
        match stage.jobs.pop_front() {
            Some(job) => job,
            None => return false,
        }
    };
    execute_stage_job(shared, job);
    true
}

/// Execute one queued CPU job, prioritizing commit-side CPU tasks (which may
/// unblock the I/O committer holding `v1/lock`) ahead of artifact staging.
fn pump_one_cpu_job(shared: &Arc<PublishShared>) -> bool {
    if pump_one_commit_cpu_job(shared) {
        return true;
    }
    pump_one_stage_job(shared)
}

struct CpuReplySlot<R> {
    state: Mutex<CpuReplyState<R>>,
    cv: Condvar,
}

enum CpuReplyState<R> {
    Pending,
    Ready(Result<R, Box<dyn std::any::Any + Send>>),
    Cancelled,
}

/// Ensures the waiting committer thread is always woken if a queued commit-CPU
/// closure is dropped without running (for example on panic/cancellation).
struct CpuReplySender<R> {
    slot: Arc<CpuReplySlot<R>>,
    sent: bool,
}

impl<R> CpuReplySender<R> {
    fn complete(mut self, result: Result<R, Box<dyn std::any::Any + Send>>) {
        self.sent = true;
        let mut state = self.slot.state.lock().unwrap_or_else(|e| e.into_inner());
        *state = CpuReplyState::Ready(result);
        drop(state);
        self.slot.cv.notify_one();
    }
}

impl<R> Drop for CpuReplySender<R> {
    fn drop(&mut self) {
        if !self.sent {
            let mut state = self.slot.state.lock().unwrap_or_else(|e| e.into_inner());
            if matches!(*state, CpuReplyState::Pending) {
                *state = CpuReplyState::Cancelled;
            }
            drop(state);
            self.slot.cv.notify_one();
        }
    }
}

struct CommitCpuRunner<'a> {
    shared: &'a Arc<PublishShared>,
    next_key: usize,
}

impl CommitCpu for CommitCpuRunner<'_> {
    fn run<R: Send + 'static>(&mut self, work: impl FnOnce() -> R + Send + 'static) -> Option<R> {
        let slot = Arc::new(CpuReplySlot {
            state: Mutex::new(CpuReplyState::Pending),
            cv: Condvar::new(),
        });
        let sender = CpuReplySender {
            slot: Arc::clone(&slot),
            sent: false,
        };
        #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
        let hook = self.shared.cpu_work_hook.clone();
        let task: Box<dyn FnOnce() + Send> = Box::new(move || {
            let result = panic::catch_unwind(AssertUnwindSafe(move || {
                #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
                if let Some(hook) = hook {
                    hook(CacheCpuKind::IndexEncodeHash);
                }
                work()
            }));
            sender.complete(result);
        });
        {
            let mut stage = self
                .shared
                .stage_mu
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if stage.cancelled {
                drop(stage);
                drop(task);
                return None;
            }
            stage.commit_cpu_jobs.push_back(task);
            self.shared.stage_space_cv.notify_all();
        }
        {
            let mut commit = self
                .shared
                .commit_mu
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if commit.cancelled {
                return None;
            }
            commit.pending_cpu += 1;
            self.shared.done_cv.notify_all();
        }
        if let Some(pool) = &self.shared.pool {
            let key = COMMIT_CPU_INDEX_TAG | self.next_key;
            self.next_key = self.next_key.wrapping_add(1) & (COMMIT_CPU_INDEX_TAG - 1);
            let shared = Arc::clone(self.shared);
            let _ = pool.submit_plain(
                key,
                Box::new(move || {
                    pump_one_cpu_job(&shared);
                }),
            );
        }
        let mut state = slot.state.lock().unwrap_or_else(|e| e.into_inner());
        while matches!(*state, CpuReplyState::Pending) {
            state = slot.cv.wait(state).unwrap_or_else(|e| e.into_inner());
        }
        match std::mem::replace(&mut *state, CpuReplyState::Cancelled) {
            CpuReplyState::Ready(Ok(value)) => Some(value),
            CpuReplyState::Ready(Err(payload)) => {
                drop(state);
                self.shared.record_panic(payload);
                None
            }
            CpuReplyState::Cancelled | CpuReplyState::Pending => None,
        }
    }
}

fn execute_stage_job(shared: &Arc<PublishShared>, job: StageJob) {
    let StageJob {
        seq,
        id,
        name,
        estimated_bytes,
        artifact,
        fields,
        revalidate,
    } = job;

    let staged = panic::catch_unwind(AssertUnwindSafe(|| {
        run_stage(shared, &name, artifact, fields, revalidate)
    }));

    {
        let mut stage = shared.stage_mu.lock().unwrap_or_else(|e| e.into_inner());
        stage.inflight_jobs = stage.inflight_jobs.saturating_sub(1);
        stage.inflight_bytes = stage.inflight_bytes.saturating_sub(estimated_bytes);
        shared.stage_space_cv.notify_one();
    }

    let outcome = match staged {
        Ok(outcome) => outcome,
        Err(payload) => {
            shared.record_panic(payload);
            return;
        }
    };

    let mut commit = shared.commit_mu.lock().unwrap_or_else(|e| e.into_inner());
    if commit.cancelled {
        drop(outcome);
        return;
    }
    match &outcome {
        StageOutcome::Encoded(_, bytes) => {
            commit.staged_count += 1;
            commit.staged_bytes = commit.staged_bytes.saturating_add(*bytes);
        }
        #[cfg(test)]
        StageOutcome::Staged(_, bytes) => {
            commit.staged_count += 1;
            commit.staged_bytes = commit.staged_bytes.saturating_add(*bytes);
        }
        _ => {}
    }
    let pressure = commit.staged_count >= shared.max_pending_commit_jobs
        || commit.staged_bytes >= shared.max_pending_commit_bytes;
    commit.pending.insert(seq, (id, name, outcome));
    if seq == commit.next_seq || pressure {
        shared.commit_cv.notify_one();
    }
}

fn run_stage(
    shared: &PublishShared,
    name: &str,
    mut artifact: NamespaceArtifact,
    fields: ManifestFields,
    revalidate: Option<(
        RevalidateInputs,
        String,
        Arc<dyn SourceProvider + Send + Sync>,
    )>,
) -> StageOutcome {
    if shared.store.write_failure().is_some() {
        drop(revalidate);
        drop(artifact);
        drop(fields);
        return StageOutcome::Unwritable;
    }
    if let Some((inputs, text, sources)) = revalidate {
        let check = crate::cache_key::revalidate_owned(&inputs, &text, &*sources);
        drop(text);
        drop(inputs);
        drop(sources);
        if let Err(mismatch) = check {
            drop(artifact);
            drop(fields);
            return StageOutcome::Refused(mismatch);
        }
    }
    #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
    if let Some(hook) = &shared.cpu_work_hook {
        hook(CacheCpuKind::ArtifactEncodeHash);
    }
    let encoded = artifact_codec::encode(&artifact, &fields);
    let names = std::mem::take(&mut artifact.names);
    drop(artifact);
    let key = fields.front_end_key.digest;
    drop(fields);
    let bytes = match encoded {
        Ok(bytes) => bytes,
        Err(reason) => return StageOutcome::Unencodable(reason),
    };
    // Only CPU work (encoding + SHA-256 digest) runs on the pool worker; the
    // temporary-file write runs on the I/O committer thread so slow cache I/O
    // never occupies a deferred-analysis worker slot.
    let prepared = Store::prepare_encoded(name.to_owned(), key, names, bytes);
    let encoded_bytes = prepared.approximate_bytes();
    StageOutcome::Encoded(prepared, encoded_bytes)
}

type CommitBatchItem = (usize, NamespaceId, String, StagedArtifact, usize);

fn commit_batch_and_record(
    shared: &Arc<PublishShared>,
    cpu: &mut CommitCpuRunner<'_>,
    to_commit: Vec<CommitBatchItem>,
) {
    if to_commit.is_empty() {
        return;
    }
    let mut meta = Vec::with_capacity(to_commit.len());
    let mut artifacts = Vec::with_capacity(to_commit.len());
    let mut batch_staged_bytes = 0usize;
    for (seq, id, name, staged, staged_bytes) in to_commit {
        meta.push((seq, id, name));
        artifacts.push(staged);
        batch_staged_bytes = batch_staged_bytes.saturating_add(staged_bytes);
    }
    let batch_len = artifacts.len();
    let results = shared.store.commit_batch_with_cpu(artifacts, cpu);

    let mut commit = shared.commit_mu.lock().unwrap_or_else(|e| e.into_inner());
    commit.staged_count = commit.staged_count.saturating_sub(batch_len);
    commit.staged_bytes = commit.staged_bytes.saturating_sub(batch_staged_bytes);
    for ((seq, id, name), published) in meta.into_iter().zip(results) {
        let result = match published {
            Publish::Published(entry) => FinishedResult::Published {
                artifact: entry.artifact,
                bytes: entry.bytes,
            },
            Publish::Refused(mismatch) => FinishedResult::Unpublished(format!(
                "its inputs changed while it compiled: {mismatch}"
            )),
            Publish::Unencodable(reason) => FinishedResult::Unpublished(reason),
            Publish::Unwritable => FinishedResult::Unwritable,
        };
        commit.finished.push(FinishedPublish {
            seq,
            id,
            name,
            result,
        });
        commit.completed += 1;
    }
    if commit.flush_requested && commit.completed == commit.submitted {
        commit.flush_requested = false;
    }
    shared.done_cv.notify_all();
}

struct WriteEncodedItem {
    seq: usize,
    id: NamespaceId,
    name: String,
    encoded: EncodedArtifact,
    encoded_bytes: usize,
}

enum CommitAction {
    WriteEncoded(Box<WriteEncodedItem>),
    CommitBatch(Vec<CommitBatchItem>),
    Exit,
}

fn commit_worker_loop(shared: Arc<PublishShared>) {
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        let mut batch: Vec<CommitBatchItem> = Vec::with_capacity(PUBLISH_BATCH_SIZE);
        let mut cpu = CommitCpuRunner {
            shared: &shared,
            next_key: 0,
        };
        loop {
            let action = {
                let mut commit = shared.commit_mu.lock().unwrap_or_else(|e| e.into_inner());
                loop {
                    if commit.cancelled {
                        drop(batch);
                        commit.pending.clear();
                        commit.staged_count = 0;
                        commit.staged_bytes = 0;
                        commit.pending_cpu = 0;
                        shared.done_cv.notify_all();
                        return;
                    }
                    // Drain contiguous completed staging outcomes in submission order.
                    let mut barrier = false;
                    let mut completed_any = false;
                    let mut write_encoded = None;
                    while let Some(entry) = commit.pending.get(&commit.next_seq) {
                        let is_barrier = match &entry.2 {
                            StageOutcome::Refused(_) | StageOutcome::Unencodable(_) => true,
                            #[cfg(test)]
                            StageOutcome::Io(_) => true,
                            _ => false,
                        };
                        if !batch.is_empty() && is_barrier {
                            // Commit earlier valid staged items before deciding
                            // store-failure precedence for this outcome.
                            barrier = true;
                            break;
                        }
                        let seq = commit.next_seq;
                        let (id, name, outcome) = commit.pending.remove(&seq).unwrap();
                        commit.next_seq += 1;
                        match outcome {
                            StageOutcome::Encoded(encoded, encoded_bytes) => {
                                if shared.store.write_failure().is_some() {
                                    drop(encoded);
                                    commit.staged_count = commit.staged_count.saturating_sub(1);
                                    commit.staged_bytes =
                                        commit.staged_bytes.saturating_sub(encoded_bytes);
                                    commit.finished.push(FinishedPublish {
                                        seq,
                                        id,
                                        name,
                                        result: FinishedResult::Unwritable,
                                    });
                                    commit.completed += 1;
                                    completed_any = true;
                                } else {
                                    write_encoded = Some((seq, id, name, encoded, encoded_bytes));
                                    break;
                                }
                            }
                            #[cfg(test)]
                            StageOutcome::Staged(staged, staged_bytes) => {
                                if shared.store.write_failure().is_some() {
                                    drop(staged);
                                    commit.staged_count = commit.staged_count.saturating_sub(1);
                                    commit.staged_bytes =
                                        commit.staged_bytes.saturating_sub(staged_bytes);
                                    commit.finished.push(FinishedPublish {
                                        seq,
                                        id,
                                        name,
                                        result: FinishedResult::Unwritable,
                                    });
                                    commit.completed += 1;
                                    completed_any = true;
                                } else {
                                    batch.push((seq, id, name, staged, staged_bytes));
                                    if batch.len() >= PUBLISH_BATCH_SIZE {
                                        break;
                                    }
                                }
                            }
                            StageOutcome::CaptureFailed(reason) => {
                                commit.finished.push(FinishedPublish {
                                    seq,
                                    id,
                                    name,
                                    result: FinishedResult::Unpublished(reason),
                                });
                                commit.completed += 1;
                                completed_any = true;
                            }
                            StageOutcome::Refused(mismatch) => {
                                let result = if shared.store.write_failure().is_some() {
                                    FinishedResult::Unwritable
                                } else {
                                    FinishedResult::Unpublished(format!(
                                        "its inputs changed while it compiled: {mismatch}"
                                    ))
                                };
                                commit.finished.push(FinishedPublish {
                                    seq,
                                    id,
                                    name,
                                    result,
                                });
                                commit.completed += 1;
                                completed_any = true;
                            }
                            StageOutcome::Unencodable(reason) => {
                                let result = if shared.store.write_failure().is_some() {
                                    FinishedResult::Unwritable
                                } else {
                                    FinishedResult::Unpublished(reason)
                                };
                                commit.finished.push(FinishedPublish {
                                    seq,
                                    id,
                                    name,
                                    result,
                                });
                                commit.completed += 1;
                                completed_any = true;
                            }
                            #[cfg(test)]
                            StageOutcome::Io(error) => {
                                shared.store.fail(&error);
                                commit.finished.push(FinishedPublish {
                                    seq,
                                    id,
                                    name,
                                    result: FinishedResult::Unwritable,
                                });
                                commit.completed += 1;
                                completed_any = true;
                            }
                            StageOutcome::Unwritable => {
                                commit.finished.push(FinishedPublish {
                                    seq,
                                    id,
                                    name,
                                    result: FinishedResult::Unwritable,
                                });
                                commit.completed += 1;
                                completed_any = true;
                            }
                        }
                    }

                    if completed_any {
                        shared.done_cv.notify_all();
                    }
                    if let Some((seq, id, name, encoded, encoded_bytes)) = write_encoded {
                        break CommitAction::WriteEncoded(Box::new(WriteEncodedItem {
                            seq,
                            id,
                            name,
                            encoded,
                            encoded_bytes,
                        }));
                    }

                    let flush_ready = (commit.flush_requested || commit.shutdown)
                        && commit.next_seq == commit.submitted;
                    let pressure_commit = commit.staged_count >= shared.max_pending_commit_jobs
                        || commit.staged_bytes >= shared.max_pending_commit_bytes;
                    if batch.len() >= PUBLISH_BATCH_SIZE
                        || (!batch.is_empty() && (flush_ready || barrier || pressure_commit))
                    {
                        break CommitAction::CommitBatch(std::mem::take(&mut batch));
                    }
                    if commit.flush_requested && commit.completed == commit.submitted {
                        commit.flush_requested = false;
                        shared.done_cv.notify_all();
                    }
                    if commit.shutdown && commit.next_seq == commit.submitted {
                        shared.done_cv.notify_all();
                        break CommitAction::Exit;
                    }
                    commit = shared
                        .commit_cv
                        .wait(commit)
                        .unwrap_or_else(|e| e.into_inner());
                }
            };

            match action {
                CommitAction::WriteEncoded(item) => {
                    let WriteEncodedItem {
                        seq,
                        id,
                        name,
                        encoded,
                        encoded_bytes,
                    } = *item;
                    match shared.store.write_staged(encoded) {
                        Ok(staged) => {
                            let staged_bytes = staged.approximate_bytes();
                            {
                                let mut commit =
                                    shared.commit_mu.lock().unwrap_or_else(|e| e.into_inner());
                                commit.staged_bytes = commit
                                    .staged_bytes
                                    .saturating_sub(encoded_bytes)
                                    .saturating_add(staged_bytes);
                                if commit.cancelled {
                                    drop(staged);
                                    drop(batch);
                                    commit.pending.clear();
                                    commit.staged_count = 0;
                                    commit.staged_bytes = 0;
                                    commit.pending_cpu = 0;
                                    shared.done_cv.notify_all();
                                    return;
                                }
                                shared.done_cv.notify_all();
                            }
                            batch.push((seq, id, name, staged, staged_bytes));
                            if batch.len() >= PUBLISH_BATCH_SIZE {
                                commit_batch_and_record(
                                    &shared,
                                    &mut cpu,
                                    std::mem::take(&mut batch),
                                );
                            }
                        }
                        Err(error) => {
                            if !batch.is_empty() {
                                commit_batch_and_record(
                                    &shared,
                                    &mut cpu,
                                    std::mem::take(&mut batch),
                                );
                            }
                            shared.store.fail(&error);
                            let mut commit =
                                shared.commit_mu.lock().unwrap_or_else(|e| e.into_inner());
                            commit.staged_count = commit.staged_count.saturating_sub(1);
                            commit.staged_bytes = commit.staged_bytes.saturating_sub(encoded_bytes);
                            commit.finished.push(FinishedPublish {
                                seq,
                                id,
                                name,
                                result: FinishedResult::Unwritable,
                            });
                            commit.completed += 1;
                            if commit.flush_requested
                                && commit.completed == commit.submitted
                                && batch.is_empty()
                            {
                                commit.flush_requested = false;
                            }
                            shared.done_cv.notify_all();
                        }
                    }
                }
                CommitAction::CommitBatch(to_commit) => {
                    commit_batch_and_record(&shared, &mut cpu, to_commit);
                }
                CommitAction::Exit => return,
            }
        }
    }));
    if let Err(payload) = result {
        shared.record_panic(payload);
    }
}

/// What one analyzed namespace gives [`declaration_view`].
pub(crate) struct DeclarationSource<'a> {
    pub namespace: &'a str,
    pub source_index: usize,
    /// The name its spans and registration locations give its source.
    pub source_name: &'a str,
    pub text: &'a str,
    /// Its top-level forms' byte ranges, in source order.
    pub forms: &'a [std::ops::Range<usize>],
    /// End marks captured immediately after this namespace's top-level pass,
    /// before later namespaces run during deferred keying.
    pub units_end: usize,
    pub registrations_end: usize,
    pub globals_end: usize,
    /// Whether this namespace's deferred units have already merged into
    /// `program` (`merge_front`).
    pub merged: bool,
}

/// What a source macro of a later namespace can see of the namespace
/// `source` just analyzed in `program`, whose analysis began at `start`
/// ([`crate::dependency_manifest::DeclarationView`], light-cljs-347): its
/// reader context and the Var names that change a code read with `*ns*`
/// bound to it, and, when it registered a declaration bundle or declared a
/// macro, the SHA-256 of its text up to the end of the last top-level form
/// that did. The prefix is the whole text when it declares a macro (its
/// macros' places are not looked for), when a registration's location is not
/// in its own source, or when a form after that end defines a Var that
/// existed before it: a redefinition, or the definition of a `declare`d
/// Var, which code before the end may call. The Vars its units' bodies
/// define (a `def` in a function) count too, read from their settled
/// fragments (or merged unit records once drained); when a fragment cannot
/// be read, or there is no session, the view is `None`, unknown, and later
/// views cover its whole key.
pub(crate) fn declaration_view(
    program: &crate::analyzer::Program,
    start: &crate::artifact_capture::CaptureStart,
    session: Option<&crate::compilation_session::CompilationSession>,
    macros: &MacroHost,
    source: &DeclarationSource<'_>,
) -> Option<crate::dependency_manifest::DeclarationView> {
    use crate::compilation_session::RegistrationRecord;
    use crate::sha256::Fingerprint;
    let namespace = source.namespace;
    let session = session?;
    let registrations =
        session.captured_registrations(start.registrations()..source.registrations_end);
    // The globals its units' bodies add (a `def` in a function) reach the
    // program, and a later macro's reader tables, only when they merge: read
    // them from settled fragments before merge, or merged records after merge.
    // Bundle units belong to the declaration prefix, and `:test` units append
    // their IR into their owning definition's fragment and release their own
    // `Arc<Program>` immediately (`release_definition_test`).
    let skip_units: Vec<usize> = registrations
        .iter()
        .filter_map(|registration| match registration {
            RegistrationRecord::Bundle(bundle) => Some(bundle.unit),
            RegistrationRecord::Root {
                test: Some(test), ..
            } => Some(*test),
            RegistrationRecord::Root { test: None, .. } => None,
        })
        .collect();
    let deferred = session.unit_global_names(
        start.units()..source.units_end,
        namespace,
        &skip_units,
        source.merged.then_some(program),
    )?;
    let mut interface = Fingerprint::new("light-cljs declaration interface");
    match program.reader_contexts.get(namespace) {
        None => interface.absent(),
        Some(context) => {
            interface.present().count(context.aliases.len());
            for (alias, target) in &context.aliases {
                interface.text(alias).text(target);
            }
            interface.count(context.refers.len());
            for (local, target, original) in &context.refers {
                interface.text(local).text(target).text(original);
            }
            interface.count(context.excludes.len());
            for name in &context.excludes {
                interface.text(name);
            }
            interface.count(context.macros.len());
            for name in &context.macros {
                interface.text(name);
            }
            // A code read qualifies a symbol by whether the namespace binds
            // it only when it is a core name or a refer.
            let mut shadowing: Vec<&str> = program
                .globals
                .range(start.globals()..source.globals_end)
                .filter(|global| global.namespace == namespace)
                .map(|global| global.name.as_str())
                .chain(deferred.iter().map(String::as_str))
                .filter(|name| {
                    reader_core_name(name) || context.refers.iter().any(|(local, ..)| local == name)
                })
                .collect();
            shadowing.sort_unstable();
            shadowing.dedup();
            interface.count(shadowing.len());
            for name in shadowing {
                interface.text(name);
            }
            &mut interface
        }
    };
    let interface = interface.finish();
    let declares = macros.declares_source_macros(namespace);
    let bundles = registrations
        .iter()
        .any(|registration| matches!(registration, RegistrationRecord::Bundle(_)));
    if !declares && !bundles {
        return Some(crate::dependency_manifest::DeclarationView {
            interface,
            prefix: None,
        });
    }
    let whole = source.text.len();
    let lines = crate::compiler_data::Source::new(source.source_name, source.text);
    let position = |offset: usize| lines.line_column(offset);
    // The top-level form holding a registration's location, by index.
    let form_of = |location: &str| -> Option<usize> {
        let mut parts = location.rsplitn(3, ':');
        let column: usize = parts.next()?.parse().ok()?;
        let line: usize = parts.next()?.parse().ok()?;
        (parts.next()? == source.source_name).then_some(())?;
        let at = (line, column);
        let index = source
            .forms
            .partition_point(|form| position(form.start).is_some_and(|start| start <= at));
        let index = index.checked_sub(1)?;
        (position(source.forms[index].end.saturating_sub(1))? >= at).then_some(index)
    };
    let end = (|| -> Option<usize> {
        if declares {
            return None;
        }
        let mut last = None;
        for registration in &registrations {
            if let RegistrationRecord::Bundle(bundle) = registration {
                let index = form_of(&bundle.location)?;
                last = last.max(Some(index));
            }
        }
        let end = source.forms[last?].end;
        // A Var defined after the end must be new there.
        for registration in &registrations {
            let RegistrationRecord::Root { name, location, .. } = registration else {
                continue;
            };
            if form_of(location)? <= last? {
                continue;
            }
            // A `:test` unit is named after its Var.
            let local = name.strip_prefix(namespace)?.strip_prefix('/')?;
            let local = local.split_once(' ').map_or(local, |(var, _)| var);
            let global = program
                .globals
                .range(start.globals()..source.globals_end)
                .find(|global| global.namespace == namespace && global.name == local)?;
            if global.source_index != source.source_index || global.span.start < end {
                return None;
            }
        }
        Some(end)
    })()
    .unwrap_or(whole);
    let mut prefix = Fingerprint::new("light-cljs declaration prefix");
    prefix.number(end as u64).text(&source.text[..end]);
    Some(crate::dependency_manifest::DeclarationView {
        interface,
        prefix: Some(prefix.finish()),
    })
}

/// Whether the compile-time code reader treats `name` as a `cljs.core`
/// name (`readerCoreNames` in `src/js_runtime.js`, from the same tables).
fn reader_core_name(name: &str) -> bool {
    static NAMES: std::sync::OnceLock<std::collections::HashSet<&'static str>> =
        std::sync::OnceLock::new();
    NAMES
        .get_or_init(|| {
            crate::compiler_reader::core_public_bindings()
                .map(|(name, _)| name)
                .chain(
                    include_str!("core_var_metadata.tsv")
                        .lines()
                        .filter(|line| !line.starts_with(";;"))
                        .filter_map(|line| line.split_once('\t').map(|(name, _)| name)),
                )
                .collect()
        })
        .contains(name)
}

/// Whether `namespace` declares macros in `macros`' inventory, which makes a
/// require of it a macro-provider edge. A lookup of its names, not a scan
/// of every macro, so keying stays linear in the namespaces.
fn declares_macros(macros: &MacroHost, namespace: &str) -> bool {
    macros.declares_source_macros(namespace)
}

/// The `--stats` kind of a key mismatch: its component.
fn kind(mismatch: &Mismatch) -> &'static str {
    match mismatch {
        Mismatch::Compiler => "compiler",
        Mismatch::Options => "options",
        Mismatch::Config => "config",
        Mismatch::Source => "source",
        Mismatch::Resolution => "resolution",
        Mismatch::Dependency(_) => "dependencies",
        Mismatch::Inputs => "inputs",
        Mismatch::View => "view",
        Mismatch::Inconsistent => "inconsistent",
    }
}

fn bypass_name(reason: Bypass) -> &'static str {
    match reason {
        Bypass::CacheOff => "cache-off",
        Bypass::NoCache => "no-cache",
        Bypass::AlwaysCompile => "always",
        Bypass::Blocked => "blocked",
        Bypass::NonCacheableInput => "non-cacheable-input",
    }
}

/// An analyzed namespace the cache keyed and will publish once its artifact
/// is complete ([`CompileCache::analyzed`]).
pub(crate) struct Publication {
    name: String,
    id: NamespaceId,
    key: FrontEndKey,
}

/// The output store of a compilation that has ended
/// ([`CompileCache::into_output`], light-cljs-418). A build whose every
/// namespace has an artifact keys its output by them and by its emission
/// options ([`crate::cache_key::output_key`]). When a stored output has
/// that key, the build writes it instead of preparing and emitting the
/// application, so a rebuild with no change costs its lookups and installs
/// alone; otherwise it stores what it emits.
pub struct OutputCache {
    store: Store,
    environment: KeyEnvironment,
    program: Option<Digest>,
    retention: Retention,
    counts: OutputCounts,
}

impl OutputCache {
    /// The key of this build's output with `options`, or `None` when some
    /// namespace has no artifact, so its output is neither read nor stored.
    pub fn key(&self, options: &crate::cache_key::EmissionOptions<'_>) -> Option<Digest> {
        self.program
            .as_ref()
            .map(|program| crate::cache_key::output_key(&self.environment, program, options))
    }

    /// The files stored as the output of `key`, each a name and its text.
    pub fn load(&mut self, key: &Digest) -> Option<Vec<(String, String)>> {
        let started = Instant::now();
        let files = self.store.read_output(key, MAX_OUTPUT_BYTES);
        self.counts.time += started.elapsed();
        if let Some(files) = &files {
            self.counts.reused = true;
            self.counts.bytes_read = files.iter().map(|(_, text)| text.len() as u64).sum();
        }
        files
    }

    /// Store `files`, each a name and its text, as the output of `key`.
    pub fn save(&mut self, key: &Digest, files: &[(&str, &str)]) {
        let started = Instant::now();
        if let Some(bytes) = self
            .store
            .write_output(key, files, &self.retention, MAX_OUTPUT_BYTES)
        {
            self.counts.stored = true;
            self.counts.bytes_written = bytes;
        }
        self.counts.time += started.elapsed();
    }

    /// What this build did with outputs, for `--stats`.
    pub fn counts(&self) -> OutputCounts {
        self.counts.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated_names::GeneratedNames;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn publish_pipeline_io_barrier_commits_earlier_staged_items_without_deadlock() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let dir = std::env::temp_dir().join(format!(
            "light-cljs-pipeline-io-barrier-{}-{nanos}",
            std::process::id()
        ));
        let store = Store::open(&dir);
        let mut pipeline = PublishPipeline::start(
            store.clone(),
            Slots::new(4).unwrap(),
            None,
            #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
            None,
        )
        .unwrap();

        // Stage two valid artifacts directly and inject them out-of-order
        // alongside an Io error at seq 2 and a third staged artifact at seq 3
        // to verify the committer commits seq 0..=1 before handling seq 2's
        // Io error, never deadlocks, and returns outcomes in deterministic seq
        // order.
        let staged0 = store
            .stage_encoded(
                "app.a".into(),
                [1u8; 32],
                GeneratedNames::default(),
                b"artifact-0".to_vec(),
            )
            .unwrap();
        let staged1 = store
            .stage_encoded(
                "app.b".into(),
                [2u8; 32],
                GeneratedNames::default(),
                b"artifact-1".to_vec(),
            )
            .unwrap();
        let staged3 = store
            .stage_encoded(
                "app.d".into(),
                [4u8; 32],
                GeneratedNames::default(),
                b"artifact-3".to_vec(),
            )
            .unwrap();

        pipeline.submitted = 7;
        {
            let mut commit = pipeline.shared.commit_mu.lock().unwrap();
            let b0 = staged0.approximate_bytes();
            let b1 = staged1.approximate_bytes();
            let b3 = staged3.approximate_bytes();
            commit.staged_count = 3;
            commit.staged_bytes = b0 + b1 + b3;
            // Capture failure after store failure keeps its capture reason;
            // refusal and unencodable after store failure report unwritable.
            commit.pending.insert(
                6,
                (
                    NamespaceId(6),
                    "app.g".into(),
                    StageOutcome::CaptureFailed("capture declined".into()),
                ),
            );
            commit.pending.insert(
                5,
                (
                    NamespaceId(5),
                    "app.f".into(),
                    StageOutcome::Unencodable("too deep".into()),
                ),
            );
            commit.pending.insert(
                4,
                (
                    NamespaceId(4),
                    "app.e".into(),
                    StageOutcome::Refused(Mismatch::Resolution),
                ),
            );
            commit.pending.insert(
                3,
                (
                    NamespaceId(3),
                    "app.d".into(),
                    StageOutcome::Staged(staged3, b3),
                ),
            );
            commit.pending.insert(
                2,
                (
                    NamespaceId(2),
                    "app.c".into(),
                    StageOutcome::Io(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "simulated mid-batch io error",
                    )),
                ),
            );
            commit.pending.insert(
                1,
                (
                    NamespaceId(1),
                    "app.b".into(),
                    StageOutcome::Staged(staged1, b1),
                ),
            );
            commit.pending.insert(
                0,
                (
                    NamespaceId(0),
                    "app.a".into(),
                    StageOutcome::Staged(staged0, b0),
                ),
            );
            pipeline.shared.commit_cv.notify_one();
        }

        let finished = pipeline.flush();
        assert_eq!(finished.len(), 7);
        assert_eq!(
            finished
                .iter()
                .map(|f| (f.seq, f.name.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (0, "app.a"),
                (1, "app.b"),
                (2, "app.c"),
                (3, "app.d"),
                (4, "app.e"),
                (5, "app.f"),
                (6, "app.g"),
            ]
        );
        assert!(matches!(
            finished[0].result,
            FinishedResult::Published { .. }
        ));
        assert!(matches!(
            finished[1].result,
            FinishedResult::Published { .. }
        ));
        assert!(matches!(finished[2].result, FinishedResult::Unwritable));
        assert!(matches!(finished[3].result, FinishedResult::Unwritable));
        assert!(matches!(finished[4].result, FinishedResult::Unwritable));
        assert!(matches!(finished[5].result, FinishedResult::Unwritable));
        assert!(matches!(
            &finished[6].result,
            FinishedResult::Unpublished(reason) if reason == "capture declined"
        ));
        assert!(store.write_failure().is_some());
        assert_eq!(store.candidates("app.a").len(), 1);
        assert_eq!(store.candidates("app.b").len(), 1);
        assert!(store.candidates("app.d").is_empty());

        // Zero leaked temporary files in objects/ or index/.
        for sub in ["objects", "index"] {
            let temps: Vec<_> = std::fs::read_dir(store.root().join(sub))
                .unwrap()
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with(".tmp-"))
                .collect();
            assert!(temps.is_empty(), "leaked temporaries in {sub}: {temps:?}");
        }

        drop(pipeline);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn publish_pipeline_backpressures_a_blocked_committer() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::Duration;

        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let dir = std::env::temp_dir().join(format!(
            "light-cljs-pipeline-backpressure-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(dir.join("v1/objects")).unwrap();
        std::fs::create_dir_all(dir.join("v1/index")).unwrap();
        let lock_file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join("v1/lock"))
            .unwrap();
        // Hold the advisory lock so the committer blocks on its first commit.
        lock_file.lock().unwrap();

        let store = Store::open(&dir);
        let mut pipeline = PublishPipeline::start_with_limits(
            store.clone(),
            None,
            8,
            1 << 20,
            3,
            1 << 20,
            #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
            None,
        )
        .unwrap();
        let submitted = Arc::new(AtomicUsize::new(0));
        let submitted_producer = Arc::clone(&submitted);
        let producer = thread::spawn(move || {
            for i in 0..8usize {
                let mut artifact = minimal_artifact(&format!("app.bp{i}"));
                artifact.header.source_length = i as u32;
                pipeline.enqueue(
                    NamespaceId(i as u32),
                    format!("app.bp{i}"),
                    1024,
                    artifact,
                    ManifestFields::default(),
                    None,
                );
                submitted_producer.fetch_add(1, Ordering::SeqCst);
            }
            pipeline.flush()
        });

        let deadline = Instant::now() + Duration::from_secs(2);
        while submitted.load(Ordering::SeqCst) < 3 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        thread::sleep(Duration::from_millis(80));
        // While v1/lock is held, the producer must plateau at max_pending_commit_jobs = 3
        // rather than admitting all 8 jobs unboundedly.
        assert_eq!(submitted.load(Ordering::SeqCst), 3);

        drop(lock_file);
        let finished = producer.join().unwrap();
        assert_eq!(finished.len(), 8);
        assert!(
            finished
                .iter()
                .all(|f| matches!(f.result, FinishedResult::Published { .. }))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn publish_pipeline_accounts_for_deferred_bodies_and_oversized_jobs() {
        use crate::namespace_artifact::Bodies;

        let base = minimal_artifact("app.small");
        let small_est = publish_estimated_bytes(&base, &ManifestFields::default(), 0);
        let mut with_bodies = minimal_artifact("app.big");
        let mut bodies = Bodies::default();
        // 10 MiB of UTF-16 code units inside deferred bodies (not counted by
        // `NamespaceArtifact::approximate_bytes`).
        bodies.ir.strings.push(vec![b'a' as u16; 10 * 1024 * 1024]);
        with_bodies.bodies = Some(Box::new(bodies));
        assert_eq!(
            base.approximate_bytes(),
            with_bodies.approximate_bytes(),
            "approximate_bytes omits deferred bodies"
        );
        let big_est = publish_estimated_bytes(&with_bodies, &ManifestFields::default(), 0);
        assert!(
            big_est > PUBLISH_MAX_INFLIGHT_BYTES,
            "deferred bodies push estimate ({big_est}) above 32 MiB cap (small={small_est})"
        );
    }

    #[test]
    fn inline_publish_io_barrier_commits_earlier_staged_items() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let dir = std::env::temp_dir().join(format!(
            "light-cljs-inline-barrier-{}-{nanos}",
            std::process::id()
        ));
        let store = Store::open(&dir);
        let host = MacroHost::new();
        let options = CacheOptions {
            dir: dir.clone(),
            ..CacheOptions::default()
        };
        let mut cache = CompileCache::open(&options, &host, Default::default(), "app.main");
        cache.artifacts = vec![None, None];
        let staged0 = store
            .stage_encoded(
                "app.a".into(),
                [1u8; 32],
                GeneratedNames::default(),
                b"inline-0".to_vec(),
            )
            .unwrap();
        cache
            .inline_batch
            .push((NamespaceId(0), "app.a".into(), staged0));
        // Simulate the inline StageAttempt::Io path: commit earlier staged
        // items before latching Store::fail.
        cache.commit_inline_batch();
        cache.store.fail(&std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "simulated inline io error",
        ));
        assert_eq!(cache.counts.published, 1);
        assert!(cache.artifacts[0].is_some());
        assert_eq!(store.candidates("app.a").len(), 1);
        assert!(cache.store.write_failure().is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn compile_cache_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<CompileCache>();
    }

    #[test]
    fn publish_pipeline_drains_staging_while_pool_worker_is_blocked() {
        use crate::compilation_session::{CompilationSession, PoolEngines};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::time::Duration;

        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let host = MacroHost::new();
        let session = CompilationSession::new();
        session
            .start_pool(
                Slots::new(2).unwrap(),
                PoolEngines::None,
                1,
                host.environment(),
                None,
                Default::default(),
            )
            .unwrap();
        let pool = session.pool_task_submitter().unwrap();

        // Occupy the sole `--jobs 2` pool worker with a gated job so every
        // queued staging and commit-CPU task must be drained inline by the
        // coordinator without deadlocking.
        let gate = Arc::new((Mutex::new((false, false)), Condvar::new()));
        let worker_gate = Arc::clone(&gate);
        assert!(pool.submit_plain(
            1,
            Box::new(move || {
                let (mu, cv) = &*worker_gate;
                let mut st = mu.lock().unwrap();
                st.0 = true;
                cv.notify_all();
                while !st.1 {
                    st = cv.wait(st).unwrap();
                }
            }),
        ));
        {
            let (mu, cv) = &*gate;
            let mut st = mu.lock().unwrap();
            let deadline = Instant::now() + Duration::from_secs(2);
            while !st.0 && Instant::now() < deadline {
                let (next, _) = cv.wait_timeout(st, Duration::from_millis(20)).unwrap();
                st = next;
            }
            assert!(st.0, "sole pool worker did not reach gate");
        }

        let caller = thread::current().id();
        let ran_on_coordinator = Arc::new(AtomicBool::new(false));
        let hook_flag = Arc::clone(&ran_on_coordinator);
        let hook: Arc<dyn Fn(CacheCpuKind) + Send + Sync> = Arc::new(move |kind| {
            if kind == CacheCpuKind::ArtifactEncodeHash && thread::current().id() == caller {
                hook_flag.store(true, Ordering::SeqCst);
            }
        });

        // Case 1: staging backpressure (max_inflight_jobs = 2) drains the
        // oldest queued job inline on the coordinator and finishes in order
        // while the sole pool worker remains blocked.
        let dir_stage = std::env::temp_dir().join(format!(
            "light-cljs-stage-pressure-{}-{nanos}",
            std::process::id()
        ));
        let store_stage = Store::open(&dir_stage);
        let mut stage_pipe = PublishPipeline::start_with_limits(
            store_stage.clone(),
            Some(pool.clone()),
            2,
            1 << 20,
            64,
            1 << 20,
            Some(Arc::clone(&hook)),
        )
        .unwrap();
        for i in 0..5usize {
            let mut artifact = minimal_artifact(&format!("app.sp{i}"));
            artifact.header.source_length = i as u32;
            stage_pipe.enqueue(
                NamespaceId(i as u32),
                format!("app.sp{i}"),
                1024,
                artifact,
                ManifestFields::default(),
                None,
            );
        }
        assert!(
            ran_on_coordinator.load(Ordering::SeqCst),
            "expected coordinator to drain oldest queued stage job under staging pressure"
        );
        let finished_stage = stage_pipe.flush();
        assert_eq!(finished_stage.len(), 5);
        assert_eq!(
            finished_stage
                .iter()
                .map(|f| (f.seq, f.name.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (0, "app.sp0"),
                (1, "app.sp1"),
                (2, "app.sp2"),
                (3, "app.sp3"),
                (4, "app.sp4"),
            ]
        );
        assert!(
            finished_stage
                .iter()
                .all(|f| matches!(f.result, FinishedResult::Published { .. }))
        );
        drop(stage_pipe);
        let _ = std::fs::remove_dir_all(&dir_stage);

        // Case 2: commit backpressure (max_pending_commit_jobs = 2, plenty of
        // staging slots) drains queued stage jobs and commit-side CPU work
        // inline on the coordinator while the sole pool worker remains blocked.
        ran_on_coordinator.store(false, Ordering::SeqCst);
        let dir_commit = std::env::temp_dir().join(format!(
            "light-cljs-commit-pressure-{}-{nanos}",
            std::process::id()
        ));
        let store_commit = Store::open(&dir_commit);
        let mut commit_pipe = PublishPipeline::start_with_limits(
            store_commit.clone(),
            Some(pool),
            16,
            1 << 20,
            2,
            1 << 20,
            Some(hook),
        )
        .unwrap();
        for i in 0..5usize {
            let mut artifact = minimal_artifact(&format!("app.cp{i}"));
            artifact.header.source_length = i as u32;
            commit_pipe.enqueue(
                NamespaceId(i as u32),
                format!("app.cp{i}"),
                1024,
                artifact,
                ManifestFields::default(),
                None,
            );
        }
        assert!(
            ran_on_coordinator.load(Ordering::SeqCst),
            "expected coordinator to drain queued stage job under commit pressure"
        );
        let finished_commit = commit_pipe.flush();
        assert_eq!(finished_commit.len(), 5);
        assert_eq!(
            finished_commit
                .iter()
                .map(|f| (f.seq, f.name.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (0, "app.cp0"),
                (1, "app.cp1"),
                (2, "app.cp2"),
                (3, "app.cp3"),
                (4, "app.cp4"),
            ]
        );
        assert!(
            finished_commit
                .iter()
                .all(|f| matches!(f.result, FinishedResult::Published { .. }))
        );
        drop(commit_pipe);
        let _ = std::fs::remove_dir_all(&dir_commit);

        // Release the blocked pool worker only after both pipelines finished.
        {
            let (mu, cv) = &*gate;
            let mut st = mu.lock().unwrap();
            st.1 = true;
            cv.notify_all();
        }
        session.finish_pool();
    }

    fn minimal_artifact(namespace: &str) -> NamespaceArtifact {
        use crate::namespace_artifact::{
            Declarations, DeferredUnits, Diagnostics, Expr, ExprContext, ExprKind, ExprRef, Header,
            Live, References, SemanticIr, SourceRef,
        };
        NamespaceArtifact {
            header: Header {
                namespace: namespace.to_owned(),
                path: format!("{namespace}.cljs"),
                source_length: 0,
                cljc: false,
                source_name: None,
                entry: false,
                root: ExprRef(0),
                policy: Default::default(),
            },
            declarations: Declarations::default(),
            ir: SemanticIr {
                exprs: vec![Expr {
                    kind: ExprKind::Nil,
                    span: (0, 0),
                    context: ExprContext::Value,
                    source: SourceRef::Own,
                }],
                ..SemanticIr::default()
            },
            patches: Vec::new(),
            units: DeferredUnits::default(),
            diagnostics: Diagnostics::default(),
            references: References::default(),
            names: GeneratedNames::default(),
            accounting: None,
            registry: Vec::new(),
            bodies: None,
            live: Live::default(),
        }
    }
}
