import std;
import .source_vm;
import .realm;
import .snapshot;
import .form_bridge;
import .cell_owner;

namespace source;

// Atoms, volatiles and process-wide cells (light-cljs-799; decision 3 of
// docs/javascript-coverage.md, "Cells"; the owner is cell_owner.goose).
//
// A job's own atom or volatile is a row: a node of kind 17 whose `value` is
// its state and whose `length` is its flavor (0 atom, 1 volatile). It is
// reclaimed with the job's other rows, and its operations follow the V8
// build's runtime (src/js_runtime.js, `swapState`, `resetAtom`): swap! calls
// its function once and stores the result.
//
// Publishing a value (a prepared definition, a produced helper) turns every
// atom or volatile it reaches into a cell (publish_cells): the owner makes
// the cell and holds its state's flat encoding, and the row becomes a cell
// reference, kind 18, whose `function` is the cell and `length` its flavor.
// Every job that installs the publication holds the reference; its
// operations are messages to the owner, made while the job runs (effects of
// the job: a job that faults afterwards leaves them in place, as shadow-cljs
// does). deref loads, reset! and vreset! store, compare-and-set! compares by
// `=` (cljs.core) and swap! runs its function on the value it loaded and
// stores the result if the cell still holds that value's identity, rerunning
// the function only when another write replaced it, as Clojure's Atom.swap.
//
// A value stored into a cell is a primitive or a form graph: forms (lists,
// vectors, maps, sets, symbols, keywords and scalar occurrences), atoms (each
// becomes a cell first) and cell references. Anything else, a mutable
// JavaScript object or array above all, fails the operation with a named
// error before anything changes; so do functions (not yet shareable), watches
// and validators. Job.cell_rows records the cell and identity of every row a
// job loaded or stored, so the job imports each cell and each identity once.

inline fn atom_kind() -> i64 { return 17; }
inline fn cell_kind() -> i64 { return 18; }
// The flavor names built-ins use in their errors.
fn flavor_name(flavor: i64) -> const u8[:] { return if flavor == 0 { "an atom" } else { "a volatile" }; }
// A named failure the macro cannot catch: NativeBoundary, `name: what`.
fn cell_fail(job: Job&, b: Builtin, what: const u8[:]) -> Value {
    job.detail.clear();
    for c in BUILTINS[code(b)].name { job.detail.push(c); }
    for c in ": " { job.detail.push(c); }
    for c in what { job.detail.push(c); }
    if job.heap.fault == Fault.None { job.heap.fault = Fault.NativeBoundary; }
    return Value.Undefined;
}
// An owner error as the operation's named failure.
fn owner_fail(job: Job&, b: Builtin, error: i64) -> Value {
    return cell_fail(job, b, match error { 1 => "cell budget", 2 => "no such cell", 3 => "the session is halting", _ => "no cell owner in this program" });
}
// The flavor of reference `a` (an atom, volatile or cell), or -1.
fn reference_flavor(heap: Heap&, a: Value) -> i64 {
    if !valid(heap, a) || a.tag() != 5 { return -1; }
    let n = node(heap, a.id());
    return if n.kind == atom_kind() || n.kind == cell_kind() { n.length } else { -1 };
}
// The cell and identity a job knows for row `row`.
fn known(rows: const CellRow[:], row: i64) -> CellRow {
    for r in rows { if r.row == row { return r; } }
    return CellRow { row: row };
}
fn remember(rows: CellRow[>..]&, row: i64, cell: i64, identity: i64) {
    for r, i in rows { if r.row == row { if cell >= 0 { rows[i].cell = cell; } if identity > 0 { rows[i].identity = identity; } return; } }
    rows.push(CellRow { row: row, cell: cell, identity: identity });
}

