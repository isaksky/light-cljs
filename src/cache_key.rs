//! The cache keys of a namespace (docs/compiler-cache.md, "Conservative keys
//! and invalidation"): a front-end key, which gates reusing its artifact,
//! and a separate emission key for output built from it. Both are SHA-256
//! [`Fingerprint`]s over content, never timestamps.
//!
//! A [`FrontEndKey`] keeps the digest of each [`Component`] beside the key,
//! which is the digest of the components, so that comparing a stored key
//! with the current one names the first component that differs
//! ([`FrontEndKey::mismatch`]). The components are, in that order:
//!
//! - **compiler**: [`compiler_identity`], the hash `build.rs` computes of the
//!   crate's sources (the embedded runtime, bootstrap, core macros and
//!   bundled first-party libraries are among them), its vendored engines,
//!   `Cargo.lock`, features, profile, target and `rustc` version, with the
//!   artifact schema version, the core operation table and the macro engine
//!   that runs this compilation's macros (V8 with its version, or QuickJS).
//!   A key from one engine never matches a key from the other.
//! - **options**: the analysis and macro options of [`KeyEnvironment::new`].
//! - **config**: the fingerprint of the `--config` map
//!   ([`crate::BuildConfiguration::fingerprint`]), the host's
//!   ([`KeyEnvironment::new`]), which is also the `:config` broad view.
//! - **source**: the namespace's name, whether it is the entry, its resolved
//!   file (which diagnostics name), `.cljs` or `.cljc`, provider or bundled,
//!   and the SHA-256 of its text.
//! - **resolution**: the lookup of its own name, every candidate tried
//!   included, and each require as written with what it became and the
//!   lookups that decided it.
//! - **dependencies**: each namespace it has a compilation edge to, other
//!   than the coarse fallback's, with the edge's kinds and that namespace's
//!   own key. Since each of those keys covers its own dependencies, external
//!   inputs and source, this is a Merkle hash of the closure: an edit to a
//!   helper's implementation that keeps its signature changes every
//!   consumer's key.
//! - **inputs**: the `light.build` resources (each candidate, the identity,
//!   the SHA-256 and length of the text, or absence), declared namespaces,
//!   provider fingerprints, the configuration's fingerprint and the broad
//!   views it depends on.
//! - **view**: the incoming view the dependency manifest observed: for one
//!   with the `:declarations` broad view, the chain of every earlier
//!   namespace's name and key in source order ([`NamespaceKeys`]); for any
//!   other coarse namespace, the chain of every earlier namespace's
//!   declaration view ([`NamespaceKeys::append`], light-cljs-347); for one
//!   with the `:namespaces` view, the build's inventory, every namespace's
//!   name and source identity (file, `.cljs` or `.cljc`, provider or
//!   bundled), later ones too, which an edit to a namespace's text leaves
//!   alone; and for each name it looked for and did not find loaded, whether an
//!   earlier namespace has that name now. The `:config` view is the config
//!   component.
//!
//! [`revalidate`] re-reads a namespace's inputs after it compiles, so that a
//! store does not publish an artifact whose inputs changed meanwhile.

use crate::analyzer::AnalysisOptions;
use crate::application::{ApplicationOptions, OptimizationLevel};
use crate::dependency_manifest::{
    BroadView, DependencyManifest, EdgeKinds, ExternalInput, LookupId, NamespaceId,
    NamespaceRecord, RequireForm, RequireTarget, ResolvedResource, ResourceDependency, SourceKind,
    SourceOrigin,
};
use crate::macro_host::MacroHost;
use crate::manifest_prefix::NamespaceRecords;
use crate::sha256::{Fingerprint, digest};
use crate::source_provider::{LoadError, LoadRequest, Reload, SourceProvider};
use std::fmt;
use std::path::PathBuf;

pub type Digest = [u8; 32];

/// A part of a [`FrontEndKey`], in the order a comparison checks them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Component {
    Compiler,
    Options,
    Config,
    Source,
    Resolution,
    Dependencies,
    Inputs,
    View,
}

impl Component {
    pub const ALL: [Self; 8] = [
        Self::Compiler,
        Self::Options,
        Self::Config,
        Self::Source,
        Self::Resolution,
        Self::Dependencies,
        Self::Inputs,
        Self::View,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Compiler => "compiler",
            Self::Options => "options",
            Self::Config => "config",
            Self::Source => "source",
            Self::Resolution => "resolution",
            Self::Dependencies => "dependencies",
            Self::Inputs => "inputs",
            Self::View => "view",
        }
    }
}

/// A namespace's front-end key.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FrontEndKey {
    /// The key: the digest of the components, in [`Component::ALL`] order.
    pub digest: Digest,
    /// By [`Component::ALL`] position.
    pub components: [Digest; Component::ALL.len()],
    /// The edges the dependencies component covers, ascending in source
    /// order.
    pub dependencies: Vec<DependencyKey>,
}

/// A dependency of a [`FrontEndKey`]: the namespace, the
/// [`EdgeKinds`] bits of the edge and the namespace's key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DependencyKey {
    pub namespace: String,
    pub kinds: u8,
    pub key: Digest,
}

/// The first difference between a stored key and the current one, which
/// `--stats` reports as the miss reason.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Mismatch {
    Compiler,
    Options,
    Config,
    Source,
    Resolution,
    /// This dependency's key changed, or the namespace gained or lost a
    /// dependency on it.
    Dependency(String),
    Inputs,
    View,
    /// The stored key's digest does not follow from its components.
    Inconsistent,
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Compiler => f.write_str("compiler changed"),
            Self::Options => f.write_str("analysis options changed"),
            Self::Config => f.write_str("config changed"),
            Self::Source => f.write_str("source changed"),
            Self::Resolution => f.write_str("resolution changed"),
            Self::Dependency(name) => write!(f, "dependency {name} changed"),
            Self::Inputs => f.write_str("external input changed"),
            Self::View => f.write_str("earlier namespaces changed"),
            Self::Inconsistent => f.write_str("the stored key is inconsistent"),
        }
    }
}

