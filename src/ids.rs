//! Private runtime identifiers.
//!
//! These are opaque, internally allocated IDs. They compare and hash by their
//! numeric value only (never by user-defined equality), so internal registries
//! can key on them safely (`docs/design.md` §5). Embedders can read and compare
//! them but cannot construct them.

use std::fmt;

/// Identity of one task within its [`Runtime`](crate::Runtime).
///
/// Allocated from a per-runtime counter when the task is spawned; never reused
/// within that runtime. Future wake handles are task IDs (`docs/design.md` §8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskId(u64);

impl TaskId {
    pub(crate) fn from_raw(raw: u64) -> Self {
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
/// Every task runs in exactly one world. This slice only has the runtime's
/// initial world; creating further worlds belongs to the simulated host and
/// dependency-cell work (`docs/design.md` §§7, 9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorldId(u64);

impl WorldId {
    /// The world every runtime starts in.
    pub(crate) const INITIAL: WorldId = WorldId(0);
}

impl fmt::Display for WorldId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "world#{}", self.0)
    }
}

/// Identity of a definition or query that a task can depend on.
///
/// Reserved: nothing in this slice allocates one. Dependency cells
/// (`docs/design.md` §7) will key on (world, dependency, result kind).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DependencyId(u64);

impl fmt::Display for DependencyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "dep#{}", self.0)
    }
}

/// What a waiting task is waiting for: the dependency key of
/// `docs/design.md` §7 (world, definition/query identity).
///
/// Reserved for [`RunOutcome::Waiting`](crate::RunOutcome::Waiting); this slice
/// never produces it. The requested result kind joins the key when dependency
/// cells land.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WaitKey {
    world: WorldId,
    dependency: DependencyId,
}

impl WaitKey {
    /// The world the waiting task runs in.
    pub fn world(&self) -> WorldId {
        self.world
    }

    /// The definition or query being waited on.
    pub fn dependency(&self) -> DependencyId {
        self.dependency
    }
}
