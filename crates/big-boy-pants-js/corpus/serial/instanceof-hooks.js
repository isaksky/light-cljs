// instanceof's user code (light-cljs-834): serial_tests.goose installs,
// before the job, an accessor `prototype` on Function.prototype whose getter
// is protoGet, which every function without its own `prototype` (an arrow)
// inherits, and on the global object `hooked`, an ordinary object whose
// Symbol.hasInstance is an accessor with hookGet as its getter, as
// Object.defineProperty or a computed accessor name would, which Goose does
// not run yet. Each getter and user @@hasInstance method runs as a frame
// above the operator's native frame; every budget cut matches the uncut run.
// Completes with true; without the accessors it fails.
var enabled;
if (enabled) {
  globalThis.unused = [function protoGet() {
    reads += 1;
    if (this.boom) { throw "boom"; }
    return this.proto;
  }, function hookGet() {
    log += "g";
    return this.method;
  }];
}
var log = "", reads = 0;
var arrow = () => 1;
var p = {};
arrow.proto = p;
// OrdinaryHasInstance reads `prototype` through the getter.
var a = { __proto__: p } instanceof arrow;
var b = ({}) instanceof arrow;
var c = Function.prototype[Symbol.hasInstance].call(arrow, { __proto__: { __proto__: p } });
// A primitive is no instance, and the getter is not run for it.
var d = 1 instanceof arrow;
arrow.proto = 1;
var e = "";
try { ({}) instanceof arrow; } catch (x) { e = x.name; }
arrow.boom = true;
var f = "";
try { ({}) instanceof arrow; } catch (x) { f = x; }
// The Symbol.hasInstance getter supplies a user method, then no method,
// then the realm's method on a target that is not callable.
hooked.method = function (v) { log += "m"; return v === 7 ? "yes" : 0; };
var g = 7 instanceof hooked && !(8 instanceof hooked);
hooked.method = undefined;
var h = "";
try { 7 instanceof hooked; } catch (x) { h = x.name; }
hooked.method = Function.prototype[Symbol.hasInstance];
var i = !({} instanceof hooked);
hooked.method = function () { throw "thrown"; };
var j = "";
try { 7 instanceof hooked; } catch (x) { j = x; }
function F() {}
var k = new F() instanceof F && !({} instanceof F) && F instanceof Function;
// Built-in methods: Number converts an object (its valueOf runs in a
// Convert frame), and map runs as a native frame, its receiver the target.
var numeric = {};
numeric[Symbol.hasInstance] = Number;
var l = { valueOf: function () { log += "v"; return 7; } } instanceof numeric;
var list = [1];
list[Symbol.hasInstance] = list.map;
var calls = 0;
var m = (function (x) { calls += 1; return x; }) instanceof list;
a && !b && c && !d && e === "TypeError" && f === "boom" && g && h === "TypeError" && i && j === "thrown" && k &&
  l && m && calls === 1 && reads === 5 && log === "gmgmgggv";
