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
// A compiler literal (light-cljs-935): an explicit descriptor the compiler
// reads as its CompilerLiteral of kind `number`, a literal_tag::, whose
// payload is its one child (start and count a run of one). Its node in
// the session: a regex's (literal_tag::Regex, its payload the string
// `(?flags)source`, the V8 build's compilerFormBridge text, `v` left out)
// is a RegExp, a shareable regex value (regexp_object.goose) unless its
// flags hold `g` or `y`, whose lastIndex is the job's own. A RegExp in a
// result, shareable or not, goes back as one; a global or sticky one is
// refused. Never in a cell value or publication (Control.cells), which
// carries a RegExp by the callable transport, and with no metadata, which
// a RegExp does not keep. An instant's (literal_tag::Instant,
// light-cljs-963; its payload the Number of its time value, the V8 build's
// `dateTime(v)`) is a new Date (date.goose) of that time value TimeClipped,
// as the V8 build's `new Date(payload)`; any Date in a result goes back as
// one, by its [[DateValue]] alone (own properties and prototype play no
// part, as the V8 build's intrinsic getTime reads it), and an invalid one
// is refused, as the V8 build's "invalid instant". An imported Date keeps
// its compiler occurrence and the time value it came with
// (namespace_value); while its time value is that one it goes back as the
// compiler's form, otherwise as a new form, and so does each of the
// argument's collections that holds it (changed_occurrences), as the V8
// build's bridge compares an argument's final canonical data with what it
// sent. Never in a cell value or publication either: a Date is mutable
// (cells.goose refuses it as `date-unshareable` first). light-cljs-929
// adds the other descriptors.
let form_tag::Literal = 11;
// A compiler literal's descriptor (Form.number): compiler_bridge.rs names
// the same numbers (literal_tag::REGEX ..), and form_tags_match_the_goose_names
// keeps the two equal.
let literal_tag::Regex = 0;
let literal_tag::Instant = 1;
// The payload tag a known literal descriptor carries: a regex's text, an
// instant's time value; -1 for any other descriptor.
inline fn payload_tag(descriptor: f64) -> i64 {
    return if descriptor == literal_tag::Regex { form_tag::String } else if descriptor == literal_tag::Instant { form_tag::Number } else { -1 };
}
// List, vector, map or set: a node with children.
inline fn collection(tag: i64) -> bool { return tag >= form_tag::List && tag <= form_tag::Set; }
// A collection or a compiler literal: a node with child edges.
inline fn linked(tag: i64) -> bool { return collection(tag) || tag == form_tag::Literal; }
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
fn validate(g: const Graph&, c: Control&) -> bool { var sets: i64[>..] = []; return validate_graph(g, c, sets, false); }
// validate, and with `record` the graph's maps and sets in that postorder
// (each after every map or set it reaches), for import_graph's merge.
fn validate_graph(g: const Graph&, c: Control&, sets: i64[>..]&, record: bool) -> bool {
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
        if n.tag == form_tag::Literal && (c.cells || payload_tag(n.number) < 0 || n.metadata >= 0) { return reject(c, Error.Unsupported); }
        if n.tag == form_tag::Literal && (n.count != 1 || !range(n.start, n.count, g.children.len)) { return reject(c); }
        if n.tag < form_tag::Nil || n.tag > form_tag::Literal || n.metadata < -1 || n.metadata >= g.nodes.len || n.source < -1 || n.source >= g.sources.len || n.origin < -1 { return reject(c); }
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
        // A collection's number is 0, or inside the session 1 for a
        // hash-ordered map or set (map_order::Hash), a realized integer
        // range, a list (seq_flavor::IntegerRange; light-cljs-877), or a map
        // entry, a vector of two items (vector_flavor::MapEntry;
        // light-cljs-918), or 2 for the realm's empty collection of its kind
        // (realm_empty).
        if collection(n.tag) && n.number != 0 && !(n.number == 1 && c.cells && (n.tag != form_tag::Vector || n.count == 2)) && !(realm_empty(n) && c.cells) { return reject(c); }
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
            let edge_count = (if linked(n.tag) { n.count } else { 0 }) + (if n.metadata >= 0 { 1 } else { 0 });
            let next = w.edge + 1;
            if next < edge_count {
                stack[stack.len - 1].edge = next;
                let child = if next == 0 && n.metadata >= 0 { n.metadata } else { g.children[n.start + next - (if n.metadata >= 0 { 1 } else { 0 })] };
                if states[child] == 1 { return reject(c); }
                if states[child] == 0 { if stack.len >= c.depth { return reject(c, Error.Depth); } stack.push(Walk { id: child }); }
            } else {
                var height: i64 = 1;
                if n.metadata >= 0 { height = max(height, 1 + heights[n.metadata]); }
                if linked(n.tag) { for i in n.count { if !poll(c) { return false; } height = max(height, 1 + heights[g.children[n.start + i]]); } }
                if height > c.depth { return reject(c, Error.Depth); }
                // A regex literal's payload is a string, an instant's a number.
                if n.tag == form_tag::Literal && g.nodes[g.children[n.start]].tag != payload_tag(n.number) { return reject(c); }
                heights[w.id] = height; states[w.id] = 2; stack.pop();
                if record && (n.tag == form_tag::Map || n.tag == form_tag::Set) { sets.push(w.id); }
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
// The traversal order (map_order) of a compiler form's map: the V8 build's
// form bridge decodes a map with mapFromArray, a hash map past 16 operands
// (8 key positions, equal keys included), and a set with setFromArray,
// which promotes at its ninth distinct member (merge_keys;
// light-cljs-877, src/js_runtime.js decodeNode). A cell value's or
// publication's graph (Control.cells) carries the order itself
// (Form.number).
fn decoded_hash_ordered(n: Form) -> bool { return n.tag == form_tag::Map && n.count > 16; }
// A compiler graph's maps and sets merged as the V8 build's form bridge
// decodes them (merge_keys): a merged node's run is `kids[start[id]..]`,
// `count[id]` long (start -1 or empty arrays: its own run); `class` holds
// the collections' key classes once `known`.
root struct Merging { kids: i64[>..] = [], start: i64[>..] = [], count: i64[>..] = [], class: u64[>..] = [], known: bool[>..] = [] }
fn run_count(g: const Graph&, m: const Merging&, id: i64) -> i64 { return if m.start.len > 0 && m.start[id] >= 0 { m.count[id] } else { g.nodes[id].count }; }
fn run_child(g: const Graph&, m: const Merging&, id: i64, i: i64) -> i64 { return if m.start.len > 0 && m.start[id] >= 0 { m.kids[m.start[id] + i] } else { g.children[g.nodes[id].start + i] }; }
fn units_hash(units: const u16[:]) -> u64 { var h: u64 = 0; for u in units { h = (h * 31 + u) & 4294967295; } return h; }
// A node's key class: nodes equal by graph_equal (as form_equal would find
// them once imported) have equal classes, and most unequal ones differ:
// numbers by number_hash (1 and 1.0, 0 and -0 meet), text by its units,
// lists and vectors (equal to each other) by their children in order, maps
// and sets by their merged entries in any order, instants by their time
// values. A collection's class is kept, so a shared subgraph is hashed once;
// nesting is the graph's height, which validate bounds.
recursive fn key_class(g: const Graph&, m: Merging&, id: i64) -> u64 {
    let n = g.nodes[id];
    if n.tag == form_tag::Nil { return 1; }
    if n.tag == form_tag::Boolean { return if n.number != 0 { 2 } else { 3 }; }
    if n.tag == form_tag::Number { let h: u64 = source::number_hash(n.number); return (4 << 32) | h; }
    if n.tag == form_tag::Literal && n.number == literal_tag::Instant { let h: u64 = source::number_hash(instant_time(g, n)); return (12 << 32) | h; }
    if n.tag == form_tag::String { return (5 << 32) | units_hash(g.units[n.start..(n.start + n.count)]); }
    if named(n.tag) {
        let space: u64 = if n.ns_count < 0 { 4294967295 } else { units_hash(g.units[n.ns_start..(n.ns_start + n.ns_count)]) };
        let kind:  u64 = if n.tag == form_tag::Symbol { 6 } else { 7 };
        return (kind << 32) | ((units_hash(g.units[n.start..(n.start + n.count)]) * 31 + space) & 4294967295);
    }
    if !collection(n.tag) { return 8 << 32; }
    if m.known.len == 0 { m.known.resize(g.nodes.len, false); m.class.resize(g.nodes.len, 0); }
    if m.known[id] { return m.class[id]; }
    let count = run_count(g, m, id); var h: u64 = 0;
    if n.tag == form_tag::List || n.tag == form_tag::Vector {
        h = 9 << 32; for i in count { h = h * 1000003 + key_class(g, m, run_child(g, m, id, i)); }
    } else if n.tag == form_tag::Map {
        h = 10 << 32; var i: i64 = 0; while i + 1 < count { h += key_class(g, m, run_child(g, m, id, i)) * 31 + key_class(g, m, run_child(g, m, id, i + 1)); i += 2; }
    } else { h = 11 << 32; for i in count { h += key_class(g, m, run_child(g, m, id, i)); } }
    m.known[id] = true; m.class[id] = h; return h;
}
fn same_units(g: const Graph&, a_start: i64, a_count: i64, b_start: i64, b_count: i64) -> bool { return a_count == b_count && g.units[a_start..(a_start + a_count)] == g.units[b_start..(b_start + b_count)]; }
// Whether nodes a and b are equal as form_equal finds their imported forms
// (source_vm.goose): nil, booleans and numbers by value, strings, symbols
// and keywords by their text, lists and vectors by their children in
// order (a list equals a vector), maps and sets by their merged runs: the
// same count and each key, then its value, found in the other's, the first
// equal key's value; instants by the time values of the Dates they import
// as (an invalid one equals none), as structuralEqualPair compares two
// Dates' valueOf (light-cljs-963); two regexes never. Metadata does not
// count. A step is a poll.
recursive fn graph_equal(g: const Graph&, m: Merging&, a: i64, b: i64, c: Control&) -> bool {
    if !poll(c) { return false; }
    if a == b { return true; }
    let x = g.nodes[a]; let y = g.nodes[b];
    let sequential = (x.tag == form_tag::List || x.tag == form_tag::Vector) && (y.tag == form_tag::List || y.tag == form_tag::Vector);
    if x.tag != y.tag && !sequential { return false; }
    if x.tag == form_tag::Nil { return true; }
    if x.tag == form_tag::Boolean || x.tag == form_tag::Number { return x.number == y.number; }
    if x.tag == form_tag::String { return same_units(g, x.start, x.count, y.start, y.count); }
    if named(x.tag) { return same_units(g, x.start, x.count, y.start, y.count) && (x.ns_count < 0 && y.ns_count < 0 || x.ns_count >= 0 && y.ns_count >= 0 && same_units(g, x.ns_start, x.ns_count, y.ns_start, y.ns_count)); }
    if x.tag == form_tag::Literal { return x.number == literal_tag::Instant && y.number == literal_tag::Instant && instant_time(g, x) == instant_time(g, y); }
    if !collection(x.tag) { return false; }
    let count = run_count(g, m, a);
    if count != run_count(g, m, b) { return false; }
    if x.tag == form_tag::Map || x.tag == form_tag::Set {
        let stride: i64 = if x.tag == form_tag::Map { 2 } else { 1 };
        for i in (count / stride) {
            var found = false;
            for j in (count / stride) {
                let same = graph_equal(g, m, run_child(g, m, a, i * stride), run_child(g, m, b, j * stride), c);
                if c.error != Error.None { return false; }
                if same { found = stride == 1 || graph_equal(g, m, run_child(g, m, a, i * stride + 1), run_child(g, m, b, j * stride + 1), c); break; }
            }
            if c.error != Error.None || !found { return false; }
        }
        return true;
    }
    for i in count { if !graph_equal(g, m, run_child(g, m, a, i), run_child(g, m, b, i), c) { return false; } }
    return true;
}
struct Keyed { class: u64, index: i64 }
// Map or set `id`'s keys or members that are equal at run time although
// the reader kept them apart (1 and 1.0, [1] and '(1)) merge as the V8
// build's form bridge decodes them (mapFromArray, setFromArray;
// light-cljs-879): the first key with the last value, the first member,
// in their first positions. Keys are grouped by key_class and only keys of
// one class are compared. Before import_graph installs anything, inner
// maps and sets first (validate's postorder); false on a poll's failure.
fn merge_keys(g: const Graph&, m: Merging&, id: i64, c: Control&) -> bool {
    let stride:  i64 = if g.nodes[id].tag == form_tag::Map { 2 } else { 1 };
    let entries: i64 = g.nodes[id].count / stride;
    if entries < 2 { return true; }
    var keys: Keyed[>..] = [];
    for i in entries { keys.push(Keyed { class: key_class(g, m, run_child(g, m, id, i * stride)), index: i }); }
    sort(keys) { x, y => x.class < y.class || x.class == y.class && x.index < y.index };
    // first[i]: the entry whose key entry i merges into, itself if none.
    var first: i64[>..] = []; var merged: i64 = 0; var at: i64 = 0;
    while at < entries {
        var end = at + 1; while end < entries && keys[end].class == keys[at].class { end += 1; }
        if end - at > 1 {
            if first.len == 0 { for i in entries { first.push(i); } }
            for j in (at + 1)..end {
                for k in at..j {
                    let a = keys[k].index; let b = keys[j].index;
                    if first[a] != a { continue; }
                    let same = graph_equal(g, m, run_child(g, m, id, a * stride), run_child(g, m, id, b * stride), c);
                    if c.error != Error.None { return false; }
                    if same { first[b] = a; merged += 1; break; }
                }
            }
        }
        at = end;
    }
    if merged == 0 { return true; }
    var last: i64[>..] = []; last.resize(entries, -1); for i in entries { last[first[i]] = i; }
    if m.start.len == 0 { m.start.resize(g.nodes.len, -1); m.count.resize(g.nodes.len, 0); }
    let start = m.kids.len;
    for i in entries {
        if first[i] != i { continue; }
        m.kids.push(run_child(g, m, id, i * stride));
        if stride == 2 { m.kids.push(run_child(g, m, id, last[i] * 2 + 1)); }
    }
    m.start[id] = start; m.count[id] = m.kids.len - start;
    return true;
}
// Inside the session (Control.cells), the realm's empty list, vector, map
// or set (light-cljs-878; realm.goose empty_form_row), an empty literal's
// one object, goes as a collection of its kind with no children or
// metadata and number 2, and every link to it imports as a link to that
// realm row, so a publication or cell value keeps the literal's identity.
// Its own node still takes a row, unlinked, so the graph's nodes stay one
// row each.
fn realm_empty(n: Form) -> bool { return collection(n.tag) && n.number == 2 && n.count == 0 && n.metadata < 0; }
// The value a link to node `id` of an imported graph whose nodes start at
// row `base` names.
fn imported_link(heap: const source::Heap&, g: const Graph&, base: i64, id: i64, c: const Control&) -> source::Value {
    let n = g.nodes[id];
    if c.cells && realm_empty(n) && heap.global.is_object() { return source::object(source::empty_form_row(heap_kind(n.tag))); }
    return source::object(base + id);
}
// Where a regex literal's payload's source begins (the Pants engine's
// RegexCell::from_payload): past a `(?flags)` prefix whose letters are
// those re-pattern reads (dgimsuxy), its flags between, or 0 for a payload
// that is all source.
fn payload_split(text: const u16[:]) -> i64 {
    if text.len < 3 || text[0] != 40 || text[1] != 63 { return 0; }
    var at: i64 = 2;
    while at < text.len && prefix_flag(text[at]) { at += 1; }
    return if at < text.len && text[at] == 41 { at + 1 } else { 0 };
}
inline fn prefix_flag(u: u16) -> bool { return u == 100 || u == 103 || u == 105 || u == 109 || u == 115 || u == 117 || u == 120 || u == 121; }
// The time value of the Date that instant literal node n (validated)
// imports as: its payload's Number TimeClipped (date.goose time_clip), as
// the V8 build's `new Date(payload)` makes it (light-cljs-963).
fn instant_time(g: const Graph&, n: Form) -> f64 { return source::time_clip(g.nodes[g.children[n.start]].number); }
// The programs of graph g's regex literals (light-cljs-935), compiled into
// the job's code before import_graph installs a row: programs[i] the
// Code.patterns row of regex literal node i, -1 for any other node, and
// none at all for a graph without regex literals (an instant needs none). A
// pattern that does not compile (an `x` flag JavaScript lacks, an invalid
// pattern) refuses the graph as Unsupported, its fault cleared and its
// message in the job's detail. A malformed literal compiles nothing:
// validate refuses the graph.
fn compile_literals(job: source::Job&, g: const Graph&, programs: i64[>..]&, c: Control&) -> bool {
    programs.clear();
    var any = false;
    for n in g.nodes { if n.tag == form_tag::Literal && n.number == literal_tag::Regex { any = true; break; } }
    if !any { return true; }
    programs.resize(g.nodes.len, -1);
    for n, i in g.nodes {
        if n.tag != form_tag::Literal || n.number != literal_tag::Regex || n.count != 1 || !range(n.start, 1, g.children.len) || g.children[n.start] < 0 || g.children[n.start] >= g.nodes.len { continue; }
        let p = g.nodes[g.children[n.start]];
        if p.tag != form_tag::String || !range(p.start, p.count, g.units.len) { continue; }
        if !poll(c, p.count) { return false; }
        let text = g.units[p.start..(p.start + p.count)];
        let split = payload_split(text);
        var units: u16[>..] = [];
        if split > 0 { units.append(text[2..(split - 1)]); }
        let flags = units.len; units.append(text[split..]);
        let r = source::compile_units(job, units, flags);
        if r < 0 { job.heap.fault = source::Fault.None; return reject(c, Error.Unsupported); }
        programs[i] = r;
    }
    return true;
}
fn import_graph(heap: source::Heap&, g: const Graph&, roots: source::Value[>..]&, c: Control&) -> bool { let none: i64[0] = []; return import_graph(heap, g, roots, c, none[..]); }
// With `programs`, compile_literals' (light-cljs-935): each regex literal
// becomes a RegExp of its program, its pattern text the payload's source,
// shareable unless the program is global or sticky (form_tag::Literal); a
// graph with a regex literal and no program for it is refused. Each instant
// literal becomes a Date of its time value (instant_time, light-cljs-963).
fn import_graph(heap: source::Heap&, g: const Graph&, roots: source::Value[>..]&, c: Control&, programs: const i64[:]) -> bool {
    var sets: i64[>..] = [];
    if !validate_graph(g, c, sets, !c.cells) { return false; }
    // Each string occurrence is copied into its own canonical run (Latin-1 or
    // UTF-16), so work counts the units copied, not the shared input table.
    // An omission's rows (Omitted) count as the graph's own would. A regex
    // literal's pattern text is a string of its own, at most its payload's.
    let o = g.omitted; var text: i64 = o.units;
    for n, i in g.nodes {
        if textual(n.tag) { text += n.count; } if named(n.tag) && n.ns_count >= 0 { text += n.ns_count; }
        // A literal's object needs the realm's prototype; an instant adds no text.
        if n.tag == form_tag::Literal && !heap.global.is_object() { return reject(c, Error.Unsupported); }
        if n.tag == form_tag::Literal && n.number == literal_tag::Regex {
            if i >= programs.len || programs[i] < 0 || programs[i] >= heap.code.patterns.len { return reject(c, Error.Unsupported); }
            text += g.nodes[g.children[n.start]].count;
        }
    }
    let nodes = g.nodes.len + o.nodes; let children = g.children.len + o.children;
    let work = nodes + children + max(g.units.len + o.units, text) + g.bytes.len + g.sources.len + g.roots.len;
    // Final cancellation and capacity checks precede any session installation.
    if !poll(c, work) { return false; }
    // Maps and sets merge equal keys (merge_keys), inner ones first; a merged
    // run takes new value rows, preflighted with the rest.
    var m = Merging {};
    for id in sets { if !merge_keys(g, m, id, c) { return false; } }
    if !source::nodes_fit(heap, nodes) || !source::values_fit(heap, children + m.kids.len) || !source::units_fit(heap, text) || 2 * nodes > heap.limits.units - (heap.strings.len - heap.image.strings) || g.bytes.len > c.bytes - heap.source_bytes.len { return reject(c, Error.Heap); }
    if !source::charge(heap, work + m.kids.len) { heap.fault = source::Fault.None; return reject(c, Error.Heap); }
    // The omitted rows are the job's for the rest of it, as if imported.
    source::reserve_rows(heap, o.nodes, o.children, o.units, o.strings); c.reserved += o.nodes;
    // The preflight above reserved every row, so no allocation below fails.
    let sources = heap.source_ranges.len;
    let bytes = source::add_source_bytes(heap, g.bytes);
    // An identity source copies no text: the row names the compiler's source.
    for s in g.sources { source::add_source_range(heap, source::shifted_source(s, bytes)); }
    let base = source::add_nodes(heap, [], g.nodes.len, source::Node { kind: node_kind::Scalar });
    var links: source::Value[>..] = []; for id in g.children { links.push(imported_link(heap, g, base, id, c)); } for id in m.kids { links.push(imported_link(heap, g, base, id, c)); }
    let children = source::add_values(heap, links);
    for n, i in g.nodes {
        var node = source::Node { kind: node_kind::Scalar, origin: n.origin, source: if n.source < 0 { source::no_row() } else { (sources + n.source) as u32 }, span_start: n.span_start as u32, span_end: n.span_end as u32 };
        if n.metadata >= 0 { node.metadata = imported_link(heap, g, base, n.metadata, c); }
        if collection(n.tag) { node.kind = heap_kind(n.tag); node.start = (children + n.start) as u32; node.count = n.count as u32; if m.start.len > 0 && m.start[i] >= 0 { node.start = (children + g.children.len + m.start[i]) as u32; node.count = m.count[i] as u32; } if c.cells && n.number == 1 && n.tag == form_tag::List { node.kind = node_kind::Concat; node.length = seq_flavor::IntegerRange; } else if c.cells && n.number == 1 && n.tag == form_tag::Vector { node.length = vector_flavor::MapEntry; } else if c.cells && n.number == 1 || !c.cells && (decoded_hash_ordered(n) || n.tag == form_tag::Set && node.count > 8) { node.length = map_order::Hash; } }
        else if n.tag == form_tag::Cell { node = source::Node { kind: node_kind::Cell, function: n.number as u32, length: n.count as u32 }; }
        else if n.tag == form_tag::Literal && n.number == literal_tag::Instant {
            // An instant: a Date, as date.goose new_date makes one, its
            // compiler occurrence kept with the time value it names.
            node.kind = node_kind::Date; node.value = source::number(instant_time(g, n)); node.namespace_value = node.value; node.proto = source::object(source::date_prototype_row());
        }
        else if n.tag == form_tag::Literal {
            // A regex: a RegExp laid out as regexp_object.goose's
            // new_shared_regexp lays one out, its compiler occurrence kept.
            let p = g.nodes[g.children[n.start]]; let payload = g.units[p.start..(p.start + p.count)];
            let shared = !source::stateful_pattern(heap.code.patterns[programs[i]]);
            node.kind = node_kind::RegExp; node.function = programs[i] as u32; node.key_count = 1; node.value = source::import_text(heap, payload[payload_split(payload)..]);
            node.shape = source::node(heap, source::regexp_template_row()).shape; node.proto = source::object(source::regexp_prototype_row()); node.slots = [source::number(0), source::Value.Undefined];
            node.immutable = shared; node.non_extensible = shared;
        }
        else if named(n.tag) {
            node.kind = heap_kind(n.tag); node.value = source::import_text(heap, g.units[n.start..(n.start + n.count)]);
            if n.ns_count >= 0 { node.namespace_value = source::import_text(heap, g.units[n.ns_start..(n.ns_start + n.ns_count)]); }
        } else { node.value = if n.tag == form_tag::Nil { source::nil() } else if n.tag == form_tag::Boolean { source::boolean(n.number != 0) } else if n.tag == form_tag::Number { source::number(n.number) } else { source::import_text(heap, g.units[n.start..(n.start + n.count)]) }; }
        source::set_node(heap, base + i, node);
    }
    for id in g.roots { roots.push(imported_link(heap, g, base, id, c)); }
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
    var queue: Edge[>..] = []; var payload: u16[>..] = []; var new_instants = false;
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
                // A hash-ordered map or set keeps its order, an integer
                // range or a map entry its flavor, in a cell value or
                // publication; the
                // compiler's forms have neither.
                if c.cells && (source::hash_ordered(node) || source::integer_range(node) || node.kind == node_kind::Vector && node.length == vector_flavor::MapEntry) { n.number = 1; }
                if c.cells && v.is_object() && heap.global.is_object() && source::empty_form(v.id()) { n.number = 2; }
                if !poll(c, node.count) { return false; }
                if !range(node.start, node.count, heap.values.len) { return reject(c); }
                // To the compiler, a hash-ordered map's or set's children
                // go in its traversal order, as the V8 build's form bridge
                // walks its hash map (light-cljs-877). A key that does not
                // hash here could not cross anyway. A compiler occurrence
                // (origin) goes back as it came: the compiler reuses its form.
                if !c.cells && source::hash_ordered(node) && node.origin < 0 {
                    var order: i64[>..] = [];
                    if !source::traversal_order(heap, node, order) {
                        let failed = heap.fault != source::Fault.None; heap.fault = source::Fault.None;
                        return reject(c, if failed { Error.Heap } else { Error.Unsupported });
                    }
                    let stride: i64 = if node.kind == node_kind::Map { 2 } else { 1 };
                    for e in order { for j in stride { let field = g.children.len; g.children.push(-1); queue.push(Edge { value: source::slot(heap, node.start + e * stride + j), parent: id, field: field }); } }
                } else {
                    for i in node.count { let field = g.children.len; g.children.push(-1); queue.push(Edge { value: source::slot(heap, node.start + i), parent: id, field: field }); }
                }
            } else if node.kind == node_kind::Cell {
                // A cell reference, inside the session only (Control.cells).
                if !c.cells { return reject(c, Error.Unsupported); }
                n.tag = form_tag::Cell; n.number = node.function; n.count = node.length;
            } else if node.kind == node_kind::RegExp {
                // A RegExp, shareable or not (light-cljs-935): a regex
                // literal, its payload the V8 build's compilerFormBridge
                // text (regexp_object.goose regexp_payload), added after
                // it. A global or sticky one is refused, as is one in a
                // cell value or publication (form_tag::Literal).
                if c.cells || source::stateful_pattern(source::regexp_program(heap, v.id())) { return reject(c, Error.Unsupported); }
                n.tag = form_tag::Literal; n.number = literal_tag::Regex; n.start = g.children.len; n.count = 1; g.children.push(-1);
                payload.clear();
                if !source::regexp_payload(heap, v.id(), payload) { heap.fault = source::Fault.None; return reject(c, Error.Heap); }
                if !poll(c, payload.len) { return false; }
            } else if node.kind == node_kind::Date {
                // A Date (light-cljs-963): an instant literal, its payload
                // its time value, added after it. An invalid one (NaN) is
                // refused, as the V8 build's "invalid instant", as is one
                // in a cell value or publication (form_tag::Literal). An
                // imported one whose time value is no longer the one it
                // came with goes as a new instant (changed_occurrences).
                if c.cells || !node.value.is_number() || !is_finite(node.value.num()) { return reject(c, Error.Unsupported); }
                n.tag = form_tag::Literal; n.number = literal_tag::Instant; n.start = g.children.len; n.count = 1; g.children.push(-1);
                if node.origin >= 0 && !source::same_bits(node.value, node.namespace_value) { node.origin = -1; }
                if node.origin < 0 { new_instants = true; }
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
            // A regex literal's payload, a new string with the literal's
            // span; an instant's, a new number with it.
            if n.tag == form_tag::Literal {
                if g.nodes.len >= c.nodes { return reject(c, Error.Nodes); }
                g.children[n.start] = g.nodes.len; rows.push(-1);
                if n.number == literal_tag::Instant { g.nodes.push(Form { tag: form_tag::Number, number: node.value.num(), source: n.source, span_start: n.span_start, span_end: n.span_end }); }
                else {
                    g.nodes.push(Form { tag: form_tag::String, start: g.units.len, count: payload.len, source: n.source, span_start: n.span_start, span_end: n.span_end });
                    g.units.append(payload[..]);
                }
            }
            if !node.metadata.is_undefined() { queue.push(Edge { value: node.metadata, parent: id, field: -1 }); }
            if footprint(g) > c.bytes { return reject(c, Error.Bytes); }
        }
        if edge.parent < 0 { g.roots[edge.field] = id; } else if edge.field < 0 { g.nodes[edge.parent].metadata = id; } else { g.children[edge.field] = id; }
    }
    if new_instants && !changed_occurrences(heap, g, call, rows, source_ids, c) { return false; }
    if !validate(g, c) { return false; }
    append_graph(out, g); return true;
}
// A node of exported graph g with a compiler occurrence that reaches, by
// child or metadata edges, an instant without one goes back as a new form
// (light-cljs-963): an argument's collections are immutable, so that
// instant is a Date of the argument whose time value a setter changed
// (export_rows sent it without its occurrence), and the V8 build's bridge
// restores an argument only when its canonical data is unchanged.
// Such a node takes the call's span, and a hash-ordered map or set its
// traversal order, as any new form; the parts of it that reach no such
// instant keep their occurrences. `rows` is each node's heap row, and only
// a graph with a new instant runs this (export_rows). A fixed point: export
// numbers most children after their parents, so a pass from the last node
// settles most graphs, and one over a cycle, which validate refuses next,
// ends too.
fn changed_occurrences(heap: source::Heap&, g: Graph&, call: source::Value, rows: const i64[:], source_ids: i64[>..]&, c: Control&) -> bool {
    var reaches: bool[>..] = []; reaches.resize(g.nodes.len, false);
    for n, i in g.nodes { if n.tag == form_tag::Literal && n.number == literal_tag::Instant && n.origin < 0 { reaches[i] = true; } }
    var again = true;
    while again {
        again = false;
        var i = g.nodes.len;
        while i > 0 {
            i -= 1;
            let n = g.nodes[i];
            if reaches[i] { continue; }
            let edges = if linked(n.tag) { n.count } else { 0 };
            if !poll(c, 1 + edges) { return false; }
            var found = n.metadata >= 0 && reaches[n.metadata];
            if linked(n.tag) { for k in n.count { if reaches[g.children[n.start + k]] { found = true; break; } } }
            if found { reaches[i] = true; again = true; }
        }
    }
    let spanned = c.spans && call.is_object() && source::valid(heap, call);
    var at: i64 = -1; var span_start: i64 = 0; var span_end: i64 = 0;
    if spanned {
        let s = source::node(heap, call.id()); span_start = s.span_start; span_end = s.span_end;
        at = graph_source(heap, s.source, g, source_ids, c);
        if c.error != Error.None { return false; }
    }
    for i in g.nodes.len {
        if !reaches[i] || g.nodes[i].origin < 0 { continue; }
        let m .= g.nodes[i]; m.origin = -1;
        if spanned { m.source = at; m.span_start = span_start; m.span_end = span_end; }
        if rows[i] < 0 || !source::hash_ordered(source::node(heap, rows[i])) { continue; }
        var order: i64[>..] = [];
        if !source::traversal_order(heap, source::node(heap, rows[i]), order) {
            let failed = heap.fault != source::Fault.None; heap.fault = source::Fault.None;
            return reject(c, if failed { Error.Heap } else { Error.Unsupported });
        }
        let n = g.nodes[i]; let stride: i64 = if n.tag == form_tag::Map { 2 } else { 1 };
        if !poll(c, n.count) || order.len * stride != n.count { return reject(c); }
        var run: i64[>..] = []; run.append(g.children[n.start..(n.start + n.count)]);
        var to = n.start; for e in order { for j in stride { g.children[to] = run[e * stride + j]; to += 1; } }
    }
    return true;
}
