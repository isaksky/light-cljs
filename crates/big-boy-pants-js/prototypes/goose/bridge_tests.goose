import std;
import .program;
import .source_vm;
import .form_bridge;

fn code_init(code: source::Code&) {
    var image = program::Image {}; assert(program::install(image, embed_file("corpus/closure-captured.pants.gsp")) == program::Error.None); assert(source::load(code, image));
}
fn graph_init(g: bridge::Graph&) {
    // Empty/non-ASCII/supplementary/lone-surrogate UTF-16 is transported as
    // code units. Two equal leaves are distinct occurrences, with one reused.
    g.units.append([233, 55357, 56832, 55296]); g.bytes.append("one two"); g.sources.push(source::SourceRange { start: 0, count: 7 }); g.sources.push(source::SourceRange { start: 0, count: 7 });
    g.nodes.push(bridge::Form { tag: 3, count: 0 });
    g.nodes.push(bridge::Form { tag: 3, start: 0, count: 4 });
    g.nodes.push(bridge::Form { tag: 2, number: 1, origin: 2, source: 0, span_start: 0, span_end: 3 });
    g.nodes.push(bridge::Form { tag: 2, number: 1, origin: 3, source: 1, span_start: 4, span_end: 7 });
    g.nodes.push(bridge::Form { tag: 4, start: 0, count: 1, ns_start: 1, ns_count: 3 });
    g.nodes.push(bridge::Form { tag: 5, start: 0, count: 1 });
    g.children.append([5, 1]); g.nodes.push(bridge::Form { tag: 8, start: 0, count: 2 });
    g.children.append([2, 3, 2, 0, 1, 4, 5]); g.nodes.push(bridge::Form { tag: 7, start: 2, count: 7, metadata: 6 });
    g.roots.push(7);
}
fn roundtrip() {
    var code = source::Code {}; code_init(code); var output = bridge::Graph {};
    {
        var heap = source::Heap { code: code }; assert(source::initialize(heap)); var g = bridge::Graph {}; graph_init(g);
        var c = bridge::Control {}; var roots: source::Value[>..] = []; assert(bridge::import_graph(heap, g, roots, c));
        let old = roots[0]; let a = source::form_nth(heap, old, 0); let b = source::form_nth(heap, old, 1);
        assert(a.a != b.a && source::form_equal(heap, a, b) && source::form_nth(heap, old, 2).a == a.a);
        let changed = source::form_assoc(heap, old, source::number(1), source::number(8));
        assert(source::form_nth(heap, old, 1).a == b.a && source::form_nth(heap, changed, 0).a == a.a && source::form_nth(heap, changed, 2).a == a.a);
        let append = source::form_assoc(heap, old, source::number(7), a); assert(source::form_count(heap, append) == 8 && source::form_nth(heap, append, 7).a == a.a);
        let meta = source::form_op(heap, 33, [old]); assert(source::form_kind(heap, meta) == 7);
        let with_meta = source::form_op(heap, 34, [old, source::nil()]); assert(source::form_op(heap, 33, [with_meta]).tag == 1 && source::form_op(heap, 33, [old]).a == meta.a);
        let map = source::form_assoc(heap, meta, a, old); let map2 = source::form_assoc(heap, map, b, append);
        assert(source::form_count(heap, map) == 2 && source::form_count(heap, map2) == 2 && source::form_get(heap, map, b, source::nil()).a == old.a && source::form_get(heap, map2, a, source::nil()).a == append.a);
        let constructed = source::form_construct(heap, 7, [a, old, b, append]); let set = source::form_construct(heap, 8, [a, b]);
        assert(source::form_count(heap, constructed) == 1 && source::form_get(heap, constructed, a, source::nil()).a == append.a);
        assert(heap.values[heap.nodes[constructed.a].start].a == a.a && source::form_count(heap, set) == 1 && heap.values[heap.nodes[set.a].start].a == a.a);
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
fn positional_map_equality() {
    var code = source::Code {}; code_init(code); var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let a = source::allocate(heap, source::Node { kind: 11, value: source::number(7) });
    let b = source::allocate(heap, source::Node { kind: 11, value: source::number(7) });
    assert(a.a != b.a && source::form_equal(heap, a, b));
    // LiteralMap retains duplicate operands; lookup and equality must both
    // stop at the first equal key, even when a later value would match.
    let first = source::form_op(heap, 65, [source::form_collection(heap, 6, [a, source::number(1), b, source::number(2)])]);
    let swapped = source::form_op(heap, 65, [source::form_collection(heap, 6, [b, source::number(2), a, source::number(1)])]);
    assert(source::form_count(heap, first) == 2 && source::form_count(heap, swapped) == 2);
    assert(source::form_get(heap, first, b, source::nil()).number == 1 && source::form_get(heap, swapped, a, source::nil()).number == 2);
    assert(!source::form_equal(heap, first, swapped) && !source::form_equal(heap, swapped, first));
    assert(!source::form_equal(heap, source::form_collection(heap, 8, [first]), source::form_collection(heap, 8, [swapped])));
    let outer = source::form_construct(heap, 7, [first, source::number(10), swapped, source::number(20)]);
    assert(source::form_count(heap, outer) == 2);
    assert(source::form_get(heap, outer, first, source::nil()).number == 10 && source::form_get(heap, outer, swapped, source::nil()).number == 20);
    let changed = source::form_assoc(heap, outer, swapped, source::number(30));
    assert(source::form_get(heap, changed, first, source::nil()).number == 10 && source::form_get(heap, changed, swapped, source::nil()).number == 30 && source::form_get(heap, outer, swapped, source::nil()).number == 20);
    let ordered = source::form_construct(heap, 7, [a, source::number(1), source::number(8), source::number(2)]);
    let reordered = source::form_construct(heap, 7, [source::number(8), source::number(2), b, source::number(1)]);
    assert(source::form_equal(heap, ordered, reordered) && source::form_equal(heap, reordered, ordered) && heap.fault == source::Fault.None);
    print("positional map equality: first matching key agrees with lookup, nested keys and persistent assoc passed");
}
fn number_bits() {
    var code = source::Code {}; code_init(code); var g = bridge::Graph {};
    for number in [-0.0, 0.0 / 0.0, 1.0 / 0.0, -1.0 / 0.0] { g.roots.push(g.nodes.len); g.nodes.push(bridge::Form { tag: 2, number: number }); }
    var heap = source::Heap { code: code }; assert(source::initialize(heap)); var roots: source::Value[>..] = []; var c = bridge::Control {}; assert(bridge::import_graph(heap, g, roots, c));
    var out = bridge::Graph {}; assert(bridge::export_graph(heap, roots, out, c)); var bytes: u8[>..] = []; assert(bridge::encode(out, bytes, c)); var restored = bridge::Graph {}; assert(bridge::decode(restored, bytes, c));
    for i in 4 { assert(to_bytes([g.nodes[i].number]) == to_bytes([restored.nodes[restored.roots[i]].number])); }
    assert(source::form_equal(heap, source::nil(), source::Value {}));
    assert(source::form_equal(heap, source::number(-0.0), source::number(0.0)));
    assert(!source::form_equal(heap, source::number(0.0 / 0.0), source::number(0.0 / 0.0)));
    assert(source::form_equal(heap, roots[1], roots[1]));
}
fn unsupported_values() {
    var code = source::Code {}; code_init(code); var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let ns = source::text(heap, "a/b"); let name = source::text(heap, "x");
    source::form_op(heap, 1, [ns, name]); assert(heap.fault == source::Fault.NativeBoundary); heap.fault = source::Fault.None;
    let keyword = source::form_op(heap, 0, [source::nil(), name]); source::form_op(heap, 34, [keyword, source::nil()]); assert(heap.fault == source::Fault.TypeError); heap.fault = source::Fault.None;
    let vector = source::form_collection(heap, 6, [source::number(1)]);
    for index in [0.0 / 0.0, 1.0 / 0.0, -1.0 / 0.0, 1e100] {
        assert(source::form_get(heap, vector, source::number(index), source::number(99)).number == 99 && heap.fault == source::Fault.None);
        source::form_assoc(heap, vector, source::number(index), source::nil()); assert(heap.fault == source::Fault.TypeError); heap.fault = source::Fault.None;
        source::form_op(heap, 25, [vector, source::number(index)]); assert(heap.fault == source::Fault.TypeError); heap.fault = source::Fault.None;
    }
    let object = source::allocate(heap, source::Node { kind: 0 }); let container = source::form_collection(heap, 6, [object]);
    var c = bridge::Control {}; var out = bridge::Graph {}; assert(!bridge::export_graph(heap, [container], out, c) && c.error == bridge::Error.Unsupported && out.nodes.len == 0);
    var g = bridge::Graph {}; g.units.append([97, 47, 98]); g.nodes.push(bridge::Form { tag: 4, start: 0, count: 3 }); g.roots.push(0);
    var bad = bridge::Control {}; var roots: source::Value[>..] = []; let before = heap.nodes.len;
    assert(!bridge::import_graph(heap, g, roots, bad) && bad.error == bridge::Error.Unsupported && roots.len == 0 && heap.nodes.len == before);
}
fn primitive_fallback() {
    var code = source::Code {}; code_init(code); var heap = source::Heap { code: code }; assert(source::initialize(heap));
    heap.source_bytes.append("one two"); heap.source_ranges.push(source::SourceRange { start: 0, count: 7 });
    let text = source::string(heap, [233, 55357, 56832, 55296]);
    let argument = source::allocate(heap, source::Node { kind: 11, value: text, origin: 0, source: 0, span_start: 0, span_end: 3 });
    let annotation = source::allocate(heap, source::Node { kind: 11, value: text, origin: 0, source: 0, span_start: 4, span_end: 7 });
    let metadata = source::form_collection(heap, 7, [source::number(1), annotation]);
    let call = source::form_collection(heap, 5, [source::nil(), argument]); heap.nodes[call.a].metadata = metadata;
    var c = bridge::Control {}; var out = bridge::Graph {}; assert(bridge::export_graph(heap, [text], out, c, call));
    // Bare primitive compatibility starts from arguments, not &form metadata.
    assert(out.nodes[out.roots[0]].span_start == 0 && out.nodes[out.roots[0]].origin == 0);
    for cut in c.visits { var cancelled = bridge::Control { cancel_after: cut }; var untouched = bridge::Graph {}; assert(!bridge::export_graph(heap, [text], untouched, cancelled, call) && cancelled.error == bridge::Error.Cancelled && untouched.nodes.len == 0); }
}
fn budgets() {
    var code = source::Code {}; code_init(code); var g = bridge::Graph {}; graph_init(g); var full = bridge::Control {}; assert(bridge::validate(g, full));
    for mode in 4 {
        var c = bridge::Control {}; if mode == 0 { c.nodes = 7; } if mode == 1 { c.depth = 2; } if mode == 2 { c.bytes = bridge::footprint(g) - 1; } if mode == 3 { c.work = full.visits - 1; }
        assert(!bridge::validate(g, c)); assert(c.error == (if mode == 0 { bridge::Error.Nodes } else if mode == 1 { bridge::Error.Depth } else if mode == 2 { bridge::Error.Bytes } else { bridge::Error.Work }));
    }
    var heap = source::Heap { code: code }; assert(source::initialize(heap)); var roots: source::Value[>..] = []; var control = bridge::Control {}; assert(bridge::import_graph(heap, g, roots, control));
    let nodes = heap.nodes.len; let units = heap.units.len; let values = heap.values.len; let text = heap.source_bytes.len;
    var success = bridge::Control {}; var imported: source::Value[>..] = []; assert(bridge::import_graph(heap, g, imported, success));
    let stable_nodes = heap.nodes.len; let stable_units = heap.units.len; let stable_values = heap.values.len; let stable_text = heap.source_bytes.len;
    for cut in success.visits {
        var c = bridge::Control { cancel_after: cut }; var untouched: source::Value[>..] = [];
        assert(!bridge::import_graph(heap, g, untouched, c) && c.error == bridge::Error.Cancelled && untouched.len == 0);
        assert(heap.nodes.len == stable_nodes && heap.units.len == stable_units && heap.values.len == stable_values && heap.source_bytes.len == stable_text);
    }
    heap.limits.nodes = heap.nodes.len; var c = bridge::Control {}; var untouched: source::Value[>..] = [];
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
    let saved = heap.nodes[roots[0].a].metadata; heap.nodes[roots[0].a].metadata = roots[0];
    var cycle_control = bridge::Control {}; var cycle_out = bridge::Graph {};
    assert(!bridge::export_graph(heap, roots, cycle_out, cycle_control, roots[0]) && cycle_control.error == bridge::Error.Depth && cycle_out.nodes.len == 0);
    heap.nodes[roots[0].a].metadata = saved;
}
fn update_case(heap: source::Heap&, operation: i64, old: source::Value, key: source::Value, value: source::Value) -> source::Value {
    if operation <= 3 { return source::form_assoc(heap, old, if operation == 0 { source::number(0) } else if operation == 1 { source::number(2) } else if operation == 2 { key } else { source::number(8) }, value); }
    return source::form_conj(heap, if operation == 6 { source::nil() } else { old }, value);
}
fn direct_update_boundaries() {
    var code = source::Code {}; code_init(code);
    for operation in 7 {
        var cost: i64 = 0;
        for mode in 8 {
            var heap = source::Heap { code: code }; assert(source::initialize(heap));
            let key = source::allocate(heap, source::Node { kind: 11, value: source::number(7) });
            let equal_key = source::allocate(heap, source::Node { kind: 11, value: source::number(7) });
            let metadata = source::form_collection(heap, 7, []);
            let old = if operation == 2 || operation == 3 { source::form_collection(heap, 7, [key, source::number(1), equal_key, source::number(2)]) } else { source::form_collection(heap, if operation == 4 { 5 } else { 6 }, [key, equal_key]) };
            heap.nodes[old.a].metadata = metadata; heap.nodes[old.a].origin = 17; heap.nodes[old.a].source = 3; heap.nodes[old.a].span_start = 11; heap.nodes[old.a].span_end = 22;
            let n = heap.nodes[old.a]; let nodes = heap.nodes.len; let values = heap.values.len; let work = heap.work;
            let count: i64 = if operation == 0 { 2 } else if operation == 1 { 3 } else if operation == 2 { 4 } else if operation == 3 { 6 } else if operation == 6 { 1 } else { 3 };
            if mode == 1 { heap.limits.work = work + cost; heap.limits.nodes = nodes + 1; heap.limits.values = values + count; }
            if mode == 2 || mode == 3 { heap.limits.work = work + cost - (mode - 1); }
            if mode == 4 { heap.limits.nodes = nodes; }
            if mode == 5 { heap.limits.values = values + count - 1; }
            var value = key;
            if mode == 6 { value.session += 1; }
            if mode == 7 {
                if operation == 6 { continue; }
                heap.values[n.start + n.count - 1] = source::Value { tag: 5, a: nodes, session: heap.identity };
            }
            let result = update_case(heap, operation, old, equal_key, value);
            if mode == 0 || mode == 1 {
                assert(heap.fault == source::Fault.None && heap.nodes.len == nodes + 1 && heap.values.len == values + count);
                if mode == 0 { cost = heap.work - work; }
                let updated = heap.nodes[result.a]; assert(updated.start == values && updated.count == count && updated.origin == -1);
                if operation != 6 { assert(updated.metadata.a == metadata.a && updated.source == 3 && updated.span_start == 11 && updated.span_end == 22); }
                assert(heap.nodes[old.a].start == n.start && heap.nodes[old.a].origin == 17 && heap.values[n.start].a == key.a);
                if operation == 0 { assert(heap.values[updated.start].a == key.a && heap.values[updated.start + 1].a == equal_key.a); }
                if operation == 1 || operation == 5 { assert(heap.values[updated.start + 2].a == key.a && heap.values[updated.start + 1].a == equal_key.a); }
                if operation == 2 { assert(heap.values[updated.start].a == key.a && heap.values[updated.start + 1].a == key.a && heap.values[updated.start + 2].a == equal_key.a && heap.values[updated.start + 3].number == 2); }
                if operation == 3 { assert(heap.values[updated.start + 4].number == 8 && heap.values[updated.start + 5].a == key.a); }
                if operation == 4 { assert(heap.values[updated.start].a == key.a && heap.values[updated.start + 1].a == key.a && heap.values[updated.start + 2].a == equal_key.a); }
            } else {
                assert(heap.nodes.len == nodes && heap.values.len == values);
                assert(heap.fault == (if mode == 2 || mode == 3 { source::Fault.WorkLimit } else if mode == 4 || mode == 5 { source::Fault.MemoryLimit } else { source::Fault.StaleID }));
            }
        }
    }
    // A replaced child is not part of the resulting range and was never
    // validated by form_collection. Keep that existing replacement behavior.
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let old = source::form_collection(heap, 6, [source::number(1)]); heap.values[heap.nodes[old.a].start] = source::Value { tag: 5, a: heap.nodes.len, session: heap.identity };
    let fixed = source::form_assoc(heap, old, source::number(0), source::number(2)); assert(heap.fault == source::Fault.None && source::form_nth(heap, fixed, 0).number == 2);
    print("direct persistent updates: exact work/node/value limits, final allocation charge, stale children, duplicate keys, aliases and metadata passed");
}
fn main() { adversarial_forms(); roundtrip(); positional_map_equality(); direct_update_boundaries(); number_bits(); unsupported_values(); primitive_fallback(); budgets(); print("compiler form bridge: owned graphs/metadata/aliases/UTF16/Number/order, exhaustive cancellation/truncation and transactional limits passed"); }

fn adversarial_forms() {
    // db6f7f07:tests/parser.rs's independent breadth/depth/decoded-text
    // budgets, translated to owned GFB1 graphs rather than another parser.
    var g = bridge::Graph {}; g.bytes.append("é😀"); g.sources.push(source::SourceRange { start: 0, count: g.bytes.len });
    g.units.append([233 as u16, 55357, 56832, 55296]);
    g.nodes.push(bridge::Form { tag: 3, count: 4, source: 0, span_start: 0, span_end: g.bytes.len }); g.roots.push(0);
    var complete = bridge::Control {}; var bytes: u8[>..] = []; assert(bridge::encode(g, bytes, complete));
    var code = source::Code {}; code_init(code); var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var roots: source::Value[>..] = []; var committed = bridge::Control {}; assert(bridge::import_graph(heap, g, roots, committed));
    let original_nodes = copy(heap.nodes); let original_units = copy(heap.units); let original_work = heap.work;
    for mode in 13 {
        var bad = copy(bytes);
        let at = [8, 12, 16, 20, 24, 28, 32, 44, 48, 68, 72, 76, bytes.len - 4][mode];
        for i in 4 { bad[at + i] = ((2147483647 >> (i * 8)) & 255) as u8; }
        var c = bridge::Control {}; var graph = bridge::Graph {}; assert(!bridge::decode(graph, bad, c) && graph.nodes.len == 0);
        assert(bytes_of(heap.nodes) == bytes_of(original_nodes) && heap.units == original_units && heap.work == original_work);
    }
    // Valid UTF-8 source spans cannot split either multibyte code point;
    // runtime UTF-16 can still retain an unpaired surrogate unchanged.
    for start in [1, 3, 4, 5] {
        g.nodes[0].span_start = start; var c = bridge::Control {}; var no: source::Value[>..] = [];
        assert(!bridge::import_graph(heap, g, no, c) && c.error == bridge::Error.Malformed && no.len == 0);
        assert(heap.nodes.len == original_nodes.len && heap.units == original_units && heap.work == original_work);
    }
    g.nodes[0].span_start = 0; g.bytes[0] = 255;
    var invalid = bridge::Control {}; assert(!bridge::validate(g, invalid));
    for width in [1, 256] {
        var wide = bridge::Graph {}; wide.nodes.push(bridge::Form { tag: 3, start: 0, count: 4 }); wide.units.append([233 as u16, 55357, 56832, 55296]);
        for i in width { wide.children.push(0); }
        wide.nodes.push(bridge::Form { tag: 7, count: width }); wide.roots.push(1);
        var c = bridge::Control { nodes: 2, depth: 2 }; assert(bridge::validate(wide, c));
        var shallow = bridge::Control { depth: 1 }; assert(!bridge::validate(wide, shallow) && shallow.error == bridge::Error.Depth);
        var nodes = bridge::Control { nodes: 1, depth: 1024 }; assert(!bridge::validate(wide, nodes) && nodes.error == bridge::Error.Nodes);
        var text = bridge::Control { depth: 1024, bytes: bridge::footprint(wide) - 1 }; assert(!bridge::validate(wide, text) && text.error == bridge::Error.Bytes);
    }
    var deep = bridge::Graph {}; deep.nodes.push(bridge::Form { tag: 0 });
    for i in 256 { deep.children.push(i); deep.nodes.push(bridge::Form { tag: 7, start: i, count: 1 }); } deep.roots.push(256);
    var limit = bridge::Control { depth: 128 }; assert(!bridge::validate(deep, limit) && limit.error == bridge::Error.Depth);
    var enough = bridge::Control { depth: 257 }; assert(bridge::validate(deep, enough));
    var nodes = bridge::Control { nodes: 256, depth: 257 }; assert(!bridge::validate(deep, nodes) && nodes.error == bridge::Error.Nodes);
    print("adversarial forms: corrupt fields/UTF8 spans preserve committed heap; width=256/depth=257/text/node budgets independent; lone surrogate legal");
}
