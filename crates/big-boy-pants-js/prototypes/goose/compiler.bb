#!/usr/bin/env bb
(require '[babashka.fs :as fs] '[babashka.process :as p])
(def directory (fs/parent (fs/canonicalize *file*)))
(def repository (fs/canonicalize (fs/path directory "../../../..")))
(def temporary (fs/create-temp-dir {:dir (str directory) :prefix ".goose-compiler-"}))
(def executable (str (fs/path temporary "compiler-session")))
(try
  (p/shell {:dir (str repository)} "goose" "version")
  (p/shell {:dir (str repository)} "goose" "format" "--check" (str (fs/path directory "compiler_session.goose")))
  (p/shell {:dir (str repository)} "goose" "check" (str (fs/path directory "compiler_session.goose")))
  (doseq [mode [["--debug"] []]]
    (apply p/shell {:dir (str repository)} (concat ["goose" "build" (str (fs/path directory "compiler_session.goose"))] mode ["-o" executable]))
    (p/shell {:dir (str repository) :env (assoc (into {} (System/getenv)) "GOOSE_COMPILER_RUNNER" executable "CARGO_BUILD_JOBS" "3" "LIGHT_CLJS_V8_CACHE" "require")}
             "cargo" "test" "--locked" "--features" "goose-macros" "--lib" "goose_compiler" "--" "--include-ignored" "--nocapture" "--test-threads=1"))
  (finally (fs/delete-tree temporary)))
