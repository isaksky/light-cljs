(require '[babashka.process :as p] '[babashka.fs :as fs] '[clojure.java.io :as io] '[clojure.string :as str])
(def root (fs/parent *file*))
(def fixtures "/private/tmp/light-cljs-penpot-fixed-20261016/fixtures")
(doseq [[fixture entry] [["calls-8000" "probe.main"] ["namespaces-100" "probe.n99"] ["synthetic-4000" "demo.main"]]]
  (let [out (fs/file root (str fixture ".mjs")) log (fs/file root (str fixture ".time.txt"))
        result (with-open [o (io/output-stream out) e (io/output-stream log)]
                 (p/shell {:out o :err e :continue true} "/usr/bin/time" "-l" (str (fs/file root "light-cljs"))
                          "--compile" "--esm" "--macro-expansions" "100000000" "--macro-nodes" "4000000000" "--macro-timeout-ms" "3600000" "--macro-memory-limit" "4000000000" "--macro-poll-budget" "1000000000000" "--optimize" "none" "--source-root" (str (fs/path fixtures fixture)) "--entry" entry))
        timing (slurp log)]
    (assert (zero? (:exit result)) timing)
    (println fixture (re-find #"[0-9.]+ real\s+[0-9.]+ user\s+[0-9.]+ sys" timing)
             (re-find #"\d+\s+maximum resident set size" timing))
    (flush)))
