//! The collector: tracing contract, root registration, and a precise
//! nonmoving stop-the-world mark/sweep (`docs/design.md` §11).
//!
//! # Tracing boundary
//!
//! * Every structure that stores raw `Value`s implements [`Trace`], which
//!   reports each heap reference it holds directly. Implemented for values,
//!   heap objects (including [`NativePayload`](super::NativePayload) edges),
//!   the persistent root table, scope locals, executable constants, and task
//!   VM state.
//! * A **root source** ([`RootSource`]) is a structure outside the heap that
//!   holds values the collector must treat as roots. Sources register a
//!   `Weak` with [`Heap::register_source`](super::Heap::register_source) and
//!   are traced at every collection while alive; dropping the owner
//!   unregisters it. Tasks are root sources today; dependency results, atom
//!   contents, closure environments and pending native states plug in the
//!   same way (or become heap objects reachable from such a source).
//! * [`RootCategory`] names every root source kind; each root is reported
//!   with its category so tests can inspect the root snapshot.
//!
//! # Collection
//!
//! [`Heap::collect`](super::Heap::collect) (embedders:
//! [`Runtime::collect`](crate::Runtime::collect)) marks everything reachable
//! from the roots and sweeps the rest, cycles included. Objects never move.
//!
//! **When collection may run (single-mutator core).** A collection needs a
//! consistent view of every root, so it runs only when no other mutator can
//! hold unrooted values or inaccessible roots:
//!
//! * a **run guard** ([`RunGuard`](crate::RunGuard)) is a registered mutator
//!   from acquisition to drop: its task's frames are borrowed by the run and
//!   the dispatch loop may hold values in Rust locals mid-instruction;
//! * a **scope** ([`Scope`](crate::Scope)) open on another thread is a
//!   mutator: it may have allocated an object it has not stored yet. Scopes
//!   on the collecting thread are fine; their locals are roots.
//!
//! If either kind of mutator exists, `collect` returns
//! [`CollectError::MutatorActive`] without doing anything. It never waits for
//! a mutator. Cooperative safepoints, where a collection request waits for
//! running mutators to reach a poll point, are pellet clj-quickjs-17. While a
//! collection runs it holds the heap locks, so new runs, scopes, allocations
//! and root changes on other threads wait for the (short) pause.
//!
//! This is not a heap lock held throughout VM execution: runs of different
//! tasks proceed in parallel and only take short metadata locks; the mutator
//! count only makes the collector refuse to start.
//!
//! QuickJS reference (edge inventory only): `JS_MarkValue` / `mark_children`
//! in `research/quickjs/quickjs.c` and `JSClassGCMark` in
//! `research/quickjs/quickjs.h` (revision 7d1c13c5d8f8). QuickJS's reference
//! counting, cycle removal and running-stack exemption are not used: here
//! every root, running or suspended, is traced and nothing is counted.

use std::sync::{Arc, Weak};

use super::{Heap, HeapObject, ObjectId, lock};
use crate::value::Value;

/// A source of collector roots. Append-only registry: later pellets add
/// categories at the end (dependency results, atom contents, closure
/// environments, native continuations, ...).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RootCategory {
    /// [`Rooted`](crate::Rooted) handles, including task results.
    PersistentHandle,
    /// Values held by an open [`Scope`](crate::Scope).
    ScopeLocal,
    /// Task-owned VM state: locals and operand stack of not-yet-run, yielded
    /// (and, once safepoints exist, running) tasks.
    TaskFrame,
    /// Constants of the executable a live task runs.
    ExecutableConstant,
}

/// Report every heap reference held by `self`.
///
/// Implementations must visit each [`ObjectId`] stored directly in the
/// structure; they must not allocate on the heap, take heap locks, or run
/// bytecode (the collector calls them while holding the heap locks).
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

impl<T: Trace> Trace for [T] {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        self.iter().for_each(|v| v.trace(visit));
    }
}

