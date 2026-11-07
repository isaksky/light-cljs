// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_functions.rs lines 2093-2200, 334-360:
// arrow_function_values (line 2093, bbp-98), arrows_capture_this_lexically
// (2168, bbp-98) and the rows of new_target_in_functions_and_arrows (334,
// bbp-99) without eval or classes; js_source.rs
// method_calls_bind_this (14597, bbp-116). Rewritten: the `__rt.reduce`
// callback row runs the arrow in a loop; the ASI row (`() => {}\n+1`) checks
// the arrow in a `var` initializer instead of the script's completion. The
// `bind` rows and parts of rows are back (light-cljs-852). Left
// out: arrows completed by the script and called from another task (Pants
// runtime API); error messages. Each row is a
// `row(...)` check; the script completes with the number of rows, or minus
// the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// ASI after an arrow's block body.
var asiArrow = () => {}
+1
row(typeof asiArrow === "function");
// Parameters, concise bodies and closure capture.
{ const add = (a, b) => a + b; row(add(2, 3) === 5); }
{ const k = x => () => x; row(k(7)() === 7); }
row(((a, b,) => b)(1, 2, 3) === 2);
row(((a, b) => b)(1) === undefined);
row((() => {})() === undefined);
// Block bodies return, see and assign enclosing bindings.
{ let n = 0; const inc = () => { n = n + 1; return n; }; inc(); row(inc() === 2); }
{ const fact = n => n <= 1 ? 1 : n * fact(n - 1); row(fact(5) === 120); }
// Each evaluation creates a new function object.
{ const mk = () => () => 1; row(mk() !== mk()); }
// An arrow is a callback like any function value.
{
  const reduce = (f, acc, a) => { for (let i = 0; i < a.length; i++) acc = f(acc, a[i]); return acc; };
  row(reduce((acc, x) => acc + x * 10, 1, [1, 2, 3]) === 61);
}
// The temporal dead zone applies to what an arrow captures.
{ const f = () => later; const e = name(f); const later = 1; row(e === "ReferenceError" && f() === 1); }
// An arrow inside an ordinary function captures its receiver.
{ function make() { const f = () => this; return f; } row(make()() === undefined); }
// A script-level arrow has no `arguments`.
{ const f = (a) => arguments; row(name(f) === "ReferenceError"); }
// Lexical `this`.
{ function f() { return (() => this)(); } row(f() === undefined); }
{ let r; function F() { r = (() => this)(); } const o = new F(); row(r === o); }
{ let r; function F() { const k = () => () => this; r = k()(); } const o = new F(); row(r === o); }
{
  let r; function F() { r = () => this; }
  const o = new F(); const first = r; const p = new F();
  row(first() === o && r() === p);
}
{ function make() { return () => this; } const a = make.call(7); const o = {a: a}; row(a() === 7 && o.a() === 7 && a.call(8) === 7 && a.apply(9, []) === 7 && Reflect.apply(a, 10, []) === 7 && a.bind(11)() === 7); }
{ function make() { return () => { return function () { return () => this; }; }; } const a = make.call(1)().call(2); row(a.call(3) === 2); }
{ function make() { return (x = this) => x; } const a = make.call(7); row(a.call(8) === 7 && a(9) === 9); }
{ function make(a = () => this) { return a; } row(make.call(7)() === 7); }
{ function F() { this.a = (x = () => this) => x; } const o = new F(); row(o.a()() === o); }
{ function make() { return () => 1; } row(make()() === 1); }
{ function make(n) { return (x = this) => { const a = () => n + this + x; return a() + this; }; } row(make.call(7, 2)() === 23); }
{ function f() { return () => this; } row(f.call(7).call(8) === 7); }
{ function f() { return (x = this) => x; } row(f.call(7).bind(8)() === 7); }
{ const f = () => this; row(f() === globalThis); }
// An arrow is not a constructor.
{ function make() { return () => this; } const a = make(); row(name(function () { new a(); }) === "TypeError"); }
// new.target, directly and through arrows.
{ let r; function F() { r = new.target; } new F(); row(r === F); }
{ let r; function F() { r = new.target; } F(); row(r === undefined); }
{ let r; function F() { r = (() => new.target)(); } new F(); row(r === F); }
{ let r; function F() { r = (() => new.target)(); } F(); row(r === undefined); }
{ function F() { return () => () => new.target; } row(new F()()() === F && F()()() === undefined); }
{
  function F(a = new.target, b = () => new.target) { return {a: a, b: b}; }
  const r = new F();
  row(r.a === F && r.b() === F && F().a === undefined);
}
{ function F() { return function G() { return new.target; }; } const G = new F(); row(G() === undefined && new G() === G); }
{ let r; function F() { r = new.target; } const B = F.bind(null); new B(); row(r === F); }
{ function F() { this.name = new.target.name; } row(new F().name === "F"); }
{ function F(again) { if (!again) return new new.target(true); this.ok = new.target === F; } row(new F().ok); }
{ function F() { return new.target; } row(F.call({}) === undefined && F.apply(null, []) === undefined && Reflect.apply(F, null, []) === undefined); }
{
  function* G() { yield new.target; return (() => new.target)(); }
  const g = G();
  row(g.next().value === undefined && g.next().value === undefined);
}
{ const o = {get x() { return new.target; }, m(a = new.target) { return a; }}; row(o.x === undefined && o.m() === undefined); }
// Method calls bind `this`.
{
  let o = {x: 1, m: function () { return this.x; }};
  row(o.m() === 1 && (o.m)() === 1 && ((o.m))() === 1 && o["m"]() === 1);
  // A comma or assignment result, or a plain reference, is not a member
  // callee: `this` is undefined, so `this.x` throws a TypeError.
  row(name(function () { (0, o.m)(); }) === "TypeError");
  row(name(function () { let f = o.m; f(); }) === "TypeError");
  row(name(function () { let f; (f = o.m)(); }) === "TypeError");
  row(({inner: o}).inner.m() === 1);
  // The base is evaluated once.
  let n = 0;
  function getO() { n = n + 1; return o; }
  row(getO().m() * 10 + n === 11);
}
{ function g() { return this; } row(g() === undefined); }
{ let p = {me: function () { return this; }}; row(p.me() === p); }
{ let p = {v: 0, set: function (v) { this.v = v; }}; p.set(10); row(p.v === 10); }
{ let c = {v: 2, get: function () { return this.v; }, twice: function () { return this.get() * 2; }}; row(c.twice() === 4); }
{
  let c = {v: 3, m: function () { function inner() { return this; } return inner() === undefined ? this.v : -1; }};
  row(c.m() === 3);
}
{ let c = {v: 5, m: function (a = this) { return a.v; }}; row(c.m() === 5); }
{ function get() { return this.v; } let a = {v: 1, get}; let b = {v: 2, get}; row(a.get() * 10 + b.get() === 12); }
{
  let c = {v: 7, m: function () { try { throw 1; } catch (e) { return this.v; } finally { this.v = 8; } }};
  row(c.m() * 10 + c.v === 78);
}
failed === 0 ? rows : -failed;
