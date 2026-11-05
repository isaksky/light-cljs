// Synthetic fixture, not light-cljs output.
// A consumer of the memory measurement (examples/bench) that suspends inside
// a resumable native operation: `__rt.reduce` over a vector of the shared
// input calls a callback that needs a pending dependency, so the parked task
// keeps a native continuation and the callback's frame. It returns the
// input's entry unchanged when the fold saw every element.
function consume_fold(form, def, index) {
  const entry = __rt.nth(form, index);
  const total = __rt.reduce(function (acc, x) {
    return acc + x * __rt.need(def, 1);
  }, 0, __rt.nth(entry, 2));
  if (total === 3 * index + 3) {
    return entry;
  }
  return null;
}
