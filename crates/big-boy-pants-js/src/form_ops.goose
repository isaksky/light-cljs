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
        form_op::CljIsColl => kind >= node_kind::List && kind <= node_kind::Set || kind == node_kind::Concat || kind == node_kind::LazySeq,
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
    if op == form_op::CljDissoc { return if kind == node_kind::Map { CoreError.None } else { CoreError.DissocType }; }
    return if kind == node_kind::Set { CoreError.None } else { CoreError.DisjType };
}
// dissoc of a map's key (`set` false) or disj of a set's member (form.rs
// dissoc and disj): v without every entry whose key is equal (a literal
// keeps reader-distinct keys that are equal once read, and the V8 build's
// array map drops them all; it pads its count with undefined entries,
// which it cannot return, and this one does not), keeping its metadata and
// traversal order, or v itself when none is; nil stays nil. Another value
// is a TypeError (the clj forms raise removal_error's error first).
fn form_remove(heap: Heap&, v: Value, k: Value, set: bool) -> Value {
    let x = form_value(heap, v); let kind = form_kind(heap, x);
    if heap.fault != Fault.None { return Value.Undefined; }
    if x.tag() <= value_tag::Null { return nil(); }
    if kind != (if set { node_kind::Set } else { node_kind::Map }) { heap.fault = Fault.TypeError; return Value.Undefined; }
    let n = node(heap, x.id()); let stride: i64 = if set { 1 } else { 2 };
    var kept: Value[>..] = [];
    for i in (n.count / stride) {
        let key = slot(heap, n.start + i * stride);
        let equal = form_equal(heap, key, k);
        if heap.fault != Fault.None { return Value.Undefined; }
        if equal { continue; }
        if !valid(heap, key) { return Value.Undefined; } kept.push(key);
        if !set { let value = slot(heap, n.start + i * 2 + 1); if !valid(heap, value) { return Value.Undefined; } kept.push(value); }
    }
    if kept.len == n.count { return v; }
    if !values_fit(heap, kept.len) || !nodes_fit(heap, 1) { memory(heap); return Value.Undefined; }
    if !charge(heap, kept.len) { return Value.Undefined; }
    var made = n; made.origin = -1; made.start = heap.values.len as u32; made.count = kept.len as u32;
    let result = allocate(heap, made); if heap.fault != Fault.None { return Value.Undefined; }
    add_values(heap, kept);
    return result;
}
// `formEmpty` (form.rs empty): an empty map or set of v's kind, traversal
// order and metadata; a TypeError for another value.
fn form_empty(heap: Heap&, v: Value) -> Value {
    let x = form_value(heap, v); let kind = form_kind(heap, x);
    if heap.fault != Fault.None { return Value.Undefined; }
    if kind != node_kind::Map && kind != node_kind::Set { heap.fault = Fault.TypeError; return Value.Undefined; }
    var made = node(heap, x.id()); made.origin = -1; made.start = heap.values.len as u32; made.count = 0;
    return allocate(heap, made);
}
// `formKeys` (form.rs entries): a new vector of a map's keys or a set's
// members, in traversal order; a TypeError for another value.
fn form_keys(heap: Heap&, v: Value) -> Value {
    let x = form_value(heap, v); let kind = form_kind(heap, x);
    if heap.fault != Fault.None { return Value.Undefined; }
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
inline fn job_form(op: i64) -> bool { return op == form_op::Seal || op == form_op::SealBindings || op == form_op::SealReachable; }
fn job_form_op(job: Job&, op: i64, v: Value) -> Value {
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
    return kind >= node_kind::List && kind <= node_kind::Concat || kind == node_kind::LazySeq || kind == node_kind::Delay || kind == node_kind::Reduced || kind == node_kind::Builtin || kind == node_kind::Cell && n.length == cell_flavor::Delay;
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
