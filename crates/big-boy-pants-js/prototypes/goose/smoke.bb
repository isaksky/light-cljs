#!/usr/bin/env bb
(require '[babashka.fs :as fs]
         '[babashka.process :as process]
         '[clojure.string :as str])

;; Resolve inputs from this script, independently of the caller's working directory.
(let [directory (fs/parent (fs/canonicalize *file*))
      goose (or (System/getenv "GOOSE") "goose")
      driver (str (fs/path directory "tests.goose"))
      temporary (fs/create-temp-dir {:dir (str directory) :prefix ".goose-smoke-"})
      executable (str (fs/path temporary "tiny-vm"))
      run! (fn [& arguments]
             (println "+" (str/join " " arguments))
             (flush)
             (apply process/shell {:dir (str directory)} arguments))]
  (try
    (run! goose "version")
    (doseq [source ["vm.goose" "tests.goose" "encoding.goose" "program.goose" "scalar.goose" "export_schema.goose" "inspect.goose" "source_tests.goose" "layout_probe.goose"]]
      (run! goose "format" "--check" (str (fs/path directory source))))
    (let [probe (str (fs/path directory "layout_probe.goose"))]
      (run! goose "check" probe)
      (run! goose probe "--debug")
      (run! goose "build" probe "-o" executable)
      (run! executable))
    (run! goose "check" driver)
    (run! goose driver "--debug")
    (run! goose "build" driver "-o" executable)
    (run! executable)
    (let [source-driver (str (fs/path directory "source_tests.goose"))
          inspector (str (fs/path directory "inspect.goose"))
          images (mapv str (sort (fs/glob (fs/path directory "corpus") "**.gsp")))
          inspect-executable (str (fs/path temporary "inspect"))]
      (run! goose "check" source-driver)
      (run! goose source-driver "--debug")
      (run! goose "build" source-driver "-o" executable)
      (run! executable)
      (apply run! goose inspector "--debug" "--" images)
      (run! goose "build" inspector "-o" inspect-executable)
      (apply run! inspect-executable images))
    (println "Goose format/check/debug/native and source/decode smoke passed")
    (finally
      (fs/delete-tree temporary))))
