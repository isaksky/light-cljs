// bbp-authored conformance case (light-cljs-905).
// Derives from: rust/tests/fixtures/js/string_methods.js lines 11-20 and 56-67
// (its toLocaleUpperCase and toLocaleLowerCase rows) and rust/tests/js_source.rs
// lines 30554-30628, 30645-30749 and 30751-30770
// (string_locale_case_receiver_precedes_locales and the JavaScript rows of
// string_methods_budget_and_gc and string_methods_wait_collect_and_migrate).
// The first requested locale's language selects the Turkish and Azeri or
// the Lithuanian rules; the locales go through CanonicalizeLocaleList.
// Left out: the rows where ECMA-402 and the Pants engine convert the
// receiver before reading the locales and QuickJS reads the locales first
// (string_locale_case_receiver_precedes_locales: a receiver of null,
// undefined, a Symbol, a throwing toString or @@toPrimitive with a
// throwing `length` getter, the logged order, and a receiver whose
// toString edits the list), and a language that only begins with "tr"
// (`'I'.toLocaleLowerCase('trv')` is "i"; QuickJS matches the prefix and
// answers "ı"); corpus/serial/string-case.js checks them on Goose.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function error(f) { try { f(); } catch(e) { return e.name; } return ''; }
// string_methods.js
row('Iİiı'.toLocaleLowerCase('tr') === 'ıiiı');
row('Iİiı'.toLocaleUpperCase('az') === 'IİİI');
row('İ'.toLocaleLowerCase('tr') === 'i');
row('Í̇'.toLocaleLowerCase('tr') === 'ı́̇');
row('Ị̇'.toLocaleLowerCase('tr') === 'ị');
row('Í'.toLocaleLowerCase('lt') === 'i̇́');
row('ÌÍĨ'.toLocaleLowerCase('lt') === 'i̇̀i̇́i̇̃');
row('í̇'.toLocaleUpperCase('lt') === 'Í');
row('i̇ͅ'.toLocaleUpperCase('lt') === 'Iͅ');
row('i'.toLocaleUpperCase('en-US') === 'I');
for (let name of ['toLocaleUpperCase', 'toLocaleLowerCase']) {
  row(typeof String.prototype[name] === 'function');
  row(Object.getOwnPropertyDescriptor(String.prototype, name).enumerable === false);
  row(error(() => String.prototype[name].call(null)) === 'TypeError');
  row(error(() => new String.prototype[name]()) === 'TypeError');
  row(String.prototype[name].name === name && String.prototype[name].length === 0);
}
// js_source.rs string_methods_budget_and_gc
{ let s = 'I' + '̣'.repeat(4096) + '̇'; row(s.toLocaleLowerCase('tr').length === s.length - 1); }
// js_source.rs string_methods_wait_collect_and_migrate
{ let count = 0; row('i'.toLocaleUpperCase({length: 1, get 0() { count++; return 'tr'; }}) === 'İ' && count === 1); }
{ let count = 0; row('I'.toLocaleLowerCase({length: 1, get 0() { count++; return 'tr'; }}) === 'ı' && count === 1); }
// js_source.rs string_locale_case_receiver_precedes_locales (the rows both
// oracles share).
for (let [method, input, expected] of [[String.prototype.toLocaleLowerCase, 'I', 'ı'], [String.prototype.toLocaleUpperCase, 'i', 'İ']]) {
  { let log = ''; let locales = {get length() { log += 'l'; return 1; }, get 0() { log += 'i'; return {toString() { log += 'c'; return 'tr'; }}; }};
    row(method.call(input, locales) === expected && log === 'lic'); }
  row(method.call({toString() { return input; }}, ['tr']) === expected);
}
// Planner rows.
row('i'.toLocaleUpperCase() + 'I'.toLocaleLowerCase() + 'i'.toLocaleUpperCase(['tr', 'en']) === 'Iiİ');
// The tailorings and the rest of the list.
row('i'.toLocaleUpperCase('tr') === 'İ' && 'I'.toLocaleLowerCase('az') === 'ı' && 'İ'.toLocaleLowerCase('tr') === 'i');
row('I'.toLocaleLowerCase('TR') === 'ı' && 'i'.toLocaleUpperCase('tr-TR') === 'İ' && 'i'.toLocaleUpperCase('az-Latn-AZ') === 'İ');
row('i'.toLocaleUpperCase('en') === 'I' && 'I'.toLocaleLowerCase('de') === 'i');
row('i'.toLocaleUpperCase(['en', 'tr']) === 'I' && 'I'.toLocaleLowerCase(['lt', 'tr']) === 'i');
row('I'.toLocaleLowerCase([]) === 'i' && 'i'.toLocaleUpperCase(undefined) === 'I');
row('I'.toLocaleLowerCase('tr-u-co-phonebk') === 'ı' && 'I'.toLocaleLowerCase('tr-x-private') === 'ı');
row('İ'.toLocaleLowerCase('en') === 'i̇' && 'ı'.toLocaleUpperCase('en') === 'I');
row('Iİ'.toLocaleLowerCase('lt') === 'ii̇' && 'J̀'.toLocaleLowerCase('lt') === 'j̇̀' && 'Į́'.toLocaleLowerCase('lt') === 'į̇́');
row('Ì'.toLocaleLowerCase('lt') === 'i̇̀' && 'IA'.toLocaleLowerCase('lt') === 'ia');
row('j̇'.toLocaleUpperCase('lt') === 'J' && 'i̇'.toLocaleUpperCase('lt') === 'I' && 'i̇'.toLocaleUpperCase('tr') === 'İ̇');
row('İ̧'.toLocaleLowerCase('tr') === 'i̧' && 'İ́'.toLocaleLowerCase('tr') === 'í');
row('ΑΣ'.toLocaleLowerCase('tr') === 'ας' && 'ß'.toLocaleUpperCase('lt') === 'SS');
row(error(() => 'a'.toLocaleUpperCase('en_US')) === 'RangeError' && error(() => 'a'.toLocaleLowerCase('')) === 'RangeError');
row(error(() => 'a'.toLocaleUpperCase(['en', 'x'])) === 'RangeError' && error(() => 'a'.toLocaleUpperCase('en-u')) === 'RangeError');
row(error(() => 'a'.toLocaleUpperCase('abcd')) === 'RangeError' && error(() => 'a'.toLocaleUpperCase('en-US-US')) === 'RangeError');
row(error(() => 'a'.toLocaleUpperCase(['tr', 1])) === 'TypeError' && error(() => 'a'.toLocaleUpperCase([null])) === 'TypeError');
row(error(() => 'a'.toLocaleUpperCase(null)) === 'TypeError');
row('i'.toLocaleUpperCase(5) === 'I' && 'i'.toLocaleUpperCase(true) === 'I');
{ let log = []; let list = new Proxy(['tr', 'en'], {get(t, k) { log.push(String(k)); return t[k]; }, has(t, k) { log.push('has ' + String(k)); return k in t; }});
  row('i'.toLocaleUpperCase(list) === 'İ' && log.join() === 'length,has 0,0,has 1,1'); }
{ let list = {length: 3, 0: 'lt', 2: 'tr'}; row('Ì'.toLocaleLowerCase(list) === 'i̇̀'); }
{ let list = {length: '2', 1: 'tr'}; row('I'.toLocaleLowerCase(list) === 'ı'); }
{ let list = {length: {valueOf() { return 1; }}, 0: {toString() { return 'az'; }}}; row('i'.toLocaleUpperCase(list) === 'İ'); }
row(error(() => 'a'.toLocaleUpperCase({length: 1, get 0() { throw new EvalError('x'); }})) === 'EvalError');
row(error(() => 'a'.toLocaleUpperCase({length: 1, 0: {toString() { return 'en_US'; }}})) === 'RangeError');
row(error(() => 'a'.toLocaleUpperCase({length: Symbol()})) === 'TypeError');
failed === 0 ? rows : -failed;
