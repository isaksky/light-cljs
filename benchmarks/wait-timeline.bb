#!/usr/bin/env bb
;; Summarize a wait timeline (src/wait_timeline.rs) of one build: what each
;; thread did, what it waited for, what the coordinator's waits were caused
;; by, and the critical path (light-cljs-573,
;; docs/shared-heap-compilation.md, "Why the Pants build leaves cores idle").
;;
;;   LIGHT_CLJS_WAIT_TIMELINE=/tmp/t.tsv light-cljs release bench --no-cache --stats --jobs 4
;;   bb benchmarks/wait-timeline.bb /tmp/t.tsv [--one-thread-wall SECONDS] [--threads N]
;;
;; Times are seconds. Each moment of a thread is given to its innermost open
;; interval: a job a waiting thread helps with counts as that job.
(require '[clojure.string :as str])

(def stage-names ["other" "load" "prepare-source" "top-level" "drain" "append"
                  "index" "release" "warnings" "unit" "finish" "wait"])
(def kinds {0 :stage 1 :engine 2 :job 3 :idle 4 :unit-wait 5 :help-idle 6 :park
            7 :demand 8 :forward 9 :bridge-wait 10 :bridge-hold 11 :produce
            12 :dispatch 13 :settle 14 :queue 15 :wake 16 :gc 17 :gc-waits 18 :submit
            19 :end 20 :namespaces-wait 21 :namespaces-hold})
(def key-kinds {2 "binding" 3 "helper" 4 "initializer" 5 "lane" 6 "gensym" 7 "namespace"})
(defn key-kind [k] (get key-kinds k "service"))
(defn s [ns] (/ (double ns) 1e9))
(defn fmt [x] (format "%.2f" (double x)))
(defn pct [x total] (if (pos? total) (format "%.0f%%" (* 100.0 (/ (double x) total))) "-"))

(defn parse [file]
  (with-open [r (clojure.java.io/reader file)]
    (reduce
     (fn [acc line]
       (cond
         (str/starts-with? line "#") acc
         (str/starts-with? line "T\t")
         (let [[_ i name] (str/split line #"\t")] (assoc-in acc [:names (parse-long i)] name))
         :else
         (let [[i t ph k a b] (str/split line #"\t")
               i (parse-long i)]
           (update-in acc [:events i] (fnil conj [])
                      [(parse-long t) (.charAt ^String ph 0) (kinds (parse-long k))
                       (parse-long a) (parse-long b)]))))
     {:names {} :events {}} (line-seq r))))

(defn category
  "The category of a thread's moment given its open intervals (innermost
  last)."
  [stack]
  (let [[kind a b] (peek stack)
        enclosing (fn []
                    (or (some (fn [[k a b]]
                                (case k
                                  :stage (stage-names a)
                                  :produce "helper-production"
                                  :job (when (= b 2) "macro-step")
                                  nil))
                              (rseq stack))
                        "none"))]
    (case kind
      nil "none"
      :engine (str "macros/" (enclosing))
      :job (if (= b 2) "macros/macro-step" "compile/job")
      :produce "macros/helper-production"
      :bridge-hold "macros/bridge-hold"
      :bridge-wait "wait/bridge"
      :namespaces-wait "wait/namespace-registry"
      :namespaces-hold "macros/namespace-registry-hold"
      :unit-wait "wait/unit"
      (:park :demand) (str "wait/" (key-kind a))
      :forward "wait/forward"
      :idle "idle"
      :stage (if (= a 11) "wait/unit" (str "compile/" (stage-names a)))
      (str "other/" (name kind)))))

(defn innermost-unit
  "The unit or macro task whose job is innermost on the stack."
  [stack]
  (some (fn [[k a b]]
          (cond (and (= k :job) (not= b 2)) [:unit a]
                (and (= k :stage) (= a 9)) [:unit b]
                (= k :job) [:task a]))
        (rseq stack)))

(defn sweep
  "Walk one thread's events: totals per category, its state segments and
  the intervals of interest."
  [events]
  (loop [[e & more] events stack [] last nil totals {} segs (transient [])
         ivs (transient []) instants (transient [])]
    (if-not e
      {:totals totals :segments (persistent! segs) :intervals (persistent! ivs)
       :instants (persistent! instants) :end last}
      (let [[t ph kind a b] e
            dt (if last (- t last) 0)
            cat (category stack)
            totals (if (pos? dt) (update totals cat (fnil + 0) dt) totals)
            segs (if (pos? dt) (conj! segs [last t cat (innermost-unit stack)]) segs)]
        (case ph
          \B (recur more (conj stack [kind a b t]) t totals segs ivs instants)
          \E (let [[k0 a0 b0 t0] (peek stack)]
               (when (not= k0 kind)
                 (throw (ex-info "unbalanced timeline" {:open (peek stack) :event e})))
               (recur more (pop stack) t totals segs
                      (conj! ivs {:kind kind :a a0 :b b0 :start t0 :end t :ea a :eb b
                                  :stack (mapv first (pop stack))})
                      instants))
          \I (recur more stack t totals segs ivs (conj! instants {:kind kind :t t :a a :b b})))))))

(defn overlap [s1 e1 s2 e2] (max 0 (- (min e1 e2) (max s1 s2))))

(defn segments-in
  "The state segments of `segs` (sorted by start) overlapping [start, end)."
  [segs start end]
  (let [n (count segs)
        lo (loop [lo 0 hi n]
             (if (< lo hi)
               (let [mid (quot (+ lo hi) 2)]
                 (if (<= (nth (nth segs mid) 1) start) (recur (inc mid) hi) (recur lo mid)))
               lo))]
    (take-while #(< (first %) end) (drop lo segs))))

(let [[file & opts] *command-line-args*
      opts (apply hash-map opts)
      one-thread (some-> (get opts "--one-thread-wall") parse-double)
      {:keys [names events]} (parse file)
      swept (into {} (map (fn [[i evs]] [i (sweep (sort-by first evs))]) events))
      main (some (fn [[i n]] (when (= n "main") i)) names)
      pool (set (keep (fn [[i n]] (when (= n "light-cljs-units") i)) names))
      ;; Unit pool workers: those that ran a job (the source parsing pool's
      ;; threads, also named light-cljs-worker-N, run none).
      slots (filter (fn [i] (and (not (pool i)) (not= i main)
                                 (some #(= :job (:kind %)) (:intervals (swept i)))))
                    (sort (keys swept)))
      wall (apply max (map :end (vals swept)))
      instants (mapcat (fn [[i sw]] (map #(assoc % :thread i) (:instants sw))) swept)
      intervals (mapcat (fn [[i sw]] (map #(assoc % :thread i) (:intervals sw))) swept)
      by-kind (group-by :kind instants)
      ivs-kind (group-by :kind intervals)
      dispatch (into {} (map (juxt :a :t) (by-kind :dispatch)))
      settle (into {} (map (juxt :a :t) (by-kind :settle)))
      ;; Each unit's analysis frame (outermost) and the thread it ran on.
      unit-frames (->> (ivs-kind :stage)
                       (filter #(= 9 (:a %)))
                       (group-by :b)
                       (map (fn [[u fs]] [u (apply max-key #(- (:end %) (:start %)) fs)]))
                       (into {}))
      main-sw (swept main)
      totals-of (fn [i] (:totals (swept i)))
      sum-cat (fn [i pred] (reduce + 0 (keep (fn [[c v]] (when (pred c) v)) (totals-of i))))
      busy? #(or (str/starts-with? % "compile/") (str/starts-with? % "macros/"))]
  (println (format "file %s: wall %s s, %d threads (%d worker slots), %d events"
                   file (fmt (s wall)) (count swept) (count slots)
                   (reduce + (map count (vals events)))))
  ;; 1. Per thread: busy compiling, busy running macros, waiting, idle.
  (println "\n## Threads (seconds)")
  (println "thread | name | compile | macros | wait | idle | unrecorded")
  (doseq [i (cons main slots)]
    (let [tot (totals-of i)
          g (fn [p] (s (reduce + 0 (keep (fn [[c v]] (when (str/starts-with? c p) v)) tot))))]
      (println (str/join " | " [i (names i) (fmt (g "compile/")) (fmt (g "macros/"))
                                (fmt (g "wait/")) (fmt (g "idle")) (fmt (g "none"))]))))
  ;; 2. Categories summed over the coordinator and the worker slots.
  (let [all (apply merge-with + (map totals-of (cons main slots)))
        total (reduce + (vals all))]
    (println "\n## Thread time by category, coordinator and workers (seconds, share)")
    (doseq [[c v] (sort-by (comp - val) all)]
      (println (format "%-34s %8s %5s   coordinator %s" c (fmt (s v)) (pct v total)
                       (fmt (s (get (totals-of main) c 0)))))))
  ;; 3. Macro engine time split.
  (let [all (apply merge-with + (map totals-of (cons main slots)))
        m (fn [c] (s (get all c 0)))]
    (println "\n## Macro engine time (seconds)")
    (println (format "top-level passes %s, units %s, resumed macro steps %s, helper production %s, bridge held %s"
                     (fmt (m "macros/top-level")) (fmt (m "macros/unit"))
                     (fmt (m "macros/macro-step")) (fmt (m "macros/helper-production"))
                     (fmt (m "macros/bridge-hold")))))
  ;; 4. Waits: how they ended, and resume latency.
  (let [uw (ivs-kind :unit-wait)
        ended (frequencies (map :ea uw))
        parks (concat (ivs-kind :park) (ivs-kind :demand))
        wake (group-by :a (by-kind :wake))
        steps (group-by :a (filter #(= 2 (:b %)) (ivs-kind :job)))
        main-uw (filter #(= main (:thread %)) uw)
        lat (keep (fn [w] (when-let [st (settle (:a w))]
                            (when (and (<= (:start w) st) (<= st (:end w)) (not= 2 (:ea w)))
                              (- (:end w) st))))
                  uw)]
    (println "\n## Unit waits")
    (println (format "%d wait iterations: %d woken by notification, %d by the 20 ms timeout, %d ended by help or a state change; coordinator %d"
                     (count uw) (get ended 0 0) (get ended 1 0) (get ended 2 0) (count main-uw)))
    (when (seq lat)
      (println (format "settle-to-resume latency: n %d, mean %.3f ms, max %.3f ms, total %s s"
                       (count lat) (/ (reduce + lat) (count lat) 1e6) (/ (apply max lat) 1e6)
                       (fmt (s (reduce + lat))))))
    (println "\n## Macro task parks (by awaited kind)")
    (doseq [[k ps] (sort-by key (group-by #(key-kind (:a %)) parks))]
      (let [total (reduce + (map #(- (:end %) (:start %)) ps))
            timeouts (reduce + (map :ea ps))
            notified (count (filter #(= 1 (bit-and 1 (:eb %))) ps))]
        (println (format "%-12s %6d parks, %8s s held (incl. helped jobs), %d timeouts expired, %d woken by notification"
                         k (count ps) (fmt (s total)) timeouts notified))))
    (let [lats (for [p (ivs-kind :park)
                     :let [task (bit-shift-right (:eb p) 1)
                           w (some #(when (<= (:start p) (:t %) (:end p)) (:t %)) (wake task))
                           st (some #(when (and w (<= w (:start %) (:end p))) (:start %)) (steps task))]
                     :when (and w st)]
                 [(- st w) (- (:end p) w)])]
      (when (seq lats)
        (println (format "wake-to-step-start: mean %.3f ms, max %.3f ms; wake-to-resume: mean %.3f ms, max %.3f ms (n %d)"
                         (/ (reduce + (map first lats)) (count lats) 1e6) (/ (apply max (map first lats)) 1e6)
                         (/ (reduce + (map second lats)) (count lats) 1e6) (/ (apply max (map second lats)) 1e6)
                         (count lats))))))
  ;; 5. Ready but disallowed: help found queued jobs it may not take.
  (let [hi (filter #(pos? (:a %)) (by-kind :help-idle))
        by-thread (group-by :thread hi)
        est (reduce + (for [[i hs] by-thread
                            :let [ts (sort (map :t hs))]
                            [a b] (map vector ts (concat (rest ts) [Long/MAX_VALUE]))]
                        (min 20000000 (- b a))))]
    (println "\n## Ready but disallowed")
    (println (format "%d help attempts found only queued jobs they may not take (all: %d); about %s s of waiting followed them"
                     (count hi) (count (by-kind :help-idle)) (fmt (s est)))))
  ;; 6. Queue depth and idle workers against what the coordinator did.
  (let [main-segs (:segments main-sw)
        coord-class (fn [cat] (cond (str/starts-with? cat "wait/") "coordinator waiting"
                                    (#{"compile/top-level" "macros/top-level"} cat) "coordinator in a top-level pass"
                                    :else "coordinator doing other work"))
        idle (filter #(not (pool (:thread %))) (ivs-kind :idle))
        split (apply merge-with +
                     {}
                     (for [iv idle
                           [s0 e0 cat] (segments-in main-segs (:start iv) (:end iv))]
                       {(coord-class cat) (overlap s0 e0 (:start iv) (:end iv))}))
        unit-pool (into pool slots)
        queue (sort-by :t (filter #(unit-pool (:thread %)) (by-kind :queue)))
        depth-max (apply max 0 (map :a queue))
        _ (println (format "unit pool: %d queue samples (job starts and submissions), mean runnable %.1f"
                           (count queue) (if (seq queue) (/ (reduce + (map :a queue)) (double (count queue))) 0.0)))
        submits (by-kind :submit)
        inbox-lat (keep (fn [sub] (when-let [d (dispatch (:a sub))] (- (:t sub) d))) submits)]
    (println "\n## Idle workers (empty queue) by what the coordinator was doing (seconds)")
    (doseq [[k v] (sort-by key split)] (println (format "%-34s %s" k (fmt (s v)))))
    (println (format "peak runnable queue depth %d; %d batches; dispatch-to-scheduler latency mean %.2f ms, max %.2f ms"
                     depth-max (count submits)
                     (if (seq inbox-lat) (/ (reduce + inbox-lat) (count inbox-lat) 1e6) 0.0)
                     (if (seq inbox-lat) (/ (apply max inbox-lat) 1e6) 0.0))))
  ;; 7. Bridge per thread.
  (println "\n## Bridge lock per thread (seconds): wait / hold")
  (doseq [i (cons main slots)]
    (println (format "%s %s: %s / %s" i (names i) (fmt (s (get (totals-of i) "wait/bridge" 0)))
                     (fmt (s (get (totals-of i) "macros/bridge-hold" 0))))))
  ;; 8. Collections.
  (when-let [gc (last (by-kind :gc))]
    (let [w (last (by-kind :gc-waits))]
      (println (format "\n## Collections: %d, pause %s s, rendezvous %s s, threads stopped (summed) %s s"
                       (:a gc) (fmt (s (:b gc))) (fmt (s (:a w))) (fmt (s (:b w)))))))
  ;; 9. What the coordinator's unit waits were spent on: the awaited unit's state.
  (let [main-segs (:segments main-sw)
        thread-segs (into {} (map (fn [[i sw]] [i (:segments sw)]) swept))
        attribution
        (apply merge-with + {}
               (for [w (filter #(= main (:thread %)) (ivs-kind :unit-wait))
                     :let [u (:a w) f (unit-frames u) d (dispatch u)]]
                 (cond
                   (nil? f) {"awaited unit: no analysis frame" (- (:end w) (:start w))}
                   :else
                   (let [before (overlap (:start w) (:end w) 0 (:start f))
                         undispatched (if d (overlap (:start w) (:end w) 0 d) 0)
                         segs (segments-in (thread-segs (:thread f)) (max (:start w) (:start f)) (min (:end w) (:end f)))]
                     (apply merge-with +
                            {"awaited unit: queued, not started" (- before undispatched)
                             "awaited unit: not yet dispatched" undispatched
                             "awaited unit: finished, coordinator not yet resumed" (overlap (:start w) (:end w) (:end f) Long/MAX_VALUE)}
                            (for [[s0 e0 cat inner] segs
                                  :let [o (overlap s0 e0 (max (:start w) (:start f)) (min (:end w) (:end f)))]]
                              {(if (and inner (not= inner [:unit u]) (not= (first inner) :task))
                                 (str "awaited unit's thread helping another unit: " cat)
                                 (str "awaited unit " cat)) o}))))))]
    (println "\n## Coordinator unit waits, by what the awaited unit was doing (seconds)")
    (doseq [[k v] (sort-by (comp - val) attribution)]
      (when (> v 1000000) (println (format "%-70s %s" k (fmt (s v)))))))
  ;; 10. Critical path.
  (let [segs (:segments main-sw)
        main-dispatch (sort-by :t (filter #(= main (:thread %)) (by-kind :dispatch)))
        tl-end (->> (ivs-kind :stage) (filter #(and (= main (:thread %)) (= 3 (:a %)))) (map :end) (apply max 0))
        r (fn [u] (when-let [f (unit-frames u)] (when (not= main (:thread f)) (- (:end f) (:start f)))))
        ;; The coordinator's own work, its unit waits and its dispatches, in order.
        steps (->> (concat
                    (for [[s0 e0 cat] segs]
                      (if (= cat "wait/unit") [:wait s0 (- e0 s0)] [:busy s0 (- e0 s0) cat]))
                    (for [d main-dispatch] [:dispatch (:t d) (:a d)]))
                   (sort-by second))
        waits (filter #(= main (:thread %)) (ivs-kind :unit-wait))
        wait-unit (fn [t] (some #(when (<= (:start %) t (dec (:end %))) (:a %)) waits))
        waits-sorted (vec (sort-by :start waits))
        bridge (get (:totals main-sw) "wait/bridge" 0)
        all-ready (fn [dp] (apply max 0 (keep (fn [[u d]] (some->> (r u) (+ d))) dp)))
        ;; Replay the coordinator with infinite workers: a unit is ready its
        ;; analysis time after its dispatch. `lag?` keeps the drain waits;
        ;; without it only the tail waits for every unit. `lanes?` keeps the
        ;; coordinator's own lane waits.
        simulate
        (fn [lag? lanes?]
          (loop [[st & more] steps t 0 dp {} seen #{} edges {} tail-done false]
            (cond
              (nil? st)
              [(if (not tail-done) (max t (all-ready dp)) t) edges]
              (and (not tail-done) (>= (second st) tl-end) (not= :dispatch (first st)))
              (let [t2 (max t (all-ready dp))]
                (recur (cons st more) t2 dp seen (update edges "tail: last unit to finish" (fnil + 0) (- t2 t)) true))
              :else
              (case (first st)
                :busy (recur more (if (and (not lanes?) (= "wait/lane" (nth st 3))) t (+ t (nth st 2))) dp seen edges tail-done)
                :dispatch (recur more t (assoc dp (nth st 2) t) seen edges tail-done)
                :wait (let [u (let [s0 (second st)] (some #(when (and (<= (:start %) s0) (< s0 (:end %))) (:a %)) waits-sorted))
                            tail? (>= (second st) tl-end)]
                        (if (or (nil? u) (seen u) (not lag?))
                          (recur more t dp (cond-> seen u (conj u)) edges tail-done)
                          (let [t2 (max t (+ (get dp u 0) (or (r u) 0)))]
                            (recur more t2 dp (conj seen u)
                                   (update edges (if tail? "tail: units after the last top-level pass" "drain point: unit result (fixed drain lag)") (fnil + 0) (- t2 t))
                                   tail-done))))))))
        [cp-lag edges] (simulate true true)
        [cp-nolag edges2] (simulate false true)
        [cp-nolane edges3] (simulate true false)
        [cp-neither edges4] (simulate false false)
        busy-cats (apply merge-with + {} (for [[_ _ dt cat] (filter #(= :busy (first %)) steps)] {cat dt}))
        busy (reduce + (vals busy-cats))
        n (or (some-> (get opts "--threads") parse-long) (inc (count slots)))]
    (println "\n## Critical path (seconds)")
    (println (format "coordinator busy %s (of which bridge wait %s), unit waits %s, wall %s"
                     (fmt (s busy)) (fmt (s bridge)) (fmt (s (- wall busy))) (fmt (s wall))))
    (println "coordinator busy by category:")
    (doseq [[c v] (sort-by (comp - val) busy-cats)]
      (when (> v 10000000) (println (format "  %-34s %s" c (fmt (s v))))))
    (println (format "with the current drain points, infinite workers: %s; edges: %s"
                     (fmt (s cp-lag)) (str/join ", " (for [[k v] edges] (str k " " (fmt (s v)))))))
    (println (format "draining only after the last top-level pass: %s; edges: %s"
                     (fmt (s cp-nolag)) (str/join ", " (for [[k v] edges2] (str k " " (fmt (s v)))))))
    (println (format "without the coordinator's lane waits: %s (current drain points; edges: %s), %s (drain at the end)"
                     (fmt (s cp-nolane)) (str/join ", " (for [[k v] edges3] (str k " " (fmt (s v)))))
                     (fmt (s cp-neither))))
    (println (format "without the coordinator's bridge waits: %s" (fmt (s (- cp-nolag bridge)))))
    (when one-thread
      (println (format "best wall at %d threads, max(critical path, one-thread wall / N): %s (current drain points), %s (drain at the end)"
                       n (fmt (max (s cp-lag) (/ one-thread n))) (fmt (max (s cp-nolag) (/ one-thread n))))))))
