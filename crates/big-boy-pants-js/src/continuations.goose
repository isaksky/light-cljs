import std;
import math;
import .source_vm;
import .realm;

namespace source;

// Generators, async functions and promise jobs (light-cljs-790; decision 4
// of docs/javascript-coverage.md, "Generators and async functions" and
// "Promise jobs"). Ported from the Pants engine's movable VM activation
// (rust/src/vm/generator.rs and vm/frames.rs, DetachedActivation;
// vm/asynchronous.rs), with the bytecode's own abrupt-completion paths: a
// resumed yield receives [value, kind] and runs `return` and `throw` under
// its saved handlers (rust/src/frontend/lower.rs, ExprKind::Yield).
//
// Suspension is data (rule 5). A yield or await *detaches* the top frame's
// activation: its frame (a Frame row of heap.conts), its locals and
// operands (copied into a run of heap.values, the activation's slot run) and its handlers (copied into
// Saved rows of heap.conts, their heights relative to the frame's bases)
// become one Activation row of heap.conts, and the frame leaves the job's
// stacks. Resumption *attaches* it: the rows are copied back on top of the
// job's stacks at the current heights. Nothing keeps a Goose or native stack
// across a suspension, and the rows are flat, linked only by plain u32 row
// IDs (rule 2), so a suspended generator or async function moves with the
// image in every snapshot and saved continuation (snapshot.goose validates
// every link). The rows are the job's (rule 1, amended by light-cljs-792):
// the reset truncates heap.conts to the image length like every table, so
// nothing a job left suspended reaches the next job on the same worker.
// An activation keeps its run and handler rows for its next suspension when
// they fit, so a long-running generator or async loop does not grow the
// tables per step; a run that no longer fits is replaced by one twice as
// large (never a free list: the old run stays the job's until the reset).
//
// Generators (node_kind::Generator, `function` its Activation row, `length` its state):
// GeneratorInitialYield makes the generator object (its [[Prototype]] the
// function's `prototype`, else %GeneratorPrototype%), detaches the
// initialized activation in state 0 (suspended start) and returns the
// object to the caller. next, return and throw push a Resume native frame
// (Native.Resume, its kind a resume_kind::): state 2 (executing) attaches the
// activation above it, pushing [value, kind] at a yield; GeneratorYield
// detaches it again (state 1, suspended yield) and gives the value to the
// Resume frame, which returns {value, done: false}; the generator frame's
// Return gives it the value of {value, done: true} (state 3, completed). An
// exception or cancellation unwinding the Resume frame completes the
// generator (unwind_natives). return and throw on a generator in its
// suspended start complete it without running it. GeneratorYieldStar and
// async generators stay explicit Unsupported for the generators area
// (light-cljs-768).
let generator_state::SuspendedStart = 0;
let generator_state::SuspendedYield = 1;
let generator_state::Executing = 2;
let generator_state::Completed = 3;
// A Resume frame's operation (Native.Resume's kind).
let resume_kind::Next = 0;
let resume_kind::Return = 1;
let resume_kind::Throw = 2;
//
// Async functions: AsyncStart makes the function's promise (node_kind::Promise) and
// pushes its Async handler, the frame's completion boundary (HandlerKind):
// an exception that reaches it rejects the promise and a return resolves it;
// either way the frame ends delivering the promise. Await resolves its
// operand to a promise (a promise as itself, any other value a promise
// fulfilled with it; a thenable object is a named UnsupportedOperand),
// detaches the activation (the promise's `activation`), registers a resume
// reaction on that promise and delivers the async function's promise to the
// frame below: the caller at the first await, nothing for an activation a
// promise job resumed.
//
// Promises (node_kind::Promise, `function` its Promise row): a state (promise_status:: Pending,
// Fulfilled or Rejected), a result, the reactions registered while pending
// (Reaction rows, a list in registration order) and, for an async
// function's, its activation. Settling queues one promise job per reaction,
// in order; registering a reaction on a settled promise queues its job at
// once. Resolving a promise with a promise queues a job that registers the
// adoption (the specification's NewPromiseResolveThenableJob, so the job
// order is QuickJS's); resolving a promise with itself rejects it with a
// TypeError. The Promise constructor, its methods and thenable objects
// belong to the promises area (light-cljs-768).
let promise_status::Pending = 0;
let promise_status::Fulfilled = 1;
let promise_status::Rejected = 2;
//
// The promise job queue (Job.queue) is the job's FIFO of Tasks. When the
// entry finishes (entry_done), the job keeps its value and drains the queue
// one task a step whenever its frame stack is empty (drain_step, under the
// budget, instruction and work limits of every step); a resumed activation's
// own completion value goes nowhere. When the queue is empty the job ends
// (finish_jobs): with Fault.UnhandledRejection, its thrown value the reason,
// if a promise was rejected and nothing ever registered a reaction on it
// (the Pants engine and QuickJS report the same rejections); otherwise with
// the entry's value or, for an entry that returned a promise, that promise's
// outcome: its value, its reason thrown, or Fault.Unsettled for a promise
// still pending, which nothing left can settle. An entry that is not a
// promise, with no promise job queued, completes exactly as before.
//
// Dependency suspension and cancellation (decision 4). A job that waits for
// a compiler dependency keeps its suspended activations where they are, in
// its tables: a wait in place keeps them in its worker, and a saved
// continuation carries them in its snapshot (and its queue in Saved,
// compiler_session.goose). A saved continuation detaches the job's whole
// stack the same way (suspend_stack, light-cljs-813): every frame, native
// frames included, with its locals, operands and handlers becomes one more
// Activation row, which the restoring worker attaches at the same heights
// (resume_stack) and the job's next save reuses. Cancellation unwinds the running frames, running
// their finally blocks, and drops suspended activations and queued jobs
// with the job's tables; a yield or await reached by a finally block that a
// cancellation runs abandons that block and goes on to the next one, as the
// Pants engine's does.

