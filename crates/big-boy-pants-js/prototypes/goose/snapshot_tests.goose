import std;
import .snapshot;
import .snapshot_fixture;
import .source_vm;
import .form_bridge;
import .vm;

fn isolation() {
    var code = source::Code {}; var parent = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; snapshot_test::prepare(code, parent, bindings);
    var c = snapshot::Control {}; let saved = snapshot::capture(parent, bindings, c);
    assert(c.error == snapshot::Error.None && c.snapshot_bytes == snapshot::footprint(saved) && c.copied_bytes == c.snapshot_bytes);
    assert(saved.header.checkpoint != parent.identity && saved.header.checkpoint > 0 && c.validation_peak == saved.ops.len);
    // Parent changes *after* capture, including code growth and private cycles.
    parent.nodes[bindings[5].value.a].value = source::number(900);
    assert(source::install_field(parent, bindings[6].value, "parentOnly", source::number(99)));
    source::allocate(parent, source::Node { kind: 0 }); code.source_bytes.append(" ");
    var artifact = bridge::Graph {};
    for n in [10, 20] {
        var worker_code = source::Code {}; var heap = source::Heap { code: worker_code }; var roots: snapshot::Binding[>..] = [];
        var r = snapshot::Control {}; assert(snapshot::restore(saved, worker_code, heap, roots, source::reserve_identity(), r));
        assert(r.copied_bytes == snapshot::payload_bytes(saved) && heap.identity != parent.identity);
        let result = snapshot_test::verify(heap, roots, n);
        let own, found = source::own_property(heap, roots[6].value, source::text(heap, "parentOnly")); assert(!found && own.tag == 0);
        if n == 10 { var b = bridge::Control {}; assert(bridge::export_graph(heap, [result], artifact, b)); }
        // A reset is a fresh arena/identity; the old handle cannot authorize
        // replacement slot zero or carry cell immutability into a new object.
        var reset = source::Heap { code: worker_code }; assert(source::initialize(reset));
        assert(reset.global.a == 0 && !reset.nodes[0].immutable);
        assert(!source::valid(reset, heap.global) && reset.fault == source::Fault.StaleID);
    }
    assert(parent.nodes[bindings[5].value.a].value.number == 900);
    var b = bridge::Control {}; assert(bridge::validate(artifact, b) && artifact.nodes.len > 0);
    var encoded: u8[>..] = []; var enc = snapshot::Control {}; assert(snapshot::encode(saved, encoded, enc));
    assert(enc.copied_bytes == snapshot::footprint(saved) + encoded.len);
    var dec = snapshot::Control {}; let restored = snapshot::decode(encoded, dec); assert(dec.error == snapshot::Error.None && restored.len == 1);
    assert(dec.copied_bytes == 2 * snapshot::footprint(saved));
    for cut in encoded.len {
        var fail = snapshot::Control {}; let no = snapshot::decode(encoded[..cut], fail); assert(no.len == 0 && fail.error != snapshot::Error.None);
    }
    print("snapshot serial isolation/code/form ownership/codec cuts passed: bytes=", c.snapshot_bytes, " capture-ns=", c.capture_ns);
}
fn malformed() {
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; snapshot_test::prepare(code, heap, bindings);
    var c = snapshot::Control {}; let original = snapshot::capture(heap, bindings, c); assert(c.error == snapshot::Error.None);
    for mode in 28 {
        var bad = copy(original);
        match mode {
            0 => { bad.header.version = 1; }, 1 => { bad.header.allocator = 0; }, 2 => { bad.header.ends[6] -= 1; },
            3 => { bad.header.base = 0; }, 4 => { bad.bindings[0].generation = 0; }, 5 => { bad.bindings[1].id = bad.bindings[0].id; },
            6 => { bad.values[0].session += 1; }, 7 => { bad.nodes[bad.bindings[4].value.a].function = bad.functions.len; },
            8 => { bad.properties[0].next = 0; }, 9 => { bad.properties[0].value.a = bad.nodes.len; },
            10 => { bad.nodes[bad.bindings[7].value.a].count = 9999999; }, 11 => { bad.sources[0].count = 9999999; },
            12 => { bad.header.work = bad.header.limits.work + 1; }, 13 => { bad.children[0] = bad.functions.len; },
            14 => { bad.ops[0].depth = 1; }, 15 => { bad.nodes[0].kind = 13; }, 16 => { bad.header.ends[13] += 1; },
            17 => { bad.header.base = 2147483648; },
            18 => { bad.code_bytes[0] = 255; }, 19 => { bad.nodes[bad.bindings[7].value.a].span_start = -9223372036854775807; bad.nodes[bad.bindings[7].value.a].source = 0; },
            20 => { bad.ops[0].arg = 9223372036854775807; }, 21 => { bad.values[0].tag = 6; }, 22 => { bad.header.checkpoint = 0; },
            23 => { bad.header.limits.nodes = -1; }, 24 => { bad.nodes[0].metadata = bad.bindings[5].value; }, 25 => { bad.properties[0].next = 9223372036854775807; },
            26 => { bad.header.call_method.session += 1; }, 27 => { bad.header.call_method = bad.header.global; }, _ => { abort("unexpected mode"); },
        }
        var dest = source::Code {}; var target = source::Heap { code: dest }; var roots: snapshot::Binding[>..] = []; var check = snapshot::Control {};
        var wire: u8[>..] = []; to_bytes([bad], wire); var decode = snapshot::Control {};
        let rejected = snapshot::decode(wire, decode); assert(rejected.len == 0 && decode.error != snapshot::Error.None);
        assert(!snapshot::restore(bad, dest, target, roots, source::reserve_identity(), check));
        assert(check.error != snapshot::Error.None && !dest.valid && dest.ops.len == 0 && target.identity == 0 && target.nodes.len == 0 && roots.len == 0);
    }
    for mode in 2 {
        var dest = source::Code {}; var target = source::Heap { code: dest }; var roots: snapshot::Binding[>..] = []; var check = snapshot::Control {};
        if mode == 0 { check.bytes = snapshot::footprint(original) - 1; } else { check.work = 1; }
        assert(!snapshot::restore(original, dest, target, roots, source::reserve_identity(), check));
        assert(check.error == (if mode == 0 { snapshot::Error.Bytes } else { snapshot::Error.Work }) && target.nodes.len == 0 && !dest.valid);
    }
    for identity in [0, original.header.base, original.header.checkpoint, 2147483648] {
        var dest = source::Code {}; var target = source::Heap { code: dest }; var roots: snapshot::Binding[>..] = []; var check = snapshot::Control {};
        assert(!snapshot::restore(original, dest, target, roots, identity, check) && check.error == snapshot::Error.Identity && target.identity == 0 && !dest.valid);
    }
    let before_nodes = heap.nodes.len; let before_work = heap.work; let before_identity = heap.identity;
    var occupied = snapshot::Control {};
    assert(!snapshot::restore(original, code, heap, bindings, source::reserve_identity(), occupied) && occupied.error == snapshot::Error.Destination);
    assert(heap.nodes.len == before_nodes && heap.work == before_work && heap.identity == before_identity && code.valid);
    print("snapshot malformed version/length/link/generation/allocator/code and atomic budget rejection passed");
}
fn live_boundary() {
    // Adapted lifetime obligations from 158f0cb7/d87ac453:tests/gc.rs:
    // frame-local and operand-only objects survive yields in the existing Job;
    // our checkpoint boundary explicitly refuses to copy any live Job.
    var code = source::Code {};
    code.functions.push(source::Function { start: 0, count: 4, constants: 0, children: 0, params: 0, locals: 1, captures: 0, strict: true });
    for op, i in [16, 4, 3, 15] { code.ops.push(source::Op { code: op, arg: 0, offset: [0, 1, 4, 7][i], start: 0, end: 0, depth: [0, 1, 0, 1][i] }); } code.valid = true;
    var heap = source::Heap { code: code }; assert(source::initialize(heap)); let entry = source::closure(heap, 0, []);
    var job = source::Job { heap: heap }; assert(source::start(job, entry, []));
    assert(source::claim(job) && !source::retire(job) && job.registered && heap.live_jobs == 1);
    assert(source::run_claimed(job, 0) == tiny::Outcome.Yielded && !job.claimed);
    for cut in [1, 1, 1] {
        assert(source::run(job, cut) == tiny::Outcome.Yielded);
        let steps = job.steps; let work = heap.work; let nodes = heap.nodes.len; let operands = job.operands.len; let locals = job.locals.len;
        let frames_before = copy(job.frames); let locals_before = copy(job.locals); let operands_before = copy(job.operands);
        var c = snapshot::Control {}; let no = snapshot::capture(heap, [], c);
        assert(c.error == snapshot::Error.Boundary && no.nodes.len == 0 && c.copied_bytes == 0 && job.steps == steps && heap.work == work && heap.nodes.len == nodes && job.operands.len == operands && job.locals.len == locals && job.status == tiny::Status.Ready);
        assert(bytes_of(job.frames) == bytes_of(frames_before) && bytes_of(job.locals) == bytes_of(locals_before) && bytes_of(job.operands) == bytes_of(operands_before));
    }
    assert(source::run(job, -1) == tiny::Outcome.Complete && job.result.tag == 5);
    var c = snapshot::Control {}; let terminal = snapshot::capture(heap, [], c); assert(c.error == snapshot::Error.Boundary && terminal.nodes.len == 0);
    let result = job.result; assert(source::retire(job));
    var done = snapshot::Control {}; let saved = snapshot::capture(heap, [snapshot::Binding { id: 0, generation: 1, value: result }], done);
    assert(done.error == snapshot::Error.None && saved.nodes.len == heap.nodes.len);
    assert(source::valid(heap, result));
    var abandoned = source::Job { heap: heap }; assert(source::start(abandoned, entry, [])); assert(source::retire(abandoned) && heap.live_jobs == 0 && abandoned.status == tiny::Status.Failed);
    assert(!source::start(abandoned, entry, []) && heap.live_jobs == 0);
    // The serial authority fails before wrapping; restored snapshots never
    // carry or alter it. Keep this last in the process.
    source::next_identity = 2147483647;
    assert(source::reserve_identity() == 2147483647 && source::reserve_identity() == 0 && source::reserve_identity() == 0);
    var exhausted = snapshot::Control {}; let no = snapshot::capture(heap, [], exhausted);
    assert(exhausted.error == snapshot::Error.Identity && exhausted.copied_bytes == 0 && no.nodes.len == 0);
    print("snapshot live operands/locals and terminal-owner rejection, retirement and identity exhaustion passed");
}
fn stale_classification() {
    // Arena-reset translation of 8e0e6219:src/heap/mod.rs's stale
    // shareability/generation tests. There is no publication/import cache yet;
    // stale retained definition entries must fail the same handle validation.
    var code = source::Code {}; snapshot_test::append(code, embed_file("corpus/serial/calls.gsp"));
    var stale = source::Value {}; var stale_text = source::Value {};
    var old_binding = snapshot::Binding { id: 1, generation: 3, value: source::Value {} };
    {
        var old = source::Heap { code: code }; assert(source::initialize(old));
        stale = source::allocate(old, source::Node { kind: 3, initialized: true, immutable: true, value: source::number(1) });
        stale_text = source::text(old, "old"); old_binding.value = stale;
        for i in 64 { source::allocate(old, source::Node { kind: 0 }); }
        assert(source::valid(old, stale) && old.nodes[stale.a].immutable);
    }
    var fresh = source::Heap { code: code }; assert(source::initialize(fresh));
    let replacement = source::allocate(fresh, source::Node { kind: 0 }); let new_text = source::text(fresh, "new");
    assert(replacement.a == stale.a && new_text.a == stale_text.a && replacement.session != stale.session && !fresh.nodes[replacement.a].immutable);
    for value in [stale, stale_text, old_binding.value] {
        assert(!source::valid(fresh, value) && fresh.fault == source::Fault.StaleID); fresh.fault = source::Fault.None;
    }
    assert(source::valid(fresh, replacement) && source::install_field(fresh, replacement, "writable", source::number(2)));
    print("snapshot reset: stale object/string/definition handles cannot authorize replacements or inherit immutability");
}
fn reused_destination() {
    var parent_code = source::Code {}; var parent = source::Heap { code: parent_code }; var original: snapshot::Binding[>..] = []; snapshot_test::prepare(parent_code, parent, original);
    var capture = snapshot::Control {}; let saved = snapshot::capture(parent, original, capture); assert(capture.error == snapshot::Error.None);
    var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; var job = source::Job { heap: heap };
    var stale = source::Value {};
    for n in [3, 20, 1, 50, 3, 20] {
        var restore = snapshot::Control {}; assert(snapshot::restore(saved, code, heap, roots, source::reserve_identity(), restore));
        if stale.tag != 0 { assert(!source::valid(heap, stale) && heap.fault == source::Fault.StaleID); heap.fault = source::Fault.None; }
        stale = heap.global;
        assert(source::start(job, roots[0].value, [source::nil(), source::nil(), source::number(n)]));
        assert(!source::reset(job) && !snapshot::reset(code, heap, roots));
        assert(source::claim(job) && !source::reset(job));
        assert(source::run_claimed(job, -1) == tiny::Outcome.Complete && !job.claimed);
        assert(!source::reset(job) && !snapshot::reset(code, heap, roots));
        assert(source::retire(job) && source::reset(job));
        assert(job.frames.len == 0 && job.locals.len == 0 && job.operands.len == 0 && job.writes.len == 0 && job.handlers.len == 0);
        assert(job.result.tag == 0 && job.steps == 0 && job.status == tiny::Status.Ready && !job.registered && !job.effect_pending && job.effect_count == 0);
        assert(snapshot::reset(code, heap, roots));
        assert(heap.identity == 0 && heap.global.tag == 0 && heap.live_jobs == 0 && heap.nodes.len == 0 && heap.work == 0 && roots.len == 0 && !code.valid && code.ops.len == 0);
        var wrong = snapshot::Control {}; assert(!snapshot::restore(saved, code, heap, roots, saved.header.base, wrong) && wrong.error == snapshot::Error.Identity && heap.identity == 0);
    }
    var corrupt = copy(saved); corrupt.nodes[0].kind = 999; var invalid = snapshot::Control {};
    assert(!snapshot::restore(corrupt, code, heap, roots, source::reserve_identity(), invalid) && heap.identity == 0 && roots.len == 0 && !code.valid);
    var restored = snapshot::Control {}; assert(snapshot::restore(saved, code, heap, roots, source::reserve_identity(), restored));
    snapshot_test::verify(heap, roots, 7);
    print("reused code/heap/job destination: live/claimed/terminal guards, fresh IDs, alternating payloads, complete reset and transactional validation passed");
}
fn main() { finite_map_ownership(); assembly_ownership(); source_spans(); argument_ownership(); isolation(); malformed(); stale_classification(); reused_destination(); live_boundary(); }

