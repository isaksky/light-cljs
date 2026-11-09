// bbp-authored conformance case (light-cljs-880).
// Derives from: none (light-cljs-766's planned rows of Object.hasOwn and
// Object.prototype.hasOwnProperty, which the Pants engine does not have, so
// areas.edn scores this case against QuickJS alone with :pants-gap): own,
// inherited, absent and own-undefined properties, an accessor whose getter
// must not run, converted primitive keys, deleted properties, array elements,
// holes and `length`, the TypeError of a null or undefined receiver, and the
// methods' lengths. Left out: a key object whose toString must run, which
// Goose's Object.hasOwn refuses today (Object statics, light-cljs-884).
// Each row is a `row(...)` check; the script completes with the number of
// rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// Object.hasOwn and Object.prototype.hasOwnProperty on ordinary objects.
row(Object.hasOwn({x: 1}, 'x') && !Object.hasOwn({__proto__: {x: 1}}, 'x') && !Object.hasOwn({}, 'x'));
row(({x: 1}).hasOwnProperty('x') && !({__proto__: {x: 1}}).hasOwnProperty('x') && !({}).hasOwnProperty('toString'));
row(Object.hasOwn({x: undefined}, 'x') && ({x: undefined}).hasOwnProperty('x'));
{
  let n = 0; let o = {get x() { n++; return 1; }};
  row(Object.hasOwn(o, 'x') && o.hasOwnProperty('x') && n === 0);
}
{
  let o = {1: 'a', true: 'b'};
  row(Object.hasOwn(o, 1) && o.hasOwnProperty(true) && !Object.hasOwn(o, 2));
}
{
  let o = {x: 1}; delete o.x;
  row(!Object.hasOwn(o, 'x') && !o.hasOwnProperty('x'));
}
row(Object.hasOwn([5], 0) && Object.hasOwn([5], 'length') && !Object.hasOwn([5], 1) && !Object.hasOwn([, 5], 0));
row(Object.prototype.hasOwnProperty.call({x: 1}, 'x') && Object.hasOwn(Object.prototype, 'toString'));
row((name(() => Object.hasOwn(null, 'x'))) === "TypeError");
row((name(() => Object.prototype.hasOwnProperty.call(undefined, 'x'))) === "TypeError");
row((Object.hasOwn.length + ',' + Object.prototype.hasOwnProperty.length) === "2,1");
failed === 0 ? rows : -failed;
