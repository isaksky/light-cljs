import std;
import os;
import .source_vm;
import .snapshot;
import .snapshot_fixture;
import .scheduler;
import .effects;
import .dependency;
import .artifact;
import .form_bridge;
import .export_schema;
import .heap_program;

// Hand-assembled counter leaves are protocol fixtures, not emitted macros.
// The unchanged compiler-emitted effect.cljs macro is exercised separately.
fn fixture(code: source::Code&, kinds: const i64[:], fail: bool = false) -> i64 {
    var part = source::Code {}; let names = ["unused", "unused", "unused", "unused", "unused", "gensym", "sharedIncrement", "sharedRead", "external", "missing"];
    for name in names { let at = part.units.len; for byte in name { part.units.push(byte); } part.strings.push(source::Str { start: at as u32, len: name.len as u32 }); part.constants.push(source::Value.String { s: (part.strings.len - 1) as u32 }); }
    let count = kinds.len * 3 + (if fail { 2 } else { 0 });
    part.functions.push(source::Function { start: 0, count: count as u32, constants: 0, children: 0, params: 0, locals: 0, captures: 0, strict: true });
    for kind, i in kinds {
        for op, j in [opcode::GetGlobal, opcode::Call, if i == kinds.len - 1 && !fail { opcode::Return } else { opcode::Pop }] { part.ops.push(source::Op { code: op, arg: if j == 0 { kind } else { 0 }, offset: part.ops.len, start: 0, end: 0, depth: if j == 0 { 0 } else { 1 } }); }
    }
    if fail { part.ops.push(source::Op { code: opcode::GetGlobal, arg: 9, offset: part.ops.len, start: 0, end: 0, depth: 0 }); part.ops.push(source::Op { code: opcode::Return, arg: 0, offset: part.ops.len, start: 0, end: 0, depth: 1 }); }
    var offset: i64 = 0; for &op in part.ops { op.offset = offset; offset += 1 + (export_descriptor(op.code) >> 16); }
    part.valid = true; return source::append_code(code, part);
}
fn prepare(code: source::Code&, heap: source::Heap&, roots: snapshot::Binding[>..]&, entries: i64[>..]&) {
    entries.push(fixture(code, [6])); entries.push(fixture(code, [7])); entries.push(fixture(code, [8])); entries.push(fixture(code, [6, 6, 7])); entries.push(fixture(code, [6], true));
    entries.push(snapshot_test::append(code, embed_file("../corpus/serial/mutations.gsp")));
    entries.push(snapshot_test::append(code, embed_file("../corpus/effect-announce.pants.gsp")));
    var captured = source::Code {}; captured.functions.push(source::Function { start: 0, count: 3, constants: 0, children: 0, params: 0, locals: 0, captures: 1, strict: true });
    for op, i in [opcode::GetCapture, opcode::CellGet, opcode::Return] { captured.ops.push(source::Op { code: op, arg: 0, offset: [0, 3, 4][i], start: 0, end: 0, depth: [0, 1, 1][i] }); } captured.valid = true; entries.push(source::append_code(code, captured));
    var mapped = source::Code {}; mapped.functions.push(source::Function { start: 0, count: 5, constants: 0, children: 0, params: 3, locals: 3, captures: 0, strict: true });
    for op, i in [opcode::GetLocal, opcode::GetLocal, opcode::GetLocal, opcode::Call, opcode::Return] { mapped.ops.push(source::Op { code: op, arg: [0, 1, 2, 2, 0][i], offset: [0, 3, 6, 9, 12][i], start: 0, end: 0, depth: [0, 1, 2, 3, 1][i] }); } mapped.valid = true; entries.push(source::append_code(code, mapped));
    assert(source::initialize(heap)); let rt = source::boot_runtime(heap); roots.push(snapshot::Binding { id: 1, generation: 1, value: rt });
    for kind, i in [6, 7, 8] { let leaf = source::builtin(heap, kind); assert(source::install_field(heap, heap.global, ["sharedIncrement", "sharedRead", "external"][i], leaf)); }
}
fn grant(id: i64, owner: i64, sequence: i64, kind: i64, resource: i64 = 0) -> effects::Grant { return effects::Grant { id: id, owner: owner, sequence: sequence, kind: kind, resource: resource }; }
fn key(id: i64) -> dependency::Key { return dependency::Key { compilation: 1, session: 1, generation: 1, definition: id, kind: 0 }; }
fn spec(id: i64, entry: i64, ordered: bool = false) -> scheduler::Spec { return scheduler::Spec { id: id, key: key(id), entry: entry, ordered: ordered }; }
// An owned copy of the entry list for a thread program's arguments.
fn owned(entries: const i64[:]) -> i64[>..] { var out: i64[>..] = []; out.append(entries); return out; }
fn check(run: const scheduler::Run&) {
    assert(run.active == 0 && run.joined == run.workers && run.peak <= run.workers);
    for state in run.states { assert(state.starts <= 1 && (!state.dispatched || state.received)); }
}
fn counters(base: const snapshot::Snapshot&, entries: const i64[:]) {
    for workers in [1, 2, 4, 8] { for quantum in [1, 2, 3, 4, 5, 8, 1000000] {
        var plan = scheduler::Plan {};
        plan.jobs.push(spec(30, entries[0], true)); plan.jobs.push(spec(10, entries[0], true)); plan.jobs.push(spec(20, entries[1], true));
        plan.effects.append([grant(100, 30, 0, 6), grant(200, 10, 0, 6), grant(300, 20, 0, 7)]);
        // The dependent read waits for the second write's artifact as well as
        // the compiler-declared effect order; unrelated private mutations run.
        plan.needs.push(scheduler::Need { consumer: 20, key: key(10) });
        for i in 6 { plan.jobs.push(spec(40 + i, entries[5])); }
        var table = dependency::Table { compilation: 1, session: 1 }; var run = scheduler::Run { table: table };
        assert(scheduler::dispatch(plan, base, run, scheduler::Options { workers: workers, quantum: quantum })); check(run);
        assert(run.effect_receipts.len == 3 && run.effect_counters[0] == 2 && run.admitted == 9);
        for r, i in run.effect_receipts { assert(r.grant.id == [100, 200, 300][i] && r.value == [1, 2, 2][i]); }
        for result in run.results {
            assert(result.terminal.outcome == dependency::Outcome.Success);
            if result.id < 40 { assert(result.artifact.root.num() == (if result.id == 30 { 1 } else { 2 })); }
            else { assert(result.artifact.root.tag() == 2 && result.artifact.root.flag()); }
        }
        if quantum == 1000000 { print("effects workers=", workers, " serialized-restore/execute/publish-ns=", run.ordered_ns, " authority-service-ns=", run.effect_service_ns, " pure-worker-sum-ns=", run.pure_ns); }
    } }
    print("counter serial 1/2, dependent read 2, pure local mutation isolation, all worker limits/quanta passed");
}
fn failures(base: const snapshot::Snapshot&, entries: const i64[:]) {
    for workers in [1, 2, 4, 8] { for mode in 7 {
        var plan = scheduler::Plan {};
        // mode0: pure read; mode1: pure write; mode2: undeclared second effect;
        // mode3: external effect; mode4: host delivery error; mode5: VM failure
        // after a committed effect; mode6: a declared effect was not executed.
        // Subsequent ordered effects never overtake.
        let first = if mode == 0 { entries[1] } else if mode == 2 { entries[3] } else if mode == 3 { entries[2] } else if mode == 5 { entries[4] } else if mode == 6 { entries[5] } else { entries[0] };
        plan.jobs.push(spec(10, first, mode >= 2)); plan.jobs.push(spec(20, entries[0], true)); plan.jobs.push(spec(30, entries[5]));
        if mode == 2 || mode >= 4 { plan.effects.push(grant(100, 10, 0, 6)); }
        plan.effects.push(grant(200, 20, 0, 6));
        var table = dependency::Table { compilation: 1, session: 1 }; var run = scheduler::Run { table: table };
        assert(scheduler::dispatch(plan, base, run, scheduler::Options { workers: workers, quantum: 1, effect_fail_id: if mode == 4 { 100 } else { -1 } })); check(run);
        assert(scheduler::terminal(run, 0).outcome == dependency::Outcome.Failed && scheduler::terminal(run, 2).outcome == dependency::Outcome.Success);
        assert(run.states[0].fault == (if mode == 4 { source::Fault.HostEffect } else if mode == 5 { source::Fault.ReferenceError } else { source::Fault.EffectContract }));
        if mode >= 2 { assert(scheduler::terminal(run, 1).diagnostic == dependency::Diagnostic.Prerequisite && run.states[1].starts == 0); }
        else { assert(scheduler::terminal(run, 1).outcome == dependency::Outcome.Success); }
        let expected = if mode == 3 || mode == 6 { 0 } else { 1 };
        assert(run.effect_receipts.len == expected && run.effect_counters[0] == (if mode == 3 || mode == 4 || mode == 6 { 0 } else { 1 }));
    } }
    print("pure shared reads/writes, undeclared/external effects, ordered failures, terminal host error, unrelated work/join passed");
}
fn duplicates() {
    var a = effects::Authority {}; let g = grant(1, 10, 0, 6); let next = grant(2, 10, 1, 7); a.grants.append([g, next]);
    assert(effects::commit(a, next).fault == source::Fault.EffectContract && a.next == 0);
    let r = effects::commit(a, g); assert(r.value == 1);
    for i in 10 { assert(effects::commit(a, g).value == 1 && a.receipts.len == 1 && a.counters[0] == 1); }
    var conflict = g; conflict.owner = 20; assert(effects::commit(a, conflict).fault == source::Fault.EffectContract && a.counters[0] == 1);
    assert(effects::commit(a, next).value == 1 && a.receipts.len == 2);
    var failed = effects::Authority { fail_id: 1 }; failed.grants.push(g);
    assert(effects::commit(failed, g).fault == source::Fault.HostEffect && effects::commit(failed, g).fault == source::Fault.HostEffect && failed.counters[0] == 0 && failed.receipts.len == 1);
    var named = effects::Authority {}; let sym = grant(3, 10, 0, 5); named.grants.push(sym);
    assert(effects::commit(named, sym, ['x' as u16]).value == 1 && effects::commit(named, sym, ['x' as u16]).value == 1);
    assert(effects::commit(named, sym, ['y' as u16]).fault == source::Fault.EffectContract && named.counters[0] == 1);
    print("logical effect duplicate/conflict/out-of-order/failure receipts are idempotent");
}
fn emitted(base: const snapshot::Snapshot&, entries: const i64[:]) {
    for workers in [1, 2, 4, 8] {
        var plan = scheduler::Plan {}; var s = spec(10, entries[6], true); s.runtime_binding = 1; s.call_result = true; s.arguments[0] = source::nil(); s.arguments[1] = source::nil(); s.arguments[2] = source::number(7); s.argument_count = 3; plan.jobs.push(s); plan.effects.push(grant(100, 10, 0, 5));
        var table = dependency::Table { compilation: 1, session: 1 }; var run = scheduler::Run { table: table };
        assert(scheduler::dispatch(plan, base, run, scheduler::Options { workers: workers, quantum: 1 })); check(run);
        assert(run.effect_receipts.len == 1 && run.effect_counters[0] == 1 && scheduler::terminal(run, 0).outcome == dependency::Outcome.Success);
        heap_program::join(thread_spawn(emitted_import, copy(base), copy(run.results)));
    }
    print("unchanged compiler-emitted effect.cljs expansion (quote [7 true]) with one declared gensym passed");
}
thread_fn emitted_import(base: snapshot::Snapshot, results: scheduler::Result[]) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; var sc = snapshot::Control {}; assert(snapshot::restore(base, code, heap, roots, source::reserve_identity(), sc));
    var imports = artifact::Imports { base: base.header.checkpoint, session: heap.identity }; var c = artifact::Control {};
    for packet in results { let result = artifact::import_graph(packet.artifact, base, heap, imports, c); assert(c.error == artifact::Error.None);
        let expansion = source::form_nth(heap, result, 1); assert(source::form_nth(heap, expansion, 0).num() == 7 && source::plain(source::form_nth(heap, expansion, 1)).payload == 1); }
}
// Messages between the heap-free main and the thread programs that each own
// one heap (light-cljs-794).
struct Base { image: snapshot::Snapshot, entries: i64[] }
struct Caught { image: snapshot::Snapshot }
thread_fn base_image() {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; var entries: i64[>..] = []; prepare(code, heap, roots, entries);
    var sc = snapshot::Control {}; let base = snapshot::capture(heap, roots, sc); assert(sc.error == snapshot::Error.None);
    assert(qput(Base { image: copy(base), entries: owned(entries) }));
}
// main builds no heap; every heap is its own thread program.
fn main() {
    heap_program::join(thread_spawn(base_image)); let message = qget<Base>(); let base .= message.image; let entries .= message.entries;
    let arguments = args(); if arguments.len == 2 && arguments[1] == "--measure" { measure(base, entries); return; }
    counters(base, entries); failures(base, entries); duplicates(); emitted(base, entries); suspension(base, entries); contracts(base, entries);
    heap_program::join(thread_spawn(result_failure, copy(base), owned(entries))); native_callbacks(base, entries); prefix_rejections(base, entries); caught_errors();
}

