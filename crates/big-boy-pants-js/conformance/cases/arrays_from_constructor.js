// bbp-authored conformance case (light-cljs-911).
// Derives from: none (light-cljs-911: rows of rust/tests/js_source.rs array_from_iterables_and_array_likes, array_copy_order_species_and_errors and the planner's that the Pants engine answers differently, mostly Array.from and Array.of with a custom constructor as `this`, which it refuses; scored against QuickJS)
// (Array.from and Array.of constructing `this`: the constructor called with no argument for an iterable and with the length for an array-like and for Array.of, results filled with CreateDataPropertyOrThrow and their length set). Completes with "ok", or the number of the first failed check.
(function () {
  var failed = 0;
  function check(n, ok) { if (!ok && failed === 0) { failed = n; } }
  check(1, (function () {  return (Array.from.call(function C(n) { this.made = n }, [1]).made === undefined); })());
  check(2, (function () { let r = Proxy.revocable([], {get(t,k,p) { if (k === 'length') { r.revoke(); return 0; } return Reflect.get(t,k,p); }}); let q = Proxy.revocable([], {get(t,k,p) { if (k === 'length') { q.revoke(); return 0; } return Reflect.get(t,k,p); }}); return ([].concat(r.proxy).length === 0 && [q.proxy].flat().length === 0); })());
  check(3, (function () { let log = ''; function numbers() { return {[Symbol.iterator]() { let i = 0; return {next() { i++; return {done: i > 3, value: i}; }, return() { log += 'r'; throw 99; }}; }}; } let caught = []; for (let trap of [function () { throw 17; }, function () { return false; }]) { function C() { return new Proxy({}, {defineProperty: trap}); } try { Array.from.call(C, numbers()); } catch (e) { caught.push(e === 17 ? 17 : e instanceof TypeError ? 'T' : e); } } return (caught.join() === '17,T' && log === 'rr'); })());
  check(4, (function () { let made = 'x'; function C(n) { made = arguments.length + ':' + n; } let r = Array.from.call(C, {length: 2, 0: 'a', 1: 'b'}); return (r instanceof C && made === '1:2' && r[1] === 'b' && r.length === 2); })());
  check(5, (function () { let made; function C() { made = arguments.length; } let r = Array.from.call(C, [5, 6]); return (r instanceof C && made === 0 && r.length === 2 && r[1] === 6); })());
  check(6, (function () { let log = ''; function C() { return new Proxy({}, {defineProperty(t, k, d) { log += 'd' + String(k); return Reflect.defineProperty(t, k, d); }, set(t, k, v, r) { log += 's' + String(k); return Reflect.set(t, k, v, r); }}); } Array.from.call(C, [5, 6]); Array.of.call(C, 1); return (log === 'd0d1slengthdlengthd0slengthdlength'); })());
  return failed === 0 ? "ok" : failed;
})();
