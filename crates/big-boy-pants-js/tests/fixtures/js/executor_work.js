// Synthetic fixture, not light-cljs output.
// Unrelated useful work for the executor tests (tests/executor.rs): runs on a
// worker while consumers are parked. Sums the squares of 1..20 and completes
// with 2870; with a small budget it spans several runs.
let i = 1;
let sum = 0;
while (i <= 20) {
  sum = sum + i * i;
  i = i + 1;
}
sum;
