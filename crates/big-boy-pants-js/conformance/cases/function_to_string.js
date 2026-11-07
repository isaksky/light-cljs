// bbp-authored conformance case.
// Derives from: ~/src/quickjs/quickjs.c lines 42191-42235
// (js_function_toString, revision 7d1c13c) and the outputs of its qjs for
// each row (light-cljs-853):
// Function.prototype.toString of a function compiled from source is its
// exact source text (QuickJS's debug.source, ECMAScript [[SourceText]]):
// declarations and expressions from `function`/`async` to the closing
// brace, comments and spacing kept; methods, getters and setters from the
// property name with `get`, `set`, `async` and `*`; arrows whole. Built-in
// and bound
// functions give "function " + name + "() {\n    [native code]\n}"; a
// receiver that is not callable is a TypeError. The Pants engine refuses
// toString (rust/src/vm/function.rs FUNCTION_PROTOTYPE_NAMES), so this case
// is scored against QuickJS alone. Left out: classes (Goose does not create
// a class yet, light-cljs-766; the Rust exporter test
// function_source_text_is_exported checks a class constructor's text is the
// whole class and a method's starts after `static`), computed method keys
// (SetComputedName, the objects area), a proxy of a function (Goose fails a
// proxy's [[Get]] of `name` by name, light-cljs-861) and functions from the
// Function constructor (a later child of light-cljs-764), and a `name`
// getter (Object.defineProperty is pending; the frame reads the name with
// bind's get_named). Each row is a `row(...)` check; the script completes
// with the number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
function native(n) { return "function " + n + "() {\n    [native code]\n}"; }
// Declarations and expressions, comments and spacing kept.
function  f ( a, b ) { return a; }
row(f.toString() === "function  f ( a, b ) { return a; }");
row((function (x) { return x }).toString() === "function (x) { return x }");
row((function /*a*/ named /*b*/ (/*c*/) /*d*/ {/*e*/}).toString() === "function /*a*/ named /*b*/ (/*c*/) /*d*/ {/*e*/}");
row((function* g(x) { yield x }).toString() === "function* g(x) { yield x }");
row((async function h(x) { await x }).toString() === "async function h(x) { await x }");
row((async function* ag(x) { }).toString() === "async function* ag(x) { }");
row((function () { return function inner() { /* c */ } })().toString() === "function inner() { /* c */ }");
// Arrows.
row((x => x * 2).toString() === "x => x * 2");
row(((a, b) => { return a; }).toString() === "(a, b) => { return a; }");
row((async (x) => x).toString() === "async (x) => x");
row((async x => x).toString() === "async x => x");
// Object literal methods, accessors and property values.
var o = { m(a) { return a; }, get p() { return 1; }, set p(v) {}, *gen() {}, async am() {}, async *agm() {}, 'str'(q) {}, 42(n) {}, f: function() {}, a: () => 1 };
row(o.m.toString() === "m(a) { return a; }");
row(Object.getOwnPropertyDescriptor(o, "p").get.toString() === "get p() { return 1; }");
row(Object.getOwnPropertyDescriptor(o, "p").set.toString() === "set p(v) {}");
row(o.gen.toString() === "*gen() {}" && o.am.toString() === "async am() {}" && o.agm.toString() === "async *agm() {}");
row(o.str.toString() === "'str'(q) {}" && o[42].toString() === "42(n) {}");
row(o.f.toString() === "function() {}" && o.a.toString() === "() => 1");
// Non-ASCII source text.
var μ = function μ() { return "é😀"; };
row(μ.toString() === "function μ() { return \"é😀\"; }" && μ.toString().length === 30);
// Built-in functions.
row(Function.prototype.toString.toString() === native("toString"));
row(Object.toString() === native("Object") && Array.prototype.push.toString() === native("push"));
row(Function.prototype.toString.call(Function.prototype) === native(""));
row(Function.prototype.toString.call(Reflect.apply) === native("apply"));
// Bound functions: the native form with the bound function's name.
row(f.bind(null).toString() === native("bound f"));
row(f.bind().bind().toString() === native("bound bound f"));
row((function () {}).bind().toString() === native("bound "));
row((async function () {}).bind().toString() === native("bound ") && (function* () {}).bind().toString() === native("bound "));
// The method itself, through call and apply, on a String() of a function.
row(Function.prototype.toString.call(f) === f.toString() && Function.prototype.toString.apply(o.m) === "m(a) { return a; }");
row(String(o.m) === "m(a) { return a; }" && "" + (x => x) === "x => x");
// A receiver that is not callable.
row(name(() => Function.prototype.toString.call({})) === "TypeError");
row(name(() => Function.prototype.toString.call(undefined)) === "TypeError" && name(() => Function.prototype.toString.call("f")) === "TypeError");
// Function.prototype.toString's own property.
{ const d = Object.getOwnPropertyDescriptor(Function.prototype, "toString"); row(d.writable === true && d.enumerable === false && d.configurable === true && d.value.length === 0 && d.value.name === "toString"); }
// The native form's name is Get(F, "name") and its ToString: an object
// name converts through its toString (a user method, whose exception
// propagates), a Symbol is a TypeError, a number its text. Last: it deletes
// Function.prototype.name so that the bound function can own a new `name`.
{
  delete Function.prototype.name;
  const bf = (function () {}).bind();
  delete bf.name;
  bf.name = { toString() { return "renamed"; } };
  row(bf.toString() === native("renamed") && Function.prototype.toString.call(bf) === native("renamed") && String(bf) === native("renamed"));
  bf.name = { toString() { throw new RangeError("no name"); } };
  row(name(() => bf.toString()) === "RangeError");
  bf.name = Symbol.iterator;
  row(name(() => bf.toString()) === "TypeError");
  bf.name = 42;
  row(bf.toString() === native("42"));
  delete bf.name;
  row(bf.toString() === native(""));
}
failed === 0 ? rows : -failed;
