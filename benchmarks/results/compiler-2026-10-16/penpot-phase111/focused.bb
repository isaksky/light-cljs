(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def env {"CARGO_TARGET_DIR" (str (fs/file here "target"))
          "RUSTY_V8_ARCHIVE" "/private/tmp/light-cljs-phase109/frozen-engine/librusty_v8.a"
          "RUSTY_V8_SRC_BINDING_PATH" "/private/tmp/light-cljs-phase109/frozen-engine/src_binding.rs"
          "LIGHT_V8_MACROS" "1" "LIGHT_V8_HEAP_BYTES" "268435456"})
(with-open [out (io/output-stream (fs/file here "focused.log"))]
  (p/shell {:dir (str (fs/file here "project")) :extra-env env :out out :err :out}
    "cargo" "test" "--locked" "--lib" "v8_evaluator::buffer_tests::" "--" "--nocapture"))
