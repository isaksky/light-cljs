// bbp-authored conformance case.
// Derives from: ~/src/quickjs/tests/test_loop.js lines 336-354
// (test_try_catch7): its body inside a function the script calls, `s`
// returned instead of asserted. Completes with "tfag".
(function () {
  var s;
  s = "";

  try {
    try {
      s += "t";
      throw "a";
    } finally {
      s += "f";
    }
  } catch(e) {
    s += e;
  } finally {
    s += "g";
  }
  return s;
})();
