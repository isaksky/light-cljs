import std;
import .source_vm;
import .snapshot;
import .continuations;

namespace source;

// Reclamation inside a job (light-cljs-847; docs/javascript-coverage.md,
// decision 3, "Reclamation inside a job"). A job's garbage is otherwise
// reclaimed only when its worker truncates its tables after the job; a
// long-running job whose live set is small can still pass a table's row
// limit. Past a trigger, the job *evacuates in place*: it finds every row
// above its floor that a root reaches, slides those rows down over the dead
// ones in their table order, and rewrites every link to them through a
// remap. Rows stay plain u32 IDs (decision 2); there is no free list, no
// mark bit in any row and no write barrier.
//
// Cost before the trigger: the limit check every allocation function
// already makes, against a lowered limit, and a flag the dispatch loop's
// fuel reads (below). Each table's check compares against its limit
// less a margin (Heap.slack); passing that soft limit takes the check's cold
// path (exhausted), which, when the row still fits the real limit, zeroes
// the table's margin, asks for a collection (Heap.reclaim) and lets the
// allocation go on. A job that never passes the soft limit never collects
// and its tables are byte for byte what they were without this.
//
// The collection runs at the next safe point: the top of run_claimed's
// loop, where every value the job holds is in its own state (frames,
// operands, handlers ...) or in rows, never in a Goose local. Asking for a
// collection also spends the dispatch loop's fuel (source_vm.goose, fuel),
// so execute returns to that point after the instruction that asked; a
// native step returns there after every step, and bytecode runs in slices of
// at most reclaim_interval() instructions besides. The margin leaves half
// the remaining rows for what that instruction or step still allocates.
//
// The floor (Job.floor) is each table's length when the job started: rows
// below it (the image, a continuation's earlier rows, the arguments the host
// made) never move, so a value the host held across the run stays valid.
// Every row below the floor is a root, and every link it holds into the
// job's rows is rewritten; that also covers the service rows and image runs
// a job may write. The other roots: every shape's key (shapes are a
// transition cache and are kept), the code constants, the heap's global,
// call method and pending key, and the job's frames, locals, operands,
// handlers, result, lexicals, root receiver, entry, effect arguments and
// frame, promise queue from its head, unhandled rejections, cell rows (a
// publication's rows whole, so its identities stay consecutive), foreign
// ranges (whole, so they stay ranges) and the keys of its write log. The
// write log's objects are weak, and so are its values unless the job keeps
// them (Job.logged_values, for a host that reads them: execute.goose's
// typed outcome): one that only the log names becomes undefined there (the
// compiler counts entries and reads keys). The interned-key index is weak
// too: it is rebuilt from the live key headers.
//
// The pass reaches the same links snapshot::validate checks: a node's
// values, element or capture run (has_values), shape-mode overflow run or
// dictionary records (key_capacity rows, spare ones included), a generator's
// activation and a promise's state; an activation's frame, handler and slot
// runs (capacity rows); a promise's result, reactions and activation; a
// reaction's promise; a string's code units (a view keeps its units, and an
// interned key the units of the text it was made from). A live row's link
// to a row this pass did not keep is a bug in the root set: it aborts rather
// than leaving a dangling row.
//
// After a collection the margin is half of what is left below each limit,
// or none (no further collection of that table) once fewer than
// reclaim_minimum() rows are left: a job whose live set stays that near a
// limit still ends MemoryLimit, as without reclamation, while one whose live
// set only came near it collects again and goes on once that set shrinks.

// Each table's margin below its limit (Limits); 0 disables the trigger.
struct Slack { nodes: i64 = 0, values: i64 = 0, properties: i64 = 0, units: i64 = 0, strings: i64 = 0, shapes: i64 = 0, conts: i64 = 0 }
// Rows a limit must leave free for a collection to be worth starting.
inline fn reclaim_minimum() -> i64 { return 4096; }
// The most instructions run_claimed lets one execute call run.
inline fn reclaim_interval() -> i64 { return 4096; }

