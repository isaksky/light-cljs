(require '[babashka.fs :as fs] '[clojure.string :as str])
(def here (fs/parent *file*))
(def records
  (for [file (sort (fs/glob here "*.time"))
        :let [text (slurp (str file))
              [_ real user system] (re-find #"([0-9.]+)\s+real\s+([0-9.]+)\s+user\s+([0-9.]+)\s+sys" text)
              label (str/replace (str (fs/file-name file)) #"\.time$" "")
              js (fs/file here (str label ".js"))]
        :when real]
    {:label label :wall-seconds (parse-double real) :user-seconds (parse-double user)
     :system-seconds (parse-double system)
     :peak-rss-bytes (parse-long (second (re-find #"(\d+)\s+maximum resident set size" text)))
     :output-bytes (fs/size js)
     :has-output? (pos? (fs/size js))
     :source-stats (re-find #"v8-source[^\n]+" text)
     :heap-stats (vec (re-seq #"v8-stats[^\n]+" text))}))
(spit (fs/file here "benchmark-summary.txt")
  (apply str (map #(str (pr-str %) "\n") records)))
(def profiles
  (for [file (fs/glob here "*profile.time")
        :let [rows (mapv (fn [[_ name stage n inclusive exclusive]]
                          {:macro name :stage stage :calls (parse-long n)
                           :inclusive (parse-double inclusive) :exclusive (parse-double exclusive)})
                     (re-seq #"PHASE35\t([^\t]+)\t([^\t]+)\t(\d+)\t([0-9.]+)\t([0-9.]+)" (slurp (str file))))]]
    (with-out-str
      (println (fs/file-name file))
      (println "Exclusive totals subtract nested instrumented stages only.")
      (doseq [row (sort-by :exclusive >
                    (for [[stage rs] (group-by :stage rows)]
                      {:stage stage :calls (reduce + (map :calls rs))
                       :inclusive (reduce + (map :inclusive rs))
                       :exclusive (reduce + (map :exclusive rs))}))]
        (prn row))
      (println "Top macro calls")
      (doseq [row (take 20 (sort-by :exclusive > (filter #(= "call" (:stage %)) rows)))]
        (prn row)))))
(spit (fs/file here "profile-summary.txt") (apply str profiles))
(println "Summarized" (count records) "trials")
