import std;
import .program;
import .source_vm;
import .form_bridge;
import .heap_program;

fn code_init(code: source::Code&) {
    var image = program::Image {}; assert(program::install(image, embed_file("../corpus/closure-captured.pants.gsp")) == program::Error.None); assert(source::load(code, image));
}
fn graph_init(g: bridge::Graph&) {
    // Empty/non-ASCII/supplementary/lone-surrogate UTF-16 is transported as
    // code units. Two equal leaves are distinct occurrences, with one reused.
    g.units.append([233, 55357, 56832, 55296]); g.bytes.append("one two"); g.sources.push(source::SourceRange { start: 0, count: 7 }); g.sources.push(source::SourceRange { start: 0, count: 7 });
    g.nodes.push(bridge::Form { tag: form_tag::String, count: 0 });
    g.nodes.push(bridge::Form { tag: form_tag::String, start: 0, count: 4 });
    g.nodes.push(bridge::Form { tag: form_tag::Number, number: 1, origin: 2, source: 0, span_start: 0, span_end: 3 });
    g.nodes.push(bridge::Form { tag: form_tag::Number, number: 1, origin: 3, source: 1, span_start: 4, span_end: 7 });
    g.nodes.push(bridge::Form { tag: form_tag::Symbol, start: 0, count: 1, ns_start: 1, ns_count: 3 });
    g.nodes.push(bridge::Form { tag: form_tag::Keyword, start: 0, count: 1 });
    g.children.append([5, 1]); g.nodes.push(bridge::Form { tag: form_tag::Map, start: 0, count: 2 });
    g.children.append([2, 3, 2, 0, 1, 4, 5]); g.nodes.push(bridge::Form { tag: form_tag::Vector, start: 2, count: 7, metadata: 6 });
    g.roots.push(7);
}
thread_fn roundtrip() {
    defer { heap_program::finish(); }
    var code = source::Code {}; code_init(code); var output = bridge::Graph {};
    {
        var heap = source::Heap { code: code }; assert(source::initialize(heap)); var g = bridge::Graph {}; graph_init(g);
        var c = bridge::Control {}; var roots: source::Value[>..] = []; assert(bridge::import_graph(heap, g, roots, c));
        let old = roots[0]; let a = source::form_nth(heap, old, 0); let b = source::form_nth(heap, old, 1);
        assert(a.id() != b.id() && source::form_equal(heap, a, b) && source::form_nth(heap, old, 2).id() == a.id());
        let changed = source::form_assoc(heap, old, source::number(1), source::number(8));
        assert(source::form_nth(heap, old, 1).id() == b.id() && source::form_nth(heap, changed, 0).id() == a.id() && source::form_nth(heap, changed, 2).id() == a.id());
        let append = source::form_assoc(heap, old, source::number(7), a); assert(source::form_count(heap, append) == 8 && source::form_nth(heap, append, 7).id() == a.id());
        let meta = source::form_op(heap, form_op::CljMeta, [old]); assert(source::form_kind(heap, meta) == node_kind::Map);
        let with_meta = source::form_op(heap, form_op::CljWithMeta, [old, source::nil()]); assert(source::form_op(heap, form_op::CljMeta, [with_meta]).is_null() && source::form_op(heap, form_op::CljMeta, [old]).id() == meta.id());
        let map = source::form_assoc(heap, meta, a, old); let map2 = source::form_assoc(heap, map, b, append);
        assert(source::form_count(heap, map) == 2 && source::form_count(heap, map2) == 2 && source::form_get(heap, map, b, source::nil()).id() == old.id() && source::form_get(heap, map2, a, source::nil()).id() == append.id());
        let constructed = source::form_construct(heap, node_kind::Map, [a, old, b, append]); let set = source::form_construct(heap, node_kind::Set, [a, b]);
        assert(source::form_count(heap, constructed) == 1 && source::form_get(heap, constructed, a, source::nil()).id() == append.id());
        assert(source::slot(heap, source::node(heap, constructed.id()).start).id() == a.id() && source::form_count(heap, set) == 1 && source::slot(heap, source::node(heap, set.id()).start).id() == a.id());
        assert(bridge::export_graph(heap, [old, changed, append], output, c));
        // assoc outside [0,count] fails, and the preceding persistent values
        // remain valid after the failure is acknowledged.
        source::form_assoc(heap, old, source::number(-1), a); assert(heap.fault == source::Fault.TypeError); heap.fault = source::Fault.None;
        source::form_assoc(heap, old, source::number(8), a); assert(heap.fault == source::Fault.TypeError); heap.fault = source::Fault.None;
        assert(source::form_count(heap, old) == 7);
    }
    var c = bridge::Control {}; var bytes: u8[>..] = []; assert(bridge::encode(output, bytes, c)); var decoded = bridge::Graph {}; assert(bridge::decode(decoded, bytes, c));
    let root = decoded.nodes[decoded.roots[0]]; assert(root.metadata >= 0 && root.count == 7);
    let first = decoded.children[root.start]; let second = decoded.children[root.start + 1]; assert(first != second && first == decoded.children[root.start + 2]);
    assert(decoded.nodes[first].span_start == 0 && decoded.nodes[second].span_start == 4);
    assert(decoded.nodes[first].source != decoded.nodes[second].source && decoded.sources.len == 2);
    let unicode = decoded.nodes[decoded.children[root.start + 4]]; assert(decoded.units[unicode.start..(unicode.start + unicode.count)] == [233 as u16, 55357, 56832, 55296]);
    assert(decoded.bytes[..] == "one two");
}
// light-cljs-877: a map's or set's traversal order (map_order). Updates
// past 8 entries promote to hash order and keep it; a cell value's graph
// carries the order (Form.number 1), which a compiler graph may not; a
// compiler form's map past 8 entries and set past 8 members import
// hash-ordered, as the V8 build's form bridge decodes them.
fn order_of(heap: source::Heap&, v: source::Value) -> i64 { return source::node(heap, v.id()).length; }
fn transported(heap: source::Heap&, roots: const source::Value[:], cells: bool, out: source::Value[>..]&) {
    var g = bridge::Graph {}; var c = bridge::Control { cells: cells, spans: false }; var bytes: u8[>..] = [];
    assert(bridge::export_graph(heap, roots, g, c) && bridge::encode(g, bytes, c));
    var d = bridge::Graph {}; var r = bridge::Control { cells: cells }; assert(bridge::decode(d, bytes, r) && bridge::import_graph(heap, d, out, r));
}
// light-cljs-878: inside the session the realm's empty literals keep their
// rows through a transport, as a child, a metadata edge or a root; to the
// compiler they are new forms. An empty collection with metadata is no
// realm row, and the marker (number 2) needs a cell graph, no children and
// no metadata.
thread_fn realm_empties() {
    defer { heap_program::finish(); }
    var code = source::Code {}; code_init(code); var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let v = source::empty_literal(heap, node_kind::Vector); let m = source::empty_literal(heap, node_kind::Map);
    assert(source::empty_form(v.id()) && source::empty_form(m.id()) && source::node(heap, m.id()).kind == node_kind::Map);
    let tagged = source::form_collection(heap, node_kind::Vector, [source::number(1)]); source::node_mut(heap, tagged.id()).metadata = m;
    let marked = source::form_collection(heap, node_kind::Vector, []); source::node_mut(heap, marked.id()).metadata = m;
    let outer = source::form_collection(heap, node_kind::List, [v, m, tagged, marked]);
    var kept: source::Value[>..] = []; transported(heap, [outer, v], true, kept);
    let k = source::node(heap, kept[0].id());
    assert(source::slot(heap, k.start) == v && source::slot(heap, k.start + 1) == m && kept[1] == v);
    let kt = source::slot(heap, k.start + 2); let km = source::slot(heap, k.start + 3);
    assert(source::node(heap, kt.id()).metadata == m && !source::empty_form(km.id()) && source::node(heap, km.id()).metadata == m);
    var fresh: source::Value[>..] = []; transported(heap, [outer, v], false, fresh);
    let f = source::node(heap, fresh[0].id());
    assert(!source::empty_form(source::slot(heap, f.start).id()) && !source::empty_form(fresh[1].id()) && !source::empty_form(source::node(heap, source::slot(heap, f.start + 2).id()).metadata.id()));
    var g = bridge::Graph {}; var c = bridge::Control { cells: true, spans: false }; assert(bridge::export_graph(heap, [v, marked], g, c));
    assert(g.nodes[g.roots[0]].number == 2 && g.nodes[g.roots[1]].number == 0);
    var bytes: u8[>..] = []; g.nodes[g.roots[1]].number = 2; var cells = bridge::Control { cells: true }; assert(!bridge::encode(g, bytes, cells) && cells.error == bridge::Error.Malformed);
    var plain = bridge::Graph {}; var pc = bridge::Control { spans: false }; assert(bridge::export_graph(heap, [v], plain, pc) && plain.nodes[plain.roots[0]].number == 0);
    plain.nodes[plain.roots[0]].number = 2; var pe = bridge::Control {}; assert(!bridge::encode(plain, bytes, pe) && pe.error == bridge::Error.Malformed);
    print("compiler form bridge: realm empty literals keep their rows in cell graphs, as children, metadata and roots");
}
thread_fn traversal_order() {
    defer { heap_program::finish(); }
    var code = source::Code {}; code_init(code); var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var pairs: source::Value[>..] = []; var members: source::Value[>..] = [];
    for i in 9 { pairs.push(source::number(i)); pairs.push(source::number(i * 10)); members.push(source::number(i)); }
    let eight = source::form_construct(heap, node_kind::Map, pairs[..16]); let nine = source::form_construct(heap, node_kind::Map, pairs[..]);
    let small = source::form_construct(heap, node_kind::Set, members[..8]); let wide = source::form_construct(heap, node_kind::Set, members[..]);
    assert(order_of(heap, eight) == map_order::Insertion && order_of(heap, nine) == map_order::Insertion && order_of(heap, wide) == map_order::Insertion);
    // A ninth entry or member promotes; replacing one or a present member does not.
    let promoted = source::form_assoc(heap, eight, source::number(100), source::nil()); let replaced = source::form_assoc(heap, eight, source::number(0), source::nil());
    let joined = source::form_conj(heap, small, source::number(100)); let present = source::form_conj(heap, small, source::number(0));
    assert(order_of(heap, promoted) == map_order::Hash && order_of(heap, replaced) == map_order::Insertion && order_of(heap, joined) == map_order::Hash && present.id() == small.id());
    // An insertion-ordered map past 8 entries replaces in place and promotes on a new key; hash order stays.
    assert(order_of(heap, source::form_assoc(heap, nine, source::number(0), source::nil())) == map_order::Insertion && order_of(heap, source::form_assoc(heap, nine, source::number(100), source::nil())) == map_order::Hash);
    assert(order_of(heap, source::form_assoc(heap, promoted, source::number(0), source::nil())) == map_order::Hash);
    // NewFormMap's map is hash-ordered at any size; NewFormSet's past 8 members.
    let literal = source::form_ordered(heap, node_kind::Map, pairs[..4], true); let set8 = source::form_ordered(heap, node_kind::Set, members[..8], false); let set9 = source::form_ordered(heap, node_kind::Set, members[..], false);
    assert(order_of(heap, literal) == map_order::Hash && order_of(heap, set8) == map_order::Insertion && order_of(heap, set9) == map_order::Hash);
    // A cell value keeps each order; a compiler graph decides by size.
    var kept: source::Value[>..] = []; transported(heap, [literal, nine, joined, small], true, kept);
    assert(order_of(heap, kept[0]) == map_order::Hash && order_of(heap, kept[1]) == map_order::Insertion && order_of(heap, kept[2]) == map_order::Hash && order_of(heap, kept[3]) == map_order::Insertion);
    var decoded: source::Value[>..] = []; transported(heap, [literal, nine, eight, wide, small], false, decoded);
    assert(order_of(heap, decoded[0]) == map_order::Insertion && order_of(heap, decoded[1]) == map_order::Hash && order_of(heap, decoded[2]) == map_order::Insertion && order_of(heap, decoded[3]) == map_order::Hash && order_of(heap, decoded[4]) == map_order::Insertion);
    // To the compiler a hash-ordered map's entries go in hash order; to a cell in insertion order.
    let mixed = source::form_ordered(heap, node_kind::Map, [source::number(1), source::nil(), source::number(32), source::nil()], true);
    var wire = bridge::Graph {}; var wc = bridge::Control { spans: false }; assert(bridge::export_graph(heap, [mixed], wire, wc));
    assert(wire.nodes[wire.children[wire.nodes[wire.roots[0]].start]].number == 32);
    var stored = bridge::Graph {}; var sc = bridge::Control { cells: true, spans: false }; assert(bridge::export_graph(heap, [mixed], stored, sc));
    assert(stored.nodes[stored.children[stored.nodes[stored.roots[0]].start]].number == 1 && stored.nodes[stored.roots[0]].number == 1);
    // A compiler graph cannot carry an order, nor a cell graph another number.
    var g = bridge::Graph {}; var c = bridge::Control { spans: false }; assert(bridge::export_graph(heap, [eight], g, c));
    g.nodes[g.roots[0]].number = 1; var plain = bridge::Control {}; var bytes: u8[>..] = []; assert(!bridge::encode(g, bytes, plain) && plain.error == bridge::Error.Malformed);
    g.nodes[g.roots[0]].number = 2; var cells = bridge::Control { cells: true }; assert(!bridge::encode(g, bytes, cells) && cells.error == bridge::Error.Malformed);
    // A hash-ordered map's keys: nil first, then by five-bit hash paths from the low bits.
    let h1, ok1 = source::form_hash(heap, source::number(1)); let h32, ok32 = source::form_hash(heap, source::number(32)); let h2, ok2 = source::form_hash(heap, source::number(2));
    assert(ok1 && ok32 && ok2 && h1 == 1 && h32 == 32 && h2 == 2);
    assert(source::legacy_order_key(heap, source::nil(), 0) < source::legacy_order_key(heap, source::number(0), 0));
    assert(source::legacy_order_key(heap, source::number(32), h32) < source::legacy_order_key(heap, source::number(1), h1) && source::legacy_order_key(heap, source::number(1), h1) < source::legacy_order_key(heap, source::number(2), h2));
    let atom = source::allocate(heap, source::Node { kind: node_kind::Atom }); let ha, oka = source::form_hash(heap, atom); assert(!oka && heap.fault == source::Fault.None);
    print("traversal order: promotion past 8 entries, hash-ordered literals, cell graphs keep it, compiler graphs decide by size, hash paths passed");
}
// light-cljs-879: a compiler graph's map or set merges keys or members
// equal at run time (two occurrences of 1, the reader's 1 and 1.0), inner
// ones first, as the V8 build's form bridge decodes them: the first key
// with the last value, the first member; a set is hash-ordered past 8
// distinct members. Each keeps its origin. A cell graph is not merged.
fn merge_graph(g: bridge::Graph&, tag: i64, keys: const f64[:], origin: i64 = -1) -> i64 {
    var ids: i64[>..] = [];
    for k, i in keys { g.nodes.push(bridge::Form { tag: form_tag::Number, number: k, origin: 10 + i }); ids.push(g.nodes.len - 1); if tag == form_tag::Map { g.nodes.push(bridge::Form { tag: form_tag::Number, number: 100 + i }); ids.push(g.nodes.len - 1); } }
    let start = g.children.len; g.children.append(ids); g.nodes.push(bridge::Form { tag: tag, start: start, count: ids.len, origin: origin }); return g.nodes.len - 1;
}
thread_fn merged_keys() {
    defer { heap_program::finish(); }
    var code = source::Code {}; code_init(code); var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var g = bridge::Graph {};
    g.roots.push(merge_graph(g, form_tag::Set, [1, 1, 2], 7)); g.roots.push(merge_graph(g, form_tag::Set, [1, 1, 2, 3, 4, 5, 6, 7, 8])); g.roots.push(merge_graph(g, form_tag::Set, [1, 1, 2, 3, 4, 5, 6, 7, 8, 9]));
    g.roots.push(merge_graph(g, form_tag::Map, [1, 2, 1])); g.roots.push(merge_graph(g, form_tag::Map, [1, 1, 2, 3, 4, 5, 6, 7, 8]));
    let inner = merge_graph(g, form_tag::Set, [1, 1]); let other = merge_graph(g, form_tag::Set, [1]); let start = g.children.len; g.children.append([inner, other]); g.nodes.push(bridge::Form { tag: form_tag::Set, start: start, count: 2 }); g.roots.push(g.nodes.len - 1);
    var roots: source::Value[>..] = []; var c = bridge::Control {}; assert(bridge::import_graph(heap, g, roots, c));
    let first = source::node(heap, roots[0].id()); assert(first.count == 2 && first.origin == 7 && source::node(heap, source::slot(heap, first.start).id()).origin == 10 && order_of(heap, roots[0]) == map_order::Insertion);
    assert(source::form_count(heap, roots[1]) == 8 && order_of(heap, roots[1]) == map_order::Insertion && source::form_count(heap, roots[2]) == 9 && order_of(heap, roots[2]) == map_order::Hash);
    let map = source::node(heap, roots[3].id()); assert(map.count == 4 && source::node(heap, source::slot(heap, map.start).id()).origin == 10 && source::form_value(heap, source::slot(heap, map.start + 1)).num() == 102);
    assert(source::form_count(heap, roots[4]) == 8 && order_of(heap, roots[4]) == map_order::Hash && source::form_count(heap, roots[5]) == 1);
    // A cell value's graph keeps every position.
    var cells: source::Value[>..] = []; var cc = bridge::Control { cells: true }; assert(bridge::import_graph(heap, g, cells, cc)); assert(source::node(heap, cells[0].id()).count == 3 && source::form_count(heap, cells[5]) == 2);
    // Merging precedes installation: a merge comparison that exhausts the
    // work leaves every table as it was.
    var probe = bridge::Control {}; var counted: source::Value[>..] = []; assert(bridge::import_graph(heap, g, counted, probe));
    let nodes = heap.nodes.len; let values = heap.values.len;
    var short = bridge::Control { work: probe.visits - 1 }; var none: source::Value[>..] = [];
    assert(!bridge::import_graph(heap, g, none, short) && short.error == bridge::Error.Work && none.len == 0 && heap.nodes.len == nodes && heap.values.len == values);
    // Distinct collection keys rarely share a class: 5,000 one-element
    // vectors compare almost never (about 80,000 polls, all validation and copying).
    var wide = bridge::Graph {}; var members: i64[>..] = [];
    for i in 5000 { wide.nodes.push(bridge::Form { tag: form_tag::Number, number: i }); wide.children.push(wide.nodes.len - 1); wide.nodes.push(bridge::Form { tag: form_tag::Vector, start: wide.children.len - 1, count: 1 }); members.push(wide.nodes.len - 1); }
    let wstart = wide.children.len; wide.children.append(members); wide.nodes.push(bridge::Form { tag: form_tag::Set, start: wstart, count: 5000 }); wide.roots.push(wide.nodes.len - 1);
    var wc = bridge::Control {}; var wr: source::Value[>..] = []; assert(bridge::import_graph(heap, wide, wr, wc) && source::form_count(heap, wr[0]) == 5000 && wc.visits < 100000);
    print("merged keys: equal keys and members merge as the V8 build decodes them, inner first, origins kept, cell graphs positional, before installation, distinct vectors apart passed");
}
thread_fn positional_map_equality() {
    defer { heap_program::finish(); }
    var code = source::Code {}; code_init(code); var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let a = source::allocate(heap, source::Node { kind: node_kind::Scalar, value: source::number(7) });
    let b = source::allocate(heap, source::Node { kind: node_kind::Scalar, value: source::number(7) });
    assert(a.id() != b.id() && source::form_equal(heap, a, b));
    // LiteralMap retains duplicate operands; lookup and equality must both
    // stop at the first equal key, even when a later value would match.
    let first = source::form_op(heap, form_op::LiteralMap, [source::form_collection(heap, node_kind::Vector, [a, source::number(1), b, source::number(2)])]);
    let swapped = source::form_op(heap, form_op::LiteralMap, [source::form_collection(heap, node_kind::Vector, [b, source::number(2), a, source::number(1)])]);
    assert(source::form_count(heap, first) == 2 && source::form_count(heap, swapped) == 2);
    assert(source::form_get(heap, first, b, source::nil()).num() == 1 && source::form_get(heap, swapped, a, source::nil()).num() == 2);
    assert(!source::form_equal(heap, first, swapped) && !source::form_equal(heap, swapped, first));
    assert(!source::form_equal(heap, source::form_collection(heap, node_kind::Set, [first]), source::form_collection(heap, node_kind::Set, [swapped])));
    let outer = source::form_construct(heap, node_kind::Map, [first, source::number(10), swapped, source::number(20)]);
    assert(source::form_count(heap, outer) == 2);
    assert(source::form_get(heap, outer, first, source::nil()).num() == 10 && source::form_get(heap, outer, swapped, source::nil()).num() == 20);
    let changed = source::form_assoc(heap, outer, swapped, source::number(30));
    assert(source::form_get(heap, changed, first, source::nil()).num() == 10 && source::form_get(heap, changed, swapped, source::nil()).num() == 30 && source::form_get(heap, outer, swapped, source::nil()).num() == 20);
    let ordered = source::form_construct(heap, node_kind::Map, [a, source::number(1), source::number(8), source::number(2)]);
    let reordered = source::form_construct(heap, node_kind::Map, [source::number(8), source::number(2), b, source::number(1)]);
    assert(source::form_equal(heap, ordered, reordered) && source::form_equal(heap, reordered, ordered) && heap.fault == source::Fault.None);
    print("positional map equality: first matching key agrees with lookup, nested keys and persistent assoc passed");
}
thread_fn number_bits() {
    defer { heap_program::finish(); }
    var code = source::Code {}; code_init(code); var g = bridge::Graph {};
    for number in [-0.0, 0.0 / 0.0, 1.0 / 0.0, -1.0 / 0.0] { g.roots.push(g.nodes.len); g.nodes.push(bridge::Form { tag: form_tag::Number, number: number }); }
    var heap = source::Heap { code: code }; assert(source::initialize(heap)); var roots: source::Value[>..] = []; var c = bridge::Control {}; assert(bridge::import_graph(heap, g, roots, c));
    var out = bridge::Graph {}; assert(bridge::export_graph(heap, roots, out, c)); var bytes: u8[>..] = []; assert(bridge::encode(out, bytes, c)); var restored = bridge::Graph {}; assert(bridge::decode(restored, bytes, c));
    for i in 4 { assert(to_bytes([g.nodes[i].number]) == to_bytes([restored.nodes[restored.roots[i]].number])); }
    assert(source::form_equal(heap, source::nil(), source::Value.Undefined));
    assert(source::form_equal(heap, source::number(-0.0), source::number(0.0)));
    assert(!source::form_equal(heap, source::number(0.0 / 0.0), source::number(0.0 / 0.0)));
    assert(source::form_equal(heap, roots[1], roots[1]));
}
thread_fn unsupported_values() {
    defer { heap_program::finish(); }
    var code = source::Code {}; code_init(code); var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let ns = source::text(heap, "a/b"); let name = source::text(heap, "x");
    source::form_op(heap, form_op::Symbol, [ns, name]); assert(heap.fault == source::Fault.NativeBoundary); heap.fault = source::Fault.None;
    let keyword = source::form_op(heap, form_op::Keyword, [source::nil(), name]); source::form_op(heap, form_op::CljWithMeta, [keyword, source::nil()]); assert(heap.fault == source::Fault.TypeError); heap.fault = source::Fault.None;
    let vector = source::form_collection(heap, node_kind::Vector, [source::number(1)]);
    for index in [0.0 / 0.0, 1.0 / 0.0, -1.0 / 0.0, 1e100] {
        assert(source::form_get(heap, vector, source::number(index), source::number(99)).num() == 99 && heap.fault == source::Fault.None);
        source::form_assoc(heap, vector, source::number(index), source::nil()); assert(heap.fault == source::Fault.TypeError); heap.fault = source::Fault.None;
        source::form_op(heap, form_op::Nth, [vector, source::number(index)]); assert(heap.fault == source::Fault.TypeError); heap.fault = source::Fault.None;
        // cljs.core's nth throws its Error "Index out of bounds" (light-cljs-873).
        source::form_op(heap, form_op::CljNth, [vector, source::number(index)]); assert(heap.fault == source::Fault.HostEffect); heap.fault = source::Fault.None;
    }
    let object = source::allocate(heap, source::Node { kind: node_kind::Object }); let container = source::form_collection(heap, node_kind::Vector, [object]);
    var c = bridge::Control {}; var out = bridge::Graph {}; assert(!bridge::export_graph(heap, [container], out, c) && c.error == bridge::Error.Unsupported && out.nodes.len == 0);
    var g = bridge::Graph {}; g.units.append([97, 47, 98]); g.nodes.push(bridge::Form { tag: form_tag::Symbol, start: 0, count: 3 }); g.roots.push(0);
    var bad = bridge::Control {}; var roots: source::Value[>..] = []; let before = heap.nodes.len;
    assert(!bridge::import_graph(heap, g, roots, bad) && bad.error == bridge::Error.Unsupported && roots.len == 0 && heap.nodes.len == before);
}
thread_fn primitive_fallback() {
    defer { heap_program::finish(); }
    var code = source::Code {}; code_init(code); var heap = source::Heap { code: code }; assert(source::initialize(heap));
    heap.source_bytes.append("one two"); heap.source_ranges.push(source::SourceRange { start: 0, count: 7 });
    let text = source::string(heap, [233, 55357, 56832, 55296]);
    let argument = source::allocate(heap, source::Node { kind: node_kind::Scalar, value: text, origin: 0, source: 0, span_start: 0, span_end: 3 });
    let annotation = source::allocate(heap, source::Node { kind: node_kind::Scalar, value: text, origin: 0, source: 0, span_start: 4, span_end: 7 });
    let metadata = source::form_collection(heap, node_kind::Map, [source::number(1), annotation]);
    let call = source::form_collection(heap, node_kind::List, [source::nil(), argument]); source::node_mut(heap, call.id()).metadata = metadata;
    var c = bridge::Control {}; var out = bridge::Graph {}; assert(bridge::export_graph(heap, [text], out, c, call));
    // Bare primitive compatibility starts from arguments, not &form metadata.
    assert(out.nodes[out.roots[0]].span_start == 0 && out.nodes[out.roots[0]].origin == 0);
    for cut in c.visits { var cancelled = bridge::Control { cancel_after: cut }; var untouched = bridge::Graph {}; assert(!bridge::export_graph(heap, [text], untouched, cancelled, call) && cancelled.error == bridge::Error.Cancelled && untouched.nodes.len == 0); }
}
thread_fn budgets() {
    defer { heap_program::finish(); }
    var code = source::Code {}; code_init(code); var g = bridge::Graph {}; graph_init(g); var full = bridge::Control {}; assert(bridge::validate(g, full));
    for mode in 4 {
        var c = bridge::Control {}; if mode == 0 { c.nodes = 7; } if mode == 1 { c.depth = 2; } if mode == 2 { c.bytes = bridge::footprint(g) - 1; } if mode == 3 { c.work = full.visits - 1; }
        assert(!bridge::validate(g, c)); assert(c.error == (if mode == 0 { bridge::Error.Nodes } else if mode == 1 { bridge::Error.Depth } else if mode == 2 { bridge::Error.Bytes } else { bridge::Error.Work }));
    }
    var heap = source::Heap { code: code }; assert(source::initialize(heap)); var roots: source::Value[>..] = []; var control = bridge::Control {}; assert(bridge::import_graph(heap, g, roots, control));
    let nodes = heap.nodes.len; let units = heap.units.len; let values = heap.values.len; let text = heap.source_bytes.len;
    var success = bridge::Control {}; var imported: source::Value[>..] = []; assert(bridge::import_graph(heap, g, imported, success));
    let stable_nodes = heap.nodes.len; let stable_units = heap.units.len; let stable_latin1 = heap.latin1.len; let stable_values = heap.values.len; let stable_text = heap.source_bytes.len;
    for cut in success.visits {
        var c = bridge::Control { cancel_after: cut }; var untouched: source::Value[>..] = [];
        assert(!bridge::import_graph(heap, g, untouched, c) && c.error == bridge::Error.Cancelled && untouched.len == 0);
        assert(heap.nodes.len == stable_nodes && heap.units.len == stable_units && heap.latin1.len == stable_latin1 && heap.values.len == stable_values && heap.source_bytes.len == stable_text);
    }
    heap.limits.nodes = heap.nodes.len - heap.image.nodes; var c = bridge::Control {}; var untouched: source::Value[>..] = [];
    assert(!bridge::import_graph(heap, g, untouched, c) && c.error == bridge::Error.Heap && heap.nodes.len == stable_nodes);
    var exported = bridge::Graph {}; var complete = bridge::Control {}; assert(bridge::export_graph(heap, roots, exported, complete, roots[0]));
    for mode in 5 {
        var c = bridge::Control {}; var out = bridge::Graph {};
        if mode == 0 { c.nodes = 2; } if mode == 1 { c.depth = 2; } if mode == 2 { c.bytes = 100; } if mode == 3 { c.work = 1; } if mode == 4 { c.cancelled = true; }
        assert(!bridge::export_graph(heap, roots, out, c, roots[0]) && out.nodes.len == 0 && out.bytes.len == 0);
        assert(c.error == (if mode == 0 { bridge::Error.Nodes } else if mode == 1 { bridge::Error.Depth } else if mode == 2 { bridge::Error.Bytes } else if mode == 3 { bridge::Error.Work } else { bridge::Error.Cancelled }));
    }
    for cut in complete.visits {
        var c = bridge::Control { cancel_after: cut }; var out = bridge::Graph {};
        assert(!bridge::export_graph(heap, roots, out, c, roots[0]) && c.error == bridge::Error.Cancelled && out.nodes.len == 0 && out.bytes.len == 0);
    }
    var bytes: u8[>..] = []; var encode = bridge::Control {}; assert(bridge::encode(g, bytes, encode));
    var decode = bridge::Control {}; var decoded = bridge::Graph {}; assert(bridge::decode(decoded, bytes, decode));
    for cut in decode.visits { var c = bridge::Control { cancel_after: cut }; var out = bridge::Graph {}; assert(!bridge::decode(out, bytes, c) && c.error == bridge::Error.Cancelled && out.nodes.len == 0); }
    for cut in encode.visits { var c = bridge::Control { cancel_after: cut }; var out: u8[>..] = [42]; assert(!bridge::encode(g, out, c) && c.error == bridge::Error.Cancelled && out.len == 1 && out[0] == 42); }
    for cut in bytes.len { var c = bridge::Control {}; var out = bridge::Graph {}; assert(!bridge::decode(out, bytes[..cut], c) && out.nodes.len == 0); }
    // Metadata cycles, child cycles, malformed ranges and hidden/deep DAG
    // paths are all rejected before installation, including unreachable nodes.
    g.nodes[1].metadata = 6; var bad = bridge::Control {}; assert(!bridge::validate(g, bad) && bad.error == bridge::Error.Malformed); g.nodes[1].metadata = -1;
    g.nodes[4].ns_count = -2; var bad_ns = bridge::Control {}; assert(!bridge::validate(g, bad_ns)); g.nodes[4].ns_count = 3;
    g.children[0] = g.nodes.len; var bad_id = bridge::Control {}; assert(!bridge::validate(g, bad_id));
    let saved = source::node(heap, roots[0].id()).metadata; source::node_mut(heap, roots[0].id()).metadata = roots[0];
    var cycle_control = bridge::Control {}; var cycle_out = bridge::Graph {};
    assert(!bridge::export_graph(heap, roots, cycle_out, cycle_control, roots[0]) && cycle_control.error == bridge::Error.Depth && cycle_out.nodes.len == 0);
    source::node_mut(heap, roots[0].id()).metadata = saved;
}
fn update_case(heap: source::Heap&, operation: i64, old: source::Value, key: source::Value, value: source::Value) -> source::Value {
    if operation <= 3 { return source::form_assoc(heap, old, if operation == 0 { source::number(0) } else if operation == 1 { source::number(2) } else if operation == 2 { key } else { source::number(8) }, value); }
    return source::form_conj(heap, if operation == 6 { source::nil() } else { old }, value);
}
// One heap per (operation, mode) case; mode 0 reports the exact work the
// limit modes cut against.
struct Cost { work: i64 }
thread_fn update_boundary(operation: i64, mode: i64, cost: i64) {
    defer { heap_program::finish(); }
    var code = source::Code {}; code_init(code);
    {
        {
            var heap = source::Heap { code: code }; assert(source::initialize(heap));
            let key = source::allocate(heap, source::Node { kind: node_kind::Scalar, value: source::number(7) });
            let equal_key = source::allocate(heap, source::Node { kind: node_kind::Scalar, value: source::number(7) });
            let metadata = source::form_collection(heap, node_kind::Map, []);
            let old = if operation == 2 || operation == 3 { source::form_collection(heap, node_kind::Map, [key, source::number(1), equal_key, source::number(2)]) } else { source::form_collection(heap, if operation == 4 { node_kind::List } else { node_kind::Vector }, [key, equal_key]) };
            source::node_mut(heap, old.id()).metadata = metadata; source::node_mut(heap, old.id()).origin = 17; source::node_mut(heap, old.id()).source = 3; source::node_mut(heap, old.id()).span_start = 11; source::node_mut(heap, old.id()).span_end = 22;
            let n = source::node(heap, old.id()); let nodes = heap.nodes.len; let values = heap.values.len; let work = heap.work;
            let count: i64 = if operation == 0 { 2 } else if operation == 1 { 3 } else if operation == 2 { 4 } else if operation == 3 { 6 } else if operation == 6 { 1 } else { 3 };
            if mode == 1 { heap.limits.work = work + cost; heap.limits.nodes = nodes + 1; heap.limits.values = values + count; }
            if mode == 2 || mode == 3 { heap.limits.work = work + cost - (mode - 1); }
            if mode == 4 { heap.limits.nodes = nodes; }
            if mode == 5 { heap.limits.values = values + count - 1; }
            var value = key;
            if mode == 6 { value = source::object(nodes + 100); }
            if mode == 7 {
                if operation == 6 { return; }
                source::set_slot(heap, n.start + n.count - 1, source::object(nodes));
            }
            let result = update_case(heap, operation, old, equal_key, value);
            if mode == 0 || mode == 1 {
                assert(heap.fault == source::Fault.None && heap.nodes.len == nodes + 1 && heap.values.len == values + count);
                if mode == 0 { assert(qput(Cost { work: heap.work - work })); }
                let updated = source::node(heap, result.id()); assert(updated.start == values && updated.count == count && updated.origin == -1);
                if operation != 6 { assert(updated.metadata.id() == metadata.id() && updated.source == 3 && updated.span_start == 11 && updated.span_end == 22); }
                assert(source::node(heap, old.id()).start == n.start && source::node(heap, old.id()).origin == 17 && source::slot(heap, n.start).id() == key.id());
                if operation == 0 { assert(source::slot(heap, updated.start).id() == key.id() && source::slot(heap, updated.start + 1).id() == equal_key.id()); }
                if operation == 1 || operation == 5 { assert(source::slot(heap, updated.start + 2).id() == key.id() && source::slot(heap, updated.start + 1).id() == equal_key.id()); }
                if operation == 2 { assert(source::slot(heap, updated.start).id() == key.id() && source::slot(heap, updated.start + 1).id() == key.id() && source::slot(heap, updated.start + 2).id() == equal_key.id() && source::slot(heap, updated.start + 3).num() == 2); }
                if operation == 3 { assert(source::slot(heap, updated.start + 4).num() == 8 && source::slot(heap, updated.start + 5).id() == key.id()); }
                if operation == 4 { assert(source::slot(heap, updated.start).id() == key.id() && source::slot(heap, updated.start + 1).id() == key.id() && source::slot(heap, updated.start + 2).id() == equal_key.id()); }
            } else {
                assert(heap.nodes.len == nodes && heap.values.len == values);
                assert(heap.fault == (if mode == 2 || mode == 3 { source::Fault.WorkLimit } else if mode == 4 || mode == 5 { source::Fault.MemoryLimit } else { source::Fault.StaleID }));
            }
        }
    }
}
// A replaced child is not part of the resulting range and was never
// validated by form_collection. Keep that existing replacement behavior.
thread_fn replaced_child() {
    defer { heap_program::finish(); }
    var code = source::Code {}; code_init(code);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let old = source::form_collection(heap, node_kind::Vector, [source::number(1)]); source::set_slot(heap, source::node(heap, old.id()).start, source::object(heap.nodes.len));
    let fixed = source::form_assoc(heap, old, source::number(0), source::number(2)); assert(heap.fault == source::Fault.None && source::form_nth(heap, fixed, 0).num() == 2);
}
fn direct_update_boundaries() {
    for operation in 7 {
        var cost: i64 = 0;
        for mode in 8 { heap_program::join(thread_spawn(update_boundary, operation, mode, cost)); if mode == 0 { cost = qget<Cost>().work; } }
    }
    heap_program::join(thread_spawn(replaced_child));
    print("direct persistent updates: exact work/node/value limits, final allocation charge, stale children, duplicate keys, aliases and metadata passed");
}
// main builds no heap; each heap-building test is its own thread program.
fn main() {
    heap_program::join(thread_spawn(assembly)); heap_program::join(thread_spawn(plain_form_operations)); heap_program::join(thread_spawn(adversarial_forms)); heap_program::join(thread_spawn(roundtrip)); heap_program::join(thread_spawn(positional_map_equality)); heap_program::join(thread_spawn(traversal_order)); heap_program::join(thread_spawn(realm_empties)); heap_program::join(thread_spawn(merged_keys));
    direct_update_boundaries(); heap_program::join(thread_spawn(number_bits)); heap_program::join(thread_spawn(unsupported_values)); heap_program::join(thread_spawn(primitive_fallback)); heap_program::join(thread_spawn(budgets)); heap_program::join(thread_spawn(shared_text_work)); heap_program::join(thread_spawn(spanless_export)); heap_program::join(thread_spawn(identity_sources)); heap_program::join(thread_spawn(omission));
    print("compiler form bridge: owned graphs/metadata/aliases/UTF16/Number/order, exhaustive cancellation/truncation and transactional limits passed"); }

