function stringMethods() {
  // light-cljs-903: native conversions, String.raw's getters, search
  // predicates' @@match order and UTF-16 scans survive every budget cut,
  // collection and saved continuation without repeating user effects.
  let log = '';
  function spin(t) { for (let i = 0; i < 3; i++) {} log += t; }
  const raw = {
    get length() { spin('l'); return {valueOf() { spin('n'); return 2; }}; },
    get 0() { spin('0'); return {toString() { spin('a'); return 'x\ud800'; }}; },
    get 1() { spin('1'); return 'y'; },
  };
  const r = String.raw({get raw() { spin('r'); return raw; }}, {toString() { spin('s'); return 'S'; }});
  const a = String.fromCharCode({valueOf() { spin('c'); return 65; }}, 0xd800);
  const b = String.fromCodePoint({valueOf() { spin('p'); return 0x1f600; }});
  let caught = false;
  try { String.fromCodePoint(1.5, {valueOf() { spin('!'); return 65; }}); } catch (e) { caught = e instanceof RangeError; }
  const n = String.prototype.lastIndexOf.call({toString() { spin('t'); return 'aba'; }}, {toString() { spin('q'); return 'a'; }}, {valueOf() { spin('i'); return NaN; }});
  const yes = 'abc'.startsWith({get [Symbol.match]() { spin('m'); return false; }, toString() { spin('v'); return 'b'; }}, {valueOf() { spin('o'); return 1; }});
  return r === 'x\ud800Sy' && a.charCodeAt(1) === 0xd800 && b.codePointAt(0) === 0x1f600
    && caught && n === 2 && yes && !a.isWellFormed() && a.toWellFormed() === 'A�'
    && log === 'rln0as1cptqimvo';
}