// The cold path of a table's limit check (room): `count` more rows passed
// the limit less its margin. Within the limit itself, ask for a collection
// and allow them; otherwise MemoryLimit, as before.
fn exhausted(heap: Heap&, used: i64, limit: i64, slack: i64&, count: i64) -> bool {
    if slack > 0 && count >= 0 && count <= limit - used { slack = 0; heap.reclaim = true; return true; }
    return memory(heap);
}
fn margin(limit: i64, used: i64) -> i64 {
    let left = limit - used;
    return if left >= reclaim_minimum() { left / 2 } else { 0 };
}
fn set_margins(heap: Heap&) {
    heap.slack = Slack {
        nodes:      margin(heap.limits.nodes, heap.nodes.len - heap.image.nodes), values: margin(heap.limits.values, heap.values.len - heap.image.values),
        properties: margin(heap.limits.properties, heap.properties.len - heap.image.properties), units: margin(heap.limits.units, unit_rows(heap)),
        strings:    margin(heap.limits.units, heap.strings.len - heap.image.strings), shapes: margin(heap.limits.shapes, heap.shapes.len + heap.accessors.len - heap.image.shapes - heap.image.accessors),
        conts:      margin(heap.limits.conts, heap.conts.len - heap.image.conts),
    };
}
// A job starts or resumes: its floor is the tables' lengths now.
fn arm(job: Job&) { job.floor = tops(job.heap); job.reclaimable = true; job.heap.reclaim = false; set_margins(job.heap); }
// The job ends: no margins, so the host's allocations see only the limits.
fn disarm(job: Job&) {
    if !job.reclaimable { return; }
    job.reclaimable = false; job.heap.slack = Slack {}; job.heap.reclaim = false;
}

