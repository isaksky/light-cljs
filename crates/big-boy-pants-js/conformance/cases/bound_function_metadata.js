// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_functions.rs lines 860-985:
// bound_functions (line 860, bbp-103) and
// bound_function_properties_and_native_callbacks (966, bbp-103), their
// metadata rows; QuickJS tests/test_builtin.js test_function (line 83), the
// first `bind` group's `length` and `name` (light-cljs-852). A bound
// function's own `length` (the target's own numeric `length` less the bound
// arguments, at least 0, else 0) and `name` ("bound " and the target's
// `name` when it is a string), both non-writable, non-enumerable and
// configurable; no `prototype`; its prototype the target's; `typeof` and
// Object.prototype.toString; bind's own `length` and `name`. Left out: rows
// that need Object.defineProperty or Object.getOwnPropertyNames (the
// objects area, light-cljs-766), among them a non-number `length` and a
// non-string `name` of the target, a getter for either, and Infinity;
// publication rows (Pants runtime tests). Each row is a `row(...)` check;
// the script completes with the number of rows, or minus the first failing
// row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
row(Function.prototype.bind.length === 1 && Function.prototype.bind.name === "bind");
{ function f(a, b) {} const g = f.bind(1, 2); row(g.length === 1 && g.name === "bound f" && g.bind(2).name === "bound bound f"); }
{ function f(a, b, c) {} row(f.bind(null, 1, 2, 3, 4).length === 0 && f.bind().length === 3 && f.bind(null).length === 3); }
{
  function f(a, b) {}
  const g = f.bind(null, 1, 2, 3);
  const d = Object.getOwnPropertyDescriptor(g, "length");
  const n = Object.getOwnPropertyDescriptor(g, "name");
  row(g.length === 0 && d.value === 0 && !d.writable && !d.enumerable && d.configurable && n.value === "bound f" && !n.writable && !n.enumerable && n.configurable);
}
// Assignment is a TypeError (strict code); delete then reveals
// Function.prototype's.
{ function f() {} const g = f.bind(); row(name(function () { g.name = "x"; }) === "TypeError" && g.name === "bound f" && delete g.name && g.name === ""); }
{ function f(a) {} const g = f.bind(); row(name(function () { g.length = 7; }) === "TypeError" && g.length === 1 && delete g.length && g.length === 0); }
// A target without its own `length` or `name`.
{ function f(a, b) {} delete f.length; delete f.name; const g = f.bind(null, 1); row(g.length === 0 && g.name === "bound "); }
{ function f() {} const g = f.bind(); delete g.name; row(g.bind().name === "bound " && g.bind().length === 0); }
{ const g = (function () {}).bind(); row(g.name === "bound " && g.bind().name === "bound bound "); }
// Built-in targets.
row(Object.is.bind(null).name === "bound is" && Object.is.bind(null, 1).length === 1 && Array.bind().name === "bound Array");
// Kind, prototype and keys.
{
  function f() {}
  const g = f.bind();
  row(typeof g === "function" && Object.getPrototypeOf(g) === Function.prototype && Object.prototype.toString.call(g) === "[object Function]");
  row(g.prototype === undefined && Object.getOwnPropertyDescriptor(g, "prototype") === undefined && !("prototype" in g) && "length" in g && "name" in g);
}
{ function f() {} const g = f.bind(); let keys = ""; for (const k in g) keys += k; g.x = 1; g.y = 2; let after = ""; for (const k in g) after += k; row(keys === "" && after === "xy" && g.x === 1); }
{ function f() {} const g = f.bind(); g.prototype = 3; row(g.prototype === 3 && f.prototype !== 3); }
{ function* gen() { yield 1; } row(Object.getPrototypeOf(gen.bind()) === Object.getPrototypeOf(gen)); }
failed === 0 ? rows : -failed;
