function symbolBaseWrite() {
  'use strict';
  // A property write on a Symbol base finds no setter on %Symbol.prototype%
  // (light-cljs-885), so strict code throws a TypeError.
  const s = Symbol('x');
  try { s.x = 1; } catch (e) { return e instanceof TypeError ? 1 : 3; }
  return 2;
}
