(ns state.store)
(def current (atom {:n 0} :meta {:label "counter"} :validator map?))
(def temporary (volatile! 10))
(def change (let [a current] (fn [& xs] (apply swap! a update :n + xs))))
