(let [make-adder (fn [x] (fn [y] (+ x y)))
      add (make-adder 19)]
  (add 23))
