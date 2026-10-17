(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def project (fs/file here "project"))
(p/shell {:dir (str project)} "cargo" "fmt")
(def env {"CARGO_TARGET_DIR" (str (fs/file here "target"))
          "RUSTY_V8_ARCHIVE" "/private/tmp/light-cljs-phase109/frozen-engine/librusty_v8.a"
          "RUSTY_V8_SRC_BINDING_PATH" "/private/tmp/light-cljs-phase109/frozen-engine/src_binding.rs"
          "LIGHT_V8_MACROS" "1" "LIGHT_V8_HEAP_BYTES" "268435456"})
(with-open [out (io/output-stream (fs/file here "negative-control.log"))]
  (let [result (p/shell {:dir (str project) :extra-env env :out out :err :out :continue true}
                 "cargo" "test" "--locked" "--lib"
                 "v8_evaluator::buffer_tests::heap_and_backing_stores_share_one_session_budget"
                 "--" "--nocapture")]
    (spit (fs/file here "negative-control.edn") (pr-str {:exit (:exit result) :environment env}))
    (assert (= 101 (:exit result)) "Inspect whether the aggregate budget regression reproduced")))
