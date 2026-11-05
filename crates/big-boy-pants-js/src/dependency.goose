import std;

namespace dependency;

// Ordered coordinator messages adapt be0e53e7:tests/dependency.rs and
// 27d517f4:src/dep.rs. This authority is never part of a VM snapshot.
struct Key { compilation: i64, session: i64, generation: i64, definition: i64, kind: i64 }
enum Outcome { Pending, Success, Failed, Cancelled }
enum Diagnostic { None, VM, Snapshot, Artifact, Missing, Cycle, MidJobNeed, Protocol, Prerequisite, Cancelled, Budget }
struct Terminal { outcome: Outcome = Outcome.Pending, diagnostic: Diagnostic = Diagnostic.None, payload: i64 = -1 }
struct Cell { key: Key, producer: i64, terminal: Terminal = Terminal {} }
struct Edge { consumer: i64, cell: i64, delivered: bool = false }
struct Delivery { consumer: i64, cell: i64, terminal: Terminal }
struct Request { key: Key, producer: i64 }
struct Read { cell: i64 = -1, producer: i64 = 0, terminal: Terminal = Terminal {} }
struct Completion { key: Key, producer: i64, terminal: Terminal }
root struct Table {
    compilation: i64, session: i64, next_producer: i64 = 1,
    capacity:    i64                                   = 256, edge_capacity: i64 = 2048,
    cells:       Cell[>..]                             = [], edges: Edge[>..] = [], requests: Request[>..] = [], deliveries: Delivery[>..] = [],
}
fn same(a: Key, b: Key) -> bool { return a.compilation == b.compilation && a.session == b.session && a.generation == b.generation && a.definition == b.definition && a.kind == b.kind; }
fn valid(t: const Table&, k: Key) -> bool { return k.compilation == t.compilation && k.session == t.session && k.compilation > 0 && k.session > 0 && k.generation > 0 && k.definition >= 0 && k.kind >= 0; }
fn find(t: const Table&, key: Key) -> i64 { for cell, i in t.cells { if same(cell.key, key) { return i; } } return -1; }
fn ensure(t: Table&, key: Key) -> Read {
    if !valid(t, key) { return Read {}; }
    let existing = find(t, key);
    if existing >= 0 { return Read { cell: existing, terminal: t.cells[existing].terminal }; }
    if t.cells.len >= t.capacity { return Read {}; }
    let producer = t.next_producer; t.next_producer += 1;
    let index = t.cells.len; t.cells.push(Cell { key: key, producer: producer });
    t.requests.push(Request { key: key, producer: producer });
    return Read { cell: index, producer: producer };
}
fn demand(t: Table&, consumer: i64, key: Key) -> Read {
    let old = find(t, key);
    for edge in t.edges { if edge.consumer == consumer && edge.cell == old { return Read { cell: old, terminal: t.cells[old].terminal }; } }
    if consumer < 0 || t.edges.len >= t.edge_capacity { return Read {}; }
    let read = ensure(t, key); if read.cell < 0 { return read; }
    let done = read.terminal.outcome != Outcome.Pending;
    t.edges.push(Edge { consumer: consumer, cell: read.cell, delivered: done });
    if done { t.deliveries.push(Delivery { consumer: consumer, cell: read.cell, terminal: read.terminal }); }
    return read;
}
fn may_complete(t: const Table&, c: Completion) -> bool {
    let i = find(t, c.key);
    return valid(t, c.key) && i >= 0 && t.cells[i].producer == c.producer && t.cells[i].terminal.outcome == Outcome.Pending && c.terminal.outcome != Outcome.Pending;
}
// A caller may validate two artifacts before either installation. Always
// recheck here: validation never reserves or changes the single-assignment cell.
fn install(t: Table&, c: Completion) -> bool {
    if !may_complete(t, c) { return false; }
    let i = find(t, c.key); t.cells[i].terminal = c.terminal;
    for &edge in t.edges {
        if edge.cell == i && !edge.delivered { edge.delivered = true; t.deliveries.push(Delivery { consumer: edge.consumer, cell: i, terminal: c.terminal }); }
    }
    return true;
}
