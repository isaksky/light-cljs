(require '[babashka.fs :as fs] '[babashka.process :as p])
(def here (fs/parent *file*))
(def out (fs/file "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase89"))
(fs/create-dirs out)
(doseq [pattern ["*.bb" "*.time" "*.edn"] file (fs/glob here pattern)]
  (fs/copy file (fs/file out (fs/file-name file)) {:replace-existing true}))
(p/shell "cmp" (str (fs/file here "main-none.js")) "/private/tmp/light-cljs-phase42/production/main-none.js")
(spit (fs/file out "source-hashes.txt") (:out (p/shell {:out :string} "shasum" "-a" "256"
  "/private/tmp/light-cljs-phase88/light-cljs-quickjs-namespace-cache"
  (str (fs/file here "main-none.js")))))
