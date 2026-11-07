import std;
import os;
import .source_vm;
import .export_schema;
import .program;

namespace snapshot;

// This is a private copy, not immutable artifact publication. All components
// are flat owned arrays, and every graph edge is a row index into the image's
// own tables: no value carries a session, so restore copies rows verbatim.
// Allocator 1 is the current append-only arena: its complete metadata is the
// fourteen end cursors. There are deliberately no fictional free lists.
struct Binding { id: i64, generation: i64, value: source::Value }
struct Header {
    version: i64     = 2, allocator: i64 = 1, base: i64 = 0, checkpoint: i64 = 1,
    ends:    i64[21] = [0; 21], limits: source::Limits = source::Limits {},
    work:    i64     = 0, global: source::Value = source::Value.Undefined, call_method: source::Value = source::Value.Undefined,
    boot:    i64     = 0, initializers: i64 = 0, producers: i64 = 0, property_rows: i64 = 0,
}
struct Snapshot {
    header:     Header          = Header {},
    code_bytes: u8[]            = [], functions: source::Function[] = [], code_constants: source::Value[] = [],
    children:   u32[]           = [], ops: source::Op[] = [], code_units: u16[] = [], code_strings: source::Str[] = [],
    nodes:      source::Node[]  = [], values: source::Value[] = [], properties: source::Property[] = [],
    units:      u16[]           = [], strings: source::Str[] = [], constants: source::Value[] = [], bytes: u8[] = [], sources: source::SourceRange[] = [], bindings: Binding[] = [], latin1: u8[] = [],
    shapes:     source::Shape[] = [], accessors: source::Pair[] = [], keys: source::KeySlot[] = [], conts: source::Cont[] = [],
}
// A borrowing counterpart lets capture validate before allocating a copy.
struct View {
    header:     Header,
    code_bytes: const u8[:], functions: const source::Function[:], code_constants: const source::Value[:],
    children:   const u32[:], ops: const source::Op[:], code_units: const u16[:], code_strings: const source::Str[:],
    nodes:      const source::Node[:], values: const source::Value[:], properties: const source::Property[:],
    units:      const u16[:], strings: const source::Str[:], constants: const source::Value[:], bytes: const u8[:], sources: const source::SourceRange[:], bindings: const Binding[:], latin1: const u8[:],
    shapes:     const source::Shape[:], accessors: const source::Pair[:], keys: const source::KeySlot[:], conts: const source::Cont[:],
}
enum Error { None, Boundary, Version, Allocator, Identity, Length, Link, Code, Unsupported, Bytes, Work, Destination }
struct Control {
    bytes:          i64 = 67108864, work: i64 = 10000000, visited: i64 = 0, error: Error = Error.None,
    snapshot_bytes: i64 = 0, copied_bytes: i64 = 0, validation_peak: i64 = 0, capture_ns: i64 = 0, restore_ns: i64 = 0,
}
fn reject(c: Control&, error: Error) -> bool { if c.error == Error.None { c.error = error; } return false; }
fn poll(c: Control&, n: i64 = 1) -> bool {
    if c.error != Error.None { return false; }
    if n < 0 || n > c.work - c.visited { return reject(c, Error.Work); }
    c.visited += n; return true;
}
fn range(start: i64, count: i64, len: i64) -> bool { return start >= 0 && count >= 0 && start <= len && count <= len - start; }
// Kinds whose start/count name a run of values: an ordinary object's or an
// array's element run (light-cljs-804), captures, form children, arguments
// and a deferred map's memo.
fn has_values(kind: i64) -> bool { return kind <= 2 || kind >= 5 && kind <= 8 || kind == 12 || kind == 14 || kind == 15; }
// An element run (an ordinary object's or an array's) is exclusive and may
// hold holes.
fn elements(kind: i64) -> bool { return kind == 0 || kind == 1; }
// All ranges have already passed bounds validation. Immutable views may share
// slots, but an unrealized map owns its entire writable memo exclusively,
// including the completed prefix. Charge both scratch initialization and each
// range visit; overlapping immutable views cannot hide unbounded scan work.
fn memo_ranges<T>(s: const T&, c: Control&) -> bool {
    if s.values.len > c.bytes - footprint(s) { return reject(c, Error.Bytes); }
    if !poll(c, s.values.len) { return false; }
    c.validation_peak = max(c.validation_peak, s.values.len);
    var used: u8[>..] = []; used.resize(s.values.len, 0);
    for n in s.nodes {
        if !poll(c) { return false; }
        // A shape-mode object's overflow run is exclusive (light-cljs-787).
        if shaped(n) {
            if !range(n.keys, n.key_capacity, s.values.len) { return reject(c, Error.Link); }
            if !poll(c, n.key_capacity) { return false; }
            for slot in n.keys..(n.keys + n.key_capacity) { if used[slot] != 0 { return reject(c, Error.Link); } used[slot] = 2; }
        }
        if !has_values(n.kind) { continue; }
        if !poll(c, n.count) { return false; }
        for slot in n.start..(n.start + n.count) {
            if n.kind == 15 || elements(n.kind) {
                if used[slot] != 0 { return reject(c, Error.Link); }
                used[slot] = 2;
            } else {
                if used[slot] == 2 { return reject(c, Error.Link); }
                used[slot] = 1;
            }
        }
    }
    return true;
}
// The kinds with own properties by name (source::Node): ordinary objects,
// arrays and closures (light-cljs-805), built-in functions and error objects
// (light-cljs-788), generators, promises and primitive wrappers.
fn named(kind: i64) -> bool { return kind >= 0 && kind <= 2 || kind == 4 || kind == 13 || kind == 20 || kind == 21 || kind == 22; }
// A shape-mode object, dense array or closure: without `initialized`.
fn shaped(n: const source::Node&) -> bool { return named(n.kind) && !n.initialized; }
// A dictionary-mode object, a dictionary array or a closure's records.
fn dictionary(n: const source::Node&) -> bool { return named(n.kind) && n.initialized; }
// A run of property records: a dictionary's, or a key list's (kind 16, the
// keys a for-in walk or an object spread visits; source::Node).
fn record_run(n: const source::Node&) -> bool { return dictionary(n) || n.kind == 16; }
// A property key: an interned string (source::intern) or a well-known symbol.
fn key_value<T>(s: const T&, k: source::Value) -> bool {
    return match k { String x => range(x.s, 1, s.strings.len) && s.strings[x.s].key, Symbol x => export_well_known_symbol(x.s).len > 0, _ => false };
}
// Own properties (light-cljs-787, source::Node). Dictionary runs of records
// are exclusive (an object writes its run in place), hold keyed records up to
// key_count and cleared rows past it, and were charged to property_rows. A
// shape-mode object's shape has key_count properties, its overflow run (in
// values, memo_ranges) holds those past the inline slots, and inline slots
// past key_count are cleared. Other nodes keep no properties.
fn key_runs<T>(s: const T&, c: Control&) -> bool {
    if s.properties.len > c.bytes - footprint(s) { return reject(c, Error.Bytes); }
    if !poll(c, s.properties.len) { return false; }
    c.validation_peak = max(c.validation_peak, s.properties.len);
    var used: bool[>..] = []; used.resize(s.properties.len, false); var records: i64 = 0;
    for n in s.nodes {
        if !poll(c, 1 + n.key_capacity) { return false; }
        if n.key_count > n.key_capacity + (if shaped(n) { source::inline_slots() } else { 0 }) { return reject(c, Error.Link); }
        if !shaped(n) && n.shape != 0 { return reject(c, Error.Link); }
        for v, i in n.slots { if (if shaped(n) && i < n.key_count { !slot_value(s, v) } else { !v.is_undefined() }) { return reject(c, Error.Link); } }
        if shaped(n) {
            if !range(n.keys, n.key_capacity, s.values.len) || n.shape != 0 && !range(n.shape, 1, s.shapes.len) || (if n.shape == 0 { 0 } else { s.shapes[n.shape].count }) != n.key_count { return reject(c, Error.Link); }
            continue;
        }
        if !record_run(n) && (n.key_count != 0 || n.key_capacity != 0) { return reject(c, Error.Link); }
        if !range(n.keys, n.key_capacity, s.properties.len) { return reject(c, Error.Link); }
        for row in n.keys..(n.keys + n.key_capacity) { if used[row] { return reject(c, Error.Link); } used[row] = true; }
        for row in n.keys..(n.keys + n.key_capacity) {
            let p = s.properties[row];
            if (if row < n.keys + n.key_count { !key_value(s, p.key) || !slot_value(s, p.value) || p.attrs > 7 } else { !p.key.is_undefined() || !p.value.is_undefined() || p.attrs != 0 }) { return reject(c, Error.Link); }
        }
        records += n.key_capacity;
    }
    return if records <= s.header.property_rows { true } else { reject(c, Error.Allocator) };
}
// Shapes, accessor pairs and the interned-key index (light-cljs-787): row 0
// of a non-empty shape table is the empty shape; every other row extends an
// earlier parent by one interned key, with its older sibling and its newest
// child among that parent's and its own children. Pairs hold functions or
// Undefined. The index holds every key header once, at its hash's probe
// sequence, at most half full.
fn key_hash<T>(s: const T&, r: source::Str) -> u32 {
    var h: u32 = 2166136261;
    if r.wide { for u in s.units[r.start..(r.start + r.len)] { h = (h ^ u) * 16777619; } } else { for b in s.latin1[r.start..(r.start + r.len)] { h = (h ^ b) * 16777619; } }
    return h;
}
fn property_model<T>(s: const T&, c: Control&) -> bool {
    if !poll(c, s.shapes.len + s.accessors.len + s.keys.len) { return false; }
    for e, r in s.shapes {
        if r == 0 { if e.parent != 0 || !e.key.is_undefined() || e.attrs != 0 || e.count != 0 || e.sibling != source::no_row() || e.child != source::no_row() && !range(e.child, 1, s.shapes.len) { return reject(c, Error.Link); } continue; }
        if e.parent >= r || !key_value(s, e.key) || e.attrs > 7 || e.count != s.shapes[e.parent].count + 1 { return reject(c, Error.Link); }
        if e.sibling != source::no_row() && (e.sibling >= r || s.shapes[e.sibling].parent != e.parent) { return reject(c, Error.Link); }
        if e.child != source::no_row() && (e.child <= r || e.child >= s.shapes.len || s.shapes[e.child].parent != r) { return reject(c, Error.Link); }
    }
    if s.shapes.len > 0 && s.shapes[0].child != source::no_row() && s.shapes[s.shapes[0].child].parent != 0 { return reject(c, Error.Link); }
    for p in s.accessors { if !accessor_half(s, p.get) || !accessor_half(s, p.set) { return reject(c, Error.Link); } }
    if s.keys.len != 0 && (s.keys.len < 64 || (s.keys.len & (s.keys.len - 1)) != 0) { return reject(c, Error.Link); }
    var seen: bool[>..] = []; seen.resize(s.strings.len, false); var occupied: i64 = 0;
    for e, i in s.keys {
        if e.row == source::no_row() { continue; }
        if !range(e.row, 1, s.strings.len) || !s.strings[e.row].key || seen[e.row] { return reject(c, Error.Link); }
        let r = s.strings[e.row];
        if !poll(c, r.len + 1) { return false; }
        if key_hash(s, r) != e.hash { return reject(c, Error.Link); }
        // Linear probing: no empty slot between the hash's slot and this one.
        var at = e.hash & (s.keys.len - 1);
        while at != i { if !poll(c) { return false; } if s.keys[at].row == source::no_row() { return reject(c, Error.Link); } at = (at + 1) & (s.keys.len - 1); }
        seen[e.row] = true; occupied += 1;
    }
    var flagged: i64 = 0; for r in s.strings { if r.key { flagged += 1; } }
    if flagged != occupied || 2 * occupied > s.keys.len { return reject(c, Error.Link); }
    // Prototype chains end (source::set_prototype refuses a cycle): each
    // walk stops at a node an earlier walk finished.
    if !poll(c, 2 * s.nodes.len) { return false; }
    var state: u8[>..] = []; state.resize(s.nodes.len, 0); var path: i64[>..] = [];
    for n, i in s.nodes {
        var at: i64 = i; path.clear();
        while state[at] == 0 {
            state[at] = 1; path.push(at);
            let p = s.nodes[at].proto;
            if !p.is_object() { break; }
            at = p.id();
            if state[at] == 1 { return reject(c, Error.Link); }
        }
        for r in path { state[r] = 2; }
    }
    return true;
}
fn accessor_half<T>(s: const T&, v: source::Value) -> bool { return v.is_undefined() || v.is_object() && value(s, v); }
// A property's value: any value, or an accessor naming a pair.
fn slot_value<T>(s: const T&, v: source::Value) -> bool { return match v { Accessor a => range(a.a, 1, s.accessors.len), _ => value(s, v) }; }
// The number of occupied index slots.
// Continuation rows (light-cljs-790, source::Cont): whether row r of conts
// is variant `kind` (0 Activation, 1 Saved, 2 Promise, 3 Reaction, 4 Frame).
fn cont_kind(c: source::Cont) -> i64 { return match c { Activation => 0, Saved => 1, Promise => 2, Reaction => 3, Frame => 4 }; }
fn cont_is<T>(s: const T&, r: u32, kind: i64) -> bool { return r < s.conts.len && cont_kind(s.conts[r]) == kind; }
fn link_or_none<T>(s: const T&, r: u32, kind: i64) -> bool { return r == source::no_row() || cont_is(s, r, kind); }
// A run of `capacity` rows of kind `kind` at `first`, no_row() when empty.
fn cont_run<T>(s: const T&, first: u32, capacity: u32, kind: i64) -> bool {
    if capacity == 0 { return first == source::no_row(); }
    if !range(first, capacity, s.conts.len) { return false; }
    for i in capacity { if !cont_is(s, first + i, kind) { return false; } }
    return true;
}
// Whether v is an object of node kind `kind` (-1: any kind).
fn object_of<T>(s: const T&, v: source::Value, kind: i64) -> bool { return v.is_object() && v.id() < s.nodes.len && (kind < 0 || s.nodes[v.id()].kind == kind); }
// A native frame's state that its steps read by row: a Resume frame's
// generator, a MapSeq frame's deferred map, a Force frame's delay, a Swap
// frame's function and a CopyData frame's key list.
fn native_valid<T>(s: const T&, f: source::SavedFrame) -> bool {
    return match f.native {
        Resume => object_of(s, f.input, source::generator_kind()),
        MapSeq => object_of(s, f.input, 15),
        Force => object_of(s, f.input, -1),
        Swap => object_of(s, f.callee, -1),
        CopyData => object_of(s, f.callee, 16),
        // A HasInstance frame at pc 4 calls its built-in method (light-cljs-834).
        HasInstance => f.pc != 4 || object_of(s, f.callee, 4),
        // Array.prototype.push, join and toString read their object (light-cljs-842).
        ArrayMethod => object_of(s, f.input, -1),
        // A GetIterator frame awaits one of its three phases; from `next`'s
        // getter on, it holds the iterator (light-cljs-843).
        GetIterator => f.pc >= 1 && f.pc <= 3 && (f.pc != 3 || object_of(s, f.input, -1)),
        _ => true,
    };
}
// An activation (light-cljs-813, source::Activation): its runs of Frame,
// value and Saved rows; its frames bottom first, the first at the run's
// bases and every base within the run, never below the frame's under it; a
// bytecode frame's code, pc, locals and arguments, a native frame's
// operation; and its handlers' frames and heights.
fn activation_valid<T>(s: const T&, a: source::Activation, c: Control&) -> bool {
    let locals: i64 = a.locals; let operands: i64 = a.operands; let count: i64 = a.frame_count;
    if a.frame_count > a.frame_capacity || a.handler_count > a.handler_capacity || locals + operands > a.capacity || !range(a.values, a.capacity, s.values.len) { return reject(c, Error.Link); }
    if !cont_run(s, a.frames, a.frame_capacity, 4) || !cont_run(s, a.handlers, a.handler_capacity, 1) { return reject(c, Error.Link); }
    if count == 0 && (locals != 0 || operands != 0 || a.handler_count != 0) { return reject(c, Error.Link); }
    if !poll(c, count + a.handler_count) { return false; }
    var below = source::SavedFrame {};
    for i in count {
        let f = match s.conts[a.frames + i] { Frame y => y.f, _ => source::SavedFrame {} };
        if i == 0 && (f.local_base != 0 || f.operand_base != 0) || i > 0 && (f.local_base < below.local_base || f.operand_base < below.operand_base) || f.local_base > a.locals || f.operand_base > a.operands { return reject(c, Error.Link); }
        let end: i64 = if i + 1 < count { match s.conts[a.frames + i + 1] { Frame y => y.f.local_base, _ => 0 } } else { locals };
        let span = end - f.local_base;
        let native = match f.native { None => false, _ => true };
        if f.function == source::no_row() {
            if !native || f.argument_count < 0 || !native_valid(s, f) { return reject(c, Error.Link); }
        } else {
            if f.function >= s.functions.len || native { return reject(c, Error.Link); }
            let code = s.functions[f.function];
            if f.pc < code.start || f.pc >= code.start + code.count || span < code.locals || f.argument_count > span - code.locals { return reject(c, Error.Link); }
        }
        below = f;
    }
    for i in a.handler_count {
        let h = match s.conts[a.handlers + i] { Saved y => y.h, _ => source::SavedHandler {} };
        if h.frame >= a.frame_count || h.locals > a.locals || h.operands > a.operands { return reject(c, Error.Link); }
    }
    return true;
}
// Every row's links: an activation's (activation_valid); a frame's values;
// a handler's value; a promise's result, reactions and activation; a
// reaction's promise and next reaction.
fn conts_valid<T>(s: const T&, c: Control&) -> bool {
    for row in s.conts {
        if !poll(c) { return false; }
        match row {
            Activation x => { if !activation_valid(s, x.a, c) { return false; } },
            // A frame's values; the activation that runs it checks the rest.
            Frame x => { let f = x.f; if !value(s, f.callee) || !value(s, f.receiver) || !value(s, f.input) || !value(s, f.output) { return reject(c, Error.Link); } },
            // An async function's boundary names its promise.
            Saved x => { if !value(s, x.h.value) || x.h.pc < -1 || x.h.kind == source::HandlerKind.Async && (!x.h.value.is_object() || s.nodes[x.h.value.id()].kind != 21) { return reject(c, Error.Link); } },
            Promise x => {
                let p = x.p;
                if p.state > 2 || !value(s, p.result) || !link_or_none(s, p.first, 3) || !link_or_none(s, p.last, 3) || (p.first == source::no_row()) != (p.last == source::no_row()) || p.state != 0 && p.first != source::no_row() || !link_or_none(s, p.activation, 0) { return reject(c, Error.Link); }
                // The pending reactions end, at `last`, within the table.
                var at = p.first; var tail = source::no_row(); var steps: i64 = 0;
                while at != source::no_row() {
                    if !poll(c) { return false; }
                    steps += 1; if steps > s.conts.len { return reject(c, Error.Link); }
                    tail = at; at = match s.conts[at] { Reaction y => y.r.next, _ => source::no_row() };
                }
                if tail != p.last { return reject(c, Error.Link); }
            },
            Reaction x => {
                let r = x.r;
                if r.target >= s.nodes.len || s.nodes[r.target].kind != 21 || !link_or_none(s, r.next, 3) { return reject(c, Error.Link); }
            },
        }
    }
    return true;
}
fn interned<T>(s: const T&) -> i64 { var n: i64 = 0; for e in s.keys { if e.row != source::no_row() { n += 1; } } return n; }
// A heap string run equals these UTF-16 units.
fn same_units<T>(s: const T&, r: source::Str, units: const u16[:]) -> bool {
    if r.wide { return s.units[r.start..(r.start + r.len)] == units; }
    for b, i in s.latin1[r.start..(r.start + r.len)] { if units[i] != b { return false; } }
    return true;
}
fn ends<T>(s: const T&) -> i64[21] {
    return [s.code_bytes.len, s.functions.len, s.code_constants.len, s.children.len, s.ops.len, s.code_units.len,
            s.nodes.len, s.values.len, s.properties.len, s.units.len, s.constants.len, s.bytes.len, s.sources.len, s.bindings.len, s.code_strings.len, s.strings.len, s.latin1.len, s.shapes.len, s.accessors.len, s.keys.len, s.conts.len];
}
fn payload_bytes<T>(s: const T&) -> i64 {
    return bytes_of(s.code_bytes).len + bytes_of(s.functions).len + bytes_of(s.code_constants).len + bytes_of(s.children).len + bytes_of(s.ops).len + bytes_of(s.code_units).len
         + bytes_of(s.nodes).len + bytes_of(s.values).len + bytes_of(s.properties).len + bytes_of(s.units).len + bytes_of(s.constants).len + bytes_of(s.bytes).len + bytes_of(s.sources).len + bytes_of(s.bindings).len + bytes_of(s.latin1).len + bytes_of(s.code_strings).len + bytes_of(s.strings).len
         + bytes_of(s.shapes).len + bytes_of(s.accessors).len + bytes_of(s.keys).len + bytes_of(s.conts).len;
}
fn footprint<T>(s: const T&) -> i64 { let empty: Snapshot[] = [Snapshot {}]; return bytes_of(empty).len + payload_bytes(s); }
fn value<T>(s: const T&, v: source::Value, code: bool = false, constant: bool = false) -> bool {
    // Symbol and BigInt constants carry no runtime value (source::load).
    return match v {
        // A well-known symbol is also a row value (the realm's Symbol
        // properties, light-cljs-788).
        Symbol x => export_well_known_symbol(x.s).len > 0,
        BigInt x => (code || constant) && x.b == 0,
        Empty, Accessor => false,
        Object o => !code && range(o.o, 1, s.nodes.len),
        String x => range(x.s, 1, if code { s.code_strings.len } else { s.strings.len }),
        _ => true,
    };
}
fn code_valid<T>(s: const T&, c: Control&) -> bool {
    if s.functions.len == 0 || s.functions.len > 65536 || s.code_bytes.len > 8388608 { return reject(c, Error.Code); }
    for r in s.code_strings { if !poll(c) { return false; } if !range(r.start, r.len, s.code_units.len) { return reject(c, Error.Code); } }
    for v in s.code_constants { if !poll(c) { return false; } if !value(s, v, true) { return reject(c, Error.Code); } }
    for id in s.children { if !poll(c) { return false; } if !range(id, 1, s.functions.len) { return reject(c, Error.Code); } }
    if !utf8(s.code_bytes) { return reject(c, Error.Code); }
    if s.ops.len > c.bytes - footprint(s) { return reject(c, Error.Bytes); }
    c.validation_peak = max(c.validation_peak, s.ops.len);
    var used: bool[>..] = []; used.resize(s.ops.len, false);
    var next: i64       = 0;
    for f in s.functions {
        if !poll(c) { return false; }
        if f.count <= 0 || !range(f.start, f.count, s.ops.len) || !range(f.constants, 0, s.code_constants.len) || !range(f.children, 0, s.children.len) || f.params < 0 || f.params > f.locals || f.locals > 65535 || f.captures < 0 || f.captures > 65535 || f.kind < 0 || f.kind > constructor::DefaultDerivedClass || !range(f.name_start, f.name_len, s.code_units.len) || f.length > 65535 { return reject(c, Error.Code); }
        if s.ops[f.start].depth != 0 { return reject(c, Error.Code); }
        var offset: i64 = 0;
        for pc in f.start..(f.start + f.count) {
            if !poll(c) { return false; }
            if used[pc] { return reject(c, Error.Code); } used[pc] = true;
            let op = s.ops[pc]; let descriptor = export_descriptor(op.code);
            if descriptor < 0 || op.direct_eval && !export_direct_eval_call(op.code) || op.arg < 0 || op.offset != offset || op.start < 0 || op.end < op.start || !range(op.start, op.end - op.start, s.code_bytes.len) || op.depth < -1 || op.depth > 65536 { return reject(c, Error.Code); }
            if !program::boundary(s.code_bytes, op.start) || !program::boundary(s.code_bytes, op.end) { return reject(c, Error.Code); }
            let kind = (descriptor >> 8) & 255; let width = descriptor >> 16; offset += width + 1;
            if width == 0 && op.arg != 0 || width == 2 && op.arg > 65535 { return reject(c, Error.Code); }
            if kind == operand::Const && !range(op.arg, 1, s.code_constants.len - f.constants) || kind == operand::Local && op.arg >= f.locals || kind == operand::Function && !range(op.arg, 1, s.children.len - f.children) || kind == operand::Capture && op.arg >= f.captures { return reject(c, Error.Code); }
            if kind == operand::Const && export_name_operand(op.code) && !s.code_constants[f.constants + op.arg].is_string() { return reject(c, Error.Code); }
            if kind == operand::Target && !range(op.arg - f.start, 1, f.count) || op.code == opcode::FormOp && export_form_arity(op.arg) < 0 || kind == operand::Immediate && !program::selector(op.code, op.arg) { return reject(c, Error.Code); }
            // Check stack flow without allocating another decoded Code owner.
            let pop, push = effect(s, f, op);
            if pop < 0 || push < 0 { return reject(c, Error.Code); }
            if op.depth >= 0 {
                if op.depth < pop || op.depth - pop + push > 65536 { return reject(c, Error.Code); }
                let next_flow = source::flow(op.code);
                if next_flow != flow::End {
                    for edge in (if next_flow == flow::Branch || next_flow == flow::Catch { 2 } else { 1 }) {
                        let target = if next_flow != flow::Next && edge == 0 { op.arg } else { pc + 1 };
                        let extra = if next_flow == flow::Catch && edge == 0 { 1 } else { 0 };
                        if !range(target - f.start, 1, f.count) || s.ops[target].depth != op.depth - pop + push + extra { return reject(c, Error.Code); }
                    }
                }
            }
        }
        next += f.count;
    }
    return if next == s.ops.len { true } else { reject(c, Error.Code) };
}
fn utf8(bytes: const u8[:]) -> bool {
    var remaining: i64 = 0; var cp: i64 = 0; var minimum: i64 = 0;
    for byte in bytes {
        if remaining == 0 {
            if byte < 128 { continue; }
            if byte >= 194 && byte <= 223 { remaining = 1; cp = byte & 31; minimum = 128; }
            else if byte >= 224 && byte <= 239 { remaining = 2; cp = byte & 15; minimum = 2048; }
            else if byte >= 240 && byte <= 244 { remaining = 3; cp = byte & 7; minimum = 65536; }
            else { return false; }
        } else {
            if (byte & 192) != 128 { return false; }
            cp = cp * 64 + (byte & 63); remaining -= 1;
            if remaining == 0 && (cp < minimum || cp > 1114111 || cp >= 55296 && cp <= 57343) { return false; }
        }
    }
    return remaining == 0;
}
// The generated table source::stack_effect reads; its only data-dependent
// count (child captures) is resolved after validating the child link above.
fn effect<T>(s: const T&, f: source::Function, op: source::Op) -> i64, i64 {
    let e = export_stack_effect(op.code);
    let captures = if e >= 0 && ((e >> 24) & 15) == count::Captures { s.functions[s.children[f.children + op.arg]].captures } else { 0 };
    return source::effect_word(e, op, captures);
}
fn validate<T>(s: const T&, c: Control&) -> bool {
    if !poll(c) { return false; }
    if s.header.version != 2 { return reject(c, Error.Version); }
    if s.header.allocator != 1 { return reject(c, Error.Allocator); }
    if s.header.base <= 0 || s.header.base > 2147483647 || s.header.checkpoint <= 0 || s.header.checkpoint > 2147483647 || s.header.checkpoint == s.header.base { return reject(c, Error.Identity); }
    let lengths = ends(s);
    if lengths != s.header.ends { return reject(c, Error.Length); }
    for n in lengths { if n < 0 || n > 8388608 { return reject(c, Error.Length); } }
    c.snapshot_bytes = footprint(s);
    if c.snapshot_bytes > c.bytes { return reject(c, Error.Bytes); }
    if !poll(c, s.code_bytes.len + s.code_units.len + s.units.len + s.latin1.len + s.bytes.len) { return false; }
    let l = s.header.limits;
    if l.nodes < 0 || l.values < 0 || l.units < 0 || l.properties < 0 || l.shapes < 0 || l.work < 0 || l.steps < 0 || l.frames < 0 || l.stack < 0 || s.header.work < 0 || s.header.work > l.work || s.header.property_rows < 0 || s.header.property_rows > s.properties.len || s.header.boot < 0 || s.header.boot > 1 || s.header.initializers < 0 || s.header.producers < 0 { return reject(c, Error.Allocator); }
    if !code_valid(s, c) { return false; }
    if !value(s, s.header.global) || !s.header.global.is_object() || s.nodes[s.header.global.id()].kind != 0 { return reject(c, Error.Link); }
    if !value(s, s.header.call_method) || !s.header.call_method.is_object() || s.nodes[s.header.call_method.id()].kind != 4 || s.nodes[s.header.call_method.id()].function != 14 { return reject(c, Error.Link); }
    if !source::realm_valid(s.nodes, s.accessors, s.header.global, s.header.call_method) { return reject(c, Error.Link); }
    // A header names a run of its own table; a wide run must need two bytes
    // (some unit above 255), so the encoding stays canonical (source::Str).
    for r in s.strings {
        if !poll(c) { return false; }
        if !range(r.start, r.len, if r.wide { s.units.len } else { s.latin1.len }) { return reject(c, Error.Link); }
        if r.wide && (!poll(c, r.len) || source::fits_latin1(s.units[r.start..(r.start + r.len)])) { return reject(c, Error.Link); }
    }
    // Holes (Empty) may appear only inside array element runs, counted below.
    var holes: i64 = 0;
    for v in s.values { if !poll(c) { return false; } if v.is_hole() { holes += 1; continue; } if !slot_value(s, v) { return reject(c, Error.Link); } }
    if s.constants.len != s.code_constants.len { return reject(c, Error.Length); }
    for v, i in s.constants {
        if !poll(c) { return false; }
        let original = s.code_constants[i];
        if !value(s, v, false, true) { return reject(c, Error.Link); }
        // Live compiler sessions append code after runtime strings exist.
        // String constants keep exact UTF-16 contents, but their runtime range
        // need not have the same offset as the immutable code's string pool.
        if original.is_string() {
            let o = s.code_strings[original.sid()];
            if !poll(c, o.len) { return false; }
            if !v.is_string() { return reject(c, Error.Link); }
            let r = s.strings[v.sid()];
            if r.len != o.len || !same_units(s, r, s.code_units[o.start..(o.start + o.len)]) { return reject(c, Error.Link); }
        } else if !source::same_bits(v, original) { return reject(c, Error.Link); }
    }
    for p, i in s.properties {
        if !poll(c) { return false; }
        // A row is a record (interned key) or cleared; key runs are checked below.
        if !p.key.is_undefined() && !key_value(s, p.key) || !slot_value(s, p.value) { return reject(c, Error.Link); }
    }
    for r in s.sources {
        if !source::source_row_ok(r, s.bytes.len) { return reject(c, Error.Link); }
        if !poll(c, r.count + 1) { return false; }
        if source::has_text(r) && !utf8(s.bytes[r.start..(r.start + r.count)]) { return reject(c, Error.Link); }
    }
    var has_memo = false; var array_holes: i64 = 0;
    for n in s.nodes {
        if !poll(c) { return false; }
        if n.kind < 0 || n.kind > 22 { return reject(c, Error.Unsupported); }
        // A primitive wrapper (22, light-cljs-832) holds a number, string or
        // boolean and keeps no element run.
        if n.kind == 22 && (source::wrapper_type(n.value) < 0 || n.count != 0 || n.start != 0) { return reject(c, Error.Link); }
        // A generator (20) names its activation or nothing and has a state,
        // 0 to 3; a promise (21) names its state row (light-cljs-790).
        if n.kind == 20 && (n.length > 3 || n.function != source::no_row() && !cont_is(s, n.function, 0) || n.length != 3 && n.function == source::no_row()) || n.kind == 21 && !cont_is(s, n.function, 2) { return reject(c, Error.Link); }
        // An atom or volatile (17) has a flavor, 0 or 1; a cell reference (18,
        // no state of its own) 0, 1 or 2, a delay, whose value is its thunk
        // or nothing; a job's own delay (19) none (source::Node, cells.goose).
        if n.kind == 17 && n.length > 1 || n.kind == 18 && (n.length > 2 || !n.value.is_undefined() && (n.length != 2 || !n.value.is_object()) || n.function > 2147483647) || n.kind == 19 && n.length != 0 { return reject(c, Error.Link); }
        if n.kind == 1 && (n.length > l.values || n.length > 4294967295) { return reject(c, Error.Allocator); }
        // A key list (source::Node kind 16): its walk position lies in its
        // records, and it walks an object or nothing.
        if n.kind == 0 && n.length != 0 || n.kind == 16 && (n.count != 0 || n.initialized || n.start > n.length || n.length > n.key_count || !n.value.is_undefined() && !n.value.is_object()) { return reject(c, Error.Link); }
        if !value(s, n.value) || !value(s, n.namespace_value) || !value(s, n.metadata) || n.source != source::no_row() && n.source >= s.sources.len || n.origin < -1 || !n.proto.is_hole() && !n.proto.is_null() && (!n.proto.is_object() || !value(s, n.proto)) { return reject(c, Error.Link); }
        has_memo = has_memo || shaped(n) && n.key_capacity > 0;
        if n.source != source::no_row() && (n.span_end < n.span_start || !range(n.span_start, n.span_end - n.span_start, s.sources[n.source].count)) { return reject(c, Error.Link); }
        // The compiler checks an identity source's UTF-8 boundaries.
        if n.source != source::no_row() && source::has_text(s.sources[n.source]) {
            let r = s.sources[n.source]; let text = s.bytes[r.start..(r.start + r.count)];
            if !program::boundary(text, n.span_start) || !program::boundary(text, n.span_end) { return reject(c, Error.Link); }
        }
        if n.kind == 2 && (!range(n.function, 1, s.functions.len) || n.count != s.functions[n.function].captures) { return reject(c, Error.Link); }
        if has_values(n.kind) { if !range(n.start, n.count, s.values.len) || n.kind == 7 && n.count % 2 != 0 { return reject(c, Error.Link); } }
        if elements(n.kind) {
            // A dictionary array (initialized) has records and no run; a dense
            // one has a run, and its named properties in a shape
            // (light-cljs-805, key_runs). An ordinary object's element run
            // (light-cljs-804) has no length.
            if n.kind == 1 && n.initialized && n.count != 0 { return reject(c, Error.Link); }
            has_memo = has_memo || n.count > 0;
            if !poll(c, n.count) { return false; }
            // Slots at or past an array's length are holes (source::array_length).
            for v, i in s.values[n.start..(n.start + n.count)] { if v.is_hole() { array_holes += 1; } else if n.kind == 1 && i >= n.length { return reject(c, Error.Link); } }
        }
        if n.kind == 15 {
            has_memo = has_memo || n.count > 0;
            if n.length > n.count { return reject(c, Error.Link); }
            let input = n.namespace_value;
            if input.tag() > value_tag::Null && (!input.is_object() || (s.nodes[input.id()].kind != 5 && s.nodes[input.id()].kind != 12) || s.nodes[input.id()].count != n.count) || input.tag() <= value_tag::Null && n.count != 0 { return reject(c, Error.Link); }
        }
        // A built-in function's code names a Builtin (source::BUILTINS); an
        // error object's, the catchable fault it was raised for or 0.
        if n.kind == 4 && (n.function >= source::builtin_count() || n.function == 9) || n.kind == 13 && n.function > 5 { return reject(c, Error.Unsupported); }
        if (n.kind == 9 || n.kind == 10) && (!n.value.is_string() || !n.namespace_value.is_null() && !n.namespace_value.is_string()) || n.kind == 11 && n.value.tag() > value_tag::String { return reject(c, Error.Link); }
        if !n.metadata.is_undefined() && !n.metadata.is_null() && (!n.metadata.is_object() || s.nodes[n.metadata.id()].kind != 7) { return reject(c, Error.Link); }
    }
    if has_memo && !memo_ranges(s, c) { return false; }
    if !property_model(s, c) || !key_runs(s, c) { return false; }
    // Activation runs hold no holes (source::detach refuses one), so every
    // hole is still an array element's.
    if !conts_valid(s, c) { return false; }
    if holes != array_holes { return reject(c, Error.Link); }
    var previous: i64 = -1;
    for b in s.bindings {
        if !poll(c) { return false; }
        if b.id <= previous || b.generation <= 0 || !value(s, b.value) { return reject(c, Error.Identity); } previous = b.id;
    }
    return true;
}
fn borrow(heap: const source::Heap&, bindings: const Binding[:], checkpoint: i64) -> View {
    let code .= heap.code;
    var v = View { header: Header { base: heap.identity, checkpoint: checkpoint, limits: heap.limits, work: heap.work, global: heap.global, call_method: heap.call_method, boot: heap.boot_count, initializers: heap.initializer_count, producers: heap.producer_count, property_rows: heap.property_rows },
        code_bytes: code.source_bytes, functions: code.functions, code_constants: code.constants, children: code.children, ops: code.ops, code_units: code.units, code_strings: code.strings,
        nodes:      heap.nodes, values: heap.values, properties: heap.properties, units: heap.units, strings: heap.strings, constants: heap.constants, bytes: heap.source_bytes, sources: heap.source_ranges, bindings: bindings, latin1: heap.latin1,
        shapes: heap.shapes, accessors: heap.accessors, keys: heap.keys, conts: heap.conts };
    v.header.ends = ends(v); return v;
}
// `live`: the heap's one job is running a built-in that only reads it (a
// function stored into a cell, cells.goose encode_callable), so its rows
// hold no half-made state.
fn capture(heap: const source::Heap&, bindings: const Binding[:], c: Control&, checkpoint: i64 = 0, live: bool = false) -> Snapshot {
    let started = clock_ns(); defer { c.capture_ns += clock_ns() - started; }
    if heap.live_jobs != 0 && !(live && heap.live_jobs == 1) || heap.fault != source::Fault.None || !heap.code.valid { reject(c, Error.Boundary); return Snapshot {}; }
    var s = borrow(heap, bindings, if heap.identity == 1 { 2 } else { 1 });
    if !validate(s, c) { return Snapshot {}; }
    // Snapshot identities and private arena identities share the coordinator's
    // non-reusing namespace. The authority itself is never in the image.
    // A live compiler worker receives its checkpoint from the serial admission
    // authority. Its copied global counter must never allocate session IDs.
    s.header.checkpoint = if checkpoint == 0 { source::reserve_identity() } else { checkpoint };
    if s.header.checkpoint <= 0 || s.header.checkpoint > 2147483647 || s.header.checkpoint == heap.identity { reject(c, Error.Identity); return Snapshot {}; }
    c.copied_bytes += footprint(s);
    return Snapshot { header: s.header, code_bytes: s.code_bytes, functions: s.functions, code_constants: s.code_constants, children: s.children, ops: s.ops, code_units: s.code_units, code_strings: s.code_strings,
        nodes: s.nodes, values: s.values, properties: s.properties, units: s.units, strings: s.strings, constants: s.constants, bytes: s.bytes, sources: s.sources, bindings: s.bindings, latin1: s.latin1, shapes: s.shapes, accessors: s.accessors, keys: s.keys, conts: s.conts };
}
// Reset only after the dispatch's job has retired. This reestablishes every
// restore destination invariant; restore still validates the entire image and
// requires a fresh identity. No references/roots may cross this boundary.
fn reset(code: source::Code&, heap: source::Heap&, bindings: Binding[>..]&) -> bool {
    if !(heap.code .== code) || heap.live_jobs != 0 { return false; }
    bindings.clear();
    heap.nodes.clear(); heap.values.clear(); heap.properties.clear(); heap.units.clear(); heap.latin1.clear(); heap.strings.clear(); heap.constants.clear(); heap.source_bytes.clear(); heap.source_ranges.clear();
    heap.shapes.clear(); heap.accessors.clear(); heap.keys.clear(); heap.interned = 0; heap.image_written = false; heap.conts.clear();
    code.source_bytes.clear(); code.functions.clear(); code.constants.clear(); code.children.clear(); code.ops.clear(); code.units.clear(); code.strings.clear(); code.ins.clear(); code.effects.clear(); code.heights.clear(); code.decoded = 0;
    code.valid = false; code.error = tiny::LoadError.None; code.error_pc = -1;
    heap.identity = 0; heap.limits = source::Limits {}; heap.work = 0; heap.fault = source::Fault.None; heap.global = source::Value.Undefined; heap.call_method = source::Value.Undefined;
    heap.boot_count = 0; heap.initializer_count = 0; heap.producer_count = 0; heap.property_rows = 0; heap.image = source::Tops {};
    return true;
}
fn restore(s: const Snapshot&, code: source::Code&, heap: source::Heap&, bindings: Binding[>..]&, identity: i64, c: Control&) -> bool {
    let started = clock_ns(); defer { c.restore_ns += clock_ns() - started; }
    if !(heap.code .== code) || heap.identity != 0 || heap.live_jobs != 0 || heap.nodes.len != 0 || heap.values.len != 0 || heap.properties.len != 0 || heap.units.len != 0 || heap.latin1.len != 0 || heap.strings.len != 0 || heap.constants.len != 0 || heap.source_bytes.len != 0 || heap.source_ranges.len != 0 || heap.shapes.len != 0 || heap.accessors.len != 0 || heap.keys.len != 0 || heap.conts.len != 0 || code.valid || code.source_bytes.len != 0 || code.functions.len != 0 || code.constants.len != 0 || code.children.len != 0 || code.ops.len != 0 || code.units.len != 0 || code.strings.len != 0 || bindings.len != 0 { return reject(c, Error.Destination); }
    if identity <= 0 || identity > 2147483647 || identity == s.header.base || identity == s.header.checkpoint { return reject(c, Error.Identity); }
    if !validate(s, c) { return false; }
    // No initialization, producer, publication cache, claim or waiter code.
    code.source_bytes.append(s.code_bytes); code.functions.append(s.functions); code.constants.append(s.code_constants); code.children.append(s.children); code.ops.append(s.ops); code.units.append(s.code_units); code.strings.append(s.code_strings); source::decode(code); code.valid = true;
    heap.identity = identity; heap.limits = s.header.limits; heap.work = s.header.work; heap.global = s.header.global; heap.call_method = s.header.call_method;
    heap.boot_count = s.header.boot; heap.initializer_count = s.header.initializers; heap.producer_count = s.header.producers; heap.property_rows = s.header.property_rows;
    // The image install, not an allocation: rows are copied verbatim into
    // empty tables (links count from each table's start, so the
    // destination's empty tables give every row its captured index); the
    // tables' one allocation functions (source_vm) add rows above it.
    heap.nodes.append(s.nodes); heap.values.append(s.values); heap.properties.append(s.properties);
    heap.units.append(s.units); heap.latin1.append(s.latin1); heap.strings.append(s.strings); heap.constants.append(s.constants);
    heap.source_bytes.append(s.bytes); heap.source_ranges.append(s.sources);
    heap.shapes.append(s.shapes); heap.accessors.append(s.accessors); heap.keys.append(s.keys); heap.interned = interned(s); heap.image_written = false; heap.conts.append(s.conts);
    // The restored rows are the image: table limits count rows added above it.
    heap.image = source::tops(heap);
    bindings.append(s.bindings);
    c.copied_bytes += payload_bytes(s); return true;
}
// A reused job worker (light-cljs-795; docs/javascript-coverage.md, decision
// 3) restores its image once, from the snapshot its spawn copied, and keeps
// that snapshot as the image's pristine twin. `service` names the image's
// node rows compiler services write (the roots object, the published-helper
// cache); with the property rows of their image key runs they are the only
// image rows a job changes, since JavaScript cannot write a row the job did
// not allocate (source::writable).
//
// After every job, truncate cuts every table back to the twin's length and
// restores the service rows and the heap's state fields from the twin. The
// next job's tables are then a fresh worker's, at the same rows.
fn truncate(twin: const Snapshot&, service: const i64[:], identity: i64, code: source::Code&, heap: source::Heap&, bindings: Binding[>..]&) -> bool {
    if !(heap.code .== code) || heap.live_jobs != 0 || !code.valid || code.functions.len < twin.functions.len || heap.nodes.len < twin.nodes.len { return false; }
    code.source_bytes.resize(twin.code_bytes.len); code.functions.resize(twin.functions.len); code.constants.resize(twin.code_constants.len); code.children.resize(twin.children.len);
    code.ops.resize(twin.ops.len); code.units.resize(twin.code_units.len); code.strings.resize(twin.code_strings.len);
    code.ins.resize(twin.ops.len); code.effects.resize(twin.ops.len); code.heights.resize(twin.functions.len); code.decoded = twin.functions.len;
    heap.nodes.resize(twin.nodes.len); heap.values.resize(twin.values.len); heap.properties.resize(twin.properties.len);
    heap.units.resize(twin.units.len); heap.latin1.resize(twin.latin1.len); heap.strings.resize(twin.strings.len); heap.constants.resize(twin.constants.len);
    heap.source_bytes.resize(twin.bytes.len); heap.source_ranges.resize(twin.sources.len);
    heap.shapes.resize(twin.shapes.len); heap.accessors.resize(twin.accessors.len); heap.conts.resize(twin.conts.len);
    for r in service { service_row(heap, twin, twin, r); }
    // A job's transitions from image shapes and its interned keys wrote the
    // image's shape rows and key index (source::Heap.image_written): both
    // come back whole from the twin.
    if heap.image_written { for e, i in twin.shapes { heap.shapes[i] = e; } heap.keys.clear(); heap.keys.append(twin.keys); heap.interned = interned(twin); heap.image_written = false; }
    state(twin.header, identity, heap);
    heap.image = source::tops(heap);
    bindings.clear(); bindings.append(twin.bindings);
    return true;
}
// Service row r and the property rows of its image runs (twin's: in values
// for a shape-mode object, in properties for a dictionary-mode one, and its
// element run in values, light-cljs-804), as `from` has them.
fn service_row(heap: source::Heap&, from: const Snapshot&, twin: const Snapshot&, r: i64) {
    heap.nodes[r] = from.nodes[r]; let n = twin.nodes[r];
    for k in n.keys..(n.keys + n.key_capacity) { if shaped(n) { heap.values[k] = from.values[k]; } else { heap.properties[k] = from.properties[k]; } }
    if n.kind == 0 { for k in n.start..(n.start + n.count) { heap.values[k] = from.values[k]; } }
}
// The heap's state fields, as a restore of this header sets them.
fn state(h: const Header&, identity: i64, heap: source::Heap&) {
    heap.identity = identity; heap.limits = h.limits; heap.work = h.work; heap.global = h.global; heap.call_method = h.call_method; heap.fault = source::Fault.None;
    heap.boot_count = h.boot; heap.initializer_count = h.initializers; heap.producer_count = h.producers; heap.property_rows = h.property_rows;
}
fn same_rows<T>(a: const T[:], b: const T[:]) -> bool { return a.len == b.len && bytes_of(a) == bytes_of(b); }
// The reset check: every table and state field equals the twin's, byte for
// byte (tests and BBP_RESET_CHECK=1).
fn pristine(twin: const Snapshot&, identity: i64, code: const source::Code&, heap: const source::Heap&, bindings: const Binding[:]) -> bool {
    let h = twin.header;
    if heap.identity != identity || !same_bits(heap.limits, h.limits) || heap.work != h.work || !source::same_bits(heap.global, h.global) || !source::same_bits(heap.call_method, h.call_method) || heap.fault != source::Fault.None || heap.live_jobs != 0 { return false; }
    if heap.boot_count != h.boot || heap.initializer_count != h.initializers || heap.producer_count != h.producers || heap.property_rows != h.property_rows || code.decoded != twin.functions.len || code.heights.len != twin.functions.len || code.ins.len != twin.ops.len { return false; }
    return same_rows(code.source_bytes, twin.code_bytes) && same_rows(code.functions, twin.functions) && same_rows(code.constants, twin.code_constants) && same_rows(code.children, twin.children) && same_rows(code.ops, twin.ops) && same_rows(code.units, twin.code_units) && same_rows(code.strings, twin.code_strings)
        && same_rows(heap.nodes, twin.nodes) && same_rows(heap.values, twin.values) && same_rows(heap.properties, twin.properties) && same_rows(heap.units, twin.units) && same_rows(heap.latin1, twin.latin1) && same_rows(heap.strings, twin.strings) && same_rows(heap.constants, twin.constants)
        && same_rows(heap.source_bytes, twin.bytes) && same_rows(heap.source_ranges, twin.sources) && same_rows(bindings, twin.bindings)
        && same_rows(heap.shapes, twin.shapes) && same_rows(heap.accessors, twin.accessors) && same_rows(heap.keys, twin.keys) && heap.interned == interned(twin) && same_rows(heap.conts, twin.conts);
}
fn same_bits(a: source::Limits, b: source::Limits) -> bool { let x: source::Limits[] = [a]; let y: source::Limits[] = [b]; return bytes_of(x) == bytes_of(y); }
// Whether a job snapshot's rows below the twin's lengths are the image's,
// except the service rows (a saved continuation carries the roots and cache
// rows its job's compiler services wrote). Tests and BBP_RESET_CHECK=1 only;
// otherwise this holds by construction.
fn over_image(s: const Snapshot&, twin: const Snapshot&, service: const i64[:]) -> bool {
    let a = ends(s); let b = ends(twin); for n, i in b { if a[i] < n { return false; } }
    var keys: i64[>..] = []; var slots: i64[>..] = [];
    for r in service { let n = twin.nodes[r]; for k in n.keys..(n.keys + n.key_capacity) { if shaped(n) { slots.push(k); } else { keys.push(k); } } if n.kind == 0 { for k in n.start..(n.start + n.count) { slots.push(k); } } }
    for row, i in twin.nodes { var skip = false; for r in service { if r == i { skip = true; } } if !skip && !same_rows(s.nodes[i..(i + 1)], twin.nodes[i..(i + 1)]) { return false; } }
    for row, i in twin.properties { var skip = false; for k in keys { if k == i { skip = true; } } if !skip && !same_rows(s.properties[i..(i + 1)], twin.properties[i..(i + 1)]) { return false; } }
    for row, i in twin.values { var skip = false; for k in slots { if k == i { skip = true; } } if !skip && !same_rows(s.values[i..(i + 1)], twin.values[i..(i + 1)]) { return false; } }
    // Image shapes keep their layout; only their newest child may differ.
    for e, i in twin.shapes { var x = s.shapes[i]; x.child = e.child; if !same_rows([x], [e]) { return false; } }
    if !same_rows(s.accessors[..twin.accessors.len], twin.accessors) || !same_rows(s.conts[..twin.conts.len], twin.conts) { return false; }
    return same_rows(s.code_bytes[..twin.code_bytes.len], twin.code_bytes) && same_rows(s.functions[..twin.functions.len], twin.functions) && same_rows(s.code_constants[..twin.code_constants.len], twin.code_constants) && same_rows(s.children[..twin.children.len], twin.children)
        && same_rows(s.ops[..twin.ops.len], twin.ops) && same_rows(s.code_units[..twin.code_units.len], twin.code_units) && same_rows(s.code_strings[..twin.code_strings.len], twin.code_strings)
        && same_rows(s.units[..twin.units.len], twin.units) && same_rows(s.latin1[..twin.latin1.len], twin.latin1) && same_rows(s.strings[..twin.strings.len], twin.strings)
        && same_rows(s.constants[..twin.constants.len], twin.constants) && same_rows(s.bytes[..twin.bytes.len], twin.bytes) && same_rows(s.sources[..twin.sources.len], twin.sources);
}
// Install a job's snapshot over the image (light-cljs-795): validate all of
// it, as restore does, then append only its rows above the twin's lengths
// and copy its service rows; rows below are the image's (over_image). The
// result equals restore's into empty tables, row for row, with this
// snapshot's state fields and the job's image tops at its lengths.
fn extend(s: const Snapshot&, twin: const Snapshot&, service: const i64[:], code: source::Code&, heap: source::Heap&, bindings: Binding[>..]&, identity: i64, c: Control&) -> bool {
    let started = clock_ns(); defer { c.restore_ns += clock_ns() - started; }
    if !(heap.code .== code) || heap.live_jobs != 0 { return reject(c, Error.Destination); }
    let current = borrow(heap, bindings, 1); if ends(current) != twin.header.ends { return reject(c, Error.Destination); }
    if identity <= 0 || identity > 2147483647 || identity == s.header.base || identity == s.header.checkpoint { return reject(c, Error.Identity); }
    if !validate(s, c) { return false; }
    let a = ends(s); let b = twin.header.ends; for n, i in b { if a[i] < n { return reject(c, Error.Destination); } }
    code.source_bytes.append(s.code_bytes[twin.code_bytes.len..]); code.functions.append(s.functions[twin.functions.len..]); code.constants.append(s.code_constants[twin.code_constants.len..]); code.children.append(s.children[twin.children.len..]);
    code.ops.append(s.ops[twin.ops.len..]); code.units.append(s.code_units[twin.code_units.len..]); code.strings.append(s.code_strings[twin.code_strings.len..]); source::decode(code);
    state(s.header, identity, heap);
    heap.nodes.append(s.nodes[twin.nodes.len..]); heap.values.append(s.values[twin.values.len..]); heap.properties.append(s.properties[twin.properties.len..]);
    heap.units.append(s.units[twin.units.len..]); heap.latin1.append(s.latin1[twin.latin1.len..]); heap.strings.append(s.strings[twin.strings.len..]); heap.constants.append(s.constants[twin.constants.len..]);
    heap.source_bytes.append(s.bytes[twin.bytes.len..]); heap.source_ranges.append(s.sources[twin.sources.len..]);
    heap.accessors.append(s.accessors[twin.accessors.len..]); heap.conts.append(s.conts[twin.conts.len..]);
    // The job's shapes and key index replace the image's whole (truncate
    // restores the image's).
    heap.shapes.clear(); heap.shapes.append(s.shapes); heap.keys.clear(); heap.keys.append(s.keys); heap.interned = interned(s); heap.image_written = true;
    for r in service { service_row(heap, s, twin, r); }
    heap.image = source::tops(heap);
    bindings.clear(); bindings.append(s.bindings);
    c.copied_bytes += payload_bytes(s) - payload_bytes(twin); return true;
}
// Optional local codec: Goose's checked flat image format plus our semantic
// version/allocator checks. This is compiler-ABI-specific, not a wire standard.
fn encode(s: const Snapshot&, out: u8[>..]&, c: Control&) -> bool {
    if !validate(s, c) { return false; }
    let before = out.len; let records: Snapshot[] = [copy(s)]; to_bytes(records, out);
    // [s] is an intermediate full copy, followed by the serialization copy.
    c.copied_bytes += footprint(s) + out.len - before; return true;
}
fn decode(bytes: const u8[:], c: Control&) -> Snapshot[>..] {
    if bytes.len > c.bytes { reject(c, Error.Bytes); return []; }
    if !poll(c, bytes.len) { return []; }
    var records, ok = from_bytes<Snapshot[>..]>(bytes);
    if !ok || records.len != 1 { reject(c, Error.Length); return []; }
    c.copied_bytes += bytes_of(records).len;
    for s in records { if !validate(s, c) { return []; } }
    c.copied_bytes += bytes_of(records).len; return copy(records);
}
// Admission accounting for a finite batch, not a scheduling system. Reserve
// before spawn, release only after thread_wait has released private storage.
struct Resources { limit: i64 = 268435456, workers: i64 = 2, active: i64 = 0, retained: i64 = 0, peak: i64 = 0 }
fn reserve(r: Resources&, bytes: i64) -> bool {
    if bytes < 0 || r.active >= r.workers || bytes > r.limit - r.retained { return false; }
    r.active += 1; r.retained += bytes; r.peak = max(r.peak, r.retained); return true;
}
fn release(r: Resources&, bytes: i64) { assert(r.active > 0 && bytes >= 0 && bytes <= r.retained); r.active -= 1; r.retained -= bytes; }
