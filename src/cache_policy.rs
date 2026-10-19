//! Whether each namespace of a build may read and write the persistent
//! compiler cache, and why not when it may not (docs/compiler-cache.md,
//! "Opt-outs and their propagation"). One evaluator serves native builds and
//! the watch loop.
//!
//! [`evaluate`] reads only current inputs: the build's [`CacheSettings`], the
//! metadata of each namespace's `ns` form ([`source_metadata`]), the
//! `light.build` inputs that cannot be keyed, and the build's
//! [`DependencyManifest`]. It takes no artifact, so a changed setting or
//! annotation takes effect even when an old artifact exists.
//!
//! The reasons ([`Bypass`]):
//!
//! - the persistent cache is off for the build;
//! - `^:light-cljs/no-cache` on the namespace, which covers that namespace
//!   only;
//! - `^:dev/always` on the namespace or on a namespace its compilation
//!   depends on;
//! - a `--cache-blocker` namespace its compilation depends on, or the
//!   namespace itself;
//! - a `light.build` input that cannot be keyed (a provider without a stable
//!   identity) in the namespace or in one its compilation depends on.
//!
//! The last three propagate to every compilation consumer: the reverse
//! closure through [`EdgeKinds::COMPILATION`] edges, which are the runtime,
//! macro-provider, helper, observed and coarse edges. That is deliberately
//! broader than Shadow's direct-only blocker rule. Each propagated reason
//! keeps, per namespace, the dependency it was reached through, so
//! [`CachePolicy::path`] gives a shortest path from the namespace that set
//! it. One breadth-first walk per reason visits each namespace and edge at
//! most once.
//!
//! [`CachePolicy::always_compile`] is kept apart from the bypass reasons:
//! watch recompiles that set on every triggered step, while a disk bypass
//! leaves valid in-memory output alone. It is computed whether or not the
//! persistent cache is on.

use crate::dependency_manifest::{DependencyManifest, EdgeKinds, ExternalInput, NamespaceId};
use std::collections::VecDeque;

/// What the build's configuration says about the persistent cache.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CacheSettings {
    /// Whether the build reads and writes the persistent cache at all.
    pub enabled: bool,
    /// The `--cache-blocker` namespaces. A name the build does not load
    /// blocks nothing.
    pub blockers: Vec<String>,
}

/// The policy flags of one namespace's `ns` form: its symbol's metadata,
/// docstring and attribute map, as [`crate::MacroHost`] keeps them.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NamespaceMetadata {
    /// `^:light-cljs/no-cache`: no reads or writes for this namespace only.
    pub no_cache: bool,
    /// `^:dev/always`: always compile this namespace and its compilation
    /// consumers.
    pub always: bool,
}

impl NamespaceMetadata {
    /// The flags of a namespace symbol carrying its `ns` form's metadata.
    /// A flag is set by any value other than `nil` or `false`.
    pub fn of_symbol(symbol: &crate::compiler_data::Datum) -> Self {
        Self {
            no_cache: metadata_flag(symbol, "light-cljs", "no-cache"),
            always: metadata_flag(symbol, "dev", "always"),
        }
    }
}

/// The metadata of every namespace of `manifest`, by [`NamespaceId`], from
/// the `ns` forms `host` analyzed in the build the manifest records.
pub fn source_metadata(
    manifest: &DependencyManifest,
    host: &crate::MacroHost,
) -> Vec<NamespaceMetadata> {
    manifest
        .namespaces
        .iter()
        .map(|record| {
            host.namespace_symbol(&record.name)
                .map(NamespaceMetadata::of_symbol)
                .unwrap_or_default()
        })
        .collect()
}

