// bbp-authored conformance case (light-cljs-910).
// Derives from: rust/tests/js_source.rs lines 25950-26050 (array_sort_default_and_comparator, array_sort_merge_boundaries_and_errors without its BigInt row, light-cljs-771, and array_sort_native_comparators) and line 20551 (sort of a symbol's wrapper), rust/tests/fixtures/js/array_copy.js lines 17-19, 31-33 and 53-54 (its toSorted rows), and light-cljs-767's planner rows
// (Array.prototype.sort and toSorted: the default order by the code units of ToString, comparators and their ToNumber, NaN and string results, stability at every length to 66, undefineds last and holes kept after them, array-likes, inherited elements, the comparator check before any read, abrupt completions, refused writes and deletes, built-in and bound comparators, and toSorted's new array with holes read as undefined). Completes with "ok", or the number of the first failed check.
(function () {
  var failed = 0;
  function check(n, ok) { if (!ok && failed === 0) { failed = n; } }
  check(1, (function () {  return ([3, 1, 10, 2].sort().join() === '1,10,2,3'); })());
  check(2, (function () { let a = [3, 1, 10, 2]; return (a.sort(function(a,b) { return a-b; }) === a && a.join() === '1,2,3,10'); })());
  check(3, (function () { let a = [{k:1,v:'a'}, {k:0,v:'b'}, {k:1,v:'c'}]; a.sort(function(a,b) { return a.k-b.k; }); return (a[0].v === 'b' && a[1].v === 'a' && a[2].v === 'c'); })());
  check(4, (function () { let a = [undefined, 2, , 1]; a.sort(); return (a.join() === '1,2,,' && a.length === 4 && a[2] === undefined && (2 in a) && !(3 in a)); })());
  check(5, (function () { let ok = false; try { [1].sort(5); } catch(e) { ok = e instanceof TypeError; } return (ok); })());
  check(6, (function () { let reads = 0, o = {get length() { reads++; return 1; }, get 0() { reads++; return 1; }}; let ok = false; try { Array.prototype.sort.call(o, 5); } catch(e) { ok = e instanceof TypeError; } return (ok && reads === 0); })());
  check(7, (function () { let a = [3,2,1]; a.sort(function(a,b) { return {valueOf() { return a-b; }}; }); return (a.join() === '1,2,3'); })());
  check(8, (function () { let a = [3,2,1]; a.sort(function() { return NaN; }); return (a.join() === '3,2,1'); })());
  check(9, (function () { let a = [3,2,1]; a.sort(function() { return '0'; }); return (a.join() === '3,2,1'); })());
  check(10, (function () { let calls = 0; [undefined, 1, undefined].sort(function() { calls++; return 0; }); return (calls === 0); })());
  check(11, (function () { let a = [{toString() { return 'b'; }}, {toString() { return 'a'; }}]; a.sort(); return (a[0].toString() === 'a'); })());
  check(12, (function () {  return (['\ue000', '\ud800\udc00', '\ud800'].sort().join('|') === '\ud800|\ud800\udc00|\ue000'); })());
  check(13, (function () { let a = [3,2,1], e = {}; let caught; try { a.sort(function() { throw e; }); } catch(x) { caught = x; } return (caught === e && a.join() === '3,2,1'); })());
  check(14, (function () { let o = {length: 4, 0: undefined, 2: 2, 3: 1}; return (Array.prototype.sort.call(o) === o && o[0] === 1 && o[1] === 2 && o[2] === undefined && !(3 in o)); })());
  check(15, (function () { let a = [,2,1]; Object.setPrototypeOf(a, {0: 3}); Array.prototype.sort.call(a); return (a[0] === 1 && a[1] === 2 && a[2] === 3); })());
  check(16, (function () { let a = [2,1]; Object.defineProperty(a, '0', {writable: false}); let ok = false; try { a.sort(); } catch(e) { ok = e instanceof TypeError; } return (ok); })());
  check(17, (function () {  return (Array.prototype.sort.length === 1 && Array.prototype.sort.name === 'sort' && !Object.getOwnPropertyDescriptor(Array.prototype, 'sort').enumerable); })());
  check(18, (function () { let ok = true; for (let n = 0; n < 67; n++) { let a = []; for (let i = 0; i < n; i++) a.push({k: (i * 13) % 7, i: i}); a.sort(function(a,b) { return a.k-b.k; }); if (a.length !== n) ok = false; for (let i = 1; i < n; i++) if (a[i-1].k > a[i].k || (a[i-1].k === a[i].k && a[i-1].i > a[i].i)) ok = false; } return (ok); })());
  check(19, (function () { let a = [2,,1]; Object.defineProperty(a, '2', {configurable: false}); let ok = false; try { a.sort(); } catch(e) { ok = e instanceof TypeError; } return (ok && a[0] === 1 && a[1] === 2); })());
  check(20, (function () { let e = {}, caught; try { [{toString() { throw e; }}, 1].sort(); } catch(x) { caught = x; } return (caught === e); })());
  check(21, (function () { let e = {}, caught; try { [2,1].sort(function() { return {valueOf() { throw e; }}; }); } catch(x) { caught = x; } return (caught === e); })());
  check(22, (function () { let ok = false; try { [Symbol(), 1].sort(); } catch(e) { ok = e instanceof TypeError; } return (ok); })());
  check(23, (function () { let ok = false; [2,1].sort(function(a,b) { 'use strict'; ok = this === undefined; return a-b; }); return (ok); })());
  check(24, (function () { let n = Array.prototype.sort.call(5); return (typeof n === 'object' && n.valueOf() === 5); })());
  check(25, (function () { let ok = false; try { Array.prototype.sort.call('ba'); } catch(e) { ok = e instanceof TypeError; } return (ok); })());
  check(26, (function () { let count = 0; let a = [2,1]; a.sort(function(a,b) { count++; return a-b; }.bind({})); return (count === 1 && a.join() === '1,2'); })());
  check(27, (function () { let a = [3,1,2]; return (a.sort(Number.isNaN) === a && a.join() === '3,1,2'); })());
  check(28, (function () {  return ([3,1,2].sort(Number.isNaN.bind(null)).join() === '3,1,2'); })());
  check(29, (function () {  return ([1,2].sort(Number).join() === '2,1'); })());
  check(30, (function () { let a = [{i:3,valueOf() { return 0; }}, {i:1,valueOf() { return 0; }}, {i:2,valueOf() { return 0; }}]; a.sort(Number); return (a[0].i === 3 && a[1].i === 1 && a[2].i === 2); })());
  check(40, (function () { let a = [3, 1, undefined, 10, , 2]; a.sort(); return (a.length + ':' + a.join() + ';' + [3,1,2].sort((x,y) => y - x).join() + ';' + [3,1,2].toSorted().join() === '6:1,10,2,3,,;3,2,1;1,2,3'); })());
  check(41, (function () { let a = [{k:1,i:0},{k:0,i:1},{k:1,i:2}]; a.sort((x,y) => x.k - y.k); return (a.map(x => x.i).join() === '1,0,2'); })());
  check(42, (function () { try { [1,2].sort(3); } catch (e) { return e instanceof TypeError; } return false; })());
  check(43, (function () { try { [1,2].toSorted(3); } catch (e) { return e instanceof TypeError; } return false; })());
  check(44, (function () { let a = [3,1,2]; let r = a.toSorted((x,y)=>y-x); return (r.join() === '3,2,1' && a.join() === '3,1,2' && r !== a && [10,2,1].toSorted().join() === '1,10,2'); })());
  check(45, (function () { let holes = new Array(4); holes[1] = 2; let proto = Object.create(Array.prototype); proto[2] = 7; Object.setPrototypeOf(holes, proto); let sorted = holes.toSorted(); return (sorted.length === 4 && sorted[0] === 2 && sorted[1] === 7 && 2 in sorted && 3 in sorted && Array.isArray(sorted) && Object.getPrototypeOf(sorted) === Array.prototype); })());
  check(46, (function () { let stable=[{k:1,id:0},{k:0,id:1},{k:1,id:2}].toSorted((a,b)=>a.k-b.k); return (stable[0].id === 1 && stable[1].id === 0 && stable[2].id === 2); })());
  check(47, (function () { let s = Symbol('x'); return (Array.prototype.sort.call(s).valueOf() === s); })());
  check(48, (function () { let d = Object.getOwnPropertyDescriptor(Array.prototype, 'toSorted'); return (d.value.name === 'toSorted' && d.value.length === 1 && d.writable && d.configurable && !d.enumerable); })());
  check(49, (function () { let a = []; for (let i = 0; i < 300; i++) a.push((i * 7919) % 301); let b = a.toSorted((x, y) => x - y); a.sort(); let ok = true; for (let i = 1; i < 300; i++) { if (b[i - 1] > b[i] || String(a[i - 1]) > String(a[i])) ok = false; } return (ok); })());
  return failed === 0 ? "ok" : failed;
})();
