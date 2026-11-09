// bbp-authored conformance case (light-cljs-880).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 1558-1624:
// optional_chaining (line 1558, bbp-134) and
// optional_chaining_runtime_error_positions (1602): short-circuiting of
// member, computed, call, spread-call, tagged and delete links, receivers
// kept through `?.`, parenthesized chains ending the short circuit, optional
// indirect `eval`, and which operator throws (by name; the Pants test also
// checks positions). The `JSON.stringify` check is a for-in check here (JSON
// is the core built-ins area, light-cljs-767). Left out: the rows deleting a
// property made non-configurable by `Object.defineProperty`
// (light-cljs-881). The last row is the planner's (light-cljs-766). Each row
// is a `row(...)` check; the script completes with the number of rows, or
// minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// optional_chaining (line 1558, bbp-134).
row(null?.x === undefined && undefined?.x === undefined);
{
  let n = 0; function f() { ++n; } undefined?.[f()]?.(f()).x[f()](f());
  row(n === 0);
}
{
  let z = null;
  row(z?.b.c === undefined && z?.b['c'] === undefined);
}
{
  let z = null; let caught = false; try { (z?.b).c; } catch(e) { caught = e instanceof TypeError; }
  row(caught);
}
{
  let z = null;
  row(delete z?.b.c === true && delete z?.b['c'] === true);
}
{
  let a = {b: {c: 2}}; let s = '';
  row(delete a?.b.c === true && a.b.c === undefined && (() => { for (let k in a.b) s += k; return s === ''; })());
}
{
  let a = {b: {c: 2}};
  row(delete a?.b['c'] === true && a.b.c === undefined);
}
{
  let a = {b() { return this._b; }, _b: {c: 42}};
  row((a?.b)().c === 42 && (a?.['b'])().c === 42 && a?.b().c === 42 && a?.['b']().c === 42);
}
{
  let g = null;
  row(g?.() === undefined);
}
{
  let o = {v: 7, m() { return this.v; }};
  row(o.m?.() === 7 && (o.m)?.() === 7 && o?.m?.() === 7);
}
{
  let n = 0; let o = {get m() { ++n; return null; }};
  row(o?.m?.(++n).x === undefined && n === 1);
}
{
  let n = 0; let o = {get m() { ++n; return function() { return this.v; }; }, v: 9};
  row(o?.m() === 9 && n === 1);
}
{
  let n = 0; let o = {m: null};
  row(o.m?.(...[++n]).x[++n] === undefined && n === 0);
}
{
  let o = {v: 7, m(a, b) { return this.v + a + b; }};
  row(o?.m(...[1, 2]) === 10 && o.m?.(...[1, 2]) === 10);
}
{
  let n = 0; function base() { ++n; return null; }
  row(delete base()?.[++n].x === true && n === 1);
}
{
  let caught = false; try { ({b: null})?.b.c; } catch(e) { caught = e instanceof TypeError; }
  row(caught);
}
{
  let caught = false; try { (null?.m)(); } catch(e) { caught = e instanceof TypeError; }
  row(caught);
}
{
  let n = 0; function f() { ++n; } let caught = false; try { (null?.m)(f()); } catch(e) { caught = e instanceof TypeError; }
  row(caught && n === 1);
}
{
  let f = function() { 'use strict'; return this; };
  row(f?.() === undefined);
}
{
  let o = {m() { return this; }};
  row((0, o?.m)?.() === undefined);
}
{
  var x = 1; function f() { let x = 2; return eval?.('x'); }
  row(f() === 1);
}
row(eval?.('null?.x') === undefined);
{
  let o = {m() { return {v: 3, n() { return this.v; }}; }};
  row(o?.m().n?.() === 3);
}
{
  let n = 0; let o = {get x() { ++n; }};
  row(delete o?.x && n === 0);
}
{
  let n = 0; let key = {toString() { ++n; return 'x'; }};
  row(null?.[key] === undefined && n === 0);
}
{
  let n = 0; let key = {toString() { ++n; return 'x'; }}; let o = {x: 1};
  row(delete o?.[key] && n === 1 && o.x === undefined);
}
{
  let o = {b: {}}; (o?.b).c = 7;
  row(o.b.c === 7);
}
{
  let o = {v: 7, tag() { return this.v; }};
  row((o?.tag)`` === 7);
}
{
  let n = 0; let o = {m() { ++n; }};
  row(delete o?.m() && n === 1);
}
{
  let n = 0; let o = {m: null};
  row(delete o.m?.(++n) && n === 0);
}
{
  let n = 0; let caught = false; try { missing?.(++n); } catch(e) { caught = e instanceof ReferenceError; }
  row(caught && n === 0);
}
row(0?.x === undefined && ''?.length === 0);
row((false?.x) === undefined);
// optional_chaining_runtime_error_positions (line 1602): the failing operator, compared by name.
{
  let a = {b: null};
  row((name(() => a?.b.c)) === "TypeError");
}
{
  let a = {b: null};
  row((name(() => a?.b['c'])) === "TypeError");
}
{
  let a = null;
  row((name(() => (a?.b).c)) === "TypeError");
}
{
  let f = 1;
  row((name(() => f?.())) === "TypeError");
}
{
  let a = {m: 1};
  row((name(() => a?.m())) === "TypeError");
}
{
  let a = {m: 1};
  row((name(() => a.m?.())) === "TypeError");
}
// The planner's row (light-cljs-766).
{
  let a = {b: {c: 2}}, z = null;
  row(((z?.b.c === undefined) + ',' + a?.b.c + ',' + (delete a?.b.c) + ',' + (a.b.c === undefined)) === "true,2,true,true");
}
failed === 0 ? rows : -failed;
