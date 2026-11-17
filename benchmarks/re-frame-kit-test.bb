#!/usr/bin/env bb
;; Tests for re-frame.bb's port failures: bb benchmarks/re-frame-kit-test.bb
;; Nothing is built or run: the example is a temporary directory and the
;; porter's process is stubbed.
(require '[babashka.fs :as fs]
         '[babashka.process :as process]
         '[clojure.string :as str]
         '[clojure.test :refer [deftest is testing run-tests]])
;; Loading the kit supplies its functions without running its command line.
(load-file (str (.getParent (java.io.File. *file*)) "/re-frame.bb"))

(defn- port-failure
  "The message `port` throws for an example in DIR when the porter's process
  returns RESULT (nil: the process must not start)."
  [dir result]
  (with-redefs [example (constantly dir)
                process/sh (fn [& _] (or result (throw (ex-info "the porter started" {}))))]
    (try (with-out-str (port "simple" ["--porter" (str dir)])) nil
         (catch clojure.lang.ExceptionInfo e (ex-message e)))))

(deftest port-rejects-missing-setup
  (let [dir (fs/create-temp-dir)]
    (try
      (let [msg (port-failure dir nil)]
        (is (str/includes? msg "has no node_modules"))
        (is (str/includes? msg "run `bb benchmarks/re-frame.bb setup` first")))
      (finally (fs/delete-tree dir)))))

(deftest port-includes-porter-cause
  (let [dir (fs/create-temp-dir)]
    (try
      (fs/create-dirs (fs/path dir "node_modules"))
      (testing "from the porter's Caused by: on"
        (let [msg (port-failure dir {:exit 1 :out ""
                                     :err (str "failed to run function: light-cljs.port/main\n"
                                               (str/join (repeat 200 "\tat frame (cli_actual.clj:110)\n"))
                                               "Caused by:\nThe required JS dependency \"react\" is not available\n")})]
          (is (str/starts-with? msg "port failed:\nCaused by:\nThe required JS dependency \"react\""))
          (is (not (str/includes? msg "cli_actual.clj")))))
      (testing "its last 3,000 characters without a Caused by:"
        (let [msg (port-failure dir {:exit 1 :out "" :err (str (str/join (repeat 5000 "x")) "\nError building classpath.\n")})]
          (is (str/includes? msg "Error building classpath."))
          (is (<= (count msg) (+ (count "port failed:\n") 3000)))))
      (testing "a port that succeeds throws nothing"
        (is (nil? (port-failure dir {:exit 0 :out "" :err "[light-cljs.port] wrote 1 changed files\n"}))))
      (finally (fs/delete-tree dir)))))

;; growth's text transforms and attribution, on small hand-made outputs.

(deftest keyed-names-by-kind
  (is (= [:mask :kind :slot :marker :field :other]
         (map keyed-kind ["cljs$lang$protocol_mask$partition0$" "cljs$lang$kind$" "cljs$core$ICounted$_count$arity$1"
                          "cljs$core$ISeqable$" "cnt" "cljs$lang$cell"])))
  (let [text "a.cnt$abcd1234+b.cljs$lang$kind$$abcd1234+\"$abcd1234\"+c.cnt$abcd12345"]
    (testing "the key goes from the kinds named only, never from the key itself or a longer name"
      (is (= "a.cnt+b.cljs$lang$kind$$abcd1234+\"$abcd1234\"+c.cnt$abcd12345" (strip-key text "abcd1234" #{:field}))))
    (testing "short names, most frequent first"
      (is (= "x.a$abcd1234;y.a$abcd1234;z.b$abcd1234"
             (rename-keyed "x.cnt$abcd1234;y.cnt$abcd1234;z.tail$abcd1234" "abcd1234" :index-key))))))

(deftest keyword-hashes-dropped-from-the-keyword-constructor-only
  (is (= "p(null,\"a\"),p(\"ns\",\"b\"),q(null,\"c\",5),x.p(null,\"d\",6)"
         (drop-keyword-hashes "p(null,\"a\",123),p(\"ns\",\"b\",-4),q(null,\"c\",5),x.p(null,\"d\",6)"))))

(deftest runtime-text-is-attributed-to-its-unit
  ;; Line 0: "A" from the runtime's line 1 (unit u1), "B" from app.cljs;
  ;; line 1: "CD" from the runtime's line 3 (unit u2).
  (let [m {"sources" ["light-cljs/js_runtime.js" "app.cljs"]
           "sourcesContent" ["//@unit u1\nx\n  //@unit u2 requires: u1\ny" nil]
           "mappings" "AACA,CCDA;ADGA"}]
    (is (= [[0 1 "rt:u1"] [1 2 "program"] [2 3 "unmapped"] [3 5 "rt:u2"]] (labelled-runs "AB\nCD" m)))
    (is (= "B\n" (runs-without "AB\nCD" (labelled-runs "AB\nCD" m) #{"u1" "u2"})))))

(deftest growth-causes-add-up-to-the-growth
  (let [unit (fn [n body] (str "function " n "(a){return a.cnt$abcd1234+" body "}\n"))
        build (fn [units] (let [text (apply str (map second units))
                                ends (reductions + (map (comp count second) units))]
                            {:text text :runs (mapv (fn [[n] s e] [s e (str "rt:" n)]) units (cons 0 ends) ends)}))
        base (build [["kept" (unit "kept" "1")] ["gone" (unit "gone" "2")]])
        now (build [["kept" (str (unit "kept" "1") "p(null,\"k\",-12345);(\"$abcd1234\",\"0123456789abcdef\");")]
                    ["slotFirst" (unit "slotFirst" "3")] ["countSite" (unit "countSite" "4")]])
        chain (growth-chain base now)]
    (is (= (- (gzip-size (:text now)) (gzip-size (:text base))) (reduce + (map :gzip chain))))
    (is (= #{"new units: slot-first wrappers of the operations" "new units: call-site entries" "dropped units: other"}
           (set (keep #(when (:units %) (:cause %)) chain))))))

(let [{:keys [fail error]} (run-tests)] (System/exit (if (zero? (+ fail error)) 0 1)))
