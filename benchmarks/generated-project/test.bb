#!/usr/bin/env bb
;; Pure generator tests by default. Optional end-to-end compiler comparisons:
;; bb benchmarks/generated-project/test.bb --light target/release/light-cljs --shadow
(ns generated-project.test
  (:require [babashka.fs :as fs]
            [babashka.process :as process]
            [clojure.edn :as edn]
            [clojure.string :as str]
            [clojure.test :refer [deftest is testing run-tests]]
            [edamame.core :as edamame]))

(def kit (fs/parent (fs/absolutize *file*)))
(load-file (str (fs/path kit "generate.bb")))
(load-file (str (fs/path kit "oracle.bb")))
(load-file (str (fs/path kit "check-waste.bb")))
(load-file (str (fs/path kit "compare.bb")))
(alias 'gen 'generated-project.generate)
(alias 'oracle 'generated-project.oracle)
(alias 'waste 'generated-project.check-waste)
(alias 'compare 'generated-project.compare)

(def options
  (loop [[arg & more] *command-line-args* result {}]
    (case arg
      nil result
      "--light" (if-let [path (first more)]
                  (recur (rest more) (assoc result :light (str (fs/absolutize path))))
                  (throw (ex-info "--light needs a binary path" {})))
      "--shadow" (recur more (assoc result :shadow true))
      (throw (ex-info "Usage: bb test.bb [--light BINARY] [--shadow]" {:argument arg})))))

