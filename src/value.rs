//! Value representation.
//!
//! Two layers, deliberately kept apart:
//!
//! * [`Value`] (crate-private) is the raw VM value stored in task frames,
//!   operand stacks, root tables and heap objects. A heap reference inside it is
//!   an unrooted [`ObjectId`]. Raw values never leave the crate, because a Rust
//!   local holding one is not a collector root (`docs/design.md` §11).
//! * [`Primitive`] and [`ValueView`] are the embedder-visible, root-free views.
//!   They carry no heap address, so they can be copied and kept anywhere. Access
//!   to heap objects goes through a persistent [`Rooted`](crate::Rooted) handle
//!   or a scoped [`Local`](crate::Local) handle.
//!
//! Equality and identity are distinct: [`Value::strict_equals`] is JavaScript
//! `===`; [`Value::same_object`] is heap identity only.

use crate::heap::ObjectId;

/// A raw VM value. See the module docs for why this is crate-private.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Value {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    /// Reference to a heap object; identity is the [`ObjectId`].
    Object(ObjectId),
}

impl Value {
    /// JavaScript ToBoolean for the supported value types.
    pub(crate) fn truthy(self) -> bool {
        match self {
            Value::Undefined | Value::Null => false,
            Value::Bool(b) => b,
            Value::Number(n) => !(n == 0.0 || n.is_nan()),
            Value::Object(_) => true,
        }
    }

    /// JavaScript `===` (IsStrictlyEqual) for the supported value types:
    /// numbers compare by IEEE value (`NaN !== NaN`, `+0 === -0`), objects by
    /// identity, and values of different types are never equal.
    pub(crate) fn strict_equals(self, other: Value) -> bool {
        match (self, other) {
            (Value::Undefined, Value::Undefined) | (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::Object(a), Value::Object(b)) => a == b,
            _ => false,
        }
    }

    /// True only when both values reference the same heap object.
    pub(crate) fn same_object(self, other: Value) -> bool {
        matches!((self, other), (Value::Object(a), Value::Object(b)) if a == b)
    }

    /// Short type name used in diagnostics (JavaScript `typeof` spelling,
    /// except `null`).
    pub(crate) fn type_name(self) -> &'static str {
        match self {
            Value::Undefined => "undefined",
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::Object(_) => "object",
        }
    }

    /// The embedder-visible view of this value.
    pub(crate) fn view(self) -> ValueView {
        match self {
            Value::Undefined => ValueView::Undefined,
            Value::Null => ValueView::Null,
            Value::Bool(b) => ValueView::Bool(b),
            Value::Number(n) => ValueView::Number(n),
            Value::Object(_) => ValueView::Object,
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
        }
    }
}

/// A non-heap value: usable as an executable constant or a host argument
/// without any rooting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Primitive {
    Undefined,
    Null,
    Bool(bool),
    /// A JavaScript number (IEEE-754 double).
    Number(f64),
}

/// Root-free, copyable view of a value for inspection by the embedder.
///
/// `Object` intentionally carries no address: to keep or compare an object,
/// hold a [`Rooted`](crate::Rooted) or [`Local`](crate::Local) handle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ValueView {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    /// A heap object (in this slice: a property-less plain object).
    Object,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_equals_follows_javascript_for_supported_types() {
        assert!(Value::Number(0.0).strict_equals(Value::Number(-0.0)));
        assert!(!Value::Number(f64::NAN).strict_equals(Value::Number(f64::NAN)));
        assert!(!Value::Undefined.strict_equals(Value::Null));
        assert!(!Value::Number(1.0).strict_equals(Value::Bool(true)));
        let a = ObjectId::from_index(0);
        let b = ObjectId::from_index(1);
        assert!(Value::Object(a).strict_equals(Value::Object(a)));
        assert!(!Value::Object(a).strict_equals(Value::Object(b)));
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
    }
}
