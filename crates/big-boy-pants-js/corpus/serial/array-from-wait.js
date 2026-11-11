function arrayFromWait(dep) {
  // Array.from across a dependency wait (light-cljs-911): `dep` is a host
  // effect the test answers with "D", called in the map function for the
  // second item, above the Array.from frame while it holds its Catch
  // handler, in place or after the job's whole stack was saved as a
  // continuation and restored in another heap (snapshot_tests.goose), so
  // the frame's iterator record, result, index, item and handler travel in
  // it. `next` and the map function run once an item: nothing is replayed.
  // A later map function that throws still closes its iterator. Completes
  // with "D:true".
  let d = "";
  let log = "";
  function numbers(limit) {
    return { [Symbol.iterator]: function () {
      let i = 0;
      return { next: function () { log += "n"; i++; return { done: i > limit, value: i }; }, return: function () { log += "r"; return {}; } };
    } };
  }
  const a = Array.from(numbers(3), function (v, i) { log += "m"; if (i === 1) { d = dep(); } return v * 2; });
  let caught = 0;
  try { Array.from(numbers(3), function (v) { if (v === 2) { throw 42; } return v; }); } catch (e) { caught = e; }
  // A definition that throws closes the iterator too (a custom constructor
  // whose result is a proxy; the Pants engine refuses both).
  let defined = 0;
  function Refusing() { return new Proxy({}, { defineProperty: function () { throw 17; } }); }
  try { Array.from.call(Refusing, numbers(2)); } catch (e) { defined = e; }
  const copy = [1, [2, [3]]].flat(Infinity).concat(a.slice(1)).toReversed();
  return d + ":" + (a.join() === "2,4,6" && caught === 42 && defined === 17 && log === "nmnmnmnnnrnr" && copy.join() === "6,4,3,2,1");
}