(def fixture-options (assoc gen/defaults :seed 42))
(def fixture (delay (gen/project fixture-options)))
(def loaded-expectations #{:removable :best-effort :retained})

(deftest deterministic-and-sensitive-to-seed
  (is (= (map :source (:files @fixture)) (map :source (:files (gen/project fixture-options)))))
  (is (= (:model @fixture) (:model (gen/project fixture-options))))
  (let [other (gen/project (assoc fixture-options :seed -7))]
    (is (not= (:model @fixture) (:model other)))
    (is (not= (map :source (:files @fixture)) (map :source (:files other)))))
  (let [model (:model @fixture)]
    ;; Frozen; cross-checked with light none/prepare/advanced and shadow dev/release.
    (is (= 305050 (oracle/expected model 42 3)))
    (is (> (count (set (map #(oracle/expected model % 1) [0 1 42 1000002]))) 1))))

(defn ns-aliases [source]
  (let [ns-form (edamame/parse-string source {:all true})]
    (into {} (for [clause (rest ns-form) :when (and (seq? clause) (= :require (first clause)))
                   [lib _ alias] (rest clause)]
               [alias lib]))))

(defn read-all [source]
  (let [aliases (ns-aliases source) current (second (edamame/parse-string source {:all true}))]
    (edamame/parse-string-all source {:all true :auto-resolve #(if (= :current %) current (aliases %))})))

(deftest useful-size-variety-and-bounded-graph
  (let [{:keys [files model]} @fixture
        sources (map :source (remove #(= :shim (:role %)) files))
        metrics (gen/source-metrics sources)
        workers (:workers model)]
    (is (<= 5000 (:code-lines metrics) 5600))
    ;; The old fixed-template generator produced 244 distinct lines at 200k.
    (is (>= (:distinct-line-ratio metrics) 0.6))
    (is (every? #(>= (count (:kernels %)) 2) workers))
    (testing "every expression node evaluates to a value in the model's range"
      (binding [oracle/*model* model]
        (doseq [{:keys [kernels]} workers k kernels input [0 1000002]]
          (is (< -1 (oracle/call-fn k [input] {}) gen/modulus)))))
    ;; Every canonical source is readable, including macros, auto-resolved keywords and waste.
    (doseq [{:keys [path source]} files]
      (testing path (is (pos? (count (read-all source))))))))

(deftest every-operation-is-live-at-moderate-size
  (let [model (:model (gen/project (assoc gen/defaults :seed 7 :lines 30000)))
        live (set (keys (gen/op-counts (gen/live-trees model))))
        ops (concat (mapcat #(map first (val %)) gen/tables)
                    [:lit :local :dyn :lambda :lambda2 :fref :partial :comp :juxtsum])]
    (is (empty? (remove live ops)))))

(deftest dependency-graph-is-shallow
  (let [p (gen/project (assoc gen/defaults :seed 3 :lines 20000))
        files (filter #(= :worker (:role %)) (:files p))
        index (into {} (map-indexed (fn [i f] [(:ns f) i]) files))]
    (doseq [{:keys [ns source]} files
            :let [i (index ns) deps (keep index (map (comp str first) (rest (first (filter seq? (rest (edamame/parse-string source {:all true})))))))]]
      (is (<= (count deps) 3) ns)
      (is (every? #(< % (quot (inc i) 2)) deps) ns))))

(deftest waste-markers-are-unique-and-classified
  (let [{:keys [files markers]} @fixture
        text (str/join "\n" (map :source files))]
    (is (apply distinct? (map :marker markers)))
    (is (= #{:removable :best-effort :retained :compile-time :never-loaded} (set (map :expect markers))))
    (is (= #{:helper :kernel :record :chain :waste-only :common :idle :arity :protocol-impl
             :extension :defmethod :macro :orphan}
           (set (map :category markers))))
    (doseq [{:keys [marker]} markers]
      (is (= 1 (count (re-seq (re-pattern marker) text))) marker))))

(deftest rejects-invalid-arguments
  (doseq [args [[] ["out" "--seed"] ["out" "--lines" "wat"]
                ["out" "--lines" "0"] ["out" "--namespace-lines" "10"]
                ["out" "--unknown"] ["out" "another"]
                ["out" "--seed" "9223372036854775808"]]]
    (is (thrown? Exception (gen/parse-options args)) (pr-str args)))
  (is (= 800000 (:lines (gen/parse-options ["out" "--lines" "800k"]))))
  (doseq [[input iterations] [[-1 1] [1000003 1] [0 0] [nil 1] [1 1.5]]]
    (is (thrown? Exception (oracle/expected (:model @fixture) input iterations)))))

(defn with-project [opts f]
  (let [dir (fs/create-temp-dir {:prefix "light-generated-test-"})]
    (try
      (let [manifest (gen/generate! (assoc opts :output (str dir)))]
        (f dir manifest))
      (finally (fs/delete-tree dir)))))

(defn command [dir & args]
  (let [result (apply process/sh {:dir (str dir)} args)]
    (when-not (zero? (:exit result))
      (throw (ex-info "Command failed" (assoc result :command args :directory (str dir)))))
    result))

(defn file-contents [dir]
  (into (sorted-map)
        (for [file (fs/glob dir "**") :when (fs/regular-file? file)]
          [(str (fs/relativize dir file)) (slurp (str file))])))

(defn markers-with [manifest pred] (set (filter #(pred (:expect %)) (:waste-markers manifest))))

(deftest standalone-project-and-waste-checker
  (with-project
    fixture-options
    (fn [dir manifest]
      (is (thrown? Exception (gen/generate! (assoc fixture-options :output (str dir)))))
      (is (= "305050\n" (:out (command dir "bb" "oracle.bb" "42" "3"))))
      (with-project fixture-options
        (fn [other _] (is (= (file-contents dir) (file-contents other)))))
      (is (every? (fn [{:keys [path sha256]}]
                    (= sha256 (gen/sha256 (slurp (fs/file dir path))))) (:files manifest)))
      (let [loaded (markers-with manifest loaded-expectations)
            kept (markers-with manifest #{:best-effort :retained})
            out (fs/path dir "fake-output")]
        (fs/create-dirs out)
        (spit (fs/file out "app.js") (str/join "\n" (map :marker loaded)))
        ;; Source maps can legally contain eliminated code; don't scan them.
        (spit (fs/file out "app.js.map") (str/join "\n" (map :marker (:waste-markers manifest))))
        (is (= loaded (set (:survivors (waste/scan manifest out)))))
        (is (= 1 (:exit (process/sh {:dir (str dir)} "bb" "check-waste.bb" "--strict" (str out)))))
        (is (zero? (:exit (process/sh {:dir (str dir)} "bb" "check-waste.bb" (str out)))))
        ;; Best-effort and retained survivors do not fail --strict.
        (spit (fs/file out "app.js") (str/join "\n" (map :marker kept)))
        (command dir "bb" "check-waste.bb" "--strict" (str out))
        (spit (fs/file out "app.js") "console.log(123);")
        (is (empty? (:survivors (waste/scan manifest out))))
        (spit (fs/file out "app.js") (:marker (first (markers-with manifest #{:never-loaded}))))
        (is (= 1 (:exit (process/sh {:dir (str dir)} "bb" "check-waste.bb" (str out))))))
      (let [light (edn/read-string (slurp (fs/file dir "light-cljs.edn")))
            shadow (edn/read-string (slurp (fs/file dir "deps.edn")))]
        (is (= ["src" "macros"] (:source-paths light)))
        (is (= ["src" "macros" "shadow-support"] (:paths shadow)))))))

(def leaf-original
  "Bytes a text round trip would not reproduce: invalid UTF-8, CRLF, no final newline."
  (byte-array (concat (.getBytes "(ns leaf)\r\n(def x \"" "UTF-8") [-1 -2 -128] (.getBytes "\")" "UTF-8"))))

(defn leaf-bytes [dir] (vec (fs/read-all-bytes (fs/file dir "leaf.cljs"))))

(deftest leaf-scenario-restores-source-after-failure
  (let [dir (str (fs/create-temp-dir {:prefix "light-leaf-test-"}))
        sample {:s 1.0 :user-s 1.0 :rss-gb 0.1}
        edited? #(str/includes? (slurp (fs/file dir "leaf.cljs")) "compare-leaf-edit-")]
    (try
      (fs/write-bytes (fs/file dir "leaf.cljs") leaf-original)
      (testing "a build that throws"
        (with-redefs [compare/timed (fn [_ _] (is (edited?)) (throw (ex-info "build broke" {})))]
          (is (thrown-with-msg? Exception #"build broke"
                                (compare/leaf-scenario dir 2 "leaf.cljs" ["build"] (constantly {})))))
        (is (= (vec leaf-original) (leaf-bytes dir))))
      (testing "builds that succeed, then a check that throws"
        (let [builds (atom 0)]
          (with-redefs [compare/timed (fn [_ _] (is (edited?)) (swap! builds inc) sample)]
            (is (thrown-with-msg? Exception #"check broke"
                                  (compare/leaf-scenario dir 2 "leaf.cljs" ["build"]
                                                         #(do (is (edited?)) (throw (ex-info "check broke" {})))))))
          (is (= 2 @builds)))
        (is (= (vec leaf-original) (leaf-bytes dir))))
      (testing "a run that succeeds"
        (with-redefs [compare/timed (constantly sample)]
          (is (= {:ok true} (:output (compare/leaf-scenario dir 1 "leaf.cljs" ["build"] (constantly {:ok true}))))))
        (is (= (vec leaf-original) (leaf-bytes dir))))
      (testing "a run terminated during its build"
        (let [script (str "(load-file " (pr-str (str (fs/path kit "compare.bb"))) ")"
                          "(with-redefs [generated-project.compare/timed"
                          " (fn [_ _] (println :edited) (flush) (Thread/sleep 60000))]"
                          " (generated-project.compare/leaf-scenario " (pr-str dir) " 1 \"leaf.cljs\" [] (constantly nil)))")
              proc (process/process {:err :inherit} "bb" "-e" script)]
          ;; The child prints once the edit is on disk, then blocks in the build.
          (is (= ":edited" (.readLine (java.io.BufferedReader. (java.io.InputStreamReader. (:out proc))))))
          (is (edited?))
          (process/destroy proc) ; SIGTERM
          (is (not (zero? (:exit @proc))))
          (is (= (vec leaf-original) (leaf-bytes dir)))))
      (finally (fs/delete-tree dir)))))

(defn check-results [dir model runner]
  (doseq [[input iterations] [[0 1] [1 1] [42 3] [1000002 2]]]
    (is (= (str (oracle/expected model input iterations) "\n")
           (:out (apply command dir (concat ["node" runner] (map str [input iterations])))))
        (str runner " input=" input " iterations=" iterations))))

(defn check-waste-at [manifest path optimized?]
  (let [survivors (set (:survivors (waste/scan manifest path)))]
    (is (empty? (filter #(#{:never-loaded :compile-time} (:expect %)) survivors)) (str path))
    (if optimized?
      (is (empty? (filter #(= :removable (:expect %)) survivors)) (str path))
      (is (= (markers-with manifest loaded-expectations) survivors) (str path)))))

(deftest compiler-comparisons
  (when (or (:light options) (:shadow options))
    (doseq [opts [fixture-options (assoc gen/defaults :seed -7 :lines 1000)]]
      (with-project
        opts
        (fn [dir manifest]
          (let [model (edn/read-string (slurp (fs/file dir "model.edn")))]
            (when-let [binary (:light options)]
              (doseq [level ["none" "prepare" "advanced"]]
                (println "light" level (:seed opts)) (flush)
                (command dir binary "release" "bench" "--optimize" level)
                (check-results dir model "run-light.mjs")
                (when (not= level "prepare")
                  (check-waste-at manifest (fs/path dir "out/light/workload.main.js") (= level "advanced")))))
            (when (:shadow options)
              (doseq [mode ["compile" "release"]]
                (println "shadow" mode (:seed opts)) (flush)
                (let [{:keys [out err]} (command dir "clojure" "-M:shadow" mode "bench")]
                  (is (not (re-find #"WARNING #" (str out err))) (str "shadow " mode " warnings")))
                (check-results dir model "out/shadow.js")
                (check-waste-at manifest (fs/path dir (if (= mode "compile") "out/shadow-dev" "out/shadow.js"))
                                (= mode "release"))))))))))

(deftest macro-comparison-options-and-watchdog
  (let [opts (compare/parse-options ["fixture" "--light" "compiler" "--backend" "goose"
                                     "--runner" "runner" "--workers" "1,2,4,8" "--phase" "check"])]
    (is (= [1 2 4 8] (:workers opts)))
    (is (= "goose" (:backend opts))))
  (let [environment (into {} (System/getenv))
        result (compare/bounded-run "." environment 3000 ["sh" "-c" "printf expected; exit 7"])]
    (is (= 7 (:exit result)))
    (is (= "expected" (:out result))))
  (let [marker (fs/create-temp-file {:prefix "generated-watchdog-"})
        environment (assoc (into {} (System/getenv)) "WATCHDOG_CHILD_PID" (str marker))]
    (try
      (is (thrown-with-msg? clojure.lang.ExceptionInfo #"watchdog expired"
                            (compare/bounded-run "." environment 200
                              ["sh" "-c" "sleep 60 & echo $! > \"$WATCHDOG_CHILD_PID\"; wait"])))
      (let [pid (parse-long (str/trim (slurp (str marker))))
            child (java.lang.ProcessHandle/of pid)]
        (is (or (not (.isPresent child)) (not (.isAlive (.get child))))))
      (finally (fs/delete-if-exists marker)))))

(deftest independent-compiler-slots-and-native-capacity
  (let [opts (compare/parse-options ["fixture" "--light" "compiler" "--backend" "goose"
                                     "--runner" "runner" "--workers" "4,8" "--compiler-slots" "4,8"])
        cells (compare/comparison-cells opts)]
    (is (= [[4 4] [4 8] [8 4] [8 8]] (mapv (juxt compare/cell-slots :workers) cells)))
    (is (= 4 (count (distinct (map compare/cell-name cells)))))
    (doseq [cell cells]
      (is (= [(:light opts) "release" "bench" "--optimize" "advanced" "--jobs"
              (str (:compiler-slots cell)) "--no-cache"]
             (compare/macro-command (:light opts) (:dir opts) "measure" cell))))
    (doseq [slots [[] [0] [-1] [nil] [4 4]]]
      (is (thrown-with-msg? AssertionError #"distinct positive integers"
                           (compare/macro-main (assoc opts :compiler-slots slots))))))
  (testing "existing coupled cells retain their names and compiler limits"
    (let [cells (compare/comparison-cells {:backend "all" :workers [1 2 4 8]})]
      (is (= ["v81" "goose1" "goose2" "goose4" "goose8"] (mapv compare/cell-name cells)))
      (is (= [1 1 2 4 8] (mapv compare/cell-slots cells)))))
  (testing "V8 compiler slots vary independently of its single execution worker"
    (is (= [[4 1] [8 1]]
           (mapv (juxt compare/cell-slots :workers)
                 (compare/comparison-cells {:backend "v8" :compiler-slots [4 8]}))))))

(deftest extra-labelled-binaries-share-rounds
  (let [opts (compare/parse-options ["fixture" "--light" "compiler" "--backend" "all" "--runner" "runner"
                                     "--workers" "1,4" "--also-light" "before=old"
                                     "--also-light" "novee=nov8,goose"])
        cells (compare/comparison-cells opts)
        old (str (fs/absolutize "old"))]
    (is (= ["v81" "before-v81" "goose1" "before-goose1" "novee-goose1"
            "goose4" "before-goose4" "novee-goose4"]
           (mapv compare/cell-name cells)))
    (is (= [old "release" "bench" "--optimize" "advanced" "--jobs" "4" "--no-cache"]
           (compare/macro-command (or (:light (nth cells 6)) (:light opts)) (:dir opts) "measure" (nth cells 6))))
    (is (= [4 4] ((juxt compare/cell-slots :workers) (nth cells 7))))
    (is (= ["before-goose-slots8-workers4" "novee-goose-slots8-workers4"]
           (mapv compare/cell-name (rest (compare/comparison-cells (assoc opts :backend "goose" :workers [4] :compiler-slots [8]))))))
    (doseq [bad ["" "Upper=x" "label"]]
      (is (thrown-with-msg? AssertionError #"LABEL=BINARY" (compare/parse-also-light bad))))
    (is (thrown-with-msg? AssertionError #"distinct"
                         (compare/macro-main (update opts :also-light conj {:label "before" :light old}))))
    (is (thrown-with-msg? AssertionError #"cache/leaf"
                         (compare/macro-main (assoc opts :backend "v8" :phase "cached"))))))

(deftest quiet-gate-waits-and-keeps-rejections
  (let [readings (atom [88.5 89.9 95.0])]
    (with-redefs [compare/quiet-once! (fn [_] (let [idle (first @readings)]
                                                (swap! readings rest)
                                                (assert (>= idle 90) (str "Quiet gate: idle=" idle))
                                                {:idle-percent idle}))
                  compare/sleep-between-gates! (fn [])]
      (is (= {:idle-percent 95.0 :rejected-idle-percent [88.5 89.9]} (compare/quiet! {})))
      (reset! readings (repeat 13 50.0))
      (is (thrown-with-msg? AssertionError #"idle=50.0" (compare/quiet! {}))))))

(deftest process-tree-sampling-excludes-unrelated-processes
  (let [text " 10 1 20 /bin/time\n 11 10 100 compiler\n 12 11 50 native\n 13 1 999 unrelated\n"]
    (is (= [10 11 12] (mapv :pid (compare/process-tree 10 text))))
    (is (= 170 (reduce + (map :rss-kib (compare/process-tree 10 text)))))))

(deftest macro-comparison-checks-complete-output-and-exact-stdout
  (let [dir (fs/create-temp-dir {:prefix "generated-output-"})
        output (fs/path dir "out/light/workload.main.js")
        reference (fs/path dir "reference.js")
        expected (vec (repeat 5 "42\n"))]
    (try
      (fs/create-dirs (fs/parent output))
      (spit (str output) "original JS")
      (with-redefs [compare/checked (fn [_ _ cmd] (if (= "node" (first cmd)) "42\n" "waste ok"))]
        (is (= 11 (:bytes (compare/verify-macro-output (str dir) {} expected reference))))
        (spit (str output) "different JS with the same checksum")
        (is (thrown-with-msg? AssertionError #"Complete emitted JS differs"
                             (compare/verify-macro-output (str dir) {} expected reference))))
      (with-redefs [compare/checked (fn [_ _ _] "42")]
        (is (thrown-with-msg? AssertionError #"Oracle mismatch"
                             (compare/verify-macro-output (str dir) {} expected reference))))
      (let [counts (fs/path dir "counts.tsv")]
        (spit (str counts) "expanded/workload.macros/m\t2\t30\nbootstrap/setup\t1\t40\n")
        (is (= {"expanded/workload.macros/m" 2}
               (compare/verify-macro-counts (str dir) "v8" counts)))
        (is (= {"expanded/workload.macros/m" 2}
               (compare/verify-macro-counts (str dir) "goose" counts)))
        (spit (str counts) "expanded/workload.macros/m\t1\t30\n")
        (is (thrown-with-msg? AssertionError #"identities/counts differ"
                             (compare/verify-macro-counts (str dir) "goose" counts))))
      (let [native (fs/path dir "native.txt")]
        (spit (str native) "POOL 0 0 5 5 256 1 4 4 10 64 64 64 64\nTHREADS 0 4 4 3\nENGINES v8-platform=0 v8-isolates=0 quickjs-runtimes=0 pants-runtimes=0\nCLOSED 1\n")
        (is (= [4 3] ((juxt :native-thread-total :spawned-threads-joined)
                      (compare/goose-diagnostics native 1))))
        (spit (str native) "POOL 0 0 5 5 256 1 4 4 10 64 64 64 64\nTHREADS 0 4 4 2\nCLOSED 1\n")
        (is (thrown-with-msg? AssertionError #"spawned threads did not all join"
                             (compare/goose-diagnostics native 1)))
        (spit (str native) "POOL 0 0 5 5 256 1 4 4 10 64 64 64 64\nTHREADS 0 4 4 3\nENGINES v8-platform=1 v8-isolates=2 quickjs-runtimes=0 pants-runtimes=0\nCLOSED 1\n")
        (is (thrown-with-msg? AssertionError #"engine census is not all zero"
                             (compare/goose-diagnostics native 1))))
      (finally (fs/delete-tree dir)))))

(let [{:keys [fail error]} (run-tests 'generated-project.test)]
  (System/exit (if (zero? (+ fail error)) 0 1)))
