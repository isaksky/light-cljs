#!/usr/bin/env bb
;; The per-thread factor k of a macro backend against V8 on one
;; collection-heavy macro; see benchmarks/native-macros.md, "Per-thread
;; factor k".
;;
;;   bb benchmarks/native-macros-k.bb OUTPUT [--runs 5] [--n 0,100,400]
;;      [--workload churn|protocol|lazy] NAME=BINARY ...
;;
;; `churn` is the collection-heavy macro; `protocol` (light-cljs-478)
;; dispatches protocol methods on records, a deftype and numbers; `lazy`
;; (light-cljs-479) realizes `for`, `filter`, `map` and `range` sequences.
;;
;; One backend must be named v8. Every compile is a fresh `--jobs 1
;; --no-cache` process; one warm-up compile per backend and n is
;; discarded, then rounds alternate the backend order. A sample's
;; macro time is `time.analysis-ms` from `--stats` (analysis with macro
;; expansion, which also starts the engine), and k at n is the backend's
;; median analysis time at n minus at n = 0, over V8's: engine start, the
;; macro definition and process start cancel out. Each program's Node
;; output must agree across backends.
(require '[babashka.fs :as fs]
         '[babashka.process :as p]
         '[clojure.string :as str])

(def churn-source
  "(ns k.churn)
(defmacro churn [n]
  (loop [i 0 v [] m {}]
    (if (= i n)
      (loop [s (seq v) total 0]
        (if s
          (recur (next s) (+ total (first s) (get m 1 0) (count (rest [1 2 3]))))
          total))
      (recur (+ i 1) (conj v i) (assoc m (count v) (count m))))))
(js/console.log (churn N) (churn N) (churn N) (churn N))
")

(def protocol-source
  "(ns k.churn)
(defprotocol Shape (area [s]) (scale [s k]))
(defrecord Rect [w h] Shape (area [_] (* w h)) (scale [r k] (assoc r :w (* w k))))
(defrecord Circle [r] Shape (area [_] (* 3 r r)) (scale [_ k] (->Circle (* r k))))
(deftype Pt [x] Shape (area [_] 0) (scale [_ k] (Pt. (* x k))))
(extend-protocol Shape number (area [x] x) (scale [x k] (* x k)))
(defmacro churn [n]
  (loop [i 0 j 0 total 0]
    (if (= i n)
      total
      (let [s (if (= j 0) (->Rect i 2) (if (= j 1) (->Circle i) (if (= j 2) (Pt. i) i)))
            s2 (scale s 2)]
        (recur (+ i 1) (if (= j 3) 0 (+ j 1))
               (+ total (area s2) (if (satisfies? Shape s2) 1 0) (if (record? s2) (count s2) 0)))))))
(js/console.log (churn N) (churn N) (churn N) (churn N))
")

(def lazy-source
  "(ns k.churn)
(defmacro churn [n]
  (let [v (vec (range n))
        odds (filter odd? (for [x v :when (even? (inc x))] (inc x)))]
    (+ (reduce + (map (fn [x] (* 2 x)) odds)) (count (doall (take-while (fn [x] (< x n)) (iterate inc 0)))))))
(js/console.log (churn N) (churn N) (churn N) (churn N))
")

(defn options [args]
  (loop [[arg & more] args o {:runs 5 :n [0 100 400] :backends [] :workload "churn"}]
    (cond
      (nil? arg) o
      (= arg "--workload") (recur (rest more) (assoc o :workload (first more)))
      (= arg "--runs") (recur (rest more) (assoc o :runs (parse-long (first more))))
      (= arg "--n") (recur (rest more) (assoc o :n (mapv parse-long (str/split (first more) #","))))
      (str/includes? arg "=") (let [[name bin] (str/split arg #"=" 2)]
                                (recur more (update o :backends conj [name (str (fs/absolutize bin))])))
      (:output o) (throw (ex-info (str "Unexpected argument: " arg) {}))
      :else (recur more (assoc o :output (str (fs/absolutize arg)))))))

(defn median [xs]
  (let [s (vec (sort xs)) c (count s)]
    (if (odd? c) (s (quot c 2)) (/ (+ (s (dec (quot c 2))) (s (quot c 2))) 2.0))))

(defn sample [o [name bin] n run]
  (let [root (fs/path (:output o) (str "src-" n))
        out (str (fs/path (:output o) (format "%s-n%d-r%d.js" name n run)))
        begun (System/nanoTime)
        r (p/shell {:out out :err :string :continue true}
                   bin "--compile" "--entry" "k.churn" "--source-root" (str root)
                   "--jobs" "1" "--no-cache" "--optimize" "none" "--stats"
                   "--macro-timeout-ms" "60000" "--macro-poll-budget" "100000000")
        wall (/ (- (System/nanoTime) begun) 1e6)
        _ (when-not (zero? (:exit r)) (throw (ex-info (str name " n " n ": " (:err r)) {})))
        analysis (some->> (re-find #"(?m)^time\.analysis-ms (\S+)$" (:err r)) second parse-double)
        node (p/shell {:out :string :err :string} (or (System/getenv "NODE") "node") out)]
    {:backend name :n n :run run :wall-ms wall :analysis-ms analysis :stdout (:out node)}))

(let [o (options *command-line-args*)
      _ (when (or (nil? (:output o)) (fs/exists? (:output o)) (not (some #{"v8"} (map first (:backends o)))))
          (throw (ex-info "Usage: OUTPUT (new directory) [--runs R] [--n 0,100,400] v8=BIN NAME=BIN ..." {})))
      _ (doseq [n (:n o)]
          (fs/create-dirs (fs/path (:output o) (str "src-" n) "k"))
          (spit (str (fs/path (:output o) (str "src-" n) "k" "churn.cljs")) (str/replace ({"churn" churn-source "protocol" protocol-source "lazy" lazy-source} (:workload o)) "N" (str n))))
      ;; One discarded warm-up compile per backend and n (file caches).
      _ (doseq [n (:n o) backend (:backends o)] (sample o backend n -1))
      samples (vec (for [run (range (:runs o)) n (:n o)
                         backend (if (even? run) (:backends o) (reverse (:backends o)))]
                     (sample o backend n run)))
      by (group-by (juxt :backend :n) samples)
      med (fn [k b n] (median (map k (by [b n]))))]
  (doseq [n (:n o)]
    (when (apply not= (map :stdout (filter #(= n (:n %)) samples)))
      (throw (ex-info (str "Program output differs at n " n) {}))))
  (spit (str (fs/path (:output o) "samples.tsv"))
        (str/join "\n" (cons "backend\tn\trun\twall-ms\tanalysis-ms"
                             (map #(str/join "\t" ((juxt :backend :n :run :wall-ms :analysis-ms) %)) samples))))
  (println "backend\tn\twall-ms median [min-max]\tanalysis-ms median [min-max]\tk")
  (doseq [[b _] (:backends o) n (:n o)]
    (let [range* (fn [k] (let [xs (map k (by [b n]))]
                           (format "%.1f [%.1f-%.1f]" (double (med k b n)) (double (apply min xs)) (double (apply max xs)))))
          base (:n o)
          k (when (pos? n)
              (/ (- (med :analysis-ms b n) (med :analysis-ms b (first base)))
                 (- (med :analysis-ms "v8" n) (med :analysis-ms "v8" (first base)))))]
      (println (str/join "\t" [b n (range* :wall-ms) (range* :analysis-ms) (if k (format "%.1f" (double k)) "-")])))))
