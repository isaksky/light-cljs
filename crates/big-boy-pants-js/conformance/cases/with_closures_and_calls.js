// bbp-authored conformance case (sloppy script).
// Derives from: rust/tests/js_source.rs lines 16224-16267
// with_statement_resolution (the closure, call and strict-function rows) and
// light-cljs-855. A closure made in a `with` body keeps the object
// environment and reads through it later; a call of a name found in the
// object passes the object as `this` (ECMAScript 9.1.1.2.10 WithBaseObject),
// through parentheses and spread arguments; a strict function made in the
// body still resolves through the object, and its GetBindingValue or
// SetMutableBinding throws ReferenceError when the property is gone by
// then. Left out: an optional call (`f?.()`) of a name in a `with` body,
// which QuickJS (7d1c13c) fails to compile in most positions ("inconsistent
// stack size"); Goose and the Pants engine agree on it. Each row is a
// `row(...)` check; the script completes with the number of rows, or minus
// the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
row((function () { var f, o = { x: 1 }; with (o) { f = () => x; } o.x = 9; return f() === 9; })());
row((function () { function g() { var o = { x: 5 }; with (o) { return function () { return x; }; } } var h = g(); return h() === 5; })());
row((function () { var o = { x: 1 }; function k() { with (o) { return () => { x = 4; return x; }; } } return k()() === 4 && o.x === 4; })());
// The closure sees properties added and removed after it was made.
row((function () { var x = "outer", f, o = {}; with (o) { f = function () { return x; }; } if (f() !== "outer") return false; o.x = "in"; return f() === "in"; })());
// A loop inside the body, its condition and step read through the object.
row((function () { var f, o = { x: 0, get step() { return 1; } }; with (o) { f = () => x; while (x < 8) x += step; } o = undefined; return f() === 8; })());
// Calls through the object pass it as `this`.
row((function () { var o = { x: 2, f() { return this.x; } }; with (o) { return f() === 2 && (f)() === 2; } })());
row((function () { var o = { x: 2, f() { return this.x; } }; with (o) { return f(...[]) === 2 && f(...[1, 2]) === 2; } })());
row((function () { var o = { f() { return this === o; } }; with (o) { return f(); } })());
row((function () { var o = { f() { "use strict"; return this; } }; with (o) { return f() === o; } })());
// A function found outside the object is called with an undefined `this`.
row((function () { function f() { "use strict"; return this; } with ({}) { return f() === undefined; } })());
// Strict functions made inside the body.
row((function () { var f, o = { x: 2 }; with (o) { f = function () { "use strict"; return x; }; } return f() === 2; })());
row((function () { var f, o = { x: 2 }; with (o) { f = function () { "use strict"; x = (delete o.x, 3); }; } return name(f) === "ReferenceError" && !("x" in o); })());
row((function () {
  var f, o = { x: 2, get [Symbol.unscopables]() { delete this.x; return {}; } };
  with (o) { f = function () { "use strict"; return x; }; }
  return name(f) === "ReferenceError";
})());
row((function () { var f, o = { x: 2 }; with (o) { f = function () { "use strict"; x = 5; }; } f(); return o.x === 5; })());
failed === 0 ? rows : -failed;
