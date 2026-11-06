// bbp-authored conformance case.
// Derives from: none (light-cljs-805: `+` on an object whose valueOf, or inherited toString, is behind a getter runs the getter with the
// object as `this`, then calls the method it returned; a getter giving a non-callable skips that method). Completes with "42|b2!|s".
(function () {
  var o = { n: 41, get valueOf() { var self = this; return function () { return self.n + 1; }; } };
  var base = { get toString() { return function () { return "b" + this.k; }; } };
  var heir = { __proto__: base, k: "2", valueOf: 0 };
  var skip = { get valueOf() { return 5; }, toString: function () { return "s"; } };
  return (o + 0 === 42 ? "42" : "wrong") + "|" + (heir + "!") + "|" + (skip + "");
})();
