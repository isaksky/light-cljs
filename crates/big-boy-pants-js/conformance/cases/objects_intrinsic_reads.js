// bbp-authored conformance case (light-cljs-880).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 15381-15522; object_to_string.rs lines 25-148:
// object_intrinsics (js_source.rs line 15381, bbp-117): Object.prototype's
// toString and valueOf, the Object constructor and Object.is, their own
// properties, shadowing and receivers; Object.getPrototypeOf and
// Object.getOwnPropertyDescriptor reads (the planner's rows, light-cljs-766);
// and the rows of object_to_string.rs builtin_and_intrinsic_tags (line 25)
// and user_tags_and_proxy_dispatch (100) for the values Goose has today.
// Left out: `__rt` rows and opcode checks; Object.hasOwn and
// Object.prototype.hasOwnProperty, which the Pants engine does not have (they are
// objects_own_property_reads, scored against QuickJS alone); a proxy's
// @@toStringTag read (light-cljs-890); the tags of Symbol (light-cljs-885), BigInt (light-cljs-771), Math and JSON
// (light-cljs-767), Date and RegExp (light-cljs-770), Promise and async
// generators (light-cljs-768), Map, Set, weak collections, typed arrays and
// ArrayBuffer (light-cljs-769), disposable stacks (light-cljs-772), Intl
// (light-cljs-774), SuppressedError, and Proxy dispatch (light-cljs-890).
// Each row is a `row(...)` check; the script completes with the number of
// rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// object_intrinsics (line 15381, bbp-117).
row((({}).toString()) === "[object Object]");
{
  let a = {};
  row((a.toString()) === "[object Object]");
}
{
  let o = {};
  row(o.valueOf() === o);
}
row(({}).toString === Object.prototype.toString);
row((({toString: 1}).toString) === 1);
row(Object.is(NaN, NaN) === true);
row(Object.is(0, -0) === false);
{
  let o = {};
  row(Object.is(o, o) === true);
}
{
  let o = {};
  row(Object.is(o, {}) === false);
}
row(Object() !== Object());
{
  let o = {};
  row(Object(o) === o);
}
row(Object.is('ab', 'a' + 'b') === true);
row(Object.is(null, undefined) === false);
row(Object.is() === true);
row(Object.is(-0, -0) === true);
row(Object.is(1, '1') === false);
row(Object(undefined) !== Object(undefined));
{
  let f = () => 1;
  row(Object(f) === f);
}
row(Object(Object) === Object);
row(({}).toString() === "[object Object]" && ({}).toString === Object.prototype.toString);
row(Object.is(NaN, NaN) && !Object.is(0, -0) && Object() !== Object() && typeof Object === "function");
{
  let o = {toString: () => "own"};
  row(o.toString() === "own" && Object(o) === o && o.valueOf() === o);
}
row((Object(null).toString()) === "[object Object]");
row((Object().toString()) === "[object Object]");
row((typeof Object) === "function");
row((typeof Object.is) === "function");
row((typeof Object.prototype) === "object");
row((typeof Object.prototype.toString) === "function");
row((Object.length) === 1);
row((Object.is.length) === 2);
row((Object.prototype.toString.length) === 0);
row((Object.name) === "Object");
row((Object.prototype.valueOf.name) === "valueOf");
row(Object.prototype.constructor === Object);
row(({}).constructor === Object);
row(Object(null).valueOf === Object.prototype.valueOf);
row((Object.prototype.missing) === undefined);
row((({}).missing) === undefined);
{
  let Object = 1;
  row((Object) === 1);
}
{
  function f(Object) { return Object; }
  row((f(2)) === 2);
}
{
  { const Object = 'c'; }
  row((typeof Object) === "function");
}
{
  let o = {}; o.toString = function () { return 'mine'; };
  row((o.toString()) === "mine");
}
{
  let o = {}; o.toString = 1; let p = {};
  row((p.toString() + ':' + typeof o.toString) === "[object Object]:number");
}
{
  let o = Object(); o.x = 2;
  row((o.x) === 2);
}
{
  let o = {}; o.constructor = 'c';
  row((o.constructor) === "c");
}
{
  let f = Object.prototype.toString;
  row((f()) === "[object Undefined]");
}
{
  let o = {};
  row(((o.toString)()) === "[object Object]");
}
{
  let o = {};
  row(((0, o.toString)()) === "[object Undefined]");
}
row((Object.prototype.toString()) === "[object Object]");
{
  let o = {f: function () {}};
  row((Object.prototype.toString === o.f ? 'same' : 'other') === "other");
}
{
  let n = 0; function g() { n = n + 1; return {}; } g().toString();
  row((n) === 1);
}
row((({inner: {}}).inner.toString()) === "[object Object]");
{
  let o = {f: Object.prototype.valueOf};
  row((o.f() === o ? 'same' : 'other') === "same");
}
{
  let o = {f: Object.prototype.toString};
  row((o.f()) === "[object Object]");
}
{
  let o = {m: function (a, b) { return a - b; }};
  row((o.m(5, 2)) === 3);
}
{
  let o = {m: (x) => x * 2};
  row((o.m(21)) === 42);
}
// Own and inherited property reads through Object statics and Object.prototype (the planner's rows, light-cljs-766).
row(Object.getPrototypeOf({}) === Object.prototype);
{
  let p = {x: 1};
  row(Object.getPrototypeOf({__proto__: p}) === p);
}
{
  let d = Object.getOwnPropertyDescriptor({x: 1}, 'x');
  row(d.value === 1 && d.writable && d.enumerable && d.configurable);
}
row(Object.getOwnPropertyDescriptor({}, 'x') === undefined);
// object_to_string.rs builtin_and_intrinsic_tags (line 25) and user_tags_and_proxy_dispatch (line 100): the rows Goose runs.
row(Object.prototype.toString.call(undefined) === '[object Undefined]');
row(Object.prototype.toString.call(null) === '[object Null]');
row(Object.prototype.toString.call(true) === '[object Boolean]');
row(Object.prototype.toString.call(1) === '[object Number]');
row(Object.prototype.toString.call('abc') === '[object String]');
row(Object.prototype.toString.call({}) === '[object Object]');
row(Object.prototype.toString.call([]) === '[object Array]');
row(Object.prototype.toString.call(Array.prototype) === '[object Array]');
row(Object.prototype.toString.call(function() {}) === '[object Function]');
row(Object.prototype.toString.call(new Error()) === '[object Error]');
row(Object.prototype.toString.call(new Boolean(false)) === '[object Boolean]');
row(Object.prototype.toString.call(new Number(0)) === '[object Number]');
row(Object.prototype.toString.call(new String('')) === '[object String]');
row(Object.prototype.toString.call((function() { return arguments; })(1)) === '[object Arguments]');
row(Object.prototype.toString.call(Reflect) === '[object Reflect]');
row(Object.prototype.toString.call((function*() {})()) === '[object Generator]');
row(Object.prototype.toString.call([].values()) === '[object Array Iterator]');
row(Object.prototype.toString.call(''[Symbol.iterator]()) === '[object String Iterator]');
{
  let o = {[Symbol.toStringTag]: 'Custom'};
  row(Object.prototype.toString.call(o) === '[object Custom]');
}
{
  let o = {[Symbol.toStringTag]: {toString() {throw 42;}}};
  row(Object.prototype.toString.call(o) === '[object Object]');
}
{
  let o = {[Symbol.toStringTag]: '\ud800'};
  row(Object.prototype.toString.call(o) === '[object \ud800]');
}
// An @@toStringTag getter (object_to_string.rs lines 105, 120 and 123;
// light-cljs-884's Statics frame), through every caller.
{
  let calls = 0, o = Object.create({get [Symbol.toStringTag]() { calls++; return this.x; }}); o.x = 'Inherited';
  row(Object.prototype.toString.call(o) === '[object Inherited]' && calls === 1);
}
{ let a = []; a.join = 0; Object.defineProperty(a, Symbol.toStringTag, {get() { return 'Fallback'; }}); row(Array.prototype.toString.call(a) === '[object Fallback]'); }
{ let caught = false; try { Object.prototype.toString.call({get [Symbol.toStringTag]() { throw 42; }}); } catch (e) { caught = e === 42; } row(caught); }
{ let o = {get [Symbol.toStringTag]() { return 'G'; }}; row(String(o) + `${o}` + (o + '') === '[object G][object G][object G]' && [o].join() === '[object G]'); }
{ let o = {get [Symbol.toStringTag]() { return 7; }}; row(Object.prototype.toString.call(o) === '[object Object]'); }
// The planner's rows (light-cljs-766).
row((Object.prototype.toString.call({[Symbol.toStringTag]: 'T'})) === "[object T]");
row((Object.prototype.toString.call([]) + Object.prototype.toString.call(null) + Object.prototype.toString.call(function () {})) === "[object Array][object Null][object Function]");
failed === 0 ? rows : -failed;
