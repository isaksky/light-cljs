import std;
import math;
import .source_vm;
import .realm;
import .array_methods;
import .array_callbacks;
import .object_statics;
import .function_calls;
import .iterators;
import .math_object;

namespace source;

// Map and Set objects (light-cljs-943; decisions 2 to 6 of
// docs/javascript-coverage.md, "Map and Set"; the Pants engine's
// object/map.rs, collections/map.rs and vm/native/add_entries.rs, QuickJS's
// js_map_constructor, map_normalize_key, map_hash_key, map_find_record,
// js_map_set, js_map_get, js_map_has, js_map_delete, js_map_clear and
// js_map_get_size).
//
// A Map or Set is one node_kind::Collection row, an ordinary object (own
// properties, integrity levels, prototype) with internal slots:
//   length    its flavor (collection::Map, collection::Set, and the weak
//             flavors collection::WeakMap and collection::WeakSet of
//             light-cljs-945, below)
//   start     its run in heap.values: `capacity` entries of
//   count     entry_stride values (key, a Map's value, insertion number),
//             then 2 * capacity index slots; count is capacity times
//             (stride + 2), and 0 for a collection with no run yet
//   function  the entries used in the run, live or deleted
//   value     its size, the live entries, as a Number
//   namespace_value  the next insertion number (never reset by clear)
// A deleted entry is a tombstone, its key and value Empty, its insertion
// number kept, so binary search can recover a cursor after compaction.
// Insertion numbers increase on appends, not updates, and survive rebuilds;
// iterators and forEach keep the next number, never a physical offset.
// Entries keep their insertion order; entries past `function` are Empty too. The
// index is open-addressed with linear probing over 2 * capacity slots,
// each Empty or a Number, an entry position: every live entry's position
// is in it once (a tombstone's may be), so it is never more than half
// full. Keys are SameValueZero: a number by value, -0 stored as +0 and
// every NaN one key; a string by its code units, whatever its header; a
// keyword form by namespace and name, as `===` compares the V8 build's
// interned keywords (source_vm.goose same_keyword); any other object and a
// symbol by identity, its row; booleans, undefined and null by value. A
// BigInt key fails by name until BigInts have a payload (light-cljs-771).
// Hashing and comparing run no JavaScript. An object's or symbol row's
// hash is its row, so in-job reclamation, which moves rows, rebuilds the
// index of every collection holding such a key (reclaim.goose
// rehash_collections); a snapshot copies rows at their places, so its
// collections need nothing.
//
// The run is append-only between rebuilds: set and add append, delete
// leaves a tombstone. An append to a full run rebuilds it at the smallest
// power-of-two capacity of at least twice the live entries (at least
// collection_minimum()): in place, compacting the live entries, when that
// is the capacity it has, so a map that sets and deletes one key forever
// keeps one small run (bbp-287); otherwise into a new run of the table's
// end, the old one becoming garbage that in-job reclamation or the job's
// truncation drops (no free lists). clear drops the run. A run no node owns
// any longer is cleared to undefined, as an array's old element run is.
//
// The keyed methods, the size getters and @@species are plain built-ins
// (decision 5). A write (set, add, a delete that finds its key, a clear of
// a nonempty collection) into a collection the job did not allocate, an
// image row the core's boot made, faults NativeBoundary before it mutates
// (decision 3); reads of image collections work. The constructors run
// their iterable argument in a Collection native frame (decision 4), one
// entry a step (decision 5); see "Construction" below.
//
// WeakMap and WeakSet (light-cljs-945; ECMAScript 24.3, 24.4; the Pants
// engine's object/weak_map.rs and vm/builtin.rs, QuickJS's
// js_map_constructor, js_map_set, js_map_get, js_map_has and js_map_delete
// with MAGIC_WEAK, and js_weakref_is_target) are the same Collection rows
// in the weak flavors, laid out as a Map and a Set are (insertion
// positions included, which nothing iterates), so the run, its index,
// construction, the write check, publication refusal, snapshot validation
// and reclamation's rehash are the same code. Their keys are what
// CanBeHeldWeakly accepts (held_weakly): an object, or a symbol that
// Symbol.for did not register; they hash and compare by identity as a
// Map's object and symbol keys do (a keyword form by namespace and name,
// as `===` compares it). WeakMap.prototype.set and WeakSet.prototype.add
// throw a TypeError for any other key; get, has and delete answer
// undefined or false for one. They have no size, clear, forEach or
// iterators. Liveness is the implementation's to approximate: until
// light-cljs-951 makes their entries ephemerons, in-job reclamation traces
// their keys and values strongly, as a Map's, and the job's truncation
// drops them with their rows.

