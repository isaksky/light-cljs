(require '[babashka.fs :as fs] '[babashka.process :as p])
(def here (fs/parent *file*))
(def output (fs/file "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase56"))
(fs/create-dirs output)
(doseq [pattern ["*.bb" "*.log" "*.time" "*.edn"] file (fs/glob here pattern)]
  (fs/copy file (fs/file output (fs/file-name file)) {:replace-existing true}))
(spit (fs/file output "source-hashes.txt")
  (:out (apply p/shell {:out :string} "shasum" "-a" "256"
          (map #(str (fs/file here %)) ["light-cljs-thin-lto" "main-thin-lto.js" "project/src/analyzer.rs" "project/Cargo.lock"]))))
(p/shell "diff" "-qr" "/private/tmp/light-cljs-phase55/project/src" (str (fs/file here "project/src")))
(println "Archived thin LTO trial; source matches phase55.")
