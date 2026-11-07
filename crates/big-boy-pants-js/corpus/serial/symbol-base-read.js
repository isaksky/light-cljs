function symbolBaseRead() {
  // A property read on a Symbol base fails by name (light-cljs-833):
  // %Symbol.prototype% is the objects area's (light-cljs-766). The fault is
  // not an exception, so the catch below never runs.
  const s = Symbol.iterator;
  try { return s["x"]; } catch (e) { return 1; }
}
