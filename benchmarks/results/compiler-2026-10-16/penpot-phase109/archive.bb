(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.edn :as edn]
         '[clojure.string :as str])
(def here (fs/parent *file*))
(def output (fs/file "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase109"))
(assert (= [1435 0 106] (:tests (edn/read-string (slurp (fs/file here "validation.edn"))))))
(fs/create-dirs output)
(defn patch [baseline current files]
  (apply str (for [file files]
    (let [result (p/shell {:out :string :err :string :continue true}
                   "diff" "-u" "--label" (str "a/" file) "--label" (str "b/" file)
                   (str (if (fs/exists? (fs/file baseline file)) (fs/file baseline file) "/dev/null"))
                   (str (fs/file current file)))]
      (assert (contains? #{0 1} (:exit result)) result)
      (:out result)))))
(spit (fs/file here "engine-budget.patch")
  (patch "/private/tmp/light-cljs-phase108/v8" (fs/file here "v8")
    ["v8/include/v8-array-buffer.h" "v8/src/objects/backing-store.h" "v8/src/objects/backing-store.cc"]))
(spit (fs/file here "compiler-budget.patch")
  (patch "/private/tmp/light-cljs-phase107/project" (fs/file here "project")
    ["build.rs" "src/v8_evaluator.rs" "src/v8_buffers.cc" "src/v8_buffers.rs"
     "src/v8_buffer_tests.rs" "vendor/v8-headers/include/v8-array-buffer.h"]))
(doseq [pattern ["*.bb" "*.log" "*.time" "*.edn" "*.txt" "*.patch"]
        file (fs/glob here pattern)]
  (let [destination (fs/file output (fs/file-name file))]
    (if (str/ends-with? (str file) ".log")
      (spit destination
        (str/replace (slurp (str file)) #"(https://storage.googleapis.com/chrome-infra-packages/[^?\s]+)\?[^\s]+" "$1?[temporary download signature omitted]"))
      (fs/copy file destination {:replace-existing true}))))
(doseq [label ["render" "main-none" "main-advanced"]]
  (p/shell "cmp" (str (fs/file here (str label ".js")))
    (str "/private/tmp/light-cljs-phase108/" label ".js")))
(doseq [name ["Cargo.toml" "Cargo.lock"]]
  (fs/copy (fs/file here "project" name) (fs/file output (str "project-" name)) {:replace-existing true}))
(doseq [[source destination] [["v8/.cargo_vcs_info.json" "engine-crate-vcs.json"]
                              ["v8/third_party/rust-toolchain/VERSION" "rust-toolchain-version.txt"]
                              ["frozen-engine/args.gn" "engine-args.gn"]]]
  (fs/copy (fs/file here source) (fs/file output destination) {:replace-existing true}))
(spit (fs/file output "native-compiler-version.txt")
  (:out (p/shell {:out :string} (str (fs/file here "target/release/clang/bin/clang")) "--version")))
(spit (fs/file output "source-hashes.txt")
  (:out (apply p/shell {:out :string} "shasum" "-a" "256"
    (map #(str (fs/file here %))
      ["light-cljs-v8-buffer-budget" "render.js" "main-none.js" "main-advanced.js"
       "project/src/v8_evaluator.rs" "project/src/v8_watchdog.rs"
       "project/src/v8_buffers.rs" "project/src/v8_buffers.cc" "project/src/v8_buffer_tests.rs"
       "v8/Cargo.toml" "v8/build.rs" "v8/src/binding.cc"
       "v8/v8/src/objects/backing-store.cc" "v8/v8/src/objects/backing-store.h"
       "v8/v8/include/v8-array-buffer.h"]))))
(let [repo (fs/file "/Users/admin/src/light-cljs")
      files (sort-by str
              (filter fs/regular-file?
                (mapcat #(fs/glob (fs/file repo "research/penpot-light" %) "**")
                  ["frontend/src" "common/src" "vendor" "generated" "shims"])))
      digest (java.security.MessageDigest/getInstance "SHA-256")]
  (doseq [file files]
    (.update digest (.getBytes (str (fs/relativize repo file)) "UTF-8"))
    (.update digest (fs/read-all-bytes file)))
  (let [result (str (count files) " files\n" (format "%064x" (java.math.BigInteger. 1 (.digest digest))) "\n")]
    (assert (= result (slurp (fs/file repo "benchmarks/results/compiler-2026-10-16/penpot-phase42/port-hash.txt"))))
    (spit (fs/file output "port-hash.txt") result)))
(println "Backing-store quota evidence archived")
