// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 19290-19315,
// primitive_wrapper_constructors_convert_object_arguments, its
// rows without bind, apply, Reflect and the light-cljs runtime, then
// Number, String and Boolean called on every primitive kind but BigInt (the
// BigInt area's, light-cljs-771) and on objects (light-cljs-832; QuickJS's
// js_number_constructor, js_string_constructor and js_boolean_constructor):
// Number is ToNumeric, String ToString except that a symbol argument gives
// its descriptive string, Boolean ToBoolean, and an object argument of Number
// or String runs its ToPrimitive (hint number or string). Each row is a
// `row(...)` check; the script completes with the number of rows, or minus
// the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function thrown(f) { try { f(); } catch (e) { return typeof e === 'object' ? e.name : e; } return 'none'; }
// Number on primitives.
row(Number(undefined) !== Number(undefined) && Number(null) === 0 && Number(true) === 1 && Number(false) === 0);
row(Number('  12  ') === 12 && Number('') === 0 && Number('1e3') === 1000 && Number('0b101') === 5 && Number('abc') !== Number('abc'));
row(1 / Number(-0) === -Infinity && Number(1.5) === 1.5 && Number('-Infinity') === -Infinity);
row(thrown(() => Number(Symbol.iterator)) === 'TypeError');
// String on primitives.
row(String(undefined) === 'undefined' && String(true) === 'true' && String(false) === 'false' && String(-0) === '0' && String(1.5) === '1.5' && String('x') === 'x');
row(String(Symbol.iterator) === 'Symbol(Symbol.iterator)' && String(Symbol.toPrimitive) === 'Symbol(Symbol.toPrimitive)' && String() === '');
row(thrown(() => new String(Symbol.iterator)) === 'TypeError' && thrown(() => '' + Symbol.iterator) === 'TypeError');
// Boolean on primitives.
row(Boolean('') === false && Boolean('0') === true && Boolean(NaN) === false && Boolean(-0) === false && Boolean(Symbol.iterator) === true && Boolean([]) === true);
// Objects: ToPrimitive, hint number for Number and string for String.
row(Number({valueOf: function () { return 7; }}) === 7);
row(String({toString: function () { return 's'; }}) === 's');
{ let n = new Number({valueOf: function () { return 7; }}); row(typeof n === 'object' && Object.getPrototypeOf(n) === Number.prototype && n.valueOf() === 7 && n.toString() === '7' && n !== 7); }
{ let s = new String({toString: function () { return 's'; }}); row(typeof s === 'object' && Object.getPrototypeOf(s) === String.prototype && s.valueOf() === 's' && s.toString() === 's' && s !== 's'); }
row(Number(new String('0x10')) === 16 && String(new Number(1e21)) === '1e+21' && Number(new Number(5)) === 5 && String(new String('q')) === 'q' && Boolean(new Boolean(false)) === true);
{ let o = {valueOf: function () { return 3; }, toString: function () { return 's'; }}; row(Number(o) === 3 && String(o) === 's'); }
row(Number({valueOf: function () { return {}; }, toString: function () { return '4'; }}) === 4);
row(String({toString: function () { return {}; }, valueOf: function () { return 4; }}) === '4');
{ let log = ''; let o = {valueOf: function () { log = log + 'v'; return 3; }}; row(Number(o, log = log + 'a') === 3 && log === 'av'); }
{ let o = {valueOf: function () { return 3; }}; row(Number.call(null, o) === 3); }
{ let o = {toString: function () { return 's'; }}; row(String.call(null, o) === 's'); }
{ let o = {valueOf: function () { return Number({valueOf: function () { return 6; }}); }}; row(Number(o) === 6); }
row(thrown(() => Number({valueOf: function () { throw 42; }})) === 42);
row(thrown(() => new String({toString: function () { return {}; }, valueOf: function () { return {}; }})) === 'TypeError');
row(String({}) === '[object Object]' && Number({}) !== Number({}));
// A symbol from ToPrimitive is not described: ToString and ToNumber throw.
row(thrown(() => String({toString: function () { return Symbol.iterator; }})) === 'TypeError' && thrown(() => Number({valueOf: function () { return Symbol.iterator; }})) === 'TypeError');
{ const a = {}, b = {}; a[Symbol.toPrimitive] = function (h) { return h; }; b[Symbol.toPrimitive] = function (h) { return h === 'number' ? 1 : 2; }; row(String(a) === 'string' && Number(b) === 1 && new String(a).valueOf() === 'string'); }
// Boolean runs no conversion.
{ let n = 0; row(new Boolean({valueOf: function () { n++; return false; }}).valueOf() === true && Boolean({toString: function () { n++; return ''; }}) === true && n === 0); }
failed === 0 ? rows : -failed;
