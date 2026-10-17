(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def project (fs/file here "project"))
(p/shell "touch" (str (fs/file project "src/lib.rs")) (str (fs/file project "src/main.rs")) (str (fs/file project "build.rs")))
(p/shell {:dir (str project)} "cargo" "fmt")
(with-open [out (io/output-stream (fs/file here "evaluator::namespace_registration_tests-tests.log"))]
  (p/shell {:dir (str project) :out out :err :out
            :extra-env {"CARGO_TARGET_DIR" "/private/tmp/light-cljs-phase43/v8-probe/target"}}
    "cargo" "test" "--lib" "evaluator::namespace_registration_tests" "--" "--test-threads=4"))
