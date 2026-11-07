// Function.prototype.toString by function kind (light-cljs-853): with source
// text each function gives it; serial_tests.goose also runs this image with
// every function row's source text cleared, where each gives QuickJS's
// native form with its prefix by kind ("function ", "function *",
// "async function ", "async function *"). The Pants engine keeps toString
// unsupported, so only Goose runs it.
var fs = [function a() {}, function* b() {}, async function c() {}, async function* d() {}];
fs.map(function (f) { return f.toString(); }).join("|");
