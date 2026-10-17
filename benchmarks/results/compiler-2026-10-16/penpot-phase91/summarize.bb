(require '[babashka.fs :as fs] '[clojure.string :as str])
(def here (fs/parent *file*))
(def rows
  (->> (str/split-lines (slurp (fs/file here "main-none.time")))
       (filter #(str/starts-with? % "MACRO_TIMING\t"))
       (mapv #(let [[_ group label count total own] (str/split % #"\t")]
                {:group (parse-long group) :label label :count (parse-long count)
                 :total-ns (parse-long total) :exclusive-ns (parse-long own)}))))
(spit (fs/file here "timings.edn") (pr-str rows))
(spit (fs/file here "summary.txt")
  (str/join "\n" (for [group [0 1]]
    (str (if (zero? group) "Macro timings" "Stage timings") "\n"
      (str/join "\n" (for [{:keys [label count total-ns exclusive-ns]}
                          (take 40 (filter #(= group (:group %)) rows))]
        (format "%8.3f s inclusive %8.3f s exclusive %6d calls %s"
                (/ total-ns 1e9) (/ exclusive-ns 1e9) count label)))))))
(println (slurp (fs/file here "summary.txt")))