// The pass's scratch, local to one collection. Per table, one entry a row at
// or above the floor and one more: 1 for a row a root reaches, then (number)
// the count of reached rows before it, the extra entry their total.
root struct Evacuation {
    floor: Tops     = Tops {},
    nodes: u32[>..] = [], values: u32[>..] = [], properties: u32[>..] = [], strings: u32[>..] = [],
    units: u32[>..] = [], latin1: u32[>..] = [], accessors: u32[>..] = [], conts: u32[>..] = [],
    // Code-unit coverage by the reached strings: +1 where one's units
    // start, -1 past their end.
    wide: i32[>..]  = [], narrow: i32[>..] = [],
    work: i64[>..<] = [], pending: i64[>..<] = [],
    // Mapped parameter cells above the floor (weak links, mark_cells) and
    // the value each would keep once detached.
    cells: i64[>..] = [], kept_values: Value[>..] = [],
}
// An object or string link (an accessor pair's halves hold no accessor).
fn mark_link(e: Evacuation&, heap: const Heap&, v: Value) {
    match v {
        Object x => { let i = x.o - e.floor.nodes; if i >= 0 && e.nodes[i] == 0 { e.nodes[i] = 1; e.work.push(x.o); } },
        String x => { mark_string(e, heap, x.s); },
        _ => {},
    }
}
fn mark(e: Evacuation&, heap: const Heap&, v: Value) {
    match v {
        Accessor x => {
            let i = x.a - e.floor.accessors;
            if i >= 0 && e.accessors[i] == 0 { e.accessors[i] = 1; let p = accessor_row(heap, x.a); mark_link(e, heap, p.get); mark_link(e, heap, p.set); }
        },
        _ => { mark_link(e, heap, v); },
    }
}
fn mark_string(e: Evacuation&, heap: const Heap&, s: i64) {
    let i = s - e.floor.strings;
    if i < 0 || e.strings[i] != 0 { return; }
    e.strings[i] = 1;
    let r = header(heap, s); let floor = if r.wide { e.floor.units } else { e.floor.latin1 };
    let start: i64 = r.start; let first = max(start, floor); let end = start + r.len;
    if end <= first { return; }
    if r.wide { e.wide[first - floor] += 1; e.wide[end - floor] -= 1; } else { e.narrow[first - floor] += 1; e.narrow[end - floor] -= 1; }
}
fn mark_object(e: Evacuation&, row: i64) {
    let i = row - e.floor.nodes;
    if i >= 0 && e.nodes[i] == 0 { e.nodes[i] = 1; e.work.push(row); }
}
// The rows of a run at or above the floor, and what they hold.
fn mark_values(e: Evacuation&, heap: const Heap&, start: i64, count: i64) {
    for r in max(start, e.floor.values)..(start + count) {
        let i = r - e.floor.values;
        if e.values[i] == 0 { e.values[i] = 1; mark(e, heap, slot(heap, r)); }
    }
}
fn mark_properties(e: Evacuation&, heap: const Heap&, start: i64, count: i64) {
    for r in max(start, e.floor.properties)..(start + count) {
        let i = r - e.floor.properties;
        if e.properties[i] == 0 { e.properties[i] = 1; let p = property_row(heap, r); mark(e, heap, p.key); mark(e, heap, p.value); }
    }
}
fn mark_cont(e: Evacuation&, r: u32) {
    if r == no_row() { return; }
    let i = r - e.floor.conts;
    if i >= 0 && e.conts[i] == 0 { e.conts[i] = 1; e.pending.push(r); }
}
fn mark_conts(e: Evacuation&, first: u32, capacity: u32) {
    if capacity == 0 || first == no_row() { return; }
    for k in capacity { mark_cont(e, first + k); }
}
fn trace_node(e: Evacuation&, heap: const Heap&, o: i64) {
    let n .= node(heap, o);
    // A mapped cell's arguments object is a weak link (mark_cells).
    if n.kind == node_kind::MappedBinding && o >= e.floor.nodes { e.cells.push(o); } else { mark(e, heap, n.value); }
    mark(e, heap, n.namespace_value); mark(e, heap, n.metadata); mark(e, heap, n.proto);
    for v in n.slots { mark(e, heap, v); }
    if snapshot::has_values(n.kind) { mark_values(e, heap, n.start, n.count); }
    if snapshot::shaped(n) { mark_values(e, heap, n.keys, n.key_capacity); } else if snapshot::record_run(n) { mark_properties(e, heap, n.keys, n.key_capacity); }
    if n.kind == node_kind::Generator || n.kind == node_kind::Promise { mark_cont(e, n.function); }
}
fn trace_cont(e: Evacuation&, heap: const Heap&, r: i64) {
    match cont_row(heap, r) {
        Activation x => { mark_conts(e, x.a.frames, x.a.frame_capacity); mark_conts(e, x.a.handlers, x.a.handler_capacity); mark_values(e, heap, x.a.values, x.a.capacity); },
        Saved x => { mark(e, heap, x.h.value); },
        Promise x => { mark(e, heap, x.p.result); mark_cont(e, x.p.first); mark_cont(e, x.p.last); mark_cont(e, x.p.activation); },
        Reaction x => { mark_object(e, x.r.target); mark_cont(e, x.r.next); },
        Frame x => { mark(e, heap, x.f.callee); mark(e, heap, x.f.receiver); mark(e, heap, x.f.input); mark(e, heap, x.f.output); },
    }
}
fn mark_frame(e: Evacuation&, heap: const Heap&, f: const Frame&) { mark(e, heap, f.callee); mark(e, heap, f.receiver); mark(e, heap, f.input); mark(e, heap, f.output); }
// Node rows row..row + count, whole.
fn mark_range(e: Evacuation&, row: i64, count: i64) { for r in max(row, e.floor.nodes)..(row + count) { mark_object(e, r); } }
fn mark_roots(e: Evacuation&, job: const Job&) {
    let heap .= job.heap;
    for o in e.floor.nodes { trace_node(e, heap, o); }
    for r in e.floor.values { mark(e, heap, heap.values[r]); }
    for r in e.floor.properties { let p = heap.properties[r]; mark(e, heap, p.key); mark(e, heap, p.value); }
    for r in e.floor.accessors { let p = heap.accessors[r]; mark_link(e, heap, p.get); mark_link(e, heap, p.set); }
    for r in e.floor.conts { trace_cont(e, heap, r); }
    for s in heap.shapes { mark(e, heap, s.key); }
    for v in heap.constants { mark(e, heap, v); }
    mark(e, heap, heap.global); mark(e, heap, heap.call_method); mark(e, heap, heap.pending);
    for f in job.frames { mark_frame(e, heap, f); }
    for v in job.locals { mark(e, heap, v); }
    for v in job.operands { mark(e, heap, v); }
    for h in job.handlers { mark(e, heap, h.value); }
    mark(e, heap, job.result); mark(e, heap, job.lexicals); mark(e, heap, job.root_receiver); mark(e, heap, job.entry);
    for v in job.effect_args { mark(e, heap, v); }
    mark_frame(e, heap, job.effect_frame);
    for t in job.queue[job.queue_head..] { mark_object(e, t.target); if t.thenable != no_row() { mark_object(e, t.thenable); } mark(e, heap, t.value); }
    for r in job.rejections { mark_object(e, r); }
    for c in job.cell_rows { mark_range(e, c.row, c.count); }
    var i: i64 = 0;
    while i + 1 < job.foreign.len { mark_range(e, job.foreign[i], job.foreign[i + 1] - job.foreign[i]); i += 2; }
    for w in job.writes { mark(e, heap, w.key); if job.logged_values { mark(e, heap, w.value); } }
}

