#!/usr/bin/env bb
;; Measure advanced/release output sizes (raw bytes and gzip -9 bytes) of four
;; cljs.pprint programs built by light-cljs and shadow-cljs 3.5.3, and check
;; that both compilers' outputs print identically in Node.
;;
;; Usage:
;;   bb benchmarks/cljs-pprint/size.bb [--light BINARY] [--shadow-only | --light-only]
;;                                     [--size-report]
(require '[babashka.fs :as fs]
         '[babashka.process :as process]
         '[clojure.string :as str])

(def here (fs/parent (fs/absolutize *file*)))
(def repo (fs/parent (fs/parent here)))

(def programs
  [{:id "prn"            :ns "pprint-bench.prn"}
   {:id "pprint-only"    :ns "pprint-bench.pprint-only"}
   {:id "cl-format-only" :ns "pprint-bench.cl-format-only"}
   {:id "code-dispatch"  :ns "pprint-bench.code-dispatch"}])

(defn parse-args [args]
  (loop [[a & more] args opts {:mode :both :size-report false}]
    (cond
      (nil? a) opts
      (= a "--light") (recur (rest more) (assoc opts :light (str (fs/absolutize (first more)))))
      (= a "--shadow-only") (recur more (assoc opts :mode :shadow))
      (= a "--light-only") (recur more (assoc opts :mode :light))
      (= a "--size-report") (recur more (assoc opts :size-report true))
      :else (throw (ex-info (str "Unknown arg: " a) {})))))

(defn sh! [opts & cmd]
  (let [r (apply process/sh (merge {:dir (str here)} opts) (map str cmd))]
    (when-not (zero? (:exit r))
      (let [err (str (:err r) "\n" (:out r))]
        (throw (ex-info (str "Failed: " (str/join " " cmd) "\n"
                             (subs err (max 0 (- (count err) 3000)))) {}))))
    r))

(defn gzip-bytes [path]
  (let [r (process/sh {:out :bytes} "gzip" "-9" "-c" (str path))]
    (when-not (zero? (:exit r))
      (throw (ex-info (str "gzip failed for " path) {})))
    (alength ^bytes (:out r))))

(defn light-binary [opts]
  (let [bin (or (:light opts) (System/getenv "LIGHT_CLJS_BINARY")
                (str (fs/path repo "target/release/light-cljs")))]
    (when-not (fs/executable? bin)
      (throw (ex-info (str "No executable light-cljs binary at " bin) {})))
    bin))

(defn build-shadow! [{:keys [id]}]
  (let [out (fs/path here (str "out/shadow-" id ".js"))]
    (fs/delete-if-exists out)
    (sh! {} "clojure" "-M:shadow" "release" id)
    (let [raw (fs/size out)
          gz (gzip-bytes out)
          stdout (:out (sh! {} "node" (str out)))]
      {:id id :raw raw :gz gz :stdout stdout :path (str out)})))

(defn build-light! [bin {:keys [id ns]}]
  (let [out-dir (fs/path here (str "out/light-" id))]
    (fs/delete-tree out-dir)
    (sh! {} bin "release" id)
    (spit (str (fs/path out-dir "package.json")) "{\"type\":\"module\"}\n")
    (let [js-file (fs/path out-dir (str ns ".js"))
          raw (fs/size js-file)
          gz (gzip-bytes js-file)
          run-code (str "const { default: main } = await import(" (pr-str (str "file://" js-file)) "); main();")
          stdout (:out (sh! {} "node" "--input-type=module" "-e" run-code))]
      {:id id :raw raw :gz gz :stdout stdout :path (str js-file)})))

(defn fmt-int [n] (format "%,d" (long n)))
(defn fmt-delta [n] (format "%+,d" (long n)))

(defn print-table [label rows]
  (let [base (first rows)]
    (println (str "\n### " label))
    (println "| Program | Raw bytes | Delta vs prn | gzip -9 | Delta vs prn |")
    (println "|---|---:|---:|---:|---:|")
    (doseq [{:keys [id raw gz]} rows]
      (println (format "| `%s` | %s | %s | %s | %s |"
                       id
                       (fmt-int raw)
                       (if (= id "prn") "0" (fmt-delta (- raw (:raw base))))
                       (fmt-int gz)
                       (if (= id "prn") "0" (fmt-delta (- gz (:gz base)))))))))

(defn -main [& args]
  (let [opts (parse-args args)
        load-avg (str/trim (:out (sh! {} "uptime")))]
    (println "Load:" load-avg)
    (let [shadow-rows (when (#{:both :shadow} (:mode opts))
                        (mapv build-shadow! programs))
          light-rows (when (#{:both :light} (:mode opts))
                       (let [bin (light-binary opts)]
                         (mapv #(build-light! bin %) programs)))]
      (when (and shadow-rows light-rows)
        (doseq [[s l] (map vector shadow-rows light-rows)]
          (when-not (= (:stdout s) (:stdout l))
            (throw (ex-info (str "Output mismatch for " (:id s)
                                 "\nshadow:\n" (:stdout s)
                                 "\nlight:\n" (:stdout l)) {})))))
      (when shadow-rows (print-table "shadow-cljs 3.5.3" shadow-rows))
      (when light-rows (print-table "light-cljs" light-rows))
      (when (and (:size-report opts) (#{:both :light} (:mode opts)))
        (let [bin (light-binary opts)]
          (doseq [{:keys [id ns]} (rest programs)]
            (println (str "\n### light-cljs --size-report: " id))
            (let [r (sh! {} bin "--compile" "--optimize" "advanced" "--size-report"
                         "--entry" ns "--source-root" "src" "--no-cache")]
              (println (str/trim (:err r))))))))))

(apply -main *command-line-args*)
