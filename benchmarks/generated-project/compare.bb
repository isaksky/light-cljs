#!/usr/bin/env bb
;; The light-cljs vs shadow-cljs head-to-head: release builds of one project with
;; both compilers, alternated, for N rounds per scenario, each build gated on the
;; load average, reported as medians and ranges of wall time, user and system CPU
;; and peak memory. See benchmarks/README.md for the commands.
;;
;; bb benchmarks/generated-project/compare.bb PROJECT-DIR --light BINARY [options]
;;
;; PROJECT-DIR holds light-cljs.edn; shadow-cljs.edn is there too unless
;; --shadow-dir names its directory. A generated project (with oracle.bb and
;; model.edn) also has its outputs checked against the oracle and the waste
;; checker; any other project gets the size of what each build wrote.
;; Scenarios: a clean build (light --no-cache, shadow with .shadow-cljs removed),
;; a rebuild with nothing changed (one untimed build fills each cache), and with
;; --leaf FILE a rebuild after a one-file edit: on the caches the previous
;; scenario left, each round appends a different defn to FILE (restored
;; afterwards). Writes OUT.edn and OUT.md (OUT defaults to
;; PROJECT-DIR/compare-result).
(ns generated-project.compare
  (:require [babashka.fs :as fs]
            [babashka.process :as process]
            [clojure.edn :as edn]
            [clojure.pprint :as pprint]
            [clojure.string :as str]))

(def usage
  (str/join
   "\n"
   ["Usage: bb compare.bb PROJECT-DIR --light BINARY [options]"
    "  --runs N            rounds per scenario (5)"
    "  --max-load L        wait before a build until the 1-minute load average is at most L (2);"
    "                      a round with a build that started above it is repeated"
    "  --load-wait S       give up waiting after S seconds and run anyway (900)"
    "  --max-repeats N     repeat a round at most N times, then keep it marked (3)"
    "  --leaf FILE         add the one-file edit scenario, FILE relative to PROJECT-DIR"
    "  --shadow-leaf FILE  the file to edit for shadow, relative to its directory (FILE)"
    "  --warm-jvm N        also run N shadow release builds in one JVM, as a running server does"
    "  --shadow-dir DIR    directory of shadow-cljs.edn (PROJECT-DIR)"
    "  --light-build ID    build of light-cljs.edn (its only build)"
    "  --shadow-build ID   build of shadow-cljs.edn (its only build)"
    "  --shadow-cmd CMD    how to run shadow-cljs (\"clojure -M:shadow\" with a :shadow"
    "                      alias in deps.edn, otherwise \"npx shadow-cljs\")"
    "  --light-commit SHA  the commit the binary was built from (from a light-cljs-SHA name)"
    "  --light-only        build with light-cljs alone"
    "  --shadow-heaps LIST sweep shadow's maximum heap: clean builds only, one shadow build per"
    "                      JVM size in LIST (-Xmx syntax, e.g. 1g,1536m,8g) instead of one; a"
    "                      failed build (OutOfMemoryError) is recorded, not fatal; needs clojure"
    "  --light-memory-limits LIST  with the sweep, also light builds at each --macro-memory-limit"
    "                      in LIST (bytes), besides the one light-cljs.edn configures"
    "  --out PREFIX        write PREFIX.edn and PREFIX.md (PROJECT-DIR/compare-result)"]))

(def defaults {:runs 5 :max-load 2.0 :load-wait 900 :max-repeats 3 :warm-jvm 0})

(defn- number-option [parse pred what]
  (fn [text]
    (let [n (some-> text parse)]
      (if (and n (pred n)) n (throw (ex-info (str "Expected " what ", got " (pr-str text)) {}))))))