// light-cljs-817: the compiler sends a source's identity and byte length, no
// text. The call's graph: a vector of two spanned forms from source 7
// (21 bytes) and one from source 9 (5 bytes); source 11 is named by no form.
fn identity_graph(g: bridge::Graph&) {
    g.sources.push(source::SourceRange { start: 0, count: 21, identity: 7 }); g.sources.push(source::SourceRange { start: 0, count: 5, identity: 9 });
    g.sources.push(source::SourceRange { start: 0, count: 40, identity: 11 });
    g.nodes.push(bridge::Form { tag: form_tag::Number, number: 1, origin: 1, source: 0, span_start: 1, span_end: 2 });
    g.nodes.push(bridge::Form { tag: form_tag::Number, number: 2, origin: 2, source: 0, span_start: 3, span_end: 21 });
    g.nodes.push(bridge::Form { tag: form_tag::Nil, origin: 3, source: 1, span_start: 0, span_end: 5 });
    g.children.append([0, 1, 2]); g.nodes.push(bridge::Form { tag: form_tag::Vector, count: 3, origin: 0, source: 0, span_start: 0, span_end: 21 }); g.roots.push(3);
}
// Spans round-trip without text: the wire and the job heap hold no source
// bytes, an export names the identities its forms came from (and no other),
// a new form takes the call's span, and the graph decodes to the same rows.
// A span outside its source, an unknown source, an identity row claiming
// text, an identity past 32 bits and a GFB1 or version-1 header are
// refused; cancellation and over-budget conversions fail with no change. An
// identity source's UTF-8 boundaries are the compiler's to check (Goose has
// no text): an offset that splits a text source's code point is refused, the
// same offset into an identity source is not.
thread_fn identity_sources() {
    defer { heap_program::finish(); }
    var code = source::Code {}; code_init(code); var g = bridge::Graph {}; identity_graph(g);
    var c = bridge::Control {}; var bytes: u8[>..] = []; assert(bridge::encode(g, bytes, c));
    assert(bytes[..4] == "GFB2" && bytes.len == bridge::footprint(g) && bytes.len == 32 + 4 * 48 + 3 * 4 + 3 * 12 + 4);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var decoded = bridge::Graph {}; var roots: source::Value[>..] = []; assert(bridge::decode(decoded, bytes, c) && bridge::import_graph(heap, decoded, roots, c));
    assert(heap.source_bytes.len == 0 && heap.source_ranges.len == 3 && heap.source_ranges[1].identity == 9 && heap.source_ranges[1].count == 5);
    let vector = roots[0];
    let fresh = source::form_collection(heap, node_kind::Vector, [source::form_nth(heap, vector, 2), source::form_nth(heap, vector, 0)]);
    var out = bridge::Graph {}; var e = bridge::Control {}; assert(bridge::export_graph(heap, [fresh], out, e, vector));
    assert(out.bytes.len == 0 && out.sources.len == 2 && out.sources[0].identity == 7 && out.sources[0].count == 21 && out.sources[1].identity == 9 && out.sources[1].count == 5);
    let root = out.nodes[out.roots[0]]; let first = out.nodes[out.children[root.start]]; let second = out.nodes[out.children[root.start + 1]];
    assert(root.origin == -1 && out.sources[root.source].identity == 7 && root.span_start == 0 && root.span_end == 21);
    assert(first.origin == 3 && out.sources[first.source].identity == 9 && first.span_end == 5 && second.origin == 1 && out.sources[second.source].identity == 7 && second.span_start == 1);
    var wire: u8[>..] = []; assert(bridge::encode(out, wire, e)); var back = bridge::Graph {}; assert(bridge::decode(back, wire, e));
    assert(bytes_of(back.sources) == bytes_of(out.sources) && back.bytes.len == 0 && back.nodes.len == out.nodes.len);
    for n, i in back.nodes { assert(n.source == out.nodes[i].source && n.span_start == out.nodes[i].span_start && n.span_end == out.nodes[i].span_end && n.origin == out.nodes[i].origin); }
    // Refusals, each before any heap row changes.
    let nodes = heap.nodes.len; let ranges = heap.source_ranges.len;
    for mode in 6 {
        var bad = bridge::Graph {}; identity_graph(bad);
        if mode == 0 { bad.nodes[1].span_end = 22; }
        if mode == 1 { bad.nodes[2].span_start = 6; bad.nodes[2].span_end = 6; }
        if mode == 2 { bad.nodes[0].source = 3; }
        if mode == 3 { bad.sources[1].start = 1; }
        if mode == 4 { bad.sources[1].identity = 2147483648; }
        if mode == 5 { bad.bytes.append("x"); bad.sources[0].start = 1; }
        var r = bridge::Control {}; var none: source::Value[>..] = [];
        assert(!bridge::import_graph(heap, bad, none, r) && r.error == bridge::Error.Malformed && none.len == 0);
        assert(heap.nodes.len == nodes && heap.source_ranges.len == ranges && heap.source_bytes.len == 0);
    }
    for at in [3, 4] {
        var bad = copy(bytes); if at == 3 { bad[3] = 49; } else { bad[4] = 1; }
        var r = bridge::Control {}; var graph = bridge::Graph {}; assert(!bridge::decode(graph, bad, r) && r.error == bridge::Error.Malformed && graph.nodes.len == 0);
    }
    var mixed = bridge::Graph {}; mixed.bytes.append("é"); mixed.sources.push(source::SourceRange { start: 0, count: 2 }); mixed.sources.push(source::SourceRange { start: 0, count: 2, identity: 5 });
    mixed.nodes.push(bridge::Form { tag: form_tag::Nil, source: 1, span_start: 1, span_end: 2 }); mixed.roots.push(0);
    var m = bridge::Control {}; assert(bridge::validate(mixed, m));
    mixed.nodes[0].source = 0; var split = bridge::Control {}; assert(!bridge::validate(mixed, split) && split.error == bridge::Error.Malformed);
    // Cancellation at every visit, and one unit short of the work and byte budgets.
    var full = bridge::Control {}; var graph = bridge::Graph {}; var installed: source::Value[>..] = [];
    assert(bridge::decode(graph, bytes, full) && bridge::import_graph(heap, graph, installed, full));
    let stable_nodes = heap.nodes.len; let stable_ranges = heap.source_ranges.len;
    for cut in full.visits {
        var r = bridge::Control { cancel_after: cut }; var partial = bridge::Graph {}; var none: source::Value[>..] = [];
        assert(!(bridge::decode(partial, bytes, r) && bridge::import_graph(heap, partial, none, r)) && r.error == bridge::Error.Cancelled && none.len == 0);
        assert(heap.nodes.len == stable_nodes && heap.source_ranges.len == stable_ranges && heap.source_bytes.len == 0);
    }
    var short = bridge::Control { work: full.visits - 1 }; var partial = bridge::Graph {}; var none: source::Value[>..] = [];
    assert(!(bridge::decode(partial, bytes, short) && bridge::import_graph(heap, partial, none, short)) && short.error == bridge::Error.Work && none.len == 0);
    var small = bridge::Control { bytes: bytes.len - 1 }; var refused = bridge::Graph {}; assert(!bridge::decode(refused, bytes, small) && small.error == bridge::Error.Bytes);
    var export_cut = bridge::Control {}; var sized = bridge::Graph {}; assert(bridge::export_graph(heap, [fresh], sized, export_cut, vector));
    for cut in export_cut.visits { var r = bridge::Control { cancel_after: cut }; var untouched = bridge::Graph {}; assert(!bridge::export_graph(heap, [fresh], untouched, r, vector) && r.error == bridge::Error.Cancelled && untouched.nodes.len == 0 && untouched.sources.len == 0); }
    assert(heap.nodes.len == stable_nodes && heap.source_ranges.len == stable_ranges);
    print("identity sources: spans round-trip without text, only named sources export, bad spans/sources/headers refused, cancellation and budgets clean");
}
// A cell value's export (spans off, light-cljs-809) carries what the value
// reaches and none of the heap's source text: with 8 KB of unrelated source
// over a 4 KB byte budget it succeeds and round-trips without provenance,
// where the export that carries spans refuses it.
thread_fn spanless_export() {
    defer { heap_program::finish(); }
    var code = source::Code {}; code_init(code); var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var g = bridge::Graph {}; graph_init(g); var imported = bridge::Control {}; var roots: source::Value[>..] = [];
    assert(bridge::import_graph(heap, g, roots, imported));
    var text: u8[>..] = []; for i in 8192 { text.push(59); } source::add_source_bytes(heap, text);
    var spans = bridge::Control { bytes: 4096 }; var refused = bridge::Graph {};
    assert(!bridge::export_graph(heap, roots, refused, spans) && spans.error == bridge::Error.Bytes);
    var c = bridge::Control { bytes: 4096, spans: false }; var out = bridge::Graph {};
    assert(bridge::export_graph(heap, roots, out, c));
    assert(out.bytes.len == 0 && out.sources.len == 0 && out.nodes.len == g.nodes.len);
    for n in out.nodes { assert(n.source == -1 && n.span_start == 0 && n.span_end == 0); }
    var bytes: u8[>..]            = []; assert(bridge::encode(out, bytes, c)); var decoded = bridge::Graph {}; assert(bridge::decode(decoded, bytes, c));
    var back:  source::Value[>..] = []; var again = bridge::Control {}; assert(bridge::import_graph(heap, decoded, back, again) && back.len == 1 && source::form_equal(heap, back[0], roots[0]));
}
// An environment map {:a 1, :ns/b ^{} ["xyz" s]} (light-cljs-820), and the
// same map narrowed to {:a 1} with the omission record of the left-out
// entry: five nodes, four child slots (two the map's, two the vector's), seven
// units, four string rows, three units of names (`ns`, `s`) and depth 3.
fn whole_environment(g: bridge::Graph&) {
    g.units.append([97, 98, 110, 115, 120, 121, 122, 115]);
    g.nodes.push(bridge::Form { tag: form_tag::Keyword, start: 0, count: 1 }); g.nodes.push(bridge::Form { tag: form_tag::Number, number: 1 });
    g.nodes.push(bridge::Form { tag: form_tag::Keyword, start: 1, count: 1, ns_start: 2, ns_count: 2 }); g.nodes.push(bridge::Form { tag: form_tag::String, start: 4, count: 3 });
    g.nodes.push(bridge::Form { tag: form_tag::Symbol, start: 7, count: 1 }); g.nodes.push(bridge::Form { tag: form_tag::Map, start: 0, count: 0 });
    g.children.append([3, 4]); g.nodes.push(bridge::Form { tag: form_tag::Vector, start: 0, count: 2, metadata: 5 });
    g.children.append([0, 1, 2, 6]); g.nodes.push(bridge::Form { tag: form_tag::Map, start: 2, count: 4 }); g.roots.push(7);
}
fn narrowed_environment(g: bridge::Graph&) {
    g.units.push(97); g.nodes.push(bridge::Form { tag: form_tag::Keyword, start: 0, count: 1 }); g.nodes.push(bridge::Form { tag: form_tag::Number, number: 1 });
    g.children.append([0, 1]); g.nodes.push(bridge::Form { tag: form_tag::Map, start: 0, count: 2 }); g.roots.push(2);
    g.omitted = bridge::Omitted { nodes: 5, children: 4, units: 7, strings: 4, names: 3, depth: 3 };
}
// The rows a heap's limits count as the job's: nodes, values, code units
// and strings above its image tops.
fn taken(heap: const source::Heap&) -> i64[4] { return [heap.nodes.len - heap.image.nodes, heap.values.len - heap.image.values, source::unit_rows(heap), heap.strings.len - heap.image.strings]; }
// What decoding and importing `bytes` into `heap` cost and took, under
// `limit` (0 nodes, 1 depth, 2 bytes, 3 work, 4/5/6 heap nodes/values/units
// one short of the whole graph's, or -1 for none; `budget` is the work for
// 3): its error, the
// control's visits, the heap's work and its rows taken.
struct Imported { error: bridge::Error, visits: i64, work: i64, taken: i64[4] }
fn import_wire(heap: source::Heap&, bytes: const u8[:], limit: i64, whole: const bridge::Graph&, budget: i64 = 0) -> Imported {
    let before = taken(heap); let work = heap.work; let limits = heap.limits;
    var c = bridge::Control {};
    if limit == 0 { c.nodes = whole.nodes.len - 1; } if limit == 1 { c.depth = 2; } if limit == 2 { c.bytes = bridge::footprint(whole) - 1; } if limit == 3 { c.work = budget; }
    if limit == 4 { heap.limits.nodes = before[0] + whole.nodes.len - 1; } if limit == 5 { heap.limits.values = before[1] + whole.children.len - 1; } if limit == 6 { heap.limits.units = before[2] + whole.units.len - 1; }
    var g = bridge::Graph {}; var roots: source::Value[>..] = [];
    let ok = bridge::decode(g, bytes, c) && bridge::import_graph(heap, g, roots, c);
    let after = taken(heap); heap.limits = limits;
    if !ok { assert(after == before && heap.work == work && roots.len == 0); }
    return Imported { error: c.error, visits: c.visits, work: heap.work - work, taken: [after[0] - before[0], after[1] - before[1], after[2] - before[2], after[3] - before[3]] };
}
// A narrowed graph's omission record decodes, validates, imports and is
// charged as the entries it left out would be: the same polls, work, limit
// failures and rows taken as the whole graph, whose headroom it leaves
// (light-cljs-820); a malformed count, or a record in a cell graph, is
// refused.
thread_fn omission() {
    defer { heap_program::finish(); }
    var code = source::Code {}; code_init(code); var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var whole = bridge::Graph {}; whole_environment(whole); var narrowed = bridge::Graph {}; narrowed_environment(narrowed);
    var wire:  u8[>..] = []; var w = bridge::Control {}; assert(bridge::encode(whole, wire, w));
    var bytes: u8[>..] = []; var n = bridge::Control {}; assert(bridge::encode(narrowed, bytes, n));
    assert(bridge::word(wire, 4) == 2 && bridge::word(bytes, 4) == 3 && bytes.len == 56 + 3 * 48 + 2 * 4 + 2 + 4 && bridge::footprint(narrowed) == wire.len && w.visits == n.visits);
    var decoded = bridge::Graph {}; var d = bridge::Control {}; assert(bridge::decode(decoded, bytes, d));
    assert(bytes_of([decoded.omitted]) == bytes_of([narrowed.omitted]) && decoded.nodes.len == 3);
    var again: u8[>..] = []; assert(bridge::encode(decoded, again, d) && again == bytes);
    var vw = bridge::Control {}; var vn = bridge::Control {}; assert(bridge::validate(whole, vw) && bridge::validate(narrowed, vn) && vw.visits == vn.visits);
    let full = import_wire(heap, wire, -1, whole); let part = import_wire(heap, bytes, -1, whole);
    assert(full.error == bridge::Error.None && part.error == bridge::Error.None && full.visits == part.visits && full.work == part.work && full.taken == part.taken);
    assert(full.taken == [8 as i64, 6, 8, 5]);
    // A result exported with the import's control, as a job's is, polls
    // the omitted nodes as heap rows: in two fresh heaps, the same visits.
    var exported: i64[2] = [0; 2];
    for narrow in 2 {
        var fresh = source::Heap { code: code }; assert(source::initialize(fresh));
        var c = bridge::Control {}; var g = bridge::Graph {}; var roots: source::Value[>..] = [];
        assert(bridge::decode(g, if narrow == 1 { bytes[..] } else { wire[..] }, c) && bridge::import_graph(fresh, g, roots, c));
        var out = bridge::Graph {}; assert(bridge::export_graph(fresh, [source::number(7)], out, c)); exported[narrow] = c.visits;
    }
    assert(exported[0] == exported[1]);
    for limit in 7 {
        let a = import_wire(heap, wire, limit, whole, full.visits - 1); let b = import_wire(heap, bytes, limit, whole, full.visits - 1);
        let expected = if limit == 0 { bridge::Error.Nodes } else if limit == 1 { bridge::Error.Depth } else if limit == 2 { bridge::Error.Bytes } else if limit == 3 { bridge::Error.Work } else { bridge::Error.Heap };
        assert(a.error == expected && b.error == expected && b.taken == [0 as i64; 4]);
    }
    // Cancellation at every visit before the last refuses it with no change.
    for cut in part.visits {
        let before = taken(heap); var c = bridge::Control { cancel_after: cut }; var g = bridge::Graph {}; var roots: source::Value[>..] = [];
        assert(!(bridge::decode(g, bytes, c) && bridge::import_graph(heap, g, roots, c)) && c.error == bridge::Error.Cancelled && taken(heap) == before);
    }
    // Malformed counts: no nodes, more child slots than nodes, fewer units
    // than name units, more than two string rows a node, units without a
    // string row, a depth without a kept ancestor or past every node.
    for mode in 8 {
        var bad = copy(bytes);
        let at = [32, 36, 40, 44, 44, 52, 52, 48][mode]; let value = [0, 6, 2, 11, 0, 1, 9, 8][mode];
        for i in 4 { bad[at + i] = ((value >> (i * 8)) & 255) as u8; }
        var c = bridge::Control {}; var g = bridge::Graph {}; assert(!bridge::decode(g, bad, c) && c.error == bridge::Error.Malformed && g.nodes.len == 0);
    }
    var cells = bridge::Control { cells: true }; var g = bridge::Graph {}; assert(!bridge::decode(g, bytes, cells) && cells.error == bridge::Error.Malformed);
    var valid = bridge::Control {}; narrowed.omitted.children = 6; assert(!bridge::validate(narrowed, valid) && valid.error == bridge::Error.Malformed);
    print("omission record: same polls, work, limit failures and rows taken as the whole graph; malformed counts refused");
}
// Fifty strings sharing one 100-unit range copy 5,000 units on import, and
// the import is charged for them: one visit short refuses it with no change.
thread_fn shared_text_work() {
    defer { heap_program::finish(); }
    var g = bridge::Graph {}; for i in 100 { g.units.push(97); }
    for i in 50 { g.nodes.push(bridge::Form { tag: form_tag::String, start: 0, count: 100 }); g.children.push(i); }
    g.nodes.push(bridge::Form { tag: form_tag::Vector, start: 0, count: 50 }); g.roots.push(50);
    var code = source::Code {}; code_init(code); var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var probe = bridge::Control {}; assert(bridge::validate(g, probe)); let validation = probe.visits;
    let latin1 = heap.latin1.len; let nodes = heap.nodes.len;
    var short = bridge::Control { work: validation + 51 + 50 + 5000 + 1 - 1 }; var none: source::Value[>..] = [];
    assert(!bridge::import_graph(heap, g, none, short) && short.error == bridge::Error.Work && none.len == 0 && heap.latin1.len == latin1 && heap.nodes.len == nodes);
    var exact = bridge::Control { work: validation + 51 + 50 + 5000 + 1 }; var roots: source::Value[>..] = [];
    assert(bridge::import_graph(heap, g, roots, exact) && roots.len == 1 && heap.latin1.len == latin1 + 5000);
}
thread_fn assembly() {
    defer { heap_program::finish(); }
    var code = source::Code {}; code_init(code); var out = bridge::Graph {};
    {
        var heap = source::Heap { code: code }; assert(source::initialize(heap));
        var graph = bridge::Graph {}; graph_init(graph);
        var roots: source::Value[>..] = []; var c = bridge::Control {}; assert(bridge::import_graph(heap, graph, roots, c));
        let old = roots[0]; let a = source::form_nth(heap, old, 0); let b = source::form_nth(heap, old, 1);
        let left = source::form_collection(heap, node_kind::List, [a, b, a]);
        let right = source::form_collection(heap, node_kind::Vector, [b]);
        let joined = source::form_op(heap, form_op::CljConcat, [left, right]);
        let nested = source::form_op(heap, form_op::CljConcatAll, [source::form_collection(heap, node_kind::List, [source::nil(), joined, source::form_collection(heap, node_kind::List, []), right])]);
        assert(source::form_count(heap, nested) == 5 && source::form_count(heap, left) == 3 && source::form_count(heap, right) == 1);
        for i in 5 { assert(source::form_nth(heap, nested, i).id() == (if i == 0 || i == 2 { a.id() } else { b.id() })); }
        let empty = source::form_op(heap, form_op::CljConcat, [source::nil(), source::form_collection(heap, node_kind::List, [])]);
        assert(source::form_count(heap, empty) == 0 && source::form_op(heap, form_op::CljSeq, [empty]).is_null());
        assert(source::native_value(heap, 16, [nested]).id() == nested.id());
        assert(source::native_value(heap, 16, [left]).id() == left.id());
        assert(source::form_count(heap, source::native_value(heap, 16, [source::nil()])) == 0);
        let map = source::form_collection(heap, node_kind::Map, [a, source::boolean(false), source::number(8), source::nil()]);
        assert(source::form_contains(heap, map, b) && source::form_contains(heap, map, source::number(8)) && !source::form_contains(heap, map, source::number(9)));
        let set = source::form_collection(heap, node_kind::Set, [a]); assert(source::form_contains(heap, set, b));
        assert(source::form_contains(heap, right, source::number(0)) && !source::form_contains(heap, right, source::number(1)) && !source::form_contains(heap, source::nil(), a));
        assert(bridge::export_graph(heap, [nested, old], out, c));
        // Every limit is checked before publishing a partial concatenation.
        for mode in 3 {
            let values = heap.values.len; let nodes = heap.nodes.len; let work = heap.work;
            if mode == 0 { heap.limits.values = values + 3; }
            if mode == 1 { heap.limits.nodes = nodes; }
            if mode == 2 { heap.limits.work = work + 3; }
            source::form_concat(heap, [left, right]);
            assert(heap.fault == (if mode == 2 { source::Fault.WorkLimit } else { source::Fault.MemoryLimit }));
            assert(heap.values.len == values && heap.nodes.len == nodes);
            heap.fault = source::Fault.None; heap.limits = source::Limits {};
        }
        let before = heap.work; source::form_concat(heap, [left, right]); let required = heap.work - before;
        for cut in required {
            let values = heap.values.len; let nodes = heap.nodes.len;
            heap.limits.work = heap.work + cut;
            source::form_concat(heap, [left, right]);
            assert(heap.fault == source::Fault.WorkLimit && heap.values.len == values && heap.nodes.len == nodes);
            heap.fault = source::Fault.None; heap.limits = source::Limits {};
        }
        source::form_concat(heap, [map]); assert(heap.fault == source::Fault.UnsupportedOperand);
    }
    // Occurrence metadata/provenance/aliases remain usable after heap teardown.
    var c = bridge::Control {}; assert(bridge::validate(out, c));
    let joined = out.nodes[out.roots[0]]; let original = out.nodes[out.roots[1]];
    assert(joined.count == 5 && original.metadata >= 0);
    let a = out.children[joined.start]; let b = out.children[joined.start + 1];
    assert(a != b && a == out.children[joined.start + 2] && b == out.children[joined.start + 4]);
    assert(a == out.children[original.start] && b == out.children[original.start + 1]);
    assert(out.nodes[a].span_start == 0 && out.nodes[b].span_start == 4 && out.nodes[a].source != out.nodes[b].source);
    print("assembly: nested/nil/empty concat, presence, immutable occurrence aliases, teardown provenance and work/retention rejection passed");
}

