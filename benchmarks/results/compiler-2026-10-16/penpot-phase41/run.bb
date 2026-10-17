(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def project "/private/tmp/light-cljs-phase21/project")
(def target "/private/tmp/light-cljs-phase13-storage-profile/target")
(with-open [out (io/output-stream (fs/file here "build.log"))]
  (p/shell {:dir project :extra-env {"CARGO_TARGET_DIR" target} :out out :err :out}
    "cargo" "build" "--release" "--bin" "light-cljs"))
(fs/copy (fs/file target "release/light-cljs") (fs/file here "light-cljs") {:replace-existing true})
(def roots (mapcat #(vector "--source-root" (str "/Users/admin/src/light-cljs/research/penpot-light/" %))
                  ["frontend/src" "common/src" "vendor" "generated" "shims"]))
(def command (vec (concat [(str (fs/file here "light-cljs"))
                          "--compile" "--esm" "--macro-expansions" "100000000" "--macro-nodes" "4000000000"
                          "--macro-timeout-ms" "3600000" "--macro-memory-limit" "4000000000"
                          "--macro-poll-budget" "1000000000000"] roots ["--entry" "app.render" "--optimize" "none"])))
(doseq [phase ["before" "after"] ext ["ready" "resume"]]
  (fs/delete-if-exists (fs/file here (str phase "." ext))))
(println "Compiling render with live allocation stacks") (flush)
(with-open [out (io/output-stream (fs/file here "render.js"))
            err (io/output-stream (fs/file here "render.log"))]
  (let [start (System/nanoTime)
        compiler (p/process command {:extra-env {"MallocStackLogging" "lite" "LIGHT_HEAP_PROFILE" (str here)} :out out :err err})
        pid (.pid ^Process (:proc compiler))]
    (spit (fs/file here "pid.txt") (str pid "\n"))
    (doseq [phase ["before" "after"]]
      (loop []
        (when-not (fs/exists? (fs/file here (str phase ".ready")))
          (assert (.isAlive ^Process (:proc compiler)) "Compiler exited before heap checkpoint")
          (Thread/sleep 100) (recur)))
      (println "Collecting live allocations" phase pid) (flush)
      (with-open [heap-out (io/output-stream (fs/file here (str phase "-heap.txt")))
                  heap-err (io/output-stream (fs/file here (str phase "-heap.log")))]
        (let [result (p/shell {:continue true :out heap-out :err heap-err} "/usr/bin/malloc_history" (str pid) "-allBySize")]
          (spit (fs/file here (str phase "-exit.txt")) (str (:exit result) "\n"))))
      (spit (fs/file here (str phase ".resume")) "resume\n"))
    (let [result @compiler]
      (spit (fs/file here "result.edn") (pr-str {:exit (:exit result) :wall-seconds (/ (- (System/nanoTime) start) 1e9) :command command}))
      (assert (zero? (:exit result)))
      (println "Heap diagnostic complete"))))
