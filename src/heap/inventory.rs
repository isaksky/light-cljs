//! The collector's root inventory and its cross-check (clj-quickjs-31,
//! `docs/design.md` §11 "The collector must see").
//!
//! [`Holder`] lists every place a heap value can be held such that the
//! collector must keep it: each is a root of some [`RootCategory`], or an
//! edge of some [`ObjectKind`] reached from one. Two exhaustive matches map
//! every category and every object kind to the holders that exercise it, so
//! adding a variant to either registry fails to compile until it is mapped
//! here; [`coverage`] names each holder's retention and reclamation tests,
//! and [`every_category_and_kind_has_retention_and_reclamation_tests`]
//! checks that those tests exist (in `tests/gc_roots.rs`, or below for
//! holders the public API cannot reach) and that the inventory in
//! `docs/architecture.md` ("Root inventory") names them.
//!
//! The tests below cover the crate-internal holders: object constants of
//! executable code, native payload edges, and a dependency cell's outcome
//! once its cell table is gone (public cells live as long as the runtime).

use std::sync::Arc;

use super::gc::{RootCategory, Trace};
use super::{Heap, HeapObject, NativePayload, ObjectId, ObjectKind, Rooted};
use crate::value::Value;
use crate::value::share::Shareability;

/// Every holder of heap values the collector must see.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Holder {
    /// A `Rooted` handle held by the host.
    PersistentHandle,
    /// A local of an open host scope.
    ScopeLocal,
    /// A task's published `Complete` result.
    TaskResult,
    /// The frames of a run stopped at a VM safepoint.
    RunningFrame,
    /// The saved frames of a yielded (or finished) task.
    SuspendedFrame,
    /// The saved frames of a task suspended at a pending `__rt.need`.
    PendingNeedFrame,
    /// A task in an executor's ready queue.
    ReadyQueue,
    /// A task an executor parked on a pending dependency.
    ExecutorParked,
    /// A pending `return`/`throw` completion of a running `finally` block.
    HandlerCompletion,
    /// A resumable native's frame (`__rt.reduce` accumulator).
    NativeFrame,
    /// An open transient builder's draft.
    TransientBuilder,
    /// A completed dependency cell's `Ready` value or `Failed` error.
    DependencyOutcome,
    /// An atom's current value.
    AtomCurrent,
    /// An atom's replaced value, still held by a reader (load-to-root).
    AtomRetainedOld,
    /// A value captured by a closure.
    ClosureEnvironment,
    /// A mutable captured binding's cell.
    BindingCell,
    /// Edges of persistent vectors, maps and occurrences.
    PersistentCollectionEdges,
    /// An object constant of executable code a live task runs.
    ExecutableConstant,
    /// Edges reported by a native payload's `Trace`.
    NativePayloadEdges,
    /// Property values (and the prototype) of an ordinary object.
    ObjectProperty,
    /// The realm's intrinsic objects (`crate::realm`) and the property
    /// values of built-in functions.
    Intrinsic,
    /// Elements of an array object.
    ArrayElement,
    /// A function object's own-property record and its values (the lazy
    /// `prototype` object among them).
    FunctionProperties,
    /// A bound callable's target, receiver, arguments and metadata.
    BoundFunction,
    /// A concise method's immutable home-object reference.
    MethodHomeObject,

    /// The task's mutable global object, independent of operand/local slots.
    TaskGlobalObject,
    /// Per-realm template-site arrays, independent of tasks and closures.
    TemplateCache,
    /// The original cause in an engine Error's internal diagnostic slot.
    EngineErrorCause,
    /// Live JavaScript Map keys and values, plus its ordinary properties.
    JsMapEntries,

    /// Ordinary properties and prototype links of ArrayBuffer objects.
    ArrayBufferProperty,
    TypedArrayEdges,
}