/// A registered structure outside the heap whose values are roots. See the
/// module docs. Same restrictions as [`Trace`]; in addition the collector
/// calls it only when no run guard exists, so it may lock state that run
/// guards hold (it must not wait for anything else).
pub(crate) trait RootSource: Send + Sync {
    fn trace_roots(&self, visit: &mut dyn FnMut(RootCategory, ObjectId));
}

/// Registered root sources.
#[derive(Debug, Default)]
pub(crate) struct SourceRegistry {
    sources: Vec<Weak<dyn RootSource>>,
    /// Prune dead entries when the list reaches this length.
    prune_at: usize,
}

impl SourceRegistry {
    pub(crate) fn register(&mut self, source: Weak<dyn RootSource>) {
        if self.sources.len() >= self.prune_at {
            self.sources.retain(|s| s.strong_count() > 0);
            self.prune_at = (self.sources.len() * 2).max(16);
        }
        self.sources.push(source);
    }

    /// Strong references to the live sources; dead ones are dropped.
    fn live(&mut self) -> Vec<Arc<dyn RootSource>> {
        let mut live = Vec::with_capacity(self.sources.len());
        self.sources.retain(|s| match s.upgrade() {
            Some(strong) => {
                live.push(strong);
                true
            }
            None => false,
        });
        live
    }
}

/// Collector state, behind the heap's `gc` lock.
#[derive(Debug, Default)]
pub(crate) struct GcState {
    /// Registered mutators: live run guards.
    mutators: usize,
    /// Completed collections.
    pub(crate) collections: u64,
}

/// Registration of one mutator (a run guard); deregisters on drop, including
/// on unwind.
#[derive(Debug)]
pub(crate) struct MutatorToken<'h> {
    heap: &'h Heap,
}

impl Drop for MutatorToken<'_> {
    fn drop(&mut self) {
        lock(&self.heap.gc).mutators -= 1;
    }
}

/// What a completed collection did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CollectStats {
    /// Root references reported (one object may be reported several times).
    pub roots: usize,
    /// Objects found reachable and kept.
    pub live: usize,
    /// Objects reclaimed.
    pub reclaimed: usize,
}

/// Why a collection did not run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollectError {
    /// A run guard is held (by any thread, including the caller), or a scope
    /// is open on another thread. See the [`gc`](crate::heap::gc) module docs.
    MutatorActive,
}

impl std::fmt::Display for CollectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CollectError::MutatorActive => {
                f.write_str("cannot collect while a task run or another thread's scope is active")
            }
        }
    }
}

impl std::error::Error for CollectError {}