// Whether a value can be stored in a cell: what the walk from `roots`
// reaches is primitives (no symbols), forms, atoms (collected in `atoms`,
// with their states) and cell references. Otherwise the description of the
// first value that is not shareable.
fn unshareable(heap: Heap&, roots: const Value[:], atoms: i64[>..]&) -> const u8[:] {
    var marks: bool[>..]   = []; marks.resize(heap.nodes.len, false);
    var work:  Value[>..<] = []; for v in roots { work.push(v); }
    while work.len > 0 {
        let v = work.pop();
        if !valid(heap, v) { heap.fault = Fault.None; return "a value this job cannot read"; }
        if v.tag() == 6 || v.tag() == 7 { return "a symbol or bigint value"; }
        if v.tag() != 5 || marks[v.id()] { continue; }
        marks[v.id()] = true; let n = node(heap, v.id());
        if !charge(heap) { return "a value past the work limit"; }
        if n.metadata.tag() == 5 { work.push(n.metadata); }
        if n.kind >= 5 && n.kind <= 8 || n.kind == 12 { for i in n.count { work.push(slot(heap, n.start + i)); } }
        else if n.kind == atom_kind() { atoms.push(v.id()); work.push(n.value); }
        else if n.kind == 2 || n.kind == 4 { return "a function (functions are not shareable yet)"; }
        else if n.kind == 15 { return "an unrealized lazy sequence"; }
        else if n.kind != 9 && n.kind != 10 && n.kind != 11 && n.kind != cell_kind() { return "a mutable JavaScript object"; }
    }
    return "";
}
// Make each atom of `atoms` a cell: the owner makes it, the row becomes its
// reference, and then each state is stored (so a state may reach any of
// them, itself included). False with the owner's answer on a failure.
fn make_cells(heap: Heap&, rows: CellRow[>..]&, atoms: const i64[:], failure: i64&) -> bool {
    // A new cell holds undefined until its state is stored.
    var states: Value[>..] = []; var made: i64[>..] = [];
    for o in atoms {
        let n = node(heap, o);
        if n.kind != atom_kind() { states.push(Value.Undefined); made.push(-1); continue; }
        let a = cells::exchange(cells::Request { op: 1, flavor: n.length });
        if !a.ok { failure = a.error; return false; }
        states.push(n.value); made.push(a.cell);
        var r = n; r.kind = cell_kind(); r.function = a.cell as u32; r.value = Value.Undefined; set_node(heap, o, r);
        remember(rows, o, a.cell, 0);
    }
    for o, i in atoms {
        if made[i] < 0 || states[i].tag() == 0 { continue; }
        let f, ok = encode(heap, rows, states[i], failure);
        if !ok { return false; }
        let a = cells::exchange(cells::Request { op: 3, cell: made[i], value: copy(f) });
        if !a.ok { failure = a.error; return false; }
        if states[i].tag() == 5 && a.value.kind == 5 { remember(rows, states[i].id(), -1, a.value.identity); }
    }
    return true;
}
// The flat encoding of a shareable value whose atoms are cells already.
fn encode(heap: Heap&, rows: const CellRow[:], v: Value, failure: i64&) -> cells::Flat, bool {
    let t = v.tag();
    if t == 0 { return cells::Flat {}, true; }
    if t == 1 { return cells::Flat { kind: 1 }, true; }
    if t == 2 { return cells::Flat { kind: 2, number: if v.flag() { 1.0 } else { 0.0 } }, true; }
    if t == 3 { return cells::Flat { kind: 3, number: v.num() }, true; }
    if t == 4 {
        var bytes: u8[>..] = []; for u in utf16(heap, v) { bytes.push((u & 255) as u8); bytes.push((u >> 8) as u8); }
        return cells::Flat { kind: 4, data: bytes }, true;
    }
    var graph = bridge::Graph {}; var c = bridge::Control { cells: true }; var bytes: u8[>..] = [];
    if t != 5 || !bridge::export_graph(heap, [v], graph, c) { failure = 5; return cells::Flat {}, false; }
    // A value outlives the call it was made in: the compiler occurrences and
    // source spans of that call's forms mean nothing to a later job, which
    // sees the value as one built afresh, as the V8 build's form bridge sees
    // an object it did not hand out.
    for n, i in graph.nodes { graph.nodes[i].origin = -1; graph.nodes[i].source = -1; graph.nodes[i].span_start = 0; graph.nodes[i].span_end = 0; }
    graph.sources.clear(); graph.bytes.clear();
    if !bridge::encode(graph, bytes, c) { failure = 5; return cells::Flat {}, false; }
    return cells::Flat { kind: 5, identity: known(rows, v.id()).identity, data: bytes }, true;
}
// The value a job stores: checked, its atoms made cells, then encoded. A
// failure is the operation's named error and changes nothing but the cells
// already made for its atoms.
fn store_value(job: Job&, b: Builtin, v: Value) -> cells::Flat, bool {
    let heap .= job.heap;
    var atoms: i64[>..] = [];
    let bad = unshareable(heap, [v], atoms);
    if heap.fault != Fault.None { return cells::Flat {}, false; }
    if bad.len > 0 { var what: u8[>..] = []; for c in "cannot share " { what.push(c); } what.append(bad); cell_fail(job, b, what); return cells::Flat {}, false; }
    var failure: i64 = 0;
    if atoms.len > 0 && !make_cells(heap, job.cell_rows, atoms, failure) { owner_or_bridge(job, b, failure); return cells::Flat {}, false; }
    let f, ok = encode(heap, job.cell_rows, v, failure);
    if !ok { owner_or_bridge(job, b, failure); return cells::Flat {}, false; }
    return f, true;
}
fn owner_or_bridge(job: Job&, b: Builtin, failure: i64) { if failure == 5 { cell_fail(job, b, "the value does not encode"); } else { owner_fail(job, b, failure); } }
// A loaded value in the job: a primitive, or the row of its identity,
// imported once.
fn decode(job: Job&, b: Builtin, f: const cells::Flat&) -> Value {
    let heap .= job.heap;
    match f.kind {
        0 => { return Value.Undefined; }, 1 => { return Value.Null; }, 2 => { return boolean(f.number != 0); }, 3 => { return number(f.number); },
        4 => { var units: u16[>..] = []; var i: i64 = 0; while i + 1 < f.data.len { units.push(f.data[i] | (f.data[i + 1] << 8)); i += 2; } return string(heap, units); },
        _ => {},
    }
    for r in job.cell_rows { if r.identity == f.identity && f.identity > 0 { return object(r.row); } }
    var graph = bridge::Graph {}; var c = bridge::Control { cells: true }; var roots: Value[>..] = [];
    if !bridge::decode(graph, f.data, c) || !import_cells(heap, job.cell_rows, graph, roots, c) || roots.len != 1 {
        if heap.fault == Fault.None { return cell_fail(job, b, "a cell value does not import"); }
        return Value.Undefined;
    }
    for v in roots { if f.identity > 0 { remember(job.cell_rows, v.id(), -1, f.identity); } return v; }
    return Value.Undefined;
}
// Import a graph that may hold cell references (tag 10): a cell the rows
// already name keeps its row, so every reference to one cell in a job is one
// object, and a new one is remembered.
fn import_cells(heap: Heap&, rows: CellRow[>..]&, g: const bridge::Graph&, roots: Value[>..]&, c: bridge::Control&) -> bool {
    let base = heap.nodes.len; let children = heap.values.len; let first = roots.len;
    if !bridge::import_graph(heap, g, roots, c) { return false; }
    for n, i in g.nodes {
        if n.tag != 10 { continue; }
        let row = base + i; var prior: i64 = -1;
        for r in rows { if r.cell == n.number as i64 && r.row != row { prior = r.row; break; } }
        if prior < 0 { remember(rows, row, n.number as i64, 0); continue; }
        for k in children..(children + g.children.len) { if heap.values[k] == object(row) { heap.values[k] = object(prior); } }
        for k in first..roots.len { if roots[k] == object(row) { roots[k] = object(prior); } }
    }
    return true;
}
// The expected value of a compare-and-set: what the job loaded, by identity
// for a graph, by value for a primitive.
fn expected(job: Job&, v: Value) -> cells::Flat {
    let heap .= job.heap;
    if v.tag() == 5 { return cells::Flat { kind: 5, identity: known(job.cell_rows, v.id()).identity }; }
    var failure: i64 = 0; let f, ok = encode(heap, job.cell_rows, v, failure);
    return f;
}
// Publication (the preparer, a helper production): make a cell of every atom
// `root` reaches through any link, the image's rows (below `floor`) aside.
// "" or the reason the publication fails.
fn publish_cells(heap: Heap&, root: Value, floor: i64) -> u8[] {
    var marks: bool[>..] = []; marks.resize(heap.nodes.len, false);
    var work:  i64[>..<] = []; var atoms: i64[>..] = [];
    if root.tag() == 5 && root.id() < heap.nodes.len { work.push(root.id()); marks[root.id()] = true; }
    while work.len > 0 {
        let o = work.pop(); let n = node(heap, o);
        if n.kind == atom_kind() { atoms.push(o); }
        var edges: Value[>..] = [n.value, n.namespace_value, n.metadata, n.proto];
        if snapshot::has_values(n.kind) { for i in n.count { edges.push(slot(heap, n.start + i)); } }
        if snapshot::shaped(n) {
            for v, i in n.slots { if i < n.key_count { edges.push(v); } }
            for row in n.keys..(n.keys + overflow(n)) { edges.push(slot(heap, row)); }
        } else if snapshot::record_run(n) { for p in n.keys..(n.keys + n.key_count) { edges.push(property_row(heap, p).value); } }
        var accessors: Value[>..] = [];
        for e in edges { if e.tag() == -2 { let p = pair(heap, e); accessors.push(p.get); accessors.push(p.set); } }
        edges.append(accessors);
        for v in edges {
            if v.tag() == 5 && v.id() >= floor && v.id() < heap.nodes.len && !marks[v.id()] { marks[v.id()] = true; work.push(v.id()); }
        }
    }
    if atoms.len == 0 { return []; }
    // Each atom's state must itself be shareable.
    var nested: i64[>..] = [];
    for o in atoms {
        let bad = unshareable(heap, [node(heap, o).value], nested);
        if bad.len > 0 { return str("cell-unshareable ", bad); }
    }
    var rows: CellRow[>..] = []; var failure: i64 = 0;
    if !make_cells(heap, rows, atoms, failure) { return str("cell-error ", failure); }
    for o in nested { if node(heap, o).kind == atom_kind() { return str("cell-error 5"); } }
    return [];
}

