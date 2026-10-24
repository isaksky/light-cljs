//! Publication rules: which values may be handed from one task to another
//! (`docs/design.md` §4).
//!
//! # Categories
//!
//! Every heap object kind declares one [`Shareability`] category:
//!
//! | Category | Kinds today | Shareable? |
//! |----------|-------------|------------|
//! | [`Shareability::Persistent`] | vector, map, occurrence, function (closure; its own-property record is a separate ordinary object), built-in function, realm intrinsic ordinary object (`Object.prototype`, `crate::realm`), internal frozen template arrays; natives that declare it | when everything it references is shareable (transitively) |
//! | [`Shareability::TaskOwned`] | transient builders (any state), ordinary objects other than realm intrinsics, JavaScript Maps and WeakMaps, ArrayBuffers and typed-array views, binding cells, natives that declare it | never |
//! | [`Shareability::ConcurrentReference`] | atoms (`crate::atom`), Promises and resolving-pair guards (`crate::vm::promise`); reserved for any future heap-object handle to a dependency cell (cells themselves are runtime-owned, not heap objects; `crate::dep` checks each result it installs) | always; not descended into |
//!
//! Primitives are always shareable, strings, symbols and BigInts included: a string
//! (`crate::value::JsString`, clj-quickjs-65) is immutable `Arc` storage
//! outside the heap with no heap edges, not a heap object, so it needs no
//! category and the walk never descends into it. Symbols use the same
//! immutable leaf storage boundary (`crate::value::symbol`) with no heap
//! edges, and preserve identity across publication. BigInts have an inline
//! small form or an immutable Arc limb body, also without heap edges, so
//! no heap-kind category is needed and collection never descends into them.
//! Ordinary objects
//! (`crate::object`, clj-quickjs-114) are the mutable JavaScript objects of
//! `design.md` §4: task-owned mutation, never shareable, whatever their
//! properties hold (an object literal is rejected like a builder). The
//! same applies to ArrayBuffer objects, including detached buffers: their
//! mutable backing stores and ordinary properties are task-owned. Typed-array
//! views retain their task-owned buffer and remain task-owned after detachment. The
//! exception is the realm's intrinsic objects (`Object.prototype`, and the
//! property stores of built-in functions; `crate::realm`, clj-quickjs-117):
//! ownerless and never changed once the realm is installed, they are
//! persistent, so an object that inherits from one is rejected at the
//! object, never at its prototype. A builder
//! stays task-owned after `freeze` or release: the persistent result of a
//! freeze is a separate object, the builder handle itself never becomes
//! shareable. A concurrent reference is shareable without being immutable
//! because its own operations (set, compare-and-swap, single-assignment
//! completion) run this check on every value they install; the walk
//! therefore stops at it instead of reading contents that may change.
//! Promises (`crate::vm::promise`) additionally store opaque owner-bound
//! reaction and result envelopes. Every settlement checks publication;
//! an unshareable result remains confined to its settling task, and foreign
//! reactions reject rather than extracting it. Only checked results cross
//! task boundaries; tracing an opaque envelope grants no foreign read access.
//! Resolving-pair guards contain only a fixed Promise edge and a first-call
//! flag claimed under the heap metadata lock; they never store user values.
//!
//! A function object is persistent: its code and captured values never change
//! after allocation. A closure is therefore shareable exactly when its
//! captured values are: one that captured only immutable shareable values
//! may be published, and one that captured a binding cell (the task-owned
//! storage of a mutable binding, `crate::vm::function`) is rejected at that
//! cell, with a path ending in `.capture(i)`. Publishing a closure never
//! publishes its code: code is immutable `Arc` storage outside the heap,
//! shared by every task whether or not any closure over it is published.
//!
//! A concise method (clj-quickjs-126) also has an immutable home-object edge,
//! fixed before allocation. Its category stays `Persistent`, and the walk
//! follows that edge with `.home_object` ([`PathStep::HomeObject`]). Literal
//! objects are task-owned, so methods cannot publish, even with no captures
//! or own-property record and even if detached from the literal.
//!
//! ## Functions with own properties (clj-quickjs-125)
//!
//! A function's JavaScript own properties (`f.x = 1`, its lazy `prototype`)
//! live in a separate task-owned ordinary object, its property record,
//! created on first write or first `prototype` read
//! (`crate::vm::function`, "Own properties"; decision record:
//! `docs/architecture.md`, "Function own properties"). The function stays
//! `Persistent`; its record is one more edge, which the walk follows like a
//! capture, so a function with a record is rejected at it, with a path step
//! `.properties` ([`PathStep::FunctionProperties`]; the record is a
//! task-owned `object`, and the description continues into its property
//! values: `value.properties.property(prototype)` once `prototype` was
//! read). An untouched function publishes as before.
//!
//! That edge is the one exception to "a persistent object's edges never
//! change": it is written at most once, and never on a function whose
//! known-shareable bit is set. Both the attach and the setting of the bit
//! happen under the object-table lock: the attach refuses a known function
//! (the write is then the `TypeError` of a shared, immutable function), and
//! [`check`] sets bits with [`Heap::mark_shareable_unless`], which marks
//! nothing and makes the check walk again if a function it expanded gained
//! a record meanwhile. So the sticky-bit invariant below holds for
//! functions too, and a function is never known shareable at allocation
//! (its creator may still give it properties): it becomes known when a
//! check proves it, and from then on it is immutable.
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
//!   stays effective. (Closures are `Persistent` objects whose mutable
//!   bindings live in separate `TaskOwned` binding cells, and whose own
//!   properties live in a separate `TaskOwned` record attached at most once
//!   before the function is known shareable; see above.)
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
//! When that first task-owned object is an ordinary object, the description
//! walks on into its property values: while the object reached has an own
//! property (in creation order) whose value is a task-owned object not yet
//! on the path, the path continues with a `.property(name)` step to it. So
//! publishing `[{inner: {}}]` fails at `value[0].property(inner)`, naming
//! the innermost mutable object along the first such chain. An array
//! (clj-quickjs-118) is walked the same way, its elements first in index
//! order with an `[index]` step ([`PathStep::Element`]), then its named
//! properties: publishing `[[1, {}]]` (an array inside a vector) fails at
//! `value[0][1]`. Every object the path passes after its last persistent
//! step is an ordinary object or an array, which is itself task-owned:
//! the value becomes publishable only once nothing on the path is
//! reachable from it. The extension reads each
//! object under its own short lock, like the walk, and is bounded by the
//! number of objects (no object is entered twice).
//!
//! No mutator token is needed: the published value is rooted by the caller
//! and the walk descends only through persistent objects, whose edges never
//! change, so everything it visits stays reachable for the whole walk. The
//! path extension through ordinary objects is the one exception: their
//! properties may change while it reads them (their owner may be running),
//! so it may see an object being changed or one that a collection has just
//! reclaimed. Every read is generation-checked, so it then describes a
//! slightly different (still task-owned) object, or stops early; the
//! verdict itself never depends on it.
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
//! object's edges never change (a function's one write-once edge is attached
//! only while its bit is clear, above) and no object's category ever changes. The
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
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;

