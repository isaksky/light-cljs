(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(p/shell "bb" (str (fs/file here "build-only.bb")))
(def roots (mapcat #(vector "--source-root" (str "/Users/admin/src/light-cljs/research/penpot-light/" %))
                  ["frontend/src" "common/src" "vendor" "generated" "shims"]))
(def command (vec (concat [(str (fs/file here "light-cljs-current-v8-profile"))
                          "--compile" "--esm" "--macro-expansions" "100000000" "--macro-nodes" "4000000000"
                          "--macro-timeout-ms" "3600000" "--macro-memory-limit" "4000000000"
                          "--macro-poll-budget" "1000000000000"] roots ["--entry" "app.main" "--optimize" "none"])))
(def env {"LIGHT_V8_MACROS" "1" "LIGHT_V8_HEAP_BYTES" "268435456" "LIGHT_V8_FLAGS" ""
          "LIGHT_V8_CPU_PROFILE" (str (fs/file here "cpu-profile.json"))})
(with-open [out (io/output-stream (fs/file here "main.js"))
            err (io/output-stream (fs/file here "main.log"))
            sample-log (io/output-stream (fs/file here "sample.log"))]
  (let [start (System/nanoTime)
        compiler (p/process command {:out out :err err :extra-env env})
        pid (.pid ^Process (:proc compiler))]
    (println "Sampling current V8 compiler" pid) (flush)
    (spit (fs/file here "pid.txt") (str pid "\n"))
    (let [sampler (p/process ["/usr/bin/sample" (str pid) "45" "10" "-file" (str (fs/file here "sample.txt"))]
                            {:out sample-log :err :out})
          result @compiler
          duration (/ (- (System/nanoTime) start) 1e9)]
      (spit (fs/file here "result.edn") (pr-str {:exit (:exit result) :wall-seconds duration
                                               :sample-exit (:exit @sampler)
                                               :command command :environment env}))
      (assert (zero? (:exit result)))
      (println "Compiler profile complete" duration))))
(p/shell "cmp" (str (fs/file here "main.js")) "/private/tmp/light-cljs-phase102/main-none.js")
(with-open [out (io/output-stream (fs/file here "summary.txt"))]
  (p/shell {:out out :err :out} "bb" (str (fs/file here "summarize.bb"))))
(with-open [out (io/output-stream (fs/file here "line-ticks.txt"))]
  (p/shell {:out out :err :out} "bb" (str (fs/file here "line-ticks.bb"))))
