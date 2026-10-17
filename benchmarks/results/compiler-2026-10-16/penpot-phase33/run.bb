(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def roots (mapcat #(vector "--source-root" (str "/Users/admin/src/light-cljs/research/penpot-light/" %))
                  ["frontend/src" "common/src" "vendor" "generated" "shims"]))
(doseq [[job entry] [["render" "app.render"] ["main" "app.main"]]]
  (println "Starting prototype" job)
  (flush)
  (with-open [out (io/output-stream (fs/file here (str job ".js")))
              err (io/output-stream (fs/file here (str job ".log")))]
    (apply p/shell {:out out :err err}
      (concat ["/usr/bin/time" "-l" (str (fs/file here "light-cljs"))
               "--compile" "--esm" "--macro-expansions" "100000000" "--macro-nodes" "4000000000"
               "--macro-timeout-ms" "3600000" "--macro-memory-limit" "4000000000"
               "--macro-poll-budget" "1000000000000"] roots ["--entry" entry "--optimize" "none"]))))
