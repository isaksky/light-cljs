(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def project (fs/file "/private/tmp/light-cljs-phase21/project"))
(when-not (fs/exists? (fs/file project "research"))
  (fs/create-sym-link (fs/file project "research") "/Users/admin/src/light-cljs/research"))
(let [path (fs/file project "src/evaluator.rs")
      marker "    #[test]\n    fn native_bridge_strings_preserve_every_utf16_code_unit_in_both_directions()"]
  (spit path (str/replace (slurp path) marker (str (slurp (fs/file here "evaluator-tests.rs")) "\n" marker))))
(let [path (fs/file project "tests/compiler/macros.rs")
      marker "#[test]\nfn compiled_source_macros_preserve_form_env_and_caller_owned_syntax()"]
  (spit path (str/replace (slurp path) marker (str (slurp (fs/file here "macro-tests.rs")) "\n" marker))))
(with-open [out (io/output-stream (fs/file here "focused-tests.log"))]
  (p/shell {:dir (str project) :out out :err :out
            :extra-env {"CARGO_TARGET_DIR" "/private/tmp/light-cljs-phase13-storage-profile/target"}}
           "cargo" "test" "macro_" "--" "--nocapture"))
