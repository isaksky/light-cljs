// bbp-authored conformance case (strict script).
// Derives from: ~/src/quickjs/tests/test_closure.js lines 176-198
// (test_eval_closure) and crates/big-boy-pants-js/rust/tests/js_source.rs
// lines 2192-2193 (strict_direct_eval's loop rows). Functions a direct eval
// makes in a `for (let ...)` body capture that iteration's binding, and see
// later writes to it. Each row is a `row(...)` check; the script completes
// with the number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
// Each row runs as one script: `run` as eval code of a direct eval in a
// fresh function, so its declarations stay its own; `script` through an
// indirect eval, as global code.
function run(source) { return eval(source); }
function script(source) { return (0, eval)(source); }
row(run("let fs = []; for (let i = 0; i < 3; i++) fs.push(eval('() => i')); fs[0]() === 0 && fs[1]() === 1 && fs[2]() === 2") === true);
row(run("let fs = []; for (let i = 0; i < 3; i++) { function f() { return eval('() => i'); } fs.push(f()); } fs[0]() === 0 && fs[1]() === 1 && fs[2]() === 2") === true);
// Direct sites of this file's own code.
{
  const fs = [];
  for (let i = 0; i < 3; i++) { fs.push(eval("() => i")); }
  row(fs[0]() === 0 && fs[1]() === 1 && fs[2]() === 2);
  const gs = [];
  for (let i = 0; i < 3; i++) { gs.push(eval("(function () { return i; })")); i++; }
  row(gs.length === 2 && gs[0]() === 1 && gs[1]() === 3);
  const hs = [];
  for (let i of [5, 6]) { let j = i * 2; hs.push(eval("() => i + j")); }
  row(hs[0]() === 15 && hs[1]() === 18);
}
failed === 0 ? rows : -failed;
