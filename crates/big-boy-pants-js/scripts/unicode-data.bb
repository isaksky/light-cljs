#!/usr/bin/env bb
;; Generates src/unicode_data.goose, the Unicode 17 tables of the String case
;; and normalization methods (light-cljs-905), from the Pants engine's pinned
;; data: rust/src/value/string_data.rs (full upper and lower case mappings,
;; full canonical and compatibility decompositions, canonical combining
;; classes and canonical compositions, generated from QuickJS's libunicode
;; tables; rust/src/value/string_provenance.txt) and three binary properties
;; of rust/src/regexp/property_data.rs (Cased, Case_Ignorable, Soft_Dotted).
;; The tables are static `let` data (decision 6 of
;; docs/javascript-coverage.md), never heap rows.
;;
;; Usage: bb unicode-data.bb [--check]   (scripts/regenerate.bb runs it)
(require '[babashka.fs :as fs]
         '[clojure.string :as str])

(def directory (fs/parent (fs/canonicalize *file*)))
(def root (fs/parent directory))
(def check? (= ["--check"] (vec *command-line-args*)))
(assert (or check? (empty? *command-line-args*)) "Usage: bb unicode-data.bb [--check]")
(def output (fs/path root "src/unicode_data.goose"))

(defn hex [s] (Long/parseLong (str/replace s #"^0x" "") 16))
(defn table-lines
  "The lines of `static NAME` in a Rust data file, between its header and `];`."
  [lines header]
  (let [body (rest (drop-while #(not (str/starts-with? % header)) lines))]
    (assert (seq body) (str "Missing table " header))
    (take-while #(not= "];" %) body)))

(def data (str/split-lines (slurp (str (fs/path root "rust/src/value/string_data.rs")))))
(assert (some #(str/includes? % "Unicode 17.0.0") (take 3 data)) "string_data.rs is not Unicode 17.0.0")
(defn mapping [name]
  (vec (for [line (table-lines data (str "pub(super) static " name ":"))]
         (let [[_ code run] (re-matches #"\((0x[0-9A-F]+),&\[([0-9A-Fx,]+)\]\)," line)]
           (assert code (str name ": " line))
           [(hex code) (mapv hex (str/split run #","))]))))
(def classes
  (vec (for [line (table-lines data "pub(super) static CCC:")]
         (let [[_ code class] (re-matches #"\((0x[0-9A-F]+),([0-9]+)\)," line)]
           (assert code line)
           [(hex code) (parse-long class)]))))
(def compositions
  (vec (for [line (table-lines data "pub(super) static COMPOSE:")]
         (let [[_ a b c] (re-matches #"\(\((0x[0-9A-F]+),(0x[0-9A-F]+)\),(0x[0-9A-F]+)\)," line)]
           (assert a line)
           [(hex a) (hex b) (hex c)]))))

(def properties (slurp (str (fs/path root "rust/src/regexp/property_data.rs"))))
(assert (str/includes? properties "Unicode 17.0.0") "property_data.rs is not Unicode 17.0.0")
(defn property [aliases]
  (let [[_ ranges] (re-find (re-pattern (str "aliases: \"" aliases "\",\\s+group: PropertyGroup::Binary,\\s+ranges: (RANGES_[0-9]+),")) properties)
        _ (assert ranges (str "Missing property " aliases))
        [_ body] (re-find (re-pattern (str "(?s)\nconst " ranges ": &\\[\\(u32, u32\\)\\] = &\\[\n(.*?)\n\\];")) properties)]
    (assert body (str "Missing ranges " ranges))
    (vec (for [[_ a b] (re-seq #"\((0x[0-9A-F]+), (0x[0-9A-F]+)\)" body)] [(hex a) (hex b)]))))

(defn sorted-unique! [name keys]
  (assert (and (apply < keys) (every? #(<= 0 % 0x10ffff) (flatten [keys]))) (str name " keys are not sorted and unique")))
(defn ranges! [name ranges]
  (assert (every? (fn [[a b]] (<= 0 a b 0x10ffff)) ranges) name)
  (assert (every? (fn [[[_ b] [c _]]] (< (inc b) c)) (partition 2 1 ranges)) (str name " ranges overlap or touch")))

;; string_case.goose's normalize returns its receiver when every unit lies
;; below U+0300 (NFC), U+00C0 (NFD) or U+00A0 (NFKD and NFKC): no code point
;; below those has a decomposition, a combining class or a part in a
;; composition, and each canonical decomposition below U+0300 composes back.
(let [nfd (mapping "NFD") pairs (into {} (map (fn [[a b c]] [[a b] c]) compositions))]
  (assert (>= (ffirst nfd) 0xc0) "an NFD row below U+00C0")
  (assert (>= (ffirst (mapping "NFKD")) 0xa0) "an NFKD row below U+00A0")
  (assert (>= (ffirst classes) 0x300) "a combining class below U+0300")
  (assert (every? (fn [[_ b _]] (>= b 0x300)) compositions) "a composition's second code point below U+0300")
  (doseq [[code run] nfd :when (< code 0x300)]
    (assert (= code (reduce (fn [a b] (get pairs [a b])) run)) (format "U+%04X does not compose back" code))))

(defn row [values] (str/join ", " values))
(defn hex-text [n] (format "0x%X" n))
(defn emit-array [out name type values]
  (.append out (str "const " name ": " type "[" (count values) "] = [\n"))
  (doseq [chunk (partition-all 12 values)] (.append out (str "    " (row chunk) ",\n")))
  (.append out "];\n"))
(defn emit-mapping [out name comment table]
  (let [codes (mapv first table)
        starts (reductions + 0 (map (comp count second) table))
        run (vec (mapcat second table))]
    (sorted-unique! name codes)
    (assert (every? seq (map second table)) name)
    (assert (< (last starts) 65536) (str name " run exceeds u16 offsets"))
    (.append out (str "\n// " comment "\n"))
    (emit-array out (str name "_CODES") "u32" (map hex-text codes))
    (emit-array out (str name "_STARTS") "u16" starts)
    (emit-array out (str name "_RUN") "u32" (map hex-text run))))
(defn emit-property [out name comment ranges]
  (ranges! name ranges)
  (.append out (str "\n// " comment "\n"))
  (emit-array out (str name "_FIRST") "u32" (map (comp hex-text first) ranges))
  (emit-array out (str name "_LAST") "u32" (map (comp hex-text second) ranges)))

(def text
  (let [out (StringBuilder.)
        license (str/split-lines (slurp (str (fs/path root "rust/docs/UNICODE-LICENSE"))))]
    (.append out "// Generated by scripts/unicode-data.bb from the Pants engine's Unicode 17.0.0\n")
    (.append out "// data (rust/src/value/string_data.rs, rust/src/regexp/property_data.rs;\n")
    (.append out "// rust/src/value/string_provenance.txt); do not edit. Regenerate with\n")
    (.append out "// `bb crates/big-boy-pants-js/scripts/regenerate.bb`.\n//\n")
    (.append out "// The tables are derived from the Unicode Character Database, used under\n")
    (.append out "// the Unicode license (rust/docs/UNICODE-LICENSE), reproduced here:\n//\n")
    (doseq [line license] (.append out (str/trimr (str "// " line "\n"))) (.append out "\n"))
    (.append out "\nnamespace unicode;\n")
    (.append out "\n// A mapping table NAME: code point NAME_CODES[i] (ascending) maps to the\n")
    (.append out "// code points NAME_RUN[NAME_STARTS[i]..NAME_STARTS[i + 1]].\n")
    (emit-mapping out "UPPER" "Full uppercase mapping (UnicodeData plus SpecialCasing's unconditional rows)." (mapping "UPPER"))
    (emit-mapping out "LOWER" "Full lowercase mapping (UnicodeData plus SpecialCasing's unconditional rows)." (mapping "LOWER"))
    (emit-mapping out "NFD" "Full canonical decomposition (Hangul syllables are algorithmic, not here)." (mapping "NFD"))
    (emit-mapping out "NFKD" "Full compatibility decomposition (Hangul syllables are algorithmic, not here)." (mapping "NFKD"))
    (sorted-unique! "CLASS" (mapv first classes))
    (assert (every? #(< 0 (second %) 256) classes))
    (.append out "\n// Nonzero canonical combining classes: CLASSES[i] of CLASS_CODES[i] (ascending).\n")
    (emit-array out "CLASS_CODES" "u32" (map (comp hex-text first) classes))
    (emit-array out "CLASSES" "u8" (map second classes))
    (let [keys (mapv (fn [[a b _]] (+ (* a 0x200000) b)) compositions)]
      (assert (every? #(<= 0 % 0x10ffff) (flatten compositions)) "COMPOSE code point overflow")
      (assert (apply < keys) "COMPOSE pairs are not sorted and unique")
      (.append out "\n// Canonical compositions, exclusions removed (Hangul is algorithmic):\n")
      (.append out "// the pair (first, second) as first * 0x200000 + second (ascending)\n")
      (.append out "// composes to COMPOSED[i].\n")
      (emit-array out "COMPOSE_PAIRS" "u64" (map hex-text keys))
      (emit-array out "COMPOSED" "u32" (map (comp hex-text #(nth % 2)) compositions)))
    (.append out "\n// Binary properties as ascending, disjoint inclusive ranges\n")
    (.append out "// [NAME_FIRST[i], NAME_LAST[i]].\n")
    (emit-property out "CASED" "Cased (Final_Sigma and the case methods' context)." (property "Cased"))
    (emit-property out "CASE_IGNORABLE" "Case_Ignorable." (property "Case_Ignorable,CI"))
    (emit-property out "SOFT_DOTTED" "Soft_Dotted (the Lithuanian rules)." (property "Soft_Dotted,SD"))
    (str out)))

(if check?
  (assert (= text (when (fs/exists? output) (slurp (str output)))) "src/unicode_data.goose differs; run bb scripts/regenerate.bb")
  (spit (str output) text))
(println (str "Unicode 17.0.0 String tables " (if check? "verified" "written") ": " (fs/relativize root output)))
