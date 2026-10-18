//! What each namespace's compilation depends on: the conservative record the
//! compiler cache keys on and invalidates with (docs/compiler-cache.md,
//! "Conservative keys and invalidation"). One [`DependencyManifest`] covers
//! a compilation. It is built once, after analysis, from what the loader
//! looked up and what analysis observed, and only when the caller asks for
//! it ([`crate::compiler::analyze_namespace_entry_with_dependencies`]).
//!
//! A namespace is a dense [`NamespaceId`], its source index (load order,
//! dependencies first). The graph is held twice in compressed rows, forward
//! and reverse, and every compilation edge points to an earlier namespace,
//! so closures are walks over integer rows. A namespace's record holds:
//!
//! - how its source was found: every [`Lookup`] of its name with the
//!   candidates tried in order, present or absent, and the bundled
//!   first-party fallback when no candidate was present;
//! - each require as written and what it became: a source namespace, the
//!   virtual core, a `goog.*` host namespace, a `clojure.X` rewritten to
//!   `cljs.X`, or the implicit `light.lazy` require;
//! - its edges by [`EdgeKinds`];
//! - the namespace names its analysis looked for and did not find loaded;
//! - its external inputs: the build configuration's fingerprint, and the
//!   resources, namespaces, provider tokens and broad views `light.build`
//!   read or declared ([`ExternalInput`]), its own and those of the helper
//!   units and data readers of any namespace it depends on.
//!
//! Matching source text is never enough on its own: a consumer's record
//! changes when a lookup it depends on would now find another file.

use std::collections::HashMap;
use std::path::PathBuf;

/// A namespace of the compilation: its source index.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct NamespaceId(pub u32);

/// A lookup of [`DependencyManifest::lookups`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct LookupId(pub u32);

/// Why one namespace depends on another, as bits; one edge may carry several.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct EdgeKinds(pub u8);

