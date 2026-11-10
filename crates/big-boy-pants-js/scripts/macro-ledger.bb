#!/usr/bin/env bb
;; Macro-idiom ledger (light-cljs-916): scores the Goose compiler path on
;; ordinary macro idioms against the V8 path, beside conformance.bb, which
;; scores JavaScript against the Pants engine and QuickJS.
;;
;;   bb crates/big-boy-pants-js/scripts/macro-ledger.bb [options]
;;     --step NAME        only this step: idioms or core-boot (default both)
;;     --idiom ID         only this idiom (repeatable)
;;     --jobs N           idioms built in parallel (default 4)
;;     --timeout S        seconds per build, counted as a failure (default 120)
;;     --check            exit 1 on a regression against macro-ledger/
;;                        expected-pass.edn, on a pass it does not list, or
;;                        when V8's refusals differ from idioms.edn's; and
;;                        when the core boot stops anywhere but at the point
;;                        macro-ledger/core-boot.edn records
;;     --write-expected   rewrite expected-pass.edn with today's passes
;;                        (every idiom only) and core-boot.edn with today's
;;                        stopping point
;;     --light PATH       light-cljs built with --features goose-macros
;;                        (default <target>/release/light-cljs, built here)
;;     --runner PATH      the Goose compiler runner (default
;;                        <target>/macro-ledger/runner, built here from
;;                        src/compiler_session.goose when a Goose source is
;;                        newer)
;;     --no-build         use the tools under the target directory as they are
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
;;
;; The core-boot step (light-cljs-917): how far the real cljs.core, the
;; shared-heap profile of the runtime (runtime_manifest::SHARED_RUNTIME;
;; docs/real-core-design.md), boots on Goose. examples/shared_runtime.rs
;; prints the exact text the compiler embeds; the step wraps it as
;;   const rt = <text>;
;;   Object.keys(rt.core).length;
;; exports that strict script with rust/examples/goose_export and runs the
;; image on src/conformance.goose's runner, which reports the outcome, the
;; fault and its detail, the instruction (with its source span) it stopped
;; at, and the instructions executed (:steps). macro-ledger/core-boot.edn
;; records the stopping point. A stop is the recorded one when its status,
;; fault, detail and source line (or a completed boot's value) match;
;; otherwise the one with more steps got further, an export refusal being
;; before every run and a completed boot after every stop. --check fails when
;; the boot stops earlier, and asks for the record to be advanced
;; (--write-expected) when it gets further; any other change fails too. The
;; recorded steps, byte offset, line number and export counts are only
;; reported, so an edit of the runtime text that moves the stop's line does
;; not fail the check. The step prints the image size and the run's wall
;; time and table rows (src/census.goose), recorded once the boot completes,
;; and two lists read from the profile's code (its comments, strings and
;; regular expressions removed): the `__rt` members it calls that the Goose
;; VM does not implement (no `opcode::` arm in source_vm.goose's
;; instruction, no `form_op::` arm in its form_op or in form_ops.goose's
;; job_form_op), and the names it uses that src/realm.goose still has as
;; PENDING.
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
        "--step" (recur (rest more) (assoc o :step (first more)))
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
(when-not (contains? #{nil "idioms" "core-boot"} (:step options)) (fail! "--step takes idioms or core-boot"))
(when (and (= "core-boot" (:step options)) (seq (:idioms options))) (fail! "--idiom selects idioms; --step core-boot runs none"))
(def idioms-step? (not= "core-boot" (:step options)))
(def core-boot-step? (not= "idioms" (:step options)))

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

;; ---------------------------------------------------------------- core boot
(def rust (fs/path root "rust"))
(def boot-dir (fs/path out-dir "core-boot"))
(def boot-file (str (fs/path ledger-dir "core-boot.edn")))
(def shared-runtime-tool (str (fs/path target "release" "examples" "shared_runtime")))
(def exporter (str (fs/path target "release" "examples" "goose_export")))
(def boot-runner (str (fs/path out-dir "boot-runner")))

;; The line number and trimmed text of the line holding byte `at` of the
;; UTF-8 bytes b (a runner span is a byte offset into the exported source).
(defn source-line [^bytes b at]
  (when (and (int? at) (<= 0 at) (< at (alength b)))
    (let [newline? #(= 10 (aget b %))
          start (loop [i at] (if (and (pos? i) (not (newline? (dec i)))) (recur (dec i)) i))
          end (loop [i at] (if (and (< i (alength b)) (not (newline? i))) (recur (inc i)) i))]
      {:line-number (inc (count (filter newline? (range start))))
       :line (bounded (str/trim (String. b (int start) (int (- end start)) "UTF-8")) 160)})))

;; Text a lexer would call code: comments, string and template text and
;; regular-expression literals become spaces (newlines and template
;; substitutions stay), so a name found in it is a name the code uses. A
;; slash starts a regular expression after an operator, an opening bracket,
;; a keyword such as `return`, or nothing, as in src/js_syntax.rs.
(def regex-keywords #{"return" "typeof" "instanceof" "in" "of" "new" "delete" "void" "throw" "case" "do" "else" "yield" "await"})
(defn code-only [^String s]
  (let [n (.length s) sb (StringBuilder. s)
        at #(if (< % n) (.charAt s %) \u0000)
        blank! (fn [from to] (doseq [j (range from (min to n))] (when-not (= \newline (.charAt s j)) (.setCharAt sb j \space))))
        word? #(or (Character/isLetterOrDigit (char %)) (= \_ %) (= \$ %))
        ;; The end of the quoted text starting at i (after its closing quote).
        quoted (fn [i q] (loop [j (inc i)] (let [c (at j)] (cond (>= j n) n (= c \\) (recur (+ j 2)) (= c q) (inc j) (= c \newline) j :else (recur (inc j))))))
        ;; A regular expression from i: [its closing slash, its end after
        ;; the flags].
        regex (fn [i] (loop [j (inc i) class? false]
                        (let [c (at j)]
                          (cond (or (>= j n) (= c \newline)) [(min j n) (min j n)] (= c \\) (recur (+ j 2) class?)
                                (= c \[) (recur (inc j) true) (= c \]) (recur (inc j) false)
                                (and (= c \/) (not class?)) [j (loop [k (inc j)] (if (word? (at k)) (recur (inc k)) k))]
                                :else (recur (inc j) class?)))))
        ;; Template text from i: [end :close] after its closing backtick, or
        ;; [end :substitution] after a `${`.
        template (fn [i] (loop [j i] (let [c (at j)] (cond (>= j n) [n :close] (= c \\) (recur (+ j 2)) (= c \`) [(inc j) :close]
                                                          (and (= c \$) (= \{ (at (inc j)))) [(+ j 2) :substitution] :else (recur (inc j))))))
        in-template (fn [i braces] (let [[e how] (template i)] (blank! i (if (= how :close) (dec e) (- e 2))) [e (if (= how :close) braces (conj braces 0))]))]
    (loop [i 0 regex-ok true braces ()]
      (if (>= i n)
        (str sb)
        (let [c (.charAt s i) d (at (inc i))]
          (cond
            (Character/isWhitespace c) (recur (inc i) regex-ok braces)
            (and (= c \/) (= d \/)) (let [e (or (str/index-of s "\n" i) n)] (blank! i e) (recur e regex-ok braces))
            (and (= c \/) (= d \*)) (let [e (if-let [k (str/index-of s "*/" (+ i 2))] (+ k 2) n)] (blank! i e) (recur e regex-ok braces))
            (or (= c \") (= c \')) (let [e (quoted i c)] (blank! (inc i) (dec e)) (recur e false braces))
            (= c \`) (let [[e braces] (in-template (inc i) braces)] (recur e false braces))
            (and (= c \}) (seq braces) (zero? (first braces))) (let [[e braces] (in-template (inc i) (rest braces))] (recur e false braces))
            (= c \/) (if regex-ok (let [[close e] (regex i)] (blank! (inc i) close) (recur e false braces)) (recur (inc i) true braces))
            (word? c) (let [e (loop [k (inc i)] (if (word? (at k)) (recur (inc k)) k))]
                        (recur e (contains? regex-keywords (subs s i e)) braces))
            :else (recur (inc i) (not (#{\) \] \}} c))
                         (cond (empty? braces) braces
                               (= c \{) (conj (rest braces) (inc (first braces)))
                               (= c \}) (conj (rest braces) (dec (first braces)))
                               :else braces))))))))

;; The text of the function or table that starts at `start` in text, up to
;; `end`, the first one after it.
(defn section [text start end]
  (let [from (or (str/index-of text start) (fail! "no `" start "` where the core-boot step reads it"))]
    (subs text from (or (str/index-of text end from) (fail! "no end of `" start "`")))))

;; The sources the lists are read from, by role.
(defn source-texts []
  {:intrinsics (slurp (str (fs/path rust "src/frontend/intrinsics.rs")))
   :instr (slurp (str (fs/path rust "src/vm/instr.rs")))
   :vm (slurp (str (fs/path root "src" "source_vm.goose")))
   :ops (slurp (str (fs/path root "src" "form_ops.goose")))
   :realm (slurp (str (fs/path root "src" "realm.goose")))
   :schema (slurp (str (fs/path root "src" "export_schema.goose")))})

;; `__rt` member -> what it lowers to, {:opcode Name} or {:form-op Name}, read
;; from the frontend's intrinsic table (rust/src/frontend/intrinsics.rs) and
;; FormOp::intrinsic (rust/src/vm/instr.rs); the three lowerings that name
;; no opcode there emit the one rust/src/frontend/lower.rs emits for them.
(defn intrinsic-table [{:keys [intrinsics]}] (section intrinsics "pub(crate) const INTRINSICS" "\n];"))
(defn intrinsic-lowerings [{:keys [instr] :as sources}]
  (let [table (intrinsic-table sources)
        form-names (into {} (map (comp vec rest) (re-seq #"FormOp::(\w+) => \"(\w+)\"" (section instr "pub const fn intrinsic(self)" "\n    }\n"))))
        emitted {"Call" "CallMethod" "HostEffect" "HostEffect" "Need" "Need"}]
    (merge (into {} (for [[_ m op] (re-seq #"op\(\"(\w+)\", \d+, Opcode::(\w+)\)" table)] [m {:opcode op}]))
           (into {} (for [[_ m lowering op] (re-seq #"name: \"(\w+)\",\s*arity: [^\n]*\n\s*lowering: Lowering::(\w+)(?:\(Opcode::(\w+)\))?" table)]
                      [m {:opcode (or op (emitted lowering) (fail! "intrinsic " m " lowers to " lowering ", which names no opcode"))}]))
           (into {} (for [[_ op] (re-seq #"form_op\(FormOp::(\w+)\)" table)]
                      [(or (form-names op) (fail! "FormOp::" op " has no intrinsic name")) {:form-op op}])))))

;; The opcodes the Goose VM decodes into an instruction and the form
;; operations its form_op (src/source_vm.goose) or, for those that need the
;; job, job_form_op (src/form_ops.goose) has an arm for; every other one
;; fails by name when it runs.
(defn goose-implemented [{:keys [vm ops]}]
  {:opcode (set (map second (re-seq #"opcode::(\w+)" (section vm "fn instruction(op: Op) -> Ins {" "\n}\n"))))
   :form-op (set (map second (concat (re-seq #"form_op::(\w+)" (section vm "fn form_op(heap: Heap&, op: i64, args: const Value[:]) -> Value {" "\n}\n"))
                                     (re-seq #"form_op::(\w+)" (section ops "fn job_form_op(job: Job&, op: i64, v: Value) -> Value {" "\n}\n")))))})

;; The realm's PENDING properties (src/realm.goose), each {:holder, the
;; holder's JavaScript name from the realm's fixed-row functions, :kind
;; :global, :static (a constructor's) or :property, :key}, a well-known
;; symbol's key spelled @@name.
(defn camel [words] (apply str (map str/capitalize words)))
(defn realm-pending [{:keys [realm schema]}]
  (let [names (fn [table] (mapv second (re-seq #"text: \"(\w+)\"" (or (re-find (re-pattern (str "let " table ":[^\n]*")) realm) ""))))
        families {"wrapper" (names "WRAPPER_NAMES") "error" (names "ERROR_NAMES") "function_kind" (names "FUNCTION_KIND_TAGS")}
        holder (fn [function family-name]
                 (let [words (str/split function #"_") role (last words)
                       base (or family-name (camel (butlast words)))]
                   (cond (= function "global") {:holder "globalThis" :kind :global}
                         (= role "prototype") {:holder (str base ".prototype") :kind :property}
                         (= role "constructor") {:holder base :kind :static}
                         :else {:holder (camel words) :kind :property})))
        rows (merge (into {} (for [[_ f n] (re-seq #"inline fn (\w+)_row\(\) -> i64 \{ return (\d+); \}" realm)] [(parse-long n) (holder f nil)]))
                    (into {} (for [[_ f n] (re-seq #"inline fn (\w+)_row\(\w+: i64\) -> i64 \{ return (\d+) \+ \w+; \}" realm)
                                   :let [table (some (fn [[family t]] (when (str/starts-with? f family) t)) families)]
                                   [i entry] (map-indexed vector table)]
                               [(+ (parse-long n) i) (holder f entry)])))
        symbols (into {} (for [[_ i s] (re-seq #"(\d+) => \"(\w+)\"" (section schema "fn export_well_known_symbol(" "\n}\n"))] [(parse-long i) s]))]
    (for [[_ row key symbol] (re-seq #"PendingName \{ holder: (\d+), key: \"([^\"]*)\"(?:, symbol: (\d+))? \}" (section realm "let PENDING:" "\n];"))]
      (merge (rows (parse-long row) {:holder (str "row " row) :kind :property})
             (if symbol {:key (str "@@" (symbols (parse-long symbol) (str "symbol " symbol)))} {:key key})))))

;; That the readers above read the real sources whole: every intrinsic of
;; the table, every PENDING entry the table's length declares, each with a
;; named holder and key, and only operations the schema defines.
(defn check-readers! [sources]
  (let [table (intrinsic-table sources)
        entries (+ (count (re-seq #"\bIntrinsic \{" table)) (count (re-seq #"\bop\(\"" table)) (count (re-seq #"\bform_op\(FormOp::" table)))
        lowerings (intrinsic-lowerings sources)
        declared (some-> (re-find #"let PENDING: PendingName\[(\d+)\]" (:realm sources)) second parse-long)
        pending (realm-pending sources)
        implemented (goose-implemented sources)
        defined (fn [space] (set (map second (re-seq (re-pattern (str "(?m)^let " space "::(\\w+) =")) (:schema sources)))))]
    (when-not (= entries (count lowerings)) (fail! "read " (count lowerings) " of the " entries " intrinsics in rust/src/frontend/intrinsics.rs"))
    (when-not (= declared (count pending)) (fail! "read " (count pending) " of the " declared " PENDING entries in src/realm.goose"))
    (when-let [bad (seq (filter #(or (str/starts-with? (:holder %) "row ") (str/starts-with? (:key %) "@@symbol") (str/blank? (:key %))) pending))]
      (fail! "PENDING entries with no named holder or key: " (pr-str bad)))
    (doseq [[how space] [[:opcode "opcode"] [:form-op "form_op"]]]
      (when (empty? (implemented how)) (fail! "read no implemented " space " from src/source_vm.goose"))
      (when-let [bad (seq (remove (defined space) (concat (implemented how) (keep how (vals lowerings)))))]
        (fail! space " names src/export_schema.goose does not define: " (str/join " " (sort (set bad))))))))


;; How often the code uses a pending property: a global as a free name, a
;; constructor's static as Holder.key, a well-known symbol as Symbol.name,
;; and a prototype's (or another object's) method by name, whatever the
;; receiver: called (.key(...), .key.call(...)) or read off a prototype
;; (prototype.key), so a data property of the same name is no use of it.
(defn uses [code {:keys [holder kind key]}]
  (let [q #(java.util.regex.Pattern/quote %)
        pattern (cond (str/starts-with? key "@@") (str "(?<![\\w$.#])Symbol\\s*\\.\\s*" (q (subs key 2)) "(?![\\w$])")
                      (= kind :global) (str "(?<![\\w$.#])" (q key) "(?![\\w$])")
                      (= kind :static) (str "(?<![\\w$.#])" (q holder) "\\s*\\.\\s*" (q key) "(?![\\w$])")
                      :else (str "(?<!\\.\\.)\\.\\s*" (q key) "\\s*(?=\\(|\\.\\s*(?:call|apply|bind)(?![\\w$]))"
                                 "|(?<![\\w$])prototype\\s*\\.\\s*" (q key) "(?![\\w$])"))]
    (count (re-seq (re-pattern pattern) code))))
(assert (= [2 0 1 1] (map #(uses "a.slice(1); b.slice.call(c); d.sub; ...fixed.map(f); Array.from(x); new Map()" %)
                          [{:kind :property :key "slice"} {:kind :property :key "fixed"} {:kind :static :holder "Array" :key "from"} {:kind :global :key "Map"}])))

;; What the profile's code uses that Goose lacks: [missing-rt pending], each
;; a sequence of maps with :uses, the count in the code.
(defn boot-gaps [code sources]
  (let [lowerings (intrinsic-lowerings sources) implemented (goose-implemented sources)
        members (frequencies (map second (re-seq #"__rt\s*\.\s*([A-Za-z_$][\w$]*)" code)))]
    [(for [[m n] (sort members)
           :let [[how op] (first (lowerings m {:unknown m}))]
           :when (not (contains? (get implemented how #{}) op))]
       {:member m :lowering how :op op :uses n})
     (for [p (realm-pending sources) :let [n (uses code p)] :when (pos? n)] (assoc p :uses n))]))

;; The readers over a small source of each kind.
(let [sources {:intrinsics (str "pub(crate) const INTRINSICS: &[Intrinsic] = &[\n"
                                "    Intrinsic {\n        name: \"call\",\n        arity: Arity::AtLeast(2),\n        lowering: Lowering::Call,\n    },\n"
                                "    Intrinsic {\n        name: \"vector\",\n        arity: Arity::Any,\n        lowering: Lowering::Counted(Opcode::NewVector),\n    },\n"
                                "    op(\"count\", 1, Opcode::Count),\n    form_op(FormOp::Seal),\n    form_op(FormOp::CljFirst),\n];\n")
               :instr (str "    pub const fn intrinsic(self) -> &'static str {\n        match self {\n"
                           "            FormOp::Seal => \"seal\",\n            FormOp::CljFirst => \"cljFirst\",\n        }\n    }\n")
               :vm (str "fn instruction(op: Op) -> Ins {\n    return match op.code { opcode::CallMethod => Ins.Call, opcode::Count => Ins.Count, _ => Ins.Unsupported };\n}\n"
                        "fn form_op(heap: Heap&, op: i64, args: const Value[:]) -> Value {\n    match op { form_op::CljFirst => { return a; }, _ => { return b; } }\n}\n"
                        "fn other() { opcode::NewVector; form_op::Seal; }\n")
               :ops (str "fn job_form_op(job: Job&, op: i64, v: Value) -> Value {\n    if op == form_op::CljPeek { return v; }\n}\n"
                         "fn other() { form_op::Seal; }\n")
               :realm (str "inline fn global_row() -> i64 { return 0; }\ninline fn array_prototype_row() -> i64 { return 3; }\n"
                           "inline fn array_constructor_row() -> i64 { return 14; }\ninline fn wrapper_prototype_row(t: i64) -> i64 { return 26 + t; }\n"
                           "let WRAPPER_NAMES: Name[3] = [Name { text: \"Number\" }, Name { text: \"String\" }, Name { text: \"Boolean\" }];\n"
                           "let PENDING: PendingName[6] = [\n    // Array.prototype\n"
                           "    PendingName { holder: 0, key: \"Map\" }, PendingName { holder: 3, key: \"slice\" }, PendingName { holder: 14, key: \"from\" },\n"
                           "    PendingName { holder: 27, key: \"padEnd\" }, PendingName { holder: 0, key: \"Date\" }, PendingName { holder: 14, key: \"\", symbol: 1 },\n];\n")
               :schema "fn export_well_known_symbol(index: i64) -> const u8[:] {\n    return match index {\n        0 => \"iterator\",\n        1 => \"asyncIterator\",\n    };\n}\n"}
      code (code-only (str "const m = new Map(); // Date\nconst s = 'WeakMap Date'; x.slice(1); __rt.seal(a); __rt.vector(1); __rt.call(o, f);\n"
                           "__rt.cljFirst(s); __rt.count(v); Array.from(y); /* __rt.lazySeq(z) */ s.padEnd(2); Symbol.asyncIterator;"))]
  (assert (= {"call" {:opcode "CallMethod"} "vector" {:opcode "NewVector"} "count" {:opcode "Count"} "seal" {:form-op "Seal"} "cljFirst" {:form-op "CljFirst"}}
             (intrinsic-lowerings sources)))
  (assert (= {:opcode #{"CallMethod" "Count"} :form-op #{"CljFirst" "CljPeek"}} (goose-implemented sources)))
  (assert (= [["globalThis" "Map"] ["Array.prototype" "slice"] ["Array" "from"] ["String.prototype" "padEnd"] ["globalThis" "Date"] ["Array" "@@asyncIterator"]]
             (map (juxt :holder :key) (realm-pending sources))))
  (assert (= [[{:member "seal" :lowering :form-op :op "Seal" :uses 1} {:member "vector" :lowering :opcode :op "NewVector" :uses 1}]
              [["Map" 1] ["slice" 1] ["from" 1] ["padEnd" 1] ["@@asyncIterator" 1]]]
             (let [[missing pending] (boot-gaps code sources)] [missing (map (juxt :key :uses) pending)]))))

;; The boot's outcome from the exporter's and the runner's processes, each
;; {:exit :out :err} (the runner's nil when the export did not succeed): an
;; export refusal only for goose_export's named refusals (exit 3,
;; unsupported:..., or 2, syntax:...); a classified runner outcome only from
;; a runner that exited 0 with its outcome map and its heap counters; every
;; other result a :timeout or an :infrastructure failure, which never
;; passes --check or is recorded.
(def classified #{:returned :thrown :rejected :unsupported :limit})
(defn boot-outcome [e r]
  (let [line (str (first (str/split-lines (str (:out e)))))
        exported (re-matches #"exported:(\d+):(\d+)" line)
        lines (str/split-lines (str (:out r)))
        read (fn [s] (try (edn/read-string s) (catch Exception _ nil)))
        outcome (read (first lines))
        heap (:heap (read (second lines)))]
    (cond (= :timeout (:exit e)) {:status :timeout :detail (str "export " (:err e))}
          (and (= 3 (:exit e)) (str/starts-with? line "unsupported:")) {:status :export-refused :detail line}
          (and (= 2 (:exit e)) (str/starts-with? line "syntax:")) {:status :export-refused :detail line}
          (not (and (= 0 (:exit e)) exported)) {:status :infrastructure :detail (bounded (str "export exit " (:exit e) ": " line " " (:err e)) 300)}
          :else
          (merge {:export {:functions (parse-long (nth exported 1)) :instructions (parse-long (nth exported 2))}}
                 (cond (= :timeout (:exit r)) {:status :timeout :detail (:err r)}
                       (and (= 0 (:exit r)) (map? outcome) (classified (:status outcome)) (int? (:steps outcome))
                            (every? #(int? (get heap %)) [:nodes :values :properties :units]))
                       (assoc (select-keys outcome [:status :fault :detail :operation :value :steps :span]) :rows heap)
                       :else {:status :infrastructure
                              :detail (bounded (str "runner exit " (:exit r) ": " (or (:reason outcome) (str (:out r) (:err r)))) 300)})))))
(let [ok {:exit 0 :out "exported:3:9\n"}
      run {:exit 0 :out "{:status :returned :value \"n\" :steps 7}\n{:heap {:nodes 1 :values 2 :properties 3 :units 4}}\n"}]
  (assert (= {:export {:functions 3 :instructions 9} :status :returned :value "n" :steps 7 :rows {:nodes 1 :values 2 :properties 3 :units 4}}
             (boot-outcome ok run)))
  (assert (= :export-refused (:status (boot-outcome {:exit 3 :out "unsupported:module"} nil))))
  (assert (= :timeout (:status (boot-outcome {:exit :timeout :out "" :err "timed out"} nil))))
  (assert (= :infrastructure (:status (boot-outcome {:exit 1 :out "error:x.js: denied"} nil))))
  (assert (= :infrastructure (:status (boot-outcome {:exit 0 :out "garbage"} nil))))
  (assert (= :timeout (:status (boot-outcome ok {:exit :timeout :out "" :err "timed out"}))))
  ;; An outcome printed by a runner that then failed, or without its heap counters.
  (assert (= :infrastructure (:status (boot-outcome ok (assoc run :exit 134)))))
  (assert (= :infrastructure (:status (boot-outcome ok (update run :out #(first (str/split-lines %)))))))
  (assert (= :infrastructure (:status (boot-outcome ok {:exit 1 :out "{:status :infrastructure :reason \"cannot read program\"}"})))))

;; Boot the profile: export, run, and the outcome as a row.
(defn boot! []
  (fs/create-dirs boot-dir)
  (let [r (run-process {:dir repository :env base-env :timeout (:timeout options)} shared-runtime-tool)
        _ (when-not (= 0 (:exit r)) (fail! shared-runtime-tool " failed: " (bounded (str (:err r)) 300)))
        profile (:out r)
        js (str (fs/path boot-dir "boot.js")) image (str (fs/path boot-dir "boot.gsp"))
        _ (spit js (str "const rt = " profile ";\nObject.keys(rt.core).length;\n"))
        _ (fs/delete-if-exists image)
        e (run-process {:dir repository :env base-env :timeout (:timeout options)} exporter js image)
        started (System/nanoTime)
        r (when (and (= 0 (:exit e)) (fs/exists? image))
            (run-process {:dir repository :env base-env :timeout (:timeout options)} boot-runner image "heap"))
        seconds (/ (- (System/nanoTime) started) 1e9)
        outcome (boot-outcome e r)
        [start] (:span outcome)]
    (cond-> (merge {:profile-bytes (count (.getBytes ^String profile "UTF-8")) :code (code-only profile)}
                   (dissoc outcome :span))
      (:export outcome) (assoc-in [:export :image-bytes] (fs/size image))
      r (assoc :seconds seconds)
      start (merge (assoc (source-line (fs/read-all-bytes js) start) :byte start)))))

;; A stop's identity, and its place in the boot: an export refusal before
;; every run, a completed boot after every stop, a timeout or an
;; infrastructure failure nowhere (it always fails the check).
(def stop-keys [:status :fault :detail :operation :value :line])
(defn stop-rank [{:keys [status]}] (case status :export-refused 0 :returned 2 1))
(defn compare-stop [recorded now]
  (cond (#{:timeout :infrastructure} (:status now)) :broken
        (= (select-keys recorded stop-keys) (select-keys now stop-keys)) :same
        :else (let [c (compare [(stop-rank now) (or (:steps now) 0)] [(stop-rank recorded) (or (:steps recorded) 0)])]
                (cond (pos? c) :further (neg? c) :earlier :else :changed))))
(defn describe-stop [{:keys [status fault detail operation value reason steps line line-number byte]}]
  (str (case status
         :returned (str "completes with " value)
         :export-refused (str "is refused by the exporter: " detail)
         (:unsupported :limit) (str fault (when (or detail operation) (str " (" (or detail operation) ")")))
         (:thrown :rejected) (str (name status) " " value)
         (str (some-> status name) (when (or detail reason) (str ": " (or detail reason)))))
       (when steps (str " after " steps " instructions"))
       (when line (str "\n    source line " line-number " (byte " byte "): " line))))

;; The scoring rules, checked on every run.
(let [seal {:status :unsupported :fault "Fault.NativeBoundary" :detail "form operation seal" :steps 8404 :line "kinds: __rt.seal([...])," :byte 69475}]
  (assert (= :same (compare-stop seal (assoc seal :steps 9000 :byte 70000 :line-number 1))))
  (assert (= :further (compare-stop seal {:status :unsupported :fault "Fault.UnsupportedOperand" :detail "pending Map" :steps 9000 :line "x"})))
  (assert (= :earlier (compare-stop seal {:status :thrown :value "error:TypeError" :steps 100})))
  (assert (= :earlier (compare-stop seal {:status :export-refused :detail "unsupported:x"})))
  (assert (= :further (compare-stop seal {:status :returned :value "688" :steps 10})))
  (assert (= :changed (compare-stop seal (assoc seal :detail "form operation sealReachable"))))
  (assert (= :broken (compare-stop seal {:status :timeout}))))
(assert (= (code-only "a = 'x//y' // c\nb = /re\\/'/g; `t${u + `v`}w` /* d */ c / 2")
           "a = '    '     \nb = /     /g; ` ${u + ` `} `         c / 2"))
(assert (= (code-only "return /[/]x/.test(s) ? {a: 1} : f(x) / y")
           "return /    /.test(s) ? {a: 1} : f(x) / y"))

(defn print-gaps! [[missing pending]]
  (let [label (fn [{:keys [member key uses]}] (str (or member key) " (" uses ")"))]
    (println (str "\ncore boot: __rt members the profile calls that the Goose VM does not implement ("
                  (count missing) "; uses in the profile's code):"))
    (doseq [[how group] (sort-by key (group-by :lowering missing))]
      (println (str "  " (case how :form-op "form operations" :opcode "opcodes" "unknown") ": " (str/join " " (map label group)))))
    (println (str "\ncore boot: names the profile uses that src/realm.goose has as PENDING (" (count pending)
                  "; a method by name, whatever its receiver):"))
    ;; Holders whose used names are the same share a line.
    (doseq [[items holders] (sort-by (fn [[items holders]] [(- (count items)) holders])
                                     (for [[items group] (group-by first (for [[holder group] (group-by :holder pending)] [(mapv label group) holder]))]
                                       [items (str/join ", " (sort (map second group)))]))]
      (println (str "  " holders ": " (str/join " " items))))))

(defn run-core-boot!
  "Run the step and print it; false when --check fails."
  []
  (let [b (boot!)
        record (when (fs/exists? boot-file) (edn/read-string (slurp boot-file)))
        recorded (:stop record)
        {:keys [functions instructions image-bytes]} (:export b)]
    (println (str "\ncore boot: the shared-profile runtime (" (:profile-bytes b) " bytes)"
                  (if (:export b) (str " exports as " functions " functions and " instructions " instructions, a " image-bytes "-byte image") "")))
    (println (str "core boot: " (if (= :returned (:status b)) "the boot " "the boot stops: ") (describe-stop b)))
    (when (:seconds b)
      (println (format "core boot: the run took %.2f s; table rows %s: %s" (:seconds b) (if (= :returned (:status b)) "used" "at the stop")
                       (str/join " " (for [k [:nodes :values :properties :units]] (str (name k) " " (get (:rows b) k)))))))
    (let [sources (source-texts)]
      (check-readers! sources)
      (print-gaps! (boot-gaps (:code b) sources)))
    (spit (str (fs/path out-dir "core-boot.edn")) (with-out-str (pprint/pprint (dissoc b :code))))
    (when (:write-expected options)
      (when (#{:timeout :infrastructure} (:status b)) (fail! "core boot: will not record a " (name (:status b)) ": " (:detail b)))
      (spit boot-file (str ";; Where the shared-profile cljs.core boot stops on Goose: `macro-ledger.bb\n"
                           ";; --check` fails when it stops earlier or elsewhere, and asks for this\n"
                           ";; record to be advanced (--write-expected) when it gets further. :stop's\n"
                           ";; status, fault, detail, operation, value and line identify the stop;\n"
                           ";; :steps orders two stops; the rest is reported only.\n"
                           (with-out-str
                             (pprint/pprint (cond-> {:export (:export b)
                                                     :stop (select-keys b (concat stop-keys [:steps :byte :line-number]))}
                                              (= :returned (:status b)) (assoc :completed {:seconds (/ (Math/round (* 100.0 (:seconds b))) 100.0) :rows (:rows b)}))))))
      (println "wrote" boot-file))
    (if-not (:check options)
      true
      (if-not recorded
        (do (println "BOOT NOT RECORDED: run with --write-expected to record" boot-file) false)
        (let [outcome (compare-stop recorded b)
              moved (for [k [:steps :byte :line-number] :when (not= (k recorded) (k b))] (str (name k) " " (k recorded) " -> " (k b)))
              export (when (and (:export record) (:export b) (not= (:export record) (:export b))) (str "export " (pr-str (:export record)) " -> " (pr-str (:export b))))]
          (case outcome
            :same (println (str "core boot check: stops at the recorded point"
                                (when-let [notes (seq (remove nil? (concat moved [export])))] (str "; changed from the record: " (str/join ", " notes)))))
            :further (println (str "BOOT ADVANCED (record it with --step core-boot --write-expected): recorded stop "
                                   (describe-stop recorded) "\n  now " (describe-stop b)))
            :earlier (println (str "BOOT REGRESSION: the boot stops earlier than recorded: " (describe-stop recorded) "\n  now " (describe-stop b)))
            :changed (println (str "BOOT STOP CHANGED at the same step: recorded " (describe-stop recorded) "\n  now " (describe-stop b)))
            :broken (println (str "BOOT BROKEN: " (describe-stop b))))
          (= :same outcome))))))

;; ---------------------------------------------------------------- tools
(defn shell! [& command] (println "+" (str/join " " command)) (flush) (apply p/shell {:dir (str repository)} command))
(def light (str (fs/absolutize (or (:light options) (fs/path target "release" "light-cljs")))))
(def runner (str (fs/absolutize (or (:runner options) (fs/path out-dir "runner")))))
;; A Goose program is rebuilt when a Goose source, C header or build sidecar
;; under src is newer than it.
(defn goose-build! [program output]
  (let [sources (fs/glob (fs/path root "src") "*.{goose,h,build}")
        newest (apply max (map #(.toMillis (fs/last-modified-time %)) sources))]
    (when (or (not (fs/exists? output)) (< (.toMillis (fs/last-modified-time output)) newest))
      (shell! goose "build" (str (fs/path root "src" program)) "-o" output))))
(fs/create-dirs out-dir)
(when-not (:no-build options)
  ;; One Cargo run builds light-cljs and the example printing its runtime,
  ;; which share the library.
  (let [light? (and idioms-step? (not (:light options)))]
    (when (or light? core-boot-step?)
      (apply shell! (concat ["cargo" "build" "--release" "--locked" "--features" "goose-macros"]
                            (when light? ["--bin" "light-cljs"]) (when core-boot-step? ["--example" "shared_runtime"])))))
  (when (and idioms-step? (not (:runner options))) (goose-build! "compiler_session.goose" runner))
  (when core-boot-step?
    ;; The run-time compiler the runner loads on its first eval
    ;; (src/runtime_compiler.h) is goose_frontend's library.
    (shell! "cargo" "build" "--locked" "--release" "-p" "big-boy-pants-js" "--example" "goose_export" "--example" "goose_frontend")
    (goose-build! "conformance.goose" boot-runner)))
(doseq [tool (concat (when idioms-step? [light runner]) (when core-boot-step? [shared-runtime-tool exporter boot-runner]))]
  (when-not (fs/exists? tool) (fail! "missing tool " tool)))

;; ---------------------------------------------------------------- run
(defn run-idioms!
  "Run the idioms and print the ledger; false when --check fails."
  []
  (let [rows (let [pool (Executors/newFixedThreadPool (:jobs options))
                   started (System/nanoTime)
                   futures (mapv (fn [i] (.submit pool ^Callable (fn [] (run-idiom light runner i)))) idioms)
                   results (mapv #(.get %) futures)]
               (.shutdown pool) (.awaitTermination pool 1 TimeUnit/MINUTES)
               (println (format "built %d idioms on the V8 and Goose paths in %.1f s with %d jobs"
                                (count idioms) (/ (- (System/nanoTime) started) 1e9) (:jobs options)))
               results)
        counts (frequencies (map :result rows))
        scored (remove #(= :v8-refuses (:result %)) rows)
        passes (set (map :id (filter #(= :pass (:result %)) rows)))]
    (spit (str (fs/path out-dir "ledger.edn"))
          (with-out-str (println ";; Generated by crates/big-boy-pants-js/scripts/macro-ledger.bb; one row per idiom.")
            (pprint/pprint rows)))
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
    (when (:write-expected options)
      (spit expected-file (str ";; Macro idioms that must pass on the Goose compiler path: `macro-ledger.bb\n"
                               ";; --check` fails when one does not, or when a pass is not listed here.\n"
                               (with-out-str (pprint/pprint (vec (sort passes))))))
      (println "wrote" expected-file (count passes) "idioms"))
    (if-not (:check options)
      true
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
        (not (or (seq regressions) (seq new-passes) (seq refusal-changes)))))))

(let [idioms-ok (if idioms-step? (run-idioms!) true)
      boot-ok (if core-boot-step? (run-core-boot!) true)]
  (when-not (and idioms-ok boot-ok) (System/exit 1)))
