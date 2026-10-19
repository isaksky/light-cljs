// Checks a built playground/dist from Node: loads the wasm package, compiles
// every example in examples.js at every optimization level, runs the output
// and compares the program's value, and checks that `compile_chunks` writes
// each of them as one file, its `--esm` module. Then it compiles a program
// with two lazy features through `compile_chunks` at every level, checks the
// manifest against the files, and runs the modules through the compiler
// tests' Node driver against the single file. Then come a located error, a
// missing namespace and an unknown level, which leave every output empty.
// Last, the page's compiler worker gives the direct results, drops a
// superseded one, ends a compile whose browser macros spin past the 10 s
// deadline with the embedded engine's diagnostic, and compiles correctly
// after a stopped, replaced or failed worker.
// `bb playground/build.bb check` runs it; CI runs it after the build.
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { isDeepStrictEqual } from "node:util";
import { Worker } from "node:worker_threads";

const here = path.dirname(fileURLToPath(import.meta.url));
const dist = path.join(here, "dist");
const pkg = path.join(dist, "pkg");
// The one Node driver for single-file `--esm` and chunked builds.
const driver = path.join(here, "..", "tests", "support", "chunked_execution.mjs");
const { default: init, compile, compile_chunks, serve_macro_host, set_browser_macros, version } = await import(path.join(pkg, "light_cljs_playground.js"));
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

