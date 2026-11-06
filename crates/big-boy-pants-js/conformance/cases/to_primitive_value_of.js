// bbp-authored conformance case.
// Derives from: none (light-cljs-786: `+` on an object runs its own valueOf,
// or its toString when valueOf gives an object, in a resumable native
// frame). Completes with "42 s!".
(function () {
  var o = { valueOf: function () { return 41; } };
  var s = { valueOf: function () { return {}; }, toString: function () { return "s"; } };
  return (o + 1 === 42 ? "42" : "x") + " " + (s + "!");
})();
