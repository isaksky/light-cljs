(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def project (fs/file here "project"))
(def env {"CARGO_TARGET_DIR" "/private/tmp/light-cljs-phase43/v8-probe/target" "LIGHT_V8_MACROS" "1" "LIGHT_V8_HEAP_BYTES" "268435456"})
(p/shell "touch" (str (fs/file project "src/lib.rs")) (str (fs/file project "src/main.rs")))
(p/shell {:dir (str project)} "cargo" "fmt")
(defn logged [label & command]
  (println "Validating buffer candidate" label) (flush)
  (with-open [out (io/output-stream (fs/file here (str label ".log")))]
    (apply p/shell {:dir (str project) :out out :err :out :extra-env env} command)))
(logged "buffer-and-callback-tests" "cargo" "test" "v8_evaluator::tests" "--" "--test-threads=1")
(logged "clippy" "cargo" "clippy" "--all-targets" "--" "-D" "warnings")
(println "Buffer candidate focused validation completed; full suite not run.")
