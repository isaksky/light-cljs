// bbp-authored conformance case.
// Derives from: none (light-cljs-788: an error object JavaScript makes and does not catch is the thrown
// value, named by its prototype's `name`). Throws a TypeError.
(function () {
  throw TypeError("not caught");
})();
