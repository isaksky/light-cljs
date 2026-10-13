// Checks a built playground/dist from Node: loads the wasm package, compiles
// a small multi-namespace program with a macro at every optimization level,
// runs the output, and checks a located error. `bb playground/build.bb check`
// runs it; CI runs it after the build.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const dist = path.join(path.dirname(fileURLToPath(import.meta.url)), "dist");
const pkg = path.join(dist, "pkg");
const { default: init, compile, version } = await import(path.join(pkg, "light_cljs_playground.js"));
await init({ module_or_path: fs.readFileSync(path.join(pkg, "light_cljs_playground_bg.wasm")) });

const paths = ["demo/main.cljs", "demo/math.cljc", "demo/macros.cljs"];
const sources = [
  `(ns demo.main (:require [demo.math :as math] [demo.macros :refer [twice unless]] [clojure.string :as str]))
   (enable-console-print!)
   (println "squares:" (mapv math/square (range 1 6)))
   (unless (empty? math/primes) (println "primes:" math/primes))
   {:total (reduce + (map math/square (range 1 6))) :words (str/split "a b c" #" ") :t (twice 21)}`,
  `(ns demo.math)
   (defn square [x] (* x x))
   (defn prime? [n] (and (> n 1) (not-any? #(zero? (rem n %)) (range 2 (inc (js/Math.floor (js/Math.sqrt n)))))))
   (def primes (vec (filter prime? (range 30))))`,
  "(ns demo.macros) (defmacro twice [x] `(let [v# ~x] (+ v# v#))) (defmacro unless [test & body] `(if ~test nil (do ~@body)))",
];

let failures = 0;
function check(condition, message) {
  if (!condition) {
    failures += 1;
    console.error("FAIL:", message);
  }
}

console.log(`playground ${version()}`);
for (const level of ["none", "prepare", "advanced"]) {
  const result = compile(paths, sources, "demo.main", level, true, false, level === "advanced", false);
  check(result.error == null, `${level}: ${result.error}`);
  check(result.warnings === "", `${level}: unexpected warnings ${result.warnings}`);
  if (result.js == null) continue;
  const printed = [];
  const log = console.log;
  console.log = (...args) => printed.push(args.join(" "));
  let value;
  try {
    value = (0, eval)(result.js);
  } finally {
    console.log = log;
  }
  check(String(value) === '{:total 55, :words ["a" "b" "c"], :t 42}', `${level}: value ${String(value)}`);
  check(printed.join("\n") === "squares: [1 4 9 16 25]\nprimes: [2 3 5 7 11 13 17 19 23 29]", `${level}: printed ${printed}`);
  check(level !== "advanced" || result.size_report.includes("runtime slice"), "advanced: no size report");
  console.log(`${level}: ${result.millis.toFixed(0)} ms, ${result.js.length} bytes`);
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
