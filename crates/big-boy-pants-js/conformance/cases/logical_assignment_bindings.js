// bbp-authored conformance case.
// Derives from: rust/tests/fixtures/js/logical_assignment.js lines 2-24, 50-58
// and 64-66 (crates/big-boy-pants-js): `&&=`, `||=` and `??=` on bindings, their
// short-circuiting, the errors of a `const`, unresolvable or uninitialized
// target, and named evaluation of anonymous functions and arrows. The
// property-target, class and BigInt rows of that fixture are left out. Each
// row is a `row(...)` check; the script completes with the number of rows,
// or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
let calls = 0;
function rhs() { calls++; return 42; }
let a = null, b, c = 0, d = false, e = "";
row((a ?? rhs()) === 42 && (b ?? 3) === 3 && (c ?? rhs()) === 0);
row((d ?? rhs()) === false && (e ?? rhs()) === "" && calls === 1);
row((a ??= 4) === 4 && a === 4);
row((a ??= rhs()) === 4 && calls === 1);
row((b ||= 5) === 5 && b === 5);
row((b ||= rhs()) === 5 && calls === 1);
row((c &&= rhs()) === 0 && c === 0);
row((d &&= rhs()) === false && calls === 1);
row((e ||= 6) === 6 && (e &&= 7) === 7 && e === 7 && calls === 1);
{ let x, y; row((x ??= y ||= 10) === 10 && x === 10 && y === 10); }
{ let z = 0; z ||= 1; z &&= 2; z ??= 3; row(z === 2); }
{ var vz = null; vz ??= "v"; vz &&= vz + "w"; row(vz === "vw"); }
{ let n = NaN; let r = (n ?? rhs()); row(r !== r && calls === 1); }
{ let n = NaN; n ||= 8; row(n === 8); }
// A `const` that short-circuits is not written, so it does not throw.
const zero = 0, one = 1;
row((zero &&= rhs()) === 0 && (one ||= rhs()) === 1 && (one ??= rhs()) === 1 && calls === 1);
// Errors: a `const` that would be written, an unresolvable name, the
// temporal dead zone.
row(name(() => { zero ||= rhs(); }) === "TypeError" && calls === 2 && zero === 0);
row(name(() => { missing ??= rhs(); }) === "ReferenceError" && calls === 2);
row(name(() => { tdz &&= rhs(); let tdz; }) === "ReferenceError" && calls === 2);
// Captured bindings.
{ let cell; const fill = () => cell ??= rhs(); fill(); fill(); row(cell === 42 && calls === 3); }
{ let flag = 1; const off = () => flag &&= 0; off(); row(flag === 0); }
// Named evaluation of anonymous functions and arrows.
let f, arrow, generator, asyncFn;
f ||= function () {}; arrow ??= () => {}; generator ||= function* () {};
asyncFn ??= async function () {};
row(f.name === "f" && arrow.name === "arrow");
row(generator.name === "generator" && asyncFn.name === "asyncFn");
let named; named ||= function explicit() {};
let notNamed; notNamed ||= (0, function () {});
row(named.name === "explicit" && notNamed.name === "");
let truthy = true; truthy &&= function () {};
let asyncArrow; asyncArrow ??= async () => {};
row(truthy.name === "truthy" && asyncArrow.name === "asyncArrow");
// The right side is not evaluated, so its function is not named either.
{ let keep = function () {}; keep ??= () => {}; row(keep.name === "keep"); }
// An object operand is not converted.
{
  let hits = 0;
  const noCoercion = {toString() { hits++; throw 99; }, valueOf() { hits++; throw 99; }};
  let o = noCoercion; o ||= 1; o ??= 2;
  row((noCoercion ?? rhs()) === noCoercion && o === noCoercion && hits === 0 && calls === 3);
}
failed === 0 ? rows : -failed;