fn finite_map_ownership() {
    var code = source::Code {}; snapshot_test::append(code, embed_file("corpus/serial/calls.gsp"));
    snapshot_test::append(code, embed_file("corpus/serial/captures.gsp"));
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let input = source::form_collection(heap, 5, [source::number(1), source::number(2)]);
    let tail = source::form_sequence(heap, input, 1, false);
    let mapped = source::native_value(heap, 17, [source::builtin(heap, 1), input]);
    var captured: i64 = -1;
    for f, id in code.functions { if f.captures > 0 { captured = id; break; } }
    assert(captured >= 0);
    var captures: source::Value[>..] = []; captures.resize(code.functions[captured].captures, source::number(99));
    let callback = source::closure(heap, captured, captures);
    let alias = source::allocate(heap, heap.nodes[callback.a]);
    let other = source::native_value(heap, 17, [source::builtin(heap, 1), input]);
    // These immutable views overlap legitimately, on both sides of memo nodes.
    let later_tail = source::form_sequence(heap, input, 1, false);
    var c = snapshot::Control {}; let saved = snapshot::capture(heap, [snapshot::Binding { id: 0, generation: 1, value: mapped }, snapshot::Binding { id: 1, generation: 1, value: mapped }], c);
    assert(c.error == snapshot::Error.None);
    for i in 2 {
        var dest = source::Code {}; var worker = source::Heap { code: dest }; var bindings: snapshot::Binding[>..] = []; var control = snapshot::Control {};
        assert(snapshot::restore(saved, dest, worker, bindings, source::reserve_identity(), control));
        let value = bindings[0].value; assert(value.a == bindings[1].value.a && value.session == worker.identity);
        assert(worker.nodes[value.a].length == 0 && worker.nodes[value.a].value.session == worker.identity);
        var job = source::Job { heap: worker }; assert(source::start(job, source::closure(worker, 0, []), []));
        assert(source::demand_map(job, value));
        while job.frames.len > 1 { assert(source::run(job, 1) == tiny::Outcome.Yielded); }
        assert(source::form_nth(worker, value, 0).number == 2 && source::form_nth(worker, bindings[1].value, 1).number == 3);
        assert(source::form_nth(worker, snapshot::relocated(input, worker.identity), 0).number == 1);
        assert(source::form_nth(worker, snapshot::relocated(tail, worker.identity), 0).number == 2);
        assert(source::form_nth(worker, snapshot::relocated(later_tail, worker.identity), 0).number == 2);
        assert(worker.values[worker.nodes[callback.a].start].number == 99 && worker.nodes[alias.a].start == worker.nodes[callback.a].start);
        assert(worker.nodes[other.a].kind == 15 && worker.nodes[other.a].length == 0);
        assert(source::demand_map(job, snapshot::relocated(other, worker.identity)));
        while job.frames.len > 1 { assert(source::run(job, 1) == tiny::Outcome.Yielded); }
        assert(source::form_nth(worker, snapshot::relocated(other, worker.identity), 1).number == 3);
        source::retire(job);
    }
    assert(heap.nodes[mapped.a].kind == 15 && heap.nodes[mapped.a].length == 0);
    for mode in 12 {
        var bad = copy(saved);
        if mode == 0 { bad.nodes[mapped.a].length = 3; }
        if mode == 1 { bad.nodes[mapped.a].start = bad.values.len; }
        if mode == 2 { bad.nodes[mapped.a].namespace_value = mapped; }
        if mode == 3 { bad.nodes[mapped.a].namespace_value.session += 1; }
        if mode == 4 { bad.nodes[mapped.a].start = bad.nodes[input.a].start; }
        if mode == 5 { bad.nodes[mapped.a].start = bad.nodes[input.a].start + 1; }
        if mode == 6 { bad.nodes[callback.a].start = bad.nodes[mapped.a].start; }
        if mode == 7 { bad.nodes[mapped.a].start = bad.nodes[callback.a].start; }
        if mode == 8 { bad.nodes[other.a].start = bad.nodes[mapped.a].start; }
        if mode == 9 { bad.nodes[other.a].start = bad.nodes[mapped.a].start + 1; }
        if mode == 10 { bad.nodes[later_tail.a].start = bad.nodes[mapped.a].start; }
        if mode == 11 { bad.nodes[mapped.a].start = bad.nodes[other.a].start - 1; }
        var check = snapshot::Control {}; assert(!snapshot::validate(bad, check) && check.error == snapshot::Error.Link);
        var encoded: u8[>..] = []; var encode = snapshot::Control {};
        assert(!snapshot::encode(bad, encoded, encode) && encoded.len == 0 && encode.error == snapshot::Error.Link);
        var wire: u8[>..] = []; to_bytes([bad], wire); var decode = snapshot::Control {};
        assert(snapshot::decode(wire, decode).len == 0 && decode.error == snapshot::Error.Link);
        var dest = source::Code {}; var target = source::Heap { code: dest }; var roots: snapshot::Binding[>..] = []; var restore = snapshot::Control {};
        assert(!snapshot::restore(bad, dest, target, roots, source::reserve_identity(), restore) && restore.error == snapshot::Error.Link && restore.copied_bytes == 0);
        let empty = snapshot::borrow(target, roots, 1);
        assert(snapshot::ends(empty) == [0; 14] && target.identity == 0 && !dest.valid && target.fault == source::Fault.None);
        var retry = snapshot::Control {}; assert(snapshot::restore(saved, dest, target, roots, source::reserve_identity(), retry));
    }
    // The scratch bitmap and aliased-range scans obey their own byte/work cuts.
    var tight_bytes = snapshot::Control { bytes: snapshot::footprint(saved) + saved.values.len - 1 };
    assert(!snapshot::memo_ranges(saved, tight_bytes) && tight_bytes.error == snapshot::Error.Bytes && tight_bytes.validation_peak == 0);
    var exact = snapshot::Control {}; assert(snapshot::memo_ranges(saved, exact));
    assert(exact.validation_peak == saved.values.len);
    for cut in [saved.values.len - 1, exact.visited - 1] {
        var tight_work = snapshot::Control { work: cut };
        assert(!snapshot::memo_ranges(saved, tight_work) && tight_work.error == snapshot::Error.Work && tight_work.visited <= cut);
    }
    print("finite mapSeq snapshots: immutable aliases, exclusive memo/capture/input ranges, bounded validation and transactional malformed restores passed");
}