impl FrontEndKey {
    fn new(components: [Digest; Component::ALL.len()], dependencies: Vec<DependencyKey>) -> Self {
        let mut fingerprint = Fingerprint::new("light-cljs front-end key");
        for component in &components {
            fingerprint.digest(component);
        }
        Self {
            digest: fingerprint.finish(),
            components,
            dependencies,
        }
    }

    pub fn component(&self, component: Component) -> &Digest {
        &self.components[component as usize]
    }

    /// The first component in which `current` differs from this stored
    /// key, or `None` when they match.
    pub fn mismatch(&self, current: &Self) -> Option<Mismatch> {
        if self.digest == current.digest {
            return None;
        }
        for component in Component::ALL {
            if self.component(component) == current.component(component) {
                continue;
            }
            return Some(match component {
                Component::Compiler => Mismatch::Compiler,
                Component::Options => Mismatch::Options,
                Component::Config => Mismatch::Config,
                Component::Source => Mismatch::Source,
                Component::Resolution => Mismatch::Resolution,
                Component::Dependencies => {
                    let (stored, now) = (&self.dependencies, &current.dependencies);
                    let at = stored
                        .iter()
                        .zip(now)
                        .position(|(stored, now)| stored != now)
                        .unwrap_or(stored.len().min(now.len()));
                    // The same namespace with another key or edge, else the
                    // first one lost, else the first one gained.
                    let changed = match (stored.get(at), now.get(at)) {
                        (Some(stored), Some(now)) if stored.namespace == now.namespace => stored,
                        (Some(stored), _)
                            if !now.iter().any(|now| now.namespace == stored.namespace) =>
                        {
                            stored
                        }
                        (_, Some(now)) => now,
                        (Some(stored), None) => stored,
                        (None, None) => return Some(Mismatch::Inconsistent),
                    };
                    Mismatch::Dependency(changed.namespace.clone())
                }
                Component::Inputs => Mismatch::Inputs,
                Component::View => Mismatch::View,
            });
        }
        Some(Mismatch::Inconsistent)
    }
}

/// What every namespace key of one compilation shares: the compiler, the
/// analysis and macro options, and the configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyEnvironment {
    pub compiler: Digest,
    pub options: Digest,
    pub config: Digest,
}

/// The identity `build.rs` computed of the compiler's sources and build.
pub fn build_identity() -> Digest {
    let text = env!("LIGHT_CLJS_COMPILER_IDENTITY");
    let mut identity = [0; 32];
    for (byte, at) in identity.iter_mut().zip((0..64).step_by(2)) {
        *byte = u8::from_str_radix(&text[at..at + 2], 16).expect("a hexadecimal identity");
    }
    identity
}

/// The compiler component of every key `host` analyzes with: the
/// [`build_identity`], the artifact schema version, the core operation
/// table and the macro engine with its version.
pub fn compiler_identity(host: &MacroHost) -> Digest {
    let mut fingerprint = Fingerprint::new("light-cljs compiler");
    fingerprint
        .digest(&build_identity())
        .number(u64::from(crate::artifact_codec::SCHEMA_VERSION))
        .digest(&crate::artifact_codec::core_table());
    if let Some(script) = &host.engine.script_host
        && !host.tables.interactive
    {
        fingerprint.text("host").text(&script.engine());
        return fingerprint.finish();
    }
    #[cfg(feature = "bbp-macros")]
    if host.native_macro_backend() {
        // The native form ABI (sets since light-cljs-477, lazy and cons
        // sequences since light-cljs-479) and the compile-time emission
        // mode the backend selects: core operations and literals lowered to
        // the engine's `__rt` intrinsics.
        fingerprint
            .text("big-boy-pants-js")
            .text("native-forms-4")
            .text("native-core-intrinsics-2");
        for (name, source) in host.tables.native_helpers.iter() {
            fingerprint.text(name).text(source);
        }
        // `--macro-core real` runs macros on another cljs.core
        // (light-cljs-648): never serve the port's artifacts to it, nor
        // its to the port. The port's identity is unchanged.
        if host.evaluator_options.macro_core == crate::evaluator::MacroCore::Real {
            fingerprint.text("macro-core-real");
        }
        return fingerprint.finish();
    }
    #[cfg(light_v8)]
    if !host.tables.interactive {
        fingerprint.text("v8").text(v8::V8::get_version());
        return fingerprint.finish();
    }
    let _ = host;
    fingerprint.text("quickjs");
    fingerprint.finish()
}

/// The fingerprint of an empty `--config` map.
pub fn empty_config() -> Digest {
    crate::BuildConfiguration::new().fingerprint()
}

impl KeyEnvironment {
    /// The environment of a compilation by `host` with `options`. The
    /// options component covers:
    ///
    /// - every [`AnalysisOptions`] field, the warning mode among them;
    /// - whether assertions are on (`--elide-asserts`);
    /// - [`MacroHost::opaque_packages`], true for `--compile` and false in
    ///   watch;
    /// - the reader features, which are `:cljs` alone;
    /// - the source of each compiler data reader and of the default one: a
    ///   registered reader is always its source, so it has a stable
    ///   identity;
    /// - an interactive host's name policy;
    /// - the logical macro limits (`--macro-expansions`, `--macro-nodes` and
    ///   the expansion depth) and the evaluator stack and memory limits,
    ///   which a macro can observe by catching the error (a caught
    ///   `RangeError` returns normally). The deadline and poll budget are
    ///   not: they terminate the work, which is never stored, and when they
    ///   do depends on the machine.
    pub fn new(host: &MacroHost, options: AnalysisOptions) -> Self {
        let AnalysisOptions {
            warning_mode,
            checked_arrays,
            array_checks,
        } = options;
        let crate::macro_host::MacroLimits {
            expansions,
            generated_nodes,
            expansion_depth,
        } = host.limits;
        let mut fingerprint = Fingerprint::new("light-cljs analysis options");
        fingerprint
            .flag(warning_mode)
            .flag(checked_arrays)
            .number(array_checks as u64)
            .flag(host.assertions_enabled())
            .flag(host.opaque_packages)
            .count(1)
            .text("cljs");
        host.fingerprint_data_readers(&mut fingerprint);
        host.fingerprint_name_policy(&mut fingerprint);
        fingerprint
            .number(expansions as u64)
            .number(generated_nodes as u64)
            .number(expansion_depth as u64)
            .number(host.evaluator_options.stack_limit as u64)
            .number(host.evaluator_options.memory_limit as u64);
        Self {
            compiler: compiler_identity(host),
            options: fingerprint.finish(),
            config: host.build_configuration().fingerprint(),
        }
    }
}

