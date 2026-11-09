// bbp-authored conformance case (light-cljs-881), run as a sloppy script.
// Derives from: none (ECMAScript Annex B.2.2.2 to B.2.2.5
// (Object.prototype.__defineGetter__, __defineSetter__, __lookupGetter__
// and __lookupSetter__) and QuickJS's js_object___defineGetter__ and
// js_object___lookupGetter__ (~/src/quickjs/quickjs.c); and the planner's
// row (light-cljs-766)). The Pants engine lacks these methods, so the case is
// scored against QuickJS alone (areas.edn :pants-gap). Each row is a
// `row(...)` check; the script completes with the number of rows, or minus
// the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// The planner's row (light-cljs-766).
{ var o = {}; o.__defineGetter__('x', function () { return 4; }); row(o.x === 4); }
// __defineGetter__ and __defineSetter__ define an enumerable, configurable
// accessor half, keeping the other half.
{ var o = {}; o.__defineGetter__('x', function () { return this.v; }); o.__defineSetter__('x', function (v) { this.v = v * 2; }); o.x = 3; var d = Object.getOwnPropertyDescriptor(o, 'x'); row(o.x === 6 && d.enumerable && d.configurable && typeof d.get === 'function' && typeof d.set === 'function'); }
{ var o = {x: 1}; o.__defineGetter__('x', function () { return 2; }); row(o.x === 2 && !('value' in Object.getOwnPropertyDescriptor(o, 'x'))); }
row(name(function () { ({}).__defineGetter__('x', 1); }) === "TypeError" && name(function () { ({}).__defineSetter__('x', {}); }) === "TypeError");
{ var o = {}; Object.defineProperty(o, 'k', {value: 1}); row(name(function () { o.__defineGetter__('k', function () {}); }) === "TypeError" && o.k === 1); }
row(({}).__defineGetter__('x', function () {}) === undefined);
// __lookupGetter__ and __lookupSetter__ find the first own property of the
// key along the prototype chain.
{ var g = function () { return 1; }; var s = function (v) {}; var p = {}; p.__defineGetter__('x', g); p.__defineSetter__('y', s); var o = {__proto__: p}; row(o.__lookupGetter__('x') === g && o.__lookupSetter__('y') === s && o.__lookupSetter__('x') === undefined && o.__lookupGetter__('y') === undefined); }
{ var p = {get x() { return 1; }}; var o = {__proto__: p, x: 2}; row(o.__lookupGetter__('x') === undefined && o.__lookupGetter__('missing') === undefined); }
row(Object.prototype.__lookupGetter__.call(1, 'x') === undefined && name(function () { Object.prototype.__lookupGetter__.call(null, 'x'); }) === "TypeError");
row(Object.prototype.__defineGetter__.length === 2 && Object.prototype.__lookupSetter__.length === 1 && Object.prototype.__defineSetter__.name === '__defineSetter__');
failed === 0 ? rows : -failed;
