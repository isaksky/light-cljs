import std;
import .source_vm;
import .export_schema;
import .realm;
import .printer;
import .integrity;
import .private;
import .reclaim;

namespace source;

// The plain form operations (light-cljs-918): cljs.core's builtin
// predicates, the keyword and symbol constructors and readers, cons, peek
// and pop, dissoc and disj, the seq of a map, set or string, hash, compare
// and the reduced marker. None runs user code or needs storage beyond the
// node kinds the VM has. The Pants engine's collections/clj.rs and form.rs
// are their specification; the errors cljs.core raises take the V8 build's
// wording (src/js_runtime.js; light-cljs-873), classified here and worded
// by core_error. form_op dispatches each from its arm (src/source_vm.goose).
//
// A map's seq is its entries as map entries, two-element vectors whose
// `length` is vector_flavor::MapEntry, in its traversal order
// (traversal_order), as the V8 build's runtime walks it; a set's is its
// members in that order. An update of a map entry (assoc, conj, pop,
// with-meta) is an ordinary vector, as ClojureScript's MapEntry gives.
// A cons is a list of its item and its collection's seq that keeps that
// collection (or seq) as its rest and is no stack (list_flavor::Cons).

// Whether v is a map entry (vector_flavor::MapEntry).
fn entry_value(heap: Heap&, v: Value) -> bool { let x = form_value(heap, v); return x.is_object() && node(heap, x.id()).kind == node_kind::Vector && node(heap, x.id()).length == vector_flavor::MapEntry; }
// cljs.core's predicates of v, read through an occurrence, never
// realizing a lazy sequence or reading an element (clj.rs is_list and its
// siblings): false for a value of no form kind.
fn form_predicate(heap: Heap&, op: i64, v: Value) -> bool {
    let x = form_value(heap, v);
    if heap.fault != Fault.None { return false; }
    if op == form_op::CljIsFalse { return x.is_bool() && !x.flag(); }
    if op == form_op::CljIsZero { return x.is_number() && x.num() == 0; }
    if !x.is_object() { return false; }
    let n = node(heap, x.id()); let kind = n.kind;
    let entry = kind == node_kind::Vector && n.length == vector_flavor::MapEntry;
    return match op {
        // An atom (a job's or a cell's), not a volatile or a delay.
        form_op::CljIsAtom => kind == node_kind::Atom && n.length == cell_flavor::Atom || kind == node_kind::Cell && n.length == cell_flavor::Atom,
        // A record too (light-cljs-920; clj.rs is_coll).
        form_op::CljIsColl => kind >= node_kind::List && kind <= node_kind::Set || kind == node_kind::Concat || kind == node_kind::LazySeq || kind == node_kind::Struct && record_value(heap, x),
        form_op::CljIsEditable => kind == node_kind::Vector && !entry || kind == node_kind::Map || kind == node_kind::Set,
        form_op::CljIsList => kind == node_kind::List,
        form_op::CljIsMapEntry => entry,
        form_op::CljIsSeq => kind == node_kind::List || kind == node_kind::Concat || kind == node_kind::LazySeq,
        form_op::CljIsSequential => kind == node_kind::List || kind == node_kind::Vector || kind == node_kind::Concat || kind == node_kind::LazySeq,
        form_op::CljIsVector => kind == node_kind::Vector,
        form_op::CljIsReduced => kind == node_kind::Reduced,
        // An array map or set: insertion-ordered (map_order).
        form_op::IsArrayMap => (kind == node_kind::Map || kind == node_kind::Set) && n.length == map_order::Insertion,
        form_op::HasHostKeys => (kind == node_kind::Map || kind == node_kind::Set) && host_keys(heap, n),
        _ => false,
    };
}
// Whether map or set `n` has a key (member) that is a JavaScript object, no
// form (form.rs host_key).
fn host_keys(heap: Heap&, n: Node) -> bool {
    let stride: i64 = if n.kind == node_kind::Map { 2 } else { 1 };
    if !charge(heap, n.count / stride) { return false; }
    for i in (n.count / stride) {
        let k = form_value(heap, slot(heap, n.start + i * stride));
        if !k.is_object() { continue; }
        let kind = node(heap, k.id()).kind;
        if !(kind >= node_kind::List && kind <= node_kind::Keyword || kind == node_kind::Concat || kind == node_kind::LazySeq || kind == node_kind::Delay || kind == node_kind::Reduced) { return true; }
    }
    return false;
}

// `name` (qualifier false) or `namespace` of a keyword or symbol (form.rs
// name): a string, or a namespace's null; a TypeError for another value.
fn form_name(heap: Heap&, v: Value, qualifier: bool) -> Value {
    let x = form_value(heap, v); let kind = form_kind(heap, x);
    if heap.fault != Fault.None { return Value.Undefined; }
    // A struct type's qualified name, as one unqualified name (form.rs name).
    if kind == node_kind::StructType { return if qualifier { nil() } else { node(heap, x.id()).value }; }
    // A shareable regex value (light-cljs-935), the Pants engine's regex
    // compiler literal: its tag, unqualified.
    if kind == node_kind::RegExp && shared_regexp(heap, x.id()) { return if qualifier { nil() } else { text(heap, "regex") }; }
    if kind != node_kind::Keyword && kind != node_kind::Symbol { heap.fault = Fault.TypeError; return Value.Undefined; }
    let n = node(heap, x.id());
    return if qualifier { n.namespace_value } else { n.value };
}
// A new keyword or symbol (`kind`) of namespace `ns`, nil or a string, and
// `name`, a string; one whose spelling would not split back into them
// (named_supported) fails NativeBoundary, as form_op::Keyword does. A
// keyword is cljs.core/keyword's, fresh (`initialized`; same_keyword):
// identical only to itself, as the V8 build's keywordCore makes it.
fn new_named(heap: Heap&, kind: i64, ns: Value, name: Value) -> Value {
    if heap.fault != Fault.None { return Value.Undefined; }
    if !named_supported(heap, ns, name) { if heap.fault == Fault.None { heap.fault = Fault.NativeBoundary; } return Value.Undefined; }
    return allocate(heap, Node { kind: kind, value: name, namespace_value: if ns.is_string() { ns } else { nil() }, initialized: kind == node_kind::Keyword });
}
// The errors of the keyword and symbol constructors (the V8 build's
// keywordCore and symbolCore): `(keyword ns name)` takes nil or a string
// namespace and a string name, a keyword or symbol giving its name;
// `(symbol x)` a symbol, keyword or string; `(symbol ns name)` nil or a
// string and a string.
fn name_error(heap: Heap&, op: i64, args: const Value[:]) -> CoreError {
    if op == form_op::CljSymbol {
        let x = form_value(heap, args[0]); let kind = form_kind(heap, x);
        return if x.is_string() || kind == node_kind::Symbol || kind == node_kind::Keyword { CoreError.None } else { CoreError.SymbolConversion };
    }
    let keyword = op == form_op::CljKeywordNs;
    let ns = if keyword { keyword_part(heap, args[0]) } else { form_value(heap, args[0]) };
    let name = if keyword { keyword_part(heap, args[1]) } else { form_value(heap, args[1]) };
    if ns.tag() > value_tag::Null && !ns.is_string() || !name.is_string() { return if keyword { CoreError.KeywordArgs } else { CoreError.SymbolArgs }; }
    return CoreError.None;
}
// A keyword or symbol operand's name; any other operand as it is.
fn keyword_part(heap: Heap&, v: Value) -> Value {
    let x = form_value(heap, v); let kind = form_kind(heap, x);
    return if kind == node_kind::Keyword || kind == node_kind::Symbol { node(heap, x.id()).value } else { x };
}
// `(keyword x)` (clj.rs keyword): a keyword itself; a symbol's namespace
// and name; a string split at its one `/` (`"/"` is the name `/`); nil for
// anything else.
fn clj_keyword(heap: Heap&, v: Value) -> Value {
    let x = form_value(heap, v); let kind = form_kind(heap, x);
    if heap.fault != Fault.None { return Value.Undefined; }
    if kind == node_kind::Keyword { return v; }
    if kind == node_kind::Symbol { let n = node(heap, x.id()); return new_named(heap, node_kind::Keyword, n.namespace_value, n.value); }
    if !x.is_string() { return nil(); }
    let units = utf16(heap, x);
    if !charge(heap, units.len) { return Value.Undefined; }
    var slashes: i64 = 0; var at: i64 = -1;
    for c, i in units { if c == 47 { slashes += 1; if at < 0 { at = i; } } }
    if slashes != 1 || units.len == 1 { return new_named(heap, node_kind::Keyword, nil(), x); }
    let ns = string(heap, units[0..at]);
    let name = if heap.fault == Fault.None { string(heap, units[(at + 1)..]) } else { Value.Undefined };
    return new_named(heap, node_kind::Keyword, ns, name);
}
// `(symbol x)` of a symbol, keyword or string (name_error) (clj.rs
// symbol): a symbol itself; otherwise the spelling, a keyword's `ns/name`,
// split at its first `/` unless that leads it.
fn clj_symbol(heap: Heap&, v: Value) -> Value {
    let x = form_value(heap, v); let kind = form_kind(heap, x);
    if heap.fault != Fault.None { return Value.Undefined; }
    if kind == node_kind::Symbol { return v; }
    var units: u16[>..] = [];
    if kind == node_kind::Keyword {
        let n = node(heap, x.id());
        if n.namespace_value.is_string() { append_utf16(heap, n.namespace_value, units); units.push(47); }
        append_utf16(heap, n.value, units);
    } else { append_utf16(heap, x, units); }
    if !charge(heap, units.len) { return Value.Undefined; }
    var at: i64 = -1;
    for c, i in units { if c == 47 { at = i; break; } }
    if at < 1 { return new_named(heap, node_kind::Symbol, nil(), if kind == node_kind::Keyword { string(heap, units) } else { x }); }
    let ns = string(heap, units[0..at]);
    let name = if heap.fault == Fault.None { string(heap, units[(at + 1)..]) } else { Value.Undefined };
    return new_named(heap, node_kind::Symbol, ns, name);
}

