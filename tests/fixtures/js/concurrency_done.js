// Synthetic fixture, not light-cljs output.
// The VM part of a task whose need the concurrency suite's driver issues
// through the run guard: it records one NOTE effect (host operation 4) per
// run that reaches it, then completes.
function done(tag) {
  __rt.effect(4, tag);
  return tag;
}
