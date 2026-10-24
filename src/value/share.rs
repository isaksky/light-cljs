//! Publication rules: which values may be handed from one task to another
//! (`docs/design.md` §4).
//!
//! # Categories
//!
//! Every heap object kind declares one [`Shareability`] category:
//!
//! | Category | Kinds today | Shareable? |
//! |----------|-------------|------------|
//! | [`Shareability::Persistent`] | vector, map, occurrence; natives that declare it | when everything it references is shareable (transitively) |
//! | [`Shareability::TaskOwned`] | transient builders (any state), plain objects, natives that declare it | never |
//! | [`Shareability::ConcurrentReference`] | none yet: reserved for atoms (clj-quickjs-10) and dependency cells (clj-quickjs-6) | always; not descended into |
//!
//! Primitives are always shareable. Plain objects are the ordinary mutable
//! JavaScript objects of `design.md` §4: they have no property storage yet,
//! but they are classified as task-owned mutation now so a collection that
//! holds one is not silently shareable once properties exist. A builder
//! stays task-owned after `freeze` or release: the persistent result of a
//! freeze is a separate object, the builder handle itself never becomes
//! shareable. A concurrent reference is shareable without being immutable
//! because its own operations (set, compare-and-swap, single-assignment
//! completion) run this check on every value they install; the walk
//! therefore stops at it instead of reading contents that may change.
//!
//! The classification is the exhaustive match in [`classify`]: adding a
//! heap object kind does not compile until it declares a category.
//!
//! # What a later object kind must declare
//!
//! * Its category, in [`classify`] (natives: [`NativePayload::shareability`]).
//!   The category is fixed for the object's lifetime.
//! * If `Persistent`: its edges must never change after allocation (every
//!   direct reference reported by its `Trace` is part of its value), and it
//!   should be allocated with [`Heap::alloc_persistent`] so the cache below
//!   stays effective. Closures (clj-quickjs-20) are `Persistent` only when
//!   their captured environment is immutable; a closure over mutable
//!   bindings is `TaskOwned`.
//! * If `ConcurrentReference`: every operation that installs a value in it
//!   calls [`check`] on that value first and rejects it on failure.
//! * Otherwise `TaskOwned`.
//!
//! # The check
//!
//! [`check`] walks the graph reachable from a value breadth-first with an
//! explicit queue (no recursion, so depth is limited only by memory) and a
//! visited set (so cycles, which can exist through task-owned objects and
//! native payloads, terminate and every object is inspected at most once).
//! It reads each object under one short object-table lock, copying out the
//! collection body (an `Arc` clone) and walking it with no lock held. It
//! fails at the first task-owned object it meets, with a [`PublishError`]
//! naming the path from the published value to that object (breadth-first,
//! so a shortest such path) and the object's kind and owner.
//!
//! No mutator token is needed: the published value is rooted by the caller
//! and the walk descends only through persistent objects, whose edges never
//! change, so everything it visits stays reachable for the whole walk.
//!
//! # Cache: the sticky known-shareable bit
//!
//! Each heap slot carries a known-shareable bit. It is set
//!
//! * **at allocation** of a persistent object whose parts are all already
//!   known shareable ([`Heap::alloc_persistent`]): a new vector or map from
//!   known parts, an `assoc`/`conj` of a known collection with a known
//!   value, a `dissoc`/`pop` of a known collection, a `freeze` whose draft
//!   holds only known values. This costs O(parts), never a graph walk, so
//!   graphs built bottom-up from shareable parts are known shareable as soon
//!   as they exist, including every new version derived from them;
//! * **after a successful check**, on every persistent object the walk
//!   expanded (an update that removes the only task-owned element, for
//!   example, yields a collection not yet known shareable).
//!
//! Invariant: the bit is set only on objects whose whole reachable graph is
//! shareable, and stays true while the object lives, because a persistent
//! object's edges never change and no object's category ever changes. The
//! walk stops at known objects, so publishing the same value again, or any
//! version derived from it with known parts, is O(1). The bit is rewritten
//! whenever a slot is reused, so it never describes a reclaimed object.
//!
//! # Entry point
//!
//! [`Runtime::publish`](crate::Runtime::publish) is the embedding's single
//! publication entry point: the embedder calls it before handing a value
//! from one task to another (for example a task result passed as another
//! task's argument). Runtime handoffs added by later pellets (dependency
//! results, atom contents, closure captures) call [`check`] directly.
//! `Runtime::spawn` itself does not check its arguments; see its docs.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, VecDeque};
use std::fmt;

