(require '[babashka.fs :as fs] '[clojure.string :as s])
(def path (fs/file (fs/parent *file*) "project/src/js_runtime.js"))
(def text (slurp path))
(def start (s/index-of text "  //@unit privateStore\n"))
(def end (s/index-of text "  //@unit iteratorSeqs\n" start))
(defn store [name]
  (str "  // A separate private brand and method body lets engines specialize this store.\n"
       "  const " name " = class extends (function (value) { return value; }) {\n"
       "    #s;\n"
       "    static has(value) { return (typeof value === \"object\" ? value !== null : typeof value === \"function\") && #s in value; }\n"
       "    static get(value) { if ((typeof value === \"object\" ? value !== null : typeof value === \"function\") && #s in value) return value.#s; }\n"
       "    static set(value, payload) { if (#s in value) value.#s = payload; else new " name "(value).#s = payload; }\n"
       "  };"))
(spit path (-> (str (subs text 0 start) "  //@unit cells\n" (store "cells")
                         "\n  //@unit views\n" (store "views") "\n" (subs text end))
               (s/replace "  //@unit lazyValues requires: privateStore\n  const lazyValues = privateStore(), delays = new WeakMap();"
                          (str "  //@unit lazyValues\n" (store "lazyValues") "\n  const delays = new WeakMap();"))
               (s/replace "  //@unit chunks requires: privateStore" "  //@unit chunks")
               (s/replace "  const chunks = privateStore(), chunkBuffers = new WeakMap();"
                          (str (store "chunks") "\n  const chunkBuffers = new WeakMap();"))))
(assert (not (s/includes? (slurp path) "privateStore")))
(println "Specialized four private stores; brand and replacement semantics retained")
