// Synthetic fixture, not light-cljs output.
// Persistent vector lookup (examples/bench): n reads cycling through v.
function vec_nth(v, n) {
  const c = __rt.count(v);
  let i = 0;
  let s = 0;
  while (i < n) {
    s = s + __rt.nth(v, i % c);
    i = i + 1;
  }
  return s;
}
