(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def project (fs/file here "project"))
(def env {"CARGO_TARGET_DIR" "/private/tmp/light-cljs-phase43/v8-probe/target" "LIGHT_V8_MACROS" "1" "LIGHT_V8_HEAP_BYTES" "268435456"})
(doseq [file ["src/lib.rs" "src/main.rs" "build.rs" "vendor/rquickjs-sys/build.rs" "vendor/rquickjs-sys/quickjs/quickjs.c"]]
  (p/shell "touch" (str (fs/file project file))))
(p/shell {:dir (str project)} "cargo" "fmt")
(doseq [[label args] [["retention" ["--lib" "retention_tests"]] ["macros" ["--test" "compiler" "macros::"]]]]
  (println "Checking" label) (flush)
  (with-open [out (io/output-stream (fs/file here (str label ".log")))]
    (apply p/shell {:dir (str project) :out out :err :out :extra-env env} "cargo" "test" args)))
