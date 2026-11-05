// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/fixtures/js/closures.js lines 4-30.
// Those lines inside a function the script calls (no global let/const
// bindings); the final expression is returned. Completes with 131070.
(function () {
  function makeCounter(start) {
    let count = start;
    function step(by) {
      count = count + by;
      return count;
    }
    return step;                        // escapes; `count` outlives the call
  }

  const c1 = makeCounter(10);
  const c2 = makeCounter(100);
  c1(1);
  c1(2);                                // c1's count: 13
  c2(5);                                // c2's count: 105 (separate binding)

  // Each iteration's `j` is a new binding; `first` keeps iteration 0's.
  let i = 0;
  let first, last;
  while (i < 3) {
    let j = i * 10;
    const get = function () { return j; };
    if (i === 0) { first = get; }
    last = get;
    i = i + 1;
  }

  return c1(0) * 10000 + c2(0) * 10 + first() + last();   // 130000 + 1050 + 0 + 20
})();
