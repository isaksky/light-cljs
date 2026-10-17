(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def root "/Users/admin/src/light-cljs")
;; Only reuse the already source-built engine; compiler release options now
;; come from Cargo.toml, without benchmark-only profile or runtime overrides.
(def env {"RUSTY_V8_ARCHIVE" (str (fs/file here "frozen-engine/librusty_v8.a"))
          "RUSTY_V8_SRC_BINDING_PATH" (str (fs/file here "frozen-engine/src_binding.rs"))})
(spit (fs/file here "production-build-environment.edn") (pr-str env))
(with-open [out (io/output-stream (fs/file here "production-build.log"))]
  (p/shell {:dir root :extra-env env :out out :err :out} "cargo" "build" "--release" "--locked"))
(println "Production binary rebuilt with ordinary release defaults")
