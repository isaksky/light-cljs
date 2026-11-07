// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 9124-9146,
// loose_equality_with_a_boolean_left_operand (bbp-60), and the loose rows of
// lines 9046-9096, truthiness_and_equality_follow_javascript; plus `null` and
// `undefined` against "" and objects, and object identity. Rows that convert
// a string to a number wait for the StringToNumber pellet. Each row is a
// `row(...)` check; the script completes with the number of rows, or minus
// the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
var o = {}, p = {}, a = [], fn = function () {};
// `null` and `undefined` equal only each other.
row((null == undefined) === true);
row((undefined == null) === true);
row((null == null) === true);
row((undefined == undefined) === true);
row((null != undefined) === false);
row((null == 0) === false);
row((undefined == 0) === false);
row((0 == null) === false);
row((null != 0) === true);
row((null == false) === false);
row((undefined == false) === false);
row((false == null) === false);
row((false == undefined) === false);
row((null == "") === false);
row(("" == undefined) === false);
row((null == o) === false);
row((o == undefined) === false);
row((undefined == a) === false);
row((null == fn) === false);
row((null == NaN) === false);
// A boolean compares as a number.
row((true == 1) === true);
row((1 == true) === true);
row((0 == false) === true);
row((false == 0) === true);
row((false == -0) === true);
row((true == 2) === false);
row((2 == true) === false);
row((true != 1) === false);
row((1 != true) === false);
row((false != 0) === false);
row((true == NaN) === false);
row((true == true) === true);
row((true == false) === false);
row((false != true) === true);
// Numbers: NaN equals nothing, -0 equals 0.
row((NaN == NaN) === false);
row((NaN != NaN) === true);
row((0 == -0) === true);
row((Infinity == Infinity) === true);
row((Infinity == -Infinity) === false);
// Strings compare by content.
row(("a" == "a") === true);
row(("a" == "b") === false);
row(("" == "") === true);
// Objects compare by identity.
row((o == o) === true);
row((o == p) === false);
row((o != p) === true);
row(({} == {}) === false);
row((a == a) === true);
row(([] == []) === false);
row((fn == fn) === true);
row((function () {} == function () {}) === false);
{ let q = o; row((q == o) === true && (q != o) === false); }
// Symbols compare by identity.
row((Symbol.iterator == Symbol.iterator) === true);
row((Symbol.iterator == Symbol.asyncIterator) === false);
failed === 0 ? rows : -failed;
