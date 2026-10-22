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
(alias 'gen 'generated-project.generate)
(alias 'oracle 'generated-project.oracle)
(alias 'waste 'generated-project.check-waste)

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

(let [{:keys [fail error]} (run-tests 'generated-project.test)]
  (System/exit (if (zero? (+ fail error)) 0 1)))
