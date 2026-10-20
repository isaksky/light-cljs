#!/usr/bin/env bb
;; Model parallel namespace compilation from NSPROBE lines, or from the trace
;; `light-cljs --compile --stats-file PATH` writes (light-cljs-331).
;;
;; Usage: bb benchmarks/concurrency/dag.bb FILE [--workers P,P...]
;;
;; FILE is a probe run (`NSPREP`/`NSPROBE` lines, as in main-probe.tsv) or a
;; `light-cljs-trace` file (version 1, or 2 with `read` rows). From a trace
;; the namespace columns the models read are rebuilt as the probes measured
;; them: a top-level pass includes the units it demanded, a drain includes
;; the units it analyzed, and each drained unit includes the units it
;; demanded. A trace also prints its exclusive stage totals, demand edges
;; (root evaluations and, from version 2, root reads), macro-host share and
;; the ready width of each namespace's drain (the last section; --workers
;; sets its P values), and runs Option U (stage A) again with the root
;; evaluations and reads as dependencies.
(require '[clojure.string :as str])
(def args *command-line-args*)
(def workers (or (some->> (drop-while #(not= "--workers" %) args) second (#(str/split % #",")) (mapv parse-long))
                 [4 8 12]))
(def lines (->> (str/split-lines (slurp (first args)))))
(def trace? (str/starts-with? (first lines) "light-cljs-trace\t"))
(def trace-version (when trace? (parse-long (second (str/split (first lines) #"\t")))))
(defn rows [kind]
  (for [l lines :when (str/starts-with? l (str kind "\t"))]
    (vec (rest (str/split l #"\t" -1)))))
(def trace-units
  (when trace?
    (vec (for [[id ns name line column us engine calls wait exp nodes host kind demander depth thread] (rows "unit")]
           {:id (parse-long id) :ns (parse-long ns) :name name :line (parse-long line) :column (parse-long column)
            :us (parse-long us) :engine (parse-long engine) :calls (parse-long calls) :wait (parse-long wait)
            :expansions (parse-long exp) :nodes (parse-long nodes) :host (= host "1")
            :kind kind :demander (parse-long demander) :depth (parse-long depth) :thread (parse-long thread)}))))
(def unit-by-id (into {} (map (juxt :id identity) trace-units)))
(defn inclusive [u] (+ (:us u) (:wait u)))
;; Root demand edges: each first evaluation of a unit's compile-time root
;; (`execute`) and each distinct read of a root or bundle (`read`, light-cljs-359),
;; with the innermost frame they were made in.
(def trace-reads
  (when trace?
    (vec (for [[unit kind demander] (rows "read")]
           {:unit (parse-long unit) :kind kind :demander (parse-long demander)}))))
(def trace-executions
  (when trace?
    (vec (for [[unit kind demander] (rows "execute")]
           {:unit (parse-long unit) :kind kind :demander (parse-long demander)}))))
;; The task of the stage A model whose cost includes a unit: the drained unit
;; whose analysis demanded it, or the top-level pass that did.
(defn task-of [u]
  (loop [u u]
    (case (:kind u)
      "unit" (some-> (unit-by-id (:demander u)) recur)
      "drain" [:u (:id u)]
      "top-level" [:b (:demander u)]
      nil)))
;; A root edge as [demanding task, task it waits for], or nil when either end
;; is outside the model (another stage's frame, a unit installed from an
;; artifact) or both are one task.
(defn root-edge [{:keys [unit kind demander]}]
  (let [from (case kind "unit" (some-> (unit-by-id demander) task-of) "top-level" [:b demander] nil)
        to (some-> (unit-by-id unit) task-of)]
    (when (and from to (not= from to)) [from to])))
;; The namespace pass (top-level or drain) whose demand chain led to a unit.
(defn root-of [u]
  (loop [u u]
    (if (= (:kind u) "unit")
      (if-let [parent (unit-by-id (:demander u))] (recur parent) [(:kind u) (:demander u)])
      [(:kind u) (:demander u)])))
(def subtree-engine
  (when trace?
    (reduce (fn [m u] (update m (root-of u) (fnil + 0) (:engine u))) {} trace-units)))
(def subtree-calls
  (when trace?
    (reduce (fn [m u] (update m (root-of u) (fnil + 0) (:calls u))) {} trace-units)))
(def prep (if trace?
            (into {} (for [[i _ _ _ p] (rows "namespace")] [(parse-long i) (parse-long p)]))
            (into {} (for [l lines :when (str/starts-with? l "NSPREP\t")]
                       (let [[_ i us] (str/split l #"\t")] [(parse-long i) (parse-long us)])))))
(def nss
  (if trace?
    (let [by-ns (group-by :ns trace-units)
          drained (group-by :demander (filter #(= "drain" (:kind %)) trace-units))]
      (vec (for [[i name bytes _load _prep top top-engine top-wait drain drain-engine warn _exp _installed reqs]
                 (rows "namespace")
                 :let [idx (parse-long i)]]
             {:idx idx :name name :bytes (parse-long bytes)
              :body (+ (parse-long top) (parse-long top-wait))
              :js-body (+ (parse-long top-engine) (get subtree-engine ["top-level" idx] 0))
              :drain (+ (parse-long drain) (reduce + (map inclusive (drained idx))))
              :js-drain (+ (parse-long drain-engine) (get subtree-engine ["drain" idx] 0))
              :warn (parse-long warn) :units (count (by-ns idx))
              :calls (+ (get subtree-calls ["top-level" idx] 0) (get subtree-calls ["drain" idx] 0))
              :prep (get prep idx 0)
              :reqs (if (str/blank? reqs) [] (str/split reqs #","))
              :unit-us (mapv inclusive (drained idx))})))
    (vec (for [l lines :when (str/starts-with? l "NSPROBE\t")]
           (let [[_ idx name bytes body jsb drain jsd warn units calls reqs unit-us] (str/split l #"\t" -1)
                 idx (parse-long idx)]
             {:idx idx :name name :bytes (parse-long bytes)
              :body (parse-long body) :js-body (parse-long jsb)
              :drain (parse-long drain) :js-drain (parse-long jsd)
              :warn (parse-long warn) :units (parse-long units) :calls (parse-long calls)
              :prep (get prep idx 0)
              :reqs (if (str/blank? reqs) [] (str/split reqs #","))
              :unit-us (if (str/blank? unit-us) [] (mapv parse-long (str/split unit-us #",")))})))))
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
(println "\nunits" (count units) " total" (s (reduce + units)) " max" (s (reduce max 0 units))
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
  (println "correlation(bytes, cost)" (if (pos? (* vx vy)) (format "%.2f" (/ cov (Math/sqrt (* vx vy)))) "-")))
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
;; Option U with the trace's root edges: a drained unit also waits for the
;; drained units whose roots it evaluated or read, and a top-level pass for
;; those its own code did. The trace lists units in the order they finished,
;; which every such edge respects, so one pass in that order schedules them.
(when (seq trace-units)
  (let [edges (distinct (keep root-edge (concat trace-executions trace-reads)))
        waits (reduce (fn [m [from to]] (update m from (fnil conj []) to)) {} edges)
        drained (group-by :demander (filter #(= "drain" (:kind %)) trace-units))
        simulate (fn [W]
                   (loop [nss nss t 0 workers (vec (repeat W 0)) fin {}]
                     (if-let [n (first nss)]
                       (let [i (:idx n)
                             done (fn [fin task] (case (first task) :u (get fin task 0) :b 0))
                             start (reduce max t (map #(done fin %) (waits [:b i])))
                             body (+ start (:prep n) (:body n) (:warn n))
                             [workers fin]
                             (reduce (fn [[workers fin] u]
                                       (let [task [:u (:id u)]
                                             ready (reduce max body (map #(done fin %) (waits task)))
                                             [free & more] workers
                                             end (+ (max free ready) (inclusive u))]
                                         [(vec (sort (conj (vec more) end))) (assoc fin task end)]))
                                     [workers fin] (drained i))]
                         (recur (rest nss) body workers fin))
                       (max t (peek workers)))))]
    (println (format "Option U with root edges (%d edges between tasks, %d of them from a top-level pass):"
                     (count edges) (count (filter #(= :b (ffirst %)) edges))))
    (doseq [W [1 2 4 8 12 16]] (println (format "  W=%2d  %s" W (s (simulate W)))))))
;; Option U with a fixed lag (light-cljs-391): the coordinator runs the
;; top-level passes in source order and gives each namespace's units to the
;; W workers, which take them earliest namespace first; it drains namespace
;; k right after the top-level pass of k+L, waiting there for k's units, so
;; the pass of k+L+1 starts only once they are done. Lag 0 waits at each
;; namespace's own drain, as light-cljs-339 built it. Units cost their
;; serial time times the inflation (light-cljs-343 measured worker units at
;; 1.38x their one-worker time). `held` is the most units whose namespaces
;; wait for their drain point at once (at the first W), a proxy for the
;; queued IR.
(when trace?
  (let [simulate (fn [W L inflation]
                   (loop [j 0 t 0.0 workers (vec (repeat W 0.0)) done [] held 0]
                     (if (< j N)
                       (let [n (nss j)
                             t (+ t (:prep n) (:body n) (:warn n))
                             [workers finish]
                             (reduce (fn [[workers finish] u]
                                       (let [[free & more] workers
                                             end (+ (max free t) (* inflation u))]
                                         [(vec (sort (conj (vec more) end))) (max finish end)]))
                                     [workers t] (:unit-us n))
                             done (conj done finish)
                             drained (- j L)
                             held (max held (reduce + (map #(:units (nss %)) (range (max 0 drained) (inc j)))))
                             t (if (>= drained 0) (max t (done drained)) t)]
                         (recur (inc j) t workers done held))
                       [(reduce max t done) held])))]
    (doseq [inflation [1.0 1.38]]
      (println (format "\nOption U with a fixed lag L (drain k after the top-level pass of k+L), units at %.2fx:" inflation))
      (doseq [L (distinct (map #(min % N) [0 1 2 3 4 6 8 16 N]))]
        (println (format "  L=%-4s %s  held units %d"
                         (if (= L N) "all" (str L))
                         (str/join "  " (for [W workers] (format "W=%2d %s" W (s (first (simulate W L inflation))))))
                         (second (simulate (first workers) L inflation))))))))
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
;; The trace's own figures (light-cljs-331).
(when trace?
  (let [ms (fn [us] (format "%.1f ms" (/ us 1e3)))
        pct (fn [a b] (if (pos? b) (format "%.1f%%" (* 100.0 (/ (double a) b))) "-"))
        stages (rows "stage")
        units trace-units
        unit-time (reduce + (map :us units))
        ns-name (into {} (map (juxt :idx :name) nss))]
    (println "\ntrace: exclusive stage totals (each moment counts once)")
    (doseq [[name us engine calls] stages]
      (println (format "  %-20s %12s  engine %12s  host entries %s" name (ms (parse-long us)) (ms (parse-long engine)) calls)))
    (println "\ntrace: demand edges")
    (doseq [[kind group] (sort-by key (group-by :kind units))]
      (println (format "  demanded by %-10s %6d units, %s inclusive" kind (count group) (s (reduce + (map inclusive group))))))
    (println "  deepest demand chain" (reduce max 0 (map :depth units)) "  threads" (sort (distinct (map :thread units))))
    ;; First evaluations of compile-time roots, then every distinct read of
    ;; a root or bundle (version 2), the first evaluations included.
    (let [executions (rows "execute")
          unit-ns (into {} (map (juxt :id :ns) units))
          own-ns (fn [kind demander] (case kind ("top-level" "drain") (parse-long demander) "unit" (unit-ns (parse-long demander)) nil))
          ;; A demander outside any pass crosses, as in the --stats count.
          crosses? (fn [[unit kind demander]] (let [own (own-ns kind demander)] (or (nil? own) (not= own (unit-ns (parse-long unit))))))]
      (println (format "  root evaluations %d (%d analyzed on demand); by %s; %d evaluate a unit of another namespace"
                       (count executions) (count (filter #(= "0" (nth % 3)) executions))
                       (into (sorted-map) (frequencies (map second executions)))
                       (count (filter crosses? executions))))
      (when (>= trace-version 2)
        (let [reads (rows "read")]
          (println (format "  root reads %d distinct (demander, unit) pairs; by %s; %d read a unit of another namespace; %d demanders read %d distinct units"
                           (count reads) (into (sorted-map) (frequencies (map second reads)))
                           (count (filter crosses? reads))
                           (count (distinct (map rest reads))) (count (distinct (map first reads))))))))
    (let [host (filter :host units)
          plain (remove #(or (:host %) (pos? (:expansions %))) units)]
      (println "\ntrace: macro host")
      (println (format "  units that entered the host %d of %d (%s); their exclusive time %s of %s (%s)"
                       (count host) (count units) (pct (count host) (count units))
                       (s (reduce + (map :us host))) (s unit-time) (pct (reduce + (map :us host)) unit-time)))
      (println (format "  units with no macro call at all, native ones included: %d (%s)"
                       (count plain) (s (reduce + (map :us plain))))))
    (when-let [longest (when (seq units) (apply max-key inclusive units))]
      (println (format "\ntrace: longest unit %s (%s), %s inclusive, %s exclusive, host %s, demanded by %s"
                       (:name longest) (ns-name (:ns longest)) (s (inclusive longest)) (s (:us longest))
                       (:host longest) (:kind longest)))
      (println "  top units by inclusive time:")
      (doseq [u (take 10 (sort-by (comp - inclusive) units))]
        (println (format "   %8.3f s  excl %8.3f s  host %-5s %-9s %s:%d" (/ (inclusive u) 1e6) (/ (:us u) 1e6)
                         (:host u) (:kind u) (:name u) (:line u)))))
    ;; Ready width: the units each drain started, all ready at once.
    (let [drains (->> units (filter #(= "drain" (:kind %))) (group-by :demander)
                      (map (fn [[ns us]]
                             (let [free (remove :host us) host (filter :host us)]
                               {:ns ns :width (count us) :free (count free) :host (count host)
                                :work (reduce + (map inclusive us)) :free-work (reduce + (map inclusive free))
                                :longest (reduce max 0 (map inclusive us))
                                :free-longest (reduce max 0 (map inclusive free))
                                :host-work (reduce + (map inclusive host))})))
                      (sort-by :ns) vec)
          q (fn [xs p] (let [v (vec (sort xs))] (if (seq v) (v (min (dec (count v)) (int (* p (count v))))) 0)))]
      (println "\ntrace: ready width per namespace drain (units the drain started; macro-free = did not enter the host)")
      (println (format "  drains %d; width median %d, p90 %d, max %d; macro-free median %d, p90 %d, max %d"
                       (count drains) (q (map :width drains) 0.5) (q (map :width drains) 0.9) (reduce max 0 (map :width drains))
                       (q (map :free drains) 0.5) (q (map :free drains) 0.9) (reduce max 0 (map :free drains))))
      (println (format "  drained units %d (%s): macro-free %d (%s), host %d (%s)"
                       (reduce + (map :width drains)) (s (reduce + (map :work drains)))
                       (reduce + (map :free drains)) (s (reduce + (map :free-work drains)))
                       (reduce + (map :host drains)) (s (reduce + (map :host-work drains)))))
      (println "  widest drains:")
      (doseq [d (take 10 (sort-by (comp - :width) drains))]
        (println (format "   %4d units (%4d macro-free, %4d host) %8.3f s  longest %7.3f s  %s"
                         (:width d) (:free d) (:host d) (/ (:work d) 1e6) (/ (:longest d) 1e6) (ns-name (:ns d)))))
      ;; Lower bounds with a barrier at each drain: the serial rest, plus per
      ;; drain max(W/P, longest unit) (docs/compiler-concurrency.md, "Ready width").
      (let [total (tot :cost)
            drained (reduce + (map :work drains))
            free (reduce + (map :free-work drains))]
        (println "\n  drain-barrier lower bound, all drained units parallel / only macro-free ones:")
        (doseq [P workers]
          (let [all (+ (- total drained) (reduce + (map #(max (/ (:work %) (double P)) (:longest %)) drains)))
                some (+ (- total free) (reduce + (map #(max (/ (:free-work %) (double P)) (:free-longest %)) drains)))]
            (println (format "  P=%2d  %s   %s   (serial %s)" P (s all) (s some) (s total)))))))))
