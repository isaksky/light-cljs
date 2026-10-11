(def finished (atom []))
(defn quote-price [quantity]
  (try
    (when (zero? quantity)
      (throw (ex-info "Quantity must be positive" {:quantity quantity})))
    {:price (/ 20 quantity)}
    (catch :default error
      {:rejected (:quantity (ex-data error))})
    (finally (swap! finished conj quantity))))
(def quotes (mapv quote-price [5 0 4]))
(println "Completed" (count @finished) "quotes")
[quotes @finished]
