(ns app.view
  "Rendering: a record, a protocol extended to it, and a function using a
  macro. `render` logs the state through console.log, so it runs without a
  DOM."
  (:require [app.macros :refer [labeled]]
            [app.state :as state]))

(defprotocol Describe
  (describe [this] "One line of text for the value."))

(defrecord Card [label clicks])

(extend-protocol Describe
  Card
  (describe [card] (labeled (:label card) (:clicks card))))

(defn render
  "Logs a card of the current state and keeps it in the state."
  []
  (let [card (->Card "clicks" (:clicks @state/state))]
    (swap! state/state assoc :card card)
    (js/console.log (describe card))))
