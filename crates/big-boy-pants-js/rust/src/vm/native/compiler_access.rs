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
static GROUP: FixedText = FixedText::new("group");
static ORDER: FixedText = FixedText::new("order");
static FRONTIER: FixedText = FixedText::new("frontier");
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
                // `f.call`, as the legacy `f.call(null, …)` reads it: a native
                // form's member defers to the task's resolver
                // (`property::Deferred::Form`), like any property read.
                let read = property::read(
                    heap,
                    state.diag.task,
                    &callee,
                    &property::Key::Name(CALL.get()),
                )?;
                read_result(heap, state, read, callee)?;
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
            // An embedder may require the live Proxy's visibility policy on
            // every access, including data properties and ready products.
            if collections::count(heap, context.clone())? > 6
                && collections::nth(heap, context.clone(), 6)? == Value::Bool(true)
            {
                let guarded = if receiver == collections::nth(heap, context.clone(), 0)? {
                    collections::nth(heap, context.clone(), 4)?
                } else {
                    receiver
                };
                let read = property::read(heap, state.diag.task, &guarded, &key)?;
                read_result(heap, state, read, guarded)?;
                return Ok(true);
            }
            // The context is created once per compiler task. It retains the
            // original Proxy, its target and its live task-owned overlay Map.
            // Replacing the public registry takes the ordinary property path.
            let proxy = collections::nth(heap, context.clone(), 0)?;
            let (target, readers, missing, frontier) = if receiver == proxy {
                let target = collections::nth(heap, context.clone(), 1)?;
                let overlays = collections::nth(heap, context.clone(), 2)?;
                if map_get(heap, state.diag.task, &overlays, &name)?.is_some() {
                    // Bound values still pass through the original Proxy.
                    // Its get invariant rejects a different value for a fixed
                    // target property or an accessor without a getter.
                    let read = property::read(heap, state.diag.task, &receiver, &key)?;
                    read_result(heap, state, read, receiver)?;
                    return Ok(true);
                }
                let count = collections::count(heap, context.clone())?;
                let missing = (count > 4)
                    .then(|| collections::nth(heap, context.clone(), 4))
                    .transpose()?;
                // The declaration frontier of the running initializer: a
                // bundle declared after it is read through its getter, which
                // selects the definition visible there (`declarationSlot`).
                // While a definition group executes, its own products take
                // precedence over any installed bundle (`productOwner`), so
                // every product read takes its getter.
                let frontier = match (count > 5)
                    .then(|| collections::nth(heap, context.clone(), 5))
                    .transpose()?
                {
                    Some(cell)
                        if data(heap, state.diag.task, &cell, &GROUP)?
                            == Some(Value::Bool(true)) =>
                    {
                        f64::NEG_INFINITY
                    }
                    Some(cell) => match data(heap, state.diag.task, &cell, &FRONTIER)? {
                        Some(Value::Number(n)) => n,
                        _ => f64::INFINITY,
                    },
                    None => f64::INFINITY,
                };
                (
                    target,
                    Some(collections::nth(heap, context, 3)?),
                    missing,
                    frontier,
                )
            } else {
                (receiver.clone(), None, None, f64::INFINITY)
            };
            let read = property::read(heap, state.diag.task, &target, &key)?;
            if let (
                Some(readers),
                Some(property::Read::Deferred(property::Deferred::Getter(getter))),
            ) = (readers, &read)
                && let Some(product) = map_get(heap, state.diag.task, &readers, getter)?
                && let bundle = collections::nth(heap, product.clone(), 0)?
                // Definition groups can fall back to a previous generation
                // for a conditional product, or expose the executing owner.
                // Their installed getter owns those decisions.
                && data(heap, state.diag.task, &bundle, &GROUP)? != Some(Value::Bool(true))
                && (frontier == f64::INFINITY
                    || matches!(
                        data(heap, state.diag.task, &bundle, &ORDER)?,
                        Some(Value::Number(order)) if order <= frontier
                    ))
                && matches!(
                    data(heap, state.diag.task, &bundle, &STATE)?,
                    Some(Value::Number(2.0))
                )
                && let Some(products) = data(heap, state.diag.task, &bundle, &PRODUCTS)?
            {
                // Only the exact compiler-installed getter takes this path.
                // Ready bundles' force is a state check. A plain product read
                // cannot mutate the root descriptor; a getter/Proxy can, so
                // it must run through the original root Proxy and its final
                // get invariant check. No value or root generation is cached.
                let Value::String(name) = collections::nth(heap, product, 1)? else {
                    return Err(Fault::TypeError(
                        "compiler product name must be a string".into(),
                    ));
                };
                let read =
                    property::read(heap, state.diag.task, &products, &property::Key::Name(name))?;
                if !matches!(read, Some(property::Read::Deferred(_))) {
                    read_result(heap, state, read, products)?;
                    return Ok(true);
                }
            }
            // Accessors may freeze or replace the root while suspended. The
            // original Proxy must check the descriptor after they return.
            // A name the target lacks is also read through the Proxy: a task
            // may install a name's catalog registrations when its Proxy is
            // first asked for it (light-cljs-590).
            let fallback = if read.is_none() {
                missing.as_ref().unwrap_or(&receiver)
            } else {
                &receiver
            };
            let read =
                if target != receiver && matches!(read, None | Some(property::Read::Deferred(_))) {
                    property::read(heap, state.diag.task, fallback, &key)?
                } else {
                    read
                };
            read_result(heap, state, read, fallback.clone())?;
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
    .ok_or_else(|| Fault::Internal("compiler root context was reclaimed"))?
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
