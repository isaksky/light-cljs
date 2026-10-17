(require '[babashka.fs :as fs] '[babashka.process :as process]
         '[clojure.java.io :as io] '[clojure.string :as str])
(def out (fs/absolutize (fs/parent *file*)))
(def fixtures (fs/path out "fixtures"))
(binding [*command-line-args* [(str fixtures)]]
  (load-file "benchmarks/results/compiler-2026-10-16/penpot-investigation/generate.bb"))
(def cases (concat (for [n [1000 2000 4000 8000]] [(str "calls-" n) "probe.main" 1])
                   (for [n [25 50 100]] [(str "namespaces-" n) (str "probe.n" (dec n)) 7])
                   (for [n [0 50 100]] [(str "macros-" n) "probe.main" 1])
                   [["synthetic-4000" "demo.main" 4006]]))
(doseq [[version binary] [["baseline" "/private/tmp/light-cljs-investigation-20261016/baseline"]
                         ["fixed" (str (fs/absolutize "target/release/light-cljs"))]]
        [fixture entry expected] cases]
  (let [stem (str version "-" fixture)
        output (fs/file out (str stem ".mjs"))
        log (fs/file out (str stem ".time.txt"))
        command ["/usr/bin/time" "-l" binary "--compile" "--entry" entry
                 "--source-root" (str (fs/path fixtures fixture)) "--esm" "--optimize" "none"
                 "--macro-expansions" "100000000" "--macro-nodes" "4000000000"
                 "--macro-timeout-ms" "3600000" "--macro-memory-limit" "4000000000"
                 "--macro-poll-budget" "1000000000000"]
        result (with-open [stdout (io/output-stream output) stderr (io/output-stream log)]
                 (apply process/shell {:out stdout :err stderr :continue true} command))
        timing (slurp log)
        [_ real user system] (re-find #"([0-9.]+)\s+real\s+([0-9.]+)\s+user\s+([0-9.]+)\s+sys" timing)
        rss (second (re-find #"(\d+)\s+maximum resident set size" timing))
        record {:version version :fixture fixture :exit (:exit result)
                :wall-seconds (some-> real parse-double) :user-seconds (some-> user parse-double)
                :system-seconds (some-> system parse-double) :peak-rss-bytes (some-> rss parse-long)
                :output-bytes (fs/size output)}]
    (spit (fs/file out "probes.edn") (str (pr-str record) "\n") :append true)
    (prn record) (flush)
    (when-not (zero? (:exit result)) (throw (ex-info "Probe failed" record)))))
;; Validate outside the timed builds, using a separate Node heap for each module.
(doseq [version ["baseline" "fixed"] [fixture _ expected] cases]
  (let [path (str (fs/path out (str version "-" fixture ".mjs")))
        result (process/shell {:out :string} "node" "--input-type=module" "-e"
                              "const m = await import('file://' + process.argv[1]); console.log(m.default(7));" path)]
    (assert (= (str expected) (str/trim (:out result))) [version fixture (:out result)])))
(println "All 22 synthetic modules returned their expected results in Node.")
