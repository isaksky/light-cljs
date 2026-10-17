(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def project (fs/file here "project"))
(def env {"CARGO_TARGET_DIR" (str (fs/file here "target"))
          "RUSTY_V8_ARCHIVE" "/private/tmp/light-cljs-phase112/frozen-engine/librusty_v8.a"
          "RUSTY_V8_SRC_BINDING_PATH" "/private/tmp/light-cljs-phase112/frozen-engine/src_binding.rs"
          "CARGO_PROFILE_RELEASE_LTO" "thin" "CARGO_PROFILE_RELEASE_CODEGEN_UNITS" "1"})
(with-open [out (io/output-stream (fs/file here "build.log"))]
  (p/shell {:dir (str project) :extra-env env :out out :err :out} "cargo" "build" "--release" "--locked"))
(def binary (fs/file here "light-cljs-specialized-stores"))
(fs/copy (fs/file here "target/release/light-cljs") binary {:replace-existing true})
(fs/copy "/private/tmp/light-cljs-phase58/run-bench.bb" (fs/file here "run-bench.bb") {:replace-existing true})
(doseq [[label script] [["node-runtime" "tests/support/js_runtime.test.js"] ["node-core" "tests/support/js_core_helpers.test.js"] ["node-execution" "tests/support/execution.test.js"]]]
  (with-open [out (io/output-stream (fs/file here (str label ".log")))]
    (p/shell {:dir (str project) :out out :err :out} "node" script)))
(doseq [[label bin] [["baseline-1" "/private/tmp/light-cljs-phase112/light-cljs-v8-integrated"]
                    ["candidate-1" (str binary)] ["baseline-2" "/private/tmp/light-cljs-phase112/light-cljs-v8-integrated"]
                    ["candidate-2" (str binary)]]]
  (p/shell {:extra-env {"LIGHT_V8_BINARY" bin}} "bb" (str (fs/file here "run-bench.bb")) "app.main" label "--optimize" "none"))
(println "Alternating specialization comparison complete")
