(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def output (fs/file "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase58"))
(fs/create-dirs output)
(doseq [pattern ["*.bb" "*.log" "*.time" "*.edn"] file (fs/glob here pattern)]
  (fs/copy file (fs/file output (fs/file-name file)) {:replace-existing true}))
(def scratch (fs/file here "patch-inputs"))
(doseq [[side root] [["before" "/private/tmp/light-cljs-phase56/project"] ["after" (fs/file here "project")]]
        file ["src/v8_evaluator.rs" "src/v8_bridge.rs" "src/emitter.rs"]]
  (fs/create-dirs (fs/file scratch side "src"))
  (fs/copy (fs/file root file) (fs/file scratch side file) {:replace-existing true}))
(def delta (p/shell {:dir (str scratch) :out :string :err :string :continue true}
             "git" "diff" "--no-index" "--" "before" "after"))
(assert (= 1 (:exit delta)))
(spit (fs/file output "callback-errors.patch")
  (-> (:out delta) (str/replace "a/before/" "a/") (str/replace "b/after/" "b/")))
(def counts (reduce #(mapv + %1 %2) [0 0 0]
              (map #(mapv parse-long (rest %))
                (re-seq #"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored"
                  (slurp (fs/file here "tests-v8-4threads.log"))))))
(assert (= counts [1404 0 106]) counts)
(spit (fs/file output "validation.edn") (pr-str {:tests counts :threads 4 :clippy :passed :format :passed :node-workflows 3}))
(doseq [job ["render" "main-none" "main-advanced"]]
  (let [file (fs/file here (str job ".js"))]
    (p/shell "cmp" (str file) (str "/private/tmp/light-cljs-phase42/production/" job ".js"))
    (let [result (with-open [in (io/input-stream file)]
                   (p/shell {:in in :out :string :err :string :continue true}
                     "node" "--check" "--input-type=module"))]
      (assert (= (if (= job "render") 0 1) (:exit result)))
      (spit (fs/file output (str job "-syntax.txt"))
        (str "Byte-identical to phase42.\nExplicit ESM syntax exit: " (:exit result) "\n" (:out result) (:err result))))))
(spit (fs/file output "source-hashes.txt")
  (:out (apply p/shell {:out :string} "shasum" "-a" "256"
          (map #(str (fs/file here %)) ["light-cljs-callbacks" "render.js" "main-none.js" "main-advanced.js" "project/src/v8_evaluator.rs" "project/Cargo.lock"]))))
(println "Archived full candidate validation and cold build results.")
