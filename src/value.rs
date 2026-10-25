//! Value representation.
//!
//! Two layers, deliberately kept apart:
//!
//! * [`Value`] (crate-private) is the raw VM value stored in task frames,
//!   operand stacks, root tables and heap objects. A heap reference inside it is
//!   an unrooted [`ObjectId`]. Raw values never leave the crate, because a Rust
//!   local holding one is not a collector root (`docs/design.md` §11).
//! * [`Primitive`] and [`ValueView`] are the embedder-visible, root-free views.
//!   They carry no heap address, so they can be cloned and kept anywhere. Access
//!   to heap objects goes through a persistent [`Rooted`](crate::Rooted) handle
//!   or a scoped [`Local`](crate::Local) handle.
//! * Strings ([`JsString`], `Value::String`, clj-quickjs-65) are primitives
//!   over immutable `Arc` storage outside the heap ([`string`] module docs):
//!   not heap objects, compared by content, shareable by construction.
//!
//! Equality and identity are distinct: [`Value::strict_equals`] is JavaScript
//! `===`; [`Value::same_object`] is heap identity only.

pub(crate) mod bigint;
pub(crate) mod share;
pub(crate) mod string;
pub(crate) mod string_unicode;
pub(crate) mod symbol;

use crate::heap::ObjectId;
pub use bigint::{BigIntError, JsBigInt, MAX_BIGINT_BITS};
pub use string::JsString;
pub use symbol::JsSymbol;

/// A raw VM value. See the module docs for why this is crate-private.
///
/// Not `Copy`: a string owns a reference to its immutable body
/// ([`JsString`]), so copying a value is an explicit, cheap `clone`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Value {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    /// Reference to a heap object; identity is the [`ObjectId`].
    Object(ObjectId),
    /// A string primitive: immutable code units outside the heap
    /// ([`string`] module docs). Compared by content, never by identity.
    String(JsString),
    /// An immutable primitive compared by identity.
    Symbol(JsSymbol),
    /// An exact integer primitive with canonical immutable storage.
    BigInt(JsBigInt),
}

impl Value {
    /// JavaScript ToBoolean for the supported value types.
    pub(crate) fn truthy(&self) -> bool {
        match self {
            Value::Undefined | Value::Null => false,
            Value::Bool(b) => *b,
            Value::Number(n) => !(*n == 0.0 || n.is_nan()),
            Value::Object(_) | Value::Symbol(_) => true,
            Value::String(s) => !s.is_empty(),
            Value::BigInt(n) => !n.is_zero(),
        }
    }

    /// JavaScript `===` (IsStrictlyEqual) for the supported value types:
    /// numbers compare by IEEE value (`NaN !== NaN`, `+0 === -0`), strings
    /// by content (code units), objects by identity, and values of different
    /// types are never equal.
    pub(crate) fn strict_equals(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Undefined, Value::Undefined) | (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::Object(a), Value::Object(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Symbol(a), Value::Symbol(b)) => a == b,
            (Value::BigInt(a), Value::BigInt(b)) => a == b,
            _ => false,
        }
    }

    /// True only when both values reference the same heap object.
    pub(crate) fn same_object(&self, other: &Value) -> bool {
        matches!((self, other), (Value::Object(a), Value::Object(b)) if a == b)
    }

    /// Short type name used in diagnostics (JavaScript `typeof` spelling,
    /// except `null`).
    pub(crate) fn type_name(&self) -> &'static str {
        match self {
            Value::Undefined => "undefined",
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::Object(_) => "object",
            Value::String(_) => "string",
            Value::Symbol(_) => "symbol",
            Value::BigInt(_) => "bigint",
        }
    }

    /// The value as a root-free primitive; `None` for heap references.
    pub(crate) fn to_primitive(&self) -> Option<Primitive> {
        match self {
            Value::Undefined => Some(Primitive::Undefined),
            Value::Null => Some(Primitive::Null),
            Value::Bool(b) => Some(Primitive::Bool(*b)),
            Value::Number(n) => Some(Primitive::Number(*n)),
            Value::Object(_) => None,
            Value::String(s) => Some(Primitive::String(s.clone())),
            Value::Symbol(s) => Some(Primitive::Symbol(s.clone())),
            Value::BigInt(n) => Some(Primitive::BigInt(n.clone())),
        }
    }

    /// The embedder-visible view of this value.
    pub(crate) fn view(&self) -> ValueView {
        match self {
            Value::Undefined => ValueView::Undefined,
            Value::Null => ValueView::Null,
            Value::Bool(b) => ValueView::Bool(*b),
            Value::Number(n) => ValueView::Number(*n),
            Value::Object(_) => ValueView::Object,
            Value::String(_) => ValueView::String,
            Value::Symbol(_) => ValueView::Symbol,
            Value::BigInt(_) => ValueView::BigInt,
        }
    }
}

