(ns shared.logic
  (:require #?@(:clj [[java.not-a-real-namespace :as jvm]]
                :cljs [[shared.leaf :as leaf] [shared.helper :as h]])))
#?(:clj (def value (java.time.Instant/now))
   :cljs (def value (+ leaf/value #?(:other 0 :default 2))))
#?(:cljs (js/console.log "logic"))

(defmacro add [x] `(cljs.core/+ ~x ~(h/offset)))
(defmacro keyword-name [x] (str x))
