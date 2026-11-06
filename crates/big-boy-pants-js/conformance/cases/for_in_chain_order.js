// bbp-authored conformance case.
// Derives from: none (light-cljs-804: for-in visits own keys in
// OrdinaryOwnPropertyKeys order, indices first, then each prototype's keys
// read when the walk reaches them; a name shadowed nearer the operand is not
// visited again; a key deleted during the loop before it is reached is
// skipped and one added is not visited). Completes with
// "0,z,shared,1,y,2,x,".
(function () {
  var a = { x: 1, shared: 1, gone: 1, 2: "a2" };
  var b = { __proto__: a, y: 2, shared: 2, 1: "b1" };
  var c = { __proto__: b, z: 3, shared: 3, late: 3, 0: "c0" };
  var seen = "";
  for (var k in c) {
    seen += k + ",";
    if (k === "z") { delete c.late; delete a.gone; c.added = 1; }
  }
  return seen;
})();
