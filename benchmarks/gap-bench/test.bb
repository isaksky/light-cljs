#!/usr/bin/env bb
;; Unit tests for benchmarks/gap-bench/run.bb pure helpers, parsers, math,
;; gap ranking, baseline diffing, and Markdown scorecard rendering.
;;
;;   bb benchmarks/gap-bench/test.bb
(ns gap-bench.test
  (:require [babashka.fs :as fs]
            [clojure.string :as str]
            [clojure.test :refer [deftest is testing run-tests]]))

(def here (fs/parent (fs/absolutize *file*)))
(load-file (str (fs/path here "run.bb")))
(alias 'gb 'gap-bench.run)

(deftest parse-options-test
  (testing "defaults"
    (let [o (gb/parse-options [])]
      (is (= [:compile :watch :size :runtime] (:only o)))
      (is (= :auto (:shadow-mode o)))
      (is (= 3 (:runs o)))
      (is (= 2 (:compile-runs o)))
      (is (= 2 (:watch-edits o)))
      (is (= 1000 (:todos o)))
      (is (= "1536m" (:shadow-heap o)))
      (is (false? (:skip-browser o)))))
  (testing "flags and slices"
    (let [o (gb/parse-options ["--only" "compile,runtime" "--fresh-shadow"
                               "--runs" "5" "--compile-runs" "1" "--watch-edits" "2"
                               "--todos" "500" "--shadow-heap" "2g" "--skip-browser"])]
      (is (= [:compile :runtime] (:only o)))
      (is (= :fresh (:shadow-mode o)))
      (is (= 5 (:runs o)))
      (is (= 1 (:compile-runs o)))
      (is (= 2 (:watch-edits o)))
      (is (= 500 (:todos o)))
      (is (= "2g" (:shadow-heap o)))
      (is (true? (:skip-browser o)))))
  (testing "invalid inputs throw"
    (is (thrown? Exception (gb/parse-options ["--only" "bogus"])))
    (is (thrown? Exception (gb/parse-options ["--runs" "0"])))
    (is (thrown? Exception (gb/parse-options ["--shadow-heap" "nope"])))
    (is (thrown? Exception (gb/parse-options ["--unknown"])))))

(deftest parse-size-report-test
  (let [sample (str "[:bench] Compiling ...\n"
                    "advanced: runtime slice, 1126 of 2138 units, 134,537 unit bytes, 134,818 runtime bytes\n"
                    "constant pool: 15 entries (4 composite, 0 lazy), 780 bytes\n")
        parsed (gb/parse-size-report sample)]
    (is (= 1126 (:units-kept parsed)))
    (is (= 2138 (:units-total parsed)))
    (is (= 134537 (:unit-bytes parsed)))
    (is (= 134818 (:runtime-bytes parsed)))
    (is (= 15 (:const-entries parsed)))
    (is (= 4 (:const-composite parsed)))
    (is (= 0 (:const-lazy parsed)))
    (is (= 780 (:const-bytes parsed))))
  (is (nil? (gb/parse-size-report "unrelated stderr text"))))

(deftest math-helpers-test
  (testing "scaling-exponent"
    (is (< (Math/abs (- 1.0 (gb/scaling-exponent 50000 2.0 200000 8.0))) 1e-9))
    (is (< (Math/abs (- 2.0 (gb/scaling-exponent 50000 2.0 200000 32.0))) 1e-9))
    (is (nil? (gb/scaling-exponent 50000 0 200000 8.0))))
  (testing "geomean"
    (is (< (Math/abs (- 1.0 (gb/geomean [0.5 2.0]))) 1e-9))
    (is (< (Math/abs (- 2.0 (gb/geomean [1.0 2.0 4.0]))) 1e-9))
    (is (nil? (gb/geomean []))))
  (testing "ratio and delta"
    (is (= 0.5 (gb/ratio 5 10)))
    (is (nil? (gb/ratio 5 0)))
    (is (= "-20.0%" (gb/fmt-delta 80 100)))
    (is (= "+25.0%" (gb/fmt-delta 125 100)))))

(deftest summarize-ops-runs-test
  (let [runs [{:tool :light :load [1.2] :rows {"a" {:ns-per-op 10.0 :checksum "1"}
                                               "b" {:ns-per-op 200.0 :checksum "2"}}}
              {:tool :shadow :load [1.3] :rows {"a" {:ns-per-op 20.0 :checksum "1"}
                                                "b" {:ns-per-op 100.0 :checksum "2"}}}]
        s (gb/summarize-ops-runs runs ["a" "b"] nil)]
    (is (= 2 (:count s)))
    (is (= 1 (:parity-count s)))
    (is (< (Math/abs (- 1.0 (:geomean s))) 1e-9))
    (is (= ["b"] (mapv :op (:top-slower s))))
    (is (= 2.0 (:ratio (first (:top-slower s)))))))

