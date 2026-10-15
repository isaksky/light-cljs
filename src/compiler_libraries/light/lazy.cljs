(ns light.lazy
  "Code a chunked build may load later (docs/code-splitting.md, \"The loading
  API\"). `(light.lazy/loadable app.editor/start)`, or `(light.lazy/loadable
  app.editor)` for a namespace, is a special form the compiler turns into a
  `Loadable`, with or without a require of this namespace. In one output file
  every target is resident: a loadable is always loaded. In a chunked build a
  loadable whose target is in a chunk that may not have evaluated yet imports
  that chunk when `load` first runs; the compiler gives it that `load`.")

(def Loadable
  "The type of a loadable, `(Loadable. importer getter)`: `importer` loads the
  chunk holding the target (nil while the target is resident) and `getter`
  reads the target's current value. It implements IDeref: `@l` is the target's
  current value, and throws while the target is not loaded. The runtime owns the
  type, so that a program pays only for what it uses of it."
  (light.lazy/loadable-type))

(defn loaded?
  "Whether the target of loadable `l` is loaded, so that `@l` returns its value."
  [l]
  (nil? (.-importer l)))

(defn load
  "A promise of the current value of the target of loadable `l` once it is
  loaded. It is always asynchronous: a resident target resolves on the next
  microtask, as a chunk already loaded would."
  [l]
  (.load l))
