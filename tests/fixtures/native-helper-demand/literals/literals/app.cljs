(ns literals.app (:require [literals.a :as a] [literals.b :as b]
                           [literals.macros :refer [echo fresh]]))
(js/console.log (pr-str (subvec a/value 0 3)) (pr-str (subvec b/value 0 3))
                (pr-str (subvec (echo :app #js []) 0 3))
                (.charCodeAt (.-value (aget (fresh) 0)) 0))
