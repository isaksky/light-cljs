function mapSetConstructionWait(dep) {
  // new Map(iterable) across a dependency wait (light-cljs-943): `dep` is a
  // host effect the test answers with "D", called in the iterator's `next`
  // above the Collection frame after one entry was stored, in place or
  // after the job's whole stack was saved as a continuation and restored in
  // another heap (snapshot_tests.goose), so the frame's collection, adder
  // and iterator record travel in it and the entries stored before the wait
  // answer after it, keyed by a job object and a symbol. `next` runs once
  // per entry and once more: nothing is replayed. Completes with "D:true".
  let d = "";
  let i = 0;
  const a = {}, b = Symbol("b");
  const items = [[a, 1], [b, 2], [a, 3], ["s", 4]];
  const it = {
    [Symbol.iterator]() { return this; },
    next() { const k = i++; if (k === 1) { d = dep(); } return { done: k >= items.length, value: items[k] }; },
  };
  const m = new Map(it);
  const ok = m.size === 3 && m.get(a) === 3 && m.get(b) === 2 && m.get("s") === 4 && i === 5 && new Set([a, b, a]).size === 2;
  return d + ":" + ok;
}
