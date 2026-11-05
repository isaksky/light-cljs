// Synthetic fixture, not light-cljs output.
// Unrelated useful work for the end-to-end scenario (examples/e2e), run on
// the workers while both consumers wait: builds a persistent vector of the
// squares 1..n (every intermediate version becomes garbage for the forced
// collection) and returns their sum.
function squares(n) {
  let v = __rt.vector();
  let i = 1;
  while (i <= n) {
    v = __rt.conj(v, i * i);
    i = i + 1;
  }
  let sum = 0;
  let j = 0;
  while (j < __rt.count(v)) {
    sum = sum + __rt.nth(v, j);
    j = j + 1;
  }
  return sum;
}