thread_fn adversarial_forms() {
    defer { heap_program::finish(); }
    // db6f7f07:tests/parser.rs's independent breadth/depth/decoded-text
    // budgets, translated to owned GFB2 graphs rather than another parser.
    var g = bridge::Graph {}; g.bytes.append("é😀"); g.sources.push(source::SourceRange { start: 0, count: g.bytes.len as u32 });
    g.units.append([233 as u16, 55357, 56832, 55296]);
    g.nodes.push(bridge::Form { tag: form_tag::String, count: 4, source: 0, span_start: 0, span_end: g.bytes.len }); g.roots.push(0);
    var complete = bridge::Control {}; var bytes: u8[>..] = []; assert(bridge::encode(g, bytes, complete));
    var code = source::Code {}; code_init(code); var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var roots: source::Value[>..] = []; var committed = bridge::Control {}; assert(bridge::import_graph(heap, g, roots, committed));
    let original_nodes = copy(heap.nodes); let original_units = copy(heap.units); let original_latin1 = copy(heap.latin1); let original_work = heap.work;
    for mode in 13 {
        var bad = copy(bytes);
        let at = [8, 12, 16, 20, 24, 28, 32, 44, 48, 68, 72, 76, bytes.len - 4][mode];
        for i in 4 { bad[at + i] = ((2147483647 >> (i * 8)) & 255) as u8; }
        var c = bridge::Control {}; var graph = bridge::Graph {}; assert(!bridge::decode(graph, bad, c) && graph.nodes.len == 0);
        assert(source::plain_rows(heap.nodes) == source::plain_rows(original_nodes) && heap.units == original_units && heap.latin1 == original_latin1 && heap.work == original_work);
    }
    // Valid UTF-8 source spans cannot split either multibyte code point;
    // runtime UTF-16 can still retain an unpaired surrogate unchanged.
    for start in [1, 3, 4, 5] {
        g.nodes[0].span_start = start; var c = bridge::Control {}; var no: source::Value[>..] = [];
        assert(!bridge::import_graph(heap, g, no, c) && c.error == bridge::Error.Malformed && no.len == 0);
        assert(heap.nodes.len == original_nodes.len && heap.units == original_units && heap.latin1 == original_latin1 && heap.work == original_work);
    }
    g.nodes[0].span_start = 0; g.bytes[0] = 255;
    var invalid = bridge::Control {}; assert(!bridge::validate(g, invalid));
    for width in [1, 256] {
        var wide = bridge::Graph {}; wide.nodes.push(bridge::Form { tag: form_tag::String, start: 0, count: 4 }); wide.units.append([233 as u16, 55357, 56832, 55296]);
        for i in width { wide.children.push(0); }
        wide.nodes.push(bridge::Form { tag: form_tag::Vector, count: width }); wide.roots.push(1);
        var c = bridge::Control { nodes: 2, depth: 2 }; assert(bridge::validate(wide, c));
        var shallow = bridge::Control { depth: 1 }; assert(!bridge::validate(wide, shallow) && shallow.error == bridge::Error.Depth);
        var nodes = bridge::Control { nodes: 1, depth: 1024 }; assert(!bridge::validate(wide, nodes) && nodes.error == bridge::Error.Nodes);
        var text = bridge::Control { depth: 1024, bytes: bridge::footprint(wide) - 1 }; assert(!bridge::validate(wide, text) && text.error == bridge::Error.Bytes);
    }
    var deep = bridge::Graph {}; deep.nodes.push(bridge::Form { tag: form_tag::Nil });
    for i in 256 { deep.children.push(i); deep.nodes.push(bridge::Form { tag: form_tag::Vector, start: i, count: 1 }); } deep.roots.push(256);
    var limit = bridge::Control { depth: 128 }; assert(!bridge::validate(deep, limit) && limit.error == bridge::Error.Depth);
    var enough = bridge::Control { depth: 257 }; assert(bridge::validate(deep, enough));
    var nodes = bridge::Control { nodes: 256, depth: 257 }; assert(!bridge::validate(deep, nodes) && nodes.error == bridge::Error.Nodes);
    print("adversarial forms: corrupt fields/UTF8 spans preserve committed heap; width=256/depth=257/text/node budgets independent; lone surrogate legal");
}

