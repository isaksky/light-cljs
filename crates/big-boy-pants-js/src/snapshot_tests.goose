import std;
import .snapshot;
import .snapshot_fixture;
import .source_vm;
import .form_bridge;
import .vm;
import .callable;
import .heap_program;

// Messages between the heap-free main and the thread programs that each own
// one heap (light-cljs-794). A parent captures and publishes; each restore
// runs in a program of its own.
struct Saved { image: snapshot::Snapshot, identity: i64 = 0 }
struct Handle { value: source::Value }
fn isolation() {
    heap_program::join(thread_spawn(isolation_parent)); let saved = qget<Saved>();
    for n in [10, 20] {
        heap_program::join(thread_spawn(isolation_worker, copy(saved.image), saved.identity, n)); let global = qget<Handle>();
        heap_program::join(thread_spawn(isolation_reset, copy(saved.image), global.value));
    }
}
thread_fn isolation_worker(saved: snapshot::Snapshot, parent: i64, n: i64) {
    defer { heap_program::finish(); }
    var artifact = bridge::Graph {};
    {
        var worker_code = source::Code {}; var heap = source::Heap { code: worker_code }; var roots: snapshot::Binding[>..] = [];
        var r = snapshot::Control {}; assert(snapshot::restore(saved, worker_code, heap, roots, source::reserve_identity(), r));
        assert(r.copied_bytes == snapshot::payload_bytes(saved) && heap.identity != parent);
        let result = snapshot_test::verify(heap, roots, n);
        let own, found = source::own_property(heap, roots[6].value, source::text(heap, "parentOnly")); assert(!found && own.tag() == 0);
        if n == 10 { var b = bridge::Control {}; assert(bridge::export_graph(heap, [result], artifact, b)); }
        assert(qput(Handle { value: heap.global }));
    }
    // The exported form outlives the heap that produced it.
    if n == 10 { var b = bridge::Control {}; assert(bridge::validate(artifact, b) && artifact.nodes.len > 0); }
}
// A reset is a fresh arena; the old handle cannot authorize
// replacement slot zero or carry cell immutability into a new object.
thread_fn isolation_reset(saved: snapshot::Snapshot, old: source::Value) {
    defer { heap_program::finish(); }
    // The worker's code exactly as restore installs it, without its heap.
    var worker_code = source::Code {};
    worker_code.source_bytes.append(saved.code_bytes); worker_code.functions.append(saved.functions); worker_code.constants.append(saved.code_constants); worker_code.children.append(saved.children); worker_code.ops.append(saved.ops); worker_code.units.append(saved.code_units); worker_code.strings.append(saved.code_strings); source::decode(worker_code); worker_code.valid = true;
    var reset = source::Heap { code: worker_code }; assert(source::initialize(reset));
    assert(reset.global.id() == 0 && !reset.nodes[0].immutable);
    // No session (light-cljs-785): the old handle names the fresh row 0, the
    // reset global, and inherits nothing from the old heap.
    assert(source::valid(reset, old) && old == reset.global && !reset.nodes[old.id()].immutable);
}
thread_fn isolation_parent() {
    defer { heap_program::finish(); }
    var code = source::Code {}; var parent = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; snapshot_test::prepare(code, parent, bindings);
    var c = snapshot::Control {}; let saved = snapshot::capture(parent, bindings, c);
    assert(c.error == snapshot::Error.None && c.snapshot_bytes == snapshot::footprint(saved) && c.copied_bytes == c.snapshot_bytes);
    assert(saved.header.checkpoint != parent.identity && saved.header.checkpoint > 0 && c.validation_peak == saved.ops.len);
    // Parent changes *after* capture, including code growth and private cycles.
    parent.nodes[bindings[5].value.id()].value = source::number(900);
    assert(source::install_field(parent, bindings[6].value, "parentOnly", source::number(99)));
    source::allocate(parent, source::Node { kind: 0 }); code.source_bytes.append(" ");
    assert(qput(Saved { image: copy(saved), identity: parent.identity }));
    assert(parent.nodes[bindings[5].value.id()].value.num() == 900);
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
    heap_program::join(thread_spawn(malformed_parent)); let saved = qget<Saved>(); let original .= saved.image;
    for mode in 33 { heap_program::join(thread_spawn(malformed_target, copy(original), mode)); }
    for mode in 2 { heap_program::join(thread_spawn(malformed_budget, copy(original), mode)); }
    key_run_budgets(original);
    for identity in [0, original.header.base, original.header.checkpoint, 2147483648] { heap_program::join(thread_spawn(malformed_identity, copy(original), identity)); }
    print("snapshot malformed version/length/link/generation/allocator/code and atomic budget rejection passed");
}
// The occupied source heap refuses a restore without changing.
thread_fn malformed_parent() {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; snapshot_test::prepare(code, heap, bindings);
    var c = snapshot::Control {}; let original = snapshot::capture(heap, bindings, c); assert(c.error == snapshot::Error.None);
    assert(qput(Saved { image: copy(original) }));
    let before_nodes = heap.nodes.len; let before_work = heap.work; let before_identity = heap.identity;
    var occupied = snapshot::Control {};
    assert(!snapshot::restore(original, code, heap, bindings, source::reserve_identity(), occupied) && occupied.error == snapshot::Error.Destination);
    assert(heap.nodes.len == before_nodes && heap.work == before_work && heap.identity == before_identity && code.valid);
}
thread_fn malformed_target(original: snapshot::Snapshot, mode: i64) {
    defer { heap_program::finish(); }
    {
        var bad = copy(original);
        match mode {
            0 => { bad.header.version = 1; }, 1 => { bad.header.allocator = 0; }, 2 => { bad.header.ends[6] -= 1; },
            3 => { bad.header.base = 0; }, 4 => { bad.bindings[0].generation = 0; }, 5 => { bad.bindings[1].id = bad.bindings[0].id; },
            6 => { bad.values[0] = source::object(bad.nodes.len); }, 7 => { bad.nodes[bad.bindings[4].value.id()].function = bad.functions.len as u32; },
            8 => { bad.nodes[bad.header.global.id()].key_count = bad.nodes[bad.header.global.id()].key_capacity + source::inline_slots() as u32 + 1; }, 9 => { bad.properties[0].value = source::object(bad.nodes.len); },
            10 => { bad.nodes[bad.bindings[7].value.id()].count = 9999999; }, 11 => { bad.sources[0].count = 9999999; },
            12 => { bad.header.work = bad.header.limits.work + 1; }, 13 => { bad.children[0] = bad.functions.len as u32; },
            14 => { bad.ops[0].depth = 1; }, 15 => { bad.nodes[0].kind = 13; }, 16 => { bad.header.ends[13] += 1; },
            17 => { bad.header.base = 2147483648; },
            18 => { bad.code_bytes[0] = 255; }, 19 => { bad.nodes[bad.bindings[7].value.id()].span_start = 4294967295; bad.nodes[bad.bindings[7].value.id()].source = 0; },
            20 => { bad.ops[0].arg = 9223372036854775807; }, 21 => { bad.values[0] = source::Value.Symbol { s: 0 }; }, 22 => { bad.header.checkpoint = 0; },
            23 => { bad.header.limits.nodes = -1; }, 24 => { bad.nodes[0].metadata = bad.bindings[5].value; }, 25 => { bad.nodes[bad.header.global.id()].keys = 4294967295; },
            26 => { bad.header.call_method = source::object(bad.nodes.len); }, 27 => { bad.header.call_method = bad.header.global; },
            // Key runs are exclusive over their whole capacity: another node
            // claiming the global's run, even only its spare rows, is refused.
            28 => { let g = bad.header.global.id(); let o = if g == 0 { 1 } else { 0 }; bad.nodes[o].keys = bad.nodes[g].keys; bad.nodes[o].key_capacity = bad.nodes[g].key_capacity; },
            29 => { let g = bad.header.global.id(); assert(source::overflow(bad.nodes[g]) > 0); bad.header.property_rows = source::overflow(bad.nodes[g]) - 1; },
            // Strings keep a canonical encoding (light-cljs-800): a wide run
            // whose units all fit Latin-1, or a Latin-1 run past its table, is refused.
            30 => { var at: i64 = -1; for r, i in bad.strings { if r.wide && r.len > 0 { at = i; } } assert(at >= 0); let r = bad.strings[at]; for i in r.start..(r.start + r.len) { bad.units[i] = 97; } },
            31 => { var at: i64 = -1; for r, i in bad.strings { if !r.wide && r.len > 0 { at = i; } } assert(at >= 0); bad.strings[at].start = bad.latin1.len as u32; },
            // An inline slot past key_count must be cleared (light-cljs-800).
            32 => { var at: i64 = -1; for n, i in bad.nodes { if n.key_count < 2 { at = i; } } assert(at >= 0); bad.nodes[at].slots[1] = source::Property { value: source::number(1) }; },
            _ => { abort("unexpected mode"); },
        }
        var dest = source::Code {}; var target = source::Heap { code: dest }; var roots: snapshot::Binding[>..] = []; var check = snapshot::Control {};
        var wire: u8[>..] = []; to_bytes([bad], wire); var decode = snapshot::Control {};
        let rejected = snapshot::decode(wire, decode); assert(rejected.len == 0 && decode.error != snapshot::Error.None);
        assert(!snapshot::restore(bad, dest, target, roots, source::reserve_identity(), check));
        assert(check.error != snapshot::Error.None && !dest.valid && dest.ops.len == 0 && target.identity == 0 && target.nodes.len == 0 && roots.len == 0);
    }
}
thread_fn malformed_budget(original: snapshot::Snapshot, mode: i64) {
    defer { heap_program::finish(); }
    {
        var dest = source::Code {}; var target = source::Heap { code: dest }; var roots: snapshot::Binding[>..] = []; var check = snapshot::Control {};
        if mode == 0 { check.bytes = snapshot::footprint(original) - 1; } else { check.work = 1; }
        assert(!snapshot::restore(original, dest, target, roots, source::reserve_identity(), check));
        assert(check.error == (if mode == 0 { snapshot::Error.Bytes } else { snapshot::Error.Work }) && target.nodes.len == 0 && !dest.valid);
    }
}
// The key-run ownership bitmap (one byte a property row) obeys its own byte
// and work cuts at their exact boundaries; no heap is needed to validate it.
fn key_run_budgets(saved: const snapshot::Snapshot&) {
    assert(saved.properties.len > 0);
    var tight_bytes = snapshot::Control { bytes: snapshot::footprint(saved) + saved.properties.len - 1 };
    assert(!snapshot::key_runs(saved, tight_bytes) && tight_bytes.error == snapshot::Error.Bytes && tight_bytes.visited == 0 && tight_bytes.validation_peak == 0);
    var exact_bytes = snapshot::Control { bytes: snapshot::footprint(saved) + saved.properties.len };
    assert(snapshot::key_runs(saved, exact_bytes) && exact_bytes.validation_peak == saved.properties.len);
    var exact = snapshot::Control {}; assert(snapshot::key_runs(saved, exact));
    var at_work = snapshot::Control { work: exact.visited }; assert(snapshot::key_runs(saved, at_work) && at_work.visited == exact.visited);
    for cut in [saved.properties.len - 1, exact.visited - 1] {
        var tight_work = snapshot::Control { work: cut };
        assert(!snapshot::key_runs(saved, tight_work) && tight_work.error == snapshot::Error.Work && tight_work.visited <= cut);
    }
}
thread_fn malformed_identity(original: snapshot::Snapshot, identity: i64) {
    defer { heap_program::finish(); }
    var dest = source::Code {}; var target = source::Heap { code: dest }; var roots: snapshot::Binding[>..] = []; var check = snapshot::Control {};
    assert(!snapshot::restore(original, dest, target, roots, identity, check) && check.error == snapshot::Error.Identity && target.identity == 0 && !dest.valid);
}
thread_fn live_boundary() {
    defer { heap_program::finish(); }
    // Adapted lifetime obligations from 158f0cb7/d87ac453:tests/gc.rs:
    // frame-local and operand-only objects survive yields in the existing Job;
    // our checkpoint boundary explicitly refuses to copy any live Job.
    var code = source::Code {};
    code.functions.push(source::Function { start: 0, count: 4, constants: 0, children: 0, params: 0, locals: 1, captures: 0, strict: true });
    for op, i in [16, 4, 3, 15] { code.ops.push(source::Op { code: op, arg: 0, offset: [0, 1, 4, 7][i], start: 0, end: 0, depth: [0, 1, 0, 1][i] }); } source::decode(code); code.valid = true;
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
        assert(source::plain_rows(job.frames) == source::plain_rows(frames_before) && source::plain_rows(job.locals) == source::plain_rows(locals_before) && source::plain_rows(job.operands) == source::plain_rows(operands_before));
    }
    assert(source::run(job, -1) == tiny::Outcome.Complete && job.result.tag() == 5);
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
// Arena-reset translation of 8e0e6219:src/heap/mod.rs's stale
// shareability/generation tests. There is no publication/import cache yet.
// Values carry no session (light-cljs-785): an old handle at a row the fresh
// heap also has names the fresh row and inherits nothing from the old one; a
// handle past the fresh heap's rows fails handle validation.
struct Stale { stale: source::Value, stale_text: source::Value, old_binding: snapshot::Binding, past: source::Value, past_text: source::Value }
fn stale_classification() {
    heap_program::join(thread_spawn(stale_old)); let old = qget<Stale>();
    heap_program::join(thread_spawn(stale_fresh, old));
}
thread_fn stale_old() {
    defer { heap_program::finish(); }
    var code = source::Code {}; snapshot_test::append(code, embed_file("../corpus/serial/calls.gsp"));
    var stale = source::Value.Undefined; var stale_text = source::Value.Undefined;
    var old_binding = snapshot::Binding { id: 1, generation: 3, value: source::Value.Undefined };
    var past = source::Value.Undefined; var past_text = source::Value.Undefined;
    {
        var old = source::Heap { code: code }; assert(source::initialize(old));
        stale = source::allocate(old, source::Node { kind: 3, initialized: true, immutable: true, value: source::number(1) });
        stale_text = source::text(old, "old"); old_binding.value = stale;
        for i in 64 { source::allocate(old, source::Node { kind: 0 }); }
        past = source::object(old.nodes.len - 1); for i in 64 { source::text(old, "padding"); } past_text = source::text(old, "past");
        assert(source::valid(old, stale) && old.nodes[stale.id()].immutable);
    }
    assert(qput(Stale { stale: stale, stale_text: stale_text, old_binding: old_binding, past: past, past_text: past_text }));
}
thread_fn stale_fresh(old: Stale) {
    defer { heap_program::finish(); }
    let stale = old.stale; let stale_text = old.stale_text; let old_binding = old.old_binding;
    var code = source::Code {}; snapshot_test::append(code, embed_file("../corpus/serial/calls.gsp"));
    var fresh = source::Heap { code: code }; assert(source::initialize(fresh));
    let replacement = source::allocate(fresh, source::Node { kind: 0 }); let new_text = source::text(fresh, "new");
    assert(replacement == stale && new_text == stale_text && old_binding.value == replacement && !fresh.nodes[replacement.id()].immutable);
    for value in [old.past, old.past_text] {
        assert(!source::valid(fresh, value) && fresh.fault == source::Fault.StaleID); fresh.fault = source::Fault.None;
    }
    assert(source::valid(fresh, replacement) && source::install_field(fresh, replacement, "writable", source::number(2)));
    print("snapshot reset: handles past the fresh rows are stale; same-row handles name the fresh rows and inherit no immutability");
}
// Symbol and BigInt constants on dead paths have no runtime value: capture,
// restore and recapture keep them unchanged, and the restored code runs.
struct Dormant { image: snapshot::Snapshot, constants: source::Value[], entry: i64 }
fn dormant_constants() {
    heap_program::join(thread_spawn(dormant_capture)); let saved = qget<Dormant>();
    heap_program::join(thread_spawn(dormant_restore, copy(saved)));
}
thread_fn dormant_capture() {
    defer { heap_program::finish(); }
    var code = source::Code {}; let entry = snapshot_test::append(code, embed_file("../corpus/serial/dormant-constants.gsp"));
    var symbol: i64 = -1; var bigints: i64 = 0;
    for v, i in code.constants { if v.tag() == 7 { bigints += 1; if symbol < 0 { symbol = i; } } }
    assert(bigints >= 5 && symbol >= 0);
    // One dormant BigInt becomes a well-known Symbol (exported only by sloppy `with`).
    code.constants[symbol] = source::Value.Symbol { s: 7 };
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var first = snapshot::Control {}; let saved = snapshot::capture(heap, [], first); assert(first.error == snapshot::Error.None);
    assert(qput(Dormant { image: copy(saved), constants: copy(code.constants), entry: entry }));
}
thread_fn dormant_restore(dormant: Dormant) {
    defer { heap_program::finish(); }
    let saved .= dormant.image; let entry = dormant.entry;
    var restored_code = source::Code {}; var restored = source::Heap { code: restored_code }; var roots: snapshot::Binding[>..] = [];
    var restore = snapshot::Control {}; assert(snapshot::restore(saved, restored_code, restored, roots, source::reserve_identity(), restore));
    for v, i in restored.constants { if v.tag() >= 6 { assert(source::plain_rows([v]) == source::plain_rows([dormant.constants[i]])); } }
    var second = snapshot::Control {}; snapshot::capture(restored, [], second); assert(second.error == snapshot::Error.None);
    let result = snapshot_test::invoke(restored, source::closure(restored, entry, []), []); assert(result.tag() == 3 && result.num() == 7);
    print("snapshot: dormant Symbol/BigInt constants survive capture, restore and recapture");
}
fn reused_destination() {
    heap_program::join(thread_spawn(reused_parent)); let saved = qget<Saved>();
    heap_program::join(thread_spawn(reused_target, copy(saved.image)));
}
thread_fn reused_parent() {
    defer { heap_program::finish(); }
    var parent_code = source::Code {}; var parent = source::Heap { code: parent_code }; var original: snapshot::Binding[>..] = []; snapshot_test::prepare(parent_code, parent, original);
    var capture = snapshot::Control {}; let saved = snapshot::capture(parent, original, capture); assert(capture.error == snapshot::Error.None);
    assert(qput(Saved { image: copy(saved) }));
}
// One destination heap, reset and reused for every restore.
thread_fn reused_target(saved: snapshot::Snapshot) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; var job = source::Job { heap: heap };
    var stale = source::Value.Undefined;
    for n in [3, 20, 1, 50, 3, 20] {
        var restore = snapshot::Control {}; assert(snapshot::restore(saved, code, heap, roots, source::reserve_identity(), restore));
        if stale.tag() != 0 { assert(!source::valid(heap, stale) && heap.fault == source::Fault.StaleID); heap.fault = source::Fault.None; }
        assert(source::start(job, roots[0].value, [source::nil(), source::nil(), source::number(n)]));
        assert(!source::reset(job) && !snapshot::reset(code, heap, roots));
        assert(source::claim(job) && !source::reset(job));
        assert(source::run_claimed(job, -1) == tiny::Outcome.Complete && !job.claimed);
        // A row the job added is past the next restore's rows: stale there.
        // Values carry no session, so a row the image has names the new row.
        assert(heap.nodes.len > saved.nodes.len); stale = source::object(heap.nodes.len - 1);
        assert(!source::reset(job) && !snapshot::reset(code, heap, roots));
        assert(source::retire(job) && source::reset(job));
        assert(job.frames.len == 0 && job.locals.len == 0 && job.operands.len == 0 && job.writes.len == 0 && job.handlers.len == 0);
        assert(job.result.tag() == 0 && job.steps == 0 && job.status == tiny::Status.Ready && !job.registered && !job.effect_pending && job.effect_count == 0);
        assert(snapshot::reset(code, heap, roots));
        assert(heap.identity == 0 && heap.global.tag() == 0 && heap.live_jobs == 0 && heap.nodes.len == 0 && heap.work == 0 && roots.len == 0 && !code.valid && code.ops.len == 0);
        var wrong = snapshot::Control {}; assert(!snapshot::restore(saved, code, heap, roots, saved.header.base, wrong) && wrong.error == snapshot::Error.Identity && heap.identity == 0);
    }
    var corrupt = copy(saved); corrupt.nodes[0].kind = 999; var invalid = snapshot::Control {};
    assert(!snapshot::restore(corrupt, code, heap, roots, source::reserve_identity(), invalid) && heap.identity == 0 && roots.len == 0 && !code.valid);
    var restored = snapshot::Control {}; assert(snapshot::restore(saved, code, heap, roots, source::reserve_identity(), restored));
    snapshot_test::verify(heap, roots, 7);
    print("reused code/heap/job destination: live/claimed/terminal guards, fresh IDs, alternating payloads, complete reset and transactional validation passed");
}
// main builds no heap; every heap is its own thread program.
fn main() { dormant_constants(); callable_publication(); finite_map_ownership(); assembly_ownership(); source_spans(); argument_ownership(); isolation(); malformed(); stale_classification(); reused_destination(); heap_program::join(thread_spawn(live_boundary)); }

