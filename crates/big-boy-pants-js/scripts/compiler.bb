#!/usr/bin/env bb
(require '[babashka.fs :as fs] '[babashka.process :as p])
(def directory (fs/parent (fs/canonicalize *file*)))
(def root (fs/parent directory))
(def repository (fs/canonicalize (fs/path directory "../../..")))
(def temporary (fs/create-temp-dir {:dir (str directory) :prefix ".goose-compiler-"}))
(def executable (str (fs/path temporary "compiler-session")))
;; --goose-build runs the tests in the Goose build, which links no V8 or
;; QuickJS; tests that compare with V8 are cfg-gated out there.
(def features (if (some #{"--goose-build"} *command-line-args*)
                ["--no-default-features" "--features" "goose-macros"]
                ["--features" "goose-macros"]))
(try
  (p/shell {:dir (str repository)} "goose" "version")
  (p/shell {:dir (str repository)} "goose" "format" "--check" (str (fs/path root "src/compiler_session.goose")))
  (p/shell {:dir (str repository)} "goose" "check" (str (fs/path root "src/compiler_session.goose")))
  (doseq [mode [["--debug"] []]]
    (doseq [source ["readiness.goose" "readiness_tests.goose"]]
      (p/shell {:dir (str repository)} "goose" "format" "--check" (str (fs/path root "src" source))))
    (p/shell {:dir (str repository)} "goose" "check" (str (fs/path root "src/readiness_tests.goose")))
    (apply p/shell {:dir (str repository)} (concat ["goose" "build" (str (fs/path root "src/readiness_tests.goose"))] mode ["-o" executable]))
    (p/shell executable)
    (apply p/shell {:dir (str repository)} (concat ["goose" "build" (str (fs/path root "src/compiler_session.goose"))] mode ["-o" executable]))
    (apply p/shell {:dir (str repository) :env (assoc (into {} (System/getenv)) "GOOSE_COMPILER_RUNNER" executable "CARGO_BUILD_JOBS" "3" "LIGHT_CLJS_V8_CACHE" "require")}
             (concat ["cargo" "test" "--locked"] features ["--lib" "goose_compiler" "--" "--include-ignored" "--nocapture" "--test-threads=1"])))
  (finally (fs/delete-tree temporary)))
