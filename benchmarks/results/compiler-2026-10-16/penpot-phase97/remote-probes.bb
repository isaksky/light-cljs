(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.edn :as edn]
         '[clojure.string :as str] '[cheshire.core :as json])
(import '[java.security MessageDigest] '[java.time Instant ZoneOffset]
        '[java.time.format DateTimeFormatter] '[java.util Base64])
(def here (fs/parent *file*))
(def root "/Users/admin/src/light-cljs")
(def common (str (fs/real-path (str/trim (:out (p/shell {:dir root :out :string} "git" "rev-parse" "--path-format=absolute" "--git-common-dir"))))))
(defn digest [s] (subs (format "%064x" (BigInteger. 1 (.digest (MessageDigest/getInstance "SHA-256") (.getBytes s "UTF-8")))) 0 12))
(def ids (str "r" (digest common) "/w" (digest root)))
(defn sq [s] (str "'" (str/replace (str s) "'" "'\"'\"'") "'"))
(defn ps [s] (str "powershell -NoLogo -NoProfile -NonInteractive -EncodedCommand "
                  (.encodeToString (Base64/getEncoder) (.getBytes s "UTF-16LE"))))
(def input (fs/file here "probe-input"))
(fs/create-dirs input)
(fs/copy (fs/file here "stack-probe.cc") (fs/file input "stack-probe.cc") {:replace-existing true})
(fs/copy (fs/file here "project/src/v8_stack.cc") (fs/file input "v8_stack.cc") {:replace-existing true})
(fs/copy (fs/file here "project/src/v8_thread_stack.cc") (fs/file input "v8_thread_stack.cc") {:replace-existing true})
(when-not (fs/exists? (fs/file input "include"))
  (p/shell "cp" "-a" (str (fs/file here "project/vendor/v8-headers/include")) (str (fs/file input "include"))))
(doseq [platform (if (seq *command-line-args*) *command-line-args* ["linux" "windows"])]
  (let [windows? (= platform "windows")
        host (if windows? "windows-test-vm" "oma")
        connection (edn/read-string (slurp (fs/file here (str platform "-connection.edn"))))
        ssh ["ssh" "-S" (:socket connection) "-o" "BatchMode=yes" "-o" "StrictHostKeyChecking=yes" "-o" "ConnectTimeout=30" host]
        remote (fn [cmd] (select-keys (apply p/shell {:out :string :err :string :continue true} (conj ssh cmd)) [:exit :out :err :cmd]))
        parent (str (if windows? "C:/WT/" "/home/isak/LT/") ids)
        start (str (Instant/now))
        run-id (str (.format (.withZone (DateTimeFormatter/ofPattern "yyMMddHHmmss") ZoneOffset/UTC) (Instant/now)) "-" (subs (str/replace (str (random-uuid)) "-" "") 0 6))
        run (str parent "/" run-id)
        out (fs/file here (str platform "-results"))
        ownership {:git-common-dir common :worktree-root root :name "light-cljs" :created-at start}
        existing (remote (if windows?
                           (ps (str "if (Test-Path -LiteralPath '" parent "/workspace.json') { Get-Content -Raw -LiteralPath '" parent "/workspace.json'; exit 0 } else { exit 1 }"))
                           (str "cat " (sq (str parent "/workspace.json")))))
        capacity (remote (if windows? (ps "(Get-PSDrive -Name C).Free") "df -h /home/isak && df -i /home/isak"))
        upload (fn [local destination]
                 (p/shell "scp" "-q" "-o" (str "ControlPath=" (:socket connection)) "-o" "BatchMode=yes" "-o" "StrictHostKeyChecking=yes" (str local) (str host ":" destination)))
        record {:run run :start-time start :state "active" :socket (:socket connection)
                :local-results (str out) :purpose "Native V8 stack-boundary shim probe; not a full V8 runtime build"}]
    (println platform "capacity" capacity)
    (spit (fs/file here (str platform "-capacity.edn")) (pr-str capacity))
    (assert (zero? (:exit capacity)) capacity)
    ;; This standalone probe uses under 10 MiB at peak, with no downloads or
    ;; toolchain installation. Keep at least 512 MiB of system headroom.
    (when windows?
      (let [free (parse-long (str/trim (:out capacity)))]
        (when (< free (* 10 1024 1024 1024))
          (println "Windows capacity warning: small probe budget is under 10 MiB; no cleanup needed"))
        (assert (> free (* 512 1024 1024)))))
    (assert (#{0 1} (:exit existing)) existing)
    (when (zero? (:exit existing))
      (assert (= (select-keys ownership [:git-common-dir :worktree-root])
                 (select-keys (json/parse-string (:out existing) true) [:git-common-dir :worktree-root])) existing))
    (let [made (remote (if windows?
                         (ps (str "$ErrorActionPreference='Stop'; if (Test-Path -LiteralPath '" run "') { throw 'Run already exists' }; New-Item -ItemType Directory -Force -Path '" parent "' | Out-Null; New-Item -ItemType Directory -Path '" run "','" run "/i','" run "/w','" run "/o' | Out-Null"))
                         (str "mkdir -p " (sq parent) " && mkdir " (sq run) " && mkdir "
                              (str/join " " (map #(sq (str run "/" %)) ["i" "w" "o"])))))]
      (assert (zero? (:exit made)) made))
    (when-not (zero? (:exit existing))
      (spit (fs/file here "workspace.json") (json/generate-string ownership {:pretty true}))
      (upload (fs/file here "workspace.json") (str parent "/workspace.json")))
    (spit (fs/file here (str platform "-run.json")) (json/generate-string record {:pretty true}))
    (upload (fs/file here (str platform "-run.json")) (str run "/run.json"))
    (when windows?
      (doseq [f (fs/glob input "**" {:recursive true}) :when (fs/regular-file? f)]
        (assert (< (count (str run "/i/" (fs/relativize input f))) 240))))
    (p/shell "scp" "-qr" "-o" (str "ControlPath=" (:socket connection)) "-o" "BatchMode=yes" "-o" "StrictHostKeyChecking=yes"
      (str input "/.") (str host ":" run "/i/"))
    (let [script (if windows? "windows-run.ps1" "linux-run.sh")
          _ (upload (fs/file here script) (str run "/w/" script))
          command (if windows? (ps (str "& '" run "/w/" script "' -Run '" run "'; exit $LASTEXITCODE"))
                      (str "bash " (sq (str run "/w/" script)) " " (sq run)))
          result (remote command)]
      (spit (fs/file here (str platform "-execution.edn")) (pr-str (assoc result :command command)))
      (fs/create-dirs out)
      (p/shell "scp" "-qr" "-o" (str "ControlPath=" (:socket connection)) "-o" "BatchMode=yes" "-o" "StrictHostKeyChecking=yes"
        (str host ":" run "/o/.") (str out))
      (assert (not= 255 (:exit result)) result)
      (let [exit-code (parse-long (str/trim (slurp (fs/file out "exit-code.txt"))))]
        (assert (= exit-code (:exit result)) result)
        (spit (fs/file here (str platform "-run.json"))
          (json/generate-string (assoc record :state "completed" :end-time (str (Instant/now)) :command command :exit exit-code) {:pretty true}))
        (upload (fs/file here (str platform "-run.json")) (str run "/run.json"))
        (println platform (slurp (fs/file out "probe.log")))
        (assert (zero? exit-code) result)))))
