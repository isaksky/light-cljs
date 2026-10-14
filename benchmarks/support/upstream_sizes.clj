;; Upstream column of `cargo bench --bench compiler -- --levels --upstream DIR`
;; (benches/support/levels.rs). Compiles each program namespace with ClojureScript at
;; :advanced and, for the size column, again with :lite-mode and :elide-to-string as
;; upstream's lite-mode ratchets do (build_api_tests.clj). Prints one tab-separated
;; line per output: UPSTREAM, program name, variant, path of the compiled main.js.
;;
;; levels.rs runs it with the ClojureScript checkout DIR as a :local/root dependency,
;; from a directory whose src/bench/ holds the program namespaces:
;;   clojure -Srepro \
;;     -Sdeps '{:paths ["src"] :deps {org.clojure/clojurescript {:local/root "DIR"}}}' \
;;     -M upstream_sizes.clj OUTPUT_DIR NAME FILE VARIANTS [NAME FILE VARIANTS ...]
;; FILE declares bench.NAME (so src/bench/NAME.cljs, dashes as underscores and dots as
;; directories). VARIANTS is `advanced` or `advanced,lite`. The steady-state workloads
;; are named steady.NAME and need only `advanced`, which is what levels.rs times.
(require '[cljs.build.api :as build] '[clojure.java.io :as io] '[clojure.string :as str])

(def variants {"advanced" {} "lite" {:lite-mode true :elide-to-string true}})

(let [[output & programs] *command-line-args*]
  (doseq [[name file wanted] (partition 3 programs)
          variant (str/split wanted #",")]
    (let [dir (io/file output variant name)
          main (io/file dir "main.js")]
      (build/build (build/inputs (io/file file))
                   (merge {:main (symbol (str "bench." name))
                           :output-dir (.getPath dir)
                           :output-to (.getPath main)
                           :optimizations :advanced}
                          (or (variants variant) (throw (ex-info (str "unknown variant " variant) {})))))
      (println (str "UPSTREAM\t" name "\t" variant "\t" (.getPath main))))))
