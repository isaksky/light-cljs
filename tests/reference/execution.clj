;; Explicit release/checkout oracle. Its runtime/namespace features are reference
;; plumbing, not features supplied by light-cljs.
(require '[cljs.build.api :as build]
         '[cljs.analyzer :as ana]
         '[cljs.util :as util]
         '[clojure.java.io :as io]
         '[clojure.java.shell :as shell]
         '[clojure.string :as str])

(defn checked-reference [kind identity checkout]
  (case kind
    "release"
    (do
      (assert (= identity "1.12.134" (util/clojurescript-version))
              "The required oracle is Maven ClojureScript 1.12.134")
      (assert (str/starts-with? (str (io/resource "cljs/analyzer.cljc")) "jar:")
              "The release compiler must be loaded from its Maven jar"))
    "checkout"
    (let [root (.getCanonicalFile (io/file checkout))
          git (fn [& args]
                (let [{:keys [exit out err]} (apply shell/sh "git" "-C" (str root) args)]
                  (assert (zero? exit) (str "Checkout git validation failed: " err))
                  (str/trim out)))]
      (assert (re-matches #"[0-9a-f]{40}" identity) "Checkout requires a full commit SHA")
      (assert (= (str root) (git "rev-parse" "--show-toplevel")) "Checkout must be a repository root")
      (assert (= identity (git "rev-parse" "HEAD")) "Checkout revision mismatch")
      (assert (str/blank? (git "status" "--porcelain" "--untracked-files=normal"))
              "Checkout must be clean to identify the source by commit")
      (assert (.isFile (io/file root "deps.edn")) "Checkout deps.edn is missing")
      ;; A release on the classpath must never silently replace checkout code.
      (doseq [path ["cljs/build/api.clj" "cljs/analyzer.cljc" "cljs/compiler.cljc"
                    "cljs/core.cljs"]]
        (let [resource (io/resource path)
              source-root (if (str/ends-with? path ".cljs") "src/main/cljs" "src/main/clojure")]
          (assert (and resource (= "file" (.getProtocol resource))
                       (= (.getCanonicalFile (io/file root source-root path))
                          (.getCanonicalFile (io/file resource))))
                  (str "Checkout source was not loaded: " path " -> " resource)))))
    (throw (ex-info "Reference must be release or checkout" {:reference kind})))
  (println "REFERENCE IDENTITY" kind identity)
  (println "REFERENCE RUNTIME" (pr-str {:clojurescript (util/clojurescript-version)
                                       :clojure (clojure-version)
                                       :java (System/getProperty "java.runtime.version")})))

(def expected-settings
  {:optimizations :none :static-fns false :def-emits-var false})

(defn checked-build [source output main checked-namespaces]
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
    (println "REFERENCE SETTINGS" (pr-str expected-settings))))

(try
  (let [[kind identity checkout output mode input entry] *command-line-args*]
    (assert output (str "Usage: execution.clj release 1.12.134 - OUTPUT [cases TSV | program SOURCE_ROOT ENTRY_NS]"
                        " or checkout COMMIT CHECKOUT OUTPUT [cases TSV | program SOURCE_ROOT ENTRY_NS]"))
    (checked-reference kind identity checkout)
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
        ;; Verify effective settings in every namespace under the fixture root.
        (let [namespaces (->> (file-seq (io/file input))
                              (filter #(str/ends-with? (str %) ".cljs"))
                              (map #(with-open [reader (java.io.PushbackReader. (io/reader %))]
                                      (second (read reader))))
                              set)]
          (checked-build input output (symbol entry) namespaces)))
      (throw (ex-info "Unknown reference harness mode" {:mode mode})))
    ;; Detect a checkout changed while it was compiling as well.
    (checked-reference kind identity checkout))
  (finally (shutdown-agents)))