// Browser macros (light-cljs-307, docs/browser-macros.md): with macro-host.js
// installed, macros run in this engine instead of the module's QuickJS.
// Every example at every level, and programs that lean on macro semantics
// (a helper the compiler emits on demand, recursion, gensyms, &form and &env,
// a form returned unchanged keeping its location, special numbers, unpaired
// surrogates), compile to exactly what QuickJS gives. A macro's error differs
// only in the stack trace QuickJS appends to its message.
const { createMacroHost } = await import(path.join(dist, "macro-host.js"));
globalThis.lightCljsMacroHost = createMacroHost(serve_macro_host);
const SEMANTICS = `(ns demo.main)
(defn helper [x] (list 'inc x))
(defmacro via-helper [x] (helper x))
(defmacro line-of [] (:line (meta &form)))
(defmacro local-names [] (vec (sort (map str (keys (:locals &env))))))
(defmacro ident [x] x)
(defmacro my-and ([] true) ([x] x) ([x & more] \`(let [v# ~x] (if v# (my-and ~@more) v#))))
(defmacro traced [label & body] (let [start (gensym "start")] \`(let [~start ~label r# (do ~@body)] [~start r#])))
(defmacro nums [] [##NaN ##Inf ##-Inf -0.0 0.1 1e300 9007199254740993])
(defmacro text [s] (str s "\\u00e9 \\ud83d\\ude00 \\ud800"))
(defmacro same? [a] (identical? a (second &form)))
(let [a 1 b 2] (js/console.log (local-names)))
(js/console.log (via-helper 41) (line-of) (pr-str (nums)) (text "x") (same? (+ 1 2)) (my-and 1 2 3) (traced :t (+ 1 2)))
(ident (undeclared-thing 1))
`;
const macroPrograms = [
  ...EXAMPLES.map((example) => [example.id, example.files.map((file) => file.path), example.files.map((file) => file.source), example.entry]),
  ["semantics", ["demo/main.cljs"], [SEMANTICS], "demo.main"],
  ["throws", ["demo/main.cljs"], ['(ns demo.main)\n(defmacro boom [] (throw (ex-info "macro failed" {})))\n(boom)\n'], "demo.main"],
  ["bad result", ["demo/main.cljs"], ["(ns demo.main)\n(defmacro bad [] (atom 1))\n(bad)\n"], "demo.main"],
];
// Programs whose browser-macro result is also checked on its own: a helper
// keeps the macro generation and values it was defined against (22, not
// 112); a compiler service that fails under a JavaScript catch still fails
// the compile with its own diagnostic (QuickJS reports only that the session
// was invalidated, so that one is not compared); a literal a macro mutates is
// not restored from the compiler's original.
const SNAPSHOTS = `(ns demo.main)
(def factor 2)
(defmacro old [] 10)
(defn helper [x] (+ (old) (* factor x)))
(defmacro old [] 100)
(def factor 3)
(defmacro run [] (helper 4))
(pr-str [(run) (helper 4)])
`;
const CAUGHT = "(ns demo.main)\n(declare helper)\n(defmacro m [] (try (helper) (catch :default e :caught)))\n(defn helper [] (m))\n(m)\n";
const MUTATED = '(ns demo.main)\n(defmacro bump [o] (aset o "x" 2) o)\n(.-x (bump #js {:x 1}))\n';
macroPrograms.push(
  ["definition snapshots", ["demo/main.cljs"], [SNAPSHOTS], "demo.main"],
  ["mutated literal", ["demo/main.cljs"], [MUTATED], "demo.main"],
);
const withoutStack = (fields) => fields.map((field) => (typeof field === "string" ? field.replace(/\n {4}at [^\n]*/g, "").replace(/\n;/g, ";") : field));
let browserCompiles = 0;
for (const [name, paths, sources, entry] of macroPrograms) {
  for (const level of ["none", "prepare", "advanced"]) {
    set_browser_macros(false);
    const embedded = fields(compile(paths, sources, entry, level, true, false, level === "advanced", false));
    set_browser_macros(true);
    const browser = fields(compile(paths, sources, entry, level, true, false, level === "advanced", false));
    set_browser_macros(false);
    check(isDeepStrictEqual(browser, withoutStack(embedded)), `${name} @ ${level} with browser macros: ${JSON.stringify(browser).slice(0, 400)} is not ${JSON.stringify(embedded).slice(0, 400)}`);
    browserCompiles += 1;
  }
}
// A macro sees the engine it runs in: QuickJS has neither `process` nor
// `navigator`, and this one has one of them.
const ENGINE = ["(ns demo.main)\n(defmacro hosted? [] (boolean (or (exists? js/process) (exists? js/navigator))))\n(hosted?)\n"];
function hosted(js) {
  return js == null ? js : String(run(js).value);
}
set_browser_macros(true);
for (const level of ["none", "prepare", "advanced"]) {
  const snapshots = compile(["demo/main.cljs"], [SNAPSHOTS], "demo.main", level, true, false, false, false);
  check(hosted(snapshots.js) === "[22 22]", `browser macros, definition snapshots @ ${level}: ${snapshots.error ?? hosted(snapshots.js)}`);
  const caught = compile(["demo/main.cljs"], [CAUGHT], "demo.main", level, true, false, false, false);
  check(caught.js == null && String(caught.error).includes("compile dependency cycle"), `browser macros, caught service failure @ ${level}: ${caught.error ?? caught.js?.slice(0, 100)}`);
  const mutated = compile(["demo/main.cljs"], [MUTATED], "demo.main", level, true, false, false, false);
  check(hosted(mutated.js) === "2", `browser macros, mutated literal @ ${level}: ${mutated.error ?? hosted(mutated.js)}`);
}
check(hosted(compile(["demo/main.cljs"], ENGINE, "demo.main", "none", true, false, false, false).js) === "true", "browser macros ran in QuickJS");
// Compiles share this realm, but a global one compile's macros define is gone
// when the next one starts, even beside one they made non-configurable.
const LEAK = ['(ns demo.main)\n(defmacro leak [] (.defineProperty js/Object js/globalThis "leakedFixed" #js {:value 1}) (set! js/globalThis.leaked 42) nil)\n(leak)\n'];
const PEEK = ["(ns demo.main)\n(defmacro peek [] (js/String (.-leaked js/globalThis)))\n(peek)\n"];
check(compile(["demo/main.cljs"], LEAK, "demo.main", "none", true, false, false, false).error == null, "browser macros: the leaking compile failed");
const peeked = compile(["demo/main.cljs"], PEEK, "demo.main", "none", true, false, false, false);
check(hosted(peeked.js) === "undefined", `browser macros: a global leaked into the next compile: ${peeked.error ?? hosted(peeked.js)}`);
set_browser_macros(false);
check(hosted(compile(["demo/main.cljs"], ENGINE, "demo.main", "none", true, false, false, false).js) === "false", "embedded macros ran in this engine");
console.log(`browser macros: ${browserCompiles} compiles identical to the embedded engine's`);

