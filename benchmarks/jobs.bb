#!/usr/bin/env bb
;; Cold builds at several --jobs values, alternating a parent binary (no
;; --jobs) with a candidate at each value, round by round (light-cljs-343;
;; docs/compiler-concurrency.md, "Gates and first steps").
;;
;; Usage: bb benchmarks/jobs.bb OUTPUT-DIR [option ...] SCENARIO ...
;;   --parent BIN      the parent commit's binary, run without --jobs
;;   --candidate BIN   the binary run at each --jobs value (default target/release/light-cljs)
;;   --jobs 1,2,4,8    the candidate's --jobs values (default 1,2,4,8,12)
;;   --rounds N        rounds of every configuration (default 3)
;;   --stats           pass --stats and keep each report (adds the trace's cost)
;;   --source-maps     also write a source map per build and hash it with the output
;; Scenarios: main-none, main-advanced and abnf (Penpot's app.main at none and
;; advanced, and instaparse.abnf alone, through benchmarks/penpot.bb), and the
;; generated fixtures small (pool start-up dominates), macros (macro-heavy),
;; wide (a wide DAG) and chain (a chain of namespaces).
;;
;; Each build's output is hashed and then deleted. OUTPUT-DIR, which must not
;; hold an earlier invocation's runs.edn, gets runs.edn with one row per build; the summary printed at the end gives, per scenario and
;; configuration, the median and range of wall time, the median user and
;; system time and peak RSS, the 1-minute load range and the output digests.
(require '[babashka.fs :as fs]
         '[babashka.process :as process]
         '[clojure.edn :as edn]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def repo (fs/parent (fs/parent (fs/absolutize *file*))))

(defn parse-args [args]
  (loop [[arg & more :as args] args
         options {:candidate (str (fs/path repo "target/release/light-cljs"))
                  :jobs [1 2 4 8 12] :rounds 3 :scenarios []}]
    (case arg
      nil options
      "--parent" (recur (rest more) (assoc options :parent (str (fs/absolutize (first more)))))
      "--candidate" (recur (rest more) (assoc options :candidate (str (fs/absolutize (first more)))))
      "--jobs" (recur (rest more) (assoc options :jobs (mapv parse-long (str/split (first more) #","))))
      "--rounds" (recur (rest more) (assoc options :rounds (parse-long (first more))))
      "--stats" (recur more (assoc options :stats true))
      "--source-maps" (recur more (assoc options :source-maps true))
      (if (:output options)
        (recur more (update options :scenarios conj arg))
        (recur more (assoc options :output (fs/absolutize arg)))))))

(def options (parse-args *command-line-args*))
(def output (or (:output options) (throw (ex-info "An output directory is required" {}))))
(fs/create-dirs output)
;; One invocation per directory, so a summary never mixes experiments.
(when (fs/exists? (fs/path output "runs.edn"))
  (throw (ex-info "OUTPUT-DIR holds an earlier invocation's runs.edn; give each its own directory"
                  {:output (str output)})))

;; The generated fixtures: namespace name to source text.
(def macro-ns
  (str "(ns bench.macros)\n"
       "(defn table [n] (vec (for [i (range n)] {:k (keyword (str \"k\" i)) :v (* i i)})))\n"
       "(defmacro rows [n] (table n))\n"
       "(defmacro sum-of [& xs] (reduce + 0 (map count (map str xs))))\n"
       "(defmacro guarded [x & body] `(when (some? ~x) (let [v# ~x] ~@body v#)))\n"))

(defn definitions [prefix n dependency]
  (str/join "\n"
            (for [i (range n)]
              (case (mod i 3)
                0 (format "(def %s%d (rows %d))" prefix i (+ 4 (mod i 7)))
                1 (format "(defn %s%d [x] (guarded x (sum-of a b %d) %s))" prefix i i
                          (if dependency (str "(count " dependency ")") "1"))
                2 (format "(def %s%d (sum-of %s))" prefix i (str/join " " (range (mod i 9))))))))

(defn requires [namespaces]
  (str/join " " (for [n namespaces] (format "[%s]" n))))

(def fixtures
  {"small" {"bench.small" "(ns bench.small)\n(def greeting \"hello\")\n(defn shout [s] (str s \"!\"))\n(js/console.log (shout greeting))\n"}
   "macros" (into {"bench.macros" macro-ns}
                  (concat
                   (for [k (range 4)]
                     [(str "bench.heavy" k)
                      (str (format "(ns bench.heavy%d (:require [bench.macros :refer [rows sum-of guarded]]))\n" k)
                           (definitions "h" 600 nil))])
                   [["bench.main" (str "(ns bench.main (:require " (requires (map #(str "bench.heavy" %) (range 4))) "))\n"
                                       "(js/console.log (count bench.heavy3/h0))\n")]]))
   "wide" (into {"bench.macros" macro-ns}
                (concat
                 (for [k (range 64)]
                   [(str "bench.leaf" k)
                    (str (format "(ns bench.leaf%d (:require [bench.macros :refer [rows sum-of guarded]]))\n" k)
                         (definitions "w" 60 nil))])
                 [["bench.main" (str "(ns bench.main (:require " (requires (map #(str "bench.leaf" %) (range 64))) "))\n"
                                     "(js/console.log (count bench.leaf63/w0))\n")]]))
   "chain" (into {"bench.macros" macro-ns}
                 (concat
                  (for [k (range 64)]
                    [(str "bench.link" k)
                     (str (format "(ns bench.link%d (:require [bench.macros :refer [rows sum-of guarded]]%s))\n"
                                  k (if (pos? k) (format " [bench.link%d :as prev]" (dec k)) ""))
                          (definitions "c" 60 (when (pos? k) "prev/c0")))])
                  [["bench.main" "(ns bench.main (:require [bench.link63]))\n(js/console.log (count bench.link63/c0))\n"]]))})

(def fixture-entries {"small" "bench.small" "macros" "bench.main" "wide" "bench.main" "chain" "bench.main"})
(def penpot-scenarios #{"main-none" "main-advanced" "abnf"})

(defn write-fixture [scenario]
  (let [root (fs/path output "fixtures" scenario)]
    (doseq [[namespace text] (fixtures scenario)]
      (let [file (fs/path root (str (str/replace namespace "." "/") ".cljs"))]
        (fs/create-dirs (fs/parent file))
        (spit (str file) text)))
    (str root)))

(defn sha256 [file]
  (when (fs/exists? file)
    (let [digest (java.security.MessageDigest/getInstance "SHA-256")]
      (with-open [in (io/input-stream (str file))]
        (let [buffer (byte-array 65536)]
          (loop []
            (let [n (.read in buffer)]
              (when (pos? n) (.update digest buffer 0 n) (recur))))))
      (apply str (map #(format "%02x" %) (.digest digest))))))

(defn load-average []
  (some-> (re-find #"load averages?: ([0-9.]+)" (:out (process/sh "uptime"))) second parse-double))

(defn time-fields [timing]
  (let [[_ real user system] (re-find #"([0-9.]+)\s+real\s+([0-9.]+)\s+user\s+([0-9.]+)\s+sys" timing)
        rss (second (re-find #"(\d+)\s+maximum resident set size" timing))]
    {:wall-seconds (some-> real parse-double)
     :user-seconds (some-> user parse-double)
     :system-seconds (some-> system parse-double)
     :peak-rss-bytes (some-> rss parse-long)}))

(defn stats-counters [text]
  (into (sorted-map)
        (for [line (str/split-lines text)
              :let [[_ key value] (re-matches #"([a-z][a-z0-9.-]*) (-?[0-9.]+)" line)]
              :when key]
          [(keyword key) (parse-double value)])))

(defn extra-args [jobs dir]
  (cond-> []
    jobs (into ["--jobs" (str jobs)])
    (:stats options) (conj "--stats")
    (:source-maps options) (into ["--source-map" (str (fs/path dir "out.map"))])))

(defn run-penpot [scenario binary args dir]
  (let [result (process/shell {:dir (str repo) :continue true :out :string :err :string
                               :extra-env {"LIGHT_CLJS_BINARY" binary
                                           "LIGHT_CLJS_EXTRA_ARGS" (str/join " " args)}}
                              "bb" (str (fs/path repo "benchmarks/penpot.bb")) (str dir) scenario)
        record (some-> (fs/file dir "results.edn") slurp str/split-lines last edn/read-string)]
    {:exit (:exit result)
     :js (fs/path dir (str scenario ".js"))
     :fields (select-keys record [:wall-seconds :user-seconds :system-seconds :peak-rss-bytes :stats])
     :error (when-not (zero? (:exit result)) (str/join "\n" (take-last 5 (str/split-lines (str (:err result) (:out result))))))}))

;; Builds are cold: a binary whose cache is on by default (light-cljs-328)
;; gets --no-cache.
(def no-cache
  (memoize (fn [binary]
             (when (str/includes? (:out (process/sh binary "--help")) "--no-cache") ["--no-cache"]))))

(defn run-fixture [scenario binary args dir]
  (let [root (write-fixture scenario)
        js (fs/path dir (str scenario ".js"))
        command (concat ["/usr/bin/time" "-l" binary "--compile" "--esm" "--source-root" root
                         "--entry" (fixture-entries scenario) "--optimize" "none"
                         "--macro-expansions" "100000000" "--macro-nodes" "4000000000"
                         "--macro-timeout-ms" "3600000" "--macro-memory-limit" "4000000000"
                         "--macro-poll-budget" "1000000000000"]
                        (no-cache binary) args)
        result (with-open [out (io/output-stream (str js))]
                 (apply process/shell {:dir (str repo) :continue true :out out :err :string} command))
        timing (:err result)]
    {:exit (:exit result)
     :js js
     :fields (cond-> (time-fields timing)
               (:stats options) (assoc :stats (stats-counters timing)))
     :error (when-not (zero? (:exit result)) (str/join "\n" (filter #(str/starts-with? % "light-cljs") (str/split-lines timing))))}))

(def configurations
  (vec (concat (when (:parent options) [{:name "parent" :binary (:parent options)}])
               (for [jobs (:jobs options)]
                 {:name (str "jobs-" jobs) :binary (:candidate options) :jobs jobs}))))

(defn rotate [xs n] (let [k (mod n (count xs))] (concat (drop k xs) (take k xs))))

(def runs (fs/file output "runs.edn"))

(doseq [round (range (:rounds options))
        scenario (:scenarios options)
        configuration (rotate configurations round)]
  (let [dir (fs/path output scenario (str (:name configuration) "-" round))
        _ (fs/create-dirs dir)
        args (extra-args (:jobs configuration) dir)
        load (load-average)
        _ (do (println "round" round scenario (:name configuration) "load" load) (flush))
        run (if (penpot-scenarios scenario) run-penpot run-fixture)
        result (run scenario (:binary configuration) args dir)
        map-file (fs/path dir "out.map")
        record (merge {:round round :scenario scenario :configuration (:name configuration)
                       :binary (:binary configuration) :args args :load load :exit (:exit result)
                       :output-bytes (when (fs/exists? (:js result)) (fs/size (:js result)))
                       :output-sha256 (sha256 (:js result))
                       :map-sha256 (sha256 map-file)}
                      (:fields result))]
    (when (:stats record)
      (spit (fs/file dir "stats.edn") (pr-str (:stats record))))
    (fs/delete-if-exists (:js result))
    (fs/delete-if-exists map-file)
    (spit runs (str (pr-str (dissoc record :stats)) "\n") :append true)
    (prn (select-keys record [:wall-seconds :user-seconds :peak-rss-bytes :output-sha256]))
    (when-not (zero? (:exit result))
      (throw (ex-info "Build failed" (assoc record :error (:error result)))))))

(defn median [xs]
  (let [xs (vec (sort (remove nil? xs))) n (count xs)]
    (when (pos? n)
      (if (odd? n) (xs (quot n 2)) (/ (+ (xs (dec (quot n 2))) (xs (quot n 2))) 2.0)))))

(let [rows (->> (slurp runs) str/split-lines (map edn/read-string))
      order (into {} (map-indexed (fn [i c] [(:name c) i]) configurations))]
  (println "| Scenario | Configuration | Runs | Wall (s) median | Wall range | User (s) | System (s) | Peak RSS (GB) | Load | Output SHA-256 |")
  (println "| --- | --- | ---: | ---: | --- | ---: | ---: | ---: | --- | --- |")
  (doseq [[[scenario configuration] group] (sort-by (fn [[[s c]]] [s (order c 99)])
                                                    (group-by (juxt :scenario :configuration) rows))]
    (let [walls (map :wall-seconds group)
          loads (keep :load group)
          digests (distinct (map #(str (some-> (:output-sha256 %) (subs 0 16))
                                       (when (:map-sha256 %) (str "/" (subs (:map-sha256 %) 0 16))))
                                 group))]
      (println (format "| %s | %s | %d | %.2f | %.2f-%.2f | %.1f | %.1f | %.3f | %.1f-%.1f | %s |"
                       scenario configuration (count group) (median walls)
                       (apply min walls) (apply max walls)
                       (median (map :user-seconds group)) (median (map :system-seconds group))
                       (/ (median (map :peak-rss-bytes group)) 1e9)
                       (apply min loads) (apply max loads)
                       (str/join ", " digests))))))
