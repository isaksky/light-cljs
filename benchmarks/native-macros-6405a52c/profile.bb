#!/usr/bin/env bb
;; One bounded profile of the slowest successful jobs-1 fixture; no repetitions.
(ns profile)
(require '[babashka.fs :as fs]
         '[babashka.process :as p]
         '[cheshire.core :as json])

(def base (str (fs/parent (fs/absolutize *file*))))
(def report (json/parse-string (slurp (str (fs/path base "runs/report.json"))) true))
(def slowest (last (sort-by #(get-in % [:metrics :wall :median])
                            (filter #(and (= 1 (:jobs %)) (:performance-eligible %)) (:summary report)))))
(def row (first (filter #(and (= (:fixture slowest) (:fixture %))
                              (= 1 (:jobs %)) (= "ok" (:status %))) (:samples report))))
(assert row "No successful serial fixture to profile")
(def dir (str (fs/path base "profile")))
(assert (not (fs/exists? dir)) "Never overwrite or repeat the profile")
(fs/create-dirs dir)
(def args (vec (drop 2 (:command row))))
(def started (System/nanoTime))
(def compiler (p/process args {:dir (:cwd row) :out (str (fs/path dir "out.js"))
                               :err (str (fs/path dir "compiler.stderr")) :shutdown p/destroy-tree}))
(def sample-args ["/usr/bin/sample" (str (.pid (:proc compiler))) "30" "1" "-mayDie"
                  "-file" (str (fs/path dir "sample.txt"))])
(def sampler (p/process sample-args {:out (str (fs/path dir "sample.stdout"))
                                     :err (str (fs/path dir "sample.stderr")) :shutdown p/destroy-tree}))
(defn reap [process timeout-ms]
  (let [result (deref process timeout-ms ::timeout)]
    (when (= ::timeout result)
      (with-open [children (.descendants (:proc process))]
        (doseq [child (iterator-seq (.iterator children))] (.destroyForcibly child)))
      (.destroyForcibly (:proc process)))
    {:exit (:exit (if (= ::timeout result) @process result)) :timeout (= ::timeout result)}))
(def compiled (reap compiler 30000))
(def elapsed-ms (/ (- (System/nanoTime) started) 1e6))
(def sampled (reap sampler (long (max 1 (- 30000 elapsed-ms)))))
(doseq [[flag filename] [["--source-map" "out.map"] ["--stats-file" "trace.tsv"]]
        :let [source (second (drop-while #(not= flag %) args))]
        :when (fs/exists? source)]
  (fs/copy source (fs/path dir filename)))
(spit (str (fs/path dir "metadata.json"))
      (json/generate-string {:revision (:revision report) :fixture (:fixture row) :jobs 1
                             :command args :cwd (:cwd row) :sample-command sample-args
                             :compiler compiled :sampler sampled :compiler-wall-ms elapsed-ms
                             :cap-ms 30000 :interval-ms 1
                             :selection "Largest successful jobs-1 median wall time; one unchanged invocation"}
                            {:pretty true}))