// The page compiles in compiler-worker.js through compiler.js. Here both run
// in a worker_threads worker behind an adapter that gives it the web worker
// globals it uses: its results are the direct compile's, a superseded request
// resolves stale, and stopping a compile that spins in a macro leaves a fresh
// worker that compiles correctly, while this thread keeps running.
const { createCompiler, DEADLINE_MS, ALLOWANCE_MS } = await import(path.join(dist, "compiler.js"));
const ADAPTER = `
const { parentPort, workerData } = require("node:worker_threads");
globalThis.self = globalThis;
globalThis.postMessage = (message) => parentPort.postMessage(message);
import(workerData.script).then(() => parentPort.on("message", (data) => self.onmessage({ data })));
`;
const compilerScript = pathToFileURL(path.join(dist, "compiler-worker.js")).href;
// Every thread started, terminated when the check ends.
const threads = [];
function spawnWorker() {
  const thread = new Worker(ADAPTER, { eval: true, workerData: { script: compilerScript } });
  threads.push(thread);
  const handle = {
    onmessage: null,
    onerror: null,
    postMessage: (message) => thread.postMessage(message),
    terminate: () => void thread.terminate(),
  };
  thread.on("message", (data) => handle.onmessage?.({ data }));
  thread.on("error", (error) => handle.onerror?.({ message: String(error) }));
  return handle;
}
const wasmModule = await WebAssembly.compile(fs.readFileSync(path.join(pkg, "light_cljs_playground_bg.wasm")));
const compiler = createCompiler({ spawn: spawnWorker, module: wasmModule });
const workerVersion = await compiler.ready;
check(workerVersion === version(), `worker version ${workerVersion}, page ${version()}`);

function request(paths, sources, entry, optimize) {
  return { paths, sources, entry, optimize, warnings: true, pseudo: false, sizes: optimize === "advanced" };
}
// What the page reads of a result, without the time.
function fields(result) {
  return ["js", "error", "error_file", "error_line", "error_column", "warnings", "size_report"].map((field) => result[field] ?? null);
}
function direct({ paths, sources, entry, optimize, warnings, pseudo, sizes }) {
  const result = compile(paths, sources, entry, optimize, warnings, pseudo, sizes, false);
  return fields(result);
}
async function checkWorkerCompile(label, compiled) {
  const outcome = await compiler.compile(compiled);
  check(outcome.status === "done", `worker ${label}: ${JSON.stringify(outcome)}`);
  if (outcome.status === "done") {
    check(isDeepStrictEqual(fields(outcome.result), direct(compiled)), `worker ${label}: ${JSON.stringify(outcome.result)} is not the direct compile`);
    check(typeof outcome.result.millis === "number", `worker ${label}: millis ${outcome.result.millis}`);
  }
  return outcome;
}
const examplePaths = (example) => example.files.map((file) => file.path);
const exampleSources = (example) => example.files.map((file) => file.source);
for (const example of EXAMPLES) {
  await checkWorkerCompile(`${example.id} @ advanced`, request(examplePaths(example), exampleSources(example), example.entry, "advanced"));
}
for (const example of EXAMPLES) {
  await checkWorkerCompile(`${example.id} @ none, browser macros`, { ...request(examplePaths(example), exampleSources(example), example.entry, "none"), macros: "browser" });
}
const hostedOutcome = await compiler.compile({ ...request(["demo/main.cljs"], ENGINE, "demo.main", "none"), macros: "browser" });
check(hostedOutcome.status === "done" && hosted(hostedOutcome.result.js) === "true", `worker: browser macros ran in QuickJS: ${JSON.stringify(hostedOutcome).slice(0, 200)}`);
await checkWorkerCompile("located error", request(["demo/main.cljs"], ["(ns demo.main)\n(defn f [x] (g x)"], "demo.main", "prepare"));
const warned = await checkWorkerCompile("warning", request(["demo/main.cljs"], ["(ns demo.main)\n(defn f [x] (g x))\n1"], "demo.main", "prepare"));
check(warned.result?.warnings.includes("undeclared"), `worker warning: ${JSON.stringify(warned.result?.warnings)}`);
check(!compiler.cancel(), "worker: cancel with nothing pending reports a cancelled compile");

