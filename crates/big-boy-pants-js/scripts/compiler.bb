#!/usr/bin/env bb
(require '[babashka.fs :as fs] '[babashka.process :as p])
(def directory (fs/parent (fs/canonicalize *file*)))
(def root (fs/parent directory))
(def repository (fs/canonicalize (fs/path directory "../../..")))
(def temporary (fs/create-temp-dir {:dir (str directory) :prefix ".goose-compiler-"}))
(def executable (str (fs/path temporary "compiler-session")))
;; The run-time compiler a session loads on its first eval (light-cljs-856;
;; src/runtime_compiler.h), from the Cargo target directory the tests use.
(def frontend-library
  (str (fs/path (or (System/getenv "CARGO_TARGET_DIR") (str (fs/path repository "target"))) "release/examples"
                (if (re-find #"(?i)mac" (System/getProperty "os.name")) "libgoose_frontend.dylib" "libgoose_frontend.so"))))
;; --goose-build runs the tests in the Goose build, which links no V8 or
;; QuickJS; tests that compare with V8 are cfg-gated out there.
(def features (if (some #{"--goose-build"} *command-line-args*)
                ["--no-default-features" "--features" "goose-macros"]
                ["--features" "goose-macros"]))
(try
  (p/shell {:dir (str repository)} "goose" "version")
  (p/shell {:dir (str repository) :extra-env {"CARGO_BUILD_JOBS" "3"}} "cargo" "build" "--locked" "--release" "-p" "big-boy-pants-js" "--example" "goose_frontend")
  (p/shell {:dir (str repository)} "goose" "format" "--check" (str (fs/path root "src/compiler_session.goose")))
  (p/shell {:dir (str repository)} "goose" "check" (str (fs/path root "src/compiler_session.goose")))
  (doseq [mode [["--debug"] []]]
    (doseq [source ["readiness.goose" "readiness_tests.goose"]]
      (p/shell {:dir (str repository)} "goose" "format" "--check" (str (fs/path root "src" source))))
    (p/shell {:dir (str repository)} "goose" "check" (str (fs/path root "src/readiness_tests.goose")))
    (apply p/shell {:dir (str repository)} (concat ["goose" "build" (str (fs/path root "src/readiness_tests.goose"))] mode ["-o" executable]))
    (p/shell executable)
    (apply p/shell {:dir (str repository)} (concat ["goose" "build" (str (fs/path root "src/compiler_session.goose"))] mode ["-o" executable]))
    (apply p/shell {:dir (str repository) :env (assoc (into {} (System/getenv)) "GOOSE_COMPILER_RUNNER" executable "BBP_FRONTEND_LIBRARY" frontend-library "CARGO_BUILD_JOBS" "3" "LIGHT_CLJS_V8_CACHE" "require" "BBP_RESET_CHECK" "1")}
             (concat ["cargo" "test" "--locked"] features ["--lib" "goose_compiler" "--" "--include-ignored" "--nocapture" "--test-threads=1"])))
  (finally (fs/delete-tree temporary)))
