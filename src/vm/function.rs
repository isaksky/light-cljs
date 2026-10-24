//! Function objects, lexical closures and binding cells: the heap kinds a
//! `Call` invokes and a closure's environment is made of.
//!
//! A function object is a heap value (`HeapObject::Function`) holding an `Arc`
//! of shared, immutable function [`Code`] and the values it captured when it
//! was created. Creating one (`Function`, `CurrentFunction`) never copies the
//! code, and every task that calls it runs that one copy in its own
//! task-owned frames (`crate::vm::frames`).
//!
//! Identity is per object: each `Function` evaluation allocates a new object,
//! as each evaluation of a JavaScript function expression does, and `===`
//! compares objects, not code.
//!
//! # Lexical environments (clj-quickjs-20)
//!
//! A closure's lexical environment is the array of values it captured,
//! fixed when `Function` creates it (the opcodes are in [`crate::vm::instr`]).
//! The code declares how many it carries ([`Code::captures`]); `GetCapture i`
//! reads entry `i` of the running frame's function object. Each entry is
//! either
//!
//! * **an immutable value**, captured by value: correct for a binding that is
//!   initialized before the closure is created and never assigned afterwards
//!   (a `const`, or a parameter or `let` the compiler proves is never
//!   reassigned). Copying it loses nothing: no one can observe a change; or
//! * **a binding cell** ([`BindingCell`], `HeapObject::Cell`), the storage of
//!   a mutable binding (or of any binding a closure may read before its
//!   initialization). The creating frame keeps the cell in the binding's local
//!   slot and every closure that captures the binding holds the *same* cell,
//!   so they all observe one binding (QuickJS's `JSVarRef`, which
//!   `get_var_ref` shares between closures of one frame), and the binding
//!   outlives the frame because the cell is an ordinary heap object reached
//!   from the closures. A closure created inside a closure re-captures with
//!   `GetCapture`, passing the same cell (or value) on, as QuickJS's
//!   `js_closure2` copies a parent's `var_refs`.
//!
//! Unlike QuickJS there is no "open" variable reference pointing into a live
//! frame and no `close_var_refs` copy when the frame exits: a captured mutable
//! binding lives in its cell from its declaration on, so frames stay plain
//! task-owned storage that may move when the value stack grows, and nothing
//! has to be fixed up when a frame is popped, yielded or collected. Each
//! execution of a declaration's `NewCell` makes a fresh binding (a
//! `let` in a loop body gets one per iteration).
//!
//! **Temporal dead zone.** A cell starts uninitialized; `CellInit` gives it
//! its first value, and `CellGet`/`CellSet` on an uninitialized cell fail with
//! `ReferenceError`. This is the run-time check a compiler needs once a
//! closure can run before a `let`/`const` declaration has executed (the
//! static decision of `crate::frontend::lower` is only sound without
//! closures).
//!
//! # Ownership and publication
//!
//! Executable code is immutable `Arc` storage shared by every task; closure
//! environments are heap objects and follow the publication rules of
//! [`crate::value::share`], which are separate from code sharing:
//!
//! * a function object never changes its code or captured values after
//!   allocation, so it is [`Persistent`](crate::value::share::Shareability::Persistent):
//!   shareable exactly when every captured value is. A closure capturing only
//!   immutable shareable values (primitives, persistent collections, other
//!   such closures) may be published and called from another task;
//! * a binding cell is mutable, so it is
//!   [`TaskOwned`](crate::value::share::Shareability::TaskOwned) by the task
//!   that created it. A closure that captures one is therefore rejected by the
//!   publication check, with a path ending in `.capture(i)`. Cell operations
//!   from any other task fail with
//!   [`VmErrorKind::ForeignBinding`](crate::VmErrorKind::ForeignBinding), so
//!   even a closure handed over without the check (`Runtime::spawn` arguments
//!   are not checked) cannot read or write another task's bindings.
//!
//! # Tracing
//!
//! A function object's `Trace` reports its code's constants (primitives in
//! this slice) and its captured values; a cell's reports its value. Cycles
//! (a closure stored in a binding it captures, as in
//! `let f = function () { return f; }`) are ordinary heap cycles: the
//! collector traces through them while they are reachable from a task frame
//! or a root and reclaims them once they are not.

use std::sync::Arc;

use crate::heap::ObjectId;
use crate::heap::gc::Trace;
use crate::ids::TaskId;
use crate::value::Value;
use crate::vm::executable::Code;

/// Storage of a function object. See the module docs.
#[derive(Debug)]
pub(crate) struct FunctionObject {
    pub(crate) code: Arc<Code>,
    /// The closure's environment: `code.captures` values, never changed
    /// after allocation.
    pub(crate) captures: Box<[Value]>,
}

impl Trace for FunctionObject {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        self.code.trace(visit);
        self.captures.trace(visit);
    }
}

/// Storage of a mutable captured binding. See the module docs.
#[derive(Debug)]
pub(crate) struct BindingCell {
    /// The task whose run created the cell; the only one that may use it.
    pub(crate) owner: TaskId,
    /// `None` until `CellInit` runs (temporal dead zone).
    pub(crate) value: Option<Value>,
}

impl Trace for BindingCell {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        self.value.iter().for_each(|v| v.trace(visit));
    }
}
