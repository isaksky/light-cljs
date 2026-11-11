function arraySortWait(dep) {
  // Array.prototype.sort across a dependency wait (light-cljs-910): `dep` is
  // a host effect the test answers with "D", called in the comparator's
  // third call, above the sort frame in the middle of a merge pass, in
  // place or after the job's whole stack was saved as a continuation and
  // restored in another heap (snapshot_tests.goose), so the frame's two
  // scratch arrays, run width and cursors travel in it. No comparison is
  // made twice. Completes with "D:true".
  let d = "";
  let calls = 0;
  const a = [{ k: 4 }, { k: 2 }, { k: 3 }, { k: 1 }, { k: 5 }];
  a.sort(function (x, y) { calls++; if (calls === 3) { d = dep(); } return x.k - y.k; });
  let ordered = true;
  for (let i = 1; i < a.length; i++) { if (a[i - 1].k > a[i].k) { ordered = false; } }
  return d + ":" + (ordered && a.length === 5 && calls >= 4 && calls <= 10);
}
