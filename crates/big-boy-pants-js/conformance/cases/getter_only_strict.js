// bbp-authored conformance case.
// Derives from: none (light-cljs-787: in strict code, assigning to an
// inherited accessor without a setter is a TypeError, which a catch block
// receives as a value). Completes with "refused 1".
(function () {
  "use strict";
  var base = { get x() { return 1; } };
  var o = { __proto__: base };
  var caught = "accepted";
  try { o.x = 2; } catch (e) { caught = "refused"; }
  return caught + (o.x === 1 ? " 1" : " ?");
})();
