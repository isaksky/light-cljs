//! Terse emission (docs/advanced-optimization.md, Phase 3): what `Advanced`
//! output writes instead of the readable text of the passes before it.
//!
//! - The runtime slice embeds the terse units `build.rs` writes from
//!   `src/js_runtime.js`: no comments or whitespace, locals renamed to short
//!   names, and each top-level binding a placeholder that [`terse_slice`]
//!   names by its frequency in the slice.
//! - The program is renamed and rewritten by [`terse_program`], which
//!   replaces the hygiene pass: every compiler-bound name gets a short name
//!   by frequency, members read with a constant identifier key become dotted,
//!   redundant parentheses go and tokens are joined without whitespace.
//!
//! [`crate::ApplicationOptions::pseudo_names`] keeps the readable output.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use crate::js_syntax::{
    Join, Kind, NameGenerator, Shape, TerseWriter, Token, closes_control_head, joined_declarations,
    lex_section, partners, redundant_semicolons, reserved, resolve,
};

/// One runtime unit as `build.rs` writes it.
pub(crate) struct TerseUnit {
    /// The body, terse, with locals renamed. Each read or declaration of a
    /// top-level binding is `\u{1}<index into TERSE_BINDINGS>\u{2}`.
    pub(crate) text: &'static str,
    /// For bindings the unit names, the space-separated local names visible
    /// at some of its occurrences: the binding's name must be none of them.
    pub(crate) avoid: &'static [(usize, &'static str)],
    /// The keyword of the `let` or `const` statement the unit starts with.
    pub(crate) opens: Option<&'static str>,
    /// The keyword of the `let` or `const` statement the unit ends with,
    /// whose `;` is the text's last character.
    pub(crate) closes: Option<&'static str>,
    /// For source maps ([`crate::source_map`]): where the first token of
    /// each source line starts in `text`, and its byte offset in
    /// `src/js_runtime.js`, as pairs of Base64 VLQ deltas ([`unit_lines`]).
    pub(crate) lines: &'static str,
}

/// The pairs of [`TerseUnit::lines`]: offsets in the unit's text, each with
/// the byte offset in `src/js_runtime.js` of the text there.
fn unit_lines(encoded: &'static str) -> impl Iterator<Item = (usize, usize)> {
    let mut digits = encoded.bytes();
    let mut number = move || {
        let mut value = 0u64;
        let mut shift = 0;
        loop {
            let byte = digits.next()?;
            let digit = match byte {
                b'A'..=b'Z' => byte - b'A',
                b'a'..=b'z' => byte - b'a' + 26,
                b'0'..=b'9' => byte - b'0' + 52,
                b'+' => 62,
                _ => 63,
            };
            value |= u64::from(digit & 31) << shift;
            shift += 5;
            if digit & 32 == 0 {
                let magnitude = (value >> 1) as i64;
                return Some(if value & 1 == 1 {
                    -magnitude
                } else {
                    magnitude
                });
            }
        }
    };
    let mut at = (0i64, 0i64);
    std::iter::from_fn(move || {
        at = (at.0 + number()?, at.1 + number()?);
        Some((at.0 as usize, at.1 as usize))
    })
}

include!(concat!(env!("OUT_DIR"), "/terse_runtime.rs"));

const OPEN: u8 = 1;
const CLOSE: u8 = 2;

/// The placeholders of a terse unit text, in order: their byte ranges and
/// binding indices.
fn placeholders(text: &str) -> impl Iterator<Item = (Range<usize>, usize)> + '_ {
    let bytes = text.as_bytes();
    let mut at = 0;
    std::iter::from_fn(move || {
        let start = at + bytes[at..].iter().position(|&b| b == OPEN)?;
        let end = start + bytes[start..].iter().position(|&b| b == CLOSE)?;
        at = end + 1;
        let index = text[start + 1..end].parse().expect("a binding index");
        Some((start..end + 1, index))
    })
}

/// Whether a runtime name keeps its spelling in the slice, as `build.rs`
/// keeps it in the units: a constructor, a function or class with an
/// uppercase initial, whose `name` the host and the printer show
/// ([`TERSE_KEPT`]). No generated name is one of them.
fn keeps_name(name: &str) -> bool {
    TERSE_KEPT.binary_search(&name).is_ok()
}

/// The index of a top-level runtime binding in [`TERSE_BINDINGS`].
fn binding_index(name: &str) -> usize {
    TERSE_BINDINGS
        .binary_search(&name)
        .unwrap_or_else(|_| panic!("`{name}` is not a top-level runtime binding"))
}

/// The terse runtime factory with the `kept` units (by unit index), which
/// returns `returned` as an array, `(()=>{"use strict";...return[a,b]})()`,
/// and the bytes of each unit in it, by unit index.
///
/// Each top-level binding of the slice gets a short name: the most frequent
/// first, each the first name of the [`NameGenerator`] sequence that no
/// other binding has and no local visible at one of its occurrences has.
/// (Closure's same-length batches in order of first occurrence, which the
/// program's names use, measured no gzip gain here.)
pub(crate) fn terse_slice(kept: &[bool], returned: &[&str]) -> (String, Vec<usize>) {
    terse_slice_marked(kept, returned, None)
}

