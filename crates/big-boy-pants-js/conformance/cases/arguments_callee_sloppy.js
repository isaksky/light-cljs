// bbp-authored conformance case (sloppy script).
// Derives from: ~/src/quickjs/quickjs.c lines 59951-59963 (the mapped
// arguments shape, revision 7d1c13c) and light-cljs-854: `callee` in sloppy
// code. A function with a simple parameter list gets a mapped arguments object (ECMAScript
// 10.4.4.7 CreateMappedArgumentsObject step 20; QuickJS
// js_build_mapped_arguments) whose `callee` is the function, a writable,
// configurable, non-enumerable data property; one with a default, rest or
// destructured parameter gets an unmapped object, whose `callee` is the
// %ThrowTypeError% accessor (light-cljs-853). QuickJS is the oracle; the
// Pants engine defines no `callee`. Left out: parameter aliasing of the
// mapped object (light-cljs-860), so no row writes a parameter or its
// element. Each row is a `row(...)` check; the script completes with the
// number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
row((function () { function f() { return arguments.callee === f; } return f(); })());
row((function () { function f(a, b) { return arguments.callee; } return f(1) === f; })());
row((function () { const g = function () { return arguments.callee; }; return g() === g; })());
row((function () { const o = { m(x) { return arguments.callee; } }; return o.m() === o.m; })());
row((function () {
  function f() { return Object.getOwnPropertyDescriptor(arguments, "callee"); }
  const d = f();
  return d.value === f && d.writable && !d.enumerable && d.configurable;
})());
row((function () { function f() { arguments.callee = 5; return arguments.callee; } return f() === 5; })());
row((function () { function f() { return delete arguments.callee && !("callee" in arguments); } return f(); })());
row((function () { function f() { var s = ""; for (var k in arguments) s += k; return s; } return f(1, 2) === "01"; })());
// Recursion through callee, the pre-ES5 idiom.
row((function () { const fact = function (n) { return n <= 1 ? 1 : n * arguments.callee(n - 1); }; return fact(5) === 120; })());
// An arrow reads its function's object.
row((function () { function f() { return (() => arguments.callee)(); } return f() === f; })());
// Non-simple parameter lists give the throwing accessor.
row((function () { function f(a = 1) { return arguments.callee; } return name(f) === "TypeError"; })());
row((function () { function f(...r) { return arguments.callee; } return name(f) === "TypeError"; })());
row((function () { function f({ a }) { return arguments.callee; } return name(() => f({})) === "TypeError"; })());
row((function () { function f(a, [b]) { return arguments.callee; } return name(() => f(1, [])) === "TypeError"; })());
row((function () {
  function f(a = 1) { return Object.getOwnPropertyDescriptor(arguments, "callee"); }
  const d = f();
  return typeof d.get === "function" && d.get === d.set && !d.enumerable && !d.configurable;
})());
// A "use strict" function in sloppy code is strict.
row((function () { function f(a) { "use strict"; return arguments.callee; } return name(f) === "TypeError"; })());
failed === 0 ? rows : -failed;