// The finite map's parent handles that restored heaps look up by index.
struct FiniteMap { image: snapshot::Snapshot, input: source::Value, tail: source::Value, mapped: source::Value, callback: source::Value, alias: source::Value, other: source::Value, later_tail: source::Value }
fn finite_map_ownership() {
    heap_program::join(thread_spawn(finite_map_parent)); let parent = qget<FiniteMap>();
    for i in 2 { heap_program::join(thread_spawn(finite_map_worker, copy(parent))); }
    for mode in 12 { heap_program::join(thread_spawn(finite_map_malformed, copy(parent), mode)); }
    print("finite mapSeq snapshots: immutable aliases, exclusive memo/capture/input ranges, bounded validation and transactional malformed restores passed");
}
thread_fn finite_map_parent() {
    defer { heap_program::finish(); }
    var code = source::Code {}; snapshot_test::append(code, embed_file("../corpus/serial/calls.gsp"));
    snapshot_test::append(code, embed_file("../corpus/serial/captures.gsp"));
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let input = source::form_collection(heap, 5, [source::number(1), source::number(2)]);
    let tail = source::form_sequence(heap, input, 1, false);
    let mapped = source::native_value(heap, 17, [source::builtin(heap, 1), input]);
    var captured: i64 = -1;
    for f, id in code.functions { if f.captures > 0 { captured = id; break; } }
    assert(captured >= 0);
    var captures: source::Value[>..] = []; captures.resize(code.functions[captured].captures, source::number(99));
    let callback = source::closure(heap, captured, captures);
    let alias = source::allocate(heap, source::node(heap, callback.id()));
    let other = source::native_value(heap, 17, [source::builtin(heap, 1), input]);
    // These immutable views overlap legitimately, on both sides of memo nodes.
    let later_tail = source::form_sequence(heap, input, 1, false);
    var c = snapshot::Control {}; let saved = snapshot::capture(heap, [snapshot::Binding { id: 0, generation: 1, value: mapped }, snapshot::Binding { id: 1, generation: 1, value: mapped }], c);
    assert(c.error == snapshot::Error.None);
    assert(qput(FiniteMap { image: copy(saved), input: input, tail: tail, mapped: mapped, callback: callback, alias: alias, other: other, later_tail: later_tail }));
    assert(source::node(heap, mapped.id()).kind == 15 && source::node(heap, mapped.id()).length == 0);
    // The scratch bitmap and aliased-range scans obey their own byte/work cuts.
    var tight_bytes = snapshot::Control { bytes: snapshot::footprint(saved) + saved.values.len - 1 };
    assert(!snapshot::memo_ranges(saved, tight_bytes) && tight_bytes.error == snapshot::Error.Bytes && tight_bytes.validation_peak == 0);
    var exact = snapshot::Control {}; assert(snapshot::memo_ranges(saved, exact));
    assert(exact.validation_peak == saved.values.len);
    for cut in [saved.values.len - 1, exact.visited - 1] {
        var tight_work = snapshot::Control { work: cut };
        assert(!snapshot::memo_ranges(saved, tight_work) && tight_work.error == snapshot::Error.Work && tight_work.visited <= cut);
    }
}
thread_fn finite_map_worker(parent: FiniteMap) {
    defer { heap_program::finish(); }
    let saved .= parent.image; let input = parent.input; let tail = parent.tail; let callback = parent.callback; let alias = parent.alias; let other = parent.other; let later_tail = parent.later_tail;
    {
        var dest = source::Code {}; var worker = source::Heap { code: dest }; var bindings: snapshot::Binding[>..] = []; var control = snapshot::Control {};
        assert(snapshot::restore(saved, dest, worker, bindings, source::reserve_identity(), control));
        let value = bindings[0].value; assert(value == bindings[1].value);
        assert(worker.nodes[value.id()].length == 0);
        var job = source::Job { heap: worker }; assert(source::start(job, source::closure(worker, 0, []), []));
        assert(source::demand_map(job, value));
        while job.frames.len > 1 { assert(source::run(job, 1) == tiny::Outcome.Yielded); }
        assert(source::form_nth(worker, value, 0).num() == 2 && source::form_nth(worker, bindings[1].value, 1).num() == 3);
        assert(source::form_nth(worker, input, 0).num() == 1);
        assert(source::form_nth(worker, tail, 0).num() == 2);
        assert(source::form_nth(worker, later_tail, 0).num() == 2);
        assert(worker.values[worker.nodes[callback.id()].start].num() == 99 && worker.nodes[alias.id()].start == worker.nodes[callback.id()].start);
        assert(worker.nodes[other.id()].kind == 15 && worker.nodes[other.id()].length == 0);
        assert(source::demand_map(job, other));
        while job.frames.len > 1 { assert(source::run(job, 1) == tiny::Outcome.Yielded); }
        assert(source::form_nth(worker, other, 1).num() == 3);
        source::retire(job);
    }
}
thread_fn finite_map_malformed(parent: FiniteMap, mode: i64) {
    defer { heap_program::finish(); }
    let saved .= parent.image; let input = parent.input; let mapped = parent.mapped; let callback = parent.callback; let other = parent.other; let later_tail = parent.later_tail;
    {
        var bad = copy(saved);
        if mode == 0 { bad.nodes[mapped.id()].length = 3; }
        if mode == 1 { bad.nodes[mapped.id()].start = bad.values.len as u32; }
        if mode == 2 { bad.nodes[mapped.id()].namespace_value = mapped; }
        if mode == 3 { bad.nodes[mapped.id()].namespace_value = source::object(bad.nodes.len); }
        if mode == 4 { bad.nodes[mapped.id()].start = bad.nodes[input.id()].start; }
        if mode == 5 { bad.nodes[mapped.id()].start = bad.nodes[input.id()].start + 1; }
        if mode == 6 { bad.nodes[callback.id()].start = bad.nodes[mapped.id()].start; }
        if mode == 7 { bad.nodes[mapped.id()].start = bad.nodes[callback.id()].start; }
        if mode == 8 { bad.nodes[other.id()].start = bad.nodes[mapped.id()].start; }
        if mode == 9 { bad.nodes[other.id()].start = bad.nodes[mapped.id()].start + 1; }
        if mode == 10 { bad.nodes[later_tail.id()].start = bad.nodes[mapped.id()].start; }
        if mode == 11 { bad.nodes[mapped.id()].start = bad.nodes[other.id()].start - 1; }
        var check = snapshot::Control {}; assert(!snapshot::validate(bad, check) && check.error == snapshot::Error.Link);
        var encoded: u8[>..] = []; var encode = snapshot::Control {};
        assert(!snapshot::encode(bad, encoded, encode) && encoded.len == 0 && encode.error == snapshot::Error.Link);
        var wire: u8[>..] = []; to_bytes([bad], wire); var decode = snapshot::Control {};
        assert(snapshot::decode(wire, decode).len == 0 && decode.error == snapshot::Error.Link);
        var dest = source::Code {}; var target = source::Heap { code: dest }; var roots: snapshot::Binding[>..] = []; var restore = snapshot::Control {};
        assert(!snapshot::restore(bad, dest, target, roots, source::reserve_identity(), restore) && restore.error == snapshot::Error.Link && restore.copied_bytes == 0);
        let empty = snapshot::borrow(target, roots, 1);
        assert(snapshot::ends(empty) == [0; 17] && target.identity == 0 && !dest.valid && target.fault == source::Fault.None);
        var retry = snapshot::Control {}; assert(snapshot::restore(saved, dest, target, roots, source::reserve_identity(), retry));
    }
}

