use super::*;
use crate::{JsString, regexp::parse::parse};

fn program(pattern: &str, flags: &str) -> Result<Program, MatchError> {
    compile(&parse(&JsString::from(pattern), &JsString::from(flags)).unwrap())
}

fn check(pattern: &str, flags: &str, input: &str, expected: Option<(usize, &[Option<&str>])>) {
    let input: Vec<u16> = input.encode_utf16().collect();
    let result = exec(&program(pattern, flags).unwrap(), &input, 0).unwrap();
    match (result, expected) {
        (None, None) => {}
        (Some(actual), Some(expected)) => {
            assert_eq!(actual.index, expected.0, "{pattern}");
            let actual_groups: Vec<_> = actual
                .groups
                .iter()
                .map(|range| range.as_ref().map(|r| input[r.clone()].to_vec()))
                .collect();
            let wanted: Vec<_> = expected
                .1
                .iter()
                .map(|group| group.map(|s| s.encode_utf16().collect::<Vec<_>>()))
                .collect();
            assert_eq!(actual_groups, wanted, "{pattern}");
        }
        (actual, expected) => panic!("{pattern}: {actual:?} != {expected:?}"),
    }
}

// Adapted from QuickJS tests/test_builtin.js::test_regexp (7d1c13c5d8f8).
// Copyright (c) 2017-2021 Fabrice Bellard
// Copyright (c) 2017-2021 Charlie Gordon
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
// THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.
#[test]
fn upstream_rows() {
    check(
        "(b+)c",
        "",
        "abbbbbc",
        Some((1, &[Some("bbbbbc"), Some("bbbbb")])),
    );
    check(
        r"(\.(?=com|org)|\/)",
        "",
        "ah.com",
        Some((2, &[Some("."), Some(".")])),
    );
    check(r"(\.(?!com|org)|\/)", "", "ah.com", None);
    check(
        "(?=(a+))",
        "",
        "baaabac",
        Some((1, &[Some(""), Some("aaa")])),
    );
    check(
        "(z)((a+)?(b+)?(c))*",
        "",
        "zaacbbbcac",
        Some((
            0,
            &[
                Some("zaacbbbcac"),
                Some("z"),
                Some("ac"),
                Some("a"),
                None,
                Some("c"),
            ],
        )),
    );
    check(
        "(?:(?=(abc)))a",
        "",
        "abc",
        Some((0, &[Some("a"), Some("abc")])),
    );
    for pattern in ["(?:(?=(abc)))?a", "(?:(?=(abc))){0,2}a"] {
        check(pattern, "", "abc", Some((0, &[Some("a"), None])));
    }
    check(
        r"(?:|[\w])+([0-9])",
        "",
        "123a23",
        Some((0, &[Some("123a23"), Some("3")])),
    );
    check("()*?a", "", ",", None);
    for (pattern, input, matched) in [
        (r"\c0", r"\c0", r"\c0"),
        ("a{1+", "a{11", "a{11"),
        (r"[\q{a\b}]", "a\u{8}", "a"),
        (r"[\b]", "\u{8}", "\u{8}"),
        (r"\x61", "a", "a"),
        (r"\u0061", "a", "a"),
        (r"\ca", "\u{1}", "\u{1}"),
        (r"\\a", r"\a", r"\a"),
        (r"\0a", "\0a", "\0a"),
    ] {
        check(pattern, "", input, Some((0, &[Some(matched)])));
    }
}

#[test]
fn semantics() {
    check("a+?", "", "aaa", Some((0, &[Some("a")])));
    check("a+", "", "aaa", Some((0, &[Some("aaa")])));
    check("a{2,3}", "", "aaaa", Some((0, &[Some("aaa")])));
    check("a{2,3}?", "", "aaaa", Some((0, &[Some("aa")])));
    check("a{2,3}", "", "a", None);
    check("^b", "m", "a\nb", Some((2, &[Some("b")])));
    check("a.b", "s", "a\nb", Some((0, &[Some("a\nb")])));
    check("a.b", "", "a\nb", None);
    check(r"\bcat\b", "", "a cat!", Some((2, &[Some("cat")])));
    check(r"\Bcat\B", "", "scats", Some((1, &[Some("cat")])));
    check(
        r"(a+)b\1",
        "",
        "aaabaaa",
        Some((0, &[Some("aaabaaa"), Some("aaa")])),
    );
    assert_eq!(
        exec(&program("(a*)*b", "").unwrap(), &[97; 30], 0),
        Err(MatchError::LimitExceeded)
    );
}