/// A mark of a terse runtime factory: an offset in the factory, the byte
/// offset in `src/js_runtime.js` of the text there, and the source name of
/// the binding a unit declares first, at the unit's start.
pub(crate) type SliceMark = (usize, usize, Option<&'static str>);

/// The terse runtime factory [`terse_slice`] writes for `kept` and
/// `returned`, with its marks: each source line of a unit maps to its first
/// token, and a unit's start is named by its first binding, which a stack
/// trace through the renamed runtime then shows.
pub(crate) fn terse_slice_marks(kept: &[bool], returned: &[&str]) -> (String, Vec<SliceMark>) {
    let mut marks = Vec::new();
    let (text, _) = terse_slice_marked(kept, returned, Some(&mut marks));
    (text, marks)
}

fn terse_slice_marked(
    kept: &[bool],
    returned: &[&str],
    mut marks: Option<&mut Vec<SliceMark>>,
) -> (String, Vec<usize>) {
    debug_assert_eq!(kept.len(), TERSE_UNITS.len());
    let units = TERSE_UNITS
        .iter()
        .enumerate()
        .zip(kept)
        .filter_map(|(unit, kept)| kept.then_some(unit))
        .collect::<Vec<_>>();
    let mut unit_bytes = vec![0; TERSE_UNITS.len()];
    // Per binding: occurrences and the order of its first occurrence.
    let mut count = vec![0usize; TERSE_BINDINGS.len()];
    let mut first = vec![usize::MAX; TERSE_BINDINGS.len()];
    let mut seen = 0;
    let mut occur = |index: usize| {
        count[index] += 1;
        if first[index] == usize::MAX {
            first[index] = seen;
            seen += 1;
        }
    };
    for (_, unit) in &units {
        for (_, index) in placeholders(unit.text) {
            occur(index);
        }
    }
    let returned = returned
        .iter()
        .map(|name| binding_index(name))
        .collect::<Vec<_>>();
    for &index in &returned {
        occur(index);
    }
    let mut avoid: HashMap<usize, HashSet<&str>> = HashMap::new();
    for (_, unit) in &units {
        for &(binding, names) in unit.avoid {
            avoid.entry(binding).or_default().extend(names.split(' '));
        }
    }
    let mut bindings = (0..TERSE_BINDINGS.len())
        .filter(|&index| count[index] > 0 && !keeps_name(TERSE_BINDINGS[index]))
        .collect::<Vec<_>>();
    bindings.sort_by_key(|&index| (std::cmp::Reverse(count[index]), first[index]));
    let mut candidates = NameGenerator::default().filter(|name| !keeps_name(name));
    let mut generated: Vec<String> = Vec::new();
    let mut taken: HashSet<usize> = HashSet::new();
    let mut names: Vec<Option<usize>> = vec![None; TERSE_BINDINGS.len()];
    for &binding in &bindings {
        let avoided = avoid.get(&binding);
        let mut at = 0;
        let chosen = loop {
            if at == generated.len() {
                generated.push(candidates.next().expect("names never run out"));
            }
            if !taken.contains(&at)
                && !avoided.is_some_and(|avoided| avoided.contains(generated[at].as_str()))
            {
                break at;
            }
            at += 1;
        };
        taken.insert(chosen);
        names[binding] = Some(chosen);
    }
    let name = |index: usize| match names[index] {
        Some(name) => generated[name].as_str(),
        None => TERSE_BINDINGS[index],
    };
    let mut text = String::from(TERSE_PROLOGUE);
    let manifest = crate::runtime_manifest::RuntimeManifest::embedded();
    if let Some(marks) = marks.as_deref_mut() {
        let prologue = &crate::runtime_manifest::ANNOTATED_RUNTIME[manifest.prologue.clone()];
        marks.push((0, prologue.find("(() =>").unwrap_or_default(), None));
    }
    let mut closes = None;
    for &(id, unit) in &units {
        let start = text.len();
        let mut at = 0;
        // `const a=1;` and `const b=2;` of adjacent units join as
        // `const a=1,b=2;` (`js_syntax::joined_declarations`).
        if let Some(keyword) = unit.opens.filter(|keyword| closes == Some(*keyword)) {
            text.pop();
            text.push(',');
            at = keyword.len() + usize::from(unit.text.as_bytes()[keyword.len()] == b' ');
        }
        // Line marks before `limit` in the unit's text, where the text from
        // `at` on is written from `text.len()` on; a mark before `at` (in
        // the `const` that joining dropped) or at a placeholder maps to
        // where the copy starts.
        let mut lines = unit_lines(unit.lines).peekable();
        let mut first = manifest.units[id].declares.first().copied();
        let mut copy = |text: &str, at: usize, limit: usize| {
            if let Some(marks) = marks.as_deref_mut() {
                while let Some((offset, source)) = lines.next_if(|(offset, _)| *offset < limit) {
                    marks.push((text.len() + offset.saturating_sub(at), source, first.take()));
                }
            }
        };
        for (range, index) in placeholders(unit.text) {
            if range.start < at {
                continue;
            }
            copy(&text, at, range.start);
            text.push_str(&unit.text[at..range.start]);
            copy(&text, range.start, range.end);
            text.push_str(name(index));
            at = range.end;
        }
        copy(&text, at, usize::MAX);
        text.push_str(&unit.text[at..]);
        unit_bytes[id] = text.len() - start;
        closes = unit.closes;
    }
    text.push_str("return[");
    for (position, &index) in returned.iter().enumerate() {
        if position > 0 {
            text.push(',');
        }
        text.push_str(name(index));
    }
    text.push_str("]})()");
    (text, unit_bytes)
}

// ---------------------------------------------------------------------------
// The program

/// Whether `word` is a name the emitter binds in a program's output: the
/// numbered families (`l7`, `g3_1`, `k0`, `c2`, `m1`, `a0`, `r5_0`, `next0`,
/// `saved1`, `temporary1`, `captures4`, `self4`, `recur5`, `installed1`,
/// `operand0`), the fixed helper names, and the `Advanced` runtime reads and
/// core cells (`rt$add`, `core$inc`). Raw `js*` text must see none of them,
/// and terse output renames all of them, wherever they occur: a capture
/// object's keys (`captures4.l7`) are such names too. A constant member key
/// that is one of them stays bracketed (`x["f"]`).
pub(crate) fn compiler_name(word: &str) -> bool {
    const NUMBERED: [&str; 15] = [
        "l",
        "g",
        "k",
        "c",
        "m",
        "a",
        "r",
        "next",
        "saved",
        "temporary",
        "captures",
        "self",
        "recur",
        "installed",
        "operand",
    ];
    NUMBERED.iter().any(|head| {
        word.strip_prefix(head).is_some_and(|tail| {
            !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit() || b == b'_')
        })
    }) || matches!(
        word,
        "rt" | "truth"
            | "host"
            | "pkg"
            | "f"
            | "text"
            | "savedPrint"
            | "savedNewline"
            | "caught"
            | "caseValue"
    ) || word.starts_with("rt$")
        || word.starts_with("core$")
}