use crate::collections::{PMap, PVector, object_name};
use crate::heap::{HandleError, Heap, HeapObject, ObjectId};
use crate::ids::TaskId;
use crate::value::{Primitive, Value};

#[cfg(doc)]
use crate::heap::NativePayload;

/// The publication category of a heap object kind. See the module docs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shareability {
    /// Immutable: shareable exactly when everything it references is.
    Persistent,
    /// Task-owned mutation (builders, plain objects, mutable natives):
    /// never shareable.
    TaskOwned,
    /// An explicit concurrent reference: shareable without being immutable.
    /// Reserved; no kind uses it outside tests yet.
    #[cfg_attr(not(test), allow(dead_code))]
    ConcurrentReference,
}

/// The category of `object`. Exhaustive on purpose: every new kind must
/// declare one (module docs, "What a later object kind must declare").
pub(crate) fn classify(object: &HeapObject) -> Shareability {
    match object {
        HeapObject::Plain => Shareability::TaskOwned,
        HeapObject::Native(payload) => payload.shareability(),
        HeapObject::Vector(_) | HeapObject::Map(_) | HeapObject::Occurrence(_) => {
            Shareability::Persistent
        }
        HeapObject::Builder(_) => Shareability::TaskOwned,
    }
}

/// The task that owns a task-owned object, when it has one.
fn owner(object: &HeapObject) -> Option<TaskId> {
    match object {
        HeapObject::Builder(b) => Some(b.owner()),
        HeapObject::Plain
        | HeapObject::Native(_)
        | HeapObject::Vector(_)
        | HeapObject::Map(_)
        | HeapObject::Occurrence(_) => None,
    }
}

/// A map key as named in a [`PathStep`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KeyName {
    /// A primitive key, by value.
    Primitive(Primitive),
    /// A heap-object key (keys compare by identity), by its kind name.
    Object(&'static str),
}

impl fmt::Display for KeyName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeyName::Primitive(Primitive::Undefined) => f.write_str("undefined"),
            KeyName::Primitive(Primitive::Null) => f.write_str("null"),
            KeyName::Primitive(Primitive::Bool(b)) => write!(f, "{b}"),
            KeyName::Primitive(Primitive::Number(n)) if n.is_infinite() => {
                f.write_str(if *n > 0.0 { "Infinity" } else { "-Infinity" })
            }
            KeyName::Primitive(Primitive::Number(n)) => write!(f, "{n}"),
            KeyName::Object(kind) => write!(f, "<{kind}>"),
        }
    }
}

/// One step from a value to something it references.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathStep {
    /// The element of a vector at an index. Displayed `[i]`.
    Index(usize),
    /// The value of a map entry, by its key. Displayed `{key}`.
    MapValue(KeyName),
    /// The key of a map entry itself. Displayed `{key}.key`.
    MapKey(KeyName),
    /// The value an occurrence wraps. Displayed `.value`.
    OccurrenceValue,
    /// The n-th edge a native payload reports. Displayed `.edge(n)`.
    NativeEdge(usize),
}

/// A path from a published value to one object it reaches. Displayed as
/// `value` followed by its steps, e.g. `value[2]{1}.value`.
#[derive(Clone, Debug, PartialEq)]
pub struct ValuePath(Vec<PathStep>);

