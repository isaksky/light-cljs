(ns string-join.main (:require [clojure.string :as s :refer [join]]))

;; The complete original SCI source is retained byte-for-byte inside this body.
(defn original-five [] [(clojure.string/join [nil false "x" \a]) (clojure.string/join "," [nil 1 :x]) (clojure.string/join nil [1 2]) (clojure.string/join \- [1 2]) (clojure.string/join :x [1 2])])

(defn join-contract []
  (let [saved join
        public (get (ns-publics 'clojure.string) 'join)
        seen (atom [])
        element (fn [n] (js-obj "toString" (fn [] (swap! seen conj ["element" n]) (str n))))
        separator (js-obj "valueOf" (fn [] (throw (js/Error. "separator must use string hint"))) "toString" (fn [] (swap! seen conj ["separator"]) "|"))
        values (letfn [(step [n] (lazy-seq (when (< n 3) (swap! seen conj ["next" n]) (cons (element n) (step (inc n))))))] (step 0))]
    (assert (= ["falsexa" ",1,:x" "1null2" "1-2" "1:x2"] (original-five)))
    (assert (= ["" "" "" "" "" "" "" "x"]
               [(join nil) (join []) (join nil nil) (join nil [])
                (join false []) (join :x []) (join nil [nil]) (join nil ["x"])]))
    (assert (= ["falsexa" "1null2null" "1false2false" "1,2," "1-2-" "1:x2:x"]
               [(join [nil false "x" \a]) (join nil [1 2 nil]) (join false [1 2 nil])
                (join "," [1 2 nil]) (join \- [1 2 nil]) (join :x [1 2 nil])]))
    (assert (= "1null2" (clojure.string/join nil [1 2]) (s/join nil [1 2])
               (saved nil [1 2]) (public nil [1 2]) (apply saved [nil [1 2]])))
    (assert (= "12" (saved [1 2]) (public [1 2]) (apply saved [[1 2]])))
    (assert (= "\uD800\uD83D\uDE00\uDC00" (join "\uD83D\uDE00" ["\uD800" "\uDC00"])))
    (assert (= "0|1|2" (join separator values)))
    (assert (= [["next" 0] ["element" 0] ["next" 1] ["separator"]
                ["element" 1] ["next" 2] ["separator"] ["element" 2]] @seen))
    (reset! seen [])
    (letfn [(step [n] (lazy-seq (when (< n 3) (swap! seen conj ["next" n]) (cons (element n) (step (inc n))))))]
      (assert (= "012" (join (step 0)))))
    (assert (= [["next" 0] ["element" 0] ["next" 1] ["element" 1]
                ["next" 2] ["element" 2]] @seen))
    (reset! seen [])
    (assert (= "" (join separator [])))
    (assert (= "0" (join separator [(element 0)])))
    (assert (= [["element" 0]] @seen))
    (assert (= ["Invalid arity: 0" "Invalid arity: 3"]
               (mapv (fn [args] (try (apply saved args) "unexpected success" (catch :default e (.-message e))))
                     [[] ["," [1 2] :extra]])))
    true))

(defn run [] (join-contract))
