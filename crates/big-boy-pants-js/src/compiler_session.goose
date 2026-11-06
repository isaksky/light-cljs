import std;
import os;
import .source_vm;
import .program;
import .snapshot;
import .form_bridge;
import .vm;
import .readiness;
import .callable;
import .census;
import .cell_owner;

// Process high-water RSS, in native getrusage units; see measurement.h.
extern fn peak_memory() -> i64 = "goose_measure_peak_rss";
extern fn resident_memory() -> i64 = "goose_measure_resident";
extern fn observe(bytes: const u8&, count: i64) -> i64 = "goose_measure_observe";
fn consume<T>(values: const T[:]) -> i64 { let bytes = bytes_of(values); return if bytes.len == 0 { 0 } else { observe(bytes[0], bytes.len) }; }
fn consume(s: const snapshot::Snapshot&) -> i64 { return s.header.work + consume(s.code_bytes) + consume(s.functions) + consume(s.code_constants) + consume(s.children) + consume(s.ops) + consume(s.code_units) + consume(s.nodes) + consume(s.values) + consume(s.properties) + consume(s.units) + consume(s.constants) + consume(s.bytes) + consume(s.sources) + consume(s.bindings) + consume(s.code_strings) + consume(s.strings) + consume(s.latin1) + consume(s.shapes) + consume(s.accessors) + consume(s.keys); }
fn retained(heap: const source::Heap&) -> i64 { return bytes_of(heap.nodes).len + bytes_of(heap.values).len + bytes_of(heap.properties).len + bytes_of(heap.units).len + heap.latin1.len + bytes_of(heap.constants).len + heap.source_bytes.len + bytes_of(heap.source_ranges).len + bytes_of(heap.shapes).len + bytes_of(heap.accessors).len + bytes_of(heap.keys).len; }

