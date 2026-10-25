#!/usr/bin/env bb
;; Run one bounded compiler attempt. The command is persisted before launch.
(require '[babashka.fs :as fs]
         '[babashka.process :as process]
         '[clojure.java.io :as io])

(let [[label & command] *command-line-args*
      output (fs/parent (fs/absolutize *file*))
      timeout-ms 120000
      record {:label label :command (vec command) :cwd (str (fs/cwd))
              :started-at (str (java.time.Instant/now)) :timeout-ms timeout-ms}
      prefix (str (fs/path output label))]
  (assert (and label (seq command)) "Usage: attempt.bb LABEL COMMAND ARG...")
  (assert (not (fs/exists? (str prefix ".command.edn"))) "Use a fresh label")
  (spit (str prefix ".command.edn") (str (pr-str record) "\n"))
  (with-open [out (io/output-stream (str prefix ".stdout"))
              err (io/output-stream (str prefix ".stderr"))]
    (let [started (System/nanoTime)
          p (process/process (into ["/usr/bin/time" "-l"] command) {:out out :err err})
          waited (deref p timeout-ms ::timeout)
          timed-out? (= waited ::timeout)
          result (if timed-out? (do (process/destroy-tree p) @p) waited)
          outcome (assoc record :exit (:exit result) :timed-out timed-out?
                         :elapsed-seconds (/ (- (System/nanoTime) started) 1e9))]
      (spit (str prefix ".result.edn") (str (pr-str outcome) "\n"))
      (prn (select-keys outcome [:label :exit :timed-out :elapsed-seconds])))))
