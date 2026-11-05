// Synthetic fixture, not light-cljs output.
// Persistent vector update (examples/bench): n appends, each making a new
// version that shares structure with the previous one.
function vec_conj(n) {
  let v = __rt.vector();
  let i = 0;
  while (i < n) {
    v = __rt.conj(v, i);
    i = i + 1;
  }
  return v;
}
