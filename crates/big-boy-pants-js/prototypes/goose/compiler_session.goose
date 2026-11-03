import std;
import os;
import .source_vm;
import .program;
import .snapshot;
import .form_bridge;
import .vm;

// GCS1: one stdin reader, one coordinator, bounded native execution jobs.
// Payloads are GSP2/GFB1 files in the compiler-owned session directory.
// Each reply/demand carries the original request ID; no pointer crosses here.
struct Command { id: i64 }
struct Event { id: i64, kind: i64, sequence: i64 = 0, name: u8[] = [] }
struct Running { id: i64, thread: i64 }
thread_fn input() {
    loop { var line: u8[>..] = []; if !read_line(line) { assert(qput(Command { id: 0 })); return; }
        let id, ok = parse_int(line); if !ok || id < 0 { assert(qput(Command { id: -1 })); return; }
        assert(qput(Command { id: id })); if id == 0 { return; }
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
fn invoke(code: source::Code&, heap: source::Heap&, rt: source::Value, callee: source::Value, args: const source::Value[:], directory: const u8[:], id: i64) -> source::Value {
    var job = source::Job { heap: heap, effects_enabled: true }; defer { source::retire(job); }
    source::start(job, callee, args); var sequence: i64 = 0;
    loop {
        let outcome = source::run(job, 10000);
        if outcome == tiny::Outcome.Complete { return job.result; }
        if outcome == tiny::Outcome.Failed { print("FAULT ", id, " ", job.fault, " function=", job.fault_function, " pc=", job.fault_pc); heap.fault = job.fault; return source::Value {}; }
        if !job.effect_pending { continue; }
        if job.effect_kind != 9 || job.effect_count != 3 { source::deliver_effect(job, source::Value {}, source::Fault.NativeBoundary); continue; }
        assert(!job.claimed); let key = job.effect_args[2]; var name: u8[>..] = [];
        if key.tag != 4 { source::deliver_effect(job, source::Value {}, source::Fault.TypeError); continue; }
        for u in heap.units[key.a..(key.a + key.b)] { if u > 127 || u <= 32 { source::deliver_effect(job, source::Value {}, source::Fault.NativeBoundary); break; } name.push(u as u8); }
        if !job.effect_pending { continue; }
        let path = str(directory, "/", id, "-", sequence, ".helper");
        assert(qput(Event { id: id, kind: 1, sequence: sequence, name: copy(name) }));
        // No execution claim is held while the compiler performs nested analysis.
        // The compiler writes the file atomically after the nested request ends.
        var reply: u8[>..] = []; while !read_file(path, reply) { sleep_ms(1); }
        let entry = append(code, heap, path);
        if entry < 0 { source::deliver_effect(job, source::Value {}, source::Fault.HostEffect); continue; }
        let factory = leaf(heap, source::closure(heap, entry, []), []);
        let value = leaf(heap, factory, [rt]);
        if heap.fault != source::Fault.None { source::deliver_effect(job, source::Value {}, heap.fault); continue; }
        let receiver = job.effect_args[0]; source::put(heap, receiver, key, value);
        assert(source::deliver_effect(job, value)); sequence += 1;
    }
}
thread_fn worker(base: snapshot::Snapshot, directory: u8[>..], id: i64, identity: i64) {
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = [];
    var sc = snapshot::Control {}; assert(snapshot::restore(base, code, heap, bindings, identity, sc)); let rt = bindings[0].value;
    let entry = append(code, heap, str(directory, "/", id, ".gsp")); assert(entry >= 0);
    var bytes: u8[>..] = []; assert(read_file(str(directory, "/", id, ".gfb"), bytes));
    var graph = bridge::Graph {}; var control = bridge::Control {}; assert(bridge::decode(graph, bytes, control));
    var args: source::Value[>..] = []; assert(bridge::import_graph(heap, graph, args, control)); assert(args.len >= 2);
    let factory = leaf(heap, source::closure(heap, entry, []), []); let function = leaf(heap, factory, [rt]);
    let result = invoke(code, heap, rt, function, args, directory, id);
    if heap.fault != source::Fault.None { assert(qput(Event { id: id, kind: 3 })); return; }
    var out = bridge::Graph {}; var encoded: u8[>..] = [];
    if !bridge::export_graph(heap, [result], out, control, args[0]) || !bridge::encode(out, encoded, control) { assert(qput(Event { id: id, kind: 3 })); return; }
    assert(write_file(str(directory, "/", id, ".out"), encoded)); assert(qput(Event { id: id, kind: 2 }));
}
fn main() {
    let arguments = args(); assert(arguments.len == 3); var directory: u8[>..] = []; directory.append(arguments[1]); let limit, valid = parse_int(arguments[2]); assert(valid && limit >= 1 && limit <= 8);
    var code = source::Code {}; assert(load(code, str(directory, "/base.gsp")) >= 0);
    var heap = source::Heap { code: code }; assert(source::initialize(heap)); let rt = source::boot_runtime(heap);
    let roots = source::allocate(heap, source::Node { kind: 0 }); assert(source::install_field(heap, heap.global, "__lightCompilerRoots", roots));
    var sc = snapshot::Control {}; let base = snapshot::capture(heap, [snapshot::Binding { id: 0, generation: 1, value: rt }], sc); assert(sc.error == snapshot::Error.None);
    let reader = thread_spawn(input); var running: Running[>..] = []; var queue: i64[>..] = []; var received: i64 = 0; var closing = false;
    print("READY 1 ", heap.boot_count); flush_stdout();
    while !closing || running.len > 0 {
        let command, available = qpoll<Command>(); if available { if command.id <= 0 { closing = true; } else { queue.push(command.id); } }
        if !closing && received < queue.len && running.len < limit {
            let id = queue[received]; received += 1;
            let t = thread_spawn_contained(worker, copy(base), copy(directory), id, source::reserve_identity()); running.push(Running { id: id, thread: t });
            print("START ", id); flush_stdout();
        }
        let event, present = qpoll<Event>(); if present {
            if event.kind == 1 { print("NEED ", event.id, " ", event.sequence, " ", event.name); }
            else {
                print(if event.kind == 2 { "DONE " } else { "ERROR " }, event.id);
                for r, i in running { if r.id == event.id { var reason: u8[>..] = []; assert(thread_wait(r.thread, reason)); running[i] = running[running.len - 1]; running.pop(); break; } }
            }
            flush_stdout();
        }
        sleep_ms(1);
    }
    assert(thread_wait(reader)); print("CLOSED 1"); flush_stdout();
}
