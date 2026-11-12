// bbp-authored conformance case (light-cljs-940).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 5673-5696
// (date_json_serialization, its own-hook rows) and light-cljs-770's
// QuickJS-only planner rows, probed on QuickJS (TZ=UTC): Date.prototype.toJSON
// (ECMAScript 21.4.4.37: ToObject, ToPrimitive with hint number, null for a
// non-finite Number, else Invoke(O, "toISOString")), Annex B getYear and
// setYear (B.2.3.1, B.2.3.2), and Date.UTC.length, 7 (21.4.3.4). The Pants
// engine fails toJSON UnsupportedOperand (date_json_serialization), has no
// getYear or setYear and gives Date.UTC.length 2, so areas.edn scores this
// case against QuickJS alone (:pants-gap). QuickJS's setYear reads the time
// value after converting its argument, where B.2.3.2 reads it first (Goose
// does), so no row lets that valueOf change the date. JSON.stringify's call of toJSON
// is light-cljs-907's. Each row is a `row(...)` check; the script completes
// with the number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function throws(f, type) { try { f(); } catch (e) { return e instanceof type; } return false; }
row(Date.UTC.length === 7);
// toJSON.
row(new Date(0).toJSON() === '1970-01-01T00:00:00.000Z' && new Date(NaN).toJSON() === null && new Date(1506098258091).toJSON('k') === '2017-09-22T16:37:38.091Z');
row(Date.prototype.toJSON.call({toISOString() { return 'I'; }}) === 'I');
row(Date.prototype.toJSON.name === 'toJSON' && Date.prototype.toJSON.length === 1);
{ let d = Object.getOwnPropertyDescriptor(Date.prototype, 'toJSON'); row(d.writable && !d.enumerable && d.configurable); }
row(Date.prototype.toJSON.call({valueOf() { return Infinity; }, toISOString() { return 'X'; }}) === null);
row(Date.prototype.toJSON.call({valueOf() { return NaN; }}) === null && Date.prototype.toJSON.call({valueOf() { return -Infinity; }}) === null);
row(Date.prototype.toJSON.call({valueOf() { return 'NaN'; }, toISOString() { return 'S'; }}) === 'S');
row(Date.prototype.toJSON.call({valueOf() { return 5; }, toISOString() { return this.valueOf() + 1; }}) === 6);
{ let log = ''; let o = {valueOf() { log += 'v'; return 1; }, get toISOString() { log += 'g'; return function () { log += 'c'; return 'r'; }; }}; row(Date.prototype.toJSON.call(o) === 'r' && log === 'vgc'); }
{ let o = {[Symbol.toPrimitive](h) { return h === 'number' ? 1 : NaN; }, toISOString() { return 'P'; }}; row(Date.prototype.toJSON.call(o) === 'P'); }
{ let d = new Date(0); d.toISOString = function () { return 'own'; }; row(d.toJSON() === 'own'); }
{ let d = new Date(0); Object.defineProperty(d, Symbol.toPrimitive, {value() { return Infinity; }}); row(d.toJSON() === null); }
row(throws(() => Date.prototype.toJSON.call(1), TypeError) && throws(() => Date.prototype.toJSON.call(null), TypeError) && throws(() => Date.prototype.toJSON.call(undefined), TypeError) && throws(() => Date.prototype.toJSON.call({toISOString: 1}), TypeError));
row(throws(() => Date.prototype.toJSON.call({valueOf() { return Symbol(); }, toString() { return {}; }}), TypeError) && throws(() => Date.prototype.toJSON.call({valueOf() { return {}; }, toString() { return {}; }}), TypeError));
{ let log = ''; try { Date.prototype.toJSON.call({valueOf() { log += 'v'; throw 7; }, get toISOString() { log += 'g'; return null; }}); } catch (e) { log += e; } row(log === 'v7'); }
// getYear and setYear.
row(typeof Date.prototype.getYear === 'function' && typeof Date.prototype.setYear === 'function' && Date.prototype.getYear.length === 0 && Date.prototype.setYear.length === 1);
row(new Date(0).getYear() === 70 && new Date(Date.UTC(2017, 0)).getYear() === 117 && new Date(Date.UTC(1800, 0)).getYear() === -100 && Number.isNaN(new Date(NaN).getYear()));
{ let d = new Date(0); row(d.setYear(99) === Date.UTC(1999, 0) && d.getFullYear() === 1999); }
{ let d = new Date(Date.UTC(2000, 1, 29, 12)); row(d.setYear(2001) === Date.UTC(2001, 2, 1, 12)); }
{ let d = new Date(0); row(d.setYear(100) === Date.UTC(100, 0) && d.setYear(-1) === Date.UTC(-1, 0) && d.setYear(99.9) === Date.UTC(1999, 0)); }
{ let d = new Date(NaN); row(d.setYear(5) === Date.UTC(1905, 0) && d.getUTCHours() === 0); }
{ let d = new Date(0); row(Number.isNaN(d.setYear(NaN)) && Number.isNaN(d.getTime()) && Number.isNaN(new Date(0).setYear()) && Number.isNaN(new Date(0).setYear(Infinity))); }
row(throws(() => Date.prototype.getYear.call({}), TypeError) && throws(() => Date.prototype.setYear.call({}, 1), TypeError));
failed === 0 ? rows : -failed;
