(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def project (fs/file here "project"))
(def old-project (fs/file "/private/tmp/light-cljs-phase46/project"))
(def env {"CARGO_TARGET_DIR" "/private/tmp/light-cljs-phase43/v8-probe/target" "LIGHT_V8_MACROS" "1"})
(p/shell "cmp" (str (fs/file here "main-bindings.js"))
  "/private/tmp/light-cljs-phase42/production/main-none.js")
(p/shell "bb" "/private/tmp/light-cljs-phase46/micro.bb")
;; Run the new semantic regression against the unfiltered reader too.
(fs/copy (fs/file old-project "tests/compiler/macros.rs")
  (fs/file here "baseline-macros.rs") {:replace-existing true})
(fs/copy (fs/file project "tests/compiler/macros.rs")
  (fs/file old-project "tests/compiler/macros.rs") {:replace-existing true})
(try
  (with-open [out (io/output-stream (fs/file here "qualification-baseline.log"))]
    (p/shell {:dir (str old-project) :out out :err :out :extra-env env}
      "cargo" "test" "--test" "compiler"
      "macros::reader_qualification_preserves_alias_collisions_refers_and_definition_views" "--" "--exact"))
  (finally
    (fs/copy (fs/file here "baseline-macros.rs")
      (fs/file old-project "tests/compiler/macros.rs") {:replace-existing true})))
(with-open [out (io/output-stream (fs/file here "tests-v8-4threads.log"))]
  (p/shell {:dir (str project) :out out :err :out :extra-env env}
    "cargo" "test" "--" "--test-threads=4"))
(println "V8 full suite passed with four test threads.")
