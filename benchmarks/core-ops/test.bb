#!/usr/bin/env bb
;; Tests for run.bb's selection and checksum rules: bb benchmarks/core-ops/test.bb
;; Nothing is built or run.
(require '[clojure.test :refer [deftest is testing run-tests]])
;; Loading the runner supplies its functions without running main.
(load-file (str (.getParent (java.io.File. *file*)) "/run.bb"))

(defn- rejected [options]
  (try (selected-ops options) nil (catch clojure.lang.ExceptionInfo e (ex-message e))))

(deftest selection-follows-the-harness
  (testing "all operations by default, in the harness's order, the loops last"
    (let [ops (selected-ops {})]
      (is (= 68 (count ops)))
      (is (= ["kw-small" "bit-and" "aget-bit-and"] [(first ops) (nth ops 66) (nth ops 67)]))))
  (testing "a name, or the operations it starts up to a hyphen"
    (is (= ["kw-small" "assoc-small-existing" "assoc-small-new" "vec-conj-64"]
           (selected-ops {:ops ["vec-conj" "assoc-small" "kw-small"]})))
    (is (= ["seq-walk-vec-1000"] (selected-ops {:pseudo-names "seq-walk-vec"})))))

(deftest selection-rejects-what-it-cannot-measure
  (is (re-find #"Unknown operations nope" (rejected {:ops ["kw-small" "nope"]})))
  (is (re-find #"Unknown operations kw-sma" (rejected {:ops ["kw-sma"]})) "a prefix ends at a hyphen")
  (is (re-find #"names no operation" (rejected {:ops []})) "--ops \"\" or \",\"")
  (is (= [] ((second (value-options "--ops")) ",")) "a comma-only --ops parses to nothing")
  (is (re-find #"profiles one operation" (rejected {:pseudo-names "assoc-small"})))
  (is (re-find #"drop --ops" (rejected {:ops ["get-small"] :pseudo-names "kw-small"}))))

(defn- run [tool & checksums]
  {:tool tool :rows (zipmap ["kw-small" "count-small"] (map (fn [c] {:ns-per-op 1.0 :checksum c}) checksums))})

(deftest checksums-must-agree-everywhere
  (let [ops ["kw-small" "count-small"]]
    (is (nil? (check-checksums! [(run :light "5" "7") (run :shadow "5" "7") (run :light "5" "7")] ops)))
    (is (thrown-with-msg? clojure.lang.ExceptionInfo #"Checksums of count-small differ"
                          (check-checksums! [(run :light "5" "7") (run :shadow "5" "8")] ops))
        "light-cljs against shadow-cljs")
    (is (thrown-with-msg? clojure.lang.ExceptionInfo #"Checksums of kw-small differ"
                          (check-checksums! [(run :light "5" "7") (run :shadow "5" "7") (run :light "6" "7")] ops))
        "two runs of one compiler")
    (is (thrown-with-msg? clojure.lang.ExceptionInfo #"Checksums of kw-small differ"
                          (check-checksums! [(run :light "5" "7") {:tool :shadow :rows {}}] ops))
        "a run without the operation")))

(let [{:keys [fail error]} (run-tests)] (System/exit (if (zero? (+ fail error)) 0 1)))
