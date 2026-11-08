import std;
import .program;
import .source_vm;
import .realm;

namespace source;

// Run-time compilation (light-cljs-856; docs/javascript-coverage.md,
// decision 1, "Run-time compilation"). JavaScript a job hands the VM as a
// string, an indirect eval's script here and later the Function
// constructor's source (light-cljs-857) and a direct eval's (light-cljs-858),
// goes through bbp's Rust frontend and Goose exporter, the same compiler
// that made the job's image: runtime_compiler.h opens it in this process on
// the first call and compiles on a helper thread with the frontend's 2 MiB
// stack. What comes back is bytes: a GSP image this module validates
// (program::install) and decodes (load) before any of it runs, exactly as a
// loaded program, then appends to the job's code (append_code) as job rows
// above the image's functions. The worker's reset cuts them away with every
// other row the job added (snapshot::truncate, decision 3); a saved
// continuation and a captured snapshot carry them, since a snapshot holds the
// whole code tables (snapshot::capture), and so does a callable publication's
// code suffix (callable.goose).
//
// compile_runtime's statuses (runtime_compiler.h): runtime_status::Image,
// the image in `out`; SyntaxError, the frontend's message in `out` (UTF-8);
// Refused, the frontend refused the request (`out` says why); Unavailable,
// the library could not be loaded (`out` says why).
extern fn compile_runtime(out: u8[>..]&, units: const u16&, count: i64, mode: i32, strict: bool, parameter_units: i64, line: i64&, column: i64&) -> i32 = "goose_runtime_compile";
let runtime_mode::IndirectEval: i32 = 0;
let runtime_mode::DynamicFunction: i32 = 1;
let runtime_mode::DirectEval: i32 = 2;
let runtime_status::Image: i32 = 0;
let runtime_status::SyntaxError: i32 = 1;
let runtime_status::Refused: i32 = 2;
let runtime_status::Unavailable: i32 = 3;

// A run-time compilation the VM cannot complete fails UnsupportedOperand by
// name: what was compiled, then why.
fn runtime_unsupported(job: Job&, what: const u8[:], why: const u8[:]) -> i64 {
    job.detail.clear();
    for c in what { job.detail.push(c); }
    job.detail.push(':'); job.detail.push(' ');
    if !append_utf8(job.detail, why) { for c in "an undecodable reason" { job.detail.push(c); } }
    if job.heap.fault == Fault.None { job.heap.fault = Fault.UnsupportedOperand; }
    return -1;
}
// Compile the UTF-16 source `units` in `mode` (runtime_mode::) and append
// its functions to the job's code: the new root function's ID, or -1 with
// the heap's fault set. A compile error is a catchable SyntaxError with the
// frontend's message. The rows count against Limits.code (a MemoryLimit),
// and the source and the instructions against the work budget. `what` names
// the operation in a failure (`eval`).
fn compile_code(job: Job&, what: const u8[:], units: const u16[:], mode: i32, strict: bool, parameter_units: i64) -> i64 {
    let heap .= job.heap;
    let code = job.writable_code;
    if code == null { return runtime_unsupported(job, what, "the job's code is not writable here"); }
    if !charge(heap, units.len + 1) { return -1; }
    // The boundary takes a reference to the first unit: an empty source
    // passes a placeholder and a count of 0.
    var padded: u16[>..] = []; padded.append(units); if padded.len == 0 { padded.push(0); }
    var out:    u8[>..]  = []; var line: i64 = 0; var column: i64 = 0;
    let status = compile_runtime(out, padded[0], units.len, mode, strict, parameter_units, line, column);
    if status == runtime_status::SyntaxError {
        job.detail.clear();
        if !append_utf8(job.detail, out) { for c in "invalid source" { job.detail.push(c); } }
        heap.fault = Fault.SyntaxError; return -1;
    }
    if status != runtime_status::Image { return runtime_unsupported(job, what, out); }
    // Never run unvalidated bytes: the image passes program::validate and
    // load's flow checks, as a loaded program does, before it joins the code.
    var image = program::Image {};
    let failure = program::install(image, out);
    if failure != program::Error.None { var why: u8[>..] = []; format(why, "the run-time compiler's image fails validation (", failure, ")"); return runtime_unsupported(job, what, why); }
    var part = Code {};
    if !load(part, image) { var why: u8[>..] = []; format(why, "the run-time compiler's image fails its flow check (", part.error, ")"); return runtime_unsupported(job, what, why); }
    if part.ops.len > heap.limits.code - job.compiled_ops { heap.fault = Fault.MemoryLimit; return -1; }
    // Every code table stays within what a snapshot of it admits
    // (snapshot::code_valid: 65,536 functions, 8 MiB of source), so a
    // continuation that captures the job's code never fails on it.
    if part.functions.len > 65536 - code.functions.len || part.source_bytes.len > 8388608 - code.source_bytes.len { heap.fault = Fault.MemoryLimit; return -1; }
    if !charge(heap, part.ops.len) { return -1; }
    // Code constants and heap constants share their rows (initialize): the
    // new constants' strings become heap strings, as a session's appended
    // unit's do (compiler_session.goose append).
    if heap.constants.len != code.constants.len { heap.fault = Fault.InvalidState; return -1; }
    let first = code.constants.len;
    let root = append_code(code, part);
    for c in code.constants[first..] {
        var value = c;
        if c.is_string() { value = string(heap, code_chars(code, c)); }
        if heap.fault != Fault.None { return -1; }
        add_constants(heap, [value]);
    }
    // Eval code's var and function declarations make configurable global
    // properties (global_op): the root knows it is eval code.
    code.functions[root].eval = mode != runtime_mode::DynamicFunction;
    job.compiled_ops += part.ops.len;
    return root;
}
// The global `eval` called other than as a direct-eval site (bbp-106;
// ECMAScript PerformEval with direct false, QuickJS js_global_eval): a
// non-string argument is the result, unchanged; a string is compiled as
// global script code, sloppy unless its own prologue says "use strict", and
// runs in a new frame of the dispatch loop (decision 4: no host recursion;
// the job's limits, budget and cancellation apply inside it), whose return,
// the script's completion value, arrives at `base` as a call's does. Its
// `this` is the global object (the compiler gives eval code no receiver of
// its own).
fn start_indirect_eval(job: Job&, args: const Value[:], base: i64) {
    let heap .= job.heap;
    let x = arg(args, 0);
    if !x.is_string() { job.operands.resize(base); job.operands.push(x); return; }
    let units = utf16(heap, x);
    let root = compile_code(job, "eval", units, runtime_mode::IndirectEval, false, 0);
    if root < 0 { return; }
    let entry = closure(heap, root, []);
    if heap.fault != Fault.None { return; }
    if push_frame(job, entry, heap.global, [], false, base) { job.operands.resize(base); }
}
