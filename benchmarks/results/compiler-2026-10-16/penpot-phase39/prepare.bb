(require '[babashka.fs :as fs] '[clojure.string :as str])
(def here (fs/parent *file*))
(def project (fs/file "/private/tmp/light-cljs-phase21/project"))
(doseq [path ["Cargo.toml" "Cargo.lock" "build.rs"]]
  (fs/copy path (fs/file project path) {:replace-existing true}))
(doseq [path ["src" "vendor" "benches" "tests" "examples"]]
  (fs/copy-tree path (fs/file project path) {:replace-existing true}))
(fs/copy "/private/tmp/light-cljs-phase11/probe.rs" (fs/file project "examples/replay_probe.rs") {:replace-existing true})
(def profile (-> (slurp "/private/tmp/light-cljs-phase21/interrupt-profile.c")
  (str/replace "static unsigned long long light_profile_polls, light_profile_dropped;"
    "static unsigned long long light_profile_polls, light_profile_dropped;\nstatic uint64_t light_profile_previous_ns;")
  (str/replace "    light_profile_polls++;"
    "    struct timespec now;\n    clock_gettime(CLOCK_MONOTONIC, &now);\n    uint64_t ns = (uint64_t)now.tv_sec * 1000000000ull + now.tv_nsec;\n    if (ns - light_profile_previous_ns < 1000000ull) return;\n    light_profile_previous_ns = ns;\n    light_profile_polls++;")))
(spit (fs/file here "time-gated-profile.c") profile)
(def c-text (slurp "vendor/rquickjs-sys/quickjs/quickjs.c"))
(def anchor "static no_inline __exception int __js_poll_interrupts(JSContext *ctx)\n{")
(assert (str/includes? c-text anchor))
(spit (fs/file project "vendor/rquickjs-sys/quickjs/quickjs.c")
  (str/replace c-text anchor (str profile "\n" anchor "\n    light_profile_sample(ctx);")))
(fs/copy "src/js_runtime.js" (fs/file here "runtime.js") {:replace-existing true})
(println "Prepared time-gated interrupt sampling in isolated project")