// A detached activation (Cont.Activation): a run of frames, bottom first,
// with their locals, operands and handlers. The frames are Frame rows at
// heap.conts[frames..frames + frame_count] in a run of frame_capacity;
// their locals then operands are at heap.values[values..values + locals +
// operands] in a run of `capacity` rows; their handlers are Saved rows at
// heap.conts[handlers..handlers + handler_count] in a run of
// handler_capacity. A generator's or an async function's activation is one
// frame; a dependency wait's saved continuation is the job's whole stack,
// native frames included (light-cljs-813, suspend_stack).
struct Activation { frames: u32, frame_count: u32, frame_capacity: u32, values: u32, locals: u32, operands: u32, capacity: u32, handlers: u32, handler_count: u32, handler_capacity: u32 }
// A frame of a detached run (Cont.Frame): source::Frame's fields, its bases
// relative to the run's; function no_row() for a native frame, whose Native
// names its operation and whose input, output and index hold its state.
struct SavedFrame { function: u32 = 4294967295, pc: u32 = 0, local_base: u32 = 0, operand_base: u32 = 0, argument_count: i64 = 0, index: i64 = 0, callee: Value = Value.Undefined, receiver: Value = Value.Undefined, input: Value = Value.Undefined, output: Value = Value.Undefined, native: Native = Native.None }
// A handler of a detached activation: its frame's index in the run and its
// heights relative to the run's local and operand bases.
struct SavedHandler { frame: u32 = 0, pc: i64 = 0, operands: u32 = 0, locals: u32 = 0, kind: HandlerKind = HandlerKind.Catch, value: Value = Value.Undefined }
// A promise's internal slots (Cont.Promise); first and last name its
// pending reactions, activation an async function's detached activation
// (no_row() for none). handled: a reaction was registered
// ([[PromiseIsHandled]]).
struct PromiseState { state: u8 = 0, result: Value = Value.Undefined, first: u32 = 4294967295, last: u32 = 4294967295, activation: u32 = 4294967295, handled: bool = false }
// A reaction of a pending promise: resume the activation of async function
// promise `target` (resume) or settle promise `target` the same way (an
// adoption); next is the promise's next reaction.
struct Reaction { target: u32, resume: bool, next: u32 = 4294967295 }
// A row of the continuation table heap.conts (decision 2's `conts`).
enum Cont { Activation { a: Activation }, Saved { h: SavedHandler }, Promise { p: PromiseState }, Reaction { r: Reaction }, Frame { f: SavedFrame } }
// A promise job (Job.queue): reaction `target`/`resume` with the settled
// value, or, with `thenable` a promise row, the registration of target's
// adoption of that promise.
struct Task { target: u32, resume: bool = false, value: Value = Value.Undefined, rejected: bool = false, thenable: u32 = 4294967295 }

