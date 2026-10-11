;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See research/clojurescript/epl-v10.html; retain this notice.
(ns cljs.letfn-test)

(defn run [] (do (do (letfn [(ev? [x]
            (if (zero? x)
              true
              (od? (dec x))))
          (od? [x]
            (if (zero? x)
              false
              (ev? (dec x))))]
    (assert (ev? 0))
    (assert (ev? 10))
    (assert (not (ev? 1)))
    (assert (not (ev? 11)))
    (assert (not (od? 0)))
    (assert (not (od? 10)))
    (assert (od? 1))
    (assert (od? 11))) true)))
