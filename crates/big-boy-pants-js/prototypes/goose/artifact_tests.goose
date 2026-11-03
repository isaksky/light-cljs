import std;
import .artifact;
import .snapshot;
import .snapshot_fixture;
import .source_vm;
import .form_bridge;

fn restored(base: const snapshot::Snapshot&, code: source::Code&, heap: source::Heap&, roots: snapshot::Binding[>..]&) {
    var c = snapshot::Control {}; assert(snapshot::restore(base, code, heap, roots, source::reserve_identity(), c));
}
fn produce(base: const snapshot::Snapshot&, which: i64, c: artifact::Control&) -> artifact::Artifact {
    var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; restored(base, code, heap, roots);
    var imports = artifact::Imports { base: base.header.checkpoint, session: heap.identity };
    let produced = snapshot_test::factory(heap, roots[which].value.number as i64, roots[0].value); heap.producer_count += 1;
    let cell = source::allocate(heap, source::Node { kind: 3, initialized: true, immutable: true, value: source::number(40 + which) });
    let closure = source::closure(heap, roots[4].value.number as i64, [cell]);
    let x = source::allocate(heap, source::Node { kind: 11, value: source::number(1), origin: 70 });
    let y = source::allocate(heap, source::Node { kind: 11, value: source::number(1), origin: 71 });
    let root = source::form_collection(heap, 6, [produced, roots[5].value, closure, x, y, x]);
    let a = artifact::export_graph(heap, base, imports, root, which, 1, c); assert(c.error == artifact::Error.None); return copy(a);
}
fn state(heap: const source::Heap&, imports: const artifact::Imports&) -> u8[>..] {
    var out: u8[>..] = []; let empty: snapshot::Binding[] = []; let view = snapshot::borrow(heap, empty, imports.base);
    to_bytes([view.header], out); to_bytes(heap.nodes, out); to_bytes(heap.values, out); to_bytes(heap.properties, out); to_bytes(heap.units, out); to_bytes(heap.constants, out); to_bytes(heap.source_bytes, out); to_bytes(heap.source_ranges, out);
    to_bytes([imports.base, imports.session], out); to_bytes(imports.entries, out); to_bytes(imports.definitions, out); to_bytes(imports.signatures, out); to_bytes(imports.deliveries, out); return out;
}
fn reject_import(a: const artifact::Artifact&, base: const snapshot::Snapshot&, heap: source::Heap&, imports: artifact::Imports&, error: artifact::Error) {
    let before = state(heap, imports); var c = artifact::Control {}; artifact::import_graph(a, base, heap, imports, c);
    if c.error != error { print("wrong error ", c.error, " expected ", error); } assert(c.error == error && state(heap, imports) == before);
}
fn joins(base: const snapshot::Snapshot&, a: const artifact::Artifact&, b: const artifact::Artifact&) {
    assert(a.keys[0].slot == b.keys[0].slot && a.keys[0].arena != b.keys[0].arena);
    var canonical = bridge::Graph {};
    for reverse in 2 {
        var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; restored(base, code, heap, roots);
        var imports = artifact::Imports { base: base.header.checkpoint, session: heap.identity };
        var ca = artifact::Control {}; var cb = artifact::Control {}; var left = source::Value {}; var right = source::Value {};
        if reverse == 0 { left = artifact::import_graph(a, base, heap, imports, ca); right = artifact::import_graph(b, base, heap, imports, cb); }
        else { right = artifact::import_graph(b, base, heap, imports, cb); left = artifact::import_graph(a, base, heap, imports, ca); }
        assert(ca.error == artifact::Error.None && cb.error == artifact::Error.None && left.a != right.a);
        assert(source::form_nth(heap, left, 1).a == roots[5].value.a && source::form_nth(heap, right, 1).a == roots[5].value.a);
        let before = state(heap, imports); var duplicate = artifact::Control {}; let again = artifact::import_graph(a, base, heap, imports, duplicate);
        assert(duplicate.error == artifact::Error.None && duplicate.copied_bytes == 0 && duplicate.copied_nodes == 0 && duplicate.staged_bytes > 0 && again.a == left.a && state(heap, imports) == before);
        let closure = source::form_nth(heap, left, 2); assert(snapshot_test::invoke(heap, closure, []).number == 41);
        let x = source::form_nth(heap, left, 3); let y = source::form_nth(heap, left, 4);
        assert(x.a != y.a && x.a == source::form_nth(heap, left, 5).a && source::form_equal(heap, x, y));
        let definitions = source::allocate(heap, source::Node { kind: 0 }); assert(source::install_field(heap, heap.global, "__lightCompilerRoots", definitions));
        assert(source::install_field(heap, definitions, "cljs.user/left-result", source::form_nth(heap, left, 0)));
        assert(source::install_field(heap, definitions, "cljs.user/right-result", source::form_nth(heap, right, 0)));
        let consumer = snapshot_test::factory(heap, roots[3].value.number as i64, roots[0].value);
        let result = snapshot_test::invoke(heap, consumer, [source::nil(), source::nil()]);
        let expansion = source::form_nth(heap, result, 1); assert(source::form_count(heap, expansion) == 4);
        for expected, i in [2, 3, 9, 16] { assert(source::form_nth(heap, expansion, i).number == expected); }
        var graph = bridge::Graph {}; var control = bridge::Control {}; assert(bridge::export_graph(heap, [result], graph, control));
        if reverse == 0 { canonical.nodes.append(graph.nodes); canonical.children.append(graph.children); canonical.units.append(graph.units); canonical.bytes.append(graph.bytes); canonical.sources.append(graph.sources); canonical.roots.append(graph.roots); }
        else { assert(bytes_of(graph.nodes) == bytes_of(canonical.nodes) && graph.children == canonical.children && graph.units == canonical.units); }
        // Diamond: each new branch publishes a form referring to the imported A.
        let top = source::form_collection(heap, 6, [left, right, left]); var ec = artifact::Control {};
        let diamond = artifact::export_graph(heap, base, imports, top, 20 + reverse, 1, ec); assert(ec.error == artifact::Error.None);
        var dc = artifact::Control {}; let joined = artifact::import_graph(diamond, base, heap, imports, dc); assert(dc.error == artifact::Error.None);
        assert(source::form_nth(heap, joined, 0).a == left.a && source::form_nth(heap, joined, 2).a == left.a && source::form_nth(heap, joined, 1).a == right.a);
        { var conflict = copy(a); conflict.version = 2; reject_import(conflict, base, heap, imports, artifact::Error.Conflict); }
        { var conflict = copy(a); conflict.nodes[0].origin = 999; reject_import(conflict, base, heap, imports, artifact::Error.Conflict); }
        { var conflict = copy(a); conflict.definition = 99; conflict.nodes[0].origin = 999; reject_import(conflict, base, heap, imports, artifact::Error.Conflict); }
        { var conflict = copy(a); conflict.base += 100; reject_import(conflict, base, heap, imports, artifact::Error.Context); }
        print("artifact join order=", reverse, " import-nodes=", ca.copied_nodes + cb.copied_nodes, " import-bytes=", ca.copied_bytes + cb.copied_bytes, " staged-bytes=", ca.staged_bytes + cb.staged_bytes, " import-ns=", ca.import_ns + cb.import_ns, " diamond-bytes=", dc.copied_bytes, " diamond-ns=", dc.import_ns);
    }
}
fn relay(base: const snapshot::Snapshot&, a: const artifact::Artifact&, definition: i64) -> artifact::Artifact {
    var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; restored(base, code, heap, roots);
    var imports = artifact::Imports { base: base.header.checkpoint, session: heap.identity }; var c = artifact::Control {};
    let shared = artifact::import_graph(a, base, heap, imports, c); assert(c.error == artifact::Error.None);
    let root = source::form_collection(heap, 6, [shared, shared]);
    var export_control = artifact::Control {}; let result = artifact::export_graph(heap, base, imports, root, definition, 1, export_control);
    assert(export_control.error == artifact::Error.None); return copy(result);
}
fn diamond_delivery(base: const snapshot::Snapshot&, a: const artifact::Artifact&) {
    let left = relay(base, a, 51); let right = relay(base, a, 52);
    for reverse in 2 {
        var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; restored(base, code, heap, roots);
        var imports = artifact::Imports { base: base.header.checkpoint, session: heap.identity };
        var lc = artifact::Control {}; var rc = artifact::Control {}; var l = source::Value {}; var r = source::Value {};
        if reverse == 0 { l = artifact::import_graph(left, base, heap, imports, lc); r = artifact::import_graph(right, base, heap, imports, rc); }
        else { r = artifact::import_graph(right, base, heap, imports, rc); l = artifact::import_graph(left, base, heap, imports, lc); }
        assert(lc.error == artifact::Error.None && rc.error == artifact::Error.None && l.a != r.a);
        let shared = source::form_nth(heap, l, 0);
        assert(shared.a == source::form_nth(heap, r, 0).a && shared.a == source::form_nth(heap, l, 1).a);
        let nodes = heap.nodes.len; var direct = artifact::Control {}; let delivered = artifact::import_graph(a, base, heap, imports, direct);
        assert(direct.error == artifact::Error.None && delivered.a == shared.a && heap.nodes.len == nodes);
        var duplicate = artifact::Control {}; let again = artifact::import_graph(right, base, heap, imports, duplicate);
        assert(duplicate.error == artifact::Error.None && again.a == r.a && heap.nodes.len == nodes);
    }
    print("artifact two independent relays/diamond identity/direct and duplicate delivery passed");
}
fn invalid(base: const snapshot::Snapshot&, a: const artifact::Artifact&) {
    var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; restored(base, code, heap, roots);
    var imports = artifact::Imports { base: base.header.checkpoint, session: heap.identity };
    for mode in 13 {
        var bad = copy(a);
        match mode {
            0 => { bad.root.a = bad.nodes.len; },
            1 => { bad.values[0].a = bad.nodes.len; bad.values[0].tag = 5; },
            2 => { bad.nodes[0].kind = 0; },
            3 => { bad.values[bad.nodes[0].start] = bad.root; },
            4 => { bad.schema = 99; },
            5 => { bad.keys[1] = bad.keys[0]; },
            6 => { bad.nodes[0].metadata = source::Value { tag: 5, a: bad.nodes.len }; },
            7 => { bad.nodes[0].start = 9223372036854775807; },
            8 => { bad.nodes[0].source = bad.sources.len; },
            9 => { bad.nodes[0].count = 9223372036854775807; },
            10 => { bad.keys[0].arena = base.header.checkpoint; bad.keys[0].slot = 0; },
            11 => { bad.version = 0; },
            12 => { bad.nodes[0].count -= 1; bad.values[bad.nodes[0].start + bad.nodes[0].count] = source::Value { tag: 5, a: bad.nodes.len }; },
            _ => {},
        }
        reject_import(bad, base, heap, imports, match mode { 2 => artifact::Error.Mutable, 3 => artifact::Error.Cycle, 4, 11 => artifact::Error.Version, 10 => artifact::Error.Conflict, _ => artifact::Error.Link });
    }
    // A rejection must not positively classify any earlier visited sibling.
    {
        var bad = copy(a); bad.definition = 99;
        for &n in bad.nodes { if n.kind == 3 { n.immutable = false; } }
        let before = state(heap, imports); var c = artifact::Control {}; artifact::import_graph(bad, base, heap, imports, c);
        assert(c.error == artifact::Error.Mutable && c.path.len == 3 && state(heap, imports) == before);
        var repaired = artifact::Control {}; artifact::import_graph(a, base, heap, imports, repaired); assert(repaired.error == artifact::Error.None);
        // The destination was already populated when the second rejection ran.
        reject_import(bad, base, heap, imports, artifact::Error.Mutable);
    }
    let mutable = source::allocate(heap, source::Node { kind: 3, initialized: true, value: source::number(7) });
    let wrapped = source::allocate(heap, source::Node { kind: 11, value: source::number(3) });
    let nested = source::form_collection(heap, 6, [wrapped, mutable]);
    let map = source::form_collection(heap, 7, [source::number(1), nested]);
    var c = artifact::Control {}; let rejected = artifact::export_graph(heap, base, imports, map, 30, 1, c);
    assert(c.error == artifact::Error.Mutable && c.path.len == 3 && rejected.nodes.len == 0 && imports.definitions.len == 1);
    let map_key = source::form_collection(heap, 7, [nested, source::number(1)]); var kc = artifact::Control {}; artifact::export_graph(heap, base, imports, map_key, 31, 1, kc); assert(kc.error == artifact::Error.Mutable && kc.path.len == 3);
    let closure = source::closure(heap, roots[4].value.number as i64, [mutable]); var cc = artifact::Control {}; artifact::export_graph(heap, base, imports, closure, 32, 1, cc); assert(cc.error == artifact::Error.Mutable && cc.path.len == 2);
    // An occurrence is scalar-only, but its metadata still participates.
    let metadata = source::form_collection(heap, 7, [source::number(1), mutable]); heap.nodes[wrapped.a].metadata = metadata; var oc = artifact::Control {}; artifact::export_graph(heap, base, imports, wrapped, 33, 1, oc); assert(oc.error == artifact::Error.Mutable && oc.path.len >= 3);
    heap.nodes[wrapped.a].metadata = source::nil();
    {
        heap.nodes[roots[5].value.a].value = source::number(99);
        var changed_base = copy(a); changed_base.definition = 98; reject_import(changed_base, base, heap, imports, artifact::Error.Conflict);
        heap.nodes[roots[5].value.a].value = source::number(9);
        imports.session += 1; reject_import(a, base, heap, imports, artifact::Error.Context); imports.session -= 1;
    }
    for limit in 3 {
        let before = state(heap, imports); var small = artifact::Control {};
        if limit == 0 { small.work = 1; } else if limit == 1 { small.bytes = artifact::payload(a) - 1; } else { heap.limits.nodes = heap.nodes.len; }
        var fresh_definition = copy(a); fresh_definition.definition = 97; if limit == 2 { fresh_definition.keys[0].arena = source::reserve_identity(); }
        let current = state(heap, imports); artifact::import_graph(fresh_definition, base, heap, imports, small);
        assert(small.error == (if limit == 0 { artifact::Error.Work } else { artifact::Error.Bytes }) && state(heap, imports) == current);
        if limit < 2 { assert(current == before); }
    }
    print("artifact malformed/mutable paths/cycle/conflict/atomic-budget rejection passed");
}
fn work_cuts(base: const snapshot::Snapshot&, a: const artifact::Artifact&) {
    var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; restored(base, code, heap, roots);
    var imports = artifact::Imports { base: base.header.checkpoint, session: heap.identity }; var success = artifact::Control {};
    artifact::import_graph(a, base, heap, imports, success); assert(success.error == artifact::Error.None);
    var target_code = source::Code {}; var target = source::Heap { code: target_code }; var target_roots: snapshot::Binding[>..] = []; restored(base, target_code, target, target_roots);
    var cache = artifact::Imports { base: base.header.checkpoint, session: target.identity }; let before = state(target, cache);
    for cut in success.visited {
        var failure = artifact::Control { work: cut }; artifact::import_graph(a, base, target, cache, failure);
        assert(failure.error == artifact::Error.Work && failure.copied_nodes == 0 && failure.copied_bytes == 0 && state(target, cache) == before);
    }
    var exact = artifact::Control { work: success.visited }; artifact::import_graph(a, base, target, cache, exact); assert(exact.error == artifact::Error.None && exact.visited == success.visited);
    print("artifact every import work cut/unchanged heap and caches/exact budget passed: ", success.visited);
}
fn provenance_artifact(base: const snapshot::Snapshot&) -> artifact::Artifact {
    var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; restored(base, code, heap, roots);
    var graph = bridge::Graph {}; graph.bytes.append("one two"); graph.sources.push(source::SourceRange { start: 0, count: 7 });
    graph.units.append([0xD83D, 0xDE00, 0xD800, 'n', 's', 'x']);
    graph.nodes.push(bridge::Form { tag: 2, number: 1, origin: 10, source: 0, span_start: 0, span_end: 3 });
    graph.nodes.push(bridge::Form { tag: 2, number: 1, origin: 11, source: 0, span_start: 4, span_end: 7 });
    graph.nodes.push(bridge::Form { tag: 3, start: 0, count: 3 }); graph.nodes.push(bridge::Form { tag: 8 });
    graph.nodes.push(bridge::Form { tag: 4, start: 5, count: 1, ns_start: 3, ns_count: 2, metadata: 3 });
    graph.children.append([0, 1, 0, 2, 4]); graph.nodes.push(bridge::Form { tag: 7, start: 0, count: 5 }); graph.roots.push(5);
    var bc = bridge::Control {}; var values: source::Value[>..] = []; assert(bridge::import_graph(heap, graph, values, bc));
    var imports = artifact::Imports { base: base.header.checkpoint, session: heap.identity }; var c = artifact::Control {};
    let a = artifact::export_graph(heap, base, imports, values[0], 60, 1, c); assert(c.error == artifact::Error.None); return copy(a);
}
fn provenance(base: const snapshot::Snapshot&) {
    let a = provenance_artifact(base); var result = bridge::Graph {};
    {
        var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; restored(base, code, heap, roots);
        var imports = artifact::Imports { base: base.header.checkpoint, session: heap.identity }; var c = artifact::Control {};
        let v = artifact::import_graph(a, base, heap, imports, c); assert(c.error == artifact::Error.None);
        let x = source::form_nth(heap, v, 0); let y = source::form_nth(heap, v, 1);
        assert(x.a != y.a && x.a == source::form_nth(heap, v, 2).a && source::form_equal(heap, x, y));
        assert(heap.nodes[x.a].origin == 10 && heap.nodes[y.a].origin == 11 && heap.nodes[y.a].span_start == 4);
        let text = source::form_value(heap, source::form_nth(heap, v, 3)); assert(heap.units[text.a..(text.a + text.b)] == [0xD83D as u16, 0xDE00, 0xD800]);
        let name = heap.nodes[source::form_nth(heap, v, 4).a]; assert(heap.nodes[name.metadata.a].kind == 7 && heap.units[name.namespace_value.a..(name.namespace_value.a + name.namespace_value.b)] == ['n' as u16, 's']);
        var bc = bridge::Control {}; assert(bridge::export_graph(heap, [v], result, bc));
    }
    var bc = bridge::Control {}; assert(bridge::validate(result, bc));
    let root = result.nodes[result.roots[0]]; let x = result.nodes[result.children[root.start]]; let r = result.sources[x.source];
    assert(result.bytes[r.start..(r.start + r.count)] == "one two");
    print("artifact owned UTF16/namespace/metadata/source spans/occurrences survive producer and consumer teardown");
}
fn scalars(base: const snapshot::Snapshot&) {
    var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; restored(base, code, heap, roots);
    var imports = artifact::Imports { base: base.header.checkpoint, session: heap.identity };
    let values: source::Value[] = [source::Value {}, source::nil(), source::boolean(true), source::number(-0.0), source::number(0.0 / 0.0), source::text(heap, "unicode 😀")];
    for v, i in values {
        var ec = artifact::Control {}; let a = artifact::export_graph(heap, base, imports, v, 100 + i, 1, ec); assert(ec.error == artifact::Error.None && a.nodes.len == 0);
        var ic = artifact::Control {}; let got = artifact::import_graph(a, base, heap, imports, ic); assert(ic.error == artifact::Error.None && ic.copied_nodes == 0);
        if v.tag == 4 { assert(heap.units[v.a..(v.a + v.b)] == heap.units[got.a..(got.a + got.b)]); }
        else { assert(bytes_of([v]) == bytes_of([got])); }
        var encoded: u8[>..] = []; assert(artifact::encode(a, base, encoded, ec));
        let decoded = artifact::decode(encoded, base, ec); assert(ec.error == artifact::Error.None && decoded.len == 1);
    }
    print("artifact primitive/NaN/signed-zero/string roots and reusable controls passed");
}
fn graphs(base: const snapshot::Snapshot&) {
    var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; restored(base, code, heap, roots);
    var imports = artifact::Imports { base: base.header.checkpoint, session: heap.identity };
    let cell = source::allocate(heap, source::Node { kind: 3, initialized: true, immutable: true }); let closure = source::closure(heap, roots[4].value.number as i64, [cell]); heap.nodes[cell.a].value = closure;
    var c = artifact::Control {}; let cycle = artifact::export_graph(heap, base, imports, closure, 40, 1, c); assert(c.error == artifact::Error.None && cycle.nodes.len == 2);
    var ic = artifact::Control {}; let relocated = artifact::import_graph(cycle, base, heap, imports, ic); assert(ic.error == artifact::Error.None);
    let result = snapshot_test::invoke(heap, relocated, []); assert(result.a == relocated.a);
    var leaf = roots[5].value;
    for i in 40 { leaf = source::form_collection(heap, 6, [leaf, leaf]); }
    var dc = artifact::Control {}; let diamond = artifact::export_graph(heap, base, imports, leaf, 41, 1, dc);
    assert(dc.error == artifact::Error.None && dc.nodes == 41 && dc.edges == 40 * 5 + 3);
    var converted = artifact::Control {}; let joined = artifact::import_graph(diamond, base, heap, imports, converted); assert(converted.error == artifact::Error.None);
    assert(source::form_nth(heap, joined, 0).a == source::form_nth(heap, joined, 1).a);
    var enc = artifact::Control {}; var bytes: u8[>..] = []; assert(artifact::encode(cycle, base, bytes, enc));
    var dec = artifact::Control {}; let decoded = artifact::decode(bytes, base, dec); assert(dec.error == artifact::Error.None && decoded.len == 1);
    for cut in bytes.len { var failure = artifact::Control {}; let no = artifact::decode(bytes[..cut], base, failure); assert(no.len == 0 && failure.error != artifact::Error.None); }
    var bad = copy(bytes); bad[0] = 255; var bc = artifact::Control {}; let no = artifact::decode(bad, base, bc); assert(no.len == 0 && bc.error != artifact::Error.None);
    print("artifact callable cycles/40-level shared diamond/codec truncations passed, nodes=", dc.nodes, " edges=", dc.edges, " work=", dc.visited);
}
fn main() {
    var code = source::Code {}; var heap = source::Heap { code: code };
    let left = snapshot_test::append(code, embed_file("corpus/branches-helper-0.pants.gsp"));
    let right = snapshot_test::append(code, embed_file("corpus/branches-helper-1.pants.gsp"));
    let consumer = snapshot_test::append(code, embed_file("corpus/branches-consume.pants.gsp"));
    var getter = source::Code {}; getter.functions.push(source::Function { start: 0, count: 3, constants: 0, children: 0, params: 0, locals: 0, captures: 1, strict: true });
    for op, i in [46, 49, 15] { getter.ops.push(source::Op { code: op, arg: 0, offset: [0, 3, 4][i], start: 0, end: 0, depth: [0, 1, 1][i] }); } getter.valid = true; let read = source::append_code(code, getter);
    assert(source::initialize(heap)); let rt = source::boot_runtime(heap); let shared = source::allocate(heap, source::Node { kind: 11, value: source::number(9), origin: 90 });
    let bindings: snapshot::Binding[] = [snapshot::Binding { id: 0, generation: 1, value: rt }, snapshot::Binding { id: 1, generation: 1, value: source::number(left) }, snapshot::Binding { id: 2, generation: 1, value: source::number(right) }, snapshot::Binding { id: 3, generation: 1, value: source::number(consumer) }, snapshot::Binding { id: 4, generation: 1, value: source::number(read) }, snapshot::Binding { id: 5, generation: 1, value: shared }];
    var c = snapshot::Control {}; let base = snapshot::capture(heap, bindings, c); assert(c.error == snapshot::Error.None);
    var ca = artifact::Control {}; var cb = artifact::Control {}; let a = produce(base, 1, ca); let b = produce(base, 2, cb);
    joins(base, a, b); diamond_delivery(base, a); invalid(base, a); graphs(base); provenance(base); scalars(base); work_cuts(base, a);
}
