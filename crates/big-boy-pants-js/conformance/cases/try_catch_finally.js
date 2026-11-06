// bbp-authored conformance case.
// Derives from: ~/src/quickjs/tests/test_loop.js lines 276-303
// (test_try_catch3 and test_try_catch4): their bodies inside a function the
// script calls, the two strings joined and returned instead of asserted.
// Completes with "tf tcf".
(function () {
  var s;
  s = "";
  try {
    s += "t";
  } catch (e) {
    s += "c";
  } finally {
    s += "f";
  }
  var r = s;
  s = "";
  try {
    s += "t";
    throw "c";
  } catch (e) {
    s += e;
  } finally {
    s += "f";
  }
  return r + " " + s;
})();
