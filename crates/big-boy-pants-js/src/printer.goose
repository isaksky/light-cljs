import std;
import .source_vm;
import .realm;

namespace source;

// cljs.core's str, pr-str and identical?, and ExceptionInfo.prototype's
// toString (light-cljs-876): the V8 build's native macro runtime
// (src/bbp_runtime.js `str`, `prStr`, printWriterImpl, `identical` and
// ExceptionInfo.prototype.toString) as plain Goose built-ins (decision 5).
// No step of theirs runs JavaScript: where the V8 build's would (a
// `toString`, `valueOf` or @@toPrimitive that is JavaScript code, a getter
// on an error's fields, a lazy sequence's callbacks), they fail by name
// (unsupported_step), never with a value JavaScript could take for the
// real one.
//
// The printer writes UTF-16 units into one buffer and makes one string at
// the end. It walks collections with an explicit stack of PrintFrames, not
// host recursion, so nesting costs a frame row; at Limits.frames rows it
// stops with the catchable StackLimit (the V8 build's printer overflows
// its stack the same way on a cycle through an atom or an error's fields).
// Every printed value is charged one unit of work, and the string the
// units become is charged to the unit limit (string).
//
// What it prints, as printWriterImpl does with the runtime's defaults
// (*print-readably* true, *print-length*, *print-level* and
// *print-namespace-maps* off, *print-meta* false; a Goose job cannot
// rebind them, since rt.core is an image row):
//   nil and undefined `nil`; booleans; numbers as Number::toString, but
//   ##NaN, ##Inf, ##-Inf and -0.0; strings quoted, escaping `"`, `\`, \b,
//   \f, \n, \r and \t; keywords and symbols by their names; lists, concats
//   and seqs `(...)`, vectors `[...]`, sets `#{...}`, maps `{k v, ...}`;
//   a job's atom or volatile `#object[cljs.core.Atom {:val v}]` (Volatile);
//   a job's delay `#object[cljs.core.Delay {:status :pending, :val nil}]`
//   (:ready and its value once forced); an ExceptionInfo through its
//   toString (`#error {:message m, :data d, :cause c}`, data and cause
//   only when truthy); another error `#object[Ctor Name: message]`; a
//   function `#object[name]`.
// Maps and sets print in their traversal order (map_order, light-cljs-877),
// as the V8 build's runtime prints its array maps and hash maps and sets:
// an insertion-ordered one in insertion order, a hash-ordered one in
// ClojureScript's hash order, its keys sorted by legacy_order_key of
// form_hash. A hash-ordered map or set whose
// keys hash by identity in the V8 build (an atom, a function, a JavaScript
// object) fails by name. A lazy sequence, a process-wide cell's value, a
// JavaScript array or plain object and a BigInt fail by name too; a
// Symbol prints as #object[Symbol(description)] (light-cljs-885).

// A PrintFrame's layout: what separates its items and closes it.
enum PrintKind { Seq, Map, Fields, Wrapped }
enum PrintClose { Paren, Bracket, Brace, ObjectEnd }
// A collection or record being printed: `count` items from `start`, in the
// heap's value table or (scratch) in the printer's scratch values; `at` is
// the next one. Fields: an ExceptionInfo's message, data and cause, an
// Empty one skipped. Wrapped: the one value inside `#object[...]`.
struct PrintFrame { kind: PrintKind, close: PrintClose, start: i64, count: i64, at: i64 = 0, scratch: bool = false } // The printer's state for one str, prStr or toString call.
root struct Printer { out: u16[>..] = [], frames: PrintFrame[>..] = [], scratch: Value[>..] = [] }

