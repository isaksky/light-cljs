// bbp-authored conformance case (light-cljs-933).
// Derives from: crates/big-boy-pants-js/rust/src/regexp/matcher/tests.rs lines 52-287 (the Pants engine matcher's regression table: upstream_rows, semantics, rollback_and_atomic_lookahead, assertions_classes_and_modifiers, offsets_search_and_code_units) and 351-381 (explicit_stacks_and_shared_work_limit), their case-sensitive rows without the step-limit rows, which QuickJS does not share:
// each row runs one pattern on one input from position 0 and compares the
// match's index and every group (undefined for an unmatched one), or no
// match. Left out: case-insensitive rows (light-cljs-936) and lookbehind
// (light-cljs-939). Each row is a `row(...)` check; the script completes
// with the number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
// check(pattern, flags, input, null) or check(..., [index, group0, group1, ...]).
function check(pattern, flags, input, expected) {
  let m = new RegExp(pattern, flags).exec(input);
  if (expected === null) { row(m === null); return; }
  if (m === null || m.index !== expected[0] || m.length !== expected.length - 1) { row(false); return; }
  for (let i = 1; i < expected.length; i++) { if (m[i - 1] !== expected[i]) { row(false); return; } }
  row(true);
}
// upstream_rows (QuickJS's test_regexp).
check('(b+)c', '', 'abbbbbc', [1, 'bbbbbc', 'bbbbb']);
check('(\\.(?=com|org)|\\/)', '', 'ah.com', [2, '.', '.']);
check('(\\.(?!com|org)|\\/)', '', 'ah.com', null);
check('(?=(a+))', '', 'baaabac', [1, '', 'aaa']);
check('(z)((a+)?(b+)?(c))*', '', 'zaacbbbcac', [0, 'zaacbbbcac', 'z', 'ac', 'a', undefined, 'c']);
check('(?:(?=(abc)))a', '', 'abc', [0, 'a', 'abc']);
check('(?:(?=(abc)))?a', '', 'abc', [0, 'a', undefined]);
check('(?:(?=(abc))){0,2}a', '', 'abc', [0, 'a', undefined]);
check('(?:|[\\w])+([0-9])', '', '123a23', [0, '123a23', '3']);
check('()*?a', '', ',', null);
check('\\c0', '', '\\c0', [0, '\\c0']);
check('a{1+', '', 'a{11', [0, 'a{11']);
check('[\\q{a\\b}]', '', 'a\b', [0, 'a']);
check('[\\b]', '', '\b', [0, '\b']);
check('\\x61', '', 'a', [0, 'a']);
check('\\u0061', '', 'a', [0, 'a']);
check('\\ca', '', '\x01', [0, '\x01']);
check('\\\\a', '', '\\a', [0, '\\a']);
check('\\0a', '', '\0a', [0, '\0a']);
// semantics
check('a+?', '', 'aaa', [0, 'a']);
check('a+', '', 'aaa', [0, 'aaa']);
check('a{2,3}', '', 'aaaa', [0, 'aaa']);
check('a{2,3}?', '', 'aaaa', [0, 'aa']);
check('a{2,3}', '', 'a', null);
check('^b', 'm', 'a\nb', [2, 'b']);
check('a.b', 's', 'a\nb', [0, 'a\nb']);
check('a.b', '', 'a\nb', null);
check('\\bcat\\b', '', 'a cat!', [2, 'cat']);
check('\\Bcat\\B', '', 'scats', [1, 'cat']);
check('(a+)b\\1', '', 'aaabaaa', [0, 'aaabaaa', 'aaa']);
// rollback_and_atomic_lookahead
check('(a|ab)c', '', 'abc', [0, 'abc', 'ab']);
check('((a)|b)+', '', 'ab', [0, 'ab', 'b', undefined]);
check('(a?){2}', '', 'a', [0, 'a', '']);
check('(a?)*', '', 'a', [0, 'a', 'a']);
check('(a?){0}', '', 'a', [0, '', undefined]);
check('(a|ab)+?c', '', 'abc', [0, 'abc', 'ab']);
check('(a(b)?)+b', '', 'aab', [0, 'aab', 'a', undefined]);
check('(?=(a+))a*b\\1', '', 'baaabac', [3, 'aba', 'a']);
check('(?!(a))b\\1', '', 'b', [0, 'b', undefined]);
check('(?!(a)c)a\\1', '', 'aa', [0, 'a', undefined]);
check('(?=(a))b|a', '', 'a', [0, 'a', undefined]);
check('(?!(?!a))a', '', 'a', [0, 'a']);
check('(?!(?=a))b', '', 'b', [0, 'b']);
check('(?=(?=(a)))a', '', 'a', [0, 'a', 'a']);
check('(?=(a|ab))\\1c', '', 'abc', null);
check('\\1(a)', '', 'a', [0, 'a', 'a']);
check('(a\\1)', '', 'a', [0, 'a', 'a']);
check('(?:(?<x>a)|(?<x>b))\\k<x>', '', 'bb', [0, 'bb', undefined, 'b']);
// assertions_classes_and_modifiers
for (let newline of ['\n', '\r', ' ', ' ']) {
  check('^b$', 'm', 'a' + newline + 'b' + newline + 'c', [2, 'b']);
  check('a.b', '', 'a' + newline + 'b', null);
  check('a.b', 's', 'a' + newline + 'b', [0, 'a' + newline + 'b']);
}
check('a$', '', 'a\n', null);
check('a$', 'm', 'a\n', [0, 'a']);
check('(?m:^b$)', '', 'a\nb\nc', [2, 'b']);
check('(?-m:^b)', 'm', 'a\nb', null);
check('(?s:a.b).', '', 'a\nb\n', null);
check('(?s:a.b)(?-s:.)', 's', 'a\nb\n', null);
check('[^a-c\\d]+', '', 'ab7XYZ', [3, 'XYZ']);
check('[\\D]+', '', '123abc', [3, 'abc']);
check('\\w+\\W\\d+\\D', '', 'ab_!42?', [0, 'ab_!42?']);
check('[]', '', 'x', null);
check('[^]', '', '\n', [0, '\n']);
check('\\b', '', '', null);
check('\\B', '', '', [0, '']);
for (let c of ['\t', '\n', '\v', '\f', '\r', ' ', '\xa0', ' ', ' ', ' ', ' ', ' ', ' ', ' ', '　', '﻿']) {
  check('^\\s$', '', c, [0, c]);
  check('^\\S$', '', c, null);
}
check('\\S', '', '\x85', [0, '\x85']);
// offsets_search_and_code_units
check('(?=(ab))a', '', 'ab', [0, 'a', 'ab']);
check('b', 'gd', 'ab', [1, 'b']);
check('$', '', 'abc', [3, '']);
// explicit_stacks_and_shared_work_limit: deep nesting and long repeats
// within the allowance.
{
  let open = '(?:', close = ')', pattern = 'a';
  for (let i = 0; i < 2000; i++) pattern = open + pattern + close;
  check(pattern, '', 'a', [0, 'a']);
  let input = 'a';
  while (input.length < 8192) input += input;
  let m = /(a)*/.exec(input);
  row(m[0].length === 8192 && m[1] === 'a' && m.index === 0);
}
rows = failed === 0 ? rows : -failed;
rows;
