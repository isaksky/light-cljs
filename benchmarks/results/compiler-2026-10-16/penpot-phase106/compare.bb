(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str]
         '[clojure.java.io :as io])
(def here (fs/parent *file*))
(p/shell "bb" (str (fs/file here "run.bb")))
(p/shell {:extra-env {"LIGHT_V8_BINARY" "/private/tmp/light-cljs-phase101/production/light-cljs"}}
  "bb" (str (fs/file here "run-bench.bb")) "app.main" "baseline-main" "--optimize" "none")
(defn bridge [path]
  (let [source (slurp path)
        start (str/index-of source "  function compilerFormBridge() {")
        end (str/index-of source "  //@unit inspect" start)]
    (subs source start end)))
(def old (bridge "/private/tmp/light-cljs-phase101/project/src/js_runtime.js"))
(def new (bridge (fs/file here "project/src/js_runtime.js")))
(def output (slurp (fs/file here "main-none.js")))
(assert (= 1 (count (re-seq (re-pattern (java.util.regex.Pattern/quote new)) output))))
(assert (= (str/replace output new old) (slurp "/private/tmp/light-cljs-phase101/production/main-none.js")))
(p/shell "cmp" (str (fs/file here "baseline-main.js")) "/private/tmp/light-cljs-phase101/production/main-none.js")
(spit (fs/file here "equivalence.txt")
  "Replacing only the expected compilerFormBridge function restores byte identity with phase101. All other output is unchanged.\n")
(with-open [in (io/input-stream (fs/file here "main-none.js"))
            out (io/output-stream (fs/file here "main-syntax.log"))]
  (p/shell {:in in :out out :err :out} "node" "--check" "--input-type=module"))
(println "QuickJS lazy error path comparison and syntax check passed")
