(ns app.state
  "Application state. `defonce` keeps the atom, and the clicks it counts,
  when a reload runs this namespace again.")

(defonce state (atom {:clicks 0 :card nil}))

(defn click!
  "Counts one click."
  []
  (swap! state update :clicks inc))