#[test]
fn rollback_and_atomic_lookahead() {
    // Synthetic fixtures, not light-cljs output.
    check("(a|ab)c", "", "abc", Some((0, &[Some("abc"), Some("ab")])));
    check(
        "((a)|b)+",
        "",
        "ab",
        Some((0, &[Some("ab"), Some("b"), None])),
    );
    check("(a?){2}", "", "a", Some((0, &[Some("a"), Some("")])));
    check("(a?)*", "", "a", Some((0, &[Some("a"), Some("a")])));
    check("(a?){0}", "", "a", Some((0, &[Some(""), None])));
    check(
        "(a|ab)+?c",
        "",
        "abc",
        Some((0, &[Some("abc"), Some("ab")])),
    );
    check(
        "(a(b)?)+b",
        "",
        "aab",
        Some((0, &[Some("aab"), Some("a"), None])),
    );
    check(
        r"(?=(a+))a*b\1",
        "",
        "baaabac",
        Some((3, &[Some("aba"), Some("a")])),
    );
    check(r"(?!(a))b\1", "", "b", Some((0, &[Some("b"), None])));
    check(r"(?!(a)c)a\1", "", "aa", Some((0, &[Some("a"), None])));
    check("(?=(a))b|a", "", "a", Some((0, &[Some("a"), None])));
    check("(?!(?!a))a", "", "a", Some((0, &[Some("a")])));
    check("(?!(?=a))b", "", "b", Some((0, &[Some("b")])));
    check("(?=(?=(a)))a", "", "a", Some((0, &[Some("a"), Some("a")])));
    // A successful lookahead is atomic: its alternatives cannot be revisited
    // when the following term fails.
    check("(?=(a|ab))\\1c", "", "abc", None);
    check(r"\1(a)", "", "a", Some((0, &[Some("a"), Some("a")])));
    check(r"(a\1)", "", "a", Some((0, &[Some("a"), Some("a")])));
    check(
        r"(?:(?<x>a)|(?<x>b))\k<x>",
        "",
        "bb",
        Some((0, &[Some("bb"), None, Some("b")])),
    );
}

#[test]
fn assertions_classes_and_modifiers() {
    for newline in ["\n", "\r", "\u{2028}", "\u{2029}"] {
        check(
            "^b$",
            "m",
            &format!("a{newline}b{newline}c"),
            Some((2, &[Some("b")])),
        );
        check("a.b", "", &format!("a{newline}b"), None);
        check(
            "a.b",
            "s",
            &format!("a{newline}b"),
            Some((0, &[Some(&format!("a{newline}b"))])),
        );
    }
    check("a$", "", "a\n", None);
    check("a$", "m", "a\n", Some((0, &[Some("a")])));
    check("(?m:^b$)", "", "a\nb\nc", Some((2, &[Some("b")])));
    check("(?-m:^b)", "m", "a\nb", None);
    check("(?s:a.b).", "", "a\nb\n", None);
    check("(?s:a.b)(?-s:.)", "s", "a\nb\n", None);
    check(r"[^a-c\d]+", "", "ab7XYZ", Some((3, &[Some("XYZ")])));
    check(r"[\D]+", "", "123abc", Some((3, &[Some("abc")])));
    check(r"\w+\W\d+\D", "", "ab_!42?", Some((0, &[Some("ab_!42?")])));
    check(r"[]", "", "x", None);
    check(r"[^]", "", "\n", Some((0, &[Some("\n")])));
    check(r"\b", "", "", None);
    check(r"\B", "", "", Some((0, &[Some("")])));
    for c in [
        0x9, 0xa, 0xb, 0xc, 0xd, 0x20, 0xa0, 0x1680, 0x2000, 0x200a, 0x2028, 0x2029, 0x202f,
        0x205f, 0x3000, 0xfeff,
    ] {
        assert!(
            exec(&program(r"^\s$", "").unwrap(), &[c], 0)
                .unwrap()
                .is_some()
        );
        assert!(
            exec(&program(r"^\S$", "").unwrap(), &[c], 0)
                .unwrap()
                .is_none()
        );
    }
    check(r"\S", "", "\u{85}", Some((0, &[Some("\u{85}")])));
}

