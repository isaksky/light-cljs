import std;
import .program;
import .source_vm;
import .vm;
import .form_bridge;

fn decode(code: source::Code&, bytes: const u8[:]) {
    var image = program::Image {}; assert(program::install(image, bytes) == program::Error.None); assert(source::load(code, image));
}
fn invoke(heap: source::Heap&, callee: source::Value, args: const source::Value[:]) -> source::Value {
    var job = source::Job { heap: heap }; assert(source::start(job, callee, args));
    let outcome = source::run(job, -1);
    if outcome != tiny::Outcome.Complete { print("failed ", job.fault, " f=", job.fault_function, " pc=", job.fault_pc); }
    assert(outcome == tiny::Outcome.Complete); return job.result;
}
fn macro_factory(heap: source::Heap&, rt: source::Value) -> source::Value {
    let factory = invoke(heap, source::closure(heap, 0, []), []);
    let result = invoke(heap, factory, [rt]); heap.initializer_count += 1; return result;
}
fn churn() {
    var code = source::Code {}; decode(code, embed_file("../corpus/churn-churn.pants.gsp"));
    var heap = source::Heap { code: code }; assert(source::initialize(heap)); let rt = source::boot_runtime(heap);
    let macro = macro_factory(heap, rt);
    for n in [0, 16, 100] { let result = invoke(heap, macro, [source::nil(), source::nil(), source::number(n)]); assert(result.tag == 3 && result.number == (if n == 0 { 0 } else if n == 16 { 168 } else { 5250 })); }
    assert(heap.boot_count == 1 && heap.initializer_count == 1); print("real emitted churn: 0/16/100 passed, boot=1 initializer=1");
}
fn closure() {
    var code = source::Code {}; decode(code, embed_file("../corpus/closure-captured.pants.gsp"));
    var heap = source::Heap { code: code }; assert(source::initialize(heap)); let rt = source::boot_runtime(heap); let macro = macro_factory(heap, rt);
    let input = source::form_collection(heap, 6, [source::number(1), source::number(2), source::number(3)]);
    for base in [7, 10] {
        let result = invoke(heap, macro, [source::nil(), source::nil(), source::number(base), input]);
        let out = source::form_nth(heap, result, 1); assert(source::form_count(heap, out) == 3);
        for i in 3 { let pair = source::form_nth(heap, out, i); assert(source::form_nth(heap, pair, 0).number == i + 1 && source::form_nth(heap, pair, 1).number == base + 4 + i); }
    }
    print("real emitted captured closure/mapv passed");
}
fn main() { churn(); closure(); bridge_smoke(); callbacks_and_failures(); finite_maps(); unsupported_effect(); }

fn finite_maps() {
    var code = source::Code {}; decode(code, embed_file("../corpus/closure-captured.pants.gsp"));
    for cut in 8 {
        var heap = source::Heap { code: code }; assert(source::initialize(heap));
        let callback = source::builtin(heap, 1);
        let input = source::form_collection(heap, 5, [source::number(1), source::number(2), source::number(3)]);
        let mapped = source::native_value(heap, 17, [callback, input]);
        assert(heap.nodes[mapped.a].kind == 15 && heap.nodes[mapped.a].length == 0);
        var job = source::Job { heap: heap }; assert(source::start(job, source::closure(heap, 0, []), []));
        assert(source::demand_map(job, mapped)); source::run(job, cut);
        while job.frames.len > 1 { assert(source::run(job, 1) == tiny::Outcome.Yielded); }
        assert(job.steps == 7 && heap.nodes[mapped.a].kind == 12 && heap.nodes[mapped.a].length == 3);
        for i in 3 { assert(source::form_nth(heap, mapped, i).number == i + 2); }
        assert(!source::demand_map(job, mapped)); source::retire(job);
    }
    for mode in 6 {
        var heap = source::Heap { code: code }; assert(source::initialize(heap));
        let callback = source::builtin(heap, 1); let input = source::form_collection(heap, 5, [source::number(1), source::number(2)]);
        let values = heap.values.len;
        if mode == 0 { heap.limits.values = values + 1; }
        if mode == 1 { heap.limits.work = heap.work + 1; }
        let mapped = source::native_value(heap, 17, [callback, input]);
        if mode < 2 { assert(heap.fault == (if mode == 0 { source::Fault.MemoryLimit } else { source::Fault.WorkLimit }) && heap.values.len == values); continue; }
        var job = source::Job { heap: heap }; assert(source::start(job, source::closure(heap, 0, []), []));
        if mode == 2 { heap.limits.frames = 1; assert(!source::demand_map(job, mapped) && heap.fault == source::Fault.StackLimit); }
        else {
            assert(source::demand_map(job, mapped));
            if mode == 3 { heap.limits.work = heap.work + 1; } else if mode == 4 { heap.limits.steps = 1; } else { heap.limits.stack = 0; }
            assert(source::run(job, -1) == tiny::Outcome.Failed);
            assert(job.fault == (if mode == 3 { source::Fault.WorkLimit } else if mode == 4 { source::Fault.InstructionLimit } else { source::Fault.StackLimit }));
        }
        source::retire(job);
    }
    {
        var heap = source::Heap { code: code }; assert(source::initialize(heap));
        let input = source::form_collection(heap, 5, [source::number(1), source::number(2), source::number(3)]);
        let mapped = source::native_value(heap, 17, [source::builtin(heap, 1), input]);
        let entry = source::closure(heap, 0, []); var job = source::Job { heap: heap };
        assert(source::start(job, entry, []) && source::demand_map(job, mapped));
        source::run(job, 2); assert(heap.nodes[mapped.a].length == 1 && heap.nodes[mapped.a].initialized);
        assert(source::retire(job) && !heap.nodes[mapped.a].initialized && source::reset(job));
        assert(source::start(job, entry, []) && source::demand_map(job, mapped));
        while job.frames.len > 1 { source::run(job, 1); }
        assert(job.steps == 5 && source::form_nth(heap, mapped, 2).number == 4);
        source::retire(job);
    }
    print("finite mapSeq: deferred memo, all budget cuts, value/work/frame/instruction limits passed");
}

