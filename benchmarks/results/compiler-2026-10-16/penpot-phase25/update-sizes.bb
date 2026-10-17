(require '[clojure.string :as str])
(def path "tests/support/ratchet_fixtures.rs")
(def sizes
  (into {} (for [[_ name a b] (re-seq #"(?m)^([a-z0-9-]+)\s+([\d,]+) / [\d,]+!\s+([\d,]+) / [\d,]+!"
                                    (slurp "/private/tmp/light-cljs-phase25/sizes.log"))]
             [name (mapv #(parse-long (str/replace % "," "")) [a b])])))
(assert (= 34 (count sizes)))
(defn ceiling [n] (str (quot (+ n 999) 1000) "_000"))
(def changed (atom 0))
(spit path
  (str/replace (slurp path)
    #"(?s)(name: \"([^\"]+)\",.*?ceilings: \[)([\d_]+), ([\d_]+), ([\d_]+)(\])"
    (fn [[all prefix name _ _ c suffix]]
      (if-let [[a b] (sizes name)]
        (do (swap! changed inc) (str prefix (ceiling a) ", " (ceiling b) ", " c suffix))
        all))))
(assert (= 34 @changed))
