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
    for mode in 26 {
        var bad = copy(original);
        match mode {
            0 => { bad.header.version = 2; }, 1 => { bad.header.allocator = 0; }, 2 => { bad.header.ends[6] -= 1; },
            3 => { bad.header.base = 0; }, 4 => { bad.bindings[0].generation = 0; }, 5 => { bad.bindings[1].id = bad.bindings[0].id; },
            6 => { bad.values[0].session += 1; }, 7 => { bad.nodes[bad.bindings[4].value.a].function = bad.functions.len; },
            8 => { bad.properties[0].next = 0; }, 9 => { bad.properties[0].value.a = bad.nodes.len; },
            10 => { bad.nodes[bad.bindings[7].value.a].count = 9999999; }, 11 => { bad.sources[0].count = 9999999; },
            12 => { bad.header.work = bad.header.limits.work + 1; }, 13 => { bad.children[0] = bad.functions.len; },
            14 => { bad.ops[0].depth = 1; }, 15 => { bad.nodes[0].kind = 13; }, 16 => { bad.header.ends[13] += 1; },
            17 => { bad.header.base = 2147483648; },
            18 => { bad.code_bytes[0] = 255; }, 19 => { bad.nodes[bad.bindings[7].value.a].span_start = -9223372036854775807; bad.nodes[bad.bindings[7].value.a].source = 0; },
            20 => { bad.ops[0].arg = 9223372036854775807; }, 21 => { bad.values[0].tag = 6; }, 22 => { bad.header.checkpoint = 0; },
            23 => { bad.header.limits.nodes = -1; }, 24 => { bad.nodes[0].metadata = bad.bindings[5].value; }, 25 => { bad.properties[0].next = 9223372036854775807; }, _ => { abort("unexpected mode"); },
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
fn main() { source_spans(); isolation(); malformed(); stale_classification(); live_boundary(); }

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
