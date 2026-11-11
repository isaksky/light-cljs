function arrayCallbackMethodsWait(dep) {
  // Array.prototype.filter across a dependency wait (light-cljs-908): `dep`
  // is a host effect the test answers with "D", called in filter's callback
  // above its ArrayCallbacks frame after one element was kept, in place or
  // after the job's whole stack was saved as a continuation and restored in
  // another heap (snapshot_tests.goose), so the frame's result, the element
  // being judged and the next result index travel in it. The callback runs
  // once per element: nothing is replayed. Completes with "D:true".
  let d = "";
  let calls = 0;
  const a = [{ x: 1 }, { x: 2 }, { x: 3 }];
  const kept = a.filter(function (v, i) { calls++; if (i === 1) { d = dep(); } return v.x !== 3; });
  const total = a.reduce(function (acc, v) { return acc + v.x; }, 0);
  return d + ":" + (kept.length === 2 && kept[0] === a[0] && kept[1] === a[1] && calls === 3 && total === 6);
}
