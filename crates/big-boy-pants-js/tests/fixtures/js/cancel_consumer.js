// Synthetic fixture, not light-cljs output.
// A consumer's work for the cancellation tests (tests/cancellation.rs): a
// loop long enough to be cut into several budgeted runs. It sums 0..99 and
// completes with 4950 when nothing cancels it.
let i = 0;
let sum = 0;
while (i < 100) {
  sum = sum + i;
  i = i + 1;
}
sum;
