(ns shared.logic
  #?(:cljs (:require-macros [shared.logic]))
  (:require #?@(:clj [[java.not-a-real-namespace :as jvm]] :cljs [[shared.leaf :as leaf]])))
#?(:clj (def value (java.time.Instant/now))
   :cljs (def value (+ leaf/value #?(:other 0 :default 2))))
#?(:cljs (js/console.log "logic"))