// Exact bytes of rows with enum fields, for the differential tests'
// comparisons (as source::plain_bytes for the other rows).
fn handler_code(k: HandlerKind) -> u8 { return match k { Catch => 0, Finally => 1, Normal => 2, Return => 3, Throw => 4, Cancel => 5, Jump => 6, Async => 7 }; }
fn plain_bytes(c: Cont, out: u8[>..]&) {
    match c {
        Activation x => { let a = x.a; out.push(0); out.append(bytes_of([a.frames, a.frame_count, a.frame_capacity, a.values, a.locals, a.operands, a.capacity, a.handlers, a.handler_count, a.handler_capacity])); },
        Saved x => { out.push(1); out.append(bytes_of([x.h.pc])); out.append(bytes_of([x.h.frame, x.h.operands, x.h.locals])); out.push(handler_code(x.h.kind)); plain_bytes(x.h.value, out); },
        Promise x => { out.push(2); out.push(x.p.state); plain_bytes(x.p.result, out); out.append(bytes_of([x.p.first, x.p.last, x.p.activation])); out.push(if x.p.handled { 1 } else { 0 }); },
        Reaction x => { out.push(3); out.append(bytes_of([x.r.target, x.r.next])); out.push(if x.r.resume { 1 } else { 0 }); },
        Frame x => { let f = x.f; out.push(4); out.append(bytes_of([f.function, f.pc, f.local_base, f.operand_base])); out.append(bytes_of([f.argument_count, f.index, native_code(f.native)])); plain_bytes(f.callee, out); plain_bytes(f.receiver, out); plain_bytes(f.input, out); plain_bytes(f.output, out); },
    }
}
fn plain_bytes(t: Task, out: u8[>..]&) { out.append(bytes_of([t.target, t.thenable])); out.push(if t.resume { 1 } else { 0 }); out.push(if t.rejected { 1 } else { 0 }); plain_bytes(t.value, out); }

inline fn is_promise(heap: const Heap&, v: Value) -> bool { return v.is_object() && node(heap, v.id()).kind == node_kind::Promise; }

// Rows of heap.conts, named like every table's (light-cljs-800).
fn cont_row(heap: const Heap&, r: i64) -> Cont { if r < 0 || r >= heap.conts.len { abort("row is not in table conts"); } return heap.conts[r]; }
fn set_cont(heap: Heap&, r: i64, c: Cont) { if r < 0 || r >= heap.conts.len { abort("row is not in table conts"); } heap.conts[r] = c; }
fn activation(heap: const Heap&, r: i64) -> Activation { return match cont_row(heap, r) { Activation x => x.a, _ => { abort("row is not an activation"); Activation { frames: 0, frame_count: 0, frame_capacity: 0, values: 0, locals: 0, operands: 0, capacity: 0, handlers: 0, handler_count: 0, handler_capacity: 0 } } }; }
fn saved_frame(heap: const Heap&, r: i64) -> SavedFrame { return match cont_row(heap, r) { Frame x => x.f, _ => { abort("row is not a saved frame"); SavedFrame {} } }; }
fn promise_state(heap: const Heap&, p: i64) -> PromiseState { return match cont_row(heap, node(heap, p).function) { Promise x => x.p, _ => { abort("row is not a promise"); PromiseState {} } }; }
fn set_promise_state(heap: Heap&, p: i64, s: PromiseState) { set_cont(heap, node(heap, p).function, Cont.Promise { p: s }); }
fn reaction(heap: const Heap&, r: i64) -> Reaction { return match cont_row(heap, r) { Reaction x => x.r, _ => { abort("row is not a reaction"); Reaction { target: 0, resume: false } } }; }
// The table's one allocation function: `count` copies of `fill`, refused
// with MemoryLimit past the conts limit counted above the image.
fn conts_fit(heap: const Heap&, count: i64) -> bool { return count >= 0 && count <= heap.limits.conts - (heap.conts.len - heap.image.conts); }
fn add_conts(heap: Heap&, fill: Cont, count: i64) -> i64 {
    if !conts_fit(heap, count) { memory(heap); return -1; }
    let at = heap.conts.len; for i in count { heap.conts.push(fill); }
    return at;
}
// A fault after the frame changed ends the job here: the dispatch loop's
// fault path names the frame it ran, which is gone.
fn end_with_fault(job: Job&, frame: Frame) {
    let fault = job.heap.fault; job.heap.fault = Fault.None;
    if fault != Fault.None { terminate(job, fault, frame); }
}
fn unsupported_detail(job: Job&, what: const u8[:]) {
    job.detail.clear(); for c in what { job.detail.push(c); }
    if job.heap.fault == Fault.None { job.heap.fault = Fault.UnsupportedOperand; }
}

