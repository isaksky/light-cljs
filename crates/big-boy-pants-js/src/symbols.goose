import std;
import .source_vm;
import .realm;

namespace source;

// Symbol values (light-cljs-885; docs/javascript-coverage.md, decision 2;
// the Pants engine's value/symbol.rs and vm/builtin.rs, bbp-127 to bbp-129;
// QuickJS's js_symbol_constructor and js_symbol_for).
//
// A symbol a job makes is a row of the node table, node_kind::JsSymbol,
// named by a Symbol value whose payload is symbol_floor() plus the row
// (source_vm.goose symbol_at); the realm's well-known symbols keep their
// indices below the floor and take no rows. A symbol row holds its
// description and nothing JavaScript can write, so it needs no write
// check, and it lives and dies with the job's other rows: the worker reset
// truncates it, snapshots validate it, reclamation moves it and saved
// continuations carry it.
//
// The Symbol.for registry is a chain through the registered rows: each
// links the one registered before it, and the heap's state field
// Heap.registry names the newest. Symbol.for walks the chain comparing
// keys; nothing outside the rows indexes it (decision 3), so the reset
// restores it with the heap's other state fields and a job sees only the
// registrations of its own run (and of the image and a saved continuation
// it extends). A symbol never leaves its job: a publication or cell value
// that reaches one fails by name (cells.goose publish_cells and
// form_unshareable, callable.goose), so no identity has to be shared
// across jobs or workers.
//
// Symbol(description) and Symbol.for(key) run no JavaScript here: a call
// whose argument is an object converts it in a Coerce frame
// first, as String(o) does (their plan, coerce.goose), and a construction
// of Symbol is a TypeError before any conversion (construct).

// thisSymbolValue(receiver) (ECMAScript 20.4.3): a symbol, or a Symbol
// wrapper's symbol; anything else is a TypeError.
fn this_symbol(job: Job&, receiver: Value) -> Value {
    let heap .= job.heap;
    if receiver.is_symbol() { return receiver; }
    if receiver.is_object() && node(heap, receiver.id()).kind == node_kind::Wrapper && node(heap, receiver.id()).value.is_symbol() { return node(heap, receiver.id()).value; }
    return type_error(job, "not a symbol");
}
// A new symbol row with `description` (a string, or undefined when not
// registered); a registered one joins the registry's chain.
fn new_symbol(heap: Heap&, description: Value, registered: bool) -> Value {
    let o = allocate(heap, Node { kind: node_kind::JsSymbol, value: description, initialized: registered, namespace_value: if registered { heap.registry } else { Value.Null } });
    if !o.is_object() { return Value.Undefined; }
    let s = symbol_at(o.id());
    if registered { heap.registry = s; }
    return s;
}
// Symbol.for(key) of string key: the registered symbol whose key it is,
// or a new registered one. The walk is charged a step a symbol and the
// units it compares.
fn symbol_for(heap: Heap&, key: Value) -> Value {
    var at = heap.registry;
    while !at.is_null() {
        let n = node(heap, symbol_node(at));
        if !charge(heap, 1 + str_len(heap, key)) { return Value.Undefined; }
        if same_text(heap, n.value, key) { return at; }
        at = n.namespace_value;
    }
    return new_symbol(heap, key, true);
}
// Symbol, Symbol.for, Symbol.keyFor and Symbol.prototype's built-ins,
// called (call_builtin); none runs JavaScript.
fn symbol_builtin(job: Job&, b: Builtin, receiver: Value, args: const Value[:]) -> Value {
    let heap .= job.heap;
    match b {
        // Symbol([description]): undefined stays undefined, anything else
        // is its ToString (20.4.1.1).
        SymbolFunction => {
            let d = form_value(heap, arg(args, 0));
            if heap.fault != Fault.None { return Value.Undefined; }
            if d.is_undefined() { return new_symbol(heap, d, false); }
            let text = primitive_string(job, b, d);
            return if heap.fault == Fault.None { new_symbol(heap, text, false) } else { Value.Undefined };
        },
        // Symbol.for(key): ToString(key), undefined included (20.4.2.2).
        SymbolFor => {
            let k = form_value(heap, arg(args, 0));
            if heap.fault != Fault.None { return Value.Undefined; }
            let text = primitive_string(job, b, k);
            return if heap.fault == Fault.None { symbol_for(heap, text) } else { Value.Undefined };
        },
        // Symbol.keyFor(sym): a registered symbol's key, otherwise
        // undefined; a well-known symbol is not registered (20.4.2.6).
        SymbolKeyFor => {
            let s = arg(args, 0);
            if !s.is_symbol() { return type_error(job, "not a symbol"); }
            return if registered_symbol(heap, s) { node(heap, symbol_node(s)).value } else { Value.Undefined };
        },
        // SymbolDescriptiveString(thisSymbolValue(this)) (20.4.3.3).
        SymbolPrototypeToString => {
            let s = this_symbol(job, receiver);
            if heap.fault != Fault.None { return Value.Undefined; }
            var units: u16[>..] = []; append_descriptive(heap, s, units);
            if !units_fit(heap, units.len) { memory(heap); return Value.Undefined; }
            return string(heap, units);
        },
        // valueOf and @@toPrimitive: thisSymbolValue(this) (20.4.3.4, 20.4.3.5).
        SymbolPrototypeValueOf, SymbolPrototypeToPrimitive => { return this_symbol(job, receiver); },
        // get description (20.4.3.2): a symbol row's description, a
        // well-known symbol's "Symbol.name".
        SymbolPrototypeDescription => {
            let s = this_symbol(job, receiver);
            if heap.fault != Fault.None { return Value.Undefined; }
            if symbol_node(s) >= 0 { return node(heap, symbol_node(s)).value; }
            var units: u16[>..] = []; append_description(heap, s, units);
            return string(heap, units);
        },
        _ => { heap.fault = Fault.InvalidState; return Value.Undefined; },
    }
}
