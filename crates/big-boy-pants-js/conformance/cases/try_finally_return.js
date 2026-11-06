// bbp-authored conformance case.
// Derives from: ~/src/quickjs/tests/test_loop.js lines 321-334
// (test_try_catch6): its body inside a function the script calls, the
// result and `s` returned instead of asserted. Completes with "1tf".
(function () {
  function f() {
    try {
      s += 't';
      return 1;
    } finally {
      s += "f";
    }
  }
  var s = "";
  var r = f();
  return (r === 1 ? "1" : "x") + s;
})();
