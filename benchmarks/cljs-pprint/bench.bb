#!/usr/bin/env bb
;; Measure compile, cache, watch, test, and Node load cost of requiring
;; cljs.pprint in light-cljs against shadow-cljs 3.5.3.
;;
;; Usage:
;;   bb benchmarks/cljs-pprint/bench.bb --light BINARY [--runs 10]
;;                                      [--light-commit SHA] [--out PREFIX]
;;                                      [--only SCENARIOS]
;;
;; Scenarios (all run by default; comma-separated for --only):
;;   dev-compile, dev-load, leaf-cache, uncached-prepare, uncached-release,
;;   release-load, watch, test
;;
;; Never waits for a quiet machine: records the 1-minute load average before
;; every run so all figures are labeled unreserved.
(ns pprint-bench.bench
  (:require [babashka.fs :as fs]
            [babashka.process :as process]
            [clojure.java.io :as io]
            [clojure.pprint :as pp]
            [clojure.string :as str]))

(def here (fs/parent (fs/absolutize *file*)))
(def repo (fs/parent (fs/parent here)))
(def mac? (str/includes? (System/getProperty "os.name") "Mac"))

(def twin-programs
  [{:id "prn"         :ns "pprint-bench.prn"         :file "src/pprint_bench/prn.cljs"}
   {:id "pprint-only" :ns "pprint-bench.pprint-only" :file "src/pprint_bench/pprint_only.cljs"}])

(def test-programs
  [{:id "prn-test"    :ns "pprint-bench.prn-test"    :file "test/pprint_bench/prn_test.cljs"}
   {:id "pprint-test" :ns "pprint-bench.pprint-test" :file "test/pprint_bench/pprint_test.cljs"}])

(def all-scenarios
  ["dev-compile" "dev-load" "leaf-cache" "uncached-prepare"
   "uncached-release" "release-load" "watch" "test"])

(defn log [& parts]
  (binding [*out* *err*]
    (apply println parts)
    (flush)))

