(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def project "/private/tmp/light-cljs-phase21/project")
(def target "/private/tmp/light-cljs-phase13-storage-profile/target")
(def c-file (fs/file project "vendor/rquickjs-sys/quickjs/quickjs.c"))
(def c-text (slurp "vendor/rquickjs-sys/quickjs/quickjs.c"))
(def anchor "static no_inline __exception int __js_poll_interrupts(JSContext *ctx)\n{")
(assert (str/includes? c-text anchor))
(spit c-file (str/replace c-text anchor (str (slurp "/private/tmp/light-cljs-phase21/interrupt-profile.c") "\n" anchor "\n    light_profile_sample(ctx);")))
(fs/create-dirs (fs/file project "examples"))
(fs/copy "/private/tmp/light-cljs-phase11/probe.rs" (fs/file project "examples/replay_probe.rs") {:replace-existing true})
(with-open [out (io/output-stream (fs/file here "profile-build.log"))]
  (p/shell {:dir project :extra-env {"CARGO_TARGET_DIR" target} :out out :err :out}
           "cargo" "build" "--release" "--example" "replay_probe"))
(fs/copy (fs/file target "release/examples/replay_probe") (fs/file here "profile-probe") {:replace-existing true})
(with-open [out (io/output-stream (fs/file here "replay-profile.log"))]
  (p/shell {:extra-env {"LIGHT_PROFILE_INTERRUPTS" "1"} :out out :err :out}
           (str (fs/file here "profile-probe")) "src/js_runtime.js" (str (fs/file here "replay.js"))))
(let [log (slurp (fs/file here "replay-profile.log"))
      rows (for [[_ top incl at] (re-seq #"JS_SAMPLE top=(\d+) inclusive=(\d+) ([^\n]+)" log)]
             {:top (parse-long top) :inclusive (parse-long incl) :at at})]
  (spit (fs/file here "replay-profile-summary.txt")
        (str "Interrupt-check samples, not wall-clock percentages.\n\nTop frame\n"
             (str/join "\n" (map pr-str (take 45 (sort-by :top > rows))))
             "\n\nInclusive\n" (str/join "\n" (map pr-str (take 45 (sort-by :inclusive > rows)))) "\n")))
