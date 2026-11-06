// bbp-authored conformance case.
// Derives from: ~/src/quickjs/tests/test_loop.js lines 254-263
// (test_try_catch1): its body inside a function the script calls, the
// caught value returned instead of asserted. Completes with "hello".
(function () {
  try {
    throw "hello";
  } catch (e) {
    return e;
  }
})();
