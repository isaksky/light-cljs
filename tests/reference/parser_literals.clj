;; Run from the repository root:
;; clojure -Sdeps '{:paths ["research/clojurescript/src/main/clojure"]}' -M tests/reference/parser_literals.clj
;; Compare literal syntax and decoded UTF-16 values, not macro expansion.
(require '[cljs.vendor.clojure.tools.reader :as reader]
         '[cljs.vendor.clojure.tools.reader.reader-types :as rt]
         '[clojure.string :as str])

(def cases
  (concat
   ["nil" "true" "false" "+" "-" "/" "ns//" "foo'" "foo#" "x:kw"
    "café" "λ" "😀" "a/" "/a" "a/b/c" "a/1" "a:"
    ":x" ":ns/name" ":1" ":/" ":a:b" ":" "::" ":::x" ":a:" ":a/1"
    "0" "-0" "+42" "077" "0xff" "0XFE" "2r101" "36rN" "36r10N"
    "9999999999999999999999999999" "42N" "0xFFN" "077N"
    "1.0" "2." "2e3" "-1E-3" "08.0" "1M" "1.25M" "1e-10M" "09M"
    "1/2" "1/1" "-13/2" "+01/002" "1x" "09" "09N" "0x" "0xG"
    "1e" "2e+" "1.2.3" "2r102" "2r10N" "1r0" "37r0" "1/0"
    "1/000" "1/-2" "١" "1Nf"]
   (map #(str "\\" %) ["λ" ")" "space" "newline" "return" "o141" "u03bb"
                       "\r\n" "uＦＦ４１" "unknown" "uD800" "u123" "u12345"
                       "o400" "o7777" "😀"])
   (map #(str "\"" % "\"") ["café λ 😀" "a\r\nb\r\fc\rd" "\\t\\r\\n\\\\\\\"\\b\\f\\u03bb\\141"
                           "\\1 \\12 \\1234" "\\uＦＦ４１\\u٠٠٤١" "\\uD800\\uDC00\\uDFFF"
                           "\\q" "\\u123" "\\u12zz" "\\400" "\\8" "\\12a"])))

(defn hex-source [s]
  (apply str (map #(format "%02x" (bit-and 255 %)) (.getBytes s "UTF-8"))))

(defn code-units [s]
  (str/join "," (map #(format "%04x" (int %)) s)))

(println "# source_utf8_hex\tcategory\tdecoded_utf16_hex")
(doseq [source cases]
  (let [[category value]
        (try
          (binding [reader/*read-eval* false]
            (let [r (rt/indexing-push-back-reader source)
                  x (reader/read r)
                  category (cond (nil? x) "nil" (boolean? x) "boolean"
                                 (symbol? x) "symbol" (keyword? x) "keyword"
                                 (number? x) "number" (string? x) "string"
                                 (char? x) "character" :else "other")]
              (assert (= ::eof (reader/read {:eof ::eof} r)))
              [category (if (or (string? x) (char? x)) (code-units (str x)) "-")]))
          (catch Exception _ ["error" "-"]))]
    (println (str (hex-source source) "\t" category "\t" value))))
