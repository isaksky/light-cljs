// Synthetic fixture, not light-cljs output.
// Persistent map lookup (examples/bench): n reads cycling through the
// number keys 0 .. count(m) - 1 of m.
function map_get(m, n) {
  const c = __rt.count(m);
  let i = 0;
  let s = 0;
  while (i < n) {
    s = s + __rt.get(m, i % c);
    i = i + 1;
  }
  return s;
}
