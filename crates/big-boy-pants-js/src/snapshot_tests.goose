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
        let own, found = source::own_property(heap, roots[6].value, source::text(heap, "parentOnly")); assert(!found && own.is_undefined());
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
    assert(saved.header.checkpoint != parent.identity && saved.header.checkpoint > 0 && c.validation_peak == max(saved.ops.len, max(saved.values.len, saved.properties.len)));
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
    for mode in 45 { heap_program::join(thread_spawn(malformed_target, copy(original), mode)); }
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
// The fixture's key list row (kind 16).
fn list_row(s: const snapshot::Snapshot&) -> i64 { for n, i in s.nodes { if n.kind == 16 { assert(n.key_count > 0); return i; } } abort("no key list"); return -1; }
thread_fn malformed_target(original: snapshot::Snapshot, mode: i64) {
    defer { heap_program::finish(); }
    {
        var bad = copy(original);
        match mode {
            0 => { bad.header.version = 1; }, 1 => { bad.header.allocator = 0; }, 2 => { bad.header.ends[6] -= 1; },
            3 => { bad.header.base = 0; }, 4 => { bad.bindings[0].generation = 0; }, 5 => { bad.bindings[1].id = bad.bindings[0].id; },
            6 => { bad.values[0] = source::object(bad.nodes.len); }, 7 => { bad.nodes[bad.bindings[4].value.id()].function = bad.functions.len as u32; },
            8 => { bad.nodes[bad.header.global.id()].key_count = bad.nodes[bad.header.global.id()].key_capacity + source::inline_slots() as u32 + 1; }, 9 => { let g = bad.header.global.id(); assert(source::overflow(bad.nodes[g]) > 0); bad.values[bad.nodes[g].keys] = source::object(bad.nodes.len); },
            10 => { bad.nodes[bad.bindings[7].value.id()].count = 9999999; }, 11 => { bad.sources[0].count = 9999999; },
            12 => { bad.header.work = bad.header.limits.work + 1; }, 13 => { bad.children[0] = bad.functions.len as u32; },
            14 => { bad.ops[0].depth = 1; }, 15 => { bad.nodes[0].kind = 13; }, 16 => { bad.header.ends[13] += 1; },
            17 => { bad.header.base = 2147483648; },
            18 => { bad.code_bytes[0] = 255; }, 19 => { bad.nodes[bad.bindings[7].value.id()].span_start = 4294967295; bad.nodes[bad.bindings[7].value.id()].source = 0; },
            20 => { bad.ops[0].arg = 9223372036854775807; }, 21 => { bad.values[0] = source::Value.Symbol { s: 99 }; }, 22 => { bad.header.checkpoint = 0; },
            23 => { bad.header.limits.nodes = -1; }, 24 => { bad.nodes[0].metadata = bad.bindings[5].value; }, 25 => { bad.nodes[bad.header.global.id()].keys = 4294967295; },
            26 => { bad.header.call_method = source::object(bad.nodes.len); }, 27 => { bad.header.call_method = bad.header.global; },
            // Key runs are exclusive over their whole capacity: another node
            // claiming the global's run, even only its spare rows, is refused.
            28 => { let g = bad.header.global.id(); let o = if g == 0 { 1 } else { 0 }; bad.nodes[o].keys = bad.nodes[g].keys; bad.nodes[o].key_capacity = bad.nodes[g].key_capacity; },
            // A shape-mode object has its shape's key count (light-cljs-787).
            29 => { let g = bad.header.global.id(); assert(source::overflow(bad.nodes[g]) > 0); bad.nodes[g].key_count -= 1; },
            // Strings keep a canonical encoding (light-cljs-800): a wide run
            // whose units all fit Latin-1, or a Latin-1 run past its table, is refused.
            30 => { var at: i64 = -1; for r, i in bad.strings { if r.wide && r.len > 0 { at = i; } } assert(at >= 0); let r = bad.strings[at]; for i in r.start..(r.start + r.len) { bad.units[i] = 97; } },
            31 => { var at: i64 = -1; for r, i in bad.strings { if !r.wide && r.len > 0 { at = i; } } assert(at >= 0); bad.strings[at].start = bad.latin1.len as u32; },
            // An inline slot past key_count must be cleared (light-cljs-800).
            32 => { var at: i64 = -1; for n, i in bad.nodes { if n.key_count < 2 { at = i; } } assert(at >= 0); bad.nodes[at].slots[1] = source::number(1); },
            // Shapes extend an earlier parent; the key index holds each key at
            // its hash's probe sequence; prototype chains end (light-cljs-787).
            33 => { assert(bad.shapes.len > 2); bad.shapes[1].parent = 2; },
            34 => { var at: i64 = -1; for e, i in bad.keys { if e.row != source::no_row() { at = i; } } assert(at >= 0); bad.keys[at].hash += 1; },
            35 => { let g = bad.header.global.id(); bad.nodes[g].proto = bad.header.global; },
            // A shaped object's overflow run past the values table.
            37 => { let g = bad.header.global.id(); assert(bad.nodes[g].key_capacity > 0); bad.nodes[g].keys = bad.values.len as u32; },
            36 => { var at: i64 = -1; for r, i in bad.strings { if !r.key && r.len > 0 { at = i; } } assert(at >= 0); bad.strings[at].key = true; },
            // An ordinary object's element run (light-cljs-804) is exclusive
            // and holds holes only inside it; it has no length.
            38 => { let o = bad.bindings[6].value.id(); let g = bad.header.global.id(); assert(bad.nodes[o].count > 0 && bad.nodes[g].key_capacity > 0); bad.nodes[o].start = bad.nodes[g].keys; },
            39 => { let o = bad.bindings[6].value.id(); assert(bad.nodes[o].count > 0); bad.nodes[o].count -= 1; },
            40 => { let o = bad.bindings[6].value.id(); bad.nodes[o].length = 1; },
            // A key list's position lies in its records; it walks an object
            // or nothing, and its records are keyed.
            41 => { let l = list_row(bad); bad.nodes[l].length = bad.nodes[l].key_count + 1; },
            42 => { let l = list_row(bad); bad.nodes[l].start = bad.nodes[l].length + 1; },
            43 => { let l = list_row(bad); bad.nodes[l].value = source::number(1); },
            44 => { let l = list_row(bad); bad.properties[bad.nodes[l].keys].key = source::number(1); },
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
    for op, i in [opcode::NewObject, opcode::SetLocal, opcode::GetLocal, opcode::Return] { code.ops.push(source::Op { code: op, arg: 0, offset: [0, 1, 4, 7][i], start: 0, end: 0, depth: [0, 1, 0, 1][i] }); } source::decode(code); code.valid = true;
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
    assert(source::run(job, -1) == tiny::Outcome.Complete && job.result.is_object());
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
    for v, i in code.constants { if v.is_bigint() { bigints += 1; if symbol < 0 { symbol = i; } } }
    assert(bigints >= 5 && symbol >= 0);
    // One dormant BigInt becomes a well-known Symbol (exported only by sloppy `with`).
    code.constants[symbol] = source::Value.Symbol { s: well_known::Unscopables };
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var first = snapshot::Control {}; let saved = snapshot::capture(heap, [], first); assert(first.error == snapshot::Error.None);
    assert(qput(Dormant { image: copy(saved), constants: copy(code.constants), entry: entry }));
}
thread_fn dormant_restore(dormant: Dormant) {
    defer { heap_program::finish(); }
    let saved .= dormant.image; let entry = dormant.entry;
    var restored_code = source::Code {}; var restored = source::Heap { code: restored_code }; var roots: snapshot::Binding[>..] = [];
    var restore = snapshot::Control {}; assert(snapshot::restore(saved, restored_code, restored, roots, source::reserve_identity(), restore));
    for v, i in restored.constants { if v.tag() >= value_tag::Symbol { assert(source::plain_rows([v]) == source::plain_rows([dormant.constants[i]])); } }
    var second = snapshot::Control {}; snapshot::capture(restored, [], second); assert(second.error == snapshot::Error.None);
    let result = snapshot_test::invoke(restored, source::closure(restored, entry, []), []); assert(result.is_number() && result.num() == 7);
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
        // One decoded height a function: a reset leaves none of the last restore's.
        assert(code.heights.len == code.functions.len && code.ins.len == code.ops.len);
        if !stale.is_undefined() { assert(!source::valid(heap, stale) && heap.fault == source::Fault.StaleID); heap.fault = source::Fault.None; }
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
        assert(job.result.is_undefined() && job.steps == 0 && job.status == tiny::Status.Ready && !job.registered && !job.effect_pending && job.effect_count == 0);
        assert(snapshot::reset(code, heap, roots));
        assert(heap.identity == 0 && heap.global.is_undefined() && heap.live_jobs == 0 && heap.nodes.len == 0 && heap.work == 0 && roots.len == 0 && !code.valid && code.ops.len == 0 && code.heights.len == 0);
        var wrong = snapshot::Control {}; assert(!snapshot::restore(saved, code, heap, roots, saved.header.base, wrong) && wrong.error == snapshot::Error.Identity && heap.identity == 0);
    }
    var corrupt = copy(saved); corrupt.nodes[0].kind = 999; var invalid = snapshot::Control {};
    assert(!snapshot::restore(corrupt, code, heap, roots, source::reserve_identity(), invalid) && heap.identity == 0 && roots.len == 0 && !code.valid);
    var restored = snapshot::Control {}; assert(snapshot::restore(saved, code, heap, roots, source::reserve_identity(), restored));
    snapshot_test::verify(heap, roots, 7);
    print("reused code/heap/job destination: live/claimed/terminal guards, fresh IDs, alternating payloads, complete reset and transactional validation passed");
}
// A reused job worker's image (light-cljs-795): a base image restored once,
// a derived snapshot installed over it and the tables truncated back.
struct Derived { base: snapshot::Snapshot, derived: snapshot::Snapshot, service: i64 }
fn truncated_image() {
    heap_program::join(thread_spawn(truncated_parent)); let saved = qget<Derived>();
    heap_program::join(thread_spawn(truncated_worker, copy(saved.base), copy(saved.derived), saved.service));
}
thread_fn truncated_parent() {
    defer { heap_program::finish(); }
    var code = source::Code {}; var parent = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; snapshot_test::prepare(code, parent, bindings);
    let service = source::allocate(parent, source::Node { kind: 0 }); assert(source::install_field(parent, parent.global, "service", service));
    // The service object has an element in the image (light-cljs-804).
    assert(source::install_field(parent, service, "1", source::number(1)) && source::node(parent, service.id()).count == 4);
    var c = snapshot::Control {}; let base = snapshot::capture(parent, bindings, c); assert(c.error == snapshot::Error.None);
    // The derived image: base, a definition's code and rows above it.
    var dc = source::Code {}; var derived = source::Heap { code: dc }; var db: snapshot::Binding[>..] = []; var r = snapshot::Control {}; assert(snapshot::restore(base, dc, derived, db, source::reserve_identity(), r));
    var extra = source::Code {}; extra_code(extra); assert(source::append_code(dc, extra) >= 0);
    let o = source::allocate(derived, source::Node { kind: 0 }); assert(source::install_field(derived, o, "derived", source::text(derived, "row above the image")));
    for i in 3 { assert(source::install_field(derived, o, str("k", i), source::number(i))); }
    db.push(snapshot::Binding { id: 100, generation: 1, value: o });
    var d = snapshot::Control {}; let image = snapshot::capture(derived, db, d); assert(d.error == snapshot::Error.None);
    assert(qput(Derived { base: copy(base), derived: copy(image), service: service.id() }));
}
thread_fn truncated_worker(base: snapshot::Snapshot, derived: snapshot::Snapshot, service: i64) {
    defer { heap_program::finish(); }
    let identity = source::reserve_identity(); let rows: i64[1] = [service];
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; var r = snapshot::Control {};
    assert(snapshot::restore(base, code, heap, bindings, identity, r)); assert(snapshot::truncate(base, rows, identity, code, heap, bindings));
    assert(snapshot::pristine(base, identity, code, heap, bindings) && snapshot::over_image(derived, base, rows));
    for n in [3, 20, 1] {
        // Installed over the image, the derived snapshot's tables are a
        // fresh restore's, row for row.
        let job_identity = source::reserve_identity(); var c = snapshot::Control {};
        assert(snapshot::extend(derived, base, rows, code, heap, bindings, job_identity, c));
        assert(snapshot::pristine(derived, job_identity, code, heap, bindings) && heap.image.nodes == derived.nodes.len);
        assert(c.copied_bytes == snapshot::payload_bytes(derived) - snapshot::payload_bytes(base));
        // A destination not at the image refuses another install.
        var twice = snapshot::Control {}; assert(!snapshot::extend(derived, base, rows, code, heap, bindings, source::reserve_identity(), twice) && twice.error == snapshot::Error.Destination);
        // A job's garbage and a compiler service's write into an image row.
        var job = source::Job { heap: heap };
        assert(source::start(job, bindings[0].value, [source::nil(), source::nil(), source::number(n)]));
        assert(source::run(job, -1) == tiny::Outcome.Complete && heap.nodes.len > derived.nodes.len);
        assert(!snapshot::truncate(base, rows, identity, code, heap, bindings));
        assert(source::retire(job));
        for i in 4 { assert(source::install_field(heap, source::object(service), str("s", i), source::number(i))); }
        // The service writes its image element in place, then grows the run past it.
        assert(source::install_field(heap, source::object(service), "1", source::number(n)) && source::install_field(heap, source::object(service), "9", source::number(9)));
        assert(source::node(heap, service).count > 4);
        assert(!snapshot::pristine(base, identity, code, heap, bindings));
        assert(snapshot::truncate(base, rows, identity, code, heap, bindings) && snapshot::pristine(base, identity, code, heap, bindings));
    }
    // An image row other than a service row is not the image's.
    var changed = copy(derived); changed.nodes[0].immutable = !changed.nodes[0].immutable; assert(!snapshot::over_image(changed, base, rows));
    print("reused worker image: install over the image equals restore, truncation and service rows restore the twin");
}
// Shapes and interned keys on a reused worker (light-cljs-787, amended by
// light-cljs-792): jobs add transitions to image shapes and keys to the
// image's key index, and the reset restores both, so two jobs building the
// same layouts get the same shapes, keys, key order and results, and the
// reset check passes after each.
fn reused_layouts() {
    heap_program::join(thread_spawn(layout_parent)); let base = qget<snapshot::Snapshot>();
    heap_program::join(thread_spawn(layout_worker, copy(base)));
    print("reused worker layouts: identical shapes, keys, key order and results in two jobs; the reset restores image shapes and keys");
}
thread_fn layout_parent() {
    defer { heap_program::finish(); }
    var code = source::Code {}; snapshot_test::append(code, embed_file("../corpus/serial/accessors.gsp"));
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    // An image object whose shapes and keys are image rows.
    let point = source::allocate(heap, source::Node { kind: 0 }); assert(source::install_field(heap, heap.global, "point", point));
    assert(source::install_field(heap, point, "x", source::number(1)) && source::install_field(heap, point, "y", source::number(2)));
    var bindings: snapshot::Binding[>..] = [snapshot::Binding { id: 1, generation: 1, value: source::closure(heap, 0, []) }, snapshot::Binding { id: 2, generation: 1, value: point }];
    var c = snapshot::Control {}; let base = snapshot::capture(heap, bindings, c);
    assert(c.error == snapshot::Error.None && base.shapes.len > 2 && base.keys.len > 0);
    assert(qput(base));
}
thread_fn layout_worker(base: snapshot::Snapshot) {
    defer { heap_program::finish(); }
    let identity = source::reserve_identity(); let rows: i64[] = [];
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; var r = snapshot::Control {};
    assert(snapshot::restore(base, code, heap, bindings, identity, r) && snapshot::truncate(base, rows, identity, code, heap, bindings));
    let point = bindings[1].value; let image = heap.image;
    var first: u8[>..] = [];
    for run in 2 {
        var job = source::Job { heap: heap };
        assert(source::start(job, bindings[0].value, []) && source::run(job, -1) == tiny::Outcome.Complete && job.result.is_bool() && job.result.flag());
        assert(source::retire(job));
        // The same keys in the same order share the image object's shape; a
        // new key extends it above the image, and the image shape gets a child.
        let p = source::allocate(heap, source::Node { kind: 0 });
        assert(source::put(heap, p, source::text(heap, "x"), source::number(3)) && source::put(heap, p, source::text(heap, "y"), source::number(4)));
        assert(source::node(heap, p.id()).shape == source::node(heap, point.id()).shape && source::node(heap, p.id()).shape < image.shapes);
        assert(source::put(heap, p, source::text(heap, "z"), source::number(5)) && source::node(heap, p.id()).shape >= image.shapes && heap.image_written);
        var keys:  source::Value[>..] = []; assert(source::own_keys(heap, p, keys) && keys.len == 3);
        var state: u8[>..]            = source::plain_rows(heap.shapes); state.append(bytes_of(heap.keys)); state.append(bytes_of(heap.strings)); state.append(source::plain_rows(keys));
        for k in keys { for u in source::utf16(heap, k) { state.push(u as u8); } }
        assert(source::key_is(heap, keys[0], "x") && source::key_is(heap, keys[1], "y") && source::key_is(heap, keys[2], "z"));
        if run == 0 { first.append(state); } else { assert(state[..] == first[..]); }
        assert(snapshot::truncate(base, rows, identity, code, heap, bindings) && snapshot::pristine(base, identity, code, heap, bindings) && !heap.image_written);
    }
}
// main builds no heap; every heap is its own thread program.
fn main() { dormant_constants(); callable_publication(); finite_map_ownership(); assembly_ownership(); source_spans(); argument_ownership(); isolation(); malformed(); stale_classification(); reused_destination(); truncated_image(); reused_layouts(); suspended_continuations(); heap_program::join(thread_spawn(live_boundary)); }

// Suspended continuations (light-cljs-790; decision 4, amended by
// light-cljs-792). A dependency wait while a generator is suspended at its
// yield and an async function awaits: answered in place, the worker keeps
// both in its tables; saved as a continuation, the snapshot carries their
// rows and the job's whole stack as one more activation row
// (light-cljs-813), its promise jobs carry the rest, and a fresh heap that
// restores them completes the job the same way. Then a reused
// worker: a generator one job leaves suspended is gone after the reset,
// and the next job cannot reach it.
struct Waiting {
    image: snapshot::Snapshot = snapshot::Snapshot {}, stack: u32 = 4294967295,
    queue: source::Task[]     = [], queue_head: i64 = 0, draining: bool = false, entry: source::Value = source::Value.Undefined, rejections: u32[] = [],
    steps: i64                = 0, kind: i64 = 0, count: i64 = 0, frame: source::Frame = source::Frame { function: -1, pc: -1, local_base: 0, operand_base: 0, callee: source::Value.Undefined, receiver: source::Value.Undefined },
    args:  source::Value[8]   = [source::Value.Undefined; 8],
}
fn suspended_continuations() {
    let wait = embed_file("../corpus/serial/suspended-wait.gsp");
    heap_program::join(thread_spawn(suspended_wait, str(wait), false));
    heap_program::join(thread_spawn(suspended_wait, str(wait), true)); let saved = qget<Waiting>();
    heap_program::join(thread_spawn(resumed_wait, copy(saved)));
    let stacked = embed_file("../corpus/serial/suspended-stack.gsp");
    heap_program::join(thread_spawn(stack_limits, str(stacked)));
    heap_program::join(thread_spawn(stack_wait, str(stacked))); let first = qget<Waiting>();
    heap_program::join(thread_spawn(stack_resumed, copy(first), false)); let second = qget<Waiting>();
    heap_program::join(thread_spawn(stack_resumed, copy(second), true));
    heap_program::join(thread_spawn(suspension_parent, str(embed_file("../corpus/serial/suspended-generator.gsp")))); let base = qget<snapshot::Snapshot>();
    heap_program::join(thread_spawn(suspension_worker, copy(base)));
    let constructing = embed_file("../corpus/serial/construct-wait.gsp");
    heap_program::join(thread_spawn(construct_wait, str(constructing), false));
    heap_program::join(thread_spawn(construct_wait, str(constructing), true)); let waiting = qget<Waiting>();
    heap_program::join(thread_spawn(construct_resumed, copy(waiting)));
    print("suspended continuations: a dependency wait keeps them in place or carries them, with its whole stack as rows (native frames included), in a saved continuation; a reused worker's next job cannot reach them");
}
// Run suspended-wait.js to its dependency wait, with a generator and an
// async function suspended; answer it in place, or save the continuation.
thread_fn suspended_wait(bytes: u8[], save: bool) {
    defer { heap_program::finish(); }
    var code = source::Code {}; snapshot_test::append(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let dep = source::builtin(heap, 8);
    var job = source::Job { heap: heap, effects_enabled: true }; assert(source::start(job, source::closure(heap, 0, []), [dep]));
    assert(source::run(job, -1) == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8);
    // The generator's and the async function's activations are rows.
    var activations: i64 = 0; for c in heap.conts { match c { Activation => { activations += 1; }, _ => {} } }
    assert(activations == 4 && job.frames.len == 1);
    if !save {
        assert(source::deliver_effect(job, source::text(heap, "D")));
        assert(source::run(job, -1) == tiny::Outcome.Complete && source::key_is(heap, job.result, "axDbfypz"));
        assert(source::retire(job)); return;
    }
    // The stack leaves the job: the continuation carries it only as rows.
    let stack = source::suspend_stack(job, -1); assert(stack >= 0 && job.frames.len == 0 && job.locals.len == 0 && job.operands.len == 0 && job.handlers.len == 0);
    let a = source::activation(heap, stack); assert(a.frame_count == 1 && a.handler_count == 1 && snapshot::cont_kind(source::cont_row(heap, a.handlers)) == 1);
    assert(source::retire(job, true));
    var c = snapshot::Control {}; let image = snapshot::capture(heap, [], c); assert(c.error == snapshot::Error.None && image.conts.len == heap.conts.len);
    // Malformed continuation rows are refused before a restore: an async
    // boundary naming no promise, a pending promise's reaction cycle, and a
    // reaction list whose `last` is not its end.
    for mode in 3 {
        // af's promise has two pending reactions (the two `also` awaits).
        var bad = copy(image); var changed = false;
        for row, i in image.conts {
            match row {
                Saved s => { if mode == 0 && s.h.kind == source::HandlerKind.Async { var h = s.h; h.value = source::number(0); bad.conts[i] = source::Cont.Saved { h: h }; changed = true; } },
                Promise p => {
                    if mode > 0 && p.p.first != source::no_row() && p.p.first != p.p.last {
                        if mode == 1 { match image.conts[p.p.last] { Reaction r => { var x = r.r; x.next = p.p.first; bad.conts[p.p.last] = source::Cont.Reaction { r: x }; }, _ => {} } }
                        else { var s = p.p; s.last = s.first; bad.conts[i] = source::Cont.Promise { p: s }; }
                        changed = true;
                    }
                },
                _ => {},
            }
        }
        assert(changed);
        var check = snapshot::Control {}; assert(!snapshot::validate(bad, check) && check.error == snapshot::Error.Link);
        var dest = source::Code {}; var target_heap = source::Heap { code: dest }; var roots: snapshot::Binding[>..] = []; var refused = snapshot::Control {};
        assert(!snapshot::restore(bad, dest, target_heap, roots, source::reserve_identity(), refused) && refused.error == snapshot::Error.Link && target_heap.nodes.len == 0);
    }
    assert(qput(Waiting { image: copy(image), stack: stack as u32, queue: copy(job.queue), queue_head: job.queue_head, draining: job.draining, entry: job.entry, rejections: copy(job.rejections), steps: job.steps, kind: job.effect_kind, count: job.effect_count, frame: job.effect_frame, args: job.effect_args }));
}
// A fresh heap restores the saved continuation and completes the job.
thread_fn resumed_wait(saved: Waiting) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; var r = snapshot::Control {};
    assert(snapshot::restore(saved.image, code, heap, bindings, source::reserve_identity(), r));
    var job = source::Job { heap: heap, effects_enabled: true };
    assert(source::resume_stack(job, saved.stack) && job.frames.len == 1);
    for t in saved.queue { job.queue.push(t); } job.queue_head = saved.queue_head; job.draining = saved.draining; job.entry = saved.entry; for p in saved.rejections { job.rejections.push(p); }
    job.steps = saved.steps; job.registered = true; heap.live_jobs += 1; job.effect_pending = true; job.effect_kind = saved.kind; job.effect_count = saved.count; job.effect_frame = saved.frame; for v, i in saved.args { job.effect_args[i] = v; }
    assert(source::deliver_effect(job, source::text(heap, "D")));
    assert(source::run(job, -1) == tiny::Outcome.Complete && source::key_is(heap, job.result, "axDbfypz"));
    assert(source::retire(job));
}
// A dependency wait inside a constructor (construct-wait.js, light-cljs-831):
// the constructor's frame holds its new.target and its new object, which
// replaces the primitive it returns after the wait, answered in place or
// after the whole stack was saved as a continuation and restored in a fresh
// heap (construct_resumed).
thread_fn construct_wait(bytes: u8[], save: bool) {
    defer { heap_program::finish(); }
    var code = source::Code {}; snapshot_test::append(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var job = source::Job { heap: heap, effects_enabled: true }; assert(source::start(job, source::closure(heap, 0, []), [source::builtin(heap, 8)]));
    assert(source::run(job, -1) == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8);
    let c = job.frames[job.frames.len - 1];
    assert(job.frames.len == 2 && c.input.is_object() && c.input == c.callee && c.receiver.is_object() && job.frames[0].input.is_undefined());
    if !save {
        assert(source::deliver_effect(job, source::text(heap, "D")));
        assert(source::run(job, -1) == tiny::Outcome.Complete && source::key_is(heap, job.result, "aDb1"));
        assert(source::retire(job)); return;
    }
    let stack = source::suspend_stack(job, -1); assert(stack >= 0 && job.frames.len == 0);
    let a = source::activation(heap, stack); assert(a.frame_count == 2);
    assert(source::retire(job, true));
    var k = snapshot::Control {}; let image = snapshot::capture(heap, [], k); assert(k.error == snapshot::Error.None);
    assert(qput(Waiting { image: copy(image), stack: stack as u32, steps: job.steps, kind: job.effect_kind, count: job.effect_count, frame: job.effect_frame, args: job.effect_args }));
}
thread_fn construct_resumed(saved: Waiting) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; var r = snapshot::Control {};
    assert(snapshot::restore(saved.image, code, heap, bindings, source::reserve_identity(), r));
    var job = source::Job { heap: heap, effects_enabled: true };
    assert(source::resume_stack(job, saved.stack) && job.frames.len == 2);
    let c = job.frames[1]; assert(c.input.is_object() && c.input == c.callee && c.receiver.is_object());
    job.steps = saved.steps; job.registered = true; heap.live_jobs += 1; job.effect_pending = true; job.effect_kind = saved.kind; job.effect_count = saved.count; job.effect_frame = saved.frame; for v, i in saved.args { job.effect_args[i] = v; }
    assert(source::deliver_effect(job, source::text(heap, "D")));
    assert(source::run(job, -1) == tiny::Outcome.Complete && source::key_is(heap, job.result, "aDb1"));
    assert(source::retire(job));
}
// A saved continuation whose stack holds native frames (light-cljs-813):
// suspended-stack.js waits twice inside a map callback inside a running
// generator. Each wait detaches the whole stack, five frames, two native,
// into one activation row; a fresh heap restores it, and the second wait
// reuses the restored row. Malformed frame rows are refused.
thread_fn stack_wait(bytes: u8[]) {
    defer { heap_program::finish(); }
    var code = source::Code {}; snapshot_test::append(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var job = source::Job { heap: heap, effects_enabled: true }; assert(source::start(job, source::closure(heap, 0, []), [source::builtin(heap, 8)]));
    assert(source::run(job, -1) == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8);
    var natives: i64 = 0; for f in job.frames { match f.native { None => {}, _ => { natives += 1; } } }
    assert(job.frames.len == 5 && natives == 2 && job.handlers.len > 0);
    let frames = source::plain_rows(job.frames); let locals = source::plain_rows(job.locals); let operands = source::plain_rows(job.operands); let handlers = job.handlers.len;
    let stack = source::suspend_stack(job, -1); assert(stack >= 0 && job.frames.len == 0 && job.locals.len == 0 && job.operands.len == 0 && job.handlers.len == 0);
    let a = source::activation(heap, stack); assert(a.frame_count == 5 && a.handler_count == handlers);
    // Attached and detached again, the stack is the same and reuses its row.
    let rows = heap.conts.len; let values = heap.values.len;
    assert(source::resume_stack(job, stack) && source::plain_rows(job.frames) == frames && source::plain_rows(job.locals) == locals && source::plain_rows(job.operands) == operands && job.handlers.len == handlers);
    assert(!source::resume_stack(job, stack) && heap.fault == source::Fault.InvalidState); heap.fault = source::Fault.None;
    // A native frame's state keeps its full width: map's length is an
    // array-like's (up to 2^53 - 1), not a row count.
    var at: i64 = -1; for f, i in job.frames { match f.native { ArrayCallbacks => { at = i; }, _ => {} } }
    assert(at >= 0); let length = job.frames[at].argument_count; job.frames[at].argument_count = 4294967297;
    assert(source::suspend_stack(job, stack) == stack && source::resume_stack(job, stack) && job.frames[at].argument_count == 4294967297);
    job.frames[at].argument_count = length;
    assert(source::suspend_stack(job, stack) == stack && heap.conts.len == rows && heap.values.len == values);
    assert(source::retire(job, true));
    var c = snapshot::Control {}; let image = snapshot::capture(heap, [], c); assert(c.error == snapshot::Error.None);
    // Refused: a native frame without its operation, a bytecode frame's pc
    // outside its code, bases falling up the stack, a handler past the run,
    // a Resume frame naming no generator.
    for mode in 5 {
        var bad = copy(image); var changed = false;
        for i in a.frame_count {
            let r = a.frames + i;
            match image.conts[r] {
                Frame x => {
                    var f = x.f;
                    if mode == 0 && f.function == source::no_row() && !changed { f.native = source::Native.None; changed = true; }
                    if mode == 1 && f.function != source::no_row() && !changed { f.pc = 4000000000; changed = true; }
                    if mode == 2 && i == a.frame_count - 1 { f.local_base = 0; changed = true; }
                    if mode == 4 { match f.native { Resume => { f.input = source::Value.Undefined; changed = true; }, _ => {} } }
                    bad.conts[r] = source::Cont.Frame { f: f };
                },
                _ => {},
            }
        }
        if mode == 3 { match image.conts[a.handlers] { Saved x => { var h = x.h; h.frame = a.frame_count; bad.conts[a.handlers] = source::Cont.Saved { h: h }; changed = true; }, _ => {} } }
        assert(changed);
        var check = snapshot::Control {}; assert(!snapshot::validate(bad, check) && check.error == snapshot::Error.Link);
        var dest = source::Code {}; var target_heap = source::Heap { code: dest }; var roots: snapshot::Binding[>..] = []; var refused = snapshot::Control {};
        assert(!snapshot::restore(bad, dest, target_heap, roots, source::reserve_identity(), refused) && refused.error == snapshot::Error.Link && target_heap.nodes.len == 0);
    }
    assert(qput(Waiting { image: copy(image), stack: stack as u32, steps: job.steps, kind: job.effect_kind, count: job.effect_count, frame: job.effect_frame, args: job.effect_args }));
}
// A stack whose rows pass the job's continuation or value limits stays on
// the job with every table unchanged; the session then delivers the fault
// at the wait (compiler_session.goose, execute), which ends the job.
thread_fn stack_limits(bytes: u8[]) {
    defer { heap_program::finish(); }
    var code = source::Code {}; snapshot_test::append(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var job = source::Job { heap: heap, effects_enabled: true }; assert(source::start(job, source::closure(heap, 0, []), [source::builtin(heap, 8)]));
    assert(source::run(job, -1) == tiny::Outcome.Yielded && job.effect_pending && job.frames.len == 5);
    let frames = source::plain_rows(job.frames); let locals = source::plain_rows(job.locals); let operands = source::plain_rows(job.operands); let handlers = job.handlers.len;
    let rows = heap.conts.len; let values = heap.values.len; let limits = heap.limits;
    for table in 2 {
        if table == 0 { heap.limits.conts = heap.conts.len - heap.image.conts; } else { heap.limits.values = heap.values.len - heap.image.values; }
        assert(source::suspend_stack(job, -1) == -1 && heap.fault == source::Fault.MemoryLimit);
        assert(heap.conts.len == rows && heap.values.len == values && source::plain_rows(job.frames) == frames && source::plain_rows(job.locals) == locals && source::plain_rows(job.operands) == operands && job.handlers.len == handlers);
        heap.fault = source::Fault.None; heap.limits = limits;
    }
    assert(source::deliver_effect(job, source::Value.Undefined, source::Fault.MemoryLimit));
    assert(source::run(job, -1) == tiny::Outcome.Failed && job.fault == source::Fault.MemoryLimit);
    assert(source::retire(job));
}
// Restore a stack_wait continuation in a fresh heap, answer it, and run to
// the next wait (saved again into the same row) or to the end.
thread_fn stack_resumed(saved: Waiting, last: bool) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; var r = snapshot::Control {};
    assert(snapshot::restore(saved.image, code, heap, bindings, source::reserve_identity(), r));
    var job = source::Job { heap: heap, effects_enabled: true };
    assert(source::resume_stack(job, saved.stack) && job.frames.len == 5);
    job.steps = saved.steps; job.registered = true; heap.live_jobs += 1; job.effect_pending = true; job.effect_kind = saved.kind; job.effect_count = saved.count; job.effect_frame = saved.frame; for v, i in saved.args { job.effect_args[i] = v; }
    assert(source::deliver_effect(job, source::text(heap, "D")));
    if last { assert(source::run(job, -1) == tiny::Outcome.Complete && source::key_is(heap, job.result, "a1D2Dybf")); assert(source::retire(job)); return; }
    assert(source::run(job, -1) == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8 && job.frames.len == 5);
    let rows = heap.conts.len; let values = heap.values.len;
    assert(source::suspend_stack(job, saved.stack) == saved.stack && heap.conts.len == rows && heap.values.len == values);
    assert(source::retire(job, true));
    var c = snapshot::Control {}; let image = snapshot::capture(heap, [], c); assert(c.error == snapshot::Error.None);
    assert(qput(Waiting { image: copy(image), stack: saved.stack, steps: job.steps, kind: job.effect_kind, count: job.effect_count, frame: job.effect_frame, args: job.effect_args }));
}
thread_fn suspension_parent(bytes: u8[]) {
    defer { heap_program::finish(); }
    var code = source::Code {}; snapshot_test::append(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var bindings: snapshot::Binding[>..] = [snapshot::Binding { id: 1, generation: 1, value: source::closure(heap, 0, []) }];
    var c = snapshot::Control {}; let base = snapshot::capture(heap, bindings, c); assert(c.error == snapshot::Error.None && base.conts.len == 0);
    assert(qput(base));
}
thread_fn suspension_worker(base: snapshot::Snapshot) {
    defer { heap_program::finish(); }
    let identity = source::reserve_identity(); let rows: i64[] = [];
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; var r = snapshot::Control {};
    assert(snapshot::restore(base, code, heap, bindings, identity, r) && snapshot::truncate(base, rows, identity, code, heap, bindings));
    // The first job leaves its generator suspended at its yield.
    var first = source::Job { heap: heap };
    assert(source::start(first, bindings[0].value, []) && source::run(first, -1) == tiny::Outcome.Complete);
    let g = first.result; assert(g.is_object() && source::node(heap, g.id()).kind == source::generator_kind() && source::node(heap, g.id()).length == 1 && heap.conts.len > base.conts.len);
    assert(source::retire(first));
    // The reset drops its rows, and the next job cannot name it.
    assert(snapshot::truncate(base, rows, identity, code, heap, bindings) && snapshot::pristine(base, identity, code, heap, bindings));
    assert(heap.conts.len == base.conts.len && g.id() >= heap.nodes.len);
    var second = source::Job { heap: heap };
    assert(!source::start(second, bindings[0].value, [g]) && second.fault == source::Fault.StaleID);
    var third = source::Job { heap: heap };
    assert(source::start(third, bindings[0].value, []) && source::run(third, -1) == tiny::Outcome.Complete && third.result.is_object() && third.result.id() == g.id());
    assert(source::retire(third) && snapshot::truncate(base, rows, identity, code, heap, bindings) && snapshot::pristine(base, identity, code, heap, bindings));
}

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
        assert(snapshot::ends(empty) == [0; 21] && target.identity == 0 && !dest.valid && target.fault == source::Fault.None);
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
    assert(source::node(heap, array.id()).length == 1 && source::get(heap, array, source::key(heap, source::number(2))).is_undefined());
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
        for j in 1..3 { assert(source::get(restored, rarray, source::key(restored, source::number(j))).is_undefined()); }
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
        for op in code.ops[f.start..(f.start + f.count)] { args = args || op.code == opcode::NewArguments; closure = closure || op.code == opcode::Function; }
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
    graph.nodes.push(bridge::Form { tag: form_tag::Number, number: 7, source: 0, span_start: 0, span_end: 6 }); graph.roots.push(0);
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
    // Its name, "f", is a run of its own code units (light-cljs-806).
    extra.units.push(102);
    extra.functions.push(source::Function { start: 0, count: 3, constants: 0, children: 0, params: 0, locals: 0, captures: 2, strict: true, name_start: 0, name_len: 1 });
    for op, i in [opcode::GetCapture, opcode::CellGet, opcode::Return] { extra.ops.push(source::Op { code: op, arg: if i == 0 { 1 } else { 0 }, offset: [0, 3, 4][i], start: 0, end: 1, depth: [0, 1, 1][i] }); }
    // A second unreachable function must still be validated on publication.
    extra.functions.push(source::Function { start: 3, count: 2, constants: 0, children: 0, params: 0, locals: 0, captures: 0, strict: true });
    for op, i in [opcode::Undefined, opcode::Return] { extra.ops.push(source::Op { code: op, arg: 0, offset: i, start: 0, end: 1, depth: i }); } extra.valid = true;
}
fn callable_publication() {
    heap_program::join(thread_spawn(callable_base)); let saved = qget<Saved>(); let base .= saved.image;
    heap_program::join(thread_spawn(callable_producer, copy(base), false)); let p = qget<Published>(); let wire .= p.wire;
    for cut in [0, 1, wire.len / 2, wire.len - 1] { heap_program::join(thread_spawn(callable_cut, copy(base), str(wire[..cut]))); }
    let records, parsed = from_bytes<callable::Publication[>..]>(wire); assert(parsed && records.len == 1);
    for original in records {
        for mode in -1..17 { heap_program::join(thread_spawn(callable_mode, copy(base), copy(original), copy(p), mode)); }
        for limit in 2 { heap_program::join(thread_spawn(callable_limit, copy(base), copy(original), wire.len, limit)); }
    }
    heap_program::join(thread_spawn(callable_producer, copy(base), true)); let q = qget<Published>();
    heap_program::join(thread_spawn(callable_identity, copy(base), copy(q)));
    print("callable code/capture relocation, runtime reconnection, occurrence aliases and transactional malformed/limited publication passed");
}
thread_fn callable_base() {
    defer { heap_program::finish(); }
    var base_code = source::Code {}; snapshot_test::append(base_code, embed_file("../corpus/serial/calls.gsp"));
    var base_heap = source::Heap { code: base_code }; assert(source::initialize(base_heap)); let runtime = source::boot_runtime(base_heap);
    var capture = snapshot::Control {}; let base = snapshot::capture(base_heap, [snapshot::Binding { id: 0, generation: 1, value: runtime }], capture); assert(capture.error == snapshot::Error.None);
    assert(qput(Saved { image: copy(base) }));
}
// `identity`: the captured forms' source is a compiler identity with no text
// (light-cljs-817), as a job's call sends it.
thread_fn callable_producer(base: snapshot::Snapshot, identity: bool) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var producer = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; var restore = snapshot::Control {};
    assert(snapshot::restore(base, code, producer, bindings, source::reserve_identity(), restore));
    var extra = source::Code {}; extra_code(extra);
    let entry = source::append_code(code, extra);
    let rt_cell = source::allocate(producer, source::Node { kind: 3, initialized: true, immutable: true, value: bindings[0].value });
    if identity { producer.source_ranges.push(source::SourceRange { start: 0, count: 3, identity: 77 }); } else { producer.source_bytes.append("a b"); producer.source_ranges.push(source::SourceRange { start: 0, count: 3 }); }
    let one = source::allocate(producer, source::Node { kind: 11, value: source::number(7), origin: 1, source: 0, span_start: 0, span_end: 1 });
    let two = source::allocate(producer, source::Node { kind: 11, value: source::number(7), origin: 2, source: 0, span_start: 2, span_end: 3 });
    let marked = source::form_op(producer, form_op::Keyword, [source::nil(), source::text(producer, "marked")]);
    let metadata = source::form_collection(producer, 7, [marked, source::boolean(true)]); producer.nodes[one.id()].metadata = metadata; producer.nodes[two.id()].metadata = metadata;
    let forms = source::form_collection(producer, 6, [one, two, one]);
    let cell = source::allocate(producer, source::Node { kind: 3, initialized: true, immutable: true, value: forms });
    let function = source::closure(producer, entry, [rt_cell, cell]);
    producer.nodes[function.id()].immutable = true;
    var encoding = snapshot::Control {}; var wire: u8[>..] = []; assert(callable::encode(producer, base, function, wire, encoding, source::reserve_identity()));
    assert(qput(Published { wire: copy(wire), entry: entry, runtime: bindings[0].value, rt_cell: rt_cell, two: two, forms: forms, cell: cell, function: function }));
}
// A helper publication whose captured forms name an identity source installs
// into a consumer that already holds source text: the installed forms name a
// row with the same identity and length and no text, at their own spans, the
// consumer's text row is unchanged, and the heap still captures as a valid
// snapshot.
thread_fn callable_identity(base: snapshot::Snapshot, p: Published) {
    defer { heap_program::finish(); }
    var extra = source::Code {}; extra_code(extra);
    var target_code = source::Code {}; var target = source::Heap { code: target_code }; var roots: snapshot::Binding[>..] = []; var sc = snapshot::Control {};
    assert(snapshot::restore(base, target_code, target, roots, source::reserve_identity(), sc)); source::append_code(target_code, extra);
    let own = source::add_source_bytes(target, "(own)"); let own_row = source::add_source_range(target, source::SourceRange { start: own as u32, count: 5 });
    var check = snapshot::Control {}; let imported = callable::decode_install(p.wire, base.header, target_code, target, check);
    assert(check.error == snapshot::Error.None && imported.is_object());
    let result = snapshot_test::invoke(target, imported, []); let x = source::form_nth(target, result, 0); let y = source::form_nth(target, result, 1);
    let nx = target.nodes[x.id()]; let ny = target.nodes[y.id()];
    assert(nx.source == ny.source && nx.source != own_row && nx.span_start == 0 && nx.span_end == 1 && ny.span_start == 2 && ny.span_end == 3);
    let r = target.source_ranges[nx.source]; assert(r.identity == 77 && r.count == 3 && r.start == 0 && !source::has_text(r));
    let mine = target.source_ranges[own_row]; assert(source::has_text(mine) && target.source_bytes[mine.start..(mine.start + mine.count)] == "(own)" && target.source_bytes.len == 5);
    var valid = snapshot::Control {}; let finished = snapshot::capture(target, roots, valid); assert(valid.error == snapshot::Error.None);
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
            15 => { bad.image.nodes[p.runtime.id()].length += 1; },
            16 => { bad.image.functions[entry].name_start = 0; }, _ => {}, // 16: a name in the base prefix
        }
        let before = snapshot::capture(target, roots, sc); assert(sc.error == snapshot::Error.None);
        var encoded: u8[>..] = []; to_bytes([bad], encoded);
        var check = snapshot::Control {}; let imported = callable::decode_install(encoded, base.header, target_code, target, check);
        if mode >= 0 {
            assert(check.error != snapshot::Error.None);
            var after = snapshot::Control {}; var unchanged = snapshot::capture(target, roots, after); unchanged.header.checkpoint = before.header.checkpoint; assert(after.error == snapshot::Error.None);
            var left: u8[>..] = []; var right: u8[>..] = []; to_bytes([before], left); to_bytes([unchanged], right); assert(left == right);
        } else {
            assert(check.error == snapshot::Error.None && imported.is_object());
            assert(target.nodes[imported.id()].function == entry + extra.functions.len && target.nodes[imported.id()].immutable);
            // Its name is relocated behind the consumer's own code units.
            let named = target_code.functions[entry + extra.functions.len]; assert(named.name_len == 1 && target_code.units[named.name_start] == 102 && named.name_start >= extra.units.len);
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