// A collection's flavor, its node's `length`.
let collection::Map = 0;
let collection::Set = 1;
let collection::WeakMap = 2;
let collection::WeakSet = 3;
// Whether a flavor's entries hold a value beside their key: Map and WeakMap.
inline fn valued_flavor(flavor: i64) -> bool { return flavor == collection::Map || flavor == collection::WeakMap; }
// Whether a flavor's keys are held weakly: WeakMap and WeakSet.
inline fn weak_flavor(flavor: i64) -> bool { return flavor == collection::WeakMap || flavor == collection::WeakSet; }
// The smallest entry capacity of a run.
inline fn collection_minimum() -> i64 { return 4; }
// The values an entry takes: key, a Map's or WeakMap's value, insertion position.
inline fn entry_stride(flavor: i64) -> i64 { return if valued_flavor(flavor) { 3 } else { 2 }; }
// Positions are exact Numbers. Exhaustion is a work limit, never a wrap.
inline fn collection_position_limit() -> f64 { return 9007199254740991.0; }
fn collection_position(v: Value) -> bool { return v.is_number() && v.num() >= 0 && v.num() <= collection_position_limit() && trunc(v.num()) == v.num(); }
// Whether v is a collection of `flavor`.
fn is_collection(heap: const Heap&, v: Value, flavor: i64) -> bool {
    if !v.is_object() || v.id() >= heap.nodes.len { return false; }
    let n .= node(heap, v.id());
    return n.kind == node_kind::Collection && n.length == flavor;
}
// The entry capacity of collection row n's run.
fn entry_capacity(n: const Node&) -> i64 { let count: i64 = n.count; let flavor: i64 = n.length; return count / (entry_stride(flavor) + 2); }
// Collection row n's size.
inline fn collection_size(n: const Node&) -> i64 { return n.value.num() as i64; }
// The flavor plain collection built-in b (collection_method) or collection
// constructor b belongs to: its receiver's or its object's.
fn builtin_flavor(b: Builtin) -> i64 {
    return match b {
        Map, MapPrototypeGet, MapPrototypeSet, MapPrototypeHas, MapPrototypeDelete, MapPrototypeClear, MapPrototypeSize, MapSpecies => collection::Map,
        WeakMap, WeakMapPrototypeGet, WeakMapPrototypeSet, WeakMapPrototypeHas, WeakMapPrototypeDelete => collection::WeakMap,
        WeakSet, WeakSetPrototypeAdd, WeakSetPrototypeHas, WeakSetPrototypeDelete => collection::WeakSet,
        _ => collection::Set,
    };
}
// The flavor collection constructor b makes, -1 for another built-in
// (snapshot validation of a Collection frame).
fn constructor_flavor(b: Builtin) -> i64 {
    return match b { Map => collection::Map, Set => collection::Set, WeakMap => collection::WeakMap, WeakSet => collection::WeakSet, _ => -1 };
}
// Whether built-in b is a plain collection built-in (collection_builtin).
fn collection_method(b: Builtin) -> bool {
    return match b {
        MapPrototypeGet, MapPrototypeSet, MapPrototypeHas, MapPrototypeDelete, MapPrototypeClear, MapPrototypeSize, MapSpecies,
        SetPrototypeAdd, SetPrototypeHas, SetPrototypeDelete, SetPrototypeClear, SetPrototypeSize, SetSpecies,
        WeakMapPrototypeGet, WeakMapPrototypeSet, WeakMapPrototypeHas, WeakMapPrototypeDelete,
        WeakSetPrototypeAdd, WeakSetPrototypeHas, WeakSetPrototypeDelete => true,
        _ => false,
    };
}
// The %Map.prototype%-like row of `flavor`, its objects' default prototype.
fn collection_prototype_row(flavor: i64) -> i64 {
    return if flavor == collection::Map { map_prototype_row() } else if flavor == collection::Set { set_prototype_row() } else if flavor == collection::WeakMap { weak_map_prototype_row() } else { weak_set_prototype_row() };
}
// CanBeHeldWeakly(v) (ECMAScript 9.13): an object, or a symbol Symbol.for
// did not register (a well-known symbol or a job's Symbol()). A weak
// flavor's keys are these.
fn held_weakly(heap: const Heap&, v: Value) -> bool { return v.is_object() || v.is_symbol() && !registered_symbol(heap, v); }
// Whether built-in b is an accessor's getter: the size getters and
// @@species (realm.goose build_realm).
fn collection_accessor(b: Builtin) -> bool { return b == Builtin.MapPrototypeSize || b == Builtin.SetPrototypeSize || b == Builtin.MapSpecies || b == Builtin.SetSpecies; }

// ---- Keys
// A keyword form that compares by namespace and name (same_keyword):
// not cljs.core/keyword's fresh one.
inline fn keyword_key(heap: const Heap&, o: i64) -> bool { let n .= node(heap, o); return n.kind == node_kind::Keyword && !n.initialized; }
fn same_named_key(heap: const Heap&, a: i64, b: i64) -> bool {
    let x .= node(heap, a); let y .= node(heap, b);
    if !same_text(heap, x.value, y.value) || x.namespace_value.is_string() != y.namespace_value.is_string() { return false; }
    return !x.namespace_value.is_string() || same_text(heap, x.namespace_value, y.namespace_value);
}
// SameValueZero of a stored key and a key, without running JavaScript or
// charging work (the operation charged it). A tombstone (Empty) is no key.
fn same_entry_key(heap: const Heap&, a: Value, b: Value) -> bool {
    return match a {
        Number x => b.is_number() && (x.v == b.num() || x.v != x.v && b.num() != b.num()),
        String => b.is_string() && same_text(heap, a, b),
        Object x => b.is_object() && (x.o == b.id() || keyword_key(heap, x.o) && keyword_key(heap, b.id()) && same_named_key(heap, x.o, b.id())),
        Symbol x => b.is_symbol() && x.s == symbol_index(b),
        Bool x => b.is_bool() && x.v == b.flag(),
        Undefined => b.is_undefined(),
        Null => b.is_null(),
        _ => false,
    };
}
// splitmix64's finalizer, folded to 32 bits.
fn mix_hash(x: u64) -> u32 {
    var z = x;
    z = (z ^ (z >> 30)) * 13787848793156543929;
    z = (z ^ (z >> 27)) * 10723151780598845931;
    z = z ^ (z >> 31);
    return ((z ^ (z >> 32)) & 4294967295) as u32;
}
// A number key's hash: equal keys (+0 and -0, every NaN) hash alike.
fn number_entry_hash(x: f64) -> u32 {
    if x != x { return mix_hash(9221120237041090560); }
    if x == 0 { return mix_hash(0); }
    if x == trunc(x) && x >= -9007199254740991.0 && x <= 9007199254740991.0 { let i: i64 = x as i64; return mix_hash(i as! u64); }
    return mix_hash(f64_bits(x));
}
// The hash of key v (not a BigInt, Empty or Accessor), consistent with
// same_entry_key: a string's code units whatever its header (key_hash,
// FNV-1a), a keyword form's name and namespace the same way, any other
// object's or symbol's row. It reads no JavaScript-visible state and
// charges nothing, so in-job reclamation can rehash with it. Snapshot
// validation hashes over a snapshot's tables with the same pieces
// (snapshot.goose collection_key_hash).
fn entry_hash(heap: const Heap&, v: Value) -> u32 {
    return match v {
        String x => string_entry_hash(key_hash(heap, header(heap, x.s))),
        Object x => {
            if !keyword_key(heap, x.o) { plain_entry_hash(v) } else {
                let n .= node(heap, x.o);
                keyword_entry_hash(key_hash(heap, header(heap, n.value.sid())), n.namespace_value.is_string(), if n.namespace_value.is_string() { key_hash(heap, header(heap, n.namespace_value.sid())) } else { 0 })
            }
        },
        _ => plain_entry_hash(v),
    };
}
// The hash of a key that reads no table: undefined, null, a boolean, a
// number, a symbol, or an object (not a keyword form) by its row.
fn plain_entry_hash(v: Value) -> u32 {
    return match v {
        Undefined => mix_hash(1), Null => mix_hash(2),
        Bool x => mix_hash(if x.v { 4 } else { 3 }),
        Number x => number_entry_hash(x.v),
        Symbol x => mix_hash((x.s as u64) | 8589934592),
        Object x => mix_hash((x.o as u64) | 12884901888),
        _ => 0,
    };
}
// A string key's hash from its units' FNV-1a hash.
inline fn string_entry_hash(units: u32) -> u32 { return mix_hash((units as u64) | 4294967296); }
// A keyword form's hash from its name's and namespace's FNV-1a hashes.
fn keyword_entry_hash(name: u32, qualified: bool, space: u32) -> u32 { return mix_hash(((name as u64) << 32) | space) ^ (if qualified { 2654435769 } else { 0 }); }
// The stored form of key k: -0 is +0.
inline fn canonical_key(k: Value) -> Value { return if k.is_number() && k.num() == 0 { number(0) } else { k }; }

