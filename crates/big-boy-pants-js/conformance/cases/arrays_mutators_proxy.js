// bbp-authored conformance case (light-cljs-909).
// Derives from: none (light-cljs-909: the mutators over a proxy with every trap; the Pants engine refuses the set trap, so this slice is scored against QuickJS)
// (shift and reverse through a proxy: the get, has, set and deleteProperty traps and the length's read and write, in the specification's order). Completes with "ok", or the number of the first failed check.
(function () {
  var failed = 0;
  function check(n, ok) { if (!ok && failed === 0) { failed = n; } }
  check(1, (function () { let log = ''; let p = new Proxy([1, 2, 3], {get(t, k, r) { log += 'g' + String(k) + ';'; return Reflect.get(t, k, r); }, set(t, k, v, r) { log += 's' + String(k) + ';'; return Reflect.set(t, k, v, r); }, has(t, k) { log += 'h' + String(k) + ';'; return k in t; }, deleteProperty(t, k) { log += 'd' + String(k) + ';'; return delete t[k]; }}); let r = Array.prototype.shift.call(p); return (r === 1 && log === 'glength;g0;h1;g1;s0;h2;g2;s1;d2;slength;'); })());
  check(2, (function () { let log = ''; let o = {length: 3, 0: 'a', 2: 'c'}; let p = new Proxy(o, {get(t, k, r) { log += 'g' + String(k) + ';'; return Reflect.get(t, k, r); }, set(t, k, v, r) { log += 's' + String(k) + ';'; return Reflect.set(t, k, v, r); }, has(t, k) { log += 'h' + String(k) + ';'; return k in t; }, deleteProperty(t, k) { log += 'd' + String(k) + ';'; return delete t[k]; }}); Array.prototype.reverse.call(p); return (log === 'glength;h0;g0;h2;g2;s0;s2;' && o[0] === 'c' && o[2] === 'a'); })());
  return failed === 0 ? "ok" : failed;
})();
