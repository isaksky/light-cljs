// Runs a dev build (docs/dev-workflow.md, "The dev build") or a single-file
// build in Node and prints its outcome in the shared protocol of
// execution.js, so a dev build and its `--optimize none` oracle compare
// through this one driver. Usage:
//
//   node tests/support/dev_execution.mjs BUILD [--start] [UNIT ...]
//
// BUILD is a dev build directory, which holds manifest.json, or a file holding
// the JavaScript expression of a single-file build. The driver loads the
// build, running a dev build's files as scripts in manifest order, and takes
// the program's value: the value of the dev build's entry namespace, or of the
// expression. With --start, a function value is the application's start
// function, which the driver calls with execution.js's `observe` (awaiting a
// returned promise); its result is the outcome value.
//
// Each UNIT is a file holding a unit of the dev build, such as the unit of the
// same namespace in a build of edited sources. The driver applies them with
// the hook protocol: the manifest's before-load functions in reverse
// definition order, the units in order, then the after-load functions in
// order, each found in the registry when it runs. An async hook gets a
// continuation, and the driver waits until it is called. A failing unit or
// hook ends the run as the outcome, and later units stay unapplied.
//
// The driver then waits until nothing is pending (microtasks, timers) and
// prints one record with id "program": the stdout of the whole run, the
// `observe` trace and the outcome. Language values are read through the
// runtime the build loaded (`inspect`), which the driver exposes from a
// single-file build too; a function outcome value is recorded as the string
// "#function". A program that never settles fails the run instead.
import { readFileSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { runInThisContext } from "node:vm";

const args = process.argv.slice(2);
if (args.length === 0) {
  throw new Error("usage: node dev_execution.mjs BUILD [--start] [UNIT ...]");
}
const build = resolve(args[0]);
const start = args.includes("--start");
const units = args.slice(1).filter(arg => arg !== "--start").map(unit => resolve(unit));
const dev = statSync(build).isDirectory();
const manifest = dev ? JSON.parse(readFileSync(join(build, "manifest.json"), "utf8")) : null;
if (!dev && units.length > 0) throw new Error("units apply only to a dev build");

const script = file => runInThisContext(readFileSync(file, "utf8"), { filename: file });
const hook = ({ name, async }) => {
  const run = globalThis.__lightApplicationRoots[name];
  return async ? new Promise(done => run(done)) : run();
};

// Node empties the microtask queue before every macrotask, and emits
// beforeExit only once no timer or I/O request is left.
const idle = () => new Promise(done => process.once("beforeExit", done));

// Capture stdout as execution.js does, but across the whole asynchronous run.
const stdout = [];
const write = process.stdout.write;
process.stdout.write = function (chunk, encoding, callback) {
  stdout.push(Buffer.isBuffer(chunk) ? Buffer.from(chunk) : Buffer.from(chunk, typeof encoding === "string" ? encoding : "utf8"));
  if (typeof encoding === "function") encoding();
  else if (typeof callback === "function") callback();
  return true;
};
const trace = [];
const observe = value => { trace.push(value); return value; };
let result;
try {
  let value;
  if (dev) {
    for (const file of ["runtime.js", "client.js", ...manifest.namespaces.map(namespace => namespace.file)]) {
      script(join(build, file));
    }
    value = globalThis.__lightDev.values[manifest.entry];
  } else {
    const text = readFileSync(build, "utf8").replace(
      " = // Original bounded runtime",
      " = globalThis.__lightRuntime = // Original bounded runtime");
    value = runInThisContext(text, { filename: build });
  }
  if (start && typeof value === "function") value = value(observe);
  if (value != null && typeof value.then === "function") value = await value;
  if (units.length > 0) {
    for (const before of [...manifest["before-load"]].reverse()) await hook(before);
    for (const unit of units) script(unit);
    for (const after of manifest["after-load"]) await hook(after);
  }
  result = { value: typeof value === "function" ? "#function" : value };
} catch (error) {
  result = { error };
}
await idle();
process.stdout.write = write;

// Replay the settled run as one synchronous case, so execution.js encodes the
// record: the trace, the captured output, then the value or the thrown error.
globalThis.lightInspectValue = value => globalThis.__lightRuntime?.inspect(value) ?? null;
globalThis.lightCases = [replay => {
  for (const value of trace) replay(value);
  process.stdout.write(Buffer.concat(stdout));
  if ("error" in result) throw result.error;
  return result.value;
}];
globalThis.lightCaseMetadata = [["program", "program"]];
(0, eval)(readFileSync(new URL("execution.js", import.meta.url), "utf8"));
