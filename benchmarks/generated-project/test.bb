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

(defn gate-off
  "compare.bb options for `runs` rounds whose load gate never waits or repeats."
  [runs]
  {:runs runs :max-load 1e9 :load-wait 0 :max-repeats 0})

(deftest compare-options
  (let [options (compare/parse-options ["proj" "--light" "bin/light-cljs-abc1234"])]
    (is (= {:runs 5 :max-load 2.0 :load-wait 900 :max-repeats 3 :warm-jvm 0}
           (select-keys options [:runs :max-load :load-wait :max-repeats :warm-jvm])))
    (is (fs/absolute? (:dir options)))
    (is (fs/absolute? (:light options))))
  (is (= {:runs 3 :max-load 1.5 :shadow-cmd ["npx" "shadow-cljs"] :light-build "client" :light-only true}
         (select-keys (compare/parse-options ["p" "--light" "b" "--runs" "3" "--max-load" "1.5" "--light-only"
                                              "--shadow-cmd" " npx  shadow-cljs" "--light-build" "client"])
                      [:runs :max-load :shadow-cmd :light-build :light-only])))
  (doseq [args [["p"] ["--light" "b"] ["p" "--light" "b" "--runs" "0"] ["p" "--light" "b" "--max-load" "x"]
                ["p" "--light" "b" "--runs"] ["p" "q" "--light" "b"] ["p" "--light" "b" "--wat"]]]
    (is (thrown? Exception (compare/parse-options args)) (pr-str args))))

(deftest compare-statistics
  (is (= 2 (compare/median [3 1 2])))
  (is (= 2.5 (compare/median [4 1 3 2])))
  (let [s (compare/summary [{:wall-s 3.0 :user-s 2.0 :system-s 1.0 :peak-memory-gb 0.3 :load [1.0 0 0]}
                            {:wall-s 1.0 :user-s 4.0 :system-s 0.5 :peak-memory-gb 0.1 :load [1.5 0 0]}
                            {:wall-s 2.0 :user-s 3.0 :system-s 2.0 :peak-memory-gb 0.2 :load [0.5 0 0]}]
                           [])]
    (is (= 3 (:runs s)))
    (is (= {:median 2.0 :min 1.0 :max 3.0} (:wall-s s)))
    (is (= {:median 3.0 :min 2.0 :max 4.0} (:user-s s)))
    (is (= {:median 1.0 :min 0.5 :max 2.0} (:system-s s)))
    (is (= {:median 4.5 :min 3.0 :max 5.0} (:cpu-s s)))
    (is (= {:median 0.2 :min 0.1 :max 0.3} (:peak-memory-gb s)))
    (is (= {:median 1.0 :min 0.5 :max 1.5} (:load-1m s)))))

