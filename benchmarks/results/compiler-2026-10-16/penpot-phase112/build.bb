(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def project (fs/file here "project"))
(def env {"CARGO_TARGET_DIR" (str (fs/file here "target"))
          "LIBCLANG_PATH" "/opt/homebrew/opt/llvm@23/lib"
          "CARGO_PROFILE_RELEASE_LTO" "thin"
          "CARGO_PROFILE_RELEASE_CODEGEN_UNITS" "1"})
(spit (fs/file here "build-environment.edn") (pr-str env))
(with-open [out (io/output-stream (fs/file here "build.log"))]
  (p/shell {:dir (str project) :extra-env env :out out :err :out}
    "cargo" "build" "--release" "--locked" "-j" "20"))
(fs/create-dirs (fs/file here "frozen-engine"))
(doseq [[from to] [["obj/librusty_v8.a" "librusty_v8.a"] ["src_binding.rs" "src_binding.rs"] ["args.gn" "args.gn"]]]
  (fs/copy (fs/file here "target/release/gn_out" from) (fs/file here "frozen-engine" to) {:replace-existing true}))
(fs/copy (fs/file here "target/release/light-cljs") (fs/file here "light-cljs-v8-integrated") {:replace-existing true})
(println "Packaged default backend built from pinned source")
