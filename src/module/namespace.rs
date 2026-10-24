//! Module namespace exotic objects. Export properties retain binding cells,
//! never snapshots or JavaScript getters. All operations are task-owned.

use std::collections::BTreeMap;

use crate::TaskId;
use crate::heap::gc::{Trace, TraceEdge};
use crate::heap::{Heap, HeapObject, ObjectId};
use crate::object::{Property, PropertyDescriptor, PropertyFlags};
use crate::value::{JsString, Value};
use crate::vm::dispatch::Fault;
use crate::vm::keys::{KeyFilter, OwnKey};
use crate::vm::property::Key;

#[derive(Debug)]
pub(crate) struct Namespace {
    pub owner: TaskId,
    // JsString's Ord compares UTF-16 code units, including numeric names.
    pub exports: BTreeMap<JsString, ObjectId>,
}

impl Namespace {
    pub fn check_owner(&self, task: TaskId) -> Result<(), Fault> {
        if self.owner == task {
            Ok(())
        } else {
            Err(Fault::ForeignObject {
                owner: Some(self.owner),
                caller: task,
            })
        }
    }
}

impl Trace for Namespace {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        for cell in self.exports.values() {
            visit(TraceEdge::Object(*cell));
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Entry {
    Cell(ObjectId),
    Tag,
}

/// Outer None is a different heap kind; inner None is an absent own key.
/// No cell read here: HasProperty and Delete must work during the TDZ.
pub(crate) fn probe(
    heap: &Heap,
    task: TaskId,
    id: ObjectId,
    key: &Key,
) -> Result<Option<Option<Entry>>, Fault> {
    heap.read(id, |o| {
        let HeapObject::ModuleNamespace(ns) = o else {
            return Ok(None);
        };
        ns.check_owner(task)?;
        let entry = match key {
            Key::Name(name) => ns.exports.get(name).copied().map(Entry::Cell),
            Key::Index(i) => ns
                .exports
                .get(&JsString::from(i.to_string().as_str()))
                .copied()
                .map(Entry::Cell),
            Key::Symbol(symbol) if *symbol == crate::value::symbol::well_known("toStringTag") => {
                Some(Entry::Tag)
            }
            Key::Symbol(_) => None,
        };
        Ok(Some(entry))
    })
    .unwrap_or(Err(Fault::Internal("namespace was reclaimed")))
}

pub(crate) fn property(heap: &Heap, task: TaskId, entry: Entry) -> Result<Property, Fault> {
    let (value, flags) = match entry {
        Entry::Tag => (Value::String("Module".into()), PropertyFlags::FIXED),
        Entry::Cell(id) => {
            let value = heap
                .read(id, |o| match o {
                    HeapObject::Cell(cell) if cell.owner != task => Err(Fault::ForeignObject {
                        owner: Some(cell.owner),
                        caller: task,
                    }),
                    HeapObject::Cell(cell) => cell
                        .value
                        .clone()
                        .ok_or(Fault::Throw(crate::vm::ThrowKind::Uninitialized)),
                    _ => Err(Fault::Internal("namespace export is not a binding cell")),
                })
                .unwrap_or(Err(Fault::Internal("namespace cell was reclaimed")))?;
            (
                value,
                PropertyFlags {
                    writable: true,
                    enumerable: true,
                    configurable: false,
                },
            )
        }
    };
    Ok(Property {
        value,
        flags,
        accessor: None,
    })
}

pub(crate) fn define(
    heap: &Heap,
    task: TaskId,
    entry: Option<Entry>,
    desc: &PropertyDescriptor,
) -> Result<(), Fault> {
    let rejected = || Fault::TypeError("cannot redefine module namespace property".into());
    let Some(entry) = entry else {
        return Err(rejected());
    };
    // [[GetOwnProperty]] reads the cell even for a descriptor with no value.
    let current = property(heap, task, entry)?;
    if matches!(entry, Entry::Tag) {
        return crate::object::validate_and_apply(Some(&current), desc)
            .map(|_| ())
            .map_err(|_| rejected());
    }
    if desc.configurable == Some(true)
        || desc.enumerable == Some(false)
        || desc.is_accessor()
        || desc.writable == Some(false)
        || desc
            .value
            .as_ref()
            .is_some_and(|v| !crate::atom::same_value(v, &current.value))
    {
        return Err(rejected());
    }
    Ok(())
}

pub(crate) fn keys(
    heap: &Heap,
    task: TaskId,
    id: ObjectId,
    filter: KeyFilter,
) -> Result<Option<Vec<OwnKey>>, Fault> {
    let entries = heap
        .read(id, |o| {
            let HeapObject::ModuleNamespace(ns) = o else {
                return Ok(None);
            };
            ns.check_owner(task)?;
            Ok(Some(
                ns.exports
                    .iter()
                    .map(|(name, cell)| (name.clone(), *cell))
                    .collect::<Vec<_>>(),
            ))
        })
        .unwrap_or(Err(Fault::Internal("namespace was reclaimed")))?;
    let Some(entries) = entries else {
        return Ok(None);
    };
    let mut keys = Vec::with_capacity(entries.len() + 1);
    for (name, cell) in entries {
        if filter == KeyFilter::Enumerable {
            property(heap, task, Entry::Cell(cell))?;
        }
        keys.push(OwnKey {
            key: Key::from_name(&name),
            enumerable: true,
        });
    }
    if filter == KeyFilter::All {
        keys.push(OwnKey {
            key: Key::Symbol(crate::value::symbol::well_known("toStringTag")),
            enumerable: false,
        });
    }
    Ok(Some(keys))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::Rooted;
    use crate::vm::function::BindingCell;

    #[test]
    fn namespace_get_in_tdz_throws_reference_error() {
        // Internal fixture: cyclic graph execution is a separate pellet.
        // Supply an actual namespace with an uninitialized exported cell to
        // source code, so this checks catchable VM errors, not just Faults.
        let rt = crate::Runtime::new();
        let owner = TaskId::from_raw(1);
        let heap = rt.heap();
        let cell = heap
            .alloc(HeapObject::Cell(BindingCell { owner, value: None }))
            .unwrap();
        let ns = heap
            .alloc(HeapObject::ModuleNamespace(Box::new(Namespace {
                owner,
                exports: BTreeMap::from([("x".into(), cell)]),
            })))
            .unwrap();
        let root = Rooted::new(heap, Value::Object(ns));
        let code = crate::frontend::compile_function("function test(m) { let errors=0; if (!('x' in m)) return false; try { m.x; } catch(e) { if(e instanceof ReferenceError) errors++; } try { Object.getOwnPropertyDescriptor(m,'x'); } catch(e) { if(e instanceof ReferenceError) errors++; } try { Object.keys(m); } catch(e) { if(e instanceof ReferenceError) errors++; } try { Object.defineProperty(m,'x',{}); } catch(e) { if(e instanceof ReferenceError) errors++; } return errors===4 && Object.getOwnPropertyNames(m)[0]==='x'; }").unwrap();
        let task = rt.spawn(&code, &[&root]).unwrap();
        let outcome = task.run().unwrap();
        assert!(
            matches!(outcome, crate::RunOutcome::Complete(ref v) if v.view() == crate::ValueView::Bool(true)),
            "{outcome:?}"
        );
    }

    #[test]
    fn namespace_cells_retain_and_reclaim() {
        let heap = std::sync::Arc::new(Heap::default());
        let owner = TaskId::from_raw(1);
        let value = heap.alloc(HeapObject::host_object()).unwrap();
        let cell = heap
            .alloc(HeapObject::Cell(BindingCell {
                owner,
                value: Some(Value::Object(value)),
            }))
            .unwrap();
        let ns = heap
            .alloc(HeapObject::ModuleNamespace(Box::new(Namespace {
                owner,
                exports: BTreeMap::from([("value".into(), cell)]),
            })))
            .unwrap();
        let root = Rooted::new(&heap, Value::Object(ns));
        assert_eq!(heap.collect().unwrap().live, 3);
        assert!(heap.is_live(cell) && heap.is_live(value));
        assert!(crate::value::share::check(&heap, &Value::Object(ns)).is_err());
        drop(root);
        assert_eq!(heap.collect().unwrap().reclaimed, 3);
    }

    #[test]
    fn namespace_capture_cannot_publish() {
        let rt = crate::Runtime::new();
        let ns = rt
            .heap()
            .alloc(HeapObject::ModuleNamespace(Box::new(Namespace {
                owner: TaskId::from_raw(1),
                exports: BTreeMap::new(),
            })))
            .unwrap();
        let root = Rooted::new(rt.heap(), Value::Object(ns));
        assert!(rt.publish(&root).is_err());
        let code =
            crate::frontend::compile_function("function capture(m) { return () => m; }").unwrap();
        let task = rt.spawn(&code, &[&root]).unwrap();
        let crate::RunOutcome::Complete(closure) = task.run().unwrap() else {
            panic!("capture failed");
        };
        assert!(rt.publish(&closure).is_err());
    }

    #[test]
    fn namespace_own_keys_and_ownership() {
        let heap = Heap::default();
        let owner = TaskId::from_raw(1);
        let cell = heap
            .alloc(HeapObject::Cell(BindingCell {
                owner,
                value: Some(Value::Undefined),
            }))
            .unwrap();
        let ns = heap
            .alloc(HeapObject::ModuleNamespace(Box::new(Namespace {
                owner,
                exports: ["2", "10", "𐀀", "ﬀ"].map(|s| (s.into(), cell)).into(),
            })))
            .unwrap();
        let keys = keys(&heap, owner, ns, KeyFilter::All)
            .unwrap_or_else(|_| panic!("keys failed"))
            .unwrap();
        assert_eq!(
            keys.iter().take(4).map(OwnKey::name).collect::<Vec<_>>(),
            ["10", "2", "𐀀", "ﬀ"]
        );
        assert!(
            matches!(&keys[4].key, Key::Symbol(s) if *s == crate::value::symbol::well_known("toStringTag"))
        );
        assert!(!keys[4].enumerable);
        let other = TaskId::from_raw(2);
        assert!(matches!(
            probe(&heap, other, ns, &Key::Name("2".into())),
            Err(Fault::ForeignObject { .. })
        ));
        assert!(keys_foreign(&heap, other, ns));
        assert!(crate::vm::prototype::get_prototype_of(&heap, other, ns).is_err());
        assert!(crate::vm::integrity::is_extensible(&heap, other, &Value::Object(ns)).is_err());
    }

    fn keys_foreign(heap: &Heap, task: TaskId, ns: ObjectId) -> bool {
        matches!(
            keys(heap, task, ns, KeyFilter::All),
            Err(Fault::ForeignObject { .. })
        )
    }
}
