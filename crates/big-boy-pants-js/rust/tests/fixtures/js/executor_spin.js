// Synthetic fixture, not light-cljs output.
// A task that never finishes on its own, for the executor's cancellation and
// shutdown tests (tests/executor.rs): only budgets bound each of its runs.
let n = 0;
while (true) {
  n = n + 1;
}
