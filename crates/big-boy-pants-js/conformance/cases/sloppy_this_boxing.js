// bbp-authored conformance case, run as a sloppy script (areas.edn :mode).
// Derives from: crates/big-boy-pants-js/rust/src/vm/dispatch.rs lines 1132-1147,
// checked_this, which fails this by name on the Pants engine; ECMAScript
// 10.2.1.2 OrdinaryCallBindThis and QuickJS's js_call_internal (JS_ToObject
// of a non-strict function's this_obj) give the rows (light-cljs-833): a sloppy function
// called with a primitive receiver sees its wrapper object, one wrapper for
// the whole call, which arrows inherit; a strict one sees the primitive;
// undefined and null become the global object. Function.prototype.call
// supplies the receivers, since the Pants engine's built-in prototypes are
// immutable. Each row is a `row(...)` check; the script completes with the
// number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function self() { return this; }
function strictSelf() { 'use strict'; return this; }
function same() { return this === this; }
function arrow() { var a = () => this; return a() === this && typeof a() === 'object'; }
function write() { this.x = 1; return this.x; }
var one = self.call(1);
row(typeof one === 'object' && Object.getPrototypeOf(one) === Number.prototype && one.valueOf() === 1);
row(self.call(1) !== self.call(1) && same.call(1) === true && arrow.call(2) === true);
row(typeof strictSelf.call(1) === 'number' && strictSelf.call(1) === 1 && strictSelf.call('a') === 'a' && strictSelf.call(true) === true);
{ var s = self.call('ab'); row(typeof s === 'object' && s.length === 2 && s[1] === 'b' && Object.getPrototypeOf(s) === String.prototype && s.toString() === 'ab'); }
{ var b = self.call(false); row(typeof b === 'object' && b.valueOf() === false && Object.getPrototypeOf(b) === Boolean.prototype && !!b); }
row(self.call(undefined) === globalThis && self.call(null) === globalThis && self() === globalThis);
row(strictSelf.call(undefined) === undefined && strictSelf.call(null) === null);
row(write.call(3) === 1 && Object.prototype.toString.call(self.call(3)) === '[object Number]');
{ var o = {}; row(self.call(o) === o && strictSelf.call(o) === o); }
failed === 0 ? rows : -failed;
