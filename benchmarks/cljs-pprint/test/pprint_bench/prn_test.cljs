(ns pprint-bench.prn-test
  (:require [cljs.test :refer [deftest is]]))

(deftest prn-basic-test
  (is (= "{:a 1, :b [0 1 2]}" (pr-str {:a 1 :b [0 1 2]}))))
