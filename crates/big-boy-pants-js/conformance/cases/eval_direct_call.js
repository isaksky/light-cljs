// bbp-authored conformance case.
// Derives from: ~/src/quickjs/tests/test_closure.js lines 155-159 (test_with's
// direct eval of a variable reference). Executes a direct eval in the caller's
// scope (light-cljs-858; light-cljs-823 failed it by name). Completes with 42.
(function () {
  var x = 41;
  function read() {
    return eval("x + 1");
  }
  return read();
})();
