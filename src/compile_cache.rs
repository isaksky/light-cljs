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
use crate::artifact_store::{Inputs, Publish, Retention, Snapshot, Store};
use crate::cache_key::{Digest, FrontEndKey, KeyEnvironment, Mismatch, NamespaceKeys};
use crate::cache_policy::{Bypass, NamespaceMetadata};
use crate::compile_stats::CacheCounts;
use crate::dependency_manifest::{BroadView, EdgeKinds, ExternalInput, LoaderRecord, NamespaceId};
use crate::macro_host::MacroHost;
use crate::manifest_prefix::{ManifestPrefix, NamespaceRecords};
use crate::namespace_artifact::NamespaceArtifact;
use crate::source_provider::SourceProvider;
use std::path::PathBuf;
use std::time::Instant;

/// What a build asks of the persistent cache.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CacheOptions {
    /// `--cache-dir DIR`.
    pub dir: PathBuf,
    /// `--cache-blocker NS`: these namespaces and every compilation consumer
    /// of them neither read nor write the cache.
    pub blockers: Vec<String>,
}

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
    limits: DecodeLimits,
    counts: CacheCounts,
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
            limits: DecodeLimits::default(),
            counts,
        }
    }

    /// The generated names of every candidate this compilation may install.
    pub fn reservation(&self) -> &crate::generated_names::Reservation {
        self.snapshot.reservation()
    }

    /// What the cache did so far.
    pub fn counts(&self) -> &CacheCounts {
        &self.counts
    }

    pub fn take_counts(&mut self) -> CacheCounts {
        std::mem::take(&mut self.counts)
    }

    /// Bound the cache directory after the build succeeded, keeping what it
    /// used and published.
    pub fn finish(&self) {
        if self.store.write_failure().is_some() {
            return;
        }
        if let Err(error) = self.store.trim(&Retention::default(), &self.in_use) {
            eprintln!(
                "warning: cannot trim the compiler cache in {}: {error}",
                self.store.root().display()
            );
        }
    }

    fn miss(&mut self, kind: &'static str, description: String) {
        if !matches!(self.pending, Some(Pending::Miss(..))) {
            self.pending = Some(Pending::Miss(kind, description));
        }
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
            let bytes = match self.store.read(candidate, &self.limits) {
                Ok(bytes) => bytes,
                Err(error) => {
                    self.miss("unreadable", error.to_string());
                    continue;
                }
            };
            self.counts.bytes_read += bytes.len() as u64;
            let manifest = match artifact_codec::read_manifest(&bytes, &self.limits) {
                Ok(manifest) if manifest.namespace == name => manifest,
                Ok(manifest) => {
                    self.miss(
                        "undecodable",
                        format!("the artifact is {}'s", manifest.namespace),
                    );
                    continue;
                }
                Err(error) => {
                    self.miss("undecodable", error.to_string());
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
                    Err(fields
                        .front_end_key
                        .mismatch(&key)
                        .unwrap_or(Mismatch::Inconsistent))
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

    /// The artifact [`Self::lookup`] returned was installed.
    pub(crate) fn installed(&mut self, macros: &MacroHost) {
        let Some(Pending::Hit { key, artifact }) = self.pending.take() else {
            unreachable!("only a matched candidate is installed");
        };
        let id = NamespaceId(self.prefix.len() as u32 - 1);
        let name = self.prefix.record(id).name.clone();
        self.prefix.set_macros(id, declares_macros(macros, &name));
        if let Some(log) = macros.dependency_log.borrow().as_ref() {
            self.prefix.skip_observations(log);
        }
        self.keys.append(&name, *key);
        self.propagated.push(None);
        self.in_use.push(artifact);
        self.counts.hits += 1;
    }

    /// The artifact [`Self::lookup`] returned was refused; its namespace is
    /// analyzed instead.
    pub(crate) fn refused(&mut self, reason: &str) {
        self.prefix.pop();
        self.pending = None;
        self.miss("refused", reason.to_owned());
    }

    /// The namespace the last lookup did not install was analyzed, and
    /// captured as `artifact`: key it, decide its policy and publish it.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn analyzed(
        &mut self,
        loader: &LoaderRecord,
        text: &str,
        sources: &dyn SourceProvider,
        macros: &MacroHost,
        program_namespaces: &crate::arena::Arena<String>,
        artifact: Option<Result<NamespaceArtifact, String>>,
    ) {
        let index = self.prefix.len();
        let name = loader.sources[index].name.clone();
        let id = {
            let log = macros.dependency_log.borrow();
            let log = log.as_ref().expect("the cache records observations");
            self.prefix.push_analyzed(
                loader,
                log,
                program_namespaces,
                declares_macros(macros, &name),
            )
        };
        let key = self
            .keys
            .push(&self.environment, &self.prefix, id, text, &self.entry)
            .clone();
        let metadata = macros
            .namespace_symbol(&name)
            .map(NamespaceMetadata::of_symbol)
            .unwrap_or_default();
        let propagated = self
            .origin(id, metadata.always, true)
            .map(|reason| (reason, id.0))
            .or_else(|| self.reaching(id));
        self.propagated.push(propagated);
        let pending = self.pending.take();
        if metadata.no_cache {
            self.bypass(&name, Bypass::NoCache, String::new());
            return;
        }
        if let Some((reason, _)) = propagated {
            let path = self.path(id);
            self.bypass(&name, reason, path);
            return;
        }
        let (kind, description) = match pending {
            Some(Pending::Miss(kind, description)) => (kind, description),
            _ => ("no-artifact", "no artifact was published".to_owned()),
        };
        self.counts.misses.push((name.clone(), kind, description));
        let artifact = match artifact {
            Some(Ok(artifact)) => artifact,
            Some(Err(reason)) => {
                self.counts.unpublished.push((name, reason));
                return;
            }
            None => return,
        };
        let record = self.prefix.record(id);
        let fields = ManifestFields {
            front_end_key: key,
            emission_key: [0; 32],
            dependencies: self
                .prefix
                .row(id)
                .iter()
                .map(|&(target, kinds)| artifact_codec::Dependency {
                    namespace: self.prefix.record(target).name.clone(),
                    kinds: kinds.0,
                })
                .collect(),
            inputs: self.prefix.external_inputs(id).cloned().collect(),
            coarse: record.coarse,
            absent: record.absent.clone(),
            no_cache: false,
            always: false,
        };
        let started = Instant::now();
        let published = self.store.publish(
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
        self.counts.write_time += started.elapsed();
        match published {
            Publish::Published(entry) => {
                self.counts.published += 1;
                self.counts.bytes_written += entry.bytes;
                self.in_use.push(entry.artifact);
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

/// Whether `namespace` declares macros in `macros`' inventory, which makes a
/// require of it a macro-provider edge.
fn declares_macros(macros: &MacroHost, namespace: &str) -> bool {
    macros.javascript_macros.keys().any(|key| {
        key.split_once('/')
            .is_some_and(|(owner, _)| owner == namespace)
    })
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