// light-cljs-918: the plain form operations (form_ops.goose), each on the
// operands cljs.core gives it and its errors: a HostEffect fault, which
// core_error words (the compiler tests compare that wording and the
// results with the V8 build), a strict operation's TypeError, or a
// NativeBoundary for an operand this runtime does not implement.
fn holds(heap: source::Heap&, which: i64, v: source::Value) -> bool { let r = source::form_op(heap, which, [v]); assert(heap.fault == source::Fault.None && r.is_bool()); return r.flag(); }
fn raises(heap: source::Heap&, which: i64, args: const source::Value[:], fault: source::Fault) { source::form_op(heap, which, args); assert(heap.fault == fault); heap.fault = source::Fault.None; }
fn spelled(heap: source::Heap&, v: source::Value, text: const u8[:]) -> bool { return v.is_string() && source::same_text(heap, v, source::text(heap, text)); }
fn named_as(heap: source::Heap&, v: source::Value, kind: i64, ns: const u8[:], name: const u8[:]) -> bool {
    if source::form_kind(heap, v) != kind { return false; }
    let n = source::node(heap, v.id());
    return spelled(heap, n.value, name) && (if ns.len == 0 { n.namespace_value.is_null() } else { spelled(heap, n.namespace_value, ns) });
}
thread_fn plain_form_operations() {
    defer { heap_program::finish(); }
    var code = source::Code {}; code_init(code); var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let zero = source::number(0); let one = source::number(1); let two = source::number(2);
    let a = source::form_op(heap, form_op::Keyword, [source::nil(), source::text(heap, "a")]); let b = source::form_op(heap, form_op::Keyword, [source::nil(), source::text(heap, "b")]);
    let sym = source::form_op(heap, form_op::Symbol, [source::text(heap, "n"), source::text(heap, "s")]);
    let vector = source::form_collection(heap, node_kind::Vector, [one, two]); let list = source::form_collection(heap, node_kind::List, [one, two]);
    let map = source::form_collection(heap, node_kind::Map, [a, one, b, two]); let set = source::form_collection(heap, node_kind::Set, [one, two]);
    let map1 = source::form_collection(heap, node_kind::Map, [a, one]);
    let empty_list = source::form_collection(heap, node_kind::List, []); let empty_vector = source::form_collection(heap, node_kind::Vector, []);
    let object = source::allocate(heap, source::Node { kind: node_kind::Object });
    let atom = source::allocate(heap, source::Node { kind: node_kind::Atom, value: one }); let volatile = source::allocate(heap, source::Node { kind: node_kind::Atom, value: one, length: 1 });
    let lazy = source::allocate(heap, source::Node { kind: node_kind::LazySeq });
    let hashed = source::form_ordered(heap, node_kind::Map, [a, one, b, two], true);
    assert(heap.fault == source::Fault.None);

    // A map's seq is map entries, in traversal order; a list is its own seq
    // and its rest a list; a set's and a string's seqs are their items.
    let entries = source::form_op(heap, form_op::CljSeq, [map]); let entry = source::form_op(heap, form_op::CljFirst, [map]);
    assert(source::form_count(heap, entries) == 2 && source::form_equal(heap, entry, source::form_collection(heap, node_kind::Vector, [a, one])) && source::form_equal(heap, source::form_nth(heap, entries, 1), source::form_collection(heap, node_kind::Vector, [b, two])));
    assert(source::form_count(heap, source::form_op(heap, form_op::CljRest, [map])) == 1 && source::form_op(heap, form_op::CljNext, [map1]).is_null() && source::form_op(heap, form_op::CljFirst, [set]).num() == 1);
    assert(source::form_op(heap, form_op::CljSeq, [list]).id() == list.id() && source::form_kind(heap, source::form_op(heap, form_op::CljRest, [list])) == node_kind::List);
    let chars = source::form_op(heap, form_op::CljSeq, [source::text(heap, "ab")]); assert(source::form_count(heap, chars) == 2 && spelled(heap, source::form_nth(heap, chars, 1), "b"));
    assert(heap.fault == source::Fault.None);

    // Predicates, a lazy sequence's without realizing it.
    assert(holds(heap, form_op::CljIsList, list) && !holds(heap, form_op::CljIsList, vector) && !holds(heap, form_op::CljIsList, entries) && !holds(heap, form_op::CljIsList, lazy));
    assert(holds(heap, form_op::CljIsSeq, list) && holds(heap, form_op::CljIsSeq, entries) && holds(heap, form_op::CljIsSeq, lazy) && !holds(heap, form_op::CljIsSeq, vector) && !holds(heap, form_op::CljIsSeq, source::nil()));
    assert(holds(heap, form_op::CljIsSequential, vector) && holds(heap, form_op::CljIsSequential, lazy) && !holds(heap, form_op::CljIsSequential, map) && !holds(heap, form_op::CljIsSequential, set));
    assert(holds(heap, form_op::CljIsColl, map) && holds(heap, form_op::CljIsColl, set) && holds(heap, form_op::CljIsColl, entries) && !holds(heap, form_op::CljIsColl, one) && !holds(heap, form_op::CljIsColl, a) && !holds(heap, form_op::CljIsColl, object));
    assert(holds(heap, form_op::CljIsVector, vector) && holds(heap, form_op::CljIsVector, entry) && !holds(heap, form_op::CljIsVector, list));
    assert(holds(heap, form_op::CljIsMapEntry, entry) && !holds(heap, form_op::CljIsMapEntry, vector) && !holds(heap, form_op::CljIsMapEntry, map));
    assert(holds(heap, form_op::CljIsEditable, vector) && !holds(heap, form_op::CljIsEditable, entry) && holds(heap, form_op::CljIsEditable, map) && holds(heap, form_op::CljIsEditable, set) && !holds(heap, form_op::CljIsEditable, list));
    assert(holds(heap, form_op::CljIsZero, zero) && holds(heap, form_op::CljIsZero, source::number(-0.0)) && !holds(heap, form_op::CljIsZero, one) && !holds(heap, form_op::CljIsZero, source::nil()));
    assert(holds(heap, form_op::CljIsFalse, source::boolean(false)) && !holds(heap, form_op::CljIsFalse, source::nil()) && !holds(heap, form_op::CljIsFalse, zero));
    assert(holds(heap, form_op::CljIsAtom, atom) && !holds(heap, form_op::CljIsAtom, volatile) && !holds(heap, form_op::CljIsAtom, map) && !holds(heap, form_op::CljIsAtom, object));
    assert(holds(heap, form_op::IsArrayMap, map) && holds(heap, form_op::IsArrayMap, set) && !holds(heap, form_op::IsArrayMap, hashed) && !holds(heap, form_op::IsArrayMap, vector));
    assert(!holds(heap, form_op::HasHostKeys, map) && holds(heap, form_op::HasHostKeys, source::form_collection(heap, node_kind::Map, [object, one])) && holds(heap, form_op::HasHostKeys, source::form_collection(heap, node_kind::Set, [atom])));
    let reduced = source::form_op(heap, form_op::CljReduced, [vector]);
    assert(holds(heap, form_op::CljIsReduced, reduced) && !holds(heap, form_op::CljIsReduced, vector) && source::form_op(heap, form_op::CljUnreduced, [reduced]).id() == vector.id() && source::form_op(heap, form_op::CljUnreduced, [two]).num() == 2);
    // Every other form operation of a lazy sequence stays a boundary.
    raises(heap, form_op::CljCons, [one, lazy], source::Fault.NativeBoundary);
    // The operations that only wrap, return or refuse an operand take a
    // lazy sequence as it is; formKind names a reduced marker.
    assert(holds(heap, form_op::CljIsReduced, source::form_op(heap, form_op::CljReduced, [lazy])) && source::form_op(heap, form_op::CljUnreduced, [lazy]).id() == lazy.id() && source::form_op(heap, form_op::CljKeyword, [lazy]).is_null());
    raises(heap, form_op::Name, [lazy], source::Fault.TypeError); raises(heap, form_op::CljSymbol, [lazy], source::Fault.HostEffect);
    assert(spelled(heap, source::form_op(heap, form_op::Kind, [reduced]), "reduced"));
    // A map entry's index is exactly 0 or 1; its call takes a fallback
    // and one or two arguments.
    assert(source::form_op(heap, form_op::CljNthOr, [entry, source::number(0.5), b]).id() == b.id() && source::form_op(heap, form_op::CljInvoke, [entry, two, b, two]).id() == b.id() && source::form_op(heap, form_op::CljInvoke, [entry, zero, b, one]).id() == a.id());
    raises(heap, form_op::CljInvoke, [entry, source::number(0.5), source::nil(), one], source::Fault.HostEffect); raises(heap, form_op::CljInvoke, [entry, a, source::nil(), one], source::Fault.HostEffect);
    raises(heap, form_op::CljInvoke, [entry, one, source::nil(), source::number(3)], source::Fault.HostEffect);

    // Names: a keyword's or symbol's; the constructors as cljs.core's.
    assert(spelled(heap, source::form_op(heap, form_op::Name, [sym]), "s") && spelled(heap, source::form_op(heap, form_op::Namespace, [sym]), "n") && source::form_op(heap, form_op::Namespace, [a]).is_null());
    raises(heap, form_op::Name, [one], source::Fault.TypeError); raises(heap, form_op::Namespace, [list], source::Fault.TypeError);
    assert(named_as(heap, source::form_op(heap, form_op::CljKeyword, [source::text(heap, "x/y")]), node_kind::Keyword, "x", "y") && named_as(heap, source::form_op(heap, form_op::CljKeyword, [source::text(heap, "/")]), node_kind::Keyword, "", "/"));
    assert(named_as(heap, source::form_op(heap, form_op::CljKeyword, [sym]), node_kind::Keyword, "n", "s") && source::form_op(heap, form_op::CljKeyword, [a]).id() == a.id() && source::form_op(heap, form_op::CljKeyword, [one]).is_null());
    assert(named_as(heap, source::form_op(heap, form_op::CljKeywordNs, [a, sym]), node_kind::Keyword, "a", "s") && named_as(heap, source::form_op(heap, form_op::CljKeywordNs, [source::nil(), source::text(heap, "c")]), node_kind::Keyword, "", "c"));
    raises(heap, form_op::CljKeywordNs, [one, source::text(heap, "c")], source::Fault.HostEffect); raises(heap, form_op::CljKeyword, [source::text(heap, "a/b/c")], source::Fault.NativeBoundary);
    assert(named_as(heap, source::form_op(heap, form_op::CljSymbol, [source::text(heap, "x/y/z")]), node_kind::Symbol, "x", "y/z") && named_as(heap, source::form_op(heap, form_op::CljSymbol, [source::text(heap, "/")]), node_kind::Symbol, "", "/"));
    let qualified = source::form_op(heap, form_op::CljKeywordNs, [source::text(heap, "q"), source::text(heap, "r")]);
    assert(named_as(heap, source::form_op(heap, form_op::CljSymbol, [qualified]), node_kind::Symbol, "q", "r") && source::form_op(heap, form_op::CljSymbol, [sym]).id() == sym.id());
    raises(heap, form_op::CljSymbol, [one], source::Fault.HostEffect); raises(heap, form_op::CljSymbol, [source::text(heap, "/a")], source::Fault.NativeBoundary);
    assert(named_as(heap, source::form_op(heap, form_op::CljSymbolNs, [source::nil(), source::text(heap, "c")]), node_kind::Symbol, "", "c"));
    raises(heap, form_op::CljSymbolNs, [a, source::text(heap, "c")], source::Fault.HostEffect); raises(heap, form_op::CljSymbolNs, [source::nil(), one], source::Fault.HostEffect);

    // cons, cljList and cljDrop.
    let consed = source::form_op(heap, form_op::CljCons, [zero, vector]);
    assert(source::form_kind(heap, consed) == node_kind::List && source::form_count(heap, consed) == 3 && source::form_nth(heap, consed, 0).num() == 0 && source::form_nth(heap, consed, 2).num() == 2);
    assert(source::form_count(heap, source::form_op(heap, form_op::CljCons, [zero, source::nil()])) == 1 && holds(heap, form_op::CljIsMapEntry, source::form_nth(heap, source::form_op(heap, form_op::CljCons, [zero, map]), 1)));
    // A cons keeps its list or sequence as its rest and is no stack; one
    // onto nil is a list; a conj onto a cons is a cons whose rest is it.
    let onto = source::form_op(heap, form_op::CljCons, [zero, list]);
    assert(source::form_op(heap, form_op::CljRest, [onto]).id() == list.id() && source::form_op(heap, form_op::CljNext, [onto]).id() == list.id() && holds(heap, form_op::CljIsList, onto));
    assert(source::form_kind(heap, source::form_op(heap, form_op::CljRest, [consed])) == node_kind::Concat && source::form_op(heap, form_op::CljPeek, [source::form_op(heap, form_op::CljCons, [zero, source::nil()])]).num() == 0);
    assert(source::form_op(heap, form_op::CljRest, [source::form_conj(heap, onto, two)]).id() == onto.id());
    raises(heap, form_op::CljPeek, [consed], source::Fault.HostEffect); raises(heap, form_op::CljPop, [onto], source::Fault.HostEffect);
    // A keyword cljs.core/keyword makes is fresh: equal, not identical.
    let fresh = source::form_op(heap, form_op::CljKeyword, [source::text(heap, "a")]);
    assert(source::form_equal(heap, fresh, a) && !source::same_value(heap, fresh, a) && !source::same_value(heap, fresh, source::form_op(heap, form_op::CljKeyword, [source::text(heap, "a")])) && source::same_value(heap, a, source::form_op(heap, form_op::Keyword, [source::nil(), source::text(heap, "a")])));
    raises(heap, form_op::CljCons, [zero, one], source::Fault.HostEffect); raises(heap, form_op::CljCons, [zero, object], source::Fault.NativeBoundary);
    assert(source::form_equal(heap, source::form_op(heap, form_op::CljList, [set]), list) && source::form_kind(heap, source::form_op(heap, form_op::CljList, [source::nil()])) == node_kind::List);
    let dropped = source::form_op(heap, form_op::CljDrop, [vector, one]);
    assert(source::form_kind(heap, dropped) == node_kind::Concat && source::form_count(heap, dropped) == 1 && source::form_nth(heap, dropped, 0).num() == 2);
    assert(source::form_count(heap, source::form_op(heap, form_op::CljDrop, [vector, source::number(5)])) == 0 && source::form_op(heap, form_op::CljDrop, [list, one]).is_null());
    raises(heap, form_op::CljDrop, [vector, zero], source::Fault.TypeError);

    // peek and pop: a vector's end, a list's front; metadata stays.
    let marked = source::form_op(heap, form_op::WithMeta, [vector, map1]);
    assert(source::form_op(heap, form_op::CljPeek, [vector]).num() == 2 && source::form_op(heap, form_op::CljPeek, [list]).num() == 1 && source::form_op(heap, form_op::CljPeek, [source::nil()]).is_null() && source::form_op(heap, form_op::CljPeek, [empty_vector]).is_null());
    let popped = source::form_op(heap, form_op::CljPop, [marked]);
    assert(source::form_count(heap, popped) == 1 && source::form_op(heap, form_op::CljMeta, [popped]).id() == map1.id() && source::form_op(heap, form_op::CljPop, [source::nil()]).is_null());
    let tail = source::form_op(heap, form_op::CljPop, [list]); assert(source::form_kind(heap, tail) == node_kind::List && source::form_nth(heap, tail, 0).num() == 2);
    assert(!holds(heap, form_op::CljIsMapEntry, source::form_op(heap, form_op::CljPop, [entry])));
    raises(heap, form_op::CljPop, [empty_vector], source::Fault.HostEffect); raises(heap, form_op::CljPop, [empty_list], source::Fault.HostEffect);
    raises(heap, form_op::CljPeek, [one], source::Fault.HostEffect); raises(heap, form_op::CljPop, [map], source::Fault.HostEffect);

    // dissoc and disj: absent keys give the collection itself; metadata and
    // traversal order stay.
    let removed = source::form_op(heap, form_op::CljDissoc, [map, a]);
    assert(source::form_count(heap, removed) == 1 && source::form_get(heap, removed, b, source::nil()).num() == 2 && source::form_op(heap, form_op::CljDissoc, [map, source::text(heap, "z")]).id() == map.id() && source::form_op(heap, form_op::CljDissoc, [source::nil(), a]).is_null());
    assert(order_of(heap, source::form_op(heap, form_op::Dissoc, [hashed, a])) == map_order::Hash && source::form_count(heap, source::form_op(heap, form_op::CljDisj, [set, one])) == 1 && source::form_op(heap, form_op::Disj, [set, zero]).id() == set.id());
    // A positional map's or set's keys equal once read all go.
    let doubled = source::form_op(heap, form_op::CljDissoc, [source::form_collection(heap, node_kind::Map, [one, a, one, b, two, a]), one]);
    assert(source::form_count(heap, doubled) == 1 && !source::form_contains(heap, doubled, one) && source::form_count(heap, source::form_op(heap, form_op::CljDisj, [source::form_collection(heap, node_kind::Set, [one, one, two]), one])) == 1);
    raises(heap, form_op::CljDissoc, [vector, one], source::Fault.HostEffect); raises(heap, form_op::CljDisj, [map, one], source::Fault.HostEffect);
    raises(heap, form_op::Dissoc, [source::nil(), a], source::Fault.TypeError); raises(heap, form_op::Disj, [map, one], source::Fault.TypeError);

    // formEmpty, formKeys and formLiteralSet.
    let emptied = source::form_op(heap, form_op::Empty, [hashed]); assert(source::form_kind(heap, emptied) == node_kind::Map && source::form_count(heap, emptied) == 0 && order_of(heap, emptied) == map_order::Hash);
    assert(source::form_equal(heap, source::form_op(heap, form_op::Keys, [map]), source::form_collection(heap, node_kind::Vector, [a, b])) && source::form_count(heap, source::form_op(heap, form_op::LiteralSet, [vector])) == 2);
    assert(source::form_kind(heap, source::form_op(heap, form_op::LiteralSet, [empty_vector])) == node_kind::Set);
    raises(heap, form_op::Empty, [vector], source::Fault.TypeError); raises(heap, form_op::Keys, [list], source::Fault.TypeError); raises(heap, form_op::LiteralSet, [map], source::Fault.TypeError);

    // with-meta in macro code: nil on a collection without metadata is the
    // collection; a map entry's copy is an ordinary vector.
    assert(source::form_op(heap, form_op::CljWithExportedMeta, [vector, source::nil()]).id() == vector.id());
    let cleared = source::form_op(heap, form_op::CljWithExportedMeta, [marked, source::nil()]); assert(cleared.id() != marked.id() && source::form_op(heap, form_op::CljMeta, [cleared]).is_null());
    let plain = source::form_op(heap, form_op::CljWithExportedMeta, [entry, map1]); assert(!holds(heap, form_op::CljIsMapEntry, plain) && source::form_op(heap, form_op::CljMeta, [plain]).id() == map1.id());
    assert(source::form_op(heap, form_op::CljWithExportedMeta, [sym, source::nil()]).id() != sym.id());
    raises(heap, form_op::CljWithExportedMeta, [a, map1], source::Fault.TypeError); raises(heap, form_op::CljWithExportedMeta, [vector, one], source::Fault.TypeError);

    // hash as the V8 build's (signed); an identity hash; compare.
    assert(source::form_op(heap, form_op::Hash, [a]).num() == -2123407586 && source::form_op(heap, form_op::Hash, [source::text(heap, "a")]).num() == 1455541201 && source::form_op(heap, form_op::Hash, [vector]).num() == 1185709346);
    assert(source::form_op(heap, form_op::Hash, [source::form_op(heap, form_op::CljFirst, [map1])]).num() == -325413422 && source::form_op(heap, form_op::Hash, [source::nil()]).num() == 0);
    raises(heap, form_op::Hash, [object], source::Fault.NativeBoundary);
    // An image row's identity hash is stable; a job row's fails by name.
    // (The rows so far become this heap's image, as a worker's are.)
    heap.image = source::tops(heap);
    let p = source::object(source::object_prototype_row()); let f = source::object(source::function_prototype_row());
    assert(p.id() < heap.image.nodes && f.id() < heap.image.nodes && source::form_op(heap, form_op::IdentityHash, [one]).num() == 0 && source::form_op(heap, form_op::IdentityHash, [p]).num() == source::form_op(heap, form_op::IdentityHash, [p]).num() && source::form_op(heap, form_op::IdentityHash, [p]).num() != source::form_op(heap, form_op::IdentityHash, [f]).num());
    raises(heap, form_op::IdentityHash, [source::form_collection(heap, node_kind::Vector, [one])], source::Fault.NativeBoundary);
    assert(source::form_op(heap, form_op::Compare, [one, two]).num() == -1 && source::form_op(heap, form_op::Compare, [two, one]).num() == 1 && source::form_op(heap, form_op::Compare, [one, one]).num() == 0);
    assert(source::form_op(heap, form_op::Compare, [source::text(heap, "a"), source::text(heap, "b")]).num() == -1 && source::form_op(heap, form_op::Compare, [a, b]).num() == -1 && source::form_op(heap, form_op::Compare, [sym, sym]).num() == 0);
    assert(source::form_op(heap, form_op::Compare, [source::nil(), one]).num() == -1 && source::form_op(heap, form_op::Compare, [one, source::nil()]).num() == 1 && source::form_op(heap, form_op::Compare, [source::boolean(true), source::boolean(false)]).num() == 1);
    assert(source::form_op(heap, form_op::Compare, [vector, source::form_collection(heap, node_kind::Vector, [one, source::number(3)])]).num() == -1 && source::form_op(heap, form_op::Compare, [vector, empty_vector]).num() == 1);
    raises(heap, form_op::Compare, [a, one], source::Fault.HostEffect); raises(heap, form_op::Compare, [one, a], source::Fault.HostEffect); raises(heap, form_op::Compare, [a, sym], source::Fault.HostEffect); raises(heap, form_op::Compare, [list, empty_list], source::Fault.HostEffect);
    assert(heap.fault == source::Fault.None);
    print("plain form operations: predicates, names, cons, peek/pop, dissoc/disj, map entries, hash and compare passed");
}
