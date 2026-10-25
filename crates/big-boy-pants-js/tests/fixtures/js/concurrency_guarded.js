// Synthetic fixture, not light-cljs output.
// A consumer whose need is guarded by catch and finally (tests/concurrency,
// cross-feature): a failed dependency is caught at the need (outcome 2); a
// consumer cancellation skips the catch block but runs the finally block.
// Host operations: 1 = PRE (pre-wait, never twice), 3 = POST (the cleanup
// in finally, exactly once however the try ends), 4 = NOTE (the outcome,
// reached only when the task is not cancelled; also 20 on entering catch).
function guarded(tag, def, kind) {
  let outcome = 0;
  try {
    __rt.effect(1, tag);
    __rt.need(def, kind);
    outcome = 1;
  } catch (e) {
    outcome = 2;
    __rt.effect(4, 20);
  } finally {
    __rt.effect(3, tag);
  }
  __rt.effect(4, outcome);
  return outcome;
}
