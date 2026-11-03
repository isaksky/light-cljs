#!/usr/bin/env bb
(require '[babashka.fs :as fs]
         '[babashka.process :as process]
         '[clojure.string :as str])

;; Resolve inputs from this script, independently of the caller's working directory.
(let [goose (or (System/getenv "GOOSE") "goose")]
  (when-not (fs/which goose)
    (binding [*out* *err*] (println (str "Missing Goose compiler: " goose " (set GOOSE or install goose)")))
    (System/exit 1)))

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
    (doseq [source ["vm.goose" "tests.goose" "encoding.goose" "program.goose" "scalar.goose" "export_schema.goose" "inspect.goose" "source_tests.goose" "layout_probe.goose" "source_vm.goose" "execute.goose" "serial_tests.goose" "form_bridge.goose" "bridge_tests.goose" "macro_tests.goose" "macro_run.goose" "snapshot.goose" "snapshot_fixture.goose" "snapshot_tests.goose" "snapshot_workers.goose" "artifact.goose" "artifact_tests.goose" "dependency.goose" "dependency_tests.goose" "scheduler.goose" "scheduler_tests.goose" "effects.goose" "effects_tests.goose"]]
      (run! goose "format" "--check" (str (fs/path directory source))))
    (let [probe (str (fs/path directory "layout_probe.goose"))]
      (run! goose "check" probe)
      (run! goose probe "--debug")
      (run! goose "build" probe "-o" executable)
      (run! executable))
    (doseq [source ["bridge_tests.goose" "macro_tests.goose" "snapshot_tests.goose" "artifact_tests.goose" "dependency_tests.goose"]]
      (let [path (str (fs/path directory source))]
        (run! goose "check" path)
        (run! goose path "--debug")
        (run! goose "build" path "-o" executable)
        (run! executable)))
    ;; Threads require native linking, including the debug isolation run.
    (doseq [source ["snapshot_workers.goose" "scheduler_tests.goose" "effects_tests.goose"]
            :let [workers (str (fs/path directory source))]]
      (run! goose "check" workers)
      (run! goose "build" workers "--debug" "-o" executable)
      (run! executable)
      (run! goose "build" workers "-o" executable)
      (run! executable))
    (run! goose "check" driver)
    (run! goose driver "--debug")
    (run! goose "build" driver "-o" executable)
    (run! executable)
    (let [serial-driver (str (fs/path directory "serial_tests.goose"))
          runner (str (fs/path directory "execute.goose"))]
      (run! goose "check" serial-driver)
      (run! goose serial-driver "--debug")
      (run! goose "build" serial-driver "-o" executable)
      (run! executable)
      (run! goose "check" runner)
      (run! goose "build" runner "-o" executable)
      (doseq [name ["serial/calls" "serial/captures" "serial/mutations" "serial/global-undefined" "serial/global-this" "synthetic/control_flow" "synthetic/unicode" "closure-captured.pants"]]
        (run! executable (str (fs/path directory "corpus" (str name ".gsp")))))
      (doseq [image (sort (fs/glob (fs/path directory "corpus") "*.gsp"))]
        (run! executable (str image)))
      (doseq [name ["serial/tdz" "serial/type-error" "serial/not-callable" "serial/global-missing" "serial/loop"]]
        (let [result (process/shell {:continue true :out :string :err :string}
                                    executable (str (fs/path directory "corpus" (str name ".gsp"))))]
          (assert (= 1 (:exit result)) (str "Expected nonzero bounded diagnostic: " name))
          (assert (and (str/starts-with? (:out result) "exception:") (< (count (:out result)) 256)))))
      (let [bad (fs/read-all-bytes (fs/path directory "corpus/synthetic/arithmetic.gsp"))
            path (str (fs/path temporary "bad.gsp"))]
        (aset-byte bad 4 (byte 1)) (fs/write-bytes path bad)
        (let [result (process/shell {:continue true :out :string :err :string} executable path)]
          (assert (= 1 (:exit result))) (assert (= "load:Error.Version\n" (:out result))))))
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