struct Assembly { image: snapshot::Snapshot, a: source::Value }
fn assembly_ownership() {
    heap_program::join(thread_spawn(assembly_parent)); let parent = qget<Assembly>();
    for i in 2 { heap_program::join(thread_spawn(assembly_restore, copy(parent), i)); }
    print("snapshot assembly: sequence builtin, concatenated aliases and truncated-array holes restore into isolated private heaps");
}
thread_fn assembly_parent() {
    defer { heap_program::finish(); }
    var code = source::Code {}; snapshot_test::append(code, embed_file("../corpus/serial/calls.gsp"));
    var heap = source::Heap { code: code }; assert(source::initialize(heap)); source::boot_runtime(heap);
    let a = source::allocate(heap, source::Node { kind: 11, value: source::number(7), origin: 2 });
    let b = source::allocate(heap, source::Node { kind: 11, value: source::number(7), origin: 3 });
    let left = source::form_collection(heap, 6, [a, b, a]); let joined = source::form_concat(heap, [left, left]);
    let array = source::allocate(heap, source::Node { kind: 1 });
    let length = source::text(heap, "length");
    for i in 3 { assert(source::put(heap, array, source::key(heap, source::number(i)), if i == 1 { b } else { a })); }
    assert(source::put(heap, array, length, source::number(1)));
    // Growth past a holed run retires it, and a sparse write makes a
    // dictionary array; both images must still validate (light-cljs-785).
    let grown = source::allocate(heap, source::Node { kind: 1 }); let sparse = source::allocate(heap, source::Node { kind: 1 });
    assert(source::put(heap, grown, source::key(heap, source::number(1)), a) && source::put(heap, grown, source::key(heap, source::number(6)), b));
    assert(source::put(heap, sparse, source::key(heap, source::number(2)), a) && source::put(heap, sparse, source::key(heap, source::number(100000)), b));
    assert(source::node(heap, sparse.id()).initialized && !source::node(heap, grown.id()).initialized && source::node(heap, grown.id()).count == 8);
    { var check = snapshot::Control {}; let image = snapshot::capture(heap, [snapshot::Binding { id: 0, generation: 1, value: grown }], check); assert(check.error == snapshot::Error.None && image.nodes.len == heap.nodes.len); }
    let roots = [snapshot::Binding { id: 0, generation: 1, value: array }, snapshot::Binding { id: 1, generation: 1, value: array }, snapshot::Binding { id: 2, generation: 1, value: joined }];
    var c = snapshot::Control {}; let saved = snapshot::capture(heap, roots, c); assert(c.error == snapshot::Error.None);
    assert(qput(Assembly { image: copy(saved), a: a }));
    assert(source::node(heap, array.id()).length == 1 && source::get(heap, array, source::key(heap, source::number(2))).tag() == 0);
    var bad = copy(saved); bad.nodes[array.id()].length = (bad.header.limits.values + 1) as u32;
    var control = snapshot::Control {}; assert(!snapshot::validate(bad, control) && control.error == snapshot::Error.Allocator);
}
thread_fn assembly_restore(parent: Assembly, i: i64) {
    defer { heap_program::finish(); }
    let saved .= parent.image; let a = parent.a;
    {
        var restored_code = source::Code {}; var restored = source::Heap { code: restored_code }; var bindings: snapshot::Binding[>..] = []; var control = snapshot::Control {};
        assert(snapshot::restore(saved, restored_code, restored, bindings, source::reserve_identity(), control));
        assert(bindings[0].value == bindings[1].value);
        let rarray = bindings[0].value; let rjoined = bindings[2].value;
        assert(source::native_value(restored, 16, [rjoined]).id() == rjoined.id());
        assert(source::form_nth(restored, rjoined, 0).id() == source::form_nth(restored, rjoined, 3).id());
        assert(source::form_nth(restored, rjoined, 0).id() != source::form_nth(restored, rjoined, 1).id());
        assert(source::put(restored, rarray, source::text(restored, "length"), source::number(3)));
        assert(source::get(restored, rarray, source::key(restored, source::number(0))).id() == a.id());
        for j in 1..3 { assert(source::get(restored, rarray, source::key(restored, source::number(j))).tag() == 0); }
        assert(source::put(restored, rarray, source::key(restored, source::number(2)), source::number(i + 10)));
    }
}

