function mathSumPreciseWait(dep) {
  // Math.sumPrecise across a dependency wait (light-cljs-901): `dep` is a
  // host effect the test answers with "D", called in the iterator's `next`
  // above the SumPrecise frame after two elements were added, in place or
  // after the job's whole stack was saved as a continuation and restored in
  // another heap (snapshot_tests.goose), so the frame's iterator record and
  // exact sum travel in it. Completes with "D:true".
  let d = "";
  let i = 0;
  const items = [1e20, 0.1, -1e20];
  const it = {
    [Symbol.iterator]() { return this; },
    next() { const k = i++; if (k === 2) { d = dep(); } return { done: k >= items.length, value: items[k] }; },
  };
  const ok = Math.sumPrecise(it) === 0.1 && i === 4;
  return d + ":" + ok;
}
