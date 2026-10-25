#!/usr/bin/env bb
(require '[babashka.fs :as fs]
         '[babashka.process :as process]
         '[clojure.string :as str])

;; Run from any directory. Build this checkout's compiler, with QuickJS only
;; for compilation/macros. Engine tests themselves use no external evaluator.
(def root (fs/canonicalize (fs/path (fs/parent *file*) "../../..")))
(def fixtures (fs/path root "crates/big-boy-pants-js/tests/fixtures/light-cljs"))
(defn run [& args]
  (:out (apply process/shell {:dir (str root) :out :string :err :inherit
                             :extra-env {"CARGO_BUILD_JOBS" "3"}} args)))
(run "cargo" "build" "--locked" "-p" "light-cljs" "--no-default-features")
(def revision (str/trim (run "git" "rev-parse" "HEAD")))
(def runtime (atom nil))
(doseq [line (str/split-lines (slurp (str (fs/path fixtures "expected.tsv"))))]
  (let [[name _] (str/split line #"\t")
        output (run (str (fs/path root "target/debug/light-cljs"))
                    "--compile" (str (fs/path fixtures (str name ".cljs"))))
        start (+ (str/index-of output "  const rt = ") (count "  const rt = "))
        end (str/index-of output "\n  rt.core.registerReaderNamespace" start)
        factory (subs output start (dec end))]
    (if @runtime
      (assert (= @runtime factory) (str "runtime differs for " name))
      (reset! runtime factory))
    ;; Exact reassembly replaces only this marker with the one shared factory.
    ;; No JavaScript is rewritten and the emitted program remains an expression.
    (spit (str (fs/path fixtures (str name ".js")))
          (str (subs output 0 start) "/* LIGHT_CLJS_RUNTIME */" (subs output (dec end))))))
(spit (str (fs/path fixtures "runtime.js")) @runtime)
(fs/copy (fs/path root "LICENSE") (fs/path fixtures "LICENSE") {:replace-existing true})
(spit (str (fs/path fixtures "generation.txt"))
      (str "Compiler revision: " revision "\n"
           "Command: bb crates/big-boy-pants-js/tools/generate-light-cljs.bb\n"
           "Compiler: cargo build --locked -p light-cljs --no-default-features\n"
           "Each source: target/debug/light-cljs --compile <name>.cljs\n"
           "Output: exact emitted text with its common factory factored into runtime.js.\n"))
