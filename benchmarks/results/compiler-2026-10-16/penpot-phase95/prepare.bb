(require '[babashka.fs :as fs] '[clojure.string :as str])
(def here (fs/parent *file*))
(spit (fs/file here "run.bb")
  (-> (slurp "/private/tmp/light-cljs-phase87/run.bb")
      (str/replace "(def project (fs/file here \"project\"))"
                   "(def project (fs/file here \"project\"))\n(p/shell \"touch\" (str (fs/file project \"vendor/rquickjs-sys/build.rs\")) (str (fs/file project \"vendor/rquickjs-sys/quickjs/quickjs.c\")))")
      (str/replace "light-cljs-quickjs-shared-storage" "light-cljs-quickjs-native-stores")
      (str/replace "QuickJS shared storage" "QuickJS native stores")))
