import std;
import os;
import .scheduler;
import .dependency;
import .snapshot;
import .snapshot_fixture;
import .source_vm;
import .artifact;
import .form_bridge;
import .effects;
import .heap_program;

fn key(id: i64, generation: i64 = 1, kind: i64 = 0, session: i64 = 1) -> dependency::Key { return dependency::Key { compilation: 1, session: session, generation: generation, definition: id, kind: kind }; }
fn prepare(code: source::Code&, heap: source::Heap&, roots: snapshot::Binding[>..]&, entries: i64[>..]&) {
    entries.push(snapshot_test::append(code, embed_file("../corpus/branches-helper-0.pants.gsp")));
    entries.push(snapshot_test::append(code, embed_file("../corpus/branches-helper-1.pants.gsp")));
    entries.push(snapshot_test::append(code, embed_file("../corpus/branches-consume.pants.gsp")));
    assert(source::initialize(heap)); let rt = source::boot_runtime(heap);
    roots.push(snapshot::Binding { id: 1, generation: 1, value: rt });
    // No helper result exists in this initialized base. This is intentionally
    // distinct from snapshot_fixture's preinitialized isolation workload.
    assert(source::get(heap, heap.global, source::text(heap, "__lightCompilerRoots")).tag() == 0);
}
fn job(id: i64, entry: i64, call: bool = false, delay: i64 = 0) -> scheduler::Spec {
    var s = scheduler::Spec { id: id, key: key(id), entry: entry, runtime_binding: 1, call_result: call, delay_ms: delay };
    if call { s.arguments[0] = source::nil(); s.arguments[1] = source::nil(); s.argument_count = 2; }
    return s;
}
fn check_trace(plan: const scheduler::Plan&, run: const scheduler::Run&) {
    assert(run.active == 0 && run.peak <= run.workers && run.joined == run.workers);
    var started: bool[>..] = []; var installed: bool[>..] = []; started.resize(plan.jobs.len, false); installed.resize(run.table.cells.len, false);
    for event in run.events {
        if event.kind == scheduler::EventKind.Start {
            assert(!started[event.job]); started[event.job] = true;
            for need in plan.needs { if need.consumer == plan.jobs[event.job].id { let cell = dependency::find(run.table, need.key); assert(installed[cell] && run.table.cells[cell].terminal.outcome == dependency::Outcome.Success); } }
        }
        if event.kind == scheduler::EventKind.Install { assert(!installed[event.cell]); installed[event.cell] = true; }
    }
    for cell, i in run.table.cells { assert(installed[i] && cell.terminal.outcome != dependency::Outcome.Pending); }
    for state in run.states { assert(state.starts <= 1 && (!state.dispatched || state.received)); }
    for edge in run.table.edges { assert(edge.delivered); var count: i64 = 0; for delivery in run.table.deliveries { if delivery.cell == edge.cell && delivery.consumer == edge.consumer { count += 1; } } assert(count == 1); }
}
// Messages between the heap-free main and the thread programs that each own
// one heap (light-cljs-794).
struct Base { image: snapshot::Snapshot, entries: i64[] }
struct Exported { nodes: bridge::Form[], children: i64[], units: u16[], sources: source::SourceRange[], bytes: u8[], roots: i64[] }
struct Restored { identity: i64 }
fn verify_artifacts(plan: const scheduler::Plan&, run: const scheduler::Run&, base: const snapshot::Snapshot&, consumer: i64, graph: bridge::Graph&) {
    var results: scheduler::Result[>..] = []; for result in run.results { if result.id == consumer { results.push(copy(result)); } }
    heap_program::join(thread_spawn(import_artifacts, copy(base), copy(results)));
    let out = qget<Exported>(); graph.nodes.append(out.nodes); graph.children.append(out.children); graph.units.append(out.units); graph.sources.append(out.sources); graph.bytes.append(out.bytes); graph.roots.append(out.roots);
}
thread_fn import_artifacts(base: snapshot::Snapshot, results: scheduler::Result[]) {
    defer { heap_program::finish(); }
    var graph = bridge::Graph {};
    var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = []; var sc = snapshot::Control {};
    assert(snapshot::restore(base, code, heap, bindings, source::reserve_identity(), sc));
    var imports = artifact::Imports { base: base.header.checkpoint, session: heap.identity };
    for result in results {
        assert(result.terminal.outcome == dependency::Outcome.Success);
        var c = artifact::Control {}; let value = artifact::import_graph(result.artifact, base, heap, imports, c); assert(c.error == artifact::Error.None);
        let expansion = source::form_nth(heap, value, 1); assert(source::form_count(heap, expansion) == 4);
        for expected, i in [2, 3, 9, 16] { assert(source::form_nth(heap, expansion, i).num() == expected); }
        var bc = bridge::Control {}; assert(bridge::export_graph(heap, [value], graph, bc));
    }
    assert(graph.roots.len == 1);
    assert(qput(Exported { nodes: copy(graph.nodes), children: copy(graph.children), units: copy(graph.units), sources: copy(graph.sources), bytes: copy(graph.bytes), roots: copy(graph.roots) }));
}
fn schedule_delay(id: i64, seed: i64) -> i64 { return ((id * 1103515245 + seed * 12345) & 127) % 16; }
fn graphs(base: const snapshot::Snapshot&, entries: const i64[:]) {
    for workers in [1, 2, 4, 8] {
        for shape in 3 {
            var canonical: u8[>..] = []; var canonical_graph = bridge::Graph {};
            for reverse in 2 {
                let seed = [17, 42][reverse];
                var plan = scheduler::Plan {};
                // A four-job chain, a wide fan-out/fan-in, and a diamond. The
                // first two exercise compiler-declared ordering; the diamond
                // executes actual emitted helper reads, with no prefilled roots.
                if shape == 0 {
                    for i in 4 { plan.jobs.push(job(i, entries[0], false, schedule_delay(i, seed))); if i > 0 { plan.needs.push(scheduler::Need { consumer: i, key: key(i - 1) }); } }
                } else if shape == 1 {
                    for i in 10 { plan.jobs.push(job(i, entries[0], false, schedule_delay(i, seed))); }
                    for i in 1..9 { plan.needs.push(scheduler::Need { consumer: i, key: key(0) }); plan.needs.push(scheduler::Need { consumer: 9, key: key(i) }); }
                } else {
                    plan.jobs.push(job(0, entries[0]));
                    plan.jobs.push(job(1, entries[0], false, schedule_delay(1, seed)));
                    plan.jobs.push(job(2, entries[1], false, schedule_delay(2, seed)));
                    plan.needs.push(scheduler::Need { consumer: 1, key: key(0) }); plan.needs.push(scheduler::Need { consumer: 2, key: key(0) });
                    plan.jobs.push(job(3, entries[2], true)); plan.jobs.push(job(4, entries[2], true));
                    for i in 3..5 {
                        plan.needs.push(scheduler::Need { consumer: i, key: key(1), name: "cljs.user/left-result" });
                        plan.needs.push(scheduler::Need { consumer: i, key: key(2), name: "cljs.user/right-result" });
                    }
                    // Idempotent duplicate demand from the same consumer.
                    plan.needs.push(scheduler::Need { consumer: 3, key: key(1), name: "cljs.user/left-result" });
                }
                var table = dependency::Table { compilation: 1, session: 1 }; var run = scheduler::Run { table: table };
                assert(scheduler::dispatch(plan, base, run, scheduler::Options { workers: workers })); check_trace(plan, run);
                assert(run.admitted == plan.jobs.len && table.requests.len == plan.jobs.len);
                for state, i in run.states { assert(scheduler::terminal(run, i).outcome == dependency::Outcome.Success && state.starts == 1); }
                var out: u8[>..] = []; scheduler::output(plan, run, out);
                if reverse == 0 { canonical.append(out); } else { assert(out == canonical); }
                if shape == 2 {
                    var graph = bridge::Graph {}; var other = bridge::Graph {}; verify_artifacts(plan, run, base, 3, graph); verify_artifacts(plan, run, base, 4, other);
                    if reverse == 0 { canonical_graph.nodes.append(graph.nodes); canonical_graph.children.append(graph.children); canonical_graph.units.append(graph.units); }
                    else { assert(bytes_of(graph.nodes) == bytes_of(canonical_graph.nodes) && graph.children == canonical_graph.children && graph.units == canonical_graph.units); }
                }
                print("scheduler shape=", shape, " reorder=", reverse, " seed=", seed, " coordinator=1 workers=", run.workers, " jobs=", run.admitted, " peak-admitted=", run.peak, " joined=", run.joined);
            }
        }
    }
}
fn failures(base: const snapshot::Snapshot&, entries: const i64[:]) {
    var canonical: u8[>..] = [];
    for reverse in 2 {
        var plan = scheduler::Plan {};
        plan.jobs.push(job(0, entries[2], true, if reverse == 0 { 10 } else { 0 })); // real unresolved read -> VM failure, never replay
        plan.jobs.push(job(1, entries[0])); plan.needs.push(scheduler::Need { consumer: 1, key: key(0) });
        plan.jobs.push(job(2, entries[0])); // unrelated succeeds
        plan.jobs.push(job(3, entries[0])); plan.needs.push(scheduler::Need { consumer: 3, key: key(99) });
        plan.jobs.push(job(4, entries[0])); plan.needs.push(scheduler::Need { consumer: 4, key: key(5) });
        plan.jobs.push(job(5, entries[0])); plan.needs.push(scheduler::Need { consumer: 5, key: key(4) });
        var unsupported = job(6, entries[0]); unsupported.mid_job_need = true; plan.jobs.push(unsupported);
        // Wrong result kinds/versions must not be satisfied by producer 2.
        plan.jobs.push(job(7, entries[0])); plan.needs.push(scheduler::Need { consumer: 7, key: key(2, 1, 1) });
        plan.jobs.push(job(8, entries[0])); plan.needs.push(scheduler::Need { consumer: 8, key: key(2, 2) });
        var table = dependency::Table { compilation: 1, session: 1 }; var run = scheduler::Run { table: table };
        assert(scheduler::dispatch(plan, base, run, scheduler::Options { workers: 4 }));
        check_trace(plan, run); assert(run.joined == 4 && run.admitted == 2 && run.active == 0 && run.states[0].fault == source::Fault.DependencyNeed && run.states[0].fault_function >= 0 && run.states[0].fault_pc >= 0);
        assert(scheduler::terminal(run, 0).diagnostic == dependency::Diagnostic.MidJobNeed && scheduler::terminal(run, 1).diagnostic == dependency::Diagnostic.Prerequisite);
        assert(scheduler::terminal(run, 2).outcome == dependency::Outcome.Success);
        for i in [3, 7, 8] { assert(scheduler::terminal(run, i).diagnostic == dependency::Diagnostic.Prerequisite && run.states[i].starts == 0); }
        assert(scheduler::terminal(run, 4).diagnostic == dependency::Diagnostic.Cycle && scheduler::terminal(run, 5).diagnostic == dependency::Diagnostic.Cycle);
        assert(scheduler::terminal(run, 6).diagnostic == dependency::Diagnostic.MidJobNeed && run.states[6].starts == 0);
        var out: u8[>..] = []; scheduler::output(plan, run, out); if reverse == 0 { canonical.append(out); } else { assert(out == canonical); }
        assert(table.requests.len == 12 && run.unresolved.len == 3); // nine jobs + three unresolved compiler requests
    }
    for workers in [1, 2, 4, 8] {
        var plan = scheduler::Plan {}; for i in 16 { plan.jobs.push(job(i, entries[0], false, 10)); }
        var table = dependency::Table { compilation: 1, session: 1 }; var run = scheduler::Run { table: table };
        assert(scheduler::dispatch(plan, base, run, scheduler::Options { workers: workers, cancel_after: workers }));
        assert(run.cancelled && run.joined == workers && run.active == 0 && run.admitted >= workers && run.admitted <= 2 * workers);
        check_trace(plan, run);
        for result in run.results { let i = scheduler::position(plan, result.id); assert(result.terminal.outcome == scheduler::terminal(run, i).outcome); }
        var cancelled: i64 = 0; for state, i in run.states { if scheduler::terminal(run, i).outcome == dependency::Outcome.Cancelled { cancelled += 1; } }
        assert(cancelled >= plan.jobs.len - run.admitted && cancelled > 0);
        // No old work/result/stop leaks into a new compilation session.
        let extra, got = qpoll<scheduler::Result>(); assert(!got && extra.id == 0);
        let work, pending = qpoll<scheduler::Work>(); assert(!pending && !work.stop);
    }
    print("scheduler failure propagation/unrelated work/missing/version/kind/cycles/mid-job rejection/cancel/join passed");
}
fn saturation(base: const snapshot::Snapshot&, entries: const i64[:]) {
    var plan = scheduler::Plan {}; for i in 16 { plan.jobs.push(job(i, entries[0])); }
    var table = dependency::Table { compilation: 1, session: 1 }; var run = scheduler::Run { table: table };
    assert(scheduler::dispatch(plan, base, run, scheduler::Options { workers: 8, receive_delay_ms: 50 }));
    assert(run.admitted == 16 && run.joined == 8); check_trace(plan, run);
    full_queues(base, entries);
    print("scheduler capacity=1 retained all work/results/stops; refusals=", run.work_retries, "/", run.result_retries, "/", run.stop_retries);
}
// The parent image's own thread program. Dispatches run in other programs,
// so they cannot reach this heap: no live job, one boot, no producer.
thread_fn base_image() {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; var entries: i64[>..] = []; prepare(code, heap, roots, entries);
    var c = snapshot::Control {}; let base = snapshot::capture(heap, roots, c); assert(c.error == snapshot::Error.None);
    assert(heap.live_jobs == 0 && heap.boot_count == 1 && heap.producer_count == 0);
    assert(qput(Base { image: copy(base), entries: copy(entries) }));
}
// main builds no heap; every heap is its own thread program.
fn main() {
    heap_program::join(thread_spawn(base_image)); let message = qget<Base>(); let base .= message.image; let entries .= message.entries;
    let started = clock_ns(); install_orders(base, entries); graphs(base, entries); failures(base, entries); saturation(base, entries); keys_and_restore(base, entries); budgets_and_control(base, entries);
    print("native dependency-ready pool passed; clean prerequisite import; all owned workers joined; elapsed-ms=", (clock_ns() - started) / 1000000);
}

