(require '[clojure.string :as str] '[babashka.fs :as fs])
(def here (fs/parent *file*))
(def lines (str/split-lines (slurp (fs/file here "sample.txt"))))
(def main-lines (drop-while #(not (re-find #"com.apple.main-thread" %)) lines))
(def expected (parse-long (second (re-find #"^\s+(\d+) Thread_" (first main-lines)))))
(def tree (->> main-lines rest
               (take-while #(not (re-find #"^(?:    \d+ Thread_|Total number in stack|Sort by top|Binary Images:)" %)))))
(def stack (atom []))
(def totals (atom {}))
(def inclusive (atom {}))
(def dedup (atom {}))
(def callers (atom {}))
(defn close! []
  (let [{:keys [n children label raw]} (peek @stack)
        samples (- n children)]
    (swap! stack pop)
    (assert (not (neg? samples)) raw)
    (when (pos? samples)
      (when (and (str/includes? label "GlobalBinding") (str/includes? label "4next")) (swap! callers update (first (filter #(and (str/includes? % "light_cljs") (not (str/includes? % "5arena")) (not (str/includes? % "4core"))) (reverse (map :label @stack)))) (fnil + 0) samples))
      (swap! totals update label (fnil + 0) samples)
      (doseq [ancestor (conj (set (map :label @stack)) label)]
        (swap! inclusive update ancestor (fnil + 0) samples))
      (when (str/includes? label "<deduplicated_symbol>")
        (let [[_ offset address] (re-find #"\+ (\d+)[^\[]+\[(0x[0-9a-f]+)" raw)
              start (- (Long/parseUnsignedLong (subs address 2) 16) (parse-long offset))]
          (swap! dedup update [(format "0x%x" start) (get (peek @stack) :label)] (fnil + 0) samples))))
    (when (seq @stack) (swap! stack update-in [(dec (count @stack)) :children] + n))))
(doseq [line tree
        :let [[_ indent n raw] (re-matches #"([ +!|:]+)(\d+) (.*)" line)]
        :when n]
  (let [depth (count indent)]
    (while (and (seq @stack) (<= depth (:depth (peek @stack)))) (close!))
    (swap! stack conj {:depth depth :n (parse-long n) :children 0 :raw raw
                      :label (str/replace raw #" \+ \d.*$" "")})))
(while (seq @stack) (close!))
(assert (= expected (reduce + (vals @totals))) [expected (reduce + (vals @totals))])
(spit (fs/file here "sample-counts.edn") (pr-str {:total expected :exclusive @totals :inclusive @inclusive}))
(spit (fs/file here "main-thread-summary.txt")
  (with-out-str
    (println "Main-thread exclusive sample counts; total:" (reduce + (vals @totals)))
    (doseq [[label n] (take 80 (sort-by val > @totals))] (println n label))
    (println "\nDeduplicated functions by entry address and immediate caller")
    (doseq [[label n] (take 40 (sort-by val > @dedup))] (prn n label))))
(spit (fs/file here "inclusive-samples.txt")
  (with-out-str
    (println "Inclusive main-thread observations, recursion counted once per named symbol; overlapping totals must not be added.")
    (doseq [[label n] (take 100 (sort-by val > (filter #(str/includes? (key %) "light_cljs") @inclusive)))]
      (println n label))))

(doseq [[label n] (sort-by val > @callers)] (println n label))
