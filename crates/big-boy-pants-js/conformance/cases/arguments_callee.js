// bbp-authored conformance case.
// Derives from: ~/src/quickjs/quickjs.c lines 59937-59949 (the unmapped
// arguments shape, revision 7d1c13c) and light-cljs-854: the `callee` of an
// unmapped arguments object (CreateUnmappedArgumentsObject, ECMAScript 10.4.4.6 step 8; QuickJS
// js_build_arguments) in strict functions: an accessor whose getter and
// setter are %ThrowTypeError% (light-cljs-853), neither enumerable nor
// configurable. QuickJS is the oracle; the Pants engine defines no
// `callee`. Each row is a `row(...)` check; the script completes with the
// number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
{ function f() { return arguments.callee; } row(name(f) === "TypeError"); }
{ function f() { arguments.callee = f; } row(name(f) === "TypeError"); }
{ function f() { return delete arguments.callee; } row(name(f) === "TypeError"); }
{
  function f() { return Object.getOwnPropertyDescriptor(arguments, "callee"); }
  const d = f(), caller = Object.getOwnPropertyDescriptor(Function.prototype, "caller");
  row(typeof d.get === "function" && d.get === d.set && d.get === caller.get && !d.enumerable && !d.configurable && !("value" in d));
}
{ function f() { var s = ""; for (var k in arguments) s += k; return s; } row(f(1) === "0"); }
{ function f() { return "callee" in arguments && arguments.hasOwnProperty("callee"); } row(f()); }
// Arrows read their function's object.
{ function f() { return () => arguments.callee; } row(name(f()) === "TypeError"); }
// A @@hasInstance hook on an arguments object runs for instanceof (the
// Pants engine refuses an arguments right operand).
{ function f() { return arguments; } const b = f(2); b[Symbol.hasInstance] = function (v) { return this === b && v === 5; }; row((5 instanceof b) === true && (6 instanceof b) === false); }
failed === 0 ? rows : -failed;