// An Activation link as detach's `reuse`: -1 for none.
inline fn reuse_row(r: u32) -> i64 { return if r == no_row() { -1 } else { r }; }
// Detach the activation of the frame at `top`, the job's top frame (see
// above), into row `reuse` (an Activation row, or -1 for a new one), and pop
// it. Returns its row, or -1 with heap.fault set and nothing changed.
fn detach(job: Job&, top: i64, reuse: i64) -> i64 { return detach_run(job, top, reuse, true); }
// Detach the job's whole stack, every frame with its locals, operands and
// handlers, into row `reuse` (-1 for a new one): a dependency wait's saved
// continuation (compiler_session.goose, save), resumed by resume_stack at
// the same heights. It is the compiler session's step, not JavaScript's, so
// it charges no work; its rows count against the job's limits like every
// row. Returns its row, or -1 with heap.fault set and nothing changed.
fn suspend_stack(job: Job&, reuse: i64) -> i64 { return detach_run(job, 0, reuse, false); }
// Attach a whole stack suspend_stack detached on a job with empty stacks.
// False, with heap.fault set and nothing changed, when it does not fit.
fn resume_stack(job: Job&, row: i64) -> bool {
    if job.frames.len != 0 || job.locals.len != 0 || job.operands.len != 0 || job.handlers.len != 0 { job.heap.fault = Fault.InvalidState; return false; }
    return attach_run(job, row, false);
}
// Detach frames from..top of the job, the top of its stacks, with the locals
// and operands above frame `from`'s bases and the handlers of those frames,
// into an Activation row (see Activation). A run that no longer fits its
// reused row is replaced by one twice the old size.
fn detach_run(job: Job&, from: i64, reuse: i64, charged: bool) -> i64 {
    let heap .= job.heap;
    let nf = job.frames.len - from;
    if from < 0 || nf < 0 { heap.fault = Fault.InvalidState; return -1; }
    let lb = if nf > 0 { job.frames[from].local_base } else { job.locals.len }; let ob = if nf > 0 { job.frames[from].operand_base } else { job.operands.len };
    // The job's stacks hold nothing below its first frame.
    if from == 0 && (lb != 0 || ob != 0) { heap.fault = Fault.InvalidState; return -1; }
    let nl = job.locals.len - lb; let no = job.operands.len - ob;
    var hb = job.handlers.len; while hb > 0 && job.handlers[hb - 1].frame >= from { hb -= 1; }
    let nh = job.handlers.len - hb;
    // Stacks never hold a hole (Empty); a run never does either (snapshot::validate).
    for v in job.locals[lb..] { if v.is_hole() { heap.fault = Fault.InvalidState; return -1; } }
    for v in job.operands[ob..] { if v.is_hole() { heap.fault = Fault.InvalidState; return -1; } }
    // Frame bases only rise up the stack, and every pc is an instruction or a native step.
    for f in job.frames[from..] { if f.pc < 0 || f.local_base < lb || f.operand_base < ob { heap.fault = Fault.InvalidState; return -1; } }
    var a = Activation { frames: no_row(), frame_count: nf as u32, frame_capacity: 0, values: 0, locals: nl as u32, operands: no as u32, capacity: 0, handlers: no_row(), handler_count: nh as u32, handler_capacity: 0 };
    if reuse >= 0 { let old = activation(heap, reuse); a.frames = old.frames; a.frame_capacity = old.frame_capacity; a.values = old.values; a.capacity = old.capacity; a.handlers = old.handlers; a.handler_capacity = old.handler_capacity; }
    let frames = if a.frame_capacity >= nf { 0 } else { max(nf, 2 * (a.frame_capacity as i64)) };
    let values = if a.capacity >= nl + no { 0 } else { max(nl + no, 2 * (a.capacity as i64)) };
    let handlers = if a.handler_capacity >= nh { 0 } else { max(nh, 2 * (a.handler_capacity as i64)) };
    let rows = frames + handlers + (if reuse >= 0 { 0 } else { 1 });
    if !values_fit(heap, values) || !conts_fit(heap, rows) { memory(heap); return -1; }
    if charged && !charge(heap, nf + nl + no + nh) { return -1; }
    if frames > 0 { a.frames = add_conts(heap, Cont.Frame { f: SavedFrame {} }, frames) as u32; a.frame_capacity = frames as u32; }
    if values > 0 { a.values = add_values(heap, [], values) as u32; a.capacity = values as u32; }
    if handlers > 0 { a.handlers = add_conts(heap, Cont.Saved { h: SavedHandler {} }, handlers) as u32; a.handler_capacity = handlers as u32; }
    for i in nf {
        let f = job.frames[from + i];
        set_cont(heap, a.frames + i, Cont.Frame { f: SavedFrame { function: if f.function < 0 { no_row() } else { f.function as u32 }, pc: f.pc as u32, local_base: (f.local_base - lb) as u32, operand_base: (f.operand_base - ob) as u32, argument_count: f.argument_count, index: f.index, callee: f.callee, receiver: f.receiver, input: f.input, output: f.output, native: f.native } });
    }
    for i in nl { set_slot(heap, a.values + i, job.locals[lb + i]); }
    for i in no { set_slot(heap, a.values + nl + i, job.operands[ob + i]); }
    for i in nh {
        let h = job.handlers[hb + i];
        set_cont(heap, a.handlers + i, Cont.Saved { h: SavedHandler { frame: (h.frame - from) as u32, pc: h.pc, operands: (h.operands - ob) as u32, locals: (h.locals - lb) as u32, kind: h.kind, value: h.value } });
    }
    var row = reuse;
    if reuse >= 0 { set_cont(heap, reuse, Cont.Activation { a: a }); } else { row = add_conts(heap, Cont.Activation { a: a }, 1); }
    job.frames.resize(from); job.locals.resize(lb); job.operands.resize(ob); job.handlers.resize(hb);
    return row;
}
// Whether activation `row` and `extra` more operands fit the call limits.
fn attach_fits(job: const Job&, row: i64, extra: i64) -> bool {
    let heap .= job.heap;
    let a = activation(heap, row);
    let n: i64 = a.locals; let m: i64 = a.operands; let need = n + m + extra; let frames: i64 = a.frame_count; let handlers: i64 = a.handler_count;
    return frames <= heap.limits.frames - job.frames.len && need <= heap.limits.stack - job.locals.len - job.operands.len && handlers <= heap.limits.stack - job.handlers.len;
}
// Attach activation `row` on top of the job's stacks, at their current
// heights. False, with StackLimit set and nothing changed, when its frames,
// values or handlers would pass the call limits (Limits).
fn attach(job: Job&, row: i64) -> bool { return attach_run(job, row, true); }
fn attach_run(job: Job&, row: i64, charged: bool) -> bool {
    let heap .= job.heap;
    let a = activation(heap, row);
    let n = (a.locals + a.operands) as i64;
    if !attach_fits(job, row, 0) { heap.fault = Fault.StackLimit; return false; }
    if charged && !charge(heap, a.frame_count + n) { return false; }
    let lb = job.locals.len; let ob = job.operands.len; let fb = job.frames.len;
    for i in a.locals { job.locals.push(slot(heap, a.values + i)); }
    for i in a.operands { job.operands.push(slot(heap, a.values + a.locals + i)); }
    for i in a.frame_count {
        let f = saved_frame(heap, a.frames + i);
        job.frames.push(Frame { function: if f.function == no_row() { -1 } else { f.function }, pc: f.pc, local_base: lb + f.local_base, operand_base: ob + f.operand_base, callee: f.callee, receiver: f.receiver, argument_count: f.argument_count, input: f.input, output: f.output, index: f.index, native: f.native });
    }
    for i in a.handler_count {
        match cont_row(heap, a.handlers + i) {
            Saved s => { job.handlers.push(Handler { frame: fb + s.h.frame, pc: s.h.pc, operands: ob + s.h.operands, locals: lb + s.h.locals, kind: s.h.kind, value: s.h.value }); },
            _ => { abort("row is not a saved handler"); },
        }
    }
    return true;
}