/// The keys of a compilation's namespaces, by [`NamespaceId`], computed in
/// source order, with the chain of every earlier name and key a coarse
/// namespace's view covers.
#[derive(Clone, Debug, Default)]
pub struct NamespaceKeys {
    keys: Vec<FrontEndKey>,
    /// `chain[i]` covers namespaces `0..i`.
    chain: Vec<Digest>,
    /// `declared[i]` covers the declaration views of namespaces `0..i`.
    declared: Vec<Digest>,
    /// The `:namespaces` broad view, every namespace's name and source
    /// identity, once a namespace with that view needs it.
    inventory: std::cell::OnceCell<Digest>,
}

impl NamespaceKeys {
    /// The key of every namespace of `manifest`, whose sources are `files`
    /// by source index, `entry` being the entry namespace.
    pub fn compute(
        environment: &KeyEnvironment,
        manifest: &DependencyManifest,
        files: &[(PathBuf, String)],
        entry: &str,
    ) -> Self {
        assert_eq!(
            files.len(),
            manifest.namespaces.len(),
            "a source per namespace"
        );
        let mut keys = Self::default();
        for (index, (_, text)) in files.iter().enumerate() {
            keys.push(
                environment,
                manifest,
                NamespaceId(index as u32),
                text,
                entry,
            );
        }
        keys
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    pub fn key(&self, id: NamespaceId) -> &FrontEndKey {
        &self.keys[id.0 as usize]
    }

    /// The digest of every earlier namespace's name and key, in source
    /// order, before namespace `id`.
    fn earlier(&self, id: NamespaceId) -> Digest {
        self.chain
            .get(id.0 as usize)
            .copied()
            .unwrap_or_else(|| Fingerprint::new("light-cljs earlier namespaces").finish())
    }

    /// The declaration view of the namespaces before `id`, in source order,
    /// which a coarse namespace's view covers ([`Self::append`]).
    fn declared(&self, id: NamespaceId) -> Digest {
        self.declared
            .get(id.0 as usize)
            .copied()
            .unwrap_or_else(|| Fingerprint::new("light-cljs earlier declarations").finish())
    }

    /// Compute the key of namespace `id`, the next in source order, whose
    /// source is `text`, from the keys of the namespaces before it.
    pub fn push(
        &mut self,
        environment: &KeyEnvironment,
        manifest: &(impl NamespaceRecords + ?Sized),
        id: NamespaceId,
        text: &str,
        entry: &str,
    ) -> &FrontEndKey {
        let key = self.next(environment, manifest, id, text, entry);
        self.append(manifest.record(id), key)
    }

    /// Append `key`, the key of the next namespace, whose record is
    /// `record` ([`Self::next`]).
    ///
    /// It extends two chains. The earlier-namespace chain takes its name
    /// and key, for the `:declarations` broad view. The declaration chain,
    /// a coarse namespace's view, takes only what a source macro of a later
    /// namespace can see of it that its compilation edges do not key
    /// (docs/compiler-cache.md, "Conservative keys and invalidation"): its
    /// place in the namespace inventory (name and source identity), its
    /// [`crate::dependency_manifest::DeclarationView`] interface, whether it ran a source macro (which
    /// decides what it adds to the compile-time namespace registry), and,
    /// when it registered declaration bundles or declared macros, its
    /// declaration key ([`declaration_key`]). A namespace whose
    /// declarations are unknown adds its whole key, as the
    /// earlier-namespace chain does. So an edit after the last form that
    /// registers anything, or to a namespace that registers nothing, leaves
    /// the declaration chain alone, unless it changes the reader context,
    /// adds a Var shadowing a core name or a refer, or starts or stops
    /// running source macros.
    pub fn append(&mut self, record: &NamespaceRecord, key: FrontEndKey) -> &FrontEndKey {
        let id = NamespaceId(self.keys.len() as u32);
        if self.chain.is_empty() {
            self.chain.push(self.earlier(id));
            self.declared.push(self.declared(id));
        }
        let mut chain = Fingerprint::new("light-cljs earlier namespaces");
        chain
            .digest(&self.earlier(id))
            .text(&record.name)
            .digest(&key.digest);
        self.chain.push(chain.finish());
        let mut declared = Fingerprint::new("light-cljs earlier declarations");
        declared
            .digest(&self.declared(id))
            .text(&record.name)
            .text(&record.source.file)
            .flag(record.source.kind == SourceKind::Cljc)
            .flag(record.source.origin == SourceOrigin::Bundled)
            .flag(record.coarse);
        match &record.declarations {
            None => declared.absent().digest(&key.digest),
            Some(view) => {
                declared.present().digest(&view.interface);
                match &view.prefix {
                    None => declared.absent(),
                    Some(prefix) => declared
                        .present()
                        .digest(&declaration_key(record, &key, prefix)),
                }
            }
        };
        self.declared.push(declared.finish());
        self.keys.push(key);
        &self.keys[id.0 as usize]
    }

    /// Append `key` for a speculative Stage B hit directly from `src` and
    /// `fields`, matching [`Self::append`] identically without constructing a
    /// temporary [`NamespaceRecord`] or coarse-edge row in [`ManifestPrefix`].
    pub(crate) fn append_stored(
        &mut self,
        src: &crate::dependency_manifest::LoadedSource,
        fields: &crate::artifact_codec::ManifestFields,
        key: FrontEndKey,
    ) -> &FrontEndKey {
        let id = NamespaceId(self.keys.len() as u32);
        if self.chain.is_empty() {
            self.chain.push(self.earlier(id));
            self.declared.push(self.declared(id));
        }
        let mut chain = Fingerprint::new("light-cljs earlier namespaces");
        chain
            .digest(&self.earlier(id))
            .text(&src.name)
            .digest(&key.digest);
        self.chain.push(chain.finish());
        let mut declared = Fingerprint::new("light-cljs earlier declarations");
        declared
            .digest(&self.declared(id))
            .text(&src.name)
            .text(&src.file)
            .flag(src.kind == SourceKind::Cljc)
            .flag(src.origin == SourceOrigin::Bundled)
            .flag(fields.coarse);
        match &fields.declarations {
            None => declared.absent().digest(&key.digest),
            Some(view) => {
                declared.present().digest(&view.interface);
                match &view.prefix {
                    None => declared.absent(),
                    Some(prefix) => {
                        let mut decl_key = Fingerprint::new("light-cljs declaration key");
                        decl_key
                            .text(&src.name)
                            .text(&src.file)
                            .flag(src.kind == SourceKind::Cljc)
                            .flag(src.origin == SourceOrigin::Bundled)
                            .digest(prefix);
                        for component in Component::ALL {
                            if component != Component::Source {
                                decl_key.digest(&key.components[component as usize]);
                            }
                        }
                        declared.present().digest(&decl_key.finish())
                    }
                }
            }
        };
        self.declared.push(declared.finish());
        self.keys.push(key);
        &self.keys[id.0 as usize]
    }

    /// Compute the [`FrontEndKey`] of a candidate in Stage B directly from
    /// `loader`, `fields`, `ns_positions`, and precomputed `source_digest` /
    /// `resolution_digest` without mutating [`ManifestPrefix`].
    pub(crate) fn next_stored(
        &self,
        environment: &KeyEnvironment,
        loader: &crate::dependency_manifest::LoaderRecord,
        fields: &crate::artifact_codec::ManifestFields,
        ns_positions: &std::collections::HashMap<String, usize>,
        id: NamespaceId,
        (source_digest, resolution_digest): (Digest, Digest),
    ) -> FrontEndKey {
        assert_eq!(
            id.0 as usize,
            self.keys.len(),
            "keys are computed in source order"
        );
        let mut components = [[0; 32]; Component::ALL.len()];
        components[Component::Compiler as usize] = environment.compiler;
        components[Component::Options as usize] = environment.options;
        components[Component::Config as usize] = environment.config;
        components[Component::Source as usize] = source_digest;
        components[Component::Resolution as usize] = resolution_digest;

        let exact = EdgeKinds(EdgeKinds::COMPILATION.0 & !EdgeKinds::COARSE.0);
        let mut edges: Vec<(NamespaceId, EdgeKinds)> =
            Vec::with_capacity(fields.dependencies.len());
        for dependency in &fields.dependencies {
            let kinds = EdgeKinds(dependency.kinds & !EdgeKinds::LAZY.0);
            if kinds == EdgeKinds::default() {
                continue;
            }
            if let Some(&pos) = ns_positions.get(dependency.namespace.as_str())
                && pos < id.0 as usize
            {
                edges.push((NamespaceId(pos as u32), kinds));
            }
        }
        edges.sort_by_key(|&(target, _)| target.0);
        let mut merged: Vec<(NamespaceId, EdgeKinds)> = Vec::with_capacity(edges.len());
        for (target, kinds) in edges {
            if let Some(last) = merged.last_mut()
                && last.0 == target
            {
                last.1 |= kinds;
            } else {
                merged.push((target, kinds));
            }
        }
        let dependencies: Vec<DependencyKey> = merged
            .into_iter()
            .filter(|(_, kinds)| kinds.intersects(exact))
            .map(|(target, mut kinds)| {
                if fields.coarse {
                    kinds |= EdgeKinds::COARSE;
                }
                DependencyKey {
                    namespace: loader.sources[target.0 as usize].name.clone(),
                    kinds: kinds.0,
                    key: self.key(target).digest,
                }
            })
            .collect();
        let mut fingerprint = Fingerprint::new("light-cljs dependencies");
        fingerprint.count(dependencies.len());
        for dependency in &dependencies {
            fingerprint
                .text(&dependency.namespace)
                .number(u64::from(dependency.kinds))
                .digest(&dependency.key);
        }
        components[Component::Dependencies as usize] = fingerprint.finish();

        let mut inputs = Fingerprint::new("light-cljs inputs");
        inputs.count(fields.inputs.len());
        let (mut namespaces, mut declarations) = (false, false);
        for input in &fields.inputs {
            external_input(&mut inputs, input);
            match input {
                ExternalInput::View(BroadView::Namespaces) => namespaces = true,
                ExternalInput::View(BroadView::Declarations) => declarations = true,
                _ => {}
            }
        }
        components[Component::Inputs as usize] = inputs.finish();

        let mut view = Fingerprint::new("light-cljs view");
        view.flag(fields.coarse).flag(declarations).flag(namespaces);
        if declarations {
            view.digest(&self.earlier(id));
        } else if fields.coarse {
            view.digest(&self.declared(id));
        }
        if namespaces {
            let mut inv = Fingerprint::new("light-cljs namespace inventory");
            let count = id.0 as usize + 1;
            inv.count(count);
            for src in &loader.sources[..count] {
                inv.text(&src.name)
                    .text(&src.file)
                    .flag(src.kind == SourceKind::Cljc)
                    .flag(src.origin == SourceOrigin::Bundled);
            }
            view.digest(&inv.finish());
        }
        view.count(fields.absent.len());
        for name in &fields.absent {
            view.text(name);
            match ns_positions
                .get(name.as_str())
                .copied()
                .filter(|&pos| pos < id.0 as usize)
            {
                Some(pos) => view
                    .present()
                    .digest(&self.key(NamespaceId(pos as u32)).digest),
                None => view.absent(),
            };
        }
        components[Component::View as usize] = view.finish();

        FrontEndKey::new(components, dependencies)
    }

    /// Remove the last key [`Self::append`] appended.
    pub fn pop(&mut self) {
        if self.keys.pop().is_some() {
            self.chain.pop();
            self.declared.pop();
        }
    }

    /// The key of namespace `id`, the next in source order, whose source is
    /// `text`, from the keys of the namespaces before it, without appending
    /// it: a lookup computes the key a stored artifact would have now and
    /// keeps it only when it matches.
    pub fn next(
        &self,
        environment: &KeyEnvironment,
        manifest: &(impl NamespaceRecords + ?Sized),
        id: NamespaceId,
        text: &str,
        entry: &str,
    ) -> FrontEndKey {
        assert_eq!(
            id.0 as usize,
            self.keys.len(),
            "keys are computed in source order"
        );
        let record = manifest.record(id);
        let mut source = Fingerprint::new("light-cljs source");
        source
            .text(&record.name)
            .flag(record.name == entry)
            .text(&record.source.file)
            .flag(record.source.kind == SourceKind::Cljc)
            .flag(record.source.origin == SourceOrigin::Bundled)
            .digest(&digest(text.as_bytes()));
        let source_digest = source.finish();

        let mut resolution = Fingerprint::new("light-cljs resolution");
        lookup(&mut resolution, manifest, record.source.lookup);
        resolution.count(record.requires.len());
        for require in &record.requires {
            resolution.text(&require.written);
            match require.target {
                RequireTarget::Namespace(target) => {
                    resolution.number(0).text(&manifest.record(target).name)
                }
                RequireTarget::Core => resolution.number(1),
                RequireTarget::Host => resolution.number(2),
            };
            resolution.number(match require.form {
                RequireForm::Declared => 0,
                RequireForm::ClojureRewrite => 1,
                RequireForm::ImplicitLazy => 2,
            });
            resolution.count(require.lookups.len());
            for &id in &require.lookups {
                lookup(&mut resolution, manifest, id);
            }
        }
        let resolution_digest = resolution.finish();
        self.next_with_digests(environment, manifest, id, source_digest, resolution_digest)
    }

    /// [`Self::next`] using precomputed [`Component::Source`] and
    /// [`Component::Resolution`] digests (computed in parallel during
    /// sliding-window cache lookup).
    pub(crate) fn next_with_digests(
        &self,
        environment: &KeyEnvironment,
        manifest: &(impl NamespaceRecords + ?Sized),
        id: NamespaceId,
        source_digest: Digest,
        resolution_digest: Digest,
    ) -> FrontEndKey {
        assert_eq!(
            id.0 as usize,
            self.keys.len(),
            "keys are computed in source order"
        );
        let record = manifest.record(id);
        let mut components = [[0; 32]; Component::ALL.len()];
        components[Component::Compiler as usize] = environment.compiler;
        components[Component::Options as usize] = environment.options;
        components[Component::Config as usize] = environment.config;
        components[Component::Source as usize] = source_digest;
        components[Component::Resolution as usize] = resolution_digest;

        // The coarse fallback's edges are the view's chain; a lazy target is
        // not a compilation dependency and may come later.
        let exact = EdgeKinds(EdgeKinds::COMPILATION.0 & !EdgeKinds::COARSE.0);
        let dependencies: Vec<DependencyKey> = manifest
            .dependency_row(id)
            .into_iter()
            .filter(|(_, kinds)| kinds.intersects(exact))
            .map(|(target, kinds)| DependencyKey {
                namespace: manifest.record(target).name.clone(),
                kinds: kinds.0,
                key: self.key(target).digest,
            })
            .collect();
        let mut fingerprint = Fingerprint::new("light-cljs dependencies");
        fingerprint.count(dependencies.len());
        for dependency in &dependencies {
            fingerprint
                .text(&dependency.namespace)
                .number(u64::from(dependency.kinds))
                .digest(&dependency.key);
        }
        components[Component::Dependencies as usize] = fingerprint.finish();

        let mut inputs = Fingerprint::new("light-cljs inputs");
        inputs.count(record.external_inputs.len());
        let (mut namespaces, mut declarations) = (false, false);
        for &input in &record.external_inputs {
            let input = manifest.input(input);
            external_input(&mut inputs, input);
            match input {
                ExternalInput::View(BroadView::Namespaces) => namespaces = true,
                ExternalInput::View(BroadView::Declarations) => declarations = true,
                _ => {}
            }
        }
        components[Component::Inputs as usize] = inputs.finish();

        // The broad views carry no fingerprint of their own. `:declarations`
        // is every earlier namespace's key; the coarse fallback's view is
        // the declaration chain, what a source macro can see of the earlier
        // namespaces beyond the edges (light-cljs-347); `:namespaces` is the
        // build's inventory, every name and source identity, later
        // namespaces' too, which a content edit leaves alone; `:config` is
        // the config component.
        let mut view = Fingerprint::new("light-cljs view");
        view.flag(record.coarse).flag(declarations).flag(namespaces);
        if declarations {
            view.digest(&self.earlier(id));
        } else if record.coarse {
            view.digest(&self.declared(id));
        }
        if namespaces {
            let inventory = *self.inventory.get_or_init(|| manifest.inventory());
            view.digest(&inventory);
        }
        view.count(record.absent.len());
        for name in &record.absent {
            view.text(name);
            match manifest.id(name).filter(|found| *found < id) {
                Some(found) => view.present().digest(&self.key(found).digest),
                None => view.absent(),
            };
        }
        components[Component::View as usize] = view.finish();

        FrontEndKey::new(components, dependencies)
    }
}

/// Precompute [`Component::Source`] for `source_rec` (matching
/// [`NamespaceKeys::next`]) without touching [`ManifestPrefix`].
pub(crate) fn source_digest_parts(
    name: &str,
    file: &str,
    kind: SourceKind,
    origin: SourceOrigin,
    text: &str,
    entry: &str,
) -> Digest {
    let mut source = Fingerprint::new("light-cljs source");
    source
        .text(name)
        .flag(name == entry)
        .text(file)
        .flag(kind == SourceKind::Cljc)
        .flag(origin == SourceOrigin::Bundled)
        .digest(&digest(text.as_bytes()));
    source.finish()
}

/// Precompute [`Component::Resolution`] for `loader.sources[id]` (matching
/// [`NamespaceKeys::next`] and [`crate::manifest_prefix::ManifestPrefix::push_stored`])
/// without touching [`ManifestPrefix`].
pub(crate) fn resolution_digest_from_loader(
    loader: &crate::dependency_manifest::LoaderRecord,
    id: usize,
    ns_positions: &std::collections::HashMap<String, usize>,
) -> Digest {
    let source_rec = &loader.sources[id];
    let mut resolution = Fingerprint::new("light-cljs resolution");
    let hash_lookup = |fingerprint: &mut Fingerprint, lookup_id: LookupId| {
        let lookup = &loader.lookups[lookup_id.0 as usize];
        fingerprint.text(&lookup.namespace).text(&lookup.requester);
        candidates(fingerprint, &lookup.candidates);
        fingerprint.optional_text(lookup.found.as_deref());
    };
    hash_lookup(&mut resolution, source_rec.lookup);
    resolution.count(source_rec.requires.len());
    for (written, target, form, lookups) in &source_rec.requires {
        resolution.text(written);
        if matches!(target.as_str(), "cljs.core" | "clojure.core") {
            resolution.number(1);
        } else if ns_positions
            .get(target.as_str())
            .is_some_and(|&pos| pos < id)
        {
            resolution.number(0).text(target);
        } else {
            resolution.number(2);
        }
        resolution.number(match form {
            RequireForm::Declared => 0,
            RequireForm::ClojureRewrite => 1,
            RequireForm::ImplicitLazy => 2,
        });
        resolution.count(lookups.len());
        for &lookup_id in lookups {
            hash_lookup(&mut resolution, lookup_id);
        }
    }
    resolution.finish()
}

/// The `:namespaces` broad view of `namespaces`: every namespace's name and
/// source identity (file, `.cljs` or `.cljc`, provider or bundled), in source
/// order.
pub(crate) fn inventory(namespaces: &[crate::dependency_manifest::NamespaceRecord]) -> Digest {
    let mut inventory = Fingerprint::new("light-cljs namespace inventory");
    inventory.count(namespaces.len());
    for record in namespaces {
        inventory
            .text(&record.name)
            .text(&record.source.file)
            .flag(record.source.kind == SourceKind::Cljc)
            .flag(record.source.origin == SourceOrigin::Bundled);
    }
    inventory.finish()
}

/// The declaration key of the namespace `record` keyed `key`, which
/// registered declaration bundles or declared macros, given `prefix`, the
/// SHA-256 of its source up to the end of the last top-level form that did
/// ([`crate::dependency_manifest::DeclarationView::prefix`]): its key with the source text replaced by
/// that prefix. Every component but the source decides how the prefix
/// compiles: the dependencies' keys cover the code its bundles and macros
/// can call in other namespaces, the inputs and view what its own macro
/// calls read. A form after the prefix can neither register a bundle nor
/// change one, since a bundle's code sees only the Vars defined before it
/// and the prefix ends at a whole form; [`crate::dependency_manifest::DeclarationView::prefix`] is the
/// whole text where a later form would redefine one of them.
pub(crate) fn declaration_key(
    record: &NamespaceRecord,
    key: &FrontEndKey,
    prefix: &Digest,
) -> Digest {
    let mut fingerprint = Fingerprint::new("light-cljs declaration key");
    fingerprint
        .text(&record.name)
        .text(&record.source.file)
        .flag(record.source.kind == SourceKind::Cljc)
        .flag(record.source.origin == SourceOrigin::Bundled)
        .digest(prefix);
    for component in Component::ALL {
        if component != Component::Source {
            fingerprint.digest(&key.components[component as usize]);
        }
    }
    fingerprint.finish()
}

fn lookup(
    fingerprint: &mut Fingerprint,
    manifest: &(impl NamespaceRecords + ?Sized),
    id: LookupId,
) {
    let lookup = manifest.lookup(id);
    fingerprint.text(&lookup.namespace).text(&lookup.requester);
    candidates(fingerprint, &lookup.candidates);
    fingerprint.optional_text(lookup.found.as_deref());
}

fn candidates(fingerprint: &mut Fingerprint, candidates: &[crate::dependency_manifest::Candidate]) {
    fingerprint.count(candidates.len());
    for candidate in candidates {
        fingerprint
            .bytes(candidate.path.as_os_str().as_encoded_bytes())
            .flag(candidate.present);
    }
}

fn external_input(fingerprint: &mut Fingerprint, input: &ExternalInput) {
    match input {
        ExternalInput::Resource(ResourceDependency {
            path,
            candidates: tried,
            found,
            stable,
        }) => {
            fingerprint.number(0).text(path);
            candidates(fingerprint, tried);
            match found {
                None => fingerprint.absent(),
                Some(ResolvedResource {
                    identity,
                    sha256,
                    bytes,
                }) => fingerprint
                    .present()
                    .optional_text(identity.as_deref())
                    .digest(sha256)
                    .number(*bytes),
            };
            fingerprint.flag(*stable);
        }
        ExternalInput::Namespace(name) => {
            fingerprint.number(1).text(name);
        }
        ExternalInput::Provider {
            token,
            fingerprint: value,
        } => {
            fingerprint.number(2).text(token).text(value);
        }
        ExternalInput::Configuration { sha256 } => {
            fingerprint.number(3).digest(sha256);
        }
        ExternalInput::View(view) => {
            fingerprint.number(4).number(match view {
                BroadView::Namespaces => 0,
                BroadView::Declarations => 1,
                BroadView::Config => 2,
            });
        }
    }
}

/// How the output is laid out, beside the [`ApplicationOptions`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OutputLayout {
    /// One script (the default `--compile` output).
    Script,
    /// One ES module (`--esm`).
    Module,
    /// Chunked ES modules (`--out-dir`).
    Chunks,
    /// A dev build (`--reloadable --out-dir`).
    Reloadable,
}

