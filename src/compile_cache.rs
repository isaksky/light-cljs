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
    Inputs, MAX_OUTPUT_BYTES, Publish, Retention, Snapshot, StageAttempt, StagedArtifact, Store,
};
use crate::cache_key::{
    Digest, FrontEndKey, KeyEnvironment, Mismatch, NamespaceKeys, RevalidateInputs,
};
use crate::cache_policy::{Bypass, NamespaceMetadata};
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
    /// CPU slots available to this compilation, detected from the session's
    /// unit pool policy when `--jobs > 1`.
    slots: Slots,
    /// Active compilation session whose unit pool supplies the CPU slots for
    /// cache staging (revalidation, encoding, hashing) when `--jobs > 1`.
    session: Option<std::rc::Weak<crate::compilation_session::CompilationSession>>,
    /// Staged artifacts awaiting a batched commit on the inline (`Slots::ONE`)
    /// path, flushed every `PUBLISH_BATCH_SIZE` namespaces and on flush/finish.
    inline_batch: Vec<(NamespaceId, String, StagedArtifact)>,
    /// Background publish pipeline when `slots > 1`, started lazily on the
    /// first publish so warm no-op builds spawn no threads.
    pipeline: Option<PublishPipeline>,
    /// True if spawning the background commit thread failed, so later publishes
    /// stay inline instead of retrying thread creation.
    pipeline_failed: bool,
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
        let started = Instant::now();
        let store = Store::open(&options.dir);
        let snapshot = store.snapshot_all();
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
            slots: Slots::ONE,
            session: None,
            inline_batch: Vec::new(),
            pipeline: None,
            pipeline_failed: false,
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
        let decoded = artifact_codec::decode(&bytes, &self.limits, cancelled);
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
            if self.session.is_none() {
                self.session = Some(std::rc::Rc::downgrade(&session));
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
            match PublishPipeline::start(self.store.clone(), self.slots) {
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
                let session = self.session.as_ref().and_then(std::rc::Weak::upgrade);
                if let Some(pipeline) = &mut self.pipeline {
                    pipeline.enqueue(
                        session.as_deref(),
                        id,
                        name,
                        estimated_bytes,
                        artifact,
                        fields,
                        revalidate,
                    );
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
    Staged(StagedArtifact, usize),
    Refused(Mismatch),
    Unencodable(String),
    CaptureFailed(String),
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
    finished: Vec<FinishedPublish>,
    completed: usize,
    flush_requested: bool,
    shutdown: bool,
    cancelled: bool,
    panic: Option<String>,
}

struct PublishShared {
    store: Store,
    max_inflight_jobs: usize,
    max_inflight_bytes: usize,
    max_pending_commit_jobs: usize,
    max_pending_commit_bytes: usize,
    stage_mu: Mutex<StageQueue>,
    stage_space_cv: Condvar,
    commit_mu: Mutex<CommitQueue>,
    commit_cv: Condvar,
    done_cv: Condvar,
}

impl PublishShared {
    fn record_panic(&self, payload: Box<dyn std::any::Any + Send>) {
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "panic".to_owned());
        {
            let mut stage = self.stage_mu.lock().unwrap_or_else(|e| e.into_inner());
            stage.cancelled = true;
            stage.jobs.clear();
            self.stage_space_cv.notify_all();
        }
        {
            let mut commit = self.commit_mu.lock().unwrap_or_else(|e| e.into_inner());
            commit.cancelled = true;
            commit.pending.clear();
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
    fn start(store: Store, slots: Slots) -> std::io::Result<Self> {
        Self::start_with_limits(
            store,
            slots,
            PUBLISH_MAX_INFLIGHT_BYTES,
            PUBLISH_MAX_PENDING_COMMIT_JOBS,
            PUBLISH_MAX_PENDING_COMMIT_BYTES,
        )
    }

    fn start_with_limits(
        store: Store,
        slots: Slots,
        max_inflight_bytes: usize,
        max_pending_commit_jobs: usize,
        max_pending_commit_bytes: usize,
    ) -> std::io::Result<Self> {
        let max_inflight_jobs = (slots.get() * 2).min(PUBLISH_MAX_INFLIGHT_JOBS);
        let shared = Arc::new(PublishShared {
            store,
            max_inflight_jobs,
            max_inflight_bytes,
            max_pending_commit_jobs: max_pending_commit_jobs.max(1),
            max_pending_commit_bytes: max_pending_commit_bytes.max(1),
            stage_mu: Mutex::new(StageQueue {
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
                finished: Vec::new(),
                completed: 0,
                flush_requested: false,
                shutdown: false,
                cancelled: false,
                panic: None,
            }),
            commit_cv: Condvar::new(),
            done_cv: Condvar::new(),
        });

        // CPU-bound revalidation, encoding and artifact hashing run on the
        // build's existing `UnitPool` worker slots (or inline on the
        // coordinator under backpressure / flush), keeping CPU work within
        // `--jobs N`. Only the I/O committer runs on its own thread.
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

    #[allow(clippy::too_many_arguments)]
    fn enqueue(
        &mut self,
        session: Option<&crate::compilation_session::CompilationSession>,
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
                {
                    break;
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
            // Run the oldest unstarted job inline on the coordinator's CPU
            // slot so staging backpressure never deadlocks against a pool
            // worker waiting on the coordinator.
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
        let dispatched = session.is_some_and(|session| {
            let shared = Arc::clone(&self.shared);
            session.submit_namespace(
                STAGE_JOB_INDEX_TAG | seq,
                false,
                Box::new(move |_| {
                    pump_one_stage_job(&shared);
                    false
                }),
            )
        });
        if !dispatched {
            pump_one_stage_job(&self.shared);
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

    fn flush(&mut self) -> Vec<FinishedPublish> {
        // Drain any unstarted jobs inline so flushing never stalls if the
        // unit pool has already finished, was cancelled, or has a worker
        // waiting on the coordinator.
        while pump_one_stage_job(&self.shared) {}
        let target = self.submitted;
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
                commit = self
                    .shared
                    .done_cv
                    .wait(commit)
                    .unwrap_or_else(|e| e.into_inner());
            }
        }
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
            while pump_one_stage_job(&self.shared) {}
        }
        {
            let mut stage = self
                .shared
                .stage_mu
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if cancelling {
                stage.cancelled = true;
                stage.jobs.clear();
            }
            self.shared.stage_space_cv.notify_all();
        }
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
        run_stage(&shared.store, &name, artifact, fields, revalidate)
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
    if let StageOutcome::Staged(_, staged_bytes) = &outcome {
        commit.staged_count += 1;
        commit.staged_bytes = commit.staged_bytes.saturating_add(*staged_bytes);
    }
    let pressure = commit.staged_count >= shared.max_pending_commit_jobs
        || commit.staged_bytes >= shared.max_pending_commit_bytes;
    commit.pending.insert(seq, (id, name, outcome));
    if seq == commit.next_seq || pressure {
        shared.commit_cv.notify_one();
    }
}

fn run_stage(
    store: &Store,
    name: &str,
    mut artifact: NamespaceArtifact,
    fields: ManifestFields,
    revalidate: Option<(
        RevalidateInputs,
        String,
        Arc<dyn SourceProvider + Send + Sync>,
    )>,
) -> StageOutcome {
    if store.write_failure().is_some() {
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
    let encoded = artifact_codec::encode(&artifact, &fields);
    let names = std::mem::take(&mut artifact.names);
    drop(artifact);
    let key = fields.front_end_key.digest;
    drop(fields);
    let bytes = match encoded {
        Ok(bytes) => bytes,
        Err(reason) => return StageOutcome::Unencodable(reason),
    };
    match store.stage_encoded(name.to_owned(), key, names, bytes) {
        Ok(staged) => {
            let staged_bytes = staged.approximate_bytes();
            StageOutcome::Staged(staged, staged_bytes)
        }
        Err(error) => StageOutcome::Io(error),
    }
}

fn commit_worker_loop(shared: Arc<PublishShared>) {
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        let mut batch: Vec<(usize, NamespaceId, String, StagedArtifact, usize)> =
            Vec::with_capacity(PUBLISH_BATCH_SIZE);
        loop {
            let to_commit = {
                let mut commit = shared.commit_mu.lock().unwrap_or_else(|e| e.into_inner());
                loop {
                    if commit.cancelled {
                        drop(batch);
                        commit.pending.clear();
                        commit.staged_count = 0;
                        commit.staged_bytes = 0;
                        shared.done_cv.notify_all();
                        return;
                    }
                    // Drain contiguous completed staging outcomes in submission order.
                    let mut barrier = false;
                    let mut completed_any = false;
                    while let Some(entry) = commit.pending.get(&commit.next_seq) {
                        if !batch.is_empty()
                            && matches!(
                                entry.2,
                                StageOutcome::Io(_)
                                    | StageOutcome::Refused(_)
                                    | StageOutcome::Unencodable(_)
                            )
                        {
                            // Commit earlier valid staged items before deciding
                            // store-failure precedence for this outcome.
                            barrier = true;
                            break;
                        }
                        let seq = commit.next_seq;
                        let (id, name, outcome) = commit.pending.remove(&seq).unwrap();
                        commit.next_seq += 1;
                        match outcome {
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

                    let flush_ready = (commit.flush_requested || commit.shutdown)
                        && commit.next_seq == commit.submitted;
                    let pressure_commit = commit.staged_count >= shared.max_pending_commit_jobs
                        || commit.staged_bytes >= shared.max_pending_commit_bytes;
                    if batch.len() >= PUBLISH_BATCH_SIZE
                        || (!batch.is_empty() && (flush_ready || barrier || pressure_commit))
                    {
                        break std::mem::take(&mut batch);
                    }
                    if completed_any {
                        shared.done_cv.notify_all();
                    }
                    if commit.flush_requested && commit.completed == commit.submitted {
                        commit.flush_requested = false;
                        shared.done_cv.notify_all();
                    }
                    if commit.shutdown && commit.next_seq == commit.submitted {
                        shared.done_cv.notify_all();
                        return;
                    }
                    commit = shared
                        .commit_cv
                        .wait(commit)
                        .unwrap_or_else(|e| e.into_inner());
                }
            };

            let mut meta = Vec::with_capacity(to_commit.len());
            let mut artifacts = Vec::with_capacity(to_commit.len());
            let mut batch_staged_bytes = 0usize;
            for (seq, id, name, staged, staged_bytes) in to_commit {
                meta.push((seq, id, name));
                artifacts.push(staged);
                batch_staged_bytes = batch_staged_bytes.saturating_add(staged_bytes);
            }
            let batch_len = artifacts.len();
            let results = shared.store.commit_batch(artifacts);

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
            if commit.flush_requested && commit.completed == commit.submitted && batch.is_empty() {
                commit.flush_requested = false;
            }
            shared.done_cv.notify_all();
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
        let mut pipeline = PublishPipeline::start(store.clone(), Slots::new(4).unwrap()).unwrap();

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
            Slots::new(4).unwrap(),
            1 << 20,
            3,
            1 << 20,
        )
        .unwrap();
        let submitted = Arc::new(AtomicUsize::new(0));
        let submitted_producer = Arc::clone(&submitted);
        let producer = thread::spawn(move || {
            for i in 0..8usize {
                let mut artifact = minimal_artifact(&format!("app.bp{i}"));
                artifact.header.source_length = i as u32;
                pipeline.enqueue(
                    None,
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
