// Attributes in strict code (light-cljs-787): assigning to an accessor
// property without a setter is refused, a TypeError in strict code, even
// when the getter is inherited.
function getterOnly() {
  "use strict";
  const base = { get x() { return 1; } };
  const o = { __proto__: base };
  o.x = 2;
  return o.x;
}
