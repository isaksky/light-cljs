import std;
import os;
import .source_vm;
import .snapshot;
import .artifact;
import .dependency;
import .vm;
import .effects;

namespace scheduler;

// Compiler-supplied, already-lowered work. `entry` is a Code function index;
// runtime_binding >= 0 invokes its returned factory with that base binding.
// call_result then invokes the returned macro. The adapter/form transport is
// separate work; this protocol initially accepts at most eight scalar args.
struct Spec {
    id:           i64, key: dependency::Key, entry: i64, runtime_binding: i64 = -1, call_result: bool = false,
    arguments:    source::Value[8]                                            = [source::Value.Undefined; 8], argument_count: i64 = 0,
    ordered:      bool                                                        = false,
    mid_job_need: bool                                                        = false, delay_ms: i64 = 0,
}
struct Need { consumer: i64, key: dependency::Key, name: u8[..128] = [] }
root struct Plan { jobs: Spec[>..] = [], needs: Need[>..] = [], effects: effects::Grant[>..] = [] }
struct Input { name: u8[..128], artifact: artifact::Artifact }
struct Work { stop: bool = false, spec: Spec, producer: i64, identity: i64, publication: i64 = -1, inputs: Input[] = [] }
struct Result {
    id:             i64, key: dependency::Key, producer: i64, terminal: dependency::Terminal,
    fault:          source::Fault = source::Fault.None, artifact: artifact::Artifact = artifact::Artifact {},
    execution_ns:   i64           = 0,
    result_retries: i64           = 0, fault_function: i64 = -1, fault_pc: i64 = -1,
}
struct Cancel { compilation: i64, session: i64 }
// One refusal notice per result, bounded by the sealed plan's 128 jobs.
struct Pressure { id: i64 }
struct State { cell: i64, dispatched: bool = false, received: bool = false, starts: i64 = 0, fault: source::Fault = source::Fault.None, fault_function: i64 = -1, fault_pc: i64 = -1 }
// Physical event order is observable for causality. Stable output is rendered
// separately in submission order; arrival order is deliberately not its order.
enum EventKind { Start, Install, Receive, Stop, Join }
struct Event { kind: EventKind, job: i64, cell: i64 = -1 }
root struct Run {
    table:           dependency::Table&, states: State[>..] = [], results: Result[>..] = [], events: Event[>..] = [],
    workers:         i64                                    = 0, joined: i64 = 0, active: i64 = 0, peak: i64 = 0, admitted: i64 = 0,
    work_retries:    i64                                    = 0, stop_retries: i64 = 0, result_retries: i64 = 0, cancelled: bool = false,
    effect_receipts: effects::Receipt[>..]                  = [], effect_counters: i64[16] = [0; 16], effect_service_ns: i64 = 0, ordered_ns: i64 = 0, pure_ns: i64 = 0,
    unresolved:      dependency::Request[>..]               = [], retained_bytes: i64 = 0, packet_peak: i64 = 0,
}
struct Options { workers: i64 = 1, queue: i64 = 1, steps: i64 = 100000, work: i64 = 1000000, cancel_after: i64 = -1, receive_delay_ms: i64 = 0, artifact_bytes: i64 = 1048576, packet_bytes: i64 = 8388608, retained_bytes: i64 = 67108864, quantum: i64 = 1000000, effect_fail_id: i64 = -1 }
fn execute(authority: effects::Authority&, owner: i64, sequence: i64&, quantum: i64, heap: source::Heap&, callee: source::Value, args: const source::Value[:], terminal: dependency::Terminal&, fault: source::Fault&, fault_function: i64&, fault_pc: i64&) -> source::Value {
    var job = source::Job { heap: heap, effects_enabled: true }; let started = source::start(job, callee, args);
    var outcome = if started { tiny::Outcome.Yielded } else { tiny::Outcome.Failed };
    while outcome == tiny::Outcome.Yielded {
        outcome = source::run(job, quantum);
        if job.effect_pending { assert(effects::service(authority, job, owner, sequence)); }
    }
    let result = job.result;
    if outcome != tiny::Outcome.Complete { terminal.outcome = dependency::Outcome.Failed; terminal.diagnostic = if job.fault == source::Fault.DependencyNeed { dependency::Diagnostic.MidJobNeed } else { dependency::Diagnostic.VM }; fault = job.fault; fault_function = job.fault_function; fault_pc = job.fault_pc; }
    assert(source::retire(job)); return result;
}
fn perform(base: const snapshot::Snapshot&, work: const Work&, options: Options, authority: effects::Authority&) -> Result {
    var result = Result { id: work.spec.id, key: work.spec.key, producer: work.producer, terminal: dependency::Terminal { outcome: dependency::Outcome.Success } };
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = [];
    var sc = snapshot::Control {};
    if !snapshot::restore(base, code, heap, bindings, work.identity, sc) { result.terminal.outcome = dependency::Outcome.Failed; result.terminal.diagnostic = dependency::Diagnostic.Snapshot; return copy(result); }
    heap.limits.steps = min(heap.limits.steps, options.steps); heap.limits.work = min(heap.limits.work, heap.work + options.work);
    // Table limits count the rows this job adds above the restored image.
    heap.limits.nodes = min(heap.limits.nodes, 8192); heap.limits.values = min(heap.limits.values, 32768);
    heap.limits.properties = min(heap.limits.properties, 4096); heap.limits.shapes = min(heap.limits.shapes, 4096); heap.limits.units = min(heap.limits.units, 65536);
    heap.limits.stack = min(heap.limits.stack, 1024); heap.limits.frames = min(heap.limits.frames, 64);
    var imports = artifact::Imports { base: base.header.checkpoint, session: heap.identity };
    var definitions = source::get(heap, heap.global, source::text(heap, "__lightCompilerRoots"));
    if definitions.tag() == 0 { definitions = source::allocate(heap, source::Node { kind: 0 }); source::install_field(heap, heap.global, "__lightCompilerRoots", definitions); }
    // This is a clean heap boundary: no Job exists while prerequisites import.
    for input in work.inputs {
        var c = artifact::Control { bytes: options.packet_bytes }; let value = artifact::import_graph(input.artifact, base, heap, imports, c);
        if c.error != artifact::Error.None { result.terminal.outcome = dependency::Outcome.Failed; result.terminal.diagnostic = dependency::Diagnostic.Artifact; return copy(result); }
        if input.name.len > 0 && !source::install_field(heap, definitions, input.name, value) { result.terminal.outcome = dependency::Outcome.Failed; result.terminal.diagnostic = dependency::Diagnostic.Artifact; return copy(result); }
    }
    if heap.fault != source::Fault.None { result.terminal.outcome = dependency::Outcome.Failed; result.terminal.diagnostic = dependency::Diagnostic.VM; result.fault = heap.fault; return copy(result); }
    let spec = work.spec; var sequence: i64 = 0;
    var value = execute(authority, spec.id, sequence, options.quantum, heap, source::closure(heap, spec.entry, []), [], result.terminal, result.fault, result.fault_function, result.fault_pc);
    if result.terminal.outcome == dependency::Outcome.Success && spec.runtime_binding >= 0 {
        var rt = source::Value.Undefined; for binding in bindings { if binding.id == spec.runtime_binding { rt = binding.value; } }
        value = execute(authority, spec.id, sequence, options.quantum, heap, value, [rt], result.terminal, result.fault, result.fault_function, result.fault_pc);
    }
    if result.terminal.outcome == dependency::Outcome.Success && spec.call_result { value = execute(authority, spec.id, sequence, options.quantum, heap, value, spec.arguments[..spec.argument_count], result.terminal, result.fault, result.fault_function, result.fault_pc); }
    if result.terminal.outcome == dependency::Outcome.Success {
        var expected: i64 = 0; for grant in authority.grants { if grant.owner == spec.id { expected += 1; } }
        if sequence != expected { result.terminal.outcome = dependency::Outcome.Failed; result.terminal.diagnostic = dependency::Diagnostic.VM; result.fault = source::Fault.EffectContract; return copy(result); }
        var c = artifact::Control { bytes: options.artifact_bytes }; let published = artifact::export_graph(heap, base, imports, value, work.publication, spec.key.generation, c);
        if c.error == artifact::Error.None { return Result { id: result.id, key: result.key, producer: result.producer, terminal: result.terminal, fault: result.fault, artifact: copy(published) }; }
        result.terminal.outcome = dependency::Outcome.Failed; result.terminal.diagnostic = dependency::Diagnostic.Artifact;
    }
    return copy(result);
}
// One VM heap per thread program (light-cljs-794): a pool worker builds no
// heap; each job's restored heap lives in a `serve` program it spawns and
// joins before taking the next job.
thread_fn worker(base: snapshot::Snapshot, options: Options) {
    loop {
        let work = qget<Work>(); if work.stop { break; }
        if work.spec.delay_ms > 0 { sleep_ms(work.spec.delay_ms); }
        assert(thread_wait(thread_spawn(serve, copy(base), copy(work), options)));
    }
}
thread_fn serve(base: snapshot::Snapshot, work: Work, options: Options) {
    var pure = effects::Authority {}; let started = clock_ns();
    var result = perform(base, work, options, pure); result.execution_ns = clock_ns() - started;
    while !qput(result) { if result.result_retries == 0 { assert(qput(Pressure { id: result.id })); } result.result_retries += 1; sleep_ms(1); }
}
// The effect authority's flat twin. An ordered job runs in its own program
// while the coordinator waits for it; the coordinator lends its authority for
// exactly that span and adopts the returned state, so one authority exists at
// any time and it never enters a VM snapshot.
struct Lent { grants: effects::Grant[] = [], receipts: effects::Receipt[] = [], counters: i64[16] = [0; 16], next: i64 = 0, fail_id: i64 = -1, service_ns: i64 = 0 }
fn lend(a: const effects::Authority&) -> Lent { return Lent { grants: copy(a.grants), receipts: copy(a.receipts), counters: a.counters, next: a.next, fail_id: a.fail_id, service_ns: a.service_ns }; }
fn adopt(a: effects::Authority&, l: const Lent&) {
    a.grants.clear(); a.grants.append(l.grants); a.receipts.clear(); a.receipts.append(l.receipts);
    a.counters = l.counters; a.next = l.next; a.fail_id = l.fail_id; a.service_ns = l.service_ns;
}
struct Performed { result: Result, authority: Lent }
thread_fn performer(base: snapshot::Snapshot, work: Work, options: Options, lent: Lent) {
    var authority = effects::Authority {}; adopt(authority, lent);
    let result = perform(base, work, options, authority);
    assert(qput(Performed { result: copy(result), authority: lend(authority) }));
}
// perform, with the job's heap in a thread program of its own.
fn perform_isolated(base: const snapshot::Snapshot&, work: const Work&, options: Options, authority: effects::Authority&) -> Result {
    assert(thread_wait(thread_spawn(performer, copy(base), copy(work), options, lend(authority))));
    let done = qget<Performed>(); adopt(authority, done.authority); return copy(done.result);
}
fn position(plan: const Plan&, id: i64) -> i64 { for spec, i in plan.jobs { if spec.id == id { return i; } } return -1; }
fn terminal(run: const Run&, i: i64) -> dependency::Terminal { return run.table.cells[run.states[i].cell].terminal; }
fn finish(run: Run&, i: i64, value: dependency::Terminal) {
    let cell = run.states[i].cell;
    let c = run.table.cells[cell];
    if dependency::install(run.table, dependency::Completion { key: c.key, producer: c.producer, terminal: value }) { run.events.push(Event { kind: EventKind.Install, job: i, cell: cell }); }
}
fn invalid(plan: const Plan&, base: const snapshot::Snapshot&, run: const Run&, options: Options) -> bool {
    if options.quantum < 1 || options.quantum > 1000000 || !effects::valid(plan.effects) || run.effect_receipts.len > 0 || run.states.len > 0 || run.workers != 0 || run.results.len > 0 || run.events.len > 0 || run.table.cells.len > 0 || run.table.edges.len > 0 || run.table.requests.len > 0 || run.table.deliveries.len > 0 || run.table.compilation <= 0 || run.table.session <= 0 || run.table.next_producer <= 0 || options.artifact_bytes < 1 || options.artifact_bytes > 67108864 || options.packet_bytes < 1 || options.packet_bytes > 67108864 || options.retained_bytes < 1 || options.retained_bytes > 67108864 || options.workers < 1 || options.workers > 8 || options.queue < 1 || options.queue > 8 || options.steps < 1 || options.steps > 1000000 || options.work < 1 || options.work > 10000000 || options.receive_delay_ms < 0 || options.receive_delay_ms > 100 || plan.jobs.len > 128 || plan.needs.len > 1024 || run.table.capacity < plan.jobs.len + plan.needs.len || run.table.edge_capacity < plan.needs.len { return true; }
    for spec, i in plan.jobs {
        if !dependency::valid(run.table, spec.key) || spec.id < 0 || spec.entry < 0 || spec.entry >= base.functions.len || spec.argument_count < 0 || spec.argument_count > 8 || spec.delay_ms < 0 || spec.delay_ms > 100 { return true; }
        if spec.runtime_binding >= 0 { var found = false; for binding in base.bindings { if binding.id == spec.runtime_binding { found = true; } } if !found { return true; } }
        for j in i { if plan.jobs[j].id == spec.id || dependency::same(plan.jobs[j].key, spec.key) { return true; } }
        for v in spec.arguments[..spec.argument_count] { if v.tag() < 0 || v.tag() > 3 { return true; } }
    }
    var previous: i64 = -1;
    for grant in plan.effects { let owner = position(plan, grant.owner); if owner < 0 || !plan.jobs[owner].ordered || owner < previous { return true; } previous = owner; }
    for need in plan.needs { if position(plan, need.consumer) < 0 || !dependency::valid(run.table, need.key) || !snapshot::utf8(need.name) { return true; } }
    return false;
}
fn make_work(plan: const Plan&, run: const Run&, i: i64, identity: i64) -> Work {
    var inputs: Input[>..] = [];
    for need in plan.needs {
        if need.consumer != plan.jobs[i].id { continue; }
        let cell = dependency::find(run.table, need.key); let t = run.table.cells[cell].terminal;
        assert(t.outcome == dependency::Outcome.Success);
        var found = false;
        for result, n in run.results { if n == t.payload { inputs.push(Input { name: need.name, artifact: copy(result.artifact) }); found = true; } }
        assert(found);
    }
    let cell = run.table.cells[run.states[i].cell];
    return Work { spec: plan.jobs[i], producer: cell.producer, identity: identity, publication: run.states[i].cell, inputs: copy(inputs) };
}
fn packet_size(plan: const Plan&, run: const Run&, i: i64) -> i64 {
    var bytes: i64 = 0;
    for need in plan.needs {
        if need.consumer != plan.jobs[i].id { continue; }
        let cell = dependency::find(run.table, need.key); let payload = run.table.cells[cell].terminal.payload;
        for result, n in run.results { if n == payload { bytes += artifact::footprint(result.artifact) + 256; } }
    }
    return bytes;
}
fn ready(plan: const Plan&, run: Run&, i: i64) -> bool {
    if run.states[i].dispatched || terminal(run, i).outcome != dependency::Outcome.Pending { return false; }
    if plan.jobs[i].ordered {
        for j in i { if plan.jobs[j].ordered { let prior = terminal(run, j).outcome;
            if prior == dependency::Outcome.Pending { return false; }
            if prior != dependency::Outcome.Success { finish(run, i, dependency::Terminal { outcome: dependency::Outcome.Failed, diagnostic: dependency::Diagnostic.Prerequisite }); return false; }
        } }
    }
    // Inspect all prerequisites before choosing a deterministic failure. A
    // pending earlier edge cannot hide a failed later edge indefinitely.
    var pending = false; var failed = false;
    for need in plan.needs {
        if need.consumer != plan.jobs[i].id { continue; }
        let cell = dependency::find(run.table, need.key); let t = run.table.cells[cell].terminal;
        if t.outcome == dependency::Outcome.Pending { pending = true; }
        else if t.outcome != dependency::Outcome.Success { failed = true; }
    }
    if failed { finish(run, i, dependency::Terminal { outcome: dependency::Outcome.Failed, diagnostic: dependency::Diagnostic.Prerequisite }); return false; }
    return !pending;
}
fn accept(plan: const Plan&, base: const snapshot::Snapshot&, run: Run&, options: Options, result: const Result&) {
    let i = position(plan, result.id); assert(i >= 0 && run.states[i].dispatched && !run.states[i].received);
    let cell = run.table.cells[run.states[i].cell]; assert(dependency::same(cell.key, result.key) && cell.producer == result.producer);
    run.states[i].received = true; run.active -= 1; run.result_retries += result.result_retries;
    run.events.push(Event { kind: EventKind.Receive, job: i });
    var t = result.terminal; let winner = terminal(run, i);
    if winner.outcome != dependency::Outcome.Pending { t = winner; } else { run.states[i].fault = result.fault; run.states[i].fault_function = result.fault_function; run.states[i].fault_pc = result.fault_pc; }
    if t.outcome == dependency::Outcome.Success { var c = artifact::Control { bytes: options.artifact_bytes }; if !artifact::validate(result.artifact, base, c) || result.artifact.definition != run.states[i].cell || result.artifact.version != cell.key.generation { t.outcome = dependency::Outcome.Failed; t.diagnostic = dependency::Diagnostic.Artifact; } }
    if t.outcome == dependency::Outcome.Success && artifact::footprint(result.artifact) > options.retained_bytes - run.retained_bytes { t.outcome = dependency::Outcome.Failed; t.diagnostic = dependency::Diagnostic.Budget; }
    t.payload = if t.outcome == dependency::Outcome.Success { run.results.len } else { -1 };
    if t.outcome == dependency::Outcome.Success && terminal(run, i).outcome == dependency::Outcome.Pending { run.retained_bytes += artifact::footprint(result.artifact); run.results.push(copy(result)); }
    else { run.results.push(Result { id: result.id, key: result.key, producer: result.producer, terminal: t, fault: run.states[i].fault, fault_function: run.states[i].fault_function, fault_pc: run.states[i].fault_pc }); }
    finish(run, i, t);
    if !plan.jobs[i].ordered { run.pure_ns += result.execution_ns; }
}
// A sealed compiler plan is finite. The compiler supplies every entry and
// prerequisite key; absent requests are returned in Run.unresolved and settled
// Missing. There is no name resolution/compilation inside this coordinator.
fn dispatch(plan: const Plan&, base: const snapshot::Snapshot&, run: Run&, options: Options) -> bool {
    if invalid(plan, base, run, options) { return false; }
    var validation = snapshot::Control {}; if !snapshot::validate(base, validation) { return false; }
    // Equal per-job quotas make retention failure independent of completion
    // order. Reserve even for jobs that later fail or never start.
    let empty_artifact = artifact::Artifact {}; let header_bytes = artifact::footprint(empty_artifact);
    let quota = options.retained_bytes / max(1, plan.jobs.len) - header_bytes;
    if quota < 1 { return false; }
    var authority = effects::Authority { fail_id: options.effect_fail_id }; authority.grants.append(plan.effects);
    var effective = options; effective.artifact_bytes = min(options.artifact_bytes, quota);
    for spec in plan.jobs { let claim = dependency::ensure(run.table, spec.key); assert(claim.cell >= 0); run.states.push(State { cell: claim.cell }); }
    for need in plan.needs { assert(dependency::demand(run.table, need.consumer, need.key).cell >= 0); }
    // Compiler could not resolve these exact keys; no execution owns them.
    for cell, i in run.table.cells {
        var supplied = false; for state in run.states { if state.cell == i { supplied = true; } }
        if !supplied { run.unresolved.push(dependency::Request { key: cell.key, producer: cell.producer }); assert(dependency::install(run.table, dependency::Completion { key: cell.key, producer: cell.producer, terminal: dependency::Terminal { outcome: dependency::Outcome.Failed, diagnostic: dependency::Diagnostic.Missing } })); run.events.push(Event { kind: EventKind.Install, job: -1, cell: i }); }
    }
    for spec, i in plan.jobs { if spec.mid_job_need { finish(run, i, dependency::Terminal { outcome: dependency::Outcome.Failed, diagnostic: dependency::Diagnostic.MidJobNeed }); } }
    qlimit<Work>(options.queue); qlimit<Result>(options.queue); qlimit<Cancel>(8); qlimit<Pressure>(128);
    var threads: i64[>..] = [];
    for n in options.workers { threads.push(thread_spawn(worker, copy(base), effective)); run.workers += 1; }
    var slept = false;
    loop {
        let cancel, got_cancel = qpoll<Cancel>();
        if got_cancel && cancel.compilation == run.table.compilation && cancel.session == run.table.session { run.cancelled = true; }
        if options.cancel_after >= 0 && run.admitted >= options.cancel_after { run.cancelled = true; }
        if run.cancelled { for state, i in run.states { if terminal(run, i).outcome == dependency::Outcome.Pending { finish(run, i, dependency::Terminal { outcome: dependency::Outcome.Cancelled, diagnostic: dependency::Diagnostic.Cancelled }); } } }
        var progress = false;
        if !run.cancelled {
            for spec, i in plan.jobs {
                if run.active >= options.workers { break; }
                if ready(plan, run, i) {
                    let packet = packet_size(plan, run, i);
                    if packet > options.packet_bytes { finish(run, i, dependency::Terminal { outcome: dependency::Outcome.Failed, diagnostic: dependency::Diagnostic.Budget }); continue; }
                    run.packet_peak = max(run.packet_peak, packet);
                    let identity = source::reserve_identity(); let work = make_work(plan, run, i, identity);
                    if spec.ordered {
                        run.states[i].dispatched = true; run.states[i].starts += 1; run.admitted += 1; run.active += 1; run.peak = max(run.peak, run.active);
                        run.events.push(Event { kind: EventKind.Start, job: i, cell: run.states[i].cell });
                        let started = clock_ns(); let result = perform_isolated(base, work, effective, authority); run.ordered_ns += clock_ns() - started;
                        accept(plan, base, run, effective, result); progress = true;
                        if options.cancel_after >= 0 && run.admitted >= options.cancel_after { run.cancelled = true; break; }
                    } else if qput(work) {
                        run.states[i].dispatched = true; run.states[i].starts += 1; run.admitted += 1; run.active += 1; run.peak = max(run.peak, run.active);
                        run.events.push(Event { kind: EventKind.Start, job: i, cell: run.states[i].cell }); progress = true;
                        if options.cancel_after >= 0 && run.admitted >= options.cancel_after { run.cancelled = true; break; }
                    } else { run.work_retries += 1; break; }
                }
            }
        }
        if run.cancelled { for state, i in run.states { if terminal(run, i).outcome == dependency::Outcome.Pending { finish(run, i, dependency::Terminal { outcome: dependency::Outcome.Cancelled, diagnostic: dependency::Diagnostic.Cancelled }); } } }
        if !slept && run.active > 0 && options.receive_delay_ms > 0 { sleep_ms(options.receive_delay_ms); slept = true; }
        let result, received = qpoll<Result>();
        if received {
            accept(plan, base, run, effective, result); progress = true;
        }
        var pending = false; for state, i in run.states { if terminal(run, i).outcome == dependency::Outcome.Pending { pending = true; } }
        if run.active == 0 && !pending { break; }
        if !progress && run.active == 0 {
            // Re-run propagation until stable before identifying an unresolved
            // cycle. Unrelated jobs have already completed and stay successful.
            var changed = false;
            for state, i in run.states { let before = terminal(run, i).outcome; ready(plan, run, i); if terminal(run, i).outcome != before { changed = true; } }
            if !changed { for state, i in run.states { if terminal(run, i).outcome == dependency::Outcome.Pending { finish(run, i, dependency::Terminal { outcome: dependency::Outcome.Failed, diagnostic: dependency::Diagnostic.Cycle }); } } }
        }
        if !progress { sleep_ms(1); }
    }
    run.effect_receipts.append(authority.receipts); run.effect_counters = authority.counters; run.effect_service_ns = authority.service_ns;
    // Active is zero only after every admitted result was drained. Stops share
    // the work queue; retrying each stop preserves one stop per owned consumer.
    let empty = Spec { id: 0, key: dependency::Key { compilation: 0, session: 0, generation: 0, definition: 0, kind: 0 }, entry: 0 };
    for n in threads.len { while !qput(Work { stop: true, spec: empty, producer: 0, identity: 0 }) { run.stop_retries += 1; sleep_ms(1); } run.events.push(Event { kind: EventKind.Stop, job: n }); }
    for thread, n in threads { assert(thread_wait(thread)); run.joined += 1; run.events.push(Event { kind: EventKind.Join, job: n }); }
    loop { let pressure, pending = qpoll<Pressure>(); if !pending { break; } assert(position(plan, pressure.id) >= 0); }
    return true;
}
fn output(plan: const Plan&, run: const Run&, out: u8[>..]&) {
    for spec, i in plan.jobs {
        let t = terminal(run, i); format(out, spec.id, ":", t.outcome, ":", t.diagnostic, ":", run.states[i].fault, ":", run.states[i].fault_function, ":", run.states[i].fault_pc, "\n");
    }
}
