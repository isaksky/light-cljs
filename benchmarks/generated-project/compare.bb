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
            [clojure.edn]
            [clojure.string :as str]))

(def usage "Usage: bb compare.bb PROJECT-DIR --light BINARY [--runs N] [--warm-jvm N] [--leaf FILE] [--light-only]")

(defn parse-options [args]
  (loop [[arg & more] args options {:runs 3 :warm-jvm 0 :workers [1 2 4 8] :phase "check" :timeout-ms 3600000}]
    (case arg
      nil (if (and (:dir options) (:light options)) options (throw (ex-info usage {})))
      "--light" (recur (rest more) (assoc options :light (str (fs/absolutize (first more)))))
      "--runs" (recur (rest more) (assoc options :runs (parse-long (first more))))
      "--warm-jvm" (recur (rest more) (assoc options :warm-jvm (parse-long (first more))))
      "--leaf" (recur (rest more) (assoc options :leaf (first more)))
      "--backend" (recur (rest more) (assoc options :backend (first more)))
      "--phase" (recur (rest more) (assoc options :phase (first more)))
      "--runner" (recur (rest more) (assoc options :runner (str (fs/absolutize (first more)))))
      "--workers" (recur (rest more) (assoc options :workers (mapv parse-long (str/split (first more) #","))))
      "--compiler-slots" (recur (rest more) (assoc options :compiler-slots (mapv parse-long (str/split (or (first more) "") #","))))
      "--timeout-ms" (recur (rest more) (assoc options :timeout-ms (parse-long (first more))))
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

;; The pinned generated-200k experiment reuses this comparator's summaries,
;; oracle/waste tools and source-restoration helper. No Shadow cells run here.
(def oracle-inputs [[0 1] [1 2] [42 3] [999983 2] [1000002 1]])
(def kit (fs/parent (fs/canonicalize *file*)))
(def repository (fs/canonicalize (fs/path kit "../..")))
(defn sha256 [path]
  (let [digest (java.security.MessageDigest/getInstance "SHA-256")]
    (with-open [input (java.io.FileInputStream. (str path))]
      (let [buffer (byte-array 65536)]
        (loop [] (let [n (.read input buffer)] (when (pos? n) (.update digest buffer 0 n) (recur))))))
    (apply str (map #(format "%02x" (bit-and 255 %)) (.digest digest)))))

(defn stop-tree! [proc]
  (let [parent (.toHandle (:proc proc))
        children (with-open [stream (.descendants parent)] (vec (.toArray stream)))]
    ;; Children first, while their parent can still reap them.
    (doseq [child (reverse children)] (.destroyForcibly child))
    (deref proc 1000 nil)
    (.destroyForcibly parent)
    (deref proc 5000 nil)
    (when (some #(.isAlive %) (conj children parent))
      (throw (ex-info "Owned process survived cleanup" {})))))

(defn process-tree [root text]
  (let [rows (vec (for [line (str/split-lines text)
                       :let [[_ pid parent rss command] (re-matches #"\s*(\d+)\s+(\d+)\s+(\d+)\s+(.+)" line)]
                       :when pid]
                   {:pid (parse-long pid) :parent (parse-long parent)
                    :rss-kib (parse-long rss) :command command}))]
    (loop [ids #{root}]
      (let [expanded (into ids (keep #(when (ids (:parent %)) (:pid %)) rows))]
        (if (= ids expanded) (filterv #(ids (:pid %)) rows) (recur expanded))))))

(defn bounded-run [dir environment timeout-ms cmd]
  (let [proc (process/process cmd {:dir dir :env environment :out :string :err :string :continue true})
        hook (Thread. ^Runnable #(stop-tree! proc))
        sampler (when-let [file (get environment "GENERATED_COMPARE_TREE")]
                  (future
                    (spit file (str (pr-str {:kind :scope :root (.pid (:proc proc))
                                             :interval-ms 100 :rss-unit :kib
                                             :scope "sampled sum of resident pages in owned command tree; shared pages counted per process; not PSS or a continuous peak"}) "\n"))
                    (while (.isAlive (:proc proc))
                      (let [r (process/sh "ps" "-axo" "pid=,ppid=,rss=,comm=")]
                        (assert (zero? (:exit r)) "Process-tree sampling failed")
                        (spit file (str (pr-str {:at-ns (System/nanoTime)
                                                :processes (process-tree (.pid (:proc proc)) (:out r))}) "\n") :append true))
                      (Thread/sleep 100))))]
    (.addShutdownHook (Runtime/getRuntime) hook)
    (try
      (let [result (deref proc timeout-ms ::timeout)]
        (if (= ::timeout result)
          (do (stop-tree! proc) (throw (ex-info "Command watchdog expired; owned tree reaped" {:command cmd})))
          (select-keys result [:exit :out :err])))
      (finally
        (when (.isAlive (:proc proc)) (stop-tree! proc))
        (try (.removeShutdownHook (Runtime/getRuntime) hook) (catch IllegalStateException _))
        (when sampler
          (when (= ::timeout (deref sampler 5000 ::timeout))
            (future-cancel sampler)
            (throw (ex-info "Process-tree sampler did not stop" {}))))))))

(defn checked [dir environment cmd]
  (let [r (bounded-run dir environment 180000 cmd)]
    (when-not (zero? (:exit r)) (throw (ex-info "Validation failed" {:command cmd :result r})))
    (:out r)))

(def ^:dynamic *leaf-edit* nil)
(defn pinned! [dir]
  (let [manifest (clojure.edn/read-string (slurp (str (fs/path dir "manifest.edn"))))
        frozen (fs/path repository "crates/big-boy-pants-js/evidence/generated-200k-manifest.edn")]
    (assert (= (sha256 frozen) (sha256 (fs/path dir "manifest.edn"))) "Pinned manifest differs")
    (assert (= [199129 450 653] [(:code-lines manifest) (:worker-namespaces manifest) (count (:files manifest))]))
    (doseq [[path hash] {"model.edn" "75a743002f95e38b31f7b351b512e3e8c1dd618a2b7c4ca1947d398503a33bc1", "light-cljs.edn" "7c14e540bed768879c75d817010bd3a09ed0c2b96119f5276c41f0b319f8906c", "run-light.mjs" "d55d1a350da3dd5384f5dbaaf4b55e1d929d4e4176a25206fadd3d7322a4d317", "oracle.bb" "d341b739e7db6a28d102fa88e10fc16f9235c0dfc52e8f847dec7b6256eaaebf", "check-waste.bb" "fc6cbedb8dcd0e89d1852aa0c619788cd1da8b4835211385f57eb69c182094ba"}]
      (assert (= hash (sha256 (fs/path dir path))) (str "Tooling differs: " path)))
    (assert (= (set (map :path (:files manifest)))
               (set (for [file (fs/glob dir "**.{cljs,cljc}")]
                      (str (fs/relativize dir file))))) "Source population differs")
    (doseq [{:keys [path] expected :sha256} (:files manifest)]
      (assert (= (if (= path (:path *leaf-edit*)) (:sha256 *leaf-edit*) expected) (sha256 (fs/path dir path))) (str "Source differs: " path)))
    (doseq [[path hash] (:generator-sha256 manifest)]
      (assert (= hash (sha256 (fs/path kit path))) (str "Generator differs: " path)))
    {:manifest-sha256 (sha256 frozen)
     :tooling (into (sorted-map) (for [file ["model.edn" "light-cljs.edn" "run-light.mjs" "oracle.bb" "check-waste.bb"]]
                                [file (sha256 (fs/path dir file))]))
     :population (select-keys manifest [:code-lines :physical-lines :bytes :worker-namespaces :source-namespaces])}))

(defn quiet! [environment]
  (let [snapshot (checked (str repository) environment ["top" "-l" "2" "-s" "1" "-n" "0"])
        idle (some-> (last (re-seq #"([0-9.]+)% idle" snapshot)) second parse-double)
        processes (checked (str repository) environment ["ps" "-axo" "pid,ppid,%cpu,comm"])]
    (assert (and idle (>= idle 90)) (str "Quiet gate: idle=" idle))
    (assert (not (re-find #"(?m)(?:/|\s)(?:rustc|cargo|goose|compiler-session(?:-[a-z]+)?)\s*$" processes)) "Competing build/compiler")
    {:idle-percent idle :at (str (java.time.Instant/now)) :processes processes}))

(defn verify-macro-output [dir environment expected reference]
  (let [output (fs/path dir "out/light/workload.main.js")
        got (mapv (fn [[input iterations]]
                    (checked dir environment ["node" "run-light.mjs" (str input) (str iterations)])) oracle-inputs)
        waste (checked dir environment ["bb" "check-waste.bb" "--strict" "out/light/workload.main.js"])
        hash (sha256 output)]
    (assert (= expected got) (str "Oracle mismatch " got))
    (if (fs/exists? reference)
      (assert (= (sha256 reference) hash) "Complete emitted JS differs from V8 (no normalization)")
      (fs/copy output reference))
    {:sha256 hash :bytes (fs/size output) :stdout got :waste waste}))

(defn source-macro-counts [path]
  (into (sorted-map)
        (for [line (str/split-lines (slurp (str path)))
              :when (str/starts-with? line "expanded/workload.")
              :let [[identity calls _] (str/split line #"\t")]]
          [identity (parse-long calls)])))

(defn verify-macro-counts [dir backend path]
  (let [counts (source-macro-counts path)
        reference (fs/path dir "macro-v8-counts.edn")]
    (assert (seq counts) "Instrumented build executed no generated source macros")
    (if (and (= backend "v8") (not (fs/exists? reference)))
      (spit (str reference) (pr-str counts))
      (assert (= (clojure.edn/read-string (slurp (str reference))) counts)
              "Executed source-macro identities/counts differ from V8"))
    counts))

(defn goose-diagnostics [path workers]
  (let [text (slurp (str path))
        [_ created peak joined] (re-find #"(?m)^THREADS 0 (\d+) (\d+) (\d+)$" text)
        pools (vec (for [line (str/split-lines text) :when (str/starts-with? line "POOL 0 ")]
                     (zipmap [:slot :chunks :resets :capacity-bytes :growths :reuses :regions :reset-ns
                              :code-bytes :heap-bytes :job-bytes :bridge-bytes]
                             (map parse-long (drop 2 (str/split line #" "))))))]
    (assert (str/includes? text "CLOSED 1") "Native Goose session did not close")
    (assert (= (str (+ 2 (* 2 workers))) created peak) "Native Goose thread total differs")
    ;; Native total includes its coordinator; every other thread is joined.
    (assert (= (str (+ 1 (* 2 workers))) joined) "Native Goose spawned threads did not all join")
    (assert (= workers (count pools)) "Missing private worker diagnostics")
    {:native-thread-total (parse-long created) :spawned-threads-joined (parse-long joined) :workers pools
     :trace-scope "bounded early protocol prefix (4096 rows); final POOL/THREADS/CLOSED replace prefix rows; session cost counters cover all jobs"}))

(defn comparison-cells [{:keys [backend workers compiler-slots]}]
  (let [coupled (vec (concat (when (#{"v8" "all"} backend) [{:backend "v8" :workers 1}])
                            (when (#{"goose" "all"} backend) (map #(hash-map :backend "goose" :workers %) workers))))]
    (if compiler-slots
      (vec (for [slots compiler-slots cell coupled] (assoc cell :compiler-slots slots)))
      coupled)))

(defn cell-slots [cell] (or (:compiler-slots cell) (:workers cell)))
(defn cell-name [cell]
  (str (:backend cell) (when (:compiler-slots cell) (str "-slots" (:compiler-slots cell) "-workers")) (:workers cell)))
(defn macro-command [light dir phase cell]
  (cond-> [light "release" "bench" "--optimize" "advanced" "--jobs" (str (cell-slots cell))]
    (#{"cached" "leaf"} phase) (into ["--cache-dir" (str (fs/path dir "macro-v8-cache"))])
    (not (#{"cached" "leaf"} phase)) (conj "--no-cache")))

(defn macro-main [{:keys [dir light runner backend workers compiler-slots phase runs timeout-ms leaf] :as opts}]
  (assert (#{"v8" "goose" "all"} backend))
  (assert (#{"check" "measure" "instrument" "cached" "leaf" "leaf-uncached"} phase))
  (assert (and (pos? runs) (or (not= phase "measure") (>= runs 5))))
  (assert (and (seq workers) (= (count workers) (count (distinct workers)))
               (every? #{1 2 4 8} workers)))
  (assert (pos? timeout-ms))
  (when compiler-slots
    (assert (and (seq compiler-slots) (= (count compiler-slots) (count (distinct compiler-slots)))
                 (every? #(and (integer? %) (pos? %)) compiler-slots))
            "--compiler-slots requires distinct positive integers"))
  (assert (or (= "v8" backend) runner) "Goose requires --runner optimized-native-binary")
  (assert (or (not (#{"cached" "leaf"} phase)) (= "v8" backend))
          "Goose cached artifact installation is not validated; use --backend v8 for these scenarios")
  (assert (or (not (#{"leaf" "leaf-uncached"} phase)) leaf) "leaf scenario requires --leaf relative/path.cljs")
  (when leaf
    (assert (some #(and (= leaf (:path %)) (= :worker (:role %)))
                  (:files (clojure.edn/read-string (slurp (str (fs/path dir "manifest.edn"))))))
            "Leaf must be a canonical worker source listed in the manifest"))
  (let [environment (apply dissoc (into {} (System/getenv))
                           ["LIGHT_CLJS_GOOSE_RUNNER" "LIGHT_CLJS_GOOSE_WORKERS" "LIGHT_CLJS_GOOSE_REPORT"
                            "LIGHT_CLJS_MACRO_PROBE" "GOOSE_BENCH_INSTRUMENT" "LIGHT_V8_STATS"
                            "GENERATED_COMPARE_TREE" "GOOSE_GENERATED_USAGE" "GOOSE_GENERATED_PROTOCOL"
                            "LIGHT_CLJS_NAMESPACE_WORKERS" "LIGHT_CLJS_FRESH_UNIT_ISOLATES" "LIGHT_CLJS_WAIT_TIMELINE"])
        results (fs/path dir (str "macro-" phase (when compiler-slots (str "-slots-" (str/join "-" compiler-slots)))))
        _ (fs/create-dirs results)
        evidence (fs/path results "samples.edn")
        _ (assert (not (fs/exists? evidence)) (str "Preserve existing samples: " evidence))
        diagnostic-runner (fs/path results "goose-instrumented.sh")
        _ (when (and runner (= "instrument" phase))
            ;; Existing native session diagnostic mode, excluded from clean runs.
            (spit (str diagnostic-runner) "#!/bin/bash\nset -o pipefail\n/usr/bin/time -l -o \"$GOOSE_GENERATED_USAGE\" \"$GOOSE_GENERATED_NATIVE\" \"$@\" instrumented | tee \"$GOOSE_GENERATED_PROTOCOL\"\nstatus=$?\nfor file in \"$1\"/phase-*.edn; do\n  if [ -f \"$file\" ]; then cp \"$file\" \"$GOOSE_GENERATED_PROTOCOL.${file##*/}\" || exit 1; fi\ndone\nexit \"$status\"\n")
            (checked dir environment ["chmod" "700" (str diagnostic-runner)]))
        write! #(spit (str evidence) (str (pr-str %) "\n") :append true)
        fixed (pinned! dir)
        reference (fs/path dir "macro-v8-reference.js")
        expected (mapv (fn [[i n]] (checked dir environment ["bb" "oracle.bb" (str i) (str n)])) oracle-inputs)
        cells (comparison-cells opts)
        _ (assert (or (not= backend "goose") (fs/exists? reference)) "Run V8 check first to establish complete JS reference")
        cmd (partial macro-command light dir phase)
        run-cell (fn [cell round warmup?]
                   (let [stem (str (cell-name cell) "-" (if warmup? "warmup" round))
                         log (fs/path results (str stem ".stderr"))
                         trace (fs/path results (str stem ".trace.tsv"))
                         tree (fs/path results (str stem ".tree.edn"))
                         env (cond-> environment
                               (= "goose" (:backend cell)) (assoc "LIGHT_CLJS_GOOSE_RUNNER" (if (= "instrument" phase) (str diagnostic-runner) runner)
                                                                  "GOOSE_GENERATED_NATIVE" runner
                                                                  "GOOSE_GENERATED_USAGE" (str (fs/path results (str stem ".native-rusage")))
                                                                  "GOOSE_GENERATED_PROTOCOL" (str (fs/path results (str stem ".protocol")))
                                                                  "LIGHT_CLJS_GOOSE_WORKERS" (str (:workers cell)))
                               (= "instrument" phase) (assoc "GENERATED_COMPARE_TREE" (str tree))
                               (#{"instrument" "cached" "leaf" "leaf-uncached"} phase) (assoc "LIGHT_CLJS_MACRO_PROBE" (str (fs/path results (str stem ".macros.tsv")))
                                                              "LIGHT_CLJS_GOOSE_REPORT" (str (fs/path results (str stem ".goose.txt")))))
                         command (cond-> (cmd cell) (#{"instrument" "cached" "leaf" "leaf-uncached"} phase) (into ["--stats-file" (str trace)]))
                         _ (fs/delete-tree (fs/path dir "out/light"))
                         quiet (when (= "measure" phase) (quiet! environment))
                         started (System/nanoTime)
                         r (try (bounded-run dir env timeout-ms (into ["/usr/bin/time" "-l"] command))
                                (catch Exception e {:exit -1 :out "" :err (str e)}))
                         wall (- (System/nanoTime) started)
                         _ (spit (str log) (:err r))
                         output (when (zero? (:exit r)) (verify-macro-output dir environment expected reference))
                         macro-counts (when (and (#{"instrument" "leaf-uncached"} phase) (zero? (:exit r)))
                                        (verify-macro-counts dir (:backend cell)
                                          (fs/path results (str stem ".macros.tsv"))))
                         native (when (and (= "instrument" phase) (= "goose" (:backend cell)) (zero? (:exit r)))
                                  (goose-diagnostics (fs/path results (str stem ".goose.txt")) (:workers cell)))
                         err (:err r)
                         number #(some-> (re-find % err) second parse-double)
                         sample (merge cell {:kind (if warmup? :warmup :sample) :round round :phase phase :command command
                                             :compiler-slots (cell-slots cell) :execution-workers (:workers cell)
                                             :stdout (:out r)
                                             :leaf-edit *leaf-edit*
                                             :executed-source-macros macro-counts
                                             :native native
                                             :process-tree (when (= "instrument" phase) (str (fs/file-name tree)))
                                             :wall-ns wall :exit (:exit r) :output output :stderr (str (fs/file-name log))
                                             :user-s (number #"([0-9.]+) user") :system-s (number #"([0-9.]+) sys")
                                             :max-rss-bytes (number #"([0-9]+)  maximum resident set size") :quiet quiet})]
                     (pinned! dir)
                     (write! sample)
                     (println (select-keys sample [:backend :workers :kind :round :exit :wall-ns]))
                     (when (and (= "instrument" phase) (zero? (:exit r)))
                       (spit (str (fs/path results (str stem ".dag.txt")))
                             (checked (str repository) environment ["bb" "benchmarks/concurrency/dag.bb" (str trace) "--workers" "2,4,8"])))
                     sample))]
    (when (= phase "measure")
      (assert (str/includes? (System/getProperty "os.name") "Mac") "Quiet/rusage measurement currently requires macOS"))
    (write! {:kind :manifest :phase phase :options opts :fixed fixed :inputs oracle-inputs :expected expected
             :harness-sha256 (sha256 (fs/path kit "compare.bb"))
             :native-diagnostic-mode (when (= "instrument" phase) "compiler_session instrumented; tee preserves full protocol; inner time records native rusage; 100ms owned-tree RSS sampling")
             :compiler-sha256 (sha256 light) :runner-sha256 (when runner (sha256 runner))
             :git (checked (str repository) environment ["git" "rev-parse" "HEAD"])
             :source-hashes (into (sorted-map)
                                  (for [path (str/split-lines (checked (str repository) environment ["git" "ls-files" "src" "crates/big-boy-pants-js" "Cargo.toml" "Cargo.lock" "build.rs"]))
                                        :when (or (re-find #"\.(rs|goose|js|cljs|cljc|c|h|build|tsv|toml|lock)$" path) (#{"Cargo.toml" "Cargo.lock" "build.rs"} path))]
                                    [path (sha256 (fs/path repository path))]))
             :source-diff (checked (str repository) environment ["git" "diff" "--binary"])
             :toolchains (into {} (for [c [["rustc" "--version"] ["goose" "version"] ["bb" "--version"] ["node" "--version"] ["clang" "--version"] ["uname" "-a"]]]
                                   [c (checked (str repository) environment c)]))
             :memory-scope "macOS time -l compiler command rusage; not additive process-tree peak; Goose native stats separate"
             :boundary "Goose source-macro execution with V8 core/bootstrap; Node application; fresh process per run"})
    (let [warmups (mapv #(run-cell % -1 true) cells)
          viable (mapv first (filter #(zero? (:exit (second %))) (map vector cells warmups)))
          run-rounds #(doall (for [round (range runs) cell (if (even? round) viable (reverse viable))]
                               (run-cell cell round false)))
          samples (if (#{"leaf" "leaf-uncached"} phase)
                    (with-restored (fs/path dir leaf)
                      (fn [original]
                        ;; Each round appends a distinct inert definition, as the existing compare leaf scenario does.
                        (doall (for [round (range runs)]
                                 (do (spit (str (fs/path dir leaf)) (str original "\n(defn compare-leaf-edit-" round " [] " round ")\n"))
                                     (binding [*leaf-edit* {:path leaf :sha256 (sha256 (fs/path dir leaf))}]
                                       (mapv #(run-cell % round false) (if (even? round) viable (reverse viable)))))))))
                    (run-rounds))]
      (pinned! dir)
      (write! {:kind :complete :success (every? #(zero? (:exit %)) (concat warmups (flatten samples)))})
      (when-not (every? #(zero? (:exit %)) (concat warmups (flatten samples)))
        (throw (ex-info "One or more selected compiler cells failed; see preserved diagnostics (no fallback)" {:samples (str evidence)})))
      (println "Wrote" (str evidence)))))


(defn legacy-main [args]
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

(defn main [args]
  (let [options (parse-options args)]
    (if (:backend options) (macro-main options) (legacy-main args))))

(when (= (fs/absolutize *file*)
         (some-> (System/getProperty "babashka.file") fs/absolutize))
  (main *command-line-args*))
