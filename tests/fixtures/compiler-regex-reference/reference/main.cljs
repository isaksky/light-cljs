;; Upstream Copyright (c) Rich Hickey; Eclipse Public License 1.0.
;; Pinned a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e; complete test bodies.
(ns reference.main (:require [clojure.string :as s]))
(defn upstream-contract [] (let [observations (atom [])]
(do (do (let [r1 #"foo", r2 (re-pattern r1)]
      (swap! observations conj (boolean (= r1 r2)))) (swap! observations conj (boolean (= (str (re-pattern "f(.)o")) (str (js* "/f(.)o/"))))) (swap! observations conj (boolean (= (re-find (re-pattern "foo") "foo bar foo baz foo zot") "foo"))) (swap! observations conj (boolean (= (re-find (re-pattern "f(.)o") "foo bar foo baz foo zot") ["foo" "o"]))) (swap! observations conj (boolean (= (re-matches (re-pattern "foo") "foo") "foo"))) (swap! observations conj (boolean (= (re-matches (re-pattern "foo") "foo bar foo baz foo zot") nil))) (swap! observations conj (boolean (= (re-matches (re-pattern "foo.*") "foo bar foo baz foo zot") "foo bar foo baz foo zot"))) (swap! observations conj (boolean (= (re-seq (re-pattern "foo") "foo bar foo baz foo zot") (list "foo" "foo" "foo")))) (swap! observations conj (boolean (= (re-seq (re-pattern "f(.)o") "foo bar foo baz foo zot") (list ["foo" "o"] ["foo" "o"] ["foo" "o"])))) (swap! observations conj (boolean (= (re-matches (re-pattern "(?i)foo") "Foo") "Foo"))) (swap! observations conj (boolean (#{"#\"\"" "#\"(?:)\""} (pr-str #"")))) (swap! observations conj (boolean (= (re-find (re-pattern "[\u2028]") " \u2028 ") "\u2028")))))
(do (swap! observations conj (boolean (= (re-seq #"[Bc]?" "aBcD") '("" "B" "c" "" "")))) (swap! observations conj (boolean (= (re-seq #"[BcD]?$" "aBcD") '("D" "")))) (swap! observations conj (boolean (= (map first (re-seq #"(\d+)" "ClojureScript 1.9.222")) '("1" "9" "222")))) (swap! observations conj (boolean (= (re-seq #"\d+" "a1b2c3d") '("1" "2" "3")))) (swap! observations conj (boolean (= (re-seq #"\d?" "a1b2c3d") '("" "1" "" "2" "" "3" "" "")))) (swap! observations conj (boolean (= (re-seq #"\d*" "a1b2c3d") '("" "1" "" "2" "" "3" "" "")))) (swap! observations conj (boolean (= (re-seq #"\d+" "a1b22c333d") '("1" "22" "333")))) (swap! observations conj (boolean (= (re-seq #"\d?" "a1b22c333d") '("" "1" "" "2" "2" "" "3" "3" "3" "" "")))) (swap! observations conj (boolean (= (re-seq #"\d*" "a1b22c333d") '("" "1" "" "22" "" "333" "" "")))) (swap! observations conj (boolean (= (re-seq #"\w+" "once upon a time") '("once" "upon" "a" "time")))) (swap! observations conj (boolean (nil? (re-seq #"\w+" "")))))
[(count @observations) (every? true? @observations)]))
;; Literal boundary contracts shared with application and compiled macro tests.
(defn regex-contract []
  (let [re (re-pattern "(?im)^a(.)$")
        global (js/RegExp. "a" "g")
        seen (atom [])]
    (and
      (regexp? re) (not (regexp? "a")) (identical? re (re-pattern re))
      (= "im" (.-flags re)) (= "^a(.)$" (.-source re))
      (= ["Ab" "b"] (re-find re "x\nAb\nz"))
      (= ["ab" "b"] (re-matches re "ab")) (nil? (re-matches re "zab"))
      (= ["b" nil "b"] (re-find #"(a)?(b)" "b"))
      (= ["b" nil "b"] (re-matches #"(a)?(b)" "b"))
      (= ["b" nil "b"] (first (re-seq #"(a)?(b)" "b")))
      (= ["" "" ""] (vec (re-seq #"" "ab")))
      (= ["D" ""] (vec (re-seq #"[BcD]?$" "aBcD")))
      (= ["\uD83D" "\uDE00"] (vec (re-seq #"." "\uD83D\uDE00")))
      (= ["\uD83D\uDE00"] (vec (re-seq (re-pattern "(?u).") "\uD83D\uDE00")))
      (= ["" "" ""] (vec (re-seq (re-pattern "(?u)") "\uD83D\uDE00")))
      (= "\uD800" (re-find #"." "\uD800"))
      (= "\uD800" (re-find (re-pattern "(?u).") "\uD800"))
      (= "x" (re-find (re-pattern "(?s).") "x"))
      (= "\n" (re-find (re-pattern "(?s).") "\n"))
      (= "a" (apply re-find [#"a" "a"]))
      (= "a" ((identity re-matches) #"a" "a"))
      (= "a" (re-find global "aba")) (= 1 (.-lastIndex global))
      (= "a" (re-find global "aba")) (= 3 (.-lastIndex global))
      (nil? (re-find global "aba")) (= 0 (.-lastIndex global))
      (every? true? (for [f [re-find re-matches re-seq] x [nil 1 #js {}]]
        (try (f #"." x) false (catch js/TypeError e true))))
      (every? true? (for [pattern ["[" "(?ii)a" "(?x)a"]]
        (try (re-pattern pattern) false (catch js/SyntaxError e true))))
      (try (re-pattern 1) false (catch js/TypeError e true))
      (= "AB-AB" (s/replace "ab-ab" #"(a)(b)" (fn [[whole a b]]
        (swap! seen conj [whole a b]) (s/upper-case whole))))
      (= [["ab" "a" "b"] ["ab" "a" "b"]] @seen)
      (= "a:1" (s/replace "a1" #"(a)(1)" "$1:$2"))
      (= "Xbc" (s/replace-first "abc" #"a" "X"))
      (= ["a" "b"] (s/split "a-b" #"-"))
      (= ["a" "b-c"] (s/split "a-b-c" #"-" 2))
      (= ["a" "b" ""] (s/split "a-b-" #"-" -1)))))
(defn run [] [(upstream-contract) (regex-contract)])
