(ns global.probe)
(refer-global :only '[Date])
(defn run [observe]
  (assert (identical? Date js/Date))
  (assert (= 0 (.getTime (Date. 0))))
  true)
run