/// A prefix that no raw `js*` segment of `source` contains: `__light<n>_`.
pub(crate) fn hygiene_prefix(source: &str, raw: &[Range<usize>]) -> String {
    let mut index = 0;
    loop {
        let candidate = format!("__light{index}_");
        if !raw
            .iter()
            .any(|range| source[range.clone()].contains(&candidate))
        {
            return candidate;
        }
        index += 1;
    }
}

/// Short names for `words`, the compiler names of a program in order of
/// occurrence: the most frequent first, from the [`NameGenerator`] sequence
/// without `excluded` names; among the names of one length, in order of
/// first occurrence, as Closure's `RenameVars` assigns them
/// (`RenameVars.java:466-498` in the pinned clone), which helps gzip.
fn short_names<'s>(
    words: impl IntoIterator<Item = &'s str>,
    excluded: impl Fn(&str) -> bool,
) -> HashMap<&'s str, String> {
    let mut stats: HashMap<&str, (usize, usize)> = HashMap::new();
    for (position, word) in words.into_iter().enumerate() {
        stats.entry(word).or_insert((0, position)).0 += 1;
    }
    let mut order = stats.keys().copied().collect::<Vec<_>>();
    order.sort_by_key(|word| (std::cmp::Reverse(stats[word].0), stats[word].1));
    let generated = NameGenerator::default()
        .filter(|name| !excluded(name))
        .take(order.len())
        .collect::<Vec<_>>();
    let mut start = 0;
    while start < order.len() {
        let length = generated[start].len();
        let end = start
            + generated[start..]
                .iter()
                .take_while(|name| name.len() == length)
                .count();
        order[start..end].sort_by_key(|word| stats[word].1);
        start = end;
    }
    order.into_iter().zip(generated).collect()
}

/// The terse output of an `Advanced` program, `source` as the emitter wrote
/// it: `segments` are its raw `js*` ranges and `runtime` the embedded runtime
/// factory, already terse ([`terse_slice`]).
///
/// Every compiler name ([`compiler_name`]) gets a short name, the most
/// frequent first. When the program has raw `js*` text, the names carry the
/// hygiene prefix (`__light0_a`), so raw text still sees only its holes and
/// host globals, and the layout is kept. Otherwise the new names avoid every
/// other identifier of the output (host globals, helper parameters,
/// properties) and the ECMAScript globals the runtime reads, and the program
/// is rewritten: a member read with a constant identifier key is dotted
/// (`x.length`), redundant parentheses and semicolons go, and tokens are
/// joined by [`TerseWriter`].
///
/// `marks`, positions in `source` ([`crate::source_map`]), move with the
/// text they mark: a mark before a token to where that token is written
/// (the next written one, for a token the rewrite drops), and a mark in
/// the runtime factory by its offset there.
pub(crate) fn terse_program(
    source: &str,
    segments: &[Range<usize>],
    runtime: Option<Range<usize>>,
    raw_js: bool,
    marks: Option<&mut crate::source_map::Marks>,
) -> String {
    let mut raw = segments
        .iter()
        .filter(|segment| Some(*segment) != runtime.as_ref() && !segment.is_empty())
        .cloned()
        .collect::<Vec<_>>();
    raw.sort_by_key(|range| range.start);
    if raw_js {
        let (output, anchors) = hygienic(source, &raw, runtime);
        if let Some(marks) = marks {
            marks.relocate(|offset| Some(renamed_offset(&anchors, offset)));
        }
        return output;
    }
    match rewritten(source, runtime.clone()) {
        Some((output, written)) => {
            if let Some(marks) = marks {
                marks.relocate(|offset| match &runtime {
                    Some(factory) if factory.start < offset && offset < factory.end => {
                        let at = written.partition_point(|(old, _)| *old < factory.start);
                        Some(written[at].1 + offset - factory.start)
                    }
                    _ => written
                        .get(written.partition_point(|(old, _)| *old < offset))
                        .map(|(_, new)| *new),
                });
            }
            output
        }
        None => {
            debug_assert!(false, "emitted JavaScript does not lex:\n{source}");
            source.to_owned()
        }
    }
}

