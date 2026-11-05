// Synthetic fixture, not light-cljs output.
// Entry function of examples/resumable_counter.rs
// (`cargo run --example resumable_counter`). Counts to `n`, bumping a host
// counter once per step through the reserved effect intrinsic
// `__rt.effect(0, i)` (host operation 0) inside a closure, and returns the
// last counter value the host reported.
function count(n) {
  let last;
  const bump = function (i) {
    return __rt.effect(0, i);
  };
  let i = 0;
  while (i < n) {
    last = bump(i);
    i = i + 1;
  }
  return last;
}
