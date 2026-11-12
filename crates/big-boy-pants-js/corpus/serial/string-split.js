function stringSplit() {
  // light-cljs-904: delegated GetMethod and Call, conversions, partial
  // comparisons and the result array survive collection and every cut.
  let log = '';
  function spin(t) { for (let i = 0; i < 3; i++) {} log += t; }
  const receiver = {toString() { throw 9; }}, limit = {};
  const sep = {get [Symbol.split]() {
    spin('g'); return function(s, l) { spin('c'); return this === sep && s === receiver && l === limit; };
  }};
  const delegated = String.prototype.split.call(receiver, sep, limit);
  const r = {toString() { spin('r'); return 'abaXabaX'; }};
  const s = {get [Symbol.split]() { spin('m'); return undefined; }, toString() { spin('s'); return 'X'; }};
  const parts = String.prototype.split.call(r, s, {valueOf() { spin('l'); return 3; }});
  const pad = 'z'.padStart({valueOf() { spin('n'); return 7; }}, {toString() { spin('f'); return 'ab'; }});
  const skipped = 'abc'.padEnd(2, {toString() { throw 8; }});
  const zero = 'ab'.split({toString() { spin('0'); return 'a'; }}, 0);
  const chars = '😀'.split('');
  const scan = 'a'.repeat(1050).concat('b').split('aaaab');
  return delegated && parts.join('|') === 'aba|aba|' && pad === 'abababz' && skipped === 'abc'
    && zero.length === 0 && chars[0] === '\ud83d' && chars[1] === '\ude00'
    && scan.length === 2 && scan[0].length === 1046 && scan[1] === '' && log === 'gcmrlsnf0';
}
