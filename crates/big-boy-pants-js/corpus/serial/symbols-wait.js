function symbolsWait(dep) {
  // Symbols across a dependency wait (light-cljs-885): `dep` is a host
  // effect the test answers with "D", in place or after the job's whole
  // stack was saved as a continuation and restored in another heap
  // (snapshot_tests.goose). A unique symbol key, a registered symbol and a
  // wrapper made before the wait keep their identity, and the registry its
  // entries, after it. Completes with "D:true".
  const u = Symbol('u');
  const r = Symbol.for('r');
  const o = {[u]: 1};
  const w = Object(u);
  const d = dep();
  return d + ':' + (o[u] === 1 && Symbol.for('r') === r && Symbol.keyFor(r) === 'r' && w.valueOf() === u && u.description === 'u' && Symbol.for('q') !== r && Symbol.keyFor(u) === undefined);
}
