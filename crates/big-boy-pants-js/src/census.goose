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
fn mark(v: source::Value, nodes: bool[>..]&, units: bool[>..]&, work: i64[>..<]&) {
    if v.tag == 5 && v.a >= 0 && v.a < nodes.len && !nodes[v.a] { nodes[v.a] = true; work.push(v.a); }
    if v.tag == 4 && v.a >= 0 && v.b > 0 && v.b <= units.len - v.a { for i in v.a..(v.a + v.b) { units[i] = true; } }
}
fn take(heap: const source::Heap&, roots: const source::Value[:]) -> Census {
    var nodes:      bool[>..] = []; nodes.resize(heap.nodes.len, false);
    var values:     bool[>..] = []; values.resize(heap.values.len, false);
    var properties: bool[>..] = []; properties.resize(heap.properties.len, false);
    var units:      bool[>..] = []; units.resize(heap.units.len, false);
    var work:       i64[>..<] = [];
    for v in roots { mark(v, nodes, units, work); }
    for v in heap.constants { mark(v, nodes, units, work); }
    mark(heap.global, nodes, units, work); mark(heap.call_method, nodes, units, work);
    while work.len > 0 {
        let n = heap.nodes[work.pop()];
        mark(n.value, nodes, units, work); mark(n.namespace_value, nodes, units, work); mark(n.metadata, nodes, units, work);
        if snapshot::has_values(n.kind) {
            for slot in n.start..(n.start + n.count) { if slot >= 0 && slot < values.len { values[slot] = true; mark(heap.values[slot], nodes, units, work); } }
        }
        var p = n.head;
        while p >= 0 && p < properties.len && !properties[p] {
            properties[p] = true; let record = heap.properties[p];
            mark(record.key, nodes, units, work); mark(record.value, nodes, units, work); p = record.next;
        }
    }
    var c = Census { nodes: heap.nodes.len, values: heap.values.len, properties: heap.properties.len, units: heap.units.len };
    for live in nodes { if !live { c.dead_nodes += 1; } }
    for live in values { if !live { c.dead_values += 1; } }
    for live in properties { if !live { c.dead_properties += 1; } }
    for live in units { if !live { c.dead_units += 1; } }
    return c;
}
