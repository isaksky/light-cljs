(ns light.lazy
  "Code a chunked build may load later (docs/code-splitting.md, \"The loading
  API\"). `(light.lazy/loadable app.editor/start)`, or `(light.lazy/loadable
  app.editor)` for a namespace, is a special form the compiler turns into a
  `Loadable`, with or without a require of this namespace. Every target is
  resident in the one output file for now: a loadable is always loaded.")

(deftype Loadable [importer getter]
  ;; `importer` loads the chunk holding the target (nil while the target is
  ;; resident); `getter` reads the target's current value.
  IDeref
  (-deref [_]
    (if (nil? importer)
      (getter)
      (throw (js/Error. "light.lazy: the target of this loadable is not loaded")))))

(defn loaded?
  "Whether the target of loadable `l` is loaded, so that `@l` returns its value."
  [l]
  (nil? (.-importer l)))

(defn load
  "A promise of the current value of the target of loadable `l` once it is
  loaded. It is always asynchronous: a resident target resolves on the next
  microtask, as a chunk already loaded would."
  [l]
  (.then (js/Promise.resolve) (fn [_] (deref l))))
