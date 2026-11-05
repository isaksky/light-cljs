import std;
import .program;
import .source_vm;
import .vm;
import .heap_program;

fn decode(code: source::Code&, bytes: const u8[:]) {
    var image = program::Image {}; assert(program::install(image, bytes) == program::Error.None);
    assert(source::load(code, image));
}
// Everything same_state compares, flattened so the two differential sides of
// one logical session image can run in separate thread programs, each with
// its own heap (light-cljs-794).
struct State {
    thrown:     bool = false, status: tiny::Status = tiny::Status.Ready, steps: i64 = 0, claimed: bool = false, fault: source::Fault = source::Fault.None, fault_pc: i64 = -1, fault_function: i64 = -1,
    registered: bool = false, live_jobs: i64 = 0, result: u8[] = [], frames: u8[] = [], locals: u8[] = [], operands: u8[] = [], writes: u8[] = [],
    nodes:      u8[] = [], values: u8[] = [], properties: u8[] = [], units: u16[] = [], latin1: u8[] = [], identity: i64 = 0, work: i64 = 0, heap_fault: source::Fault = source::Fault.None,
}
fn state(a: source::Job&) -> State {
    return State {
        thrown:     a.thrown, status: a.status, steps: a.steps, claimed: a.claimed, fault: a.fault, fault_pc: a.fault_pc, fault_function: a.fault_function,
        registered: a.registered, live_jobs: a.heap.live_jobs, result: source::plain_rows([a.result]), frames: source::plain_rows(a.frames), locals: source::plain_rows(a.locals), operands: source::plain_rows(a.operands), writes: source::plain_rows(a.writes),
        nodes:      source::plain_rows(a.heap.nodes), values: source::plain_rows(a.heap.values), properties: source::plain_rows(a.heap.properties), units: copy(a.heap.units), latin1: copy(a.heap.latin1), identity: a.heap.identity, work: a.heap.work, heap_fault: a.heap.fault,
    };
}
fn same_state(a: const State&, b: const State&) {
    // Byte comparisons preserve NaNs and signed zero in every explicit field.
    assert(a.thrown == b.thrown && a.status == b.status && a.steps == b.steps && a.claimed == b.claimed && a.fault == b.fault && a.fault_pc == b.fault_pc && a.fault_function == b.fault_function);
    assert(a.registered == b.registered && a.live_jobs == b.live_jobs);
    assert(a.result == b.result);
    assert(a.frames == b.frames);
    assert(a.locals == b.locals);
    assert(a.operands == b.operands);
    assert(a.writes == b.writes);
    assert(a.nodes == b.nodes);
    assert(a.values == b.values);
    assert(a.properties == b.properties);
    assert(a.units[..] == b.units[..] && a.latin1[..] == b.latin1[..] && a.identity == b.identity && a.work == b.work && a.heap_fault == b.heap_fault);
}
struct Run { status: tiny::Status, steps: i64 }
// States that differ only in Latin-1 bytes are different states.
thread_fn latin1_state_differs(a: State, b: State) { same_state(a, b); }
fn latin1_state_check() {
    let a = State { latin1: [97, 98] }; let b = State { latin1: [97, 99] };
    assert(thread_wait(thread_spawn_contained(latin1_state_differs, copy(a), copy(a))));
    assert(!thread_wait(thread_spawn_contained(latin1_state_differs, copy(a), copy(b))));
}
thread_fn check_run(bytes: u8[], expected: u8[], fault: source::Fault) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let entry = source::closure(heap, 0, []);
    var job = source::Job { heap: heap }; assert(source::start(job, entry, []));
    assert(source::run(job, -1) == (if fault == source::Fault.None { tiny::Outcome.Complete } else { tiny::Outcome.Failed }));
    assert(job.fault == fault);
    if fault == source::Fault.None {
        var output: u8[>..] = []; source::canonical(heap, job.result, output); assert(output[..] == expected);
    } else if fault == source::Fault.Unsupported {
        // The fault names the unimplemented opcode it stopped at.
        var text: u8[>..] = []; source::describe_fault(text, code, job); assert(text[..] == expected);
        assert(job.fault_opcode == code.ops[job.fault_pc].code && !job.thrown);
    }
    assert(qput(Run { status: job.status, steps: job.steps }));
}
// Side a of one cut: publishes its state at the three comparison points.
thread_fn check_cut(bytes: u8[], cut: i64, run: Run, fault: source::Fault) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, bytes);
    var h1 = source::Heap { code: code }; assert(source::initialize(h1));
    let e1 = source::closure(h1, 0, []);
    var a = source::Job { heap: h1 };
    assert(source::start(a, e1, []));
    let before = h1.work;
    assert(source::run(a, 0) == tiny::Outcome.Yielded && a.steps == 0 && h1.work == before);
    assert(qput(state(a)));
    source::run(a, cut);
    assert(qput(state(a)));
    assert(source::claim(a) == (a.status == tiny::Status.Ready));
    if a.claimed {
        assert(source::run(a, 1) == tiny::Outcome.Busy);
        source::run_claimed(a, 0);
    }
    source::run(a, -1); assert(qput(state(a)));
    assert(a.status == run.status && a.fault == fault && a.steps == run.steps);
    let stable = a.steps; source::run(a, -1); assert(a.steps == stable);
}
// Side b of the same cut, one instruction per slice, against side a's states.
thread_fn check_steps(bytes: u8[], cut: i64) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, bytes);
    let first = qget<State>();
    var h2 = source::Heap { code: code }; assert(source::initialize(h2));
    // Two differential executions of one logical session image.
    // Values carry no session, so only the heap's arena label differs.
    h2.identity = first.identity;
    let e2 = source::closure(h2, 0, []);
    var b = source::Job { heap: h2 };
    assert(source::start(b, e2, []));
    let started = state(b); same_state(first, started);
    for i in cut { source::run(b, 1); }
    let cut_a = qget<State>(); let cut_b = state(b); same_state(cut_a, cut_b);
    source::run(b, -1); let done_a = qget<State>(); let done_b = state(b); same_state(done_a, done_b);
}
fn check(bytes: const u8[:], expected: const u8[:], fault: source::Fault = source::Fault.None) {
    heap_program::join(thread_spawn(check_run, str(bytes), str(expected), fault)); let run = qget<Run>();
    for cut in (run.steps + 1) {
        heap_program::join(thread_spawn(check_cut, str(bytes), cut, run, fault));
        heap_program::join(thread_spawn(check_steps, str(bytes), cut));
    }
}
// Limit points (light-cljs-801). The dispatch loop runs a frame in place and
// takes the exact limit checks only when its fuel is spent; one instruction a
// slice spends it every instruction, which is the per-step check. For every
// instruction limit, work limit above the started job, and stack limit up to
// what the run needs, one uncut run and one run in slices of one instruction
// must stop at the same instruction with the same fault and counts, and
// leave the same state. kind 0 limits.steps, 1 limits.work, 2 limits.stack.
struct Need { steps: i64, work: i64, stack: i64 }
thread_fn limit_need(bytes: u8[]) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let entry = source::closure(heap, 0, []);
    var job = source::Job { heap: heap }; assert(source::start(job, entry, []));
    let work = heap.work;
    // The deepest the locals and operands go, as the stack limit counts them.
    var stack: i64 = 0;
    while source::run(job, 1) == tiny::Outcome.Yielded { stack = max(stack, max(job.locals.len, job.operands.len)); }
    assert(job.status == tiny::Status.Complete && job.fault == source::Fault.None);
    assert(qput(Need { steps: job.steps, work: heap.work - work, stack: stack }));
}
thread_fn limit_side(bytes: u8[], kind: i64, limit: i64, sliced: bool, identity: i64) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    if identity != 0 { heap.identity = identity; }
    let entry = source::closure(heap, 0, []);
    var job = source::Job { heap: heap }; assert(source::start(job, entry, []));
    if kind == 0 { heap.limits.steps = limit; } else if kind == 1 { heap.limits.work = heap.work + limit; } else { heap.limits.stack = limit; }
    if sliced { while source::run(job, 1) == tiny::Outcome.Yielded {} } else { source::run(job, -1); }
    // Neither count overruns its limit, not even by instructions run in place.
    assert(job.steps <= heap.limits.steps && heap.work <= heap.limits.work);
    assert(qput(state(job)));
}
fn limit_points(bytes: const u8[:]) {
    heap_program::join(thread_spawn(limit_need, str(bytes))); let need = qget<Need>();
    for kind in 3 {
        let count = if kind == 0 { need.steps } else if kind == 1 { need.work } else { need.stack };
        for limit in (count + 1) {
            heap_program::join(thread_spawn(limit_side, str(bytes), kind, limit, false, 0)); let whole = qget<State>();
            heap_program::join(thread_spawn(limit_side, str(bytes), kind, limit, true, whole.identity)); let sliced = qget<State>();
            same_state(whole, sliced);
            // A work limit a helper's own charge meets fails with that
            // helper's fault (key_is, for one, answers false and its caller
            // reports UnsupportedOperand), on both sides alike.
            let fault = if kind == 0 { source::Fault.InstructionLimit } else { source::Fault.StackLimit };
            if limit < count { assert(whole.status == tiny::Status.Failed && (kind == 1 || whole.fault == fault)); }
            if kind == 0 { assert(whole.steps == min(limit, need.steps)); }
            if limit == count { assert(whole.status == tiny::Status.Complete); }
        }
    }
}
fn limits(bytes: const u8[:]) {
    for kind in 3 { heap_program::join(thread_spawn(initialization_limit, str(bytes), kind)); }
    for kind in 6 { heap_program::join(thread_spawn(execution_limit, str(bytes), kind)); }
    heap_program::join(thread_spawn(stale_owner, str(bytes))); let object = qget<Stale>();
    heap_program::join(thread_spawn(stale_other, str(bytes), object.value));
}
thread_fn initialization_limit(bytes: u8[], kind: i64) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, bytes);
    // The globalThis own property participates in the unit and work budgets.
    // Initialization imports each code string's units, and the property is
    // the global's first inline slot, so it takes no property row (light-cljs-800).
    {
        var text: i64 = 0; for s in code.strings { text += s.len; }
        var limit = source::Limits {};
        if kind == 0 { limit.properties = 0; }
        if kind == 1 { limit.units = text; }
        if kind == 2 { limit.work = text + code.constants.len + 1; }
        var heap = source::Heap { code: code, limits: limit };
        if kind == 0 { assert(source::initialize(heap) && heap.properties.len == 0 && source::node(heap, heap.global.id()).key_count == 1); return; }
        assert(!source::initialize(heap));
        assert(heap.fault == (if kind == 2 { source::Fault.WorkLimit } else { source::Fault.MemoryLimit }));
    }
}
thread_fn execution_limit(bytes: u8[], kind: i64) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, bytes);
    {
        var limit = source::Limits {};
        if kind == 0 { limit.steps = 1; }
        if kind == 1 { limit.frames = 1; }
        if kind == 2 { limit.stack = 1; }
        if kind == 3 { limit.nodes = 2; }
        if kind == 4 { limit.values = 0; }
        var heap = source::Heap { code: code, limits: limit };
        if !source::initialize(heap) { assert(heap.fault == source::Fault.MemoryLimit); return; }
        if kind == 5 { heap.limits.work = heap.work + 2; }
        let entry = source::closure(heap, 0, []);
        var job = source::Job { heap: heap };
        source::start(job, entry, []); source::run(job, -1);
        assert(job.status == tiny::Status.Failed);
        assert(job.fault == (if kind == 0 { source::Fault.InstructionLimit } else if kind == 1 || kind == 2 { source::Fault.StackLimit } else if kind == 5 { source::Fault.WorkLimit } else { source::Fault.MemoryLimit }));
    }
}
// A handle past a heap's rows is stale there. Values carry no session
// (light-cljs-785): an in-range handle from another heap is not detectable,
// because each thread program owns exactly one heap (light-cljs-794).
struct Stale { value: source::Value }
thread_fn stale_owner(bytes: u8[]) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let object = source::allocate(heap, source::Node { kind: 0 });
    for i in 200 { source::allocate(heap, source::Node { kind: 0 }); }
    assert(source::valid(heap, object));
    assert(qput(Stale { value: source::object(heap.nodes.len - 1) }));
    heap.fault = source::Fault.None;
    assert(!source::valid(heap, source::object(heap.nodes.len)));
}
thread_fn stale_other(bytes: u8[], object: source::Value) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, bytes);
    var other = source::Heap { code: code }; assert(source::initialize(other));
    assert(!source::valid(other, object) && other.fault == source::Fault.StaleID);
}
fn malformed(bytes: const u8[:]) {
    var image = program::Image {}; assert(program::install(image, bytes) == program::Error.None);
    let l = program::layout(bytes);
    var bad: u8[>..] = []; bad.append(bytes);
    // Replace the entry instruction with a same-width consumer at depth zero.
    let first = program::word(bytes, l.functions); let at = l.instructions + first * 20;
    let width = program::word(bytes, at) >> 16;
    let descriptor: i64 = if width == 0 { 5 } else if width == 2 { 131588 } else { 262926 };
    for i in 4 { bad[at + i] = ((descriptor >> (i * 8)) & 255) as u8; bad[at + 4 + i] = 0; }
    assert(program::install(image, bad) == program::Error.None);
    var code = source::Code {}; assert(!source::load(code, image));
    assert(code.error == tiny::LoadError.Underflow);
}
thread_fn factory(bytes: u8[]) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let entry = source::closure(heap, 0, []);
    var job = source::Job { heap: heap }; assert(source::start(job, entry, []));
    assert(source::run(job, -1) == tiny::Outcome.Complete && job.result.tag() == 5);
    assert(source::node(heap, job.result.id()).kind == 2);
}
// Row helpers name their table (light-cljs-800): in a contained worker, a
// row that another table holds but that is past this table fails by this
// table's name. One case a helper family.
struct WrongRow { family: i64 }
thread_fn wrong_row(probe: WrongRow) {
    var code = source::Code {}; decode(code, embed_file("../corpus/serial/calls.gsp"));
    var heap = source::Heap { code: code }; assert(source::initialize(heap)); source::boot_runtime(heap);
    let f = code.functions[0]; let big = heap.latin1.len;
    match probe.family {
        0 => { assert(heap.nodes.len < big); source::node(heap, heap.nodes.len); },
        1 => { assert(heap.nodes.len < big); source::node_mut(heap, heap.nodes.len).kind = 0; },
        2 => { assert(heap.values.len < heap.nodes.len); source::slot(heap, heap.values.len); },
        3 => { source::run(heap, 0, heap.values.len + 1); },
        4 => { assert(heap.properties.len < big); source::property_row(heap, heap.properties.len); },
        5 => { assert(heap.strings.len < big); source::header(heap, heap.strings.len); },
        6 => { assert(heap.constants.len < big); source::constant(heap, f, heap.constants.len - f.constants); },
        7 => { assert(heap.source_ranges.len < heap.nodes.len); source::source_range(heap, heap.source_ranges.len); },
        // A header whose run is past its unit table (only a corrupted row can say so).
        8 => { heap.strings.push(source::Str { start: big as u32, len: 1, wide: true }); source::utf16(heap, source::Value.String { s: (heap.strings.len - 1) as u32 }); },
        _ => { heap.strings.push(source::Str { start: heap.latin1.len as u32, len: 1, wide: false }); source::unit(heap, source::Value.String { s: (heap.strings.len - 1) as u32 }, 0); },
    }
}
// Strings are Latin-1 (one byte a unit) when every unit fits, else UTF-16,
// canonically, through every operation that makes one (light-cljs-800).
thread_fn latin1_strings() {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, embed_file("../corpus/serial/calls.gsp"));
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    for s in heap.strings { assert(!s.wide); }
    let units = heap.units.len; let latin1 = heap.latin1.len;
    let a = source::string(heap, [99 as u16, 97, 102, 233]);
    assert(!source::header(heap, a.sid()).wide && heap.latin1.len == latin1 + 4 && heap.units.len == units);
    let w = source::string(heap, [0x263A as u16, 97]);
    assert(source::header(heap, w.sid()).wide && heap.units.len == units + 2 && heap.latin1.len == latin1 + 4);
    let b = source::string(heap, [99 as u16, 97, 102, 233]);
    assert(source::same_text(heap, a, b) && source::equal(heap, a, b) && !source::same_text(heap, a, w) && !source::equal(heap, a, w));
    let ab = source::binary_value(heap, 73, a, b); assert(!source::header(heap, ab.sid()).wide && source::str_len(heap, ab) == 8);
    let aw = source::binary_value(heap, 73, a, w);
    assert(source::header(heap, aw.sid()).wide && source::utf16(heap, aw) == [99 as u16, 97, 102, 233, 0x263A, 97]);
    assert(source::unit(heap, a, 3) == 233 && source::unit(heap, w, 0) == 0x263A);
    assert(source::binary_value(heap, 74, a, w).flag() && !source::binary_value(heap, 75, a, w).flag());
    // One unit of a wide string that fits Latin-1 is a Latin-1 string.
    let one = source::form_nth(heap, w, 1); assert(!source::header(heap, one.sid()).wide && source::unit(heap, one, 0) == 97);
    var out: u8[>..] = []; source::canonical(heap, aw, out); assert(out == "utf16:[99,97,102,233,9786,97]");
    let k = source::key(heap, source::number(42)); assert(!source::header(heap, k.sid()).wide && source::index(heap, k) == 42 && source::key_is(heap, k, "42"));
    print("serial source VM: Latin-1 and UTF-16 strings stay canonical through make, concatenate, compare, index and print");
}
// Small objects keep their first two properties in the header's inline
// slots; the third spills into an overflow run (light-cljs-800).
thread_fn inline_slots() {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, embed_file("../corpus/serial/calls.gsp"));
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let o = source::allocate(heap, source::Node { kind: 0 }); let rows = heap.properties.len;
    let a = source::text(heap, "a"); let b = source::text(heap, "b"); let c = source::text(heap, "c");
    assert(source::put(heap, o, a, source::number(1)) && source::put(heap, o, b, source::number(2)));
    assert(heap.properties.len == rows && source::node(heap, o.id()).key_count == 2 && source::node(heap, o.id()).key_capacity == 0);
    // Overwriting an inline record allocates nothing.
    assert(source::put(heap, o, a, source::number(10)) && heap.properties.len == rows);
    assert(source::put(heap, o, c, source::number(3)));
    let n = source::node(heap, o.id());
    assert(n.key_count == 3 && n.key_capacity == 4 && heap.properties.len == rows + 4 && source::overflow(n) == 1);
    for k, i in [a, b, c] { let v, found = source::own_property(heap, o, k); assert(found && v.num() == [10.0, 2.0, 3.0][i]); }
    let none, missing = source::own_property(heap, o, source::text(heap, "d")); assert(!missing && none.tag() == 0);
    // A sparse write turns an array into a dictionary array, its first two
    // records inline; cutting its length clears the cut-off inline slot.
    let array = source::allocate(heap, source::Node { kind: 1 }); let before = heap.properties.len;
    assert(source::set_element(heap, array, 0, source::number(5)) && source::set_element(heap, array, 5000, source::number(6)));
    let d = source::node(heap, array.id()); assert(d.initialized && d.key_count == 2 && d.key_capacity == 0 && heap.properties.len == before);
    assert(source::put(heap, array, source::text(heap, "length"), source::number(1)));
    let cut = source::node(heap, array.id()); assert(cut.key_count == 1 && cut.length == 1 && cut.slots[1].key.tag() == 0);
    let kept, present = source::own_element(heap, array, 0); assert(present && kept.num() == 5);
    print("serial source VM: two inline property slots, then an overflow run; dictionary arrays share them");
}
fn wrong_rows() {
    let tables = ["table nodes", "table nodes", "table values", "table values", "table properties", "table strings", "table constants", "table source_ranges", "table units", "table latin1"];
    for name, family in tables {
        var why: u8[>..] = [];
        assert(!thread_wait(thread_spawn_contained(wrong_row, WrongRow { family: family }), why) && find(why, name) >= 0);
    }
    print("serial source VM: every row helper family refuses a wrong-table row by its table name");
}
fn main() {
    array_lengths(); latin1_state_check();
    wrong_rows();
    heap_program::join(thread_spawn(latin1_strings)); heap_program::join(thread_spawn(inline_slots));
    check(embed_file("../corpus/synthetic/arithmetic.gsp"), "number:27.5");
    check(embed_file("../corpus/synthetic/control_flow.gsp"), "number:7307.0");
    check(embed_file("../corpus/synthetic/wide.gsp"), "number:299.0");
    check(embed_file("../corpus/synthetic/unicode.gsp"), "utf16:[55357,56832,55296]");
    check(embed_file("../corpus/serial/calls.gsp"), "boolean:true");
    check(embed_file("../corpus/serial/arguments.gsp"), "boolean:true");
    argument_limits(embed_file("../corpus/serial/arguments.gsp"));
    check(embed_file("../corpus/serial/captures.gsp"), "boolean:true");
    check(embed_file("../corpus/serial/mutations.gsp"), "boolean:true");
    check(embed_file("../corpus/serial/control.gsp"), "boolean:true");
    check(embed_file("../corpus/serial/numbers.gsp"), "boolean:true");
    check(embed_file("../corpus/serial/strings.gsp"), "boolean:true");
    check(embed_file("../corpus/serial/global-undefined.gsp"), "undefined");
    check(embed_file("../corpus/serial/global-this.gsp"), "boolean:true");
    check(embed_file("../corpus/serial/global-missing.gsp"), "", source::Fault.ReferenceError);
    check(embed_file("../corpus/serial/negative-zero.gsp"), "number:-0");
    check(embed_file("../corpus/serial/nan.gsp"), "number:NaN");
    check(embed_file("../corpus/serial/tdz.gsp"), "", source::Fault.ReferenceError);
    check(embed_file("../corpus/serial/type-error.gsp"), "", source::Fault.TypeError);
    check(embed_file("../corpus/serial/not-callable.gsp"), "", source::Fault.TypeError);
    check(embed_file("../corpus/serial/unsupported-coercion.gsp"), "", source::Fault.UnsupportedOperand);
    check(embed_file("../corpus/serial/unsupported-prototype.gsp"), "", source::Fault.UnsupportedOperand);
    // Every opcode exports (light-cljs-783): `construct` on a dead path and in
    // a dead nested function never runs; reached, it fails by name even under
    // a `catch`. A class constructor cannot be called.
    check(embed_file("../corpus/serial/unsupported-dead.gsp"), "number:7.0");
    check(embed_file("../corpus/serial/unsupported-reached.gsp"), "Fault.Unsupported:construct", source::Fault.Unsupported);
    class_kinds(embed_file("../corpus/serial/calls.gsp"));
    check(embed_file("../corpus/serial/dormant-constants.gsp"), "number:7.0");
    check(embed_file("../corpus/serial/bigint-reached.gsp"), "Fault.Unsupported:const(BigInt)", source::Fault.Unsupported);
    bigint_constants(embed_file("../corpus/serial/dormant-constants.gsp"));
    fixed_cells(embed_file("../corpus/serial/mutations.gsp"));
    heap_program::join(thread_spawn(failed_job_isolation, str(embed_file("../corpus/serial/calls.gsp"))));
    allocation_limits(embed_file("../corpus/serial/mutations.gsp"), embed_file("../corpus/serial/strings.gsp"));
    flow_errors(embed_file("../corpus/serial/control.gsp"));
    heap_program::join(thread_spawn(runtime_boundary, str(embed_file("../corpus/branches-helper-0.pants.gsp"))));
    limits(embed_file("../corpus/serial/captures.gsp"));
    for image in [embed_file("../corpus/serial/calls.gsp"), embed_file("../corpus/serial/captures.gsp"), embed_file("../corpus/serial/control.gsp"), embed_file("../corpus/serial/arguments.gsp"), embed_file("../corpus/serial/mutations.gsp")] { limit_points(image); }
    heap_program::join(thread_spawn(factory, str(embed_file("../corpus/closure-captured.pants.gsp"))));
    malformed(embed_file("../corpus/serial/captures.gsp"));
    heap_program::join(thread_spawn(instruction_limit));
    // Image comparisons are bit-exact: NaN matches itself, -0 differs from +0.
    assert(source::same_bits(source::number(0.0 / 0.0), source::number(0.0 / 0.0)) && !source::same_bits(source::number(0.0), source::number(-0.0)));
    print("serial source VM: calls/captures/cells/properties/truth/UTF16/errors, exact slices and limits passed");
}
thread_fn instruction_limit() {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, embed_file("../corpus/serial/loop.gsp"));
    var heap = source::Heap { code: code, limits: source::Limits { steps: 100 } }; assert(source::initialize(heap));
    let entry = source::closure(heap, 0, []); var job = source::Job { heap: heap }; assert(source::start(job, entry, []));
    assert(source::run(job, -1) == tiny::Outcome.Failed && job.fault == source::Fault.InstructionLimit && job.steps == 100);
}
fn array_lengths() {
    heap_program::join(thread_spawn(array_length_bounds));
    // Exercise every work boundary through the length cut, including its
    // final charge, against exactly the same array contents.
    for cut in 40 { heap_program::join(thread_spawn(array_length_cut, cut)); }
    heap_program::join(thread_spawn(sparse_array));
    print("temporary arrays: shorter/zero/extension/deleted holes, numeric bounds, atomic work, retained-slot rejection and sparse records passed");
}
thread_fn array_length_bounds() {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, embed_file("../corpus/serial/mutations.gsp"));
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let array = source::allocate(heap, source::Node { kind: 1 }); let length = source::text(heap, "length");
    for i in 4 { assert(source::put(heap, array, source::key(heap, source::number(i)), source::number(i + 10))); }
    assert(source::put(heap, array, length, source::number(2)));
    assert(source::get(heap, array, source::key(heap, source::number(1))).num() == 11);
    assert(source::get(heap, array, source::key(heap, source::number(2))).tag() == 0);
    assert(source::put(heap, array, length, source::number(4)));
    assert(source::get(heap, array, source::key(heap, source::number(3))).tag() == 0);
    assert(source::put(heap, array, source::key(heap, source::number(2)), source::number(99)));
    assert(source::get(heap, array, source::key(heap, source::number(2))).num() == 99);
    assert(source::put(heap, array, length, source::number(0)) && source::node(heap, array.id()).key_count == 0);
    assert(source::put(heap, array, length, source::number(3)));
    for i in 3 { assert(source::get(heap, array, source::key(heap, source::number(i))).tag() == 0); }
    for n in [-1.0, 1.5, 0.0 / 0.0, 1.0 / 0.0, 4294967296.0] {
        assert(!source::put(heap, array, length, source::number(n)) && heap.fault == source::Fault.UnsupportedOperand);
        heap.fault = source::Fault.None; assert(source::node(heap, array.id()).length == 3);
    }
    assert(!source::put(heap, array, length, source::number(heap.limits.values + 1)) && heap.fault == source::Fault.MemoryLimit);
    heap.fault = source::Fault.None;
    assert(source::put(heap, array, source::key(heap, source::number(1)), source::number(7)));
    let before = copy(heap.properties); let slots = copy(heap.values); let records = source::node(heap, array.id()).key_count;
    heap.limits.work = heap.work + 8;
    assert(!source::put(heap, array, length, source::number(0)) && heap.fault == source::Fault.WorkLimit);
    assert(source::node(heap, array.id()).length == 3 && source::node(heap, array.id()).key_count == records && source::plain_rows(heap.properties) == source::plain_rows(before) && source::plain_rows(heap.values) == source::plain_rows(slots));
    heap.fault = source::Fault.None; heap.limits = source::Limits {};
    // Elements live in the array's own value run (light-cljs-785), so the
    // value-slot limit, not the property limit, bounds a growing array.
    let retained = heap.values.len; heap.limits.values = retained; let capacity = source::node(heap, array.id()).count;
    assert(source::put(heap, array, length, source::number(0)));
    assert(!source::put(heap, array, source::key(heap, source::number(capacity)), source::number(1)) && heap.fault == source::Fault.MemoryLimit);
    assert(heap.values.len == retained && source::node(heap, array.id()).length == 0 && heap.properties.len == before.len);
}
thread_fn array_length_cut(cut: i64) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, embed_file("../corpus/serial/mutations.gsp"));
    {
        var h = source::Heap { code: code }; assert(source::initialize(h));
        let a = source::allocate(h, source::Node { kind: 1 }); let k = source::text(h, "length");
        for i in 4 { assert(source::put(h, a, source::key(h, source::number(i)), source::number(i))); }
        let nodes = copy(h.nodes); let properties = copy(h.properties); let values = copy(h.values);
        h.limits.work = h.work + cut;
        let ok = source::put(h, a, k, source::number(2));
        if !ok { assert(h.fault == source::Fault.WorkLimit && source::plain_rows(h.nodes) == source::plain_rows(nodes) && source::plain_rows(h.properties) == source::plain_rows(properties) && source::plain_rows(h.values) == source::plain_rows(values)); }
        else { assert(h.nodes[a.id()].length == 2); }
    }
}
// Sparse writes far past the run use index-keyed records, so two
// elements near the value-slot limit still fit (Sol review, light-cljs-785).
thread_fn sparse_array() {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, embed_file("../corpus/serial/mutations.gsp"));
    {
        var h = source::Heap { code: code }; assert(source::initialize(h));
        let a = source::allocate(h, source::Node { kind: 1 }); let k = source::text(h, "length");
        assert(source::put(h, a, source::key(h, source::number(0)), source::number(5)));
        assert(source::put(h, a, source::key(h, source::number(524287)), source::number(1)));
        assert(source::put(h, a, source::key(h, source::number(524288)), source::number(2)));
        assert(h.nodes[a.id()].initialized && h.nodes[a.id()].count == 0 && h.nodes[a.id()].length == 524289 && h.values.len < 1024);
        assert(source::get(h, a, source::key(h, source::number(524287))).num() + source::get(h, a, source::key(h, source::number(524288))).num() == 3);
        assert(source::get(h, a, source::key(h, source::number(0))).num() == 5 && source::get(h, a, source::key(h, source::number(7))).tag() == 0);
        assert(source::put(h, a, k, source::number(524288)) && source::get(h, a, source::key(h, source::number(524288))).tag() == 0 && source::get(h, a, source::key(h, source::number(524287))).num() == 1);
    }
}
fn argument_limits(bytes: const u8[:]) {
    for op in [101, 179] { for limit in 4 { heap_program::join(thread_spawn(argument_limit, str(bytes), op, limit)); } }
}
thread_fn argument_limit(bytes: u8[], op: i64, limit: i64) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, bytes);
    {
        var function: i64 = -1;
        for f, i in code.functions {
            if f.captures != 0 { continue; }
            for instruction in code.ops[f.start..(f.start + f.count)] { if instruction.code == op { function = i; break; } }
            if function >= 0 { break; }
        }
        assert(function >= 0);
        {
            var heap = source::Heap { code: code }; assert(source::initialize(heap));
            let entry = source::closure(heap, function, []);
            if limit == 0 { heap.limits.stack = code.functions[function].locals + 2; }
            if limit == 1 { heap.limits.nodes = heap.nodes.len - heap.image.nodes; }
            if limit == 2 {
                if op == 101 { heap.limits.values = heap.values.len - heap.image.values + 2; }
                // A rest array keeps its elements in value slots (light-cljs-785).
                else { heap.limits.values = heap.values.len - heap.image.values; }
            }
            if limit == 3 { heap.limits.work = heap.work + code.functions[function].locals + 3; }
            var job = source::Job { heap: heap };
            source::start(job, entry, [source::number(1), source::number(2), source::number(3)]);
            assert(source::run(job, -1) == tiny::Outcome.Failed);
            assert(job.fault == (if limit == 0 { source::Fault.StackLimit } else if limit == 3 { source::Fault.WorkLimit } else { source::Fault.MemoryLimit }));
            assert(source::retire(job) && source::reset(job) && job.locals.len == 0);
        }
    }
}

