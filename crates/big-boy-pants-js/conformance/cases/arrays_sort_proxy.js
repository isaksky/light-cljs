// bbp-authored conformance case (light-cljs-910).
// Derives from: none (light-cljs-910: sort through a proxy with every trap; the Pants engine refuses the set trap, so this slice is scored against QuickJS)
// (sort through a proxy: the length read, has and get of every index, then set of the sorted values and deleteProperty of the hole's index, in the specification's order). Completes with "ok", or the number of the first failed check.
(function () {
  var failed = 0;
  function check(n, ok) { if (!ok && failed === 0) { failed = n; } }
  check(1, (function () { let log = ''; let t = [3, , 1]; let p = new Proxy(t, {get(t, k, r) { log += 'g' + String(k) + ';'; return Reflect.get(t, k, r); }, set(t, k, v, r) { log += 's' + String(k) + ';'; return Reflect.set(t, k, v, r); }, has(t, k) { log += 'h' + String(k) + ';'; return k in t; }, deleteProperty(t, k) { log += 'd' + String(k) + ';'; return delete t[k]; }}); Array.prototype.sort.call(p); return (log === 'glength;h0;g0;h1;h2;g2;s0;s1;d2;' && t[0] === 1 && t[1] === 3 && !(2 in t)); })());
  return failed === 0 ? "ok" : failed;
})();