impl ValuePath {
    /// The steps, from the published value outwards.
    pub fn steps(&self) -> &[PathStep] {
        &self.0
    }
}

impl fmt::Display for ValuePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("value")?;
        for step in &self.0 {
            match step {
                PathStep::Index(i) => write!(f, "[{i}]")?,
                PathStep::MapValue(k) => write!(f, "{{{k}}}")?,
                PathStep::MapKey(k) => write!(f, "{{{k}}}.key")?,
                PathStep::OccurrenceValue => f.write_str(".value")?,
                PathStep::NativeEdge(n) => write!(f, ".edge({n})")?,
            }
        }
        Ok(())
    }
}

/// Why a value could not be published.
#[derive(Clone, Debug, PartialEq)]
pub enum PublishError {
    /// The handle does not belong to this runtime.
    Handle(HandleError),
    /// The value reaches an object that is not shareable (task-owned
    /// mutation: a transient builder, a plain object, or a mutable native).
    NotShareable {
        /// Where the object sits inside the published value.
        path: ValuePath,
        /// The object's kind (`vector builder`, `object`, ...).
        found: &'static str,
        /// The owning task, for builders.
        owner: Option<TaskId>,
    },
}

impl fmt::Display for PublishError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PublishError::Handle(e) => write!(f, "cannot publish: {e}"),
            PublishError::NotShareable { path, found, owner } => {
                write!(f, "cannot publish: {path} is a task-owned {found}")?;
                if let Some(owner) = owner {
                    write!(f, " (owned by {owner})")?;
                }
                f.write_str(", which is not shareable")
            }
        }
    }
}

impl std::error::Error for PublishError {}

/// A step as recorded during the walk: keys stay raw values until an error
/// needs their names, so the walk never takes a lock to name a key.
#[derive(Clone, Copy, Debug)]
enum Step {
    Index(usize),
    MapValue(Value),
    MapKey(Value),
    OccurrenceValue,
    NativeEdge(usize),
}

/// What one object contributes to the walk.
enum Node {
    /// Known shareable (cached bit, or a concurrent reference): stop here.
    Shareable,
    /// Task-owned: the walk fails here.
    Rejected {
        found: &'static str,
        owner: Option<TaskId>,
    },
    /// Persistent and not yet known: expand its references.
    Expand(Children),
}

/// The references of a persistent object, copied out of the heap.
enum Children {
    Vector(PVector),
    Map(PMap),
    Occurrence(Value),
    Native(Vec<ObjectId>),
}

impl Children {
    fn for_each(&self, mut f: impl FnMut(Step, Value)) {
        match self {
            Children::Vector(vec) => {
                let mut index = 0;
                vec.for_each(|v| {
                    f(Step::Index(index), v);
                    index += 1;
                });
            }
            Children::Map(map) => map.for_each(|k, v| {
                f(Step::MapKey(k), k);
                f(Step::MapValue(k), v);
            }),
            Children::Occurrence(v) => f(Step::OccurrenceValue, *v),
            Children::Native(edges) => {
                for (n, &id) in edges.iter().enumerate() {
                    f(Step::NativeEdge(n), Value::Object(id));
                }
            }
        }
    }
}

/// Inspect one object under the object lock: copy out only what the walk
/// needs.
fn inspect(object: &HeapObject, known: bool) -> Node {
    if known {
        return Node::Shareable;
    }
    match classify(object) {
        Shareability::ConcurrentReference => Node::Shareable,
        Shareability::TaskOwned => Node::Rejected {
            found: object_name(object),
            owner: owner(object),
        },
        Shareability::Persistent => Node::Expand(match object {
            HeapObject::Vector(vec) => Children::Vector(vec.clone()),
            HeapObject::Map(map) => Children::Map(map.clone()),
            HeapObject::Occurrence(occ) => Children::Occurrence(occ.value),
            other => {
                let mut edges = Vec::new();
                crate::heap::gc::Trace::trace(other, &mut |id| edges.push(id));
                Children::Native(edges)
            }
        }),
    }
}

