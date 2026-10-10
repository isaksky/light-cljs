(ns reify-events.events (:require [reify-events.api :as p]))
(defn increment-event [amount]
  (let [seen (atom 0)]
    (reify
      p/Marker
      p/UpdateEvent
      (-update [_ {:keys [total] :as state}]
        (swap! seen inc)
        (assoc state :total (+ total amount)))
      p/EffectEvent
      (-effect [self state {:keys [emit]}]
        (emit [amount (:total state) @seen (identical? self self)]))
      p/WatchEvent
      (-watch [self {:keys [total]} stream]
        (fn [] [total stream amount @seen self]))
      Object
      (toString [_] (str "increment:" amount)))))
