#!/usr/bin/env bb
;; Times advanced builds of one generated project with light-cljs and shadow-cljs.
;; bb benchmarks/generated-project/compare.bb PROJECT-DIR --light BINARY [--runs N] [--warm-jvm N] [--leaf FILE]
;; --leaf FILE (relative to PROJECT-DIR) adds a leaf-edit scenario for each tool:
;; on the cache its cached scenario left, each run appends a different defn to
;; FILE and rebuilds. FILE is restored afterwards.
(ns generated-project.compare
  (:require [babashka.fs :as fs]
            [babashka.process :as process]
            [clojure.pprint :as pprint]
            [clojure.string :as str]))

(def usage "Usage: bb compare.bb PROJECT-DIR --light BINARY [--runs N] [--warm-jvm N] [--leaf FILE] [--light-only]")

(defn parse-options [args]
  (loop [[arg & more] args options {:runs 3 :warm-jvm 0}]
    (case arg
      nil (if (and (:dir options) (:light options)) options (throw (ex-info usage {})))
      "--light" (recur (rest more) (assoc options :light (str (fs/absolutize (first more)))))
      "--runs" (recur (rest more) (assoc options :runs (parse-long (first more))))
      "--warm-jvm" (recur (rest more) (assoc options :warm-jvm (parse-long (first more))))
      "--leaf" (recur (rest more) (assoc options :leaf (first more)))
      "--light-only" (recur more (assoc options :light-only true))
      (if (or (str/starts-with? arg "-") (:dir options))
        (throw (ex-info usage {:argument arg}))
        (recur more (assoc options :dir (str (fs/absolutize arg))))))))

(defn run! [dir & cmd]
  (let [result (apply process/sh {:dir dir} cmd)]
    (when-not (zero? (:exit result))
      (throw (ex-info "Command failed" {:command cmd :err (:err result)})))
    (:out result)))

