// Array.prototype.sort and toSorted (light-cljs-910) as a resumable stable
// merge sort: a user comparator and its valueOf result, the default order
// over objects' toString and numbers, undefineds and holes, an array-like
// with accessors, stability, toSorted's copy, a throwing comparator and
// the comparator check, each HasProperty, Get, comparison, conversion, Set
// and delete its own step (serial_tests.goose cuts and collects at every
// one). The checks do not depend on the order comparisons are made in.
function arraySort() {
  const numbers = [5, 3, 9, 1, 7, 2, 8];
  numbers.sort(function (a, b) { return { valueOf: function () { return a - b; } }; });
  const words = [{ toString: function () { return "pear"; } }, "apple", { toString: function () { return "fig"; } }, 10, 9];
  words.sort();
  const sparse = [3, undefined, , 1, , 2];
  sparse.sort();
  let sets = "";
  const like = { length: 4, get 0() { return "d"; }, set 0(v) { sets += "0" + v; }, 1: "b", get 3() { return "a"; }, set 3(v) { sets += "3" + v; } };
  Array.prototype.sort.call(like);
  const records = [];
  for (let i = 0; i < 23; i++) { records.push({ k: (i * 13) % 5, i: i }); }
  const stable = records.toSorted(function (a, b) { return a.k - b.k; });
  let ordered = stable.length === 23 && records[0].i === 0 && records[22].i === 22;
  for (let i = 1; i < 23; i++) { if (stable[i - 1].k > stable[i].k || stable[i - 1].k === stable[i].k && stable[i - 1].i > stable[i].i) { ordered = false; } }
  const holes = [2, , 1].toSorted();
  let thrown = 0, kept = [3, 2, 1];
  try { kept.sort(function () { throw 42; }); } catch (e) { thrown = e; }
  let typeError = false;
  try { [1].sort(5); } catch (e) { typeError = e.name === "TypeError"; }
  return numbers.join() === "1,2,3,5,7,8,9" && words.join() === "10,9,apple,fig,pear" &&
    sparse.length === 6 && sparse[0] === 1 && sparse[2] === 3 && sparse[3] === undefined && (3 in sparse) && !(4 in sparse) && !(5 in sparse) &&
    sets === "0a" && like[1] === "b" && like[2] === "d" && !(3 in like) &&
    ordered && holes.length === 3 && holes[0] === 1 && holes[1] === 2 && holes[2] === undefined && (2 in holes) &&
    thrown === 42 && kept.join() === "3,2,1" && typeError;
}
