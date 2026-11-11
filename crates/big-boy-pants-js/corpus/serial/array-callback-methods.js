// The Array.prototype callback methods and search methods (light-cljs-908)
// run in resumable native frames: filter and reduce with a callback that
// deletes and redefines later elements, some, every and the find family
// stopping early, flatMap over a mapped array with an element getter,
// ArraySpeciesCreate through `constructor` and @@species getters and a
// species constructor, the search methods' length getter and valueOf, their
// fromIndex conversion and an element getter, and a proxy receiver's has
// trap.
function arrayCallbackMethods() {
  let log = "";
  const values = [1, 2, 3, 4];
  const kept = values.filter(function (v, i, o) {
    if (i === 0) { delete o[1]; Object.defineProperty(o, 2, { get: function () { log += "g"; return 30; }, configurable: true }); }
    return v !== 4;
  });
  const sum = [1, 2, 3, 4].reduce(function (acc, v, i, o) { if (i === 1) { delete o[2]; } return acc + v; });
  const right = [1, 2, 3].reduceRight(function (acc, v) { return acc + v; }, "");
  let calls = 0;
  const some = [1, 2, 3].some(function (v) { calls++; return v === 2; });
  const every = [1, 2, 3].every(function (v) { calls++; return v < 2; });
  const found = [5, 6, 7].find(function (v) { return v > 5; });
  const last = [5, 6, 7].findLastIndex(function (v) { return v < 7; });
  const missing = [5, 6, 7].findIndex(function (v) { return v > 9; });
  const inner = [8, 9, 10];
  Object.defineProperty(inner, 1, { get: function () { log += "i"; return 90; } });
  const flat = [1, 2].flatMap(function (v) { return v === 1 ? inner : v; });
  const source = [1, 2, 3];
  Object.defineProperty(source, "constructor", { get: function () {
    log += "c";
    return { get [Symbol.species]() { log += "s"; return function (n) { log += "n" + n; this.made = n; }; } };
  } });
  const species = source.map(function (v) { return v * 2; });
  const picked = source.filter(function (v) { return v > 1; });
  let reads = 0;
  const generic = { 0: "a", get 1() { log += "e"; return "b"; }, 2: "c", get length() { reads++; return { valueOf: function () { reads++; return 3; } }; } };
  const from = { valueOf: function () { log += "f"; return 1; } };
  const index = Array.prototype.indexOf.call(generic, "c", from);
  const includes = Array.prototype.includes.call(generic, "b");
  const at = Array.prototype.at.call(generic, -1);
  const lastIndex = [1, 2, 1].lastIndexOf(1, from);
  const hidden = new Proxy([1, 2, 3], { has: function (t, k) { return k !== "1" && k in t; } });
  const visible = Array.prototype.filter.call(hidden, function () { return true; });
  const absent = Array.prototype.indexOf.call(hidden, 2);
  const viaNative = [0, 1].map(Array.prototype.at, [7, 8]);
  let typeError = false;
  try { [].reduce(function () {}); } catch (e) { typeError = e.name === "TypeError"; }
  return kept.length === 2 && kept[0] === 1 && kept[1] === 30 && sum === 7 && right === "321" && some && !every && calls === 4 &&
    found === 6 && last === 1 && missing === -1 && flat.length === 4 && flat[1] === 90 && flat[3] === 2 &&
    species.made === 3 && species[2] === 6 && species.length === undefined && picked.made === 0 && picked[1] === 3 &&
    index === 2 && includes && at === "c" && lastIndex === 0 && reads === 6 && visible.length === 2 && visible[1] === 3 && absent === -1 && viaNative[0] === 7 && viaNative[1] === 8 &&
    typeError && log === "gicsn3csn0feef";
}