// Synthetic fault injection into the same handler used by emitted catches.
// Covers all three runtime-error routes separately from explicit JS throws.
fn caught_errors() {
    for fault in [source::Fault.TypeError, source::Fault.ReferenceError, source::Fault.HostEffect] { for limited in [false, true] {
        heap_program::join(thread_spawn(caught_error, fault, limited));
        if !limited { let caught = qget<Caught>(); heap_program::join(thread_spawn(caught_restore, copy(caught.image))); }
    } }
    print("caught TypeError/ReferenceError/HostEffect identity, false nil equality, snapshot retention, realm name and message and terminal allocation failure passed");
}
// The caught error's snapshot restores into a heap of its own.
thread_fn caught_restore(saved: snapshot::Snapshot) {
    defer { heap_program::finish(); }
    var sc = snapshot::Control {};
    var restored_code = source::Code {}; var restored = source::Heap { code: restored_code }; var roots: snapshot::Binding[>..] = [];
    assert(snapshot::restore(saved, restored_code, restored, roots, source::reserve_identity(), sc));
    assert(source::truthy(restored, roots[0].value) && !source::equal(restored, roots[0].value, source::nil()));
    var graph = bridge::Graph {}; var bc = bridge::Control {}; assert(!bridge::export_graph(restored, [roots[0].value], graph, bc) && bc.error == bridge::Error.Unsupported);
    // The realm's error object (light-cljs-788): its type's `name`, inherited,
    // and an own non-empty `message`.
    let name = source::get(restored, roots[0].value, source::text(restored, "name")); let message = source::get(restored, roots[0].value, source::text(restored, "message"));
    assert(restored.fault == source::Fault.None && name.tag() == 4 && message.tag() == 4 && source::str_len(restored, message) > 0);
}
thread_fn caught_error(fault: source::Fault, limited: bool) {
    defer { heap_program::finish(); }
    {
        var code = source::Code {}; code.constants.push(source::nil());
        code.functions.push(source::Function { start: 0, count: 7, constants: 0, children: 0, params: 0, locals: 1, captures: 0, strict: true });
        var offset: i64 = 0;
        for op, i in [opcode::Undefined, opcode::Return, opcode::SetLocal, opcode::GetLocal, opcode::Const, opcode::StrictEq, opcode::Return] { code.ops.push(source::Op { code: op, arg: 0, offset: offset, start: 0, end: 0, depth: [0, 1, 1, 0, 1, 2, 1][i] }); offset += 1 + (export_descriptor(op) >> 16); } source::decode(code); code.valid = true;
        var heap = source::Heap { code: code }; assert(source::initialize(heap));
        var job = source::Job { heap: heap }; assert(source::start(job, source::closure(heap, 0, []), []));
        job.handlers.push(source::Handler { frame: 0, pc: 2, operands: 0, locals: 1 });
        if limited { heap.limits.nodes = heap.nodes.len - heap.image.nodes; }
        source::fail(job, fault, job.frames[0]);
        if limited { assert(source::run(job, -1) == tiny::Outcome.Failed && job.fault == source::Fault.MemoryLimit && job.fault_function == 0 && job.fault_pc == 0); }
        else {
            let error = job.operands[0]; assert(error.tag() == 5 && source::truthy(heap, error) && source::equal(heap, error, error));
            let other = source::allocate(heap, source::Node { kind: 13 }); assert(!source::equal(heap, error, other) && !source::form_equal(heap, error, other) && source::form_equal(heap, error, error));
            assert(source::run(job, -1) == tiny::Outcome.Complete && job.result.tag() == 2 && !job.result.flag());
            assert(source::retire(job));
            var sc = snapshot::Control {}; let saved = snapshot::capture(heap, [snapshot::Binding { id: 1, generation: 1, value: error }], sc); assert(sc.error == snapshot::Error.None);
            assert(qput(Caught { image: copy(saved) }));
        }
        assert(source::retire(job));
    }
}

