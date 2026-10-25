// Synthetic fixture, not light-cljs output (clj-quickjs-300).
function check(b) { if (!b) throw new Error('String methods'); }
check(''.toUpperCase() === '' && ''.toLowerCase() === '');
check('ßﬃΐ'.toUpperCase() === 'SSFFIΙ\u0308\u0301');
check('İ'.toLowerCase() === 'i\u0307');
check('ΟΣ\u0301'.toLowerCase() === 'ος\u0301');
check('ΟΣ\u0301Α'.toLowerCase() === 'οσ\u0301α');
check('Σ'.toLowerCase() === 'σ' && 'AΣ\u0345'.toLowerCase() === 'aς\u0345');
check('\ud801\udc28'.toUpperCase() === '\ud801\udc00');
check('\ud800x\udfff'.toUpperCase() === '\ud800X\udfff');
check('Iİiı'.toLocaleLowerCase('tr') === 'ıiiı');
check('Iİiı'.toLocaleUpperCase('az') === 'IİİI');
check('I\u0307'.toLocaleLowerCase('tr') === 'i');
check('I\u0301\u0307'.toLocaleLowerCase('tr') === 'ı\u0301\u0307');
check('I\u0323\u0307'.toLocaleLowerCase('tr') === 'i\u0323');
check('I\u0301'.toLocaleLowerCase('lt') === 'i\u0307\u0301');
check('ÌÍĨ'.toLocaleLowerCase('lt') === 'i\u0307\u0300i\u0307\u0301i\u0307\u0303');
check('i\u0301\u0307'.toLocaleUpperCase('lt') === 'I\u0301');
check('i\u0345\u0307'.toLocaleUpperCase('lt') === 'I\u0345');
check('i'.toLocaleUpperCase('en-US') === 'I');
check('\ufeff\u2028\u2029\u00a0 x \t\r\n'.trim() === 'x');
check('\u0085x\u200b'.trim() === '\u0085x\u200b');
check('  a  '.trimStart() === 'a  ' && '  a  '.trimEnd() === '  a');
check('  '.trim() === '' && ''.trimStart() === '' && ''.trimEnd() === '');
check('ab'.repeat(2.9) === 'abab' && 'x'.repeat(NaN) === '' && ''.repeat(1e100) === '');
check('x'.repeat(-0.5) === '' && 'x'.repeat(-0) === '');
for (let n of [-1, Infinity, -Infinity]) {
    let caught = false; try { ''.repeat(n); } catch(e) { caught = e instanceof RangeError; } check(caught);
}
check('abc'.startsWith('b', 1) && !'abc'.startsWith('b', 0));
check('abc'.startsWith('', Infinity) && 'abc'.startsWith('a', -Infinity));
check('abc'.endsWith('b', 2) && !'abc'.endsWith('c', 2));
check('abc'.endsWith('c', undefined) && 'abc'.endsWith('', -Infinity));
check('abc'.includes('bc', 1) && !'abc'.includes('a', 1));
check('abc'.includes('', Infinity) && 'undefined'.includes());
check('\ud83d\ude00'.at(0) === '\ud83d' && '\ud83d\ude00'.at(-1) === '\ude00');
check('abc'.at(-4) === undefined && ''.at(0) === undefined && 'abc'.at(Infinity) === undefined);
check('abc'.at(NaN) === 'a' && 'abc'.at(-1.5) === 'c');
check('é'.normalize('NFD') === 'e\u0301' && 'e\u0301'.normalize('NFC') === 'é');
check('Å'.normalize() === 'Å' && 'ﬁ①'.normalize('NFKD') === 'fi1');
check('ﬁ①'.normalize('NFKC') === 'fi1' && 'ﬁ①'.normalize('NFC') === 'ﬁ①');
check('a\u0315\u0300'.normalize() === 'à\u0315');
check('a\u0305\u0301'.normalize() === 'a\u0305\u0301');
check('\u0344'.normalize() === '\u0308\u0301');
check('\u1100\u1161\u11a8'.normalize() === '각' && '각'.normalize('NFD') === '\u1100\u1161\u11a8');
check('\ud800é\udfff'.normalize('NFD') === '\ud800e\u0301\udfff');
check('\ud834\udd5e'.normalize() === '\ud834\udd57\ud834\udd65');
check(''.normalize('NFKD') === '' && 'abc'.normalize(undefined) === 'abc');
check('2'.localeCompare('10', 'en', {numeric:true}) < 0);
check('ä'.localeCompare('ae', 'de-u-co-phonebk', {sensitivity:'base'}) === 0);
check('A'.localeCompare('a', 'en', {sensitivity:'base'}) === 0);
check('é'.localeCompare('e\u0301') === new Intl.Collator().compare('é', 'e\u0301'));
for (let name of ['startsWith','endsWith','includes']) {
    let caught = false; try { 'abc'[name](/a/); } catch(e) { caught = e instanceof TypeError; } check(caught);
}
for (let name of ['toUpperCase','toLowerCase','toLocaleUpperCase','toLocaleLowerCase','trim','trimStart','trimEnd','repeat','startsWith','endsWith','includes','at','localeCompare','normalize']) {
    check(typeof String.prototype[name] === 'function');
    check(Object.getOwnPropertyDescriptor(String.prototype, name).enumerable === false);
    let caught = false; try { String.prototype[name].call(null); } catch(e) { caught = e instanceof TypeError; } check(caught);
    caught = false; try { new String.prototype[name](); } catch(e) { caught = e instanceof TypeError; } check(caught);
}
for (let form of ['nfc', '', 'NFKX']) {
    let caught = false; try { 'x'.normalize(form); } catch(e) { caught = e instanceof RangeError; } check(caught);
}
for (let name of ['toUpperCase','toLowerCase','toLocaleUpperCase','toLocaleLowerCase','trim','trimStart','trimEnd','normalize']) {
    check(String.prototype[name].name === name && String.prototype[name].length === 0);
}
for (let name of ['repeat','startsWith','endsWith','includes','at','localeCompare']) {
    check(String.prototype[name].name === name && String.prototype[name].length === 1);
}
check(String.prototype.trim.call(123) === '123');
check(String.prototype.repeat.call(true,2) === 'truetrue');
check(String.prototype.at.call(123,1) === '2');
let order = '';
let receiver = {toString() { order += 's'; return 'abc'; }};
let needle = {toString() { order += 'n'; return 'b'; }};
let pos = {valueOf() { order += 'p'; return 1; }};
check(String.prototype.startsWith.call(receiver, needle, pos) && order === 'snp');
order = '';
check(String.prototype.repeat.call(receiver, {valueOf(){order += 'n'; return 2;}}) === 'abcabc' && order === 'sn');
let count = 0;
try { String.prototype.includes.call({toString(){throw 77;}}, {toString(){count++; return 'a';}}); } catch(e) { check(e === 77); }
check(count === 0);
true;
