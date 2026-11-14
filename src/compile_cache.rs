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
use crate::artifact_store::{Inputs, MAX_OUTPUT_BYTES, Publish, Retention, Snapshot, Store};
use crate::cache_key::{Digest, FrontEndKey, KeyEnvironment, Mismatch, NamespaceKeys};
use crate::cache_policy::{Bypass, NamespaceMetadata};
use crate::compile_stats::{CacheCounts, OutputCounts};
use crate::dependency_manifest::{BroadView, EdgeKinds, ExternalInput, LoaderRecord, NamespaceId};
use crate::macro_host::MacroHost;
use crate::manifest_prefix::{ManifestPrefix, NamespaceRecords};
use crate::namespace_artifact::NamespaceArtifact;
use crate::source_provider::SourceProvider;
use std::path::PathBuf;
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
    pub fn finish(&self) {
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
    pub fn into_output(self) -> OutputCache {
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
            store: self.store,
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
        let Publication { name, id, key } = publication;
        let artifact = match artifact {
            Ok(artifact) => artifact,
            Err(reason) => {
                self.counts.unpublished.push((name, reason));
                return;
            }
        };
        let record = self.prefix.record(id);
        let fields = ManifestFields {
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

/// What one analyzed namespace gives [`declaration_view`].
pub(crate) struct DeclarationSource<'a> {
    pub namespace: &'a str,
    pub source_index: usize,
    /// The name its spans and registration locations give its source.
    pub source_name: &'a str,
    pub text: &'a str,
    /// Its top-level forms' byte ranges, in source order.
    pub forms: Vec<std::ops::Range<usize>>,
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
/// fragments, since they reach the program only at its drain point; when a
/// fragment cannot be read, or there is no session, the view is `None`,
/// unknown, and later views cover its whole key.
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
    let (units, end, _) = session.capture_marks();
    let registrations = session.captured_registrations(start.registrations()..end);
    // The globals its units' bodies add (a `def` in a function) reach the
    // program, and a later macro's reader tables, only when they merge,
    // after this namespace is keyed: read them from the settled fragments.
    let bundle_units: Vec<usize> = registrations
        .iter()
        .filter_map(|registration| match registration {
            RegistrationRecord::Bundle(bundle) => Some(bundle.unit),
            RegistrationRecord::Root { .. } => None,
        })
        .collect();
    let deferred = session.unit_global_names(start.units()..units, namespace, &bundle_units)?;
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
                .iter()
                .skip(start.globals())
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
                .iter()
                .skip(start.globals())
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
