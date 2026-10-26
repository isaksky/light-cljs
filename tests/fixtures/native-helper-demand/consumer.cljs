(declare helper)
(fn [x]
  (js* "globalThis.beforeDemand = (globalThis.beforeDemand || 0) + 1")
  (try
    ((fn [] (helper x)))
    (finally (js* "globalThis.afterDemand = (globalThis.afterDemand || 0) + 1"))))
