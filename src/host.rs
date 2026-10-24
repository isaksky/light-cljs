//! Leaf host effects: the narrow host trait used by the `HostEffect` opcode.
//!
//! A [`Host`] is supplied by the embedder for one run
//! ([`Task::run_with`](crate::Task::run_with)). When the task executes
//! `HostEffect op`, the VM pops one argument, calls [`Host::effect`]
//! synchronously on the running thread, and pushes the returned primitive.
//!
//! **Leaf contract** (`docs/design.md` §10: "Leaf native operations that
//! cannot suspend or reenter may remain ordinary Rust calls"). A host effect:
//!
//! * **may not reenter the VM.** While an effect runs on a thread, starting
//!   any task run on that thread fails with
//!   [`RunError::InsideHostEffect`](crate::RunError::InsideHostEffect), and
//!   opening a heap scope ([`Runtime::scope`](crate::Runtime::scope)) panics.
//! * **may not allocate or hold heap values across suspension.** It sees only
//!   root-free [`Primitive`]s: the argument is a primitive (an object argument
//!   fails the task with `UnsupportedOperand` before the host is called) and
//!   so is the result. It is given no heap access.
//! * **may not suspend.** It returns a value or an error; there is no way to
//!   report `Waiting` or `Yielded` from it. Resumable natives that invoke
//!   JavaScript or wait on dependencies are a separate protocol (pellet
//!   clj-quickjs-12).
//!
//! **Exactly-once execution.** The execution budget is only checked between
//! instructions, and the `HostEffect` instruction advances the saved
//! instruction position before calling the host. So an effect that has run is
//! never re-executed when the task yields and is run again; an effect that has
//! not run yet runs on a later run. If the host returns an error the task
//! fails ([`VmErrorKind::HostEffect`](crate::VmErrorKind::HostEffect)).
//!
//! The simulated compiler host of worlds, result kinds and producers is the
//! [`sim`] submodule; it drives tasks from outside and is not a leaf effect.

pub mod sim;

use std::cell::Cell;

use crate::ids::TaskId;
use crate::value::Primitive;

/// One call of a leaf host effect.
#[derive(Clone, Debug, PartialEq)]
pub struct HostCall {
    /// The task executing the effect.
    pub task: TaskId,
    /// The host-defined operation selector (the `HostEffect` operand).
    pub op: u16,
    /// The argument popped from the operand stack.
    pub arg: Primitive,
}

/// Embedder-supplied leaf host effects. See the module docs for the contract.
pub trait Host {
    /// Default locale for this run. A leaf effect: no reentry, heap access or
    /// suspension. Reads are not dependency inputs; embedders may vary this
    /// between runs/worlds and should keep it stable when reproducibility matters.
    fn default_locale(&mut self) -> String {
        "en-US".to_owned()
    }

    /// Perform the effect and return its result, or an error message that
    /// fails the task.
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String>;
}

/// The host used by [`Task::run`](crate::Task::run): every effect fails.
#[derive(Debug, Default)]
pub(crate) struct NoHost;

impl Host for NoHost {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        Err(format!(
            "no host supplied for host operation {} (use Task::run_with)",
            call.op
        ))
    }
}

thread_local! {
    /// True while a host effect is executing on this thread.
    static IN_EFFECT: Cell<bool> = const { Cell::new(false) };
}

/// True while the current thread is inside [`Host::effect`].
pub(crate) fn inside_effect() -> bool {
    IN_EFFECT.with(Cell::get)
}

/// Clears the in-effect flag on drop, including on unwind.
struct EffectMark;

impl Drop for EffectMark {
    fn drop(&mut self) {
        IN_EFFECT.with(|f| f.set(false));
    }
}

/// Call `host.effect(call)` with the reentrancy flag set.
pub(crate) fn call_leaf(host: &mut dyn Host, call: HostCall) -> Result<Primitive, String> {
    // Runs cannot start inside an effect, so the flag is never already set.
    IN_EFFECT.with(|f| f.set(true));
    let _mark = EffectMark;
    host.effect(call)
}

/// Read the host locale under the same leaf guard as `effect`.
pub(crate) fn call_default_locale(host: &mut dyn Host) -> String {
    IN_EFFECT.with(|f| assert!(!f.replace(true), "nested host locale effect"));
    let _mark = EffectMark;
    host.default_locale()
}
