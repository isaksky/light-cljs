(require '[babashka.fs :as fs])
(def here (fs/parent *file*))
(doseq [file ["main-none.js" "main-none.time" "main-none.command.edn" "build.log" "light-cljs-unused-form"]]
  (fs/move (fs/file here file) (fs/file here (str "stale-generated-runtime-" file))))
