// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_functions.rs lines 2535-2741:
// call_apply_and_reflect_apply (line 2535, bbp-102), the apply rows of
// sloppy_nullish_receivers_use_the_task_global_object (2699; sloppy, in
// function_apply_sloppy) and apply_null_lists_and_zero_lengths (2741);
// QuickJS tests/test_builtin.js test_function, its first three groups;
// js_source.rs accessors_in_native_operations (7201), its getter-list `apply` row
// (light-cljs-851). Function.prototype.call and apply: receivers, argument
// lists from arrays, holes, array-like objects, getters and a `length`
// object, null and odd lengths, `call` and `apply` through each other and
// from native frames (a map or forEach callback, a ToPrimitive method),
// built-in targets, the TypeErrors, and a list past every limit (a
// RangeError above 65,535 arguments on Goose and QuickJS; the Pants engine
// counts its value limit, so a 70,000-element list is not a row).
// Left out: rows with Object.create, `bind` (light-cljs-852) or the Pants
// runtime (`__rt.reduce`, budgets and collection); the forwarding cycle
// through an array holding itself (QuickJS ends it with InternalError, the
// Pants engine with an uncatchable failure). Each row is a `row(...)`
// check; the script completes with the number of rows, or minus the first
// failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
{ function my_func(a, b) { return a + b; } row(my_func.call(null, 1, 2) === 3 && my_func.apply(null, [1, 2]) === 3); }
row((function () { return 1; }).apply(null, undefined) === 1);
// `this` of a strict callee is the receiver passed, unchanged.
{ function f() { return this; } row(f.call(7) === 7 && f.apply(8) === 8 && f.call() === undefined); }
{ function f(a, b) { return this + a + b; } row(f.apply(1, [2, 3]) === 6); }
// Arguments from an array, an array-like object and `call`'s rest; extra
// ones are ignored, missing ones undefined, holes undefined.
{
  function g(a, b, c) { return this * 1000 + a * 100 + b * 10 + c; }
  row(g.call(1, 2, 3, 4, 5) === 1234);
  row(g.apply(1, [2, 3, 4]) === 1234);
  row(g.apply(1, {length: 3, 0: 2, 1: 3, 2: 4, 3: 9}) === 1234);
  row(g.call.call(g, 1, 2, 3, 4) === 1234 && g.apply.call(g, 1, [2, 3, 4]) === 1234);
}
{ function f() { return arguments.length; } row(f.apply(null, new Array(5)) === 5 && f.apply(null, [1, , 3]) === 3); }
{ function f(a, b, c) { return b === undefined && c === undefined && arguments.length === 2; } row(f.apply(null, [1, , ])); }
{ function f() { return arguments[1]; } function g() { return f.apply(null, arguments); } row(g(1, 2, 3) === 2); }
// A null list, and lengths that are missing, negative, NaN or not numbers.
{
  function none(a) { return a === undefined; }
  row(none.apply(null, null) && none.apply(null, {0: 9}) && none.apply(null, {length: -1, 0: 9}));
  row(none.apply(null, {length: NaN, 0: 9}) && none.apply(null, {length: 'x', 0: 9}) && none.apply(null, {length: undefined, 0: 9}));
}
// Getters run in order, length first; a `length` object converts.
{
  let order = '';
  let args = {get length() { order += 'l'; return 2; }, get 0() { order += 'a'; return 3; }, get 1() { order += 'b'; return 4; }};
  let f = function (a, b) { return this.x + a + b; };
  row(f.apply({x: 2}, args) === 9 && order === 'lab');
}
{ function f(x, y) { return x + y; } row(f.apply(null, {length: {valueOf() { return 2; }}, 0: 1, 1: 2}) === 3); }
// Built-in targets.
row(Object.is.call(null, 1, 1) && !Object.is.apply(null, [1, 2]));
row(Object.prototype.toString.call([]) === '[object Array]');
row(Number.apply(null, [{valueOf() { return 7; }}]) === 7 && String.call(null, {toString() { return 'x'; }}) === 'x');
row(Array.apply(null, [1, 2, 3]).length === 3);
// call and apply from native frames.
{ function f(a) { return this * 10 + a; } row([1, 2].map(f.call, f).join() === '10,21'); }
{
  function f(a, b) { return this + a + b; }
  const r = [];
  [[1, [2, 3]], [4, [5, 6]]].forEach(function (p) { r.push(Function.prototype.apply.apply(f, p)); });
  row(r.join() === '6,15');
}
{ function f(a, b) { return [this, a, b].join(); } row(f.call.apply(f, [7, 1, 2]) === '7,1,2'); }
{ const o = {valueOf: Function.prototype.call}; row(name(function () { return o + 1; }) === 'TypeError'); }
{ const ap = Function.prototype.apply; function f(a) { return a + this; } row(ap.call(f, 1, [2]) === 3); }
// Function.prototype: callable, with `call` and `apply`.
{
  function h() {}
  row(Function.prototype() === undefined && typeof Function.prototype === 'function' && h.call === Function.prototype.call && Object.is.apply === h.apply);
  row(Function.prototype.call.length === 1 && Function.prototype.apply.length === 2 && Function.prototype.call.name === 'call' && Function.prototype.apply.name === 'apply');
}
// A callee reached through call or apply runs in its own frame.
{
  function loop(n) { let s = 0; for (let i = 0; i < n; i++) { s = s + this.k + i; } return s; }
  const o = {k: 1};
  row(loop.call(o, 40) + loop.apply(o, [10]) * 10000 === 820 + 550000);
}
// Failures: catchable TypeErrors, and too many arguments.
{
  function f() {}
  row(name(function () { f.apply(null, 1); }) === 'TypeError');
  row(name(function () { Function.prototype.call.call(1); }) === 'TypeError');
  row(name(function () { Function.prototype.apply.call({}, null, []); }) === 'TypeError');
  row(name(function () { const c = f.call; c(1); }) === 'TypeError');
  row(name(function () { new f.apply(); }) === 'TypeError');
  row(name(function () { f.apply(null, {length: 1100000}); }) !== 'none');
}
failed === 0 ? rows : -failed;