// ---------------------------------------------------------------- generators

// Whether built-in b is a generator's next, return or throw (a native frame).
fn generator_method(b: Builtin) -> bool { return b == Builtin.GeneratorPrototypeNext || b == Builtin.GeneratorPrototypeReturn || b == Builtin.GeneratorPrototypeThrow; }
// The generator a yield or an exception completes.
fn complete_generator(heap: Heap&, g: i64) { if node(heap, g).kind == node_kind::Generator { node_mut(heap, g).length = generator_state::Completed; } }
// {value, done}, a new ordinary object (CreateIterResultObject).
fn iter_result(job: Job&, value: Value, done: bool) -> Value {
    let heap .= job.heap;
    let o = allocate(heap, Node { kind: node_kind::Object });
    if heap.fault != Fault.None || !realm_value(heap, o.id(), "value", value, attrs_data()) || !realm_value(heap, o.id(), "done", boolean(done), attrs_data()) { return Value.Undefined; }
    return o;
}
// The generator object's [[Prototype]]: Get(F, "prototype") if an object,
// else %GeneratorPrototype% (OrdinaryCreateFromConstructor). F's lazy
// `prototype` is made first, as a read makes it.
fn generator_prototype_of(job: Job&, callee: Value) -> Value {
    return prototype_from_constructor(job, callee, object(generator_prototype_row()), "a generator function's prototype getter");
}
// GeneratorInitialYield at `at`, after the parameters are bound.
fn initial_yield(job: Job&, top: i64, at: i64) {
    let heap .= job.heap;
    let proto = generator_prototype_of(job, job.frames[top].callee);
    if heap.fault != Fault.None { return; }
    let g = allocate(heap, Node { kind: node_kind::Generator, function: no_row(), proto: proto });
    if heap.fault != Fault.None { return; }
    let row = detach(job, top, -1);
    if row < 0 { return; }
    node_mut(heap, g.id()).function = row as u32;
    deliver(job, g);
}
// GeneratorYield at `at`: the frame below is the generator's Resume frame.
fn generator_yield(job: Job&, top: i64, at: i64) {
    let heap .= job.heap;
    if job.cancelling { unwind_cancel(job, frame_at(job, top, at)); return; }
    var resumed = false; if top > 0 { resumed = match job.frames[top - 1].native { Resume => true, _ => false }; }
    if !resumed { heap.fault = Fault.InvalidState; return; }
    let g: i64 = job.frames[top - 1].input.id();
    let value = job.operands.pop();
    let row = detach(job, top, node(heap, g).function);
    if row < 0 { job.operands.push(value); return; }
    node_mut(heap, g).function = row as u32; node_mut(heap, g).length = generator_state::SuspendedYield;
    job.operands.push(value);
}
// next(v), return(v) or throw(v) called on `receiver`: a Resume frame whose
// first step runs the generator's state machine (resume_step).
fn start_resume(job: Job&, b: Builtin, receiver: Value, v: Value, base: i64) {
    let heap .= job.heap;
    job.operands.resize(base);
    if !receiver.is_object() || node(heap, receiver.id()).kind != node_kind::Generator { type_error(job, "not a generator"); return; }
    if node(heap, receiver.id()).length == generator_state::Executing { type_error(job, "cannot invoke a running generator"); return; }
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
    let kind: u8 = if b == Builtin.GeneratorPrototypeNext { resume_kind::Next } else if b == Builtin.GeneratorPrototypeReturn { resume_kind::Return } else { resume_kind::Throw };
    job.frames.push(Frame { function: -1, pc: 0, local_base: job.locals.len, operand_base: base, callee: Value.Undefined, receiver: receiver, input: receiver, output: v, native: Native.Resume { kind: kind } });
}
// The Resume frame's steps: pc 0 starts the resumption, pc 1 takes the
// value the generator yielded (state 1) or returned (still 2).
fn resume_step(job: Job&, frame: Frame) {
    let heap .= job.heap; let at = job.frames.len - 1; let g: i64 = frame.input.id();
    let kind: i64 = match frame.native { Resume r => r.kind, _ => resume_kind::Next };
    if frame.pc == 0 {
        var state = node(heap, g).length;
        if state == generator_state::Executing { type_error(job, "cannot invoke a running generator"); return; }
        if !writable(job, g) { heap.fault = Fault.NativeBoundary; return; }
        if state == generator_state::SuspendedStart && kind != resume_kind::Next { complete_generator(heap, g); state = generator_state::Completed; }
        if state == generator_state::Completed {
            if kind == resume_kind::Throw { raise(job, frame.output, frame); return; }
            let r = iter_result(job, if kind == resume_kind::Return { frame.output } else { Value.Undefined }, true);
            if heap.fault == Fault.None { finish_native(job, frame, r); }
            return;
        }
        // A resumption past the call limits leaves the generator suspended:
        // the Resume frame goes first, so the unwind does not complete it.
        if !attach_fits(job, node(heap, g).function, if state == generator_state::SuspendedYield { 2 } else { 0 }) {
            job.frames.pop(); job.operands.resize(frame.operand_base); heap.fault = Fault.StackLimit; return;
        }
        node_mut(heap, g).length = generator_state::Executing; job.frames[at].pc = 1;
        if !attach(job, node(heap, g).function) { return; }
        if state == generator_state::SuspendedYield { job.operands.push(frame.output); job.operands.push(number(kind)); }
        return;
    }
    let value = job.operands.pop();
    let done = node(heap, g).length != generator_state::SuspendedYield;
    if done { complete_generator(heap, g); }
    let r = iter_result(job, value, done);
    if heap.fault == Fault.None { finish_native(job, frame, r); }
}