fn argument_ownership() {
    heap_program::join(thread_spawn(argument_parent)); let saved = qget<Saved>();
    for n in [8, 9] { heap_program::join(thread_spawn(argument_restore, copy(saved.image), n)); }
    print("snapshot arguments: escaped ownership, aliases, repeated calls, isolated restore and invalid range passed");
}
thread_fn argument_parent() {
    defer { heap_program::finish(); }
    var code = source::Code {}; snapshot_test::append(code, embed_file("../corpus/serial/arguments.gsp"));
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
    let arguments = snapshot_test::invoke(heap, getter, []); assert(source::node(heap, arguments.id()).kind == 14);
    var c = snapshot::Control {}; let saved = snapshot::capture(heap, [snapshot::Binding { id: 0, generation: 1, value: getter }, snapshot::Binding { id: 1, generation: 1, value: arguments }, snapshot::Binding { id: 2, generation: 1, value: object }], c); assert(c.error == snapshot::Error.None);
    assert(source::install_field(heap, object, "n", source::number(99)));
    snapshot_test::invoke(heap, maker, [source::number(123)]);
    assert(qput(Saved { image: copy(saved) }));
    var bad = copy(saved); bad.nodes[arguments.id()].start = (bad.values.len + 1) as u32;
    var check = snapshot::Control {}; assert(!snapshot::validate(bad, check) && check.error == snapshot::Error.Link);
}
thread_fn argument_restore(saved: snapshot::Snapshot, n: i64) {
    defer { heap_program::finish(); }
    {
        var worker_code = source::Code {}; var worker = source::Heap { code: worker_code }; var roots: snapshot::Binding[>..] = []; var restore = snapshot::Control {};
        assert(snapshot::restore(saved, worker_code, worker, roots, source::reserve_identity(), restore));
        let args = snapshot_test::invoke(worker, roots[0].value, []); assert(source::equal(worker, args, roots[1].value));
        let left = source::get(worker, args, source::text(worker, "0")); let right = source::get(worker, args, source::text(worker, "1"));
        assert(source::equal(worker, left, right) && source::equal(worker, left, roots[2].value));
        assert(source::get(worker, left, source::text(worker, "n")).num() == 7);
        assert(source::install_field(worker, left, "n", source::number(n)));
        assert(source::get(worker, right, source::text(worker, "n")).num() == n);
        assert(source::equal(worker, source::get(worker, roots[0].value, source::text(worker, "call")), worker.call_method));
    }
}

