import std;
import .source_vm;
import .snapshot;

namespace callable;

// Local compiler-ABI transport. Full producer code is validated, including
// unreachable nested functions. Only reachable immutable captures are installed;
// frozen base handles reconnect to the consumer's own runtime, never its sender.
struct Publication { version: i64 = 1, base: snapshot::Header, image: snapshot::Snapshot }
struct Visit { id: i64, next: i64 = 0 }
fn fail(c: snapshot::Control&, e: snapshot::Error = snapshot::Error.Unsupported) -> bool { return snapshot::reject(c, e); }
fn same(a: source::Value, b: source::Value) -> bool { return a == b; }
// Field by field: an enum's unused payload bytes are not part of its value.
fn same(a: source::Node, b: source::Node) -> bool {
    return a.kind == b.kind && a.function == b.function && a.start == b.start && a.count == b.count && a.head == b.head && a.length == b.length && a.initialized == b.initialized && a.immutable == b.immutable
        && a.value == b.value && a.namespace_value == b.namespace_value && a.metadata == b.metadata && a.origin == b.origin && a.source == b.source && a.span_start == b.span_start && a.span_end == b.span_end;
}
fn code_prefix(p: const Publication&, bytes: const u8[:], functions: const source::Function[:], constants: const source::Value[:], children: const i64[:], ops: const source::Op[:], units: const u16[:], strings: const source::Str[:], c: snapshot::Control&) -> bool {
    let s .= p.image; let e = p.base.ends;
    let lengths:     i64[6] = [s.code_bytes.len, s.functions.len, s.code_constants.len, s.children.len, s.ops.len, s.code_units.len];
    let destination: i64[6] = [bytes.len, functions.len, constants.len, children.len, ops.len, units.len];
    for i in 6 { if !snapshot::range(0, e[i], lengths[i]) || !snapshot::range(0, e[i], destination[i]) { return fail(c, snapshot::Error.Code); } }
    if !snapshot::range(0, e[14], s.code_strings.len) || !snapshot::range(0, e[14], strings.len) || bytes_of(s.code_strings[..e[14]]) != bytes_of(strings[..e[14]]) { return fail(c, snapshot::Error.Code); }
    if !snapshot::poll(c, e[0] + e[1] + e[2] + e[3] + e[4] + e[5]) { return false; }
    if s.code_bytes[..e[0]] != bytes[..e[0]] || bytes_of(s.functions[..e[1]]) != bytes_of(functions[..e[1]]) || s.code_constants[..e[2]] != constants[..e[2]] || s.children[..e[3]] != children[..e[3]] || bytes_of(s.ops[..e[4]]) != bytes_of(ops[..e[4]]) || s.code_units[..e[5]] != units[..e[5]] { return fail(c, snapshot::Error.Code); }
    // Suffix owners are independently appendable; cross-owner ranges would
    // otherwise change meaning when the consumer already has macro/helper code.
    for f in s.functions[e[1]..] { if !snapshot::poll(c) { return false; } if f.start < e[4] || f.constants < e[2] || f.children < e[3] { return fail(c, snapshot::Error.Code); } }
    for op in s.ops[e[4]..] { if !snapshot::poll(c) { return false; } if op.start < e[0] { return fail(c, snapshot::Error.Code); } }
    for v in s.code_constants[e[2]..] { if !snapshot::poll(c) { return false; } if v.tag() == 4 && v.sid() < e[14] { return fail(c, snapshot::Error.Code); } }
    for r in s.code_strings[e[14]..] { if !snapshot::poll(c) { return false; } if r.start < e[5] { return fail(c, snapshot::Error.Code); } }
    return true;
}
// The prior image a publication is checked against: a consumer's heap and its
// code, or the frozen base snapshot itself (encode), so a producer's
// self-check builds no second heap (light-cljs-794).
fn code_matches(p: const Publication&, heap: const source::Heap&, c: snapshot::Control&) -> bool {
    let code .= heap.code; return code_prefix(p, code.source_bytes, code.functions, code.constants, code.children, code.ops, code.units, code.strings, c);
}
fn code_matches(p: const Publication&, s: const snapshot::Snapshot&, c: snapshot::Control&) -> bool { return code_prefix(p, s.code_bytes, s.functions, s.code_constants, s.children, s.ops, s.code_units, s.code_strings, c); }
fn base_node<H>(p: const Publication&, heap: const H&, id: i64, c: snapshot::Control&) -> bool {
    let s .= p.image; let n = s.nodes[id];
    if id >= heap.nodes.len || !same(n, heap.nodes[id]) { return fail(c, snapshot::Error.Link); }
    // Base ranges and property indices retain their original locations. Check
    // both values and text, and follow referenced base nodes in graph().
    if snapshot::has_values(n.kind) {
        if !snapshot::range(n.start, n.count, heap.values.len) { return fail(c, snapshot::Error.Link); }
        for i in n.count { if !snapshot::poll(c) { return false; } if !same(s.values[n.start + i], heap.values[n.start + i]) { return fail(c, snapshot::Error.Link); } }
    }
    var at = n.head;
    while at >= 0 {
        if !snapshot::poll(c) { return false; }
        if at >= heap.properties.len { return fail(c, snapshot::Error.Link); }
        let a = s.properties[at]; let b = heap.properties[at];
        if a.next != b.next || !same(a.key, b.key) || !same(a.value, b.value) { return fail(c, snapshot::Error.Link); } at = a.next;
    }
    return true;
}
fn edge_count(s: const snapshot::Snapshot&, n: source::Node, c: snapshot::Control&) -> i64 {
    var count = 3 + (if snapshot::has_values(n.kind) { n.count } else { 0 });
    var at = n.head; while at >= 0 { if !snapshot::poll(c) { return -1; } count += 2; at = s.properties[at].next; } return count;
}
fn edge(s: const snapshot::Snapshot&, n: source::Node, i: i64, c: snapshot::Control&) -> source::Value {
    if i < 3 { return match i { 0 => n.value, 1 => n.namespace_value, _ => n.metadata }; }
    var rest = i - 3;
    if snapshot::has_values(n.kind) { if rest < n.count { return s.values[n.start + rest]; } rest -= n.count; }
    var at = n.head; while rest >= 2 { if !snapshot::poll(c) { return source::Value.Undefined; } at = s.properties[at].next; rest -= 2; }
    return if rest == 0 { s.properties[at].key } else { s.properties[at].value };
}
fn graph<H>(p: const Publication&, expected: snapshot::Header, heap: const H&, ids: i64[>..]&, slots: i64[>..]&, c: snapshot::Control&) -> bool {
    if p.version != 1 || bytes_of([p.base]) != bytes_of([expected]) { return fail(c, snapshot::Error.Version); }
    let s .= p.image; let base = expected.ends[6];
    if base < 0 || base > s.nodes.len || s.bindings.len != 1 || s.bindings[0].value.tag() != 5 || s.nodes[s.bindings[0].value.id()].kind != 2 { return fail(c, snapshot::Error.Link); }
    if !code_matches(p, heap, c) { return false; }
    if !snapshot::range(0, expected.ends[9], s.units.len) || !snapshot::range(0, expected.ends[9], heap.units.len) || !snapshot::poll(c, expected.ends[9]) { return fail(c, snapshot::Error.Link); }
    if s.units[..expected.ends[9]] != heap.units[..expected.ends[9]] { return fail(c, snapshot::Error.Link); }
    if !snapshot::range(0, expected.ends[15], s.strings.len) || !snapshot::range(0, expected.ends[15], heap.strings.len) || bytes_of(s.strings[..expected.ends[15]]) != bytes_of(heap.strings[..expected.ends[15]]) { return fail(c, snapshot::Error.Link); }
    if !snapshot::poll(c, s.nodes.len * 2) { return false; }
    ids.resize(s.nodes.len, -1); var colors: u8[>..] = []; colors.resize(s.nodes.len, 0);
    var stack: Visit[>..<] = [Visit { id: s.bindings[0].value.id() }];
    while stack.len > 0 {
        if !snapshot::poll(c) { return false; }
        let id = stack[stack.len - 1].id; let n = s.nodes[id];
        if colors[id] == 0 {
            colors[id] = 1;
            if id < base {
                if n.kind == 1 || n.kind == 3 && (!n.immutable || !n.initialized) { return fail(c); }
                if !base_node(p, heap, id, c) { return false; } ids[id] = id;
            } else {
                if n.kind == 0 || n.kind == 1 || n.kind > 12 || n.head != -1 || n.kind == 3 && (!n.immutable || !n.initialized) { return fail(c); }
                ids[id] = heap.nodes.len + slots.len; slots.push(id);
            }
        }
        let i = stack[stack.len - 1].next;
        let edges = edge_count(s, n, c); if c.error != snapshot::Error.None { return false; }
        if i == edges { colors[id] = 2; stack.pop(); continue; }
        stack[stack.len - 1].next += 1;
        let v = edge(s, n, i, c); if c.error != snapshot::Error.None { return false; }
        if v.tag() != 5 { continue; }
        // A frozen base graph must not capture any producer-private object.
        if id < base && v.id() >= base { return fail(c, snapshot::Error.Link); }
        if colors[v.id()] == 1 {
            var inside = false;
            for f in stack { if !snapshot::poll(c) { return false; } if f.id == v.id() { inside = true; } if inside && s.nodes[f.id].kind != 2 && s.nodes[f.id].kind != 3 { return fail(c); } }
        } else if colors[v.id()] == 0 { stack.push(Visit { id: v.id() }); }
    }
    return true;
}
fn encode(heap: const source::Heap&, base: const snapshot::Snapshot&, root: source::Value, out: u8[>..]&, c: snapshot::Control&, checkpoint: i64) -> bool {
    let bindings = [snapshot::Binding { id: 0, generation: 1, value: root }];
    let view = snapshot::borrow(heap, bindings, checkpoint);
    if 5 * snapshot::footprint(view) + 2 * snapshot::footprint(base) > c.bytes { return fail(c, snapshot::Error.Bytes); }
    let image = snapshot::capture(heap, [snapshot::Binding { id: 0, generation: 1, value: root }], c, checkpoint);
    if c.error != snapshot::Error.None { return false; }
    let p = Publication { base: base.header, image: copy(image) };
    // Verify against the actual frozen base, not the producer's own claims.
    // No restore/initialization/identity allocation is needed for comparison.
    var ids: i64[>..] = []; var slots: i64[>..] = [];
    if !graph(p, base.header, base, ids, slots, c) { return false; }
    var bytes: u8[>..] = []; to_bytes([p], bytes);
    if bytes.len > c.bytes || !snapshot::poll(c, bytes.len) { return fail(c, snapshot::Error.Bytes); }
    out.append(bytes); return true;
}
// Installing a publication above the consumer's rows is linking, not restore:
// an object maps through the installed-slot table, a string to its header
// appended after the consumer's.
// Base headers (below `base`) are shared and were checked equal in graph().
fn relocated(v: source::Value, ids: const i64[:], base: i64, strings: i64) -> source::Value {
    return match v { Object o => source::object(ids[o.o]), String x => if x.s < base { v } else { source::Value.String { s: (x.s + strings) as u32 } }, _ => v };
}
fn shifted(n: i64, base: i64, delta: i64) -> i64 { return if n < base { n } else { n + delta }; }
fn install(p: const Publication&, expected: snapshot::Header, code: source::Code&, heap: source::Heap&, c: snapshot::Control&) -> source::Value {
    if !(heap.code .== code) || !code.valid || heap.fault != source::Fault.None || heap.live_jobs != 0 { fail(c, snapshot::Error.Boundary); return source::Value.Undefined; }
    if !snapshot::validate(p.image, c) { return source::Value.Undefined; }
    var ids: i64[>..] = []; var slots: i64[>..] = [];
    if !graph(p, expected, heap, ids, slots, c) { return source::Value.Undefined; }
    let s .= p.image; let e = expected.ends;
    let delta:   i64[6] = [code.source_bytes.len - e[0], code.functions.len - e[1], code.constants.len - e[2], code.children.len - e[3], code.ops.len - e[4], code.units.len - e[5]];
    let lengths: i64[6] = [s.code_bytes.len, s.functions.len, s.code_constants.len, s.children.len, s.ops.len, s.code_units.len];
    for i in 6 { if lengths[i] + delta[i] > 8388608 { fail(c, snapshot::Error.Bytes); return source::Value.Undefined; } }
    var count: i64 = 0; for id in slots { if snapshot::has_values(s.nodes[id].kind) { count += s.nodes[id].count; } }
    if slots.len > heap.limits.nodes - heap.nodes.len || count > heap.limits.values - heap.values.len || s.units.len > heap.limits.units - heap.units.len || s.code_constants.len - e[2] > heap.limits.values - heap.constants.len || s.bytes.len > 8388608 - heap.source_bytes.len || s.sources.len > 1048576 - heap.source_ranges.len || s.functions.len + delta[1] > 65536 || s.code_bytes.len + delta[0] > 8388608 { fail(c, snapshot::Error.Bytes); return source::Value.Undefined; }
    // Cover existing destination + full decoded image + staging/mapping work.
    let empty: snapshot::Binding[] = []; let view = snapshot::borrow(heap, empty, expected.checkpoint);
    let cost = snapshot::footprint(view) + 2 * snapshot::footprint(s) + bytes_of(ids).len + bytes_of(slots).len;
    if cost > c.bytes || !snapshot::poll(c, snapshot::payload_bytes(s) + count + slots.len) { fail(c, snapshot::Error.Bytes); return source::Value.Undefined; }
    let units = heap.units.len; let sources = heap.source_ranges.len; let bytes = heap.source_bytes.len;
    // Commit only after complete code/graph/context validation and all budgets.
    for f in s.functions[e[1]..] { var n = f; n.start += delta[4]; n.constants += delta[2]; n.children += delta[3]; code.functions.push(n); }
    for op in s.ops[e[4]..] { var n = op; n.start += delta[0]; n.end += delta[0]; if source::branches(n.code) { n.arg += delta[4]; } code.ops.push(n); }
    let code_strings = code.strings.len - e[14];
    for r in s.code_strings[e[14]..] { code.strings.push(source::Str { start: r.start + delta[5], len: r.len }); }
    for v in s.code_constants[e[2]..] { code.constants.push(if v.tag() == 4 { source::Value.String { s: (v.sid() + code_strings) as u32 } } else { v }); }
    for id in s.children[e[3]..] { code.children.push(shifted(id, e[1], delta[1])); }
    code.units.append(s.code_units[e[5]..]); code.source_bytes.append(s.code_bytes[e[0]..]); source::decode(code);
    // Constants carry heap strings, not offsets into the separate Code owner.
    // The base prefix of strings is shared (checked in graph); the rest follow.
    let strings = heap.strings.len - expected.ends[15];
    for r in s.strings[expected.ends[15]..] { heap.strings.push(source::Str { start: r.start + units, len: r.len }); }
    for v in s.constants[e[2]..] { heap.constants.push(relocated(v, ids, expected.ends[15], strings)); }
    heap.units.append(s.units); heap.source_bytes.append(s.bytes);
    for r in s.sources { heap.source_ranges.push(source::SourceRange { start: bytes + r.start, count: r.count }); }
    for id in slots {
        var n = s.nodes[id]; n.value = relocated(n.value, ids, expected.ends[15], strings); n.namespace_value = relocated(n.namespace_value, ids, expected.ends[15], strings); n.metadata = relocated(n.metadata, ids, expected.ends[15], strings);
        if n.kind == 2 { n.function = shifted(n.function, e[1], delta[1]); }
        if snapshot::has_values(n.kind) { n.start = heap.values.len; for v in s.values[s.nodes[id].start..(s.nodes[id].start + n.count)] { heap.values.push(relocated(v, ids, expected.ends[15], strings)); } }
        if n.source >= 0 { n.source += sources; } heap.nodes.push(n);
    }
    c.copied_bytes += snapshot::payload_bytes(s); return relocated(s.bindings[0].value, ids, expected.ends[15], strings);
}
fn decode_install(bytes: const u8[:], base: snapshot::Header, code: source::Code&, heap: source::Heap&, c: snapshot::Control&) -> source::Value {
    if bytes.len > c.bytes || !snapshot::poll(c, bytes.len) { fail(c, snapshot::Error.Bytes); return source::Value.Undefined; }
    let records, ok = from_bytes<Publication[>..]>(bytes);
    if !ok || records.len != 1 { fail(c, snapshot::Error.Length); return source::Value.Undefined; }
    for p in records { return install(p, base, code, heap, c); }
    return source::Value.Undefined;
}
