import std;
import os;
import .source_vm;
import .program;
import .form_bridge;
import .vm;

extern fn exit(status: i32) = "exit";
fn invoke(heap: source::Heap&, callee: source::Value, args: const source::Value[:]) -> source::Value, i64 {
    var job = source::Job { heap: heap }; source::start(job, callee, args);
    if source::run(job, -1) != tiny::Outcome.Complete { print("exception:", job.fault, ":function=", job.fault_function, ":pc=", job.fault_pc); exit(1); }
    return job.result, job.steps;
}
fn session(arguments: u8[:][>..]&) -> i64 {
    // OUTPUT-DIRECTORY MACRO-IMAGE... --helpers HELPER-IMAGE... --jobs PACKET...
    guard arguments.len >= 5 else { print("usage: macro_run OUTPUT MACRO... --helpers HELPER... --jobs PACKET..."); exit(1); return 0; }
    let begin = clock_ns(); var code = source::Code {}; var entries: i64[>..] = []; var helpers: i64[>..] = []; var packets: const u8[:][>..] = [];
    var mode: i64 = 0; var warming = false; var warmups: bool[>..] = []; var diagnostic = false;
    for path in arguments[2..] {
        if path == "--diagnostic" { diagnostic = true; continue; }
        if path == "--warmup" { mode = 2; warming = true; continue; }
        if path == "--helpers" { mode = 1; continue; } if path == "--jobs" { mode = 2; warming = false; continue; }
        if mode == 2 { packets.push(path); warmups.push(warming); continue; }
        var data: u8[>..] = []; assert(read_file(path, data)); var image = program::Image {}; if program::install(image, data) != program::Error.None { print("load failed:", path); exit(1); }
        var part = source::Code {}; if !source::load(part, image) { print("decode failed:", part.error); exit(1); }
        let entry = source::append_code(code, part); if mode == 0 { entries.push(entry); } else { helpers.push(entry); }
    }
    let loaded = clock_ns(); var heap = source::Heap { code: code }; assert(source::initialize(heap)); let rt = source::boot_runtime(heap);
    var macros: source::Value[>..] = [];
    for entry in entries { let factory, _ = invoke(heap, source::closure(heap, entry, []), []); let value, _ = invoke(heap, factory, [rt]); macros.push(value); heap.initializer_count += 1; }
    let roots = source::allocate(heap, source::Node { kind: 0 }); source::install_field(heap, heap.global, "__lightCompilerRoots", roots);
    for entry, i in helpers {
        let factory, _ = invoke(heap, source::closure(heap, entry, []), []); let value, _ = invoke(heap, factory, [rt]); heap.producer_count += 1;
        assert(i < 2); source::install_field(heap, roots, if i == 0 { "cljs.user/left-result" } else { "cljs.user/right-result" }, value);
    }
    let booted = clock_ns();
    print("{:phase :boot :load-ns ", loaded - begin, " :boot-ns ", booted - loaded, " :boot-count ", heap.boot_count, " :initializers ", heap.initializer_count, " :producers ", heap.producer_count, "}");
    for path, index in packets {
        let started = clock_ns(); var input = bridge::Graph {}; var bytes: u8[>..] = []; assert(read_file(path, bytes)); var c = bridge::Control {};
        if !bridge::decode(input, bytes, c) { print("bridge input:", c.error); exit(1); }
        if input.roots.len != 4 { print("bridge input:MalformedRoots"); exit(1); }
        var imported: source::Value[>..] = []; if !bridge::import_graph(heap, input, imported, c) { print("bridge import:", c.error); exit(1); }
        let call = imported[0]; let env = imported[1]; let argv = imported[2]; let which = source::form_value(heap, imported[3]);
        if which.tag != 3 || !(which.number >= 0 && which.number < macros.len) || which.number != (which.number as i64) || source::form_kind(heap, argv) != 6 { print("bridge input:MalformedArguments"); exit(1); }
        var params: source::Value[>..] = [call, env]; for i in source::form_count(heap, argv) { params.push(source::form_nth(heap, argv, i)); }
        let ready = clock_ns();
        let nodes_before = heap.nodes.len; let values_before = heap.values.len; let work_before = heap.work;
        let result, steps = invoke(heap, macros[which.number as i64], params); let invoked = clock_ns();
        let visits_before = c.visits;
        var output = bridge::Graph {}; var out: u8[>..] = [];
        if !bridge::export_graph(heap, [result], output, c, call) || !bridge::encode(output, out, c) { print("bridge export:", c.error); exit(1); }
        let converted = clock_ns();
        let target = str(arguments[1], "/", index, ".gfb"); assert(write_file(target, out));
        let exported = clock_ns();
        print("{:phase :job :id ", index, " :warmup ", warmups[index], " :import-ns ", ready - started, " :invoke-ns ", invoked - ready, " :export-ns ", converted - invoked, " :write-ns ", exported - converted, " :nodes ", output.nodes.len, " :bytes ", out.len, "}");
        if diagnostic { print("{:phase :diagnostic :id ", index, " :steps ", steps, " :invoke-work ", heap.work - work_before, " :new-nodes ", heap.nodes.len - nodes_before, " :new-values ", heap.values.len - values_before, " :retained-nodes ", heap.nodes.len, " :retained-values ", heap.values.len, " :retained-source-bytes ", heap.source_bytes.len, " :export-visits ", c.visits - visits_before, "}"); }
    }
    return clock_ns();
}

fn main() {
    let arguments = args(); let started = clock_ns(); let before_cleanup = session(arguments); let ended = clock_ns();
    print("{:phase :session :total-ns ", ended - started, " :cleanup-ns ", ended - before_cleanup, "}");
}
