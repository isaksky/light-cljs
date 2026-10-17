(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def project (fs/file here "project"))
(p/shell "touch" (str (fs/file project "src/lib.rs")) (str (fs/file project "src/main.rs"))
  (str (fs/file project "build.rs")) (str (fs/file project "vendor/rquickjs-sys/build.rs"))
  (str (fs/file project "vendor/rquickjs-sys/quickjs/quickjs.c")))
(p/shell {:dir (str project)} "cargo" "fmt")
(with-open [out (io/output-stream (fs/file here "focused-tests.log"))]
  (p/shell {:dir (str project) :out out :err :out
            :extra-env {"CARGO_TARGET_DIR" "/private/tmp/light-cljs-phase43/v8-probe/target"}}
    "cargo" "test" "--lib" "v8_evaluator::tests"))
