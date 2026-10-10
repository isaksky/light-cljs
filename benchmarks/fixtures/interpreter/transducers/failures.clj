; Reuse each xf; failed invocations abandon fresh lazy sources and reducer state.
[(let [xf (comp (map (fn [x] (if (= x 17) (throw (ex-info "partial" {})) x)))
                (partition-all 32))]
   (fn [] (transduce xf (completing conj) [] (range 1001))))
 (let [xf (comp cat (partition-all 32))]
   (fn [] (transduce xf
                    (completing (fn [a group] (throw (ex-info "cat" {}))))
                    [] (map (fn [x] (range x (+ x 100))) (range 100)))))]
