import std;
import .artifact;
import .snapshot;
import .snapshot_fixture;
import .source_vm;
import .form_bridge;
import .heap_program;

fn restored(base: const snapshot::Snapshot&, code: source::Code&, heap: source::Heap&, roots: snapshot::Binding[>..]&) {
    var c = snapshot::Control {}; assert(snapshot::restore(base, code, heap, roots, source::reserve_identity(), c));
}
fn produce(base: const snapshot::Snapshot&, which: i64, c: artifact::Control&) -> artifact::Artifact {
    var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; restored(base, code, heap, roots);
    var imports = artifact::Imports { base: base.header.checkpoint, session: heap.identity };
    let produced = snapshot_test::factory(heap, roots[which].value.num() as i64, roots[0].value); heap.producer_count += 1;
    let cell = source::allocate(heap, source::Node { kind: 3, initialized: true, immutable: true, value: source::number(40 + which) });
    let closure = source::closure(heap, roots[4].value.num() as i64, [cell]);
    let x = source::allocate(heap, source::Node { kind: 11, value: source::number(1), origin: 70 });
    let y = source::allocate(heap, source::Node { kind: 11, value: source::number(1), origin: 71 });
    let root = source::form_collection(heap, 6, [produced, roots[5].value, closure, x, y, x]);
    let a = artifact::export_graph(heap, base, imports, root, which, 1, c); assert(c.error == artifact::Error.None); return copy(a);
}
fn state(heap: const source::Heap&, imports: const artifact::Imports&) -> u8[>..] {
    var out: u8[>..] = []; let empty: snapshot::Binding[] = []; let view = snapshot::borrow(heap, empty, imports.base);
    let header = view.header; to_bytes([header.ends], out); to_bytes([header.limits], out); to_bytes([header.version, header.allocator, header.base, header.checkpoint, header.work, header.boot, header.initializers, header.producers, header.property_rows, heap.identity], out); out.append(source::plain_rows([view.header.global, view.header.call_method]));
    out.append(source::plain_rows(heap.nodes)); out.append(source::plain_rows(heap.values)); out.append(source::plain_rows(heap.properties)); to_bytes(heap.units, out); to_bytes(heap.latin1, out); to_bytes(heap.strings, out); out.append(source::plain_rows(heap.constants)); to_bytes(heap.source_bytes, out); to_bytes(heap.source_ranges, out);
    to_bytes([imports.base, imports.session], out); to_bytes(imports.entries, out); to_bytes(imports.definitions, out); to_bytes(imports.signatures, out); to_bytes(imports.deliveries, out); return out;
}
fn reject_import(a: const artifact::Artifact&, base: const snapshot::Snapshot&, heap: source::Heap&, imports: artifact::Imports&, error: artifact::Error) {
    let before = state(heap, imports); var c = artifact::Control {}; artifact::import_graph(a, base, heap, imports, c);
    if c.error != error { print("wrong error ", c.error, " expected ", error); } assert(c.error == error && state(heap, imports) == before);
}
// Messages between the heap-free main and the thread programs that each own
// one heap (light-cljs-794).
struct Base { image: snapshot::Snapshot }
struct Published { artifact: artifact::Artifact }
struct Visits { count: i64 }
// The first join order's expansion, compared against the second order's.
struct Canonical { nodes: bridge::Form[], children: i64[], units: u16[] }
thread_fn producer(base: snapshot::Snapshot, which: i64) {
    defer { heap_program::finish(); }
    var c = artifact::Control {}; assert(qput(Published { artifact: produce(base, which, c) }));
}
fn joins(base: const snapshot::Snapshot&, a: const artifact::Artifact&, b: const artifact::Artifact&) {
    assert(a.keys[0].slot == b.keys[0].slot && a.keys[0].arena != b.keys[0].arena);
    for reverse in 2 { heap_program::join(thread_spawn(join_order, copy(base), copy(a), copy(b), reverse)); }
}
thread_fn join_order(base: snapshot::Snapshot, a: artifact::Artifact, b: artifact::Artifact, reverse: i64) {
    defer { heap_program::finish(); }
    {
        var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; restored(base, code, heap, roots);
        var imports = artifact::Imports { base: base.header.checkpoint, session: heap.identity };
        var ca = artifact::Control {}; var cb = artifact::Control {}; var left = source::Value.Undefined; var right = source::Value.Undefined;
        if reverse == 0 { left = artifact::import_graph(a, base, heap, imports, ca); right = artifact::import_graph(b, base, heap, imports, cb); }
        else { right = artifact::import_graph(b, base, heap, imports, cb); left = artifact::import_graph(a, base, heap, imports, ca); }
        assert(ca.error == artifact::Error.None && cb.error == artifact::Error.None && left.id() != right.id());
        assert(source::form_nth(heap, left, 1).id() == roots[5].value.id() && source::form_nth(heap, right, 1).id() == roots[5].value.id());
        let before = state(heap, imports); var duplicate = artifact::Control {}; let again = artifact::import_graph(a, base, heap, imports, duplicate);
        assert(duplicate.error == artifact::Error.None && duplicate.copied_bytes == 0 && duplicate.copied_nodes == 0 && duplicate.staged_bytes > 0 && again.id() == left.id() && state(heap, imports) == before);
        let closure = source::form_nth(heap, left, 2); assert(snapshot_test::invoke(heap, closure, []).num() == 41);
        let x = source::form_nth(heap, left, 3); let y = source::form_nth(heap, left, 4);
        assert(x.id() != y.id() && x.id() == source::form_nth(heap, left, 5).id() && source::form_equal(heap, x, y));
        let definitions = source::allocate(heap, source::Node { kind: 0 }); assert(source::install_field(heap, heap.global, "__lightCompilerRoots", definitions));
        assert(source::install_field(heap, definitions, "cljs.user/left-result", source::form_nth(heap, left, 0)));
        assert(source::install_field(heap, definitions, "cljs.user/right-result", source::form_nth(heap, right, 0)));
        let consumer = snapshot_test::factory(heap, roots[3].value.num() as i64, roots[0].value);
        let result = snapshot_test::invoke(heap, consumer, [source::nil(), source::nil()]);
        let expansion = source::form_nth(heap, result, 1); assert(source::form_count(heap, expansion) == 4);
        for expected, i in [2, 3, 9, 16] { assert(source::form_nth(heap, expansion, i).num() == expected); }
        var graph = bridge::Graph {}; var control = bridge::Control {}; assert(bridge::export_graph(heap, [result], graph, control));
        if reverse == 0 { assert(qput(Canonical { nodes: copy(graph.nodes), children: copy(graph.children), units: copy(graph.units) })); }
        else { let canonical = qget<Canonical>(); assert(bytes_of(graph.nodes) == bytes_of(canonical.nodes) && graph.children == canonical.children && graph.units == canonical.units); }
        // Diamond: each new branch publishes a form referring to the imported A.
        let top = source::form_collection(heap, 6, [left, right, left]); var ec = artifact::Control {};
        let diamond = artifact::export_graph(heap, base, imports, top, 20 + reverse, 1, ec); assert(ec.error == artifact::Error.None);
        var dc = artifact::Control {}; let joined = artifact::import_graph(diamond, base, heap, imports, dc); assert(dc.error == artifact::Error.None);
        assert(source::form_nth(heap, joined, 0).id() == left.id() && source::form_nth(heap, joined, 2).id() == left.id() && source::form_nth(heap, joined, 1).id() == right.id());
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
thread_fn relayer(base: snapshot::Snapshot, a: artifact::Artifact, definition: i64) {
    defer { heap_program::finish(); }
    assert(qput(Published { artifact: relay(base, a, definition) }));
}
fn diamond_delivery(base: const snapshot::Snapshot&, a: const artifact::Artifact&) {
    heap_program::join(thread_spawn(relayer, copy(base), copy(a), 51)); let published_left = qget<Published>(); let left .= published_left.artifact;
    heap_program::join(thread_spawn(relayer, copy(base), copy(a), 52)); let published_right = qget<Published>(); let right .= published_right.artifact;
    for reverse in 2 { heap_program::join(thread_spawn(diamond_order, copy(base), copy(a), copy(left), copy(right), reverse)); }
    print("artifact two independent relays/diamond identity/direct and duplicate delivery passed");
}
thread_fn diamond_order(base: snapshot::Snapshot, a: artifact::Artifact, left: artifact::Artifact, right: artifact::Artifact, reverse: i64) {
    defer { heap_program::finish(); }
    {
        var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; restored(base, code, heap, roots);
        var imports = artifact::Imports { base: base.header.checkpoint, session: heap.identity };
        var lc = artifact::Control {}; var rc = artifact::Control {}; var l = source::Value.Undefined; var r = source::Value.Undefined;
        if reverse == 0 { l = artifact::import_graph(left, base, heap, imports, lc); r = artifact::import_graph(right, base, heap, imports, rc); }
        else { r = artifact::import_graph(right, base, heap, imports, rc); l = artifact::import_graph(left, base, heap, imports, lc); }
        assert(lc.error == artifact::Error.None && rc.error == artifact::Error.None && l.id() != r.id());
        let shared = source::form_nth(heap, l, 0);
        assert(shared.id() == source::form_nth(heap, r, 0).id() && shared.id() == source::form_nth(heap, l, 1).id());
        let nodes = heap.nodes.len; var direct = artifact::Control {}; let delivered = artifact::import_graph(a, base, heap, imports, direct);
        assert(direct.error == artifact::Error.None && delivered.id() == shared.id() && heap.nodes.len == nodes);
        var duplicate = artifact::Control {}; let again = artifact::import_graph(right, base, heap, imports, duplicate);
        assert(duplicate.error == artifact::Error.None && again.id() == r.id() && heap.nodes.len == nodes);
    }
}
thread_fn invalid(base: snapshot::Snapshot, a: artifact::Artifact) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; restored(base, code, heap, roots);
    var imports = artifact::Imports { base: base.header.checkpoint, session: heap.identity };
    for mode in 13 {
        var bad = copy(a);
        match mode {
            0 => { bad.root = source::object(bad.nodes.len); },
            1 => { bad.values[0] = source::object(bad.nodes.len); },
            2 => { bad.nodes[0].kind = 0; },
            3 => { bad.values[bad.nodes[0].start] = bad.root; },
            4 => { bad.schema = 99; },
            5 => { bad.keys[1] = bad.keys[0]; },
            6 => { bad.nodes[0].metadata = source::object(bad.nodes.len); },
            7 => { bad.nodes[0].start = 4294967295; },
            8 => { bad.nodes[0].source = bad.sources.len as u32; },
            9 => { bad.nodes[0].count = 4294967295; },
            10 => { bad.keys[0].arena = base.header.checkpoint; bad.keys[0].slot = 0; },
            11 => { bad.version = 0; },
            12 => { bad.nodes[0].count -= 1; bad.values[bad.nodes[0].start + bad.nodes[0].count] = source::object(bad.nodes.len); },
            _ => {},
        }
        var wire: u8[>..] = []; to_bytes([bad], wire); var decode = artifact::Control {};
        let rejected = artifact::decode(wire, base, decode);
        // A well-shaped colliding provenance key is rejected by the import
        // cache below; the stateless decoder cannot know prior publications.
        if mode == 10 { assert(rejected.len == 1 && decode.error == artifact::Error.None); }
        else { assert(rejected.len == 0 && decode.error != artifact::Error.None); }
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
    let cycle: i64[3] = [map.id(), nested.id(), mutable.id()]; assert(c.path[..] == cycle);
    let map_key = source::form_collection(heap, 7, [nested, source::number(1)]); var kc = artifact::Control {}; artifact::export_graph(heap, base, imports, map_key, 31, 1, kc); assert(kc.error == artifact::Error.Mutable && kc.path.len == 3);
    let closure = source::closure(heap, roots[4].value.num() as i64, [mutable]); var cc = artifact::Control {}; artifact::export_graph(heap, base, imports, closure, 32, 1, cc); assert(cc.error == artifact::Error.Mutable && cc.path.len == 2);
    // A closure with own properties (light-cljs-805) is mutable, in shape
    // mode (a property or a made `prototype`) and in dictionary mode after a
    // delete; nothing is registered.
    {
        let owned = source::closure(heap, roots[4].value.num() as i64, [source::number(1)]);
        assert(source::materialize_function(heap, owned.id()));
        assert(source::put(heap, owned, source::text(heap, "x"), source::number(1)));
        let definitions = imports.definitions.len; let before = state(heap, imports);
        var shaped = artifact::Control {}; let none = artifact::export_graph(heap, base, imports, owned, 34, 1, shaped);
        assert(shaped.error == artifact::Error.Mutable && shaped.path.len == 1 && none.nodes.len == 0 && imports.definitions.len == definitions && state(heap, imports) == before);
        assert(source::delete_property(heap, owned, source::key_named(heap, "x")) && source::node(heap, owned.id()).initialized);
        var dictionary = artifact::Control {}; artifact::export_graph(heap, base, imports, owned, 35, 1, dictionary); assert(dictionary.error == artifact::Error.Mutable && imports.definitions.len == definitions);
    }
    // An occurrence is scalar-only, but its metadata still participates.
    let metadata = source::form_collection(heap, 7, [source::number(1), mutable]); source::node_mut(heap, wrapped.id()).metadata = metadata; var oc = artifact::Control {}; artifact::export_graph(heap, base, imports, wrapped, 33, 1, oc); assert(oc.error == artifact::Error.Mutable && oc.path.len >= 3);
    source::node_mut(heap, wrapped.id()).metadata = source::nil();
    {
        source::node_mut(heap, roots[5].value.id()).value = source::number(99);
        var changed_base = copy(a); changed_base.definition = 98; reject_import(changed_base, base, heap, imports, artifact::Error.Conflict);
        source::node_mut(heap, roots[5].value.id()).value = source::number(9);
        imports.session += 1; reject_import(a, base, heap, imports, artifact::Error.Context); imports.session -= 1;
    }
    for limit in 3 {
        let before = state(heap, imports); var small = artifact::Control {};
        if limit == 0 { small.work = 1; } else if limit == 1 { small.bytes = artifact::payload(a) - 1; } else { heap.limits.nodes = heap.nodes.len - heap.image.nodes; }
        var fresh_definition = copy(a); fresh_definition.definition = 97; if limit == 2 { fresh_definition.keys[0].arena = source::reserve_identity(); }
        let current = state(heap, imports); artifact::import_graph(fresh_definition, base, heap, imports, small);
        assert(small.error == (if limit == 0 { artifact::Error.Work } else { artifact::Error.Bytes }) && state(heap, imports) == current);
        if limit < 2 { assert(current == before); }
    }
    print("artifact malformed/mutable paths/cycle/conflict/atomic-budget rejection passed");
}
fn work_cuts(base: const snapshot::Snapshot&, a: const artifact::Artifact&) {
    heap_program::join(thread_spawn(work_measure, copy(base), copy(a))); let visited = qget<Visits>().count;
    heap_program::join(thread_spawn(work_target, copy(base), copy(a), visited));
}
thread_fn work_measure(base: snapshot::Snapshot, a: artifact::Artifact) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; restored(base, code, heap, roots);
    var imports = artifact::Imports { base: base.header.checkpoint, session: heap.identity }; var success = artifact::Control {};
    artifact::import_graph(a, base, heap, imports, success); assert(success.error == artifact::Error.None);
    assert(qput(Visits { count: success.visited }));
}
thread_fn work_target(base: snapshot::Snapshot, a: artifact::Artifact, visited: i64) {
    defer { heap_program::finish(); }
    var target_code = source::Code {}; var target = source::Heap { code: target_code }; var target_roots: snapshot::Binding[>..] = []; restored(base, target_code, target, target_roots);
    var cache = artifact::Imports { base: base.header.checkpoint, session: target.identity }; let before = state(target, cache);
    for cut in visited {
        var failure = artifact::Control { work: cut }; artifact::import_graph(a, base, target, cache, failure);
        assert(failure.error == artifact::Error.Work && failure.copied_nodes == 0 && failure.copied_bytes == 0 && state(target, cache) == before);
    }
    var exact = artifact::Control { work: visited }; artifact::import_graph(a, base, target, cache, exact); assert(exact.error == artifact::Error.None && exact.visited == visited);
    print("artifact every import work cut/unchanged heap and caches/exact budget passed: ", visited);
}
// `identity`: the form's source is a compiler identity, with no text
// (light-cljs-817), which the artifact carries as it is.
fn provenance_artifact(base: const snapshot::Snapshot&, identity: bool) -> artifact::Artifact {
    var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; restored(base, code, heap, roots);
    var graph = bridge::Graph {};
    if identity { graph.sources.push(source::SourceRange { start: 0, count: 7, identity: 42 }); } else { graph.bytes.append("one two"); graph.sources.push(source::SourceRange { start: 0, count: 7 }); }
    graph.units.append([0xD83D, 0xDE00, 0xD800, 'n', 's', 'x']);
    graph.nodes.push(bridge::Form { tag: form_tag::Number, number: 1, origin: 10, source: 0, span_start: 0, span_end: 3 });
    graph.nodes.push(bridge::Form { tag: form_tag::Number, number: 1, origin: 11, source: 0, span_start: 4, span_end: 7 });
    graph.nodes.push(bridge::Form { tag: form_tag::String, start: 0, count: 3 }); graph.nodes.push(bridge::Form { tag: form_tag::Map });
    graph.nodes.push(bridge::Form { tag: form_tag::Symbol, start: 5, count: 1, ns_start: 3, ns_count: 2, metadata: 3 });
    graph.children.append([0, 1, 0, 2, 4]); graph.nodes.push(bridge::Form { tag: form_tag::Vector, start: 0, count: 5 }); graph.roots.push(5);
    var bc = bridge::Control {}; var values: source::Value[>..] = []; assert(bridge::import_graph(heap, graph, values, bc));
    var imports = artifact::Imports { base: base.header.checkpoint, session: heap.identity }; var c = artifact::Control {};
    let a = artifact::export_graph(heap, base, imports, values[0], 60, 1, c); assert(c.error == artifact::Error.None); return copy(a);
}
thread_fn provenance_producer(base: snapshot::Snapshot, identity: bool) {
    defer { heap_program::finish(); }
    assert(qput(Published { artifact: provenance_artifact(base, identity) }));
}
fn provenance(base: const snapshot::Snapshot&) {
    for identity in [false, true] {
        heap_program::join(thread_spawn(provenance_producer, copy(base), identity)); let published_a = qget<Published>(); let a .= published_a.artifact;
        if identity { assert(a.bytes.len == 0 && a.sources.len == 1 && a.sources[0].identity == 42 && a.sources[0].count == 7); }
        heap_program::join(thread_spawn(provenance_consumer, copy(base), copy(a), identity));
    }
}
thread_fn provenance_consumer(base: snapshot::Snapshot, a: artifact::Artifact, identity: bool) {
    defer { heap_program::finish(); }
    var result = bridge::Graph {};
    {
        var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; restored(base, code, heap, roots);
        var imports = artifact::Imports { base: base.header.checkpoint, session: heap.identity }; var c = artifact::Control {};
        let v = artifact::import_graph(a, base, heap, imports, c); assert(c.error == artifact::Error.None);
        let x = source::form_nth(heap, v, 0); let y = source::form_nth(heap, v, 1);
        assert(x.id() != y.id() && x.id() == source::form_nth(heap, v, 2).id() && source::form_equal(heap, x, y));
        assert(source::node(heap, x.id()).origin == 10 && source::node(heap, y.id()).origin == 11 && source::node(heap, y.id()).span_start == 4);
        assert(source::node(heap, x.id()).source == source::node(heap, y.id()).source);
        let text = source::form_value(heap, source::form_nth(heap, v, 3)); assert(source::utf16(heap, text) == [0xD83D as u16, 0xDE00, 0xD800]);
        let name = source::node(heap, source::form_nth(heap, v, 4).id()); assert(source::node(heap, name.metadata.id()).kind == 7 && source::utf16(heap, name.namespace_value) == ['n' as u16, 's']);
        var bc = bridge::Control {}; assert(bridge::export_graph(heap, [v], result, bc));
    }
    var bc = bridge::Control {}; assert(bridge::validate(result, bc));
    let root = result.nodes[result.roots[0]]; let x = result.nodes[result.children[root.start]]; let r = result.sources[x.source];
    if identity { assert(result.bytes.len == 0 && r.identity == 42 && r.count == 7 && x.span_start == 0 && x.span_end == 3); } else { assert(result.bytes[r.start..(r.start + r.count)] == "one two"); }
    print("artifact owned UTF16/namespace/metadata/source spans/occurrences survive producer and consumer teardown");
}
thread_fn scalars(base: snapshot::Snapshot) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; restored(base, code, heap, roots);
    var imports = artifact::Imports { base: base.header.checkpoint, session: heap.identity };
    let values: source::Value[] = [source::Value.Undefined, source::nil(), source::boolean(true), source::number(-0.0), source::number(0.0 / 0.0), source::text(heap, "unicode 😀")];
    for v, i in values {
        var ec = artifact::Control {}; let a = artifact::export_graph(heap, base, imports, v, 100 + i, 1, ec); assert(ec.error == artifact::Error.None && a.nodes.len == 0);
        var ic = artifact::Control {}; let got = artifact::import_graph(a, base, heap, imports, ic); assert(ic.error == artifact::Error.None && ic.copied_nodes == 0);
        if v.is_string() { assert(source::utf16(heap, v) == source::utf16(heap, got)); }
        else { assert(source::plain_rows([v]) == source::plain_rows([got])); }
        var encoded: u8[>..] = []; assert(artifact::encode(a, base, encoded, ec));
        let decoded = artifact::decode(encoded, base, ec); assert(ec.error == artifact::Error.None && decoded.len == 1);
    }
    print("artifact primitive/NaN/signed-zero/string roots and reusable controls passed");
}
thread_fn graphs(base: snapshot::Snapshot) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; restored(base, code, heap, roots);
    var imports = artifact::Imports { base: base.header.checkpoint, session: heap.identity };
    let cell = source::allocate(heap, source::Node { kind: 3, initialized: true, immutable: true }); let closure = source::closure(heap, roots[4].value.num() as i64, [cell]); source::node_mut(heap, cell.id()).value = closure;
    var c = artifact::Control {}; let cycle = artifact::export_graph(heap, base, imports, closure, 40, 1, c); assert(c.error == artifact::Error.None && cycle.nodes.len == 2);
    var ic = artifact::Control {}; let relocated = artifact::import_graph(cycle, base, heap, imports, ic); assert(ic.error == artifact::Error.None);
    let result = snapshot_test::invoke(heap, relocated, []); assert(result.id() == relocated.id());
    var leaf = roots[5].value;
    for i in 40 { leaf = source::form_collection(heap, 6, [leaf, leaf]); }
    var dc = artifact::Control {}; let diamond = artifact::export_graph(heap, base, imports, leaf, 41, 1, dc);
    assert(dc.error == artifact::Error.None && dc.nodes == 41 && dc.edges == 40 * 5 + 3);
    var converted = artifact::Control {}; let joined = artifact::import_graph(diamond, base, heap, imports, converted); assert(converted.error == artifact::Error.None);
    assert(source::form_nth(heap, joined, 0).id() == source::form_nth(heap, joined, 1).id());
    var enc = artifact::Control {}; var bytes: u8[>..] = []; assert(artifact::encode(cycle, base, bytes, enc));
    var dec = artifact::Control {}; let decoded = artifact::decode(bytes, base, dec); assert(dec.error == artifact::Error.None && decoded.len == 1);
    for cut in bytes.len { var failure = artifact::Control {}; let no = artifact::decode(bytes[..cut], base, failure); assert(no.len == 0 && failure.error != artifact::Error.None); }
    var bad = copy(bytes); bad[0] = 255; var bc = artifact::Control {}; let no = artifact::decode(bad, base, bc); assert(no.len == 0 && bc.error != artifact::Error.None);
    print("artifact callable cycles/40-level shared diamond/codec truncations passed, nodes=", dc.nodes, " edges=", dc.edges, " work=", dc.visited);
}
thread_fn overlapping_cycles(base: snapshot::Snapshot) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; restored(base, code, heap, roots);
    var imports = artifact::Imports { base: base.header.checkpoint, session: heap.identity };
    let b = source::allocate(heap, source::Node { kind: 3, initialized: true, immutable: true });
    let c = source::allocate(heap, source::Node { kind: 3, initialized: true, immutable: true });
    let d = source::allocate(heap, source::Node { kind: 3, initialized: true, immutable: true, value: b });
    let a = source::closure(heap, roots[4].value.num() as i64 + 1, [b, c]); source::node_mut(heap, b.id()).value = a; source::node_mut(heap, c.id()).value = d;
    // B's DFS frame finishes before C -> D -> B. All four nodes still
    // belong to the same callable SCC, including that completed sibling.
    var ec = artifact::Control {}; let callable = artifact::export_graph(heap, base, imports, a, 43, 1, ec);
    assert(ec.error == artifact::Error.None && callable.nodes.len == 4);
    var ic = artifact::Control {}; let imported = artifact::import_graph(callable, base, heap, imports, ic);
    assert(ic.error == artifact::Error.None && snapshot_test::invoke(heap, imported, []).id() == imported.id());
    let outside = source::form_collection(heap, 6, [a]); var outside_control = artifact::Control {};
    artifact::export_graph(heap, base, imports, outside, 44, 1, outside_control); assert(outside_control.error == artifact::Error.None);
    // Replace D with V, a vector containing B: A captures B,C; B -> A;
    // C -> V -> B. V belongs to the larger cycle and must reject.
    var bad = copy(callable); bad.definition = 45;
    assert(bad.nodes[0].kind == 2 && bad.nodes[1].value.id() == 0 && bad.nodes[2].value.id() == 3);
    bad.nodes[3] = source::Node { kind: 6, start: bad.nodes[0].start, count: 1 };
    let before = state(heap, imports); var rejected_import = artifact::Control {};
    artifact::import_graph(bad, base, heap, imports, rejected_import);
    assert(rejected_import.error == artifact::Error.Cycle && rejected_import.path[..] == [0, 2, 3] && state(heap, imports) == before);
    let start = heap.values.len; heap.values.push(b); source::set_node(heap, d.id(), source::Node { kind: 6, start: start as u32, count: 1 });
    let current = state(heap, imports); var rejected_export = artifact::Control {};
    let no = artifact::export_graph(heap, base, imports, a, 46, 1, rejected_export);
    let path: i64[3] = [a.id(), c.id(), d.id()];
    assert(rejected_export.error == artifact::Error.Cycle && no.nodes.len == 0 && rejected_export.path[..] == path && state(heap, imports) == current);
    print("artifact overlapping callable SCC/form-cycle rejection/path/atomicity passed");
}
thread_fn signature_budgets(base: snapshot::Snapshot) {
    defer { heap_program::finish(); }
    // Compact input shares one UTF16 range across every vector element.
    // Its exact signature expands that range each time, beyond payload size.
    var units:  u16[>..]           = []; units.resize(256, 'x');
    var values: source::Value[>..] = []; values.resize(16, source::Value.String { s: 0 });
    let strings: source::Str[]         = [source::Str { start: 0, len: units.len as u32 }];
    let nodes:   source::Node[]        = [source::Node { kind: 6, count: values.len as u32, source: 0, span_end: 3 }];
    let keys:    artifact::Key[]       = [artifact::Key { arena: source::reserve_identity(), slot: 0 }];
    let sources: source::SourceRange[] = [source::SourceRange { start: 0, count: 3 }];
    let a = artifact::Artifact { base: base.header.checkpoint, definition: 47, root: source::object(0), keys: keys[..], nodes: nodes[..], values: values[..], units: units[..], strings: copy(strings), bytes: "src", sources: sources[..] };
    // The frozen/destination base signature takes the same checked path.
    let s = snapshot::Snapshot { nodes: nodes[..], values: values[..], units: units[..], strings: copy(strings), bytes: "src", sources: sources[..] };
    var expected: u8[>..] = []; var success = artifact::Control {};
    assert(artifact::signature(a, 0, expected, success) && expected.len > artifact::payload(a));
    for from_base in 2 {
        var whole: u8[>..] = []; var complete = artifact::Control {};
        assert(if from_base == 0 { artifact::signature(a, 0, whole, complete) } else { artifact::base_signature(s, 0, a.base, whole, complete) });
        assert(whole == expected && complete.staged_bytes == whole.len);
        // Every byte/work cut rejects before any append can exceed its limit.
        for cut in whole.len {
            var out: u8[>..] = []; var small = artifact::Control { bytes: cut };
            let ok = if from_base == 0 { artifact::signature(a, 0, out, small) } else { artifact::base_signature(s, 0, a.base, out, small) };
            assert(!ok && small.error == artifact::Error.Bytes && out.len <= cut && small.staged_bytes == out.len);
        }
        for cut in complete.visited {
            var out: u8[>..] = []; var small = artifact::Control { work: cut };
            let ok = if from_base == 0 { artifact::signature(a, 0, out, small) } else { artifact::base_signature(s, 0, a.base, out, small) };
            assert(!ok && small.error == artifact::Error.Work && out.len <= small.visited && small.visited <= cut && small.staged_bytes == out.len);
        }
        var exact_out: u8[>..] = []; var exact = artifact::Control { bytes: whole.len, work: complete.visited };
        assert(if from_base == 0 { artifact::signature(a, 0, exact_out, exact) } else { artifact::base_signature(s, 0, a.base, exact_out, exact) });
        assert(exact_out == whole && exact.visited == complete.visited && exact.staged_bytes == whole.len);
        // Existing staged records consume the byte allowance too.
        var prefixed: u8[>..] = [123]; var no_room = artifact::Control { bytes: whole.len };
        assert(!(if from_base == 0 { artifact::signature(a, 0, prefixed, no_room) } else { artifact::base_signature(s, 0, a.base, prefixed, no_room) }));
        assert(no_room.error == artifact::Error.Bytes && prefixed.len <= no_room.bytes && prefixed[0] == 123);
    }
    var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; restored(base, code, heap, roots);
    var imports = artifact::Imports { base: base.header.checkpoint, session: heap.identity }; let before = state(heap, imports);
    var byte_limit = artifact::Control { bytes: artifact::footprint(a) + 16 };
    artifact::import_graph(a, base, heap, imports, byte_limit);
    assert(byte_limit.error == artifact::Error.Bytes && byte_limit.copied_nodes == 0 && byte_limit.copied_bytes == 0 && state(heap, imports) == before);
    var valid = artifact::Control {}; artifact::validate(a, base, valid); assert(valid.error == artifact::Error.None);
    var empty:    artifact::Control = artifact::Control {}; assert(artifact::context(heap, base, imports, empty));
    var delivery: u8[>..]           = []; to_bytes([a], delivery);
    var work_limit = artifact::Control { work: valid.visited + empty.visited + delivery.len + 128 };
    artifact::import_graph(a, base, heap, imports, work_limit);
    assert(work_limit.error == artifact::Error.Work && work_limit.copied_nodes == 0 && work_limit.copied_bytes == 0 && state(heap, imports) == before);
    var repaired = artifact::Control {}; artifact::import_graph(a, base, heap, imports, repaired); assert(repaired.error == artifact::Error.None);
    print("artifact shared-string signature byte/work cuts/exact limits/atomic rejection passed");
}
// The base image's own thread program: builds and captures it.
thread_fn base_image() {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code };
    let left = snapshot_test::append(code, embed_file("../corpus/branches-helper-0.pants.gsp"));
    let right = snapshot_test::append(code, embed_file("../corpus/branches-helper-1.pants.gsp"));
    let consumer = snapshot_test::append(code, embed_file("../corpus/branches-consume.pants.gsp"));
    // Synthetic capture getter tests relocation/cycles separately from the
    // unchanged compiler-emitted producer/consumer bodies above.
    var getter = source::Code {}; getter.functions.push(source::Function { start: 0, count: 3, constants: 0, children: 0, params: 0, locals: 0, captures: 1, strict: true });
    for op, i in [opcode::GetCapture, opcode::CellGet, opcode::Return] { getter.ops.push(source::Op { code: op, arg: 0, offset: [0, 3, 4][i], start: 0, end: 0, depth: [0, 1, 1][i] }); } getter.valid = true; let read = source::append_code(code, getter);
    getter.functions[0].captures = 2; assert(source::append_code(code, getter) == read + 1);
    assert(source::initialize(heap)); let rt = source::boot_runtime(heap); let shared = source::allocate(heap, source::Node { kind: 11, value: source::number(9), origin: 90 });
    let named = source::allocate(heap, source::Node { kind: 11, value: source::text(heap, "x"), origin: 91 });
    let bindings: snapshot::Binding[] = [snapshot::Binding { id: 0, generation: 1, value: rt }, snapshot::Binding { id: 1, generation: 1, value: source::number(left) }, snapshot::Binding { id: 2, generation: 1, value: source::number(right) }, snapshot::Binding { id: 3, generation: 1, value: source::number(consumer) }, snapshot::Binding { id: 4, generation: 1, value: source::number(read) }, snapshot::Binding { id: 5, generation: 1, value: shared }, snapshot::Binding { id: 6, generation: 1, value: named }];
    var c = snapshot::Control {}; let base = snapshot::capture(heap, bindings, c); assert(c.error == snapshot::Error.None);
    assert(qput(Base { image: copy(base) }));
}
// A base node whose string is Latin-1 reconnects by its UTF-16 signature, and
// a changed destination string is a conflict (light-cljs-800).
thread_fn latin1_producer(base: snapshot::Snapshot) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; restored(base, code, heap, roots);
    assert(!source::header(heap, source::node(heap, roots[6].value.id()).value.sid()).wide);
    var imports = artifact::Imports { base: base.header.checkpoint, session: heap.identity }; var c = artifact::Control {};
    let root = source::form_collection(heap, 6, [roots[6].value, source::number(1)]);
    let a = artifact::export_graph(heap, base, imports, root, 61, 1, c); assert(c.error == artifact::Error.None);
    assert(qput(Published { artifact: copy(a) }));
}
thread_fn latin1_consumer(base: snapshot::Snapshot, a: artifact::Artifact) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; restored(base, code, heap, roots);
    {
        var imports = artifact::Imports { base: base.header.checkpoint, session: heap.identity }; var c = artifact::Control {};
        let got = artifact::import_graph(a, base, heap, imports, c); assert(c.error == artifact::Error.None);
        assert(source::form_nth(heap, got, 0) == roots[6].value);
    }
    var imports = artifact::Imports { base: base.header.checkpoint, session: heap.identity };
    source::node_mut(heap, roots[6].value.id()).value = source::text(heap, "y");
    reject_import(a, base, heap, imports, artifact::Error.Conflict);
}
// main builds no heap; every heap is its own thread program.
fn main() {
    heap_program::join(thread_spawn(base_image)); let message = qget<Base>(); let base .= message.image;
    heap_program::join(thread_spawn(producer, copy(base), 1)); let published_a = qget<Published>(); let a .= published_a.artifact;
    heap_program::join(thread_spawn(producer, copy(base), 2)); let published_b = qget<Published>(); let b .= published_b.artifact;
    joins(base, a, b); diamond_delivery(base, a); heap_program::join(thread_spawn(invalid, copy(base), copy(a))); heap_program::join(thread_spawn(graphs, copy(base))); heap_program::join(thread_spawn(overlapping_cycles, copy(base)));
    heap_program::join(thread_spawn(signature_budgets, copy(base))); heap_program::join(thread_spawn(source_spans, copy(base))); provenance(base); heap_program::join(thread_spawn(scalars, copy(base))); work_cuts(base, a);
    heap_program::join(thread_spawn(latin1_producer, copy(base))); let latin1 = qget<Published>();
    heap_program::join(thread_spawn(latin1_consumer, copy(base), copy(latin1.artifact)));
}

