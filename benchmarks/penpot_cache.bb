#!/usr/bin/env bb
;; The persistent compiler cache on the Penpot port (light-cljs-328,
;; docs/compiler-cache.md, "Validation and performance gates"), companion to
;; penpot.bb and with its flags. Build the release binary first.
;; Usage: bb benchmarks/penpot_cache.bb OUTPUT-DIR ROUNDS [SCENARIO ...]
;; Each round runs the named scenarios (default: all but watch) in this order:
;;   off        app.main --optimize none (penpot.bb main-none), cache off
;;   populate   the same into an empty cache directory
;;   warm       again on that directory, nothing edited
;;   leaf       warm after editing the leaf namespace below (a comment)
;;   macro      warm after editing the macro and helper namespace below
;;   resource   warm after editing a resource a light.build macro inlines,
;;              through a fixture entry in OUTPUT-DIR that requires app.main
;;   config     warm after adding a --config value
;;   resolution warm after adding a .cljs file shadowing a .cljc namespace
;;   advanced   --optimize advanced (penpot.bb main-advanced): cache off,
;;              populating, then warm
;;   watch      --watch: populating start, warm start, then STEPS saves of
;;              the leaf (LIGHT_CLJS_WATCH_STEPS, default 3) with peak RSS
;; Edit scenarios run on an APFS clone of the warm directory, so each round
;; starts from the same cache, and the port is restored after every edit.
;; Every run has --stats; each appends a record to OUTPUT-DIR/results.edn
;; with its wall, CPU, peak RSS, cache directory size, uptime load, output
;; SHA-256 and the report's counters. LIGHT_CLJS_BINARY selects an
;; immutable binary; a binary whose usage names --no-cache gets it for off.
(require '[babashka.fs :as fs]
         '[babashka.process :as process]
         '[clojure.string :as str])

(def repo (fs/parent (fs/parent (fs/absolutize *file*))))
(def output (some-> (first *command-line-args*) fs/absolutize))
(def rounds (some-> (second *command-line-args*) parse-long))
(when-not (and output rounds) (throw (ex-info "Usage: OUTPUT-DIR ROUNDS [SCENARIO ...]" {})))
(fs/create-dirs output)
(def port (fs/path repo "research/penpot-light"))
(def binary (or (System/getenv "LIGHT_CLJS_BINARY") (str (fs/path repo "target/release/light-cljs"))))
(def limits ["--macro-expansions" "100000000" "--macro-nodes" "4000000000"
             "--macro-timeout-ms" "3600000" "--macro-memory-limit" "4000000000"
             "--macro-poll-budget" "1000000000000"])
(def roots (mapcat #(vector "--source-root" (str (fs/path port %)))
                   ["frontend/src" "common/src" "vendor" "generated" "shims"]))
(def no-cache (if (str/includes? (:out (process/sh binary "--help")) "--no-cache") ["--no-cache"] []))
(def scenarios (or (seq (drop 2 *command-line-args*))
                   ["off" "populate" "warm" "leaf" "macro" "resource" "config" "resolution" "advanced"]))
(def watch-steps (or (some-> (System/getenv "LIGHT_CLJS_WATCH_STEPS") parse-long) 3))

(def leaf "frontend/src/app/main/ui/workspace/palette.cljs") ; app.main.ui.workspace.palette, one consumer
(def macro "common/src/app/common/exceptions.cljc")          ; app.common.exceptions, macros and helpers
(def shadowed "common/src/app/common/geom/line.cljc")        ; app.common.geom.line, shadowed by line.cljs
(def fixture (fs/path output "fixture"))

(defn load-average [] (second (re-find #"load averages?: (.*)$" (str/trim (:out (process/sh "uptime"))))))
(defn sha256 [file] (when (fs/exists? file) (first (str/split (:out (process/sh "shasum" "-a" "256" (str file))) #" "))))
(defn disk-bytes [dir] (when (and dir (fs/exists? dir)) (* 1024 (parse-long (first (str/split (:out (process/sh "du" "-sk" (str dir))) #"\s"))))))
(defn counters
  "Every `key number` line of a --stats report."
  [stderr]
  (into (sorted-map)
        (for [[_ key value] (re-seq #"(?m)^([a-z][a-z0-9.-]*) (-?[0-9]+(?:\.[0-9]+)?)$" stderr)]
          [(keyword key) (parse-double value)])))
(defn clone [from to] (fs/delete-tree to) (process/shell "cp" "-cR" (str from) (str to)) to)

(defn with-edit
  "Calls f with `relative` in the port changed by `change` (text to text,
  nil deletes it), then restores the file as it was."
  [relative change f]
  (let [file (fs/file port relative)
        before (when (fs/exists? file) (slurp file))]
    (try
      (if-let [text (change before)] (spit file text) (fs/delete-if-exists file))
      (f)
      (finally (if before (spit file before) (fs/delete-if-exists file))))))

(defn compile!
  "One --compile run as `scenario`, recorded."
  [round scenario {:keys [entry args cache roots-extra]}]
  (let [js (fs/file output (str scenario ".js"))
        err (fs/file output (str scenario ".err.txt"))
        load (load-average)
        command (concat ["/usr/bin/time" "-l" binary "--compile" "--esm"] limits roots roots-extra
                        ["--entry" (or entry "app.main") "--stats"]
                        ;; The level is explicit: the compiler's default is prepare.
                        (when-not (some #{"--optimize"} args) ["--optimize" "none"])
                        (if cache ["--cache-dir" (str cache)] no-cache) args)
        result (apply process/shell {:dir (str repo) :out js :err err :continue true} (map str command))
        text (slurp err)
        [_ real user system] (re-find #"([0-9.]+)\s+real\s+([0-9.]+)\s+user\s+([0-9.]+)\s+sys" text)
        stats (counters text)
        record {:round round :scenario scenario :exit (:exit result) :load load
                :wall-seconds (some-> real parse-double) :user-seconds (some-> user parse-double)
                :system-seconds (some-> system parse-double)
                :peak-rss-bytes (some-> (re-find #"(\d+)\s+maximum resident set size" text) second parse-long)
                :output-bytes (fs/size js) :output-sha256 (sha256 js) :cache-bytes (disk-bytes cache)
                :hits (:cache.hits stats) :misses (:cache.misses stats) :bypasses (:cache.bypasses stats)
                :macro-calls (:macro-calls stats) :stats stats
                :misses-table (str/join "\n" (take 12 (rest (drop-while #(not= % "[cache-misses-and-bypasses]") (str/split-lines text)))))}]
    (spit (fs/file output "results.edn") (str (pr-str record) "\n") :append true)
    (prn (dissoc record :stats :misses-table))
    (flush)
    (when-not (zero? (:exit result))
      (throw (ex-info (str "Benchmark failed; see " err) (dissoc record :stats))))
    (fs/delete-if-exists js)
    record))

(defn append-comment [round] (fn [text] (str text "\n;; light-cljs-328 benchmark edit, round " round "\n")))

(defn watch!
  "A --watch run on `cache`: its startup, then `steps` saves of the leaf,
  each waited for, with the process's RSS after each."
  [round scenario cache steps]
  (let [out-dir (fs/path output "watch-out")
        err (fs/file output (str scenario ".err.txt"))
        _ (fs/delete-tree out-dir)
        load (load-average)
        started (System/nanoTime)
        proc (apply process/process {:dir (str repo) :out :discard :err err}
                              (map str (concat [binary "--watch"] limits roots
                                               ["--entry" "app.main" "--out-dir" out-dir "--stats" "--poll-ms" "100"
                                                "--cache-dir" cache])))
        pid (.pid (:proc proc))
        text #(slurp err)
        rss #(some-> (:out (process/sh "ps" "-o" "rss=" "-p" (str pid))) str/trim parse-long (* 1024))
        wait-for (fn [pred]
                   (loop [] (let [t (text)]
                              (cond (pred t) t
                                    (not (.isAlive (:proc proc))) (throw (ex-info "watch exited" {:err (str err)}))
                                    :else (do (Thread/sleep 100) (recur))))))
        count-steps #(count (re-seq #"(?m)^light-cljs: (recompiled|.*error)" %))]
    (try
      (wait-for #(str/includes? % "; watching every"))
      (let [startup-ms (/ (- (System/nanoTime) started) 1e6)
            startup-rss (rss)
            samples (with-edit leaf identity
                      (fn []
                        (vec (for [step (range 1 (inc steps))]
                               (let [before (count-steps (text))]
                                 (Thread/sleep 300)
                                 (spit (fs/file port leaf) (str "\n;; light-cljs-328 watch edit " round " " step "\n") :append true)
                                 (let [t (wait-for #(> (count-steps %) before))
                                       line (last (re-seq #"(?m)^light-cljs: (?:recompiled|.*error).*$" t))]
                                   {:step step :line line :rss (rss)
                                    :ms (some-> (re-find #"in ([0-9.]+) ms" line) second parse-double)}))))))
            stats (counters (text))
            record {:round round :scenario scenario :load load :startup-ms startup-ms :startup-rss startup-rss
                    :cache-bytes (disk-bytes cache) :hits (:cache.hits stats) :misses (:cache.misses stats)
                    :macro-calls (:macro-calls stats) :session-load-ms (:time.session-load-ms stats)
                    :build-line (re-find #"(?m)^light-cljs: built .*$" (text))
                    :steps samples :stats stats}]
        (spit (fs/file output "results.edn") (str (pr-str record) "\n") :append true)
        (prn (dissoc record :stats))
        (flush)
        record)
      (finally
        (process/destroy-tree proc)
        @proc
        (fs/delete-tree out-dir)))))

(defn write-fixture [text]
  (fs/create-dirs (fs/path fixture "light_cljs_bench"))
  (spit (fs/file fixture "light_cljs_bench/inputs.cljs")
        "(ns light-cljs-bench.inputs (:require [light.build :as b]))\n(defmacro inline [path] (b/resource path))\n")
  (spit (fs/file fixture "light_cljs_bench/main.cljs")
        "(ns light-cljs-bench.main (:require [app.main] [light-cljs-bench.inputs :refer [inline]]))\n(def text (inline \"light_cljs_bench/data.txt\"))\n")
  (spit (fs/file fixture "light_cljs_bench/data.txt") text))

(doseq [round (range 1 (inc rounds))]
  (let [warm (fs/path output "cache-warm")
        edit (fs/path output "cache-edit")
        selected (set scenarios)
        run (fn [scenario options] (compile! round scenario options))]
    (when (selected "off") (run "off" {}))
    (when (or (selected "populate") (selected "warm"))
      (fs/delete-tree warm)
      (run "populate" {:cache warm})
      (run "warm" {:cache warm}))
    (when (selected "leaf")
      (with-edit leaf (append-comment round) #(run "leaf" {:cache (clone warm edit)})))
    (when (selected "macro")
      (with-edit macro (append-comment round) #(run "macro" {:cache (clone warm edit)})))
    (when (selected "resource")
      (let [options {:cache (clone warm edit) :entry "light-cljs-bench.main" :roots-extra ["--source-root" fixture]}]
        (write-fixture "first")
        (run "resource-prime" options)
        (write-fixture (str "edited in round " round))
        (run "resource" options)))
    (when (selected "config")
      (run "config" {:cache (clone warm edit) :args ["--config" (str "light-cljs-bench/round=" round)]}))
    (when (selected "resolution")
      (let [target (str/replace shadowed #"\.cljc$" ".cljs")
            text (slurp (fs/file port shadowed))]
        (when (fs/exists? (fs/file port target)) (throw (ex-info "The shadowing file already exists" {:target target})))
        (with-edit target (constantly text) #(run "resolution" {:cache (clone warm edit)}))))
    (when (selected "advanced")
      (let [args ["--optimize" "advanced" "--pseudo-names" "--elide-asserts"]
            advanced (fs/path output "cache-advanced")]
        (fs/delete-tree advanced)
        (run "advanced-off" {:args args})
        (run "advanced-populate" {:cache advanced :args args})
        (run "advanced-warm" {:cache advanced :args args})
        (fs/delete-tree advanced)))
    (when (selected "watch")
      (let [watch (fs/path output "cache-watch")]
        (fs/delete-tree watch)
        (watch! round "watch-populate" watch 0)
        (watch! round "watch-warm" watch watch-steps)
        (fs/delete-tree watch)))
    (fs/delete-tree edit)))
