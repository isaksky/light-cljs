// Synthetic fixture, not light-cljs output.
// A consumer task of the end-to-end compiler-host scenario (examples/e2e):
// shaped like an emitted expansion step. It inspects a form it shares with
// another consumer, asks the host for the CALLABLE result (kind 2) of
// definition 7, a helper that a producer task compiles later, and returns
// the subtree the helper selects, unchanged. The need sits in a try block
// with a catch and a finally: if the producer fails, the catch block gets
// the dependency error and returns it; the finally block runs once, after
// the wait (waiting is not unwinding), or as cancellation cleanup.
//
// Host operations: 1 = PRE (pre-wait work; must never run twice),
// 2 = MEET (returns only once the other consumer is inside its own MEET),
// 3 = POST (first effect after the wait), 4 = REPORT (a number computed
// from task-owned state kept across the wait), 5 = CAUGHT (the catch block
// observed a failed dependency), 6 = CLEANUP (the finally block).
function expand(form, tag, depth) {
  const seen = __rt.vectorBuilder();
  const note = function (x) {
    __rt.builderConj(seen, x);
    return x;
  };
  const head = note(__rt.nth(form, 0));
  const arity = note(__rt.count(__rt.nth(form, 1)));
  __rt.effect(1, tag);
  __rt.effect(2, tag);
  const scratch = __rt.vector(head, arity, tag);
  let select;
  try {
    select = __rt.need(7, 2);
  } catch (e) {
    __rt.effect(5, tag);
    return e;
  } finally {
    __rt.effect(6, tag);
  }
  __rt.effect(3, __rt.nth(scratch, 2));
  note(__rt.count(scratch));
  const subtree = select(form, depth);
  __rt.effect(4, __rt.count(__rt.freeze(seen)) * 100 + __rt.nth(scratch, 0) * 10 + __rt.nth(scratch, 1));
  return subtree;
}
