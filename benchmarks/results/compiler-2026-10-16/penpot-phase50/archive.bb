(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(def here (fs/parent *file*))
(def output (fs/file "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase50"))
(fs/create-dirs output)
(doseq [pattern ["*.bb" "*.log" "*.time" "*.command.edn" "sources.txt"] file (fs/glob here pattern)]
  (fs/copy file (fs/file output (fs/file-name file)) {:replace-existing true}))
(def paths ["Cargo.toml" "Cargo.lock" "src/main.rs"])
(def scratch (fs/file here "patch-inputs"))
(doseq [file paths
        [side root] [["before" "/private/tmp/light-cljs-phase47/project"] ["after" (fs/file here "project")]]]
  (fs/create-dirs (fs/parent (fs/file scratch side file)))
  (fs/copy (fs/file root file) (fs/file scratch side file) {:replace-existing true}))
(def delta (p/shell {:dir (str scratch) :out :string :err :string :continue true}
             "git" "diff" "--no-index" "--" "before" "after"))
(assert (= 1 (:exit delta)))
(spit (fs/file output "allocator.patch")
  (-> (:out delta) (str/replace "a/before/" "a/") (str/replace "b/after/" "b/")))
(spit (fs/file output "source-hashes.txt")
  (:out (apply p/shell {:out :string} "shasum" "-a" "256"
          (map str (concat [(fs/file here "light-cljs-mimalloc") (fs/file here "main-mimalloc.js")]
                     (map #(fs/file here "project" %) paths))))))
(println "Archived CLI allocator trial.")