(deftest scorecard-gaps-and-markdown-test
  (let [mk-res
        (fn [light-clean light-leaf-watch light-todomvc-gz light-ext-ns]
          {:schema-version 1
           :environment {:light-cljs {:binary "/tmp/light-cljs-abcd1234" :commit "abcd1234"
                                      :sha256 "0123456789abcdef0123456789abcdef"}
                         :shadow-cljs "3.5.3" :jdk "openjdk 21" :node "v22"
                         :babashka "1.12" :os "macOS" :date "2026-11-20"
                         :machine {:cpu "Apple M3" :cpus 24 :memory-gib 32}}
           :options {:shadow-mode :fresh}
           :slices [:compile :watch :size :runtime]
           :total-elapsed-s 300.0
           :compile {:elapsed-s 80.0 :expected-checksum "1 2"
                     :light {:clean {:wall-s {:median light-clean} :cpu-s {:median 20.0}
                                     :peak-memory-gb {:median 0.45}
                                     :stats-median {"time.resolve-parse-ms" 500 "time.analysis-ms" 4000
                                                    "time.deferred-units-ms" 200 "time.prepare-application-ms" 300
                                                    "time.emission-ms" 800 "time.write-ms" 10}}
                             :deterministic? true :clean-hashes ["deadbeef0123456789" "deadbeef0123456789"]
                             :cache-fill {:wall-s {:median 8.5} :cpu-s {:median 21.0}
                                          :peak-memory-gb {:median 0.48} :overhead-s 0.5 :disk-bytes 25000000}
                             :warm-unchanged {:wall-s {:median 1.1} :cpu-s {:median 1.5} :peak-memory-gb {:median 0.25}}
                             :warm-leaf {:wall-s {:median 1.3} :cpu-s {:median 1.8} :peak-memory-gb {:median 0.26}}
                             :warm-decl {:wall-s {:median 1.5} :cpu-s {:median 2.0} :peak-memory-gb {:median 0.27}}
                             :scaling {:lines-50k 50000 :lines-200k 200000 :wall-50k-s 2.0 :wall-200k-s light-clean
                                       :wall-exp 1.0 :cpu-exp 1.0 :mem-exp 0.9}}
                     :shadow {:shadow-heap "1536m" :reused false
                              :clean {:wall-s {:median 28.0} :cpu-s {:median 80.0} :peak-memory-gb {:median 1.6}}
                              :warm-unchanged {:wall-s {:median 10.0} :cpu-s {:median 25.0} :peak-memory-gb {:median 1.1}}
                              :warm-leaf {:wall-s {:median 14.0} :cpu-s {:median 35.0} :peak-memory-gb {:median 1.2}}}}
           :watch {:elapsed-s 90.0 :watch-edits 3
                   :scenario-order ["view (leaf): app.baga.views"]
                   :light {:startup-ms 9000.0 :startup-cpu-s 22.0 :page-load-ms 2500.0
                           :startup-peak-bytes 500000000 :steady-peak-bytes 350000000
                           :scenarios {"view (leaf): app.baga.views"
                                       {:running-ms {:median light-leaf-watch} :step-ms {:median 40.0}
                                        :noticing-ms {:median 200.0} :output-to-msg-ms {:median 10.0}
                                        :msg-to-running-ms {:median 50.0} :cpu-s {:median 0.1}}}}
                   :shadow {:startup-ms 35000.0 :startup-cpu-s 90.0 :page-load-ms 6000.0
                            :startup-peak-bytes 1800000000 :steady-peak-bytes 1500000000
                            :scenarios {"view (leaf): app.baga.views"
                                        {:running-ms {:median 450.0} :step-ms {:median 120.0}}}}}
           :size {:elapsed-s 60.0
                  :apps {"todomvc" {:light {:app {:gzip light-todomvc-gz} :total {:raw 200000 :gzip 70000 :brotli 58000}
                                            :es2015 {:raw 205000 :gzip 71000 :brotli 59000}
                                            :size-report {:units-kept 900 :units-total 2138 :runtime-bytes 120000}}
                                    :shadow {:app {:gzip 50000} :total {:raw 180000 :gzip 60000 :brotli 50000}}}}}
           :runtime {:elapsed-s 70.0
                     :startup {:light {:compile-ms {:median 120.0} :runtime-ms {:median 10.0}
                                       :top-level-ms {:median 200.0} :load-total-ms {:median 330.0}
                                       :first-iter-ms {:median 50.0} :total-ms {:median 380.0}}
                               :shadow {:compile-ms {:median 150.0} :top-level-ms {:median 250.0}
                                        :load-total-ms {:median 400.0} :first-iter-ms {:median 60.0}
                                        :total-ms {:median 460.0}}}
                     :node-200k {:counts {1 {:light {:first-output-s {:median 0.5} :wall-s {:median 0.52} :peak-memory-gb {:median 0.25}}
                                             :shadow {:first-output-s {:median 0.6} :wall-s {:median 0.62} :peak-memory-gb {:median 0.28}}}
                                          10 {:light {:first-output-s {:median 1.0} :wall-s {:median 1.02} :peak-memory-gb {:median 0.30}}
                                              :shadow {:first-output-s {:median 1.1} :wall-s {:median 1.12} :peak-memory-gb {:median 0.32}}}}
                                 :steady-ms-per-iter {:light 55.0 :shadow 55.0}}
                     :todomvc {:todos 1000 :behavior-checks {:passed 9 :total 9 :errors []}
                               :light {:steps {:load {:ms {:median 200.0} :script-ms {:median 100.0}}
                                               :add {:ms {:median 800.0} :script-ms {:median 600.0}}
                                               :toggleAll {:ms {:median 120.0} :script-ms {:median 80.0}}
                                               :edit {:ms {:median 50.0} :script-ms {:median 30.0}}
                                               :clearCompleted {:ms {:median 40.0} :script-ms {:median 20.0}}}
                                       :heap-after-load-bytes {:median 6000000} :heap-after-add-bytes {:median 14000000}}
                               :shadow {:steps {:load {:ms {:median 180.0} :script-ms {:median 90.0}}
                                                :add {:ms {:median 900.0} :script-ms {:median 700.0}}
                                                :toggleAll {:ms {:median 140.0} :script-ms {:median 90.0}}
                                                :edit {:ms {:median 55.0} :script-ms {:median 35.0}}
                                                :clearCompleted {:ms {:median 45.0} :script-ms {:median 25.0}}}
                                        :heap-after-load-bytes {:median 5500000} :heap-after-add-bytes {:median 15000000}}}
                     :core-ops {:count 1 :parity-count 1 :geomean 0.75 :median-ratio 0.75
                                :groups [{:group "maps" :count 1 :parity 1 :geomean 0.75 :max-ratio 0.75}]
                                :top-slower [] :all-slower []}
                     :extend-ops {:count 1 :parity-count 0 :geomean 1.5 :median-ratio 1.5
                                  :ops ["ext-host-seq-nth"]
                                  :by-op {"ext-host-seq-nth" {:light {:median light-ext-ns} :shadow {:median 180.0}
                                                              :ratio (/ light-ext-ns 180.0)}}
                                  :top-slower [{:op "ext-host-seq-nth" :light {:median light-ext-ns}
                                                :shadow {:median 180.0} :ratio (/ light-ext-ns 180.0)}]
                                  :all-slower [{:op "ext-host-seq-nth" :light {:median light-ext-ns}
                                                :shadow {:median 180.0} :ratio (/ light-ext-ns 180.0)}]}}})
        base (mk-res 8.0 300.0 60000 2000.0)
        now (mk-res 7.0 300.0 55000 1000.0)
        gaps (gb/extract-gaps now)
        md (gb/render-markdown now "/tmp/prev.edn" base)]
    (is (= :extend-op-ext-host-seq-nth (:id (first gaps))))
    (is (some #(= :size-todomvc-app-gz (:id %)) gaps))
    (is (str/includes? md "## Executive Scorecard"))
    (is (str/includes? md "## Remaining Gaps Ranked"))
    (is (str/includes? md "Δ light"))
    (is (str/includes? md "-12.5%"))
    (is (str/includes? md "ext-host-seq-nth"))
    (is (= [1.5 3.2] (gb/collect-load-range {:counts (sorted-map 1 {:load-1m {:min 1.5 :max 2.1}}
                                                                 6 {:load-1m {:min 2.0 :max 3.2}})})))))

(let [{:keys [fail error]} (run-tests 'gap-bench.test)]
  (when (pos? (+ fail error))
    (System/exit 1)))
