#!/usr/bin/env bb
(require '[clojure.test :refer [deftest is run-tests]])
;; Loading the entry point supplies its functions and fs/json aliases, without
;; invoking main. Lint both files together, as documented.
(load-file (str (.getParent (java.io.File. *file*)) "/native-macros.bb"))

(deftest native-build-overrides-configured-target-and-ignores-stale-binary
  (let [dir (fs/create-temp-dir {:dir (fs/create-dirs (fs/path repo "target")) :prefix "native-build-test-"})]
    (try
      (fs/create-dirs (fs/path dir ".cargo"))
      (fs/create-dirs (fs/path dir "src"))
      (fs/create-dirs (fs/path dir "target/release"))
      (spit (str (fs/path dir "Cargo.toml"))
            "[package]\nname = \"light-cljs\"\nversion = \"0.0.0\"\nedition = \"2021\"\n[workspace]\n[features]\ndefault = [\"v8\"]\nv8 = []\nbbp-macros = []\n")
      (spit (str (fs/path dir "Cargo.lock")) "version = 3\n[[package]]\nname = \"light-cljs\"\nversion = \"0.0.0\"\n")
      (spit (str (fs/path dir "src/main.rs"))
            "fn main() { assert!(cfg!(feature = \"bbp-macros\")); assert!(!cfg!(feature = \"v8\")); println!(\"native-only\"); }\n")
      (spit (str (fs/path dir ".cargo/config.toml"))
            "[build]\ntarget = \"invalid-configured-target\"\ntarget-dir = \"wrong-configured-directory\"\n")
      (spit (str (fs/path dir "target/release/light-cljs")) "stale-v8-binary")
      (with-redefs [repo (str dir)]
        (let [{:keys [binary target target-dir] build :command} (native-build)
              env (dissoc (into {} (System/getenv)) "CARGO_BUILD_TARGET")]
          ;; Exercise both Cargo configuration and the environment override. The
          ;; invalid targets ensure success requires the explicit host selection.
          (doseq [env [env (assoc env "CARGO_BUILD_TARGET" "invalid-env-target")]]
            (let [built (apply p/shell {:dir repo :env (assoc env "CARGO_TARGET_DIR" (str (fs/path dir "wrong-env-directory")))
                                        :out :string :err :string :continue true} build)]
              (is (zero? (:exit built)) (:err built))))
          (is (= (str (fs/path dir "target")) target-dir))
          (is (= (str (fs/path target-dir target "release/light-cljs")) binary))
          (is (= "native-only\n" (:stdout (command [binary]))))
          (is (= "stale-v8-binary" (slurp (str (fs/path dir "target/release/light-cljs")))))))
      (finally (fs/delete-tree dir)))))

(deftest alternating-fresh-process-plan
  (let [plan (vec (schedule {:runs 5 :jobs [1 2 4 8]} [{:name "native"} {:name "v8"}]))
        cells (group-by (juxt #(get-in % [:fixture :name]) #(get-in % [:backend :name]) :jobs) plan)]
    (is (= 120 (count plan)))
    (is (every? #(= 5 (count %)) (vals cells)))
    (is (= [1 1 2 2 4 4 8 8] (mapv :jobs (take 8 plan))))
    (is (= [8 8 4 4 2 2 1 1] (mapv :jobs (take 8 (drop 24 plan)))))
    (is (= ["native" "v8" "v8" "native"] (mapv #(get-in % [:backend :name]) (take 4 plan))))
    (is (= "v8" (get-in plan [24 :backend :name])))))

(deftest validation-excludes-wrong-results-maps-and-unproven-helper
  (let [fixture (first fixtures) sample {:exit 0 :stderr ""} result {:exit 0 :stdout (:expected fixture)}]
    (is (= "ok" (result-status fixture sample result true)))
    (is (= "result-failure" (result-status fixture sample (assoc result :stdout "wrong") true)))
    (is (= "result-failure" (result-status fixture sample (assoc result :exit 1) true)))
    (is (= "identity-failure" (result-status fixture sample result false)))
    (is (= "timeout" (result-status fixture (assoc sample :timeout true) result true)))
    (is (= "unsupported" (result-status fixture {:exit 1 :stderr "unsupported operation: helper not preloaded: native-bench.helper/demanded"} nil false)))
    (is (= "compile-failure" (result-status fixture {:exit 1 :stderr "panic"} nil false)))
    (let [helper (last fixtures)]
      (is (= "unverified-helper" (result-status helper sample {:exit 0 :stdout (:expected helper)} true))))))

(deftest identity-mismatch-invalidates-every-sample-for-fixture
  (let [row {:fixture "collections" :backend "native" :jobs 1 :status "ok"
             :js-sha256 "a" :map-sha256 "b" :result "42" :metrics {:wall {:value 2 :unit "seconds"}}}]
    (doseq [change [{:js-sha256 "x"} {:map-sha256 "x"} {:result "x"}]]
      (let [rows (validate-identities [row (merge row {:jobs 8} change)])]
        (is (every? #(= "identity-failure" (:status %)) rows))
        (is (every? #(and (zero? (:samples %)) (empty? (:metrics %)) (not (:performance-eligible %))) (summarize rows 5)))))
    (is (= [row row] (validate-identities [row row])))
    (is (not (:performance-eligible (first (summarize [row] 1)))))
    (is (:performance-eligible (first (summarize (repeat 5 row) 5))))
    (is (= {:median 3.0 :min 1 :max 5 :spread 4 :unit "seconds"}
           (get-in (first (summarize (map #(assoc-in row [:metrics :wall :value] %) [1 5 2 4 3]) 5)) [:metrics :wall])))))

(deftest metrics-and-trace-preserve-units
  (let [sample {:wall-seconds 2 :stderr "0.50 real 0.25 user 0.10 sys\n1024 maximum resident set size\nmacro.bridge.0.ns 7\nmacro.bridge.1.ns 5\n"}
        m (metrics sample true)]
    (is (= 1024 (get-in m [:peak-rss :value])))
    (is (= 0.25 (get-in m [:user-cpu :value])))
    (is (= 12.0 (get-in m [:form-bridge :value])))
    (is (string? (get-in m [:parked-tasks :unavailable])))
    (is (= 1024 (get-in (metrics {:stderr "Maximum resident set size (kbytes): 1"} false) [:peak-rss :value]))))
  (is (= [{:id "7" :namespace "a.cljs" :name "f" :line "2" :column "3" :engine-us "40" :engine-calls "1"}]
         (unit-identities "unit\t7\ta.cljs\tf\t2\t3\t50\t40\t1\t0\n"))))

(deftest timeout-kills-and-reaps-process
  (let [result (timed-process [(or (System/getenv "NODE") "node") "-e" "process.on('SIGTERM',()=>{});setInterval(()=>{},1000)"] 100)]
    (is (:timeout result))
    (is (not= 0 (:exit result)))
    (is (< (:wall-seconds result) 5))))

;; Optionally validate the output of the actual CLI smoke, not just pure helpers.
(when-let [report-path (first *command-line-args*)]
  (deftest real-smoke-output
    (let [report (json/parse-string (slurp report-path) true)
          rows (:samples report)]
      (is (= 1 (get-in report [:options :runs])))
      (is (= 12 (count rows)))
      (let [binary (get-in report [:backends 0 :binary])]
        (is (= (str (fs/path (:build-target-dir report) (:build-target report) "release/light-cljs")) binary))
        (is (= ["--target" (:build-target report) "--target-dir" (:build-target-dir report)]
               (take-last 4 (:build-command report))))
        (is (= (sha256 binary) (get-in report [:backends 0 :sha256])))
        (is (every? #(= binary (nth (:command %) 2)) (filter #(= "bbp-macros" (:backend %)) rows))))
      (doseq [fixture (take 2 fixtures)
              :let [row (first (filter #(and (= (:name fixture) (:fixture %)) (= 1 (:jobs %))) rows))]]
        (is (= "ok" (:status row)))
        (is (= (:expected fixture) (:result row)))
        (is (= (:js-sha256 row) (sha256 (fs/path (:artifacts row) "out.js"))))
        (is (= (:map-sha256 row) (sha256 (fs/path (:artifacts row) "out.map"))))
        (is (= (:native-macro-calls fixture)
               (count (filter #(re-matches #"macro.bridge.\d+.ns" (name %)) (keys (:stats row)))))))
      (doseq [row rows :when (or (> (:jobs row) 1) (= "helper" (:fixture row)))]
        (is (= "unsupported" (:status row)))
        (is (re-find (if (= 1 (:jobs row)) #"helper not preloaded: native-bench.helper/demanded" #"requires --jobs 1") (:diagnostic row))))
      (is (every? (complement :performance-eligible) (:summary report))))))

(let [{:keys [fail error]} (run-tests)] (System/exit (if (zero? (+ fail error)) 0 1)))
