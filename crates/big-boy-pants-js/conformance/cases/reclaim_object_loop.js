// bbp-authored conformance case.
// Derives from: none (light-cljs-840's finding, light-cljs-847: a loop that
// allocates an object per iteration and keeps only the last one; 150,000
// objects pass the Goose VM's 100,000-node job limit, so the job reclaims
// its unreachable nodes while it runs). Completes with "149999,300000".
(function () {
  let o;
  for (let i = 0; i < 150000; i++) o = {i};
  let pairs = 0;
  for (let i = 0; i < 150000; i++) { const p = [i, {j: i}]; pairs += p.length; }
  return o.i + "," + pairs;
})();