use crate::collections::{PMap, PVector, object_name};
use crate::heap::{HandleError, Heap, HeapObject, ObjectId};
use crate::ids::TaskId;
use crate::object::{OrdinaryObject, PropertyKey};
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
    ConcurrentReference,
}

/// The category of `object`. Exhaustive on purpose: every new kind must
/// declare one (module docs, "What a later object kind must declare").
pub(crate) fn classify(object: &HeapObject) -> Shareability {
    match object {
        HeapObject::Promise(_) | HeapObject::PromiseResolving(_) => {
            Shareability::ConcurrentReference
        }
        HeapObject::PromiseReaction(_) => Shareability::TaskOwned,
        HeapObject::Generator(_) | HeapObject::AsyncState(_) => Shareability::TaskOwned,
        // Realm intrinsics never change after the realm is built
        // (`crate::realm`); every other ordinary object, including an
        // engine Error, Intl.Locale, Intl.NumberFormat, Intl.PluralRules, Intl.Collator, Intl.DateTimeFormat and
        // RegExp instances have task-owned ordinary properties.
        HeapObject::Plain(o) if o.is_intrinsic() => Shareability::Persistent,
        HeapObject::Plain(_) => Shareability::TaskOwned,
        HeapObject::Native(payload) => payload.shareability(),
        HeapObject::Vector(_) | HeapObject::Map(_) | HeapObject::Occurrence(_) => {
            Shareability::Persistent
        }
        HeapObject::Builder(_) => Shareability::TaskOwned,
        // Code and captured values are fixed at creation (`crate::vm::function`).
        HeapObject::Function(_) | HeapObject::BoundFunction(_) => Shareability::Persistent,
        HeapObject::Cell(_) => Shareability::TaskOwned,
        // Every operation that installs a value checks it first (`crate::atom`).
        HeapObject::Atom(_) => Shareability::ConcurrentReference,
        // A realm intrinsic: immutable once built (`crate::vm::builtin`).
        HeapObject::Builtin(_) => Shareability::Persistent,
        HeapObject::Array(a) if a.object().is_intrinsic() => Shareability::Persistent,
        HeapObject::Array(_) | HeapObject::JsMap(_) | HeapObject::WeakMap(_) => {
            Shareability::TaskOwned
        }
        HeapObject::ArrayBuffer(_) => Shareability::TaskOwned,
        HeapObject::TypedArray(_) => Shareability::TaskOwned,
    }
}

