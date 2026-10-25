//! Immutable symbol primitives outside the traced heap, like `JsString`.
//! Identity is process-wide and never reused, including across runtimes.
//! The body has no heap edges; clones keep the description alive directly.

use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use super::JsString;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// One registry per runtime heap. The mutex linearizes lookup/insertion and
/// publishes immutable symbol bodies to other workers. No heap locks,
/// safepoints, JavaScript or host callbacks run while it is held.
#[derive(Debug, Default)]
pub(crate) struct SymbolRegistry(Mutex<RegistryEntries>);

#[derive(Debug, Default)]
struct RegistryEntries {
    by_key: HashMap<JsString, JsSymbol>,
    by_id: HashMap<u64, JsString>,
}

impl SymbolRegistry {
    /// The caller finishes ToString before entering this leaf operation.
    pub(crate) fn symbol_for(&self, key: JsString) -> JsSymbol {
        let mut entries = self.0.lock().unwrap();
        if let Some(symbol) = entries.by_key.get(&key) {
            return symbol.clone();
        }
        let symbol = JsSymbol::create(Some(key.clone()), true);
        entries.by_id.insert(symbol.identity(), key.clone());
        entries.by_key.insert(key, symbol.clone());
        symbol
    }

    pub(crate) fn key_for(&self, symbol: &JsSymbol) -> Option<JsString> {
        self.0
            .lock()
            .unwrap()
            .by_id
            .get(&symbol.identity())
            .cloned()
    }
}

/// Well-known identities are process-wide; every realm and task shares
/// them. These immutable bodies live until process exit.
pub(crate) fn well_known(name: &str) -> JsSymbol {
    static ITERATOR: OnceLock<JsSymbol> = OnceLock::new();
    static ASYNC_ITERATOR: OnceLock<JsSymbol> = OnceLock::new();
    static TO_PRIMITIVE: OnceLock<JsSymbol> = OnceLock::new();
    static HAS_INSTANCE: OnceLock<JsSymbol> = OnceLock::new();
    static TO_STRING_TAG: OnceLock<JsSymbol> = OnceLock::new();
    static DISPOSE: OnceLock<JsSymbol> = OnceLock::new();
    static ASYNC_DISPOSE: OnceLock<JsSymbol> = OnceLock::new();
    static UNSCOPABLES: OnceLock<JsSymbol> = OnceLock::new();
    static SPECIES: OnceLock<JsSymbol> = OnceLock::new();
    static IS_CONCAT_SPREADABLE: OnceLock<JsSymbol> = OnceLock::new();
    static MATCH: OnceLock<JsSymbol> = OnceLock::new();
    let slot = match name {
        "iterator" => &ITERATOR,
        "asyncIterator" => &ASYNC_ITERATOR,
        "toPrimitive" => &TO_PRIMITIVE,
        "hasInstance" => &HAS_INSTANCE,
        "toStringTag" => &TO_STRING_TAG,
        "dispose" => &DISPOSE,
        "asyncDispose" => &ASYNC_DISPOSE,
        "unscopables" => &UNSCOPABLES,
        "species" => &SPECIES,
        "isConcatSpreadable" => &IS_CONCAT_SPREADABLE,
        "match" => &MATCH,
        _ => unreachable!("unknown well-known symbol"),
    };
    slot.get_or_init(|| JsSymbol::new(Some(JsString::from(format!("Symbol.{name}").as_str()))))
        .clone()
}

/// An immutable, identity-compared JavaScript symbol. Cloning preserves
/// identity; constructing a symbol always creates a fresh identity.
/// To retain WeakMap entries keyed by this symbol across collection, keep
/// it in a runtime `Rooted` or scope local. A bare Rust clone is not a GC root.
#[derive(Clone, Debug)]
pub struct JsSymbol(Arc<SymbolData>);

#[derive(Debug)]
struct SymbolData {
    id: u64,
    description: Option<JsString>,
    registered: bool,
}

impl JsSymbol {
    /// A fresh symbol with an optional description (distinct from its key).
    pub fn new(description: Option<JsString>) -> Self {
        Self::create(description, false)
    }

