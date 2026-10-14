;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See epl-v10.html at the repository root; retain this notice.
(ns residualcomplete.arrayprobe (:require-macros [cljs.array-access-test :refer [suppress-errs]]))
(defn run [] (let [previous *print-err-fn* result (suppress-errs (nil? *print-err-fn*))] [result (identical? previous *print-err-fn*)]))
