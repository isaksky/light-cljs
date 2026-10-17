(require '[babashka.process :as p] '[clojure.java.io :as io])
(def destination "/private/tmp/light-cljs-phase2-profile")
(def entry (or (first *command-line-args*) "app.render"))
(def stem (if (= entry "app.main") "main" "render"))
(def roots (mapcat #(vector "--source-root" (str "/Users/admin/src/light-cljs/research/penpot-light/" %)) ["frontend/src" "common/src" "vendor" "generated" "shims"]))
(with-open [out (io/output-stream (str destination "/" stem ".js")) err (io/output-stream (str destination "/" stem ".log"))]
  (let [result (apply p/shell {:out out :err err :continue true} (concat ["/usr/bin/time" "-l" (str destination "/target/release/light-cljs") "--compile" "--esm" "--macro-expansions" "100000000" "--macro-nodes" "4000000000" "--macro-timeout-ms" "3600000" "--macro-memory-limit" "4000000000" "--macro-poll-budget" "1000000000000"] roots ["--entry" entry "--optimize" "none"]))]
    (assert (zero? (:exit result)) (slurp (str destination "/" stem ".log")))))
