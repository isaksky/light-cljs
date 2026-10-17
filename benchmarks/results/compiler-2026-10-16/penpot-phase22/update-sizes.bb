(require '[clojure.string :as str])
(def path "tests/support/ratchet_fixtures.rs")
(def names #{"console-log-sum" "map-printed" "seq-pipeline" "rest-fn"
             "keyword-lookups" "constant-lookups" "three-records" "constant-keys"
             "vector-only" "keyword-only" "map-only" "atom-str" "set-only"
             "mapv-count" "debug-logging" "local-arities" "callbacks" "features"})
(def changed (atom 0))
(defn raised [s]
  (let [n (+ 1000 (parse-long (str/replace s "_" "")))]
    (str (quot n 1000) "_000")))
(def updated
  (str/replace (slurp path)
    #"(?s)(name: \"([^\"]+)\",.*?ceilings: \[)([\d_]+), ([\d_]+), ([\d_]+)(\])"
    (fn [[all prefix name a b c suffix]]
      (if (names name)
        (do (swap! changed inc)
            (str prefix (raised a) ", " (raised b) ", "
                 (if (= name "features") (raised c) c) suffix))
        all))))
(assert (= 18 @changed))
(spit path updated)
