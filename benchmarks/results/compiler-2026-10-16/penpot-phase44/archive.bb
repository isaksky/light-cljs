(require '[babashka.fs :as fs] '[babashka.process :as p]
         '[clojure.string :as str] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def repo (fs/file "/Users/admin/src/light-cljs"))
(def project (fs/file here "project"))
(def destination (fs/file repo "benchmarks/results/compiler-2026-10-16/penpot-phase44"))
(fs/create-dirs destination)
(def paths ["Cargo.toml" "Cargo.lock" "src/lib.rs" "src/evaluator.rs"
            "src/form_bridge.rs" "src/compiler_bootstrap.rs" "src/macro_host.rs"
            "src/compilation_session.rs" "src/emitter.rs" "src/v8_evaluator.rs"
            "src/v8_bridge.rs" "src/v8_services.js"])
(def scratch (fs/file here "patch-inputs"))
(doseq [file paths]
  (doseq [[base root] [["baseline" repo] ["prototype" project]]]
    (let [from (fs/file root file) to (fs/file scratch base file)]
      (when (fs/exists? from)
        (fs/create-dirs (fs/parent to))
        (fs/copy from to {:replace-existing true})))))
(def patch
  (apply str
    (for [file paths]
      (let [baseline (if (fs/exists? (fs/file scratch "baseline" file))
                       (str "baseline/" file) "/dev/null")
            result (p/shell {:dir (str scratch) :out :string :err :string :continue true}
                     "git" "diff" "--no-index" "--" baseline (str "prototype/" file))]
        (assert (#{0 1} (:exit result)) (:err result))
        (-> (:out result)
            (str/replace "a/baseline/" "a/")
            (str/replace "a/prototype/" "a/")
            (str/replace "b/prototype/" "b/"))))))
(spit (fs/file destination "prototype.patch") patch)
(p/shell {:dir (str repo)} "git" "apply" "--check" (str (fs/file destination "prototype.patch")))
(doseq [pattern ["*.bb" "timers.rs" "*.time" "*.log" "*.command.edn" "*-summary.txt" "*-syntax.txt"]
        file (fs/glob here pattern)]
  (fs/copy file (fs/file destination (fs/file-name file)) {:replace-existing true}))
(defn sha [file]
  (let [digest (java.security.MessageDigest/getInstance "SHA-256")]
    (with-open [input (io/input-stream (str file))]
      (let [buffer (byte-array 65536)]
        (loop []
          (let [n (.read input buffer)]
            (when (pos? n) (.update digest buffer 0 n) (recur))))))
    (format "%064x" (java.math.BigInteger. 1 (.digest digest)))))
(spit (fs/file destination "source-hashes.txt")
  (apply str
    (for [file (concat (map #(fs/file project %) paths)
                       (fs/glob here "light-cljs-v*")
                       (filter #(pos? (fs/size %)) (fs/glob here "*.js"))
                       [(fs/file here "render-v9.heapsnapshot")])]
      (str (sha file) "  " file "\n"))))
(let [files (sort-by str
              (filter fs/regular-file?
                (mapcat #(fs/glob (fs/file repo "research/penpot-light" %) "**")
                  ["frontend/src" "common/src" "vendor" "generated" "shims"])))
      digest (java.security.MessageDigest/getInstance "SHA-256")]
  (doseq [file files]
    (.update digest (.getBytes (str (fs/relativize repo file)) "UTF-8"))
    (.update digest (fs/read-all-bytes file)))
  (let [result (str (count files) " files\n" (format "%064x" (java.math.BigInteger. 1 (.digest digest))) "\n")]
    (assert (= result (slurp (fs/file repo "benchmarks/results/compiler-2026-10-16/penpot-phase42/port-hash.txt"))))
    (spit (fs/file destination "port-hash.txt") result)))
(println "Archived isolated prototype; patch applies to the retained production worktree.")
