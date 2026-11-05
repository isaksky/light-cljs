// Synthetic fixture, not light-cljs output.
// A dependency consumer for the concurrency suite (tests/concurrency): a
// pre-wait effect that must never run twice, a test-controlled gate just
// before the need, the need of result `kind` of definition `def` in the
// task's world, and one effect after the wait that reads a local built
// before it. Host operations: 1 = PRE, 2 = GATE (blocks while the test
// holds gate `tag`), 3 = POST.
function consume(tag, def, kind) {
  const kept = __rt.vector(tag, def, kind);
  __rt.effect(1, tag);
  __rt.effect(2, tag);
  const value = __rt.need(def, kind);
  __rt.effect(3, __rt.nth(kept, 0));
  return value;
}
