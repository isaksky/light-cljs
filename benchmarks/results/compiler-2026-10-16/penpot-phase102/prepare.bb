(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(def here (fs/parent *file*))
(def project (fs/file here "project"))
(when-not (fs/exists? project)
  (p/shell "cp" "-a" "/private/tmp/light-cljs-phase99/project" (str project)))
(def inputs (fs/file here "patch-inputs"))
(def files ["src/analyzer.rs" "src/js_syntax.rs" "src/emitter.rs" "src/emitter/chunked.rs"
            "tests/compiler/macros.rs" "tests/compiler/optimization.rs" "docs/compiler.md"])
(doseq [file files]
  (fs/create-dirs (fs/parent (fs/file project file)))
  (let [original (fs/file "/private/tmp/light-cljs-phase99/project" file)]
    (fs/copy (if (fs/exists? original) original
              (fs/file "/private/tmp/light-cljs-phase100/project" file))
      (fs/file project file) {:replace-existing true})))
(doseq [[side root] [["before" "/private/tmp/light-cljs-phase100/project"]
                    ["after" "/private/tmp/light-cljs-phase101/project"]]]
  (doseq [file files]
    (fs/create-dirs (fs/parent (fs/file inputs side file)))
    (fs/copy (fs/file root file) (fs/file inputs side file) {:replace-existing true})))
(def difference (p/shell {:dir (str inputs) :out :string :continue true}
                 "git" "diff" "--no-index" "--" "before" "after"))
(assert (= 1 (:exit difference)))
(def patch (fs/file here "lexical-js-vars.patch"))
(spit patch (-> (:out difference) (str/replace "a/before/" "a/") (str/replace "b/after/" "b/")))
(p/shell {:dir (str project)} "patch" "--batch" "-p1" "--forward" "-i" (str patch))
(fs/delete-if-exists (fs/file project "Oops.rej"))
(spit (fs/file here "validate.bb")
  (str/replace (slurp "/private/tmp/light-cljs-phase99/validate.bb")
    "prepared deferred units" "V8 lexical JS assignment"))
