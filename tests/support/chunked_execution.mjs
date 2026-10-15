// Runs a program emitted as ES modules and prints its outcome in the shared
// protocol of execution.js, so single-file and chunked builds compare through
// this one driver. Usage:
//
//   node tests/support/chunked_execution.mjs DIRECTORY STARTUP
//
// DIRECTORY holds the startup file and every file it imports; a single-file
// --esm output is a directory with one file. The driver writes
// DIRECTORY/package.json with "type": "module", imports the startup file, calls
// its default export when that is a function (awaiting a returned promise), and
// then waits until nothing is pending: microtasks, timers and the module loads
// a lazy feature starts. It prints one record whose id is STARTUP, with the
// program's stdout from the start of the import, and as its trace the values
// the program passed to `js/lightObserve` (the per-case observer of
// execution.js, which generated programs call) in that time, so a program
// that never calls it has an empty trace. The outcome value and the observed
// values must be host values (nil, a number, a string, a plain array or
// object): no inspection adapter reaches a runtime inside the modules. A
// program that never settles, or rejects a promise nobody handles, fails the
// run instead of printing a record.
import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

if (process.argv.length !== 4) {
  throw new Error("usage: node chunked_execution.mjs DIRECTORY STARTUP");
}
const [directory, startup] = process.argv.slice(2);
writeFileSync(resolve(directory, "package.json"), '{"type": "module"}\n');

// Node empties the microtask queue before every macrotask, and emits
// beforeExit only once no timer, I/O request or module load is left.
const idle = () => new Promise(done => process.once("beforeExit", done));

// Capture stdout and observations as execution.js does, but across the whole
// asynchronous run.
const trace = [];
globalThis.lightObserve = value => { trace.push(value); return value; };
const stdout = [];
const write = process.stdout.write;
process.stdout.write = function (chunk, encoding, callback) {
  stdout.push(Buffer.isBuffer(chunk) ? Buffer.from(chunk) : Buffer.from(chunk, typeof encoding === "string" ? encoding : "utf8"));
  if (typeof encoding === "function") encoding();
  else if (typeof callback === "function") callback();
  return true;
};
let result;
try {
  let value = (await import(pathToFileURL(resolve(directory, startup)).href)).default;
  if (typeof value === "function") value = value();
  if (value != null && typeof value.then === "function") value = await value;
  result = { value };
} catch (error) {
  result = { error };
}
await idle();
process.stdout.write = write;

// Replay the settled run as one synchronous case, so execution.js encodes the
// record: the observations and the captured output, then the value or the
// thrown error.
globalThis.lightCases = [observe => {
  trace.forEach(value => observe(value));
  process.stdout.write(Buffer.concat(stdout));
  if ("error" in result) throw result.error;
  return result.value;
}];
globalThis.lightCaseMetadata = [[startup, startup]];
(0, eval)(readFileSync(new URL("execution.js", import.meta.url), "utf8"));
