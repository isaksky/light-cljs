// bbp-authored conformance case.
// Derives from: ~/src/quickjs/tests/test_loop.js lines 265-274
// (test_try_catch2): its body inside a function the script calls, `a`
// returned instead of asserted. Completes with 1.
(function () {
  var a;
  try {
    a = 1;
  } catch (e) {
    a = 2;
  }
  return a;
})();
