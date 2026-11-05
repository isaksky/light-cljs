import std;
import binary;
import .source_vm;
import .snapshot;
import .program;

namespace bridge;

// GFB1: tagged structured graphs, never source text. Child and metadata edges
// name occurrences; repeated IDs preserve aliases, equal nodes are not interned.
// Tags nil/bool/Number/string/symbol/keyword/list/vector/map/set = 0..9.
struct Form { tag: i64 = 0, number: f64 = 0, start: i64 = 0, count: i64 = 0, ns_start: i64 = 0, ns_count: i64 = -1, metadata: i64 = -1, origin: i64 = -1, source: i64 = -1, span_start: i64 = 0, span_end: i64 = 0 }
root struct Graph { nodes: Form[>..] = [], children: i64[>..] = [], units: u16[>..] = [], sources: source::SourceRange[>..] = [], bytes: u8[>..] = [], roots: i64[>..] = [] }
enum Error { None, Malformed, Nodes, Depth, Bytes, Work, Cancelled, Unsupported, Heap }
struct Control { nodes: i64 = 100000, depth: i64 = 128, bytes: i64 = 16777216, work: i64 = 10000000, cancel_after: i64 = -1, visits: i64 = 0, error: Error = Error.None, cancelled: bool = false }
fn poll(c: Control&, count: i64 = 1) -> bool {
    if c.error != Error.None { return false; }
    if c.cancelled { c.error = Error.Cancelled; return false; }
    if count < 0 || count > c.work - c.visits { c.error = Error.Work; return false; }
    if c.cancel_after >= 0 && count > c.cancel_after - c.visits { c.error = Error.Cancelled; return false; }
    c.visits += count; return true;
}
fn reject(c: Control&, error: Error = Error.Malformed) -> bool { if c.error == Error.None { c.error = error; } return false; }
fn range(start: i64, count: i64, len: i64) -> bool { return start >= 0 && count >= 0 && start <= len && count <= len - start; }
fn footprint(g: const Graph&) -> i64 { return 32 + g.nodes.len * 48 + g.children.len * 4 + g.units.len * 2 + g.sources.len * 8 + g.bytes.len + g.roots.len * 4; }
struct Edge { value: source::Value, parent: i64, field: i64 }
struct Retain { value: source::Value, depth: i64, argument: bool = false }
fn scalar_equal(heap: source::Heap&, a: source::Value, b: source::Value, c: Control&) -> bool {
    if !source::valid(heap, a) || !source::valid(heap, b) { heap.fault = source::Fault.None; reject(c, Error.Heap); return false; }
    if a.tag() != b.tag() { return false; }
    match a.tag() {
        0, 1 => { return true; }, 2 => { return a.flag() == b.flag(); },
        3 => { return a.num() == b.num() && (a.num() != 0 || 1.0 / a.num() == 1.0 / b.num()) || a.num() != a.num() && b.num() != b.num(); },
        4 => { if !poll(c, source::str_len(heap, a) + source::str_len(heap, b)) { return false; } return source::same_text(heap, a, b); },
        _ => { return false; },
    }
}
struct Walk { id: i64, edge: i64 = -1 }
// Validate every node, including unreachable metadata and shared subgraphs.
// Explicit postorder computes longest path without an unbudgeted retain walk.
fn validate(g: const Graph&, c: Control&) -> bool {
    if g.nodes.len > c.nodes { return reject(c, Error.Nodes); }
    if footprint(g) > c.bytes { return reject(c, Error.Bytes); }
    var states: i64[>..] = []; var heights: i64[>..] = [];
    if !poll(c, g.nodes.len + g.children.len + g.units.len + g.sources.len + g.bytes.len + g.roots.len) { return false; }
    states.resize(g.nodes.len, 0); heights.resize(g.nodes.len, 0);
    for s in g.sources {
        if !range(s.start, s.count, g.bytes.len) { return reject(c); }
        if !poll(c, s.count) { return false; }
        if !snapshot::utf8(g.bytes[s.start..(s.start + s.count)]) { return reject(c); }
    }
    for id in g.roots { if id < 0 || id >= g.nodes.len { return reject(c); } }
    for n in g.nodes {
        if !poll(c) { return false; }
        if n.tag < 0 || n.tag > 9 || n.metadata < -1 || n.metadata >= g.nodes.len || n.source < -1 || n.source >= g.sources.len || n.origin < -1 { return reject(c); }
        if n.source >= 0 && !range(n.span_start, n.span_end - n.span_start, g.sources[n.source].count) { return reject(c); }
        if n.source >= 0 {
            let s = g.sources[n.source]; let text = g.bytes[s.start..(s.start + s.count)];
            if !program::boundary(text, n.span_start) || !program::boundary(text, n.span_end) { return reject(c); }
        }
        if n.tag == 1 && n.number != 0 && n.number != 1 { return reject(c); }
        if n.tag >= 3 && n.tag <= 5 && !range(n.start, n.count, g.units.len) { return reject(c); }
        if (n.tag == 4 || n.tag == 5) && (n.ns_count < -1 || n.ns_count != -1 && !range(n.ns_start, n.ns_count, g.units.len)) { return reject(c); }
        if n.tag == 4 || n.tag == 5 {
            if n.ns_count >= 0 {
                if n.ns_count == 0 && n.count == 0 { return reject(c, Error.Unsupported); }
                for i in n.ns_count { if !poll(c) { return false; } if g.units[n.ns_start + i] == 47 { return reject(c, Error.Unsupported); } }
            } else { for i in n.count { if !poll(c) { return false; } if g.units[n.start + i] == 47 && n.count != 1 { return reject(c, Error.Unsupported); } } }
        }
        if n.metadata >= 0 && g.nodes[n.metadata].tag != 8 { return reject(c, Error.Unsupported); }
        if n.tag >= 6 && (!range(n.start, n.count, g.children.len) || n.tag == 8 && n.count % 2 != 0) { return reject(c); }
    }
    for id in g.children { if id < 0 || id >= g.nodes.len { return reject(c); } }
    for root in g.nodes.len {
        if states[root] == 2 { continue; }
        var stack: Walk[>..<] = [Walk { id: root }];
        while stack.len > 0 {
            if !poll(c) { return false; }
            let w = stack[stack.len - 1]; let n = g.nodes[w.id]; states[w.id] = 1;
            let edge_count = (if n.tag >= 6 { n.count } else { 0 }) + (if n.metadata >= 0 { 1 } else { 0 });
            let next = w.edge + 1;
            if next < edge_count {
                stack[stack.len - 1].edge = next;
                let child = if next == 0 && n.metadata >= 0 { n.metadata } else { g.children[n.start + next - (if n.metadata >= 0 { 1 } else { 0 })] };
                if states[child] == 1 { return reject(c); }
                if states[child] == 0 { if stack.len >= c.depth { return reject(c, Error.Depth); } stack.push(Walk { id: child }); }
            } else {
                var height: i64 = 1;
                if n.metadata >= 0 { height = max(height, 1 + heights[n.metadata]); }
                if n.tag >= 6 { for i in n.count { if !poll(c) { return false; } height = max(height, 1 + heights[g.children[n.start + i]]); } }
                if height > c.depth { return reject(c, Error.Depth); }
                heights[w.id] = height; states[w.id] = 2; stack.pop();
            }
        }
    }
    return true;
}
fn word(bytes: const u8[:], at: i64) -> i64 { let n, ok = binary::read_u32le(bytes, at); assert(ok); return n; }
fn signed(bytes: const u8[:], at: i64) -> i64 { let n = word(bytes, at); return if n >= 2147483648 { n - 4294967296 } else { n }; }
fn append_graph(out: Graph&, g: const Graph&) {
    assert(out.nodes.len == 0 && out.children.len == 0 && out.units.len == 0 && out.sources.len == 0 && out.bytes.len == 0 && out.roots.len == 0);
    out.nodes.append(g.nodes); out.children.append(g.children); out.units.append(g.units); out.sources.append(g.sources); out.bytes.append(g.bytes); out.roots.append(g.roots);
}
fn decode(out: Graph&, bytes: const u8[:], c: Control&) -> bool {
    if bytes.len > c.bytes { return reject(c, Error.Bytes); }
    if !poll(c, bytes.len) { return false; }
    if bytes.len < 32 || bytes[..4] != "GFB1" || word(bytes, 4) != 1 { return reject(c); }
    let nodes = word(bytes, 8); let children = word(bytes, 12); let units = word(bytes, 16); let sources = word(bytes, 20); let text = word(bytes, 24); let roots = word(bytes, 28);
    if nodes > c.nodes { return reject(c, Error.Nodes); }
    if 32 + nodes * 48 + children * 4 + units * 2 + sources * 8 + text + roots * 4 != bytes.len { return reject(c); }
    var g = Graph {}; var at: i64 = 32;
    for i in nodes {
        if !poll(c) { return false; }
        let number, ok = binary::read_f64le(bytes, at + 4); assert(ok);
        g.nodes.push(Form { tag: word(bytes, at), number: number, start: signed(bytes, at + 12), count: signed(bytes, at + 16), ns_start: signed(bytes, at + 20), ns_count: signed(bytes, at + 24), metadata: signed(bytes, at + 28), origin: signed(bytes, at + 32), source: signed(bytes, at + 36), span_start: signed(bytes, at + 40), span_end: signed(bytes, at + 44) }); at += 48;
    }
    for i in children { if !poll(c) { return false; } g.children.push(word(bytes, at)); at += 4; }
    for i in units { if !poll(c) { return false; } let u, ok = binary::read_u16le(bytes, at); assert(ok); g.units.push(u); at += 2; }
    for i in sources { if !poll(c) { return false; } g.sources.push(source::SourceRange { start: word(bytes, at) as u32, count: word(bytes, at + 4) as u32 }); at += 8; }
    for i in text { if !poll(c) { return false; } g.bytes.push(bytes[at]); at += 1; }
    for i in roots { if !poll(c) { return false; } g.roots.push(word(bytes, at)); at += 4; }
    if !validate(g, c) { return false; } append_graph(out, g); return true;
}
fn put_word(out: u8[>..]&, n: i64) { for i in 4 { out.push(((n >> (i * 8)) & 255) as u8); } }
fn encode(g: const Graph&, out: u8[>..]&, c: Control&) -> bool {
    if !validate(g, c) { return false; }
    if !poll(c, footprint(g)) { return false; }
    var bytes: u8[>..] = []; bytes.append("GFB1"); put_word(bytes, 1);
    for n in [g.nodes.len, g.children.len, g.units.len, g.sources.len, g.bytes.len, g.roots.len] { put_word(bytes, n); }
    for n in g.nodes {
        put_word(bytes, n.tag); for byte in bytes_of([n.number]) { bytes.push(byte); }
        for x in [n.start, n.count, n.ns_start, n.ns_count, n.metadata, n.origin, n.source, n.span_start, n.span_end] { put_word(bytes, x); }
    }
    for id in g.children { put_word(bytes, id); }
    for u in g.units { bytes.push((u & 255) as u8); bytes.push((u >> 8) as u8); }
    for s in g.sources { put_word(bytes, s.start); put_word(bytes, s.count); }
    bytes.append(g.bytes); for id in g.roots { put_word(bytes, id); }
    out.append(bytes); return true;
}
// A VM node's provenance row as a graph source: -1 when it has none.
fn graph_source(row: u32) -> i64 { return if row == source::no_row() { -1 } else { row }; }
fn import_graph(heap: source::Heap&, g: const Graph&, roots: source::Value[>..]&, c: Control&) -> bool {
    if !validate(g, c) { return false; }
    // Each string occurrence is copied into its own canonical run (Latin-1 or
    // UTF-16), so work counts the units copied, not the shared input table.
    var text: i64 = 0;
    for n in g.nodes { if n.tag == 3 || n.tag == 4 || n.tag == 5 { text += n.count; } if (n.tag == 4 || n.tag == 5) && n.ns_count >= 0 { text += n.ns_count; } }
    let work = g.nodes.len + g.children.len + max(g.units.len, text) + g.bytes.len + g.sources.len + g.roots.len;
    // Final cancellation and capacity checks precede any session installation.
    if !poll(c, work) { return false; }
    if !source::nodes_fit(heap, g.nodes.len) || !source::values_fit(heap, g.children.len) || !source::units_fit(heap, text) || 2 * g.nodes.len > heap.limits.units - (heap.strings.len - heap.image.strings) || g.bytes.len > c.bytes - heap.source_bytes.len { return reject(c, Error.Heap); }
    if !source::charge(heap, work) { heap.fault = source::Fault.None; return reject(c, Error.Heap); }
    // The preflight above reserved every row, so no allocation below fails.
    let sources = heap.source_ranges.len;
    let bytes = source::add_source_bytes(heap, g.bytes);
    for s in g.sources { source::add_source_range(heap, source::SourceRange { start: (bytes + s.start) as u32, count: s.count }); }
    let base = source::add_nodes(heap, [], g.nodes.len, source::Node { kind: 11 });
    var links: source::Value[>..] = []; for id in g.children { links.push(source::object(base + id)); }
    let children = source::add_values(heap, links);
    for n, i in g.nodes {
        var node = source::Node { kind: 11, origin: n.origin, source: if n.source < 0 { source::no_row() } else { (sources + n.source) as u32 }, span_start: n.span_start as u32, span_end: n.span_end as u32 };
        if n.metadata >= 0 { node.metadata = source::object(base + n.metadata); }
        if n.tag >= 6 { node.kind = n.tag - 1; node.start = (children + n.start) as u32; node.count = n.count as u32; }
        else if n.tag == 4 || n.tag == 5 {
            node.kind = n.tag + 5; node.value = source::import_text(heap, g.units[n.start..(n.start + n.count)]);
            if n.ns_count >= 0 { node.namespace_value = source::import_text(heap, g.units[n.ns_start..(n.ns_start + n.ns_count)]); }
        } else { node.value = if n.tag == 0 { source::nil() } else if n.tag == 1 { source::boolean(n.number != 0) } else if n.tag == 2 { source::number(n.number) } else { source::import_text(heap, g.units[n.start..(n.start + n.count)]) }; }
        source::set_node(heap, base + i, node);
    }
    for id in g.roots { roots.push(source::object(base + id)); }
    return true;
}
// Export first discovers reachable heap nodes (including metadata) into a
// private graph, then validates and commits the complete independently owned
// result. Session teardown cannot invalidate any byte/child/source in out.
fn export_graph(heap: source::Heap&, roots: const source::Value[:], out: Graph&, c: Control&, call: source::Value = source::Value.Undefined) -> bool {
    var g = Graph {}; var ids: i64[>..] = [];
    if !poll(c, heap.nodes.len + heap.source_bytes.len + heap.source_ranges.len) { return false; }
    if heap.source_bytes.len > c.bytes { return reject(c, Error.Bytes); }
    ids.resize(heap.nodes.len, -1); g.bytes.append(heap.source_bytes); g.sources.append(heap.source_ranges);
    // Match the compiler's current primitive-result compatibility contract:
    // a newly computed bare primitive can recover the latest argument's span.
    // Explicit occurrence handles bypass this fallback and remain distinct.
    var origins:        source::Value[>..] = []; var pending: Retain[>..<] = [];
    var retained_nodes: i64                = 0; var retained_bytes: i64 = heap.source_bytes.len;
    if call.tag() != 0 { pending.push(Retain { value: call, depth: 1 }); }
    while pending.len > 0 {
        if !poll(c) { return false; }
        let retained = pending.pop(); let v = retained.value;
        retained_nodes += 1; retained_bytes += 48;
        if retained_nodes > c.nodes { return reject(c, Error.Nodes); }
        if retained_bytes > c.bytes { return reject(c, Error.Bytes); }
        if retained.depth > c.depth { return reject(c, Error.Depth); }
        if !source::valid(heap, v) { heap.fault = source::Fault.None; return reject(c, Error.Heap); }
        if v.tag() != 5 { continue; }
        let n = source::node(heap, v.id());
        if n.kind == 11 && n.value.tag() == 4 { retained_bytes += source::str_len(heap, n.value) * 2; }
        if n.kind == 9 || n.kind == 10 { retained_bytes += source::str_len(heap, n.value) * 2 + source::str_len(heap, n.namespace_value) * 2; }
        if n.kind >= 5 && n.kind <= 8 || n.kind == 12 { retained_bytes += n.count * 4; }
        if retained_bytes > c.bytes { return reject(c, Error.Bytes); }
        if n.kind == 11 && n.origin >= 0 && retained.argument { origins.push(v); }
        if n.metadata.tag() != 0 { pending.push(Retain { value: n.metadata, depth: retained.depth + 1, argument: retained.argument }); }
        if n.kind >= 5 && n.kind <= 8 || n.kind == 12 {
            if !range(n.start, n.count, heap.values.len) || !poll(c, n.count) { return reject(c); }
            for i in n.count { let at = n.count - i - 1; pending.push(Retain { value: source::slot(heap, n.start + at), depth: retained.depth + 1, argument: retained.argument || retained.depth == 1 && at >= 1 }); }
        }
    }
    for v in roots { g.roots.push(-1); }
    // Queue entries are edges, with target slots naming roots or a Form field.
    var queue: Edge[>..] = [];
    for v, i in roots { queue.push(Edge { value: v, parent: -1, field: i }); }
    var cursor: i64 = 0;
    while cursor < queue.len {
        if !poll(c) { return false; }
        let edge = queue[cursor]; cursor += 1; let v = edge.value;
        if !source::valid(heap, v) { heap.fault = source::Fault.None; return reject(c, Error.Heap); }
        var id: i64 = if v.tag() == 5 { ids[v.id()] } else { -1 };
        if id < 0 {
            if g.nodes.len >= c.nodes { return reject(c, Error.Nodes); }
            id = g.nodes.len; if v.tag() == 5 { ids[v.id()] = id; }
            var n = Form {}; var node = source::Node { kind: 11, value: v };
            if v.tag() == 5 { node = source::node(heap, v.id()); }
            else {
                for original in origins {
                    if !poll(c) { return false; }
                    let candidate = source::node(heap, original.id()); let x = candidate.value;
                    let same = scalar_equal(heap, x, v, c);
                    if c.error != Error.None { return false; }
                    if same { node = candidate; }
                }
            }
            var x = if node.kind == 11 { node.value } else { v };
            if node.kind >= 5 && node.kind <= 8 || node.kind == 12 {
                n.tag = if node.kind == 12 { 6 } else { node.kind + 1 }; n.start = g.children.len; n.count = node.count;
                if !poll(c, node.count) { return false; }
                if !range(node.start, node.count, heap.values.len) { return reject(c); }
                for i in node.count { let field = g.children.len; g.children.push(-1); queue.push(Edge { value: source::slot(heap, node.start + i), parent: id, field: field }); }
            } else {
                if node.kind == 9 || node.kind == 10 { n.tag = node.kind - 5; x = node.value; }
                else { n.tag = if x.tag() == 1 { 0 } else if x.tag() == 2 { 1 } else if x.tag() == 3 { 2 } else if x.tag() == 4 { 3 } else { -1 }; }
                if n.tag < 0 || !source::valid(heap, x) { heap.fault = source::Fault.None; return reject(c, Error.Unsupported); }
                n.number = if x.tag() == 2 { if x.flag() { 1.0 } else { 0.0 } } else { x.num() };
                if x.tag() == 4 { let count = source::str_len(heap, x); if !poll(c, count) { return false; } n.start = g.units.len; n.count = count; source::append_utf16(heap, x, g.units); }
                if n.tag == 4 || n.tag == 5 { let ns = node.namespace_value; if ns.tag() == 4 { if !source::valid(heap, ns) || !poll(c, source::str_len(heap, ns)) { return reject(c); } n.ns_start = g.units.len; n.ns_count = source::str_len(heap, ns); source::append_utf16(heap, ns, g.units); } }
            }
            n.origin = node.origin; n.source = graph_source(node.source); n.span_start = node.span_start; n.span_end = node.span_end;
            if n.origin == -1 && call.tag() == 5 && source::valid(heap, call) { let context = source::node(heap, call.id()); n.source = graph_source(context.source); n.span_start = context.span_start; n.span_end = context.span_end; }
            g.nodes.push(n);
            if node.metadata.tag() != 0 { queue.push(Edge { value: node.metadata, parent: id, field: -1 }); }
            if footprint(g) > c.bytes { return reject(c, Error.Bytes); }
        }
        if edge.parent < 0 { g.roots[edge.field] = id; } else if edge.field < 0 { g.nodes[edge.parent].metadata = id; } else { g.children[edge.field] = id; }
    }
    if !validate(g, c) { return false; }
    append_graph(out, g); return true;
}
