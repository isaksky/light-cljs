(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(def here (fs/parent *file*))
(def output (fs/file "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase103"))
(fs/create-dirs output)
(doseq [label ["early-tiers" "default-tiers"]]
  (p/shell "cmp" (str (fs/file here (str label ".js"))) "/private/tmp/light-cljs-phase102/main-none.js")
  (assert (not (re-find #"(?i)(unrecognized|unknown|invalid).*flag" (slurp (fs/file here (str label ".time")))))))
(doseq [pattern ["*.bb" "*.time" "*.edn"] file (fs/glob here pattern)]
  (fs/copy file (fs/file output (fs/file-name file)) {:replace-existing true}))
(spit (fs/file output "source-hashes.txt")
  (:out (p/shell {:out :string} "shasum" "-a" "256"
    "/private/tmp/light-cljs-phase102/light-cljs-v8-lexical-js-vars"
    (str (fs/file here "early-tiers.js")) (str (fs/file here "default-tiers.js"))
    "/Users/admin/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/v8-152.2.0/v8/src/flags/flag-definitions.h")))
