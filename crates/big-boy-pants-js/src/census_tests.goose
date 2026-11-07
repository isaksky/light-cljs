import std;
import .source_vm;
import .census;

// census::take on a hand-built heap with known reachability: a cycle through
// a closure's slot run and a cell, a property chain, overlapping string
// ranges, an unowned slot, and two disconnected objects.
fn node(id: i64) -> source::Value { return source::object(id); }
fn text(heap: source::Heap&, start: i64, count: i64) -> source::Value { return source::string_ref(heap, start, count, true); }
fn main() {
    var code = source::Code {}; var heap = source::Heap { code: code };
    for u in "abcdef" { heap.units.push(u); }
    // 0: dictionary-mode object (light-cljs-787): records 3..6, the last -> closure 1.
    heap.nodes.push(source::Node { kind: node_kind::Object, keys: 3, key_count: 3, key_capacity: 3, initialized: true });
    // 1: closure whose slots 0..2 hold the cell and a string.
    heap.nodes.push(source::Node { kind: node_kind::Closure, start: 0, count: 2 });
    // 2: cell pointing back at the closure (a cycle).
    heap.nodes.push(source::Node { kind: node_kind::Binding, value: node(1) });
    // 3: disconnected dictionary-mode object with records 0..3;
    // 4: disconnected, empty.
    heap.nodes.push(source::Node { kind: node_kind::Object, keys: 0, key_count: 3, key_capacity: 3, initialized: true });
    heap.nodes.push(source::Node { kind: node_kind::Object });
    heap.values.push(node(2)); heap.values.push(text(heap, 0, 3)); heap.values.push(source::number(7));
    heap.properties.push(source::Property { key: text(heap, 3, 2), value: source::number(1) });
    heap.properties.push(source::Property { key: text(heap, 3, 1), value: source::number(8) });
    heap.properties.push(source::Property { key: text(heap, 4, 1), value: source::number(9) });
    heap.properties.push(source::Property { key: text(heap, 0, 1), value: source::number(5) });
    heap.properties.push(source::Property { key: text(heap, 0, 2), value: source::number(6) });
    heap.properties.push(source::Property { key: text(heap, 0, 1), value: node(1) });
    let c = census::take(heap, [node(0)]);
    assert(c.nodes == 5 && c.values == 3 && c.properties == 6 && c.units == 6);
    // Dead: nodes 3 and 4, slot 2, properties 0..3, units 3..6 ("def").
    assert(c.dead_nodes == 2 && c.dead_values == 1 && c.dead_properties == 3 && c.dead_units == 3);
    // A root inside the cycle reaches the same rows except object 0 and its property.
    let inner = census::take(heap, [node(2)]);
    assert(inner.dead_nodes == 3 && inner.dead_values == 1 && inner.dead_properties == 6 && inner.dead_units == 3);
    // No roots: everything is dead.
    let none: source::Value[] = [];
    let empty = census::take(heap, none);
    assert(empty.dead_nodes == 5 && empty.dead_values == 3 && empty.dead_properties == 6 && empty.dead_units == 6);
    shaped();
    elements_and_key_lists();
    print("census tests passed");
}
// Element runs and key lists (light-cljs-804): a node reachable only through
// an ordinary object's element slot is live, and so are a key list's walked
// object and its records' key text; the run's holes count as its live rows;
// an unlinked node and value are dead.
fn elements_and_key_lists() {
    var code = source::Code {}; var heap = source::Heap { code: code };
    for u in "xy" { heap.units.push(u); }
    // 0: ordinary object, element run values 0..3: hole, node 1, hole.
    heap.nodes.push(source::Node { kind: node_kind::Object, start: 0, count: 3 });
    heap.nodes.push(source::Node { kind: node_kind::Object });
    // 2: key list walking node 3, one record keyed "x" in properties 0..2.
    heap.nodes.push(source::Node { kind: node_kind::KeyList, keys: 0, key_count: 1, key_capacity: 2, value: node(3) });
    heap.nodes.push(source::Node { kind: node_kind::Object });
    // 4: unlinked; value 3 unowned.
    heap.nodes.push(source::Node { kind: node_kind::Object });
    heap.values.push(source::hole()); heap.values.push(node(1)); heap.values.push(source::hole()); heap.values.push(source::number(1));
    heap.properties.push(source::Property { key: text(heap, 0, 1) }); heap.properties.push(source::Property {});
    let c = census::take(heap, [node(0), node(2)]);
    assert(c.nodes == 5 && c.values == 4 && c.properties == 2 && c.units == 2);
    assert(c.dead_nodes == 1 && c.dead_values == 1 && c.dead_properties == 1 && c.dead_units == 1);
    // Without the key list, its walked object and key text are dead.
    let elements = census::take(heap, [node(0)]);
    assert(elements.dead_nodes == 3 && elements.dead_values == 1 && elements.dead_properties == 2 && elements.dead_units == 2);
}
// Shape mode, prototypes and accessors (light-cljs-787): nodes reachable only
// through a shaped object's inline slot, its overflow run, its prototype, or
// an accessor pair's getter and setter are live; one unlinked node is dead.
fn shaped() {
    var code = source::Code {}; var heap = source::Heap { code: code };
    let root = source::allocate(heap, source::Node { kind: node_kind::Object }); let proto = source::allocate(heap, source::Node { kind: node_kind::Object });
    let inline = source::allocate(heap, source::Node { kind: node_kind::Binding }); let spilled = source::allocate(heap, source::Node { kind: node_kind::Binding });
    let getter = source::allocate(heap, source::Node { kind: node_kind::Binding }); let setter = source::allocate(heap, source::Node { kind: node_kind::Binding });
    let unlinked = source::allocate(heap, source::Node { kind: node_kind::Binding });
    assert(source::put(heap, root, source::text(heap, "a"), inline) && source::put(heap, root, source::text(heap, "b"), source::number(1)) && source::put(heap, root, source::text(heap, "c"), spilled));
    let g = source::key_named(heap, "g");
    assert(source::define_own(heap, root.id(), g, getter, source::attrs_accessor(), 1) && source::define_own(heap, root.id(), g, setter, source::attrs_accessor(), 2));
    assert(source::set_prototype(heap, root.id(), proto) && !source::node(heap, root.id()).initialized && source::overflow(source::node(heap, root.id())) == 2);
    let c = census::take(heap, [root]);
    assert(c.nodes == 7 && c.dead_nodes == 1 && c.properties == 0);
    // The overflow run is live; no other value row is.
    assert(c.dead_values == c.values - source::overflow(source::node(heap, root.id())));
    let alone = census::take(heap, [unlinked]); assert(alone.dead_nodes == 6);
}
