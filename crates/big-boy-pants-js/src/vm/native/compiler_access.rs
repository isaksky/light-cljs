//! Bounded compiler call preparation and live root reads. Property access
//! hands getters/Proxies to the ordinary task-owned continuation machinery;
//! no user code executes under a heap borrow or on this Rust stack.
use super::{Fault, VmState, read_result};
use crate::collections::{self, form};
use crate::heap::{Heap, HeapObject};
use crate::ids::TaskId;
use crate::value::{Value, string::FixedText};
use crate::vm::{instr::FormOp, property};

static CALL: FixedText = FixedText::new("call");
static STATE: FixedText = FixedText::new("state");
static PRODUCTS: FixedText = FixedText::new("products");
static MULTIFN: FixedText = FixedText::new("cljs.core/MultiFn");

pub(crate) fn start(state: &mut VmState, heap: &Heap, op: FormOp) -> Result<bool, Fault> {
    match op {
        FormOp::CljCallMember => {
            let entry = state.pop()?;
            let callee = form::unwrap(heap, state.pop()?);
            let ty = form::type_of(heap, &callee);
            if ty != Value::Null && MULTIFN.matches(&form::name(heap, &ty)?.name) {
                state.push(entry);
            } else {
                let read = property::read(
                    heap,
                    state.diag.task,
                    &callee,
                    &property::Key::Name(CALL.get()),
                )?;
                read_result(state, read, callee)?;
            }
        }
        FormOp::CljReadRoot => {
            let name = state.pop()?;
            let context = state.pop()?;
            let receiver = state.pop()?;
            let Value::String(key) = &name else {
                return Err(Fault::TypeError(
                    "compiler root name must be a string".into(),
                ));
            };
            let key = property::Key::Name(key.clone());
            // The context is created once per compiler task. It retains the
            // original Proxy, its target and its live task-owned overlay Map.
            // Replacing the public registry takes the ordinary property path.
            let proxy = collections::nth(heap, context.clone(), 0)?;
            let (target, readers) = if receiver == proxy {
                let target = collections::nth(heap, context.clone(), 1)?;
                let overlays = collections::nth(heap, context.clone(), 2)?;
                if let Some(value) = map_get(heap, state.diag.task, &overlays, &name)? {
                    state.push(value);
                    return Ok(true);
                }
                (target, Some(collections::nth(heap, context, 3)?))
            } else {
                (receiver.clone(), None)
            };
            let read = property::read(heap, state.diag.task, &target, &key)?;
            if let (
                Some(readers),
                Some(property::Read::Deferred(property::Deferred::Getter(getter))),
            ) = (readers, &read)
                && let Some(product) = map_get(heap, state.diag.task, &readers, getter)?
                && let bundle = collections::nth(heap, product.clone(), 0)?
                && matches!(
                    data(heap, state.diag.task, &bundle, &STATE)?,
                    Some(Value::Number(2.0))
                )
                && let Some(products) = data(heap, state.diag.task, &bundle, &PRODUCTS)?
            {
                // Only the exact compiler-installed getter takes this path.
                // Ready bundles' force is a state check; read the live product
                // property, including an ordinary getter/Proxy, with its own
                // receiver. Neither a value nor a root generation is cached.
                let Value::String(name) = collections::nth(heap, product, 1)? else {
                    return Err(Fault::TypeError(
                        "compiler product name must be a string".into(),
                    ));
                };
                let read =
                    property::read(heap, state.diag.task, &products, &property::Key::Name(name))?;
                read_result(state, read, products)?;
                return Ok(true);
            }
            read_result(state, read, receiver)?;
        }
        _ => return Ok(false),
    }
    Ok(true)
}

fn map_get(heap: &Heap, task: TaskId, map: &Value, key: &Value) -> Result<Option<Value>, Fault> {
    let Value::Object(id) = map else {
        return Err(Fault::TypeError(
            "compiler root context requires a Map".into(),
        ));
    };
    heap.read(*id, |object| {
        let HeapObject::JsMap(map) = object else {
            return Err(Fault::TypeError(
                "compiler root context requires a Map".into(),
            ));
        };
        if map.object().owner() != Some(task) {
            return Err(Fault::ForeignObject {
                owner: map.object().owner(),
                caller: task,
            });
        }
        Ok(map.get(key).cloned())
    })
    .ok_or(Fault::Internal("compiler root context was reclaimed"))?
}

// Peeking never executes an accessor. If compiler-owned bookkeeping has
// become exotic, the original getter performs its normal reads and effects.
fn data(
    heap: &Heap,
    task: TaskId,
    object: &Value,
    name: &FixedText,
) -> Result<Option<Value>, Fault> {
    Ok(
        match property::read(heap, task, object, &property::Key::Name(name.get()))? {
            Some(property::Read::Value(value)) => Some(value),
            _ => None,
        },
    )
}
