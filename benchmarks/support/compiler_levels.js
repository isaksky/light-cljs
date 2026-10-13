// Node side of `cargo bench --bench compiler -- --levels` (benches/support/levels.rs).
// One fresh process per sample; the program file is read before timing starts.
//
//   node compiler_levels.js startup PROGRAM.js
//     Compiles PROGRAM.js with vm.Script (parse) and runs it once (execute), then
//     prints its value the way the size ratchet does (a function prints as
//     #function) after whatever the program printed. Node's lazy stdout setup
//     happens before timing.
//
//   node compiler_levels.js steady PROGRAM.js INPUT WARMUP_CALLS CALLS
//     PROGRAM.js evaluates to a one-argument function. Calls it WARMUP_CALLS times
//     untimed and then CALLS times in one timed batch, all with INPUT.
//
// The timings go to stderr as the last line, tab-separated after a LEVELS tag:
// parse and execute nanoseconds for startup, batch nanoseconds and the last
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
  const [input, warmups, calls] = rest.map(Number);
  const run = new vm.Script(source, { filename: file }).runInThisContext();
  if (typeof run !== "function") throw new TypeError("a steady-state program must evaluate to a function");
  let result;
  for (let call = 0; call < warmups; call++) result = run(input);
  const results = new Array(calls);
  const start = now();
  for (let call = 0; call < calls; call++) results[call] = run(input);
  const elapsed = now() - start;
  for (const value of results) {
    if (!Object.is(value, results[0])) throw new Error(`steady-state results differ: ${results[0]} then ${value}`);
  }
  report(elapsed, calls > 0 ? results[0] : result);
} else {
  throw new Error(`unknown mode ${mode}`);
}