/// Where a source map goes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceMapOutput {
    None,
    /// `--source-map PATH`: the file name the output's comment names.
    File(String),
    /// `--source-map-inline`.
    Inline,
}

/// What emission takes besides the program.
#[derive(Clone, Debug)]
pub struct EmissionOptions<'a> {
    pub application: &'a ApplicationOptions,
    pub layout: OutputLayout,
    pub source_map: SourceMapOutput,
    /// `--source-map-include-sources`.
    pub include_sources: bool,
    /// The fingerprint of the module plan the namespace is emitted into:
    /// for chunked output, which chunk holds it and what that chunk imports,
    /// which depend on the whole program's lazy roots and not on this
    /// namespace's inputs alone. `None` for one-file output, or before the
    /// plan exists.
    pub modules: Option<Digest>,
}

/// The emission key of output built from the namespace whose front-end
/// key is `front_end`: that key, the compiler, and every emission option
/// (`--optimize`, defines, exports, `--pseudo-names`, `--runtime-key` and
/// the inlining and folding switches, the output layout, the module plan
/// and the source map). The artifacts hold pre-optimization IR, so until
/// they hold emitted sections nothing is reused by this key.
pub fn emission_key(
    environment: &KeyEnvironment,
    front_end: &FrontEndKey,
    options: &EmissionOptions<'_>,
) -> Digest {
    let mut fingerprint = Fingerprint::new("light-cljs emission key");
    fingerprint
        .digest(&front_end.digest)
        .digest(&environment.compiler);
    emission_fields(&mut fingerprint, options);
    fingerprint.finish()
}