fn bridge_smoke() {
    var code = source::Code {}; decode(code, embed_file("../corpus/closure-captured.pants.gsp"));
    var output = bridge::Graph {};
    {
        var heap = source::Heap { code: code }; assert(source::initialize(heap));
        var g = bridge::Graph {}; g.bytes.append("one two"); g.sources.push(source::SourceRange { start: 0, count: 7 });
        g.nodes.push(bridge::Form { tag: 2, number: 1, origin: 0, source: 0, span_start: 0, span_end: 3 });
        g.nodes.push(bridge::Form { tag: 2, number: 1, origin: 1, source: 0, span_start: 4, span_end: 7 });
        g.children.append([0, 1, 0]); g.nodes.push(bridge::Form { tag: 7, start: 0, count: 3 }); g.roots.push(2);
        var c = bridge::Control {}; var roots: source::Value[>..] = [];
        assert(bridge::import_graph(heap, g, roots, c));
        let a = source::form_nth(heap, roots[0], 0); let b = source::form_nth(heap, roots[0], 1); let again = source::form_nth(heap, roots[0], 2);
        assert(a.a != b.a && a.a == again.a && source::form_equal(heap, a, b));
        assert(bridge::export_graph(heap, roots, output, c));
    }
    assert(output.nodes.len == 3 && output.bytes[..] == "one two");
    let root = output.nodes[output.roots[0]];
    assert(output.children[root.start] == output.children[root.start + 2] && output.children[root.start] != output.children[root.start + 1]);
    var c = bridge::Control {}; var bytes: u8[>..] = []; assert(bridge::encode(output, bytes, c));
    var restored = bridge::Graph {}; assert(bridge::decode(restored, bytes, c)); assert(restored.nodes.len == 3);
    print("structured compiler forms: equal distinct occurrences/repeated aliases and independent output passed");
}

fn callbacks_and_failures() {
    var code = source::Code {}; decode(code, embed_file("../corpus/closure-captured.pants.gsp"));
    var heap = source::Heap { code: code }; assert(source::initialize(heap)); let rt = source::boot_runtime(heap); let macro = macro_factory(heap, rt);
    let a = source::allocate(heap, source::Node { kind: 11, value: source::number(1), origin: 0, span_start: 0, span_end: 1 });
    let b = source::allocate(heap, source::Node { kind: 11, value: source::number(1), origin: 0, span_start: 2, span_end: 3 });
    let input = source::form_collection(heap, 6, [a, b]); let params: source::Value[4] = [source::nil(), source::nil(), source::number(7), input];
    let before = heap.nodes.len; var whole = source::Job { heap: heap }; assert(source::start(whole, macro, params)); assert(source::run(whole, -1) == tiny::Outcome.Complete); let allocated = heap.nodes.len - before;
    for cut in (whole.steps + 1) {
        let before = heap.nodes.len; var job = source::Job { heap: heap }; assert(source::start(job, macro, params)); source::run(job, cut); source::run(job, -1);
        assert(job.status == tiny::Status.Complete && job.steps == whole.steps && heap.nodes.len - before == allocated && source::form_equal(heap, whole.result, job.result));
        let vector = source::form_nth(heap, job.result, 1);
        assert(source::form_nth(heap, source::form_nth(heap, vector, 0), 0).a == a.a && source::form_nth(heap, source::form_nth(heap, vector, 1), 0).a == b.a);
    }
    let invalid = source::form_collection(heap, 6, [source::text(heap, "not a number")]); var bad = source::Job { heap: heap };
    assert(source::start(bad, macro, [source::nil(), source::nil(), source::number(7), invalid])); assert(source::run(bad, -1) == tiny::Outcome.Failed && bad.fault == source::Fault.UnsupportedOperand);
    let good = invoke(heap, macro, params); assert(source::form_equal(heap, whole.result, good));
    source::boot_runtime(heap); assert(heap.fault == source::Fault.InvalidState && heap.boot_count == 1); heap.fault = source::Fault.None;
    print("native mapv: every slice preserves callback allocations and distinct scalar occurrences; error/reuse passed");
}
fn unsupported_effect() {
    var code = source::Code {}; decode(code, embed_file("../corpus/effect-announce.pants.gsp"));
    var heap = source::Heap { code: code }; assert(source::initialize(heap)); let rt = source::boot_runtime(heap); let macro = macro_factory(heap, rt);
    var job = source::Job { heap: heap }; assert(source::start(job, macro, [source::nil(), source::nil(), source::number(7)]));
    assert(source::run(job, -1) == tiny::Outcome.Failed && job.fault == source::Fault.NativeBoundary && job.writes.len == 0 && heap.producer_count == 0);
    print("effectful emitted macro rejected before gensym/effects, with no fallback");
}