// ---- The run
// The position of key k (hash h) in collection o, or -1. The probe stops
// at an Empty slot; a slot's position names an entry of the run (snapshot
// validation checks it), and at most 2 * capacity slots are visited.
fn find_entry(heap: const Heap&, o: i64, k: Value, h: u32) -> i64 {
    let n .= node(heap, o);
    if n.count == 0 { return -1; }
    let flavor: i64 = n.length; let stride = entry_stride(flavor); let capacity = entry_capacity(n);
    let start:  i64 = n.start; let index = start + capacity * stride; let mask = 2 * capacity - 1;
    var j: i64 = h & mask;
    for step in 2 * capacity {
        let s = slot(heap, index + j);
        if s.is_hole() { return -1; }
        let p: i64 = s.num() as i64;
        if same_entry_key(heap, slot(heap, start + p * stride), k) { return p; }
        j = (j + 1) & mask;
    }
    return -1;
}
// Index position p (hash h) in the index of `slots` slots at `index`.
fn index_entry(heap: Heap&, index: i64, slots: i64, h: u32, p: i64) {
    let mask = slots - 1; var j: i64 = h & mask;
    while !slot(heap, index + j).is_hole() { j = (j + 1) & mask; }
    let f: f64 = p;
    set_slot(heap, index + j, number(f));
}
// The capacity a rebuild of a run holding `size` live entries takes.
fn rebuilt_capacity(size: i64) -> i64 {
    var capacity = collection_minimum();
    while capacity < 2 * size { capacity *= 2; }
    return capacity;
}
// Rebuild collection o's run for one more entry (see above); false with a
// fault (MemoryLimit, WorkLimit).
fn rebuild_run(heap: Heap&, o: i64) -> bool {
    let n = node(heap, o);
    let flavor: i64 = n.length; let stride = entry_stride(flavor);
    let size = collection_size(n); let used: i64 = n.function; let old: i64 = n.start;
    let capacity = rebuilt_capacity(size); let total = capacity * (stride + 2);
    let count: i64 = n.count;
    if !charge(heap, total + count) { return false; }
    if n.count > 0 && capacity == entry_capacity(n) {
        // Compaction in place: live entries slide down, keeping their
        // order; then the index is rebuilt.
        var p: i64 = 0;
        for i in used {
            let k = slot(heap, old + i * stride);
            if k.is_hole() { continue; }
            if p != i {
                for j in stride { set_slot(heap, old + p * stride + j, slot(heap, old + i * stride + j)); set_slot(heap, old + i * stride + j, Value.Empty); }
            }
            p += 1;
        }
        // Deleted entries kept positions for binary search. Once the live
        // prefix is compacted, unused entries must be entirely Empty.
        for j in (p * stride)..(used * stride) { set_slot(heap, old + j, Value.Empty); }
        node_mut(heap, o).function = p as u32;
        reindex_run(heap, o);
        return true;
    }
    let at = add_values(heap, [], total, Value.Empty);
    if at < 0 { return false; }
    var p: i64 = 0;
    for i in used {
        let k = slot(heap, old + i * stride);
        if k.is_hole() { continue; }
        for j in stride { set_slot(heap, at + p * stride + j, slot(heap, old + i * stride + j)); }
        p += 1;
    }
    // No node owns the old run now: holes may live only in the runs their
    // nodes own (snapshot validation counts them), and nothing it held
    // stays reachable through it.
    drop_run(heap, old, count);
    let m .= node_mut(heap, o); m.start = at as u32; m.count = total as u32; m.function = p as u32;
    reindex_run(heap, o);
    return true;
}
// Clear collection o's index and index every live entry again (a rebuild,
// and in-job reclamation once object and symbol rows moved: their hashes
// are their rows). Charges nothing.
fn reindex_run(heap: Heap&, o: i64) {
    let n = node(heap, o);
    if n.count == 0 { return; }
    let flavor: i64 = n.length; let stride = entry_stride(flavor); let capacity = entry_capacity(n);
    let start:  i64 = n.start; let index = start + capacity * stride; let used: i64 = n.function;
    for j in 2 * capacity { set_slot(heap, index + j, Value.Empty); }
    for p in used {
        let k = slot(heap, start + p * stride);
        if !k.is_hole() { index_entry(heap, index, 2 * capacity, entry_hash(heap, k), p); }
    }
}
// Append the entry of canonical key k (hash h, not present) and value v
// to collection o; false with a fault.
fn append_entry(heap: Heap&, o: i64, k: Value, v: Value, h: u32) -> bool {
    let position = node(heap, o).namespace_value.num();
    if position >= collection_position_limit() { heap.fault = Fault.WorkLimit; return false; }
    if node(heap, o).count == 0 || node(heap, o).function >= entry_capacity(node(heap, o)) {
        if !rebuild_run(heap, o) { return false; }
    }
    let n = node(heap, o);
    let flavor: i64 = n.length; let stride = entry_stride(flavor); let capacity = entry_capacity(n);
    let start:  i64 = n.start; let p: i64 = n.function;
    set_slot(heap, start + p * stride, k);
    if valued_flavor(flavor) { set_slot(heap, start + p * stride + 1, v); }
    set_slot(heap, start + p * stride + stride - 1, number(position));
    index_entry(heap, start + capacity * stride, 2 * capacity, h, p);
    let size: f64 = collection_size(n) + 1;
    let m .= node_mut(heap, o); m.function = (p + 1) as u32; m.value = number(size); m.namespace_value = number(position + 1);
    return true;
}
// Set entry k of collection o to v (a Set's or WeakSet's v is unused): its
// value when k is present, else a new entry; false with a fault. The
// caller checked that the job may write o, that a weak flavor may hold k
// (held_weakly) and charged the operation.
fn put_entry(heap: Heap&, o: i64, k: Value, v: Value) -> bool {
    let key = canonical_key(k); let h = entry_hash(heap, key);
    let p = find_entry(heap, o, key, h);
    if p < 0 { return append_entry(heap, o, key, v, h); }
    let n .= node(heap, o); let flavor: i64 = n.length;
    if valued_flavor(flavor) { let at: i64 = n.start + p * entry_stride(flavor) + 1; set_slot(heap, at, v); }
    return true;
}
// Delete the entry at position p of collection o: a tombstone.
fn remove_entry(heap: Heap&, o: i64, p: i64) {
    let n = node(heap, o);
    let flavor: i64 = n.length; let stride = entry_stride(flavor); let start: i64 = n.start;
    set_slot(heap, start + p * stride, Value.Empty);
    if valued_flavor(flavor) { set_slot(heap, start + p * stride + 1, Value.Empty); }
    let size: f64 = collection_size(n) - 1;
    node_mut(heap, o).value = number(size);
}
// Clear a run no node owns any longer to undefined (element_room's rule).
fn drop_run(heap: Heap&, start: i64, count: i64) { for at in start..(start + count) { set_slot(heap, at, Value.Undefined); } }
// Clear collection o: it drops its run, charged.
fn clear_entries(heap: Heap&, o: i64) -> bool {
    let start: i64 = node(heap, o).start; let count: i64 = node(heap, o).count;
    if !charge(heap, count) { return false; }
    drop_run(heap, start, count);
    let m .= node_mut(heap, o); m.start = 0; m.count = 0; m.function = 0; m.value = number(0);
    return true;
}
// A new, empty collection of `flavor` with prototype `proto`.
fn new_collection(heap: Heap&, flavor: i64, proto: Value) -> Value {
    return allocate(heap, Node { kind: node_kind::Collection, length: flavor as u32, value: number(0), namespace_value: number(0), proto: proto });
}
// Whether the job may write collection o; otherwise its NativeBoundary.
fn collection_writable(job: Job&, o: i64) -> bool {
    if writable(job, o) { return true; }
    let flavor: i64 = node(job.heap, o).length;
    image_write(job, Value.Undefined, if flavor == collection::Map { "a Map entry" } else if flavor == collection::Set { "a Set entry" } else if flavor == collection::WeakMap { "a WeakMap entry" } else { "a WeakSet entry" });
    return false;
}
// A key built-in b cannot take yet: a BigInt (light-cljs-771). A weak
// flavor never gets here: a BigInt is no key it can hold (weak_key_error).
fn bigint_key(job: Job&, b: Builtin) -> Value {
    job.detail.clear();
    for c in (if builtin_flavor(b) == collection::Map { "Map key: BigInt (light-cljs-771)" } else { "Set key: BigInt (light-cljs-771)" }) { job.detail.push(c); }
    if job.heap.fault == Fault.None { job.heap.fault = Fault.UnsupportedOperand; }
    return Value.Undefined;
}
// WeakMap.prototype.set's and WeakSet.prototype.add's TypeError for a key
// CanBeHeldWeakly refuses (QuickJS's message).
fn weak_key_error(job: Job&, flavor: i64) -> Value { return type_error(job, if flavor == collection::WeakMap { "invalid value used as WeakMap key" } else { "invalid value used as WeakSet key" }); }
// The work an operation on key k costs: one, and a string's length, which
// hashing and comparing it read.
inline fn key_work(heap: const Heap&, k: Value) -> i64 { return 1 + str_len(heap, k); }

