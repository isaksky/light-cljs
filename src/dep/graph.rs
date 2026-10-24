//! Dependency graph semantics (`docs/design.md` §9): recorded uses versus
//! active waits, and detection of genuine wait cycles.
//!
//! # Two relationships, kept apart
//!
//! * A **recorded use** (`task` used result `key`) is history. It is added
//!   for every need, ready reads included, and never removed: resolution,
//!   failure and cancellation leave it in place.
//! * An **active wait** (`task` cannot proceed until `key` completes) exists
//!   only while the task is suspended on a pending cell. A task has at most
//!   one. It is removed when the key is resolved ([`DependencyGraph::resolve`]:
//!   any terminal outcome, `Ready`, `Failed` or `Cancelled`) or the task stops
//!   waiting for another reason ([`DependencyGraph::withdraw`]: consumer
//!   cancellation, a wake, a refused wait).
//!
//! Only active waits are traversed for deadlocks. A recursive reference graph
//! (two callables using each other's bindings) is a set of recorded uses with
//! no active wait between them, so it is never a cycle.
//!
//! # Cycles
//!
//! A wait cycle needs a third relationship, the **producer binding**: the
//! task executing the claimed producer of a pending key
//! ([`DependencyGraph::bind_producer`]). Each pending key has at most one
//! producer and each task at most one active wait, so from a new edge
//! `task → key` the graph follows one chain: `key`'s producer task, the key
//! that task waits on, its producer task, and so on. The new edge closes a
//! cycle exactly when the chain comes back to `task`; the walk is linear in
//! the chain length. Keys that are resolved, unclaimed, or claimed by a
//! producer the graph does not know (outside the host) end the chain:
//! nothing in the graph can be blocked on them.
//!
//! **Terminal policy: the newest demand is refused.** A wait that would
//! close a cycle is not added; [`DependencyGraph::wait`] returns the
//! [`WaitCycle`] (every step's waiting task, demand site, key and producer)
//! and the caller must not suspend the task on that key. The caller settles
//! the refused demand as a failure: the simulated host withdraws the task's
//! subscription and fails the cell its job produces with
//! [`SimError::WaitCycle`](crate::host::sim::SimError::WaitCycle), which wakes
//! the next task on the cycle, whose input has now failed, and so on around
//! the cycle. So every task on the cycle (and every other waiter on those
//! keys) settles; nothing is left silently waiting. Exactly one demand per
//! cycle is reported: the edges already in the graph form no cycle.
//!
//! # Identity and concurrency
//!
//! The graph's maps are keyed only by private numeric identities
//! ([`TaskId`], [`DependencyKey`], [`ProducerId`]), hashed and compared by
//! their derived implementations; no user hashing, equality or getter runs.
//!
//! The graph is plain data owned by the scheduler; the simulated host keeps
//! one per host and updates it from its single driver thread. A scheduler
//! with worker threads (such as `exec::Executor`, which does not track
//! producers yet) keeps it under one leaf lock, held
//! only for these calls (never while taking a cell lock, running host code or
//! JavaScript), and follows the same order the simulated host does:
//!
//! * record a wait only after the need returned `Waiting` (the task is
//!   subscribed);
//! * a producer resolves its key **before** completing the cell, and an
//!   embedder before cancelling it. A producer that is completing is not
//!   blocked, so its key cannot be on a genuine cycle; resolving first means
//!   a reported cycle never runs through a key that is already complete.
//!   A wait recorded after the resolution adds no edge (`resolve` marks the
//!   key; cells never return to pending), and one recorded before is removed
//!   by it, so neither order leaves a stale edge.
//!
//! A scheduler that cannot see every settlement (an embedder completing or
//! cancelling cells behind its back) must revalidate a returned cycle
//! against the cells before acting on it: resolve the keys that are
//! already terminal and retry the wait. The simulated host does this.

use std::collections::{HashMap, HashSet};
use std::fmt;

use crate::error::SourceLocation;
use crate::ids::{DependencyKey, ProducerId, TaskId};

/// Where a demand was made: a readable description from the scheduler (for
/// the simulated host, the producing result and which input or call), and the
/// JavaScript source location when the demand comes from VM code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DemandSite {
    pub description: String,
    pub location: Option<SourceLocation>,
}

impl DemandSite {
    /// A site described by `description`, without a source location.
    pub fn new(description: impl Into<String>) -> DemandSite {
        DemandSite {
            description: description.into(),
            location: None,
        }
    }

    /// This site at source location `location`.
    pub fn at(mut self, location: SourceLocation) -> DemandSite {
        self.location = Some(location);
        self
    }
}

