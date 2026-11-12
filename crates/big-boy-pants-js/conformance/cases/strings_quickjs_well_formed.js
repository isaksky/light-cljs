// bbp-authored conformance case (light-cljs-903).
// Derives from: none (the Pants engine lacks both well-formedness methods).
// QuickJS-only slice (light-cljs-903): js_string_isWellFormed and
// js_string_toWellFormed, ECMAScript 22.1.3.10 and 22.1.3.31.
// The Pants engine lacks both methods. Each row is an assertion.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function error(f) { try { f(); } catch (e) { return e.name; } return ''; }
for (let s of ['', 'abc', 'é', '€', '😀', '\ud800\udc00', '\udbff\udfff']) row(s.isWellFormed() && s.toWellFormed() === s);
for (let pair of [['\ud800', '�'], ['\udc00', '�'], ['a\ud800b', 'a�b'], ['\udc00\ud800', '��'], ['\ud800\ud800\udc00', '�\ud800\udc00'], ['\ud800\udc00\udfff', '\ud800\udc00�']]) row(!pair[0].isWellFormed() && pair[0].toWellFormed() === pair[1] && pair[0].toWellFormed().isWellFormed());
row('a\ud800b'.toWellFormed().charCodeAt(1) === 65533);
for (let f of ['isWellFormed', 'toWellFormed']) {
  row(error(() => String.prototype[f].call(null)) === 'TypeError' && error(() => String.prototype[f].call(undefined)) === 'TypeError');
  row(error(() => String.prototype[f].call(Symbol())) === 'TypeError' && error(() => new String.prototype[f]()) === 'TypeError');
  let d = Object.getOwnPropertyDescriptor(String.prototype, f);
  row(d.value.name === f && d.value.length === 0 && d.writable && !d.enumerable && d.configurable);
}
row(String.prototype.isWellFormed.call(123) && String.prototype.toWellFormed.call({toString() { return 'x\ud800'; }}) === 'x�');
{ let s = ''; for (let i = 0; i < 1100; i++) s += 'a'; s += '\ud800b'; row(!s.isWellFormed() && s.toWellFormed().charCodeAt(1100) === 65533); }
failed === 0 ? rows : -failed;
