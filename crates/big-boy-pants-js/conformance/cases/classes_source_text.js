// bbp-authored conformance case (light-cljs-886).
// Derives from: none (light-cljs-766's planner names Function.prototype.toString
// of a class, and light-cljs-853 has the exporter record a class
// constructor's [[SourceText]] as the whole class): a class's text is its class
// definition, a method's its method definition, static members included.
// QuickJS is the oracle; the Pants engine has no Function.prototype.toString.
// Each row is a `row(...)` check; the script completes with the number of
// rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
{ class C { m() { return 1 } } row(String(C) === "class C { m() { return 1 } }"); }
{ class C { m() { return 1 } } row(C.prototype.m.toString() === "m() { return 1 }"); }
{ let C = class { static s() {} }; row(Function.prototype.toString.call(C) === "class { static s() {} }" && C.s.toString() === "s() {}"); }
{ class C { get x() { return 1 } static set y(v) {} } let d = Object.getOwnPropertyDescriptor(C.prototype, 'x'); row(d.get.toString() === "get x() { return 1 }" && Object.getOwnPropertyDescriptor(C, 'y').set.toString() === "set y(v) {}"); }
{ class C { constructor(a) { this.a = a } } row(`${C}` === "class C { constructor(a) { this.a = a } }"); }
failed === 0 ? rows : -failed;
