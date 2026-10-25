// Synthetic fixture, not light-cljs output.
// The other half of the pair in sim_is_even.js: refers to `isEven`. A
// returned number is a tail call of `isEven` (simulated-host convention).
function isOdd(n) {
  if (n === 0) {
    return false;
  }
  return n - 1;
}
