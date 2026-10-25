// Synthetic fixture, not light-cljs output.
// A consumer of the memory measurement (examples/bench): it inspects the
// shared input form, keeps task-owned state (a transient builder) across a
// dependency wait, and returns one subtree of the input unchanged.
function consume(form, def, index) {
  const seen = __rt.vectorBuilder();
  __rt.builderConj(seen, __rt.count(form));
  const picked = __rt.nth(form, index);
  const helper = __rt.need(def, 1);
  __rt.builderConj(seen, helper);
  if (__rt.count(__rt.freeze(seen)) === 2) {
    return picked;
  }
  return null;
}