// The compiler may await a prerequisite before delivering a suspended request.
// Adapt 99d4f9fb:tests/resumable.rs and 1ddd5633:src/task.rs: every instruction
// cut and effect-adjacent yield preserves the continuation; no stack/claim is
// held while the host services a dependency or correlated nested helper.
fn suspension(base: const snapshot::Snapshot&, entries: const i64[:]) {
    for failed in 2 { for cut in 10 { for quantum in [1, 2, 3, 4, 9, 1000000] { heap_program::join(thread_spawn(suspended, copy(base), owned(entries), failed, cut, quantum)); } } }
    print("all instruction cuts/effect yields, committed effect before dependency wait/failure once, nested callback after release, overlapping-run rejection passed");
}
thread_fn suspended(base: snapshot::Snapshot, entries: i64[], failed: i64, cut: i64, quantum: i64) {
    defer { heap_program::finish(); }
    {
        var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; var sc = snapshot::Control {}; assert(snapshot::restore(base, code, heap, roots, source::reserve_identity(), sc));
        var authority = effects::Authority {}; authority.grants.append([grant(10, 1, 0, 6), grant(11, 1, 1, 6), grant(12, 1, 2, 7)]); var sequence: i64 = 0;
        var job = source::Job { heap: heap, effects_enabled: true }; assert(source::start(job, source::closure(heap, entries[3], []), []));
        assert(source::claim(job)); let before = job.steps;
        assert(!source::claim(job) && source::run(job, 1) == tiny::Outcome.Busy && !source::retire(job) && !effects::service(authority, job, 1, sequence) && job.steps == before);
        source::run_claimed(job, cut); assert(!job.claimed);
        var waited = false;
        while job.status == tiny::Status.Ready {
            if job.effect_pending {
                assert(!source::claim(job)); let stable = job.steps; let work = heap.work;
                for i in 3 { assert(source::run(job, quantum) == tiny::Outcome.Yielded && job.steps == stable && heap.work == work && !job.claimed); }
                if sequence == 1 {
                    assert(authority.counters[0] == 1 && authority.receipts.len == 1); waited = true;
                    var table = dependency::Table { compilation: 1, session: 1 }; let request = dependency::demand(table, 1, key(99)); assert(request.producer > 0 && request.terminal.outcome == dependency::Outcome.Pending);
                    // Nested compiler work runs only now, with the original
                    // job's active mutation ownership released. It is not a
                    // restart of the waiting parent and has no shared grants.
                    assert(heap.live_jobs == 1 && !job.claimed);
                    let nested = snapshot_test::invoke(heap, source::closure(heap, entries[5], []), []); assert(nested.tag() == 2 && nested.flag() && heap.live_jobs == 1);
                    assert(dependency::install(table, dependency::Completion { key: key(99), producer: request.producer, terminal: dependency::Terminal { outcome: if failed == 1 { dependency::Outcome.Failed } else { dependency::Outcome.Success } } }));
                    assert(table.deliveries.len == 1 && !job.claimed);
                    if table.deliveries[0].terminal.outcome == dependency::Outcome.Failed { assert(source::deliver_effect(job, source::Value.Undefined, source::Fault.DependencyNeed)); continue; }
                }
                assert(effects::service(authority, job, 1, sequence));
                // Duplicate delivery cannot append a second result or mutate
                // a completed logical operation; its receipt is stable.
                assert(!effects::service(authority, job, 1, sequence) && !source::deliver_effect(job, source::number(100)));
            }
            source::run(job, quantum);
        }
        assert(waited && !job.claimed);
        if failed == 0 { assert(job.status == tiny::Status.Complete && job.result.num() == 2 && authority.receipts.len == 3 && authority.counters[0] == 2); }
        else { assert(job.status == tiny::Status.Failed && job.fault == source::Fault.DependencyNeed && authority.receipts.len == 1 && authority.counters[0] == 1); }
        let steps = job.steps; for i in 3 { assert(source::run(job, -1) == (if failed == 0 { tiny::Outcome.Complete } else { tiny::Outcome.Failed }) && job.steps == steps); }
        assert(source::retire(job) && heap.live_jobs == 0);
    }
}
fn contracts(base: const snapshot::Snapshot&, entries: const i64[:]) {
    for mode in 6 {
        var plan = scheduler::Plan {}; plan.jobs.push(spec(1, entries[0], true)); plan.jobs.push(spec(2, entries[0], true)); plan.effects.append([grant(10, 1, 0, 6), grant(11, 2, 0, 6)]);
        if mode == 0 { plan.effects[1].id = 10; }
        if mode == 1 { plan.effects[0].sequence = 1; }
        if mode == 2 { plan.effects[0].owner = 999; }
        if mode == 3 { plan.effects[0].resource = 16; }
        if mode == 4 { plan.effects[0].kind = 8; }
        if mode == 5 { let first = plan.effects[0]; plan.effects[0] = plan.effects[1]; plan.effects[1] = first; }
        var table = dependency::Table { compilation: 1, session: 1 }; var run = scheduler::Run { table: table };
        assert(!scheduler::dispatch(plan, base, run, scheduler::Options {}) && run.workers == 0 && table.cells.len == 0);
    }
    heap_program::join(thread_spawn(mutable_capture, copy(base), owned(entries)));
    print("compiler grant validation is atomic; forbidden mutable capture rejects with transitive path/reason");
}
// Publishing a branch-local mutable capture cannot merge it with another
// worker: the transitive path and reason survive in the artifact control.
thread_fn mutable_capture(base: snapshot::Snapshot, entries: i64[]) {
    defer { heap_program::finish(); }
    var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; var sc = snapshot::Control {}; assert(snapshot::restore(base, code, heap, roots, source::reserve_identity(), sc));
    let cell = source::allocate(heap, source::Node { kind: 3, initialized: true, value: source::number(9) });
    let value = source::form_collection(heap, 6, [source::closure(heap, entries[7], [cell])]); var imports = artifact::Imports { base: base.header.checkpoint, session: heap.identity }; var c = artifact::Control {};
    artifact::export_graph(heap, base, imports, value, 100, 1, c);
    assert(c.error == artifact::Error.Mutable && c.path.len == 3);
}

