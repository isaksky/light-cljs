import std;
import .source_vm;
import .realm;
import .snapshot;
import .form_bridge;
import .cell_owner;
import .callable;

namespace source;

// Atoms, volatiles, delays and process-wide cells (light-cljs-799,
// light-cljs-808; decision 3 of docs/javascript-coverage.md, "Cells"; the
// owner is cell_owner.goose).
//
// A job's own atom or volatile is a row: node_kind::Atom whose `value` is
// its state and whose `length` is its flavor (cell_flavor::Atom or Volatile). A job's
// own delay is node_kind::Delay: `value` its thunk, pending while truthy,
// and `namespace_value` the value it realized. They are reclaimed with the
// job's other rows, and their operations follow the V8 build's runtime
// (src/js_runtime.js, `swapState`, `resetAtom`, `deferredValue`): swap!
// calls its function once and stores the result; a delay's force calls its
// thunk and keeps the value, and a thunk that throws stays, so a later force
// calls it again.
//
// Publishing a value (a prepared definition, a produced helper) turns every
// atom, volatile or delay it reaches into a cell (publish_cells): the owner
// makes the cell and holds its state's flat encoding, and the row becomes a
// cell reference, node_kind::Cell, whose `function` is the cell and `length` its
// flavor (cell_flavor::Delay for a delay, whose `value` keeps the thunk). Every job that
// installs the publication holds the reference; its operations are messages
// to the owner, made while the job runs (effects of the job: a job that
// faults afterwards leaves them in place, as shadow-cljs does). deref loads,
// reset! and vreset! store, compare-and-set! compares by `=` (cljs.core) and
// swap! runs its function on the value it loaded and stores the result if
// the cell still holds that value's identity, rerunning the function only
// when another write replaced it, as Clojure's Atom.swap. A delay cell's
// first force claims it at the owner and runs its thunk in the claiming job;
// a force while another job holds the claim waits (the coordinator resumes
// it when the claim ends) and then takes the value, or the error the thunk
// threw, as Clojure's Delay; a cycle of claims fails by name.
//
// A value stored into a cell is a primitive, a form graph (forms: lists,
// vectors, maps, sets, symbols, keywords and scalar occurrences; atoms, each
// a cell first; and cell references) or, when it reaches a function or a
// job's own delay, a callable graph (light-cljs-809): published as
// callable::encode publishes a helper, its reachable immutable captures and
// forms with cells for its atoms and delays, and installed in a loading job
// as callable::decode_install installs one, once per identity. A built-in
// or another function of the image is its row. Anything else, a mutable
// JavaScript object or array above all, a function with own properties or
// one capturing a variable it may assign, fails the operation with a named
// error before anything changes; so do watches and validators. An error a
// delay's thunk threw is kept as its type and message.
//
// Identity (light-cljs-808). Every object of a stored graph has a
// process-wide identity: the owner numbers one when the object is first
// stored, and an object of a publication has one from the moment it leaves
// its job, named by the publication and its place in it
// (publication_identity), so a publication needs no owner message.
// Job.cell_rows records the cell or identity of every row a job loaded,
// stored or installed (a publication's rows as one range), and every path
// that brings rows into a job (a loaded cell value, an installed helper, a
// prepared definition) maps an object whose identity or cell the job already
// holds to the row it holds, so `identical?` answers what one shared heap
// would.

// The header of the session's image (the coordinator's base snapshot,
// which every worker and preparer inherits), against which a function in a
// cell is published and installed (light-cljs-809). Set by the coordinator
// before it spawns any program; `base` 0 elsewhere, where a function
// stored into a cell fails as any cell operation does without an owner.
var cell_base: snapshot::Header = snapshot::Header {};
// The flavor names built-ins use in their errors.
fn flavor_name(flavor: i64) -> const u8[:] { return if flavor == cell_flavor::Atom { "an atom" } else if flavor == cell_flavor::Volatile { "a volatile" } else { "a delay" }; }
// A named failure the macro cannot catch: NativeBoundary, `name: what`.
fn cell_fail(job: Job&, b: Builtin, what: const u8[:]) -> Value {
    job.detail.clear();
    for c in BUILTINS[code(b)].name { job.detail.push(c); }
    for c in ": " { job.detail.push(c); }
    for c in what { job.detail.push(c); }
    if job.heap.fault == Fault.None { job.heap.fault = Fault.NativeBoundary; }
    return Value.Undefined;
}
// cljs.core's Error for a reference operation on a value that does not
// support it (light-cljs-873), in the V8 build's runtime's words: a
// catchable HostEffect fault (realm.goose runtime_error). The operations
// that deref first (compare-and-set!, reset-vals!, swap-vals!,
// set-validator!) fail as deref does on a value that is no reference; on a
// delay, which that deref would force, they fail by name.
fn reference_error(job: Job&, message: const u8[:]) -> Value { let none: u16[0] = []; return runtime_error(job, message, none); }
fn not_derefable(job: Job&) -> Value { return reference_error(job, "value does not support deref"); }
fn derefs_first(job: Job&, b: Builtin, a: Value, protocol: const u8[:]) -> Value {
    if is_delay(job.heap, a) { return cell_fail(job, b, "a delay, which it would force before it fails"); }
    return if reference_flavor(job.heap, a) < 0 { not_derefable(job) } else { reference_error(job, protocol) };
}
fn no_reset() -> const u8[:] { return "No protocol method IReset/-reset! defined for this value"; }
fn no_swap() -> const u8[:] { return "No protocol method ISwap/-swap! defined for this value"; }
// An owner error as the operation's named failure.
fn owner_fail(job: Job&, b: Builtin, error: i64) -> Value {
    return cell_fail(job, b, match error { cell_error::Budget => "cell budget", cell_error::NoCell => "no such cell", cell_error::Halting => "the session is halting", cell_error::Cycle => "a cycle of delay claims", cell_error::NotClaim => "not a delay claim this job holds", _ => "no cell owner in this program" });
}
// The flavor of reference `a` (an atom, volatile or cell), or -1.
fn reference_flavor(heap: Heap&, a: Value) -> i64 {
    if !valid(heap, a) || !a.is_object() { return -1; }
    let n = node(heap, a.id());
    return if n.kind == node_kind::Atom || n.kind == node_kind::Cell { n.length } else { -1 };
}
// Whether `v` is a delay: a job's own or a delay cell's reference.
fn is_delay(heap: Heap&, v: Value) -> bool {
    if !valid(heap, v) { heap.fault = Fault.None; return false; }
    if !v.is_object() { return false; }
    let n = node(heap, v.id());
    return n.kind == node_kind::Delay || n.kind == node_kind::Cell && n.length == cell_flavor::Delay;
}