(defn timed
  "Wall seconds, user CPU seconds and peak RSS (GB) from macOS /usr/bin/time -l,
  or GNU time -v, with the 1/5/15-minute load average before the run."
  [dir cmd]
  (let [mac? (str/includes? (System/getProperty "os.name") "Mac")
        load (second (re-find #"load averages?: (.*)$" (str/trim (:out (process/sh "uptime")))))
        result (apply process/sh {:dir dir} "/usr/bin/time" (if mac? "-l" "-v") cmd)
        err (:err result)]
    (when-not (zero? (:exit result))
      (throw (ex-info "Build failed" {:command cmd :err (subs err (max 0 (- (count err) 3000)))})))
    (if mac?
      {:s (parse-double (second (re-find #"([0-9.]+) real" err)))
       :user-s (parse-double (second (re-find #"([0-9.]+) user" err))) :load load
       :rss-gb (/ (parse-long (second (re-find #"([0-9]+)  maximum resident set size" err))) 1e9)}
      (let [[_ m s] (re-find #"Elapsed \(wall clock\) time.*: (?:[0-9]+:)?([0-9]+):([0-9.]+)" err)]
        {:s (+ (* 60 (parse-long m)) (parse-double s)) :load load
         :user-s (parse-double (second (re-find #"User time \(seconds\): ([0-9.]+)" err)))
         :rss-gb (/ (parse-long (second (re-find #"Maximum resident set size \(kbytes\): ([0-9]+)" err))) 1e6)}))))

(defn median [xs] (let [v (vec (sort xs))] (v (quot (count v) 2))))
(defn summary [samples]
  {:median-s (median (map :s samples)) :median-rss-gb (median (map :rss-gb samples))
   :median-user-s (median (map :user-s samples))
   :min-s (apply min (map :s samples)) :max-s (apply max (map :s samples)) :samples samples})

(defn gzip-bytes [file]
  (count (:out (process/sh {:in (fs/file file) :out :bytes} "gzip" "-9" "-c"))))

(defn verify [dir expected runner output]
  (let [got (str/trim (run! dir "node" runner "42" "3"))
        waste (process/sh {:dir dir} "bb" "check-waste.bb" "--strict" output)]
    (when-not (= expected got)
      (throw (ex-info "Checksum differs from the oracle" {:runner runner :expected expected :got got})))
    {:bytes (fs/size (fs/file dir output)) :gzip (gzip-bytes (fs/file dir output))
     :waste-ok (zero? (:exit waste)) :waste (str/trim (:out waste))}))

(defn scenario
  "Uncached runs clean before every sample; cached runs clean once, warm up, then keep the cache."
  [dir runs clean! cmd cached?]
  (clean!)
  (when cached? (timed dir cmd))
  (summary (vec (for [i (range runs)]
                  (do (when-not cached? (clean!))
                      (let [sample (timed dir cmd)]
                        (binding [*out* *err*] (println (str/join " " (take-last 3 cmd)) i sample))
                        sample))))))

(defn with-restored
  "Calls (f original-text) and then writes `file`'s original bytes back, also
  when f throws or the script is interrupted (SIGINT and SIGTERM run shutdown
  hooks, not finally blocks)."
  [file f]
  (let [original (fs/read-all-bytes file)
        restore #(fs/write-bytes file original)
        hook (Thread. ^Runnable restore)]
    (.addShutdownHook (Runtime/getRuntime) hook)
    (try
      (f (String. original "UTF-8"))
      (finally
        (restore)
        (try (.removeShutdownHook (Runtime/getRuntime) hook) (catch IllegalStateException _))))))

(defn leaf-scenario
  "On the cache the cached scenario left: per run, a different defn appended to
  `leaf`, then a rebuild. The last output is checked; `leaf` is restored."
  [dir runs leaf cmd check]
  (with-restored
    (fs/file dir leaf)
    (fn [original]
      (let [result (summary (vec (for [i (range runs)]
                                   (do (spit (fs/file dir leaf) (str original "\n(defn compare-leaf-edit-" i " [] " i ")\n"))
                                       (let [sample (timed dir cmd)]
                                         (binding [*out* *err*] (println "leaf" (str/join " " (take-last 3 cmd)) i sample))
                                         sample)))))]
        (assoc result :leaf leaf :output (check))))))

(defn warm-jvm
  "Repeated shadow releases in one JVM, as with a running shadow-cljs server."
  [dir runs]
  (let [script (str (fs/create-temp-file {:suffix ".clj"}))]
    (spit script (str "(require '[shadow.cljs.devtools.api :as api])\n"
                      "(dotimes [_ " runs "] (let [t (System/nanoTime)] (api/release :bench)"
                      " (println \"RUN\" (/ (- (System/nanoTime) t) 1e9))))\n"
                      "(shutdown-agents) (System/exit 0)\n"))
    (try
      (mapv #(parse-double (second %)) (re-seq #"RUN ([0-9.E]+)" (run! dir "clojure" "-J-Xmx8g" "-M" "-i" script)))
      (finally (fs/delete script)))))

(defn main [args]
  (let [{:keys [dir light runs light-only leaf] :as options} (parse-options args)
        expected (str/trim (run! dir "bb" "oracle.bb" "42" "3"))
        light-output "out/light/workload.main.js"
        clean-light! #(doseq [p [light-output ".light-cljs" ".compare-cache"]] (fs/delete-tree (fs/path dir p)))
        clean-shadow! #(do (fs/delete-tree (fs/path dir ".shadow-cljs")) (fs/delete-if-exists (fs/path dir "out/shadow.js")))
        light-cmd [light "release" "bench"]
        light-cached-cmd (conj light-cmd "--cache-dir" ".compare-cache")
        shadow-cmd ["clojure" "-M:shadow" "release" "bench"]
        result (cond->
                {:project dir :light-binary light :expected expected
                 :light (cond-> {:uncached (scenario dir runs clean-light! (conj light-cmd "--no-cache") false)
                                 :output (verify dir expected "run-light.mjs" light-output)
                                 :cached (scenario dir runs clean-light! light-cached-cmd true)}
                          leaf (assoc :leaf (leaf-scenario dir runs leaf light-cached-cmd
                                                           #(verify dir expected "run-light.mjs" light-output))))}
                 (not light-only)
                 (assoc :shadow
                        (do (run! dir "clojure" "-M:shadow" "--help") ; resolve Maven outside the timings
                            (cond-> {:uncached (scenario dir runs clean-shadow! shadow-cmd false)
                                     :output (verify dir expected "out/shadow.js" "out/shadow.js")
                                     :cached (scenario dir runs clean-shadow! shadow-cmd true)}
                              leaf (assoc :leaf (leaf-scenario dir runs leaf shadow-cmd
                                                               #(verify dir expected "out/shadow.js" "out/shadow.js"))))))
                 (and (not light-only) (pos? (:warm-jvm options)))
                 (assoc-in [:shadow :warm-jvm-s] (warm-jvm dir (:warm-jvm options))))]
    (spit (str (fs/path dir "compare-result.edn")) (with-out-str (pprint/pprint result)))
    (doseq [tool [:light :shadow] :when (tool result)
            :let [{:keys [uncached cached output] edit :leaf} (tool result)]]
      (println (format "%-6s uncached %.2f s %.1f user %.2f GB | cached %.2f s %.2f GB%s | %d B, gzip %d B, waste %s"
                       (name tool) (:median-s uncached) (:median-user-s uncached) (:median-rss-gb uncached)
                       (:median-s cached) (:median-rss-gb cached)
                       (if edit (format " | leaf %.2f s %.2f GB" (:median-s edit) (:median-rss-gb edit)) "")
                       (:bytes output) (:gzip output) (if (:waste-ok output) "ok" "FAILED"))))
    (when-let [warm (get-in result [:shadow :warm-jvm-s])]
      (println "shadow in one JVM:" (str/join ", " (map #(format "%.2f s" %) warm))))
    (println "Wrote" (str (fs/path dir "compare-result.edn")))))

(when (= (fs/absolutize *file*)
         (some-> (System/getProperty "babashka.file") fs/absolutize))
  (main *command-line-args*))
