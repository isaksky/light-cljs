// bbp-authored conformance case (light-cljs-940).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 5245-5295
// (date_utc_and_time_value), 5299-5342 (date_utc_huge_months), 5388-5422
// (date_to_iso_string), 5560-5587 (date_to_iso_string_receiver_and_descriptor),
// 5590-5670 (date_slots_and_coercion, without its string and clock rows) and
// 18849-18890 (date_symbol_to_primitive, without its 'integer' hint, which
// QuickJS accepts), tests/builtin_subclasses.rs's Date rows (line 78), and
// light-cljs-770's planner rows, probed on the Pants engine and QuickJS
// (TZ=UTC): time values,
// Date.UTC, the constructor's numeric and field forms, getTime, valueOf and
// setTime, toISOString, Date.prototype[@@toPrimitive] and the [[DateValue]]
// slot. The QuickJS rows of test262's fp-evaluation-order.js check MakeTime's
// and MakeDate's floating-point order. Rows that read the clock, convert a
// Date to a string or name a hint QuickJS accepts beyond the specification
// ("integer") stay out: the oracles disagree there (the Pants engine and the
// conformance runner supply no clock). Each row is a `row(...)` check; the
// script completes with the number of rows, or minus the first failing
// row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function throws(f, type) { try { f(); } catch (e) { return e instanceof type; } return false; }
row([new Date(0).getTime(), Date.UTC(2017, 9, 22, 18, 10, 11, 91), new Date(1506098258091).toISOString(), new Date(8.64e15 + 1).getTime(), typeof new Date(0).valueOf(), new Date(new Date(5)).getTime(), new Date(true).getTime(), Date.UTC(), Date.UTC(2017, 9, 22, 18, 10, 11, 91, NaN), new Date(-1).getUTCFullYear()].join('|')
  === "0|1508695811091|2017-09-22T16:37:38.091Z|NaN|number|5|1|NaN|1508695811091|1969");
// Date.UTC with one to eight arguments; NaN in any of the first seven.
row([Date.UTC(2017), Date.UTC(2017, 9), Date.UTC(2017, 9, 22), Date.UTC(2017, 9, 22, 18), Date.UTC(2017, 9, 22, 18, 10), Date.UTC(2017, 9, 22, 18, 10, 11), Date.UTC(2017, 9, 22, 18, 10, 11, 91)].join()
  === "1483228800000,1506816000000,1508630400000,1508695200000,1508695800000,1508695811000,1508695811091");
