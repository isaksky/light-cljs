//! Immutable symbol primitives outside the traced heap, like `JsString`.
//! Identity is process-wide and never reused, including across runtimes.
//! The body has no heap edges; clones keep the description alive directly.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use super::JsString;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Well-known identities are process-wide; every realm and task shares
/// them. These three immutable bodies live until process exit.
pub(crate) fn well_known(name: &str) -> JsSymbol {
    static ITERATOR: OnceLock<JsSymbol> = OnceLock::new();
    static TO_PRIMITIVE: OnceLock<JsSymbol> = OnceLock::new();
    static HAS_INSTANCE: OnceLock<JsSymbol> = OnceLock::new();
    let slot = match name {
        "iterator" => &ITERATOR,
        "toPrimitive" => &TO_PRIMITIVE,
        "hasInstance" => &HAS_INSTANCE,
        _ => unreachable!("unknown well-known symbol"),
    };
    slot.get_or_init(|| JsSymbol::new(Some(JsString::from(format!("Symbol.{name}").as_str()))))
        .clone()
}

/// An immutable, identity-compared JavaScript symbol. Cloning preserves
/// identity; constructing a symbol always creates a fresh identity.
#[derive(Clone, Debug)]
pub struct JsSymbol(Arc<SymbolData>);

#[derive(Debug)]
struct SymbolData {
    id: u64,
    description: Option<JsString>,
}

impl JsSymbol {
    /// A fresh symbol with an optional description (distinct from its key).
    pub fn new(description: Option<JsString>) -> Self {
        let id = NEXT_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("symbol identities exhausted");
        Self(Arc::new(SymbolData { id, description }))
    }

    /// The original description; an absent description differs from `""`.
    pub fn description(&self) -> Option<&JsString> {
        self.0.description.as_ref()
    }

    pub(crate) fn identity(&self) -> u64 {
        self.0.id
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
}
