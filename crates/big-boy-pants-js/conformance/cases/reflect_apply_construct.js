// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_functions.rs lines 2535-2741:
// the Reflect.apply rows of call_apply_and_reflect_apply (line 2535,
// bbp-102) and apply_null_lists_and_zero_lengths (2741); js_source.rs
// accessors_in_native_operations (7201), its getter-list Reflect.apply row;
// QuickJS tests/test_builtin.js test_function, its Reflect.apply group; the
// Reflect.construct steps of ECMAScript 28.1.2 checked against QuickJS's
// js_reflect_construct (light-cljs-851). Reflect is an object with
// @@toStringTag "Reflect"; Reflect.apply needs a callable target and an
// object list; Reflect.construct needs constructors, gives the new object
// newTarget's `prototype` (for ordinary functions and the realm's
// constructors alike) and runs the target with that new.target. Left out:
// rows with Object.create; the other eleven Reflect methods (the objects
// area, light-cljs-766). Each row is a `row(...)` check; the script
// completes with the number of rows, or minus the first failing row's
// number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
row(typeof Reflect === 'object' && Object.prototype.toString.call(Reflect) === '[object Reflect]');
row(Object.getPrototypeOf(Reflect) === Object.prototype && Reflect[Symbol.toStringTag] === 'Reflect');
row(Reflect.apply.length === 3 && Reflect.apply.name === 'apply' && Reflect.construct.length === 2 && Reflect.construct.name === 'construct');
// Reflect.apply.
{ const f2 = (a, b) => a + b; row(Reflect.apply(f2, null, [2, 3]) === 5); }
{ function f() { return this; } row(Reflect.apply(f, 9, []) === 9); }
{ function add(a, b) { return a + b; } row(Reflect.apply(add, null, [1, 2]) === 3); }
row(Reflect.apply(function (a, b, c) { return c === undefined && a + b === 5; }, 1, {length: 2.9, 0: 2, 1: 3, 2: 4}));
{ function g(a, b, c) { return this * 1000 + a * 100 + b * 10 + c; } row(Reflect.apply(Function.prototype.call, g, [1, 2, 3, 4]) === 1234); }
row(!Reflect.apply(Object.is, null, [1, 2]));
{
  function none(a) { return a === undefined; }
  row(Reflect.apply(none, null, {}) && Reflect.apply(none, null, {0: 9}) && Reflect.apply(none, null, {length: -1, 0: 9}));
  row(Reflect.apply(none, null, {length: NaN, 0: 9}) && Reflect.apply(none, null, {length: 'x', 0: 9}) && Reflect.apply(none, null, {length: undefined, 0: 9}));
}
{
  let args = {get length() { return {valueOf: () => 2}; }, get 0() { return 3; }, get 1() { return 4; }};
  row(Reflect.apply((a, b) => a + b, null, args) === 7);
}
{
  function loop(n) { let s = 0; for (let i = 0; i < n; i++) { s = s + this.k + i; } return s; }
  row(Reflect.apply(loop, {k: 1}, [5]) === 15);
}
row(name(function () { Reflect.apply(function () { return 1; }, null, undefined); }) === 'TypeError');
row(name(function () { Reflect.apply(function () {}, null, null); }) === 'TypeError');
row(name(function () { Reflect.apply(1, null, []); }) === 'TypeError');
row(name(function () { Reflect.apply(function () {}, null, 1); }) === 'TypeError');
// Reflect.construct.
{ function C(a) { this.a = a; } row(Reflect.construct(C, [4]).a === 4 && Reflect.construct(C, [4]) instanceof C); }
{ function F() { return new.target; } const o = Reflect.construct(function () { this.t = new.target; }, []); row(Reflect.construct(F, []) === F && typeof o.t === 'function'); }
{
  function C() { this.t = new.target; }
  function D() {}
  D.prototype.k = 3;
  const o = Reflect.construct(C, [], D);
  row(o.t === D && o.k === 3 && Object.getPrototypeOf(o) === D.prototype && !(o instanceof C));
}
{
  function D() {}
  D.prototype.k = 3;
  const e = Reflect.construct(Error, ['m'], D);
  row(e.k === 3 && e.message === 'm' && Object.getPrototypeOf(e) === D.prototype);
  row(Object.getPrototypeOf(Reflect.construct(Array, [3], D)) === D.prototype && Reflect.construct(Array, [3]).length === 3);
  row(Object.getPrototypeOf(Reflect.construct(Object, [], D)) === D.prototype && Reflect.construct(Number, [5], D).k === 3);
}
{ function D() {} D.prototype = 1; row(Object.getPrototypeOf(Reflect.construct(function () {}, [], D)) === Object.prototype); }
row(Reflect.construct(Number, [5]) instanceof Number && Reflect.construct(Object, [1]) instanceof Number);
row(Reflect.construct(Error, ['x']) instanceof Error && Reflect.construct(TypeError, []) instanceof TypeError);
{ function C(a, b) { this.s = a + b; } row(Reflect.construct(C, {length: 2, 0: 1, 1: 2}).s === 3); }
{
  let log = '';
  const n = Reflect.construct(Number, [{valueOf() { log += 'v'; return 7; }}]);
  const s = Reflect.construct(String, [{toString() { log += 's'; return 'x'; }}]);
  row(n instanceof Number && n.valueOf() === 7 && s instanceof String && s.valueOf() === 'x' && log === 'vs');
}
{
  const list = {length: 0};
  row(name(function () { Reflect.construct(Array.prototype.values, list); }) === 'TypeError');
  row(name(function () { Reflect.construct(function () {}, list, Array.prototype.values); }) === 'TypeError');
  row(name(function () { new Array.prototype.keys(); }) === 'TypeError');
  row(name(function () { new Symbol(); }) === 'TypeError');
}
row(name(function () { Reflect.construct(() => 1, []); }) === 'TypeError');
row(name(function () { Reflect.construct({}, []); }) === 'TypeError');
row(name(function () { Reflect.construct(1, []); }) === 'TypeError');
row(name(function () { Reflect.construct(Object.is, []); }) === 'TypeError');
row(name(function () { Reflect.construct(function () {}, [], () => 1); }) === 'TypeError');
row(name(function () { Reflect.construct(function () {}, 1); }) === 'TypeError');
row(name(function () { Reflect.construct(function () {}); }) === 'TypeError');
row(name(function () { new Reflect.construct(function () {}, []); }) === 'TypeError');
failed === 0 ? rows : -failed;
