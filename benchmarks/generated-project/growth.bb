#!/usr/bin/env bb
;; How build time and peak memory grow with program size: reads compare.bb
;; results of generated projects of one or more sizes and shapes, and
;; optionally the --stats output of light-cljs builds of them, and writes
;; OUT.edn (plot-ready points and fits) and OUT.md (the tables).
;;
;; bb benchmarks/generated-project/growth.bb --out OUT RESULT.edn ... [STATS.txt ...]
;;
;; A RESULT.edn is compare.bb's --out EDN; its project's manifest.edn gives the
;; size (nonblank lines) and shape. A STATS.txt is everything a
;; `light-cljs release ID --no-cache --stats` build printed; its "config:" line
;; names the project. For each shape, compiler, build and measure the fit is
;; the least-squares slope of log(value) against log(lines), the growth
;; exponent (1 is linear), with the ratios and exponents between consecutive
;; sizes.
(ns generated-project.growth
  (:require [babashka.fs :as fs]
            [clojure.edn :as edn]
            [clojure.pprint :as pprint]
            [clojure.string :as str]))

(def usage "Usage: bb growth.bb --out PREFIX RESULT.edn ... [STATS.txt ...]")

(defn read-edn [file] (edn/read-string {:default (fn [_tag value] value)} (slurp (str file))))

(defn project-size
  "{:shape :lines :project} of a generated project directory."
  [dir]
  (let [{:keys [code-lines generator-options]} (read-edn (fs/file dir "manifest.edn"))]
    {:project (str (fs/file-name dir)) :lines code-lines
     :shape (keyword (or (some-> (:shape generator-options) name) "workload"))}))

(def measures [:wall-s :user-s :system-s :cpu-s :peak-memory-gb])

(defn result-points
  "One point per scenario and compiler of a compare.bb result."
  [result]
  (let [size (project-size (get-in result [:project :light-dir]))]
    (for [[scenario by-tool] (:scenarios result) [tool s] by-tool
          :when (:wall-s s)]
      (merge size {:compiler tool :scenario scenario :runs (- (:runs s) (:failures s 0))
                   :load-1m (:load-1m s) :date (get-in result [:environment :date])
                   :light-commit (get-in result [:environment :light-cljs :commit])}
             (select-keys s measures)))))