fn fixed_cells(bytes: const u8[:]) {
    // Synthetic opcode variation of a source image, not a const-source claim.
    let l = program::layout(bytes); var variant: u8[>..] = []; variant.append(bytes);
    var changed = false;
    for i in program::word(bytes, 24) {
        let at = l.instructions + i * 20;
        if program::word(bytes, at) == 47 { variant[at] = 197; changed = true; }
    }
    assert(changed); check(variant, "", source::Fault.ConstAssignment);
}
thread_fn failed_job_isolation(bytes: u8[]) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let entry = source::closure(heap, 0, []);
    // A malformed foreign argument fails before entering any frame. A later
    // job still executes on the same initialized session and closure.
    var bad = source::Job { heap: heap };
    assert(!source::start(bad, entry, [source::object(heap.nodes.len + 1)]));
    assert(bad.fault == source::Fault.StaleID && !bad.claimed);
    var good = source::Job { heap: heap }; assert(source::start(good, entry, []));
    assert(source::run(good, -1) == tiny::Outcome.Complete);
    assert(source::equal(heap, good.result, source::boolean(true)));
}
fn allocation_limits(bytes: const u8[:], strings: const u8[:]) {
    for mode in 3 { heap_program::join(thread_spawn(allocation_limit, str(bytes), str(strings), mode)); }
}
thread_fn allocation_limit(bytes: u8[], strings: u8[], mode: i64) {
    defer { heap_program::finish(); }
    {
        // Array elements need no key strings (light-cljs-785), so the unit
        // limit is exercised by string indexing instead.
        var code = source::Code {}; decode(code, if mode == 1 { strings } else { bytes });
        var heap = source::Heap { code: code }; assert(source::initialize(heap));
        let entry = source::closure(heap, 0, []);
        if mode == 0 { heap.limits.properties = 0; }
        if mode == 1 { heap.limits.units = heap.units.len - heap.image.units; }
        if mode == 2 { heap.limits.values = 0; }
        var job = source::Job { heap: heap }; assert(source::start(job, entry, []));
        let outcome = source::run(job, -1); assert(outcome == tiny::Outcome.Failed && job.fault == source::Fault.MemoryLimit);
    }
}

