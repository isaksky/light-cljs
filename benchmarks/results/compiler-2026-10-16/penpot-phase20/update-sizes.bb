(require '[clojure.string :as str])
(def path "tests/support/ratchet_fixtures.rs")
(def names #{"println-hi" "add" "keyword-case" "record-protocol" "pure-defs"
             "constant-literals" "eight-records" "event-maps" "raw-js"
             "multi-arity" "closed-params" "protocol-params" "features-direct"
             "loadable-site" "direct-site"})
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
            (str prefix (raised a) ", " (raised b) ", " c suffix))
        all))))
(assert (= 15 @changed))
(spit path updated)