/// The key of a whole build's output (light-cljs-418): `program`, the
/// digest of every namespace the compilation assembled, each with the
/// artifact it was installed from or published as
/// ([`crate::compile_cache::OutputCache`]), the compiler, and every
/// emission option, as [`emission_key`] takes them. Those artifacts are the
/// assembled program, so equal keys emit equal output.
pub fn output_key(
    environment: &KeyEnvironment,
    program: &Digest,
    options: &EmissionOptions<'_>,
) -> Digest {
    let mut fingerprint = Fingerprint::new("light-cljs output key");
    fingerprint.digest(program).digest(&environment.compiler);
    emission_fields(&mut fingerprint, options);
    fingerprint.finish()
}

/// Every emission option of `options`, into `fingerprint`.
fn emission_fields(fingerprint: &mut Fingerprint, options: &EmissionOptions<'_>) {
    let ApplicationOptions {
        optimize,
        exports,
        defines,
        no_inlining,
        no_folding,
        pseudo_names,
        runtime_key,
    } = options.application;
    fingerprint
        .number(match optimize {
            OptimizationLevel::None => 0,
            OptimizationLevel::Prepare => 1,
            OptimizationLevel::Advanced => 2,
        })
        .count(exports.len());
    for export in exports {
        fingerprint.number(export.0 as u64);
    }
    fingerprint.count(defines.len());
    for define in defines {
        fingerprint.text(&define.name);
        match &define.value {
            crate::defines::DefineValue::Boolean(value) => fingerprint.number(0).flag(*value),
            crate::defines::DefineValue::Number(value) => {
                fingerprint.number(1).number(value.to_bits())
            }
            crate::defines::DefineValue::String(value) => fingerprint.number(2).text(value),
        };
    }
    fingerprint
        .flag(*no_inlining)
        .flag(*no_folding)
        .flag(*pseudo_names)
        .optional_text(runtime_key.as_deref())
        .number(match options.layout {
            OutputLayout::Script => 0,
            OutputLayout::Module => 1,
            OutputLayout::Chunks => 2,
            OutputLayout::Reloadable => 3,
        });
    match &options.source_map {
        SourceMapOutput::None => fingerprint.number(0),
        SourceMapOutput::File(name) => fingerprint.number(1).text(name),
        SourceMapOutput::Inline => fingerprint.number(2),
    };
    fingerprint.flag(options.include_sources);
    match &options.modules {
        Some(modules) => fingerprint.present().digest(modules),
        None => fingerprint.absent(),
    };
}