/// Whether `symbol`'s metadata maps the keyword `:namespace/name` to a value
/// other than `nil` or `false`.
pub(crate) fn metadata_flag(
    symbol: &crate::compiler_data::Datum,
    namespace: &str,
    name: &str,
) -> bool {
    use crate::compiler_data::{DatumKind, Name};
    use crate::parser::CollectionKind;
    let Some(metadata) = symbol.metadata() else {
        return false;
    };
    let DatumKind::Collection {
        kind: CollectionKind::Map,
        children,
    } = metadata.kind()
    else {
        return false;
    };
    let flag = Name::new(Some(namespace), name);
    children.as_chunks::<2>().0.iter().any(|[key, value]| {
        matches!(key.kind(), DatumKind::Keyword(keyword) if *keyword == flag)
            && !matches!(value.kind(), DatumKind::Nil | DatumKind::Boolean(false))
    })
}

/// The `--cache-blocker` namespaces and their consumers that a build knows
/// before it analyzes each namespace, for the main-isolate affinity of
/// `--jobs N` (docs/compiler-concurrency.md, "As built (light-cljs-340)").
/// A namespace is reached when it is a blocker, when one of its requires
/// was reached (the runtime and macro-provider edges of the `ns` form,
/// whose transitive closure also covers every helper edge, since a helper
/// is a direct dependency of a provider), or when its source names a
/// reached namespace: a symbol qualified with it or with an alias of it,
/// such as a fully qualified macro of a loaded provider the namespace does
/// not require, or the namespace's own name, such as a reflected
/// `'provider`. That scan stands, conservatively and before any unit is
/// dispatched, for the observed edges [`evaluate`] follows after analysis;
/// a name built at compile time escapes it. The coarse edges of source
/// macros give no affinity (see "Open questions" in the design), and
/// neither does `^:light-cljs/no-cache`.
#[derive(Clone, Debug, Default)]
pub struct BlockerReach {
    blockers: Vec<String>,
    /// The namespaces admitted so far that were reached.
    reached: std::collections::HashSet<String>,
}

impl BlockerReach {
    pub fn new(blockers: &[String]) -> Self {
        Self {
            blockers: blockers.to_vec(),
            reached: Default::default(),
        }
    }

    /// Whether namespace `name` is a blocker or a consumer of one, recorded
    /// for the namespaces after it: with `requires` (each admitted before
    /// it), `aliases` (alias, target) and the text of each symbol its
    /// source has, `symbols`, read only while nothing else decides.
    /// Without blockers nothing is read or recorded.
    pub fn admit<'s>(
        &mut self,
        name: &str,
        requires: &[String],
        aliases: &[(String, String)],
        symbols: impl IntoIterator<Item = &'s str>,
    ) -> bool {
        if self.blockers.is_empty() {
            return false;
        }
        let reached = self.blockers.iter().any(|blocker| blocker == name)
            || requires.iter().any(|target| self.reached.contains(target))
            || (!self.reached.is_empty()
                && symbols.into_iter().any(|symbol| {
                    let named = match symbol.split_once('/') {
                        Some((namespace, _)) if !namespace.is_empty() => namespace,
                        _ => symbol,
                    };
                    self.reached.contains(named)
                        || aliases
                            .iter()
                            .any(|(alias, target)| alias == named && self.reached.contains(target))
                }));
        if reached {
            self.reached.insert(name.to_owned());
        }
        reached
    }
}

/// A `light.build` input of a namespace that cannot be keyed, such as a
/// resource from a provider without a stable identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NonCacheableInput {
    pub namespace: NamespaceId,
    /// What the input is, for the bypass report.
    pub description: String,
}

/// The `light.build` inputs of `manifest` that cannot be keyed, for
/// [`evaluate`]: each resource a provider answered without a stable identity
/// ([`crate::dependency_manifest::ResourceDependency::stable`]), on every
/// namespace whose manifest carries it.
pub fn non_cacheable_inputs(manifest: &DependencyManifest) -> Vec<NonCacheableInput> {
    let mut inputs = Vec::new();
    for id in 0..manifest.namespaces.len() as u32 {
        let namespace = NamespaceId(id);
        for input in manifest.external_inputs(namespace) {
            if let ExternalInput::Resource(resource) = input
                && !resource.stable
            {
                inputs.push(NonCacheableInput {
                    namespace,
                    description: format!(
                        "resource {} from a provider without stable identities",
                        resource.path
                    ),
                });
            }
        }
    }
    inputs
}

