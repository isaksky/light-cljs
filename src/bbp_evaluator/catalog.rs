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
//! costs what it reads rather than the whole catalog.
use super::*;
use crate::compilation_session::Registration;

#[derive(Default)]
pub(super) struct Registry {
    // Tasks own their frontiers. The cache shares live immutable data without
    // rooting every historical prefix (including its copied vector spine).
    versions: BTreeMap<usize, WeakHandle>,
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
    pub(crate) fn install_catalog(
        &self,
        first: usize,
        registrations: &[Registration],
    ) -> Result<(), EvalError> {
        if registrations.is_empty() {
            return Ok(());
        }
        let end = first + registrations.len();
        let snapshot = {
            let mut registry = self.shared.0.catalog.lock().unwrap();
            let collected = self.runtime.heap_stats().collections;
            if registry.collected != collected {
                registry.versions.retain(|_, snapshot| snapshot.is_live());
                registry.collected = collected;
            }
            if let Some(snapshot) = registry.versions.get(&end).and_then(|s| s.upgrade().ok()) {
                snapshot
            } else {
                // Reuse the greatest live prefix, even when another task
                // installed it. Weak cache entries must not retain history.
                let cached = registry
                    .versions
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
                            self.cx()?.catalog_snapshot.borrow().clone()
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
                            Registration::Root(name, unit, _, once, _) => {
                                let name = text(s, name)?;
                                names = index(names, name, position)?;
                                s.new_vector(&[
                                    number(0),
                                    name,
                                    unit.map_or_else(|| s.primitive(Primitive::Null), number),
                                    boolean(*once),
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
                registry.versions.insert(end, snapshot.downgrade());
                snapshot
            }
        };
        self.use_catalog(snapshot, first, end, false)?;
        self.cx()?.catalog.set(Some(end));
        Ok(())
    }

    /// Apply exactly the newly registered suffix. A producer starts at zero
    /// with a fresh ordinary object and the very same immutable data root,
    /// which it adopts (`lazy`) rather than applies: each name and protocol
    /// gets its registrations when the task first uses it, with the same
    /// result as applying them all here.
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
            c.catalog_nanos
                .fetch_add(started.elapsed().as_nanos() as usize, Ordering::Relaxed);
        }
        Ok(())
    }

    /// Charge the catalog entries this producer's task applied on first use
    /// of a name or protocol (`__lightCatalogApplied`) to the producer and
    /// catalog counters, once its work is done.
    pub(super) fn note_catalog_applied(&self) {
        if !crate::compile_trace::any_recording() {
            return;
        }
        let Ok(cx) = self.cx() else { return };
        let Ok(Some(applied)) = cx.task.global_value("__lightCatalogApplied") else {
            return;
        };
        if let ValueView::Number(applied) = applied.view() {
            let c = &self.shared.0.counters;
            c.producer_catalog_entries
                .fetch_add(applied as usize, Ordering::Relaxed);
            c.catalog_entries
                .fetch_add(applied as usize, Ordering::Relaxed);
        }
    }
}
