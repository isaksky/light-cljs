// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/fixtures/js/arithmetic.js lines 4-9.
// Those lines, unchanged, inside a function the script calls, so the script
// declares no global let/const bindings. Completes with 27.5.
(function () {
  let a = 6, b = 4;
  const c = a * b - a / b;   // 24 - 1.5 = 22.5
  let d = (a + b) % 3;       // 1
  let e = -(c - 2.5) / 4;    // -5
  a = a - +b;                // 2
  return c + d * 10 + e + a * 0;   // 22.5 + 10 - 5 + 0
})();