impl Heap {
    /// Register a mutator; blocks only while a collection is in progress.
    pub(crate) fn enter_mutator(&self) -> MutatorToken<'_> {
        lock(&self.gc).mutators += 1;
        MutatorToken { heap: self }
    }

    /// Report every root with its category, holding the root, scope and
    /// source locks. The caller holds the `gc` lock with no mutators.
    fn trace_all_roots(
        roots: &super::RootTable,
        scopes: &super::ScopeTable,
        sources: &[Arc<dyn RootSource>],
        visit: &mut dyn FnMut(RootCategory, ObjectId),
    ) {
        roots.trace(&mut |id| visit(RootCategory::PersistentHandle, id));
        scopes.trace(&mut |id| visit(RootCategory::ScopeLocal, id));
        for source in sources {
            source.trace_roots(visit);
        }
    }

    /// Stop-the-world mark/sweep. See the module docs for when it may run.
    pub(crate) fn collect(&self) -> Result<CollectStats, CollectError> {
        let mut gc = lock(&self.gc);
        if gc.mutators > 0 {
            return Err(CollectError::MutatorActive);
        }
        let roots = lock(&self.roots);
        let scopes = lock(&self.scopes);
        if scopes.open_elsewhere(std::thread::current().id()) {
            return Err(CollectError::MutatorActive);
        }
        // Strong references keep sources alive while traced; they are dropped
        // after every heap lock is released.
        let sources = lock(&self.sources).live();

        let mut grey = Vec::new();
        let mut root_count = 0;
        Self::trace_all_roots(&roots, &scopes, &sources, &mut |_, id| {
            root_count += 1;
            grey.push(id);
        });

        let mut objects = lock(&self.objects);
        let mut live = 0;
        while let Some(id) = grey.pop() {
            if objects.mark(id, &mut |child| grey.push(child)) {
                live += 1;
            }
        }
        let garbage: Vec<HeapObject> = objects.sweep();
        let stats = CollectStats {
            roots: root_count,
            live,
            reclaimed: garbage.len(),
        };
        gc.collections += 1;
        drop((objects, scopes, roots, gc));
        // Reclaimed payloads and possibly-last source references are dropped
        // with no heap lock held, so their destructors cannot deadlock.
        drop((garbage, sources));
        Ok(stats)
    }

    /// The current roots with their categories (tests and diagnostics). Same
    /// preconditions as [`Heap::collect`].
    #[cfg(test)]
    pub(crate) fn root_snapshot(&self) -> Result<Vec<(RootCategory, ObjectId)>, CollectError> {
        let gc = lock(&self.gc);
        if gc.mutators > 0 {
            return Err(CollectError::MutatorActive);
        }
        let roots = lock(&self.roots);
        let scopes = lock(&self.scopes);
        if scopes.open_elsewhere(std::thread::current().id()) {
            return Err(CollectError::MutatorActive);
        }
        let sources = lock(&self.sources).live();
        let mut out = Vec::new();
        Self::trace_all_roots(&roots, &scopes, &sources, &mut |c, id| out.push((c, id)));
        drop((scopes, roots, gc));
        drop(sources);
        Ok(out)
    }
}

