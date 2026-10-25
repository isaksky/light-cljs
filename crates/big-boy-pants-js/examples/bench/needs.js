// Synthetic fixture, not light-cljs output.
// Dependency reads (examples/bench): needs result kind 1 of definitions
// first, first + step, ... (n needs) and sums the results. With step 0 and
// a completed cell every need is an already-ready read; with step 1 and
// pending cells every need suspends the task until the host publishes.
function needs(first, step, n) {
  let i = 0;
  let s = 0;
  while (i < n) {
    s = s + __rt.need(first + i * step, 1);
    i = i + 1;
  }
  return s;
}
