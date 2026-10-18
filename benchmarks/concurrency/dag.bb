#!/usr/bin/env bb
;; Model parallel namespace compilation from NSPROBE lines.
(require '[clojure.string :as str])
(def lines (->> (str/split-lines (slurp (first *command-line-args*)))))
(def prep (into {} (for [l lines :when (str/starts-with? l "NSPREP\t")]
                     (let [[_ i us] (str/split l #"\t")] [(parse-long i) (parse-long us)]))))
(def nss (vec (for [l lines :when (str/starts-with? l "NSPROBE\t")]
                (let [[_ idx name bytes body jsb drain jsd warn units calls reqs unit-us] (str/split l #"\t" -1)
                      idx (parse-long idx)]
                  {:idx idx :name name :bytes (parse-long bytes)
                   :body (parse-long body) :js-body (parse-long jsb)
                   :drain (parse-long drain) :js-drain (parse-long jsd)
                   :warn (parse-long warn) :units (parse-long units) :calls (parse-long calls)
                   :prep (get prep idx 0)
                   :reqs (if (str/blank? reqs) [] (str/split reqs #","))
                   :unit-us (if (str/blank? unit-us) [] (mapv parse-long (str/split unit-us #",")))}))))
(def by-name (into {} (map (juxt :name :idx) nss)))
(def nss (mapv (fn [n] (assoc n :deps (vec (distinct (keep by-name (:reqs n))))
                              :cost (+ (:body n) (:drain n) (:warn n) (:prep n)))) nss))
(def N (count nss))
(defn s [us] (format "%.2f s" (/ us 1e6)))
(def tot (fn [k] (reduce + (map k nss))))
(println "namespaces" N)
(println "sum cost" (s (tot :cost)) " body" (s (tot :body)) " (js" (s (tot :js-body)) ") drain" (s (tot :drain)) " (js" (s (tot :js-drain)) ") warn" (s (tot :warn)) " prep" (s (tot :prep)))
(println "units" (tot :units) " js calls" (tot :calls))
(println "\ntop 25 namespaces by cost:")
(doseq [n (take 25 (sort-by (comp - :cost) nss))]
  (println (format "%8.3f s  body %6.3f (js %6.3f) drain %6.3f (js %6.3f) units %4d maxunit %6.3f  %s"
                   (/ (:cost n) 1e6) (/ (:body n) 1e6) (/ (:js-body n) 1e6) (/ (:drain n) 1e6) (/ (:js-drain n) 1e6)
                   (:units n) (/ (reduce max 0 (:unit-us n)) 1e6) (:name n))))
;; critical path: namespace granularity
(defn longest [cost-fn start-fn]
  ;; finish[i] = max(start constraint) + cost
  (reduce (fn [fin n]
            (let [ready (reduce max 0 (map #(start-fn fin %) (:deps n)))]
              (assoc fin (:idx n) (+ ready (cost-fn n)))))
          {} nss))
(def fin (longest :cost (fn [fin d] (fin d))))
(println "\ncritical path (ns granularity, deps complete):" (s (reduce max (vals fin))))
;; finer: dependents wait only for the dependency's top-level pass (body+prep);
;; drain of each ns can run its units in parallel (unlimited), bounded by the max unit.
(def body-fin
  (reduce (fn [acc n]
            (let [ready (reduce max 0 (map #(get-in acc [% :body]) (:deps n)))
                  b (+ ready (:prep n) (:body n))
                  drained (+ b (reduce max 0 (:unit-us n)) (:warn n))]
              (assoc acc (:idx n) {:body b :all drained})))
          {} nss))
(println "critical path (deps wait for top-level pass only; units fully parallel):"
         (s (reduce max (map :all (vals body-fin)))))
(def body-fin2
  (reduce (fn [acc n]
            (let [ready (reduce max 0 (map #(get-in acc [% :body]) (:deps n)))
                  b (+ ready (:prep n) (:body n))
                  drained (+ b (:drain n) (:warn n))]
              (assoc acc (:idx n) {:body b :all drained})))
          {} nss))
(println "critical path (deps wait for top-level pass only; units serial per ns):"
         (s (reduce max (map :all (vals body-fin2)))))
;; list scheduling simulation at namespace granularity
(def dependents (reduce (fn [m n] (reduce #(update %1 %2 (fnil conj []) (:idx n)) m (:deps n))) {} nss))
(def bottom ;; longest path to sink, for priority
  (reduce (fn [acc i]
            (let [n (nss i)]
              (assoc acc i (+ (:cost n) (reduce max 0 (map acc (dependents i [])))))))
          {} (range (dec N) -1 -1)))
(defn simulate [P prio]
  (loop [t 0 ready (sorted-set-by (fn [a b] (compare [(prio a) a] [(prio b) b])))
         remaining (into {} (map (juxt :idx (comp count :deps)) nss))
         running [] done 0 first? true]
    (let [ready (if first? (into ready (keep (fn [[i c]] (when (zero? c) i)) remaining)) ready)]
      (if (= done N) t
          (let [free (- P (count running))
                start (take free ready)
                ready (reduce disj ready start)
                running (into running (map (fn [i] [(+ t (:cost (nss i))) i]) start))
                running (sort running)
                [[tf i] & rest] running
                newly (filter (fn [d] (= 1 (remaining d))) (dependents i []))
                remaining (reduce (fn [r d] (update r d dec)) remaining (dependents i []))]
            (recur tf (into ready newly) remaining (vec rest) (inc done) false))))))
(println "\nsimulated namespace-granularity schedule (critical-path-first):")
(doseq [P [1 2 4 6 8 12 16 24]]
  (let [t (simulate P (fn [i] (- (bottom i))))]
    (println (format "  P=%2d  %s  speedup %.2fx" P (s t) (/ (double (tot :cost)) t)))))
(println "simulated (source order priority):")
(doseq [P [2 4 8 16]]
  (let [t (simulate P identity)]
    (println (format "  P=%2d  %s  speedup %.2fx" P (s t) (/ (double (tot :cost)) t)))))
;; depth histogram
(def level (reduce (fn [acc n] (assoc acc (:idx n) (inc (reduce max -1 (map acc (:deps n)))))) {} nss))
(println "\nDAG depth (longest require chain):" (inc (reduce max (vals level))))
(println "namespaces per level:" (frequencies (vals level)))
(println "avg deps" (format "%.1f" (/ (double (reduce + (map (comp count :deps) nss))) N)))
(def units (mapcat :unit-us nss))
(println "\nunits" (count units) " total" (s (reduce + units)) " max" (s (reduce max units))
         " >100ms:" (count (filter #(> % 100000) units)) " >10ms:" (count (filter #(> % 10000) units))
         " >1ms:" (count (filter #(> % 1000) units)))
(println "top units (s):" (map #(format "%.3f" (/ % 1e6)) (take 15 (sort > units))))
;; critical chain (ns granularity)
(defn chain [fin]
  (loop [i (key (apply max-key val fin)) acc []]
    (let [n (nss i) d (when (seq (:deps n)) (apply max-key fin (:deps n)))]
      (if d (recur d (conj acc i)) (conj acc i)))))
(println "\ncritical chain (ns granularity), length" (count (chain fin)) ":")
(doseq [i (take 12 (chain fin))] (println (format "   %7.3f s finish %7.3f s  %s" (/ (:cost (nss i)) 1e6) (/ (fin i) 1e6) (:name (nss i)))))
;; same without the instaparse.abnf outlier unit (pretend it costs as its body)
(def fin-no (longest (fn [n] (if (= (:name n) "instaparse.abnf") (:body n) (:cost n))) (fn [fin d] (fin d))))
(println "critical path without the instaparse.abnf unit:" (s (reduce max (vals fin-no))))
(println "critical chain without it:")
(doseq [i (take 8 (chain fin-no))] (println (format "   %7.3f s finish %7.3f s  %s" (/ (:cost (nss i)) 1e6) (/ (fin-no i) 1e6) (:name (nss i)))))
;; Model C: tasks = ns top-level pass + each unit. body(B) after body(deps); units(B) after body(B).
(defn simulate-c [P skip-outlier?]
  (let [tasks (vec (concat
                    (for [n nss] {:id [:b (:idx n)] :cost (+ (:prep n) (:body n) (:warn n))
                                  :deps (mapv (fn [d] [:b d]) (:deps n))})
                    (for [n nss [k u] (map-indexed vector (:unit-us n))]
                      {:id [:u (:idx n) k] :cost (if (and skip-outlier? (= (:name n) "instaparse.abnf")) 0 u)
                       :deps [[:b (:idx n)]]})))
        idx (into {} (map-indexed (fn [i t] [(:id t) i]) tasks))
        tasks (mapv (fn [t] (update t :deps #(mapv idx %))) tasks)
        M (count tasks)
        dependents (reduce (fn [m [i t]] (reduce #(update %1 %2 (fnil conj []) i) m (:deps t))) {} (map-indexed vector tasks))
        ;; bottom levels: process in reverse topological order; ids are topological since ns order is dep order and units follow
        order (concat (map idx (for [n (reverse nss) [k _] (reverse (map-indexed vector (:unit-us n)))] [:u (:idx n) k]))
                      (map idx (for [n (reverse nss)] [:b (:idx n)])))
        ;; simple: compute bottom by iterating until fixpoint on reverse topo order (units first then bodies reversed)
        bottom (reduce (fn [acc i] (assoc acc i (+ (:cost (tasks i)) (reduce max 0 (map #(get acc % 0) (dependents i [])))))) {} order)
        prio (fn [i] (- (bottom i)))]
    (loop [t 0 ready (into (sorted-set-by (fn [a b] (compare [(prio a) a] [(prio b) b])))
                           (keep (fn [[i tk]] (when (empty? (:deps tk)) i)) (map-indexed vector tasks)))
           remaining (into {} (map-indexed (fn [i tk] [i (count (:deps tk))]) tasks))
           running (sorted-set) done 0]
      (if (= done M) t
          (let [free (- P (count running))
                start (take free ready)
                ready (reduce disj ready start)
                running (into running (map (fn [i] [(+ t (:cost (tasks i))) i]) start))
                [tf i :as head] (first running)
                running (disj running head)
                ds (dependents i [])
                newly (filter (fn [d] (= 1 (remaining d))) ds)
                remaining (reduce (fn [r d] (update r d dec)) remaining ds)]
            (recur tf (into ready newly) remaining running (inc done)))))))
(println "\nModel C (ns top-level pass serial per ns; units parallel; deps wait for top-level pass):")
(doseq [P [1 2 4 8 12 16 24]]
  (let [t (simulate-c P false) t2 (simulate-c P true)]
    (println (format "  P=%2d  %s (%.2fx)   without instaparse outlier %s (%.2fx)" P (s t) (/ (double (tot :cost)) t) (s t2) (/ (- (double (tot :cost)) 2.0e6) t2)))))
(println "\nModel A without outlier:")
(def bottom-bytes
  (reduce (fn [acc i] (let [n (nss i)] (assoc acc i (+ (:bytes n) (reduce max 0 (map acc (dependents i []))))))) {} (range (dec N) -1 -1)))
(def bottom-count
  (reduce (fn [acc i] (assoc acc i (+ 1 (reduce max 0 (map acc (dependents i [])))))) {} (range (dec N) -1 -1)))
(println "\nnamespace-granularity schedule with static priorities:")
(doseq [P [4 8 12 16]]
  (println (format "  P=%2d  bytes-path %s   depth-path %s   exact-cost %s" P
                   (s (simulate P (fn [i] (- (bottom-bytes i)))))
                   (s (simulate P (fn [i] (- (bottom-count i)))))
                   (s (simulate P (fn [i] (- (bottom i))))))))
(let [xs (map (fn [n] [(double (:bytes n)) (double (:cost n))]) nss)
      mx (/ (reduce + (map first xs)) N) my (/ (reduce + (map second xs)) N)
      cov (reduce + (map (fn [[x y]] (* (- x mx) (- y my))) xs))
      vx (reduce + (map (fn [[x _]] (* (- x mx) (- x mx))) xs))
      vy (reduce + (map (fn [[_ y]] (* (- y my) (- y my))) xs))]
  (println "correlation(bytes, cost)" (format "%.2f" (/ cov (Math/sqrt (* vx vy))))))
(println "sum body (top-level passes)" (s (tot :body)) " critical path of top-level passes only:"
         (s (reduce max (map :body (vals body-fin)))))
;; Option U: top-level passes serial in source order on a coordinator; units fan out to W workers.
(defn simulate-u [W]
  (let [body-done (reductions + (map #(+ (:prep %) (:body %) (:warn %)) nss))
        arrivals (sort-by first (for [[n t] (map vector nss body-done) u (:unit-us n)] [t u]))
        workers (reduce (fn [ws [t u]]
                          (let [[free & more] ws]
                            (vec (sort (conj (vec more) (+ (max free t) u))))))
                        (vec (repeat W 0)) arrivals)]
    (max (last body-done) (peek workers))))
(println "\nOption U (serial top-level passes; units on W workers):")
(doseq [W [1 2 4 8 12 16]] (println (format "  W=%2d  %s" W (s (simulate-u W)))))
;; Model D: in-order commit without overlays. B's top-level pass starts once every namespace
;; up to its latest dependency (in source order) has finished its top-level pass; units fan out.
(let [maxdep (mapv (fn [n] (reduce max -1 (:deps n))) nss)
      top (fn [n] (+ (:prep n) (:body n) (:warn n)))
      ;; unlimited workers
      res (reduce (fn [{:keys [fin commit all]} n]
                    (let [i (:idx n) md (maxdep i)
                          start (if (neg? md) 0 (commit md))
                          f (+ start (top n))
                          c (max (if (zero? i) 0 (commit (dec i))) f)
                          done (+ f (reduce max 0 (:unit-us n)))]
                      {:fin (assoc fin i f) :commit (assoc commit i c) :all (max all done)}))
                  {:fin {} :commit {} :all 0} nss)]
  (println "\nModel D (in-order commit, no overlays), unlimited workers:" (s (:all res))
           " commit frontier reaches the end at" (s ((:commit res) (dec N)))))
(defn schedule [tasks P]
  ;; tasks: vector of {:cost :deps [indices]} in topological order; list scheduling, bottom-level priority
  (let [M (count tasks)
        dependents (reduce (fn [m [i t]] (reduce #(update %1 %2 (fnil conj []) i) m (:deps t))) {} (map-indexed vector tasks))
        bottom (reduce (fn [acc i] (assoc acc i (+ (:cost (tasks i)) (reduce max 0 (map #(get acc % 0) (dependents i [])))))) {} (range (dec M) -1 -1))
        prio (fn [i] (- (bottom i)))]
    (loop [t 0 ready (into (sorted-set-by (fn [a b] (compare [(prio a) a] [(prio b) b])))
                           (keep (fn [[i tk]] (when (empty? (:deps tk)) i)) (map-indexed vector tasks)))
           remaining (into {} (map-indexed (fn [i tk] [i (count (:deps tk))]) tasks))
           running (sorted-set) done 0]
      (if (= done M) t
          (let [free (- P (count running))
                start (take free ready)
                ready (reduce disj ready start)
                running (into running (map (fn [i] [(+ t (:cost (tasks i))) i]) start))
                [tf i :as head] (first running)
                running (disj running head)
                ds (dependents i [])
                newly (filter (fn [d] (= 1 (remaining d))) ds)
                remaining (reduce (fn [r d] (update r d dec)) remaining ds)]
            (recur tf (into ready newly) remaining running (inc done)))))))
;; Model D tasks, topological layout per namespace i: commit(i-1).. body(i), commit(i), units(i)
(let [maxdep (mapv (fn [n] (reduce max -1 (:deps n))) nss)
      layout (reduce (fn [{:keys [tasks body commit]} n]
                       (let [i (:idx n)
                             b (count tasks)
                             md (maxdep i)
                             tasks (conj tasks {:cost (+ (:prep n) (:body n) (:warn n))
                                                :deps (if (neg? md) [] [(commit md)])})
                             c (count tasks)
                             tasks (conj tasks {:cost 0 :deps (cond-> [b] (pos? i) (conj (commit (dec i))))})
                             tasks (into tasks (map (fn [u] {:cost u :deps [b]}) (:unit-us n)))]
                         {:tasks tasks :body (assoc body i b) :commit (assoc commit i c)}))
                     {:tasks [] :body {} :commit {}} nss)
      tasks (:tasks layout)]
  (println "\nModel D (in-order commit) with P workers:")
  (doseq [P [4 8 12 16]] (println (format "  P=%2d  %s" P (s (schedule tasks P))))))
