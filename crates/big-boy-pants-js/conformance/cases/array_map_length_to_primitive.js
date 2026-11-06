// bbp-authored conformance case.
// Derives from: none (light-cljs-789 review: map's length converts through @@toPrimitive with hint number before valueOf;
// the Goose VM does not run @@toPrimitive yet and faults by name rather than call valueOf). Completes with "1:a".
(function () {
  var len = { valueOf: function () { return 2; } };
  len[Symbol.toPrimitive] = function (hint) { return hint === "number" ? 1 : 5; };
  var calls = "";
  var m = Array.prototype.map.call({ 0: "a", 1: "b", length: len }, function (v) { calls += v; return v; });
  return m.length + ":" + calls;
})();
