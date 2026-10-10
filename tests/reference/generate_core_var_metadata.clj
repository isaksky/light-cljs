;; Core Var metadata is data for the finite first-party runtime, not runtime code.
;; Upstream data is EPL-1.0: research/clojurescript/epl-v10.html.
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
        names (map second (re-seq #"(?m)^            \"([^\"\n]+)\" => Self::" (slurp "src/analyzer.rs")))
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
