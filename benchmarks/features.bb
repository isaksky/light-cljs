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
;;   bb benchmarks/features.bb
(require '[babashka.fs :as fs]
         '[babashka.process :as process]
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
                                         binary "--compile" "--optimize" level "--size-report"
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

(defn- grouped [n] (String/format java.util.Locale/ROOT "%,d" (object-array [n])))

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
          rows (vec (concat
                     [[(str "`" entry "`, the whole application")
                       (measure binary out sources entry)]]
                     (for [removed removals
                           :let [dir (variant (fs/path tmp (str/join "+" removed)) (set removed))
                                 label (str "`" entry "` without "
                                            (str/join " and " (map #(str "`" % "`") removed))
                                            (when (= removed features) ": startup lower bound"))
                                 vout (fs/create-dirs (fs/path out (str/join "+" removed)))]]
                       [label (measure binary vout dir entry)])
                     (for [feature features]
                       [(str "`" feature "` as its own entry: chunk upper bound")
                        (measure binary out sources feature)])))]
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
                    ".")))
    (finally (fs/delete-tree tmp))))
