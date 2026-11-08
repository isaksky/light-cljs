// A sloppy script's functions box a primitive `this` (light-cljs-833;
// OrdinaryCallBindThis): one wrapper object for the whole call, which an
// arrow inherits, its prototype the primitive's; a strict function keeps the
// primitive; undefined and null become the global object. The Pants engine
// fails this by name (checked_this), so its conformance slice,
// conformance/cases/sloppy_this_boxing.js, is an oracle disagreement; these
// are that slice's rows, where Goose matches QuickJS. Direct eval code reads
// the same wrapper as its caller (light-cljs-858). Completes with true.
function self() { return this; }
function strictSelf() { "use strict"; return this; }
function same() { return this === this; }
function arrow() { var a = () => this; return a() === this && typeof a() === "object"; }
function write() { this.x = 1; return this.x; }
function evalSame() { return eval("this === this"); }
function evalCaller() { var e = eval("this"); return e === this && typeof e === "object"; }
function evalArrow() { var a = eval("() => this"); return a() === this && a() === a(); }
var one = self.call(1);
var s = self.call("ab");
var b = self.call(false);
var o = {};
typeof one === "object" && Object.getPrototypeOf(one) === Number.prototype && one.valueOf() === 1 &&
  self.call(1) !== self.call(1) && same.call(1) === true && arrow.call(2) === true &&
  typeof strictSelf.call(1) === "number" && strictSelf.call(1) === 1 && strictSelf.call("a") === "a" && strictSelf.call(true) === true &&
  typeof s === "object" && s.length === 2 && s[1] === "b" && Object.getPrototypeOf(s) === String.prototype && s.toString() === "ab" &&
  typeof b === "object" && b.valueOf() === false && Object.getPrototypeOf(b) === Boolean.prototype && !!b &&
  self.call(undefined) === globalThis && self.call(null) === globalThis && self() === globalThis &&
  strictSelf.call(undefined) === undefined && strictSelf.call(null) === null &&
  write.call(3) === 1 && Object.prototype.toString.call(self.call(3)) === "[object Number]" &&
  self.call(o) === o && strictSelf.call(o) === o &&
  evalSame.call(1) && evalCaller.call("s") && evalArrow.call(2);
