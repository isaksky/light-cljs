// bbp-authored conformance case.
// Derives from: ~/src/quickjs/tests/test_loop.js lines 305-319
// (test_try_catch5): its body inside a function the script calls, `s`
// returned instead of asserted. Completes with "tf".
(function () {
  var s;
  s = "";
  for(;;) {
    try {
      s += "t";
      break;
      s += "b";
    } finally {
      s += "f";
    }
  }
  return s;
})();
