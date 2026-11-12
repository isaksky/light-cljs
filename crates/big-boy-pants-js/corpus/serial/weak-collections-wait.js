function weakCollectionsWait(dep) {
  // new WeakMap(iterable) across a dependency wait (light-cljs-945): `dep`
  // is a host effect the test answers with "D", called in the iterator's
  // `next` above the Collection frame after one entry was stored, in place
  // or after the job's whole stack was saved as a continuation and
  // restored in another heap (snapshot_tests.goose), so the frame's
  // WeakMap, adder and iterator record travel in it and the entries stored
  // before the wait answer after it, keyed by job objects and a symbol.
  // `next` runs once per entry and once more: nothing is replayed. The job
  // registers a symbol with Symbol.for first, which the test puts in the
  // saved WeakMap's run to see the snapshot refused. Completes with "D:true".
  let d = "";
  let i = 0;
  const r = Symbol.for("weak-collections-wait");
  const a = {}, b = Symbol("b"), c = {};
  const items = [[a, 1], [b, 2], [a, 3], [c, 4]];
  const it = {
    [Symbol.iterator]() { return this; },
    next() { const k = i++; if (k === 1) { d = dep(); } return { done: k >= items.length, value: items[k] }; },
  };
  const m = new WeakMap(it);
  const ok = m.get(a) === 3 && m.get(b) === 2 && m.get(c) === 4 && i === 5 && new WeakSet([a, b, a]).has(b) && !m.has({}) && !m.has(r);
  return d + ":" + ok;
}
