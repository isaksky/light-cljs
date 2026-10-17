(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(def here (fs/parent *file*))
(def project (fs/file here "project"))
(assert (not (fs/exists? project)))
(p/shell "cp" "-a" "/private/tmp/light-cljs-phase102/project" (str project))
(def patch (fs/file here "cpu-profile-instrumentation.patch"))
(spit patch
  (str/replace
    (slurp "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase60/cpu-profile-instrumentation.patch")
    "    watchdog: Option<std::thread::JoinHandle<()>>,"
    "    stack: stack::StackLimit,"))
(p/shell {:dir (str project)} "patch" "--batch" "-p1" "--forward" "-i" (str patch))
(spit (fs/file here "build-only.bb")
  (str/replace (slurp "/private/tmp/light-cljs-phase102/build-only.bb")
    "light-cljs-v8-lexical-js-vars" "light-cljs-current-v8-profile"))
(fs/copy "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase60/summarize.bb"
  (fs/file here "summarize.bb"))
(spit (fs/file here "line-ticks.bb")
  (str/replace
    (slurp "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase60/line-ticks.bb")
    "/private/tmp/light-cljs-phase58/project" (str project)))
