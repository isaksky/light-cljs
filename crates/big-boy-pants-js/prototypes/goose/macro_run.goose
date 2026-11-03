import std;
import os;
import .source_vm;
import .program;
import .form_bridge;
import .vm;

extern fn exit(status: i32) = "exit";
fn invoke(heap: source::Heap&, callee: source::Value, args: const source::Value[:]) -> source::Value {
    var job = source::Job { heap: heap }; source::start(job, callee, args);
    if source::run(job, -1) != tiny::Outcome.Complete { print("exception:", job.fault, ":function=", job.fault_function, ":pc=", job.fault_pc); exit(1); }
    return job.result;
}
fn main() {
    // OUTPUT-DIRECTORY MACRO-IMAGE... --helpers HELPER-IMAGE... --jobs PACKET...
    let arguments = args(); guard arguments.len >= 5 else { print("usage: macro_run OUTPUT MACRO... --helpers HELPER... --jobs PACKET..."); exit(1); return; }
    let begin = clock_ns(); var code = source::Code {}; var entries: i64[>..] = []; var helpers: i64[>..] = []; var packets: const u8[:][>..] = [];
    var mode: i64 = 0;
    for path in arguments[2..] {
        if path == "--helpers" { mode = 1; continue; } if path == "--jobs" { mode = 2; continue; }
        if mode == 2 { packets.push(path); continue; }
        var data: u8[>..] = []; assert(read_file(path, data)); var image = program::Image {}; if program::install(image, data) != program::Error.None { print("load failed:", path); exit(1); }
        var part = source::Code {}; if !source::load(part, image) { print("decode failed:", part.error); exit(1); }
        let entry = source::append_code(code, part); if mode == 0 { entries.push(entry); } else { helpers.push(entry); }
    }
    let loaded = clock_ns(); var heap = source::Heap { code: code }; assert(source::initialize(heap)); let rt = source::boot_runtime(heap);
    var macros: source::Value[>..] = [];
    for entry in entries { let factory = invoke(heap, source::closure(heap, entry, []), []); macros.push(invoke(heap, factory, [rt])); heap.initializer_count += 1; }
    let roots = source::allocate(heap, source::Node { kind: 0 }); source::install_field(heap, heap.global, "__lightCompilerRoots", roots);
    for entry, i in helpers {
        let factory = invoke(heap, source::closure(heap, entry, []), []); let value = invoke(heap, factory, [rt]); heap.producer_count += 1;
        assert(i < 2); source::install_field(heap, roots, if i == 0 { "cljs.user/left-result" } else { "cljs.user/right-result" }, value);
    }
    let booted = clock_ns();
    print("{:phase :boot :load-ns ", loaded - begin, " :boot-ns ", booted - loaded, " :boot-count ", heap.boot_count, " :initializers ", heap.initializer_count, " :producers ", heap.producer_count, "}");
    for path, index in packets {
        let started = clock_ns(); var input = bridge::Graph {}; var bytes: u8[>..] = []; assert(read_file(path, bytes)); var c = bridge::Control {};
        if !bridge::decode(input, bytes, c) { print("bridge input:", c.error); exit(1); }
        var imported: source::Value[>..] = []; assert(bridge::import_graph(heap, input, imported, c)); assert(imported.len == 4);
        let call = imported[0]; let env = imported[1]; let argv = imported[2]; let which = source::form_value(heap, imported[3]); assert(which.tag == 3 && which.number >= 0 && which.number < macros.len);
        var params: source::Value[>..] = [call, env]; for i in source::form_count(heap, argv) { params.push(source::form_nth(heap, argv, i)); }
        let ready = clock_ns(); let result = invoke(heap, macros[which.number as i64], params); let invoked = clock_ns();
        var output = bridge::Graph {}; var out: u8[>..] = [];
        assert(bridge::export_graph(heap, [result], output, c, call)); assert(bridge::encode(output, out, c));
        let target = str(arguments[1], "/", index, ".gfb"); assert(write_file(target, out));
        let exported = clock_ns();
        print("{:phase :job :id ", index, " :import-ns ", ready - started, " :invoke-ns ", invoked - ready, " :export-ns ", exported - invoked, " :nodes ", output.nodes.len, " :bytes ", out.len, "}");
    }
}
