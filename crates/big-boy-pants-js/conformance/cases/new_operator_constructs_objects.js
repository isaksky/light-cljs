// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 14661-14795,
// new_operator_constructs_objects (bbp-71), and rust/src/vm/dispatch.rs
// construct_target (ECMAScript [[Construct]] of an ordinary function,
// OrdinaryCreateFromConstructor and the return override): the new object
// inherits F.prototype (Object.prototype when that is not an object) and is
// `this`; an object result replaces it, any other result is ignored; a plain
// strict call's `this` stays undefined; callee and arguments are evaluated
// before a non-constructor is a TypeError. Each row is a `row(...)` check;
// the script completes with the number of rows, or minus the first failing
// row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
function F(x) { this.x = x; }
row(new F(2).x === 2);
row((new F).x === undefined);
row(new F(1) !== new F(1));
row(typeof new F(1) === "object");
// Instances share F.prototype, whose `constructor` is F, and see its later
// properties.
{ const a = new F(1), b = new F(2); F.prototype.k = 7;
  row(a.constructor === F && b.k === 7 && a.k === 7 && a.x === 1 && Object.getPrototypeOf(a) === F.prototype); }
row(Object.getPrototypeOf(F.prototype) === Object.prototype);
row(Object.getOwnPropertyDescriptor(new F(3), "x").value === 3);
{ function P() {} P.prototype = {m: function () { return this.v * 2; }};
  function Q(v) { this.v = v; } Q.prototype = P.prototype; row(new Q(21).m() === 42); }
// A `prototype` that is not an object gives Object.prototype.
{ function N() {} N.prototype = 5; const n = new N();
  row(n.constructor === Object && Object.getPrototypeOf(n) === Object.prototype); }
{ function M() {} M.prototype = null; row(Object.getPrototypeOf(new M()) === Object.prototype); }
// Return override.
{ function G() { this.y = 1; return {y: 2}; } row(new G().y === 2); }
{ function H() { this.z = 1; return 5; } row(new H().z === 1); }
{ function U() { this.z = 3; return; } row(new U().z === 3); }
{ function U() { this.z = 3; return null; } row(new U().z === 3); }
{ function S() { this.z = 3; return "s"; } row(new S().z === 3); }
{ const fn = function () { return 1; }; function R() { this.r = 1; return fn; } row(new R() === fn); }
{ const a = [1]; function R() { return a; } row(new R() === a); }
// Through try/finally: the completion the finally block leaves is checked.
{ function T() { try { this.a = 1; return 2; } finally { this.b = 3; } } const t = new T(); row(t.a * 10 + t.b === 13); }
{ const o = {}; function T() { try { return o; } finally { this.b = 3; } } row(new T() === o); }
// Arguments, nesting, and the result as a callee or a base.
{ function S(a, b) { this.s = a + b; } row(new S(1, 2).s + new S(new S(3, 4).s, 1).s === 11); }
{ function K() { return function () { return 9; }; } row(new K()() === 9); }
{ const ns = {C: function (v) { this.v = v; }}; row(new ns.C(6).v === 6); }
{ function A() { this.n = 1; } function B() { return new A(); } row(new (new B().constructor)().n === 1); }
{ function Args() { this.n = arguments.length; this.last = arguments[arguments.length - 1]; } const r = new Args(1, 2, 3); row(r.n === 3 && r.last === 3); }
// A plain call is not a construction: strict `this` is undefined, also from
// inside a constructor, whose `this` is intact afterwards.
{ function P() { return this; } row(P() === undefined); }
{ function inner() { return this === undefined ? 1 : 0; } function O() { this.u = inner(); this.w = this.u + 1; } row(new O().w === 2); }
// A constructor that throws: the exception propagates, no object results.
{ function E() { this.e = 1; throw "boom"; } let r = "none"; try { new E(); } catch (e) { r = e; } row(r === "boom"); }
// Recursion through `new`.
{ function L(n) { this.next = n > 0 ? new L(n - 1) : null; this.n = n; } let l = new L(3), s = 0; while (l) { s += l.n; l = l.next; } row(s === 6); }
// Not constructors: catchable TypeErrors, after callee and arguments.
row(name(() => new 1) === "TypeError");
row(name(() => new undefined) === "TypeError");
row(name(() => { const o = {}; new o(); }) === "TypeError");
row(name(() => { const a = () => 1; new a(); }) === "TypeError");
{ let log = ""; function c() { log += "c"; return 1; } function a() { log += "a"; return 0; }
  try { new (c())(a()); } catch (e) { log += e.name; } row(log === "caTypeError"); }
failed === 0 ? rows : -failed;
