(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str]
         '[clojure.edn :as edn] '[clojure.java.io :as io])
(def repo (fs/file "/Users/admin/src/light-cljs"))
(def phases (if (seq *command-line-args*) (map parse-long *command-line-args*) [96 97]))
(doseq [phase phases]
  (let [here (fs/file (str "/private/tmp/light-cljs-phase" phase))
        out (fs/file repo "benchmarks/results/compiler-2026-10-16" (str "penpot-phase" phase))
        old (fs/file (str "/private/tmp/light-cljs-phase" (if (= phase 96) 85 96) "/project"))
        binary (if (= phase 96) "light-cljs-shared-watchdog" "light-cljs-v8-stack-limits")
        files (if (= phase 96) ["src/v8_evaluator.rs" "src/v8_watchdog.rs"]
                ["Cargo.toml" "Cargo.lock" "build.rs" "src/v8_evaluator.rs" "src/v8_stack.rs"
                 "src/v8_stack.cc" "src/v8_thread_stack.cc" "vendor/v8-headers/README.md" "vendor/v8-headers/LICENSE"])
        counts (:tests (edn/read-string (slurp (fs/file here "validation.edn"))))]
    (assert (= counts [(if (= phase 96) 1412 1416) 0 106]) counts)
    (fs/create-dirs out)
    (doseq [pattern ["*.bb" "*.log" "*.edn" "*.json" "*.cc" "*.sh" "*.ps1"] file (fs/glob here pattern)]
      (fs/copy file (fs/file out (fs/file-name file)) {:replace-existing true}))
    (doseq [folder ["clean" "stack-comparison" "linux-results" "windows-results" "linux-header-warning" "windows-probe-warning" "windows-probe-link"]
            :let [directory (fs/file here folder)] :when (fs/exists? directory)]
      (fs/create-dirs (fs/file out folder))
      (doseq [pattern ["*.bb" "*.time" "*.edn" "*.json" "*.log" "*.txt"] file (fs/glob directory pattern)]
        (fs/copy file (fs/file out folder (fs/file-name file)) {:replace-existing true})))
    (let [scratch (fs/file here "archive-patch-inputs")]
      (doseq [[side root] [["before" old] ["after" (fs/file here "project")]] file files
              :when (fs/exists? (fs/file root file))]
        (fs/create-dirs (fs/parent (fs/file scratch side file)))
        (fs/copy (fs/file root file) (fs/file scratch side file) {:replace-existing true}))
      (let [result (p/shell {:dir (str scratch) :out :string :continue true} "git" "diff" "--no-index" "--" "before" "after")]
        (assert (= (:exit result) 1))
        (spit (fs/file out "changes.patch")
          (-> (:out result) (str/replace "a/before/" "a/") (str/replace "b/after/" "b/")))))
    (doseq [label ["render" "main-none" "main-advanced"]]
      (let [output (fs/file here "clean" (str label ".js"))]
        (p/shell "cmp" (str output) (str "/private/tmp/light-cljs-phase42/production/" label ".js"))
        (let [result (with-open [in (io/input-stream output)]
                       (p/shell {:in in :out :string :err :string :continue true} "node" "--check" "--input-type=module"))]
          (assert (= (:exit result) (if (= label "render") 0 1)))
          (spit (fs/file out "clean" (str label "-syntax.txt"))
            (str "Byte-identical to phase42. Explicit ESM syntax exit: " (:exit result) "\n" (:out result) (:err result))))))
    (spit (fs/file out "source-hashes.txt")
      (:out (apply p/shell {:out :string} "shasum" "-a" "256"
              (concat (map #(str (fs/file here %)) [binary "clean/render.js" "clean/main-none.js" "clean/main-advanced.js"])
                (map #(str (fs/file here "project" %)) files)))))
    (when (= phase 97)
      (let [headers (sort-by str (filter fs/regular-file? (fs/glob (fs/file here "project/vendor/v8-headers/include") "**")))]
        (spit (fs/file out "v8-header-hashes.txt")
          (:out (apply p/shell {:out :string} "shasum" "-a" "256" (map str headers))))))
    (println "Archived phase" phase)))
(let [files (sort-by str (filter fs/regular-file?
              (mapcat #(fs/glob (fs/file repo "research/penpot-light" %) "**")
                ["frontend/src" "common/src" "vendor" "generated" "shims"])))
      digest (java.security.MessageDigest/getInstance "SHA-256")]
  (doseq [file files]
    (.update digest (.getBytes (str (fs/relativize repo file)) "UTF-8"))
    (.update digest (fs/read-all-bytes file)))
  (let [result (str (count files) " files\n" (format "%064x" (java.math.BigInteger. 1 (.digest digest))) "\n")]
    (assert (= result (slurp (fs/file repo "benchmarks/results/compiler-2026-10-16/penpot-phase42/port-hash.txt"))))
    (doseq [phase phases]
      (spit (fs/file repo "benchmarks/results/compiler-2026-10-16" (str "penpot-phase" phase) "port-hash.txt") result))))
