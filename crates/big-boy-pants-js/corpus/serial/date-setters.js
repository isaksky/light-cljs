function dateSetters() {
  // Date objects (light-cljs-940): Date native frames whose conversions run
  // valueOf methods, getters and @@toPrimitive in frames above them, so
  // serial_tests.goose stops and resumes them at every budget cut and
  // collects inside the job at every cut while the frames hold the Date,
  // its time value read before the conversions, the converted fields and
  // the constructor's newTarget: setters (UTC and local, setTime, Annex B
  // setYear), the constructor's field and one-argument forms and a
  // subclass whose newTarget's `prototype` is a proxy's getter, Date.UTC,
  // toJSON and Date.prototype[@@toPrimitive]. Completes with 1 when the
  // embedder supplies the clock serial_tests.goose fixes (1506098258091),
  // 0 when it supplies none (Date.now(), new Date() and Date() throw a
  // RangeError), and -1 when a check fails.
  let log = "";
  const v = (tag, n) => ({ valueOf() { log += tag; return n; } });
  const d = new Date(1506098258091);
  const r1 = d.setUTCHours(v("h", 18), v("m", 10), v("s", 11));
  const r2 = d.setUTCHours({ valueOf() { log += "x"; d.setTime(0); return 1; } });
  const n = new Date(NaN);
  const r3 = n.setUTCMinutes({ valueOf() { log += "n"; n.setTime(5); return 1; } });
  const local = new Date(v("Y", 99), v("M", 11), v("D", 31), v("H", 23), 59, 59, 999, Symbol());
  const one = new Date({ valueOf() { log += "o"; return {}; }, toString() { log += "t"; return 7; } });
  const utc = Date.UTC(v("y", 2017), v("m", 9), 22);
  const p = Object.create(Date.prototype);
  const F = new Proxy(function () {}, { get(t, k) { if (k === "prototype") { log += "p"; return p; } return t[k]; } });
  const sub = Reflect.construct(Date, [v("a", 2000), v("b", 1)], F);
  const y = new Date(0); const r4 = y.setYear(v("z", 99)); const r5 = y.setMonth(v("q", 13), v("w", 2));
  const t = new Date(9); const r6 = t.setTime(v("T", 3));
  const json = Date.prototype.toJSON.call({ valueOf() { log += "j"; return 1; }, get toISOString() { log += "g"; return function () { return "I"; }; } });
  const nul = new Date(NaN).toJSON();
  const iso = new Date(Date.UTC(-1, 0)).toJSON();
  const prim = +new Date(42) + (new Date(1) - new Date(0));
  const tagged = new Date(3); Object.defineProperty(tagged, "toString", { value() { log += "S"; return "T"; } });
  const s = `${tagged}` + tagged[Symbol.toPrimitive]("number");
  let caught = "";
  try { Date.prototype[Symbol.toPrimitive].call(tagged, "x"); } catch (e) { caught += e instanceof TypeError; }
  try { new Date(0).setUTCSeconds(Symbol()); } catch (e) { caught += e instanceof TypeError; }
  const ok = r1 === 1506103811091 && r2 === 1506042611091 && d.toISOString() === "2017-09-22T01:10:11.091Z" && Number.isNaN(r3) && n.getTime() === 5
    && local.getTime() === 946684799999 && local.getUTCDay() === 5 && one.getTime() === 7 && utc === 1508630400000
    && Object.getPrototypeOf(sub) === p && sub.getTime() === 949363200000 && Object.prototype.toString.call(sub) === "[object Date]"
    && r4 === 915148800000 && r5 === 949449600000 && y.getUTCMonth() === 1 && y.getUTCDate() === 2 && y.getFullYear() === 2000 && r6 === 3 && t.getTime() === 3
    && json === "I" && nul === null && iso === "-000001-01-01T00:00:00.000Z" && prim === 43 && s === "T3" && caught === "truetrue"
    && log === "hmsxnYMDHotymabpzqwTjgS";
  if (!ok) return -1;
  let clock = 0;
  try { clock = Date.now() === 1506098258091 && new Date().getTime() === 1506098258091 ? 1 : -1; } catch (e) { clock = e instanceof RangeError ? 0 : -1; }
  if (clock === 0) {
    for (const f of [() => new Date(), () => Date()]) { try { f(); return -1; } catch (e) { if (!(e instanceof RangeError)) return -1; } }
  }
  return clock;
}
