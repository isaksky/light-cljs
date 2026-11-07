import std;
import os;
import slice_pool;
import .readiness;

namespace cells;

// Process-wide cells (light-cljs-799; docs/javascript-coverage.md, decision
// 3, "Cells"). Compile-time state a macro namespace keeps in an atom or a
// volatile outlives the job that made it and is shared by every job worker:
// when a published value (a prepared definition, a produced helper) reaches
// an atom, the atom becomes a cell, held here as a flat value by one owner
// thread program with no VM tables, for the session. Workers never share a
// heap (probe P17): a job loads a cell's value into its own rows and stores
// or compare-and-sets a flat encoding back, each a message to the owner.
//
// A flat value (Flat) is a primitive, or a form graph (bridge::Graph in GFB2
// bytes, with cell references as form_tag::Cell) and its process-wide identity: an
// object that leaves a job in a cell value gets one when it first leaves
// (the owner numbers it), a job remembers the identity of every object it
// loaded or stored (source::CellRow), and it imports each identity once, so a
// value stored back keeps its identity and `identical?` between two loads,
// or between a stored value and a later load, answers what one shared heap
// would. A compare-and-set (swap!) succeeds when the cell still holds the
// expected value's identity, or for a primitive the same primitive, as
// Clojure's Atom.swap compares references; no version counter moves.
//
// Replies go on a queue private to the requester's execution slot (probe
// P18: one queue routed by address bounced replies millions of times): a
// requester takes one of slots() Slot tokens for the exchange, and
// Reply<S0>..Reply<S7>, one marker type per slot, select distinct queues. At
// most `limit` (at most 8) jobs execute at once, so a token is free whenever
// an executing job asks; one held by a worker that aborted is gone, but an
// abort ends the session and Halt releases every waiter.
//
// Cells and identities stay until the session ends, counted against one
// budget (BBP_CELL_BUDGET, a count of cells plus identities); their bytes
// against byte_budget(). Past either an operation fails with the named error
// "cell budget". No compaction: the 200k project, the expanded fixture and
// the conformance cases make no cell, so no measured workload needs one
// (light-cljs-809's census; the JOBS line ends with the bytes held).

