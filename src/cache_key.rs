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
//! - **view**: the incoming view the dependency manifest observed: for a
//!   coarse namespace or one with the `:declarations` broad view, the chain
//!   of every earlier namespace's name and key in source order
//!   ([`NamespaceKeys`]); for one with the `:namespaces` view, the build's
//!   inventory, every namespace's name and source identity (file, `.cljs` or
//!   `.cljc`, provider or bundled), later ones too, which an edit to a
//!   namespace's text leaves alone;
//!   and for each name it looked for and did not find loaded, whether an
//!   earlier namespace has that name now. The `:config` view is the config
//!   component.
//!
//! [`revalidate`] re-reads a namespace's inputs after it compiles, so that a
//! store does not publish an artifact whose inputs changed meanwhile.

use crate::analyzer::AnalysisOptions;
use crate::application::{ApplicationOptions, OptimizationLevel};
use crate::dependency_manifest::{
    BroadView, DependencyManifest, EdgeKinds, ExternalInput, LookupId, NamespaceId, RequireForm,
    RequireTarget, ResolvedResource, ResourceDependency, SourceKind, SourceOrigin,
};
use crate::macro_host::MacroHost;
use crate::manifest_prefix::NamespaceRecords;
use crate::sha256::{Fingerprint, digest};
use crate::source_provider::{LoadRequest, Reload, SourceProvider};
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
    if let Some(script) = &host.script_host
        && !host.interactive
    {
        fingerprint.text("host").text(&script.engine());
        return fingerprint.finish();
    }
    #[cfg(light_v8)]
    if !host.interactive {
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
        self.append(&manifest.record(id).name, key)
    }

    /// Append `key`, the key of the next namespace, `name`
    /// ([`Self::next`]).
    pub fn append(&mut self, name: &str, key: FrontEndKey) -> &FrontEndKey {
        let id = NamespaceId(self.keys.len() as u32);
        if self.chain.is_empty() {
            self.chain.push(self.earlier(id));
        }
        let mut chain = Fingerprint::new("light-cljs earlier namespaces");
        chain
            .digest(&self.earlier(id))
            .text(name)
            .digest(&key.digest);
        self.chain.push(chain.finish());
        self.keys.push(key);
        &self.keys[id.0 as usize]
    }

    /// Remove the last key [`Self::append`] appended.
    pub fn pop(&mut self) {
        if self.keys.pop().is_some() {
            self.chain.pop();
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
        let mut components = [[0; 32]; Component::ALL.len()];
        components[Component::Compiler as usize] = environment.compiler;
        components[Component::Options as usize] = environment.options;
        components[Component::Config as usize] = environment.config;

        let mut source = Fingerprint::new("light-cljs source");
        source
            .text(&record.name)
            .flag(record.name == entry)
            .text(&record.source.file)
            .flag(record.source.kind == SourceKind::Cljc)
            .flag(record.source.origin == SourceOrigin::Bundled)
            .digest(&digest(text.as_bytes()));
        components[Component::Source as usize] = source.finish();

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
        components[Component::Resolution as usize] = resolution.finish();

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
        // is the coarse fallback's view, every earlier namespace's key;
        // `:namespaces` is the build's inventory, every name and source
        // identity, later namespaces' too, which a content edit leaves alone;
        // `:config` is the config component.
        let mut view = Fingerprint::new("light-cljs view");
        view.flag(record.coarse).flag(declarations).flag(namespaces);
        if record.coarse || declarations {
            view.digest(&self.earlier(id));
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
/// (`--optimize`, defines, exports, `--pseudo-names` and the inlining and
/// folding switches, the output layout, the module plan and the source
/// map). The artifacts hold pre-optimization IR, so until they hold emitted
/// sections nothing is reused by this key.
pub fn emission_key(
    environment: &KeyEnvironment,
    front_end: &FrontEndKey,
    options: &EmissionOptions<'_>,
) -> Digest {
    let ApplicationOptions {
        optimize,
        exports,
        defines,
        no_inlining,
        no_folding,
        pseudo_names,
    } = options.application;
    let mut fingerprint = Fingerprint::new("light-cljs emission key");
    fingerprint
        .digest(&front_end.digest)
        .digest(&environment.compiler)
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
    fingerprint.finish()
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
    let reload = |lookup: &crate::dependency_manifest::Lookup| {
        let request = LoadRequest {
            namespace: crate::compiler_data::Name::new(None, &lookup.namespace),
            requesting_namespace: crate::compiler_data::Name::new(None, &lookup.requester),
            requesting_file: None,
            reload: Reload::None,
        };
        let mut tried = Vec::new();
        let loaded = sources.load_traced(&request, &mut tried).ok().flatten();
        let unchanged = tried == lookup.candidates
            && loaded.as_ref().map(|input| input.file.as_str()) == lookup.found.as_deref();
        (unchanged, loaded)
    };
    let own = manifest.lookup(record.source.lookup);
    let (unchanged, loaded) = reload(own);
    if !unchanged {
        return Err(Mismatch::Resolution);
    }
    if record.source.origin == SourceOrigin::Provider
        && loaded.is_none_or(|input| digest(input.source.as_bytes()) != digest(text.as_bytes()))
    {
        return Err(Mismatch::Source);
    }
    for require in &record.requires {
        for &lookup in &require.lookups {
            if !reload(manifest.lookup(lookup)).0 {
                return Err(Mismatch::Resolution);
            }
        }
    }
    inputs_unchanged(
        record
            .external_inputs
            .iter()
            .map(|&input| manifest.input(input)),
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
                // A failed lookup is recorded as absent, with its candidates.
                (Err(_), None) => true,
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

    #[test]
    fn the_macro_engine_is_part_of_the_compiler_identity() {
        let clean = MacroHost::new();
        let mut interactive = MacroHost::new();
        interactive.interactive = true;
        let (v8, quickjs) = (compiler_identity(&clean), compiler_identity(&interactive));
        if cfg!(light_v8) {
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
