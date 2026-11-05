// Synthetic fixture, not light-cljs output.
// A native fold whose callback needs a different dependency for every
// element (definition `x`, kind 3), so the task suspends once per element,
// for tests/native_fold.rs. Host operation 2 reports each element before its
// need; the accumulator is a persistent vector built with `conj`.
function fold_needs(defs) {
  function step(acc, x) {
    __rt.effect(2, x);
    return __rt.conj(acc, __rt.need(x, 3) + x);
  }
  return __rt.reduce(step, __rt.vector(), defs);
}