(deftest rounds-alternate-and-repeat-loaded-rounds
  (let [tools [{:tool :light :dir "l" :cmd ["light"]} {:tool :shadow :dir "s" :cmd ["shadow"]}]
        sample (fn [i] {:wall-s (double i) :user-s 1.0 :system-s 0.1 :peak-memory-gb 0.1})]
    (testing "the compilers alternate which goes first"
      (let [builds (atom []) prepared (atom [])
            result (with-redefs [compare/timed (fn [_ [cmd]] (swap! builds conj cmd) (sample (count @builds)))
                                 compare/load-average (constantly [0.5 0.5 0.5])]
                     (compare/rounds {:runs 3 :max-load 2.0 :load-wait 0 :max-repeats 3}
                                     #(swap! prepared conj %) tools))]
        (is (= ["light" "shadow" "shadow" "light" "light" "shadow"] @builds))
        (is (= [0 1 2] @prepared))
        (is (= [1.0 4.0 5.0] (map :wall-s (get-in result [:light :samples]))))
        (is (= [0 1 2] (map :round (get-in result [:shadow :samples]))))
        (is (= [0 1 0] (map :position (get-in result [:light :samples]))))
        (is (every? empty? (map :discarded (vals result))))))
    (testing "a round with a build that started above the threshold is repeated"
      (let [loads (atom [[0.5] [3.0] [0.5] [0.5] [0.5] [0.5]]) prepared (atom [])
            result (with-redefs [compare/timed (fn [_ _] (sample 1))
                                 compare/load-average #(let [[l & more] @loads] (reset! loads more) l)]
                     (compare/rounds {:runs 2 :max-load 2.0 :load-wait 0 :max-repeats 3}
                                     #(swap! prepared conj %) tools))]
        (is (= [0 1 2] @prepared) "each attempt gets a fresh number")
        (is (= [0 1] (map :round (get-in result [:light :samples]))))
        (is (= [[3.0]] (map :load (get-in result [:shadow :discarded]))))
        (is (= [[0.5]] (map :load (get-in result [:light :discarded]))))
        (is (not-any? :over-max-load (mapcat :samples (vals result))))))
    (testing "after max-repeats the last attempt is kept and marked"
      (let [result (with-redefs [compare/timed (fn [_ _] (sample 1))
                                 compare/load-average (constantly [9.0 9.0 9.0])]
                     (compare/rounds {:runs 1 :max-load 2.0 :load-wait 0 :max-repeats 2} (constantly nil) tools))]
        (is (= 2 (count (get-in result [:light :discarded]))))
        (is (= [true] (map :over-max-load (get-in result [:shadow :samples]))))))))

(deftest any-project-with-both-configurations
  (let [dir (fs/create-temp-dir {:prefix "light-compare-project-"})
        shadow-dir (fs/path dir "upstream")]
    (try
      (fs/create-dirs (fs/path shadow-dir "node_modules/shadow-cljs"))
      (spit (fs/file dir "light-cljs.edn")
            (str "{:source-paths [\"src\"] :builds {:client {:target :esm :output-dir \"out/dev\" :x #light-cljs/env [\"X\"]"
                 " :release {:output-dir \"out/js\"}}}}"))
      (spit (fs/file shadow-dir "shadow-cljs.edn")
            (str "{:cache-root \"target/shadow-cache\" :builds {:app {:target :browser :output-dir \"public/js\"}"
                 " :test {:target :node-test :output-to \"t.js\"}}}"))
      (is (= :client (compare/build-id (fs/file dir "light-cljs.edn") nil)))
      (is (= :app (compare/build-id (fs/file shadow-dir "shadow-cljs.edn") ":app")))
      (is (thrown? Exception (compare/build-id (fs/file shadow-dir "shadow-cljs.edn") nil)))
      (is (thrown? Exception (compare/build-id (fs/file shadow-dir "shadow-cljs.edn") "nope")))
      (is (= ["npx" "shadow-cljs"] (compare/default-shadow-cmd shadow-dir)))
      (spit (fs/file shadow-dir "deps.edn")
            "{:aliases {:shadow {:extra-deps {thheller/shadow-cljs {:mvn/version \"3.5.3\"}}}}}")
      (is (= ["npx" "shadow-cljs"] (compare/default-shadow-cmd shadow-dir)) "an alias without shadow's main")
      (spit (fs/file shadow-dir "deps.edn")
            "{:aliases {:shadow {:main-opts [\"-m\" \"shadow.cljs.devtools.cli\"]}}}")
      (is (= ["clojure" "-M:shadow"] (compare/default-shadow-cmd shadow-dir)))
      (with-redefs [compare/run! (fn [d & cmd]
                                   (is (= (str shadow-dir) (str d)))
                                   (is (= ["npx" "shadow-cljs" "clj-eval"] (take 3 cmd)))
                                   "SHADOW-VERSION 3.5.3\nnil\n")]
        (is (= "3.5.3" (compare/shadow-version shadow-dir ["npx" "shadow-cljs"]))))
      (let [{:keys [tools generated? light-build shadow-build shadow-cache-root]}
            (compare/project {:dir (str dir) :light "/bin/light" :shadow-dir (str shadow-dir) :shadow-build "app"})
            {:keys [light shadow]} (into {} (map (juxt :tool identity) tools))]
        (is (not generated?))
        (is (= [:client :app] [light-build shadow-build]))
        (is (= ["/bin/light" "release" "client" "--no-cache"] (:clean-cmd light)))
        (is (= ["clojure" "-M:shadow" "release" "app"] (:cached-cmd shadow)))
        (is (= (str shadow-dir) (:dir shadow)))
        (is (= "target/shadow-cache" shadow-cache-root))
        (fs/create-dirs (fs/path shadow-dir "target/shadow-cache/builds"))
        ((:clean! shadow))
        (is (not (fs/exists? (fs/path shadow-dir "target/shadow-cache"))))
        (fs/create-dirs (fs/path dir "out/js/nested"))
        (spit (fs/file dir "out/js/old.js") "stale")
        (let [since (+ 1000 (System/currentTimeMillis))]
          (doseq [[file text] [["out/js/client.js" "var a=1;"] ["out/js/nested/chunk.js" "var b=2;"] ["out/js/chunks.json" "{}"]]]
            (spit (fs/file dir file) text)
            (fs/set-last-modified-time (fs/path dir file) (java.nio.file.attribute.FileTime/fromMillis since)))
          (is (= {:files 2 :bytes 16} (select-keys ((:output light) since) [:files :bytes])))))
      (finally (fs/delete-tree dir)))))

(deftest markdown-report
  (let [summary (compare/summary [{:wall-s 1.0 :user-s 1.0 :system-s 0.5 :peak-memory-gb 0.5 :load [1.0]}] [])
        report (compare/markdown
                {:environment {:light-cljs {:binary "/b/light-cljs-abc1234" :commit "abc1234" :sha256 (apply str (repeat 64 "0"))}
                               :shadow-cljs "3.5.3" :jdk "openjdk 25" :node "v26" :clojure-cli "Clojure CLI 1.12"
                               :babashka "1.13" :os "macOS 15" :date "2026-11-13" :machine {:cpu "M3" :cpus 24 :memory-gib 32}}
                 :options {:runs 1 :max-load 2.0 :max-repeats 3}
                 :project {:light-build :bench :shadow-build :bench}
                 :scenarios {:uncached {:light summary :shadow (update-in summary [:wall-s :median] * 4)}}
                 :outputs {:uncached {:light {:bytes 1000 :gzip 300 :checksum "42" :waste-ok true}
                                      :shadow {:bytes 1200 :gzip 400 :files 1}}}
                 :warm-jvm {:builds [{:wall-s 3.0 :cpu-s 5.0 :load-1m 1.5}] :process {:peak-memory-gb 2.0}}})]
    (is (str/includes? report "light-cljs `abc1234`"))
    (is (str/includes? report "Release builds of light-cljs.edn build `:bench` and shadow-cljs.edn build `:bench`."))
    (is (str/includes? report "| clean build | light-cljs | 1.00 s (1.00-1.00) | 1.00 s (1.00-1.00) | 0.50 s (0.50-0.50) | 0.500 GB (0.500-0.500) | 1.00-1.00 |"))
    (is (str/includes? report "| clean build | 0.25 | 1.00 | 1.00 |"))
    (is (str/includes? report "| clean build | light-cljs | 1,000 | 300 | oracle 42, waste ok |"))
    (is (str/includes? report "3.00 s (5.0 s CPU, load 1.50)"))))

(deftest time-and-load-parsing
  (is (= [1.64 3.01 2.95] (compare/parse-load "{ 1.64 3.01 2.95 }\n")))
  (is (= [0.12 0.2 0.3] (compare/parse-load "0.12 0.20 0.30 1/234 5678\n")))
  (is (= {:wall-s 40.98 :user-s 122.37 :system-s 126.74 :peak-memory-gb 3.298246656}
         (compare/parse-time true (str "       40.98 real       122.37 user       126.74 sys\n"
                                       "          3298246656  maximum resident set size\n"))))
  (let [gnu (fn [elapsed] (str "\tUser time (seconds): 6.29\n\tSystem time (seconds): 0.36\n"
                               "\tElapsed (wall clock) time (h:mm:ss or m:ss): " elapsed "\n"
                               "\tMaximum resident set size (kbytes): 1051394\n"))]
    (is (= {:wall-s 6.09 :user-s 6.29 :system-s 0.36 :peak-memory-gb 1.051394}
           (compare/parse-time false (gnu "0:06.09"))))
    (is (= 3725.5 (:wall-s (compare/parse-time false (gnu "1:02:05.50")))))))

(deftest memory-limit-sweep
  (testing "options"
    (is (= {:shadow-heaps ["1g" "1536m" "8g"] :light-memory-limits [67108864 2147483648]}
           (select-keys (compare/parse-options ["p" "--light" "b" "--shadow-heaps" "1g,1536m,8g"
                                                "--light-memory-limits" "67108864,2147483648"])
                        [:shadow-heaps :light-memory-limits])))
    (doseq [args [["p" "--light" "b" "--shadow-heaps" "1.5g"] ["p" "--light" "b" "--shadow-heaps" "1g,"]
                  ["p" "--light" "b" "--light-memory-limits" "64m"] ["p" "--light" "b" "--light-memory-limits" "0"]]]
      (is (thrown? Exception (compare/parse-options args)) (pr-str args)))
    (is (= [1073741824 1610612736 8589934592 524288] (map compare/jvm-size-bytes ["1g" "1536m" "8G" "512k"]))))
  (testing "GC log and failed builds"
    (is (= {:gc-pause-s 0.1 :gc-pauses 3 :full-gcs 1}
           (compare/parse-gc-log (str "[0.010s][info][gc] Using G1\n"
                                      "[1.2s][info][gc] GC(0) Pause Young (Normal) (G1 Evacuation Pause) 24M->3M(256M) 2.500ms\n"
                                      "[2.0s][info][gc] GC(1) Concurrent Mark Cycle 45.600ms\n"
                                      "[2.1s][info][gc] GC(1) Pause Remark 120M->110M(256M) 7.500ms\n"
                                      "[3.0s][info][gc] GC(2) Pause Full (G1 Compaction Pause) 250M->200M(256M) 90.000ms\n"))))
    (let [failed (compare/timed-or-failed "." ["sh" "-c" "echo 'java.lang.OutOfMemoryError: Java heap space' >&2; exit 3"])
          ok (compare/timed-or-failed "." ["true"])]
      (is (= [true 3 true] ((juxt :failed :exit :oom) failed)))
      (is (number? (:wall-s failed)))
      (is (not (:failed ok)))))
  (testing "failed builds count but stay out of the medians"
    (let [s (compare/summary [{:wall-s 9.0 :user-s 1.0 :system-s 1.0 :peak-memory-gb 1.0 :load [1.0] :failed true :oom true}
                              {:wall-s 2.0 :user-s 1.0 :system-s 1.0 :peak-memory-gb 2.0 :load [2.0] :gc-pause-s 0.5 :gc-pauses 4 :full-gcs 0}]
                             [])]
      (is (= [2 1] ((juxt :runs :failures) s)))
      (is (= 2.0 (get-in s [:wall-s :median])))
      (is (= 0.5 (get-in s [:gc-pause-s :median])))
      (is (= {:median 1.5 :min 1.0 :max 2.0} (:load-1m s))))
    (is (nil? (:wall-s (compare/summary [{:wall-s 1.0 :user-s 1.0 :system-s 1.0 :peak-memory-gb 1.0 :load [1.0] :failed true}] [])))))
  (testing "rounds record failures and merge :after"
    (let [result (with-redefs [compare/timed-or-failed (fn [_ [cmd]] (cond-> {:wall-s 1.0 :user-s 1.0 :system-s 0.1 :peak-memory-gb 0.1}
                                                                       (= "small" cmd) (assoc :failed true)))
                               compare/load-average (constantly [0.5 0.5 0.5])]
                   (compare/rounds (gate-off 2) (constantly nil)
                                   [{:tool :small :cmd ["small"] :may-fail true :after (fn [s _] {:checked (not (:failed s))})}
                                    {:tool :big :cmd ["big"] :may-fail true :after (fn [s _] {:checked (not (:failed s))})}]))]
      (is (= [2 0] (map #(get-in result [% :failures]) [:small :big])))
      (is (= [true true] (map :checked (get-in result [:big :samples]))))))
  (let [dir (fs/create-temp-dir {:prefix "light-sweep-project-"})]
    (try
      (spit (fs/file dir "light-cljs.edn") "{:builds {:bench {:target :esm :output-dir \"out/light\" :compiler-options {:macro-memory-limit 2147483648}}}}")
      (spit (fs/file dir "shadow-cljs.edn") "{:builds {:bench {:target :node-script :output-to \"out/shadow.js\"}}}")
      (spit (fs/file dir "deps.edn") "{:aliases {:shadow {:jvm-opts [\"-Xmx8g\"] :main-opts [\"-m\" \"shadow.cljs.devtools.cli\"]}}}")
      (let [options {:dir (str dir) :light "/bin/light" :shadow-heaps ["1g" "8g"] :light-memory-limits [67108864]}
            project (compare/project options)
            tools (compare/sweep-tools options project "/gc")
            by-id (into {} (map (juxt :tool identity) tools))]
        (testing "one build per limit, the heap flag after the alias's"
          (is (= [:light :light-mml67108864 :shadow-xmx1g :shadow-xmx8g] (map :tool tools)))
          (is (= ["/bin/light" "release" "bench" "--no-cache" "--macro-memory-limit" "67108864"] (:clean-cmd (by-id :light-mml67108864))))
          (is (= ["clojure" "-J-Xmx1g" "-J-XX:+ExitOnOutOfMemoryError" "-J-Xlog:gc:file=/gc/gc-1g.log" "-M:shadow" "release" "bench"]
                 (:clean-cmd (by-id :shadow-xmx1g))))
          (is (= "light-cljs.edn's :macro-memory-limit 2147483648 (2 GiB)" (:limit (by-id :light))))
          (is (= "--macro-memory-limit 67108864 (64 MiB)" (:limit (by-id :light-mml67108864))))
          (is (every? :may-fail tools))
          (is (thrown? Exception (compare/sweep-tools options (assoc project :shadow-cmd ["npx" "shadow-cljs"]) "/gc"))))
        (testing "the smallest adequate heap and the ratios"
          (let [s (fn [wall peak & [failures]] {:runs 3 :failures (or failures 0) :load-1m {:min 1.0 :max 2.0} :samples []
                                                :wall-s {:median wall :min wall :max wall} :user-s {:median 1 :min 1 :max 1}
                                                :system-s {:median 1 :min 1 :max 1} :peak-memory-gb {:median peak :min peak :max peak}})
                tools (into tools [{:tool :shadow-xmx2g :label "shadow-cljs" :limit "-Xmx2g (2 GiB)" :heap-bytes (compare/jvm-size-bytes "2g")}])
                by-tool {:light (s 6.0 1.0) :light-mml67108864 (s 6.0 0.9) :shadow-xmx1g (assoc (s 60.0 1.1 1) :wall-s nil)
                         :shadow-xmx2g (s 43.0 2.0) :shadow-xmx8g (s 40.0 4.0)}
                verdict (compare/sweep-verdict tools by-tool)
                report (compare/sweep-markdown
                        {:environment {:light-cljs {:binary "/b/light-cljs-abc1234" :commit "abc1234" :sha256 (apply str (repeat 64 "0"))}
                                       :jdk "openjdk 25" :node "v26" :clojure-cli "CLI" :babashka "1" :os "macOS" :date "2026-11-14"
                                       :machine {:cpu "M3" :cpus 24 :memory-gib 32}}
                         :options {:runs 3 :max-load 1000.0}
                         :project {:light-build :bench :shadow-build :bench :sweep-tools tools}
                         :scenarios {:uncached by-tool}
                         :outputs {:uncached {:light {:bytes 10 :gzip 5 :checksum "1" :waste-ok true}}}})]
            (is (= {:best :shadow-xmx8g :adequate :shadow-xmx2g :largest :shadow-xmx8g}
                   (select-keys verdict [:best :adequate :largest])))
            (is (= [0.5 0.45] (map :adequate (:ratios verdict))))
            (is (= [0.25 0.225] (map :largest (:ratios verdict))))
            (is (str/includes? report "smallest heap with no failed build and a median within 10% of that is -Xmx2g (2 GiB), 43.00 s at 2.000 GB peak memory."))
            (is (str/includes? report "| shadow-cljs | -Xmx1g (1 GiB) | - | - | - | - | - | 1 of 3 | - | 1.00-2.00 |"))
            (is (str/includes? report "light-cljs at --macro-memory-limit 67108864 (64 MiB): 0.900 GB peak memory, 0.45x"))
            (is (str/includes? report "| light-cljs at light-cljs.edn's :macro-memory-limit 2147483648 (2 GiB) | 10 | 5 | oracle 1, waste ok |")))))
      (finally (fs/delete-tree dir)))))

(def leaf-original
  "Bytes a text round trip would not reproduce: invalid UTF-8, CRLF, no final newline."
  (byte-array (concat (.getBytes "(ns leaf)\r\n(def x \"" "UTF-8") [-1 -2 -128] (.getBytes "\")" "UTF-8"))))

(defn leaf-bytes [dir] (vec (fs/read-all-bytes (fs/file dir "leaf.cljs"))))

(deftest leaf-scenario-restores-source-after-failure
  (let [dir (str (fs/create-temp-dir {:prefix "light-leaf-test-"}))
        sample {:wall-s 1.0 :user-s 1.0 :system-s 0.1 :peak-memory-gb 0.1}
        leaf (fn [runs check] (compare/leaf-scenario (gate-off runs) [(fs/file dir "leaf.cljs")]
                                                     [{:tool :light :dir dir :cmd ["build"]}] check))
        edited? #(str/includes? (slurp (fs/file dir "leaf.cljs")) "compare-leaf-edit-")]
    (try
      (fs/write-bytes (fs/file dir "leaf.cljs") leaf-original)
      (testing "a build that throws"
        (with-redefs [compare/timed (fn [_ _] (is (edited?)) (throw (ex-info "build broke" {})))]
          (is (thrown-with-msg? Exception #"build broke"
                                (leaf 2 (constantly {})))))
        (is (= (vec leaf-original) (leaf-bytes dir))))
      (testing "builds that succeed, then a check that throws"
        (let [builds (atom 0)]
          (with-redefs [compare/timed (fn [_ _] (is (edited?)) (swap! builds inc) sample)]
            (is (thrown-with-msg? Exception #"check broke"
                                  (leaf 2 #(do (is (edited?)) (throw (ex-info "check broke" {})))))))
          (is (= 2 @builds)))
        (is (= (vec leaf-original) (leaf-bytes dir))))
      (testing "a run that succeeds"
        (with-redefs [compare/timed (constantly sample)]
          (is (= {:ok true} (:outputs (leaf 1 (constantly {:ok true}))))))
        (is (= (vec leaf-original) (leaf-bytes dir))))
      (testing "a run terminated during its build"
        (let [script (str "(load-file " (pr-str (str (fs/path kit "compare.bb"))) ")"
                          "(with-redefs [generated-project.compare/timed"
                          " (fn [_ _] (println :edited) (flush) (Thread/sleep 60000))]"
                          " (generated-project.compare/leaf-scenario " (pr-str (gate-off 1)) " [" (pr-str (str (fs/file dir "leaf.cljs"))) "]"
                          " [{:tool :light :dir " (pr-str dir) " :cmd []}] (constantly nil)))")
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

;;; ---------------------------------------------------------------------------
;;; The re-frame shape (generate.bb --shape re-frame, re-frame-oracle.bb).
;;; Tests that write a project need the library sources re-frame-libs.bb makes
;;; (research/ is gitignored); without them they print a note and skip.

(load-file (str (fs/path kit "re-frame-oracle.bb")))
(alias 'rf-oracle 'generated-project.re-frame-oracle)

(def rf-options (assoc gen/defaults :seed 42 :shape "re-frame"))
(def rf-fixture (delay (gen/re-frame-project rf-options)))
(def rf-libs (let [dir (gen/rf-libs-dir)] (when (fs/exists? (fs/path dir "libs.edn")) dir)))
(defn rf-skip [what] (println "SKIP" what "- no re-frame library sources; run re-frame-libs.bb"))

(deftest re-frame-deterministic-and-sensitive-to-seed
  (is (= (map :source (:files @rf-fixture)) (map :source (:files (gen/re-frame-project rf-options)))))
  (is (= (:model @rf-fixture) (:model (gen/re-frame-project rf-options))))
  (let [other (gen/re-frame-project (assoc rf-options :seed -7))]
    (is (not= (:model @rf-fixture) (:model other)))
    (is (not= (map :source (:files @rf-fixture)) (map :source (:files other)))))
  (let [model (:model @rf-fixture)]
    ;; Frozen; cross-checked with light none/prepare/advanced and shadow dev/release.
    (is (= "317532 191053" (rf-oracle/expected model 42 3)))
    (is (> (count (set (map #(rf-oracle/expected model % 1) [0 1 42 1000002]))) 1))
    (is (thrown? Exception (rf-oracle/expected model 1000003 1)))
    (is (thrown? Exception (rf-oracle/expected (:model @fixture) 1 1)))))

(deftest re-frame-size-shape-and-readable-sources
  (let [{:keys [files model markers]} @rf-fixture
        sources (map :source (remove #(= :shim (:role %)) files))
        features (:features model)
        text (str/join "\n" (map :source files))]
    (is (<= 5000 (:code-lines (gen/source-metrics sources)) 5600))
    (is (>= (count features) 8))
    (is (= (* 4 (count features)) (count (filter #(= :feature (:role %)) files))))
    (is (= 1 (count (filter #(= :entry (:role %)) files))))
    ;; Every feature uses each kind of re-frame and reagent construct.
    (doseq [needle ["rf/reg-event-db" "rf/reg-event-fx" "rf/inject-cofx" "rf/path" "rf/trim-v" "rf/enrich"
                    "rf/after" "rf/->interceptor" ":<- [::" "(fn [_ _] [(rf/subscribe" "r/with-let"
                    "r/create-class" "^{:key"]]
      (is (= (count features) (count (filter #(str/includes? (:source %) needle)
                                             (filter #(re-find #"/(events|subs|views)\.cljs$" (:path %)) files))))
          needle))
    (is (some :dep features))
    (is (= #{"add" "toggle" "compute" "remove" "filter" "rename" "clear"}
           (set (remove #(str/starts-with? % "bump-") (mapcat :events features)))))
    (is (apply distinct? (map :marker markers)))
    (doseq [{:keys [marker]} markers]
      (is (= 1 (count (re-seq (re-pattern marker) text))) marker))
    (doseq [{:keys [path source]} files]
      (testing path (is (pos? (count (read-all source))))))))

(deftest re-frame-options
  (is (= "re-frame" (:shape (gen/parse-options ["out" "--shape" "re-frame"]))))
  (is (not (contains? (gen/parse-options ["out" "--shape" "plain"]) :shape)))
  (doseq [args [["out" "--shape" "vue"] ["out" "--shape"] ["out" "--libs"]]]
    (is (thrown? Exception (gen/parse-options args)) (pr-str args)))
  (let [dir (fs/create-temp-dir {:prefix "light-re-frame-test-"})]
    (try
      (is (thrown-with-msg? Exception #"re-frame-libs.bb"
                            (gen/generate! (assoc rf-options :output (str (fs/path dir "p")) :libs (str (fs/path dir "none"))))))
      (is (not (fs/exists? (fs/path dir "p"))))
      (finally (fs/delete-tree dir)))))

(deftest re-frame-standalone-project
  (if-not rf-libs
    (rf-skip "re-frame-standalone-project")
    (with-project
      rf-options
      (fn [dir manifest]
        (is (= "317532 191053\n" (:out (command dir "bb" "oracle.bb" "42" "3"))))
        (with-project rf-options (fn [other _] (is (= (file-contents dir) (file-contents other)))))
        (is (every? (fn [[path sha]] (= sha (gen/sha256 (slurp (fs/file dir path))))) (:library-sha256 manifest)))
        (let [light (edn/read-string (slurp (fs/file dir "light-cljs.edn")))
              shadow (edn/read-string (slurp (fs/file dir "deps.edn")))]
          (is (= (into ["src" "macros"] gen/rf-light-roots) (:source-paths light)))
          (is (= ["src" "macros" "shadow-support" "lib/shadow/re-frame"] (:paths shadow)))
          (is (= "1.2.0" (get-in shadow [:deps 'reagent/reagent :mvn/version]))))))))

(deftest re-frame-compiler-comparisons
  (when (or (:light options) (:shadow options))
    (if-not rf-libs
      (rf-skip "re-frame-compiler-comparisons")
      (doseq [opts [rf-options (assoc rf-options :seed -7 :lines 2000)]]
        (with-project
          opts
          (fn [dir manifest]
            (command dir "npm" "install" "--no-audit" "--no-fund")
            (let [model (edn/read-string (slurp (fs/file dir "model.edn")))
                  check (fn [runner]
                          (doseq [[input iterations] [[0 1] [42 3] [1000002 2]]]
                            (is (= (str (rf-oracle/expected model input iterations) "\n")
                                   (:out (command dir "node" runner (str input) (str iterations))))
                                (str runner " input=" input " iterations=" iterations))))]
              (when-let [binary (:light options)]
                (doseq [level ["none" "advanced"]]
                  (println "light re-frame" level (:seed opts)) (flush)
                  (command dir binary "release" "bench" "--optimize" level)
                  (check "run-light.mjs")
                  (check-waste-at manifest (fs/path dir "out/light/workload.main.js") (= level "advanced"))))
              (when (:shadow options)
                (doseq [mode ["compile" "release"]]
                  (println "shadow re-frame" mode (:seed opts)) (flush)
                  (let [{:keys [out err]} (command dir "clojure" "-M:shadow" mode "bench")]
                    (is (not (re-find #"WARNING #" (str out err))) (str "shadow " mode " warnings")))
                  (check "out/shadow.js")
                  (when (= mode "release")
                    (check-waste-at manifest (fs/path dir "out/shadow.js") true)))))))))))

;;; End of the re-frame shape's tests.
;;; ---------------------------------------------------------------------------

(let [{:keys [fail error]} (run-tests 'generated-project.test)]
  (System/exit (if (zero? (+ fail error)) 0 1)))
