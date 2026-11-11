// bbp-authored conformance case (light-cljs-909).
// Derives from: none (light-cljs-909: Array.prototype.toSpliced, which the Pants engine lacks; scored against QuickJS; light-cljs-767's planner row)
// (Array.prototype.toSpliced: the copy with a range replaced, relative indices, holes read as undefined, an array-like receiver, getters read once in order, the receiver unchanged, a plain Array whatever the receiver's species, and the 2^53 - 1 TypeError). Completes with "ok", or the number of the first failed check.
(function () {
  var failed = 0;
  function check(n, ok) { if (!ok && failed === 0) { failed = n; } }
  check(1, (function () {  return ([1,2,3].toSpliced(1, 1, 'x').join() === '1,x,3'); })());
  check(2, (function () { let a = [1,2,3,4]; let r = a.toSpliced(-3, 2); return (r.join() === '1,4' && a.join() === '1,2,3,4' && r !== a); })());
  check(3, (function () { let a = [1,2,3]; return (a.toSpliced().join() === '1,2,3' && a.toSpliced(1).join() === '1' && a.toSpliced(1, undefined, 9).join() === '1,9,2,3' && a.toSpliced(Infinity, 1, 9).join() === '1,2,3,9' && a.toSpliced(-Infinity, Infinity).length === 0); })());
  check(4, (function () { let a = [1,,3]; let r = a.toSpliced(0, 0); return (r.length === 3 && (1 in r) && r[1] === undefined); })());
  check(5, (function () { let log = ''; let o = {length: 3, get 0() { log += 'a'; return 1; }, get 1() { log += 'b'; return 2; }, get 2() { log += 'c'; return 3; }}; let r = Array.prototype.toSpliced.call(o, {valueOf() { log += 's'; return 1; }}, {valueOf() { log += 'k'; return 1; }}, 9); return (Array.isArray(r) && r.join() === '1,9,3' && log === 'skac'); })());
  check(6, (function () { class A extends Array {}; let a = new A(1, 2, 3); let r = a.toSpliced(0, 1); return (Array.isArray(r) && !(r instanceof A) && r.join() === '2,3'); })());
  check(7, (function () { try { Array.prototype.toSpliced.call({length: 2**53 - 1}, 0, 0, 1); } catch (e) { return e instanceof TypeError; } return false; })());
  check(8, (function () { let f = Array.prototype.toSpliced, d = Object.getOwnPropertyDescriptor(Array.prototype, 'toSpliced'); return (f.name === 'toSpliced' && f.length === 2 && d.writable && d.configurable && !d.enumerable); })());
  check(9, (function () { let a = []; for (let i = 0; i < 200; i++) a.push({x: i}); let r = a.toSpliced(1, 100, {x: -1}); return (r.length === 101 && r[1].x === -1 && r[2].x === 101 && a.length === 200); })());
  return failed === 0 ? "ok" : failed;
})();
