(ns host.bootstrap)
(set! js/React (js-obj "createElement" (fn [tag] [:element tag])))
