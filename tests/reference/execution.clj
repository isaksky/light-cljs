;; Used by the ignored Rust differential test. Only this reference-side harness
;; needs namespaces, arrays, interop, and the standard ClojureScript runtime.
(require '[cljs.build.api :as build]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(let [output (first *command-line-args*)
      src (io/file output "src")
      source-file (io/file src "semantic" "reference.cljs")
      cases (->> (str/split-lines (slurp "tests/fixtures/execution.tsv"))
                 (remove #(or (str/blank? %) (str/starts-with? % "#")))
                 (map #(second (str/split % #"\t"))))]
  (io/make-parents source-file)
  (spit source-file
        (str "(ns semantic.reference)\n"
             "(set! (.-lightCases js/globalThis) (array\n"
             (str/join "\n" (map #(str "(fn* [observe] " % ")") cases))
             "))\n"))
  (build/build (str src)
               {:main 'semantic.reference
                :target :nodejs
                :optimizations :none
                :output-to (str (io/file output "reference.js"))
                :output-dir (str (io/file output "out"))
                :asset-path "out"
                :parallel-build false}))
