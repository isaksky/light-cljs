;; Upstream Copyright (c) Rich Hickey; Eclipse Public License 1.0.
;; Pinned a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e; complete test bodies.
(deftest test-regexps
  (testing "Testing regexps"
    (let [r1 #"foo", r2 (re-pattern r1)]
      (is (= r1 r2)))
    (is (= (str (re-pattern "f(.)o")) (str (js* "/f(.)o/"))))
    (is (= (re-find (re-pattern "foo") "foo bar foo baz foo zot") "foo"))
    (is (= (re-find (re-pattern "f(.)o") "foo bar foo baz foo zot") ["foo" "o"]))
    (is (= (re-matches (re-pattern "foo") "foo") "foo"))
    (is (= (re-matches (re-pattern "foo") "foo bar foo baz foo zot") nil))
    (is (= (re-matches (re-pattern "foo.*") "foo bar foo baz foo zot") "foo bar foo baz foo zot"))
    (is (= (re-seq (re-pattern "foo") "foo bar foo baz foo zot") (list "foo" "foo" "foo")))
    (is (= (re-seq (re-pattern "f(.)o") "foo bar foo baz foo zot") (list ["foo" "o"] ["foo" "o"] ["foo" "o"])))
    (is (= (re-matches (re-pattern "(?i)foo") "Foo") "Foo"))
    ; new RegExp("").source => "(?:)" on webkit-family envs, "" elsewhere
    (is (#{"#\"\"" "#\"(?:)\""} (pr-str #"")))
    (is (= (re-find (re-pattern "[\u2028]") " \u2028 ") "\u2028"))))

(deftest test-cljs-2453
  (is (= (re-seq #"[Bc]?" "aBcD") '("" "B" "c" "" "")))
  (is (= (re-seq #"[BcD]?$" "aBcD") '("D" "")))
  (is (= (map first (re-seq #"(\d+)" "ClojureScript 1.9.222")) '("1" "9" "222")))
  (is (= (re-seq #"\d+" "a1b2c3d") '("1" "2" "3")))
  (is (= (re-seq #"\d?" "a1b2c3d") '("" "1" "" "2" "" "3" "" "")))
  (is (= (re-seq #"\d*" "a1b2c3d") '("" "1" "" "2" "" "3" "" "")))
  (is (= (re-seq #"\d+" "a1b22c333d") '("1" "22" "333")))
  (is (= (re-seq #"\d?" "a1b22c333d") '("" "1" "" "2" "2" "" "3" "3" "3" "" "")))
  (is (= (re-seq #"\d*" "a1b22c333d") '("" "1" "" "22" "" "333" "" "")))
  (is (= (re-seq #"\w+" "once upon a time") '("once" "upon" "a" "time")))
  (is (nil? (re-seq #"\w+" ""))))
