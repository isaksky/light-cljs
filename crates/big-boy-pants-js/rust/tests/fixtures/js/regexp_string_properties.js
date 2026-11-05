// Synthetic fixture, not light-cljs output.
function check(v) { if (!v) throw new Error('RegExp string property assertion'); }
let names = ['Basic_Emoji', 'Emoji_Keycap_Sequence', 'RGI_Emoji_Modifier_Sequence',
             'RGI_Emoji_Flag_Sequence', 'RGI_Emoji_Tag_Sequence', 'RGI_Emoji_ZWJ_Sequence',
             'RGI_Emoji'];
let samples = ['😀', '1\uFE0F\u20E3', '👍🏽', '🇺🇸',
               '\u{1F3F4}\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F}',
               '👩🏽‍🚀', '👨‍👩‍👧‍👦'];
for (let i = 0; i < names.length; i++) {
    let source = '\\p{' + names[i] + '}';
    check(RegExp('^' + source + '$', 'v').test(samples[i]));
    check(RegExp('^[' + source + ']$', 'vi').test(samples[i]));
    check(!RegExp('^' + source + '$', 'v').test('A'));
    check(/^\p{RGI_Emoji}$/v.test(samples[i]));
    for (let invalid of [[source, 'u'], ['\\P{' + names[i] + '}', 'v'],
                         ['[^' + source + ']', 'v']]) {
        let caught = false;
        try { RegExp(invalid[0], invalid[1]); }
        catch(e) { caught = e instanceof SyntaxError; }
        check(caught);
    }
}
check(/^\p{Basic_Emoji}$/v.test('©️'));
check(!/^\p{Basic_Emoji}$/v.test('©'));
check(!/^\p{Emoji_Keycap_Sequence}$/v.test('1\u20E3'));
check(!/^\p{RGI_Emoji_Flag_Sequence}$/v.test('🇦🇦'));
check(!/^\p{RGI_Emoji_Modifier_Sequence}$/v.test('😀🏽'));
check(!/^\p{RGI_Emoji_Modifier_Sequence}$/v.test('👪🏽'));
check(!/^\p{RGI_Emoji}$/v.test('👪🏽')); // Emoji 17 excludes FAMILY + modifier.
check(!/^\p{RGI_Emoji_ZWJ_Sequence}$/v.test('👩‍😀'));
check(/^\p{Basic_Emoji}$/v.test('\u{1FAEA}')); // Emoji 17 distorted face.
check(/\p{RGI_Emoji}/v.exec('👩🏽‍🚀!')[0] === '👩🏽‍🚀');
check(/[\p{RGI_Emoji}\q{👩🏽‍🚀!}]/v.exec('👩🏽‍🚀!')[0] === '👩🏽‍🚀!');
check(/^[\p{RGI_Emoji}&&\q{👩🏽‍🚀|x}]$/v.test('👩🏽‍🚀'));
check(!/^[\p{RGI_Emoji}&&\q{👩🏽‍🚀|x}]$/v.test('x'));
check(!/^[\p{RGI_Emoji}--\p{Basic_Emoji}]$/v.test('😀'));
check(/^[\p{RGI_Emoji}--\p{Basic_Emoji}]$/v.test('👍🏽'));
check(!/^[\p{RGI_Emoji}--\q{👍🏽}]$/v.test('👍🏽'));
check(/^[\q{👍🏽|x}--\p{RGI_Emoji}]$/v.test('x'));
check(!/^[\q{👍🏽|x}--\p{RGI_Emoji}]$/v.test('👍🏽'));
check(/^[^\p{RGI_Emoji}&&\p{ASCII}]$/v.test('a'));
check(/^[^\p{ASCII}--\p{RGI_Emoji}]$/v.test('😀'));
check(/^\p{RGI_Emoji}\u{1F3FD}$/v.exec('👍🏽')[0] === '👍🏽'); // Backtrack to 👍.
let indexed = /\p{RGI_Emoji}/dgv;
let m = indexed.exec('x👩🏽‍🚀y🇺🇸');
check(m[0] === '👩🏽‍🚀' && m.index === 1 && m.indices[0][1] === 8 && indexed.lastIndex === 8);
check(indexed.exec('x👩🏽‍🚀y🇺🇸')[0] === '🇺🇸' && indexed.lastIndex === 13);
check('😀👍🏽'.replace(/\p{RGI_Emoji}/gv, 'x') === 'xx');
let caught = false;
try { RegExp('[\\p{RGI_Emoji}]'.repeat(100), 'v'); }
catch(e) { caught = e instanceof RangeError; }
check(caught);
true;