// ---------------------------------------------------------------- promises

// A new pending promise, or Undefined with heap.fault set.
fn new_promise(heap: Heap&) -> Value {
    if !nodes_fit(heap, 1) { memory(heap); return Value.Undefined; }
    let row = add_conts(heap, Cont.Promise { p: PromiseState {} }, 1);
    if row < 0 { return Value.Undefined; }
    return allocate(heap, Node { kind: node_kind::Promise, function: row as u32, proto: object(promise_prototype_row()) });
}
fn queue_task(job: Job&, t: Task) {
    if job.queue.len - job.queue_head >= job.heap.limits.conts { memory(job.heap); return; }
    job.queue.push(t);
}
// Register reaction (target, resume) on promise q (PerformPromiseThen).
fn add_reaction(job: Job&, q: i64, target: u32, resume: bool) {
    let heap .= job.heap;
    var s = promise_state(heap, q);
    if s.state != promise_status::Pending {
        if s.state == promise_status::Rejected && !s.handled { if !writable(job, q) { heap.fault = Fault.NativeBoundary; return; } s.handled = true; set_promise_state(heap, q, s); }
        queue_task(job, Task { target: target, resume: resume, value: s.result, rejected: s.state == promise_status::Rejected });
        return;
    }
    if !writable(job, q) { heap.fault = Fault.NativeBoundary; return; }
    let row = add_conts(heap, Cont.Reaction { r: Reaction { target: target, resume: resume } }, 1);
    if row < 0 { return; }
    if s.last != no_row() { var last = reaction(heap, s.last); last.next = row as u32; set_cont(heap, s.last, Cont.Reaction { r: last }); } else { s.first = row as u32; }
    s.last = row as u32; s.handled = true; set_promise_state(heap, q, s);
}
// Fulfill or reject pending promise p: one job per reaction, in order.
fn settle_promise(job: Job&, p: i64, value: Value, rejected: bool) {
    let heap .= job.heap;
    var s = promise_state(heap, p);
    if s.state != promise_status::Pending { return; }
    if !writable(job, p) { heap.fault = Fault.NativeBoundary; return; }
    var at = s.first;
    while at != no_row() && heap.fault == Fault.None { let x = reaction(heap, at); queue_task(job, Task { target: x.target, resume: x.resume, value: value, rejected: rejected }); at = x.next; }
    if heap.fault != Fault.None { return; }
    s.state = if rejected { promise_status::Rejected } else { promise_status::Fulfilled }; s.result = value; s.first = no_row(); s.last = no_row();
    set_promise_state(heap, p, s);
    if rejected && !s.handled { job.rejections.push(p as u32); }
}
// Resolve promise p with v (the promise resolve functions): a promise is
// adopted by a later job, p itself is a TypeError rejection, a thenable
// object is not implemented, and anything else fulfills p.
fn resolve_promise(job: Job&, p: i64, v: Value) {
    let heap .= job.heap;
    if is_promise(heap, v) {
        if v.id() == p {
            let e = error_object(heap, fault_type(Fault.TypeError), error_code(Fault.TypeError), text(heap, "cannot resolve a promise with itself"));
            if heap.fault == Fault.None { settle_promise(job, p, e, true); }
            return;
        }
        // Adoption is the intrinsic `then` (still %Promise.prototype%'s);
        // a promise whose visible `then` is gone or not callable is a value.
        let t = promise_then(job, v);
        if t < 0 { return; }
        if t == 0 { queue_task(job, Task { target: p as u32, thenable: v.id() }); return; }
        settle_promise(job, p, v, false);
        return;
    }
    if v.is_object() && thenable(job, v) { return; }
    if heap.fault == Fault.None { settle_promise(job, p, v, false); }
}
// Promise v's visible `then`: 0 the intrinsic one (no own `then`, its
// [[Prototype]] %Promise.prototype% with its own, still the pending
// placeholder the promises area replaces), 1 none callable, -1 a
// different callable `then` or a getter, which fails by name.
fn promise_then(job: Job&, v: Value) -> i64 {
    let heap .= job.heap;
    let k = key_named(heap, "then");
    if heap.fault != Fault.None { return -1; }
    let own = own_entry(heap, v, k);
    if heap.fault != Fault.None { return -1; }
    let proto = prototype(heap, v.id());
    if !own.found && proto.is_object() && proto.id() == promise_prototype_row() {
        let p = own_entry(heap, proto, k);
        if heap.fault != Fault.None { return -1; }
        if p.found && is_pending(p.value) { return 0; }
    }
    return if thenable(job, v) { -1 } else { 1 };
}
// Whether object v has a callable or getter `then`: a thenable, whose
// resolution (calling `then`) is not implemented; it fails by name.
fn thenable(job: Job&, v: Value) -> bool {
    let heap .= job.heap;
    let k = key_named(heap, "then");
    if heap.fault != Fault.None { return true; }
    let then, getter = lookup(heap, v, k);
    if heap.fault != Fault.None { return true; }
    if getter.is_object() || callable(heap, then) { unsupported_detail(job, "resolving with a thenable object"); return true; }
    return false;
}

