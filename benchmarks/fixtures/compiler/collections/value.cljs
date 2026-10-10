(let* [original {:items [1 2] [1 2] :pair}
       changed (assoc original :items (conj (:items original) 3))]
  [(get original :items) (get changed :items)
   (get changed '(1 2)) (= #{:a :b} #{:b :a})
   (reduce + 0 (:items changed))])