/// Why a namespace may not read or write the persistent cache. A namespace
/// may have several; [`CachePolicy::reason`] reports the first in this
/// order.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Bypass {
    /// The persistent cache is off for the build.
    CacheOff,
    /// `^:light-cljs/no-cache` on the namespace.
    NoCache,
    /// `^:dev/always` on the namespace or on a dependency.
    AlwaysCompile,
    /// A `--cache-blocker` namespace, or a consumer of one.
    Blocked,
    /// A `light.build` input that cannot be keyed, here or in a dependency.
    NonCacheableInput,
}

impl Bypass {
    const ALL: [Self; 5] = [
        Self::CacheOff,
        Self::NoCache,
        Self::AlwaysCompile,
        Self::Blocked,
        Self::NonCacheableInput,
    ];
    fn bit(self) -> u8 {
        1 << self as u8
    }
    /// The propagated reasons' index into [`CachePolicy::via`].
    fn propagated(self) -> Option<usize> {
        match self {
            Self::AlwaysCompile => Some(0),
            Self::Blocked => Some(1),
            Self::NonCacheableInput => Some(2),
            Self::CacheOff | Self::NoCache => None,
        }
    }
}

/// No namespace: a propagated reason that did not reach it.
const UNREACHED: u32 = u32::MAX;

/// The cache decision for every namespace of one build, by [`NamespaceId`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CachePolicy {
    /// The [`Bypass`] bits of each namespace.
    reasons: Vec<u8>,
    /// Per propagated reason, each namespace's predecessor on a shortest
    /// path from the namespace that set it: itself at the origin,
    /// [`UNREACHED`] where the reason does not apply.
    via: [Vec<u32>; 3],
    /// The `^:dev/always` namespaces and their consumers, ascending.
    always: Vec<NamespaceId>,
    /// The non-cacheable inputs of the namespaces that recorded them.
    inputs: Vec<NonCacheableInput>,
}

/// Decide, for every namespace of `manifest`, whether it may read and write
/// the persistent cache, from the current `settings`, the `metadata` of each
/// namespace's `ns` form (by [`NamespaceId`]; a namespace past its end has
/// none) and the inputs that cannot be keyed.
pub fn evaluate(
    manifest: &DependencyManifest,
    settings: &CacheSettings,
    metadata: &[NamespaceMetadata],
    non_cacheable: &[NonCacheableInput],
) -> CachePolicy {
    let count = manifest.namespaces.len();
    let mut reasons = vec![0u8; count];
    let flags = |id: usize| metadata.get(id).copied().unwrap_or_default();
    for (id, reason) in reasons.iter_mut().enumerate() {
        if !settings.enabled {
            *reason |= Bypass::CacheOff.bit();
        }
        if flags(id).no_cache {
            *reason |= Bypass::NoCache.bit();
        }
    }
    let always_origins: Vec<u32> = (0..count as u32)
        .filter(|&id| flags(id as usize).always)
        .collect();
    let blocker_origins: Vec<u32> = settings
        .blockers
        .iter()
        .filter_map(|name| manifest.id(name))
        .map(|id| id.0)
        .collect();
    let input_origins: Vec<u32> = non_cacheable
        .iter()
        .map(|input| input.namespace.0)
        .filter(|&id| (id as usize) < count)
        .collect();
    let mut queue = VecDeque::new();
    let via = [
        (Bypass::AlwaysCompile, always_origins),
        (Bypass::Blocked, blocker_origins),
        (Bypass::NonCacheableInput, input_origins),
    ]
    .map(|(reason, origins)| {
        let via = consumers(manifest, &origins, &mut queue);
        for (id, &before) in via.iter().enumerate() {
            if before != UNREACHED {
                reasons[id] |= reason.bit();
            }
        }
        via
    });
    let always = (0..count as u32)
        .filter(|&id| via[0][id as usize] != UNREACHED)
        .map(NamespaceId)
        .collect();
    CachePolicy {
        reasons,
        via,
        always,
        inputs: non_cacheable
            .iter()
            .filter(|input| (input.namespace.0 as usize) < count)
            .cloned()
            .collect(),
    }
}

