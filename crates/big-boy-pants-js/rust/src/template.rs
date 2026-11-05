//! Checkpoint templates for initialized, command-driven entry tasks.
//! Executable code and explicitly shared values retain identity; private object
//! graphs are copied with one identity map, including closure cells and cycles.
//! Unsupported state is rejected, never silently shared or partially reset.

use crate::heap::gc::{RootCategory, RootSource, Trace, TraceEdge};
use crate::heap::{Heap, HeapObject, ObjectId, Rooted};
use crate::ids::TaskId;
use crate::value::{Value, share};
use crate::vm::Executable;
use crate::vm::frames::VmTemplate;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

// Only supported, immutable snapshot bodies inhabit this enum. In particular
// no NativePayload or suspended native continuation crosses into a template.
macro_rules! nodes {
    ($($kind:ident($body:ty)),* $(,)?) => {
        pub(crate) enum Node { $($kind($body)),* }
        impl Node {
            fn from_object(object: HeapObject) -> Self {
                match object { $(HeapObject::$kind(body) => Self::$kind(body),)* _ => unreachable!() }
            }
        }
        impl Trace for Node {
            fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
                match self { $(Self::$kind(body) => body.trace(visit),)* }
            }
        }
    }
}
nodes! {
    Vector(crate::collections::PVector),
    Plain(crate::object::OrdinaryObject),
    Array(Box<crate::object::array::ArrayObject>),
    JsMap(Box<crate::object::map::MapObject>),
    JsSet(Box<crate::object::map::MapObject>),
    WeakMap(Box<crate::object::weak_map::WeakMapObject>),
    WeakSet(Box<crate::object::weak_map::WeakMapObject>),
    Proxy(crate::vm::proxy::Proxy),
    Cell(crate::vm::function::BindingCell),
    Function(crate::vm::function::FunctionObject),
    BoundFunction(crate::vm::function::BoundFunction),
}

pub(crate) type IdMap = HashMap<ObjectId, ObjectId>;
pub(crate) fn remap_id(id: ObjectId, ids: &IdMap) -> ObjectId {
    ids.get(&id).copied().unwrap_or(id)
}
pub(crate) fn remap_value(value: &mut Value, ids: &IdMap) {
    if let Value::Object(id) = value {
        *id = remap_id(*id, ids);
    }
}

/// An immutable snapshot of a task at an entry-frame dependency boundary.
/// Instances have independent globals, closures, cells and mutable collections.
/// Primitive values, published values and explicit concurrent references keep
/// their identities. This is intentionally not a general JavaScript snapshot:
/// active continuations, internal slots and populated weak collections fail.
#[derive(Clone)]
pub struct TaskTemplate(pub(crate) Arc<TemplateData>);

pub(crate) struct TemplateData {
    pub(crate) heap: Arc<Heap>,
    pub(crate) executable: Executable,
    pub(crate) vm: VmTemplate,
    pub(crate) nodes: Vec<(ObjectId, Node)>,
    pub(crate) anchors: Vec<ObjectId>,
    private: HashSet<ObjectId>,
    _reservation: crate::heap::budget::ByteReservation,
}

impl RootSource for TemplateData {
    fn trace_roots(&self, visit: &mut dyn FnMut(RootCategory, TraceEdge)) {
        let mut edge = |edge| {
            // Private IDs are relocation labels, not references to the live
            // source task. Keeping those objects alive would retain subsequent
            // source-task mutations, even after that task had finished.
            if !matches!(edge, TraceEdge::Object(id) if self.private.contains(&id)) {
                visit(RootCategory::PersistentHandle, edge);
            }
        };
        self.vm.trace(&mut edge);
        for (_, node) in &self.nodes {
            node.trace(&mut edge);
        }
    }
}