impl EdgeKinds {
    /// A direct require, after `clojure.X` rewrites, with the implicit
    /// `light.lazy` require.
    pub const RUNTIME: Self = Self(1);
    /// A direct require of a namespace that declares macros, or a fully
    /// qualified macro of a loaded namespace that was expanded.
    pub const MACRO_PROVIDER: Self = Self(1 << 1);
    /// A namespace a macro provider depends on directly: ordinary helper
    /// code its macros may run while this namespace's macros expand.
    pub const HELPER: Self = Self(1 << 2);
    /// A qualified name resolved in a loaded namespace this namespace does
    /// not require directly (macro expansions name their own namespace's
    /// dependencies), a fully qualified macro of a loaded namespace, or a
    /// namespace reflected with `ns-publics` and its relatives.
    pub const OBSERVED: Self = Self(1 << 3);
    /// A coarse dependency on every earlier namespace. The namespace ran a
    /// source macro, which sees the compilation's whole declaration view,
    /// and no finer edge can attribute that view. It includes every
    /// declaration bundle registered so far: protocols, types, extensions
    /// and multimethods, including a namespace's first one. It includes
    /// every loaded macro's metadata in `&env`, and the namespace inventory
    /// of `all-ns`, `find-ns` and `the-ns`.
    pub const COARSE: Self = Self(1 << 4);
    /// A `light.lazy/loadable` target. Not a compilation dependency: the
    /// target is checked when the program closes, and it may come later.
    pub const LAZY: Self = Self(1 << 5);
    /// The kinds a compilation closure follows: all but [`Self::LAZY`].
    pub const COMPILATION: Self = Self(0b1_1111);

    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
    pub fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

impl std::ops::BitOr for EdgeKinds {
    type Output = Self;
    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl std::ops::BitOrAssign for EdgeKinds {
    fn bitor_assign(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

/// One file a lookup tried, in the order tried.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Candidate {
    pub path: PathBuf,
    pub present: bool,
}

/// One question to the source provider: the namespace, every candidate
/// tried in order (for explicit roots, each root at `.cljs`, then each root
/// at `.cljc` when no `.cljs` was present), and the file it answered. A
/// provider other than source roots reports no candidates, only its answer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Lookup {
    pub namespace: String,
    /// The namespace whose require asked (the entry for the entry itself),
    /// as the provider saw it in `LoadRequest::requesting_namespace`.
    pub requester: String,
    pub candidates: Vec<Candidate>,
    pub found: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceKind {
    Cljs,
    Cljc,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceOrigin {
    /// The source provider (explicit roots for the CLI).
    Provider,
    /// The compiler's bundled first-party library, after the provider had
    /// no source for the name.
    Bundled,
}

/// Where a namespace's source came from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceIdentity {
    /// The resolved file: canonical for source roots, `<first-party>/...`
    /// for a bundled library.
    pub file: String,
    pub kind: SourceKind,
    pub origin: SourceOrigin,
    /// The provider lookup of the name, absent candidates included, which
    /// a bundled source followed.
    pub lookup: LookupId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequireTarget {
    Namespace(NamespaceId),
    /// `cljs.core` or `clojure.core`, emitted with the unit.
    Core,
    /// A `goog.*` Closure namespace no source provides: JavaScript host refers.
    Host,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequireForm {
    /// As written in the `ns` form.
    Declared,
    /// `clojure.X` had no source and `cljs.X` did.
    ClojureRewrite,
    /// Added because the source spells `light.lazy/loadable`.
    ImplicitLazy,
}

/// One require of a namespace and how it resolved.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequireRecord {
    /// The namespace as written (`clojure.string` before its rewrite).
    pub written: String,
    pub target: RequireTarget,
    pub form: RequireForm,
    /// Lookups made to decide this require beyond the target's own: the
    /// `goog.*` source check and the `clojure.X` check before a rewrite.
    pub lookups: Vec<LookupId>,
}

/// An input of [`DependencyManifest::inputs`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct InputId(pub u32);

/// An input a namespace depends on outside its source graph, read or
/// declared through `light.build` (docs/compiler.md, "Compile-time
/// services: light.build"). A dependency's fingerprint covers its own
/// inputs.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ExternalInput {
    /// A resource `resource` or `resource?` read, or `depend!` declared with
    /// `{:resource path}`.
    Resource(ResourceDependency),
    /// `(depend! {:namespace sym})`. When the namespace loaded before the one
    /// being analyzed, that one also gets an [`EdgeKinds::OBSERVED`] edge to
    /// it; otherwise the name is among its `absent` names.
    Namespace(String),
    /// `(depend! {:provider token :fingerprint string})`: a host provider's
    /// token and the fingerprint it gave of its current state.
    Provider { token: String, fingerprint: String },
    /// The fingerprint of the whole build configuration
    /// ([`crate::BuildConfiguration::fingerprint`]), which every namespace
    /// depends on, whether or not it reads a key with `light.build/config`.
    Configuration { sha256: [u8; 32] },
    /// `(depend-on! view)`: a coarse dependency on a broad compiler view.
    View(BroadView),
}

/// A broad compiler view `light.build/depend-on!` names, for reflective
/// macros such as test discovery. Each is coarse on purpose: a finer view,
/// such as one namespace's declarations, is a later optimization.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum BroadView {
    /// `:namespaces`: the build's namespace inventory, every namespace's name
    /// and resolved source identity.
    Namespaces,
    /// `:declarations`: the declaration views of every namespace earlier in
    /// source order.
    Declarations,
    /// `:config`: the build configuration's fingerprint.
    Config,
}

/// How a resource path resolved: one operation with reading it.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ResourceDependency {
    /// The logical path searched in every root, `./` and `../` resolved.
    pub path: String,
    /// Every location tried, in order, present or absent. A file added in
    /// another root, which would make the path ambiguous, is an absent one.
    pub candidates: Vec<Candidate>,
    /// The resource found, or `None` when absent (`resource?`'s nil, or a
    /// `depend!` of a missing path).
    pub found: Option<ResolvedResource>,
    /// Whether the provider's answer has a stable identity. A provider that
    /// cannot give one makes the consuming work non-cacheable.
    pub stable: bool,
}

/// The resource a path resolved to.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ResolvedResource {
    /// The provider's identity of the resource: the canonical file for
    /// source roots, `None` when the provider has none.
    pub identity: Option<String>,
    /// The SHA-256 of the exact text returned, as UTF-8.
    pub sha256: [u8; 32],
    pub bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NamespaceRecord {
    pub name: String,
    pub source: SourceIdentity,
    pub requires: Vec<RequireRecord>,
    /// Names analysis looked for among the loaded namespaces and did not
    /// find (a dotted symbol's prefix, a warning-mode unknown qualifier, a
    /// reflected namespace not loaded yet): such a namespace appearing
    /// earlier changes the result.
    pub absent: Vec<String>,
    /// Whether the namespace depends on every earlier one
    /// ([`EdgeKinds::COARSE`]) because it ran a source macro.
    pub coarse: bool,
    /// The [`DependencyManifest::inputs`] this namespace depends on,
    /// ascending: those read or declared while it was analyzed, and those
    /// recorded by a helper unit or data reader of it or of any namespace in
    /// its [`DependencyManifest::closure`]. The second rule is conservative:
    /// it covers a helper value computed while one namespace was compiled
    /// and reused in memory by a later one.
    pub external_inputs: Vec<InputId>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Rows {
    /// Row `i` is `targets[offsets[i]..offsets[i + 1]]`.
    offsets: Vec<u32>,
    targets: Vec<u32>,
    kinds: Vec<EdgeKinds>,
}

impl Rows {
    fn row(&self, id: u32) -> impl Iterator<Item = (NamespaceId, EdgeKinds)> + '_ {
        let range = self.offsets[id as usize] as usize..self.offsets[id as usize + 1] as usize;
        self.targets[range.clone()]
            .iter()
            .zip(&self.kinds[range])
            .map(|(&target, &kinds)| (NamespaceId(target), kinds))
    }
}

/// The dependency record of one compilation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DependencyManifest {
    /// By [`NamespaceId`].
    pub namespaces: Vec<NamespaceRecord>,
    /// By [`LookupId`], in the order the loader asked.
    pub lookups: Vec<Lookup>,
    /// By [`InputId`]: each distinct input once, in the order first recorded.
    pub inputs: Vec<ExternalInput>,
    ids: HashMap<String, u32>,
    forward: Rows,
    reverse: Rows,
}

