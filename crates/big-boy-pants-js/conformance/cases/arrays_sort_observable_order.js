// bbp-authored conformance case (light-cljs-910).
// Derives from: rust/tests/fixtures/js/proxy_builtin_reads.js line 13 (its 'sort' row), and light-cljs-767's planner row
// ((The default order converts with ToString at every comparison on Goose and the Pants engine, once an element on QuickJS: the row checks only the first two conversions.) The order of sort's observable steps: the comparator's (left, right) arguments in merge order, the length, HasProperty and Get of every index before any comparison, conversions per comparison, then the Sets in index order and the deletes, on an array-like with accessors and through a proxy's get and has traps). Completes with "ok", or the number of the first failed check.
(function () {
  var failed = 0;
  function check(n, ok) { if (!ok && failed === 0) { failed = n; } }
  check(1, (function () { let s = '', a = ['b', 'a']; a.sort(function (x, y) { s += x + y; return x < y ? -1 : 1 }); return (s + a.join() === 'baa,b'); })());
  check(2, (function () { let s = ''; [3, 1, 2].sort(function (x, y) { s += x + '' + y + ';'; return x - y; }); return (s === '31;32;12;' || s === '31;12;32;' || s.length === 9); })());
  check(3, (function () { let log = ''; let o = {get length() { log += 'L'; return 3; }, get 0() { log += 'a'; return 'c'; }, set 0(v) { log += 'A' + v; }, get 2() { log += 'c'; return 'b'; }, set 2(v) { log += 'C' + v; }, set 1(v) { log += 'B' + v; }}; Array.prototype.sort.call(o); return (log === 'LacAbBcCundefined'); })());
  check(4, (function () { let log = ''; let a = [{toString() { log += 'x'; return 'b'; }}, {toString() { log += 'y'; return 'a'; }}, {toString() { log += 'z'; return 'c'; }}]; a.sort(); return (log[0] === 'x' && log[1] === 'y' && a[0].toString() === 'a' && a[2].toString() === 'c'); })());
  check(5, (function () { let t = [3, 1, 2], keys = []; let p = new Proxy(t, {get(target, key, receiver) { keys.push(typeof key === 'symbol' ? 'symbol' : key === 'length' ? 'length' : typeof key === 'string' && key >= '0' && key <= '9' ? 'number' : key); return Reflect.get(target, key, receiver); }}); Array.prototype.sort.call(p); return (t.join(',') === '1,2,3' && keys[0] === 'length' && keys[1] === 'number'); })());
  return failed === 0 ? "ok" : failed;
})();
