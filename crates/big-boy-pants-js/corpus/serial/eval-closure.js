// A function compiled at run time (light-cljs-856): an indirect eval
// declares a counter of its own and makes a closure over it, which the job
// calls once and returns. snapshot_tests.goose runs it in a reused worker,
// carries the closure in a captured snapshot to another program, and checks
// that the worker's reset drops the code it compiled. The eval source's
// comment holds a lone surrogate, which the code's source bytes keep as
// WTF-8 (light-cljs-864), so the snapshot's validation must admit it.
function evalClosure() {
  var bump = (0, eval)("let made = 0; (function bump() { /*\ud800*/ return ++made; })");
  bump();
  return bump;
}
