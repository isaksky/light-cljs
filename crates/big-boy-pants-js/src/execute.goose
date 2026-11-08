import std;
import os;
import .program;
import .source_vm;
import .vm;

// Exit status only; all loading, evaluation and heap operations are Goose.
extern fn process_exit(status: i32) = "exit";
fn error() { flush_stdout(); process_exit(1); }
// Machine-readable semantic record. Program diagnostics are distinct from
// loader/protocol/unsupported failures, which still exit nonzero.
fn typed_outcome(heap: source::Heap&, job: source::Job&) {
    // An unimplemented opcode is named, e.g. `unsupported:Fault.Unsupported:construct`.
    if job.fault == source::Fault.Unsupported { var fault: u8[>..] = []; source::describe_fault(fault, heap.code, job); print("unsupported:", fault); error(); return; }
    if job.fault != source::Fault.None && !job.thrown && job.fault != source::Fault.TypeError && job.fault != source::Fault.ReferenceError { print("infrastructure:", job.fault); error(); return; }
    var value: u8[>..] = [];
    if job.fault == source::Fault.None || job.thrown { source::canonical(heap, job.result, value); }
    else { format(value, "error:", if job.fault == source::Fault.TypeError { "TypeError" } else { "ReferenceError" }); }
    if job.result.is_object() && (job.fault == source::Fault.None || job.thrown) { print("infrastructure:unsupported outcome identity"); error(); return; }
    var out: u8[>..] = []; format(out, "{:status :", if job.fault == source::Fault.None { "returned" } else { "thrown" }, " :value \"", value, "\" :effects [");
    for write in job.writes {
        if source::key_is(heap, write.key, "observed") { var effect: u8[>..] = []; source::canonical(heap, write.value, effect); format(out, "\"", effect, "\" "); }
    }
    out.append("] :stdout \"\"}"); print(out);
}
fn main() {
    let arguments = args();
    if arguments.len < 2 || arguments.len > 3 { print("usage: execute PROGRAM.gsp [REPETITIONS=1, max=10000]"); error(); return; }
    var repetitions: i64 = 1;
    let typed = arguments.len == 3 && arguments[2] == "--outcome";
    if arguments.len == 3 && !typed { let count, ok = parse_int(arguments[2]); if !ok || count < 1 || count > 10000 { print("error: invalid repetition limit"); error(); return; } repetitions = count; }
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
    var result = source::Value.Undefined; var checksum: f64 = 0;
    let warmup: i64 = if repetitions > 1 { 100 } else { 0 };
    var warmed = prepared;
    for i in (repetitions + warmup) {
        if i == warmup { warmed = clock_ns(); }
        // The typed outcome reads the write log's values (light-cljs-847).
        var job = source::Job { heap: heap, logged_values: typed, writable_code: code };
        defer { source::retire(job); }
        if !source::start(job, entry, []) { print("start:", job.fault); error(); return; }
        let outcome = source::run(job, -1);
        if typed { typed_outcome(heap, job); return; }
        if outcome != tiny::Outcome.Complete {
            var fault: u8[>..] = []; source::describe_fault(fault, code, job);
            print("exception:", fault, ":function=", job.fault_function, ":pc=", job.fault_pc);
            if job.fault_pc >= 0 { let op = code.ops[job.fault_pc]; print("source-bytes:", op.start, "..", op.end); }
            error(); return;
        }
        result = job.result;
        if repetitions > 1 {
            if !result.is_number() { print("measurement requires a numeric source result"); error(); return; }
            if i >= warmup { checksum += result.num(); }
        }
    }
    let executed = clock_ns();
    var output: u8[>..] = []; source::canonical(heap, result, output); print(output);
    if repetitions > 1 {
        print("timing:repetitions=", repetitions, ",checksum=", checksum, ",load_boot_ns=", prepared - beginning, ",warmup_ns=", warmed - prepared, ",execute_dispose_ns=", executed - warmed, ",total_ns=", clock_ns() - beginning);
    }
}
