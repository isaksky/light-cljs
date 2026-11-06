// bbp-authored conformance case.
// Derives from: none (light-cljs-787: object literal getters and setters, own
// and inherited through `__proto__`, run with the original receiver; getter
// and setter halves merge; an assignment yields its right-hand side).
// Completes with 4 + 10 + 7 + 14 + 5 = 40.
(function () {
  var base = {
    get v() { return this.k * 2; },
    set v(x) { this.k = x; return 99; },
  };
  var own = { k: 3, get g() { return this.k + 1; } };
  var child = { __proto__: base, k: 5 };
  var merged = { get m() { return this.n; }, n: 1, set m(x) { this.n = x + 1; } };
  var first = own.g + child.v;
  var assigned = (child.v = 7);
  merged.m = 4;
  return first + assigned + child.v + merged.m;
})();
