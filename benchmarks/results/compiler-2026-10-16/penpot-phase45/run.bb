(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def roots (mapcat #(vector "--source-root" (str "/Users/admin/src/light-cljs/research/penpot-light/" %))
                  ["frontend/src" "common/src" "vendor" "generated" "shims"]))
(def command (vec (concat ["/private/tmp/light-cljs-phase44/light-cljs-v12"
                          "--compile" "--esm" "--macro-expansions" "100000000" "--macro-nodes" "4000000000"
                          "--macro-timeout-ms" "3600000" "--macro-memory-limit" "4000000000"
                          "--macro-poll-budget" "1000000000000"] roots ["--entry" "app.main" "--optimize" "none"])))
(with-open [out (io/output-stream (fs/file here "main.js"))
            err (io/output-stream (fs/file here "main.log"))
            sample-log (io/output-stream (fs/file here "sample.log"))]
  (let [start (System/nanoTime)
        compiler (p/process command {:out out :err err :extra-env {"LIGHT_V8_MACROS" "1" "LIGHT_V8_HEAP_BYTES" "268435456"}})
        pid (.pid ^Process (:proc compiler))]
    (println "Sampling compiler" pid) (flush)
    (spit (fs/file here "pid.txt") (str pid "\n"))
    (let [sampler (p/process ["/usr/bin/sample" (str pid) "90" "5" "-file" (str (fs/file here "sample.txt"))]
                            {:out sample-log :err :out})
          result @compiler
          duration (/ (- (System/nanoTime) start) 1e9)]
      (spit (fs/file here "result.edn") (pr-str {:exit (:exit result) :wall-seconds duration :sample-exit (:exit @sampler) :command command}))
      (assert (zero? (:exit result)))
      (println "Compiler complete" duration))))
