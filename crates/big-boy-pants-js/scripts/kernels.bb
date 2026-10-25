#!/usr/bin/env bb
(require '[babashka.fs :as fs]
         '[babashka.process :as p]
         '[clojure.string :as str])

;; Run from any directory. Rust owns kernel sources, sizes and expected results;
;; --node-kernels exports those exact inputs rather than maintaining JS copies.
(let [root (str (fs/normalize (fs/path (fs/parent *file*) "../../..")))
      args (if (some #{"kernels" "scaling"} *command-line-args*)
             *command-line-args*
             (cons "scaling" *command-line-args*))
      cargo ["cargo" "run" "--quiet" "--locked" "--release" "-p" "big-boy-pants-js"
             "--example" "bench" "--"]
      run (fn [cmd opts]
            (let [result @(p/process cmd (merge {:dir root :out :string :err :inherit} opts))]
              (when-not (zero? (:exit result)) (System/exit (:exit result)))
              (:out result)))
      engine (run (into cargo args) {})]
  (print engine)
  (flush)
  (if-let [node (fs/which "node")]
    (let [source (run (into cargo (cons "--node-kernels" args)) {})
          reference (run [(str node) "--jitless" "-"] {:in source})
          engine-rows (into {} (for [[_ name median]
                                    (re-seq #"kernels\.([a-z_]+)\s+threads=1\s+n=\d+\s+tasks=4\s+([\d.]+)" engine)]
                                [name (parse-double median)]))]
      (println "\nSingle-thread reference (same sources; ns/iteration, median [min..max]):")
      (println (format "%-18s %14s %26s %14s" "kernel" "engine median" "node --jitless" "engine / node"))
      (doseq [line (str/split-lines reference)
              :let [[name median spread] (str/split line #"\t")
                    engine-ns (get engine-rows name)]]
        (println (format "%-18s %14.1f %26s %13.2fx"
                         name engine-ns (str median " [" spread "]")
                         (/ engine-ns (parse-double median))))))
    (println "\nNode absent from PATH; optional --jitless reference omitted.")))
