(require '[babashka.fs :as fs] '[babashka.process :as p])
(def here (fs/parent *file*))
(p/shell "bb" (str (fs/file here "validate.bb")))
(def counts (reduce #(mapv + %1 %2) [0 0 0]
  (map #(mapv parse-long (rest %))
    (re-seq #"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored"
      (slurp (fs/file here "tests-v8-default.log"))))))
(assert (= counts [1418 0 106]) counts)
(spit (fs/file here "validation.edn")
  (pr-str {:tests counts :concurrency :default :clippy :passed :format :passed :node-workflows 3}))
(p/shell "bb" (str (fs/file here "build-bench.bb")))
