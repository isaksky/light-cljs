(ns app.helper)

(js/console.log "helper loads")

(def loads (atom 0))

(swap! loads inc)

(def calls (atom 0))

(defn shout [s]
  (swap! calls inc)
  (str s "!" @calls " after " @loads " load"))