impl From<Primitive> for Value {
    fn from(p: Primitive) -> Self {
        match p {
            Primitive::Undefined => Value::Undefined,
            Primitive::Null => Value::Null,
            Primitive::Bool(b) => Value::Bool(b),
            Primitive::Number(n) => Value::Number(n),
            Primitive::String(s) => Value::String(s),
            Primitive::Symbol(s) => Value::Symbol(s),
            Primitive::BigInt(n) => Value::BigInt(n),
        }
    }
}

impl From<&Primitive> for Value {
    fn from(p: &Primitive) -> Self {
        Value::from(p.clone())
    }
}

/// A non-heap value: usable as an executable constant or a host argument
/// without any rooting.
///
/// Not `Copy` since strings exist (clj-quickjs-65): a string shares its
/// immutable body, so `clone` is O(1).
#[derive(Clone, Debug, PartialEq)]
pub enum Primitive {
    Undefined,
    Null,
    Bool(bool),
    /// A JavaScript number (IEEE-754 double).
    Number(f64),
    /// A JavaScript string ([`JsString`]): immutable, compared by content.
    String(JsString),
    /// An immutable primitive compared by identity.
    Symbol(JsSymbol),
    /// An exact immutable integer.
    BigInt(JsBigInt),
}

impl Primitive {
    /// A string primitive with the contents of `s`.
    pub fn string(s: &str) -> Primitive {
        Primitive::String(JsString::from(s))
    }
}

/// Root-free, copyable view of a value for inspection by the embedder.
///
/// `Object` intentionally carries no address: to keep or compare an object,
/// hold a [`Rooted`](crate::Rooted) or [`Local`](crate::Local) handle.
/// `String` carries no contents, so the view stays `Copy`; read them with
/// [`Rooted::string`](crate::Rooted::string) or
/// [`Scope::string`](crate::Scope::string).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ValueView {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    /// A heap object (in this slice: a property-less plain object, a
    /// collection, or a function object).
    Object,
    /// A string primitive.
    String,
    /// A symbol primitive.
    Symbol,
    /// A BigInt primitive; inspect with `Rooted::bigint` or `Scope::bigint`.
    BigInt,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_equals_follows_javascript_for_supported_types() {
        assert!(Value::Number(0.0).strict_equals(&Value::Number(-0.0)));
        assert!(!Value::Number(f64::NAN).strict_equals(&Value::Number(f64::NAN)));
        assert!(!Value::Undefined.strict_equals(&Value::Null));
        assert!(!Value::Number(1.0).strict_equals(&Value::Bool(true)));
        let a = ObjectId::from_index(0);
        let b = ObjectId::from_index(1);
        assert!(Value::Object(a).strict_equals(&Value::Object(a)));
        assert!(!Value::Object(a).strict_equals(&Value::Object(b)));
        let s = |t: &str| Value::String(JsString::from(t));
        assert!(s("ab").strict_equals(&s("ab")));
        assert!(!s("ab").strict_equals(&s("ba")));
        assert!(!s("1").strict_equals(&Value::Number(1.0)));
    }

    #[test]
    fn truthiness_follows_javascript_to_boolean() {
        assert!(!Value::Undefined.truthy());
        assert!(!Value::Null.truthy());
        assert!(!Value::Number(0.0).truthy());
        assert!(!Value::Number(-0.0).truthy());
        assert!(!Value::Number(f64::NAN).truthy());
        assert!(Value::Number(-1.0).truthy());
        assert!(Value::Object(ObjectId::from_index(0)).truthy());
        assert!(!Value::String(JsString::from("")).truthy());
        assert!(Value::String(JsString::from("0")).truthy());
    }
}