/// Each namespace's predecessor on a shortest path from one of `origins`
/// through the reverse compilation edges: itself at an origin, [`UNREACHED`]
/// where no origin reaches it.
fn consumers(
    manifest: &DependencyManifest,
    origins: &[u32],
    queue: &mut VecDeque<u32>,
) -> Vec<u32> {
    let mut via = vec![UNREACHED; manifest.namespaces.len()];
    if origins.is_empty() {
        return via;
    }
    queue.clear();
    for &origin in origins {
        if via[origin as usize] == UNREACHED {
            via[origin as usize] = origin;
            queue.push_back(origin);
        }
    }
    while let Some(id) = queue.pop_front() {
        for (consumer, kinds) in manifest.dependents(NamespaceId(id)) {
            if kinds.intersects(EdgeKinds::COMPILATION) && via[consumer.0 as usize] == UNREACHED {
                via[consumer.0 as usize] = id;
                queue.push_back(consumer.0);
            }
        }
    }
    via
}

impl CachePolicy {
    /// Whether `id` may read and write the persistent cache.
    pub fn may_use_cache(&self, id: NamespaceId) -> bool {
        self.reasons[id.0 as usize] == 0
    }
    /// The first reason `id` may not use the cache, in [`Bypass`] order.
    pub fn reason(&self, id: NamespaceId) -> Option<Bypass> {
        self.reasons(id).next()
    }
    /// Every reason `id` may not use the cache, in [`Bypass`] order.
    pub fn reasons(&self, id: NamespaceId) -> impl Iterator<Item = Bypass> + '_ {
        let bits = self.reasons[id.0 as usize];
        Bypass::ALL
            .into_iter()
            .filter(move |reason| bits & reason.bit() != 0)
    }
    /// Whether `reason` applies to `id`.
    pub fn has(&self, id: NamespaceId, reason: Bypass) -> bool {
        self.reasons[id.0 as usize] & reason.bit() != 0
    }
    /// For a propagated reason that applies to `id`, a shortest path of
    /// compilation dependencies from the namespace that set it to `id`, both
    /// included: `[id]` when `id` set it. Empty when the reason does not
    /// apply, and for [`Bypass::CacheOff`] and [`Bypass::NoCache`], which
    /// do not propagate.
    pub fn path(&self, id: NamespaceId, reason: Bypass) -> Vec<NamespaceId> {
        let Some(index) = reason.propagated() else {
            return Vec::new();
        };
        let via = &self.via[index];
        let mut path = Vec::new();
        let mut at = id.0;
        if via[at as usize] == UNREACHED {
            return path;
        }
        loop {
            path.push(NamespaceId(at));
            let before = via[at as usize];
            if before == at {
                break;
            }
            at = before;
        }
        path.reverse();
        path
    }
    /// The `^:dev/always` namespaces and their compilation consumers,
    /// ascending: what watch recompiles on every triggered step. It does
    /// not depend on whether the persistent cache is on.
    pub fn always_compile(&self) -> &[NamespaceId] {
        &self.always
    }
    /// The inputs that cannot be keyed which namespace `origin` recorded.
    pub fn non_cacheable_inputs(
        &self,
        origin: NamespaceId,
    ) -> impl Iterator<Item = &NonCacheableInput> + '_ {
        self.inputs
            .iter()
            .filter(move |input| input.namespace == origin)
    }
    /// A line for the bypass report: the first reason, with the path of a
    /// propagated one. `None` when `id` may use the cache.
    pub fn describe(&self, manifest: &DependencyManifest, id: NamespaceId) -> Option<String> {
        let reason = self.reason(id)?;
        let path = |reason| {
            self.path(id, reason)
                .iter()
                .map(|&id| manifest.record(id).name.as_str())
                .collect::<Vec<_>>()
                .join(" -> ")
        };
        Some(match reason {
            Bypass::CacheOff => "cache off for the build".to_owned(),
            Bypass::NoCache => "^:light-cljs/no-cache".to_owned(),
            Bypass::AlwaysCompile => format!("^:dev/always through {}", path(reason)),
            Bypass::Blocked => format!("cache blocker through {}", path(reason)),
            Bypass::NonCacheableInput => {
                let origin = self.path(id, reason)[0];
                let inputs: Vec<&str> = self
                    .non_cacheable_inputs(origin)
                    .map(|input| input.description.as_str())
                    .collect();
                format!(
                    "input without a stable identity ({}) through {}",
                    inputs.join(", "),
                    path(reason)
                )
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dependency_manifest::{
        LoadedSource, LoaderRecord, LookupId, ObservationLog, RequireForm, SourceKind, SourceOrigin,
    };

    /// A manifest of `sources`, `(name, requires)` in load order, where the
    /// names in `macros` declare macros. No namespace ran a source macro,
    /// so there are no coarse edges.
    fn manifest(sources: &[(&str, &[&str])], macros: &[&str]) -> DependencyManifest {
        let loader = LoaderRecord {
            sources: sources
                .iter()
                .map(|(name, requires)| LoadedSource {
                    name: (*name).to_owned(),
                    file: format!("{name}.cljs"),
                    kind: SourceKind::Cljs,
                    origin: SourceOrigin::Provider,
                    lookup: LookupId(0),
                    requires: requires
                        .iter()
                        .map(|target| {
                            (
                                (*target).to_owned(),
                                (*target).to_owned(),
                                RequireForm::Declared,
                                Vec::new(),
                            )
                        })
                        .collect(),
                })
                .collect(),
            ..LoaderRecord::default()
        };
        let program_namespaces = sources.iter().map(|(name, _)| (*name).to_owned()).collect();
        crate::dependency_manifest::build(
            loader,
            ObservationLog::default(),
            &program_namespaces,
            |name| macros.contains(&name),
        )
    }

    fn on(blockers: &[&str]) -> CacheSettings {
        CacheSettings {
            enabled: true,
            blockers: blockers.iter().map(|name| (*name).to_owned()).collect(),
        }
    }

    fn id(manifest: &DependencyManifest, name: &str) -> NamespaceId {
        manifest.id(name).unwrap()
    }

    fn names(manifest: &DependencyManifest, ids: &[NamespaceId]) -> Vec<String> {
        ids.iter()
            .map(|&id| manifest.record(id).name.clone())
            .collect()
    }

    /// The namespaces `policy` lets use the cache, in load order.
    fn cacheable(manifest: &DependencyManifest, policy: &CachePolicy) -> Vec<String> {
        manifest
            .namespaces
            .iter()
            .filter(|record| policy.may_use_cache(id(manifest, &record.name)))
            .map(|record| record.name.clone())
            .collect()
    }

    fn metadata(
        manifest: &DependencyManifest,
        flags: &[(&str, NamespaceMetadata)],
    ) -> Vec<NamespaceMetadata> {
        let mut metadata = vec![NamespaceMetadata::default(); manifest.namespaces.len()];
        for (name, flags) in flags {
            metadata[id(manifest, name).0 as usize] = *flags;
        }
        metadata
    }

    const NO_CACHE: NamespaceMetadata = NamespaceMetadata {
        no_cache: true,
        always: false,
    };
    const ALWAYS: NamespaceMetadata = NamespaceMetadata {
        no_cache: false,
        always: true,
    };

    /// `app` uses the macros of `m`, which requires the plain `h`; `app`
    /// requires `m` and `r`, never `h`, so `app` reaches `h` only through
    /// a helper edge. `leaf` is unrelated.
    fn helper_graph() -> DependencyManifest {
        manifest(
            &[
                ("h", &[]),
                ("m", &["h"]),
                ("r", &[]),
                ("leaf", &[]),
                ("app", &["m", "r"]),
            ],
            &["m"],
        )
    }

    #[test]
    fn with_no_opt_out_every_namespace_may_use_the_cache() {
        let manifest = helper_graph();
        let policy = evaluate(&manifest, &on(&[]), &[], &[]);
        assert_eq!(
            cacheable(&manifest, &policy),
            ["h", "m", "r", "leaf", "app"]
        );
        assert!(policy.always_compile().is_empty());
        assert_eq!(policy.describe(&manifest, id(&manifest, "app")), None);
    }

    #[test]
    fn cache_off_bypasses_every_namespace_but_keeps_the_always_set() {
        let manifest = helper_graph();
        let flags = metadata(&manifest, &[("m", ALWAYS)]);
        let policy = evaluate(&manifest, &CacheSettings::default(), &flags, &[]);
        assert!(cacheable(&manifest, &policy).is_empty());
        for record in &manifest.namespaces {
            assert_eq!(
                policy.reason(id(&manifest, &record.name)),
                Some(Bypass::CacheOff)
            );
        }
        assert_eq!(
            policy.describe(&manifest, id(&manifest, "leaf")).unwrap(),
            "cache off for the build"
        );
        // Watch still recompiles `^:dev/always` and its consumers.
        assert_eq!(names(&manifest, policy.always_compile()), ["m", "app"]);
        let app = id(&manifest, "app");
        assert_eq!(
            policy.reasons(app).collect::<Vec<_>>(),
            [Bypass::CacheOff, Bypass::AlwaysCompile]
        );
    }

    #[test]
    fn no_cache_covers_only_its_own_namespace() {
        let manifest = helper_graph();
        let flags = metadata(&manifest, &[("h", NO_CACHE)]);
        let policy = evaluate(&manifest, &on(&[]), &flags, &[]);
        assert_eq!(cacheable(&manifest, &policy), ["m", "r", "leaf", "app"]);
        let h = id(&manifest, "h");
        assert_eq!(policy.reason(h), Some(Bypass::NoCache));
        assert!(policy.path(h, Bypass::NoCache).is_empty());
        assert_eq!(
            policy.describe(&manifest, h).unwrap(),
            "^:light-cljs/no-cache"
        );
        assert!(policy.always_compile().is_empty());
    }

    #[test]
    fn a_blocker_reaches_consumers_through_a_macro_provider_and_a_helper_only_edge() {
        let manifest = helper_graph();
        let app = id(&manifest, "app");
        // `app` reaches `h` through no require, only the helper edge.
        assert_eq!(
            manifest.edge(app, id(&manifest, "h")),
            Some(EdgeKinds::HELPER)
        );
        let policy = evaluate(&manifest, &on(&["h"]), &[], &[]);
        assert_eq!(cacheable(&manifest, &policy), ["r", "leaf"]);
        assert_eq!(policy.reason(app), Some(Bypass::Blocked));
        // A shortest path: the helper edge, not `h -> m -> app`.
        assert_eq!(
            names(&manifest, &policy.path(app, Bypass::Blocked)),
            ["h", "app"]
        );
        assert_eq!(
            names(&manifest, &policy.path(id(&manifest, "m"), Bypass::Blocked)),
            ["h", "m"]
        );
        assert_eq!(
            policy.describe(&manifest, app).unwrap(),
            "cache blocker through h -> app"
        );
        // The blocker itself is blocked, by itself.
        assert_eq!(
            names(&manifest, &policy.path(id(&manifest, "h"), Bypass::Blocked)),
            ["h"]
        );

        // Blocking the macro provider blocks its consumer, not its helper.
        let policy = evaluate(&manifest, &on(&["m"]), &[], &[]);
        assert_eq!(cacheable(&manifest, &policy), ["h", "r", "leaf"]);
        assert_eq!(
            manifest.edge(app, id(&manifest, "m")),
            Some(EdgeKinds::RUNTIME | EdgeKinds::MACRO_PROVIDER)
        );
        assert_eq!(
            names(&manifest, &policy.path(app, Bypass::Blocked)),
            ["m", "app"]
        );
        assert!(policy.always_compile().is_empty());
    }

    #[test]
    fn the_reach_before_analysis_is_the_blocked_set_without_observed_or_coarse_edges() {
        // Without observed or coarse edges, the require closure the
        // affinity of `--jobs N` follows before each namespace is analyzed
        // is the set `evaluate` blocks after the build, helper edges
        // included; `^:light-cljs/no-cache` on the blocker changes neither.
        let helper: &[(&str, &[&str])] = &[
            ("h", &[]),
            ("m", &["h"]),
            ("r", &[]),
            ("leaf", &[]),
            ("app", &["m", "r"]),
        ];
        let chain: &[(&str, &[&str])] = &[("a", &[]), ("b", &["a"]), ("c", &["b"]), ("d", &[])];
        for (sources, macros) in [(helper, &["m"][..]), (chain, &[][..])] {
            let manifest = manifest(sources, macros);
            for (blocker, _) in sources {
                let settings = on(&[blocker]);
                let flags = metadata(&manifest, &[(blocker, NO_CACHE)]);
                let policy = evaluate(&manifest, &settings, &flags, &[]);
                let mut reach = BlockerReach::new(&settings.blockers);
                for (name, requires) in sources {
                    let requires: Vec<String> = requires.iter().map(|r| (*r).to_owned()).collect();
                    let blocked = policy
                        .reasons(id(&manifest, name))
                        .any(|reason| reason == Bypass::Blocked);
                    assert_eq!(
                        reach.admit(name, &requires, &[], []),
                        blocked,
                        "{blocker}: {name}"
                    );
                }
            }
            let mut reach = BlockerReach::new(&[]);
            assert!(
                sources
                    .iter()
                    .all(|(name, _)| !reach.admit(name, &[], &[], [*name]))
            );
        }
    }

    #[test]
    fn the_reach_follows_names_of_reached_namespaces_outside_the_requires() {
        let mut reach = BlockerReach::new(&["m".to_owned()]);
        assert!(reach.admit("m", &[], &[], []));
        // Names that only resemble the blocker reach nothing.
        assert!(!reach.admit("w", &[], &[], ["mm/f", "other/m", "/", "clojure.core//"]));
        // A fully qualified macro, an alias-only alias and a reflected name.
        assert!(reach.admit("q", &[], &[], ["inc", "m/next-value"]));
        let alias = [("a".to_owned(), "m".to_owned())];
        assert!(reach.admit("aliased", &[], &alias, ["a/f"]));
        assert!(reach.admit("reflects", &[], &[], ["ns-publics", "m"]));
        // Consumers of a namespace reached that way are reached too.
        assert!(reach.admit("later", &["q".to_owned()], &[], []));
        assert!(!reach.admit("w2", &["w".to_owned()], &[], []));
    }

    #[test]
    fn a_blocker_propagates_transitively_and_unknown_blockers_block_nothing() {
        let manifest = manifest(&[("a", &[]), ("b", &["a"]), ("c", &["b"]), ("d", &[])], &[]);
        let policy = evaluate(&manifest, &on(&["a", "missing"]), &[], &[]);
        assert_eq!(cacheable(&manifest, &policy), ["d"]);
        assert_eq!(
            names(&manifest, &policy.path(id(&manifest, "c"), Bypass::Blocked)),
            ["a", "b", "c"]
        );
        assert_eq!(
            policy.describe(&manifest, id(&manifest, "c")).unwrap(),
            "cache blocker through a -> b -> c"
        );
    }

    #[test]
    fn always_on_a_provider_marks_its_consumers_and_the_always_set() {
        let manifest = helper_graph();
        let flags = metadata(&manifest, &[("h", ALWAYS)]);
        let policy = evaluate(&manifest, &on(&[]), &flags, &[]);
        assert_eq!(cacheable(&manifest, &policy), ["r", "leaf"]);
        assert_eq!(names(&manifest, policy.always_compile()), ["h", "m", "app"]);
        let app = id(&manifest, "app");
        assert_eq!(policy.reason(app), Some(Bypass::AlwaysCompile));
        assert_eq!(
            names(&manifest, &policy.path(app, Bypass::AlwaysCompile)),
            ["h", "app"]
        );
        assert_eq!(
            policy.describe(&manifest, app).unwrap(),
            "^:dev/always through h -> app"
        );
        // Always on a consumer marks nothing it depends on.
        let flags = metadata(&manifest, &[("app", ALWAYS)]);
        let policy = evaluate(&manifest, &on(&[]), &flags, &[]);
        assert_eq!(cacheable(&manifest, &policy), ["h", "m", "r", "leaf"]);
        assert_eq!(names(&manifest, policy.always_compile()), ["app"]);
    }

    #[test]
    fn a_non_cacheable_input_bypasses_its_namespace_and_consumers() {
        let manifest = helper_graph();
        let inputs = [NonCacheableInput {
            namespace: id(&manifest, "m"),
            description: "resource icons.svg from a provider without identity".to_owned(),
        }];
        let policy = evaluate(&manifest, &on(&[]), &[], &inputs);
        assert_eq!(cacheable(&manifest, &policy), ["h", "r", "leaf"]);
        let app = id(&manifest, "app");
        assert_eq!(policy.reason(app), Some(Bypass::NonCacheableInput));
        assert_eq!(
            policy.describe(&manifest, app).unwrap(),
            "input without a stable identity (resource icons.svg from a provider without \
             identity) through m -> app"
        );
        assert!(policy.always_compile().is_empty());
    }

    #[test]
    fn every_applicable_reason_is_kept_and_the_first_reported() {
        let manifest = helper_graph();
        let flags = metadata(&manifest, &[("app", NO_CACHE), ("h", ALWAYS)]);
        let inputs = [NonCacheableInput {
            namespace: id(&manifest, "r"),
            description: "provider token".to_owned(),
        }];
        let policy = evaluate(&manifest, &on(&["m"]), &flags, &inputs);
        let app = id(&manifest, "app");
        assert_eq!(
            policy.reasons(app).collect::<Vec<_>>(),
            [
                Bypass::NoCache,
                Bypass::AlwaysCompile,
                Bypass::Blocked,
                Bypass::NonCacheableInput
            ]
        );
        assert_eq!(policy.reason(app), Some(Bypass::NoCache));
        assert!(policy.has(app, Bypass::Blocked));
        assert!(!policy.has(app, Bypass::CacheOff));
        assert_eq!(cacheable(&manifest, &policy), ["leaf"]);
    }

    #[test]
    fn a_policy_change_takes_effect_on_the_same_manifest() {
        let manifest = helper_graph();
        let app = id(&manifest, "app");
        let before = evaluate(&manifest, &on(&[]), &[], &[]);
        assert!(before.may_use_cache(app));
        let blocked = evaluate(&manifest, &on(&["r"]), &[], &[]);
        assert_eq!(blocked.reason(app), Some(Bypass::Blocked));
        let flags = metadata(&manifest, &[("r", ALWAYS)]);
        let always = evaluate(&manifest, &on(&[]), &flags, &[]);
        assert_eq!(always.reason(app), Some(Bypass::AlwaysCompile));
        assert_eq!(names(&manifest, always.always_compile()), ["r", "app"]);
        // Removing the setting and the annotation restores the decision.
        assert_eq!(evaluate(&manifest, &on(&[]), &[], &[]), before);
    }

    #[test]
    fn lazy_edges_do_not_propagate() {
        let mut loader = LoaderRecord::default();
        for (name, requires) in [("app", vec![]), ("page", vec!["app"])] {
            loader.sources.push(LoadedSource {
                name: name.to_owned(),
                file: format!("{name}.cljs"),
                kind: SourceKind::Cljs,
                origin: SourceOrigin::Provider,
                lookup: LookupId(0),
                requires: requires
                    .into_iter()
                    .map(|target: &str| {
                        (
                            target.to_owned(),
                            target.to_owned(),
                            RequireForm::Declared,
                            Vec::new(),
                        )
                    })
                    .collect(),
            });
        }
        // `app` refers lazily to `page`, which loads after it.
        loader.lazy.push((0, "page".to_owned()));
        let program_namespaces = ["app", "page"].into_iter().map(str::to_owned).collect();
        let manifest = crate::dependency_manifest::build(
            loader,
            ObservationLog::default(),
            &program_namespaces,
            |_| false,
        );
        let policy = evaluate(&manifest, &on(&["page"]), &[], &[]);
        assert_eq!(cacheable(&manifest, &policy), ["app"]);
    }
}
