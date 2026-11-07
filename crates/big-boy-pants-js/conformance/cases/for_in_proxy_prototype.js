// bbp-authored conformance case.
// Derives from: none (light-cljs-845: ECMAScript 10.5.1 [[GetPrototypeOf]]
// and 14.7.5.9 EnumerateObjectProperties; the Pants engine has no
// getPrototypeOf trap, so QuickJS alone scores it). A for-in walk that has
// used up a proxy's own keys takes the next object from its getPrototypeOf
// trap; without a trap, from its target; a result that is neither an
// object nor null is a TypeError. Completes with "az|ac|TypeError".
(function () {
  var out = [];
  var s = "";
  for (var k in new Proxy({ a: 1 }, { getPrototypeOf: function () { return { z: 1 }; } })) s += k;
  out.push(s);
  s = "";
  for (var k2 in new Proxy(new Proxy({ __proto__: { c: 3 }, a: 1 }, {}), {})) s += k2;
  out.push(s);
  try { for (var k3 in new Proxy({}, { getPrototypeOf: function () { return 1; } })) {} out.push("none"); } catch (e) { out.push(e.name); }
  return out.join("|");
})();
