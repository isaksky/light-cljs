// Checks a built playground/dist from Node: loads the wasm package, compiles
// every example in examples.js at every optimization level, runs the output
// and compares the program's value, and checks that `compile_chunks` writes
// each of them as one file, its `--esm` module. Then it compiles a program
// with two lazy features through `compile_chunks` at every level, checks the
// manifest against the files, and runs the modules through the compiler
// tests' Node driver against the single file. Last come a located error, a
// missing namespace and an unknown level, which leave every output empty.
// `bb playground/build.bb check` runs it; CI runs it after the build.
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { isDeepStrictEqual } from "node:util";

const here = path.dirname(fileURLToPath(import.meta.url));
const dist = path.join(here, "dist");
const pkg = path.join(dist, "pkg");
// The one Node driver for single-file `--esm` and chunked builds.
const driver = path.join(here, "..", "tests", "support", "chunked_execution.mjs");
const { default: init, compile, compile_chunks, version } = await import(path.join(pkg, "light_cljs_playground.js"));
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

// Writes `files` with their parallel `sources` into a fresh directory, runs
// the driver on `startup` there and returns its one record, parsed; throws
// with the driver's output when it fails or prints anything else.
function runModules(files, sources, startup) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "light-cljs-playground-"));
  try {
    files.forEach((file, index) => fs.writeFileSync(path.join(directory, file), sources[index]));
    const driven = spawnSync(process.execPath, [driver, directory, startup], { encoding: "utf8", timeout: 60_000 });
    const record = driven.stdout.endsWith("\n") ? driven.stdout.slice(0, -1) : null;
    if (driven.status !== 0 || record == null || record.includes("\n")) {
      throw new Error(`driver exited with ${driven.status ?? driven.signal}\nstdout:\n${driven.stdout}\nstderr:\n${driven.stderr}`);
    }
    return JSON.parse(record);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
}

// Checks a chunked build's parallel `files` and `chunks` against its manifest
// (docs/compiler.md, "Chunked builds"): startup first, each file once, named
// by its chunk and kept in one directory, after every file it imports, with
// its size in bytes; the static imports each module spells are those the
// manifest lists, and every `import()` names a file holding a loadable's
// target. Returns the manifest, or null when it does not parse.
function checkManifest(label, result) {
  const { files, chunks } = result;
  check(files.length > 0 && files.length === chunks.length, `${label}: ${files.length} files but ${chunks.length} chunks`);
  check(new Set(files).size === files.length, `${label}: repeated files ${files}`);
  let manifest;
  try {
    manifest = JSON.parse(result.manifest);
  } catch (error) {
    check(false, `${label}: manifest ${result.manifest} does not parse: ${error}`);
    return null;
  }
  check(manifest.startup === files[0], `${label}: manifest startup ${manifest.startup}, files ${files}`);
  const listed = Array.isArray(manifest.chunks) ? manifest.chunks : [];
  const shaped = (chunk) =>
    typeof chunk.name === "string" && typeof chunk.file === "string" && Number.isInteger(chunk.bytes) &&
    ["imports", "namespaces", "roots", "loadables"].every((field) => Array.isArray(chunk[field]));
  if (!listed.every(shaped) || !isDeepStrictEqual(listed.map((chunk) => chunk.file), files)) {
    check(false, `${label}: manifest ${result.manifest} does not list the files ${files}`);
    return null;
  }
  const loading = new Set(listed.filter((chunk) => chunk.loadables.length > 0).map((chunk) => chunk.file));
  listed.forEach((chunk, index) => {
    const file = files[index];
    const source = chunks[index] ?? "";
    const at = `${label}: ${file}`;
    check(path.basename(file) === file && `${chunk.name}.js` === file, `${at}: chunk name ${chunk.name}`);
    check(chunk.bytes === Buffer.byteLength(source), `${at}: manifest says ${chunk.bytes} bytes, the module has ${Buffer.byteLength(source)}`);
    const earlier = files.slice(0, index);
    check(chunk.imports.every((imported) => earlier.includes(imported)), `${at}: imports ${chunk.imports}, not all of them before it`);
    const spelled = [...source.matchAll(/^import\b.*"\.\/([^"]+)";$/gm)].map((match) => match[1]);
    check(isDeepStrictEqual([...spelled].sort(), [...chunk.imports].sort()), `${at}: imports ${spelled} but the manifest lists ${chunk.imports}`);
    for (const [, loaded] of source.matchAll(/\bimport\("\.\/([^"]+)"\)/g)) {
      check(loading.has(loaded), `${at}: import("./${loaded}") names no file holding a loadable's target`);
    }
  });
  return manifest;
}

