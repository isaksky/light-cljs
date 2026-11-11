// Array.prototype.toSpliced (light-cljs-909), which the Pants engine lacks:
// its copy over holes, getters read once in order, valueOf conversions,
// inserted items and object-valued elements, through the units (an
// array-like) and the dense fast path, at every budget cut and collection
// (serial_tests.goose). Completes with true.
function arrayToSpliced() {
  let log = "";
  const a = { x: 1 }, b = { x: 2 }, c = { x: 3 };
  const like = { length: 4, get 0() { log += "a"; return a; }, 1: b, get 3() { log += "d"; return c; } };
  const copy = Array.prototype.toSpliced.call(like, { valueOf: function () { log += "s"; return 1; } }, { valueOf: function () { log += "k"; return 1; } }, { y: 9 }, "z");
  const holes = [1, , 3].toSpliced(0, 0);
  const dense = [a, b, c].toSpliced(-2, 1, { y: 8 });
  const whole = [1, 2, 3].toSpliced();
  return log === "skad" && copy.length === 5 && copy[0] === a && copy[1].y === 9 && copy[2] === "z" && copy[3] === undefined && (3 in copy) && copy[4] === c &&
    holes.length === 3 && (1 in holes) && holes[1] === undefined && dense.length === 3 && dense[0] === a && dense[1].y === 8 && dense[2] === c && whole.join() === "1,2,3";
}
