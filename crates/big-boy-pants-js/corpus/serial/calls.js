// Original source-derived VM regression, not emitted macro code.
function calls() {
  function add(a, b) { return a + b; }
  function twice(f, n) { return f(n, f(n, 1)); }
  function missing(a) { return a === undefined; }
  return twice(add, 3) === 7 && missing() && add(2, 3, 99) === 5;
}
