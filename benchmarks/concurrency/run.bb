#!/usr/bin/env bb
;; One cold `--compile --optimize none` build with the concurrency probes on,
;; optionally stack-sampled. It applies no patch: point it at a binary built
;; with instrumentation.patch (and conc_probe.rs) applied; see README.md.
;;
;; Usage: bb benchmarks/concurrency/run.bb OUTPUT-DIR [--binary PATH] [--entry NS]
;;          [--source-root DIR]... [--sample] [-- EXTRA-COMPILER-ARGS...]
;;
;; The binary is --binary, else LIGHT_CLJS_BINARY, else target/release/light-cljs.
;; Without --source-root the five Penpot port roots of benchmarks/penpot.bb are used.
;; The entry defaults to app.main. Writes ENTRY.js (stdout), ENTRY.probe.tsv (probe
;; lines plus `/usr/bin/time -l`) and, with --sample, ENTRY.sample.txt and the
;; main thread's call tree in ENTRY.main-thread-sample.txt.
(require '[babashka.fs :as fs]
         '[babashka.process :as process]
         '[clojure.string :as str])

(def repo (fs/parent (fs/parent (fs/parent (fs/absolutize *file*)))))
(def here (fs/path repo "benchmarks/concurrency"))

(defn usage! [message]
  (binding [*out* *err*]
    (println message)
    (println "Usage: bb benchmarks/concurrency/run.bb OUTPUT-DIR [--binary PATH] [--entry NS] [--source-root DIR]... [--sample] [-- EXTRA-COMPILER-ARGS...]"))
  (System/exit 2))

(defn parse-args [args]
  (loop [args args options {:roots [] :extra []}]
    (let [[flag value & more] args]
      (cond
        (empty? args) options
        (= flag "--") (assoc options :extra (vec (rest args)))
        (= flag "--sample") (recur (rest args) (assoc options :sample true))
        (#{"--binary" "--entry" "--source-root"} flag)
        (do (when-not value (usage! (str flag " needs a value")))
            (recur more (case flag
                          "--binary" (assoc options :binary value)
                          "--entry" (assoc options :entry value)
                          "--source-root" (update options :roots conj value))))
        (str/starts-with? flag "-") (usage! (str "Unknown option " flag))
        (:output options) (usage! (str "Unexpected argument " flag))
        :else (recur (rest args) (assoc options :output flag))))))

(def options (parse-args *command-line-args*))
(when-not (:output options) (usage! "An output directory is required"))
(def output (fs/absolutize (:output options)))
(fs/create-dirs output)
(def binary (str (fs/absolutize (or (:binary options)
                                    (System/getenv "LIGHT_CLJS_BINARY")
                                    (fs/path repo "target/release/light-cljs")))))
(when-not (fs/executable? binary) (usage! (str "No executable binary at " binary)))
(def entry (or (:entry options) "app.main"))
(def roots (if (seq (:roots options))
             (map #(str (fs/absolutize %)) (:roots options))
             (let [port (fs/path repo "research/penpot-light")]
               (map #(str (fs/path port %)) ["frontend/src" "common/src" "vendor" "generated" "shims"]))))
(doseq [root roots]
  (when-not (fs/directory? root) (usage! (str "Missing source root " root))))

;; The same limits as benchmarks/penpot.bb, so the probe build is the benchmark build.
(def command (concat ["/usr/bin/time" "-l" binary "--compile" "--esm"
                      "--macro-expansions" "100000000" "--macro-nodes" "4000000000"
                      "--macro-timeout-ms" "3600000" "--macro-memory-limit" "4000000000"
                      "--macro-poll-budget" "1000000000000"]
                     (mapcat #(vector "--source-root" %) roots)
                     ["--entry" entry "--optimize" "none"]
                     ;; A cold build: --no-cache where the cache is on by default.
                     (when (str/includes? (:out (process/sh binary "--help")) "--no-cache") ["--no-cache"])
                     (:extra options)))
(def js-file (fs/file output (str entry ".js")))
(def probe-file (fs/file output (str entry ".probe.tsv")))
(def sample-file (fs/file output (str entry ".sample.txt")))
(def main-thread-file (fs/file output (str entry ".main-thread-sample.txt")))

(defn child-pid
  "The light-cljs process that /usr/bin/time started, once it exists."
  [parent]
  (loop [tries 0]
    (let [found (-> (process/sh "pgrep" "-P" (str parent)) :out str/trim)]
      (cond
        (seq found) (first (str/split-lines found))
        (< tries 200) (do (Thread/sleep 5) (recur (inc tries)))
        :else nil))))

(defn main-thread-tree
  "The main thread's call tree from a `sample` report: its header line through
  the line before the next thread or the end of the call graph."
  [report]
  (let [lines (str/split-lines report)
        thread? #(re-find #"^\s+\d+ Thread_" %)
        [_ from] (split-with #(not (and (thread? %) (str/includes? % "com.apple.main-thread"))) lines)]
    (when (seq from)
      (cons (first from) (take-while #(not (or (thread? %) (str/blank? %))) (rest from))))))

;; Never report an earlier run's samples next to this run's timings.
(fs/delete-if-exists sample-file)
(fs/delete-if-exists main-thread-file)

(println "Binary" binary)
(println "Entry" entry "with" (count roots) "source roots")
(flush)
(let [[compile sampled] (let [running (apply process/process {:dir (str repo) :extra-env {"LIGHT_NS_PROBE" "1"}
                                                :out :write :out-file js-file
                                                :err :write :err-file probe-file}
                                               command)
                      sampler (when (:sample options)
                                (if-let [pid (child-pid (.pid (:proc running)))]
                                  ;; 30 s at 1 ms, as in the study; it stops when the build exits.
                                  (process/process {:out :string :err :string}
                                                   "sample" pid "30" "1" "-mayDie" "-file" (str sample-file))
                                  (binding [*out* *err*] (println "The build exited before sampling could attach") nil)))
                      result @running]
                  [result (when sampler
                            (let [done @sampler]
                              (or (zero? (:exit done))
                                  (binding [*out* *err*] (println "sample failed:" (str/trim (:err done))) false))))])
      timing (slurp probe-file)
      probes (count (re-seq #"(?m)^NSPROBE\t" timing))
      [_ real] (re-find #"([0-9.]+)\s+real" timing)
      rss (second (re-find #"(\d+)\s+maximum resident set size" timing))]
  (println "exit" (:exit compile) "wall" real "s" "peak RSS" rss "bytes" "NSPROBE lines" probes)
  (when (zero? probes)
    (println "No NSPROBE lines: the binary was not built with instrumentation.patch, so dag.bb has nothing to model."))
  (when (and sampled (fs/exists? sample-file))
    (if-let [tree (main-thread-tree (slurp sample-file))]
      (do (spit main-thread-file (str (str/join "\n" tree) "\n"))
          (println "Main thread samples in" (str main-thread-file)))
      (println "No main thread call tree found in" (str sample-file))))
  (println "Model:" "bb" (str (fs/path here "dag.bb")) (str probe-file))
  (when (fs/exists? main-thread-file)
    (println "Phases:" "bb" (str (fs/path here "phases.bb")) (str main-thread-file))
    (println "V8 callers:" "bb" (str (fs/path here "jscallers2.bb")) (str main-thread-file)))
  (when-not (zero? (:exit compile))
    (System/exit (:exit compile))))
