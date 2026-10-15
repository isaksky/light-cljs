// Runs a dev build (docs/dev-workflow.md, "The dev build") or a single-file
// build in Node and prints its outcome in the shared protocol of
// execution.js, so a dev build and its `--optimize none` oracle compare
// through this one driver. Usage:
//
//   node tests/support/dev_execution.mjs BUILD [--start] [UNIT ...]
//   node tests/support/dev_execution.mjs BUILD [--start] --connect URL --messages N
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
// With --connect, the build's dev client (client.js) reads the event stream
// of a dev server (`--watch --serve`) at URL instead, and applies its loads
// with the same hook protocol, until it has handled N messages after `hello`.
// A message whose handling fails (a unit that throws) counts, and the run goes
// on, as the client does. The client's lines, and one line per message it
// handled (`message hello 1`, `message load replay app.state app.view`,
// `message error FILE:LINE:COLUMN: MESSAGE`, `message warnings ...`, with
// ` failed: MESSAGE` after one that failed), go to stderr, where a test
// follows the run.
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
const option = name => {
  const at = args.indexOf(name);
  if (at < 0) return null;
  const value = args[at + 1];
  if (value === undefined) throw new Error(name + " requires a value");
  args.splice(at, 2);
  return value;
};
const connect = option("--connect");
const messages = Number(option("--messages") ?? 0);
if (connect !== null && !(messages > 0)) throw new Error("--connect requires --messages N");
const units = args.slice(1).filter(arg => arg !== "--start").map(unit => resolve(unit));
if (connect !== null && units.length > 0) throw new Error("--connect applies no unit files");
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
  if (connect !== null) {
    if (!dev) throw new Error("--connect applies only to a dev build");
    const location = diagnostic =>
      [diagnostic.file, diagnostic.line, diagnostic.column].filter(part => part != null).join(":");
    const summary = message => {
      switch (message.op) {
        case "hello": return "hello " + message.runtime;
        case "load":
          return ["load", ...(message.replay ? ["replay"] : []), ...message.units.map(unit => unit.namespace)].join(" ");
        case "error": return `error ${location(message.error)}: ${message.error.message}`;
        case "warnings":
          return "warnings " + message.warnings.map(warning => `${location(warning)}: ${warning.message}`).join("; ");
        default: return message.op;
      }
    };
    await new Promise(resolve => {
      let handled = 0;
      const connection = globalThis.__lightDev.connect(connect, {
        log: (level, text) => process.stderr.write(text + "\n"),
        onHandled(message, error) {
          const failure = error ? " failed: " + (error?.message ?? error) : "";
          process.stderr.write("message " + summary(message) + failure + "\n");
          if (message.op !== "hello" && ++handled === messages) {
            connection.close();
            resolve();
          }
        },
      });
    });
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
