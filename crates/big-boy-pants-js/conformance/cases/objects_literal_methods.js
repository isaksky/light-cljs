// bbp-authored conformance case (light-cljs-880).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 14422-14480, 14525-14571, 14597-14711:
// object_literal_methods (line 14422, bbp-116), the strict-script run row of
// object_literal_methods_inherit_strictness (14525) and the strict rows of
// method_calls_bind_this (14597, bbp-116, bbp-126): method names, `this`,
// receivers of member, parenthesized and computed callees, plain calls with
// an undefined `this`, methods are not constructors and have no prototype.
// Errors are compared by name. Left out: the early-error rows of
// object_literal_methods_inherit_strictness (exporter SyntaxErrors, not
// execution cases), the corrupted-bytecode and opcode checks, the entry
// function and `__rt.reduce` rows (Pants runtime API), and `super` in literal
// methods (light-cljs-888). The sloppy rows are objects_literal_methods_sloppy.
// Each row is a `row(...)` check; the script completes with the number of
// rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// object_literal_methods (line 14422, bbp-116).
row((({b() { return this._b; }, _b: {c: 42}}).b().c) === 42);
row((({get() { return 1; }}).get()) === 1);
row((({static() { return 42; }}).static()) === 42);
row((({m() {}}).m.prototype) === undefined);
{
  let o = {m() { return this; }};
  row(o.m() === o);
}
row((({m() {}}).m.name) === "m");
row((({1.5() {}})[1.5].name) === "1.5");
row((({"s"() {}}).s.name) === "s");
row((({if() { return 42; }}).if()) === 42);
row((({async() { return 42; }}).async()) === 42);
row((({set(a = 42) { return arguments.length + a; }}).set()) === 42);
{
  let o = {m() { return () => this; }};
  row(o.m()() === o);
}
{
  let f = ({m() { return this; }}).m;
  row((f()) === undefined);
}
{
  let o = {m() {}}; let d = Object.getOwnPropertyDescriptor(o, 'm');
  row(d.writable && d.enumerable && d.configurable);
}
{
  let o = {m() {}};
  row((name(() => new o.m())) === "TypeError");
}
row((({__proto__: null, __proto__() { return 42; }}).__proto__()) === 42);
{
  const a = 20, b = 22;
  row((({m() { return a + b; }}).m()) === 42);
}
{
  let m = 42;
  row((({m() { return m; }}).m()) === 42);
}
{
  let o = {x: 1, m() { return this.x; }};
  row((o.m.call({x: 42})) === 42);
}
{
  let i = 0; let o = {m() { i = i + 1; return i; }};
  row((o.m() + o.m()) === 3);
}
row((({m(a, b = 1, c) {}}).m.length) === 1);
row(Object.getOwnPropertyDescriptor(({m() {}}).m, 'prototype') === undefined);
// object_literal_methods_inherit_strictness (line 14525): the strict-script run rows.
row((({m() { return this; }}).m.call(undefined)) === undefined);
// method_calls_bind_this (line 14597, bbp-116, bbp-126).
{
  let o = {x: 1, m: function () { return this.x; }};
  row((o.m()) === 1);
}
{
  let o = {x: 1, m: function () { return this.x; }};
  row(((o.m)()) === 1);
}
{
  let o = {x: 1, m: function () { return this.x; }};
  row((((o.m))()) === 1);
}
{
  let o = {x: 1, m: function () { return this.x; }};
  row((o["m"]()) === 1);
}
{
  let o = {x: 1, m: function () { return this.x; }}; let r = 'none'; try { (0, o.m)(); } catch (e) { r = 'caught'; }
  row((r) === "caught");
}
{
  let o = {x: 1, m: function () { return this.x; }}; let r = 'none'; try { let f = o.m; f(); } catch (e) { r = 'caught'; }
  row((r) === "caught");
}
{
  let o = {x: 1, m: function () { return this.x; }}; let r = 'none'; try { let f; (f = o.m)(); } catch (e) { r = 'caught'; }
  row((r) === "caught");
}
{
  let o = {x: 1, m: function () { return this.x; }};
  row((name(() => (0, o.m)())) === "TypeError");
}
{
  function g() { return this; }
  row((g()) === undefined);
}
{
  let o = {x: 1, m: function () { return this.x; }};
  row((({inner: o}).inner.m()) === 1);
}
{
  let o = {x: 1, m: function () { return this.x; }}; let n = 0; function getO() { n = n + 1; return o; }
  row((getO().m() * 10 + n) === 11);
}
{
  let p = {me: function () { return this; }};
  row(p.me() === p);
}
{
  let p = {v: 0, set: function (v) { this.v = v; }}; p.set(10);
  row((p.v) === 10);
}
{
  let c = {v: 2, get: function () { return this.v; }, twice: function () { return this.get() * 2; }};
  row((c.twice()) === 4);
}
{
  let c = {v: 3, m: function () { function inner() { return this; } return inner() === undefined ? this.v : -1; }};
  row((c.m()) === 3);
}
{
  let c = {v: 5, m: function (a = this) { return a.v; }};
  row((c.m()) === 5);
}
{
  function get() { return this.v; } let a = {v: 1, get}; let b = {v: 2, get};
  row((a.get() * 10 + b.get()) === 12);
}
{
  let c = {v: 7, m: function () { try { throw 1; } catch (e) { return this.v; } finally { this.v = 8; } }};
  row((c.m() * 10 + c.v) === 78);
}
failed === 0 ? rows : -failed;
