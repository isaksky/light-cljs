(require '[babashka.process :as p] '[babashka.fs :as fs] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def project (fs/file here "project"))
(def env {"CARGO_TARGET_DIR" "/private/tmp/light-cljs-phase43/v8-probe/target"
          "LIGHT_V8_MACROS" "1"})
(defn logged [root filename & command]
  (with-open [out (io/output-stream (fs/file here filename))]
    (apply p/shell {:dir (str root) :extra-env env :out out :err :out} command)))
(p/shell {:dir (str project)} "cargo" "fmt")
(logged project "qualification-test.log" "cargo" "test" "--test" "compiler"
  "macros::reader_qualification_preserves_alias_collisions_refers_and_definition_views" "--" "--exact")
(logged project "build.log" "cargo" "build" "--release")
(fs/copy "/private/tmp/light-cljs-phase43/v8-probe/target/release/light-cljs"
  (fs/file here "light-cljs-bindings") {:replace-existing true})
(p/shell {:extra-env {"LIGHT_V8_BINARY" (str (fs/file here "light-cljs-bindings"))
                      "LIGHT_V8_HEAP_BYTES" "268435456"}}
  "bb" (str (fs/file here "run-bench.bb")) "app.main" "main-bindings")
