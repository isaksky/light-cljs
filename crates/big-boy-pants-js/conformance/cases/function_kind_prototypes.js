// bbp-authored conformance case.
// Derives from: none (ECMAScript 27.3.3, 27.7.3 and 27.4.3: the
// [[Prototype]] of generator, async and async generator functions is
// %GeneratorFunction.prototype%, %AsyncFunction.prototype% or
// %AsyncGeneratorFunction.prototype%, each inheriting %Function.prototype%
// with an own `constructor` and @@toStringTag; light-cljs-857). Goose keeps
// those `constructor`s pending (the generators area, light-cljs-768), so
// this case only observes that they exist and are not Function. Each row is
// a `row(...)` check; the script completes with the number of rows, or
// minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
const kinds = [
  [function* () {}, "GeneratorFunction"],
  [async function () {}, "AsyncFunction"],
  [async () => {}, "AsyncFunction"],
  [async function* () {}, "AsyncGeneratorFunction"],
];
for (const [f, tag] of kinds) {
  const p = Object.getPrototypeOf(f);
  row(p !== Function.prototype && Object.getPrototypeOf(p) === Function.prototype);
  row(Object.hasOwn(p, "constructor"));
  row(p[Symbol.toStringTag] === tag);
  row(f instanceof Function && typeof f === "function");
}
row(Object.getPrototypeOf(function* () {}) === Object.getPrototypeOf(function* g() {}));
row(Object.getPrototypeOf(async function () {}) === Object.getPrototypeOf(async () => {}));
row(Object.getPrototypeOf(function () {}) === Function.prototype && Object.getPrototypeOf(() => {}) === Function.prototype);
// %GeneratorFunction.prototype%.prototype is %GeneratorPrototype%.
row(Object.getPrototypeOf(function* () {}).prototype === Object.getPrototypeOf((function* () {}).prototype));
failed === 0 ? rows : -failed;