fn keys_and_restore(base: const snapshot::Snapshot&, entries: const i64[:]) {
    var plan = scheduler::Plan {};
    var left = job(501, entries[0]); left.key = key(77, 1, 0);
    var right = job(123, entries[1]); right.key = key(77, 1, 1);
    var version = job(987, entries[0]); version.key = key(77, 2, 0);
    plan.jobs.push(left); plan.jobs.push(right); plan.jobs.push(version); plan.jobs.push(job(9, entries[2], true));
    plan.needs.push(scheduler::Need { consumer: 9, key: left.key, name: "cljs.user/left-result" });
    plan.needs.push(scheduler::Need { consumer: 9, key: right.key, name: "cljs.user/right-result" });
    plan.needs.push(scheduler::Need { consumer: 9, key: version.key });
    var table = dependency::Table { compilation: 1, session: 1 }; var run = scheduler::Run { table: table };
    assert(scheduler::dispatch(plan, base, run, scheduler::Options { workers: 4 })); check_trace(plan, run);
    assert(table.cells.len == 4 && run.admitted == 4);
    var graph = bridge::Graph {}; verify_artifacts(plan, run, base, 9, graph);
    for result in run.results { let i = scheduler::position(plan, result.id); assert(result.artifact.definition == run.states[i].cell); }
    // A restored VM identity is not a producer claim. Restore two full private
    // heaps while one coordinator subscription/claim remains pending.
    var authority = dependency::Table { compilation: 1, session: 1, next_producer: 1000000 };
    let read = dependency::demand(authority, 11, key(1));
    for n in 2 {
        heap_program::join(thread_spawn(restore_private, copy(base))); let heap = qget<Restored>();
        assert(!dependency::install(authority, dependency::Completion { key: key(1), producer: heap.identity, terminal: dependency::Terminal { outcome: dependency::Outcome.Success } }));
        assert(dependency::demand(authority, 11, key(1)).producer == 0 && authority.requests.len == 1 && authority.edges.len == 1 && authority.deliveries.len == 0);
    }
    assert(dependency::install(authority, dependency::Completion { key: key(1), producer: read.producer, terminal: dependency::Terminal { outcome: dependency::Outcome.Success } }));
    assert(authority.deliveries.len == 1);
    heap_program::join(thread_spawn(root_reads, copy(base)));
    print("scheduler same-definition different version/kind imports and snapshot authority exclusion passed");
}
// A full private heap restored while the coordinator's claim stays pending.
thread_fn restore_private(base: snapshot::Snapshot) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; var c = snapshot::Control {};
    assert(snapshot::restore(base, code, heap, roots, source::reserve_identity(), c));
    assert(qput(Restored { identity: heap.identity }));
}
thread_fn root_reads(base: snapshot::Snapshot) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; var c = snapshot::Control {};
    assert(snapshot::restore(base, code, heap, roots, source::reserve_identity(), c));
    let receiver = source::allocate(heap, source::Node { kind: 0 }); let name = source::text(heap, "root");
    assert(source::put(heap, receiver, name, source::Value.Undefined));
    assert(source::form_op(heap, form_op::CljReadRoot, [receiver, source::nil(), name]).tag() == 0 && heap.fault == source::Fault.None);
    source::form_op(heap, form_op::CljReadRoot, [receiver, source::nil(), source::text(heap, "missing")]); assert(heap.fault == source::Fault.DependencyNeed);
}
fn budgets_and_control(base: const snapshot::Snapshot&, entries: const i64[:]) {
    for mode in 5 {
        var plan = scheduler::Plan {}; plan.jobs.push(job(0, entries[0])); plan.jobs.push(job(1, entries[0]));
        plan.needs.push(scheduler::Need { consumer: 1, key: key(0) });
        var table = dependency::Table { compilation: 1, session: 1 }; var run = scheduler::Run { table: table }; var options = scheduler::Options { workers: 2 };
        if mode == 0 { options.steps = 1; }
        if mode == 1 { options.work = 1; }
        if mode == 2 { options.artifact_bytes = 1; }
        if mode == 3 { options.packet_bytes = 1; }
        if mode == 4 { let empty_artifact = artifact::Artifact {}; options.retained_bytes = 2 * artifact::footprint(empty_artifact) + 2; }
        assert(scheduler::dispatch(plan, base, run, options));
        assert(run.joined == 2 && run.active == 0 && run.retained_bytes <= options.retained_bytes);
        if mode == 0 { assert(run.states[0].fault == source::Fault.InstructionLimit); }
        if mode == 1 { assert(run.states[0].fault == source::Fault.WorkLimit); }
        if mode == 2 || mode == 4 { assert(scheduler::terminal(run, 0).diagnostic == dependency::Diagnostic.Artifact); }
        if mode == 3 { assert(scheduler::terminal(run, 0).outcome == dependency::Outcome.Success && scheduler::terminal(run, 1).diagnostic == dependency::Diagnostic.Budget && run.states[1].starts == 0); }
    }
    // An already queued cancellation belongs to its exact compilation/session.
    for session in [1, 2] {
        var plan = scheduler::Plan {}; var s = job(0, entries[0]); s.key = key(0, 1, 0, session); plan.jobs.push(s);
        assert(qput(scheduler::Cancel { compilation: 1, session: 1 }));
        var table = dependency::Table { compilation: 1, session: session }; var run = scheduler::Run { table: table };
        assert(scheduler::dispatch(plan, base, run, scheduler::Options { workers: 2 }));
        assert(run.joined == 2 && run.cancelled == (session == 1) && run.admitted == (if session == 1 { 0 } else { 1 }));
    }
    // Malformed compiler plans are refused before claims, threads or queues.
    for mode in 5 {
        var plan = scheduler::Plan {}; var s = job(0, entries[0]);
        if mode == 0 { s.key.session = 2; }
        if mode == 1 { s.entry = base.functions.len; }
        if mode == 2 { s.arguments[0] = source::object(1); s.argument_count = 1; }
        plan.jobs.push(s);
        if mode == 3 { plan.jobs.push(s); }
        if mode == 4 { plan.needs.push(scheduler::Need { consumer: 999, key: key(0) }); }
        var table = dependency::Table { compilation: 1, session: 1 }; var run = scheduler::Run { table: table };
        assert(!scheduler::dispatch(plan, base, run, scheduler::Options {}) && run.workers == 0 && table.cells.len == 0);
    }
    print("scheduler bounded VM/artifact/packet/retention failures, scoped cancellation and atomic malformed-plan refusal passed");
}