// ---- The plain built-ins
// Map.prototype's get, set, has, delete, clear and size, Set.prototype's
// add, has, delete, clear and size, @@species (its receiver), and
// WeakMap.prototype's get, set, has and delete and WeakSet.prototype's add,
// has and delete (light-cljs-945). A receiver that is not a collection of
// the method's flavor is a TypeError, a Map for a WeakMap method too.
fn collection_builtin(job: Job&, b: Builtin, receiver: Value, args: const Value[:]) -> Value {
    let heap .= job.heap;
    if b == Builtin.MapSpecies || b == Builtin.SetSpecies { return receiver; }
    let flavor = builtin_flavor(b);
    if !is_collection(heap, receiver, flavor) {
        return type_error(job, if flavor == collection::Map { "Map method called on an object that is not a Map" } else if flavor == collection::Set { "Set method called on an object that is not a Set" }
            else if flavor == collection::WeakMap { "WeakMap method called on an object that is not a WeakMap" } else { "WeakSet method called on an object that is not a WeakSet" });
    }
    let o: i64 = receiver.id();
    if b == Builtin.MapPrototypeSize || b == Builtin.SetPrototypeSize { let size: f64 = collection_size(node(heap, o)); return number(size); }
    if b == Builtin.MapPrototypeClear || b == Builtin.SetPrototypeClear {
        if node(heap, o).count == 0 { return Value.Undefined; }
        if !charge(heap) || !collection_writable(job, o) { return Value.Undefined; }
        clear_entries(heap, o); return Value.Undefined;
    }
    let k = arg(args, 0);
    let adds = b == Builtin.MapPrototypeSet || b == Builtin.SetPrototypeAdd || b == Builtin.WeakMapPrototypeSet || b == Builtin.WeakSetPrototypeAdd;
    // A key a weak flavor cannot hold: set and add throw, get, has and
    // delete find nothing (ECMAScript 24.3.3.2 to 24.3.3.5, 24.4.3.1 to
    // 24.4.3.4).
    if weak_flavor(flavor) && !held_weakly(heap, k) {
        if !charge(heap) { return Value.Undefined; }
        if adds { return weak_key_error(job, flavor); }
        return if b == Builtin.WeakMapPrototypeGet { Value.Undefined } else { boolean(false) };
    }
    if k.is_bigint() { return bigint_key(job, b); }
    if !valid(heap, k) || !charge(heap, key_work(heap, k)) { return Value.Undefined; }
    if adds {
        if !collection_writable(job, o) || !put_entry(heap, o, k, arg(args, 1)) { return Value.Undefined; }
        return receiver;
    }
    let key = canonical_key(k);
    let p = find_entry(heap, o, key, entry_hash(heap, key));
    if b == Builtin.MapPrototypeGet || b == Builtin.WeakMapPrototypeGet { return if p < 0 { Value.Undefined } else { slot(heap, node(heap, o).start + p * entry_stride(flavor) + 1) }; }
    if b == Builtin.MapPrototypeHas || b == Builtin.SetPrototypeHas || b == Builtin.WeakMapPrototypeHas || b == Builtin.WeakSetPrototypeHas { return boolean(p >= 0); }
    if b == Builtin.MapPrototypeDelete || b == Builtin.SetPrototypeDelete || b == Builtin.WeakMapPrototypeDelete || b == Builtin.WeakSetPrototypeDelete {
        if p < 0 { return boolean(false); }
        if !collection_writable(job, o) { return Value.Undefined; }
        remove_entry(heap, o, p); return boolean(true);
    }
    heap.fault = Fault.InvalidState; return Value.Undefined;
}

