(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def project (fs/file here "project"))
(def env {"CARGO_TARGET_DIR" "/private/tmp/light-cljs-phase43/v8-probe/target"})
(doseq [file ["src/lib.rs" "src/main.rs" "build.rs" "vendor/rquickjs-sys/build.rs" "vendor/rquickjs-sys/quickjs/quickjs.c"]]
  (p/shell "touch" (str (fs/file project file))))
(p/shell {:dir (str project)} "cargo" "fmt")
(with-open [out (io/output-stream (fs/file here "focused.log"))]
  (p/shell {:dir (str project) :out out :err :out :extra-env env}
    "cargo" "test" "raw_js_var_holes_assign_lexical_locals_without_losing_iteration_captures" "--" "--nocapture"))
(println "Focused lexical raw-JS assignment tests passed")