pub(crate) fn capture(
    heap: &Arc<Heap>,
    executable: Executable,
    vm: VmTemplate,
    anchors: &[&Rooted],
    owner: TaskId,
) -> Result<TaskTemplate, &'static str> {
    let anchors = anchors
        .iter()
        .map(|root| match root.value_in(heap) {
            Ok(Value::Object(id)) => Ok(id),
            _ => Err("template: anchors must be objects of this runtime"),
        })
        .collect::<Result<Vec<_>, _>>()?;
    // An anchor must not occur inside a published object or atom: those retain
    // identity and are deliberately not traversed or rewritten. Requiring a
    // non-publishable parameter makes that invariant transitive.
    for &id in &anchors {
        if !has_private_state(heap, id) {
            return Err("template: replaceable parameters must not be publishable");
        }
    }
    let mut seen: HashSet<ObjectId> = anchors.iter().copied().collect();
    let mut pending = Vec::new();
    vm.trace(&mut |edge| {
        if let TraceEdge::Object(id) = edge {
            pending.push(id);
        }
    });
    let mut nodes = Vec::new();
    let mut reservation = heap
        .reserve_bytes(4096)
        .ok_or("template: heap byte limit exceeded")?;
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        reservation.merge(
            heap.reserve_bytes(std::mem::size_of::<HeapObject>() + 512)
                .ok_or("template: heap byte limit exceeded")?,
        );
        if heap.known_shareable(id) == Some(true) {
            continue;
        }
        let persistent = heap
            .read(id, |o| {
                matches!(
                    o,
                    HeapObject::Map(_)
                        | HeapObject::Occurrence(_)
                        | HeapObject::Form(_)
                        | HeapObject::Atom(_)
                )
            })
            .ok_or("template: stale object")?;
        if persistent && share_data(heap, id) {
            continue;
        }
        let node = heap
            .read(id, |object| {
                let object_owner = match object {
                    HeapObject::Plain(o) => o.owner(),
                    HeapObject::Array(o) => o.object().owner(),
                    HeapObject::JsMap(o) | HeapObject::JsSet(o) => o.object().owner(),
                    HeapObject::WeakMap(o) | HeapObject::WeakSet(o) => o.object().owner(),
                    HeapObject::Function(o) => Some(o.owner),
                    HeapObject::BoundFunction(o) => Some(o.owner),
                    HeapObject::Cell(o) => Some(o.owner),
                    HeapObject::Proxy(o) => Some(o.owner),
                    _ => None,
                };
                if object_owner.is_some_and(|task| task != owner) {
                    return Err("template: foreign private object");
                }
                copy_object(heap, object, owner)
            })
            .ok_or("template: stale object")??;
        reservation.merge(
            heap.reserve_bytes(node.boxed_bytes())
                .ok_or("template: heap byte limit exceeded")?,
        );
        node.trace(&mut |edge| {
            if let TraceEdge::Object(id) = edge {
                pending.push(id);
            }
        });
        nodes.push((id, Node::from_object(node)));
    }
    let private = nodes
        .iter()
        .map(|(id, _)| *id)
        .chain(anchors.iter().copied())
        .collect();
    let data = Arc::new(TemplateData {
        heap: heap.clone(),
        executable,
        vm,
        nodes,
        anchors,
        private,
        _reservation: reservation,
    });
    let root: Arc<dyn RootSource> = data.clone();
    heap.register_source(Arc::downgrade(&root));
    Ok(TaskTemplate(data))
}

fn has_private_state(heap: &Heap, root: ObjectId) -> bool {
    let mut pending = vec![root];
    let mut seen = HashSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) || heap.known_shareable(id) == Some(true) {
            continue;
        }
        let private = heap.read(id, |object| match share::classify(object) {
            share::Shareability::TaskOwned => true,
            share::Shareability::ConcurrentReference => false,
            share::Shareability::Persistent => {
                object.trace(&mut |edge| {
                    if let TraceEdge::Object(id) = edge {
                        pending.push(id);
                    }
                });
                false
            }
        });
        if private != Some(false) {
            return true;
        }
    }
    false
}

/// Publication may seal previously private functions. Capturing a checkpoint
/// must not change the source task's ability to mutate them, so only publish
/// data graphs whose unpublished nodes are all persistent data or atoms.
fn share_data(heap: &Heap, root: ObjectId) -> bool {
    let mut pending = vec![root];
    let mut seen = HashSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) || heap.known_shareable(id) == Some(true) {
            continue;
        }
        let valid = heap.read(id, |object| match object {
            HeapObject::Atom(_) => true,
            HeapObject::Vector(_)
            | HeapObject::Map(_)
            | HeapObject::Occurrence(_)
            | HeapObject::Form(_) => {
                object.trace(&mut |edge| {
                    if let TraceEdge::Object(id) = edge {
                        pending.push(id);
                    }
                });
                true
            }
            _ => false,
        });
        if valid != Some(true) {
            return false;
        }
    }
    share::check(heap, &Value::Object(root)).is_ok()
}

