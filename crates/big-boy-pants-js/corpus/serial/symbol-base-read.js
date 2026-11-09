function symbolBaseRead() {
  // Property reads and method calls on a Symbol base go through
  // %Symbol.prototype% (light-cljs-885), as on numbers and strings
  // (light-cljs-833); a missing property reads undefined.
  const s = Symbol('x');
  return s.description + s.toString() + String(s["x"]);
}
