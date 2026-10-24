// Synthetic fixture, not light-cljs output.
// A producer task's result for tests/concurrency: a parity callable that
// refers to its partner through `__rt.need` when called (a recursive
// reference, not a wait at production time). `me` is 0 for `is_even`
// (definition 71, partner 72) and 1 for `is_odd` (definition 72, partner
// 71); each returns 1 for true and 0 for false.
function produce_parity(me) {
  if (me === 0) {
    return function is_even(x) {
      if (x === 0) {
        return 1;
      }
      const odd = __rt.need(72, 2);
      return odd(x - 1);
    };
  }
  return function is_odd(x) {
    if (x === 0) {
      return 0;
    }
    const even = __rt.need(71, 2);
    return even(x - 1);
  };
}