// The built-ins (realm.goose Builtin): cljs.core's atom, deref, reset!,
// reset-vals!, compare-and-set!, volatile!, vreset! and the watch and
// validator functions, which fail by name. swap! and swap-vals! run user code
// and are native frames (swap_step).
fn cell_builtin(job: Job&, b: Builtin, args: const Value[:]) -> Value {
    let heap .= job.heap;
    match b {
        Atom, Volatile => {
            if args.len > 1 { return cell_fail(job, b, "options (metadata and validators) are not supported"); }
            return allocate(heap, Node { kind: atom_kind(), length: if b == Builtin.Atom { 0 } else { 1 }, value: arg(args, 0) });
        },
        AddWatch, RemoveWatch, SetValidator => { return cell_fail(job, b, "watches and validators are not supported"); },
        Deref => {
            let a = arg(args, 0); let flavor = reference_flavor(heap, a);
            if heap.fault != Fault.None { return Value.Undefined; }
            if flavor < 0 { return unsupported_step(job, b, "a value that is not an atom or volatile"); }
            return load(job, b, a);
        },
        Reset, Vreset, ResetVals => {
            let a = arg(args, 0); let v = arg(args, 1); let flavor = reference_flavor(heap, a);
            if heap.fault != Fault.None { return Value.Undefined; }
            if flavor != (if b == Builtin.Vreset { 1 } else { 0 }) { return type_error(job, if b == Builtin.Vreset { "vreset! needs a volatile" } else { "reset! needs an atom" }); }
            if node(heap, a.id()).kind == atom_kind() {
                if !writable(job, a.id()) { heap.fault = Fault.NativeBoundary; return Value.Undefined; }
                let old = node(heap, a.id()).value; node_mut(heap, a.id()).value = v;
                return if b == Builtin.ResetVals { form_collection(heap, 6, [old, v]) } else { v };
            }
            let f, ok = store_value(job, b, v); if !ok { return Value.Undefined; }
            let cell: i64 = node(heap, a.id()).function;
            if b != Builtin.ResetVals {
                let r = cells::exchange(cells::Request { op: 3, cell: cell, value: copy(f) });
                if !r.ok { return owner_fail(job, b, r.error); }
                if v.tag() == 5 { remember(job.cell_rows, v.id(), -1, r.value.identity); }
                return v;
            }
            // reset-vals!: the value replaced, atomically.
            loop {
                let current = cells::exchange(cells::Request { op: 2, cell: cell });
                if !current.ok { return owner_fail(job, b, current.error); }
                let old = decode(job, b, current.value); if heap.fault != Fault.None { return Value.Undefined; }
                let r = cells::exchange(cells::Request { op: 4, cell: cell, expected: expected(job, old), value: copy(f) });
                if r.ok { if v.tag() == 5 { remember(job.cell_rows, v.id(), -1, r.value.identity); } return form_collection(heap, 6, [old, v]); }
                if r.error != 0 { return owner_fail(job, b, r.error); }
            }
            return Value.Undefined;
        },
        CompareAndSet => {
            let a = arg(args, 0); let old = arg(args, 1); let v = arg(args, 2);
            if reference_flavor(heap, a) != 0 { if heap.fault == Fault.None { type_error(job, "compare-and-set! needs an atom"); } return Value.Undefined; }
            if node(heap, a.id()).kind == atom_kind() {
                let same = form_equal(heap, node(heap, a.id()).value, old); if heap.fault != Fault.None { return Value.Undefined; }
                if !same { return boolean(false); }
                if !writable(job, a.id()) { heap.fault = Fault.NativeBoundary; return Value.Undefined; }
                node_mut(heap, a.id()).value = v; return boolean(true);
            }
            let f, ok = store_value(job, b, v); if !ok { return Value.Undefined; }
            let cell: i64 = node(heap, a.id()).function;
            loop {
                let current = cells::exchange(cells::Request { op: 2, cell: cell });
                if !current.ok { return owner_fail(job, b, current.error); }
                let held = decode(job, b, current.value); if heap.fault != Fault.None { return Value.Undefined; }
                let same = form_equal(heap, held, old); if heap.fault != Fault.None { return Value.Undefined; }
                if !same { return boolean(false); }
                let r = cells::exchange(cells::Request { op: 4, cell: cell, expected: expected(job, held), value: copy(f) });
                if r.ok { if v.tag() == 5 { remember(job.cell_rows, v.id(), -1, r.value.identity); } return boolean(true); }
                if r.error != 0 { return owner_fail(job, b, r.error); }
            }
            return Value.Undefined;
        },
        _ => { heap.fault = Fault.InvalidState; return Value.Undefined; },
    }
}
// deref of an atom, volatile or cell.
fn load(job: Job&, b: Builtin, a: Value) -> Value {
    let n = node(job.heap, a.id());
    if n.kind == atom_kind() { return n.value; }
    let r = cells::exchange(cells::Request { op: 2, cell: n.function });
    if !r.ok { return owner_fail(job, b, r.error); }
    return decode(job, b, r.value);
}

