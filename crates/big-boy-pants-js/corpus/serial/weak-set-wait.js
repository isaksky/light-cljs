function weakSetWait(dep) {
  // new WeakSet(iterable) across a dependency wait (light-cljs-945), as
  // weak-collections-wait.js waits for a WeakMap: `dep` is a host effect
  // the test answers with "D", called in the iterator's `next` above the
  // WeakSet's Collection frame after one value was stored, in place or
  // after the job's whole stack was saved as a continuation and restored
  // in another heap (snapshot_tests.goose); the values stored before the
  // wait answer after it, a job object, a job symbol and a well-known
  // symbol, and nothing is replayed. The job registers a symbol with
  // Symbol.for first, which the test puts in the saved WeakSet's run to see
  // the snapshot refused. Completes with "D:true".
  let d = "";
  let i = 0;
  const r = Symbol.for("weak-set-wait");
  const a = {}, b = Symbol("b");
  const items = [a, b, a, Symbol.iterator];
  const it = {
    [Symbol.iterator]() { return this; },
    next() { const k = i++; if (k === 1) { d = dep(); } return { done: k >= items.length, value: items[k] }; },
  };
  const s = new WeakSet(it);
  const ok = s.has(a) && s.has(b) && s.has(Symbol.iterator) && !s.has(r) && !s.has({}) && i === 5;
  return d + ":" + ok;
}
