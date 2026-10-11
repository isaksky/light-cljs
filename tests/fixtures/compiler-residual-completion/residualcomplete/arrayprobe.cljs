;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See research/clojurescript/epl-v10.html; retain this notice.
(ns residualcomplete.arrayprobe (:require [cljs.array-access-test :refer [suppress-errs]]))
(defn run [] (let [previous *print-err-fn* result (suppress-errs (nil? *print-err-fn*))] [result (identical? previous *print-err-fn*)]))