(def ^:private absolute #(str (fs/absolutize %)))

(def value-options
  {"--light" [:light absolute]
   "--light-commit" [:light-commit identity]
   "--shadow-dir" [:shadow-dir absolute]
   "--light-build" [:light-build identity]
   "--shadow-build" [:shadow-build identity]
   "--shadow-cmd" [:shadow-cmd #(str/split (str/trim %) #"\s+")]
   "--runs" [:runs (number-option parse-long pos? "a positive integer")]
   "--max-load" [:max-load (number-option parse-double pos? "a positive number")]
   "--load-wait" [:load-wait (number-option parse-long (complement neg?) "a non-negative integer")]
   "--max-repeats" [:max-repeats (number-option parse-long (complement neg?) "a non-negative integer")]
   "--warm-jvm" [:warm-jvm (number-option parse-long (complement neg?) "a non-negative integer")]
   "--leaf" [:leaf identity]
   "--shadow-leaf" [:shadow-leaf identity]
   "--out" [:out absolute]
   "--shadow-heaps" [:shadow-heaps (number-option #(let [v (str/split % #"," -1)] (when (every? (partial re-matches #"[0-9]+[kKmMgG]") v) v))
                                                  seq "comma-separated JVM sizes such as 1g,1536m")]
   "--light-memory-limits" [:light-memory-limits (number-option #(let [v (map parse-long (str/split % #"," -1))] (when (every? some? v) (vec v)))
                                                                (partial every? pos?) "comma-separated byte counts")]})

(defn parse-options [args]
  (loop [[arg & more] args options defaults]
    (cond
      (nil? arg) (if (and (:dir options) (:light options)) options (throw (ex-info usage {})))
      (= "--light-only" arg) (recur more (assoc options :light-only true))
      (value-options arg) (let [[k parse] (value-options arg)]
                            (when (nil? (first more)) (throw (ex-info (str arg " needs a value\n" usage) {})))
                            (recur (rest more) (assoc options k (parse (first more)))))
      (or (str/starts-with? arg "-") (:dir options)) (throw (ex-info usage {:argument arg}))
      :else (recur more (assoc options :dir (absolute arg))))))

(defn run! [dir & cmd]
  (let [result (apply process/sh {:dir (str dir)} cmd)]
    (when-not (zero? (:exit result))
      (throw (ex-info "Command failed" {:command cmd :err (:err result)})))
    (:out result)))

(def mac? (str/includes? (System/getProperty "os.name") "Mac"))

(defn log [& parts] (binding [*out* *err*] (apply println parts) (flush)))

;; Measuring one build

(defn parse-load
  "The three load averages of `sysctl -n vm.loadavg` or /proc/loadavg text."
  [text]
  (mapv parse-double (take 3 (re-seq #"[0-9]+(?:\.[0-9]+)?" text))))

(defn load-average
  "The 1-, 5- and 15-minute load averages."
  []
  (parse-load (if mac? (:out (process/sh "sysctl" "-n" "vm.loadavg")) (slurp "/proc/loadavg"))))

(defn await-load
  "Waits, polling every 5 s, until the 1-minute load average is at most
  :max-load or :load-wait seconds have passed, and returns the load averages
  when it stops waiting."
  [{:keys [max-load load-wait]}]
  (let [deadline (+ (System/currentTimeMillis) (* 1000 load-wait))]
    (loop [announced false]
      (let [load (load-average)]
        (if (or (<= (first load) max-load) (>= (System/currentTimeMillis) deadline))
          load
          (do (when-not announced (log "waiting for the 1-minute load average" (first load) "to reach" max-load))
              (Thread/sleep 5000)
              (recur true)))))))

(defn parse-time
  "Wall time, user and system CPU seconds and peak memory in GB from the stderr
  of macOS `/usr/bin/time -l` (mac? true) or GNU `time -v`."
  [mac? err]
  (let [number (fn [re] (some-> (re-find re err) second parse-double))]
    (if mac?
       {:wall-s (number #"([0-9.]+) real") :user-s (number #"([0-9.]+) user") :system-s (number #"([0-9.]+) sys")
        :peak-memory-gb (/ (number #"([0-9]+)  maximum resident set size") 1e9)}
       (let [[_ h m s] (re-find #"Elapsed \(wall clock\) time.*: (?:([0-9]+):)?([0-9]+):([0-9.]+)" err)]
         {:wall-s (+ (* 3600 (or (some-> h parse-long) 0)) (* 60 (parse-long m)) (parse-double s))
          :user-s (number #"User time \(seconds\): ([0-9.]+)")
          :system-s (number #"System time \(seconds\): ([0-9.]+)")
          :peak-memory-gb (/ (number #"Maximum resident set size \(kbytes\): ([0-9]+)") 1e6)}))))

(defn timed-with-output
  "Runs cmd under /usr/bin/time (-l on macOS, GNU -v elsewhere) and returns
  [sample stdout], the sample as parse-time gives it."
  [dir cmd]
  (let [result (apply process/sh {:dir (str dir)} "/usr/bin/time" (if mac? "-l" "-v") cmd)
        err (:err result)]
    (when-not (zero? (:exit result))
      (throw (ex-info "Build failed" {:command cmd :err (subs err (max 0 (- (count err) 3000)))})))
    [(parse-time mac? err) (:out result)]))

(defn timed [dir cmd] (first (timed-with-output dir cmd)))

(defn timed-or-failed
  "timed, but a build that exits non-zero gives its sample marked :failed, with
  :oom when its output names an OutOfMemoryError, instead of throwing."
  [dir cmd]
  (let [result (apply process/sh {:dir (str dir)} "/usr/bin/time" (if mac? "-l" "-v") cmd)
        err (:err result)
        ;; the build's own messages: what precedes time's report
        own (str (:out result) (first (str/split err #"(?m)^\s*[0-9.]+ real |^\tCommand being timed:" 2)))]
    (cond-> (parse-time mac? err)
      (not (zero? (:exit result)))
      (assoc :failed true :exit (:exit result) :oom (str/includes? own "OutOfMemoryError")
             :error (subs own (max 0 (- (count own) 1500)))))))

(defn parse-gc-log
  "Total stop-the-world pause seconds, pause count and full collections from a
  JVM -Xlog:gc log (lines such as `GC(3) Pause Young (Normal) ... 12.345ms`)."
  [text]
  (let [pauses (keep #(when (str/includes? % " Pause ")
                        (some-> (re-find #"([0-9.]+)ms\s*$" %) second parse-double))
                     (str/split-lines text))]
    {:gc-pause-s (/ (reduce + 0.0 pauses) 1000) :gc-pauses (count pauses)
     :full-gcs (count (re-seq #" Pause Full " text))}))

;; Statistics

(defn median [xs]
  (let [v (vec (sort xs)) n (count v) h (quot n 2)]
    (if (odd? n) (v h) (/ (+ (v (dec h)) (v h)) 2.0))))

(defn spread [xs] {:median (median xs) :min (apply min xs) :max (apply max xs)})

(def measures [:wall-s :user-s :system-s :cpu-s :peak-memory-gb])

(def gc-measures [:gc-pause-s :gc-pauses :full-gcs])

(defn summary
  "Median and range of every measure (:cpu-s is user plus system CPU, and the
  GC measures when the samples have them) over the builds that did not fail,
  the number that failed, and the 1-minute load average before each build."
  [samples discarded]
  (let [samples (mapv #(assoc % :cpu-s (+ (or (:user-s %) 0) (or (:system-s %) 0))) samples)
        ok (remove :failed samples)]
    (into {:runs (count samples) :failures (count (filter :failed samples))
           :load-1m (spread (map (comp first :load) samples))
           :samples samples :discarded (vec discarded)}
          (for [m (concat measures (filter (fn [m] (some m samples)) gc-measures)) :when (seq ok)]
            [m (spread (map m ok))]))))

;; Rounds

(defn rounds
  "Runs (:runs options) rounds of one scenario and returns a summary per tool.
  tools is a vector of {:tool :dir :cmd :before}, in the first round's order;
  every later round reverses the previous one's, so the compilers alternate and
  neither always goes first. (prepare! n) runs before each attempt at a round
  with a number never used before (the one-file edit writes its edit there); a
  tool's :before runs before its build (the clean build cleans there); a tool
  with :may-fail records a failed build instead of throwing, and its
  (:after sample start-millis) returns entries merged into the sample. Every
  build waits for the load gate. A round in which a build started above
  :max-load is discarded and repeated, at most :max-repeats times; its last
  attempt is then kept, marked :over-max-load."
  [{:keys [runs max-load max-repeats] :as options} prepare! tools]
  (let [attempts (atom -1) kept (atom {}) discarded (atom {})
        record! (fn [store samples] (doseq [[tool sample] samples] (swap! store update tool (fnil conj []) sample)))]
    (doseq [round (range runs)
            :let [order (if (even? round) tools (vec (rseq tools)))]]
      (loop [repeat 0]
        (prepare! (swap! attempts inc))
        (let [samples (vec (for [[position {:keys [tool dir cmd before may-fail after]}] (map-indexed vector order)]
                             (do (when before (before))
                                 (let [load (await-load options)
                                       started (System/currentTimeMillis)
                                       sample (if may-fail (timed-or-failed dir cmd) (timed dir cmd))
                                       sample (cond-> (assoc sample :load load :round round :position position)
                                                after (merge (after sample started)))]
                                   (log (name tool) (str/join " " (take-last 3 cmd)) "round" round (pr-str sample))
                                   [tool sample]))))
              over? (some (fn [[_ sample]] (> (first (:load sample)) max-load)) samples)]
          (cond
            (not over?) (record! kept samples)
            (< repeat max-repeats) (do (log "repeating round" round "of a build that started above" max-load)
                                       (record! discarded samples)
                                       (recur (inc repeat)))
            :else (record! kept (for [[tool sample] samples] [tool (assoc sample :over-max-load true)]))))))
    (into {} (for [{:keys [tool]} tools] [tool (summary (@kept tool) (@discarded tool))]))))

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

(defn with-all-restored
  "with-restored for several files: (f {file original-text})."
  [files f]
  (if-let [[file & more] (seq files)]
    (with-restored file (fn [original] (with-all-restored more #(f (assoc % file original)))))
    (f {})))

(defn leaf-scenario
  "On the caches the previous scenario left: before each round a different defn
  appended to every file in `files`, then the tools' builds. Returns
  {:summaries ... :outputs (check)}, check running before the files are
  restored."
  [options files tools check]
  (with-all-restored
    (distinct (map str files))
    (fn [originals]
      (let [summaries (rounds options
                              (fn [n] (doseq [[file original] originals]
                                        (spit file (str original "\n(defn compare-leaf-edit-" n " [] " n ")\n"))))
                              tools)]
        {:summaries summaries :outputs (check)}))))

;; Projects

(defn read-edn [file] (edn/read-string {:default (fn [_tag value] value)} (slurp (str file))))

(defn build-id
  "The build named by `requested` (with or without its colon), or the file's
  only build."
  [file requested]
  (let [ids (keys (:builds (read-edn file)))]
    (cond
      requested (let [id (keyword (str/replace requested #"^:" ""))]
                  (if (some #{id} ids) id (throw (ex-info (str "No build " id " in " file) {:builds ids}))))
      (= 1 (count ids)) (first ids)
      :else (throw (ex-info (str file " has several builds; name one") {:builds ids})))))

(defn find-version
  "The :mvn/version of thheller/shadow-cljs anywhere in an EDN value."
  [data]
  (some #(when (map? %) (some-> (get % 'thheller/shadow-cljs) :mvn/version))
        (tree-seq coll? seq data)))

(def version-form
  "Prints the version of the shadow-cljs JAR the JVM loaded."
  (str "(println \"SHADOW-VERSION\" (some->> (clojure.java.io/resource \"META-INF/maven/thheller/shadow-cljs/pom.properties\")"
       " slurp (re-find #\"version=(\\S+)\") second))"))

(defn shadow-version
  "The shadow-cljs version the selected command runs, asked of its JVM, which
  also fetches its dependencies outside the timings."
  [shadow-dir shadow-cmd]
  (let [out (apply run! shadow-dir (concat shadow-cmd ["clj-eval" version-form]))]
    (or (second (re-find #"SHADOW-VERSION (\S+)" out)) "unknown")))

(defn default-shadow-cmd
  "clojure -M:shadow when deps.edn's :shadow alias runs shadow-cljs's command
  line, otherwise npx shadow-cljs."
  [shadow-dir]
  (let [deps (fs/file shadow-dir "deps.edn")
        main-opts (when (fs/exists? deps) (get-in (read-edn deps) [:aliases :shadow :main-opts]))]
    (if (some #{"shadow.cljs.devtools.cli"} main-opts)
      ["clojure" "-M:shadow"]
      ["npx" "shadow-cljs"])))

(defn release-build
  "A build's map with its :release map merged over it, as a release build sees it."
  [file id]
  (let [build (get-in (read-edn file) [:builds id])]
    (merge (dissoc build :dev :release) (:release build))))

(defn gzip-bytes [file]
  (count (:out (process/sh {:in (fs/file file) :out :bytes} "gzip" "-9" "-c"))))

(defn written-js
  "Bytes and gzip -9 bytes of the .js files under `path` (a file or a
  directory) modified at or after `since` (epoch milliseconds)."
  [path since]
  (let [files (if (fs/directory? path) (fs/glob path "**.js") (when (fs/exists? path) [path]))
        files (sort (filter #(>= (.toMillis (fs/last-modified-time %)) since) files))]
    {:path (str path) :files (count files)
     :bytes (reduce + 0 (map fs/size files)) :gzip (reduce + 0 (map gzip-bytes files))}))

(defn verify-generated [dir expected runner output]
  (let [got (str/trim (run! dir "node" runner "42" "3"))
        waste (process/sh {:dir (str dir)} "bb" "check-waste.bb" "--strict" output)]
    (when-not (= expected got)
      (throw (ex-info "Checksum differs from the oracle" {:runner runner :expected expected :got got})))
    {:path output :bytes (fs/size (fs/file dir output)) :gzip (gzip-bytes (fs/file dir output))
     :checksum got :waste-ok (zero? (:exit waste)) :waste (str/trim (:out waste))}))

(defn project
  "Each compiler's directory, build, commands, cleaning and output check."
  [{:keys [dir light light-only] :as options}]
  (let [shadow-dir (or (:shadow-dir options) dir)
        light-build (build-id (fs/file dir "light-cljs.edn") (:light-build options))
        generated? (and (fs/exists? (fs/file dir "oracle.bb")) (fs/exists? (fs/file dir "model.edn")))
        expected (when generated? (str/trim (run! dir "bb" "oracle.bb" "42" "3")))
        light-out (:output-dir (release-build (fs/file dir "light-cljs.edn") light-build))
        light-cmd [light "release" (name light-build)]
        tools [{:tool :light :dir dir
                :clean-cmd (conj light-cmd "--no-cache")
                :cached-cmd (conj light-cmd "--cache-dir" ".compare-cache")
                :clean! #(doseq [p (cond-> [".light-cljs" ".compare-cache"] generated? (conj "out/light/workload.main.js"))]
                           (fs/delete-tree (fs/path dir p)))
                :output (if generated?
                          (fn [_] (verify-generated dir expected "run-light.mjs" "out/light/workload.main.js"))
                          #(written-js (fs/path dir light-out) %))}]]
    (if light-only
      {:light-dir dir :generated? generated? :expected expected :light-build light-build :tools tools}
      (let [shadow-build (build-id (fs/file shadow-dir "shadow-cljs.edn") (:shadow-build options))
            build (release-build (fs/file shadow-dir "shadow-cljs.edn") shadow-build)
            cache-root (or (:cache-root (read-edn (fs/file shadow-dir "shadow-cljs.edn"))) ".shadow-cljs")
            shadow-cmd (vec (or (:shadow-cmd options) (default-shadow-cmd shadow-dir)))
            cmd (into shadow-cmd ["release" (name shadow-build)])]
        {:light-dir dir :shadow-dir shadow-dir :generated? generated? :expected expected :shadow-cache-root cache-root
         :light-build light-build :shadow-build shadow-build :shadow-cmd shadow-cmd
         :tools (conj tools
                      {:tool :shadow :dir shadow-dir :clean-cmd cmd :cached-cmd cmd
                       :clean! #(do (fs/delete-tree (fs/path shadow-dir cache-root))
                                    (when generated? (fs/delete-if-exists (fs/path shadow-dir "out/shadow.js"))))
                       :output (if generated?
                                 (fn [_] (verify-generated shadow-dir expected "out/shadow.js" "out/shadow.js"))
                                 #(written-js (fs/path shadow-dir (or (:output-to build) (:output-dir build))) %))})}))))

;; Memory-limit sweep

(defn jvm-size-bytes
  "Bytes of a -Xmx size such as 1536m or 8g."
  [size]
  (let [[_ n unit] (re-matches #"([0-9]+)([kKmMgG])" size)]
    (* (parse-long n) ({"k" 1024 "m" (* 1024 1024) "g" (* 1024 1024 1024)} (str/lower-case unit)))))

(defn- gib-text [bytes]
  (let [g (/ (double bytes) (* 1024 1024 1024))]
    (if (>= g 1) (str (str/replace (format "%.2f" g) #"\.?0+$" "") " GiB") (str (quot bytes (* 1024 1024)) " MiB"))))

(defn sweep-tools
  "The tools of a memory-limit sweep, clean builds only, each with :label,
  :limit (its limit as text) and :may-fail: the light build as light-cljs.edn
  configures it, one per --light-memory-limits value (its --macro-memory-limit)
  and, replacing the one shadow build, one per --shadow-heaps size, whose JVM
  gets -Xmx (after the :shadow alias's own, so it wins), exits on an
  OutOfMemoryError and logs its collections to a file in gc-dir. Every build
  that succeeds has its output checked as the clean build's is; a shadow
  build's sample also gets its garbage-collection pauses."
  [{:keys [dir shadow-heaps light-memory-limits]} {:keys [tools light-build shadow-cmd]} gc-dir]
  (let [{:keys [light shadow]} (into {} (map (juxt :tool identity) tools))
        configured (get-in (release-build (fs/file dir "light-cljs.edn") light-build) [:compiler-options :macro-memory-limit])
        checked (fn [{:keys [output]}] (fn [sample started] (when-not (:failed sample) {:output (output started)})))
        light-tool (fn [tool limit cmd] (assoc light :tool tool :label "light-cljs" :limit limit :clean-cmd cmd
                                              :may-fail true :after (checked light)))]
    (when (and shadow-heaps (not= "clojure" (first shadow-cmd)))
      (throw (ex-info "--shadow-heaps passes -J-Xmx to the Clojure CLI; the shadow command is not clojure" {:shadow-cmd shadow-cmd})))
    (vec
     (concat
      [(light-tool :light (if configured
                            (str "light-cljs.edn's :macro-memory-limit " configured " (" (gib-text configured) ")")
                            "its default :macro-memory-limit")
                   (:clean-cmd light))]
      (for [n light-memory-limits]
        (light-tool (keyword (str "light-mml" n)) (str "--macro-memory-limit " n " (" (gib-text n) ")")
                    (into (:clean-cmd light) ["--macro-memory-limit" (str n)])))
      (for [size shadow-heaps :when shadow
            :let [log (str (fs/path gc-dir (str "gc-" size ".log")))
                  [clojure & more] (:clean-cmd shadow)]]
        (assoc shadow :tool (keyword (str "shadow-xmx" size)) :label "shadow-cljs"
               :limit (str "-Xmx" size " (" (gib-text (jvm-size-bytes size)) ")") :heap-bytes (jvm-size-bytes size)
               :clean-cmd (into [clojure (str "-J-Xmx" size) "-J-XX:+ExitOnOutOfMemoryError" (str "-J-Xlog:gc:file=" log)] more)
               :before #(do ((:clean! shadow)) (fs/delete-if-exists log))
               :may-fail true
               :after (let [check (checked shadow)]
                        (fn [sample started]
                          (merge (when (fs/exists? log) (parse-gc-log (slurp log))) (check sample started))))))))))

(defn sweep-verdict
  "shadow's best median wall time over the heaps where no build failed, the
  smallest such heap within 10% of it (the smallest adequate heap), and its
  peak memory beside every light build's: {:best tool :adequate tool
  :ratios [{:light tool :adequate r :largest r}]}, ratios light/shadow peak
  memory medians, :largest at the largest heap with no failed build."
  [tools by-tool]
  (let [ok? #(let [s (by-tool (:tool %))] (and s (zero? (:failures s)) (:wall-s s)))
        wall #(get-in by-tool [(:tool %) :wall-s :median])
        peak #(get-in by-tool [(:tool %) :peak-memory-gb :median])
        heaps (sort-by :heap-bytes (filter #(and (:heap-bytes %) (ok? %)) tools))
        best (when (seq heaps) (apply min-key wall heaps))
        adequate (first (filter #(<= (wall %) (* 1.1 (wall best))) heaps))
        largest (last heaps)]
    {:best (:tool best) :adequate (:tool adequate) :largest (:tool largest)
     :ratios (vec (for [t tools :when (and (= "light-cljs" (:label t)) (ok? t) adequate)]
                    {:light (:tool t) :adequate (/ (peak t) (peak adequate)) :largest (/ (peak t) (peak largest))}))}))

;; Versions

(defn- command-text [& cmd]
  (try (let [{:keys [out err]} (apply process/sh cmd)] (str/trim (str out err)))
       (catch Exception _ nil)))

(defn- sha256-file [file]
  (let [digest (java.security.MessageDigest/getInstance "SHA-256")]
    (with-open [in (java.io.FileInputStream. (str file))]
      (let [buffer (byte-array 65536)]
        (loop [] (let [n (.read in buffer)] (when (pos? n) (.update digest buffer 0 n) (recur))))))
    (apply str (map #(format "%02x" %) (.digest digest)))))

(defn environment [{:keys [light light-commit]} shadow-version]
  (let [sysctl #(command-text "sysctl" "-n" %)]
    {:date (str (java.time.LocalDate/now))
     :light-cljs {:binary light :sha256 (sha256-file light)
                  :commit (or light-commit (second (re-find #"light-cljs-([0-9a-f]{7,40})$" (str (fs/file-name light)))))}
     :shadow-cljs shadow-version
     :jdk (let [line (first (str/split-lines (or (command-text "java" "-version") "")))]
            (if-let [[_ vendor version] (re-find #"^(\S+) version \"([^\"]+)\"" line)] (str vendor " " version) line))
     :node (command-text "node" "--version")
     :clojure-cli (command-text "clojure" "--version")
     :babashka (System/getProperty "babashka.version")
     :os (if mac? (str "macOS " (command-text "sw_vers" "-productVersion")) (command-text "uname" "-sr"))
     :machine (if mac?
                {:model (sysctl "hw.model") :cpu (sysctl "machdep.cpu.brand_string")
                 :cpus (some-> (sysctl "hw.ncpu") parse-long)
                 :memory-gib (some-> (sysctl "hw.memsize") parse-long (quot (* 1024 1024 1024)))}
                {:cpu (some->> (command-text "grep" "-m1" "model name" "/proc/cpuinfo") (re-find #":\s*(.*)") second)
                 :cpus (.availableProcessors (Runtime/getRuntime))
                 :memory-gib (some->> (command-text "grep" "MemTotal" "/proc/meminfo") (re-find #"([0-9]+)") second
                                      parse-long (#(quot % (* 1024 1024))))})}))

;; Shadow in one JVM

(defn warm-jvm
  "N shadow release builds in one JVM, as a running shadow-cljs server does,
  starting from the cache the earlier scenarios left: each build's wall time,
  the JVM's CPU time (user and system) during it and the 1-minute load average
  before it, and the whole process's measurements. The JVM applies the load
  gate before every build too, but a build that started above it is marked
  :over-max-load, not repeated: the builds are one resident sequence, and
  another build would be a later one in it."
  [{:keys [max-load load-wait] :as options} {:keys [shadow-dir shadow-cmd shadow-build]} n]
  (let [form (str "(require '[shadow.cljs.devtools.api :as api])"
                  "(let [os (java.lang.management.ManagementFactory/getOperatingSystemMXBean)]"
                  " (dotimes [_ " n "]"
                  "  (let [deadline (+ (System/currentTimeMillis) " (* 1000 load-wait) ")"
                  "        load (loop [] (let [l (.getSystemLoadAverage os)]"
                  "               (if (or (<= l " max-load ") (>= (System/currentTimeMillis) deadline)) l"
                  "                   (do (Thread/sleep 5000) (recur)))))"
                  "        t (System/nanoTime) c (.getProcessCpuTime os)]"
                  "   (api/release " shadow-build ")"
                  "   (println \"RUN\" (/ (- (System/nanoTime) t) 1e9) (/ (- (.getProcessCpuTime os) c) 1e9) load))))")
        load (await-load options)
        [sample out] (timed-with-output shadow-dir (concat shadow-cmd ["clj-eval" form]))
        builds (mapv (fn [[_ wall cpu load]]
                       (cond-> {:wall-s (parse-double wall) :cpu-s (parse-double cpu) :load-1m (parse-double load)}
                         (> (parse-double load) max-load) (assoc :over-max-load true)))
                     (re-seq #"RUN ([0-9.E-]+) ([0-9.E-]+) ([0-9.E-]+)" out))]
    (log "shadow in one JVM" (pr-str builds))
    {:builds builds :process (assoc sample :load load)}))

;; Report

(def scenario-labels
  [[:uncached "clean build"] [:cached "rebuild, nothing changed"] [:leaf "rebuild after a one-file edit"]])
(def tool-labels {:light "light-cljs" :shadow "shadow-cljs"})

(defn- spread-text [{:keys [median min max]} digits unit]
  (let [f #(format (str "%." digits "f") (double %))]
    (str (f median) " " unit " (" (f min) "-" (f max) ")")))

(defn- ratio [a b] (let [r (/ (double a) b)] (format (if (< r 0.1) "%.3f" "%.2f") r)))

(defn- environment-line [{:keys [light-cljs machine] :as environment}]
  (str "light-cljs " (if (:commit light-cljs) (str "`" (:commit light-cljs) "`") "(commit not named)")
       " (`" (fs/file-name (:binary light-cljs)) "`, SHA-256 `" (subs (:sha256 light-cljs) 0 16) "...`)"
       (when (:shadow-cljs environment) (str ", shadow-cljs " (:shadow-cljs environment)))
       ", " (:jdk environment) ", Node " (:node environment) ", " (:clojure-cli environment)
       ", Babashka " (:babashka environment) "; " (:cpu machine) (when (:model machine) (str " (" (:model machine) ")"))
       ", " (:cpus machine) " CPUs, " (:memory-gib machine) " GiB, " (:os environment) ", " (:date environment) "."))

(defn- builds-text [project]
  (str "Release builds of " (str/join " and " (for [[k file] [[:light-build "light-cljs.edn"] [:shadow-build "shadow-cljs.edn"]]
                                                  :let [id (project k)] :when id]
                                              (str file " build `" id "`")))))

(defn- output-row [label o]
  (str "| " label " | " (format "%,d" (:bytes o)) " | " (format "%,d" (:gzip o)) " | "
       (if (:checksum o)
         (str "oracle " (:checksum o) ", waste " (if (:waste-ok o) "ok" "FAILED"))
         (str (:files o) " .js file(s) written")) " |"))

(defn sweep-markdown
  "The report of a memory-limit sweep: one row per build configuration, the
  smallest adequate shadow heap and the light/shadow peak memory ratios."
  [{:keys [environment options project scenarios outputs]}]
  (let [tools (:sweep-tools project)
        by-tool (:uncached scenarios)
        {:keys [best adequate largest ratios]} (sweep-verdict tools by-tool)
        by-id (into {} (map (juxt :tool identity) tools))
        name-of #(let [t (by-id %)] (str (:label t) " at " (:limit t)))
        best-wall (get-in by-tool [best :wall-s :median])
        gb #(get-in by-tool [% :peak-memory-gb :median])]
    (str/join
     "\n"
     (concat
      [(environment-line environment)
       ""
       (str (builds-text project) ", clean builds only. Rounds: " (:runs options)
            ", the configurations alternated (each round reverses the previous round's order)"
            ". Each build waited for a 1-minute load average of at most " (:max-load options)
            ". Medians over the builds that succeeded, with ranges in brackets; GC pauses are the"
            " shadow JVM's stop-the-world collection pauses in seconds, with full collections.")
       ""
       "| Compiler | Limit | Wall time | User CPU | System CPU | Peak memory | GC pauses | Failed | Wall time / shadow's best | 1-minute load before |"
       "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |"]
      (for [{:keys [tool label limit]} tools :let [s (by-tool tool)] :when s]
        (str "| " label " | " limit " | "
             (if (:wall-s s)
               (str (spread-text (:wall-s s) 2 "s") " | " (spread-text (:user-s s) 2 "s") " | "
                    (spread-text (:system-s s) 2 "s") " | " (spread-text (:peak-memory-gb s) 3 "GB") " | "
                    (if (:gc-pause-s s)
                      (str (spread-text (:gc-pause-s s) 2 "s") ", " (long (:median (:full-gcs s))) " full")
                      "-"))
               "- | - | - | - | -")
             " | " (:failures s) " of " (:runs s)
             (let [ooms (count (filter :oom (:samples s)))] (when (pos? ooms) (str " (" ooms " OutOfMemoryError)")))
             " | " (if (and best-wall (:wall-s s)) (ratio (:median (:wall-s s)) best-wall) "-")
             " | " (format "%.2f-%.2f" (double (:min (:load-1m s))) (double (:max (:load-1m s)))) " |"))
      [""
       (if adequate
         (str "shadow-cljs's best median wall time is " (format "%.2f s" best-wall) " (" (name-of best) "); its smallest heap"
              " with no failed build and a median within 10% of that is " (:limit (by-id adequate))
              (format ", %.2f s at %.3f GB peak memory." (get-in by-tool [adequate :wall-s :median]) (gb adequate)))
         "No shadow-cljs heap completed every build.")]
      (for [{:keys [light] :as r} ratios]
        (str "light-cljs at " (:limit (by-id light)) (format ": %.3f GB peak memory, " (gb light))
             (format "%.2fx" (:adequate r)) " shadow-cljs's at its smallest adequate heap and "
             (format "%.2fx" (:largest r)) " at " (:limit (by-id largest)) "."))
      ["" "| Output | Bytes | gzip -9 bytes | Checked |" "| --- | --- | --- | --- |"]
      (for [{:keys [tool]} tools :let [o (get-in outputs [:uncached tool])] :when o]
        (output-row (name-of tool) o))
      [""]))))

(defn markdown [{:keys [environment options project scenarios outputs warm-jvm]}]
  (let [tools (filter #(get-in scenarios [:uncached %]) [:light :shadow])
        repeated (reduce + (for [[_ by-tool] scenarios [_ s] by-tool] (count (:discarded s))))
        marked (count (for [[_ by-tool] scenarios [_ s] by-tool sample (:samples s) :when (:over-max-load sample)] sample))]
    (str/join
     "\n"
     (concat
      [(environment-line environment)
       ""
       (str (builds-text project)
            ". Rounds per scenario: " (:runs options)
            (when (= 2 (count tools)) ", the compilers alternated (each round reverses the previous round's order)")
            ". Each build waited for a 1-minute load average of at most " (:max-load options)
            "; " repeated " build(s) in rounds that started above it were discarded and repeated"
            (when (pos? marked) (str ", and " marked " kept after " (:max-repeats options) " repeats are marked in the EDN"))
            ". Medians, with ranges in brackets.")
       ""
       "| Build | Compiler | Wall time | User CPU | System CPU | Peak memory | 1-minute load before |"
       "| --- | --- | --- | --- | --- | --- | --- |"]
      (for [[scenario label] scenario-labels tool tools
            :let [s (get-in scenarios [scenario tool])] :when s]
        (str "| " label " | " (tool-labels tool) " | " (spread-text (:wall-s s) 2 "s") " | "
             (spread-text (:user-s s) 2 "s") " | " (spread-text (:system-s s) 2 "s") " | "
             (spread-text (:peak-memory-gb s) 3 "GB") " | "
             (format "%.2f-%.2f" (double (:min (:load-1m s))) (double (:max (:load-1m s)))) " |"))
      (when (= 2 (count tools))
        (concat
         ["" "| Build | light/shadow wall time | light/shadow CPU (user + system) | light/shadow peak memory |"
          "| --- | --- | --- | --- |"]
         (for [[scenario label] scenario-labels
               :let [{l :light s :shadow} (scenarios scenario)] :when (and l s)]
           (str "| " label " | " (ratio (:median (:wall-s l)) (:median (:wall-s s))) " | "
                (ratio (:median (:cpu-s l)) (:median (:cpu-s s))) " | "
                (ratio (:median (:peak-memory-gb l)) (:median (:peak-memory-gb s))) " |"))))
      ["" "| Output | Compiler | Bytes | gzip -9 bytes | Checked |" "| --- | --- | --- | --- | --- |"]
      (for [[scenario label] scenario-labels tool tools
            :let [o (get-in outputs [scenario tool])] :when o]
        (output-row (str label " | " (tool-labels tool)) o))
      (when-let [{:keys [builds process]} warm-jvm]
        ["" (str "shadow-cljs in one JVM (" (count builds) " release builds, as a running server): "
                 (str/join ", " (map #(format "%.2f s (%.1f s CPU, load %.2f)" (:wall-s %) (:cpu-s %) (:load-1m %)) builds))
                 (format "; the JVM's peak memory %.3f GB" (:peak-memory-gb process)) ".")])
      [""]))))

(defn sweep
  "The memory-limit sweep's clean-build rounds over sweep-tools: {:tools ...
  :uncached summaries :outputs the last successful build's checked output per
  tool}."
  [options project]
  (let [gc-dir (fs/create-temp-dir {:prefix "compare-gc-"})]
    (try
      (let [tools (sweep-tools options project gc-dir)
            uncached (rounds options (constantly nil)
                             (mapv #(-> (select-keys % [:tool :dir :may-fail :after])
                                        (assoc :cmd (:clean-cmd %) :before (or (:before %) (:clean! %))))
                                   tools))]
        {:tools tools :uncached uncached
         :outputs (into {} (for [[tool s] uncached :let [o (some :output (rseq (:samples s)))] :when o] [tool o]))})
      (finally (fs/delete-tree gc-dir)))))

(defn main [args]
  (let [options (parse-options args)
        sweep? (or (:shadow-heaps options) (:light-memory-limits options))
        _ (when (and sweep? (or (:leaf options) (pos? (:warm-jvm options))))
            (throw (ex-info "The memory-limit sweep times clean builds only; drop --leaf and --warm-jvm" {})))
        {:keys [tools generated?] :as project} (project options)
        environment (environment options (when-let [cmd (:shadow-cmd project)] ; untimed: fetches dependencies
                                           (shadow-version (:shadow-dir project) cmd)))
        output-check (fn [since] #(into {} (for [{:keys [tool output]} tools] [tool (output since)])))
        build-tools (fn [cmd-key before?] (mapv #(cond-> {:tool (:tool %) :dir (:dir %) :cmd (cmd-key %)}
                                                   before? (assoc :before (:clean! %)))
                                                tools))
        project-entry (fn [tools] (cond-> (dissoc project :tools)
                                    true (assoc :commands (into {} (for [t tools] [(:tool t) {:clean (:clean-cmd t) :cached (:cached-cmd t)}])))
                                    (not generated?) (dissoc :expected)))
        result (if sweep?
                 (let [{:keys [tools uncached outputs]} (sweep options project)]
                   {:environment environment
                    :options (dissoc options :light)
                    :project (assoc (project-entry tools)
                                    :sweep-tools (mapv #(select-keys % [:tool :label :limit :heap-bytes]) tools))
                    :scenarios {:uncached uncached}
                    :outputs {:uncached outputs}})
                 (let [started (System/currentTimeMillis)
                       uncached (rounds options (constantly nil) (build-tools :clean-cmd true))
                       uncached-outputs ((output-check started))
                       _ (doseq [{:keys [clean! dir cached-cmd]} tools] (clean!) (timed dir cached-cmd))
                       cached (rounds options (constantly nil) (build-tools :cached-cmd false))
                       leaf (when-let [leaf (:leaf options)]
                              (leaf-scenario options
                                             (cond-> [(fs/file (:light-dir project) leaf)]
                                               (:shadow-dir project) (conj (fs/file (:shadow-dir project) (or (:shadow-leaf options) leaf))))
                                             (build-tools :cached-cmd false)
                                             (output-check (System/currentTimeMillis))))
                       warm (when (and (:shadow-dir project) (pos? (:warm-jvm options)))
                              (warm-jvm options project (:warm-jvm options)))]
                   {:environment environment
                    :options (dissoc options :light)
                    :project (project-entry tools)
                    :scenarios (cond-> {:uncached uncached :cached cached} leaf (assoc :leaf (:summaries leaf)))
                    :outputs (cond-> {:uncached uncached-outputs} leaf (assoc :leaf (:outputs leaf)))
                    :warm-jvm warm}))
        prefix (or (:out options) (str (fs/path (:dir options) "compare-result")))
        report (if sweep? (sweep-markdown result) (markdown result))]
    (spit (str prefix ".edn") (with-out-str (pprint/pprint result)))
    (spit (str prefix ".md") report)
    (println report)
    (println "Wrote" (str prefix ".edn") "and" (str prefix ".md"))))

(when (= (fs/absolutize *file*)
         (some-> (System/getProperty "babashka.file") fs/absolutize))
  (main *command-line-args*))
