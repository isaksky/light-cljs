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
extern fn compile_runtime(out: u8[>..]&, units: const u16&, count: i64, mode: i32, strict: bool, parameter_units: i64, site: const u8&, site_len: i64, line: i64&, column: i64&) -> i32 = "goose_runtime_compile";
let runtime_mode::IndirectEval: i32 = 0;
let runtime_mode::DynamicFunction: i32 = 1;
let runtime_mode::DirectEval: i32 = 2;
// A regular expression (light-cljs-932, compile_pattern): the units are
// the flags', `parameter_units` of them, then the pattern's; the answer is
// a regular-expression section of one program (program::PatternLayout).
let runtime_mode::Pattern: i32 = 3;
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
// the operation in a failure (`eval`). `site` is a direct eval's opaque site
// record (runtime_mode::DirectEval; Code.sites), empty for the other modes.
fn compile_code(job: Job&, what: const u8[:], units: const u16[:], mode: i32, strict: bool, parameter_units: i64, site: const u8[:] = []) -> i64 {
    let heap .= job.heap;
    let code = job.writable_code;
    if code == null { return runtime_unsupported(job, what, "the job's code is not writable here"); }
    if !charge(heap, units.len + 1) { return -1; }
    // The boundary takes a reference to the first unit: an empty source
    // passes a placeholder and a count of 0.
    var padded: u16[>..] = []; padded.append(units); if padded.len == 0 { padded.push(0); }
    var record: u8[>..]  = []; record.append(site); if record.len == 0 { record.push(0); }
    var out:    u8[>..]  = []; var line: i64 = 0; var column: i64 = 0;
    let status = compile_runtime(out, padded[0], units.len, mode, strict, parameter_units, record[0], site.len, line, column);
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
    // Its regular-expression programs' rows count as its instructions do
    // (light-cljs-932).
    let rows = part.ops.len + part.pattern_ops.len;
    if rows > heap.limits.code - job.compiled_ops { heap.fault = Fault.MemoryLimit; return -1; }
    // Every code table stays within what a snapshot of it admits
    // (snapshot::code_valid: 65,536 functions, 8 MiB of source), so a
    // continuation that captures the job's code never fails on it.
    if part.functions.len > 65536 - code.functions.len || part.source_bytes.len > 8388608 - code.source_bytes.len { heap.fault = Fault.MemoryLimit; return -1; }
    // So do its direct-eval site tables (light-cljs-858), each within a
    // snapshot table's 8,388,608 rows.
    if part.sites.len > 8388608 - code.sites.len || part.site_sources.len > 8388608 - code.site_sources.len || part.site_records.len > 8388608 - code.site_records.len { heap.fault = Fault.MemoryLimit; return -1; }
    // and so do its regular-expression tables and the code units
    // (light-cljs-932).
    if part.patterns.len > 8388608 - code.patterns.len || part.pattern_ops.len > 8388608 - code.pattern_ops.len || part.units.len > 8388608 - code.units.len { heap.fault = Fault.MemoryLimit; return -1; }
    if !charge(heap, rows) { return -1; }
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
    job.compiled_ops += rows;
    return root;
}
// Compile the pattern of `new RegExp(pattern, flags)` (light-cljs-932;
// regexp_object.goose), both strings, and append its program to the job's
// code: its row in Code.patterns, or -1 with the heap's fault set. The run-time
// compiler parses the flags and the pattern and compiles the matcher
// program (runtime_mode::Pattern); its section is validated
// (program::validate_patterns) before any row joins the code. A failure is
// the program's (regexp_object.goose pattern_failure): invalid flags or
// pattern a SyntaxError with the parser's message. The program's op rows
// count against Limits.code as eval's instructions do, its text and rows
// against the work budget; the worker's reset truncates them, and
// snapshots, saved continuations and callable publications carry them as
// code rows. Its `op` is the code's instruction count now, which keeps the
// table in ascending `op` order.
fn compile_pattern(job: Job&, pattern: Value, flags: Value) -> i64 {
    let heap .= job.heap;
    let code = job.writable_code;
    if code == null { return runtime_unsupported(job, "RegExp", "the job's code is not writable here"); }
    var units: u16[>..] = []; append_utf16(heap, flags, units);
    let flag_units = units.len; append_utf16(heap, pattern, units);
    if !charge(heap, units.len + 1) { return -1; }
    let count = units.len; if units.len == 0 { units.push(0); }
    var record: u8[>..] = [0];
    var out:    u8[>..] = []; var line: i64 = 0; var column: i64 = 0;
    let status = compile_runtime(out, units[0], count, runtime_mode::Pattern, false, flag_units, record[0], 0, line, column);
    if status != runtime_status::Image { return runtime_unsupported(job, "RegExp", out); }
    let pl = program::pattern_layout(out, 0);
    let failure = program::validate_patterns(out, pl, 0, false);
    if failure != program::Error.None { var why: u8[>..] = []; format(why, "the run-time compiler's pattern fails validation (", failure, ")"); return runtime_unsupported(job, "RegExp", why); }
    var p = program::pattern_at(out, pl, 0);
    let rows: i64 = p.instructions + p.members + p.names;
    if p.failure != regexp_failure::None {
        job.detail.clear();
        if p.failure == regexp_failure::Unsupported { for c in "RegExp: " { job.detail.push(c); } }
        for i in p.unit_count { job.detail.push(program::unit(out, pl.units, i) as u16); }
        heap.fault = if p.failure == regexp_failure::Syntax { Fault.SyntaxError } else if p.failure == regexp_failure::Limit { Fault.RangeError } else { Fault.UnsupportedOperand };
        return -1;
    }
    if rows > heap.limits.code - job.compiled_ops || rows > 8388608 - code.pattern_ops.len || code.patterns.len >= 8388608 || p.unit_count > 8388608 - code.units.len { heap.fault = Fault.MemoryLimit; return -1; }
    if !charge(heap, rows) { return -1; }
    let start = code.pattern_ops.len; let text = code.units.len;
    for i in rows { code.pattern_ops.push(program::pattern_op(out, pl, i)); }
    for i in p.unit_count { code.units.push(program::unit(out, pl.units, i) as u16); }
    p.op = code.ops.len as u32; p.literal = false; p.start = start as u32; p.units = text as u32;
    code.patterns.push(p);
    job.compiled_ops += rows;
    return code.patterns.len - 1;
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
// A direct eval (light-cljs-858; ECMAScript PerformEval with direct true,
// the Pants engine's start_direct_eval and eval_source in
// rust/src/vm/native.rs): the marked call at instruction row `at` of the
// frame at `top`, whose callee is the realm's %eval%, with `argc` arguments
// above it (a method form's receiver below it is ignored). A non-string
// first argument is the result, unchanged; extra arguments were evaluated
// and are ignored. A string compiles in the caller's scope: the run-time
// compiler gets the call's site record (Code.sites) and compiles eval code
// against it (compile_direct_eval: strict or sloppy as the caller, its
// bindings, receiver, new.target, dynamic environments and super context);
// a compile error is a catchable SyntaxError. The eval code is a closure
// whose captures are the site's sources read from this frame now, in its
// capture order: a local or the closure's capture (a binding cell is the
// cell itself, so eval code shares the caller's variable), the running
// function, the frame's receiver (eval code checks a sloppy caller's with
// SloppyThis), the global object, new.target. Sloppy eval code declares its
// `var` and function bindings through the caller's variable environment, a
// binding cell among those captures (light-cljs-855), or as configurable
// globals at script level (global_op with Function.eval). The closure runs
// in a new frame of the dispatch loop (decision 4: the job's budget, limits
// and cancellation apply inside it), with no receiver of its own, and its
// completion value arrives where the call's result goes. A call without a
// site record fails by name.
fn start_direct_eval(job: Job&, top: i64, at: i64, argc: i64, method: bool) {
    let heap .= job.heap;
    let base = job.operands.len - argc - (if method { 2 } else { 1 });
    let x = if argc > 0 { job.operands[job.operands.len - argc] } else { Value.Undefined };
    if !x.is_string() { job.operands.resize(base); job.operands.push(x); return; }
    let row = site_at(heap.code, at);
    if row < 0 { runtime_unsupported(job, "eval", "the direct-eval call has no site record"); return; }
    let site = heap.code.sites[row]; let frame = job.frames[top];
    var captures: Value[>..] = [];
    for source in heap.code.site_sources[site.sources..(site.sources + site.count)] {
        let kind = source.kind as i64;
        if kind == eval_source::Local {
            if source.index >= heap.code.functions[frame.function].locals { heap.fault = Fault.InvalidState; return; }
            captures.push(job.locals[frame.local_base + source.index]);
        } else if kind == eval_source::Capture {
            let callee = frame.callee;
            if !callee.is_object() || node(heap, callee.id()).kind != node_kind::Closure || source.index >= node(heap, callee.id()).count { heap.fault = Fault.InvalidState; return; }
            captures.push(slot(heap, node(heap, callee.id()).start + source.index));
        } else if kind == eval_source::CurrentFunction { captures.push(frame.callee);
        } else if kind == eval_source::This {
            // A sloppy caller's primitive receiver is boxed once, in its
            // frame, as This boxes it (OrdinaryCallBindThis), so eval code
            // and the caller read the one wrapper.
            var receiver = frame.receiver;
            if !heap.code.functions[frame.function].strict && !receiver.is_object() && !receiver.is_undefined() && !receiver.is_null() {
                receiver = to_object(job, receiver);
                if heap.fault != Fault.None { return; }
                job.frames[top].receiver = receiver;
            }
            captures.push(receiver);
        } else if kind == eval_source::GlobalObject { captures.push(heap.global);
        } else if kind == eval_source::NewTarget { captures.push(frame.input);
        } else { heap.fault = Fault.InvalidState; return; }
    }
    var record: u8[>..] = []; record.append(heap.code.site_records[site.record..(site.record + site.length)]);
    let units = utf16(heap, x);
    let root = compile_code(job, "eval", units, runtime_mode::DirectEval, false, 0, record);
    if root < 0 { return; }
    if heap.code.functions[root].captures != captures.len { runtime_unsupported(job, "eval", "the compiled eval code's captures differ from its site's sources"); return; }
    let entry = closure(heap, root, captures);
    if heap.fault != Fault.None { return; }
    if push_frame(job, entry, Value.Undefined, [], false, base) { job.operands.resize(base); }
}
// The Function constructor (light-cljs-857; ECMAScript
// CreateDynamicFunction with kind normal, QuickJS js_function_constructor,
// the Pants engine's DynamicFunction in rust/src/vm/native/eval.rs).
// `Function(p1, ..., pn, body)` and `new Function(...)` are the same: every
// argument converts by ToString in order, then the source the specification
// gives, "function anonymous(" + p1 + "," + ... + pn + "\n) {\n" + body +
// "\n}", compiles in the dynamic-function mode (the parameter boundary keeps
// a `)` in the parameter text from closing the list early, so "a){" is a
// SyntaxError) and becomes a closure with no captures: its scope is the
// global one, never the caller's. A compile error is a catchable
// SyntaxError. Its `name` ("anonymous"), `length`, `prototype` and toString
// text come from the compiled function, as any function's do.
//
// The conversions can run JavaScript (an object's toString or valueOf), so
// the operation is the DynamicFunction native frame (decision 5), its
// arguments the frame's operands from operand_base:
//   argument_count  n, the argument count
//   index           the next argument to convert
//   receiver        new.target when it is not Function itself
//                   (Reflect.construct), whose `prototype` the function
//                   takes after compilation (GetPrototypeFromConstructor);
//                   otherwise undefined
//   pc 0            converts the arguments from `index` on: a primitive in
//                   place, an object in a Convert frame above this one (hint
//                   string, then ToString), after which this step runs again
//   pc 1            awaits newTarget's `prototype` getter (light-cljs-887),
//                   the compiled function in `output`
// Generator, async and async generator functions built at run time
// (%GeneratorFunction% and its siblings) belong to the generators area
// (light-cljs-768).
fn start_dynamic_function(job: Job&, args: const Value[:], base: i64, new_target: Value) {
    let heap .= job.heap;
    var copied: Value[>..] = []; copied.append(args);
    job.operands.resize(base);
    if job.frames.len >= heap.limits.frames || copied.len > heap.limits.stack - job.locals.len - job.operands.len { heap.fault = Fault.StackLimit; return; }
    job.frames.push(Frame { function: -1, pc: 0, local_base: job.locals.len, operand_base: base, callee: Value.Undefined, receiver: new_target, argument_count: copied.len, native: Native.DynamicFunction });
    for v in copied { job.operands.push(v); }
    dynamic_function_step(job, job.frames[job.frames.len - 1]);
}
// One step of the DynamicFunction frame at the top (see above).
fn dynamic_function_step(job: Job&, frame: Frame) {
    let heap .= job.heap; let at = job.frames.len - 1;
    let base = frame.operand_base; let n = frame.argument_count;
    if frame.pc == 1 {
        let p = job.operands.pop();
        if !valid(heap, p) { return; }
        if job.operands.len != base + n || !frame.output.is_object() { heap.fault = Fault.InvalidState; return; }
        if p.is_object() { node_mut(heap, frame.output.id()).proto = p; }
        finish_native(job, frame, frame.output);
        return;
    }
    if frame.pc != 0 || job.operands.len != base + n || frame.index < 0 || frame.index > n { heap.fault = Fault.InvalidState; return; }
    for i in frame.index..n {
        // A scalar form (a primitive with its provenance) is its primitive.
        let v = form_value(heap, job.operands[base + i]);
        if v.is_string() { job.operands[base + i] = v; continue; }
        if v.is_object() {
            if !convertible(heap, v) { unsupported_step(job, Builtin.Function, "ToString of a light-cljs runtime value"); return; }
            job.frames[at].index = i;
            start_convert(job, Hint.String, true, 1, n - i);
            return;
        }
        let s = to_string(heap, v);
        if heap.fault != Fault.None {
            if heap.fault == Fault.TypeError && job.detail.len == 0 { for c in "Cannot convert a Symbol value to a string" { job.detail.push(c); } }
            return;
        }
        job.operands[base + i] = s;
    }
    var units: u16[>..] = [];
    for c in "function anonymous(" { units.push(c); }
    for i in 0..(n - 1) {
        if i > 0 { units.push(','); }
        append_utf16(heap, job.operands[base + i], units);
    }
    for c in "\n)" { units.push(c); }
    let parameter_units = units.len;
    for c in " {\n" { units.push(c); }
    if n > 0 { append_utf16(heap, job.operands[base + n - 1], units); }
    for c in "\n}" { units.push(c); }
    let root = compile_code(job, "Function", units, runtime_mode::DynamicFunction, false, parameter_units);
    if root < 0 { return; }
    let f = closure(heap, root, []);
    if heap.fault != Fault.None { return; }
    if frame.receiver.is_object() {
        let proto, getter = closure_prototype(job, frame.receiver);
        if heap.fault != Fault.None { return; }
        if getter.is_object() { job.frames[at].output = f; job.frames[at].pc = 1; call_accessor(job, getter, frame.receiver, []); return; }
        if proto.is_object() { node_mut(heap, f.id()).proto = proto; }
    }
    finish_native(job, frame, f);
}
