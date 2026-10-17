(require '[babashka.fs :as fs] '[clojure.string :as str] '[clojure.edn :as edn])
(def root (fs/parent *file*))
(def destination (fs/path "benchmarks/results/compiler-2026-10-16/penpot-phase2"))
(doseq [name ["results.edn" "render.time.txt" "main-none.time.txt" "main-advanced.time.txt" "shadow-release.time.txt" "shadow-release.log" "shadow-compile.time.txt" "shadow-compile.log" "inputs-after.edn" "verification.edn" "probe-verification.log"]]
  (fs/copy (fs/path root name) (fs/path destination name) {:replace-existing true}))
(def records (mapv edn/read-string (str/split-lines (slurp (fs/file root "results.edn")))))
(doseq [r records]
  (println (:job r) (:wall-seconds r) (format "%.3f GiB" (/ (:peak-rss-bytes r) 1073741824.0))))
(let [by (into {} (map (juxt :job identity) records))]
  (doseq [[job original rss first] [["render" 214.98 4913905664 59.63] ["main-none" 2057.49 19960512512 525.39] ["main-advanced" 2043.0 19934527488 568.97]]]
    (let [r (by job)] (println job "speedup" (/ original (:wall-seconds r)) "memory-saving-percent" (* 100 (- 1 (/ (:peak-rss-bytes r) (double rss)))) "first-pass-speedup" (/ first (:wall-seconds r)))))
  (println "advanced/shadow-release" (/ (:wall-seconds (by "main-advanced")) (:wall-seconds (by "shadow-release"))))
  (println "none/shadow-dev" (/ (:wall-seconds (by "main-none")) (:wall-seconds (by "shadow-compile")))))
