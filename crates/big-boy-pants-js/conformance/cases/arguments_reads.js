// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_functions.rs lines 482-532,
// arguments_object_holds_every_argument (bbp-97): reads of an unmapped
// arguments object: `length`, indexed reads, extra and missing arguments, an
// arrow's lexical `arguments`, a fresh object per call, defaults that read
// it, construct calls, @@iterator and for-of, and a ReferenceError for
// `arguments` at script level. The entry-function row (explicit undefined
// kept beside a default) is a script call here; the `__rt.reduce` callback
// row runs the callback in a loop. Row 5 (an element write) and rows 6 and
// 20 (descriptors, Object.keys counted with for-in) are
// arguments_writes_and_delete's (light-cljs-854). The `call`, `apply`,
// `Reflect.apply` and `bind` row is whole (light-cljs-852). Each row is a
// `row(...)` check; the script completes with the number of rows, or minus
// the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
{ function f() { return arguments.length; } row(f(1, 3) === 2); }
{ function f(a) { return arguments[1]; } row(f(1, 3) === 3); }
{ function f(a, b) { return arguments[0] === 1 && arguments.length === 1; } row(f(1)); }
{ function f() { return (() => arguments)(); } const a = f(1, 2); row(a.length === 2 && a[0] === 1 && a[1] === 2); }
{ function f() { return arguments; } row(f() !== f()); }
{ function f() { return () => () => arguments; } const a = f(4, 5)()(); row(a[1] === 5 && a.length === 2); }
{ function f() { function g() { return arguments; } return g(9)[0] === 9 && arguments[0] === 1; } row(f(1)); }
{ function f(a, b = arguments[0]) { return b === a && arguments.length === 1; } row(f(8)); }
{ function f(a, b = () => arguments) { return b; } const a = f(8)(); row(a[0] === 8 && a.length === 1); }
{ function f(a = 7) { return arguments.length === 0 && arguments[0] === undefined; } row(f()); }
{ function f() { return () => this.x + arguments[0]; } const g = f.call({x: 2}, 3); row(g.call({x: 100}) === 5); }
{ function f(a = () => () => arguments[1] + this.x) { return a; } row(f.call({x: 2}, undefined, 9)()() === 11); }
{ function f() { return arguments[1]; } row(f.call(undefined, 1, 3) === 3 && f.apply(undefined, [1, 4]) === 4 && Reflect.apply(f, undefined, [1, 5]) === 5 && f.bind(undefined, 1)(6) === 6); }
{ function F() { return arguments; } const a = new F(1, 2); row(a[1] === 2 && a.length === 2); }
{
  const callback = function (a, x) { return a + x + arguments.length; };
  let acc = 0;
  const items = [1, 2];
  for (let i = 0; i < items.length; i++) acc = callback(acc, items[i]);
  row(acc === 7);
}
{ function f() { return [...arguments]; } const a = f(1, 2); row(Array.isArray(a) && a.length === 2 && a[0] === 1 && a[1] === 2); }
{ function g(x, y, z) { return x + y + z; } function f() { return g(...arguments); } row(f(1, 2, 3) === 6); }
{ function f() { return arguments[Symbol.iterator] === Array.prototype.values; } row(f()); }
{ function f() { let s = 0; for (const x of arguments) s += x; return s; } row(f(1, 2, 3) === 6); }
{ function f() { return typeof arguments; } row(f() === "object"); }
// Explicit undefined stays in the object, even when its parameter defaults.
{
  function f(a, b = 2) { return arguments.length === 3 && b === 2 && arguments[1] === undefined && arguments[2] === 3; }
  row(f(1, undefined, 3));
}
// A script-level arrow has no `arguments`.
{ const g = () => arguments; row(name(g) === "ReferenceError"); }
failed === 0 ? rows : -failed;
