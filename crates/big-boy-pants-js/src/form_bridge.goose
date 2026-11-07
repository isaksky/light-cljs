import std;
import binary;
import .source_vm;
import .snapshot;
import .program;

namespace bridge;

// GFB2: tagged structured graphs, never printed forms. Child and metadata edges
// name occurrences; repeated IDs preserve aliases, equal nodes are not interned.
// Tags: form_tag::Nil .. form_tag::Set (below).
// A source entry is three words, identity, start and count
// (source::SourceRange): the compiler sends its identity for each source and
// the source's byte length, no text, and gets the same identities back
// (light-cljs-817); a source with identity -1 carries its text in `bytes`
// (Goose's own fixtures). GFB1, which carried every source's text, is refused.
struct Form { tag: i64 = form_tag::Nil, number: f64 = 0, start: i64 = 0, count: i64 = 0, ns_start: i64 = 0, ns_count: i64 = -1, metadata: i64 = -1, origin: i64 = -1, source: i64 = -1, span_start: i64 = 0, span_end: i64 = 0 }
// The omission record (light-cljs-820): what the entries the compiler left
// out of a narrowed environment (light-cljs-815) would have added to an
// argument graph, which version 3 of the encoding carries after the six
// counts, six words in this order: their nodes, child value rows, text
// units, string rows (one a string, one or two a symbol or keyword), the
// units validate reads in their symbols and keywords (the namespace's if
// any, otherwise the name's), and the deepest depth among them, the root's
// nodes at 1. Every omitted node has one parent edge, a child slot or a
// metadata edge, so the omission adds `nodes - children` metadata edges.
// validate and import_graph check, poll and charge it as if the entries
// were present, and import_graph reserves its rows for the rest of the job
// (source::reserve_rows). A graph without one has nodes 0.
struct Omitted { nodes: i64 = 0, children: i64 = 0, units: i64 = 0, strings: i64 = 0, names: i64 = 0, depth: i64 = 0 }
root struct Graph { nodes: Form[>..] = [], children: i64[>..] = [], units: u16[>..] = [], sources: source::SourceRange[>..] = [], bytes: u8[>..] = [], roots: i64[>..] = [], omitted: Omitted = Omitted {} }
enum Error { None, Malformed, Nodes, Depth, Bytes, Work, Cancelled, Unsupported, Heap }
// cells: the graph may hold cell references, form_tag::Cell (number the cell,
// count its flavor; light-cljs-799, cells.goose): a cell's value or a
// produced helper, which stay inside the Goose session. A job's result for
// the compiler never does, so a cell there is Unsupported, as the V8 build's
// form bridge refuses an atom. spans: an export carries each form's source
// span and the sources its forms name (an identity, or a fixture's text); a
// cell value, which outlives its call, carries neither (light-cljs-809), so a
// store exports only what the value reaches. reserved: the nodes of the
// omissions this control imported (light-cljs-820), which an export with
// spans polls as heap rows, as it would the imported entries; a job's
// control lasts as long as the job, through a saved continuation too.
struct Control { nodes: i64 = 100000, depth: i64 = 128, bytes: i64 = 16777216, work: i64 = 10000000, cancel_after: i64 = -1, visits: i64 = 0, error: Error = Error.None, cancelled: bool = false, cells: bool = false, spans: bool = true, reserved: i64 = 0 }
// GFB2's node tags, a node's first word. compiler_bridge.rs names the same
// numbers (form_tag::NIL ..), and its form_tags_match_the_goose_names test
// keeps the two equal. A cell reference (Cell) is Goose-only.
let form_tag::Nil = 0;
let form_tag::Boolean = 1;
let form_tag::Number = 2;
let form_tag::String = 3;
let form_tag::Symbol = 4;
let form_tag::Keyword = 5;
let form_tag::List = 6;
let form_tag::Vector = 7;
let form_tag::Map = 8;
let form_tag::Set = 9;
let form_tag::Cell = 10;
// List, vector, map or set: a node with children.
inline fn collection(tag: i64) -> bool { return tag >= form_tag::List && tag <= form_tag::Set; }
// String, symbol or keyword: a node with text.
inline fn textual(tag: i64) -> bool { return tag >= form_tag::String && tag <= form_tag::Keyword; }
// Symbol or keyword: a name with an optional namespace.
inline fn named(tag: i64) -> bool { return tag == form_tag::Symbol || tag == form_tag::Keyword; }
// The heap node kind of a collection, symbol or keyword tag (source::Node:
// node_kind::List .. node_kind::Keyword), and the tag of such a node kind,
// where node_kind::Concat (a sequence form_concat or a rest builds) exports as a list.
inline fn heap_kind(tag: i64) -> i64 {
    return match tag { form_tag::List => node_kind::List, form_tag::Vector => node_kind::Vector, form_tag::Map => node_kind::Map, form_tag::Set => node_kind::Set, form_tag::Symbol => node_kind::Symbol, _ => node_kind::Keyword };
}
inline fn tag_of(kind: i64) -> i64 {
    return match kind { node_kind::Vector => form_tag::Vector, node_kind::Map => form_tag::Map, node_kind::Set => form_tag::Set, node_kind::Symbol => form_tag::Symbol, node_kind::Keyword => form_tag::Keyword, _ => form_tag::List };
}
fn poll(c: Control&, count: i64 = 1) -> bool {
    if c.error != Error.None { return false; }
    if c.cancelled { c.error = Error.Cancelled; return false; }
    if count < 0 || count > c.work - c.visits { c.error = Error.Work; return false; }
    if c.cancel_after >= 0 && count > c.cancel_after - c.visits { c.error = Error.Cancelled; return false; }
    c.visits += count; return true;
}
fn reject(c: Control&, error: Error = Error.Malformed) -> bool { if c.error == Error.None { c.error = error; } return false; }
fn range(start: i64, count: i64, len: i64) -> bool { return start >= 0 && count >= 0 && start <= len && count <= len - start; }
// The graph's encoded size, with the rows its omission would add (not the
// record's own words): the size of the graph the omission narrowed.
fn footprint(g: const Graph&) -> i64 { let o = g.omitted; return 32 + (g.nodes.len + o.nodes) * 48 + (g.children.len + o.children) * 4 + (g.units.len + o.units) * 2 + g.sources.len * 12 + g.bytes.len + g.roots.len * 4; }
// Whether omission `o` can describe entries left out of a graph of `nodes`
// nodes: at least one node, at most one child slot and two string rows a
// node, name units among its units, a string row for any unit, and a depth
// of at least one kept ancestor and one omitted node, at most every node
// on one path. A cell value or helper (c.cells) is never narrowed.
fn omission_ok(o: Omitted, nodes: i64, c: const Control&) -> bool {
    return !c.cells && o.nodes > 0 && o.children >= 0 && o.children <= o.nodes && o.units >= 0 && o.strings >= 0 && o.strings <= 2 * o.nodes && o.names >= 0 && o.names <= o.units && (o.units == 0 || o.strings > 0) && o.depth >= 2 && o.depth <= nodes + o.nodes;
}
struct Edge { value: source::Value, parent: i64, field: i64 }
struct Retain { value: source::Value, depth: i64, argument: bool = false }
fn scalar_equal(heap: source::Heap&, a: source::Value, b: source::Value, c: Control&) -> bool {
    if !source::valid(heap, a) || !source::valid(heap, b) { heap.fault = source::Fault.None; reject(c, Error.Heap); return false; }
    if a.tag() != b.tag() { return false; }
    match a {
        Undefined, Null => { return true; }, Bool => { return a.flag() == b.flag(); },
        Number => { return a.num() == b.num() && (a.num() != 0 || 1.0 / a.num() == 1.0 / b.num()) || a.num() != a.num() && b.num() != b.num(); },
        String => { if !poll(c, source::str_len(heap, a) + source::str_len(heap, b)) { return false; } return source::same_text(heap, a, b); },
        _ => { return false; },
    }
}
struct Walk { id: i64, edge: i64 = -1 }
// Validate every node, including unreachable metadata and shared subgraphs.
// Explicit postorder computes longest path without an unbudgeted retain walk.
// An omission (Omitted) counts as the nodes it left out would: in the
// limits, and in the polls each of its nodes, child slots, units and name
// units would take, which follow the graph's own.
fn validate(g: const Graph&, c: Control&) -> bool {
    let o = g.omitted;
    if o.nodes != 0 && !omission_ok(o, g.nodes.len, c) { return reject(c); }
    if g.nodes.len + o.nodes > c.nodes { return reject(c, Error.Nodes); }
    if footprint(g) > c.bytes { return reject(c, Error.Bytes); }
    var states: i64[>..] = []; var heights: i64[>..] = [];
    if !poll(c, g.nodes.len + o.nodes + g.children.len + o.children + g.units.len + o.units + g.sources.len + g.bytes.len + g.roots.len) { return false; }
    states.resize(g.nodes.len, 0); heights.resize(g.nodes.len, 0);
    for s in g.sources {
        if !source::source_row_ok(s, g.bytes.len) { return reject(c); }
        if source::has_text(s) {
            if !poll(c, s.count) { return false; }
            if !snapshot::utf8(g.bytes[s.start..(s.start + s.count)]) { return reject(c); }
        }
    }
    for id in g.roots { if id < 0 || id >= g.nodes.len { return reject(c); } }
    for n in g.nodes {
        if !poll(c) { return false; }
        if n.tag == form_tag::Cell && (!c.cells || n.number < 0 || n.number > 2147483647 || n.number != (n.number as i64) || n.count < 0 || n.count > 2 || n.metadata >= 0) { return reject(c, Error.Unsupported); }
        if n.tag < form_tag::Nil || n.tag > form_tag::Cell || n.metadata < -1 || n.metadata >= g.nodes.len || n.source < -1 || n.source >= g.sources.len || n.origin < -1 { return reject(c); }
        if n.source >= 0 && !range(n.span_start, n.span_end - n.span_start, g.sources[n.source].count) { return reject(c); }
        // An identity source has no text here: the compiler checks those
        // spans' UTF-8 boundaries against its own source.
        if n.source >= 0 && source::has_text(g.sources[n.source]) {
            let s = g.sources[n.source]; let text = g.bytes[s.start..(s.start + s.count)];
            if !program::boundary(text, n.span_start) || !program::boundary(text, n.span_end) { return reject(c); }
        }
        if n.tag == form_tag::Boolean && n.number != 0 && n.number != 1 { return reject(c); }
        if textual(n.tag) && !range(n.start, n.count, g.units.len) { return reject(c); }
        if named(n.tag) && (n.ns_count < -1 || n.ns_count != -1 && !range(n.ns_start, n.ns_count, g.units.len)) { return reject(c); }
        if named(n.tag) {
            if n.ns_count >= 0 {
                if n.ns_count == 0 && n.count == 0 { return reject(c, Error.Unsupported); }
                for i in n.ns_count { if !poll(c) { return false; } if g.units[n.ns_start + i] == 47 { return reject(c, Error.Unsupported); } }
            } else { for i in n.count { if !poll(c) { return false; } if g.units[n.start + i] == 47 && n.count != 1 { return reject(c, Error.Unsupported); } } }
        }
        if n.metadata >= 0 && g.nodes[n.metadata].tag != form_tag::Map { return reject(c, Error.Unsupported); }
        if collection(n.tag) && (!range(n.start, n.count, g.children.len) || n.tag == form_tag::Map && n.count % 2 != 0) { return reject(c); }
    }
    // The omitted nodes' own polls and their names' units.
    if !poll(c, o.nodes + o.names) { return false; }
    for id in g.children { if id < 0 || id >= g.nodes.len { return reject(c); } }
    for root in g.nodes.len {
        if states[root] == 2 { continue; }
        var stack: Walk[>..<] = [Walk { id: root }];
        while stack.len > 0 {
            if !poll(c) { return false; }
            let w = stack[stack.len - 1]; let n = g.nodes[w.id]; states[w.id] = 1;
            let edge_count = (if collection(n.tag) { n.count } else { 0 }) + (if n.metadata >= 0 { 1 } else { 0 });
            let next = w.edge + 1;
            if next < edge_count {
                stack[stack.len - 1].edge = next;
                let child = if next == 0 && n.metadata >= 0 { n.metadata } else { g.children[n.start + next - (if n.metadata >= 0 { 1 } else { 0 })] };
                if states[child] == 1 { return reject(c); }
                if states[child] == 0 { if stack.len >= c.depth { return reject(c, Error.Depth); } stack.push(Walk { id: child }); }
            } else {
                var height: i64 = 1;
                if n.metadata >= 0 { height = max(height, 1 + heights[n.metadata]); }
                if collection(n.tag) { for i in n.count { if !poll(c) { return false; } height = max(height, 1 + heights[g.children[n.start + i]]); } }
                if height > c.depth { return reject(c, Error.Depth); }
                heights[w.id] = height; states[w.id] = 2; stack.pop();
            }
        }
    }
    // The walk's steps over the omitted nodes, two each (its own and its
    // parent's edge to it) and one a child slot, and their deepest path.
    if !poll(c, 2 * o.nodes + o.children) { return false; }
    if o.depth > c.depth { return reject(c, Error.Depth); }
    return true;
}
fn word(bytes: const u8[:], at: i64) -> i64 { let n, ok = binary::read_u32le(bytes, at); assert(ok); return n; }
fn signed(bytes: const u8[:], at: i64) -> i64 { let n = word(bytes, at); return if n >= 2147483648 { n - 4294967296 } else { n }; }
fn append_graph(out: Graph&, g: const Graph&) {
    assert(out.nodes.len == 0 && out.children.len == 0 && out.units.len == 0 && out.sources.len == 0 && out.bytes.len == 0 && out.roots.len == 0 && out.omitted.nodes == 0);
    out.nodes.append(g.nodes); out.children.append(g.children); out.units.append(g.units); out.sources.append(g.sources); out.bytes.append(g.bytes); out.roots.append(g.roots); out.omitted = g.omitted;
}
// Version 2, or version 3: version 2 with an omission record (Omitted)
// after the counts, whose rows count in the byte limit and polls as the
// graph's own would.
fn decode(out: Graph&, bytes: const u8[:], c: Control&) -> bool {
    let extended = bytes.len >= 56 && bytes[..4] == "GFB2" && word(bytes, 4) == 3;
    var o = Omitted {}; let header: i64 = if extended { 56 } else { 32 };
    if extended { o = Omitted { nodes: word(bytes, 32), children: word(bytes, 36), units: word(bytes, 40), strings: word(bytes, 44), names: word(bytes, 48), depth: word(bytes, 52) }; }
    let whole = bytes.len - header + 32 + o.nodes * 48 + o.children * 4 + o.units * 2;
    if whole > c.bytes { return reject(c, Error.Bytes); }
    if !poll(c, whole) { return false; }
    if bytes.len < 32 || bytes[..4] != "GFB2" || (word(bytes, 4) != 2 && !extended) { return reject(c); }
    let nodes = word(bytes, 8); let children = word(bytes, 12); let units = word(bytes, 16); let sources = word(bytes, 20); let text = word(bytes, 24); let roots = word(bytes, 28);
    if extended && !omission_ok(o, nodes, c) { return reject(c); }
    if nodes + o.nodes > c.nodes { return reject(c, Error.Nodes); }
    if header + nodes * 48 + children * 4 + units * 2 + sources * 12 + text + roots * 4 != bytes.len { return reject(c); }
    var g = Graph { omitted: o }; var at: i64 = header;
    for i in nodes {
        if !poll(c) { return false; }
        let number, ok = binary::read_f64le(bytes, at + 4); assert(ok);
        g.nodes.push(Form { tag: word(bytes, at), number: number, start: signed(bytes, at + 12), count: signed(bytes, at + 16), ns_start: signed(bytes, at + 20), ns_count: signed(bytes, at + 24), metadata: signed(bytes, at + 28), origin: signed(bytes, at + 32), source: signed(bytes, at + 36), span_start: signed(bytes, at + 40), span_end: signed(bytes, at + 44) }); at += 48;
    }
    for i in children { if !poll(c) { return false; } g.children.push(word(bytes, at)); at += 4; }
    for i in units { if !poll(c) { return false; } let u, ok = binary::read_u16le(bytes, at); assert(ok); g.units.push(u); at += 2; }
    for i in sources { if !poll(c) { return false; } g.sources.push(source::SourceRange { start: word(bytes, at + 4) as u32, count: word(bytes, at + 8) as u32, identity: signed(bytes, at) }); at += 12; }
    for i in text { if !poll(c) { return false; } g.bytes.push(bytes[at]); at += 1; }
    for i in roots { if !poll(c) { return false; } g.roots.push(word(bytes, at)); at += 4; }
    // The omitted nodes', child slots' and units' reads.
    if !poll(c, o.nodes + o.children + o.units) { return false; }
    if !validate(g, c) { return false; } append_graph(out, g); return true;
}
fn put_word(out: u8[>..]&, n: i64) { for i in 4 { out.push(((n >> (i * 8)) & 255) as u8); } }
fn encode(g: const Graph&, out: u8[>..]&, c: Control&) -> bool {
    if !validate(g, c) { return false; }
    if !poll(c, footprint(g)) { return false; }
    let o = g.omitted; var bytes: u8[>..] = []; bytes.append("GFB2"); put_word(bytes, if o.nodes > 0 { 3 } else { 2 });
    for n in [g.nodes.len, g.children.len, g.units.len, g.sources.len, g.bytes.len, g.roots.len] { put_word(bytes, n); }
    if o.nodes > 0 { for n in [o.nodes, o.children, o.units, o.strings, o.names, o.depth] { put_word(bytes, n); } }
    for n in g.nodes {
        put_word(bytes, n.tag); for byte in bytes_of([n.number]) { bytes.push(byte); }
        for x in [n.start, n.count, n.ns_start, n.ns_count, n.metadata, n.origin, n.source, n.span_start, n.span_end] { put_word(bytes, x); }
    }
    for id in g.children { put_word(bytes, id); }
    for u in g.units { bytes.push((u & 255) as u8); bytes.push((u >> 8) as u8); }
    for s in g.sources { put_word(bytes, s.identity); put_word(bytes, s.start); put_word(bytes, s.count); }
    bytes.append(g.bytes); for id in g.roots { put_word(bytes, id); }
    out.append(bytes); return true;
}
// A VM node's provenance row as a graph source of `g`: -1 when it has none.
// A row's first use adds its source (its identity, or a fixture's text), so
// an export carries only the sources its forms name, not every row the heap
// holds (an installed helper's, say). `ids` maps heap rows to graph sources.
fn graph_source(heap: const source::Heap&, row: u32, g: Graph&, ids: i64[>..]&, c: Control&) -> i64 {
    if row == source::no_row() { return -1; }
    if row >= ids.len { reject(c); return -1; }
    if ids[row] < 0 {
        let r = heap.source_ranges[row];
        if !source::source_row_ok(r, heap.source_bytes.len) { reject(c); return -1; }
        // The first text source copies the heap's source text, which text
        // rows share at their own offsets.
        if source::has_text(r) && g.bytes.len < heap.source_bytes.len {
            if !poll(c, heap.source_bytes.len) { return -1; }
            if heap.source_bytes.len > c.bytes { reject(c, Error.Bytes); return -1; }
            g.bytes.append(heap.source_bytes);
        }
        g.sources.push(r);
        ids[row] = g.sources.len - 1;
    }
    return ids[row];
}
fn import_graph(heap: source::Heap&, g: const Graph&, roots: source::Value[>..]&, c: Control&) -> bool {
    if !validate(g, c) { return false; }
    // Each string occurrence is copied into its own canonical run (Latin-1 or
    // UTF-16), so work counts the units copied, not the shared input table.
    // An omission's rows (Omitted) count as the graph's own would.
    let o = g.omitted; var text: i64 = o.units;
    for n in g.nodes { if textual(n.tag) { text += n.count; } if named(n.tag) && n.ns_count >= 0 { text += n.ns_count; } }
    let nodes = g.nodes.len + o.nodes; let children = g.children.len + o.children;
    let work = nodes + children + max(g.units.len + o.units, text) + g.bytes.len + g.sources.len + g.roots.len;
    // Final cancellation and capacity checks precede any session installation.
    if !poll(c, work) { return false; }
    if !source::nodes_fit(heap, nodes) || !source::values_fit(heap, children) || !source::units_fit(heap, text) || 2 * nodes > heap.limits.units - (heap.strings.len - heap.image.strings) || g.bytes.len > c.bytes - heap.source_bytes.len { return reject(c, Error.Heap); }
    if !source::charge(heap, work) { heap.fault = source::Fault.None; return reject(c, Error.Heap); }
    // The omitted rows are the job's for the rest of it, as if imported.
    source::reserve_rows(heap, o.nodes, o.children, o.units, o.strings); c.reserved += o.nodes;
    // The preflight above reserved every row, so no allocation below fails.
    let sources = heap.source_ranges.len;
    let bytes = source::add_source_bytes(heap, g.bytes);
    // An identity source copies no text: the row names the compiler's source.
    for s in g.sources { source::add_source_range(heap, source::shifted_source(s, bytes)); }
    let base = source::add_nodes(heap, [], g.nodes.len, source::Node { kind: node_kind::Scalar });
    var links: source::Value[>..] = []; for id in g.children { links.push(source::object(base + id)); }
    let children = source::add_values(heap, links);
    for n, i in g.nodes {
        var node = source::Node { kind: node_kind::Scalar, origin: n.origin, source: if n.source < 0 { source::no_row() } else { (sources + n.source) as u32 }, span_start: n.span_start as u32, span_end: n.span_end as u32 };
        if n.metadata >= 0 { node.metadata = source::object(base + n.metadata); }
        if collection(n.tag) { node.kind = heap_kind(n.tag); node.start = (children + n.start) as u32; node.count = n.count as u32; }
        else if n.tag == form_tag::Cell { node = source::Node { kind: node_kind::Cell, function: n.number as u32, length: n.count as u32 }; }
        else if named(n.tag) {
            node.kind = heap_kind(n.tag); node.value = source::import_text(heap, g.units[n.start..(n.start + n.count)]);
            if n.ns_count >= 0 { node.namespace_value = source::import_text(heap, g.units[n.ns_start..(n.ns_start + n.ns_count)]); }
        } else { node.value = if n.tag == form_tag::Nil { source::nil() } else if n.tag == form_tag::Boolean { source::boolean(n.number != 0) } else if n.tag == form_tag::Number { source::number(n.number) } else { source::import_text(heap, g.units[n.start..(n.start + n.count)]) }; }
        source::set_node(heap, base + i, node);
    }
    for id in g.roots { roots.push(source::object(base + id)); }
    return true;
}
// Export first discovers reachable heap nodes (including metadata) into a
// private graph, then validates and commits the complete independently owned
// result. Session teardown cannot invalidate any byte/child/source in out.
fn export_graph(heap: source::Heap&, roots: const source::Value[:], out: Graph&, c: Control&, call: source::Value = source::Value.Undefined) -> bool {
    var rows: i64[>..] = []; return export_rows(heap, roots, out, c, call, rows);
}
// export_graph, and the heap row each graph node came from (-1 for a
// primitive), for the identities of a cell value's nodes (cells.goose).
fn export_rows(heap: source::Heap&, roots: const source::Value[:], out: Graph&, c: Control&, call: source::Value, rows: i64[>..]&) -> bool {
    var g = Graph {}; var ids: i64[>..] = []; var source_ids: i64[>..] = []; rows.clear();
    if c.spans {
        if !poll(c, heap.nodes.len + c.reserved + heap.source_ranges.len) { return false; }
        source_ids.resize(heap.source_ranges.len, -1);
    }
    ids.resize(heap.nodes.len, -1);
    // Match the compiler's current primitive-result compatibility contract:
    // a newly computed bare primitive can recover the latest argument's span.
    // Explicit occurrence handles bypass this fallback and remain distinct.
    var origins:        source::Value[>..] = []; var pending: Retain[>..<] = [];
    var retained_nodes: i64                = 0; var retained_bytes: i64 = 0;
    if !call.is_undefined() { pending.push(Retain { value: call, depth: 1 }); }
    while pending.len > 0 {
        if !poll(c) { return false; }
        let retained = pending.pop(); let v = retained.value;
        retained_nodes += 1; retained_bytes += 48;
        if retained_nodes > c.nodes { return reject(c, Error.Nodes); }
        if retained_bytes > c.bytes { return reject(c, Error.Bytes); }
        if retained.depth > c.depth { return reject(c, Error.Depth); }
        if !source::valid(heap, v) { heap.fault = source::Fault.None; return reject(c, Error.Heap); }
        if !v.is_object() { continue; }
        let n = source::node(heap, v.id());
        if n.kind == node_kind::Scalar && n.value.is_string() { retained_bytes += source::str_len(heap, n.value) * 2; }
        if n.kind == node_kind::Symbol || n.kind == node_kind::Keyword { retained_bytes += source::str_len(heap, n.value) * 2 + source::str_len(heap, n.namespace_value) * 2; }
        if n.kind >= node_kind::List && n.kind <= node_kind::Set || n.kind == node_kind::Concat { retained_bytes += n.count * 4; }
        if retained_bytes > c.bytes { return reject(c, Error.Bytes); }
        if n.kind == node_kind::Scalar && n.origin >= 0 && retained.argument { origins.push(v); }
        if !n.metadata.is_undefined() { pending.push(Retain { value: n.metadata, depth: retained.depth + 1, argument: retained.argument }); }
        if n.kind >= node_kind::List && n.kind <= node_kind::Set || n.kind == node_kind::Concat {
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
        var id: i64 = if v.is_object() { ids[v.id()] } else { -1 };
        if id < 0 {
            if g.nodes.len >= c.nodes { return reject(c, Error.Nodes); }
            id = g.nodes.len; if v.is_object() { ids[v.id()] = id; } rows.push(if v.is_object() { v.id() } else { -1 });
            var n = Form {}; var node = source::Node { kind: node_kind::Scalar, value: v };
            if v.is_object() { node = source::node(heap, v.id()); }
            else {
                for original in origins {
                    if !poll(c) { return false; }
                    let candidate = source::node(heap, original.id()); let x = candidate.value;
                    let same = scalar_equal(heap, x, v, c);
                    if c.error != Error.None { return false; }
                    if same { node = candidate; }
                }
            }
            var x = if node.kind == node_kind::Scalar { node.value } else { v };
            if node.kind >= node_kind::List && node.kind <= node_kind::Set || node.kind == node_kind::Concat {
                n.tag = tag_of(node.kind); n.start = g.children.len; n.count = node.count;
                if !poll(c, node.count) { return false; }
                if !range(node.start, node.count, heap.values.len) { return reject(c); }
                for i in node.count { let field = g.children.len; g.children.push(-1); queue.push(Edge { value: source::slot(heap, node.start + i), parent: id, field: field }); }
            } else if node.kind == node_kind::Cell {
                // A cell reference, inside the session only (Control.cells).
                if !c.cells { return reject(c, Error.Unsupported); }
                n.tag = form_tag::Cell; n.number = node.function; n.count = node.length;
            } else {
                if node.kind == node_kind::Symbol || node.kind == node_kind::Keyword { n.tag = tag_of(node.kind); x = node.value; }
                else if x.is_null() { n.tag = form_tag::Nil; } else if x.is_bool() { n.tag = form_tag::Boolean; } else if x.is_number() { n.tag = form_tag::Number; } else if x.is_string() { n.tag = form_tag::String; }
                else { heap.fault = source::Fault.None; return reject(c, Error.Unsupported); }
                if !source::valid(heap, x) { heap.fault = source::Fault.None; return reject(c, Error.Unsupported); }
                n.number = if x.is_bool() { if x.flag() { 1.0 } else { 0.0 } } else { x.num() };
                if x.is_string() { let count = source::str_len(heap, x); if !poll(c, count) { return false; } n.start = g.units.len; n.count = count; source::append_utf16(heap, x, g.units); }
                if named(n.tag) { let ns = node.namespace_value; if ns.is_string() { if !source::valid(heap, ns) || !poll(c, source::str_len(heap, ns)) { return reject(c); } n.ns_start = g.units.len; n.ns_count = source::str_len(heap, ns); source::append_utf16(heap, ns, g.units); } }
            }
            n.origin = node.origin;
            if c.spans {
                // A new form takes the call's span.
                var span = node;
                if n.origin == -1 && call.is_object() && source::valid(heap, call) { span = source::node(heap, call.id()); }
                n.source = graph_source(heap, span.source, g, source_ids, c); n.span_start = span.span_start; n.span_end = span.span_end;
                if c.error != Error.None { return false; }
            }
            g.nodes.push(n);
            if !node.metadata.is_undefined() { queue.push(Edge { value: node.metadata, parent: id, field: -1 }); }
            if footprint(g) > c.bytes { return reject(c, Error.Bytes); }
        }
        if edge.parent < 0 { g.roots[edge.field] = id; } else if edge.field < 0 { g.nodes[edge.parent].metadata = id; } else { g.children[edge.field] = id; }
    }
    if !validate(g, c) { return false; }
    append_graph(out, g); return true;
}
