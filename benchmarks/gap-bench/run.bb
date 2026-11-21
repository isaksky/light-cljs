#!/usr/bin/env bb
;; Unified light-cljs vs shadow-cljs gap-closing benchmark suite.
;;
;; Measures all key axes in <= 8 minutes (full head-to-head) or ~2-3 minutes
;; (--reuse-shadow, default once a shadow baseline exists, while still running
;; live alternated runtime benchmarks against saved shadow JS artifacts):
;;
;;   1. compile : re-frame-200k clean (+ determinism hash, oracle, waste),
;;                cold cache-fill (+ overhead & disk size), warm unchanged,
;;                warm leaf-body edit, warm declaration edit, --stats phase
;;                breakdown, 50k -> 200k scaling exponent, shadow capped at
;;                -Xmx1536m.
;;   2. watch   : re-frame-200k in headless Chromium via watch-vs-shadow.mjs
;;                (startup, CPU, dev page load, memory, 0 errors, leaf/common/
;;                macro edits with jitter + per-stage breakdown).
;;   3. size    : simple, todomvc, flow, re-frame-200k (raw, gzip -9, brotli
;;                -q 11, whole bundle vs app-only, todomvc ES2015-lowered,
;;                --size-report runtime slice & constant pool).
;;   4. runtime : re-frame-200k startup phase breakdown (startup-probe.mjs),
;;                re-frame-200k Node throughput & peak RSS (1 & 10 iters),
;;                TodoMVC Chromium 9/9 behavior checks + 1,000-todo runtime &
;;                CDP JS heap, core-ops (76 ops), extend-ops (16 ops).
;;
;; Usage:
;;   bb benchmarks/gap-bench/run.bb [options]
(ns gap-bench.run
  (:require [babashka.fs :as fs]
            [babashka.process :as process]
            [cheshire.core :as json]
            [clojure.edn :as edn]
            [clojure.pprint :as pprint]
            [clojure.string :as str]
            [edamame.core :as edamame]))

(def here (fs/parent (fs/absolutize *file*)))
(def repo (fs/parent (fs/parent here)))

(load-file (str (fs/path repo "benchmarks/generated-project/compare.bb")))
(alias 'cmp 'generated-project.compare)

(def all-slices [:compile :watch :size :runtime])
(def shadow-version-pin "3.5.3")
(def esbuild-version "0.25.10")
(def playwright-version "1.64.1")
(def shadow-watch-port 9687)
(def re-frame-leaf "src/workload/besmeik.cljs")
(def runtime-seed 42)
(def runtime-iters [1 6])
(def ops-rounds 3)
(def ops-scale 0.25)
(def gap-threshold 1.05)

(def usage
  (str/join
   "\n"
   ["Usage: bb benchmarks/gap-bench/run.bb [options]"
    "  --light BINARY          light-cljs binary (LIGHT_CLJS_BINARY, else target/release/light-cljs)"
    "  --light-commit SHA      commit the binary was built from (parsed from light-cljs-SHA or git HEAD)"
    "  --only SLICES           comma-separated subset of compile,watch,size,runtime (all)"
    "  --fresh-shadow          run shadow-cljs fresh for selected slices and update shadow baseline"
    "  --reuse-shadow          require cached shadow baseline for slow shadow builds/watch/size"
    "                          (default: auto-reuse per slice when present, else run fresh)"
    "  --shadow-baseline FILE  shadow baseline EDN path (<work-dir>/shadow-baseline.edn)"
    "  --baseline FILE         earlier gap-bench EDN to diff against"
    "  --research-dir DIR      read-only research/ dir (auto-detected from repo or main checkout)"
    "  --work-dir DIR          isolated mutable staging dir (benchmarks/gap-bench/work)"
    "  --out PREFIX            write PREFIX.edn and PREFIX.md (benchmarks/gap-bench/out/gap-bench-COMMIT)"
    "  --runs N                alternated runtime runs per compiler (3)"
    "  --compile-runs N        repeated light-cljs clean/warm-unchanged compile runs (2)"
    "  --watch-edits N         timed edits per watch scenario after 1 warm-up (2)"
    "  --todos N               TodoMVC browser runtime todo count (1000)"
    "  --shadow-heap SIZE      shadow-cljs JVM -Xmx cap at 200k (1536m)"
    "  --skip-browser          skip headless Chromium steps (watch + TodoMVC browser checks/runtime)"
    "  -h, --help              show this help"]))

(def defaults
  {:only all-slices
   :shadow-mode :auto
   :runs 3
   :compile-runs 2
   :watch-edits 2
   :todos 1000
   :shadow-heap "1536m"
   :skip-browser false})

(defn- abs-str [p] (str (fs/absolutize p)))

(defn- pos-int [text flag]
  (let [n (some-> text parse-long)]
    (if (and n (pos? n)) n (throw (ex-info (str flag " expects a positive integer, got " (pr-str text)) {})))))

