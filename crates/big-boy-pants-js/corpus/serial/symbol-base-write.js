function symbolBaseWrite() {
  // A property write on a Symbol base fails by name (light-cljs-833), as a
  // read does (symbol-base-read.js); the catch never runs.
  const s = Symbol.iterator;
  try { s.x = 1; } catch (e) { return 1; }
  return 2;
}
