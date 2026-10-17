(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(def here (fs/parent *file*))
(def project (fs/file here "project"))
(def archive "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16")
(defn patch! [phase & options]
  (let [file (str (fs/file archive (str "penpot-phase" phase) "changes.patch"))]
    (apply p/shell {:dir (str project)} "git" "apply" "--check" (concat options [file]))
    (apply p/shell {:dir (str project)} "git" "apply" (concat options [file]))))
(patch! 76 "--exclude=src/analyzer.rs")
;; The reader-lookup preparation replaces only the private reader-view region;
;; it leaves QuickJS's different namespace-global bookkeeping in place.
(fs/copy "/private/tmp/light-cljs-phase76/prepare.bb" (fs/file here "prepare-reader.bb") {:replace-existing true})
(p/shell "bb" (str (fs/file here "prepare-reader.bb")))
(doseq [phase [78 83 84 85]] (patch! phase))
(spit (fs/file here "run.bb")
  (-> (slurp "/private/tmp/light-cljs-phase72/run.bb")
      (str/replace "light-cljs-quickjs-unit-hygiene" "light-cljs-quickjs-shared-storage")
      (str/replace "QuickJS unit hygiene" "QuickJS shared storage")))
(spit (fs/file here "validate.bb")
  (-> (slurp "/private/tmp/light-cljs-phase72/validate.bb")
      (str/replace "QuickJS unit hygiene" "QuickJS shared storage")
      (str/replace "[1400 0 106]" "[1401 0 106]")))
