#!/usr/bin/env bb
;; Sequential cold-process builds of the existing Penpot port on macOS.
;; Build the release binary and finish tests before running this script.
;; Usage: bb benchmarks/penpot.bb OUTPUT-DIR [render main-none main-advanced shadow-release shadow-compile]
;; LIGHT_CLJS_BINARY optionally selects an immutable candidate binary.
(require '[babashka.fs :as fs]
         '[babashka.process :as process]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def repo (fs/parent (fs/parent (fs/absolutize *file*))))
(def output (some-> (first *command-line-args*) fs/absolutize))
(when-not output (throw (ex-info "An output directory is required" {})))
(fs/create-dirs output)
(def port (fs/path repo "research/penpot-light"))
(def frontend (fs/path repo "research/penpot/frontend"))
(def binary (or (System/getenv "LIGHT_CLJS_BINARY")
                (str (fs/path repo "target/release/light-cljs"))))
(def common [binary "--compile" "--esm"
             "--macro-expansions" "100000000" "--macro-nodes" "4000000000"
             "--macro-timeout-ms" "3600000" "--macro-memory-limit" "4000000000"
             "--macro-poll-budget" "1000000000000"])
(def roots (mapcat #(vector "--source-root" (str (fs/path port %)))
                   ["frontend/src" "common/src" "vendor" "generated" "shims"]))
(def jobs {"render" ["app.render" "--optimize" "none"]
           "main-none" ["app.main" "--optimize" "none"]
           "main-advanced" ["app.main" "--optimize" "advanced" "--pseudo-names" "--elide-asserts"]
           "shadow-release" "release"
           "shadow-compile" "compile"})
(def selected (or (seq (rest *command-line-args*))
                  ["render" "main-none" "main-advanced" "shadow-release" "shadow-compile"]))

(doseq [job selected]
  (let [spec (or (jobs job) (throw (ex-info "Unknown benchmark" {:job job})))
        shadow? (string? spec)
        stdout (fs/file output (str job (if shadow? ".log" ".js")))
        stderr (fs/file output (str job ".time.txt"))
        command (if shadow?
                  ["clojure" "-M:dev:shadow-cljs" spec "main"]
                  (concat common roots ["--entry" (first spec)] (rest spec)))]
    (when shadow?
      ;; These are generated compiler caches, never port sources.
      (fs/delete-tree (fs/path frontend ".shadow-cljs"))
      (fs/delete-if-exists (fs/path frontend "target/index.js")))
    (println "Starting" job)
    (flush)
    (let [result (with-open [out (io/output-stream stdout) err (io/output-stream stderr)]
                   (apply process/shell
                          {:dir (str (if shadow? frontend repo)) :out out :err err :continue true
                           :extra-env {"SHADOW_SERVER_URL" "http://localhost:3448"}}
                          (concat ["/usr/bin/time" "-l"] command)))
          timing (slurp stderr)
          [_ real user system] (re-find #"([0-9.]+)\s+real\s+([0-9.]+)\s+user\s+([0-9.]+)\s+sys" timing)
          rss (second (re-find #"(\d+)\s+maximum resident set size" timing))
          record {:job job :exit (:exit result) :command (vec command)
                  :environment (into {} (for [name ["LIGHT_V8_MACROS" "LIGHT_V8_HEAP_BYTES" "LIGHT_V8_FLAGS" "LIGHT_V8_STATS" "LIGHT_V8_SNAPSHOT"]]
                                          [name (System/getenv name)]))
                  :wall-seconds (some-> real parse-double)
                  :user-seconds (some-> user parse-double)
                  :system-seconds (some-> system parse-double)
                  :peak-rss-bytes (some-> rss parse-long)
                  :output-bytes (when-not shadow? (fs/size stdout))
                  :shadow-summary (when shadow? (re-find #"[^\n]*Build completed[^\n]*" (slurp stdout)))}]
      (spit (fs/file output "results.edn") (str (pr-str record) "\n") :append true)
      (prn (dissoc record :command))
      (flush)
      (when-not (zero? (:exit result))
        (throw (ex-info "Benchmark failed; inspect its log" record))))))
