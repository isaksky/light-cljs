(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def project (fs/file here "project"))
(p/shell "touch" (str (fs/file project "src/lib.rs")) (str (fs/file project "src/main.rs")))
(with-open [out (io/output-stream (fs/file here "build.log"))]
 (p/shell {:dir (str project) :out out :err :out :extra-env {"CARGO_TARGET_DIR" "/private/tmp/light-cljs-phase43/v8-probe/target" "CARGO_PROFILE_RELEASE_LTO" "thin" "CARGO_PROFILE_RELEASE_CODEGEN_UNITS" "1"}} "cargo" "build" "--release"))
(fs/copy "/private/tmp/light-cljs-phase43/v8-probe/target/release/light-cljs" (fs/file here "light-cljs-runtime-trials") {:replace-existing true})