console.log(`playground ${version()}`);
check(EXAMPLES.length > 0, "no examples");
for (const example of EXAMPLES) {
  const paths = example.files.map((file) => file.path);
  const sources = example.files.map((file) => file.source);
  let printedAtNone = null;
  for (const level of ["none", "prepare", "advanced"]) {
    const label = `${example.id} @ ${level}`;
    const result = compile(paths, sources, example.entry, level, true, false, level === "advanced", false);
    check(result.error == null, `${label}: ${result.error}`);
    check(result.warnings === "", `${label}: unexpected warnings ${result.warnings}`);
    // Without a loadable, a chunked build is one file, byte for byte the
    // `--esm` module.
    const module = compile(paths, sources, example.entry, level, true, false, false, true);
    const chunked = compile_chunks(paths, sources, example.entry, level, true, false, false);
    check(chunked.error == null && chunked.js == null, `${label} chunked: ${chunked.error}`);
    checkManifest(`${label} chunked`, chunked);
    check(
      isDeepStrictEqual(chunked.files, [`${example.entry}.js`]) && chunked.chunks[0] === module.js,
      `${label} chunked: files ${chunked.files} are not the --esm module`,
    );
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

// Two lazy features sharing a namespace that startup does not require, so a
// chunked build is startup, the shared chunk and one chunk per feature. The
// shared namespace is a .cljc file, which the in-memory provider finds after
// .cljs. The default export loads both features and resolves to a string.
const LAZY = [
  {
    path: "app/main.cljs",
    source: `(ns app.main
  (:require [app.shared :as shared]
            [light.lazy :as lazy]))

(js/console.log "menu:" (shared/label :editor) "|" (shared/label :reports))

(def editor (lazy/loadable app.editor/start))

(def reports (lazy/loadable app.reports/start))

(defn start []
  (-> (lazy/load editor)
      (.then (fn [start-editor] (start-editor) (lazy/load reports)))
      (.then (fn [start-reports]
               (str "loaded " (lazy/loaded? editor) ": " (start-reports))))))

start
`,
  },
  {
    path: "app/shared.cljs",
    source: `(ns app.shared)

(defn label [k]
  (let [text (name k)]
    (str (.toUpperCase (subs text 0 1)) (subs text 1))))
`,
  },
  {
    path: "app/text.cljc",
    source: `(ns app.text
  (:require [clojure.string :as string]))

(defn words [text] (string/split text #" "))

(defn shout [text] (string/upper-case text))
`,
  },
  {
    path: "app/editor.cljs",
    source: `(ns app.editor
  (:require [app.text :as text]))

(defn start []
  (js/console.log "editor:" (count (text/words "one two three")) "words"))
`,
  },
  {
    path: "app/reports.cljs",
    source: `(ns app.reports
  (:require [app.text :as text]))

(defn start []
  (text/shout (pr-str (into (sorted-set) (text/words "b a c")))))
`,
  },
];
const LAZY_RECORD = {
  id: "app.main.js",
  outcome: { status: "returned", value: 'loaded true: #{"A" "B" "C"}' },
  trace: [],
  stdout: "menu: Editor | Reports\neditor: 3 words\n",
};
const lazyPaths = LAZY.map((file) => file.path);
const lazySources = LAZY.map((file) => file.source);
for (const level of ["none", "prepare", "advanced"]) {
  const label = `lazy features @ ${level}`;
  const single = compile(lazyPaths, lazySources, "app.main", level, true, false, false, true);
  check(single.error == null, `${label} single file: ${single.error}`);
  const result = compile_chunks(lazyPaths, lazySources, "app.main", level, true, false, level === "advanced");
  check(result.error == null && result.js == null, `${label}: ${result.error}`);
  check(result.warnings === "", `${label}: unexpected warnings ${result.warnings}`);
  check(
    level !== "advanced" || result.size_report.includes("runtime slice"),
    `${label}: no size report`,
  );
  const manifest = checkManifest(label, result);
  if (single.js == null || manifest == null) continue;
  // Each feature is a chunk of its own, which the loadable of its target in
  // app.main names.
  check(result.files.length > 2, `${label}: ${result.files.length} files`);
  check(manifest.chunks[0].roots.includes("app.main"), `${label}: startup roots ${manifest.chunks[0].roots}`);
  for (const feature of ["app.editor", "app.reports"]) {
    const holders = manifest.chunks.filter((chunk) =>
      chunk.loadables.some((site) => site.namespace === "app.main" && site.target === `${feature}/start`));
    check(
      holders.length === 1 && holders[0].file === `${feature}.js` && holders[0].namespaces.includes(feature) && holders[0].roots.includes(feature),
      `${label}: the loadable of ${feature}/start resolves to ${holders.map((chunk) => chunk.file)}`,
    );
  }
  // The modules print what the single file prints.
  try {
    const oracle = runModules(["app.main.js"], [single.js], "app.main.js");
    check(isDeepStrictEqual(oracle, LAZY_RECORD), `${label} single file: ${JSON.stringify(oracle)}`);
    const chunked = runModules(result.files, result.chunks, manifest.startup);
    check(isDeepStrictEqual(chunked, oracle), `${label}: ${JSON.stringify(chunked)}\nbut the single file gave ${JSON.stringify(oracle)}`);
  } catch (error) {
    check(false, `${label}: ${error && error.stack ? error.stack : error}`);
    continue;
  }
  const sizes = result.chunks.map((chunk, index) => `${result.files[index]} ${chunk.length}`).join(", ");
  console.log(`${label}: ${result.millis.toFixed(0)} ms, ${sizes} bytes`);
}

// A failed compilation sets the error and no output, from either export.
function checkNoOutput(label, result) {
  check(
    result.js == null && result.manifest == null && result.files.length === 0 && result.chunks.length === 0,
    `${label}: output beside the error: js ${JSON.stringify(result.js)}, manifest ${JSON.stringify(result.manifest)}, files ${JSON.stringify(result.files)}, ${result.chunks.length} chunks`,
  );
}
for (const [name, build] of [
  ["compile", (paths, sources, entry, level) => compile(paths, sources, entry, level, false, false, false, false)],
  ["compile_chunks", (paths, sources, entry, level) => compile_chunks(paths, sources, entry, level, false, false, false)],
]) {
  const broken = build(["demo/main.cljs"], ["(ns demo.main)\n(defn f [x] (g x)"], "demo.main", "prepare");
  check(broken.error != null && broken.error_file === "demo/main.cljs" && broken.error_line === 2, `${name} located error: ${broken.error}`);
  checkNoOutput(`${name} located error`, broken);

  const missing = build(["demo/main.cljs"], ["(ns demo.main (:require [demo.nope :as n]))\n(n/x)"], "demo.main", "prepare");
  check(missing.error != null && missing.error.includes("demo.nope not found"), `${name} missing namespace: ${missing.error}`);
  checkNoOutput(`${name} missing namespace`, missing);

  const lazyMissing = build(
    ["demo/main.cljs"],
    ["(ns demo.main (:require [light.lazy :as lazy]))\n(def later (lazy/loadable demo.nope/start))"],
    "demo.main",
    "advanced",
  );
  check(
    lazyMissing.error != null && lazyMissing.error.includes("demo.nope not found") && lazyMissing.error_line === 2,
    `${name} missing lazy target: ${lazyMissing.error}`,
  );
  checkNoOutput(`${name} missing lazy target`, lazyMissing);

  const level = build(lazyPaths, lazySources, "app.main", "fast");
  check(level.error != null && level.error.includes("unknown optimization level"), `${name} unknown level: ${level.error}`);
  checkNoOutput(`${name} unknown level`, level);
}

if (failures) {
  console.error(`${failures} check(s) failed`);
  process.exit(1);
}
console.log("playground check passed");
