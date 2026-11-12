// bbp-authored conformance case (light-cljs-904).
// Derives from: none (Annex B expectations from QuickJS js_string_substr and js_string_CreateHTML)
// QuickJS-only Annex B rows (light-cljs-904); QuickJS js_string_substr,
// js_string_CreateHTML and trimLeft/trimRight realm aliases.
// QuickJS rejects nullish HTML attributes; these shared-oracle rows use
// defined values. CreateHTML itself uses ToString for the attribute.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
row('abc'.substr(-2, 1) + 'abc'.substr(1) === 'bbc');
row('abc'.substr(-Infinity, Infinity) === 'abc' && 'abc'.substr(Infinity) === '' && 'abc'.substr(1, -2) === '');
row('abc'.substr(NaN, 1.9) === 'a' && 'abc'.substr(-1, undefined) === 'c' && 'abc'.substr(0, {valueOf() { return undefined; }}) === '');
row(String.prototype.trimLeft === String.prototype.trimStart && String.prototype.trimRight === String.prototype.trimEnd && String.prototype.trimLeft.name === 'trimStart');
row('x'.anchor('"a"') + 'x'.bold() + 'x'.link('u') === '<a name="&quot;a&quot;">x</a><b>x</b><a href="u">x</a>');
for (let pair of [['big','big'],['blink','blink'],['bold','b'],['fixed','tt'],['italics','i'],['small','small'],['strike','strike'],['sub','sub'],['sup','sup']]) row('x'[pair[0]]() === '<' + pair[1] + '>x</' + pair[1] + '>');
row('x'.fontcolor('r"&<>') === '<font color="r&quot;&<>">x</font>' && 'x'.fontsize(3) === '<font size="3">x</font>');
{ let log = ''; let r = {toString() { log += 'r'; return 'x'; }}, a = {toString() { log += 'a'; return '"'; }}; row(String.prototype.link.call(r, a) === '<a href="&quot;">x</a>' && log === 'ra'); }
{ let count = 0; row('x'.bold({toString() { count++; throw 1; }}) === '<b>x</b>' && count === 0); }
for (let pair of [['substr',2],['anchor',1],['big',0],['blink',0],['bold',0],['fixed',0],['fontcolor',1],['fontsize',1],['italics',0],['link',1],['small',0],['strike',0],['sub',0],['sup',0]]) {
  let f = pair[0], d = Object.getOwnPropertyDescriptor(String.prototype, f), caught = false;
  row(d.value.name === f && d.value.length === pair[1] && d.writable && !d.enumerable && d.configurable);
  try { String.prototype[f].call(null, {toString() { throw 1; }}); } catch(e) { caught = e instanceof TypeError; } row(caught);
}
failed === 0 ? rows : -failed;
