(ns reports.main (:require [reports.stats :as stats]))

(def orders [{:region :west :amount 12}
             {:region :east :amount 7}
             {:region :west :amount 8}])
(def totals (stats/summarize orders))
(println "Processed" (count orders) "orders")
[(get totals :west) (get totals :east) (reduce + (vals totals))]