/// Every holder. A new [`Holder`] must also be added to [`coverage`]
/// (exhaustive) and here.
const HOLDERS: [Holder; 31] = [
    Holder::PersistentHandle,
    Holder::ScopeLocal,
    Holder::TaskResult,
    Holder::RunningFrame,
    Holder::SuspendedFrame,
    Holder::PendingNeedFrame,
    Holder::ReadyQueue,
    Holder::ExecutorParked,
    Holder::HandlerCompletion,
    Holder::NativeFrame,
    Holder::TransientBuilder,
    Holder::DependencyOutcome,
    Holder::AtomCurrent,
    Holder::AtomRetainedOld,
    Holder::ClosureEnvironment,
    Holder::BindingCell,
    Holder::PersistentCollectionEdges,
    Holder::ExecutableConstant,
    Holder::NativePayloadEdges,
    Holder::ObjectProperty,
    Holder::Intrinsic,
    Holder::ArrayElement,
    Holder::FunctionProperties,
    Holder::BoundFunction,
    Holder::MethodHomeObject,
    Holder::TaskGlobalObject,
    Holder::TemplateCache,
    Holder::EngineErrorCause,
    Holder::JsMapEntries,
    Holder::ArrayBufferProperty,
    Holder::TypedArrayEdges,
];

/// Every root category; a new one fails to compile in
/// [`holders_of_category`] until it is mapped, and must be listed here.
const CATEGORIES: [RootCategory; 6] = [
    RootCategory::PersistentHandle,
    RootCategory::ScopeLocal,
    RootCategory::TaskFrame,
    RootCategory::ExecutableConstant,
    RootCategory::DependencyOutcome,
    RootCategory::Intrinsic,
];

/// Every object kind; a new one fails to compile in [`holders_of_kind`]
/// until it is mapped, and must be listed here.
const KINDS: [ObjectKind; 15] = [
    ObjectKind::PlainObject,
    ObjectKind::Native,
    ObjectKind::Vector,
    ObjectKind::Map,
    ObjectKind::Occurrence,
    ObjectKind::Builder,
    ObjectKind::Function,
    ObjectKind::Cell,
    ObjectKind::Atom,
    ObjectKind::Builtin,
    ObjectKind::Array,
    ObjectKind::BoundFunction,
    ObjectKind::JsMap,
    ObjectKind::ArrayBuffer,
    ObjectKind::TypedArray,
];

/// The holders whose values the collector reaches as roots of `category`.
fn holders_of_category(category: RootCategory) -> &'static [Holder] {
    use Holder::*;
    match category {
        RootCategory::PersistentHandle => &[
            PersistentHandle,
            TaskResult,
            AtomRetainedOld,
            AtomCurrent,
            ClosureEnvironment,
            BindingCell,
            PersistentCollectionEdges,
            NativePayloadEdges,
            FunctionProperties,
            BoundFunction,
            MethodHomeObject,
            EngineErrorCause,
            JsMapEntries,
            TypedArrayEdges,
        ],
        RootCategory::ScopeLocal => &[ScopeLocal],
        RootCategory::TaskFrame => &[
            RunningFrame,
            SuspendedFrame,
            PendingNeedFrame,
            ReadyQueue,
            ExecutorParked,
            HandlerCompletion,
            NativeFrame,
            TransientBuilder,
            ObjectProperty,
            ArrayElement,
            TaskGlobalObject,
            ArrayBufferProperty,
        ],
        RootCategory::ExecutableConstant => &[ExecutableConstant],
        RootCategory::DependencyOutcome => &[DependencyOutcome],
        RootCategory::Intrinsic => &[Intrinsic, TemplateCache],
    }
}

/// The holders whose tests retain a value through an edge of `kind` (for
/// [`ObjectKind::PlainObject`]: retain one itself, and through its properties).
fn holders_of_kind(kind: ObjectKind) -> &'static [Holder] {
    use Holder::*;
    match kind {
        ObjectKind::PlainObject => &[
            PersistentHandle,
            ObjectProperty,
            TaskGlobalObject,
            EngineErrorCause,
        ],
        ObjectKind::Native => &[NativePayloadEdges],
        ObjectKind::Vector | ObjectKind::Map | ObjectKind::Occurrence => {
            &[PersistentCollectionEdges, DependencyOutcome]
        }
        ObjectKind::Builder => &[TransientBuilder],
        ObjectKind::Function => &[
            ClosureEnvironment,
            BindingCell,
            FunctionProperties,
            MethodHomeObject,
        ],
        ObjectKind::Cell => &[BindingCell],
        ObjectKind::Atom => &[AtomCurrent],
        ObjectKind::Builtin => &[Intrinsic],
        ObjectKind::Array => &[ArrayElement],
        ObjectKind::BoundFunction => &[BoundFunction],
        ObjectKind::JsMap => &[JsMapEntries],

        ObjectKind::ArrayBuffer => &[ArrayBufferProperty],
        ObjectKind::TypedArray => &[TypedArrayEdges],
    }
}

