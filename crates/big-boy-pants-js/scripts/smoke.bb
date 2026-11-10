#!/usr/bin/env bb
(require '[babashka.fs :as fs]
         '[babashka.process :as process]
         '[clojure.edn]
         '[clojure.string :as str])

;; Resolve inputs from this script, independently of the caller's working directory.
(let [goose (or (System/getenv "GOOSE") "goose")]
  (when-not (fs/which goose)
    (binding [*out* *err*] (println (str "Missing Goose compiler: " goose " (set GOOSE or install goose)")))
    (System/exit 1)))

(let [directory (fs/parent (fs/canonicalize *file*))
      root (fs/parent directory)
      goose (or (System/getenv "GOOSE") "goose")
      driver (str (fs/path root "src/tests.goose"))
      temporary (fs/create-temp-dir {:dir (str directory) :prefix ".goose-smoke-"})
      executable (str (fs/path temporary "tiny-vm"))
      ;; The run-time compiler the VM programs load on their first eval
      ;; (light-cljs-856; src/runtime_compiler.h).
      repository (fs/parent (fs/parent root))
      frontend-library (str (fs/path (or (System/getenv "CARGO_TARGET_DIR") (str (fs/path repository "target"))) "release/examples"
                                     (if (re-find #"(?i)mac" (System/getProperty "os.name")) "libgoose_frontend.dylib" "libgoose_frontend.so")))
      run! (fn [& arguments]
             (println "+" (str/join " " arguments))
             (flush)
             (apply process/shell {:dir (str directory) :extra-env {"BBP_FRONTEND_LIBRARY" frontend-library}} arguments))]
  (try
    (run! goose "version")
    (process/shell {:dir (str repository) :extra-env {"CARGO_BUILD_JOBS" "3"}} "cargo" "build" "--locked" "--release" "-p" "big-boy-pants-js" "--example" "goose_frontend")
    (doseq [source ["vm.goose" "tests.goose" "encoding.goose" "program.goose" "scalar.goose" "export_schema.goose" "inspect.goose" "source_tests.goose" "layout_probe.goose" "source_vm.goose" "realm.goose" "printer.goose" "form_ops.goose" "execute.goose" "serial_tests.goose" "form_bridge.goose" "bridge_tests.goose" "macro_tests.goose" "macro_run.goose" "snapshot.goose" "callable.goose" "snapshot_fixture.goose" "snapshot_tests.goose" "snapshot_workers.goose" "artifact.goose" "artifact_tests.goose" "dependency.goose" "dependency_tests.goose" "scheduler.goose" "scheduler_tests.goose" "effects.goose" "effects_tests.goose" "census.goose" "census_tests.goose" "heap_program.goose" "compiler_session.goose" "conformance.goose" "cells.goose" "cell_owner.goose" "continuations.goose" "runtime_compile.goose"]]
      (run! goose "format" "--check" (str (fs/path root "src" source))))
    (let [probe (str (fs/path root "src/layout_probe.goose"))]
      (run! goose "check" probe)
      (run! goose probe "--debug")
      (run! goose "build" probe "-o" executable)
      (run! executable))
    (doseq [source ["dependency_tests.goose" "census_tests.goose"]]
      (let [path (str (fs/path root "src" source))]
        (run! goose "check" path)
        (run! goose path "--debug")
        (run! goose "build" path "-o" executable)
        (run! executable)))
    ;; Threads require native linking, including the debug isolation run. Each
    ;; of these runs every VM heap in a thread program of its own (light-cljs-794).
    (doseq [source ["snapshot_workers.goose" "bridge_tests.goose" "macro_tests.goose" "snapshot_tests.goose" "artifact_tests.goose" "scheduler_tests.goose" "effects_tests.goose" "serial_tests.goose"]
            :let [workers (str (fs/path root "src" source))]]
      (run! goose "check" workers)
      (run! goose "build" workers "--debug" "-o" executable)
      ;; serial_tests' debug build samples its long budget-cut and limit-point
      ;; sweeps; its native build runs every one (light-cljs-927).
      (apply run! executable (when (= source "serial_tests.goose") ["--sampled"]))
      (run! goose "build" workers "-o" executable)
      (run! executable))
    (run! goose "check" driver)
    (run! goose driver "--debug")
    (run! goose "build" driver "-o" executable)
    (run! executable)
    (let [runner (str (fs/path root "src/execute.goose"))]
      (run! goose "check" runner)
      (run! goose "check" (str (fs/path root "src/conformance.goose")))
      (run! goose "build" runner "-o" executable)
      (doseq [name ["serial/calls" "serial/captures" "serial/mutations" "serial/global-undefined" "serial/global-this" "synthetic/control_flow" "synthetic/unicode" "closure-captured.pants"]]
        (run! executable (str (fs/path root "corpus" (str name ".gsp")))))
      (doseq [image (sort (fs/glob (fs/path root "corpus") "*.gsp"))]
        (run! executable (str image)))
      (doseq [name ["serial/tdz" "serial/type-error" "serial/not-callable" "serial/global-missing" "serial/loop"]]
        (let [result (process/shell {:continue true :out :string :err :string}
                                    executable (str (fs/path root "corpus" (str name ".gsp"))))]
          (assert (= 1 (:exit result)) (str "Expected nonzero bounded diagnostic: " name))
          (assert (and (str/starts-with? (:out result) "exception:") (< (count (:out result)) 256)))))
      (let [bad (fs/read-all-bytes (fs/path root "corpus/synthetic/arithmetic.gsp"))
            path (str (fs/path temporary "bad.gsp"))]
        (aset-byte bad 4 (byte 1)) (fs/write-bytes path bad)
        (let [result (process/shell {:continue true :out :string :err :string} executable path)]
          (assert (= 1 (:exit result))) (assert (= "load:Error.Version\n" (:out result))))
        ;; The conformance runner's outcome contract (scripts/conformance.bb):
        ;; exactly one EDN map, exit 0 for classified outcomes, 1 for
        ;; infrastructure failures. A classified outcome carries :steps, the
        ;; instructions the job executed (the macro ledger's core-boot step
        ;; measures progress by it, light-cljs-917), the faulting instruction
        ;; counted: type-error.js throws at its fourth instruction and
        ;; global-missing.js at its first. These images are checked in, so
        ;; the counts change only with the VM's counting.
        (let [conformance (str (fs/path temporary "conformance"))]
          (run! goose "build" (str (fs/path root "src/conformance.goose")) "-o" conformance)
          ;; A runaway case: the runner gives a case no steps or work budget
          ;; (light-cljs-840), so loop.gsp's `while (true) {}` runs until the
          ;; harness's per-engine timeout kills it. It must still be running,
          ;; with nothing printed, at a short deadline (the VM's default
          ;; budget ends it in milliseconds); then it is killed as
          ;; conformance.bb kills a timed-out case. Smoke itself ending
          ;; during the wait kills it too.
          (let [runaway (process/process {:out :string :err :string :shutdown process/destroy-tree} conformance (str (fs/path root "corpus/serial/loop.gsp")))
                ended (deref runaway 2000 nil)]
            (when-not ended (process/destroy-tree runaway) (deref runaway 5000 nil))
            (assert (nil? ended) (str "conformance runner ended corpus/serial/loop.gsp instead of running until killed: " (:out ended))))
          (doseq [[image exit expected]
                  [["corpus/serial/calls.gsp" 0 {:status :returned :steps 52}]
                   ["corpus/serial/type-error.gsp" 0 {:status :thrown :value "error:TypeError" :steps 4}]
                   ["corpus/serial/global-missing.gsp" 0 {:status :thrown :value "error:ReferenceError" :steps 1}]
                   ["corpus/serial/unsupported-reached.gsp" 0 {:status :unsupported :fault "Fault.Unsupported" :operation "using_check" :steps 66}]
                   [path 1 {:status :infrastructure :reason "load:Error.Version"}]
                   ["corpus/missing.gsp" 1 {:status :infrastructure :reason "cannot read program"}]]]
            (let [result (process/shell {:continue true :out :string :err :string} conformance (if (fs/absolute? image) image (str (fs/path root image))))
                  outcome (clojure.edn/read-string (:out result))]
              (assert (= exit (:exit result)) (str "conformance runner exit for " image ": " (:exit result)))
              (assert (= 1 (count (str/split-lines (:out result)))) (str "one outcome line for " image))
              (assert (= expected (select-keys outcome (keys expected))) (str "conformance outcome for " image ": " (:out result))))))))
    (let [source-driver (str (fs/path root "src/source_tests.goose"))
          inspector (str (fs/path root "src/inspect.goose"))
          images (mapv str (sort (fs/glob (fs/path root "corpus") "**.gsp")))
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