fn full_queues(base: const snapshot::Snapshot&, entries: const i64[:]) {
    // Occupy each capacity-one queue before its consumers exist. Pressure
    // acknowledgement proves an actual refused result put, with no sleep or
    // timing assumption; then drain and observe the retry's identical result.
    qlimit<scheduler::Work>(1); qlimit<scheduler::Result>(1); qlimit<scheduler::Pressure>(128);
    let first = scheduler::Work { spec: job(40, entries[0]), producer: 11, identity: source::reserve_identity(), publication: 40 };
    let second = scheduler::Work { spec: job(41, entries[1]), producer: 12, identity: source::reserve_identity(), publication: 41 };
    assert(qput(first) && !qput(second));
    let sentinel = scheduler::Result { id: -1, key: key(0), producer: 0, terminal: dependency::Terminal { outcome: dependency::Outcome.Cancelled } };
    assert(qput(sentinel));
    let a = thread_spawn(scheduler::worker, copy(base), scheduler::Options {}); let b = thread_spawn(scheduler::worker, copy(base), scheduler::Options {});
    while !qput(second) { sleep_ms(1); }
    let refused = qget<scheduler::Pressure>(); assert(refused.id == 40 || refused.id == 41);
    assert(qget<scheduler::Result>().id == -1);
    var seen: i64 = 0; var retried: i64 = 0;
    for n in 2 {
        let result = qget<scheduler::Result>(); assert(result.id == 40 || result.id == 41);
        let bit = 1 << (result.id - 40); assert((seen & bit) == 0); seen |= bit;
        assert(result.terminal.outcome == dependency::Outcome.Success && result.producer == result.id - 29 && result.artifact.definition == result.id);
        retried += result.result_retries;
    }
    assert(seen == 3 && retried > 0);
    let stop = scheduler::Work { stop: true, spec: job(0, entries[0]), producer: 0, identity: 0 };
    for n in 2 { while !qput(stop) { sleep_ms(1); } }
    assert(thread_wait(a) && thread_wait(b));
    loop { let pressure, pending = qpoll<scheduler::Pressure>(); if !pending { break; } assert(pressure.id == 40 || pressure.id == 41); }
    // Force the stop refusal before either worker can remove the first stop.
    assert(qput(stop) && !qput(stop));
    let c = thread_spawn(scheduler::worker, copy(base), scheduler::Options {}); let d = thread_spawn(scheduler::worker, copy(base), scheduler::Options {});
    while !qput(stop) { sleep_ms(1); }
    assert(thread_wait(c) && thread_wait(d));
    let extra, result_pending = qpoll<scheduler::Result>(); let pending_work, work_pending = qpoll<scheduler::Work>();
    assert(!result_pending && !work_pending && extra.id == 0 && !pending_work.stop);
    print("scheduler deterministic full work/result/stop queues: two correlated jobs, four joined workers and refused puts all retained");
}

