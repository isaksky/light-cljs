(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(def here (fs/parent *file*))
(def text (slurp (fs/file here "main.log")))
(def stats (mapv (fn [[_ name stage n inclusive exclusive]]
                  {:macro name :stage stage :calls (parse-long n) :inclusive (parse-double inclusive) :exclusive (parse-double exclusive)})
                (re-seq #"PHASE35\t([^\t]+)\t([^\t]+)\t(\d+)\t([0-9.]+)\t([0-9.]+)" text)))
(assert (seq stats))
(def totals (for [[stage rows] (group-by :stage stats)]
              {:stage stage :calls (reduce + (map :calls rows))
               :inclusive (reduce + (map :inclusive rows))
               :exclusive (reduce + (map :exclusive rows))}))
(def summary
  (with-out-str
    (println "Stage totals; exclusive subtracts nested instrumented stages, not uninstrumented work.")
    (doseq [row (sort-by :exclusive > totals)] (prn row))
    (println "\nTop macro call costs")
    (doseq [row (take 30 (sort-by :exclusive > (filter #(= "call" (:stage %)) stats)))] (prn row))
    (println "\nTop total macro costs (sum of exclusive stages)")
    (doseq [row (take 30 (sort-by :seconds >
                  (for [[name rows] (group-by :macro stats)]
                    {:macro name :seconds (reduce + (map :exclusive rows))})))] (prn row))))
(spit (fs/file here "summary.txt") summary)
(assert (= (slurp (fs/file here "main.js")) (slurp "/private/tmp/light-cljs-phase33/production/main-none.js")))
(p/shell {:out :string :err :string} "node" "--check" (str (fs/file here "main.js")))
(spit (fs/file here "equivalence.txt") "Complete main output is byte-identical to phase33 and passes node --check.\n")
(print summary)
