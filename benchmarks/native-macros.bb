#!/usr/bin/env bb
;; Real native compiler macros; see benchmarks/native-macros.md.
(require '[babashka.fs :as fs]
         '[babashka.process :as p]
         '[cheshire.core :as json]
         '[clojure.string :as str])

(def repo (str (fs/parent (fs/parent (fs/absolutize *file*)))))
(def source-root (str (fs/path repo "benchmarks/fixtures/native-macros")))
(def fixtures
  [{:name "collections" :entry "native-bench.collections" :native-macro-calls 13
    :expected "(1 2) ([:a 1]) (\"a\" \"b\")\ntrue true true true\ntrue true true true true\n2\n"}
   {:name "environment" :entry "native-bench.environment" :native-macro-calls 16
    :expected (str (str/join " " (repeat 4 "native-bench.environment")) "\n"
                   (str/join " " (repeat 4 "native-bench.environment")) "\n1 1 1 1\n42 42\n42 42\n")}
   {:name "helper" :entry "native-bench.helper" :native-macro-calls 1 :expected "[42 1]\n"
    :proof-required "Compiler trace must prove actual helper park/resume and no effect replay; output alone is insufficient (light-cljs-463)."}])

(defn sha256 [file]
  (when (fs/exists? file)
    (let [digest (java.security.MessageDigest/getInstance "SHA-256")]
      (.update digest (fs/read-all-bytes file))
      (apply str (map #(format "%02x" %) (.digest digest))))))

(defn command [args]
  (let [result (apply p/shell {:dir repo :out :string :err :string :continue true} args)]
    {:command args :exit (:exit result) :stdout (:out result) :stderr (:err result)}))

(defn timed-process [args timeout-ms]
  (let [begun (System/nanoTime)
        proc (p/process args {:dir repo :out :string :err :string :shutdown p/destroy-tree})
        result (deref proc timeout-ms ::timeout)
        timed-out? (= ::timeout result)]
    (when timed-out?
      ;; Kill descendants before reaping time(1); a compiler that ignores TERM
      ;; must not turn the runner's timeout into an unbounded wait.
      (with-open [children (.descendants (:proc proc))]
        (doseq [child (iterator-seq (.iterator children))] (.destroyForcibly child)))
      (.destroyForcibly (:proc proc)))
    (let [result (if (= ::timeout result) @proc result)]
      {:exit (:exit result) :stdout (:out result) :stderr (:err result)
       :timeout timed-out?
       :wall-seconds (/ (- (System/nanoTime) begun) 1e9)})))

(defn parse-options [args]
  (loop [[arg & more] args options {:runs 5 :jobs [1 2 4 8] :timeout-ms 120000}]
    (case arg
      nil (do (when-not (:output options) (throw (ex-info "Usage: bb benchmarks/native-macros.bb OUTPUT [--smoke] [--jobs 1,2,4,8] [--timeout-ms N] [--v8-bin PATH --v8-revision SHA]" {}))) options)
      "--smoke" (recur more (assoc options :runs 1 :smoke true))
      "--jobs" (let [jobs (mapv parse-long (str/split (or (first more) "") #","))]
                 (when-not (and (seq jobs) (every? #{1 2 4 8} jobs) (= (count jobs) (count (distinct jobs))))
                   (throw (ex-info "--jobs must be distinct members of 1,2,4,8" {})))
                 (recur (rest more) (assoc options :jobs jobs)))
      "--timeout-ms" (let [n (parse-long (or (first more) ""))]
                       (when-not (and n (pos? n)) (throw (ex-info "Positive timeout required" {})))
                       (recur (rest more) (assoc options :timeout-ms n)))
      "--v8-bin" (recur (rest more) (assoc options :v8-bin (some-> (first more) fs/absolutize str)))
      "--v8-revision" (recur (rest more) (assoc options :v8-revision (first more)))
      (if (or (:output options) (str/starts-with? arg "--"))
        (throw (ex-info (str "Unexpected argument: " arg) {}))
        (recur more (assoc options :output (str (fs/absolutize arg))))))))

(defn schedule [{:keys [runs jobs]} backends]
  (for [round (range runs) fixture fixtures
        [job-index jobs] (map-indexed vector (if (even? round) jobs (reverse jobs)))
        backend (if (even? (+ round job-index)) backends (reverse backends))]
    {:round (inc round) :fixture fixture :jobs jobs :backend backend}))

(defn stats [stderr]
  (into (sorted-map)
        (for [line (str/split-lines stderr)
              :let [[_ key value] (re-matches #"([a-z][a-z0-9.-]*) (-?[0-9.]+)" line)]
              :when key] [key (parse-double value)])))

(defn unavailable [reason] {:unavailable reason})
(defn measured [value unit reason]
  (if (some? value) {:value value :unit unit} (unavailable reason)))

(defn metrics [sample darwin?]
  (let [s (stats (:stderr sample))
        user (if darwin? (second (re-find #"([0-9.]+)\s+user" (:stderr sample)))
                 (second (re-find #"User time \(seconds\): ([0-9.]+)" (:stderr sample))))
        rss (if darwin? (second (re-find #"(\d+)\s+maximum resident set size" (:stderr sample)))
                (second (re-find #"Maximum resident set size \(kbytes\): (\d+)" (:stderr sample))))
        bridge (seq (for [[k v] s :when (re-matches #"macro.bridge.\d+.ns" k)] v))]
    {:wall (measured (:wall-seconds sample) "seconds" "Process did not start")
     :user-cpu (measured (some-> user parse-double) "seconds" "Platform time did not emit user CPU")
     :peak-rss (measured (when rss (* (parse-long rss) (if darwin? 1 1024))) "bytes" "Platform time did not emit peak RSS")
     :form-bridge (measured (when bridge (reduce + bridge)) "nanoseconds" "No macro.bridge.N.ns counters in CLI stats")
     :runtime-initialization (unavailable "CLI trace does not isolate native runtime initialization")
     :js-compilation (unavailable "Trace engine-us combines evaluation work; no separate JS compilation counter")
     :js-execution (unavailable "Trace engine-us combines evaluation work; no separate JS execution counter")
     :allocation (unavailable "CLI exposes no total allocated native engine bytes")
     :gc-count (unavailable "CLI exposes no native GC collection count")
     :gc-pauses (unavailable "CLI exposes no native GC pause durations")
     :parked-tasks (unavailable "pool.parks counts worker waits, not suspended native macro tasks")
     :help-while-waiting-depth (unavailable "No native scheduler help-depth counter is exposed")
     :worker-utilization (measured (get s "pool.utilization") "ratio" "No compiler worker utilization for serial native backend")}))

(defn unit-identities [trace]
  (vec (for [line (str/split-lines trace) :when (str/starts-with? line "unit\t")
             :let [fields (str/split line #"\t" -1)]]
         {:id (get fields 1) :namespace (get fields 2) :name (get fields 3)
          :line (get fields 4) :column (get fields 5) :engine-us (get fields 7)
          :engine-calls (get fields 8)})))

(defn identity-key [row] (select-keys row [:js-sha256 :map-sha256 :result]))
(defn result-status [fixture sample result has-map?]
  (cond (:timeout sample) "timeout"
        (not= 0 (:exit sample)) (if (str/includes? (:stderr sample) "unsupported operation:") "unsupported" "compile-failure")
        (or (:timeout result) (not= 0 (:exit result)) (not= (:expected fixture) (:stdout result))) "result-failure"
        (not has-map?) "identity-failure"
        (:proof-required fixture) "unverified-helper"
        :else "ok"))

(defn validate-identities [rows]
  (let [bad (set (for [[fixture rs] (group-by :fixture rows)
                       :let [valid (filter #(= "ok" (:status %)) rs)]
                       :when (> (count (set (map identity-key valid))) 1)] fixture))]
    (mapv #(if (and (= "ok" (:status %)) (bad (:fixture %)))
             (assoc % :status "identity-failure" :diagnostic "JavaScript, source-map or result differs across runs/jobs/backends; all fixture samples excluded") %) rows)))

(defn median [xs]
  (let [xs (vec (sort xs)) n (count xs)]
    (when (pos? n) (/ (+ (xs (quot n 2)) (xs (quot (dec n) 2))) 2.0))))
(defn summarize [rows required]
  (vec (for [[[fixture backend jobs] rs] (sort-by key (group-by (juxt :fixture :backend :jobs) rows))
             :let [valid (filter #(= "ok" (:status %)) rs)]]
         {:fixture fixture :backend backend :jobs jobs :samples (count valid)
          :performance-eligible (and (= 5 required) (= required (count valid)))
          :statuses (frequencies (map :status rs))
          :metrics (into (sorted-map)
                         (for [metric (keys (:metrics (first rs)))
                               :let [values (keep #(get-in % [:metrics metric :value]) valid)]
                               :when (seq values)]
                           [metric {:median (median values) :min (apply min values) :max (apply max values)
                                    :spread (- (apply max values) (apply min values))
                                    :unit (get-in (first valid) [:metrics metric :unit])}]))})))

(defn run-cell [options work order {:keys [round fixture jobs backend]}]
  (let [dir (fs/path (:output options) (format "%03d-%s-%s-j%d" order (:name fixture) (:name backend) jobs))
        map-file (fs/path work "out.map") trace-file (fs/path work "trace.tsv")
        _ (doseq [file [map-file trace-file]] (fs/delete-if-exists file))
        _ (fs/create-dirs dir)
        args [(:binary backend) "--compile" "--entry" (:entry fixture) "--source-root" source-root
              "--jobs" (str jobs) "--no-cache" "--optimize" "none"
              "--macro-timeout-ms" "60000" "--macro-poll-budget" "100000000"
              "--stats-file" (str trace-file) "--source-map" (str map-file) "--source-map-include-sources"]
        darwin? (= "Mac OS X" (System/getProperty "os.name"))
        timed (into ["/usr/bin/time" (if darwin? "-l" "-v")] args)
        load (command ["uptime"])
        sample (timed-process timed (:timeout-ms options))
        _ (spit (str (fs/path dir "out.js")) (:stdout sample))
        _ (spit (str (fs/path dir "stderr.txt")) (:stderr sample))
        _ (doseq [file [map-file trace-file] :when (fs/exists? file)] (fs/copy file (fs/path dir (fs/file-name file))))
        result (when (zero? (:exit sample)) (timed-process [(or (System/getenv "NODE") "node") (str (fs/path dir "out.js"))] (:timeout-ms options)))
        status (result-status fixture sample result (fs/exists? map-file))]
    (merge (select-keys sample [:exit :wall-seconds :timeout])
           {:order order :round round :fixture (:name fixture) :backend (:name backend) :jobs jobs
            :command timed :cwd repo :timeout-ms (:timeout-ms options) :load load :status status
            :diagnostic (case status "ok" nil "unverified-helper" (:proof-required fixture)
                              "result-failure" (str "Expected " (pr-str (:expected fixture)) "; Node returned " (pr-str result))
                              "identity-failure" "Compiler did not write its source map"
                              "timeout" "Compiler process exceeded runner timeout; killed process tree" (:stderr sample))
            :result (:stdout result) :node-command (when result [(or (System/getenv "NODE") "node") (str (fs/path dir "out.js"))])
            :js-sha256 (sha256 (fs/path dir "out.js")) :map-sha256 (sha256 map-file)
            :trace-file (str (fs/path dir "trace.tsv")) :stats (stats (:stderr sample))
            :units (if (fs/exists? trace-file) (unit-identities (slurp (str trace-file))) [])
            :unit-matching-note "Match namespace/name/line/column only within identical fixture hash and repository revision; retain raw trace for demander edges. These top-level fixtures currently emit no native unit rows, so a per-unit V8 ratio is unavailable."
            :metrics (metrics sample darwin?) :artifacts (str dir)})))

(defn native-build []
  (let [rustc (command ["rustc" "-vV"])
        target (second (re-find #"(?m)^host: (\S+)$" (:stdout rustc)))
        target-dir (str (fs/path repo "target"))]
    (when-not (and (zero? (:exit rustc)) target)
      (throw (ex-info "Cannot determine native rustc host target" rustc)))
    {:target target :target-dir target-dir
     :binary (str (fs/path target-dir target "release/light-cljs"))
     :command ["cargo" "build" "--locked" "--release" "--no-default-features" "--features" "bbp-macros"
               "--target" target "--target-dir" target-dir]}))

(defn main [args]
  (let [options (parse-options args)
        output (:output options)
        revision (str/trim (:stdout (command ["git" "rev-parse" "HEAD"])))
        {:keys [binary target target-dir] build :command} (native-build)]
    (when (fs/exists? output) (throw (ex-info "Output must be a new directory" {:output output})))
    (when (and (:v8-bin options) (not= revision (:v8-revision options)))
      (throw (ex-info "--v8-revision must attest the existing V8 binary was built from this exact HEAD" {:revision revision})))
    (when (and (:v8-bin options) (or (not (fs/exists? (:v8-bin options))) (= (str (fs/canonicalize (:v8-bin options))) (str (fs/canonicalize binary)))))
      (throw (ex-info "V8 reference must be an existing separate binary (never built by this runner)" {})))
    (fs/create-dirs output)
    (println "Building explicit native-only release backend (3 build jobs)")
    (let [built (apply p/shell {:dir repo :extra-env {"CARGO_BUILD_JOBS" "3"} :out :string :err :string :continue true} build)]
      (spit (str (fs/path output "build.log")) (str (:out built) (:err built)))
      (when-not (zero? (:exit built)) (throw (ex-info "Native release build failed; see build.log" {}))))
    (let [backends (cond-> [{:name "bbp-macros" :binary binary}]
                     (:v8-bin options) (conj {:name "v8" :binary (:v8-bin options)}))
          metadata {:schema 1 :revision revision :git-status (command ["git" "status" "--porcelain"])
                    :source-diff-sha256 (do (spit (str (fs/path output "source.diff")) (:stdout (command ["git" "diff" "HEAD"]))) (sha256 (fs/path output "source.diff")))
                    :runner-sha256 (sha256 *file*) :started-at (str (java.time.Instant/now))
                    :options options :build-command build :build-jobs 3 :build-target target :build-target-dir target-dir :profile "Cargo release (repository Cargo.toml)"
                    :cargo-toml-sha256 (sha256 (fs/path repo "Cargo.toml")) :lock-sha256 (sha256 (fs/path repo "Cargo.lock"))
                    :build-environment (into (sorted-map) (filter (fn [[k _]] (or (str/starts-with? k "CARGO_PROFILE_RELEASE_") (str/starts-with? k "CARGO_TARGET_") (#{"RUSTFLAGS" "CARGO_ENCODED_RUSTFLAGS" "RUSTC_WRAPPER" "RUSTUP_TOOLCHAIN" "CC" "CFLAGS"} k))) (System/getenv)))
                    :cache-policy "--no-cache; each measured compiler and Node invocation is a fresh process; OS filesystem cache is not flushed; no warmups"
                    :toolchain (mapv command [["rustc" "-Vv"] ["cargo" "-V"] ["bb" "--version"] [(or (System/getenv "NODE") "node") "--version"]])
                    :machine (command ["uname" "-a"]) :cpus (.availableProcessors (Runtime/getRuntime))
                    :backends (mapv #(assoc % :sha256 (sha256 (:binary %)) :provenance (if (= "v8" (:name %)) "User-attested existing V8 binary and revision; build settings not reconstructed" "Native-only release build above")) backends)
                    :fixtures (mapv #(let [path (fs/path source-root "native_bench" (str (:name %) ".cljs"))]
                                       (assoc % :path (str path) :bytes (fs/size path) :sha256 (sha256 path))) fixtures)}
          work (fs/create-dirs (fs/path output "work"))
          rows (loop [[cell & more] (schedule options backends) rows [] stopped #{}]
                 (if-not cell rows
                         (let [key [(:name (:fixture cell)) (:name (:backend cell)) (:jobs cell)]]
                           (if (stopped key) (recur more rows stopped)
                               (let [row (run-cell options work (inc (count rows)) cell)]
                                 (println (:order row) key (:status row)) (flush)
                                 (spit (str (fs/path output "raw.jsonl")) (str (json/generate-string row) "\n") :append true)
                                 (recur more (conj rows row) (if (= "ok" (:status row)) stopped (conj stopped key))))))))
          checked (validate-identities rows)
          report (assoc metadata :samples checked :summary (summarize checked (:runs options)))]
      (spit (str (fs/path output "report.json")) (json/generate-string report {:pretty true}))
      (println "Report:" (str (fs/path output "report.json")))
      (when (some #(#{"compile-failure" "result-failure" "identity-failure" "timeout"} (:status %)) checked) (System/exit 1)))))

(when (= (System/getProperty "babashka.file") *file*) (main *command-line-args*))
