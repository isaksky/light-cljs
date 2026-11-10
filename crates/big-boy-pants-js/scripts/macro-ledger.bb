#!/usr/bin/env bb
;; Macro-idiom ledger (light-cljs-916): scores the Goose compiler path on
;; ordinary macro idioms against the V8 path, beside conformance.bb, which
;; scores JavaScript against the Pants engine and QuickJS.
;;
;;   bb crates/big-boy-pants-js/scripts/macro-ledger.bb [options]
;;     --idiom ID         only this idiom (repeatable)
;;     --jobs N           idioms built in parallel (default 4)
;;     --timeout S        seconds per build, counted as a failure (default 120)
;;     --check            exit 1 on a regression against macro-ledger/
;;                        expected-pass.edn, on a pass it does not list, or
;;                        when V8's refusals differ from idioms.edn's
;;     --write-expected   rewrite expected-pass.edn with today's passes
;;                        (every idiom only)
;;     --light PATH       light-cljs built with --features goose-macros
;;                        (default <target>/release/light-cljs, built here)
;;     --runner PATH      the Goose compiler runner (default
;;                        <target>/macro-ledger/runner, built here from
;;                        src/compiler_session.goose when a Goose source is
;;                        newer)
;;     --no-build         use --light and --runner as they are
;;
;; Each idiom in macro-ledger/idioms.edn becomes one tiny project in
;; <target>/macro-ledger/work/ID: a macro namespace probe.m whose macro `p`
;; evaluates the idiom's :body with x bound to the argument form
;;   [1 2 3 {:a 1 :b "s"} foo/bar :k "str" (f 4 5)]
;; and returns (list 'quote result), and a main namespace probe.main that
;; prints the expansion. The project is built with
;;   light-cljs release bench --optimize advanced --jobs 1 --no-cache
;; once on the V8 path and once on the Goose path (LIGHT_CLJS_GOOSE_RUNNER,
;; LIGHT_CLJS_GOOSE_WORKERS=1), and node runs each output that differs.
;;
;; Outcomes. :pass, the Goose output is byte-identical to V8's; :differs,
;; Goose built other output (both printed expansions are kept); :fail, Goose
;; refused, with its reason. An idiom V8 refuses is :v8-refuses and not
;; scored; idioms.edn records why under :v8-refuses. The reason of a :fail is
;; the Goose fault with what it lacks (`Fault.NativeBoundary:no-core-leaf-take`,
;; the spellings of boundary_detail in src/source_vm.goose) or the error's
;; first clause; failures print grouped by reason, so a pellet sees which
;; idioms the step it adds unlocks. The ledger is written to
;; <target>/macro-ledger/ledger.edn.
;;
;; Idiom keys: :id; :body, ClojureScript source over x; :requires, the
;; require specs of probe.m; :defs, top-level forms of probe.m before p;
;; :files, more source files by project path; :main, top-level forms of
;; probe.main before its run function; :v8-refuses, why V8 refuses it.
;; Every probe.m also defines the helpers below (helper, helper-walk,
;; helper-table, registry).
(require '[babashka.fs :as fs] '[babashka.process :as p]
         '[clojure.edn :as edn] '[clojure.string :as str] '[clojure.pprint :as pprint])
(import '[java.util.concurrent Executors TimeUnit])

(def directory (fs/parent (fs/canonicalize *file*)))
(def root (fs/parent directory))
(def repository (fs/canonicalize (fs/path root "../..")))
(def ledger-dir (fs/path root "macro-ledger"))
;; Absolute, a relative CARGO_TARGET_DIR read from the repository, where
;; this script runs Cargo: idiom projects and their builds run elsewhere.
(def target (let [t (fs/path (or (System/getenv "CARGO_TARGET_DIR") "target"))] (if (fs/absolute? t) t (fs/path repository t))))
(def out-dir (fs/path target "macro-ledger"))
(def goose (or (System/getenv "GOOSE") "goose"))

(defn fail! [& message]
  (binding [*out* *err*] (println (apply str "macro-ledger: " message)))
  (System/exit 2))

(def options
  (loop [args *command-line-args* o {:jobs 4 :timeout 120 :idioms #{}}]
    (if-let [[a & more] (seq args)]
      (case a
        "--idiom" (recur (rest more) (update o :idioms conj (first more)))
        "--jobs" (recur (rest more) (assoc o :jobs (some-> (first more) parse-long)))
        "--timeout" (recur (rest more) (assoc o :timeout (some-> (first more) parse-long)))
        "--light" (recur (rest more) (assoc o :light (first more)))
        "--runner" (recur (rest more) (assoc o :runner (first more)))
        "--check" (recur more (assoc o :check true))
        "--write-expected" (recur more (assoc o :write-expected true))
        "--no-build" (recur more (assoc o :no-build true))
        (fail! "unknown argument " a))
      o)))
(when-not (and (pos-int? (:jobs options)) (pos-int? (:timeout options))) (fail! "--jobs and --timeout take positive integers"))

;; ---------------------------------------------------------------- idioms
(def idiom-keys #{:id :body :requires :defs :files :main :v8-refuses})
(def all-idioms
  (let [idioms (edn/read-string (slurp (str (fs/path ledger-dir "idioms.edn"))))]
    (when-not (vector? idioms) (fail! "idioms.edn must be a vector of idiom maps"))
    (doseq [i idioms]
      (when-not (and (map? i) (string? (:id i)) (re-matches #"[a-z0-9][a-z0-9-]*" (:id i)) (string? (:body i)))
        (fail! "idiom without a lower-case, dashed :id and a string :body: " (pr-str i)))
      (when-let [extra (seq (remove idiom-keys (keys i)))] (fail! "idiom " (:id i) " has unknown keys " (pr-str extra))))
    (when-let [twice (seq (for [[id n] (frequencies (map :id idioms)) :when (> n 1)] id))] (fail! "idioms listed twice: " (str/join ", " twice)))
    idioms))
(def idioms
  (let [known (set (map :id all-idioms))]
    (when-let [missing (seq (remove known (:idioms options)))] (fail! "unknown idiom " (str/join ", " missing)))
    (when (and (:write-expected options) (seq (:idioms options))) (fail! "--write-expected needs every idiom"))
    (filterv #(or (empty? (:idioms options)) ((:idioms options) (:id %))) all-idioms)))
(def expected-file (str (fs/path ledger-dir "expected-pass.edn")))
(def expected
  (let [listed (edn/read-string (slurp expected-file)) known (set (map :id all-idioms))]
    (when-not (and (vector? listed) (every? string? listed)) (fail! "expected-pass.edn must be a vector of idiom IDs"))
    (when-let [bad (seq (remove known listed))] (fail! "expected-pass.edn names unknown idioms: " (str/join ", " bad)))
    (when-let [bad (seq (filter (set listed) (map :id (filter :v8-refuses all-idioms))))]
      (fail! "expected-pass.edn names idioms V8 refuses: " (str/join ", " bad)))
    (set listed)))

;; ---------------------------------------------------------------- projects
(def argument-form "[1 2 3 {:a 1 :b \"s\"} foo/bar :k \"str\" (f 4 5)]")
(def helpers
  (str "(defn helper [v] (if (number? v) (* v 2) v))\n"
       "(defn helper-walk [form] (cond (vector? form) (mapv helper-walk form) (map? form) (into {} (map (fn [[k v]] [k (helper-walk v)])) form) (seq? form) (map helper-walk form) :else (helper form)))\n"
       "(def helper-table {:a 1 :b [2 3]})\n"
       "(defonce registry (atom []))\n"))

(defn project! [dir {:keys [body requires defs files main]}]
  (fs/delete-tree dir)
  (fs/create-dirs (fs/path dir "src" "probe"))
  (fs/create-dirs (fs/path dir "macros" "probe"))
  (spit (str (fs/path dir "light-cljs.edn"))
        (pr-str {:source-paths ["src" "macros"]
                 :builds {:bench {:target :esm :output-dir "out" :modules {:main {:entries ['probe.main]}}
                                  :build-options {:cache-level :off}}}}))
  (spit (str (fs/path dir "macros" "probe" "m.cljc"))
        (str "(ns probe.m" (when (seq requires) (str " (:require " (str/join " " (map pr-str requires)) ")")) ")\n"
             helpers
             (some-> defs (str "\n"))
             "(defmacro p [x] (let [r " body "] (list 'quote r)))\n"))
  (doseq [[path text] files]
    (fs/create-dirs (fs/parent (fs/path dir path)))
    (spit (str (fs/path dir path)) text))
  (spit (str (fs/path dir "src" "probe" "main.cljs"))
        (str "(ns probe.main (:require [probe.m :as m]))\n"
             (some-> main (str "\n"))
             "(defn ^:export run [a] (let [loc a] (m/p " argument-form ")))\n"
             "(js/console.log (pr-str (run 1)))\n")))

(defn run-process [{:keys [dir env timeout]} & command]
  (let [proc (apply p/process {:dir (str dir) :env env :out :string :err :string :shutdown p/destroy-tree} command)
        result (deref proc (* 1000 timeout) nil)]
    (or result
        (do (p/destroy-tree proc) (deref proc 5000 nil)
            {:exit :timeout :out "" :err (str "timed out after " timeout " s")}))))

(def base-env (apply dissoc (into {} (System/getenv)) ["LIGHT_CLJS_GOOSE_RUNNER" "LIGHT_CLJS_GOOSE_WORKERS" "LIGHT_CLJS_GOOSE_REPORT"]))

;; The compiler's message without the build's preamble and the call site the
;; macro was expanded at, which every idiom shares.
(defn clean-error [dir text]
  (-> (str text)
      (str/replace (str dir "/") "")
      (str/replace #"(?s)^.*?\[:bench\] Compiling \.\.\.\n" "")
      (str/replace #"^light-cljs release bench: \S+: macro expansion: while expanding \S+: \S+: " "")
      str/trim))

(defn build [light dir env]
  (fs/delete-tree (fs/path dir "out"))
  (let [r (run-process {:dir dir :env env :timeout (:timeout options)} light "release" "bench" "--optimize" "advanced" "--jobs" "1" "--no-cache")
        out (fs/path dir "out" "probe.main.js")]
    (if (and (= 0 (:exit r)) (fs/exists? out))
      {:js (slurp (str out))}
      {:error (let [e (clean-error dir (:err r))] (if (str/blank? e) (str "exit " (:exit r) " with no output") e))})))

(defn node [dir file js]
  (let [f (str (fs/absolutize (fs/path dir file)))]
    (spit f js)
    (let [r (run-process {:dir dir :env base-env :timeout 30} "node" f)]
      (if (= 0 (:exit r)) (str/trim (:out r)) (str "node failed: " (last (str/split-lines (str (:err r)))))))))

(defn bounded [s n] (if (> (count s) n) (str (subs s 0 n) "...") s))

;; What a refusal names: the Goose fault with what it lacks, another failed
;; job's description, or else the message's first clause, without the macro
;; and definition every idiom shares.
(defn reason [error]
  (let [clause (-> (first (str/split-lines error))
                   (str/replace #"; definition .*$" "")
                   (str/replace #"^macro \S+: " ""))]
    (or (re-find #"Fault\.[A-Za-z]+(?::[^\s;]+)?" clause)
        (some-> (re-find #"^Goose: ERROR \d+ (.*?) function=" clause) second)
        (bounded clause 160))))

;; The kind of a reason: its fault and the kind of what it lacks, from
;; boundary_detail's spellings (src/source_vm.goose) and the pending
;; property of an UnsupportedOperand.
(def detail-kinds ["no-core-leaf" "no-rt-member" "no-global" "no-image-member" "write-to-an-image-row"
                   "form-operation" "at" "in-native-frame" "pending"])
(defn kind [reason]
  (if-let [[_ fault detail] (re-matches #"(Fault\.[A-Za-z]+)(?::(.*))?" reason)]
    (cond (nil? detail) fault
          :else (if-let [k (first (filter #(or (= detail %) (str/starts-with? detail (str % "-"))) detail-kinds))]
                  (str fault ":" k)
                  (str fault ": a built-in's step")))
    "other refusals"))

;; An idiom's outcome from its V8 and Goose builds, each {:js output} or
;; {:error message}; g is nil when V8 refused and Goose was not built. Only
;; the output's bytes count: a differing output is no pass even when it
;; prints the same expansion.
(defn score [v8 g]
  (cond (not (:js v8)) {:result :v8-refuses :v8-error (bounded (:error v8) 600)}
        (= (:js v8) (:js g)) {:result :pass}
        (:js g) {:result :differs :reason "output differs"}
        :else {:result :fail :reason (reason (:error g)) :goose-error (bounded (:error g) 700)}))

;; What --check reports over the selected idioms' rows.
(defn check-outcome [selected-idioms rows expected]
  (let [selected (set (map :id selected-idioms))
        rows (filter (comp selected :id) rows)
        expected (set (filter selected expected))
        passes (set (map :id (filter #(= :pass (:result %)) rows)))
        recorded (set (map :id (filter :v8-refuses selected-idioms)))
        refused (set (map :id (filter #(= :v8-refuses (:result %)) rows)))]
    {:expected expected
     :regressions (sort (remove passes expected))
     :new-passes (sort (remove expected passes))
     :refusal-changes (sort (concat (remove recorded refused) (remove refused recorded)))}))

;; The scoring and check rules, checked on every run.
(let [v8 {:js "out"} other {:js "out2"}
      native "macro probe.m/p: Goose: ERROR 1 Fault.NativeBoundary:no-core-leaf-take function=6 pc=21 emitted-span=298:299 writes=0; definition macros/probe/m.cljc:6:1 (bytes 79..131)"
      preparation "macro probe.m/p: Goose: ERROR 1 preparation-symbol-unshareable a symbol made by a job (not shareable) function=-1 pc=-1 emitted-span=-1:-1; definition macros/probe/m.cljc:6:1 (bytes 79..131)"
      compiler "macro probe.m/p: Goose: probe.m/render is a declaration-bundle product; definition macros/probe/m.cljc:9:1 (bytes 79..131)"]
  (assert (= {:result :pass} (score v8 v8)))
  (assert (= {:result :differs :reason "output differs"} (score v8 other)))
  (assert (= :fail (:result (score v8 {:error native}))))
  (assert (= native (:goose-error (score v8 {:error native}))))
  (assert (= :v8-refuses (:result (score {:error "x"} nil))))
  (assert (= "Fault.NativeBoundary:no-core-leaf-take" (reason native)))
  (assert (= "preparation-symbol-unshareable a symbol made by a job (not shareable)" (reason preparation)))
  (assert (= "Goose: probe.m/render is a declaration-bundle product" (reason compiler)))
  (assert (= "Fault.NativeBoundary:no-core-leaf" (kind "Fault.NativeBoundary:no-core-leaf-take")))
  (assert (= "Fault.NativeBoundary: a built-in's step" (kind "Fault.NativeBoundary:mapSeq:-no-core-leaf-map-over-a-vector")))
  (assert (= "Fault.UnsupportedOperand:pending" (kind "Fault.UnsupportedOperand:pending-indexOf")))
  (assert (= "Fault.DependencyNeed" (kind "Fault.DependencyNeed")))
  (assert (= "other refusals" (kind (reason compiler))))
  (let [idioms [{:id "a"} {:id "b"} {:id "c"} {:id "d" :v8-refuses "r"} {:id "e"}]
        rows [{:id "a" :result :pass} {:id "b" :result :differs} {:id "c" :result :pass}
              {:id "d" :result :pass} {:id "e" :result :v8-refuses}]
        outcome (check-outcome idioms rows #{"a" "b" "z"})]
    (assert (= #{"a" "b"} (:expected outcome)))
    (assert (= ["b"] (:regressions outcome)))
    (assert (= ["c" "d"] (:new-passes outcome)))
    (assert (= ["d" "e"] (:refusal-changes outcome)))
    (assert (= {:expected #{"a" "b"} :regressions ["b"] :new-passes [] :refusal-changes []}
               (check-outcome (take 2 idioms) rows #{"a" "b" "c"})))))

(defn run-idiom [light runner {:keys [id v8-refuses] :as idiom}]
  (let [dir (str (fs/path out-dir "work" id))
        _ (project! dir idiom)
        v8 (build light dir base-env)
        g (when (:js v8) (build light dir (assoc base-env "LIGHT_CLJS_GOOSE_RUNNER" runner "LIGHT_CLJS_GOOSE_WORKERS" "1")))
        row (merge (cond-> {:id id} v8-refuses (assoc :recorded v8-refuses)) (score v8 g))]
    (cond-> row
      (= :differs (:result row)) (assoc :v8-prints (node dir "v8.mjs" (:js v8)) :goose-prints (node dir "goose.mjs" (:js g))))))

;; ---------------------------------------------------------------- tools
(defn shell! [& command] (println "+" (str/join " " command)) (flush) (apply p/shell {:dir (str repository)} command))
(def light (str (fs/absolutize (or (:light options) (fs/path target "release" "light-cljs")))))
(def runner (str (fs/absolutize (or (:runner options) (fs/path out-dir "runner")))))
(fs/create-dirs out-dir)
(when-not (:no-build options)
  (when-not (:light options)
    (shell! "cargo" "build" "--release" "--locked" "--features" "goose-macros" "--bin" "light-cljs"))
  (when-not (:runner options)
    ;; The runner is rebuilt when a Goose source, C header or build sidecar
    ;; beside compiler_session.goose is newer than it.
    (let [sources (fs/glob (fs/path root "src") "*.{goose,h,build}")
          newest (apply max (map #(.toMillis (fs/last-modified-time %)) sources))]
      (when (or (not (fs/exists? runner)) (< (.toMillis (fs/last-modified-time runner)) newest))
        (shell! goose "build" (str (fs/path root "src" "compiler_session.goose")) "-o" runner)))))
(doseq [tool [light runner]] (when-not (fs/exists? tool) (fail! "missing tool " tool)))

;; ---------------------------------------------------------------- run
(def rows
  (let [pool (Executors/newFixedThreadPool (:jobs options))
        started (System/nanoTime)
        futures (mapv (fn [i] (.submit pool ^Callable (fn [] (run-idiom light runner i)))) idioms)
        results (mapv #(.get %) futures)]
    (.shutdown pool) (.awaitTermination pool 1 TimeUnit/MINUTES)
    (println (format "built %d idioms on the V8 and Goose paths in %.1f s with %d jobs"
                     (count idioms) (/ (- (System/nanoTime) started) 1e9) (:jobs options)))
    results))

(spit (str (fs/path out-dir "ledger.edn"))
      (with-out-str (println ";; Generated by crates/big-boy-pants-js/scripts/macro-ledger.bb; one row per idiom.")
        (pprint/pprint rows)))

(def counts (frequencies (map :result rows)))
(def scored (remove #(= :v8-refuses (:result %)) rows))
(when (pos? (get counts :fail 0))
  (println "\nGoose refusals by kind and reason (the idioms each reason stops):"))
(doseq [[k failures] (sort-by (fn [[k group]] [(- (count group)) k]) (group-by (comp kind :reason) (filter #(= :fail (:result %)) rows)))]
  (println (str "\n" k ": " (count failures) (if (= 1 (count failures)) " idiom" " idioms")))
  (doseq [[r group] (sort-by (fn [[r group]] [(- (count group)) r]) (group-by :reason failures))]
    (println (format "  %3d  %s" (count group) r))
    (println (str "       " (str/join " " (map :id group))))))
(when-let [differs (seq (filter #(= :differs (:result %)) rows))]
  (println "\nGoose output differs from V8's:")
  (doseq [r differs] (println (str "  " (:id r) ": V8 prints " (:v8-prints r) "; Goose prints " (:goose-prints r)))))
(when-let [refused (seq (filter #(= :v8-refuses (:result %)) rows))]
  (println "\nV8 refuses (not scored):")
  (doseq [r refused] (println (str "  " (:id r) ": " (or (:recorded r) (bounded (:v8-error r) 200))))))
(println (str "\npass (" (get counts :pass 0) "): " (str/join " " (map :id (filter #(= :pass (:result %)) rows)))))
(println (format "\n%d of %d scored idioms pass on Goose (%d fail, %d differ); %d refused by V8"
                 (get counts :pass 0) (count scored) (get counts :fail 0) (get counts :differs 0) (get counts :v8-refuses 0)))
(println "ledger:" (str (fs/path out-dir "ledger.edn")))

(def passes (set (map :id (filter #(= :pass (:result %)) rows))))
(when (:write-expected options)
  (spit expected-file (str ";; Macro idioms that must pass on the Goose compiler path: `macro-ledger.bb\n"
                           ";; --check` fails when one does not, or when a pass is not listed here.\n"
                           (with-out-str (pprint/pprint (vec (sort passes))))))
  (println "wrote" expected-file (count passes) "idioms"))
(when (:check options)
  (let [{:keys [expected regressions new-passes refusal-changes]} (check-outcome idioms rows expected)
        by-id (into {} (map (juxt :id identity) rows))]
    (doseq [id new-passes] (println "NEW PASS (add it to expected-pass.edn):" id))
    (doseq [id regressions] (println "REGRESSION:" id "-" (:reason (by-id id))))
    (doseq [id refusal-changes]
      (println "V8 REFUSAL CHANGED:" id "-" (if (= :v8-refuses (:result (by-id id)))
                                               (str "V8 now refuses: " (bounded (:v8-error (by-id id)) 200))
                                               "V8 now builds it; drop its :v8-refuses")))
    (println (format "check: %d expected passes, %d regressions, %d unlisted passes, %d V8 refusal changes"
                     (count expected) (count regressions) (count new-passes) (count refusal-changes)))
    (when (or (seq regressions) (seq new-passes) (seq refusal-changes)) (System/exit 1))))
