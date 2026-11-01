//! Shared catalog data, with ordinary task-owned JavaScript registrations.
//!
//! A version is a persistent vector `[entries, names, protocols, bundles]`:
//! the registration descriptors in source order, a map from each name to
//! the positions of the descriptors that register it, a map from each
//! protocol to the positions of the bundles that extend it, and how many
//! bundles `entries` holds (a bundle's descriptor records how many precede
//! it). They contain only immutable data, never task objects, initialized
//! values or getters. Tasks apply descriptors in source order through the
//! same registration functions as individual registrations; nothing is
//! coalesced or inferred from a property descriptor. Producers import data
//! rather than compile and evaluate their parent's registration sources, and
//! apply a name's or a protocol's descriptors only when they first use it
//! (light-cljs-590; `lazyAdopt` in src/bbp_runtime.js), so setting one up
//! costs what it reads rather than the whole catalog. Every other task (the
//! coordinator's and each pool worker's) adopts the catalog the same way and
//! extends its adoption by each later version (light-cljs-675; `lazyExtend`),
//! so a task's registration state follows the definitions it reads, not the
//! program, and adding threads does not copy the program's registrations.
use super::*;
use crate::compilation_session::Registration;

/// A live immutable prefix for fresh namespace tasks. Keeping this one root
/// alive lets every task fork the same data without rebuilding descriptors.
#[derive(Clone)]
pub(crate) struct CatalogSeed {
    cursor: usize,
    snapshot: Option<Rooted>,
}
impl CatalogSeed {
    pub(crate) fn len(&self) -> usize { self.cursor }
}

#[derive(Default)]
pub(super) struct Registry {
    // Tasks own their frontiers. The cache shares live immutable data without
    // rooting every historical prefix (including its copied vector spine).
    versions: BTreeMap<usize, WeakHandle>,
    // One registry per append-only journal. Tasks on that journal share this
    // cache and its build lock. Forked namespace journals own separate caches:
    // constructing their immutable suffixes must not serialize each other.
    collected: u64,
}

fn text<'s>(s: &Scope<'s>, value: &str) -> Result<Local<'s>, EvalError> {
    s.try_primitive(Primitive::string(value))
        .ok_or_else(|| error("heap byte or string limit exceeded"))
}

/// `map` with `position` appended to the positions it holds for `key`.
fn index<'s>(
    s: &Scope<'s>,
    map: Local<'s>,
    key: Local<'s>,
    position: usize,
) -> Result<Local<'s>, EvalError> {
    let positions = match s.get(map, key).map_err(error)? {
        Some(positions) => positions,
        None => s.new_vector(&[]).map_err(error)?,
    };
    let position = s.primitive(Primitive::Number(position as f64));
    let positions = s.conj(positions, position).map_err(error)?;
    s.assoc(map, key, positions).map_err(error)
}

impl BbpEvaluator {
    #[cfg(test)]
    pub(super) fn catalog_versions(&self) -> usize {
        self.cx().unwrap().catalog_branch.borrow().lock().unwrap().versions.len()
    }
    pub(crate) fn catalog_seed(&self) -> Result<CatalogSeed, EvalError> {
        let cx = self.cx()?;
        Ok(CatalogSeed {
            cursor: cx.catalog.get().unwrap_or(0),
            snapshot: cx.catalog_snapshot.borrow().clone(),
        })
    }

    pub(crate) fn fork_catalog_from(&self, seed: &CatalogSeed) -> Result<(), EvalError> {
        if self.cx()?.catalog.get().unwrap_or(0) != 0 {
            return Err(error("a seeded namespace task must have an empty catalog"));
        }
        if let Some(snapshot) = &seed.snapshot {
            self.host_scope(|scope| scope.local(snapshot).map(|_| ()).map_err(error))?;
        }
        self.new_catalog_branch(seed)
    }

    pub(crate) fn adopt_catalog_seed(&self, seed: &CatalogSeed) -> Result<(), EvalError> {
        self.fork_catalog_from(seed)?;
        if let Some(snapshot) = &seed.snapshot {
            self.use_catalog(snapshot.clone(), 0, seed.cursor, true)?;
        }
        self.cx()?.catalog.set(Some(seed.cursor));
        Ok(())
    }

