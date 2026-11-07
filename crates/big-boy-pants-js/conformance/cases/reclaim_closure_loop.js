// bbp-authored conformance case.
// Derives from: none (light-cljs-847: closures made in a loop, each over
// that iteration's `let` binding and a block's `const`, of which only the
// last is kept. Every iteration allocates a closure and its captured
// binding cells; 150,000 iterations pass the Goose VM's 100,000-node job
// limit, so the job reclaims the unreachable ones while it runs).
// Completes with "449997,33749775000".
(function () {
  let f, sum = 0;
  for (let i = 0; i < 150000; i++) {
    const k = i * 2;
    f = () => k + i;
    sum += f();
  }
  return f() + "," + sum;
})();
