;; Core Var metadata is data for the finite first-party runtime, not runtime code.
;; Upstream data is EPL-1.0: epl-v10.html at the repository root.
(require '[cljs.analyzer :as ana] '[cljs.env :as env] '[clojure.java.io :as io]
         '[clojure.string :as str] '[clojure.java.shell :as shell])
(def revision "a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e")
(assert (= revision (str/trim (:out (shell/sh "git" "-C" "research/clojurescript" "rev-parse" "HEAD")))))
(assert (str/blank? (:out (shell/sh "git" "-C" "research/clojurescript" "status" "--porcelain"))))
(assert (= (.getCanonicalFile (io/file "research/clojurescript/src/main/cljs/cljs/core.cljs"))
           (.getCanonicalFile (io/file (io/resource "cljs/core.cljs")))))
(env/with-compiler-env (env/default-compiler-env)
  (ana/analyze-file (io/resource "cljs/core.cljs"))
  (let [defs (get-in @env/*compiler* [:cljs.analyzer/namespaces 'cljs.core :defs])
        ;; Read every spelling in the resolver, including combined alias arms
        ;; and arms whose Self expression is on the following line.
        resolver (second (str/split (slurp "src/analyzer.rs") #"pub\(crate\) fn resolve\(name: &str\)" 2))
        resolver (first (str/split resolver #"\n    }" 2))
        names (map second (re-seq #"\"([^\"\n]+)\"" resolver))
        missing (set (remove #(contains? defs (symbol %)) names))
        boundaries (set (for [row (str/split-lines (slurp "tests/reference/core-var-metadata-boundaries.tsv"))
                             :when (not (str/starts-with? row ";;"))]
                         (first (str/split row #"\t"))))
        _ (assert (= missing boundaries) (str "Mapped metadata boundary drift: " missing))
        result (str ";; Generated from ClojureScript " revision " core definitions; EPL-1.0.\n"
                 (apply str (for [n (sort names) :let [v (get defs (symbol n))] :when v]
                   (str n "\t" (pr-str
                     (merge (dissoc (:meta v) :test :arglists)
                       (select-keys v [:doc :line :column])
                       {:file "cljs/core.cljs" :arglists (let [a (:arglists v)] (or (if (= 'quote (first a)) (second a) a) '()))})) "\n"))))]
    (if (= ["--check"] *command-line-args*)
      (assert (= result (slurp "src/core_var_metadata.tsv")) "Core metadata drift")
      (spit "src/core_var_metadata.tsv" result))))
(shutdown-agents)
