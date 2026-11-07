// A sloppy script's functions box a primitive `this` (light-cljs-833;
// OrdinaryCallBindThis): one wrapper object for the whole call, which an
// arrow inherits, its prototype the primitive's; a strict function keeps the
// primitive; undefined and null become the global object. The Pants engine
// fails this by name (checked_this), so only Goose runs it; QuickJS agrees
// with conformance/cases/sloppy_this_boxing.js. Completes with true.
function self() { return this; }
function strictSelf() { "use strict"; return this; }
function same() { var a = () => this; return this === this && a() === this; }
var one = self.call(1);
var text = self.call("ab");
var flag = self.call(false);
typeof one === "object" && one.valueOf() === 1 && Object.getPrototypeOf(one) === Number.prototype && self.call(1) !== one &&
  same.call(2) && strictSelf.call(1) === 1 && typeof strictSelf.call("a") === "string" &&
  typeof text === "object" && text.length === 2 && text[1] === "b" && Object.getPrototypeOf(text) === String.prototype &&
  flag.valueOf() === false && Object.getPrototypeOf(flag) === Boolean.prototype &&
  self.call(undefined) === globalThis && self.call(null) === globalThis && strictSelf.call(null) === null;