impl fmt::Display for DemandSite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.description)?;
        match self.location {
            Some(location) => write!(f, " at {location}"),
            None => Ok(()),
        }
    }
}

/// One edge of a wait cycle: `task` waits (at `site`) on `key`, whose claimed
/// `producer` runs on `producer_task`, the waiting task of the next step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WaitStep {
    pub task: TaskId,
    pub site: DemandSite,
    /// The awaited result: its world, definition and result kind.
    pub key: DependencyKey,
    pub producer: ProducerId,
    pub producer_task: TaskId,
}

/// A genuine active wait cycle (module docs, "Cycles"). The first step is the
/// refused demand; the last step's producer task is the first step's task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WaitCycle {
    steps: Vec<WaitStep>,
}

impl WaitCycle {
    /// The edges of the cycle in wait order, starting with the refused
    /// demand.
    pub fn steps(&self) -> &[WaitStep] {
        &self.steps
    }

    /// The awaited keys, in wait order.
    pub fn keys(&self) -> Vec<DependencyKey> {
        self.steps.iter().map(|s| s.key).collect()
    }

    /// The waiting tasks, in wait order.
    pub fn tasks(&self) -> Vec<TaskId> {
        self.steps.iter().map(|s| s.task).collect()
    }
}

impl fmt::Display for WaitCycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "wait cycle of {} demand(s): ", self.steps.len())?;
        for (i, step) in self.steps.iter().enumerate() {
            if i > 0 {
                f.write_str(" -> ")?;
            }
            write!(
                f,
                "{} ({}) waits on {}, produced by {} on {}",
                step.task, step.site, step.key, step.producer, step.producer_task
            )?;
        }
        Ok(())
    }
}

/// Recorded uses of one task, in first-use order, without duplicates.
#[derive(Debug, Default)]
struct Uses {
    order: Vec<DependencyKey>,
    set: HashSet<DependencyKey>,
}

/// Recorded uses, active waits and producer bindings. See the module docs.
#[derive(Debug, Default)]
pub struct DependencyGraph {
    uses: HashMap<TaskId, Uses>,
    /// Active waits: at most one per task.
    waits: HashMap<TaskId, (DependencyKey, DemandSite)>,
    /// Reverse index of `waits`, for resolution.
    waiters: HashMap<DependencyKey, HashSet<TaskId>>,
    /// The task running each pending key's claimed producer.
    producers: HashMap<DependencyKey, (ProducerId, TaskId)>,
    /// Keys with a terminal outcome (or about to get one): no edge onto them.
    resolved: HashSet<DependencyKey>,
}

impl DependencyGraph {
    pub fn new() -> DependencyGraph {
        DependencyGraph::default()
    }

    /// Record that `task` used `key`, whatever the outcome. Never removed.
    pub fn record_use(&mut self, task: TaskId, key: DependencyKey) {
        let uses = self.uses.entry(task).or_default();
        if uses.set.insert(key) {
            uses.order.push(key);
        }
    }

    /// The keys `task` has used, in first-use order.
    pub fn uses(&self, task: TaskId) -> &[DependencyKey] {
        self.uses.get(&task).map_or(&[], |u| &u.order)
    }

    /// True if `task` has used `key`.
    pub fn used(&self, task: TaskId, key: DependencyKey) -> bool {
        self.uses.get(&task).is_some_and(|u| u.set.contains(&key))
    }

    /// `task` runs `producer`, the claimed producer of pending `key`. Bind it
    /// before the task waits on anything. A resolved key is not bound.
    pub fn bind_producer(&mut self, key: DependencyKey, producer: ProducerId, task: TaskId) {
        if !self.resolved.contains(&key) {
            self.producers.insert(key, (producer, task));
        }
    }

    /// The producer of pending `key` and the task running it, if bound.
    pub fn producer(&self, key: DependencyKey) -> Option<(ProducerId, TaskId)> {
        self.producers.get(&key).copied()
    }

    /// `task` is suspended on pending `key`, demanded at `site`: record the use
    /// and add the active wait (replacing any other wait of `task`), unless
    /// `key` is already resolved (then only the use is recorded).
    ///
    /// Returns the cycle, adding no wait, if the edge would close one (module
    /// docs, "Cycles"); the use is recorded either way.
    pub fn wait(
        &mut self,
        task: TaskId,
        key: DependencyKey,
        site: DemandSite,
    ) -> Result<(), WaitCycle> {
        self.record_use(task, key);
        self.withdraw(task);
        if self.resolved.contains(&key) {
            return Ok(());
        }
        if let Some(cycle) = self.closes_cycle(task, key, &site) {
            return Err(cycle);
        }
        self.waits.insert(task, (key, site));
        self.waiters.entry(key).or_default().insert(task);
        Ok(())
    }

