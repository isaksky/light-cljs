(require '[babashka.fs :as fs] '[babashka.process :as p]
         '[clojure.java.io :as io] '[clojure.string :as str])
(def here (fs/parent *file*))
(def body (second (re-find #"(?s)let body = r#\"(.*?)\"#;" (slurp (fs/file here "list-test.rs")))))
(assert body)
(spit (fs/file here "list-count.tsv")
      (str "list-count\t" (str/replace body #"\s+" " ") "\t[1 999 3 [10 20 30]]\n"))
(def output (fs/file here "reference"))
(fs/create-dirs output)
(with-open [out (io/output-stream (fs/file here "reference-build.log"))]
  (p/shell {:out out :err :out}
    "clojure" "-Srepro" "-Sdeps" "{:deps {org.clojure/clojurescript {:mvn/version \"1.12.134\"}}}"
    "-M" "tests/reference/execution.clj" "release" "1.12.134" "-"
    (str output) "cases" (str (fs/file here "list-count.tsv"))))
(def script (str "process.chdir(" (pr-str (str output)) "); require("
                 (pr-str (str (fs/file output "reference.js")))
                 "); for (const f of globalThis.lightCases) console.log(cljs.core.pr_str(f(() => { throw Error('unexpected observer'); })));"))
(def result (:out (p/shell {:out :string} "node" "-e" script)))
(spit (fs/file here "reference-result.log") result)
(assert (= "[1 999 3 [10 20 30]]" (str/trim result)))
(print result)
