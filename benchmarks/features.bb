#!/usr/bin/env bb
;; Measures examples/features, the code-splitting example, and prints the
;; Markdown table recorded under "Goal" in docs/code-splitting.md and in
;; benchmarks/compiler.md. Each cell is raw / `gzip -9` bytes of the
;; single-file `--esm` output (gzip of the bytes alone, without a file name);
;; `advanced` is terse. The rows are
;;
;;   - the whole application, entry app.main;
;;   - the application with each feature removed, and with both removed: the
;;     last is the lower bound for the startup chunk;
;;   - each feature compiled as its own entry, whose value is its last form
;;     (`start`, the loadable target): the upper bound for its chunk.
;;
;; A variant without a feature is generated into a temporary directory from
;; the sources: the feature's file is left out, its libspec is dropped from
;; every `ns` form, and every list naming it (a `lazy/loadable` site, or a
;; direct call) becomes nil. The last column is the runtime slice of the
;; `advanced` output as --size-report counts it. A line after the table names
;; the rows whose `advanced` output is not fully terse: whose names carry the
;; `__light<n>_` hygiene prefix, kept only where raw `js*` text puts a hole
;; inside one of its strings or comments (src/terse.rs).
;;
;; A second table gives the chunks of the whole application's `--out-dir`
;; build at `advanced` (light-cljs-280), raw / `gzip -9` bytes of each file,
;; beside its bound: startup beside the lower bound and the Goal target (the
;; lower bound plus 200 bytes per `loadable` site), with what the Goal counts
;; of it: less the runtime units `--size-report` attributes to roots of other
;; chunks and less the chunk loader, which the Goal does not count (the
;; user's decision of 2026-10-20); and the first load of each feature (its
;; chunk and the chunks it imports other than startup) beside the feature as
;; its own entry.
;;
;;   bb benchmarks/features.bb
(require '[babashka.fs :as fs]
         '[babashka.process :as process]
         '[cheshire.core :as json]
         '[clojure.string :as str]
         '[rewrite-clj.node :as n]
         '[rewrite-clj.zip :as z])

(def root (-> *file* fs/absolutize fs/parent fs/parent))
(def sources (fs/path root "examples" "features"))
(def entry 'app.main)
(def features '[app.editor app.reports])
(def levels ["none" "prepare" "advanced"])

(defn- source-file [dir ns]
  (fs/path dir (str (str/replace (munge (str ns)) "." "/") ".cljs")))

(defn- light-cljs
  "Builds the compiler and returns the binary's path."
  []
  (process/shell {:dir (str root)} "cargo" "build" "--locked" "--quiet" "--bin" "light-cljs")
  (str (fs/path (or (System/getenv "CARGO_TARGET_DIR") (fs/path root "target"))
                "debug" "light-cljs")))

(defn- edit-all
  "Applies `edit` to the first location matching `pred` until none matches."
  [zloc pred edit]
  (if-let [loc (z/find zloc z/next pred)]
    (recur (z/of-node (z/root (edit loc))) pred edit)
    zloc))

(defn- ns-form? [loc]
  (and (= :list (z/tag loc)) (= 'ns (some-> loc z/down z/sexpr))))

(defn- aliases
  "Alias symbol -> namespace symbol from the `:require` clauses of `ns-form`."
  [ns-form]
  (into {} (for [clause ns-form
                 :when (and (seq? clause) (= :require (first clause)))
                 spec (rest clause)
                 :when (vector? spec)
                 :let [as (:as (apply hash-map (rest spec)))]
                 :when as]
             [as (first spec)])))

(defn- without
  "The source text with every use of the namespaces `removed` taken out."
  [text removed]
  (let [zloc (z/of-string text)
        alias->ns (some-> (z/find zloc z/right ns-form?) z/sexpr aliases)
        removed? (fn [sym]
                   (or (contains? removed sym)
                       (when-let [qualifier (some-> sym namespace symbol)]
                         (contains? removed (get alias->ns qualifier qualifier)))))
        libspec? (fn [loc]
                   (and (some-> loc z/up z/down z/sexpr (= :require))
                        (some-> loc z/up z/up ns-form?)
                        (let [spec (z/sexpr loc)]
                          (removed? (if (vector? spec) (first spec) spec)))))
        names-removed? (fn [loc]
                         (and (= :list (z/tag loc))
                              (not (ns-form? loc))
                              (some #(and (= :token (n/tag %))
                                          (symbol? (n/sexpr %))
                                          (removed? (n/sexpr %)))
                                    (n/children (z/node loc)))))]
    (-> zloc
        (edit-all libspec? z/remove)
        (edit-all names-removed? #(z/replace % (n/token-node nil)))
        z/root-string)))

(defn- variant
  "Copies the sources into `dir` without the namespaces `removed`."
  [dir removed]
  (fs/copy-tree sources dir)
  (doseq [ns removed] (fs/delete (source-file dir ns)))
  (doseq [file (fs/glob dir "**.{cljs,cljc}")
          :let [text (slurp (str file))
                edited (without text removed)]
          :when (not= text edited)]
    (spit (str file) edited))
  dir)

(defn- measure
  "Raw and gzip bytes at each level, and the advanced runtime slice's bytes."
  [binary out source-root ns]
  (let [sizes (for [level levels
                    :let [file (str (fs/path out (str ns "." level ".js")))
                          {:keys [err]} (process/shell
                                         {:out (fs/file file) :err :string :dir (str root)}
                                         binary "--compile" "--no-cache" "--optimize" level "--size-report"
                                         "--entry" (str ns) "--source-root" (str source-root)
                                         "--esm")
                          gzip (:out (process/shell {:out :bytes} "gzip" "-9" "-n" "-c" file))]]
                {:bytes (fs/size file)
                 :gzip (count gzip)
                 :runtime (-> (re-find #"([\d,]+) runtime bytes" err) second
                              (str/replace "," "") parse-long)
                 :prefixed (boolean (re-find #"__light\d+_" (slurp file)))})]
    {:levels (mapv #(select-keys % [:bytes :gzip]) sizes)
     :runtime (:runtime (last sizes))
     :prefixed (:prefixed (last sizes))}))

(defn- gzip-bytes [file]
  (count (:out (process/shell {:out :bytes} "gzip" "-9" "-n" "-c" (str file)))))

(def ^:private loader-bytes
  "Startup's chunk loading, which the Goal does not count: the `Loadable`
  type, `lazy$`, the sites' calls and import thunks and `light.lazy/Loadable`'s
  definition, as light-cljs-353 measured it (`FEATURES_STARTUP_LOADER` in
  tests/compiler/size_ratchet.rs)."
  612)

(defn- held-bytes
  "The bytes of the runtime units the `--size-report` text `report` lists for
  its first chunk, startup, under roots another chunk's code first demanded:
  the rows of its roots table whose demand names a chunk file."
  [report]
  (let [[_ startup] (str/split report #"(?m)^(?=\S+\.js: [\d,]+ bytes)" 3)]
    (->> (re-seq #"(?m)^\S+\s+\d+\s+([\d,]+)\s+\S+\.js, " startup)
         (map #(parse-long (str/replace (second %) "," "")))
         (reduce + 0))))

(defn- chunked
  "The chunks of the `--out-dir` build of the whole application at
  `advanced`, in chunks.json order, each with its raw and gzip bytes, and
  startup with `:held`, the bytes of its units only other chunks' roots
  need."
  [binary out]
  (let [dir (fs/create-dirs (fs/path out "chunks"))
        report (:err (process/shell {:dir (str root) :err :string}
                                    binary "--compile" "--optimize" "advanced" "--entry" (str entry)
                                    "--source-root" (str sources) "--out-dir" (str dir)
                                    "--size-report"))
        chunks (for [chunk (:chunks (json/parse-string (slurp (str (fs/path dir "chunks.json"))) true))
                     :let [file (fs/path dir (:file chunk))]]
                 (assoc chunk :size {:bytes (fs/size file) :gzip (gzip-bytes file)}))]
    (cons (assoc (first chunks) :held (held-bytes report)) (rest chunks))))

(defn- grouped [n] (String/format java.util.Locale/ROOT "%,d" (object-array [n])))

(defn- cell [{:keys [bytes gzip]}] (str (grouped bytes) " / " (grouped gzip)))

(defn- advanced [measured] (last (:levels measured)))

(defn- chunk-rows
  "The Markdown rows of the chunk table: each chunk's size beside its bound,
  then what loading each feature first costs, its chunk and the chunks it
  imports other than startup, beside the feature as its own entry."
  [chunks lower uppers]
  (let [by-file (into {} (map (juxt :file identity) chunks))
        startup (first chunks)
        loadables (count (mapcat :loadables chunks))
        prerequisites (fn prerequisites [file]
                        (let [imports (remove #{(:file startup)} (:imports (by-file file)))]
                          (distinct (concat (mapcat prerequisites imports) imports))))
        total (fn [files] (apply merge-with + (map (comp :size by-file) files)))]
    (concat
     [(str "| `" (:file startup) "`, startup | " (cell (:size startup))
           "; less " (grouped (:held startup)) " bytes of units for other chunks' roots and "
           (grouped loader-bytes) " of chunk loading, "
           (grouped (- (:bytes (:size startup)) (:held startup) loader-bytes)) " raw"
           " | lower bound, the application without its features: " (cell (advanced lower))
           "; target under " (grouped (+ (:bytes (advanced lower)) (* 200 loadables)))
           " raw, 200 bytes per `loadable` |")]
     (for [chunk (rest chunks)]
       (str "| `" (:file chunk) "`"
            (if (seq (:roots chunk))
              (str ", loaded by " (str/join " and " (map #(str "`" % "`") (:roots chunk))))
              ", shared")
            " | " (cell (:size chunk)) " | |"))
     (for [chunk (rest chunks)
           root (:roots chunk)
           :let [files (concat (prerequisites (:file chunk)) [(:file chunk)])]]
       (str "| first load of `" root "`: " (str/join " + " (map #(str "`" % "`") files))
            " | " (cell (total files))
            " | upper bound, `" root "` as its own entry: " (cell (advanced (uppers root)))
            " |")))))

(defn- row [label {:keys [levels runtime]}]
  (str "| " label " | "
       (str/join " | " (map (fn [{:keys [bytes gzip]}] (str (grouped bytes) " / " (grouped gzip)))
                            levels))
       " | " (grouped runtime) " |"))

(let [binary (light-cljs)
      tmp (fs/create-temp-dir {:prefix "light-cljs-features-"})]
  (try
    (let [out (fs/create-dirs (fs/path tmp "out"))
          removals (concat (map vector features) [features])
          variants (vec (for [removed removals
                              :let [dir (variant (fs/path tmp (str/join "+" removed)) (set removed))
                                    vout (fs/create-dirs (fs/path out (str/join "+" removed)))]]
                          [removed (measure binary vout dir entry)]))
          uppers (into {} (for [feature features]
                            [(str feature) (measure binary out sources feature)]))
          rows (vec (concat
                     [[(str "`" entry "`, the whole application")
                       (measure binary out sources entry)]]
                     (for [[removed measured] variants]
                       [(str "`" entry "` without "
                             (str/join " and " (map #(str "`" % "`") removed))
                             (when (= removed features) ": startup lower bound"))
                        measured])
                     (for [feature features]
                       [(str "`" feature "` as its own entry: chunk upper bound")
                        (uppers (str feature))])))]
      (println (str "| Program | " (str/join " | " (map #(str "`" % "`") levels))
                    " | `advanced` runtime slice |"))
      (println (str "| --- |" (str/join (repeat (inc (count levels)) " ---: |"))))
      (doseq [[label sizes] rows] (println (row label sizes)))
      (println)
      (println (str "`advanced` output with hygiene-prefixed names (`__light<n>_`): "
                    (or (some->> (seq (keep (fn [[label sizes]] (when (:prefixed sizes) label))
                                            rows))
                                 (str/join "; "))
                        "none")
                    "."))
      (println)
      (println "| Chunk, `--out-dir` at `advanced` | Bytes | Bound |")
      (println "| --- | ---: | --- |")
      (doseq [row (chunk-rows (chunked binary out) (second (last variants)) uppers)]
        (println row)))
    (finally (fs/delete-tree tmp))))
