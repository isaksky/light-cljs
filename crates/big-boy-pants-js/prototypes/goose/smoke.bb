#!/usr/bin/env bb
(require '[babashka.fs :as fs]
         '[babashka.process :as process]
         '[clojure.string :as str])

;; Resolve inputs from this script, independently of the caller's working directory.
(let [directory (fs/parent (fs/canonicalize *file*))
      goose (or (System/getenv "GOOSE") "goose")
      driver (str (fs/path directory "tests.goose"))
      temporary (fs/create-temp-dir {:prefix "goose-vm-smoke-"})
      executable (str (fs/path temporary "tiny-vm"))
      run! (fn [& arguments]
             (println "+" (str/join " " arguments))
             (flush)
             (apply process/shell {:dir (str directory)} arguments))]
  (try
    (run! goose "version")
    (doseq [source ["vm.goose" "tests.goose"]]
      (run! goose "format" "--check" (str (fs/path directory source))))
    (run! goose "check" driver)
    (run! goose driver "--debug")
    (run! goose "build" driver "-o" executable)
    (run! executable)
    (println "Goose format/check/debug/native smoke passed")
    (finally
      (fs/delete-tree temporary))))
