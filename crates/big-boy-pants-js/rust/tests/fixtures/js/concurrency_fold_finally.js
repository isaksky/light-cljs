// Synthetic fixture, not light-cljs output.
// A resumable native fold whose callback suspends while a `finally` block is
// pending (tests/concurrency, cross-feature). `defs` is a vector of
// definition ids: for each, the callback needs that definition's result
// kind 2 (a shared object published later) and appends it to the
// accumulator, so the whole task suspends inside the callback, inside the
// fold, inside the `try`. The cleanup effect (host operation 3) must run
// exactly once, after the fold completed; operation 1 is the pre-fold
// effect and must never repeat.
function fold_under_finally(defs, tag) {
  let calls = 0;
  let result = 0;
  try {
    __rt.effect(1, tag);
    result = __rt.reduce(function (acc, def) {
      calls = calls + 1;
      return __rt.conj(acc, __rt.need(def, 2));
    }, __rt.vector(), defs);
  } finally {
    __rt.effect(3, calls);
  }
  return result;
}