// A flat value's kind. Goose-private numbers: the owner and the jobs are
// the only readers.
let flat_kind::Undefined = 0;
let flat_kind::Null = 1;
// A boolean, number 0 or 1.
let flat_kind::Bool = 2;
let flat_kind::Number = 3;
// UTF-16 little-endian units in data.
let flat_kind::String = 4;
// A form graph, GFB2 in data.
let flat_kind::Graph = 5;
// An error a delay's thunk threw (cells.goose error_flat), with an identity.
let flat_kind::Thrown = 6;
// A callable graph (light-cljs-809: a value that reaches a function, as
// callable::encode_cell publishes it, in data), numbered as a form graph.
let flat_kind::Callable = 7;
// A function of the session's image, its row in number.
let flat_kind::Function = 8;
// A graph's nodes have identities (light-cljs-808): `nodes` holds one per
// graph node, 0 for one the owner numbers when it is first stored and -1
// for one that has none (a primitive child), and `root` is the root's
// index; `identity` is the root's.
struct Flat { kind: i64 = flat_kind::Undefined, number: f64 = 0, identity: i64 = 0, data: u8[] = [], root: i64 = 0, nodes: i64[] = [] }
// A cell's flavor.
let cell_flavor::Atom = 0;
let cell_flavor::Volatile = 1;
let cell_flavor::Delay = 2;
// A request's operation. New: a cell of `flavor` holding `value`. Store:
// `value` (a delay's: realized). CompareAndSet: store `value` if the cell
// holds `expected`, otherwise answer the value it holds. Stop: the
// coordinator, after every job worker is joined; the owner answers with
// Stats. A delay's (light-cljs-808, the operations from Force on): Force,
// claimed by `holder` (a macro job's compiler id) when pending; Settle the
// claim with `value`, its thunk's result or, `failed`, what it threw;
// Release every claim `holder` holds (or just its claim of `cell`), for a
// job that ended or a force that failed without settling.
let cell_op::None = 0;
let cell_op::New = 1;
let cell_op::Load = 2;
let cell_op::Store = 3;
let cell_op::CompareAndSet = 4;
let cell_op::Force = 5;
let cell_op::Settle = 6;
let cell_op::Release = 7;
let cell_op::Stop = 9;
struct Request { slot: i64 = -1, op: i64 = cell_op::None, cell: i64 = -1, flavor: i64 = cell_flavor::Atom, expected: Flat = Flat {}, value: Flat = Flat {}, holder: i64 = 0, failed: bool = false }
// An answer's error. Budget: the session's cell budget. NoOwner: a program
// that runs no cell owner. NotClaim: not a delay, or not this holder's
// claim. Encode is never an answer's: a job's own failure to encode a
// value (cells.goose), in the same numbering.
let cell_error::None = 0;
let cell_error::Budget = 1;
let cell_error::NoCell = 2;
let cell_error::Halting = 3;
let cell_error::NoOwner = 4;
let cell_error::Encode = 5;
let cell_error::Cycle = 6;
let cell_error::NotClaim = 7;
// A delay's state (force and load answers). Claimed: by the requester.
// Realized: value. Failed: value, what the thunk threw. Waiting: claimed by
// another job, which the requester waits for: `epoch` names the claim, and
// the coordinator hears Settled with it when the claim ends.
let delay_state::Pending = 0;
let delay_state::Claimed = 1;
let delay_state::Realized = 2;
let delay_state::Failed = 3;
let delay_state::Waiting = 4;
struct Answer { ok: bool = true, error: i64 = cell_error::None, cell: i64 = -1, value: Flat = Flat {}, state: i64 = delay_state::Pending, epoch: i64 = 0 }
// A delay claim that ended (settled or released) while jobs waited for it,
// for the coordinator, which resumes them (compiler_session.goose).
struct Settled { cell: i64, epoch: i64 }
struct Reply<M> { answer: Answer, marker: M }
struct S0 {}
struct S1 {}
struct S2 {}
struct S3 {}
struct S4 {}
struct S5 {}
struct S6 {}
struct S7 {}
struct Slot { slot: i64 }
// Posted once, and re-queued by whoever polls it, when the session cancels
// or a worker aborts: a waiter for a token or a reply then gives up.
struct Halt {}
// The owner's totals for the JOBS line: cells made, identities numbered,
// operations served, compare-and-sets refused (each a swap! rerun), delay
// waits (forces answered "claimed by another job"), the nanoseconds it
// spent serving and the bytes of cell values it holds at the end (the
// census, light-cljs-809).
struct Stats { cells: i64 = 0, identities: i64 = 0, operations: i64 = 0, conflicts: i64 = 0, waits: i64 = 0, owner_ns: i64 = 0, bytes: i64 = 0 }
inline fn slots() -> i64 { return 8; }
// Set by the coordinator once its owner runs, before it spawns any other
// program, so every preparer and job worker inherits it; false elsewhere
// (the serial control, the test programs), where a cell fails by name.
var enabled: bool = false;

