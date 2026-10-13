#!/usr/bin/env bb
;; Summarizes saved `cargo nextest run` output: wall-clock, summed test time and
;; the slowest tests. Given a second log, also compares the tests both runs share,
;; grouped by their duration in the first.
;;
;;   cargo nextest run -j 8 > before.log 2>&1
;;   bb benchmarks/nextest_times.bb before.log [after.log]
(require '[clojure.string :as str])

(defn- run [path]
  (let [lines (str/split-lines (slurp path))]
    {:tests (into {}
                  (keep #(when-let [[_ secs name]
                                    (re-find #"^\s*PASS\s*\[\s*([0-9.]+)s\]\s*(?:\(\s*\d+/\s*\d+\))?\s*(.+?)\s*$" %)]
                           [name (parse-double secs)]))
                  lines)
     :summary (some #(re-find #"Summary \[\s*([0-9.]+)s\]\s*(.*)" %) lines)}))

(defn- report [path {:keys [tests summary]}]
  (printf "%s%n  %s%n  wall %.1f s, summed %.1f s%n  slowest:%n"
          path (some-> summary (nth 2) str/trim) (some-> summary second parse-double)
          (reduce + (vals tests)))
  (doseq [[name secs] (take 8 (sort-by (comp - val) tests))]
    (printf "  %7.2f s  %s%n" secs name)))

(defn- bucket [secs]
  (some (fn [[limit label]] (when (< secs limit) label))
        [[0.1 "under 0.1 s"] [0.3 "0.1-0.3 s"] [1 "0.3-1 s"] [5 "1-5 s"]
         [##Inf "over 5 s"]]))

(let [[a b] *command-line-args*
      first-run (run a)]
  (report a first-run)
  (when b
    (let [second-run (run b)
          [ta tb] (map :tests [first-run second-run])
          shared (filter tb (keys ta))]
      (report b second-run)
      (printf "%d shared tests, by duration in the first run:%n" (count shared))
      (doseq [label (distinct (map bucket [0 0.2 0.5 2 10]))
              :let [names (filter #(= label (bucket (ta %))) shared)
                    sa (reduce + (map ta names))
                    sb (reduce + (map tb names))]
              :when (seq names)]
        (printf "  %-12s %4d tests  %7.1f s -> %7.1f s  (%.2f)%n"
                label (count names) sa sb (/ sb sa))))))