(defn parse-slices [text]
  (let [xs (mapv #(keyword (str/trim %)) (remove str/blank? (str/split text #",")))
        bad (remove (set all-slices) xs)]
    (when (empty? xs) (throw (ex-info "--only requires at least one slice" {})))
    (when (seq bad)
      (throw (ex-info (str "Unknown slice(s): " (str/join ", " (map name bad))
                           "; valid: " (str/join ", " (map name all-slices))) {})))
    (vec (distinct xs))))

(defn parse-options [args]
  (loop [[arg & more] args opts defaults]
    (cond
      (nil? arg) opts
      (#{"-h" "--help"} arg) (assoc opts :help true)
      (= "--fresh-shadow" arg) (recur more (assoc opts :shadow-mode :fresh))
      (= "--reuse-shadow" arg) (recur more (assoc opts :shadow-mode :reuse))
      (= "--skip-browser" arg) (recur more (assoc opts :skip-browser true))
      (#{"--light" "--light-commit" "--only" "--shadow-baseline" "--baseline"
         "--research-dir" "--work-dir" "--out" "--runs" "--compile-runs"
         "--watch-edits" "--todos" "--shadow-heap"} arg)
      (let [v (first more)]
        (when (nil? v) (throw (ex-info (str arg " needs a value\n" usage) {})))
        (recur (rest more)
               (case arg
                 "--light" (assoc opts :light (abs-str v))
                 "--light-commit" (assoc opts :light-commit v)
                 "--only" (assoc opts :only (parse-slices v))
                 "--shadow-baseline" (assoc opts :shadow-baseline (abs-str v))
                 "--baseline" (assoc opts :baseline (abs-str v))
                 "--research-dir" (assoc opts :research-dir (abs-str v))
                 "--work-dir" (assoc opts :work-dir (abs-str v))
                 "--out" (assoc opts :out (abs-str v))
                 "--runs" (assoc opts :runs (pos-int v arg))
                 "--compile-runs" (assoc opts :compile-runs (pos-int v arg))
                 "--watch-edits" (assoc opts :watch-edits (pos-int v arg))
                 "--todos" (assoc opts :todos (pos-int v arg))
                 "--shadow-heap"
                 (if (re-matches #"[0-9]+[kKmMgG]" v)
                   (assoc opts :shadow-heap v)
                   (throw (ex-info (str "--shadow-heap expects JVM size like 1536m, got " (pr-str v)) {}))))))
      :else (throw (ex-info (str "Unknown argument: " arg "\n" usage) {})))))

;; ---------------------------------------------------------------------------
;; Pure parsers, math, and formatting helpers (unit-tested in test.bb)

(defn parse-size-report
  "Parse light-cljs --size-report stderr lines into runtime slice and constant pool maps."
  [text]
  (let [rt (re-find #"runtime slice,\s*([\d,]+)\s+of\s+([\d,]+)\s+units,\s*([\d,]+)\s+unit bytes,\s*([\d,]+)\s+runtime bytes"
                    (or text ""))
        cp (re-find #"constant pool:\s*([\d,]+)\s+entries\s*\(([\d,]+)\s+composite,\s*([\d,]+)\s+lazy\),\s*([\d,]+)\s+bytes"
                    (or text ""))
        p #(parse-long (str/replace % "," ""))]
    (when (or rt cp)
      (cond-> {}
        rt (merge {:units-kept (p (nth rt 1))
                   :units-total (p (nth rt 2))
                   :unit-bytes (p (nth rt 3))
                   :runtime-bytes (p (nth rt 4))})
        cp (merge {:const-entries (p (nth cp 1))
                   :const-composite (p (nth cp 2))
                   :const-lazy (p (nth cp 3))
                   :const-bytes (p (nth cp 4))})))))

(defn scaling-exponent
  "Log-log scaling exponent between two sizes: log(y2/y1) / log(x2/x1). 1.0 is linear."
  [x1 y1 x2 y2]
  (when (and x1 y1 x2 y2 (pos? x1) (pos? y1) (pos? x2) (pos? y2) (not= x1 x2))
    (/ (Math/log (/ (double y2) (double y1)))
       (Math/log (/ (double x2) (double x1))))))

(defn geomean
  "Geometric mean of positive numbers."
  [xs]
  (let [ps (filter #(and % (pos? %)) xs)]
    (when (seq ps)
      (Math/exp (/ (reduce + 0.0 (map #(Math/log (double %)) ps)) (count ps))))))

(defn ratio
  "a / b when both are numbers and b > 0, else nil."
  [a b]
  (when (and (number? a) (number? b) (pos? b))
    (/ (double a) (double b))))

(defn pct-delta
  "Percentage change (now - before) / before * 100, or nil."
  [now before]
  (when (and (number? now) (number? before) (pos? before))
    (* 100.0 (/ (- (double now) (double before)) (double before)))))

(defn fmt-num
  ([x digits] (if (number? x) (format (str "%." digits "f") (double x)) "-"))
  ([x digits unit] (if (number? x) (str (format (str "%." digits "f") (double x)) " " unit) "-")))

(defn fmt-int [x] (if (number? x) (format "%,d" (Math/round (double x))) "-"))

(defn fmt-bytes-kb [b]
  (if (number? b) (format "%.1f KB" (/ (double b) 1000.0)) "-"))

(defn fmt-mb [gb-or-bytes mode]
  (if (number? gb-or-bytes)
    (case mode
      :from-gb (format "%.0f MB" (* 1000.0 (double gb-or-bytes)))
      :from-bytes (format "%.1f MB" (/ (double gb-or-bytes) 1e6)))
    "-"))

(defn fmt-ratio [r]
  (if (number? r)
    (let [r (double r)]
      (format (if (< r 0.1) "%.3fx" "%.2fx") r))
    "-"))

(defn fmt-delta [now before]
  (if-let [d (pct-delta now before)]
    (format "%+.1f%%" d)
    "-"))

(defn fmt-ms [x]
  (if (number? x)
    (let [x (double x)]
      (cond (< x 10) (format "%.2f ms" x)
            (< x 1000) (format "%.1f ms" x)
            :else (format "%,.0f ms" x)))
    "-"))

(defn fmt-ns [x]
  (if (number? x)
    (let [x (double x)]
      (cond (< x 10) (format "%.2f" x)
            (< x 1000) (format "%.1f" x)
            :else (format "%,.0f" x)))
    "-"))

(defn core-op-group
  "Coarse operation family for core-ops summary table."
  [op]
  (cond
    (re-find #"^(kw|get|contains|count|assoc|dissoc|update|map-literal|merge|select-keys|keys|vals|reduce-kv|reduce-entries)-" op) "maps"
    (re-find #"^(vec|destructure|into-vec)" op) "vectors & destructuring"
    (re-find #"^(reduce|filter|into-xform|mapv|lazy-walk|seq-walk|group-by|sort-by)" op) "seqs & transducers"
    (re-find #"^(transient|sorted-map|disj|into-set|list-pop)" op) "transients, sets & sorted"
    (re-find #"^(str|eq|hash)-" op) "str, = & hash"
    (re-find #"^(protocol|record|multimethod)" op) "protocols & records"
    (re-find #"^(multi-arity|variadic|apply|unknown-fn|closure)" op) "functions & calls"
    (re-find #"^swap-" op) "atoms"
    :else "loop baseline"))

(defn summarize-ops-runs
  "Summarize alternated microbenchmark runs into per-op medians, geomean, parity count, and groups."
  [runs ops group-fn]
  (let [by-op (into {}
                    (for [op ops
                          :let [ns-of (fn [t] (keep #(when (= t (:tool %)) (get-in % [:rows op :ns-per-op])) runs))
                                l (cmp/spread (ns-of :light))
                                s (cmp/spread (ns-of :shadow))
                                r (ratio (:median l) (:median s))]]
                      [op {:light l :shadow s :ratio r
                           :checksum (get-in (first runs) [:rows op :checksum])}]))
        ratios (keep (comp :ratio val) by-op)
        parity (count (filter #(<= % gap-threshold) ratios))
        slower (vec (->> by-op
                         (keep (fn [[op v]] (when (and (:ratio v) (> (:ratio v) gap-threshold)) (assoc v :op op))))
                         (sort-by (comp - :ratio))))
        groups (when group-fn
                 (vec (for [[g g-ops] (sort-by key (group-by group-fn ops))
                            :let [rs (keep #(get-in by-op [% :ratio]) g-ops)]]
                        {:group g
                         :count (count g-ops)
                         :parity (count (filter #(<= % gap-threshold) rs))
                         :geomean (geomean rs)
                         :max-ratio (when (seq rs) (apply max rs))})))]
    {:ops (vec ops)
     :by-op by-op
     :count (count ops)
     :parity-count parity
     :geomean (geomean ratios)
     :median-ratio (when (seq ratios) (cmp/median ratios))
     :top-slower (vec (take 10 slower))
     :all-slower slower
     :groups groups
     :load-1m (cmp/spread (map (comp first :load) runs))}))

;; ---------------------------------------------------------------------------
;; System / filesystem / process helpers

(defn now-ms [] (/ (System/nanoTime) 1e6))
(defn epoch-ms [] (double (System/currentTimeMillis)))

(defn sh!
  [opts & cmd]
  (let [r (apply process/sh (merge {:err :string :out :string} opts) (map str cmd))]
    (when-not (zero? (:exit r))
      (let [tail (str (:err r) "\n" (:out r))]
        (throw (ex-info (str "Command failed: " (str/join " " cmd) "\n"
                             (subs tail (max 0 (- (count tail) 3000))))
                        {:cmd cmd :exit (:exit r)}))))
    r))

(defn sha256-file [path]
  (let [digest (java.security.MessageDigest/getInstance "SHA-256")]
    (with-open [in (java.io.FileInputStream. (str path))]
      (let [buf (byte-array 65536)]
        (loop []
          (let [n (.read in buf)]
            (when (pos? n) (.update digest buf 0 n) (recur))))))
    (apply str (map #(format "%02x" %) (.digest digest)))))

(defn dir-size-bytes [dir]
  (if (fs/exists? dir)
    (reduce + 0 (for [p (fs/glob dir "**") :when (fs/regular-file? p)] (fs/size p)))
    0))

(defn compressed-sizes [file]
  (let [f (fs/file file)]
    {:raw (fs/size f)
     :gzip (count (:out (process/sh {:in f :out :bytes} "gzip" "-9" "-c")))
     :brotli (count (:out (process/sh {:in f :out :bytes} "brotli" "-q" "11" "-c")))}))

(defn minus-sizes [a b] (merge-with - a b))

(defn package-imports [file]
  (->> (re-seq #"\b(?:from|import)\s*\"(@?[a-z0-9][\w.-]*(?:/[\w.-]+)*)\"" (slurp (fs/file file)))
       (map second) distinct sort vec))

(defn shadow-packages [manifest]
  (->> (edn/read-string (slurp (fs/file manifest)))
       (mapcat :sources)
       (filter #(str/starts-with? (str %) "node_modules/"))
       sort vec))

(defn git-common-root []
  (try
    (let [r (process/sh {:dir (str repo) :err :string :out :string} "git" "rev-parse" "--git-common-dir")]
      (when (zero? (:exit r))
        (let [gd (fs/path repo (str/trim (:out r)))]
          (fs/parent (fs/canonicalize gd)))))
    (catch Exception _ nil)))

(defn git-head-short []
  (try
    (let [r (process/sh {:dir (str repo) :err :string :out :string} "git" "rev-parse" "--short=8" "HEAD")]
      (when (zero? (:exit r)) (str/trim (:out r))))
    (catch Exception _ nil)))

(defn resolve-research-dir [opts]
  (let [candidates (remove nil?
                           [(:research-dir opts)
                            (str (fs/path repo "research"))
                            (when-let [root (git-common-root)] (str (fs/path root "research")))])
        found (first (filter #(fs/exists? (fs/path % "shadow-credibility/projects/re-frame-200k-seed42")) candidates))]
    (or found
        (throw (ex-info (str "Could not locate read-only research/ directory with re-frame-200k-seed42. Checked: "
                             (str/join ", " candidates)
                             ". Pass --research-dir DIR.") {})))))

(defn resolve-light-binary [opts]
  (let [bin (or (:light opts) (System/getenv "LIGHT_CLJS_BINARY")
                (str (fs/path repo "target/release/light-cljs")))]
    (when-not (fs/executable? bin)
      (throw (ex-info (str "No executable light-cljs binary at " bin "; build one or pass --light BIN") {})))
    (abs-str bin)))

(defn resolve-port-source [research-dir app]
  (let [candidates (case app
                     "simple" ["shadow-credibility/simple-port-973" "shadow-credibility/simple-port" "simple-port"]
                     "todomvc" ["shadow-credibility/todomvc-port-980" "shadow-credibility/todomvc-port-973" "todomvc-port"]
                     "flow" ["shadow-credibility/flow-port-980" "shadow-credibility/flow-port" "flow-port"])]
    (or (first (for [rel candidates :let [p (fs/path research-dir rel)] :when (fs/exists? (fs/path p "light-cljs.edn"))] p))
        (throw (ex-info (str "Could not find pre-ported " app " under " research-dir) {:candidates candidates})))))

;; ---------------------------------------------------------------------------
;; Idempotent staging into isolated <work-dir> (never mutates research/)

(defn copy-project-without-artifacts!
  "Copy src-dir to dst-dir, skipping build caches/outputs and symlinking node_modules."
  [src-dir dst-dir]
  (let [skip #{".cpcache" ".shadow-cljs" ".light-cljs" ".compare-cache" ".gap-cache" "out" "node_modules"}]
    (fs/delete-tree dst-dir)
    (fs/create-dirs dst-dir)
    (doseq [entry (fs/list-dir src-dir)
            :let [nm (fs/file-name entry)
                  target (fs/path dst-dir nm)]
            :when (not (skip nm))]
      (if (fs/directory? entry)
        (fs/copy-tree entry target)
        (fs/copy entry target)))
    (let [nm (fs/path src-dir "node_modules")]
      (when (fs/exists? nm)
        (fs/create-sym-link (fs/path dst-dir "node_modules") (fs/canonicalize nm))))))

(defn ensure-npm-tools! [research-dir work-dir]
  (let [existing (fs/path research-dir "npm-tools")]
    (if (and (fs/exists? (fs/path existing "node_modules/.bin/esbuild"))
             (fs/exists? (fs/path existing "node_modules/playwright-core/package.json")))
      (str existing)
      (let [dst (fs/path work-dir "npm-tools")]
        (when-not (and (fs/exists? (fs/path dst "node_modules/.bin/esbuild"))
                       (fs/exists? (fs/path dst "node_modules/playwright-core/package.json")))
          (cmp/log "staging npm-tools (esbuild + playwright-core) in" (str dst))
          (fs/create-dirs dst)
          (sh! {:dir (str dst)} "npm" "install" "--no-audit" "--no-fund"
               (str "esbuild@" esbuild-version) (str "playwright-core@" playwright-version)))
        (str dst)))))

(defn ensure-generated-staged! [src-dir dst-dir]
  (let [marker (fs/path dst-dir ".gap-staged")
        src-str (str (fs/canonicalize src-dir))]
    (when-not (and (fs/exists? marker) (= src-str (str/trim (slurp (fs/file marker)))))
      (cmp/log "staging generated project" (fs/file-name dst-dir) "from" src-str)
      (copy-project-without-artifacts! src-dir dst-dir)
      (spit (fs/file marker) (str src-str "\n")))
    (str dst-dir)))

(def light-watch-loader
  (str "<script src=\"packages.js\"></script>\n"
       "<script src=\"index.js\"></script>\n"
       "<script>globalThis.__lightDevLoaded.then((start) => start());</script>\n"))

(defn build-watch-packages! [esbuild node-modules-dir out-file]
  (let [entry (fs/path node-modules-dir ".gap-watch-packages.mjs")]
    (spit (fs/file entry)
          (str "import * as react from \"react\";\n"
               "import * as reactDom from \"react-dom\";\n"
               "import * as reactDomClient from \"react-dom/client\";\n"
               "globalThis.lightCljsPackages = { \"react\": react, \"react-dom\": reactDom, \"react-dom/client\": reactDomClient };\n"))
    (try
      (sh! {:dir (str node-modules-dir)} esbuild (str entry) "--bundle" "--format=iife"
           "--define:process.env.NODE_ENV=\"development\"" (str "--outfile=" out-file) "--log-level=warning")
      (finally (fs/delete-if-exists entry)))))

(defn ensure-watch-staged! [src-dir dst-dir esbuild]
  (let [marker (fs/path dst-dir ".gap-watch-staged")
        src-str (str (fs/canonicalize src-dir) "|" shadow-watch-port)]
    (when-not (and (fs/exists? marker)
                   (= src-str (str/trim (slurp (fs/file marker))))
                   (fs/exists? (fs/path dst-dir "out/light-watch/packages.js")))
      (cmp/log "staging watch project" (str dst-dir))
      (copy-project-without-artifacts! src-dir dst-dir)
      (spit (fs/file dst-dir "shadow-cljs.edn")
            (pr-str {:deps {:aliases [:shadow]}
                     :dev-http {shadow-watch-port "out/shadow-watch"}
                     :builds {:watch {:target :browser
                                      :output-dir "out/shadow-watch/js"
                                      :asset-path "/js"
                                      :modules {:main {:init-fn 'workload.main/main}}
                                      :compiler-options {:warnings {:invalid-arithmetic false}}}}}))
      (fs/create-dirs (fs/path dst-dir "out/shadow-watch"))
      (spit (fs/file dst-dir "out/shadow-watch/index.html")
            "<!doctype html>\n<html><head><meta charset=\"utf-8\"></head><body>\n<script src=\"/js/main.js\"></script>\n</body></html>\n")
      (fs/create-dirs (fs/path dst-dir "out/light-watch"))
      (build-watch-packages! esbuild dst-dir (fs/path dst-dir "out/light-watch/packages.js"))
      (spit (fs/file dst-dir "out/light-watch/index.html")
            (str "<!doctype html>\n<html><head><meta charset=\"utf-8\"></head><body>\n"
                 light-watch-loader "</body></html>\n"))
      (spit (fs/file marker) (str src-str "\n")))
    (str dst-dir)))

(defn ensure-re-frame-examples-staged! [research-dir work-dir]
  (let [rf-src (fs/path research-dir "re-frame")
        rf-dst (fs/path work-dir "re-frame")
        marker (fs/path rf-dst ".gap-rf-staged")
        sig (str (fs/canonicalize rf-src) "|" shadow-version-pin)]
    (when-not (and (fs/exists? marker) (= sig (str/trim (slurp (fs/file marker)))))
      (cmp/log "staging re-frame examples (simple, todomvc, flow) in" (str rf-dst))
      (fs/delete-tree rf-dst)
      (fs/create-dirs (fs/path rf-dst "examples"))
      (fs/copy (fs/path rf-src "deps.edn") (fs/path rf-dst "deps.edn"))
      (fs/copy-tree (fs/path rf-src "src") (fs/path rf-dst "src"))
      (doseq [app ["simple" "todomvc" "flow"]
              :let [src-ex (fs/path rf-src "examples" app)
                    dst-ex (fs/path rf-dst "examples" app)]]
        (fs/copy-tree src-ex dst-ex)
        (doseq [[file pattern] [["package.json" #"(\"shadow-cljs\": \")[^\"]+"]
                                ["deps.edn" #"(thheller/shadow-cljs\s+\{:mvn/version \")[^\"]+"]]
                :let [f (fs/file dst-ex file)]]
          (spit f (str/replace (slurp f) pattern (str "$1" shadow-version-pin))))
        (cmp/log "npm install in staged" app)
        (sh! {:dir (str dst-ex)} "npm" "install" "--no-audit" "--no-fund"))
      (spit (fs/file marker) (str sig "\n")))
    (str rf-dst)))

(defn ensure-ports-staged! [research-dir work-dir]
  (into {}
        (for [app ["simple" "todomvc" "flow"]
              :let [src (resolve-port-source research-dir app)
                    dst (fs/path work-dir "ports" app)
                    marker (fs/path dst ".gap-port-staged")
                    sig (str (fs/canonicalize src))]]
          (do
            (when-not (and (fs/exists? marker) (= sig (str/trim (slurp (fs/file marker)))))
              (cmp/log "staging port" app "from" sig)
              (copy-project-without-artifacts! src dst)
              (spit (fs/file marker) (str sig "\n")))
            [app (str dst)]))))

;; ---------------------------------------------------------------------------
;; Shadow baseline persistence

(defn read-shadow-baseline [path]
  (when (fs/exists? path)
    (cmp/read-edn path)))

(defn save-shadow-baseline! [path data]
  (fs/create-dirs (fs/parent path))
  (let [tmp (str path ".tmp")]
    (spit tmp (with-out-str (pprint/pprint data)))
    (fs/move tmp path {:replace-existing true})))

(defn use-cached-shadow?
  "Decide whether to reuse cached shadow baseline for `slice`."
  [mode cached-slice]
  (case mode
    :fresh false
    :reuse (if cached-slice
             true
             (throw (ex-info (str "No cached shadow baseline for slice; run with --fresh-shadow first") {})))
    :auto (boolean cached-slice)))

;; ---------------------------------------------------------------------------
;; Slice 1: Compiler performance, cold/warm cache, build memory & scaling

(defn timed-light-stats
  "Run a light-cljs command under /usr/bin/time and capture wall/CPU/RSS + --stats + optional --size-report."
  [dir cmd]
  (let [load (cmp/load-average)
        r (apply process/sh {:dir (str dir) :err :string :out :string}
                 "/usr/bin/time" (if cmp/mac? "-l" "-v") (map str cmd))
        err (:err r)]
    (when-not (zero? (:exit r))
      (throw (ex-info (str "light-cljs failed: " (str/join " " cmd) "\n"
                           (subs err (max 0 (- (count err) 3000)))) {})))
    (let [t (cmp/parse-time cmp/mac? err)
          stats (cmp/parse-stats err)
          sr (parse-size-report err)]
      (cond-> (assoc t :cpu-s (+ (:user-s t) (:system-s t)) :load load :stats stats)
        sr (assoc :size-report sr)))))

(defn timed-shadow-cmd
  "Run a shadow-cljs command under /usr/bin/time and return timing sample + load."
  [dir cmd]
  (let [load (cmp/load-average)
        r (apply process/sh {:dir (str dir) :err :string :out :string}
                 "/usr/bin/time" (if cmp/mac? "-l" "-v") (map str cmd))
        err (:err r)]
    (when-not (zero? (:exit r))
      (throw (ex-info (str "shadow-cljs failed: " (str/join " " cmd) "\n"
                           (subs err (max 0 (- (count err) 3000)))) {})))
    (let [t (cmp/parse-time cmp/mac? err)]
      (assoc t :cpu-s (+ (:user-s t) (:system-s t)) :load load))))

(defn summarize-samples [samples]
  (let [m (fn [k] (cmp/spread (map k samples)))
        stat-keys (when (seq (:stats (first samples)))
                    (keys (:stats (first samples))))
        stats-med (when stat-keys
                    (into (sorted-map)
                          (for [k stat-keys
                                :let [vs (keep #(get-in % [:stats k]) samples)]
                                :when (seq vs)]
                            [k (cmp/median vs)])))]
    (cond-> {:runs (count samples)
             :wall-s (m :wall-s)
             :user-s (m :user-s)
             :system-s (m :system-s)
             :cpu-s (m :cpu-s)
             :peak-memory-gb (m :peak-memory-gb)
             :load-1m (cmp/spread (map (comp first :load) samples))
             :samples samples}
      stats-med (assoc :stats-median stats-med))))

(defn verify-200k-output! [dir expected runner out-rel]
  (let [got (str/trim (:out (sh! {:dir dir} "node" runner "42" "3")))
        waste (process/sh {:dir dir :err :string :out :string} "bb" "check-waste.bb" "--strict" out-rel)]
    (when-not (= expected got)
      (throw (ex-info (str "Oracle mismatch for " runner ": expected " expected ", got " got) {})))
    (when-not (zero? (:exit waste))
      (throw (ex-info (str "Waste check failed for " out-rel ": " (:out waste) (:err waste)) {})))
    {:checksum got :waste-ok true :waste (str/trim (:out waste))
     :bytes (fs/size (fs/path dir out-rel))}))

(defn project-code-lines [dir]
  (:code-lines (cmp/read-edn (fs/file dir "manifest.edn"))))

(defn run-compile-slice!
  [{:keys [light compile-runs shadow-heap shadow-mode]} ctx shadow-baseline]
  (let [{:keys [dir-200k dir-50k shadow-artifacts-dir]} ctx
        t0 (now-ms)
        expected (str/trim (:out (sh! {:dir dir-200k} "bb" "oracle.bb" "42" "3")))
        lines-200k (project-code-lines dir-200k)
        lines-50k (project-code-lines dir-50k)
        light-out-file (fs/path dir-200k "out/light/workload.main.js")
        pristine-light (fs/path dir-200k "out/.pristine-light")
        cache-dir ".gap-cache"

        ;; 1. Clean light-cljs builds (compile-runs, determinism hash check)
        ;; Pass --size-report on final --no-cache run: --no-cache has no output
        ;; cache to bypass and retention formatting takes <1 ms after emit.
        _ (cmp/log "== [compile] light-cljs 200k clean (" compile-runs "runs)")
        clean-res
        (vec (for [i (range compile-runs)]
               (do (fs/delete-tree (fs/path dir-200k ".light-cljs"))
                   (fs/delete-tree (fs/path dir-200k cache-dir))
                   (fs/delete-tree (fs/path dir-200k "out/light"))
                   (let [cmd (cond-> [light "release" "bench" "--no-cache" "--stats"]
                               (= i (dec compile-runs)) (conj "--size-report"))
                         s (timed-light-stats dir-200k cmd)
                         h (sha256-file light-out-file)]
                     (spit (fs/file dir-200k "out/light/package.json") "{\"type\":\"module\"}\n")
                     (cmp/log (format "  light clean #%d: %.2fs wall, %.2fs cpu, %.3f GB, sha=%s"
                                      (inc i) (:wall-s s) (:cpu-s s) (:peak-memory-gb s) (subs h 0 12)))
                     (assoc s :output-sha256 h)))))
        size-report-200k (:size-report (last clean-res))
        clean-hashes (mapv :output-sha256 clean-res)
        deterministic? (= 1 (count (set clean-hashes)))
        light-verify (verify-200k-output! dir-200k expected "run-light.mjs" "out/light/workload.main.js")
        ;; Preserve pristine clean light build for size/runtime slices.
        _ (do (fs/delete-tree pristine-light)
              (fs/copy-tree (fs/path dir-200k "out/light") pristine-light))

        ;; 2. Cold cache-fill
        _ (cmp/log "== [compile] light-cljs 200k cold cache-fill")
        _ (do (fs/delete-tree (fs/path dir-200k cache-dir))
              (fs/delete-tree (fs/path dir-200k "out/light")))
        cache-fill (timed-light-stats dir-200k [light "release" "bench" "--cache-dir" cache-dir "--stats"])
        cache-disk-bytes (dir-size-bytes (fs/path dir-200k cache-dir))
        _ (cmp/log (format "  light cache-fill: %.2fs wall, cache disk %.1f MB"
                           (:wall-s cache-fill) (/ cache-disk-bytes 1e6)))

        ;; 3. Warm unchanged (never pass --size-report here: it disables the output cache!)
        _ (cmp/log "== [compile] light-cljs 200k warm unchanged (" compile-runs "runs)")
        warm-unchanged
        (vec (for [i (range compile-runs)
                   :let [s (timed-light-stats dir-200k [light "release" "bench" "--cache-dir" cache-dir "--stats"])]]
               (do (cmp/log (format "  light warm-unchanged #%d: %.2fs wall, %.2fs cpu"
                                    (inc i) (:wall-s s) (:cpu-s s)))
                   s)))

        ;; 4. Warm leaf-body edit (1 run on warmed cache)
        leaf-path (str (fs/path dir-200k re-frame-leaf))
        _ (cmp/log "== [compile] light-cljs 200k warm leaf-body edit (1 run)")
        warm-leaf
        (cmp/with-restored leaf-path
          (fn [orig]
            (spit leaf-path (str orig "\n(defn gap-bench-leaf-edit-0 [] 0)\n"))
            (let [s (timed-light-stats dir-200k [light "release" "bench" "--cache-dir" cache-dir "--stats"])]
              (cmp/log (format "  light warm-leaf: %.2fs wall, %.2fs cpu" (:wall-s s) (:cpu-s s)))
              [s])))

        ;; 5. Warm declaration edit (unused defprotocol, 1 run)
        _ (cmp/log "== [compile] light-cljs 200k warm declaration edit (1 run)")
        warm-decl
        (cmp/with-restored leaf-path
          (fn [orig]
            (spit leaf-path (str orig "\n(defprotocol GapBenchDeclEdit (-gap-decl-edit [this]))\n"))
            (let [s (timed-light-stats dir-200k [light "release" "bench" "--cache-dir" cache-dir "--stats"])]
              (cmp/log (format "  light warm-decl: %.2fs wall, %.2fs cpu" (:wall-s s) (:cpu-s s)))
              s)))

        ;; Restore pristine clean light output so downstream slices measure unmodified code.
        _ (do (fs/delete-tree (fs/path dir-200k "out/light"))
              (fs/copy-tree pristine-light (fs/path dir-200k "out/light"))
              (fs/delete-tree pristine-light))

        ;; 6. Scaling canary: 50k clean build
        _ (cmp/log "== [compile] light-cljs 50k clean (scaling canary)")
        _ (fs/delete-tree (fs/path dir-50k "out/light"))
        clean-50k (timed-light-stats dir-50k [light "release" "bench" "--no-cache" "--stats"])
        _ (cmp/log (format "  light 50k clean: %.2fs wall, %.2fs cpu, %.3f GB"
                           (:wall-s clean-50k) (:cpu-s clean-50k) (:peak-memory-gb clean-50k)))

        light-clean-sum (summarize-samples clean-res)
        light-scaling {:lines-50k lines-50k
                       :lines-200k lines-200k
                       :wall-50k-s (:wall-s clean-50k)
                       :wall-200k-s (get-in light-clean-sum [:wall-s :median])
                       :wall-exp (scaling-exponent lines-50k (:wall-s clean-50k)
                                                   lines-200k (get-in light-clean-sum [:wall-s :median]))
                       :cpu-exp (scaling-exponent lines-50k (:cpu-s clean-50k)
                                                  lines-200k (get-in light-clean-sum [:cpu-s :median]))
                       :mem-exp (scaling-exponent lines-50k (:peak-memory-gb clean-50k)
                                                  lines-200k (get-in light-clean-sum [:peak-memory-gb :median]))}

        ;; 7. Shadow-cljs (fresh or cached: 1 clean + 1 warm leaf edit at -Xmx<shadow-heap>)
        cached-shadow (let [c (get-in shadow-baseline [:slices :compile])]
                        (when (and c (= shadow-heap (:shadow-heap c))) c))
        reuse? (use-cached-shadow? shadow-mode cached-shadow)
        shadow-data
        (if reuse?
          (do (cmp/log "== [compile] reusing cached shadow-cljs baseline (heap -Xmx" shadow-heap ")")
              (assoc cached-shadow :reused true))
          (let [shadow-cmd ["clojure" (str "-J-Xmx" shadow-heap) "-J-XX:+ExitOnOutOfMemoryError"
                            "-M:shadow" "release" "bench"]
                shadow-out (fs/path dir-200k "out/shadow.js")
                saved-artifact (fs/path shadow-artifacts-dir "re-frame-200k-shadow.js")]
            (cmp/log "== [compile] shadow-cljs 200k clean (-Xmx" shadow-heap ")")
            (fs/delete-tree (fs/path dir-200k ".shadow-cljs"))
            (fs/delete-if-exists shadow-out)
            (let [s-clean (timed-shadow-cmd dir-200k shadow-cmd)
                  s-verify (verify-200k-output! dir-200k expected "out/shadow.js" "out/shadow.js")]
              (cmp/log (format "  shadow clean: %.2fs wall, %.2fs cpu, %.3f GB"
                               (:wall-s s-clean) (:cpu-s s-clean) (:peak-memory-gb s-clean)))
              (fs/create-dirs shadow-artifacts-dir)
              (fs/copy shadow-out saved-artifact {:replace-existing true})

              (cmp/log "== [compile] shadow-cljs 200k warm leaf-body edit")
              (let [s-leaf (cmp/with-restored leaf-path
                             (fn [orig]
                               (spit leaf-path (str orig "\n(defn gap-bench-shadow-leaf [] 1)\n"))
                               (let [s (timed-shadow-cmd dir-200k shadow-cmd)]
                                 (cmp/log (format "  shadow warm-leaf: %.2fs wall, %.2fs cpu"
                                                  (:wall-s s) (:cpu-s s)))
                                 s)))]
                ;; Restore pristine clean shadow.js to out/shadow.js
                (fs/copy saved-artifact shadow-out {:replace-existing true})
                {:shadow-heap shadow-heap
                 :reused false
                 :clean (summarize-samples [s-clean])
                 :warm-leaf (summarize-samples [s-leaf])
                 :verify s-verify}))))
        ;; Ensure out/shadow.js has the saved artifact if we reused shadow
        _ (when reuse?
            (let [saved (fs/path shadow-artifacts-dir "re-frame-200k-shadow.js")]
              (when (fs/exists? saved)
                (fs/create-dirs (fs/path dir-200k "out"))
                (fs/copy saved (fs/path dir-200k "out/shadow.js") {:replace-existing true}))))]
    {:elapsed-s (/ (- (now-ms) t0) 1000.0)
     :expected-checksum expected
     :light {:clean light-clean-sum
             :deterministic? deterministic?
             :clean-hashes clean-hashes
             :verify light-verify
             :cache-fill (assoc (summarize-samples [cache-fill])
                                :disk-bytes cache-disk-bytes
                                :overhead-s (- (:wall-s cache-fill) (get-in light-clean-sum [:wall-s :median])))
             :warm-unchanged (summarize-samples warm-unchanged)
             :warm-leaf (summarize-samples warm-leaf)
             :warm-decl (summarize-samples [warm-decl])
             :clean-50k (summarize-samples [clean-50k])
             :scaling light-scaling
             :size-report-200k size-report-200k}
     :shadow shadow-data}))

;; ---------------------------------------------------------------------------
;; Slice 2: Watch & hot-reload (re-frame-200k in headless Chromium)

(defn ps-rows []
  (for [l (str/split-lines (:out (sh! {} "ps" "-A" "-o" "pid=,ppid=,rss=")))
        :when (not (str/blank? l))
        :let [[pid ppid rss] (str/split (str/trim l) #"\s+")]]
    [(parse-long pid) (parse-long ppid) (parse-long rss)]))

(defn pid-tree [pid]
  (let [rows (ps-rows)
        children (group-by second rows)
        rss (into {} (map (fn [[p _ r]] [p (* 1024 r)]) rows))]
    (loop [todo [pid] acc {}]
      (if-let [[p & more] (seq todo)]
        (recur (concat more (map first (children p))) (assoc acc p (rss p 0)))
        acc))))

(defn ps-cpu-seconds [text]
  (let [[days clock] (if (str/includes? text "-") (str/split text #"-") ["0" text])]
    (+ (* 86400 (parse-long days))
       (reduce (fn [acc x] (+ (* acc 60) (parse-double x))) 0 (str/split clock #":")))))

(defn tree-cpu [pid]
  (let [rows (for [l (str/split-lines (:out (sh! {} "ps" "-A" "-o" "pid=,ppid=,utime=,stime=")))
                   :when (not (str/blank? l))]
               (str/split (str/trim l) #"\s+"))
        pids (set (keys (pid-tree pid)))]
    (reduce (fn [[u s] [p _ pu ps]]
              (if (pids (parse-long p)) [(+ u (ps-cpu-seconds pu)) (+ s (ps-cpu-seconds ps))] [u s]))
            [0.0 0.0] rows)))

(defn watch-scenarios [watch-dir]
  [{:id :leaf
    :name "view (leaf): app.baga.views"
    :file (str (fs/path watch-dir "src/app/baga/views.cljs"))
    :from "[:h2 \"Baga\"]"
    :to #(str "[:h2 \"Baga " % "\"]")
    :probe {:call "app.baga.views/root" :args [] :original "Baga"}}
   {:id :common
    :name "widely required: workload.common"
    :file (str (fs/path watch-dir "src/workload/common.cljs"))
    :from "\"GENERATED_WASTE_C_COMMON1_END\""
    :to #(str "\"GENERATED_WASTE_C_COMMON1_END " % "\"")
    :probe {:call "workload.common/geinklarvou" :args [1] :original "GENERATED_WASTE_C_COMMON1_END"}}
   {:id :macro
    :name "macro: workload.sobis/hainfoulvei"
    :file (str (fs/path watch-dir "macros/workload/sobis.cljc"))
    :from "(+ 11 (do ~@body))"
    :to #(str "(+ " % " (do ~@body))")
    :numeric true
    :probe {:call "workload.breisrei/dokgem" :args [3] :offset 11 :modulus 1000003}}])

(defn start-watch-proc [watch-dir tool light-bin]
  (let [out (fs/path watch-dir (if (= tool :light) "out/light-watch" "out/shadow-watch"))
        cmd (case tool
              :light [light-bin "watch" "bench" "--out-dir" (str out) "--serve" "0" "--no-nrepl"]
              :shadow ["clojure" "-M:shadow" "watch" "watch"])
        _ (case tool
            :light (fs/delete-tree (fs/path watch-dir ".light-cljs"))
            :shadow (fs/delete-tree (fs/path watch-dir ".shadow-cljs/builds/watch")))
        lines (atom [])
        samples (atom [])
        start (epoch-ms)
        proc (apply process/process {:dir watch-dir :out :stream :err :stream} (map str cmd))
        pump (fn [stream]
               (future (with-open [r (java.io.BufferedReader. (java.io.InputStreamReader. stream))]
                         (doseq [l (line-seq r)] (swap! lines conj [(epoch-ms) l])))))
        pid (.pid ^Process (:proc proc))
        running (atom true)]
    (pump (:out proc))
    (pump (:err proc))
    (future (while @running
              (try (swap! samples conj [(epoch-ms) (reduce + 0 (vals (pid-tree pid)))])
                   (catch Exception _))
              (Thread/sleep 250)))
    {:proc proc :pid pid :lines lines :samples samples :start start :running running :out (str out)}))

(defn stop-watch-proc [{:keys [pid running]}]
  (reset! running false)
  (doseq [p (keys (pid-tree pid))] (process/sh "kill" "-TERM" (str p)))
  (Thread/sleep 1500)
  (doseq [p (keys (pid-tree pid))] (process/sh "kill" "-KILL" (str p))))

(defn wait-watch-line [{:keys [lines proc]} pattern what timeout-ms]
  (let [deadline (+ (now-ms) timeout-ms)]
    (loop []
      (if-let [hit (some (fn [[t l]] (when-let [m (re-find pattern l)] [t m])) @lines)]
        hit
        (do (when (or (> (now-ms) deadline) (not (process/alive? proc)))
              (throw (ex-info (str "Watch timed out waiting for " what "; last lines:\n"
                                   (str/join "\n" (map second (take-last 30 @lines)))) {})))
            (Thread/sleep 20)
            (recur))))))

(defn parse-watch-step-lines [tool lines]
  (keep (fn [[t l]]
          (case tool
            :light (when-let [[_ nss ms] (re-find #"recompiled (.*) in ([\d.]+) ms" l)]
                     [t (parse-double ms) (count (str/split nss #", "))])
            :shadow (when-let [[_ _ compiled s] (re-find #"Build completed\. \((\d+) files, (\d+) compiled, \d+ warnings, ([\d.]+)s\)" l)]
                      [t (* 1000.0 (parse-double s)) (parse-long compiled)])))
        lines))

(defn summarize-watch-tool [raw]
  (let [by-sc (group-by :scenario (filter #(pos? (:i %)) (:edits raw)))
        sp (fn [xs] (when (and (seq xs) (every? number? xs)) (cmp/spread xs)))
        diff-sp (fn [es a b]
                  (let [ds (for [e es :when (and (number? (a e)) (number? (b e)))] (- (a e) (b e)))]
                    (sp ds)))
        scenarios (into {}
                        (for [[sc es] by-sc]
                          [sc {:edits (count es)
                               :output-ms (sp (map :output es))
                               :message-ms (sp (map :message es))
                               :running-ms (sp (map :running es))
                               :step-ms (sp (map :step-ms es))
                               :step-namespaces (sp (map :step-namespaces es))
                               :noticing-ms (diff-sp es :output :step-ms)
                               :output-to-msg-ms (diff-sp es :message :output)
                               :msg-to-running-ms (diff-sp es :running :message)
                               :cpu-s (sp (map #(+ (:cpuUser %) (:cpuSys %)) es))
                               :load-1m (sp (map :load es))}]))]
    (assoc (dissoc raw :edits) :scenarios scenarios)))

(defn run-watch-tool! [watch-dir npm-tools tool light-bin watch-edits]
  (let [scenarios (watch-scenarios watch-dir)
        configs (mapv (fn [sc]
                        (let [text (slurp (:file sc))
                              tokens (for [k (range (inc watch-edits))]
                                       (if (:numeric sc) (str (+ 972001 k)) (str "gbe" (inc k))))]
                          {:name (:name sc) :file (:file sc) :text text
                           :edits (mapv #(str/replace text (:from sc) ((:to sc) %)) tokens)
                           :tokens (vec tokens) :probe (:probe sc)}))
                      scenarios)
        load0 (first (cmp/load-average))
        w (start-watch-proc watch-dir tool light-bin)
        ready-pat (if (= tool :light) #"watching every" #"Build completed\.")
        url-pat (if (= tool :light) #"serving .* at (http://\S+?)/?$" #":dev-http (http://\S+) serving")]
    (try
      (let [[ready-at _] (wait-watch-line w ready-pat "first build" 600000)
            startup-cpu (tree-cpu (:pid w))
            [_ [_ url]] (wait-watch-line w url-pat "server URL" 60000)
            cfg (fs/create-temp-file {:prefix "gap-watch" :suffix ".json"})
            driver-start (epoch-ms)
            _ (spit (fs/file cfg)
                    (json/generate-string
                     {:tool (name tool)
                      :url url
                      :events (str (str/replace url #"/$" "") "/events")
                      :npmTools npm-tools
                      :outDir (:out w)
                      :rootPid (:pid w)
                      :ready "!!document.querySelector('#app-root header')"
                      :timeoutMs 300000
                      :pauseMs 250
                      :pauseJitterMs 400
                      :scenarios configs}))
            r (process/sh {:err :inherit :out :string}
                          "node" (str (fs/path repo "benchmarks/watch-vs-shadow.mjs")) (str cfg))
            _ (fs/delete-if-exists cfg)
            _ (when-not (zero? (:exit r)) (throw (ex-info (str "watch-vs-shadow.mjs failed for " (name tool)) {})))
            res (json/parse-string (:out r) true)
            edits-start (+ driver-start (:pageLoadMs res))
            samples @(:samples w)
            steps (parse-watch-step-lines tool @(:lines w))
            with-step (fn [e]
                        (let [[t ms n] (first (filter #(>= (first %) (:mtime e)) steps))]
                          (assoc e :step-ms ms :step-namespaces n :step-logged t)))]
        (summarize-watch-tool
         {:tool tool
          :watch-edits watch-edits
          :load-before load0
          :startup-ms (- ready-at (:start w))
          :startup-cpu-user-s (first startup-cpu)
          :startup-cpu-sys-s (second startup-cpu)
          :startup-cpu-s (+ (first startup-cpu) (second startup-cpu))
          :page-load-ms (:pageLoadMs res)
          :startup-peak-bytes (apply max 0 (map second (filter #(<= (first %) ready-at) samples)))
          :steady-peak-bytes (apply max 0 (map second (filter #(>= (first %) edits-start) samples)))
          :errors (vec (:errors res))
          :edits (mapv with-step (:edits res))}))
      (finally
        (stop-watch-proc w)
        (doseq [{:keys [file text]} configs
                :when (not= text (slurp file))]
          (spit file text))))))

(defn run-watch-slice!
  [{:keys [light watch-edits shadow-mode skip-browser]} ctx shadow-baseline]
  (if skip-browser
    {:skipped true :reason "--skip-browser"}
    (let [{:keys [watch-dir npm-tools]} ctx
          t0 (now-ms)
          _ (cmp/log "== [watch] light-cljs watch re-frame-200k (" watch-edits "timed edits/scenario)")
          light-res (run-watch-tool! watch-dir npm-tools :light light watch-edits)
          _ (when (seq (:errors light-res))
              (throw (ex-info (str "light-cljs watch had browser errors: " (pr-str (:errors light-res))) {})))
          cached-shadow (get-in shadow-baseline [:slices :watch])
          reuse? (use-cached-shadow? shadow-mode cached-shadow)
          shadow-res (if reuse?
                       (do (cmp/log "== [watch] reusing cached shadow-cljs watch baseline")
                           (assoc cached-shadow :reused true))
                       (do (cmp/log "== [watch] shadow-cljs watch re-frame-200k (" watch-edits "timed edits/scenario)")
                           (let [r (run-watch-tool! watch-dir npm-tools :shadow light watch-edits)]
                             (when (seq (:errors r))
                               (throw (ex-info (str "shadow-cljs watch had browser errors: " (pr-str (:errors r))) {})))
                             (assoc r :reused false))))]
      {:elapsed-s (/ (- (now-ms) t0) 1000.0)
       :watch-edits watch-edits
       :scenario-order (mapv :name (watch-scenarios watch-dir))
       :light light-res
       :shadow shadow-res})))

;; ---------------------------------------------------------------------------
;; Slice 3: Output size & quality metrics (simple, todomvc, flow, re-frame-200k)

(defn esbuild-bundle! [esbuild dir entry out & extra]
  (apply sh! {:dir dir} esbuild (str entry) "--bundle" "--minify"
         "--define:process.env.NODE_ENV=\"production\"" (str "--outfile=" out) "--log-level=warning"
         (concat (when-not (some #(str/starts-with? % "--format") extra) ["--format=esm"]) extra)))

(defn react-only-light! [esbuild ex-dir packages]
  (let [entry (fs/path ex-dir "out/react-only.mjs")
        out (fs/path ex-dir "out/light-react-only.js")]
    (fs/create-dirs (fs/parent entry))
    (spit (fs/file entry)
          (str (str/join (map-indexed (fn [i p] (str "import * as p" i " from " (pr-str p) ";\n")) packages))
               "console.log(" (str/join ", " (map #(str "p" %) (range (count packages)))) ");\n"))
    (esbuild-bundle! esbuild ex-dir entry out)
    (compressed-sizes out)))

(defn react-only-shadow! [ex-dir packages]
  (let [src (fs/path ex-dir "out/react-only-src")
        ns-file (fs/path src "light_size/react_only.cljs")
        out-js (fs/path ex-dir "out/shadow-react-only/client.js")]
    (fs/delete-tree src)
    (fs/delete-tree (fs/path ex-dir "out/shadow-react-only"))
    (fs/create-dirs (fs/parent ns-file))
    (spit (fs/file ns-file)
          (str "(ns light-size.react-only (:require "
               (str/join " " (map-indexed (fn [i p] (str "[" (pr-str p) " :as p" i "]")) packages))
               "))\n(defn run [] (js/console.log "
               (str/join " " (map #(str "p" %) (range (count packages)))) "))\n"))
    (fs/delete-tree (fs/path ex-dir ".shadow-cljs/builds"))
    (sh! {:dir ex-dir} "clojure" "-Sdeps" (pr-str {:aliases {:react-only {:extra-paths [(str src)]}}})
         "-M:react-only" "-m" "shadow.cljs.devtools.cli" "release" "client" "--config-merge"
         (pr-str {:output-dir "out/shadow-react-only"
                  :modules {:client {:init-fn 'light-size.react-only/run}}}))
    {:sizes (compressed-sizes out-js)
     :npm-files (shadow-packages (fs/path ex-dir "out/shadow-react-only/manifest.edn"))}))

(defn light-release-example! [light port-dir ex-dir]
  (let [out (fs/path ex-dir "out/light-release")]
    (fs/delete-tree out)
    (let [r (sh! {:dir port-dir} light "release" "client" "--no-cache" "--size-report" "--out-dir" (str out))
          files (vec (fs/glob out "*.js"))]
      (when (not= 1 (count files))
        (throw (ex-info (str "Expected 1 JS file in " out ", got " (count files)) {})))
      {:file (first files)
       :size-report (parse-size-report (:err r))})))

(defn shadow-release-example! [ex-dir]
  (fs/delete-tree (fs/path ex-dir ".shadow-cljs/builds"))
  (fs/delete-tree (fs/path ex-dir "out/shadow-release"))
  (sh! {:dir ex-dir} "npx" "shadow-cljs" "release" "client"
       "--config-merge" (pr-str {:output-dir "out/shadow-release"}))
  {:file (fs/path ex-dir "out/shadow-release/client.js")
   :npm-files (shadow-packages (fs/path ex-dir "out/shadow-release/manifest.edn"))})

(defn example-init-fn [port-dir]
  (let [report (edn/read-string (slurp (fs/file port-dir "port-report.edn")))
        f (:value (first (filter #(= [:builds :client :modules :client :init-fn] (:key %)) (:config report))))
        file (some #(let [p (fs/path port-dir "src" (str (str/replace (munge (namespace f)) "." "/") %))]
                      (when (fs/exists? p) p))
                   [".cljs" ".cljc"])
        last-form (last (edamame/parse-string-all (slurp (fs/file file))
                                                  {:all true :read-cond :allow :features #{:cljs}
                                                   :auto-resolve name}))]
    (when-not (and (seq? last-form) (= 'defn (first last-form)) (= (name f) (name (second last-form))))
      (throw (ex-info (str "init-fn " f " is not final form of " file) {})))
    f))

(defn stage-todomvc-browser!
  "Prepare out/browser-light and/or out/browser-shadow for TodoMVC browser checks & runtime."
  [esbuild ex-dir port-dir compiler source-js]
  (let [bdir (fs/path ex-dir "out" (str "browser-" (name compiler)))
        client (fs/path bdir "js/client.js")]
    (fs/delete-tree bdir)
    (fs/copy-tree (fs/path ex-dir "resources/public") bdir)
    (fs/delete-tree (fs/path bdir "js"))
    (fs/create-dirs (fs/parent client))
    (case compiler
      :shadow (fs/copy source-js client)
      :light (let [f (example-init-fn port-dir)
                   entry (fs/path ex-dir "out/browser-entry.mjs")]
               (spit (fs/file entry)
                     (str "import init from " (pr-str (str "./light-release/" (fs/file-name source-js))) ";\n"
                          "if (typeof init !== \"function\") throw new Error(" (pr-str (str f " is not default export")) ");\n"
                          "init();\n"))
               (esbuild-bundle! esbuild ex-dir entry client "--format=iife")))
    (str bdir)))

(defn run-size-slice!
  [{:keys [light shadow-mode]} ctx shadow-baseline compile-result]
  (let [{:keys [rf-dir ports dir-200k esbuild shadow-artifacts-dir]} ctx
        t0 (now-ms)
        cached-shadow (get-in shadow-baseline [:slices :size])
        reuse? (use-cached-shadow? shadow-mode cached-shadow)
        light-react-cache (atom {})
        apps ["simple" "todomvc" "flow"]

        ;; 1. Light builds (fast, sequential, ~2.5s total)
        light-by-app
        (into (array-map)
              (for [app apps
                    :let [ex-dir (str (fs/path rf-dir "examples" app))
                          port-dir (ports app)]]
                (do
                  (cmp/log "== [size]" app "(light-cljs)")
                  (let [{:keys [file size-report]} (light-release-example! light port-dir ex-dir)
                        pkgs (package-imports file)
                        bundle-file (fs/path ex-dir "out/light-bundle.js")
                        _ (esbuild-bundle! esbuild ex-dir file bundle-file)
                        light-total (compressed-sizes bundle-file)
                        light-output (compressed-sizes file)
                        light-react (or (@light-react-cache pkgs)
                                        (let [r (react-only-light! esbuild ex-dir pkgs)]
                                          (swap! light-react-cache assoc pkgs r)
                                          r))
                        light-app (minus-sizes light-total light-react)
                        es2015 (when (= app "todomvc")
                                 (let [f2015 (fs/path ex-dir "out/light-bundle-es2015.js")]
                                   (esbuild-bundle! esbuild ex-dir file f2015 "--target=es2015")
                                   (compressed-sizes f2015)))
                        _ (when (= app "todomvc")
                            (stage-todomvc-browser! esbuild ex-dir port-dir :light file))]
                    [app {:packages pkgs
                          :light {:total light-total :react light-react :app light-app
                                  :output light-output :es2015 es2015 :size-report size-report}}]))))

        ;; 2. Shadow builds: reused from baseline, or run concurrently across
        ;; independent example directories (only output bytes are measured, never time).
        shadow-by-app
        (if reuse?
          (do
            (cmp/log "== [size] reusing cached shadow-cljs size baseline")
            (let [td-dir (str (fs/path rf-dir "examples/todomvc"))
                  saved (fs/path shadow-artifacts-dir "todomvc-shadow-client.js")]
              (when (fs/exists? saved)
                (stage-todomvc-browser! esbuild td-dir (ports "todomvc") :shadow saved)))
            (into {} (for [app apps] [app (assoc (get-in cached-shadow [:apps app]) :reused true)])))
          (do
            (cmp/log "== [size] building shadow-cljs examples concurrently (simple, todomvc, flow)")
            (let [app-futs (into {}
                                 (for [app apps
                                       :let [ex-dir (str (fs/path rf-dir "examples" app))]]
                                   [app (future (shadow-release-example! ex-dir))]))
                  app-built (into {} (for [[app fut] app-futs] [app @fut]))
                  ;; Deduplicate React-only shadow builds by [packages app-npm] and run concurrently
                  unique-keys (into {}
                                    (for [app apps
                                          :let [pkgs (get-in light-by-app [app :packages])
                                                app-npm (get-in app-built [app :npm-files])]]
                                      [[pkgs app-npm] app]))
                  _ (cmp/log "== [size] building shadow-cljs React-only baseline(s) (" (count unique-keys) ")")
                  react-futs (into {}
                                   (for [[[pkgs _] rep-app] unique-keys
                                         :let [ex-dir (str (fs/path rf-dir "examples" rep-app))]]
                                     [[pkgs (get-in app-built [rep-app :npm-files])]
                                      (future (react-only-shadow! ex-dir pkgs))]))
                  react-built (into {} (for [[k fut] react-futs] [k @fut]))]
              (into {}
                    (for [app apps
                          :let [ex-dir (str (fs/path rf-dir "examples" app))
                                pkgs (get-in light-by-app [app :packages])
                                {s-file :file app-npm :npm-files} (app-built app)
                                s-total (compressed-sizes s-file)
                                s-react-entry (react-built [pkgs app-npm])
                                _ (when (not= app-npm (:npm-files s-react-entry))
                                    (throw (ex-info (str app ": shadow React-only npm files mismatch")
                                                    {:app app-npm :react (:npm-files s-react-entry)})))
                                s-react (:sizes s-react-entry)
                                s-app (minus-sizes s-total s-react)]]
                      (do
                        (when (= app "todomvc")
                          (fs/create-dirs shadow-artifacts-dir)
                          (fs/copy s-file (fs/path shadow-artifacts-dir "todomvc-shadow-client.js")
                                   {:replace-existing true})
                          (stage-todomvc-browser! esbuild ex-dir (ports "todomvc") :shadow s-file))
                        [app {:total s-total :react s-react :app s-app
                              :npm-files-count (count app-npm) :reused false}]))))))

        example-results
        (into (array-map)
              (for [app apps]
                [app (assoc (light-by-app app) :shadow (shadow-by-app app))]))

        ;; Also measure re-frame-200k bundle size + runtime slice
        _ (cmp/log "== [size] re-frame-200k")
        light-200k-js (fs/path dir-200k "out/light/workload.main.js")
        sr-200k (or (get-in compile-result [:light :size-report-200k])
                    (let [r (sh! {:dir dir-200k} light "release" "bench" "--cache-dir" ".gap-cache" "--size-report")]
                      (spit (fs/file dir-200k "out/light/package.json") "{\"type\":\"module\"}\n")
                      (parse-size-report (:err r))))
        _ (when-not (fs/exists? light-200k-js)
            (sh! {:dir dir-200k} light "release" "bench" "--no-cache")
            (spit (fs/file dir-200k "out/light/package.json") "{\"type\":\"module\"}\n"))
        light-200k-sizes (compressed-sizes light-200k-js)
        shadow-200k-entry
        (if reuse?
          (assoc (get-in cached-shadow [:apps "re-frame-200k"]) :reused true)
          (let [s-js (fs/path dir-200k "out/shadow.js")
                saved (fs/path shadow-artifacts-dir "re-frame-200k-shadow.js")]
            (when-not (fs/exists? s-js)
              (if (fs/exists? saved)
                (do (fs/create-dirs (fs/parent s-js)) (fs/copy saved s-js))
                (do (sh! {:dir dir-200k} "clojure" "-J-Xmx1536m" "-M:shadow" "release" "bench")
                    (fs/create-dirs shadow-artifacts-dir)
                    (fs/copy s-js saved {:replace-existing true}))))
            {:total (compressed-sizes s-js) :app (compressed-sizes s-js) :reused false}))
        all-apps (assoc example-results
                        "re-frame-200k"
                        {:packages ["react" "react-dom" "react-dom/client"]
                         :light {:total light-200k-sizes :app light-200k-sizes
                                 :output light-200k-sizes :size-report sr-200k}
                         :shadow shadow-200k-entry})]
    {:elapsed-s (/ (- (now-ms) t0) 1000.0)
     :apps all-apps
     :shadow-cache {:apps (into {} (for [[a v] all-apps] [a (dissoc (:shadow v) :reused)]))}}))

;; ---------------------------------------------------------------------------
;; Slice 4: Runtime speed, startup phase breakdown & runtime memory

(defn run-startup-probes! [dir-200k runs expected-1]
  (cmp/log "== [runtime] re-frame-200k startup phase breakdown (" runs "alternated runs)")
  (let [probe-script (str (fs/path here "startup-probe.mjs"))
        light-js (str (fs/path dir-200k "out/light/workload.main.js"))
        shadow-js (str (fs/path dir-200k "out/shadow.js"))
        samples
        (vec (for [r (range runs)
                   tool (if (even? r) [:light :shadow] [:shadow :light])]
               (let [load (cmp/load-average)
                     file (if (= tool :light) light-js shadow-js)
                     res (sh! {:dir dir-200k} "node" probe-script
                              "--tool" (name tool) "--file" file "--project-dir" dir-200k
                              "--seed" (str runtime-seed))
                     data (json/parse-string (:out res) true)]
                 (when-not (= expected-1 (:checksum data))
                   (throw (ex-info (str "startup-probe checksum mismatch for " (name tool))
                                   {:expected expected-1 :got (:checksum data)})))
                 (assoc data :tool tool :run r :load load))))
        summarize-tool
        (fn [tool]
          (let [ts (filter #(= tool (:tool %)) samples)
                sp (fn [k] (let [vs (keep k ts)] (when (= (count vs) (count ts)) (cmp/spread vs))))]
            {:runs (count ts)
             :instrumented (:instrumented (first ts))
             :compile-ms (sp :compileMs)
             :runtime-ms (sp :runtimeMs)
             :top-level-ms (sp :topLevelMs)
             :load-total-ms (sp :loadTotalMs)
             :first-iter-ms (sp :firstIterMs)
             :total-ms (sp :totalMs)
             :load-1m (cmp/spread (map (comp first :load) ts))}))]
    {:expected expected-1
     :light (summarize-tool :light)
     :shadow (summarize-tool :shadow)}))

(defn run-node-200k! [dir-200k runs]
  (cmp/log "== [runtime] re-frame-200k Node throughput & RSS (" runs "alternated runs at" runtime-iters "iters)")
  (let [by-count
        (into (sorted-map)
              (for [n runtime-iters
                    :let [expected (str/trim (:out (sh! {:dir dir-200k} "bb" "oracle.bb"
                                                        (str runtime-seed) (str n))))
                          check (fn [tool]
                                  (fn [out]
                                    (when-not (= expected (str/trim out))
                                      (throw (ex-info (str "Node 200k checksum mismatch for " (name tool))
                                                      {:iters n :expected expected :got (str/trim out)})))))
                          samples
                          (vec (for [r (range runs)
                                     tool (if (even? r) [:light :shadow] [:shadow :light])]
                                 (let [load (cmp/load-average)
                                       cmd (if (= tool :light)
                                             ["node" "run-light.mjs" (str runtime-seed) (str n)]
                                             ["node" "out/shadow.js" (str runtime-seed) (str n)])
                                       s (cmp/timed-checked dir-200k cmd (check tool))]
                                   (assoc s :tool tool :run r :load load
                                          :cpu-s (+ (:user-s s) (:system-s s))))))
                          sum-tool
                          (fn [tool]
                            (let [ts (filter #(= tool (:tool %)) samples)]
                              {:runs (count ts)
                               :first-output-s (cmp/spread (map :first-output-s ts))
                               :wall-s (cmp/spread (map :wall-s ts))
                               :cpu-s (cmp/spread (map :cpu-s ts))
                               :peak-memory-gb (cmp/spread (map :peak-memory-gb ts))
                               :load-1m (cmp/spread (map (comp first :load) ts))}))]]
                [n {:expected expected :light (sum-tool :light) :shadow (sum-tool :shadow)}]))
        lo (first runtime-iters)
        hi (last runtime-iters)
        steady (into {}
                     (for [t [:light :shadow]]
                       [t (* 1000.0 (/ (- (get-in by-count [hi t :wall-s :median])
                                          (get-in by-count [lo t :wall-s :median]))
                                       (- hi lo)))]))]
    {:counts by-count :steady-ms-per-iter steady}))

(defn run-todomvc-browser! [ex-dir npm-tools runs todos]
  (let [browser-script (str (fs/path repo "benchmarks/re-frame-browser.mjs"))
        runtime-script (str (fs/path repo "benchmarks/re-frame-runtime.mjs"))
        bdir (fn [c] (str (fs/path ex-dir "out" (str "browser-" (name c)))))]
    (cmp/log "== [runtime] TodoMVC Chromium 9/9 behavior check (light-cljs)")
    (let [check-r (sh! {} "node" browser-script "--app" "todomvc" "--dir" (bdir :light)
                       "--npm-tools" npm-tools "--runs" "1")
          check-json (json/parse-string (:out check-r) true)
          run0 (first (:runs check-json))
          passed (count (filter :ok (:checks run0)))
          total (count (:checks run0))
          errors (vec (:errors run0))]
      (when (or (not= passed total) (seq errors))
        (throw (ex-info (str "TodoMVC behavior checks failed: " passed "/" total ", errors: " (pr-str errors))
                        {:checks (:checks run0) :errors errors})))
      (cmp/log (format "  TodoMVC behavior: %d/%d checks passed, 0 console errors" passed total))

      (cmp/log "== [runtime] TodoMVC Chromium runtime (" runs "alternated runs," todos "todos)")
      (let [samples
            (vec (for [r (range runs)
                       tool (if (even? r) [:light :shadow] [:shadow :light])]
                   (let [load (cmp/load-average)
                         res (sh! {} "node" runtime-script "--dir" (bdir tool)
                                  "--npm-tools" npm-tools "--todos" (str todos))
                         data (json/parse-string (:out res) true)]
                     (when (seq (:errors data))
                       (throw (ex-info (str "TodoMVC runtime errors for " (name tool) ": " (pr-str (:errors data))) {})))
                     (cmp/log (format "  todomvc #%d %-6s load %.1f load=%.0fms add=%.0fms heap=%.1fMB"
                                      (inc r) (name tool) (first load)
                                      (double (get-in data [:steps :load :ms]))
                                      (double (get-in data [:steps :add :ms]))
                                      (/ (double (:heapAfterAdd data)) 1e6)))
                     (assoc data :tool tool :run r :load load))))
            steps [:load :add :toggleAll :edit :clearCompleted]
            sum-tool
            (fn [tool]
              (let [ts (filter #(= tool (:tool %)) samples)]
                {:runs (count ts)
                 :steps (into {}
                              (for [st steps]
                                [st {:ms (cmp/spread (map #(get-in % [:steps st :ms]) ts))
                                     :script-ms (cmp/spread (map #(get-in % [:steps st :scriptMs]) ts))}]))
                 :heap-after-load-bytes (cmp/spread (map :heapAfterLoad ts))
                 :heap-after-add-bytes (cmp/spread (map :heapAfterAdd ts))
                 :load-1m (cmp/spread (map (comp first :load) ts))}))]
        {:todos todos
         :behavior-checks {:passed passed :total total :errors errors}
         :light (sum-tool :light)
         :shadow (sum-tool :shadow)}))))

(defn parse-ops-tsv [out]
  (into {}
        (for [line (str/split-lines out)
              :let [[op ns-s csum] (str/split line #"\t")]
              :when csum]
          [op {:ns-per-op (parse-double ns-s) :checksum csum}])))

(defn extract-ops-list [cljs-file]
  (vec (keep #(second (re-find #"^\s+\[\[?\"([^\"]+)\" b-" %))
             (str/split-lines (slurp (str cljs-file))))))

(defn prepare-ops-builds!
  "Build light (always) and shadow (fresh or copied from cached artifact) for core-ops and extend-ops.
  Runs fresh shadow builds concurrently since build time is not measured here."
  [suites {:keys [light shadow-mode]} shadow-artifacts-dir]
  (let [shadow-futs
        (doall
         (for [{:keys [suite-name suite-dir artifact-name]} suites
               :let [light-out (fs/path suite-dir "out/light")
                     shadow-out (fs/path suite-dir "out/shadow.js")
                     saved-shadow (fs/path shadow-artifacts-dir artifact-name)
                     reuse? (case shadow-mode
                              :fresh false
                              :reuse (if (fs/exists? saved-shadow)
                                       true
                                       (throw (ex-info (str "Missing cached shadow artifact " saved-shadow "; run --fresh-shadow") {})))
                              :auto (fs/exists? saved-shadow))]]
           (do
             (fs/delete-tree light-out)
             (sh! {:dir suite-dir} light "release" "bench")
             (spit (fs/file light-out "package.json") "{\"type\":\"module\"}\n")
             [suite-name reuse?
              (if reuse?
                (future (fs/create-dirs (fs/parent shadow-out))
                        (fs/copy saved-shadow shadow-out {:replace-existing true}))
                (future (cmp/log "== [runtime] building shadow-cljs for" suite-name)
                        (fs/delete-if-exists shadow-out)
                        (apply sh! {:dir suite-dir} (concat (cmp/default-shadow-cmd suite-dir) ["release" "bench"]))
                        (fs/create-dirs shadow-artifacts-dir)
                        (fs/copy shadow-out saved-shadow {:replace-existing true})))])))]
    (into {} (for [[nm reuse? fut] shadow-futs] (do @fut [nm reuse?])))))

(defn run-ops-suite!
  [{:keys [suite-name suite-dir source-rel group-fn]} {:keys [runs]} reuse-shadow?]
  (let [ops (extract-ops-list (fs/path suite-dir source-rel))]
    (cmp/log "== [runtime]" suite-name "(" (count ops) "ops," runs "alternated runs, scale" ops-scale ")")
    (let [results
          (vec (for [r (range runs)
                     tool (if (even? r) [:light :shadow] [:shadow :light])]
                 (let [load (cmp/load-average)
                       script (if (= tool :light) "run-light.mjs" "out/shadow.js")
                       out (:out (sh! {:dir suite-dir} "node" script "7" (str ops-rounds) "all" (str ops-scale)))
                       rows (parse-ops-tsv out)
                       missing (remove rows ops)]
                   (when (seq missing)
                     (throw (ex-info (str suite-name " missing ops for " (name tool) ": " (str/join ", " missing)) {})))
                   {:tool tool :run r :load load :rows rows})))]
      (doseq [op ops
              :let [cs (set (map #(get-in % [:rows op :checksum]) results))]]
        (when (< 1 (count cs))
          (throw (ex-info (str suite-name " checksum mismatch on " op ": " (pr-str cs)) {}))))
      (let [s (summarize-ops-runs results ops group-fn)]
        (cmp/log (format "  %s: geomean %s, parity %d/%d (<=%.2fx)"
                         suite-name (fmt-ratio (:geomean s)) (:parity-count s) (:count s) gap-threshold))
        (assoc s :shadow-reused? reuse-shadow?)))))

(defn ensure-runtime-builds!
  "When --only runtime skips compile/size, ensure light and shadow outputs exist."
  [{:keys [light shadow-mode skip-browser]} ctx]
  (let [{:keys [dir-200k rf-dir ports esbuild shadow-artifacts-dir]} ctx
        light-200k (fs/path dir-200k "out/light/workload.main.js")
        shadow-200k (fs/path dir-200k "out/shadow.js")
        saved-200k (fs/path shadow-artifacts-dir "re-frame-200k-shadow.js")]
    (when-not (fs/exists? light-200k)
      (cmp/log "== [runtime] building re-frame-200k (light-cljs)")
      (sh! {:dir dir-200k} light "release" "bench" "--no-cache")
      (spit (fs/file dir-200k "out/light/package.json") "{\"type\":\"module\"}\n"))
    (if (and (not= shadow-mode :fresh) (fs/exists? saved-200k))
      (do (fs/create-dirs (fs/parent shadow-200k))
          (fs/copy saved-200k shadow-200k {:replace-existing true}))
      (when-not (fs/exists? shadow-200k)
        (when (= shadow-mode :reuse)
          (throw (ex-info (str "Missing cached " saved-200k "; run --fresh-shadow") {})))
        (cmp/log "== [runtime] building re-frame-200k (shadow-cljs)")
        (sh! {:dir dir-200k} "clojure" "-J-Xmx1536m" "-M:shadow" "release" "bench")
        (fs/create-dirs shadow-artifacts-dir)
        (fs/copy shadow-200k saved-200k {:replace-existing true})))
    (when-not skip-browser
      (let [ex-dir (str (fs/path rf-dir "examples/todomvc"))
            port-dir (ports "todomvc")
            bl (fs/path ex-dir "out/browser-light/js/client.js")
            bs (fs/path ex-dir "out/browser-shadow/js/client.js")
            saved-td (fs/path shadow-artifacts-dir "todomvc-shadow-client.js")]
        (when-not (fs/exists? bl)
          (cmp/log "== [runtime] building todomvc browser bundle (light-cljs)")
          (let [{:keys [file]} (light-release-example! light port-dir ex-dir)]
            (stage-todomvc-browser! esbuild ex-dir port-dir :light file)))
        (if (and (not= shadow-mode :fresh) (fs/exists? saved-td))
          (stage-todomvc-browser! esbuild ex-dir port-dir :shadow saved-td)
          (when-not (fs/exists? bs)
            (when (= shadow-mode :reuse)
              (throw (ex-info (str "Missing cached " saved-td "; run --fresh-shadow") {})))
            (cmp/log "== [runtime] building todomvc browser bundle (shadow-cljs)")
            (let [{:keys [file]} (shadow-release-example! ex-dir)]
              (fs/create-dirs shadow-artifacts-dir)
              (fs/copy file saved-td {:replace-existing true})
              (stage-todomvc-browser! esbuild ex-dir port-dir :shadow file))))))))

(defn run-runtime-slice! [opts ctx]
  (let [{:keys [dir-200k rf-dir npm-tools shadow-artifacts-dir]} ctx
        {:keys [runs todos skip-browser]} opts
        t0 (now-ms)
        _ (ensure-runtime-builds! opts ctx)
        expected-1 (str/trim (:out (sh! {:dir dir-200k} "bb" "oracle.bb" (str runtime-seed) "1")))
        startup (run-startup-probes! dir-200k runs expected-1)
        node-200k (run-node-200k! dir-200k runs)
        todomvc (if skip-browser
                  {:skipped true :reason "--skip-browser"}
                  (run-todomvc-browser! (str (fs/path rf-dir "examples/todomvc")) npm-tools runs todos))
        ops-suites [{:suite-name "core-ops"
                     :suite-dir (str (fs/path repo "benchmarks/core-ops"))
                     :source-rel "src/core_ops/main.cljs"
                     :artifact-name "core-ops-shadow.js"
                     :group-fn core-op-group}
                    {:suite-name "extend-ops"
                     :suite-dir (str (fs/path here "extend-ops"))
                     :source-rel "src/extend_ops/main.cljs"
                     :artifact-name "extend-ops-shadow.js"
                     :group-fn nil}]
        reused-map (prepare-ops-builds! ops-suites opts shadow-artifacts-dir)
        core-ops (run-ops-suite! (first ops-suites) opts (reused-map "core-ops"))
        extend-ops (run-ops-suite! (second ops-suites) opts (reused-map "extend-ops"))]
    {:elapsed-s (/ (- (now-ms) t0) 1000.0)
     :startup startup
     :node-200k node-200k
     :todomvc todomvc
     :core-ops core-ops
     :extend-ops extend-ops}))

;; ---------------------------------------------------------------------------
;; Scorecard & Remaining Gaps extraction (unit-tested in test.bb)

(defn scorecard-rows
  "Build normalized scorecard metrics [{:id :area :metric :light :shadow :ratio :fmt}] from result map."
  [res]
  (let [c (:compile res)
        w (:watch res)
        sz (:size res)
        rt (:runtime res)
        hi-iter (when rt (last (sort (keys (get-in rt [:node-200k :counts])))))
        add-row (fn [rows id area metric l s fmt]
                  (if (or (number? l) (number? s))
                    (conj rows {:id id :area area :metric metric
                                :light l :shadow s :ratio (ratio l s) :fmt fmt})
                    rows))]
    (cond-> []
      c (-> (add-row :compile-clean "Compile" "re-frame-200k clean wall"
                     (get-in c [:light :clean :wall-s :median])
                     (get-in c [:shadow :clean :wall-s :median]) :sec2)
            (add-row :compile-warm-unchanged "Compile" "re-frame-200k warm unchanged"
                     (get-in c [:light :warm-unchanged :wall-s :median])
                     (get-in c [:shadow :warm-unchanged :wall-s :median]) :sec2)
            (add-row :compile-warm-leaf "Compile" "re-frame-200k warm leaf edit"
                     (get-in c [:light :warm-leaf :wall-s :median])
                     (get-in c [:shadow :warm-leaf :wall-s :median]) :sec2)
            (add-row :compile-peak-mem "Compile" "re-frame-200k clean peak memory"
                     (get-in c [:light :clean :peak-memory-gb :median])
                     (get-in c [:shadow :clean :peak-memory-gb :median]) :gb3))
      (and w (not (:skipped w)))
      (-> (add-row :watch-startup "Watch" "re-frame-200k watch startup"
                   (some-> (get-in w [:light :startup-ms]) (/ 1000.0))
                   (some-> (get-in w [:shadow :startup-ms]) (/ 1000.0)) :sec1)
          (add-row :watch-page-load "Watch" "re-frame-200k dev page load"
                   (some-> (get-in w [:light :page-load-ms]) (/ 1000.0))
                   (some-> (get-in w [:shadow :page-load-ms]) (/ 1000.0)) :sec2)
          (add-row :watch-steady-mem "Watch" "re-frame-200k watch steady RSS"
                   (some-> (get-in w [:light :steady-peak-bytes]) (/ 1e6))
                   (some-> (get-in w [:shadow :steady-peak-bytes]) (/ 1e6)) :mb0)
          (add-row :watch-leaf "Watch" "Reload: view (leaf)"
                   (get-in w [:light :scenarios "view (leaf): app.baga.views" :running-ms :median])
                   (get-in w [:shadow :scenarios "view (leaf): app.baga.views" :running-ms :median]) :ms0)
          (add-row :watch-common "Watch" "Reload: widely required (common)"
                   (get-in w [:light :scenarios "widely required: workload.common" :running-ms :median])
                   (get-in w [:shadow :scenarios "widely required: workload.common" :running-ms :median]) :ms0)
          (add-row :watch-macro "Watch" "Reload: macro (sobis)"
                   (get-in w [:light :scenarios "macro: workload.sobis/hainfoulvei" :running-ms :median])
                   (get-in w [:shadow :scenarios "macro: workload.sobis/hainfoulvei" :running-ms :median]) :ms0))
      sz (-> (add-row :size-simple-app-gz "Size" "simple app-only gzip -9"
                      (get-in sz [:apps "simple" :light :app :gzip])
                      (get-in sz [:apps "simple" :shadow :app :gzip]) :kb1)
             (add-row :size-todomvc-app-gz "Size" "todomvc app-only gzip -9"
                      (get-in sz [:apps "todomvc" :light :app :gzip])
                      (get-in sz [:apps "todomvc" :shadow :app :gzip]) :kb1)
             (add-row :size-todomvc-total-br "Size" "todomvc whole bundle brotli"
                      (get-in sz [:apps "todomvc" :light :total :brotli])
                      (get-in sz [:apps "todomvc" :shadow :total :brotli]) :kb1)
             (add-row :size-flow-app-gz "Size" "flow app-only gzip -9"
                      (get-in sz [:apps "flow" :light :app :gzip])
                      (get-in sz [:apps "flow" :shadow :app :gzip]) :kb1)
             (add-row :size-200k-gz "Size" "re-frame-200k gzip -9"
                      (get-in sz [:apps "re-frame-200k" :light :total :gzip])
                      (get-in sz [:apps "re-frame-200k" :shadow :total :gzip]) :kb1))
      rt (-> (add-row :rt-startup-load "Runtime" "re-frame-200k module load (probe)"
                      (get-in rt [:startup :light :load-total-ms :median])
                      (get-in rt [:startup :shadow :load-total-ms :median]) :ms0)
             (add-row :rt-startup-total "Runtime" "re-frame-200k startup+1st iter (probe)"
                      (get-in rt [:startup :light :total-ms :median])
                      (get-in rt [:startup :shadow :total-ms :median]) :ms0)
             (add-row :rt-200k-steady "Runtime" "re-frame-200k Node steady ms/iter"
                      (get-in rt [:node-200k :steady-ms-per-iter :light])
                      (get-in rt [:node-200k :steady-ms-per-iter :shadow]) :ms1)
             (add-row :rt-200k-rss "Runtime" (str "re-frame-200k Node peak RSS (" hi-iter " iters)")
                      (some-> (get-in rt [:node-200k :counts hi-iter :light :peak-memory-gb :median]) (* 1000.0))
                      (some-> (get-in rt [:node-200k :counts hi-iter :shadow :peak-memory-gb :median]) (* 1000.0)) :mb0))
      (and rt (not (get-in rt [:todomvc :skipped])))
      (-> (add-row :rt-todomvc-load "Runtime" "TodoMVC Chromium page load"
                   (get-in rt [:todomvc :light :steps :load :ms :median])
                   (get-in rt [:todomvc :shadow :steps :load :ms :median]) :ms0)
          (add-row :rt-todomvc-add "Runtime" "TodoMVC Chromium add todos"
                   (get-in rt [:todomvc :light :steps :add :ms :median])
                   (get-in rt [:todomvc :shadow :steps :add :ms :median]) :ms0)
          (add-row :rt-todomvc-toggle "Runtime" "TodoMVC Chromium toggleAll"
                   (get-in rt [:todomvc :light :steps :toggleAll :ms :median])
                   (get-in rt [:todomvc :shadow :steps :toggleAll :ms :median]) :ms0)
          (add-row :rt-todomvc-heap "Runtime" "TodoMVC Chromium JS heap (after add)"
                   (some-> (get-in rt [:todomvc :light :heap-after-add-bytes :median]) (/ 1e6))
                   (some-> (get-in rt [:todomvc :shadow :heap-after-add-bytes :median]) (/ 1e6)) :mb1))
      rt (-> (add-row :rt-core-ops "Runtime" "core-ops geomean (76 ops)"
                      (get-in rt [:core-ops :geomean]) 1.0 :ratio-only)
             (add-row :rt-extend-ops "Runtime" "extend-ops geomean (16 ops)"
                      (get-in rt [:extend-ops :geomean]) 1.0 :ratio-only)))))

(defn fmt-metric [val fmt-kind]
  (if-not (number? val)
    "-"
    (case fmt-kind
      :sec1 (format "%.1f s" (double val))
      :sec2 (format "%.2f s" (double val))
      :gb3 (format "%.3f GB" (double val))
      :mb0 (format "%.0f MB" (double val))
      :mb1 (format "%.1f MB" (double val))
      :ms0 (format "%,.0f ms" (double val))
      :ms1 (format "%.1f ms" (double val))
      :kb1 (format "%.1f KB" (/ (double val) 1000.0))
      :ratio-only (fmt-ratio val)
      (str val))))

(defn extract-gaps
  "Return ranked list of gaps where light/shadow > gap-threshold across scorecard + worst micro-ops."
  [res]
  (let [base-gaps (->> (scorecard-rows res)
                       (filter #(and (:ratio %) (> (:ratio %) gap-threshold)))
                       (mapv (fn [{:keys [id area metric light shadow ratio fmt]}]
                               {:id id :area area :metric metric
                                :light-text (fmt-metric light fmt)
                                :shadow-text (if (= fmt :ratio-only) "1.00x" (fmt-metric shadow fmt))
                                :light light :ratio ratio})))
        op-gaps (concat
                 (for [x (take 5 (get-in res [:runtime :core-ops :all-slower]))]
                   {:id (keyword (str "core-op-" (:op x)))
                    :area "core-ops"
                    :metric (:op x)
                    :light-text (str (fmt-ns (get-in x [:light :median])) " ns")
                    :shadow-text (str (fmt-ns (get-in x [:shadow :median])) " ns")
                    :light (get-in x [:light :median])
                    :ratio (:ratio x)})
                 (for [x (take 5 (get-in res [:runtime :extend-ops :all-slower]))]
                   {:id (keyword (str "extend-op-" (:op x)))
                    :area "extend-ops"
                    :metric (:op x)
                    :light-text (str (fmt-ns (get-in x [:light :median])) " ns")
                    :shadow-text (str (fmt-ns (get-in x [:shadow :median])) " ns")
                    :light (get-in x [:light :median])
                    :ratio (:ratio x)}))]
    (vec (sort-by (comp - :ratio) (concat base-gaps op-gaps)))))

;; ---------------------------------------------------------------------------
;; Markdown scorecard renderer

(defn- env-summary-line [{:keys [light-cljs shadow-cljs jdk node babashka os machine date]}]
  (str "light-cljs `" (or (:commit light-cljs) "unknown") "`"
       " (`" (fs/file-name (:binary light-cljs)) "`, sha256 `" (subs (:sha256 light-cljs) 0 12) "`)"
       ", shadow-cljs " shadow-cljs ", " jdk ", Node " node ", Babashka " babashka
       " — " (:cpu machine) ", " (:cpus machine) " CPUs, " (:memory-gib machine) " GiB, " os ", " date "."))

(defn collect-load-range
  "Find [min max] 1-minute load average across all recorded spreads in res."
  [res]
  (let [spreads (keep (fn [node]
                        (when (and (map? node) (not (sorted? node)))
                          (let [l (:load-1m node)]
                            (when (and (map? l) (number? (:min l)) (number? (:max l)))
                              l))))
                      (tree-seq coll? seq res))]
    (when (seq spreads)
      [(apply min (map :min spreads)) (apply max (map :max spreads))])))

(defn render-markdown
  "Render complete gap-bench Markdown report from result map and optional baseline EDN."
  [res baseline-file baseline]
  (let [rows (scorecard-rows res)
        base-by-id (when baseline (into {} (map (juxt :id identity) (scorecard-rows baseline))))
        gaps (extract-gaps res)
        base-gaps (when baseline (into {} (map (juxt :id identity) (extract-gaps baseline))))
        load-range (collect-load-range res)
        c (:compile res)
        w (:watch res)
        sz (:size res)
        rt (:runtime res)
        out (atom [])
        emit! (fn [& lines] (swap! out into lines))]

    (emit! "# light-cljs vs shadow-cljs Gap Benchmark"
           ""
           (env-summary-line (:environment res))
           (str "Slices: `" (str/join "," (map name (:slices res))) "` | "
                "Shadow mode: `" (name (get-in res [:options :shadow-mode])) "` | "
                "Total wall time: **" (format "%.1f s (%.1f min)" (:total-elapsed-s res) (/ (:total-elapsed-s res) 60.0)) "**"
                (when-let [[lo hi] load-range]
                  (str " | 1m load: `" (format "%.2f–%.2f" (double lo) (double hi)) "`"
                       (when (> hi 2.0) " *(unreserved: load > 2.0)*")))
                (when baseline-file
                  (str " | Diff vs `" (fs/file-name baseline-file) "` (`"
                       (or (get-in baseline [:environment :light-cljs :commit]) "?") "`)")))
           "")

    ;; Executive Scorecard
    (emit! "## Executive Scorecard (lower is better; `< 1.00x` = light-cljs wins)"
           ""
           (str "| Area | Metric | light-cljs | shadow-cljs | light/shadow |"
                (when base-by-id " before | Δ light |"))
           (str "| --- | --- | ---: | ---: | ---: |"
                (when base-by-id " ---: | ---: |")))
    (doseq [{:keys [id area metric light shadow ratio fmt]} rows
            :let [b (get base-by-id id)
                  s-txt (if (= fmt :ratio-only) "1.00x" (fmt-metric shadow fmt))]]
      (emit! (str "| " area " | " metric " | " (fmt-metric light fmt) " | " s-txt " | **" (fmt-ratio ratio) "** |"
                  (when base-by-id
                    (if b
                      (str " " (fmt-metric (:light b) fmt) " | " (fmt-delta light (:light b)) " |")
                      " - | - |")))))
    (emit! "")

    ;; Remaining Gaps
    (emit! (str "## Remaining Gaps Ranked (> " (format "%.2fx" gap-threshold) " shadow-cljs)")
           "")
    (if (empty? gaps)
      (emit! "No measured gaps exceed 1.05x shadow-cljs!" "")
      (do
        (emit! (str "| # | Area | Metric | light-cljs | shadow-cljs | light/shadow |"
                    (when base-gaps " before | Δ |"))
               (str "| ---: | --- | --- | ---: | ---: | ---: |"
                    (when base-gaps " ---: | ---: |")))
        (doseq [[idx g] (map-indexed vector gaps)
                :let [bg (get base-gaps (:id g))]]
          (emit! (str "| " (inc idx) " | " (:area g) " | " (:metric g) " | " (:light-text g)
                      " | " (:shadow-text g) " | **" (fmt-ratio (:ratio g)) "** |"
                      (when base-gaps
                        (if bg
                          (str " " (fmt-ratio (:ratio bg)) " | " (fmt-delta (:light g) (:light bg)) " |")
                          " - | - |")))))
        (emit! "")))

    ;; Section 1: Compile
    (when c
      (let [lc (get-in c [:light :clean])
            lcf (get-in c [:light :cache-fill])
            lwu (get-in c [:light :warm-unchanged])
            lwl (get-in c [:light :warm-leaf])
            lwd (get-in c [:light :warm-decl])
            sc (get-in c [:shadow :clean])
            swu (get-in c [:shadow :warm-unchanged])
            swl (get-in c [:shadow :warm-leaf])
            sc50 (get-in c [:light :scaling])
            st (:stats-median lc)]
        (emit! (str "## 1. Compiler Performance, Cache, Memory & Scaling (" (format "%.1fs" (:elapsed-s c)) ")")
               ""
               (str "Project: `re-frame-200k` (" (fmt-int (:lines-200k sc50)) " nonblank lines, seed 42), "
                    "shadow-cljs capped at `-Xmx" (get-in c [:shadow :shadow-heap]) "`"
                    (when (get-in c [:shadow :reused]) " *(cached shadow baseline)*") ".")
               ""
               "| Scenario | light wall | shadow wall | light/shadow | light CPU | shadow CPU | light RSS | shadow RSS |"
               "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |"
               (format "| Clean (`--no-cache`) | %s | %s | %s | %s | %s | %s | %s |"
                       (fmt-num (get-in lc [:wall-s :median]) 2 "s")
                       (fmt-num (get-in sc [:wall-s :median]) 2 "s")
                       (fmt-ratio (ratio (get-in lc [:wall-s :median]) (get-in sc [:wall-s :median])))
                       (fmt-num (get-in lc [:cpu-s :median]) 2 "s")
                       (fmt-num (get-in sc [:cpu-s :median]) 2 "s")
                       (fmt-num (get-in lc [:peak-memory-gb :median]) 3 "GB")
                       (fmt-num (get-in sc [:peak-memory-gb :median]) 3 "GB"))
               (format "| Cold cache-fill | %s | - | - | %s | - | %s | - |"
                       (fmt-num (get-in lcf [:wall-s :median]) 2 "s")
                       (fmt-num (get-in lcf [:cpu-s :median]) 2 "s")
                       (fmt-num (get-in lcf [:peak-memory-gb :median]) 3 "GB"))
               (format "| Warm unchanged | %s | %s | %s | %s | %s | %s | %s |"
                       (fmt-num (get-in lwu [:wall-s :median]) 2 "s")
                       (fmt-num (get-in swu [:wall-s :median]) 2 "s")
                       (fmt-ratio (ratio (get-in lwu [:wall-s :median]) (get-in swu [:wall-s :median])))
                       (fmt-num (get-in lwu [:cpu-s :median]) 2 "s")
                       (fmt-num (get-in swu [:cpu-s :median]) 2 "s")
                       (fmt-num (get-in lwu [:peak-memory-gb :median]) 3 "GB")
                       (fmt-num (get-in swu [:peak-memory-gb :median]) 3 "GB"))
               (format "| Warm leaf-body edit | %s | %s | %s | %s | %s | %s | %s |"
                       (fmt-num (get-in lwl [:wall-s :median]) 2 "s")
                       (fmt-num (get-in swl [:wall-s :median]) 2 "s")
                       (fmt-ratio (ratio (get-in lwl [:wall-s :median]) (get-in swl [:wall-s :median])))
                       (fmt-num (get-in lwl [:cpu-s :median]) 2 "s")
                       (fmt-num (get-in swl [:cpu-s :median]) 2 "s")
                       (fmt-num (get-in lwl [:peak-memory-gb :median]) 3 "GB")
                       (fmt-num (get-in swl [:peak-memory-gb :median]) 3 "GB"))
               (format "| Warm declaration edit (`defprotocol`) | %s | - | - | %s | - | %s | - |"
                       (fmt-num (get-in lwd [:wall-s :median]) 2 "s")
                       (fmt-num (get-in lwd [:cpu-s :median]) 2 "s")
                       (fmt-num (get-in lwd [:peak-memory-gb :median]) 3 "GB"))
               ""
               (str "- **Cache-fill overhead:** " (format "%+.2f s" (double (:overhead-s lcf)))
                    " vs clean; cache disk size: **" (fmt-mb (:disk-bytes lcf) :from-bytes) "**")
               (str "- **Scaling canary (`50k -> 200k`, " (fmt-int (:lines-50k sc50)) " -> " (fmt-int (:lines-200k sc50)) " lines):** "
                    "wall `" (fmt-num (:wall-50k-s sc50) 2 "s") " -> " (fmt-num (:wall-200k-s sc50) 2 "s") "` "
                    "(exponent **" (fmt-num (:wall-exp sc50) 2) "**), CPU exp `" (fmt-num (:cpu-exp sc50) 2)
                    "`, memory exp `" (fmt-num (:mem-exp sc50) 2) "` (1.00 = linear)")
               (str "- **Clean determinism:** " (if (get-in c [:light :deterministic?]) "PASS" "**FAIL**")
                    " (`" (subs (first (get-in c [:light :clean-hashes])) 0 16) "...`) | "
                    "Oracle `" (:expected-checksum c) "` + `check-waste.bb --strict`: **PASS**")
               (when (seq st)
                 (str "- **light-cljs clean `--stats` phases (median ms):** "
                      "resolve/parse `" (fmt-int (st "time.resolve-parse-ms")) "`, "
                      "analysis `" (fmt-int (st "time.analysis-ms")) "`, "
                      "deferred `" (fmt-int (st "time.deferred-units-ms")) "`, "
                      "prepare `" (fmt-int (st "time.prepare-application-ms")) "`, "
                      "emit `" (fmt-int (st "time.emission-ms")) "`, "
                      "write `" (fmt-int (st "time.write-ms")) "`"))
               "")))

    ;; Section 2: Watch
    (when (and w (not (:skipped w)))
      (let [lw (:light w) sw (:shadow w)]
        (emit! (str "## 2. Watch & Hot-Reload — re-frame-200k in Headless Chromium (" (format "%.1fs" (:elapsed-s w)) ")")
               ""
               (str (:watch-edits w) " timed edits per scenario (after 1 warm-up), 250–650 ms jittered pauses"
                    (when (:reused sw) "; *shadow-cljs from cached baseline*") ".")
               ""
               "| Metric | light-cljs | shadow-cljs | light/shadow |"
               "| --- | ---: | ---: | ---: |"
               (format "| Startup to first build | %.1f s | %.1f s | %s |"
                       (/ (:startup-ms lw) 1000.0) (/ (:startup-ms sw) 1000.0)
                       (fmt-ratio (ratio (:startup-ms lw) (:startup-ms sw))))
               (format "| Startup CPU (user + sys) | %.1f s | %.1f s | %s |"
                       (:startup-cpu-s lw) (:startup-cpu-s sw)
                       (fmt-ratio (ratio (:startup-cpu-s lw) (:startup-cpu-s sw))))
               (format "| Dev page load in Chromium | %.2f s | %.2f s | %s |"
                       (/ (:page-load-ms lw) 1000.0) (/ (:page-load-ms sw) 1000.0)
                       (fmt-ratio (ratio (:page-load-ms lw) (:page-load-ms sw))))
               (format "| Peak memory (startup / editing) | %.0f / %.0f MB | %.0f / %.0f MB | %s |"
                       (/ (:startup-peak-bytes lw) 1e6) (/ (:steady-peak-bytes lw) 1e6)
                       (/ (:startup-peak-bytes sw) 1e6) (/ (:steady-peak-bytes sw) 1e6)
                       (fmt-ratio (ratio (:steady-peak-bytes lw) (:steady-peak-bytes sw))))
               ""
               "| Scenario | light running | shadow running | light/shadow | light step | shadow step | light breakdown (notice / step / push / apply) | light CPU/edit |"
               "| --- | ---: | ---: | ---: | ---: | ---: | --- | ---: |")
        (doseq [sc (:scenario-order w)
                :let [le (get-in lw [:scenarios sc])
                      se (get-in sw [:scenarios sc])]]
          (emit! (format "| %s | %s | %s | **%s** | %s | %s | %s / %s / %s / %s | %s |"
                         sc
                         (fmt-ms (get-in le [:running-ms :median]))
                         (fmt-ms (get-in se [:running-ms :median]))
                         (fmt-ratio (ratio (get-in le [:running-ms :median]) (get-in se [:running-ms :median])))
                         (fmt-ms (get-in le [:step-ms :median]))
                         (fmt-ms (get-in se [:step-ms :median]))
                         (fmt-int (get-in le [:noticing-ms :median]))
                         (fmt-int (get-in le [:step-ms :median]))
                         (fmt-int (get-in le [:output-to-msg-ms :median]))
                         (fmt-int (get-in le [:msg-to-running-ms :median]))
                         (fmt-num (get-in le [:cpu-s :median]) 2 "s"))))
        (emit! "")))

    ;; Section 3: Size
    (when sz
      (emit! (str "## 3. Output Size & Runtime Slice (" (format "%.1fs" (:elapsed-s sz)) ")")
             ""
             "| App | light app gz | shadow app gz | app gz ratio | light whole br | shadow whole br | whole br ratio | light runtime slice |"
             "| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |")
      (doseq [[app v] (:apps sz)
              :let [la (get-in v [:light :app :gzip])
                    sa (get-in v [:shadow :app :gzip])
                    lb (get-in v [:light :total :brotli])
                    sb (get-in v [:shadow :total :brotli])
                    sr (get-in v [:light :size-report])
                    sr-txt (if sr
                             (str (:units-kept sr) "/" (:units-total sr) " units (" (fmt-bytes-kb (:runtime-bytes sr)) ")")
                             "-")]]
        (emit! (format "| %s | %s | %s | **%s** | %s | %s | %s | %s |"
                       app (fmt-bytes-kb la) (fmt-bytes-kb sa) (fmt-ratio (ratio la sa))
                       (fmt-bytes-kb lb) (fmt-bytes-kb sb) (fmt-ratio (ratio lb sb)) sr-txt)))
      (when-let [e2015 (get-in sz [:apps "todomvc" :light :es2015])]
        (let [e-esnext (get-in sz [:apps "todomvc" :light :total])]
          (emit! ""
                 (str "- **TodoMVC ES2015-lowered (`--target=es2015`):** "
                      (fmt-bytes-kb (:raw e2015)) " raw / " (fmt-bytes-kb (:gzip e2015)) " gz / "
                      (fmt-bytes-kb (:brotli e2015)) " br "
                      "(vs default esbuild: " (fmt-delta (:gzip e2015) (:gzip e-esnext)) " gz)"))))
      (emit! ""))

    ;; Section 4: Runtime
    (when rt
      (emit! (str "## 4. Runtime Speed, Startup Breakdown & Runtime Memory (" (format "%.1fs" (:elapsed-s rt)) ")")
             "")
      ;; 4a. Startup phase breakdown
      (let [ls (get-in rt [:startup :light])
            ss (get-in rt [:startup :shadow])]
        (emit! "### 4a. `re-frame-200k` Startup Phase Breakdown (`startup-probe.mjs`, Node)"
               ""
               "| Phase | light-cljs | shadow-cljs | light/shadow |"
               "| --- | ---: | ---: | ---: |"
               (format "| V8 parse & compile | %s | %s | %s |"
                       (fmt-ms (get-in ls [:compile-ms :median]))
                       (fmt-ms (get-in ss [:compile-ms :median]))
                       (fmt-ratio (ratio (get-in ls [:compile-ms :median]) (get-in ss [:compile-ms :median]))))
               (format "| Runtime units init | %s | *(combined below)* | - |"
                       (fmt-ms (get-in ls [:runtime-ms :median])))
               (format "| Top-level defs init | %s | %s | - |"
                       (fmt-ms (get-in ls [:top-level-ms :median]))
                       (fmt-ms (get-in ss [:top-level-ms :median])))
               (format "| **Module load total** | **%s** | **%s** | **%s** |"
                       (fmt-ms (get-in ls [:load-total-ms :median]))
                       (fmt-ms (get-in ss [:load-total-ms :median]))
                       (fmt-ratio (ratio (get-in ls [:load-total-ms :median]) (get-in ss [:load-total-ms :median]))))
               (format "| First iteration | %s | %s | %s |"
                       (fmt-ms (get-in ls [:first-iter-ms :median]))
                       (fmt-ms (get-in ss [:first-iter-ms :median]))
                       (fmt-ratio (ratio (get-in ls [:first-iter-ms :median]) (get-in ss [:first-iter-ms :median]))))
               (format "| **Total (load + 1st iter)** | **%s** | **%s** | **%s** |"
                       (fmt-ms (get-in ls [:total-ms :median]))
                       (fmt-ms (get-in ss [:total-ms :median]))
                       (fmt-ratio (ratio (get-in ls [:total-ms :median]) (get-in ss [:total-ms :median]))))
               ""))

      ;; 4b. re-frame-200k Node throughput
      (let [nc (get-in rt [:node-200k :counts])
            st (get-in rt [:node-200k :steady-ms-per-iter])]
        (emit! "### 4b. `re-frame-200k` Node Throughput & Peak RSS"
               ""
               "| Iterations | light first-out | shadow first-out | light wall | shadow wall | wall ratio | light RSS | shadow RSS | RSS ratio |"
               "| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |")
        (doseq [[n d] nc
                :let [l (:light d) s (:shadow d)]]
          (emit! (format "| %d | %s | %s | %s | %s | %s | %s | %s | %s |"
                         n
                         (fmt-num (get-in l [:first-output-s :median]) 3 "s")
                         (fmt-num (get-in s [:first-output-s :median]) 3 "s")
                         (fmt-num (get-in l [:wall-s :median]) 3 "s")
                         (fmt-num (get-in s [:wall-s :median]) 3 "s")
                         (fmt-ratio (ratio (get-in l [:wall-s :median]) (get-in s [:wall-s :median])))
                         (fmt-mb (get-in l [:peak-memory-gb :median]) :from-gb)
                         (fmt-mb (get-in s [:peak-memory-gb :median]) :from-gb)
                         (fmt-ratio (ratio (get-in l [:peak-memory-gb :median]) (get-in s [:peak-memory-gb :median]))))))
        (let [ks (sort (keys nc))]
          (emit! ""
                 (format "- **Steady-state per iteration (%d -> %d):** light `%.1f ms/iter` vs shadow `%.1f ms/iter` (**%s**)"
                         (first ks) (last ks)
                         (double (:light st)) (double (:shadow st)) (fmt-ratio (ratio (:light st) (:shadow st))))
                 "")))

      ;; 4c. TodoMVC Chromium
      (when-let [td (:todomvc rt)]
        (when-not (:skipped td)
          (let [lt (:light td) st (:shadow td)
                bc (:behavior-checks td)]
            (emit! (str "### 4c. TodoMVC in Headless Chromium (" (:todos td) " todos, "
                        (:passed bc) "/" (:total bc) " behavior checks PASS, 0 errors)")
                   ""
                   "| Step / Metric | light-cljs | shadow-cljs | light/shadow | light script | shadow script |"
                   "| --- | ---: | ---: | ---: | ---: | ---: |")
            (doseq [[k label] [[:load "Page load to interactive"]
                               [:add (str "Add " (:todos td) " todos")]
                               [:toggleAll "Toggle all"]
                               [:edit "Edit middle todo"]
                               [:clearCompleted "Clear completed"]]]
              (let [lm (get-in lt [:steps k :ms :median])
                    sm (get-in st [:steps k :ms :median])]
                (emit! (format "| %s | %s | %s | **%s** | %s | %s |"
                               label (fmt-ms lm) (fmt-ms sm) (fmt-ratio (ratio lm sm))
                               (fmt-ms (get-in lt [:steps k :script-ms :median]))
                               (fmt-ms (get-in st [:steps k :script-ms :median]))))))
            (let [hl (get-in lt [:heap-after-load-bytes :median])
                  hs (get-in st [:heap-after-load-bytes :median])
                  al (get-in lt [:heap-after-add-bytes :median])
                  as (get-in st [:heap-after-add-bytes :median])]
              (emit! (format "| JS heap after load | %s | %s | %s | - | - |"
                             (fmt-mb hl :from-bytes) (fmt-mb hs :from-bytes) (fmt-ratio (ratio hl hs)))
                     (format "| JS heap after %d todos | %s | %s | **%s** | - | - |"
                             (:todos td) (fmt-mb al :from-bytes) (fmt-mb as :from-bytes) (fmt-ratio (ratio al as)))
                     "")))))

      ;; 4d. core-ops
      (when-let [co (:core-ops rt)]
        (emit! (format "### 4d. `core-ops` (%d microbenchmarks, all checksums matched)" (:count co))
               ""
               (format "- **Geomean:** **%s** | **Median:** %s | **At parity (<=%.2fx):** %d/%d"
                       (fmt-ratio (:geomean co)) (fmt-ratio (:median-ratio co)) gap-threshold
                       (:parity-count co) (:count co))
               ""
               "| Group | Ops | Parity (<=1.05x) | Geomean | Worst |"
               "| --- | ---: | ---: | ---: | ---: |")
        (doseq [{:keys [group count parity geomean max-ratio]} (:groups co)]
          (emit! (format "| %s | %d | %d/%d | %s | %s |"
                         group count parity count (fmt-ratio geomean) (fmt-ratio max-ratio))))
        (when (seq (:top-slower co))
          (emit! ""
                 "**Top slower `core-ops` (> 1.05x):**"
                 ""
                 "| Operation | light ns/op | shadow ns/op | light/shadow |"
                 "| --- | ---: | ---: | ---: |")
          (doseq [x (:top-slower co)]
            (emit! (format "| `%s` | %s | %s | **%s** |"
                           (:op x) (fmt-ns (get-in x [:light :median]))
                           (fmt-ns (get-in x [:shadow :median])) (fmt-ratio (:ratio x))))))
        (emit! ""))

      ;; 4e. extend-ops
      (when-let [eo (:extend-ops rt)]
        (emit! (format "### 4e. `extend-ops` (%d `extend-type`-active microbenchmarks, all checksums matched)" (:count eo))
               ""
               (format "- **Geomean:** **%s** | **Median:** %s | **At parity (<=%.2fx):** %d/%d"
                       (fmt-ratio (:geomean eo)) (fmt-ratio (:median-ratio eo)) gap-threshold
                       (:parity-count eo) (:count eo))
               ""
               "| Operation | light ns/op | shadow ns/op | light/shadow |"
               "| --- | ---: | ---: | ---: |")
        (doseq [op (:ops eo)
                :let [x (get-in eo [:by-op op])]]
          (emit! (format "| `%s` | %s | %s | %s |"
                         op (fmt-ns (get-in x [:light :median]))
                         (fmt-ns (get-in x [:shadow :median])) (fmt-ratio (:ratio x)))))
        (emit! "")))

    (str/join "\n" @out)))

;; ---------------------------------------------------------------------------
;; Main orchestrator

(defn main [args]
  (let [opts (parse-options args)]
    (when (:help opts)
      (println usage)
      (System/exit 0))
    (let [t-total (now-ms)
          light (resolve-light-binary opts)
          research-dir (resolve-research-dir opts)
          work-dir (or (:work-dir opts) (str (fs/path here "work")))
          shadow-baseline-path (or (:shadow-baseline opts) (str (fs/path work-dir "shadow-baseline.edn")))
          shadow-artifacts-dir (str (fs/path work-dir "shadow-artifacts"))
          slices (set (:only opts))
          _ (fs/create-dirs work-dir)

          ;; Stage only the assets needed by selected slices
          npm-tools (when (or (slices :watch) (slices :size) (and (slices :runtime) (not (:skip-browser opts))))
                      (ensure-npm-tools! research-dir work-dir))
          esbuild (when npm-tools (str (fs/path npm-tools "node_modules/.bin/esbuild")))
          dir-200k (when (or (slices :compile) (slices :size) (slices :runtime))
                     (ensure-generated-staged!
                      (fs/path research-dir "shadow-credibility/projects/re-frame-200k-seed42")
                      (fs/path work-dir "re-frame-200k")))
          dir-50k (when (slices :compile)
                    (ensure-generated-staged!
                     (fs/path research-dir "shadow-credibility/projects/re-frame-50k-seed42")
                     (fs/path work-dir "re-frame-50k")))
          watch-dir (when (and (slices :watch) (not (:skip-browser opts)))
                      (ensure-watch-staged!
                       (fs/path research-dir "shadow-credibility/projects/re-frame-200k-seed42")
                       (fs/path work-dir "re-frame-200k-watch")
                       esbuild))
          rf-dir (when (or (slices :size) (and (slices :runtime) (not (:skip-browser opts))))
                   (ensure-re-frame-examples-staged! research-dir work-dir))
          ports (when (or (slices :size) (and (slices :runtime) (not (:skip-browser opts))))
                  (ensure-ports-staged! research-dir work-dir))

          ctx {:research-dir research-dir :work-dir work-dir :npm-tools npm-tools :esbuild esbuild
               :dir-200k dir-200k :dir-50k dir-50k :watch-dir watch-dir :rf-dir rf-dir :ports ports
               :shadow-artifacts-dir shadow-artifacts-dir}

          env-raw (cmp/environment {:light light :light-commit (:light-commit opts)} shadow-version-pin)
          env (if (get-in env-raw [:light-cljs :commit])
                env-raw
                (assoc-in env-raw [:light-cljs :commit] (or (git-head-short) "local")))
          commit (get-in env [:light-cljs :commit])
          opts (assoc opts :light light)
          shadow-baseline (atom (or (read-shadow-baseline shadow-baseline-path)
                                    {:schema-version 1
                                     :shadow-version shadow-version-pin
                                     :jdk (:jdk env)
                                     :node (:node env)
                                     :machine (:machine env)
                                     :slices {}}))
          persist-shadow! (fn [slice-k data]
                            (swap! shadow-baseline
                                   (fn [m]
                                     (-> m
                                         (assoc :updated-at (str (java.time.Instant/now))
                                                :shadow-version shadow-version-pin
                                                :jdk (:jdk env)
                                                :node (:node env))
                                         (assoc-in [:slices slice-k] data))))
                            (save-shadow-baseline! shadow-baseline-path @shadow-baseline))

          compile-res (when (slices :compile)
                        (let [r (run-compile-slice! opts ctx @shadow-baseline)]
                          (when-not (get-in r [:shadow :reused])
                            (persist-shadow! :compile (dissoc (:shadow r) :reused)))
                          r))
          watch-res (when (slices :watch)
                      (let [r (run-watch-slice! opts ctx @shadow-baseline)]
                        (when (and (not (:skipped r)) (not (get-in r [:shadow :reused])))
                          (persist-shadow! :watch (dissoc (:shadow r) :reused)))
                        r))
          size-res (when (slices :size)
                     (let [r (run-size-slice! opts ctx @shadow-baseline compile-res)]
                       (when (some (fn [[_ v]] (not (get-in v [:shadow :reused]))) (:apps r))
                         (persist-shadow! :size (:shadow-cache r)))
                       (dissoc r :shadow-cache)))
          runtime-res (when (slices :runtime)
                        (run-runtime-slice! opts ctx))

          total-elapsed-s (/ (- (now-ms) t-total) 1000.0)
          result (cond-> {:schema-version 1
                          :environment env
                          :options (dissoc opts :light)
                          :slices (vec (:only opts))
                          :total-elapsed-s total-elapsed-s}
                   compile-res (assoc :compile compile-res)
                   watch-res (assoc :watch watch-res)
                   size-res (assoc :size size-res)
                   runtime-res (assoc :runtime runtime-res))
          baseline-data (some-> (:baseline opts) cmp/read-edn)
          report (render-markdown result (:baseline opts) baseline-data)
          out-prefix (or (:out opts) (str (fs/path here "out" (str "gap-bench-" commit))))]
      (fs/create-dirs (fs/parent out-prefix))
      (spit (str out-prefix ".edn") (with-out-str (pprint/pprint result)))
      (spit (str out-prefix ".md") report)
      (println report)
      (cmp/log (format "Done in %.1f s (%.1f min). Wrote %s.edn and %s.md"
                       total-elapsed-s (/ total-elapsed-s 60.0) out-prefix out-prefix)))))

(when (= (fs/absolutize *file*)
         (some-> (System/getProperty "babashka.file") fs/absolutize))
  (main *command-line-args*))
