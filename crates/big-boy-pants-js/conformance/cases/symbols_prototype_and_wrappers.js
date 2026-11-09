// bbp-authored conformance case (light-cljs-885).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 20534-20580 (symbol_prototype_and_wrappers, bbp-129) and 20582-20612 (symbol_primitive_property_writes):
// %Symbol.prototype% (toString, valueOf, the `description` getter,
// @@toPrimitive and @@toStringTag), property reads, method calls and
// writes on symbol primitives, and Symbol wrapper objects: Object(sym),
// ToObject, Object.prototype.toString, valueOf, ToPrimitive and the
// TypeErrors of converting one to a string or number. Left out: JSON.stringify
// (pending, light-cljs-767) and Array.prototype.sort (pending). Each row is
// a `row(...)` check; the script completes with the number of rows, or
// minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// The planner's rows (light-cljs-766).
row(Object(Symbol('w')) instanceof Symbol);
row(Symbol('d').description === "d");
{ let s = Symbol('z'); row(s[Symbol.toPrimitive]('default') === s); }
row(typeof Object.getOwnPropertyDescriptor(Symbol.prototype, 'description').get === "function");
// symbol_prototype_and_wrappers (js_source.rs line 20534).
{ let a = Symbol('abc'); row(String(a) === 'Symbol(abc)'); }
{ let a = Symbol('aaa'); row(a.valueOf() === a && a.toString() === 'Symbol(aaa)'); }
{ let a = Symbol('aaa'); let b = Object(a); row(b.valueOf() === a && b.toString() === 'Symbol(aaa)'); }
row(Symbol().toString() === 'Symbol()' && Symbol('').toString() === 'Symbol()' && Symbol('\uD800').toString() === 'Symbol(\uD800)');
row(Symbol('d').description === 'd' && Symbol().description === undefined && Symbol('').description === '');
{ let a = Symbol(); row(Object(a) !== Object(a) && typeof Object(a) === 'object' && Object(Object(a)).valueOf() === a); }
{ let a = Symbol('d'); let b = Object(a); row(b.description === 'd' && Object.getPrototypeOf(b) === Symbol.prototype && Object.getPrototypeOf(a) === Symbol.prototype && a.__proto__ === Symbol.prototype && Symbol.prototype.constructor === Symbol && Object.getPrototypeOf(Symbol.prototype) === Object.prototype); }
{ let a = Symbol(); let b = Object(a); row(a[Symbol.toPrimitive]('string') === a && b[Symbol.toPrimitive]('number') === a && b == a && a == b && b != Object(a)); }
{ let a = Symbol('d'); let b = Object(a); b.toString = function() { throw 'wrong toString'; }; b.valueOf = function() { throw 'wrong valueOf'; }; row(b == a && ({[b]: 7})[a] === 7); }
{ let d = Object.getOwnPropertyDescriptor(Symbol.prototype, 'description'); row(typeof d.get === 'function' && d.get.name === 'get description' && d.get.length === 0 && d.set === undefined && !d.enumerable && d.configurable); }
{ let d = Object.getOwnPropertyDescriptor(Symbol.prototype, Symbol.toPrimitive); row(!d.writable && !d.enumerable && d.configurable && d.value.name === '[Symbol.toPrimitive]' && d.value.length === 1); }
{ let d = Object.getOwnPropertyDescriptor(Symbol, 'prototype'); row(!d.writable && !d.enumerable && !d.configurable && Symbol.prototype.toString.length === 0 && Symbol.prototype.valueOf.length === 0); }
{ let s = Symbol('x'); row(Object.prototype.valueOf.call(s).valueOf() === s && Array.prototype.join.call(s) === ''); }
{ let b = Object(Symbol()); Object.defineProperty(b, Symbol.toPrimitive, {value: null}); b.toString = function() { return 'fallback'; }; row(String(b) === 'fallback'); }
{ let b = Object(Symbol()); let p = {[Symbol.toPrimitive](hint) { return hint; }}; Object.setPrototypeOf(b, p); row(String(b) === 'string' && b + '' === 'default'); }
{ let s = Symbol('x'); let b = Object(s); b.x = 7; Object.freeze(b); row(Object.isFrozen(b) && b.x === 7 && b.valueOf() === s && b instanceof Symbol); }
{ let b = Object(Symbol('x')); Object.setPrototypeOf(b, Object.prototype); row(Object.prototype.toString.call(b) === '[object Object]' && String(b) === '[object Object]'); }
{ let b = Object(Symbol()); Object.setPrototypeOf(b, null); row(Object.prototype.toString.call(b) === '[object Object]'); }
row(name(() => Symbol.prototype.valueOf.call(1)) === "TypeError");
row(name(() => Symbol.prototype.toString.call({})) === "TypeError");
row(name(() => Symbol.prototype.valueOf()) === "TypeError");
row(name(() => Symbol.prototype.description) === "TypeError");
row(name(() => Object.getOwnPropertyDescriptor(Symbol.prototype, 'description').get.call({})) === "TypeError");
row(name(() => Symbol.prototype[Symbol.toPrimitive].call('s')) === "TypeError");
row(name(() => Symbol.prototype.valueOf.call(Object.create(Symbol.prototype))) === "TypeError");
row(name(() => Symbol.prototype.valueOf.call(new Number(1))) === "TypeError");
row(name(() => String(Object(Symbol('x')))) === "TypeError");
row(name(() => '' + Object(Symbol('x'))) === "TypeError");
row(name(() => `${Object(Symbol())}`) === "TypeError");
row(name(() => Number(Object(Symbol()))) === "TypeError");
row(name(() => new String(Object(Symbol()))) === "TypeError");
row(name(() => Symbol.keyFor(Object(Symbol.for('x')))) === "TypeError");
row(name(() => { let b = Object(Symbol()); Object.defineProperty(b, Symbol.toPrimitive, {value: 1}); String(b); }) === "TypeError");
row(name(() => { let b = Object(Symbol()); Object.defineProperty(b, Symbol.toPrimitive, {value() { return {}; }}); String(b); }) === "TypeError");
// Object.prototype.toString: a symbol and its wrapper are "[object Symbol]"
// through %Symbol.prototype%[@@toStringTag].
row(Object.prototype.toString.call(Symbol()) === '[object Symbol]' && Object.prototype.toString.call(Object(Symbol.iterator)) === '[object Symbol]');
{ let d = Object.getOwnPropertyDescriptor(Symbol.prototype, Symbol.toStringTag); row(d.value === 'Symbol' && !d.writable && !d.enumerable && d.configurable); }
// Well-known symbols are symbols like any other.
row(Symbol.iterator.toString() === 'Symbol(Symbol.iterator)' && Object(Symbol.iterator).description === 'Symbol.iterator' && Symbol.iterator.valueOf() === Symbol.iterator);
// symbol_primitive_property_writes (js_source.rs line 20582), strict: the
// inherited __proto__ setter takes a primitive receiver and changes
// nothing; another write is a TypeError.
{ let s = Symbol(); s.__proto__ = null; row(Object.getPrototypeOf(s) === Symbol.prototype); }
{ let s = Symbol(); s['__proto__'] = {}; row(Object.getPrototypeOf(s) === Symbol.prototype); }
row(name(() => { let s = Symbol(); s.x = 1; }) === "TypeError");
row(name(() => { let s = Symbol(); s['x'] = 1; }) === "TypeError");
// A symbol is not iterable.
row(name(() => { for (let x of Symbol()) {} }) === "TypeError");
failed === 0 ? rows : -failed;