thread_fn source_spans(base: snapshot::Snapshot) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; restored(base, code, heap, roots);
    var graph = bridge::Graph {}; graph.bytes.append("é😀"); graph.sources.push(source::SourceRange { start: 0, count: 6 });
    graph.nodes.push(bridge::Form { tag: form_tag::Number, number: 7, source: 0, span_start: 0, span_end: 6 }); graph.roots.push(0);
    var bc = bridge::Control {}; var values: source::Value[>..] = []; assert(bridge::import_graph(heap, graph, values, bc));
    var imports = artifact::Imports { base: base.header.checkpoint, session: heap.identity }; var c = artifact::Control {};
    let saved = artifact::export_graph(heap, base, imports, values[0], 701, 1, c); assert(c.error == artifact::Error.None);
    var committed = artifact::Control {}; let result = artifact::import_graph(saved, base, heap, imports, committed); assert(committed.error == artifact::Error.None);
    for offset in [1, 3, 4, 5] { for end in [false, true] {
        var bad = copy(saved); for &n in bad.nodes { if n.source != source::no_row() { if end { n.span_end = offset as u32; } else { n.span_start = offset as u32; } } }
        reject_import(bad, base, heap, imports, artifact::Error.Link);
        assert(source::form_value(heap, result).num() == 7);
    } }
    print("artifact multibyte source span starts/ends reject before relocation; committed identity/value/cache preserved");
}