fn measure(base: const snapshot::Snapshot&, entries: const i64[:]) {
    for workers in [1, 2, 4, 8] { for sample in 6 {
        var plan = scheduler::Plan {};
        for i in 16 { plan.jobs.push(spec(i, entries[3], true)); for j, n in [6, 6, 7] { plan.effects.push(grant(i * 3 + n, i, n, j)); } }
        for i in 48 { plan.jobs.push(spec(100 + i, entries[5])); }
        let started = clock_ns();
        {
            var table = dependency::Table { compilation: 1, session: 1 }; var run = scheduler::Run { table: table };
            assert(scheduler::dispatch(plan, base, run, scheduler::Options { workers: workers })); check(run);
            assert(run.effect_counters[0] == 32 && run.effect_receipts.len == 48 && run.admitted == 64);
            for state, i in run.states { assert(scheduler::terminal(run, i).outcome == dependency::Outcome.Success); }
            for result in run.results { if result.id < 100 { assert(result.artifact.root.tag() == 3 && result.artifact.root.num() == (result.id + 1) * 2); } else { assert(result.artifact.root.tag() == 2 && result.artifact.root.flag()); } }
            for r, i in run.effect_receipts { assert(r.grant.id == i && r.value == (i / 3) * 2 + (if i % 3 == 0 { 1 } else { 2 })); }
            if sample > 0 { print("effect-sample workers=", workers, " sample=", sample, " ordered-ns=", run.ordered_ns, " service-ns=", run.effect_service_ns, " pure-worker-sum-ns=", run.pure_ns, " dispatch-through-join-ns=", clock_ns() - started); }
        }
        // Retention, authority and result teardown are included in this row.
        if sample > 0 { print("effect-teardown-inclusive workers=", workers, " sample=", sample, " ns=", clock_ns() - started); }
    } }
}

