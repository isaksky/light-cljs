(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def project (fs/file here "project"))
(def env {"CARGO_TARGET_DIR" "/private/tmp/light-cljs-phase43/v8-probe/target" "LIGHT_V8_MACROS" "1"})
(doseq [[label args] [["scan-test" ["test" "streamed_name_scan_preserves_string_boundaries_and_lossy_utf16"]]
                      ["hygiene-test" ["test" "unit_runtime_parameter_preserves_raw_javascript_hygiene"]]
                      ["build" ["build" "--release"]]]]
  (with-open [out (io/output-stream (fs/file here (str label ".log")))]
    (apply p/shell {:dir (str project) :out out :err :out :extra-env env} "cargo" args)))
(fs/copy "/private/tmp/light-cljs-phase43/v8-probe/target/release/light-cljs"
  (fs/file here "light-cljs-scan") {:replace-existing true})
(p/shell {:extra-env {"LIGHT_V8_BINARY" (str (fs/file here "light-cljs-scan")) "LIGHT_V8_HEAP_BYTES" "268435456"}}
  "bb" (str (fs/file here "run-bench.bb")) "app.main" "main-scan")
(p/shell "cmp" (str (fs/file here "main-scan.js")) "/private/tmp/light-cljs-phase42/production/main-none.js")
