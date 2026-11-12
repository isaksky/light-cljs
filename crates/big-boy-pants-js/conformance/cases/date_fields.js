// bbp-authored conformance case (light-cljs-940).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 23588-23624
// (date_utc_fields) and 23627-23721 (date_utc_fields_coercion, without its
// 4,097-unit string row, the Pants engine's own limit), tests/date_host.rs
// lines 186-196 (date_without_host_time_inputs) and 199-227
// (date_local_fields_and_setters, its rows recomputed for UTC), and light-cljs-770's planner
// rows, probed on the Pants engine and QuickJS (TZ=UTC, the zone of both
// engines here): the UTC and local field getters and setters and
// getTimezoneOffset. Under UTC local fields are UTC fields. A setter reads
// its time value before converting its arguments in order, answers NaN
// without writing when that value was NaN (setFullYear starts from +0), and
// TimeClips the result. Each row is a `row(...)` check; the script
// completes with the number of rows, or minus the first failing row's
// number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function throws(f, type) { try { f(); } catch (e) { return e instanceof type; } return false; }
{ let d = new Date(1506098258091); d.setUTCHours(18, 10, 11); row([d.toISOString(), d.getUTCDay(), d.getUTCMonth(), d.getUTCDate(), d.getUTCMilliseconds(), d.setUTCFullYear(2020), d.setUTCMonth(13), d.toISOString()].join('|') === "2017-09-22T18:10:11.091Z|5|8|22|91|1600798211091|1614017411091|2021-02-22T18:10:11.091Z"); }
{ let d = new Date(2000, 0); row([d.getTime(), d.getTimezoneOffset(), d.getHours(), d.getDay(), new Date(2000, 0, 1, 25).getDate(), d.setHours(5), d.getFullYear()].join('|') === "946684800000|0|0|6|2|946702800000|2000"); }
{ let log = []; let u = Date.UTC({valueOf() { log.push('y'); return 1970; }}, {valueOf() { log.push('m'); return 0; }}); let d = new Date(0); let r = d.setUTCHours({valueOf() { log.push('h'); return 1; }}, {valueOf() { log.push('mi'); return 2; }}); row(log.join() === 'y,m,h,mi' && u === 0 && r === 3720000 && d.getTime() === 3720000); }
// The UTC setters.
{ let d = new Date(1506098258091); row(d.setUTCHours(18, 10, 11) === 1506103811091 && d.toISOString() === '2017-09-22T18:10:11.091Z'); }
{ let d = new Date(1506098258091); d.setUTCHours(25); row(d.toISOString() === '2017-09-23T01:37:38.091Z'); }
{ let d = new Date(Date.UTC(2017, 0, 15)); d.setUTCMonth(1, 30); row(d.toISOString() === '2017-03-02T00:00:00.000Z'); }
{ let d = new Date(NaN); row(d.setUTCFullYear(2000) === 946684800000 && d.toISOString() === '2000-01-01T00:00:00.000Z'); }
{ let d = new Date(NaN); row(Number.isNaN(d.setUTCMinutes(1)) && Number.isNaN(d.getTime())); }
{ let d = new Date(1506098258091); d.setUTCDate(0); row(d.toISOString() === '2017-08-31T16:37:38.091Z'); }
{ let d = new Date(1506098258091); d.setUTCMinutes(-1, 60, 1000); row(d.toISOString() === '2017-09-22T16:00:01.000Z'); }
{ let d = new Date(1506098258091); d.setUTCSeconds(60); row(d.toISOString() === '2017-09-22T16:38:00.091Z'); }
{ let d = new Date(1506098258091); d.setUTCMilliseconds(-1.9); row(d.toISOString() === '2017-09-22T16:37:37.999Z'); }
{ let d = new Date(0); d.setUTCFullYear(1); row(d.toISOString() === '0001-01-01T00:00:00.000Z'); }
{ let d = new Date(-1); d.setUTCFullYear(2000, 1, 29); row(d.toISOString() === '2000-02-29T23:59:59.999Z'); }
{ let d = new Date(8640000000000000); d.setUTCMilliseconds(1); row(Number.isNaN(d.getTime())); }
{ let d = new Date(-8640000000000000); d.setUTCMilliseconds(-1); row(Number.isNaN(d.getTime())); }
{ let d = new Date(0); row(Object.is(d.setUTCMilliseconds(-0.9), 0)); }
// The getters.
for (let [name, expected] of [['getUTCFullYear', 2017], ['getUTCMonth', 8], ['getUTCDate', 22], ['getUTCDay', 5], ['getUTCHours', 16], ['getUTCMinutes', 37], ['getUTCSeconds', 38], ['getUTCMilliseconds', 91],
                              ['getFullYear', 2017], ['getMonth', 8], ['getDate', 22], ['getDay', 5], ['getHours', 16], ['getMinutes', 37], ['getSeconds', 38], ['getMilliseconds', 91]]) {
  row(new Date(1506098258091)[name]() === expected && Number.isNaN(new Date(NaN)[name]()));
}
row([new Date(-1).getUTCDay(), new Date(-62198755200000).getUTCFullYear(), new Date(8.64e15).getUTCFullYear(), new Date(-8.64e15).getUTCMonth(), new Date(951782400000).getUTCDate(), new Date(-2208988800000).getUTCDay()].join() === "3,-1,275760,3,29,1");
row(new Date(-1).getUTCDay({valueOf() { throw 1; }}) === 3);
row(new Date(0).getTimezoneOffset() === 0 && Number.isNaN(new Date(NaN).getTimezoneOffset()) && new Date(8.64e15).getTimezoneOffset() === 0);
// Optional arguments: an omitted one keeps its field, an undefined one is NaN.
for (let [name, args] of [['setUTCFullYear', [2017, 8, 22]], ['setUTCMonth', [8, 22]], ['setUTCHours', [16, 37, 38, 91]], ['setUTCMinutes', [37, 38, 91]], ['setUTCSeconds', [38, 91]],
                          ['setFullYear', [2017, 8, 22]], ['setMonth', [8, 22]], ['setHours', [16, 37, 38, 91]], ['setMinutes', [37, 38, 91]], ['setSeconds', [38, 91]]]) {
  for (let optional = 1; optional < args.length; optional++) {
    let d = new Date(1506098258091);
    row(d[name](...args.slice(0, optional)) === 1506098258091 && d.getTime() === 1506098258091);
    let calls = 0;
    let explicit = args.map((a, i) => i < optional ? a : i === optional ? undefined : {valueOf() { calls++; return a; }});
    let e = new Date(1506098258091);
    row(Number.isNaN(e[name](...explicit)) && Number.isNaN(e.getTime()) && calls === args.length - optional - 1);
  }
}
// Names, lengths, descriptors and receivers.
for (let [name, length] of [['getUTCFullYear', 0], ['getUTCMonth', 0], ['getUTCDate', 0], ['getUTCDay', 0], ['getUTCHours', 0], ['getUTCMinutes', 0], ['getUTCSeconds', 0], ['getUTCMilliseconds', 0],
                            ['getFullYear', 0], ['getMonth', 0], ['getDate', 0], ['getDay', 0], ['getHours', 0], ['getMinutes', 0], ['getSeconds', 0], ['getMilliseconds', 0], ['getTimezoneOffset', 0],
                            ['setUTCFullYear', 3], ['setUTCMonth', 2], ['setUTCDate', 1], ['setUTCHours', 4], ['setUTCMinutes', 3], ['setUTCSeconds', 2], ['setUTCMilliseconds', 1],
                            ['setFullYear', 3], ['setMonth', 2], ['setDate', 1], ['setHours', 4], ['setMinutes', 3], ['setSeconds', 2], ['setMilliseconds', 1]]) {
  let m = Date.prototype[name]; let p = Object.getOwnPropertyDescriptor(Date.prototype, name);
  row(m.name === name && m.length === length && p.value === m && p.writable && !p.enumerable && p.configurable);
  for (let receiver of [{}, Date.prototype, Object.create(new Date(0)), null, 1]) {
    let converted = false;
    row(throws(() => m.call(receiver, {valueOf() { converted = true; return 1; }}), TypeError) && !converted);
  }
  row(throws(() => new m(), TypeError));
  if (length > 0) {
    for (let args of [[], [undefined], [NaN], [Infinity]]) { let d = new Date(0); row(Number.isNaN(d[name](...args)) && Number.isNaN(d.getTime())); }
    let d = new Date(1); row(throws(() => d[name](Symbol()), TypeError) && d.getTime() === 1);
  }
}
// Conversion order, and the time value read before the conversions.
{ let log = ''; let d = new Date(NaN); let result = d.setUTCHours({valueOf() { log += 'h'; return NaN; }}, {valueOf() { log += 'm'; return 1; }}, {valueOf() { log += 's'; return 2; }}, {valueOf() { log += 'f'; return 3; }}, {valueOf() { throw 4; }}); row(log === 'hmsf' && Number.isNaN(result)); }
{ let log = ''; let d = new Date(1); try { d.setUTCMinutes({valueOf() { log += 'm'; return 1; }}, {valueOf() { log += 's'; throw 2; }}, {valueOf() { log += 'f'; return 3; }}); } catch (e) { log += e; } row(log === 'ms2' && d.getTime() === 1); }
{ let log = ''; try { new Date(NaN).setUTCMinutes(Symbol(), {valueOf() { log += 's'; return 1; }}); } catch (e) { log += e instanceof TypeError; } row(log === 'true'); }
{ let d = new Date(1506098258091); d.setUTCHours({valueOf() { d.setTime(0); return 18; }}); row(d.toISOString() === '2017-09-22T18:37:38.091Z'); }
{ let d = new Date(NaN); let r = d.setUTCMinutes({valueOf() { d.setTime(0); return 1; }}); row(Number.isNaN(r) && d.getTime() === 0); }
{ let d = new Date(NaN); d.setUTCFullYear({valueOf() { d.setTime(100000); return 2000; }}); row(d.toISOString() === '2000-01-01T00:00:00.000Z'); }
{ let d = new Date(1); Object.freeze(d); row(d.setUTCSeconds(1) === 1001 && d.getUTCSeconds() === 1); }
{ let d = new Date(1); Object.setPrototypeOf(d, null); row(Date.prototype.setUTCSeconds.call(d, 1) === 1001 && Date.prototype.getUTCSeconds.call(d) === 1); }
// The local setters under UTC.
{ let d = new Date(2017, 8, 22, 18, 10, 11, 91); row(d.getFullYear() === 2017 && d.getMonth() === 8 && d.getDate() === 22 && d.getHours() === 18 && d.getMinutes() === 10 && d.getSeconds() === 11 && d.getMilliseconds() === 91 && d.getDay() === 5 && d.getUTCHours() === 18); }
{ let d = new Date(2000, 0); d.setFullYear(2001, 1, 2); d.setMonth(2, 3); d.setDate(4); d.setHours(5, 6, 7, 8); d.setMinutes(9, 10, 11); d.setSeconds(12, 13); d.setMilliseconds(14); row(d.toISOString() === '2001-03-04T05:09:12.014Z'); }
{ let d = new Date(NaN); let nan = d.setHours(3); row(Number.isNaN(nan) && Number.isNaN(d.getTimezoneOffset()) && Number.isNaN(d.getFullYear()) && d.setFullYear(2000) === Date.UTC(2000, 0, 1)); }
{ let d = new Date(0); let n = d.setHours({valueOf() { d.setTime(86400000); return 2; }}); row(n === 7200000 && d.getTime() === 7200000); }
{ let log = ''; let d = new Date(NaN); d.setHours({valueOf() { log += 'h'; return NaN; }}, {valueOf() { log += 'm'; return 1; }}); row(log === 'hm' && Number.isNaN(d.getTime())); }
{ let called = false; let caught = false; try { Date.prototype.setHours.call({}, {valueOf() { called = true; return 1; }}); } catch (e) { caught = e instanceof TypeError; } row(caught && !called); }
{ let d = new Date(8.64e15); row(d.getHours() === 0 && d.setHours(0) === 8.64e15 && new Date(275760, 8, 13, 0).getTime() === 8.64e15); }
{ let d = new Date(2000, 0, 31); d.setMonth(1); row(d.getMonth() === 2 && d.getDate() === 2); }
failed === 0 ? rows : -failed;
