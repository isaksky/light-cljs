// Synthetic fixture, not light-cljs output.
function check(v) { if (!v) throw new Error('RegExp script assertion'); }
check(/\p{Script=Greek}/u.test('α'));
check(/\p{scx=Grek}/u.test('α'));
for (let flag of ['u', 'v', 'iu', 'iv']) {
    for (let key of ['Script', 'sc', 'Script_Extensions', 'scx']) {
        for (let name of ['Greek', 'Grek']) {
            let re = new RegExp('^\\p{' + key + '=' + name + '}$', flag);
            let neg = new RegExp('^\\P{' + key + '=' + name + '}$', flag);
            check(re.test('α') && re.test('Ω') && !re.test('A'));
            check(!neg.test('α') && neg.test('A') && neg.test('😀'));
        }
    }
    check(new RegExp('\\p{sc=Latn}', flag).test('A'));
    check(new RegExp('\\p{sc=Cyrl}', flag).test('Ж'));
    check(new RegExp('\\p{sc=Deseret}', flag).test('\u{10400}'));
    check(new RegExp('\\p{sc=Copt}', flag).test('Ϣ'));
    check(new RegExp('\\p{sc=Qaac}', flag).test('Ϣ'));
    check(new RegExp('\\p{sc=Unknown}', flag).test('\u0378'));
    check(new RegExp('\\p{scx=Zzzz}', flag).test('\uD800'));
    check(!new RegExp('\\p{sc=Hrkt}', flag).test('あ'));
    check(new RegExp('\\p{sc=Common}', flag).test('ー'));
    check(!new RegExp('\\p{scx=Zyyy}', flag).test('ー'));
    check(new RegExp('\\p{scx=Hira}', flag).test('ー'));
    check(new RegExp('\\p{scx=Katakana}', flag).test('ー'));
    check(!new RegExp('\\p{sc=Hira}', flag).test('ー'));
    check(new RegExp('\\p{sc=Inherited}', flag).test('\u0342'));
    check(new RegExp('\\p{sc=Qaai}', flag).test('\u0342'));
    check(!new RegExp('\\p{scx=Zinh}', flag).test('\u0342'));
    check(new RegExp('\\p{scx=Greek}', flag).test('\u0342'));
    check(new RegExp('\\p{scx=Inherited}', flag).test('\u034F'));
    check(new RegExp('\\p{scx=Common}', flag).test('!'));
    for (let invalid of ['Script=greek', 'script=Greek', 'sc=GREEK', 'scx=Nope',
                         'scx=', 'sc=Grek=Greek', 'sc=Old Italic', 'Greek']) {
        let caught = false;
        try { new RegExp('\\p{' + invalid + '}', flag); }
        catch(e) { caught = e instanceof SyntaxError && e.message.indexOf('SyntaxError:') < 0; }
        check(caught);
    }
}
check(/[\p{sc=Greek}]/u.test('α'));
check(/[\p{scx=Hira}&&\p{scx=Kana}]/v.test('ー'));
check(!/[\p{scx=Hira}--\p{scx=Kana}]/v.test('ー'));
check(/[^\P{sc=Greek}]/iu.test('Ω'));
check(/\p{sc=Latn}/iu.test('K') && /\p{sc=Grek}/iv.test('Ω'));
check(!/\P{sc=Greek}/iv.test('α'));
check(/\p{sc=Inherited}/iu.test('ι') && /\p{sc=Inherited}/iv.test('ι'));
check(/\P{sc=Greek}/iu.test('ι') && !/\P{sc=Greek}/iv.test('ι'));
check(/(?i:\p{sc=Greek})(?-i:\p{scx=Hira})/u.test('Ωー'));
let exhausted = false;
let longInput = '';
for (let i = 0; i < 25; i++) longInput += 'α';
try { /(\p{sc=Greek}+)+b/u.test(longInput); }
catch(e) { exhausted = e instanceof RangeError && e.message.indexOf('step limit') >= 0; }
check(exhausted);
true;
