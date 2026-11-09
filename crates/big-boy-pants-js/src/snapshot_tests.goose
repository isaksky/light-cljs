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
    source::allocate(parent, source::Node { kind: node_kind::Object }); code.source_bytes.append(" ");
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
    for mode in 47 { heap_program::join(thread_spawn(malformed_target, copy(original), mode)); }
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
// The fixture's key list row (node_kind::KeyList).
fn list_row(s: const snapshot::Snapshot&) -> i64 { for n, i in s.nodes { if n.kind == node_kind::KeyList { assert(n.key_count > 0); return i; } } abort("no key list"); return -1; }
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
            14 => { bad.ops[0].depth = 1; }, 15 => { bad.nodes[0].kind = node_kind::Error; }, 16 => { bad.header.ends[13] += 1; },
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
            // A primitive wrapper (light-cljs-832) holds a number, string or
            // boolean and no element run.
            45 => { let w = source::wrapper_prototype_row(1); assert(bad.nodes[w].kind == node_kind::Wrapper); bad.nodes[w].value = source::Value.Null; },
            46 => { let w = source::wrapper_prototype_row(0); assert(bad.nodes[w].kind == node_kind::Wrapper); bad.nodes[w].count = 1; },
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
        stale = source::allocate(old, source::Node { kind: node_kind::Binding, initialized: true, immutable: true, value: source::number(1) });
        stale_text = source::text(old, "old"); old_binding.value = stale;
        for i in 64 { source::allocate(old, source::Node { kind: node_kind::Object }); }
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
    let replacement = source::allocate(fresh, source::Node { kind: node_kind::Object }); let new_text = source::text(fresh, "new");
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
    let service = source::allocate(parent, source::Node { kind: node_kind::Object }); assert(source::install_field(parent, parent.global, "service", service));
    // The service object has an element in the image (light-cljs-804).
    assert(source::install_field(parent, service, "1", source::number(1)) && source::node(parent, service.id()).count == 4);
    var c = snapshot::Control {}; let base = snapshot::capture(parent, bindings, c); assert(c.error == snapshot::Error.None);
    // The derived image: base, a definition's code and rows above it.
    var dc = source::Code {}; var derived = source::Heap { code: dc }; var db: snapshot::Binding[>..] = []; var r = snapshot::Control {}; assert(snapshot::restore(base, dc, derived, db, source::reserve_identity(), r));
    var extra = source::Code {}; extra_code(extra); assert(source::append_code(dc, extra) >= 0);
    let o = source::allocate(derived, source::Node { kind: node_kind::Object }); assert(source::install_field(derived, o, "derived", source::text(derived, "row above the image")));
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
    let point = source::allocate(heap, source::Node { kind: node_kind::Object }); assert(source::install_field(heap, heap.global, "point", point));
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
        let p = source::allocate(heap, source::Node { kind: node_kind::Object });
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
// Primitive wrappers a job made (light-cljs-832) survive capture, the byte
// codec and restoration in another program: their primitives (-0 by bits, a
// wide string's units), prototypes, own properties in shape and dictionary
// mode, a String object's exotic `length` and indices first in its keys,
// and identity across two bindings.
fn wrappers_roundtrip() {
    heap_program::join(thread_spawn(wrappers_parent)); let saved = qget<Saved>();
    heap_program::join(thread_spawn(wrappers_restored, copy(saved.image)));
}
thread_fn wrappers_parent() {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; snapshot_test::prepare(code, heap, bindings);
    let n = source::allocate(heap, source::Node { kind: node_kind::Wrapper, value: source::number(-0.0), proto: source::object(source::wrapper_prototype_row(0)) });
    let b = source::allocate(heap, source::Node { kind: node_kind::Wrapper, value: source::boolean(true), proto: source::object(source::wrapper_prototype_row(2)) });
    let s = source::allocate(heap, source::Node { kind: node_kind::Wrapper, value: source::string(heap, [0xD83D, 0xDE00, 'z']), proto: source::object(source::wrapper_prototype_row(1)) });
    assert(source::put(heap, n, source::text(heap, "x"), source::number(1)) && source::put(heap, s, source::text(heap, "7"), n));
    assert(source::put(heap, b, source::text(heap, "a"), source::number(2)) && source::put(heap, b, source::text(heap, "b"), s) && source::delete_property(heap, b, source::key_named(heap, "a")) && source::node(heap, b.id()).initialized);
    for v, i in [n, b, s, s] { bindings.push(snapshot::Binding { id: 20 + i, generation: 1, value: v }); }
    var c = snapshot::Control {}; let saved = snapshot::capture(heap, bindings, c); assert(c.error == snapshot::Error.None);
    var encoded: u8[>..] = []; var enc = snapshot::Control {}; assert(snapshot::encode(saved, encoded, enc));
    var dec = snapshot::Control {}; let decoded = snapshot::decode(encoded, dec); assert(dec.error == snapshot::Error.None && decoded.len == 1);
    for image in decoded { assert(qput(Saved { image: copy(image) })); }
}
thread_fn wrappers_restored(saved: snapshot::Snapshot) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; var r = snapshot::Control {};
    assert(snapshot::restore(saved, code, heap, roots, source::reserve_identity(), r));
    let base = roots.len - 4; let n = roots[base].value; let b = roots[base + 1].value; let s = roots[base + 2].value;
    assert(roots[base + 3].value == s && source::get(heap, s, source::text(heap, "7")) == n && source::get(heap, b, source::text(heap, "b")) == s);
    for v, t in [n, s, b] { assert(source::node(heap, v.id()).kind == node_kind::Wrapper && source::prototype(heap, v.id()) == source::object(source::wrapper_prototype_row([0, 1, 2][t]))); }
    assert(source::same_bits(source::node(heap, n.id()).value, source::number(-0.0)) && source::node(heap, b.id()).value.flag() && source::get(heap, n, source::text(heap, "x")).num() == 1);
    assert(source::node(heap, b.id()).initialized && !source::own_entry(heap, b, source::key_named(heap, "a")).found);
    let length = source::own_entry(heap, s, source::key_named(heap, "length")); let high = source::own_entry(heap, s, source::key_named(heap, "0"));
    assert(length.found && length.attrs == 0 && length.value.num() == 3 && high.found && high.attrs == source::attr_enumerable() && source::unit(heap, high.value, 0) == 0xD83D);
    var keys: source::Value[>..] = []; assert(source::own_keys(heap, s, keys) && keys.len == 5);
    for name, i in ["0", "1", "2", "7", "length"] { assert(source::key_is(heap, keys[i], name)); }
    assert(heap.fault == source::Fault.None);
    print("snapshot primitive wrappers: payloads, prototypes, shape and dictionary properties, String exotic keys and identity survive restoration passed");
}
// Bound functions a job made (light-cljs-852) survive capture, the byte
// codec and restoration in another program: target, bound `this`, the
// bound arguments' run, own `length` and `name`, and a bound function of a
// bound function. A bound function whose target is a later row, is not
// callable, or that has code is refused.
fn bound_roundtrip() {
    heap_program::join(thread_spawn(bound_parent)); let saved = qget<Saved>();
    heap_program::join(thread_spawn(bound_restored, copy(saved.image)));
    for mode in 3 { heap_program::join(thread_spawn(bound_malformed, copy(saved.image), mode)); }
}
fn make_bound(heap: source::Heap&, target: source::Value, this_value: source::Value, args: const source::Value[:], name: const u8[:], length: f64) -> source::Value {
    let at = source::add_values(heap, args); assert(at >= 0);
    let f = source::allocate(heap, source::Node { kind: node_kind::BoundFunction, start: at as u32, count: args.len as u32, value: target, namespace_value: this_value, proto: source::node(heap, target.id()).proto });
    assert(source::add_own(heap, f.id(), source::key_named(heap, "length"), source::number(length), source::attr_configurable()));
    assert(source::add_own(heap, f.id(), source::key_named(heap, "name"), source::text(heap, name), source::attr_configurable()));
    return f;
}
thread_fn bound_parent() {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; snapshot_test::prepare(code, heap, bindings);
    let closure = bindings[4].value; let object = bindings[6].value;
    let two = source::text(heap, "two"); let first: source::Value[2] = [source::number(1), two]; let second: source::Value[1] = [object]; let none: source::Value[0] = [];
    let zero: f64 = 0; let is_length: f64 = 2; let is = source::object(source::realm_function_row(source::Builtin.ObjectIs));
    let a = make_bound(heap, closure, object, first[..], "bound ", zero);
    let b = make_bound(heap, a, source::number(9), second[..], "bound bound ", zero);
    let c = make_bound(heap, is, source::Value.Undefined, none[..], "bound is", is_length);
    for v, i in [a, b, c, b] { bindings.push(snapshot::Binding { id: 20 + i, generation: 1, value: v }); }
    var k = snapshot::Control {}; let saved = snapshot::capture(heap, bindings, k); assert(k.error == snapshot::Error.None);
    var encoded: u8[>..] = []; var enc = snapshot::Control {}; assert(snapshot::encode(saved, encoded, enc));
    var dec = snapshot::Control {}; let decoded = snapshot::decode(encoded, dec); assert(dec.error == snapshot::Error.None && decoded.len == 1);
    for image in decoded { assert(qput(Saved { image: copy(image) })); }
}
thread_fn bound_restored(saved: snapshot::Snapshot) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; var r = snapshot::Control {};
    assert(snapshot::restore(saved, code, heap, roots, source::reserve_identity(), r));
    let base = roots.len - 4; let a = roots[base].value; let b = roots[base + 1].value; let c = roots[base + 2].value;
    assert(roots[base + 3].value == b && source::node(heap, b.id()).value == a && source::node(heap, b.id()).namespace_value.num() == 9);
    let n = source::node(heap, a.id());
    assert(n.kind == node_kind::BoundFunction && source::node(heap, n.value.id()).kind == node_kind::Closure && source::node(heap, n.namespace_value.id()).kind == node_kind::Object && n.count == 2);
    assert(source::slot(heap, n.start).num() == 1 && source::key_is(heap, source::slot(heap, n.start + 1), "two"));
    assert(source::slot(heap, source::node(heap, b.id()).start) == n.namespace_value && source::node(heap, c.id()).count == 0);
    let length = source::own_entry(heap, c, source::key_named(heap, "length")); let name = source::own_entry(heap, b, source::key_named(heap, "name"));
    assert(length.found && length.attrs == source::attr_configurable() && length.value.num() == 2 && name.found && source::key_is(heap, name.value, "bound bound "));
    assert(source::prototype(heap, a.id()) == source::object(source::function_prototype_row()) && heap.fault == source::Fault.None);
    print("snapshot bound functions: targets, bound this and arguments, own length and name and identity survive restoration passed");
}
thread_fn bound_malformed(saved: snapshot::Snapshot, mode: i64) {
    defer { heap_program::finish(); }
    {
        var bad = copy(saved);
        var at: i64 = -1; for n, i in bad.nodes { if n.kind == node_kind::BoundFunction && at < 0 { at = i; } } assert(at >= 0);
        match mode {
            0 => { bad.nodes[at].value = source::object(at + 1); },
            1 => { bad.nodes[at].value = bad.nodes[at].namespace_value; },
            _ => { bad.nodes[at].function = 1; },
        }
        var dest = source::Code {}; var target = source::Heap { code: dest }; var roots: snapshot::Binding[>..] = []; var check = snapshot::Control {};
        assert(!snapshot::restore(bad, dest, target, roots, source::reserve_identity(), check) && check.error == snapshot::Error.Link && target.nodes.len == 0);
    }
}
// [[Extensible]] and integrity levels (light-cljs-882) survive capture, the
// byte codec and restoration in another program: a non-extensible object,
// a frozen object (its restricted shape), a sealed and a frozen dense array
// (their flags), a frozen dictionary array; the restored objects keep
// refusing new properties. An image with [[Extensible]] cleared on a node
// without own properties, a Sealed array that is extensible or in
// dictionary mode, or a Frozen array that is not Sealed is refused.
fn integrity_roundtrip() {
    heap_program::join(thread_spawn(integrity_parent)); let saved = qget<Saved>();
    heap_program::join(thread_spawn(integrity_restored, copy(saved.image)));
    for mode in 4 { heap_program::join(thread_spawn(integrity_malformed, copy(saved.image), mode)); }
    heap_program::join(thread_spawn(integrity_boundaries));
    heap_program::join(thread_spawn(private_boundaries));
    heap_program::join(thread_spawn(prototype_boundaries));
}
// [[SetPrototypeOf]]'s contracts (light-cljs-883): a row the job did not
// allocate faults NativeBoundary unchanged unless it has the prototype
// asked already; %Object.prototype% refuses another prototype (a TypeError,
// not a boundary); a prototype set back to the kind's realm one is stored
// as Empty; a proxy and a form fail UnsupportedOperand by name; and an
// image whose `__proto__` pair does not hold the realm's getter and setter
// is refused.
thread_fn prototype_boundaries() {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; snapshot_test::prepare(code, heap, bindings);
    var job = source::Job { heap: heap };
    let o = source::allocate(heap, source::Node { kind: node_kind::Object }); let p = source::allocate(heap, source::Node { kind: node_kind::Object });
    let before = source::plain_rows(heap.nodes[o.id()..(o.id() + 1)]);
    let setter = source::object(source::realm_function_row(source::Builtin.ObjectPrototypeProtoSetter));
    for limit in 3 {
        job.protected_nodes = 0; job.owned_nodes = 0; job.foreign.clear();
        if limit == 0 { job.protected_nodes = heap.nodes.len; } else if limit == 1 { job.owned_nodes = heap.nodes.len; } else { job.foreign.push(o.id()); job.foreign.push(o.id() + 1); }
        heap.fault = source::Fault.None;
        source::prototype_builtin(job, source::Builtin.ObjectSetPrototypeOf, source::Value.Undefined, [o, p]);
        assert(heap.fault == source::Fault.NativeBoundary && source::plain_rows(heap.nodes[o.id()..(o.id() + 1)]) == before);
        heap.fault = source::Fault.None;
        source::prototype_builtin(job, source::Builtin.ObjectPrototypeProtoSetter, o, [p]);
        assert(heap.fault == source::Fault.NativeBoundary && source::plain_rows(heap.nodes[o.id()..(o.id() + 1)]) == before);
        // The prototype it has already: nothing would change, so nothing faults.
        heap.fault = source::Fault.None;
        let proto = source::object(source::object_prototype_row());
        assert(source::prototype_builtin(job, source::Builtin.ObjectSetPrototypeOf, source::Value.Undefined, [o, proto]) == o && source::prototype_builtin(job, source::Builtin.ReflectSetPrototypeOf, source::Value.Undefined, [o, proto]) == source::boolean(true) && heap.fault == source::Fault.None);
    }
    job.protected_nodes = 0; job.owned_nodes = 0; job.foreign.clear();
    // A realm row keeps its prototype even where the job may write it.
    let ap = source::array_prototype_row(); let realm_before = source::plain_rows(heap.nodes[ap..(ap + 1)]);
    heap.fault = source::Fault.None;
    source::prototype_builtin(job, source::Builtin.ReflectSetPrototypeOf, source::Value.Undefined, [source::object(ap), p]);
    assert(heap.fault == source::Fault.NativeBoundary && source::plain_rows(heap.nodes[ap..(ap + 1)]) == realm_before);
    // %Object.prototype% is immutable even where its row may be written.
    heap.fault = source::Fault.None;
    assert(source::prototype_builtin(job, source::Builtin.ReflectSetPrototypeOf, source::Value.Undefined, [source::object(source::object_prototype_row()), p]) == source::boolean(false) && heap.fault == source::Fault.None);
    source::prototype_builtin(job, source::Builtin.ObjectSetPrototypeOf, source::Value.Undefined, [source::object(source::object_prototype_row()), p]);
    assert(heap.fault == source::Fault.TypeError && source::node(heap, source::object_prototype_row()).proto.is_null());
    // Another prototype is a link; the realm's again is Empty; a cycle is refused.
    heap.fault = source::Fault.None;
    assert(source::prototype_builtin(job, source::Builtin.ReflectSetPrototypeOf, source::Value.Undefined, [o, p]) == source::boolean(true) && source::node(heap, o.id()).proto == p);
    assert(source::prototype_builtin(job, source::Builtin.ReflectSetPrototypeOf, source::Value.Undefined, [p, o]) == source::boolean(false) && heap.fault == source::Fault.None);
    assert(source::prototype_builtin(job, source::Builtin.ObjectSetPrototypeOf, source::Value.Undefined, [o, source::object(source::object_prototype_row())]) == o && source::node(heap, o.id()).proto.is_hole());
    let a = source::allocate(heap, source::Node { kind: node_kind::Array });
    assert(source::prototype_builtin(job, source::Builtin.ObjectSetPrototypeOf, source::Value.Undefined, [a, p]) == a && source::node(heap, a.id()).kind == node_kind::Array && !source::dense(job, a));
    assert(source::prototype_builtin(job, source::Builtin.ObjectSetPrototypeOf, source::Value.Undefined, [a, source::object(source::array_prototype_row())]) == a && source::node(heap, a.id()).proto.is_hole() && source::dense(job, a));
    // A proxy and a form fail by name, uncatchable.
    let x = source::allocate(heap, source::Node { kind: node_kind::Proxy, value: o, namespace_value: p, proto: source::Value.Null });
    heap.fault = source::Fault.None; heap.proxy_method = -1;
    source::prototype_builtin(job, source::Builtin.ObjectSetPrototypeOf, source::Value.Undefined, [x, p]);
    assert(heap.fault == source::Fault.UnsupportedOperand && !source::catchable(heap.fault) && heap.proxy_method == proxy_method::SetPrototypeOf);
    heap.fault = source::Fault.None; heap.proxy_method = -1;
    source::prototype_builtin(job, source::Builtin.ReflectGetPrototypeOf, source::Value.Undefined, [x]);
    assert(heap.fault == source::Fault.UnsupportedOperand && heap.proxy_method == proxy_method::GetPrototypeOf);
    heap.fault = source::Fault.None; let form = source::allocate(heap, source::Node { kind: node_kind::Vector });
    for b in [source::Builtin.ObjectSetPrototypeOf, source::Builtin.ReflectGetPrototypeOf] {
        heap.fault = source::Fault.None; job.detail.clear();
        source::prototype_builtin(job, b, source::Value.Undefined, [form, p]);
        assert(heap.fault == source::Fault.UnsupportedOperand && !source::catchable(heap.fault) && job.detail.len > 0 && source::node(heap, form.id()).proto.is_hole());
    }
    // isPrototypeOf fails by name on a form argument, on a chain that
    // reaches a form, and on a proxy on the chain, even for a primitive this.
    heap.fault = source::Fault.None; let heir = source::allocate(heap, source::Node { kind: node_kind::Object, proto: form });
    let viaproxy = source::allocate(heap, source::Node { kind: node_kind::Object, proto: x });
    for c in [[source::object(source::object_prototype_row()), form], [source::object(source::object_prototype_row()), heir], [source::number(1), viaproxy]] {
        heap.fault = source::Fault.None; job.detail.clear(); heap.proxy_method = -1;
        source::prototype_builtin(job, source::Builtin.ObjectPrototypeIsPrototypeOf, c[0], [c[1]]);
        assert(heap.fault == source::Fault.UnsupportedOperand && !source::catchable(heap.fault) && (job.detail.len > 0 || heap.proxy_method == proxy_method::GetPrototypeOf));
    }
    // The realm's `__proto__` pair.
    heap.fault = source::Fault.None;
    assert(source::realm_valid(heap.nodes, heap.accessors, heap.global, heap.call_method) && heap.accessors[source::proto_pair()].set == setter);
    heap.accessors[source::proto_pair()].set = source::Value.Undefined;
    assert(!source::realm_valid(heap.nodes, heap.accessors, heap.global, heap.call_method));
    print("prototype boundaries: protected, owned and foreign rows fault NativeBoundary unchanged unless the prototype is the same; Object.prototype immutable; Empty kept; proxies and forms fail by name; the __proto__ pair validated passed");
}
// The fault contracts (decision 3, light-cljs-891, forms): a row the job
// did not allocate (below protected_nodes or owned_nodes, or in a foreign
// range) faults NativeBoundary before anything changes, unless it is at
// the level asked already; a proxy's [[PreventExtensions]] and
// [[IsExtensible]] and a form fail UnsupportedOperand by name, which
// JavaScript cannot catch.
thread_fn integrity_boundaries() {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; snapshot_test::prepare(code, heap, bindings);
    var job = source::Job { heap: heap };
    let o = source::allocate(heap, source::Node { kind: node_kind::Object }); let frozen = source::allocate(heap, source::Node { kind: node_kind::Object });
    assert(source::put(heap, o, source::text(heap, "x"), source::number(1)) && source::put(heap, frozen, source::text(heap, "y"), source::number(2)));
    assert(source::integrity_builtin(job, source::Builtin.ObjectFreeze, frozen) == frozen && heap.fault == source::Fault.None);
    let before = source::plain_rows(heap.nodes[o.id()..(o.id() + 1)]); let shape = heap.shapes.len;
    for limit in 3 {
        job.protected_nodes = 0; job.owned_nodes = 0; job.foreign.clear();
        if limit == 0 { job.protected_nodes = heap.nodes.len; } else if limit == 1 { job.owned_nodes = heap.nodes.len; } else { job.foreign.push(o.id()); job.foreign.push(frozen.id() + 1); }
        for b in [source::Builtin.ObjectPreventExtensions, source::Builtin.ObjectSeal, source::Builtin.ObjectFreeze, source::Builtin.ReflectPreventExtensions] {
            heap.fault = source::Fault.None;
            source::integrity_builtin(job, b, o);
            assert(heap.fault == source::Fault.NativeBoundary && source::plain_rows(heap.nodes[o.id()..(o.id() + 1)]) == before && heap.shapes.len == shape);
        }
        // Already at the level asked: nothing would change, so nothing faults.
        heap.fault = source::Fault.None;
        assert(source::integrity_builtin(job, source::Builtin.ObjectFreeze, frozen) == frozen && source::integrity_builtin(job, source::Builtin.ObjectSeal, frozen) == frozen && source::integrity_builtin(job, source::Builtin.ObjectPreventExtensions, frozen) == frozen);
        assert(source::integrity_builtin(job, source::Builtin.ObjectIsExtensible, o) == source::boolean(true) && heap.fault == source::Fault.None);
        // [[DefineOwnProperty]] of such a row (light-cljs-897: SetIntegrityLevel
        // through a proxy defines on its target) accepts a definition that
        // changes nothing outside the compiler's protected rows, which it
        // does not read; any other definition faults NativeBoundary with
        // nothing changed.
        let kx = source::key_named(heap, "x"); let ky = source::key_named(heap, "y");
        let sealing = source::PropertyDescriptor { has_configurable: true }; let freezing = source::PropertyDescriptor { has_configurable: true, has_writable: true };
        let same = source::PropertyDescriptor { has_value: true, value: source::number(2) }; let open = source::PropertyDescriptor { has_configurable: true, configurable: true, has_enumerable: true, enumerable: true, has_writable: true, writable: true };
        let fixed = source::plain_rows(heap.nodes[frozen.id()..(frozen.id() + 1)]);
        if limit > 0 {
            heap.fault = source::Fault.None;
            assert(source::define_descriptor(job, frozen, ky, sealing) && source::define_descriptor(job, frozen, ky, freezing) && source::define_descriptor(job, frozen, ky, same) && source::define_descriptor(job, o, kx, open) && heap.fault == source::Fault.None);
        }
        for d, i in [source::PropertyDescriptor { has_value: true, value: source::number(3) }, open, sealing, sealing] {
            heap.fault = source::Fault.None;
            let target = if i < 2 { frozen } else { o };
            let k = if i < 2 { ky } else if i == 2 { kx } else { source::key_named(heap, "z") };
            assert(!source::define_descriptor(job, target, k, d) && heap.fault == source::Fault.NativeBoundary);
        }
        assert(source::plain_rows(heap.nodes[o.id()..(o.id() + 1)]) == before && source::plain_rows(heap.nodes[frozen.id()..(frozen.id() + 1)]) == fixed && heap.shapes.len == shape);
        heap.fault = source::Fault.None;
    }
    job.protected_nodes = 0; job.owned_nodes = 0; job.foreign.clear();
    let p = source::allocate(heap, source::Node { kind: node_kind::Proxy, value: o, namespace_value: frozen, proto: source::Value.Null });
    for b in [source::Builtin.ObjectIsExtensible, source::Builtin.ObjectPreventExtensions, source::Builtin.ObjectFreeze, source::Builtin.ObjectIsFrozen, source::Builtin.ReflectIsExtensible] {
        heap.fault = source::Fault.None; heap.proxy_method = -1;
        source::integrity_builtin(job, b, p);
        assert(heap.fault == source::Fault.UnsupportedOperand && !source::catchable(heap.fault) && (heap.proxy_method == proxy_method::IsExtensible || heap.proxy_method == proxy_method::PreventExtensions));
    }
    heap.fault = source::Fault.None; assert(!source::node(heap, o.id()).non_extensible);
    let form = source::allocate(heap, source::Node { kind: node_kind::Vector });
    for b in [source::Builtin.ObjectIsExtensible, source::Builtin.ObjectFreeze, source::Builtin.ObjectIsSealed] {
        heap.fault = source::Fault.None; job.detail.clear();
        source::integrity_builtin(job, b, form);
        assert(heap.fault == source::Fault.UnsupportedOperand && !source::node(heap, form.id()).non_extensible && job.detail.len > 0);
    }
    print("integrity boundaries: protected, owned and foreign rows fault NativeBoundary unchanged unless already at the level; proxies and forms fail by name passed");
}
// Private elements (light-cljs-889) obey the write boundary: PutPrivate
// of a field and every DefinePrivate operation on a row the job did not
// allocate (below protected_nodes or owned_nodes, or in a foreign range)
// fault NativeBoundary before anything changes, the receiver's row, its
// private elements and every table length included; reads and `#x in`
// still answer.
thread_fn private_boundaries() {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; snapshot_test::prepare(code, heap, bindings);
    var job = source::Job { heap: heap };
    let o = source::allocate(heap, source::Node { kind: node_kind::Object }); let bare = source::allocate(heap, source::Node { kind: node_kind::Object });
    let x = source::new_symbol(heap, source::text(heap, "#x"), false); let y = source::new_symbol(heap, source::text(heap, "#y"), false);
    let f = source::closure(heap, 0, []);
    job.operands.push(o); job.operands.push(x); job.operands.push(source::number(1));
    source::define_private(job, opcode::DefinePrivateField);
    assert(heap.fault == source::Fault.None && job.operands.len == 1 && job.operands.pop() == o);
    let elements = source::node(heap, o.id()).metadata; assert(elements.is_object() && source::node(heap, elements.id()).kind == node_kind::PrivateElements);
    let before = source::plain_rows(heap.nodes[o.id()..]); let records = source::plain_rows(heap.properties[..]);
    let nodes = heap.nodes.len; let properties = heap.properties.len; let accessors = heap.accessors.len;
    for limit in 3 {
        job.protected_nodes = 0; job.owned_nodes = 0; job.foreign.clear();
        if limit == 0 { job.protected_nodes = heap.nodes.len; } else if limit == 1 { job.owned_nodes = heap.nodes.len; } else { job.foreign.push(o.id()); job.foreign.push(bare.id() + 1); }
        heap.fault = source::Fault.None; job.operands.clear();
        job.operands.push(o); job.operands.push(x); job.operands.push(source::number(2));
        source::put_private(job);
        assert(heap.fault == source::Fault.NativeBoundary);
        for op in [opcode::DefinePrivateField, opcode::DefinePrivateMethod, opcode::DefinePrivateGetter, opcode::DefinePrivateSetter] {
            for target in [o, bare] {
                heap.fault = source::Fault.None; job.operands.clear();
                job.operands.push(target); job.operands.push(y); job.operands.push(f);
                source::define_private(job, op);
                assert(heap.fault == source::Fault.NativeBoundary);
            }
        }
        assert(source::plain_rows(heap.nodes[o.id()..]) == before && source::plain_rows(heap.properties[..]) == records && heap.nodes.len == nodes && heap.properties.len == properties && heap.accessors.len == accessors);
        // Reads and brand checks change nothing and still answer.
        heap.fault = source::Fault.None; job.operands.clear();
        job.operands.push(o); job.operands.push(x); source::get_private(job);
        assert(heap.fault == source::Fault.None && job.operands.pop() == source::number(1));
        job.operands.push(x); job.operands.push(bare); source::private_in(job);
        assert(heap.fault == source::Fault.None && job.operands.pop() == source::boolean(false));
    }
    print("private boundaries: protected, owned and foreign rows fault NativeBoundary unchanged on PutPrivate and every DefinePrivate; reads and brand checks answer passed");
}
thread_fn integrity_parent() {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; snapshot_test::prepare(code, heap, bindings);
    var job = source::Job { heap: heap };
    let p = source::allocate(heap, source::Node { kind: node_kind::Object }); let f = source::allocate(heap, source::Node { kind: node_kind::Object });
    let s = source::allocate(heap, source::Node { kind: node_kind::Array }); let a = source::allocate(heap, source::Node { kind: node_kind::Array }); let d = source::allocate(heap, source::Node { kind: node_kind::Array });
    assert(source::put(heap, p, source::text(heap, "x"), source::number(1)) && source::put(heap, f, source::text(heap, "y"), source::number(2)) && source::put(heap, f, source::text(heap, "3"), source::number(3)));
    for v, i in [source::number(1), source::number(2)] { assert(source::set_element(heap, s, i, v) && source::set_element(heap, a, i, v)); }
    assert(source::set_element(heap, d, 0, source::number(5)) && source::set_element(heap, d, 5000, source::number(6)) && source::node(heap, d.id()).initialized);
    assert(source::integrity_builtin(job, source::Builtin.ObjectPreventExtensions, p) == p && source::integrity_builtin(job, source::Builtin.ObjectFreeze, f) == f);
    assert(source::integrity_builtin(job, source::Builtin.ObjectSeal, s) == s && source::integrity_builtin(job, source::Builtin.ObjectFreeze, a) == a && source::integrity_builtin(job, source::Builtin.ObjectFreeze, d) == d);
    assert(heap.fault == source::Fault.None && source::node(heap, a.id()).function & array_flag::Frozen != 0 && !source::node(heap, a.id()).initialized);
    for v, i in [p, f, s, a, d] { bindings.push(snapshot::Binding { id: 20 + i, generation: 1, value: v }); }
    var c = snapshot::Control {}; let saved = snapshot::capture(heap, bindings, c); assert(c.error == snapshot::Error.None);
    var encoded: u8[>..] = []; var enc = snapshot::Control {}; assert(snapshot::encode(saved, encoded, enc));
    var dec = snapshot::Control {}; let decoded = snapshot::decode(encoded, dec); assert(dec.error == snapshot::Error.None && decoded.len == 1);
    for image in decoded { assert(qput(Saved { image: copy(image) })); }
}
thread_fn integrity_restored(saved: snapshot::Snapshot) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; var r = snapshot::Control {};
    assert(snapshot::restore(saved, code, heap, roots, source::reserve_identity(), r));
    var job = source::Job { heap: heap };
    let base = roots.len - 5; let p = roots[base].value; let f = roots[base + 1].value; let s = roots[base + 2].value; let a = roots[base + 3].value; let d = roots[base + 4].value;
    for v in [p, f, s, a, d] { assert(source::node(heap, v.id()).non_extensible && source::integrity_builtin(job, source::Builtin.ObjectIsExtensible, v) == source::boolean(false)); }
    for v in [f, a, d] { assert(source::integrity_builtin(job, source::Builtin.ObjectIsFrozen, v) == source::boolean(true)); }
    assert(source::integrity_builtin(job, source::Builtin.ObjectIsSealed, s) == source::boolean(true) && source::integrity_builtin(job, source::Builtin.ObjectIsFrozen, s) == source::boolean(false));
    assert(source::integrity_builtin(job, source::Builtin.ObjectIsSealed, p) == source::boolean(false));
    // Existing properties keep their attributes; new ones are refused.
    let y = source::own_entry(heap, f, source::key_named(heap, "y")); let three = source::own_entry(heap, f, source::key_named(heap, "3"));
    assert(y.found && y.attrs == source::attr_enumerable() && three.found && three.attrs == source::attr_enumerable() && !three.element);
    assert(accepts(heap, p, "x", source::number(7)) && !accepts(heap, p, "z", source::number(1)) && accepts(heap, s, "0", source::number(9)) && !accepts(heap, s, "2", source::number(1)) && !accepts(heap, a, "0", source::number(9)) && !accepts(heap, d, "0", source::number(9)));
    assert(source::get(heap, d, source::text(heap, "5000")).num() == 6 && source::node(heap, d.id()).length == 5001);
    assert(source::get(heap, s, source::text(heap, "0")).num() == 9 && source::get(heap, a, source::text(heap, "0")).num() == 1 && heap.fault == source::Fault.None);
    print("snapshot integrity levels: [[Extensible]], sealed and frozen arrays' flags and restricted shapes and records survive restoration passed");
}
// Whether [[Set]] of key k on object o accepts v (no setter runs here).
fn accepts(heap: source::Heap&, o: source::Value, k: const u8[:], v: source::Value) -> bool {
    let setter, ok = source::write(heap, o, source::text(heap, k), v);
    return ok && setter.is_undefined() && heap.fault == source::Fault.None;
}
thread_fn integrity_malformed(saved: snapshot::Snapshot, mode: i64) {
    defer { heap_program::finish(); }
    {
        var bad = copy(saved);
        var sealed: i64 = -1; var cell: i64 = -1;
        for n, i in bad.nodes { if n.kind == node_kind::Array && n.function & array_flag::Sealed != 0 && sealed < 0 { sealed = i; } if n.kind == node_kind::Binding && cell < 0 { cell = i; } }
        assert(sealed >= 0 && cell >= 0);
        match mode {
            0 => { bad.nodes[cell].non_extensible = true; },
            1 => { bad.nodes[sealed].non_extensible = false; },
            2 => { bad.nodes[sealed].initialized = true; bad.nodes[sealed].count = 0; },
            _ => { bad.nodes[sealed].function = array_flag::Frozen; },
        }
        var dest = source::Code {}; var target = source::Heap { code: dest }; var roots: snapshot::Binding[>..] = []; var check = snapshot::Control {};
        assert(!snapshot::restore(bad, dest, target, roots, source::reserve_identity(), check) && check.error == snapshot::Error.Link && target.nodes.len == 0);
    }
}
// main builds no heap; every heap is its own thread program.
fn main() { dormant_constants(); callable_publication(); heap_program::join(thread_spawn(traversal_flags)); finite_map_ownership(); assembly_ownership(); source_spans(); argument_ownership(); isolation(); wrappers_roundtrip(); bound_roundtrip(); integrity_roundtrip(); malformed(); stale_classification(); reused_destination(); truncated_image(); reused_layouts(); suspended_continuations(); runtime_code(); runtime_eval_callable_publication(); heap_program::join(thread_spawn(live_boundary)); }

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
    // The wait a saved stack stopped at, for tests that check its frame.
    wait: i64 = 0,
}
fn suspended_continuations() {
    let wait = embed_file("../corpus/serial/suspended-wait.gsp");
    heap_program::join(thread_spawn(suspended_wait, str(wait), false));
    heap_program::join(thread_spawn(suspended_wait, str(wait), true)); let saved = qget<Waiting>();
    heap_program::join(thread_spawn(resumed_wait, copy(saved)));
    let mapped = embed_file("../corpus/serial/sloppy-mapped-wait.gsp");
    heap_program::join(thread_spawn(mapped_wait, str(mapped), false));
    heap_program::join(thread_spawn(mapped_wait, str(mapped), true)); let pending_mapped = qget<Waiting>();
    heap_program::join(thread_spawn(mapped_resumed, copy(pending_mapped)));
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
    // The Function constructor (light-cljs-857): a wait inside its first
    // argument's toString, in place or after the stack was saved and
    // restored in a fresh heap.
    let dynamic = embed_file("../corpus/serial/function-constructor-wait.gsp");
    heap_program::join(thread_spawn(dynamic_function_wait, str(dynamic), false));
    heap_program::join(thread_spawn(dynamic_function_wait, str(dynamic), true)); let pending_function = qget<Waiting>();
    heap_program::join(thread_spawn(dynamic_function_resumed, copy(pending_function)));
    // A wait in newTarget's `prototype` getter after the Function
    // constructor compiled (light-cljs-887): its DynamicFunction frame at pc
    // 1, in place and from a saved continuation; a saved frame at pc 1
    // without its function is refused.
    let prototype_wait = embed_file("../corpus/serial/function-prototype-wait.gsp");
    heap_program::join(thread_spawn(function_prototype_wait, str(prototype_wait), false));
    heap_program::join(thread_spawn(function_prototype_wait, str(prototype_wait), true)); let pending_prototype = qget<Waiting>();
    heap_program::join(thread_spawn(function_prototype_resumed, copy(pending_prototype)));
    // for-of (light-cljs-843): five waits, in place, or each after the
    // stack was saved and restored in a fresh heap.
    let iterating = embed_file("../corpus/serial/for-of-wait.gsp");
    heap_program::join(thread_spawn(for_of_wait, str(iterating), false));
    heap_program::join(thread_spawn(for_of_wait, str(iterating), true));
    var waits: i64 = 1;
    while qget<bool>() { let w = qget<Waiting>(); heap_program::join(thread_spawn(for_of_resumed, copy(w))); waits += 1; }
    assert(waits == 6);
    // The built-in iterators (light-cljs-844): three waits, in an element
    // getter above %ArrayIteratorPrototype%.next's frame and in two loop
    // bodies, each cursor resuming at its index.
    let cursors = embed_file("../corpus/serial/builtin-iterator-wait.gsp");
    heap_program::join(thread_spawn(builtin_iterator_wait, str(cursors), false));
    heap_program::join(thread_spawn(builtin_iterator_wait, str(cursors), true));
    waits = 1;
    while qget<bool>() { let w = qget<Waiting>(); heap_program::join(thread_spawn(builtin_iterator_resumed, copy(w))); waits += 1; }
    assert(waits == 4);
    // for-in over a proxy (light-cljs-845): seventeen waits, one in every trap
    // phase, each in a frame above a Proxy frame above the ForIn frame.
    let traps = embed_file("../corpus/serial/proxy-for-in-wait.gsp");
    heap_program::join(thread_spawn(proxy_wait, str(traps), false));
    heap_program::join(thread_spawn(proxy_wait, str(traps), true));
    waits = 1;
    while qget<bool>() { let w = qget<Waiting>(); heap_program::join(thread_spawn(proxy_resumed, copy(w))); waits += 1; }
    assert(waits == 18);
    // Property descriptors (light-cljs-881): four waits, one in every
    // phase of the Descriptor frame, each resumed from a saved stack.
    let defining = embed_file("../corpus/serial/define-property-wait.gsp");
    heap_program::join(thread_spawn(define_property_wait, str(defining), false));
    heap_program::join(thread_spawn(define_property_wait, str(defining), true));
    waits = 1;
    while qget<bool>() { let w = qget<Waiting>(); heap_program::join(thread_spawn(define_property_resumed, copy(w))); waits += 1; }
    assert(waits == 5);
    // The Object statics (light-cljs-884): eleven waits, one in every phase
    // of the Statics frame, each resumed from a saved stack.
    let statics = embed_file("../corpus/serial/object-statics-wait.gsp");
    heap_program::join(thread_spawn(object_statics_wait, str(statics), false));
    heap_program::join(thread_spawn(object_statics_wait, str(statics), true));
    waits = 1;
    while qget<bool>() { let w = qget<Waiting>(); heap_program::join(thread_spawn(object_statics_resumed, copy(w))); waits += 1; }
    assert(waits == 12);
    // Symbol values (light-cljs-885): symbol rows and the Symbol.for
    // registry across a wait, in place and from a saved continuation.
    let symbols = embed_file("../corpus/serial/symbols-wait.gsp");
    heap_program::join(thread_spawn(symbols_wait, str(symbols), false));
    heap_program::join(thread_spawn(symbols_wait, str(symbols), true));
    assert(qget<bool>()); let sw = qget<Waiting>(); heap_program::join(thread_spawn(symbols_resumed, copy(sw)));
    heap_program::join(thread_spawn(symbols_reused, str(symbols)));
    // Base classes (light-cljs-886): a wait inside newTarget's `prototype`
    // getter, above the Construct frame, in place and from a saved
    // continuation; an image whose class constructor looks lazy is refused.
    let classes = embed_file("../corpus/serial/classes-wait.gsp");
    heap_program::join(thread_spawn(classes_wait, str(classes), false));
    heap_program::join(thread_spawn(classes_wait, str(classes), true));
    assert(qget<bool>()); let cw = qget<Waiting>(); heap_program::join(thread_spawn(symbols_resumed, copy(cw)));
    // Derived classes (light-cljs-887): a wait in a parent constructor
    // during super(), above the derived frame whose `this` cell is unbound,
    // inside a built-in's newTarget `prototype` getter above its Construct
    // frame, in place and from a saved continuation; a saved Construct
    // frame whose constructor is a built-in without [[Construct]] is refused.
    let derived = embed_file("../corpus/serial/derived-classes-wait.gsp");
    heap_program::join(thread_spawn(derived_wait, str(derived), false));
    heap_program::join(thread_spawn(derived_wait, str(derived), true));
    assert(qget<bool>()); let dw = qget<Waiting>(); heap_program::join(thread_spawn(symbols_resumed, copy(dw)));
    // Super properties and Reflect.get and Reflect.set (light-cljs-888):
    // eight waits, in a super getter, key conversion and setter and in
    // every phase of the Reflect frames, each resumed from a saved stack
    // with the methods' home objects.
    let supers = embed_file("../corpus/serial/super-reflect-wait.gsp");
    heap_program::join(thread_spawn(super_reflect_wait, str(supers), false));
    heap_program::join(thread_spawn(super_reflect_wait, str(supers), true));
    waits = 1;
    while qget<bool>() { let w = qget<Waiting>(); heap_program::join(thread_spawn(super_reflect_resumed, copy(w))); waits += 1; }
    assert(waits == 9);
    // The Proxy traps of light-cljs-891: eight waits, one in each trap
    // while its Proxy frame awaits the result and one holding a revoked
    // proxy, each resumed from a saved stack.
    let traps = embed_file("../corpus/serial/proxy-traps-wait.gsp");
    heap_program::join(thread_spawn(proxy_traps_wait, str(traps), false));
    heap_program::join(thread_spawn(proxy_traps_wait, str(traps), true));
    waits = 1;
    while qget<bool>() { let w = qget<Waiting>(); heap_program::join(thread_spawn(proxy_traps_resumed, copy(w))); waits += 1; }
    assert(waits == 9);
    // Integrity levels of a proxy and a proxy target in the ownKeys
    // invariants (light-cljs-897): nine waits, six under an integrity
    // built-in's Statics frame and three under a [[OwnPropertyKeys]] frame
    // at its target phases, each resumed from a saved stack.
    let integrity = embed_file("../corpus/serial/proxy-integrity-wait.gsp");
    heap_program::join(thread_spawn(proxy_integrity_wait, str(integrity), false));
    heap_program::join(thread_spawn(proxy_integrity_wait, str(integrity), true));
    waits = 1;
    while qget<bool>() { let w = qget<Waiting>(); heap_program::join(thread_spawn(proxy_integrity_resumed, copy(w))); waits += 1; }
    assert(waits == 10);
    // Object spread, object rest and getOwnPropertyDescriptor through
    // proxies (light-cljs-895): nine waits, five under a CopyData frame,
    // two under a getOwnPropertyDescriptor built-in's Descriptor frame and
    // two under a [[GetOwnProperty]] Proxy frame at its proxy target
    // phases, each resumed from a saved stack.
    let copies = embed_file("../corpus/serial/proxy-copy-wait.gsp");
    heap_program::join(thread_spawn(proxy_copy_wait, str(copies), false));
    heap_program::join(thread_spawn(proxy_copy_wait, str(copies), true));
    waits = 1;
    while qget<bool>() { let w = qget<Waiting>(); heap_program::join(thread_spawn(proxy_copy_resumed, copy(w))); waits += 1; }
    assert(waits == 10);
    // A proxy's [[Get]], [[Set]], [[HasProperty]] and [[Delete]]
    // (light-cljs-890, -896): sixteen waits across the phases of their
    // Proxy frames, each frame collected and checked in place, or
    // collected, saved, refused when malformed and resumed from a saved
    // stack.
    let properties = embed_file("../corpus/serial/proxy-property-wait.gsp");
    heap_program::join(thread_spawn(proxy_property_wait, str(properties), false));
    heap_program::join(thread_spawn(proxy_property_wait, str(properties), true));
    waits = 1;
    while qget<bool>() { let w = qget<Waiting>(); heap_program::join(thread_spawn(proxy_property_resumed, copy(w))); waits += 1; }
    assert(waits == 17);
    // An array's object `length` (light-cljs-898): eight waits, two in each
    // definition's conversions, above a Descriptor frame at pc 5 or a
    // [[DefineOwnProperty]] Proxy frame at pc Length, each resumed from a
    // saved stack.
    let lengths = embed_file("../corpus/serial/array-length-definition-wait.gsp");
    heap_program::join(thread_spawn(length_definition_wait, str(lengths), false));
    heap_program::join(thread_spawn(length_definition_wait, str(lengths), true));
    waits = 1;
    while qget<bool>() { let w = qget<Waiting>(); heap_program::join(thread_spawn(length_definition_resumed, copy(w))); waits += 1; }
    assert(waits == 9);
    // Private class elements (light-cljs-889): four waits, in a private
    // getter, setter, method and static method, each resumed from a saved
    // stack with the private names and elements; images whose private
    // elements lose their owner, gain a second one or key a record by a
    // string are refused.
    let privates = embed_file("../corpus/serial/private-elements-wait.gsp");
    heap_program::join(thread_spawn(private_wait, str(privates), false));
    heap_program::join(thread_spawn(private_wait, str(privates), true));
    waits = 1;
    while qget<bool>() { let w = qget<Waiting>(); heap_program::join(thread_spawn(private_resumed, copy(w))); waits += 1; }
    assert(waits == 5);
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
// Mapped arguments objects across a dependency wait (sloppy-mapped-wait.js,
// light-cljs-868): the sloppy script's entry waits with its own mapped cell
// on the stack, an escaped mapped object whose closures keep its
// parameters (index 1 moved to a shape key by a redefinition that kept the
// link) and a sloppy async function awaiting with its mapped object.
// Answered in place, or saved: the image's mapped links validate, a cell
// naming the wrong index is refused, and a fresh heap that restores it
// completes the job with aliasing both ways and a delete that unmaps.
fn mapped_wait_start(heap: source::Heap&, job: source::Job&) {
    assert(source::initialize(heap) && source::install_field(heap, heap.global, "dep", source::builtin(heap, 8)));
    assert(source::start(job, source::closure(heap, 0, []), []));
    assert(source::run(job, -1) == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8);
}
fn mapped_wait_done(job: source::Job&) {
    assert(source::deliver_effect(job, source::text(job.heap, "D")));
    assert(source::run(job, -1) == tiny::Outcome.Complete && source::key_is(job.heap, job.result, "D,s,,t,m:n,qpB:undefined,true"));
    assert(source::retire(job));
}
thread_fn mapped_wait(bytes: u8[], save: bool) {
    defer { heap_program::finish(); }
    var code = source::Code {}; snapshot_test::append(code, bytes);
    var heap = source::Heap { code: code }; var job = source::Job { heap: heap, effects_enabled: true };
    mapped_wait_start(heap, job);
    // Three mapped objects (the entry's, keep's, af's) and their cells.
    var maps: i64 = 0; var cells: i64 = 0; var moved: i64 = 0;
    for n in heap.nodes {
        if n.kind == node_kind::ArgumentMap { maps += 1; }
        if n.kind == node_kind::MappedBinding { cells += 1; if !n.namespace_value.is_null() { moved += 1; } }
    }
    assert(maps == 3 && cells == 5 && moved == 1);
    if !save { mapped_wait_done(job); return; }
    let stack = source::suspend_stack(job, -1); assert(stack >= 0 && job.frames.len == 0 && source::activation(heap, stack).frame_count == 2);
    assert(source::retire(job, true));
    var c = snapshot::Control {}; let image = snapshot::capture(heap, [], c); assert(c.error == snapshot::Error.None);
    var valid = snapshot::Control {}; assert(snapshot::validate(image, valid));
    // A cell naming another index of its object's map is refused.
    var bad = copy(image); var changed = false;
    for n, i in image.nodes { if !changed && n.kind == node_kind::MappedBinding && n.length == 0 { bad.nodes[i].length = 1; changed = true; } }
    assert(changed);
    var check = snapshot::Control {}; assert(!snapshot::validate(bad, check) && check.error == snapshot::Error.Link);
    assert(qput(Waiting { image: copy(image), stack: stack as u32, queue: copy(job.queue), queue_head: job.queue_head, draining: job.draining, entry: job.entry, rejections: copy(job.rejections), steps: job.steps, kind: job.effect_kind, count: job.effect_count, frame: job.effect_frame, args: job.effect_args }));
}
thread_fn mapped_resumed(saved: Waiting) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; var r = snapshot::Control {};
    assert(snapshot::restore(saved.image, code, heap, bindings, source::reserve_identity(), r));
    var job = source::Job { heap: heap, effects_enabled: true };
    assert(source::resume_stack(job, saved.stack) && job.frames.len == 2);
    for t in saved.queue { job.queue.push(t); } job.queue_head = saved.queue_head; job.draining = saved.draining; job.entry = saved.entry; for p in saved.rejections { job.rejections.push(p); }
    job.steps = saved.steps; job.registered = true; heap.live_jobs += 1; job.effect_pending = true; job.effect_kind = saved.kind; job.effect_count = saved.count; job.effect_frame = saved.frame; for v, i in saved.args { job.effect_args[i] = v; }
    mapped_wait_done(job);
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
// A dependency wait inside the Function constructor's first conversion
// (function-constructor-wait.js, light-cljs-857): the DynamicFunction frame
// holds its two arguments as operands, its conversion index 0 and
// Reflect.construct's newTarget, below the Convert frame and the toString
// frame that waits; answered in place or after the whole stack was saved as
// a continuation and restored in a fresh heap (dynamic_function_resumed),
// the second argument converts once and the new function, compiled into the
// job's writable code, takes the newTarget's prototype.
fn dynamic_function_frame(job: const source::Job&) -> source::Frame {
    var found: i64 = -1; var count: i64 = 0;
    for f, i in job.frames { match f.native { DynamicFunction => { found = i; count += 1; }, _ => {} } }
    assert(count == 1);
    let d = job.frames[found];
    assert(d.index == 0 && d.argument_count == 2 && d.receiver.is_object() && job.operands.len >= d.operand_base + 2);
    return d;
}
thread_fn dynamic_function_wait(bytes: u8[], save: bool) {
    defer { heap_program::finish(); }
    var code = source::Code {}; snapshot_test::append(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var job = source::Job { heap: heap, effects_enabled: true, writable_code: code }; assert(source::start(job, source::closure(heap, 0, []), [source::builtin(heap, 8)]));
    assert(source::run(job, -1) == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8);
    dynamic_function_frame(job);
    if !save {
        assert(source::deliver_effect(job, source::text(heap, "x")));
        assert(source::run(job, -1) == tiny::Outcome.Complete && source::key_is(heap, job.result, "pb:4:true"));
        assert(source::retire(job)); return;
    }
    let stack = source::suspend_stack(job, -1); assert(stack >= 0 && job.frames.len == 0);
    assert(source::retire(job, true));
    var k = snapshot::Control {}; let image = snapshot::capture(heap, [], k); assert(k.error == snapshot::Error.None);
    assert(qput(Waiting { image: copy(image), stack: stack as u32, steps: job.steps, kind: job.effect_kind, count: job.effect_count, frame: job.effect_frame, args: job.effect_args }));
}
thread_fn dynamic_function_resumed(saved: Waiting) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; var r = snapshot::Control {};
    assert(snapshot::restore(saved.image, code, heap, bindings, source::reserve_identity(), r));
    let functions = code.functions.len;
    var job = source::Job { heap: heap, effects_enabled: true, writable_code: code };
    assert(source::resume_stack(job, saved.stack));
    dynamic_function_frame(job);
    job.steps = saved.steps; job.registered = true; heap.live_jobs += 1; job.effect_pending = true; job.effect_kind = saved.kind; job.effect_count = saved.count; job.effect_frame = saved.frame; for v, i in saved.args { job.effect_args[i] = v; }
    assert(source::deliver_effect(job, source::text(heap, "x")));
    assert(source::run(job, -1) == tiny::Outcome.Complete && source::key_is(heap, job.result, "pb:4:true"));
    assert(code.functions.len > functions);
    assert(source::retire(job));
}
// function-prototype-wait.js (light-cljs-887): the one DynamicFunction
// frame waits at pc 1 for newTarget's `prototype` getter, holding the
// function it compiled.
fn function_prototype_frame(job: const source::Job&) -> source::Frame {
    var found: i64 = -1; var count: i64 = 0;
    for f, i in job.frames { match f.native { DynamicFunction => { found = i; count += 1; }, _ => {} } }
    assert(count == 1);
    let d = job.frames[found];
    assert(d.pc == 1 && d.argument_count == 2 && d.receiver.is_object() && d.output.is_object() && source::node(job.heap, d.output.id()).kind == node_kind::Closure);
    return d;
}
thread_fn function_prototype_wait(bytes: u8[], save: bool) {
    defer { heap_program::finish(); }
    var code = source::Code {}; snapshot_test::append(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var job = source::Job { heap: heap, effects_enabled: true, writable_code: code }; assert(source::start(job, source::closure(heap, 0, []), [source::builtin(heap, 8)]));
    assert(source::run(job, -1) == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8);
    function_prototype_frame(job);
    if !save {
        assert(source::deliver_effect(job, source::text(heap, "x")));
        assert(source::run(job, -1) == tiny::Outcome.Complete && source::key_is(heap, job.result, "1:4:true"));
        assert(source::retire(job)); return;
    }
    let stack = source::suspend_stack(job, -1); assert(stack >= 0 && job.frames.len == 0);
    assert(source::retire(job, true));
    let a = source::activation(heap, stack);
    var k = snapshot::Control {}; let image = snapshot::capture(heap, [], k); assert(k.error == snapshot::Error.None);
    var bad = copy(image); var changed = false;
    for i in a.frame_count {
        let r = a.frames + i;
        match image.conts[r] {
            Frame x => {
                var f = x.f;
                match f.native { DynamicFunction => { f.output = source::Value.Undefined; changed = true; }, _ => {} }
                bad.conts[r] = source::Cont.Frame { f: f };
            },
            _ => {},
        }
    }
    assert(changed);
    var check = snapshot::Control {}; assert(!snapshot::validate(bad, check) && check.error == snapshot::Error.Link);
    assert(qput(Waiting { image: copy(image), stack: stack as u32, steps: job.steps, kind: job.effect_kind, count: job.effect_count, frame: job.effect_frame, args: job.effect_args }));
}
thread_fn function_prototype_resumed(saved: Waiting) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; var r = snapshot::Control {};
    assert(snapshot::restore(saved.image, code, heap, bindings, source::reserve_identity(), r));
    var job = source::Job { heap: heap, effects_enabled: true, writable_code: code };
    assert(source::resume_stack(job, saved.stack));
    function_prototype_frame(job);
    job.steps = saved.steps; job.registered = true; heap.live_jobs += 1; job.effect_pending = true; job.effect_kind = saved.kind; job.effect_count = saved.count; job.effect_frame = saved.frame; for v, i in saved.args { job.effect_args[i] = v; }
    assert(source::deliver_effect(job, source::text(heap, "x")));
    assert(source::run(job, -1) == tiny::Outcome.Complete && source::key_is(heap, job.result, "1:4:true"));
    assert(source::retire(job));
}
// Dependency waits inside for-of (for-of-wait.js, light-cljs-843): in the
// user @@iterator method and a `next` getter (above GetIterator's native
// frame), in `next`, in the body and in `return`, answered in place, or
// each after the whole stack was saved as a continuation and restored in a
// fresh heap (for_of_resumed): the loop resumes where it waited and
// `return` runs once. A saving run publishes true and the wait, or false
// once the job completed.
thread_fn for_of_wait(bytes: u8[], save: bool) {
    defer { heap_program::finish(); }
    var code = source::Code {}; snapshot_test::append(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var job = source::Job { heap: heap, effects_enabled: true }; assert(source::start(job, source::closure(heap, 0, []), [source::builtin(heap, 8)]));
    assert(source::run(job, -1) == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8);
    // The first wait is in the @@iterator method, above GetIterator's frame.
    var natives: i64 = 0; for f in job.frames { match f.native { GetIterator => { natives += 1; }, _ => {} } }
    assert(natives == 1 && job.frames.len == 3);
    if save { for_of_save(job, heap); return; }
    var waits: i64 = 1;
    loop {
        assert(source::deliver_effect(job, source::text(heap, "D")));
        let outcome = source::run(job, -1);
        if outcome == tiny::Outcome.Complete { break; }
        assert(outcome == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8); waits += 1;
    }
    assert(waits == 5 && source::key_is(heap, job.result, "iDnDbDRDgD12"));
    assert(source::retire(job));
}
fn for_of_save(job: source::Job&, heap: source::Heap&) {
    let stack = source::suspend_stack(job, -1); assert(stack >= 0 && job.frames.len == 0 && job.operands.len == 0);
    assert(source::retire(job, true));
    var k = snapshot::Control {}; let image = snapshot::capture(heap, [], k); assert(k.error == snapshot::Error.None);
    assert(qput(true));
    assert(qput(Waiting { image: copy(image), stack: stack as u32, steps: job.steps, kind: job.effect_kind, count: job.effect_count, frame: job.effect_frame, args: job.effect_args }));
}
thread_fn for_of_resumed(saved: Waiting) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; var r = snapshot::Control {};
    assert(snapshot::restore(saved.image, code, heap, bindings, source::reserve_identity(), r));
    var job = source::Job { heap: heap, effects_enabled: true };
    assert(source::resume_stack(job, saved.stack));
    job.steps = saved.steps; job.registered = true; heap.live_jobs += 1; job.effect_pending = true; job.effect_kind = saved.kind; job.effect_count = saved.count; job.effect_frame = saved.frame; for v, i in saved.args { job.effect_args[i] = v; }
    assert(source::deliver_effect(job, source::text(heap, "D")));
    let outcome = source::run(job, -1);
    if outcome == tiny::Outcome.Yielded { assert(job.effect_pending && job.effect_kind == 8); for_of_save(job, heap); return; }
    assert(outcome == tiny::Outcome.Complete && source::key_is(heap, job.result, "iDnDbDRDgD12"));
    assert(source::retire(job));
    assert(qput(false));
}
// Dependency waits inside built-in iteration (builtin-iterator-wait.js,
// light-cljs-844): in an element getter above the array iterator's native
// frame, in the body of a loop over an array and over a string, answered in
// place, or each after the whole stack was saved as a continuation and
// restored in a fresh heap (builtin_iterator_resumed); each cursor resumes
// at the index where it waited.
thread_fn builtin_iterator_wait(bytes: u8[], save: bool) {
    defer { heap_program::finish(); }
    var code = source::Code {}; snapshot_test::append(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var job = source::Job { heap: heap, effects_enabled: true }; assert(source::start(job, source::closure(heap, 0, []), [source::builtin(heap, 8)]));
    assert(source::run(job, -1) == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8);
    // The first wait is in the element getter, above next's native frame.
    var natives: i64 = 0; for f in job.frames { match f.native { BuiltinIterator => { natives += 1; }, _ => {} } }
    assert(natives == 1 && job.frames.len == 3);
    if save { iterator_frame_refusals(job, heap); return; }
    var waits: i64 = 1;
    loop {
        assert(source::deliver_effect(job, source::text(heap, "D")));
        let outcome = source::run(job, -1);
        if outcome == tiny::Outcome.Complete { break; }
        assert(outcome == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8); waits += 1;
    }
    assert(waits == 3 && source::key_is(heap, job.result, "agDbc1wD23xsDy"));
    assert(source::retire(job));
}
// Save the first wait (as for_of_save does), checking that its saved
// %ArrayIteratorPrototype%.next frame is refused when its cursor is a string
// iteration's (mode 0) or its source is not an object (mode 1).
fn iterator_frame_refusals(job: source::Job&, heap: source::Heap&) {
    let stack = source::suspend_stack(job, -1); assert(stack >= 0 && job.frames.len == 0 && job.operands.len == 0);
    assert(source::retire(job, true));
    let a = source::activation(heap, stack);
    var c = snapshot::Control {}; let image = snapshot::capture(heap, [], c); assert(c.error == snapshot::Error.None);
    for mode in 2 {
        var bad = copy(image); var changed = false;
        for i in a.frame_count {
            let r = a.frames + i;
            match image.conts[r] {
                Frame x => {
                    var f = x.f;
                    match f.native {
                        BuiltinIterator => {
                            if mode == 0 { bad.nodes[f.input.id()].function = iteration::CodePoints; bad.nodes[f.input.id()].value = source::Value.Undefined; } else { f.receiver = source::Value.Undefined; }
                            changed = true;
                        },
                        _ => {},
                    }
                    bad.conts[r] = source::Cont.Frame { f: f };
                },
                _ => {},
            }
        }
        assert(changed);
        var check = snapshot::Control {}; assert(!snapshot::validate(bad, check) && check.error == snapshot::Error.Link);
        var dest = source::Code {}; var target_heap = source::Heap { code: dest }; var roots: snapshot::Binding[>..] = []; var refused = snapshot::Control {};
        assert(!snapshot::restore(bad, dest, target_heap, roots, source::reserve_identity(), refused) && refused.error == snapshot::Error.Link && target_heap.nodes.len == 0);
    }
    assert(qput(true));
    assert(qput(Waiting { image: copy(image), stack: stack as u32, steps: job.steps, kind: job.effect_kind, count: job.effect_count, frame: job.effect_frame, args: job.effect_args }));
}
// Dependency waits in every trap phase of for-in over a proxy
// (proxy-for-in-wait.js, light-cljs-845): the handler's getters, both traps,
// the key list's `length` and element getters, the descriptor's field
// getters and the getPrototypeOf trap, answered in place, or each after the
// whole stack was saved as a continuation and restored in a fresh heap
// (proxy_resumed). The first wait is in the ownKeys getter, above a Proxy
// frame above the ForIn frame.
thread_fn proxy_wait(bytes: u8[], save: bool) {
    defer { heap_program::finish(); }
    var code = source::Code {}; snapshot_test::append(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var job = source::Job { heap: heap, effects_enabled: true }; assert(source::start(job, source::closure(heap, 0, []), [source::builtin(heap, 8)]));
    assert(source::run(job, -1) == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8);
    var walks: i64 = 0; var traps: i64 = 0;
    for f in job.frames { match f.native { ForIn => { walks += 1; }, Proxy => { traps += 1; }, _ => {} } }
    assert(walks == 1 && traps == 1 && job.frames.len == 5);
    if save { proxy_frame_refusals(job, heap); return; }
    var waits: i64 = 1;
    loop {
        assert(source::deliver_effect(job, source::text(heap, "D")));
        let outcome = source::run(job, -1);
        if outcome == tiny::Outcome.Complete { break; }
        assert(outcome == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8); waits += 1;
    }
    assert(waits == 17 && source::key_is(heap, job.result, "ab:gDkDlDiDhDdDeDcDvDwDhDdDeDcDGDSDpD"));
    assert(source::retire(job));
}
// Save the first wait (as for_of_save does), checking that the image is
// refused when its saved Proxy frame names no proxy (mode 0), its ForIn
// frame no key list (1), the key list is pending for no proxy (2), or the
// proxy's target is not an earlier row (3).
fn proxy_frame_refusals(job: source::Job&, heap: source::Heap&) {
    let stack = source::suspend_stack(job, -1); assert(stack >= 0 && job.frames.len == 0 && job.operands.len == 0);
    assert(source::retire(job, true));
    let a = source::activation(heap, stack);
    var c = snapshot::Control {}; let image = snapshot::capture(heap, [], c); assert(c.error == snapshot::Error.None);
    for mode in 4 {
        var bad = copy(image); var changed = false;
        for i in a.frame_count {
            let r = a.frames + i;
            match image.conts[r] {
                Frame x => {
                    var f = x.f;
                    match f.native {
                        Proxy => {
                            if mode == 0 { f.input = f.output; changed = true; }
                            if mode == 3 { bad.nodes[f.input.id()].value = f.input; changed = true; }
                        },
                        ForIn => {
                            if mode == 1 { f.input = source::Value.Undefined; changed = true; }
                            if mode == 2 { bad.nodes[f.input.id()].value = source::Value.Undefined; changed = true; }
                        },
                        _ => {},
                    }
                    bad.conts[r] = source::Cont.Frame { f: f };
                },
                _ => {},
            }
        }
        assert(changed);
        var check = snapshot::Control {}; assert(!snapshot::validate(bad, check) && check.error == snapshot::Error.Link);
        var dest = source::Code {}; var target_heap = source::Heap { code: dest }; var roots: snapshot::Binding[>..] = []; var refused = snapshot::Control {};
        assert(!snapshot::restore(bad, dest, target_heap, roots, source::reserve_identity(), refused) && refused.error == snapshot::Error.Link && target_heap.nodes.len == 0);
    }
    assert(qput(true));
    assert(qput(Waiting { image: copy(image), stack: stack as u32, steps: job.steps, kind: job.effect_kind, count: job.effect_count, frame: job.effect_frame, args: job.effect_args }));
}
thread_fn proxy_resumed(saved: Waiting) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; var r = snapshot::Control {};
    assert(snapshot::restore(saved.image, code, heap, bindings, source::reserve_identity(), r));
    var job = source::Job { heap: heap, effects_enabled: true };
    assert(source::resume_stack(job, saved.stack));
    job.steps = saved.steps; job.registered = true; heap.live_jobs += 1; job.effect_pending = true; job.effect_kind = saved.kind; job.effect_count = saved.count; job.effect_frame = saved.frame; for v, i in saved.args { job.effect_args[i] = v; }
    assert(source::deliver_effect(job, source::text(heap, "D")));
    let outcome = source::run(job, -1);
    if outcome == tiny::Outcome.Yielded { assert(job.effect_pending && job.effect_kind == 8); for_of_save(job, heap); return; }
    assert(outcome == tiny::Outcome.Complete && source::key_is(heap, job.result, "ab:gDkDlDiDhDdDeDcDvDwDhDdDeDcDGDSDpD"));
    assert(source::retire(job));
    assert(qput(false));
}
// Dependency waits in every phase of the Descriptor frame
// (define-property-wait.js, light-cljs-881): a property getter of
// defineProperties's properties object and a field getter of its
// descriptor, then defineProperty's object key in a Convert frame and a
// field getter of its descriptor, answered in place, or each after the
// whole stack was saved as a continuation and restored in a fresh heap
// (define_property_resumed). The first wait is in the property getter,
// above the Descriptor frame, whose saved state an image may not break.
thread_fn define_property_wait(bytes: u8[], save: bool) {
    defer { heap_program::finish(); }
    var code = source::Code {}; snapshot_test::append(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var job = source::Job { heap: heap, effects_enabled: true }; assert(source::start(job, source::closure(heap, 0, []), [source::builtin(heap, 8)]));
    assert(source::run(job, -1) == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8);
    var defining: i64 = 0;
    for f in job.frames { match f.native { Descriptor d => { assert(d.which == source::Builtin.ObjectDefineProperties && f.pc == 1); defining += 1; }, _ => {} } }
    assert(defining == 1 && job.frames.len == 3);
    if save { descriptor_frame_refusals(job, heap); return; }
    var waits: i64 = 1;
    loop {
        assert(source::deliver_effect(job, source::text(heap, "D")));
        let outcome = source::run(job, -1);
        if outcome == tiny::Outcome.Complete { break; }
        assert(outcome == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8); waits += 1;
    }
    assert(waits == 4 && source::key_is(heap, job.result, "aDvDkDwD:1:true"));
    assert(source::retire(job));
}
// Save the first wait (as for_of_save does), checking that the image is
// refused when its saved Descriptor frame names no target (mode 0), its key
// list is no key list (1) or its cursor lies past the keys (2).
fn descriptor_frame_refusals(job: source::Job&, heap: source::Heap&) {
    let stack = source::suspend_stack(job, -1); assert(stack >= 0 && job.frames.len == 0 && job.operands.len == 0);
    assert(source::retire(job, true));
    let a = source::activation(heap, stack);
    var c = snapshot::Control {}; let image = snapshot::capture(heap, [], c); assert(c.error == snapshot::Error.None);
    for mode in 3 {
        var bad = copy(image); var changed = false;
        for i in a.frame_count {
            let r = a.frames + i;
            match image.conts[r] {
                Frame x => {
                    var f = x.f;
                    match f.native {
                        Descriptor => {
                            if mode == 0 { f.input = source::Value.Undefined; }
                            if mode == 1 { f.output = f.input; }
                            if mode == 2 { f.argument_count = 9; }
                            changed = true;
                        },
                        _ => {},
                    }
                    bad.conts[r] = source::Cont.Frame { f: f };
                },
                _ => {},
            }
        }
        assert(changed);
        var check = snapshot::Control {}; assert(!snapshot::validate(bad, check) && check.error == snapshot::Error.Link);
    }
    assert(qput(true));
    assert(qput(Waiting { image: copy(image), stack: stack as u32, steps: job.steps, kind: job.effect_kind, count: job.effect_count, frame: job.effect_frame, args: job.effect_args }));
}
thread_fn define_property_resumed(saved: Waiting) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; var r = snapshot::Control {};
    assert(snapshot::restore(saved.image, code, heap, bindings, source::reserve_identity(), r));
    var job = source::Job { heap: heap, effects_enabled: true };
    assert(source::resume_stack(job, saved.stack));
    job.steps = saved.steps; job.registered = true; heap.live_jobs += 1; job.effect_pending = true; job.effect_kind = saved.kind; job.effect_count = saved.count; job.effect_frame = saved.frame; for v, i in saved.args { job.effect_args[i] = v; }
    assert(source::deliver_effect(job, source::text(heap, "D")));
    let outcome = source::run(job, -1);
    if outcome == tiny::Outcome.Yielded { assert(job.effect_pending && job.effect_kind == 8); for_of_save(job, heap); return; }
    assert(outcome == tiny::Outcome.Complete && source::key_is(heap, job.result, "aDvDkDwD:1:true"));
    assert(source::retire(job));
    assert(qput(false));
}
// Dependency waits in every phase of the Statics frame
// (object-statics-wait.js, light-cljs-884): Object.values's getter,
// Object.assign's setter, Object.fromEntries's `next`, entry getter and key
// conversion, Object.groupBy's callback, an iterator's `return` while the
// frame closes it (its Catch handler saved with the stack), a proxy's
// ownKeys trap, toString's @@toStringTag getter, Object.hasOwn's key
// conversion and toLocaleString's toString: answered in place, or each
// after the whole stack was saved as a continuation and restored in a fresh
// heap (object_statics_resumed).
thread_fn object_statics_wait(bytes: u8[], save: bool) {
    defer { heap_program::finish(); }
    var code = source::Code {}; snapshot_test::append(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var job = source::Job { heap: heap, effects_enabled: true }; assert(source::start(job, source::closure(heap, 0, []), [source::builtin(heap, 8)]));
    assert(source::run(job, -1) == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8);
    var walking: i64 = 0;
    for f in job.frames { match f.native { Statics x => { assert(x.which == source::Builtin.ObjectValues && f.pc == statics_pc::Value); walking += 1; }, _ => {} } }
    assert(walking == 1 && job.frames.len == 3);
    if save { statics_frame_refusals(job, heap); return; }
    var waits: i64 = 1;
    loop {
        assert(source::deliver_effect(job, source::text(heap, "D")));
        let outcome = source::run(job, -1);
        if outcome == tiny::Outcome.Complete { break; }
        assert(outcome == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8); waits += 1;
    }
    assert(waits == 11 && source::key_is(heap, job.result, "vDsDnDeDkDgDrDoDtDhDlD:1,5,7,2,true,m,[object T],true,L"));
    assert(source::retire(job));
}
// Save the first wait (as for_of_save does), checking that the image is
// refused when its saved Statics frame names no object (mode 0), its key
// list is no key list (1) or its cursor lies past the keys (2).
fn statics_frame_refusals(job: source::Job&, heap: source::Heap&) {
    let stack = source::suspend_stack(job, -1); assert(stack >= 0 && job.frames.len == 0 && job.operands.len == 0);
    assert(source::retire(job, true));
    let a = source::activation(heap, stack);
    var c = snapshot::Control {}; let image = snapshot::capture(heap, [], c); assert(c.error == snapshot::Error.None);
    for mode in 3 {
        var bad = copy(image); var changed = false;
        for i in a.frame_count {
            let r = a.frames + i;
            match image.conts[r] {
                Frame x => {
                    var f = x.f;
                    match f.native {
                        Statics => {
                            if mode == 0 { f.input = source::Value.Undefined; }
                            if mode == 1 { f.callee = f.output; }
                            if mode == 2 { f.index = 99; }
                            changed = true;
                        },
                        _ => {},
                    }
                    bad.conts[r] = source::Cont.Frame { f: f };
                },
                _ => {},
            }
        }
        assert(changed);
        var check = snapshot::Control {}; assert(!snapshot::validate(bad, check) && check.error == snapshot::Error.Link);
    }
    assert(qput(true));
    assert(qput(Waiting { image: copy(image), stack: stack as u32, steps: job.steps, kind: job.effect_kind, count: job.effect_count, frame: job.effect_frame, args: job.effect_args }));
}
thread_fn object_statics_resumed(saved: Waiting) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; var r = snapshot::Control {};
    assert(snapshot::restore(saved.image, code, heap, bindings, source::reserve_identity(), r));
    var job = source::Job { heap: heap, effects_enabled: true };
    assert(source::resume_stack(job, saved.stack));
    job.steps = saved.steps; job.registered = true; heap.live_jobs += 1; job.effect_pending = true; job.effect_kind = saved.kind; job.effect_count = saved.count; job.effect_frame = saved.frame; for v, i in saved.args { job.effect_args[i] = v; }
    assert(source::deliver_effect(job, source::text(heap, "D")));
    let outcome = source::run(job, -1);
    if outcome == tiny::Outcome.Yielded { assert(job.effect_pending && job.effect_kind == 8); for_of_save(job, heap); return; }
    assert(outcome == tiny::Outcome.Complete && source::key_is(heap, job.result, "vDsDnDeDkDgDrDoDtDhDlD:1,5,7,2,true,m,[object T],true,L"));
    assert(source::retire(job));
    assert(qput(false));
}
// symbols-wait.js (light-cljs-885) to its one wait: answered in place,
// or after the stack was saved with its symbol rows and the registry (the
// snapshot header) and restored in a fresh heap (symbols_resumed). The
// saved image is refused when its registry names an unregistered symbol,
// a registered symbol links one that is not earlier, or a value names a
// row that is no symbol as a symbol.
thread_fn symbols_wait(bytes: u8[], save: bool) {
    defer { heap_program::finish(); }
    var code = source::Code {}; snapshot_test::append(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var job = source::Job { heap: heap, effects_enabled: true }; assert(source::start(job, source::closure(heap, 0, []), [source::builtin(heap, 8)]));
    assert(source::run(job, -1) == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8);
    assert(source::registered_symbol(heap, heap.registry));
    if !save {
        assert(source::deliver_effect(job, source::text(heap, "D")) && source::run(job, -1) == tiny::Outcome.Complete && source::key_is(heap, job.result, "D:true"));
        assert(source::retire(job)); return;
    }
    let stack = source::suspend_stack(job, -1); assert(stack >= 0);
    assert(source::retire(job, true));
    var c = snapshot::Control {}; let image = snapshot::capture(heap, [], c); assert(c.error == snapshot::Error.None);
    assert(source::same_bits(image.header.registry, heap.registry));
    var unique: i64 = -1; var registered: i64 = -1;
    for n, row in image.nodes { if n.kind == node_kind::JsSymbol { if n.initialized { registered = row; } else { unique = row; } } }
    assert(unique >= 0 && registered >= 0);
    var bad = copy(image); bad.header.registry = source::symbol_at(unique);
    var check = snapshot::Control {}; assert(!snapshot::validate(bad, check) && check.error == snapshot::Error.Link);
    var chain = copy(image); chain.nodes[registered].namespace_value = source::symbol_at(registered);
    check = snapshot::Control {}; assert(!snapshot::validate(chain, check) && check.error == snapshot::Error.Link);
    var named = copy(image); named.header.registry = source::symbol_at(source::object_prototype_row());
    check = snapshot::Control {}; assert(!snapshot::validate(named, check) && check.error == snapshot::Error.Link);
    // Every registered symbol is on the chain: a Null head disconnects one.
    var cut = copy(image); cut.header.registry = source::Value.Null;
    check = snapshot::Control {}; assert(!snapshot::validate(cut, check) && check.error == snapshot::Error.Link);
    assert(qput(true));
    assert(qput(Waiting { image: copy(image), stack: stack as u32, steps: job.steps, kind: job.effect_kind, count: job.effect_count, frame: job.effect_frame, args: job.effect_args }));
}
// Run classes-wait.js to its dependency wait, inside newTarget's
// `prototype` getter above the Construct native frame; answer it in place,
// or save the continuation (symbols_resumed answers it, "D:true").
// A class constructor's closure row with its properties cleared, as a lazy
// closure's are, is refused.
thread_fn classes_wait(bytes: u8[], save: bool) {
    defer { heap_program::finish(); }
    var code = source::Code {}; snapshot_test::append(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var job = source::Job { heap: heap, effects_enabled: true }; assert(source::start(job, source::closure(heap, 0, []), [source::builtin(heap, 8)]));
    assert(source::run(job, -1) == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8 && job.frames.len == 3);
    let constructing = match job.frames[1].native { Construct => true, _ => false }; assert(constructing);
    if !save {
        assert(source::deliver_effect(job, source::text(heap, "D")) && source::run(job, -1) == tiny::Outcome.Complete && source::key_is(heap, job.result, "D:true"));
        assert(source::retire(job)); return;
    }
    let stack = source::suspend_stack(job, -1); assert(stack >= 0);
    assert(source::retire(job, true));
    var c = snapshot::Control {}; let image = snapshot::capture(heap, [], c); assert(c.error == snapshot::Error.None);
    var class_row: i64 = -1;
    for n, row in image.nodes { if n.kind == node_kind::Closure && image.functions[n.function].kind == constructor::BaseClass { class_row = row; } }
    assert(class_row >= 0 && image.nodes[class_row].key_count == 5);
    var bad = copy(image); bad.nodes[class_row].key_count = 0; bad.nodes[class_row].shape = 0; bad.nodes[class_row].keys = 0; bad.nodes[class_row].key_capacity = 0; bad.nodes[class_row].slots = [source::Value.Undefined; 2];
    var check = snapshot::Control {}; assert(!snapshot::validate(bad, check) && check.error == snapshot::Error.Link);
    assert(qput(true));
    assert(qput(Waiting { image: copy(image), stack: stack as u32, steps: job.steps, kind: job.effect_kind, count: job.effect_count, frame: job.effect_frame, args: job.effect_args }));
}
// Run derived-classes-wait.js to its dependency wait: the entry, the
// Construct frame of Array, newTarget's `prototype` getter, the derived
// class B's frame with its unbound `this` cell and its parent A's frame;
// answer it in place, or save the continuation (symbols_resumed answers it,
// "D:true"), checking that the saved Construct frame is refused with a
// built-in constructor that has no [[Construct]].
thread_fn derived_wait(bytes: u8[], save: bool) {
    defer { heap_program::finish(); }
    var code = source::Code {}; snapshot_test::append(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var job = source::Job { heap: heap, effects_enabled: true }; assert(source::start(job, source::closure(heap, 0, []), [source::builtin(heap, 8)]));
    assert(source::run(job, -1) == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8 && job.frames.len == 5);
    let constructing = match job.frames[1].native { Construct => true, _ => false };
    assert(constructing && source::node(heap, job.frames[1].callee.id()).kind == node_kind::Builtin);
    let cell = job.frames[3].receiver;
    assert(cell.is_object() && source::node(heap, cell.id()).kind == node_kind::Binding && !source::node(heap, cell.id()).initialized);
    if !save {
        assert(source::deliver_effect(job, source::text(heap, "D")) && source::run(job, -1) == tiny::Outcome.Complete && source::key_is(heap, job.result, "D:true"));
        assert(source::retire(job)); return;
    }
    let stack = source::suspend_stack(job, -1); assert(stack >= 0);
    assert(source::retire(job, true));
    let a = source::activation(heap, stack);
    var c = snapshot::Control {}; let image = snapshot::capture(heap, [], c); assert(c.error == snapshot::Error.None);
    var other: i64 = -1;
    for n, row in image.nodes { if other < 0 && n.kind == node_kind::Builtin && n.function < source::BUILTINS.len && !source::constructor_builtin(source::BUILTINS[n.function].which) { other = row; } }
    assert(other >= 0);
    var bad = copy(image); var changed = false;
    for i in a.frame_count {
        let r = a.frames + i;
        match image.conts[r] {
            Frame x => {
                var f = x.f;
                match f.native { Construct => { f.callee = source::object(other); changed = true; }, _ => {} }
                bad.conts[r] = source::Cont.Frame { f: f };
            },
            _ => {},
        }
    }
    assert(changed);
    var check = snapshot::Control {}; assert(!snapshot::validate(bad, check) && check.error == snapshot::Error.Link);
    assert(qput(true));
    assert(qput(Waiting { image: copy(image), stack: stack as u32, steps: job.steps, kind: job.effect_kind, count: job.effect_count, frame: job.effect_frame, args: job.effect_args }));
}
// Run super-reflect-wait.js (light-cljs-888) through its eight waits:
// answered in place, checking that the Reflect frames wait at pc Key,
// Result, Stored and Length (twice: both conversions of the new length)
// with an object receiver, or each after the stack was saved and restored
// in a fresh heap (super_reflect_resumed), where super_reflect_save checks
// that an image is refused when a closure's home object is no object or a
// saved Reflect frame names no target.
thread_fn super_reflect_wait(bytes: u8[], save: bool) {
    defer { heap_program::finish(); }
    var code = source::Code {}; snapshot_test::append(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var job = source::Job { heap: heap, effects_enabled: true }; assert(source::start(job, source::closure(heap, 0, []), [source::builtin(heap, 8)]));
    assert(source::run(job, -1) == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8);
    if save { super_reflect_save(job, heap); return; }
    var waits: i64 = 1; var pcs: i64[>..] = [];
    loop {
        for f in job.frames { match f.native { Statics x => { if source::statics_receiver(x.which) { assert(f.input.is_object() && f.receiver.is_object()); pcs.push(f.pc); } }, _ => {} } }
        assert(source::deliver_effect(job, source::text(heap, "D")));
        let outcome = source::run(job, -1);
        if outcome == tiny::Outcome.Complete { break; }
        assert(outcome == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8); waits += 1;
    }
    assert(waits == 8 && pcs.len == 5 && pcs[0] == statics_pc::Key && pcs[1] == statics_pc::Result && pcs[2] == statics_pc::Stored && pcs[3] == statics_pc::Length && pcs[4] == statics_pc::Length);
    assert(source::key_is(heap, job.result, "gDkDsDKDgDsDlDlD:11,5,10,3,true,7,true,1,9"));
    assert(source::retire(job));
}
fn super_reflect_save(job: source::Job&, heap: source::Heap&) {
    let stack = source::suspend_stack(job, -1); assert(stack >= 0 && job.frames.len == 0 && job.operands.len == 0);
    assert(source::retire(job, true));
    let a = source::activation(heap, stack);
    var c = snapshot::Control {}; let image = snapshot::capture(heap, [], c); assert(c.error == snapshot::Error.None);
    var bad = copy(image); var homed = false;
    for n, row in image.nodes { if !homed && n.kind == node_kind::Closure && n.value.is_object() { bad.nodes[row].value = source::number(1); homed = true; } }
    assert(homed);
    var check = snapshot::Control {}; assert(!snapshot::validate(bad, check) && check.error == snapshot::Error.Link);
    var targetless = copy(image); var reflecting = false;
    for i in a.frame_count {
        let r = a.frames + i;
        match image.conts[r] {
            Frame x => {
                var f = x.f;
                match f.native { Statics s => { if source::statics_receiver(s.which) { f.input = source::Value.Undefined; reflecting = true; } }, _ => {} }
                targetless.conts[r] = source::Cont.Frame { f: f };
            },
            _ => {},
        }
    }
    if reflecting { var refused = snapshot::Control {}; assert(!snapshot::validate(targetless, refused) && refused.error == snapshot::Error.Link); }
    assert(qput(true));
    assert(qput(Waiting { image: copy(image), stack: stack as u32, steps: job.steps, kind: job.effect_kind, count: job.effect_count, frame: job.effect_frame, args: job.effect_args }));
}
thread_fn super_reflect_resumed(saved: Waiting) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; var r = snapshot::Control {};
    assert(snapshot::restore(saved.image, code, heap, bindings, source::reserve_identity(), r));
    var job = source::Job { heap: heap, effects_enabled: true };
    assert(source::resume_stack(job, saved.stack));
    job.steps = saved.steps; job.registered = true; heap.live_jobs += 1; job.effect_pending = true; job.effect_kind = saved.kind; job.effect_count = saved.count; job.effect_frame = saved.frame; for v, i in saved.args { job.effect_args[i] = v; }
    assert(source::deliver_effect(job, source::text(heap, "D")));
    let outcome = source::run(job, -1);
    if outcome == tiny::Outcome.Yielded { assert(job.effect_pending && job.effect_kind == 8); super_reflect_save(job, heap); return; }
    assert(outcome == tiny::Outcome.Complete && source::key_is(heap, job.result, "gDkDsDKDgDsDlDlD:11,5,10,3,true,7,true,1,9"));
    assert(source::retire(job));
    assert(qput(false));
}
// Run proxy-traps-wait.js (light-cljs-891) through its eight waits:
// answered in place, checking the method of the Proxy frame nearest the
// top at each (apply, construct, defineProperty above a [[Set]] at its
// receiver phase, isExtensible, getPrototypeOf above a HasInstance frame
// at pc 5, setPrototypeOf, a revocable proxy's get; then none, with the
// proxy revoked and its revoke function spent), or each after the
// stack was saved and restored in a fresh heap (proxy_traps_resumed), where
// proxy_traps_save checks that an image is refused when a saved
// [[Call]] or [[Construct]] frame holds no argument array, an
// [[IsExtensible]] or [[SetPrototypeOf]] frame would throw on a false
// answer, or a revoke function names no proxy.
let PROXY_TRAP_WAITS: i64[8] = [proxy_method::Call, proxy_method::Construct, proxy_method::DefineOwnProperty, proxy_method::IsExtensible, proxy_method::GetPrototypeOf, proxy_method::SetPrototypeOf, proxy_method::Get, -1];
thread_fn proxy_traps_wait(bytes: u8[], save: bool) {
    defer { heap_program::finish(); }
    var code = source::Code {}; snapshot_test::append(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var job = source::Job { heap: heap, effects_enabled: true }; assert(source::start(job, source::closure(heap, 0, []), [source::builtin(heap, 8)]));
    assert(source::run(job, -1) == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8);
    if save { proxy_traps_save(job, heap); return; }
    var waits: i64 = 0;
    loop {
        collect_at_wait(job, heap);
        var method: i64 = -1; var receiver_phase = false; var walking = false;
        for f in job.frames {
            match f.native {
                Proxy p => { method = p.method; if p.method == proxy_method::Set && f.pc == proxy_pc::ReceiverDefine { receiver_phase = true; } },
                HasInstance => { if f.pc == 5 { walking = true; } },
                _ => {},
            }
        }
        assert(waits < PROXY_TRAP_WAITS.len && method == PROXY_TRAP_WAITS[waits]);
        if method == proxy_method::DefineOwnProperty { assert(receiver_phase); }
        if method == proxy_method::GetPrototypeOf { assert(walking); }
        if method == -1 {
            var revoked: i64 = 0; var spent: i64 = 0;
            for n in heap.nodes {
                if n.kind == node_kind::Proxy && n.initialized { revoked += 1; }
                if n.kind == node_kind::Builtin && n.function == source::code(source::Builtin.ProxyRevoke) && n.value.is_null() { spent += 1; }
            }
            assert(revoked == 1 && spent == 1);
        }
        waits += 1;
        assert(source::deliver_effect(job, source::text(heap, "D")));
        let outcome = source::run(job, -1);
        if outcome == tiny::Outcome.Complete { break; }
        assert(outcome == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8);
    }
    assert(waits == 8 && source::key_is(heap, job.result, "aDcDdDeDgDsDrDvD:6,2,4,true,true,true,3,true,object,true,true"));
    assert(source::retire(job));
}
fn proxy_traps_save(job: source::Job&, heap: source::Heap&) {
    collect_at_wait(job, heap);
    let stack = source::suspend_stack(job, -1); assert(stack >= 0 && job.frames.len == 0 && job.operands.len == 0);
    assert(source::retire(job, true));
    let a = source::activation(heap, stack);
    var c = snapshot::Control {}; let image = snapshot::capture(heap, [], c); assert(c.error == snapshot::Error.None);
    // A revoke function naming no proxy.
    var bad = copy(image); var revoker = false;
    for n, row in image.nodes { if !revoker && n.kind == node_kind::Builtin && n.function == source::code(source::Builtin.ProxyRevoke) { bad.nodes[row].value = source::number(1); revoker = true; } }
    assert(revoker);
    var check = snapshot::Control {}; assert(!snapshot::validate(bad, check) && check.error == snapshot::Error.Link);
    // A saved [[Call]] or [[Construct]] frame without its argument array.
    var listless = copy(image); var calling = false;
    for i in a.frame_count {
        let r = a.frames + i;
        match image.conts[r] {
            Frame x => {
                var f = x.f;
                match f.native { Proxy p => { if p.method == proxy_method::Call || p.method == proxy_method::Construct { f.output = source::Value.Undefined; calling = true; } }, _ => {} }
                listless.conts[r] = source::Cont.Frame { f: f };
            },
            _ => {},
        }
    }
    if calling { var refused = snapshot::Control {}; assert(!snapshot::validate(listless, refused) && refused.error == snapshot::Error.Link); }
    // A saved [[IsExtensible]] or [[SetPrototypeOf]] frame that would throw
    // on a false answer, a mode neither method has (light-cljs-896).
    var throwing = copy(image); var answering = false;
    for i in a.frame_count {
        let r = a.frames + i;
        match image.conts[r] {
            Frame x => {
                var f = x.f;
                match f.native { Proxy p => { if p.method == proxy_method::IsExtensible || p.method == proxy_method::SetPrototypeOf { f.index = answer::Throws; answering = true; } }, _ => {} }
                throwing.conts[r] = source::Cont.Frame { f: f };
            },
            _ => {},
        }
    }
    if answering { var refused = snapshot::Control {}; assert(!snapshot::validate(throwing, refused) && refused.error == snapshot::Error.Link); }
    assert(qput(true));
    assert(qput(Waiting { image: copy(image), stack: stack as u32, steps: job.steps, kind: job.effect_kind, count: job.effect_count, frame: job.effect_frame, args: job.effect_args }));
}
thread_fn proxy_traps_resumed(saved: Waiting) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; var r = snapshot::Control {};
    assert(snapshot::restore(saved.image, code, heap, bindings, source::reserve_identity(), r));
    var job = source::Job { heap: heap, effects_enabled: true };
    assert(source::resume_stack(job, saved.stack));
    job.steps = saved.steps; job.registered = true; heap.live_jobs += 1; job.effect_pending = true; job.effect_kind = saved.kind; job.effect_count = saved.count; job.effect_frame = saved.frame; for v, i in saved.args { job.effect_args[i] = v; }
    assert(source::deliver_effect(job, source::text(heap, "D")));
    let outcome = source::run(job, -1);
    if outcome == tiny::Outcome.Yielded { assert(job.effect_pending && job.effect_kind == 8); proxy_traps_save(job, heap); return; }
    assert(outcome == tiny::Outcome.Complete && source::key_is(heap, job.result, "aDcDdDeDgDsDrDvD:6,2,4,true,true,true,3,true,object,true,true"));
    assert(source::retire(job));
    assert(qput(false));
}
// Run proxy-integrity-wait.js (light-cljs-897) through its nine waits:
// answered in place, checking at each the phase of the frame awaiting the
// trap (an integrity built-in's Statics frame: seal's [[PreventExtensions]],
// key list and [[DefineOwnProperty]], freeze's and isSealed's
// [[GetOwnProperty]], isFrozen's [[IsExtensible]]; then a [[OwnPropertyKeys]]
// Proxy frame awaiting its proxy target's [[IsExtensible]],
// [[OwnPropertyKeys]] and [[GetOwnProperty]]), or each after the stack was
// saved and restored in a fresh heap (proxy_integrity_resumed), where
// proxy_integrity_save checks that an image is refused when a saved
// integrity frame walking keys holds no key list, or a saved
// [[OwnPropertyKeys]] frame at a target phase lacks its trap's keys.
let INTEGRITY_WAITS: source::Builtin[6] = [source::Builtin.ObjectSeal, source::Builtin.ObjectSeal, source::Builtin.ObjectSeal, source::Builtin.ObjectFreeze, source::Builtin.ObjectIsSealed, source::Builtin.ObjectIsFrozen];
let INTEGRITY_PCS:   i64[9]             = [statics_pc::Prevented, statics_pc::List, statics_pc::Defined, statics_pc::Own, statics_pc::Own, statics_pc::Extensible, proxy_pc::TargetExtensible, proxy_pc::TargetKeys, proxy_pc::TargetKey];
let INTEGRITY_RESULT = "pDkDdDoDsDeDEDKDGD:true,true,true,false,x,true,writable/configurable";
thread_fn proxy_integrity_wait(bytes: u8[], save: bool) {
    defer { heap_program::finish(); }
    var code = source::Code {}; snapshot_test::append(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var job = source::Job { heap: heap, effects_enabled: true }; assert(source::start(job, source::closure(heap, 0, []), [source::builtin(heap, 8)]));
    assert(source::run(job, -1) == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8);
    if save { proxy_integrity_save(job, heap); return; }
    var waits: i64 = 0;
    loop {
        collect_at_wait(job, heap);
        assert(waits < INTEGRITY_PCS.len);
        var found: i64 = 0;
        for f in job.frames {
            match f.native {
                Statics x => { if source::integrity_walks(x.which) { assert(waits < INTEGRITY_WAITS.len && x.which == INTEGRITY_WAITS[waits] && f.pc == INTEGRITY_PCS[waits]); found += 1; } },
                Proxy p => { if p.method == proxy_method::OwnPropertyKeys && f.pc >= proxy_pc::TargetExtensible { assert(waits >= INTEGRITY_WAITS.len && f.pc == INTEGRITY_PCS[waits]); found += 1; } },
                _ => {},
            }
        }
        assert(found == 1);
        waits += 1;
        assert(source::deliver_effect(job, source::text(heap, "D")));
        let outcome = source::run(job, -1);
        if outcome == tiny::Outcome.Complete { break; }
        assert(outcome == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8);
    }
    assert(waits == 9 && source::key_is(heap, job.result, INTEGRITY_RESULT));
    assert(source::retire(job));
}
fn proxy_integrity_save(job: source::Job&, heap: source::Heap&) {
    collect_at_wait(job, heap);
    let stack = source::suspend_stack(job, -1); assert(stack >= 0 && job.frames.len == 0 && job.operands.len == 0);
    assert(source::retire(job, true));
    let a = source::activation(heap, stack);
    var c = snapshot::Control {}; let image = snapshot::capture(heap, [], c); assert(c.error == snapshot::Error.None);
    // An integrity frame at a key without its key list, or a target-phase
    // [[OwnPropertyKeys]] frame without its trap's keys.
    var bad = copy(image); var broken = false;
    for i in a.frame_count {
        let r = a.frames + i;
        match image.conts[r] {
            Frame x => {
                var f = x.f;
                match f.native {
                    Statics s => { if source::integrity_walks(s.which) && f.callee.is_object() { f.callee = source::Value.Undefined; broken = true; } },
                    Proxy p => { if p.method == proxy_method::OwnPropertyKeys && f.pc >= proxy_pc::TargetExtensible { f.callee = source::Value.Undefined; broken = true; } },
                    _ => {},
                }
                bad.conts[r] = source::Cont.Frame { f: f };
            },
            _ => {},
        }
    }
    if broken { var refused = snapshot::Control {}; assert(!snapshot::validate(bad, refused) && refused.error == snapshot::Error.Link); }
    assert(qput(true));
    assert(qput(Waiting { image: copy(image), stack: stack as u32, steps: job.steps, kind: job.effect_kind, count: job.effect_count, frame: job.effect_frame, args: job.effect_args }));
}
thread_fn proxy_integrity_resumed(saved: Waiting) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; var r = snapshot::Control {};
    assert(snapshot::restore(saved.image, code, heap, bindings, source::reserve_identity(), r));
    var job = source::Job { heap: heap, effects_enabled: true };
    assert(source::resume_stack(job, saved.stack));
    job.steps = saved.steps; job.registered = true; heap.live_jobs += 1; job.effect_pending = true; job.effect_kind = saved.kind; job.effect_count = saved.count; job.effect_frame = saved.frame; for v, i in saved.args { job.effect_args[i] = v; }
    assert(source::deliver_effect(job, source::text(heap, "D")));
    let outcome = source::run(job, -1);
    if outcome == tiny::Outcome.Yielded { assert(job.effect_pending && job.effect_kind == 8); proxy_integrity_save(job, heap); return; }
    assert(outcome == tiny::Outcome.Complete && source::key_is(heap, job.result, INTEGRITY_RESULT));
    assert(source::retire(job));
    assert(qput(false));
}
// Run proxy-copy-wait.js (light-cljs-895) through its nine waits: answered
// in place, checking at each the frame awaiting the trap (a CopyData frame
// at an object spread's Keys, Own and Value phases and at object rest's
// RestKeys and RestEnumerable; Object's and then Reflect's
// getOwnPropertyDescriptor Descriptor frame at pc 6; then a
// [[GetOwnProperty]] Proxy frame awaiting its proxy target's
// [[GetOwnProperty]] and [[IsExtensible]]), or each after the stack was
// saved and restored in a fresh heap (proxy_copy_resumed), where
// proxy_copy_save checks that an image is refused when that frame's proxy
// is a plain object, or a target-phase [[GetOwnProperty]] frame holding a
// trap result lacks the descriptor it builds.
let COPY_PCS:         i64[5]             = [copy_pc::Keys, copy_pc::Own, copy_pc::Value, copy_pc::RestKeys, copy_pc::RestEnumerable];
let COPY_DESCRIPTORS: source::Builtin[2] = [source::Builtin.ObjectGetOwnPropertyDescriptor, source::Builtin.ReflectGetOwnPropertyDescriptor];
let COPY_TARGET_PCS:  i64[2]             = [proxy_pc::Descriptor, proxy_pc::TargetExtensible];
let COPY_RESULT = "kDoDgDKDODdDrDiDeD:1,1,2,b,1,true,2,true";
thread_fn proxy_copy_wait(bytes: u8[], save: bool) {
    defer { heap_program::finish(); }
    var code = source::Code {}; snapshot_test::append(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var job = source::Job { heap: heap, effects_enabled: true }; assert(source::start(job, source::closure(heap, 0, []), [source::builtin(heap, 8)]));
    assert(source::run(job, -1) == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8);
    if save { proxy_copy_save(job, heap); return; }
    var waits: i64 = 0;
    loop {
        collect_at_wait(job, heap);
        assert(waits < 9);
        var copies: i64 = 0; var descriptors: i64 = 0; var targets: i64 = 0;
        for f in job.frames {
            match f.native {
                CopyData => { assert(waits < COPY_PCS.len && f.pc == COPY_PCS[waits]); copies += 1; },
                Descriptor d => { if f.pc == 6 { assert(waits >= COPY_PCS.len && d.which == COPY_DESCRIPTORS[if waits == COPY_PCS.len + 1 { 1 } else { 0 }]); descriptors += 1; } },
                Proxy p => { if p.method == proxy_method::GetOwnProperty && (f.pc == proxy_pc::Descriptor || f.pc == proxy_pc::TargetExtensible) { assert(waits >= COPY_PCS.len + 2 && f.pc == COPY_TARGET_PCS[waits - COPY_PCS.len - 2]); targets += 1; } },
                _ => {},
            }
        }
        assert(copies == (if waits < COPY_PCS.len { 1 } else { 0 }) && descriptors == (if waits < COPY_PCS.len { 0 } else { 1 }) && targets == (if waits < COPY_PCS.len + 2 { 0 } else { 1 }));
        waits += 1;
        assert(source::deliver_effect(job, source::text(heap, "D")));
        let outcome = source::run(job, -1);
        if outcome == tiny::Outcome.Complete { break; }
        assert(outcome == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8);
    }
    assert(waits == 9 && source::key_is(heap, job.result, COPY_RESULT));
    assert(source::retire(job));
}
fn proxy_copy_save(job: source::Job&, heap: source::Heap&) {
    collect_at_wait(job, heap);
    let stack = source::suspend_stack(job, -1); assert(stack >= 0 && job.frames.len == 0 && job.operands.len == 0);
    assert(source::retire(job, true));
    let a = source::activation(heap, stack);
    var c = snapshot::Control {}; let image = snapshot::capture(heap, [], c); assert(c.error == snapshot::Error.None);
    // A plain object of the image.
    var plain: i64 = -1;
    for n, row in image.nodes { if plain < 0 && n.kind == node_kind::Object { plain = row; } }
    assert(plain >= 0);
    // A CopyData frame at a proxy phase or a getOwnPropertyDescriptor
    // frame whose proxy is a plain object, or a target-phase
    // [[GetOwnProperty]] frame at pc 12 holding a trap result without its
    // descriptor.
    var bad = copy(image); var broken = false;
    for i in a.frame_count {
        let r = a.frames + i;
        match image.conts[r] {
            Frame x => {
                var f = x.f;
                match f.native {
                    CopyData => { if f.pc >= copy_pc::Keys { f.input = source::object(plain); broken = true; } },
                    Descriptor => { if f.pc == 6 { f.input = source::object(plain); broken = true; } },
                    Proxy p => { if p.method == proxy_method::GetOwnProperty && f.pc == proxy_pc::TargetExtensible && f.receiver.is_object() { f.callee = source::Value.Undefined; broken = true; } },
                    _ => {},
                }
                bad.conts[r] = source::Cont.Frame { f: f };
            },
            _ => {},
        }
    }
    if broken { var refused = snapshot::Control {}; assert(!snapshot::validate(bad, refused) && refused.error == snapshot::Error.Link); }
    assert(qput(true));
    assert(qput(Waiting { image: copy(image), stack: stack as u32, steps: job.steps, kind: job.effect_kind, count: job.effect_count, frame: job.effect_frame, args: job.effect_args }));
}
thread_fn proxy_copy_resumed(saved: Waiting) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; var r = snapshot::Control {};
    assert(snapshot::restore(saved.image, code, heap, bindings, source::reserve_identity(), r));
    var job = source::Job { heap: heap, effects_enabled: true };
    assert(source::resume_stack(job, saved.stack));
    job.steps = saved.steps; job.registered = true; heap.live_jobs += 1; job.effect_pending = true; job.effect_kind = saved.kind; job.effect_count = saved.count; job.effect_frame = saved.frame; for v, i in saved.args { job.effect_args[i] = v; }
    assert(source::deliver_effect(job, source::text(heap, "D")));
    let outcome = source::run(job, -1);
    if outcome == tiny::Outcome.Yielded { assert(job.effect_pending && job.effect_kind == 8); proxy_copy_save(job, heap); return; }
    assert(outcome == tiny::Outcome.Complete && source::key_is(heap, job.result, COPY_RESULT));
    assert(source::retire(job));
    assert(qput(false));
}
// Run proxy-property-wait.js (light-cljs-896) through the sixteen waits in
// the phases of light-cljs-890's [[Get]], [[Set]], [[HasProperty]] and
// [[Delete]] Proxy frames: a forced collection at each wait, the frame's
// state checked after it (proxy_property_frame), then answered in place;
// or at each wait collected, the stack saved and restored in a fresh heap
// (proxy_property_resumed), where proxy_property_save checks that an image
// is refused when such a frame's key is no property key, its phase is not
// one of its method's, or its answer mode is not one its method has. The
// result string counts every trap, getter and setter that ran.
let PROPERTY_METHODS: i64[16] = [
    proxy_method::Get, proxy_method::Get, proxy_method::HasProperty, proxy_method::Get, proxy_method::Set, proxy_method::Set, proxy_method::Set, proxy_method::Delete,
    proxy_method::Delete, proxy_method::Get, proxy_method::Set, proxy_method::HasProperty, proxy_method::Delete, proxy_method::Get, proxy_method::Set, proxy_method::Set,
];
// The handler getter's phase is 1 and the trap call's 2 (proxy_step).
let PROPERTY_PCS:   i64[16] = [1, 2, 2, proxy_pc::Accessor, proxy_pc::Accessor, 2, 2, 2, 2, proxy_pc::Descriptor, proxy_pc::Descriptor, proxy_pc::TargetExtensible, proxy_pc::TargetExtensible, 2, proxy_pc::Length, proxy_pc::Length];
let PROPERTY_MODES: i64[16] = [answer::Boolean, answer::Boolean, answer::Boolean, answer::Boolean, answer::Throws, answer::Throws, answer::Boolean, answer::Throws, answer::Boolean, answer::Boolean, answer::Boolean, answer::Boolean, answer::Throws, answer::Boolean, answer::Boolean, answer::Boolean];
let PROPERTY_KEYS: source::Name[16] = [
    source::Name { text: "x" }, source::Name { text: "x" }, source::Name { text: "virtual" }, source::Name { text: "y" }, source::Name { text: "z" }, source::Name { text: "w" },
    source::Name { text: "w" }, source::Name { text: "q" }, source::Name { text: "q" }, source::Name { text: "f" }, source::Name { text: "f" }, source::Name { text: "e" },
    source::Name { text: "e" }, source::Name { text: "k" }, source::Name { text: "length" }, source::Name { text: "length" },
];
// The receiver's kind (node_kind::None for undefined) and a [[Set]]'s
// value (-1 for an object).
let PROPERTY_RECEIVERS: i64[16] = [
    node_kind::Proxy, node_kind::Proxy, node_kind::None, node_kind::Object, node_kind::Object, node_kind::Object, node_kind::Object, node_kind::None,
    node_kind::None, node_kind::Proxy, node_kind::Proxy, node_kind::None, node_kind::None, node_kind::Proxy, node_kind::Array, node_kind::Array,
];
let PROPERTY_VALUES: f64[16] = [0, 0, 0, 0, 5, 7, 7, 0, 0, 0, 3, 0, 0, 0, -1, -1];
let PROPERTY_RESULT = "gDtDhDGDZDsDSDxDXDiDIDeDEDrDnDnD:2,true,true,zz,5,true,true,false,true,false,true,false,3,true,true,true,true,true,1";
inline fn property_method(m: i64) -> bool { return m == proxy_method::Get || m == proxy_method::Set || m == proxy_method::HasProperty || m == proxy_method::Delete; }
// The lowest [[Get]], [[Set]], [[HasProperty]] or [[Delete]] Proxy frame at
// wait w holds its method, phase, answer mode, key, receiver and value,
// and only the restarted [[Get]] (wait 14) counts a hop.
fn proxy_property_frame(job: const source::Job&, heap: source::Heap&, w: i64) {
    assert(w < PROPERTY_METHODS.len);
    for f in job.frames {
        match f.native {
            Proxy p => {
                if property_method(p.method) {
                    assert(p.method == PROPERTY_METHODS[w] && f.pc == PROPERTY_PCS[w] && f.index == PROPERTY_MODES[w] && f.argument_count == (if w == 13 { 1 } else { 0 }));
                    assert(source::key_is(heap, f.output, PROPERTY_KEYS[w].text));
                    let kind = PROPERTY_RECEIVERS[w];
                    if kind == node_kind::None { assert(f.receiver.is_undefined()); } else { assert(f.receiver.is_object() && heap.nodes[f.receiver.id()].kind == kind); }
                    if p.method == proxy_method::Set { if PROPERTY_VALUES[w] < 0 { assert(f.callee.is_object()); } else { assert(f.callee.is_number() && source::num(f.callee) == PROPERTY_VALUES[w]); } }
                    return;
                }
            },
            _ => {},
        }
    }
    abort("no property Proxy frame");
}
// A forced collection at a wait (reclaim.goose), which moves every row the
// suspended frames hold. The Proxy wait tests (light-cljs-891, -895, -897,
// -898 and -890's here) collect at each wait answered in place, before
// checking its frames, and before saving the stack (light-cljs-896).
fn collect_at_wait(job: source::Job&, heap: source::Heap&) {
    let before = heap.reclaims;
    heap.reclaim = true; source::reclaim(job);
    assert(heap.reclaims == before + 1 && !heap.reclaim);
}
thread_fn proxy_property_wait(bytes: u8[], save: bool) {
    defer { heap_program::finish(); }
    var code = source::Code {}; snapshot_test::append(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var job = source::Job { heap: heap, effects_enabled: true }; assert(source::start(job, source::closure(heap, 0, []), [source::builtin(heap, 8)]));
    assert(source::run(job, -1) == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8);
    if save { proxy_property_save(job, heap, 0); return; }
    var waits: i64 = 0;
    loop {
        collect_at_wait(job, heap);
        proxy_property_frame(job, heap, waits);
        waits += 1;
        assert(source::deliver_effect(job, source::text(heap, "D")));
        let outcome = source::run(job, -1);
        if outcome == tiny::Outcome.Complete { break; }
        assert(outcome == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8);
    }
    assert(waits == 16 && source::key_is(heap, job.result, PROPERTY_RESULT));
    assert(source::retire(job));
}
fn proxy_property_save(job: source::Job&, heap: source::Heap&, w: i64) {
    collect_at_wait(job, heap);
    proxy_property_frame(job, heap, w);
    let stack = source::suspend_stack(job, -1); assert(stack >= 0 && job.frames.len == 0 && job.operands.len == 0);
    assert(source::retire(job, true));
    let a = source::activation(heap, stack);
    var c = snapshot::Control {}; let image = snapshot::capture(heap, [], c); assert(c.error == snapshot::Error.None);
    // The property frame with a number for its key, a phase of another
    // method's (a forwarded accessor's for [[HasProperty]] and [[Delete]],
    // the target's extensibility for [[Get]] and [[Set]]), or an answer
    // mode its method lacks (a throwing [[Get]] or [[HasProperty]], an
    // object-giving [[Set]] or [[Delete]]): each refused alone.
    for mode in 3 {
        var bad = copy(image); var broken = false;
        for i in a.frame_count {
            let r = a.frames + i;
            match image.conts[r] {
                Frame x => {
                    var f = x.f;
                    match f.native {
                        Proxy p => {
                            if !broken && property_method(p.method) {
                                let reads = p.method == proxy_method::Get || p.method == proxy_method::Set;
                                let answers = p.method == proxy_method::Set || p.method == proxy_method::Delete;
                                if mode == 0 { f.output = source::number(1); }
                                else if mode == 1 { f.pc = if reads { proxy_pc::TargetExtensible } else { proxy_pc::Accessor }; }
                                else { f.index = if answers { answer::Object } else { answer::Throws }; }
                                broken = true;
                            }
                        },
                        _ => {},
                    }
                    bad.conts[r] = source::Cont.Frame { f: f };
                },
                _ => {},
            }
        }
        assert(broken);
        var refused = snapshot::Control {}; assert(!snapshot::validate(bad, refused) && refused.error == snapshot::Error.Link);
    }
    assert(qput(true));
    assert(qput(Waiting { image: copy(image), stack: stack as u32, steps: job.steps, kind: job.effect_kind, count: job.effect_count, frame: job.effect_frame, args: job.effect_args, wait: w }));
}
thread_fn proxy_property_resumed(saved: Waiting) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; var r = snapshot::Control {};
    assert(snapshot::restore(saved.image, code, heap, bindings, source::reserve_identity(), r));
    var job = source::Job { heap: heap, effects_enabled: true };
    assert(source::resume_stack(job, saved.stack));
    job.steps = saved.steps; job.registered = true; heap.live_jobs += 1; job.effect_pending = true; job.effect_kind = saved.kind; job.effect_count = saved.count; job.effect_frame = saved.frame; for v, i in saved.args { job.effect_args[i] = v; }
    // The restored frame is the one saved.
    proxy_property_frame(job, heap, saved.wait);
    assert(source::deliver_effect(job, source::text(heap, "D")));
    let outcome = source::run(job, -1);
    if outcome == tiny::Outcome.Yielded { assert(job.effect_pending && job.effect_kind == 8); proxy_property_save(job, heap, saved.wait + 1); return; }
    assert(outcome == tiny::Outcome.Complete && source::key_is(heap, job.result, PROPERTY_RESULT));
    assert(source::retire(job));
    assert(qput(false));
}
// Run array-length-definition-wait.js (light-cljs-898) through its eight
// waits: answered in place, checking at each the frame awaiting both
// conversions (Object.defineProperty's and then Object.defineProperties's
// Descriptor frame at pc 5, twice each; then a [[DefineOwnProperty]] Proxy
// frame at pc Length, twice under Reflect.defineProperty's Descriptor
// frame and twice under a [[Set]] frame at its receiver phase), or each
// after the stack was saved and restored in a fresh heap
// (length_definition_resumed), where length_definition_save checks that an
// image is refused when that frame defines on no array: a Descriptor
// frame's target is another object or its defineProperties cursor lies past
// the keys, or a Proxy frame's proxy is one of a plain object.
let LENGTH_WAITS: source::Builtin[4] = [source::Builtin.ObjectDefineProperty, source::Builtin.ObjectDefineProperty, source::Builtin.ObjectDefineProperties, source::Builtin.ObjectDefineProperties];
let LENGTH_RESULT = "aDaDbDbDcDcDdDdD:1,2,4,1,true,0,true,object";
thread_fn length_definition_wait(bytes: u8[], save: bool) {
    defer { heap_program::finish(); }
    var code = source::Code {}; snapshot_test::append(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var job = source::Job { heap: heap, effects_enabled: true }; assert(source::start(job, source::closure(heap, 0, []), [source::builtin(heap, 8)]));
    assert(source::run(job, -1) == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8);
    if save { length_definition_save(job, heap); return; }
    var waits: i64 = 0;
    loop {
        collect_at_wait(job, heap);
        var found: i64 = 0; var receiver_phase = false; var converting = false;
        for f in job.frames {
            match f.native {
                Descriptor d => { if f.pc == 5 { assert(waits < LENGTH_WAITS.len && d.which == LENGTH_WAITS[waits]); found += 1; } },
                Proxy p => {
                    if p.method == proxy_method::DefineOwnProperty && f.pc == proxy_pc::Length { assert(waits >= LENGTH_WAITS.len); found += 1; }
                    if p.method == proxy_method::Set && f.pc == proxy_pc::ReceiverDefine { receiver_phase = true; }
                },
                Convert => { converting = true; },
                _ => {},
            }
        }
        assert(found == 1 && converting && receiver_phase == (waits >= 6));
        waits += 1;
        assert(source::deliver_effect(job, source::text(heap, "D")));
        let outcome = source::run(job, -1);
        if outcome == tiny::Outcome.Complete { break; }
        assert(outcome == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8);
    }
    assert(waits == 8 && source::key_is(heap, job.result, LENGTH_RESULT));
    assert(source::retire(job));
}
fn length_definition_save(job: source::Job&, heap: source::Heap&) {
    collect_at_wait(job, heap);
    let stack = source::suspend_stack(job, -1); assert(stack >= 0 && job.frames.len == 0 && job.operands.len == 0);
    assert(source::retire(job, true));
    let a = source::activation(heap, stack);
    var c = snapshot::Control {}; let image = snapshot::capture(heap, [], c); assert(c.error == snapshot::Error.None);
    // The fixture's proxy of a plain object.
    var plain: i64 = -1;
    for n, row in image.nodes { if plain < 0 && n.kind == node_kind::Proxy && n.value.is_object() && image.nodes[n.value.id()].kind == node_kind::Object { plain = row; } }
    assert(plain >= 0);
    var refusals: i64 = 0;
    for mode in 3 {
        var bad = copy(image); var broken = false;
        for i in a.frame_count {
            let r = a.frames + i;
            match image.conts[r] {
                Frame x => {
                    var f = x.f;
                    match f.native {
                        Descriptor d => {
                            if f.pc == 5 && mode == 0 { f.input = source::object(plain); broken = true; }
                            if f.pc == 5 && mode == 1 && d.which == source::Builtin.ObjectDefineProperties { f.index = f.argument_count; broken = true; }
                        },
                        Proxy p => { if p.method == proxy_method::DefineOwnProperty && f.pc == proxy_pc::Length && mode == 2 { f.input = source::object(plain); broken = true; } },
                        _ => {},
                    }
                    bad.conts[r] = source::Cont.Frame { f: f };
                },
                _ => {},
            }
        }
        if broken { var refused = snapshot::Control {}; assert(!snapshot::validate(bad, refused) && refused.error == snapshot::Error.Link); refusals += 1; }
    }
    assert(refusals > 0);
    assert(qput(true));
    assert(qput(Waiting { image: copy(image), stack: stack as u32, steps: job.steps, kind: job.effect_kind, count: job.effect_count, frame: job.effect_frame, args: job.effect_args }));
}
thread_fn length_definition_resumed(saved: Waiting) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; var r = snapshot::Control {};
    assert(snapshot::restore(saved.image, code, heap, bindings, source::reserve_identity(), r));
    var job = source::Job { heap: heap, effects_enabled: true };
    assert(source::resume_stack(job, saved.stack));
    job.steps = saved.steps; job.registered = true; heap.live_jobs += 1; job.effect_pending = true; job.effect_kind = saved.kind; job.effect_count = saved.count; job.effect_frame = saved.frame; for v, i in saved.args { job.effect_args[i] = v; }
    assert(source::deliver_effect(job, source::text(heap, "D")));
    let outcome = source::run(job, -1);
    if outcome == tiny::Outcome.Yielded { assert(job.effect_pending && job.effect_kind == 8); length_definition_save(job, heap); return; }
    assert(outcome == tiny::Outcome.Complete && source::key_is(heap, job.result, LENGTH_RESULT));
    assert(source::retire(job));
    assert(qput(false));
}
// Run private-elements-wait.js (light-cljs-889) through its four waits:
// answered in place, checking that only the setter's wait (the second)
// runs under a Setter native frame, or each after the stack was saved and
// restored in a fresh heap (private_resumed), where private_save checks that
// an image is refused when an object's private elements lose their owner,
// gain a second owner or key a record by a well-known symbol.
thread_fn private_wait(bytes: u8[], save: bool) {
    defer { heap_program::finish(); }
    var code = source::Code {}; snapshot_test::append(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var job = source::Job { heap: heap, effects_enabled: true }; assert(source::start(job, source::closure(heap, 0, []), [source::builtin(heap, 8)]));
    assert(source::run(job, -1) == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8);
    if save { private_save(job, heap); return; }
    var waits: i64 = 0; var setters: i64[>..] = [];
    loop {
        for f in job.frames { match f.native { Setter => { setters.push(waits); }, _ => {} } }
        assert(source::deliver_effect(job, source::text(heap, "D"))); waits += 1;
        let outcome = source::run(job, -1);
        if outcome == tiny::Outcome.Complete { break; }
        assert(outcome == tiny::Outcome.Yielded && job.effect_pending && job.effect_kind == 8);
    }
    assert(waits == 4 && setters.len == 1 && setters[0] == 1);
    assert(source::key_is(heap, job.result, "gDsDmDtD:1,4,5,5,true,true,true,true,false"));
    assert(source::retire(job));
}
fn private_save(job: source::Job&, heap: source::Heap&) {
    let stack = source::suspend_stack(job, -1); assert(stack >= 0 && job.frames.len == 0 && job.operands.len == 0);
    assert(source::retire(job, true));
    var c = snapshot::Control {}; let image = snapshot::capture(heap, [], c); assert(c.error == snapshot::Error.None);
    // The first object with private elements, its row and theirs, and an
    // earlier ordinary object without any.
    var owner: i64 = -1; var elements: i64 = -1; var other: i64 = -1;
    for n, row in image.nodes {
        if owner < 0 && n.metadata.is_object() && image.nodes[n.metadata.id()].kind == node_kind::PrivateElements { owner = row; elements = n.metadata.id(); }
    }
    for n, row in image.nodes { if other < 0 && row != owner && row < elements && n.kind == node_kind::Object && n.metadata.is_undefined() { other = row; } }
    assert(owner >= 0 && other >= 0 && image.nodes[elements].key_count > 0);
    var orphaned = copy(image); orphaned.nodes[owner].metadata = source::Value.Undefined;
    var check = snapshot::Control {}; assert(!snapshot::validate(orphaned, check) && check.error == snapshot::Error.Link);
    var shared = copy(image); shared.nodes[other].metadata = image.nodes[owner].metadata;
    var check2 = snapshot::Control {}; assert(!snapshot::validate(shared, check2) && check2.error == snapshot::Error.Link);
    var public = copy(image); public.properties[image.nodes[elements].keys].key = source::Value.Symbol { s: well_known::Iterator as u32 };
    var check3 = snapshot::Control {}; assert(!snapshot::validate(public, check3) && check3.error == snapshot::Error.Link);
    // The owner bitmap (one byte a node row) obeys its byte and work cuts
    // at their exact boundaries.
    var rows: i64 = 0; for n in image.nodes { if n.kind == node_kind::PrivateElements { rows += 1; } }
    var tight = snapshot::Control { bytes: snapshot::footprint(image) + image.nodes.len - 1 };
    assert(!snapshot::private_owners(image, tight, rows, rows) && tight.error == snapshot::Error.Bytes && tight.visited == 0 && tight.validation_peak == 0);
    var exact = snapshot::Control { bytes: snapshot::footprint(image) + image.nodes.len };
    assert(snapshot::private_owners(image, exact, rows, rows) && exact.validation_peak == image.nodes.len);
    var at_work = snapshot::Control { work: exact.visited }; assert(snapshot::private_owners(image, at_work, rows, rows));
    var below = snapshot::Control { work: exact.visited - 1 }; assert(!snapshot::private_owners(image, below, rows, rows) && below.error == snapshot::Error.Work);
    var miscounted = snapshot::Control {}; assert(!snapshot::private_owners(image, miscounted, rows, rows + 1) && miscounted.error == snapshot::Error.Link);
    assert(qput(true));
    assert(qput(Waiting { image: copy(image), stack: stack as u32, steps: job.steps, kind: job.effect_kind, count: job.effect_count, frame: job.effect_frame, args: job.effect_args }));
}
thread_fn private_resumed(saved: Waiting) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; var r = snapshot::Control {};
    assert(snapshot::restore(saved.image, code, heap, bindings, source::reserve_identity(), r));
    var job = source::Job { heap: heap, effects_enabled: true };
    assert(source::resume_stack(job, saved.stack));
    job.steps = saved.steps; job.registered = true; heap.live_jobs += 1; job.effect_pending = true; job.effect_kind = saved.kind; job.effect_count = saved.count; job.effect_frame = saved.frame; for v, i in saved.args { job.effect_args[i] = v; }
    assert(source::deliver_effect(job, source::text(heap, "D")));
    let outcome = source::run(job, -1);
    if outcome == tiny::Outcome.Yielded { assert(job.effect_pending && job.effect_kind == 8); private_save(job, heap); return; }
    assert(outcome == tiny::Outcome.Complete && source::key_is(heap, job.result, "gDsDmDtD:1,4,5,5,true,true,true,true,false"));
    assert(source::retire(job));
    assert(qput(false));
}
// A reused worker (light-cljs-795) runs symbols-wait.js twice: each job
// registers symbols, the reset restores the image's empty registry with
// its tables, and the second job registers afresh at the same rows.
thread_fn symbols_reused(bytes: u8[]) {
    defer { heap_program::finish(); }
    var pc = source::Code {}; snapshot_test::append(pc, bytes);
    var parent = source::Heap { code: pc }; assert(source::initialize(parent));
    var c = snapshot::Control {}; let base = snapshot::capture(parent, [], c); assert(c.error == snapshot::Error.None && base.header.registry.is_null());
    let identity = source::reserve_identity(); let rows: i64[0] = [];
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; var r = snapshot::Control {};
    assert(snapshot::restore(base, code, heap, bindings, identity, r));
    var first = source::Value.Null;
    for round in 2 {
        var job = source::Job { heap: heap, effects_enabled: true }; assert(source::start(job, source::closure(heap, 0, []), [source::builtin(heap, 8)]));
        assert(source::run(job, -1) == tiny::Outcome.Yielded && source::deliver_effect(job, source::text(heap, "D")) && source::run(job, -1) == tiny::Outcome.Complete && source::key_is(heap, job.result, "D:true"));
        assert(source::registered_symbol(heap, heap.registry));
        if round == 0 { first = heap.registry; } else { assert(source::same_bits(heap.registry, first)); }
        assert(source::retire(job));
        assert(!snapshot::pristine(base, identity, code, heap, bindings));
        assert(snapshot::truncate(base, rows, identity, code, heap, bindings) && snapshot::pristine(base, identity, code, heap, bindings) && heap.registry.is_null());
    }
}
thread_fn symbols_resumed(saved: Waiting) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; var r = snapshot::Control {};
    assert(snapshot::restore(saved.image, code, heap, bindings, source::reserve_identity(), r));
    assert(source::same_bits(heap.registry, saved.image.header.registry));
    var job = source::Job { heap: heap, effects_enabled: true };
    assert(source::resume_stack(job, saved.stack));
    job.steps = saved.steps; job.registered = true; heap.live_jobs += 1; job.effect_pending = true; job.effect_kind = saved.kind; job.effect_count = saved.count; job.effect_frame = saved.frame; for v, i in saved.args { job.effect_args[i] = v; }
    assert(source::deliver_effect(job, source::text(heap, "D")));
    assert(source::run(job, -1) == tiny::Outcome.Complete && source::key_is(heap, job.result, "D:true"));
    assert(source::retire(job));
}
thread_fn builtin_iterator_resumed(saved: Waiting) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; var r = snapshot::Control {};
    assert(snapshot::restore(saved.image, code, heap, bindings, source::reserve_identity(), r));
    var job = source::Job { heap: heap, effects_enabled: true };
    assert(source::resume_stack(job, saved.stack));
    job.steps = saved.steps; job.registered = true; heap.live_jobs += 1; job.effect_pending = true; job.effect_kind = saved.kind; job.effect_count = saved.count; job.effect_frame = saved.frame; for v, i in saved.args { job.effect_args[i] = v; }
    assert(source::deliver_effect(job, source::text(heap, "D")));
    let outcome = source::run(job, -1);
    if outcome == tiny::Outcome.Yielded { assert(job.effect_pending && job.effect_kind == 8); for_of_save(job, heap); return; }
    assert(outcome == tiny::Outcome.Complete && source::key_is(heap, job.result, "agDbc1wD23xsDy"));
    assert(source::retire(job));
    assert(qput(false));
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
// Run-time code in a reused worker (light-cljs-856; decision 3): a job's
// indirect eval appends its compiled functions to the worker's code above
// the image, a captured snapshot carries them (with the closure over them)
// to another program, where the closure runs, and the worker's reset cuts
// them away: the tables are pristine, and the next job compiles the same
// code again into the same rows a fresh worker would use. The direct eval
// variant (direct-eval-closure.js, light-cljs-858) does the same with the
// image's eval site and the one its eval code adds (`sites`).
fn runtime_code() {
    for direct in [false, true] {
        let bytes = if direct { embed_file("../corpus/serial/direct-eval-closure.gsp") } else { embed_file("../corpus/serial/eval-closure.gsp") };
        let sites: i64 = if direct { 1 } else { 0 };
        heap_program::join(thread_spawn(suspension_parent, str(bytes))); let base = qget<snapshot::Snapshot>();
        assert(base.sites.len == sites && base.header.ends[21] == sites);
        heap_program::join(thread_spawn(runtime_code_worker, copy(base), sites)); let saved = qget<Saved>();
        heap_program::join(thread_spawn(runtime_code_carried, copy(saved.image), copy(base)));
    }
    print("snapshot runtime code: eval code, indirect or direct, joins a reused worker's code above its image with its eval sites, travels in a snapshot and leaves with the reset");
}
thread_fn runtime_code_worker(base: snapshot::Snapshot, sites: i64) {
    defer { heap_program::finish(); }
    let identity = source::reserve_identity(); let rows: i64[] = [];
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; var r = snapshot::Control {};
    assert(snapshot::restore(base, code, heap, bindings, identity, r) && snapshot::truncate(base, rows, identity, code, heap, bindings));
    var first_row: i64 = -1; var first_function: i64 = -1;
    for run in 2 {
        // The worker's write boundary: the job owns only rows it allocates.
        var job = source::Job { heap: heap, owned_nodes: heap.nodes.len, writable_code: code };
        assert(source::start(job, bindings[0].value, []) && source::run(job, -1) == tiny::Outcome.Complete);
        let bump = job.result;
        assert(bump.is_object() && source::node(heap, bump.id()).kind == node_kind::Closure);
        // Its function is a job row above the image's, the root of the
        // eval code below it, which is marked as eval code.
        let f = source::node(heap, bump.id()).function as i64;
        assert(f > base.functions.len && code.functions.len > base.functions.len && code.ops.len > base.ops.len);
        assert(code.functions[base.functions.len].eval && !code.functions[f].eval && job.compiled_ops == code.ops.len - base.ops.len);
        // The outer eval code's own direct-eval site joined the image's.
        assert(code.sites.len == 2 * sites && (sites == 0 || code.sites[1].op as i64 >= base.ops.len));
        assert(source::retire(job));
        if run == 0 {
            first_row = bump.id(); first_function = f;
            var c = snapshot::Control {}; let image = snapshot::capture(heap, [snapshot::Binding { id: 1, generation: 1, value: bump }], c);
            assert(c.error == snapshot::Error.None && image.functions.len == code.functions.len && image.sites.len == code.sites.len);
            assert(qput(Saved { image: copy(image) }));
        } else {
            // The next job's tables were a fresh worker's: the same rows.
            assert(bump.id() == first_row && f == first_function);
        }
        assert(snapshot::truncate(base, rows, identity, code, heap, bindings) && snapshot::pristine(base, identity, code, heap, bindings));
        assert(code.functions.len == base.functions.len && code.ops.len == base.ops.len && code.ins.len == base.ops.len && heap.constants.len == base.code_constants.len);
        assert(code.sites.len == sites && code.site_records.len == base.site_records.len);
    }
}
// The captured snapshot restores in another program with the eval code in
// its code tables, and the closure runs there: its counter, the eval
// code's `let` in a binding cell (or the caller's, read through the nested
// direct eval, which compiles here), is 1 from the first call and becomes 2.
thread_fn runtime_code_carried(image: snapshot::Snapshot, base: snapshot::Snapshot) {
    defer { heap_program::finish(); }
    // Snapshot validation checks the eval sites as an image load does: a
    // site on an unmarked call, a local past its function's, a second
    // receiver, an empty record and a site table its header does not count
    // are refused.
    for mode in (if image.sites.len > 0 { 5 } else { 0 }) {
        var bad = copy(image); let first = bad.sites[0].sources;
        match mode {
            0 => { bad.sites[0].op -= 1; },
            1 => { var at: i64 = -1; for x, i in bad.site_sources { if x.kind as i64 == eval_source::Local { at = i; } } assert(at >= 0); bad.site_sources[at].index = 65535; },
            2 => { for i in bad.sites[0].count { bad.site_sources[first + i].role = eval_role::Receiver as u8; } },
            3 => { bad.sites[0].length = 0; },
            _ => { bad.header.ends[21] += 1; },
        }
        var c = snapshot::Control {}; assert(!snapshot::validate(bad, c) && c.error == (if mode == 4 { snapshot::Error.Length } else { snapshot::Error.Code }));
    }
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; var r = snapshot::Control {};
    assert(snapshot::restore(image, code, heap, bindings, source::reserve_identity(), r));
    assert(code.functions.len > base.functions.len && code.functions[base.functions.len].eval && code.sites.len == image.sites.len);
    var job = source::Job { heap: heap, writable_code: code };
    assert(source::start(job, bindings[0].value, []) && source::run(job, -1) == tiny::Outcome.Complete);
    assert(job.result.is_number() && job.result.num() == 2.0);
    // The indirect eval's closure keeps its source text's lone surrogate
    // (light-cljs-864): the carried code bytes passed validation as WTF-8.
    if image.sites.len == 0 {
        let text = source::function_text(job, bindings[0].value); assert(text.is_string());
        var lone = false; for u in source::utf16(heap, text) { if u == 55296 { lone = true; } }
        assert(lone);
    }
    assert(source::retire(job));
}
// A closure compiled by eval leaves its job as a callable publication
// (light-cljs-863: a helper initializer may compile, and the helper it
// produces outlives the job). The producer, a reused worker over the
// calls.gsp image, runs an indirect eval whose script makes a closure over
// its own immutable bindings; the publication's code suffix carries the
// eval code. The producer's reset leaves its tables
// pristine. A consumer that already appended unrelated code installs the
// closure, its eval code relocated behind that code, and calls it.
fn runtime_eval_callable_publication() {
    heap_program::join(thread_spawn(callable_base)); let saved = qget<Saved>(); let base .= saved.image;
    heap_program::join(thread_spawn(runtime_eval_producer, copy(base))); let p = qget<Published>();
    heap_program::join(thread_spawn(runtime_eval_consumer, copy(base), copy(p.wire)));
    print("snapshot runtime eval publication: a closure eval compiled travels with its code suffix to a consumer with code of its own, and the producer's reset is pristine");
}
thread_fn runtime_eval_producer(base: snapshot::Snapshot) {
    defer { heap_program::finish(); }
    let identity = source::reserve_identity(); let rows: i64[] = [];
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; var r = snapshot::Control {};
    assert(snapshot::restore(base, code, heap, bindings, identity, r) && snapshot::truncate(base, rows, identity, code, heap, bindings));
    // What the global eval does with a string (start_indirect_eval): compile
    // it as an indirect eval's script into the job's code, then run its root
    // with the global object as `this`.
    var job = source::Job { heap: heap, owned_nodes: heap.nodes.len, writable_code: code };
    let units = source::utf16(heap, source::text(heap, "const k = 41; const get = function () { return k; }; (function plus(x) { return get() + x; })"));
    let root = source::compile_code(job, "eval", units, runtime_mode::IndirectEval, false, 0); assert(root == base.functions.len);
    assert(source::start(job, source::closure(heap, root, []), [], heap.global) && source::run(job, -1) == tiny::Outcome.Complete);
    let plus = job.result; assert(source::retire(job));
    assert(plus.is_object() && source::node(heap, plus.id()).kind == node_kind::Closure);
    let f = source::node(heap, plus.id()).function as i64;
    assert(f > base.functions.len && code.functions[base.functions.len].eval && !code.functions[f].eval);
    // The closure captures another closure of the eval code, `get`, which
    // captures the script's `const` (immutable once initialized, so the
    // compiler captures its value): the publication carries both closures.
    let n = source::node(heap, plus.id()); var inner = false;
    for i in n.count {
        let v = heap.values[n.start + i];
        if v.is_object() && v.id() >= base.nodes.len && source::node(heap, v.id()).kind == node_kind::Closure {
            let g = source::node(heap, v.id()); assert(g.function as i64 > base.functions.len && g.count == 1);
            let k = heap.values[g.start]; inner = k.is_number() && k.num() == 41.0;
        }
    }
    assert(inner);
    var encoding = snapshot::Control {}; var wire: u8[>..] = []; assert(callable::encode(heap, base, plus, wire, encoding, source::reserve_identity()));
    assert(encoding.error == snapshot::Error.None);
    // The worker's reset cuts the eval code and the closure away.
    assert(snapshot::truncate(base, rows, identity, code, heap, bindings) && snapshot::pristine(base, identity, code, heap, bindings));
    assert(code.functions.len == base.functions.len && code.ops.len == base.ops.len && heap.nodes.len == base.nodes.len);
    assert(qput(Published { wire: copy(wire), entry: f, runtime: bindings[0].value, rt_cell: source::Value.Undefined, two: source::Value.Undefined, forms: source::Value.Undefined, cell: source::Value.Undefined, function: plus }));
}
thread_fn runtime_eval_consumer(base: snapshot::Snapshot, wire: u8[]) {
    defer { heap_program::finish(); }
    var extra = source::Code {}; extra_code(extra);
    var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; var sc = snapshot::Control {};
    assert(snapshot::restore(base, code, heap, roots, source::reserve_identity(), sc)); source::append_code(code, extra);
    let suffix = base.functions.len + extra.functions.len;
    var check = snapshot::Control {}; let imported = callable::decode_install(wire, base.header, code, heap, check);
    assert(check.error == snapshot::Error.None && imported.is_object());
    // The eval code's root, still marked as eval code, and the closure's
    // function sit behind the consumer's own code.
    let f = source::node(heap, imported.id()).function as i64;
    assert(code.functions[suffix].eval && f > suffix && !code.functions[f].eval && code.functions[f].start >= base.ops.len + extra.ops.len);
    let result = snapshot_test::invoke(heap, imported, [source::number(1)]);
    assert(result.is_number() && result.num() == 42.0);
    var valid = snapshot::Control {}; let finished = snapshot::capture(heap, roots, valid); assert(valid.error == snapshot::Error.None);
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
    let g = first.result; assert(g.is_object() && source::node(heap, g.id()).kind == node_kind::Generator && source::node(heap, g.id()).length == generator_state::SuspendedYield && heap.conts.len > base.conts.len);
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
    let input = source::form_collection(heap, node_kind::List, [source::number(1), source::number(2)]);
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
    assert(source::node(heap, mapped.id()).kind == node_kind::LazySeq && source::node(heap, mapped.id()).length == 0);
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
        assert(worker.nodes[other.id()].kind == node_kind::LazySeq && worker.nodes[other.id()].length == 0);
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
        assert(snapshot::ends(empty) == [0; 24] && target.identity == 0 && !dest.valid && target.fault == source::Fault.None);
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
    let a = source::allocate(heap, source::Node { kind: node_kind::Scalar, value: source::number(7), origin: 2 });
    let b = source::allocate(heap, source::Node { kind: node_kind::Scalar, value: source::number(7), origin: 3 });
    let left = source::form_collection(heap, node_kind::Vector, [a, b, a]); let joined = source::form_concat(heap, [left, left]);
    let array = source::allocate(heap, source::Node { kind: node_kind::Array });
    let length = source::text(heap, "length");
    for i in 3 { assert(source::put(heap, array, source::key(heap, source::number(i)), if i == 1 { b } else { a })); }
    assert(source::put(heap, array, length, source::number(1)));
    // Growth past a holed run retires it, and a sparse write makes a
    // dictionary array; both images must still validate (light-cljs-785).
    let grown = source::allocate(heap, source::Node { kind: node_kind::Array }); let sparse = source::allocate(heap, source::Node { kind: node_kind::Array });
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

// light-cljs-877: a map's or set's traversal order and a realized integer
// range's flavor (node `length`) restore as captured; any other value is a
// Link rejection.
thread_fn traversal_flags() {
    defer { heap_program::finish(); }
    var code = source::Code {}; snapshot_test::append(code, embed_file("../corpus/serial/arguments.gsp"));
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var pairs: source::Value[>..] = []; for i in 9 { pairs.push(source::number(i)); pairs.push(source::nil()); }
    let map = source::form_ordered(heap, node_kind::Map, pairs[..4], true); let set = source::form_ordered(heap, node_kind::Set, pairs[..], false);
    let range = source::allocate(heap, source::Node { kind: node_kind::Concat, start: source::node(heap, set.id()).start, count: 2, length: seq_flavor::IntegerRange as u32 });
    var c = snapshot::Control {}; let saved = snapshot::capture(heap, [snapshot::Binding { id: 0, generation: 1, value: map }, snapshot::Binding { id: 1, generation: 1, value: set }, snapshot::Binding { id: 2, generation: 1, value: range }], c); assert(c.error == snapshot::Error.None);
    {
        var dest = source::Code {}; var target = source::Heap { code: dest }; var roots: snapshot::Binding[>..] = []; var restore = snapshot::Control {};
        assert(snapshot::restore(saved, dest, target, roots, source::reserve_identity(), restore));
        assert(source::hash_ordered(source::node(target, roots[0].value.id())) && source::hash_ordered(source::node(target, roots[1].value.id())) && source::integer_range(source::node(target, roots[2].value.id())));
    }
    for v in [map, set, range] {
        var bad = copy(saved); bad.nodes[v.id()].length = 2;
        var check = snapshot::Control {}; assert(!snapshot::validate(bad, check) && check.error == snapshot::Error.Link);
    }
    print("snapshot traversal flags: hash order and integer ranges restore, other values refused");
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
    let object = source::allocate(heap, source::Node { kind: node_kind::Object }); assert(source::install_field(heap, object, "n", source::number(7)));
    let maker = source::closure(heap, keeper, []);
    let getter = snapshot_test::invoke(heap, maker, [object, object]);
    let arguments = snapshot_test::invoke(heap, getter, []); assert(source::node(heap, arguments.id()).kind == node_kind::Arguments);
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
    // Its name, "f", is a run of its own code units (light-cljs-806), its
    // source text its own code byte (light-cljs-853).
    extra.units.push(102);
    extra.functions.push(source::Function { start: 0, count: 3, constants: 0, children: 0, params: 0, locals: 0, captures: 2, strict: true, name_start: 0, name_len: 1, has_text: true, text_start: 0, text_end: 1 });
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
        for mode in -1..19 { heap_program::join(thread_spawn(callable_mode, copy(base), copy(original), copy(p), mode)); }
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
    let rt_cell = source::allocate(producer, source::Node { kind: node_kind::Binding, initialized: true, immutable: true, value: bindings[0].value });
    if identity { producer.source_ranges.push(source::SourceRange { start: 0, count: 3, identity: 77 }); } else { producer.source_bytes.append("a b"); producer.source_ranges.push(source::SourceRange { start: 0, count: 3 }); }
    let one = source::allocate(producer, source::Node { kind: node_kind::Scalar, value: source::number(7), origin: 1, source: 0, span_start: 0, span_end: 1 });
    let two = source::allocate(producer, source::Node { kind: node_kind::Scalar, value: source::number(7), origin: 2, source: 0, span_start: 2, span_end: 3 });
    let marked = source::form_op(producer, form_op::Keyword, [source::nil(), source::text(producer, "marked")]);
    let metadata = source::form_collection(producer, node_kind::Map, [marked, source::boolean(true)]); producer.nodes[one.id()].metadata = metadata; producer.nodes[two.id()].metadata = metadata;
    let forms = source::form_collection(producer, node_kind::Vector, [one, two, one]);
    let cell = source::allocate(producer, source::Node { kind: node_kind::Binding, initialized: true, immutable: true, value: forms });
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
            4 => { bad.image.nodes[cell.id()].kind = node_kind::Object; },
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
            16 => { bad.image.functions[entry].name_start = 0; }, // 16: a name in the base prefix
            17 => { bad.image.functions[entry].text_start = 0; }, // 17: source text in the base prefix
            18 => { bad.image.functions[entry].text_end = bad.image.code_bytes.len as u32 + 1; }, _ => {}, // 18: source text past the code bytes
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
            // So is its source text, behind the consumer's source bytes.
            assert(named.has_text && named.text_end == named.text_start + 1 && named.text_start >= base.code_bytes.len + extra.source_bytes.len && target_code.source_bytes[named.text_start] == 10);
            let n = target.nodes[imported.id()]; let captured = target.nodes[target.values[n.start].id()].value;
            assert(captured == roots[0].value);
            let result = snapshot_test::invoke(target, imported, []); let x = source::form_nth(target, result, 0); let y = source::form_nth(target, result, 1);
            assert(x.id() != y.id() && x.id() == source::form_nth(target, result, 2).id() && source::form_equal(target, x, y));
            assert(target.nodes[x.id()].span_start == 0 && target.nodes[y.id()].span_start == 2);
            assert(target.nodes[x.id()].metadata.id() == target.nodes[y.id()].metadata.id() && target.nodes[target.nodes[x.id()].metadata.id()].kind == node_kind::Map);
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
