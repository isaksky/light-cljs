(require '[babashka.fs :as fs] '[clojure.string :as str])
(def here (fs/parent *file*))
(doseq [job ["render" "main"]]
  (let [log (slurp (fs/file here (str job ".log")))
        [_ total dropped] (re-find #"JS_INTERRUPT_PROFILE polls=(\d+) dropped=(\d+)" log)
        rows (for [[_ top incl at] (re-seq #"JS_SAMPLE top=(\d+) inclusive=(\d+) ([^\n]+)" log)]
               {:top (parse-long top) :inclusive (parse-long incl) :at at})]
    (assert total job)
    (assert (= "0" dropped))
    (spit (fs/file here (str job "-summary.txt"))
      (str job ": " total " interrupt-check samples, zero dropped.\n"
           "These measure poll occurrences, not wall-clock percentages.\n\n"
           "Top frame\n" (str/join "\n" (map pr-str (take 30 (sort-by :top > rows))))
           "\n\nInclusive (deduplicated within each sampled stack)\n"
           (str/join "\n" (map pr-str (take 30 (sort-by :inclusive > rows)))) "\n"))))
