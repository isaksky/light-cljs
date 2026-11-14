#!/usr/bin/env bb
;; re-frame's example apps (todomvc, simple, flow) built by shadow-cljs and by
;; light-cljs, for benchmarks/re-frame-vs-shadow.md.
;;
;;   bb benchmarks/re-frame.bb setup
;;   bb benchmarks/re-frame.bb port APP [--porter DIR]
;;   bb benchmarks/re-frame.bb size APP [APP ...]
;;   bb benchmarks/re-frame.bb time APP [--runs N]
;;   bb benchmarks/re-frame.bb inventory APP
;;   bb benchmarks/re-frame.bb breakdown APP
;;   bb benchmarks/re-frame.bb bundle APP [--compiler shadow|light]
;;   bb benchmarks/re-frame.bb browser APP [--compiler C] [--runs N] [--no-build]
;;   bb benchmarks/re-frame.bb runtime todomvc [--runs N] [--todos N] [--no-build]
;;
;; Paths (all under research/, which is gitignored; a worktree uses copies, see
;; the shadow-comparison-credibility project notes):
;;   research/re-frame/examples/APP  the unmodified upstream example, which
;;                                   shadow-cljs builds (setup pins 3.5.3)
;;   research/APP-port               its light-cljs port, written by `port`
;;   research/shadow-port-tool       the porter (light-cljs.port), --porter
;;   research/npm-tools              esbuild and playwright-core, installed by setup
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
;;
;; bundle: each compiler's clean release build, served the way a user ships
;; it: out/browser-COMPILER is the example's own resources/public with
;; js/client.js replaced (see bundle-browser). Other kits reuse these
;; directories and benchmarks/re-frame-browser.mjs's serve/launch/openApp.
;;
;; browser: bundle, then run re-frame-browser.mjs's checks N times (default 3)
;; per compiler in headless Chromium: the TodoMVC specification's behaviours,
;; simple's clock and colour input, and flow's buttons, whose page text after
;; each step must equal shadow-cljs's. Prints a pass/fail table and every
;; failure, console error and uncaught exception.
;;
;; runtime: bundle, then time each build in headless Chromium with
;; benchmarks/re-frame-runtime.mjs, N runs (default 10) per compiler, alternated:
;; page load to interactive, adding --todos todos (default 1,000), toggle all,
;; editing one, clear completed, and the JavaScript heap; medians and ranges,
;; every run kept in the example's out/runtime.edn.
(require '[babashka.fs :as fs]
         '[babashka.process :as process]
         '[cheshire.core :as json]
         '[clojure.edn :as edn]
         '[edamame.core :as edamame]
         '[clojure.string :as str])

(def repo (fs/parent (fs/parent (fs/absolutize *file*))))
(def research (fs/path repo "research"))
(def shadow-version "3.5.3")
(def esbuild-version "0.25.10")
;; 1.64.1 drives Chromium revision 1248; setup installs the package only, so
;; the browser must already be in Playwright's cache (npx playwright install).
(def playwright-version "1.64.1")
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
       (str "esbuild@" esbuild-version) (str "playwright-core@" playwright-version))
  (println "esbuild" esbuild-version "and playwright-core" playwright-version "in"
           (str (fs/path research "npm-tools"))))

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
  EXTRA adds esbuild flags (a --target, a --format other than esm)."
  [app entry out & extra]
  (apply time-l (example app) esbuild (str entry) "--bundle" "--minify"
         "--define:process.env.NODE_ENV=\"production\"" (str "--outfile=" out) "--log-level=warning"
         (concat (when-not (some #(str/starts-with? % "--format") extra) ["--format=esm"]) extra)))

(defn package-imports
  "The packages light-cljs's output imports: bare specifiers after `from` or
  `import`, so a string or keyword that happens to follow those words (flow's
  output has the keyword :import) is not taken for one."
  [file]
  (->> (re-seq #"\b(?:from|import)\s*\"(@?[a-z0-9][\w.-]*(?:/[\w.-]+)*)\"" (slurp (fs/file file)))
       (map second) distinct sort vec))

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
  "Add to ACC, with (ADD acc source line from to), each span of LINES (a
  section of the output) attributed by the source map M: a segment owns the
  text up to the next one; text before a line's first segment is :unmapped."
  [acc m lines add]
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
                            (add acc (if s (nth sources s) :unmapped) line
                                 (min col (count line)) (min (max col end) (count line))))
                          (add acc :unmapped line 0 (min (if (seq cols) (ffirst cols) (count line)) (count line)))
                          (map vector cols ends))]
          (recur (rest groups) (rest lines) src acc))
        acc))))