impl DependencyManifest {
    pub fn id(&self, name: &str) -> Option<NamespaceId> {
        self.ids.get(name).map(|&id| NamespaceId(id))
    }
    pub fn record(&self, id: NamespaceId) -> &NamespaceRecord {
        &self.namespaces[id.0 as usize]
    }
    pub fn lookup(&self, id: LookupId) -> &Lookup {
        &self.lookups[id.0 as usize]
    }
    pub fn input(&self, id: InputId) -> &ExternalInput {
        &self.inputs[id.0 as usize]
    }
    /// The external inputs namespace `id` depends on.
    pub fn external_inputs(&self, id: NamespaceId) -> impl Iterator<Item = &ExternalInput> + '_ {
        self.record(id)
            .external_inputs
            .iter()
            .map(|&input| self.input(input))
    }
    /// The namespaces `id` depends on directly, ascending, with the kinds.
    pub fn dependencies(
        &self,
        id: NamespaceId,
    ) -> impl Iterator<Item = (NamespaceId, EdgeKinds)> + '_ {
        self.forward.row(id.0)
    }
    /// The namespaces that depend on `id` directly, ascending, with the kinds.
    pub fn dependents(
        &self,
        id: NamespaceId,
    ) -> impl Iterator<Item = (NamespaceId, EdgeKinds)> + '_ {
        self.reverse.row(id.0)
    }
    /// The kinds of the edge from `from` to `to`, if any.
    pub fn edge(&self, from: NamespaceId, to: NamespaceId) -> Option<EdgeKinds> {
        self.dependencies(from)
            .find(|(target, _)| *target == to)
            .map(|(_, kinds)| kinds)
    }
    /// Every namespace `id`'s compilation depends on, directly or not,
    /// through [`EdgeKinds::COMPILATION`] edges, ascending, without `id`.
    pub fn closure(&self, id: NamespaceId) -> Vec<NamespaceId> {
        self.walk(&self.forward, id, EdgeKinds::COMPILATION)
    }
    /// Every namespace whose compilation depends on `id`, directly or not:
    /// what a change to `id` invalidates, ascending, without `id`.
    pub fn reverse_closure(&self, id: NamespaceId) -> Vec<NamespaceId> {
        self.walk(&self.reverse, id, EdgeKinds::COMPILATION)
    }
    /// The closure of `id` through edges with any of `kinds`.
    pub fn closure_through(&self, id: NamespaceId, kinds: EdgeKinds) -> Vec<NamespaceId> {
        self.walk(&self.forward, id, kinds)
    }
    /// How many edges the graph holds.
    pub fn edge_count(&self) -> usize {
        self.forward.targets.len()
    }

    /// The manifest of the same program after a running session (`--watch`)
    /// compiled some of its namespaces again: the namespaces in `order`, load
    /// order, dependencies first; `revised` gives, by name, what the session
    /// knows of each one it compiled again, and `declares_macros` whether a
    /// namespace declares macros now. Every other namespace keeps its record:
    /// its lookups, requires, observations, coarse flag and inputs. The
    /// edges are derived again by the rules that built this manifest, so a
    /// changed require, a namespace that begins to declare macros or one
    /// new to the program changes the macro-provider and helper edges of
    /// every namespace it reaches.
    ///
    /// A revision's requires and observations of a namespace that is not
    /// earlier in `order` (a cycle the session refuses) make no edge.
    pub fn revise(
        &self,
        order: &[&str],
        revised: &HashMap<String, Revision>,
        declares_macros: impl Fn(&str) -> bool,
    ) -> Self {
        let position: HashMap<&str, usize> = order
            .iter()
            .enumerate()
            .map(|(at, &name)| (name, at))
            .collect();
        let earlier = |name: &str, at: usize| position.get(name).is_none_or(|&target| target < at);
        let mut loader = LoaderRecord {
            lookups: self.lookups.clone(),
            ..LoaderRecord::default()
        };
        let mut log = ObservationLog::default();
        let configuration = self.inputs.iter().find_map(|input| match input {
            ExternalInput::Configuration { sha256 } => Some(*sha256),
            _ => None,
        });
        for (at, &name) in order.iter().enumerate() {
            let observer = at as u32;
            let old = self.id(name).map(|id| (id, self.record(id)));
            let (file, kind, origin, lookup) = match old {
                Some((_, record)) => (
                    record.source.file.clone(),
                    record.source.kind,
                    record.source.origin,
                    record.source.lookup,
                ),
                None => {
                    let file = revised
                        .get(name)
                        .map(|r| r.file.clone())
                        .unwrap_or_default();
                    loader.lookups.push(Lookup {
                        namespace: name.to_owned(),
                        requester: name.to_owned(),
                        candidates: Vec::new(),
                        found: Some(file.clone()),
                    });
                    let kind = if file.ends_with(".cljc") {
                        SourceKind::Cljc
                    } else {
                        SourceKind::Cljs
                    };
                    let lookup = LookupId(loader.lookups.len() as u32 - 1);
                    (file, kind, SourceOrigin::Provider, lookup)
                }
            };
            let mut requires = Vec::new();
            match (revised.get(name), old) {
                (Some(revision), old) => {
                    for required in &revision.requires {
                        if earlier(required, at) {
                            requires.push((
                                required.clone(),
                                required.clone(),
                                RequireForm::Declared,
                                Vec::new(),
                            ));
                        }
                    }
                    // The session analyzes a form as if it required every
                    // namespace it has loaded, so it logs no qualified name
                    // of a loaded namespace: what the record observed stays,
                    // with its lazy references, an over-approximation.
                    if let Some((id, _)) = old {
                        self.keep_observations(id, observer, &|name| earlier(name, at), &mut log);
                        for (target, kinds) in self.dependencies(id) {
                            if kinds.contains(EdgeKinds::LAZY) {
                                loader
                                    .lazy
                                    .push((observer, self.record(target).name.clone()));
                            }
                        }
                    }
                    let observations = &revision.observations;
                    for (observed, kinds) in &observations.named {
                        if earlier(observed, at) {
                            log.named.push((observer, observed.clone(), *kinds));
                        }
                    }
                    if observations.coarse {
                        log.executes.push(observer);
                    }
                    for input in &observations.inputs {
                        log.inputs.push(InputRecord {
                            own: observer,
                            helpers: Vec::new(),
                            initializer: false,
                            input: input.clone(),
                        });
                    }
                    // A read while a compile-time unit ran belongs to that
                    // unit's namespace as well, which a reused value of it
                    // keeps depending on. The session cannot name it, so it
                    // goes to the macro providers and helpers this namespace
                    // depends on, as the first build's helper records do,
                    // and from those to their compilation consumers.
                    if !observations.helper_inputs.is_empty() {
                        let helpers = self.compile_time_dependencies(
                            name,
                            revision,
                            revised,
                            &declares_macros,
                            &position,
                            at,
                        );
                        for input in &observations.helper_inputs {
                            log.inputs.push(InputRecord {
                                own: observer,
                                helpers: helpers.clone(),
                                initializer: true,
                                input: input.clone(),
                            });
                        }
                    }
                }
                (None, Some((id, record))) => {
                    for require in &record.requires {
                        let target = match require.target {
                            RequireTarget::Namespace(target) => self.record(target).name.clone(),
                            RequireTarget::Core => "cljs.core".to_owned(),
                            RequireTarget::Host => require.written.clone(),
                        };
                        if earlier(&target, at) {
                            requires.push((
                                require.written.clone(),
                                target,
                                require.form,
                                require.lookups.clone(),
                            ));
                        }
                    }
                    self.keep_observations(id, observer, &|name| earlier(name, at), &mut log);
                    for (target, kinds) in self.dependencies(id) {
                        if kinds.contains(EdgeKinds::LAZY) {
                            loader
                                .lazy
                                .push((observer, self.record(target).name.clone()));
                        }
                    }
                    for input in self.external_inputs(id) {
                        log.inputs.push(InputRecord {
                            own: observer,
                            helpers: Vec::new(),
                            initializer: false,
                            input: input.clone(),
                        });
                    }
                }
                (None, None) => {}
            }
            loader.sources.push(LoadedSource {
                name: name.to_owned(),
                file,
                kind,
                origin,
                lookup,
                requires,
            });
        }
        log.configuration = configuration;
        build(
            loader,
            log,
            &crate::arena::Arena::default(),
            declares_macros,
        )
    }

    /// Log what the record of `id` observed beyond its `ns` form for the
    /// namespace at `observer` of a revision: its observed edges to the
    /// namespaces still `earlier`, its absent names and its coarse flag. An
    /// observed edge may also carry a require's macro-provider kind, which
    /// then stays: an over-approximation.
    fn keep_observations(
        &self,
        id: NamespaceId,
        observer: u32,
        earlier: &dyn Fn(&str) -> bool,
        log: &mut ObservationLog,
    ) {
        for (target, kinds) in self.dependencies(id) {
            let target = &self.record(target).name;
            if kinds.contains(EdgeKinds::OBSERVED) && earlier(target) {
                let kept = kinds.0 & (EdgeKinds::OBSERVED.0 | EdgeKinds::MACRO_PROVIDER.0);
                log.named.push((observer, target.clone(), EdgeKinds(kept)));
            }
        }
        let record = self.record(id);
        for absent in &record.absent {
            log.absent.push((observer, absent.clone()));
        }
        if record.coarse {
            log.executes.push(observer);
        }
    }

    /// The positions, before `at` in a revision's order, of the namespaces
    /// whose code may run while the revised namespace `name` compiles: the
    /// macro providers it requires, names or depended on, and their direct
    /// dependencies (its helpers), as the record and the revisions have them.
    fn compile_time_dependencies(
        &self,
        name: &str,
        revision: &Revision,
        revised: &HashMap<String, Revision>,
        declares_macros: &dyn Fn(&str) -> bool,
        position: &HashMap<&str, usize>,
        at: usize,
    ) -> Vec<u32> {
        let mut providers: Vec<String> = revision
            .requires
            .iter()
            .filter(|required| declares_macros(required))
            .cloned()
            .collect();
        providers.extend(
            revision
                .observations
                .named
                .iter()
                .filter(|(_, kinds)| kinds.contains(EdgeKinds::MACRO_PROVIDER))
                .map(|(observed, _)| observed.clone()),
        );
        let mut reached = Vec::new();
        if let Some(id) = self.id(name) {
            for (target, kinds) in self.dependencies(id) {
                let target = self.record(target).name.clone();
                if kinds.contains(EdgeKinds::MACRO_PROVIDER) {
                    providers.push(target);
                } else if kinds.contains(EdgeKinds::HELPER) {
                    reached.push(target);
                }
            }
        }
        for provider in &providers {
            reached.push(provider.clone());
            match (revised.get(provider), self.id(provider)) {
                (Some(revision), _) => reached.extend(revision.requires.iter().cloned()),
                (None, Some(id)) => reached.extend(
                    self.dependencies(id)
                        .filter(|(_, kinds)| {
                            kinds.intersects(EdgeKinds::RUNTIME | EdgeKinds::OBSERVED)
                        })
                        .map(|(target, _)| self.record(target).name.clone()),
                ),
                (None, None) => {}
            }
        }
        // A bundled library does not change while the build runs.
        let bundled = |name: &str| {
            self.id(name)
                .is_some_and(|id| self.record(id).source.origin == SourceOrigin::Bundled)
        };
        let mut positions: Vec<u32> = reached
            .iter()
            .filter(|name| !bundled(name))
            .filter_map(|name| position.get(name.as_str()))
            .filter(|&&target| target < at)
            .map(|&target| target as u32)
            .collect();
        positions.sort_unstable();
        positions.dedup();
        positions
    }

    fn walk(&self, rows: &Rows, start: NamespaceId, kinds: EdgeKinds) -> Vec<NamespaceId> {
        let mut seen = vec![false; self.namespaces.len()];
        seen[start.0 as usize] = true;
        let mut stack = vec![start.0];
        let mut reached = Vec::new();
        while let Some(id) = stack.pop() {
            for (target, edge) in rows.row(id) {
                if edge.intersects(kinds) && !std::mem::replace(&mut seen[target.0 as usize], true)
                {
                    reached.push(target);
                    stack.push(target.0);
                }
            }
        }
        reached.sort_unstable();
        reached
    }
}

