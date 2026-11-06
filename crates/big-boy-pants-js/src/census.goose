import std;
import .source_vm;
import .snapshot;

namespace census;

// Diagnostic only (light-cljs-784): a job heap's table rows at the job's end
// and how many of them no root reaches. It frees and rewrites nothing; a job's
// tables are reclaimed when its worker exits. The walk follows every link a
// snapshot validates (node values, slot runs, property chains, string ranges
// in both unit tables).
// A property record shadowed by a later put on the same chain is still linked
// from it and counts as reachable, so the dead counts are lower bounds.
struct Census {
    nodes:      i64 = 0, values: i64 = 0, properties: i64 = 0, units: i64 = 0,
    dead_nodes: i64 = 0, dead_values: i64 = 0, dead_properties: i64 = 0, dead_units: i64 = 0,
}
fn mark(heap: const source::Heap&, v: source::Value, nodes: bool[>..]&, units: bool[>..]&, work: i64[>..<]&) {
    if v.tag() == 5 && v.id() < nodes.len && !nodes[v.id()] { nodes[v.id()] = true; work.push(v.id()); }
    // Unit rows: the UTF-16 table, then the Latin-1 table.
    if v.tag() == 4 && v.sid() < heap.strings.len {
        let r = heap.strings[v.sid()]; let base = if r.wide { 0 } else { heap.units.len }; let length = if r.wide { heap.units.len } else { heap.latin1.len };
        if r.len > 0 && r.len <= length - r.start { for i in r.start..(r.start + r.len) { units[base + i] = true; } }
    }
}
// A property's value: an accessor marks its pair's functions.
fn mark_slot(heap: const source::Heap&, v: source::Value, nodes: bool[>..]&, units: bool[>..]&, work: i64[>..<]&) {
    if v.tag() == -2 { let p = source::pair(heap, v); mark(heap, p.get, nodes, units, work); mark(heap, p.set, nodes, units, work); } else { mark(heap, v, nodes, units, work); }
}
fn mark_activation(heap: const source::Heap&, row: u32, values: bool[>..]&, nodes: bool[>..]&, units: bool[>..]&, work: i64[>..<]&) {
    let a = source::activation(heap, row);
    for i in a.frame_count { let f = source::saved_frame(heap, a.frames + i); mark(heap, f.callee, nodes, units, work); mark(heap, f.receiver, nodes, units, work); mark(heap, f.input, nodes, units, work); mark(heap, f.output, nodes, units, work); }
    let first: i64 = a.values; for r in first..min(first + a.capacity, values.len) { values[r] = true; mark(heap, source::slot(heap, r), nodes, units, work); }
    for i in a.handler_count { match source::cont_row(heap, a.handlers + i) { Saved s => { mark(heap, s.h.value, nodes, units, work); }, _ => {} } }
}
fn take(heap: const source::Heap&, roots: const source::Value[:]) -> Census {
    var nodes:      bool[>..] = []; nodes.resize(heap.nodes.len, false);
    var values:     bool[>..] = []; values.resize(heap.values.len, false);
    var properties: bool[>..] = []; properties.resize(heap.properties.len, false);
    var units:      bool[>..] = []; units.resize(heap.units.len + heap.latin1.len, false);
    var work:       i64[>..<] = [];
    for v in roots { mark(heap, v, nodes, units, work); }
    for v in heap.constants { mark(heap, v, nodes, units, work); }
    mark(heap, heap.global, nodes, units, work); mark(heap, heap.call_method, nodes, units, work);
    while work.len > 0 {
        let id = work.pop(); let n = source::node(heap, id);
        mark(heap, n.value, nodes, units, work); mark(heap, n.namespace_value, nodes, units, work); mark(heap, n.metadata, nodes, units, work);
        if snapshot::has_values(n.kind) {
            for row in n.start..(n.start + n.count) { if row < values.len { values[row] = true; mark(heap, source::slot(heap, row), nodes, units, work); } }
        }
        // Own properties (light-cljs-787): a shape-mode object's slots and
        // overflow run, a dictionary's records, accessors' functions, and
        // the prototype. Every shape's key is live. A key list's records
        // (kind 16) are a dictionary's; element runs are value runs.
        mark(heap, n.proto, nodes, units, work);
        // A generator's activation or a promise's state (light-cljs-790):
        // the activation's slot run and handlers, the promise's result,
        // reactions' promises and an async function's activation.
        if n.kind == source::generator_kind() && n.function != source::no_row() { mark_activation(heap, n.function, values, nodes, units, work); }
        if n.kind == source::promise_kind() {
            let s = source::promise_state(heap, id);
            mark(heap, s.result, nodes, units, work);
            if s.activation != source::no_row() { mark_activation(heap, s.activation, values, nodes, units, work); }
            var at = s.first; while at != source::no_row() { let r = source::reaction(heap, at); mark(heap, source::object(r.target), nodes, units, work); at = r.next; }
        }
        if snapshot::shaped(n) {
            for v, i in n.slots { if i < n.key_count { mark_slot(heap, v, nodes, units, work); } }
            for row in n.keys..min(n.keys + source::overflow(n), values.len) { values[row] = true; mark_slot(heap, source::slot(heap, row), nodes, units, work); }
        } else if snapshot::record_run(n) {
            for p in n.keys..min((n.keys + n.key_count) as i64, properties.len) {
                properties[p] = true; let record = source::property_row(heap, p);
                mark(heap, record.key, nodes, units, work); mark_slot(heap, record.value, nodes, units, work);
            }
        }
    }
    for s in heap.shapes { mark(heap, s.key, nodes, units, work); }
    var c = Census { nodes: heap.nodes.len, values: heap.values.len, properties: heap.properties.len, units: heap.units.len + heap.latin1.len };
    for live in nodes { if !live { c.dead_nodes += 1; } }
    for live in values { if !live { c.dead_values += 1; } }
    for live in properties { if !live { c.dead_properties += 1; } }
    for live in units { if !live { c.dead_units += 1; } }
    return c;
}