#[test]
fn offsets_search_and_code_units() {
    let p = program("(.)", "").unwrap();
    let input = [0xd800, 0xdc00, 0xdfff];
    assert_eq!(
        exec(&p, &input, 1).unwrap(),
        Some(Captures {
            index: 1,
            groups: vec![Some(1..2), Some(1..2)],
        })
    );
    assert_eq!(exec(&p, &input, 4).unwrap(), None);
    assert_eq!(exec(&p, &input, usize::MAX).unwrap(), None);
    assert_eq!(
        exec(&program(r"(\uD800)\1", "").unwrap(), &[0xd800, 0xd800], 0).unwrap(),
        Some(Captures {
            index: 0,
            groups: vec![Some(0..2), Some(0..1)],
        })
    );
    assert_eq!(
        exec(&program("(?=(ab))a", "").unwrap(), &[97, 98], 0).unwrap(),
        Some(Captures {
            index: 0,
            groups: vec![Some(0..1), Some(0..2)],
        })
    );
    assert_eq!(
        exec(&program("^b", "").unwrap(), &[97, 98], 1).unwrap(),
        None
    );
    assert_eq!(
        exec(&program("b", "y").unwrap(), &[97, 98], 0).unwrap(),
        None
    );
    assert_eq!(
        exec(&program("b", "y").unwrap(), &[97, 98], 1)
            .unwrap()
            .unwrap()
            .index,
        1
    );
    check("b", "gd", "ab", Some((1, &[Some("b")])));
    check("$", "", "abc", Some((3, &[Some("")])));
}

#[test]
fn unsupported_features_fail_explicitly() {
    for pattern in ["(?<=a)b", "(?<!a)b"] {
        assert!(matches!(
            program(pattern, ""),
            Err(MatchError::Unsupported("lookbehind"))
        ));
    }
    assert!(matches!(
        program("a", "u"),
        Err(MatchError::Unsupported("Unicode matching"))
    ));
    assert!(matches!(
        program("a", "i"),
        Err(MatchError::Unsupported("ignore-case matching"))
    ));
    assert!(matches!(
        program("(?i:a)", ""),
        Err(MatchError::Unsupported("ignore-case matching"))
    ));
    assert!(
        MatchError::LimitExceeded
            .to_string()
            .starts_with("RangeError:")
    );
}

#[test]
fn explicit_stacks_and_shared_work_limit() {
    let depth = 20_000;
    let pattern = format!("{}a{}", "(?:".repeat(depth), ")".repeat(depth));
    check(&pattern, "", "a", Some((0, &[Some("a")])));
    let input = vec![97; 10_000];
    let matched = exec(&program("(a)*", "").unwrap(), &input, 0)
        .unwrap()
        .unwrap();
    assert_eq!(matched.groups, vec![Some(0..10_000), Some(9_999..10_000)]);
    // Even a linear failed search is bounded across candidate positions.
    assert_eq!(
        exec(&program("b", "").unwrap(), &vec![97; MAX_MATCH_STEPS], 0),
        Err(MatchError::LimitExceeded)
    );
    // Backreferences compare one code unit at a time under the same budget.
    assert_eq!(
        exec(&program(r"(?=(a*))\1b", "").unwrap(), &vec![97; 2000], 0),
        Err(MatchError::LimitExceeded)
    );
    // Mandatory empty repetitions cannot evade the limit.
    assert_eq!(
        exec(&program("(){1000000000}", "").unwrap(), &[], 0),
        Err(MatchError::LimitExceeded)
    );
    assert!(matches!(
        program(&"a".repeat(MAX_MATCH_STEPS), ""),
        Err(MatchError::LimitExceeded)
    ));
}

#[test]
fn malformed_arenas_are_rejected() {
    let mut p = parse(&JsString::from("a"), &JsString::from("")).unwrap();
    p.nodes[p.root.0] = Node::Group {
        body: p.root,
        capture: None,
    };
    assert!(matches!(compile(&p), Err(MatchError::InvalidPattern)));
}