/// Owned snapshot of the inputs [`revalidate`] needs for one namespace, so
/// background cache publishing workers can revalidate without borrowing
/// [`crate::manifest_prefix::ManifestPrefix`].
#[derive(Clone, Debug)]
pub(crate) struct RevalidateInputs {
    pub origin: SourceOrigin,
    pub own: crate::dependency_manifest::Lookup,
    pub require_lookups: Vec<crate::dependency_manifest::Lookup>,
    pub external_inputs: Vec<ExternalInput>,
}

impl RevalidateInputs {
    pub(crate) fn from_manifest(
        manifest: &(impl NamespaceRecords + ?Sized),
        id: NamespaceId,
        external_inputs: Vec<ExternalInput>,
    ) -> Self {
        let record = manifest.record(id);
        let own = manifest.lookup(record.source.lookup).clone();
        let mut require_lookups = Vec::new();
        for require in &record.requires {
            for &lookup in &require.lookups {
                require_lookups.push(manifest.lookup(lookup).clone());
            }
        }
        Self {
            origin: record.source.origin,
            own,
            require_lookups,
            external_inputs,
        }
    }
}

fn revalidate_parts<'a>(
    origin: SourceOrigin,
    own: &crate::dependency_manifest::Lookup,
    require_lookups: impl IntoIterator<Item = &'a crate::dependency_manifest::Lookup>,
    external_inputs: impl IntoIterator<Item = &'a ExternalInput>,
    text: &str,
    sources: &dyn SourceProvider,
) -> Result<(), Mismatch> {
    let request_for = |lookup: &crate::dependency_manifest::Lookup| LoadRequest {
        namespace: crate::compiler_data::Name::new(None, &lookup.namespace),
        requesting_namespace: crate::compiler_data::Name::new(None, &lookup.requester),
        requesting_file: None,
        reload: Reload::None,
    };
    let locate_unchanged = |lookup: &crate::dependency_manifest::Lookup| {
        let request = request_for(lookup);
        let mut tried = Vec::new();
        let found = sources
            .revalidate_locate_traced(&request, &mut tried)
            .ok()
            .flatten();
        tried == lookup.candidates && found.as_deref() == lookup.found.as_deref()
    };
    if origin == SourceOrigin::Provider {
        let request = request_for(own);
        let mut tried = Vec::new();
        let loaded = sources.load_traced(&request, &mut tried).ok().flatten();
        let unchanged = tried == own.candidates
            && loaded.as_ref().map(|input| input.file.as_str()) == own.found.as_deref();
        if !unchanged {
            return Err(Mismatch::Resolution);
        }
        if loaded.is_none_or(|input| input.source.as_bytes() != text.as_bytes()) {
            return Err(Mismatch::Source);
        }
    } else if !locate_unchanged(own) {
        return Err(Mismatch::Resolution);
    }
    for lookup in require_lookups {
        if !locate_unchanged(lookup) {
            return Err(Mismatch::Resolution);
        }
    }
    inputs_unchanged(external_inputs, sources)
}