/// Collector tests over the crate-internal heap API (native payloads, raw
/// identities, root categories). Programs are hand-assembled bytecode: early
/// VM tests, not JavaScript source support.
#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::heap::{NativePayload, ObjectKind, Rooted};
    use crate::value::Primitive;
    use crate::vm::executable::Code;
    use crate::vm::instr::Instr;
    use crate::vm::{Executable, Opcode};
    use crate::{RunOutcome, Runtime};

    /// Test payload: a mutable list of edges, plus an optional drop counter.
    #[derive(Debug, Default)]
    struct Edges {
        edges: Vec<Value>,
        drops: Option<Arc<AtomicUsize>>,
    }

    impl Trace for Edges {
        fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
            self.edges.trace(visit);
        }
    }

    impl NativePayload for Edges {}

    impl Drop for Edges {
        fn drop(&mut self) {
            if let Some(d) = &self.drops {
                d.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    fn native(heap: &Heap, edges: Vec<Value>) -> ObjectId {
        let payload = Edges { edges, drops: None };
        heap.alloc(HeapObject::Native(Box::new(payload))).unwrap()
    }

    fn plain(heap: &Heap) -> ObjectId {
        heap.alloc(HeapObject::Plain).unwrap()
    }

    fn set_edges(heap: &Heap, id: ObjectId, edges: Vec<Value>) {
        heap.with_object_mut(id, |o| match o {
            HeapObject::Native(p) => {
                let any: &mut dyn Any = &mut **p;
                any.downcast_mut::<Edges>().unwrap().edges = edges;
            }
            HeapObject::Plain => panic!("not a native object"),
        })
        .expect("live object");
    }

    fn obj(id: ObjectId) -> Value {
        Value::Object(id)
    }

    fn object_id(value: Value) -> ObjectId {
        match value {
            Value::Object(id) => id,
            other => panic!("expected an object, got {other:?}"),
        }
    }

    #[test]
    fn reachable_objects_keep_identity_and_address() {
        let heap = Arc::new(Heap::default());
        let kept = plain(&heap);
        let _root = Rooted::new(&heap, obj(kept));
        let address = heap.address(kept).unwrap();
        let garbage: Vec<ObjectId> = (0..10).map(|_| plain(&heap)).collect();

        let stats = heap.collect().unwrap();
        assert_eq!((stats.live, stats.reclaimed), (1, 10));
        // Refill the freed slots and grow the table well past its old size.
        let fresh: Vec<ObjectId> = (0..1000).map(|_| plain(&heap)).collect();
        heap.collect().unwrap();

        assert!(heap.is_live(kept));
        assert_eq!(heap.address(kept), Some(address), "objects never move");
        assert_eq!(heap.kind(kept), Some(ObjectKind::PlainObject));
        assert!(garbage.iter().all(|&g| !heap.is_live(g)));
        assert!(fresh.iter().all(|&f| !heap.is_live(f)));
        assert_eq!(heap.stats().objects, 1);
        assert_eq!(heap.stats().collections, 2);
    }

    #[test]
    fn unreachable_cycles_are_reclaimed() {
        let heap = Arc::new(Heap::default());
        let a = native(&heap, vec![]);
        let b = native(&heap, vec![obj(a)]);
        set_edges(&heap, a, vec![obj(b)]); // a <-> b
        let selfish = native(&heap, vec![]);
        set_edges(&heap, selfish, vec![obj(selfish)]); // selfish -> selfish

        let root = Rooted::new(&heap, obj(a));
        let stats = heap.collect().unwrap();
        assert_eq!((stats.live, stats.reclaimed), (2, 1), "self-cycle freed");
        assert!(heap.is_live(a) && heap.is_live(b));

        drop(root);
        let stats = heap.collect().unwrap();
        assert_eq!(
            (stats.live, stats.reclaimed),
            (0, 2),
            "two-node cycle freed"
        );
        assert!(!heap.is_live(a) && !heap.is_live(b));
        assert_eq!(heap.stats().objects, 0);
    }

    #[test]
    fn native_payload_edges_are_traced() {
        let heap = Arc::new(Heap::default());
        let leaf = plain(&heap);
        let deep = plain(&heap);
        let inner = native(&heap, vec![obj(deep), Value::Number(1.0)]);
        let outer = native(&heap, vec![obj(leaf), obj(inner)]);
        let _root = Rooted::new(&heap, obj(outer));

        let stats = heap.collect().unwrap();
        assert_eq!((stats.live, stats.reclaimed), (4, 0));
        assert!([leaf, deep, inner, outer].iter().all(|&o| heap.is_live(o)));
        assert_eq!(heap.kind(outer), Some(ObjectKind::Native));

        // Dropping an edge from the payload makes its subtree unreachable.
        set_edges(&heap, outer, vec![obj(leaf)]);
        let stats = heap.collect().unwrap();
        assert_eq!((stats.live, stats.reclaimed), (2, 2));
        assert!(heap.is_live(leaf) && heap.is_live(outer));
        assert!(!heap.is_live(inner) && !heap.is_live(deep));
    }

    #[test]
    fn reclaimed_payloads_are_dropped() {
        let heap = Arc::new(Heap::default());
        let drops = Arc::new(AtomicUsize::new(0));
        for _ in 0..3 {
            let payload = Edges {
                edges: vec![],
                drops: Some(Arc::clone(&drops)),
            };
            heap.alloc(HeapObject::Native(Box::new(payload))).unwrap();
        }
        assert_eq!(heap.collect().unwrap().reclaimed, 3);
        assert_eq!(drops.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn stale_ids_are_rejected_after_slot_reuse() {
        let heap = Arc::new(Heap::default());
        let old = native(&heap, vec![]);
        heap.collect().unwrap();
        let new = plain(&heap);
        assert_eq!(new.index, old.index, "the freed slot is reused");
        assert_ne!(new.generation, old.generation);

        assert!(!heap.is_live(old));
        assert_eq!(heap.kind(old), None);
        assert_eq!(heap.address(old), None);
        assert!(heap.with_object_mut(old, |_| ()).is_none());
        assert_eq!(heap.root_if_live(obj(old)), None, "cannot be re-rooted");
        assert_eq!(heap.kind(new), Some(ObjectKind::PlainObject));

        // Even a stale id that reached a root is ignored, never followed or
        // confused with the slot's new occupant.
        let _stale_root = Rooted::new(&heap, obj(old));
        let stats = heap.collect().unwrap();
        assert_eq!((stats.roots, stats.live, stats.reclaimed), (1, 0, 1));
        assert!(!heap.is_live(new));
    }

    /// `return constant[0]`, with an object constant. Loaded code only has
    /// primitive constants in this slice; this builds `Code` directly to
    /// exercise the constant-pool edge.
    fn spawn_with_constant(rt: &Runtime, constant: ObjectId, args: &[&Rooted]) -> crate::Task {
        let code = Code {
            name: "object-constant".into(),
            params: 1,
            locals: 1,
            constants: Box::new([obj(constant)]),
            offsets: Box::new([0, 3]),
            instrs: Box::new([Instr::Const(0), Instr::Return]),
            max_stack: 1,
        };
        rt.spawn(&Executable(Arc::new(code)), args).unwrap()
    }

    #[test]
    fn root_snapshot_reports_every_category() {
        let rt = Runtime::new();
        let heap = rt.heap();
        let held = rt.scope(|s| {
            let o = s.new_object();
            s.persist(o)
        });
        let held_id = object_id(held.value_in(heap).unwrap());
        let constant = plain(heap);
        let frame_obj = plain(heap);
        let arg = Rooted::new(heap, obj(frame_obj));
        let task = spawn_with_constant(&rt, constant, &[&arg]);
        drop(arg);
        rt.scope(|s| {
            let local_id = object_id(s.raw(s.new_object()));
            let mut snapshot = heap.root_snapshot().unwrap();
            snapshot.sort_by_key(|&(_, id)| id);
            let mut expected = vec![
                (RootCategory::PersistentHandle, held_id),
                (RootCategory::ScopeLocal, local_id),
                (RootCategory::ExecutableConstant, constant),
                (RootCategory::TaskFrame, frame_obj),
            ];
            expected.sort_by_key(|&(_, id)| id);
            assert_eq!(snapshot, expected);
        });
        drop(task);
    }

    #[test]
    fn executable_constants_are_roots_while_a_task_holds_the_code() {
        let rt = Runtime::new();
        let heap = rt.heap();
        let constant = plain(heap);
        let task = spawn_with_constant(&rt, constant, &[]);
        rt.collect().unwrap();
        assert!(heap.is_live(constant));
        let result = match task.run().unwrap() {
            RunOutcome::Complete(r) => r,
            other => panic!("{other:?}"),
        };
        drop(task);
        rt.collect().unwrap();
        assert!(heap.is_live(constant), "now held by the result root");
        drop(result);
        rt.collect().unwrap();
        assert!(!heap.is_live(constant));
    }

    #[test]
    fn finished_and_dropped_tasks_stop_being_roots() {
        let rt = Runtime::new();
        let heap = rt.heap();
        let mut asm = crate::Assembler::new();
        asm.op(Opcode::NewObject).set_local(0);
        asm.constant(Primitive::Null).op(Opcode::Return);
        let exe = asm.finish("drop-local", 0, 1).unwrap();

        // Finished: the frames are released at completion.
        let task = rt.spawn(&exe, &[]).unwrap();
        assert!(matches!(task.run().unwrap(), RunOutcome::Complete(_)));
        assert_eq!(heap.stats().objects, 1);
        assert_eq!(rt.collect().unwrap().reclaimed, 1);

        // Dropped while yielded: the source registry forgets the task.
        let task = rt.spawn(&exe, &[]).unwrap();
        let outcome = task
            .run_with(crate::Budget::instructions(2), &mut crate::host::NoHost)
            .unwrap();
        assert!(matches!(outcome, RunOutcome::Yielded));
        assert_eq!(rt.collect().unwrap().reclaimed, 0, "held by saved locals");
        drop(task);
        assert_eq!(rt.collect().unwrap().reclaimed, 1);
        assert!(heap.root_snapshot().unwrap().is_empty());
    }
}
