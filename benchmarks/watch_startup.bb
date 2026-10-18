#!/usr/bin/env bb
;; Watch startup with --stats: the startup build (V8), writing the dev build
;; and the session load (QuickJS), each run in a fresh process stopped once it
;; reports that it is watching.
;; Usage: bb benchmarks/watch_startup.bb [--cache] OUTPUT-DIR [RUNS] [NAMESPACES]
;; With --cache, the watch runs with --cache-dir OUTPUT-DIR/cache, emptied
;; first: the first run fills it and the others start warm.
;; With NAMESPACES, the sources are a generated program of that many
;; namespaces, a tenth of them macro namespaces, written under OUTPUT-DIR/src;
;; without it, they are examples/dev. LIGHT_CLJS_BINARY selects the binary
;; (default target/release/light-cljs). Prints one EDN map per run.
(require '[babashka.fs :as fs]
         '[babashka.process :as process]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def repo (fs/parent (fs/parent (fs/absolutize *file*))))
(def cache? (= "--cache" (first *command-line-args*)))
(def arguments (if cache? (rest *command-line-args*) *command-line-args*))
(def output (some-> (first arguments) fs/absolutize))
(when-not output (throw (ex-info "An output directory is required" {})))
;; The script deletes and rewrites OUTPUT-DIR/src and OUTPUT-DIR/dev, so it
;; writes only into a directory that is new, empty, or marked as its own.
(def marker (fs/path output ".watch-startup-output"))
(when (and (fs/exists? output) (not (fs/exists? marker)) (seq (fs/list-dir output)))
  (throw (ex-info (str output " is not empty and was not made by this script") {})))
(fs/create-dirs output)
(spit (str marker) "")
(def runs (parse-long (or (second arguments) "3")))
(def namespaces (some-> (nth arguments 2 nil) parse-long))
(def cache (str (fs/path output "cache")))
(fs/delete-tree cache)
(def binary (or (System/getenv "LIGHT_CLJS_BINARY")
                (str (fs/path repo "target/release/light-cljs"))))

(defn macro-namespace
  "Macro namespace `k`: macros whose expansions call a helper, destructure
  and build collections."
  [k]
  (str "(ns bench.macros" k ")\n\n"
       "(defn bracket [text] (str \"[\" text \"]\"))\n\n"
       "(defmacro labeled [label & body]\n"
       "  `(str ~label \": \" (bracket (str ~@body))))\n\n"
       "(defmacro with-fields [[a b] m & body]\n"
       "  `(let [{:keys [~a ~b]} ~m] ~@body))\n\n"
       "(defmacro table [& rows]\n"
       "  `(vec (for [[k# v#] (partition 2 [~@rows])] {:key k# :value v# :macro " k "})))\n"))

(defn namespace-source
  "Ordinary namespace `i`, which requires its group's macro namespace and up
  to two earlier namespaces: a table of data, a function calling a macro and
  functions written with `fn*`, so that the program stays within the watch's
  1,024 macro expansions per compilation."
  [i macros]
  (let [m (* 10 (quot i 10))
        deps (filter #(and (>= % 0) (not (macros %))) [(- i 1) (- i 7)])]
    (str "(ns bench.n" i "\n"
         "  (:require [bench.macros" m " :as m]"
         (str/join (map #(str "\n            [bench.n" % " :as n" % "]") deps))
         "))\n\n"
         "(def table\n  ["
         (str/join "\n   " (for [j (range 20)]
                              (str "{:id " (+ (* 100 i) j) " :name \"item-" i "-" j
                                   "\" :tags #{:a" (mod j 3) " :b" (mod j 5) "} :weight " (* j 1.5) "}")))
         "])\n\n"
         "(def total\n  (fn* [xs] (reduce (fn* [acc x] (+ acc (get x :weight))) 0 xs)))\n\n"
         "(def names\n  (fn* [xs] (into [] (map (fn* [x] (get x :name))) xs)))\n\n"
         "(defn label [n]\n  (m/labeled \"n" i "\" n (total table) (count (names table))))\n\n"
         "(def value\n  (fn* []\n    [" (str/join " " (map #(str "(n" % "/value)") deps))
         " (label " i ")]))\n")))

(defn generate
  "The sources of a program of `n` namespaces under `dir`, entry bench.main."
  [dir n]
  (let [macros (set (range 0 n 10))]
    (fs/delete-tree dir)
    (fs/create-dirs (fs/path dir "bench"))
    (doseq [i (range n)]
      (spit (str (fs/path dir "bench" (str (if (macros i) "macros" "n") i ".cljs")))
            (if (macros i) (macro-namespace i) (namespace-source i macros))))
    (let [leaves (remove macros (range (max 0 (- n 10)) n))]
      (spit (str (fs/path dir "bench" "main.cljs"))
            (str "(ns bench.main\n  (:require "
                 (str/join "\n            " (map #(str "[bench.n" % " :as n" % "]") leaves))
                 "))\n\n(defn ^:dev/after-load reload [] nil)\n\n["
                 (str/join " " (map #(str "(n" % "/value)") leaves)) "]\n")))
    "bench.main"))

(def root (if namespaces (str (fs/path output "src")) (str (fs/path repo "examples/dev"))))
(def entry (if namespaces (generate root namespaces) "app.main"))

(defn run-once
  "Start the watch, wait for its watching line, stop it and return the
  counters of its report, its startup time and the dev build's digest."
  []
  (let [out (str (fs/path output "dev"))
        _ (fs/delete-tree out)
        proc (process/process (concat [binary "--watch" "--entry" entry "--source-root" root
                                       "--out-dir" out "--stats" "--poll-ms" "60000"]
                                      (when cache? ["--cache-dir" cache]))
                              {:err :pipe :out :inherit})
        lines (with-open [reader (io/reader (:err proc))]
                (loop [lines []]
                  (let [line (.readLine reader)]
                    (cond (nil? line) lines
                          (str/includes? line "; watching every ") (conj lines line)
                          :else (recur (conj lines line))))))]
    (process/destroy-tree proc)
    @proc
    (let [built (last lines)]
      (when-not (and built (str/includes? built "; watching every "))
        (throw (ex-info "The watch did not start" {:stderr (str/join "\n" lines)})))
      (into (sorted-map
             :startup-ms (parse-long (second (re-find #" in (\d+) ms; watching" built)))
             :digest (-> (process/shell {:out :string :dir out} "sh" "-c"
                                        "cat $(ls | sort) | shasum -a 256")
                         :out (str/split #" ") first))
            (for [line lines
                  :let [[_ k v] (re-matches #"(time\.[a-z-]+|namespaces|macro-calls|cache\.hits|cache\.misses) (\S+)" line)]
                  :when k]
              [(keyword k) (parse-double v)])))))

(doseq [_ (range runs)]
  (prn (run-once)))
