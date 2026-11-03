import std;
import os;
import .source_vm;
import .program;
import .snapshot;
import .form_bridge;
import .vm;

// GCS1: one input thread + one coordinator + at most limit execution threads.
// A wait returns a flat continuation and joins its thread before the compiler
// callback. Nested compiler work therefore also works with limit=1.
struct Saved {
    heap:   snapshot::Snapshot = snapshot::Snapshot {}, frames: source::Frame[] = [], locals: source::Value[] = [], operands: source::Value[] = [], writes: source::Write[] = [], handlers: source::Handler[] = [],
    steps:  i64                = 0, args: source::Value[8] = [source::Value {}; 8], frame: source::Frame = source::Frame { function: -1, pc: -1, local_base: 0, operand_base: 0, callee: source::Value {}, receiver: source::Value {} },
    bridge: bridge::Control    = bridge::Control {}, sequence: i64 = 0, costs: i64[2] = [0; 2],
}
struct Command { id: i64 }
struct Event { id: i64, kind: i64, saved: Saved = Saved {}, sequence: i64 = 0, key: i64 = -1, name: u8[] = [], data: u8[] = [], steps: i64 = 0, writes: i64 = 0 }
struct Running { id: i64, thread: i64, key: i64 = -1 }
struct Waiting { id: i64, name: u8[], saved: Saved, key: i64 = -2, active: bool = true }
struct Produced { key: i64, data: u8[] }
thread_fn input() {
    loop { var line: u8[>..] = []; if !read_line(line) { assert(qput(Command { id: 0 })); return; }
        let id, ok = parse_int(line); if !ok || id < 0 { assert(qput(Command { id: -1 })); return; }
        while !qput(Command { id: id }) { sleep_ms(1); } if id == 0 { return; }
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
    for c in code.constants[first..] { var value = c; if c.tag == 4 { value = source::string(heap, code.units[c.a..(c.a + c.b)]); } heap.constants.push(value); }
    return entry;
}
fn leaf(heap: source::Heap&, callee: source::Value, args: const source::Value[:]) -> source::Value {
    var job = source::Job { heap: heap }; defer { source::retire(job); }
    source::start(job, callee, args); if source::run(job, -1) != tiny::Outcome.Complete { heap.fault = job.fault; return source::Value {}; } return job.result;
}
fn relocated(f: source::Frame, identity: i64) -> source::Frame {
    var out = f; out.callee = snapshot::relocated(f.callee, identity); out.receiver = snapshot::relocated(f.receiver, identity); out.input = snapshot::relocated(f.input, identity); out.output = snapshot::relocated(f.output, identity); return out;
}
fn save(job: source::Job&, rt: source::Value, call: source::Value, control: bridge::Control, sequence: i64, costs: i64[2]) -> Saved {
    assert(!job.claimed && job.effect_pending && job.status == tiny::Status.Ready && job.effect_kind == 9 && job.effect_count == 3);
    assert(source::retire(job)); var c = snapshot::Control {}; let heap = snapshot::capture(job.heap, [snapshot::Binding { id: 0, generation: 1, value: rt }, snapshot::Binding { id: 1, generation: 1, value: call }], c); assert(c.error == snapshot::Error.None);
    return Saved { heap: copy(heap), frames: copy(job.frames), locals: copy(job.locals), operands: copy(job.operands), writes: copy(job.writes), handlers: copy(job.handlers), steps: job.steps, args: job.effect_args, frame: job.effect_frame, bridge: control, sequence: sequence, costs: costs };
}
fn fail(id: i64, heap: source::Heap&, job: source::Job&) {
    let pc = job.fault_pc; var start: i64 = -1; var end: i64 = -1;
    if pc >= 0 && pc < heap.code.ops.len { start = heap.code.ops[pc].start; end = heap.code.ops[pc].end; }
    let text = str(job.fault, " function=", job.fault_function, " pc=", pc, " emitted-span=", start, ":", end);
    assert(qput(Event { id: id, kind: 3, name: copy(text) }));
}
thread_fn worker(saved: Saved, directory: u8[>..], id: i64, identity: i64, mode: i64, helper: u8[], key: i64) {
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; var sc = snapshot::Control {};
    assert(snapshot::restore(saved.heap, code, heap, bindings, identity, sc)); let rt = bindings[0].value;
    var costs = saved.costs; var control = saved.bridge; var args: source::Value[>..] = []; var call = source::Value {}; var function = source::Value {};
    if mode == 0 || mode == 2 {
        let path = if mode == 0 { str(directory, "/", id, ".gsp") } else { str(directory, "/", id, "-", saved.sequence, ".helper") }; let loading = clock_ns(); let entry = append(code, heap, path); assert(entry >= 0); costs[0] += clock_ns() - loading;
        if mode == 0 { let importing = clock_ns();
            var bytes: u8[>..] = []; assert(read_file(str(directory, "/", id, ".gfb"), bytes)); var graph = bridge::Graph {}; assert(bridge::decode(graph, bytes, control)); assert(bridge::import_graph(heap, graph, args, control)); assert(args.len >= 2); call = args[0]; costs[1] += clock_ns() - importing;
        }
        let factory = leaf(heap, source::closure(heap, entry, []), []); function = leaf(heap, factory, [rt]);
        if mode == 2 { let exporting = clock_ns();
            var graph = bridge::Graph {}; var bytes: u8[>..] = []; if !bridge::export_graph(heap, [function], graph, control) || !bridge::encode(graph, bytes, control) { assert(qput(Event { id: id, kind: 3, name: str("helper-", heap.fault, "-", control.error) })); return; } costs[1] += clock_ns() - exporting; assert(qput(Event { id: id, kind: 5, key: key, name: str(costs[0], " ", costs[1]), data: copy(bytes) })); return;
        }
    } else { call = bindings[1].value; }
    var job = source::Job { heap: heap, effects_enabled: true, helpers_enabled: true }; defer { source::retire(job); }
    if mode == 0 { source::start(job, function, args); } else {
        for f in saved.frames { job.frames.push(relocated(f, identity)); } for v in saved.locals { job.locals.push(snapshot::relocated(v, identity)); } for v in saved.operands { job.operands.push(snapshot::relocated(v, identity)); }
        for w in saved.writes { job.writes.push(source::Write { object: snapshot::relocated(w.object, identity), key: snapshot::relocated(w.key, identity), value: snapshot::relocated(w.value, identity) }); }
        for h in saved.handlers { job.handlers.push(h); } job.steps = saved.steps; job.registered = true; heap.live_jobs += 1; job.effect_pending = true; job.effect_kind = 9; job.effect_count = 3; job.effect_frame = relocated(saved.frame, identity); for v, i in saved.args { job.effect_args[i] = snapshot::relocated(v, identity); }
        var graph = bridge::Graph {}; var values: source::Value[>..] = [];
        if mode == 3 || !bridge::decode(graph, helper, control) || !bridge::import_graph(heap, graph, values, control) || values.len != 1 { source::deliver_effect(job, source::Value {}, source::Fault.HostEffect); } else { source::put(heap, job.effect_args[0], job.effect_args[2], values[0]); assert(source::deliver_effect(job, values[0])); }
    }
    loop {
        let outcome = source::run(job, 10000); if outcome == tiny::Outcome.Complete { break; } if outcome == tiny::Outcome.Failed { fail(id, heap, job); return; }
        if !job.effect_pending { continue; }
        if job.effect_kind != 9 || job.effect_count != 3 { source::deliver_effect(job, source::Value {}, source::Fault.NativeBoundary); continue; }
        let v = job.effect_args[2]; var name: u8[>..] = []; if v.tag != 4 { source::deliver_effect(job, source::Value {}, source::Fault.TypeError); continue; }
        for u in heap.units[v.a..(v.a + v.b)] { if u <= 32 || u > 127 { source::deliver_effect(job, source::Value {}, source::Fault.NativeBoundary); break; } name.push(u as u8); } if !job.effect_pending { continue; }
        let state = save(job, rt, call, control, if mode == 0 { 0 } else { saved.sequence + 1 }, costs); assert(qput(Event { id: id, kind: 1, saved: copy(state), name: copy(name), steps: job.steps })); return;
    }
    let exporting = clock_ns(); let result = job.result; let steps = job.steps; assert(source::retire(job)); var graph = bridge::Graph {}; var bytes: u8[>..] = [];
    if !bridge::export_graph(heap, [result], graph, control, call) || !bridge::encode(graph, bytes, control) { assert(qput(Event { id: id, kind: 3, name: str("bridge-", control.error) })); return; }
    assert(write_file(str(directory, "/", id, ".out"), bytes)); costs[1] += clock_ns() - exporting; assert(qput(Event { id: id, kind: 2, name: str(costs[0], " ", costs[1]), steps: steps, writes: job.writes.len }));
}
fn join(running: Running[>..]&, id: i64) { for r, i in running { if r.id == id { var reason: u8[>..] = []; assert(thread_wait(r.thread, reason)); running.swap_remove(i); return; } } assert(false); }
fn identity(saved: const Saved&) -> i64 { var id = source::reserve_identity(); while id == saved.heap.header.base || id == saved.heap.header.checkpoint { id = source::reserve_identity(); } return id; }
fn main() {
    let arguments = args(); assert(arguments.len == 3); var directory: u8[>..] = []; directory.append(arguments[1]); let limit, valid = parse_int(arguments[2]); assert(valid && limit >= 1 && limit <= 8);
    let beginning = clock_ns(); var code = source::Code {}; assert(load(code, str(directory, "/base.gsp")) >= 0); let loaded = clock_ns(); var heap = source::Heap { code: code }; assert(source::initialize(heap)); let rt = source::boot_runtime(heap); let roots = source::allocate(heap, source::Node { kind: 0 }); assert(source::install_field(heap, heap.global, "__lightCompilerRoots", roots));
    let booted = clock_ns(); var sc = snapshot::Control {}; let base = snapshot::capture(heap, [snapshot::Binding { id: 0, generation: 1, value: rt }], sc); assert(sc.error == snapshot::Error.None);
    qlimit<Command>(128); qlimit<Event>(16); let reader = thread_spawn(input); var running: Running[>..] = []; var queue: i64[>..] = []; var received: i64 = 0; var waiting: Waiting[>..] = []; var produced: Produced[>..] = []; var closing = false; var retained: i64 = 0;
    print("READY 1 ", heap.boot_count, " ", loaded - beginning, " ", booted - loaded, " ", clock_ns() - booted); flush_stdout();
    while !closing || running.len > 0 {
        let command, available = qpoll<Command>(); if available { if command.id <= 0 { closing = true; } else { assert(queue.len < 1024); queue.push(command.id); } }
        if !closing && received < queue.len && running.len < limit { let id = queue[received]; received += 1; let initial = Saved { heap: copy(base) }; let t = thread_spawn(worker, copy(initial), copy(directory), id, identity(initial), 0, [], -1); running.push(Running { id: id, thread: t }); print("START ", id); flush_stdout(); }
        let event, present = qpoll<Event>(); if present {
            join(running, event.id); let bytes = if event.kind == 1 { snapshot::footprint(event.saved.heap) + bytes_of(event.saved.frames).len + bytes_of(event.saved.locals).len + bytes_of(event.saved.operands).len + bytes_of(event.saved.writes).len + bytes_of(event.saved.handlers).len } else { event.data.len }; if bytes > 67108864 - retained { print("ERROR ", event.id, " retained-budget"); flush_stdout(); continue; } retained += bytes; if event.kind == 2 || event.kind == 5 { print("COST ", event.id, " ", event.name); }
            if event.kind == 1 { assert(waiting.len < 128); waiting.push(Waiting { id: event.id, name: copy(event.name), saved: copy(event.saved) }); print("NEED ", event.id, " ", event.saved.sequence, " ", event.name, " ", event.steps, " ", event.saved.writes.len); }
            else if event.kind == 5 { produced.push(Produced { key: event.key, data: copy(event.data) }); print("PRODUCED ", event.id, " ", event.key); }
            else if event.kind == 2 { print("FINISH ", event.id, " ", event.steps, " ", event.writes); print("DONE ", event.id); }
            else { print("ERROR ", event.id, " ", event.name); }
            flush_stdout();
        }
        if !closing && running.len < limit { for w in waiting { if !w.active { continue; }
            if w.key == -2 { var bytes: u8[>..] = []; if !read_file(str(directory, "/", w.id, "-", w.saved.sequence, ".key"), bytes) { continue; } let key, ok = parse_int(bytes); assert(ok && key >= -1); w.key = key; }
            let key = w.key; if key == -1 { let t = thread_spawn(worker, copy(w.saved), copy(directory), w.id, identity(w.saved), 3, [], -1); running.push(Running { id: w.id, thread: t }); print("RESUME ", w.id); flush_stdout(); w.active = false; break; } var found = false;
            for p in produced { if p.key == key { let t = thread_spawn(worker, copy(w.saved), copy(directory), w.id, identity(w.saved), 1, copy(p.data), key); running.push(Running { id: w.id, thread: t }); print("RESUME ", w.id); flush_stdout(); w.active = false; found = true; break; } }
            if found { break; }
            var producing = false; for r in running { if r.key == key { producing = true; } }
            if !producing { let initial = Saved { heap: copy(base), sequence: w.saved.sequence }; let t = thread_spawn(worker, copy(initial), copy(directory), w.id, identity(initial), 2, [], key); running.push(Running { id: w.id, thread: t, key: key }); print("PRODUCE ", w.id, " ", key); flush_stdout(); break; }
        } }
        sleep_ms(1);
    }
    assert(thread_wait(reader)); print("CLOSED 1"); flush_stdout();
}