// ---- Live traversal (light-cljs-944)
// Recover a logical position after growth or compaction. Tombstones keep
// their position, so the ordered run can be searched without scanning an
// old prefix. The caller visits at most one entry (including tombstones)
// per native step. A plain next charges each tombstone it skips.
fn seek_entry(heap: Heap&, o: i64, position: i64) -> i64 {
    let n = node(heap, o); let flavor: i64 = n.length; let stride = entry_stride(flavor);
    let start: i64 = n.start; var lo: i64 = 0; var hi: i64 = n.function;
    while lo < hi {
        if !charge(heap) { return -1; }
        let mid = lo + (hi - lo) / 2;
        if slot(heap, start + mid * stride + stride - 1).num() < position { lo = mid + 1; } else { hi = mid; }
    }
    return if lo < n.function { lo } else { -1 };
}
fn collection_for_each(b: Builtin) -> bool { return b == Builtin.MapPrototypeForEach || b == Builtin.SetPrototypeForEach; }
// CreateMapIterator / CreateSetIterator: branded receivers, no conversion.
fn collection_iterator(job: Job&, b: Builtin, receiver: Value) -> Value {
    let map = b == Builtin.MapPrototypeKeys || b == Builtin.MapPrototypeValues || b == Builtin.MapPrototypeEntries;
    if !is_collection(job.heap, receiver, if map { collection::Map } else { collection::Set }) { return type_error(job, if map { "not a Map" } else { "not a Set" }); }
    let kind = match b { MapPrototypeKeys => iteration::MapKeys, MapPrototypeValues => iteration::MapValues, MapPrototypeEntries => iteration::MapEntries, SetPrototypeEntries => iteration::SetEntries, _ => iteration::SetValues };
    return allocate(job.heap, Node { kind: node_kind::Iterator, function: kind as u32, value: receiver, namespace_value: number(0), proto: object(if map { map_iterator_prototype_row() } else { set_iterator_prototype_row() }) });
}
// %MapIteratorPrototype%.next / %SetIteratorPrototype%.next. A done
// cursor releases its source. Each entry pair is a fresh two-element array.
fn collection_next(job: Job&, b: Builtin, receiver: Value) -> Value {
    let heap .= job.heap; let map = b == Builtin.MapIteratorPrototypeNext;
    if !receiver.is_object() || receiver.id() >= heap.nodes.len || node(heap, receiver.id()).kind != node_kind::Iterator || (if map { !map_iteration(node(heap, receiver.id()).function) } else { !set_iteration(node(heap, receiver.id()).function) }) { return type_error(job, if map { "not a Map Iterator" } else { "not a Set Iterator" }); }
    let c = receiver.id(); let source = node(heap, c).value;
    if source.is_undefined() { return iter_result(job, Value.Undefined, true); }
    if !writable(job, c) { image_write(job, Value.Undefined, "an iterator's position"); return Value.Undefined; }
    let n = node(heap, source.id()); let flavor: i64 = n.length; let stride = entry_stride(flavor); let start: i64 = n.start;
    var p = seek_entry(heap, source.id(), node(heap, c).namespace_value.num() as i64);
    if heap.fault != Fault.None { return Value.Undefined; }
    while p >= 0 && p < n.function {
        if !charge(heap) { return Value.Undefined; }
        let at = start + p * stride; let k = slot(heap, at);
        if !k.is_hole() {
            node_mut(heap, c).namespace_value = number(slot(heap, at + stride - 1).num() + 1);
            let kind = node(heap, c).function;
            let v = if map { slot(heap, at + 1) } else { k };
            var result = if kind == iteration::MapKeys { k } else { v };
            if kind == iteration::MapEntries || kind == iteration::SetEntries {
                result = allocate(heap, Node { kind: node_kind::Array });
                if heap.fault != Fault.None || !set_element(heap, result, 0, k) || !set_element(heap, result, 1, v) { return Value.Undefined; }
            }
            return iter_result(job, result, false);
        }
        p += 1;
    }
    node_mut(heap, c).value = Value.Undefined;
    return iter_result(job, Value.Undefined, true);
}