    /// Follow the chain from the would-be edge `task → key`.
    fn closes_cycle(
        &self,
        task: TaskId,
        key: DependencyKey,
        site: &DemandSite,
    ) -> Option<WaitCycle> {
        // (waiting task, key, producer, producer task) per step; sites are
        // cloned only for a cycle.
        let mut chain = Vec::new();
        let mut waiter = task;
        let mut awaited = key;
        loop {
            let &(producer, producer_task) = self.producers.get(&awaited)?;
            chain.push((waiter, awaited, producer, producer_task));
            if producer_task == task {
                break;
            }
            let &(next, _) = self.waits.get(&producer_task)?;
            // Every waiting task appears at most once on a chain into `task`;
            // a longer chain loops without `task` (not closed by this edge).
            if chain.len() > self.waits.len() {
                return None;
            }
            waiter = producer_task;
            awaited = next;
        }
        let steps = chain
            .into_iter()
            .map(|(waiter, key, producer, producer_task)| WaitStep {
                task: waiter,
                site: if waiter == task {
                    site.clone()
                } else {
                    self.waits[&waiter].1.clone()
                },
                key,
                producer,
                producer_task,
            })
            .collect();
        Some(WaitCycle { steps })
    }

    /// The key `task` actively waits on, if any.
    pub fn waiting(&self, task: TaskId) -> Option<DependencyKey> {
        self.waits.get(&task).map(|(key, _)| *key)
    }

    /// Where `task` demanded the key it actively waits on.
    pub fn waiting_site(&self, task: TaskId) -> Option<&DemandSite> {
        self.waits.get(&task).map(|(_, site)| site)
    }

    /// The tasks actively waiting on `key`, in task order.
    pub fn waiters(&self, key: DependencyKey) -> Vec<TaskId> {
        let mut tasks: Vec<TaskId> = self
            .waiters
            .get(&key)
            .map(|w| w.iter().copied().collect())
            .unwrap_or_default();
        tasks.sort();
        tasks
    }

    /// The number of active waits.
    pub fn active_waits(&self) -> usize {
        self.waits.len()
    }

    /// `task` no longer waits (it was cancelled, woken, or its wait was
    /// refused): remove its active wait, keeping its recorded uses.
    pub fn withdraw(&mut self, task: TaskId) {
        if let Some((key, _)) = self.waits.remove(&task)
            && let Some(waiters) = self.waiters.get_mut(&key)
        {
            waiters.remove(&task);
            if waiters.is_empty() {
                self.waiters.remove(&key);
            }
        }
    }

    /// `key` has (or is about to get) a terminal outcome: remove every active
    /// wait on it and its producer binding. Recorded uses stay. Later waits on
    /// `key` add no edge.
    pub fn resolve(&mut self, key: DependencyKey) {
        self.resolved.insert(key);
        self.producers.remove(&key);
        for task in self.waiters.remove(&key).unwrap_or_default() {
            self.waits.remove(&task);
        }
    }

