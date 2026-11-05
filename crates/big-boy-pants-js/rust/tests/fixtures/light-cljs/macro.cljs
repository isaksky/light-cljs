(defmacro twice [form] `(+ ~form ~form))
(twice 21)