// Native.CollectionForEach holds input the collection, callee the callback,
// receiver thisArg and index the next insertion number. Each Next step
// visits one entry; Call discards the callback's result. All linked values
// are ordinary frame fields, carried by continuations and reclamation.
let for_each_pc::Next = 0;
let for_each_pc::Call = 1;
fn start_collection_for_each(job: Job&, b: Builtin, receiver: Value, args: const Value[:], base: i64) {
    let heap .= job.heap; let map = b == Builtin.MapPrototypeForEach;
    if !is_collection(heap, receiver, if map { collection::Map } else { collection::Set }) { type_error(job, if map { "not a Map" } else { "not a Set" }); return; }
    let callback = arg(args, 0); let this_arg = arg(args, 1);
    if !is_callable(heap, callback) { type_error(job, "forEach callback is not callable"); return; }
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
    job.operands.resize(base);
    job.frames.push(Frame { function: -1, pc: for_each_pc::Next, local_base: job.locals.len, operand_base: base, callee: callback, receiver: this_arg, input: receiver, native: Native.CollectionForEach { which: b } });
}
fn collection_for_each_step(job: Job&, frame: Frame) {
    let heap .= job.heap; let at = job.frames.len - 1;
    if frame.pc == for_each_pc::Call { job.operands.pop(); job.frames[at].pc = for_each_pc::Next; return; }
    if frame.pc != for_each_pc::Next { heap.fault = Fault.InvalidState; return; }
    let o = frame.input.id(); let p = seek_entry(heap, o, frame.index);
    if heap.fault != Fault.None { return; }
    if p < 0 { finish_native(job, frame, Value.Undefined); return; }
    let n = node(heap, o); let flavor: i64 = n.length; let stride = entry_stride(flavor); let entry: i64 = n.start + p * stride;
    job.frames[at].index = (slot(heap, entry + stride - 1).num() as i64) + 1;
    let k = slot(heap, entry);
    if k.is_hole() { return; }
    let v = if flavor == collection::Map { slot(heap, entry + 1) } else { k };
    job.frames[at].pc = for_each_pc::Call;
    call_accessor(job, frame.callee, frame.receiver, [v, k, frame.input]);
}

// ---- Construction
// new Map(iterable), new Set(iterable), new WeakMap(iterable) and new
// WeakSet(iterable) (ECMAScript 24.1.1.1, 24.2.1.1, 24.3.1.1, 24.4.1.1;
// AddEntriesFromIterable 24.1.1.2): the object comes first, from
// newTarget's `prototype` (construct reads it), then for an iterable other
// than undefined or null a Collection native frame (Native.Collection, its
// constructor the payload) whose steps each consume the one value they
// asked for: Get(collection, "set" or "add") once (a getter above it; a
// TypeError when not callable, before GetIterator), GetIterator, then per
// entry IteratorStep (`next`, the result's `done` and `value`, getters
// above), for a Map or WeakMap a TypeError when the item is not an object,
// its "0" and "1", then the adder called with the collection as `this` (a
// subclass's own `set` or `add` runs as a frame above). From the item on,
// the frame holds a Catch handler at collection_pc::Close (object_statics
// close_guard), so an abrupt completion of those reads or the adder closes
// the iterator: its `return` is read and called, and whatever it does the
// original exception is rethrown. An exception from `next`, `done` or
// `value` does not close it. The realm's own %Map.prototype.set%,
// %Set.prototype.add%, %WeakMap.prototype.set% or %WeakSet.prototype.add%
// as the adder on its flavor stores the entry without a call, which
// nothing observes; a weak flavor's throws its TypeError for a key it
// cannot hold there, which closes the iterator as the call's would.
//
// The fast path (collection_pc::Fast) skips the protocol where nothing
// could observe it: a dense array of the realm's %Array.prototype% with no
// own properties, while %Array.prototype%[@@iterator] is still
// %Array.prototype.values% and %ArrayIteratorPrototype%.next still its
// own, and the realm's adder. Its steps take element `index` of the array
// (and, for a Map or WeakMap, an entry that is a dense array's own
// elements 0 and 1), one a step. An element it cannot take so (a hole,
// which reads through the prototype chain, another entry, or a key a weak
// flavor cannot hold, whose TypeError closes an iterator whose `return`
// a script may have defined) leaves the fast path: a cursor
// over the array at that element and the record of the next that was read
// at the start take its place, and the protocol goes on from there, as if
// it had run from the start (an array iterator's `next` reads the length
// and the element at its index each time).
//
// The frame:
//   receiver  the collection, the result
//   output    the adder, from pc Record on
//   callee    the iterable until pc Record, then the iterator record (in
//             the fast path, the array)
//   input     the iterator result while its `done` and `value` are read,
//             a Map's entry while its "0" is read, then its key while its
//             "1" is; while the iterator closes, the exception
//   index     the fast path's next element
//   pc        the value awaited (collection_pc::), which arrives on the
//             operands; Fast awaits nothing
// It is frame state, so in-job reclamation keeps and moves it, a saved
// continuation carries it and snapshot validation checks it
// (snapshot.goose collection_frame_valid).
let collection_pc::Adder = 0;    // Get(collection, "set" or "add")
let collection_pc::Record = 1;   // GetIterator's record
let collection_pc::Step = 2;     // next()'s result
let collection_pc::Done = 3;     // the result's `done` from its getter
let collection_pc::Item = 4;     // the result's `value` from its getter
let collection_pc::Key = 5;      // a Map entry's "0" from its getter
let collection_pc::Value = 6;    // a Map entry's "1" from its getter
let collection_pc::Added = 7;    // the adder's result
let collection_pc::Close = 8;    // the exception that closes the iterator
let collection_pc::Return = 9;   // the iterator's `return` from its getter
let collection_pc::Closed = 10;  // `return`'s result, or its exception
let collection_pc::Fast = 11;    // the next element of a dense array, no protocol