/// Check that `value` may be published: every object it reaches is
/// persistent with shareable contents, or a concurrent reference. On success
/// every persistent object the walk expanded is marked known shareable.
/// See the module docs.
///
/// The caller keeps `value` alive (a root, a scope local, or a frame).
pub(crate) fn check(heap: &Heap, value: Value) -> Result<(), PublishError> {
    check_counted(heap, value, &mut 0)
}

/// [`check`], counting the objects whose references it expands.
fn check_counted(heap: &Heap, value: Value, expanded: &mut usize) -> Result<(), PublishError> {
    let Value::Object(root) = value else {
        return Ok(());
    };
    // Each discovered object maps to the step that first reached it.
    let mut parents: HashMap<ObjectId, Option<(ObjectId, Step)>> = HashMap::new();
    parents.insert(root, None);
    let mut queue = VecDeque::from([root]);
    let mut proven = Vec::new();
    while let Some(id) = queue.pop_front() {
        let node = heap.read_shareable(id, inspect).unwrap_or(Node::Rejected {
            // Unreachable while the caller roots `value` (module docs);
            // reported rather than trusted.
            found: "reclaimed object",
            owner: None,
        });
        match node {
            Node::Shareable => {}
            Node::Rejected { found, owner } => {
                return Err(PublishError::NotShareable {
                    path: path_to(heap, &parents, id),
                    found,
                    owner,
                });
            }
            Node::Expand(children) => {
                *expanded += 1;
                proven.push(id);
                children.for_each(|step, child| {
                    if let Value::Object(child) = child
                        && let Entry::Vacant(slot) = parents.entry(child)
                    {
                        slot.insert(Some((id, step)));
                        queue.push_back(child);
                    }
                });
            }
        }
    }
    heap.mark_shareable(&proven);
    Ok(())
}

/// The path from the walk's root to `id`, following first-discovery links.
fn path_to(
    heap: &Heap,
    parents: &HashMap<ObjectId, Option<(ObjectId, Step)>>,
    mut id: ObjectId,
) -> ValuePath {
    let mut steps = Vec::new();
    while let Some(&Some((parent, step))) = parents.get(&id) {
        steps.push(name_step(heap, step));
        id = parent;
    }
    steps.reverse();
    ValuePath(steps)
}