    fn create(description: Option<JsString>, registered: bool) -> Self {
        let id = NEXT_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("symbol identities exhausted");
        Self(Arc::new(SymbolData {
            id,
            description,
            registered,
        }))
    }

    /// The original description; an absent description differs from `""`.
    pub fn description(&self) -> Option<&JsString> {
        self.0.description.as_ref()
    }

    pub(crate) fn identity(&self) -> u64 {
        self.0.id
    }

    pub(crate) fn can_be_held_weakly(&self) -> bool {
        !self.0.registered
    }
}

impl PartialEq for JsSymbol {
    fn eq(&self, other: &Self) -> bool {
        self.identity() == other.identity()
    }
}

impl Eq for JsSymbol {}

impl Hash for JsSymbol {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.identity().hash(state);
    }
}

impl fmt::Display for JsSymbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Symbol(")?;
        if let Some(description) = self.description() {
            write!(f, "{description}")?;
        }
        f.write_str(")")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Internal collector fixture: a live WeakRef must not retain a cleared
    /// symbol's immutable body/description merely to reconstruct deref.
    #[test]
    fn weak_ref_releases_cleared_symbol_metadata() {
        use crate::heap::gc::TraceEdge;
        use crate::heap::{Heap, HeapObject, Rooted};
        use crate::value::Value;
        let heap = Arc::new(Heap::default());
        let symbol = JsSymbol::new(Some(JsString::from(
            "large description ".repeat(4096).as_str(),
        )));
        let observer = Arc::downgrade(&symbol.0);
        let target = Value::Symbol(symbol);
        let target_root = Rooted::new(&heap, target.clone());
        let prototype = crate::realm::intrinsics(&heap).unwrap().weak_ref_prototype;
        let Value::Symbol(symbol) = &target else {
            unreachable!()
        };
        let id = heap
            .alloc(HeapObject::WeakRef(Box::new(
                crate::object::weak_ref::WeakRefObject::new(
                    crate::ids::TaskId::from_raw(1),
                    prototype,
                    &target,
                    TraceEdge::Symbol(symbol.identity()),
                ),
            )))
            .unwrap();
        let _weak_ref_root = Rooted::new(&heap, Value::Object(id));
        drop(target);
        heap.collect().unwrap();
        assert!(
            observer.upgrade().is_some(),
            "strong symbol root preserves metadata"
        );
        drop(target_root);
        heap.collect().unwrap();
        assert!(heap.is_live(id), "the cleared WeakRef remains live");
        assert!(
            observer.upgrade().is_none(),
            "clearing releases symbol metadata"
        );
        heap.read(id, |object| {
            let HeapObject::WeakRef(weak) = object else {
                unreachable!()
            };
            assert!(matches!(weak.deref(), Value::Undefined));
        })
        .unwrap();
    }

    #[test]
    fn identity_description_and_leaf_lifetime() {
        let s = JsSymbol::new(Some("x".into()));
        let clone = s.clone();
        let weak = Arc::downgrade(&s.0);
        assert_eq!(s, clone);
        assert_ne!(s, JsSymbol::new(Some("x".into())));
        assert_eq!(s.description(), Some(&JsString::from("x")));
        assert_eq!(JsSymbol::new(None).description(), None);
        assert_eq!(
            JsSymbol::new(Some("".into())).description(),
            Some(&JsString::from(""))
        );
        drop(s);
        assert!(weak.upgrade().is_some());
        drop(clone);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn registry_owns_leaves_until_runtime_drop() {
        let registry = SymbolRegistry::default();
        let symbol = registry.symbol_for("k".into());
        let weak = Arc::downgrade(&symbol.0);
        drop(symbol);
        assert!(weak.upgrade().is_some());
        assert_eq!(
            registry.key_for(&registry.symbol_for("k".into())),
            Some("k".into())
        );
        assert_eq!(registry.key_for(&JsSymbol::new(Some("k".into()))), None);
        let other = SymbolRegistry::default();
        assert_ne!(
            registry.symbol_for("k".into()),
            other.symbol_for("k".into())
        );
        assert_eq!(other.key_for(&registry.symbol_for("k".into())), None);
        drop(registry);
        assert!(weak.upgrade().is_none());
    }
}
