(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io]
         '[clojure.string :as str] '[clojure.edn :as edn])
(def here (fs/parent *file*))
(def project (fs/file here "project"))
(assert (= 0 (:exit (edn/read-string (slurp (fs/file here "build-result.edn"))))))
(def frozen (fs/file here "frozen-engine"))
(fs/create-dirs frozen)
(doseq [[source name] [["obj/librusty_v8.a" "librusty_v8.a"] ["src_binding.rs" "src_binding.rs"] ["args.gn" "args.gn"]]]
  (fs/copy (fs/file here "target/release/gn_out" source) (fs/file frozen name) {:replace-existing true}))
(spit (fs/file here "engine-hashes.txt")
  (:out (apply p/shell {:out :string} "shasum" "-a" "256"
    (map #(str (fs/file frozen %)) ["librusty_v8.a" "src_binding.rs" "args.gn"]))))
(def env {"CARGO_TARGET_DIR" (str (fs/file here "target"))
          "RUSTY_V8_ARCHIVE" (str (fs/file frozen "librusty_v8.a"))
          "RUSTY_V8_SRC_BINDING_PATH" (str (fs/file frozen "src_binding.rs"))
          "LIGHT_V8_MACROS" "1" "LIGHT_V8_HEAP_BYTES" "268435456"})
(spit (fs/file here "validation-environment.edn") (pr-str env))
(defn logged [label & command]
  (println "Validating source-built engine" label) (flush)
  (with-open [out (io/output-stream (fs/file here (str label ".log")))]
    (apply p/shell {:dir (str project) :out out :err :out :extra-env env} command)))
(logged "buffer-tests" "cargo" "test" "--locked" "--lib" "v8_evaluator::buffer_tests::" "--" "--nocapture")
(logged "tests" "cargo" "test" "--locked")
(logged "clippy" "cargo" "clippy" "--locked" "--all-targets" "--" "-D" "warnings")
(logged "fmt" "cargo" "fmt" "--check")
(doseq [[label script] [["node-runtime" "tests/support/js_runtime.test.js"] ["node-core" "tests/support/js_core_helpers.test.js"] ["node-execution" "tests/support/execution.test.js"]]]
  (logged label "node" script))
(def counts (reduce #(mapv + %1 %2) [0 0 0]
  (map #(mapv parse-long (rest %))
    (re-seq #"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored" (slurp (fs/file here "tests.log"))))))
(assert (= counts [1435 0 106]) counts)
(spit (fs/file here "validation.edn") (pr-str {:tests counts :clippy :passed :format :passed :node-workflows 3}))
(fs/copy "/private/tmp/light-cljs-phase58/run-bench.bb" (fs/file here "run-bench.bb") {:replace-existing true})
(doseq [[entry label options]
        [["app.render" "render" ["--optimize" "none"]]
         ["app.main" "main-none" ["--optimize" "none"]]
         ["app.main" "main-advanced" ["--optimize" "advanced" "--pseudo-names" "--elide-asserts"]]]]
  (apply p/shell {:extra-env {"LIGHT_V8_BINARY" (str (fs/file here "light-cljs-v8-buffer-budget"))
                            "LIGHT_V8_HEAP_BYTES" "268435456"}}
    "bb" (str (fs/file here "run-bench.bb")) entry label options)
  (p/shell "cmp" (str (fs/file here (str label ".js")))
    (str "/private/tmp/light-cljs-phase107/" label ".js"))
  (with-open [in (io/input-stream (fs/file here (str label ".js")))
              out (io/output-stream (fs/file here (str label "-syntax.log")))]
    (p/shell {:in in :out out :err :out} "node" "--check" "--input-type=module")))
(println "Source-built engine fully validated; all three outputs identical")
