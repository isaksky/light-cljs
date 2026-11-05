import std;
import .source_vm;
import .snapshot;

namespace census;

// Diagnostic only (light-cljs-784): a job heap's table rows at the job's end
// and how many of them no root reaches. It frees and rewrites nothing; a job's
// tables are reclaimed when its worker exits. The walk follows every link a
// snapshot validates (node values, slot runs, property chains, string ranges).
// A property record shadowed by a later put on the same chain is still linked
// from it and counts as reachable, so the dead counts are lower bounds.
struct Census {
    nodes:      i64 = 0, values: i64 = 0, properties: i64 = 0, units: i64 = 0,
    dead_nodes: i64 = 0, dead_values: i64 = 0, dead_properties: i64 = 0, dead_units: i64 = 0,
}
fn mark(heap: const source::Heap&, v: source::Value, nodes: bool[>..]&, units: bool[>..]&, work: i64[>..<]&) {
    if v.tag() == 5 && v.id() < nodes.len && !nodes[v.id()] { nodes[v.id()] = true; work.push(v.id()); }
    if v.tag() == 4 && v.sid() < heap.strings.len { let r = heap.strings[v.sid()]; if r.len > 0 && r.len <= units.len - r.start { for i in r.start..(r.start + r.len) { units[i] = true; } } }
}
fn take(heap: const source::Heap&, roots: const source::Value[:]) -> Census {
    var nodes:      bool[>..] = []; nodes.resize(heap.nodes.len, false);
    var values:     bool[>..] = []; values.resize(heap.values.len, false);
    var properties: bool[>..] = []; properties.resize(heap.properties.len, false);
    var units:      bool[>..] = []; units.resize(heap.units.len, false);
    var work:       i64[>..<] = [];
    for v in roots { mark(heap, v, nodes, units, work); }
    for v in heap.constants { mark(heap, v, nodes, units, work); }
    mark(heap, heap.global, nodes, units, work); mark(heap, heap.call_method, nodes, units, work);
    while work.len > 0 {
        let n = source::node(heap, work.pop());
        mark(heap, n.value, nodes, units, work); mark(heap, n.namespace_value, nodes, units, work); mark(heap, n.metadata, nodes, units, work);
        if snapshot::has_values(n.kind) {
            for row in n.start..(n.start + n.count) { if row < values.len { values[row] = true; mark(heap, source::slot(heap, row), nodes, units, work); } }
        }
        for p in n.keys..min((n.keys + n.key_count) as i64, properties.len) {
            properties[p] = true; let record = source::property_row(heap, p);
            mark(heap, record.key, nodes, units, work); mark(heap, record.value, nodes, units, work);
        }
    }
    var c = Census { nodes: heap.nodes.len, values: heap.values.len, properties: heap.properties.len, units: heap.units.len };
    for live in nodes { if !live { c.dead_nodes += 1; } }
    for live in values { if !live { c.dead_values += 1; } }
    for live in properties { if !live { c.dead_properties += 1; } }
    for live in units { if !live { c.dead_units += 1; } }
    return c;
}