/// What a running session knows of a namespace it compiled again, for
/// [`DependencyManifest::revise`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Revision {
    /// The file the namespace was loaded from.
    pub file: String,
    /// The namespaces it requires now, in the order its directives named them.
    pub requires: Vec<String>,
    pub observations: Observations,
}

/// What a session's compile of one namespace observed beyond its `ns` form
/// ([`crate::dev_session::DevSession::observing`]): the namespaces it named
/// (with [`EdgeKinds::OBSERVED`], and [`EdgeKinds::MACRO_PROVIDER`] for a
/// fully qualified macro), whether it ran a source macro, and what
/// `light.build` read or declared.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Observations {
    pub named: Vec<(String, EdgeKinds)>,
    pub coarse: bool,
    /// What `light.build` read or declared for the forms themselves.
    pub inputs: Vec<ExternalInput>,
    /// What it read or declared while a deferred unit's initializer ran,
    /// which computes a value later compiles reuse.
    pub helper_inputs: Vec<ExternalInput>,
}

/// What analysis observed beyond the `ns` forms, logged through the macro
/// host while a manifest is being recorded. Observers are source indices;
/// the macro host, which knows names only, logs names.
#[derive(Debug, Default)]
pub(crate) struct ObservationLog {
    /// `(observer, position in Program::namespaces)`: a qualified name
    /// resolved in a loaded namespace the observer does not require directly.
    pub loaded: Vec<(u32, u32)>,
    /// The source being analyzed. A namespace's deferred units are analyzed
    /// before the next source begins, so the macro host's observations
    /// belong to it.
    pub window: u32,
    /// `(observer, observed namespace, kinds)` from the macro host.
    pub named: Vec<(u32, String, EdgeKinds)>,
    /// `(observer, namespace)`: a namespace analysis looked for, not loaded.
    pub absent: Vec<(u32, String)>,
    /// Sources that ran source macros.
    pub executes: Vec<u32>,
    /// Each `light.build` read or declaration, in the order made.
    pub inputs: Vec<InputRecord>,
    /// The build configuration's fingerprint, which every namespace depends
    /// on ([`ExternalInput::Configuration`]); `None` records no such input.
    pub configuration: Option<[u8; 32]>,
}

