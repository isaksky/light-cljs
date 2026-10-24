// Synthetic fixture, not light-cljs output.
// The fold of concurrency_fold_finally.js running inside a `finally` block
// while the `try` block's `return tag` is pending (tests/concurrency,
// cross-feature): the callback suspends with a pending return completion
// on the handler stack. Host operation 1 is the pre-wait effect; operation 3
// reports how many results the fold collected.
function fold_in_finally(defs, tag) {
  try {
    __rt.effect(1, tag);
    return tag;
  } finally {
    const got = __rt.reduce(function (acc, def) {
      return __rt.conj(acc, __rt.need(def, 2));
    }, __rt.vector(), defs);
    __rt.effect(3, __rt.count(got));
  }
}
