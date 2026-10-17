(require '[babashka.fs :as fs] '[clojure.string :as str])
(def here (fs/parent *file*))
(def dest (fs/file here "project"))
(fs/create-dirs dest)
(doseq [path ["Cargo.toml" "Cargo.lock" "build.rs"]]
  (fs/copy path (fs/file dest path) {:replace-existing true}))
(doseq [path ["src" "vendor" "benches"]]
  (fs/copy-tree path (fs/file dest path) {:replace-existing true}))
(def c-file (fs/file dest "vendor/rquickjs-sys/quickjs/quickjs.c"))
(def c-source (slurp c-file))
(def anchor "static no_inline __exception int __js_poll_interrupts(JSContext *ctx)\n{")
(assert (str/includes? c-source anchor))
(spit c-file
  (str/replace c-source anchor
    (str (slurp (fs/file here "interrupt-profile.c")) "\n" anchor
         "\n    light_profile_sample(ctx);")))
(spit (fs/file here "runtime.js") (slurp "src/js_runtime.js"))
(println dest)
