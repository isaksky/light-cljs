//! Function objects: the heap kind a `Call` instruction invokes.
//!
//! A function object is a heap value (`HeapObject::Function`) holding an `Arc`
//! of shared, immutable function [`Code`]. Creating one (`Function`,
//! `CurrentFunction`) never copies the code, and every task that calls it runs
//! that one copy in its own task-owned frames (`crate::vm::frames`).
//!
//! Identity is per object: each `Function` evaluation allocates a new object,
//! as each evaluation of a JavaScript function expression does, and `===`
//! compares objects, not code.
//!
//! **Publication.** In this slice a function object has no properties and no
//! captured bindings, so it is immutable and classified
//! [`Persistent`](crate::value::share::Shareability::Persistent): shareable
//! exactly when its edges are. Its edges are its code's constants (primitives
//! in this slice), reported by its [`Trace`]. Closures with captured bindings
//! (clj-quickjs-20) add their environment edges and their own rules.

use std::sync::Arc;

use crate::heap::ObjectId;
use crate::heap::gc::Trace;
use crate::vm::executable::Code;

/// Storage of a function object. See the module docs.
#[derive(Debug)]
pub(crate) struct FunctionObject {
    pub(crate) code: Arc<Code>,
}

impl Trace for FunctionObject {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        self.code.trace(visit);
    }
}
