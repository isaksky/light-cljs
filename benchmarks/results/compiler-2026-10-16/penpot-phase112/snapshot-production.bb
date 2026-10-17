(require '[babashka.fs :as fs])
(def here (fs/parent *file*))
(def root (fs/path "/Users/admin/src/light-cljs"))
(def files
  (concat ["Cargo.toml" "Cargo.lock" "build.rs" "README.md" "THIRD_PARTY_NOTICES.md"
           "docs/compiler.md" "docs/evaluator.md" ".github/workflows/conformance.yml"
           "playground/crate/Cargo.lock"]
          (map #(str "src/" %) ["analyzer.rs" "compilation_session.rs" "compiler_bootstrap.rs"
               "eval_compiler.rs" "evaluator.rs" "form_bridge.rs" "lib.rs" "macro_host.rs" "main.rs"])))
(defn sha [f]
  (format "%064x" (java.math.BigInteger. 1
    (.digest (java.security.MessageDigest/getInstance "SHA-256") (fs/read-all-bytes f)))))
(doseq [file files]
  (let [target (fs/file here "before-native-import" file)]
    (assert (not (fs/exists? target)))
    (fs/create-dirs (fs/parent target))
    (fs/copy (fs/file root file) target)))
(spit (fs/file here "production-before-import.edn")
      (pr-str (into (sorted-map) (map #(vector % (sha (fs/file root %))) files))))