/// Re-read the inputs of namespace `id` from `sources` after it compiled,
/// and report the first that no longer matches what the compilation read:
/// its source file and text (`text`, by source index), each lookup of its
/// resolution, asked again as the loader asked it (the same requesting
/// namespace), and each resource it read, by content. A store refuses to
/// publish an artifact whose inputs changed during its compilation, since
/// the key it would publish under describes inputs the artifact did not
/// see. A bundled source is the compiler's own, and declared namespaces and
/// provider fingerprints are values, so none of those is re-read.
pub fn revalidate(
    manifest: &(impl NamespaceRecords + ?Sized),
    id: NamespaceId,
    text: &str,
    sources: &dyn SourceProvider,
) -> Result<(), Mismatch> {
    let record = manifest.record(id);
    let own = manifest.lookup(record.source.lookup);
    revalidate_parts(
        record.source.origin,
        own,
        record.requires.iter().flat_map(|require| {
            require
                .lookups
                .iter()
                .map(|&lookup| manifest.lookup(lookup))
        }),
        record
            .external_inputs
            .iter()
            .map(|&input| manifest.input(input)),
        text,
        sources,
    )
}

/// Owned counterpart of [`revalidate`] for background cache publish workers.
pub(crate) fn revalidate_owned(
    inputs: &RevalidateInputs,
    text: &str,
    sources: &dyn SourceProvider,
) -> Result<(), Mismatch> {
    revalidate_parts(
        inputs.origin,
        &inputs.own,
        inputs.require_lookups.iter(),
        inputs.external_inputs.iter(),
        text,
        sources,
    )
}

