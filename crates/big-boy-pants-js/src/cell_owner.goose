import std;
import os;
import slice_pool;

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
// A flat value (Flat) is a primitive, or a form graph (bridge::Graph in GFB1
// bytes, with cell references as tag 10) and its process-wide identity: an
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
// "cell budget". No compaction: the 200k project makes no cell, and no
// measured workload needs one (light-cljs-799's census).

// kind: 0 undefined, 1 null, 2 boolean (number 0 or 1), 3 number, 4 string
// (UTF-16 little-endian units in data), 5 a form graph (GFB1 in data).
struct Flat { kind: i64 = 0, number: f64 = 0, identity: i64 = 0, data: u8[] = [] }
// Operations: 1 new cell of `flavor` (0 atom, 1 volatile) holding `value`;
// 2 load; 3 store `value`; 4 compare-and-set: store `value` if the cell holds
// `expected`, otherwise answer the value it holds; 9 stop (the coordinator,
// after every job worker is joined; the owner answers with Stats).
struct Request { slot: i64 = -1, op: i64 = 0, cell: i64 = -1, flavor: i64 = 0, expected: Flat = Flat {}, value: Flat = Flat {} }
// error: 0 none, 1 the session's cell budget, 2 no such cell, 3 the session
// is halting, 4 no owner (a program that runs no cell owner).
struct Answer { ok: bool = true, error: i64 = 0, cell: i64 = -1, value: Flat = Flat {} }
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
// waits (none yet) and the nanoseconds it spent serving.
struct Stats { cells: i64 = 0, identities: i64 = 0, operations: i64 = 0, conflicts: i64 = 0, waits: i64 = 0, owner_ns: i64 = 0 }
inline fn slots() -> i64 { return 8; }
// Set by the coordinator once its owner runs, before it spawns any other
// program, so every preparer and job worker inherits it; false elsewhere
// (the serial control, the test programs), where a cell fails by name.
var enabled: bool = false;

// One cell: its flavor and value, the value's bytes a run of `bytes`.
struct Entry { flavor: i64 = 0, kind: i64 = 0, number: f64 = 0, identity: i64 = 0, start: i64 = 0, len: i64 = 0 }
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
fn stop() -> Stats { assert(qput(Request { op: 9 })); enabled = false; return qget<Stats>(); }
fn halt() { qput(Halt {}); }
fn halted() -> bool { let h, present = qpoll<Halt>(); if present { assert(qput(h)); } return present; }
// Whether a held value matches the expected one: the same identity for a
// graph, the same primitive otherwise.
fn matches(e: const Entry&, data: const u8[:], x: const Flat&) -> bool {
    if e.kind != x.kind { return false; }
    if e.kind == 5 { return e.identity == x.identity; }
    return bytes_of([e.number]) == bytes_of([x.number]) && data == x.data;
}
fn serve(limit: i64) {
    var entries: Entry[>..] = []; var bytes = slice_pool<u8> {}; var stats = Stats {}; var held: i64 = 0; var next: i64 = 1;
    loop {
        let r = qget<Request>();
        if r.op == 9 { assert(qput(stats)); return; }
        let started = clock_ns(); stats.operations += 1;
        let value .= r.value;
        if r.op != 1 && (r.cell < 0 || r.cell >= entries.len) { reply(r.slot, Answer { ok: false, error: 2, cell: r.cell }); stats.owner_ns += clock_ns() - started; continue; }
        // A load, or a compare-and-set the cell refuses, answers what the
        // cell holds; it numbers and charges nothing.
        if r.op == 2 || r.op == 4 {
            let e .= entries[r.cell]; let current = bytes.items[e.start..(e.start + e.len)];
            if r.op == 2 || !matches(e, current, r.expected) {
                if r.op == 4 { stats.conflicts += 1; }
                stats.owner_ns += clock_ns() - started;
                reply(r.slot, Answer { ok: r.op == 2, cell: r.cell, value: Flat { kind: e.kind, number: e.number, identity: e.identity, data: current } });
                continue;
            }
        }
        // A graph stored for the first time gets its identity here.
        let numbered = value.kind == 5 && value.identity == 0;
        let numbers = (if r.op == 1 { 1 } else { 0 }) + (if numbered { 1 } else { 0 });
        let freed = if r.op == 1 { 0 } else { entries[r.cell].len };
        if numbers > 0 && entries.len + stats.identities + numbers > limit || held - freed + value.data.len > byte_budget() { reply(r.slot, Answer { ok: false, error: 1, cell: r.cell }); }
        else {
            var identity = value.identity; if numbered { identity = next; next += 1; stats.identities += 1; }
            var cell = r.cell; if r.op == 1 { entries.push(Entry { flavor: r.flavor }); cell = entries.len - 1; stats.cells += 1; }
            let e .= entries[cell];
            let current = bytes.items[e.start..(e.start + e.len)];
            held -= e.len; if e.len > 0 { bytes.free_slice(current); }
            var start: i64 = 0; if value.data.len > 0 { let run = bytes.alloc_copy(value.data); start = bytes.items.range_index(run, "cell bytes"); }
            e = Entry { flavor: e.flavor, kind: value.kind, number: value.number, identity: identity, start: start, len: value.data.len }; held += value.data.len;
            stats.owner_ns += clock_ns() - started;
            reply(r.slot, Answer { cell: cell, value: Flat { kind: value.kind, number: value.number, identity: identity } });
            continue;
        }
        stats.owner_ns += clock_ns() - started;
    }
}
fn reply(slot: i64, a: Answer) {
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
    if !enabled { return Answer { ok: false, error: 4 }; }
    var slot: i64 = -1;
    loop { let token, present = qget<Slot>(5); if present { slot = token.slot; break; } if halted() { return Answer { ok: false, error: 3 }; } }
    var request = copy(r); request.slot = slot; assert(qput(request));
    loop {
        let a, present = receive(slot);
        if present { assert(qput(Slot { slot: slot })); return a; }
        if halted() { return Answer { ok: false, error: 3 }; }
    }
    return Answer { ok: false, error: 3 };
}