/// The task that owns a task-owned object, when it has one.
fn owner(object: &HeapObject) -> Option<TaskId> {
    match object {
        HeapObject::Builder(b) => Some(b.owner()),
        HeapObject::Cell(c) => Some(c.owner),
        HeapObject::Generator(g) => Some(g.owner),
        HeapObject::AsyncState(a) => Some(a.owner),
        HeapObject::Plain(o) => o.owner(),
        HeapObject::Array(a) => a.object().owner(),
        HeapObject::JsMap(m) => m.object().owner(),
        HeapObject::WeakMap(m) => m.object().owner(),

        HeapObject::ArrayBuffer(b) => b.object.owner(),
        HeapObject::TypedArray(a) => a.object.owner(),
        HeapObject::Native(_)
        | HeapObject::Vector(_)
        | HeapObject::Map(_)
        | HeapObject::Occurrence(_)
        | HeapObject::Function(_)
        | HeapObject::BoundFunction(_)
        | HeapObject::Atom(_)
        | HeapObject::Builtin(_)
        | HeapObject::Promise(_)
        | HeapObject::PromiseReaction(_)
        | HeapObject::PromiseResolving(_) => None,
    }
}

/// A map key as named in a [`PathStep`].
#[derive(Clone, Debug, PartialEq)]
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
            KeyName::Primitive(Primitive::String(s)) => write!(f, "{s:?}"),
            KeyName::Primitive(Primitive::Symbol(s)) => write!(f, "{s}"),
            KeyName::Primitive(Primitive::BigInt(n)) => write!(f, "{n}n"),
            KeyName::Object(kind) => write!(f, "<{kind}>"),
        }
    }
}

