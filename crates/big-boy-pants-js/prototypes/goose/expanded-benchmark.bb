#!/usr/bin/env bb
;; Actual compiler scaling fixture. Historical benchmark modes remain in benchmark.bb.
(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.edn :as edn]
         '[clojure.string :as str] '[cheshire.core :as json])
(import '[java.security MessageDigest])
(def directory (fs/parent (fs/canonicalize *file*)))
(def repository (fs/canonicalize (fs/path directory "../../../..")))
(def mode (or (first *command-line-args*) "--check"))
(assert (#{"--check" "--measure" "--instrument" "--limits" "--summarize"} mode))
(def instrumented? (#{"--instrument" "--limits"} mode))
(def debug? (= "1" (System/getenv "GOOSE_BENCH_DEBUG")))
(assert (or (not debug?) (= mode "--check")))
(def target (fs/path repository "target/expanded-benchmark"))
(fs/create-dirs target)
(def env (-> (into {} (System/getenv))
             (dissoc "LIGHT_CLJS_NAMESPACE_WORKERS" "LIGHT_CLJS_FRESH_UNIT_ISOLATES" "LIGHT_V8_STATS" "LIGHT_CLJS_WAIT_TIMELINE" "GOOSE_BENCH_INSTRUMENT")
             (assoc "CARGO_BUILD_JOBS" "3" "LIGHT_CLJS_V8_CACHE" "require"
                    "CARGO_TARGET_DIR" (str (fs/path repository "target")) "GOOSE_HOME" (str (fs/path repository "target/goose")))))
(defn await-command [proc timeout-ms args]
  (let [result (deref proc timeout-ms ::timeout)]
    (when (= result ::timeout)
      (let [handle (.toHandle (:proc proc))]
        (with-open [children (.descendants handle)]
          (doseq [child (.toArray children)] (.destroyForcibly child)))
        (.destroyForcibly handle))
      (throw (ex-info "Command watchdog expired; stopped owned process tree" {:command args})))
    result))
(defn command [environment & args]
  (let [proc (p/process (vec args) {:dir (str repository) :env environment :out :string :err :string :continue true})
        result (await-command proc (if (get environment "GOOSE_BENCH_DIRECTORY") 180000 900000) args)]
    (assert (zero? (:exit result)) (str "Command failed: " args "\n" (:err result) "\n" (:out result))) (:out result)))
(defn hash-bytes [bytes] (apply str (map #(format "%02x" (bit-and 255 %)) (.digest (MessageDigest/getInstance "SHA-256") bytes))))
(defn hash-file [path] (hash-bytes (fs/read-all-bytes path)))
(defn read-rows [path] (mapv edn/read-string (str/split-lines (slurp (str path)))))
(defn median [xs] (nth (vec (sort xs)) (quot (count xs) 2)))
(def focus (System/getenv "GOOSE_EXPANDED_FOCUS"))
(assert (or (nil? focus) (= "heavier" focus)))
(def predecessor (some-> (System/getenv "GOOSE_EXPANDED_PREDECESSOR") fs/path))
(assert (or (nil? predecessor) (and (= "heavier" focus) (#{"--measure" "--instrument"} mode))))
(def cases (cond->> [{:id :light :work 16} {:id :medium :work 256} {:id :heavier :work 8192}]
             focus (filterv #(= :heavier (:id %)))))
(def cells (if predecessor
             [{:engine :v8 :workers 1 :control :branch :revision-label :current}
              {:engine :goose :workers 8 :control :branch :revision-label :predecessor}
              {:engine :goose :workers 8 :control :branch :revision-label :current}]
             (if focus
               (vec (cons {:engine :v8 :workers 1 :control :branch}
                          (for [n (if (= mode "--check") [1 2 4 8] [8])] {:engine :goose :workers n :control :branch})))
               (vec (concat [{:engine :v8 :workers 1 :control :branch}
                         {:engine :pants :workers 1 :control :branch}
                         {:engine :goose :workers 1 :control :serial}]
                       (for [n [1 2 4 8]] {:engine :goose :workers n :control :branch}))))))
(defn unsupported? [c cell]
  (and (= :heavier (:id c)) (= :goose (:engine cell)) (= :serial (:control cell))))
(defn active-cells [c] (remove #(unsupported? c %) cells))
(def seed 24301)
(def width 32)
(def namespaces-per-wave 7)
(def calls-per-namespace 8)
(def deferred-calls-per-namespace 7)
(def drain-lag 6)
(def phase-counts [56 8 56])
(def macro-source
  "(ns macros) (def shared [11 22])\n(defmacro work [n x target]\n  (loop [i 0 v [] m {}]\n    (if (= i 32)\n      (loop [s (seq v) initial 0]\n        (if s (recur (next s) (+ initial (first s) (get m 1 0) (count (rest [1 2 3]))))\n          (loop [j 0 k 0 total initial]\n            (if (= j n) (if target (list 'def target (list 'quote [total x shared])) (list 'quote [total x shared]))\n              (recur (+ j 1) (if (= k 31) 0 (+ k 1)) (+ total (+ (+ (get v k 0) (get m k 0)) (count v))))))))\n      (recur (+ i 1) (conj v i) (assoc m (count v) (count m))))))\n")
(defn payload [marker] (str "[" marker " " (str/join " " (map #(mod (+ seed (* 17 %)) 101) (range width))) "]"))
(defn expected-output [c]
  (let [result (fn [marker call]
                 (let [n (+ (:work c) (mod call 2))]
                   [(+ 592 (* (quot n 32) 2016) (* (mod n 32) 32) (* (mod n 32) (dec (mod n 32))))
                    (edn/read-string (payload (+ marker call))) [11 22]]))
        wave (fn [w] (vec (for [i (range namespaces-per-wave) call (range calls-per-namespace)]
                                      (result (+ (* w 1000) (* i calls-per-namespace)) call))))]
    (into (into [(wave 0)] (map #(result 2000 %) (range calls-per-namespace))) (wave 1))))
(defn require-wave [wave] (str/join " " (for [i (range namespaces-per-wave)] (str "[wave" wave "_" i " :as w" i "]"))))
(defn wave-values [wave] (str/join " " (for [i (range namespaces-per-wave) call (range calls-per-namespace)] (str (if (= wave 0) "w" "r") i "/result" call))))
(defn fixture! [c]
  (let [dir (fs/path target (name (:id c)))
        _ (fs/create-dirs dir)
        _ (doseq [file (fs/glob dir "*.cljs")] (fs/delete file))
        _ (doseq [name ["reference.js" "reference.stdout"]]
            (fs/delete-if-exists (fs/path dir name)))
        definitions (fn [marker] (str/join "\n" (for [call (range calls-per-namespace)]
                                   (if (< call deferred-calls-per-namespace)
                                     (str "(def result" call " (work " (+ (:work c) (mod call 2)) " " (payload (+ marker call)) " nil))")
                                     (str "(work " (+ (:work c) (mod call 2)) " " (payload (+ marker call)) " result" call ")")))))]
    (spit (str (fs/path dir "input.cljs")) "")
    (spit (str (fs/path dir "macros.cljs")) macro-source)
    (doseq [wave [0 1] i (range namespaces-per-wave)]
      (spit (str (fs/path dir (str "wave" wave "_" i ".cljs")))
            (str "(ns wave" wave "_" i " (:require " (when (= wave 1) "[reusegate5] ") "[macros :refer [work]]))\n" (definitions (+ (* wave 1000) (* i calls-per-namespace))) "\n")))
    ;; These ordinary require chains enforce phase boundaries at DRAIN_LAG.
    ;; They are excluded from the substantive namespace count.
    (doseq [prefix ["coldgate" "reusegate"] i (range drain-lag)]
      (spit (str (fs/path dir (str prefix i ".cljs")))
            (str "(ns " prefix i " (:require " (if (zero? i) (if (= prefix "coldgate") (require-wave 0) "[join]") (str "[" prefix (dec i) "]")) "))\n")))
    (spit (str (fs/path dir "join.cljs"))
          (str "(ns join (:require [coldgate5] " (require-wave 0) " [macros :refer [work]]))\n"
               (definitions 2000) "\n(def inputs [" (wave-values 0) "])\n"))
    (spit (str (fs/path dir "main.cljs"))
          (str "(ns main (:require [join :as j] " (str/join " " (for [i (range namespaces-per-wave)] (str "[wave1_" i " :as r" i "]"))) "))\n"
               "(js/console.log (pr-str [j/inputs " (str/join " " (map #(str "j/result" %) (range calls-per-namespace))) " " (wave-values 1) "]))\n"))
    (assoc c :directory (str dir) :source-hashes (into (sorted-map) (for [f (sort (fs/glob dir "*.cljs"))] [(str (fs/file-name f)) (hash-file f)]))
           :source-bytes (reduce + (map fs/size (fs/glob dir "*.cljs"))))))
(defn quiet! []
  (let [snapshot (command env "top" "-l" "2" "-s" "1" "-n" "0")
        idle (Double/parseDouble (second (last (re-seq #"([0-9.]+)% idle" snapshot))))
        processes (command env "ps" "-axo" "pid,ppid,%cpu,comm")]
    (assert (>= idle 90.0) (str "Quiet measurement gate failed: idle=" idle))
    (assert (not (re-find #"(?m)(?:/|\s)(?:rustc|cargo|goose|compiler-session(?:-[a-z]+)?)\s*$" processes)) "Competing compiler/build")
    {:idle-percent idle :snapshot snapshot :time (str (java.time.Instant/now))}))
(def samples (if-let [path (System/getenv "GOOSE_EXPANDED_SAMPLES")] (fs/path path)
                (fs/path directory (str "expanded-" (subs mode 2) (when debug? "-debug") "-samples.edn"))))
(defn summary! [path]
  (let [rows (if (fs/exists? path) (read-rows path) (mapv edn/read-string (str/split-lines (command env "gzip" "-cd" (str path ".gz")))))
        samples (filter #(= :sample (:kind %)) rows)]
    (doseq [[cell rs] (sort-by (comp pr-str key) (group-by #(select-keys % [:case :engine :workers :control :revision-label]) samples))]
      (let [metrics (fn [key] (let [xs (keep key rs)] (when (seq xs) {:median (median xs) :min (apply min xs) :max (apply max xs)})))
            phase (fn [phase n key scale] (metrics #(some (fn [r] (when (and (= phase (:phase r)) (or (nil? n) (= n (:batch r)))) (/ (get r key) scale))) (:rows %))))]
        (println (pr-str (merge cell {:rounds (count rs) :wall-ms (metrics #(/ (:process-wall-ns %) 1e6)) :user-ms (metrics :user-ms) :system-ms (metrics :system-ms) :test-process-peak-rss-bytes (metrics :test-process-peak-rss-bytes)
                                     :cold-ms (phase :batch 0 :wall-ns 1e6) :join-ms (phase :batch 1 :wall-ns 1e6) :reuse-ms (phase :batch 2 :wall-ns 1e6)
                                     :analyze-ns (phase :compiler nil :analyze-ns 1) :cleanup-ns (phase :compiler nil :cleanup-ns 1)})))))))
(if (= mode "--summarize") (summary! (or (second *command-line-args*) (fs/path directory "expanded-measure-samples.edn")))
  (let [_ (when instrumented? (load-file (str (fs/path directory "expanded-instrument.bb"))))
        native-source (if instrumented? (fs/path target "instrumented/compiler_session.goose") (fs/path directory "compiler_session.goose"))
        native (str (fs/path target (if instrumented? "compiler-session-instrumented" (if debug? "compiler-session-debug" "compiler-session"))))
        _ (println "Build native runner and release compiler executable (excluded from measurements)")
        _ (command env "goose" "format" "--check" (str native-source))
        _ (command env "goose" "check" (str native-source))
        _ (apply command env (concat ["goose" "build" (str native-source)] (when debug? ["--debug"]) ["-o" native]))
        build (command env "cargo" "test" "--release" "--locked" "--features" "goose-macros" "--lib" "final_benchmark" "--no-run" "--message-format=json")
        binary (some #(let [r (json/parse-string % true)] (when (and (= "compiler-artifact" (:reason r)) (get-in r [:profile :test]) (:executable r)) (:executable r))) (str/split-lines build))
        _ (assert binary)
        _ (spit (str (fs/path target "binary-path.txt")) binary)
        prepared (mapv fixture! cases)
        _ (when (= mode "--instrument")
            (let [wrapper (fs/path target "native-instrument-resources")]
              (spit (str wrapper) "#!/bin/sh\nexec /usr/bin/time -l -o \"$GOOSE_BENCH_DIRECTORY/native-resources.txt\" \"$GOOSE_BENCH_NATIVE\" \"$@\"\n")
              (.setExecutable (fs/file wrapper) true)))
        predecessor-binary (when predecessor (str (fs/path predecessor "compiler-binary")))
        predecessor-native (when predecessor
                             (if (= mode "--instrument")
                               (let [old-directory (fs/path predecessor "crates/big-boy-pants-js/prototypes/goose")
                                     old-target (fs/path target "predecessor")
                                     executable (str (fs/path old-target "compiler-session-instrumented"))]
                                 ;; Instrument the preserved predecessor with its own
                                 ;; matching source transform, never today's anchors.
                                 (with-redefs [directory old-directory target old-target]
                                   (load-file (str (fs/path old-directory "expanded-instrument.bb"))))
                                 (command env "goose" "build" (str (fs/path old-target "instrumented/compiler_session.goose")) "-o" executable)
                                 executable)
                               (str (fs/path predecessor "compiler-session"))))
        invoke (fn [c cell]
                 (let [old? (= :predecessor (:revision-label cell))
                       executable (if old? predecessor-binary binary)
                       runner (if old? predecessor-native native)
                       dir (:directory c)
                       e (merge env {"GOOSE_COMPILER_RUNNER" runner "GOOSE_BENCH_DIRECTORY" dir "GOOSE_BENCH_SCOPE" "compiler" "GOOSE_BENCH_FANOUT" "56" "GOOSE_BENCH_PHASE_COUNTS" (str/join "," phase-counts) "GOOSE_BENCH_ENGINE" (name (:engine cell)) "GOOSE_BENCH_WORKERS" (str (:workers cell)) "GOOSE_BENCH_CONTROL" (name (:control cell))}
                                (when (= mode "--instrument") {"GOOSE_BENCH_INSTRUMENT" "1" "LIGHT_CLJS_WAIT_TIMELINE" (str (fs/path dir "timeline.tsv")) "GOOSE_BENCH_ACTIVITY" (str (fs/path dir "activity.edn")) "GOOSE_BENCH_NATIVE" runner "GOOSE_COMPILER_RUNNER" (str (fs/path target "native-instrument-resources"))}))
                       _ (when (= mode "--instrument") (spit (str (fs/path dir "activity.edn")) ""))
                       started (System/nanoTime)
                       _ (command e "/usr/bin/time" "-l" "-o" (str (fs/path dir "resources.txt")) executable "compiler_bootstrap::goose_capture::benchmark::final_benchmark" "--ignored" "--exact" "--test-threads=1")
                       wall (- (System/nanoTime) started)
                       resources (slurp (str (fs/path dir "resources.txt")))
                       [_ _ user system] (re-find #"([0-9.]+) real\s+([0-9.]+) user\s+([0-9.]+) sys" resources)
                       js (fs/path dir "actual.js") reference (fs/path dir "reference.js")]
                   (if (fs/exists? reference) (assert (= (hash-file reference) (hash-file js)) (str "Exact JS mismatch " c cell))
                     (do (assert (= :v8 (:engine cell))) (fs/copy js reference)))
                   (let [out (command env "node" (str js)) refout (fs/path dir "reference.stdout")]
                     (assert (= (expected-output c) (edn/read-string out)) "Independent result oracle mismatch")
                     (if (fs/exists? refout) (assert (= (slurp (str refout)) out) "Node output mismatch") (spit (str refout) out)))
                   (merge cell {:case (:id c) :process-wall-ns wall :rows (read-rows (fs/path dir "rows.edn"))
                                :user-ms (* 1000 (Double/parseDouble user)) :system-ms (* 1000 (Double/parseDouble system))
                                :test-process-peak-rss-bytes (some-> (re-find #"(?m)^\s*(\d+)\s+maximum resident set size" resources) second parse-long)
                                :resources resources :activity (when (= mode "--instrument") (read-rows (fs/path dir "activity.edn"))) :timeline (when (= mode "--instrument") (slurp (str (fs/path dir "timeline.tsv"))))
                                :native-resources (when (and (= mode "--instrument") (= :goose (:engine cell))) (slurp (str (fs/path dir "native-resources.txt"))))
                                :js-sha256 (hash-file js)})))
        rounds (if (= mode "--measure") 5 1)]
    (assert (not (fs/exists? (str samples ".gz"))) (str "Preserve existing data: " samples ".gz"))
    (spit (str samples) (str (pr-str {:kind :manifest :version 1 :seed seed :width width :calls-per-namespace calls-per-namespace :deferred-calls-per-namespace deferred-calls-per-namespace :independent-namespaces-per-wave namespaces-per-wave
                                    :substantive-namespaces 15 :ordering-namespaces 12 :helper-namespaces 1 :entry-namespaces 1 :total-namespaces 29
                                    :phase-counts phase-counts :drain-lag drain-lag :cases (mapv #(dissoc % :directory) prepared) :cells cells
                                    :macro-timeout-seconds 120 :vm-limits {:nodes 100000 :values 1048576 :work 10000000 :steps-per-job 1000000}
                                    :unsupported-cells [{:case :heavier :engine :goose :workers 1 :control :serial :reason :retained-session-budget-exceeded}]
                                    :rounds rounds :warmup (if (= mode "--measure") :one-complete-process-per-cell :none) :focus focus
                                    :pair-order (when predecessor :alternating-adjacent-predecessor-current)
                                    :predecessor (when predecessor {:revision (str/trim (slurp (str (fs/path predecessor "revision.txt"))))
                                                                    :binary-sha256 (hash-file predecessor-binary) :native-sha256 (hash-file predecessor-native)
                                                                    :native-source-sha256 (hash-file (fs/path predecessor "compiler_session.goose"))
                                                                    :adapter-source-sha256 (hash-file (fs/path predecessor "src/goose_evaluator.rs"))})
                                    :debug debug? :revision (str/trim (command env "git" "rev-parse" "HEAD")) :diff-sha256 (hash-bytes (.getBytes (command env "git" "diff") "UTF-8"))
                                    :benchmark-source-sha256 (hash-file (fs/path directory "expanded-benchmark.bb")) :rust-source-sha256 (hash-file (fs/path directory "benchmark.rs")) :native-source-sha256 (hash-file native-source)
                                    :instrument-source-sha256 (when (= mode "--instrument") (hash-file (fs/path directory "expanded-instrument.bb")))
                                    :binary-sha256 (hash-file binary) :native-sha256 (hash-file native) :goose (str/trim (command env "goose" "version")) :rustc (str/trim (command env "rustc" "--version")) :node (str/trim (command env "node" "--version"))
                                    :os (str/trim (command env "uname" "-a")) :hardware (str/trim (command env "sysctl" "-n" "machdep.cpu.brand_string")) :environment (select-keys env ["CARGO_BUILD_JOBS" "GOOSE_HOME" "LIGHT_CLJS_V8_CACHE"])
                                    :resource-method :darwin-time-l-test-process :native-runner-wrapped (= mode "--instrument") :output-checked :exact-js-node-stdout-and-independent-numeric-oracle}) "\n"))
    (when (= mode "--limits")
      (let [c (last prepared)
            e (merge env {"GOOSE_COMPILER_RUNNER" native "GOOSE_BENCH_DIRECTORY" (:directory c) "GOOSE_BENCH_SCOPE" "compiler" "GOOSE_BENCH_FANOUT" "56" "GOOSE_BENCH_PHASE_COUNTS" (str/join "," phase-counts) "GOOSE_BENCH_ENGINE" "goose" "GOOSE_BENCH_WORKERS" "1" "GOOSE_BENCH_CONTROL" "serial"})
            proc (p/process [binary "compiler_bootstrap::goose_capture::benchmark::final_benchmark" "--ignored" "--exact" "--test-threads=1"] {:dir (str repository) :env e :out :string :err :string :continue true})
            result (await-command proc 180000 [binary :limit-diagnosis])
            text (str (:out result) (:err result))
            fault (re-find #"serial-([A-Za-z.]+) work=(\d+) nodes=(\d+) values=(\d+) steps=(\d+)" text)]
        (assert (and (not (zero? (:exit result))) fault) (str "Expected a precise serial quota failure: " text))
        (spit (str samples) (str (pr-str {:kind :unsupported :case :heavier :engine :goose :workers 1 :control :serial :fault (second fault) :work (parse-long (nth fault 2)) :nodes (parse-long (nth fault 3)) :values (parse-long (nth fault 4)) :steps (parse-long (nth fault 5)) :result (select-keys result [:exit :out :err])}) "\n") :append true)
        (println "Confirmed retained serial limit:" (second fault))
        (System/exit 0)))
    (doseq [c prepared]
      (println "Verify fixture" (:id c)) (flush)
      (invoke c (first cells))
      (when (= mode "--measure")
        (spit (str samples) (str (pr-str (assoc (quiet!) :kind :quiet :case (:id c))) "\n") :append true)
        (doseq [cell (active-cells c)] (invoke c cell)))
      (doseq [round (range rounds) cell (if (even? round) (active-cells c) (reverse (active-cells c)))]
        (println "Run" (:id c) (inc round) (:engine cell) (:control cell) (:workers cell)) (flush)
        (let [sample (assoc (invoke c cell) :kind :sample :round (inc round))]
          (spit (str samples) (str (pr-str sample) "\n") :append true))))
    (when (= mode "--measure") (command env "gzip" "-n" (str samples)))
    (println "Saved" samples)
    (when (= mode "--measure") (summary! samples))))
