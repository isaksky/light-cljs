import std;
import os;
import math;
import .source_vm;
import .export_schema;
import .program;
import .private;

namespace snapshot;

// This is a private copy, not immutable artifact publication. All components
// are flat owned arrays, and every graph edge is a row index into the image's
// own tables: no value carries a session, so restore copies rows verbatim.
// Allocator 1 is the current append-only arena: its complete metadata is the
// fourteen end cursors. There are deliberately no fictional free lists.
struct Binding { id: i64, generation: i64, value: source::Value }
// Version 3 (light-cljs-858) adds the code's direct-eval sites, ends 21..23;
// version 4 (light-cljs-932) its regular expressions, ends 24 and 25.
struct Header {
    version: i64     = 4, allocator: i64 = 1, base: i64 = 0, checkpoint: i64 = 1,
    ends:    i64[26] = [0; 26], limits: source::Limits = source::Limits {},
    work:    i64     = 0, global: source::Value = source::Value.Undefined, call_method: source::Value = source::Value.Undefined,
    boot:    i64     = 0, initializers: i64 = 0, producers: i64 = 0, property_rows: i64 = 0,
    // The Symbol.for registry's newest symbol (source::Heap.registry,
    // light-cljs-885), or Null.
    registry: source::Value = source::Value.Null,
    // The Math.random stream's state (source::Heap.random, light-cljs-901):
    // every value is a valid state.
    random: u64 = 0,
}
struct Snapshot {
    header:     Header             = Header {},
    code_bytes: u8[]               = [], functions: source::Function[] = [], code_constants: source::Value[] = [],
    children:   u32[]              = [], ops: source::Op[] = [], code_units: u16[] = [], code_strings: source::Str[] = [],
    nodes:      source::Node[]     = [], values: source::Value[] = [], properties: source::Property[] = [],
    units:      u16[]              = [], strings: source::Str[] = [], constants: source::Value[] = [], bytes: u8[] = [], sources: source::SourceRange[] = [], bindings: Binding[] = [], latin1: u8[] = [],
    shapes:     source::Shape[]    = [], accessors: source::Pair[] = [], keys: source::KeySlot[] = [], conts: source::Cont[] = [],
    sites:      source::Site[]     = [], site_sources: source::SiteSource[] = [], site_records: u8[] = [],
    patterns:   program::Pattern[] = [], pattern_ops: program::PatternOp[] = [],
}
// A borrowing counterpart lets capture validate before allocating a copy.
struct View {
    header:     Header,
    code_bytes: const u8[:], functions: const source::Function[:], code_constants: const source::Value[:],
    children:   const u32[:], ops: const source::Op[:], code_units: const u16[:], code_strings: const source::Str[:],
    nodes:      const source::Node[:], values: const source::Value[:], properties: const source::Property[:],
    units:      const u16[:], strings: const source::Str[:], constants: const source::Value[:], bytes: const u8[:], sources: const source::SourceRange[:], bindings: const Binding[:], latin1: const u8[:],
    shapes:     const source::Shape[:], accessors: const source::Pair[:], keys: const source::KeySlot[:], conts: const source::Cont[:],
    sites:      const source::Site[:], site_sources: const source::SiteSource[:], site_records: const u8[:],
    patterns:   const program::Pattern[:], pattern_ops: const program::PatternOp[:],
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
// array's element run (light-cljs-804), captures, form children, arguments,
// a deferred map's memo, a bound function's arguments (light-cljs-852) and
// an arguments map's cells (light-cljs-860), and a Map's or Set's entries
// and index (light-cljs-943).
fn has_values(kind: i64) -> bool { return kind <= node_kind::Closure || kind >= node_kind::List && kind <= node_kind::Set || kind == node_kind::Concat || kind == node_kind::Arguments || kind == node_kind::LazySeq || kind == node_kind::BoundFunction || kind == node_kind::ArgumentMap || kind == node_kind::StructType || kind == node_kind::Struct || kind == node_kind::InitializerSlot || kind == node_kind::Collection; }
fn map_row<S>(s: const S&, v: source::Value) -> bool { return v.is_object() && v.id() < s.nodes.len && s.nodes[v.id()].kind == node_kind::Map; }
// Whether row n is a shareable regex value laid out as
// regexp_object.goose's new_shared_regexp lays one out (light-cljs-935):
// immutable and not extensible, its one own property `lastIndex` 0 in the
// realm's RegExp template shape, %RegExp.prototype% its prototype, and a
// program of the code's without `g` or `y`.
fn shared_regexp_layout<S>(s: const S&, n: source::Node) -> bool {
    let t = source::regexp_template_row();
    if n.kind != node_kind::RegExp || !n.immutable || !n.non_extensible || n.initialized || t >= s.nodes.len || n.function >= s.patterns.len { return false; }
    return n.shape == s.nodes[t].shape && n.key_count == 1 && n.keys == 0 && n.key_capacity == 0 && source::same_bits(n.proto, source::object(source::regexp_prototype_row())) && source::same_bits(n.slots[0], source::number(0)) && n.slots[1].is_undefined() && n.metadata.is_undefined() && !source::stateful_pattern(s.patterns[n.function]);
}
// Whether struct row n (light-cljs-920) has a struct type of as many fields,
// is immutable unless that type is Mutable, and has an extension map or
// metadata (form maps) only as a record; no other link or flag.
fn struct_row<S>(s: const S&, n: source::Node) -> bool {
    if !n.value.is_object() || n.value.id() >= s.nodes.len { return false; }
    let t = s.nodes[n.value.id()];
    if t.kind != node_kind::StructType || n.count != t.count / 2 || n.immutable == (t.length == type_flavor::Mutable) { return false; }
    let record = t.length == type_flavor::Record;
    if !n.namespace_value.is_null() && !(record && map_row(s, n.namespace_value)) || !n.metadata.is_undefined() && !(record && map_row(s, n.metadata)) { return false; }
    return n.proto.is_hole() && n.keys == 0 && n.key_count == 0 && n.key_capacity == 0 && n.shape == 0 && n.function == 0 && n.length == 0 && !n.initialized && !n.non_extensible;
}
// An element run (an ordinary object's, an arguments object's or an array's)
// is exclusive and may hold holes.
fn elements(kind: i64) -> bool { return kind == node_kind::Object || kind == node_kind::Arguments || kind == node_kind::Array; }
// All ranges have already passed bounds validation. Immutable views may share
// slots, but an unrealized map owns its entire writable memo exclusively,
// including the completed prefix. Charge both scratch initialization and each
// range visit; overlapping immutable views cannot hide unbounded scan work.
// Private elements (light-cljs-889, source::private_holder): validate
// checked each holder's link (a later PrivateElements row) and counted
// `owners` links and `rows` such rows; here every row has exactly one
// owner. Only an image with private elements pays for the bitmap, charged
// like memo_ranges' scratch.
fn private_owners<T>(s: const T&, c: Control&, owners: i64, rows: i64) -> bool {
    if owners != rows { return reject(c, Error.Link); }
    if s.nodes.len > c.bytes - footprint(s) { return reject(c, Error.Bytes); }
    if !poll(c, s.nodes.len) { return false; }
    c.validation_peak = max(c.validation_peak, s.nodes.len);
    var owned: u8[>..] = []; owned.resize(s.nodes.len, 0);
    for n in s.nodes {
        if !poll(c) { return false; }
        if !n.metadata.is_object() || s.nodes[n.metadata.id()].kind != node_kind::PrivateElements { continue; }
        if owned[n.metadata.id()] != 0 { return reject(c, Error.Link); }
        owned[n.metadata.id()] = 1;
    }
    return true;
}
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
            // A mutable struct's fields and an initializer slot's value
            // (light-cljs-920) are written in place: exclusive too, as a
            // collection's run is (light-cljs-943).
            if n.kind == node_kind::LazySeq || elements(n.kind) || n.kind == node_kind::ArgumentMap || n.kind == node_kind::InitializerSlot || n.kind == node_kind::Struct && !n.immutable || n.kind == node_kind::Collection {
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
// Mapped arguments objects (light-cljs-860; source_vm.goose, "Mapped
// arguments objects"): an arguments object names nothing or its map, and
// the map (an exclusive run, memo_ranges) names that object back and
// nothing else, so each map is scanned once, its entries charged to the
// work budget; each entry is Undefined or a mapped cell naming that object
// and its index. A mapped cell names an arguments object whose map lists it
// at its index, is initialized and mutable and has no run, and its
// property is where the VM reads it: an element present in the object's
// run, or, when the cell keeps an interned key (a redefinition moved the
// property), the decimal text of its index, with no element left in the
// run (the shape key or record itself is checked when the VM uses it).
// Rows the checks follow are range-checked first.
fn argument_links<T>(s: const T&, n: const source::Node&, row: i64, c: Control&) -> bool {
    if n.kind == node_kind::Arguments && !n.namespace_value.is_null() {
        if !object_of(s, n.namespace_value, node_kind::ArgumentMap) { return false; }
        let m = s.nodes[n.namespace_value.id()];
        if !(m.value.is_object() && m.value.id() == row) || !range(m.start, m.count, s.values.len) || !poll(c, m.count) { return false; }
        for v, i in s.values[m.start..(m.start + m.count)] {
            if v.is_undefined() { continue; }
            if !object_of(s, v, node_kind::MappedBinding) { return false; }
            let cell = s.nodes[v.id()];
            if !(cell.value.is_object() && cell.value.id() == row) || cell.length != i { return false; }
        }
    }
    if n.kind == node_kind::ArgumentMap {
        if !object_of(s, n.value, node_kind::Arguments) || !n.namespace_value.is_null() || n.function != 0 || n.length != 0 || n.initialized { return false; }
        let o = s.nodes[n.value.id()];
        if !(o.namespace_value.is_object() && o.namespace_value.id() == row) { return false; }
    }
    if n.kind == node_kind::MappedBinding {
        if !n.initialized || n.immutable || n.start != 0 || n.count != 0 || !object_of(s, n.value, node_kind::Arguments) { return false; }
        let o = s.nodes[n.value.id()];
        if !object_of(s, o.namespace_value, node_kind::ArgumentMap) { return false; }
        let m = s.nodes[o.namespace_value.id()];
        if n.length >= m.count || !range(m.start, m.count, s.values.len) || !range(o.start, o.count, s.values.len) { return false; }
        let entry = s.values[m.start + n.length];
        if !(entry.is_object() && entry.id() == row) { return false; }
        let present = n.length < o.count && !s.values[o.start + n.length].is_hole();
        if n.namespace_value.is_null() { return present; }
        return !present && key_value(s, n.namespace_value) && index_text(s, n.namespace_value, n.length);
    }
    return true;
}
// Whether interned key k is the canonical decimal text of index i.
fn index_text<T>(s: const T&, k: source::Value, i: i64) -> bool {
    if !k.is_string() { return false; }
    let h = s.strings[k.sid()];
    if h.wide || h.len == 0 || h.len > 10 || !range(h.start, h.len, s.latin1.len) { return false; }
    var v: i64 = 0;
    for b, j in s.latin1[h.start..(h.start + h.len)] {
        if b < 48 || b > 57 || j == 0 && b == 48 && h.len > 1 { return false; }
        v = v * 10 + (b as i64) - 48;
    }
    return v == i;
}
// The kinds with own properties by name (source::Node): ordinary objects,
// arrays and closures (light-cljs-805), built-in functions and error objects
// (light-cljs-788), generators, promises and primitive wrappers, iterators,
// bound functions and arguments objects (light-cljs-854).
fn named(kind: i64) -> bool { return kind >= node_kind::Object && kind <= node_kind::Closure || kind == node_kind::Arguments || kind == node_kind::Builtin || kind == node_kind::Error || kind == node_kind::Generator || kind == node_kind::Promise || kind == node_kind::Wrapper || kind == node_kind::Iterator || kind == node_kind::RegExp || kind == node_kind::Collection || kind == node_kind::BoundFunction; }
// A shape-mode object, dense array or closure: without `initialized`.
fn shaped(n: const source::Node&) -> bool { return named(n.kind) && !n.initialized; }
// A dictionary-mode object, a dictionary array or a closure's records.
fn dictionary(n: const source::Node&) -> bool { return named(n.kind) && n.initialized; }
// A run of property records: a dictionary's, or a key list's (node_kind::KeyList, the
// keys a for-in walk or an object spread visits; source::Node).
fn record_run(n: const source::Node&) -> bool { return dictionary(n) || n.kind == node_kind::KeyList || n.kind == node_kind::PrivateElements; }
// A property key: an interned string (source::intern) or a symbol, a
// well-known one or a symbol row (light-cljs-885).
fn key_value<T>(s: const T&, k: source::Value) -> bool {
    return match k { String x => range(x.s, 1, s.strings.len) && s.strings[x.s].key, Symbol x => source::symbol_payload(s.nodes, x.s), _ => false };
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
            // A private element's key is a private name, a symbol row, and
            // its only attribute a field's writable bit (light-cljs-889).
            let private = n.kind == node_kind::PrivateElements && (source::symbol_node(p.key) < 0 || p.attrs > source::attr_writable() || p.attrs != 0 && source::is_accessor(p.value));
            if (if row < n.keys + n.key_count { private || !key_value(s, p.key) || !slot_value(s, p.value) || p.attrs > 7 } else { !p.key.is_undefined() || !p.value.is_undefined() || p.attrs != 0 }) { return reject(c, Error.Link); }
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
// Whether node kind `kind` can be a bound function's target: a closure, a
// built-in, a proxy (whose [[Call]] decides) or a bound function.
fn callable_kind(kind: i64) -> bool { return kind == node_kind::Closure || kind == node_kind::Builtin || kind == node_kind::Proxy || kind == node_kind::BoundFunction; }
// Whether v is an object of node kind `kind` (-1: any kind).
fn object_of<T>(s: const T&, v: source::Value, kind: i64) -> bool { return v.is_object() && v.id() < s.nodes.len && (kind == node_kind::None || s.nodes[v.id()].kind == kind); }
// A native frame's state that its steps read by row: a Resume frame's
// generator, a MapSeq frame's deferred map, a Force frame's delay, a Swap
// frame's function and a CopyData frame's key list or proxy source.
fn native_valid<T>(s: const T&, f: source::SavedFrame) -> bool {
    return match f.native {
        Resume => object_of(s, f.input, node_kind::Generator),
        MapSeq => object_of(s, f.input, node_kind::LazySeq),
        Force => object_of(s, f.input, node_kind::None),
        Swap => object_of(s, f.callee, node_kind::None),
        CopyData => copy_frame_valid(s, f),
        // A HasInstance frame awaits one of its phases (light-cljs-834);
        // pc 4 is retired (light-cljs-900).
        HasInstance => f.pc >= 1 && f.pc <= 5 && f.pc != 4 && (f.pc != 5 || f.callee.is_undefined() || object_of(s, f.callee, node_kind::None)),
        // Array.prototype.push, join and toString and the search methods
        // (light-cljs-908) read their object (light-cljs-842).
        // The mutators (light-cljs-909; array_mutators.goose) hold their
        // argument count and are in one of their phases, the species ones
        // splice's, slice's, concat's and flat's, the handler ones
        // Array.from's (light-cljs-911; array_copy.goose); their step
        // function checks the scratch operands.
        ArrayMethod m => object_of(s, f.input, node_kind::None) && (!source::array_mutator(m.which) || f.index >= 0 && f.argument_count >= 0 && (f.pc >= mutate_pc::Length && f.pc <= mutate_pc::Await || m.which == source::Builtin.ArrayFrom && (f.pc == mutate_pc::Caught || f.pc == mutate_pc::Closed) || source::copier_species(m.which) && f.pc >= callbacks_pc::Constructor && f.pc <= callbacks_pc::Created)),
        // The Array callback methods (light-cljs-789, -908) walk their
        // object within its length, each in the phases its method has (the
        // species phases are map's, filter's and flatMap's, the inner walk
        // flatMap's; the step function checks the scratch operands); map,
        // filter and flatMap hold their result once ArraySpeciesCreate
        // made it.
        ArrayCallbacks a => object_of(s, f.input, node_kind::None) && f.pc >= callbacks_pc::Length && f.pc <= callbacks_pc::InnerStored && (f.pc < callbacks_pc::InnerLength || a.which == source::Builtin.ArrayPrototypeFlatMap) && (f.pc < callbacks_pc::Constructor || source::callbacks_species(a.which)) && f.argument_count >= 0 && f.index >= -1 && (!source::callbacks_species(a.which) || f.pc == callbacks_pc::Length || f.pc >= callbacks_pc::Constructor && f.pc <= callbacks_pc::Created || object_of(s, f.output, node_kind::None)),
        // A GetIterator frame awaits one of its three phases; from `next`'s
        // getter on, it holds the iterator (light-cljs-843).
        GetIterator => f.pc >= 1 && f.pc <= 3 && (f.pc != 3 || object_of(s, f.input, node_kind::None)),
        // An array iterator's `next` awaits its length or an element and
        // holds an array cursor, its source object and index;
        // String.prototype[@@iterator] awaits its string (light-cljs-844).
        BuiltinIterator m => if m.which == source::Builtin.ArrayIteratorPrototypeNext { f.pc >= 0 && f.pc <= 1 && f.index >= 0 && object_of(s, f.input, node_kind::Iterator) && s.nodes[f.input.id()].function <= iteration::Entries && object_of(s, f.receiver, node_kind::None) } else { m.which == source::Builtin.StringPrototypeIterator && f.pc == 0 },
        // A ForIn frame awaits one of its three phases over its key list; a
        // Proxy frame runs one of its methods on a proxy (light-cljs-845 and
        // -890, proxy.goose): an enumeration method from pc 3 with the key
        // list or descriptor it builds and the trap result it reads, a
        // property method with its key (and its throw flag).
        ForIn => f.pc >= 1 && f.pc <= 3 && object_of(s, f.input, node_kind::KeyList),
        Proxy p => object_of(s, f.input, node_kind::Proxy) && (if p.method <= proxy_method::GetOwnProperty { enumeration_frame_valid(s, f, p.method) } else { property_frame_valid(s, f, p.method) }),
        // A Forward frame (light-cljs-851, function_calls.goose) holds the
        // function it runs, and while it reads its argument list (pc 0 to
        // 2) the list.
        Forward => f.pc >= 0 && f.pc <= 3 && object_of(s, f.callee, node_kind::None) && (f.pc == 3 || object_of(s, f.input, node_kind::None)) && f.index >= 0 && f.output.is_number(),
        // A Bind frame (light-cljs-852) holds its target and the bound
        // function it is making.
        Bind => f.pc >= 0 && f.pc <= 2 && object_of(s, f.callee, node_kind::None) && object_of(s, f.receiver, node_kind::BoundFunction) && f.output.is_number(),
        // A FunctionText frame (light-cljs-853) awaits its function's name.
        FunctionText => f.pc == 0 && object_of(s, f.callee, node_kind::None) && callable_kind(s.nodes[f.callee.id()].kind),
        // A DynamicFunction frame (light-cljs-857) converts its arguments
        // from `index` on; its newTarget, if any, is an object; at pc 1 it
        // awaits that newTarget's `prototype` getter for the function it
        // made (light-cljs-887).
        DynamicFunction => (f.pc == 0 || f.pc == 1 && object_of(s, f.output, node_kind::Closure) && f.receiver.is_object()) && f.argument_count >= 0 && f.index >= 0 && f.index <= f.argument_count && (f.receiver.is_undefined() || object_of(s, f.receiver, node_kind::None)),
        // An ErrorConstruct frame (light-cljs-870) holds the error object it
        // is making; at pc 2 it awaits its options object's `cause` getter,
        // at pc 3 a proxy's answer whether it has one (light-cljs-890).
        ErrorConstruct e => source::error_type(e.which) >= 0 && (f.pc == 0 || f.pc == 2 || f.pc == 3) && object_of(s, f.output, node_kind::Error) && (f.pc == 0 || object_of(s, f.receiver, node_kind::None)),
        // An ErrorText frame (light-cljs-870) holds its this object, and
        // from pc 1 on the converted name.
        ErrorText => f.pc >= 0 && f.pc <= 1 && object_of(s, f.receiver, node_kind::None) && (f.pc == 0 || f.input.is_string()),
        // A Descriptor frame (light-cljs-881, descriptors.goose) defines on
        // its target object; defineProperties's key list holds the
        // properties object and the descriptors read so far, its cursor
        // within it, and at pc 1 it awaits a property's getter; the other
        // two await their key's conversion (pc 0) or hold it; at pc 2
        // each awaits a field of the descriptor object it reads into a
        // descriptor key list, and at pc 3 its presence (light-cljs-890).
        // At pc 4 and 5 it defines (definition_frame_valid).
        // At pc 6 Object.getOwnPropertyDescriptor or
        // Reflect.getOwnPropertyDescriptor awaits a proxy's
        // [[GetOwnProperty]] of its key (light-cljs-895).
        Descriptor d => if f.pc == 6 { source::own_descriptor_reads(d.which) && object_of(s, f.input, node_kind::Proxy) && (f.output.is_string() || f.output.is_symbol()) } else if f.pc == 4 || f.pc == 5 { source::descriptor_native(d.which) && definition_frame_valid(s, f, d.which) } else { source::descriptor_native(d.which) && f.pc >= 0 && f.pc <= 3 && object_of(s, f.input, node_kind::None)
            && (if d.which == source::Builtin.ObjectDefineProperties { f.pc != 0 && object_of(s, f.output, node_kind::KeyList) && f.argument_count >= 0 && f.argument_count < s.nodes[f.output.id()].key_count } else { if f.pc == 0 { f.output.is_undefined() } else { f.pc >= 2 && (f.output.is_string() || f.output.is_symbol()) } })
            && (f.pc < 2 || f.index >= 0 && f.index < source::FIELD_NAMES.len && object_of(s, f.callee, node_kind::KeyList) && object_of(s, f.receiver, node_kind::None)) },
        // A Statics frame (light-cljs-884, object_statics.goose) awaits one
        // of its values for its built-in.
        Statics x => statics_frame_valid(s, f, x.which),
        // A Construct frame (light-cljs-886) awaits newTarget's
        // `prototype` getter for its constructor, a closure or a
        // constructor built-in (light-cljs-887).
        Construct => f.pc == 0 && (object_of(s, f.callee, node_kind::Closure) || object_of(s, f.callee, node_kind::Builtin) && s.nodes[f.callee.id()].function < source::BUILTINS.len && source::constructor_builtin(source::BUILTINS[s.nodes[f.callee.id()].function].which)) && object_of(s, f.input, node_kind::None),
        // A Coerce frame (light-cljs-900, coerce.goose) holds a built-in
        // with a plan that converts, its receiver and arguments as its
        // operands (snapshot frame checks bound them), and the next
        // position; a construction's newTarget is an object.
        Coerce c => f.pc == 0 && object_of(s, f.callee, node_kind::Builtin) && s.nodes[f.callee.id()].function < source::BUILTINS.len && source::BUILTINS[s.nodes[f.callee.id()].function].which == c.which && source::BUILTINS[s.nodes[f.callee.id()].function].plan != plan::None && f.argument_count >= 0 && f.index >= 0 && f.index <= f.argument_count + 1 && (!c.construct || object_of(s, f.input, node_kind::None)),
        StringMethod m => string_frame_valid(s, f, m.which),
        SumPrecise => sum_frame_valid(s, f),
        // A RegExp frame (light-cljs-932, regexp_object.goose): the
        // constructor awaits one of its phases with its state bits, its
        // callee newTarget (an object or, called, undefined) or from the
        // prototype phase on the prototype; the flags getter awaits read pc
        // with the bits of the reads before it; toString awaits `source` or
        // `flags`, holding the source's string for the second; exec and
        // test (light-cljs-933) await an exec_pc:: phase; replace and
        // replaceAll a replace_pc::, @@replace a symbol_pc:: (light-cljs-934).
        RegExp r => regexp_frame_valid(s, f, r.which),
        // A Collection frame (light-cljs-943, collections.goose): its
        // collection of the constructor's flavor; from the adder on, the
        // adder; the iterable until the record arrives, then the record (in
        // the fast path, the array); while a result's `done` and `value` or
        // an entry's "0" are read, that object.
        Collection x => collection_frame_valid(s, f, x.which),
        CollectionForEach x => source::collection_for_each(x.which) && (f.pc == for_each_pc::Next || f.pc == for_each_pc::Call)
            && object_of(s, f.input, node_kind::Collection) && s.nodes[f.input.id()].length == (if x.which == source::Builtin.MapPrototypeForEach { collection::Map } else { collection::Set })
            && object_of(s, f.callee, node_kind::None) && callable_kind(s.nodes[f.callee.id()].kind)
            && f.index >= 0 && f.index <= s.nodes[f.input.id()].namespace_value.num(),
        _ => true,
    };
}
// The code units of string value v in the snapshot's tables, -1 for none.
fn string_units<T>(s: const T&, v: source::Value) -> i64 { return if v.is_string() && v.sid() < s.strings.len { s.strings[v.sid()].len } else { -1 }; }
fn iterator_valid<T>(s: const T&, n: const source::Node&) -> bool {
    if n.count != 0 || n.start != 0 || n.function > iteration::SetEntries { return false; }
    if source::map_iteration(n.function) || source::set_iteration(n.function) {
        if !source::collection_position(n.namespace_value) || n.length != 0 { return false; }
        if n.value.is_undefined() { return true; }
        return object_of(s, n.value, node_kind::Collection) && s.nodes[n.value.id()].length == (if source::map_iteration(n.function) { collection::Map } else { collection::Set })
            && n.namespace_value.num() <= s.nodes[n.value.id()].namespace_value.num();
    }
    return n.value.is_undefined() || (if n.function == iteration::CodePoints { n.value.is_string() } else { object_of(s, n.value, node_kind::None) });
}
fn collection_frame_valid<T>(s: const T&, f: source::SavedFrame, which: source::Builtin) -> bool {
    let flavor = if which == source::Builtin.Map { collection::Map } else if which == source::Builtin.Set { collection::Set } else { -1 };
    if flavor < 0 || !object_of(s, f.receiver, node_kind::Collection) || s.nodes[f.receiver.id()].length != flavor || f.pc < collection_pc::Adder || f.pc > collection_pc::Fast { return false; }
    if f.pc == collection_pc::Adder { return f.output.is_undefined() && f.callee.tag() > value_tag::Null; }
    if !object_of(s, f.output, node_kind::None) { return false; }
    if f.pc == collection_pc::Fast { return object_of(s, f.callee, node_kind::Array) && f.index >= 0; }
    if f.pc == collection_pc::Record { return f.callee.tag() > value_tag::Null; }
    if !object_of(s, f.callee, node_kind::Object) { return false; }
    return f.pc != collection_pc::Done && f.pc != collection_pc::Item && f.pc != collection_pc::Key || object_of(s, f.input, node_kind::None);
}
// A Map or Set (light-cljs-943, source::node_kind::Collection;
// collections.goose): a flavor, its size and next insertion position Numbers,
// and its run: none (count 0, nothing used, size 0) or `capacity` entries,
// a power of two at least collection_minimum(), of entry_stride values
// each, then 2 * capacity index slots. An entry below `function` is live
// (a key that is neither -0 nor an accessor, and a Map's value no
// accessor) or a tombstone (Empty key/value, position kept), as many live as its size; past
// `function` every entry is Empty. An index slot is Empty or a Number, the
// position of an entry below `function`, each at most once, and every live
// entry's position is there, reached by its key's probe with no equal key
// before it. The holes its run holds (Empty values, which
// validate counts with array holes), -1 for an invalid row, or -2 when
// the validation's budget ran out (poll).
fn collection_holes<T>(s: const T&, n: const source::Node&, c: Control&) -> i64 {
    if n.length > collection::Set || !n.value.is_number() || !source::collection_position(n.namespace_value) { return -1; }
    let size_value = n.value.num();
    if !(size_value >= 0 && size_value <= 4294967295.0) || trunc(size_value) != size_value { return -1; }
    let size: i64 = size_value as i64; let used: i64 = n.function; let count: i64 = n.count;
    if count == 0 { return if used == 0 && size == 0 { 0 } else { -1 }; }
    let flavor: i64 = n.length; let stride = source::entry_stride(flavor);
    if count % (stride + 2) != 0 { return -1; }
    let capacity = count / (stride + 2);
    if capacity < source::collection_minimum() || (capacity & (capacity - 1)) != 0 || used > capacity || size > used { return -1; }
    let start: i64 = n.start; let run = s.values[start..(start + count)];
    var holes: i64 = 0; var live: i64 = 0; var previous: f64 = -1;
    for i in capacity {
        let k = run[i * stride]; let v = if flavor == collection::Map { run[i * stride + 1] } else { source::Value.Empty };
        let position = run[i * stride + stride - 1];
        if i >= used { for j in stride { if !run[i * stride + j].is_hole() { return -1; } } holes += stride; continue; }
        if !source::collection_position(position) || position.num() <= previous || position.num() >= n.namespace_value.num() { return -1; }
        previous = position.num();
        if k.is_hole() { if !v.is_hole() { return -1; } holes += stride - 1; continue; }
        if source::is_accessor(k) || k.is_number() && k.num() == 0 && 1.0 / k.num() < 0 || flavor == collection::Map && (v.is_hole() || source::is_accessor(v)) { return -1; }
        live += 1;
    }
    if live != size { return -1; }
    var seen:    bool[>..] = []; seen.resize(used, false);
    var indexed: i64       = 0;
    for j in 2 * capacity {
        let x = run[capacity * stride + j];
        if x.is_hole() { holes += 1; continue; }
        if !x.is_number() { return -1; }
        let p = x.num();
        if !(p >= 0 && p < used) || trunc(p) != p { return -1; }
        let at: i64 = p as i64;
        if seen[at] { return -1; }
        seen[at] = true;
        if !run[at * stride].is_hole() { indexed += 1; }
    }
    if indexed != live { return -1; }
    // Placement (linear probing, as the interned-key index is checked): a
    // live entry's probe from its key's hash reaches its slot before any
    // Empty slot and passes no live entry with an equal key, so every key
    // is found and none is there twice.
    let mask = 2 * capacity - 1;
    for j in 2 * capacity {
        let x = run[capacity * stride + j];
        if x.is_hole() { continue; }
        let k = run[(x.num() as i64) * stride];
        if k.is_hole() { continue; }
        var at: i64 = collection_key_hash(s, k) & mask;
        while at != j {
            if !poll(c) { return -2; }
            let y = run[capacity * stride + at];
            if y.is_hole() { return -1; }
            let other = run[(y.num() as i64) * stride];
            if !other.is_hole() && same_collection_key(s, other, k) { return -1; }
            at = (at + 1) & mask;
        }
    }
    return holes;
}
// The hash collections.goose entry_hash gives key k, over a snapshot's
// tables: a string's units' FNV-1a (key_hash), a keyword form's name's
// and namespace's, any other key by its value or row. A keyword row whose
// name is no string, which its own check refuses, hashes by its row.
fn collection_key_hash<T>(s: const T&, k: source::Value) -> u32 {
    return match k {
        String x => source::string_entry_hash(key_hash(s, s.strings[x.s])),
        Object x => {
            let n = s.nodes[x.o];
            if n.kind != node_kind::Keyword || n.initialized || !n.value.is_string() || !n.namespace_value.is_null() && !n.namespace_value.is_string() { source::plain_entry_hash(k) } else {
                source::keyword_entry_hash(key_hash(s, s.strings[n.value.sid()]), n.namespace_value.is_string(), if n.namespace_value.is_string() { key_hash(s, s.strings[n.namespace_value.sid()]) } else { 0 })
            }
        },
        _ => source::plain_entry_hash(k),
    };
}
// Unit i of string header r of a snapshot.
inline fn unit_at<T>(s: const T&, r: source::Str, i: i64) -> u32 { return if r.wide { s.units[r.start + i] } else { s.latin1[r.start + i] }; }
fn same_strings<T>(s: const T&, a: source::Value, b: source::Value) -> bool {
    if a.sid() == b.sid() { return true; }
    let x = s.strings[a.sid()]; let y = s.strings[b.sid()];
    if x.len != y.len { return false; }
    for i in x.len { if unit_at(s, x, i) != unit_at(s, y, i) { return false; } }
    return true;
}
// A keyword form row of a snapshot that compares by name (same_keyword).
fn keyword_row<T>(s: const T&, o: i64) -> bool { let n = s.nodes[o]; return n.kind == node_kind::Keyword && !n.initialized && n.value.is_string() && (n.namespace_value.is_null() || n.namespace_value.is_string()); }
// SameValueZero of two keys of a snapshot's collection (collections.goose
// same_entry_key).
fn same_collection_key<T>(s: const T&, a: source::Value, b: source::Value) -> bool {
    return match a {
        Number x => b.is_number() && (x.v == b.num() || x.v != x.v && b.num() != b.num()),
        String => b.is_string() && same_strings(s, a, b),
        Object x => b.is_object() && (x.o == b.id() || keyword_row(s, x.o) && keyword_row(s, b.id()) && same_strings(s, s.nodes[x.o].value, s.nodes[b.id()].value) && s.nodes[x.o].namespace_value.is_string() == s.nodes[b.id()].namespace_value.is_string() && (!s.nodes[x.o].namespace_value.is_string() || same_strings(s, s.nodes[x.o].namespace_value, s.nodes[b.id()].namespace_value))),
        Symbol x => b.is_symbol() && x.s == source::symbol_index(b),
        Bool x => b.is_bool() && x.v == b.flag(),
        Undefined => b.is_undefined(),
        Null => b.is_null(),
        _ => false,
    };
}
fn regexp_frame_valid<T>(s: const T&, f: source::SavedFrame, which: source::Builtin) -> bool {
    if which == source::Builtin.RegExp {
        let proto = f.index & regexp_state::Prototype != 0;
        return f.pc >= regexp_pc::Match && f.pc <= regexp_pc::FlagsText && f.index >= 0 && f.index <= 15 && proto == (f.pc >= regexp_pc::PatternText)
            && (if proto { object_of(s, f.callee, node_kind::None) } else { f.callee.is_undefined() || object_of(s, f.callee, node_kind::None) });
    }
    // exec and test (light-cljs-933): exec's receiver is a RegExp, test's
    // any object until it reads lastIndex, which only a RegExp's frame does
    // (its next step matches with the receiver's program); past the
    // argument's conversion each holds its string, and only test reads
    // `exec` or awaits a callable exec's result.
    if source::regexp_matching(which) {
        let test = which == source::Builtin.RegExpPrototypeTest;
        return object_of(s, f.receiver, if test && f.pc != exec_pc::LastIndex { node_kind::None } else { node_kind::RegExp })
            && (f.pc == exec_pc::Input || f.input.is_string() && (f.pc == exec_pc::LastIndex || test && (f.pc == exec_pc::Exec || f.pc == exec_pc::Result)));
    }
    // replace and replaceAll (light-cljs-934, regexp_replace.goose) spend
    // a numbered allowance; from Search on the receiver is its string, from
    // Replacement on the search value is, and from Scan on the replacement
    // is a template string or a function, with the scan's cursors within
    // the receiver: the end of the last occurrence replaced, and where the
    // scan resumes (one past the end once every occurrence is found) or
    // the occurrence a replacer was called for. Its steps check the pieces
    // they read.
    if which == source::Builtin.StringPrototypeReplace || which == source::Builtin.StringPrototypeReplaceAll {
        let len = string_units(s, f.receiver);
        return f.pc >= replace_pc::Match && f.pc <= replace_pc::Callback && f.output.is_number() && f.index >= 0 && f.argument_count >= 0
            && (f.pc < replace_pc::Search || len >= 0) && (f.pc < replace_pc::Replacement || f.input.is_string())
            && (f.pc < replace_pc::Scan || (f.callee.is_string() || object_of(s, f.callee, node_kind::None)) && f.argument_count <= len && f.index <= (if f.pc == replace_pc::Scan { len + 1 } else { len }));
    }
    // @@replace (light-cljs-934) on an object rx: from Replacement on its
    // argument's string, from Flags on its template or replacer, while
    // collecting symbol_flag:: bits and while rendering a result index
    // within the results (its registers and scratch, being operands, its
    // steps check).
    if which == source::Builtin.RegExpPrototypeReplace {
        return f.pc >= symbol_pc::Input && f.pc <= symbol_pc::Named && object_of(s, f.receiver, node_kind::None) && f.output.is_number() && f.argument_count >= 0
            && (f.pc <= symbol_pc::Input || f.input.is_string()) && (f.pc <= symbol_pc::Replacement || f.callee.is_string() || object_of(s, f.callee, node_kind::None))
            && (if f.pc < symbol_pc::Render { f.index >= 0 && f.index <= (symbol_flag::Global | symbol_flag::Unicode) } else { f.index >= 0 && f.index <= f.argument_count && (f.pc == symbol_pc::Render || f.index < f.argument_count) });
    }
    if !object_of(s, f.receiver, node_kind::None) { return false; }
    if which == source::Builtin.RegExpPrototypeFlags { return f.pc >= 0 && f.pc < 8 && f.argument_count >= 0 && f.argument_count < (1 << f.pc); }
    return which == source::Builtin.RegExpPrototypeToString && (f.pc == 0 || f.pc == 1 && f.input.is_string());
}
// A SumPrecise frame (light-cljs-901, math_object.goose) awaits one of its
// values with its exact sum, an array of sum_layout::Size limbs and flags,
// and the elements it added; from pc Record on it holds the iterator
// record, while it reads a result's `done` and `value` that result.
fn sum_frame_valid<T>(s: const T&, f: source::SavedFrame) -> bool {
    if f.pc < sum_pc::Iterable || f.pc > sum_pc::Closed || f.index < 0 || f.index > sum_layout::MaxElements || !object_of(s, f.output, node_kind::Array) { return false; }
    let sum = s.nodes[f.output.id()];
    if sum.initialized || sum.count != sum_layout::Size || sum.length != sum_layout::Size || !range(sum.start, sum.count, s.values.len) || !source::sum_words_valid(s.values[sum.start..(sum.start + sum.count)]) { return false; }
    if f.pc <= sum_pc::Record { return f.callee.is_undefined(); }
    if !object_of(s, f.callee, node_kind::Object) { return false; }
    return f.pc != sum_pc::Done && f.pc != sum_pc::Item || object_of(s, f.input, node_kind::None);
}
// A CopyData frame (light-cljs-804, source_vm.goose copy_step) walks its
// key list at pc Next and Value; of a proxy source (light-cljs-895) it
// awaits the proxy's key list (pc Keys, none held yet) or, within its key
// list, key index - 1's descriptor (pc Own), for its target object; object
// rest's frames hold no list and no target (pc RestKeys) or the key whose
// descriptor they await (pc RestEnumerable).
fn copy_frame_valid<T>(s: const T&, f: source::SavedFrame) -> bool {
    if f.pc == copy_pc::Next || f.pc == copy_pc::Value { return object_of(s, f.callee, node_kind::KeyList); }
    if !object_of(s, f.input, node_kind::Proxy) { return false; }
    if f.pc == copy_pc::Keys { return f.callee.is_undefined() && object_of(s, f.output, node_kind::Object); }
    if f.pc == copy_pc::Own { return object_of(s, f.callee, node_kind::KeyList) && f.index >= 1 && f.index <= s.nodes[f.callee.id()].key_count && object_of(s, f.output, node_kind::Object); }
    if f.pc == copy_pc::RestKeys { return f.callee.is_undefined() && f.output.is_undefined(); }
    return f.pc == copy_pc::RestEnumerable && f.callee.is_undefined() && (f.output.is_string() || f.output.is_symbol());
}
// A Descriptor frame that defines: at pc 4 on a proxy, whose
// [[DefineOwnProperty]] answers (light-cljs-891), at pc 5 on an array,
// whose new `length` given as an object converts (light-cljs-898). It
// holds the key and descriptor, or defineProperties's list read whole and
// the cursor of its definitions, there (pc 5) a descriptor key list's.
fn definition_frame_valid<T>(s: const T&, f: source::SavedFrame, which: source::Builtin) -> bool {
    if !object_of(s, f.input, if f.pc == 4 { node_kind::Proxy } else { node_kind::Array }) { return false; }
    if which != source::Builtin.ObjectDefineProperties { return (f.output.is_string() || f.pc == 4 && f.output.is_symbol()) && object_of(s, f.callee, node_kind::KeyList); }
    if !object_of(s, f.output, node_kind::KeyList) || f.argument_count != s.nodes[f.output.id()].key_count || f.index < 0 || f.index >= f.argument_count { return false; }
    let row: i64 = s.nodes[f.output.id()].keys + f.index;
    return f.pc == 4 || row < s.properties.len && object_of(s, s.properties[row].value, node_kind::KeyList);
}
// A Proxy frame of an enumeration method (light-cljs-845): pc 3 and 4 list
// keys, pc 5 and 8 read a descriptor's field `index` (its value from a
// getter, its presence from a proxy's [[HasProperty]], light-cljs-890).
fn enumeration_frame_valid<T>(s: const T&, f: source::SavedFrame, method: i64) -> bool {
    // [[OwnPropertyKeys]] checking its trap's keys against a proxy target
    // (light-cljs-897) holds those keys, and awaits the target's
    // [[IsExtensible]] (pc 12), its [[OwnPropertyKeys]] (pc 14, the
    // extensibility read) or the [[GetOwnProperty]] of key `index` of the
    // target's key list (pc 15).
    if method == proxy_method::OwnPropertyKeys && f.pc >= 12 {
        if !object_of(s, f.callee, node_kind::KeyList) { return false; }
        if f.pc == 12 { return f.receiver.is_undefined(); }
        if f.argument_count < 0 || f.argument_count > 1 { return false; }
        if f.pc == 14 { return f.receiver.is_undefined(); }
        return f.pc == 15 && object_of(s, f.receiver, node_kind::KeyList) && f.index >= 0 && f.index < s.nodes[f.receiver.id()].key_count;
    }
    // [[GetOwnProperty]] awaiting a proxy target's [[GetOwnProperty]] (pc
    // 6) holds its trap's result, undefined or an object; awaiting its
    // [[IsExtensible]] (pc 12), that result and, an object, the descriptor
    // it builds (light-cljs-895).
    if method == proxy_method::GetOwnProperty && (f.pc == 6 || f.pc == 12) {
        if f.receiver.is_undefined() { return f.pc == 6 || f.callee.is_undefined(); }
        return object_of(s, f.receiver, node_kind::None) && (f.pc == 6 || object_of(s, f.callee, node_kind::KeyList));
    }
    // [[GetPrototypeOf]] awaiting a proxy target's [[IsExtensible]] or
    // [[GetPrototypeOf]] holds its trap's result (light-cljs-891).
    if f.pc == 12 || f.pc == 13 { return method == proxy_method::GetPrototypeOf && (f.receiver.is_null() || object_of(s, f.receiver, node_kind::None)); }
    if f.pc > 8 || f.pc == 6 || f.pc == 7 { return false; }
    if f.pc < 3 { return true; }
    let fields = f.pc == 5 || f.pc == 8;
    return (if fields { method == proxy_method::GetOwnProperty && f.index >= 0 && f.index < source::FIELD_NAMES.len } else { method == proxy_method::OwnPropertyKeys && f.index >= 0 }) && object_of(s, f.callee, node_kind::KeyList) && object_of(s, f.receiver, node_kind::None);
}
// A Proxy frame of [[Get]], [[Set]], [[HasProperty]] or [[Delete]]
// (light-cljs-890): its key, a throw flag only for [[Set]] and [[Delete]],
// and a phase of its method's (pc 6 a target descriptor, pc 7 a forwarded
// getter's or setter's result, pc 9 a forwarded [[Set]]'s or
// [[DefineOwnProperty]]'s array length conversions).
//
// The methods of light-cljs-891: [[Call]] and [[Construct]] hold their
// argument array until the trap is called; [[IsExtensible]],
// [[PreventExtensions]] and [[SetPrototypeOf]] (its new prototype) an
// answer mode (an object to give back for answer::Object) and no key;
// [[DefineOwnProperty]] its key and descriptor, awaiting the trap or a
// target's descriptor (pc 6); a [[Set]] at its proxy receiver's phases
// (pc 10 and 11) holds that receiver.
fn property_frame_valid<T>(s: const T&, f: source::SavedFrame, method: i64) -> bool {
    if method > proxy_method::SetPrototypeOf || f.index < 0 || f.argument_count < 0 { return false; }
    if method == proxy_method::Call || method == proxy_method::Construct { return f.pc <= 2 && f.index == answer::Boolean && object_of(s, f.output, node_kind::Array); }
    if method >= proxy_method::IsExtensible {
        let modes = if method == proxy_method::IsExtensible { answer::Boolean } else if method == proxy_method::PreventExtensions { answer::Object } else { answer::Nothing };
        let set = method == proxy_method::SetPrototypeOf;
        // pc 12 awaits a proxy target's [[IsExtensible]] (the trap's
        // boolean in callee, but for [[SetPrototypeOf]]'s new prototype),
        // pc 13 its [[GetPrototypeOf]].
        let phase = f.pc <= 2 || f.pc == 12 && (set || f.callee.is_bool()) || f.pc == 13 && set;
        return phase && f.index <= modes && f.index != answer::Throws && (f.index != answer::Object || object_of(s, f.receiver, node_kind::None)) && (!set || f.callee.is_object() || f.callee.is_null()) && f.output.is_undefined();
    }
    if !(f.output.is_string() || f.output.is_symbol()) { return false; }
    // A trapless [[DefineOwnProperty]] awaiting both conversions of an
    // array target's new `length` (pc 9, light-cljs-898) has that target.
    if method == proxy_method::DefineOwnProperty { return (f.pc <= 2 || f.pc == 6 || f.pc == 12 && (f.receiver.is_undefined() || object_of(s, f.receiver, node_kind::KeyList)) || f.pc == proxy_pc::Length && object_of(s, s.nodes[f.input.id()].value, node_kind::Array)) && f.index == answer::Boolean && object_of(s, f.callee, node_kind::KeyList); }
    let throws = method == proxy_method::Set || method == proxy_method::Delete;
    if f.index > (if throws { answer::Throws } else { answer::Boolean }) { return false; }
    return f.pc <= 2 || f.pc == 6 || f.pc == 7 && (method == proxy_method::Get || method == proxy_method::Set) || f.pc == 9 && method == proxy_method::Set || (f.pc == 10 || f.pc == 11) && method == proxy_method::Set && object_of(s, f.receiver, node_kind::Proxy)
        || f.pc == 12 && (method == proxy_method::HasProperty || method == proxy_method::Delete);
}
// A Statics frame of built-in b (light-cljs-884): the value it awaits is
// one of its built-in's, and the rows its steps read are there: a key walk's
// object, result and key list (Undefined while a proxy's keys are listed)
// with its cursor within the list, Object.assign's sources array, an
// iteration's result and iterator record, a keyed built-in's proxy.
fn statics_frame_valid<T>(s: const T&, f: source::SavedFrame, b: source::Builtin) -> bool {
    let pc = f.pc;
    // Object.seal, freeze, isSealed and isFrozen of a proxy (light-cljs-897,
    // integrity.goose): the proxy; its [[PreventExtensions]] (seal, freeze)
    // or [[IsExtensible]] (the tests) and its keys awaited before the key
    // list; then, within the list, key index - 1's descriptor (all but
    // seal) or definition (seal, freeze).
    if source::integrity_walks(b) {
        if !object_of(s, f.input, node_kind::Proxy) || !f.output.is_undefined() { return false; }
        let tests = source::integrity_tests(b);
        if pc == statics_pc::Prevented || pc == statics_pc::Extensible || pc == statics_pc::List { return (pc != statics_pc::Prevented || !tests) && (pc != statics_pc::Extensible || tests) && f.callee.is_undefined() && f.index == 0; }
        return (pc == statics_pc::Own && b != source::Builtin.ObjectSeal || pc == statics_pc::Defined && !tests) && object_of(s, f.callee, node_kind::KeyList) && f.index >= 1 && f.index <= s.nodes[f.callee.id()].key_count;
    }
    if source::statics_walks(b) || source::statics_names(b) {
        if !object_of(s, f.input, node_kind::None) || !object_of(s, f.output, node_kind::None) { return false; }
        if b == source::Builtin.ObjectAssign && (!object_of(s, f.receiver, node_kind::Array) || f.argument_count < 1) { return false; }
        if pc == statics_pc::List { return f.callee.is_undefined() && object_of(s, f.input, node_kind::Proxy) && f.index == 0; }
        if source::statics_names(b) { return false; }
        return (pc == statics_pc::Own || pc == statics_pc::Value || pc == statics_pc::Set) && object_of(s, f.callee, node_kind::KeyList) && f.index >= 1 && f.index <= s.nodes[f.callee.id()].key_count
            && (pc != statics_pc::Own || object_of(s, f.input, node_kind::Proxy));
    }
    if source::statics_iterates(b) {
        if !object_of(s, f.output, node_kind::Object) || (pc < statics_pc::Record || pc > statics_pc::Closed) && pc != statics_pc::Iterable { return false; }
        if b == source::Builtin.ObjectGroupBy && pc != statics_pc::First && pc != statics_pc::Second && !object_of(s, f.receiver, node_kind::None) { return false; }
        return pc == statics_pc::Record || pc == statics_pc::Iterable || object_of(s, f.callee, node_kind::Object);
    }
    // Reflect.has requires an object target, and may await a proxy's
    // [[HasProperty]] (pc Result, light-cljs-890); the others convert theirs
    // later.
    if source::statics_keyed(b) { return (pc == statics_pc::Key && (b != source::Builtin.ReflectHas || object_of(s, f.input, node_kind::None)) || pc == statics_pc::Own && object_of(s, f.input, node_kind::Proxy) || pc == statics_pc::Result && b == source::Builtin.ReflectHas && object_of(s, f.input, node_kind::None)) && f.output.is_undefined(); }
    // Reflect.get and Reflect.set (light-cljs-888): an object target; a
    // key converting, a getter's or (Reflect.set) a setter's result or an
    // array receiver's converting length awaited.
    if source::statics_receiver(b) { return object_of(s, f.input, node_kind::None) && (pc == statics_pc::Key || pc == statics_pc::Result || b == source::Builtin.ReflectSet && (pc == statics_pc::Stored || pc == statics_pc::Length && object_of(s, f.receiver, node_kind::Array))) && (b == source::Builtin.ReflectSet || f.output.is_undefined()); }
    if b == source::Builtin.ObjectPrototypeToString { return pc == statics_pc::Tag; }
    if b == source::Builtin.ObjectPrototypeToLocaleString { return pc == statics_pc::Method || pc == statics_pc::Result; }
    return false;
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
            // Every phase of padding and split pops or indexes exactly one
            // operand when it runs, so a top frame must own exactly one (a
            // lower one gets its operand from the frame above it). Split's
            // scanner owns its numeric cursor and cannot have a child frame.
            match f.native {
                StringMethod m => {
                    if (m.which == source::Builtin.StringPrototypeSplit || source::string_pad(m.which)) && i + 1 == count && operands - f.operand_base != 1 { return reject(c, Error.Link); }
                    if m.which == source::Builtin.StringPrototypeSplit && f.pc == string_pc::SplitScan {
                        if i + 1 != count { return reject(c, Error.Link); }
                        let cursor = s.values[a.values + locals + f.operand_base];
                        if !cursor.is_number() || !(cursor.num() >= 0 && cursor.num() <= f.index) || trunc(cursor.num()) != cursor.num() { return reject(c, Error.Link); }
                    }
                },
                _ => {},
            }
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
            Saved x => { if !value(s, x.h.value) || x.h.pc < -1 || x.h.kind == source::HandlerKind.Async && (!x.h.value.is_object() || s.nodes[x.h.value.id()].kind != node_kind::Promise) { return reject(c, Error.Link); } },
            Promise x => {
                let p = x.p;
                if p.state > promise_status::Rejected || !value(s, p.result) || !link_or_none(s, p.first, 3) || !link_or_none(s, p.last, 3) || (p.first == source::no_row()) != (p.last == source::no_row()) || p.state != promise_status::Pending && p.first != source::no_row() || !link_or_none(s, p.activation, 0) { return reject(c, Error.Link); }
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
                if r.target >= s.nodes.len || s.nodes[r.target].kind != node_kind::Promise || !link_or_none(s, r.next, 3) { return reject(c, Error.Link); }
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
fn ends<T>(s: const T&) -> i64[26] {
    return [s.code_bytes.len, s.functions.len, s.code_constants.len, s.children.len, s.ops.len, s.code_units.len,
            s.nodes.len, s.values.len, s.properties.len, s.units.len, s.constants.len, s.bytes.len, s.sources.len, s.bindings.len, s.code_strings.len, s.strings.len, s.latin1.len, s.shapes.len, s.accessors.len, s.keys.len, s.conts.len,
            s.sites.len, s.site_sources.len, s.site_records.len, s.patterns.len, s.pattern_ops.len];
}
fn payload_bytes<T>(s: const T&) -> i64 {
    return bytes_of(s.code_bytes).len + bytes_of(s.functions).len + bytes_of(s.code_constants).len + bytes_of(s.children).len + bytes_of(s.ops).len + bytes_of(s.code_units).len
         + bytes_of(s.nodes).len + bytes_of(s.values).len + bytes_of(s.properties).len + bytes_of(s.units).len + bytes_of(s.constants).len + bytes_of(s.bytes).len + bytes_of(s.sources).len + bytes_of(s.bindings).len + bytes_of(s.latin1).len + bytes_of(s.code_strings).len + bytes_of(s.strings).len
         + bytes_of(s.shapes).len + bytes_of(s.accessors).len + bytes_of(s.keys).len + bytes_of(s.conts).len
         + bytes_of(s.sites).len + bytes_of(s.site_sources).len + bytes_of(s.site_records).len + bytes_of(s.patterns).len + bytes_of(s.pattern_ops).len;
}
fn footprint<T>(s: const T&) -> i64 { let empty: Snapshot[] = [Snapshot {}]; return bytes_of(empty).len + payload_bytes(s); }
// Whether v is a symbol row Symbol.for registered (light-cljs-885).
fn registered<T>(s: const T&, v: source::Value) -> bool { let r = source::symbol_node(v); return r >= 0 && r < s.nodes.len && s.nodes[r].kind == node_kind::JsSymbol && s.nodes[r].initialized; }
fn value<T>(s: const T&, v: source::Value, code: bool = false, constant: bool = false) -> bool {
    // Symbol and BigInt constants carry no runtime value (source::load).
    return match v {
        // A well-known symbol is also a row value (the realm's Symbol
        // properties, light-cljs-788), and so is a symbol row
        // (light-cljs-885), which code never names.
        Symbol x => if code || constant { export_well_known_symbol(x.s).len > 0 } else { source::symbol_payload(s.nodes, x.s) },
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
    if !program::source_text(s.code_bytes) { return reject(c, Error.Code); }
    if s.ops.len > c.bytes - footprint(s) { return reject(c, Error.Bytes); }
    c.validation_peak = max(c.validation_peak, s.ops.len);
    var used: bool[>..] = []; used.resize(s.ops.len, false);
    var next: i64       = 0;
    for f in s.functions {
        if !poll(c) { return false; }
        if f.count <= 0 || !range(f.start, f.count, s.ops.len) || !range(f.constants, 0, s.code_constants.len) || !range(f.children, 0, s.children.len) || f.params < 0 || f.params > f.locals || f.locals > 65535 || f.captures < 0 || f.captures > 65535 || f.kind < 0 || f.kind > constructor::DefaultDerivedClass || !range(f.name_start, f.name_len, s.code_units.len) || f.length > 65535 { return reject(c, Error.Code); }
        // Source text (light-cljs-853): a non-empty range of the code bytes on
        // UTF-8 boundaries, or none.
        if f.has_text && (f.text_start >= f.text_end || !range(f.text_start, f.text_end - f.text_start, s.code_bytes.len) || !program::boundary(s.code_bytes, f.text_start) || !program::boundary(s.code_bytes, f.text_end)) || !f.has_text && (f.text_start != 0 || f.text_end != 0) { return reject(c, Error.Code); }
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
    if next != s.ops.len { return reject(c, Error.Code); }
    return sites_valid(s, c) && patterns_valid(s, c);
}
// Direct-eval sites (light-cljs-858), as program::validate checks an image's:
// one per marked call in ascending op order, source and record ranges tiling
// their tables, sources in capture order with one receiver, and local and
// capture indices within the calling function's.
fn sites_valid<T>(s: const T&, c: Control&) -> bool {
    var marked: i64 = 0; for op in s.ops { if op.direct_eval { marked += 1; } }
    if marked != s.sites.len || !poll(c, s.sites.len + s.site_sources.len) { return reject(c, Error.Code); }
    if s.sites.len == 0 { return if s.site_sources.len == 0 && s.site_records.len == 0 { true } else { reject(c, Error.Code) }; }
    var owner: u32[>..] = []; owner.resize(s.ops.len, 0);
    for f, id in s.functions { for pc in f.start..(f.start + f.count) { owner[pc] = id as u32; } }
    var sources: i64 = 0; var records: i64 = 0; var previous: i64 = -1;
    for site in s.sites {
        if site.op as i64 <= previous || site.op >= s.ops.len || !s.ops[site.op].direct_eval || site.sources != sources || site.record != records || site.length == 0 || site.count > 65535 || !range(sources, site.count, s.site_sources.len) || !range(records, site.length, s.site_records.len) { return reject(c, Error.Code); }
        let f = s.functions[owner[site.op]];
        var last: i64 = -1; var receivers: i64 = 0;
        for x in s.site_sources[sources..(sources + site.count)] {
            let kind = x.kind as i64; let role = x.role as i64;
            if kind > eval_source::NewTarget || role > eval_role::SuperFields || role < last || role == last && role != eval_role::Binding && role != eval_role::Environment || x.cell && role != eval_role::Binding && role != eval_role::SuperFields { return reject(c, Error.Code); }
            if kind == eval_source::Local && x.index >= f.locals || kind == eval_source::Capture && x.index >= f.captures || kind != eval_source::Local && kind != eval_source::Capture && x.index != 0 { return reject(c, Error.Code); }
            if role == eval_role::Receiver { receivers += 1; }
            last = role;
        }
        if receivers != 1 { return reject(c, Error.Code); }
        previous = site.op; sources += site.count; records += site.length;
    }
    return if sources == s.site_sources.len && records == s.site_records.len { true } else { reject(c, Error.Code) };
}
// Regular expressions (light-cljs-932), as program::validate checks an
// image's: rows in ascending `op`, a literal's naming a RegExpLiteral
// instruction and every such instruction having exactly one literal row, a
// run-time row's `op` at most the instruction count; each program's op rows
// tiling the op table in order and its unit run within the code units, and
// each program valid (program::pattern_valid).
fn patterns_valid<T>(s: const T&, c: Control&) -> bool {
    if !poll(c, s.patterns.len + s.pattern_ops.len) { return false; }
    var literals: i64 = 0; for op in s.ops { if op.code == opcode::RegExpLiteral { literals += 1; } }
    var ops:      i64 = 0; var previous: i64 = -1; var last_literal: i64 = -1; var found: i64 = 0;
    for p in s.patterns {
        if p.op < previous || p.op > s.ops.len || p.literal && (p.op == s.ops.len || s.ops[p.op].code != opcode::RegExpLiteral || p.op <= last_literal) { return reject(c, Error.Code); }
        if p.literal { last_literal = p.op; found += 1; }
        previous = p.op;
        let total: i64 = p.instructions + p.members + p.names;
        if p.start != ops || !range(ops, total, s.pattern_ops.len) || !range(p.units, p.unit_count, s.code_units.len) { return reject(c, Error.Code); }
        if !program::pattern_valid(p, s.pattern_ops[ops..(ops + total)], s.code_units[p.units..(p.units + p.unit_count)]) { return reject(c, Error.Code); }
        ops += total;
    }
    return if ops == s.pattern_ops.len && found == literals { true } else { reject(c, Error.Code) };
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
    if s.header.version != 4 { return reject(c, Error.Version); }
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
    if !value(s, s.header.global) || !s.header.global.is_object() || s.nodes[s.header.global.id()].kind != node_kind::Object { return reject(c, Error.Link); }
    if !value(s, s.header.call_method) || !s.header.call_method.is_object() || s.nodes[s.header.call_method.id()].kind != node_kind::Builtin || s.nodes[s.header.call_method.id()].function != 14 { return reject(c, Error.Link); }
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
    var has_memo = false; var array_holes: i64 = 0; var registrations: i64 = 0;
    var owners: i64 = 0; var private_rows: i64 = 0;
    for n, row in s.nodes {
        if !poll(c) { return false; }
        if n.kind < node_kind::Object || n.kind > node_kind::Collection { return reject(c, Error.Unsupported); }
        // A RegExp (light-cljs-932, source::node_kind::RegExp) names a compiled
        // program of the code's and holds its pattern text, a string, and
        // no element run.
        if n.kind == node_kind::RegExp && (n.function >= s.patterns.len || s.patterns[n.function].failure != regexp_failure::None || !n.value.is_string() || n.count != 0 || n.start != 0 || n.length != 0 || !n.namespace_value.is_null()) { return reject(c, Error.Link); }
        // A shareable regex value (light-cljs-935) keeps its layout.
        if n.kind == node_kind::RegExp && n.immutable && !shared_regexp_layout(s, n) { return reject(c, Error.Link); }
        // A struct type (light-cljs-920, source::node_kind::StructType) is
        // immutable and has a name, a run of field then member names
        // (strings, checked with its run below), a type_flavor and a key
        // field inside it, never a Mutable one's, and is declared or not
        // (initialized); a struct has its type, a
        // field per type field, and a record's extension map and metadata,
        // and is immutable unless its type is Mutable; an initializer slot
        // has a state, 0 to 3, its defined flag, its unit and name, and its
        // value as a run of one.
        if n.kind == node_kind::StructType && (!n.value.is_string() || n.count % 2 != 0 || n.length > type_flavor::Mutable || n.function > n.count / 2 || n.function != 0 && n.length == type_flavor::Mutable || !n.namespace_value.is_null() || !n.metadata.is_undefined() || !n.proto.is_hole() || n.keys != 0 || n.key_count != 0 || n.key_capacity != 0 || n.shape != 0 || !n.immutable || n.non_extensible) { return reject(c, Error.Link); }
        if n.kind == node_kind::Struct && !struct_row(s, n) { return reject(c, Error.Link); }
        if n.kind == node_kind::InitializerSlot && (n.length > 3 || n.count != 1 || !n.metadata.is_undefined() || n.keys != 0 || n.key_count != 0 || n.key_capacity != 0 || n.shape != 0 || n.function != 0 || !n.proto.is_hole() || n.immutable || n.non_extensible) { return reject(c, Error.Link); }
        // A reduced marker (light-cljs-918) holds its value and nothing else.
        if n.kind == node_kind::Reduced && (!n.namespace_value.is_null() || !n.metadata.is_undefined() || !n.proto.is_hole() || n.count != 0 || n.start != 0 || n.keys != 0 || n.key_count != 0 || n.key_capacity != 0 || n.shape != 0 || n.function != 0 || n.length != 0 || n.initialized || n.immutable || n.non_extensible) { return reject(c, Error.Link); }
        // An object's private elements (light-cljs-889) are records
        // (record_run, key_runs) and nothing else; their one owner is
        // checked with its metadata below.
        if n.kind == node_kind::PrivateElements && (!n.value.is_undefined() || !n.namespace_value.is_null() || !n.metadata.is_undefined() || !n.proto.is_hole() || n.count != 0 || n.start != 0 || n.shape != 0 || n.function != 0 || n.length != 0 || n.initialized || n.immutable || n.non_extensible) { return reject(c, Error.Link); }
        // A symbol row (light-cljs-885) has a description, a string or (not
        // registered) undefined, and nothing else but, registered, a link to
        // the symbol registered before it, an earlier row.
        if n.kind == node_kind::JsSymbol && (!n.value.is_string() && (n.initialized || !n.value.is_undefined()) || !n.namespace_value.is_null() && (!n.initialized || !registered(s, n.namespace_value) || source::symbol_node(n.namespace_value) >= row) || n.count != 0 || n.start != 0 || n.keys != 0 || n.key_count != 0 || n.key_capacity != 0 || n.shape != 0 || n.function != 0 || n.length != 0 || !n.proto.is_hole() || !n.metadata.is_undefined() || n.non_extensible) { return reject(c, Error.Link); }
        if n.kind == node_kind::JsSymbol && n.initialized { registrations += 1; }
        // A proxy (light-cljs-845, proxy.goose) holds an object target, an
        // earlier row, and an object handler, and nothing else but its
        // revoked flag (`initialized`, light-cljs-891).
        if n.kind == node_kind::Proxy && (!n.value.is_object() || n.value.id() >= row || !n.namespace_value.is_object() || n.count != 0 || n.start != 0 || !n.proto.is_null()) { return reject(c, Error.Link); }
        // A revoke function (Proxy.revocable, light-cljs-891) names its
        // proxy, an earlier row, until it revokes it (null).
        if n.kind == node_kind::Builtin && n.function == source::code(source::Builtin.ProxyRevoke) && !n.value.is_null() && !(n.value.is_object() && n.value.id() < row && s.nodes[n.value.id()].kind == node_kind::Proxy) { return reject(c, Error.Link); }
        // A bound function (light-cljs-852) binds a callable target, an
        // earlier row, and has no code or length; its arguments are a run
        // (has_values) and its bound `this` a value.
        if n.kind == node_kind::BoundFunction && (!n.value.is_object() || n.value.id() >= row || !callable_kind(s.nodes[n.value.id()].kind) || n.function != 0 || n.length != 0) { return reject(c, Error.Link); }
        // A key list's keys are listed, or pending for the proxy it walks.
        if n.kind == node_kind::KeyList && (n.function > key_list::Pending || n.function == key_list::Pending && !(n.value.is_object() && n.value.id() < s.nodes.len && s.nodes[n.value.id()].kind == node_kind::Proxy)) { return reject(c, Error.Link); }
        // A built-in iterator (light-cljs-844, iterators.goose) has an
        // iteration and no element run, and iterates an object, a string
        // (a string iteration) or, done, nothing.
        if n.kind == node_kind::Iterator && !iterator_valid(s, n) { return reject(c, Error.Link); }
        // A primitive wrapper (22, light-cljs-832) holds a number, string,
        // boolean or symbol (light-cljs-885) and keeps no element run.
        if n.kind == node_kind::Wrapper && (source::wrapper_type(n.value) < 0 && !n.value.is_symbol() || n.count != 0 || n.start != 0) { return reject(c, Error.Link); }
        // A generator (20) names its activation or nothing and has a state,
        // 0 to 3; a promise (21) names its state row (light-cljs-790).
        if n.kind == node_kind::Generator && (n.length > generator_state::Completed || n.function != source::no_row() && !cont_is(s, n.function, 0) || n.length != generator_state::Completed && n.function == source::no_row()) || n.kind == node_kind::Promise && !cont_is(s, n.function, 2) { return reject(c, Error.Link); }
        // An atom or volatile (17) has a flavor, 0 or 1; a cell reference (18,
        // no state of its own) 0, 1 or 2, a delay, whose value is its thunk
        // or nothing; a job's own delay (19) none (source::Node, cells.goose).
        if n.kind == node_kind::Atom && n.length > 1 || n.kind == node_kind::Cell && (n.length > 2 || !n.value.is_undefined() && (n.length != 2 || !n.value.is_object()) || n.function > 2147483647) || n.kind == node_kind::Delay && n.length != 0 || (n.kind == node_kind::Map || n.kind == node_kind::Set) && n.length > map_order::Hash || n.kind == node_kind::Concat && n.length > seq_flavor::IntegerRange || n.kind == node_kind::Vector && n.length > vector_flavor::MapEntry || n.kind == node_kind::List && n.length > list_flavor::Cons { return reject(c, Error.Link); }
        // A binding cell's mode (light-cljs-919, source::binding_mode): a
        // sealable cell is mutable, a sealed one initialized and immutable.
        if n.kind == node_kind::Binding && (n.length > binding_mode::Sealed || n.length == binding_mode::Sealable && n.immutable || n.length == binding_mode::Sealed && (!n.immutable || !n.initialized)) { return reject(c, Error.Link); }
        if n.kind == node_kind::Array && (n.length > l.values || n.length > 4294967295) { return reject(c, Error.Allocator); }
        // An array's `function` holds its attribute flags (light-cljs-881,
        // source::array_flag); only a dictionary array is Attributed, and
        // only a dense, non-extensible one Sealed, and Frozen only with
        // Sealed (light-cljs-882).
        if n.kind == node_kind::Array && (n.function > (array_flag::ReadOnlyLength | array_flag::Attributed | array_flag::Sealed | array_flag::Frozen) || n.function & array_flag::Attributed != 0 && !n.initialized || n.function & (array_flag::Sealed | array_flag::Frozen) != 0 && (n.initialized || !n.non_extensible || n.function & array_flag::Sealed == 0)) { return reject(c, Error.Link); }
        // [[Extensible]] cleared (light-cljs-882): only on a kind with own
        // properties, and a closure's lazy properties made first.
        if n.non_extensible && (!named(n.kind) || n.kind == node_kind::Closure && n.key_count == 0 && !n.initialized) { return reject(c, Error.Link); }
        // A key list (node_kind::KeyList): its walk position lies in its
        // records, and it walks an object or nothing.
        if (n.kind == node_kind::Object || n.kind == node_kind::Arguments) && n.length != 0 || n.kind == node_kind::KeyList && (n.count != 0 || n.initialized || n.start > n.length || n.length > n.key_count || !n.value.is_undefined() && !n.value.is_object()) { return reject(c, Error.Link); }
        if !value(s, n.value) || !value(s, n.namespace_value) || !value(s, n.metadata) || n.source != source::no_row() && n.source >= s.sources.len || n.origin < -1 || !n.proto.is_hole() && !n.proto.is_null() && (!n.proto.is_object() || !value(s, n.proto)) { return reject(c, Error.Link); }
        has_memo = has_memo || shaped(n) && n.key_capacity > 0 || n.kind == node_kind::ArgumentMap && n.count > 0 || n.kind == node_kind::Collection && n.count > 0;
        if !argument_links(s, n, row, c) { return reject(c, Error.Link); }
        if n.source != source::no_row() && (n.span_end < n.span_start || !range(n.span_start, n.span_end - n.span_start, s.sources[n.source].count)) { return reject(c, Error.Link); }
        // The compiler checks an identity source's UTF-8 boundaries.
        if n.source != source::no_row() && source::has_text(s.sources[n.source]) {
            let r = s.sources[n.source]; let text = s.bytes[r.start..(r.start + r.count)];
            if !program::boundary(text, n.span_start) || !program::boundary(text, n.span_end) { return reject(c, Error.Link); }
        }
        // A closure's value is its home object (light-cljs-888), if any.
        if n.kind == node_kind::Closure && (!range(n.function, 1, s.functions.len) || n.count != s.functions[n.function].captures || !n.value.is_undefined() && !n.value.is_object()) { return reject(c, Error.Link); }
        // A class constructor makes its properties when it is made
        // (light-cljs-886): it is never a lazy closure.
        if n.kind == node_kind::Closure && s.functions[n.function].kind >= constructor::BaseClass && n.key_count == 0 && !n.initialized { return reject(c, Error.Link); }
        if has_values(n.kind) { if !range(n.start, n.count, s.values.len) || n.kind == node_kind::Map && n.count % 2 != 0 { return reject(c, Error.Link); } }
        // A Map or Set (light-cljs-943): its run's layout (collection_holes),
        // whose holes are allowed, as an array's are.
        if n.kind == node_kind::Collection {
            if !poll(c, n.count) { return false; }
            let h = collection_holes(s, n, c);
            if h == -2 { return false; }
            if h < 0 { return reject(c, Error.Link); }
            array_holes += h;
        }
        if n.kind == node_kind::StructType {
            if !poll(c, n.count) { return false; }
            for v in s.values[n.start..(n.start + n.count)] { if !v.is_string() { return reject(c, Error.Link); } }
        }
        if elements(n.kind) {
            // A dictionary array (initialized) has records and no run; a dense
            // one has a run, and its named properties in a shape
            // (light-cljs-805, key_runs). An ordinary object's element run
            // (light-cljs-804) has no length.
            if n.kind == node_kind::Array && n.initialized && n.count != 0 { return reject(c, Error.Link); }
            has_memo = has_memo || n.count > 0;
            if !poll(c, n.count) { return false; }
            // Slots at or past an array's length are holes (source::array_length).
            for v, i in s.values[n.start..(n.start + n.count)] { if v.is_hole() { array_holes += 1; } else if n.kind == node_kind::Array && i >= n.length { return reject(c, Error.Link); } }
        }
        if n.kind == node_kind::LazySeq {
            has_memo = has_memo || n.count > 0;
            if n.length > n.count { return reject(c, Error.Link); }
            let input = n.namespace_value;
            if input.tag() > value_tag::Null && (!input.is_object() || (s.nodes[input.id()].kind != node_kind::List && s.nodes[input.id()].kind != node_kind::Concat) || s.nodes[input.id()].count != n.count) || input.tag() <= value_tag::Null && n.count != 0 { return reject(c, Error.Link); }
        }
        // A built-in function's code names a Builtin (source::BUILTINS); an
        // error object's, the catchable fault it was raised for or 0, and
        // its value a fault's error's original message or undefined.
        if n.kind == node_kind::Builtin && (n.function >= source::builtin_count() || n.function == 9) || n.kind == node_kind::Error && n.function > 5 { return reject(c, Error.Unsupported); }
        if n.kind == node_kind::Error && !n.value.is_undefined() && (n.function == 0 || !n.value.is_string()) { return reject(c, Error.Link); }
        if (n.kind == node_kind::Symbol || n.kind == node_kind::Keyword) && (!n.value.is_string() || !n.namespace_value.is_null() && !n.namespace_value.is_string()) || n.kind == node_kind::Scalar && n.value.tag() > value_tag::String { return reject(c, Error.Link); }
        // Metadata is a form's map or, on an object that can hold them, its
        // private elements, a later row no other object names
        // (light-cljs-889, source::private_holder).
        if !n.metadata.is_undefined() && !n.metadata.is_null() && (!n.metadata.is_object() || s.nodes[n.metadata.id()].kind != node_kind::Map) {
            if !n.metadata.is_object() || s.nodes[n.metadata.id()].kind != node_kind::PrivateElements || !source::private_holder(n.kind) || n.metadata.id() <= row { return reject(c, Error.Link); }
            owners += 1;
        }
        if n.kind == node_kind::PrivateElements { private_rows += 1; }
    }
    if private_rows > 0 && !private_owners(s, c, owners, private_rows) { return false; }
    // The registry (light-cljs-885) is a chain from the header through every
    // registered symbol row, each once: a row off the chain would be a
    // second symbol for its key.
    var chained: i64 = 0; var at = s.header.registry;
    while !at.is_null() {
        if !poll(c) { return false; }
        if !registered(s, at) || chained >= registrations { return reject(c, Error.Link); }
        chained += 1; at = s.nodes[source::symbol_node(at)].namespace_value;
    }
    if chained != registrations { return reject(c, Error.Link); }
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
    var v = View { header: Header { base: heap.identity, checkpoint: checkpoint, limits: heap.limits, work: heap.work, global: heap.global, call_method: heap.call_method, boot: heap.boot_count, initializers: heap.initializer_count, producers: heap.producer_count, property_rows: heap.property_rows, registry: heap.registry, random: heap.random },
        code_bytes: code.source_bytes, functions: code.functions, code_constants: code.constants, children: code.children, ops: code.ops, code_units: code.units, code_strings: code.strings,
        nodes:      heap.nodes, values: heap.values, properties: heap.properties, units: heap.units, strings: heap.strings, constants: heap.constants, bytes: heap.source_bytes, sources: heap.source_ranges, bindings: bindings, latin1: heap.latin1[..source::latin1_len(heap)],
        shapes: heap.shapes, accessors: heap.accessors, keys: heap.keys, conts: heap.conts, sites: code.sites, site_sources: code.site_sources, site_records: code.site_records, patterns: code.patterns, pattern_ops: code.pattern_ops };
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
        nodes: s.nodes, values: s.values, properties: s.properties, units: s.units, strings: s.strings, constants: s.constants, bytes: s.bytes, sources: s.sources, bindings: s.bindings, latin1: s.latin1, shapes: s.shapes, accessors: s.accessors, keys: s.keys, conts: s.conts,
        sites: s.sites, site_sources: s.site_sources, site_records: s.site_records, patterns: s.patterns, pattern_ops: s.pattern_ops };
}
// Reset only after the dispatch's job has retired. This reestablishes every
// restore destination invariant; restore still validates the entire image and
// requires a fresh identity. No references/roots may cross this boundary.
fn reset(code: source::Code&, heap: source::Heap&, bindings: Binding[>..]&) -> bool {
    if !(heap.code .== code) || heap.live_jobs != 0 { return false; }
    bindings.clear();
    heap.nodes.clear(); heap.values.clear(); heap.properties.clear(); heap.units.clear(); heap.latin1.clear(); heap.latin1_spare = 0; heap.strings.clear(); heap.constants.clear(); heap.source_bytes.clear(); heap.source_ranges.clear();
    heap.shapes.clear(); heap.accessors.clear(); heap.keys.clear(); heap.interned = 0; heap.image_written = false; heap.conts.clear();
    code.source_bytes.clear(); code.functions.clear(); code.constants.clear(); code.children.clear(); code.ops.clear(); code.units.clear(); code.strings.clear(); code.ins.clear(); code.effects.clear(); code.heights.clear(); code.decoded = 0;
    code.sites.clear(); code.site_sources.clear(); code.site_records.clear(); code.patterns.clear(); code.pattern_ops.clear();
    code.valid = false; code.error = tiny::LoadError.None; code.error_pc = -1;
    heap.identity = 0; heap.limits = source::Limits {}; heap.work = 0; heap.fault = source::Fault.None; heap.global = source::Value.Undefined; heap.call_method = source::Value.Undefined; heap.registry = source::Value.Null; heap.random = 0;
    heap.boot_count = 0; heap.initializer_count = 0; heap.producer_count = 0; heap.property_rows = 0; heap.image = source::Tops {};
    return true;
}
fn restore(s: const Snapshot&, code: source::Code&, heap: source::Heap&, bindings: Binding[>..]&, identity: i64, c: Control&) -> bool {
    let started = clock_ns(); defer { c.restore_ns += clock_ns() - started; }
    if !(heap.code .== code) || heap.identity != 0 || heap.live_jobs != 0 || heap.nodes.len != 0 || heap.values.len != 0 || heap.properties.len != 0 || heap.units.len != 0 || heap.latin1.len != 0 || heap.strings.len != 0 || heap.constants.len != 0 || heap.source_bytes.len != 0 || heap.source_ranges.len != 0 || heap.shapes.len != 0 || heap.accessors.len != 0 || heap.keys.len != 0 || heap.conts.len != 0 || code.valid || code.source_bytes.len != 0 || code.functions.len != 0 || code.constants.len != 0 || code.children.len != 0 || code.ops.len != 0 || code.units.len != 0 || code.strings.len != 0 || code.sites.len != 0 || code.site_sources.len != 0 || code.site_records.len != 0 || code.patterns.len != 0 || code.pattern_ops.len != 0 || bindings.len != 0 { return reject(c, Error.Destination); }
    if identity <= 0 || identity > 2147483647 || identity == s.header.base || identity == s.header.checkpoint { return reject(c, Error.Identity); }
    if !validate(s, c) { return false; }
    // No initialization, producer, publication cache, claim or waiter code.
    code.source_bytes.append(s.code_bytes); code.functions.append(s.functions); code.constants.append(s.code_constants); code.children.append(s.children); code.ops.append(s.ops); code.units.append(s.code_units); code.strings.append(s.code_strings);
    code.sites.append(s.sites); code.site_sources.append(s.site_sources); code.site_records.append(s.site_records); code.patterns.append(s.patterns); code.pattern_ops.append(s.pattern_ops); source::decode(code); code.valid = true;
    heap.identity = identity; heap.limits = s.header.limits; heap.work = s.header.work; heap.global = s.header.global; heap.call_method = s.header.call_method; heap.registry = s.header.registry; heap.random = s.header.random;
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
    code.sites.resize(twin.sites.len); code.site_sources.resize(twin.site_sources.len); code.site_records.resize(twin.site_records.len); code.patterns.resize(twin.patterns.len); code.pattern_ops.resize(twin.pattern_ops.len);
    code.ins.resize(twin.ops.len); code.effects.resize(twin.ops.len); code.heights.resize(twin.functions.len); code.decoded = twin.functions.len;
    heap.nodes.resize(twin.nodes.len); heap.values.resize(twin.values.len); heap.properties.resize(twin.properties.len);
    heap.units.resize(twin.units.len); heap.latin1.resize(twin.latin1.len); heap.latin1_spare = 0; heap.strings.resize(twin.strings.len); heap.constants.resize(twin.constants.len);
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
    if n.kind == node_kind::Object { for k in n.start..(n.start + n.count) { heap.values[k] = from.values[k]; } }
}
// The heap's state fields, as a restore of this header sets them.
fn state(h: const Header&, identity: i64, heap: source::Heap&) {
    heap.identity = identity; heap.limits = h.limits; heap.work = h.work; heap.global = h.global; heap.call_method = h.call_method; heap.registry = h.registry; heap.random = h.random; heap.fault = source::Fault.None;
    heap.boot_count = h.boot; heap.initializer_count = h.initializers; heap.producer_count = h.producers; heap.property_rows = h.property_rows;
}
fn same_rows<T>(a: const T[:], b: const T[:]) -> bool { return a.len == b.len && bytes_of(a) == bytes_of(b); }
// The reset check: every table and state field equals the twin's, byte for
// byte (tests and BBP_RESET_CHECK=1).
fn pristine(twin: const Snapshot&, identity: i64, code: const source::Code&, heap: const source::Heap&, bindings: const Binding[:]) -> bool {
    let h = twin.header;
    if heap.identity != identity || !same_bits(heap.limits, h.limits) || heap.work != h.work || !source::same_bits(heap.global, h.global) || !source::same_bits(heap.call_method, h.call_method) || !source::same_bits(heap.registry, h.registry) || heap.random != h.random || heap.fault != source::Fault.None || heap.live_jobs != 0 { return false; }
    if heap.boot_count != h.boot || heap.initializer_count != h.initializers || heap.producer_count != h.producers || heap.property_rows != h.property_rows || code.decoded != twin.functions.len || code.heights.len != twin.functions.len || code.ins.len != twin.ops.len { return false; }
    return same_rows(code.source_bytes, twin.code_bytes) && same_rows(code.functions, twin.functions) && same_rows(code.constants, twin.code_constants) && same_rows(code.children, twin.children) && same_rows(code.ops, twin.ops) && same_rows(code.units, twin.code_units) && same_rows(code.strings, twin.code_strings)
        && same_rows(code.sites, twin.sites) && same_rows(code.site_sources, twin.site_sources) && same_rows(code.site_records, twin.site_records) && same_rows(code.patterns, twin.patterns) && same_rows(code.pattern_ops, twin.pattern_ops)
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
    for r in service { let n = twin.nodes[r]; for k in n.keys..(n.keys + n.key_capacity) { if shaped(n) { slots.push(k); } else { keys.push(k); } } if n.kind == node_kind::Object { for k in n.start..(n.start + n.count) { slots.push(k); } } }
    for row, i in twin.nodes { var skip = false; for r in service { if r == i { skip = true; } } if !skip && !same_rows(s.nodes[i..(i + 1)], twin.nodes[i..(i + 1)]) { return false; } }
    for row, i in twin.properties { var skip = false; for k in keys { if k == i { skip = true; } } if !skip && !same_rows(s.properties[i..(i + 1)], twin.properties[i..(i + 1)]) { return false; } }
    for row, i in twin.values { var skip = false; for k in slots { if k == i { skip = true; } } if !skip && !same_rows(s.values[i..(i + 1)], twin.values[i..(i + 1)]) { return false; } }
    // Image shapes keep their layout; only their newest child may differ.
    for e, i in twin.shapes { var x = s.shapes[i]; x.child = e.child; if !same_rows([x], [e]) { return false; } }
    if !same_rows(s.accessors[..twin.accessors.len], twin.accessors) || !same_rows(s.conts[..twin.conts.len], twin.conts) { return false; }
    return same_rows(s.code_bytes[..twin.code_bytes.len], twin.code_bytes) && same_rows(s.functions[..twin.functions.len], twin.functions) && same_rows(s.code_constants[..twin.code_constants.len], twin.code_constants) && same_rows(s.children[..twin.children.len], twin.children)
        && same_rows(s.ops[..twin.ops.len], twin.ops) && same_rows(s.code_units[..twin.code_units.len], twin.code_units) && same_rows(s.code_strings[..twin.code_strings.len], twin.code_strings)
        && same_rows(s.sites[..twin.sites.len], twin.sites) && same_rows(s.site_sources[..twin.site_sources.len], twin.site_sources) && same_rows(s.site_records[..twin.site_records.len], twin.site_records) && same_rows(s.patterns[..twin.patterns.len], twin.patterns) && same_rows(s.pattern_ops[..twin.pattern_ops.len], twin.pattern_ops)
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
    if !(heap.code .== code) || heap.live_jobs != 0 || heap.latin1_spare != 0 { return reject(c, Error.Destination); }
    let current = borrow(heap, bindings, 1); if ends(current) != twin.header.ends { return reject(c, Error.Destination); }
    if identity <= 0 || identity > 2147483647 || identity == s.header.base || identity == s.header.checkpoint { return reject(c, Error.Identity); }
    if !validate(s, c) { return false; }
    let a = ends(s); let b = twin.header.ends; for n, i in b { if a[i] < n { return reject(c, Error.Destination); } }
    code.source_bytes.append(s.code_bytes[twin.code_bytes.len..]); code.functions.append(s.functions[twin.functions.len..]); code.constants.append(s.code_constants[twin.code_constants.len..]); code.children.append(s.children[twin.children.len..]);
    code.ops.append(s.ops[twin.ops.len..]); code.units.append(s.code_units[twin.code_units.len..]); code.strings.append(s.code_strings[twin.code_strings.len..]);
    code.sites.append(s.sites[twin.sites.len..]); code.site_sources.append(s.site_sources[twin.site_sources.len..]); code.site_records.append(s.site_records[twin.site_records.len..]); code.patterns.append(s.patterns[twin.patterns.len..]); code.pattern_ops.append(s.pattern_ops[twin.pattern_ops.len..]); source::decode(code);
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
// A snapshot over a base image, kept as what it adds to that image
// (light-cljs-960): the coordinator keeps each prepared definition so, not as
// a whole copy that repeats the base, and rebuilds the whole snapshot
// (rebase) for every job it starts from one, which the job's worker
// validates and installs over its image as before (extend). `above` holds
// the snapshot's header and bindings and every table's rows from the base's
// length on. The patches are the image rows the snapshot holds differently
// from the base, by row, in the tables a job writes below the image's
// lengths: the rows of compiler services and their property and element
// rows (nodes, values, properties), transitions from image shapes (shapes)
// and keys interned into the image's key index (keys; a rehash rewrites
// it). Every other table is append-only: delta refuses a snapshot whose rows
// below the base's length differ there. So rebase(delta(s, base), base) is
// s, byte for byte. `image` names the base (its checkpoint identity).
struct Delta {
    image:      i64   = 0, above: Snapshot = Snapshot {},
    node_rows:  u32[] = [], nodes: source::Node[] = [], value_rows: u32[] = [], values: source::Value[] = [], property_rows: u32[] = [], properties: source::Property[] = [],
    shape_rows: u32[] = [], shapes: source::Shape[] = [], key_rows: u32[] = [], keys: source::KeySlot[] = [],
}
// Whether `rows` starts with `image`'s rows.
fn over<T>(rows: const T[:], image: const T[:]) -> bool { return rows.len >= image.len && same_rows(rows[..image.len], image); }
// The rows below image's length where `rows` differs from `image`, and their
// contents in `rows`.
fn changed<T>(rows: const T[:], image: const T[:], at: u32[>..]&, out: T[>..]&) {
    if same_rows(rows[..image.len], image) { return; }
    for e, i in image { if !same_rows(rows[i..(i + 1)], image[i..(i + 1)]) { at.push(i as u32); out.push(rows[i]); } }
}
// `s` as rows over `base`; false when s is not over base: a table shorter
// than base's, or an append-only table whose rows below base's length are
// not base's.
fn delta(s: const Snapshot&, base: const Snapshot&) -> Delta, bool {
    if !(over(s.code_bytes, base.code_bytes) && over(s.functions, base.functions) && over(s.code_constants, base.code_constants) && over(s.children, base.children) && over(s.ops, base.ops) && over(s.code_units, base.code_units) && over(s.code_strings, base.code_strings)
         && over(s.units, base.units) && over(s.strings, base.strings) && over(s.constants, base.constants) && over(s.bytes, base.bytes) && over(s.sources, base.sources) && over(s.latin1, base.latin1) && over(s.accessors, base.accessors) && over(s.conts, base.conts)
         && over(s.sites, base.sites) && over(s.site_sources, base.site_sources) && over(s.site_records, base.site_records) && over(s.patterns, base.patterns) && over(s.pattern_ops, base.pattern_ops)) { return Delta {}, false; }
    if s.nodes.len < base.nodes.len || s.values.len < base.values.len || s.properties.len < base.properties.len || s.shapes.len < base.shapes.len || s.keys.len < base.keys.len { return Delta {}, false; }
    var node_rows:     u32[>..] = []; var nodes: source::Node[>..] = []; changed(s.nodes, base.nodes, node_rows, nodes);
    var value_rows:    u32[>..] = []; var values: source::Value[>..] = []; changed(s.values, base.values, value_rows, values);
    var property_rows: u32[>..] = []; var properties: source::Property[>..] = []; changed(s.properties, base.properties, property_rows, properties);
    var shape_rows:    u32[>..] = []; var shapes: source::Shape[>..] = []; changed(s.shapes, base.shapes, shape_rows, shapes);
    var key_rows:      u32[>..] = []; var keys: source::KeySlot[>..] = []; changed(s.keys, base.keys, key_rows, keys);
    return Delta { image: base.header.checkpoint, above: Snapshot { header: s.header,
            code_bytes: s.code_bytes[base.code_bytes.len..], functions: s.functions[base.functions.len..], code_constants: s.code_constants[base.code_constants.len..], children: s.children[base.children.len..], ops: s.ops[base.ops.len..], code_units: s.code_units[base.code_units.len..], code_strings: s.code_strings[base.code_strings.len..],
            nodes:      s.nodes[base.nodes.len..], values: s.values[base.values.len..], properties: s.properties[base.properties.len..], units: s.units[base.units.len..], strings: s.strings[base.strings.len..], constants: s.constants[base.constants.len..], bytes: s.bytes[base.bytes.len..], sources: s.sources[base.sources.len..], bindings: copy(s.bindings), latin1: s.latin1[base.latin1.len..],
            shapes:     s.shapes[base.shapes.len..], accessors: s.accessors[base.accessors.len..], keys: s.keys[base.keys.len..], conts: s.conts[base.conts.len..],
            sites: s.sites[base.sites.len..], site_sources: s.site_sources[base.site_sources.len..], site_records: s.site_records[base.site_records.len..], patterns: s.patterns[base.patterns.len..], pattern_ops: s.pattern_ops[base.pattern_ops.len..] },
        node_rows: node_rows[..], nodes: nodes[..], value_rows: value_rows[..], values: values[..], property_rows: property_rows[..], properties: properties[..], shape_rows: shape_rows[..], shapes: shapes[..], key_rows: key_rows[..], keys: keys[..] }, true;
}
fn joined<T>(image: const T[:], rows: const T[:]) -> T[>..] { var out: T[>..] = []; out.append(image); out.append(rows); return out; }
fn patched<T>(image: const T[:], rows: const T[:], at: const u32[:], contents: const T[:]) -> T[>..] { var out = joined(image, rows); for r, i in at { out[r] = contents[i]; } return out; }
// The whole snapshot `d` keeps over `base`, which must be the base it was
// taken over.
fn rebase(d: const Delta&, base: const Snapshot&) -> Snapshot {
    assert(d.image == base.header.checkpoint); let a .= d.above;
    return Snapshot { header: a.header,
        code_bytes: joined(base.code_bytes, a.code_bytes), functions: joined(base.functions, a.functions), code_constants: joined(base.code_constants, a.code_constants), children: joined(base.children, a.children), ops: joined(base.ops, a.ops), code_units: joined(base.code_units, a.code_units), code_strings: joined(base.code_strings, a.code_strings),
        nodes:      patched(base.nodes, a.nodes, d.node_rows, d.nodes), values: patched(base.values, a.values, d.value_rows, d.values), properties: patched(base.properties, a.properties, d.property_rows, d.properties),
        units:      joined(base.units, a.units), strings: joined(base.strings, a.strings), constants: joined(base.constants, a.constants), bytes: joined(base.bytes, a.bytes), sources: joined(base.sources, a.sources), bindings: copy(a.bindings), latin1: joined(base.latin1, a.latin1),
        shapes:     patched(base.shapes, a.shapes, d.shape_rows, d.shapes), accessors: joined(base.accessors, a.accessors), keys: patched(base.keys, a.keys, d.key_rows, d.keys), conts: joined(base.conts, a.conts),
        sites: joined(base.sites, a.sites), site_sources: joined(base.site_sources, a.site_sources), site_records: joined(base.site_records, a.site_records), patterns: joined(base.patterns, a.patterns), pattern_ops: joined(base.pattern_ops, a.pattern_ops) };
}
// The bytes a Delta holds, as footprint counts a snapshot's.
fn delta_footprint(d: const Delta&) -> i64 {
    let empty: Delta[] = [Delta {}];
    return bytes_of(empty).len + payload_bytes(d.above) + bytes_of(d.node_rows).len + bytes_of(d.nodes).len + bytes_of(d.value_rows).len + bytes_of(d.values).len + bytes_of(d.property_rows).len + bytes_of(d.properties).len
         + bytes_of(d.shape_rows).len + bytes_of(d.shapes).len + bytes_of(d.key_rows).len + bytes_of(d.keys).len;
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

// StringMethod's state is entirely Values and operand pieces, preserved
// by SavedFrame and reclamation's general frame traversal (light-cljs-903).
fn string_frame_valid<T>(s: const T&, f: source::SavedFrame, b: source::Builtin) -> bool {
    if source::string_pad(b) { return f.pc == string_pc::PadFill && f.receiver.is_string() && f.callee.is_number() && f.callee.num() > s.strings[f.receiver.sid()].len && f.callee.num() <= 9007199254740991.0 && trunc(f.callee.num()) == f.callee.num(); }
    if b == source::Builtin.StringPrototypeSplit {
        if f.pc < string_pc::SplitMethod || f.pc > string_pc::SplitScan { return false; }
        if f.pc <= string_pc::SplitText { return true; }
        if !f.receiver.is_string() { return false; }
        if f.pc == string_pc::SplitLimit { return true; }
        if !f.callee.is_number() || !(f.callee.num() >= 0 && f.callee.num() <= 4294967295) || trunc(f.callee.num()) != f.callee.num() { return false; }
        if f.pc == string_pc::SplitSeparator { return true; }
        if !f.input.is_string() || !f.output.is_object() || s.nodes[f.output.id()].kind != node_kind::Array || s.nodes[f.output.id()].length >= f.callee.num() { return false; }
        let len = s.strings[f.receiver.sid()].len as i64; let size = s.strings[f.input.sid()].len as i64;
        return f.index >= 0 && f.index <= len && f.argument_count >= 0 && (if size == 0 { f.argument_count == 0 } else { f.argument_count < size && f.index + f.argument_count <= len });
    }
    if source::string_points(b) { return (f.pc == string_pc::Points || f.pc == string_pc::PointValue || f.pc == string_pc::Finish) && f.argument_count >= 0 && f.index >= 0 && f.index <= f.argument_count && (f.pc != string_pc::PointValue || f.index < f.argument_count) && (f.pc != string_pc::Finish || f.index == f.argument_count); }
    if source::string_search(b) {
        if !f.receiver.is_string() || f.pc < string_pc::Match || f.pc > string_pc::Scan { return false; }
        if f.pc == string_pc::Match { return source::string_predicate(b) && f.input.is_object(); }
        if f.pc == string_pc::Needle { return true; }
        if !f.input.is_string() { return false; }
        if f.pc == string_pc::Position { return true; }
        let len = s.strings[f.receiver.sid()].len as i64; let size = s.strings[f.input.sid()].len as i64;
        return size > 0 && f.index >= -1 && f.index <= len + 1 && f.argument_count >= 0 && f.argument_count < size;
    }
    if source::string_well_formed(b) { return (f.pc == string_pc::WellFormed || f.pc == string_pc::Finish && b == source::Builtin.StringPrototypeToWellFormed) && f.receiver.is_string() && f.index >= 0 && f.index <= s.strings[f.receiver.sid()].len && (f.pc != string_pc::Finish || f.index == s.strings[f.receiver.sid()].len); }
    if b != source::Builtin.StringRaw || f.pc < string_pc::Raw || f.pc > string_pc::SubstitutionCopy && f.pc != string_pc::Finish || !f.receiver.is_object() || f.argument_count < 1 || f.index < 0 { return false; }
    if f.pc == string_pc::Raw || f.pc == string_pc::Length { return f.index == 0; }
    if !f.callee.is_number() || f.callee.num() < 1 || f.callee.num() > 9007199254740991.0 || trunc(f.callee.num()) != f.callee.num() || f.index >= f.callee.num() { return false; }
    if f.pc == string_pc::Finish { return f.index + 1 == f.callee.num(); }
    if f.pc == string_pc::Substitution || f.pc == string_pc::SubstitutionCopy { if f.index + 1 >= f.argument_count { return false; } }
    if f.pc == string_pc::PieceCopy || f.pc == string_pc::SubstitutionCopy { return f.input.is_string() && f.output.is_number() && f.output.num() >= 0 && f.output.num() <= s.strings[f.input.sid()].len && trunc(f.output.num()) == f.output.num(); }
    return true;
}