/// Where a test lives.
#[derive(Clone, Copy, Debug)]
enum File {
    /// `tests/gc_roots.rs`: through the public API.
    GcRoots,
    /// This module: crate-internal holders.
    Inventory,
    /// JavaScript Map result-root tracing.
    Gc,
}

/// A holder's retention and reclamation tests.
struct Coverage {
    retains: (File, &'static str),
    reclaims: (File, &'static str),
}

fn coverage(holder: Holder) -> Coverage {
    use File::*;
    let public = |retains, reclaims| Coverage {
        retains: (GcRoots, retains),
        reclaims: (GcRoots, reclaims),
    };
    match holder {
        Holder::TypedArrayEdges => Coverage {
            retains: (Gc, "typed_array_retains_buffer_and_properties"),
            reclaims: (Gc, "typed_array_retains_buffer_and_properties"),
        },
        Holder::ArrayBufferProperty => public(
            "array_buffer_properties_retained_and_reclaimed",
            "array_buffer_properties_retained_and_reclaimed",
        ),
        Holder::TemplateCache => Coverage {
            retains: (Inventory, "template_cache_retains_and_reclaims"),
            reclaims: (Inventory, "template_cache_retains_and_reclaims"),
        },
        Holder::JsMapEntries => Coverage {
            retains: (Gc, "map_entries_are_traced"),
            reclaims: (Gc, "map_entries_are_traced"),
        },
        Holder::TaskGlobalObject => public(
            "task_global_object_retains_and_reclaims",
            "finished_task_releases_global_object",
        ),
        Holder::BoundFunction => public("bound_function_retains", "bound_function_reclaims"),
        Holder::PersistentHandle => {
            public("persistent_handle_retains", "persistent_handle_reclaims")
        }
        Holder::ScopeLocal => public("scope_local_retains", "scope_local_reclaims"),
        Holder::TaskResult => public("task_result_retains", "task_result_reclaims"),
        Holder::RunningFrame => public("running_frame_retains", "running_frame_reclaims"),
        Holder::SuspendedFrame => public("suspended_frame_retains", "suspended_frame_reclaims"),
        Holder::PendingNeedFrame => {
            public("pending_need_frame_retains", "pending_need_frame_reclaims")
        }
        Holder::ReadyQueue => public("ready_queue_retains", "ready_queue_reclaims"),
        Holder::ExecutorParked => public(
            "executor_parked_task_retains",
            "executor_parked_task_reclaims",
        ),
        Holder::HandlerCompletion => {
            public("handler_completion_retains", "handler_completion_reclaims")
        }
        Holder::NativeFrame => public("native_frame_retains", "native_frame_reclaims"),
        Holder::TransientBuilder => {
            public("transient_builder_retains", "transient_builder_reclaims")
        }
        Holder::DependencyOutcome => Coverage {
            retains: (GcRoots, "dependency_outcome_retains"),
            reclaims: (Inventory, "dependency_outcome_reclaims_with_its_cell"),
        },
        Holder::AtomCurrent => public("atom_current_value_retains", "atom_current_value_reclaims"),
        Holder::AtomRetainedOld => public(
            "atom_retained_old_value_retains",
            "atom_retained_old_value_reclaims",
        ),
        Holder::ClosureEnvironment => public(
            "closure_environment_retains",
            "closure_environment_reclaims",
        ),
        Holder::BindingCell => public("binding_cell_retains", "binding_cell_reclaims"),
        Holder::PersistentCollectionEdges => public(
            "persistent_collection_edges_retain",
            "persistent_collection_edges_reclaim",
        ),
        Holder::ExecutableConstant => Coverage {
            retains: (Inventory, "executable_constant_retains"),
            reclaims: (Inventory, "executable_constant_reclaims"),
        },
        Holder::NativePayloadEdges => Coverage {
            retains: (Inventory, "native_payload_edges_retain"),
            reclaims: (Inventory, "native_payload_edges_reclaim"),
        },
        Holder::ObjectProperty => public("object_property_retains", "object_property_reclaims"),
        Holder::Intrinsic => Coverage {
            retains: (GcRoots, "realm_intrinsics_retain"),
            reclaims: (Inventory, "realm_intrinsics_reclaim_with_their_heap"),
        },
        Holder::ArrayElement => public("array_element_retains", "array_element_reclaims"),
        Holder::FunctionProperties => {
            public("function_properties_retain", "function_properties_reclaim")
        }
        Holder::MethodHomeObject => {
            public("method_home_object_retains", "method_home_object_reclaims")
        }
        Holder::EngineErrorCause => Coverage {
            retains: (Inventory, "engine_error_cause_retains_and_reclaims"),
            reclaims: (Inventory, "engine_error_cause_retains_and_reclaims"),
        },
    }
}

const GC_ROOTS_SOURCE: &str = include_str!("../../tests/gc_roots.rs");
const GC_SOURCE: &str = include_str!("../../tests/gc.rs");
const INVENTORY_SOURCE: &str = include_str!("inventory.rs");
const ARCHITECTURE_DOC: &str = include_str!("../../docs/architecture.md");

/// True when `source` defines a `#[test]` function named `name`.
fn defines_test(source: &str, name: &str) -> bool {
    source
        .split("#[test]")
        .skip(1)
        .any(|after| after.trim_start().starts_with(&format!("fn {name}()")))
}

#[test]
fn every_category_and_kind_has_retention_and_reclamation_tests() {
    for category in CATEGORIES {
        let holders = holders_of_category(category);
        assert!(!holders.is_empty(), "{category:?} has no holder");
        assert!(holders.iter().all(|h| HOLDERS.contains(h)));
    }
    for kind in KINDS {
        let holders = holders_of_kind(kind);
        assert!(!holders.is_empty(), "{kind:?} has no holder");
        assert!(holders.iter().all(|h| HOLDERS.contains(h)));
    }
    let doc = ARCHITECTURE_DOC
        .split("## Root inventory")
        .nth(1)
        .expect("docs/architecture.md has a \"Root inventory\" section");
    for holder in HOLDERS {
        assert!(
            CATEGORIES
                .iter()
                .any(|&c| holders_of_category(c).contains(&holder)),
            "{holder:?} is not a root of any category"
        );
        let Coverage { retains, reclaims } = coverage(holder);
        for (file, name) in [retains, reclaims] {
            let source = match file {
                File::GcRoots => GC_ROOTS_SOURCE,
                File::Inventory => INVENTORY_SOURCE,
                File::Gc => GC_SOURCE,
            };
            assert!(
                defines_test(source, name),
                "{holder:?}: no test `{name}` in {file:?}"
            );
            assert!(
                doc.contains(&format!("`{name}`")),
                "{holder:?}: the root inventory doc does not name `{name}`"
            );
        }
    }
}

/// `id` is reclaimed and rejected as stale, also after new objects reuse
/// the freed slots (which get a new generation).
fn assert_stale(heap: &Arc<Heap>, ids: &[ObjectId]) {
    let check = || {
        for &id in ids {
            assert!(!heap.is_live(id), "{id:?} survived its holder");
            assert_eq!(heap.kind(id), None);
            assert!(heap.read(id, |_| ()).is_none());
            assert_eq!(heap.root_if_live(Value::Object(id)), None);
        }
    };
    check();
    let fresh: Vec<Rooted> = (0..ids.len() + 4)
        .map(|_| {
            Rooted::new(
                heap,
                Value::Object(heap.alloc(HeapObject::host_object()).unwrap()),
            )
        })
        .collect();
    check();
    drop(fresh);
}

fn plain(heap: &Heap) -> ObjectId {
    heap.alloc(HeapObject::host_object()).unwrap()
}

/// `return constant[0]`, with an object constant. Loaded code only has
/// primitive constants in this slice, so this builds `Code` directly.
fn spawn_with_constant(rt: &crate::Runtime, constant: ObjectId) -> crate::Task {
    use crate::vm::executable::Code;
    use crate::vm::instr::Instr;
    let code = Code {
        regexps: std::collections::HashMap::new(),
        name: "object-constant".into(),
        params: 0,
        function_length: 0,
        function_name: crate::value::JsString::from(""),
        locals: 0,
        constants: Box::new([Value::Object(constant)]),
        functions: Box::new([]),
        captures: 0,
        strict: true,
        constructor: false,
        uses_arguments: false,
        offsets: Box::new([0, 3]),
        instrs: Box::new([Instr::Const(0), Instr::Return]),
        max_stack: 1,
        debug: None,
    };
    rt.spawn(&crate::Executable(Arc::new(code)), &[]).unwrap()
}

#[test]
fn executable_constant_retains() {
    let rt = crate::Runtime::new();
    let heap = rt.heap();
    let constant = plain(heap);
    let task = spawn_with_constant(&rt, constant);
    for _ in 0..2 {
        rt.request_collection().unwrap();
        assert!(heap.is_live(constant));
    }
    let snapshot = heap.root_snapshot().unwrap();
    assert_eq!(
        snapshot,
        vec![(RootCategory::ExecutableConstant, constant)],
        "the task's code is the only holder"
    );
    // The constant is still the same object when the code runs.
    let crate::RunOutcome::Complete(result) = task.run().unwrap() else {
        panic!("the task completes");
    };
    assert_eq!(result.value_in(heap), Ok(Value::Object(constant)));
}

#[test]
fn executable_constant_reclaims() {
    let rt = crate::Runtime::new();
    let heap = rt.heap();
    let constant = plain(heap);
    let task = spawn_with_constant(&rt, constant);
    rt.request_collection().unwrap();
    assert!(heap.is_live(constant));
    drop(task);
    assert_eq!(rt.request_collection().unwrap().reclaimed, 1);
    assert_stale(heap, &[constant]);
}

/// Native payload with a fixed list of edges.
#[derive(Debug)]
struct Edges(Vec<Value>);

impl Trace for Edges {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        self.0.trace(visit);
    }
}

impl NativePayload for Edges {
    fn shareability(&self) -> Shareability {
        Shareability::Persistent
    }
}

/// A rooted native object whose payload holds a plain object and a nested
/// native object holding another.
fn native_holding(heap: &Arc<Heap>) -> (Rooted, Vec<ObjectId>) {
    let deep = plain(heap);
    let inner = heap
        .alloc(HeapObject::Native(Box::new(Edges(vec![Value::Object(
            deep,
        )]))))
        .unwrap();
    let leaf = plain(heap);
    let edges = vec![
        Value::Object(leaf),
        Value::Object(inner),
        Value::Number(1.0),
    ];
    let outer = heap
        .alloc(HeapObject::Native(Box::new(Edges(edges))))
        .unwrap();
    (
        Rooted::new(heap, Value::Object(outer)),
        vec![outer, leaf, inner, deep],
    )
}

#[test]
fn native_payload_edges_retain() {
    let heap = Arc::new(Heap::default());
    let (root, ids) = native_holding(&heap);
    for _ in 0..2 {
        heap.request_collection().unwrap();
        assert!(ids.iter().all(|&id| heap.is_live(id)));
    }
    assert_eq!(heap.kind(ids[0]), Some(ObjectKind::Native));
    drop(root);
}

#[test]
fn native_payload_edges_reclaim() {
    let heap = Arc::new(Heap::default());
    let (root, ids) = native_holding(&heap);
    heap.request_collection().unwrap();
    assert!(ids.iter().all(|&id| heap.is_live(id)));
    drop(root);
    assert_eq!(heap.request_collection().unwrap().reclaimed, ids.len());
    assert_stale(&heap, &ids);
}

#[test]
fn engine_error_cause_retains_and_reclaims() {
    use crate::error::{Diagnostic, VmError, VmErrorKind};
    use crate::ids::{TaskId, WorldId};
    use crate::object::{DeleteOutcome, OrdinaryObject, PropertyFlags};
    use crate::value::JsString;
    use crate::vm::exception::ErrorObject;
    use crate::{DependencyId, DependencyKey, ResultKind};

    // Crate-internal holder test: public dependency cells retain their
    // outcomes for the lifetime of the runtime, masking this slot's edge.
    for delete in [false, true] {
        let heap = Arc::new(Heap::default());
        let task = TaskId::from_raw(1);
        let cause = crate::collections::new_vector(&heap, &[Value::Number(13.0)]).unwrap();
        let Value::Object(cause_id) = cause else {
            unreachable!()
        };
        let key = DependencyKey::new(WorldId::INITIAL, DependencyId::new(230), ResultKind::new(1));
        let original = VmError {
            kind: VmErrorKind::DependencyFailed(key),
            message: "dependency failed".into(),
            diagnostic: Diagnostic {
                executable: "engine-cause-gc".into(),
                task,
                world: WorldId::INITIAL,
                instruction: 0,
                byte_offset: None,
                opcode: None,
                source: None,
                call_chain: vec![],
            },
        };
        let prototype = crate::realm::intrinsics(&heap).unwrap().error_prototype;
        let mut object =
            OrdinaryObject::with_prototype(task, prototype).with_engine_error(ErrorObject {
                error: original.clone(),
                cause: cause.clone(),
            });
        let cause_key = heap.keys().intern(&JsString::from("cause")).unwrap();
        object.define_with(cause_key, cause, PropertyFlags::METHOD);
        let id = heap.alloc(HeapObject::Plain(object)).unwrap();
        let root = Rooted::new(&heap, Value::Object(id));
        let weak_cause = root.error_cause().unwrap().downgrade();
        heap.with_object_mut(id, |object| {
            let HeapObject::Plain(object) = object else {
                unreachable!()
            };
            if delete {
                assert_eq!(object.delete(cause_key), DeleteOutcome::Deleted);
            } else {
                object.define_with(cause_key, Value::Null, PropertyFlags::METHOD);
            }
        })
        .unwrap();
        for _ in 0..2 {
            heap.request_collection().unwrap();
            assert!(
                heap.is_live(cause_id),
                "the internal slot is the only cause edge"
            );
            assert!(
                root.error_cause()
                    .unwrap()
                    .same_object(&weak_cause.upgrade().unwrap())
            );
            assert_eq!(root.error(), Some(original.clone()));
        }
        drop(root);
        heap.request_collection().unwrap();
        assert!(!weak_cause.is_live());
        assert_stale(&heap, &[id, cause_id]);
    }
}

/// A completed cell roots its outcome for as long as the cell exists (the
/// runtime's table, in public use); dropping the table releases it.
#[test]
fn dependency_outcome_reclaims_with_its_cell() {
    use crate::dep::DependencyTable;
    use crate::{Completion, DependencyId, DependencyKey, ResultKind, WorldId};
    let heap = Arc::new(Heap::default());
    let table = DependencyTable::new(Arc::clone(&heap));
    let mut ids = Vec::new();
    for (def, failed) in [(1, false), (2, true)] {
        let key = DependencyKey::new(WorldId::INITIAL, DependencyId::new(def), ResultKind::new(0));
        let request = table.claim(key).unwrap();
        let inner = crate::collections::new_vector(&heap, &[]).unwrap();
        let outer = crate::collections::new_vector(&heap, std::slice::from_ref(&inner)).unwrap();
        let root = Rooted::new(&heap, outer.clone());
        let completion = if failed {
            Completion::Failed(&root)
        } else {
            Completion::Ready(&root)
        };
        let wakeups = crate::dep::complete(&table, key, request.producer(), completion).unwrap();
        assert!(wakeups.deliveries().is_empty());
        drop(root);
        ids.extend([&outer, &inner].map(|v| match *v {
            Value::Object(id) => id,
            ref other => panic!("{other:?}"),
        }));
    }
    heap.request_collection().unwrap();
    assert!(ids.iter().all(|&id| heap.is_live(id)), "held by the cells");
    drop(table);
    assert_eq!(heap.request_collection().unwrap().reclaimed, ids.len());
    assert_stale(&heap, &ids);
}

/// The realm lives as long as its heap, so a runtime never releases it;
/// dropping the realm slot directly shows that it is the intrinsics' only
/// holder (with a task-made object that inherits from `Object.prototype`
/// already gone).
#[test]
fn realm_intrinsics_reclaim_with_their_heap() {
    let mut heap = Heap::default();
    let mut ids = Vec::new();
    let prototype = {
        let intrinsics = crate::realm::intrinsics(&heap).unwrap();
        intrinsics.trace(&mut |id| ids.push(id));
        intrinsics.object_prototype
    };
    assert_eq!(ids.len(), 191); // Includes BigInt and fifteen Date UTC field methods (-181).
    let task = crate::ids::TaskId::from_raw(1);
    let object = crate::object::OrdinaryObject::with_prototype(task, prototype);
    let child = heap.alloc(HeapObject::Plain(object)).unwrap();
    assert_eq!(heap.request_collection().unwrap().reclaimed, 1);
    assert!(!heap.is_live(child), "an inheriting object does not stay");
    assert!(ids.iter().all(|&id| heap.is_live(id)), "held by the realm");
    let snapshot = heap.root_snapshot().unwrap();
    assert!(snapshot.iter().all(|(c, _)| *c == RootCategory::Intrinsic));
    assert_eq!(snapshot.len(), ids.len());
    assert!(heap.realm_mut().take().is_some());
    let heap = Arc::new(heap);
    assert_eq!(heap.request_collection().unwrap().reclaimed, ids.len());
    assert_stale(&heap, &ids);
}

/// Host of the allocating spinner: reports its first effect, then answers
/// "continue" until `stop` is set (failing after a bound).
struct SpinHost {
    entered: Option<std::sync::mpsc::Sender<()>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    deadline: std::time::Instant,
}

impl crate::Host for SpinHost {
    fn effect(&mut self, _: crate::HostCall) -> Result<crate::Primitive, String> {
        if let Some(entered) = self.entered.take() {
            let _ = entered.send(());
        }
        if std::time::Instant::now() > self.deadline {
            return Err("spin ran past its deadline".to_string());
        }
        let stop = self.stop.load(std::sync::atomic::Ordering::Acquire);
        Ok(crate::Primitive::Bool(!stop))
    }
}

/// The crate-internal holders under the multi-worker conditions of
/// `every_holder_retains_while_other_workers_allocate_and_collect` in
/// `tests/gc_roots.rs`: worker 0 runs an allocating loop without a budget
/// while worker 1 requests every collection, so each collection stops
/// worker 0 at a VM safepoint mid-run. (A dependency outcome is covered
/// there; its cell table cannot be dropped while the runtime lives.)
#[test]
fn crate_internal_holders_retain_while_other_workers_allocate_and_collect() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, mpsc};
    use std::time::{Duration, Instant};

    use crate::exec::{EventKind, ExecutorConfig, Placement, WorkerId};

    const COLLECTIONS: usize = 6;
    const TIMEOUT: Duration = Duration::from_secs(30);
    let (allocator, collector) = (WorkerId::new(0), WorkerId::new(1));
    let rt = crate::Runtime::new();
    let heap = Arc::clone(rt.heap());
    let constant = plain(&heap);
    let constant_task = spawn_with_constant(&rt, constant);
    let (native_root, native_ids) = native_holding(&heap);

    let stop = Arc::new(AtomicBool::new(false));
    let (entered_tx, entered_rx) = mpsc::channel();
    let entered_tx = Mutex::new(Some(entered_tx));
    let exec = {
        let stop = Arc::clone(&stop);
        ExecutorConfig::new()
            .workers(2)
            .host(move |_| {
                Box::new(SpinHost {
                    entered: entered_tx.lock().unwrap().clone(),
                    stop: Arc::clone(&stop),
                    deadline: Instant::now() + TIMEOUT,
                })
            })
            .driver(|guard, cx| guard.run_with(crate::Budget::UNLIMITED, &mut *cx.host))
            .start(&rt)
            .unwrap()
    };
    let spinner = crate::frontend::compile_function(
        "function f() { while (__rt.effect(0, 0)) { const g = __rt.vector(1, 2); } return 0; }",
    )
    .unwrap();
    let task = rt.spawn(&spinner, &[]).unwrap();
    let id = task.id();
    exec.submit_with(task, Placement::Worker(allocator))
        .unwrap();
    // Only the spinner runs effects, and only on worker 0.
    entered_rx
        .recv_timeout(TIMEOUT)
        .expect("the spinner never started");

    for _ in 0..COLLECTIONS {
        exec.schedule_collection(Placement::Worker(collector));
    }
    let mut collections = Vec::new();
    assert!(exec.wait_for(TIMEOUT, |events| {
        collections = events
            .iter()
            .filter_map(|e| match &e.kind {
                EventKind::Collected(c) if c.requester == Some(collector) => Some(c.clone()),
                _ => None,
            })
            .collect();
        collections.len() >= COLLECTIONS
    }));
    // The first request had to wait for the running worker; a later one may
    // find it still parked from the previous collection.
    assert!(collections[0].waited);
    for c in &collections {
        assert_eq!(
            c.state_of(allocator),
            Some(crate::MutatorState::Safepoint),
            "collection {}: the allocating worker was not stopped mid-run",
            c.number
        );
    }
    // Reclamation is permanent, so live now means live through every
    // collection above.
    assert!(heap.is_live(constant));
    assert!(native_ids.iter().all(|&id| heap.is_live(id)));

    stop.store(true, Ordering::Release);
    let outcome = exec
        .take_outcome(id, TIMEOUT)
        .expect("the spinner finished");
    assert!(matches!(outcome, crate::RunOutcome::Complete(_)));
    drop(exec.shutdown(TIMEOUT).unwrap());

    drop((constant_task, native_root));
    rt.request_collection().unwrap();
    let mut ids = native_ids;
    ids.push(constant);
    assert_stale(&heap, &ids);
}