/// One `light.build` input and the units it is recorded on.
#[derive(Debug)]
pub(crate) struct InputRecord {
    /// The source being analyzed.
    pub own: u32,
    /// The sources whose helper units or data readers were running: each
    /// deferred unit being analyzed or initialized, and the source a data
    /// reader reads.
    pub helpers: Vec<u32>,
    /// Whether a deferred unit's initializer was the innermost code running,
    /// rather than a macro or the analysis of a deferred unit: a value the
    /// initializer computes is kept and reused by later compiles.
    pub initializer: bool,
    pub input: ExternalInput,
}

impl ObservationLog {
    pub fn loaded(&mut self, observer: usize, position: usize) {
        let entry = (observer as u32, position as u32);
        if self.loaded.last() != Some(&entry) {
            self.loaded.push(entry);
        }
    }
    pub fn absent(&mut self, observer: usize, namespace: &str) {
        if self
            .absent
            .last()
            .is_none_or(|(last, name)| *last != observer as u32 || name != namespace)
        {
            self.absent.push((observer as u32, namespace.to_owned()));
        }
    }
    /// An observation of `namespace` by the source being analyzed.
    pub fn named(&mut self, namespace: &str, kinds: EdgeKinds) {
        let observer = self.window;
        if self.named.last().is_none_or(|(last, name, last_kinds)| {
            *last != observer || name != namespace || *last_kinds != kinds
        }) {
            self.named.push((observer, namespace.to_owned(), kinds));
        }
    }
    fn mark(list: &mut Vec<u32>, source: usize) {
        if list.last() != Some(&(source as u32)) {
            list.push(source as u32);
        }
    }
    pub fn executes(&mut self, source: usize) {
        Self::mark(&mut self.executes, source);
    }
}

