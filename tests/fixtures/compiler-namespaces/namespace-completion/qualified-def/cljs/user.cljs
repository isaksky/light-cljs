(ns cljs.user)
(def cljs.user/foo 43)
(defn run [observe]
  (assert (= 43 foo cljs.user/foo))
  true)
run