thread_fn result_failure(base: snapshot::Snapshot, entries: i64[]) {
    defer { heap_program::finish(); }
    // Error after the logical host effect committed (while constructing its
    // local symbol result) is terminal too; a retry cannot consume another ID.
    var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; var sc = snapshot::Control {}; assert(snapshot::restore(base, code, heap, roots, source::reserve_identity(), sc));
    let macro = snapshot_test::factory(heap, entries[6], roots[0].value);
    var job = source::Job { heap: heap, effects_enabled: true }; assert(source::start(job, macro, [source::nil(), source::nil(), source::number(7)]));
    assert(source::run(job, -1) == tiny::Outcome.Yielded && job.effect_pending && !job.claimed);
    var authority = effects::Authority {}; let g = grant(1, 1, 0, 5, 2); authority.grants.push(g); var sequence: i64 = 0;
    heap.limits.nodes = heap.nodes.len - heap.image.nodes;
    assert(effects::service(authority, job, 1, sequence) && job.status == tiny::Status.Failed && job.fault == source::Fault.MemoryLimit);
    assert(authority.receipts.len == 1 && authority.counters[2] == 1 && authority.counters[0] == 0);
    assert(effects::commit(authority, g, authority.receipts[0].prefix[..authority.receipts[0].count]).value == 1 && authority.counters[2] == 1);
    assert(source::run(job, -1) == tiny::Outcome.Failed && !effects::service(authority, job, 1, sequence) && source::retire(job));
    // Reuse the heap after terminal cleanup for unrelated compiler work.
    heap.limits.nodes += 1024;
    let nested = snapshot_test::invoke(heap, source::closure(heap, entries[5], []), []); assert(nested.tag() == 2 && nested.flag());
    print("post-commit result construction error stays terminal, original receipt retained, resource isolation and unrelated reuse passed");
}

