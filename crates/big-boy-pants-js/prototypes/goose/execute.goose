import std;
import os;
import .program;
import .source_vm;
import .vm;

// Exit status only; all loading, evaluation and heap operations are Goose.
extern fn process_exit(status: i32) = "exit";
fn error() { flush_stdout(); process_exit(1); }
fn main() {
    let arguments = args();
    if arguments.len < 2 || arguments.len > 3 { print("usage: execute PROGRAM.gsp [REPETITIONS=1, max=10000]"); error(); return; }
    var repetitions: i64 = 1;
    if arguments.len == 3 { let count, ok = parse_int(arguments[2]); if !ok || count < 1 || count > 10000 { print("error: invalid repetition limit"); error(); return; } repetitions = count; }
    let beginning = clock_ns();
    var bytes: u8[>..] = [];
    if !read_file(arguments[1], bytes) { print("error: cannot read program"); error(); return; }
    var image = program::Image {};
    let failure = program::install(image, bytes);
    if failure != program::Error.None { print("load:", failure); error(); return; }
    var code = source::Code {};
    if !source::load(code, image) { print("flow:", code.error, ":", code.error_pc); error(); return; }
    var heap = source::Heap { code: code };
    if !source::initialize(heap) { print("heap:", heap.fault); error(); return; }
    let entry = source::closure(heap, 0, []);
    let prepared = clock_ns();
    var result = source::Value {}; var checksum: f64 = 0;
    let warmup: i64 = if repetitions > 1 { 100 } else { 0 };
    var warmed = prepared;
    for i in (repetitions + warmup) {
        if i == warmup { warmed = clock_ns(); }
        var job = source::Job { heap: heap };
        defer { source::retire(job); }
        if !source::start(job, entry, []) { print("start:", job.fault); error(); return; }
        let outcome = source::run(job, -1);
        if outcome != tiny::Outcome.Complete {
            print("exception:", job.fault, ":function=", job.fault_function, ":pc=", job.fault_pc);
            if job.fault_pc >= 0 { let op = code.ops[job.fault_pc]; print("source-bytes:", op.start, "..", op.end); }
            error(); return;
        }
        result = job.result;
        if repetitions > 1 {
            if result.tag != 3 { print("measurement requires a numeric source result"); error(); return; }
            if i >= warmup { checksum += result.number; }
        }
    }
    let executed = clock_ns();
    var output: u8[>..] = []; source::canonical(heap, result, output); print(output);
    if repetitions > 1 {
        print("timing:repetitions=", repetitions, ",checksum=", checksum, ",load_boot_ns=", prepared - beginning, ",warmup_ns=", warmed - prepared, ",execute_dispose_ns=", executed - warmed, ",total_ns=", clock_ns() - beginning);
    }
}
