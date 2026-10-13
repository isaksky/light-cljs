;; Upstream size column of `cargo bench --bench compiler -- --levels --upstream DIR`
;; (benches/support/levels.rs). Compiles each fixture namespace with ClojureScript at
;; :advanced, then again with :lite-mode and :elide-to-string as upstream's lite-mode
;; ratchets do (build_api_tests.clj), and prints one tab-separated line per output:
;; UPSTREAM, fixture name, variant, path of the compiled main.js.
;;
;; levels.rs runs it with the ClojureScript checkout DIR as a :local/root dependency,
;; from a directory whose src/bench/ holds the fixture namespaces:
;;   clojure -Srepro \
;;     -Sdeps '{:paths ["src"] :deps {org.clojure/clojurescript {:local/root "DIR"}}}' \
;;     -M upstream_sizes.clj OUTPUT_DIR NAME FILE [NAME FILE ...]
;; FILE is src/bench/NAME.cljs (dashes as underscores) and declares bench.NAME.
(require '[cljs.build.api :as build] '[clojure.java.io :as io])

(let [[output & pairs] *command-line-args*]
  (doseq [[name file] (partition 2 pairs)
          [variant options] [["advanced" {}]
                             ["lite" {:lite-mode true :elide-to-string true}]]]
    (let [dir (io/file output variant name)
          main (io/file dir "main.js")]
      (build/build (build/inputs (io/file file))
                   (merge {:main (symbol (str "bench." name))
                           :output-dir (.getPath dir)
                           :output-to (.getPath main)
                           :optimizations :advanced}
                          options))
      (println (str "UPSTREAM\t" name "\t" variant "\t" (.getPath main))))))