row([Date.UTC(NaN), Date.UTC(2017, NaN), Date.UTC(2017, 9, NaN), Date.UTC(2017, 9, 22, NaN), Date.UTC(2017, 9, 22, 18, NaN), Date.UTC(2017, 9, 22, 18, 10, NaN), Date.UTC(2017, 9, 22, 18, 10, 11, NaN)].every(Number.isNaN));
// MakeTime and MakeDate in the specification's order (test262).
row(Date.UTC(1970, 0, 1, 80063993375, 29, 1, -288230376151711740) === 29312 && Date.UTC(1970, 0, 213503982336, 0, 0, 0, -18446744073709552000) === 34447360);
row(Date.UTC(2017, 9, 22 - 1e10, 18 + 24e10) === 1508695200000 && Date.UTC(2017, 9, 22, 18 - 1e10, 10 + 60e10) === 1508695800000 && Date.UTC(2017, 9, 22, 18, 10 - 1e10, 11 + 60e10) === 1508695811000 && Date.UTC(2017, 9, 22, 18, 10, 11 - 1e12, 91 + 1000e12) === 1508695811091);
// Months far outside a year balance (rows where QuickJS's floating-point year
// sum is exact too).
row([Date.UTC(-6004799503160663, 72057594037927936), Date.UTC(6004799503160663, -72057594037927968), Date.UTC(-0x7fffffff, 12 * 0x7fffffff)].join() === "-62219923200000,-62198755200000,-62167219200000");
row(Date.UTC(99.9) === Date.UTC(1999) && Date.UTC(-0.9) === Date.UTC(1900) && Date.UTC(100) !== Date.UTC(2000));
row(Date.UTC(1969, 12) === 0 && Date.UTC(1970, -1) === -2678400000 && Date.UTC(1970, 0, 0) === -86400000);
row(Date.UTC(275760, 8, 13) === 8.64e15 && Date.UTC(-271821, 3, 20) === -8.64e15 && Number.isNaN(Date.UTC(275760, 8, 14)));
row(Date.UTC('1970', null, true, false, '0', null, null) === 0 && Number.isNaN(Date.UTC(1970, undefined)));
row(Date.UTC(1970, 0, 1, 0, 0, 0, 0, {valueOf() { throw 3; }}) === 0 && Date.UTC.call(null, 1970) === 0 && Date.UTC.apply(null, [1970]) === 0);
// Date.UTC converts its arguments in order; a throw ends it.
{ let calls = ''; let x = Date.UTC({valueOf() { calls += 'y'; return '1970'; }}, {valueOf() { calls += 'm'; return 0; }}); row(x === 0 && calls === 'ym'); }
{ let calls = ''; let x = Date.UTC(NaN, {valueOf() { calls += 'm'; return 0; }}); row(Number.isNaN(x) && calls === 'm'); }
{ let calls = ''; try { Date.UTC(Symbol(), {valueOf() { calls += 'm'; return 0; }}); } catch (e) { calls += e instanceof TypeError; } row(calls === 'true'); }
{ let calls = ''; try { Date.UTC({valueOf() { calls += 'y'; throw 3; }}, {valueOf() { calls += 'm'; return 0; }}); } catch (e) { calls += e; } row(calls === 'y3'); }
// The constructor's one-argument form: a Date's time value without
// ToPrimitive, otherwise ToPrimitive with no hint, then ToNumber and TimeClip.
{ let d = new Date(7); Object.defineProperty(d, 'valueOf', {value() { throw 1; }}); Object.defineProperty(d, 'toString', {value() { throw 2; }}); let copy = new Date(d); row(copy !== d && copy.getTime() === 7); }
{ let d = new Date(NaN); Object.defineProperty(d, 'valueOf', {get() { throw 1; }}); row(Number.isNaN(new Date(d).valueOf())); }
{ let d = new Date(7); Object.setPrototypeOf(d, null); row(Date.prototype.getTime.call(d) === 7 && new Date(d).getTime() === 7); }
row(new Date(undefined).getTime() !== new Date(undefined).getTime() && new Date(null).getTime() === 0 && new Date(true).getTime() === 1);
row(new Date(1.9).getTime() === 1 && new Date(-1.9).getTime() === -1 && Object.is(new Date(-0).getTime(), 0) && Object.is(new Date(-0.9).getTime(), 0));
row(new Date(8.64e15).getTime() === 8.64e15 && new Date(-8.64e15).getTime() === -8.64e15 && Number.isNaN(new Date(-8.64e15 - 1).getTime()) && Number.isNaN(new Date(Infinity).getTime()));
{ let calls = ''; let n = new Date({valueOf() { calls += 'v'; return {}; }, toString() { calls += 's'; return 3; }}); row(calls === 'vs' && n.getTime() === 3); }
{ let n = new Date({[Symbol.toPrimitive](h) { return h === 'default' ? 42 : 17; }}); row(n.getTime() === 42); }
row(throws(() => new Date(Symbol()), TypeError) && throws(() => new Date({valueOf() { return Symbol(); }}), TypeError));
// The field forms: two to seven arguments in order, the year's 0 to 99
// meaning 1900 to 1999; later arguments do not convert.
row(new Date(2000, 0).getTime() === 946684800000 && new Date(99, 11, 31, 23, 59, 59, 999).getTime() === 946684799999 && new Date(2017, 9, 22, 18, 10, 11, 91).getTime() === 1508695811091);
{ let log = ''; let d = new Date({valueOf() { log += 'y'; return 99; }}, {valueOf() { log += 'm'; return 0; }}, {valueOf() { log += 'd'; return 1; }}, 0, 0, 0, 0, Symbol()); row(log === 'ymd' && d.getFullYear() === 1999); }
row(Number.isNaN(new Date(2000, NaN).getTime()) && Number.isNaN(new Date(275760, 8, 14).getTime()) && new Date(1970, 0, 1, 0, 0, 0, -1).getTime() === -1);
// getTime, valueOf and setTime: thisTimeValue before ToNumber.
row(new Date(1506098258091).getTime() === 1506098258091);
{ let d = new Date(0); row(d.setTime(5) === 5 && d.valueOf() === 5); }
{ let d = new Date(NaN); row(d.setTime('5.9') === 5 && d.getTime() === 5 && Object.is(d.setTime(-0), 0) && Number.isNaN(d.setTime()) && Number.isNaN(d.getTime())); }
{ let d = new Date(1); Object.freeze(d); row(d.setTime(5) === 5 && d.getTime() === 5); }
{ let d = new Date(1); try { d.setTime(Symbol()); } catch (e) {} row(d.getTime() === 1); }
{ let d = new Date(1); try { d.setTime({valueOf() { throw 3; }}); } catch (e) {} row(d.getTime() === 1); }
for (let receiver of [{}, null, undefined, 1, Object.create(Date.prototype), Date.prototype]) {
  for (let method of ['getTime', 'valueOf', 'setTime']) {
    let converted = false;
    row(throws(() => Date.prototype[method].call(receiver, {valueOf() { converted = true; return 1; }}), TypeError) && !converted);
  }
}
row(throws(() => Date.prototype.setTime(1), TypeError) && throws(() => new Date.UTC(1970), TypeError));
// Lengths, names and descriptors.
row(Date.name === 'Date' && Date.length === 7 && Date.UTC.name === 'UTC' && Date.now.name === 'now' && Date.now.length === 0);
row(Date.prototype.getTime.name === 'getTime' && Date.prototype.getTime.length === 0 && Date.prototype.valueOf.name === 'valueOf' && Date.prototype.valueOf.length === 0 && Date.prototype.setTime.name === 'setTime' && Date.prototype.setTime.length === 1);
{ let d = Object.getOwnPropertyDescriptor(Date, 'prototype'); row(d.value === Date.prototype && !d.writable && !d.enumerable && !d.configurable); }
{ let d = Object.getOwnPropertyDescriptor(Date, 'UTC'); row(d.value === Date.UTC && d.writable && !d.enumerable && d.configurable); }
{ let d = Object.getOwnPropertyDescriptor(Date.prototype, 'getTime'); row(d.value === Date.prototype.getTime && d.writable && !d.enumerable && d.configurable); }
{ let d = Object.getOwnPropertyDescriptor(Date, 'now'); row(d.writable && !d.enumerable && d.configurable && typeof Date.now === 'function'); }
// The slot and the class.
{ let d = new Date(1); row(d instanceof Date && typeof d === 'object' && Object.getPrototypeOf(d) === Date.prototype && d.constructor === Date && Object.keys(d).length === 0); }
row('toString' in new Date(0) && 'toString' in Date.prototype && 'toJSON' in new Date(0) && 'toJSON' in Date.prototype);
row(Object.prototype.toString.call(new Date(1)) === '[object Date]' && Object.prototype.toString.call(Date.prototype) === '[object Object]');
{ let d = new Date(3); d.x = 1; d[0] = 2; row(d.x === 1 && d[0] === 2 && Object.keys(d).join() === '0,x' && delete d.x && !('x' in d) && d.getTime() === 3); }
// Subclasses keep the slot (OrdinaryCreateFromConstructor).
{ class D extends Date { constructor(x) { super(x); this.k = 1; } } let d = new D(5); row(d.getTime() === 5 && d instanceof D && d instanceof Date && d.k === 1 && Object.getPrototypeOf(d) === D.prototype && Object.prototype.toString.call(d) === '[object Date]'); }
{ class D extends Date {} let d = new D(2000, 1); row(d.getUTCMonth() === 1 && d.getUTCFullYear() === 2000 && D.UTC === Date.UTC); }
{ let d = Reflect.construct(Date, [9], Object); row(Object.getPrototypeOf(d) === Object.prototype && Date.prototype.getTime.call(d) === 9); }
{ let log = ''; let p = Object.create(Date.prototype); let F = new Proxy(function () {}, {get(t, k) { if (k === 'prototype') { log += 'p'; return p; } return t[k]; }}); let d = Reflect.construct(Date, [{valueOf() { log += 'v'; return 4; }}], F); row(log === 'vp' && Object.getPrototypeOf(d) === p && d.getTime() === 4); }
row(new (Date.bind(null, 9))().getTime() === 9);
// toISOString: the extended format, six-digit years outside 0 to 9999.
for (let [t, s] of [[1506098258091, '2017-09-22T16:37:38.091Z'], [0, '1970-01-01T00:00:00.000Z'], [-1, '1969-12-31T23:59:59.999Z'], [8.64e15, '+275760-09-13T00:00:00.000Z'], [-8.64e15, '-271821-04-20T00:00:00.000Z'], [-62198755200000, '-000001-01-01T00:00:00.000Z'], [253402300800000, '+010000-01-01T00:00:00.000Z'], [-62167219200000, '0000-01-01T00:00:00.000Z'], [-62135596800000, '0001-01-01T00:00:00.000Z'], [253402300799999, '9999-12-31T23:59:59.999Z'], [Date.UTC(2000, 1, 29, 12, 34, 56, 7), '2000-02-29T12:34:56.007Z'], [Date.UTC(1900, 2, 1), '1900-03-01T00:00:00.000Z'], [-0.9, '1970-01-01T00:00:00.000Z']]) {
  row(new Date(t).toISOString() === s);
}
for (let t of [NaN, Infinity, -Infinity, 8.64e15 + 1, -8.64e15 - 1]) {
  let ok = false; try { new Date(t).toISOString(); } catch (e) { ok = e instanceof RangeError; } row(ok);
}
row(Date.prototype.toISOString.name === 'toISOString' && Date.prototype.toISOString.length === 0);
{ let d = new Date(0); Object.defineProperty(d, 'valueOf', {value() { throw 1; }}); Object.defineProperty(d, 'toString', {value() { throw 2; }}); row(d.toISOString({valueOf() { throw 3; }}) === '1970-01-01T00:00:00.000Z'); }
{ let d = new Date(-1); Object.setPrototypeOf(d, null); row(Date.prototype.toISOString.call(d) === '1969-12-31T23:59:59.999Z'); }
{ let d = new Date(NaN); d.setTime(0); Object.freeze(d); row(d.toISOString() === '1970-01-01T00:00:00.000Z'); }
row(throws(() => new Date.prototype.toISOString(), TypeError));
for (let receiver of [{}, null, undefined, 1, '0', Date.prototype, Object.create(new Date(0))]) row(throws(() => Date.prototype.toISOString.call(receiver), TypeError));
// Date.prototype[@@toPrimitive]: string first for "string" and "default",
// number first for "number"; another hint a TypeError.
{ let d = Object.getOwnPropertyDescriptor(Date.prototype, Symbol.toPrimitive); row(!d.writable && !d.enumerable && d.configurable && d.value.name === '[Symbol.toPrimitive]' && d.value.length === 1); }
{ let d = new Date(0); row(d[Symbol.toPrimitive]('number') === 0 && +d === 0 && new Date(5) - new Date(2) === 3 && new Date(0) < new Date(1)); }
{ let d = new Date(NaN); row(Number.isNaN(+d)); }
{ let d = new Date(0); Object.defineProperty(d, Symbol.toPrimitive, {value: null}); row(d + '' === '0'); }
{ let d = new Date(0); Object.defineProperty(d, Symbol.toPrimitive, {value(h) { return h === 'default' ? 42 : 17; }}); row(d + 1 === 43 && +d === 17 && new Date(d).getTime() === 0); }
{ let f = Date.prototype[Symbol.toPrimitive], log = '', o = {get [Symbol.toPrimitive]() { throw 9; }, toString() { log += 's'; return {}; }, valueOf() { log += 'v'; return 7; }}; row(f.call(o, 'default') === 7 && log === 'sv' && f.call(o, 'number') === 7 && log === 'svv'); }
{ let f = Date.prototype[Symbol.toPrimitive], o = {toString: 1, valueOf() { return 7; }}; row(f.call(o, 'string') === 7); }
{ let s = Symbol(), o = {toString() { return s; }}; row(Date.prototype[Symbol.toPrimitive].call(o, 'default') === s); }
{ let d = new Date(3); Object.defineProperty(d, 'toString', {value() { return 'T'; }}); row(d + '' === 'T' && `${d}` === 'T' && d[Symbol.toPrimitive]('default') === 'T'); }
for (let receiver of [null, undefined, 1, 's', Symbol(), true]) row(throws(() => Date.prototype[Symbol.toPrimitive].call(receiver, 'number'), TypeError));
for (let hint of [undefined, null, 1, 'bad', new String('number'), {toString() { throw 9; }}]) row(throws(() => Date.prototype[Symbol.toPrimitive].call({}, hint), TypeError));
row(throws(() => Date.prototype[Symbol.toPrimitive].call({toString() { return {}; }, valueOf() { return {}; }}, 'default'), TypeError));
row(throws(() => new Date(0)[Symbol.toPrimitive]('x'), TypeError) && throws(() => Date.prototype.getTime.call({}), TypeError) && throws(() => Date.prototype.toISOString.call(1), TypeError));
failed === 0 ? rows : -failed;