fn assembly_ownership() {
    var code = source::Code {}; snapshot_test::append(code, embed_file("corpus/serial/calls.gsp"));
    var heap = source::Heap { code: code }; assert(source::initialize(heap)); source::boot_runtime(heap);
    let a = source::allocate(heap, source::Node { kind: 11, value: source::number(7), origin: 2 });
    let b = source::allocate(heap, source::Node { kind: 11, value: source::number(7), origin: 3 });
    let left = source::form_collection(heap, 6, [a, b, a]); let joined = source::form_concat(heap, [left, left]);
    let array = source::allocate(heap, source::Node { kind: 1 });
    let length = source::text(heap, "length");
    for i in 3 { assert(source::put(heap, array, source::key(heap, source::number(i)), if i == 1 { b } else { a })); }
    assert(source::put(heap, array, length, source::number(1)));
    let roots = [snapshot::Binding { id: 0, generation: 1, value: array }, snapshot::Binding { id: 1, generation: 1, value: array }, snapshot::Binding { id: 2, generation: 1, value: joined }];
    var c = snapshot::Control {}; let saved = snapshot::capture(heap, roots, c); assert(c.error == snapshot::Error.None);
    for i in 2 {
        var restored_code = source::Code {}; var restored = source::Heap { code: restored_code }; var bindings: snapshot::Binding[>..] = []; var control = snapshot::Control {};
        assert(snapshot::restore(saved, restored_code, restored, bindings, source::reserve_identity(), control));
        assert(bindings[0].value.a == bindings[1].value.a && bindings[0].value.session == restored.identity);
        let rarray = bindings[0].value; let rjoined = bindings[2].value;
        assert(source::native_value(restored, 16, [rjoined]).a == rjoined.a);
        assert(source::form_nth(restored, rjoined, 0).a == source::form_nth(restored, rjoined, 3).a);
        assert(source::form_nth(restored, rjoined, 0).a != source::form_nth(restored, rjoined, 1).a);
        assert(source::put(restored, rarray, source::text(restored, "length"), source::number(3)));
        assert(source::get(restored, rarray, source::key(restored, source::number(0))).a == a.a);
        for j in 1..3 { assert(source::get(restored, rarray, source::key(restored, source::number(j))).tag == 0); }
        assert(source::put(restored, rarray, source::key(restored, source::number(2)), source::number(i + 10)));
    }
    assert(heap.nodes[array.a].length == 1 && source::get(heap, array, source::key(heap, source::number(2))).tag == 0);
    var bad = copy(saved); bad.nodes[array.a].length = bad.header.limits.values + 1;
    var control = snapshot::Control {}; assert(!snapshot::validate(bad, control) && control.error == snapshot::Error.Allocator);
    print("snapshot assembly: sequence builtin, concatenated aliases and truncated-array holes restore into isolated private heaps");
}

