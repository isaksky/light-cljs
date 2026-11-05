// Synthetic fixture, not light-cljs output.
// Baseline loop (examples/bench): the loop shape of the lookup fixtures with
// the intrinsic replaced by arithmetic, so the loop's own cost can be
// subtracted from theirs.
function baseline(c, n) {
  let i = 0;
  let s = 0;
  while (i < n) {
    s = s + i % c;
    i = i + 1;
  }
  return s;
}
