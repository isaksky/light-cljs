// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 15528-15651,
// computed_member_access, with keys from in_operator (bbp-74), and the Pants engine's
// rust/src/vm/property.rs to_property_key: a computed key that is a
// primitive is its ToString, so number, boolean, null and undefined keys are
// read, written, defined, tested with `in`, deleted and passed to
// Object.getOwnPropertyDescriptor under their canonical
// names; array indices keep their element path. Infinity and NaN are spelled
// 1 / 0 and 0 / 0. Each row is a `row(...)` check; the script completes with
// the number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
// Each key reads back under its canonical name.
{
  let o = {}; let r = 0;
  o["x"] = 1; r = r * 10 + o.x;
  o[1] = 2; r = r * 10 + o["1"];
  o[1.5] = 3; r = r * 10 + o["1.5"];
  o[-0] = 4; r = r * 10 + o["0"];
  o[true] = 5; r = r * 10 + o.true;
  o[undefined] = 6; r = r * 10 + o.undefined;
  let k = "y"; o[k] = 7; r = r * 10 + o.y;
  row(r === 1234567);
}
// ... and the other way round: a name reads back under a computed key.
{ let o = {x: 1}; row(o["x"] === 1); }
{ let o = {}; o["1"] = 2; row(o[1] === 2); }
{ let o = {}; o.true = 3; row(o[true] === 3); }
{ let o = {}; o["1.5"] = 4; row(o[3 / 2] === 4); }
{ let o = {}; o[0] = 5; row(o[-0] === 5); }
{ let o = {}; o[false] = 6; row(o.false === 6); }
{ let o = {}; o[null] = 7; row(o.null === 7); }
{ let o = {}; o[0 / 0] = 8; row(o.NaN === 8); }
{ let o = {}; o[1 / 0] = 9; row(o["Infinity"] === 9); }
{ let o = {}; o[-1 / 0] = 9; row(o["-Infinity"] === 9); }
{ let o = {}; o[1e21] = 10; row(o["1e+21"] === 10); }
{ let o = {}; o[0.1] = 11; row(o["0.1"] === 11); }
{ let o = {}; o[-1] = 12; row(o["-1"] === 12); }
{ let o = {}; o[4294967295] = 13; row(o["4294967295"] === 13); }
{ let o = {}; o[1e-7] = 14; row(o["1e-7"] === 14); }
// Distinct strings are distinct keys, even when they denote one number.
{ let o = {}; o["01"] = 1; row(o[1] === undefined && o["01"] === 1); }
{ let o = {}; o["1.50"] = 1; row(o[1.5] === undefined); }
// The `docs/grammar.md` example lines.
{ let o = {}; o[1] = "one"; row(o["1"] === "one" && o[1.5] === undefined); }
{ let p = {}; p[-0] = 0; p[true] = 1; row(p["0"] === 0 && p.true === 1 && p[undefined] === undefined); }
// Object literal computed keys.
{ let o = {[1.5]: 1, [true]: 2, [null]: 3, [undefined]: 4, [-1]: 5}; row(o["1.5"] === 1 && o.true === 2 && o.null === 3 && o.undefined === 4 && o["-1"] === 5); }
// A converted key reads back what an earlier write stored. (Compound
// assignment to a computed member needs dup2, not implemented yet.)
{ let o = {}; o[0.5] = 1; o[0.5] = o[0.5] + 2; o[true] = 1; o[true] = o[true] + 1; row(o["0.5"] === 3 && o.true === 2); }
// `in` (the rows light-cljs-825 left for this conversion).
row(true in {true: 1} && !(false in {true: 1}));
row(null in {null: 1} && undefined in {undefined: 1});
row(-1 in {"-1": 0} && !(-1 in {1: 0}));
row(1.5 in {"1.5": 0} && !(1.5 in {1: 0}));
row(!(-1 in [5]) && !(0.5 in [5]) && 0 in [5] && -0 in [5]);
{ const p = {true: 1}; const o = {__proto__: p}; row(true in o); }
// Built-ins convert their key argument the same way, and keep a symbol.
{
  const o = {"1.5": 1, true: 2, null: 3, [Symbol.iterator]: 4};
  row(Object.getOwnPropertyDescriptor(o, true).value === 2 && Object.getOwnPropertyDescriptor(o, 2.5) === undefined);
  row(Object.getOwnPropertyDescriptor(o, null).value === 3 && Object.getOwnPropertyDescriptor(o, 1.5).value === 1);
  row(Object.getOwnPropertyDescriptor(o, Symbol.iterator).value === 4);
}
// `delete`.
{ let o = {}; o[1.5] = 1; o[null] = 2; row(delete o[1.5] && !(1.5 in o) && null in o && delete o[null] && !("null" in o)); }
{ let o = {true: 1, undefined: 2}; row(delete o[true] && delete o[undefined] && !("true" in o) && !("undefined" in o)); }
// Arrays: a key that is not an index is a named property.
{ let a = [1, 2]; a[-1] = 3; a[1.5] = 4; row(a.length === 2 && a["-1"] === 3 && a["1.5"] === 4 && a[1] === 2); }
{ let a = []; a[true] = 1; a[null] = 2; row(a.length === 0 && a.true === 1 && a.null === 2); }
{ let a = [7]; row(a[-0] === 7 && a["0"] === 7 && a[0.5] === undefined); }
failed === 0 ? rows : -failed;
