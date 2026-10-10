(ns batch.macros)

;; Qualify emitted runtime operations explicitly: the macro executes in Clojure.
(defmacro append-item [m k v]
  `(let [m# ~m k# ~k]
     (cljs.core/assoc m# k# (cljs.core/conj (k# m#) ~v))))

(defmacro initial-data []
  '(do (js/console.log "data") {:items [1 2] [1 2] :pair}))

(defmacro literal-data []
  '(quote [:generated batch/data {:nested (x [1 2])} #{:a :b}]))