// Append the items of v's seq to `out` (clj.rs seq_items): nil none; a
// string's code units as one-unit strings; a list's, vector's or
// sequence's elements; a map's entries as map entries and a set's members,
// in traversal order. False with a fault for anything else: callers raise
// seq_error's error first, so only a lazy sequence (form_op's boundary)
// or a key that does not hash here (traversal_order) reaches that.
fn seq_items(heap: Heap&, v: Value, out: Value[>..]&) -> bool {
    let x = form_value(heap, v);
    if heap.fault != Fault.None { return false; }
    if x.tag() <= value_tag::Null { return true; }
    if x.is_string() {
        let len = str_len(heap, x);
        if !charge(heap, len) { return false; }
        for i in len { let s = unit_string(heap, x, i); if heap.fault != Fault.None { return false; } out.push(s); }
        return true;
    }
    let kind = form_kind(heap, x);
    if kind == node_kind::List || kind == node_kind::Vector || kind == node_kind::Concat {
        let n = node(heap, x.id());
        if !charge(heap, n.count) { return false; }
        out.append(run(heap, n.start, n.count)); return true;
    }
    // A record's entries (light-cljs-920).
    if record_value(heap, x) { return record_entries(heap, x, out); }
    if kind != node_kind::Map && kind != node_kind::Set { heap.fault = Fault.NativeBoundary; return false; }
    let n = node(heap, x.id());
    var order: i64[>..] = [];
    if !traversal_order(heap, n, order) { if heap.fault == Fault.None { heap.fault = Fault.NativeBoundary; } return false; }
    if !charge(heap, order.len) { return false; }
    if kind == node_kind::Set { for i in order { out.push(slot(heap, n.start + i)); } return true; }
    if !values_fit(heap, order.len * 2) || !nodes_fit(heap, order.len) { memory(heap); return false; }
    for i in order {
        let e = form_collection(heap, node_kind::Vector, run(heap, n.start + i * 2, 2), Node { kind: node_kind::Vector, length: vector_flavor::MapEntry });
        if heap.fault != Fault.None { return false; }
        out.push(e);
    }
    return true;
}
// A new sequence (node_kind::Concat) of `items`, values of this heap.
fn new_sequence(heap: Heap&, items: const Value[:]) -> Value {
    if !values_fit(heap, items.len) || !nodes_fit(heap, 1) { memory(heap); return Value.Undefined; }
    if !charge(heap, items.len) { return Value.Undefined; }
    let start = heap.values.len;
    let result = allocate(heap, Node { kind: node_kind::Concat, start: start as u32, count: items.len as u32 });
    if heap.fault != Fault.None { return Value.Undefined; }
    add_values(heap, items);
    return result;
}
// `(cons x coll)` of a seqable coll (seq_error) (clj.rs cons): a new list
// of x and coll's seq.
fn clj_cons(heap: Heap&, x: Value, coll: Value) -> Value {
    let c = form_value(heap, coll); let kind = form_kind(heap, c);
    if heap.fault != Fault.None { return Value.Undefined; }
    var items: Value[>..] = [];
    items.push(x);
    // Onto nil, a list (the V8 build's List, a stack); onto a list or a
    // sequence, a cons whose rest is it; onto another collection, a cons
    // whose rest is its seq (list_flavor::Cons).
    if c.tag() <= value_tag::Null { return form_collection(heap, node_kind::List, items); }
    let tail = if kind == node_kind::List || kind == node_kind::Concat { coll } else { form_sequence(heap, c, 0, true) };
    if !seq_items(heap, tail, items) { return Value.Undefined; }
    return form_collection(heap, node_kind::List, items, Node { kind: node_kind::List, length: list_flavor::Cons, value: tail });
}
// `cljList`: every item of a seqable v's seq as a new list (clj.rs
// to_list), `()` for none.
fn seq_list(heap: Heap&, v: Value) -> Value {
    var items: Value[>..] = [];
    if !seq_items(heap, v, items) { return Value.Undefined; }
    return form_collection(heap, node_kind::List, items);
}
// `cljDrop` (form.rs drop_shared): `(drop n v)`, n at least 1, of a vector
// or a sequence, sharing its run: the sequence n items on, or `()`; nil for
// another value, which the caller walks (a realized range too, whose drop
// keeps its flavor there). A count below 1 or no number is a TypeError.
fn clj_drop(heap: Heap&, v: Value, count: Value) -> Value {
    let k = form_value(heap, count); let x = form_value(heap, v);
    if heap.fault != Fault.None { return Value.Undefined; }
    if !k.is_number() || !(k.num() >= 1) { heap.fault = Fault.TypeError; return Value.Undefined; }
    let kind = form_kind(heap, x);
    if kind != node_kind::Vector && kind != node_kind::Concat { return nil(); }
    let n = node(heap, x.id());
    if integer_range(n) { return nil(); }
    if k.num() >= n.count { return form_collection(heap, node_kind::List, []); }
    let skip = k.num() as i64;
    return allocate(heap, Node { kind: node_kind::Concat, start: (n.start + skip) as u32, count: (n.count - skip) as u32 });
}

// The errors of peek and pop (the V8 build's): nil, a list or a vector is
// a stack, and an empty one does not pop.
fn stack_error(heap: Heap&, op: i64, v: Value) -> CoreError {
    let x = form_value(heap, v); if x.tag() <= value_tag::Null { return CoreError.None; }
    let kind = form_kind(heap, x);
    let cons = kind == node_kind::List && node(heap, x.id()).length == list_flavor::Cons;
    if kind != node_kind::List && kind != node_kind::Vector || cons { return if op == form_op::CljPeek { CoreError.PeekType } else { CoreError.PopType }; }
    if op == form_op::CljPop && node(heap, x.id()).count == 0 { return if kind == node_kind::List { CoreError.PopEmptyList } else { CoreError.PopEmptyVector }; }
    return CoreError.None;
}
// The errors of peek and pop above treat a cons (list_flavor::Cons) as no
// stack.
// `peek` of a stack (stack_error) (clj.rs peek): a list's first item, a
// vector's last, or nil.
fn clj_peek(heap: Heap&, v: Value) -> Value {
    let x = form_value(heap, v); if x.tag() <= value_tag::Null { return nil(); }
    let n = node(heap, x.id());
    if n.count == 0 { return nil(); }
    return slot(heap, n.start + (if n.kind == node_kind::List { 0 } else { n.count - 1 }));
}
// `pop` of a nonempty stack (stack_error) (clj.rs pop): nil stays nil, a
// list is its rest, and a vector drops its last item, keeping its metadata
// and sharing its run.
fn clj_pop(heap: Heap&, v: Value) -> Value {
    let x = form_value(heap, v); if x.tag() <= value_tag::Null { return nil(); }
    let n = node(heap, x.id());
    if n.kind == node_kind::List { return form_sequence(heap, x, 1, false); }
    var made = n; made.origin = -1; made.count = n.count - 1; made.length = vector_flavor::Plain;
    return allocate(heap, made);
}

