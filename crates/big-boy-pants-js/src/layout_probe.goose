import std;

// Layout/lifetime evidence only: no bytecode dispatch or macro implementation.
// Invariants motivated by corpus/closure-captured.pants.js and churn-churn.pants.js.
enum Capture { Value { value: i64 }, Cell { id: i64 } }
struct Cell { value: i64, initialized: bool = false, writable: bool = true }
struct Range { start: i64, count: i64 }
struct Atom { value: i64, origin: i64 }
root struct Storage {
    locals:   i64[>..<]    = [],
    cells:    Cell[>..]    = [],
    captures: Capture[>..] = [],
    atoms:    Atom[>..]    = [],
    children: i64[>..]     = [],
    vectors:  Range[>..]   = [],
}

fn read_capture(s: Storage&, index: i64) -> i64 {
    assert(index >= 0 && index < s.captures.len);
    match s.captures[index] {
        Value v => { return v.value; },
        Cell c => {
            assert(c.id >= 0 && c.id < s.cells.len);
            assert(s.cells[c.id].initialized);
            return s.cells[c.id].value;
        },
    }
}
fn write_cell(s: Storage&, id: i64, value: i64) -> bool {
    if id < 0 || id >= s.cells.len { return false; }
    if !s.cells[id].initialized || !s.cells[id].writable { return false; }
    s.cells[id].value = value;
    return true;
}

fn captures_probe() {
    var s = Storage {};
    let frame_base = s.locals.len;
    s.locals.append([7, 10]);
    // The lowerer selects Value for initialized, never-assigned bindings.
    // Copy the payload, never a reference into a reclaimable frame slot.
    s.captures.push(Capture.Value { value: s.locals[frame_base + 1] });
    let closure = Range { start: 0, count: 1 };
    s.locals.resize(frame_base);
    s.locals.append([99, 100]); // Reuse the exact local slots after return.
    for i in 512 { s.captures.push(Capture.Value { value: i }); }
    assert(closure.count == 1 && read_capture(s, closure.start) == 10);

    // Assigned bindings instead share one session-owned cell, even after return.
    let cell = s.cells.len;
    s.cells.push(Cell { value: 1, initialized: true });
    let setter_capture = s.captures.len;
    s.captures.push(Capture.Cell { id: cell });
    let getter_capture = s.captures.len;
    s.captures.push(Capture.Cell { id: cell });
    s.locals.clear();
    for i in 512 { s.cells.push(Cell { value: i, initialized: true }); }
    assert(write_cell(s, cell, 41));
    assert(read_capture(s, setter_capture) == 41 && read_capture(s, getter_capture) == 41);
    assert(read_capture(s, closure.start) == 10);
    let uninitialized = s.cells.len;
    s.cells.push(Cell { value: 0 });
    let immutable = s.cells.len;
    s.cells.push(Cell { value: 3, initialized: true, writable: false });
    assert(!write_cell(s, uninitialized, 8) && !write_cell(s, immutable, 8));
    assert(!write_cell(s, -1, 8) && !write_cell(s, s.cells.len, 8));
    assert(s.cells[uninitialized].value == 0 && s.cells[immutable].value == 3);
    print("layout captures: frame reuse, table growth, shared cells and rejected writes passed");
}

fn vector(s: Storage&, children: const i64[:]) -> i64 {
    let range = Range { start: s.children.len, count: children.len };
    for id in children { assert(id >= 0 && id < s.atoms.len); }
    s.children.append(children);
    let id = s.vectors.len;
    s.vectors.push(range);
    return id;
}
fn child(s: Storage&, vector_id: i64, index: i64) -> i64 {
    assert(vector_id >= 0 && vector_id < s.vectors.len);
    let r = s.vectors[vector_id];
    assert(index >= 0 && index < r.count);
    return s.children[r.start + index];
}
fn updated(s: Storage&, vector_id: i64, index: i64, atom: i64) -> i64 {
    assert(vector_id >= 0 && vector_id < s.vectors.len);
    assert(atom >= 0 && atom < s.atoms.len);
    let old = s.vectors[vector_id];
    assert(index >= 0 && index <= old.count);
    let start = s.children.len;
    // A bounded flat persistent update, copying IDs rather than child objects.
    // Copy the range record before growth; retain no slices into the destination.
    for i in old.count {
        let value = if i == index { atom } else { child(s, vector_id, i) };
        s.children.push(value);
    }
    if index == old.count { s.children.push(atom); }
    let id = s.vectors.len;
    let count = old.count + (if index == old.count { 1 } else { 0 });
    s.vectors.push(Range { start, count });
    return id;
}

fn collections_probe(exported: Atom[>..]&) {
    var s = Storage {};
    // Equal values at two source occurrences must not be interned together.
    s.atoms.append([Atom { value: 1, origin: 101 }, Atom { value: 1, origin: 202 }, Atom { value: 9, origin: 303 }]);
    let old = vector(s, [0, 1, 0]); // Same occurrence also appears twice.
    let changed = updated(s, old, 1, 2);
    let appended = updated(s, changed, 3, 1);
    assert(child(s, old, 1) == 1 && child(s, changed, 1) == 2);
    assert(child(s, changed, 0) == child(s, old, 0));
    assert(child(s, changed, 2) == child(s, old, 2));
    assert(child(s, appended, 0) == child(s, appended, 2));
    assert(child(s, appended, 0) != child(s, appended, 3));
    assert(s.atoms[child(s, appended, 0)].value == s.atoms[child(s, appended, 3)].value);
    assert(s.atoms[child(s, appended, 0)].origin != s.atoms[child(s, appended, 3)].origin);
    for i in 512 { let more = vector(s, [2]); assert(child(s, more, 0) == 2); }
    assert(s.vectors[old].count == 3 && s.vectors[appended].count == 4);
    // Independently owned scalar export survives all private storage teardown.
    // A real bridge additionally relocates graph IDs and retains source owners.
    for i in s.vectors[appended].count { exported.push(s.atoms[child(s, appended, i)]); }
}

fn main() {
    captures_probe();
    var exported: Atom[>..] = [];
    collections_probe(exported);
    assert(exported.len == 4);
    assert(exported[0].value == 1 && exported[1].value == 9);
    assert(exported[0].origin == 101 && exported[2].origin == 101 && exported[3].origin == 202);
    print("layout collections: old versions, child aliases, occurrence origins and owned export passed");
}
