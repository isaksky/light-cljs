(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(def here (fs/parent *file*))
;; The translation-unit split changed native build inputs after the first pass.
;; Preserve that pass and validate the exact final candidate before benchmarking.
(fs/create-dirs (fs/file here "before-native-split"))
(doseq [name ["tests-v8-default.log" "clippy.log" "fmt.log" "node-runtime.log" "node-core.log" "node-execution.log"]]
  (fs/copy (fs/file here name) (fs/file here "before-native-split" name) {:replace-existing true}))
(p/shell "bb" (str (fs/file here "validate.bb")))
(def counts (reduce #(mapv + %1 %2) [0 0 0]
              (map #(mapv parse-long (rest %))
                (re-seq #"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored"
                  (slurp (fs/file here "tests-v8-default.log"))))))
(assert (= counts [1416 0 106]) counts)
(spit (fs/file here "validation.edn")
  (pr-str {:tests counts :concurrency :default :clippy :passed :format :passed :node-workflows 3
           :native-boundary-probes [:linux-arm64 :windows-x64]}))
(p/shell "bb" (str (fs/file here "build-bench.bb")))
