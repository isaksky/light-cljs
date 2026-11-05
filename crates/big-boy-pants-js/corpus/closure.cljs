(defmacro captured [base xs]
  (let [offset (+ base 3)
        helper (fn [x] [x (+ offset x)])]
    (list 'quote (mapv helper xs))))
(js/console.log (pr-str (captured 7 [1 2 3])))
