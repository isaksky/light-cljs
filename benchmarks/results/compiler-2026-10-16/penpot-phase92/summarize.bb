(require '[babashka.fs :as fs] '[clojure.string :as str])
(def here (fs/parent *file*))
(def stacks
  (->> (str/split-lines (slurp (fs/file here "main-none.time")))
       (filter #(str/starts-with? % "QJS_SAMPLE\t"))
       (mapv #(vec (drop 2 (str/split % #"\t"))))))
(def exclusive (frequencies (map #(or (first %) "<empty>") stacks)))
(def inclusive (frequencies (mapcat set stacks)))
(defn render [label counts]
  (str label " (" (count stacks) " samples)\n"
    (str/join "\n" (for [[label n] (sort-by (comp - val) counts)]
                      (format "%5d %5.1f%% %s" n (* 100.0 (/ n (count stacks))) label))) "\n"))
(spit (fs/file here "exclusive.txt") (render "Exclusive" exclusive))
(spit (fs/file here "inclusive.txt") (render "Inclusive (overlapping)" inclusive))
(spit (fs/file here "counts.edn") (pr-str {:samples (count stacks) :exclusive exclusive :inclusive inclusive}))
(println (str/join "\n" (take 45 (str/split-lines (render "Exclusive" exclusive)))))
(println (str/join "\n" (take 35 (str/split-lines (render "Inclusive" inclusive)))))