// The errors of cljs.core's dissoc and disj: nil, and a map (a set) takes
// any key.
fn removal_error(heap: Heap&, op: i64, v: Value) -> CoreError {
    let x = form_value(heap, v); if x.tag() <= value_tag::Null { return CoreError.None; }
    let kind = form_kind(heap, x);
    if op == form_op::CljDissoc { return if kind == node_kind::Map || record_value(heap, x) { CoreError.None } else { CoreError.DissocType }; }
    return if kind == node_kind::Set { CoreError.None } else { CoreError.DisjType };
}
// dissoc of a map's key (`set` false) or disj of a set's member (form.rs
// dissoc and disj): v without every entry whose key is equal (a literal
// keeps reader-distinct keys that are equal once read, and the V8 build's
// array map drops them all; it pads its count with undefined entries,
// which it cannot return, and this one does not), keeping its metadata,
// traversal order and the routes of the keys it keeps (source_vm.goose
// "Routes"), or v itself when none is; nil stays nil. Another value is a
// TypeError (the clj forms raise removal_error's error first).
fn form_remove(heap: Heap&, v: Value, k: Value, set: bool) -> Value {
    let x = form_value(heap, v); let kind = form_kind(heap, x);
    if heap.fault != Fault.None { return Value.Undefined; }
    if x.tag() <= value_tag::Null { return nil(); }
    if kind != (if set { node_kind::Set } else { node_kind::Map }) { heap.fault = Fault.TypeError; return Value.Undefined; }
    let n = node(heap, x.id()); let stride: i64 = if set { 1 } else { 2 };
    let routes = if hash_ordered(n) { route_start(heap, n) } else { -1 };
    var kept: Value[>..] = []; var kept_routes: Value[>..] = []; var routed = false;
    for i in (n.count / stride) {
        let key = slot(heap, n.start + i * stride);
        let equal = form_equal(heap, key, k);
        if heap.fault != Fault.None { return Value.Undefined; }
        if equal { continue; }
        if !valid(heap, key) { return Value.Undefined; } kept.push(key);
        if !set { let value = slot(heap, n.start + i * 2 + 1); if !valid(heap, value) { return Value.Undefined; } kept.push(value); }
        if routes >= 0 { let r = slot(heap, routes + i); routed = routed || r.is_number(); kept_routes.push(r); }
    }
    if kept.len == n.count { return v; }
    let route_rows = if routed { kept_routes.len } else { 0 };
    if !values_fit(heap, kept.len + route_rows) || !nodes_fit(heap, if routed { 2 } else { 1 }) { memory(heap); return Value.Undefined; }
    if !charge(heap, kept.len) { return Value.Undefined; }
    var made = n; made.origin = -1; made.value = Value.Undefined;
    if routed { made.value = form_collection(heap, node_kind::Vector, kept_routes); if heap.fault != Fault.None { return Value.Undefined; } }
    made.start = heap.values.len as u32; made.count = kept.len as u32;
    let result = allocate(heap, made); if heap.fault != Fault.None { return Value.Undefined; }
    add_values(heap, kept);
    return result;
}
// `formEmpty` (form.rs empty): an empty map or set of v's kind, traversal
// order and metadata (no routes); a TypeError for another value.
fn form_empty(heap: Heap&, v: Value) -> Value {
    let x = form_value(heap, v); let kind = form_kind(heap, x);
    if heap.fault != Fault.None { return Value.Undefined; }
    if kind != node_kind::Map && kind != node_kind::Set { heap.fault = Fault.TypeError; return Value.Undefined; }
    var made = node(heap, x.id()); made.origin = -1; made.value = Value.Undefined; made.start = heap.values.len as u32; made.count = 0;
    return allocate(heap, made);
}
// `formKeys` (form.rs entries): a new vector of a map's keys or a set's
// members, in traversal order; a TypeError for another value.
fn form_keys(heap: Heap&, v: Value) -> Value {
    let x = form_value(heap, v); let kind = form_kind(heap, x);
    if heap.fault != Fault.None { return Value.Undefined; }
    // A record's keys: its field keywords, then its extension keys.
    if record_value(heap, x) {
        var pairs: Value[>..] = [];
        if !record_pairs(heap, x, pairs) { return Value.Undefined; }
        var keys: Value[>..] = [];
        for i in (pairs.len / 2) { keys.push(pairs[i * 2]); }
        return form_collection(heap, node_kind::Vector, keys);
    }
    if kind != node_kind::Map && kind != node_kind::Set { heap.fault = Fault.TypeError; return Value.Undefined; }
    let n = node(heap, x.id()); let stride: i64 = if kind == node_kind::Map { 2 } else { 1 };
    var order: i64[>..] = [];
    if !traversal_order(heap, n, order) { if heap.fault == Fault.None { heap.fault = Fault.NativeBoundary; } return Value.Undefined; }
    var keys: Value[>..] = [];
    for i in order { keys.push(slot(heap, n.start + i * stride)); }
    return form_collection(heap, node_kind::Vector, keys);
}
// `cljWithExportedMeta`, with-meta in macro code (intrinsics.rs): v, a
// list, vector, map, set, symbol or sequence, with metadata m, nil or a
// map. Nil metadata on a collection that has none, or the very map it
// has, gives the collection itself (the V8 build's collectionWithMeta);
// otherwise a copy, a map entry's an ordinary vector. A function
// (the V8 build's MetaFn) fails by name; another value or metadata is a
// TypeError.
fn exported_meta(heap: Heap&, v: Value, m: Value) -> Value {
    let x = form_value(heap, v); let kind = form_kind(heap, x); let meta = form_value(heap, m);
    if heap.fault != Fault.None { return Value.Undefined; }
    if kind == node_kind::Closure || kind == node_kind::Builtin || kind == node_kind::BoundFunction { heap.fault = Fault.NativeBoundary; return Value.Undefined; }
    // A record's copy with that metadata (light-cljs-920).
    if record_value(heap, x) { return record_with_meta(heap, x, m); }
    if !(kind >= node_kind::List && kind <= node_kind::Symbol || kind == node_kind::Concat) || meta.tag() > value_tag::Null && form_kind(heap, meta) != node_kind::Map { heap.fault = Fault.TypeError; return Value.Undefined; }
    let n = node(heap, x.id());
    let none = meta.tag() <= value_tag::Null;
    let entry = kind == node_kind::Vector && n.length == vector_flavor::MapEntry;
    let same = if none { n.metadata.tag() <= value_tag::Null } else { n.metadata.is_object() && n.metadata.id() == meta.id() };
    if same && kind != node_kind::Symbol && !entry { return v; }
    var made = n; made.metadata = if none { Value.Undefined } else { m }; made.origin = -1;
    if kind == node_kind::Vector { made.length = vector_flavor::Plain; }
    return allocate(heap, made);
}

// `formHash` (form.rs hash): form_hash as a signed 32-bit number, the
// V8 build's; a value form_hash cannot hash here fails by name.
fn hash_value(heap: Heap&, v: Value) -> Value {
    let h, ok = form_hash(heap, v);
    if heap.fault != Fault.None { return Value.Undefined; }
    if !ok { heap.fault = Fault.NativeBoundary; return Value.Undefined; }
    return number(signed_word(h));
}
inline fn signed_word(h: u32) -> f64 { let w: i64 = h; let s: i64 = if w >= 2147483648 { w - 4294967296 } else { w }; return s; }
// `identityHash` (intrinsics.rs identity_hash): an image object's hash of
// its row, which no job moves, the same in every job; 0 for a primitive.
// A job's own object has no identity that outlives a collection inside
// the job (reclaim.goose moves its row) or reaches another job (a cell
// imports it at another row), so hashing one fails by name.
fn identity_hash(heap: Heap&, v: Value) -> Value {
    if !v.is_object() { return number(0); }
    let row: i64 = v.id();
    if row >= heap.image.nodes { heap.fault = Fault.NativeBoundary; return Value.Undefined; }
    return number(signed_word(hash_int(((row + 1) & 4294967295) as u32)));
}

// cljs.core's compare (the V8 build's compareKeys; form.rs compare_keys):
// -1, 0 or 1 and CoreError.None, or the CoreError of operands it cannot
// compare: nil first; numbers by value (NaN equal to every number);
// keywords with keywords and symbols with symbols by namespace, then name;
// vectors by count, then item by item; strings by code units; booleans
// false first. A JavaScript array pair (compared by text there) fails by
// name.
// The operands compare rejected, nested ones as the V8 build's recursive
// compareKeys throws for them.
struct Rejected { a: Value = Value.Undefined, b: Value = Value.Undefined }
recursive fn compare_values(heap: Heap&, a: Value, b: Value, r: Rejected&, depth: i64 = 0) -> i64, CoreError {
    if !charge(heap) { return 0, CoreError.None; }
    if depth >= 128 { heap.fault = Fault.StackLimit; return 0, CoreError.None; }
    let x = form_value(heap, a); let y = form_value(heap, b);
    if heap.fault != Fault.None { return 0, CoreError.None; }
    if same_value(heap, x, y) { return 0, CoreError.None; }
    if heap.fault != Fault.None { return 0, CoreError.None; }
    if x.tag() <= value_tag::Null { return -1, CoreError.None; }
    if y.tag() <= value_tag::Null { return 1, CoreError.None; }
    if x.is_number() {
        if !y.is_number() { r.a = x; r.b = y; return 0, CoreError.CompareValues; }
        return (if x.num() < y.num() { -1 } else if x.num() > y.num() { 1 } else { 0 }), CoreError.None;
    }
    let xk = form_kind(heap, x); let yk = form_kind(heap, y);
    // A Date with a Date by time value (a NaN one equal to any), as the V8
    // build's compareKeys; with anything else neither compares
    // (light-cljs-940).
    if xk == node_kind::Date {
        if yk != node_kind::Date { r.a = x; r.b = y; return 0, CoreError.CompareValues; }
        // Their valueOf calls are the intrinsic's only for Dates without own
        // properties or another prototype (intrinsic_date).
        if !intrinsic_date(node(heap, x.id())) || !intrinsic_date(node(heap, y.id())) { return 0, CoreError.Boundary; }
        let p = node(heap, x.id()).value.num(); let q = node(heap, y.id()).value.num();
        return (if p < q { -1 } else if p > q { 1 } else { 0 }), CoreError.None;
    }
    let xn = xk == node_kind::Keyword || xk == node_kind::Symbol; let yn = yk == node_kind::Keyword || yk == node_kind::Symbol;
    if xn || yn {
        if !xn || !yn || xk != yk { return 0, CoreError.CompareTypes; }
        let p = node(heap, x.id()); let q = node(heap, y.id());
        let c, e = compare_values(heap, p.namespace_value, q.namespace_value, r, depth + 1);
        if c != 0 || e != CoreError.None || heap.fault != Fault.None { return c, e; }
        return compare_values(heap, p.value, q.value, r, depth + 1);
    }
    if xk == node_kind::Vector && yk == node_kind::Vector {
        let p = node(heap, x.id()); let q = node(heap, y.id());
        if p.count != q.count { return (if p.count < q.count { -1 } else { 1 }), CoreError.None; }
        for i in p.count {
            let c, e = compare_values(heap, slot(heap, p.start + i), slot(heap, q.start + i), r, depth + 1);
            if c != 0 || e != CoreError.None || heap.fault != Fault.None { return c, e; }
        }
        return 0, CoreError.None;
    }
    if x.is_string() && y.is_string() {
        let p = str_len(heap, x); let q = str_len(heap, y);
        if !charge(heap, min(p, q)) { return 0, CoreError.None; }
        for i in min(p, q) { let s = unit(heap, x, i); let t = unit(heap, y, i); if s != t { return (if s < t { -1 } else { 1 }), CoreError.None; } }
        return (if p < q { -1 } else if p > q { 1 } else { 0 }), CoreError.None;
    }
    if x.is_bool() && y.is_bool() { return (if x.flag() == y.flag() { 0 } else if !x.flag() { -1 } else { 1 }), CoreError.None; }
    if xk == node_kind::Array && yk == node_kind::Array { return 0, CoreError.Boundary; }
    r.a = x; r.b = y;
    return 0, CoreError.CompareValues;
}
// The message of compare's CoreError.CompareValues: "Cannot compare a to
// b", each operand's str (the V8 build's displayStr).
fn compare_message(job: Job&, a: Value, b: Value) {
    let heap .= job.heap;
    let x = core_str(job, Builtin.Str, [a]);
    if heap.fault != Fault.None { return; }
    let y = core_str(job, Builtin.Str, [b]);
    if heap.fault != Fault.None { return; }
    job.detail.clear();
    for c in "Cannot compare " { job.detail.push(c); }
    append_utf16(heap, x, job.detail);
    for c in " to " { job.detail.push(c); }
    append_utf16(heap, y, job.detail);
    heap.fault = Fault.HostEffect;
}