fn source_spans() {
    heap_program::join(thread_spawn(source_span_parent)); let saved = qget<Saved>(); let id = qget<Handle>().value.id();
    for offset in [1, 3, 4, 5] { for end in [false, true] { heap_program::join(thread_spawn(source_span_target, copy(saved.image), id, offset, end)); } }
    print("snapshot multibyte source start/end corruption and invalid UTF8 rejected transactionally; prior snapshot/heap preserved");
}
thread_fn source_span_target(saved: snapshot::Snapshot, id: i64, offset: i64, end: bool) {
    defer { heap_program::finish(); }
    {
        var bad = copy(saved); if end { bad.nodes[id].span_end = offset as u32; } else { bad.nodes[id].span_start = offset as u32; }
        var dest = source::Code {}; var target = source::Heap { code: dest }; var roots: snapshot::Binding[>..] = []; var check = snapshot::Control {};
        assert(!snapshot::restore(bad, dest, target, roots, source::reserve_identity(), check) && check.error == snapshot::Error.Link && roots.len == 0 && target.nodes.len == 0 && !dest.valid);
        var valid = snapshot::Control {}; assert(snapshot::validate(saved, valid));
    }
}
thread_fn source_span_parent() {
    defer { heap_program::finish(); }
    var code = source::Code {}; snapshot_test::append(code, embed_file("../corpus/serial/calls.gsp"));
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var graph = bridge::Graph {}; graph.bytes.append("é😀"); graph.sources.push(source::SourceRange { start: 0, count: 6 });
    graph.nodes.push(bridge::Form { tag: 2, number: 7, source: 0, span_start: 0, span_end: 6 }); graph.roots.push(0);
    var bc = bridge::Control {}; var values: source::Value[>..] = []; assert(bridge::import_graph(heap, graph, values, bc));
    var c = snapshot::Control {}; let saved = snapshot::capture(heap, [snapshot::Binding { id: 0, generation: 1, value: values[0] }], c); assert(c.error == snapshot::Error.None);
    assert(qput(Saved { image: copy(saved) })); assert(qput(Handle { value: values[0] }));
    var invalid = copy(saved); invalid.bytes[0] = 255; var check = snapshot::Control {}; assert(!snapshot::validate(invalid, check) && check.error == snapshot::Error.Link);
    assert(source::form_value(heap, values[0]).num() == 7 && heap.live_jobs == 0);
}