fn set_word(bytes: u8[:], at: i64, n: i64) { for i in 4 { bytes[at + i] = ((n >> (i * 8)) & 255) as u8; } }
// GSP function kinds and constant tags (light-cljs-783): calling a class
// constructor throws TypeError; a Symbol constant validates and fails by name
// when pushed; unknown kinds and tags are refused before installation.
fn class_kinds(bytes: const u8[:]) {
    let l = program::layout(bytes); let functions = program::word(bytes, 12); assert(functions > 1);
    var image = program::Image {};
    for kind in [2, 3, 4, 5] {
        var patched: u8[>..] = []; patched.append(bytes);
        for id in 1..functions { set_word(patched[..], l.functions + id * 52 + 48, 1 | (kind << 8)); }
        if kind == 5 { assert(program::install(image, patched) == program::Error.Operation); continue; }
        check(patched, "", source::Fault.TypeError);
    }
    var number: i64 = -1;
    for i in program::word(bytes, 16) { if program::word(bytes, l.constants + i * 12) == 3 { number = i; } }
    assert(number >= 0);
    for tag, i in [6, 6, 5, 8] {
        var patched: u8[>..] = []; patched.append(bytes); let at = l.constants + number * 12;
        set_word(patched[..], at, tag); set_word(patched[..], at + 4, if i == 1 { 15 } else { 7 }); set_word(patched[..], at + 8, 0);
        if i > 0 { assert(program::install(image, patched) == program::Error.Operation); continue; }
        check(patched, "Fault.Unsupported:const(Symbol)", source::Fault.Unsupported);
    }
}
fn set_unit(bytes: u8[:], at: i64, n: i64) { bytes[at] = (n & 255) as u8; bytes[at + 1] = ((n >> 8) & 255) as u8; }
// Exported BigInt constants (sign unit, then 16-bit digits without a high
// zero digit) and selector operands validate before installation, including
// on dead paths; a failed install keeps the previous image.
fn bigint_constants(bytes: const u8[:]) {
    let l = program::layout(bytes); var zero: i64 = -1; var wide: i64 = -1; var bigints: i64 = 0;
    for i in program::word(bytes, 16) {
        let at = l.constants + i * 12;
        if program::word(bytes, at) != 7 { continue; }
        bigints += 1; let len = program::word(bytes, at + 8);
        if len == 1 { zero = at; } else if len > 2 { wide = at; }
    }
    assert(bigints >= 5 && zero >= 0 && wide >= 0);
    var image = program::Image {}; assert(program::install(image, bytes) == program::Error.None);
    let start = program::word(bytes, wide + 4); let len = program::word(bytes, wide + 8);
    for mode in 6 {
        var bad: u8[>..] = []; bad.append(bytes);
        if mode == 0 { set_unit(bad[..], l.units + 2 * start, 2); }                  // sign must be 0 or 1
        if mode == 1 { set_unit(bad[..], l.units + 2 * (start + len - 1), 0); }      // high zero digit
        if mode == 2 { set_unit(bad[..], l.units + 2 * program::word(bytes, zero + 4), 1); } // negative zero
        if mode == 3 { set_word(bad[..], wide + 8, 0); }                               // no sign unit
        if mode == 4 { set_word(bad[..], wide + 8, program::word(bytes, 28) + 1); }   // past the pool
        if mode == 5 {
            // An unknown ThrowError selector in the dead branch.
            var found = false;
            for i in program::word(bytes, 24) { let at = l.instructions + i * 20; if (program::word(bytes, at) & 255) == 42 { set_word(bad[..], at + 4, 5); found = true; } }
            assert(found);
        }
        let expected = if mode == 3 || mode == 4 { program::Error.Reference } else { program::Error.Operation };
        assert(program::install(image, bad) == expected);
        assert(image.valid && image.bytes[..] == bytes);
    }
}
fn flow_errors(bytes: const u8[:]) {
    let l = program::layout(bytes); let start = program::word(bytes, l.functions); let count = program::word(bytes, l.functions + 4);
    var original = source::Code {}; decode(original, bytes);
    var end: i64 = -1; for i in start..(start + count) { if original.ops[i].code == 15 && original.ops[i].depth >= 0 { end = l.instructions + i * 20; break; } }
    assert(end >= 0);
    var bad: u8[>..] = []; bad.append(bytes); for i in start..(start + count) { if original.ops[i].code == 15 { set_word(bad[..], l.instructions + i * 20, 5); } }
    var image = program::Image {}; assert(program::install(image, bad) == program::Error.None);
    var falling = source::Code {}; assert(!source::load(falling, image) && falling.error == tiny::LoadError.Fallthrough);
    bad.clear(); bad.append(bytes);
    var changed = false;
    for i in count {
        let at = l.instructions + (start + i) * 20;
        if (program::word(bytes, at) & 255) == 14 { assert(original.ops[start + i + 2].depth == 1); set_word(bad[..], at + 4, original.ops[start + i + 2].offset); changed = true; break; }
    }
    assert(changed && program::install(image, bad) == program::Error.None);
    var joined = source::Code {}; let accepted = source::load(joined, image); assert(!accepted && joined.error == tiny::LoadError.Join);
}
thread_fn runtime_boundary(bytes: u8[]) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let entry = source::closure(heap, 0, []); var factory_job = source::Job { heap: heap }; assert(source::start(factory_job, entry, []));
    assert(source::run(factory_job, -1) == tiny::Outcome.Complete);
    var job = source::Job { heap: heap }; assert(source::start(job, factory_job.result, [source::Value.Undefined]));
    assert(source::run(job, -1) == tiny::Outcome.Complete);
    assert(source::form_kind(heap, job.result) == 6 && source::form_count(heap, job.result) == 2);
    assert(source::form_nth(heap, job.result, 0).num() == 2 && source::form_nth(heap, job.result, 1).num() == 3);
}
