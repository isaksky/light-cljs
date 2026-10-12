;   Copyright (c) Rich Hickey. All rights reserved.
;   The use and distribution terms for this software are covered by the
;   Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;   which can be found in the file epl-v10.html at the root of this distribution.
;   By using this software in any fashion, you are agreeing to be bound by
;   the terms of this license.
;   You must not remove this notice, or any other, from this software.

; Complete pinned printing_test body adapter.
(ns cljs.printing-test)

(defn run [observe]
(let [utility-failures (atom []) utility-check (fn [identity value & message] (when-not value (swap! utility-failures conj identity)) value)] (do (do (utility-check "src/test/cljs/cljs/printing_test.cljs::test-print-knobs::assertion-001" (= (binding [*print-length* 0] (str [1 2 3 4 5 6 7 8 9 0]))           "[...]")) (utility-check "src/test/cljs/cljs/printing_test.cljs::test-print-knobs::assertion-002" (= (binding [*print-length* 1] (str [1 2 3 4 5 6 7 8 9 0]))           "[1 ...]")) (utility-check "src/test/cljs/cljs/printing_test.cljs::test-print-knobs::assertion-003" (= (binding [*print-length* 2] (str [1 2 3 4 5 6 7 8 9 0]))           "[1 2 ...]")) (utility-check "src/test/cljs/cljs/printing_test.cljs::test-print-knobs::assertion-004" (= (binding [*print-length* 10] (str [1 2 3 4 5 6 7 8 9 0]))           "[1 2 3 4 5 6 7 8 9 0]")) (utility-check "src/test/cljs/cljs/printing_test.cljs::test-print-knobs::assertion-005" (= (binding [*print-length* 10] (str {:foo "bar"}))           "{:foo \"bar\"}")) (utility-check "src/test/cljs/cljs/printing_test.cljs::test-print-knobs::assertion-006" (= (binding [*print-length* 0] (str {:foo "bar" :baz "woz"}))           "{...}")) (utility-check "src/test/cljs/cljs/printing_test.cljs::test-print-knobs::assertion-007" (#{"{:foo \"bar\", ...}" "{:baz \"woz\", ...}"}           (binding [*print-length* 1] (str {:foo "bar" :baz "woz"})))) (utility-check "src/test/cljs/cljs/printing_test.cljs::test-print-knobs::assertion-008" (#{"{:foo \"bar\", :baz \"woz\"}" "{:baz \"woz\", :foo \"bar\"}"}           (binding [*print-length* 10] (str {:foo "bar" :baz "woz"})))))) (if (empty? @utility-failures) true @utility-failures))
)
run
