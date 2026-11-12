function stringCase() {
  // light-cljs-905: case mapping and normalization after their receiver
  // and form conversions, and a locale list read in a StringMethod frame
  // (getters, a proxy's has trap, an element's toString, holes), survive
  // every budget cut, collection and saved continuation. The receiver
  // converts before the locales are read (ECMA-402 and the Pants engine;
  // QuickJS reads the locales first, so the conformance slice leaves these
  // rows out), as do the rows QuickJS answers differently: a language that
  // only begins with "tr", and an LV syllable before U+11A7.
  let log = '';
  function spin(t) { for (let i = 0; i < 3; i++) {} log += t; }
  const upper = String.prototype.toUpperCase.call({toString() { spin('u'); return 'straße ΐ'; }});
  const lower = String.prototype.toLowerCase.call({toString() { spin('w'); return 'ΟΣ́ ΑΣ'; }});
  const nfd = String.prototype.normalize.call({toString() { spin('n'); return 'é각'; }}, {toString() { spin('f'); return 'NFD'; }});
  // Node's V8 stops at the first element, so the lists have one (after a
  // hole); conformance/cases/strings_locale_case.js reads longer ones.
  const locales = {
    get length() { spin('l'); return {valueOf() { spin('v'); return 2; }}; },
    get 1() { spin('1'); return {toString() { spin('s'); return 'TR'; }}; },
  };
  const turkish = String.prototype.toLocaleLowerCase.call({toString() { spin('r'); return 'İIİ'; }}, locales);
  const list = new Proxy(['az'], {has(t, k) { spin('h'); return k in t; }});
  const azeri = 'iı'.toLocaleUpperCase(list);
  const lithuanian = 'Ì'.toLocaleLowerCase(['lt']) + 'i̇'.toLocaleUpperCase('lt-LT');
  let order = '';
  const receiver = {toString() { order += 's'; return ''; }};
  const throwing = {get length() { order += 'l'; throw 17; }};
  let caught = 0;
  try { String.prototype.toLocaleUpperCase.call(receiver, throwing); } catch (e) { caught += e === 17; }
  try { String.prototype.toLocaleLowerCase.call(null, throwing); } catch (e) { caught += e instanceof TypeError; }
  try { String.prototype.toLocaleLowerCase.call(Symbol(), throwing); } catch (e) { caught += e instanceof TypeError; }
  try { String.prototype.toLocaleUpperCase.call({toString() { throw 23; }}, throwing); } catch (e) { caught += e === 23; }
  const edited = ['en'];
  const late = String.prototype.toLocaleLowerCase.call({toString() { edited[0] = 'tr'; return 'I'; }}, edited);
  let invalid = '';
  try { 'a'.toLocaleUpperCase({length: 1, 0: {toString() { spin('x'); return 'en_US'; }}}); } catch (e) { invalid = e.name; }
  const long = 'aÀΣ'.repeat(700).toLowerCase();
  return upper === 'STRASSE Ϊ́' && lower === 'ος́ ας' && nfd === 'é각'
    && turkish === 'iıi' && azeri === 'İI' && lithuanian === 'i̇̀I'
    && order === 'sl' && caught === 4 && late === 'ı' && invalid === 'RangeError'
    && 'I'.toLocaleLowerCase('trv') === 'i' && '가ᆧ'.normalize() === '가ᆧ'
    && long.length === 2100 && long.endsWith('aàς') && long.startsWith('aàσ')
    && log === 'uwnfrlv1shx';
}