fn native_callbacks(base: const snapshot::Snapshot&, entries: const i64[:]) {
    for quantum in [1, 2, 3, 4, 5, 16, 1000000] { heap_program::join(thread_spawn(native_callback, copy(base), owned(entries), quantum)); }
    print("effect suspension inside native mapv callback preserves iterator continuation and exact names across quanta");
}
thread_fn native_callback(base: snapshot::Snapshot, entries: i64[], quantum: i64) {
    defer { heap_program::finish(); }
    {
        var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; var sc = snapshot::Control {}; assert(snapshot::restore(base, code, heap, roots, source::reserve_identity(), sc));
        let input = source::form_collection(heap, 6, [source::text(heap, "a"), source::text(heap, "b")]);
        var job = source::Job { heap: heap, effects_enabled: true }; assert(source::start(job, source::closure(heap, entries[8], []), [source::builtin(heap, 2), source::builtin(heap, 5), input]));
        var authority = effects::Authority {}; authority.grants.append([grant(1, 1, 0, 5), grant(2, 1, 1, 5)]); var sequence: i64 = 0;
        while job.status == tiny::Status.Ready { source::run(job, quantum); if job.effect_pending { assert(effects::service(authority, job, 1, sequence)); } }
        assert(job.status == tiny::Status.Complete && sequence == 2 && authority.receipts.len == 2 && authority.counters[0] == 2);
        for i in 2 { let symbol = source::node(heap, source::form_nth(heap, job.result, i).id()); let name = symbol.value; assert(symbol.kind == 9 && source::utf16(heap, name) == (if i == 0 { ['a' as u16, '1'] } else { ['b' as u16, '2'] })); }
        assert(source::retire(job));
    }
}