macro_rules! copy_body {
    ($kind:ident, $object:expr, $heap:expr, $owner:expr) => {{
        let heap = $heap;
        let owner = $owner;
        Ok(match $object {
            $kind::Vector(o) => HeapObject::Vector(o.clone()),
            $kind::Plain(o) => HeapObject::Plain(o.template_copy(owner)?),
            $kind::Array(o) => HeapObject::Array(Box::new(o.template_copy(owner)?)),
            $kind::JsMap(o) => HeapObject::JsMap(Box::new(o.template_copy(owner)?)),
            $kind::JsSet(o) => HeapObject::JsSet(Box::new(o.template_copy(owner)?)),
            $kind::WeakMap(o) => HeapObject::WeakMap(Box::new(o.template_copy(owner)?)),
            $kind::WeakSet(o) => HeapObject::WeakSet(Box::new(o.template_copy(owner)?)),
            $kind::Proxy(o) => HeapObject::Proxy(o.template_copy(owner)?),
            $kind::Cell(o) => HeapObject::Cell(crate::vm::function::BindingCell {
                owner,
                value: o.value.clone(),
                mode: o.mode,
            }),
            $kind::Function(o) => {
                let reservation = o
                    .reservation
                    .as_ref()
                    .map_or_else(|| heap.reserve_bytes(0), |r| r.duplicate())
                    .ok_or("template: heap byte limit exceeded")?;
                let mut f = o.clone();
                f.owner = owner;
                f.captures = o.captures.to_vec().into();
                f.reservation = Some(Arc::new(reservation));
                HeapObject::Function(f)
            }
            $kind::BoundFunction(o) => {
                let reservation = o
                    ._reservation
                    .as_ref()
                    .map_or_else(|| heap.reserve_bytes(0), |r| r.duplicate())
                    .ok_or("template: heap byte limit exceeded")?;
                let mut f = o.clone();
                f.owner = owner;
                f.args = o.args.to_vec().into();
                f._reservation = Some(Arc::new(reservation));
                HeapObject::BoundFunction(f)
            }
            _ => return Err("template: unsupported object kind"),
        })
    }};
}

pub(crate) fn copy_object(
    heap: &Heap,
    object: &HeapObject,
    owner: TaskId,
) -> Result<HeapObject, &'static str> {
    copy_body!(HeapObject, object, heap, owner)
}
impl Node {
    #[allow(unreachable_patterns)]
    pub(crate) fn copy(&self, heap: &Heap, owner: TaskId) -> Result<HeapObject, &'static str> {
        copy_body!(Node, self, heap, owner)
    }
}

pub(crate) fn remap_object(object: &mut HeapObject, ids: &IdMap) {
    match object {
        HeapObject::Plain(o) => o.template_remap(ids),
        HeapObject::Array(o) => o.template_remap(ids),
        HeapObject::JsMap(o) | HeapObject::JsSet(o) => o.template_remap(ids),
        HeapObject::WeakMap(o) | HeapObject::WeakSet(o) => o.template_remap(ids),
        HeapObject::Proxy(o) => o.template_remap(ids),
        HeapObject::Cell(o) => o.value.iter_mut().for_each(|v| remap_value(v, ids)),
        HeapObject::Function(o) => {
            Arc::make_mut(&mut o.captures)
                .iter_mut()
                .for_each(|v| remap_value(v, ids));
            o.properties = o.properties.map(|id| remap_id(id, ids));
            o.home_object = o.home_object.map(|id| remap_id(id, ids));
            o.apply_to.iter_mut().for_each(|v| remap_value(v, ids));
        }
        HeapObject::BoundFunction(o) => {
            remap_value(&mut o.target, ids);
            remap_value(&mut o.this, ids);
            Arc::make_mut(&mut o.args)
                .iter_mut()
                .for_each(|v| remap_value(v, ids));
            o.metadata = remap_id(o.metadata, ids);
            o.properties = o.properties.map(|id| remap_id(id, ids));
        }
        _ => unreachable!("only supported template nodes can be instantiated"),
    }
}
