// bbp-authored conformance case.
// Derives from: none (light-cljs-787: [[Get]] through a three-object chain set
// by literal `__proto__`, shadowing, a write on the prototype seen through
// the chain, an assignment creating an own property, `in`, and a null
// prototype). Completes with 10 + 20 + 2 = 32 and "c a yes no none".
(function () {
  var a = { x: 1, shared: "a" };
  var b = { __proto__: a, y: 2 };
  var c = { __proto__: b, z: 3, shared: "c" };
  a.x = 10;
  c.y = 20;
  var bare = { __proto__: null };
  var sum = c.x + c.y + b.y;
  return (sum === 32 ? "32 " : "? ") + c.shared + " " + b.shared + ("x" in c ? " yes" : " no") + ("w" in c ? " yes" : " no") + (bare.toString === undefined ? " none" : " some");
})();
