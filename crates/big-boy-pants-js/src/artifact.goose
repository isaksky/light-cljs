import std;
import os;
import .source_vm;
import .snapshot;
import .program;

namespace artifact;

// Transitive publication invariants adapt 629899ae:src/value/share.rs and
// tests/publication.rs. Storage is copied/relocated; no shared-memory escape
// hatch, sticky classification bit, waiter registry or producer authority.
struct Key { arena: i64, slot: i64 }
struct Artifact {
    schema: i64             = 1, base: i64 = 0, definition: i64 = 0, version: i64 = 1,
    root:   source::Value   = source::Value.Undefined, keys: Key[] = [], nodes: source::Node[] = [],
    values: source::Value[] = [], units: u16[] = [], strings: source::Str[] = [], bytes: u8[] = [], sources: source::SourceRange[] = [],
}
struct Entry { key: Key, target: i64, start: i64, count: i64 }
struct Definition { id: i64, version: i64, root: source::Value, start: i64, count: i64 }
root struct Imports {
    base: i64 = 0, session: i64 = 0, entries: Entry[>..] = [], definitions: Definition[>..] = [], signatures: u8[>..] = [], deliveries: u8[>..] = [],
}
enum Error { None, Context, Version, Link, Mutable, Cycle, Conflict, Bytes, Work, Boundary }
struct Control {
    error: Error    = Error.None, work: i64 = 10000000, bytes: i64 = 67108864, visited: i64 = 0,
    nodes: i64      = 0, edges: i64 = 0, copied_nodes: i64 = 0, copied_bytes: i64 = 0, staged_bytes: i64 = 0, export_ns: i64 = 0, import_ns: i64 = 0,
    path:  i64[>..] = [],
}
fn reject(c: Control&, error: Error) -> bool { if c.error == Error.None { c.error = error; } return false; }
fn poll(c: Control&, n: i64 = 1) -> bool {
    if c.error != Error.None { return false; }
    if n < 0 || n > c.work - c.visited { return reject(c, Error.Work); } c.visited += n; return true;
}
fn payload(a: const Artifact&) -> i64 { return bytes_of(a.keys).len + bytes_of(a.nodes).len + bytes_of(a.values).len + bytes_of(a.units).len + bytes_of(a.strings).len + a.bytes.len + bytes_of(a.sources).len; }
fn footprint(a: const Artifact&) -> i64 { let empty: Artifact[] = [Artifact {}]; return bytes_of(empty).len + payload(a); }
fn context(heap: const source::Heap&, base: const snapshot::Snapshot&, imports: const Imports&, c: Control&) -> bool {
    if heap.live_jobs != 0 || heap.fault != source::Fault.None { return reject(c, Error.Boundary); }
    if base.header.version != 2 || base.header.checkpoint <= 0 || heap.identity <= 0 || heap.nodes.len < base.nodes.len || imports.base != base.header.checkpoint || imports.session != heap.identity { return reject(c, Error.Context); }
    let code .= heap.code;
    if !poll(c, base.code_bytes.len + base.ops.len + base.functions.len + base.code_constants.len + base.children.len + base.code_units.len) { return false; }
    if !code.valid || code.source_bytes != base.code_bytes || bytes_of(code.functions) != bytes_of(base.functions) || source::plain_rows(code.constants) != source::plain_rows(base.code_constants) || code.children != base.children || bytes_of(code.ops) != bytes_of(base.ops) || code.units != base.code_units || code.strings != base.code_strings { return reject(c, Error.Context); }
    return true;
}
fn origin(heap: const source::Heap&, base: const snapshot::Snapshot&, imports: const Imports&, slot: i64, c: Control&) -> Key {
    if slot < base.nodes.len { return Key { arena: base.header.checkpoint, slot: slot }; }
    for e in imports.entries { if !poll(c) { return Key { arena: 0, slot: 0 }; } if e.target == slot { return e.key; } }
    return Key { arena: heap.identity, slot: slot };
}
fn has_values(n: source::Node) -> bool { return n.kind == 2 || n.kind >= 5 && n.kind <= 8 || n.kind == 12; }
fn edge_count(n: source::Node) -> i64 { return 3 + (if has_values(n) { n.count } else { 0 }); }
fn edge<T>(a: const T&, n: source::Node, i: i64) -> source::Value {
    return match i { 0 => n.value, 1 => n.namespace_value, 2 => n.metadata, _ => a.values[n.start + i - 3] };
}
fn valid_value(a: const Artifact&, v: source::Value) -> bool {
    return match v { Empty, Symbol, BigInt, Accessor => false, String x => snapshot::range(x.s, 1, a.strings.len) && snapshot::range(a.strings[x.s].start, a.strings[x.s].len, a.units.len), Object o => snapshot::range(o.o, 1, a.nodes.len), _ => true };
}
fn shape(a: const Artifact&, n: source::Node, base: const snapshot::Snapshot&, c: Control&) -> bool {
    // A closure with own properties (light-cljs-805) is mutable like an object.
    if n.kind == 0 || n.kind == 1 || n.kind == 2 && (n.key_count != 0 || n.initialized) || n.kind == 3 && (!n.immutable || !n.initialized) { return reject(c, Error.Mutable); }
    if n.kind < 2 || n.kind > 12 || n.keys != 0 || n.key_count != 0 || n.key_capacity != 0 || n.shape != 0 || n.proto.tag() != -1 || source::plain_rows(n.slots) != source::plain_rows([source::Value.Undefined, source::Value.Undefined]) { return reject(c, Error.Link); }
    if has_values(n) && !snapshot::range(n.start, n.count, a.values.len) || n.kind == 7 && n.count % 2 != 0 { return reject(c, Error.Link); }
    if n.kind == 2 && (!snapshot::range(n.function, 1, base.functions.len) || n.count != base.functions[n.function].captures) || n.kind == 4 && (n.function > 13 || n.function == 9) { return reject(c, Error.Link); }
    if !valid_value(a, n.value) || !valid_value(a, n.namespace_value) || !valid_value(a, n.metadata) { return reject(c, Error.Link); }
    if n.metadata.tag() > 1 && (n.metadata.tag() != 5 || a.nodes[n.metadata.id()].kind != 7) { return reject(c, Error.Link); }
    if (n.kind == 9 || n.kind == 10) && (n.value.tag() != 4 || n.namespace_value.tag() != 1 && n.namespace_value.tag() != 4) || n.kind == 11 && n.value.tag() > 4 { return reject(c, Error.Link); }
    if n.source != source::no_row() && n.source >= a.sources.len || n.origin < -1 { return reject(c, Error.Link); }
    if n.source != source::no_row() && (n.span_end < n.span_start || !snapshot::range(n.span_start, n.span_end - n.span_start, a.sources[n.source].count)) { return reject(c, Error.Link); }
    if n.source != source::no_row() {
        let r = a.sources[n.source]; let text = a.bytes[r.start..(r.start + r.count)];
        if !program::boundary(text, n.span_start) || !program::boundary(text, n.span_end) { return reject(c, Error.Link); }
    }
    return true;
}
struct Cursor { id: i64, next: i64 = 0 }
// Iterative Tarjan DFS: each node and edge once. The component stack retains
// completed DFS siblings until their entire strongly connected component ends.
// Path stores artifact node IDs from root to the offending node.
fn validate(a: const Artifact&, base: const snapshot::Snapshot&, c: Control&) -> bool {
    let nodes_before = c.nodes;
    if !poll(c) { return false; }
    if a.schema != 1 || a.version <= 0 || a.definition < 0 { return reject(c, Error.Version); }
    if a.base != base.header.checkpoint { return reject(c, Error.Context); }
    if payload(a) > c.bytes || a.nodes.len > 100000 || a.values.len > 1048576 || a.units.len > 1048576 || a.bytes.len > 8388608 { return reject(c, Error.Bytes); }
    if a.keys.len != a.nodes.len || !valid_value(a, a.root) { return reject(c, Error.Link); }
    // Every header is installed, referenced or not.
    if !poll(c, a.strings.len) { return false; }
    for r in a.strings { if !snapshot::range(r.start, r.len, a.units.len) { return reject(c, Error.Link); } }
    for r in a.sources {
        if !snapshot::range(r.start, r.count, a.bytes.len) { return reject(c, Error.Link); }
        if !poll(c, r.count + 1) { return false; }
        if !snapshot::utf8(a.bytes[r.start..(r.start + r.count)]) { return reject(c, Error.Link); }
    }
    // Installation copies the whole flat value pool, including unused ranges.
    // Validate those links too before relocated() can index the mapping table.
    for v in a.values { if !poll(c) { return false; } if !valid_value(a, v) { return reject(c, Error.Link); } }
    // Sorted provenance index rejects duplicate keys without quadratic graph work.
    var order: i64[>..] = []; for k, i in a.keys {
        if !poll(c) { return false; }
        if k.arena <= 0 || k.arena > 2147483647 || k.slot < 0 || k.slot >= 100000 || k.arena == a.base && k.slot >= base.nodes.len { return reject(c, Error.Link); } order.push(i);
    }
    var levels: i64 = 0; var width = order.len; while width > 1 { width = (width + 1) / 2; levels += 1; }
    if !poll(c, order.len * levels) { return false; }
    sort(order) { x, y => a.keys[x].arena < a.keys[y].arena || a.keys[x].arena == a.keys[y].arena && a.keys[x].slot < a.keys[y].slot };
    for i in 1..order.len { let x = a.keys[order[i - 1]]; let y = a.keys[order[i]]; if x.arena == y.arena && x.slot == y.slot { return reject(c, Error.Link); } }
    var color:     i64[>..]  = []; color.resize(a.nodes.len, 0);
    var index:     i64[>..]  = []; index.resize(a.nodes.len, -1); var low: i64[>..] = []; low.resize(a.nodes.len, -1);
    var parent:    i64[>..]  = []; parent.resize(a.nodes.len, -1);
    var component: i64[>..<] = []; var stack: Cursor[>..<] = [];
    if a.root.tag() == 5 { stack.push(Cursor { id: a.root.id() }); }
    while stack.len > 0 {
        let id = stack[stack.len - 1].id;
        if color[id] == 0 {
            c.path.push(id); if !poll(c) || !shape(a, a.nodes[id], base, c) { return false; }
            color[id] = 1; index[id] = c.nodes - nodes_before; low[id] = index[id]; c.nodes += 1; component.push(id);
        }
        let n = a.nodes[id]; let i = stack[stack.len - 1].next;
        if i == edge_count(n) {
            if low[id] == index[id] {
                var count: i64 = 0; var forbidden: i64 = -1;
                loop {
                    if !poll(c) { return false; }
                    let member = component[component.len - 1]; component.pop(); color[member] = 2; count += 1;
                    if a.nodes[member].kind != 2 && a.nodes[member].kind != 3 { forbidden = member; }
                    if member == id { break; }
                }
                if count > 1 && forbidden >= 0 {
                    // Recover the root-to-form DFS path even when the form's
                    // frame finished before the SCC's closing cross edge.
                    c.path.clear(); var reversed: i64[>..] = []; var at = forbidden;
                    while at >= 0 { if !poll(c) { return false; } reversed.push(at); at = parent[at]; }
                    for j in reversed.len { c.path.push(reversed[reversed.len - j - 1]); }
                    return reject(c, Error.Cycle);
                }
            }
            stack.pop(); c.path.pop();
            if stack.len > 0 { let previous = stack[stack.len - 1].id; low[previous] = min(low[previous], low[id]); }
            continue;
        }
        stack[stack.len - 1].next += 1;
        if !poll(c) { return false; } c.edges += 1;
        let v = edge(a, n, i); if !valid_value(a, v) { return reject(c, Error.Link); }
        if v.tag() != 5 || color[v.id()] == 2 { continue; }
        if color[v.id()] == 1 {
            if v.id() == id && n.kind != 2 && n.kind != 3 { c.path.push(id); return reject(c, Error.Cycle); }
            low[id] = min(low[id], index[v.id()]);
        } else { parent[v.id()] = id; stack.push(Cursor { id: v.id() }); }
    }
    if c.nodes - nodes_before != a.nodes.len { return reject(c, Error.Link); } return true;
}
fn export_value(heap: const source::Heap&, v: source::Value, ids: i64[>..]&, slots: i64[>..]&, units: u16[>..]&, strings: source::Str[>..]&, c: Control&) -> source::Value {
    if v.tag() < 0 || v.tag() > 5 { reject(c, Error.Link); return source::Value.Undefined; }
    if v.tag() == 5 {
        let id = v.id();
        if !snapshot::range(id, 1, heap.nodes.len) { reject(c, Error.Link); return source::Value.Undefined; }
        if ids[id] < 0 { ids[id] = slots.len; slots.push(id); } return source::object(ids[id]);
    } else if v.tag() == 4 {
        if !snapshot::range(v.sid(), 1, heap.strings.len) || !poll(c, source::str_len(heap, v)) { reject(c, Error.Link); return source::Value.Undefined; }
        strings.push(source::Str { start: units.len as u32, len: source::str_len(heap, v) as u32 }); source::append_utf16(heap, v, units);
        return source::Value.String { s: (strings.len - 1) as u32 };
    }
    return v;
}
fn export_graph(heap: const source::Heap&, base: const snapshot::Snapshot&, imports: const Imports&, root: source::Value, definition: i64, version: i64, c: Control&) -> Artifact {
    let started = clock_ns(); defer { c.export_ns += clock_ns() - started; }
    if !context(heap, base, imports, c) || !poll(c, heap.nodes.len) { return Artifact {}; }
    if heap.nodes.len > 100000 || heap.source_ranges.len > 1048576 { reject(c, Error.Bytes); return Artifact {}; }
    var ids: i64[>..] = []; ids.resize(heap.nodes.len, -1); var slots: i64[>..] = [];
    if !poll(c, heap.source_ranges.len) { return Artifact {}; }
    var source_ids: i64[>..]          = []; source_ids.resize(heap.source_ranges.len, -1);
    var nodes:      source::Node[>..] = []; var keys: Key[>..] = []; var values: source::Value[>..] = []; var units: u16[>..] = []; var strings: source::Str[>..] = []; var bytes: u8[>..] = []; var sources: source::SourceRange[>..] = [];
    let result = export_value(heap, root, ids, slots, units, strings, c); var cursor: i64 = 0;
    while cursor < slots.len && c.error == Error.None {
        if !poll(c) { break; } let slot = slots[cursor]; var n = source::node(heap, slot);
        // Never read private property graphs; shape() rejects their owners.
        keys.push(origin(heap, base, imports, slot, c));
        n.value = export_value(heap, n.value, ids, slots, units, strings, c); n.namespace_value = export_value(heap, n.namespace_value, ids, slots, units, strings, c); n.metadata = export_value(heap, n.metadata, ids, slots, units, strings, c);
        if has_values(n) {
            if !snapshot::range(n.start, n.count, heap.values.len) || !poll(c, n.count) { reject(c, Error.Link); break; }
            let old = n.start; n.start = values.len as u32;
            for i in n.count { let v = export_value(heap, source::slot(heap, old + i), ids, slots, units, strings, c); values.push(v); }
        }
        if n.source != source::no_row() {
            if !snapshot::range(n.source, 1, heap.source_ranges.len) { reject(c, Error.Link); break; }
            let original_source = n.source;
            if source_ids[original_source] < 0 {
                let r = heap.source_ranges[original_source];
                if !snapshot::range(r.start, r.count, heap.source_bytes.len) || !poll(c, r.count) { reject(c, Error.Link); break; }
                source_ids[original_source] = sources.len;
                sources.push(source::SourceRange { start: bytes.len as u32, count: r.count }); bytes.append(heap.source_bytes[r.start..(r.start + r.count)]);
            }
            n.source = source_ids[original_source] as u32;
        }
        nodes.push(n); cursor += 1;
        if bytes_of(nodes).len + bytes_of(keys).len + bytes_of(values).len + bytes_of(units).len + bytes.len + bytes_of(sources).len > c.bytes { reject(c, Error.Bytes); }
    }
    if c.error != Error.None { return Artifact {}; }
    var a = Artifact { base: base.header.checkpoint, definition: definition, version: version, root: result, keys: keys[..], nodes: nodes[..], values: values[..], units: units[..], strings: strings[..], bytes: bytes[..], sources: sources[..] };
    // Validation owns its visitation counters; exported graphs are independently checked on import.
    if !validate(a, base, c) {
        // No artifact escapes on failure. Report IDs in the caller's source
        // heap so its offending path is inspectable after staging disappears.
        for &id in c.path { id = slots[id]; }
        return Artifact {};
    }
    c.copied_bytes += 2 * payload(a); return a;
}
// Exact canonical per-node bytes use provenance targets, independent of local
// artifact order/pool offsets. Never hash equal occurrences into one identity.
fn signature_bytes<T>(values: const T[:], out: u8[>..]&, c: Control&) -> bool {
    // to_bytes writes a ULEB payload-byte-count followed by bytes_of(values).
    // Charge both before serializing: shared strings can expand a compact
    // artifact into many copies of the same range in its exact signature.
    let payload = bytes_of(values).len; var size = payload + 1; var remaining = payload;
    while remaining >= 128 { remaining /= 128; size += 1; }
    if size > c.bytes - out.len { return reject(c, Error.Bytes); }
    if !poll(c, size) { return false; }
    to_bytes(values, out); c.staged_bytes += size; return true;
}
// Canonical plain bytes for a value and a node's scalar fields: an enum's
// unused payload bytes are not part of its value, so values are never hashed
// with bytes_of directly.
struct ValueSig { tag: i64, number: f64 = 0, a: i64 = 0, b: i64 = 0 }
struct NodeSig { kind: i64, function: i64, count: i64, keys: i64, key_count: i64, key_capacity: i64, length: i64, initialized: bool, immutable: bool, origin: i64, source: i64, span_start: i64, span_end: i64 }
fn node_sig(n: source::Node) -> NodeSig { return NodeSig { kind: n.kind, function: n.function, count: n.count, keys: n.keys, key_count: n.key_count, key_capacity: n.key_capacity, length: n.length, initialized: n.initialized, immutable: n.immutable, origin: n.origin, source: if n.source == source::no_row() { -1 } else { 0 }, span_start: n.span_start, span_end: n.span_end }; }
fn scalar_sig(v: source::Value, length: i64) -> ValueSig { return ValueSig { tag: v.tag(), number: v.num(), a: if v.flag() { 1 } else { 0 }, b: length }; }
fn signature_value(a: const Artifact&, v: source::Value, out: u8[>..]&, c: Control&) -> bool {
    if v.tag() == 5 { return signature_bytes([ValueSig { tag: 5, a: a.keys[v.id()].slot, b: a.keys[v.id()].arena }], out, c); }
    if v.tag() != 4 { return signature_bytes([scalar_sig(v, 0)], out, c); }
    let r = a.strings[v.sid()];
    return signature_bytes([scalar_sig(v, r.len)], out, c) && signature_bytes(a.units[r.start..(r.start + r.len)], out, c);
}
fn signature(a: const Artifact&, id: i64, out: u8[>..]&, c: Control&) -> bool {
    let original = a.nodes[id];
    if !signature_bytes([node_sig(original)], out, c) { return false; }
    for i in edge_count(original) { if !poll(c) || !signature_value(a, edge(a, original, i), out, c) { return false; } }
    if original.source != source::no_row() { let r = a.sources[original.source]; if !signature_bytes(a.bytes[r.start..(r.start + r.count)], out, c) { return false; } }
    return true;
}
fn relocated(v: source::Value, mapping: const i64[:], string_start: i64) -> source::Value {
    return match v { Object o => source::object(mapping[o.o]), String x => source::Value.String { s: (x.s + string_start) as u32 }, _ => v };
}
// The base map is implicit and checked against both the frozen snapshot and
// the destination. This examines one record's edges, never rewalks its DAG.
fn base_signature<T>(s: const T&, slot: i64, checkpoint: i64, out: u8[>..]&, c: Control&) -> bool {
    let original = s.nodes[slot];
    if !signature_bytes([node_sig(original)], out, c) { return false; }
    if has_values(original) && !snapshot::range(original.start, original.count, s.values.len) { return reject(c, Error.Link); }
    for i in edge_count(original) {
        if !poll(c) { return false; } let v = edge(s, original, i);
        if v.tag() == 5 { if !signature_bytes([ValueSig { tag: 5, a: v.id(), b: checkpoint }], out, c) { return false; } }
        else if v.tag() == 4 {
            if !snapshot::range(v.sid(), 1, s.strings.len) { return reject(c, Error.Link); }
            let r = s.strings[v.sid()];
            // Signatures carry UTF-16 units whatever the run's table (light-cljs-800).
            if !snapshot::range(r.start, r.len, if r.wide { s.units.len } else { s.latin1.len }) { return reject(c, Error.Link); }
            if !signature_bytes([scalar_sig(v, r.len)], out, c) { return false; }
            if r.wide { if !signature_bytes(s.units[r.start..(r.start + r.len)], out, c) { return false; } }
            else { var wide: u16[>..] = []; for b in s.latin1[r.start..(r.start + r.len)] { wide.push(b); } if !signature_bytes(wide, out, c) { return false; } }
        } else if !signature_bytes([scalar_sig(v, 0)], out, c) { return false; }
    }
    if original.source != source::no_row() {
        if !snapshot::range(original.source, 1, s.sources.len) { return reject(c, Error.Link); } let r = s.sources[original.source];
        if !snapshot::range(r.start, r.count, s.bytes.len) { return reject(c, Error.Link); }
        if !signature_bytes(s.bytes[r.start..(r.start + r.count)], out, c) { return false; }
    }
    return true;
}
fn import_graph(a: const Artifact&, base: const snapshot::Snapshot&, heap: source::Heap&, imports: Imports&, c: Control&) -> source::Value {
    let started = clock_ns(); defer { c.import_ns += clock_ns() - started; }
    if !context(heap, base, imports, c) || !validate(a, base, c) { return source::Value.Undefined; }
    var delivery: u8[>..] = []; to_bytes([a], delivery);
    c.staged_bytes += footprint(a) + delivery.len;
    if delivery.len > c.bytes || !poll(c, delivery.len) { reject(c, Error.Bytes); return source::Value.Undefined; }
    for d in imports.definitions {
        if !poll(c) { return source::Value.Undefined; }
        if d.id == a.definition {
            if d.version != a.version || delivery != imports.deliveries[d.start..(d.start + d.count)] { reject(c, Error.Conflict); return source::Value.Undefined; } return d.root;
        }
    }
    var mapping: i64[>..] = []; var added: Entry[>..] = []; var signatures: u8[>..] = [];
    let empty: snapshot::Binding[] = []; let destination = snapshot::borrow(heap, empty, base.header.checkpoint);
    var new_nodes: i64 = 0;
    for k, i in a.keys {
        if !poll(c) { return source::Value.Undefined; }
        let start = signatures.len; if !signature(a, i, signatures, c) { return source::Value.Undefined; } var target: i64 = -1;
        if k.arena == a.base {
            var expected_bytes: u8[>..] = []; var actual_bytes: u8[>..] = [];
            if !base_signature(base, k.slot, a.base, expected_bytes, c) || !base_signature(destination, k.slot, a.base, actual_bytes, c) { return source::Value.Undefined; }
            if expected_bytes != signatures[start..] || actual_bytes != expected_bytes { reject(c, Error.Conflict); return source::Value.Undefined; } target = k.slot;
        } else {
            for e in imports.entries {
                if !poll(c) { return source::Value.Undefined; }
                if e.key.arena == k.arena && e.key.slot == k.slot {
                    if signatures[start..] != imports.signatures[e.start..(e.start + e.count)] { reject(c, Error.Conflict); return source::Value.Undefined; } target = e.target; break;
                }
            }
        }
        if target < 0 { target = heap.nodes.len + new_nodes; new_nodes += 1; added.push(Entry { key: k, target: target, start: imports.signatures.len + start, count: signatures.len - start }); }
        mapping.push(target);
    }
    // Each string takes its own canonical run (Latin-1 or UTF-16).
    var text: i64 = 0; for r in a.strings { text += r.len; }
    if !source::nodes_fit(heap, new_nodes) || !source::values_fit(heap, a.values.len) || !source::units_fit(heap, text) || a.strings.len > heap.limits.units - (heap.strings.len - heap.image.strings) || a.bytes.len > 8388608 - heap.source_bytes.len || a.sources.len > 1048576 - heap.source_ranges.len { reject(c, Error.Bytes); return source::Value.Undefined; }
    if imports.entries.len + added.len > 100000 || imports.definitions.len >= 100000 || delivery.len > c.bytes - imports.deliveries.len || signatures.len > c.bytes - imports.signatures.len { reject(c, Error.Bytes); return source::Value.Undefined; }
    // Every string header copies its own run, shared units included.
    if !poll(c, a.values.len + max(a.units.len, text) + a.bytes.len + a.sources.len + a.nodes.len) { return source::Value.Undefined; }
    let string_start = heap.strings.len; let value_start = heap.values.len; let source_start = heap.source_ranges.len;
    // Commit: all validation, comparison, budgets and link resolution finished;
    // the preflight above reserved every row, so no allocation below fails.
    for r in a.strings { source::import_text(heap, a.units[r.start..(r.start + r.len)]); }
    var moved: source::Value[>..] = []; for v in a.values { moved.push(relocated(v, mapping, string_start)); } source::add_values(heap, moved);
    let byte_start = source::add_source_bytes(heap, a.bytes); for r in a.sources { source::add_source_range(heap, source::SourceRange { start: r.start + byte_start as u32, count: r.count }); }
    for n, i in a.nodes {
        if mapping[i] < heap.nodes.len { continue; } assert(mapping[i] == heap.nodes.len);
        var out = n; out.value = relocated(n.value, mapping, string_start); out.namespace_value = relocated(n.namespace_value, mapping, string_start); out.metadata = relocated(n.metadata, mapping, string_start);
        if has_values(n) { out.start += value_start as u32; } if n.source != source::no_row() { out.source += source_start as u32; } source::add_nodes(heap, [out]);
    }
    imports.signatures.append(signatures); imports.entries.append(added);
    let root = relocated(a.root, mapping, string_start); let delivery_start = imports.deliveries.len; imports.deliveries.append(delivery);
    imports.definitions.push(Definition { id: a.definition, version: a.version, root: root, start: delivery_start, count: delivery.len });
    // Count actual installed pool/cache records. Existing aliased nodes are
    // excluded; unused appended value/text ranges remain visible in the cost.
    c.copied_nodes += new_nodes;
    c.copied_bytes += new_nodes * bytes_of([source::Node { kind: 0 }]).len + bytes_of(a.values).len + bytes_of(a.units).len + a.bytes.len + bytes_of(a.sources).len + signatures.len + bytes_of(added).len + delivery.len + bytes_of([Definition { id: 0, version: 1, root: root, start: 0, count: 0 }]).len;
    return root;
}
// Local checked Goose ABI codec, versioned semantically before publication.
fn encode(a: const Artifact&, base: const snapshot::Snapshot&, out: u8[>..]&, c: Control&) -> bool {
    if !validate(a, base, c) { return false; } var bytes: u8[>..] = []; to_bytes([a], bytes);
    if bytes.len > c.bytes || !poll(c, bytes.len) { return reject(c, Error.Bytes); } out.append(bytes); c.copied_bytes += payload(a) + 2 * bytes.len; return true;
}
fn decode(bytes: const u8[:], base: const snapshot::Snapshot&, c: Control&) -> Artifact[>..] {
    if bytes.len > c.bytes || !poll(c, bytes.len) { reject(c, Error.Bytes); return []; }
    var records, ok = from_bytes<Artifact[>..]>(bytes); if !ok || records.len != 1 { reject(c, Error.Link); return []; }
    for a in records { if !validate(a, base, c) { return []; } }
    c.copied_bytes += 2 * bytes_of(records).len; return copy(records);
}
