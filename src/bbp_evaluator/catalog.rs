//! Shared catalog data, with ordinary task-owned JavaScript registrations.
//!
//! Versions are persistent vectors of registration descriptors. They contain
//! only immutable data, never task objects, initialized values or getters.
//! Tasks apply descriptors in source order through the same registration
//! functions as individual registrations; nothing is coalesced or inferred
//! from a property descriptor. Producers import data rather than compile and
//! evaluate their parent's registration sources.
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
                // A task already roots the prefix it has installed. It can
                // extend that data even after the cache's other versions die.
                let previous = if first == 0 {
                    None
                } else {
                    self.cx()?.catalog_snapshot.borrow().clone()
                };
                if first != 0 && previous.is_none() {
                    return Err(error("missing native catalog frontier"));
                }
                for registration in registrations {
                    self.control.poll()?;
                    self.define_root(match registration {
                        Registration::Root(_, unit, ..) => *unit,
                        Registration::Bundle(bundle) => Some(bundle.unit),
                    })?;
                }
                let snapshot = self.host_scope(|s| {
                    let mut entries = match &previous {
                        Some(previous) => s.local(previous).map_err(error)?,
                        None => s.new_vector(&[]).map_err(error)?,
                    };
                    for registration in registrations {
                        self.control.poll()?;
                        let number = |n: usize| s.primitive(Primitive::Number(n as f64));
                        let boolean = |b| s.primitive(Primitive::Bool(b));
                        let descriptor = match registration {
                            Registration::Root(name, unit, _, once, _) => s
                                .new_vector(&[
                                    number(0),
                                    text(s, name)?,
                                    unit.map_or_else(|| s.primitive(Primitive::Null), number),
                                    boolean(*once),
                                ])
                                .map_err(error)?,
                            Registration::Bundle(bundle) => {
                                let names = bundle
                                    .products
                                    .iter()
                                    .map(|name| text(s, name))
                                    .collect::<Result<Vec<_>, _>>()?;
                                // Native declaration dispatch uses only the protocol
                                // from each extension index, just like decl.register.
                                let indices = bundle
                                    .extensions
                                    .iter()
                                    .map(|(protocol, _, _)| {
                                        s.new_vector(&[text(s, protocol)?]).map_err(error)
                                    })
                                    .collect::<Result<Vec<_>, EvalError>>()?;
                                s.new_vector(&[
                                    number(1),
                                    s.new_vector(&names).map_err(error)?,
                                    s.new_vector(&indices).map_err(error)?,
                                    text(s, &bundle.location)?,
                                    boolean(bundle.once),
                                    number(bundle.unit),
                                ])
                                .map_err(error)?
                            }
                        };
                        entries = s.conj(entries, descriptor).map_err(error)?;
                    }
                    Ok::<_, EvalError>(s.persist(entries))
                })?;
                self.runtime.publish(&snapshot).map_err(error)?;
                registry.versions.insert(end, snapshot.downgrade());
                snapshot
            }
        };
        self.use_catalog(snapshot, first)?;
        self.cx()?.catalog.set(Some(end));
        Ok(())
    }

    /// Apply exactly the newly registered suffix. A producer starts at zero
    /// with a fresh ordinary object and the very same immutable data root.
    pub(super) fn use_catalog(&self, snapshot: Rooted, first: usize) -> Result<(), EvalError> {
        let cx = self.cx()?;
        let payload = self.host_scope(|s| {
            let snapshot = s.local(&snapshot).map_err(error)?;
            let first = s.primitive(Primitive::Number(first as f64));
            Ok::<_, EvalError>(s.persist(s.new_vector(&[snapshot, first]).map_err(error)?))
        })?;
        // request roots its message until the next command boundary and
        // completes the command cell weakly. It cannot retain this snapshot
        // after the producer task and its context have been dropped.
        self.request(4, self.id()?, payload)?;
        *cx.catalog_snapshot.borrow_mut() = Some(snapshot);
        Ok(())
    }
}