// ---- Sealing (light-cljs-919; docs/javascript-coverage.md, decision 3,
// "The shared core's sharing vocabulary"). The shared-heap profile of the
// runtime (src/js_shared_profile.js) seals what its boot built so that the
// Pants engine can share it between tasks (rust/src/vm/seal.rs). Goose
// shares nothing between workers: a booted core is image rows that every job
// inherits and none may write (decision 3). So sealing here leaves the
// integrity levels the Pants engine's seal leaves, with its answers:
//   seal(v)           v, in place. An ordinary object or array the job
//                     allocated is frozen (integrity.goose set_integrity);
//                     so is a function with own properties made (a lazy
//                     closure, the Pants engine's function without a
//                     property record, stays as it is; see seal_one), and a
//                     concise method's home object (a static method's class, and so
//                     its `prototype`) is sealed in turn. A primitive, a
//                     form, a built-in function, a realm row and an object
//                     frozen already come back as they are. What a seal
//                     cannot take is a TypeError, which the boot's walk
//                     catches: an object with private elements or other
//                     internal slots (an error, wrapper, arguments object or
//                     iterator), a proxy, promise, generator or atom. A row
//                     the job did not allocate and that is not frozen faults
//                     NativeBoundary before anything changes.
//   sealBindings(v)   the number of binding cells sealed: every initialized
//                     binding_mode::Sealable cell the job allocated that v
//                     reaches becomes Sealed, immutable (an assignment is a
//                     TypeError; callable.goose's publication check takes
//                     it as it takes a fixed cell). Every other cell, mutable
//                     or the image's, is passed over.
//   sealReachable(v)  sealBindings, and of every closure or bound function v
//                     reaches whose own properties are made: the job's are
//                     frozen; then, frozen now or before, its untouched
//                     default `prototype` (an ordinary object of the job's
//                     whose every own property is a data `constructor`
//                     naming the function) is frozen. The number of cells,
//                     functions and prototypes sealed.
// The walk reaches what in-job reclamation reaches from v (reclaim.goose
// reach), through image rows too, and allocates no row; its marks, a
// scratch entry a row of every table, are the instruction's, and it charges
// a work unit an entry before it starts (WorkLimit, nothing sealed). It
// follows prototypes' `constructor` properties and [[Prototype]] links, so
// a default prototype a closure captures (a derived class captures its
// base's) brings its constructor along, and the Pants engine's extra pass
// over captured prototypes has nothing to add here.
// Publication is unchanged: a frozen object is a JavaScript object still,
// which a publication refuses (callable.goose, artifact.goose).

// The form operations that need the job, not only its heap: form_op's
// caller (the Form instruction) dispatches them here.
// formRegex compiles its pattern into the job's code (light-cljs-935;
// regexp_object.goose form_regex).
inline fn job_form(op: i64) -> bool { return op == form_op::Seal || op == form_op::SealBindings || op == form_op::SealReachable || op == form_op::StructSet || op == form_op::InitializerSlot || op == form_op::InitializerSlotGet || op == form_op::InitializerSlotSet || op == form_op::Regex; }
fn job_form_op(job: Job&, op: i64, v: Value) -> Value { let args: Value[1] = [v]; return job_form_op(job, op, args[..]); }
fn job_form_op(job: Job&, op: i64, args: const Value[:]) -> Value {
    // The struct field and initializer slot writes check the job's write
    // boundary (light-cljs-920, "Struct types" below).
    if op == form_op::StructSet { return struct_set(job, args); }
    if op == form_op::InitializerSlot || op == form_op::InitializerSlotGet || op == form_op::InitializerSlotSet { return initializer_slot(job, op, args); }
    if op == form_op::Regex { return form_regex(job, args[0], args[1]); }
    let v = args[0];
    if op == form_op::Seal { return seal_value(job, v); }
    if op == form_op::SealBindings || op == form_op::SealReachable {
        let n = seal_reachable(job, v, op == form_op::SealReachable);
        return if job.heap.fault == Fault.None { number(n) } else { Value.Undefined };
    }
    job.heap.fault = Fault.InvalidState; return Value.Undefined;
}
// Whether node row o is a realm intrinsic (realm.goose realm_intrinsic: not
// the global object, which seals like any object, as the Pants engine's
// does; a heap built without initialize has none).
inline fn realm_row(heap: const Heap&, o: i64) -> bool { return heap.global.is_object() && realm_intrinsic(o); }
// Whether a seal returns node o as it is: a form (a light-cljs runtime
// value, immutable already), a delay's cell, a built-in function or a realm
// intrinsic (the Pants engine's persistent values, built-ins and intrinsics).
fn seal_keeps(heap: const Heap&, o: i64) -> bool {
    if realm_row(heap, o) { return true; }
    let n = node(heap, o); let kind = n.kind;
    // A shareable regex value (light-cljs-935) is a form, immutable already.
    return kind >= node_kind::List && kind <= node_kind::Concat || kind == node_kind::LazySeq || kind == node_kind::Delay || kind == node_kind::Reduced || kind == node_kind::Builtin || kind == node_kind::Cell && n.length == cell_flavor::Delay || shared_regexp_node(n);
}
// What a seal cannot take, as the Pants engine's TypeError names it.
fn seal_refusal(heap: const Heap&, o: i64) -> const u8[:] {
    let kind = node(heap, o).kind;
    if kind == node_kind::Proxy { return "a Proxy"; }
    if kind == node_kind::Promise { return "a Promise"; }
    if kind == node_kind::Generator { return "a generator"; }
    if kind == node_kind::Atom || kind == node_kind::Cell { return "an atom"; }
    if kind == node_kind::Array { return "an array with internal slots"; }
    return "an object with internal slots";
}
// Seal one object, not its home object; false with a fault. A lazy closure
// (source_vm.goose lazy_function: no own property made yet, its `length` and
// `name` read from its code row) is the Pants engine's function without a
// property record and stays as it is. Every other function is frozen: once
// its properties exist, Goose cannot tell its creation's from the
// program's (a redefined `name`, a bound function's), and the Pants engine
// leaves only a record-less function unsealed until its publication makes
// it immutable anyway.
fn seal_one(job: Job&, v: Value) -> bool {
    let heap .= job.heap; let o: i64 = v.id(); let kind = node(heap, o).kind;
    if seal_keeps(heap, o) { return true; }
    let takes = kind == node_kind::Object || kind == node_kind::Array || kind == node_kind::Closure || kind == node_kind::BoundFunction;
    if !takes || private_row(heap, v) >= 0 {
        var message: u8[>..] = []; message.append("cannot seal "); message.append(seal_refusal(heap, o)); message.append(": only ordinary objects, arrays and functions are sealed");
        type_error(job, message[..]); return false;
    }
    if lazy_function(heap, o) { return true; }
    if test_integrity(job, Builtin.ObjectIsFrozen, v, true) { return true; }
    if heap.fault != Fault.None { return false; }
    return set_integrity(job, Builtin.ObjectFreeze, v, true);
}
// __rt.seal(v): v, sealed with its home objects (a method's object, a static
// method's class, the class's prototype: three at most).
fn seal_value(job: Job&, v: Value) -> Value {
    let heap .= job.heap;
    if !v.is_object() || !valid(heap, v) { return v; }
    var at = v;
    for step in 3 {
        if !seal_one(job, at) { return Value.Undefined; }
        let n = node(heap, at.id());
        if n.kind != node_kind::Closure || !n.value.is_object() { return v; }
        at = n.value;
    }
    heap.fault = Fault.InvalidState; return Value.Undefined;
}
// Whether object p is closure f's untouched default prototype for
// sealReachable: an ordinary object the job allocated, not frozen, with no
// private elements, whose every own property is a data `constructor`
// naming f (none at all counts, as in the Pants engine).
fn untouched_prototype(job: Job&, p: Value, f: i64) -> bool {
    let heap .= job.heap;
    if !p.is_object() || node(heap, p.id()).kind != node_kind::Object || realm_row(heap, p.id()) || !writable(job, p.id()) || private_row(heap, p) >= 0 { return false; }
    if test_integrity(job, Builtin.ObjectIsFrozen, p, true) || heap.fault != Fault.None { return false; }
    var keys: OwnKey[>..] = [];
    if !own_property_keys(heap, p, keys) { return false; }
    for k in keys {
        if !key_is(heap, k.key, "constructor") { return false; }
        let e = own_entry(heap, p, k.key);
        if heap.fault != Fault.None || !e.found || is_accessor(e.value) || !e.value.is_object() || e.value.id() != f { return false; }
    }
    return true;
}
// A function's own data property `prototype`, or undefined (find_key: no
// row is made, and a key never interned names no property).
fn own_prototype(heap: Heap&, f: i64) -> Value {
    let k = find_key(heap, "prototype");
    if k.is_undefined() { return Value.Undefined; }
    let e = own_entry(heap, object(f), k);
    return if heap.fault == Fault.None && e.found && !is_accessor(e.value) { e.value } else { Value.Undefined };
}
// __rt.sealBindings(v) and, `functions`, __rt.sealReachable(v): the number
// sealed (see above), with heap.fault set on a failure.
fn seal_reachable(job: Job&, v: Value, functions: bool) -> i64 {
    let heap .= job.heap;
    if !v.is_object() || !valid(heap, v) { return 0; }
    // The walk's scratch has a row a table row and it visits each row at
    // most once: its work, charged before it starts.
    let t = tops(heap);
    if !charge(heap, t.nodes + t.values + t.properties + t.strings + t.units + t.latin1 + t.accessors + t.conts) { return 0; }
    var e = Evacuation {};
    reach(e, heap, v);
    let rows = heap.nodes.len;
    var sealed: i64 = 0;
    for r in rows {
        if e.nodes[r] == 0 || realm_row(heap, r) { continue; }
        let n = node(heap, r);
        if n.kind == node_kind::Binding && n.length == binding_mode::Sealable && n.initialized && writable(job, r) {
            let c .= node_mut(heap, r); c.length = binding_mode::Sealed; c.immutable = true; sealed += 1;
        }
    }
    if !functions { return sealed; }
    for r in rows {
        if e.nodes[r] == 0 || realm_row(heap, r) { continue; }
        let kind = node(heap, r).kind;
        if kind != node_kind::Closure && kind != node_kind::BoundFunction || lazy_function(heap, r) { continue; }
        let f = object(r);
        var frozen = test_integrity(job, Builtin.ObjectIsFrozen, f, true);
        if heap.fault != Fault.None { return 0; }
        if !frozen && writable(job, r) && private_row(heap, f) < 0 {
            if !set_integrity(job, Builtin.ObjectFreeze, f, true) { return 0; }
            frozen = true; sealed += 1;
        }
        if !frozen || kind != node_kind::Closure { continue; }
        let p = own_prototype(heap, r);
        if heap.fault != Fault.None { return 0; }
        if untouched_prototype(job, p, r) {
            if !set_integrity(job, Builtin.ObjectFreeze, p, true) { return 0; }
            sealed += 1;
        }
        if heap.fault != Fault.None { return 0; }
    }
    return sealed;
}

