#!/usr/bin/env bb
;; Whether builds of one light-cljs.edn evict each other's artifacts from the
;; shared compiler cache (docs/build-configuration.md, Phase 3, "Cache
;; retention, measured"). On a temporary copy of the Phase 3 test project,
;; tests/fixtures/build-configuration/multi/, with one cache directory, it runs
;;   watch app worker   until both builds report their startup build, then stops it
;;   release app
;;   release worker
;;   compile test
;;   watch app worker   again
;; and, as the single-build baseline, on a copy of examples/features
;;   watch features, release features, watch features
;; each with --stats. It prints per run and build the cache hits, misses and
;; bypasses and each miss with its reason, then the store's artifacts per
;; namespace. No source changes between the runs, so a miss in the last run
;; is an artifact the store no longer holds: Store::trim evicted it.
;;
;; Usage: bb benchmarks/build-configuration-retention.bb [FLAG ...]
;; Every FLAG is added to every run (for example --cache-retention 4).
;; LIGHT_CLJS_BINARY selects the binary (target/release/light-cljs by default).
;; The runs never see LIGHT_CLJS.
;; Exits 1 when the multi project's last run misses, or the baseline's does.
(require '[babashka.fs :as fs]
         '[babashka.process :as process]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(import '[java.nio ByteBuffer ByteOrder])

(def repo (fs/parent (fs/parent (fs/absolutize *file*))))
(def binary (or (System/getenv "LIGHT_CLJS_BINARY")
                (str (fs/path repo "target/release/light-cljs"))))
(def extra-flags (vec *command-line-args*))
;; Without LIGHT_CLJS, which would merge over the copied file and change
;; what is measured (its :cache-retention or :cache-root, say).
(def environment (dissoc (into {} (System/getenv)) "LIGHT_CLJS"))

(defn reports
  "The --stats reports in `lines`, in order: each its counters and its
  [cache-misses-and-bypasses] rows."
  [lines]
  (->> (reduce (fn [segments line]
                 (cond (str/starts-with? line "light-cljs-stats ") (conj segments [])
                       (seq segments) (conj (pop segments) (conj (peek segments) line))
                       :else segments))
               [] lines)
       (mapv (fn [body]
               (let [counters (into {} (for [line body
                                             :let [[_ k v] (re-matches #"(cache\.[a-z.-]+) (\d+)" line)]
                                             :when k]
                                         [k (parse-long v)]))
                     rows (->> body
                               (drop-while #(not= % "[cache-misses-and-bypasses]"))
                               rest
                               (take-while #(re-matches #"\S+ (miss|bypass) .*" %)))]
                 {:hits (counters "cache.hits" 0)
                  :misses (counters "cache.misses" 0)
                  :bypasses (counters "cache.bypasses" 0)
                  :published (counters "cache.published" 0)
                  :miss-rows (filterv #(re-matches #"\S+ miss .*" %) rows)})))))

(defn run-command
  "Run one command in `dir` and return its stderr lines. A watch runs until
  every build in `builds` has printed its built line."
  [dir watch? builds args]
  (let [command (vec (concat [binary] args ["--stats"] (when watch? ["--serve" "0"]) extra-flags))]
    (if watch?
      (let [proc (process/process command {:dir (str dir) :env environment :err :pipe :out :discard})
            lines (with-open [reader (io/reader (:err proc))]
                    (loop [lines [] waiting (set builds)]
                      (if (empty? waiting)
                        lines
                        (let [line (.readLine reader)]
                          (if (nil? line)
                            (throw (ex-info "The watch ended before every build was built"
                                            {:command command :stderr (str/join "\n" lines)}))
                            (recur (conj lines line)
                                   (if-let [[_ id] (re-find #"^\[:(\S+)\] built " line)]
                                     (disj waiting id)
                                     waiting)))))))]
        (process/destroy-tree proc)
        @proc
        lines)
      (let [{:keys [exit err]} (apply process/sh {:dir (str dir) :env environment} command)]
        (when-not (zero? exit)
          (throw (ex-info "A build failed" {:command command :stderr err})))
        (str/split-lines err)))))

(defn store-counts
  "The artifacts the store in `dir` keeps per namespace, from its index files:
  magic, version, the namespace's length and bytes, then the entry count."
  [dir]
  (into (sorted-map)
        (for [file (fs/list-dir (fs/path dir ".light-cljs/cache/v1/index"))
              :when (not (str/starts-with? (fs/file-name file) "."))
              :let [buffer (doto (ByteBuffer/wrap (fs/read-all-bytes file))
                             (.order ByteOrder/LITTLE_ENDIAN))
                    _ (.position buffer 12)
                    length (.getInt buffer)
                    name (byte-array length)
                    _ (.get buffer name)]]
          [(String. name "UTF-8") (.getInt buffer)])))

(defn measure
  "Copy `source` to a temporary directory and run `runs` there, one cache
  directory for all of them. Return the last run's misses."
  [title source runs]
  (let [dir (fs/create-temp-dir {:prefix "light-retention-"})
        project (fs/path dir "project")]
    (try
      (fs/copy-tree source project)
      (doseq [leftover ["out" ".light-cljs"]] (fs/delete-tree (fs/path project leftover)))
      (println (str "== " title (when (seq extra-flags) (str " with " (str/join " " extra-flags)))))
      (let [results
            (vec (for [[n {:keys [args builds]}] (map-indexed vector runs)
                       :let [watch? (= "watch" (first args))
                             found (reports (run-command project watch? builds args))]]
                   (do
                     (when (not= (count found) (count builds))
                       (throw (ex-info "Expected one --stats report per build"
                                       {:args args :reports (count found)})))
                     (doseq [[build {:keys [hits misses bypasses published miss-rows]}] (map vector builds found)]
                       (println (format "run %d  %-22s :%-7s hits %d  misses %d  bypasses %d  published %d"
                                        (inc n) (str/join " " args) build hits misses bypasses published))
                       (doseq [row miss-rows] (println "         " row)))
                     found)))
            last-misses (reduce + (map :misses (peek results)))]
        (println "artifacts per namespace in the store:")
        (doseq [[namespace entries] (store-counts project)]
          (println (format "  %-16s %d" namespace entries)))
        (println (if (zero? last-misses)
                   "last run: all hits or bypasses; no eviction"
                   (format "last run: %d misses with unchanged sources: builds evict each other" last-misses)))
        (println)
        last-misses)
      (finally (fs/delete-tree dir)))))

(let [multi (measure "tests/fixtures/build-configuration/multi"
                     (fs/path repo "tests/fixtures/build-configuration/multi")
                     [{:args ["watch" "app" "worker"] :builds ["app" "worker"]}
                      {:args ["release" "app"] :builds ["app"]}
                      {:args ["release" "worker"] :builds ["worker"]}
                      {:args ["compile" "test"] :builds ["test"]}
                      {:args ["watch" "app" "worker"] :builds ["app" "worker"]}])
      baseline (measure "examples/features (single-build baseline)"
                        (fs/path repo "examples/features")
                        [{:args ["watch" "features"] :builds ["features"]}
                         {:args ["release" "features"] :builds ["features"]}
                         {:args ["watch" "features"] :builds ["features"]}])]
  (System/exit (if (and (zero? multi) (zero? baseline)) 0 1)))
