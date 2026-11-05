import std;
import .program;
import .source_vm;
import .vm;

fn decode(code: source::Code&, bytes: const u8[:]) {
    var image = program::Image {}; assert(program::install(image, bytes) == program::Error.None);
    assert(source::load(code, image));
}
fn same_state(a: source::Job&, b: source::Job&) {
    // Byte comparisons preserve NaNs and signed zero in every explicit field.
    assert(a.thrown == b.thrown && a.status == b.status && a.steps == b.steps && a.claimed == b.claimed && a.fault == b.fault && a.fault_pc == b.fault_pc && a.fault_function == b.fault_function);
    assert(a.registered == b.registered && a.heap.live_jobs == b.heap.live_jobs);
    assert(to_bytes([a.result]) == to_bytes([b.result]));
    assert(to_bytes(a.frames) == to_bytes(b.frames));
    assert(to_bytes(a.locals) == to_bytes(b.locals));
    assert(to_bytes(a.operands) == to_bytes(b.operands));
    assert(to_bytes(a.writes) == to_bytes(b.writes));
    assert(to_bytes(a.heap.nodes) == to_bytes(b.heap.nodes));
    assert(to_bytes(a.heap.values) == to_bytes(b.heap.values));
    assert(to_bytes(a.heap.properties) == to_bytes(b.heap.properties));
    assert(a.heap.units[..] == b.heap.units[..] && a.heap.identity == b.heap.identity && a.heap.work == b.heap.work && a.heap.fault == b.heap.fault);
}
fn check(bytes: const u8[:], expected: const u8[:], fault: source::Fault = source::Fault.None) {
    var code = source::Code {}; decode(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let entry = source::closure(heap, 0, []);
    var job = source::Job { heap: heap }; assert(source::start(job, entry, []));
    assert(source::run(job, -1) == (if fault == source::Fault.None { tiny::Outcome.Complete } else { tiny::Outcome.Failed }));
    assert(job.fault == fault);
    if fault == source::Fault.None {
        var output: u8[>..] = []; source::canonical(heap, job.result, output); assert(output[..] == expected);
    }
    let steps = job.steps;
    for cut in (steps + 1) {
        var h1 = source::Heap { code: code }; assert(source::initialize(h1));
        var h2 = source::Heap { code: code }; assert(source::initialize(h2));
        // Two differential executions of one logical session image.
        h2.identity = h1.identity; h2.global.session = h1.identity; h2.call_method.session = h1.identity;
        for &v in h2.constants { if v.tag == 4 { v.session = h1.identity; } }
        for &p in h2.properties { p.key.session = h1.identity; p.value.session = h1.identity; }
        let e1 = source::closure(h1, 0, []); let e2 = source::closure(h2, 0, []);
        var a = source::Job { heap: h1 }; var b = source::Job { heap: h2 };
        assert(source::start(a, e1, []) && source::start(b, e2, []));
        let before = h1.work;
        assert(source::run(a, 0) == tiny::Outcome.Yielded && a.steps == 0 && h1.work == before);
        same_state(a, b);
        source::run(a, cut);
        for i in cut { source::run(b, 1); }
        same_state(a, b);
        assert(source::claim(a) == (a.status == tiny::Status.Ready));
        if a.claimed {
            assert(source::run(a, 1) == tiny::Outcome.Busy);
            source::run_claimed(a, 0);
        }
        source::run(a, -1); source::run(b, -1); same_state(a, b);
        assert(a.status == job.status && a.fault == fault && a.steps == steps);
        let stable = a.steps; source::run(a, -1); assert(a.steps == stable);
    }
}
fn limits(bytes: const u8[:]) {
    var code = source::Code {}; decode(code, bytes);
    // The globalThis own property participates in all initialization budgets.
    for kind in 3 {
        var limit = source::Limits {};
        if kind == 0 { limit.properties = 0; }
        if kind == 1 { limit.units = code.units.len; }
        if kind == 2 { limit.work = code.units.len + code.constants.len + 1; }
        var heap = source::Heap { code: code, limits: limit };
        assert(!source::initialize(heap));
        assert(heap.fault == (if kind == 2 { source::Fault.WorkLimit } else { source::Fault.MemoryLimit }));
    }
    for kind in 6 {
        var limit = source::Limits {};
        if kind == 0 { limit.steps = 1; }
        if kind == 1 { limit.frames = 1; }
        if kind == 2 { limit.stack = 1; }
        if kind == 3 { limit.nodes = 2; }
        if kind == 4 { limit.values = 0; }
        var heap = source::Heap { code: code, limits: limit };
        if !source::initialize(heap) { assert(heap.fault == source::Fault.MemoryLimit); continue; }
        if kind == 5 { heap.limits.work = heap.work + 2; }
        let entry = source::closure(heap, 0, []);
        var job = source::Job { heap: heap };
        source::start(job, entry, []); source::run(job, -1);
        assert(job.status == tiny::Status.Failed);
        assert(job.fault == (if kind == 0 { source::Fault.InstructionLimit } else if kind == 1 || kind == 2 { source::Fault.StackLimit } else if kind == 5 { source::Fault.WorkLimit } else { source::Fault.MemoryLimit }));
    }
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let object = source::allocate(heap, source::Node { kind: 0 });
    for i in 200 { source::allocate(heap, source::Node { kind: 0 }); }
    assert(source::valid(heap, object));
    var other = source::Heap { code: code }; assert(source::initialize(other));
    assert(!source::valid(other, object) && other.fault == source::Fault.StaleID);
    heap.fault = source::Fault.None;
    assert(!source::valid(heap, source::Value { tag: 5, a: heap.nodes.len, session: heap.identity }));
}
fn malformed(bytes: const u8[:]) {
    var image = program::Image {}; assert(program::install(image, bytes) == program::Error.None);
    let l = program::layout(bytes);
    var bad: u8[>..] = []; bad.append(bytes);
    // Replace the entry instruction with a same-width consumer at depth zero.
    let first = program::word(bytes, l.functions); let at = l.instructions + first * 20;
    let width = program::word(bytes, at) >> 16;
    let descriptor: i64 = if width == 0 { 5 } else if width == 2 { 131588 } else { 262926 };
    for i in 4 { bad[at + i] = ((descriptor >> (i * 8)) & 255) as u8; bad[at + 4 + i] = 0; }
    assert(program::install(image, bad) == program::Error.None);
    var code = source::Code {}; assert(!source::load(code, image));
    assert(code.error == tiny::LoadError.Underflow);
}
fn factory(bytes: const u8[:]) {
    var code = source::Code {}; decode(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let entry = source::closure(heap, 0, []);
    var job = source::Job { heap: heap }; assert(source::start(job, entry, []));
    assert(source::run(job, -1) == tiny::Outcome.Complete && job.result.tag == 5);
    assert(heap.nodes[job.result.a].kind == 2);
}
fn main() {
    array_lengths();
    check(embed_file("../corpus/synthetic/arithmetic.gsp"), "number:27.5");
    check(embed_file("../corpus/synthetic/control_flow.gsp"), "number:7307.0");
    check(embed_file("../corpus/synthetic/wide.gsp"), "number:299.0");
    check(embed_file("../corpus/synthetic/unicode.gsp"), "utf16:[55357,56832,55296]");
    check(embed_file("../corpus/serial/calls.gsp"), "boolean:true");
    check(embed_file("../corpus/serial/arguments.gsp"), "boolean:true");
    argument_limits(embed_file("../corpus/serial/arguments.gsp"));
    check(embed_file("../corpus/serial/captures.gsp"), "boolean:true");
    check(embed_file("../corpus/serial/mutations.gsp"), "boolean:true");
    check(embed_file("../corpus/serial/control.gsp"), "boolean:true");
    check(embed_file("../corpus/serial/numbers.gsp"), "boolean:true");
    check(embed_file("../corpus/serial/strings.gsp"), "boolean:true");
    check(embed_file("../corpus/serial/global-undefined.gsp"), "undefined");
    check(embed_file("../corpus/serial/global-this.gsp"), "boolean:true");
    check(embed_file("../corpus/serial/global-missing.gsp"), "", source::Fault.ReferenceError);
    check(embed_file("../corpus/serial/negative-zero.gsp"), "number:-0");
    check(embed_file("../corpus/serial/nan.gsp"), "number:NaN");
    check(embed_file("../corpus/serial/tdz.gsp"), "", source::Fault.ReferenceError);
    check(embed_file("../corpus/serial/type-error.gsp"), "", source::Fault.TypeError);
    check(embed_file("../corpus/serial/not-callable.gsp"), "", source::Fault.TypeError);
    check(embed_file("../corpus/serial/unsupported-coercion.gsp"), "", source::Fault.UnsupportedOperand);
    check(embed_file("../corpus/serial/unsupported-prototype.gsp"), "", source::Fault.UnsupportedOperand);
    fixed_cells(embed_file("../corpus/serial/mutations.gsp"));
    failed_job_isolation(embed_file("../corpus/serial/calls.gsp"));
    allocation_limits(embed_file("../corpus/serial/mutations.gsp"));
    flow_errors(embed_file("../corpus/serial/control.gsp"));
    runtime_boundary(embed_file("../corpus/branches-helper-0.pants.gsp"));
    limits(embed_file("../corpus/serial/captures.gsp"));
    factory(embed_file("../corpus/closure-captured.pants.gsp"));
    malformed(embed_file("../corpus/serial/captures.gsp"));
    var code = source::Code {}; decode(code, embed_file("../corpus/serial/loop.gsp"));
    var heap = source::Heap { code: code, limits: source::Limits { steps: 100 } }; assert(source::initialize(heap));
    let entry = source::closure(heap, 0, []); var job = source::Job { heap: heap }; assert(source::start(job, entry, []));
    assert(source::run(job, -1) == tiny::Outcome.Failed && job.fault == source::Fault.InstructionLimit && job.steps == 100);
    print("serial source VM: calls/captures/cells/properties/truth/UTF16/errors, exact slices and limits passed");
}
fn array_lengths() {
    var code = source::Code {}; decode(code, embed_file("../corpus/serial/mutations.gsp"));
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let array = source::allocate(heap, source::Node { kind: 1 }); let length = source::text(heap, "length");
    for i in 4 { assert(source::put(heap, array, source::key(heap, source::number(i)), source::number(i + 10))); }
    assert(source::put(heap, array, length, source::number(2)));
    assert(source::get(heap, array, source::key(heap, source::number(1))).number == 11);
    assert(source::get(heap, array, source::key(heap, source::number(2))).tag == 0);
    assert(source::put(heap, array, length, source::number(4)));
    assert(source::get(heap, array, source::key(heap, source::number(3))).tag == 0);
    assert(source::put(heap, array, source::key(heap, source::number(2)), source::number(99)));
    assert(source::get(heap, array, source::key(heap, source::number(2))).number == 99);
    assert(source::put(heap, array, length, source::number(0)) && heap.nodes[array.a].head == -1);
    assert(source::put(heap, array, length, source::number(3)));
    for i in 3 { assert(source::get(heap, array, source::key(heap, source::number(i))).tag == 0); }
    for n in [-1.0, 1.5, 0.0 / 0.0, 1.0 / 0.0, 4294967296.0] {
        assert(!source::put(heap, array, length, source::number(n)) && heap.fault == source::Fault.UnsupportedOperand);
        heap.fault = source::Fault.None; assert(heap.nodes[array.a].length == 3);
    }
    assert(!source::put(heap, array, length, source::number(heap.limits.values + 1)) && heap.fault == source::Fault.MemoryLimit);
    heap.fault = source::Fault.None;
    assert(source::put(heap, array, source::key(heap, source::number(1)), source::number(7)));
    let before = copy(heap.properties); let head = heap.nodes[array.a].head;
    heap.limits.work = heap.work + 8;
    assert(!source::put(heap, array, length, source::number(0)) && heap.fault == source::Fault.WorkLimit);
    assert(heap.nodes[array.a].length == 3 && heap.nodes[array.a].head == head && to_bytes(heap.properties) == to_bytes(before));
    heap.fault = source::Fault.None; heap.limits = source::Limits {};
    let retained = heap.properties.len; heap.limits.properties = retained;
    assert(source::put(heap, array, length, source::number(0)));
    assert(!source::put(heap, array, source::key(heap, source::number(0)), source::number(1)) && heap.fault == source::Fault.MemoryLimit);
    assert(heap.properties.len == retained && heap.nodes[array.a].length == 0);
    // Exercise every work boundary through the two-pass property unlink,
    // including its final charge, against exactly the same array contents.
    for cut in 40 {
        var h = source::Heap { code: code }; assert(source::initialize(h));
        let a = source::allocate(h, source::Node { kind: 1 }); let k = source::text(h, "length");
        for i in 4 { assert(source::put(h, a, source::key(h, source::number(i)), source::number(i))); }
        let nodes = copy(h.nodes); let properties = copy(h.properties);
        h.limits.work = h.work + cut;
        let ok = source::put(h, a, k, source::number(2));
        if !ok { assert(h.fault == source::Fault.WorkLimit && to_bytes(h.nodes) == to_bytes(nodes) && to_bytes(h.properties) == to_bytes(properties)); }
        else { assert(h.nodes[a.a].length == 2); }
    }
    print("temporary arrays: shorter/zero/extension/deleted holes, numeric bounds, atomic work and retained-property rejection passed");
}
fn argument_limits(bytes: const u8[:]) {
    var code = source::Code {}; decode(code, bytes);
    for op in [101, 179] {
        var function: i64 = -1;
        for f, i in code.functions {
            if f.captures != 0 { continue; }
            for instruction in code.ops[f.start..(f.start + f.count)] { if instruction.code == op { function = i; break; } }
            if function >= 0 { break; }
        }
        assert(function >= 0);
        for limit in 4 {
            var heap = source::Heap { code: code }; assert(source::initialize(heap));
            let entry = source::closure(heap, function, []);
            if limit == 0 { heap.limits.stack = code.functions[function].locals + 2; }
            if limit == 1 { heap.limits.nodes = heap.nodes.len; }
            if limit == 2 {
                if op == 101 { heap.limits.values = heap.values.len + 2; }
                else { heap.limits.properties = heap.properties.len; }
            }
            if limit == 3 { heap.limits.work = heap.work + code.functions[function].locals + 3; }
            var job = source::Job { heap: heap };
            source::start(job, entry, [source::number(1), source::number(2), source::number(3)]);
            assert(source::run(job, -1) == tiny::Outcome.Failed);
            assert(job.fault == (if limit == 0 { source::Fault.StackLimit } else if limit == 3 { source::Fault.WorkLimit } else { source::Fault.MemoryLimit }));
            assert(source::retire(job) && source::reset(job) && job.locals.len == 0);
        }
    }
}

fn fixed_cells(bytes: const u8[:]) {
    // Synthetic opcode variation of a source image, not a const-source claim.
    let l = program::layout(bytes); var variant: u8[>..] = []; variant.append(bytes);
    var changed = false;
    for i in program::word(bytes, 24) {
        let at = l.instructions + i * 20;
        if program::word(bytes, at) == 47 { variant[at] = 197; changed = true; }
    }
    assert(changed); check(variant, "", source::Fault.ConstAssignment);
}
fn failed_job_isolation(bytes: const u8[:]) {
    var code = source::Code {}; decode(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let entry = source::closure(heap, 0, []);
    // A malformed foreign argument fails before entering any frame. A later
    // job still executes on the same initialized session and closure.
    var bad = source::Job { heap: heap };
    assert(!source::start(bad, entry, [source::Value { tag: 5, a: 0, session: heap.identity + 1 }]));
    assert(bad.fault == source::Fault.StaleID && !bad.claimed);
    var good = source::Job { heap: heap }; assert(source::start(good, entry, []));
    assert(source::run(good, -1) == tiny::Outcome.Complete);
    assert(source::equal(heap, good.result, source::boolean(true)));
}
fn allocation_limits(bytes: const u8[:]) {
    var code = source::Code {}; decode(code, bytes);
    for mode in 3 {
        var heap = source::Heap { code: code }; assert(source::initialize(heap));
        let entry = source::closure(heap, 0, []);
        if mode == 0 { heap.limits.properties = 0; }
        if mode == 1 { heap.limits.units = heap.units.len; }
        if mode == 2 { heap.limits.values = 0; }
        var job = source::Job { heap: heap }; assert(source::start(job, entry, []));
        let outcome = source::run(job, -1); assert(outcome == tiny::Outcome.Failed && job.fault == source::Fault.MemoryLimit);
    }
}

fn set_word(bytes: u8[:], at: i64, n: i64) { for i in 4 { bytes[at + i] = ((n >> (i * 8)) & 255) as u8; } }
fn flow_errors(bytes: const u8[:]) {
    let l = program::layout(bytes); let start = program::word(bytes, l.functions); let count = program::word(bytes, l.functions + 4);
    var original = source::Code {}; decode(original, bytes);
    var end: i64 = -1; for i in start..(start + count) { if original.ops[i].code == 15 && original.ops[i].depth >= 0 { end = l.instructions + i * 20; break; } }
    assert(end >= 0);
    var bad: u8[>..] = []; bad.append(bytes); for i in start..(start + count) { if original.ops[i].code == 15 { set_word(bad[..], l.instructions + i * 20, 5); } }
    var image = program::Image {}; assert(program::install(image, bad) == program::Error.None);
    var falling = source::Code {}; assert(!source::load(falling, image) && falling.error == tiny::LoadError.Fallthrough);
    bad.clear(); bad.append(bytes);
    var changed = false;
    for i in count {
        let at = l.instructions + (start + i) * 20;
        if (program::word(bytes, at) & 255) == 14 { assert(original.ops[start + i + 2].depth == 1); set_word(bad[..], at + 4, original.ops[start + i + 2].offset); changed = true; break; }
    }
    assert(changed && program::install(image, bad) == program::Error.None);
    var joined = source::Code {}; let accepted = source::load(joined, image); assert(!accepted && joined.error == tiny::LoadError.Join);
}
fn runtime_boundary(bytes: const u8[:]) {
    var code = source::Code {}; decode(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let entry = source::closure(heap, 0, []); var factory_job = source::Job { heap: heap }; assert(source::start(factory_job, entry, []));
    assert(source::run(factory_job, -1) == tiny::Outcome.Complete);
    var job = source::Job { heap: heap }; assert(source::start(job, factory_job.result, [source::Value {}]));
    assert(source::run(job, -1) == tiny::Outcome.Complete);
    assert(source::form_kind(heap, job.result) == 6 && source::form_count(heap, job.result) == 2);
    assert(source::form_nth(heap, job.result, 0).number == 2 && source::form_nth(heap, job.result, 1).number == 3);
}
