#!/usr/bin/env cljs
(ns example.reader-forms)
; A lexical fixture: reader/evaluation semantics are intentionally not checked.
(def ^:private café "λ says \"hello\"\n\u03bb\141")
(def pattern #"[a-z]+\s*\d{2}[()]")
(def chars [\space \tab \newline \backspace \formfeed \return \u03bb \o141 \λ \) \; \\ \" ])
(def values {:count 42 :ratio 3/4 :hex 0xFF :radix 2r101 :decimal 1.5M :big 12N
             :truth true :nothing nil :auto ::local :unicode 你好})
(def names #{alpha foo# foo' foo:bar ns/name})
(def quote-forms ['café `(~x ~@xs) @state #'example/value])
(def f #(vector % %1 %2 %&))
(def objects [#js {:a #js [1 2]} #inst "2026-01-01" #uuid "00000000-0000-0000-0000-000000000000" #app/tag {}])
(def maps [#:example{:x 1} #::example{:x 2} #::{:x 3}])
(def special [##Inf ##-Inf ##NaN])
#_#_(discarded form) [discarded-too] :retained
#?(:cljs (def platform :cljs) :clj (def platform :clj))
[#?@(:cljs [:one :two] :clj [:three])]
#^String legacy-metadata
#=(reader-evaluation-is-not-executed)
