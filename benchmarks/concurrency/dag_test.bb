#!/usr/bin/env bb
;; Regression tests for dag.bb (light-cljs-372). Run from anywhere:
;;
;;   bb benchmarks/concurrency/dag_test.bb
;;
;; tests/cli.rs runs it too when bb is on PATH.
;;
;; fixtures/root-edges.trace.tsv is a synthetic three-namespace trace whose
;; costs are multiples of 100 ms. Its root edges form one chain: the top-level
;; pass of b reads a root nested in a's drained unit a/x (0.4 s), b's drained
;; unit b/z (0.5 s) starts after that pass, and c's drained unit c/y reads a
;; root nested in b/z. With the edges every W finishes at 1.30 s; without them
;; at 0.80 s from W=2. Dropping the top-level wait gives 1.00 s, dropping the
;; unit wait 1.10 s, and mapping no nested unit to its drained unit drops the
;; top-level edge (1.00 s). Each edge also comes as an execute row, a duplicate
;; read row and a read made from a nested frame, so the count of 2 edges also
;; checks that they are deduplicated after mapping. The execute rows alone and
;; the read rows alone each give the same result, so neither source can be
;; lost unnoticed. One read inside b/z and one
;; from a's drain bookkeeping are dropped.
;;
;; fixtures/main-probe.dag.txt is the output of the retained dag.bb
;; (f68c767, SHA-256 in README.md) on main-probe.tsv, which has no trace rows.
(require '[babashka.fs :as fs]
         '[babashka.process :refer [shell]]
         '[clojure.string :as str]
         '[clojure.test :refer [deftest is run-tests]])

(def kit (str (fs/parent (fs/absolutize *file*))))
(defn kit-file [name] (str (fs/path kit name)))
(def trace (kit-file "fixtures/root-edges.trace.tsv"))

(defn dag [file]
  (:out (shell {:out :string} "bb" (kit-file "dag.bb") file)))

(defn section
  "The heading line that starts with `heading` and the six W rows after it."
  [out heading]
  (->> (str/split-lines out)
       (drop-while #(not (str/starts-with? % heading)))
       (take 7)
       vec))

(def option-u "Option U (serial top-level passes; units on W workers):")
(def option-u-edges "Option U with root edges")
(defn rows [& seconds]
  (mapv (fn [w t] (format "  W=%2d  %s" w t)) [1 2 4 8 12 16] seconds))

(defn dag-without
  "dag.bb's output on the fixture without the rows whose kind matches the
  regex alternation `kinds`, header rows included."
  [kinds]
  (let [file (str (fs/create-temp-file {:prefix "dag-test-" :suffix ".tsv"}))]
    (try
      (spit file (str/join (map #(str % "\n")
                                (remove #(re-find (re-pattern (str "^#?(" kinds ")\t")) %)
                                        (str/split-lines (slurp trace))))))
      (dag file)
      (finally (fs/delete-if-exists file)))))

(def with-edges
  (into ["Option U with root edges (2 edges between tasks, 1 of them from a top-level pass):"]
        (rows "1.30 s" "1.30 s" "1.30 s" "1.30 s" "1.30 s" "1.30 s")))

(deftest option_u_root_edges_gate_units_and_top_level_passes
  (let [out (dag trace)]
    (is (= (into [option-u] (rows "1.10 s" "0.80 s" "0.80 s" "0.80 s" "0.80 s" "0.80 s"))
           (section out option-u)))
    (is (= with-edges (section out option-u-edges))))
  ;; Execute rows alone and read rows alone each carry both edges.
  (is (= with-edges (section (dag-without "read") option-u-edges)) "execute rows only")
  (is (= with-edges (section (dag-without "execute") option-u-edges)) "read rows only")
  ;; With neither, the edge model is Option U.
  (let [out (dag-without "execute|read")]
    (is (= (rest (section out option-u)) (rest (section out option-u-edges))))
    (is (= "Option U with root edges (0 edges between tasks, 0 of them from a top-level pass):"
           (first (section out option-u-edges))))))

(deftest main_probe_output_is_unchanged
  (is (= (slurp (kit-file "fixtures/main-probe.dag.txt"))
         (dag (kit-file "main-probe.tsv")))))

(let [{:keys [fail error]} (run-tests)]
  (System/exit (if (zero? (+ fail error)) 0 1)))