fn prefix_rejections(base: const snapshot::Snapshot&, entries: const i64[:]) {
    for mode in 4 { heap_program::join(thread_spawn(prefix_rejection, copy(base), owned(entries), mode)); }
    print("unsupported gensym namespace/coercion/prefix length/arity reject before committing an effect");
}
thread_fn prefix_rejection(base: snapshot::Snapshot, entries: i64[], mode: i64) {
    defer { heap_program::finish(); }
    {
        var code = source::Code {}; var heap = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; var sc = snapshot::Control {}; assert(snapshot::restore(base, code, heap, roots, source::reserve_identity(), sc));
        let macro = snapshot_test::factory(heap, entries[6], roots[0].value);
        var job = source::Job { heap: heap, effects_enabled: true }; assert(source::start(job, macro, [source::nil(), source::nil(), source::number(7)])); assert(source::run(job, -1) == tiny::Outcome.Yielded && job.effect_pending);
        if mode == 0 { job.effect_args[0] = source::text(heap, "a/b"); }
        if mode == 1 { job.effect_args[0] = source::number(1); }
        if mode == 2 { let units: u16[129] = [65; 129]; job.effect_args[0] = source::string(heap, units); }
        if mode == 3 { job.effect_count = 2; }
        var authority = effects::Authority {}; authority.grants.push(grant(1, 1, 0, 5)); var sequence: i64 = 0;
        assert(effects::service(authority, job, 1, sequence) && job.status == tiny::Status.Failed && job.fault == source::Fault.EffectContract && authority.receipts.len == 0 && authority.counters[0] == 0 && source::retire(job));
    }
}
