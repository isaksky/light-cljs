// Synthetic fixture, not light-cljs output.
// Deep recursion that waits at the bottom (tests/concurrency, cross-feature):
// `down` recurses `depth` times, then needs the callable result (kind 2) of
// definition 71, `is_even`, and calls it; `is_even` and `is_odd` (definition
// 72) are published by producer tasks and call each other through
// `__rt.need`, so the task may suspend again several frames deeper.
// Host operation 1 is the pre-wait effect.
function descend(depth, n) {
  __rt.effect(1, depth);
  function down(k) {
    if (k === 0) {
      const even = __rt.need(71, 2);
      return even(n);
    }
    return 1 + down(k - 1);
  }
  return down(depth);
}
