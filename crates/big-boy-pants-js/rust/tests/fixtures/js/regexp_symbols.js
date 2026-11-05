// Synthetic fixture, not light-cljs output. Runs without source rewriting.
function check(value) { if (!value) throw new Error('RegExp symbol assertion'); }
check('a1b2'.match(/\d/g).join() === '1,2');
check('abc'.search(/c/) === 2);
check('a1b2'.split(/\d/).join('|') === 'a|b|');
check('a1b2'.replace(/\d/g, (match, offset) => offset) === 'a1b3');
let matches = [...'a1b2'.matchAll(/\d/g)];
check(matches.length === 2 && matches[0].index === 1 && matches[1][0] === '2');
let custom = {[Symbol.replace](text, replacement) { return replacement + text; }};
check('abc'.replace(custom, 'X') === 'Xabc');
let regexp = /,/;
regexp.constructor = {[Symbol.species]: function(original, flags) {
    check(original === regexp && flags === 'y');
    return /,/y;
}};
check('a,b'.split(regexp).join('|') === 'a|b');
true;