/// Where text at `offset` of a source is in its renamed text, given the
/// `anchors` of [`rename_compiler_names`].
pub(crate) fn renamed_offset(anchors: &[(usize, usize)], offset: usize) -> usize {
    match anchors.partition_point(|(old, _)| *old <= offset) {
        0 => offset,
        at => {
            let (old, new) = anchors[at - 1];
            new + (offset - old)
        }
    }
}

/// The compiler names of `source` outside the `skipped` ranges, with their
/// ranges: identifiers outside double-quoted strings, the only literals the
/// emitter writes with free text.
pub(crate) fn scanned_names<'s>(
    source: &'s str,
    skipped: &[Range<usize>],
) -> Vec<(Range<usize>, &'s str)> {
    let mut skipped = skipped
        .iter()
        .filter(|range| !range.is_empty())
        .cloned()
        .collect::<Vec<_>>();
    skipped.sort_by_key(|range| range.start);
    let bytes = source.as_bytes();
    let mut names = Vec::new();
    let mut skipped = skipped.iter().peekable();
    let mut at = 0;
    while at < bytes.len() {
        if let Some(range) = skipped.next_if(|range| range.start <= at) {
            at = at.max(range.end);
            continue;
        }
        let limit = skipped.peek().map_or(bytes.len(), |range| range.start);
        let start = at;
        if bytes[at] == b'"' {
            at += 1;
            while at < limit {
                let byte = bytes[at];
                at += 1;
                if byte == b'\\' {
                    at += 1;
                } else if byte == b'"' {
                    break;
                }
            }
        } else if bytes[at].is_ascii_alphabetic() || matches!(bytes[at], b'_' | b'$') {
            at += 1;
            while at < limit
                && (bytes[at].is_ascii_alphanumeric() || matches!(bytes[at], b'_' | b'$'))
            {
                at += 1;
            }
            let word = &source[start..at];
            if compiler_name(word) {
                names.push((start..at, word));
            }
        } else {
            at += 1;
        }
    }
    names
}

/// A program with raw `js*` text: compiler names become the hygiene prefix
/// and a short name, and nothing else changes.
fn hygienic(
    source: &str,
    raw: &[Range<usize>],
    runtime: Option<Range<usize>>,
) -> (String, Vec<(usize, usize)>) {
    let prefix = hygiene_prefix(source, raw);
    let mut skipped = raw.to_vec();
    skipped.extend(runtime);
    let found = scanned_names(source, &skipped);
    let names = short_names(found.iter().map(|(_, word)| *word), |_| false);
    rename_compiler_names(source, &skipped, |word| format!("{prefix}{}", names[word]))
}

/// `source` with every compiler name ([`compiler_name`]) outside the
/// `skipped` ranges (raw `js*` text, the runtime) written as `rename` gives
/// it. The readable output's hygiene pass is `rename` adding the
/// [`hygiene_prefix`]; terse output with raw text adds it to a short name.
/// Also returns the anchors of [`renamed_offset`]: the start and end of
/// each renamed name in `source`, each with its offset in the output.
pub(crate) fn rename_compiler_names(
    source: &str,
    skipped: &[Range<usize>],
    rename: impl Fn(&str) -> String,
) -> (String, Vec<(usize, usize)>) {
    let mut output = String::with_capacity(source.len());
    let mut anchors = Vec::new();
    let mut at = 0;
    for (range, word) in scanned_names(source, skipped) {
        output.push_str(&source[at..range.start]);
        anchors.push((range.start, output.len()));
        output.push_str(&rename(word));
        anchors.push((range.end, output.len()));
        at = range.end;
    }
    output.push_str(&source[at..]);
    (output, anchors)
}

fn punct<'s>(source: &'s str, token: &Token) -> &'s str {
    if token.kind == Kind::Punct {
        token.text(source)
    } else {
        ""
    }
}

/// Punctuators after which a `(` opens a parenthesized expression rather than
/// arguments, parameters or a statement head.
const BEFORE_EXPRESSION: &[&str] = &[
    "(", "[", "{", ",", ";", "=", "+=", "-=", "*=", "/=", "%=", "**=", "<<=", ">>=", ">>>=", "&=",
    "|=", "^=", "&&=", "||=", "??=", "?", ":", "=>", "!", "~", "+", "-", "*", "/", "%", "**", "<",
    ">", "<=", ">=", "==", "!=", "===", "!==", "&&", "||", "??", "&", "|", "^", "<<", ">>", ">>>",
    "...",
];

