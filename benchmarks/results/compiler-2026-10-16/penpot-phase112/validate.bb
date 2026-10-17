(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def project (fs/file here "project"))
(def env {"CARGO_TARGET_DIR" (str (fs/file here "target"))
          "RUSTY_V8_ARCHIVE" (str (fs/file here "frozen-engine/librusty_v8.a"))
          "RUSTY_V8_SRC_BINDING_PATH" (str (fs/file here "frozen-engine/src_binding.rs"))
          "CARGO_PROFILE_RELEASE_LTO" "thin" "CARGO_PROFILE_RELEASE_CODEGEN_UNITS" "1"})
(spit (fs/file here "validation-environment.edn") (pr-str env))
(defn logged [label & command]
  (println "Validating default native backend" label) (flush)
  (with-open [out (io/output-stream (fs/file here (str label ".log")))]
    (apply p/shell {:dir (str project) :out out :err :out :extra-env env} command)))
(defn counts [label]
  (reduce #(mapv + %1 %2) [0 0 0]
    (map #(mapv parse-long (rest %))
      (re-seq #"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored"
        (slurp (fs/file here (str label ".log")))))))
(logged "tests" "cargo" "test" "--locked")
(assert (= [1442 0 106] (counts "tests")))
(logged "clippy" "cargo" "clippy" "--locked" "--all-targets" "--" "-D" "warnings")
(logged "fmt" "cargo" "fmt" "--check")
(doseq [[label script] [["node-runtime" "tests/support/js_runtime.test.js"] ["node-core" "tests/support/js_core_helpers.test.js"] ["node-execution" "tests/support/execution.test.js"]]]
  (logged label "node" script))
(logged "quickjs-tests" "cargo" "test" "--locked" "--no-default-features")
(assert (= [1406 0 106] (counts "quickjs-tests")))
(logged "quickjs-clippy" "cargo" "clippy" "--locked" "--no-default-features" "--all-targets" "--" "-D" "warnings")
(logged "wasm-dependencies" "cargo" "tree" "--locked" "--target" "wasm32-unknown-unknown" "-e" "normal")
(assert (not (re-find #"\bv8 v" (slurp (fs/file here "wasm-dependencies.log")))))
(spit (fs/file here "validation.edn") (pr-str {:tests (counts "tests") :quickjs-tests (counts "quickjs-tests") :clippy :passed :quickjs-clippy :passed :format :passed :node-workflows 3 :wasm-dependency-graph :no-v8}))
;; Refresh after packaging-only edits made while the source engine built.
(logged "build-final" "cargo" "build" "--release" "--locked")
(fs/copy (fs/file here "target/release/light-cljs") (fs/file here "light-cljs-v8-integrated") {:replace-existing true})
(println "Both backends validated")
