import std;
import os;
import .program;
import .source_vm;
import .vm;
import .census;

// Conformance runner (scripts/conformance.bb): load one exported program
// image, run its top level once, and print one EDN outcome map. It never
// changes VM behaviour; it only names what happened. The job keeps the VM's
// default table, frame and value limits, but no instruction (steps) or work
// budget ends a case: the Pants command line runs scripts with an unlimited
// budget and QuickJS has none, so the harness's per-engine timeout
// (conformance.bb --timeout) bounds a runaway case for all three engines
// alike (light-cljs-840). Compiler jobs keep their own budgets.
//   {:status :returned :value V}        the script completed with value V
//   {:status :rejected :value V}        a promise rejected with V that no
//                                       reaction ever handled
//   {:status :thrown :value V}          an uncaught JavaScript throw of V, or
//                                       a VM-raised TypeError, ReferenceError
//                                       or InternalError (stack overflow)
//   {:status :unsupported :fault F ...} a named VM fault: behaviour the VM
//                                       does not implement (never catchable);
//                                       Fault.Unsupported adds :operation, the
//                                       unimplemented opcode's registry name
//   {:status :limit :fault F ...}       a VM resource limit was reached
//   {:status :infrastructure :reason R} the image could not be loaded or run
// V is source::canonical's text. The exit status is 0 for every outcome the
// runner could classify and 1 for usage or infrastructure failures.
// `conformance PROGRAM.gsp heap` also prints a second map: the job heap's
// table rows at the end and how many no root reaches (census.goose).
//   {:heap {:nodes N :values V :properties P :units U :dead-nodes ...}}
extern fn process_exit(status: i32) = "exit";
// A steps and work limit no case reaches before the timeout; well below
// i64's maximum, so the VM's budget arithmetic cannot overflow.
let unbounded: i64 = 1 << 62;
fn infrastructure(reason: const u8[:], detail: const u8[:] = "") {
    var out: u8[>..] = []; format(out, "{:status :infrastructure :reason \"", reason, detail, "\"}");
    print(out); flush_stdout(); process_exit(1);
}
fn located(out: u8[>..]&, status: const u8[:], heap: const source::Heap&, job: const source::Job&) {
    format(out, "{:status :", status, " :fault \"", job.fault, "\" :function ", job.fault_function, " :pc ", job.fault_pc);
    if job.fault_pc >= 0 && job.fault_pc < heap.code.ops.len {
        let op = heap.code.ops[job.fault_pc];
        format(out, " :opcode ", op.code, " :span [", op.start, " ", op.end, "]");
    }
    if job.fault == source::Fault.Unsupported { out.append(" :operation \""); source::unsupported_operation(out, heap.code, job); out.push('"'); }
    if job.detail.len > 0 { out.append(" :detail \""); for u in job.detail { out.push(u as u8); } out.push('"'); }
    out.push('}');
}
fn main() {
    let arguments = args();
    if arguments.len != 2 && !(arguments.len == 3 && arguments[2] == "heap") { infrastructure("usage: conformance PROGRAM.gsp [heap]"); return; }
    var bytes: u8[>..] = [];
    if !read_file(arguments[1], bytes) { infrastructure("cannot read program"); return; }
    var image = program::Image {};
    let failure = program::install(image, bytes);
    if failure != program::Error.None { var detail: u8[>..] = []; format(detail, failure); infrastructure("load:", detail); return; }
    var code = source::Code {};
    if !source::load(code, image) { var detail: u8[>..] = []; format(detail, code.error, ":", code.error_pc); infrastructure("flow:", detail); return; }
    var heap = source::Heap { code: code, limits: source::Limits { steps: unbounded, work: unbounded } };
    if !source::initialize(heap) { var detail: u8[>..] = []; format(detail, heap.fault); infrastructure("heap:", detail); return; }
    let entry = source::closure(heap, 0, []);
    var job = source::Job { heap: heap, writable_code: code };
    defer { source::retire(job); }
    if !source::start(job, entry, []) { var detail: u8[>..] = []; format(detail, job.fault); infrastructure("start:", detail); return; }
    let outcome = source::run(job, -1);
    var out: u8[>..] = [];
    // A promise rejected with no reaction once the promise jobs drained
    // (light-cljs-790), as the QuickJS driver and the Pants engine report it.
    if outcome == tiny::Outcome.Failed && job.fault == source::Fault.UnhandledRejection {
        var value: u8[>..] = []; source::canonical(heap, job.result, value);
        format(out, "{:status :rejected :value \"", value, "\"}");
    } else if outcome == tiny::Outcome.Complete || job.thrown {
        var value: u8[>..] = []; source::canonical(heap, job.result, value);
        // An error object JavaScript made is named by its `name` (a data
        // property, own or inherited), as the QuickJS driver names it.
        if job.thrown && job.result.is_object() && source::node(heap, job.result.id()).kind == node_kind::Error {
            let name = source::get(heap, job.result, source::text(heap, "name"));
            if heap.fault == source::Fault.None && name.is_string() { value.clear(); value.append("error:"); for i in source::str_len(heap, name) { value.push(source::unit(heap, name, i) as u8); } }
            heap.fault = source::Fault.None;
        }
        format(out, "{:status :", if job.thrown { "thrown" } else { "returned" }, " :value \"", value, "\"}");
    } else if outcome == tiny::Outcome.Yielded && job.effect_pending {
        // A host effect (__rt.effect) has no host in this runner.
        format(out, "{:status :unsupported :fault \"HostEffect\" :effect ", job.effect_kind, "}");
    } else if outcome != tiny::Outcome.Failed {
        var detail: u8[>..] = []; format(detail, outcome); infrastructure("outcome:", detail); return;
    } else {
        match job.fault {
            // The catchable faults, uncaught, named as the Pants engine and
            // QuickJS name them (light-cljs-786).
            TypeError, ReferenceError, RangeError, SyntaxError, StackLimit => { format(out, "{:status :thrown :value \"error:", source::error_name(job.fault), "\"}"); },
            MemoryLimit, WorkLimit, InstructionLimit => { located(out, "limit", heap, job); },
            InvalidState, StaleID, None => { var detail: u8[>..] = []; format(detail, job.fault); infrastructure("fault:", detail); return; },
            _ => { located(out, "unsupported", heap, job); },
        }
    }
    print(out);
    if arguments.len == 3 {
        var roots: source::Value[>..] = [job.result];
        for f in job.frames { roots.push(f.callee); roots.push(f.receiver); roots.push(f.input); roots.push(f.output); }
        for h in job.handlers { roots.push(h.value); }
        for v in job.locals { roots.push(v); } for v in job.operands { roots.push(v); }
        roots.push(job.entry); roots.push(job.lexicals); for t in job.queue { roots.push(t.value); roots.push(source::object(t.target)); }
        let c = census::take(heap, roots); var line: u8[>..] = [];
        format(line, "{:heap {:nodes ", c.nodes, " :values ", c.values, " :properties ", c.properties, " :units ", c.units);
        format(line, " :dead-nodes ", c.dead_nodes, " :dead-values ", c.dead_values, " :dead-properties ", c.dead_properties, " :dead-units ", c.dead_units, "}}");
        print(line);
    }
}
