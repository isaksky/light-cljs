// Prototype chains (light-cljs-787): reads follow a three-object chain set by
// literal `__proto__`, an own property shadows an inherited one, a write to
// the prototype is visible through the chain, an assignment creates an own
// property on the receiver, `in` sees inherited properties, and a
// `__proto__: null` object inherits nothing, not even `toString`.
function prototypes() {
  const a = { x: 1, shared: "a" };
  const b = { __proto__: a, y: 2 };
  const c = { __proto__: b, z: 3, shared: "c" };
  const reads = c.x + c.y + c.z === 6 && c.shared === "c" && b.shared === "a";
  a.x = 10;
  c.y = 20;
  const writes = c.x === 10 && c.y === 20 && b.y === 2;
  const has = "x" in c && "z" in c && !("w" in c) && 0 in [5] && !(1 in [5]);
  const bare = { __proto__: null, q: 1 };
  return reads && writes && has && bare.q === 1 && bare.toString === undefined && c.w === undefined;
}
