// Deletion (light-cljs-787): `delete` removes an own property, turning a
// shaped object into dictionary mode, keeps the other properties and their
// order of creation, and is true for a missing property. Deleting an own
// property uncovers an inherited one. An array element becomes a hole and
// the length stays.
function deletes() {
  const p = { a: "inherited" };
  const o = { __proto__: p, a: 1, b: 2, c: 3 };
  const d1 = delete o.b;
  const d2 = delete o.missing;
  o.d = 4;
  const kept = o.c + o.d === 7 && !("b" in o) && o.b === undefined;
  const d3 = delete o.a;
  const arr = [1, 2, 3];
  const d4 = delete arr[1];
  return d1 && d2 && d3 && d4 && kept && o.a === "inherited" && arr.length === 3 && !(1 in arr) && arr[2] === 3;
}
