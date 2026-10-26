(ns transducers.macros)

;; These factories capture only immutable native functions. Each consumer
;; applies the same published pipeline to its own reducer and input callback.
(def pipeline
  (comp (partition-by (fn [x] (= x 2)))
        (filter (fn [x] (some? x)))
        (mapcat (fn [x] x))))

;; Initializers stay cold until the callbacks below demand them. In particular,
;; step-helper is first read while partition-by holds the first two inputs;
;; done-helper is first read after completion has flushed the final group.
(defn step-helper [x] (inc x))
(defn flush-helper [x] x)
(defn done-helper [r] (conj r :done))

(defmacro run [xs]
  (let [log (js/Array.)
        xf (comp (map (fn [x]
                        (.push log (str "input" x))
                        (if (= x 2)
                          (do (.push log "before-step")
                              (.push log (step-helper x))
                              (.push log "after-step")))
                        x))
                 pipeline)
        rf (fn ([] [])
             ([r] (.push log "before-done")
                  (let [result (done-helper r)]
                    (.push log "after-done")
                    result))
             ([r x] (.push log (str "out" x))
                    (if (= x 3)
                      (do (.push log "before-flush")
                          (.push log (flush-helper x))
                          (.push log "after-flush")))
                    (conj r x)))
        a (transduce xf rf xs)
        b (transduce xf rf [2 2 1])
        empty (transduce xf rf [])
        left (pipeline (completing conj))
        right (pipeline (completing conj))
        l1 (left [] 1)
        r1 (right [] 2)
        l2 (left l1 2)
        r2 (right r1 1)
        interleaved [(left l2) (right r2)]]
    (list 'quote [a b empty interleaved (.join log ",")])))
