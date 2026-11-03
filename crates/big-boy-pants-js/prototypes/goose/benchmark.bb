#!/usr/bin/env bb
(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.edn :as edn]
         '[clojure.string :as str] '[cheshire.core :as json])
(import '[java.security MessageDigest])
(def directory (fs/parent (fs/canonicalize *file*)))
(def repository (fs/canonicalize (fs/path directory "../../../..")))
(def mode (or (first *command-line-args*) "--measure"))
(def initialization? (#{"--initialization-comparison" "--initialization-allocations"} mode))
(def timing? (#{"--measure" "--initialization-comparison"} mode))
(def allocations? (#{"--allocations" "--initialization-allocations"} mode))
(def native-flags (when (= "1" (System/getenv "GOOSE_BENCH_DEBUG")) ["--debug"]))
(assert (or (empty? native-flags) (= mode "--check")) "Debug native builds are correctness-only")
(assert (#{"--check" "--measure" "--allocations" "--summarize" "--initialization-comparison" "--initialization-allocations"} mode) "Unknown benchmark mode")
(def sample-path (fs/path directory (case mode "--initialization-comparison" "initialization-samples.edn" "--initialization-allocations" "initialization-allocations.edn" "--allocations" "branch-allocations.edn" "--check" ".benchmark-check-samples.edn" "branch-samples.edn")))
(def environment (assoc (into {} (System/getenv)) "CARGO_BUILD_JOBS" "3" "LIGHT_CLJS_V8_CACHE" "require" "CARGO_TARGET_DIR" (str (fs/path repository "target"))))
(def thread-observations (atom {}))
(def diagnostics (atom ""))
(defn command [env & args]
  (let [options {:dir (str repository) :env env :out :string :err :string :continue true}
        result (if (and allocations? (get env "GOOSE_BENCH_DIRECTORY") (not= "storage" (get env "GOOSE_BENCH_ENGINE")))
                 (let [proc (p/process (vec args) options) handle (.toHandle (:proc proc)) owned (atom [])]
                   (try
                     (loop []
                       (with-open [descendants (.descendants handle)]
                         (doseq [child (.toArray descendants)]
                           (swap! owned conj child)
                           (let [role (str (fs/file-name (.orElse (.command (.info child)) "unknown")))
                                 text (:out (p/shell {:out :string :err :string :continue true} "ps" "-M" "-p" (str (.pid child))))
                                 threads (max 0 (dec (count (str/split-lines text))))]
                             (swap! thread-observations update role (fnil max 0) threads))))
                       (let [value (deref proc 10 ::waiting)] (if (= value ::waiting) (recur) value)))
                     (finally (doseq [child @owned :when (.isAlive child)] (.destroyForcibly child)))))
                 (apply p/shell options args))]
    (when (get env "GOOSE_BENCH_DIRECTORY") (reset! diagnostics (:err result)))
    (assert (zero? (:exit result)) (str "Command failed: " args "\n" (:err result) "\n" (:out result))) (:out result)))
(defn hash-bytes [bytes] (apply str (map #(format "%02x" (bit-and 255 %)) (.digest (MessageDigest/getInstance "SHA-256") bytes))))
(defn hash-file [path] (hash-bytes (fs/read-all-bytes path)))
(defn read-rows [path] (mapv edn/read-string (str/split-lines (if (fs/exists? path) (slurp (str path)) (command environment "gzip" "-cd" (str path ".gz"))))))
(defn application-values [stdout] (vec (mapcat edn/read-string (str/split-lines stdout))))
(defn quiet! []
  (let [snapshot (command environment "top" "-l" "2" "-s" "1" "-n" "0")
        idle (Double/parseDouble (second (last (re-seq #"([0-9.]+)% idle" snapshot))))
        processes (command environment "ps" "-axo" "pid,ppid,%cpu,comm")]
    (assert (>= idle 90.0) (str "Quiet measurement gate failed: idle=" idle))
    (assert (not (re-find #"(?m)\s(?:rustc|cargo|goose|compiler-session)\s*$" processes)) "Competing build/measurement process")
    {:kind :quiet :time (str (java.time.Instant/now)) :idle-percent idle :top snapshot}))
(def seed 24301)
(def cases [{:id :small :work 8 :fanout 1 :shape :wide :width 8}
            {:id :medium :work 32 :fanout 4 :shape :wide :width 32}
            {:id :larger :work 100 :fanout 8 :shape :wide :width 100}
            {:id :short :work 32 :fanout 4 :shape :short :width 100}
            {:id :deep :work 32 :fanout 4 :shape :deep :depth 12}
            {:id :plain :work 32 :fanout 4 :shape :plain :width 32}
            {:id :decoded :work 32 :fanout 4 :shape :decoded :width 32}])
(when initialization? (alter-var-root #'cases #(subvec % 0 3)))
;; Adapted from corpus/churn.cljs, originating in benchmarks/native-macros-k.bb.
(def macro-source "(defmacro work [n x] (loop [i 0 v [] m {}] (if (= i n) (loop [s (seq v) total 0] (if s (recur (next s) (+ total (first s) (get m 1 0) (count (rest [1 2 3])))) (list 'quote [total x]))) (recur (+ i 1) (conj v i) (assoc m (count v) (count m))))))\n")
(defn payload [{:keys [shape width depth]}]
  (case shape :wide (str "[" (str/join " " (map #(mod (+ seed (* 17 %)) 101) (range width))) "]")
        :short (str "[" (str/join " " (map #(str "\"v" (mod % 10) "\"") (range width))) "]")
        :deep (str (apply str (repeat depth "[")) "7" (apply str (repeat depth "]")))
        (:plain :decoded) (str "[" (str/join " " (repeat width (if (= shape :plain) "\"café\"" "\"caf\\u00e9\""))) "]")))
(def cells (vec (concat (for [engine [:v8 :pants :goose] workers (if (= engine :v8) [1] [1 2 4 8])] {:engine engine :workers workers :control :branch})
                       (for [engine [:v8 :pants :goose] control [:serial :rebuild]] {:engine engine :workers 1 :control control}))))
(defn median [xs] (nth (vec (sort xs)) (quot (count xs) 2)))
(defn observations [r]
  (let [phase (fn [p k] (let [values (keep #(when (= p (:phase %)) (get % k)) (:rows r))] (when (seq values) (reduce + values))))
        batch (fn [n] (some #(when (and (= :batch (:phase %)) (= n (:batch %))) (:wall-ns %)) (:rows r)))
        sessions (keep #(when (= :adapter (:phase %)) (:stats %)) (:rows r))
        stats (when (seq sessions) (apply merge-with + (map #(dissoc % :trace) sessions)))
        by-session (mapv (fn [s] (mapv #(mapv parse-long (drop 2 (str/split % #"\s+"))) (filter #(str/starts-with? % "METRIC ") (:trace s)))) sessions)
        native (vec (mapcat identity by-session))
        worker (reduce + 0 (map #(nth % 2) native))
        native-wall (when (some #(get % 12) native) (reduce + (map #(reduce max 0 (keep (fn [row] (get row 12)) %)) by-session)))]
    (into {} (remove (comp nil? val)
      {:process-wall-ns (:process-wall-ns r) :prepare-ns (phase :prepare :ns) :runtime-ns (phase :runtime :ns)
       :cold-batch-ns (batch 0) :reused-batch-ns (batch 1) :dispose-ns (phase :dispose :ns)
       :compiler-analyze-ns (phase :compiler :analyze-ns) :compiler-emit-ns (phase :compiler :emit-ns) :compiler-cleanup-ns (phase :compiler :cleanup-ns)
       :rust-export-sum-ns (:export-ns stats) :rust-bridge-sum-ns (:rust-bridge-ns stats) :native-base-capture-ns (:snapshot-ns stats)
       :native-restore-sum-ns (when (seq native) (reduce + (map first native)))
       :native-execution-sum-ns (when (seq native) (reduce + (map second native)))
       :native-worker-active-lower-bound-ns (when (seq native) worker)
       :native-observed-wall-ns native-wall
       :coordinator-sleep-ns (when (some #(get % 11) native) (reduce + (map #(reduce max 0 (keep (fn [row] (get row 11)) %)) by-session)))
       :worker-idle-or-uninstrumented-fraction (when native-wall (max 0.0 (- 1.0 (/ worker (* (:workers r) native-wall)))))}))))
(defn summarize []
  (let [rows (read-rows (fs/path directory "branch-samples.edn")) samples (filter #(= :sample (:kind %)) rows)]
    (doseq [[cell rs] (sort-by (comp pr-str key) (group-by #(select-keys % [:scope :case :engine :workers :control]) samples))]
      (let [times (map #(/ (:process-wall-ns %) 1e6) rs)
            reference (filter #(and (= (:scope cell) (:scope %)) (= (:case cell) (:case %)) (= :v8 (:engine %)) (= :branch (:control %))) samples)
            one (filter #(and (= (:scope cell) (:scope %)) (= (:case cell) (:case %)) (= (:engine cell) (:engine %)) (= 1 (:workers %)) (= :branch (:control %))) samples)
            metrics (apply merge-with into (map #(update-vals (observations %) vector) rs))]
        (println (pr-str (merge cell {:rounds (count rs) :median-ms (median times) :min-ms (apply min times) :max-ms (apply max times)
                                     :ratio-to-v8 (/ (median times) (/ (median (map :process-wall-ns reference)) 1e6))
                                     :speedup-vs-own-one (when (= :branch (:control cell)) (/ (median (map :process-wall-ns one)) (median (map :process-wall-ns rs))))
                                     :metrics (update-vals metrics #(hash-map :median (median %) :min (apply min %) :max (apply max %)))})))))))
(if (= mode "--summarize") (summarize)
(let [temporary (fs/create-temp-dir {:dir (str directory) :prefix ".goose-benchmark-"}) native (str (fs/path temporary "compiler-session"))]
  (try
    (println "Build native runner and release reference executable (excluded from measurements)")
    (command environment "goose" "format" "--check" (str (fs/path directory "compiler_session.goose")))
    (command environment "goose" "check" (str (fs/path directory "compiler_session.goose")))
    (apply command environment (concat ["goose" "build" (str (fs/path directory "compiler_session.goose"))] native-flags ["-o" native]))
    (let [wrapper (fs/path temporary "native-resources")
          _ (spit (str wrapper) "#!/bin/sh\nexec /usr/bin/time -l -o \"$GOOSE_BENCH_DIRECTORY/native-memory-$$.txt\" \"$GOOSE_NATIVE_BINARY\" \"$@\"\n")
          _ (.setExecutable (fs/file wrapper) true)
          features (str "goose-macros" (when allocations? ",count-allocations"))
          build (command environment "cargo" "test" "--release" "--locked" "--features" features "--lib" "final_benchmark" "--no-run" "--message-format=json")
          binary (some #(let [row (json/parse-string % true)] (when (and (= "compiler-artifact" (:reason row)) (get-in row [:profile :test]) (:executable row)) (:executable row))) (str/split-lines build))
          predecessor-binary (when (and initialization? timing?) (System/getenv "GOOSE_PREDECESSOR_BINARY"))
          predecessor-native (when (and initialization? timing?) (System/getenv "GOOSE_PREDECESSOR_RUNNER"))
          predecessor-root (when (and initialization? timing?) (System/getenv "GOOSE_PREDECESSOR_ROOT"))
          predecessor-revision (when predecessor-root (str/trim (command environment "git" "-C" predecessor-root "rev-parse" "HEAD")))
          _ (when (and initialization? timing?) (assert (and (fs/exists? predecessor-binary) (fs/exists? predecessor-native)) "Build exact predecessor and set GOOSE_PREDECESSOR_BINARY/RUNNER"))
          _ (when (and initialization? timing?)
              (assert (= "f29175c4496a25ca77812dcad4a10eb0aff4263e" predecessor-revision) "Expected exact initialization predecessor")
              (command environment "git" "-C" predecessor-root "diff" "--exit-code" "HEAD"))
          env (cond-> (assoc environment "GOOSE_COMPILER_RUNNER" native)
                allocations? (assoc "LIGHT_V8_STATS" "1" "GOOSE_COMPILER_RUNNER" (str wrapper) "GOOSE_NATIVE_BINARY" native))
          prepared (mapv (fn [c]
                           (let [dir (fs/path temporary (name (:id c))) _ (fs/create-dirs dir)
                                 calls (mapv #(str "(work " (+ (:work c) (mod % 2)) " " (payload c) ")") (range (:fanout c)))
                                 cljs (str macro-source "(js/console.log (pr-str [" (str/join " " calls) "]))\n")
                                 definitions (str/join "\n" (map-indexed #(str "(def result" %1 " " %2 ")") calls))
                                 main (str "(ns main (:require [batch1 :as b1] [batch0 :as b0]))\n(js/console.log (pr-str [" (str/join " " (for [batch [0 1] i (range (:fanout c))] (str "b" batch "/result" i))) "]))\n")]
                             (spit (str (fs/path dir "input.cljs")) cljs)
                             (spit (str (fs/path dir "macros.cljs")) (str "(ns macros) (def shared [11 22])\n" (str/replace macro-source "[total x]" "[total x shared]")))
                             (spit (str (fs/path dir "main.cljs")) main)
                             (doseq [batch [0 1]] (spit (str (fs/path dir (str "batch" batch ".cljs")))
                               (str "(ns batch" batch " (:require " (when (= batch 1) "[batch0] ") "[macros :refer [work]]))\n" definitions "\n")))
                             (doseq [[index call] (map-indexed vector calls)]
                               (let [job (fs/path dir (str "job-" index))] (fs/create-dirs job)
                                 (fs/copy (fs/path dir "macros.cljs") (fs/path job "macros.cljs"))
                                 (spit (str (fs/path job "main.cljs")) (str "(ns main (:require [macros :refer [work]]))\n(def result " call ")\n(js/console.log (pr-str [result]))\n"))))
                             (assoc c :directory (str dir) :input-bytes (count (.getBytes cljs "UTF-8")) :payload-nodes (inc (or (:width c) (:depth c))) :payload-utf16-units (case (:shape c) :short (* 2 (:width c)) (:plain :decoded) (* 4 (:width c)) 0) :source-sha256 (hash-file (fs/path dir "input.cljs"))
                                    :compiler-source-hashes (into {} (for [path (sort (concat (fs/glob dir "*.cljs") (fs/glob dir "job-*/*.cljs")))] [(str (fs/relativize dir path)) (hash-file path)]))
                                    :compiler-source-bytes (reduce + (for [file ["macros.cljs" "batch0.cljs" "batch1.cljs" "main.cljs"]] (fs/size (fs/path dir file))))
                                    :compiler-rebuild-source-bytes-per-batch (reduce + (map fs/size (fs/glob dir "job-*/*.cljs")))))) cases)
          invoke (fn [c scope cell]
                   (let [predecessor? (= :predecessor (:revision cell))
                         binary (if predecessor? predecessor-binary binary)
                         env (merge env {"GOOSE_BENCH_DIRECTORY" (:directory c) "GOOSE_BENCH_FANOUT" (str (:fanout c)) "GOOSE_BENCH_SCOPE" (name scope) "GOOSE_BENCH_ENGINE" (name (:engine cell)) "GOOSE_BENCH_WORKERS" (str (:workers cell)) "GOOSE_BENCH_CONTROL" (name (:control cell))}
                                    (when predecessor? {"GOOSE_COMPILER_RUNNER" predecessor-native}))
                         _ (reset! thread-observations {})
                         _ (when allocations? (doseq [path (fs/glob (:directory c) "native-memory-*.txt")] (fs/delete path)))
                         started (System/nanoTime)
                         _ (apply command env (concat (when (not timing?) ["/usr/bin/time" "-l" "-o" (str (fs/path (:directory c) "resources.txt"))]) [binary "compiler_bootstrap::goose_capture::benchmark::final_benchmark" "--ignored" "--exact" "--test-threads=1"] (when allocations? ["--nocapture"])))
                         wall (- (System/nanoTime) started)
                         rows (when (fs/exists? (fs/path (:directory c) "rows.edn")) (read-rows (fs/path (:directory c) "rows.edn")))
                         resource (if timing? "" (slurp (str (fs/path (:directory c) "resources.txt"))))
                         peak (some-> (re-find #"(?m)^\s*(\d+)\s+maximum resident set size" resource) second parse-long)]
                     (when (= scope :compiler)
                       (let [actual (hash-file (fs/path (:directory c) "actual.js")) reference (fs/path (:directory c) (if (= :rebuild (:control cell)) "reference-rebuild.js" "reference.js"))]
                         (if (fs/exists? reference) (assert (= actual (hash-file reference)) (str "Compiler JS mismatch " (:id c) " " cell))
                           (do (assert (= :v8 (:engine cell))) (fs/copy (fs/path (:directory c) "actual.js") reference)))))
                     (merge cell {:scope scope :case (:id c) :process-wall-ns wall :rust-peak-rss-bytes peak :rows rows}
                            (when allocations?
                              (cond-> {:storage-diagnostics @diagnostics :observed-thread-peaks @thread-observations :native-process-resources (mapv #(slurp (str %)) (sort (fs/glob (:directory c) "native-memory-*.txt"))) :allocation-counts (edn/read-string (slurp (str (fs/path (:directory c) "allocations.edn"))))}
                                (and (= scope :vm) (= :goose (:engine cell)) (= :rebuild (:control cell)))
                                (assoc :rebuild-adapter-stats (mapv #(edn/read-string (slurp (str %))) (sort (fs/glob (:directory c) "rebuild-*.edn")))))))))]
      (assert binary)
      (spit (str sample-path) (str (pr-str {:kind :manifest :version 2 :seed seed :rounds (if (or (= mode "--check") allocations?) 1 5) :warmup (if timing? :one-complete-cell-process-per-revision :none) :compiler-batches :two-dependent-namespaces-in-one-compilation :compiler-rebuild :fresh-whole-compilation-per-source-job :native-debug (boolean (seq native-flags)) :cases (mapv #(dissoc % :directory) prepared) :cells cells
                                         :predecessor (when predecessor-binary {:revision predecessor-revision :binary-sha256 (hash-file predecessor-binary) :native-sha256 (hash-file predecessor-native)
                                                                               :native-source-sha256 (hash-file (fs/path predecessor-root "crates/big-boy-pants-js/prototypes/goose/compiler_session.goose"))
                                                                               :rust-adapter-sha256 (hash-file (fs/path predecessor-root "src/goose_evaluator.rs"))})
                                         :revision (str/trim (command environment "git" "rev-parse" "HEAD")) :tracked-diff-sha256 (hash-bytes (.getBytes (command environment "git" "diff") "UTF-8")) :rust-tree (hash-file (fs/path directory "benchmark.rs")) :native-source-sha256 (hash-file (fs/path directory "compiler_session.goose")) :native-adapter-sha256 (hash-file (fs/path directory "measurement.h"))
                                         :goose (str/trim (command environment "goose" "version")) :bb (str/trim (command environment "bb" "--version")) :rust (str/trim (command environment "rustc" "-Vv"))
                                         :clang (str/trim (command environment "cc" "--version")) :machine (str/trim (command environment "sysctl" "-n" "machdep.cpu.brand_string" "hw.ncpu" "hw.memsize")) :os (str/trim (command environment "sw_vers"))
                                         :native-command (apply command environment (concat ["goose" "build" (str (fs/path directory "compiler_session.goose"))] native-flags ["--dry-run" "-o" native]))
                                         :build-environment (select-keys environment ["RUSTFLAGS" "CARGO_ENCODED_RUSTFLAGS" "GOOSE_CC" "LIGHT_V8_FLAGS" "LIGHT_CLJS_REPLAY_BOOT" "LIGHT_V8_STATS" "CARGO_BUILD_JOBS" "LIGHT_CLJS_V8_CACHE"])
                                         :before-load (str/trim (command environment "uptime")) :before-processes (str/split-lines (command environment "ps" "-axo" "pid,ppid,%cpu,comm")) :binary-sha256 (hash-file binary) :native-sha256 (hash-file native)}) "\n"))
      ;; Independent V8 emitted JS and application-output oracle before timing.
      (doseq [c prepared]
        (invoke c :compiler {:engine :v8 :workers 1 :control :branch})
        (spit (str (fs/path (:directory c) "reference.stdout")) (command env "node" (str (fs/path (:directory c) "reference.js"))))
        (invoke c :compiler {:engine :v8 :workers 1 :control :rebuild})
        (let [rebuilt (command env "node" (str (fs/path (:directory c) "reference-rebuild.js")))]
          (assert (= (application-values rebuilt) (application-values (slurp (str (fs/path (:directory c) "reference.stdout"))))) "Rebuilt jobs must perform the same application work as the reused compilation")
          (spit (str (fs/path (:directory c) "reference-rebuild.stdout")) rebuilt)))
      (let [scopes (if initialization? [:compiler] [:vm :compiler]) rounds (if (or (= mode "--check") allocations?) 1 5)
            revisions (fn [round] (if (and initialization? timing?) (if (even? round) [:predecessor :initialized] [:initialized :predecessor]) [nil]))]
        (doseq [c prepared scope scopes]
          (let [active cells]
            (println "Measure" (:id c) scope "cells" (count active) "rounds" rounds) (flush)
            (when timing? (spit (str sample-path) (str (pr-str (assoc (quiet!) :case (:id c) :scope scope)) "\n") :append true))
            (when timing? (doseq [cell active revision (revisions 0)] (invoke c scope (cond-> cell revision (assoc :revision revision)))))
            (doseq [round (range rounds) cell (let [order (vec active) n (count order) offset (mod round n) rotated (vec (concat (subvec order offset) (subvec order 0 offset)))] (if (even? round) rotated (reverse rotated))) revision (revisions round)
                    :let [cell (cond-> cell revision (assoc :revision revision))]]
              (let [row (assoc (invoke c scope cell) :kind (if allocations? :allocation :sample) :round (inc round))]
                (when (= scope :compiler) (assert (= (slurp (str (fs/path (:directory c) (if (= :rebuild (:control cell)) "reference-rebuild.stdout" "reference.stdout")))) (command env "node" (str (fs/path (:directory c) "actual.js")))) "Application output differs"))
                (spit (str sample-path) (str (pr-str row) "\n") :append true))))))
      (doseq [round (range (if initialization? 0 5)) :let [c (first prepared)]]
        (command (merge env {"GOOSE_BENCH_DIRECTORY" (:directory c) "GOOSE_BENCH_SCOPE" "vm" "GOOSE_BENCH_ENGINE" "storage" "GOOSE_BENCH_WORKERS" "1" "GOOSE_BENCH_CONTROL" "branch"}) binary "compiler_bootstrap::goose_capture::benchmark::final_benchmark" "--ignored" "--exact" "--test-threads=1")
        (spit (str sample-path) (str (pr-str {:kind :storage :round (inc round) :data (edn/read-string (slurp (str (fs/path (:directory c) "storage.edn"))))}) "\n") :append true))
      (spit (str sample-path) (str (pr-str {:kind :environment-after :load (str/trim (command environment "uptime")) :processes (str/split-lines (command environment "ps" "-axo" "pid,ppid,%cpu,comm"))}) "\n") :append true)
      (when (not= mode "--check") (command environment "gzip" "-n" "-f" (str sample-path)))
      (println "Saved" (str sample-path (when (not= mode "--check") ".gz"))))
    (finally (fs/delete-tree temporary)))))