// ---------------------------------------------------------------- async functions

// AsyncStart: the function's promise and its completion boundary.
fn async_start(job: Job&, top: i64) {
    let heap .= job.heap;
    let p = new_promise(heap);
    if heap.fault != Fault.None { return; }
    push_handler(job, top, HandlerKind.Async, job.frames[top].pc);
    if heap.fault == Fault.None { job.handlers[job.handlers.len - 1].value = p; }
}
// The async frame of Async handler h (its handlers above h already gone)
// returns (rejected false) or throws `value`: its promise is settled and the
// frame ends, delivering the promise.
fn async_settle(job: Job&, h: Handler, value: Value, rejected: bool, frame: Frame) {
    let heap .= job.heap; let p: i64 = h.value.id();
    if rejected { settle_promise(job, p, value, true); } else { resolve_promise(job, p, value); }
    if heap.fault != Fault.None { end_with_fault(job, frame); return; }
    let f = job.frames[h.frame];
    job.frames.resize(h.frame); job.locals.resize(f.local_base); job.operands.resize(f.operand_base);
    deliver(job, h.value);
}
// The Async handler of the frame at `top`, or -1.
fn async_handler(job: const Job&, top: i64) -> i64 {
    var i = job.handlers.len - 1;
    while i >= 0 && job.handlers[i].frame == top { if job.handlers[i].kind == HandlerKind.Async { return i; } i -= 1; }
    return -1;
}
// Await at `at` (the specification's Await with PromiseResolve).
fn await_value(job: Job&, top: i64, at: i64) {
    let heap .= job.heap;
    if job.cancelling { unwind_cancel(job, frame_at(job, top, at)); return; }
    let i = async_handler(job, top);
    if i < 0 { heap.fault = Fault.InvalidState; return; }
    let p = job.handlers[i].value;
    let v = job.operands[job.operands.len - 1];
    var q = v;
    if !is_promise(heap, v) {
        if v.is_object() && thenable(job, v) { return; }
        q = new_promise(heap);
        if heap.fault != Fault.None { return; }
        settle_promise(job, q.id(), v, false);
        if heap.fault != Fault.None { return; }
    }
    if !conts_fit(heap, 1) { memory(heap); return; }
    // Registering the reaction writes q's state unless q is fulfilled or a
    // handled rejection: a row the job did not allocate fails first.
    let qs = promise_state(heap, q.id());
    if (qs.state == promise_status::Pending || qs.state == promise_status::Rejected && !qs.handled) && !writable(job, q.id()) { heap.fault = Fault.NativeBoundary; return; }
    job.operands.pop();
    let row = detach(job, top, reuse_row(promise_state(heap, p.id()).activation));
    if row < 0 { job.operands.push(v); return; }
    var s = promise_state(heap, p.id()); s.activation = row as u32; set_promise_state(heap, p.id(), s);
    add_reaction(job, q.id(), p.id(), true);
    if heap.fault != Fault.None { end_with_fault(job, frame_at_row(job, row, at)); return; }
    deliver(job, p);
}
// A location for a fault after the frame at `at` detached.
fn frame_at_row(job: const Job&, row: i64, at: i64) -> Frame {
    let f = saved_frame(job.heap, activation(job.heap, row).frames);
    return Frame { function: f.function, pc: at, local_base: 0, operand_base: 0, callee: f.callee, receiver: f.receiver };
}