// One cell: its flavor and value, the value's bytes a run of `bytes` and its
// graph's node identities a run of `ids` (root the root's index). A delay
// (cell_flavor::Delay) also has its state (Answer's), the holder of its claim, the
// claim's epoch and how many jobs wait for it.
struct Entry { flavor: i64 = cell_flavor::Atom, kind: i64 = flat_kind::Undefined, number: f64 = 0, identity: i64 = 0, start: i64 = 0, len: i64 = 0, ids_start: i64 = 0, ids_len: i64 = 0, root: i64 = 0, state: i64 = delay_state::Pending, holder: i64 = 0, epoch: i64 = 0, waiters: i64 = 0 }
// A job waiting for a delay another job claimed: the cycle check follows
// these from a claim's holder.
struct WaitEdge { holder: i64, cell: i64 }
// The owner's tables (its cells' bytes and identities are the pools
// beside it, which a root struct cannot hold).
root struct Store { entries: Entry[>..] = [], edges: WaitEdge[>..] = [], stats: Stats = Stats {}, held: i64 = 0, next: i64 = 1, limit: i64 = 0 }
fn byte_budget() -> i64 { return 268435456; }
// BBP_CELL_BUDGET: cells plus identities a session may number.
fn budget() -> i64 {
    var text: u8[>..] = []; if !env("BBP_CELL_BUDGET", text) { return 1048576; }
    let n, ok = parse_int(text); assert(ok && n >= 0); return n;
}
// The owner runs in a thread program the coordinator spawns
// (compiler_session.goose cell_owner; a thread_fn here would keep every
// program that imports the VM out of the debug runs, which cannot place
// thread-local storage). Its slot tokens and halt flag are set first.
fn prepare() { qlimit<Slot>(slots()); for i in slots() { assert(qput(Slot { slot: i })); } qlimit<Halt>(1); }
// After every job worker has exited: the owner's totals.
fn stop() -> Stats { assert(qput(Request { op: cell_op::Stop })); enabled = false; return qget<Stats>(); }
fn halt() { qput(Halt {}); }
fn halted() -> bool { let h, present = qpoll<Halt>(); if present { assert(qput(h)); } return present; }
// Whether a held value matches the expected one: the same identity for a
// graph, the same primitive otherwise.
// A job's expected value names an object by identity whichever transport
// held it (source::expected), so the two graph kinds compare as one.
fn graph(kind: i64) -> bool { return kind == flat_kind::Graph || kind == flat_kind::Callable; }
fn matches(e: const Entry&, data: const u8[:], x: const Flat&) -> bool {
    if graph(e.kind) && graph(x.kind) { return e.identity == x.identity; }
    if e.kind != x.kind { return false; }
    return bytes_of([e.number]) == bytes_of([x.number]) && data == x.data;
}
fn serve(limit: i64) {
    var s = Store { limit: limit }; var bytes = slice_pool<u8> {}; var ids = slice_pool<i64> {};
    loop {
        let r = qget<Request>();
        if r.op == cell_op::Stop { s.stats.bytes = s.held; assert(qput(s.stats)); return; }
        let started = clock_ns(); s.stats.operations += 1;
        let a = answer(s, bytes, ids, r);
        s.stats.owner_ns += clock_ns() - started;
        reply(r.slot, a);
    }
}
// The value cell `cell` holds, with its data and node identities.
fn held_value(s: const Store&, bytes: const slice_pool<u8>&, ids: const slice_pool<i64>&, cell: i64) -> Flat {
    let e .= s.entries[cell];
    return Flat { kind: e.kind, number: e.number, identity: e.identity, data: bytes.items[e.start..(e.start + e.len)], root: e.root, nodes: ids.items[e.ids_start..(e.ids_start + e.ids_len)] };
}
fn answer(s: Store&, bytes: slice_pool<u8>&, ids: slice_pool<i64>&, r: const Request&) -> Answer {
    if r.op == cell_op::Release { release(s, r.holder, r.cell); return Answer {}; }
    if r.op != cell_op::New && (r.cell < 0 || r.cell >= s.entries.len) { return Answer { ok: false, error: cell_error::NoCell, cell: r.cell }; }
    if r.op >= cell_op::Force && s.entries[r.cell].flavor != cell_flavor::Delay { return Answer { ok: false, error: cell_error::NotClaim, cell: r.cell }; }
    // A load, or a compare-and-set the cell refuses, answers what the cell
    // holds; it numbers and charges nothing.
    if r.op == cell_op::Load || r.op == cell_op::CompareAndSet {
        let e .= s.entries[r.cell];
        if r.op == cell_op::Load || !matches(e, bytes.items[e.start..(e.start + e.len)], r.expected) {
            if r.op == cell_op::CompareAndSet { s.stats.conflicts += 1; }
            return Answer { ok: r.op == cell_op::Load, cell: r.cell, value: held_value(s, bytes, ids, r.cell), state: e.state, epoch: e.epoch };
        }
    }
    if r.op == cell_op::Force { return force(s, bytes, ids, r.cell, r.holder); }
    if r.op == cell_op::Settle {
        let e .= s.entries[r.cell];
        if e.state != delay_state::Claimed || e.holder != r.holder { return Answer { ok: false, error: cell_error::NotClaim, cell: r.cell }; }
    }
    var cell = r.cell;
    if r.op == cell_op::New {
        if s.entries.len + s.stats.identities + 1 > s.limit { return Answer { ok: false, error: cell_error::Budget, cell: -1 }; }
        s.entries.push(Entry { flavor: r.flavor }); cell = s.entries.len - 1; s.stats.cells += 1;
    }
    let stored = store(s, bytes, ids, cell, r.value);
    if !stored.ok { return stored; }
    // A delay's stored value realizes it (publication); a settled claim
    // ends, and its waiters hear of it.
    let e .= s.entries[cell];
    if e.flavor == cell_flavor::Delay && (r.op == cell_op::Store || r.op == cell_op::Settle) {
        e.state = if r.op == cell_op::Settle && r.failed { delay_state::Failed } else { delay_state::Realized };
        if r.op == cell_op::Settle { end_claim(s, cell); }
    }
    return stored;
}
// Store `value` into `cell`: a graph's unnumbered nodes get identities here.
fn store(s: Store&, bytes: slice_pool<u8>&, ids: slice_pool<i64>&, cell: i64, value: const Flat&) -> Answer {
    var numbers: i64 = 0;
    // A graph's unnumbered objects, or a thrown error (flat_kind::Thrown) stored for the
    // first time, get identities here.
    if graph(value.kind) { if value.nodes.len > 0 { for n in value.nodes { if n == 0 { numbers += 1; } } } else if value.identity == 0 { numbers = 1; } }
    if value.kind == flat_kind::Thrown && value.identity == 0 { numbers = 1; }
    let freed = s.entries[cell].len;
    if numbers > 0 && s.entries.len + s.stats.identities + numbers > s.limit || s.held - freed + value.data.len > byte_budget() { return Answer { ok: false, error: cell_error::Budget, cell: cell }; }
    var nodes: i64[>..] = []; nodes.append(value.nodes);
    var identity = value.identity;
    for n, i in nodes { if n == 0 { nodes[i] = s.next; s.next += 1; s.stats.identities += 1; } }
    if graph(value.kind) && nodes.len > 0 { identity = nodes[value.root]; } else if (graph(value.kind) || value.kind == flat_kind::Thrown) && identity == 0 { identity = s.next; s.next += 1; s.stats.identities += 1; }
    let e .= s.entries[cell];
    s.held -= e.len; if e.len > 0 { bytes.free_slice(bytes.items[e.start..(e.start + e.len)]); }
    if e.ids_len > 0 { ids.free_slice(ids.items[e.ids_start..(e.ids_start + e.ids_len)]); }
    var start:     i64 = 0; if value.data.len > 0 { let run = bytes.alloc_copy(value.data); start = bytes.items.range_index(run, "cell bytes"); }
    var ids_start: i64 = 0; if nodes.len > 0 { let run = ids.alloc_copy(nodes); ids_start = ids.items.range_index(run, "cell identities"); }
    e.kind = value.kind; e.number = value.number; e.identity = identity; e.start = start; e.len = value.data.len; e.ids_start = ids_start; e.ids_len = nodes.len; e.root = value.root;
    s.held += value.data.len;
    return Answer { cell: cell, value: Flat { kind: value.kind, number: value.number, identity: identity, root: value.root, nodes: nodes } };
}
// A force of delay `cell` by `holder` (Clojure's Delay.deref): its value
// when realized (or what its thunk threw), the claim when pending, and
// otherwise a wait for the claim's holder, unless that wait closes a cycle
// of claims (the holder waits, directly or through others, for one of
// `holder`'s claims), which fails by name.
fn force(s: Store&, bytes: const slice_pool<u8>&, ids: const slice_pool<i64>&, cell: i64, holder: i64) -> Answer {
    // A retry ends the requester's earlier wait.
    drop_edges(s, holder, -1);
    let e .= s.entries[cell];
    if e.state == delay_state::Realized || e.state == delay_state::Failed { return Answer { cell: cell, value: held_value(s, bytes, ids, cell), state: e.state, epoch: e.epoch }; }
    if e.state == delay_state::Pending { e.state = delay_state::Claimed; e.holder = holder; e.epoch += 1; return Answer { cell: cell, state: delay_state::Claimed, epoch: e.epoch }; }
    var at = e.holder; var steps: i64 = 0;
    while steps <= s.edges.len {
        if at == holder { return Answer { ok: false, error: cell_error::Cycle, cell: cell }; }
        var next: i64 = -1;
        for w in s.edges { if w.holder == at { next = w.cell; break; } }
        if next < 0 || s.entries[next].state != delay_state::Claimed { break; }
        at = s.entries[next].holder; steps += 1;
    }
    s.edges.push(WaitEdge { holder: holder, cell: cell }); e.waiters += 1; s.stats.waits += 1;
    return Answer { cell: cell, state: delay_state::Waiting, epoch: e.epoch };
}
// The claim of `cell` ended: its waiters hear of it, and no edge points at it.
fn end_claim(s: Store&, cell: i64) {
    let e .= s.entries[cell];
    e.holder = 0;
    if e.waiters > 0 { assert(qput(Settled { cell: cell, epoch: e.epoch })); readiness::notify(); e.waiters = 0; }
    drop_edges(s, -1, cell);
}
// Every claim `holder` holds (`cell` < 0), or its claim of `cell`, goes
// back to pending: its job ended without settling them (a fault its thunk
// could not catch, a cancellation) or could not settle one.
fn release(s: Store&, holder: i64, cell: i64) {
    if cell < 0 { drop_edges(s, holder, -1); }
    for i in s.entries.len { if (cell < 0 || i == cell) && s.entries[i].flavor == cell_flavor::Delay && s.entries[i].state == delay_state::Claimed && s.entries[i].holder == holder { s.entries[i].state = delay_state::Pending; end_claim(s, i); } }
}
// Drop the wait edges of `holder`, or those into `cell`.
fn drop_edges(s: Store&, holder: i64, cell: i64) {
    var kept: WaitEdge[>..] = [];
    for w in s.edges { if w.holder != holder && w.cell != cell { kept.push(w); } }
    if kept.len != s.edges.len { s.edges.clear(); s.edges.append(kept); }
}
fn reply(slot: i64, a: const Answer&) {
    let sent = match slot {
        0 => qput(Reply<S0> { answer: copy(a), marker: S0 {} }), 1 => qput(Reply<S1> { answer: copy(a), marker: S1 {} }),
        2 => qput(Reply<S2> { answer: copy(a), marker: S2 {} }), 3 => qput(Reply<S3> { answer: copy(a), marker: S3 {} }),
        4 => qput(Reply<S4> { answer: copy(a), marker: S4 {} }), 5 => qput(Reply<S5> { answer: copy(a), marker: S5 {} }),
        6 => qput(Reply<S6> { answer: copy(a), marker: S6 {} }), _ => qput(Reply<S7> { answer: copy(a), marker: S7 {} }),
    };
    assert(sent);
}
fn take<M>() -> Answer, bool { let r, present = qget<Reply<M>>(5); return copy(r.answer), present; }
fn receive(slot: i64) -> Answer, bool {
    match slot {
        0 => { let a, p = take<S0>(); return a, p; }, 1 => { let a, p = take<S1>(); return a, p; },
        2 => { let a, p = take<S2>(); return a, p; }, 3 => { let a, p = take<S3>(); return a, p; },
        4 => { let a, p = take<S4>(); return a, p; }, 5 => { let a, p = take<S5>(); return a, p; },
        6 => { let a, p = take<S6>(); return a, p; }, _ => { let a, p = take<S7>(); return a, p; },
    }
}
// One request and its answer, under a slot token.
fn exchange(r: Request) -> Answer {
    if !enabled { return Answer { ok: false, error: cell_error::NoOwner }; }
    var slot: i64 = -1;
    loop { let token, present = qget<Slot>(5); if present { slot = token.slot; break; } if halted() { return Answer { ok: false, error: cell_error::Halting }; } }
    var request = copy(r); request.slot = slot; assert(qput(request));
    loop {
        let a, present = receive(slot);
        if present { assert(qput(Slot { slot: slot })); return a; }
        if halted() { return Answer { ok: false, error: cell_error::Halting }; }
    }
    return Answer { ok: false, error: cell_error::Halting };
}