// Mapped parameter cells (light-cljs-867; source_vm.goose, "Mapped
// arguments objects"). While index i of an arguments object is mapped, the
// object's own property is the parameter's truth and its cell names the
// object, so a closure that kept one parameter would keep every argument,
// the map and the other cells. That link is weak here: trace_node lists a
// mapped cell above the floor instead of marking its object, and each
// listed cell's current value is marked in its place, which may reach the
// object after all (a parameter holding its own arguments object) or other
// cells, so marking repeats until no cell is left to resolve. An object
// still unmarked then is unreachable but through its cells: each of them
// becomes an ordinary binding holding the value it resolved (detach_cells,
// as unmapping leaves it), and the object, its map and whatever only they
// held are discarded. An object some root reaches keeps its cells mapped,
// and the value marked for them is its own property, which it reaches
// anyway. A cell below the floor is not the job's to change and marks its
// object as before. Only the collection pays: the VM's mapped paths are
// unchanged, and code without mapped objects lists no cells.
inline fn marked_node(e: const Evacuation&, r: i64) -> bool { return r < e.floor.nodes || e.nodes[r - e.floor.nodes] != 0; }
// Resolves the listed cells from `resolved` on; returns how many are.
fn mark_cells(e: Evacuation&, heap: Heap&, resolved: i64) -> i64 {
    var at = resolved;
    while at < e.cells.len {
        let c = e.cells[at]; at += 1;
        let o: i64 = node(heap, c).value.id();
        var v = Value.Undefined;
        if !marked_node(e, o) {
            v = mapped_value(heap, o, node(heap, c).length, c);
            if heap.fault != Fault.None { abort("reclamation could not read a mapped parameter"); }
            mark(e, heap, v);
        }
        e.kept_values.push(v);
    }
    return at;
}
fn detach_cells(e: const Evacuation&, heap: Heap&) {
    for c, k in e.cells { if !marked_node(e, node(heap, c).value.id()) { detach_cell(heap, c, e.kept_values[k]); } }
}
// After marking: entry i becomes the number of marked rows before floor + i,
// the extra last entry their total.
fn number(marks: u32[>..]&) {
    var total: u32 = 0;
    for i in marks.len - 1 { let m = marks[i]; marks[i] = total; total += m; }
    marks[marks.len - 1] = total;
}
// Marks the code units some reached string covers.
fn covered(cover: const i32[:], marks: u32[>..]&) {
    var depth: i64 = 0;
    for i in marks.len - 1 { depth += cover[i]; marks[i] = if depth > 0 { 1 } else { 0 }; }
}
inline fn kept(marks: const u32[:], floor: i64, r: i64) -> bool { return r < floor || marks[r - floor + 1] != marks[r - floor]; }
// A row's (or a run's start's, kept or not) place after the slide.
inline fn moved_row(marks: const u32[:], floor: i64, r: i64) -> i64 { return if r < floor { r } else { floor + marks[r - floor] }; }
// A link to a row the pass kept.
fn link(marks: const u32[:], floor: i64, r: i64) -> i64 {
    if !kept(marks, floor, r) { abort("reclamation reached a link to a row it did not keep"); }
    return moved_row(marks, floor, r);
}
fn node_link(e: const Evacuation&, r: u32) -> u32 { return link(e.nodes[..], e.floor.nodes, r) as u32; }
fn cont_link(e: const Evacuation&, r: u32) -> u32 { return if r == no_row() { r } else { link(e.conts[..], e.floor.conts, r) as u32 }; }
// The rewrite changes a row only where a link moves, so a row whose links
// all stay keeps its exact bytes (the worker reset compares image rows byte
// for byte, padding included). Each relink rewrites in place and returns
// whether anything changed.
fn reseat(x: u32&, to: i64) -> bool {
    if x == to { return false; }
    x = to as u32; return true;
}
fn relink(e: const Evacuation&, v: Value&) -> bool {
    match v {
        Object x => { if x.o >= e.floor.nodes { let r = node_link(e, x.o); if r != x.o { v = Value.Object { o: r }; return true; } } },
        String x => { if x.s >= e.floor.strings { let r = link(e.strings[..], e.floor.strings, x.s); if r != x.s { v = Value.String { s: r as u32 }; return true; } } },
        Accessor x => { if x.a >= e.floor.accessors { let r = link(e.accessors[..], e.floor.accessors, x.a); if r != x.a { v = Value.Accessor { a: r as u32 }; return true; } } },
        _ => {},
    }
    return false;
}
// A weak link: undefined when only it named the row.
fn relink_weak(e: const Evacuation&, v: Value&) {
    let dead = match v { Object x => !kept(e.nodes[..], e.floor.nodes, x.o), String x => !kept(e.strings[..], e.floor.strings, x.s), _ => false };
    if dead { v = Value.Undefined; } else { relink(e, v); }
}
fn relink(e: const Evacuation&, n: Node&) -> bool {
    var changed = relink(e, n.value);
    changed = relink(e, n.namespace_value) || changed; changed = relink(e, n.metadata) || changed; changed = relink(e, n.proto) || changed;
    for i in n.slots.len { changed = relink(e, n.slots[i]) || changed; }
    if snapshot::has_values(n.kind) { changed = reseat(n.start, moved_row(e.values[..], e.floor.values, n.start)) || changed; }
    if snapshot::shaped(n) { changed = reseat(n.keys, moved_row(e.values[..], e.floor.values, n.keys)) || changed; } else if snapshot::record_run(n) { changed = reseat(n.keys, moved_row(e.properties[..], e.floor.properties, n.keys)) || changed; }
    if n.kind == node_kind::Generator || n.kind == node_kind::Promise { changed = reseat(n.function, cont_link(e, n.function)) || changed; }
    return changed;
}
fn relink(e: const Evacuation&, p: Property&) -> bool { let a = relink(e, p.key); let b = relink(e, p.value); return a || b; }
fn relink(e: const Evacuation&, p: Pair&) -> bool { let a = relink(e, p.get); let b = relink(e, p.set); return a || b; }
fn relink(e: const Evacuation&, s: Str&) -> bool { return reseat(s.start, if s.wide { moved_row(e.units[..], e.floor.units, s.start) } else { moved_row(e.latin1[..], e.floor.latin1, s.start) }); }
fn relink(e: const Evacuation&, u: u16&) -> bool { return false; }
fn relink(e: const Evacuation&, f: SavedFrame&) -> bool {
    var changed = relink(e, f.callee); changed = relink(e, f.receiver) || changed; changed = relink(e, f.input) || changed; changed = relink(e, f.output) || changed; return changed;
}
fn relink(e: const Evacuation&, f: Frame&) -> bool {
    var changed = relink(e, f.callee); changed = relink(e, f.receiver) || changed; changed = relink(e, f.input) || changed; changed = relink(e, f.output) || changed; return changed;
}
fn relink(e: const Evacuation&, c: Cont&) -> bool {
    var changed = false;
    let m: Cont = match c {
        Activation x => {
            var a = x.a;
            if a.frame_capacity > 0 { changed = reseat(a.frames, cont_link(e, a.frames)) || changed; }
            if a.handler_capacity > 0 { changed = reseat(a.handlers, cont_link(e, a.handlers)) || changed; }
            changed = reseat(a.values, moved_row(e.values[..], e.floor.values, a.values)) || changed;
            Cont.Activation { a: a }
        },
        Saved x => { var h = x.h; changed = relink(e, h.value); Cont.Saved { h: h } },
        Promise x => {
            var p = x.p;
            changed = relink(e, p.result); changed = reseat(p.first, cont_link(e, p.first)) || changed; changed = reseat(p.last, cont_link(e, p.last)) || changed; changed = reseat(p.activation, cont_link(e, p.activation)) || changed;
            Cont.Promise { p: p }
        },
        Reaction x => { var r = x.r; changed = reseat(r.target, node_link(e, r.target)); changed = reseat(r.next, cont_link(e, r.next)) || changed; Cont.Reaction { r: r } },
        Frame x => { var f = x.f; changed = relink(e, f); Cont.Frame { f: f } },
    };
    if changed { c = m; }
    return changed;
}
// Rewrite rows from..floor in place and slide the kept rows above the floor
// down; returns how many rows the table lost.
fn slide<T>(e: const Evacuation&, rows: T[>..]&, marks: const u32[:], floor: i64, from: i64) -> i64 {
    let len = rows.len;
    for r in from..floor { relink(e, rows[r]); }
    for r in floor..len {
        let i = r - floor;
        if marks[i + 1] != marks[i] { let to = floor + marks[i]; if to != r { rows[to] = rows[r]; } relink(e, rows[to]); }
    }
    let count = floor + marks[len - floor];
    rows.resize(count); return len - count;
}
// The Latin-1 table slides the same way but keeps its bytes: what it no
// longer needs becomes its spare tail (Heap.latin1_spare), which add_latin1
// writes over.
fn slide_latin1(e: const Evacuation&, heap: Heap&) {
    let floor = e.floor.latin1; let len = latin1_len(heap);
    for r in floor..len { let i = r - floor; if e.latin1[i + 1] != e.latin1[i] { heap.latin1[floor + e.latin1[i]] = heap.latin1[r]; } }
    heap.latin1_spare += len - (floor + e.latin1[len - floor]);
}
// add_latin1 past a collection: over the spare tail first.
fn reuse_latin1(heap: Heap&, bytes: const u8[:]) -> i64 {
    let at = latin1_len(heap); let n = min(bytes.len, heap.latin1_spare);
    for i in n { heap.latin1[at + i] = bytes[i]; }
    heap.latin1_spare -= n;
    if n < bytes.len { heap.latin1.append(bytes[n..]); }
    return at;
}
fn reindex(e: const Evacuation&, heap: Heap&) {
    if heap.keys.len == 0 { return; }
    var old: KeySlot[>..] = [];
    for k in heap.keys { if k.row != no_row() { old.push(k); } }
    for i in heap.keys.len { heap.keys[i] = KeySlot {}; }
    var count: i64 = 0;
    for k in old {
        if !kept(e.strings[..], e.floor.strings, k.row) { continue; }
        index_slot(heap.keys, moved_row(e.strings[..], e.floor.strings, k.row) as u32, k.hash); count += 1;
    }
    heap.interned = count; heap.image_written = true;
}
fn rewrite_job(e: const Evacuation&, job: Job&) {
    let heap .= job.heap;
    relink(e, heap.global); relink(e, heap.call_method); relink(e, heap.pending);
    for i in heap.shapes.len { relink(e, heap.shapes[i].key); }
    for i in heap.constants.len { relink(e, heap.constants[i]); }
    for i in job.frames.len { relink(e, job.frames[i]); }
    for i in job.locals.len { relink(e, job.locals[i]); }
    for i in job.operands.len { relink(e, job.operands[i]); }
    for i in job.handlers.len { relink(e, job.handlers[i].value); }
    relink(e, job.result); relink(e, job.lexicals); relink(e, job.root_receiver); relink(e, job.entry);
    for i in job.effect_args.len { relink(e, job.effect_args[i]); }
    relink(e, job.effect_frame);
    for i in job.writes.len { relink_weak(e, job.writes[i].object); relink(e, job.writes[i].key); if job.logged_values { relink(e, job.writes[i].value); } else { relink_weak(e, job.writes[i].value); } }
    // The promise queue keeps its tasks from the head on.
    var tasks: Task[>..] = [];
    for t in job.queue[job.queue_head..] { var u = t; u.target = node_link(e, t.target); if t.thenable != no_row() { u.thenable = node_link(e, t.thenable); } relink(e, u.value); tasks.push(u); }
    job.queue.clear(); job.queue.append(tasks); job.queue_head = 0;
    for i in job.rejections.len { job.rejections[i] = node_link(e, job.rejections[i]); }
    for i in job.cell_rows.len { job.cell_rows[i].row = moved_row(e.nodes[..], e.floor.nodes, job.cell_rows[i].row); }
    for i in job.foreign.len { job.foreign[i] = moved_row(e.nodes[..], e.floor.nodes, job.foreign[i]) as u32; }
}
// The collection, at a safe point (run_claimed). Only the heap's one live
// job collects, and only a job that armed (start, resume_stack).
fn reclaim(job: Job&) {
    let heap .= job.heap; heap.reclaim = false;
    if !job.reclaimable || !job.registered || heap.live_jobs != 1 || heap.fault != Fault.None { return; }
    var e = Evacuation { floor: job.floor };
    // Rows below the write boundary are never the job's to move.
    e.floor.nodes = max(e.floor.nodes, max(job.owned_nodes, job.protected_nodes));
    let lengths = tops(heap);
    e.floor = Tops { nodes: min(e.floor.nodes, lengths.nodes), values: min(e.floor.values, lengths.values), properties: min(e.floor.properties, lengths.properties), units: min(e.floor.units, lengths.units), strings: min(e.floor.strings, lengths.strings), latin1: min(e.floor.latin1, lengths.latin1), shapes: lengths.shapes, accessors: min(e.floor.accessors, lengths.accessors), conts: min(e.floor.conts, lengths.conts) };
    e.nodes.resize(lengths.nodes - e.floor.nodes + 1, 0); e.values.resize(lengths.values - e.floor.values + 1, 0); e.properties.resize(lengths.properties - e.floor.properties + 1, 0);
    e.strings.resize(lengths.strings - e.floor.strings + 1, 0); e.units.resize(lengths.units - e.floor.units + 1, 0); e.latin1.resize(lengths.latin1 - e.floor.latin1 + 1, 0);
    e.accessors.resize(lengths.accessors - e.floor.accessors + 1, 0); e.conts.resize(lengths.conts - e.floor.conts + 1, 0);
    e.wide.resize(e.units.len, 0); e.narrow.resize(e.latin1.len, 0);
    mark_roots(e, job);
    var resolved: i64 = 0;
    while true {
        while e.work.len > 0 || e.pending.len > 0 {
            while e.work.len > 0 { let o = e.work.pop(); trace_node(e, heap, o); }
            while e.pending.len > 0 { let r = e.pending.pop(); trace_cont(e, heap, r); }
        }
        if resolved == e.cells.len { break; }
        resolved = mark_cells(e, heap, resolved);
    }
    detach_cells(e, heap);
    covered(e.wide[..], e.units); covered(e.narrow[..], e.latin1);
    number(e.nodes); number(e.values); number(e.properties); number(e.strings); number(e.units); number(e.latin1); number(e.accessors); number(e.conts);
    // Every table keeps its rows' order, so a run whose rows are all kept
    // stays a run and an earlier row stays earlier.
    slide(e, heap.nodes, e.nodes[..], e.floor.nodes, 0);
    slide(e, heap.values, e.values[..], e.floor.values, 0);
    heap.property_rows -= slide(e, heap.properties, e.properties[..], e.floor.properties, 0);
    slide(e, heap.accessors, e.accessors[..], e.floor.accessors, 0);
    slide(e, heap.conts, e.conts[..], e.floor.conts, 0);
    slide(e, heap.strings, e.strings[..], e.floor.strings, e.floor.strings);
    slide(e, heap.units, e.units[..], e.floor.units, e.floor.units);
    slide_latin1(e, heap);
    reindex(e, heap);
    rewrite_job(e, job);
    heap.reclaims += 1;
    set_margins(heap);
}
