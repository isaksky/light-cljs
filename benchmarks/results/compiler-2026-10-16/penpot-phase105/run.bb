(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io]
         '[clojure.string :as str])
(def here (fs/parent *file*))
(doseq [name ["replay-differential" "paths-differential"]]
  (with-open [out (io/output-stream (fs/file here (str name ".log")))]
    (p/shell {:out out :err :out} "node" (str (fs/file here (str name ".js"))))))
(p/shell "bb" (str (fs/file here "build-only.bb")))
(def binary (fs/file here "light-cljs-lazy-error-paths"))
(doseq [[label executable]
        [["main-none" binary]
         ["baseline-main" "/private/tmp/light-cljs-phase102/light-cljs-v8-lexical-js-vars"]]]
  (p/shell {:extra-env {"LIGHT_V8_BINARY" (str executable) "LIGHT_V8_HEAP_BYTES" "268435456" "LIGHT_V8_FLAGS" ""}}
    "bb" (str (fs/file here "run-bench.bb")) "app.main" label "--optimize" "none"))
(defn bridge [path]
  (let [source (slurp path)
        start (str/index-of source "  function compilerFormBridge() {")
        end (str/index-of source "  //@unit inspect" start)]
    (subs source start end)))
(def old (bridge "/private/tmp/light-cljs-phase102/project/src/js_runtime.js"))
(def new (bridge (fs/file here "project/src/js_runtime.js")))
(def output (slurp (fs/file here "main-none.js")))
(assert (= 1 (count (re-seq (re-pattern (java.util.regex.Pattern/quote new)) output))))
(assert (= (str/replace output new old) (slurp "/private/tmp/light-cljs-phase102/main-none.js")))
(p/shell "cmp" (str (fs/file here "baseline-main.js")) "/private/tmp/light-cljs-phase102/main-none.js")
(spit (fs/file here "equivalence.txt")
  "Replacing only the expected compilerFormBridge function restores byte identity with phase102. All other output is unchanged.\n")
(with-open [in (io/input-stream (fs/file here "main-none.js"))
            out (io/output-stream (fs/file here "main-syntax.log"))]
  (p/shell {:in in :out out :err :out} "node" "--check" "--input-type=module"))
(println "Lazy error path and baseline builds completed; output equivalence and ESM syntax passed")
