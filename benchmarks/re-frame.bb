#!/usr/bin/env bb
;; re-frame's example apps (todomvc, simple, flow) built by shadow-cljs and by
;; light-cljs, for benchmarks/re-frame-vs-shadow.md.
;;
;;   bb benchmarks/re-frame.bb setup
;;   bb benchmarks/re-frame.bb port APP [--porter DIR]
;;   bb benchmarks/re-frame.bb size APP [APP ...]
;;   bb benchmarks/re-frame.bb time APP [--runs N]
;;   bb benchmarks/re-frame.bb inventory APP
;;
;; Paths (all under research/, which is gitignored; a worktree uses copies, see
;; the shadow-comparison-credibility project notes):
;;   research/re-frame/examples/APP  the unmodified upstream example, which
;;                                   shadow-cljs builds (setup pins 3.5.3)
;;   research/APP-port               its light-cljs port, written by `port`
;;   research/shadow-port-tool       the porter (light-cljs.port), --porter
;;   research/npm-tools              esbuild, installed by setup
;; --light BIN (or LIGHT_CLJS_BINARY) names an immutable light-cljs binary.
;;
;; Builds write only below the example's out/ (shadow-release, light-release,
;; light-bundle.js and the React-only builds), never resources/public/js, so
;; the two compilers never share an output directory.
;;
;; size: one clean release build per compiler, reported raw, gzip -9 and
;; brotli -q 11. light-cljs writes ES modules that import "react" and
;; "react-dom/client"; esbuild bundles and minifies them with React, which is
;; what a user ships. React is counted by building the same package imports
;; alone with each toolchain (a shadow-cljs build of a namespace that only
;; requires them; esbuild of a module that only imports them); the app's own
;; code is the total minus that.
;;
;; time: clean release builds (shadow's .shadow-cljs/builds deleted, light-cljs
;; --no-cache), the two compilers alternated, first one first on even runs;
;; wall, user and system CPU time and peak memory from /usr/bin/time -l, and
;; the one-minute load average before each run.
(require '[babashka.fs :as fs]
         '[babashka.process :as process]
         '[cheshire.core :as json]
         '[clojure.edn :as edn]
         '[clojure.string :as str])

(def repo (fs/parent (fs/parent (fs/absolutize *file*))))
(def research (fs/path repo "research"))
(def shadow-version "3.5.3")
(def esbuild-version "0.25.10")
(def esbuild (str (fs/path research "npm-tools/node_modules/.bin/esbuild")))
(def macro-limits ["--macro-expansions" "100000000" "--macro-nodes" "4000000000"
                   "--macro-timeout-ms" "3600000" "--macro-memory-limit" "4000000000"
                   "--macro-poll-budget" "1000000000000"])

(defn option [args flag default]
  (or (second (drop-while #(not= flag %) args)) default))

(defn example [app] (fs/path research "re-frame/examples" app))
(defn port-dir [app] (fs/path research (str app "-port")))

(defn light-binary [args]
  (str (fs/absolutize (or (option args "--light" nil) (System/getenv "LIGHT_CLJS_BINARY")
                          (fs/path repo "target/release/light-cljs")))))

(defn sh! [opts & cmd]
  (let [r (apply process/sh (merge {:err :string :out :string} opts) (map str cmd))]
    (when-not (zero? (:exit r))
      (throw (ex-info (str "failed: " (str/join " " cmd) "\n" (:err r) (:out r)) {})))
    r))

;; ---------------------------------------------------------------------------
;; setup and port

(defn setup []
  (doseq [app ["todomvc" "simple" "flow"]
          :let [dir (example app)]]
    (doseq [[file pattern] [["package.json" #"(\"shadow-cljs\": \")[^\"]+"]
                            ["deps.edn" #"(thheller/shadow-cljs\s+\{:mvn/version \")[^\"]+"]]
            :let [f (fs/file dir file)]]
      (spit f (str/replace (slurp f) pattern (str "$1" shadow-version))))
    (println app "pinned to shadow-cljs" shadow-version)
    (sh! {:dir (str dir)} "npm" "install" "--no-audit" "--no-fund"))
  (fs/create-dirs (fs/path research "npm-tools"))
  (sh! {:dir (str (fs/path research "npm-tools"))} "npm" "install" "--no-audit" "--no-fund"
       (str "esbuild@" esbuild-version))
  (println "esbuild" esbuild-version "in" (str (fs/path research "npm-tools"))))

(defn port [app args]
  (let [porter (str (fs/absolutize (option args "--porter" (fs/path research "shadow-port-tool"))))
        out (str (port-dir app))
        deps (pr-str {:aliases {:light-cljs/port {:extra-deps {'light-cljs/port {:local/root porter}}}}})]
    (fs/delete-tree (fs/path (example app) ".shadow-cljs/builds"))
    (let [r (process/sh {:dir (str (example app)) :err :string :out :string}
                        "clojure" "-Sdeps" deps "-M:light-cljs/port" "-m" "shadow.cljs.devtools.cli"
                        "run" "light-cljs.port/main" ":client" out
                        "--compile" "--light-cljs" (light-binary args))]
      (println (str/join "\n" (filter #(str/includes? % "[light-cljs.port]")
                                      (str/split-lines (str (:err r) (:out r))))))
      (when-not (zero? (:exit r)) (throw (ex-info "port failed" {:exit (:exit r)}))))))

;; ---------------------------------------------------------------------------
;; builds

(defn time-l
  "Run CMD under /usr/bin/time -l in DIR: {:wall :user :sys :peak-bytes}."
  [dir & cmd]
  (let [r (apply process/sh {:dir (str dir) :err :string :out :string} "/usr/bin/time" "-l" (map str cmd))
        err (:err r)
        [_ wall user sys] (re-find #"([\d.]+) real\s+([\d.]+) user\s+([\d.]+) sys" err)
        [_ peak] (re-find #"(\d+)\s+maximum resident set size" err)]
    (when-not (zero? (:exit r))
      (throw (ex-info (str "failed: " (str/join " " cmd) "\n" (subs err (max 0 (- (count err) 3000)))) {})))
    {:wall (parse-double wall) :user (parse-double user) :sys (parse-double sys)
     :peak-bytes (parse-long peak)}))

(defn shadow-release
  "A clean shadow-cljs release of the unmodified example into out/shadow-release."
  [app]
  (let [dir (example app)]
    (fs/delete-tree (fs/path dir ".shadow-cljs/builds"))
    (fs/delete-tree (fs/path dir "out/shadow-release"))
    (assoc (time-l dir "npx" "shadow-cljs" "release" "client"
                   "--config-merge" (pr-str {:output-dir "out/shadow-release"}))
           :file (fs/path dir "out/shadow-release/client.js"))))

(defn light-release
  "A clean light-cljs release of the port into the example's out/light-release."
  [app binary]
  (let [out (fs/path (example app) "out/light-release")]
    (fs/delete-tree out)
    (let [t (time-l (port-dir app) binary "release" "client" "--no-cache" "--out-dir" out)
          chunks (json/parse-string (slurp (fs/file out "chunks.json")) true)
          files (fs/glob out "*.js")]
      (when (not= 1 (count files))
        (throw (ex-info (str "expected one output file, got " (count files) " " chunks) {})))
      (assoc t :file (first files)))))

(defn bundle
  "esbuild ENTRY with its npm packages into OUT, minified, as a user ships it;
  EXTRA adds esbuild flags (a --target)."
  [app entry out & extra]
  (apply time-l (example app) esbuild (str entry) "--bundle" "--format=esm" "--minify"
         "--define:process.env.NODE_ENV=\"production\"" (str "--outfile=" out) "--log-level=warning" extra))

(defn package-imports
  "The packages light-cljs's output imports."
  [file]
  (->> (re-seq #"(?:from|import)\s*\"([^\"./][^\"]*)\"" (slurp (fs/file file))) (map second) distinct sort vec))

(defn react-only-light [app packages]
  (let [dir (example app)
        entry (fs/path dir "out/react-only.mjs")]
    (spit (fs/file entry)
          (str (str/join (map-indexed (fn [i p] (str "import * as p" i " from " (pr-str p) ";\n")) packages))
               "console.log(" (str/join ", " (map #(str "p" %) (range (count packages)))) ");\n"))
    (bundle app entry (fs/path dir "out/light-react-only.js"))
    (fs/path dir "out/light-react-only.js")))

(defn react-only-shadow [app packages]
  (let [dir (example app)
        src (fs/path dir "out/react-only-src")
        ns-file (fs/path src "light_size/react_only.cljs")]
    (fs/create-dirs (fs/parent ns-file))
    (spit (fs/file ns-file)
          (str "(ns light-size.react-only (:require "
               (str/join " " (map-indexed (fn [i p] (str "[" (pr-str p) " :as p" i "]")) packages))
               "))\n(defn run [] (js/console.log "
               (str/join " " (map #(str "p" %) (range (count packages)))) "))\n"))
    (fs/delete-tree (fs/path dir ".shadow-cljs/builds"))
    (sh! {:dir (str dir)} "clojure" "-Sdeps" (pr-str {:aliases {:react-only {:extra-paths [(str src)]}}})
         "-M:react-only" "-m" "shadow.cljs.devtools.cli" "release" "client" "--config-merge"
         (pr-str {:output-dir "out/shadow-react-only"
                  :modules {:client {:init-fn 'light-size.react-only/run}}}))
    (fs/path dir "out/shadow-react-only/client.js")))

(defn shadow-packages
  "The npm files shadow's release build of APP included, from its manifest."
  [manifest]
  (->> (edn/read-string (slurp (fs/file manifest))) (mapcat :sources)
       (filter #(str/starts-with? (str %) "node_modules/")) sort vec))

;; ---------------------------------------------------------------------------
;; size

(defn compressed [file]
  (let [f (fs/file file)]
    {:raw (fs/size f)
     :gzip (count (:out (process/sh {:in f :out :bytes} "gzip" "-9" "-c")))
     :brotli (count (:out (process/sh {:in f :out :bytes} "brotli" "-q" "11" "-c")))}))

(defn minus [a b] (merge-with - a b))

(defn size [app binary]
  (let [dir (example app)
        shadow (shadow-release app)
        light (light-release app binary)
        packages (package-imports (:file light))
        bundle-file (fs/path dir "out/light-bundle.js")
        _ (bundle app (:file light) bundle-file)
        es2015-file (fs/path dir "out/light-bundle-es2015.js")
        _ (bundle app (:file light) es2015-file "--target=es2015")
        shadow-react (react-only-shadow app packages)
        light-react (react-only-light app packages)
        app-npm (shadow-packages (fs/path dir "out/shadow-release/manifest.edn"))
        react-npm (shadow-packages (fs/path dir "out/shadow-react-only/manifest.edn"))
        rows {:shadow-total (compressed (:file shadow))
              :shadow-react (compressed shadow-react)
              :light-total (compressed bundle-file)
              :light-es2015 (compressed es2015-file)
              :light-react (compressed light-react)
              :light-output (compressed (:file light))}]
    (when (not= app-npm react-npm)
      (throw (ex-info "the React-only shadow build includes other npm files than the app's"
                      {:app app-npm :react-only react-npm})))
    (assoc rows
           :app app :packages packages :npm-files app-npm
           :shadow-app (minus (:shadow-total rows) (:shadow-react rows))
           :light-app (minus (:light-total rows) (:light-react rows)))))

(defn print-size [{:keys [app] :as r}]
  (println (str "\n### " app "\n"))
  (println "| Output | Raw bytes | gzip -9 | brotli -q 11 |")
  (println "|---|---:|---:|---:|")
  (doseq [[k label] [[:shadow-total "shadow-cljs: whole release file"]
                     [:shadow-react "shadow-cljs: React alone"]
                     [:shadow-app "shadow-cljs: app code (difference)"]
                     [:light-total "light-cljs + esbuild: whole bundle"]
                     [:light-react "esbuild: React alone"]
                     [:light-app "light-cljs: app code (difference)"]
                     [:light-output "light-cljs output as written, without React"]
                     [:light-es2015 "light-cljs + esbuild --target=es2015: whole bundle"]]
          :let [{:keys [raw gzip brotli]} (get r k)]]
    (println (format "| %s | %,d | %,d | %,d |" label raw gzip brotli)))
  (println (str "\nPackages imported: " (str/join ", " (:packages r))
                "; npm files in both shadow builds: " (count (:npm-files r)))))

;; ---------------------------------------------------------------------------
;; origins: output bytes by source file, from each compiler's source map

(def vlq-digits (into {} (map-indexed (fn [i c] [c i]) "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/")))

(defn vlq-decode [^String s]
  (loop [cs (seq s) shift 0 acc 0 out []]
    (if-let [c (first cs)]
      (let [d (vlq-digits c) v (+ acc (bit-shift-left (bit-and d 31) shift))]
        (if (pos? (bit-and d 32))
          (recur (rest cs) (+ shift 5) v out)
          (recur (rest cs) 0 0 (conj out (if (odd? v) (- (bit-shift-right v 1)) (bit-shift-right v 1))))))
      out)))

(defn- attribute-section
  "Add to ACC the characters of LINES (a section of the output) attributed by
  the source map M: a segment owns the text up to the next one."
  [acc m lines]
  (let [sources (get m "sources")]
    (loop [groups (str/split (get m "mappings" "") #";" -1) lines lines src 0 acc acc]
      (if-let [line (first lines)]
        (let [segs (->> (str/split (or (first groups) "") #",") (remove str/blank?) (map vlq-decode))
              [cols _ src] (reduce (fn [[cols col src] seg]
                                     (let [col (+ col (first seg))
                                           src (if (> (count seg) 1) (+ src (second seg)) src)]
                                       [(conj cols [col (when (> (count seg) 1) src)]) col src]))
                                   [[] 0 src] segs)
              ends (concat (map first (rest cols)) [(count line)])
              acc (reduce (fn [acc [[col s] end]]
                            (let [k (if s (nth sources s) :unmapped)]
                              (assoc acc k (+ (get acc k 0) (max 0 (- end col))))))
                          (update acc :unmapped (fnil + 0) (if (seq cols) (ffirst cols) (count line)))
                          (map vector cols ends))]
          (recur (rest groups) (rest lines) src acc))
        acc))))

(defn bytes-by-source
  "Characters of JS-FILE attributed to each source of its map, sectioned or
  not; text no segment names is :unmapped."
  [js-file map-file]
  (let [m (json/parse-string (slurp (fs/file map-file)))
        lines (vec (str/split (slurp (fs/file js-file)) #"\n" -1))]
    (if-let [sections (get m "sections")]
      (let [starts (map #(get-in % ["offset" "line"]) sections)
            ends (concat (rest starts) [(count lines)])]
        (reduce (fn [acc [sec a b]] (attribute-section acc (get sec "map") (subvec lines a b)))
                {:unmapped (reduce + (map count (subvec lines 0 (first starts))))}
                (map vector sections starts ends)))
      (attribute-section {} m lines))))

(defn origins
  "Release builds with source maps, attributed by source: shadow-cljs's output
  of the example and light-cljs's output of the port."
  [app binary]
  (let [dir (example app)
        light-js (fs/path dir "out/light-mapped/app.js")]
    (fs/delete-tree (fs/path dir ".shadow-cljs/builds"))
    (sh! {:dir (str dir)} "npx" "shadow-cljs" "release" "client" "--config-merge"
         (pr-str {:output-dir "out/shadow-mapped" :compiler-options {:source-map true}}))
    (fs/create-dirs (fs/parent light-js))
    (let [entry (-> (edn/read-string (slurp (fs/file (port-dir app) "port-report.edn"))) :builds :client first)
          r (apply process/sh {:dir (str (port-dir app)) :err :string :out :string}
                   binary "--compile" "--optimize" "advanced" "--entry" (str entry)
                   "--source-root" "src" "--source-root" "vendor" "--source-root" "compat" "--esm" "--no-cache"
                   "--source-map" (str light-js ".map")
                   macro-limits)]
      (when-not (zero? (:exit r)) (throw (ex-info (:err r) {})))
      (spit (fs/file light-js) (:out r)))
    {:shadow (bytes-by-source (fs/path dir "out/shadow-mapped/client.js") (fs/path dir "out/shadow-mapped/client.js.map"))
     :light (bytes-by-source light-js (str light-js ".map"))}))

(defn origin-group
  "A source path's group: npm, Closure Library, cljs.core, compat shim, or the
  library or app directory it comes from."
  [path]
  (let [p (str/replace (str path) #"^(\.\./)+" "")]
    (cond (keyword? path) "(no source)"
          (str/includes? p "node_modules/") "npm (React)"
          (re-find #"(^|/)goog/" p) (if (str/includes? p "compat/") "compat: goog shims" "Closure Library (goog)")
          (re-find #"(^|/)cljs/core\.clj|cljs/core/" p) "cljs.core"
          (str/includes? p "compat/") "compat: other shims"
          :else (let [p (str/replace p #"^.*?(src|vendor)/" "")]
                  (str/join "/" (take 2 (str/split p #"/")))))))

(defn print-origins [app {:keys [shadow light]}]
  (let [group (fn [m] (reduce-kv (fn [acc k v] (update acc (origin-group k) (fnil + 0) v)) {} m))
        gs (group shadow) gl (group light)]
    (println (str "\n### " app ": output characters by origin (source maps)\n"))
    (println "| Origin | shadow-cljs | light-cljs |")
    (println "|---|---:|---:|")
    (doseq [k (sort-by #(- (max (gs % 0) (gl % 0))) (distinct (concat (keys gs) (keys gl))))]
      (println (format "| %s | %,d | %,d |" k (gs k 0) (gl k 0))))
    (println (format "| total | %,d | %,d |" (reduce + (vals gs)) (reduce + (vals gl))))
    (println (str "\nlight-cljs compat/ files: "
                  (str/join ", " (for [[k v] (sort-by key (filter (comp string? key) light)) :when (str/includes? k "compat/")]
                                   (format "%s %,d" (str/replace k #"^.*compat/" "") v)))))))

;; ---------------------------------------------------------------------------
;; time

(defn load-average []
  (-> (sh! {} "sysctl" "-n" "vm.loadavg") :out str/trim (str/split #"\s+") second parse-double))

(defn median [xs] (let [s (vec (sort xs)) n (count s)]
                    (if (odd? n) (s (quot n 2)) (/ (+ (s (dec (quot n 2))) (s (quot n 2))) 2.0))))

(defn summary [runs k]
  (let [xs (map k runs)] (format "%.2f (%.2f-%.2f)" (double (median xs)) (double (apply min xs)) (double (apply max xs)))))

(defn timing [app binary runs]
  (let [jobs {:shadow #(shadow-release app)
              :light #(light-release app binary)}
        results (atom {})]
    (doseq [i (range runs)
            job (if (even? i) [:shadow :light] [:light :shadow])]
      (let [load (load-average)
            t ((jobs job))]
        (when (= job :light)
          (let [entry (first (fs/glob (fs/path (example app) "out/light-release") "*.js"))
                l2 (load-average)
                b (bundle app entry (fs/path (example app) "out/light-bundle.js"))]
            (swap! results update :esbuild (fnil conj []) (assoc b :load l2))))
        (swap! results update job (fnil conj []) (assoc (dissoc t :file) :load load))
        (binding [*out* *err*]
          (println (format "run %d %-7s load %.2f wall %.2f s user %.2f s sys %.2f s peak %.0f MB"
                           i (name job) load (:wall t) (:user t) (:sys t) (/ (:peak-bytes t) 1e6))))))
    (println (str "\n### " app ", " runs " runs each\n"))
    (println "| Build | Wall s | User CPU s | System CPU s | Peak memory MB | Load average before |")
    (println "|---|---:|---:|---:|---:|---:|")
    (doseq [[k label] [[:shadow "shadow-cljs release (npx, JVM start included)"]
                       [:light "light-cljs release"]
                       [:esbuild "esbuild bundle of light-cljs's output"]]
            :let [rs (get @results k)]]
      (println (format "| %s | %s | %s | %s | %s | %s |" label (summary rs :wall) (summary rs :user)
                       (summary rs :sys) (summary (map #(update % :peak-bytes / 1e6) rs) :peak-bytes)
                       (summary rs :load))))
    @results))

;; ---------------------------------------------------------------------------
;; inventory

(defn inventory
  "Every file of the port's compat/ with its size, and the port report's
  rewrites: macro classes, config dispositions, missing goog names."
  [app]
  (let [dir (port-dir app)
        report (edn/read-string (slurp (fs/file dir "port-report.edn")))]
    (println (str "\n### " app ": compat/ files\n"))
    (println "| File | Bytes |")
    (println "|---|---:|")
    (doseq [f (sort (map str (fs/glob (fs/path dir "compat") "**.{cljs,cljc,js}")))]
      (println (format "| %s | %,d |" (str (fs/relativize (fs/path dir "compat") f)) (fs/size f))))
    (println (str "\n### " app ": macros not kept as source\n"))
    (println "| Macro | Class | Call sites | Reason |")
    (println "|---|---|---:|---|")
    (doseq [[m {:keys [class sites reasons]}] (:macros report) :when (not= :keep class)]
      (println (format "| %s | %s | %d | %s |" m (name class) sites (str/join "; " reasons))))
    (println (str "\n### " app ": configuration keys not kept\n"))
    (doseq [{:keys [key disposition why]} (:config report) :when (not= :kept disposition)]
      (println (format "- %s %s: %s" key (name disposition) why)))
    (println (str "\nReport: " (pr-str (select-keys (:report report)
                                                    [:files :spliced :patches :missing-goog :unprintable
                                                     :unsafe :splice-mismatch]))))))

;; ---------------------------------------------------------------------------

(let [[command & args] *command-line-args*
      apps (take-while #(not (str/starts-with? % "--")) args)]
  (case command
    "setup" (setup)
    "port" (doseq [app apps] (port app args))
    "size" (doseq [app apps] (print-size (size app (light-binary args))))
    "time" (doseq [app apps] (timing app (light-binary args) (parse-long (option args "--runs" "5"))))
    "inventory" (doseq [app apps] (inventory app))
    "origins" (doseq [app apps] (print-origins app (origins app (light-binary args))))
    (do (println "usage: bb benchmarks/re-frame.bb setup | port APP... | size APP... | time APP... [--runs N] | inventory APP...  [--light BIN] [--porter DIR]")
        (System/exit 1))))
