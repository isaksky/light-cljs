// ToPrimitive in every operator family (light-cljs-828): arithmetic
// (ToNumberPair, left fully converted before the right), relational, `==`,
// unary `+`/`-` and `++`, a template substitution (hint string), property
// keys in reads, writes, `in`, `delete` and computed literal keys, an
// object's Symbol.toPrimitive with the hint it is given, a method behind a
// getter, and an exception from valueOf caught past the conversion frame.
function toPrimitive() {
  let log = "";
  const a = { valueOf() { log += "a"; return 8; } };
  const b = { valueOf() { log += "b"; return "3"; } };
  const hinted = {};
  hinted[Symbol.toPrimitive] = function (h) { log += h === "string" ? "s" : h === "number" ? "n" : "d"; return h === "string" ? "k" : 2; };
  const key = { toString() { log += "t"; return "k"; }, valueOf() { return "v"; } };
  const viaGetter = { get valueOf() { log += "g"; return function () { return 5; }; } };
  const thrower = { valueOf() { throw "boom"; } };
  let caught = "";
  try { caught = thrower * 2; } catch (e) { caught = e; }
  const o = {};
  o[key] = 1;
  let n = { valueOf() { return 41; } };
  n++;
  const literal = { [hinted]: 7 };
  const results = [
    a - b === 5, a < b === false, b < a, a == 8, 8 == a, a != "8x",
    -a === -8, +b === 3, n === 42,
    `${hinted}` === "k", hinted + 1 === 3, +hinted === 2, hinted == 2,
    o.k === 1, key in o, o[key] === 1, delete o[key], !("k" in o), literal.k === 7,
    viaGetter * 2 === 10, caught === "boom",
  ];
  for (let i = 0; i < results.length; i++) { if (results[i] !== true) { return false; } }
  // Each conversion ran once, in order.
  return log === "tsababbaaaaabsdndtttg";
}
