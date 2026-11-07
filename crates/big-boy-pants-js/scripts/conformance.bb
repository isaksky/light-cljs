#!/usr/bin/env bb
;; Goose JavaScript conformance harness (docs/javascript-coverage.md,
;; decision 7). Runs every inventoried case on the Goose VM, the Pants engine
;; (bbp's Rust implementation, through its command line) and QuickJS (`qjs`),
;; writes target/conformance/ledger.edn and prints counts per area and per
;; origin. The Pants engine and QuickJS are oracles only: Goose executes the
;; case or fails by name; nothing falls back.
;;
;;   bb crates/big-boy-pants-js/scripts/conformance.bb [options]
;;     --area NAME        only cases of this area (e.g. operators-values)
;;     --case ID          only this case (repeatable)
;;     --jobs N           parallel case processes (default 4)
;;     --timeout S        seconds per engine run, counted as a failure (default 30)
;;     --check            exit 1 when a case in conformance/expected-pass.edn
;;                        does not pass on Goose; new passes are only reported
;;     --write-expected   rewrite expected-pass.edn with today's Goose passes
;;                        (whole inventory only)
;;     --stack-stats      build the Goose runner with -DGS_STACK_STATS=1 and
;;                        summarise its stack report over the run
;;     --heap-stats       record each Goose run's job heap rows at its end and
;;                        the rows no root reaches, and summarise them
;;     --no-build         reuse the built tools under the target directory
;;     --list             print the inventory and exit
;;
;; Case sources and labels (conformance/areas.edn holds each case's area):
;;   pants/NAME              rust/tests/fixtures/js/NAME.js, whole file; origin
;;                           from areas.edn, read off the file's header
;;   pants/quickjs_modules/NAME  a module fixture from that directory
;;   quickjs/FILE/FUNCTION   ~/src/quickjs/tests/FILE: "unmodified text,
;;                           per-function entry": the file's text, unchanged,
;;                           with its (last) block of top-level test calls
;;                           replaced by exactly one of those call lines
;;   quickjs/FILE            a whole unmodified upstream file
;;   bbp/NAME                conformance/cases/NAME.js, bbp-authored, whose
;;                           header names the lines it derives from
;;
;; Outcomes. Each engine yields :returned or :thrown with a canonical value,
;; or a failure status. The oracles agree when both return equal values, or
;; both throw (with the same error name when both name one); that agreed
;; outcome is the expectation. Goose then scores :pass (same outcome), :fail
;; (other outcome, VM limit, timeout or infrastructure failure, with its
;; reason) or :unsupported (a named exporter refusal or VM fault). Cases the
;; oracles do not agree on are :disagreement and are not scored, except a
;; case whose areas.edn entry has a :pants-gap reason: a feature the Pants
;; engine does not implement (its VM is not extended for Goose), scored
;; against QuickJS alone, with the Pants outcome still recorded. Returned
;; values compare exactly (numbers by bits, strings by UTF-16 code units,
;; objects by kind only); thrown values compare by error name when both
;; sides name one, and by status otherwise.
;;
;; Engines. Goose: rust/examples/goose_export.rs writes the image, then
;; src/conformance.goose runs it. The Pants engine: its command line
;; (rust/src/main.rs) with --host-bindings (its QuickJS-style std.gc and os
;; timers, the counterpart of qjs --std), plus --sloppy or --module by case.
;; QuickJS: `qjs --std --experimental` (std/os globals, as QuickJS's Makefile
;; runs test_builtin.js, and Intl, as it runs test_intl.js) with a driver that
;; evaluates the case as a global script; strict cases get a leading
;; `"use strict"; void 0;` there.
(require '[babashka.fs :as fs] '[babashka.process :as p]
         '[clojure.edn :as edn] '[clojure.string :as str] '[clojure.pprint :as pprint])
(import '[java.util.concurrent Executors TimeUnit])

(def directory (fs/parent (fs/canonicalize *file*)))
(def root (fs/parent directory))
(def repository (fs/canonicalize (fs/path root "../..")))
(def rust (fs/path root "rust"))
(def conformance (fs/path root "conformance"))
(def target (fs/path (or (System/getenv "CARGO_TARGET_DIR") (str (fs/path repository "target")))))
(def out-dir (fs/path target "conformance"))
(def quickjs-dir (fs/path (or (System/getenv "QUICKJS_DIR") (str (fs/path (System/getProperty "user.home") "src/quickjs")))))
(def qjs (str (fs/path quickjs-dir "qjs")))
(def pants (str (fs/path target "release/big-boy-pants-js")))
(def exporter (str (fs/path target "release/examples/goose_export")))
(def goose (or (System/getenv "GOOSE") "goose"))

(defn fail! [& message]
  (binding [*out* *err*] (println (apply str "conformance: " message)))
  (System/exit 2))

;; ---------------------------------------------------------------- options
(def options
  (loop [args *command-line-args* o {:jobs 4 :timeout 30 :cases #{}}]
    (if-let [[a & more] (seq args)]
      (case a
        "--area" (recur (rest more) (assoc o :area (keyword (first more))))
        "--case" (recur (rest more) (update o :cases conj (first more)))
        "--jobs" (recur (rest more) (assoc o :jobs (parse-long (first more))))
        "--timeout" (recur (rest more) (assoc o :timeout (parse-long (first more))))
        "--check" (recur more (assoc o :check true))
        "--write-expected" (recur more (assoc o :write-expected true))
        "--stack-stats" (recur more (assoc o :stack-stats true))
        "--heap-stats" (recur more (assoc o :heap-stats true))
        "--no-build" (recur more (assoc o :no-build true))
        "--list" (recur more (assoc o :list true))
        (fail! "unknown argument " a))
      o)))
(when-not (and (pos-int? (:jobs options)) (pos-int? (:timeout options))) (fail! "--jobs and --timeout take positive integers"))

;; ---------------------------------------------------------------- inventory
(def areas (edn/read-string (slurp (str (fs/path conformance "areas.edn")))))
(def area-names (set (keys (:areas areas))))
(def quickjs-revision "7d1c13c5d8f8c6b32893786ed35cdcebd88194d2")

(defn file-name [path] (str (fs/file-name path)))
(defn stem [path] (str/replace (file-name path) #"\.js$" ""))

(def call-line #"^([A-Za-z_$][\w$]*)\(.*\).*$")
(defn per-function-entries
  "The last block of top-level call lines (blank lines allowed) of an upstream
  test file; each call becomes one entry: the file's text, unchanged, with
  that block replaced by the one call line."
  [path]
  (let [lines (str/split (slurp (str path)) #"\n" -1)
        call? #(re-matches call-line %)
        end (inc (or (last (keep-indexed #(when (call? %2) %1) lines)) -1))
        start (loop [i end]
                (if (and (pos? i) (let [l (nth lines (dec i))] (or (str/blank? l) (call? l))))
                  (recur (dec i)) i))
        prefix (str/join "\n" (subvec lines 0 start))
        suffix (str/join "\n" (subvec lines end))
        entry #(str prefix "\n" % "\n" suffix)
        calls (for [l (subvec lines start end) :when (not (str/blank? l))] [(second (call? l)) l])]
    (when (pos? end)
      (for [[function line] calls] {:function function :text (entry line)}))))

(defn inventory []
  (let [fixtures (fs/path rust "tests/fixtures/js")
        pants-cases (for [f (sort (fs/glob fixtures "*.js"))]
                      {:id (str "pants/" (stem f)) :path (str f)})
        module-cases (for [f (sort (concat (fs/glob (fs/path fixtures "quickjs_modules") "*.js")
                                           (fs/glob (fs/path fixtures "quickjs_modules") "*.json")))]
                       {:id (str "pants/quickjs_modules/" (file-name f)) :path (str f) :mode :module})
        upstream (for [f (sort (fs/glob (fs/path quickjs-dir "tests") "*.js"))
                       :let [file (file-name f) whole (str "quickjs/" file)]
                       c (if (or (contains? (:cases areas) whole) (contains? (:excluded areas) whole))
                           [{:id whole :path (str f) :origin :quickjs-unmodified :mode (get-in areas [:cases whole :mode] :sloppy)}]
                           (for [{:keys [function text]} (or (seq (per-function-entries f))
                                                             (fail! "unmapped file " whole ": no top-level calls to make per-function entries from; map or exclude it in areas.edn"))]
                             {:id (str whole "/" function) :text text :mode :sloppy
                              :origin :quickjs-per-function :label "unmodified text, per-function entry"}))]
                   c)
        bbp (for [f (sort (fs/glob (fs/path conformance "cases") "*.js"))]
              (let [header (str/join "\n" (take 3 (str/split-lines (slurp (str f)))))]
                (when-not (and (str/includes? header "bbp-authored conformance case")
                               (re-find #"Derives from: (\S+.* lines? \d+|none \(.+)" header))
                  (fail! f ": a bbp-authored case needs the header lines \"bbp-authored conformance case\" and \"Derives from: PATH lines N-M\" (or \"Derives from: none (REASON)\")"))
                {:id (str "bbp/" (stem f)) :path (str f) :origin :bbp-authored}))]
    (when-not (fs/exists? (fs/path quickjs-dir "tests/test_language.js")) (fail! "missing QuickJS tests at " quickjs-dir))
    (vec (concat pants-cases module-cases upstream bbp))))

(defn labelled [cases]
  (let [ids (set (map :id cases))
        stale (remove ids (concat (keys (:cases areas)) (keys (:excluded areas))))]
    (when (seq stale) (fail! "areas.edn names cases that do not exist: " (str/join ", " (sort stale))))
    (vec (for [c cases :let [mapped (get-in areas [:cases (:id c)]) excluded (get-in areas [:excluded (:id c)])]]
           (cond
             (and mapped excluded) (fail! (:id c) " is both mapped and excluded")
             excluded (do (when (str/blank? (:reason excluded)) (fail! (:id c) " is excluded without a reason"))
                          (merge c {:area (:area excluded) :excluded (:reason excluded)
                                    :origin (or (:origin excluded) (:origin c))}))
             mapped (do (when-not (area-names (:area mapped)) (fail! (:id c) " has unknown area " (:area mapped)))
                        (let [origin (or (:origin mapped) (:origin c))]
                          (when-not origin (fail! (:id c) " needs an :origin in areas.edn"))
                          (when (and (contains? mapped :pants-gap) (str/blank? (:pants-gap mapped))) (fail! (:id c) " has a :pants-gap without a reason"))
                          (merge c (select-keys mapped [:note :pants-gap]) {:area (:area mapped) :origin origin
                                    :mode (or (:mode mapped) (:mode c)
                                              (if (#{:pants-authored :bbp-authored} origin) :strict :sloppy))})))
             :else (fail! "unmapped case " (:id c) ": add it to conformance/areas.edn"))))))

;; ---------------------------------------------------------------- processes
(defn run-process
  "Run a command with a deadline; returns {:exit :out :err} or {:timeout true}."
  [command & {:keys [dir]}]
  (let [proc (p/process command (cond-> {:out :string :err :string} dir (assoc :dir (str dir))))
        result (deref proc (* 1000 (:timeout options)) ::timeout)]
    (if (= result ::timeout)
      (do (p/destroy-tree proc) (deref proc 5000 nil) {:timeout true})
      (select-keys result [:exit :out :err]))))

(defn first-line [s] (let [l (first (str/split-lines (or s "")))] (if (> (count l) 240) (str (subs l 0 240) "...") l)))

;; ---------------------------------------------------------------- values
(defn utf16 [^String s] (mapv int (.toCharArray s)))
(defn number-value [text]
  (let [n (case text ("NaN") Double/NaN ("Infinity" "inf") Double/POSITIVE_INFINITY ("-Infinity" "-inf") Double/NEGATIVE_INFINITY
                (Double/parseDouble text))]
    [:number (if (Double/isNaN n) :nan (Double/doubleToLongBits n))]))
(defn canonical-value
  "Values in source::canonical's text form (Goose and the QuickJS driver)."
  [v]
  (cond (= v "undefined") [:undefined]
        (= v "null") [:null]
        (str/starts-with? v "boolean:") [:boolean (= v "boolean:true")]
        (str/starts-with? v "number:") (number-value (subs v 7))
        (str/starts-with? v "utf16:") [:string (edn/read-string (str/replace (subs v 6) "," " "))]
        (str/starts-with? v "bigint:") [:bigint (subs v 7)]
        (str/starts-with? v "error:") [:error (subs v 6)]
        (or (str/starts-with? v "identity:") (str/starts-with? v "object:")) [:object]
        (= v "symbol") [:symbol]
        :else [:unknown v]))
(defn rust-debug-string
  "Unescape a Rust `Debug` string literal into UTF-16 code units."
  [s]
  (let [body (subs s 1 (dec (count s)))]
    (loop [i 0 out []]
      (if (>= i (count body)) out
          (let [c (.charAt body i)]
            (if (not= c \\) (recur (inc i) (conj out (int c)))
                (let [e (.charAt body (inc i))]
                  (case e
                    \u (let [close (str/index-of body "}" i)
                             cp (Long/parseLong (subs body (+ i 3) close) 16)]
                         (recur (inc close) (into out (if (> cp 0xFFFF) (utf16 (String. (Character/toChars cp))) [cp]))))
                    (recur (+ i 2) (conj out (int (case e \n \newline \r \return \t \tab \0 (char 0) e))))))))))))
(defn pants-value [text]
  (cond (str/starts-with? text "\"") [:string (rust-debug-string text)]
        (= text "Undefined") [:undefined] (= text "Null") [:null]
        (= text "Bool(true)") [:boolean true] (= text "Bool(false)") [:boolean false]
        (= text "Object") [:object] (= text "Symbol") [:symbol] (= text "a string") [:string :opaque]
        (re-matches #"-?\d+n" text) [:bigint (subs text 0 (dec (count text)))]
        :else (try (number-value text) (catch Exception _ [:unknown text]))))
(defn error-name
  "A thrown value from an engine's diagnostic text: an error name, an object,
  or a primitive's display text (:text)."
  [text]
  (let [text (or text "")]
    (cond (re-find #"^an object" text) [:object]
          (re-find #"^(\w*Error)\b" text) [:error (second (re-find #"^(\w*Error)\b" text))]
          (#{"undefined" "null"} text) [(keyword text)]
          (#{"true" "false"} text) [:boolean (= text "true")]
          :else (try (number-value text) (catch Exception _ [:text text])))))

;; ---------------------------------------------------------------- engines
(def quickjs-driver
  "// Written by conformance.bb. Evaluates one case as a global script with
// std.evalScript and prints its outcome after the job queue has drained.
// The driver keeps everything it uses in this function's scope, captured
// before the case runs, so the case's globals (print, Error ...) cannot
// collide with it.
(function () {
  const [path, mode] = scriptArgs.slice(1);
  const { loadFile, evalScript } = std, setTimeout = os.setTimeout, out = print;
  const is = Object.is, ErrorType = Error, string = String;
  function canonical(v) {
    if (v === undefined) return 'undefined';
    if (v === null) return 'null';
    switch (typeof v) {
      case 'number': return 'number:' + (is(v, -0) ? '-0' : string(v));
      case 'string': { const u = []; for (let i = 0; i < v.length; i++) u.push(v.charCodeAt(i)); return 'utf16:[' + u.join(',') + ']'; }
      case 'boolean': return 'boolean:' + v;
      case 'bigint': return 'bigint:' + v;
      case 'symbol': return 'symbol';
      default: return 'object:' + typeof v;
    }
  }
  function thrown(e) {
    try { if (e instanceof ErrorType) return 'error:' + string(e.name).replace(/[^\\w]/g, ''); } catch (_) {}
    return canonical(e);
  }
  let line;
  try {
    let text = loadFile(path);
    // `void 0` resets the completion value the directive itself would give.
    if (mode === 'strict') text = '\"use strict\"; void 0;' + text;
    line = '{:status :returned :value \"' + canonical(evalScript(text)) + '\"}';
  } catch (e) {
    line = '{:status :thrown :value \"' + thrown(e) + '\"}';
  }
  setTimeout(() => { out(line); }, 0);
})();
")

(defn run-quickjs [{:keys [mode source]}]
  (if (= mode :module)
    (let [r (run-process [qjs "--std" "--experimental" "-m" source] :dir (fs/parent source))]
      (cond (:timeout r) {:status :timeout}
            (zero? (:exit r)) {:status :returned :value [:undefined]}
            :else {:status :thrown :value (error-name (first-line (:err r))) :detail (first-line (:err r))}))
    (let [r (run-process [qjs "--std" "--experimental" (str (fs/path out-dir "quickjs-driver.js")) source (name mode)])]
      (if (:timeout r) {:status :timeout}
          (let [line (last (filter #(str/starts-with? % "{:status") (str/split-lines (:out r))))
                outcome (some-> line edn/read-string)]
            ;; qjs reports an unhandled rejection and exits before the driver's
            ;; timer prints the outcome line, so the report comes first.
            (cond (re-find #"(?i)unhandled promise rejection" (:err r)) {:status :rejected :detail (first-line (:err r))}
                  (nil? outcome) {:status :error :detail (first-line (str (:err r) (:out r)))}
                  ;; A job or timer threw after the script completed.
                  (not (zero? (:exit r))) {:status :thrown :value (error-name (first-line (:err r))) :detail (str "after " line ": " (first-line (:err r)))}
                  :else {:status (:status outcome) :value (canonical-value (:value outcome))}))))))

(defn run-pants [{:keys [mode source]}]
  (let [r (run-process (cond-> [pants "--host-bindings"] (= mode :sloppy) (conj "--sloppy")
                         (= mode :module) (conj "--module") true (conj source)))]
    (cond (:timeout r) {:status :timeout}
          (= 2 (:exit r)) {:status :error :detail (str "compile: " (first-line (:err r)))}
          (str/includes? (:err r) "Possibly unhandled promise rejection")
          {:status :rejected :detail (first-line (:err r))}
          (= 1 (:exit r))
          (let [message (first (str/split-lines (:err r)))
                [_ body] (re-find #"^.*?: (.*)$" message)
                body (or body message)
                thrown (if-let [[_ v] (re-find #"^Thrown: uncaught exception: (.*?) \(in " body)] v body)]
            {:status :thrown :value (error-name thrown) :detail (first-line message)})
          (zero? (:exit r))
          (if-let [[_ v] (some #(re-find #" completed with (.*)$" %) (reverse (str/split-lines (:out r))))]
            {:status :returned :value (pants-value v)}
            {:status :error :detail "no completion line"})
          :else {:status :error :detail (str "exit " (:exit r) ": " (first-line (:err r)))})))

;; Opcode number -> the registry's `Opcode::name()`, the name the exporter's
;; refusals use, read from rust/src/vm/instr.rs.
(def opcode-names
  (let [text (slurp (str (fs/path rust "src/vm/instr.rs")))
        names (into {} (map (comp vec rest) (re-seq #"(?m)^\s+([A-Z]\w*) => \"([a-z_0-9]+)\"," text)))]
    (into {} (for [[_ n hex] (re-seq #"(?m)^\s+([A-Z]\w*) = 0x([0-9A-Fa-f]+)," text)]
               [(Long/parseLong hex 16) (names n n)]))))

(defn run-goose [{:keys [id mode source]} runner]
  (let [image (str (fs/path out-dir "images" (str (str/replace id "/" "__") ".gsp")))
        e (run-process (cond-> [exporter] (= mode :sloppy) (conj "--sloppy") (= mode :module) (conj "--module")
                         true (into [source image])))
        line (first-line (:out e))]
    (cond (:timeout e) {:status :timeout :detail "export"}
          (= 3 (:exit e)) {:status :unsupported :stage :export
                           :fault (if-let [[_ op] (re-find #"unsupported operation (\w+)" line)]
                                    (str "opcode " op)
                                    (str/replace (subs line 12) #"^\d+:\d+: SyntaxError: " ""))
                           :detail line}
          (= 2 (:exit e)) {:status :syntax :detail line}
          (not= 0 (:exit e)) {:status :infrastructure :detail (str "export: " line (first-line (:err e)))}
          :else
          (let [r (run-process (cond-> [runner image] (:heap-stats options) (conj "heap")))
                stats (when (:err r) (vec (filter #(str/starts-with? % "goose stack stats") (str/split-lines (:err r)))))
                heap (some->> (str/split-lines (or (:out r) "")) (filter #(str/starts-with? % "{:heap ")) first edn/read-string :heap)
                outcome (try (edn/read-string (str/trim (or (:out r) ""))) (catch Exception _ nil))
                fault (some-> (:fault outcome) (str/replace #"^Fault\." ""))]
            (cond-> (cond (:timeout r) {:status :timeout}
                          (not (map? outcome)) {:status :infrastructure :detail (str "runner exit " (:exit r) ": " (first-line (str (:out r) (:err r))))}
                          (#{:returned :thrown} (:status outcome)) {:status (:status outcome) :value (canonical-value (:value outcome))}
                          ;; A rejection nothing handled (light-cljs-790), scored as the
                          ;; oracles' :rejected is: by status alone.
                          (= :rejected (:status outcome)) {:status :rejected :detail (str "rejected with " (:value outcome))}
                          (= :unsupported (:status outcome))
                          {:status :unsupported :stage :execute
                           ;; An exported opcode the Goose VM does not implement.
                           :fault (if-let [operation (:operation outcome)]
                                    (str "explicit-unsupported " operation)
                                    (str fault (when-let [op (:opcode outcome)] (str " at opcode " (opcode-names op op)))))
                           :detail (pr-str (dissoc outcome :status))}
                          (= :limit (:status outcome)) {:status :limit :fault fault}
                          :else {:status :infrastructure :detail (:reason outcome)})
              (seq stats) (assoc :stack-stats stats)
              heap (assoc :heap heap))))))

;; ---------------------------------------------------------------- scoring
(defn thrown-values-agree?
  "Thrown values are equal canonical values, except that an error object one
  side names matches an object the other cannot name (Goose's canonical form
  gives objects no name), and a primitive the Pants engine shows only as text
  matches a string with that text. A value nobody could read never agrees."
  [a b]
  (let [named-object? (fn [x y] (and (= :error (first x)) (= [:object] y)))
        text-string? (fn [x y] (and (= :text (first x)) (= :string (first y)) (= (utf16 (second x)) (second y))))]
    (and (some? a) (some? b)
         (or (= a b) (named-object? a b) (named-object? b a) (text-string? a b) (text-string? b a)))))
(defn outcomes-agree? [a b]
  (and (= (:status a) (:status b))
       (case (:status a)
         :returned (= (:value a) (:value b))
         :thrown (thrown-values-agree? (:value a) (:value b))
         :rejected true
         false)))
(defn show [o] (str (name (:status o)) (when-let [v (:value o)] (str " " (pr-str v))) (when-let [d (:detail o)] (str " (" d ")"))))

(defn score [c g pa qj]
  (let [timeouts (keep (fn [[engine o]] (when (= :timeout (:status o)) engine)) [["goose" g] ["pants" pa] ["quickjs" qj]])]
    (cond
      (seq timeouts) {:result :fail :reason (str "timeout after " (:timeout options) " s on " (str/join ", " timeouts))}
      (and (not (:pants-gap c)) (not (outcomes-agree? pa qj)))
      {:result :disagreement :reason (str "pants " (show pa) "; quickjs " (show qj))}
      (= :unsupported (:status g)) {:result :unsupported :reason (str (name (:stage g)) ": " (:fault g))}
      ;; QuickJS's value is canonical; the Pants engine's agrees with it.
      (outcomes-agree? qj g) {:result :pass}
      :else {:result :fail :reason (str "expected " (show qj) "; goose " (show g) (when (:fault g) (str " " (:fault g))))})))

;; The scoring rules, checked on every run.
(let [ret (fn [v] {:status :returned :value v}) thr (fn [v] {:status :thrown :value v}) n number-value]
  (assert (outcomes-agree? (thr (n "1")) (thr (n "1"))))
  (assert (not (outcomes-agree? (thr (n "7")) (thr (n "9")))))
  (assert (not (outcomes-agree? (thr [:error "TypeError"]) (thr (n "7")))))
  (assert (not (outcomes-agree? (thr [:error "RangeError"]) (thr [:error "TypeError"]))))
  (assert (outcomes-agree? (thr [:error "RangeError"]) (thr [:object])))
  (assert (outcomes-agree? (thr [:text "é"]) (thr [:string (utf16 "é")])))
  (assert (not (outcomes-agree? (thr [:text "7"]) (thr [:string (utf16 "8")]))))
  (assert (not (outcomes-agree? (thr nil) (thr nil))))
  (assert (not (outcomes-agree? (ret (n "0")) (ret (n "-0")))))
  (assert (outcomes-agree? (ret (n "NaN")) (ret (n "NaN"))))
  (assert (= :fail (:result (score nil {:status :timeout} (ret [:undefined]) (ret [:undefined])))))
  (assert (= :fail (:result (score nil (ret [:undefined]) {:status :timeout} {:status :timeout}))))
  (assert (= :fail (:result (score nil (thr (n "9")) (thr (n "7")) (thr (n "7"))))))
  (assert (= :disagreement (:result (score nil (ret (n "1")) (ret (n "1")) (ret (n "2"))))))
  (assert (= :pass (:result (score {:pants-gap "r"} (ret (n "2")) (thr [:error "TypeError"]) (ret (n "2"))))))
  (assert (= :fail (:result (score {:pants-gap "r"} (ret (n "1")) (ret (n "1")) (ret (n "2"))))))
  (assert (= :pass (:result (score nil (ret (n "27.5")) (ret (n "27.5")) (ret (n "27.5")))))))

(defn run-case [c runner]
  (let [source (if (:text c)
                 (let [f (fs/path out-dir "cases" (str (str/replace (:id c) "/" "__") ".js"))] (spit (str f) (:text c)) (str f))
                 (:path c))
        c (assoc c :source source)
        g (run-goose c runner) pa (run-pants c) qj (run-quickjs c)]
    (merge {:case (:id c) :area (:area c) :origin (:origin c) :mode (:mode c)
            :goose (dissoc g :stack-stats :heap) :pants pa :quickjs qj}
           (when (:label c) {:label (:label c)})
           (when (:pants-gap c) {:pants-gap (:pants-gap c)})
           (when (:stack-stats g) {:stack-stats (:stack-stats g)})
           (when (:heap g) {:heap (:heap g)})
           (score c g pa qj))))

;; ---------------------------------------------------------------- main
(when (:list options)
  (doseq [c (inventory) :let [m (or (get-in areas [:cases (:id c)]) (get-in areas [:excluded (:id c)]))]]
    (println (:id c) (cond (nil? m) "UNMAPPED" (:reason m) "excluded" :else (name (:area m)))
             (some-> (or (:origin m) (:origin c)) name) (some-> (or (:mode m) (:mode c)) name)))
  (System/exit 0))

(def cases
  (let [all (labelled (inventory))
        selected (cond->> all
                   (:area options) (filter #(= (:area options) (:area %)))
                   (seq (:cases options)) (filter #((:cases options) (:id %))))]
    (when (and (:area options) (not (area-names (:area options)))) (fail! "unknown area " (name (:area options))))
    (when-let [missing (seq (remove (set (map :id all)) (:cases options)))] (fail! "unknown case " (str/join ", " missing)))
    (when (and (:write-expected options) (or (:area options) (seq (:cases options)))) (fail! "--write-expected needs the whole inventory"))
    (when (:check options)
      (let [runnable (set (map :id (remove :excluded all)))
            listed (edn/read-string (slurp (str (fs/path conformance "expected-pass.edn"))))]
        (when-not (and (vector? listed) (every? string? listed)) (fail! "expected-pass.edn must be a vector of case IDs"))
        (when-let [bad (seq (remove runnable listed))]
          (fail! "expected-pass.edn names cases that are unknown or excluded: " (str/join ", " bad)))))
    (vec selected)))

(defn shell! [& command] (println "+" (str/join " " command)) (flush) (apply p/shell {:dir (str repository)} command))
(def runner (str (fs/path out-dir (if (:stack-stats options) "goose-runner-stack-stats" "goose-runner"))))
(fs/create-dirs (fs/path out-dir "images")) (fs/create-dirs (fs/path out-dir "cases"))
(spit (str (fs/path out-dir "quickjs-driver.js")) quickjs-driver)
(when-not (:no-build options)
  (shell! "cargo" "build" "--locked" "--release" "-p" "big-boy-pants-js" "--bin" "big-boy-pants-js" "--example" "goose_export")
  (let [program (str (fs/path root "src/conformance.goose"))]
    (shell! goose "format" "--check" program)
    (apply shell! goose "build" program (concat (when (:stack-stats options) ["-DGS_STACK_STATS=1"]) ["-o" runner])))
  (when-not (fs/exists? qjs)
    (println "+ make qjs in" (str quickjs-dir)) (flush)
    (p/shell {:dir (str quickjs-dir)} "make" "qjs")))
(doseq [tool [qjs pants exporter runner]] (when-not (fs/exists? tool) (fail! "missing tool " tool " (run without --no-build)")))

(def rows
  (let [pool (Executors/newFixedThreadPool (:jobs options))
        runnable (remove :excluded cases)
        started (System/nanoTime)
        futures (mapv (fn [c] (.submit pool ^Callable (fn [] (run-case c runner)))) runnable)
        results (mapv #(.get %) futures)]
    (.shutdown pool) (.awaitTermination pool 1 TimeUnit/MINUTES)
    (println (format "ran %d cases on three engines in %.1f s with %d jobs" (count runnable) (/ (- (System/nanoTime) started) 1e9) (:jobs options)))
    (let [by-id (into {} (map (juxt :case identity) results))]
      (vec (for [c cases]
             (or (by-id (:id c))
                 (cond-> {:case (:id c) :area (:area c) :origin (:origin c) :result :excluded :reason (:excluded c)}
                   (:label c) (assoc :label (:label c)))))))))

(fs/create-dirs out-dir)
(spit (str (fs/path out-dir "ledger.edn"))
      (with-out-str (println ";; Generated by crates/big-boy-pants-js/scripts/conformance.bb; one row per case.")
        (pprint/pprint (mapv #(select-keys % [:case :area :origin :mode :label :pants-gap :result :reason :goose :pants :quickjs :stack-stats :heap]) rows))))

(def columns [:pass :fail :unsupported :disagreement :excluded])
(defn table [title key-fn]
  (let [groups (group-by key-fn rows)
        width (max 24 (inc (apply max (map (comp count str) (keys groups)))))]
    (println)
    (println (apply str (format (str "%-" width "s") title) (map #(format "%13s" (name %)) (conj columns :total))))
    (doseq [[k group] (sort-by (comp str key) groups)
            :let [counts (frequencies (map :result group))]]
      (println (apply str (format (str "%-" width "s") (if (keyword? k) (name k) (str k)))
                      (concat (map #(format "%13d" (get counts % 0)) columns) [(format "%13d" (count group))]))))
    (let [counts (frequencies (map :result rows))]
      (println (apply str (format (str "%-" width "s") "all") (concat (map #(format "%13d" (get counts % 0)) columns) [(format "%13d" (count rows))]))))))
(table "area" #(or (:area %) "(no area: host-specific or not applicable)"))
(table "origin" :origin)
(println)
(println "ledger:" (str (fs/path out-dir "ledger.edn")))

(when (:stack-stats options)
  (println "\nGoose runner stack report (distinct lines over the run, with counts):")
  (doseq [[line n] (sort-by key (frequencies (mapcat :stack-stats rows)))] (println (format "%5d  %s" n line))))

(when (:heap-stats options)
  (let [heaps (keep :heap rows)
        quantile (fn [xs q] (let [v (vec (sort xs))] (if (empty? v) 0 (v (min (dec (count v)) (long (* q (count v))))))))]
    (println (format "\nGoose job heap at the end of %d runs (median / 90th percentile / largest; dead = no root reaches it):" (count heaps)))
    (doseq [k [:nodes :values :properties :units]
            :let [dead (keyword (str "dead-" (name k))) xs (map k heaps) ds (map dead heaps)]]
      (println (format "  %-10s rows %d / %d / %d   dead %d / %d / %d   dead share of all rows %.1f%%"
                       (name k) (quantile xs 0.5) (quantile xs 0.9) (apply max 0 xs)
                       (quantile ds 0.5) (quantile ds 0.9) (apply max 0 ds)
                       (if (pos? (reduce + xs)) (* 100.0 (/ (reduce + ds) (reduce + xs))) 0.0))))))

(def expected-file (str (fs/path conformance "expected-pass.edn")))
(def passes (set (map :case (filter #(= :pass (:result %)) rows))))
(when (:write-expected options)
  (spit expected-file (str ";; Cases that must pass on the Goose VM: `conformance.bb --check` fails when one\n"
                           ";; does not. Area pellets add the cases their acceptance requires.\n"
                           (with-out-str (pprint/pprint (vec (sort passes))))))
  (println "wrote" expected-file (count passes) "cases"))
(when (:check options)
  (let [selected (set (map :id cases))
        expected (set (filter selected (edn/read-string (slurp expected-file))))
        regressions (sort (remove passes expected))
        new-passes (sort (remove expected passes))]
    (doseq [id new-passes] (println "new pass (not in expected-pass.edn):" id))
    (doseq [id regressions] (println "REGRESSION:" id "-" (:reason (first (filter #(= id (:case %)) rows)))))
    (println (format "check: %d expected passes, %d regressions, %d new passes" (count expected) (count regressions) (count new-passes)))
    (when (seq regressions) (System/exit 1))))
