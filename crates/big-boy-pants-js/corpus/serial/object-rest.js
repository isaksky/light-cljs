// Object rest and spread (light-cljs-804): rest takes the remaining own
// enumerable keys in own-key order, a computed key excluded once converted;
// spread copies own enumerable properties in that order, later ones
// overwriting, skips null and undefined, copies a string's indices, and runs
// a getter once, as a data property in the copy; an exception from a getter
// leaves the spread. A getter that deletes a later key and adds one: the
// listed keys stay, the deleted one is skipped, the added one not copied.
function objectRest() {
  const src = { a: 1, b: 2, 3: "three", c: 3, 1: "one" };
  const { a, ...others } = src;
  let keys = "";
  for (const k in others) keys += k + ",";
  const name = "b";
  const { [name]: bee, 1: one, ...more } = src;
  let moreKeys = "";
  for (const k in more) moreKeys += k + ",";
  const merged = { ...src, a: 10, ...null, ...undefined, ...{ z: 26 }, ..."hi" };
  let mergedKeys = "";
  for (const k in merged) mergedKeys += k + ",";
  let runs = 0;
  const g = { get v() { runs++; return 42; }, w: 1 };
  const copy = { ...g };
  const { v, ...restOfG } = g;
  let restKeys = "";
  for (const k in restOfG) restKeys += k + ",";
  const spreadSource = { get a() { delete this.b; this.c = 3; return 1; }, b: 2 };
  const spreadMut = { ...spreadSource };
  let spreadMutKeys = "";
  for (const k in spreadMut) spreadMutKeys += k + ",";
  const restSource = { get a() { delete this.b; this.c = 3; return 1; }, b: 2 };
  const { ...restMut } = restSource;
  let restMutKeys = "";
  for (const k in restMut) restMutKeys += k + ",";
  let caught = "";
  try { const bad = { ...{ ok: 1, get boom() { throw "boom"; } } }; caught = "no"; } catch (e) { caught = e; }
  return a === 1 && keys === "1,3,b,c," && bee === 2 && one === "one" && moreKeys === "3,a,c,"
    && mergedKeys === "0,1,3,a,b,c,z," && merged[1] === "i" && merged.a === 10 && merged[0] === "h"
    && copy.v === 42 && v === 42 && runs === 2 && restKeys === "w," && caught === "boom"
    && spreadMutKeys === "a," && restMutKeys === "a," && spreadSource.c === 3 && restMut.a === 1;
}