/// Synthetic source fixture; only the realm cache retains the arrays after
/// VM state and executable handles are gone. Release the realm to reclaim.
#[test]
fn template_cache_retains_and_reclaims() {
    use crate::vm::dispatch::{Exit, execute};
    use crate::vm::frames::{DiagnosticContext, VmLimits, VmState};
    let mut heap = Heap::default();
    let code = crate::frontend::compile_script("cache", "(s => s)`x`").unwrap();
    let diag = DiagnosticContext {
        task: crate::ids::TaskId::from_raw(1),
        world: crate::ids::WorldId::INITIAL,
    };
    let mut state = VmState::new(&code.0, &[], diag, VmLimits::DEFAULT);
    let Exit::Return(Value::Object(cooked)) =
        execute(&mut state, &heap, &mut crate::host::NoHost, None)
    else {
        panic!("template evaluation failed");
    };
    let raw_key = heap
        .keys()
        .lookup(&crate::value::JsString::from("raw"))
        .unwrap();
    let raw = heap
        .read(cooked, |o| {
            let HeapObject::Array(a) = o else {
                panic!("not array");
            };
            let Value::Object(raw) = a.object().get_own(raw_key).unwrap().value else {
                panic!("not object");
            };
            raw
        })
        .unwrap();
    drop((state, code));
    heap.request_collection().unwrap();
    assert!(heap.is_live(cooked) && heap.is_live(raw));
    let snapshot = heap.root_snapshot().unwrap();
    assert!(snapshot.contains(&(RootCategory::Intrinsic, cooked)));
    drop(heap.realm_mut().take());
    heap.request_collection().unwrap();
    assert_stale(&Arc::new(heap), &[cooked, raw]);
}