/// What the loader recorded for one source, in load order.
#[derive(Debug)]
pub(crate) struct LoadedSource {
    pub name: String,
    pub file: String,
    pub kind: SourceKind,
    pub origin: SourceOrigin,
    pub lookup: LookupId,
    /// Requires with target names, resolved to ids once all are loaded.
    pub requires: Vec<(String, String, RequireForm, Vec<LookupId>)>,
}

/// The loader's side of a manifest: lookups, sources and lazy references.
#[derive(Debug, Default)]
pub(crate) struct LoaderRecord {
    pub lookups: Vec<Lookup>,
    pub sources: Vec<LoadedSource>,
    /// `(source, target namespace)` of each lazy reference.
    pub lazy: Vec<(u32, String)>,
}

/// Build the manifest of a finished compilation. `program_namespaces` maps
/// the log's loaded positions to names; `declares_macros` says whether a
/// source namespace declares macros.
pub(crate) fn build(
    loader: LoaderRecord,
    mut log: ObservationLog,
    program_namespaces: &crate::arena::Arena<String>,
    declares_macros: impl Fn(&str) -> bool,
) -> DependencyManifest {
    let count = loader.sources.len();
    let ids: HashMap<String, u32> = loader
        .sources
        .iter()
        .enumerate()
        .map(|(id, source)| (source.name.clone(), id as u32))
        .collect();
    let provides_macros: Vec<bool> = loader
        .sources
        .iter()
        .map(|source| declares_macros(&source.name))
        .collect();
    // Explicit edges per source, as (target, kinds) rows merged below.
    let mut rows: Vec<Vec<(u32, EdgeKinds)>> = vec![Vec::new(); count];
    let mut absent: Vec<Vec<String>> = vec![Vec::new(); count];
    let mut namespaces = Vec::with_capacity(count);
    for (id, source) in loader.sources.into_iter().enumerate() {
        let mut requires = Vec::with_capacity(source.requires.len());
        for (written, target, form, lookups) in source.requires {
            let target = if matches!(target.as_str(), "cljs.core" | "clojure.core") {
                RequireTarget::Core
            } else if let Some(&target) = ids.get(&target) {
                let mut kinds = EdgeKinds::RUNTIME;
                if provides_macros[target as usize] {
                    kinds |= EdgeKinds::MACRO_PROVIDER;
                }
                rows[id].push((target, kinds));
                RequireTarget::Namespace(NamespaceId(target))
            } else {
                RequireTarget::Host
            };
            requires.push(RequireRecord {
                written,
                target,
                form,
                lookups,
            });
        }
        namespaces.push(NamespaceRecord {
            name: source.name,
            source: SourceIdentity {
                file: source.file,
                kind: source.kind,
                origin: source.origin,
                lookup: source.lookup,
            },
            requires,
            absent: Vec::new(),
            coarse: false,
            external_inputs: Vec::new(),
        });
    }
    for (observer, position) in log.loaded.drain(..) {
        let observed = program_namespaces
            .get(position as usize)
            .and_then(|name| ids.get(name));
        if let Some(&observed) = observed
            && (observer as usize) < count
            && observed < observer
        {
            rows[observer as usize].push((observed, EdgeKinds::OBSERVED));
        }
    }
    for (observer, observed, kinds) in log.named.drain(..) {
        if observer as usize >= count {
            continue;
        }
        // A namespace later in load order was not loaded when it was asked for.
        match ids.get(&observed) {
            Some(&observed) if observed == observer => {}
            Some(&observed) if observed < observer => {
                rows[observer as usize].push((observed, kinds | EdgeKinds::OBSERVED));
            }
            _ => absent[observer as usize].push(observed),
        }
    }
    for (observer, name) in log.absent.drain(..) {
        if let Some(list) = absent.get_mut(observer as usize) {
            list.push(name);
        }
    }
    for (source, target) in loader.lazy {
        if let Some(&target) = ids.get(&target)
            && target != source
        {
            rows[source as usize].push((target, EdgeKinds::LAZY));
        }
    }
    for row in &mut rows {
        merge(row);
    }
    // Helpers: what each macro provider depends on directly through its
    // requires and observations, which its macros' code may call.
    let helper_sources = EdgeKinds::RUNTIME | EdgeKinds::OBSERVED;
    let mut helpers = Vec::new();
    for id in 0..count {
        helpers.clear();
        for &(provider, kinds) in &rows[id] {
            if kinds.contains(EdgeKinds::MACRO_PROVIDER) {
                helpers.extend(
                    rows[provider as usize]
                        .iter()
                        .filter(|(_, kinds)| kinds.intersects(helper_sources))
                        .map(|&(target, _)| (target, EdgeKinds::HELPER)),
                );
            }
        }
        if !helpers.is_empty() {
            rows[id].append(&mut helpers);
            merge(&mut rows[id]);
        }
    }
    // A source macro sees the compilation's whole declaration view, which no
    // edge can attribute (see `EdgeKinds::COARSE`): a namespace that ran one
    // depends on every earlier namespace.
    let mut executes = vec![false; count];
    for &source in &log.executes {
        if let Some(flag) = executes.get_mut(source as usize) {
            *flag = true;
        }
    }
    for id in 0..count {
        if executes[id] {
            namespaces[id].coarse = true;
            rows[id].extend((0..id as u32).map(|earlier| (earlier, EdgeKinds::COARSE)));
            merge(&mut rows[id]);
        }
    }
    for (id, mut names) in absent.into_iter().enumerate() {
        names.sort_unstable();
        names.dedup();
        namespaces[id].absent = names;
    }
    let forward = compress(
        count,
        rows.iter()
            .enumerate()
            .flat_map(|(from, row)| row.iter().map(move |&(to, kinds)| (from as u32, to, kinds))),
    );
    let reverse = compress(
        count,
        rows.iter()
            .enumerate()
            .flat_map(|(from, row)| row.iter().map(move |&(to, kinds)| (to, from as u32, kinds))),
    );
    debug_assert!(rows.iter().enumerate().all(|(from, row)| {
        row.iter()
            .all(|&(to, kinds)| kinds == EdgeKinds::LAZY || (to as usize) < from)
    }));
    let mut manifest = DependencyManifest {
        namespaces,
        lookups: loader.lookups,
        inputs: Vec::new(),
        ids,
        forward,
        reverse,
    };
    attribute_inputs(&mut manifest, log.inputs, log.configuration);
    manifest
}