// GCS2 (light-cljs-784, light-cljs-795; docs/javascript-coverage.md,
// decision 3): one input thread, the coordinator, `limit + retention` reapers
// and contained job workers that serve many jobs each. The coordinator owns
// the session image (the realm, the base snapshot and the prepared definition
// snapshots) and is the only program that spawns job workers, on demand, at
// most `limit + retention` live. A job worker restores the base image once;
// each job installs its snapshot's rows above it, and after the job the
// worker truncates its tables back to the image (run_worker), or retires at
// its table-byte cap. At most `limit` workers execute at once. A dependency
// wait releases its execution slot; within the retention budget the worker
// blocks with its state in its own thread program until an Answer arrives,
// otherwise it saves a flat continuation and goes idle, and any worker
// restores it later.
struct Saved {
    heap:    snapshot::Snapshot = snapshot::Snapshot {}, frames: source::Frame[] = [], locals: source::Value[] = [], operands: source::Value[] = [], writes: source::Write[] = [], handlers: source::Handler[] = [],
    steps:   i64                = 0, args: source::Value[8] = [source::Value.Undefined; 8], frame: source::Frame = source::Frame { function: -1, pc: -1, local_base: 0, operand_base: 0, callee: source::Value.Undefined, receiver: source::Value.Undefined },
    bridge:  bridge::Control    = bridge::Control {}, sequence: i64 = 0, costs: i64[2] = [0; 2], protected: i64 = 0,
    storage: bool               = false, base: snapshot::Header = snapshot::Header {},
    // The job's generated-name scope (light-cljs-798): its prefix and the next
    // number of its count, 0 outside a scope. A wait carries the count out and
    // its answer back, so helper production numbers in the demanding job's scope.
    gensym_prefix: u8[] = [], gensym_next: i64 = 0,
    // The job's write boundary (source::Job.owned_nodes and foreign): a
    // continuation keeps the rows its job did not allocate read-only.
    owned: i64 = 0, foreign: u32[] = [],
    // The rows the job knows as cells or by process-wide identity
    // (source::Job.cell_rows, light-cljs-799), and how many delay claims it
    // holds unsettled (source::Job.claims, light-cljs-808).
    cell_rows: source::CellRow[] = [], claims: i64 = 0,
}
struct Command { id: i64 }
struct Cancel {}
// A single retained token is a session-wide cancellation flag. Active jobs
// inspect it only between bounded VM slices; idle workers block on Work.
fn cancelled() -> bool { let token, present = qpoll<Cancel>(); if present { assert(qput(token)); } return present; }
// Event kinds: 1 saved continuation, 2 finished, 3 failed, 5 produced helper,
// 6 reaped (key = thread, address = worker; aborted with its message in
// name), 7 a worker's job ended (retired: it exits, otherwise it is idle;
// timing: reset ns, the clock, table bytes at the job's end; image: the
// image digest in checked runs), 10 blocked wait, 11 blocked delay wait
// (light-cljs-808: delay the cell, epoch its claim; a kind 1 save of a
// delay wait carries them too). address names the job (its
// first checkpoint) and worker the job worker serving it. census: table rows at
// the worker's exit, then the restored image's rows, then unreachable rows
// (nodes, values, properties, units of both unit tables each; instrumented
// runs only). timing:
// spawn-to-start latency, the clock at this publication, restore bytes and ns.
// installs (kind 7): the job's worker_installs.
struct Event { id: i64, kind: i64, saved: Saved = Saved {}, gensym_next: i64 = 0, sequence: i64 = 0, key: i64 = -1, name: u8[] = [], data: u8[] = [], steps: i64 = 0, writes: i64 = 0, metrics: u8[] = [], callable: bool = false, address: i64 = 0, aborted: bool = false, census: i64[12] = [-1; 12], timing: i64[4] = [0; 4], worker: i64 = -1, retired: bool = false, image: u64 = 0, installs: i64[3] = [0; 3], delay: i64 = -1, epoch: i64 = 0 }
// A job worker's serial (Setup.worker), set in its own thread program's copy
// of this global; -1 in every other program.
var worker_serial: i64 = -1;
// The current job's snapshot-install ns, helper-install ns and helper
// installs, published with its kind 7 event (light-cljs-796).
var worker_installs: i64[3] = [0; 3];
fn publish(event: Event) { if event.worker < 0 { event.worker = worker_serial; } assert(qput(event)); readiness::notify(); }
// One job, from the one Work queue every idle job worker takes from
// (light-cljs-795). blocking: the worker may wait for a dependency in place;
// spawned: the coordinator's clock when it queued the job; stop: the worker
// exits instead; definition: a start's definition key, which names its
// publication's identities (light-cljs-808). mode: 0 start, 1 answered
// continuation, 2 helper production, 3 missing helper, 4 cancelled
// continuation, 5 a delay wait whose claim ended (light-cljs-808).
struct Work { saved: Saved = Saved {}, id: i64 = 0, identity: i64 = 0, mode: i64 = 0, helper: u8[] = [], key: i64 = -1, checkpoint: i64 = 0, callable: bool = false, blocking: bool = false, spawned: i64 = 0, stop: bool = false, definition: i64 = -1 }
// The coordinator's reply to a blocked worker, routed by address: mode 1
// delivers a produced helper, 3 a missing one, 5 ends a delay wait (the
// claim it waited for ended), 0 asks the worker to save its continuation
// and end the job, -1 to end it without a result. checkpoint is the
// worker's next completion barrier (dispatch-test).
struct Answer { address: i64, mode: i64 = 0, helper: u8[] = [], key: i64 = -1, callable: bool = false, checkpoint: i64 = 0 }
struct Reap { thread: i64, address: i64 = 0 }
struct Running { id: i64, thread: i64 = -1, address: i64 = 0, key: i64 = -1, checkpoint: i64 = 0 }
// address != 0: a blocked worker that keeps its state; otherwise saved holds
// the flat continuation. delay >= 0: a delay wait for that cell's claim
// `epoch` (light-cljs-808), resumed when the cell owner reports it ended.
struct Waiting { id: i64, name: u8[], saved: Saved, key: i64 = -2, active: bool = true, address: i64 = 0, thread: i64 = -1, prefix: u8[..64] = [], delay: i64 = -1, epoch: i64 = 0 }
struct Exit { address: i64, clock: i64 }
// Session totals for the JOBS line. Byte counts are spawn-argument copies
// (snapshot and helper) and restore copies inside the workers.
struct Jobs {
    spawned:   i64 = 0, spawn_ns: i64 = 0, start_ns: i64 = 0, join_ns: i64 = 0, argument_bytes: i64 = 0, restore_bytes: i64 = 0,
    peak_live: i64 = 0, peak_blocked: i64 = 0, waits: i64 = 0, saves: i64 = 0, answers: i64 = 0,
    trims:     i64 = 0, trim_before: i64 = 0, trim_after: i64 = 0, trim_ns: i64 = 0,
    // Jobs given to workers, workers retired at the table-byte cap, and the
    // workers' reset time (light-cljs-795).
    jobs: i64 = 0, retired: i64 = 0, reset_ns: i64 = 0,
    // What a per-worker definition and helper cache could save (light-cljs-796):
    // the workers' snapshot installs above the image (snapshot::extend), their
    // helper installs and how many, and the coordinator's time copying a
    // prepared definition into a job and queueing it (spawns excluded).
    restore_ns: i64 = 0, install_ns: i64 = 0, installs: i64 = 0, handoff_ns: i64 = 0,
}
struct Produced { key: i64, data: u8[], callable: bool = false }
struct Prepared { key: i64, heap: snapshot::Snapshot }
struct SerialDefinition { key: i64, function: source::Value }
// `{id}.definition` holds the definition key, then, when the compiler expands
// the macro in a generated-name scope, the scope's gensym prefix and the next
// number of its count: `key` or `key prefix next` (light-cljs-798).
fn definition(directory: const u8[:], id: i64) -> i64, u8[], i64 {
    var bytes:  u8[>..]  = []; assert(read_file(str(directory, "/", id, ".definition"), bytes));
    var spaces: i64[>..] = []; for b, i in bytes { if b == 32 { spaces.push(i); } }
    assert(spaces.len == 0 || spaces.len == 2); let key, ok = parse_int(bytes[..(if spaces.len == 0 { bytes.len } else { spaces[0] })]); assert(ok && key > 0);
    if spaces.len == 0 { return key, str(""), 0; }
    let prefix = bytes[(spaces[0] + 1)..spaces[1]]; for b in prefix { assert(b == 95 || b >= 48 && b <= 57); } assert(prefix.len > 0 && prefix.len <= 64);
    let next, valid = parse_int(bytes[(spaces[1] + 1)..]); assert(valid && next > 0 && next < 9007199254740991); return key, str(prefix), next;
}
// Put a job in a generated-name scope; its gensym_next is the count after.
fn scope(job: source::Job&, prefix: const u8[:], next: i64) { job.gensym_prefix.clear(); job.gensym_prefix.append(prefix); job.gensym_next = next; }
// `{id}-{sequence}.key` holds the helper's unit, -1 for a missing one, and,
// when the compiler registered it in a generated-name scope, what its
// initializer's gensym appends before its own count: `key` or `key prefix`
// (light-cljs-802). The helper is produced in that frozen scope from one,
// whichever job demands it first; without one its gensym faults.
fn helper_key(bytes: const u8[:]) -> i64, u8[] {
    var space: i64 = -1; for b, i in bytes { if b == 32 { assert(space < 0); space = i; } }
    let key, ok = parse_int(bytes[..(if space < 0 { bytes.len } else { space })]); assert(ok && key >= -1);
    if space < 0 { return key, str(""); }
    let prefix = bytes[(space + 1)..]; for b in prefix { assert(b == 95 || b == 105 || b >= 48 && b <= 57); } assert(prefix.len > 0 && prefix.len <= 64); return key, str(prefix);
}
// The scope a helper with initializer prefix `prefix` is produced in: its
// gensym_next is 1 there, 0 (faulting) without one.
fn helper_next(prefix: const u8[:]) -> i64 { return if prefix.len > 0 { 1 } else { 0 }; }
// Preparation runs before admission, in a thread program the coordinator
// spawns and joins (light-cljs-794: the coordinator's base image is its only
// heap). It cannot invoke compiler callbacks: those remain body effects in
// private resumable jobs. A failure's ERROR line goes to report.
fn initialize(id: i64, heap: source::Heap&, callee: source::Value, args: const source::Value[:], protected: i64, report: u8[>..]&, prefix: const u8[:], next: i64&) -> source::Value, bool {
    var job = source::Job { heap: heap, protected_nodes: protected }; defer { source::retire(job); } scope(job, prefix, next); defer { next = job.gensym_next; }
    if !source::start(job, callee, args) || source::run(job, -1) != tiny::Outcome.Complete {
        let pc = job.fault_pc; let start = if pc >= 0 && pc < heap.code.ops.len { heap.code.ops[pc].start } else { -1 }; let end = if pc >= 0 && pc < heap.code.ops.len { heap.code.ops[pc].end } else { -1 };
        var fault: u8[>..] = []; source::describe_fault(fault, heap.code, job);
        format(report, "ERROR ", id, " preparation-", fault, " function=", job.fault_function, " pc=", pc, " emitted-span=", start, ":", end); return source::Value.Undefined, false;
    }
    return job.result, true;
}
fn initialize(id: i64, heap: source::Heap&, callee: source::Value, args: const source::Value[:], protected: i64, prefix: const u8[:], next: i64&) -> source::Value, bool {
    var report: u8[>..] = []; let value, ok = initialize(id, heap, callee, args, protected, report, prefix, next); if !ok { print(report); flush_stdout(); } return value, ok;
}
// The prepared definition snapshot, or the ERROR line that refused it.
struct Preparation { ok: bool = false, image: snapshot::Snapshot = snapshot::Snapshot {}, report: u8[] = [], gensym_next: i64 = 0 }
thread_fn preparer(base: snapshot::Snapshot, directory: u8[], id: i64, key: i64, identity: i64, prefix: u8[], first: i64) {
    var next = first;
    var report: u8[>..] = [];
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; var sc = snapshot::Control {};
    assert(snapshot::restore(base, code, heap, bindings, identity, sc)); let rt = bindings[0].value;
    let entry = append(code, heap, str(directory, "/definition-", key, ".gsp")); assert(entry >= 0);
    let factory, loaded = initialize(id, heap, source::closure(heap, entry, []), [], base.nodes.len, report, prefix, next); if !loaded { assert(qput(Preparation { report: copy(report) })); return; }
    let function, ready = initialize(id, heap, factory, [rt], base.nodes.len, report, prefix, next); if !ready { assert(qput(Preparation { report: copy(report) })); return; }
    if !source::valid(heap, function) || function.tag() != 5 || source::node(heap, function.id()).kind != 2 { format(report, "ERROR ", id, " preparation-TypeError function=-1 pc=-1 emitted-span=-1:-1"); assert(qput(Preparation { report: copy(report) })); return; }
    // Every atom the definition reaches becomes a process-wide cell (light-cljs-799).
    var thunks = false; var initial: source::Initial[>..] = []; let refused = source::publish_cells(heap, function, base.nodes.len, thunks, initial);
    if refused.len > 0 { format(report, "ERROR ", id, " preparation-", refused, " function=-1 pc=-1 emitted-span=-1:-1"); assert(qput(Preparation { report: copy(report) })); return; }
    // The cells' states, whose objects of the definition have the identities
    // of their rows above the image (light-cljs-808).
    if initial.len > 0 {
        var rows: i64[>..] = []; for r in base.nodes.len..heap.nodes.len { rows.push(r); }
        let stored = source::store_initial(heap, initial, source::places_of(heap, rows), source::definition_publication(key));
        if stored.len > 0 { format(report, "ERROR ", id, " preparation-", stored, " function=-1 pc=-1 emitted-span=-1:-1"); assert(qput(Preparation { report: copy(report) })); return; }
    }
    heap.initializer_count += 1;
    let result = snapshot::capture(heap, [snapshot::Binding { id: 0, generation: 1, value: rt }, snapshot::Binding { id: 1, generation: 1, value: function }], sc); assert(sc.error == snapshot::Error.None);
    assert(qput(Preparation { ok: true, image: copy(result), gensym_next: next }));
}
// COST covers the preparer's spawn, base copy and join as well as its work.
fn prepare(base: const snapshot::Snapshot&, directory: const u8[:], id: i64, key: i64, prefix: const u8[:], next: i64&) -> snapshot::Snapshot, bool {
    let started = clock_ns();
    assert(thread_wait(thread_spawn(preparer, copy(base), str(directory), id, key, source::reserve_identity(), str(prefix), next)));
    let p = qget<Preparation>(); if !p.ok { print(p.report); flush_stdout(); return snapshot::Snapshot {}, false; } next = p.gensym_next;
    print("PREPARED ", id, " ", key); print("COST ", id, " ", clock_ns() - started, " 0"); flush_stdout(); return copy(p.image), true;
}
thread_fn input(directory: u8[], deterministic: bool) {
    var count: i64 = 0;
    loop { var line: u8[>..] = []; let present = read_line(line); let parsed, ok = parse_int(line); let id = if !present { 0 } else if !ok || parsed < -2 { -1 } else { parsed };
        // A credit reserves one command slot, including EOF/close. Blocking
        // here gives the pipe real backpressure without fixed-delay retries.
        qget<readiness::InputCredit>(); assert(qput(Command { id: id })); readiness::notify();
        if id > 0 { count += 1; } if deterministic && count == 8 { assert(write_file(str(directory, "/commands-ready"), "ready")); } if id == 0 || id == -1 { return; }
    }
}
fn load(code: source::Code&, path: const u8[:]) -> i64 {
    var bytes: u8[>..] = []; if !read_file(path, bytes) { return -1; }
    var image = program::Image {}; if program::install(image, bytes) != program::Error.None { return -1; }
    var part = source::Code {}; if !source::load(part, image) { return -1; }
    return source::append_code(code, part);
}
fn append(code: source::Code&, heap: source::Heap&, path: const u8[:]) -> i64 {
    let first = code.constants.len; let entry = load(code, path); if entry < 0 { return -1; }
    for c in code.constants[first..] { var value = c; if c.tag() == 4 { value = source::string(heap, source::code_chars(code, c)); } source::add_constants(heap, [value]); }
    return entry;
}
fn leaf(id: i64, heap: source::Heap&, callee: source::Value, args: const source::Value[:], protected: i64, prefix: const u8[:], next: i64&, address: i64 = 0, timing: i64[4] = [0; 4]) -> source::Value, bool {
    let roots = source::get(heap, heap.global, source::text(heap, "__lightCompilerRoots")); var job = source::Job { heap: heap, protected_nodes: protected, root_receiver: roots }; defer { source::retire(job); } scope(job, prefix, next); defer { next = job.gensym_next; }
    source::start(job, callee, args); if source::run(job, -1) != tiny::Outcome.Complete { fail(id, heap, job, address, timing); return source::Value.Undefined, false; } return job.result, true;
}
fn save(job: source::Job&, rt: source::Value, call: source::Value, control: bridge::Control, sequence: i64, costs: i64[2], checkpoint: i64) -> Saved {
    assert(!job.claimed && job.effect_pending && job.status == tiny::Status.Ready && (job.effect_kind == 9 && job.effect_count == 3 || job.effect_kind == 10 && job.effect_count == 2));
    // The continuation carries the job's delay claims; this worker no longer does.
    let claims = job.claims; job.claims = 0;
    assert(source::retire(job, true)); var c = snapshot::Control {}; let heap = snapshot::capture(job.heap, [snapshot::Binding { id: 0, generation: 1, value: rt }, snapshot::Binding { id: 1, generation: 1, value: call }], c, checkpoint); assert(c.error == snapshot::Error.None);
    return Saved { heap: copy(heap), frames: copy(job.frames), locals: copy(job.locals), operands: copy(job.operands), writes: copy(job.writes), handlers: copy(job.handlers), steps: job.steps, args: job.effect_args, frame: job.effect_frame, bridge: control, sequence: sequence, costs: costs, protected: job.protected_nodes, gensym_prefix: copy(job.gensym_prefix), gensym_next: job.gensym_next, owned: job.owned_nodes, foreign: copy(job.foreign), cell_rows: copy(job.cell_rows), claims: claims };
}
fn fail(id: i64, heap: source::Heap&, job: source::Job&, address: i64 = 0, timing: i64[4] = [0; 4]) {
    let pc = job.fault_pc; var start: i64 = -1; var end: i64 = -1;
    if pc >= 0 && pc < heap.code.ops.len { start = heap.code.ops[pc].start; end = heap.code.ops[pc].end; }
    var fault: u8[>..] = []; source::describe_fault(fault, heap.code, job);
    let text = str(fault, " function=", job.fault_function, " pc=", pc, " emitted-span=", start, ":", end, " writes=", job.writes.len);
    publish(Event { id: id, kind: 3, name: copy(text), address: address, timing: timing });
}
fn reset(graph: bridge::Graph&) { graph.nodes.clear(); graph.children.clear(); graph.units.clear(); graph.sources.clear(); graph.bytes.clear(); graph.roots.clear(); }
// The session's cell owner (light-cljs-799, cell_owner.goose): a program
// with no VM tables that holds every process-wide cell. The coordinator
// spawns it before any preparer or job worker, which inherit
// cells::enabled, and stops it after the last worker is reaped.
thread_fn cell_owner(budget: i64) { cells::serve(budget); }
// Reapers only join job workers and report how each ended. Each holds one
// Reap notice at a time and the coordinator keeps at most `reapers` job
// workers live, so every live worker's notice is taken at once and a
// contained abort is reported while its peers still run. A contained abort
// still terminates the whole session: effects are never replayed. Reapers
// never execute VM work or own a VM heap.
thread_fn supervise() {
    loop {
        let reap = qget<Reap>(); if reap.thread < 0 { return; }
        var reason: u8[>..] = []; let ok = thread_wait(reap.thread, reason);
        publish(Event { id: 0, kind: 6, key: reap.thread, name: copy(reason), address: reap.address, aborted: !ok, timing: [0, clock_ns(), 0, 0] });
    }
}
// A reused job worker (light-cljs-795; docs/javascript-coverage.md, decision
// 3). It restores the coordinator's base image once, from the snapshot its
// spawn copied, which stays its pristine twin, and then serves jobs from the
// one Work queue. A job installs its snapshot's rows above the image
// (snapshot::extend); its frames, operands and bridge buffers are locals of
// serve. After the job the worker truncates every table to the image and
// restores the service rows and state fields from the twin
// (snapshot::truncate), so the next job's tables are a fresh worker's, at the
// same rows. A worker whose tables ended a job above `cap` bytes retires
// instead (0: after every job, one worker per job as light-cljs-784 had it).
// `check` (tests, BBP_RESET_CHECK=1) compares the image with the twin after
// every reset and every installed snapshot's prefix with the image.
// Configuration a job worker takes at spawn.
struct Setup { worker: i64 = 0, identity: i64 = 0, deterministic: bool = false, cleanup_test: bool = false, cap: i64 = 0, check: bool = false }
thread_fn run_worker(base: snapshot::Snapshot, directory: u8[], setup: Setup) {
    worker_serial = setup.worker;
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; var sc = snapshot::Control {};
    assert(snapshot::restore(base, code, heap, bindings, setup.identity, sc));
    let roots = source::get(heap, heap.global, source::text(heap, "__lightCompilerRoots")); let cache = source::get(heap, heap.global, source::text(heap, "__lightPublishedHelpers"));
    assert(roots.tag() == 5 && cache.tag() == 5 && roots.id() < base.nodes.len && cache.id() < base.nodes.len);
    let service: i64[2] = [roots.id(), cache.id()];
    assert(snapshot::truncate(base, service, setup.identity, code, heap, bindings));
    let image = if setup.check { assert(snapshot::pristine(base, setup.identity, code, heap, bindings)); heap_digest(heap, bindings) } else { 0 };
    loop {
        let work = qget<Work>(); if work.stop { return; }
        // Deterministic poison regression: hold a started replacement before it
        // runs, so poison must also cancel a live worker that has not executed.
        if setup.cleanup_test && work.id == 2 { assert(write_file(str(directory, "/claim-ready"), "ready")); var gate: u8[>..] = []; while !read_file(str(directory, "/claim-release"), gate) { sleep_ms(1); } }
        // Deterministic regression (light-cljs-803): hold job 4's resumed
        // continuation before it runs, so the session's cancellation reaches
        // it queued, after the coordinator stopped tracking it as a waiter.
        if setup.cleanup_test && work.id == 4 && work.mode == 3 { assert(write_file(str(directory, "/resume-ready"), "ready")); var gate: u8[>..] = []; while !read_file(str(directory, "/resume-release"), gate) { sleep_ms(1); } }
        serve(work, directory, setup, base, service, code, heap, bindings);
        // Publish the real terminal result, then abort while this worker still
        // runs. The host starts a replacement before releasing the gate.
        if setup.cleanup_test && work.id == 1 { assert(write_file(str(directory, "/cleanup-ready"), "ready")); var gate: u8[>..] = []; while !read_file(str(directory, "/cleanup-abort"), gate) { sleep_ms(1); } assert(false); }
        let bytes = table_bytes(code, heap);
        // Retiring: the exit frees the tables in one step.
        if bytes > setup.cap { publish(Event { id: 0, kind: 7, timing: [0, clock_ns(), bytes, 0], worker: setup.worker, retired: true, image: image, installs: worker_installs }); return; }
        let resetting = clock_ns(); assert(snapshot::truncate(base, service, setup.identity, code, heap, bindings));
        if setup.check { assert(snapshot::pristine(base, setup.identity, code, heap, bindings)); }
        let reset_ns = clock_ns() - resetting; publish(Event { id: 0, kind: 7, timing: [reset_ns, clock_ns(), bytes, 0], worker: setup.worker, image: image, installs: worker_installs }); worker_installs = [0; 3];
    }
}
// One job: its VM scratch lives here and goes when it returns.
fn serve(work: const Work&, directory: const u8[:], setup: const Setup&, base: const snapshot::Snapshot&, service: const i64[:], code: source::Code&, heap: source::Heap&, bindings: snapshot::Binding[>..]&) {
    let started = clock_ns();
    var job = source::Job { heap: heap, cell_code: code }; var args: source::Value[>..] = []; var graph = bridge::Graph {}; var bytes: u8[>..] = [];
    execute(work, directory, started, setup, base, service, code, heap, bindings, job, args, graph, bytes);
    // A job that ended holding delay claims (a fault its thunk could not
    // catch, a cancellation) releases them, so the jobs waiting for them
    // force again (light-cljs-808).
    if job.claims > 0 && job.cell_holder > 0 { cells::exchange(cells::Request { op: 7, holder: job.cell_holder }); job.claims = 0; }
}
// The bytes a worker's VM tables hold: the retirement measure.
fn table_bytes(code: const source::Code&, heap: const source::Heap&) -> i64 {
    return bytes_of(code.source_bytes).len + bytes_of(code.functions).len + bytes_of(code.constants).len + bytes_of(code.children).len + bytes_of(code.ops).len + bytes_of(code.units).len + bytes_of(code.strings).len + bytes_of(code.ins).len + bytes_of(code.effects).len + bytes_of(code.heights).len
         + retained(heap) + bytes_of(heap.strings).len;
}
// FNV-1a over an image's table bytes: equal images, equal digests.
fn mix(h: u64, bytes: const u8[:]) -> u64 { var x = h; for b in bytes { x = (x ^ b) * 1099511628211; } return x; }
fn heap_digest(heap: const source::Heap&, bindings: const snapshot::Binding[:]) -> u64 { let v = snapshot::borrow(heap, bindings, 1); return digest(v); }
fn digest<T>(s: const T&) -> u64 {
    var h: u64 = 14695981039346656037;
    h = mix(h, bytes_of(s.code_bytes)); h = mix(h, bytes_of(s.functions)); h = mix(h, bytes_of(s.code_constants)); h = mix(h, bytes_of(s.children)); h = mix(h, bytes_of(s.ops)); h = mix(h, bytes_of(s.code_units)); h = mix(h, bytes_of(s.code_strings));
    h = mix(h, bytes_of(s.nodes)); h = mix(h, bytes_of(s.values)); h = mix(h, bytes_of(s.properties)); h = mix(h, bytes_of(s.units)); h = mix(h, bytes_of(s.strings)); h = mix(h, bytes_of(s.constants)); h = mix(h, bytes_of(s.bytes)); h = mix(h, bytes_of(s.sources)); h = mix(h, bytes_of(s.latin1)); h = mix(h, bytes_of(s.shapes)); h = mix(h, bytes_of(s.accessors)); h = mix(h, bytes_of(s.keys));
    return h;
}
// A blocked worker takes answers from the one process-wide Answer queue and
// puts back any addressed to another worker (the re-queue pattern of
// cancelled()). It also leaves on session cancellation (false).
fn await_answer(address: i64) -> Answer, bool {
    loop {
        if cancelled() { return Answer { address: address }, false; }
        let answer, present = qget<Answer>(5);
        if !present { continue; }
        if answer.address == address { return answer, true; }
        assert(qput(answer)); sleep_ms(1);
    }
}
fn timing(work: const Work&, started: i64, sc: snapshot::Control) -> i64[4] { return [started - work.spawned, clock_ns(), sc.copied_bytes, sc.restore_ns]; }
// Instrumented runs only: the job's table rows at this exit, the restored
// image's rows, and the rows no root of this exit reaches.
fn rows(work: const Work&, heap: const source::Heap&, job: const source::Job&, bindings: const snapshot::Binding[:], extra: source::Value) -> i64[12] {
    if !work.saved.storage { return [-1; 12]; }
    var roots: source::Value[>..] = [job.result, extra];
    for b in bindings { roots.push(b.value); }
    for f in job.frames { roots.push(f.callee); roots.push(f.receiver); roots.push(f.input); roots.push(f.output); }
    for h in job.handlers { roots.push(h.value); }
    for v in job.locals { roots.push(v); } for v in job.operands { roots.push(v); } for v in job.effect_args { roots.push(v); }
    for w in job.writes { roots.push(w.object); roots.push(w.key); roots.push(w.value); }
    let c = census::take(heap, roots); let s .= work.saved.heap;
    return [c.nodes, c.values, c.properties, c.units, s.nodes.len, s.values.len, s.properties.len, s.units.len + s.latin1.len, c.dead_nodes, c.dead_values, c.dead_properties, c.dead_units];
}
// A published callable helper, from the session cache or installed once.
// Its rows are the publication's (source::install_publication): the job
// knows their identities and cells, and maps those it held already.
fn published(helper: const u8[:], base: snapshot::Header, key: i64, code: source::Code&, heap: source::Heap&, rows: source::CellRow[>..]&) -> source::Value, bool {
    let cache = source::get(heap, heap.global, source::text(heap, "__lightPublishedHelpers")); let name = source::text(heap, str(key));
    let prior, found = source::own_property(heap, cache, name);
    if found { return prior, true; }
    let installing = clock_ns(); defer { worker_installs[1] += clock_ns() - installing; worker_installs[2] += 1; }
    let start = heap.nodes.len; let values = heap.values.len; let constants = heap.constants.len;
    var publication = snapshot::Control {}; let installed = callable::decode_install(helper, base, code, heap, publication);
    if publication.error != snapshot::Error.None { return installed, false; }
    let imported = source::install_publication(heap, rows, start, heap.nodes.len - start, values, constants, source::helper_publication(key), installed);
    return imported, source::put(heap, cache, name, imported);
}
fn execute(work: const Work&, directory: const u8[:], started: i64, setup: const Setup&, base: const snapshot::Snapshot&, service: const i64[:], code: source::Code&, heap: source::Heap&, bindings: snapshot::Binding[>..]&, job: source::Job&, args: source::Value[>..]&, graph: bridge::Graph&, bytes: u8[>..]&) {
    let saved .= work.saved; let id = work.id; let identity = work.identity; let mode = work.mode; let key = work.key; let checkpoint = work.checkpoint; let address = work.checkpoint;
    let deterministic = setup.deterministic; let cleanup_test = setup.cleanup_test;
    // Test-only completion barrier: publish the real event before acknowledging.
    var barrier = checkpoint;
    defer { if deterministic { assert(write_file(str(directory, "/ready-", barrier), "ready")); } }
    let worker_started = clock_ns();
    var sc = snapshot::Control {};
    // A fresh job (a start or a helper production) has entered no finally
    // block yet; a continuation checks once its activation is restored.
    if (mode == 0 || mode == 2) && cancelled() { publish(Event { id: id, kind: 3, name: str("cancelled"), address: address, timing: timing(work, started, sc) }); return; }
    // The job's rows above the worker's image; the image rows are the twin's.
    if setup.check { assert(snapshot::over_image(saved.heap, base, service)); }
    assert(snapshot::extend(saved.heap, base, service, code, heap, bindings, identity, sc)); worker_installs[0] += sc.restore_ns; let rt = bindings[0].value;
    // Rows below this are the image's and the definition's (a continuation
    // keeps its job's boundary).
    let owned = if mode == 1 || mode == 3 || mode == 4 || mode == 5 { saved.owned } else { heap.nodes.len };
    var costs = saved.costs; var control = saved.bridge; var call = source::Value.Undefined; var function = source::Value.Undefined; var next = saved.gensym_next;
    if mode == 0 { function = bindings[1].value; let importing = clock_ns();
        assert(read_file(str(directory, "/", id, ".gfb"), bytes)); assert(bridge::decode(graph, bytes, control)); assert(bridge::import_graph(heap, graph, args, control)); assert(args.len >= 2); call = args[0]; costs[1] += clock_ns() - importing;
    } else if mode == 2 {
        let loading = clock_ns(); let entry = append(code, heap, str(directory, "/", id, "-", saved.sequence, ".helper")); assert(entry >= 0); costs[0] += clock_ns() - loading;
        let factory, loaded = leaf(id, heap, source::closure(heap, entry, []), [], saved.protected, saved.gensym_prefix, next, address, timing(work, started, sc)); if !loaded { return; }
        let value, initialized = leaf(id, heap, factory, [rt], saved.protected, saved.gensym_prefix, next, address, timing(work, started, sc)); if !initialized { return; } function = value;
        if mode == 2 { let exporting = clock_ns();
            let is_closure = function.tag() == 5 && source::node(heap, function.id()).kind == 2;
            var publication = snapshot::Control {};
            // Every atom the helper reaches becomes a process-wide cell, and a
            // form helper's graph names it (light-cljs-799).
            var thunks = false; var initial: source::Initial[>..] = []; let refused = source::publish_cells(heap, function, base.nodes.len, thunks, initial);
            if refused.len > 0 { publish(Event { id: id, kind: 3, name: str("helper-", refused), address: address, timing: timing(work, started, sc) }); return; }
            // A value that reaches a pending delay goes as a callable
            // publication, which carries the delay's thunk (light-cljs-808).
            let closure = is_closure || thunks;
            control.cells = true;
            var placed: i64[>..] = [];
            let ok = if closure { callable::encode(heap, saved.heap, function, bytes, publication, checkpoint, placed) } else { bridge::export_rows(heap, [function], graph, control, source::Value.Undefined, placed) && bridge::encode(graph, bytes, control) };
            // The cells' states, whose objects of the publication have the
            // identities of their places in it (light-cljs-808).
            if ok && initial.len > 0 {
                let stored = source::store_initial(heap, initial, source::places_of(heap, placed), source::helper_publication(key));
                if stored.len > 0 { publish(Event { id: id, kind: 3, name: str("helper-", stored), address: address, timing: timing(work, started, sc) }); return; }
            }
            if !ok {
                let function_id = if is_closure { source::node(heap, function.id()).function } else { entry }; let pc = code.functions[function_id].start; let op = code.ops[pc];
                publish(Event { id: id, kind: 3, name: str("helper-", heap.fault, "-", control.error, "-", publication.error, " function=", function_id, " pc=", pc, " emitted-span=", op.start, ":", op.end), address: address, timing: timing(work, started, sc) }); return;
            } costs[1] += clock_ns() - exporting; publish(Event { id: id, kind: 5, key: key, name: str(costs[0], " ", costs[1]), data: copy(bytes), metrics: str(sc.restore_ns, " 0 ", clock_ns() - worker_started, " ", storage(saved, heap, sc, bytes.len), " ", mode), callable: closure, address: address, census: rows(work, heap, job, bindings, function), timing: timing(work, started, sc) }); return;
        }
    } else { call = bindings[1].value; }
    // Import before re-registering the suspended job; failed publication is
    // transactional and delivered at the original effect instruction.
    // The rows the job knows as cells or by identity: its continuation's, or
    // a start's definition, a publication (light-cljs-808).
    for r in saved.cell_rows { job.cell_rows.push(r); } job.claims = saved.claims;
    if mode == 0 { source::install_publication(heap, job.cell_rows, base.nodes.len, owned - base.nodes.len, heap.values.len, heap.constants.len, source::definition_publication(work.definition), source::Value.Undefined); }
    var imported = source::Value.Undefined; var import_ok = false; let installing = heap.nodes.len;
    if mode == 1 && work.callable {
        let importing = clock_ns(); let value, ok = published(work.helper, saved.base, key, code, heap, job.cell_rows); imported = value; import_ok = ok; costs[1] += clock_ns() - importing;
    }
    let roots = source::get(heap, heap.global, source::text(heap, "__lightCompilerRoots")); job.effects_enabled = true; job.helpers_enabled = true; job.protected_nodes = saved.protected; job.root_receiver = roots; defer { source::retire(job); } scope(job, saved.gensym_prefix, next);
    job.owned_nodes = owned; for r in saved.foreign { job.foreign.push(r); } job.cell_holder = id; source::install_foreign(job, installing);
    if mode == 0 { source::start(job, function, args); } else {
        // Saved rows link the restored heap's rows by index: copied verbatim.
        for f in saved.frames { job.frames.push(f); } job.locals.append(saved.locals); job.operands.append(saved.operands);
        job.writes.append(saved.writes);
        for h in saved.handlers { job.handlers.push(h); } job.steps = saved.steps; job.registered = true; heap.live_jobs += 1; job.effect_pending = true; job.effect_kind = 9; job.effect_count = 3; job.effect_frame = saved.frame; for v, i in saved.args { job.effect_args[i] = v; }
        // Mode 4: the session was cancelled while this continuation waited;
        // or it was cancelled while this resume was queued (light-cljs-803).
        // Its finally blocks run under its limits; nothing is published.
        if mode == 4 || cancelled() { publish(Event { id: id, kind: 3, name: clean_up(job), address: address, timing: timing(work, started, sc) }); return; }
        if mode == 1 && !work.callable { let start = heap.nodes.len; let installing = clock_ns(); defer { worker_installs[1] += clock_ns() - installing; worker_installs[2] += 1; } control.cells = true; import_ok = bridge::decode(graph, work.helper, control) && source::import_publication(heap, job.cell_rows, graph, args, control, source::helper_publication(key)) && args.len == 1; if import_ok { imported = args[0]; } source::install_foreign(job, start); }
        if mode == 5 { assert(source::deliver_effect(job, source::Value.Undefined)); }
        else if !import_ok { source::deliver_effect(job, source::Value.Undefined, source::Fault.HostEffect); } else { source::put(heap, job.effect_args[0], job.effect_args[2], imported); assert(source::deliver_effect(job, imported)); }
    }
    // A delay wait (mode 5) took no helper, so its root reads go on at the same sequence.
    let execution_started = clock_ns(); var sequence = if mode == 0 { 0 } else if mode == 5 { saved.sequence } else { saved.sequence + 1 };
    loop {
        if cancelled() { publish(Event { id: id, kind: 3, name: clean_up(job), address: address, timing: timing(work, started, sc) }); return; }
        let outcome = source::run(job, 10000); if outcome == tiny::Outcome.Complete { break; } if outcome == tiny::Outcome.Failed { fail(id, heap, job, address, timing(work, started, sc)); return; }
        if !job.effect_pending { continue; }
        // A force waiting for another job's delay claim (kind 10: the cell and
        // the claim's epoch, light-cljs-808) waits as a dependency does, and
        // forces again when the coordinator hears the claim ended.
        var delay: i64 = -1; var epoch: i64 = 0; var name: u8[>..] = [];
        if job.effect_kind == 10 && job.effect_count == 2 { delay = job.effect_args[0].num() as i64; epoch = job.effect_args[1].num() as i64; }
        else {
            if job.effect_kind != 9 || job.effect_count != 3 { source::deliver_effect(job, source::Value.Undefined, source::Fault.NativeBoundary); continue; }
            let v = job.effect_args[2]; if v.tag() != 4 { source::deliver_effect(job, source::Value.Undefined, source::Fault.TypeError); continue; }
            for u in source::utf16(heap, v) { if u <= 32 || u > 127 { source::deliver_effect(job, source::Value.Undefined, source::Fault.NativeBoundary); break; } name.push(u as u8); } if !job.effect_pending { continue; }
        }
        if work.blocking {
            // Wait in place: the job's state stays in this thread program.
            publish(Event { id: id, kind: if delay >= 0 { 11 } else { 10 }, saved: Saved { gensym_prefix: copy(job.gensym_prefix), gensym_next: job.gensym_next }, sequence: sequence, name: copy(name), steps: job.steps, writes: job.writes.len, address: address, timing: timing(work, started, sc), delay: delay, epoch: epoch });
            if deterministic { assert(write_file(str(directory, "/ready-", barrier), "ready")); }
            // Deterministic regression: a contained abort while blocked.
            if cleanup_test && id == 3 { assert(false); }
            // Deterministic regression: jobs above 10 leave their answers
            // queued until the host releases them, so the Answer queue must
            // hold one answer per live worker.
            if cleanup_test && id > 10 { var gate: u8[>..] = []; while !read_file(str(directory, "/answer-release"), gate) { sleep_ms(1); } }
            let answer, present = await_answer(address);
            if !present { publish(Event { id: id, kind: 3, name: clean_up(job), address: address, timing: timing(work, started, sc) }); return; }
            barrier = answer.checkpoint;
            if answer.mode == -1 { return; }
            if answer.mode == 5 { assert(source::deliver_effect(job, source::Value.Undefined)); continue; }
            if answer.mode != 0 {
                // A helper produced for this wait numbered in its own scope.
                let importing = clock_ns(); var value = source::Value.Undefined; var ok = false; let start = heap.nodes.len;
                if answer.mode == 1 && answer.callable {
                    // Installing a publication requires no live job. The
                    // suspended job holds only values into the append-only
                    // prefix, as a saved continuation does before restore.
                    job.registered = false; heap.live_jobs -= 1;
                    let installed, valid = published(answer.helper, saved.base, answer.key, code, heap, job.cell_rows); value = installed; ok = valid;
                    job.registered = true; heap.live_jobs += 1;
                } else if answer.mode == 1 {
                    reset(graph); var helper: source::Value[>..] = []; let installing = clock_ns(); defer { worker_installs[1] += clock_ns() - installing; worker_installs[2] += 1; }
                    control.cells = true; ok = bridge::decode(graph, answer.helper, control) && source::import_publication(heap, job.cell_rows, graph, helper, control, source::helper_publication(answer.key)) && helper.len == 1; if ok { for h in helper { value = h; } }
                }
                costs[1] += clock_ns() - importing; source::install_foreign(job, start);
                if !ok { source::deliver_effect(job, source::Value.Undefined, source::Fault.HostEffect); } else { source::put(heap, job.effect_args[0], job.effect_args[2], value); assert(source::deliver_effect(job, value)); }
                sequence += 1; continue;
            }
            // Over the retention budget: save the continuation and end the job.
        }
        let counted = rows(work, heap, job, bindings, call);
        let state = save(job, rt, call, control, sequence, costs, checkpoint); state.storage = saved.storage; state.base = saved.base; publish(Event { id: id, kind: 1, saved: copy(state), name: copy(name), steps: job.steps, metrics: str(sc.restore_ns, " ", clock_ns() - execution_started, " ", clock_ns() - worker_started, " ", storage(saved, heap, sc, 0), " ", mode), address: address, census: counted, timing: timing(work, started, sc), delay: delay, epoch: epoch }); return;
    }
    let exporting = clock_ns(); let execution_ns = exporting - execution_started; let result = job.result; let steps = job.steps; assert(source::retire(job)); reset(graph); bytes.clear();
    let counted = rows(work, heap, job, bindings, call);
    control.cells = false; if !bridge::export_graph(heap, [result], graph, control, call) || !bridge::encode(graph, bytes, control) { publish(Event { id: id, kind: 3, name: str("bridge-", control.error), address: address, timing: timing(work, started, sc) }); return; }
    assert(write_file(str(directory, "/", id, ".out"), bytes)); costs[1] += clock_ns() - exporting; publish(Event { id: id, kind: 2, gensym_next: job.gensym_next, name: str(costs[0], " ", costs[1]), steps: steps, writes: job.writes.len, metrics: str(sc.restore_ns, " ", execution_ns, " ", clock_ns() - worker_started, " ", storage(saved, heap, sc, bytes.len), " ", mode), address: address, census: counted, timing: timing(work, started, sc) });
}
// Session cancellation reaching a job, at the dispatch poll point, while it
// waits in place for a helper, or while it waits as a saved continuation
// (mode 4, light-cljs-803): its finally blocks run, under its limits, before
// it ends Cancelled (light-cljs-786). A cleanup cannot wait, so it never
// suspends again. The job's report: `cancelled`, then ` cleanup=` and the
// keys of the named property writes the cleanup made, in order (at most 32),
// so the session's trace shows which finally blocks ran.
fn clean_up(job: source::Job&) -> u8[] {
    let from = job.writes.len;
    if source::cancel(job) { while source::run(job, 10000) == tiny::Outcome.Yielded && !job.effect_pending {} }
    var out: u8[>..] = []; out.append("cancelled");
    for w, i in job.writes[from..] {
        if i >= 32 { break; }
        out.append(if i == 0 { " cleanup=" } else { "," });
        if w.key.tag() != 4 { out.push('?'); continue; }
        for u, j in source::utf16(job.heap, w.key) { if j >= 64 { break; } out.push(if u > 32 && u < 127 && u != 44 { u as u8 } else { '?' }); }
    }
    return str(out);
}
fn storage(saved: const Saved&, heap: const source::Heap&, sc: snapshot::Control, output: i64) -> u8[] {
    if !saved.storage { return str("0 0 0 0 0 0 0"); }
    return str(sc.copied_bytes, " ", if saved.heap.nodes.len == 0 { 0 } else { snapshot::footprint(saved.heap) }, " ", output, " ", heap.nodes.len, " ", heap.values.len, " ", peak_memory(), " ", retained(heap));
}
fn release(running: Running[>..]&, address: i64) -> Running, bool { for r, i in running { if r.address == address { let out = r; running.swap_remove(i); return out, true; } } return Running { id: 0 }, false; }
fn footprint(saved: const Saved&) -> i64 { return snapshot::footprint(saved.heap) + bytes_of(saved.frames).len + bytes_of(saved.locals).len + bytes_of(saved.operands).len + bytes_of(saved.writes).len + bytes_of(saved.handlers).len; }
fn identity(saved: const Saved&) -> i64 { var id = source::reserve_identity(); while id == saved.heap.header.base || id == saved.heap.header.checkpoint { id = source::reserve_identity(); } return id; }
// Measurement control: a single retained heap, no snapshots or execution
// threads. The bounded benchmark's pure shared helper is emitted on demand by
// the actual compiler, then evaluated and retained in this heap. This control
// does not admit nested macro jobs while waiting for that helper's source.
fn serial_call(directory: const u8[:], id: i64, code: source::Code&, heap: source::Heap&, rt: source::Value, function: source::Value, args: const source::Value[:], prefix: const u8[:], next: i64&) -> source::Value {
    let roots = source::get(heap, heap.global, source::text(heap, "__lightCompilerRoots"));
    var job = source::Job { heap: heap, effects_enabled: true, helpers_enabled: true, root_receiver: roots }; defer { source::retire(job); } scope(job, prefix, next); defer { next = job.gensym_next; }
    assert(source::start(job, function, args)); var sequence: i64 = 0;
    loop {
        let outcome = source::run(job, 10000); if outcome == tiny::Outcome.Complete { return job.result; }
        assert(outcome != tiny::Outcome.Failed);
        if !job.effect_pending { continue; }
        assert(job.effect_kind == 9 && job.effect_count == 3);
        let name = job.effect_args[2]; assert(name.tag() == 4); var text: u8[>..] = [];
        for u in source::utf16(heap, name) { assert(u > 32 && u <= 127); text.push(u as u8); }
        print("NEED ", id, " ", sequence, " ", text, " ", job.steps, " ", job.writes.len); flush_stdout();
        var notice: u8[>..] = []; assert(read_line(notice)); assert(notice == "-2");
        var bytes:  u8[>..] = []; assert(read_file(str(directory, "/", id, "-", sequence, ".key"), bytes));
        let key, scoped = helper_key(bytes);
        if key == -1 { assert(source::deliver_effect(job, source::Value.Undefined, source::Fault.HostEffect)); sequence += 1; continue; }
        let entry = append(code, heap, str(directory, "/", id, "-", sequence, ".helper")); assert(entry >= 0);
        var produced = helper_next(scoped); let factory, loaded = leaf(id, heap, source::closure(heap, entry, []), [], 0, scoped, produced); assert(loaded);
        let value, done = leaf(id, heap, factory, [rt], 0, scoped, produced); assert(done);
        source::put(heap, job.effect_args[0], job.effect_args[2], value); assert(source::deliver_effect(job, value));
        print("PRODUCED ", id, " ", key); print("RESUME ", id); flush_stdout(); sequence += 1;
    }
}
fn serial(directory: const u8[:], code: source::Code&, heap: source::Heap&, rt: source::Value, instrumented: bool) {
    var definitions: SerialDefinition[>..] = [];
    loop {
        var line: u8[>..] = []; assert(read_line(line)); let id, ok = parse_int(line); assert(ok && id >= 0); if id == 0 { break; }
        print("START ", id); flush_stdout(); let started = clock_ns(); let key, prefix, first = definition(directory, id); var next = first; var function = source::Value.Undefined; var found = false;
        for d in definitions { if d.key == key { function = d.function; found = true; break; } }
        if !found { let entry = append(code, heap, str(directory, "/definition-", key, ".gsp")); assert(entry >= 0);
            let factory, a = initialize(id, heap, source::closure(heap, entry, []), [], 0, prefix, next); if !a { continue; } let value, b = initialize(id, heap, factory, [rt], 0, prefix, next); if !b { continue; } function = value; definitions.push(SerialDefinition { key: key, function: function }); print("PREPARED ", id, " ", key); flush_stdout(); }
        let loaded = clock_ns();
        var data: u8[>..] = []; assert(read_file(str(directory, "/", id, ".gfb"), data)); var graph = bridge::Graph {}; var c = bridge::Control {}; assert(bridge::decode(graph, data, c)); var args: source::Value[>..] = []; assert(bridge::import_graph(heap, graph, args, c));
        let ready = clock_ns(); let value = serial_call(directory, id, code, heap, rt, function, args, prefix, next); let invoked = clock_ns();
        var output = bridge::Graph {}; var bytes: u8[>..] = []; assert(bridge::export_graph(heap, [value], output, c, args[0]) && bridge::encode(output, bytes, c)); assert(write_file(str(directory, "/", id, ".out"), bytes));
        let empty = Saved { storage: instrumented }; print("METRIC ", id, " 0 ", invoked - ready, " ", clock_ns() - started, " ", storage(empty, heap, snapshot::Control {}, bytes.len), " 0");
        print("COST ", id, " ", loaded - started, " ", ready - loaded + clock_ns() - invoked); print("FINISH ", id, " 0 0"); if next > 0 { print("DONE ", id, " ", next); } else { print("DONE ", id); } flush_stdout();
    }
    print("CLOSED 1"); flush_stdout();
}
// The executing (running) and blocked counts the coordinator admits against:
// at most `limit` running, at most `retention` blocked, at most `reapers`
// live (spawned and not yet reaped). idle: workers waiting for a job, less
// the jobs queued for them.
struct Counts { live: i64 = 0, blocked: i64 = 0, reapers: i64 = 0, retention: i64 = 0, idle: i64 = 0 }
// BBP_RETAINED_WORKERS sets the retention budget: how many job workers may
// block in place on a dependency. 0 saves every wait as a continuation.
fn retained_workers() -> i64 {
    var text: u8[>..] = []; if !env("BBP_RETAINED_WORKERS", text) { return 32; }
    let n, ok = parse_int(text); assert(ok && n >= 0 && n <= 128); return n;
}
// BBP_WORKER_TABLE_BYTES: a job worker whose VM tables end a job above this
// many bytes retires (0: after every job, the differential control).
fn worker_table_bytes() -> i64 {
    // 16 MiB (the user's decision, light-cljs-791): a reused worker keeps its
    // data stacks' high water, so the cap bounds what idle workers hold. On
    // the 200k project no job reaches it, so a session spawns one worker per
    // execution slot and retires none, for about a fifth more runner peak
    // memory than retiring after every job (evidence, "Reused job workers at
    // a 16 MiB cap"); 1.25 MiB stayed within 5% but retired 1,282 workers.
    var text: u8[>..] = []; if !env("BBP_WORKER_TABLE_BYTES", text) { return 16777216; }
    let n, ok = parse_int(text); assert(ok && n >= 0); return n;
}
// Whether a job can be given to a worker now: an idle one, or a new one.
fn available(counts: const Counts&) -> bool { return counts.idle > 0 || counts.live < counts.reapers; }
// The coordinator's job workers: their threads by serial, the base image they
// all inherit and the configuration they take.
struct Pool { setup: Setup = Setup {}, directory: u8[] = [], threads: i64[>..] = [] }
// Give a job to an idle worker, or spawn one for it (light-cljs-795). Every
// worker takes jobs from the one Work queue, so no job is routed: after its
// reset any idle worker is a fresh one.
fn dispatch(work: Work&, base: const snapshot::Snapshot&, pool: Pool&, counts: Counts&, stats: Jobs&) {
    assert(available(counts));
    stats.argument_bytes += footprint(work.saved) + work.helper.len; stats.jobs += 1; work.spawned = clock_ns();
    if counts.idle == 0 {
        var setup = pool.setup; setup.worker = pool.threads.len; setup.identity = source::reserve_identity();
        let spawning = clock_ns(); let thread = thread_spawn_contained(run_worker, copy(base), copy(pool.directory), setup);
        stats.spawn_ns += clock_ns() - spawning; stats.spawned += 1; stats.argument_bytes += snapshot::footprint(base); counts.live += 1; counts.idle += 1; stats.peak_live = max(stats.peak_live, counts.live);
        pool.threads.push(thread); assert(qput(Reap { thread: thread, address: setup.worker }));
    }
    counts.idle -= 1; assert(qput(work));
}
// Resume a waiter whose dependency is answered: a blocked worker gets an
// Answer, a saved continuation a worker. False when no worker may take it
// now; the waiter then stays active for a later turn.
fn resume(w: Waiting&, mode: i64, helper: const u8[:], key: i64, is_callable: bool, running: Running[>..]&, base: const snapshot::Snapshot&, pool: Pool&, counts: Counts&, stats: Jobs&) -> bool {
    let checkpoint = source::reserve_identity();
    if w.address != 0 {
        assert(qput(Answer { address: w.address, mode: mode, helper: helper, key: key, callable: is_callable, checkpoint: checkpoint }));
        running.push(Running { id: w.id, thread: w.thread, address: w.address, checkpoint: checkpoint }); counts.blocked -= 1; stats.answers += 1;
    } else {
        if !available(counts) { return false; }
        var work = Work { saved: copy(w.saved), id: w.id, identity: identity(w.saved), mode: mode, helper: helper, key: key, checkpoint: checkpoint, callable: is_callable, blocking: counts.retention > 0 };
        dispatch(work, base, pool, counts, stats); running.push(Running { id: w.id, address: checkpoint, checkpoint: checkpoint });
    }
    print("RESUME ", w.id); flush_stdout(); w.active = false; return true;
}
// A saved continuation the coordinator refuses holds delay claims no job
// will settle: release them, so their waiters force again (light-cljs-808).
fn abandon(event: const Event&) { if event.kind == 1 && event.saved.claims > 0 { cells::exchange(cells::Request { op: 7, holder: event.id }); } }
// Whether the cell owner reported that `cell`'s claim `epoch` ended.
fn claim_ended(settled: const cells::Settled[:], cell: i64, epoch: i64) -> bool { for s in settled { if s.cell == cell && s.epoch == epoch { return true; } } return false; }
// Whether a saved continuation still waits (for its answer or, cancelling,
// for its cleanup).
fn saved_waiters(waiting: const Waiting[:]) -> bool { for w in waiting { if w.active && w.address == 0 { return true; } } return false; }
// A blocked waiter left without an answer (cancellation or abort): release
// its retention slot.
fn unblock(waiting: Waiting[>..]&, address: i64, thread: i64, counts: Counts&) {
    for w in waiting { if w.active && w.address != 0 && (w.address == address || w.thread == thread) { w.active = false; counts.blocked -= 1; } }
}
// The profile mode's restore, in a thread program of its own.
struct Restored { restore_ns: i64, copied_bytes: i64 }
thread_fn profile_restore(base: snapshot::Snapshot, identity: i64) {
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; var restore = snapshot::Control {};
    assert(snapshot::restore(base, code, heap, bindings, identity, restore)); assert(qput(Restored { restore_ns: restore.restore_ns, copied_bytes: restore.copied_bytes }));
}
// The coordinator builds the base image and no other heap; preparation and
// every job run in thread programs it spawns.
fn main() {
    let arguments = args(); assert(arguments.len == 3 || arguments.len == 4); var directory: u8[>..] = []; directory.append(arguments[1]); let limit, valid = parse_int(arguments[2]); assert(valid && limit >= 1 && limit <= 8);
    let beginning = clock_ns(); var code = source::Code {}; assert(load(code, str(directory, "/base.gsp")) >= 0); let loaded = clock_ns(); var heap = source::Heap { code: code }; assert(source::initialize(heap)); let rt = source::boot_runtime(heap); let roots = source::allocate(heap, source::Node { kind: 0 }); assert(source::install_field(heap, heap.global, "__lightCompilerRoots", roots));
    let published = source::allocate(heap, source::Node { kind: 0 }); assert(source::install_field(heap, heap.global, "__lightPublishedHelpers", published));
    let instrumented = arguments.len == 4 && (arguments[3] == "instrumented" || arguments[3] == "serial-instrumented");
    let booted = clock_ns(); if arguments.len == 4 && (arguments[3] == "serial" || arguments[3] == "serial-instrumented") { assert(limit == 1); print("READY 1 ", heap.boot_count, " ", loaded - beginning, " ", booted - loaded, " 0"); flush_stdout(); serial(directory, code, heap, rt, instrumented); return; }
    var sc = snapshot::Control {}; let base = snapshot::capture(heap, [snapshot::Binding { id: 0, generation: 1, value: rt }], sc); assert(sc.error == snapshot::Error.None);
    if arguments.len == 4 && arguments[3] == "profile" {
        var checksum: i64 = 0; let barrier_started = clock_ns(); for i in 1000 { checksum += consume(base); } let barrier_ns = clock_ns() - barrier_started; let started = clock_ns();
        for i in 1000 { var copied = copy(base); copied.header.work += i; checksum += consume(copied); }
        let copy_ns = clock_ns() - started; let validating = clock_ns(); var validation = snapshot::Control {}; assert(snapshot::validate(base, validation)); let validation_ns = clock_ns() - validating;
        assert(thread_wait(thread_spawn(profile_restore, copy(base), source::reserve_identity()))); let restore = qget<Restored>();
        print("{:snapshot-bytes ", snapshot::footprint(base), " :capture-copy-bytes ", sc.copied_bytes, " :capture-ns ", sc.capture_ns, " :raw-copy-repeats 1000 :raw-copy-ns ", copy_ns, " :raw-barrier-only-ns ", barrier_ns, " :raw-copy-payload-bytes ", snapshot::payload_bytes(base) * 1000, " :validation-ns ", validation_ns, " :validation-visits ", validation.visited, " :validation-peak-bytes ", validation.validation_peak, " :restore-ns ", restore.restore_ns, " :restore-copy-bytes ", restore.copied_bytes, " :peak-rss-native ", peak_memory(), " :checksum ", checksum, "}"); return;
    }
    let deterministic = arguments.len == 4 && arguments[3] == "dispatch-test"; let cleanup_test = arguments.len == 4 && arguments[3] == "cleanup-abort-test"; let tracing = instrumented || deterministic || cleanup_test || (arguments.len == 4 && arguments[3] == "readiness-test");
    let retention = retained_workers(); var counts = Counts { reapers: limit + retention, retention: retention }; var stats = Jobs {};
    // The reset check (light-cljs-795): in the test modes and with
    // BBP_RESET_CHECK=1, every worker compares its image with the twin after
    // every reset, and its digest with the coordinator's tables'. A traced
    // measurement (readiness-test mode) turns it off with BBP_RESET_CHECK=0.
    var checking: u8[>..] = []; let configured = env("BBP_RESET_CHECK", checking);
    let check = deterministic || cleanup_test || configured && checking == "1" || !configured && arguments.len == 4 && arguments[3] == "readiness-test";
    var pool = Pool { setup: Setup { deterministic: deterministic, cleanup_test: cleanup_test, cap: worker_table_bytes(), check: check }, directory: copy(directory) };
    let image = if check { heap_digest(heap, [snapshot::Binding { id: 0, generation: 1, value: rt }]) } else { 0 };
    // Each live worker has at most four unconsumed events (a blocked wait,
    // its final event, the end of its job and its reaper's), and the
    // coordinator admits no worker past the reaper count, so these limits
    // never refuse a publish.
    // Each live worker also has at most one unconsumed Answer (a resume or,
    // over the retention budget, a save), so the re-queue never fails either.
    qlimit<Command>(128); qlimit<readiness::InputCredit>(128); for i in 128 { assert(qput(readiness::InputCredit {})); } qlimit<Event>(6 * counts.reapers + 8); qlimit<Answer>(counts.reapers + 1); qlimit<Reap>(counts.reapers); qlimit<Cancel>(1); qlimit<readiness::Wake>(1); cells::prepare(); let owner = thread_spawn(cell_owner, cells::budget()); cells::enabled = true; source::cell_base = base.header; let reader = thread_spawn(input, copy(directory), deterministic); var running: Running[>..] = []; var queue: i64[>..] = []; var received: i64 = 0; var waiting: Waiting[>..] = []; var produced: Produced[>..] = []; var prepared: Prepared[>..] = []; var closing = false; var cancelling = false; var poisoned = false; var retained: i64 = 0; var sleep_ns: i64 = 0; var turns: i64 = 0; var progress_turns: i64 = 0; var backoffs: i64 = 0; var commands: i64 = 0; var events: i64 = 0; var max_commands: i64 = 0; var max_events: i64 = 0; var exits: Exit[>..] = []; var finals: i64 = 0; var since_trim: i64 = 0; var settled: cells::Settled[>..] = [];
    var reapers: i64[>..] = []; for slot in counts.reapers { reapers.push(thread_spawn(supervise)); }
    print("READY 1 ", heap.boot_count, " ", loaded - beginning, " ", booted - loaded, " ", clock_ns() - booted); flush_stdout();
    // The deterministic fixture queues a complete batch before the first turn.
    if deterministic { var bytes: u8[>..] = []; while !read_file(str(directory, "/commands-ready"), bytes) { sleep_ms(1); } }
    while !closing || counts.live > 0 || saved_waiters(waiting) {
        var progress = false; var command_count: i64 = 0; var event_count: i64 = 0;
        readiness::clear();
        loop { let command, available = qpoll<Command>(); if !available { break; } assert(qput(readiness::InputCredit {})); progress = true; if command.id == -2 { continue; } command_count += 1; commands += 1; if command.id <= 0 { closing = true; } else if !closing { if queue.len >= 1024 { print("ERROR ", command.id, " command-budget"); flush_stdout(); } else { queue.push(command.id); } } }
        // Test mode waits for this bounded batch's real events, ensuring the
        // multi-completion drain is exercised independently of OS scheduling.
        if deterministic { for r in running { var ready: u8[>..] = []; while !read_file(str(directory, "/ready-", r.checkpoint), ready) { sleep_ms(1); } } }
        loop { let event, present = qpoll<Event>(); if !present { break; } progress = true;
            if event.kind == 6 { counts.live -= 1;
                for x, i in exits { if x.address == event.address { stats.join_ns += event.timing[1] - x.clock; exits.swap_remove(i); break; } }
                if !event.aborted { continue; }
                var reason: u8[>..] = []; for b in event.name { if reason.len >= 1024 { break; } reason.push(if b < 32 { ' ' } else { b }); } for r, i in running { if r.thread == event.key { running.swap_remove(i); break; } }
                unblock(waiting, -1, event.key, counts); closing = true; poisoned = true;
                if !cancelling { assert(qput(Cancel {})); cells::halt(); cancelling = true; }
                print("FAULT 0 worker-abort ", reason); flush_stdout(); continue; }
            // A worker's job ended: it is idle, or it retired and exits.
            if event.kind == 7 { stats.reset_ns += event.timing[0]; stats.restore_ns += event.installs[0]; stats.install_ns += event.installs[1]; stats.installs += event.installs[2];
                // Every worker, whenever spawned, inherits the same image.
                if check && event.image != image { print("FAULT 0 worker-image ", event.worker); flush_stdout(); assert(false); }
                if event.retired { stats.retired += 1; exits.push(Exit { address: event.worker, clock: event.timing[1] }); }
                else if closing && !saved_waiters(waiting) { assert(qput(Work { stop: true })); } else { counts.idle += 1; }
                continue; }
            if event.kind != 10 && event.timing[1] > 0 { finals += 1; stats.start_ns += event.timing[0]; stats.restore_bytes += event.timing[2]; }
            var owner, owned = release(running, event.address); if owned && event.worker >= 0 { owner.thread = pool.threads[event.worker]; }
            // Only a blocked waiter publishes while not running: its own
            // cancellation, which is no job result; its report is traced.
            if !owned { unblock(waiting, event.address, -1, counts); if event.kind == 3 && !poisoned { print("ERROR ", event.id, " ", event.name); flush_stdout(); } continue; }
            event_count += 1; events += 1; if poisoned { continue; }
            // A dependency wait (10), or a delay wait (11, light-cljs-808),
            // which the cell owner's Settled ends instead of an answer.
            if event.kind == 10 || event.kind == 11 { stats.waits += 1;
                if waiting.len >= 128 { print("ERROR ", event.id, " waiter-budget"); flush_stdout(); assert(qput(Answer { address: event.address, mode: -1 })); continue; }
                if counts.blocked < retention { counts.blocked += 1; stats.peak_blocked = max(stats.peak_blocked, counts.blocked); waiting.push(Waiting { id: event.id, name: copy(event.name), saved: Saved { sequence: event.sequence, gensym_prefix: copy(event.saved.gensym_prefix), gensym_next: event.saved.gensym_next }, address: event.address, thread: owner.thread, delay: event.delay, epoch: event.epoch }); if event.kind == 11 { print("WAIT ", event.id, " ", event.delay); } else { print("NEED ", event.id, " ", event.sequence, " ", event.name, " ", event.steps, " ", event.writes); } flush_stdout(); continue; }
                // Over the retention budget: the worker saves its continuation and ends the job.
                let checkpoint = source::reserve_identity(); assert(qput(Answer { address: event.address, mode: 0, checkpoint: checkpoint })); running.push(Running { id: event.id, thread: owner.thread, address: event.address, checkpoint: checkpoint }); continue;
            }
            let bytes = if event.kind == 1 { footprint(event.saved) } else { event.data.len }; if bytes > 67108864 - retained { abandon(event); print("ERROR ", event.id, " retained-budget"); flush_stdout(); continue; } retained += bytes; if event.metrics.len > 0 { print("METRIC ", event.id, " ", event.metrics, " ", sleep_ns, " ", clock_ns() - beginning); } if event.kind == 2 || event.kind == 5 { print("COST ", event.id, " ", event.name); }
            if event.census[0] >= 0 { var text: u8[>..] = []; format(text, "CENSUS 0 ", event.id, " ", event.kind); for n in event.census { format(text, " ", n); } print(text); }
            if event.kind == 1 { stats.saves += 1; if waiting.len >= 128 { abandon(event); print("ERROR ", event.id, " waiter-budget"); } else { waiting.push(Waiting { id: event.id, name: copy(event.name), saved: copy(event.saved), delay: event.delay, epoch: event.epoch }); if event.delay >= 0 { print("WAIT ", event.id, " ", event.delay); } else { print("NEED ", event.id, " ", event.saved.sequence, " ", event.name, " ", event.steps, " ", event.saved.writes.len); } } }
            else if event.kind == 5 { produced.push(Produced { key: event.key, data: copy(event.data), callable: event.callable });
                // The helper numbered in its own frozen scope, not the wait's.
                print("PRODUCED ", event.id, " ", event.key); }
            else if event.kind == 2 { print("FINISH ", event.id, " ", event.steps, " ", event.writes); if event.gensym_next > 0 { print("DONE ", event.id, " ", event.gensym_next); } else { print("DONE ", event.id); } }
            else { print("ERROR ", event.id, " ", event.name); }
            flush_stdout();
        }
        if closing && !cancelling { assert(qput(Cancel {})); cells::halt(); cancelling = true; }
        // Cancelling: every saved continuation still waiting is restored in a
        // worker to run its finally blocks (mode 4, light-cljs-803), within the
        // execution slots, as the Pants contract has a cancelled task
        // suspended at a wait do (rust/src/task.rs, "Cancellation and finally
        // blocks"). Its report is traced; the session publishes nothing.
        if cancelling { for w in waiting { if !w.active || w.address != 0 { continue; } if running.len >= limit || !available(counts) { break; }
            let checkpoint = source::reserve_identity(); var work = Work { saved: copy(w.saved), id: w.id, identity: identity(w.saved), mode: 4, checkpoint: checkpoint };
            dispatch(work, base, pool, counts, stats); running.push(Running { id: w.id, address: checkpoint, checkpoint: checkpoint }); w.active = false; progress = true; } }
        // Closing: every idle worker exits once no saved continuation waits
        // for its cleanup; busy ones get a stop when idle.
        if closing && !saved_waiters(waiting) { while counts.idle > 0 { assert(qput(Work { stop: true })); counts.idle -= 1; } }
        // Delay claims that ended while jobs waited for them (light-cljs-808).
        loop { let ended, present = qpoll<cells::Settled>(); if !present { break; } settled.push(ended); progress = true; }
        if !closing { for w in waiting { if running.len >= limit { break; } if !w.active { continue; }
            if w.delay >= 0 { if claim_ended(settled, w.delay, w.epoch) && resume(w, 5, [], -1, false, running, base, pool, counts, stats) { progress = true; } continue; }
            if w.key == -2 { var bytes: u8[>..] = []; if !read_file(str(directory, "/", w.id, "-", w.saved.sequence, ".key"), bytes) { continue; } let key, prefix = helper_key(bytes); w.key = key; w.prefix.append(prefix); }
            let key = w.key; if key == -1 { if resume(w, 3, [], -1, false, running, base, pool, counts, stats) { progress = true; } continue; } var found = false;
            for p in produced { if p.key == key { found = true; if resume(w, 1, p.data, key, p.callable, running, base, pool, counts, stats) { progress = true; } break; } }
            if found { continue; }
            var producing = false; for r in running { if r.key == key { producing = true; } }
            if !producing && available(counts) { let initial = Saved { heap: copy(base), base: base.header, sequence: w.saved.sequence, protected: base.nodes.len, storage: instrumented, gensym_prefix: w.prefix[..], gensym_next: helper_next(w.prefix[..]) }; let checkpoint = source::reserve_identity(); var work = Work { saved: copy(initial), id: w.id, identity: identity(initial), mode: 2, helper: [], key: key, checkpoint: checkpoint }; dispatch(work, base, pool, counts, stats); running.push(Running { id: w.id, address: checkpoint, key: key, checkpoint: checkpoint }); print("PRODUCE ", w.id, " ", key); flush_stdout(); progress = true; }
        } }
        // Ready continuations/helpers get first refusal; new jobs fill every remaining slot.
        while !closing && received < queue.len && running.len < limit && available(counts) { progress = true; let id = queue[received]; received += 1; let key, prefix, first = definition(directory, id); var next = first; var index: i64 = -1;
            for p, i in prepared { if p.key == key { index = i; break; } }
            if index < 0 { let checkpoint, ok = prepare(base, directory, id, key, prefix, next); if !ok { continue; } let bytes = snapshot::footprint(checkpoint); if prepared.len >= 1024 || bytes > 67108864 - retained { print("ERROR ", id, " definition-budget"); flush_stdout(); continue; } retained += bytes; index = prepared.len; prepared.push(Prepared { key: key, heap: copy(checkpoint) }); }
            for p in prepared { if p.key != key { continue; } let handing = clock_ns(); let spawning = stats.spawn_ns; let initial = Saved { heap: copy(p.heap), base: base.header, protected: base.nodes.len, storage: instrumented, gensym_prefix: copy(prefix), gensym_next: next }; let checkpoint = source::reserve_identity(); var work = Work { saved: copy(initial), id: id, identity: identity(initial), mode: 0, helper: [], key: -1, checkpoint: checkpoint, blocking: retention > 0, definition: key }; dispatch(work, base, pool, counts, stats); stats.handoff_ns += clock_ns() - handing - (stats.spawn_ns - spawning); running.push(Running { id: id, address: checkpoint, checkpoint: checkpoint }); since_trim += 1; print("START ", id); flush_stdout(); break; } }
        // Release inactive continuation owners and their budget after dispatch.
        { var inactive: i64 = 0; for w in waiting { if !w.active { inactive += 1; if w.address == 0 { retained -= footprint(w.saved); } } } if inactive == waiting.len { waiting.clear(); } else if inactive > 0 { var active: Waiting[>..] = []; for w in waiting { if w.active { active.push(copy(w)); } } waiting = copy(active); } }
        if received == queue.len { queue.clear(); received = 0; }
        // Instrumented runs measure trim_memory() once the coordinator is idle
        // after a batch of jobs (light-cljs-784); other runs never call it.
        if instrumented && since_trim > 0 && running.len == 0 && queue.len == 0 && !closing {
            let before = resident_memory(); let trimming = clock_ns(); trim_memory(); stats.trim_ns += clock_ns() - trimming; stats.trim_before += before; stats.trim_after += resident_memory(); stats.trims += 1; since_trim = 0;
        }
        turns += 1; max_commands = max(max_commands, command_count); max_events = max(max_events, event_count);
        // A progress turn immediately rechecks all sources. Atomic blocking
        // receive consumes readiness arriving before/during/after this point.
        // Stale or repeated notices only cause another bounded source scan.
        if progress { progress_turns += 1; } else if !closing || counts.live > 0 { if tracing { print("PARK 0 ", backoffs + 1); flush_stdout(); } let sleeping = clock_ns(); readiness::park(); sleep_ns += clock_ns() - sleeping; backoffs += 1; }
        if tracing { print("SCHED 0 ", turns, " ", progress_turns, " ", backoffs, " ", sleep_ns, " ", 2 + counts.reapers + stats.spawned, " ", 2 + counts.reapers + stats.peak_live, " ", stats.spawned - counts.live, " ", commands, " ", events, " ", max_commands, " ", max_events); flush_stdout(); }
    }
    // Every job worker has been reaped; the reapers are idle.
    for i in counts.reapers { assert(qput(Reap { thread: -1 })); }
    for thread in reapers { assert(thread_wait(thread)); }
    // No job runs any more: the cell owner reports and exits.
    let cell_stats = cells::stop(); assert(thread_wait(owner));
    assert(thread_wait(reader));
    // The coordinator's VM tables did not change after READY.
    if check { assert(image == heap_digest(heap, [snapshot::Binding { id: 0, generation: 1, value: rt }])); }
    if tracing {
        // spawned counts job workers; jobs the jobs they ran, retired the
        // workers that left at the table-byte cap (light-cljs-795).
        print("JOBS 0 ", retention, " ", stats.spawned, " ", finals, " ", stats.spawn_ns, " ", stats.start_ns, " ", stats.join_ns, " ", stats.argument_bytes, " ", stats.restore_bytes, " ", stats.peak_live, " ", stats.peak_blocked, " ", stats.waits, " ", stats.saves, " ", stats.answers, " ", counts.live, " ", counts.blocked, " ", stats.jobs, " ", stats.retired, " ", stats.reset_ns, " ", stats.restore_ns, " ", stats.install_ns, " ", stats.installs, " ", stats.handoff_ns, " ", cell_stats.cells, " ", cell_stats.identities, " ", cell_stats.operations, " ", cell_stats.conflicts, " ", cell_stats.waits, " ", cell_stats.owner_ns, " ", cell_stats.bytes);
        if instrumented { print("TRIM 0 ", stats.trims, " ", stats.trim_before, " ", stats.trim_after, " ", stats.trim_ns); }
        // The coordinator, the input thread, the reapers and every job worker;
        // all but the coordinator are joined. Preparers (one per PREPARED or
        // preparation ERROR line, each joined before the coordinator goes on)
        // and the profile restore are not job-path threads and are not counted.
        print("THREADS 0 ", 3 + counts.reapers + stats.spawned, " ", 3 + counts.reapers + stats.peak_live, " ", 2 + counts.reapers + stats.spawned);
    }
    print("CLOSED 1"); flush_stdout();
}
