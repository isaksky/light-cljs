import std;
import .program;
import .source_vm;
import .snapshot;
import .form_bridge;
import .vm;

namespace snapshot_test;
fn append(code: source::Code&, bytes: const u8[:]) -> i64 {
    var image = program::Image {}; assert(program::install(image, bytes) == program::Error.None);
    var part = source::Code {}; assert(source::load(part, image)); return source::append_code(code, part);
}
fn invoke(heap: source::Heap&, callee: source::Value, args: const source::Value[:]) -> source::Value {
    var job = source::Job { heap: heap }; assert(source::start(job, callee, args));
    assert(source::run(job, -1) == tiny::Outcome.Complete); let result = job.result; assert(source::retire(job)); return result;
}
fn factory(heap: source::Heap&, entry: i64, rt: source::Value) -> source::Value {
    let maker = invoke(heap, source::closure(heap, entry, []), []); return invoke(heap, maker, [rt]);
}
fn prepare(code: source::Code&, heap: source::Heap&, bindings: snapshot::Binding[>..]&) {
    let churn = append(code, embed_file("../corpus/churn-churn.pants.gsp"));
    let closure = append(code, embed_file("../corpus/closure-captured.pants.gsp"));
    let left = append(code, embed_file("../corpus/branches-helper-0.pants.gsp"));
    let right = append(code, embed_file("../corpus/branches-helper-1.pants.gsp"));
    // Synthetic cell setter only tests the storage/branch boundary. Real
    // compiler-emitted macro/helper bodies above remain unchanged.
    var synthetic = source::Code {};
    synthetic.functions.push(source::Function { start: 0, count: 6, constants: 0, children: 0, params: 1, locals: 1, captures: 1, strict: true });
    for op, i in [46, 3, 50, 46, 49, 15] {
        synthetic.ops.push(source::Op { code: op, arg: 0, offset: [0, 3, 6, 7, 10, 11][i], start: 0, end: 0, depth: [0, 1, 2, 0, 1, 1][i] });
    }
    synthetic.valid = true; let setter = source::append_code(code, synthetic);
    assert(source::initialize(heap)); let rt = source::boot_runtime(heap);
    bindings.push(snapshot::Binding { id: 1, generation: 7, value: factory(heap, churn, rt) }); heap.initializer_count += 1;
    bindings.push(snapshot::Binding { id: 2, generation: 9, value: factory(heap, closure, rt) }); heap.initializer_count += 1;
    bindings.push(snapshot::Binding { id: 3, generation: 2, value: factory(heap, left, rt) }); heap.producer_count += 1;
    bindings.push(snapshot::Binding { id: 4, generation: 3, value: factory(heap, right, rt) }); heap.producer_count += 1;
    let definitions = source::allocate(heap, source::Node { kind: 0 });
    assert(source::install_field(heap, heap.global, "__lightCompilerRoots", definitions));
    assert(source::install_field(heap, definitions, "cljs.user/left-result", bindings[2].value));
    assert(source::install_field(heap, definitions, "cljs.user/right-result", bindings[3].value));
    let cell = source::allocate(heap, source::Node { kind: 3, initialized: true, value: source::number(40) });
    bindings.push(snapshot::Binding { id: 5, generation: 1, value: source::closure(heap, setter, [cell]) });
    bindings.push(snapshot::Binding { id: 6, generation: 1, value: cell });
    let object = source::allocate(heap, source::Node { kind: 0 });
    assert(source::install_field(heap, object, "self", object)); assert(source::install_field(heap, object, "cell", cell));
    assert(source::install_field(heap, object, "nan", source::number(0.0 / 0.0)));
    assert(source::install_field(heap, object, "negativeZero", source::number(-0.0)));
    bindings.push(snapshot::Binding { id: 7, generation: 1, value: object });
    // Import all source/form pools and retain distinct occurrences plus aliases.
    var graph = bridge::Graph {}; graph.bytes.append("one two"); graph.sources.push(source::SourceRange { start: 0, count: 7 });
    graph.units.append([0xD83D, 0xDE00, 0xD800, 'n', 's', 'x']);
    graph.nodes.push(bridge::Form { tag: 2, number: 1, origin: 10, source: 0, span_start: 0, span_end: 3 });
    graph.nodes.push(bridge::Form { tag: 2, number: 1, origin: 11, source: 0, span_start: 4, span_end: 7 });
    graph.nodes.push(bridge::Form { tag: 3, start: 0, count: 3 });
    graph.nodes.push(bridge::Form { tag: 8 });
    graph.nodes.push(bridge::Form { tag: 4, start: 5, count: 1, ns_start: 3, ns_count: 2, metadata: 3 });
    graph.children.append([0, 1, 0, 2, 4]); graph.nodes.push(bridge::Form { tag: 7, start: 0, count: 5 }); graph.roots.push(5);
    var c = bridge::Control {}; var roots: source::Value[>..] = []; assert(bridge::import_graph(heap, graph, roots, c));
    bindings.push(snapshot::Binding { id: 8, generation: 12, value: roots[0] });
    // Neither the cell nor its payload has a direct binding: these are
    // retained solely through the two closure environments after return.
    let hidden = source::allocate(heap, source::Node { kind: 0 }); assert(source::install_field(heap, hidden, "payload", source::number(73)));
    let hidden_cell = source::allocate(heap, source::Node { kind: 3, initialized: true, value: hidden });
    bindings.push(snapshot::Binding { id: 9, generation: 1, value: source::closure(heap, setter, [hidden_cell]) });
    bindings.push(snapshot::Binding { id: 10, generation: 1, value: source::closure(heap, setter, [hidden_cell]) });
    assert(heap.live_jobs == 0 && heap.boot_count == 1 && heap.initializer_count == 2 && heap.producer_count == 2);
}
fn verify(heap: source::Heap&, bindings: const snapshot::Binding[:], n: i64) -> source::Value {
    assert(bindings.len == 10 && bindings[0].id == 1 && bindings[0].generation == 7 && bindings[7].generation == 12);
    assert(heap.boot_count == 1 && heap.initializer_count == 2 && heap.producer_count == 2 && heap.live_jobs == 0);
    let object = bindings[6].value; let cell = bindings[5].value;
    assert(source::get(heap, object, source::text(heap, "self")).id() == object.id());
    assert(source::get(heap, object, source::text(heap, "cell")).id() == cell.id());
    let nan = source::get(heap, object, source::text(heap, "nan")); assert(nan.num() != nan.num());
    let zero = source::get(heap, object, source::text(heap, "negativeZero")); assert(zero.num() == 0 && 1.0 / zero.num() < 0);
    let setter = heap.nodes[bindings[4].value.id()]; assert(heap.values[setter.start].id() == cell.id());
    assert(heap.nodes[cell.id()].value.num() == 40);
    let nodes = heap.nodes.len; assert(invoke(heap, bindings[4].value, [source::number(n)]).num() == n);
    assert(heap.nodes[cell.id()].value.num() == n);
    for i in 32 { assert(source::allocate(heap, source::Node { kind: 0 }).id() == nodes + i); }
    let a = invoke(heap, bindings[0].value, [source::nil(), source::nil(), source::number(16)]); assert(a.num() == 168);
    let input = source::form_collection(heap, 6, [source::number(1), source::number(2)]);
    let expanded = invoke(heap, bindings[1].value, [source::nil(), source::nil(), source::number(n), input]);
    let out = source::form_nth(heap, expanded, 1); assert(source::form_count(heap, out) == 2);
    assert(source::form_nth(heap, source::form_nth(heap, out, 0), 1).num() == n + 4);
    // Results of producer jobs are still present after their Job owners died.
    assert(source::form_count(heap, bindings[2].value) == 2 && source::form_count(heap, bindings[3].value) == 2);
    let definitions = source::get(heap, heap.global, source::text(heap, "__lightCompilerRoots"));
    assert(source::get(heap, definitions, source::text(heap, "cljs.user/left-result")).id() == bindings[2].value.id());
    assert(source::get(heap, definitions, source::text(heap, "cljs.user/right-result")).id() == bindings[3].value.id());
    let form = bindings[7].value; let x = source::form_nth(heap, form, 0); let y = source::form_nth(heap, form, 1);
    assert(x.id() == source::form_nth(heap, form, 2).id() && x.id() != y.id() && source::form_equal(heap, x, y));
    assert(heap.nodes[x.id()].origin == 10 && heap.nodes[y.id()].span_start == 4 && heap.source_bytes[..] == "one two");
    let text = source::form_value(heap, source::form_nth(heap, form, 3)); assert(source::chars(heap, text) == [0xD83D as u16, 0xDE00, 0xD800]);
    let named = heap.nodes[source::form_nth(heap, form, 4).id()]; assert(named.kind == 9 && heap.nodes[named.metadata.id()].kind == 7);
    assert(source::chars(heap, named.namespace_value) == ['n' as u16, 's']);
    assert(source::chars(heap, named.value)[0] == 'x');
    let first = heap.nodes[bindings[8].value.id()]; let second = heap.nodes[bindings[9].value.id()];
    assert(heap.values[first.start].id() == heap.values[second.start].id());
    let hidden = heap.nodes[heap.values[first.start].id()].value;
    assert(source::get(heap, hidden, source::text(heap, "payload")).num() == 73);
    assert(heap.boot_count == 1 && heap.initializer_count == 2 && heap.producer_count == 2);
    return expanded;
}
