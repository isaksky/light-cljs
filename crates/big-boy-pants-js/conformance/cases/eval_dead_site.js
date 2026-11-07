// bbp-authored conformance case.
// Derives from: ~/src/quickjs/tests/test_closure.js lines 120-131 (test_arrow_function's
// f1 and f3: an arrow whose body is a direct eval, in a function next to one
// without). Here the direct-eval function is never called (light-cljs-823: a
// program holding a direct eval exports, and only a direct eval that runs
// fails). Completes with 2.
(function () {
  function f1() {
    return (() => arguments)();
  }
  function f3() {
    return (() => eval("this"))();
  }
  var a = f1(1, 2);
  return typeof f3 === "function" ? a.length : -1;
})();