// Two requests back to back: the first one's result is dropped.
const [first, second] = EXAMPLES;
const superseded = compiler.compile(request(examplePaths(first), exampleSources(first), first.entry, "none"));
const newest = checkWorkerCompile(`${second.id} after ${first.id}`, request(examplePaths(second), exampleSources(second), second.entry, "none"));
check((await superseded).status === "stale", `worker: the superseded compile resolved ${JSON.stringify(await superseded)}`);
await newest;

// A macro that spins until the 10 s session deadline, stopped after 1 s.
// Browser macros have no deadline inside JavaScript: Stop ends such a
// compile at once, and the watchdog below ends it after the deadline.
let ticks = 0;
for (const macros of ["embedded", "browser"]) {
  const spin = compiler.compile({
    ...request(["demo/spin.cljs"], ["(ns demo.spin)\n(defmacro spin [] (loop [i 0] (if (< i 1e15) (recur (inc i)) i)))\n(spin)\n"], "demo.spin", "none"),
    macros,
  });
  let spinOutcome = null;
  spin.then((outcome) => (spinOutcome = outcome));
  ticks = 0;
  const ticker = setInterval(() => (ticks += 1), 20);
  await new Promise((resolve) => setTimeout(resolve, 1000));
  clearInterval(ticker);
  check(spinOutcome == null && compiler.busy, `worker, ${macros} macros: the spinning compile settled early: ${JSON.stringify(spinOutcome)}`);
  check(ticks >= 10, `worker, ${macros} macros: this thread ticked ${ticks} times in 1 s while the compile ran`);
  const workersBefore = threads.length;
  check(compiler.cancel(), `worker, ${macros} macros: cancel found no pending compile`);
  check((await spin).status === "cancelled", `worker, ${macros} macros: the stopped compile resolved ${JSON.stringify(await spin)}`);
  check(threads.length === workersBefore + 1 && !compiler.busy, `worker, ${macros} macros: ${threads.length - workersBefore} workers started by cancel`);
  const after = await checkWorkerCompile(`${first.id} after cancel, ${macros} macros`, request(examplePaths(first), exampleSources(first), first.entry, "prepare"));
  if (after.status === "done") {
    const text = String(run(after.result.js).value);
    check(text === first.expect, `worker ${first.id} after cancel, ${macros} macros: value ${text}, expected ${first.expect}`);
  }
}

