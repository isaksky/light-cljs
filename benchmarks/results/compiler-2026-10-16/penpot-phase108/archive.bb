(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.edn :as edn]
         '[clojure.string :as str])
(def here (fs/parent *file*))
(def output (fs/file "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase108"))
(assert (= [1426 0 106] (:tests (edn/read-string (slurp (fs/file here "validation.edn"))))))
(fs/create-dirs output)
(doseq [pattern ["*.bb" "*.log" "*.time" "*.edn" "*.txt" "*.json" "*.gitmodules"]
        file (fs/glob here pattern)]
  (let [destination (fs/file output (fs/file-name file))]
    (if (str/ends-with? (str file) ".log")
      (spit destination
        (str/replace (slurp (str file)) #"(https://storage.googleapis.com/chrome-infra-packages/[^?\s]+)\?[^\s]+" "$1?[temporary download signature omitted]"))
      (fs/copy file destination {:replace-existing true}))))
(doseq [label ["render" "main-none" "main-advanced"]]
  (p/shell "cmp" (str (fs/file here (str label ".js")))
    (str "/private/tmp/light-cljs-phase107/" label ".js")))
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
      ["light-cljs-v8-source-baseline" "render.js" "main-none.js" "main-advanced.js" "baseline-main.js"
       "project/src/v8_evaluator.rs" "project/src/v8_watchdog.rs"
       "v8/Cargo.toml" "v8/build.rs" "v8/src/binding.cc"
       "v8/v8/src/objects/backing-store.cc" "v8/v8/src/objects/backing-store.h"
       "v8/v8/include/v8-array-buffer.h"]))))
(println "Pinned engine source-build evidence archived")
