// Synthetic fixture, not light-cljs output.
// A resumable native fold for tests/native_fold.rs. `__rt.reduce` calls
// `step` for each element of `items`. The callback captures the live local
// `seen`, reports each element to the host (operation 2, the observable
// effect) and calls the helper published as definition 7 / kind 2, which is
// unavailable at first: the whole task suspends inside the callback, inside
// the native fold. Host operation 1 is the pre-fold effect; operation 3
// reports how many callbacks ran.
function fold_consumer(items, scale) {
  let seen = 0;
  const pre = __rt.effect(1, scale);
  function step(acc, x) {
    seen = seen + 1;
    __rt.effect(2, x);
    const helper = __rt.need(7, 2);
    return acc + helper(x * scale);
  }
  const total = __rt.reduce(step, pre, items);
  __rt.effect(3, seen);
  return total;
}
