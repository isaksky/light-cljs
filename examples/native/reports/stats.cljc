(ns reports.stats)

(defn summarize [orders]
  (reduce (fn [totals {:keys [region amount]}]
            (update totals region (fn [previous] (+ (or previous 0) amount))))
          {} orders))
