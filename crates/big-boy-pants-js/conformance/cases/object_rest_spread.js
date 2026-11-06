// bbp-authored conformance case.
// Derives from: none (light-cljs-804: object rest takes the remaining own
// enumerable keys in own-key order, excluding a computed key; spread copies
// own enumerable properties in order, later ones overwriting, skips null and
// undefined, and runs a getter once, defining a data property). A string
// source is left out: QuickJS at 7d1c13c copies no indices from it, where
// the Pants engine and V8 do (corpus/serial/object-rest.js covers it).
// Completes with "1,3,b,c,|3,a,c,|1,3,a,b,c,z,|ten|yes once".
(function () {
  var src = { a: 1, b: 2, 3: "three", c: 3, 1: "one" };
  var { a, ...others } = src;
  var keys = ""; for (var k in others) keys += k + ",";
  var name = "b";
  var { [name]: bee, 1: one, ...more } = src;
  var moreKeys = ""; for (var m in more) moreKeys += m + ",";
  var merged = { ...src, a: "ten", ...null, ...undefined, ...{ z: 26 } };
  var mergedKeys = ""; for (var j in merged) mergedKeys += j + ",";
  var runs = 0;
  var g = { get v() { runs++; return 42; }, w: 1 };
  var copy = { ...g };
  return keys + "|" + moreKeys + "|" + mergedKeys + "|" + merged.a + "|" + (copy.v === 42 ? "yes" : "no") + (runs === 1 ? " once" : " again");
})();
