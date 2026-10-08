;; Required release-oracle harness. Its runtime/namespace features are reference
;; plumbing, not features supplied by light-cljs.
(require '[cljs.build.api :as build]
         '[cljs.analyzer :as ana]
         '[cljs.util :as util]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def expected-settings
  {:optimizations :none :static-fns false :def-emits-var false})

(defn checked-build [source output main checked-namespaces]
  (assert (= "1.12.134" (util/clojurescript-version))
          "The required oracle is Maven ClojureScript 1.12.134")
  (let [empty-env ana/empty-env
        analyze ana/analyze
        observed (atom #{})]
    ;; def-emits-var belongs to the analysis environment, not the build options.
    ;; Pin it explicitly and inspect the live settings while analyzing fixtures.
    (binding [ana/*cljs-static-fns* false]
      (with-redefs [ana/empty-env #(assoc (empty-env) :def-emits-var false)
                    ana/analyze
                    (fn [& args]
                      (when (checked-namespaces (get-in (first args) [:ns :name]))
                        (let [effective {:optimizations (:optimizations (ana/compiler-options))
                                         :static-fns ana/*cljs-static-fns*
                                         :def-emits-var (:def-emits-var (first args))}]
                          (assert (= expected-settings effective)
                                  (str "Unexpected reference settings: " effective))
                          (swap! observed conj effective)))
                      (apply analyze args))]
        (build/build (str source)
                     {:main main
                      :target :nodejs
                      :optimizations :none
                      :static-fns false
                      :cache-analysis false
                      :output-to (str (io/file output "reference.js"))
                      :output-dir (str (io/file output "out"))
                      :asset-path "out"
                      :parallel-build false})))
    (assert (= #{expected-settings} @observed)
            "No fixture analysis settings were observed")
    (println "REFERENCE SETTINGS" (util/clojurescript-version) (pr-str expected-settings))))

(let [[output mode input entry] *command-line-args*]
  (assert output "Usage: execution.clj OUTPUT [cases TSV | program SOURCE_ROOT ENTRY_NS]")
  (case (or mode "cases")
    "cases"
    (let [src (io/file output "src")
          source-file (io/file src "semantic" "reference.cljs")
          cases (->> (str/split-lines (slurp (or input "tests/fixtures/execution.tsv")))
                     (remove #(or (str/blank? %) (str/starts-with? % "#")))
                     (map #(second (str/split % #"\t"))))
          namespaces (mapv #(symbol (str "semantic.case" %)) (range (count cases)))]
      (assert (seq cases) "The reference fixture contains no cases")
      ;; Give each case its own namespace, like each independent light-cljs
      ;; compilation. Definitions must not leak between differential cases.
      (doseq [[case-ns source] (map vector namespaces cases)]
        (let [file (io/file src "semantic" (str (last (str/split (str case-ns) #"\.")) ".cljs"))]
          (io/make-parents file)
          (spit file (str "(ns " case-ns ")\n(def run (fn* [observe] " source "))\n"))))
      (io/make-parents source-file)
      (spit source-file
            (str "(ns semantic.reference (:require "
                 (str/join " " (map #(str "[" % "]") namespaces)) "))\n"
                 "(set! (.-lightCases js/globalThis) (array\n"
                 (str/join "\n" (map #(str % "/run") namespaces)) "))\n"))
      (checked-build src output 'semantic.reference
                     (conj (set namespaces) 'semantic.reference)))
    "program"
    (do
      (assert (and input entry) "Program mode requires SOURCE_ROOT and ENTRY_NS")
      (checked-build input output (symbol entry) #{(symbol entry)}))
    (throw (ex-info "Unknown reference harness mode" {:mode mode}))))