(defn parse-stats
  "The counters of a --stats report as {name number}, and its project's directory."
  [text]
  (let [;; the counter lines: after the header, before the first [table]
        body (first (str/split (or (second (re-find #"(?s)light-cljs-stats [0-9]+\n(.*)" text)) "") #"(?m)^\[" 2))]
    {:config-dir (some->> (re-find #"config: (.*light-cljs\.edn)" text) second fs/parent str)
     :counters (into {} (for [[_ k v] (re-seq #"(?m)^([a-z][a-z0-9.\-]*) (-?[0-9.]+)$" body)]
                          [k (parse-double v)]))}))

(defn stats-point [file]
  (let [{:keys [config-dir counters]} (parse-stats (slurp (str file)))]
    (when-not config-dir (throw (ex-info (str file ": no config: line, not a --stats report of a release build") {})))
    (merge (project-size config-dir) {:file (str (fs/file-name file)) :counters counters})))

;; Fitting

(defn log-log-slope
  "Least-squares slope of log y against log x."
  [xs ys]
  (let [lx (map #(Math/log %) xs) ly (map #(Math/log %) ys) n (count lx)
        mx (/ (reduce + lx) n) my (/ (reduce + ly) n)]
    (/ (reduce + (map #(* (- %1 mx) (- %2 my)) lx ly))
       (reduce + (map #(let [d (- % mx)] (* d d)) lx)))))

(defn growth
  "For points sorted by :lines with (value point) positive: the fitted
  exponent and, between consecutive sizes, the value ratio and its exponent."
  [points value]
  (let [points (sort-by :lines points)
        xs (map :lines points) ys (map value points)]
    (when (and (>= (count points) 2) (every? #(and % (pos? %)) ys))
      {:exponent (log-log-slope xs ys)
       :steps (vec (for [[[x0 y0] [x1 y1]] (partition 2 1 (map vector xs ys))]
                     {:from x0 :to x1 :lines-ratio (/ (double x1) x0) :ratio (/ (double y1) y0)
                      :exponent (/ (Math/log (/ (double y1) y0)) (Math/log (/ (double x1) x0)))}))})))

(defn fits [points]
  (vec (for [[[shape compiler scenario] ps] (sort-by key (group-by (juxt :shape :compiler :scenario) points))
             m measures
             :let [g (growth ps (comp :median m))] :when g]
         (merge {:shape shape :compiler compiler :scenario scenario :measure m} g))))

(def stat-keys
  "The --stats counters fitted: the phases, the trace's exclusive stages and peak memory."
  ["time.wall-ms" "time.resolve-parse-ms" "time.analysis-ms" "time.deferred-units-ms"
   "time.prepare-application-ms" "time.emission-ms" "trace.top-level-ms" "trace.top-level.engine-ms"
   "trace.other-ms" "trace.load-ms" "trace.prepare-source-ms" "trace.drain-ms" "trace.append-ms"
   "trace.index-ms" "trace.release-ms" "trace.unit-ms" "trace.unit.engine-ms" "trace.finish-ms"
   "memory.peak-rss-bytes" "macro-calls" "namespaces" "deferred-units.registered"])

(defn median [xs]
  (let [v (vec (sort xs)) n (count v) h (quot n 2)]
    (if (odd? n) (v h) (/ (+ (v (dec h)) (v h)) 2.0))))

(defn stats-by-size
  "One entry per shape and size: the median of each counter over its builds."
  [stats]
  (vec (for [[[shape lines] ps] (sort-by key (group-by (juxt :shape :lines) stats))]
         {:shape shape :lines lines :project (:project (first ps)) :builds (count ps)
          :counters (into {} (for [k stat-keys :let [vs (keep #(get-in % [:counters k]) ps)] :when (seq vs)]
                               [k (median vs)]))})))

(defn stat-fits [sizes]
  (vec (for [[shape ps] (sort-by key (group-by :shape sizes)) k stat-keys
             :let [g (growth ps #(get-in % [:counters k]))] :when g]
         (merge {:shape shape :counter k} g))))

;; Report

(def scenario-labels {:uncached "clean build" :cached "rebuild, nothing changed" :leaf "rebuild after a one-file edit"})
(def tool-labels {:light "light-cljs" :shadow "shadow-cljs"})
(def measure-labels {:wall-s "wall time" :user-s "user CPU" :system-s "system CPU" :cpu-s "CPU (user + system)"
                     :peak-memory-gb "peak memory"})

(defn- lines-text [n] (str (Math/round (/ n 1000.0)) "k"))

(defn- spread-text [{:keys [median min max]} digits unit]
  (format (str "%." digits "f %s (%." digits "f-%." digits "f)") (double median) unit (double min) (double max)))

(defn- steps-text [{:keys [steps]}]
  (str/join ", " (map #(format "%.2fx (%.2f)" (:ratio %) (:exponent %)) steps)))

(defn markdown [{:keys [points fits stat-fits stat-sizes]}]
  (str/join
   "\n"
   (concat
    (for [[shape ps] (sort-by key (group-by :shape points))
          :let [sizes (sort (distinct (map :lines ps)))]]
      (str/join
       "\n"
       (concat
        [(str "#### " (name shape) " shape") ""
         "| Nonblank lines | Build | Compiler | Rounds | Wall time | User CPU | System CPU | Peak memory | 1-minute load before |"
         "| --- | --- | --- | --- | --- | --- | --- | --- | --- |"]
        (for [lines sizes [scenario label] scenario-labels tool [:light :shadow]
              :let [p (first (filter #(= [lines scenario tool] ((juxt :lines :scenario :compiler) %)) ps))] :when p]
          (str "| " (format "%,d" lines) " | " label " | " (tool-labels tool) " | " (:runs p) " | "
               (spread-text (:wall-s p) 2 "s") " | " (spread-text (:user-s p) 2 "s") " | "
               (spread-text (:system-s p) 2 "s") " | " (spread-text (:peak-memory-gb p) 3 "GB") " | "
               (format "%.2f-%.2f" (double (:min (:load-1m p))) (double (:max (:load-1m p)))) " |"))
        ["" "| Build | Compiler | Measure | Exponent (log-log fit) | Step ratios (exponent) |" "| --- | --- | --- | --- | --- |"]
        (for [[scenario label] scenario-labels tool [:light :shadow] m [:wall-s :cpu-s :peak-memory-gb]
              :let [f (first (filter #(= [shape tool scenario m] ((juxt :shape :compiler :scenario :measure) %)) fits))] :when f]
          (str "| " label " | " (tool-labels tool) " | " (measure-labels m) " | " (format "%.2f" (:exponent f)) " | "
               (steps-text f) " |"))
        [""])))
    (when (seq stat-fits)
      (concat
       ["#### light-cljs --stats counters" ""
        (str "`--no-cache --stats` builds per size (the median where there are several): "
             (str/join "; " (for [[shape ps] (sort-by key (group-by :shape stat-sizes))]
                              (str (name shape) " " (str/join ", " (map #(str (lines-text (:lines %)) " x" (:builds %)) ps)))))
             ". Counters at the largest size, exponents of the log-log fit and step ratios (exponent).")
        ""
        "| Shape | Counter | At the largest size | Exponent | Step ratios (exponent) |"
        "| --- | --- | --- | --- | --- |"]
       (for [{:keys [shape counter] :as f} stat-fits
             :let [largest (last (sort-by :lines (filter #(= shape (:shape %)) stat-sizes)))]]
         (str "| " (name shape) " | " counter " | " (format "%,.1f" (get-in largest [:counters counter])) " | "
              (format "%.2f" (:exponent f)) " | " (steps-text f) " |"))
       [""])))))

(defn main [args]
  (let [[out files] (loop [[a & more] args out nil files []]
                      (cond (nil? a) [out files]
                            (= "--out" a) (recur (rest more) (first more) files)
                            (str/starts-with? a "-") (throw (ex-info usage {:argument a}))
                            :else (recur more out (conj files a))))
        _ (when (or (nil? out) (empty? files)) (throw (ex-info usage {})))
        {edns true txts false} (group-by #(str/ends-with? % ".edn") files)
        points (vec (sort-by (juxt :shape :compiler :scenario :lines) (mapcat (comp result-points read-edn) edns)))
        stats (vec (sort-by (juxt :shape :lines) (map stats-point txts)))
        sizes (stats-by-size stats)
        result {:points points :fits (fits points) :stat-sizes sizes :stat-fits (stat-fits sizes)
                :stats (mapv #(update % :counters select-keys stat-keys) stats)}
        report (markdown result)]
    (spit (str out ".edn") (with-out-str (pprint/pprint result)))
    (spit (str out ".md") report)
    (println report)
    (println "Wrote" (str out ".edn") "and" (str out ".md"))))

(when (= (fs/absolutize *file*)
         (some-> (System/getProperty "babashka.file") fs/absolutize))
  (main *command-line-args*))
