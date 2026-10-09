use super::*;

fn graph(context: &Context, edges: Vec<Target>) -> RootedValue {
    let id = HeapId(
        context
            .owner
            .store
            .borrow_mut()
            .heap
            .insert(HeapValue::Edges(edges)),
    );
    context.owner.value(Value::Heap(id))
}

#[test]
fn collector_traces_cycles_through_units_names_and_sources_then_reclaims_them() {
    let mut context = Context::new();
    let owner = Rc::downgrade(&context.owner);
    let name = context.symbol(Name::new(Some("example"), "capture"));
    let source = context.source("cycle.clj", "capture");
    let first = graph(&context, vec![]);
    let second = graph(
        &context,
        vec![Target::Value(first.value()), Target::Value(name.value())],
    );
    let unit = context
        .executable_unit(&[second], &[source.span(0..7).unwrap()])
        .unwrap();
    let first_id = first.heap_id().unwrap();
    *context.owner.store.borrow_mut().heap.get_mut(first_id.0) =
        HeapValue::Edges(vec![Target::Unit(unit.id())]);
    let source_data = Rc::downgrade(&source.file);
    let code_data = Rc::downgrade(&unit.code);
    drop((name, source, unit));

    let live = context.collect_garbage().after;
    assert_eq!(
        (
            live.heap_values,
            live.names,
            live.sources,
            live.executable_units,
            live.roots
        ),
        (2, 1, 1, 1, 1)
    );
    assert!(source_data.upgrade().is_some());
    assert!(code_data.upgrade().is_some());
    let clone = first.clone();
    drop(first);
    assert_eq!(context.collect_garbage().after, live);
    drop(clone);
    let dead = context.collect_garbage().after;
    assert_eq!(
        (
            dead.heap_values,
            dead.names,
            dead.sources,
            dead.executable_units,
            dead.roots
        ),
        (0, 0, 0, 0, 0)
    );
    assert!(source_data.upgrade().is_none());
    assert!(code_data.upgrade().is_none());
    assert!(context.owner.store.borrow().interned.is_empty());
    assert!(context.owner.store.borrow().trace.is_empty());
    assert!(context.owner.roots.borrow().is_empty());
    drop(context);
    assert!(owner.upgrade().is_none());
}

#[test]
fn dropping_the_last_host_handle_drops_owner_even_with_an_uncollected_cycle() {
    let context = Context::new();
    let owner = Rc::downgrade(&context.owner);
    let value = graph(&context, vec![]);
    let id = value.heap_id().unwrap();
    *context.owner.store.borrow_mut().heap.get_mut(id.0) =
        HeapValue::Edges(vec![Target::Value(value.value())]);
    drop(context);
    assert!(owner.upgrade().is_some());
    drop(value);
    assert!(owner.upgrade().is_none());
}

#[test]
fn deep_graphs_use_an_iterative_trace_and_reuse_empty_scratch() {
    let mut context = Context::new();
    let mut root = graph(&context, vec![]);
    for _ in 0..20_000 {
        root = graph(&context, vec![Target::Value(root.value())]);
    }
    assert_eq!(context.collect_garbage().after.heap_values, 20_001);
    drop(root);
    assert_eq!(context.collect_garbage().after.heap_values, 0);
    assert!(context.owner.store.borrow().trace.is_empty());
}

#[test]
fn collection_reuses_root_trace_and_vacant_slot_capacity_without_dead_edges() {
    let mut context = Context::new();
    let mut capacities = None;
    for _ in 0..8 {
        let leaves: Vec<_> = (0..256).map(|_| graph(&context, vec![])).collect();
        let root = graph(
            &context,
            leaves.iter().map(|v| Target::Value(v.value())).collect(),
        );
        drop(leaves);
        assert_eq!(context.collect_garbage().after.heap_values, 257);
        drop(root);
        assert_eq!(context.collect_garbage().after.heap_values, 0);
        let store = context.owner.store.borrow();
        let roots = context.owner.roots.borrow();
        assert!(store.trace.is_empty() && roots.is_empty());
        let current = (store.trace.capacity(), roots.capacity(), store.heap.slots());
        if let Some(capacities) = capacities {
            assert_eq!(current, capacities);
        } else {
            capacities = Some(current);
        }
    }
}
