// bbp-authored conformance case.
// Derives from: none (light-cljs-787: `delete` removes an own property, moving
// a shaped object to dictionary mode; the rest keep their values, a missing
// property deletes as true, deleting an own property uncovers an inherited
// one, and an array element becomes a hole). Completes with 1 + 2 + 4 + 8 +
// 16 + 32 = 63.
(function () {
  var p = { a: "inherited" };
  var o = { __proto__: p, a: 1, b: 2, c: 3 };
  var score = 0;
  if (delete o.b && !("b" in o)) score += 1;
  if (delete o.missing) score += 2;
  o.d = 4;
  if (o.c + o.d === 7) score += 4;
  if (delete o.a && o.a === "inherited") score += 8;
  var arr = [1, 2, 3];
  if (delete arr[1] && !(1 in arr)) score += 16;
  if (arr.length === 3 && arr[2] === 3) score += 32;
  return score;
})();
