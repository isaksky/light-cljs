(ns order.shared)
(js/console.log "shared")
(def value 7)
(def read-value (fn* [] value))