// new Map(...), new Set(...), new WeakMap(...) or new WeakSet(...) (b), its
// object's prototype `proto` (Empty: the realm's), its operand base `base`
// (construct_builtin).
fn start_collection(job: Job&, b: Builtin, args: const Value[:], base: i64, proto: Value) {
    let heap .= job.heap;
    let flavor = constructor_flavor(b);
    let map = valued_flavor(flavor);
    let c = new_collection(heap, flavor, if proto.is_object() { proto } else { object(collection_prototype_row(flavor)) });
    if heap.fault != Fault.None { return; }
    let iterable = form_value(heap, arg(args, 0));
    if heap.fault != Fault.None { return; }
    job.operands.resize(base);
    if iterable.is_undefined() || iterable.is_null() { job.operands.push(c); return; }
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
    job.frames.push(Frame { function: -1, pc: collection_pc::Adder, local_base: job.locals.len, operand_base: base, callee: iterable, receiver: c, native: Native.Collection { which: b } });
    get_named(job, b, c, if map { "set" } else { "add" });
}
// Whether v is built-in function `which` (any of its function objects
// behaves alike).
fn builtin_is(heap: const Heap&, v: Value, which: Builtin) -> bool { return v.is_object() && v.id() < heap.nodes.len && node(heap, v.id()).kind == node_kind::Builtin && builtin_of(heap, v.id()) == which; }
// Whether `adder` is the realm's adder for collection constructor b.
fn realm_adder(heap: const Heap&, b: Builtin, adder: Value) -> bool {
    return builtin_is(heap, adder, if b == Builtin.Map { Builtin.MapPrototypeSet } else if b == Builtin.Set { Builtin.SetPrototypeAdd } else if b == Builtin.WeakMap { Builtin.WeakMapPrototypeSet } else { Builtin.WeakSetPrototypeAdd });
}
// Whether collection constructor b's entries are key-value pairs (Map,
// WeakMap) rather than values (Set, WeakSet).
inline fn entry_pairs(b: Builtin) -> bool { return b == Builtin.Map || b == Builtin.WeakMap; }
// Whether array v's elements are what its iteration would visit (see
// "The fast path"): a dense array of the realm's prototype with no own
// named properties, the realm's array iteration unchanged.
fn fast_source(heap: Heap&, v: Value) -> bool {
    if !v.is_object() || v.id() >= heap.nodes.len { return false; }
    let n .= node(heap, v.id());
    if n.kind != node_kind::Array || n.initialized || !n.proto.is_hole() || n.key_count != 0 { return false; }
    let values = own_entry(heap, object(array_prototype_row()), Value.Symbol { s: well_known::Iterator });
    if heap.fault != Fault.None || !values.found || !builtin_is(heap, values.value, Builtin.ArrayPrototypeValues) { return false; }
    let k = key_named(heap, "next");
    if heap.fault != Fault.None { return false; }
    let next = own_entry(heap, object(array_iterator_prototype_row()), k);
    return heap.fault == Fault.None && next.found && builtin_is(heap, next.value, Builtin.ArrayIteratorPrototypeNext);
}
// Whether the array the fast path walks is still one it can read directly.
fn fast_array(heap: const Heap&, v: Value) -> bool {
    if !v.is_object() || v.id() >= heap.nodes.len { return false; }
    let n .= node(heap, v.id());
    return n.kind == node_kind::Array && !n.initialized && n.proto.is_hole();
}
// Whether item e is a Map entry the fast path reads directly: a dense
// array whose own elements 0 and 1 are data.
fn fast_pair(heap: const Heap&, e: Value) -> bool {
    if !e.is_object() || e.id() >= heap.nodes.len { return false; }
    let n .= node(heap, e.id());
    return n.kind == node_kind::Array && !n.initialized && n.length >= 2 && n.count >= 2 && !slot(heap, n.start).is_hole() && !slot(heap, n.start + 1).is_hole();
}
// One step of the Collection frame at the top (see Construction).
fn collection_step(job: Job&, frame: Frame, b: Builtin) {
    let heap .= job.heap; let at = job.frames.len - 1;
    if frame.pc == collection_pc::Fast { fast_step(job, at, b); return; }
    if job.operands.len != frame.operand_base + 1 { heap.fault = Fault.InvalidState; return; }
    let v = job.operands.pop();
    match frame.pc {
        collection_pc::Adder => { take_adder(job, at, b, v); },
        collection_pc::Record => {
            if !v.is_object() { heap.fault = Fault.InvalidState; return; }
            job.frames[at].callee = v;
            next_entry(job, at, b);
        },
        collection_pc::Step => { entry_result(job, at, b, v); },
        collection_pc::Done => { entry_done(job, at, b, truthy(heap, form_value(heap, v))); },
        collection_pc::Item => { take_entry(job, at, b, v); },
        collection_pc::Key => { take_entry_key(job, at, b, v); },
        collection_pc::Value => { add_entry(job, at, b, job.frames[at].input, v); },
        collection_pc::Added => { close_unguard(job, at, collection_pc::Close); next_entry(job, at, b); },
        collection_pc::Close => { close_entries(job, at, b, v); },
        collection_pc::Return => { return_entries(job, at, v); },
        collection_pc::Closed => { rethrow_entries(job, at); },
        _ => { heap.fault = Fault.InvalidState; },
    }
}
// The adder arrived: a TypeError when not callable; then the fast path or
// GetIterator.
fn take_adder(job: Job&, at: i64, b: Builtin, adder: Value) {
    let heap .= job.heap;
    let f = form_value(heap, adder);
    if heap.fault != Fault.None { return; }
    if !is_callable(heap, f) {
        type_error(job, if b == Builtin.Map { "Map: 'set' is not a function" } else if b == Builtin.Set { "Set: 'add' is not a function" } else if b == Builtin.WeakMap { "WeakMap: 'set' is not a function" } else { "WeakSet: 'add' is not a function" });
        return;
    }
    job.frames[at].output = f;
    let iterable = job.frames[at].callee;
    if realm_adder(heap, b, f) && fast_source(heap, iterable) { job.frames[at].pc = collection_pc::Fast; job.frames[at].index = 0; return; }
    if heap.fault != Fault.None { return; }
    job.frames[at].pc = collection_pc::Record;
    get_iterator(job, iterable);
}
// IteratorStep: call `next`; its result arrives at pc Step.
fn next_entry(job: Job&, at: i64, b: Builtin) {
    let heap .= job.heap; let record = job.frames[at].callee;
    let it = record_field(job, record, "iterator");
    let next = record_field(job, record, "next");
    if heap.fault != Fault.None { return; }
    if !is_callable(heap, next) { type_error(job, "iterator next is not a function"); return; }
    job.frames[at].pc = collection_pc::Step; job.frames[at].input = Value.Undefined;
    call_accessor(job, next, it, []);
}
// `next`'s result r: an object, whose `done` is read (a getter above).
fn entry_result(job: Job&, at: i64, b: Builtin, r: Value) {
    let heap .= job.heap;
    if !r.is_object() { type_error(job, "iterator result is not an object"); return; }
    job.frames[at].input = r;
    let ready, done = read_field(job, at, b, r, "done", collection_pc::Done);
    if ready { entry_done(job, at, b, truthy(heap, form_value(heap, done))); }
}
// IteratorComplete: the collection is delivered, or the item's `value` read.
fn entry_done(job: Job&, at: i64, b: Builtin, done: bool) {
    if done { finish_native(job, job.frames[at], job.frames[at].receiver); return; }
    let ready, v = read_field(job, at, b, job.frames[at].input, "value", collection_pc::Item);
    if ready { take_entry(job, at, b, v); }
}
// Item v arrived: from here an abrupt completion closes the iterator. A
// Set or WeakSet adds it; a Map's or WeakMap's must be an object, whose
// "0" is read.
fn take_entry(job: Job&, at: i64, b: Builtin, v: Value) {
    let heap .= job.heap;
    job.frames[at].input = Value.Undefined;
    close_guard(job, at, collection_pc::Close);
    if !entry_pairs(b) { add_entry(job, at, b, v, Value.Undefined); return; }
    let e = form_value(heap, v);
    if heap.fault != Fault.None { return; }
    if !e.is_object() { type_error(job, "iterator value is not an entry object"); return; }
    job.frames[at].input = e;
    let ready, k = read_field(job, at, b, e, "0", collection_pc::Key);
    if ready { take_entry_key(job, at, b, k); }
}
// A Map entry's key k arrived: its "1" is read. While a getter reads it,
// the frame keeps k in place of the entry, which the getter holds.
fn take_entry_key(job: Job&, at: i64, b: Builtin, k: Value) {
    let heap .= job.heap;
    let ready, v = read_field(job, at, b, job.frames[at].input, "1", collection_pc::Value);
    if heap.fault != Fault.None { return; }
    if ready { add_entry(job, at, b, k, v); return; }
    job.frames[at].input = k;
}
// Call the adder with key k and value v (a Set's or WeakSet's v unused):
// the realm's adder stores the entry here (a weak flavor's throws for a key
// it cannot hold, which the frame's handler catches and closes the
// iterator for); any other runs above this frame and its result arrives
// at pc Added.
fn add_entry(job: Job&, at: i64, b: Builtin, k: Value, v: Value) {
    let heap .= job.heap;
    let adder = job.frames[at].output; let c = job.frames[at].receiver;
    if realm_adder(heap, b, adder) {
        let flavor = constructor_flavor(b);
        if weak_flavor(flavor) && !held_weakly(heap, k) { if charge(heap) { weak_key_error(job, flavor); } return; }
        if k.is_bigint() { bigint_key(job, b); return; }
        if !valid(heap, k) || !charge(heap, key_work(heap, k)) || !put_entry(heap, c.id(), k, v) { return; }
        close_unguard(job, at, collection_pc::Close);
        next_entry(job, at, b); return;
    }
    job.frames[at].pc = collection_pc::Added; job.frames[at].input = Value.Undefined;
    if entry_pairs(b) { call_accessor(job, adder, c, [k, v]); } else { call_accessor(job, adder, c, [k]); }
}
// IteratorClose after an abrupt completion: the exception e arrived at the
// frame's handler. `return` is read and called with a handler of its own
// (pc Closed); whatever it does, e is rethrown.
fn close_entries(job: Job&, at: i64, b: Builtin, e: Value) {
    let heap .= job.heap;
    job.frames[at].input = e;
    let it = record_field(job, job.frames[at].callee, "iterator");
    if heap.fault != Fault.None { return; }
    close_guard(job, at, collection_pc::Closed);
    let ready, m = read_field(job, at, b, it, "return", collection_pc::Return);
    if heap.fault != Fault.None { return; }
    if ready { return_entries(job, at, m); }
}
fn return_entries(job: Job&, at: i64, method: Value) {
    let heap .= job.heap;
    let m = form_value(heap, method);
    if heap.fault != Fault.None { return; }
    if m.tag() <= value_tag::Null || !is_callable(heap, m) { rethrow_entries(job, at); return; }
    let it = record_field(job, job.frames[at].callee, "iterator");
    if heap.fault != Fault.None { return; }
    job.frames[at].pc = collection_pc::Closed;
    call_accessor(job, m, it, []);
}
// Rethrow the exception that closed the iterator.
fn rethrow_entries(job: Job&, at: i64) {
    close_unguard(job, at, collection_pc::Closed);
    let frame = job.frames[at];
    raise(job, frame.input, frame);
}
// One step of the fast path: element `index` of the array is stored, the
// array is done, or the protocol takes over at that element.
fn fast_step(job: Job&, at: i64, b: Builtin) {
    let heap .= job.heap; let frame = job.frames[at];
    let a = frame.callee; let i = frame.index;
    if !fast_array(heap, a) { leave_fast(job, at, b); return; }
    let n = node(heap, a.id());
    if i >= n.length { finish_native(job, frame, frame.receiver); return; }
    let item = if i < n.count { slot(heap, n.start + i) } else { Value.Empty };
    if item.is_hole() || entry_pairs(b) && !fast_pair(heap, item) { leave_fast(job, at, b); return; }
    var k = item; var v = Value.Undefined;
    if entry_pairs(b) { let e = node(heap, item.id()); k = slot(heap, e.start); v = slot(heap, e.start + 1); }
    // A key a weak flavor cannot hold throws, which closes the iterator:
    // the protocol, from this element, reads its `return`.
    if weak_flavor(constructor_flavor(b)) && !held_weakly(heap, k) { leave_fast(job, at, b); return; }
    if k.is_bigint() { bigint_key(job, b); return; }
    if !valid(heap, k) || !charge(heap, key_work(heap, k)) || !put_entry(heap, frame.receiver.id(), k, v) { return; }
    job.frames[at].index = i + 1;
}
// Leave the fast path at element `index`: a cursor over the array there
// and the record of the realm's %ArrayIteratorPrototype%.next, which the
// start found in place, then IteratorStep.
fn leave_fast(job: Job&, at: i64, b: Builtin) {
    let heap .= job.heap; let frame = job.frames[at];
    let cursor = new_cursor(job, frame.callee, iteration::Values);
    if heap.fault != Fault.None { return; }
    node_mut(heap, cursor.id()).length = frame.index as u32;
    let record = allocate(heap, Node { kind: node_kind::Object });
    if heap.fault != Fault.None || !realm_value(heap, record.id(), "iterator", cursor, attrs_data()) || !realm_value(heap, record.id(), "next", object(realm_function_row(Builtin.ArrayIteratorPrototypeNext)), attrs_data()) { return; }
    job.frames[at].callee = record;
    next_entry(job, at, b);
}
