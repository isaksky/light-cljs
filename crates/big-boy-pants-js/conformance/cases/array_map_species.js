// bbp-authored conformance case.
// Derives from: none (light-cljs-789: map on an array whose constructor has a
// custom @@species, inside a try/catch; until species is implemented the Goose VM faults by name, which no catch intercepts).
(function () {
  var a = [1, 2];
  function Species(n) { this.made = n; }
  var C = function () {};
  C[Symbol.species] = Species;
  a.constructor = C;
  var result = "none";
  try { var m = a.map(function (v) { return v * 2; }); result = m.made + ":" + m[0] + m[1]; } catch (e) {}
  return result;
})();