    /// True once `key` has been resolved.
    pub fn is_resolved(&self, key: DependencyKey) -> bool {
        self.resolved.contains(&key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{DependencyId, ResultKind, WorldId};

    fn key(def: u64) -> DependencyKey {
        DependencyKey::new(WorldId::INITIAL, DependencyId::new(def), ResultKind::new(3))
    }

    fn t(id: u64) -> TaskId {
        TaskId::from_raw(id)
    }

    fn p(id: u64) -> ProducerId {
        ProducerId::from_raw(id)
    }

    fn site(s: &str) -> DemandSite {
        DemandSite::new(s)
    }

    #[test]
    fn a_two_task_cycle_is_refused_with_its_path() {
        let mut g = DependencyGraph::new();
        // Task 1 produces key 1, task 2 produces key 2.
        g.bind_producer(key(1), p(1), t(1));
        g.bind_producer(key(2), p(2), t(2));
        g.wait(t(1), key(2), site("a needs b")).unwrap();
        let cycle = g.wait(t(2), key(1), site("b needs a")).unwrap_err();
        assert_eq!(
            cycle.steps(),
            [
                WaitStep {
                    task: t(2),
                    site: site("b needs a"),
                    key: key(1),
                    producer: p(1),
                    producer_task: t(1),
                },
                WaitStep {
                    task: t(1),
                    site: site("a needs b"),
                    key: key(2),
                    producer: p(2),
                    producer_task: t(2),
                },
            ]
        );
        assert_eq!(
            cycle.to_string(),
            "wait cycle of 2 demand(s): task#2 (b needs a) waits on world#0/dep#1/kind#3, \
             produced by producer#1 on task#1 -> task#1 (a needs b) waits on \
             world#0/dep#2/kind#3, produced by producer#2 on task#2"
        );
        // The refused edge was not added; the use was recorded.
        assert_eq!(g.waiting(t(2)), None);
        assert!(g.used(t(2), key(1)));
        assert_eq!(g.active_waits(), 1);
    }

    #[test]
    fn a_self_wait_is_a_cycle_of_one() {
        let mut g = DependencyGraph::new();
        g.bind_producer(key(1), p(1), t(1));
        let cycle = g.wait(t(1), key(1), site("a needs a")).unwrap_err();
        assert_eq!(cycle.tasks(), [t(1)]);
        assert_eq!(cycle.keys(), [key(1)]);
    }

    #[test]
    fn chains_without_a_return_edge_are_not_cycles() {
        let mut g = DependencyGraph::new();
        g.bind_producer(key(1), p(1), t(1));
        g.bind_producer(key(2), p(2), t(2));
        // 1 waits on 2, 2 waits on 3 (no known producer): a chain, no cycle.
        g.wait(t(1), key(2), site("1")).unwrap();
        g.wait(t(2), key(3), site("2")).unwrap();
        // A consumer (producing nothing) waiting on 1 closes nothing either.
        g.wait(t(9), key(1), site("consumer")).unwrap();
        assert_eq!(g.active_waits(), 3);
        assert_eq!(g.waiters(key(2)), [t(1)]);
        assert_eq!(g.producer(key(2)), Some((p(2), t(2))));
    }

    #[test]
    fn resolution_removes_waits_and_bindings_but_keeps_uses() {
        let mut g = DependencyGraph::new();
        g.bind_producer(key(1), p(1), t(1));
        g.wait(t(2), key(1), site("x")).unwrap();
        g.wait(t(3), key(1), site("y")).unwrap();
        g.resolve(key(1));
        assert_eq!(g.active_waits(), 0);
        assert_eq!(g.waiters(key(1)), []);
        assert_eq!(g.producer(key(1)), None);
        assert!(g.used(t(2), key(1)) && g.used(t(3), key(1)));
        // A late wait on a resolved key (a need that raced the completion)
        // records the use but adds no edge; nor is the key bound again.
        g.wait(t(4), key(1), site("late")).unwrap();
        g.bind_producer(key(1), p(5), t(5));
        assert_eq!((g.waiting(t(4)), g.producer(key(1))), (None, None));
        assert_eq!(g.uses(t(4)), [key(1)]);
    }

    #[test]
    fn withdrawal_breaks_a_would_be_cycle_and_keeps_uses() {
        let mut g = DependencyGraph::new();
        g.bind_producer(key(1), p(1), t(1));
        g.bind_producer(key(2), p(2), t(2));
        g.wait(t(1), key(2), site("a needs b")).unwrap();
        // Task 1 is cancelled (or woken): its wait goes, its use stays.
        g.withdraw(t(1));
        assert_eq!(g.waiting(t(1)), None);
        assert_eq!(g.uses(t(1)), [key(2)]);
        g.wait(t(2), key(1), site("b needs a")).unwrap();
        assert_eq!(g.waiting_site(t(2)), Some(&site("b needs a")));
        // A ready read is a use with no wait.
        g.record_use(t(1), key(7));
        assert_eq!(g.uses(t(1)), [key(2), key(7)]);
        assert_eq!(g.waiting(t(1)), None);
    }

    #[test]
    fn a_new_wait_replaces_the_tasks_previous_wait() {
        let mut g = DependencyGraph::new();
        g.wait(t(1), key(1), site("first")).unwrap();
        g.wait(t(1), key(2), site("second")).unwrap();
        assert_eq!(g.waiting(t(1)), Some(key(2)));
        assert_eq!(g.waiters(key(1)), []);
        assert_eq!(g.uses(t(1)), [key(1), key(2)]);
    }

    #[test]
    fn a_demand_site_shows_its_source_location() {
        let location = SourceLocation {
            span: crate::frontend::Span { start: 4, end: 9 },
            line: 2,
            column: 5,
        };
        assert_eq!(site("need b").at(location).to_string(), "need b at 2:5");
    }
}