// Struct types (light-cljs-920). The form operations behind deftype,
// defrecord and reify, with the Pants engine's semantics
// (collections/form.rs "Structs", vm/initializer.rs) and, where a record
// answers as a map, the V8 build's record methods (src/js_runtime.js
// recordMethodTable). A struct type (node_kind::StructType) is a type's
// identity: every structType call makes a new one, which a definition or
// helper publishes like any immutable value (callable.goose), so a type a
// unit's factory made in its preparation job is the type its calls use. A
// struct (node_kind::Struct) is an instance: its type, its field values in
// the type's order and, for a record, its metadata and extension map.
//
// A record is a map of its fields, under unqualified keywords of their
// names, then its extension entries: get, contains?, count, seq, keys,
// assoc (a field's value or an extension entry, a new record either way),
// dissoc (of a field, a map of the other entries with the record's
// metadata, no longer a record; of an extension key, a record without it),
// conj, with-meta, `=` (the same type, equal fields and extension maps),
// hash (each entry's key hash xor its value hash, summed, xor the Java
// string hash of the type's name) and printing (`#ns.R{:a 1}`). A keyed
// type (structType's `record` a field index, the Pants build's Var) is
// equal and hashes by that field. A deftype instance is equal only to
// itself, and the clj operations refuse it as the V8 build's core does
// (count not supported on this type, and so on).
//
// In JavaScript a struct is a frozen object: its fields are its own
// enumerable data properties under their member names (the last field of
// a member name wins, as a constructor's assignments do), a record has
// `__meta`, `__extmap` and `__hash` too, and its prototype is
// Object.prototype. A Mutable type's struct (mutableStructType) has
// writable fields, which structSet writes in place in the job that made it;
// a publication refuses it. An initializer slot is the Pants build's
// private compiler storage, never published.

