// bbp-authored conformance case.
// Derives from: ~/src/quickjs/tests/test_closure.js lines 155-159 (test_with's
// direct eval of a variable reference). Executes a direct eval, which the Goose
// VM does not implement: it fails Unsupported naming `direct_eval`
// (light-cljs-823), never a JavaScript error. Completes with 42 on the oracles.
(function () {
  var x = 41;
  function read() {
    return eval("x + 1");
  }
  return read();
})();
