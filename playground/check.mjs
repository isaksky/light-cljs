// Checks a built playground/dist from Node: loads the wasm package, compiles
// every example in examples.js at every optimization level, runs the output
// and compares the program's value, then checks a located error and a missing
// namespace. `bb playground/build.bb check` runs it; CI runs it after the build.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const dist = path.join(path.dirname(fileURLToPath(import.meta.url)), "dist");
const pkg = path.join(dist, "pkg");
const { default: init, compile, version } = await import(path.join(pkg, "light_cljs_playground.js"));
const { EXAMPLES } = await import(path.join(dist, "examples.js"));
await init({ module_or_path: fs.readFileSync(path.join(pkg, "light_cljs_playground_bg.wasm")) });

let failures = 0;
function check(condition, message) {
  if (!condition) {
    failures += 1;
    console.error("FAIL:", message);
  }
}

// Runs a compiled program, returning its value and what it logged.
function run(js) {
  const printed = [];
  const log = console.log;
  console.log = (...args) => printed.push(args.map(String).join(" "));
  try {
    return { value: (0, eval)(js), printed };
  } finally {
    console.log = log;
  }
}

console.log(`playground ${version()}`);
check(EXAMPLES.length > 0, "no examples");
for (const example of EXAMPLES) {
  let printedAtNone = null;
  for (const level of ["none", "prepare", "advanced"]) {
    const label = `${example.id} @ ${level}`;
    const result = compile(
      example.files.map((file) => file.path),
      example.files.map((file) => file.source),
      example.entry,
      level,
      true,
      false,
      level === "advanced",
      false,
    );
    check(result.error == null, `${label}: ${result.error}`);
    check(result.warnings === "", `${label}: unexpected warnings ${result.warnings}`);
    if (result.js == null) continue;
    let value;
    let printed;
    try {
      ({ value, printed } = run(result.js));
    } catch (error) {
      check(false, `${label}: threw ${error && error.stack ? error.stack : error}`);
      continue;
    }
    const text = String(value);
    // A program that never prints has no printer at advanced, so a collection
    // value has no toString there; every level must log the same lines.
    const acceptable = text === example.expect || (level === "advanced" && text === "[object Object]");
    check(acceptable, `${label}: value ${text}, expected ${example.expect}`);
    if (level === "none") printedAtNone = printed.join("\n");
    else check(printed.join("\n") === printedAtNone, `${label}: logged\n${printed.join("\n")}\nbut none logged\n${printedAtNone}`);
    check(
      level !== "advanced" || result.size_report.includes("runtime slice") || result.size_report.includes("no runtime embedded"),
      `${label}: no size report`,
    );
    console.log(`${label}: ${result.millis.toFixed(0)} ms, ${result.js.length} bytes`);
  }
}

const broken = compile(["demo/main.cljs"], ["(ns demo.main)\n(defn f [x] (g x)"], "demo.main", "prepare", false, false, false, false);
check(broken.error != null && broken.error_file === "demo/main.cljs" && broken.error_line === 2, `located error: ${broken.error}`);

const missing = compile(["demo/main.cljs"], ["(ns demo.main (:require [demo.nope :as n]))\n(n/x)"], "demo.main", "prepare", false, false, false, false);
check(missing.error != null && missing.error.includes("demo.nope not found"), `missing namespace: ${missing.error}`);

if (failures) {
  console.error(`${failures} check(s) failed`);
  process.exit(1);
}
console.log("playground check passed");
