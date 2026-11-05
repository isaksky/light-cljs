// Synthetic fixture, not light-cljs output.
// Persistent map update (examples/bench): n associations of the number keys
// 0 .. n - 1, each making a new version that shares structure with the
// previous one.
function map_assoc(n) {
  let m = __rt.map();
  let i = 0;
  while (i < n) {
    m = __rt.assoc(m, i, i);
    i = i + 1;
  }
  return m;
}
