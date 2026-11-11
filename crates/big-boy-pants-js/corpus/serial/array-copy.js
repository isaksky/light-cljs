// The Array methods that copy (light-cljs-911) run in resumable native
// frames, every HasProperty, Get, call, definition and Set its own step:
// slice with valueOf conversions over holes and a getter, concat over a
// proxy whose has trap hides an index, a spreadable array-like and a
// species constructor behind getters, flat over nested arrays, holes and a
// proxy with a depth, toReversed and with over an array-like with a getter,
// Array.of, and Array.from over a user iterator that maps, one that is
// closed when the map function throws, one whose next throws (not closed),
// an array-like and a string, and the dense fast paths.
function arrayCopy() {
  let log = "";
  const holes = [1, , 3, 4];
  Object.defineProperty(holes, 2, { get: function () { log += "g"; return 30; }, configurable: true });
  const sliced = holes.slice({ valueOf: function () { log += "s"; return 1; } }, { valueOf: function () { log += "e"; return -1; } });
  const hidden = new Proxy([1, 2, 3], { has: function (t, k) { return k !== "1" && k in t; } });
  const joined = [0].concat(hidden, { length: 2, 1: "y", [Symbol.isConcatSpreadable]: true }, [[9]]);
  const source = [1, 2, 3];
  Object.defineProperty(source, "constructor", { get: function () {
    log += "c";
    return { get [Symbol.species]() { log += "p"; return function (n) { log += "n" + n; this.made = n; }; } };
  } });
  const species = source.slice(1);
  const flat = [1, [2, , [3, [4]]], hidden].flat(2);
  const like = { length: 3, 0: "a", get 1() { log += "l"; return "b"; }, 2: "c" };
  const reversed = Array.prototype.toReversed.call(like);
  const replaced = Array.prototype.with.call(like, -1, "z");
  const of = Array.of(7, 8, 9);
  let closed = "";
  function numbers(limit, fail) {
    return { [Symbol.iterator]: function () {
      let i = 0;
      return { next: function () { if (fail) { throw "next"; } i++; return { done: i > limit, value: i }; }, return: function () { closed += "r"; return {}; } };
    } };
  }
  const mapped = Array.from(numbers(3), function (v, i) { return v * this.k + i; }, { k: 10 });
  let thrown = "";
  try { Array.from(numbers(3), function (v) { if (v === 2) { throw "map"; } return v; }); } catch (e) { thrown += e; }
  try { Array.from(numbers(3, true)); } catch (e) { thrown += e; }
  const fromLike = Array.from({ length: 2, 0: "p", 1: "q" }, function (v, i) { return v + i; });
  const fromString = Array.from("ab");
  const dense = [1, 2, 3, 4];
  const fast = [dense.slice(1, 3).join(""), dense.concat([5], 6).join(""), dense.toReversed().join(""), dense.with(0, 9).join(""), [[1], [2, [3]]].flat().length].join();
  let rangeError = false;
  try { [1].with(5, 0); } catch (e) { rangeError = e.name === "RangeError"; }
  return log === "segcpn2ll" && sliced.length === 2 && !(0 in sliced) && sliced[1] === 30 &&
    joined.length === 7 && joined[1] === 1 && !(2 in joined) && joined[3] === 3 && !(4 in joined) && joined[5] === "y" && joined[6][0] === 9 &&
    species.made === 2 && species[0] === 2 && species[1] === 3 && species.length === 2 &&
    flat.join() === "1,2,3,4,1,3" && reversed.join() === "c,b,a" && replaced.join() === "a,b,z" && of.join() === "7,8,9" &&
    mapped.join() === "10,21,32" && thrown === "mapnext" && closed === "r" && fromLike.join() === "p0,q1" && fromString.join() === "a,b" &&
    fast === "23,123456,4321,9234,3" && rangeError;
}