/// One step from a value to something it references.
#[derive(Clone, Debug, PartialEq)]
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
    /// The n-th captured value of a closure. Displayed `.capture(n)`.
    Capture(usize),
    /// The value of an ordinary object's own property, by its name.
    /// Displayed `.property(name)`. Only in the description of a rejected
    /// ordinary object (see [`PublishError::NotShareable`]).
    Property(String),
    /// The element of an array object at an index. Displayed `[i]`, like
    /// a vector element. Only in the description of a rejected array (see
    /// [`PublishError::NotShareable`]).
    Element(u32),
    /// A function object's own-property record (clj-quickjs-125,
    /// `crate::vm::function`): an ordinary object, so always where a path
    /// is rejected (or continues with `.property(name)` steps). Displayed
    /// `.properties`.
    FunctionProperties,
    /// A concise method's immutable home object. Displayed `.home_object`.
    HomeObject,
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
                PathStep::Capture(n) => write!(f, ".capture({n})")?,
                PathStep::Property(name) => write!(f, ".property({name})")?,
                PathStep::Element(i) => write!(f, "[{i}]")?,
                PathStep::FunctionProperties => f.write_str(".properties")?,
                PathStep::HomeObject => f.write_str(".home_object")?,
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
    /// mutation: a transient builder, an ordinary object, a binding cell or
    /// a mutable native). For an ordinary object the path continues into
    /// its property values (module docs, "The check").
    NotShareable {
        /// Where the object sits inside the published value.
        path: ValuePath,
        /// The object's kind (`vector builder`, `object`, ...).
        found: &'static str,
        /// The owning task, for builders, binding cells and ordinary objects
        /// created by a task.
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
#[derive(Clone, Debug)]
enum Step {
    Index(usize),
    MapValue(Value),
    MapKey(Value),
    OccurrenceValue,
    NativeEdge(usize),
    Capture(usize),
    FunctionProperties,
    HomeObject,
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
    /// A closure's captured values and its own-property record, if it has
    /// one. Its code's constants are primitives by construction
    /// (`ExecutableSpec::constants`), so they add no edges.
    Function {
        captures: Box<[Value]>,
        properties: Option<ObjectId>,
        home_object: Option<ObjectId>,
    },
}

impl Children {
    fn for_each(&self, mut f: impl FnMut(Step, &Value)) {
        match self {
            Children::Vector(vec) => {
                let mut index = 0;
                vec.for_each(|v| {
                    f(Step::Index(index), v);
                    index += 1;
                });
            }
            Children::Map(map) => map.for_each(|k, v| {
                f(Step::MapKey(k.clone()), k);
                f(Step::MapValue(k.clone()), v);
            }),
            Children::Occurrence(v) => f(Step::OccurrenceValue, v),
            Children::Native(edges) => {
                for (n, &id) in edges.iter().enumerate() {
                    f(Step::NativeEdge(n), &Value::Object(id));
                }
            }
            Children::Function {
                captures,
                properties,
                home_object,
            } => {
                for (n, v) in captures.iter().enumerate() {
                    f(Step::Capture(n), v);
                }
                if let Some(record) = properties {
                    f(Step::FunctionProperties, &Value::Object(*record));
                }
                if let Some(home) = home_object {
                    f(Step::HomeObject, &Value::Object(*home));
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
            HeapObject::Occurrence(occ) => Children::Occurrence(occ.value.clone()),
            HeapObject::Function(func) => Children::Function {
                captures: func.captures.clone(),
                properties: func.properties,
                home_object: func.home_object,
            },
            HeapObject::BoundFunction(func) => Children::Function {
                captures: std::iter::once(func.target.clone())
                    .chain(std::iter::once(func.this.clone()))
                    .chain(func.args.iter().cloned())
                    .chain(std::iter::once(Value::Object(func.metadata)))
                    .collect(),
                properties: func.properties,
                home_object: None,
            },
            other => {
                let mut edges = Vec::new();
                crate::heap::gc::Trace::trace(other, &mut |edge| {
                    if let crate::heap::gc::TraceEdge::Object(id) = edge {
                        edges.push(id);
                    }
                });
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
pub(crate) fn check(heap: &Heap, value: &Value) -> Result<(), PublishError> {
    check_counted(heap, value, &mut 0)
}

/// [`check`], counting the objects whose references it expands.
fn check_counted(heap: &Heap, value: &Value, expanded: &mut usize) -> Result<(), PublishError> {
    let Value::Object(root) = *value else {
        return Ok(());
    };
    // A function that gained its own-property record after the walk read
    // it voids the walk: nothing is marked and the value is walked again
    // (module docs, "Functions with own properties"). Each function gains a
    // record at most once, so this ends.
    loop {
        let proven = walk(heap, root, expanded)?;
        let gained_properties = |o: &HeapObject| match o {
            HeapObject::Function(f) => f.properties.is_some(),
            HeapObject::BoundFunction(f) => f.properties.is_some(),
            _ => false,
        };
        if heap.mark_shareable_unless(&proven, gained_properties) {
            return Ok(());
        }
    }
}

/// One breadth-first walk of [`check`] from `root`: the persistent objects
/// it expanded, or the rejection.
fn walk(heap: &Heap, root: ObjectId, expanded: &mut usize) -> Result<Vec<ObjectId>, PublishError> {
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
                let mut path = path_to(heap, &parents, id);
                let (found, owner) = into_properties(heap, id, &mut path, (found, owner));
                return Err(PublishError::NotShareable { path, found, owner });
            }
            Node::Expand(children) => {
                *expanded += 1;
                proven.push(id);
                children.for_each(|step, child| {
                    if let Value::Object(child) = *child
                        && let Entry::Vacant(slot) = parents.entry(child)
                    {
                        slot.insert(Some((id, step)));
                        queue.push_back(child);
                    }
                });
            }
        }
    }
    Ok(proven)
}

/// How [`into_properties`] reaches a child: an array element or a named
/// property.
enum Via {
    Element(u32),
    Property(PropertyKey),
}

/// The object-valued named own properties of `object`, in creation order.
fn object_properties(object: &OrdinaryObject) -> Vec<(Via, ObjectId)> {
    object
        .properties()
        .filter_map(|(key, p)| match p.value {
            Value::Object(child) => Some((Via::Property(key), child)),
            _ => None,
        })
        .collect()
}

/// Extend the path of a rejected object `id` through ordinary objects'
/// property values and arrays' elements (module docs, "The check");
/// returns the kind and owner of the object the extended path ends at
/// (`rejected` if it is not extended).
fn into_properties(
    heap: &Heap,
    mut id: ObjectId,
    path: &mut ValuePath,
    mut rejected: (&'static str, Option<TaskId>),
) -> (&'static str, Option<TaskId>) {
    let mut entered = HashSet::from([id]);
    loop {
        // The object-valued own properties of `id`, if it is ordinary or an
        // array (an array's elements first, in index order).
        let candidates = heap
            .read(id, |object| match object {
                HeapObject::Plain(o) => object_properties(o),
                HeapObject::Array(a) => {
                    let mut found: Vec<(Via, ObjectId)> = a
                        .elements()
                        .filter_map(|(index, v)| match *v {
                            Value::Object(child) => Some((Via::Element(index), child)),
                            _ => None,
                        })
                        .collect();
                    found.extend(object_properties(a.object()));
                    found
                }
                _ => Vec::new(),
            })
            .unwrap_or_default();
        let next = candidates.into_iter().find_map(|(key, child)| {
            if entered.contains(&child) {
                return None;
            }
            let task_owned = |o: &HeapObject| {
                (classify(o) == Shareability::TaskOwned).then(|| (object_name(o), owner(o)))
            };
            heap.read(child, task_owned)
                .flatten()
                .map(|r| (key, child, r))
        });
        let Some((key, child, found)) = next else {
            return rejected;
        };
        path.0.push(match key {
            Via::Element(index) => PathStep::Element(index),
            Via::Property(key) => PathStep::Property(
                heap.keys()
                    .name(key)
                    .map_or_else(|| "<unnamed>".to_string(), |n| n.to_string_lossy()),
            ),
        });
        entered.insert(child);
        (id, rejected) = (child, found);
    }
}

/// The path from the walk's root to `id`, following first-discovery links.
fn path_to(
    heap: &Heap,
    parents: &HashMap<ObjectId, Option<(ObjectId, Step)>>,
    mut id: ObjectId,
) -> ValuePath {
    let mut steps = Vec::new();
    while let Some(Some((parent, step))) = parents.get(&id) {
        steps.push(name_step(heap, step.clone()));
        id = *parent;
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
        Step::Capture(n) => PathStep::Capture(n),
        Step::FunctionProperties => PathStep::FunctionProperties,
        Step::HomeObject => PathStep::HomeObject,
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

    fn id(v: &Value) -> ObjectId {
        match *v {
            Value::Object(id) => id,
            ref other => panic!("not an object: {other:?}"),
        }
    }

    fn plain(heap: &Heap) -> Value {
        Value::Object(heap.alloc(HeapObject::host_object()).unwrap())
    }

    fn known(heap: &Heap, v: &Value) -> bool {
        heap.known_shareable(id(v)).expect("live object")
    }

    /// Check `v` and return how many objects the walk expanded.
    fn expansions(heap: &Heap, v: &Value) -> Result<usize, PublishError> {
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
        fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
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
    fn set_edges(heap: &Heap, v: &Value, edges: Vec<Value>) {
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
        for v in items {
            builder::conj(heap, OWNER, b.clone(), v.clone()).unwrap();
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
        builder::freeze(&heap, OWNER, frozen.clone()).unwrap();
        let class = |v| heap.read(id(&v), classify).unwrap();
        assert_eq!(class(plain(&heap)), Shareability::TaskOwned);
        for persistent in [vec, map, occ] {
            assert_eq!(class(persistent), Shareability::Persistent);
        }
        assert_eq!(class(open.clone()), Shareability::TaskOwned);
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
        assert_eq!(heap.read(id(&open), owner).unwrap(), Some(OWNER));
    }

    #[test]
    fn values_built_from_shareable_parts_are_known_at_allocation() {
        let heap = Arc::new(Heap::default());
        let _scope = Scope::new(&heap);
        let obj = plain(&heap);
        let vec = coll::new_vector(&heap, &[n(1.0), n(2.0)]).unwrap();
        let map = coll::new_map(&heap, &[(vec.clone(), vec.clone()), (n(1.0), n(2.0))]).unwrap();
        let occ = coll::new_occurrence(&heap, map.clone(), Provenance::default()).unwrap();
        let holder = coll::new_vector(&heap, std::slice::from_ref(&occ)).unwrap();
        let grown = coll::conj(&heap, holder.clone(), n(3.0)).unwrap();
        for v in [&vec, &map, &occ, &holder, &grown] {
            assert!(known(&heap, v));
            assert_eq!(expansions(&heap, v), Ok(0), "no walk for a known value");
        }
        // Derived versions: known exactly when every new part is known.
        assert!(known(
            &heap,
            &coll::assoc(&heap, vec.clone(), n(0.0), n(9.0)).unwrap()
        ));
        assert!(known(
            &heap,
            &coll::assoc(&heap, map.clone(), n(5.0), vec.clone()).unwrap()
        ));
        assert!(known(
            &heap,
            &coll::dissoc(&heap, map.clone(), n(1.0)).unwrap()
        ));
        assert!(known(&heap, &coll::pop(&heap, vec.clone()).unwrap()));
        let tainted = coll::assoc(&heap, vec.clone(), n(0.0), obj.clone()).unwrap();
        assert!(!known(&heap, &tainted));
        assert!(!known(
            &heap,
            &coll::new_vector(&heap, std::slice::from_ref(&tainted)).unwrap()
        ));
        assert!(!known(
            &heap,
            &coll::assoc(&heap, map.clone(), obj.clone(), n(1.0)).unwrap()
        ));
        // Removing the task-owned part is not tracked at allocation (the
        // parent was not known), so the check walks it once and caches it.
        let cleaned = coll::assoc(&heap, tainted.clone(), n(0.0), n(1.0)).unwrap();
        assert!(!known(&heap, &cleaned));
        assert_eq!(expansions(&heap, &cleaned), Ok(1));
        assert!(known(&heap, &cleaned));
        // Freezing: known when the draft holds only known values.
        let good = vector_builder(&heap, &[vec.clone(), n(1.0)]);
        assert!(known(
            &heap,
            &builder::freeze(&heap, OWNER, good.clone()).unwrap()
        ));
        let bad = vector_builder(&heap, &[vec.clone(), obj.clone()]);
        assert!(!known(
            &heap,
            &builder::freeze(&heap, OWNER, bad.clone()).unwrap()
        ));
        // A concurrent reference is a known part even though it is not
        // immutable.
        let cell = native(&heap, Shareability::ConcurrentReference, vec![obj.clone()]);
        assert!(known(
            &heap,
            &coll::new_vector(&heap, std::slice::from_ref(&cell)).unwrap()
        ));
    }

    #[test]
    fn repeated_publication_does_not_rewalk_the_graph() {
        let heap = Arc::new(Heap::default());
        let _scope = Scope::new(&heap);
        // An empty vector not known shareable: popped from one holding an
        // object. Everything built on it is unknown until checked.
        let obj = plain(&heap);
        let base = coll::pop(
            &heap,
            coll::new_vector(&heap, std::slice::from_ref(&obj)).unwrap(),
        )
        .unwrap();
        assert!(!known(&heap, &base));
        let inner: Vec<Value> = (0..1000)
            .map(|i| coll::new_vector(&heap, &[base.clone(), n(f64::from(i))]).unwrap())
            .collect();
        let root = coll::new_vector(&heap, &inner).unwrap();
        assert_eq!(expansions(&heap, &root), Ok(1002), "root, 1000 inner, base");
        assert_eq!(expansions(&heap, &root), Ok(0), "second publication: O(1)");
        assert!(inner.iter().all(|v| known(&heap, v)));
        // New versions of a published value are known at allocation.
        let next = coll::conj(&heap, root.clone(), n(1.0)).unwrap();
        assert_eq!(expansions(&heap, &next), Ok(0));
        let wrapped = coll::new_map(&heap, &[(n(1.0), root.clone())]).unwrap();
        assert_eq!(expansions(&heap, &wrapped), Ok(0));
    }

    #[test]
    fn cycles_terminate_and_expand_each_object_once() {
        let heap = Arc::new(Heap::default());
        let _scope = Scope::new(&heap);
        // a <-> b, plus a self-loop on b: persistent natives.
        let a = native(&heap, Shareability::Persistent, vec![]);
        let b = native(&heap, Shareability::Persistent, vec![a.clone()]);
        set_edges(&heap, &a, vec![b.clone(), n(1.0)]);
        set_edges(&heap, &b, vec![a.clone(), b.clone()]);
        let root = coll::new_vector(&heap, &[a.clone(), b.clone(), a.clone()]).unwrap();
        assert!(!known(&heap, &root));
        assert_eq!(expansions(&heap, &root), Ok(3));
        assert!(known(&heap, &a) && known(&heap, &b) && known(&heap, &root));

        // A cycle with a builder behind it: rejected with the shortest path.
        let c = native(&heap, Shareability::Persistent, vec![]);
        let d = native(&heap, Shareability::Persistent, vec![c.clone()]);
        let builder = vector_builder(&heap, std::slice::from_ref(&c));
        set_edges(&heap, &c, vec![d.clone()]);
        set_edges(&heap, &d, vec![c.clone(), builder.clone()]);
        let err = check(&heap, &c).unwrap_err();
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
            !known(&heap, &c) && !known(&heap, &d),
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
        let cell = native(&heap, Shareability::ConcurrentReference, vec![obj.clone()]);
        assert_eq!(check(&heap, &cell), Ok(()));
        let mutable = native(&heap, Shareability::TaskOwned, vec![]);
        let occ = coll::new_occurrence(&heap, mutable, Provenance::default()).unwrap();
        let root = coll::new_map(&heap, &[(n(2.0), occ.clone())]).unwrap();
        assert_eq!(
            check(&heap, &root),
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
        let map = coll::new_map(&heap, &[(n(1.0), n(1.0)), (b.clone(), n(2.0))]).unwrap();
        let err = check(&heap, &map).unwrap_err();
        let PublishError::NotShareable { path, .. } = &err else {
            panic!("{err:?}")
        };
        assert_eq!(
            path.steps(),
            [PathStep::MapKey(KeyName::Object("vector builder"))]
        );
        assert_eq!(path.to_string(), "value{<vector builder>}.key");
        // A string key is named by its contents; strings themselves are
        // primitives, shareable without a walk.
        let key = Value::String(crate::value::JsString::from("k"));
        let map = coll::new_map(&heap, &[(key.clone(), b.clone())]).unwrap();
        let err = check(&heap, &map).unwrap_err();
        let PublishError::NotShareable { path, .. } = &err else {
            panic!("{err:?}")
        };
        assert_eq!(path.to_string(), "value{\"k\"}");
        assert_eq!(check(&heap, &key), Ok(()));
        assert_eq!(check(&heap, &n(1.0)), Ok(()), "primitives are shareable");
    }

    /// A function whose property record is attached between the walk that
    /// read it and the marking of the walk's result (clj-quickjs-125): the
    /// marking refuses, nothing is cached, and the check walks again and
    /// rejects the function at its record. Conversely a known function
    /// cannot gain a record (`crate::vm::function`).
    #[test]
    fn a_function_gaining_properties_during_a_check_is_not_cached() {
        use crate::vm::function::FunctionObject;
        for bound in [false, true] {
            let heap = Arc::new(Heap::default());
            let _scope = Scope::new(&heap);
            let code = crate::frontend::compile_script("f", "1").unwrap().0;
            let function = |heap: &Heap| {
                let f = FunctionObject::new(Arc::clone(&code), Box::new([]), OWNER);
                let f = Value::Object(heap.alloc(HeapObject::Function(f)).unwrap());
                if bound {
                    crate::vm::function::bind(heap, OWNER, &f, &[])
                        .ok()
                        .expect("bound function")
                } else {
                    f
                }
            };
            let f = function(&heap);
            assert!(!known(&heap, &f), "never known at allocation");
            let holder = coll::new_vector(&heap, std::slice::from_ref(&f)).unwrap();
            assert!(!known(&heap, &holder));

            // The walk reads `f` without a record ...
            let proven = walk(&heap, id(&holder), &mut 0).unwrap();
            assert_eq!(proven.len(), if bound { 4 } else { 2 });
            // ... then its owner attaches one ...
            let record = plain(&heap);
            let attached = heap.with_object_mut_known(id(&f), |o, known| match o {
                HeapObject::Function(func) if !known => {
                    func.properties = Some(id(&record));
                    true
                }
                HeapObject::BoundFunction(func) if !known => {
                    func.properties = Some(id(&record));
                    true
                }
                _ => false,
            });
            assert_eq!(attached, Some(true));
            // ... so the walk's result is not marked, and the check rejects.
            let gained = |o: &HeapObject| match o {
                HeapObject::Function(f) => f.properties.is_some(),
                HeapObject::BoundFunction(f) => f.properties.is_some(),
                _ => false,
            };
            assert!(!heap.mark_shareable_unless(&proven, gained));
            assert!(!known(&heap, &f) && !known(&heap, &holder));
            let (path, found) = match check(&heap, &holder) {
                Err(PublishError::NotShareable { path, found, .. }) => (path, found),
                other => panic!("{other:?}"),
            };
            assert_eq!(
                path.steps(),
                [PathStep::Index(0), PathStep::FunctionProperties]
            );
            assert_eq!(path.to_string(), "value[0].properties");
            assert_eq!(found, "object");

            // A function proved shareable is known, and a record is refused.
            let g = function(&heap);
            assert_eq!(check(&heap, &g), Ok(()));
            assert!(known(&heap, &g));
            let refused = heap.with_object_mut_known(id(&g), |_, known| known);
            assert_eq!(refused, Some(true));
        }
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
                    (0..DEPTH).fold(bottom, |v, _| {
                        coll::new_vector(&heap, std::slice::from_ref(&v)).unwrap()
                    })
                };
                let obj = plain(&heap);
                let base = coll::pop(
                    &heap,
                    coll::new_vector(&heap, std::slice::from_ref(&obj)).unwrap(),
                )
                .unwrap();
                let good = nest(base.clone());
                assert_eq!(expansions(&heap, &good), Ok(DEPTH + 1));
                assert_eq!(expansions(&heap, &good), Ok(0));

                let bad = nest(obj);
                let Err(PublishError::NotShareable { path, found, .. }) = check(&heap, &bad) else {
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
