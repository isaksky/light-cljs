import std;
import .program;
import .source_vm;
import .vm;
import .heap_program;
import .snapshot;

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
    shapes:     u8[] = [], accessors: u8[] = [], keys: u8[] = [],
    // Continuations and promise jobs (light-cljs-790).
    conts: u8[] = [], queue: u8[] = [], draining: bool = false, entry: u8[] = [], handlers: u8[] = [],
    // The global lexical record (light-cljs-824).
    lexicals: u8[] = [],
}
fn state(a: source::Job&) -> State {
    return State {
        thrown:     a.thrown, status: a.status, steps: a.steps, claimed: a.claimed, fault: a.fault, fault_pc: a.fault_pc, fault_function: a.fault_function,
        registered: a.registered, live_jobs: a.heap.live_jobs, result: source::plain_rows([a.result]), frames: source::plain_rows(a.frames), locals: source::plain_rows(a.locals), operands: source::plain_rows(a.operands), writes: source::plain_rows(a.writes),
        nodes:      source::plain_rows(a.heap.nodes), values: source::plain_rows(a.heap.values), properties: source::plain_rows(a.heap.properties), units: copy(a.heap.units), latin1: copy(a.heap.latin1), identity: a.heap.identity, work: a.heap.work, heap_fault: a.heap.fault,
        shapes:     source::plain_rows(a.heap.shapes), accessors: source::plain_rows(a.heap.accessors), keys: str(bytes_of(a.heap.keys)),
        conts:      source::plain_rows(a.heap.conts), queue: source::plain_rows(a.queue[a.queue_head..]), draining: a.draining, entry: source::plain_rows([a.entry]), handlers: handler_rows(a.handlers),
        lexicals:   source::plain_rows([a.lexicals]),
    };
}
fn handler_rows(handlers: const source::Handler[:]) -> u8[] {
    var out: u8[>..] = [];
    for h in handlers { out.append(bytes_of([h.frame, h.pc, h.operands, h.locals])); out.push(source::handler_code(h.kind)); source::plain_bytes(h.value, out); }
    return copy(out);
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
    assert(a.shapes == b.shapes && a.accessors == b.accessors && a.keys == b.keys);
    assert(a.conts == b.conts && a.queue == b.queue && a.draining == b.draining && a.entry == b.entry && a.handlers == b.handlers && a.lexicals == b.lexicals);
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
// Global properties a script case runs over (light-cljs-824), defined
// before its job starts as Object.defineProperty would (the realm does not
// implement it yet): 1, `probe`, a configurable, enumerable accessor of the
// image's functions probeGet and probeSet; 2, `reserved` = 1, neither
// writable, enumerable nor configurable; 3, `reserved` = 1, writable and
// enumerable but not configurable. 4 to 6 are the getters the cases below
// name.
fn install(heap: source::Heap&, setup: i64) {
    if setup == 0 { return; }
    let global = heap.global.id();
    if setup == 1 {
        let k = source::key_named(heap, "probe"); let attrs = source::attr_enumerable() | source::attr_configurable();
        assert(source::define_own(heap, global, k, source::closure(heap, named_function(heap.code, "probeGet"), []), attrs, 1));
        assert(source::define_own(heap, global, k, source::closure(heap, named_function(heap.code, "probeSet"), []), attrs, 2));
    } else if setup == 4 {
        // `hooked`: an object whose Symbol.toPrimitive is an accessor with
        // hookGet as its getter (to-primitive-getter.js).
        let hooked = source::allocate(heap, source::Node { kind: node_kind::Object }); let attrs = source::attr_configurable();
        assert(source::define_own(heap, hooked.id(), source::Value.Symbol { s: well_known::ToPrimitive }, source::closure(heap, named_function(heap.code, "hookGet"), []), attrs, 1));
        assert(source::define_own(heap, global, source::key_named(heap, "hooked"), hooked, source::attr_writable() | source::attr_configurable()));
    } else if setup == 5 {
        // `probe` on Number.prototype and String.prototype: an accessor of
        // probeGet and probeSet (primitive-receiver.js).
        let k = source::key_named(heap, "probe"); let attrs = source::attr_configurable();
        for t in [0, 1] {
            let proto = source::wrapper_prototype_row(t);
            assert(source::define_own(heap, proto, k, source::closure(heap, named_function(heap.code, "probeGet"), []), attrs, 1));
            assert(source::define_own(heap, proto, k, source::closure(heap, named_function(heap.code, "probeSet"), []), attrs, 2));
        }
    } else if setup == 6 {
        // instanceof's getters (instanceof-hooks.js): `prototype` on
        // Function.prototype, an accessor with protoGet as its getter, and
        // `hooked`, an ordinary object whose Symbol.hasInstance is an
        // accessor with hookGet as its getter.
        let attrs = source::attr_configurable();
        assert(source::define_own(heap, source::function_prototype_row(), source::key_named(heap, "prototype"), source::closure(heap, named_function(heap.code, "protoGet"), []), attrs, 1));
        let hooked = source::allocate(heap, source::Node { kind: node_kind::Object });
        assert(source::define_own(heap, hooked.id(), source::Value.Symbol { s: well_known::HasInstance }, source::closure(heap, named_function(heap.code, "hookGet"), []), attrs, 1));
        assert(source::define_own(heap, global, source::key_named(heap, "hooked"), hooked, source::attr_writable() | source::attr_configurable()));
    } else if setup == 7 {
        // for-of's @@iterator getter (for-of.js, light-cljs-843): `hooked`,
        // an ordinary object whose Symbol.iterator is an accessor with
        // hookGet as its getter.
        let hooked = source::allocate(heap, source::Node { kind: node_kind::Object });
        assert(source::define_own(heap, hooked.id(), source::Value.Symbol { s: well_known::Iterator }, source::closure(heap, named_function(heap.code, "hookGet"), []), source::attr_configurable(), 1));
        assert(source::define_own(heap, global, source::key_named(heap, "hooked"), hooked, source::attr_writable() | source::attr_configurable()));
    } else {
        let k = source::key_named(heap, "reserved");
        assert(source::define_own(heap, global, k, source::number(1), if setup == 2 { 0 } else { source::attr_writable() | source::attr_enumerable() }));
    }
    assert(heap.fault == source::Fault.None);
}
// The function whose code is named `name`, which captures nothing.
fn named_function(code: const source::Code&, name: const u8[:]) -> i64 {
    for f, i in code.functions {
        if f.name_len != name.len || f.captures != 0 { continue; }
        var same = true;
        for c, j in name { if code.units[f.name_start + j] != c { same = false; } }
        if same { return i; }
    }
    abort("no function of that name");
    return -1;
}
thread_fn check_run(bytes: u8[], expected: u8[], fault: source::Fault, setup: i64) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap)); install(heap, setup);
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
        // A direct eval names itself, not its call opcode (light-cljs-823).
        assert(job.direct_eval == (expected[..] == "Fault.Unsupported:direct_eval"));
    } else if expected.len > 0 {
        // A named built-in step an UnsupportedOperand did not implement.
        var text: u8[>..] = []; source::describe_fault(text, code, job); assert(text[..] == expected);
    }
    assert(qput(Run { status: job.status, steps: job.steps }));
}
// Side a of one cut: publishes its state at the three comparison points.
thread_fn check_cut(bytes: u8[], cut: i64, run: Run, fault: source::Fault, setup: i64) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, bytes);
    var h1 = source::Heap { code: code }; assert(source::initialize(h1)); install(h1, setup);
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
thread_fn check_steps(bytes: u8[], cut: i64, setup: i64) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, bytes);
    let first = qget<State>();
    var h2 = source::Heap { code: code }; assert(source::initialize(h2)); install(h2, setup);
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
fn check(bytes: const u8[:], expected: const u8[:], fault: source::Fault = source::Fault.None, setup: i64 = 0) {
    heap_program::join(thread_spawn(check_run, str(bytes), str(expected), fault, setup)); let run = qget<Run>();
    for cut in (run.steps + 1) {
        heap_program::join(thread_spawn(check_cut, str(bytes), cut, run, fault, setup));
        heap_program::join(thread_spawn(check_steps, str(bytes), cut, setup));
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
    // The most values the locals and operands hold together, as the stack
    // limit counts them (light-cljs-786).
    var stack: i64 = 0;
    while source::run(job, 1) == tiny::Outcome.Yielded { stack = max(stack, job.locals.len + job.operands.len); }
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
    // The realm (light-cljs-788) participates in the unit and work budgets.
    // Initialization imports each code string's units; every realm object
    // stays in shape mode, so the realm takes no property row (light-cljs-800).
    {
        var text: i64 = 0; for s in code.strings { text += s.len; }
        var limit = source::Limits {};
        if kind == 0 { limit.properties = 0; }
        if kind == 1 { limit.units = text; }
        if kind == 2 { limit.work = text + code.constants.len + 1; }
        var heap = source::Heap { code: code, limits: limit };
        if kind == 0 { assert(source::initialize(heap) && heap.properties.len == 0 && !source::node(heap, heap.global.id()).initialized && source::node(heap, heap.global.id()).key_count > 1); return; }
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
    let object = source::allocate(heap, source::Node { kind: node_kind::Object });
    for i in 200 { source::allocate(heap, source::Node { kind: node_kind::Object }); }
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
    assert(source::run(job, -1) == tiny::Outcome.Complete && job.result.is_object());
    assert(source::node(heap, job.result.id()).kind == node_kind::Closure);
}
// Row helpers name their table (light-cljs-800): in a contained worker, a
// row that another table holds but that is past this table fails by this
// table's name. One case a helper family.
struct WrongRow { family: i64 }
thread_fn wrong_row(probe: WrongRow) {
    var code = source::Code {}; decode(code, embed_file("../corpus/serial/calls.gsp"));
    var heap = source::Heap { code: code }; assert(source::initialize(heap)); source::boot_runtime(heap);
    // Interned keys add string headers over existing units (light-cljs-787):
    // pad the Latin-1 table so its length stays past every other table's,
    // and overflow value runs: pad the nodes past values and source ranges.
    while heap.nodes.len <= heap.values.len + heap.source_ranges.len { heap.nodes.push(source::Node { kind: node_kind::Object }); }
    while heap.latin1.len <= heap.strings.len + heap.nodes.len + heap.properties.len + heap.constants.len { heap.latin1.push(97); }
    let f = code.functions[0]; let big = heap.latin1.len;
    match probe.family {
        0 => { assert(heap.nodes.len < big); source::node(heap, heap.nodes.len); },
        1 => { assert(heap.nodes.len < big); source::node_mut(heap, heap.nodes.len).kind = node_kind::Object; },
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
// ToString of primitives and `+` with a string operand (light-cljs-826)
// take exactly their result's units from the unit limit; without the room
// they fail MemoryLimit and add no rows. Converted primitives are Latin-1;
// a wide string operand keeps the result wide.
thread_fn primitive_to_string_limits() {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, embed_file("../corpus/serial/calls.gsp"));
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let narrow = source::text(heap, "ab"); let wide = source::string(heap, [0x263A as u16]);
    var strings = heap.strings.len; var latin1 = heap.latin1.len; let units = heap.units.len;
    heap.limits.units = source::unit_rows(heap) + 2;
    assert(source::to_string(heap, source::number(1.5)).is_undefined() && heap.fault == source::Fault.MemoryLimit && heap.strings.len == strings && heap.latin1.len == latin1);
    heap.fault = source::Fault.None; heap.limits.units = source::unit_rows(heap) + 3;
    let s = source::to_string(heap, source::number(1.5));
    assert(heap.fault == source::Fault.None && !source::header(heap, s.sid()).wide && source::key_is(heap, s, "1.5") && heap.strings.len == strings + 1 && heap.latin1.len == latin1 + 3);
    strings = heap.strings.len; latin1 = heap.latin1.len;
    heap.limits.units = source::unit_rows(heap) + 5;
    assert(source::binary_value(heap, opcode::Plus, narrow, source::boolean(true)).is_undefined() && heap.fault == source::Fault.MemoryLimit && heap.strings.len == strings && heap.latin1.len == latin1);
    heap.fault = source::Fault.None; heap.limits.units = source::unit_rows(heap) + 6;
    let t = source::binary_value(heap, opcode::Plus, narrow, source::boolean(true));
    assert(heap.fault == source::Fault.None && !source::header(heap, t.sid()).wide && source::key_is(heap, t, "abtrue") && heap.latin1.len == latin1 + 6 && heap.units.len == units);
    strings = heap.strings.len; latin1 = heap.latin1.len;
    heap.limits.units = source::unit_rows(heap) + 4;
    assert(source::binary_value(heap, opcode::Plus, source::nil(), wide).is_undefined() && heap.fault == source::Fault.MemoryLimit && heap.strings.len == strings && heap.units.len == units);
    heap.fault = source::Fault.None; heap.limits.units = source::unit_rows(heap) + 5;
    let w = source::binary_value(heap, opcode::Plus, source::nil(), wide);
    assert(heap.fault == source::Fault.None && source::header(heap, w.sid()).wide && source::utf16(heap, w) == [110 as u16, 117, 108, 108, 0x263A] && heap.units.len == units + 5 && heap.latin1.len == latin1);
    // Without a string operand `+` adds; a symbol beside a string is a
    // TypeError that adds nothing.
    assert(source::binary_value(heap, opcode::Plus, source::boolean(true), source::nil()).num() == 1);
    strings = heap.strings.len;
    assert(source::binary_value(heap, opcode::Plus, narrow, source::Value.Symbol { s: 0 }).is_undefined() && heap.fault == source::Fault.TypeError && heap.strings.len == strings);
    heap.fault = source::Fault.None;
    print("serial source VM: primitive ToString and mixed + take exactly their units; Latin-1 unless a string side is wide");
}
// StringToNumber (light-cljs-827) charges one work unit a code unit of the
// string, Latin-1 or UTF-16 alike, and fails WorkLimit without the room;
// ToNumber of a symbol is a TypeError, and `==` with one converts nothing.
thread_fn string_to_number_budget() {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, embed_file("../corpus/serial/calls.gsp"));
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let narrow = source::text(heap, " 0x1f\n"); let wide = source::string(heap, [0x3000 as u16, 52, 50, 0xFEFF]);
    for s, i in [narrow, wide] {
        let n = source::str_len(heap, s); let expected = [31.0, 42.0][i];
        heap.limits.work = heap.work + n - 1;
        source::numeric(heap, s); assert(heap.fault == source::Fault.WorkLimit);
        heap.fault = source::Fault.None; heap.limits.work = heap.work + n;
        assert(source::numeric(heap, s) == expected && heap.fault == source::Fault.None && heap.work == heap.limits.work);
    }
    heap.limits.work = heap.work + 1000;
    let x = source::numeric(heap, source::string(heap, [49 as u16, 0x0661])); assert(x != x && heap.fault == source::Fault.None);
    let symbol = source::Value.Symbol { s: 0 };
    source::numeric(heap, symbol); assert(heap.fault == source::Fault.TypeError); heap.fault = source::Fault.None;
    assert(!source::binary_value(heap, opcode::LooseEq, symbol, source::number(1)).flag() && heap.fault == source::Fault.None);
    assert(source::binary_value(heap, opcode::LooseEq, narrow, source::number(31)).flag() && source::binary_value(heap, opcode::CompareGt, narrow, source::boolean(true)).flag());
    // The runtime's inc is `x + 1`: a string concatenates, not converts.
    let inc = source::native_value(heap, 1, [source::text(heap, "4")]); assert(heap.fault == source::Fault.None && source::key_is(heap, inc, "41"));
    assert(source::native_value(heap, 1, [source::boolean(true)]).num() == 2);
    print("serial source VM: StringToNumber charges its units, Latin-1 or UTF-16; a symbol's ToNumber is a TypeError");
}
// Small objects keep their first two property values in the header's
// inline slots; the third spills into an overflow run of values, and the keys
// live in shared shapes (light-cljs-800, light-cljs-787).
thread_fn inline_slots() {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, embed_file("../corpus/serial/calls.gsp"));
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let o = source::allocate(heap, source::Node { kind: node_kind::Object }); let rows = heap.properties.len; let slots = heap.values.len;
    let a = source::text(heap, "a"); let b = source::text(heap, "b"); let c = source::text(heap, "c");
    assert(source::put(heap, o, a, source::number(1)) && source::put(heap, o, b, source::number(2)));
    assert(heap.properties.len == rows && heap.values.len == slots && source::node(heap, o.id()).key_count == 2 && source::node(heap, o.id()).key_capacity == 0);
    // Overwriting an inline value allocates nothing.
    let shapes = heap.shapes.len;
    assert(source::put(heap, o, a, source::number(10)) && heap.properties.len == rows && heap.shapes.len == shapes);
    assert(source::put(heap, o, c, source::number(3)));
    let n = source::node(heap, o.id());
    assert(n.key_count == 3 && n.key_capacity == 4 && heap.properties.len == rows && heap.values.len == slots + 4 && source::overflow(n) == 1 && source::shape_row(heap, n.shape).count == 3);
    for k, i in [a, b, c] { let v, found = source::own_property(heap, o, k); assert(found && v.num() == [10.0, 2.0, 3.0][i]); }
    let none, missing = source::own_property(heap, o, source::text(heap, "d")); assert(!missing && none.is_undefined());
    // A sparse write turns an array into a dictionary array of records;
    // cutting its length clears the cut-off record.
    let array = source::allocate(heap, source::Node { kind: node_kind::Array }); let before = heap.properties.len;
    assert(source::set_element(heap, array, 0, source::number(5)) && source::set_element(heap, array, 5000, source::number(6)));
    let d = source::node(heap, array.id()); assert(d.initialized && d.key_count == 2 && d.key_capacity == 4 && heap.properties.len == before + 4);
    assert(source::put(heap, array, source::text(heap, "length"), source::number(1)));
    let cut = source::node(heap, array.id()); assert(cut.key_count == 1 && cut.length == 1 && source::dict_at(heap, array.id(), 1).key.is_undefined());
    let kept, present = source::own_element(heap, array, 0); assert(present && kept.num() == 5);
    print("serial source VM: two inline property slots, then an overflow run; dictionary arrays keep records");
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
    heap_program::join(thread_spawn(latin1_strings)); heap_program::join(thread_spawn(primitive_to_string_limits)); heap_program::join(thread_spawn(string_to_number_budget)); heap_program::join(thread_spawn(inline_slots));
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
    // +"3" converts by StringToNumber since light-cljs-827 (the Pants
    // engine's expectation in rust/src/frontend/goose.rs is the same).
    check(embed_file("../corpus/serial/unsupported-coercion.gsp"), "number:3.0");
    // StringToNumber charges work inside the dispatch loop's unary,
    // ToNumberPair and binary arms: at every work limit an uncut run and a
    // run in one-instruction slices stop alike and never overrun it.
    check(embed_file("../corpus/serial/string-to-number.gsp"), "boolean:true");
    limit_points(embed_file("../corpus/serial/string-to-number.gsp"));
    // ({}).toString() is the realm's Object.prototype.toString (light-cljs-788).
    check(embed_file("../corpus/serial/unsupported-prototype.gsp"), "utf16:[91,111,98,106,101,99,116,32,79,98,106,101,99,116,93]");
    // Every opcode exports (light-cljs-783): `new Date` on a dead path and in
    // a dead nested function never runs; a `using` declaration reached
    // (explicit resource management, light-cljs-772; after a construction,
    // light-cljs-831, and inside a for-of loop over a user iterable,
    // light-cljs-843, where `instanceof` would run, light-cljs-834) fails by
    // name even under a `catch`. A class constructor cannot be called.
    check(embed_file("../corpus/serial/unsupported-dead.gsp"), "number:7.0");
    check(embed_file("../corpus/serial/unsupported-reached.gsp"), "Fault.Unsupported:using_check", source::Fault.Unsupported);
    class_kinds(embed_file("../corpus/serial/calls.gsp"));
    check(embed_file("../corpus/serial/dormant-constants.gsp"), "number:7.0");
    check(embed_file("../corpus/serial/bigint-reached.gsp"), "Fault.Unsupported:const(BigInt)", source::Fault.Unsupported);
    bigint_constants(embed_file("../corpus/serial/dormant-constants.gsp"));
    // Direct eval (light-cljs-823): reached, it fails by name even under a
    // `catch`; replacing the global `eval` makes the marked call ordinary; an
    // indirect eval is an unimplemented step of %eval%. Spread arguments
    // from a generator are collected (light-cljs-843), then stop at their
    // unimplemented check_spread_arguments (spread calls, light-cljs-764)
    // before the marked call_spread runs, so the spread variants are checked
    // at decode (direct_eval_decode).
    check(embed_file("../corpus/serial/direct-eval-reached.gsp"), "Fault.Unsupported:direct_eval", source::Fault.Unsupported);
    check(embed_file("../corpus/serial/direct-eval-spread.gsp"), "Fault.Unsupported:check_spread_arguments", source::Fault.Unsupported);
    direct_eval_decode(embed_file("../corpus/serial/direct-eval-spread.gsp"));
    check(embed_file("../corpus/serial/direct-eval-replaced.gsp"), "number:2.0");
    check(embed_file("../corpus/serial/indirect-eval.gsp"), "Fault.UnsupportedOperand:eval:-indirect-eval", source::Fault.UnsupportedOperand);
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
    control_flow();
    continuations();
    duplicates();

    properties();
    scripts();
    primitive_methods();
    key_enumeration();
    named_properties();
    heap_program::join(thread_spawn(realm_model, str(embed_file("../corpus/serial/calls.gsp"))));
    for which in 4 { heap_program::join(thread_spawn(to_object_opcode, which)); }
    // Image comparisons are bit-exact: NaN matches itself, -0 differs from +0.
    assert(source::same_bits(source::number(0.0 / 0.0), source::number(0.0 / 0.0)) && !source::same_bits(source::number(0.0), source::number(-0.0)));
    print("serial source VM: calls/captures/cells/properties/truth/UTF16/errors, exact slices and limits passed");
}
// Control flow (light-cljs-786). Each runs in a thread program of its own.
fn control_flow() {
    // try/catch/finally with return, break, continue and throw through and
    // out of finally blocks, nested finally; and `+` on objects, whose
    // valueOf runs in a native ToPrimitive frame and whose exception unwinds
    // through it. Every budget cut matches the uncut run.
    check(embed_file("../corpus/serial/finally.gsp"), "boolean:true");
    check(embed_file("../corpus/serial/native-frame.gsp"), "boolean:true");
    // ToPrimitive in every operator family (light-cljs-828): arithmetic,
    // relational, `==`, unary and `++`, a template substitution, property
    // keys, Symbol.toPrimitive's hints and a getter-supplied method, each
    // conversion once at every cut.
    check(embed_file("../corpus/serial/to-primitive.gsp"), "boolean:true");
    // A Symbol.toPrimitive getter the test installs: its function, null,
    // non-callable and throwing results.
    check(embed_file("../corpus/serial/to-primitive-getter.gsp"), "boolean:true", source::Fault.None, 4);
    // instanceof (light-cljs-834): getters of Symbol.hasInstance and
    // `prototype` the test installs, user methods and their exceptions run
    // above the operator's HasInstance native frame at every cut.
    check(embed_file("../corpus/serial/instanceof-hooks.gsp"), "boolean:true", source::Fault.None, 6);
    // for-of (light-cljs-843): GetIterator's phases (a user @@iterator
    // method, a `next` getter and, with setup 7, an @@iterator getter the
    // test installs), `next`, the body and `return`, each cut resuming with
    // the same result; each limit met uncut or one instruction a slice stops
    // at the same point.
    check(embed_file("../corpus/serial/for-of.gsp"), "boolean:true");
    check(embed_file("../corpus/serial/for-of.gsp"), "boolean:true", source::Fault.None, 7);
    heap_program::join(thread_spawn(iterator_getter_ran, str(embed_file("../corpus/serial/for-of.gsp"))));
    limit_points(embed_file("../corpus/serial/for-of.gsp"));
    // The built-in iterators (light-cljs-844): `length` getters, valueOf and
    // element getters above %ArrayIteratorPrototype%.next's native frame,
    // keys and entries, a string's code points and a String object's
    // ToString above String.prototype[@@iterator]'s, at every cut and limit.
    check(embed_file("../corpus/serial/builtin-iterators.gsp"), "boolean:true");
    limit_points(embed_file("../corpus/serial/builtin-iterators.gsp"));
    for shape in 2 { for boundary in 4 { heap_program::join(thread_spawn(iterator_source, str(embed_file("../corpus/serial/builtin-iterator-receiver.gsp")), shape, boundary)); } }
    // for-in over proxies (light-cljs-845): handler getters, traps that
    // loop, a key list's `length` and element getters and a descriptor's
    // field getters in Proxy frames above a ForIn frame, at every cut and
    // limit; a proxy's [[Get]] fails by name.
    check(embed_file("../corpus/serial/proxy-for-in.gsp"), "boolean:true");
    limit_points(embed_file("../corpus/serial/proxy-for-in.gsp"));
    check(embed_file("../corpus/serial/proxy-get.gsp"), "Fault.UnsupportedOperand:Proxy:-[[Get]]", source::Fault.UnsupportedOperand);
    // map and forEach (light-cljs-789): their callbacks, getters and the
    // length's valueOf run in the built-in's native frame.
    check(embed_file("../corpus/serial/array-callbacks.gsp"), "boolean:true");
    // push, join and toString (light-cljs-842): a push onto an array-like
    // through its `length` getter, valueOf and setters, and joins whose
    // elements and separator run toString, at every cut.
    check(embed_file("../corpus/serial/array-push-join.gsp"), "boolean:true");
    limit_points(embed_file("../corpus/serial/array-push-join.gsp"));
    push_receivers(embed_file("../corpus/serial/array-push-receiver.gsp"));
    join_limits(embed_file("../corpus/serial/array-join-strings.gsp"));
    // [[Construct]] (light-cljs-831): ordinary functions with new.target and
    // the return override, through finally and a generator yielding inside
    // the constructor, the realm's constructors and a non-constructor's
    // TypeError, at every cut; a class constructor fails by name.
    check(embed_file("../corpus/serial/construct.gsp"), "boolean:true");
    construct_kinds(embed_file("../corpus/serial/construct.gsp"));
    // 9,999 frames of f above the entry frame fit the 10,000-frame limit, and
    // a deeper recursion is caught; one frame less fails it uncaught.
    let depth = embed_file("../corpus/serial/depth.gsp");
    heap_program::join(thread_spawn(depth_limit, str(depth), 10000, source::Fault.None));
    heap_program::join(thread_spawn(depth_limit, str(depth), 9999, source::Fault.StackLimit));
    for sliced in [false, true] {
        heap_program::join(thread_spawn(cancelled_job, str(embed_file("../corpus/serial/cancel.gsp")), sliced, 0));
        heap_program::join(thread_spawn(cancelled_job, str(embed_file("../corpus/serial/cancel-break.gsp")), sliced, 1));
        heap_program::join(thread_spawn(cancelled_job, str(embed_file("../corpus/serial/loop.gsp")), sliced, 2));
        heap_program::join(thread_spawn(cancelled_job, str(embed_file("../corpus/serial/cancel-fault.gsp")), sliced, 3));
        heap_program::join(thread_spawn(cancelled_job, str(embed_file("../corpus/serial/cancel.gsp")), sliced, 4));
        for mode in 3 { heap_program::join(thread_spawn(cancelled_for_of, str(embed_file("../corpus/serial/cancel-for-of.gsp")), sliced, mode)); }
    }
    heap_program::join(thread_spawn(pending_completions, str(embed_file("../corpus/serial/calls.gsp"))));
    print("serial source VM: finally/completions, native ToPrimitive frames, 10,000-frame depth and cancellation passed");
}
// Array.prototype.push onto a receiver in rows the job did not allocate
// (light-cljs-842, decision 3): a dense array, a dictionary array, an
// array-like object and an empty dense array, below the job's protected or
// owned rows or in a foreign range, with 0, 1 and 2 items. Each faults
// NativeBoundary in push's native frame and leaves every node, value and
// property row as it was; the same receivers the job may write take the push.
fn push_receivers(bytes: const u8[:]) {
    for receiver in 4 { for boundary in 4 { for n in 3 { heap_program::join(thread_spawn(push_receiver, str(bytes), receiver, boundary, n)); } } }
}
thread_fn push_receiver(bytes: u8[], receiver: i64, boundary: i64, n: i64) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let start = heap.nodes.len;
    let o = source::allocate(heap, source::Node { kind: if receiver == 2 { node_kind::Object } else { node_kind::Array } });
    if receiver <= 1 { assert(source::put(heap, o, source::text(heap, "0"), source::number(1)) && source::put(heap, o, source::text(heap, "1"), source::number(2))); }
    if receiver == 1 { assert(source::put(heap, o, source::text(heap, "9000"), source::number(3)) && source::node(heap, o.id()).initialized); }
    if receiver == 2 { assert(source::put(heap, o, source::text(heap, "length"), source::number(1))); }
    let end = heap.nodes.len;
    let length: i64 = if receiver == 0 { 2 } else if receiver == 1 { 9001 } else if receiver == 2 { 1 } else { 0 };
    let entry = source::closure(heap, 0, []);
    var job = source::Job { heap: heap };
    if boundary == 0 { job.protected_nodes = end; } else if boundary == 1 { job.owned_nodes = end; } else if boundary == 2 { job.foreign.push(start as u32); job.foreign.push(end as u32); }
    assert(source::start(job, entry, [o, source::number(n)]));
    let nodes = source::plain_rows(heap.nodes); let values = source::plain_rows(heap.values); let properties = source::plain_rows(heap.properties);
    let outcome = source::run(job, -1);
    if boundary == 3 {
        assert(outcome == tiny::Outcome.Complete && job.result.num() == length + n);
        let after = source::get(heap, o, source::text(heap, "length"));
        assert(heap.fault == source::Fault.None && after.num() == length + n);
        return;
    }
    // The refusal is push's (its native frame, function -1), not a read's.
    assert(outcome == tiny::Outcome.Failed && job.fault == source::Fault.NativeBoundary && job.fault_function == -1);
    assert(source::plain_rows(heap.nodes) == nodes && source::plain_rows(heap.values) == values && source::plain_rows(heap.properties) == properties);
}
// The built-in array iterators over a source the job did not allocate
// (builtin-iterator-receiver.js, light-cljs-844, decision 3): a dense array
// (shape 0) or an array-like (1) below the job's protected or owned rows,
// in a foreign range, or the job's own (boundary 3). A cursor only reads
// its source: values(), entries() and, for the array, for-of complete with
// the same result and leave every row the source had as it was.
thread_fn iterator_source(bytes: u8[], shape: i64, boundary: i64) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let start = heap.nodes.len;
    let o = source::allocate(heap, source::Node { kind: if shape == 0 { node_kind::Array } else { node_kind::Object } });
    assert(source::put(heap, o, source::text(heap, "0"), source::number(1)) && source::put(heap, o, source::text(heap, "1"), source::number(2)));
    if shape == 1 { assert(source::put(heap, o, source::text(heap, "length"), source::number(2))); }
    let end = heap.nodes.len;
    let entry = source::closure(heap, 0, []);
    var job = source::Job { heap: heap };
    if boundary == 0 { job.protected_nodes = end; } else if boundary == 1 { job.owned_nodes = end; } else if boundary == 2 { job.foreign.push(start as u32); job.foreign.push(end as u32); }
    assert(source::start(job, entry, [o]));
    let nodes = source::plain_rows(heap.nodes[..end]); let values = source::plain_rows(heap.values); let properties = source::plain_rows(heap.properties);
    assert(source::run(job, -1) == tiny::Outcome.Complete);
    assert(source::key_is(heap, job.result, if shape == 0 { "120112" } else { "1201" }));
    assert(source::plain_rows(heap.nodes[..end]) == nodes && source::plain_rows(heap.values)[..values.len] == values[..] && source::plain_rows(heap.properties)[..properties.len] == properties[..]);
}
// Array.prototype.join's pieces (light-cljs-842): long Latin-1 and UTF-16
// elements and separators, a null element. The uncut join gives the
// expected units in their canonical encoding; a work limit below the pieces'
// code units fails WorkLimit, and a unit limit below them MemoryLimit, both
// in join's native frame; exactly enough units completes.
fn join_limits(bytes: const u8[:]) {
    for wide in 2 { for mode in 4 { heap_program::join(thread_spawn(join_limit, str(bytes), wide, mode)); } }
}
thread_fn join_limit(bytes: u8[], wide: i64, mode: i64) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var narrow_units:    u8[>..]  = []; for i in 400 { narrow_units.push((97 + i % 26) as u8); }
    var wide_units:      u16[>..] = []; for i in 300 { wide_units.push(((if wide == 1 { 256 } else { 65 }) + i % 50) as u16); }
    var separator_units: u16[>..] = []; for i in 50 { separator_units.push(if wide == 1 && i == 0 { 8364 } else { 45 }); }
    let narrow = source::text(heap, narrow_units); let other = source::string(heap, wide_units); let separator = source::string(heap, separator_units);
    let a = source::allocate(heap, source::Node { kind: node_kind::Array });
    let elements: source::Value[5] = [narrow, other, narrow, source::Value.Null, narrow];
    for v, i in elements { assert(source::set_element(heap, a, i, v)); }
    // The joined units, as the join builds them.
    var expected: u16[>..] = [];
    for v, i in elements {
        if i > 0 { expected.append(separator_units); }
        if v.is_string() { source::append_utf16(heap, v, expected); }
    }
    let total = expected.len; assert(total == 3 * 400 + 300 + 4 * 50);
    let entry = source::closure(heap, 0, []);
    var job = source::Job { heap: heap };
    assert(source::start(job, entry, [a, separator]));
    if mode == 1 { heap.limits.work = heap.work + total - 1; }
    if mode == 2 { heap.limits.units = source::unit_rows(heap) + total - 1; }
    if mode == 3 { heap.limits.units = source::unit_rows(heap) + total; }
    let outcome = source::run(job, -1);
    if mode == 1 || mode == 2 {
        assert(outcome == tiny::Outcome.Failed && job.fault == (if mode == 1 { source::Fault.WorkLimit } else { source::Fault.MemoryLimit }) && job.fault_function == -1);
        return;
    }
    assert(outcome == tiny::Outcome.Complete && job.result.is_string());
    assert(source::utf16(heap, job.result)[..] == expected[..] && source::header(heap, job.result.sid()).wide == (wide == 1));
}
// Generators, async functions and promise jobs (light-cljs-790;
// continuations.goose). Every budget cut matches the uncut run, so
// suspension, resumption and the job queue are data the loop resumes at
// any instruction or promise job.
fn continuations() {
    // A generator yielding through finally blocks, return() and throw() on
    // suspended generators, an exception completing one, re-entry.
    check(embed_file("../corpus/serial/generators.gsp"), "boolean:true");
    // Async functions awaiting fulfilled values and a rejected promise, the
    // order of several queued reactions and an adoption; a wrong order is
    // an unhandled rejection, which fails the job.
    check(embed_file("../corpus/serial/async.gsp"), "boolean:true");
    // An async entry completes with its promise's value; a promise that
    // never settles fails the job by name.
    check(embed_file("../corpus/serial/async-entry.gsp"), "utf16:[120,121]");
    check(embed_file("../corpus/serial/unsettled.gsp"), "", source::Fault.Unsettled);
    // Adoption follows the visible `then`: without one, a returned promise
    // is the value.
    check(embed_file("../corpus/serial/promise-then.gsp"), "utf16:[115,97,109,101,55]");
    heap_program::join(thread_spawn(continuation_rows, str(embed_file("../corpus/serial/generator-loop.gsp"))));
    for handled in [false, true] { heap_program::join(thread_spawn(rejection_outcomes, str(embed_file("../corpus/serial/async.gsp")), handled)); }
    heap_program::join(thread_spawn(rejected_resume, str(embed_file("../corpus/serial/generator-limit.gsp"))));
    for mode in 2 { for sliced in [false, true] { heap_program::join(thread_spawn(cancelled_continuation, str(embed_file("../corpus/serial/cancel-suspended.gsp")), mode, sliced)); } }
    print("serial source VM: generators, async functions and promise jobs as continuation records passed");
}
// 2,000 yields and 2,000 awaits reuse their activations' rows: no value
// rows per step, no continuation row per yield and one (the awaited
// value's promise) per await.
thread_fn continuation_rows(bytes: u8[]) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let values = heap.values.len; let conts = heap.conts.len;
    var job = source::Job { heap: heap }; assert(source::start(job, source::closure(heap, 0, []), []));
    assert(source::run(job, -1) == tiny::Outcome.Complete && job.result.is_bool() && job.result.flag());
    assert(heap.values.len - values < 64 && heap.conts.len - conts < 2000 + 64 && heap.conts.len - conts >= 2000);
    assert(source::retire(job));
}
// A resumption the frame limit refuses leaves the generator suspended, at
// its start and at a yield (the Pants engine's
// generator_rejected_resume_preserves_suspension).
thread_fn rejected_resume(bytes: u8[]) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, bytes);
    var heap = source::Heap { code: code, limits: source::Limits { frames: 3 } }; assert(source::initialize(heap));
    var job = source::Job { heap: heap }; assert(source::start(job, source::closure(heap, 0, []), []));
    assert(source::run(job, -1) == tiny::Outcome.Complete && source::key_is(heap, job.result, "InternalError;Y7;InternalError;Y8;"));
    assert(source::retire(job));
}
// Cancellation (decision 4): the running activation's finally block runs,
// a yield or await in it ends that block, and the suspended generator's
// finally block never runs; mode 0 a resumed generator, mode 1 an async
// function a promise job resumed.
thread_fn cancelled_continuation(bytes: u8[], mode: i64, sliced: bool) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var job = source::Job { heap: heap }; assert(source::start(job, source::closure(heap, 0, []), [source::number(mode)]));
    assert(source::run(job, 2000) == tiny::Outcome.Yielded && job.draining == (mode == 1));
    let looped = job.writes.len; assert(looped > 0);
    assert(source::cancel(job));
    if sliced { while source::run(job, 1) == tiny::Outcome.Yielded {} } else { source::run(job, -1); }
    assert(job.status == tiny::Status.Failed && job.fault == source::Fault.Cancelled && !job.thrown);
    var expected: u8[>..] = []; for i in looped { expected.append("looped "); } expected.append("cleaned ");
    assert(written(heap, job)[..] == expected[..]);
    assert(source::retire(job));
}
// The job queue's ends, on hand-made promises in a job's heap: a rejection
// a reaction handles is no report (p adopted by q, q's reaction settling p
// again, a no-op), one nothing handles fails the job UnhandledRejection
// with its reason as the thrown value, once the entry's own jobs ran.
thread_fn rejection_outcomes(bytes: u8[], handled: bool) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, bytes);
    {
        var heap = source::Heap { code: code }; assert(source::initialize(heap));
        var job = source::Job { heap: heap }; assert(source::start(job, source::closure(heap, 0, []), []));
        let p = source::new_promise(heap); let q = source::new_promise(heap);
        if handled { source::add_reaction(job, p.id(), q.id(), false); source::add_reaction(job, q.id(), p.id(), false); }
        source::settle_promise(job, p.id(), source::text(heap, "r"), true);
        assert(heap.fault == source::Fault.None && job.rejections.len == (if handled { 0 } else { 1 }) && job.queue.len == (if handled { 1 } else { 0 }));
        let outcome = source::run(job, -1);
        if handled {
            assert(outcome == tiny::Outcome.Complete && job.result.is_bool() && job.result.flag());
            assert(source::promise_state(heap, q.id()).state == promise_status::Rejected && source::promise_state(heap, q.id()).handled && source::promise_state(heap, p.id()).state == promise_status::Rejected);
        } else {
            assert(outcome == tiny::Outcome.Failed && job.fault == source::Fault.UnhandledRejection && job.thrown && source::key_is(heap, job.result, "r"));
        }
        assert(source::retire(job));
    }
}
// The property model (light-cljs-787): accessors (a getter's frame, a
// setter's native frame, an exception through it), prototype chains, `in`,
// delete and a strict assignment without a setter, every budget cut
// matching the uncut run; then the model's Goose functions.
fn properties() {
    check(embed_file("../corpus/serial/accessors.gsp"), "boolean:true");
    check(embed_file("../corpus/serial/prototypes.gsp"), "boolean:true");
    check(embed_file("../corpus/serial/deletes.gsp"), "boolean:true");
    check(embed_file("../corpus/serial/getter-only.gsp"), "", source::Fault.TypeError);
    heap_program::join(thread_spawn(property_model));
    print("serial source VM: shapes, interned keys, attributes, accessors, dictionary mode, own-key order and prototype chains passed");
}
fn key(heap: source::Heap&, name: const u8[:]) -> source::Value { return source::key_named(heap, name); }
// Scripts (light-cljs-824): top-level declarations, global name resolution
// and the declaration checks, every budget cut matching the uncut run. The
// checks run over global properties the test installs (install): one script
// cannot make the oracles agree there, since the Pants engine reserves
// `undefined`, `NaN` and `Infinity` at compile time where QuickJS throws at
// run time.
// Reads and calls on primitive bases make no wrapper (light-cljs-833,
// decision 2): primitive-methods.js run for `rounds` rounds of number,
// string and boolean method calls; publishes its result and the node rows
// the job added.
struct Rounds { result: f64, rows: i64 }
thread_fn primitive_rounds(bytes: u8[], rounds: i64) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let entry = source::closure(heap, 0, []);
    let nodes = heap.nodes.len;
    var job = source::Job { heap: heap }; assert(source::start(job, entry, [source::number(rounds)]));
    assert(source::run(job, -1) == tiny::Outcome.Complete && job.fault == source::Fault.None);
    assert(qput(Rounds { result: job.result.num(), rows: heap.nodes.len - nodes }));
}
fn primitive_methods() {
    let bytes = embed_file("../corpus/serial/primitive-methods.gsp");
    check(bytes, "number:400.0");
    heap_program::join(thread_spawn(primitive_rounds, str(bytes), 0)); let none = qget<Rounds>();
    heap_program::join(thread_spawn(primitive_rounds, str(bytes), 300)); let many = qget<Rounds>();
    assert(none.result == 0 && many.result == 600 && many.rows == none.rows);
    // A Symbol base's read or write fails by name, past a `catch`.
    for image in [embed_file("../corpus/serial/symbol-base-read.gsp"), embed_file("../corpus/serial/symbol-base-write.gsp")] {
        check(image, "Fault.UnsupportedOperand:property-access-on-a-Symbol:-%Symbol.prototype%", source::Fault.UnsupportedOperand);
    }
}
fn scripts() {
    check(embed_file("../corpus/serial/script-declarations.gsp"), "boolean:true");
    // Primitive receivers (light-cljs-833): realm-prototype accessors
    // installed for the script see the primitive itself, and a sloppy
    // script's functions box a primitive `this`.
    check(embed_file("../corpus/serial/primitive-receiver.gsp"), "boolean:true", source::Fault.None, 5);
    check(embed_file("../corpus/serial/sloppy-this.gsp"), "boolean:true");
    check(embed_file("../corpus/serial/script-setter.gsp"), "boolean:true", source::Fault.None, 1);
    let function = embed_file("../corpus/serial/script-reserved-function.gsp");
    check(function, "number:1.0");
    check(function, "Fault.SyntaxError:cannot-declare-global-'reserved'", source::Fault.SyntaxError, 2);
    check(function, "number:1.0", source::Fault.None, 3);
    let lexical = embed_file("../corpus/serial/script-reserved-let.gsp");
    check(lexical, "number:2.0");
    for setup in [2, 3] { check(lexical, "Fault.SyntaxError:cannot-declare-global-lexical-'reserved'", source::Fault.SyntaxError, setup); }
    check(embed_file("../corpus/serial/script-reserved-var.gsp"), "boolean:true", source::Fault.None, 2);
    // Static throws name the binding by its source text, non-ASCII too; a
    // generator and an async function keep the script's bindings across a
    // yield and an await.
    check(embed_file("../corpus/serial/script-throw-messages.gsp"), "boolean:true");
    check(embed_file("../corpus/serial/script-suspension.gsp"), "boolean:true");
    heap_program::join(thread_spawn(protected_declarations, str(embed_file("../corpus/serial/script-declarations.gsp"))));
    heap_program::join(thread_spawn(lexical_reset, str(embed_file("../corpus/serial/script-declarations.gsp"))));
    print("serial source VM: script declarations, global names, a global accessor and declaration checks passed");
}
// A compiler job's global object is protected (Job.protected_nodes): the
// script's first declaration instruction faults NativeBoundary and nothing
// is declared.
thread_fn protected_declarations(bytes: u8[]) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var job = source::Job { heap: heap }; job.protected_nodes = heap.nodes.len;
    assert(source::start(job, source::closure(heap, 0, []), []));
    let properties = heap.properties.len; let nodes = heap.nodes.len;
    assert(source::run(job, -1) == tiny::Outcome.Failed && job.fault == source::Fault.NativeBoundary && job.steps == 1);
    assert(!source::own_entry(heap, heap.global, key(heap, "count")).found && job.lexicals.is_undefined());
    assert(heap.properties.len == properties && heap.nodes.len == nodes);
    assert(source::retire(job));
}
// The record is the job's: a reused job's reset drops it.
thread_fn lexical_reset(bytes: u8[]) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var job = source::Job { heap: heap }; assert(source::start(job, source::closure(heap, 0, []), []));
    assert(source::run(job, -1) == tiny::Outcome.Complete && job.lexicals.is_object());
    assert(source::retire(job) && source::reset(job) && job.lexicals.is_undefined());
}
// The realm (light-cljs-788): its fixed rows and default prototypes, a
// built-in's and a runtime leaf's own `length` and `name` (configurable
// only), the well-known symbols as values, and a pending property that
// fails by name.
thread_fn realm_model(bytes: u8[]) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    assert(heap.nodes.len == source::realm_nodes() && source::realm_valid(heap.nodes, heap.accessors, heap.global, heap.call_method));
    // Symbol's pending names are its own, not its neighbour's (light-cljs-806).
    let object_fn = source::get(heap, source::object(source::global_row()), source::text(heap, "Object"));
    let symbol_fn = source::get(heap, source::object(source::global_row()), source::text(heap, "Symbol"));
    let descriptor = source::get(heap, object_fn, source::text(heap, "getOwnPropertyDescriptor"));
    assert(symbol_fn.id() == source::symbol_row() && descriptor.is_object() && source::node(heap, descriptor.id()).kind == node_kind::Builtin && heap.fault == source::Fault.None);
    assert(source::own_entry(heap, symbol_fn, key(heap, "for")).found && !source::own_entry(heap, descriptor, key(heap, "for")).found && !source::own_entry(heap, descriptor, key(heap, "prototype")).found);
    source::get(heap, symbol_fn, source::text(heap, "keyFor")); assert(heap.fault == source::Fault.UnsupportedOperand); heap.fault = source::Fault.None;
    let a = source::allocate(heap, source::Node { kind: node_kind::Array }); let o = source::allocate(heap, source::Node { kind: node_kind::Object });
    assert(source::prototype(heap, a.id()) == source::object(source::array_prototype_row()) && source::prototype(heap, o.id()) == source::object(source::object_prototype_row()));
    assert(source::prototype(heap, source::object_prototype_row()).is_null() && source::prototype(heap, heap.call_method.id()) == source::object(source::function_prototype_row()));
    let rt = source::boot_runtime(heap);
    let core = source::get(heap, rt, source::text(heap, "core")); let inc = source::get(heap, core, source::text(heap, "inc"));
    for f in [inc, heap.call_method] {
        let name = source::own_entry(heap, f, key(heap, "name")); let length = source::own_entry(heap, f, key(heap, "length"));
        assert(name.found && length.found && name.attrs == source::attr_configurable() && length.attrs == source::attr_configurable() && length.value.num() == 1);
    }
    assert(source::key_is(heap, source::get(heap, inc, source::text(heap, "name")), "inc") && source::key_is(heap, source::get(heap, heap.call_method, source::text(heap, "name")), "call"));
    let symbol = source::get(heap, heap.global, source::text(heap, "Symbol"));
    let iterator = source::get(heap, symbol, source::text(heap, "iterator"));
    assert(heap.fault == source::Fault.None && iterator.is_symbol() && source::valid(heap, iterator) && source::equal(heap, iterator, source::Value.Symbol { s: 0 }));
    var text: u8[>..] = []; source::canonical(heap, iterator, text); assert(text[..] == "symbol");
    source::get(heap, a, source::text(heap, "filter")); assert(heap.fault == source::Fault.UnsupportedOperand);
    heap.fault = source::Fault.None; assert(source::key_is(heap, heap.pending, "filter")); heap.pending = source::Value.Undefined;
    assert(source::has_property(heap, a, key(heap, "filter")) && !source::has_property(heap, o, key(heap, "filter")));
    // map and forEach are real built-ins (light-cljs-789), with length 1.
    let map = source::get(heap, a, source::text(heap, "map")); let for_each = source::get(heap, a, source::text(heap, "forEach"));
    assert(heap.fault == source::Fault.None && source::builtin_of(heap, map.id()) == source::Builtin.ArrayPrototypeMap && source::builtin_of(heap, for_each.id()) == source::Builtin.ArrayPrototypeForEach);
    assert(source::key_is(heap, source::get(heap, for_each, source::text(heap, "name")), "forEach") && source::get(heap, map, source::text(heap, "length")).num() == 1);
    // The primitive wrappers (light-cljs-832): the prototypes are wrapper
    // objects of +0, "" and false inheriting Object.prototype, each its
    // constructor's `prototype`.
    for name, t in ["Number", "String", "Boolean"] {
        let p = source::object(source::wrapper_prototype_row(t)); let c = source::get(heap, heap.global, source::text(heap, name));
        assert(source::node(heap, p.id()).kind == node_kind::Wrapper && source::wrapper_type(source::node(heap, p.id()).value) == t && source::prototype(heap, p.id()) == source::object(source::object_prototype_row()));
        assert(c.id() == source::wrapper_constructor_row(t) && source::get(heap, c, source::text(heap, "prototype")) == p && source::get(heap, p, source::text(heap, "constructor")) == c);
    }
    // A String object's `length` and indices are exotic own properties: read
    // from its string, listed first, refused to writes, definitions that
    // change them and deletes; its other keys are ordinary.
    let s = source::allocate(heap, source::Node { kind: node_kind::Wrapper, value: source::text(heap, "ab"), proto: source::object(source::wrapper_prototype_row(1)) });
    let length = source::own_entry(heap, s, key(heap, "length")); let second = source::own_entry(heap, s, key(heap, "1"));
    assert(length.found && length.attrs == 0 && length.value.num() == 2 && second.found && second.attrs == source::attr_enumerable() && source::key_is(heap, second.value, "b"));
    assert(!source::own_entry(heap, s, key(heap, "2")).found && source::has_property(heap, s, key(heap, "0")) && heap.fault == source::Fault.None);
    let setter, accepted = source::write(heap, s, source::text(heap, "0"), source::number(1)); assert(!accepted && setter.is_undefined() && heap.fault == source::Fault.None);
    assert(!source::delete_property(heap, s, key(heap, "length")) && !source::delete_property(heap, s, key(heap, "1")) && heap.fault == source::Fault.None);
    assert(!source::define_property(heap, s, key(heap, "0"), source::text(heap, "z"), source::attr_enumerable()) && source::define_property(heap, s, key(heap, "0"), source::text(heap, "a"), source::attr_enumerable()));
    assert(source::put(heap, s, source::text(heap, "x"), source::number(3)) && source::put(heap, s, source::text(heap, "7"), source::number(4)));
    var keys: source::Value[>..] = []; assert(source::own_keys(heap, s, keys) && keys.len == 5);
    for name, i in ["0", "1", "7", "length", "x"] { assert(source::key_is(heap, keys[i], name)); }
    assert(source::delete_property(heap, s, key(heap, "7")) && !source::own_entry(heap, s, key(heap, "7")).found && heap.fault == source::Fault.None);
    print("serial source VM: realm rows, prototypes, built-in and runtime leaf metadata, well-known symbols, pending names and primitive wrappers passed");
}
// The ToObject opcode (176, a `with` statement's object; light-cljs-832),
// in a synthetic function `x => ToObject(x)` (no compiled source reaches it
// without `with`'s other opcodes): a string becomes a new String object
// inheriting String.prototype, a number and a boolean their wrappers, an
// object stays itself, and null is a TypeError (catchable: fault_type).
thread_fn to_object_opcode(which: i64) {
    defer { heap_program::finish(); }
    var synthetic = source::Code {};
    synthetic.functions.push(source::Function { start: 0, count: 3, constants: 0, children: 0, params: 1, locals: 1, captures: 0, strict: true });
    for op, i in [opcode::GetLocal, opcode::ToObject, opcode::Return] { synthetic.ops.push(source::Op { code: op, arg: 0, offset: [0, 3, 4][i], start: 0, end: 0, depth: [0, 1, 1][i] }); }
    synthetic.valid = true;
    var code = source::Code {}; let f = source::append_code(code, synthetic);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let input = match which { 0 => source::text(heap, "ab"), 1 => source::number(5), 2 => source::object(source::global_row()), _ => source::Value.Null };
    var job = source::Job { heap: heap }; assert(source::start(job, source::closure(heap, f, []), [input]));
    let outcome = source::run(job, -1);
    if which == 3 {
        let message = "cannot convert undefined or null to object";
        assert(outcome == tiny::Outcome.Failed && job.fault == source::Fault.TypeError && job.detail.len == message.len);
        for c, i in message { assert(job.detail[i] == c); }
        return;
    }
    assert(outcome == tiny::Outcome.Complete && job.result.is_object());
    let n = source::node(heap, job.result.id());
    if which == 2 { assert(job.result == input); return; }
    assert(n.kind == node_kind::Wrapper && source::same_value(heap, n.value, input) && source::prototype(heap, job.result.id()) == source::object(source::wrapper_prototype_row(source::wrapper_type(input))));
    if which == 0 { assert(source::own_entry(heap, job.result, key(heap, "length")).value.num() == 2); }
}
thread_fn property_model() {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, embed_file("../corpus/serial/calls.gsp"));
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    // Objects built with the same keys in the same order share their shapes;
    // another order makes other shapes. Keys are interned: a new string of
    // the same text names the same key.
    let p = source::allocate(heap, source::Node { kind: node_kind::Object }); let q = source::allocate(heap, source::Node { kind: node_kind::Object }); let r = source::allocate(heap, source::Node { kind: node_kind::Object });
    assert(source::put(heap, p, source::text(heap, "x"), source::number(1)) && source::put(heap, p, source::text(heap, "y"), source::number(2)));
    let shapes = heap.shapes.len;
    assert(source::put(heap, q, source::text(heap, "x"), source::number(3)) && source::put(heap, q, source::text(heap, "y"), source::number(4)) && heap.shapes.len == shapes);
    assert(source::put(heap, r, source::text(heap, "y"), source::number(5)) && source::put(heap, r, source::text(heap, "x"), source::number(6)) && heap.shapes.len == shapes + 2);
    assert(source::node(heap, p.id()).shape == source::node(heap, q.id()).shape && source::node(heap, p.id()).shape != source::node(heap, r.id()).shape);
    let fresh = source::text(heap, "x"); assert(source::intern(heap, fresh) == key(heap, "x") && fresh != key(heap, "x"));
    assert(source::get(heap, q, source::text(heap, "y")).num() == 4 && source::get(heap, r, source::text(heap, "y")).num() == 5);
    // Attributes: a non-writable, non-configurable property refuses
    // assignment (a TypeError for put, as in strict code; no fault from
    // write, which sloppy code ignores), a changed value or attribute and
    // delete, and accepts a definition that changes nothing.
    let o = source::allocate(heap, source::Node { kind: node_kind::Object }); let x = key(heap, "x");
    assert(source::define_own(heap, o.id(), x, source::number(1), source::attr_enumerable()));
    assert(!source::put(heap, o, source::text(heap, "x"), source::number(2)) && heap.fault == source::Fault.TypeError); heap.fault = source::Fault.None;
    let setter, accepted = source::write(heap, o, source::text(heap, "x"), source::number(2)); assert(!accepted && setter.is_undefined() && heap.fault == source::Fault.None);
    assert(source::define_own(heap, o.id(), x, source::number(1), source::attr_enumerable()) && !source::node(heap, o.id()).initialized);
    assert(!source::define_own(heap, o.id(), x, source::number(2), source::attr_enumerable()) && !source::define_own(heap, o.id(), x, source::number(1), source::attrs_data()));
    assert(!source::delete_property(heap, o, x) && heap.fault == source::Fault.None && source::get(heap, o, x).num() == 1);
    // Changing a configurable property's attributes moves the object to
    // dictionary mode, keeping values and order.
    let y = key(heap, "y"); assert(source::define_own(heap, o.id(), y, source::number(7), source::attrs_data()));
    assert(source::define_own(heap, o.id(), y, source::number(8), source::attr_writable() | source::attr_configurable()) && source::node(heap, o.id()).initialized && source::node(heap, o.id()).shape == 0);
    assert(source::get(heap, o, x).num() == 1 && source::get(heap, o, y).num() == 8 && source::dict_at(heap, o.id(), 1).attrs == 5);
    // Accessors, own and inherited: lookup returns the getter and assign the
    // setter, for the dispatch loop to run with the original receiver; a
    // getter-only property refuses assignment; a Goose get of an accessor
    // fails UnsupportedOperand (no frame can run there).
    let getter = source::closure(heap, 0, []); let setter_fn = source::closure(heap, 0, []);
    let base = source::allocate(heap, source::Node { kind: node_kind::Object }); let a = key(heap, "a"); let g = key(heap, "g");
    assert(source::define_own(heap, base.id(), a, getter, source::attrs_accessor(), 1) && source::define_own(heap, base.id(), a, setter_fn, source::attrs_accessor(), 2));
    assert(source::define_own(heap, base.id(), g, getter, source::attrs_accessor(), 1) && heap.accessors.len == 4); // after the realm's pending pair
    let child = source::allocate(heap, source::Node { kind: node_kind::Object }); assert(source::set_prototype(heap, child.id(), base));
    for target in [base, child] {
        let v, found = source::lookup(heap, target, a); assert(v.is_undefined() && found == getter);
        let f, ok = source::assign(heap, target, a, source::number(1)); assert(ok && f == setter_fn);
        let none, refused = source::assign(heap, target, g, source::number(1)); assert(!refused && none.is_undefined() && heap.fault == source::Fault.None);
    }
    source::get(heap, child, a); assert(heap.fault == source::Fault.UnsupportedOperand); heap.fault = source::Fault.None;
    assert(!source::node(heap, child.id()).initialized && source::node(heap, child.id()).key_count == 0);
    // Deletion moves a shaped object to dictionary mode; later records move
    // down and new ones append, in creation order.
    let d = source::allocate(heap, source::Node { kind: node_kind::Object });
    for name in ["a", "b", "c"] { assert(source::put(heap, d, source::text(heap, name), source::number(1))); }
    assert(source::delete_property(heap, d, key(heap, "b")) && source::node(heap, d.id()).initialized && source::node(heap, d.id()).key_count == 2);
    assert(source::delete_property(heap, d, key(heap, "missing")) && source::put(heap, d, source::text(heap, "e"), source::number(2)));
    var order: source::Value[>..] = []; assert(source::own_keys(heap, d, order) && order.len == 3 && source::key_is(heap, order[0], "a") && source::key_is(heap, order[1], "c") && source::key_is(heap, order[2], "e"));
    // Own-key order: array indices ascending, then strings in creation
    // order, then symbols in creation order.
    let mixed = source::allocate(heap, source::Node { kind: node_kind::Object }); let iterator: source::Value = source::Value.Symbol { s: 0 };
    for name in ["b", "10", "a"] { assert(source::put(heap, mixed, source::text(heap, name), source::number(1))); }
    assert(source::define_own(heap, mixed.id(), iterator, source::number(1), source::attrs_data()) && source::define_own(heap, mixed.id(), source::Value.Symbol { s: 2 }, source::number(2), source::attrs_data()));
    for name in ["2", "4294967295", "c", "01"] { assert(source::put(heap, mixed, source::text(heap, name), source::number(1))); }
    var all: source::Value[>..] = []; assert(source::own_keys(heap, mixed, all) && all.len == 9);
    let expected = ["2", "10", "b", "a", "4294967295", "c", "01"];
    for name, i in expected { assert(source::key_is(heap, all[i], name)); }
    assert(source::symbol_index(all[7]) == 0 && source::symbol_index(all[8]) == 2);
    // Symbol keys through the public entry points: get, put, has, delete and
    // an inherited read.
    let heir = source::allocate(heap, source::Node { kind: node_kind::Object }); assert(source::set_prototype(heap, heir.id(), mixed));
    assert(source::get(heap, mixed, iterator).num() == 1 && source::put(heap, mixed, iterator, source::number(5)) && source::get(heap, heir, iterator).num() == 5);
    assert(source::has_property(heap, heir, iterator) && source::put(heap, heir, iterator, source::number(6)) && source::get(heap, mixed, iterator).num() == 5);
    assert(source::delete_property(heap, heir, iterator) && source::get(heap, heir, iterator).num() == 5 && heap.fault == source::Fault.None);
    // A non-configurable accessor accepts its own getter again and refuses
    // another function or attributes.
    let fixed = source::allocate(heap, source::Node { kind: node_kind::Object }); let pairs = heap.accessors.len;
    assert(source::define_own(heap, fixed.id(), g, getter, source::attr_enumerable(), 1) && heap.accessors.len == pairs + 1);
    assert(source::define_own(heap, fixed.id(), g, getter, source::attr_enumerable(), 1) && heap.accessors.len == pairs + 1);
    assert(!source::define_own(heap, fixed.id(), g, setter_fn, source::attr_enumerable(), 1) && !source::define_own(heap, fixed.id(), g, getter, source::attrs_accessor(), 1));
    assert(!source::define_own(heap, fixed.id(), g, setter_fn, source::attr_enumerable(), 2) && heap.fault == source::Fault.None);
    // A three-object chain: reads find each level's property and the nearest
    // shadowing one; a cycle is refused.
    let c1 = source::allocate(heap, source::Node { kind: node_kind::Object }); let c2 = source::allocate(heap, source::Node { kind: node_kind::Object }); let c3 = source::allocate(heap, source::Node { kind: node_kind::Object });
    assert(source::set_prototype(heap, c2.id(), c1) && source::set_prototype(heap, c3.id(), c2));
    assert(source::put(heap, c1, source::text(heap, "deep"), source::number(1)) && source::put(heap, c1, source::text(heap, "s"), source::number(1)) && source::put(heap, c2, source::text(heap, "s"), source::number(2)));
    assert(source::get(heap, c3, source::text(heap, "deep")).num() == 1 && source::get(heap, c3, source::text(heap, "s")).num() == 2 && source::has_property(heap, c3, key(heap, "deep")));
    assert(source::get(heap, c3, source::text(heap, "nothing")).is_undefined() && heap.fault == source::Fault.None);
    assert(!source::set_prototype(heap, c1.id(), c3) && source::node(heap, c1.id()).proto.is_hole());
    // The chain ends at the realm's Object.prototype (light-cljs-788): its
    // built-in toString is found, and a name it defines but this VM does not
    // implement (a pending property) fails by name; under a null prototype
    // both are simply absent.
    let found = source::get(heap, c3, source::text(heap, "toString")); assert(heap.fault == source::Fault.None && found.is_object() && source::node(heap, found.id()).kind == node_kind::Builtin);
    source::get(heap, c3, source::text(heap, "propertyIsEnumerable")); assert(heap.fault == source::Fault.UnsupportedOperand); heap.fault = source::Fault.None;
    assert(source::set_prototype(heap, c1.id(), source::nil()) && source::get(heap, c3, source::text(heap, "toString")).is_undefined() && source::get(heap, c3, source::text(heap, "propertyIsEnumerable")).is_undefined() && heap.fault == source::Fault.None);
    // Past shape_limit() keys an object turns to dictionary mode.
    let big = source::allocate(heap, source::Node { kind: node_kind::Object });
    for i in source::shape_limit() + 1 { assert(source::put(heap, big, source::text(heap, str("k", i)), source::number(i))); }
    assert(source::node(heap, big.id()).initialized && source::get(heap, big, source::text(heap, "k3")).num() == 3 && source::get(heap, big, source::text(heap, "k64")).num() == 64);
}
// Named properties of arrays and functions (light-cljs-805): arrays' names
// through sparse, length-cut and dictionary arrays, closures' properties and
// an ordinary function's lazy `prototype`, a function's `name`
// (light-cljs-806), ToPrimitive through getters, and the gaps that still
// fail by name (an array method, a generator's `prototype`),
// every budget cut matching the uncut run; then the Goose functions.
fn named_properties() {
    check(embed_file("../corpus/serial/array-names.gsp"), "boolean:true");
    check(embed_file("../corpus/serial/function-names.gsp"), "boolean:true");
    check(embed_file("../corpus/serial/getter-value-of.gsp"), "boolean:true");
    check(embed_file("../corpus/serial/function-name.gsp"), "utf16:[102]");
    check(embed_file("../corpus/serial/function-keys.gsp"), "boolean:true");
    check(embed_file("../corpus/serial/unsupported-array-method.gsp"), "", source::Fault.UnsupportedOperand);
    // A generator function's `prototype` (light-cljs-790; an explicit gap before).
    check(embed_file("../corpus/serial/unsupported-generator-prototype.gsp"), "boolean:true");
    heap_program::join(thread_spawn(named_model));
    print("serial source VM: arrays' and functions' named properties, lazy prototypes and ToPrimitive getters passed");
}
thread_fn named_model() {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, embed_file("../corpus/serial/function-names.gsp"));
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    // A dense array keeps names in a shape beside its run; listing gives
    // indices, `length`, then names. A sparse write moves it to dictionary
    // mode with its names after its index records, and a shorter length
    // keeps them.
    let a = source::allocate(heap, source::Node { kind: node_kind::Array });
    assert(source::put(heap, a, source::text(heap, "0"), source::number(0)) && source::put(heap, a, source::text(heap, "tag"), source::number(1)) && source::put(heap, a, source::text(heap, "more"), source::number(2)));
    let n = source::node(heap, a.id()); assert(!n.initialized && n.key_count == 2 && n.shape != 0 && n.length == 1);
    var keys: source::Value[>..] = []; assert(source::own_keys(heap, a, keys) && keys.len == 4);
    for name, i in ["0", "length", "tag", "more"] { assert(source::key_is(heap, keys[i], name)); }
    assert(source::put(heap, a, source::text(heap, "9000"), source::number(9)));
    let d = source::node(heap, a.id()); assert(d.initialized && d.shape == 0 && d.key_count == 4 && d.count == 0 && source::get(heap, a, source::text(heap, "more")).num() == 2);
    assert(source::put(heap, a, source::text(heap, "length"), source::number(1)) && source::node(heap, a.id()).key_count == 3 && source::get(heap, a, source::text(heap, "tag")).num() == 1);
    assert(source::get(heap, a, source::text(heap, "9000")).is_undefined() && source::delete_property(heap, a, key(heap, "tag")) && source::node(heap, a.id()).key_count == 2);
    // A delete of a dense array's name moves its elements to records too.
    let b = source::allocate(heap, source::Node { kind: node_kind::Array });
    assert(source::put(heap, b, source::text(heap, "0"), source::number(5)) && source::put(heap, b, source::text(heap, "x"), source::number(6)) && source::put(heap, b, source::text(heap, "y"), source::number(7)));
    assert(source::delete_property(heap, b, key(heap, "x")) && source::node(heap, b.id()).initialized && source::get(heap, b, source::text(heap, "0")).num() == 5 && source::get(heap, b, source::text(heap, "y")).num() == 7);
    // The pending %Array.prototype%: its names and symbol keys fail by name,
    // other names are absent; its @@iterator is `values` (light-cljs-844).
    source::get(heap, b, source::text(heap, "pop")); assert(heap.fault == source::Fault.UnsupportedOperand); heap.fault = source::Fault.None;
    source::get(heap, b, source::Value.Symbol { s: well_known::Unscopables }); assert(heap.fault == source::Fault.UnsupportedOperand); heap.fault = source::Fault.None;
    assert(source::get(heap, b, source::Value.Symbol { s: well_known::Iterator }) == source::get(heap, b, source::text(heap, "values")) && heap.fault == source::Fault.None);
    assert(source::get(heap, b, source::text(heap, "other")).is_undefined() && heap.fault == source::Fault.None);
    // A closure (light-cljs-806): `length` and `name` come from its code row
    // and are configurable only; while lazy, reads, `in` and key listing
    // answer them and an ordinary function's `prototype` without making
    // them, and a Goose caller's change fails by name. Once made, they come
    // first in creation order and `prototype` is non-configurable. An arrow
    // (constructor kind 0) has no `prototype`.
    var ordinary: i64 = -1; var arrow: i64 = -1;
    for f, i in heap.code.functions { if f.captures == 0 && f.kind == constructor::Ordinary && ordinary < 0 && i > 0 { ordinary = i; } if f.captures == 0 && f.kind == constructor::None && arrow < 0 && i > 0 { arrow = i; } }
    assert(ordinary > 0 && arrow > 0);
    let f = source::closure(heap, ordinary, []); let g = source::closure(heap, arrow, []);
    assert(source::lazy_prototype(heap, f.id()) && !source::lazy_prototype(heap, g.id()) && source::lazy_function(heap, g.id()));
    let fname = source::get(heap, f, source::text(heap, "name"));
    assert(fname.is_string() && source::str_len(heap, fname) == heap.code.functions[ordinary].name_len && source::get(heap, g, source::text(heap, "length")).num() == heap.code.functions[arrow].length && heap.fault == source::Fault.None);
    assert(source::own_entry(heap, f, key(heap, "length")).attrs == source::attr_configurable() && source::own_entry(heap, f, key(heap, "name")).attrs == source::attr_configurable() && source::has_property(heap, g, key(heap, "name")));
    var lazy: source::Value[>..] = []; assert(source::own_keys(heap, f, lazy) && lazy.len == 3);
    for name, i in ["length", "name", "prototype"] { assert(source::key_is(heap, lazy[i], name)); }
    // `in` and delete answer a lazy prototype without making it (the Pants
    // engine's function::probe).
    assert(source::has_property(heap, f, key(heap, "prototype")) && !source::delete_property(heap, f, key(heap, "prototype")) && source::lazy_prototype(heap, f.id()) && heap.fault == source::Fault.None);
    source::get(heap, f, source::text(heap, "prototype")); assert(heap.fault == source::Fault.UnsupportedOperand); heap.fault = source::Fault.None;
    assert(!source::put(heap, f, source::text(heap, "x"), source::number(1)) && heap.fault == source::Fault.UnsupportedOperand); heap.fault = source::Fault.None;
    assert(!source::delete_property(heap, g, key(heap, "name")) && heap.fault == source::Fault.UnsupportedOperand); heap.fault = source::Fault.None;
    assert(source::lazy_function(heap, f.id()) && source::lazy_function(heap, g.id()));
    assert(source::materialize_function(heap, f.id()) && !source::lazy_prototype(heap, f.id()) && source::same_text(heap, source::get(heap, f, source::text(heap, "name")), fname));
    let p = source::get(heap, f, source::text(heap, "prototype"));
    assert(p.is_object() && source::get(heap, p, source::text(heap, "constructor")) == f && source::own_entry(heap, f, key(heap, "prototype")).attrs == source::attr_writable() && source::own_entry(heap, f, key(heap, "length")).attrs == source::attr_configurable());
    assert(source::put(heap, f, source::text(heap, "x"), source::number(1)) && source::get(heap, f, source::text(heap, "x")).num() == 1 && !source::delete_property(heap, f, key(heap, "prototype")));
    var made: source::Value[>..] = []; assert(source::own_keys(heap, f, made) && made.len == 4);
    for name, i in ["length", "name", "prototype", "x"] { assert(source::key_is(heap, made[i], name)); }
    assert(source::delete_property(heap, f, key(heap, "x")) && source::node(heap, f.id()).initialized && source::get(heap, f, source::text(heap, "x")).is_undefined() && source::get(heap, f, source::text(heap, "prototype")) == p);
    // Without `name` and `length` a closure is not lazy again: `name` is
    // Function.prototype's, the empty string.
    assert(source::delete_property(heap, f, key(heap, "name")) && source::delete_property(heap, f, key(heap, "length")) && !source::lazy_function(heap, f.id()) && !source::own_entry(heap, f, key(heap, "name")).found && source::get(heap, f, source::text(heap, "name")).is_string() && source::str_len(heap, source::get(heap, f, source::text(heap, "name"))) == 0);
    assert(source::get(heap, g, source::text(heap, "prototype")).is_undefined() && source::materialize_function(heap, g.id()) && source::put(heap, g, source::text(heap, "y"), source::number(2)) && source::get(heap, g, source::text(heap, "call")) == heap.call_method);
    var arrow_keys: source::Value[>..] = []; assert(source::own_keys(heap, g, arrow_keys) && arrow_keys.len == 3);
    for name, i in ["length", "name", "y"] { assert(source::key_is(heap, arrow_keys[i], name)); }
    source::get(heap, g, source::text(heap, "bind")); assert(heap.fault == source::Fault.UnsupportedOperand); heap.fault = source::Fault.None;
    // Snapshots validate arrays' and closures' shapes and records.
    var c = snapshot::Control {};
    let saved = snapshot::capture(heap, [snapshot::Binding { id: 0, generation: 1, value: f }, snapshot::Binding { id: 1, generation: 1, value: g }, snapshot::Binding { id: 2, generation: 1, value: a }, snapshot::Binding { id: 3, generation: 1, value: b }], c);
    assert(c.error == snapshot::Error.None && snapshot::validate(saved, c) && c.error == snapshot::Error.None);
}
// Key enumeration (light-cljs-804): for-in over a three-object chain with
// shadowing and deletion during the loop, object rest and spread (a getter
// run from a CopyData native frame, an exception through it), and ordinary
// objects' index keys, every budget cut matching the uncut run; then the
// element runs and key lists' Goose functions.
fn key_enumeration() {
    check(embed_file("../corpus/serial/for-in.gsp"), "boolean:true");
    check(embed_file("../corpus/serial/object-rest.gsp"), "boolean:true");
    check(embed_file("../corpus/serial/index-keys.gsp"), "boolean:true");
    // The new instructions run in place or push a native frame: each limit,
    // met uncut or one instruction a slice, stops at the same point.
    limit_points(embed_file("../corpus/serial/object-rest.gsp"));
    heap_program::join(thread_spawn(element_runs));
    heap_program::join(thread_spawn(for_in_rechecks));
    print("serial source VM: for-in, object rest/spread, ordinary objects' element runs and key lists passed");
}
thread_fn element_runs() {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, embed_file("../corpus/serial/calls.gsp"));
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    // Index-keyed data properties go to the element run, names to the shape:
    // the shape holds only the name, and objects with the same names but
    // other indices share it.
    let o = source::allocate(heap, source::Node { kind: node_kind::Object }); let p = source::allocate(heap, source::Node { kind: node_kind::Object });
    assert(source::put(heap, o, source::text(heap, "3"), source::number(3)) && source::put(heap, o, source::text(heap, "name"), source::number(1)) && source::put(heap, o, source::text(heap, "0"), source::number(0)));
    let n = source::node(heap, o.id());
    assert(n.key_count == 1 && n.count >= 4 && source::object_element(heap, o.id(), 3).num() == 3 && source::object_element(heap, o.id(), 1).is_hole());
    assert(source::put(heap, p, source::text(heap, "name"), source::number(2)) && source::put(heap, p, source::text(heap, "7"), source::number(7)) && source::node(heap, p.id()).shape == n.shape);
    // "01", "4294967295" and an index far past the run are shape keys.
    let rows = heap.values.len;
    assert(source::put(heap, o, source::text(heap, "01"), source::number(9)) && source::put(heap, o, source::text(heap, "4294967295"), source::number(9)) && source::put(heap, o, source::text(heap, "1000000"), source::number(9)));
    assert(source::node(heap, o.id()).key_count == 4 && heap.values.len < rows + 16);
    var order: source::Value[>..] = []; assert(source::own_keys(heap, o, order) && order.len == 6);
    for name, i in ["0", "3", "1000000", "name", "01", "4294967295"] { assert(source::key_is(heap, order[i], name)); }
    // An accessor or other attributes under an element's key move it to the
    // shape; the element becomes a hole. A data definition with every
    // attribute keeps it an element.
    let getter = source::closure(heap, 0, []);
    assert(source::define_own(heap, o.id(), key(heap, "0"), source::number(5), source::attrs_data()) && source::object_element(heap, o.id(), 0).num() == 5);
    assert(source::define_own(heap, o.id(), key(heap, "0"), getter, source::attrs_accessor(), 1) && source::object_element(heap, o.id(), 0).is_hole());
    let entry = source::own_entry(heap, o, key(heap, "0")); assert(entry.found && !entry.element && source::is_accessor(entry.value));
    assert(source::define_own(heap, o.id(), key(heap, "3"), source::number(4), source::attr_writable() | source::attr_configurable()) && source::object_element(heap, o.id(), 3).is_hole());
    let hidden = source::own_entry(heap, o, key(heap, "3")); assert(hidden.found && hidden.attrs == 5 && hidden.value.num() == 4);
    var after: source::Value[>..] = []; assert(source::own_keys(heap, o, after) && after.len == 6 && source::key_is(heap, after[0], "0") && source::key_is(heap, after[1], "3"));
    // Delete, read through a prototype, and assignment to an inherited
    // element's key: an own element on the receiver.
    assert(source::put(heap, p, source::text(heap, "2"), source::number(2)) && source::delete_property(heap, p, key(heap, "2")) && source::object_element(heap, p.id(), 2).is_hole() && !source::has_property(heap, p, key(heap, "2")));
    let heir = source::allocate(heap, source::Node { kind: node_kind::Object }); assert(source::set_prototype(heap, heir.id(), p));
    assert(source::get(heap, heir, source::text(heap, "7")).num() == 7 && source::put(heap, heir, source::text(heap, "7"), source::number(8)));
    assert(source::object_element(heap, heir.id(), 7).num() == 8 && source::object_element(heap, p.id(), 7).num() == 7);
    // Own property keys with their attributes: an array's elements, then its
    // non-enumerable `length`.
    var listed: source::OwnKey[>..] = []; let a = source::allocate(heap, source::Node { kind: node_kind::Array });
    assert(source::set_element(heap, a, 1, source::number(1)) && source::own_property_keys(heap, a, listed) && listed.len == 2 && listed[0].index == 1 && listed[0].attrs == 7 && listed[1].attrs == 1);
    // A key list keeps string keys (or every key) as records.
    var keys: source::OwnKey[>..] = []; assert(source::own_property_keys(heap, o, keys));
    let list = source::key_list(heap, o, keys, false); let l = source::node(heap, list.id());
    assert(l.kind == node_kind::KeyList && l.key_count == 6 && l.key_capacity >= 6 && l.start == 0 && l.length == 0 && source::dict_at(heap, list.id(), 0).attrs == source::attrs_accessor());
    assert(heap.fault == source::Fault.None);
}
// ForInNext against attributes (light-cljs-804): a non-enumerable own `x`
// hides an enumerable inherited `x`; a key made non-enumerable after the
// listing, before its turn, is skipped; the prototype's other key follows,
// then the walk ends at the pending %Object.prototype%. An element run of
// holes alone still costs its scan.
fn next_key(job: source::Job&, it: source::Value) -> source::Value {
    job.operands.push(it); source::for_in_next(job); assert(job.heap.fault == source::Fault.None);
    let more = job.operands.pop(); let k = job.operands.pop();
    assert(more.flag() == (k.is_string())); return k;
}
thread_fn for_in_rechecks() {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, embed_file("../corpus/serial/calls.gsp"));
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let p = source::allocate(heap, source::Node { kind: node_kind::Object }); let o = source::allocate(heap, source::Node { kind: node_kind::Object }); assert(source::set_prototype(heap, o.id(), p));
    assert(source::put(heap, p, source::text(heap, "x"), source::number(1)) && source::put(heap, p, source::text(heap, "y"), source::number(1)));
    let hidden = source::attr_writable() | source::attr_configurable();
    assert(source::define_own(heap, o.id(), key(heap, "x"), source::number(2), hidden));
    for name in ["a", "b", "c"] { assert(source::put(heap, o, source::text(heap, name), source::number(1))); }
    var job = source::Job { heap: heap };
    job.operands.push(o); source::for_in_start(job); assert(heap.fault == source::Fault.None); let it = job.operands.pop();
    assert(source::key_is(heap, next_key(job, it), "a"));
    assert(source::define_own(heap, o.id(), key(heap, "b"), source::number(2), hidden));
    assert(source::key_is(heap, next_key(job, it), "c"));
    assert(source::key_is(heap, next_key(job, it), "y"));
    assert(next_key(job, it).is_undefined() && next_key(job, it).is_undefined());
    // Holes alone: the scan of a 64-slot run is charged before it runs.
    let e = source::allocate(heap, source::Node { kind: node_kind::Object });
    assert(source::put(heap, e, source::text(heap, "63"), source::number(1)) && source::delete_property(heap, e, key(heap, "63")) && source::node(heap, e.id()).count == 64);
    heap.limits.work = heap.work + 32;
    var none: source::Value[>..] = []; assert(!source::own_keys(heap, e, none) && heap.fault == source::Fault.WorkLimit && none.len == 0);
}
thread_fn depth_limit(bytes: u8[], frames: i64, fault: source::Fault) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    assert(heap.limits.frames == 10000 && heap.limits.stack == 1048576);
    heap.limits.frames = frames;
    let entry = source::closure(heap, 0, []);
    var job = source::Job { heap: heap }; assert(source::start(job, entry, []));
    let outcome = source::run(job, -1);
    if fault == source::Fault.None {
        assert(outcome == tiny::Outcome.Complete && job.result.is_bool() && job.result.flag());
    } else { assert(outcome == tiny::Outcome.Failed && job.fault == fault && !job.thrown); }
}
// The keys of the job's named writes, in order, as one string.
fn written(heap: source::Heap&, job: const source::Job&) -> u8[>..] {
    var out: u8[>..] = [];
    // A Symbol key (`it[Symbol.iterator] = ...`) reads as @@.
    for w in job.writes { if w.key.is_symbol() { out.append("@@"); } else { for u in source::utf16(heap, w.key) { out.push(u as u8); } } out.push(' '); }
    return out;
}
// mode 0 cancel.js, 1 cancel-break.js, 2 loop.js (no finally block), 3
// cancel-fault.js, 4 cancel.js with the instruction limit two steps into the
// cleanup. A cancellation delivered between runs runs the finally blocks,
// innermost first, never the catch block, then fails the job Cancelled; a
// break out of the cleanup's finally block, an exception escaping it or a
// limit ends the cleanup, still Cancelled; with no finally block the job
// fails at once.
thread_fn cancelled_job(bytes: u8[], sliced: bool, mode: i64) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let entry = source::closure(heap, 0, []);
    var job = source::Job { heap: heap }; assert(source::start(job, entry, []));
    assert(source::run(job, 500) == tiny::Outcome.Yielded);
    let looped = job.writes.len;
    assert(source::cancel(job));
    if mode == 2 { assert(job.status == tiny::Status.Failed && job.fault == source::Fault.Cancelled && !source::cancel(job)); return; }
    // A second delivery while the cleanup runs changes nothing.
    assert(source::cancel(job));
    // The innermost finally block runs with a pending cancellation.
    let top = job.frames.len - 1;
    assert(job.status == tiny::Status.Ready && source::own(job, top, source::HandlerKind.Cancel));
    assert(source::pending_value(job, top, 172).flag() && !source::pending_value(job, top, 118).flag() && source::pending_value(job, top, 170).is_undefined());
    if mode == 4 { heap.limits.steps = job.steps + 2; }
    if sliced { while source::run(job, 1) == tiny::Outcome.Yielded {} } else { source::run(job, -1); }
    assert(job.status == tiny::Status.Failed && job.fault == source::Fault.Cancelled && !job.thrown);
    // Cleanup wrote only the finally blocks' keys after the loop's.
    let keys = written(heap, job);
    var expected: u8[>..] = []; for i in looped { expected.append("looped "); }
    expected.append(if mode == 0 { "inner outer " } else if mode == 4 { "" } else { "inner " });
    assert(keys[..] == expected[..]);
}
// With setup 7, for-of.js iterates `hooked`: its @@iterator getter ran once
// (the job wrote `reads` on the object the test installed).
thread_fn iterator_getter_ran(bytes: u8[]) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap)); install(heap, 7);
    var job = source::Job { heap: heap }; assert(source::start(job, source::closure(heap, 0, []), []));
    assert(source::run(job, -1) == tiny::Outcome.Complete && job.result.flag());
    let hooked = source::get(heap, heap.global, source::key_named(heap, "hooked"));
    assert(source::get(heap, hooked, source::key_named(heap, "reads")).num() == 1);
}
// Cancellation inside a for-of loop (cancel-for-of.js, light-cljs-843;
// decision 4): mode 0 in the body, 1 inside `next`, 2 in the body with a
// `return` that throws. The cleanup runs the body's finally block (not in
// mode 1), then `return` once, never the catch block around the loop; the
// job fails Cancelled whatever `return` did.
thread_fn cancelled_for_of(bytes: u8[], sliced: bool, mode: i64) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var job = source::Job { heap: heap }; assert(source::start(job, source::closure(heap, 0, []), [source::number(mode)]));
    assert(source::run(job, 500) == tiny::Outcome.Yielded);
    // The keys written before the cancellation: the iterator's, then the
    // loop's.
    var expected = written(heap, job); assert(find(expected[..], "looped ") >= 0);
    assert(source::cancel(job) && job.status == tiny::Status.Ready);
    if sliced { while source::run(job, 1) == tiny::Outcome.Yielded {} } else { source::run(job, -1); }
    assert(job.status == tiny::Status.Failed && job.fault == source::Fault.Cancelled && !job.thrown);
    let keys = written(heap, job);
    expected.append(if mode == 1 { "closed " } else { "inner closed " });
    assert(keys[..] == expected[..]);
}
// PendingThrow, PendingException and PendingCancel read the frame's
// innermost entry only; an entry of another frame reads as none.
thread_fn pending_completions(bytes: u8[]) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, bytes);
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    var job = source::Job { heap: heap }; assert(source::start(job, source::closure(heap, 0, []), []));
    job.handlers.push(source::Handler { frame: 0, pc: 0, operands: 0, locals: 0, kind: source::HandlerKind.Throw, value: source::number(5) });
    assert(source::pending_value(job, 0, 118).flag() && source::pending_value(job, 0, 170).num() == 5 && !source::pending_value(job, 0, 172).flag());
    assert(!source::pending_value(job, 1, 118).flag() && source::pending_value(job, 1, 170).is_undefined());
    job.handlers[0].kind = source::HandlerKind.Normal;
    assert(!source::pending_value(job, 0, 118).flag() && source::pending_value(job, 0, 170).is_undefined());
    assert(source::retire(job));
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
    let array = source::allocate(heap, source::Node { kind: node_kind::Array }); let length = source::text(heap, "length");
    for i in 4 { assert(source::put(heap, array, source::key(heap, source::number(i)), source::number(i + 10))); }
    assert(source::put(heap, array, length, source::number(2)));
    assert(source::get(heap, array, source::key(heap, source::number(1))).num() == 11);
    assert(source::get(heap, array, source::key(heap, source::number(2))).is_undefined());
    assert(source::put(heap, array, length, source::number(4)));
    assert(source::get(heap, array, source::key(heap, source::number(3))).is_undefined());
    assert(source::put(heap, array, source::key(heap, source::number(2)), source::number(99)));
    assert(source::get(heap, array, source::key(heap, source::number(2))).num() == 99);
    assert(source::put(heap, array, length, source::number(0)) && source::node(heap, array.id()).key_count == 0);
    assert(source::put(heap, array, length, source::number(3)));
    for i in 3 { assert(source::get(heap, array, source::key(heap, source::number(i))).is_undefined()); }
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
        let a = source::allocate(h, source::Node { kind: node_kind::Array }); let k = source::text(h, "length");
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
        let a = source::allocate(h, source::Node { kind: node_kind::Array }); let k = source::text(h, "length");
        assert(source::put(h, a, source::key(h, source::number(0)), source::number(5)));
        assert(source::put(h, a, source::key(h, source::number(524287)), source::number(1)));
        assert(source::put(h, a, source::key(h, source::number(524288)), source::number(2)));
        assert(h.nodes[a.id()].initialized && h.nodes[a.id()].count == 0 && h.nodes[a.id()].length == 524289 && h.values.len < 1024);
        assert(source::get(h, a, source::key(h, source::number(524287))).num() + source::get(h, a, source::key(h, source::number(524288))).num() == 3);
        assert(source::get(h, a, source::key(h, source::number(0))).num() == 5 && source::get(h, a, source::key(h, source::number(7))).is_undefined());
        assert(source::put(h, a, k, source::number(524288)) && source::get(h, a, source::key(h, source::number(524288))).is_undefined() && source::get(h, a, source::key(h, source::number(524287))).num() == 1);
    }
}
fn argument_limits(bytes: const u8[:]) {
    for op in [opcode::NewArguments, opcode::Rest] { for limit in 4 { heap_program::join(thread_spawn(argument_limit, str(bytes), op, limit)); } }
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
                if op == opcode::NewArguments { heap.limits.values = heap.values.len - heap.image.values + 2; }
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
        if program::word(bytes, at) == export_descriptor(opcode::NewCell) { variant[at] = opcode::NewFixedCell; changed = true; }
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
    for i in program::word(bytes, 16) { if program::word(bytes, l.constants + i * 12) == constant::Number { number = i; } }
    assert(number >= 0);
    for tag, i in [constant::Symbol, constant::Symbol, value_tag::Object, constant::BigInt + 1] {
        var patched: u8[>..] = []; patched.append(bytes); let at = l.constants + number * 12;
        set_word(patched[..], at, tag); set_word(patched[..], at + 4, if i == 1 { 15 } else { 7 }); set_word(patched[..], at + 8, 0);
        if i > 0 { assert(program::install(image, patched) == program::Error.Operation); continue; }
        check(patched, "Fault.Unsupported:const(Symbol)", source::Fault.Unsupported);
    }
}
// construct.js with F's constructor kind patched (light-cljs-831): as a
// class constructor `new F(2)` fails by name, uncaught; with no constructor
// kind (a method's or an arrow's) it is a TypeError, here uncaught.
fn construct_kinds(bytes: const u8[:]) {
    var code = source::Code {}; decode(code, bytes);
    let id = named_function(code, "F"); let l = program::layout(bytes); let at = l.functions + id * 52 + 48;
    for kind in [constructor::BaseClass, constructor::DerivedClass, constructor::None] {
        var patched: u8[>..] = []; patched.append(bytes);
        set_word(patched[..], at, (program::word(bytes, at) & ~(255 << 8)) | (kind << 8));
        if kind == constructor::None { check(patched, "", source::Fault.TypeError); }
        else { check(patched, "Fault.UnsupportedOperand:construct:-class-constructors", source::Fault.UnsupportedOperand); }
    }
}
fn set_unit(bytes: u8[:], at: i64, n: i64) { bytes[at] = (n & 255) as u8; bytes[at + 1] = ((n >> 8) & 255) as u8; }
// The image's direct-eval mark (light-cljs-823) survives load on exactly the
// site's call_spread, which decodes to a spread DirectEval; each marked or
// unmarked call opcode decodes as its variant.
fn direct_eval_decode(bytes: const u8[:]) {
    var code = source::Code {}; decode(code, bytes);
    var marked: i64 = 0;
    for op, i in code.ops {
        if op.direct_eval {
            marked += 1; assert(op.code == opcode::CallSpread);
            let spread = match code.ins[i] { DirectEval d => d.spread && !d.method, _ => false }; assert(spread);
        }
    }
    assert(marked == 1);
    for n in [opcode::Call, opcode::CallMethod, opcode::CallSpread, opcode::CallMethodSpread] {
        for flag in [false, true] {
            let ins = source::instruction(source::Op { code: n, arg: 0, offset: 0, start: 0, end: 0, direct_eval: flag });
            let ok = match ins {
                DirectEval d => flag && d.method == (n == opcode::CallMethod || n == opcode::CallMethodSpread) && d.spread == (n == opcode::CallSpread || n == opcode::CallMethodSpread),
                Call => !flag && n == opcode::Call, CallMethod => !flag && n == opcode::CallMethod, Unsupported u => !flag && (n == opcode::CallSpread || n == opcode::CallMethodSpread) && u.opcode == n,
                _ => false,
            };
            assert(ok);
        }
    }
}
// Exported BigInt constants (sign unit, then 16-bit digits without a high
// zero digit) and selector operands validate before installation, including
// on dead paths; a failed install keeps the previous image.
fn bigint_constants(bytes: const u8[:]) {
    let l = program::layout(bytes); var zero: i64 = -1; var wide: i64 = -1; var bigints: i64 = 0;
    for i in program::word(bytes, 16) {
        let at = l.constants + i * 12;
        if program::word(bytes, at) != constant::BigInt { continue; }
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
            for i in program::word(bytes, 24) { let at = l.instructions + i * 20; if (program::word(bytes, at) & 255) == opcode::ThrowError { set_word(bad[..], at + 4, export_selector_count(opcode::ThrowError)); found = true; } }
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
    var end: i64 = -1; for i in start..(start + count) { if original.ops[i].code == opcode::Return && original.ops[i].depth >= 0 { end = l.instructions + i * 20; break; } }
    assert(end >= 0);
    var bad: u8[>..] = []; bad.append(bytes); for i in start..(start + count) { if original.ops[i].code == opcode::Return { set_word(bad[..], l.instructions + i * 20, export_descriptor(opcode::Pop)); } }
    var image = program::Image {}; assert(program::install(image, bad) == program::Error.None);
    var falling = source::Code {}; assert(!source::load(falling, image) && falling.error == tiny::LoadError.Fallthrough);
    bad.clear(); bad.append(bytes);
    var changed = false;
    for i in count {
        let at = l.instructions + (start + i) * 20;
        if (program::word(bytes, at) & 255) == opcode::JumpIfFalse { assert(original.ops[start + i + 2].depth == 1); set_word(bad[..], at + 4, original.ops[start + i + 2].offset); changed = true; break; }
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
    assert(source::form_kind(heap, job.result) == node_kind::Vector && source::form_count(heap, job.result) == 2);
    assert(source::form_nth(heap, job.result, 0).num() == 2 && source::form_nth(heap, job.result, 1).num() == 3);
}
// dup2 and dup3 (light-cljs-830). A hand-assembled one-function image (the
// layout source_tests.goose's depth_limit writes) pushes three distinct
// values, an object, undefined and an array; dup2 or dup3 copies the top two
// or three in order, which the test reads one instruction at a time; then
// width - 1 pops leave the lowest copy on top, and the function returns it.
fn push_word(bytes: u8[>..]&, value: i64) { for i in 4 { bytes.push(((value >> (8 * i)) & 255) as u8); } }
fn duplicate_image(dup: i64, width: i64) -> u8[] {
    var ops: i64[>..] = [opcode::NewObject, opcode::Undefined, opcode::NewArray, dup];
    for i in (width - 1) { ops.push(opcode::Pop); }
    ops.push(opcode::Return);
    var bytes: u8[>..] = []; bytes.append("GSP\0");
    // Version 3, no source, one function, no constants or children.
    for n in [3, 0, 1, 0, 0, ops.len, 0] { push_word(bytes, n); }
    // The function: its instruction run, no constants, children, parameters,
    // locals, captures or name; flags 1 marks it strict.
    for n in [0, ops.len, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1] { push_word(bytes, n); }
    for op, i in ops { for n in [export_descriptor(op), 0, 0, 0, i] { push_word(bytes, n); } }
    return str(bytes);
}
thread_fn duplicate_run(bytes: u8[], width: i64) {
    defer { heap_program::finish(); }
    var code = source::Code {}; decode(code, bytes);
    assert(match code.ins[3] { Dup2 => width == 2, Dup3 => width == 3, _ => false });
    var heap = source::Heap { code: code }; assert(source::initialize(heap));
    let entry = source::closure(heap, 0, []); var job = source::Job { heap: heap }; assert(source::start(job, entry, []));
    for i in 4 { assert(source::run(job, 1) == tiny::Outcome.Yielded); }
    var seen: source::Value[>..] = []; for v in job.operands { seen.push(v); }
    assert(seen.len == 3 + width);
    assert(seen[0].is_object() && seen[1].is_undefined() && seen[2].is_object() && !source::same_value(heap, seen[0], seen[2]));
    for i in width { assert(source::same_value(heap, seen[3 - width + i], seen[3 + i])); }
    while source::run(job, 1) == tiny::Outcome.Yielded {}
    assert(job.status == tiny::Status.Complete && job.fault == source::Fault.None);
    assert(source::same_value(heap, job.result, seen[3]));
}
fn duplicates() {
    heap_program::join(thread_spawn(duplicate_run, duplicate_image(opcode::Dup2, 2), 2));
    heap_program::join(thread_spawn(duplicate_run, duplicate_image(opcode::Dup3, 3), 3));
}
