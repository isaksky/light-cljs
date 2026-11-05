import std;
import .snapshot;
import .snapshot_fixture;
import .source_vm;
import .form_bridge;
import .vm;

// Finite native isolation probe: one coordinator + at most two execution
// threads, no nested workers and no worker-addressed queue assumptions.
struct Report { id: i64, error: snapshot::Error, fault: source::Fault, copied: i64, restore_ns: i64, heap_bytes: i64, nodes: i64, work: i64, checksum: i64, boot: i64, initializers: i64, producers: i64 }
struct Request { id: i64, identity: i64, exhaust: bool = false }
fn cap(heap: source::Heap&, s: const snapshot::Snapshot&, exhaust: bool) {
    heap.limits.nodes = min(heap.limits.nodes, s.nodes.len + (if exhaust { 0 } else { 512 }));
    heap.limits.values = min(heap.limits.values, s.values.len + 4096);
    heap.limits.properties = min(heap.limits.properties, s.properties.len + 128);
    heap.limits.units = min(heap.limits.units, s.units.len + 256);
    heap.limits.work = min(heap.limits.work, heap.work + 100000);
}
fn heap_ceiling(s: const snapshot::Snapshot&) -> i64 {
    return (s.nodes.len + 512) * bytes_of([source::Node { kind: 0 }]).len
         + (s.values.len + 4096 + s.constants.len) * bytes_of([source::Value.Undefined]).len
         + (s.properties.len + 128) * bytes_of([source::Property {}]).len
         + (s.units.len + 256) * 2 + s.bytes.len + bytes_of(s.sources).len;
}
fn charge(s: const snapshot::Snapshot&) -> i64 {
    // The installed compiler constructs one flat spawn packet and runtime
    // copies it once into worker-owned storage. Finished array arguments are
    // viewed in that packet, not copied a third time by the entry thunk.
    // Include both packets, restored code/bindings/heap maximum, validator
    // bitmap, bounded Job locals/operands/frames/writes, and export allowance.
    let code = bytes_of(s.code_bytes).len + bytes_of(s.functions).len + bytes_of(s.code_constants).len + bytes_of(s.children).len + bytes_of(s.ops).len + bytes_of(s.code_units).len + bytes_of(s.bindings).len;
    return 2 * (snapshot::footprint(s) + bytes_of([Request { id: 0, identity: 1 }]).len) + code + heap_ceiling(s) + max(s.ops.len, s.values.len) + 1048576;
}
thread_fn worker(s: snapshot::Snapshot, request: Request) {
    var artifact = bridge::Graph {};
    var report = Report { id: request.id, error: snapshot::Error.None, fault: source::Fault.None, copied: 2 * snapshot::footprint(s), restore_ns: 0, heap_bytes: 0, nodes: 0, work: 0, checksum: 0, boot: 0, initializers: 0, producers: 0 };
    {
        var code = source::Code {}; var heap = source::Heap { code: code }; var bindings: snapshot::Binding[>..] = [];
        var c = snapshot::Control {}; let restored = snapshot::restore(s, code, heap, bindings, request.identity, c); report.error = c.error;
        report.copied += c.copied_bytes; report.restore_ns = c.restore_ns;
        if restored {
            cap(heap, s, request.exhaust);
            // Bound scratch as well as the arenas. This is ample for the
            // selected real macro corpus, independent of default VM limits.
            heap.limits.stack = min(heap.limits.stack, 1024); heap.limits.frames = min(heap.limits.frames, 64); heap.limits.steps = min(heap.limits.steps, 100000);
            if request.exhaust {
                var job = source::Job { heap: heap }; source::start(job, bindings[0].value, [source::nil(), source::nil(), source::number(16)]);
                source::run(job, -1); report.fault = job.fault; assert(source::retire(job));
            } else {
                let result = snapshot_test::verify(heap, bindings, request.id);
                let out = source::form_nth(heap, result, 1); report.checksum = source::form_nth(heap, source::form_nth(heap, out, 0), 1).num() as i64;
                var b = bridge::Control {}; b.nodes = 128; b.bytes = 32768; b.work = 100000;
                assert(bridge::export_graph(heap, [result], artifact, b));
            }
            report.nodes = heap.nodes.len; report.work = heap.work;
            report.boot = heap.boot_count; report.initializers = heap.initializer_count; report.producers = heap.producer_count;
            let v = snapshot::borrow(heap, bindings, 1); report.heap_bytes = snapshot::payload_bytes(v);
            assert(report.heap_bytes <= charge(s));
        }
    }
    // Independently owned compiler form output survives producer Job and Heap.
    if artifact.nodes.len > 0 { var b = bridge::Control {}; assert(bridge::validate(artifact, b)); }
    assert(qput(report));
}
fn main() {
    var code = source::Code {}; var parent = source::Heap { code: code }; var roots: snapshot::Binding[>..] = []; snapshot_test::prepare(code, parent, roots);
    var c = snapshot::Control {}; let saved = snapshot::capture(parent, roots, c); assert(c.error == snapshot::Error.None);
    let reserved = charge(saved); var resources = snapshot::Resources { limit: 2 * reserved, workers: 2 };
    qlimit<Report>(2);
    // Parent diverges after capture. All workers still see the old cell=40.
    parent.nodes[roots[5].value.id()].value = source::number(900);
    var copied: i64 = c.copied_bytes; var restore_ns: i64 = 0; var max_nodes: i64 = 0; var max_private: i64 = 0; var max_work: i64 = 0;
    for batch in 16 {
        assert(snapshot::reserve(resources, reserved)); let first = thread_spawn_contained(worker, copy(saved), Request { id: batch * 2 + 1, identity: source::reserve_identity() });
        assert(snapshot::reserve(resources, reserved)); let second = thread_spawn_contained(worker, copy(saved), Request { id: batch * 2 + 2, identity: source::reserve_identity(), exhaust: batch == 15 });
        assert(!snapshot::reserve(resources, reserved) && resources.active == 2 && resources.retained == 2 * reserved);
        // Join before receiving: a contained failure cannot strand a blocked
        // receive. Queue capacity covers all admitted reports, including errors.
        var reason: u8[>..] = []; assert(thread_wait(first, reason)); snapshot::release(resources, reserved);
        assert(thread_wait(second, reason)); snapshot::release(resources, reserved);
        var seen: i64 = 0;
        for i in 2 {
            let report, ok = qpoll<Report>(); assert(ok && report.id >= batch * 2 + 1 && report.id <= batch * 2 + 2);
            let bit = 1 << (report.id - batch * 2 - 1); assert((seen & bit) == 0); seen |= bit;
            assert(report.error == snapshot::Error.None && report.boot == 1 && report.initializers == 2 && report.producers == 2);
            assert(report.copied == 2 * snapshot::footprint(saved) + snapshot::payload_bytes(saved));
            if batch == 15 && report.id == 32 { assert(report.fault == source::Fault.MemoryLimit); }
            else { assert(report.fault == source::Fault.None && report.checksum == report.id + 4); }
            copied += report.copied; restore_ns += report.restore_ns; max_nodes = max(max_nodes, report.nodes);
            max_private = max(max_private, report.heap_bytes); max_work = max(max_work, report.work);
        }
        assert(seen == 3 && resources.active == 0 && resources.retained == 0 && reason.len == 0);
        assert(parent.nodes[roots[5].value.id()].value.num() == 900 && parent.boot_count == 1 && parent.producer_count == 2);
    }
    // Invalid data is rejected by the coordinator before any dispatch.
    var bad = copy(saved); bad.header.allocator = 0; var check = snapshot::Control {}; assert(!snapshot::validate(bad, check));
    var too_small = snapshot::Resources { limit: reserved - 1 }; assert(!snapshot::reserve(too_small, reserved) && too_small.active == 0);
    let extra, pending = qpoll<Report>(); assert(!pending && extra.id == 0);
    print("native snapshot workers: 32 jobs, 2 workers, parent/sibling/capture/cycle/form isolation, exhaustion and release passed");
    print("snapshot-bytes=", c.snapshot_bytes, " snapshot-copy-bytes=", copied, " capture-ns=", c.capture_ns, " restore-ns=", restore_ns, " private-reservation-peak=", resources.peak, " max-nodes=", max_nodes, " retained=", resources.retained);
    print("max-private-component-bytes=", max_private, " validator-scratch-bytes=", c.validation_peak, " max-work=", max_work);
}