(defn parse-args [args]
  (loop [[a & more] args opts {:runs 10 :only (set all-scenarios)}]
    (cond
      (nil? a) opts
      (= a "--light") (recur (rest more) (assoc opts :light (str (fs/absolutize (first more)))))
      (= a "--light-commit") (recur (rest more) (assoc opts :light-commit (first more)))
      (= a "--runs") (recur (rest more) (assoc opts :runs (parse-long (first more))))
      (= a "--out") (recur (rest more) (assoc opts :out (str (fs/absolutize (first more)))))
      (= a "--only") (recur (rest more) (assoc opts :only (set (str/split (first more) #","))))
      :else (throw (ex-info (str "Unknown arg: " a) {})))))

(defn light-binary [opts]
  (let [bin (or (:light opts) (System/getenv "LIGHT_CLJS_BINARY")
                (str (fs/path repo "target/release/light-cljs")))]
    (when-not (fs/executable? bin)
      (throw (ex-info (str "No executable light-cljs binary at " bin) {})))
    bin))

(defn sh! [opts & cmd]
  (let [r (apply process/sh (merge {:dir (str here)} opts) (map str cmd))]
    (when-not (zero? (:exit r))
      (let [err (str (:err r) "\n" (:out r))]
        (throw (ex-info (str "Failed: " (str/join " " cmd) "\n"
                             (subs err (max 0 (- (count err) 3000)))) {}))))
    r))

(defn command-text [& cmd]
  (try (let [{:keys [out err]} (apply process/sh cmd)]
         (str/trim (str out err)))
       (catch Exception _ nil)))

(defn sha256-file [file]
  (let [digest (java.security.MessageDigest/getInstance "SHA-256")]
    (with-open [in (java.io.FileInputStream. (str file))]
      (let [buf (byte-array 65536)]
        (loop []
          (let [n (.read in buf)]
            (when (pos? n)
              (.update digest buf 0 n)
              (recur))))))
    (apply str (map #(format "%02x" %) (.digest digest)))))

(defn load-average []
  (let [text (if mac? (:out (process/sh "sysctl" "-n" "vm.loadavg")) (slurp "/proc/loadavg"))]
    (mapv parse-double (take 3 (re-seq #"[0-9]+(?:\.[0-9]+)?" text)))))

(defn parse-time [err]
  (let [number (fn [re] (some-> (re-find re err) second parse-double))]
    (if mac?
      {:user-s (number #"([0-9.]+) user")
       :system-s (number #"([0-9.]+) sys")
       :peak-memory-mb (/ (number #"([0-9]+)  maximum resident set size") 1e6)}
      {:user-s (number #"User time \(seconds\): ([0-9.]+)")
       :system-s (number #"System time \(seconds\): ([0-9.]+)")
       :peak-memory-mb (/ (number #"Maximum resident set size \(kbytes\): ([0-9]+)") 1e3)})))

(defn parse-stats [err]
  (when-let [m (re-find #"(?s)(light-cljs-stats [0-9]+\n.*)" err)]
    (let [raw (first (str/split (second m) #"(?m)^\s*[0-9.]+ real |^\tCommand being timed:" 2))
          body (first (str/split raw #"(?m)^\[" 2))
          counters (into (sorted-map)
                         (for [[_ k v] (re-seq #"(?m)^([a-z][a-z0-9_.-]*) ([0-9]+(?:\.[0-9]+)?)$" body)]
                           [k (if (str/includes? v ".") (parse-double v) (parse-long v))]))]
      {:counters counters :raw (str/trim raw)})))

(defn timed-run
  "Run cmd under /usr/bin/time (-l on macOS, -v on Linux) and measure wall time
  with System/nanoTime. Returns {:sample ... :out ... :err ...}."
  [dir cmd]
  (let [load (load-average)
        start (System/nanoTime)
        res (apply process/sh {:dir (str dir)} "/usr/bin/time" (if mac? "-l" "-v") (map str cmd))
        end (System/nanoTime)
        err (:err res)]
    (when-not (zero? (:exit res))
      (throw (ex-info (str "Command failed: " (str/join " " cmd) "\n"
                           (subs err (max 0 (- (count err) 3000)))) {})))
    (let [t (parse-time err)
          stats (when (some #{"--stats"} cmd) (parse-stats err))
          sample (cond-> (assoc t
                                :wall-s (/ (- end start) 1e9)
                                :cpu-s (+ (or (:user-s t) 0.0) (or (:system-s t) 0.0))
                                :load load)
                   stats (assoc :stats (:counters stats)))]
      {:sample sample :out (:out res) :err err :stats-raw (:raw stats)})))

(defn timed-streaming
  "Run cmd under /usr/bin/time, measuring nanosecond time to first byte of
  stdout and total wall time. Returns {:sample ... :out ...}."
  [dir cmd]
  (let [load (load-average)
        start (System/nanoTime)
        proc (apply process/process {:dir (str dir) :out :stream :err :string}
                    "/usr/bin/time" (if mac? "-l" "-v") (map str cmd))
        in (:out proc)
        first-byte (.read ^java.io.InputStream in)
        first-at (System/nanoTime)
        buf (java.io.ByteArrayOutputStream.)]
    (when (>= first-byte 0)
      (.write buf first-byte)
      (io/copy in buf))
    (let [{:keys [exit err]} @proc
          end (System/nanoTime)]
      (when-not (zero? exit)
        (throw (ex-info (str "Run failed: " (str/join " " cmd) "\n"
                             (subs err (max 0 (- (count err) 3000)))) {})))
      (let [t (parse-time err)]
        {:sample (assoc t
                        :first-output-s (when (>= first-byte 0) (/ (- first-at start) 1e9))
                        :wall-s (/ (- end start) 1e9)
                        :cpu-s (+ (or (:user-s t) 0.0) (or (:system-s t) 0.0))
                        :load load)
         :out (.toString buf "UTF-8")}))))

(defn with-restored [file f]
  (let [original (fs/read-all-bytes file)
        restore #(fs/write-bytes file original)
        hook (Thread. ^Runnable restore)]
    (.addShutdownHook (Runtime/getRuntime) hook)
    (try
      (f (String. original "UTF-8"))
      (finally
        (restore)
        (try (.removeShutdownHook (Runtime/getRuntime) hook)
             (catch IllegalStateException _))))))

(defn with-all-restored [files f]
  (if-let [[file & more] (seq files)]
    (with-restored file (fn [orig] (with-all-restored more #(f (assoc % file orig)))))
    (f {})))

;; Statistics helpers

(defn median [xs]
  (let [v (vec (sort xs)) n (count v) h (quot n 2)]
    (if (odd? n) (v h) (/ (+ (v (dec h)) (v h)) 2.0))))

(defn spread [xs]
  {:median (double (median xs))
   :min (double (apply min xs))
   :max (double (apply max xs))})

(defn summarize [samples]
  (let [keys-to-sum (cond-> [:wall-s :cpu-s :user-s :system-s :peak-memory-mb]
                      (every? :first-output-s samples) (conj :first-output-s)
                      (every? :step-ms samples) (conj :step-ms)
                      (every? :e2e-ms samples) (conj :e2e-ms))]
    (into {:runs (count samples)
           :load-1m (spread (map (comp first :load) samples))
           :samples samples}
          (for [k keys-to-sum :when (every? k samples)]
            [k (spread (map k samples))]))))

(defn fmt-ms [{:keys [median min max]}]
  (format "%.1f ms (%.1f-%.1f)" (* 1000.0 median) (* 1000.0 min) (* 1000.0 max)))

(defn fmt-raw-ms [{:keys [median min max]}]
  (format "%.1f ms (%.1f-%.1f)" median min max))

(defn fmt-s [{:keys [median min max]}]
  (format "%.3f s (%.3f-%.3f)" median min max))

(defn fmt-mb [{:keys [median min max]}]
  (format "%.1f MB (%.1f-%.1f)" median min max))

(defn fmt-load [{:keys [min max]}]
  (format "%.2f-%.2f" min max))

(defn shadow-version []
  (let [form (str "(println \"SHADOW-VERSION\" "
                  "(some->> (clojure.java.io/resource \"META-INF/maven/thheller/shadow-cljs/pom.properties\") "
                  "slurp (re-find #\"version=(\\S+)\") second))")
        out (:out (sh! {} "clojure" "-M:shadow" "clj-eval" form))]
    (or (second (re-find #"SHADOW-VERSION (\S+)" out)) "unknown")))

(defn environment [bin opts]
  (let [sysctl #(command-text "sysctl" "-n" %)]
    {:date (str (java.time.LocalDate/now))
     :light-cljs {:binary bin
                  :sha256 (sha256-file bin)
                  :commit (or (:light-commit opts)
                              (second (re-find #"light-cljs-([0-9a-f]{7,40})$" (str (fs/file-name bin)))))}
     :shadow-cljs (shadow-version)
     :jdk (let [line (first (str/split-lines (or (command-text "java" "-version") "")))]
            (if-let [[_ vendor ver] (re-find #"^(\S+) version \"([^\"]+)\"" line)]
              (str vendor " " ver) line))
     :node (command-text "node" "--version")
     :clojure-cli (command-text "clojure" "--version")
     :babashka (System/getProperty "babashka.version")
     :os (if mac? (str "macOS " (command-text "sw_vers" "-productVersion")) (command-text "uname" "-sr"))
     :machine (if mac?
                {:model (sysctl "hw.model")
                 :cpu (sysctl "machdep.cpu.brand_string")
                 :cpus (some-> (sysctl "hw.ncpu") parse-long)
                 :memory-gib (some-> (sysctl "hw.memsize") parse-long (quot (* 1024 1024 1024)))}
                {:cpus (.availableProcessors (Runtime/getRuntime))})}))

;; Runner helpers

(defn write-light-adv-runner! [out-dir ns]
  (spit (str (fs/path out-dir "package.json")) "{\"type\":\"module\"}\n")
  (let [runner (fs/path out-dir "run.mjs")]
    (spit (str runner) (str "import main from " (pr-str (str "./" ns ".js")) ";\nmain();\n"))
    (str runner)))

(defn write-light-dev-runner! [out-dir]
  (let [runner (fs/path out-dir "run.cjs")]
    (spit (str runner) "require(\"./index.js\");\nglobalThis.__lightDevLoaded.then((main) => main());\n")
    (str runner)))

;; Scenarios

(defn run-dev-compile [bin runs]
  (log "== Scenario: warm dev compile (light-cljs --reloadable vs shadow-cljs compile)")
  (let [cache-dir (str (fs/path here "out/.bench-cache-dev"))]
    (fs/delete-tree cache-dir)
    (fs/delete-tree (fs/path here ".shadow-cljs"))
    ;; Warm both caches once (untimed)
    (doseq [{:keys [id ns]} twin-programs]
      (let [out-dir (str (fs/path here (str "out/bench-dev-" id)))]
        (fs/delete-tree out-dir)
        (sh! {} bin "--compile" "--reloadable" "--entry" ns "--source-root" "src"
             "--cache-dir" cache-dir "--out-dir" out-dir)
        (write-light-dev-runner! out-dir))
      (sh! {} "clojure" "-M:shadow" "compile" id))
    (let [acc (atom {})]
      (doseq [r (range runs)
              :let [tools (if (even? r) [:light :shadow] [:shadow :light])
                    progs (if (even? r) twin-programs (vec (rseq twin-programs)))]]
        (doseq [tool tools {:keys [id ns]} progs]
          (let [out-dir (str (fs/path here (str "out/bench-dev-" id)))
                cmd (case tool
                      :light [bin "--compile" "--reloadable" "--entry" ns "--source-root" "src"
                              "--cache-dir" cache-dir "--stats" "--out-dir" out-dir]
                      :shadow ["clojure" "-M:shadow" "compile" id])
                {:keys [sample]} (timed-run here cmd)]
            (when (and (= tool :light) (= id "pprint-only"))
              (let [hits (get-in sample [:stats "cache.hits"] 0)]
                (when (< hits 2)
                  (throw (ex-info (str "Expected cache hits on warm dev compile, got " (:stats sample)) {})))))
            (log (format "  dev-compile r=%d %s %s wall=%.3fs peak=%.1fMB load=%.2f"
                         r (name tool) id (:wall-s sample) (:peak-memory-mb sample) (first (:load sample))))
            (swap! acc update [tool id] (fnil conj []) sample))))
      ;; Re-write light dev runners so dev-load can run immediately
      (doseq [{:keys [id]} twin-programs]
        (write-light-dev-runner! (fs/path here (str "out/bench-dev-" id))))
      (into {} (for [[k v] @acc] [k (summarize v)])))))

(defn run-dev-load [runs]
  (log "== Scenario: dev build load in Node (first output)")
  (let [acc (atom {})
        outputs (atom {})]
    (doseq [r (range runs)
            :let [tools (if (even? r) [:light :shadow] [:shadow :light])
                  progs (if (even? r) twin-programs (vec (rseq twin-programs)))]]
      (doseq [tool tools {:keys [id]} progs]
        (let [cmd (case tool
                    :light ["node" (str (fs/path here (str "out/bench-dev-" id "/run.cjs")))]
                    :shadow ["node" (str (fs/path here (str "out/shadow-" id ".js")))])
              {:keys [sample out]} (timed-streaming here cmd)]
          (if-let [prev (get @outputs id)]
            (when-not (= prev out)
              (throw (ex-info (str "Dev stdout mismatch for " id "\nexpected:\n" prev "\ngot:\n" out) {})))
            (swap! outputs assoc id out))
          (swap! acc update [tool id] (fnil conj []) sample)
          (log (format "  dev-load r=%d %s %s first=%.1fms wall=%.1fms peak=%.1fMB load=%.2f"
                       r (name tool) id (* 1000.0 (:first-output-s sample)) (* 1000.0 (:wall-s sample))
                       (:peak-memory-mb sample) (first (:load sample)))))))
    (into {} (for [[k v] @acc] [k (summarize v)]))))

(defn run-leaf-cache [bin runs]
  (log "== Scenario: cached build after one-file edit of calling namespace (installing cljs.pprint from cache)")
  (let [cache-prep (str (fs/path here "out/.bench-cache-leaf-prep"))
        cache-adv (str (fs/path here "out/.bench-cache-leaf-adv"))
        files (mapv #(str (fs/path here (:file %))) twin-programs)]
    (fs/delete-tree cache-prep)
    (fs/delete-tree cache-adv)
    ;; Warm caches before edits
    (doseq [{:keys [id ns]} twin-programs]
      (sh! {} bin "--compile" "--optimize" "prepare" "--entry" ns "--source-root" "src"
           "--cache-dir" cache-prep "--out-dir" (str (fs/path here (str "out/bench-leaf-prep-" id))))
      (sh! {} bin "--compile" "--optimize" "advanced" "--entry" ns "--source-root" "src"
           "--cache-dir" cache-adv "--out-dir" (str (fs/path here (str "out/bench-leaf-adv-" id))))
      (sh! {} "clojure" "-M:shadow" "compile" id))
    (with-all-restored files
      (fn [originals]
        (let [acc (atom {})
              last-stats (atom {})]
          (doseq [r (range runs)
                  :let [progs (if (even? r) twin-programs (vec (rseq twin-programs)))]]
            ;; Edit both calling namespaces for this round
            (doseq [f files]
              (spit f (str (originals f) "\n(def bench-leaf-edit-" r " " r ")\n")))
            (let [modes (if (even? r)
                          [:light-prepare :light-advanced :shadow-compile]
                          [:shadow-compile :light-advanced :light-prepare])]
              (doseq [mode modes {:keys [id ns]} progs]
                (let [cmd (case mode
                            :light-prepare [bin "--compile" "--optimize" "prepare" "--entry" ns
                                            "--source-root" "src" "--cache-dir" cache-prep "--stats"
                                            "--out-dir" (str (fs/path here (str "out/bench-leaf-prep-" id)))]
                            :light-advanced [bin "--compile" "--optimize" "advanced" "--entry" ns
                                             "--source-root" "src" "--cache-dir" cache-adv "--stats"
                                             "--out-dir" (str (fs/path here (str "out/bench-leaf-adv-" id)))]
                            :shadow-compile ["clojure" "-M:shadow" "compile" id])
                      {:keys [sample stats-raw]} (timed-run here cmd)]
                  (when (and (#{:light-prepare :light-advanced} mode) (= id "pprint-only"))
                    (let [hits (get-in sample [:stats "cache.hits"] 0)
                          misses (get-in sample [:stats "cache.misses"] 0)]
                      (when-not (and (>= hits 2) (= misses 1))
                        (throw (ex-info (str "Expected cljs.pprint cache hit and 1 miss, got hits="
                                             hits " misses=" misses "\n" stats-raw) {})))))
                  (when stats-raw (swap! last-stats assoc [mode id] stats-raw))
                  (log (format "  leaf-cache r=%d %s %s wall=%.3fs peak=%.1fMB load=%.2f"
                               r (name mode) id (:wall-s sample) (:peak-memory-mb sample) (first (:load sample))))
                  (swap! acc update [mode id] (fnil conj []) sample)))))
          {:results (into {} (for [[k v] @acc] [k (summarize v)]))
           :stats @last-stats})))))

(defn run-uncached-prepare [bin runs]
  (log "== Scenario: uncached --compile --optimize prepare (--no-cache --stats)")
  (let [progs (conj (mapv #(assoc % :root "src") twin-programs)
                    (assoc (first test-programs) :root "test"))
        acc (atom {})
        last-stats (atom {})]
    (doseq [r (range runs)
            :let [order (if (even? r) progs (vec (rseq progs)))]]
      (doseq [{:keys [id ns root]} order]
        (let [out-dir (str (fs/path here (str "out/bench-prep-" id)))
              cmd [bin "--compile" "--optimize" "prepare" "--entry" ns
                   "--source-root" root "--no-cache" "--stats" "--out-dir" out-dir]
              {:keys [sample stats-raw]} (timed-run here cmd)]
          (swap! last-stats assoc id stats-raw)
          (log (format "  uncached-prepare r=%d %s wall=%.3fs peak=%.1fMB load=%.2f"
                       r id (:wall-s sample) (:peak-memory-mb sample) (first (:load sample))))
          (swap! acc update id (fnil conj []) sample))))
    {:results (into {} (for [[k v] @acc] [k (summarize v)]))
     :stats @last-stats}))

(defn run-uncached-release [bin runs]
  (log "== Scenario: uncached advanced/release (light-cljs --optimize advanced --no-cache vs shadow-cljs release)")
  (let [acc (atom {})
        last-stats (atom {})]
    (doseq [r (range runs)
            :let [tools (if (even? r) [:light :shadow] [:shadow :light])
                  progs (if (even? r) twin-programs (vec (rseq twin-programs)))]]
      (doseq [tool tools {:keys [id ns]} progs]
        (when (= tool :shadow)
          (fs/delete-tree (fs/path here ".shadow-cljs"))
          (fs/delete-if-exists (fs/path here (str "out/shadow-" id ".js"))))
        (let [out-dir (str (fs/path here (str "out/bench-adv-" id)))
              _ (when (= tool :light) (fs/delete-tree out-dir))
              cmd (case tool
                    :light [bin "--compile" "--optimize" "advanced" "--entry" ns
                            "--source-root" "src" "--no-cache" "--stats" "--out-dir" out-dir]
                    :shadow ["clojure" "-M:shadow" "release" id])
              {:keys [sample stats-raw]} (timed-run here cmd)]
          (when (= tool :light)
            (write-light-adv-runner! out-dir ns)
            (swap! last-stats assoc id stats-raw))
          (log (format "  uncached-release r=%d %s %s wall=%.3fs peak=%.1fMB load=%.2f"
                       r (name tool) id (:wall-s sample) (:peak-memory-mb sample) (first (:load sample))))
          (swap! acc update [tool id] (fnil conj []) sample))))
    ;; Also measure cljs.test uncached advanced once per round for line-rate reference
    (let [{:keys [id ns]} (first test-programs)]
      (doseq [r (range runs)]
        (let [out-dir (str (fs/path here (str "out/bench-adv-" id)))
              _ (fs/delete-tree out-dir)
              cmd [bin "--compile" "--optimize" "advanced" "--entry" ns
                   "--source-root" "test" "--no-cache" "--stats" "--out-dir" out-dir]
              {:keys [sample stats-raw]} (timed-run here cmd)]
          (swap! last-stats assoc id stats-raw)
          (swap! acc update [:light id] (fnil conj []) sample))))
    {:results (into {} (for [[k v] @acc] [k (summarize v)]))
     :stats @last-stats}))

(defn run-release-load [runs]
  (log "== Scenario: advanced/release build load in Node (first output)")
  (let [acc (atom {})
        outputs (atom {})]
    (doseq [r (range runs)
            :let [tools (if (even? r) [:light :shadow] [:shadow :light])
                  progs (if (even? r) twin-programs (vec (rseq twin-programs)))]]
      (doseq [tool tools {:keys [id]} progs]
        (let [cmd (case tool
                    :light ["node" (str (fs/path here (str "out/bench-adv-" id "/run.mjs")))]
                    :shadow ["node" (str (fs/path here (str "out/shadow-" id ".js")))])
              {:keys [sample out]} (timed-streaming here cmd)]
          (if-let [prev (get @outputs id)]
            (when-not (= prev out)
              (throw (ex-info (str "Release stdout mismatch for " id "\nexpected:\n" prev "\ngot:\n" out) {})))
            (swap! outputs assoc id out))
          (swap! acc update [tool id] (fnil conj []) sample)
          (log (format "  release-load r=%d %s %s first=%.1fms wall=%.1fms peak=%.1fMB load=%.2f"
                       r (name tool) id (* 1000.0 (:first-output-s sample)) (* 1000.0 (:wall-s sample))
                       (:peak-memory-mb sample) (first (:load sample)))))))
    (into {} (for [[k v] @acc] [k (summarize v)]))))

(defn process-rss-mb [pid]
  (let [{:keys [exit out]} (process/sh "ps" "-o" "rss=" "-p" (str pid))]
    (when (zero? exit)
      (some-> out str/trim parse-long (/ 1000.0)))))

(defn run-watch-for [bin {:keys [id ns file]} runs]
  (let [src-file (str (fs/path here file))
        out-dir (str (fs/path here (str "out/bench-watch-" id)))
        cache-dir (str (fs/path here (str "out/.bench-watch-cache-" id)))]
    (fs/delete-tree out-dir)
    (fs/delete-tree cache-dir)
    (with-restored src-file
      (fn [original]
        (let [lines (atom [])
              proc (process/process {:dir (str here) :out :stream :err :stream}
                                    bin "--watch" "--entry" ns "--source-root" "src"
                                    "--out-dir" out-dir "--cache-dir" cache-dir
                                    "--poll-ms" "50")
              pid (.pid ^Process (:proc proc))
              peak-mb (atom 0.0)
              running (atom true)
              pump (fn [stream]
                     (future
                       (with-open [r (java.io.BufferedReader. (java.io.InputStreamReader. stream))]
                         (doseq [l (line-seq r)]
                           (swap! lines conj [(System/nanoTime) l])))))
              _ (pump (:out proc))
              _ (pump (:err proc))
              _ (future
                  (while @running
                    (when-let [mb (process-rss-mb pid)]
                      (swap! peak-mb max mb))
                    (Thread/sleep 100)))
              wait-for (fn [pred start-idx timeout-ms]
                         (let [deadline (+ (System/currentTimeMillis) timeout-ms)]
                           (loop []
                             (let [ls @lines
                                   hit (some (fn [[idx [t l]]]
                                               (when-let [m (pred l)] [idx t m l]))
                                             (drop start-idx (map-indexed vector ls)))]
                               (if hit
                                 hit
                                 (do
                                   (when (or (> (System/currentTimeMillis) deadline)
                                             (not (process/alive? proc)))
                                     (throw (ex-info (str "watch timeout; lines:\n"
                                                          (str/join "\n" (map second (take-last 30 @lines)))) {})))
                                   (Thread/sleep 10)
                                   (recur)))))))]
          (try
            (let [[ready-idx _ _ _] (wait-for #(re-find #"watching every" %) 0 60000)
                  samples (atom [])]
              (loop [k 0 idx (inc ready-idx)]
                (when (<= k runs)
                  (Thread/sleep 200)
                  (let [load (load-average)
                        t0 (System/nanoTime)
                        _ (spit src-file (str original "\n(def watch-bench-" k " " k ")\n"))
                        [hit-idx t1 m line] (wait-for #(re-find #"recompiled (.*) in ([\d.]+) ms" %) idx 30000)
                        [_ nss ms-str] m
                        step-ms (parse-double ms-str)
                        e2e-ms (/ (- t1 t0) 1e6)]
                    (when-not (= (str/trim nss) ns)
                      (throw (ex-info (str "Expected only " ns " recompiled, got: " line) {})))
                    (when (pos? k) ; k=0 is warm-up
                      (swap! samples conj {:step-ms step-ms
                                           :e2e-ms e2e-ms
                                           :wall-s (/ step-ms 1000.0)
                                           :peak-memory-mb @peak-mb
                                           :load load}))
                    (recur (inc k) (inc hit-idx)))))
              (assoc (summarize @samples) :watch-peak-memory-mb @peak-mb))
            (finally
              (reset! running false)
              (process/sh "kill" "-TERM" (str pid))
              (Thread/sleep 300)
              (when (process/alive? proc)
                (process/sh "kill" "-KILL" (str pid))))))))))

(defn run-watch [bin runs]
  (log "== Scenario: light-cljs --watch step after editing calling namespace")
  (into {}
        (for [prog twin-programs]
          (let [res (run-watch-for bin prog runs)]
            (log (format "  watch %s step=%s e2e=%s peak=%.1fMB"
                         (:id prog) (fmt-raw-ms (:step-ms res)) (fmt-raw-ms (:e2e-ms res)) (:watch-peak-memory-mb res)))
            [(:id prog) res]))))

(defn run-test-cmd [bin runs]
  (log "== Scenario: light-cljs test (uncached and cached)")
  (let [cache-dir (str (fs/path here "out/.bench-test-cache"))
        acc (atom {})]
    ;; Uncached
    (doseq [r (range runs)
            :let [progs (if (even? r) test-programs (vec (rseq test-programs)))]]
      (doseq [{:keys [id ns]} progs]
        (let [cmd [bin "test" ns "--source-root" "test" "--no-cache" "--stats"]
              {:keys [sample]} (timed-run here cmd)]
          (log (format "  test-uncached r=%d %s wall=%.3fs peak=%.1fMB load=%.2f"
                       r id (:wall-s sample) (:peak-memory-mb sample) (first (:load sample))))
          (swap! acc update [:uncached id] (fnil conj []) sample))))
    ;; Cached (warm once)
    (fs/delete-tree cache-dir)
    (doseq [{:keys [ns]} test-programs]
      (sh! {} bin "test" ns "--source-root" "test" "--cache-dir" cache-dir))
    (doseq [r (range runs)
            :let [progs (if (even? r) test-programs (vec (rseq test-programs)))]]
      (doseq [{:keys [id ns]} progs]
        (let [cmd [bin "test" ns "--source-root" "test" "--cache-dir" cache-dir "--stats"]
              {:keys [sample]} (timed-run here cmd)]
          (log (format "  test-cached r=%d %s wall=%.3fs peak=%.1fMB load=%.2f"
                       r id (:wall-s sample) (:peak-memory-mb sample) (first (:load sample))))
          (swap! acc update [:cached id] (fnil conj []) sample))))
    (into {} (for [[k v] @acc] [k (summarize v)]))))

(defn print-summary [{:keys [environment runs dev-compile dev-load leaf-cache
                             uncached-prepare uncached-release release-load watch test]}]
  (println "\n=== Environment ===")
  (pp/pprint environment)
  (when uncached-prepare
    (println "\n### Uncached --compile --optimize prepare (--no-cache --stats)")
    (println "| Program | Wall time | CPU (user+sys) | Peak memory | Load 1m |")
    (println "|---|---:|---:|---:|---:|")
    (doseq [id ["prn" "pprint-only" "prn-test"]
            :let [s (get-in uncached-prepare [:results id])] :when s]
      (println (format "| `%s` | %s | %s | %s | %s |"
                       id (fmt-ms (:wall-s s)) (fmt-ms (:cpu-s s))
                       (fmt-mb (:peak-memory-mb s)) (fmt-load (:load-1m s))))))
  (when uncached-release
    (println "\n### Uncached advanced / release (--no-cache vs shadow-cljs clean release)")
    (println "| Program | Compiler | Wall time | CPU (user+sys) | Peak memory | Load 1m |")
    (println "|---|---|---:|---:|---:|---:|")
    (doseq [id ["prn" "pprint-only" "prn-test"]
            tool [:light :shadow]
            :let [s (get-in uncached-release [:results [tool id]])] :when s]
      (println (format "| `%s` | %s | %s | %s | %s | %s |"
                       id (if (= tool :light) "light-cljs" "shadow-cljs")
                       (fmt-s (:wall-s s)) (fmt-s (:cpu-s s))
                       (fmt-mb (:peak-memory-mb s)) (fmt-load (:load-1m s))))))
  (when dev-compile
    (println "\n### Warm dev compile (nothing changed: light-cljs --reloadable vs shadow-cljs compile)")
    (println "| Program | Compiler | Wall time | CPU (user+sys) | Peak memory | Load 1m |")
    (println "|---|---|---:|---:|---:|---:|")
    (doseq [id ["prn" "pprint-only"]
            tool [:light :shadow]
            :let [s (get dev-compile [tool id])] :when s]
      (println (format "| `%s` | %s | %s | %s | %s | %s |"
                       id (if (= tool :light) "light-cljs" "shadow-cljs")
                       (fmt-s (:wall-s s)) (fmt-s (:cpu-s s))
                       (fmt-mb (:peak-memory-mb s)) (fmt-load (:load-1m s))))))
  (when leaf-cache
    (println "\n### Cached rebuild after one-file edit (installing cljs.pprint from persistent cache)")
    (println "| Program | Mode | Wall time | CPU (user+sys) | Peak memory | Load 1m |")
    (println "|---|---|---:|---:|---:|---:|")
    (doseq [id ["prn" "pprint-only"]
            [mode label] [[:light-prepare "light-cljs prepare"]
                          [:light-advanced "light-cljs advanced"]
                          [:shadow-compile "shadow-cljs compile"]]
            :let [s (get-in leaf-cache [:results [mode id]])] :when s]
      (println (format "| `%s` | %s | %s | %s | %s | %s |"
                       id label (fmt-s (:wall-s s)) (fmt-s (:cpu-s s))
                       (fmt-mb (:peak-memory-mb s)) (fmt-load (:load-1m s))))))
  (when watch
    (println "\n### light-cljs --watch step after editing calling namespace (library not recompiled)")
    (println "| Program | Compiler step | Edit-to-log | Watch peak memory | Load 1m |")
    (println "|---|---:|---:|---:|---:|")
    (doseq [id ["prn" "pprint-only"]
            :let [s (get watch id)] :when s]
      (println (format "| `%s` | %s | %s | %.1f MB | %s |"
                       id (fmt-raw-ms (:step-ms s)) (fmt-raw-ms (:e2e-ms s))
                       (:watch-peak-memory-mb s) (fmt-load (:load-1m s))))))
  (when test
    (println "\n### light-cljs test (one-test suite)")
    (println "| Mode | Program | Wall time | CPU (user+sys) | Peak memory | Load 1m |")
    (println "|---|---|---:|---:|---:|---:|")
    (doseq [mode [:uncached :cached]
            id ["prn-test" "pprint-test"]
            :let [s (get test [mode id])] :when s]
      (println (format "| %s | `%s` | %s | %s | %s | %s |"
                       (name mode) id (fmt-ms (:wall-s s)) (fmt-ms (:cpu-s s))
                       (fmt-mb (:peak-memory-mb s)) (fmt-load (:load-1m s))))))
  (when dev-load
    (println "\n### Node load time: dev build (time to first output)")
    (println "| Program | Compiler | First output | Wall time | CPU (user+sys) | Peak memory | Load 1m |")
    (println "|---|---|---:|---:|---:|---:|---:|")
    (doseq [id ["prn" "pprint-only"]
            tool [:light :shadow]
            :let [s (get dev-load [tool id])] :when s]
      (println (format "| `%s` | %s | %s | %s | %s | %s | %s |"
                       id (if (= tool :light) "light-cljs" "shadow-cljs")
                       (fmt-ms (:first-output-s s)) (fmt-ms (:wall-s s))
                       (fmt-ms (:cpu-s s)) (fmt-mb (:peak-memory-mb s)) (fmt-load (:load-1m s))))))
  (when release-load
    (println "\n### Node load time: advanced / release build (time to first output)")
    (println "| Program | Compiler | First output | Wall time | CPU (user+sys) | Peak memory | Load 1m |")
    (println "|---|---|---:|---:|---:|---:|---:|")
    (doseq [id ["prn" "pprint-only"]
            tool [:light :shadow]
            :let [s (get release-load [tool id])] :when s]
      (println (format "| `%s` | %s | %s | %s | %s | %s | %s |"
                       id (if (= tool :light) "light-cljs" "shadow-cljs")
                       (fmt-ms (:first-output-s s)) (fmt-ms (:wall-s s))
                       (fmt-ms (:cpu-s s)) (fmt-mb (:peak-memory-mb s)) (fmt-load (:load-1m s)))))))

(defn -main [& args]
  (let [opts (parse-args args)
        bin (light-binary opts)
        runs (:runs opts)
        only (:only opts)
        env (environment bin opts)
        results (cond-> {:environment env :runs runs}
                  (only "dev-compile")      (assoc :dev-compile (run-dev-compile bin runs))
                  (only "dev-load")         (assoc :dev-load (run-dev-load runs))
                  (only "leaf-cache")       (assoc :leaf-cache (run-leaf-cache bin runs))
                  (only "uncached-prepare") (assoc :uncached-prepare (run-uncached-prepare bin runs))
                  (only "uncached-release") (assoc :uncached-release (run-uncached-release bin runs))
                  (only "release-load")     (assoc :release-load (run-release-load runs))
                  (only "watch")            (assoc :watch (run-watch bin runs))
                  (only "test")             (assoc :test (run-test-cmd bin runs)))]
    (when-let [out (:out opts)]
      (spit (str out ".edn") (pr-str results)))
    (print-summary results)
    (when-let [st (get-in results [:uncached-prepare :stats "pprint-only"])]
      (println "\n=== light-cljs --stats (uncached prepare, pprint-only) ===")
      (println st))
    (when-let [st (get-in results [:uncached-release :stats "pprint-only"])]
      (println "\n=== light-cljs --stats (uncached advanced, pprint-only) ===")
      (println st))
    (when-let [st (get-in results [:uncached-prepare :stats "prn-test"])]
      (println "\n=== light-cljs --stats (uncached prepare, prn-test / cljs.test) ===")
      (println st))))

(apply -main *command-line-args*)