// Process-wide identities of publications' objects: the object at place
// `index` of publication `publication` (helper_publication of a helper's
// unit, definition_publication of a definition's key) has this one. Bit 62
// keeps them apart from the owner's numbers.
fn publication_identity(publication: i64, index: i64) -> i64 { return 4611686018427387904 + publication * 16777216 + index; }
fn helper_publication(key: i64) -> i64 { return if key >= 0 && key < 68719476736 { key * 2 } else { -1 }; }
fn definition_publication(key: i64) -> i64 { return if key >= 0 && key < 68719476736 { key * 2 + 1 } else { -1 }; }
// A cell reference's identity in a stored graph: its cell's, so two
// references to one cell are one value (bit 61, apart from the owner's
// numbers and publication_identity).
fn cell_identity(cell: i64) -> i64 { return 2305843009213693952 + cell; }
fn is_cell_identity(identity: i64) -> bool { return identity >= 2305843009213693952 && identity < 4611686018427387904; }
// The identity the job knows for row `row`, or 0.
fn identity_of(rows: const CellRow[:], row: i64) -> i64 {
    for r in rows {
        if r.count == 1 { if r.row == row && r.identity > 0 { return r.identity; } }
        else if row >= r.row && row < r.row + r.count { return r.identity + (row - r.row); }
    }
    return 0;
}
// The row the job holds for `identity`, or -1. Earlier entries come first,
// so a row the job held before an import keeps its place.
fn row_of(rows: const CellRow[:], identity: i64) -> i64 {
    if identity <= 0 { return -1; }
    for r in rows {
        if r.count == 1 { if r.identity == identity { return r.row; } }
        else if identity >= r.identity && identity < r.identity + r.count { return r.row + (identity - r.identity); }
    }
    return -1;
}
// The row the job holds for cell `cell`, or -1.
fn cell_row(rows: const CellRow[:], cell: i64) -> i64 {
    for r in rows { if r.count == 1 && r.cell == cell { return r.row; } }
    return -1;
}
fn remember(rows: CellRow[>..]&, row: i64, cell: i64, identity: i64) {
    for r, i in rows { if r.count == 1 && r.row == row { if cell >= 0 { rows[i].cell = cell; } if identity > 0 { rows[i].identity = identity; } return; } }
    rows.push(CellRow { row: row, cell: cell, identity: identity });
}
// After a store: the identities the owner gave the stored graph's objects
// (`nodes`, by graph node) for the rows they came from (`stored`).
fn remember_stored(rows: CellRow[>..]&, stored: const i64[:], nodes: const i64[:]) {
    for row, i in stored { if row >= 0 && i < nodes.len && nodes[i] > 0 && !is_cell_identity(nodes[i]) && identity_of(rows, row) == 0 { rows.push(CellRow { row: row, identity: nodes[i] }); } }
}

