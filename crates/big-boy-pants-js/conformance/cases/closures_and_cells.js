// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_functions.rs lines 1496-1700, 1942-1976:
// closures_see_assignments_and_keep_bindings_alive (line 1496, bbp-20),
// captured_assignment_expressions_return_the_assigned_value (1942, bbp-21),
// loop_iterations_allocate_distinct_captured_cells (1963, bbp-21),
// closures_check_the_temporal_dead_zone_at_run_time (1568, bbp-94) and
// closures_over_immutable_bindings_capture_values_and_are_publishable
// (1619, bbp-94). The `__rt.vector`/`__rt.nth` pair of the first test is an
// array here. The Pants tests' error positions and messages and the
// publication and foreign-binding checks (Pants runtime API, no script
// counterpart) are left out; errors are compared by name. Each row is a
// `row(...)` check; the script completes with the number of rows, or minus
// the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// Two closures share one binding.
{
  function pair() {
    let v = 1;
    return [function (x) { v = x; }, function () { return v; }];
  }
  const p = pair();
  p[0](41);
  row(p[1]() + 1 === 42);
}
// A closure sees assignments made after it was created, in the frame and in
// other closures, and the frame sees the closure's.
{
  let a = 1;
  const get = function () { return a; };
  a = 5;
  const set = function (x) { a = x; };
  set(get() * 2);
  row(a === 10);
}
// Captured parameters are bindings too.
{
  function acc(total) {
    return function (x) { total = total + x; return total; };
  }
  const f = acc(100);
  f(1);
  row(f(2) === 103);
}
// Deep nesting: a binding three functions out.
{
  function a(x) {
    return function b(y) {
      return function c(z) { x = x + 1; return x * 100 + y * 10 + z; };
    };
  }
  const c = a(1)(2);
  c(3);
  row(c(3) === 323);
}
// An assignment expression's value is the assigned value, in the binding's
// own frame and through a closure.
{
  let x = 0;
  function set(v) { return x = v; }
  const r = set(42);
  const s = (x = 7);
  row(r * 1000 + s * 10 + x === 42077);
}
{
  function f(p) { const g = function () { return p = p + 1; }; return g() * 10 + p; }
  row(f(4) === 55);
}
// Each loop iteration's `let` is a separate binding.
{
  let i = 0;
  let first, last;
  while (i < 2) {
    let j = i;
    const bump = function () { j = j + 1; return j; };
    if (i === 0) { first = bump; }
    last = bump;
    i = i + 1;
  }
  row(first() * 100 + last() === 102);
}
// The temporal dead zone is checked when a closure runs.
{
  function g() { return x; }
  const e = name(g);
  let x = 1;
  row(e === "ReferenceError" && g() === 1);
}
{
  const f = function () { return c; };
  const e = name(f);
  const c = 1;
  row(e === "ReferenceError" && f() === 1);
}
// Assigning through a closure before initialization is a ReferenceError,
// even for a `const` (before its TypeError).
{
  function s() { y = 2; }
  const e = name(s);
  let y;
  row(e === "ReferenceError");
}
// At script level: in a block, QuickJS throws a TypeError here (the
// specification's SetMutableBinding and the Pants engine: ReferenceError), so
// that variant is not ported.
function setLateConst() { lateConst = 2; }
const lateConstError = name(setLateConst);
const lateConst = 1;
row(lateConstError === "ReferenceError" && lateConst === 1);
{
  const c = 1;
  function s() { c = 2; }
  row(name(s) === "TypeError" && c === 1);
}
// Inside its own function, position decides.
{
  function f() { let a = b; let b = 1; }
  row(name(f) === "ReferenceError");
}
// A closure in its own initializer reads it later, initialized.
{
  let f = function () { return f; };
  row(f() === f);
}
// Mutual recursion between hoisted declarations.
{
  function even(n) { return n === 0 || odd(n - 1); }
  function odd(n) { return n !== 0 && even(n - 1); }
  row(even(6) === true && odd(7) === true && even(5) === false);
}
// Closures over immutable bindings, and over an assigned one, called from
// another function.
{
  const k = 3;
  const scale = function (n) { return function (x) { return x * n * k; }; };
  const f = scale(2);
  function apply(g) { return g(5); }
  row(apply(f) === 30);
}
{
  let n = 0;
  function inc() { n = n + 1; return n; }
  function apply(g) { return g(5); }
  row(apply(inc) === 1 && apply(inc) === 2 && n === 2);
}
failed === 0 ? rows : -failed;
