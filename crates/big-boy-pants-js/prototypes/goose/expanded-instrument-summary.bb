#!/usr/bin/env bb
(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.edn :as edn] '[clojure.string :as str])
(def directory (fs/parent (fs/canonicalize *file*)))
(def input (or (first *command-line-args*) (str (fs/path directory "expanded-instrument-samples.edn"))))
(def content (if (fs/exists? input) (slurp input) (:out (p/shell {:out :string} "gzip" "-cd" (str input ".gz")))))
(def rows (mapv edn/read-string (str/split-lines content)))
(def jobs (reduce + (:phase-counts (first rows))))
(defn peak [intervals]
  (let [events (sort-by (juxt first second) (mapcat (fn [x] [[(:start-ns x) 1] [(:end-ns x) -1]]) (filter #(< (:start-ns %) (:end-ns %)) intervals)))
        active (reductions + (map second events))]
    (assert (every? #(>= % 0) active)) (assert (zero? (last active))) (apply max 0 active)))
(defn distribution [xs]
  (let [v (vec (sort xs)) at (fn [p] (nth v (min (dec (count v)) (long (Math/floor (* p (count v)))))))]
    {:count (count v) :min-ms (/ (first v) 1e6) :median-ms (/ (at 0.5) 1e6) :p90-ms (/ (at 0.9) 1e6) :max-ms (/ (last v) 1e6)}))
(doseq [r rows :when (and (= :sample (:kind r)) (= :goose (:engine r)) (= :branch (:control r)))]
  (let [intervals (filter #(= :interval (:kind %)) (:activity r))
        phase (fn [n] (filter #(= n (:phase %)) intervals))
        body (vec (phase 0)) restore (vec (phase 2)) vm (vec (phase 1)) saves (vec (phase 3))
        queue (filter #(= :native-queue (:kind %)) (:activity r))
        trace (get-in (some #(when (= :adapter-closed (:phase %)) %) (:rows r)) [:stats :trace])
        threads (last (filter #(str/starts-with? % "THREADS ") trace))
        scheduler (mapv #(mapv parse-long (drop 2 (str/split % #"\s+"))) (filter #(str/starts-with? % "SCHED ") trace))
        _ (assert (= (count queue) (count scheduler)))
        idle (frequencies
               (keep-indexed
                 (fn [i q]
                   (when (> (nth (nth scheduler i) 2) (if (zero? i) 0 (nth (nth scheduler (dec i)) 2)))
                     (cond
                       (or (pos? (:queued q)) (pos? (:ready-waiting q))) :native-capacity
                       (pos? (:running-macros q)) :macro-worker
                       (pos? (:running q)) :helper-worker
                       (pos? (:waiting q)) :compiler-helper-key
                       :else :input)))
                 (vec queue)))
        timeline (keep (fn [l] (let [p (str/split l #"\t")] (when (and (= 6 (count p)) (= "14" (nth p 3))) (mapv parse-long [(nth p 4) (nth p 5)])))) (str/split-lines (:timeline r)))
        duration (fn [iv] (map #(- (:end-ns %) (:start-ns %)) iv))]
    ;; Exact chunk counts and identities catch lost/partial append records.
    (assert (= (inc (* 2 jobs)) (count body) (count restore) (count (distinct (map :chunk body)))))
    (assert (= (* 2 jobs) (count vm))) (assert (= jobs (count saves)))
    (assert (= {0 jobs 1 jobs 2 1} (frequencies (map :mode body))))
    (assert (every? #(<= (:start-ns %) (:end-ns %)) intervals))
    (assert (seq queue)) (assert threads) (assert (seq timeline))
    (let [[_ _ created _ joined] (str/split threads #"\s+")]
      (assert (= (parse-long created) (inc (parse-long joined)) (+ 3 (* 2 jobs)))))
    (println (pr-str (merge (select-keys r [:case :workers])
      {:worker-body-peak (peak body) :vm-interval-peak (peak vm)
       :native-queued-peak (apply max (map :queued queue))
       :native-eligible-including-running-peak (apply max (map #(+ (:queued %) (:ready-waiting %) (:running-macros %)) queue))
       :native-waiting-peak (apply max (map :waiting queue))
       :idle-turn-reasons idle
       :compiler-runnable-peak (apply max (map first timeline)) :compiler-backlog-peak (apply max (map second timeline))
       :initial-vm (distribution (duration (filter #(zero? (:mode %)) vm)))
       :restore (distribution (duration restore)) :body (distribution (duration body))
       :save (distribution (duration saves)) :threads threads
       :final-scheduler (last (filter #(str/starts-with? % "SCHED ") trace))
       :native-resources (:native-resources r)})))))