/// The program rewritten from its tokens, or `None` when it does not lex,
/// with the start of each written token in `source` and in the output.
fn rewritten(source: &str, runtime: Option<Range<usize>>) -> Option<(String, Vec<(usize, usize)>)> {
    // The runtime factory is one token, an atom like a template literal.
    let mut tokens = Vec::new();
    match &runtime {
        Some(factory) => {
            tokens.extend(lex_section(source, 0..factory.start, false).ok()?);
            tokens.push(Token {
                kind: Kind::Template,
                start: factory.start,
                end: factory.end,
                newline_before: false,
            });
            tokens.extend(lex_section(source, factory.end..source.len(), false).ok()?);
        }
        None => tokens.extend(lex_section(source, 0..source.len(), false).ok()?),
    }
    let is_runtime = |token: &Token| runtime.as_ref().is_some_and(|r| r.start == token.start);

    let mut words = Vec::new();
    let mut excluded = HashSet::new();
    for token in &tokens {
        if token.kind == Kind::Name {
            let word = token.text(source);
            if compiler_name(word) {
                words.push(word);
            } else {
                excluded.insert(word);
            }
        }
    }
    let names = short_names(words, |name| excluded.contains(name));

    let partners = partners(source, &tokens);
    let dropped = redundant_semicolons(source, &tokens, &partners);
    let mut rewrite = Rewrite {
        source,
        tokens: &tokens,
        cleared: vec![false; tokens.len()],
        dotted: vec![None; tokens.len()],
        partners,
        dropped,
    };
    rewrite.run();
    let Rewrite {
        dropped,
        cleared,
        dotted,
        ..
    } = rewrite;
    // Adjacent `let` and `const` statements join: `let a,b;`.
    let joins = match resolve(source, &tokens) {
        Ok(resolution) => joined_declarations(source, &tokens, &resolution.statements),
        Err(_) => vec![Join::Keep; tokens.len()],
    };

    let mut writer = TerseWriter::default();
    let mut written = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if dropped[index] || joins[index] == Join::Drop {
            continue;
        }
        // The writer separates a token by at most one space or line break.
        let before = writer.out.len();
        let separated = |out: &str| {
            before + usize::from(matches!(out.as_bytes().get(before), Some(b' ' | b'\n')))
        };
        let newline = token.newline_before && !cleared[index];
        if joins[index] == Join::Comma {
            let comma = Shape {
                kind: Kind::Punct,
                text: ",",
            };
            writer.write(comma, ",", newline);
            written.push((token.start, separated(&writer.out)));
            continue;
        }
        if let Some(name) = dotted[index] {
            writer.write(
                Shape {
                    kind: Kind::Punct,
                    text: ".",
                },
                ".",
                newline,
            );
            writer.write(
                Shape {
                    kind: Kind::Name,
                    text: name,
                },
                name,
                false,
            );
            written.push((token.start, separated(&writer.out)));
            continue;
        }
        let word = token.text(source);
        if is_runtime(token) {
            let open = Shape {
                kind: Kind::Punct,
                text: "(",
            };
            writer.write(open, &word[..word.len() - 1], newline);
            let close = Shape {
                kind: Kind::Punct,
                text: ")",
            };
            writer.write(close, ")", false);
        } else if token.kind == Kind::Name && compiler_name(word) {
            let shape = Shape {
                kind: Kind::Name,
                text: word,
            };
            writer.write(shape, &names[word], newline);
        } else {
            let shape = Shape {
                kind: token.kind,
                text: word,
            };
            writer.write(shape, word, newline);
        }
        written.push((token.start, separated(&writer.out)));
    }
    let mut output = writer.out;
    output.push('\n');
    Some((output, written))
}

/// Keywords after which a `(` opens a parenthesized expression.
const KEYWORDS_BEFORE_EXPRESSION: &[&str] = &[
    "return",
    "typeof",
    "void",
    "delete",
    "throw",
    "case",
    "in",
    "instanceof",
    "else",
    "do",
];

/// The token-level rewrites of a program: dotted members, dropped
/// parentheses and semicolons. Each parenthesis rule is applied to the
/// program as the rules before it left it (innermost and later groups
/// first), and each keeps the program's meaning.
struct Rewrite<'s, 't> {
    source: &'s str,
    tokens: &'t [Token],
    partners: Vec<usize>,
    /// Tokens not written.
    dropped: Vec<bool>,
    /// Tokens whose preceding line break is dropped: tokens of a group whose
    /// parentheses go, where no line break could end a statement.
    cleared: Vec<bool>,
    /// At a `[` of `["name"]`: the name, written `.name`.
    dotted: Vec<Option<&'s str>>,
}

