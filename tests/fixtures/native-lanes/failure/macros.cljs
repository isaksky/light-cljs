(ns failure.macros)
(def effects (atom 0))
;; Fails once at compile time, where `process` is undefined, and every
;; later read gets the same cached failure.
(def broken
  (do (swap! effects inc)
      (when (js* "typeof process === 'undefined'") (throw (js/Error. "sticky")))))
(defmacro probe []
  (let [a (try broken (catch :default e e))
        b (try broken (catch :default e e))]
    (when-not (identical? a b) (throw (js/Error. "not cached")))
    (swap! effects + 10)))
