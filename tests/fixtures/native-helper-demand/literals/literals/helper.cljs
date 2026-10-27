(ns literals.helper)
(def log (atom []))
(defn fresh [] '#js [#js {:value "\uD800"}])
(defn emit [tag x]
  (swap! log conj tag)
  (list 'quote [(vec @log) (meta x) x (fresh)]))
