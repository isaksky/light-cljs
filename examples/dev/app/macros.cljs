(ns app.macros
  "A macro and the helper function its expansion calls.")

(defn bracket
  "`text` in brackets."
  [text]
  (str "[" text "]"))

(defmacro labeled
  "The string of `label`, a colon and the bracketed `body`."
  [label & body]
  `(str ~label ": " (bracket (str ~@body))))