fn argument_ownership() {
    var code = source::Code {}; snapshot_test::append(code, embed_file("corpus/serial/arguments.gsp"));
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var keeper: i64 = -1;
    for f, id in code.functions {
        var args = false; var closure = false;
        for op in code.ops[f.start..(f.start + f.count)] { args = args || op.code == 101; closure = closure || op.code == 43; }
        if f.captures == 0 && args && closure { keeper = id; break; }
    }
    assert(keeper >= 0);
    let object = source::allocate(heap, source::Node { kind: 0 }); assert(source::install_field(heap, object, "n", source::number(7)));
    let maker = source::closure(heap, keeper, []);
    let getter = snapshot_test::invoke(heap, maker, [object, object]);
    let arguments = snapshot_test::invoke(heap, getter, []); assert(heap.nodes[arguments.a].kind == 14);
    var c = snapshot::Control {}; let saved = snapshot::capture(heap, [snapshot::Binding { id: 0, generation: 1, value: getter }, snapshot::Binding { id: 1, generation: 1, value: arguments }, snapshot::Binding { id: 2, generation: 1, value: object }], c); assert(c.error == snapshot::Error.None);
    assert(source::install_field(heap, object, "n", source::number(99)));
    snapshot_test::invoke(heap, maker, [source::number(123)]);
    for n in [8, 9] {
        var worker_code = source::Code {}; var worker = source::Heap { code: worker_code }; var roots: snapshot::Binding[>..] = []; var restore = snapshot::Control {};
        assert(snapshot::restore(saved, worker_code, worker, roots, source::reserve_identity(), restore));
        let args = snapshot_test::invoke(worker, roots[0].value, []); assert(source::equal(worker, args, roots[1].value));
        let left = source::get(worker, args, source::text(worker, "0")); let right = source::get(worker, args, source::text(worker, "1"));
        assert(source::equal(worker, left, right) && source::equal(worker, left, roots[2].value));
        assert(source::get(worker, left, source::text(worker, "n")).number == 7);
        assert(source::install_field(worker, left, "n", source::number(n)));
        assert(source::get(worker, right, source::text(worker, "n")).number == n);
        assert(source::equal(worker, source::get(worker, roots[0].value, source::text(worker, "call")), worker.call_method));
    }
    var bad = copy(saved); bad.nodes[arguments.a].start = bad.values.len + 1;
    var check = snapshot::Control {}; assert(!snapshot::validate(bad, check) && check.error == snapshot::Error.Link);
    print("snapshot arguments: escaped ownership, aliases, repeated calls, isolated restore and invalid range passed");
}