// ---------------------------------------------------------------- the job queue

// The entry frame finished with `value` (deliver with no frame left).
fn entry_done(job: Job&, value: Value) {
    // A resumed activation's own completion value is nobody's.
    if job.draining { return; }
    if job.queue_head >= job.queue.len && job.rejections.len == 0 && !is_promise(job.heap, value) { job.result = value; job.status = tiny::Status.Complete; return; }
    job.draining = true; job.entry = value;
}
fn nowhere() -> Frame { return Frame { function: -1, pc: -1, local_base: 0, operand_base: 0, callee: Value.Undefined, receiver: Value.Undefined }; }
// One step with no frame left after the entry: the next promise job, or
// the end of the job.
fn drain_step(job: Job&) {
    let heap .= job.heap;
    if !charge(heap) { fail(job, heap.fault, nowhere()); return; }
    if job.steps >= heap.limits.steps { fail(job, Fault.InstructionLimit, nowhere()); return; }
    job.steps += 1;
    if job.queue_head >= job.queue.len { finish_jobs(job); return; }
    let t = job.queue[job.queue_head]; job.queue_head += 1;
    if job.queue_head == job.queue.len { job.queue.clear(); job.queue_head = 0; }
    if t.thenable != no_row() { add_reaction(job, t.thenable, t.target, false); }
    else if !t.resume { if t.rejected { settle_promise(job, t.target, t.value, true); } else { resolve_promise(job, t.target, t.value); } }
    else if attach(job, promise_state(heap, t.target).activation) {
        if t.rejected { raise(job, t.value, job.frames[job.frames.len - 1]); } else { job.operands.push(t.value); }
    }
    if heap.fault != Fault.None { fail(job, heap.fault, if job.frames.len > 0 { job.frames[job.frames.len - 1] } else { nowhere() }); }
}
// The queue is empty: the job ends (see above).
fn finish_jobs(job: Job&) {
    let heap .= job.heap; let e = job.entry;
    let entry: i64 = if is_promise(heap, e) { e.id() } else { -1 };
    for r in job.rejections {
        if r == entry { continue; }
        let s = promise_state(heap, r);
        if !s.handled { job.result = s.result; job.thrown = true; terminate(job, Fault.UnhandledRejection, nowhere()); return; }
    }
    if entry < 0 { job.result = e; job.status = tiny::Status.Complete; return; }
    let s = promise_state(heap, entry);
    if s.state == promise_status::Pending { terminate(job, Fault.Unsettled, nowhere()); return; }
    if s.state == promise_status::Rejected { uncaught(job, s.result, nowhere()); return; }
    job.result = s.result; job.status = tiny::Status.Complete;
}