fn install_orders(base: const snapshot::Snapshot&, entries: const i64[:]) {
    // Receiver event hook: hold both completed producer packets outside the
    // coordinator, then explicitly release them in each order. No sleep is
    // involved in proving which Install occurs first. The regular dispatch
    // matrix above independently checks admission at every worker bound.
    for seed in [0, 1, 17, 42] {
        var plan = scheduler::Plan {}; plan.jobs.push(job(1, entries[0])); plan.jobs.push(job(2, entries[1])); plan.jobs.push(job(3, entries[2], true));
        plan.needs.push(scheduler::Need { consumer: 3, key: key(1), name: "cljs.user/left-result" });
        plan.needs.push(scheduler::Need { consumer: 3, key: key(2), name: "cljs.user/right-result" });
        var table = dependency::Table { compilation: 1, session: 1 }; var run = scheduler::Run { table: table };
        for spec in plan.jobs { let claim = dependency::ensure(table, spec.key); run.states.push(scheduler::State { cell: claim.cell }); }
        for need in plan.needs { dependency::demand(table, need.consumer, need.key); }
        var held: scheduler::Result[>..] = []; var order: scheduler::Result&[>..] = [];
        for i in 2 {
            assert(scheduler::ready(plan, run, i)); let packet = scheduler::make_work(plan, run, i, source::reserve_identity());
            run.states[i].dispatched = true; run.states[i].starts = 1; run.active += 1;
            var authority = effects::Authority {}; let result .= held.push(scheduler::perform_isolated(base, packet, scheduler::Options {}, authority)); order.push(result);
        }
        let first = seed % 2; let second = 1 - first;
        assert(!scheduler::ready(plan, run, 2));
        scheduler::accept(plan, base, run, scheduler::Options {}, order[first]);
        assert(!scheduler::ready(plan, run, 2) && table.deliveries.len == 1);
        let producer = table.cells[run.states[first].cell];
        assert(!dependency::install(table, dependency::Completion { key: producer.key, producer: producer.producer, terminal: producer.terminal }));
        assert(table.deliveries.len == 1); // duplicate completion cannot release the join
        scheduler::accept(plan, base, run, scheduler::Options {}, order[second]);
        assert(scheduler::ready(plan, run, 2) && table.deliveries.len == 2);
        assert(run.events[1].kind == scheduler::EventKind.Install && run.events[1].job == first);
        assert(run.events[3].kind == scheduler::EventKind.Install && run.events[3].job == second);
        let packet = scheduler::make_work(plan, run, 2, source::reserve_identity());
        run.states[2].dispatched = true; run.states[2].starts = 1; run.active += 1;
        var authority = effects::Authority {}; let result = scheduler::perform_isolated(base, packet, scheduler::Options {}, authority);
        scheduler::accept(plan, base, run, scheduler::Options {}, result);
        assert(run.active == 0 && table.requests.len == 3);
        var graph = bridge::Graph {}; verify_artifacts(plan, run, base, 3, graph);
        print("deterministic diamond receiver seed=", seed, " install=", first, ",", second, " duplicate ignored; join expansion/provenance retained");
    }
}
