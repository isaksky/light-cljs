(require '[babashka.process :as p] '[babashka.fs :as fs] '[clojure.string :as s])
(def source-text (slurp "crates/big-boy-pants-js/scripts/expanded-benchmark.bb"))
;; Load only the actual shared watchdog definition, not benchmark execution.
(load-string (subs source-text (s/index-of source-text "(defn await-command") (s/index-of source-text "(defn command")))
(def marker (str (fs/absolutize "target/watchdog-child.pid")))
(fs/delete-if-exists marker)
(def proc (p/process ["sh" "-c" "sleep 60 & echo $! > \"$WATCHDOG_CHILD_PID\"; wait"] {:env (assoc (into {} (System/getenv)) "WATCHDOG_CHILD_PID" marker) :out :string :err :string :continue true}))
(def parent (.toHandle (:proc proc)))
(try
 (loop [n 0] (when-not (fs/exists? marker) (assert (< n 100)) (Thread/sleep 10) (recur (inc n))))
 (let [pid (parse-long (s/trim (slurp marker))) child (java.lang.ProcessHandle/of pid)]
  (assert (.isPresent child))
  (let [timed-out (try (await-command proc 100 [:owned-watchdog-test]) false (catch clojure.lang.ExceptionInfo e (= "Command watchdog expired; stopped owned process tree" (.getMessage e))))]
   (assert timed-out))
  (loop [n 0]
   (when (or (.isAlive parent) (.isAlive (.get child)))
    (assert (< n 100) "Owned process survived watchdog")
    (Thread/sleep 20) (recur (inc n))))
  (println "PASS: timed-out parent and child both terminated"))
 (finally (.destroyForcibly parent) (fs/delete-if-exists marker)))
(let [proc (p/process ["sh" "-c" "printf expected; exit 7"] {:out :string :err :string :continue true}) result (await-command proc 1000 [:expected-nonzero])]
 (assert (= 7 (:exit result))) (assert (= "expected" (:out result)))
 (println "PASS: expected nonzero result preserved for quota diagnosis"))
