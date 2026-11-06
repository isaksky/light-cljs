// Call depth (light-cljs-786): the Pants engine's limit of 10,000 frames,
// entry frame included, and a catchable overflow past it. f(9998) runs
// 9,999 frames of f above this entry frame.
function depth() {
  function f(n) { return n === 0 ? 0 : 1 + f(n - 1); }
  let caught = false;
  try { f(1000000); } catch (e) { caught = true; }
  return f(9998) === 9998 && caught;
}
