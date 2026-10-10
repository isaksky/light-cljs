(ns shared.leaf)
(def value #?(:clj java.lang.Integer/MAX_VALUE :cljs 5))
(js/console.log "leaf")
