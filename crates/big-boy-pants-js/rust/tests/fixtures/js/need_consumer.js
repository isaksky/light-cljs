// Synthetic fixture, not light-cljs output.
// A dependency consumer for tests/need.rs. Host operation 1 is the
// observable pre-need effect; host operation 2 reports a value to the host.
// Definition 5 / kind 3 is a number; definition 6 / kind 2 is a callable
// published by a producer task. Either need may suspend the whole task.
function consumer(base) {
  let lookups = 0;
  const pre = __rt.effect(1, base);
  function lookup(scale) {
    lookups = lookups + 1;
    // `scale * 10` is a live operand of the inner frame across the wait.
    return scale * 10 + __rt.need(5, 3);
  }
  const value = lookup(base);
  __rt.effect(2, value);
  const helper = __rt.need(6, 2);
  __rt.effect(2, lookups);
  return pre + helper(value);
}
