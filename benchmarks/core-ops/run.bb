#!/usr/bin/env bb
;; The core-operations gate: one ClojureScript source (src/core_ops/main.cljs)
;; built for release by light-cljs and by shadow-cljs, both outputs run in
;; Node, and each operation's nanoseconds compared. See README.md here.
;;
;;   bb benchmarks/core-ops/run.bb [--light BINARY] [--ops A,B] [--runs N]
;;                                 [--out PREFIX] [--baseline EARLIER.edn]
;;   bb benchmarks/core-ops/run.bb --pseudo-names OP [--light BINARY]
;;
;; Every run builds both outputs first (light-cljs.edn and shadow-cljs.edn,
;; build :bench). A run of an output is one Node process that times every
;; selected operation: one warm-up at a tenth of its iterations, then --rounds
;; timed rounds, of which it prints the median as ns per operation, with a
;; checksum. The compilers alternate, --runs processes each, and no run waits
;; for the machine: the 1-minute load average before each is recorded, as
;; benchmarks/generated-project/compare.bb's --runtime runs do (its helpers
;; are loaded from there). Every checksum of an operation must be the same in
;; every process of both compilers, or the run fails. Prints a table of the
;; medians over the runs, their ranges and light/shadow, and writes it to
;; PREFIX.md and every run's raw rows to PREFIX.edn.
;;
;; --pseudo-names OP builds the readable :profile builds instead and runs OP
;; once per compiler under `node --cpu-prof`, then prints where each spends
;; its time: the functions with the most self time, with their lines.
(require '[babashka.fs :as fs]
         '[babashka.process :as process]
         '[cheshire.core :as json]
         '[clojure.pprint :as pprint]
         '[clojure.string :as str])

(def here (fs/parent (fs/absolutize *file*)))
(def repo (fs/parent (fs/parent here)))

(load-file (str (fs/path repo "benchmarks/generated-project/compare.bb")))
(alias 'compare 'generated-project.compare)

(def usage
  (str/join
   "\n"
   ["Usage: bb benchmarks/core-ops/run.bb [options]"
    "  --light BINARY       light-cljs binary (LIGHT_CLJS_BINARY, else target/release/light-cljs)"
    "  --light-commit SHA   the commit the binary was built from (from a light-cljs-SHA name)"
    "  --ops A,B,...        only these operations (all); a name that is no operation's selects"
    "                       those it starts up to a hyphen (vec-conj: vec-conj-64)"
    "  --runs N             processes per compiler, alternated (5)"
    "  --rounds N           timed rounds per operation in a process, which prints their median (5)"
    "  --scale X            multiply every operation's iteration count by X (1)"
    "  --out PREFIX         write PREFIX.edn and PREFIX.md (out/core-ops-result)"
    "  --baseline FILE      an earlier run's EDN: also print each operation's change against it"
    "  --pseudo-names OP    profile OP instead: readable builds, one node --cpu-prof run per compiler"]))

(def defaults {:runs 5 :rounds 5 :scale 1.0 :seed 7 :top 25})

(defn- positive [parse what]
  (fn [text]
    (let [n (some-> text parse)]
      (if (and n (pos? n)) n (throw (ex-info (str "Expected " what ", got " (pr-str text)) {}))))))

(def ^:private absolute #(str (fs/absolutize %)))

(def value-options
  {"--light" [:light absolute]
   "--light-commit" [:light-commit identity]
   "--ops" [:ops #(vec (remove str/blank? (str/split % #",")))]
   "--runs" [:runs (positive parse-long "a positive integer")]
   "--rounds" [:rounds (positive parse-long "a positive integer")]
   "--scale" [:scale (positive parse-double "a positive number")]
   "--out" [:out absolute]
   "--baseline" [:baseline absolute]
   "--pseudo-names" [:pseudo-names identity]})

(defn parse-options [args]
  (loop [[arg & more] args options defaults]
    (cond
      (nil? arg) options
      (#{"-h" "--help"} arg) (do (println usage) (System/exit 0))
      (value-options arg) (let [[k parse] (value-options arg)]
                            (when (nil? (first more)) (throw (ex-info (str arg " needs a value\n" usage) {})))
                            (recur (rest more) (assoc options k (parse (first more)))))
      :else (throw (ex-info (str "Unknown argument " arg "\n" usage) {})))))

(defn light-binary [options]
  (let [binary (or (:light options) (System/getenv "LIGHT_CLJS_BINARY")
                   (str (fs/path repo "target/release/light-cljs")))]
    (when-not (fs/executable? binary)
      (throw (ex-info (str "No light-cljs binary at " binary "; build one or pass --light") {})))
    (absolute binary)))

(def harness-source (fs/path here "src/core_ops/main.cljs"))

(defn all-ops
  "The operations of the harness's `benches` table, in its order."
  []
  (vec (keep #(second (re-find #"^\s+\[\[?\"([^\"]+)\" b-" %)) (str/split-lines (slurp (str harness-source))))))

(defn selected-ops
  "The operations --ops (or --pseudo-names) names, in the harness's order. A
  name selects that operation, or else every one it starts up to a hyphen:
  vec-conj selects vec-conj-64, assoc-small both assoc-small operations."
  [options]
  (let [known (all-ops)
        asked (or (:ops options) (some-> (:pseudo-names options) vector))
        matches (fn [name] (if (some #{name} known) [name] (filter #(str/starts-with? % (str name "-")) known)))
        unknown (remove (comp seq matches) asked)
        selected (if asked (set (mapcat matches asked)) (set known))]
    (when (and (:ops options) (:pseudo-names options))
      (throw (ex-info "--pseudo-names names its operation itself; drop --ops" {})))
    (when (and asked (empty? asked))
      (throw (ex-info "--ops names no operation" {})))
    (when (seq unknown)
      (throw (ex-info (str "Unknown operations " (str/join ", " unknown) "; known: " (str/join ", " known)) {})))
    (when (and (:pseudo-names options) (not= 1 (count selected)))
      (throw (ex-info (str "--pseudo-names profiles one operation; " (:pseudo-names options) " selects "
                           (str/join ", " (filter selected known))) {})))
    (filterv selected known)))

;; Building

(def outputs
  {:bench {:light "out/light" :shadow "out/shadow.js"}
   :profile {:light "out/light-profile" :shadow "out/shadow-profile.js"}})

(defn sh!
  "Runs cmd in this directory and returns its stdout; fails with the end of
  its stderr."
  [opts & cmd]
  (let [r (apply process/sh (merge {:dir (str here)} opts) (map str cmd))]
    (when-not (zero? (:exit r))
      (let [err (str (:err r) (:out r))]
        (throw (ex-info (str "Failed: " (str/join " " cmd) "\n" (subs err (max 0 (- (count err) 3000)))) {}))))
    (:out r)))

(defn build!
  "Release builds of `build` (:bench or :profile) by both compilers, their old
  outputs removed first so a failed build is never measured."
  [light build]
  (let [{light-out :light shadow-out :shadow} (outputs build)]
    (fs/delete-tree (fs/path here light-out))
    (fs/delete-if-exists (fs/path here shadow-out))
    (compare/log "light-cljs release" (name build))
    (sh! {} light "release" (name build))
    ;; Node reads the ES module as one without guessing (compare.bb's generated projects do the same).
    (spit (str (fs/path here light-out "package.json")) "{\"type\":\"module\"}\n")
    (compare/log "shadow-cljs release" (name build))
    (apply sh! {} (concat (compare/default-shadow-cmd (str here)) ["release" (name build)]))))

;; Running

(defn parse-rows
  "{op {:ns-per-op x :checksum text}} from the harness's `op TAB ns TAB checksum` lines."
  [out]
  (into {} (for [line (str/split-lines out)
                 :let [[op ns-per-op checksum] (str/split line #"\t")]
                 :when checksum]
             [op {:ns-per-op (parse-double ns-per-op) :checksum checksum}])))

(defn run-output
  "One Node process of `tool`'s output over `ops`; node-flags go before the
  script (the profile mode's --cpu-prof)."
  [tool build {:keys [seed rounds scale]} ops all? node-flags]
  (let [load (compare/load-average)
        script (if (= tool :light) "run-light.mjs" (get-in outputs [build :shadow]))
        env (when (= tool :light) {"CORE_OPS_LIGHT_OUT" (get-in outputs [build :light])})
        out (apply sh! {:extra-env env}
                   (concat ["node"] node-flags [script seed rounds (if all? "all" (str/join "," ops)) scale]))
        rows (parse-rows out)
        missing (remove rows ops)]
    (when (seq missing)
      (throw (ex-info (str (name tool) " printed no result for " (str/join ", " missing)) {:out out})))
    {:tool tool :load load :rows rows}))

(defn check-checksums!
  "Fails unless every process printed one checksum per operation."
  [runs ops]
  (doseq [op ops
          :let [by-run (map (fn [r] [(:tool r) (get-in r [:rows op :checksum])]) runs)]]
    (when (< 1 (count (set (map second by-run))))
      (throw (ex-info (str "Checksums of " op " differ: " (pr-str by-run)) {})))))

(defn summarize [runs ops]
  (into {} (for [op ops
                 :let [ns-of (fn [tool] (keep #(when (= tool (:tool %)) (get-in % [:rows op :ns-per-op])) runs))
                       light (compare/spread (ns-of :light))
                       shadow (compare/spread (ns-of :shadow))]]
             [op {:light light :shadow shadow
                  :ratio (when (pos? (:median shadow)) (/ (:median light) (:median shadow)))
                  :checksum (get-in (first runs) [:rows op :checksum])}])))

;; Report

(defn ns-text [x]
  (let [x (double x)]
    (cond (< x 10) (format "%.2f" x) (< x 1000) (format "%.1f" x) :else (format "%,.0f" x))))

(defn range-text [{:keys [min max]}] (str (ns-text min) "-" (ns-text max)))

(defn ratio-text [r] (if r (format "%.2f" (double r)) "-"))

(defn table
  "The markdown table of summary over ops, with the change against baseline's
  summary when there is one."
  [summary ops baseline]
  (concat
   [(str "| op | light ns/op | shadow ns/op | light/shadow | light range | shadow range |"
         (when baseline " light before | change | light/shadow before |"))
    (str "| --- | ---: | ---: | ---: | ---: | ---: |" (when baseline " ---: | ---: | ---: |"))]
   (for [op ops
         :let [{:keys [light shadow ratio]} (summary op)
               before (get baseline op)]]
     (str "| " op " | " (ns-text (:median light)) " | " (ns-text (:median shadow)) " | " (ratio-text ratio)
          " | " (range-text light) " | " (range-text shadow) " |"
          (when baseline
            (if before
              (let [b (get-in before [:light :median])]
                (str " " (ns-text b) " | " (format "%+d%%" (Math/round (* 100 (- (/ (:median light) b) 1))))
                     " | " (ratio-text (:ratio before)) " |"))
              " - | - | - |"))))))

(defn notes [{:keys [runs rounds scale]} environment results ops baseline-file baseline]
  (let [loads (map (comp first :load) results)
        top (apply max loads)
        baseline-env (:environment baseline)]
    [(#'compare/environment-line environment)
     (str "Release builds (`:bench`), run as `node run-light.mjs` and `node out/shadow.js` with seed 7: " runs
          " runs per compiler, alternated; each run prints the median of " rounds " timed rounds per operation"
          (when (not= 1.0 (double scale)) (str " at " scale " times the iterations"))
          ", after one warm-up at a tenth of them. Medians over the runs, with their ranges."
          " Every operation's checksum was the same in every run of both outputs.")
     (str "No run waited for the machine: the 1-minute load average before a run was "
          (format "%.2f-%.2f" (double (apply min loads)) (double top))
          (when (> top 2.0) ", so these results are unreserved") ".")
     (when baseline-file
       (str "Change of light-cljs's median against `" baseline-file "`"
            (when-let [c (get-in baseline-env [:light-cljs :commit])] (str " (light-cljs `" c "`)")) "."
            (when (not= ops (:ops baseline))
              (str " That run selected other operations, and an operation's number depends on the ones that ran"
                   " before it in its process: compare runs of the same --ops."))
            (when (not= (select-keys (:options baseline) [:rounds :scale]) {:rounds rounds :scale scale})
              " That run used other --rounds or --scale.")))]))

;; Profiling

(defn profile-summary
  "The `top` functions by self time in a .cpuprofile, as share-of-samples lines."
  [file top]
  (let [p (json/parse-string (slurp (str file)) true)
        by-id (into {} (map (juxt :id identity)) (:nodes p))
        counts (frequencies (:samples p))
        total (reduce + (vals counts))
        self (reduce (fn [m [id c]]
                       (let [{:keys [functionName lineNumber url]} (:callFrame (by-id id))
                             k (str (if (str/blank? functionName) "(anonymous)" functionName)
                                    (when (seq url) (str " " (fs/file-name url) ":" (inc lineNumber))))]
                         (update m k (fnil + 0) c)))
                     {} counts)]
    (cons (str total " samples")
          (for [[k c] (take top (sort-by (comp - val) self))]
            (format "%5.1f%%  %s" (* 100.0 (/ c total)) k)))))

(defn profile! [options light op]
  (build! light :profile)
  (let [results (vec (for [tool [:light :shadow]
                           :let [dir (fs/path here "out/prof" op (name tool))]]
                       (do (fs/delete-tree dir)
                           (fs/create-dirs dir)
                           (compare/log "profiling" op (name tool))
                           (assoc (run-output tool :profile options [op] false
                                              ["--cpu-prof" "--cpu-prof-interval" "100" "--cpu-prof-dir" (str dir)])
                                  :profile (first (fs/glob dir "*.cpuprofile"))))))]
    (check-checksums! results [op])
    (doseq [{:keys [tool rows profile]} results]
      (println (str "## " (compare/tool-labels tool) ": " op " " (ns-text (get-in rows [op :ns-per-op]))
                    " ns/op (readable build, under the profiler)"))
      (println (str (fs/relativize repo profile)))
      (doseq [line (profile-summary profile (:top options))] (println line))
      (println))))

;; Main

(defn main [args]
  (let [options (parse-options args)
        light (light-binary options)
        ops (selected-ops options)]
    (if (:pseudo-names options)
      (profile! options light (first ops))
      (let [baseline-data (some-> (:baseline options) compare/read-edn)
            environment (compare/environment (assoc options :light light)
                                             (compare/find-version (compare/read-edn (fs/file here "deps.edn"))))
            _ (build! light :bench)
            all? (nil? (:ops options))
            results (vec (for [run (range (:runs options))
                               tool (if (even? run) [:light :shadow] [:shadow :light])]
                           (let [r (run-output tool :bench options ops all? [])]
                             (compare/log "run" (inc run) "of" (:runs options) (name tool)
                                          "load" (first (:load r)))
                             r)))
            _ (check-checksums! results ops)
            summary (summarize results ops)
            prefix (or (:out options) (str (fs/path here "out/core-ops-result")))
            report (str/join "\n" (concat (table summary ops (:summary baseline-data)) [""]
                                          (keep identity (notes options environment results ops (:baseline options)
                                                                baseline-data))
                                          [""]))]
        (fs/create-dirs (fs/parent prefix))
        (spit (str prefix ".edn")
              (with-out-str (pprint/pprint {:environment environment
                                            :options (dissoc options :light)
                                            :ops ops :runs results :summary summary})))
        (spit (str prefix ".md") report)
        (println report)
        (println "Wrote" (str prefix ".edn") "and" (str prefix ".md"))))))

(when (= (fs/absolutize *file*)
         (some-> (System/getProperty "babashka.file") fs/absolutize))
  (main *command-line-args*))
