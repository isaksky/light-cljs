// for-in (light-cljs-804): own keys in OrdinaryOwnPropertyKeys order (indices
// ascending, then names in creation order), then each prototype's, read when
// the walk reaches it; a name an object nearer the operand had is not
// visited again; a key deleted before it is reached is skipped, one added is
// not visited; a deleted prototype key is never listed. Array holes, string
// indices, primitives, break, continue and a `var` head.
function forIn() {
  const a = { x: 1, shared: 1, gone: 1, 2: "a2" };
  const b = { __proto__: a, y: 2, shared: 2, 1: "b1" };
  const c = { __proto__: b, z: 3, shared: 3, late: 3, 0: "c0" };
  let seen = "";
  for (const k in c) {
    seen += k + ",";
    if (k === "z") { delete c.late; delete a.gone; c.added = 1; }
  }
  const h = [5, 6, 7]; delete h[1];
  let holes = "";
  for (const i in h) holes += i;
  let chars = "";
  for (const i in "ab") chars += i;
  let none = 0;
  for (const k in null) none++;
  for (const k in undefined) none++;
  for (const k in 5) none++;
  let picked = "";
  for (const k in { p: 1, q: 2, r: 3 }) { if (k === "r") break; if (k === "p") continue; picked += k; }
  var last;
  for (last in { first: 1, last: 2 }) {}
  return seen === "0,z,shared,1,y,2,x," && holes === "02" && chars === "01" && none === 0 && picked === "q" && last === "last" && c.added === 1;
}