inline fn emit(p: Printer&, text: const u8[:]) { for c in text { p.out.push(c); } }
// Whether `n` more units fit the job's unit limit beside what is printed,
// charging them as work; otherwise MemoryLimit (or the work fault), before
// the buffer grows. Every copied string's units go through here (and every
// printed value through print_one's check), so a value shared many times
// cannot grow the buffer past the limits; printed charges nothing again.
fn room(job: Job&, p: const Printer&, n: i64) -> bool {
    let heap .= job.heap;
    if heap.fault != Fault.None { return false; }
    if !units_fit(heap, p.out.len + n) { memory(heap); return false; }
    return charge(heap, n);
}
fn emit_string(job: Job&, p: Printer&, s: Value) -> bool {
    if !room(job, p, str_len(job.heap, s)) { return false; }
    append_utf16(job.heap, s, p.out); return true;
}
// ToString of a number (Number::toString).
fn emit_number(p: Printer&, x: f64) { var digits: u8[>..] = []; number::append_decimal(digits, x); emit(p, digits); }
// A keyword's or symbol's name: `:` for a keyword, then `ns/` when it has a
// namespace, then the name.
fn emit_named(job: Job&, p: Printer&, x: Value) -> bool {
    let ns = node(job.heap, x.id()).namespace_value; let name = node(job.heap, x.id()).value;
    if node(job.heap, x.id()).kind == node_kind::Keyword { p.out.push(58); }
    if ns.is_string() { if !emit_string(job, p, ns) { return false; } p.out.push(47); }
    return !name.is_string() || emit_string(job, p, name);
}
// A string, readably: quoted, with the runtime's seven escapes.
fn emit_readable(heap: const Heap&, p: Printer&, s: Value) {
    p.out.push(34);
    let h = header(heap, s.sid());
    if h.wide { for u in wide_run(heap, h) { emit_unit(p, u); } } else { for c in narrow_run(heap, h) { emit_unit(p, c); } }
    p.out.push(34);
}
inline fn emit_unit(p: Printer&, u: u16) {
    let e: u8 = if u == 34 { '"' } else if u == 92 { '\\' } else if u == 8 { 'b' } else if u == 12 { 'f' } else if u == 10 { 'n' } else if u == 13 { 'r' } else if u == 9 { 't' } else { 0 };
    if e != 0 { p.out.push(92); p.out.push(e); } else { p.out.push(u); }
}
// Push a frame over a run of the heap's values (no rows are added).
fn print_open(job: Job&, p: Printer&, f: PrintFrame) -> bool {
    if p.frames.len >= job.heap.limits.frames { job.heap.fault = Fault.StackLimit; return false; }
    p.frames.push(f); return true;
}
// Push a frame over `values`, copied to the scratch values.
fn print_open_scratch(job: Job&, p: Printer&, kind: PrintKind, close: PrintClose, values: const Value[:]) -> bool {
    let f = PrintFrame { kind: kind, close: close, start: p.scratch.len, count: values.len, scratch: true };
    if !print_open(job, p, f) { return false; }
    p.scratch.append(values); return true;
}
// Whether x has ExceptionInfo.prototype on its prototype chain (`x
// instanceof ExceptionInfo`), or %Error.prototype% (`x instanceof Error`).
inline fn is_exception_info(job: Job&, x: Value) -> bool { return inherits(job, x, object(exception_info_prototype_row())); }
inline fn is_error(job: Job&, x: Value) -> bool { return inherits(job, x, object(error_prototype_row(0))); }
// cljs.core's truth of v (form_op CljTruth): false only for nil,
// undefined and false; 0, "" and NaN are true.
fn clj_truth(heap: Heap&, v: Value) -> bool { let x = form_value(heap, v); return x.tag() > value_tag::Null && !(x.is_bool() && !x.flag()); }
// `#error {:message ` and the Fields frame of ExceptionInfo.prototype's
// toString with `this` e: e.message, e.data and e.cause read as data
// properties (a getter fails by name), data and cause printed when truthy
// as cljs.core's truth has it (only nil, undefined and false are not).
fn print_error_fields(job: Job&, b: Builtin, p: Printer&, e: Value) -> bool {
    let heap .= job.heap;
    let m = data_get(job, b, e, "message"); if heap.fault != Fault.None { return false; }
    let d = data_get(job, b, e, "data"); if heap.fault != Fault.None { return false; }
    let c = data_get(job, b, e, "cause"); if heap.fault != Fault.None { return false; }
    emit(p, "#error {:message ");
    let fields: Value[3] = [m, if clj_truth(heap, d) { d } else { Value.Empty }, if clj_truth(heap, c) { c } else { Value.Empty }];
    return print_open_scratch(job, p, PrintKind.Fields, PrintClose.Brace, fields[..]);
}
// ToString of object o without running JavaScript (str, an ExceptionInfo
// whose toString is not the realm's, and the `String(x)` of an error's
// `#object[...]`), written to p: ToPrimitive with hint string, whose
// @@toPrimitive must be absent and whose `toString`, then `valueOf`, if
// callable, must be one of the built-ins below, run to completion; then
// ToString of the primitive. ExceptionInfo.prototype.toString pushes its
// Fields frame instead of making a string. A method that is JavaScript
// code, or another built-in, fails by name.
fn emit_object_string(job: Job&, b: Builtin, p: Printer&, o: Value) -> bool {
    let heap .= job.heap;
    let exotic, getter = lookup(heap, o, Value.Symbol { s: well_known::ToPrimitive });
    if heap.fault != Fault.None { return false; }
    if getter.is_object() || exotic.tag() > value_tag::Null { unsupported_step(job, b, "ToString of an object with @@toPrimitive"); return false; }
    let names: Name[2] = [Name { text: "toString" }, Name { text: "valueOf" }];
    for n in names {
        let f = data_get(job, b, o, n.text);
        if heap.fault != Fault.None { return false; }
        if !callable(heap, f) { continue; }
        if node(heap, f.id()).kind != node_kind::Builtin { unsupported_step(job, b, "ToString of an object whose method is JavaScript code"); return false; }
        let m = builtin_of(heap, f.id());
        let r = match m {
            ExceptionInfoPrototypeToString => { return print_error_fields(job, b, p, o); },
            ObjectPrototypeToString => object_tag(job, o),
            ErrorPrototypeToString => error_to_string(job, o),
            // A light-cljs runtime leaf is a JavaScript function in the V8
            // build, whose source text this VM does not have.
            FunctionPrototypeToString => { if o.is_object() && node(heap, o.id()).kind == node_kind::Builtin && runtime_leaf(builtin_of(heap, o.id())) { unsupported_step(job, b, "the source text of a light-cljs runtime function"); return false; } function_text(job, o) },
            ObjectPrototypeValueOf => to_object(job, o),
            NumberPrototypeToString => number_to_string(job, m, o, []),
            NumberPrototypeValueOf => this_primitive(job, m, 0, o),
            StringPrototypeToString, StringPrototypeValueOf => this_primitive(job, m, 1, o),
            BooleanPrototypeValueOf, BooleanPrototypeToString => this_primitive(job, m, 2, o),
            _ => { unsupported_step(job, b, "ToString of an object through this built-in"); return false; },
        };
        if heap.fault != Fault.None { return false; }
        let v = form_value(heap, r);
        if v.is_object() { continue; }
        let s = if m == Builtin.BooleanPrototypeToString { key_named(heap, if v.flag() { "true" } else { "false" }) } else { primitive_string(job, b, v) };
        if heap.fault != Fault.None { return false; }
        return emit_string(job, p, s);
    }
    type_error(job, "cannot convert object to primitive value");
    return false;
}
// Print one value: a scalar, a name or an object's text is written at
// once; a collection, record or wrapper pushes its frame, whose items the
// loop in print_all writes.
fn print_one(job: Job&, b: Builtin, p: Printer&, v: Value) -> bool {
    let heap .= job.heap;
    if !charge(heap) { return false; }
    if !units_fit(heap, p.out.len) { memory(heap); return false; }
    let x = form_value(heap, v);
    if heap.fault != Fault.None { return false; }
    match x {
        Undefined, Null => { emit(p, "nil"); return true; },
        Bool => { emit(p, if x.flag() { "true" } else { "false" }); return true; },
        Number => {
            let n = x.num();
            if n != n { emit(p, "##NaN"); } else if n > 1.7976931348623157e308 { emit(p, "##Inf"); } else if n < -1.7976931348623157e308 { emit(p, "##-Inf"); } else if n == 0 && 1.0 / n < 0 { emit(p, "-0.0"); } else { emit_number(p, n); }
            return true;
        },
        String => { if !room(job, p, 2 * str_len(heap, x) + 2) { return false; } emit_readable(heap, p, x); return true; },
        Object => {},
        // A symbol prints as the V8 build's runtime prints one,
        // #object[Symbol(description)] (light-cljs-885; js_runtime.js printWriter).
        Symbol => {
            var units: u16[>..] = []; for c in "#object[" { units.push(c); } append_descriptive(heap, x, units); units.push(93);
            if !room(job, p, units.len) { return false; }
            p.out.append(units[..]); return true;
        },
        _ => { unsupported_step(job, b, "printing a BigInt"); return false; },
    }
    let n .= node(heap, x.id());
    let kind = n.kind;
    if kind == node_kind::Keyword || kind == node_kind::Symbol { return emit_named(job, p, x); }
    if kind == node_kind::List || kind == node_kind::Concat { emit(p, "("); return print_open(job, p, PrintFrame { kind: PrintKind.Seq, close: PrintClose.Paren, start: n.start, count: n.count }); }
    if kind == node_kind::Vector { emit(p, "["); return print_open(job, p, PrintFrame { kind: PrintKind.Seq, close: PrintClose.Bracket, start: n.start, count: n.count }); }
    if kind == node_kind::Map || kind == node_kind::Set {
        let map = kind == node_kind::Map;
        let stride: i64 = if map { 2 } else { 1 };
        let entries = n.count / stride;
        let frame_kind = if map { PrintKind.Map } else { PrintKind.Seq };
        if !hash_ordered(n) || entries < 2 {
            emit(p, if map { "{" } else { "#{" });
            return print_open(job, p, PrintFrame { kind: frame_kind, close: PrintClose.Brace, start: n.start, count: n.count });
        }
        // A hash-ordered map or set: its entries in traversal_order.
        let start: i64 = n.start;
        var order: i64[>..] = [];
        if !traversal_order(heap, n, order) {
            if heap.fault == Fault.None { unsupported_step(job, b, if map { "the hash order of a map keyed by a JavaScript object" } else { "the hash order of a set holding a JavaScript object" }); }
            return false;
        }
        var items: Value[>..] = [];
        for i in order { items.push(slot(heap, start + i * stride)); if map { items.push(slot(heap, start + i * stride + 1)); } }
        emit(p, if map { "{" } else { "#{" });
        return print_open_scratch(job, p, frame_kind, PrintClose.Brace, items);
    }
    if kind == node_kind::Atom {
        emit(p, if n.length == 0 { "#object[cljs.core.Atom {:val " } else { "#object[cljs.core.Volatile {:val " });
        let inside: Value[1] = [n.value];
        return print_open_scratch(job, p, PrintKind.Wrapped, PrintClose.ObjectEnd, inside[..]);
    }
    if kind == node_kind::Delay {
        emit(p, if truthy(heap, n.value) { "#object[cljs.core.Delay {:status :pending, :val " } else { "#object[cljs.core.Delay {:status :ready, :val " });
        let inside: Value[1] = [n.namespace_value];
        return print_open_scratch(job, p, PrintKind.Wrapped, PrintClose.ObjectEnd, inside[..]);
    }
    if kind == node_kind::Cell { unsupported_step(job, b, "printing a process-wide atom, volatile or delay"); return false; }
    if kind == node_kind::LazySeq { unsupported_step(job, b, "printing a lazy sequence"); return false; }
    if kind == node_kind::Closure || kind == node_kind::Builtin || kind == node_kind::BoundFunction {
        // `#object[name]`, `Function` for a falsy name (*print-fn-bodies* off).
        let name = form_value(heap, data_get(job, b, x, "name"));
        if heap.fault != Fault.None { return false; }
        emit(p, "#object[");
        if name.is_string() && str_len(heap, name) > 0 { if !emit_string(job, p, name) { return false; } }
        else if !truthy(heap, name) { emit(p, "Function"); }
        else { unsupported_step(job, b, "printing a function whose name is no string"); return false; }
        emit(p, "]"); return true;
    }
    // `x.toString()`: the realm's ExceptionInfo.prototype.toString is the
    // printer itself (emit_object_string).
    if is_exception_info(job, x) {
        let f = data_get(job, b, x, "toString");
        if heap.fault != Fault.None { return false; }
        if !callable(heap, f) { type_error(job, "not a function"); return false; }
        if node(heap, f.id()).kind != node_kind::Builtin { unsupported_step(job, b, "printing an ExceptionInfo whose toString is JavaScript code"); return false; }
        return emit_object_string(job, b, p, x);
    }
    if heap.fault != Fault.None { return false; }
    if is_error(job, x) {
        // `#object[` + (x.constructor?.name || 'Object') + ' ' + String(x) +
        // ']': the name is read first. The closing bracket is a frame of no
        // items, so it follows whatever String(x) pushes.
        let c = data_get(job, b, x, "constructor");
        if heap.fault != Fault.None { return false; }
        var name = Value.Undefined;
        if c.tag() > value_tag::Null {
            if !c.is_object() { unsupported_step(job, b, "printing an error whose constructor is a primitive"); return false; }
            name = form_value(heap, data_get(job, b, c, "name"));
            if heap.fault != Fault.None { return false; }
        }
        emit(p, "#object[");
        if !truthy(heap, name) { emit(p, "Object"); }
        else if name.is_string() { if !emit_string(job, p, name) { return false; } }
        else { let t = primitive_string(job, b, name); if heap.fault != Fault.None || !emit_string(job, p, t) { return false; } }
        emit(p, " ");
        if !print_open(job, p, PrintFrame { kind: PrintKind.Seq, close: PrintClose.Bracket, start: 0, count: 0 }) { return false; }
        return emit_object_string(job, b, p, x);
    }
    if heap.fault != Fault.None { return false; }
    unsupported_step(job, b, if kind == node_kind::Array { "printing a JavaScript array" } else { "printing a JavaScript object" });
    return false;
}
// Print v and everything it contains.
fn print_all(job: Job&, b: Builtin, p: Printer&, v: Value) -> bool {
    if !print_one(job, b, p, v) { return false; }
    return print_rest(job, b, p);
}
// Run the frames print_one pushed until none is left.
fn print_rest(job: Job&, b: Builtin, p: Printer&) -> bool {
    let heap .= job.heap;
    while p.frames.len > 0 {
        let top = p.frames.len - 1;
        let f = p.frames[top];
        if f.at >= f.count {
            emit(p, match f.close { Paren => ")", Bracket => "]", Brace => "}", ObjectEnd => "}]" });
            if f.scratch { p.scratch.resize(f.start); }
            p.frames.pop();
            continue;
        }
        let i = f.at; p.frames[top].at = i + 1;
        let x = if f.scratch { p.scratch[f.start + i] } else { slot(heap, f.start + i) };
        match f.kind {
            Seq => { if i > 0 { p.out.push(32); } },
            Map => { if i > 0 { emit(p, if i % 2 == 1 { " " } else { ", " }); } },
            Fields => { if x.is_hole() { continue; } if i == 1 { emit(p, ", :data "); } else if i == 2 { emit(p, ", :cause "); } },
            Wrapped => {},
        }
        if !print_one(job, b, p, x) { return false; }
    }
    return true;
}
// The printed units as a string (charged to the unit limit).
fn printed(job: Job&, p: const Printer&) -> Value { return import_text(job.heap, p.out); }

