(letfn [(encode [v]
 (cond
  (js* "(typeof ~{} === 'undefined')" v) (js-obj "$value" "undefined")
  (nil? v) nil
  (or (boolean? v) (number? v) (string? v)) v
  (instance? js/RegExp v) (js-obj "$regex" (js-obj "source" (.-source v) "flags" (.-flags v)))
  (vector? v) (js-obj "$vector" (into-array (map encode v)))
  (set? v) (js-obj "$set" (into-array (map encode v)) "count" (count v))
  :else (throw (js/Error. "Unsupported complete CLJC transport value"))))]
 (js/JSON.stringify (js-obj "id" ORIGINAL_ID "outcome" (js-obj "status" "returned" "value" (encode original-value)) "trace" (array) "stdout" "")))