fn name_step(heap: &Heap, step: Step) -> PathStep {
    let key = |k: Value| match k {
        Value::Object(id) => {
            KeyName::Object(heap.read(id, object_name).unwrap_or("reclaimed object"))
        }
        other => KeyName::Primitive(other.to_primitive().unwrap_or(Primitive::Undefined)),
    };
    match step {
        Step::Index(i) => PathStep::Index(i),
        Step::MapValue(k) => PathStep::MapValue(key(k)),
        Step::MapKey(k) => PathStep::MapKey(key(k)),
        Step::OccurrenceValue => PathStep::OccurrenceValue,
        Step::NativeEdge(n) => PathStep::NativeEdge(n),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::collections::builder::{self, BuilderKind};
    use crate::collections::{self as coll, OwnedBuilders, Provenance};
    use crate::heap::gc::Trace;
    use crate::heap::{NativePayload, Scope};

    fn n(x: f64) -> Value {
        Value::Number(x)
    }

    fn id(v: Value) -> ObjectId {
        match v {
            Value::Object(id) => id,
            other => panic!("not an object: {other:?}"),
        }
    }

    fn plain(heap: &Heap) -> Value {
        Value::Object(heap.alloc(HeapObject::Plain).unwrap())
    }

    fn known(heap: &Heap, v: Value) -> bool {
        heap.known_shareable(id(v)).expect("live object")
    }

    /// Check `v` and return how many objects the walk expanded.
    fn expansions(heap: &Heap, v: Value) -> Result<usize, PublishError> {
        let mut expanded = 0;
        check_counted(heap, v, &mut expanded).map(|()| expanded)
    }

    /// Test payload with a declared category and settable edges.
    #[derive(Debug)]
    struct TestNative {
        class: Shareability,
        edges: Vec<Value>,
    }

    impl Trace for TestNative {
        fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
            self.edges.trace(visit);
        }
    }

    impl NativePayload for TestNative {
        fn shareability(&self) -> Shareability {
            self.class
        }
    }

    fn native(heap: &Heap, class: Shareability, edges: Vec<Value>) -> Value {
        let payload = TestNative { class, edges };
        Value::Object(heap.alloc(HeapObject::Native(Box::new(payload))).unwrap())
    }

    /// Two-phase construction of a cycle, before anything is checked (a
    /// persistent payload's edges are fixed once it can be published).
    fn set_edges(heap: &Heap, v: Value, edges: Vec<Value>) {
        heap.with_object_mut(id(v), |o| match o {
            HeapObject::Native(p) => {
                let any: &mut dyn std::any::Any = &mut **p;
                any.downcast_mut::<TestNative>().unwrap().edges = edges;
            }
            other => panic!("not a native: {other:?}"),
        })
        .unwrap();
    }

    const OWNER: TaskId = TaskId::from_raw(1);

    fn vector_builder(heap: &Heap, items: &[Value]) -> Value {
        let b = builder::new_builder(
            heap,
            OWNER,
            BuilderKind::Vector,
            &mut OwnedBuilders::default(),
        )
        .unwrap();
        for &v in items {
            builder::conj(heap, OWNER, b, v).unwrap();
        }
        b
    }

    #[test]
    fn every_kind_declares_its_category() {
        let heap = Arc::new(Heap::default());
        let _scope = Scope::new(&heap);
        let vec = coll::new_vector(&heap, &[n(1.0)]).unwrap();
        let map = coll::new_map(&heap, &[(n(1.0), n(2.0))]).unwrap();
        let occ = coll::new_occurrence(&heap, n(1.0), Provenance::default()).unwrap();
        let open = vector_builder(&heap, &[]);
        let frozen = vector_builder(&heap, &[]);
        builder::freeze(&heap, OWNER, frozen).unwrap();
        let class = |v| heap.read(id(v), classify).unwrap();
        assert_eq!(class(plain(&heap)), Shareability::TaskOwned);
        for persistent in [vec, map, occ] {
            assert_eq!(class(persistent), Shareability::Persistent);
        }
        assert_eq!(class(open), Shareability::TaskOwned);
        assert_eq!(
            class(frozen),
            Shareability::TaskOwned,
            "a frozen builder stays task-owned"
        );
        for declared in [
            Shareability::Persistent,
            Shareability::TaskOwned,
            Shareability::ConcurrentReference,
        ] {
            assert_eq!(class(native(&heap, declared, vec![])), declared);
        }
        assert_eq!(heap.read(id(open), owner).unwrap(), Some(OWNER));
    }

    #[test]
    fn values_built_from_shareable_parts_are_known_at_allocation() {
        let heap = Arc::new(Heap::default());
        let _scope = Scope::new(&heap);
        let obj = plain(&heap);
        let vec = coll::new_vector(&heap, &[n(1.0), n(2.0)]).unwrap();
        let map = coll::new_map(&heap, &[(vec, vec), (n(1.0), n(2.0))]).unwrap();
        let occ = coll::new_occurrence(&heap, map, Provenance::default()).unwrap();
        let holder = coll::new_vector(&heap, &[occ]).unwrap();
        let grown = coll::conj(&heap, holder, n(3.0)).unwrap();
        for v in [vec, map, occ, holder, grown] {
            assert!(known(&heap, v));
            assert_eq!(expansions(&heap, v), Ok(0), "no walk for a known value");
        }
        // Derived versions: known exactly when every new part is known.
        assert!(known(
            &heap,
            coll::assoc(&heap, vec, n(0.0), n(9.0)).unwrap()
        ));
        assert!(known(&heap, coll::assoc(&heap, map, n(5.0), vec).unwrap()));
        assert!(known(&heap, coll::dissoc(&heap, map, n(1.0)).unwrap()));
        assert!(known(&heap, coll::pop(&heap, vec).unwrap()));
        let tainted = coll::assoc(&heap, vec, n(0.0), obj).unwrap();
        assert!(!known(&heap, tainted));
        assert!(!known(&heap, coll::new_vector(&heap, &[tainted]).unwrap()));
        assert!(!known(&heap, coll::assoc(&heap, map, obj, n(1.0)).unwrap()));
        // Removing the task-owned part is not tracked at allocation (the
        // parent was not known), so the check walks it once and caches it.
        let cleaned = coll::assoc(&heap, tainted, n(0.0), n(1.0)).unwrap();
        assert!(!known(&heap, cleaned));
        assert_eq!(expansions(&heap, cleaned), Ok(1));
        assert!(known(&heap, cleaned));
        // Freezing: known when the draft holds only known values.
        let good = vector_builder(&heap, &[vec, n(1.0)]);
        assert!(known(&heap, builder::freeze(&heap, OWNER, good).unwrap()));
        let bad = vector_builder(&heap, &[vec, obj]);
        assert!(!known(&heap, builder::freeze(&heap, OWNER, bad).unwrap()));
        // A concurrent reference is a known part even though it is not
        // immutable.
        let cell = native(&heap, Shareability::ConcurrentReference, vec![obj]);
        assert!(known(&heap, coll::new_vector(&heap, &[cell]).unwrap()));
    }

    #[test]
    fn repeated_publication_does_not_rewalk_the_graph() {
        let heap = Arc::new(Heap::default());
        let _scope = Scope::new(&heap);
        // An empty vector not known shareable: popped from one holding an
        // object. Everything built on it is unknown until checked.
        let obj = plain(&heap);
        let base = coll::pop(&heap, coll::new_vector(&heap, &[obj]).unwrap()).unwrap();
        assert!(!known(&heap, base));
        let inner: Vec<Value> = (0..1000)
            .map(|i| coll::new_vector(&heap, &[base, n(f64::from(i))]).unwrap())
            .collect();
        let root = coll::new_vector(&heap, &inner).unwrap();
        assert_eq!(expansions(&heap, root), Ok(1002), "root, 1000 inner, base");
        assert_eq!(expansions(&heap, root), Ok(0), "second publication: O(1)");
        assert!(inner.iter().all(|&v| known(&heap, v)));
        // New versions of a published value are known at allocation.
        let next = coll::conj(&heap, root, n(1.0)).unwrap();
        assert_eq!(expansions(&heap, next), Ok(0));
        let wrapped = coll::new_map(&heap, &[(n(1.0), root)]).unwrap();
        assert_eq!(expansions(&heap, wrapped), Ok(0));
    }

    #[test]
    fn cycles_terminate_and_expand_each_object_once() {
        let heap = Arc::new(Heap::default());
        let _scope = Scope::new(&heap);
        // a <-> b, plus a self-loop on b: persistent natives.
        let a = native(&heap, Shareability::Persistent, vec![]);
        let b = native(&heap, Shareability::Persistent, vec![a]);
        set_edges(&heap, a, vec![b, n(1.0)]);
        set_edges(&heap, b, vec![a, b]);
        let root = coll::new_vector(&heap, &[a, b, a]).unwrap();
        assert!(!known(&heap, root));
        assert_eq!(expansions(&heap, root), Ok(3));
        assert!(known(&heap, a) && known(&heap, b) && known(&heap, root));

        // A cycle with a builder behind it: rejected with the shortest path.
        let c = native(&heap, Shareability::Persistent, vec![]);
        let d = native(&heap, Shareability::Persistent, vec![c]);
        let builder = vector_builder(&heap, &[c]);
        set_edges(&heap, c, vec![d]);
        set_edges(&heap, d, vec![c, builder]);
        let err = check(&heap, c).unwrap_err();
        assert_eq!(
            err,
            PublishError::NotShareable {
                path: ValuePath(vec![PathStep::NativeEdge(0), PathStep::NativeEdge(1)]),
                found: "vector builder",
                owner: Some(OWNER),
            }
        );
        assert_eq!(
            err.to_string(),
            "cannot publish: value.edge(0).edge(1) is a task-owned vector builder \
             (owned by task#1), which is not shareable"
        );
        assert!(
            !known(&heap, c) && !known(&heap, d),
            "nothing cached on failure"
        );
    }

    #[test]
    fn concurrent_references_are_shareable_leaves_and_mutable_natives_are_not() {
        let heap = Arc::new(Heap::default());
        let _scope = Scope::new(&heap);
        let obj = plain(&heap);
        // The reference's own operations check what they store; the walk
        // does not read its (changeable) contents.
        let cell = native(&heap, Shareability::ConcurrentReference, vec![obj]);
        assert_eq!(check(&heap, cell), Ok(()));
        let mutable = native(&heap, Shareability::TaskOwned, vec![]);
        let occ = coll::new_occurrence(&heap, mutable, Provenance::default()).unwrap();
        let root = coll::new_map(&heap, &[(n(2.0), occ)]).unwrap();
        assert_eq!(
            check(&heap, root),
            Err(PublishError::NotShareable {
                path: ValuePath(vec![
                    PathStep::MapValue(KeyName::Primitive(Primitive::Number(2.0))),
                    PathStep::OccurrenceValue,
                ]),
                found: "native object",
                owner: None,
            })
        );
    }

    #[test]
    fn task_owned_map_keys_are_named_by_kind() {
        let heap = Arc::new(Heap::default());
        let _scope = Scope::new(&heap);
        let b = vector_builder(&heap, &[]);
        let map = coll::new_map(&heap, &[(n(1.0), n(1.0)), (b, n(2.0))]).unwrap();
        let err = check(&heap, map).unwrap_err();
        let PublishError::NotShareable { path, .. } = &err else {
            panic!("{err:?}")
        };
        assert_eq!(
            path.steps(),
            [PathStep::MapKey(KeyName::Object("vector builder"))]
        );
        assert_eq!(path.to_string(), "value{<vector builder>}.key");
        assert_eq!(check(&heap, n(1.0)), Ok(()), "primitives are shareable");
    }

    #[test]
    fn deep_graphs_are_walked_without_recursion() {
        const DEPTH: usize = 100_000;
        // A small stack: the walk, the allocation-time cache and the error
        // path must not recurse per level.
        std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| {
                let heap = Arc::new(Heap::default());
                let _scope = Scope::new(&heap);
                let nest = |bottom: Value| {
                    (0..DEPTH).fold(bottom, |v, _| coll::new_vector(&heap, &[v]).unwrap())
                };
                let obj = plain(&heap);
                let base = coll::pop(&heap, coll::new_vector(&heap, &[obj]).unwrap()).unwrap();
                let good = nest(base);
                assert_eq!(expansions(&heap, good), Ok(DEPTH + 1));
                assert_eq!(expansions(&heap, good), Ok(0));

                let bad = nest(obj);
                let Err(PublishError::NotShareable { path, found, .. }) = check(&heap, bad) else {
                    panic!("a deep object must be found")
                };
                assert_eq!(found, "object");
                assert_eq!(path.steps().len(), DEPTH);
                assert!(path.steps().iter().all(|s| *s == PathStep::Index(0)));
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
