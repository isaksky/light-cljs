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
fn same(a: source::Value, b: source::Value) -> bool { return source::same_bits(a, b); }
// Field by field: an enum's unused payload bytes are not part of its value.
fn same(a: source::Node, b: source::Node) -> bool {
    return a.kind == b.kind && a.function == b.function && a.start == b.start && a.count == b.count && a.keys == b.keys && a.key_count == b.key_count && a.key_capacity == b.key_capacity && a.length == b.length && a.initialized == b.initialized && a.immutable == b.immutable
        && same(a.value, b.value) && same(a.namespace_value, b.namespace_value) && same(a.metadata, b.metadata) && a.origin == b.origin && a.source == b.source && a.span_start == b.span_start && a.span_end == b.span_end
        && a.shape == b.shape && same(a.proto, b.proto) && source::plain_rows(a.slots) == source::plain_rows(b.slots);
}
fn code_prefix(p: const Publication&, bytes: const u8[:], functions: const source::Function[:], constants: const source::Value[:], children: const u32[:], ops: const source::Op[:], units: const u16[:], strings: const source::Str[:], sites: const source::Site[:], site_sources: const source::SiteSource[:], site_records: const u8[:], c: snapshot::Control&) -> bool {
    let s .= p.image; let e = p.base.ends;
    // Direct-eval sites (light-cljs-858): the base's are shared; a suffix
    // site belongs to suffix code, with suffix sources and record bytes.
    if !snapshot::range(0, e[21], s.sites.len) || !snapshot::range(0, e[21], sites.len) || !snapshot::range(0, e[22], s.site_sources.len) || !snapshot::range(0, e[22], site_sources.len) || !snapshot::range(0, e[23], s.site_records.len) || !snapshot::range(0, e[23], site_records.len) { return fail(c, snapshot::Error.Code); }
    if !snapshot::poll(c, e[21] + e[22] + e[23]) { return false; }
    if bytes_of(s.sites[..e[21]]) != bytes_of(sites[..e[21]]) || bytes_of(s.site_sources[..e[22]]) != bytes_of(site_sources[..e[22]]) || s.site_records[..e[23]] != site_records[..e[23]] { return fail(c, snapshot::Error.Code); }
    for x in s.sites[e[21]..] { if !snapshot::poll(c) { return false; } if x.op < e[4] || x.sources < e[22] || x.record < e[23] { return fail(c, snapshot::Error.Code); } }
    let lengths:     i64[6] = [s.code_bytes.len, s.functions.len, s.code_constants.len, s.children.len, s.ops.len, s.code_units.len];
    let destination: i64[6] = [bytes.len, functions.len, constants.len, children.len, ops.len, units.len];
    for i in 6 { if !snapshot::range(0, e[i], lengths[i]) || !snapshot::range(0, e[i], destination[i]) { return fail(c, snapshot::Error.Code); } }
    if !snapshot::range(0, e[14], s.code_strings.len) || !snapshot::range(0, e[14], strings.len) || bytes_of(s.code_strings[..e[14]]) != bytes_of(strings[..e[14]]) { return fail(c, snapshot::Error.Code); }
    if !snapshot::poll(c, e[0] + e[1] + e[2] + e[3] + e[4] + e[5]) { return false; }
    if s.code_bytes[..e[0]] != bytes[..e[0]] || bytes_of(s.functions[..e[1]]) != bytes_of(functions[..e[1]]) || source::plain_rows(s.code_constants[..e[2]]) != source::plain_rows(constants[..e[2]]) || s.children[..e[3]] != children[..e[3]] || bytes_of(s.ops[..e[4]]) != bytes_of(ops[..e[4]]) || s.code_units[..e[5]] != units[..e[5]] { return fail(c, snapshot::Error.Code); }
    // Suffix owners are independently appendable; cross-owner ranges would
    // otherwise change meaning when the consumer already has macro/helper code.
    for f in s.functions[e[1]..] { if !snapshot::poll(c) { return false; } if f.start < e[4] || f.constants < e[2] || f.children < e[3] || f.name_len > 0 && f.name_start < e[5] || f.has_text && f.text_start < e[0] { return fail(c, snapshot::Error.Code); } }
    for op in s.ops[e[4]..] { if !snapshot::poll(c) { return false; } if op.start < e[0] { return fail(c, snapshot::Error.Code); } }
    for v in s.code_constants[e[2]..] { if !snapshot::poll(c) { return false; } if v.is_string() && v.sid() < e[14] { return fail(c, snapshot::Error.Code); } }
    for r in s.code_strings[e[14]..] { if !snapshot::poll(c) { return false; } if r.start < e[5] { return fail(c, snapshot::Error.Code); } }
    return true;
}
// The prior image a publication is checked against: a consumer's heap and its
// code, or the frozen base snapshot itself (encode), so a producer's
// self-check builds no second heap (light-cljs-794).
fn code_matches(p: const Publication&, heap: const source::Heap&, c: snapshot::Control&) -> bool {
    let code .= heap.code; return code_prefix(p, code.source_bytes, code.functions, code.constants, code.children, code.ops, code.units, code.strings, code.sites, code.site_sources, code.site_records, c);
}
fn code_matches(p: const Publication&, s: const snapshot::Snapshot&, c: snapshot::Control&) -> bool { return code_prefix(p, s.code_bytes, s.functions, s.code_constants, s.children, s.ops, s.code_units, s.code_strings, s.sites, s.site_sources, s.site_records, c); }
fn base_node<H>(p: const Publication&, heap: const H&, id: i64, c: snapshot::Control&) -> bool {
    let s .= p.image; let n = s.nodes[id];
    if id >= heap.nodes.len || !same(n, heap.nodes[id]) { return fail(c, snapshot::Error.Link); }
    // Base ranges and property indices retain their original locations. Check
    // both values and text, and follow referenced base nodes in graph().
    if snapshot::has_values(n.kind) {
        if !snapshot::range(n.start, n.count, heap.values.len) { return fail(c, snapshot::Error.Link); }
        for i in n.count { if !snapshot::poll(c) { return false; } if !same(s.values[n.start + i], heap.values[n.start + i]) { return fail(c, snapshot::Error.Link); } }
    }
    // Inline slots were compared with the header (same); the overflow run of
    // a shape-mode object, its shape chain and accessor pairs, or a
    // dictionary's records, here (light-cljs-787).
    if snapshot::shaped(n) {
        let spilled = source::overflow(n);
        if !snapshot::range(n.keys, spilled, heap.values.len) { return fail(c, snapshot::Error.Link); }
        for at in n.keys..(n.keys + spilled) { if !snapshot::poll(c) { return false; } if !same(s.values[at], heap.values[at]) || !same_pair(s, heap, s.values[at]) { return fail(c, snapshot::Error.Link); } }
        for v, i in n.slots { if i < n.key_count && !same_pair(s, heap, v) { return fail(c, snapshot::Error.Link); } }
        var at: i64 = n.shape;
        while at != 0 {
            if !snapshot::poll(c) { return false; }
            if at >= heap.shapes.len { return fail(c, snapshot::Error.Link); }
            let a = s.shapes[at]; let b = heap.shapes[at];
            if a.parent != b.parent || !same(a.key, b.key) || a.attrs != b.attrs || a.count != b.count { return fail(c, snapshot::Error.Link); }
            at = a.parent;
        }
    } else if snapshot::dictionary(n) {
        if !snapshot::range(n.keys, n.key_count, heap.properties.len) { return fail(c, snapshot::Error.Link); }
        for at in n.keys..(n.keys + n.key_count) {
            if !snapshot::poll(c) { return false; }
            let a = s.properties[at]; let b = heap.properties[at];
            if !same(a.key, b.key) || !same(a.value, b.value) || a.attrs != b.attrs || !same_pair(s, heap, a.value) { return fail(c, snapshot::Error.Link); }
        }
    }
    return true;
}
// An accessor property's pair is the same row with the same functions.
fn same_pair<H>(s: const snapshot::Snapshot&, heap: const H&, v: source::Value) -> bool {
    return match v { Accessor x => x.a < heap.accessors.len && same(s.accessors[x.a].get, heap.accessors[x.a].get) && same(s.accessors[x.a].set, heap.accessors[x.a].set), _ => true };
}
fn edge_count(s: const snapshot::Snapshot&, n: source::Node, c: snapshot::Control&) -> i64 {
    var count = 4 + (if snapshot::has_values(n.kind) { n.count } else { 0 });
    if !snapshot::poll(c, n.key_count) { return -1; } return count + 2 * n.key_count;
}
fn edge(s: const snapshot::Snapshot&, n: source::Node, i: i64, c: snapshot::Control&) -> source::Value {
    if i < 4 { return match i { 0 => n.value, 1 => n.namespace_value, 2 => n.proto, _ => n.metadata }; }
    var rest = i - 4;
    if snapshot::has_values(n.kind) { if rest < n.count { return s.values[n.start + rest]; } rest -= n.count; }
    // Each own property: its value, or an accessor's getter and setter.
    let k = rest / 2;
    let v = if snapshot::dictionary(n) { s.properties[n.keys + k].value } else if k < source::inline_slots() { n.slots[k] } else { s.values[n.keys + k - source::inline_slots()] };
    return match v { Accessor x => if rest % 2 == 0 { s.accessors[x.a].get } else { s.accessors[x.a].set }, _ => if rest % 2 == 0 { v } else { source::Value.Undefined } };
}
fn graph<H>(p: const Publication&, expected: snapshot::Header, heap: const H&, ids: i64[>..]&, slots: i64[>..]&, c: snapshot::Control&) -> bool {
    if p.version != 1 || bytes_of([p.base]) != bytes_of([expected]) { return fail(c, snapshot::Error.Version); }
    let s .= p.image; let base = expected.ends[6];
    // The root is a closure, or a value that reaches a pending delay cell
    // whose thunk only this transport carries (light-cljs-808): a form or a
    // cell reference.
    if base < 0 || base > s.nodes.len || s.bindings.len != 1 || !s.bindings[0].value.is_object() { return fail(c, snapshot::Error.Link); }
    let root_kind = s.nodes[s.bindings[0].value.id()].kind;
    if root_kind != node_kind::Closure && root_kind != node_kind::Cell && (root_kind < node_kind::List || root_kind > node_kind::Concat) { return fail(c, snapshot::Error.Link); }
    if !code_matches(p, heap, c) { return false; }
    if !snapshot::range(0, expected.ends[9], s.units.len) || !snapshot::range(0, expected.ends[9], heap.units.len) || !snapshot::poll(c, expected.ends[9]) { return fail(c, snapshot::Error.Link); }
    if s.units[..expected.ends[9]] != heap.units[..expected.ends[9]] { return fail(c, snapshot::Error.Link); }
    if !snapshot::range(0, expected.ends[16], s.latin1.len) || !snapshot::range(0, expected.ends[16], heap.latin1.len) || !snapshot::poll(c, expected.ends[16]) || s.latin1[..expected.ends[16]] != heap.latin1[..expected.ends[16]] { return fail(c, snapshot::Error.Link); }
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
                // A realm intrinsic (every realm row but the global object)
                // ends the walk (light-cljs-875): every consumer has the
                // same rows (initialize in the image owner, realm_valid on
                // every restore) and no job writes them (decision 3's write
                // rule), so what it reaches is the consumer's own, whatever
                // its kind. Its row is still compared with the consumer's,
                // but its edges are not followed: the realm's constructor
                // and prototype links are cycles of objects, which the
                // check below rejects elsewhere.
                if base >= source::realm_nodes() && source::realm_intrinsic(id) {
                    if !base_node(p, heap, id, c) { return false; }
                    ids[id] = id; colors[id] = 2; stack.pop(); continue;
                }
                if n.kind == node_kind::Array || n.kind == node_kind::Binding && (!n.immutable || !n.initialized) || n.kind == node_kind::MappedBinding || n.kind == node_kind::ArgumentMap { return fail(c); }
                if !base_node(p, heap, id, c) { return false; } ids[id] = id;
            } else {
                // A cell reference (18) is shareable; an atom was made one
                // before the publication (source::publish_cells).
                if n.kind == node_kind::Object || n.kind == node_kind::Array || n.kind > node_kind::Concat && n.kind != node_kind::Cell || n.keys != 0 || n.key_count != 0 || n.key_capacity != 0 || n.shape != 0 || !n.proto.is_hole() || source::plain_rows(n.slots) != source::plain_rows([source::Value.Undefined, source::Value.Undefined]) || n.kind == node_kind::Binding && (!n.immutable || !n.initialized) { return fail(c); }
                ids[id] = heap.nodes.len + slots.len; slots.push(id);
            }
        }
        let i = stack[stack.len - 1].next;
        let edges = edge_count(s, n, c); if c.error != snapshot::Error.None { return false; }
        if i == edges { colors[id] = 2; stack.pop(); continue; }
        stack[stack.len - 1].next += 1;
        let v = edge(s, n, i, c); if c.error != snapshot::Error.None { return false; }
        if !v.is_object() { continue; }
        // A frozen base graph must not capture any producer-private object.
        if id < base && v.id() >= base { return fail(c, snapshot::Error.Link); }
        if colors[v.id()] == 1 {
            var inside = false;
            for f in stack { if !snapshot::poll(c) { return false; } if f.id == v.id() { inside = true; } if inside && s.nodes[f.id].kind != node_kind::Closure && s.nodes[f.id].kind != node_kind::Binding { return fail(c); } }
        } else if colors[v.id()] == 0 { stack.push(Visit { id: v.id() }); }
    }
    return true;
}
fn encode(heap: const source::Heap&, base: const snapshot::Snapshot&, root: source::Value, out: u8[>..]&, c: snapshot::Control&, checkpoint: i64) -> bool { var slots: i64[>..] = []; return encode(heap, base, root, out, c, checkpoint, slots); }
// encode, and the producer row of each installed slot, in install order (the
// places of the publication's objects, light-cljs-808).
fn encode(heap: const source::Heap&, base: const snapshot::Snapshot&, root: source::Value, out: u8[>..]&, c: snapshot::Control&, checkpoint: i64, installed: i64[>..]&) -> bool {
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
    installed.clear(); installed.append(slots);
    var bytes: u8[>..] = []; to_bytes([p], bytes);
    if bytes.len > c.bytes || !snapshot::poll(c, bytes.len) { return fail(c, snapshot::Error.Bytes); }
    out.append(bytes); return true;
}
// A function stored into a cell (cells.goose, light-cljs-809): published as
// a helper is, against the session's image header `base`, from a heap whose
// job may be running the storing built-in. Its image rows are the image's
// (the write rule keeps them), so the self-check runs against the heap
// itself and every consumer's install checks them against its own.
// `installed` gets the producer row of each installed slot, in install
// order, the root first.
fn encode_cell(heap: const source::Heap&, base: snapshot::Header, root: source::Value, out: u8[>..]&, c: snapshot::Control&, installed: i64[>..]&) -> bool {
    let bindings = [snapshot::Binding { id: 0, generation: 1, value: root }];
    let view = snapshot::borrow(heap, bindings, 1);
    if 5 * snapshot::footprint(view) > c.bytes { return fail(c, snapshot::Error.Bytes); }
    let image = snapshot::capture(heap, bindings, c, if heap.identity == 1 { 2 } else { 1 }, true);
    if c.error != snapshot::Error.None { return false; }
    var p = Publication { base: base, image: copy(image) };
    var ids: i64[>..] = []; var slots: i64[>..] = [];
    if !graph(p, base, heap, ids, slots, c) { return false; }
    if slots.len == 0 || slots[0] != root.id() { return fail(c, snapshot::Error.Link); }
    // A captured form outlives its call, as a form cell value does
    // (cells.goose encode): its compiler occurrence and source span mean
    // nothing to a later expansion, which sees it as built afresh.
    for id in slots {
        let k = p.image.nodes[id].kind;
        if k >= node_kind::List && k <= node_kind::Concat { p.image.nodes[id].origin = -1; p.image.nodes[id].source = source::no_row(); p.image.nodes[id].span_start = 0; p.image.nodes[id].span_end = 0; }
    }
    installed.clear(); installed.append(slots);
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
// `live`: as encode_cell, a running job loads a function from a cell; its
// frames hold rows and code indices, which installing only appends to.
fn install(p: const Publication&, expected: snapshot::Header, code: source::Code&, heap: source::Heap&, c: snapshot::Control&, live: bool = false) -> source::Value {
    if !(heap.code .== code) || !code.valid || heap.fault != source::Fault.None || heap.live_jobs != 0 && !(live && heap.live_jobs == 1) { fail(c, snapshot::Error.Boundary); return source::Value.Undefined; }
    if !snapshot::validate(p.image, c) { return source::Value.Undefined; }
    var ids: i64[>..] = []; var slots: i64[>..] = [];
    if !graph(p, expected, heap, ids, slots, c) { return source::Value.Undefined; }
    let s .= p.image; let e = expected.ends;
    let delta:   i64[6] = [code.source_bytes.len - e[0], code.functions.len - e[1], code.constants.len - e[2], code.children.len - e[3], code.ops.len - e[4], code.units.len - e[5]];
    let lengths: i64[6] = [s.code_bytes.len, s.functions.len, s.code_constants.len, s.children.len, s.ops.len, s.code_units.len];
    for i in 6 { if lengths[i] + delta[i] > 8388608 { fail(c, snapshot::Error.Bytes); return source::Value.Undefined; } }
    // The merged direct-eval site tables stay within a snapshot's lengths too.
    if s.sites.len - e[21] > 8388608 - code.sites.len || s.site_sources.len - e[22] > 8388608 - code.site_sources.len || s.site_records.len - e[23] > 8388608 - code.site_records.len { fail(c, snapshot::Error.Bytes); return source::Value.Undefined; }
    var count: i64 = 0; for id in slots { if snapshot::has_values(s.nodes[id].kind) { count += s.nodes[id].count; } }
    if !source::nodes_fit(heap, slots.len) || !source::values_fit(heap, count) || !source::units_fit(heap, s.units.len + s.latin1.len) || s.strings.len > heap.limits.units - (heap.strings.len - heap.image.strings) || s.code_constants.len - e[2] > heap.limits.values - heap.constants.len || s.bytes.len > 8388608 - heap.source_bytes.len || s.sources.len > 1048576 - heap.source_ranges.len || s.functions.len + delta[1] > 65536 || s.code_bytes.len + delta[0] > 8388608 { fail(c, snapshot::Error.Bytes); return source::Value.Undefined; }
    // Cover existing destination + full decoded image + staging/mapping work.
    let empty: snapshot::Binding[] = []; let view = snapshot::borrow(heap, empty, expected.checkpoint);
    let cost = snapshot::footprint(view) + 2 * snapshot::footprint(s) + bytes_of(ids).len + bytes_of(slots).len;
    if cost > c.bytes || !snapshot::poll(c, snapshot::payload_bytes(s) + count + slots.len) { fail(c, snapshot::Error.Bytes); return source::Value.Undefined; }
    let sources = heap.source_ranges.len;
    // Commit only after complete code/graph/context validation and all budgets.
    for f in s.functions[e[1]..] { var n = f; n.start += delta[4] as u32; n.constants += delta[2] as u32; n.children += delta[3] as u32; n.name_start += delta[5] as u32; if n.has_text { n.text_start += delta[0] as u32; n.text_end += delta[0] as u32; } code.functions.push(n); }
    for op in s.ops[e[4]..] { var n = op; n.start += delta[0]; n.end += delta[0]; if source::branches(n.code) { n.arg += delta[4]; } code.ops.push(n); }
    let code_strings = code.strings.len - e[14];
    for r in s.code_strings[e[14]..] { code.strings.push(source::Str { start: r.start + delta[5] as u32, len: r.len }); }
    for v in s.code_constants[e[2]..] { code.constants.push(if v.is_string() { source::Value.String { s: (v.sid() + code_strings) as u32 } } else { v }); }
    for id in s.children[e[3]..] { code.children.push(shifted(id, e[1], delta[1]) as u32); }
    let site_sources = code.site_sources.len - e[22]; let site_records = code.site_records.len - e[23];
    for x in s.sites[e[21]..] { var n = x; n.op += delta[4] as u32; n.sources += site_sources as u32; n.record += site_records as u32; code.sites.push(n); }
    code.site_sources.append(s.site_sources[e[22]..]); code.site_records.append(s.site_records[e[23]..]);
    code.units.append(s.code_units[e[5]..]); code.source_bytes.append(s.code_bytes[e[0]..]); source::decode(code);
    // Constants carry heap strings, not offsets into the separate Code owner.
    // The base prefix of strings is shared (checked in graph); the rest follow.
    let strings = heap.strings.len - expected.ends[15];
    // The preflight above reserved every row, so no allocation below fails.
    let units = source::add_units(heap, s.units); let latin1 = source::add_latin1(heap, s.latin1); let bytes = source::add_source_bytes(heap, s.bytes);
    for r in s.strings[expected.ends[15]..] { source::string_ref(heap, r.start + (if r.wide { units } else { latin1 }), r.len, r.wide); }
    for v in s.constants[e[2]..] { source::add_constants(heap, [relocated(v, ids, expected.ends[15], strings)]); }
    for r in s.sources { source::add_source_range(heap, source::shifted_source(r, bytes)); }
    for id in slots {
        var n = s.nodes[id]; n.value = relocated(n.value, ids, expected.ends[15], strings); n.namespace_value = relocated(n.namespace_value, ids, expected.ends[15], strings); n.metadata = relocated(n.metadata, ids, expected.ends[15], strings);
        if n.kind == node_kind::Closure { n.function = shifted(n.function, e[1], delta[1]) as u32; }
        if snapshot::has_values(n.kind) { n.start = heap.values.len as u32; for v in s.values[s.nodes[id].start..(s.nodes[id].start + n.count)] { source::add_values(heap, [relocated(v, ids, expected.ends[15], strings)]); } }
        if n.source != source::no_row() { n.source += sources as u32; } source::add_nodes(heap, [n]);
    }
    c.copied_bytes += snapshot::payload_bytes(s); return relocated(s.bindings[0].value, ids, expected.ends[15], strings);
}
fn decode_install(bytes: const u8[:], base: snapshot::Header, code: source::Code&, heap: source::Heap&, c: snapshot::Control&, live: bool = false) -> source::Value {
    if bytes.len > c.bytes || !snapshot::poll(c, bytes.len) { fail(c, snapshot::Error.Bytes); return source::Value.Undefined; }
    let records, ok = from_bytes<Publication[>..]>(bytes);
    if !ok || records.len != 1 { fail(c, snapshot::Error.Length); return source::Value.Undefined; }
    for p in records { return install(p, base, code, heap, c, live); }
    return source::Value.Undefined;
}