// With browser macros, compiler.js's watchdog ends a spinning compile on its
// own after the 10 s deadline (light-cljs-356), with the located diagnostic
// the embedded engine gives, which a second compiler instance computes in
// another thread meanwhile: a macro that spins, a macro whose helper (a
// unit the macro's call registers through the compiler) spins, and a
// newer compile queued behind a spinning one, which then runs. They run at
// once, each in its own worker, while this thread keeps ticking.
const SPINS = [
  ["spinning macro", "(ns demo.spin)\n(defmacro spin [] (loop [i 0] (if (< i 1e15) (recur (inc i)) i)))\n(spin)\n"],
  ["spinning helper", "(ns demo.spin)\n(defn helper [] (loop [i 0] (if (< i 1e15) (recur (inc i)) i)))\n(defmacro spin [] (helper))\n(def x 1)\n  (spin)\n"],
  // Past the worker's first 500 ms, which it does not report.
  [
    "macro spinning after a slow one",
    "(ns demo.spin)\n(defmacro slow [] (let [t (js/Date.now)] (loop [] (when (< (- (js/Date.now) t) 700) (recur)))) nil)\n(defmacro spin [] (loop [i 0] (if (< i 1e15) (recur (inc i)) i)))\n(slow)\n(spin)\n",
  ],
];
// The deadline without a location, and a result a double claims.
const UNLOCATED = [null, "JavaScript session deadline exceeded", null, 0, 0, "", ""];
const OK_RESULT = { js: "1", error: null, error_file: null, error_line: 0, error_column: 0, warnings: "", size_report: "", millis: 1 };
// A worker double that is ready, says "started" for each compile, keeps
// the compile messages and then does `script(message, reply)`.
function scriptedWorker(script) {
  const handle = {
    onmessage: null,
    onerror: null,
    compiles: [],
    postMessage(message) {
      const reply = (data) => queueMicrotask(() => handle.onmessage?.({ data }));
      if (message.type === "init") return reply({ type: "ready", version: version() });
      handle.compiles.push(message);
      reply({ type: "started", id: message.id });
      script(message, reply);
    },
    terminate() {},
  };
  return handle;
}
// A replay whose fingerprint is not the trace at its call says so.
{
  const thread = spawnWorker();
  const answer = new Promise((resolve) => (thread.onmessage = ({ data }) => data.type === "compiled" && resolve(data)));
  thread.postMessage({ type: "init", module: wasmModule });
  thread.postMessage({ type: "compile", id: 1, ...request(["demo/spin.cljs"], [SPINS[0][1]], "demo.spin", "none"), macros: "browser", stopAt: 0, fingerprint: 1 });
  const replayed = await answer;
  check(replayed.stopped === false && String(replayed.result.error).includes("JavaScript session deadline exceeded"), `replay with another fingerprint: ${JSON.stringify(replayed).slice(0, 300)}`);
  thread.terminate();
}
const watchdogStart = performance.now();
let watchdogTicks = 0;
const watchdogTicker = setInterval(() => (watchdogTicks += 1), 20);
const watched = await Promise.all([
  ...SPINS.map(async ([name, source]) => {
    const spinRequest = request(["demo/spin.cljs"], [source], "demo.spin", "none");
    const browser = createCompiler({ spawn: spawnWorker, module: wasmModule });
    const embedded = createCompiler({ spawn: spawnWorker, module: wasmModule });
    const begun = performance.now();
    const [outcome, reference] = await Promise.all([
      browser.compile({ ...spinRequest, macros: "browser" }).then((outcome) => ({ ...outcome, elapsed: performance.now() - begun })),
      embedded.compile({ ...spinRequest, macros: "embedded" }),
    ]);
    check(outcome.status === "done" && reference.status === "done", `watchdog, ${name}: ${JSON.stringify([outcome, reference]).slice(0, 300)}`);
    if (outcome.status !== "done" || reference.status !== "done") return null;
    check(String(reference.result.error).includes("JavaScript session deadline exceeded"), `watchdog, ${name}: the embedded engine gave ${reference.result.error}`);
    check(isDeepStrictEqual(fields(outcome.result), fields(reference.result)), `watchdog, ${name}: ${JSON.stringify(fields(outcome.result))} is not the embedded ${JSON.stringify(fields(reference.result))}`);
    check(outcome.elapsed >= DEADLINE_MS && outcome.elapsed < DEADLINE_MS + 3000, `watchdog, ${name}: ended after ${outcome.elapsed.toFixed(0)} ms`);
    check(outcome.result.millis >= DEADLINE_MS && !browser.busy, `watchdog, ${name}: millis ${outcome.result.millis}, busy ${browser.busy}`);
    // The compiler that replayed it compiles correctly afterwards.
    const after = await browser.compile({ ...request(examplePaths(first), exampleSources(first), first.entry, "none"), macros: "browser" });
    check(after.status === "done" && String(run(after.result.js).value) === first.expect, `watchdog, ${name}: then ${JSON.stringify(after).slice(0, 200)}`);
    return `${name} ${outcome.elapsed.toFixed(0)} ms`;
  }),
  (async () => {
    const queued = createCompiler({ spawn: spawnWorker, module: wasmModule });
    const spinning = queued.compile({ ...request(["demo/spin.cljs"], [SPINS[0][1]], "demo.spin", "none"), macros: "browser" });
    const begun = performance.now();
    const newest = request(examplePaths(first), exampleSources(first), first.entry, "prepare");
    const outcome = await queued.compile({ ...newest, macros: "browser" });
    const elapsed = performance.now() - begun;
    check((await spinning).status === "stale", `watchdog, queued: the spinning compile resolved ${JSON.stringify(await spinning).slice(0, 200)}`);
    check(outcome.status === "done" && isDeepStrictEqual(fields(outcome.result), direct(newest)), `watchdog, queued: ${JSON.stringify(outcome).slice(0, 200)}`);
    check(elapsed >= DEADLINE_MS && elapsed < DEADLINE_MS + 3000, `watchdog, queued: ran after ${elapsed.toFixed(0)} ms`);
    return `queued ${elapsed.toFixed(0)} ms`;
  })(),
  // A macro that leaves a spinning timer behind compiles, but its worker
  // never starts the next compile: that one is sent to a fresh worker.
  (async () => {
    const left = createCompiler({ spawn: spawnWorker, module: wasmModule });
    const LATER = ["(ns demo.main)\n(defmacro later [] (js/setTimeout (fn [] (loop [i 0] (if (< i 1e15) (recur (inc i)) i))) 0) 1)\n(later)\n"];
    const leaving = await left.compile({ ...request(["demo/main.cljs"], LATER, "demo.main", "none"), macros: "browser" });
    check(leaving.status === "done" && leaving.result.error == null, `watchdog, timer left behind: ${JSON.stringify(leaving).slice(0, 200)}`);
    const begun = performance.now();
    const next = request(examplePaths(first), exampleSources(first), first.entry, "none");
    const outcome = await left.compile({ ...next, macros: "browser" });
    const elapsed = performance.now() - begun;
    check(outcome.status === "done" && isDeepStrictEqual(fields(outcome.result), direct(next)), `watchdog, after a timer left behind: ${JSON.stringify(outcome).slice(0, 200)}`);
    check(elapsed >= DEADLINE_MS && elapsed < DEADLINE_MS + 3000, `watchdog, after a timer left behind: ran after ${elapsed.toFixed(0)} ms`);
    return `timer left behind ${elapsed.toFixed(0)} ms`;
  })(),
  // A spin that starts 8 s in is not replayed, since the replay would redo
  // those 8 s: it ends at the deadline, without a location.
  (async () => {
    const late = createCompiler({ spawn: spawnWorker, module: wasmModule });
    const source = SPINS[2][1].replace(" 700)", " 8000)");
    const begun = performance.now();
    const outcome = await late.compile({ ...request(["demo/spin.cljs"], [source], "demo.spin", "none"), macros: "browser" });
    const elapsed = performance.now() - begun;
    check(outcome.status === "done" && isDeepStrictEqual(fields(outcome.result), UNLOCATED), `watchdog, late spin: ${JSON.stringify(outcome).slice(0, 300)}`);
    check(elapsed >= DEADLINE_MS && elapsed < DEADLINE_MS + 1000 && !late.busy, `watchdog, late spin: ended after ${elapsed.toFixed(0)} ms, busy ${late.busy}`);
    return `late spin ${elapsed.toFixed(0)} ms`;
  })(),
  // Probes and replays that do not stop where they should, through worker
  // doubles: each gives the deadline without a location, within the
  // allowance, and the next compile runs in a real worker.
  ...[
    // Nothing reported, so a probe; it compiles after all.
    ["probe that completes", false, true, { quiet: 0 }],
    // Call 3 reported open, so a replay to it; that does not stop there.
    ["replay that takes another path", true, true, { stopAt: 3, fingerprint: 77 }],
    ["replay that never ends", true, false, { stopAt: 3, fingerprint: 77 }],
  ].map(async ([name, reported, completes, mode]) => {
    const rerun = scriptedWorker((message, reply) => completes && reply({ type: "compiled", id: message.id, result: OK_RESULT, stopped: false }));
    const doubles = [scriptedWorker((message, reply) => reported && reply([3, 77])), rerun];
    const scripted = createCompiler({ spawn: () => doubles.shift() ?? spawnWorker(), module: wasmModule });
    const begun = performance.now();
    const outcome = await scripted.compile({ ...request(["demo/spin.cljs"], [SPINS[0][1]], "demo.spin", "none"), macros: "browser" });
    const elapsed = performance.now() - begun;
    check(outcome.status === "done" && isDeepStrictEqual(fields(outcome.result), UNLOCATED) && outcome.result.millis >= DEADLINE_MS, `watchdog, ${name}: ${JSON.stringify(outcome).slice(0, 300)}`);
    check(elapsed >= DEADLINE_MS && elapsed < DEADLINE_MS + ALLOWANCE_MS + 1000 && !scripted.busy && doubles.length === 0, `watchdog, ${name}: ended after ${elapsed.toFixed(0)} ms, busy ${scripted.busy}, ${doubles.length} doubles unused`);
    const rerunMode = { quiet: rerun.compiles[0]?.quiet, stopAt: rerun.compiles[0]?.stopAt, fingerprint: rerun.compiles[0]?.fingerprint };
    check(rerun.compiles.length === 1 && isDeepStrictEqual(rerunMode, { quiet: undefined, stopAt: undefined, fingerprint: undefined, ...mode }), `watchdog, ${name}: rerun as ${JSON.stringify(rerun.compiles)}`);
    // A double that answered is kept, as a healthy worker; one that never
    // did was replaced by a real worker.
    const after = await scripted.compile({ ...request(examplePaths(first), exampleSources(first), first.entry, "none"), macros: "browser" });
    const kept = completes ? rerun.compiles.length === 2 && after.result?.js === OK_RESULT.js : String(run(after.result.js).value) === first.expect;
    check(after.status === "done" && kept, `watchdog, ${name}: then ${JSON.stringify(after).slice(0, 200)}`);
    return `${name} ${elapsed.toFixed(0)} ms`;
  }),
]);
clearInterval(watchdogTicker);
const watchdogMillis = performance.now() - watchdogStart;
check(watchdogTicks >= watchdogMillis / 20 / 4, `watchdog: this thread ticked ${watchdogTicks} times in ${watchdogMillis.toFixed(0)} ms`);
console.log(`watchdog: ${watched.join(", ")}, without Stop`);