(defn- attribute
  "JS-FILE's text attributed by its source map, sectioned or not, folded into
  INIT with ADD (see attribute-section)."
  [js-file map-file init add]
  (let [m (json/parse-string (slurp (fs/file map-file)))
        lines (vec (str/split (slurp (fs/file js-file)) #"\n" -1))]
    (if-let [sections (get m "sections")]
      (let [starts (map #(get-in % ["offset" "line"]) sections)
            ends (concat (rest starts) [(count lines)])]
        (reduce (fn [acc [sec a b]] (attribute-section acc (get sec "map") (subvec lines a b) add))
                (reduce (fn [acc line] (add acc :unmapped line 0 (count line))) init (subvec lines 0 (first starts)))
                (map vector sections starts ends)))
      (attribute-section init m lines add))))

(defn bytes-by-source
  "Characters of JS-FILE attributed to each source of its map; text no
  segment names is :unmapped."
  [js-file map-file]
  (attribute js-file map-file {} (fn [acc k _ from to] (update acc k (fnil + 0) (- to from)))))

(defn text-by-source
  "The text of JS-FILE attributed to each source of its map, in output order."
  [js-file map-file]
  (update-vals (attribute js-file map-file {}
                          (fn [acc k ^String line from to]
                            (let [^StringBuilder sb (or (acc k) (StringBuilder.))]
                              (.append sb line (int from) (int to))
                              (assoc acc k sb))))
               str))

(defn shadow-mapped
  "shadow-cljs's clean release build of APP with a source map: out/shadow-mapped/client.js."
  [app]
  (let [dir (example app)]
    (fs/delete-tree (fs/path dir ".shadow-cljs/builds"))
    (sh! {:dir (str dir)} "npx" "shadow-cljs" "release" "client" "--config-merge"
         (pr-str {:output-dir "out/shadow-mapped" :compiler-options {:source-map true}}))
    (fs/path dir "out/shadow-mapped/client.js")))

(defn light-mapped
  "light-cljs's advanced build of the port in PORT with a source map, as one
  ES module LIGHT-JS (map next to it); FLAGS are added to the compile."
  [app binary port light-js & flags]
  (fs/create-dirs (fs/parent light-js))
  (let [entry (-> (edn/read-string (slurp (fs/file (port-dir app) "port-report.edn"))) :builds :client first)
        r (apply process/sh {:dir (str port) :err :string :out :string}
                 binary "--compile" "--optimize" "advanced" "--entry" (str entry)
                 "--source-root" "src" "--source-root" "vendor" "--source-root" "compat" "--esm" "--no-cache"
                 "--source-map" (str light-js ".map")
                 (concat flags macro-limits))]
    (when-not (zero? (:exit r)) (throw (ex-info (:err r) {})))
    (spit (fs/file light-js) (:out r))
    light-js))

(defn origins
  "Release builds with source maps, attributed by source: shadow-cljs's output
  of the example and light-cljs's output of the port."
  [app binary]
  (let [shadow-js (shadow-mapped app)
        light-js (light-mapped app binary (port-dir app) (fs/path (example app) "out/light-mapped/app.js"))]
    {:shadow (bytes-by-source shadow-js (str shadow-js ".map"))
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
;; breakdown: what the compressed difference consists of (light-cljs-979)
;;
;; Both files as a user ships them: shadow-cljs's release file, and
;; light-cljs's output bundled with React by esbuild --minify. Each has a
;; source map (esbuild composes light-cljs's), so each origin's text can be
;; compressed alone. An origin's share of the whole file's gzip size is its
;; size alone scaled so that the shares add up to the whole: compressed
;; together, text shares a dictionary, so the sizes alone add up to more.
;;
;; Steps, each adding to the one before, rebuild light-cljs's side with what
;; shadow-cljs's release build has and light-cljs's lacks, so the gap that
;; is left is the compilers'. The rewrites are measurements on copies under
;; out/breakdown; the port is unchanged.

(def define-rewrites
  "re-frame reads its two flags through a function, (trace/is-trace-enabled?),
  and through another def, interop/debug-enabled? (goog.DEBUG). Closure inlines
  both and drops the code behind them. light-cljs folded only a flag read
  directly until light-cljs-984, which folds both; since then these rewrites,
  which read them directly, should change nothing. The porter turns
  re-frame's goog-defines into ^:const defs (shadow-port-tool-984), and the
  rewritten debug-enabled? is one too."
  [["src/re_frame" #"\(trace/is-trace-enabled\?\)" "trace/trace-enabled?"]
   ["src/re_frame" #"\(is-trace-enabled\?\)" "trace-enabled?"]
   ["src/re_frame/interop.cljs" #"\(def \^boolean debug-enabled\? \"@define \{boolean\}\" \^boolean goog/DEBUG\)"
    "(def ^:const debug-enabled? false)"]])

(def reader-rewrites
  "TodoMVC without cljs.reader, for both compilers: its one read-string (of
  local storage) becomes JSON.parse, and the require goes."
  [["src/todomvc/db.cljs" #"\(cljs\.reader/read-string\)" "(js/JSON.parse)"]
   ["src/todomvc/db.cljs" #"\(:require \[cljs\.reader\]\s*" "(:require "]])

(def breakdown-steps
  [{:step "as built"}
   {:step "+ asserts elided, as shadow-cljs's release mode does" :flags ["--elide-asserts"]}
   {:step "+ re-frame's defines read directly" :rewrites define-rewrites}
   {:step "+ no cljs.reader, both compilers" :rewrites reader-rewrites :shadow true :apps #{"todomvc"}}])

(defn- rewrite!
  "Apply REWRITES ([path regex replacement], path relative to DIR, a file or
  a directory of sources) in place; each must match at least once."
  [dir rewrites]
  (doseq [[path re replacement] rewrites
          :let [root (fs/path dir path)
                files (if (fs/directory? root) (fs/glob root "**.{cljs,cljc}") [root])
                n (reduce + (for [f files
                                  :let [text (slurp (fs/file f))
                                        n (count (re-seq re text))]]
                              (do (when (pos? n) (spit (fs/file f) (str/replace text re replacement)))
                                  n)))]]
    (when (zero? n) (throw (ex-info (str "rewrite matched nothing: " path " " re) {})))))

(defn- without-map-comment
  "FILE without its sourceMappingURL comment, written next to it as -plain.js."
  [file]
  (let [out (fs/path (fs/parent file) (str/replace (str (fs/file-name file)) #"\.js$" "-plain.js"))]
    (spit (fs/file out) (str/replace (slurp (fs/file file)) #"\n?//# sourceMappingURL=\S+\s*$" "\n"))
    out))

(defn gzip-size [^String s] (count (:out (process/sh {:in s :out :bytes} "gzip" "-9" "-c"))))

(defn cause-group
  "The breakdown's row for an origin group (origin-group)."
  [group]
  (cond (= group "npm (React)") "React"
        (#{"Closure Library (goog)" "compat: goog shims"} group) "Closure Library / the port's goog shims"
        (or (#{"cljs.core" "light-cljs/js_runtime.js" "compat: other shims"} group) (str/starts-with? group "shadow/"))
        "runtime: cljs.core and shadow's own / light-cljs's runtime"
        (#{"cljs/tools" "cljs/reader.cljs" "%3Cfirst-party%3E/cljs"} group) "cljs.reader (and tools.reader)"
        (or (= group "%3Cfirst-party%3E/clojure") (re-find #"^clojure/" group)) "clojure.string, set, walk and data"
        (= group "cljs/spec") "cljs.spec"
        (str/starts-with? group "reagent/") "reagent"
        (str/starts-with? group "re_frame/") "re-frame"
        (#{"todomvc" "simple" "flow"} (first (str/split group #"/"))) "the app's own namespaces"
        (= group "(no source)") "(no source)"
        :else "other libraries (secretary, macrovich)"))

(defn gzip-shares
  "The text of each cause group of JS-FILE (a source map next to it): raw
  characters and its share of the file's gzip size."
  [js-file]
  (let [cause (memoize (fn [source] (cause-group (origin-group source))))
        ;; Collected span by span, so each cause's text keeps output order.
        texts (update-vals (attribute js-file (str js-file ".map") {}
                                      (fn [acc k ^String line from to]
                                        (let [c (cause k)
                                              ^StringBuilder sb (or (acc c) (StringBuilder.))]
                                          (.append sb line (int from) (int to))
                                          (assoc acc c sb))))
                           str)
        whole (:gzip (compressed (without-map-comment js-file)))
        alone (update-vals texts gzip-size)
        scale (/ whole (double (reduce + (vals alone))))]
    {:whole whole
     :rows (into {} (for [[k text] texts] [k {:raw (count text) :gzip (Math/round (* scale (alone k)))}]))}))

(defn breakdown-light
  "light-cljs's side of a breakdown step: the port copied and rewritten when
  REWRITES, built with FLAGS and bundled with React by esbuild with a source map."
  [app binary dir rewrites flags]
  (let [port (if (seq rewrites)
               (let [p (fs/path dir "port")]
                 (fs/delete-tree p)
                 (fs/copy-tree (port-dir app) p)
                 (rewrite! p rewrites)
                 p)
               (port-dir app))
        js (apply light-mapped app binary port (fs/path dir "app.js") flags)
        out (fs/path dir "bundle.js")]
    (bundle app js out "--sourcemap")
    {:app-js js :bundle out}))

(defn breakdown-shadow
  "shadow-cljs's release of APP with the example's sources rewritten in place
  for the build and restored after it."
  [app rewrites out-dir]
  (let [dir (example app)
        originals (into {} (for [[path] rewrites :let [f (fs/file dir path)]] [f (slurp f)]))]
    (try
      (rewrite! dir rewrites)
      (fs/delete-tree (fs/path dir ".shadow-cljs/builds"))
      (sh! {:dir (str dir)} "npx" "shadow-cljs" "release" "client" "--config-merge"
           (pr-str {:output-dir (str (fs/relativize dir out-dir)) :compiler-options {:source-map true}}))
      (fs/path out-dir "client.js")
      (finally (doseq [[f text] originals] (spit f text))))))

(defn what-ifs
  "gzip of BUNDLE with one construct removed or shortened: an upper bound on
  what dropping or renaming it saves. The text is no longer a program."
  [bundle]
  (let [s (slurp (fs/file bundle))
        whole (gzip-size s)
        registry #"[\w$]+\(\"[^\"]+\",\{(?:aliases|bindings):.*?\]\}(?:,!0)?\)[,;]?"
        ;; cljs$... ABI names, and protocol method slots of any namespace
        ;; (re_frame$router$IEventQueue$_add_event$arity$2).
        abi-re #"cljs\$[A-Za-z0-9_$]*|[A-Za-z_][A-Za-z0-9_]*(?:\$[A-Za-z0-9_]+)+\$arity\$[A-Za-z0-9_]+"
        abi (->> (re-seq abi-re s) frequencies (sort-by (comp - val)) (map key))
        short-abi (zipmap abi (map #(str "Q" (Integer/toString % 36)) (range)))
        declarations (first (sort-by (comp - count) (re-seq #"(?:[\w$]+,){50,}[\w$]+;" s)))]
    [["namespace registry calls removed" (count (apply str (re-seq registry s)))
      (- (gzip-size (str/replace s registry "")) whole)]
     ["compiler ABI and protocol method property names shortened to 2-3 characters"
      (- (count s) (count (str/replace s abi-re #(short-abi %))))
      (- (gzip-size (str/replace s abi-re #(short-abi %))) whole)]
     ["the up-front declaration list of every global removed" (count (or declarations ""))
      (- (gzip-size (if declarations (str/replace-first s declarations "") s)) whole)]]))

(defn minify-effects
  "light-cljs's output alone (packages external) through esbuild with each
  --minify option: raw and gzip."
  [app app-js]
  (let [dir (fs/path (example app) "out/breakdown/minify")
        in (without-map-comment app-js)]
    (fs/create-dirs dir)
    (into [["as written" (compressed in)]]
          (for [opt [nil "--minify-whitespace" "--minify-syntax" "--minify-identifiers" "--minify"]
                :let [out (fs/path dir (str (or opt "none") ".js"))]]
            (do (apply sh! {} esbuild (str in) "--bundle" "--format=esm" "--external:react"
                       "--external:react-dom/client" (str "--outfile=" out) "--log-level=error"
                       (remove nil? [opt]))
                [(str "esbuild " (or opt "--bundle only")) (compressed out)])))))

(defn breakdown [app binary]
  (let [base (fs/path (example app) "out/breakdown")
        shadow-js (shadow-mapped app)
        steps (->> breakdown-steps
                   (filter #(or (nil? (:apps %)) ((:apps %) app)))
                   (reductions (fn [acc s] (-> (merge acc (select-keys s [:step :shadow]))
                                               (update :flags into (:flags s))
                                               (update :rewrites into (:rewrites s))))
                               {:flags [] :rewrites []})
                   rest)
        results (vec (map-indexed
                      (fn [i {:keys [flags rewrites shadow] :as s}]
                        (let [dir (fs/path base (str "step-" i))
                              light (breakdown-light app binary dir rewrites flags)
                              shadow-file (if shadow
                                            (breakdown-shadow app (filter #(fs/exists? (fs/path (example app) (first %))) rewrites)
                                                              (fs/path dir "shadow"))
                                            shadow-js)]
                          (assoc s :light light :shadow-js shadow-file
                                 :light-size (compressed (without-map-comment (:bundle light)))
                                 :shadow-size (compressed (without-map-comment shadow-file)))))
                      steps))]
    {:app app :steps results
     :shares (into {} (for [i [0 2 3] :when (< i (count results))]
                        [i {:shadow (gzip-shares (get-in results [i :shadow-js]))
                            :light (gzip-shares (get-in results [i :light :bundle]))}]))
     :what-ifs (what-ifs (without-map-comment (get-in results [2 :light :bundle])))
     :minify (minify-effects app (get-in results [0 :light :app-js]))}))

(defn print-breakdown [{:keys [app steps shares what-ifs minify]}]
  (println (str "\n### " app ": whole file by step (bytes)\n"))
  (println "| Step | shadow-cljs gzip | light-cljs gzip | difference | shadow-cljs brotli | light-cljs brotli | difference |")
  (println "|---|---:|---:|---:|---:|---:|---:|")
  (doseq [{:keys [step shadow-size light-size]} steps]
    (println (format "| %s | %,d | %,d | %+,d | %,d | %,d | %+,d |" step (:gzip shadow-size) (:gzip light-size)
                     (- (:gzip light-size) (:gzip shadow-size)) (:brotli shadow-size) (:brotli light-size)
                     (- (:brotli light-size) (:brotli shadow-size)))))
  (doseq [[i {shadow-shares :shadow shares :light}] (sort shares)
          :let [s (:rows shadow-shares) l (:rows shares)]]
    (println (str "\n### " app ": gzip share by origin, " (:step (steps i)) "\n"))
    (println "| Origin | shadow-cljs raw | light-cljs raw | shadow-cljs gzip | light-cljs gzip | difference |")
    (println "|---|---:|---:|---:|---:|---:|")
    (doseq [k (sort-by #(- (Math/abs (long (- (get-in l [% :gzip] 0) (get-in s [% :gzip] 0)))))
                       (distinct (concat (keys s) (keys l))))]
      (println (format "| %s | %,d | %,d | %,d | %,d | %+,d |" k (get-in s [k :raw] 0) (get-in l [k :raw] 0)
                       (get-in s [k :gzip] 0) (get-in l [k :gzip] 0)
                       (- (get-in l [k :gzip] 0) (get-in s [k :gzip] 0)))))
    (println (format "| whole file | | | %,d | %,d | %+,d |" (:whole shadow-shares) (:whole shares)
                     (- (:whole shares) (:whole shadow-shares)))))
  (println (str "\n### " app ": constructs in light-cljs's bundle, " (:step (steps 2)) "\n"))
  (println "| Construct | Raw characters | gzip change without it |")
  (println "|---|---:|---:|")
  (doseq [[label raw gzip] what-ifs]
    (println (format "| %s | %,d | %+,d |" label raw gzip)))
  (println (str "\n### " app ": light-cljs's output (as built) through esbuild, packages external\n"))
  (println "| Output | Raw bytes | gzip -9 | brotli -q 11 |")
  (println "|---|---:|---:|---:|")
  (doseq [[label {:keys [raw gzip brotli]}] minify]
    (println (format "| %s | %,d | %,d | %,d |" label raw gzip brotli))))

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

(def compat-causes
  "The cause of each compat shim, by path prefix under compat/ (first match)."
  [["light/compat" "compiler gap: cljs.core names light-cljs lacks (catalogue :core-gaps)"]
   ["goog" "Closure Library use (non-goal)"]
   ["shadow/" "shadow-cljs build feature"]
   ["cljs/spec/" "JVM-only macro: spec stub, specs check nothing"]
   ["clojure/spec/" "JVM-only macro: spec stub, specs check nothing"]
   ["expound/" "JVM-only macro: stub of a spec library"]
   ["cljs/test" "JVM-only macro: cljs.test stub"]
   ["clojure/test/check" "JVM-only macro: test.check stub"]
   ["clojure/math" "other: a library light-cljs does not ship, written over js/Math"]
   ["" "other: development library replaced by a stub"]])

(defn compat-cause [rel] (some (fn [[prefix cause]] (when (str/starts-with? rel prefix) cause)) compat-causes))

(defn inventory
  "Every file of the port's compat/ with its cause and the characters it puts
  in a source-mapped light-cljs release (0: not reached), then every rewrite
  the port report records: macro classes, configuration dispositions, spliced
  expansions and missing goog names."
  [app binary]
  (let [dir (port-dir app)
        report (edn/read-string (slurp (fs/file dir "port-report.edn")))
        emitted (:light (origins app binary))
        emitted-of (fn [rel] (reduce + 0 (for [[k v] emitted
                                               :when (and (string? k) (str/ends-with? k (str "compat/" rel)))]
                                           v)))]
    (println (str "\n### " app ": every compat/ file\n"))
    (println "| File | Cause | Source bytes | light-cljs output characters |")
    (println "|---|---|---:|---:|")
    (doseq [f (sort (map str (fs/glob (fs/path dir "compat") "**.{cljs,cljc,js}")))
            :let [rel (str (fs/relativize (fs/path dir "compat") f))]]
      (println (format "| %s | %s | %,d | %,d |" rel (compat-cause rel) (fs/size f) (emitted-of rel))))
    (println (str "\n### " app ": every macro the port classified\n"))
    (println "| Macro | Class | Call sites | Reason |")
    (println "|---|---|---:|---|")
    (doseq [[m {:keys [class sites reasons]}] (:macros report)]
      (println (format "| %s | %s | %d | %s |" m (name class) sites (str/join "; " reasons))))
    (println (str "\n### " app ": every configuration key\n"))
    (doseq [{:keys [key disposition why]} (:config report)]
      (println (format "- %s %s: %s" key (name disposition) why)))
    (println (str "\nReport: " (pr-str (select-keys (:report report)
                                                    [:files :spliced :patches :missing-goog :unprintable
                                                     :unsafe :splice-mismatch :edits-dropped]))))))

;; ---------------------------------------------------------------------------
;; bundle and browser: each compiler's release build served with the
;; example's own resources/public page and checked in headless Chromium

(def compilers [:shadow :light])

(defn browser-dir
  "APP's resources/public with js/client.js replaced by COMPILER's bundle."
  [app compiler]
  (fs/path (example app) "out" (str "browser-" (name compiler))))

(defn init-fn
  "The port's :init-fn (shadow's module entry function), checked to be the
  entry namespace's final form: light-cljs's startup chunk exports that form's
  value, so the bootstrap can call it as shadow calls :init-fn."
  [app]
  (let [dir (port-dir app)
        report (edn/read-string (slurp (fs/file dir "port-report.edn")))
        f (:value (first (filter #(= [:builds :client :modules :client :init-fn] (:key %)) (:config report))))
        file (some #(let [p (fs/path dir "src" (str (str/replace (munge (namespace f)) "." "/") %))]
                      (when (fs/exists? p) p))
                   [".cljs" ".cljc"])
        last-form (last (edamame/parse-string-all (slurp (fs/file file))
                                                       {:all true :read-cond :allow :features #{:cljs}
                                                        :auto-resolve name}))]
    (when-not (and (seq? last-form) (= 'defn (first last-form)) (= (name f) (name (second last-form))))
      (throw (ex-info (str app ": " f " is not the final form of " file "; the bootstrap cannot reach it") {})))
    f))

(defn bundle-browser
  "COMPILER's clean release build of APP, ready to serve from browser-dir: the
  example's resources/public with js/client.js replaced. shadow-cljs's output
  is used as written. light-cljs's ES module goes through esbuild with its npm
  packages into one IIFE script, entered by a two-line module that calls the
  default export, which is the :init-fn (see init-fn). Returns the build's
  timings and the served file."
  [app compiler binary]
  (let [dir (browser-dir app compiler)
        client (fs/path dir "js/client.js")]
    (fs/delete-tree dir)
    (fs/copy-tree (fs/path (example app) "resources/public") dir)
    (fs/delete-tree (fs/path dir "js"))
    (fs/create-dirs (fs/parent client))
    (case compiler
      :shadow (let [t (shadow-release app)]
                (fs/copy (:file t) client)
                (assoc t :served client))
      :light (let [t (light-release app binary)
                   f (init-fn app)
                   entry (fs/path (example app) "out/browser-entry.mjs")]
               (spit (fs/file entry)
                     (str "import init from " (pr-str (str "./light-release/" (fs/file-name (:file t)))) ";\n"
                          "if (typeof init !== \"function\") throw new Error(" (pr-str (str f " is not the startup chunk's default export")) ");\n"
                          "init();\n"))
               (assoc t :served client :esbuild (bundle app entry client "--format=iife"))))))

(defn browser-checks
  "Run benchmarks/re-frame-browser.mjs's checks for APP RUNS times against
  COMPILER's served build."
  [app compiler runs]
  (let [r (process/sh {:err :string :out :string}
                      "node" (str (fs/path repo "benchmarks/re-frame-browser.mjs"))
                      "--app" app "--dir" (str (browser-dir app compiler))
                      "--npm-tools" (str (fs/path research "npm-tools")) "--runs" (str runs))]
    (when-not (zero? (:exit r)) (throw (ex-info (str "browser checks failed to run\n" (:err r)) {})))
    (json/parse-string (:out r) true)))

(defn print-browser [app results]
  (let [runs (count (:runs (val (first results))))
        rows (distinct (for [[_ r] results run (:runs r) c (:checks run)] (:name c)))
        passes (fn [c row] (count (for [run (:runs (results c)) ch (:checks run)
                                        :when (and (= row (:name ch)) (:ok ch))] ch)))
        clean (fn [c] (count (filter (comp empty? :errors) (:runs (results c)))))
        snaps (fn [c] (map :snapshots (:runs (results c))))
        compared? (and (= 2 (count results)) (seq (mapcat identity (snaps :shadow))))
        same (fn [c] (count (filter #(= % (first (snaps :shadow))) (snaps c))))
        cell (fn [n] (if (= n runs) (str "pass " n "/" runs) (str "**fail** " n "/" runs)))]
    (println (str "\n### " app ", " runs " runs each\n"))
    (println (str "| Check | " (str/join " | " (map {:shadow "shadow-cljs" :light "light-cljs"} (keys results))) " |"))
    (println (str "|---|" (str/join (repeat (count results) "---|"))))
    (doseq [row rows]
      (println (str "| " row " | " (str/join " | " (map #(cell (passes % row)) (keys results))) " |")))
    (println (str "| no console errors or uncaught exceptions | " (str/join " | " (map #(cell (clean %)) (keys results))) " |"))
    (when compared?
      (println (str "| page text after each step equals shadow-cljs's first run | "
                    (str/join " | " (map #(cell (same %)) (keys results))) " |")))
    (doseq [[c r] results
            msg (distinct (concat (for [run (:runs r) ch (:checks run) :when (not (:ok ch))]
                                    (str (:name ch) ": " (:error ch)))
                                  (mapcat :errors (:runs r))))]
      (println (str "\n- " (name c) ": " msg)))
    (when compared?
      (doseq [c (keys results)
              [[step a] [_ b]] (map vector (first (snaps :shadow)) (first (snaps c)))
              :when (not= a b)]
        (println (str "\n- " (name c) " differs after " step ":\n```\n" b "\n```\nshadow-cljs:\n```\n" a "\n```"))))))

(defn browser
  "Bundle APP with each of COMPILERS (unless --no-build) and run the browser
  checks RUNS times against each, printing a pass/fail table."
  [app binary args]
  (let [cs (if-let [c (option args "--compiler" nil)] [(keyword c)] compilers)
        runs (parse-long (option args "--runs" "3"))]
    (when-not (some #{"--no-build"} args)
      (doseq [c cs] (bundle-browser app c binary)))
    (print-browser app (into (array-map) (for [c cs] [c (browser-checks app c runs)])))))

;; ---------------------------------------------------------------------------
;; runtime: how fast each compiler's TodoMVC build runs in headless Chromium

(def runtime-steps
  [[:load "page load to interactive"] [:add "add N todos"] [:toggleAll "toggle all"]
   [:edit "edit one in a list of N"] [:clearCompleted "clear completed"]])

(defn parse-time-l
  "{:wall :user :sys :peak-bytes} from the stderr of /usr/bin/time -l."
  [err]
  (let [[_ wall user sys] (re-find #"([\d.]+) real\s+([\d.]+) user\s+([\d.]+) sys" err)
        [_ peak] (re-find #"(\d+)\s+maximum resident set size" err)]
    {:wall (parse-double wall) :user (parse-double user) :sys (parse-double sys) :peak-bytes (parse-long peak)}))

(defn runtime-run
  "One benchmarks/re-frame-runtime.mjs run against COMPILER's served build,
  under /usr/bin/time -l: its JSON, with :process the whole run's wall time,
  the user and system CPU of Node and the browser processes it reaped, and the
  largest single process's peak memory."
  [app compiler todos]
  (let [r (process/sh {:err :string :out :string}
                      "/usr/bin/time" "-l" "node" (str (fs/path repo "benchmarks/re-frame-runtime.mjs"))
                      "--dir" (str (browser-dir app compiler))
                      "--npm-tools" (str (fs/path research "npm-tools")) "--todos" (str todos))]
    (when-not (zero? (:exit r)) (throw (ex-info (str "runtime run failed\n" (:err r)) {})))
    (assoc (json/parse-string (:out r) true) :process (parse-time-l (:err r)))))

(defn print-runtime [app todos runs results]
  (let [label #(str/replace % "N" (format "%,d" todos))
        cell (fn [c f] (summary (results c) f))
        ratio (fn [f] (format "%.2f" (/ (double (median (map f (results :light))))
                                        (median (map f (results :shadow))))))
        step-ms (fn [step k] #(get-in % [:steps step k]))
        mb (fn [k] #(/ (k %) 1e6))]
    (println (str "\n### " app " in headless Chromium, " runs " runs each\n"))
    (println "| Step | shadow-cljs ms | light-cljs ms | light/shadow | shadow-cljs script ms | light-cljs script ms |")
    (println "|---|---:|---:|---:|---:|---:|")
    (doseq [[step text] runtime-steps]
      (println (format "| %s | %s | %s | %s | %s | %s |" (label text)
                       (cell :shadow (step-ms step :ms)) (cell :light (step-ms step :ms)) (ratio (step-ms step :ms))
                       (cell :shadow (step-ms step :scriptMs)) (cell :light (step-ms step :scriptMs)))))
    (doseq [[k text] [[:heapAfterLoad "JavaScript heap after load, MB"]
                      [:heapAfterAdd (label "JavaScript heap with N todos, MB")]]]
      (println (format "| %s | %s | %s | %s | | |" text (cell :shadow (mb k)) (cell :light (mb k)) (ratio (mb k)))))
    (doseq [[k text scale] [[:wall "whole run (fresh browser, load, every step, close), wall s" 1]
                            [:user "whole run, user CPU s (Node and the browser processes)" 1]
                            [:sys "whole run, system CPU s" 1]
                            [:peak-bytes "whole run, peak memory of the largest process, MB" 1e6]]
            :let [f #(/ (get-in % [:process k]) scale)]]
      (println (format "| %s | %s | %s | %s | | |" text (cell :shadow f) (cell :light f) (ratio f))))
    (println (format "| 1-minute load average before the run | %s | %s | | | |" (cell :shadow :load) (cell :light :load)))
    (doseq [[c rs] results msg (distinct (mapcat :errors rs))]
      (println (str "\n- " (name c) ": " msg)))))

(defn runtime
  "Bundle APP (todomvc) with both compilers (unless --no-build), then run
  re-frame-runtime.mjs RUNS times per compiler, alternated, the first one
  first on even runs, with the 1-minute load average before each run. Writes
  every run to the example's out/runtime.edn and prints medians and ranges."
  [app binary args]
  (when-not (= "todomvc" app) (throw (ex-info "runtime measures todomvc only" {:app app})))
  (let [runs (parse-long (option args "--runs" "10"))
        todos (parse-long (option args "--todos" "1000"))
        results (atom (array-map :shadow [] :light []))]
    (when-not (some #{"--no-build"} args)
      (doseq [c compilers] (bundle-browser app c binary)))
    (doseq [i (range runs) c (if (even? i) compilers (reverse compilers))]
      (let [load (load-average)
            r (assoc (runtime-run app c todos) :load load :run i)]
        (binding [*out* *err*]
          (println (format "run %d %-7s load %.2f %s" i (name c) load
                           (str/join " " (for [[step _] runtime-steps] (format "%s %.0f ms" (name step) (double (get-in r [:steps step :ms]))))))))
        (swap! results update c conj r)))
    (spit (fs/file (example app) "out/runtime.edn") (pr-str @results))
    (print-runtime app todos runs @results)))

;; ---------------------------------------------------------------------------

(let [[command & args] *command-line-args*
      apps (take-while #(not (str/starts-with? % "--")) args)]
  (case command
    "setup" (setup)
    "port" (doseq [app apps] (port app args))
    "size" (doseq [app apps] (print-size (size app (light-binary args))))
    "time" (doseq [app apps] (timing app (light-binary args) (parse-long (option args "--runs" "5"))))
    "inventory" (doseq [app apps] (inventory app (light-binary args)))
    "origins" (doseq [app apps] (print-origins app (origins app (light-binary args))))
    "breakdown" (doseq [app apps] (print-breakdown (breakdown app (light-binary args))))
    "bundle" (doseq [app apps
                     c (if-let [c (option args "--compiler" nil)] [(keyword c)] compilers)]
               (println (str (fs/relativize repo (:served (bundle-browser app c (light-binary args)))))))
    "browser" (doseq [app apps] (browser app (light-binary args) args))
    "runtime" (doseq [app apps] (runtime app (light-binary args) args))
    (do (println "usage: bb benchmarks/re-frame.bb setup | port APP... | size APP... | time APP... [--runs N] | inventory APP... | breakdown APP... | bundle APP... [--compiler C] | browser APP... [--compiler C] [--runs N] [--no-build] | runtime todomvc [--runs N] [--todos N] [--no-build]  [--light BIN] [--porter DIR]")
        (System/exit 1))))
