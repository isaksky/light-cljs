(ns probe.main (:require [cljs.reader-test] [cljs.test :as test]))
(defmethod test/report [::test/default :end-run-tests] [m]
  (set! (.-readerReferenceResults js/globalThis) (clj->js m)))
(defn run []
  (test/run-tests 'cljs.reader-test)
  (.-readerReferenceResults js/globalThis))