// A worker that fails is dropped: its pending compiles resolve failed (the
// older one stale) and the next compile starts a fresh worker. The doubles
// fail the way compiler-worker.js reports a panic, and the way a worker
// that throws does; one failing before it is ready rejects `ready`.
function failingWorker(how) {
  const handle = {
    onmessage: null,
    onerror: null,
    terminated: false,
    postMessage(message) {
      const reply = (data) => queueMicrotask(() => handle.onmessage?.({ data }));
      if (message.type === "init" && how !== "before ready") reply({ type: "ready", version: version() });
      else if (how === "error") queueMicrotask(() => handle.onerror?.({ message: "worker threw" }));
      else if (message.type === "compile" || how === "before ready") reply({ type: "failed", id: message.id, error: "the compiler crashed" });
    },
    terminate() {
      handle.terminated = true;
    },
  };
  return handle;
}
for (const how of ["failed", "error", "before ready"]) {
  const double = failingWorker(how);
  const doubles = [double];
  const recovering = createCompiler({ spawn: () => doubles.shift() ?? spawnWorker(), module: wasmModule });
  const failure = await recovering.ready.then((ready) => `ready ${ready}`, (error) => `rejected ${error.message}`);
  check(failure.startsWith(how === "before ready" ? "rejected" : "ready"), `worker ${how}: ${failure}`);
  const older = recovering.compile(request(examplePaths(first), exampleSources(first), first.entry, "none"));
  const newer = recovering.compile(request(examplePaths(first), exampleSources(first), first.entry, "none"));
  // A worker that failed before it was ready is gone before these start.
  const outcomes = [await older, await newer];
  const expected = how === "before ready" ? "done" : "failed";
  check(
    outcomes[0].status === "stale" && outcomes[1].status === expected && double.terminated && !recovering.busy && doubles.length === 0,
    `worker ${how}: ${JSON.stringify(outcomes).slice(0, 200)}, terminated ${double.terminated}, busy ${recovering.busy}`,
  );
  const recovered = how === "before ready" ? outcomes[1] : await recovering.compile(request(examplePaths(first), exampleSources(first), first.entry, "none"));
  check(
    recovered.status === "done" && isDeepStrictEqual(fields(recovered.result), direct(request(examplePaths(first), exampleSources(first), first.entry, "none"))),
    `worker ${how}: after the failure ${JSON.stringify(recovered).slice(0, 200)}`,
  );
}
console.log(`compiler worker: a compile stopped with each macro engine, the last after ${ticks} ticks of this thread, three failures recovered`);
await Promise.all(threads.map((thread) => thread.terminate()));

if (failures) {
  console.error(`${failures} check(s) failed`);
  process.exit(1);
}
console.log("playground check passed");