impl<'s> Rewrite<'s, '_> {
    fn text(&self, index: usize) -> &'s str {
        self.tokens[index].text(self.source)
    }

    fn punct(&self, index: usize) -> &'s str {
        punct(self.source, &self.tokens[index])
    }

    /// The last token before `index` that is written.
    fn previous(&self, index: usize) -> Option<usize> {
        (0..index).rev().find(|&at| !self.dropped[at])
    }

    /// The first token after `index` that is written.
    fn next(&self, index: usize) -> Option<usize> {
        (index + 1..self.tokens.len()).find(|&at| !self.dropped[at])
    }

    /// The written tokens strictly between `open` and `close`, with written
    /// brackets nested in them skipped to their partners: the tokens of the
    /// expression's top level.
    fn top_level(&self, open: usize, close: usize) -> impl Iterator<Item = usize> + '_ {
        let mut index = open + 1;
        std::iter::from_fn(move || {
            while index < close && self.dropped[index] {
                index += 1;
            }
            if index >= close {
                return None;
            }
            let at = index;
            index = if self.partners[at] > at && self.partners[at] < close {
                self.partners[at] + 1
            } else {
                at + 1
            };
            Some(at)
        })
    }

    /// A keyword at `index` that is not a property name (`x.delete(k)`).
    fn keyword_at(&self, index: usize, words: &[&str]) -> bool {
        self.tokens[index].kind == Kind::Name
            && words.contains(&self.text(index))
            && !self
                .previous(index)
                .is_some_and(|previous| matches!(self.punct(previous), "." | "?."))
    }

    /// Whether the token at `index` ends an operand that a member access may
    /// follow.
    fn operand(&self, index: usize) -> bool {
        if self.dotted[index].is_some() {
            return true;
        }
        let token = &self.tokens[index];
        match token.kind {
            Kind::Name => {
                let word = self.text(index);
                (!reserved(word) || word == "this" || word == "super")
                    && !matches!(word, "of" | "async" | "get" | "set" | "static")
            }
            Kind::Str | Kind::Template | Kind::TemplateTail | Kind::Regex => true,
            Kind::Punct => match self.text(index) {
                "]" => true,
                ")" => !closes_control_head(self.source, self.tokens, &self.partners, index),
                _ => false,
            },
            _ => false,
        }
    }

    /// Whether the `(` at `open` opens a parenthesized expression, rather
    /// than arguments, parameters or a statement head.
    fn group(&self, open: usize) -> bool {
        match self.previous(open) {
            None => true,
            Some(previous) if self.dotted[previous].is_some() => false,
            Some(previous) if self.tokens[previous].kind == Kind::Punct => {
                BEFORE_EXPRESSION.contains(&self.text(previous))
            }
            Some(previous) => {
                !self.tokens[open].newline_before
                    && self.keyword_at(previous, KEYWORDS_BEFORE_EXPRESSION)
            }
        }
    }

    /// Whether the top level of the group holds `word` (a punctuator or a
    /// keyword).
    fn holds(&self, open: usize, close: usize, word: &str) -> bool {
        self.top_level(open, close).any(|index| {
            self.dotted[index].is_none()
                && matches!(self.tokens[index].kind, Kind::Punct | Kind::Name)
                && self.text(index) == word
        })
    }

    /// Whether the group holds a member chain: a name, `this`, a string or a
    /// number, then only `.name`, calls and computed members. Its parentheses
    /// never change how it parses, except around a lone string at the start
    /// of a statement, which would become a directive.
    fn simple(&self, open: usize, close: usize) -> bool {
        let mut items = self.top_level(open, close);
        let Some(first) = items.next() else {
            return false;
        };
        let token = &self.tokens[first];
        let starts = match token.kind {
            Kind::Name => {
                let word = self.text(first);
                (!reserved(word) || word == "this")
                    && !matches!(word, "async" | "let" | "yield" | "await" | "of")
            }
            Kind::Str => {
                self.next(first) != Some(close)
                    || !matches!(
                        self.previous(open).map(|previous| self.punct(previous)),
                        None | Some(";" | "{")
                    )
            }
            Kind::Number => true,
            _ => false,
        };
        if !starts {
            return false;
        }
        let mut member = false;
        for index in items {
            if member {
                if self.tokens[index].kind != Kind::Name {
                    return false;
                }
                member = false;
                continue;
            }
            match self.punct(index) {
                "." => member = true,
                "(" | "[" if self.partners[index] < close => {}
                _ => return false,
            }
        }
        !member
    }

    fn unwrap(&mut self, open: usize, close: usize) {
        self.dropped[open] = true;
        self.dropped[close] = true;
        let inside = self.top_level(open, close).collect::<Vec<_>>();
        for index in inside {
            self.cleared[index] = true;
        }
    }

    fn run(&mut self) {
        for index in 0..self.tokens.len() {
            if self.punct(index) != "[" || self.partners[index] != index + 2 {
                continue;
            }
            let key = &self.tokens[index + 1];
            if key.kind != Kind::Str || index == 0 || !self.operand(index - 1) {
                continue;
            }
            let quoted = key.text(self.source);
            let name = &quoted[1..quoted.len() - 1];
            let identifier = name
                .bytes()
                .next()
                .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_' || b == b'$')
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$');
            if identifier && !compiler_name(name) {
                self.dotted[index] = Some(name);
                self.dropped[index + 1] = true;
                self.dropped[index + 2] = true;
            }
        }
        for open in (0..self.tokens.len()).rev() {
            let close = self.partners[open];
            if self.punct(open) != "(" || close == usize::MAX || self.next(open) == Some(close) {
                continue;
            }
            // `((x))`: the inner pair, when its top level holds no comma.
            if let Some(inner) = self.next(open)
                && self.punct(inner) == "("
                && Some(self.partners[inner]) == self.previous(close)
                && self.next(inner) != Some(self.partners[inner])
                && !self.holds(inner, self.partners[inner], ",")
            {
                self.unwrap(inner, self.partners[inner]);
            }
            if !self.group(open) {
                continue;
            }
            let before = self.previous(open).map(|previous| self.text(previous));
            let after = self.next(close).map(|next| self.punct(next));
            let unwrap = self.simple(open, close)
                // `x = (y);`: an assignment's right-hand side.
                || (before == Some("=")
                    && matches!(after, None | Some(";" | "," | ")" | "]" | "}"))
                    && !self.holds(open, close, ",")
                    && !self.holds(open, close, "in"))
                // `return (x);`
                || (self
                    .previous(open)
                    .is_some_and(|previous| self.keyword_at(previous, &["return", "throw"]))
                    && matches!(after, None | Some(";" | "}")));
            if unwrap {
                self.unwrap(open, close);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::js_syntax::{Occurrence, Resolution, lex, resolve};
    use crate::runtime_manifest::RuntimeManifest;

    /// What an identifier occurrence names, comparable across two texts.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    enum Named<'s> {
        Declared(usize),
        Free(&'s str),
    }

    /// Checks that `renamed` is `original` with names changed and nothing
    /// else: the same tokens, except dropped redundant semicolons and
    /// expanded shorthand properties (`{a}` as `{a: b}`), and every identifier
    /// naming the same binding. Declarations correspond one to one; a free
    /// name that is a top-level runtime binding (in a slice that leaves out
    /// its unit) may be renamed, one to one, and every other free name, a
    /// global, keeps its spelling. So no renamed identifier is captured by
    /// another declaration, and none escapes its own.
    fn lexed(text: &str) -> Result<(Vec<Token>, Resolution<'_>), String> {
        let tokens = lex(text, 0..text.len())
            .map_err(|error| format!("does not lex at {}: {}", error.offset, error.message))?;
        let resolution = resolve(text, &tokens)
            .map_err(|(offset, message)| format!("does not parse at {offset}: {message}"))?;
        Ok((tokens, resolution))
    }

    fn named(occurrence: Occurrence, text: &str) -> Named<'_> {
        match occurrence.declaration {
            Some(declaration) => Named::Declared(declaration),
            None => Named::Free(text),
        }
    }

    fn same_bindings(original: &str, renamed: &str) -> Result<(), String> {
        let (left, left_resolution) = lexed(original)?;
        let (right, right_resolution) = lexed(renamed)?;
        let redundant = redundant_semicolons(original, &left, &partners(original, &left));
        let mut forward: HashMap<Named, Named> = HashMap::new();
        let mut backward: HashMap<Named, Named> = HashMap::new();
        let mut at = 0;
        let context = |index: usize, text: &str| {
            let start = left[index].start;
            format!(
                "at original byte {start} (`{}`):",
                &text[start.saturating_sub(60)..(start + 60).min(text.len())]
            )
        };
        let mut joined = false;
        for (index, token) in left.iter().enumerate() {
            if redundant[index] || std::mem::take(&mut joined) {
                continue;
            }
            let text = token.text(original);
            let other = right.get(at).map(|t| t.text(renamed));
            // `;const` joined to `,` (`joined_declarations`).
            if text == ";"
                && other == Some(",")
                && left
                    .get(index + 1)
                    .is_some_and(|next| matches!(next.text(original), "let" | "const"))
            {
                joined = true;
                at += 1;
                continue;
            }
            let Some(occurrence) = left_resolution.occurrences[index] else {
                if other != Some(text) || right[at].kind != token.kind {
                    return Err(format!(
                        "{} expected `{text}`, found {other:?}",
                        context(index, original)
                    ));
                }
                at += 1;
                continue;
            };
            if occurrence.shorthand
                && right.get(at).map(|t| t.text(renamed)) == Some(text)
                && right.get(at + 1).map(|t| t.text(renamed)) == Some(":")
            {
                at += 2;
            }
            let Some(other) = right_resolution.occurrences.get(at).copied().flatten() else {
                return Err(format!(
                    "{} `{text}` is not an identifier occurrence in the renamed text",
                    context(index, original)
                ));
            };
            let other_text = right[at].text(renamed);
            let (from, to) = (named(occurrence, text), named(other, other_text));
            match (from, to) {
                (Named::Free(a), Named::Free(b))
                    if a != b && TERSE_BINDINGS.binary_search(&a).is_err() =>
                {
                    return Err(format!(
                        "{} global `{a}` became `{b}`",
                        context(index, original)
                    ));
                }
                _ => {}
            }
            if *forward.entry(from).or_insert(to) != to
                || *backward.entry(to).or_insert(from) != from
            {
                return Err(format!(
                    "{} `{text}` ({from:?}) is `{other_text}` ({to:?}), which names another binding",
                    context(index, original)
                ));
            }
            at += 1;
        }
        if at != right.len() {
            return Err(format!(
                "the renamed text has {} more tokens",
                right.len() - at
            ));
        }
        Ok(())
    }

    #[test]
    fn the_checker_finds_captures_and_escapes() {
        assert_eq!(
            same_bindings(
                "function f(a, b) { return a + b; }",
                "function f(c,d){return c+d}"
            ),
            Ok(())
        );
        assert_eq!(
            same_bindings("const a = 1, o = {a, b: 1};", "const x=1,o={a:x,b:1};"),
            Ok(())
        );
        // A capture: both parameters named `c`.
        assert!(
            same_bindings(
                "function f(a) { return (b) => a + b; }",
                "function f(c){return c=>c+c}"
            )
            .is_err()
        );
        // An escape: a reference left with its old name.
        assert!(same_bindings("function f(a) { return a; }", "function f(b){return a}").is_err());
        // A global renamed.
        assert!(same_bindings("const x = Math.max;", "const x=y.max").is_err());
    }

    #[test]
    fn the_whole_terse_runtime_renames_without_capture() {
        let manifest = RuntimeManifest::embedded();
        // `build.rs` writes one terse unit per manifest unit, and the
        // bindings are the units' declarations.
        assert_eq!(TERSE_UNITS.len(), manifest.units.len());
        let declared = manifest
            .units
            .iter()
            .flat_map(|unit| unit.declares.iter().copied())
            .collect::<std::collections::BTreeSet<_>>();
        assert!(TERSE_BINDINGS.iter().copied().eq(declared));
        let kept = vec![true; manifest.units.len()];
        let returned = ["keyword", "vectorFromArray", "str"];
        let original = manifest.sliced_bindings_text(&kept, &returned);
        let (renamed, bytes) = terse_slice(&kept, &returned);
        same_bindings(&original, &renamed).unwrap();
        assert!(bytes.iter().sum::<usize>() < renamed.len());
        // About 53% of the readable runtime, which keeps its comments.
        assert!(
            renamed.len() * 100 < original.len() * 55,
            "{} of {}",
            renamed.len(),
            original.len()
        );
    }

    #[test]
    fn partial_slices_rename_without_capture() {
        let manifest = RuntimeManifest::embedded();
        // Units kept by a small linear congruential sequence, at several
        // densities; a slice need not be closed under dependencies for its
        // names to be checked.
        let mut state = 0x2545_f491_u64;
        for density in [2, 5, 10, 30, 60, 90] {
            let kept = (0..manifest.units.len())
                .map(|_| {
                    state = state
                        .wrapping_mul(6_364_136_223_846_793_005)
                        .wrapping_add(1);
                    (state >> 33) % 100 < density
                })
                .collect::<Vec<_>>();
            let returned = manifest
                .units
                .iter()
                .zip(&kept)
                .filter(|(_, kept)| **kept)
                .flat_map(|(unit, _)| unit.declares.first().copied())
                .take(3)
                .collect::<Vec<_>>();
            let original = manifest.sliced_bindings_text(&kept, &returned);
            let (renamed, _) = terse_slice(&kept, &returned);
            same_bindings(&original, &renamed)
                .unwrap_or_else(|error| panic!("density {density}: {error}"));
        }
    }

    /// The marks of a terse slice ([`terse_slice_marks`]) point at the text
    /// of the runtime tokens they map: the same punctuator, keyword or
    /// literal, or a name, which may be renamed, and for a `let` or `const`
    /// that declarations joining dropped, the `,` or the declarator after it.
    #[test]
    fn slice_marks_point_at_the_tokens_they_map() {
        let manifest = RuntimeManifest::embedded();
        let source = crate::runtime_manifest::ANNOTATED_RUNTIME;
        let tokens = lex(source, 0..source.len()).unwrap();
        for kept in [
            vec![true; manifest.units.len()],
            (0..manifest.units.len())
                .map(|unit| unit % 3 == 0)
                .collect(),
        ] {
            let returned = ["keyword"];
            let (text, marks) = terse_slice_marks(&kept, &returned);
            assert_eq!(text, terse_slice(&kept, &returned).0);
            let lines = manifest
                .units
                .iter()
                .zip(&kept)
                .filter(|(_, kept)| **kept)
                .map(|(unit, _)| {
                    let body = &source[unit.body_range.clone()];
                    body.lines()
                        .filter(|line| {
                            let line = line.trim_start();
                            !line.is_empty() && !line.starts_with("//")
                        })
                        .count()
                })
                .sum::<usize>();
            // Lines of comments in the middle of statements have no mark.
            assert!(marks.len() * 100 > lines * 95, "{} of {lines}", marks.len());
            let named = marks.iter().filter(|(_, _, name)| name.is_some()).count();
            let declaring = manifest
                .units
                .iter()
                .zip(&kept)
                .filter(|(unit, kept)| **kept && !unit.declares.is_empty())
                .count();
            assert_eq!(named, declaring);
            for &(offset, at, _) in &marks[1..] {
                let token = &tokens[tokens.partition_point(|token| token.start < at)];
                assert_eq!(token.start, at);
                let word = token.text(source);
                let written = &text[offset..];
                let name = written
                    .bytes()
                    .next()
                    .is_some_and(crate::js_syntax::identifier_part);
                let matches = match token.kind {
                    Kind::Name if matches!(word, "let" | "const") => {
                        written.starts_with(word) || written.starts_with([',', '[', '{']) || name
                    }
                    Kind::Name if !reserved(word) => name,
                    _ => written.starts_with(word),
                };
                assert!(matches, "`{word}` at {at} maps to {:?}", &written[..20]);
            }
        }
    }
}