// The type node of struct x (a Struct row) and its field count.
inline fn struct_type_node(heap: const Heap&, x: Value) -> Node { return node(heap, node(heap, x.id()).value.id()); }
inline fn type_fields(t: Node) -> i64 { return t.count / 2; }
// Whether v (through an occurrence) is a struct, and a record.
fn struct_value(heap: Heap&, v: Value) -> bool { let x = form_value(heap, v); return x.is_object() && node(heap, x.id()).kind == node_kind::Struct; }
fn record_value(heap: Heap&, v: Value) -> bool { return struct_value(heap, v) && struct_type_node(heap, form_value(heap, v)).length == type_flavor::Record; }
// The last field of type t whose member name is field i's (a constructor's
// last assignment of that member).
fn last_member(heap: Heap&, t: Node, i: i64) -> i64 {
    let fields = type_fields(t); let member = slot(heap, t.start + fields + i);
    var j = fields - 1;
    while j > i { if equal(heap, slot(heap, t.start + fields + j), member) { return j; } j -= 1; }
    return i;
}
// The strings of form list or vector v, appended to `out`; false with a
// TypeError for another value or an item that is no string.
fn struct_names(heap: Heap&, v: Value, out: Value[>..]&) -> bool {
    let x = form_value(heap, v); let kind = form_kind(heap, x);
    if heap.fault != Fault.None { return false; }
    if kind != node_kind::List && kind != node_kind::Vector && kind != node_kind::Concat { heap.fault = Fault.TypeError; return false; }
    let n = node(heap, x.id());
    if !charge(heap, n.count) { return false; }
    for i in n.count {
        let s = form_value(heap, slot(heap, n.start + i));
        if !s.is_string() { if heap.fault == Fault.None { heap.fault = Fault.TypeError; } return false; }
        out.push(s);
    }
    return true;
}
// structType(name, fields, record, members) and mutableStructType (form.rs
// new_type_with_mutability): a new type named `name` with the field names
// of `fields`, a record when `record` is true, keyed by field `record`
// when it is that field's index, and the fields' member names `members`
// (one per field; nil for the field names). A Mutable record is the Pants
// engine's unsupported operation; it fails by name.
fn struct_type(heap: Heap&, args: const Value[:], mutable: bool) -> Value {
    let name = form_value(heap, args[0]); let record = form_value(heap, args[2]); let members = form_value(heap, args[3]);
    if heap.fault != Fault.None { return Value.Undefined; }
    if mutable && record.is_bool() && record.flag() { heap.fault = Fault.NativeBoundary; return Value.Undefined; }
    if !name.is_string() { heap.fault = Fault.TypeError; return Value.Undefined; }
    var names: Value[>..] = [];
    if !struct_names(heap, args[1], names) { return Value.Undefined; }
    let fields = names.len;
    if members.tag() <= value_tag::Null { for i in fields { names.push(names[i]); } }
    else if !struct_names(heap, members, names) { return Value.Undefined; }
    if names.len != fields * 2 { heap.fault = Fault.TypeError; return Value.Undefined; }
    var key: i64 = 0;
    if record.is_number() {
        let k = record.num();
        if mutable || !(k >= 0 && k < fields && k == trunc(k)) { heap.fault = Fault.TypeError; return Value.Undefined; }
        key = (k as i64) + 1;
    }
    let flavor = if mutable { type_flavor::Mutable } else if record.is_bool() && record.flag() { type_flavor::Record } else { type_flavor::Deftype };
    if !values_fit(heap, names.len) || !nodes_fit(heap, 1) { memory(heap); return Value.Undefined; }
    let t = allocate(heap, Node { kind: node_kind::StructType, function: key as u32, start: heap.values.len as u32, count: names.len as u32, length: flavor as u32, immutable: true, value: name });
    if heap.fault != Fault.None { return Value.Undefined; }
    add_values(heap, names);
    return t;
}
// A record's metadata or extension map operand: none (Undefined) for nil,
// the map for a form map, a TypeError otherwise.
fn record_map(heap: Heap&, v: Value) -> Value {
    let x = form_value(heap, v);
    if heap.fault != Fault.None || x.tag() <= value_tag::Null { return Value.Undefined; }
    if form_kind(heap, x) != node_kind::Map { heap.fault = Fault.TypeError; return Value.Undefined; }
    return x;
}
// A struct of type `ty` (a StructType value) with the field values
// `fields` (one per field), metadata `meta` (Undefined: none) and
// extension map `ext` (nil, or a map, kept only when it is not empty).
fn make_struct(heap: Heap&, ty: Value, fields: const Value[:], meta: Value, ext: Value) -> Value {
    let t = node(heap, ty.id());
    let extension = if ext.is_object() && form_count(heap, ext) > 0 { ext } else { nil() };
    if heap.fault != Fault.None { return Value.Undefined; }
    if !values_fit(heap, fields.len) || !nodes_fit(heap, 1) { memory(heap); return Value.Undefined; }
    if !charge(heap, fields.len) { return Value.Undefined; }
    for v in fields { if !valid(heap, v) { return Value.Undefined; } }
    let s = allocate(heap, Node { kind: node_kind::Struct, start: heap.values.len as u32, count: fields.len as u32, immutable: t.length != type_flavor::Mutable, value: ty, namespace_value: extension, metadata: meta });
    if heap.fault != Fault.None { return Value.Undefined; }
    add_values(heap, fields);
    return s;
}
// structNew(type, v1, …) (NewStruct; form.rs new_struct): a struct of
// `type` with these field values in its order, a missing one undefined
// and extra ones ignored but a record's next two, its metadata and
// extension map (each a form map or nil). A Mutable type's field that a
// later field's member overwrites is undefined, as the constructor leaves
// it.
fn new_struct(heap: Heap&, args: const Value[:]) -> Value {
    if args.len == 0 { heap.fault = Fault.TypeError; return Value.Undefined; }
    let ty = form_value(heap, args[0]);
    if heap.fault != Fault.None { return Value.Undefined; }
    if form_kind(heap, ty) != node_kind::StructType { heap.fault = Fault.TypeError; return Value.Undefined; }
    let t = node(heap, ty.id()); let n = type_fields(t);
    if !charge(heap, n) { return Value.Undefined; }
    var fields: Value[>..] = [];
    for i in n { fields.push(if i + 1 < args.len { args[i + 1] } else { Value.Undefined }); }
    if t.length == type_flavor::Mutable { for i in n { if last_member(heap, t, i) != i { fields[i] = Value.Undefined; } } }
    var meta = Value.Undefined; var ext = nil();
    if t.length == type_flavor::Record {
        if n + 1 < args.len { meta = record_map(heap, args[n + 1]); }
        if n + 2 < args.len { let m = record_map(heap, args[n + 2]); if m.is_object() { ext = m; } }
        if heap.fault != Fault.None { return Value.Undefined; }
    }
    return make_struct(heap, ty, fields, meta, ext);
}
// The index of the field of type t that string `name` names, or -1.
fn field_named(heap: Heap&, t: Node, name: Value) -> i64 {
    let fields = type_fields(t);
    if !charge(heap, fields) { return -1; }
    for i in fields { if equal(heap, slot(heap, t.start + i), name) { return i; } }
    return -1;
}
// structField(s, name) (form.rs struct_field): the field named `name`, a
// Mutable type's by its member's last field; a TypeError for no struct, or
// a name that is no string or no field's.
fn struct_field(heap: Heap&, s: Value, name: Value) -> Value {
    let x = form_value(heap, s); let k = form_value(heap, name);
    if heap.fault != Fault.None { return Value.Undefined; }
    if !struct_value(heap, x) || !k.is_string() { heap.fault = Fault.TypeError; return Value.Undefined; }
    let t = struct_type_node(heap, x); let i = field_named(heap, t, k);
    if heap.fault != Fault.None { return Value.Undefined; }
    if i < 0 { heap.fault = Fault.TypeError; return Value.Undefined; }
    let at = if t.length == type_flavor::Mutable { last_member(heap, t, i) } else { i };
    return slot(heap, node(heap, x.id()).start + at);
}
// structState(v): 1 for a struct with writable fields, 0 for any other
// value. Goose publishes no such struct, so never the Pants engine's 2.
fn struct_state(heap: Heap&, v: Value) -> Value {
    let x = form_value(heap, v);
    return number(if struct_value(heap, x) && !node(heap, x.id()).immutable { 1.0 } else { 0.0 });
}
// structSet(v, name, value) (form.rs struct_set): store a Mutable type's
// field by its source name (its member's last field) and give the value. A
// TypeError for another value or name; a struct the job did not make is an
// image row, which no job writes (NativeBoundary).
fn struct_set(job: Job&, args: const Value[:]) -> Value {
    let heap .= job.heap;
    let x = form_value(heap, args[0]); let k = form_value(heap, args[1]);
    if heap.fault != Fault.None || !valid(heap, args[2]) { return Value.Undefined; }
    if !struct_value(heap, x) || node(heap, x.id()).immutable || !k.is_string() { heap.fault = Fault.TypeError; return Value.Undefined; }
    let t = struct_type_node(heap, x); let i = field_named(heap, t, k);
    if heap.fault != Fault.None { return Value.Undefined; }
    if i < 0 { heap.fault = Fault.TypeError; return Value.Undefined; }
    if !writable(job, x.id()) { boundary(job, "a write of a struct field the job did not make"); return Value.Undefined; }
    set_slot(heap, node(heap, x.id()).start + last_member(heap, t, i), args[2]);
    return args[2];
}
// structTypeOf(v): a struct's type, or nil.
fn struct_type_of(heap: Heap&, v: Value) -> Value { let x = form_value(heap, v); return if struct_value(heap, x) { node(heap, x.id()).value } else { nil() }; }
// The keyword `:name` of a record field (the runtime's keyword(null, name)).
fn field_keyword(heap: Heap&, name: Value) -> Value { return allocate(heap, Node { kind: node_kind::Keyword, value: name, namespace_value: nil() }); }
// The field of record type t that key k names (recordKeywordFieldIndex): an
// unqualified keyword whose name is the field's, else -1.
fn record_field(heap: Heap&, t: Node, k: Value) -> i64 {
    let x = form_value(heap, k);
    if form_kind(heap, x) != node_kind::Keyword || node(heap, x.id()).namespace_value.tag() > value_tag::Null { return -1; }
    return field_named(heap, t, node(heap, x.id()).value);
}
// recordFromMap(type, m) (`map->R`, form.rs record_from_map): a record of
// `type` whose fields are m's values for the field keywords (nil when
// absent) and whose extension entries are m's other entries; m is a form
// map, a record (its entries) or nil.
fn record_from_map(heap: Heap&, ty: Value, m: Value) -> Value {
    let t = form_value(heap, ty); let x = form_value(heap, m);
    if heap.fault != Fault.None { return Value.Undefined; }
    if form_kind(heap, t) != node_kind::StructType || node(heap, t.id()).length != type_flavor::Record { heap.fault = Fault.TypeError; return Value.Undefined; }
    var source = x;
    if x.tag() <= value_tag::Null { source = empty_literal(heap, node_kind::Map); }
    else if record_value(heap, x) { source = record_as_map(heap, x); }
    else if form_kind(heap, x) != node_kind::Map { heap.fault = Fault.TypeError; return Value.Undefined; }
    if heap.fault != Fault.None { return Value.Undefined; }
    let tn = node(heap, t.id()); var fields: Value[>..] = []; var ext = source;
    for i in type_fields(tn) {
        let key = field_keyword(heap, slot(heap, tn.start + i));
        if heap.fault != Fault.None { return Value.Undefined; }
        fields.push(form_get(heap, source, key, nil()));
        ext = form_remove(heap, ext, key, false);
        if heap.fault != Fault.None { return Value.Undefined; }
    }
    return make_struct(heap, t, fields, Value.Undefined, ext);
}
// A record's entries: its fields under their keywords, then its extension
// map's entries in traversal order, appended to `out` as key-value pairs.
fn record_pairs(heap: Heap&, x: Value, out: Value[>..]&) -> bool {
    let n = node(heap, x.id()); let t = struct_type_node(heap, x);
    if !charge(heap, n.count) { return false; }
    for i in type_fields(t) {
        let key = field_keyword(heap, slot(heap, t.start + i));
        if heap.fault != Fault.None { return false; }
        out.push(key); out.push(slot(heap, n.start + i));
    }
    if !n.namespace_value.is_object() { return true; }
    let e = node(heap, n.namespace_value.id());
    var order: i64[>..] = [];
    if !traversal_order(heap, e, order) { if heap.fault == Fault.None { heap.fault = Fault.NativeBoundary; } return false; }
    for i in order { out.push(slot(heap, e.start + i * 2)); out.push(slot(heap, e.start + i * 2 + 1)); }
    return true;
}
// A record's seq items: its entries as map entries.
fn record_entries(heap: Heap&, x: Value, out: Value[>..]&) -> bool {
    var pairs: Value[>..] = [];
    if !record_pairs(heap, x, pairs) { return false; }
    if !values_fit(heap, pairs.len) || !nodes_fit(heap, pairs.len / 2) { memory(heap); return false; }
    for i in (pairs.len / 2) {
        let e = form_collection(heap, node_kind::Vector, pairs[(i * 2)..(i * 2 + 2)], Node { kind: node_kind::Vector, length: vector_flavor::MapEntry });
        if heap.fault != Fault.None { return false; }
        out.push(e);
    }
    return true;
}
// `(into {} r)`: a map of record x's entries, each assoc'd in order (an
// array map up to 8 entries, then a hash map), without its metadata.
fn record_as_map(heap: Heap&, x: Value) -> Value {
    var pairs: Value[>..] = [];
    if !record_pairs(heap, x, pairs) { return Value.Undefined; }
    var out = empty_literal(heap, node_kind::Map);
    for i in (pairs.len / 2) { out = form_assoc(heap, out, pairs[i * 2], pairs[i * 2 + 1]); if heap.fault != Fault.None { return Value.Undefined; } }
    return out;
}
// A record's count: its fields and its extension entries.
fn record_count(heap: Heap&, x: Value) -> i64 {
    let n = node(heap, x.id());
    return n.count + (if n.namespace_value.is_object() { node(heap, n.namespace_value.id()).count / 2 } else { 0 });
}
// `(get r k fallback)`: a field by its keyword, else an extension entry.
fn record_get(heap: Heap&, x: Value, k: Value, fallback: Value) -> Value {
    let n = node(heap, x.id()); let i = record_field(heap, struct_type_node(heap, x), k);
    if heap.fault != Fault.None { return Value.Undefined; }
    if i >= 0 { return slot(heap, n.start + i); }
    let at = ext_index(heap, n.namespace_value, k);
    return if at >= 0 { slot(heap, node(heap, n.namespace_value.id()).start + at * 2 + 1) } else { fallback };
}
fn record_contains(heap: Heap&, x: Value, k: Value) -> bool {
    let n = node(heap, x.id()); let i = record_field(heap, struct_type_node(heap, x), k);
    if heap.fault != Fault.None { return false; }
    return i >= 0 || ext_index(heap, n.namespace_value, k) >= 0;
}
// The entry of record extension map ext (nil: none) whose key equals k,
// the first, or -1.
fn ext_index(heap: Heap&, ext: Value, k: Value) -> i64 {
    if !ext.is_object() { return -1; }
    let e = node(heap, ext.id());
    for i in (e.count / 2) { if form_equal(heap, slot(heap, e.start + i * 2), k) { return i; } if heap.fault != Fault.None { return -1; } }
    return -1;
}
// The type a copy of a record of type `ty` takes, the V8 build's
// recordCopy `Type.currentConstructor?.() ?? Type`: a type a declaration
// made (native_type; its node `initialized`) copies to the current
// definition of its name, which is the job's compiler root of that name
// when the job ran a declaration of it (realm.goose product_target), and
// otherwise ty itself, the definition the job reads. A root of that name
// that is no struct type is a TypeError, as `new` of it is.
fn copy_type(heap: Heap&, ty: Value) -> Value {
    if !node(heap, ty.id()).initialized || !heap.global.is_object() { return ty; }
    let roots, held = own_property(heap, heap.global, key_named(heap, "__lightCompilerRoots"));
    if heap.fault != Fault.None || !held || !roots.is_object() { return ty; }
    let current, found = own_property(heap, roots, node(heap, ty.id()).value);
    if heap.fault != Fault.None || !found { return ty; }
    let c = form_value(heap, current);
    if form_kind(heap, c) != node_kind::StructType { if heap.fault == Fault.None { heap.fault = Fault.TypeError; } return Value.Undefined; }
    return c;
}
// A copy of record x with field values `fields`, metadata `meta` and
// extension map `ext` (recordCopy): of its own type, or of its name's
// current definition, which takes them by position as `new Constructor(...
// fields, meta, ext)` does (new_struct).
fn record_copy(heap: Heap&, x: Value, fields: const Value[:], meta: Value, ext: Value) -> Value {
    let own = node(heap, x.id()).value; let ty = copy_type(heap, own);
    if heap.fault != Fault.None { return Value.Undefined; }
    if ty == own { return make_struct(heap, ty, fields, meta, ext); }
    var args: Value[>..] = [ty]; args.append(fields); args.push(if meta.is_object() { meta } else { nil() }); args.push(ext);
    return new_struct(heap, args);
}
// A record like x (its type and metadata) with field i set to v (i < 0:
// none) and extension map ext.
fn record_like(heap: Heap&, x: Value, i: i64, v: Value, ext: Value) -> Value {
    let n = node(heap, x.id());
    var fields: Value[>..] = []; fields.append(run(heap, n.start, n.count));
    if i >= 0 { fields[i] = v; }
    return record_copy(heap, x, fields, n.metadata, ext);
}
// `(assoc r k v)` (the runtime's IAssociative/-assoc): a new record with
// that field, or with that extension entry.
fn record_assoc(heap: Heap&, x: Value, k: Value, v: Value) -> Value {
    let n = node(heap, x.id()); let i = record_field(heap, struct_type_node(heap, x), k);
    if heap.fault != Fault.None { return Value.Undefined; }
    if i >= 0 { return record_like(heap, x, i, v, n.namespace_value); }
    let ext = if n.namespace_value.is_object() { form_assoc(heap, n.namespace_value, k, v) } else { form_collection(heap, node_kind::Map, [k, v]) };
    if heap.fault != Fault.None { return Value.Undefined; }
    return record_like(heap, x, -1, Value.Undefined, ext);
}
// `(dissoc r k)` (IMap/-dissoc): of a field, a map of the record's other
// entries with its metadata; of another key, a new record without that
// extension entry.
fn record_dissoc(heap: Heap&, x: Value, k: Value) -> Value {
    let n = node(heap, x.id()); let i = record_field(heap, struct_type_node(heap, x), k);
    if heap.fault != Fault.None { return Value.Undefined; }
    if i >= 0 {
        var m = record_as_map(heap, x);
        if heap.fault != Fault.None { return Value.Undefined; }
        if n.metadata.is_object() { var made = node(heap, m.id()); made.metadata = n.metadata; made.origin = -1; m = allocate(heap, made); }
        if heap.fault != Fault.None { return Value.Undefined; }
        return form_remove(heap, m, k, false);
    }
    let ext = if n.namespace_value.is_object() { form_remove(heap, n.namespace_value, k, false) } else { nil() };
    if heap.fault != Fault.None { return Value.Undefined; }
    return record_like(heap, x, -1, Value.Undefined, ext);
}
// `(conj r item)` (ICollection/-conj) of an item record_conj_error
// admitted: a vector's first two items assoc'd, a map's entries in turn,
// nil none.
fn record_conj(heap: Heap&, x: Value, item: Value) -> Value {
    let y = form_value(heap, item); let kind = form_kind(heap, y);
    if heap.fault != Fault.None { return Value.Undefined; }
    if kind == node_kind::Vector { let e = node(heap, y.id()); return record_assoc(heap, x, slot(heap, e.start), slot(heap, e.start + 1)); }
    if y.tag() <= value_tag::Null { return x; }
    var out = x; var items: Value[>..] = [];
    if !seq_items(heap, y, items) { return Value.Undefined; }
    for entry in items {
        let e = node(heap, form_value(heap, entry).id());
        out = record_assoc(heap, out, slot(heap, e.start), slot(heap, e.start + 1));
        if heap.fault != Fault.None { return Value.Undefined; }
    }
    return out;
}
// conj's error for a record: a vector of at least two items, a map or nil.
fn record_conj_error(heap: Heap&, item: Value) -> CoreError {
    let y = form_value(heap, item); let kind = form_kind(heap, y);
    if y.tag() <= value_tag::Null || kind == node_kind::Map { return CoreError.None; }
    if kind == node_kind::Vector { return if node(heap, y.id()).count < 2 { CoreError.OutOfBounds } else { CoreError.None }; }
    return CoreError.Boundary;
}
// `(with-meta r m)`: a copy with metadata m (nil: none).
fn record_with_meta(heap: Heap&, x: Value, m: Value) -> Value {
    let meta = record_map(heap, m);
    if heap.fault != Fault.None { return Value.Undefined; }
    let n = node(heap, x.id());
    var fields: Value[>..] = []; fields.append(run(heap, n.start, n.count));
    return record_copy(heap, x, fields, meta, n.namespace_value);
}
// `=` of two structs (form.rs equality): keyed ones by type name and key
// value; records by type identity, fields and extension maps; a deftype
// instance only to itself (decided by the caller).
recursive fn struct_equal(heap: Heap&, x: Value, y: Value, depth: i64) -> bool {
    let n = node(heap, x.id()); let m = node(heap, y.id());
    let t = node(heap, n.value.id()); let u = node(heap, m.value.id());
    if t.function != 0 && u.function != 0 {
        return t.function == u.function && equal(heap, t.value, u.value) && form_equal(heap, slot(heap, n.start + t.function - 1), slot(heap, m.start + u.function - 1), depth + 1);
    }
    if t.length != type_flavor::Record || !(n.value == m.value) || n.count != m.count { return false; }
    for i in n.count { if !form_equal(heap, slot(heap, n.start + i), slot(heap, m.start + i), depth + 1) { return false; } }
    return form_equal(heap, n.namespace_value, m.namespace_value, depth + 1);
}
// The hash of a record or keyed struct x (form.rs hash): false, with no
// fault, for a deftype instance (identity-hashed); `imported` is
// form_hash's.
recursive fn struct_hash(heap: Heap&, x: Value, depth: i64, imported: bool = false) -> u32, bool {
    let n = node(heap, x.id()); let t = struct_type_node(heap, x);
    if t.function != 0 { return form_hash(heap, slot(heap, n.start + t.function - 1), depth + 1, imported); }
    if t.length != type_flavor::Record { return 0, false; }
    // The sum takes the entries in any order: the fields, then the
    // extension map's run.
    var sum: u32 = 0;
    // Each name is charged before its hash scans it, as form_hash charges
    // a keyword's.
    for i in n.count {
        let name = slot(heap, t.start + i);
        if !charge(heap, str_len(heap, name)) { return 0, false; }
        let k = hash_named(heap, name, nil()) + 2654435769;
        let v, ok = form_hash(heap, slot(heap, n.start + i), depth + 1, imported); if !ok { return 0, false; }
        sum = sum + (k ^ v);
    }
    if n.namespace_value.is_object() {
        let e = node(heap, n.namespace_value.id());
        for i in (e.count / 2) {
            let k, kok = form_hash(heap, slot(heap, e.start + i * 2), depth + 1, imported); if !kok { return 0, false; }
            let v, vok = form_hash(heap, slot(heap, e.start + i * 2 + 1), depth + 1, imported); if !vok { return 0, false; }
            sum = sum + (k ^ v);
        }
    }
    if !charge(heap, str_len(heap, t.value)) { return 0, false; }
    return sum ^ string_hash(heap, t.value), true;
}
// A struct's JavaScript own property `k` (form.rs struct_own_property): a
// record's `__meta` (nil for none), `__extmap` or `__hash` (nil), or the
// last field of member name k; enumerable, writable only for a Mutable
// type's.
fn struct_member(heap: Heap&, o: i64, k: Value) -> Own {
    if !k.is_string() { return Own {}; }
    let n = node(heap, o); let t = node(heap, n.value.id()); let fields = type_fields(t);
    let attrs: u8 = if n.immutable { attr_enumerable() } else { attr_writable() | attr_enumerable() };
    if t.length == type_flavor::Record {
        if key_is(heap, k, "__meta") { return Own { found: true, value: if n.metadata.is_object() { n.metadata } else { nil() }, attrs: attrs }; }
        if key_is(heap, k, "__extmap") { return Own { found: true, value: n.namespace_value, attrs: attrs }; }
        if key_is(heap, k, "__hash") { return Own { found: true, value: nil(), attrs: attrs }; }
    }
    if !charge(heap, fields) { return Own {}; }
    var i = fields - 1;
    while i >= 0 { if equal(heap, slot(heap, t.start + fields + i), k) { return Own { found: true, value: slot(heap, n.start + i), attrs: attrs }; } i -= 1; }
    return Own {};
}
// A struct's own keys (form.rs struct_own_keys): each member name at its
// first field's position, then a record's three slots.
fn struct_keys(heap: Heap&, o: i64, out: OwnKey[>..]&) -> bool {
    let n = node(heap, o); let t = node(heap, n.value.id()); let fields = type_fields(t);
    let attrs: u8 = if n.immutable { attr_enumerable() } else { attr_writable() | attr_enumerable() };
    if !charge(heap, fields * fields + 3) { return false; }
    var names: Value[>..] = [];
    for i in fields {
        let member = slot(heap, t.start + fields + i);
        var seen = false; for m in names { if equal(heap, m, member) { seen = true; } }
        if !seen { names.push(member); }
    }
    if t.length == type_flavor::Record {
        for name in ["__meta", "__extmap", "__hash"] {
            let k = key_named(heap, name);
            var seen = false; for m in names { if equal(heap, m, k) { seen = true; } }
            if !seen { names.push(k); }
        }
    }
    for m in names { let k = intern(heap, m); if heap.fault != Fault.None { return false; } out.push(OwnKey { key: k, attrs: attrs }); }
    return heap.fault == Fault.None;
}
// The printed name of struct type t, its `/` a `.` (the runtime's
// `typeNames.get(Type).replaceAll("/", ".")`), appended to `out`.
fn type_print_name(heap: Heap&, t: Node, out: u16[>..]&) {
    for u in utf16(heap, t.value) { out.push(if u == 47 { 46 } else { u }); }
}
// initializerSlot(unit, name), initializerSlotGet(slot, field) and
// initializerSlotSet(slot, field, value) (vm/initializer.rs): a slot's
// unit, name, state (0 to 3), value and defined flag at fields 0 to 4; a
// set writes the state (a number 0 to 3), the value or the defined flag (a
// boolean) and gives the value. Another slot or field is a TypeError; a
// slot the job did not make is an image row, which no job writes.
fn initializer_slot(job: Job&, op: i64, args: const Value[:]) -> Value {
    let heap .= job.heap;
    if op == form_op::InitializerSlot {
        if !valid(heap, args[0]) || !valid(heap, args[1]) { return Value.Undefined; }
        if !values_fit(heap, 1) || !nodes_fit(heap, 1) { memory(heap); return Value.Undefined; }
        let made = allocate(heap, Node { kind: node_kind::InitializerSlot, start: heap.values.len as u32, count: 1, value: args[0], namespace_value: args[1] });
        if heap.fault != Fault.None { return Value.Undefined; }
        add_values(heap, [nil()]);
        return made;
    }
    let s = args[0]; let f = args[1];
    if !valid(heap, s) || !valid(heap, f) { return Value.Undefined; }
    if !s.is_object() || node(heap, s.id()).kind != node_kind::InitializerSlot || !f.is_number() || f.num() != trunc(f.num()) || f.num() < 0 || f.num() > 4 { heap.fault = Fault.TypeError; return Value.Undefined; }
    let field = f.num() as i64; let n = node(heap, s.id());
    if op == form_op::InitializerSlotGet {
        return match field { 0 => n.value, 1 => n.namespace_value, 2 => number(n.length), 3 => slot(heap, n.start), _ => boolean(n.initialized) };
    }
    let v = args[2];
    if !valid(heap, v) { return Value.Undefined; }
    if field < 2 || field == 2 && !(v.is_number() && v.num() >= 0 && v.num() <= 3 && v.num() == trunc(v.num())) || field == 4 && !v.is_bool() { heap.fault = Fault.TypeError; return Value.Undefined; }
    if !writable(job, s.id()) { boundary(job, "a write of an initializer slot the job did not make"); return Value.Undefined; }
    if field == 3 { set_slot(heap, n.start, v); return v; }
    let slot .= node_mut(heap, s.id());
    if field == 2 { slot.length = v.num() as u32; } else { slot.initialized = v.flag(); }
    return v;
}
// rt.nativeType(fields, specs, record, name, view, mutable, type, members)
// (light-cljs-920; src/emitter.rs native_def_type): what a native
// compile-time unit's deftype or defrecord runs, the Pants build's
// `decl.type`, which gives the type its methods and returns it. A type
// without methods is a new type like the unit's (its name, fields and
// members, the same run): each evaluation of a deftype or defrecord form
// defines a type of its own, as the V8 build's does, so records an
// earlier evaluation made are not equal to a later one's; the unit's type,
// which its factory made once, is the template. A type with methods needs the protocol
// tables light-cljs-922 brings, and a type whose instances are its view
// constructor's JavaScript objects (a record with mutable fields, a raw
// deftype body) the protocol resolver that answers for them; both fail by
// name.
fn native_type(job: Job&, b: Builtin, args: const Value[:]) -> Value {
    let heap .= job.heap;
    let specs = arg(args, 1); let ty = arg(args, 6);
    if !valid(heap, specs) || !valid(heap, ty) { return Value.Undefined; }
    if !specs.is_object() || node(heap, specs.id()).kind != node_kind::Array || form_kind(heap, ty) != node_kind::StructType { return unsupported_step(job, b, "operands the emitter does not make"); }
    if node(heap, specs.id()).length != 0 { return unsupported_step(job, b, "a type with protocol or Object methods (protocol tables, light-cljs-922)"); }
    if truthy(heap, arg(args, 5)) { return unsupported_step(job, b, "a type whose instances are JavaScript objects (a record with mutable fields or a raw body)"); }
    // Declared: a copy of its record takes its name's current definition
    // (copy_type), as the V8 build's record has a currentConstructor.
    var made = node(heap, ty.id()); made.initialized = true;
    return allocate(heap, made);
}
// globalThis.__lightCompilerProductTarget(name) (light-cljs-920): where a
// declaration a macro runs directly writes its products (a deftype's or
// defrecord's type and factories), the Pants build's `directProducts`.
// Goose's is the job's compiler roots object (source_vm.goose
// set_property): the job reads its products back as it reads any root, and
// the worker's reset drops them with the job's other roots, so a direct
// declaration is the job's own; a later job that reads the name asks the
// compiler, which has no definition of it and fails by name.
fn product_target(job: Job&, b: Builtin) -> Value {
    if !job.root_receiver.is_object() { return unsupported_step(job, b, "a job without compiler roots"); }
    return job.root_receiver;
}
