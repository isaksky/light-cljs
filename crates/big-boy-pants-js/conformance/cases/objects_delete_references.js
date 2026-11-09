// bbp-authored conformance case (light-cljs-880).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 17876-18140:
// delete_property_references (line 17876, bbp-124), its strict rows without
// defineProperty, freeze or preventExtensions: own and absent properties,
// computed keys, re-added keys going last, inherited properties staying,
// array holes, primitive bases, `length` and `prototype` refusing in strict
// code, function and error properties, and non-references. Errors are
// compared by name. `Object.keys` is for-in here and `Object.create` a
// `__proto__` literal. Left out: the non-configurable and frozen rows
// (light-cljs-881, light-cljs-882); `delete` of built-in properties, which
// the Pants engine refuses (its realm is shared and immutable) where QuickJS
// deletes; `__rt.vector`; the strict `delete x` early error (an exporter
// SyntaxError); and `s[4294967294] = 1`, which exhausts Goose's heap
// (sparse arrays, the core built-ins area, light-cljs-767). The sloppy rows
// are objects_delete_references_sloppy. Each row is a `row(...)` check; the
// script completes with the number of rows, or minus the first failing row's
// number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// delete_property_references (line 17876, bbp-124).
{
  let a = {x: 1, y: 1};
  row(delete a.x === true);
}
{
  let a = {x: 1, y: 1}; delete a.x;
  row(a.x === undefined && ('x' in a) === false && a.y === 1);
}
row(delete 'abc'[100] === true);
row((name(() => delete null.a)) === "TypeError");
{
  let a = {x: 1, y: 1};
  row(delete a.missing);
}
{
  let a = {x: 1, y: 1};
  row(delete a['y'] && a.y === undefined && a.x === 1);
}
{
  let a = {x: 1, y: 1}; let n = 0;
  row(delete (a.x) && delete ((a)['y']) && (() => { for (let k in a) n++; return n === 0; })());
}
{
  let o = {1: 'one', 2: 'two'}; let n = 0;
  row(delete o[1] && delete o['2'] && (() => { for (let k in o) n++; return n === 0; })());
}
{
  let o = {true: 1, null: 2}; let n = 0;
  row(delete o[true] && delete o[null] && (() => { for (let k in o) n++; return n === 0; })());
}
{
  let a = {x: 1, y: 1};
  row(typeof delete a.x === 'boolean');
}
{
  let a = {x: 1, y: 1};
  row(delete a.x && delete a.x);
}
{
  let a = {x: 1, y: 1}; delete a.x; a.x = 2; let k = []; for (let p in a) k.push(p);
  row(k[0] === 'y' && k[1] === 'x');
}
{
  let p = {x: 1}; let o = {__proto__: p};
  row(delete o.x && o.x === 1 && p.x === 1);
}
{
  let o = {a: 0, b: 1, c: 2, d: 3, e: 4, f: 5, g: 6, h: 7, i: 8, j: 9}; let keys = () => { let k = []; for (let p in o) k.push(p); return k; };
  row(delete o.c && o.d === 3 && o.j === 9 && o.a === 0 && keys().length === 9 && keys()[2] === 'd' && (o.c = 10, keys()[9] === 'c' && o.c === 10));
}
{
  let u;
  row((name(() => delete u[0])) === "TypeError");
}
row((name(() => delete null[{}])) === "TypeError");
{
  let u; let n = 0; try { delete u[n++]; } catch (e) {}
  row(n === 1);
}
{
  let arr = [1, 2];
  row(delete arr[0]);
}
{
  let arr = [1, 2]; delete arr[0]; let k = []; for (let p in arr) k.push(p);
  row(arr.length === 2 && arr[0] === undefined && arr[1] === 2 && Object.getOwnPropertyDescriptor(arr, '0') === undefined && k.length === 1 && k[0] === '1');
}
{
  let arr = [1, 2];
  row(delete arr['1'] && arr.length === 2 && arr[1] === undefined);
}
{
  let arr = [1, 2];
  row(delete arr[5] && delete arr[-1] && arr.length === 2);
}
{
  let arr = [1, 2]; delete arr[0]; arr[0] = 3;
  row(arr[0] === 3 && arr.length === 2);
}
{
  let arr = [1, 2];
  row((name(() => delete arr.length)) === "TypeError");
}
{
  let arr = [1, 2]; arr.name = 1;
  row(delete arr.name && arr.name === undefined);
}
row((name(() => delete 'abc'[0])) === "TypeError");
row((name(() => delete 'abc'.length)) === "TypeError");
row(delete 'abc'[3] && delete 'abc'.x && delete (1).x && delete true.x);
row(delete 1);
{
  let n = 0;
  row(delete (n = n + 1, n) && n === 1);
}
{
  let n = 0; let f = function () { n = 7; return {x: 1}; };
  row(delete f() && n === 7);
}
{
  let f = function () { return delete this; };
  row(f());
}
{
  let f = function () {};
  row(delete f.x);
}
{
  let f = function () {}; f.y = 1;
  row(delete f.y && f.y === undefined);
}
{
  let f = function () {};
  row((name(() => delete f.prototype)) === "TypeError");
}
{
  let g = () => 1;
  row(delete g.prototype);
}
{
  let f = function () {};
  row(delete f.length && Object.getOwnPropertyDescriptor(f, 'length') === undefined);
}
row(delete Object.prototype.nothing && delete Object.nothing);
{
  let r; try { null.x; } catch (e) { r = delete e.message && e.message === ''; }
  row(r);
}
failed === 0 ? rows : -failed;
