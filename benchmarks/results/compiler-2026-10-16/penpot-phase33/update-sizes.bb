(require '[babashka.fs :as fs] '[clojure.string :as str])
(def here (fs/parent *file*))
(fs/copy (fs/file here "production/tests.log") (fs/file here "tests-before-size-update.log") {:replace-existing true})
(def rows
  (into {} (for [[_ name a ac am b bc bm c cc cm]
                 (re-seq #"(?m)^([a-z0-9-]+)\s+([\d,]+) / ([\d,]+)(!?)\s+([\d,]+) / ([\d,]+)(!?)\s+([\d,]+) / ([\d,]+)(!?)"
                         (slurp (fs/file here "tests-before-size-update.log")))]
             [name [[a am] [b bm] [c cm]]])))
(def changed (atom 0))
(def path "tests/support/ratchet_fixtures.rs")
(defn raised [old [size marker]]
  (if (= marker "!")
    (do (swap! changed inc)
        (str (quot (+ 999 (parse-long (str/replace size "," ""))) 1000) "_000"))
    old))
(spit path
  (str/replace (slurp path)
    #"(?s)(name: \"([^\"]+)\",.*?ceilings: \[)([\d_]+), ([\d_]+), ([\d_]+)(\])"
    (fn [[all prefix name a b c suffix]]
      (if-let [sizes (rows name)]
        (str prefix (str/join ", " (map raised [a b c] sizes)) suffix)
        all))))
(assert (pos? @changed))
(println "Raised" @changed "exceeded ceilings")
