(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def project (fs/file here "project"))
(def env {"CARGO_TARGET_DIR" "/private/tmp/light-cljs-phase43/v8-probe/target"})
(p/shell "touch" (str (fs/file project "src/lib.rs")) (str (fs/file project "src/main.rs")) (str (fs/file project "build.rs")))
(p/shell "touch" (str (fs/file project "vendor/rquickjs-sys/build.rs")) (str (fs/file project "vendor/rquickjs-sys/quickjs/quickjs.c")))
(p/shell {:dir (str project)} "cargo" "fmt")
(defn logged [label & command]
  (println "Validating QuickJS prepared units" label) (flush)
  (with-open [out (io/output-stream (fs/file here (str label ".log")))]
    (apply p/shell {:dir (str project) :out out :err :out :extra-env env} command)))
(logged "tests" "cargo" "test")
(logged "clippy" "cargo" "clippy" "--all-targets" "--" "-D" "warnings")
(logged "fmt" "cargo" "fmt" "--check")
(doseq [[label script] [["node-runtime" "tests/support/js_runtime.test.js"] ["node-core" "tests/support/js_core_helpers.test.js"] ["node-execution" "tests/support/execution.test.js"]]]
  (logged label "node" script))
(def counts (reduce #(mapv + %1 %2) [0 0 0]
  (map #(mapv parse-long (rest %)) (re-seq #"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored" (slurp (fs/file here "tests.log"))))))
(assert (= counts [1403 0 106]) counts)
(spit (fs/file here "validation.edn") (pr-str {:tests counts :clippy :passed :format :passed :node-workflows 3}))
(println "Candidate full validation passed" counts)
