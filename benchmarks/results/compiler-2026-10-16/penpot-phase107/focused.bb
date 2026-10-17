(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def project (fs/file here "project"))
(doseq [file ["src/lib.rs" "src/main.rs" "build.rs" "vendor/rquickjs-sys/build.rs" "vendor/rquickjs-sys/quickjs/quickjs.c"]]
  (p/shell "touch" (str (fs/file project file))))
(p/shell {:dir (str project)} "cargo" "fmt")
(with-open [out (io/output-stream (fs/file here "focused.log"))]
  (p/shell {:dir (str project) :out out :err :out
            :extra-env {"CARGO_TARGET_DIR" "/private/tmp/light-cljs-phase43/v8-probe/target"
                        "LIGHT_V8_MACROS" "1" "LIGHT_V8_HEAP_BYTES" "268435456"}}
    "cargo" "test" "--lib" "v8_evaluator::" "--" "--nocapture"))
(println "V8 initialization and existing engine regressions passed")