    fn new_catalog_branch(&self, seed: &CatalogSeed) -> Result<(), EvalError> {
        if seed.cursor != 0 && seed.snapshot.is_none() {
            return Err(error("missing native catalog frontier"));
        }
        let mut registry = Registry::default();
        if let Some(snapshot) = &seed.snapshot {
            registry.versions.insert(seed.cursor, snapshot.downgrade());
        }
        *self.cx()?.catalog_branch.borrow_mut() = Arc::new(Mutex::new(registry));
        Ok(())
    }
    /// Start an independent registration suffix while sharing this task's
    /// current immutable prefix. Namespace publication can then use canonical
    /// order without aliasing an arrival-ordered catalog of the same length.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn fork_catalog(&self) -> Result<(), EvalError> {
        self.new_catalog_branch(&self.catalog_seed()?)
    }

    /// Give this task the catalog's registrations from `first`: it adopts
    /// them (light-cljs-675), applying a name's or a protocol's on first
    /// use, so a task's registration state follows what it reads rather
    /// than the whole program, whichever thread it runs on.
    pub(crate) fn install_catalog(
        &self,
        first: usize,
        registrations: &[Registration],
    ) -> Result<(), EvalError> {
        self.install_catalog_as(first, registrations, true)
    }

    /// [`Self::install_catalog`], applying every registration now unless
    /// `lazy`, as tasks did before light-cljs-675: the reference that
    /// adopted catalogs are compared with.
    pub(super) fn install_catalog_as(
        &self,
        first: usize,
        registrations: &[Registration],
        lazy: bool,
    ) -> Result<(), EvalError> {
        if registrations.is_empty() {
            return Ok(());
        }
        let cx = self.cx()?;
        let branch = cx.catalog_branch.borrow().clone();
        let end = first + registrations.len();
        let snapshot = {
            let mut registry = branch.lock().unwrap();
            // Cache expiry needs only the collection epoch, not a heap census
            // that also locks the shared root and object tables on every append.
            let collected = self.runtime.collection_count();
            if registry.collected != collected {
                registry.versions.retain(|_, snapshot| snapshot.is_live());
                registry.collected = collected;
            }
            let versions = &mut registry.versions;
            if let Some(snapshot) = versions.get(&end).and_then(|s| s.upgrade().ok()) {
                snapshot
            } else {
                // Reuse the greatest live prefix, even when another task
                // installed it. Weak cache entries must not retain history.
                let cached = versions
                    .range(first..end)
                    .rev()
                    .find_map(|(&cursor, weak)| Some((cursor, weak.upgrade().ok()?)));
                let (cursor, previous) = match cached {
                    Some((cursor, snapshot)) => (cursor, Some(snapshot)),
                    None => {
                        // A task still owns its installed prefix after the
                        // cache's other versions have been collected.
                        let previous = if first == 0 {
                            None
                        } else {
                            cx.catalog_snapshot.borrow().clone()
                        };
                        if first != 0 && previous.is_none() {
                            return Err(error("missing native catalog frontier"));
                        }
                        (first, previous)
                    }
                };
                let registrations = &registrations[cursor - first..];
                for registration in registrations {
                    self.control.poll()?;
                    self.define_root(match registration {
                        Registration::Root(_, unit, ..) => *unit,
                        Registration::Bundle(bundle) => Some(bundle.unit),
                    })?;
                }
                let snapshot = self.host_scope(|s| {
                    let number = |n: usize| s.primitive(Primitive::Number(n as f64));
                    let (mut entries, mut names, mut protocols, mut bundles) = match &previous {
                        Some(previous) => {
                            let previous = s.local(previous).map_err(error)?;
                            let part = |i| s.nth(previous, i).map_err(error);
                            let ValueView::Number(bundles) = s.view(part(3)?) else {
                                return Err(error("invalid native catalog version"));
                            };
                            (part(0)?, part(1)?, part(2)?, bundles as usize)
                        }
                        None => (
                            s.new_vector(&[]).map_err(error)?,
                            s.new_map(&[]).map_err(error)?,
                            s.new_map(&[]).map_err(error)?,
                            0,
                        ),
                    };
                    let index = |map, key, position| index(s, map, key, position);
                    for (offset, registration) in registrations.iter().enumerate() {
                        self.control.poll()?;
                        let position = cursor + offset;
                        let boolean = |b| s.primitive(Primitive::Bool(b));
                        let descriptor = match registration {
                            Registration::Root(name, unit, test, once, location) => {
                                let name = text(s, name)?;
                                names = index(names, name, position)?;
                                s.new_vector(&[
                                    number(0),
                                    name,
                                    unit.map_or_else(|| s.primitive(Primitive::Null), number),
                                    boolean(*once),
                                    // The unit of the definition's :test
                                    // metadata, which Var reflection reads.
                                    test.map_or_else(|| s.primitive(Primitive::Null), number),
                                    text(s, location)?,
                                ])
                                .map_err(error)?
                            }
                            Registration::Bundle(bundle) => {
                                let mut seen = HashSet::new();
                                for name in &bundle.products {
                                    if seen.insert(name.as_str()) {
                                        names = index(names, text(s, name)?, position)?;
                                    }
                                }
                                let mut seen = HashSet::new();
                                for (protocol, ..) in &bundle.extensions {
                                    if seen.insert(protocol.as_str()) {
                                        protocols = index(protocols, text(s, protocol)?, position)?;
                                    }
                                }
                                let names = bundle
                                    .products
                                    .iter()
                                    .map(|name| text(s, name))
                                    .collect::<Result<Vec<_>, _>>()?;
                                // Native declaration dispatch uses the protocol and
                                // selector of each extension index (decl.register):
                                // an `Object` extension is installed per type.
                                let indices = bundle
                                    .extensions
                                    .iter()
                                    .map(|(protocol, selector, _)| {
                                        let selector = match selector {
                                            Some(selector) => text(s, selector)?,
                                            None => s.primitive(Primitive::Null),
                                        };
                                        s.new_vector(&[text(s, protocol)?, selector]).map_err(error)
                                    })
                                    .collect::<Result<Vec<_>, EvalError>>()?;
                                s.new_vector(&[
                                    number(1),
                                    s.new_vector(&names).map_err(error)?,
                                    s.new_vector(&indices).map_err(error)?,
                                    text(s, &bundle.location)?,
                                    boolean(bundle.once),
                                    number(bundle.unit),
                                    // The bundles before it: its declaration order.
                                    number({
                                        bundles += 1;
                                        bundles - 1
                                    }),
                                    boolean(bundle.group),
                                ])
                                .map_err(error)?
                            }
                        };
                        entries = s.conj(entries, descriptor).map_err(error)?;
                    }
                    let version = s
                        .new_vector(&[entries, names, protocols, number(bundles)])
                        .map_err(error)?;
                    Ok::<_, EvalError>(s.persist(version))
                })?;
                self.runtime.publish(&snapshot).map_err(error)?;
                versions.insert(end, snapshot.downgrade());
                snapshot
            }
        };
        self.use_catalog(snapshot, first, end, lazy)?;
        cx.catalog.set(Some(end));
        Ok(())
    }

    /// Apply exactly the newly registered suffix. A producer starts at zero
    /// with a fresh ordinary object and the very same immutable data root,
    /// which it adopts (`lazy`) rather than applies: each name and protocol
    /// gets its registrations when the task first uses it, with the same
    /// result as applying them all here. A task that adopted an earlier
    /// version extends it by the suffix (`lazyExtend` in src/bbp_runtime.js).
    pub(super) fn use_catalog(
        &self,
        snapshot: Rooted,
        first: usize,
        end: usize,
        lazy: bool,
    ) -> Result<(), EvalError> {
        let started = crate::compile_trace::any_recording().then(std::time::Instant::now);
        let cx = self.cx()?;
        let payload = self.host_scope(|s| {
            let snapshot = s.local(&snapshot).map_err(error)?;
            let first = s.primitive(Primitive::Number(first as f64));
            let lazy = s.primitive(Primitive::Bool(lazy));
            Ok::<_, EvalError>(s.persist(s.new_vector(&[snapshot, first, lazy]).map_err(error)?))
        })?;
        // request roots its message until the next command boundary and
        // completes the command cell weakly. It cannot retain this snapshot
        // after the producer task and its context have been dropped.
        self.request(4, self.id()?, payload)?;
        *cx.catalog_snapshot.borrow_mut() = Some(snapshot);
        if let Some(started) = started {
            let c = &self.shared.0.counters;
            c.catalog_uses.fetch_add(1, Ordering::Relaxed);
            if !lazy {
                c.catalog_entries
                    .fetch_add(end.saturating_sub(first), Ordering::Relaxed);
            }
            self.count_catalog_applied();
            c.catalog_nanos
                .fetch_add(started.elapsed().as_nanos() as usize, Ordering::Relaxed);
        }
        Ok(())
    }

    /// Charge the catalog entries this producer's task applied on first use
    /// of a name or protocol (`__lightCatalogApplied`) to the producer and
    /// catalog counters, once its work is done.
    pub(super) fn note_catalog_applied(&self) {
        if crate::compile_trace::any_recording() {
            self.count_catalog_applied();
        }
    }

    /// Charge the entries this task applied on first use since the last
    /// count to the catalog counters, and to the producer counter for a
    /// producer (the only evaluators nested in another's service).
    fn count_catalog_applied(&self) {
        let Ok(cx) = self.cx() else { return };
        let Ok(Some(applied)) = cx.task.global_value("__lightCatalogApplied") else {
            return;
        };
        if let ValueView::Number(applied) = applied.view() {
            let applied = applied as usize;
            let new = applied.saturating_sub(cx.catalog_applied.replace(applied));
            let c = &self.shared.0.counters;
            if self.service_depth > 0 {
                c.producer_catalog_entries.fetch_add(new, Ordering::Relaxed);
            }
            c.catalog_entries.fetch_add(new, Ordering::Relaxed);
        }
    }

    /// Record that this task's current activity registered the namespaces
    /// of `namespaces`, a program's in its order, as a legacy engine's reader
    /// registration creates them ([`super::namespaces`]): one entry for the
    /// part the registry's table starts with, which costs the names new to
    /// it, not the program (light-cljs-702). The task keeps its last table
    /// and how much of it the registry's table starts with, so a table that
    /// extends it is compared only past that; one it registered already, in
    /// the same work and no earlier position, records nothing.
    pub(super) fn record_namespaces(
        &self,
        namespaces: &crate::arena::Arena<String>,
    ) -> Result<(), EvalError> {
        let cx = self.cx()?;
        let mut tag = self
            .namespace_lanes()
            .map(|lanes| lanes.namespace_tag_here())
            .unwrap_or_default();
        // The shared lazy cells its task realizes: work an earlier reader
        // may wait for, or read once it has settled ([`super::namespaces`]).
        let realizing = self.runtime.attribute_realizing(cx.task.id());
        tag.owners
            .extend(realizing.into_iter().map(crate::effect_lanes::Target::Lazy));
        let (repeated, known) = {
            let (last, registered, known) = &*cx.namespaces.borrow();
            let shared = registered.len().min(namespaces.len());
            let same = (namespaces.changed_entries(registered, shared).first())
                .copied()
                .unwrap_or(shared);
            let repeated = same == namespaces.len()
                && same == registered.len()
                && last.owners == tag.owners
                && last.position <= tag.position;
            (repeated, same.min(*known))
        };
        if repeated {
            return Ok(());
        }
        let known = if namespaces.is_empty() {
            0
        } else {
            use crate::wait_timeline::{Kind, begin};
            let waiting = begin(Kind::NamespacesWait, 0, 0);
            let mut log = self.namespace_log().lock().unwrap();
            drop(waiting);
            let _held = begin(Kind::NamespacesHold, 0, 0);
            log.register(namespaces, known, &tag)
        };
        *cx.namespaces.borrow_mut() = (tag, namespaces.clone(), known);
        Ok(())
    }
}
