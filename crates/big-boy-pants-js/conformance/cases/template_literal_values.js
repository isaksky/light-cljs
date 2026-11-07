// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 13124-13195,
// template_literal_values (bbp-57, the evaluation of untagged templates):
// each substitution converts by ToString, its own instruction, and joins
// the text so far. The rows with an object substitution (ToPrimitive with
// hint string) are left out; tagged templates stay with the lexing area. Infinity and NaN are spelled 1 / 0 and
// 0 / 0. Each row is a `row(...)` check; the script completes with the
// number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
{
  var a, b;
  b = 123;
  a = `abc${b}d`;
  var first = a;
  a = "aaa";
  b = "bbb";
  row(first + "|" + `aaa${a, b}ccc` === "abc123d|aaabbbccc");
}
{ var bar = "Bar"; var baz = `${bar + `a${bar}` }baz`; row(baz === "BaraBarbaz"); }
{ let b = 123; row(`abc${b}d` === "abc123d"); }
row(`${1}${true}${null}${undefined}` === "1truenullundefined");
row(`${false}|${-1}` === "false|-1");
// ToString of numbers is Number::toString; a string stays itself.
row(`${1.5}|${-0}|${1e21}|${0 / 0}|${-1 / 0}|${0.1 + 0.2}|${'s'}` === "1.5|0|1e+21|NaN|-Infinity|0.30000000000000004|s");
row(`${5e-324}|${1e-7}|${123e-20}|${2147483648}` === "5e-324|1e-7|1.23e-18|2147483648");
// No substitutions, empty parts, a substitution alone.
row(`` === "");
row(`plain` === "plain");
row(`${''}${''}` === "");
row(`${42}` === "42");
row(typeof `${1}` === "string");
row(typeof `${"s"}` === "string" && `${"s"}` === "s");
// Escapes are decoded; a line continuation is nothing.
row(`a\tb\x41\`\${x}c\
d` === "a\tbA`${x}cd");
row(`\u{1F600}${1}` === "😀1");
row(`ā${2}Ă` === "ā2Ă");
// Substitutions run left to right, each converted before the next.
{
  let log = "";
  function f(x) { log = log + x; return x; }
  row(`${f(1)}-${f(2)}-${f(3)}` + ":" + log === "1-2-3:123");
}
// Closures and nesting.
{ let x = 1; let g = () => `<${x}>`; x = 2; row(`${g()}${`${g()}`}` === "<2><2>"); }
// A template is a string value like any other.
row(`ab` === "ab");
row(`${1}` + 1 === "11");
// `*` converts both templates by StringToNumber (light-cljs-827).
row(`2` * `${3}` === 6);
failed === 0 ? rows : -failed;
