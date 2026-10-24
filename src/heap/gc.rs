//! Collector boundary: tracing and root categories.
//!
//! No collector exists yet (the heap is a non-reclaiming stub, see
//! [`crate::heap`]). This module fixes the contract the collector of pellet
//! clj-quickjs-2 plugs into:
//!
//! * Every structure that stores raw `Value`s implements `Trace`, which
//!   reports each heap reference it holds. Implemented today for values, the
//!   persistent root table, scope locals, and task VM state (frames, operand
//!   stack, locals).
//! * `RootCategory` lists every root source. Executable constants are
//!   primitives only in this slice, so executables hold no heap edges yet.

use super::ObjectId;
use crate::value::Value;

/// A source of collector roots. Append-only registry: later pellets add
/// categories at the end (dependency results, atom contents, closure
/// environments, native payloads, ...).
#[allow(dead_code)] // Consumed by the collector (clj-quickjs-2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RootCategory {
    /// [`Rooted`](crate::Rooted) handles, including task results.
    PersistentHandle,
    /// Values held by an open [`Scope`](crate::Scope).
    ScopeLocal,
    /// Task-owned VM state: locals and operand stack of running and
    /// not-yet-run tasks.
    TaskFrame,
}

/// Report every heap reference held by `self`.
///
/// Implementations must visit each reachable [`ObjectId`] stored directly in
/// the structure; they must not allocate on the heap or run bytecode.
#[allow(dead_code)] // Called by the collector (clj-quickjs-2); tests call it today.
pub(crate) trait Trace {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId));
}

impl Trace for Value {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        match *self {
            Value::Object(id) => visit(id),
            Value::Undefined | Value::Null | Value::Bool(_) | Value::Number(_) => {}
        }
    }
}
