// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_functions.rs lines 1249-1304, 2275-2431:
// calls_evaluate_callee_then_arguments_and_bind_parameters (line 1249),
// call_failures_are_type_and_reference_errors_at_the_call (1287) and
// default_parameters (2275, bbp-100); js_source.rs
// function_entry_points_take_arguments_and_return (11395) as script calls,
// and the strict-compatible rows of rest_parameters_have_unmapped_arguments
// (58). Errors are compared by name (the Pants tests also check positions
// and messages). Left out: early errors (a body `let` repeating a parameter,
// "use strict" in a function with defaults: exporter SyntaxErrors, not
// execution cases); the Pants entry-function API row; `function f(a, g = ()
// => a) { a = 2; return g(); } f(1)`, which is 2 on the Pants engine and
// Goose and 1 on QuickJS (the body has no `var a`, so one binding is shared:
// 2, a QuickJS deviation); the sloppy rest rows (arguments_reads_sloppy, and
// the `arguments[1] = 9` write, which stops on Goose: light-cljs-764's
// arguments-object child). Each row is a `row(...)` check; the script
// completes with the number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// Order: callee, then arguments left to right, then the call.
{
  let log = 0;
  function note(d, v) { log = log * 10 + d; return v; }
  function add3(a, b, c) { return a + b + c; }
  row(note(1, add3)(note(2, 10), note(3, 20), note(4, 30)) * 10000 + log === 601234);
}
// Missing arguments are undefined; extra ones are evaluated and ignored.
{ function f(a, b) { return b; } row(f(1) === undefined); }
{ let n = 0; function f(a) { return a; } row(f(1, n = 5) + n === 6); }
// A function without `return` returns undefined.
{ function f() { 1; } row(f() === undefined); }
// Each evaluation of a function expression is a new object.
{ function mk() { return function () {}; } row(mk() !== mk()); }
{ function f() {} const g = f; row(g === f); }
// Parameters are locals of the call.
{ let x = 1; function f(x) { x = 9; return x; } row(f(x) + x === 10); }
// Call failures.
{ let x = 1; row(name(function () { x(2); }) === "TypeError"); }
{ let a = 1; row(name(function () { missing(a); }) === "ReferenceError"); }
// Unbounded recursion fails with QuickJS's InternalError.
{ function f(n) { return f(n + 1); } row(name(function () { f(0); }) === "InternalError"); }
// Entry-point style functions called from the script.
{
  function sum(n) {
    let acc = 0;
    while (n > 0) { acc = acc + n; n = n - 1; }
    return acc;
  }
  row(sum(10) === 55 && sum() === 0);
}
{
  function find(limit, step) {
    let i = 0;
    while (true) {
      if (i * step > limit) { return i; }
      i = i + 1;
    }
  }
  row(find(50, 7) === 8);
}
{ function f(a, b) { a = b; } row(f() === undefined); }
{ function f() { if (true) return; return 1; } row(f() === undefined); }
// A default runs when its argument is missing or `undefined`, and only then.
{
  function f(a, b = 2) { return b; }
  row(f(1) === 2 && f(1, undefined) === 2 && f(1, null) === null && f(1, 0) === 0);
}
// Defaults see earlier parameters and run left to right.
{ function f(a, b = a + 1) { return b; } row(f(1) === 2); }
{
  let log = 0;
  function f(a = (log = log * 10 + 1), b = (log = log * 10 + 2)) {}
  f(); f(5);
  row(log === 122);
}
// A later parameter, or the parameter itself, is in its temporal dead zone,
// only if the default runs.
{ function f(a = b, b) {} row(name(function () { f(); }) === "ReferenceError"); }
{ function f(a = a) {} row(name(function () { f(); }) === "ReferenceError"); }
{ function f(a = b, b) { return a; } row(f(1, 2) === 1); }
// Defaults see the function's own name and the enclosing scopes.
{ const g = function h(a = h) { return a; }; row(g() === g); }
{ function f(a = f) { return a; } row(f() === f); }
// ... but not the body's declarations.
{ const x = 1; function f(a = () => x) { let x = 2; return a(); } row(f() === 1); }
{ const x = 1; function f(a = () => x) { var x = 2; return a(); } row(f() === 1); }
// Closures in defaults capture the parameter bindings.
{ function f(g = () => b, b) { return g(); } row(f(undefined, 5) === 5); }
{ function f(g = () => b, b = g()) {} row(name(function () { f(); }) === "ReferenceError"); }
// A body `var` that repeats a parameter starts with its value; closures in
// defaults keep the parameter.
{
  function f(a = 1, g = () => a) {
    var a; const r = a; a = 3;
    return r * 100 + a * 10 + g();
  }
  row(f() === 131);
}
{ function f(g = () => b, b) { var x; return x; } row(f(undefined, 7) === undefined); }
// A body function declaration may repeat a parameter, and wins.
{ function f(a = 1) { function a() {} return typeof a === "function"; } row(f()); }
// The docs/grammar.md example.
{
  const x = 1;
  function f(a, b = a + 1, g = () => x) { var x = 2; return b * 10 + g(); }
  row(f(1) === 21 && f(1, undefined) === 21 && f(1, 5) === 51 && ((a = 1, c) => a + c)(undefined, 2) === 3);
}
// Arrow functions, with parameters after a default.
row(((a, b = 1, c) => a + c)(1, 2, 3) === 4);
row(((a = 1) => a)() === 1);
{ const k = (a, b = () => a) => b; row(k(9)() === 9); }
{ function f(a, b = a * 2) { return b; } row(f(3) === 6); }
// Defaults see the original arguments, including through arrows.
{ function f(a, b = arguments[0]) { return b; } row(f(7) === 7); }
{ function f(a, b = () => arguments) { return b; } row(f(7)()[0] === 7); }
// A default sees its function's `this`.
{ let o = {v: 4, F: function (a = this) { return a.v; }}; row(o.F() === 4); }
{ function F(a = this) { return a; } row(F() === undefined); }
{ function F(a = this) { this.d = a; } const o = new F(); row(o.d === o); }
{ let F = (a = this) => a; row(F() === globalThis); }
// An object literal default with an escaped reserved-word key.
{ const f = (a = {\u0069f: 1, b: 2}) => a.if + a.b; row(f() === 3); }
// Rest parameters: an unmapped `arguments`, and a default that closes over
// the rest binding.
{
  function f(a, ...r) { a = 8; return arguments[0] === 1 && r[0] === 2 && r.length === 2 && arguments.length === 3; }
  row(f(1, 2, 3));
}
{ function f(a = () => r, ...r) { var r; return a() === r && r[0] === 2; } row(f(undefined, 2)); }
failed === 0 ? rows : -failed;