// Whether a value can be stored in a cell: "" when it can, otherwise the
// description of the first value that is not shareable. A value that
// reaches no function or delay of the job must be a form graph
// (form_unshareable); one that does goes by the callable transport
// (callable_unshareable, light-cljs-809). `atoms` gets the atoms (and job
// delays) to make cells of, `closures` the job's closures it reaches.
fn unshareable(heap: Heap&, roots: const Value[:], atoms: i64[>..]&) -> const u8[:] { var closures: i64[>..] = []; return unshareable(heap, roots, atoms, closures); }
fn unshareable(heap: Heap&, roots: const Value[:], atoms: i64[>..]&, closures: i64[>..]&) -> const u8[:] {
    var functions = false; let bad = form_unshareable(heap, roots, atoms, functions);
    if !functions { return bad; }
    atoms.clear(); return callable_unshareable(heap, roots, atoms, closures);
}
// A stored value's closures are shared now, so the storing job may no more
// write them than an installed helper's (light-cljs-809): a later write
// faults NativeBoundary before it mutates, and no job sees a function its
// cell's encoding does not hold.
fn protect(job: Job&, closures: const i64[:]) {
    for row in closures { if writable(job, row) { job.foreign.push(row as u32); job.foreign.push((row + 1) as u32); } }
}
// Whether `v` reaches a function or a delay of the job, and so travels by
// the callable transport.
fn reaches_function(heap: Heap&, v: Value) -> bool { var atoms: i64[>..] = []; var functions = false; form_unshareable(heap, [v], atoms, functions); return functions; }
// The form walk: what `roots` reaches is primitives (no symbols), forms,
// atoms (collected in `atoms`, with their states) and cell references.
// Reaching a function or a delay of the job sets `functions` and stops.
fn form_unshareable(heap: Heap&, roots: const Value[:], atoms: i64[>..]&, functions: bool&) -> const u8[:] {
    var marks: bool[>..]   = []; marks.resize(heap.nodes.len, false);
    var work:  Value[>..<] = []; for v in roots { work.push(v); }
    while work.len > 0 {
        let v = work.pop();
        if !valid(heap, v) { heap.fault = Fault.None; return "a value this job cannot read"; }
        if v.is_symbol() || v.is_bigint() { return "a symbol or bigint value"; }
        if !v.is_object() || marks[v.id()] { continue; }
        marks[v.id()] = true; let n = node(heap, v.id());
        if !charge(heap) { return "a value past the work limit"; }
        if n.metadata.is_object() { work.push(n.metadata); }
        if n.kind >= node_kind::List && n.kind <= node_kind::Set || n.kind == node_kind::Concat { for i in n.count { work.push(slot(heap, n.start + i)); } }
        else if n.kind == node_kind::Atom { atoms.push(v.id()); work.push(n.value); }
        else if n.kind == node_kind::Closure || n.kind == node_kind::Builtin || n.kind == node_kind::Delay || n.kind == node_kind::Cell && n.length == cell_flavor::Delay && n.value.is_object() { functions = true; return ""; }
        // A bound function always has own `length` and `name` (light-cljs-852).
        else if n.kind == node_kind::BoundFunction { return "a bound function (a function with properties, a mutable JavaScript object)"; }
        else if n.kind == node_kind::LazySeq { return "an unrealized lazy sequence"; }
        else if n.kind != node_kind::Symbol && n.kind != node_kind::Keyword && n.kind != node_kind::Scalar && n.kind != node_kind::Cell { return "a mutable JavaScript object"; }
    }
    return "";
}
// The callable walk (callable::graph's rules, named): above the image,
// forms, closures without own properties, the constant variables they
// capture, cell references (a delay's with its thunk), atoms and job
// delays (collected in `atoms`, which become cells, a delay's reference
// keeping its thunk), and built-ins; the image's own rows as they are, but
// for arrays and assignable variables.
fn callable_unshareable(heap: Heap&, roots: const Value[:], atoms: i64[>..]&, closures: i64[>..]&) -> const u8[:] {
    let floor = cell_base.ends[6];
    var marks: bool[>..]   = []; marks.resize(heap.nodes.len, false);
    var work:  Value[>..<] = []; for v in roots { work.push(v); }
    while work.len > 0 {
        let v = work.pop();
        if !valid(heap, v) { heap.fault = Fault.None; return "a value this job cannot read"; }
        if v.is_symbol() || v.is_bigint() { return "a symbol or bigint value"; }
        if !v.is_object() || marks[v.id()] { continue; }
        marks[v.id()] = true; let n = node(heap, v.id());
        if !charge(heap) { return "a value past the work limit"; }
        if n.kind == node_kind::Binding && (!n.immutable || !n.initialized) || n.kind == node_kind::MappedBinding { return "a function that captures a variable it may assign"; }
        if v.id() < floor { if n.kind == node_kind::Array { return "a mutable JavaScript object"; } continue; }
        if n.kind == node_kind::LazySeq { return "an unrealized lazy sequence"; }
        if n.kind == node_kind::BoundFunction { return "a bound function (a function with properties, a mutable JavaScript object)"; }
        if n.kind == node_kind::Object || n.kind == node_kind::Array || n.kind > node_kind::Concat && n.kind != node_kind::Atom && n.kind != node_kind::Cell && n.kind != node_kind::Delay { return "a mutable JavaScript object"; }
        if n.keys != 0 || n.key_count != 0 || n.key_capacity != 0 || n.shape != 0 || !n.proto.is_hole() { return if n.kind == node_kind::Closure { "a function with properties (a mutable JavaScript object)" } else { "a mutable JavaScript object" }; }
        if n.kind == node_kind::Atom || n.kind == node_kind::Delay { atoms.push(v.id()); }
        if n.kind == node_kind::Closure { closures.push(v.id()); }
        work.push(n.value); work.push(n.namespace_value); work.push(n.metadata);
        if snapshot::has_values(n.kind) { for i in n.count { work.push(slot(heap, n.start + i)); } }
    }
    return "";
}
// Make each atom or delay of `atoms` a cell: the owner makes it, the row
// becomes its reference (a pending delay's keeps its thunk), and then each
// state is stored (so a state may reach any of them, itself included; a
// delay realized before it was published is stored realized). False with
// the owner's answer on a failure. A publication (`initial` given)
// stores the states later, with store_initial, once it knows the places of
// its objects.
struct Initial { cell: i64, state: Value }
fn make_cells(heap: Heap&, rows: CellRow[>..]&, atoms: const i64[:], failure: i64&) -> bool { var initial: Initial[>..] = []; return make_cells(heap, rows, atoms, failure, initial, false); }
fn make_cells(heap: Heap&, rows: CellRow[>..]&, atoms: const i64[:], failure: i64&, initial: Initial[>..]&, deferring: bool) -> bool {
    // A new cell holds undefined until its state is stored.
    var states: Value[>..] = []; var made: i64[>..] = []; var realized: bool[>..] = [];
    for o in atoms {
        let n = node(heap, o);
        if n.kind != node_kind::Atom && n.kind != node_kind::Delay { states.push(Value.Undefined); made.push(-1); realized.push(false); continue; }
        let delay = n.kind == node_kind::Delay; let done = delay && !truthy(heap, n.value);
        let a = cells::exchange(cells::Request { op: cell_op::New, flavor: if delay { cell_flavor::Delay } else { n.length } });
        if !a.ok { failure = a.error; return false; }
        states.push(if delay { n.namespace_value } else { n.value }); made.push(a.cell); realized.push(done);
        var r = n; r.kind = node_kind::Cell; r.function = a.cell as u32; r.namespace_value = Value.Null;
        if delay { r.length = cell_flavor::Delay; r.value = if done { Value.Undefined } else { n.value }; } else { r.value = Value.Undefined; }
        set_node(heap, o, r);
        remember(rows, o, a.cell, 0);
    }
    for o, i in atoms {
        if made[i] < 0 || states[i].is_undefined() && !realized[i] || node(heap, o).length == cell_flavor::Delay && !realized[i] { continue; }
        if deferring { initial.push(Initial { cell: made[i], state: states[i] }); continue; }
        var stored: i64[>..] = [];
        let f, ok = encode(heap, rows, states[i], failure, stored);
        if !ok { return false; }
        let a = cells::exchange(cells::Request { op: cell_op::Store, cell: made[i], value: copy(f) });
        if !a.ok { failure = a.error; return false; }
        remember_stored(rows, stored, a.value.nodes);
    }
    return true;
}
// The flat encoding of a shareable value whose atoms are cells already;
// `stored` gets the row each graph node came from (-1 for a primitive).
fn encode(heap: Heap&, rows: const CellRow[:], v: Value, failure: i64&, stored: i64[>..]&) -> cells::Flat, bool { let none: i64[] = []; let f, ok = encode(heap, rows, v, failure, stored, none, -1); return copy(f), ok; }
// With `places` (by row, -1 for none): a publication's object has the
// identity its place in `publication` names.
fn encode(heap: Heap&, rows: const CellRow[:], v: Value, failure: i64&, stored: i64[>..]&, places: const i64[:], publication: i64) -> cells::Flat, bool {
    if v.is_undefined() { return cells::Flat {}, true; }
    if v.is_null() { return cells::Flat { kind: flat_kind::Null }, true; }
    if v.is_bool() { return cells::Flat { kind: flat_kind::Bool, number: if v.flag() { 1.0 } else { 0.0 } }, true; }
    if v.is_number() { return cells::Flat { kind: flat_kind::Number, number: v.num() }, true; }
    if v.is_string() {
        var bytes: u8[>..] = []; for u in utf16(heap, v) { bytes.push((u & 255) as u8); bytes.push((u >> 8) as u8); }
        return cells::Flat { kind: flat_kind::String, data: bytes }, true;
    }
    if v.is_object() && valid(heap, v) && reaches_function(heap, v) { let f, ok = encode_callable(heap, rows, v, failure, stored, places, publication); return copy(f), ok; }
    // A value outlives the call it was made in: the compiler occurrences and
    // source spans of that call's forms mean nothing to a later job, which
    // sees the value as one built afresh, as the V8 build's form bridge sees
    // an object it did not hand out. The export leaves out the job's source
    // text and spans (light-cljs-809), so it costs what the value reaches.
    var graph = bridge::Graph {}; var c = bridge::Control { cells: true, spans: false }; var bytes: u8[>..] = [];
    if !v.is_object() || !bridge::export_rows(heap, [v], graph, c, Value.Undefined, stored) { failure = cell_error::Encode; return cells::Flat {}, false; }
    for n, i in graph.nodes { graph.nodes[i].origin = -1; }
    if !bridge::encode(graph, bytes, c) { failure = cell_error::Encode; return cells::Flat {}, false; }
    // Each object's identity, 0 for one the owner numbers; a primitive has
    // none, and a cell reference its cell's.
    var nodes: i64[>..] = [];
    for row, i in stored { nodes.push(if row < 0 { -1 } else if graph.nodes[i].tag == form_tag::Cell { cell_identity(graph.nodes[i].number as i64) } else if publication >= 0 && row < places.len && places[row] >= 0 { publication_identity(publication, places[row]) } else { identity_of(rows, row) }); }
    let root = graph.roots[0];
    return cells::Flat { kind: flat_kind::Graph, identity: nodes[root], data: bytes, root: root, nodes: nodes }, true;
}
// A value that reaches a function or a job delay (light-cljs-809): a
// built-in or other function of the image is its row (flat_kind::Function: every program
// holds the same image); anything else is published as a callable helper is
// (flat_kind::Callable: callable::encode_cell's bytes, `nodes` the identities of its
// installed slots in install order, the root's first), its atoms made cells
// already. `stored` gets each slot's row.
fn encode_callable(heap: Heap&, rows: const CellRow[:], v: Value, failure: i64&, stored: i64[>..]&, places: const i64[:], publication: i64) -> cells::Flat, bool {
    stored.clear();
    if cell_base.base <= 0 { failure = cell_error::NoOwner; return cells::Flat {}, false; }
    let n = node(heap, v.id());
    if v.id() < cell_base.ends[6] {
        if n.kind != node_kind::Closure && n.kind != node_kind::Builtin { failure = cell_error::Encode; return cells::Flat {}, false; }
        return cells::Flat { kind: flat_kind::Function, number: v.id() }, true;
    }
    var c = snapshot::Control {}; var bytes: u8[>..] = [];
    if !callable::encode_cell(heap, cell_base, v, bytes, c, stored) { failure = cell_error::Encode; return cells::Flat {}, false; }
    var nodes: i64[>..] = [];
    for row in stored {
        let m = node(heap, row);
        nodes.push(if m.kind == node_kind::Cell { cell_identity(m.function) } else if publication >= 0 && row < places.len && places[row] >= 0 { publication_identity(publication, places[row]) } else { identity_of(rows, row) });
    }
    return cells::Flat { kind: flat_kind::Callable, identity: nodes[0], data: bytes, root: 0, nodes: nodes }, true;
}
// A callable cell value (flat_kind::Callable) in the job: installed once per identity,
// as callable::decode_install installs a helper, while the job runs. Its
// cell references and objects the job holds already map to the rows it
// holds (as install_publication maps a publication's), new ones are
// remembered, and its rows are foreign to the job's writes.
fn decode_callable(job: Job&, b: Builtin, f: const cells::Flat&) -> Value {
    let heap .= job.heap;
    let held = row_of(job.cell_rows, f.identity);
    if held >= 0 { return object(held); }
    if cell_base.base <= 0 { return owner_fail(job, b, 4); }
    let code = job.writable_code;
    if code == null { return cell_fail(job, b, "a function loaded outside a macro expansion"); }
    let start = heap.nodes.len; let values = heap.values.len; let constants = heap.constants.len;
    var c = snapshot::Control {};
    let root = callable::decode_install(f.data, cell_base, code, heap, c, true);
    if c.error != snapshot::Error.None || !root.is_object() || heap.nodes.len - start != f.nodes.len {
        if heap.fault == Fault.None { return cell_fail(job, b, "a cell value does not import"); }
        return Value.Undefined;
    }
    var prior: i64[>..] = []; var any = false;
    for i in f.nodes.len {
        let n = node(heap, start + i); var p: i64 = -1;
        if n.kind == node_kind::Cell {
            p = cell_row(job.cell_rows, n.function);
            if p < 0 { job.cell_rows.push(CellRow { row: start + i, cell: n.function }); }
            else if n.length == cell_flavor::Delay && n.value.is_object() && node(heap, p).value.is_undefined() { node_mut(heap, p).value = n.value; }
        } else if f.nodes[i] > 0 {
            p = row_of(job.cell_rows, f.nodes[i]);
            if p < 0 { job.cell_rows.push(CellRow { row: start + i, identity: f.nodes[i] }); }
        }
        prior.push(p); if p >= 0 { any = true; }
    }
    install_foreign(job, start);
    if !any { return root; }
    relink(heap, start, prior, values, constants);
    return mapped(root, start, prior);
}
// An error a delay's thunk threw, as its type and message (flat_kind::Thrown; number
// the type's index in ERROR_NAMES, plus 8 when it has no own message, root
// its fault code, identity its own, 0 until the owner numbers it), or false
// for an object that is no error of the realm.
fn error_flat(heap: Heap&, rows: const CellRow[:], e: Value) -> cells::Flat, bool {
    if !e.is_object() || node(heap, e.id()).kind != node_kind::Error { return cells::Flat {}, false; }
    let n = node(heap, e.id()); var t: i64 = -1;
    for i in ERROR_NAMES.len { if n.proto == object(error_prototype_row(i)) { t = i; } }
    if t < 0 { return cells::Flat {}, false; }
    let message, found = own_property(heap, e, key_named(heap, "message"));
    if heap.fault != Fault.None { heap.fault = Fault.None; return cells::Flat {}, false; }
    var bytes: u8[>..] = [];
    if found && message.is_string() { for u in utf16(heap, message) { bytes.push((u & 255) as u8); bytes.push((u >> 8) as u8); } }
    return cells::Flat { kind: flat_kind::Thrown, number: t + (if found && message.is_string() { 0 } else { 8 }), identity: identity_of(rows, e.id()), data: bytes, root: n.function }, true;
}
// The value a job stores: checked, its atoms made cells, then encoded. A
// failure is the operation's named error and changes nothing but the cells
// already made for its atoms.
fn store_value(job: Job&, b: Builtin, v: Value, stored: i64[>..]&) -> cells::Flat, bool {
    let heap .= job.heap;
    var atoms: i64[>..] = []; var closures: i64[>..] = [];
    let bad = unshareable(heap, [v], atoms, closures);
    if heap.fault != Fault.None { return cells::Flat {}, false; }
    if bad.len > 0 { var what: u8[>..] = []; for c in "cannot share " { what.push(c); } what.append(bad); cell_fail(job, b, what); return cells::Flat {}, false; }
    var failure: i64 = 0;
    if atoms.len > 0 && !make_cells(heap, job.cell_rows, atoms, failure) { owner_or_bridge(job, b, failure); return cells::Flat {}, false; }
    let f, ok = encode(heap, job.cell_rows, v, failure, stored);
    if !ok { owner_or_bridge(job, b, failure); return cells::Flat {}, false; }
    protect(job, closures); return f, true;
}
fn owner_or_bridge(job: Job&, b: Builtin, failure: i64) { if failure == cell_error::Encode { cell_fail(job, b, "the value does not encode"); } else { owner_fail(job, b, failure); } }
fn units_of(data: const u8[:]) -> u16[>..] {
    var units: u16[>..] = []; var i: i64 = 0;
    while i + 1 < data.len { let low: u16 = data[i]; let high: u16 = data[i + 1]; units.push(low | (high << 8)); i += 2; }
    return units;
}
// A loaded value in the job: a primitive, or the row of its identity,
// imported once.
fn decode(job: Job&, b: Builtin, f: const cells::Flat&) -> Value {
    let heap .= job.heap;
    match f.kind {
        flat_kind::Undefined => { return Value.Undefined; }, flat_kind::Null => { return Value.Null; }, flat_kind::Bool => { return boolean(f.number != 0); }, flat_kind::Number => { return number(f.number); },
        flat_kind::String => { return string(heap, units_of(f.data)); },
        flat_kind::Thrown => {
            // One error object a job for each thrown error.
            let held = row_of(job.cell_rows, f.identity);
            if held >= 0 { return object(held); }
            let t = (f.number as i64) % 8; let bare = f.number >= 8;
            if f.number < 0 || f.number >= 16 || f.root < 0 || f.root > 5 { return cell_fail(job, b, "a cell value does not import"); }
            let message = if bare { Value.Undefined } else { string(heap, units_of(f.data)) };
            if heap.fault != Fault.None { return Value.Undefined; }
            let e = error_object(heap, t, f.root as u32, message);
            if heap.fault == Fault.None && f.identity > 0 { job.cell_rows.push(CellRow { row: e.id(), identity: f.identity }); }
            return e;
        },
        flat_kind::Callable => { return decode_callable(job, b, f); },
        flat_kind::Function => {
            // A function of the image: the same row in every program.
            let row = f.number as i64;
            if row < 0 || row >= cell_base.ends[6] || row >= heap.nodes.len || node(heap, row).kind != node_kind::Closure && node(heap, row).kind != node_kind::Builtin { return cell_fail(job, b, "a cell value does not import"); }
            return object(row);
        },
        _ => {},
    }
    let known = if is_cell_identity(f.identity) { cell_row(job.cell_rows, f.identity - cell_identity(0)) } else { row_of(job.cell_rows, f.identity) };
    if known >= 0 { return object(known); }
    var graph = bridge::Graph {}; var c = bridge::Control { cells: true }; var roots: Value[>..] = [];
    if !bridge::decode(graph, f.data, c) || !import_known(heap, job.cell_rows, graph, roots, c, f.nodes) || roots.len != 1 {
        if heap.fault == Fault.None { return cell_fail(job, b, "a cell value does not import"); }
        return Value.Undefined;
    }
    for v in roots { if f.nodes.len == 0 && f.identity > 0 { remember(job.cell_rows, v.id(), -1, f.identity); } return v; }
    return Value.Undefined;
}
// Rows that came in with an import map to the rows the job held before:
// `prior[i]` is the held row for imported row base + i, or -1.
fn mapped(v: Value, base: i64, prior: const i64[:]) -> Value {
    if !v.is_object() || v.id() < base || v.id() >= base + prior.len || prior[v.id() - base] < 0 { return v; }
    return object(prior[v.id() - base]);
}
// Rewrite the links of the imported rows (base..), of the values and
// constants appended with them (from `values` and `constants`) to the held
// rows `prior` names.
fn relink(heap: Heap&, base: i64, prior: const i64[:], values: i64, constants: i64) {
    for i in prior.len {
        var n = node(heap, base + i);
        n.value = mapped(n.value, base, prior); n.namespace_value = mapped(n.namespace_value, base, prior); n.metadata = mapped(n.metadata, base, prior);
        set_node(heap, base + i, n);
    }
    for k in values..heap.values.len { heap.values[k] = mapped(heap.values[k], base, prior); }
    for k in constants..heap.constants.len { heap.constants[k] = mapped(heap.constants[k], base, prior); }
}
// Import a cell value's graph, whose nodes have the identities `identities`
// (0 or -1: none) and may hold cell references (form_tag::Cell): an object or cell
// the rows already name keeps its row, so every reference to one cell, and
// every load of one object, in a job is one object; a new one is
// remembered.
fn import_known(heap: Heap&, rows: CellRow[>..]&, g: const bridge::Graph&, roots: Value[>..]&, c: bridge::Control&, identities: const i64[:]) -> bool {
    let base = heap.nodes.len; let values = heap.values.len; let constants = heap.constants.len; let first = roots.len;
    if !bridge::import_graph(heap, g, roots, c) { return false; }
    var prior: i64[>..] = []; var any = false;
    for n, i in g.nodes {
        var p: i64 = -1;
        if n.tag == form_tag::Cell { p = cell_row(rows, n.number as i64); if p < 0 { rows.push(CellRow { row: base + i, cell: n.number as i64 }); } }
        else if i < identities.len && identities[i] > 0 { p = row_of(rows, identities[i]); if p < 0 { rows.push(CellRow { row: base + i, identity: identities[i] }); } }
        prior.push(p); if p >= 0 { any = true; }
    }
    if any { relink(heap, base, prior, values, constants); for k in first..roots.len { roots[k] = mapped(roots[k], base, prior); } }
    return true;
}
// Rows a publication brought into the job (`count` rows from `start`, with the values and
// constants appended from `values` and `constants`): their identities, as
// one range, unless the place of an object is past what an identity names;
// each cell reference, unless the job holds that cell already, and then the
// held row takes the reference's thunk when it has none; and each object
// whose identity the job held before. The publication's root, mapped.
fn install_publication(heap: Heap&, rows: CellRow[>..]&, start: i64, count: i64, values: i64, constants: i64, publication: i64, root: Value) -> Value {
    if count <= 0 { return root; }
    let named = publication >= 0 && count < 16777216;
    let low = if named { publication_identity(publication, 0) } else { 0 };
    // Whether any identity of this publication is held already.
    var held = false;
    if named { for r in rows { if r.identity > 0 && r.identity < low + count && r.identity + r.count > low { held = true; break; } } }
    var prior: i64[>..] = []; var any = false;
    for i in count {
        let n = node(heap, start + i); var p: i64 = -1;
        if n.kind == node_kind::Cell {
            p = cell_row(rows, n.function);
            if p < 0 { rows.push(CellRow { row: start + i, cell: n.function }); }
            else if n.length == cell_flavor::Delay && n.value.is_object() && node(heap, p).value.is_undefined() { node_mut(heap, p).value = n.value; }
        } else if held { p = row_of(rows, low + i); }
        prior.push(p); if p >= 0 { any = true; }
    }
    if named { rows.push(CellRow { row: start, identity: low, count: count }); }
    if !any { return root; }
    relink(heap, start, prior, values, constants);
    return mapped(root, start, prior);
}
// A form helper's graph imported as its publication's rows (roots gets its
// value).
fn import_publication(heap: Heap&, rows: CellRow[>..]&, g: const bridge::Graph&, roots: Value[>..]&, c: bridge::Control&, publication: i64) -> bool {
    let start = heap.nodes.len; let values = heap.values.len; let constants = heap.constants.len; let first = roots.len;
    if !bridge::import_graph(heap, g, roots, c) { return false; }
    let root = if roots.len > first { roots[first] } else { Value.Undefined };
    let installed = install_publication(heap, rows, start, heap.nodes.len - start, values, constants, publication, root);
    if roots.len > first { roots[first] = installed; }
    for k in (first + 1)..roots.len { roots[k] = installed; }
    return true;
}
// The expected value of a compare-and-set: what the job loaded, by identity
// for a graph, by value for a primitive.
fn expected(job: Job&, v: Value) -> cells::Flat {
    let heap .= job.heap;
    if v.is_object() {
        let n = node(heap, v.id());
        if v.id() < cell_base.ends[6] && (n.kind == node_kind::Closure || n.kind == node_kind::Builtin) { return cells::Flat { kind: flat_kind::Function, number: v.id() }; }
        return cells::Flat { kind: flat_kind::Graph, identity: if n.kind == node_kind::Cell { cell_identity(n.function) } else { identity_of(job.cell_rows, v.id()) } };
    }
    var failure: i64 = 0; var stored: i64[>..] = []; let f, ok = encode(heap, job.cell_rows, v, failure, stored);
    return f;
}
// Publication (the preparer, a helper production): make a cell of every atom
// or delay `root` reaches through any link, the image's rows (below
// `floor`) aside. "" or the reason the publication fails; `thunks` is set
// when a pending delay became a cell, whose thunk only a callable
// publication carries (a form graph names just the cell). The cells'
// states go in `initial`, for store_initial once the publication's
// places are known.
fn publish_cells(heap: Heap&, root: Value, floor: i64, thunks: bool&, initial: Initial[>..]&) -> u8[] {
    var marks: bool[>..] = []; marks.resize(heap.nodes.len, false);
    var work:  i64[>..<] = []; var atoms: i64[>..] = [];
    if root.is_object() && root.id() < heap.nodes.len { work.push(root.id()); marks[root.id()] = true; }
    while work.len > 0 {
        let o = work.pop(); let n = node(heap, o);
        if n.kind == node_kind::Atom || n.kind == node_kind::Delay { atoms.push(o); }
        var edges: Value[>..] = [n.value, n.namespace_value, n.metadata, n.proto];
        if snapshot::has_values(n.kind) { for i in n.count { edges.push(slot(heap, n.start + i)); } }
        if snapshot::shaped(n) {
            for v, i in n.slots { if i < n.key_count { edges.push(v); } }
            for row in n.keys..(n.keys + overflow(n)) { edges.push(slot(heap, row)); }
        } else if snapshot::record_run(n) { for p in n.keys..(n.keys + n.key_count) { edges.push(property_row(heap, p).value); } }
        var accessors: Value[>..] = [];
        for e in edges { if e.is_accessor() { let p = pair(heap, e); accessors.push(p.get); accessors.push(p.set); } }
        edges.append(accessors);
        for v in edges {
            if v.is_object() && v.id() >= floor && v.id() < heap.nodes.len && !marks[v.id()] { marks[v.id()] = true; work.push(v.id()); }
        }
    }
    if atoms.len == 0 { return []; }
    // Each atom's state, and each realized delay's value, must itself be
    // shareable; a pending delay keeps its thunk in the publication.
    var nested: i64[>..] = [];
    for o in atoms {
        let n = node(heap, o);
        if n.kind == node_kind::Delay && truthy(heap, n.value) { thunks = true; continue; }
        let bad = unshareable(heap, [if n.kind == node_kind::Delay { n.namespace_value } else { n.value }], nested);
        if bad.len > 0 { return str("cell-unshareable ", bad); }
    }
    var rows: CellRow[>..] = []; var failure: i64 = 0;
    if !make_cells(heap, rows, atoms, failure, initial, true) { return str("cell-error ", failure); }
    for o in nested { if node(heap, o).kind == node_kind::Atom { return str("cell-error 5"); } }
    return [];
}
// Store the initial states of a publication's cells: an object of the
// publication has the identity of its place (`places`, by row: a form
// graph's node, a callable publication's slot, a definition's row above
// the image), the one its consumers register when they install it, so a
// cell's content and the same object reached through the publication are
// one object in a job. "" or the reason the publication fails.
fn store_initial(heap: Heap&, initial: const Initial[:], places: const i64[:], publication: i64) -> u8[] {
    let rows: CellRow[] = [];
    for s in initial {
        var stored: i64[>..] = []; var failure: i64 = 0;
        let f, ok = encode(heap, rows, s.state, failure, stored, places, publication);
        if !ok { return str("cell-error ", failure); }
        let a = cells::exchange(cells::Request { op: cell_op::Store, cell: s.cell, value: copy(f) });
        if !a.ok { return str("cell-error ", a.error); }
    }
    return [];
}
// The places of rows `rows` (one per place, -1 for none) by row.
fn places_of(heap: const Heap&, rows: const i64[:]) -> i64[>..] {
    var places: i64[>..] = []; places.resize(heap.nodes.len, -1);
    for row, i in rows { if row >= 0 && row < places.len && places[row] < 0 { places[row] = i; } }
    return places;
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
            return allocate(heap, Node { kind: node_kind::Atom, length: if b == Builtin.Atom { cell_flavor::Atom } else { cell_flavor::Volatile }, value: arg(args, 0) });
        },
        AddWatch, RemoveWatch, SetValidator => {
            let a = arg(args, 0); let flavor = reference_flavor(heap, a);
            if heap.fault != Fault.None { return Value.Undefined; }
            // set-validator! derefs only for a validator; with nil it writes
            // the target's property, which fails by name below.
            if b == Builtin.SetValidator && flavor < 0 && !is_delay(heap, a) && form_value(heap, arg(args, 1)).tag() > value_tag::Null { return not_derefable(job); }
            if b != Builtin.SetValidator && flavor != cell_flavor::Atom { return reference_error(job, if b == Builtin.AddWatch { "No protocol method IWatchable/-add-watch defined for this value" } else { "No protocol method IWatchable/-remove-watch defined for this value" }); }
            return cell_fail(job, b, "watches and validators are not supported");
        },
        Deref => {
            let a = arg(args, 0);
            if is_delay(heap, a) { return delay_now(job, b, a); }
            let flavor = reference_flavor(heap, a);
            if heap.fault != Fault.None { return Value.Undefined; }
            if flavor < 0 { return not_derefable(job); }
            return load(job, b, a);
        },
        Reset, Vreset, ResetVals => {
            let a = arg(args, 0); let v = arg(args, 1); let flavor = reference_flavor(heap, a);
            if heap.fault != Fault.None { return Value.Undefined; }
            if flavor != (if b == Builtin.Vreset { cell_flavor::Volatile } else { cell_flavor::Atom }) {
                if b == Builtin.ResetVals { return derefs_first(job, b, a, no_reset()); }
                return reference_error(job, if b == Builtin.Vreset { "No protocol method IVolatile/-vreset! defined for this value" } else { no_reset() });
            }
            if node(heap, a.id()).kind == node_kind::Atom {
                if !writable(job, a.id()) { heap.fault = Fault.NativeBoundary; return Value.Undefined; }
                let old = node(heap, a.id()).value; node_mut(heap, a.id()).value = v;
                return if b == Builtin.ResetVals { form_collection(heap, node_kind::Vector, [old, v]) } else { v };
            }
            var stored: i64[>..] = [];
            let f, ok = store_value(job, b, v, stored); if !ok { return Value.Undefined; }
            let cell: i64 = node(heap, a.id()).function;
            if b != Builtin.ResetVals {
                let r = cells::exchange(cells::Request { op: cell_op::Store, cell: cell, value: copy(f) });
                if !r.ok { return owner_fail(job, b, r.error); }
                remember_stored(job.cell_rows, stored, r.value.nodes);
                return v;
            }
            // reset-vals!: the value replaced, atomically.
            loop {
                let current = cells::exchange(cells::Request { op: cell_op::Load, cell: cell });
                if !current.ok { return owner_fail(job, b, current.error); }
                let old = decode(job, b, current.value); if heap.fault != Fault.None { return Value.Undefined; }
                let r = cells::exchange(cells::Request { op: cell_op::CompareAndSet, cell: cell, expected: expected(job, old), value: copy(f) });
                if r.ok { remember_stored(job.cell_rows, stored, r.value.nodes); return form_collection(heap, node_kind::Vector, [old, v]); }
                if r.error != 0 { return owner_fail(job, b, r.error); }
            }
            return Value.Undefined;
        },
        CompareAndSet => {
            let a = arg(args, 0); let old = arg(args, 1); let v = arg(args, 2);
            if reference_flavor(heap, a) != cell_flavor::Atom { if heap.fault == Fault.None { derefs_first(job, b, a, no_reset()); } return Value.Undefined; }
            if node(heap, a.id()).kind == node_kind::Atom {
                let same = form_equal(heap, node(heap, a.id()).value, old); if heap.fault != Fault.None { return Value.Undefined; }
                if !same { return boolean(false); }
                if !writable(job, a.id()) { heap.fault = Fault.NativeBoundary; return Value.Undefined; }
                node_mut(heap, a.id()).value = v; return boolean(true);
            }
            // Compare first, as cljs.core does: a miss neither checks nor
            // shares the replacement (its atoms stay the job's).
            let cell: i64 = node(heap, a.id()).function;
            let current = cells::exchange(cells::Request { op: cell_op::Load, cell: cell });
            if !current.ok { return owner_fail(job, b, current.error); }
            var held = decode(job, b, current.value); if heap.fault != Fault.None { return Value.Undefined; }
            let same = form_equal(heap, held, old); if heap.fault != Fault.None { return Value.Undefined; }
            if !same { return boolean(false); }
            var stored: i64[>..] = [];
            let f, ok = store_value(job, b, v, stored); if !ok { return Value.Undefined; }
            loop {
                let r = cells::exchange(cells::Request { op: cell_op::CompareAndSet, cell: cell, expected: expected(job, held), value: copy(f) });
                if r.ok { remember_stored(job.cell_rows, stored, r.value.nodes); return boolean(true); }
                if r.error != 0 { return owner_fail(job, b, r.error); }
                // Another write came first: compare again with what it stored.
                held = decode(job, b, r.value); if heap.fault != Fault.None { return Value.Undefined; }
                let still = form_equal(heap, held, old); if heap.fault != Fault.None { return Value.Undefined; }
                if !still { return boolean(false); }
            }
            return Value.Undefined;
        },
        _ => { heap.fault = Fault.InvalidState; return Value.Undefined; },
    }
}
// deref of an atom, volatile or cell.
fn load(job: Job&, b: Builtin, a: Value) -> Value {
    let n = node(job.heap, a.id());
    if n.kind == node_kind::Atom { return n.value; }
    let r = cells::exchange(cells::Request { op: cell_op::Load, cell: n.function });
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
    if reference_flavor(heap, a) != cell_flavor::Atom {
        if heap.fault == Fault.None { if b == Builtin.SwapVals { derefs_first(job, b, a, no_swap()); } else { reference_error(job, no_swap()); } }
        return;
    }
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
// Call the frame's function with the value read and the extra arguments,
// `this` null as the V8 build's `fn.call(null, ...)`: a plain built-in (and
// gensym in a scope) runs here (true with its result); a closure or a
// native-frame built-in (swap! itself, Array.prototype.map) gets a frame
// above this one (false; its result comes back at pc 1).
fn swap_call(job: Job&, at: i64, b: Builtin) -> Value, bool {
    let heap .= job.heap; let frame = job.frames[at];
    var args: Value[>..] = [frame.output]; args.append(job.operands[frame.operand_base..(frame.operand_base + frame.argument_count)]);
    if node(heap, frame.callee.id()).kind == node_kind::Builtin {
        let cb = builtin_of(heap, frame.callee.id());
        if cb == Builtin.Gensym && job.gensym_next > 0 { return gensym(job, args), true; }
        if forces(heap, cb, args) { job.frames[at].pc = 1; start_force(job, cb, args[0], job.operands.len); return Value.Undefined, false; }
        if native_builtin(cb) { job.frames[at].pc = 1; native_start(job, cb, Value.Null, args, job.operands.len); return Value.Undefined, false; }
        if effect_builtin(cb) || cb == Builtin.Mapv || cb == Builtin.MapSeq { unsupported_step(job, b, "a host-effect or mapv built-in function"); return Value.Undefined, false; }
        return call_builtin(job, node(heap, frame.callee.id()).function, Value.Null, args), true;
    }
    job.frames[at].pc = 1;
    if is_bound(heap, frame.callee) { start_bound(job, frame.callee, Value.Null, args, job.operands.len); return Value.Undefined, false; }
    push_frame(job, frame.callee, Value.Null, args, false, job.operands.len);
    return Value.Undefined, false;
}
// Store result r: true when the frame finished (or failed), false when the
// cell held another value, which the frame now holds as the value read.
fn swap_commit(job: Job&, at: i64, b: Builtin, r: Value) -> bool {
    let heap .= job.heap; let frame = job.frames[at]; let a = frame.input; var old = frame.output;
    if node(heap, a.id()).kind == node_kind::Atom {
        // The V8 build's swap-vals! pairs the state replaced at the store,
        // which the function itself may have reset.
        if !writable(job, a.id()) { heap.fault = Fault.NativeBoundary; return true; }
        old = node(heap, a.id()).value; node_mut(heap, a.id()).value = r;
    } else {
        var stored: i64[>..] = [];
        let f, ok = store_value(job, b, r, stored); if !ok { return true; }
        let answer = cells::exchange(cells::Request { op: cell_op::CompareAndSet, cell: node(heap, a.id()).function, expected: expected(job, frame.output), value: copy(f) });
        if !answer.ok && answer.error != 0 { owner_fail(job, b, answer.error); return true; }
        if !answer.ok {
            let current = decode(job, b, answer.value); if heap.fault != Fault.None { return true; }
            job.frames[at].output = current; return false;
        }
        remember_stored(job.cell_rows, stored, answer.value.nodes);
    }
    let result = if b == Builtin.SwapVals { form_collection(heap, node_kind::Vector, [old, r]) } else { r };
    if heap.fault == Fault.None { finish_native(job, frame, result); }
    return true;
}

// Delays (light-cljs-808): Delay (what the compiler emits for (delay ...),
// new rt.core.Delay(thunk, null)), force, realized? and delay?; deref and
// derefProtocol of a delay force it.
fn delay_builtin(job: Job&, b: Builtin, args: const Value[:]) -> Value {
    let heap .= job.heap; let d = arg(args, 0);
    match b {
        Delay => { return allocate(heap, Node { kind: node_kind::Delay, value: d, namespace_value: arg(args, 1) }); },
        IsDelay => { return boolean(is_delay(heap, d)); },
        Force => { return if is_delay(heap, d) { delay_now(job, b, d) } else { d }; },
        Realized => {
            if !is_delay(heap, d) { return unsupported_step(job, b, "a value that is not a delay"); }
            let n = node(heap, d.id());
            if n.kind == node_kind::Delay { return boolean(!truthy(heap, n.value)); }
            let a = cells::exchange(cells::Request { op: cell_op::Load, cell: n.function });
            if !a.ok { return owner_fail(job, b, a.error); }
            return boolean(a.state == delay_state::Realized || a.state == delay_state::Failed);
        },
        _ => { heap.fault = Fault.InvalidState; return Value.Undefined; },
    }
}
// realized? of a delay as the form instruction cljRealized asks it; any
// other value is a boundary there.
fn delay_realized(heap: Heap&, d: Value) -> Value {
    if !is_delay(heap, d) { heap.fault = Fault.NativeBoundary; return Value.Undefined; }
    let n = node(heap, d.id());
    if n.kind == node_kind::Delay { return boolean(!truthy(heap, n.value)); }
    let a = cells::exchange(cells::Request { op: cell_op::Load, cell: n.function });
    if !a.ok { heap.fault = Fault.NativeBoundary; return Value.Undefined; }
    return boolean(a.state == delay_state::Realized || a.state == delay_state::Failed);
}
// A delay forced where no frame can run its thunk (a built-in calling a
// callback): its value when realized, otherwise a named failure.
fn delay_now(job: Job&, b: Builtin, d: Value) -> Value {
    let heap .= job.heap; let n = node(heap, d.id());
    if n.kind == node_kind::Delay { return if truthy(heap, n.value) { unsupported_step(job, b, "a pending delay forced by a built-in's callback") } else { n.namespace_value }; }
    let a = cells::exchange(cells::Request { op: cell_op::Load, cell: n.function });
    if !a.ok { return owner_fail(job, b, a.error); }
    if a.state != delay_state::Realized { return unsupported_step(job, b, "a delay forced by a built-in's callback before it realized"); }
    return decode(job, b, a.value);
}
// Whether the dispatch loop's call of built-in b with `args` forces a delay,
// which runs as a native frame (start_force).
fn forces(heap: Heap&, b: Builtin, args: const Value[:]) -> bool { return (b == Builtin.Deref || b == Builtin.Force) && args.len > 0 && is_delay(heap, args[0]); }
// Whether `new f(...)` constructs as its call with callee f, the dispatch
// loop's map demand included: the Delay built-in, also the only callee
// rt.nativeConstruct accepts. Every other callee takes source_vm.goose's
// construct (light-cljs-831).
fn constructible(heap: Heap&, f: Value) -> bool { return valid(heap, f) && f.is_object() && node(heap, f.id()).kind == node_kind::Builtin && builtin_of(heap, f.id()) == Builtin.Delay; }

// force, deref and derefProtocol of a delay (Native.Force, its Builtin the
// payload): a native frame, so a thunk that suspends the job (a helper
// dependency) or a force that waits for another job's claim resumes it in
// any worker.
//   input   the delay
//   pc 0    a job's own delay: its value, or its thunk called (V8's
//           deferredValue); a cell: forced at the owner (claimed, waited
//           for, or its value)
//   pc 1    awaits the thunk's value: a job's own delay keeps it; a cell's
//           claim settles with it
//   pc 2    a cell's thunk threw (the catch handler this frame pushed when
//           it claimed): the claim settles failed with the error, which is
//           thrown again
//   pc 3    waits for another job's claim (a host effect, kind 10: cell,
//           epoch); when it ends the frame forces again
fn start_force(job: Job&, b: Builtin, d: Value, base: i64) {
    let heap .= job.heap;
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
    job.operands.resize(base);
    job.frames.push(Frame { function: -1, pc: 0, local_base: job.locals.len, operand_base: base, callee: Value.Undefined, receiver: Value.Undefined, input: d, native: Native.Force { which: b } });
}
fn force_step(job: Job&, frame: Frame, b: Builtin) {
    let heap .= job.heap; let at = job.frames.len - 1; let d = frame.input; let n = node(heap, d.id());
    if frame.pc == 3 { job.operands.pop(); job.frames[at].pc = 0; return; }
    if frame.pc == 1 {
        let r = job.operands.pop();
        if n.kind == node_kind::Delay {
            if !writable(job, d.id()) { heap.fault = Fault.NativeBoundary; return; }
            node_mut(heap, d.id()).value = Value.Null; node_mut(heap, d.id()).namespace_value = r;
            finish_native(job, frame, r); return;
        }
        let h = job.handlers.len - 1;
        if h < 0 || job.handlers[h].frame != at || job.handlers[h].kind != HandlerKind.Catch { heap.fault = Fault.InvalidState; return; }
        job.handlers.pop();
        if settle(job, b, frame, r, false) { finish_native(job, frame, r); }
        return;
    }
    if frame.pc == 2 {
        let e = job.operands.pop();
        if settle(job, b, frame, e, true) { raise(job, e, frame); }
        return;
    }
    if n.kind == node_kind::Delay {
        if !truthy(heap, n.value) { finish_native(job, frame, n.namespace_value); return; }
        force_call(job, at, b, n.value); return;
    }
    let cell: i64 = n.function;
    // Outside a macro job (a preparation, a helper production) nothing
    // claims: a realized cell's value, or a named failure.
    if job.cell_holder <= 0 {
        let a = cells::exchange(cells::Request { op: cell_op::Load, cell: cell });
        if !a.ok { owner_fail(job, b, a.error); return; }
        if a.state == delay_state::Realized { let v = decode(job, b, a.value); if heap.fault == Fault.None { finish_native(job, frame, v); } return; }
        if a.state == delay_state::Failed { let e = decode(job, b, a.value); if heap.fault == Fault.None { raise(job, e, frame); } return; }
        cell_fail(job, b, "a pending delay forced outside a macro expansion"); return;
    }
    let a = cells::exchange(cells::Request { op: cell_op::Force, cell: cell, holder: job.cell_holder });
    if !a.ok { owner_fail(job, b, a.error); return; }
    if a.state == delay_state::Realized { let v = decode(job, b, a.value); if heap.fault == Fault.None { finish_native(job, frame, v); } return; }
    if a.state == delay_state::Failed { let e = decode(job, b, a.value); if heap.fault == Fault.None { raise(job, e, frame); } return; }
    if a.state == delay_state::Waiting {
        // Another job holds the claim: wait for it to end, as a dependency.
        job.frames[at].pc = 3;
        request_effect(job, 10, [number(cell), number(a.epoch)], job.frames[at]);
        return;
    }
    // Claimed: run the thunk in this job, catching what it throws.
    job.claims += 1;
    if !callable(heap, n.value) {
        cells::exchange(cells::Request { op: cell_op::Release, cell: cell, holder: job.cell_holder }); job.claims -= 1;
        cell_fail(job, b, "a delay whose function this job does not hold"); return;
    }
    push_handler(job, at, HandlerKind.Catch, 2); if heap.fault != Fault.None { return; }
    force_call(job, at, b, n.value);
}
// Call thunk `f` with a null `this`, as the V8 build's `thunk.call(null)`:
// a closure gets a frame above this one, a plain built-in runs here; either
// way its value comes back at pc 1.
fn force_call(job: Job&, at: i64, b: Builtin, f: Value) {
    let heap .= job.heap;
    if !callable(heap, f) { type_error(job, "not a function"); return; }
    job.frames[at].pc = 1;
    if node(heap, f.id()).kind == node_kind::Builtin {
        let cb = builtin_of(heap, f.id());
        if native_builtin(cb) || effect_builtin(cb) || cb == Builtin.Mapv || cb == Builtin.MapSeq || cb == Builtin.Gensym { unsupported_step(job, b, "a host-effect or native-frame built-in as a delay's function"); return; }
        let v = call_builtin(job, node(heap, f.id()).function, Value.Null, []);
        if heap.fault == Fault.None { job.operands.push(v); }
        return;
    }
    if is_bound(heap, f) { start_bound(job, f, Value.Null, [], job.operands.len); return; }
    push_frame(job, f, Value.Null, [], false, job.operands.len);
}
// What a claim settles with: an error of the realm by its type and
// message, any other value as a cell stores it.
fn settled_value(job: Job&, b: Builtin, v: Value, failed: bool, stored: i64[>..]&) -> cells::Flat, bool {
    if failed { let e, error = error_flat(job.heap, job.cell_rows, v); if error { return copy(e), true; } }
    let f, ok = store_value(job, b, v, stored); return copy(f), ok;
}
// Settle the frame's claim with the thunk's value `v` or, `failed`, what it
// threw (an error of the realm keeps its type and message). True when it
// settled; otherwise the claim is released and the force fails by name.
fn settle(job: Job&, b: Builtin, frame: Frame, v: Value, failed: bool) -> bool {
    let heap .= job.heap; let cell: i64 = node(heap, frame.input.id()).function;
    var stored: i64[>..] = [];
    let f, ok = settled_value(job, b, v, failed, stored);
    if ok {
        let a = cells::exchange(cells::Request { op: cell_op::Settle, cell: cell, value: copy(f), holder: job.cell_holder, failed: failed });
        if a.ok {
            job.claims -= 1; remember_stored(job.cell_rows, stored, a.value.nodes);
            // The thrown error is the one later forces in this job throw.
            if a.value.kind == flat_kind::Thrown && identity_of(job.cell_rows, v.id()) == 0 { job.cell_rows.push(CellRow { row: v.id(), identity: a.value.identity }); }
            return true;
        }
        owner_fail(job, b, a.error);
    }
    cells::exchange(cells::Request { op: cell_op::Release, cell: cell, holder: job.cell_holder }); job.claims -= 1;
    return false;
}
