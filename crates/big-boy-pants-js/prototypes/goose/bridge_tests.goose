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
    g.units.append([233, 55357, 56832, 55296]); g.bytes.append("one two"); g.sources.push(source::SourceRange { start: 0, count: 7 });
    g.nodes.push(bridge::Form { tag: 3, count: 0 });
    g.nodes.push(bridge::Form { tag: 3, start: 0, count: 4 });
    g.nodes.push(bridge::Form { tag: 2, number: 1, origin: 2, source: 0, span_start: 0, span_end: 3 });
    g.nodes.push(bridge::Form { tag: 2, number: 1, origin: 3, source: 0, span_start: 4, span_end: 7 });
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
    let unicode = decoded.nodes[decoded.children[root.start + 4]]; assert(decoded.units[unicode.start..(unicode.start + unicode.count)] == [233 as u16, 55357, 56832, 55296]);
    assert(decoded.bytes[..] == "one two");
}
fn number_bits() {
    var code = source::Code {}; code_init(code); var g = bridge::Graph {};
    for number in [-0.0, 0.0 / 0.0, 1.0 / 0.0, -1.0 / 0.0] { g.roots.push(g.nodes.len); g.nodes.push(bridge::Form { tag: 2, number: number }); }
    var heap = source::Heap { code: code }; assert(source::initialize(heap)); var roots: source::Value[>..] = []; var c = bridge::Control {}; assert(bridge::import_graph(heap, g, roots, c));
    var out = bridge::Graph {}; assert(bridge::export_graph(heap, roots, out, c)); var bytes: u8[>..] = []; assert(bridge::encode(out, bytes, c)); var restored = bridge::Graph {}; assert(bridge::decode(restored, bytes, c));
    for i in 4 { assert(to_bytes([g.nodes[i].number]) == to_bytes([restored.nodes[restored.roots[i]].number])); }
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
fn main() { roundtrip(); number_bits(); budgets(); print("compiler form bridge: owned graphs/metadata/aliases/UTF16/Number/order, exhaustive cancellation/truncation and transactional limits passed"); }