// rt.core.prStr(...values): each value printed readably, separated by a space.
fn pr_str(job: Job&, b: Builtin, args: const Value[:]) -> Value {
    var p = Printer {};
    for v, i in args {
        if i > 0 { p.out.push(32); }
        if !print_all(job, b, p, v) { return Value.Undefined; }
    }
    return printed(job, p);
}
// rt.core.str(...xs): the concatenation of each value's text: "" for nil
// and undefined, a string itself, ToString of a primitive, a keyword's or
// symbol's name, `[object Object]` for an atom, volatile or delay (a
// process-wide one too: no value is read), a collection printed readably,
// and an object's ToString (object_string).
fn core_str(job: Job&, b: Builtin, args: const Value[:]) -> Value {
    let heap .= job.heap;
    if args.len == 1 { let x = form_value(heap, args[0]); if x.is_string() { return x; } }
    var p = Printer {};
    for v in args {
        if !charge(heap) { return Value.Undefined; }
        let x = form_value(heap, v);
        if heap.fault != Fault.None { return Value.Undefined; }
        match x {
            Undefined, Null => { continue; },
            String => { if !emit_string(job, p, x) { return Value.Undefined; } continue; },
            Bool => { emit(p, if x.flag() { "true" } else { "false" }); continue; },
            Number => { emit_number(p, x.num()); continue; },
            Symbol => {
                // String(symbol) is its SymbolDescriptiveString, a
                // well-known symbol's or a job's (light-cljs-885).
                var units: u16[>..] = []; append_descriptive(heap, x, units);
                if !room(job, p, units.len) { return Value.Undefined; }
                p.out.append(units[..]); continue;
            },
            Object => {},
            _ => { return unsupported_step(job, b, "a BigInt argument"); },
        }
        let kind = node(heap, x.id()).kind;
        if kind == node_kind::Keyword || kind == node_kind::Symbol { if !emit_named(job, p, x) { return Value.Undefined; } continue; }
        if kind == node_kind::Atom || kind == node_kind::Cell || kind == node_kind::Delay { emit(p, "[object Object]"); continue; }
        if kind >= node_kind::List && kind <= node_kind::Set || kind == node_kind::Concat || kind == node_kind::LazySeq {
            if !print_all(job, b, p, x) { return Value.Undefined; }
            continue;
        }
        if !convertible(heap, x) { return unsupported_step(job, b, "ToString of a light-cljs runtime value"); }
        if !emit_object_string(job, b, p, x) || !print_rest(job, b, p) { return Value.Undefined; }
    }
    return printed(job, p);
}
// ExceptionInfo.prototype.toString with `this` receiver: `#error {:message
// ...}` (print_error_fields). Strict code: an undefined or null `this` is
// the TypeError of reading its `message`.
fn exception_info_to_string(job: Job&, b: Builtin, receiver: Value) -> Value {
    let heap .= job.heap;
    if receiver.tag() <= value_tag::Null || receiver.is_hole() { return type_error(job, "cannot read property 'message' of undefined"); }
    if !receiver.is_object() { return unsupported_step(job, b, "a primitive this value"); }
    var p = Printer {};
    if !print_error_fields(job, b, p, receiver) || !print_rest(job, b, p) { return Value.Undefined; }
    return printed(job, p);
}
// rt.core.identical(a, b): `val(a) === val(b)`, keyword literals and empty
// collection literals included (light-cljs-878: source_vm.goose
// same_keyword, realm.goose empty_form_row).
fn core_identical(job: Job&, args: const Value[:]) -> Value {
    let heap .= job.heap;
    let x = form_value(heap, arg(args, 0)); let y = form_value(heap, arg(args, 1));
    if heap.fault != Fault.None { return Value.Undefined; }
    return boolean(equal(heap, x, y));
}
