// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 12780-12827,
// void_operator (bbp-133); lines 13226-13262,
// comma_operator_evaluates_left_to_right (bbp-61); lines 13069-13095,
// expression_evaluation_order_and_conditional_short_circuiting; the logical
// operator rows of lines 9046-9096, truthiness_and_equality_follow_javascript;
// and the `??` rows of rust/tests/fixtures/js/logical_assignment.js lines 2-8.
// Each row is a `row(...)` check; the script completes with the number of
// rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function isNaNValue(v) { return v !== v; }
// `void`: the value is undefined, the operand runs once for its effects.
row(void 0 === undefined);
row(void void 1 === undefined);
{ let n = 0; function f() { n = n + 1; return 7; } let r = void f(); row(r === undefined && n === 1); }
{ let i = 0; void i++; void (i = i + 10); row(i === 11); }
{ let t = "none"; try { t = void (() => { throw "boom"; })(); } catch (e) { t = e; } row(t === "boom"); }
{ let t = "none"; try { void nope; } catch (e) { t = e.name; } row(t === "ReferenceError"); }
row(typeof void 0 === "undefined");
row(isNaNValue(void 1 + 1));
row(!void 0 === true);
row(void "a" === void {});
{ let g = () => void 3; row(g() === undefined); }
// The comma operator: left to right, the last operand's value.
{ let a = 1, b = 2; row((a, b) === 2); }
{ let x = 0; row((x = 1, x + 1) === 2); }
{ let r = (1, 2, 3); row(r === 3); }
{ let r; if (0, 1) r = 7; else r = 8; row(r === 7); }
{ let t = "none"; try { (nope, 1); } catch (e) { t = e.name; } row(t === "ReferenceError"); }
{ let x = 1; let y = (x = x * 10, x = x + 2, x * 2); row(y * 1000 + x === 24012); }
{ let i = 0, n = 0; while (n = n + 1, i < 3) i = i + 1; row(n === 4); }
{ function f(a) { return a = a + 1, a * 2; } row(f(1) === 4); }
{ let g = (a, b) => (a, b); row(g(1, 2) === 2); }
{ let x = 0; try { throw x = 1, 2; } catch (e) { x = x * 10 + e; } row(x === 12); }
{ let h; h = a => a, 9; row(h(3) === 3); }
row((1, null) + 1 === 1);
row((null, 1) + 1 === 2);
row(1 - (true, 2) === -1);
row((1, undefined) === undefined);
// Operands evaluate left to right; assignments have their value.
{ let x = 0; let y = (x = 1) + (x = 2); row(y * 10 + x === 32); }
{ let x = 0; let y = (x = 5) - (x = 2); row(y * 10 + x === 32); }
{ let x = 1; let y = x * (x = 3); row(y * 10 + x === 33); }
{ let a, b; a = b = 4; row(a + b === 8); }
{ let x = 0; let t = (x = 1) < (x = 2); row((t ? x : -1) === 2); }
// Only the selected branch of `?:` runs.
row((true ? 7 : missing) === 7);
row((false ? missing : 8) === 8);
{ let x = 0; 1 ? (x = 1) : (x = 2); row(x === 1); }
{ let x = 0; if (0) x = 1; else x = 2; row(x === 2); }
// `&&`, `||` and `??` return an operand and evaluate the right one only
// when needed.
{ let x = 0; 1 && (x = 3); 0 && (x = 4); 1 || (x = 5); row(x === 3); }
row((0 || null) === null);
row((1 && 2) === 2);
row((0 && nope) === 0);
row((3 || nope) === 3);
row((null && 1) === null);
row((false || 0 || 4) === 4);
row((null ?? 5) === 5);
row((undefined ?? 6) === 6);
row((0 ?? nope) === 0);
row((false ?? nope) === false);
row(("" ?? nope) === "");
row(isNaNValue(NaN ?? 1));
row((null ?? undefined ?? 7) === 7);
row((false || (null ?? 9)) === 9);
row(((false || null) ?? 8) === 8);
row((null ?? (1 && 2)) === 2);
{
  let falsy = [0, -0, NaN, null, undefined, false, ""];
  for (let i = 0; i < falsy.length; i++) row(!falsy[i] === true && (falsy[i] ? 1 : 2) === 2);
  let truthy = [1, -1, 0.5, Infinity, -Infinity, true, "0", {}, []];
  for (let i = 0; i < truthy.length; i++) row(!truthy[i] === false && (truthy[i] ? 1 : 2) === 1);
}
// The order of evaluation, logged in a string.
{
  let log = "";
  function t(s, v) { log += s; return v; }
  t("a", 0) && t("b", 1);
  t("c", 1) && t("d", 1);
  t("e", 1) || t("f", 0);
  t("g", 0) || t("h", 0);
  t("i", null) ?? t("j", 2);
  t("k", 0) ?? t("l", 3);
  t("m", 1) ? t("n", 0) : t("o", 0);
  t("p", 0) ? t("q", 0) : t("r", 0);
  t("s", 0), t("t", 0), t("u", 0);
  void t("v", 0);
  t("w", 1) + t("x", 2) * t("y", 3);
  row(log === "acdeghijkmnprstuvwxy");
}
failed === 0 ? rows : -failed;
