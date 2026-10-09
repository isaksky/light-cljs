(defmacro with-order [[id amount] order & body]
  `(let [{~id :id ~amount :amount} ~order]
     ~@body))

(def calls (atom 0))
(with-order [id amount] (do (swap! calls inc) {:id "A-17" :amount 42})
  (println "Order" id)
  [id (* amount 2) @calls])
