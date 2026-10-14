// Node side of `cargo bench --bench compiler -- --levels` (benches/support/levels.rs).
// One fresh process per sample; the program file is read before timing starts.
//
//   node compiler_levels.js startup PROGRAM.js
//     Compiles PROGRAM.js with vm.Script (parse) and runs it once (execute), then
//     prints its value the way the size ratchet does (a function prints as
//     #function) after whatever the program printed. Node's lazy stdout setup
//     happens before timing.
//
//   node compiler_levels.js steady PROGRAM.js INPUT WARMUP_CALLS CALLS BUDGET_NS
//     PROGRAM.js evaluates to a one-argument function, or stores it in
//     globalThis.levelsSteady as the upstream ClojureScript programs do. Calls it up
//     to WARMUP_CALLS times untimed and then up to CALLS times in one timed batch,
//     all with INPUT. Each phase makes at least one call and stops early when one
//     more call, at the phase's average so far, would take it past BUDGET_NS.
//
// The timings go to stderr as the last line, tab-separated after a LEVELS tag:
// parse and execute nanoseconds for startup; batch nanoseconds, timed calls and the
// result for steady. The caller checks stdout and the result.
"use strict";
const fs = require("node:fs");
const vm = require("node:vm");

const [mode, file, ...rest] = process.argv.slice(2);
const source = fs.readFileSync(file, "utf8");
const now = process.hrtime.bigint;
const report = (...fields) => process.stderr.write(["LEVELS", ...fields].join("\t") + "\n");

if (mode === "startup") {
  // Node sets up stdout and its value formatter on first use, about 5 ms that
  // would otherwise land in the first program that prints. Do it untimed.
  process.stdout.write("");
  require("node:util").inspect([0, ""]);
  const start = now();
  const script = new vm.Script(source, { filename: file });
  const parsed = now();
  const value = script.runInThisContext();
  const executed = now();
  console.log(typeof value === "function" ? "#function" : value);
  report(parsed - start, executed - parsed);
} else if (mode === "steady") {
  const [input, warmups, calls, budget] = rest.map(Number);
  const value = new vm.Script(source, { filename: file }).runInThisContext();
  const run = globalThis.levelsSteady ?? value;
  if (typeof run !== "function") throw new TypeError("a steady-state program must evaluate to a function");
  const fits = (made, elapsed) => made === 0 || elapsed + elapsed / made <= budget;
  let result;
  const warm = now();
  for (let made = 0; made < warmups && fits(made, Number(now() - warm)); made++) result = run(input);
  const results = [];
  let elapsed = 0;
  const start = now();
  while (results.length < calls && fits(results.length, elapsed)) {
    results.push(run(input));
    elapsed = Number(now() - start);
  }
  for (const value of results) {
    if (!Object.is(value, results[0])) throw new Error(`steady-state results differ: ${results[0]} then ${value}`);
  }
  report(elapsed, results.length, results.length > 0 ? results[0] : result);
} else {
  throw new Error(`unknown mode ${mode}`);
}