/// Give each namespace the configuration's fingerprint, its own inputs,
/// and the inputs of the helper units and readers of each namespace it
/// depends on: those recorded on a namespace's helpers go to it and to its
/// whole reverse closure.
fn attribute_inputs(
    manifest: &mut DependencyManifest,
    records: Vec<InputRecord>,
    configuration: Option<[u8; 32]>,
) {
    if records.is_empty() && configuration.is_none() {
        return;
    }
    let count = manifest.namespaces.len();
    let mut interned: HashMap<ExternalInput, u32> = HashMap::new();
    let mut own: Vec<Vec<InputId>> = vec![Vec::new(); count];
    if let Some(sha256) = configuration {
        let input = ExternalInput::Configuration { sha256 };
        let id = InputId(manifest.inputs.len() as u32);
        manifest.inputs.push(input.clone());
        interned.insert(input, id.0);
        for list in &mut own {
            list.push(id);
        }
    }
    let mut helpers: Vec<Vec<InputId>> = vec![Vec::new(); count];
    for record in records {
        let next = manifest.inputs.len() as u32;
        let id = *interned.entry(record.input).or_insert_with_key(|input| {
            manifest.inputs.push(input.clone());
            next
        });
        if let Some(list) = own.get_mut(record.own as usize) {
            list.push(InputId(id));
        }
        for helper in record.helpers {
            if let Some(list) = helpers.get_mut(helper as usize) {
                list.push(InputId(id));
            }
        }
    }
    for (source, mut inputs) in helpers.into_iter().enumerate() {
        if inputs.is_empty() {
            continue;
        }
        inputs.sort_unstable();
        inputs.dedup();
        let source = NamespaceId(source as u32);
        for consumer in std::iter::once(source).chain(manifest.reverse_closure(source)) {
            own[consumer.0 as usize].extend_from_slice(&inputs);
        }
    }
    for (record, mut inputs) in manifest.namespaces.iter_mut().zip(own) {
        inputs.sort_unstable();
        inputs.dedup();
        record.external_inputs = inputs;
    }
}

