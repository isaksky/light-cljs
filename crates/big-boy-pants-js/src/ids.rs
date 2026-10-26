//! Private runtime identifiers.
//!
//! These are opaque, typed IDs. They compare and hash by their numeric value
//! only (never by user-defined equality), so internal registries can key on
//! them safely (`docs/design.md` §5). Embedders can read and compare all of
//! them; task, world and producer IDs are allocated by the runtime, while
//! definition/query IDs and result kinds are assigned by the host (they are
//! the compiler-facing half of the `__rt.need` contract, `docs/design.md` §7).

use std::fmt;

/// Identity of one task within its [`Runtime`](crate::Runtime).
///
/// Allocated from a per-runtime counter when the task is spawned; never reused
/// within that runtime. It is also the task's wake handle and its consumer
/// identity on dependency cells (`docs/design.md` §8): a waiter is a task ID,
/// never a JavaScript callback.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskId(u64);

impl TaskId {
    pub(crate) const fn from_raw(raw: u64) -> Self {
        TaskId(raw)
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "task#{}", self.0)
    }
}

/// Immutable world/version identity carried by a task as execution context.
///
/// Every task runs in exactly one world, fixed when it is spawned
/// ([`Runtime::spawn_in`](crate::Runtime::spawn_in)). A runtime starts in its
/// initial world; [`Runtime::new_world`](crate::Runtime::new_world) allocates
/// further ones. A world is part of every [`DependencyKey`], so a result of
/// one world never satisfies a need in another (`docs/design.md` §§7, 9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorldId(u64);

impl WorldId {
    /// The world every runtime starts in.
    pub(crate) const INITIAL: WorldId = WorldId(0);

    pub(crate) const fn from_raw(raw: u64) -> Self {
        WorldId(raw)
    }
}

impl fmt::Display for WorldId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "world#{}", self.0)
    }
}

/// Identity of a definition or query that a task can depend on.
///
/// Assigned by the host: the runtime gives the number no meaning beyond
/// identity. Typed so it cannot be confused with a [`ResultKind`] or a world.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DependencyId(u64);

impl DependencyId {
    /// The definition/query with host-assigned number `raw`.
    pub const fn new(raw: u64) -> Self {
        DependencyId(raw)
    }

    /// The host-assigned number.
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for DependencyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "dep#{}", self.0)
    }
}

/// Which result of a definition is requested: binding discovery, callable
/// availability, initializer completion, an analysis result, ...
/// (`docs/design.md` §9). Host-assigned. Each kind of one definition is a
/// separate single-assignment cell, so no definition has one coarse "ready"
/// flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResultKind(u32);

impl ResultKind {
    /// The result kind with host-assigned number `raw`.
    pub const fn new(raw: u32) -> Self {
        ResultKind(raw)
    }
}

impl fmt::Display for ResultKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "kind#{}", self.0)
    }
}

/// The identity of one dependency cell (`docs/design.md` §7): world/version,
/// definition/query and requested result kind. Two keys name the same cell
/// exactly when all three components are equal. Comparison and hashing use
/// the numeric identities only, so no user code can run during a lookup.
///
/// Also what a waiting task waits for
/// ([`RunOutcome::Waiting`](crate::RunOutcome::Waiting)).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DependencyKey {
    world: WorldId,
    dependency: DependencyId,
    kind: ResultKind,
}

impl DependencyKey {
    pub const fn new(world: WorldId, dependency: DependencyId, kind: ResultKind) -> Self {
        DependencyKey {
            world,
            dependency,
            kind,
        }
    }

    /// The world/version the result belongs to.
    pub fn world(&self) -> WorldId {
        self.world
    }

    /// The definition or query.
    pub fn dependency(&self) -> DependencyId {
        self.dependency
    }

    /// The requested result kind.
    pub fn kind(&self) -> ResultKind {
        self.kind
    }
}

impl fmt::Display for DependencyKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}/{}", self.world, self.dependency, self.kind)
    }
}

/// Identity of one producer claim on a dependency cell. Allocated by the
/// runtime when a cell's single producer request is claimed and never reused
/// within that runtime; only the claim's holder can complete the cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProducerId(u64);

impl ProducerId {
    pub(crate) const fn from_raw(raw: u64) -> Self {
        ProducerId(raw)
    }
}

impl fmt::Display for ProducerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "producer#{}", self.0)
    }
}

/// Identity of one suspension of one task: the wake handle a dependency cell
/// stores for a waiting consumer (`docs/design.md` §8).
///
/// It names the task and the **execution** (the task's run number, counted
/// from 1) in which the task subscribed. A wake is honoured only while that
/// execution is still running or parked. Once the task has been woken, has
/// ended that run without parking, has started a later run, or has finished,
/// the handle is stale and delivering it does nothing, so a duplicate or late
/// notification can never make a later, unrelated suspension runnable. It
/// also names the runtime, so a handle of another runtime is always stale. See
/// [`crate::task`] for the state machine that checks it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WakeHandle {
    /// Process-unique identity of the runtime the task belongs to: task ids
    /// restart at 1 in every runtime, so this keeps another runtime's
    /// handle from waking a task with the same id and run number.
    runtime: u64,
    task: TaskId,
    execution: u64,
}

impl WakeHandle {
    pub(crate) const fn new(runtime: u64, task: TaskId, execution: u64) -> Self {
        WakeHandle {
            runtime,
            task,
            execution,
        }
    }

    pub(crate) fn runtime(&self) -> u64 {
        self.runtime
    }

    /// The task to wake.
    pub fn task(&self) -> TaskId {
        self.task
    }

    /// The execution (run number of [`WakeHandle::task`]) that subscribed.
    pub fn execution(&self) -> u64 {
        self.execution
    }
}

impl fmt::Display for WakeHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@run{}", self.task, self.execution)
    }
}
