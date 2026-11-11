// The Array.prototype mutators (light-cljs-909) run in
// resumable native frames, every Get, HasProperty, Set and delete its own
// step: shift and unshift over holes, getters and setters, splice over a
// proxy whose has trap hides an index and through a species constructor
// behind getters, reverse and copyWithin over an array-like with accessors,
// fill and splice with valueOf conversions, pop of an array-like, the dense
// fast paths, and a frozen array's TypeError. (toSpliced, which the Pants
// engine lacks, is in array-mutators-wait.js.)
function arrayMutators() {
  let log = "";
  const holes = [1, , 3, 4];
  Object.defineProperty(holes, 2, { get: function () { log += "g"; return 30; }, set: function (v) { log += "s" + v; }, configurable: true });
  const shifted = holes.shift();
  const up = [, 2];
  const upLength = up.unshift(1);
  const target = [1, 2, 3, 4];
  const hidden = new Proxy(target, { has: function (t, k) { return k !== "1" && k in t; } });
  const removed = Array.prototype.splice.call(hidden, 0, 2, "x");
  const source = [1, 2, 3];
  Object.defineProperty(source, "constructor", { get: function () {
    log += "c";
    return { get [Symbol.species]() { log += "p"; return function (n) { log += "n" + n; this.made = n; }; } };
  } });
  const spliced = source.splice({ valueOf: function () { log += "a"; return 1; } }, { valueOf: function () { log += "d"; return 1; } }, 8, 9);
  const pairs = { length: 3, get 0() { log += "x"; return "a"; }, set 0(v) { log += "X" + v; }, 2: "c" };
  Array.prototype.reverse.call(pairs);
  const within = { length: 4, 0: "a", 1: "b", get 3() { log += "w"; return "d"; }, set 3(v) { log += "W" + v; } };
  Array.prototype.copyWithin.call(within, 2, 0);
  const filled = [1, 2, 3, 4].fill(7, { valueOf: function () { log += "f"; return -3; } }, 3);
  const like = { length: 2, 0: "p", 1: "q" };
  const popped = Array.prototype.pop.call(like);
  const dense = [1, 2, 3, 4, 5];
  const fast = [dense.pop(), dense.shift(), dense.unshift(0), dense.splice(1, 2, "x").join(""), dense.join(""), dense.reverse().join(""), [1, 2, 3, 4, 5].copyWithin(0, 3).join("")].join();
  let typeError = false;
  try { Object.freeze([1]).pop(); } catch (e) { typeError = e.name === "TypeError"; }
  return log === "gs4adcpn1xXcWbf" && shifted === 1 && holes.length === 3 && !(0 in holes) && holes[1] === 30 && holes[2] === 30 && upLength === 3 && up[0] === 1 && !(1 in up) && up[2] === 2 &&
    removed.length === 2 && removed[0] === 1 && !(1 in removed) && target.join() === "x,3,4" &&
    spliced.made === 1 && spliced[0] === 2 && source.join() === "1,8,9,3" &&
    pairs[2] === "a" && within[2] === "a" && filled.join() === "1,7,7,4" && popped === "q" && like.length === 1 && !(1 in like) &&
    fast === "5,1,4,23,0x4,4x0,45345" && typeError;
}