fn source_spans() {
    var code = source::Code {}; snapshot_test::append(code, embed_file("corpus/serial/calls.gsp"));
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var graph = bridge::Graph {}; graph.bytes.append("é😀"); graph.sources.push(source::SourceRange { start: 0, count: 6 });
    graph.nodes.push(bridge::Form { tag: 2, number: 7, source: 0, span_start: 0, span_end: 6 }); graph.roots.push(0);
    var bc = bridge::Control {}; var values: source::Value[>..] = []; assert(bridge::import_graph(heap, graph, values, bc));
    var c = snapshot::Control {}; let saved = snapshot::capture(heap, [snapshot::Binding { id: 0, generation: 1, value: values[0] }], c); assert(c.error == snapshot::Error.None);
    let id = values[0].a;
    for offset in [1, 3, 4, 5] { for end in [false, true] {
        var bad = copy(saved); if end { bad.nodes[id].span_end = offset; } else { bad.nodes[id].span_start = offset; }
        var dest = source::Code {}; var target = source::Heap { code: dest }; var roots: snapshot::Binding[>..] = []; var check = snapshot::Control {};
        assert(!snapshot::restore(bad, dest, target, roots, source::reserve_identity(), check) && check.error == snapshot::Error.Link && roots.len == 0 && target.nodes.len == 0 && !dest.valid);
        var valid = snapshot::Control {}; assert(snapshot::validate(saved, valid));
    } }
    var invalid = copy(saved); invalid.bytes[0] = 255; var check = snapshot::Control {}; assert(!snapshot::validate(invalid, check) && check.error == snapshot::Error.Link);
    assert(source::form_value(heap, values[0]).number == 7 && heap.live_jobs == 0);
    print("snapshot multibyte source start/end corruption and invalid UTF8 rejected transactionally; prior snapshot/heap preserved");
}
