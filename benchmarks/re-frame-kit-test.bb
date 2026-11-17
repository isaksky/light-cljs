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

(let [{:keys [fail error]} (run-tests)] (System/exit (if (zero? (+ fail error)) 0 1)))