// The producer's publication and the producer-side handles the consumers'
// corruptions address.
struct Published { wire: u8[], entry: i64, runtime: source::Value, rt_cell: source::Value, two: source::Value, forms: source::Value, cell: source::Value, function: source::Value }
// A closure/cell graph with a runtime capture, a form DAG, two equal
// distinct occurrences and a repeated alias. Code is deliberately absent
// from the base and the consumer appends other code before import.
fn extra_code(extra: source::Code&) {
    extra.source_bytes.append("\n");
    extra.functions.push(source::Function { start: 0, count: 3, constants: 0, children: 0, params: 0, locals: 0, captures: 2, strict: true });
    for op, i in [46, 49, 15] { extra.ops.push(source::Op { code: op, arg: if i == 0 { 1 } else { 0 }, offset: [0, 3, 4][i], start: 0, end: 1, depth: [0, 1, 1][i] }); }
    // A second unreachable function must still be validated on publication.
    extra.functions.push(source::Function { start: 3, count: 2, constants: 0, children: 0, params: 0, locals: 0, captures: 0, strict: true });
    for op, i in [2, 15] { extra.ops.push(source::Op { code: op, arg: 0, offset: i, start: 0, end: 1, depth: i }); } extra.valid = true;
}
fn callable_publication() {
    heap_program::join(thread_spawn(callable_base)); let saved = qget<Saved>(); let base .= saved.image;
    heap_program::join(thread_spawn(callable_producer, copy(base))); let p = qget<Published>(); let wire .= p.wire;
    for cut in [0, 1, wire.len / 2, wire.len - 1] { heap_program::join(thread_spawn(callable_cut, copy(base), str(wire[..cut]))); }
    let records, parsed = from_bytes<callable::Publication[>..]>(wire); assert(parsed && records.len == 1);
    for original in records {
        for mode in -1..16 { heap_program::join(thread_spawn(callable_mode, copy(base), copy(original), copy(p), mode)); }
        for limit in 2 { heap_program::join(thread_spawn(callable_limit, copy(base), copy(original), wire.len, limit)); }
    }
    print("callable code/capture relocation, runtime reconnection, occurrence aliases and transactional malformed/limited publication passed");
}
thread_fn callable_base() {
    defer { heap_program::finish(); }
    var base_code = source::Code {}; snapshot_test::append(base_code, embed_file("../corpus/serial/calls.gsp"));
    var base_heap = source::Heap { code: base_code }; assert(source::initialize(base_heap)); let runtime = source::boot_runtime(base_heap);
    var capture = snapshot::Control {}; let base = snapshot::capture(base_heap, [snapshot::Binding { id: 0, generation: 1, value: runtime }], capture); assert(capture.error == snapshot::Error.None);
    assert(qput(Saved { image: copy(base) }));
}
thread_fn callable_producer(base: snapshot::Snapshot) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var producer = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; var restore = snapshot::Control {};
    assert(snapshot::restore(base, code, producer, bindings, source::reserve_identity(), restore));
    var extra = source::Code {}; extra_code(extra);
    let entry = source::append_code(code, extra);
    let rt_cell = source::allocate(producer, source::Node { kind: 3, initialized: true, immutable: true, value: bindings[0].value });
    producer.source_bytes.append("a b"); producer.source_ranges.push(source::SourceRange { start: 0, count: 3 });
    let one = source::allocate(producer, source::Node { kind: 11, value: source::number(7), origin: 1, source: 0, span_start: 0, span_end: 1 });
    let two = source::allocate(producer, source::Node { kind: 11, value: source::number(7), origin: 2, source: 0, span_start: 2, span_end: 3 });
    let marked = source::form_op(producer, 0, [source::nil(), source::text(producer, "marked")]);
    let metadata = source::form_collection(producer, 7, [marked, source::boolean(true)]); producer.nodes[one.id()].metadata = metadata; producer.nodes[two.id()].metadata = metadata;
    let forms = source::form_collection(producer, 6, [one, two, one]);
    let cell = source::allocate(producer, source::Node { kind: 3, initialized: true, immutable: true, value: forms });
    let function = source::closure(producer, entry, [rt_cell, cell]);
    producer.nodes[function.id()].immutable = true;
    var encoding = snapshot::Control {}; var wire: u8[>..] = []; assert(callable::encode(producer, base, function, wire, encoding, source::reserve_identity()));
    assert(qput(Published { wire: copy(wire), entry: entry, runtime: bindings[0].value, rt_cell: rt_cell, two: two, forms: forms, cell: cell, function: function }));
}
// A truncated publication installs nothing into a fresh consumer.
thread_fn callable_cut(base: snapshot::Snapshot, wire: u8[]) {
    defer { heap_program::finish(); }
    var target_code = source::Code {}; var target = source::Heap { code: target_code }; var roots: snapshot::Binding[>..] = []; var sc = snapshot::Control {};
    assert(snapshot::restore(base, target_code, target, roots, source::reserve_identity(), sc));
    var check = snapshot::Control {}; callable::decode_install(wire, base.header, target_code, target, check);
    assert(check.error != snapshot::Error.None && target.nodes.len == base.nodes.len && target_code.functions.len == base.functions.len);
}
thread_fn callable_mode(base: snapshot::Snapshot, original: callable::Publication, p: Published, mode: i64) {
    defer { heap_program::finish(); }
    let entry = p.entry; let rt_cell = p.rt_cell; let two = p.two; let forms = p.forms; let cell = p.cell; let function = p.function;
    var extra = source::Code {}; extra_code(extra);
    {
        var target_code = source::Code {}; var target = source::Heap { code: target_code }; var roots: snapshot::Binding[>..] = []; var sc = snapshot::Control {};
        assert(snapshot::restore(base, target_code, target, roots, source::reserve_identity(), sc)); source::append_code(target_code, extra);
        // extra has no constants, so heap/code constant counts still agree.
        var bad = copy(original);
        match mode {
            0 => { bad.version += 1; }, 1 => { bad.base.checkpoint += 1; },
            2 => { bad.image.ops[bad.image.ops.len - 1].arg = 1; }, // unreachable return operand
            3 => { bad.image.nodes[cell.id()].immutable = false; },
            4 => { bad.image.nodes[cell.id()].kind = 0; },
            5 => { bad.image.values[bad.image.nodes[forms.id()].start] = bad.image.bindings[0].value; }, // form cycle
            6 => { bad.image.nodes[function.id()].function = bad.image.functions.len as u32; },
            7 => { bad.image.nodes[rt_cell.id()].value = bad.image.header.global; },
            8 => { bad.image.latin1[0] = 255; },
            9 => { bad.image.nodes[two.id()].span_start = 4; },
            10 => { bad.image.functions[entry].start = 0; },
            11 => { bad.image.nodes[forms.id()].metadata = bad.image.bindings[0].value; },
            12 => { bad.image.nodes[function.id()].count += 1; },
            13 => { bad.image.nodes[rt_cell.id()].initialized = false; },
            14 => { bad.image.code_bytes[0] = 32; },
            15 => { bad.image.nodes[p.runtime.id()].length += 1; }, _ => {},
        }
        let before = snapshot::capture(target, roots, sc); assert(sc.error == snapshot::Error.None);
        var encoded: u8[>..] = []; to_bytes([bad], encoded);
        var check = snapshot::Control {}; let imported = callable::decode_install(encoded, base.header, target_code, target, check);
        if mode >= 0 {
            assert(check.error != snapshot::Error.None);
            var after = snapshot::Control {}; var unchanged = snapshot::capture(target, roots, after); unchanged.header.checkpoint = before.header.checkpoint; assert(after.error == snapshot::Error.None);
            var left: u8[>..] = []; var right: u8[>..] = []; to_bytes([before], left); to_bytes([unchanged], right); assert(left == right);
        } else {
            assert(check.error == snapshot::Error.None && imported.tag() == 5);
            assert(target.nodes[imported.id()].function == entry + extra.functions.len && target.nodes[imported.id()].immutable);
            let n = target.nodes[imported.id()]; let captured = target.nodes[target.values[n.start].id()].value;
            assert(captured == roots[0].value);
            let result = snapshot_test::invoke(target, imported, []); let x = source::form_nth(target, result, 0); let y = source::form_nth(target, result, 1);
            assert(x.id() != y.id() && x.id() == source::form_nth(target, result, 2).id() && source::form_equal(target, x, y));
            assert(target.nodes[x.id()].span_start == 0 && target.nodes[y.id()].span_start == 2);
            assert(target.nodes[x.id()].metadata.id() == target.nodes[y.id()].metadata.id() && target.nodes[target.nodes[x.id()].metadata.id()].kind == 7);
            var valid = snapshot::Control {}; let finished = snapshot::capture(target, roots, valid); assert(valid.error == snapshot::Error.None);
        }
    }
}
thread_fn callable_limit(base: snapshot::Snapshot, original: callable::Publication, wire_bytes: i64, limit: i64) {
    defer { heap_program::finish(); }
    var target_code = source::Code {}; var target = source::Heap { code: target_code }; var roots: snapshot::Binding[>..] = []; var sc = snapshot::Control {};
    assert(snapshot::restore(base, target_code, target, roots, source::reserve_identity(), sc));
    var check = snapshot::Control {}; if limit == 0 { check.bytes = wire_bytes; } else { check.work = 1; }
    callable::install(original, base.header, target_code, target, check); assert(check.error != snapshot::Error.None && target.nodes.len == base.nodes.len && target_code.functions.len == base.functions.len);
}