/// Sort a row by target and merge the kinds of duplicate targets.
fn merge(row: &mut Vec<(u32, EdgeKinds)>) {
    row.sort_unstable_by_key(|&(target, _)| target);
    row.dedup_by(|next, kept| {
        if next.0 == kept.0 {
            kept.1 |= next.1;
            true
        } else {
            false
        }
    });
}

/// Compressed rows of `(row, target, kinds)` edges, each row ascending.
fn compress(count: usize, edges: impl Iterator<Item = (u32, u32, EdgeKinds)> + Clone) -> Rows {
    let mut offsets = vec![0u32; count + 1];
    for (row, _, _) in edges.clone() {
        offsets[row as usize + 1] += 1;
    }
    for i in 0..count {
        offsets[i + 1] += offsets[i];
    }
    let total = offsets[count] as usize;
    let mut targets = vec![0u32; total];
    let mut kinds = vec![EdgeKinds::default(); total];
    let mut next = offsets.clone();
    for (row, target, edge) in edges {
        let at = next[row as usize] as usize;
        targets[at] = target;
        kinds[at] = edge;
        next[row as usize] += 1;
    }
    // Forward rows arrive sorted; reverse rows arrive in source order,
    // which is ascending too, because sources are visited in order.
    Rows {
        offsets,
        targets,
        kinds,
    }
}
