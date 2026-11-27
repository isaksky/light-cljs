(ns pprint-bench.pprint-test
  (:require [cljs.test :refer [deftest is]]
            [cljs.pprint :refer [pprint]]))

(deftest pprint-basic-test
  (is (= "{:a 1, :b [0 1 2]}\n" (with-out-str (pprint {:a 1 :b [0 1 2]})))))