// swap! and swap-vals! (Native.Swap, its Builtin the payload): a native
// frame whose fields hold the operation, so a function that suspends the
// job (a helper dependency) resumes it in any worker.
//   input     the atom or cell     callee    the function
//   output    the value it read    argument_count  extra arguments, held at
//             the frame's operand base
//   pc 0      reads the value and calls the function
//   pc 1      awaits the function's result
// A job's own atom stores the result, as the V8 build's swap! does. A cell
// compare-and-sets it against the identity of the value read; when another
// write replaced that value, the frame reads again and calls the function
// again.
fn start_swap(job: Job&, b: Builtin, args: const Value[:], base: i64) {
    let heap .= job.heap;
    let a = arg(args, 0); let f = arg(args, 1);
    if reference_flavor(heap, a) != 0 { if heap.fault == Fault.None { type_error(job, "swap! needs an atom"); } return; }
    if !callable(heap, f) { type_error(job, "not a function"); return; }
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
    var extra: Value[>..] = []; if args.len > 2 { extra.append(args[2..]); }
    job.operands.resize(base); job.operands.append(extra);
    job.frames.push(Frame { function: -1, pc: 0, local_base: job.locals.len, operand_base: base, callee: f, receiver: Value.Undefined, input: a, argument_count: extra.len, native: Native.Swap { which: b } });
}
fn swap_step(job: Job&, frame: Frame, b: Builtin) {
    let heap .= job.heap; let at = job.frames.len - 1;
    var r = Value.Undefined;
    if frame.pc == 0 {
        let old = load(job, b, frame.input); if heap.fault != Fault.None { return; }
        job.frames[at].output = old;
        let value, done = swap_call(job, at, b); if !done { return; } r = value;
    } else { r = job.operands.pop(); }
    loop {
        if heap.fault != Fault.None || swap_commit(job, at, b, r) { return; }
        let value, done = swap_call(job, at, b); if !done { return; } r = value;
    }
}
// Call the frame's function with the value read and the extra arguments: a
// plain built-in runs here (true with its result), a closure gets a frame
// above this one (false; its result comes back at pc 1).
fn swap_call(job: Job&, at: i64, b: Builtin) -> Value, bool {
    let heap .= job.heap; let frame = job.frames[at];
    var args: Value[>..] = [frame.output]; args.append(job.operands[frame.operand_base..(frame.operand_base + frame.argument_count)]);
    if node(heap, frame.callee.id()).kind == 4 {
        let cb = builtin_of(heap, frame.callee.id());
        if effect_builtin(cb) || cb == Builtin.Mapv || cb == Builtin.MapSeq || native_builtin(cb) { unsupported_step(job, b, "a host-effect or native-frame built-in function"); return Value.Undefined, false; }
        return call_builtin(job, node(heap, frame.callee.id()).function, Value.Undefined, args), true;
    }
    job.frames[at].pc = 1;
    push_frame(job, frame.callee, Value.Undefined, args, false, job.operands.len);
    return Value.Undefined, false;
}
// Store result r: true when the frame finished (or failed), false when the
// cell held another value, which the frame now holds as the value read.
fn swap_commit(job: Job&, at: i64, b: Builtin, r: Value) -> bool {
    let heap .= job.heap; let frame = job.frames[at]; let a = frame.input;
    if node(heap, a.id()).kind == atom_kind() {
        if !writable(job, a.id()) { heap.fault = Fault.NativeBoundary; return true; }
        node_mut(heap, a.id()).value = r;
    } else {
        let f, ok = store_value(job, b, r); if !ok { return true; }
        let answer = cells::exchange(cells::Request { op: 4, cell: node(heap, a.id()).function, expected: expected(job, frame.output), value: copy(f) });
        if !answer.ok && answer.error != 0 { owner_fail(job, b, answer.error); return true; }
        if !answer.ok {
            let current = decode(job, b, answer.value); if heap.fault != Fault.None { return true; }
            job.frames[at].output = current; return false;
        }
        if r.tag() == 5 { remember(job.cell_rows, r.id(), -1, answer.value.identity); }
    }
    let result = if b == Builtin.SwapVals { form_collection(heap, 6, [frame.output, r]) } else { r };
    if heap.fault == Fault.None { finish_native(job, frame, result); }
    return true;
}
