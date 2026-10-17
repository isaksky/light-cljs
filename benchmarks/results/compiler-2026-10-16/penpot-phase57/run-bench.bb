(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def binary (or (System/getenv "LIGHT_V8_BINARY") "/private/tmp/light-cljs-phase43/v8-probe/target/release/light-cljs"))
(def entry (or (first *command-line-args*) "app.render"))
(def label (or (second *command-line-args*) entry))
(def optimize (or (seq (drop 2 *command-line-args*)) ["--optimize" "none"]))
(def roots (mapcat #(vector "--source-root" (str "/Users/admin/src/light-cljs/research/penpot-light/" %))
                  ["frontend/src" "common/src" "vendor" "generated" "shims"]))
(spit (fs/file here (str label ".command.edn"))
  (pr-str {:binary binary :entry entry :options (vec optimize)
           :environment (into {"LIGHT_V8_MACROS" "1"}
                          (for [name ["LIGHT_V8_RUNTIME_TRIAL" "LIGHT_V8_HEAP_BYTES" "LIGHT_V8_STATS" "LIGHT_V8_FLAGS" "LIGHT_V8_SNAPSHOT"]]
                            [name (System/getenv name)]))}))
(with-open [out (io/output-stream (fs/file here (str label ".js")))
            err (io/output-stream (fs/file here (str label ".time")))]
  (apply p/shell {:out out :err err :extra-env {"LIGHT_V8_MACROS" "1"}}
    (concat ["/usr/bin/time" "-l" binary
             "--compile" "--esm" "--macro-expansions" "100000000" "--macro-nodes" "4000000000"
             "--macro-timeout-ms" "3600000" "--macro-memory-limit" "4000000000"
             "--macro-poll-budget" "1000000000000"] roots ["--entry" entry] optimize)))
(println label "completed")
