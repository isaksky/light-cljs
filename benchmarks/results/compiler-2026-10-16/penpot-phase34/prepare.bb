(require '[babashka.fs :as fs] '[clojure.string :as str])
(def here (fs/parent *file*))
(def runtime-text (slurp "src/js_runtime.js"))
(spit (fs/file here "runtime-baseline.js") runtime-text)
(def operations [["seq" "ISeqable/-seq"] ["first" "ISeq/-first"] ["rest" "ISeq/-rest"] ["next" "INext/-next"] ["count" "ICounted/-count"]])
(def updated
  (reduce (fn [text [operation _]]
    (let [anchor (str "  " operation " = behindProtocol(" operation ",")]
      (assert (str/includes? text anchor))
      (str/replace text anchor (str "  const native_" operation " = " operation ";\n" anchor))))
    runtime-text operations))
(def anchor "  //@unit ListValue.members")
(assert (str/includes? updated anchor))
(defn installation [types]
  (str "\n  // Diagnostic only: direct native collection protocol slots.\n"
    "  for (const Type of [" (str/join ", " types) "]) {\n"
    (apply str (for [[operation key] operations]
      (let [protocol (first (str/split key #"/"))]
        (str "    if (!(Type === Cons && '" protocol "' === 'ICounted')) {\n"
             "      const mask = protocolMasks.get('" protocol "');\n"
             "      Type.prototype[mask[0]] = (Type.prototype[mask[0]] || 0) | mask[1];\n"
             "      Object.defineProperty(Type.prototype, protocolSlot('" key "', 1), {value: function (self) {return native_" operation "(self);}, writable: true, configurable: true});\n"
             "    }\n")))) "  }\n"))
(doseq [[name types] [["lists" ["ListValue" "EmptyListValue" "Cons"]]
                     ["collections" ["ListValue" "EmptyListValue" "Cons" "IndexedSeqValue" "ChunkedSeqValue" "NodeSeqType"]]]]
  (spit (fs/file here (str "runtime-" name ".js")) (str/replace updated anchor (str (installation types) anchor))))
(fs/copy "/private/tmp/light-cljs-phase31/replay.js" (fs/file here "replay.js") {:replace-existing true})
(println "Prepared baseline, list and collection protocol diagnostics; production source unchanged.")
