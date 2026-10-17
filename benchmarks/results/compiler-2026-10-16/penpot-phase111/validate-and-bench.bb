(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def project (fs/file here "project"))
(def env {"CARGO_TARGET_DIR" (str (fs/file here "target"))
          "RUSTY_V8_ARCHIVE" "/private/tmp/light-cljs-phase109/frozen-engine/librusty_v8.a"
          "RUSTY_V8_SRC_BINDING_PATH" "/private/tmp/light-cljs-phase109/frozen-engine/src_binding.rs"
          "CARGO_PROFILE_RELEASE_LTO" "thin" "CARGO_PROFILE_RELEASE_CODEGEN_UNITS" "1"
          "LIGHT_V8_MACROS" "1"})
(spit (fs/file here "validation-environment.edn") (pr-str env))
(defn logged [label & command]
  (println "Validating adaptive budget" label) (flush)
  (with-open [out (io/output-stream (fs/file here (str label ".log")))]
    (apply p/shell {:dir (str project) :out out :err :out :extra-env env} command)))
(logged "tests" "cargo" "test" "--locked")
(logged "clippy" "cargo" "clippy" "--locked" "--all-targets" "--" "-D" "warnings")
(logged "fmt" "cargo" "fmt" "--check")
(doseq [[label script] [["node-runtime" "tests/support/js_runtime.test.js"] ["node-core" "tests/support/js_core_helpers.test.js"] ["node-execution" "tests/support/execution.test.js"]]]
  (logged label "node" script))
(def counts (reduce #(mapv + %1 %2) [0 0 0]
  (map #(mapv parse-long (rest %))
    (re-seq #"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored" (slurp (fs/file here "tests.log"))))))
(assert (= counts [1442 0 106]) counts)
(spit (fs/file here "validation.edn") (pr-str {:tests counts :clippy :passed :format :passed :node-workflows 3}))
(logged "build" "cargo" "build" "--release" "--locked")
(def binary (fs/file here "light-cljs-v8-adaptive-budget"))
(fs/copy (fs/file here "target/release/light-cljs") binary {:replace-existing true})
(fs/copy "/private/tmp/light-cljs-phase58/run-bench.bb" (fs/file here "run-bench.bb") {:replace-existing true})
(doseq [[entry label options]
        [["app.render" "render" ["--optimize" "none"]]
         ["app.main" "main-none" ["--optimize" "none"]]
         ["app.main" "main-advanced" ["--optimize" "advanced" "--pseudo-names" "--elide-asserts"]]]]
  (apply p/shell {:extra-env {"LIGHT_V8_BINARY" (str binary)}}
    "bb" (str (fs/file here "run-bench.bb")) entry label options)
  (p/shell "cmp" (str (fs/file here (str label ".js")))
    (str "/private/tmp/light-cljs-phase109/" label ".js"))
  (with-open [in (io/input-stream (fs/file here (str label ".js")))
              out (io/output-stream (fs/file here (str label "-syntax.log")))]
    (p/shell {:in in :out out :err :out} "node" "--check" "--input-type=module")))
(p/shell {:extra-env {"LIGHT_V8_BINARY" "/private/tmp/light-cljs-phase110/light-cljs-v8-aggregate-budget" "LIGHT_V8_HEAP_BYTES" "268435456"}}
  "bb" (str (fs/file here "run-bench.bb")) "app.main" "baseline-main" "--optimize" "none")
(p/shell "cmp" (str (fs/file here "main-none.js")) (str (fs/file here "baseline-main.js")))
(println "Adaptive budget fully validated; outputs unchanged")
