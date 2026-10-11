(def events (atom []))
(def balance (atom 10 :validator (fn [n] (>= n 0))))
(add-watch balance :audit
  (fn [key reference old new]
    (swap! events conj [key old new (= @reference new)])))
(defn deposit [amount] (swap! balance + amount))
(doseq [amount [5 3 2]] (deposit amount))
(remove-watch balance :audit)
(println "Balance" @balance)
[@balance @events]
