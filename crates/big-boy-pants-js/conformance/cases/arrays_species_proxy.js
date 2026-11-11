// bbp-authored conformance case (light-cljs-908).
// Derives from: none (light-cljs-908: ArraySpeciesCreate results that are proxies and a redefined Array[@@species]; the Pants engine refuses the defineProperty trap and a definition on a built-in, so this slice is scored against QuickJS)
// (CreateDataPropertyOrThrow of map, filter and flatMap into a species result that is a proxy: the defineProperty trap's key and complete descriptor, in order, a false answer's TypeError, and Array[@@species] redefined by the program). Completes with "ok", or the number of the first failed check.
(function () {
  var failed = 0;
  function check(n, ok) { if (!ok && failed === 0) { failed = n; } }
  check(1, (function () { let log = ''; let a = [1, 2, 3]; a.constructor = {[Symbol.species]: function () { return new Proxy({}, {defineProperty(t, k, d) { log += k + d.value + d.writable + d.enumerable + d.configurable + ';'; return Reflect.defineProperty(t, k, d); }}); }}; let r = a.filter(v => v > 1); let m = a.map(v => v); let f = a.flatMap(v => [v]); return (log === '02truetruetrue;13truetruetrue;01truetruetrue;12truetruetrue;23truetruetrue;01truetruetrue;12truetruetrue;23truetruetrue;' && r[0] === 2 && m[2] === 3 && f[1] === 2); })());
  check(3, (function () { let n = 0; let d = Object.getOwnPropertyDescriptor(Array, Symbol.species); Object.defineProperty(Array, Symbol.species, {get() { n++; return function () { return {}; }; }, configurable: true}); let r; try { r = [Array.isArray([1].filter(() => true)), Array.isArray([1].map(v => v)), Array.isArray([1].flatMap(v => v))].join(); } finally { Object.defineProperty(Array, Symbol.species, d); } return (r === 'false,false,false' && n === 3 && Array.isArray([1].map(v => v))); })());
  check(2, (function () { let n = 0; let a = [1]; a.constructor = {[Symbol.species]: function () { return new Proxy({}, {defineProperty() { return false; }}); }}; for (let method of ['map', 'filter', 'flatMap']) { try { a[method](v => true); } catch (e) { if (e instanceof TypeError) n++; } } return (n === 3); })());
  return failed === 0 ? "ok" : failed;
})();
