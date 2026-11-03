import std;
import os;
import .source_vm;
import .snapshot;

namespace artifact;

// Transitive publication invariants adapt 629899ae:src/value/share.rs and
// tests/publication.rs. Storage is copied/relocated; no shared-memory escape
// hatch, sticky classification bit, waiter registry or producer authority.
struct Key { arena: i64, slot: i64 }
struct Artifact {
    schema: i64             = 1, base: i64 = 0, definition: i64 = 0, version: i64 = 1,
    root:   source::Value   = source::Value {}, keys: Key[] = [], nodes: source::Node[] = [],
    values: source::Value[] = [], units: u16[] = [], bytes: u8[] = [], sources: source::SourceRange[] = [],
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
fn payload(a: const Artifact&) -> i64 { return bytes_of(a.keys).len + bytes_of(a.nodes).len + bytes_of(a.values).len + bytes_of(a.units).len + a.bytes.len + bytes_of(a.sources).len; }
fn footprint(a: const Artifact&) -> i64 { let empty: Artifact[] = [Artifact {}]; return bytes_of(empty).len + payload(a); }
fn context(heap: const source::Heap&, base: const snapshot::Snapshot&, imports: const Imports&, c: Control&) -> bool {
    if heap.live_jobs != 0 || heap.fault != source::Fault.None { return reject(c, Error.Boundary); }
    if base.header.version != 1 || base.header.checkpoint <= 0 || heap.identity <= 0 || heap.nodes.len < base.nodes.len || imports.base != base.header.checkpoint || imports.session != heap.identity { return reject(c, Error.Context); }
    let code .= heap.code;
    if !poll(c, base.code_bytes.len + base.ops.len + base.functions.len + base.code_constants.len + base.children.len + base.code_units.len) { return false; }
    if !code.valid || code.source_bytes != base.code_bytes || bytes_of(code.functions) != bytes_of(base.functions) || bytes_of(code.constants) != bytes_of(base.code_constants) || code.children != base.children || bytes_of(code.ops) != bytes_of(base.ops) || code.units != base.code_units { return reject(c, Error.Context); }
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
    return v.tag >= 0 && v.tag <= 5 && v.session == 0 && (v.tag != 2 || v.a == 0 || v.a == 1) && (v.tag != 4 || snapshot::range(v.a, v.b, a.units.len)) && (v.tag != 5 || snapshot::range(v.a, 1, a.nodes.len));
}
fn shape(a: const Artifact&, n: source::Node, base: const snapshot::Snapshot&, c: Control&) -> bool {
    if n.kind == 0 || n.kind == 1 || n.kind == 3 && (!n.immutable || !n.initialized) { return reject(c, Error.Mutable); }
    if n.kind < 2 || n.kind > 12 || n.head != -1 || n.length < 0 { return reject(c, Error.Link); }
    if has_values(n) && !snapshot::range(n.start, n.count, a.values.len) || n.kind == 7 && n.count % 2 != 0 { return reject(c, Error.Link); }
    if n.kind == 2 && (!snapshot::range(n.function, 1, base.functions.len) || n.count != base.functions[n.function].captures) || n.kind == 4 && (n.function < 0 || n.function > 8) { return reject(c, Error.Link); }
    if !valid_value(a, n.value) || !valid_value(a, n.namespace_value) || !valid_value(a, n.metadata) { return reject(c, Error.Link); }
    if n.metadata.tag > 1 && (n.metadata.tag != 5 || a.nodes[n.metadata.a].kind != 7) { return reject(c, Error.Link); }
    if (n.kind == 9 || n.kind == 10) && (n.value.tag != 4 || n.namespace_value.tag != 1 && n.namespace_value.tag != 4) || n.kind == 11 && n.value.tag > 4 { return reject(c, Error.Link); }
    if n.source < -1 || n.source >= a.sources.len || n.origin < -1 { return reject(c, Error.Link); }
    if n.source >= 0 && (n.span_start < 0 || n.span_end < n.span_start || !snapshot::range(n.span_start, n.span_end - n.span_start, a.sources[n.source].count)) { return reject(c, Error.Link); }
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
    if a.root.tag == 5 { stack.push(Cursor { id: a.root.a }); }
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
        if v.tag != 5 || color[v.a] == 2 { continue; }
        if color[v.a] == 1 {
            if v.a == id && n.kind != 2 && n.kind != 3 { c.path.push(id); return reject(c, Error.Cycle); }
            low[id] = min(low[id], index[v.a]);
        } else { parent[v.a] = id; stack.push(Cursor { id: v.a }); }
    }
    if c.nodes - nodes_before != a.nodes.len { return reject(c, Error.Link); } return true;
}
fn export_value(heap: const source::Heap&, v: source::Value, ids: i64[>..]&, slots: i64[>..]&, units: u16[>..]&, c: Control&) -> source::Value {
    var out = v;
    if v.tag < 0 || v.tag > 5 || v.tag < 4 && v.session != 0 || v.tag >= 4 && v.session != heap.identity { reject(c, Error.Link); return source::Value {}; }
    if v.tag == 5 {
        if !snapshot::range(v.a, 1, heap.nodes.len) { reject(c, Error.Link); return source::Value {}; }
        if ids[v.a] < 0 { ids[v.a] = slots.len; slots.push(v.a); } out.a = ids[v.a]; out.session = 0;
    } else if v.tag == 4 {
        if !snapshot::range(v.a, v.b, heap.units.len) || !poll(c, v.b) { reject(c, Error.Link); return source::Value {}; }
        out.a = units.len; out.session = 0; units.append(heap.units[v.a..(v.a + v.b)]);
    }
    return out;
}
fn export_graph(heap: const source::Heap&, base: const snapshot::Snapshot&, imports: const Imports&, root: source::Value, definition: i64, version: i64, c: Control&) -> Artifact {
    let started = clock_ns(); defer { c.export_ns += clock_ns() - started; }
    if !context(heap, base, imports, c) || !poll(c, heap.nodes.len) { return Artifact {}; }
    if heap.nodes.len > 100000 || heap.source_ranges.len > 1048576 { reject(c, Error.Bytes); return Artifact {}; }
    var ids: i64[>..] = []; ids.resize(heap.nodes.len, -1); var slots: i64[>..] = [];
    if !poll(c, heap.source_ranges.len) { return Artifact {}; }
    var source_ids: i64[>..]          = []; source_ids.resize(heap.source_ranges.len, -1);
    var nodes:      source::Node[>..] = []; var keys: Key[>..] = []; var values: source::Value[>..] = []; var units: u16[>..] = []; var bytes: u8[>..] = []; var sources: source::SourceRange[>..] = [];
    let result = export_value(heap, root, ids, slots, units, c); var cursor: i64 = 0;
    while cursor < slots.len && c.error == Error.None {
        if !poll(c) { break; } let slot = slots[cursor]; var n = heap.nodes[slot];
        // Never read private property graphs; shape() rejects their owners.
        keys.push(origin(heap, base, imports, slot, c));
        n.value = export_value(heap, n.value, ids, slots, units, c); n.namespace_value = export_value(heap, n.namespace_value, ids, slots, units, c); n.metadata = export_value(heap, n.metadata, ids, slots, units, c);
        if has_values(n) {
            if !snapshot::range(n.start, n.count, heap.values.len) || !poll(c, n.count) { reject(c, Error.Link); break; }
            let old = n.start; n.start = values.len;
            for i in n.count { let v = export_value(heap, heap.values[old + i], ids, slots, units, c); values.push(v); }
        }
        if n.source >= 0 {
            if !snapshot::range(n.source, 1, heap.source_ranges.len) { reject(c, Error.Link); break; }
            let original_source = n.source;
            if source_ids[original_source] < 0 {
                let r = heap.source_ranges[original_source];
                if !snapshot::range(r.start, r.count, heap.source_bytes.len) || !poll(c, r.count) { reject(c, Error.Link); break; }
                source_ids[original_source] = sources.len;
                sources.push(source::SourceRange { start: bytes.len, count: r.count }); bytes.append(heap.source_bytes[r.start..(r.start + r.count)]);
            }
            n.source = source_ids[original_source];
        }
        nodes.push(n); cursor += 1;
        if bytes_of(nodes).len + bytes_of(keys).len + bytes_of(values).len + bytes_of(units).len + bytes.len + bytes_of(sources).len > c.bytes { reject(c, Error.Bytes); }
    }
    if c.error != Error.None { return Artifact {}; }
    var a = Artifact { base: base.header.checkpoint, definition: definition, version: version, root: result, keys: keys[..], nodes: nodes[..], values: values[..], units: units[..], bytes: bytes[..], sources: sources[..] };
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
fn signature_value(a: const Artifact&, v: source::Value, out: u8[>..]&, c: Control&) -> bool {
    var n = v;
    if v.tag == 5 { n.a = a.keys[v.a].slot; n.session = a.keys[v.a].arena; }
    if v.tag == 4 { n.a = 0; }
    if !signature_bytes([n], out, c) { return false; }
    return v.tag != 4 || signature_bytes(a.units[v.a..(v.a + v.b)], out, c);
}
fn signature(a: const Artifact&, id: i64, out: u8[>..]&, c: Control&) -> bool {
    var n = a.nodes[id]; n.start = 0; n.value = source::Value {}; n.namespace_value = source::Value {}; n.metadata = source::Value {}; n.source = if n.source < 0 { -1 } else { 0 };
    if !signature_bytes([n], out, c) { return false; } let original = a.nodes[id];
    for i in edge_count(original) { if !poll(c) || !signature_value(a, edge(a, original, i), out, c) { return false; } }
    if original.source >= 0 { let r = a.sources[original.source]; if !signature_bytes(a.bytes[r.start..(r.start + r.count)], out, c) { return false; } }
    return true;
}
fn relocated(v: source::Value, mapping: const i64[:], unit_start: i64, session: i64) -> source::Value {
    var out = v; if v.tag >= 4 { out.session = session; if v.tag == 5 { out.a = mapping[v.a]; } else { out.a += unit_start; } } return out;
}
// The base map is implicit and checked against both the frozen snapshot and
// the destination. This examines one record's edges, never rewalks its DAG.
fn base_signature<T>(s: const T&, slot: i64, checkpoint: i64, out: u8[>..]&, c: Control&) -> bool {
    let original = s.nodes[slot]; var n = original;
    n.start = 0; n.value = source::Value {}; n.namespace_value = source::Value {}; n.metadata = source::Value {}; n.source = if n.source < 0 { -1 } else { 0 };
    if !signature_bytes([n], out, c) { return false; }
    if has_values(original) && !snapshot::range(original.start, original.count, s.values.len) { return reject(c, Error.Link); }
    for i in edge_count(original) {
        if !poll(c) { return false; } let v = edge(s, original, i); var normalized = v;
        if v.tag >= 4 && v.session != s.header.base { return reject(c, Error.Link); }
        if v.tag == 5 { normalized.session = checkpoint; }
        if v.tag == 4 { normalized.a = 0; normalized.session = 0; }
        if !signature_bytes([normalized], out, c) { return false; }
        if v.tag == 4 {
            if !snapshot::range(v.a, v.b, s.units.len) { return reject(c, Error.Link); }
            if !signature_bytes(s.units[v.a..(v.a + v.b)], out, c) { return false; }
        }
    }
    if original.source >= 0 {
        if !snapshot::range(original.source, 1, s.sources.len) { return reject(c, Error.Link); } let r = s.sources[original.source];
        if !snapshot::range(r.start, r.count, s.bytes.len) { return reject(c, Error.Link); }
        if !signature_bytes(s.bytes[r.start..(r.start + r.count)], out, c) { return false; }
    }
    return true;
}
fn import_graph(a: const Artifact&, base: const snapshot::Snapshot&, heap: source::Heap&, imports: Imports&, c: Control&) -> source::Value {
    let started = clock_ns(); defer { c.import_ns += clock_ns() - started; }
    if !context(heap, base, imports, c) || !validate(a, base, c) { return source::Value {}; }
    var delivery: u8[>..] = []; to_bytes([a], delivery);
    c.staged_bytes += footprint(a) + delivery.len;
    if delivery.len > c.bytes || !poll(c, delivery.len) { reject(c, Error.Bytes); return source::Value {}; }
    for d in imports.definitions {
        if !poll(c) { return source::Value {}; }
        if d.id == a.definition {
            if d.version != a.version || delivery != imports.deliveries[d.start..(d.start + d.count)] { reject(c, Error.Conflict); return source::Value {}; } return d.root;
        }
    }
    var mapping: i64[>..] = []; var added: Entry[>..] = []; var signatures: u8[>..] = [];
    let empty: snapshot::Binding[] = []; let destination = snapshot::borrow(heap, empty, base.header.checkpoint);
    var new_nodes: i64 = 0;
    for k, i in a.keys {
        if !poll(c) { return source::Value {}; }
        let start = signatures.len; if !signature(a, i, signatures, c) { return source::Value {}; } var target: i64 = -1;
        if k.arena == a.base {
            var expected_bytes: u8[>..] = []; var actual_bytes: u8[>..] = [];
            if !base_signature(base, k.slot, a.base, expected_bytes, c) || !base_signature(destination, k.slot, a.base, actual_bytes, c) { return source::Value {}; }
            if expected_bytes != signatures[start..] || actual_bytes != expected_bytes { reject(c, Error.Conflict); return source::Value {}; } target = k.slot;
        } else {
            for e in imports.entries {
                if !poll(c) { return source::Value {}; }
                if e.key.arena == k.arena && e.key.slot == k.slot {
                    if signatures[start..] != imports.signatures[e.start..(e.start + e.count)] { reject(c, Error.Conflict); return source::Value {}; } target = e.target; break;
                }
            }
        }
        if target < 0 { target = heap.nodes.len + new_nodes; new_nodes += 1; added.push(Entry { key: k, target: target, start: imports.signatures.len + start, count: signatures.len - start }); }
        mapping.push(target);
    }
    if new_nodes > heap.limits.nodes - heap.nodes.len || a.values.len > heap.limits.values - heap.values.len || a.units.len > heap.limits.units - heap.units.len || a.bytes.len > 8388608 - heap.source_bytes.len || a.sources.len > 1048576 - heap.source_ranges.len { reject(c, Error.Bytes); return source::Value {}; }
    if imports.entries.len + added.len > 100000 || imports.definitions.len >= 100000 || delivery.len > c.bytes - imports.deliveries.len || signatures.len > c.bytes - imports.signatures.len { reject(c, Error.Bytes); return source::Value {}; }
    if !poll(c, a.values.len + a.units.len + a.bytes.len + a.sources.len + a.nodes.len) { return source::Value {}; }
    let unit_start = heap.units.len; let value_start = heap.values.len; let source_start = heap.source_ranges.len; let byte_start = heap.source_bytes.len;
    // Commit: all validation, comparison, budgets and link resolution finished.
    heap.units.append(a.units); for v in a.values { heap.values.push(relocated(v, mapping, unit_start, heap.identity)); }
    heap.source_bytes.append(a.bytes); for r in a.sources { heap.source_ranges.push(source::SourceRange { start: r.start + byte_start, count: r.count }); }
    for n, i in a.nodes {
        if mapping[i] < heap.nodes.len { continue; } assert(mapping[i] == heap.nodes.len);
        var out = n; out.value = relocated(n.value, mapping, unit_start, heap.identity); out.namespace_value = relocated(n.namespace_value, mapping, unit_start, heap.identity); out.metadata = relocated(n.metadata, mapping, unit_start, heap.identity);
        if has_values(n) { out.start += value_start; } if n.source >= 0 { out.source += source_start; } heap.nodes.push(out);
    }
    imports.signatures.append(signatures); imports.entries.append(added);
    let root = relocated(a.root, mapping, unit_start, heap.identity); let delivery_start = imports.deliveries.len; imports.deliveries.append(delivery);
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