/// Re-read each resource of `inputs` from `sources` and report
/// [`Mismatch::Inputs`] when one no longer resolves to the same candidates
/// and content. A cache lookup checks a stored artifact's inputs with it,
/// since the key it recomputes from them takes their recorded content.
pub fn inputs_unchanged<'a>(
    inputs: impl IntoIterator<Item = &'a ExternalInput>,
    sources: &dyn SourceProvider,
) -> Result<(), Mismatch> {
    let mut resources = None;
    for input in inputs {
        let ExternalInput::Resource(resource) = input else {
            continue;
        };
        let Some(resources) = resources.get_or_insert_with(|| sources.resources()) else {
            return Err(Mismatch::Inputs);
        };
        let mut tried = Vec::new();
        let answer = resources.load_resource(&resource.path, &mut tried);
        let unchanged = tried == resource.candidates
            && match (&answer, &resource.found) {
                (Ok(None), None) => true,
                (Ok(Some(now)), Some(then)) => {
                    now.identity == then.identity
                        && now.text.len() as u64 == then.bytes
                        && digest(now.text.as_bytes()) == then.sha256
                }
                // A caught ambiguity is recorded as absent and keyed by its
                // candidates, so the same candidates are the same failure.
                // Any other provider failure was never cacheable, and one
                // that absence turned into (a directory at the path, an
                // unreadable file) fails where the build that published
                // read nothing, with the same candidates.
                (Err(LoadError::AmbiguousSources(_)), None) => true,
                _ => false,
            };
        if !unchanged {
            return Err(Mismatch::Inputs);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "bbp-macros")]
    #[test]
    fn native_helpers_change_the_compiler_identity() {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let empty = compiler_identity(&host);
        host.preload_native_helper("cljs.user/helper", "(fn [x] x)")
            .unwrap();
        let first = compiler_identity(&host);
        assert_ne!(empty, first);
        host.preload_native_helper("cljs.user/helper", "(fn [x] (list '+ x 1))")
            .unwrap();
        assert_ne!(first, compiler_identity(&host));
    }

    /// `--macro-core` is part of the Pants build's compiler identity
    /// (light-cljs-648); the port's is what it was before the option.
    #[cfg(feature = "bbp-macros")]
    #[test]
    fn the_macro_core_changes_the_compiler_identity() {
        use crate::evaluator::MacroCore;
        let mut host = MacroHost::new();
        host.use_native_macros();
        host.evaluator_options.macro_core = MacroCore::Port;
        let port = compiler_identity(&host);
        host.evaluator_options.macro_core = MacroCore::Real;
        assert_ne!(port, compiler_identity(&host));
        let mut fingerprint = Fingerprint::new("light-cljs compiler");
        fingerprint
            .digest(&build_identity())
            .number(u64::from(crate::artifact_codec::SCHEMA_VERSION))
            .digest(&crate::artifact_codec::core_table())
            .text("big-boy-pants-js")
            .text("native-forms-4")
            .text("native-core-intrinsics-2");
        assert_eq!(port, fingerprint.finish());
    }

    #[test]
    fn the_macro_engine_is_part_of_the_compiler_identity() {
        let clean = MacroHost::new();
        let mut interactive = MacroHost::new();
        interactive.tables.interactive = true;
        let (v8, quickjs) = (compiler_identity(&clean), compiler_identity(&interactive));
        if cfg!(light_v8) || clean.native_macro_backend() {
            assert_ne!(v8, quickjs);
        } else {
            assert_eq!(v8, quickjs);
        }
        assert_eq!(
            crate::sha256::hex(&build_identity()),
            env!("LIGHT_CLJS_COMPILER_IDENTITY")
        );
        assert_eq!(
            KeyEnvironment::new(&clean, AnalysisOptions::default()).compiler,
            v8
        );
    }
}
