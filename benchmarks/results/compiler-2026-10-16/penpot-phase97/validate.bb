(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def project (fs/file here "project"))
(def env {"CARGO_TARGET_DIR" "/private/tmp/light-cljs-phase43/v8-probe/target" "LIGHT_V8_MACROS" "1" "LIGHT_V8_HEAP_BYTES" "268435456"})
(p/shell "touch" (str (fs/file project "src/lib.rs")) (str (fs/file project "src/main.rs")) (str (fs/file project "build.rs")))
(p/shell "touch" (str (fs/file project "vendor/rquickjs-sys/build.rs")) (str (fs/file project "vendor/rquickjs-sys/quickjs/quickjs.c")))
(p/shell {:dir (str project)} "cargo" "fmt")
(defn logged [label & command]
  (println "Validating bounded V8 stack and shared scheduler" label) (flush)
  (with-open [out (io/output-stream (fs/file here (str label ".log")))]
    (apply p/shell {:dir (str project) :out out :err :out :extra-env env} command)))
(logged "tests-v8-default" "cargo" "test")
(logged "clippy" "cargo" "clippy" "--all-targets" "--" "-D" "warnings")
(logged "fmt" "cargo" "fmt" "--check")
(doseq [[label script] [["node-runtime" "tests/support/js_runtime.test.js"] ["node-core" "tests/support/js_core_helpers.test.js"] ["node-execution" "tests/support/execution.test.js"]]]
  (logged label "node" script))
