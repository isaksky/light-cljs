import std;
import .source_vm;
import .census;

// census::take on a hand-built heap with known reachability: a cycle through
// a closure's slot run and a cell, a property chain, overlapping string
// ranges, an unowned slot, and two disconnected objects.
fn node(id: i64) -> source::Value { return source::object(id); }
fn text(heap: source::Heap&, start: i64, count: i64) -> source::Value { return source::string_ref(heap, start, count); }
fn main() {
    var code = source::Code {}; var heap = source::Heap { code: code };
    for u in "abcdef" { heap.units.push(u); }
    // 0: plain object, property 1 -> closure 1.
    heap.nodes.push(source::Node { kind: 0, keys: 1, key_count: 1, key_capacity: 1 });
    // 1: closure whose slots 0..2 hold the cell and a string.
    heap.nodes.push(source::Node { kind: 2, start: 0, count: 2 });
    // 2: cell pointing back at the closure (a cycle).
    heap.nodes.push(source::Node { kind: 3, value: node(1) });
    // 3: disconnected object owning property 0; 4: disconnected, empty.
    heap.nodes.push(source::Node { kind: 0, keys: 0, key_count: 1, key_capacity: 1 });
    heap.nodes.push(source::Node { kind: 0 });
    heap.values.push(node(2)); heap.values.push(text(heap, 0, 3)); heap.values.push(source::number(7));
    heap.properties.push(source::Property { key: text(heap, 3, 2), value: source::number(1) });
    heap.properties.push(source::Property { key: text(heap, 0, 1), value: node(1) });
    let c = census::take(heap, [node(0)]);
    assert(c.nodes == 5 && c.values == 3 && c.properties == 2 && c.units == 6);
    // Dead: nodes 3 and 4, slot 2, property 0, units 3..6 ("def").
    assert(c.dead_nodes == 2 && c.dead_values == 1 && c.dead_properties == 1 && c.dead_units == 3);
    // A root inside the cycle reaches the same rows except object 0 and its property.
    let inner = census::take(heap, [node(2)]);
    assert(inner.dead_nodes == 3 && inner.dead_values == 1 && inner.dead_properties == 2 && inner.dead_units == 3);
    // No roots: everything is dead.
    let none: source::Value[] = [];
    let empty = census::take(heap, none);
    assert(empty.dead_nodes == 5 && empty.dead_values == 3 && empty.dead_properties == 2 && empty.dead_units == 6);
    print("census tests passed");
}
