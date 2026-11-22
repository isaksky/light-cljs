//! Terse emission (docs/advanced-optimization.md, Phase 3): what `Advanced`
//! output writes instead of the readable text of the passes before it.
//!
//! - The runtime slice embeds the terse units `build.rs` writes from
//!   `src/js_runtime.js`: no comments or whitespace, locals renamed to short
//!   names, and each top-level binding a placeholder that [`terse_slice`]
//!   names by its frequency in the slice.
//! - The program is renamed and rewritten by [`terse_program`], which
//!   replaces the hygiene pass: every compiler-bound name gets a short name,
//!   a local per scope ([`Locals`], light-cljs-419) and every other one by
//!   frequency, members read with a constant identifier key become dotted,
//!   redundant parentheses go and tokens are joined without whitespace.
//! - A chunked build's texts share one such table ([`terse_texts`]), which
//!   also names each chunk's runtime units written at module level
//!   ([`terse_module_units`], light-cljs-303).
//!
//! [`crate::ApplicationOptions::pseudo_names`] keeps the readable output.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use crate::js_syntax::{
    Declaration, FirstToken, Join, Kind, NO_PARTNER, NO_SCOPE, NameGenerator, Resolution, Shape,
    TerseWriter, Token, WriterEnd, closes_control_head, joined_declarations, lex_section, partners,
    redundant_semicolons, reserved, resolve,
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
    marks: Option<&mut Vec<SliceMark>>,
) -> (String, Vec<usize>) {
    let names = slice_names(kept, returned);
    write_factory(&names, kept, None, returned, marks)
}

/// The short names of the runtime's top-level bindings in terse slices
/// ([`slice_names`]).
pub(crate) struct SliceNames {
    /// By index into [`TERSE_BINDINGS`]: the binding's index in `generated`,
    /// or `None` for one that keeps its name or that no slice holds.
    names: Vec<Option<usize>>,
    generated: Vec<String>,
}

impl SliceNames {
    fn name(&self, index: usize) -> &str {
        match self.names[index] {
            Some(name) => self.generated[name].as_str(),
            None => TERSE_BINDINGS[index],
        }
    }
}

/// One naming of the top-level bindings of the `kept` units (by unit
/// index), counting their occurrences in the units and in `mentioned`, the
/// bindings the slices return or take as parameters, once per mention.
/// Each binding gets a short name: the most frequent first, each the first
/// name of the [`NameGenerator`] sequence that no other binding has and no
/// local visible at one of its occurrences has. A chunked build names the
/// units of all its chunks at once (docs/code-splitting.md, "Phase 3"), so
/// a binding a chunk takes as a parameter has the name it has in the chunk
/// declaring it, and no other binding a chunk names has that name.
pub(crate) fn slice_names(kept: &[bool], mentioned: &[&str]) -> SliceNames {
    debug_assert_eq!(kept.len(), TERSE_UNITS.len());
    let units = TERSE_UNITS
        .iter()
        .zip(kept)
        .filter_map(|(unit, kept)| kept.then_some(unit))
        .collect::<Vec<_>>();
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
    for unit in &units {
        for (_, index) in placeholders(unit.text) {
            occur(index);
        }
    }
    for name in mentioned {
        occur(binding_index(name));
    }
    let mut avoid: HashMap<usize, HashSet<&str>> = HashMap::new();
    for unit in &units {
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
    SliceNames { names, generated }
}

/// The terse runtime of one chunk of a chunked build: the `kept` units as a
/// function of `parameters`, the bindings they require that other chunks
/// declare, which returns `returned` as an array, `((a,b)=>{...return[c]})`
/// (no `"use strict"`: a chunk is a module), with the bindings named by
/// `names`; and the bytes of each unit in it, by unit index.
pub(crate) fn terse_chunk_factory(
    names: &SliceNames,
    kept: &[bool],
    parameters: &[&str],
    returned: &[&str],
) -> (String, Vec<usize>) {
    write_factory(names, kept, Some(parameters), returned, None)
}

/// The startup chunk's terse runtime, as [`terse_slice`] writes a single
/// file's but with the bindings named by `names`: `(()=>{"use
/// strict";...return[a,b]})()`, and the bytes of each unit in it.
pub(crate) fn terse_startup_factory(
    names: &SliceNames,
    kept: &[bool],
    returned: &[&str],
) -> (String, Vec<usize>) {
    write_factory(names, kept, None, returned, None)
}

/// A terse runtime factory of the `kept` units named by `names`: called, as
/// a single file embeds it, or, with `parameters`, a function of them.
fn write_factory(
    names: &SliceNames,
    kept: &[bool],
    parameters: Option<&[&str]>,
    returned: &[&str],
    mut marks: Option<&mut Vec<SliceMark>>,
) -> (String, Vec<usize>) {
    let name = |index: usize| names.name(index);
    let returned = returned
        .iter()
        .map(|name| binding_index(name))
        .collect::<Vec<_>>();
    let mut text = match parameters {
        Some(parameters) => {
            let parameters = parameters
                .iter()
                .map(|parameter| name(binding_index(parameter)))
                .collect::<Vec<_>>();
            format!("(({})=>{{", parameters.join(","))
        }
        None => String::from(TERSE_PROLOGUE),
    };
    if let Some(marks) = marks.as_deref_mut() {
        let manifest = crate::runtime_manifest::RuntimeManifest::embedded();
        let prologue = &crate::runtime_manifest::ANNOTATED_RUNTIME[manifest.prologue.clone()];
        marks.push((0, prologue.find("(() =>").unwrap_or_default(), None));
    }
    let unit_bytes = write_units(&mut text, kept, name, marks);
    if parameters.is_none() || !returned.is_empty() {
        text.push_str("return[");
        for (position, &index) in returned.iter().enumerate() {
            if position > 0 {
                text.push(',');
            }
            text.push_str(name(index));
        }
        text.push(']');
    }
    text.push_str(if parameters.is_some() { "})" } else { "})()" });
    (text, unit_bytes)
}

/// `rt$<binding>` for the top-level runtime binding at `index` in
/// [`TERSE_BINDINGS`]: the compiler name a program reads it by, which names
/// the binding itself where units are written at module level
/// ([`terse_module_units`]).
fn runtime_read(index: usize) -> &'static str {
    static READS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    READS.get_or_init(|| {
        TERSE_BINDINGS
            .iter()
            .map(|binding| format!("rt${binding}"))
            .collect()
    })[index]
        .as_str()
}

/// The compiler names that the `kept` units (by unit index) name their
/// top-level bindings by at module level, one per occurrence, in order:
/// `rt$<binding>` ([`runtime_read`]).
fn unit_words(kept: &[bool]) -> impl Iterator<Item = &'static str> + '_ {
    TERSE_UNITS
        .iter()
        .zip(kept)
        .filter(|(_, kept)| **kept)
        .flat_map(|(unit, _)| placeholders(unit.text).map(|(_, index)| runtime_read(index)))
}

/// The terse runtime of one chunk of a chunked build written at module
/// level (docs/code-splitting.md, "What a chunk file contains";
/// light-cljs-303): the `kept` units' statements, each top-level binding
/// named as `name` names its compiler name `rt$<binding>`, the name the
/// chunks' one table gives a program read of it ([`terse_texts`]); and the
/// bytes of each unit in it, by unit index.
pub(crate) fn terse_module_units<'n>(
    kept: &[bool],
    name: impl Fn(&str) -> &'n str,
) -> (String, Vec<usize>) {
    let mut text = String::new();
    let unit_bytes = write_units(&mut text, kept, |index| name(runtime_read(index)), None);
    (text, unit_bytes)
}

/// Append the `kept` units to `text`, each top-level binding by index named
/// by `name`, and return the bytes of each unit, by unit index.
fn write_units<'n>(
    text: &mut String,
    kept: &[bool],
    name: impl Fn(usize) -> &'n str,
    mut marks: Option<&mut Vec<SliceMark>>,
) -> Vec<usize> {
    debug_assert_eq!(kept.len(), TERSE_UNITS.len());
    let units = TERSE_UNITS
        .iter()
        .enumerate()
        .zip(kept)
        .filter_map(|(unit, kept)| kept.then_some(unit))
        .collect::<Vec<_>>();
    let mut unit_bytes = vec![0; TERSE_UNITS.len()];
    let manifest = crate::runtime_manifest::RuntimeManifest::embedded();
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
            copy(text, at, range.start);
            text.push_str(&unit.text[at..range.start]);
            copy(text, range.start, range.end);
            text.push_str(name(index));
            at = range.end;
        }
        copy(text, at, usize::MAX);
        text.push_str(&unit.text[at..]);
        unit_bytes[id] = text.len() - start;
        closes = unit.closes;
    }
    unit_bytes
}

// ---------------------------------------------------------------------------
// The program

/// Whether `word` is a name the emitter binds in a program's output: the
/// numbered families (`l7`, `g3_1`, `k0`, `c2`, `m1`, `a0`, `r5_0`, `next0`,
/// `saved1`, `temporary1`, `captures4`, `self4`, `recur5`, `installed1`,
/// `operand0`), the fixed helper names, and the `Advanced` runtime reads and
/// core cells (`rt$add`, `core$inc`), and the setters a chunk of a chunked
/// build exports for its globals that another chunk assigns (`g7$`, and
/// `core$inc$` for a core cell), and its helper `lazy$` that gives a
/// loadable of another chunk its `load`. Raw `js*` text must see none of them,
/// and terse output renames all of them, wherever they occur: a capture
/// object's keys (`captures4.l7`) are such names too. A constant member key
/// that is one of them stays bracketed (`x["f"]`).
#[inline]
pub(crate) fn compiler_name(word: &str) -> bool {
    let bytes = word.as_bytes();
    let Some(&first) = bytes.first() else {
        return false;
    };
    match first {
        b'l' => numbered_tail(&bytes[1..]) || word == "lazy$",
        b'g' => {
            numbered_tail(&bytes[1..])
                || (bytes.len() >= 3
                    && bytes[bytes.len() - 1] == b'$'
                    && bytes[1..bytes.len() - 1].iter().all(u8::is_ascii_digit))
        }
        b'k' | b'm' | b'a' => numbered_tail(&bytes[1..]),
        b'c' => {
            numbered_tail(&bytes[1..])
                || word.starts_with("core$")
                || matches!(word, "caught" | "caseValue")
                || word
                    .strip_prefix("captures")
                    .is_some_and(|tail| numbered_tail(tail.as_bytes()))
        }
        b'r' => {
            numbered_tail(&bytes[1..])
                || word == "rt"
                || word.starts_with("rt$")
                || word
                    .strip_prefix("recur")
                    .is_some_and(|tail| numbered_tail(tail.as_bytes()))
                || word
                    .strip_prefix("receiver")
                    .is_some_and(|tail| numbered_tail(tail.as_bytes()))
        }
        b'o' => word
            .strip_prefix("operand")
            .is_some_and(|tail| numbered_tail(tail.as_bytes())),
        b't' => {
            matches!(word, "truth" | "text")
                || word
                    .strip_prefix("temporary")
                    .is_some_and(|tail| numbered_tail(tail.as_bytes()))
        }
        b's' => {
            matches!(word, "savedPrint" | "savedNewline")
                || word
                    .strip_prefix("saved")
                    .is_some_and(|tail| numbered_tail(tail.as_bytes()))
                || word
                    .strip_prefix("self")
                    .is_some_and(|tail| numbered_tail(tail.as_bytes()))
        }
        b'n' => {
            word.starts_with("nativeCore$")
                || word.starts_with("nativeCoreGetter$")
                || word
                    .strip_prefix("next")
                    .is_some_and(|tail| numbered_tail(tail.as_bytes()))
        }
        b'i' => word
            .strip_prefix("installed")
            .is_some_and(|tail| numbered_tail(tail.as_bytes())),
        b'f' => word == "f",
        b'h' => word == "host",
        b'p' => word == "pkg",
        b'_' => {
            word == "__lightSessionRuntime"
                || word
                    .strip_prefix("__lightUnitReify")
                    .is_some_and(|tail| numbered_tail(tail.as_bytes()))
        }
        _ => false,
    }
}

#[inline]
fn numbered_tail(tail: &[u8]) -> bool {
    !tail.is_empty() && tail.iter().all(|&b| b.is_ascii_digit() || b == b'_')
}

/// A prefix that no raw `js*` segment of `source` contains: `__light<n>_`.
pub(crate) fn hygiene_prefix(source: &str, raw: &[Range<usize>]) -> String {
    raw_hygiene_prefix(raw.iter().map(|range| &source[range.clone()]))
}

/// A prefix that none of the `raw` texts contains: `__light<n>_`.
pub(crate) fn raw_hygiene_prefix<'r>(raw: impl Iterator<Item = &'r str> + Clone) -> String {
    let mut index = 0;
    loop {
        let candidate = format!("__light{index}_");
        if !raw.clone().any(|text| text.contains(&candidate)) {
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
    constrained_short_names(words, excluded, &HashMap::new(), |_| None)
}

/// [`short_names`] where some words have constraints: a word of `avoid`
/// takes the first name of the sequence that no more frequent word has and
/// that is none of its avoided names (a unit's locals visible where it reads
/// the word's runtime binding), and a word that `fixed` names keeps that
/// name (a runtime binding that keeps its spelling). The same-length
/// batches reorder only the names of unconstrained words, so without
/// constraints the names are [`short_names`]'s.
fn constrained_short_names<'s>(
    words: impl IntoIterator<Item = &'s str>,
    excluded: impl Fn(&str) -> bool,
    avoid: &HashMap<&str, HashSet<&str>>,
    fixed: impl Fn(&str) -> Option<&'static str>,
) -> HashMap<&'s str, String> {
    let mut stats: HashMap<&str, (usize, usize)> = HashMap::new();
    for (position, word) in words.into_iter().enumerate() {
        stats.entry(word).or_insert((0, position)).0 += 1;
    }
    constrained_short_names_from_stats(stats, excluded, avoid, fixed)
}

/// [`constrained_short_names`] from pre-aggregated `(count, first_position)`
/// word statistics, allowing parallel workers to count each slice or chunk
/// independently before merging.
fn constrained_short_names_from_stats<'s, P: Ord + Copy>(
    stats: HashMap<&'s str, (usize, P)>,
    excluded: impl Fn(&str) -> bool,
    avoid: &HashMap<&str, HashSet<&str>>,
    fixed: impl Fn(&str) -> Option<&'static str>,
) -> HashMap<&'s str, String> {
    let mut names = HashMap::with_capacity(stats.len());
    let mut order = Vec::with_capacity(stats.len());
    for &word in stats.keys() {
        match fixed(word) {
            Some(name) => {
                names.insert(word, name.to_owned());
            }
            None => order.push(word),
        }
    }
    order.sort_by_key(|word| (std::cmp::Reverse(stats[word].0), stats[word].1));
    let mut candidates = NameGenerator::default().filter(|name| !excluded(name));
    let mut generated: Vec<String> = Vec::new();
    let mut taken: Vec<bool> = Vec::new();
    let mut free = 0;
    let mut chosen = Vec::with_capacity(order.len());
    for word in &order {
        let avoided = avoid.get(word);
        while taken.get(free) == Some(&true) {
            free += 1;
        }
        let mut at = free;
        loop {
            if at == generated.len() {
                generated.push(candidates.next().expect("names never run out"));
                taken.push(false);
            }
            if !taken[at]
                && !avoided.is_some_and(|avoided| avoided.contains(generated[at].as_str()))
            {
                break;
            }
            at += 1;
        }
        taken[at] = true;
        chosen.push(at);
    }
    // By name length: the words without constraints whose names have it, in
    // order.
    let mut batches: Vec<Vec<usize>> = Vec::new();
    for (position, word) in order.iter().enumerate() {
        if avoid.get(word).is_none_or(HashSet::is_empty) {
            let length = generated[chosen[position]].len();
            if batches.len() <= length {
                batches.resize_with(length + 1, Vec::new);
            }
            batches[length].push(position);
        }
    }
    for batch in batches {
        let mut slots = batch
            .iter()
            .map(|&position| chosen[position])
            .collect::<Vec<_>>();
        slots.sort_unstable();
        let mut words = batch;
        words.sort_by_key(|&position| stats[order[position]].1);
        for (position, slot) in words.into_iter().zip(slots) {
            chosen[position] = slot;
        }
    }
    for (word, at) in order.into_iter().zip(chosen) {
        names.insert(word, std::mem::take(&mut generated[at]));
    }
    names
}

/// What an occurrence in a scope names, for [`Locals::crossing`]: a local of
/// the text named per scope, by its number, or with this bit a compiler name
/// written by its own short name (a global, a runtime read, a local that
/// keeps its name), by its number in [`Locals::words`].
const WORD: u32 = 1 << 31;

/// The local of a token that names none of [`Locals`].
const NO_LOCAL: u32 = u32::MAX;

/// The locals of a text that get their short names per scope rather than
/// from the one table of compiler names (light-cljs-419), as Closure's
/// `RenameVars` and other minifiers name locals, so that sibling functions
/// reuse the same short names: a local below the top level whose every
/// occurrence is a compiler name outside raw `js*` text and not a shorthand
/// property, no operand local ([`operand_local`]), and that the caller
/// admits.
///
/// Each scope with such locals records what occurs in it or in a scope
/// inside it and resolves outside it, which its locals' names must not be:
/// the compiler names of the table (globals, runtime reads, every other
/// compiler name, wherever it is declared) and the locals of enclosing
/// scopes. Recording climbs from each occurrence to its declaration's scope
/// and stops at the first scope that has the name already, so the work is
/// the occurrences plus the recorded names.
struct Locals<'s> {
    /// Per token, the local it names, or [`NO_LOCAL`].
    of_token: Vec<u32>,
    /// Per local, its occurrences and its first token.
    count: Vec<u32>,
    first: Vec<u32>,
    /// Per scope with locals, parents first, its first local (a scope's
    /// locals are consecutive), and one more entry, the number of locals.
    starts: Vec<u32>,
    /// Per scope with locals, the names its locals avoid, each once: a
    /// local or, with [`WORD`], one of `words`.
    crossing: Vec<Vec<u32>>,
    /// The compiler names that some scope's locals avoid.
    words: Vec<&'s str>,
}

/// Whether `name` is an operand local, `operand0`, which the emitter
/// declares in every function that evaluates an operand into one, and which
/// is a name of the table rather than one per scope ([`Locals`]): so each
/// takes the same short name in every function, where its many reads
/// (`(operand0 = init, truth(operand0) ? operand0 : 3)`) compress better
/// than names that vary with what else the function binds (light-cljs-424).
fn operand_local(name: &str) -> bool {
    name.strip_prefix("operand")
        .is_some_and(|tail| !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()))
}

impl<'s> Locals<'s> {
    /// No locals named per scope: every compiler name is the table's.
    fn none(tokens: usize) -> Self {
        Self {
            of_token: vec![NO_LOCAL; tokens],
            count: Vec::new(),
            first: Vec::new(),
            starts: vec![0],
            crossing: Vec::new(),
            words: Vec::new(),
        }
    }

    fn new(
        source: &'s str,
        tokens: &[Token],
        in_raw: &[bool],
        resolution: &Resolution,
        eligible: impl Fn(&str) -> bool,
    ) -> Self {
        Self::new_canonical(source, tokens, in_raw, resolution, eligible, |word| word)
    }

    fn new_canonical(
        source: &'s str,
        tokens: &[Token],
        in_raw: &[bool],
        resolution: &Resolution,
        eligible: impl Fn(&str) -> bool,
        canonical: impl Fn(&'s str) -> &'s str,
    ) -> Self {
        let declarations = &resolution.declarations;
        let mut named = declarations
            .iter()
            .map(|declaration| {
                declaration.scope != 0
                    && declaration.token.is_some()
                    && compiler_name(declaration.name)
                    && eligible(declaration.name)
                    && !operand_local(declaration.name)
            })
            .collect::<Vec<_>>();
        for (index, occurrence) in resolution.occurrences.iter().enumerate() {
            if let Some(occurrence) = occurrence
                && let Some(declaration) = occurrence.declaration
                && (in_raw[index] || occurrence.shorthand)
            {
                named[declaration] = false;
            }
        }
        // Two places where `resolve` does not tell an occurrence's binding
        // as an engine does, which raw text that binds a hole again reaches:
        // a parameter default or pattern sees the scopes around its function
        // and not the body's declarations, and a `var` inside a `catch` whose
        // parameter has its name assigns the parameter. Every declaration of
        // such a spelling keeps one name for it, the table's, which is what
        // the analysis cannot get wrong.
        let mut spelled: HashMap<&str, Vec<usize>> = HashMap::new();
        for (index, declaration) in declarations.iter().enumerate() {
            if compiler_name(declaration.name) {
                spelled.entry(declaration.name).or_default().push(index);
            }
        }
        let mut misread = HashSet::new();
        let initialized = resolution
            .parameter_expressions
            .iter()
            .copied()
            .collect::<HashSet<_>>();
        let mut shadowed = HashSet::new();
        for declaration in declarations {
            if spelled
                .get(declaration.name)
                .is_some_and(|same| same.len() > 1)
            {
                shadowed.insert((declaration.scope, declaration.name));
            }
            if initialized.contains(&declaration.scope) && compiler_name(declaration.name) {
                misread.insert(declaration.name);
            }
        }
        for occurrence in resolution.occurrences.iter().flatten() {
            let Some(declaration) = occurrence.declaration else {
                continue;
            };
            let Declaration {
                name,
                scope: target,
                ..
            } = declarations[declaration];
            if target == occurrence.scope || !spelled.get(name).is_some_and(|same| same.len() > 1) {
                continue;
            }
            // Only a hoisted `var` resolves past a scope of its own name.
            let mut scope = occurrence.scope;
            while scope != target && scope != NO_SCOPE {
                if shadowed.contains(&(scope, name)) {
                    misread.insert(name);
                    break;
                }
                scope = resolution.parents[scope];
            }
        }
        for name in misread {
            for &declaration in &spelled[name] {
                named[declaration] = false;
            }
        }
        // `resolve` lists declarations by scope, and a parent scope
        // precedes its children.
        debug_assert!(declarations.is_sorted_by_key(|declaration| declaration.scope));
        let scopes = resolution.parents.len();
        let mut declaring = vec![NO_LOCAL; scopes];
        let mut local_of = vec![NO_LOCAL; declarations.len()];
        let mut starts = Vec::new();
        let mut locals = 0u32;
        for (index, declaration) in declarations.iter().enumerate() {
            if named[index] {
                if declaring[declaration.scope] == NO_LOCAL {
                    declaring[declaration.scope] = starts.len() as u32;
                    starts.push(locals);
                }
                local_of[index] = locals;
                locals += 1;
            }
        }
        starts.push(locals);
        // The nearest scope with locals at or around each scope.
        let mut up = vec![NO_SCOPE; scopes];
        for scope in 0..scopes {
            up[scope] = match (declaring[scope], resolution.parents[scope]) {
                (NO_LOCAL, NO_SCOPE) => NO_SCOPE,
                (NO_LOCAL, parent) => up[parent],
                _ => scope,
            };
        }
        let around = |scope: usize| match resolution.parents[scope] {
            NO_SCOPE => NO_SCOPE,
            parent => up[parent],
        };
        let mut crossing = vec![Vec::new(); starts.len() - 1];
        let mut recorded = HashSet::<u64>::new();
        let mut words = Vec::new();
        let mut numbers = HashMap::new();
        let mut of_token = vec![NO_LOCAL; tokens.len()];
        let mut count = vec![0u32; locals as usize];
        let mut first = vec![u32::MAX; locals as usize];
        for (index, occurrence) in resolution.occurrences.iter().enumerate() {
            let Some(occurrence) = occurrence else {
                continue;
            };
            let raw_word = tokens[index].text(source);
            if in_raw[index] || !compiler_name(raw_word) {
                continue;
            }
            let local = occurrence
                .declaration
                .map_or(NO_LOCAL, |declaration| local_of[declaration]);
            let (key, target) = match local {
                NO_LOCAL => {
                    let word = canonical(raw_word);
                    let number = *numbers.entry(word).or_insert_with(|| {
                        words.push(word);
                        words.len() as u32 - 1
                    });
                    (WORD | number, NO_SCOPE)
                }
                local => {
                    of_token[index] = local;
                    count[local as usize] += 1;
                    first[local as usize] = first[local as usize].min(index as u32);
                    let declaration = occurrence.declaration.expect("a local's declaration");
                    (local, declarations[declaration].scope)
                }
            };
            let mut scope = up[occurrence.scope];
            while scope != target
                && scope != NO_SCOPE
                && recorded.insert((scope as u64) << 32 | u64::from(key))
            {
                crossing[declaring[scope] as usize].push(key);
                scope = around(scope);
            }
        }
        Self {
            of_token,
            count,
            first,
            starts,
            crossing,
            words,
        }
    }

    /// The short names of the locals, by local, given the table's `words`:
    /// scope by scope, parents first, the scope's locals, the most frequent
    /// first, take the first `candidates` that no name it avoids is.
    fn names<F: Fn(&str) -> bool>(
        &self,
        words: &HashMap<&str, String>,
        candidates: &mut Candidates<F>,
    ) -> Vec<String> {
        self.constrained_names(words, candidates, &HashMap::new())
    }

    /// [`Locals::names`] where some locals have constraints: a local of
    /// `avoid` takes the first candidate that no more frequent local of its
    /// scope has and that is none of its avoided names, and the same-length
    /// batches reorder only the names of the scope's other locals, as
    /// [`constrained_short_names`] does for the table.
    fn constrained_names<F: Fn(&str) -> bool>(
        &self,
        words: &HashMap<&str, String>,
        candidates: &mut Candidates<F>,
        avoid: &HashMap<u32, &HashSet<&str>>,
    ) -> Vec<String> {
        let mut names = vec![String::new(); self.count.len()];
        let mut order = Vec::new();
        let mut chosen = Vec::new();
        for (scope, crossing) in self.crossing.iter().enumerate() {
            order.clear();
            order.extend(self.starts[scope] as usize..self.starts[scope + 1] as usize);
            order.sort_by_key(|&local| (std::cmp::Reverse(self.count[local]), self.first[local]));
            let avoided = crossing
                .iter()
                .map(|&key| match key & WORD {
                    0 => names[key as usize].as_str(),
                    _ => words[self.words[(key & !WORD) as usize]].as_str(),
                })
                .collect::<HashSet<_>>();
            if !avoid.is_empty()
                && order
                    .iter()
                    .any(|&local| avoid.contains_key(&(local as u32)))
            {
                let assigned = constrained_scope(
                    &order,
                    &avoided,
                    candidates,
                    |local| {
                        avoid
                            .get(&(local as u32))
                            .copied()
                            .filter(|set| !set.is_empty())
                    },
                    |local| self.first[local],
                );
                for (local, name) in assigned {
                    names[local] = name;
                }
                continue;
            }
            chosen.clear();
            let mut at = 0;
            for _ in &order {
                while avoided.contains(candidates.get(at)) {
                    at += 1;
                }
                chosen.push(candidates.get(at).to_owned());
                at += 1;
            }
            // Among the names of one length, in order of first occurrence,
            // as the table assigns them (`constrained_short_names`).
            let mut batch = 0;
            while batch < chosen.len() {
                let length = chosen[batch].len();
                let end = batch + chosen[batch..].partition_point(|name| name.len() == length);
                order[batch..end].sort_by_key(|&local| self.first[local]);
                batch = end;
            }
            for (&local, name) in order.iter().zip(chosen.drain(..)) {
                names[local] = name;
            }
        }
        names
    }
}

/// The names of one scope's locals in `order`, the most frequent first,
/// where some have avoided names of their own ([`Locals::constrained_names`]):
/// each takes the first of `candidates` that no local before it took, that
/// the scope avoids and that it avoids itself. The locals then take the
/// names of each length in order of first occurrence, each the first that
/// it does not avoid. `own_of` gives a local's constraint and `first_of`
/// gives its first occurrence.
fn constrained_scope<'a, 'b: 'a, P: Ord, F: Fn(&str) -> bool>(
    order: &[usize],
    avoided: &HashSet<&str>,
    candidates: &mut Candidates<F>,
    own_of: impl Fn(usize) -> Option<&'a HashSet<&'b str>>,
    first_of: impl Fn(usize) -> P,
) -> Vec<(usize, String)> {
    let mut taken: Vec<bool> = Vec::new();
    let mut free = 0;
    let mut chosen = Vec::with_capacity(order.len());
    let mut owns = Vec::with_capacity(order.len());
    for &local in order {
        let own = own_of(local);
        owns.push(own);
        while taken.get(free) == Some(&true) {
            free += 1;
        }
        let mut at = free;
        loop {
            if at == taken.len() {
                taken.push(false);
            } else if taken[at] {
                at += 1;
                continue;
            }
            let name = candidates.get(at);
            if avoided.contains(name) {
                // Scope-wide avoided names are unusable by every local in this
                // scope; mark `taken` so `free` skips them in amortized O(1).
                taken[at] = true;
                at += 1;
                continue;
            }
            if !own.is_some_and(|own| own.contains(name)) {
                break;
            }
            at += 1;
        }
        taken[at] = true;
        chosen.push(at);
    }
    // By name length, every local: in order of first occurrence, each takes
    // the first of the batch's names that no local before it took and that
    // it does not avoid. Where a local finds none, the batch keeps the
    // names in order of frequency.
    let mut batches: Vec<Vec<usize>> = Vec::new();
    for (position, &at) in chosen.iter().enumerate() {
        let length = candidates.get(at).len();
        if batches.len() <= length {
            batches.resize_with(length + 1, Vec::new);
        }
        batches[length].push(position);
    }
    for batch in batches {
        if batch.is_empty() {
            continue;
        }
        let mut slots = batch
            .iter()
            .map(|&position| chosen[position])
            .collect::<Vec<_>>();
        slots.sort_unstable();
        let mut positions = batch;
        positions.sort_by_key(|&position| first_of(order[position]));
        if !positions.iter().any(|&position| owns[position].is_some()) {
            for (position, slot) in positions.into_iter().zip(slots) {
                chosen[position] = slot;
            }
            continue;
        }
        let mut used = vec![false; slots.len()];
        let mut first_unused = 0;
        let mut assigned = Vec::with_capacity(positions.len());
        for &position in &positions {
            while used.get(first_unused) == Some(&true) {
                first_unused += 1;
            }
            let index = match owns[position] {
                None => first_unused,
                Some(own) => {
                    let Some(index) = (first_unused..slots.len())
                        .find(|&index| !used[index] && !own.contains(candidates.get(slots[index])))
                    else {
                        assigned.clear();
                        break;
                    };
                    index
                }
            };
            used[index] = true;
            assigned.push((position, slots[index]));
        }
        for (position, slot) in assigned {
            chosen[position] = slot;
        }
    }
    order
        .iter()
        .zip(chosen)
        .map(|(&local, at)| (local, candidates.get(at).to_owned()))
        .collect()
}

/// The [`NameGenerator`] sequence without `excluded` names, generated as far
/// as it is read.
struct Candidates<F> {
    names: Vec<String>,
    generator: NameGenerator,
    excluded: F,
}

impl<F: Fn(&str) -> bool> Candidates<F> {
    fn new(excluded: F) -> Self {
        Self {
            names: Vec::new(),
            generator: NameGenerator::default(),
            excluded,
        }
    }

    #[inline]
    fn get(&mut self, at: usize) -> &str {
        if at >= self.names.len() {
            self.grow(at);
        }
        &self.names[at]
    }

    #[cold]
    #[inline(never)]
    fn grow(&mut self, at: usize) {
        let excluded = &self.excluded;
        while self.names.len() <= at {
            let name = self
                .generator
                .find(|name| !excluded(name))
                .expect("names never run out");
            self.names.push(name);
        }
    }
}

/// The terse output of an `Advanced` program, `source` as the emitter wrote
/// it: `segments` are its raw `js*` ranges and `runtime` the embedded runtime
/// factory, already terse ([`terse_slice`]).
///
/// Every compiler name ([`compiler_name`]) gets a short name: a local below
/// the top level per scope ([`Locals`]), the first that nothing occurring in
/// its scope and resolving outside it has, so sibling functions repeat the
/// same names; every other one from one table, the most frequent first.
/// The new names avoid every other identifier of the output
/// (host globals, helper parameters, properties, and each identifier of raw
/// `js*` text, whatever it spells) and the ECMAScript globals the runtime
/// reads, so raw text still names only its holes and host globals. The
/// program is rewritten: a member read with a constant identifier key is
/// dotted (`x.length`), redundant parentheses and semicolons go, and tokens
/// are joined by [`TerseWriter`]. Raw text keeps each of its tokens and
/// comments as written (light-cljs-294); only its whitespace goes, except
/// line breaks the parse or a comment needs.
///
/// When a token crosses between raw text and a hole (a hole inside a string,
/// template or regex of the raw text), a hole with compiler names is inside
/// a comment of it, or the raw text may not lex as an engine lexes it
/// ([`raw_tokens`]), the program keeps its layout instead, and its compiler
/// names are the hygiene prefix and a short name (`__light0_a`), as every
/// program with raw text was written before light-cljs-294.
///
/// `marks`, positions in `source` ([`crate::source_map`]), move with the
/// text they mark: a mark before a token to where that token is written
/// (the next written one, for a token the rewrite drops), and a mark in
/// the runtime factory by its offset there.
pub(crate) fn terse_program(
    source: &str,
    segments: &[Range<usize>],
    runtime: Option<Range<usize>>,
    marks: Option<&mut crate::source_map::Marks>,
) -> String {
    let mut raw = segments
        .iter()
        .filter(|segment| Some(*segment) != runtime.as_ref() && !segment.is_empty())
        .cloned()
        .collect::<Vec<_>>();
    raw.sort_by_key(|range| range.start);
    match rewritten(source, runtime.clone(), &raw, marks.is_some()) {
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
        None if !raw.is_empty() => {
            let (output, anchors) = hygienic(source, &raw, runtime);
            if let Some(marks) = marks {
                marks.relocate(|offset| Some(renamed_offset(&anchors, offset)));
            }
            output
        }
        None => {
            debug_assert!(
                source.len() >= u32::MAX as usize,
                "emitted JavaScript does not lex:\n{source}"
            );
            source.to_owned()
        }
    }
}

/// The terse output of an ES module whose runtime slice is the `kept` units
/// (by unit index) written in a function around the program's own
/// (light-cljs-1062; docs/advanced-optimization.md, "Phase 3"):
/// `(()=>{<units>return(()=>{<program>})()})()`, strict code as all of a
/// module is. `source` is the program as the emitter wrote it for
/// [`terse_program`], with the slice's binding list, the `statement`
/// `const [rt$a, ...] = <factory>;` (`roots` are its bindings, in order),
/// which this form leaves out; `segments` are its raw `js*` ranges, the
/// factory's, `factory`, among them.
///
/// The program is named as [`terse_program`] names it, a root `rt$x` among
/// the locals of the program's function, but it names the units' binding
/// `x` itself, so its name also avoids the units' locals visible where they
/// read `x` ([`Locals::constrained_names`]). A root that keeps its spelling
/// (`Atom`) is a local the statement's place declares instead,
/// `const a=Atom;`, so the program never spells it. The units'
/// other bindings, which the program never reads, are named as a factory's
/// are, by their frequency in the units, avoiding the roots' names, the
/// units' visible locals and every other identifier of the program (a host
/// global that it reads resolves through the units' scope). The program's
/// own names may be theirs: its function's scope shadows them.
///
/// `marks` move as [`terse_program`] moves them, those of the statement
/// going, and the units' lines map to `src/js_runtime.js` as a factory's
/// do. Also returns each unit's bytes, by unit index. `None` where the
/// program does not lex or the units could capture a name it reads
/// ([`captured_by_units`]); the caller then writes the factory.
pub(crate) fn terse_program_nested(
    source: &str,
    segments: &[Range<usize>],
    statement: Range<usize>,
    factory: Range<usize>,
    kept: &[bool],
    roots: &[&str],
    marks: Option<&mut crate::source_map::Marks>,
) -> Option<(String, Vec<usize>)> {
    debug_assert_eq!(kept.len(), TERSE_UNITS.len());
    debug_assert!(statement.start <= factory.start && factory.end <= statement.end);
    let mut raw = segments
        .iter()
        .filter(|segment| **segment != factory && !segment.is_empty())
        .cloned()
        .collect::<Vec<_>>();
    raw.sort_by_key(|range| range.start);
    let units = TERSE_UNITS
        .iter()
        .zip(kept)
        .filter_map(|(unit, kept)| kept.then_some(unit))
        .collect::<Vec<_>>();
    // The bindings that keep their spelling, which the units declare under
    // it in the program's enclosing scope.
    let manifest = crate::runtime_manifest::RuntimeManifest::embedded();
    let spelled = manifest
        .units
        .iter()
        .zip(kept)
        .filter(|(_, kept)| **kept)
        .flat_map(|(unit, _)| unit.declares.iter().copied())
        .filter(|binding| keeps_name(binding))
        .collect::<HashSet<_>>();
    if raw
        .iter()
        .any(|range| captured_by_units(&source[range.clone()], &spelled))
    {
        return None;
    }
    let mut is_root = vec![false; TERSE_BINDINGS.len()];
    for root in roots {
        is_root[binding_index(root)] = true;
    }
    // A root's name avoids the units' locals visible where they read it.
    let mut avoid: HashMap<&'static str, HashSet<&'static str>> = HashMap::new();
    for unit in &units {
        for &(binding, names) in unit.avoid {
            avoid
                .entry(runtime_read(binding))
                .or_default()
                .extend(names.split(' '));
        }
    }
    let mut lexed = Lexed::new(source, Some(factory.clone()), &raw, |_| true)?;
    let mut words: Vec<&str> = Vec::new();
    let mut excluded = HashSet::new();
    lexed.words(&mut words, &mut excluded);
    // Every other identifier of the program, raw text's among them, could
    // be a host global it reads: one that a unit declares would capture it.
    if spelled.iter().any(|binding| excluded.contains(binding)) {
        return None;
    }
    // The statement's tokens, which no declaration around joins.
    let first = lexed
        .tokens
        .partition_point(|token| (token.start as usize) < statement.start);
    let after = lexed
        .tokens
        .partition_point(|token| (token.start as usize) < statement.end);
    for join in &mut lexed.joins[first..=after.min(lexed.tokens.len() - 1)] {
        *join = Join::Keep;
    }
    if first > 0 && lexed.joins[first - 1] == Join::Comma {
        lexed.joins[first - 1] = Join::Keep;
    }
    // The roots declared in the statement, by local. Each is named by its
    // reads in the program, as a factory's roots are: counting the units'
    // reads too measured larger after gzip (light-cljs-1062).
    let mut root_locals: Vec<(usize, u32)> = Vec::new();
    // The roots that keep their spelling, which the program's function
    // reads by short names it declares where the statement was:
    // `const a=Atom,b=LazySeq;`.
    let mut aliased: Vec<(usize, u32)> = Vec::new();
    for (index, token) in lexed.tokens[first..after].iter().enumerate() {
        let word = token.text(source);
        if token.kind == Kind::Name
            && let Some(binding) = word.strip_prefix("rt$")
        {
            let local = lexed.locals.of_token[first + index];
            let binding = binding_index(binding);
            if keeps_name(TERSE_BINDINGS[binding]) {
                if local == NO_LOCAL {
                    return None;
                }
                aliased.push((binding, local));
            } else if local != NO_LOCAL {
                root_locals.push((binding, local));
            }
        }
    }
    let local_avoid = root_locals
        .iter()
        .filter_map(|&(binding, local)| Some((local, avoid.get(runtime_read(binding))?)))
        .collect::<HashMap<_, _>>();
    let unavailable = |name: &str| excluded.contains(name) || keeps_name(name);
    let names = constrained_short_names(words, unavailable, &avoid, |_| None);
    let locals =
        lexed
            .locals
            .constrained_names(&names, &mut Candidates::new(unavailable), &local_avoid);
    // Each other root's name, by binding index: its local's, or the table's.
    let mut named: Vec<Option<&str>> = vec![None; TERSE_BINDINGS.len()];
    for &(binding, local) in &root_locals {
        named[binding] = Some(locals[local as usize].as_str());
    }
    for (index, slot) in named.iter_mut().enumerate() {
        if is_root[index] && slot.is_none() && !keeps_name(TERSE_BINDINGS[index]) {
            *slot = names.get(runtime_read(index)).map(String::as_str);
        }
    }
    debug_assert!(named.iter().enumerate().all(|(index, name)| {
        name.is_none_or(|name| {
            avoid
                .get(runtime_read(index))
                .is_none_or(|avoided| !avoided.contains(name))
        })
    }));
    // The units' other bindings avoid every name the program's function
    // could read through their scope.
    let rooted = named
        .iter()
        .flatten()
        .copied()
        .chain(names.values().map(String::as_str))
        .collect::<HashSet<_>>();
    let mut count = vec![0usize; TERSE_BINDINGS.len()];
    let mut seen_at = vec![usize::MAX; TERSE_BINDINGS.len()];
    let mut seen = 0;
    let mut internal_avoid: HashMap<usize, HashSet<&str>> = HashMap::new();
    for unit in &units {
        for (_, index) in placeholders(unit.text) {
            count[index] += 1;
            if seen_at[index] == usize::MAX {
                seen_at[index] = seen;
                seen += 1;
            }
        }
        for &(binding, names) in unit.avoid {
            internal_avoid
                .entry(binding)
                .or_default()
                .extend(names.split(' '));
        }
    }
    let mut internal = (0..TERSE_BINDINGS.len())
        .filter(|&index| {
            count[index] > 0 && named[index].is_none() && !keeps_name(TERSE_BINDINGS[index])
        })
        .collect::<Vec<_>>();
    internal.sort_by_key(|&index| (std::cmp::Reverse(count[index]), seen_at[index]));
    let mut candidates = NameGenerator::default().filter(|name| {
        !unavailable(name)
            && !rooted.contains(name.as_str())
            && TERSE_GLOBALS.binary_search(&name.as_str()).is_err()
    });
    let mut generated: Vec<String> = Vec::new();
    let mut taken: Vec<bool> = Vec::new();
    let mut free = 0;
    let mut chosen: Vec<Option<usize>> = vec![None; TERSE_BINDINGS.len()];
    for &binding in &internal {
        let avoided = internal_avoid.get(&binding);
        while taken.get(free) == Some(&true) {
            free += 1;
        }
        let mut at = free;
        loop {
            if at == generated.len() {
                generated.push(candidates.next().expect("names never run out"));
                taken.push(false);
            }
            if !taken[at]
                && !avoided.is_some_and(|avoided| avoided.contains(generated[at].as_str()))
            {
                break;
            }
            at += 1;
        }
        taken[at] = true;
        chosen[binding] = Some(at);
    }
    let name = |index: usize| -> &str {
        match (named[index], chosen[index]) {
            (Some(name), _) => name,
            (None, Some(at)) => generated[at].as_str(),
            (None, None) => TERSE_BINDINGS[index],
        }
    };
    // The statement's first token and its `;`, its last.
    let (start, semicolon) = (
        lexed.tokens[first].start as usize,
        lexed.tokens[after - 1].start as usize,
    );
    debug_assert_eq!(&source[semicolon..semicolon + 1], ";");
    let offsets = if marks.is_some() {
        RewriteOffsets::all(lexed.tokens.len())
    } else {
        RewriteOffsets::pair(start, semicolon)
    };
    let (output, offsets) = lexed.rewrite(&names, &locals, offsets);
    let (cut, written) = match offsets {
        RewriteOffsets::Pair { found, .. } => {
            let cut_start = found.0.unwrap_or(output.len());
            let cut_semi = found.1.unwrap_or(output.len());
            (cut_start..cut_semi + 1, Vec::new())
        }
        RewriteOffsets::All(written) => {
            let at = |offset: usize| {
                written
                    .get(written.partition_point(|(old, _)| *old < offset))
                    .map_or(output.len(), |(_, new)| *new)
            };
            (at(start)..at(semicolon) + 1, written)
        }
        RewriteOffsets::None => unreachable!(),
    };
    // The statement as written, through its `;`: what follows, comments of
    // raw text before the next token among it, stays.
    debug_assert_eq!(&output[cut.end - 1..cut.end], ";");
    // What the statement is written as: the aliases, if any.
    let mut alias = String::new();
    for &(binding, local) in &aliased {
        alias.push_str(if alias.is_empty() { "const " } else { "," });
        alias.push_str(&locals[local as usize]);
        alias.push('=');
        alias.push_str(TERSE_BINDINGS[binding]);
    }
    if !alias.is_empty() {
        alias.push(';');
    }
    const OPEN: &str = "(()=>{";
    let mut text = String::from(OPEN);
    let mut unit_marks = marks.is_some().then(Vec::new);
    if let Some(marks) = unit_marks.as_mut() {
        // The function's opening is the runtime's, as a factory's is.
        let prologue = &crate::runtime_manifest::ANNOTATED_RUNTIME[manifest.prologue.clone()];
        marks.push((0, prologue.find("(() =>").unwrap_or_default(), None));
    }
    let unit_bytes = write_units(&mut text, kept, name, unit_marks.as_mut());
    let units_end = text.len();
    text.push_str("return");
    let program_at = text.len();
    text.push_str(&output[..cut.start]);
    // The aliases continue a `const` statement that follows.
    let mut rest = cut.end;
    if !alias.is_empty() && output[rest..].starts_with("const ") {
        alias.pop();
        alias.push(',');
        rest += "const ".len();
    }
    text.push_str(&alias);
    text.push_str(output[rest..].trim_end());
    text.push_str("})()\n");
    if let Some(marks) = marks {
        let at = |offset: usize| {
            written
                .get(written.partition_point(|(old, _)| *old < offset))
                .map_or(output.len(), |(_, new)| *new)
        };
        marks.relocate(|offset| {
            if statement.start <= offset && offset < statement.end {
                return None;
            }
            let new = at(offset);
            Some(if new >= cut.end {
                program_at + cut.start + alias.len() + new.saturating_sub(rest)
            } else {
                program_at + new
            })
        });
        // Offsets in `text`, which the units' marks are.
        for (offset, source, name) in unit_marks.into_iter().flatten() {
            let origin = (crate::source_map::RUNTIME, source);
            match name {
                Some(name) => marks.named(offset, Some(origin), name),
                None => marks.position(offset, origin.0, origin.1),
            }
        }
        // `return` and the aliases are the compiler's; the program's own
        // text maps as before.
        marks.unmapped(units_end);
        marks.unmapped(program_at + cut.start);
    }
    Some((text, unit_bytes))
}

/// One slice fed into [`terse_program_slices`]: the header slice `H`
/// (`k_prefix = None`) or a body slice `s` (`k_prefix = Some("k{s}_")`,
/// `remap` mapping slice-local constant IDs to canonical constant IDs).
pub(crate) struct TerseSlice<'a> {
    pub(crate) source: &'a str,
    pub(crate) raw: &'a [Range<usize>],
    pub(crate) marks: Option<crate::source_map::Marks>,
    pub(crate) k_prefix: Option<&'a str>,
    pub(crate) remap: &'a [usize],
}

/// Runtime slice information for [`terse_program_slices`] when `embed && slice`.
pub(crate) struct ParallelRuntime<'a> {
    pub(crate) kept: &'a [bool],
    pub(crate) bindings: &'a [&'static str],
    /// Whether the nested outer-IIFE runtime form (`terse_program_nested`) is
    /// eligible (true for terse ES modules; falls back to a runtime factory if
    /// raw `js*` text could be captured by the runtime units).
    pub(crate) can_nest: bool,
}

/// Output of [`terse_program_slices`], ready for streaming `RuntimeKey` hashing
/// and parallel `RuntimeKey::apply_pieces` (or source-mapped assembly).
pub(crate) struct ParallelTerseOutput {
    /// Non-empty text pieces in order, split strictly at statement/wrapper
    /// boundaries (never inside an identifier or `$LKEY` placeholder).
    pub(crate) pieces: Vec<String>,
    /// Total byte length of `pieces`.
    pub(crate) total_len: usize,
    /// Relocated source marks across the full output (when `mapped` is true).
    pub(crate) marks: Option<crate::source_map::Marks>,
    /// Merged `$LKEY` stem counts across `pieces` (when `count_lkeys` is true).
    pub(crate) lkey_counts: crate::runtime_key::StemCounts,
    /// Written bytes of the runtime slice (before short keyed-name adjustment).
    pub(crate) runtime_bytes: usize,
    /// Written per-unit bytes of the runtime slice (before short keyed-name
    /// adjustment).
    pub(crate) unit_bytes: Option<Vec<usize>>,
}

#[inline]
pub(crate) fn canonical_k_word<'a>(
    word: &'a str,
    k_prefix: Option<&str>,
    remap: &[usize],
    canon_k_names: &'a [String],
) -> &'a str {
    if let Some(prefix) = k_prefix
        && let Some(rest) = word.strip_prefix(prefix)
        && let Ok(local) = rest.parse::<usize>()
        && let Some(&canon) = remap.get(local)
    {
        return canon_k_names[canon].as_str();
    }
    word
}

fn boundary_top_decls(
    source: &str,
    tokens: &[Token],
    in_raw: &[bool],
    comments: &[RawComment<'_>],
    statements: &[(usize, usize)],
) -> (Option<&'static str>, Option<&'static str>) {
    if tokens.is_empty() || statements.is_empty() {
        return (None, None);
    }
    let mut statements = statements.to_vec();
    statements.sort_unstable();
    let keyword_of = |text: &str| match text {
        "var" => Some("var"),
        "let" => Some("let"),
        "const" => Some("const"),
        _ => None,
    };
    let has_leading_comment = comments.iter().any(|&(at, ..)| at == 0);
    let has_trailing_comment = comments.iter().any(|&(at, ..)| at == tokens.len());
    let first = if !has_leading_comment && !in_raw[0] && statements[0].0 == 0 {
        keyword_of(tokens[0].text(source))
    } else {
        None
    };
    let last_idx = tokens.len() - 1;
    let last = if !has_trailing_comment && !in_raw[last_idx] {
        statements
            .iter()
            .find(|&&(_, semi)| semi == last_idx)
            .filter(|&&(kw, _)| {
                !in_raw[kw]
                    && (tokens[kw].text(source) != "var"
                        || kw == 0
                        || (tokens[kw - 1].kind == Kind::Punct
                            && matches!(tokens[kw - 1].text(source), ";" | "{" | "}")))
            })
            .and_then(|&(kw, _)| keyword_of(tokens[kw].text(source)))
    } else {
        None
    };
    (first, last)
}

/// Parallel 2-stage terse rewriting across `P+1` independently emitted slices
/// (header slice `H` followed by `P` top-level statement slices):
///
/// 1. Parallel lex + partners + `raw_tokens` + `resolve` + `Locals::new` +
///    per-slice word frequency counting, dropping `Resolution` immediately per
///    slice so at most `slots` slices hold `Resolution` simultaneously.
/// 2. Serial merge of global word stats and `excluded`, one global
///    `constrained_short_names_from_stats`, and writing the nested runtime
///    units (keeping `rooted` restricted to runtime roots so runtime-internal
///    bindings keep 1–2-char short names) or terse runtime factory.
/// 3. Parallel local naming + `Rewrite` + `TerseWriter` + mark relocation +
///    `$LKEY` stem counting, dropping per-slice token buffers immediately.
/// 4. Boundary joining with exact [`TerseWriter::boundary_separator`] and
///    optional adjacent declaration joining (`;var ` -> `,`).
///
/// Falls back to parallel hygienic renaming across all slices if any slice
/// cannot be followed by `raw_tokens` / `lex` (for example unparseable raw
/// `js*` text).
#[allow(clippy::too_many_arguments)]
pub(crate) fn terse_program_slices<'a>(
    slices: Vec<TerseSlice<'a>>,
    canon_k_names: &'a [String],
    runtime: Option<ParallelRuntime<'a>>,
    package_imports: &str,
    is_module: bool,
    mapped: bool,
    count_lkeys: bool,
    slots: crate::scheduler::Slots,
) -> ParallelTerseOutput {
    let manifest = crate::runtime_manifest::RuntimeManifest::embedded();
    let spelled: HashSet<&'static str> = match &runtime {
        Some(rt) if rt.can_nest => manifest
            .units
            .iter()
            .zip(rt.kept)
            .filter(|(_, kept)| **kept)
            .flat_map(|(unit, _)| unit.declares.iter().copied())
            .filter(|binding| keeps_name(binding))
            .collect(),
        _ => HashSet::new(),
    };
    let check_capture = runtime.as_ref().is_some_and(|rt| rt.can_nest);

    struct SliceLexed<'s> {
        source: &'s str,
        raw: &'s [Range<usize>],
        tokens: Vec<Token>,
        partners: Vec<u32>,
        in_raw: Vec<bool>,
        comments: Vec<RawComment<'s>>,
        locals: Locals<'s>,
        joins: Vec<Join>,
        marks: Option<crate::source_map::Marks>,
        k_prefix: Option<&'s str>,
        remap: &'s [usize],
        resolved_ok: bool,
        captured_by_units: bool,
        first_top_decl: Option<&'static str>,
        last_top_decl: Option<&'static str>,
        word_stats: HashMap<&'s str, (usize, u32)>,
        excluded: HashSet<&'s str>,
        top_decls: HashSet<&'s str>,
        disqualified: HashSet<&'s str>,
        occ_words: HashSet<&'s str>,
    }

    enum LexOutcome<'s> {
        Lexed(Box<SliceLexed<'s>>),
        Fallback(TerseSlice<'s>),
    }

    // Step 2.1: Parallel lex + resolve + local analysis + word counting.
    let lex_jobs: Vec<(u64, TerseSlice<'a>)> = slices
        .into_iter()
        .map(|slice| (slice.source.len() as u64, slice))
        .collect();
    let lexed_results: Vec<LexOutcome<'a>> = crate::scheduler::map(slots, lex_jobs, |slice| {
        let source = slice.source;
        if source.len() >= u32::MAX as usize {
            return LexOutcome::Fallback(slice);
        }
        let mut raw = slice
            .raw
            .iter()
            .filter(|segment| !segment.is_empty())
            .cloned()
            .collect::<Vec<_>>();
        raw.sort_by_key(|range| range.start);
        let Ok(tokens) = lex_section(source, 0..source.len(), false) else {
            return LexOutcome::Fallback(slice);
        };
        if tokens.len() >= u32::MAX as usize {
            return LexOutcome::Fallback(slice);
        }
        let partners = partners(source, &tokens);
        let Some((in_raw, comments)) = raw_tokens(source, &tokens, &partners, &raw, None) else {
            return LexOutcome::Fallback(slice);
        };
        let captured_by_units = check_capture
            && raw
                .iter()
                .any(|range| captured_by_units(&source[range.clone()], &spelled));
        let k_prefix = slice.k_prefix;
        let remap = slice.remap;
        let canon = |word: &'a str| canonical_k_word(word, k_prefix, remap, canon_k_names);
        let (
            locals,
            mut joins,
            resolved_ok,
            first_top_decl,
            last_top_decl,
            top_decls,
            disqualified,
            occ_words,
        ) = match resolve(source, &tokens) {
            Ok(resolution) => {
                let locals =
                    Locals::new_canonical(source, &tokens, &in_raw, &resolution, |_| true, canon);
                let mut top_decls = HashSet::new();
                for declaration in &resolution.declarations {
                    if declaration.scope == 0
                        && declaration.token.is_some()
                        && compiler_name(declaration.name)
                        && !operand_local(declaration.name)
                    {
                        top_decls.insert(canon(declaration.name));
                    }
                }
                let mut disqualified = HashSet::new();
                let mut occ_words = HashSet::new();
                for (index, occurrence) in resolution.occurrences.iter().enumerate() {
                    let Some(occurrence) = occurrence else {
                        continue;
                    };
                    let raw_word = tokens[index].text(source);
                    if !compiler_name(raw_word) {
                        continue;
                    }
                    let word = canon(raw_word);
                    if in_raw[index] || occurrence.shorthand {
                        disqualified.insert(word);
                    } else if locals.of_token[index] == NO_LOCAL {
                        occ_words.insert(word);
                    }
                }
                let joins = joined_declarations(source, &tokens, &resolution.statements);
                let (first_decl, last_decl) =
                    boundary_top_decls(source, &tokens, &in_raw, &comments, &resolution.statements);
                // Drop `resolution` immediately inside this job.
                (
                    locals,
                    joins,
                    true,
                    first_decl,
                    last_decl,
                    top_decls,
                    disqualified,
                    occ_words,
                )
            }
            Err(_) => (
                Locals::none(tokens.len()),
                vec![Join::Keep; tokens.len()],
                false,
                None,
                None,
                HashSet::new(),
                HashSet::new(),
                HashSet::new(),
            ),
        };
        for semicolon in 0..tokens.len() {
            if joins[semicolon] == Join::Comma && (in_raw[semicolon] || in_raw[semicolon + 1]) {
                joins[semicolon] = Join::Keep;
                joins[semicolon + 1] = Join::Keep;
            }
        }
        let mut word_stats: HashMap<&'a str, (usize, u32)> = HashMap::new();
        let mut excluded: HashSet<&'a str> = HashSet::new();
        for (index, ((token, &is_raw), &local)) in
            tokens.iter().zip(&in_raw).zip(&locals.of_token).enumerate()
        {
            if token.kind == Kind::Name {
                let raw_word = token.text(source);
                if compiler_name(raw_word) && !is_raw {
                    if local == NO_LOCAL {
                        let word = canon(raw_word);
                        word_stats.entry(word).or_insert((0, index as u32)).0 += 1;
                    }
                } else {
                    excluded.insert(raw_word);
                }
            }
        }
        LexOutcome::Lexed(Box::new(SliceLexed {
            source,
            raw: slice.raw,
            tokens,
            partners,
            in_raw,
            comments,
            locals,
            joins,
            marks: slice.marks,
            k_prefix,
            remap,
            resolved_ok,
            captured_by_units,
            first_top_decl,
            last_top_decl,
            word_stats,
            excluded,
            top_decls,
            disqualified,
            occ_words,
        }))
    });

    if lexed_results
        .iter()
        .any(|outcome| matches!(outcome, LexOutcome::Fallback(_)))
    {
        let recovered: Vec<TerseSlice<'a>> = lexed_results
            .into_iter()
            .map(|outcome| match outcome {
                LexOutcome::Lexed(lexed) => TerseSlice {
                    source: lexed.source,
                    raw: lexed.raw,
                    marks: lexed.marks,
                    k_prefix: lexed.k_prefix,
                    remap: lexed.remap,
                },
                LexOutcome::Fallback(slice) => slice,
            })
            .collect();
        return hygienic_slices(
            recovered,
            canon_k_names,
            runtime,
            package_imports,
            is_module,
            mapped,
            count_lkeys,
            slots,
        );
    }

    // Step 2.2: Serial global name table + nested units / runtime factory.
    let mut lexed_slices = Vec::with_capacity(lexed_results.len());
    let mut global_stats: HashMap<&'a str, (usize, (usize, u32, u32))> = HashMap::new();
    let mut excluded: HashSet<&'a str> = HashSet::new();
    let mut top_decls: HashSet<&'a str> = HashSet::new();
    let mut disqualified: HashSet<&'a str> = HashSet::new();
    let mut occ_words: HashSet<&'a str> = HashSet::new();
    let mut any_captured = false;
    let mut all_resolved = true;
    for (slice_idx, outcome) in lexed_results.into_iter().enumerate() {
        let LexOutcome::Lexed(mut slice) = outcome else {
            unreachable!()
        };
        any_captured |= slice.captured_by_units;
        all_resolved &= slice.resolved_ok;
        for (word, (count, first)) in std::mem::take(&mut slice.word_stats) {
            let entry = global_stats
                .entry(word)
                .or_insert((0, (slice_idx, first, 0)));
            entry.0 += count;
        }
        excluded.extend(std::mem::take(&mut slice.excluded));
        top_decls.extend(std::mem::take(&mut slice.top_decls));
        disqualified.extend(std::mem::take(&mut slice.disqualified));
        occ_words.extend(std::mem::take(&mut slice.occ_words));
        lexed_slices.push(*slice);
    }

    let use_nested = runtime.as_ref().is_some_and(|rt| {
        rt.can_nest
            && !any_captured
            && all_resolved
            && !spelled.iter().any(|binding| excluded.contains(binding))
            && !rt.bindings.iter().any(|binding| {
                keeps_name(binding) && disqualified.contains(runtime_read(binding_index(binding)))
            })
    });

    let mut avoid: HashMap<&'static str, HashSet<&'static str>> = HashMap::new();
    if let Some(rt) = &runtime {
        // Place the runtime root bindings right after `truth` in slice 0
        // (matching `const [rt$a, ...] = <factory>;` in `terse_program` and
        // `terse_program_nested`) and count each root's declaration once.
        let rt_first_token = lexed_slices
            .first()
            .and_then(|s| s.tokens.get(1).map(|t| (s.source, t)))
            .filter(|&(src, tok)| tok.kind == Kind::Name && tok.text(src) == "truth")
            .map_or(0u32, |_| 2u32);
        for (pos, &binding) in rt.bindings.iter().enumerate() {
            let word: &'a str = runtime_read(binding_index(binding));
            top_decls.insert(word);
            let rt_pos = (0, rt_first_token, pos as u32 + 1);
            let entry = global_stats.entry(word).or_insert((0, rt_pos));
            entry.0 += 1;
            if rt_pos < entry.1 {
                entry.1 = rt_pos;
            }
        }
        if use_nested {
            for (unit, _) in TERSE_UNITS.iter().zip(rt.kept).filter(|(_, kept)| **kept) {
                for &(binding, names) in unit.avoid {
                    if !keeps_name(TERSE_BINDINGS[binding]) {
                        avoid
                            .entry(runtime_read(binding))
                            .or_default()
                            .extend(names.split(' '));
                    }
                }
            }
        }
    }

    let unavailable = |name: &str| {
        excluded.contains(name)
            || (use_nested && (keeps_name(name) || TERSE_GLOBALS.binary_search(&name).is_ok()))
    };
    // Match `terse_program` / `terse_program_nested`'s two-tier naming:
    // 1) `non_top_stats` (`operand*`, `recur*`, `captures.l*`, raw/shorthand
    //    names) get short names first so `operand0..` stay 1-char globally.
    // 2) Top-level program bindings (`truth`, `rt$*`, `core$*`, `k*`, `c*`,
    //    `g*`, `host`, `pkg`) are named like Scope 1 of `(() => { ... })()`,
    //    avoiding `non_top` names that occur as variable references (`occ_words`)
    //    and keeping `a, c, d, ...` free for inner functions' per-scope locals.
    let mut global_names: HashMap<&'a str, String>;
    let mut non_top_names_for_rooted: Vec<String> = Vec::new();
    if all_resolved {
        let mut non_top_stats: HashMap<&'a str, (usize, (usize, u32, u32))> = HashMap::new();
        let mut top_entries: Vec<(&'a str, usize, (usize, u32, u32))> = Vec::new();
        for (word, (count, first)) in global_stats {
            if top_decls.contains(word) && !disqualified.contains(word) {
                top_entries.push((word, count, first));
            } else {
                non_top_stats.insert(word, (count, first));
            }
        }
        global_names =
            constrained_short_names_from_stats(non_top_stats, unavailable, &avoid, |_| None);
        let mut top_avoided: HashSet<&str> = HashSet::new();
        for (&word, short) in &global_names {
            if use_nested {
                non_top_names_for_rooted.push(short.clone());
            }
            if occ_words.contains(word) {
                top_avoided.insert(short.as_str());
            }
        }
        top_entries.sort_by_key(|&(_, count, first)| (std::cmp::Reverse(count), first));
        let order: Vec<usize> = (0..top_entries.len()).collect();
        let mut candidates = Candidates::new(unavailable);
        let assigned = constrained_scope(
            &order,
            &top_avoided,
            &mut candidates,
            |idx| avoid.get(top_entries[idx].0).filter(|set| !set.is_empty()),
            |idx| top_entries[idx].2,
        );
        drop(top_avoided);
        for (idx, short) in assigned {
            global_names.insert(top_entries[idx].0, short);
        }
    } else {
        global_names =
            constrained_short_names_from_stats(global_stats, unavailable, &avoid, |_| None);
    }

    // Build prefix text (`package_imports` + optional `export default ` +
    // runtime wrapper/factory) and runtime source marks.
    let mut prefix = String::new();
    if is_module {
        prefix.push_str(package_imports);
        prefix.push_str("export default ");
    }
    let wrapper_start = prefix.len();
    let mut prefix_marks = mapped.then(crate::source_map::Marks::default);
    let mut alias_nonempty = false;
    let (runtime_bytes, unit_bytes) = if let Some(rt) = &runtime {
        if use_nested {
            let units = TERSE_UNITS
                .iter()
                .zip(rt.kept)
                .filter_map(|(unit, kept)| kept.then_some(unit))
                .collect::<Vec<_>>();
            let mut named: Vec<Option<&str>> = vec![None; TERSE_BINDINGS.len()];
            let mut aliased: Vec<(usize, &str)> = Vec::new();
            for &binding in rt.bindings {
                let index = binding_index(binding);
                let rt_word = runtime_read(index);
                if keeps_name(TERSE_BINDINGS[index]) {
                    if let Some(short) = global_names.get(rt_word) {
                        aliased.push((index, short.as_str()));
                    }
                } else {
                    named[index] = global_names.get(rt_word).map(String::as_str);
                }
            }
            // Keep `rooted` restricted to runtime roots and free `non_top`
            // names (not `g`/`k` bindings declared inside the inner program
            // function) so runtime-internal bindings keep 1–2-char short names,
            // matching `terse_program_nested`.
            let rooted: HashSet<&str> = named
                .iter()
                .flatten()
                .copied()
                .chain(non_top_names_for_rooted.iter().map(String::as_str))
                .collect();
            let mut count = vec![0usize; TERSE_BINDINGS.len()];
            let mut seen_at = vec![usize::MAX; TERSE_BINDINGS.len()];
            let mut seen = 0usize;
            let mut internal_avoid: HashMap<usize, HashSet<&str>> = HashMap::new();
            for unit in &units {
                for (_, index) in placeholders(unit.text) {
                    count[index] += 1;
                    if seen_at[index] == usize::MAX {
                        seen_at[index] = seen;
                        seen += 1;
                    }
                }
                for &(binding, names) in unit.avoid {
                    internal_avoid
                        .entry(binding)
                        .or_default()
                        .extend(names.split(' '));
                }
            }
            let mut internal = (0..TERSE_BINDINGS.len())
                .filter(|&index| {
                    count[index] > 0 && named[index].is_none() && !keeps_name(TERSE_BINDINGS[index])
                })
                .collect::<Vec<_>>();
            internal.sort_by_key(|&index| (std::cmp::Reverse(count[index]), seen_at[index]));
            let mut candidates = NameGenerator::default().filter(|name| {
                !unavailable(name)
                    && !rooted.contains(name.as_str())
                    && TERSE_GLOBALS.binary_search(&name.as_str()).is_err()
            });
            let mut generated: Vec<String> = Vec::new();
            let mut taken: Vec<bool> = Vec::new();
            let mut free = 0;
            let mut chosen: Vec<Option<usize>> = vec![None; TERSE_BINDINGS.len()];
            for &binding in &internal {
                let avoided = internal_avoid.get(&binding);
                while taken.get(free) == Some(&true) {
                    free += 1;
                }
                let mut at = free;
                loop {
                    if at == generated.len() {
                        generated.push(candidates.next().expect("names never run out"));
                        taken.push(false);
                    }
                    if !taken[at]
                        && !avoided.is_some_and(|avoided| avoided.contains(generated[at].as_str()))
                    {
                        break;
                    }
                    at += 1;
                }
                taken[at] = true;
                chosen[binding] = Some(at);
            }
            let name = |index: usize| -> &str {
                match (named[index], chosen[index]) {
                    (Some(name), _) => name,
                    (None, Some(at)) => generated[at].as_str(),
                    (None, None) => TERSE_BINDINGS[index],
                }
            };
            let mut runtime_text = String::from("(()=>{");
            let mut unit_marks = mapped.then(Vec::new);
            if let Some(marks) = unit_marks.as_mut() {
                let prologue =
                    &crate::runtime_manifest::ANNOTATED_RUNTIME[manifest.prologue.clone()];
                marks.push((0, prologue.find("(() =>").unwrap_or_default(), None));
            }
            let raw_unit_bytes = write_units(&mut runtime_text, rt.kept, name, unit_marks.as_mut());
            let units_end = wrapper_start + runtime_text.len();
            runtime_text.push_str("return(()=>{");
            let program_body_start = wrapper_start + runtime_text.len();
            for (pos, &(binding, short)) in aliased.iter().enumerate() {
                runtime_text.push_str(if pos == 0 { "const " } else { "," });
                runtime_text.push_str(short);
                runtime_text.push('=');
                runtime_text.push_str(TERSE_BINDINGS[binding]);
            }
            if !aliased.is_empty() {
                runtime_text.push(';');
                alias_nonempty = true;
            }
            prefix.push_str(&runtime_text);
            if let Some(marks) = prefix_marks.as_mut() {
                for (offset, source, name) in unit_marks.into_iter().flatten() {
                    let origin = (crate::source_map::RUNTIME, source);
                    match name {
                        Some(name) => marks.named(wrapper_start + offset, Some(origin), name),
                        None => marks.position(wrapper_start + offset, origin.0, origin.1),
                    }
                }
                marks.unmapped(units_end);
                marks.unmapped(program_body_start);
            }
            let bytes = crate::runtime_slice::written_unit_bytes(raw_unit_bytes);
            let total_bytes = bytes.iter().sum();
            (total_bytes, Some(bytes))
        } else {
            prefix.push_str("(()=>{");
            if !rt.bindings.is_empty() {
                prefix.push_str("const[");
                for (pos, &binding) in rt.bindings.iter().enumerate() {
                    if pos > 0 {
                        prefix.push(',');
                    }
                    let rt_word = runtime_read(binding_index(binding));
                    prefix.push_str(&global_names[rt_word]);
                }
                prefix.push_str("]=");
            }
            let factory_start = prefix.len();
            let mut factory_marks = mapped.then(Vec::new);
            let (factory, raw_unit_bytes) =
                terse_slice_marked(rt.kept, rt.bindings, factory_marks.as_mut());
            let trimmed = factory.trim();
            let trimmed_lead = factory.len() - factory.trim_start().len();
            prefix.push_str(trimmed);
            prefix.push(';');
            if let Some(marks) = prefix_marks.as_mut() {
                for (offset, source, name) in factory_marks.into_iter().flatten() {
                    let origin = (crate::source_map::RUNTIME, source);
                    let pos = factory_start + offset.saturating_sub(trimmed_lead);
                    match name {
                        Some(name) => marks.named(pos, Some(origin), name),
                        None => marks.position(pos, origin.0, origin.1),
                    }
                }
                marks.unmapped(prefix.len());
            }
            let bytes = crate::runtime_slice::written_unit_bytes(raw_unit_bytes);
            let rt_bytes = crate::runtime_key::written_len(trimmed);
            (rt_bytes, Some(bytes))
        }
    } else {
        prefix.push_str("(()=>{");
        (0, None)
    };

    // Step 2.3: Parallel local naming + rewrite + TerseWriter + mark relocation
    // + `$LKEY` stem counting.
    struct SliceRewritten {
        text: String,
        marks: Option<crate::source_map::Marks>,
        lkey_counts: crate::runtime_key::StemCounts,
        first_token: Option<FirstToken>,
        writer_end: Option<WriterEnd>,
        ends_with_droppable_semi: bool,
        first_top_decl: Option<&'static str>,
        last_top_decl: Option<&'static str>,
    }

    let rewrite_jobs: Vec<(u64, SliceLexed<'a>)> = lexed_slices
        .into_iter()
        .map(|slice| (slice.tokens.len() as u64, slice))
        .collect();
    let rewritten_slices = crate::scheduler::map(slots, rewrite_jobs, |slice| {
        let SliceLexed {
            source,
            tokens,
            partners,
            in_raw,
            comments,
            locals,
            joins,
            mut marks,
            k_prefix,
            remap,
            first_top_decl,
            last_top_decl,
            ..
        } = slice;
        let mut candidates = Candidates::new(|name: &str| {
            excluded.contains(name) || (use_nested && keeps_name(name))
        });
        let local_names = locals.names(&global_names, &mut candidates);
        let Locals { of_token, .. } = locals;
        let offsets = if marks.is_some() {
            RewriteOffsets::all(tokens.len())
        } else {
            RewriteOffsets::None
        };
        let name = |index: usize, word: &str| -> &str {
            match of_token[index] {
                NO_LOCAL => {
                    let canon = canonical_k_word(word, k_prefix, remap, canon_k_names);
                    global_names[canon].as_str()
                }
                local => local_names[local as usize].as_str(),
            }
        };
        let rewritten = rewrite_tokens_inner(
            source, None, tokens, partners, in_raw, comments, joins, name, offsets, false,
        );
        if let Some(marks) = marks.as_mut()
            && let RewriteOffsets::All(written) = rewritten.offsets
        {
            let end = rewritten.output.len();
            marks.relocate(|offset| {
                Some(
                    written
                        .get(written.partition_point(|(old, _)| *old < offset))
                        .map_or(end, |(_, new)| *new),
                )
            });
        }
        let lkey_counts = if count_lkeys {
            crate::runtime_key::StemCounts::of(&rewritten.output)
        } else {
            crate::runtime_key::StemCounts::default()
        };
        SliceRewritten {
            text: rewritten.output,
            marks,
            lkey_counts,
            first_token: rewritten.first_token,
            writer_end: rewritten.writer_end,
            ends_with_droppable_semi: rewritten.ends_with_droppable_semi,
            first_top_decl,
            last_top_decl,
        }
    });

    // Step 2.4: Join slices with exact ASI/separator handling, optional
    // cross-slice declaration joining, and merged `$LKEY` counts & marks.
    let mut lkey_counts = if count_lkeys {
        crate::runtime_key::StemCounts::of(&prefix)
    } else {
        crate::runtime_key::StemCounts::default()
    };
    let mut pieces: Vec<String> = Vec::with_capacity(rewritten_slices.len() + 2);
    let mut all_marks = prefix_marks;
    let mut total_len = 0usize;

    // Merge `$LKEY` stem counts across all slices, keeping empty slices only
    // when mapped so their marks still relocate at the boundary offset.
    let mut slices_for_join: Vec<SliceRewritten> = Vec::with_capacity(rewritten_slices.len());
    for mut slice in rewritten_slices {
        if count_lkeys {
            lkey_counts.merge(std::mem::take(&mut slice.lkey_counts));
        }
        if !slice.text.is_empty() || slice.marks.is_some() {
            slices_for_join.push(slice);
        }
    }

    // Optional `const a=Atom;` + `const ...` join with the first non-empty slice.
    if alias_nonempty
        && let Some(first) = slices_for_join.iter_mut().find(|s| !s.text.is_empty())
        && first.first_top_decl == Some("const")
        && first.text.starts_with("const")
        && first
            .text
            .as_bytes()
            .get(5)
            .is_some_and(|&b| b == b' ' || b == b'[' || b == b'{')
    {
        let strip = 5 + usize::from(first.text.as_bytes()[5] == b' ');
        prefix.pop();
        prefix.push(',');
        first.text.drain(..strip);
        if let Some(marks) = first.marks.as_mut() {
            marks.relocate(|offset| Some(offset.saturating_sub(strip)));
        }
    }

    total_len += prefix.len();
    pieces.push(prefix);

    let last_non_empty = slices_for_join.iter().rposition(|s| !s.text.is_empty());
    let mut prev_end: Option<WriterEnd> = None;
    let mut prev_decl: Option<&'static str> = None;
    for (idx, mut slice) in slices_for_join.into_iter().enumerate() {
        if slice.text.is_empty() {
            if let (Some(all), Some(marks)) = (all_marks.as_mut(), slice.marks) {
                all.absorb(marks, total_len);
            }
            continue;
        }
        let mut joined_decl = false;
        if let (Some(kw), Some(next_kw)) = (prev_decl, slice.first_top_decl)
            && kw == next_kw
            && pieces.last().is_some_and(|prev| prev.ends_with(';'))
            && slice.text.starts_with(kw)
            && slice
                .text
                .as_bytes()
                .get(kw.len())
                .is_some_and(|&b| b == b' ' || b == b'[' || b == b'{')
        {
            let strip = kw.len() + usize::from(slice.text.as_bytes()[kw.len()] == b' ');
            if let Some(prev) = pieces.last_mut() {
                prev.pop();
                prev.push(',');
            }
            slice.text.drain(..strip);
            if let Some(marks) = slice.marks.as_mut() {
                marks.relocate(|offset| Some(offset.saturating_sub(strip)));
            }
            joined_decl = true;
        }
        if !joined_decl && let (Some(prev), Some(next)) = (prev_end, slice.first_token.as_ref()) {
            let sep = TerseWriter::boundary_separator(prev, next);
            if !sep.is_empty()
                && let Some(last_piece) = pieces.last_mut()
            {
                last_piece.push_str(sep);
                total_len += sep.len();
            }
        }
        if last_non_empty == Some(idx)
            && slice.ends_with_droppable_semi
            && slice.text.ends_with(';')
        {
            slice.text.pop();
            if let Some(marks) = slice.marks.as_mut() {
                let new_end = slice.text.len();
                marks.relocate(|offset| Some(offset.min(new_end)));
            }
        }
        if let (Some(all), Some(marks)) = (all_marks.as_mut(), slice.marks) {
            all.absorb(marks, total_len);
        }
        total_len += slice.text.len();
        prev_end = slice.writer_end;
        prev_decl = slice.last_top_decl;
        pieces.push(slice.text);
    }

    let suffix = match (use_nested, is_module) {
        (true, true) => concat!("})()", "})()", ";\n"),
        (true, false) => concat!("})()", "})()", "\n"),
        (false, true) => "})();\n",
        (false, false) => "})()\n",
    };
    total_len += suffix.len();
    pieces.push(suffix.to_owned());

    ParallelTerseOutput {
        pieces,
        total_len,
        marks: all_marks,
        lkey_counts,
        runtime_bytes,
        unit_bytes,
    }
}

/// Parallel hygienic fallback for [`terse_program_slices`] when any slice
/// cannot be lexed or followed by `raw_tokens`: preserves layout, prefixes
/// every compiler name with a raw-safe hygiene prefix + short name, and
/// canonicalizes per-slice `k{s}_{local}` references.
#[allow(clippy::too_many_arguments)]
fn hygienic_slices<'a>(
    slices: Vec<TerseSlice<'a>>,
    canon_k_names: &'a [String],
    runtime: Option<ParallelRuntime<'a>>,
    package_imports: &str,
    is_module: bool,
    mapped: bool,
    count_lkeys: bool,
    slots: crate::scheduler::Slots,
) -> ParallelTerseOutput {
    let hy_prefix = raw_hygiene_prefix(
        slices
            .iter()
            .flat_map(|slice| slice.raw.iter().map(|r| &slice.source[r.clone()])),
    );
    let scan_jobs: Vec<(u64, &TerseSlice<'a>)> = slices
        .iter()
        .map(|slice| (slice.source.len() as u64, slice))
        .collect();
    let per_slice_stats = crate::scheduler::map(slots, scan_jobs, |slice| {
        let mut stats: HashMap<&'a str, (usize, u32)> = HashMap::new();
        for (pos, (_, word)) in scanned_names(slice.source, slice.raw)
            .into_iter()
            .enumerate()
        {
            let canon = canonical_k_word(word, slice.k_prefix, slice.remap, canon_k_names);
            stats.entry(canon).or_insert((0, pos as u32)).0 += 1;
        }
        stats
    });
    let mut global_stats: HashMap<&'a str, (usize, (usize, u32))> = HashMap::new();
    if let Some(rt) = &runtime {
        for (pos, &binding) in rt.bindings.iter().enumerate() {
            let word: &'a str = runtime_read(binding_index(binding));
            global_stats.entry(word).or_insert((0, (0, pos as u32))).0 += 1;
        }
    }
    for (slice_idx, stats) in per_slice_stats.into_iter().enumerate() {
        for (word, (count, first)) in stats {
            let entry = global_stats
                .entry(word)
                .or_insert((0, (slice_idx + 1, first)));
            entry.0 += count;
        }
    }
    let names: HashMap<&'a str, String> =
        constrained_short_names_from_stats(global_stats, |_| false, &HashMap::new(), |_| None)
            .into_iter()
            .map(|(word, short)| (word, format!("{hy_prefix}{short}")))
            .collect();

    let mut prefix = String::new();
    if is_module {
        prefix.push_str(package_imports);
        prefix.push_str("export default ");
    }
    prefix.push_str("(() => {\n");
    let mut prefix_marks = mapped.then(crate::source_map::Marks::default);
    let (runtime_bytes, unit_bytes) = if let Some(rt) = &runtime {
        if !rt.bindings.is_empty() {
            prefix.push_str("  const [");
            for (pos, &binding) in rt.bindings.iter().enumerate() {
                if pos > 0 {
                    prefix.push_str(", ");
                }
                let rt_word = runtime_read(binding_index(binding));
                prefix.push_str(&names[rt_word]);
            }
            prefix.push_str("] = ");
        } else {
            prefix.push_str("  ");
        }
        let factory_start = prefix.len();
        let mut factory_marks = mapped.then(Vec::new);
        let (factory, raw_unit_bytes) =
            terse_slice_marked(rt.kept, rt.bindings, factory_marks.as_mut());
        let trimmed = factory.trim();
        let trimmed_lead = factory.len() - factory.trim_start().len();
        prefix.push_str(trimmed);
        prefix.push_str(";\n");
        if let Some(marks) = prefix_marks.as_mut() {
            for (offset, source, name) in factory_marks.into_iter().flatten() {
                let origin = (crate::source_map::RUNTIME, source);
                let pos = factory_start + offset.saturating_sub(trimmed_lead);
                match name {
                    Some(name) => marks.named(pos, Some(origin), name),
                    None => marks.position(pos, origin.0, origin.1),
                }
            }
            marks.unmapped(prefix.len());
        }
        let bytes = crate::runtime_slice::written_unit_bytes(raw_unit_bytes);
        let rt_bytes = crate::runtime_key::written_len(trimmed);
        (rt_bytes, Some(bytes))
    } else {
        (0, None)
    };

    let rename_jobs: Vec<(u64, TerseSlice<'a>)> = slices
        .into_iter()
        .map(|s| (s.source.len() as u64, s))
        .collect();
    let renamed_slices = crate::scheduler::map(slots, rename_jobs, |slice| {
        let k_prefix = slice.k_prefix;
        let remap = slice.remap;
        let (text, anchors) = rename_compiler_names(slice.source, slice.raw, |word| {
            let canon = canonical_k_word(word, k_prefix, remap, canon_k_names);
            names[canon].clone()
        });
        let mut marks = slice.marks;
        if let Some(marks) = marks.as_mut() {
            marks.relocate(|offset| Some(renamed_offset(&anchors, offset)));
        }
        let lkey_counts = if count_lkeys {
            crate::runtime_key::StemCounts::of(&text)
        } else {
            crate::runtime_key::StemCounts::default()
        };
        (text, marks, lkey_counts)
    });

    let mut lkey_counts = if count_lkeys {
        crate::runtime_key::StemCounts::of(&prefix)
    } else {
        crate::runtime_key::StemCounts::default()
    };
    let mut pieces = Vec::with_capacity(renamed_slices.len() + 2);
    let mut all_marks = prefix_marks;
    let mut total_len = prefix.len();
    pieces.push(prefix);
    for (text, marks, counts) in renamed_slices {
        if count_lkeys {
            lkey_counts.merge(counts);
        }
        if let (Some(all), Some(marks)) = (all_marks.as_mut(), marks) {
            all.absorb(marks, total_len);
        }
        if !text.is_empty() {
            total_len += text.len();
            pieces.push(text);
        }
    }
    let suffix = if is_module { "})();\n" } else { "})()\n" };
    total_len += suffix.len();
    pieces.push(suffix.to_owned());

    ParallelTerseOutput {
        pieces,
        total_len,
        marks: all_marks,
        lkey_counts,
        runtime_bytes,
        unit_bytes,
    }
}

/// Whether the raw `js*` text `text` could read a name that units written
/// around the program's function declare ([`terse_program_nested`]), other
/// than through its identifiers, which the units' names avoid: a direct
/// `eval`, which reads a string as code in the program's scope; a name
/// spelled with a `\u` escape (`\u{41}tom`), or text that does not lex
/// alone; or any word of it, in a string too, that is one of `spelled`, the
/// bindings that keep their spelling (`Atom`).
fn captured_by_units(text: &str, spelled: &HashSet<&str>) -> bool {
    let Ok(tokens) = lex_section(text, 0..text.len(), false) else {
        return true;
    };
    if tokens.iter().any(|token| {
        token.kind == Kind::Name && {
            let word = token.text(text);
            word == "eval" || word.contains('\\')
        }
    }) {
        return true;
    }
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$'))
        .any(|word| spelled.contains(word))
}

/// One text of a chunked build for [`terse_texts`]: the source as the
/// emitter wrote it, its raw `js*` ranges, and the embedded runtime factory
/// in it, if any; or, `units`, the runtime units (by unit index) written at
/// module level before it ([`terse_module_units`]), whose top-level
/// bindings the table names too.
pub(crate) struct Text<'s> {
    pub(crate) source: &'s str,
    pub(crate) raw: Vec<Range<usize>>,
    pub(crate) runtime: Option<Range<usize>>,
    pub(crate) units: Option<&'s [bool]>,
}

/// Whether a runtime binding that keeps its spelling ([`TERSE_KEPT`]) is
/// `name`: a unit written at module level declares it under that name.
pub(crate) fn keeps_spelling(name: &str) -> bool {
    keeps_name(name)
}

/// The constraints of the one name table of texts with runtime units at
/// module level ([`terse_texts`]): per `rt$<binding>`, the locals of the
/// units visible where they read the binding, which its name must avoid.
fn module_avoid<'s>(texts: &[Text<'s>]) -> HashMap<&'static str, HashSet<&'static str>> {
    let mut avoid: HashMap<&'static str, HashSet<&'static str>> = HashMap::new();
    for kept in texts.iter().filter_map(|text| text.units) {
        for (unit, _) in TERSE_UNITS.iter().zip(kept).filter(|(_, kept)| **kept) {
            for &(binding, names) in unit.avoid {
                avoid
                    .entry(runtime_read(binding))
                    .or_default()
                    .extend(names.split(' '));
            }
        }
    }
    avoid
}

/// Where units are at module level, `rt$<binding>` for each binding that
/// keeps its spelling: a chunk may import one that only its units read,
/// which name it as written rather than by a placeholder.
fn kept_reads(module: bool) -> impl Iterator<Item = &'static str> {
    TERSE_KEPT
        .iter()
        .filter(move |_| module)
        .filter_map(|binding| TERSE_BINDINGS.binary_search(binding).ok())
        .map(runtime_read)
}

/// The name a runtime read keeps where units are at module level: the
/// binding's own spelling, for one that keeps it.
fn kept_read(word: &str) -> Option<&'static str> {
    let binding = word.strip_prefix("rt$")?;
    TERSE_KEPT
        .binary_search(&binding)
        .ok()
        .map(|index| TERSE_KEPT[index])
}

/// The terse output of the texts of a chunked build (docs/code-splitting.md,
/// "Phase 2"), each written as [`terse_program`] writes one program but with
/// one frequency table over all of them: a compiler name has the same short
/// name in every text, so a name one chunk exports is the name another
/// imports, and the short names avoid every other identifier of every text.
/// Each text's locals are named per scope from the same sequence ([`Locals`]);
/// none is a name another text imports, which are at module level.
/// Also returns that table, by compiler name, from which the import and
/// export lines are written.
///
/// When one text cannot be followed ([`terse_program`]), every text keeps
/// its layout, and the compiler names are one hygiene prefix that no raw
/// text of any chunk contains and a short name.
///
/// Where texts have runtime units at module level (light-cljs-303), the
/// table also names the units' top-level bindings, a program read `rt$x`
/// and the binding `x` one name: their occurrences count, each binding's
/// name avoids the units' locals visible where they read it, a binding that
/// keeps its spelling keeps it, and no name is one of those spellings or a
/// global the units read ([`TERSE_GLOBALS`]). The units' text itself is
/// written afterwards from the table ([`terse_module_units`]).
#[cfg(test)]
pub(crate) fn terse_texts<'s>(texts: &[Text<'s>]) -> (Vec<String>, HashMap<&'s str, String>) {
    terse_texts_with_slots(texts, crate::scheduler::Slots::ONE)
}

/// Parallel version of [`terse_texts`] on `slots` CPU slots: fans out lexing,
/// resolution, local analysis and per-text word counting across all `texts`,
/// merges the single global frequency table serially, then fans out local
/// naming and rewriting across all `texts`. Produces output identical to
/// [`terse_texts`].
pub(crate) fn terse_texts_with_slots<'s>(
    texts: &[Text<'s>],
    slots: crate::scheduler::Slots,
) -> (Vec<String>, HashMap<&'s str, String>) {
    let module = texts.iter().any(|text| text.units.is_some());
    let fixed = |word: &str| if module { kept_read(word) } else { None };
    let module_level =
        |name: &str| module && (keeps_name(name) || TERSE_GLOBALS.binary_search(&name).is_ok());
    let avoid = module_avoid(texts);
    // A runtime read is no local; the constrained and fixed ones keep the
    // table's name.
    let eligible = |word: &str| fixed(word).is_none() && !avoid.contains_key(word);
    let raws = texts
        .iter()
        .map(|text| {
            let mut raw = text
                .raw
                .iter()
                .filter(|segment| Some(*segment) != text.runtime.as_ref() && !segment.is_empty())
                .cloned()
                .collect::<Vec<_>>();
            raw.sort_by_key(|range| range.start);
            raw
        })
        .collect::<Vec<_>>();
    let lexed_jobs = texts
        .iter()
        .zip(&raws)
        .map(|(text, raw)| (text.source.len() as u64, (text, raw.as_slice())))
        .collect::<Vec<_>>();
    let lexed_parts = crate::scheduler::map(slots, lexed_jobs, |(text, raw)| {
        let lexed = Lexed::new(text.source, text.runtime.clone(), raw, eligible)?;
        let mut word_stats: HashMap<&'s str, (usize, u32)> = HashMap::new();
        let mut excluded: HashSet<&'s str> = HashSet::new();
        let mut unit_pos = 0u32;
        if let Some(kept) = text.units {
            for word in unit_words(kept) {
                let word: &'s str = word;
                word_stats.entry(word).or_insert((0, unit_pos)).0 += 1;
                unit_pos += 1;
            }
        }
        lexed.word_stats(unit_pos, |w| w, &mut word_stats, &mut excluded);
        Some((lexed, word_stats, excluded))
    });
    if lexed_parts.iter().all(Option::is_some) {
        let mut lexed = Vec::with_capacity(lexed_parts.len());
        let mut stats: HashMap<&'s str, (usize, (usize, u32))> = HashMap::new();
        let mut excluded: HashSet<&'s str> = HashSet::new();
        for (text_index, part) in lexed_parts.into_iter().enumerate() {
            let (text_lexed, word_stats, text_excluded) = part.unwrap();
            excluded.extend(text_excluded);
            for (word, (count, first)) in word_stats {
                let entry = stats.entry(word).or_insert((0, (text_index, first)));
                entry.0 += count;
            }
            lexed.push(text_lexed);
        }
        let tail_index = texts.len();
        for (pos, word) in kept_reads(module).enumerate() {
            let word: &'s str = word;
            let entry = stats.entry(word).or_insert((0, (tail_index, pos as u32)));
            entry.0 += 1;
        }
        let names = constrained_short_names_from_stats(
            stats,
            |name| excluded.contains(name) || module_level(name),
            &avoid,
            fixed,
        );
        let rewrite_jobs: Vec<(u64, Lexed<'s>)> = lexed
            .into_iter()
            .map(|text| (text.tokens.len() as u64, text))
            .collect();
        let outputs = crate::scheduler::map(slots, rewrite_jobs, |text| {
            let mut candidates =
                Candidates::new(|name: &str| excluded.contains(name) || module_level(name));
            let locals = text.locals.names(&names, &mut candidates);
            text.rewrite(&names, &locals, RewriteOffsets::None).0
        });
        return (outputs, names);
    }
    drop(lexed_parts);
    debug_assert!(
        raws.iter().any(|raw| !raw.is_empty())
            || texts
                .iter()
                .any(|text| text.source.len() >= u32::MAX as usize),
        "emitted JavaScript does not lex"
    );
    let prefix = raw_hygiene_prefix(
        texts
            .iter()
            .zip(&raws)
            .flat_map(|(text, raw)| raw.iter().map(|range| &text.source[range.clone()])),
    );
    let skipped = texts
        .iter()
        .zip(&raws)
        .map(|(text, raw)| raw.iter().cloned().chain(text.runtime.clone()).collect())
        .collect::<Vec<Vec<_>>>();
    // A unit's locals are short names, which no prefixed name is.
    let scan_jobs = texts
        .iter()
        .zip(&skipped)
        .map(|(text, skip)| (text.source.len() as u64, (text, skip.as_slice())))
        .collect::<Vec<_>>();
    let scanned_per_text = crate::scheduler::map(slots, scan_jobs, |(text, skip)| {
        let mut word_stats: HashMap<&'s str, (usize, u32)> = HashMap::new();
        let mut pos = 0u32;
        if let Some(kept) = text.units {
            for word in unit_words(kept) {
                let word: &'s str = word;
                word_stats.entry(word).or_insert((0, pos)).0 += 1;
                pos += 1;
            }
        }
        for (_, word) in scanned_names(text.source, skip) {
            word_stats.entry(word).or_insert((0, pos)).0 += 1;
            pos += 1;
        }
        word_stats
    });
    let mut stats: HashMap<&'s str, (usize, (usize, u32))> = HashMap::new();
    for (text_index, word_stats) in scanned_per_text.into_iter().enumerate() {
        for (word, (count, first)) in word_stats {
            let entry = stats.entry(word).or_insert((0, (text_index, first)));
            entry.0 += count;
        }
    }
    let tail_index = texts.len();
    for (pos, word) in kept_reads(module).enumerate() {
        let word: &'s str = word;
        let entry = stats.entry(word).or_insert((0, (tail_index, pos as u32)));
        entry.0 += 1;
    }
    let names = constrained_short_names_from_stats(stats, |_| false, &HashMap::new(), fixed)
        .into_iter()
        .map(|(word, name)| match fixed(word) {
            Some(_) => (word, name),
            None => (word, format!("{prefix}{name}")),
        })
        .collect::<HashMap<_, _>>();
    let rename_jobs = texts
        .iter()
        .zip(&skipped)
        .map(|(text, skip)| (text.source.len() as u64, (text, skip.as_slice())))
        .collect::<Vec<_>>();
    let outputs = crate::scheduler::map(slots, rename_jobs, |(text, skip)| {
        rename_compiler_names(text.source, skip, |word| names[word].clone()).0
    });
    (outputs, names)
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

/// A program with raw `js*` text whose tokens [`rewritten`] cannot follow:
/// compiler names become the hygiene prefix and a short name, and nothing
/// else changes.
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

/// Output-offset tracking for [`rewrite_tokens`]: skipped when source maps are
/// not requested, tracking just two source offsets for [`terse_program_nested`]
/// without marks, or recording every written token for source-map relocation.
enum RewriteOffsets {
    None,
    Pair {
        targets: (usize, usize),
        found: (Option<usize>, Option<usize>),
    },
    All(Vec<(usize, usize)>),
}

impl RewriteOffsets {
    fn pair(first: usize, second: usize) -> Self {
        Self::Pair {
            targets: (first, second),
            found: (None, None),
        }
    }

    fn all(capacity: usize) -> Self {
        Self::All(Vec::with_capacity(capacity))
    }

    #[inline]
    fn record(&mut self, source_start: usize, out: &str, before: usize) {
        match self {
            Self::None => {}
            Self::Pair { targets, found } => {
                if (found.0.is_none() && source_start >= targets.0)
                    || (found.1.is_none() && source_start >= targets.1)
                {
                    let pos = before
                        + usize::from(matches!(out.as_bytes().get(before), Some(b' ' | b'\n')));
                    if found.0.is_none() && source_start >= targets.0 {
                        found.0 = Some(pos);
                    }
                    if found.1.is_none() && source_start >= targets.1 {
                        found.1 = Some(pos);
                    }
                }
            }
            Self::All(written) => {
                let pos =
                    before + usize::from(matches!(out.as_bytes().get(before), Some(b' ' | b'\n')));
                written.push((source_start, pos));
            }
        }
    }
}

/// The program rewritten from its tokens, with the start of each written
/// token in `source` and in the output when `track_written` is set; `None`
/// when it does not lex, or when a token crosses a boundary of the `raw`
/// ranges (sorted, disjoint `js*` text) or the compiler names outside them
/// are not the names [`scanned_names`] finds there.
fn rewritten(
    source: &str,
    runtime: Option<Range<usize>>,
    raw: &[Range<usize>],
    track_written: bool,
) -> Option<(String, Vec<(usize, usize)>)> {
    let lexed = Lexed::new(source, runtime, raw, |_| true)?;
    let mut words = Vec::new();
    let mut excluded = HashSet::new();
    lexed.words(&mut words, &mut excluded);
    let names = short_names(words, |name| excluded.contains(name));
    let locals = lexed
        .locals
        .names(&names, &mut Candidates::new(|name| excluded.contains(name)));
    let offsets = if track_written {
        RewriteOffsets::all(lexed.tokens.len())
    } else {
        RewriteOffsets::None
    };
    let (output, offsets) = lexed.rewrite(&names, &locals, offsets);
    let written = match offsets {
        RewriteOffsets::All(written) => written,
        RewriteOffsets::None | RewriteOffsets::Pair { .. } => Vec::new(),
    };
    Some((output, written))
}

/// A raw `js*` comment kept in terse output: the token index it precedes
/// (`tokens.len()` after the last), its source slice, and whether a line break
/// preceded it since the token before.
type RawComment<'s> = (usize, &'s str, bool);

/// Per-token raw flags paired with the raw comments kept in terse output.
type RawTokens<'s> = (Vec<bool>, Vec<RawComment<'s>>);

/// The tokens of a text [`rewritten`] writes: which are raw `js*` text, the
/// comments of raw text it keeps, its locals named per scope ([`Locals`])
/// and the declarations that join.
struct Lexed<'s> {
    source: &'s str,
    runtime: Option<Range<usize>>,
    tokens: Vec<Token>,
    partners: Vec<u32>,
    in_raw: Vec<bool>,
    comments: Vec<RawComment<'s>>,
    locals: Locals<'s>,
    joins: Vec<Join>,
}

impl<'s> Lexed<'s> {
    /// `source` lexed, with its `runtime` factory one token; `None` where
    /// [`rewritten`] cannot follow it. The locals that `eligible` admits are
    /// named per scope ([`Locals`]), unless the text does not parse.
    fn new(
        source: &'s str,
        runtime: Option<Range<usize>>,
        raw: &[Range<usize>],
        eligible: impl Fn(&str) -> bool,
    ) -> Option<Self> {
        if source.len() >= u32::MAX as usize {
            return None;
        }
        // The runtime factory is one token, an atom like a template literal.
        let mut tokens = Vec::new();
        match &runtime {
            Some(factory) => {
                tokens.extend(lex_section(source, 0..factory.start, false).ok()?);
                tokens.push(Token {
                    kind: Kind::Template,
                    start: factory.start as u32,
                    end: factory.end as u32,
                    newline_before: false,
                });
                tokens.extend(lex_section(source, factory.end..source.len(), false).ok()?);
            }
            None => tokens.extend(lex_section(source, 0..source.len(), false).ok()?),
        }
        if tokens.len() >= u32::MAX as usize {
            return None;
        }
        let partners = partners(source, &tokens);
        let (in_raw, comments) = raw_tokens(source, &tokens, &partners, raw, runtime.as_ref())?;
        // Adjacent `let` and `const` statements join: `let a,b;`, unless the
        // `;` or the keyword is raw text.
        let (locals, mut joins) = match resolve(source, &tokens) {
            Ok(resolution) => (
                Locals::new(source, &tokens, &in_raw, &resolution, eligible),
                joined_declarations(source, &tokens, &resolution.statements),
            ),
            Err(_) => (Locals::none(tokens.len()), vec![Join::Keep; tokens.len()]),
        };
        for semicolon in 0..tokens.len() {
            if joins[semicolon] == Join::Comma && (in_raw[semicolon] || in_raw[semicolon + 1]) {
                joins[semicolon] = Join::Keep;
                joins[semicolon + 1] = Join::Keep;
            }
        }
        Some(Self {
            source,
            runtime,
            tokens,
            partners,
            in_raw,
            comments,
            locals,
            joins,
        })
    }

    /// Add the compiler names of the text the table names, in order, to
    /// `words` (not its locals named per scope), and its other identifiers,
    /// which short names avoid, to `excluded`.
    fn words(&self, words: &mut Vec<&'s str>, excluded: &mut HashSet<&'s str>) {
        let tokens = self.tokens.iter().zip(&self.in_raw);
        for ((token, &in_raw), &local) in tokens.zip(&self.locals.of_token) {
            if token.kind == Kind::Name {
                let word = token.text(self.source);
                if compiler_name(word) && !in_raw {
                    if local == NO_LOCAL {
                        words.push(word);
                    }
                } else {
                    excluded.insert(word);
                }
            }
        }
    }

    /// Aggregate per-word `(count, first_pos)` statistics and `excluded`
    /// identifiers directly for parallel slice/chunk naming, applying
    /// `canonical` to each global compiler name.
    fn word_stats(
        &self,
        pos_offset: u32,
        canonical: impl Fn(&'s str) -> &'s str,
        stats: &mut HashMap<&'s str, (usize, u32)>,
        excluded: &mut HashSet<&'s str>,
    ) {
        let tokens = self.tokens.iter().zip(&self.in_raw);
        for (index, ((token, &in_raw), &local)) in tokens.zip(&self.locals.of_token).enumerate() {
            if token.kind == Kind::Name {
                let raw_word = token.text(self.source);
                if compiler_name(raw_word) && !in_raw {
                    if local == NO_LOCAL {
                        let word = canonical(raw_word);
                        let pos = pos_offset + index as u32;
                        stats.entry(word).or_insert((0, pos)).0 += 1;
                    }
                } else {
                    excluded.insert(raw_word);
                }
            }
        }
    }

    /// The text rewritten with the short `names` of its compiler names and
    /// the names of its `locals` ([`Locals::names`]), recording output offsets
    /// according to `offsets`.
    fn rewrite(
        self,
        names: &HashMap<&str, String>,
        locals: &[String],
        offsets: RewriteOffsets,
    ) -> (String, RewriteOffsets) {
        let Lexed {
            source,
            runtime,
            tokens,
            partners,
            in_raw,
            comments,
            locals: Locals { of_token, .. },
            joins,
        } = self;
        let name = |index: usize, word: &str| -> &str {
            match of_token[index] {
                NO_LOCAL => &names[word],
                local => &locals[local as usize],
            }
        };
        let rewritten = rewrite_tokens_inner(
            source, runtime, tokens, partners, in_raw, comments, joins, name, offsets, true,
        );
        (rewritten.output, rewritten.offsets)
    }
}

#[allow(dead_code)] // Fields used by terse_program_slices (light-cljs-1097)
struct RewrittenTokens {
    output: String,
    offsets: RewriteOffsets,
    first_token: Option<FirstToken>,
    writer_end: Option<WriterEnd>,
    ends_with_droppable_semi: bool,
}

#[allow(clippy::too_many_arguments)]
fn rewrite_tokens_inner<'n>(
    source: &str,
    runtime: Option<Range<usize>>,
    tokens: Vec<Token>,
    partners: Vec<u32>,
    in_raw: Vec<bool>,
    comments: Vec<RawComment<'_>>,
    joins: Vec<Join>,
    name: impl Fn(usize, &str) -> &'n str,
    mut offsets: RewriteOffsets,
    trailing_newline: bool,
) -> RewrittenTokens {
    let is_runtime = |token: &Token| {
        runtime
            .as_ref()
            .is_some_and(|r| r.start == token.start as usize)
    };
    let mut commented = vec![false; tokens.len() + 1];
    for &(index, ..) in &comments {
        commented[index] = true;
    }

    let mut dropped = redundant_semicolons(source, &tokens, &partners);
    for (dropped, in_raw) in dropped.iter_mut().zip(&in_raw) {
        *dropped &= !in_raw;
    }
    let mut rewrite = Rewrite {
        source,
        tokens: &tokens,
        raw: &in_raw,
        commented: &commented,
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
        partners,
        ..
    } = rewrite;
    let mut writer = TerseWriter::default();
    let mut comments = comments.into_iter().peekable();
    let mut first_token: Option<FirstToken> = None;
    let mut last_written: Option<usize> = None;
    for (index, token) in tokens.iter().enumerate() {
        while let Some((_, text, newline)) = comments.next_if(|(at, ..)| *at == index) {
            if first_token.is_none() {
                first_token = Some(FirstToken {
                    kind: Kind::Punct,
                    text: String::new(),
                    is_comment: true,
                    newline_before: newline,
                });
            }
            writer.comment(text, newline);
        }
        if dropped[index] || joins[index] == Join::Drop {
            continue;
        }
        last_written = Some(index);
        // The writer separates a token by at most one space or line break.
        let before = writer.out.len();
        let newline = token.newline_before && !cleared[index];
        if joins[index] == Join::Comma {
            let comma = Shape {
                kind: Kind::Punct,
                text: ",",
            };
            if first_token.is_none() {
                first_token = Some(FirstToken {
                    kind: comma.kind,
                    text: ",".to_owned(),
                    is_comment: false,
                    newline_before: newline,
                });
            }
            writer.write(comma, ",", newline);
            offsets.record(token.start as usize, &writer.out, before);
            continue;
        }
        if let Some(prop) = dotted[index] {
            let dot = Shape {
                kind: Kind::Punct,
                text: ".",
            };
            if first_token.is_none() {
                first_token = Some(FirstToken {
                    kind: dot.kind,
                    text: ".".to_owned(),
                    is_comment: false,
                    newline_before: newline,
                });
            }
            writer.write(dot, ".", newline);
            writer.write(
                Shape {
                    kind: Kind::Name,
                    text: prop,
                },
                prop,
                false,
            );
            offsets.record(token.start as usize, &writer.out, before);
            continue;
        }
        let word = token.text(source);
        if is_runtime(token) {
            let open = Shape {
                kind: Kind::Punct,
                text: "(",
            };
            if first_token.is_none() {
                first_token = Some(FirstToken {
                    kind: open.kind,
                    text: "(".to_owned(),
                    is_comment: false,
                    newline_before: newline,
                });
            }
            writer.write(open, &word[..word.len() - 1], newline);
            let close = Shape {
                kind: Kind::Punct,
                text: ")",
            };
            writer.write(close, ")", false);
        } else if token.kind == Kind::Name && compiler_name(word) && !in_raw[index] {
            let shape = Shape {
                kind: Kind::Name,
                text: word,
            };
            if first_token.is_none() {
                first_token = Some(FirstToken {
                    kind: shape.kind,
                    text: shape.text.to_owned(),
                    is_comment: false,
                    newline_before: newline,
                });
            }
            writer.write(shape, name(index, word), newline);
        } else {
            let shape = Shape {
                kind: token.kind,
                text: word,
            };
            if first_token.is_none() {
                first_token = Some(FirstToken {
                    kind: shape.kind,
                    text: shape.text.to_owned(),
                    is_comment: false,
                    newline_before: newline,
                });
            }
            writer.write(shape, word, newline);
        }
        offsets.record(token.start as usize, &writer.out, before);
    }
    let mut had_trailing_comment = false;
    for (_, text, newline) in comments {
        had_trailing_comment = true;
        if first_token.is_none() {
            first_token = Some(FirstToken {
                kind: Kind::Punct,
                text: String::new(),
                is_comment: true,
                newline_before: newline,
            });
        }
        writer.comment(text, newline);
    }
    let ends_with_droppable_semi = !had_trailing_comment
        && last_written.is_some_and(|last| {
            last > 0
                && joins[last] == Join::Keep
                && dotted[last].is_none()
                && !in_raw[last]
                && tokens[last].kind == Kind::Punct
                && tokens[last].text(source) == ";"
                && {
                    let previous = &tokens[last - 1];
                    let body = match (previous.kind, previous.text(source)) {
                        (Kind::Name, "else" | "do") | (Kind::Punct, ":") => true,
                        (Kind::Punct, ")") => {
                            closes_control_head(source, &tokens, &partners, last - 1)
                        }
                        _ => false,
                    };
                    !body
                }
        });
    let writer_end = writer.end_state();
    let mut output = writer.out;
    if trailing_newline && !output.ends_with('\n') {
        output.push('\n');
    }
    RewrittenTokens {
        output,
        offsets,
        first_token,
        writer_end,
        ends_with_droppable_semi,
    }
}

/// Which `tokens` are raw `js*` text, inside one of the `raw` ranges (sorted
/// and disjoint, without the `runtime` factory), together with the comments of
/// raw text that terse output keeps (each with the index of the token it
/// precedes, or `tokens.len()` after the last, and whether a line break
/// precedes it since the token before).
///
/// Returns `None` when a token crosses a boundary of a raw range, or when the
/// compiler names of the other tokens are not those [`scanned_names`] finds
/// outside the ranges, which the readable output renames. Either happens only
/// where raw text puts a hole inside one of its strings, comments, templates
/// or identifiers.
///
/// Also `None` where raw text may not lex as a JavaScript engine lexes it
/// (the lexer serves the runtime and emitted code): a byte outside ASCII
/// other than in a string literal (`a` and a no-break space would be one
/// name), an HTML-like comment (`<!--`, `-->`), and a `/` that the lexer
/// takes for a division or a regular expression where the tokens before it
/// do not tell which ([`slash_starts_regex`]).
fn raw_tokens<'s>(
    source: &'s str,
    tokens: &[Token],
    partners: &[u32],
    raw: &[Range<usize>],
    runtime: Option<&Range<usize>>,
) -> Option<RawTokens<'s>> {
    let mut in_raw = vec![false; tokens.len()];
    let mut comments = Vec::new();
    if raw.is_empty() {
        return Some((in_raw, comments));
    }
    let bytes = source.as_bytes();
    let non_ascii = |text: &str| {
        if text.is_ascii() {
            0
        } else {
            text.bytes().filter(|byte| !byte.is_ascii()).count()
        }
    };
    let raw_containing = |at: usize| {
        let idx = raw.partition_point(|range| range.end <= at);
        raw.get(idx).filter(|range| range.start <= at)
    };
    // Punctuators that start at `index` and touch: `<!--` and `-->` (`-->=`).
    let adjacent = |index: usize, texts: &[&str]| {
        texts.iter().enumerate().all(|(offset, text)| {
            tokens.get(index + offset).is_some_and(|token| {
                token.kind == Kind::Punct
                    && token.text(source).starts_with(*text)
                    && (offset == 0 || tokens[index + offset - 1].end == token.start)
            })
        })
    };
    let scan_gap = |index: usize,
                    gap_start: usize,
                    gap_end: usize,
                    comments: &mut Vec<RawComment<'s>>,
                    needs_full_scan: &mut bool| {
        if gap_end - gap_start < 2 || !bytes[gap_start..gap_end].contains(&b'/') {
            return;
        }
        let mut newline = false;
        let mut at = gap_start;
        while at < gap_end {
            let end = match (bytes[at], bytes.get(at + 1)) {
                (b'\n' | b'\r', _) => {
                    newline = true;
                    at += 1;
                    continue;
                }
                (b'/', Some(b'/')) => source[at..gap_end]
                    .find(['\n', '\r'])
                    .map_or(gap_end, |length| at + length),
                (b'/', Some(b'*')) => source[at + 2..gap_end]
                    .find("*/")
                    .map_or(gap_end, |length| at + length + 4),
                _ => {
                    at += 1;
                    continue;
                }
            };
            let text = &source[at..end];
            if let Some(range) = raw_containing(at) {
                comments.push((index, text, newline));
                if end > range.end {
                    *needs_full_scan = true;
                }
            } else {
                *needs_full_scan = true;
            }
            newline |= text.contains(['\n', '\r']);
            at = end;
        }
    };

    // Fast path proves `named == scanned_names(source, &skipped)` without
    // re-scanning the entire source whenever `raw` ranges are disjoint from
    // each other and `runtime`, every comment lies inside a single raw range,
    // and every emitted token matches `scanned_names`'s byte rules locally.
    let mut needs_full_scan = raw.windows(2).any(|w| w[0].end > w[1].start)
        || runtime.is_some_and(|rt| raw.iter().any(|r| r.start < rt.end && rt.start < r.end));
    let mut ranges = raw.iter().peekable();
    let mut non_ascii_in_raw_strings = 0usize;
    let mut gap_start = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        let token_start = token.start as usize;
        let token_end = token.end as usize;
        scan_gap(
            index,
            gap_start,
            token_start,
            &mut comments,
            &mut needs_full_scan,
        );
        gap_start = token_end;

        while ranges.next_if(|range| range.end <= token_start).is_some() {}
        match ranges.peek() {
            Some(range) if range.start <= token_start => {
                if token_end > range.end {
                    return None;
                }
                in_raw[index] = true;
                if token.kind == Kind::Str {
                    non_ascii_in_raw_strings += non_ascii(token.text(source));
                }
            }
            Some(range) if range.start < token_end => return None,
            _ => {}
        }

        if in_raw[index] || (index > 0 && in_raw[index - 1]) {
            if in_raw[index]
                && (adjacent(index, &["<", "!", "--"]) || adjacent(index, &["--", ">"]))
            {
                return None;
            }
            let text = token.text(source);
            let slash =
                token.kind == Kind::Regex || (token.kind == Kind::Punct && text.starts_with('/'));
            if slash
                && slash_starts_regex(source, tokens, partners, index)
                    != Some(token.kind == Kind::Regex)
            {
                return None;
            }
        }

        if !in_raw[index] && !needs_full_scan {
            let is_runtime = runtime.is_some_and(|r| r.start == token_start && r.end == token_end)
                && token.kind == Kind::Template;
            if !is_runtime {
                match token.kind {
                    Kind::Punct => {}
                    Kind::Name => {
                        let text = token.text(source);
                        if !text.is_ascii() && !scanned_names(text, &[]).is_empty() {
                            return None;
                        }
                    }
                    Kind::Str => {
                        if bytes[token_start] != b'"' {
                            let text = token.text(source);
                            if text.as_bytes().contains(&b'"') {
                                needs_full_scan = true;
                            } else if !scanned_names(text, &[]).is_empty() {
                                return None;
                            }
                        }
                    }
                    Kind::Number => {
                        let text = token.text(source);
                        if !text.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
                            if bytes.get(token_end).copied() == Some(b'$') {
                                needs_full_scan = true;
                            } else if !scanned_names(text, &[]).is_empty() {
                                return None;
                            }
                        }
                    }
                    Kind::Private => {
                        if !scanned_names(token.text(source), &[]).is_empty() {
                            return None;
                        }
                    }
                    Kind::Regex
                    | Kind::Template
                    | Kind::TemplateHead
                    | Kind::TemplateMiddle
                    | Kind::TemplateTail => {
                        needs_full_scan = true;
                    }
                }
            }
        }
    }
    scan_gap(
        tokens.len(),
        gap_start,
        source.len(),
        &mut comments,
        &mut needs_full_scan,
    );
    if raw
        .iter()
        .map(|range| non_ascii(&source[range.clone()]))
        .sum::<usize>()
        != non_ascii_in_raw_strings
    {
        return None;
    }
    if needs_full_scan {
        let mut skipped = raw.to_vec();
        skipped.extend(runtime.cloned());
        let scanned = scanned_names(source, &skipped);
        let named = tokens
            .iter()
            .zip(&in_raw)
            .filter(|(token, in_raw)| {
                !**in_raw && token.kind == Kind::Name && compiler_name(token.text(source))
            })
            .map(|(token, _)| token.start as usize..token.end as usize);
        if !named.eq(scanned.into_iter().map(|(range, _)| range)) {
            return None;
        }
    }
    Some((in_raw, comments))
}

/// Whether the `/` (or `/=`, or regular expression) token at `index` starts a
/// regular expression, as the tokens before it decide in any JavaScript:
/// `None` after `}` (a block, or an object or function expression), `++`,
/// `--`, and the words that may be identifiers (`of`, `yield`, `let`, ...).
/// The lexer guesses there, and after a keyword used as a property name
/// (`a.return / 2`) it guesses wrong.
fn slash_starts_regex(
    source: &str,
    tokens: &[Token],
    partners: &[u32],
    index: usize,
) -> Option<bool> {
    let Some(previous) = index.checked_sub(1).map(|at| &tokens[at]) else {
        return Some(true);
    };
    // A name after `.` or `?.` is a property name.
    let member = |at: usize| {
        at > 0
            && tokens[at - 1].kind == Kind::Punct
            && matches!(tokens[at - 1].text(source), "." | "?.")
    };
    let word = previous.text(source);
    match previous.kind {
        Kind::Number | Kind::Str | Kind::Template | Kind::TemplateTail | Kind::Regex => Some(false),
        Kind::TemplateHead | Kind::TemplateMiddle => Some(true),
        Kind::Private => Some(false),
        Kind::Name if member(index - 1) => Some(false),
        Kind::Name => match word {
            "this" | "super" | "null" | "true" | "false" => Some(false),
            "return" | "typeof" | "instanceof" | "in" | "new" | "delete" | "void" | "throw"
            | "case" | "do" | "else" => Some(true),
            _ if reserved(word)
                || matches!(
                    word,
                    "of" | "async" | "get" | "set" | "let" | "yield" | "await"
                ) =>
            {
                None
            }
            _ => Some(false),
        },
        Kind::Punct => match word {
            "]" => Some(false),
            // After an `if`, `for`, `while` or `with` head a statement starts.
            ")" => {
                let open = partners[index - 1];
                Some(
                    open != NO_PARTNER
                        && open > 0
                        && tokens[open as usize - 1].kind == Kind::Name
                        && matches!(
                            tokens[open as usize - 1].text(source),
                            "if" | "for" | "while" | "with"
                        )
                        && !member(open as usize - 1),
                )
            }
            "}" | "++" | "--" => None,
            _ => Some(true),
        },
    }
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
    /// Tokens of raw `js*` text, which no rule drops or changes.
    raw: &'t [bool],
    /// Tokens after a comment of raw text, which is written: a group that
    /// starts with one keeps its parentheses, since the comment may end a
    /// line (`return (// c` and a line break).
    commented: &'t [bool],
    partners: Vec<u32>,
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
            let partner = self.partners[at];
            index =
                if partner != NO_PARTNER && (partner as usize) > at && (partner as usize) < close {
                    partner as usize + 1
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
                "(" | "["
                    if self.partners[index] != NO_PARTNER
                        && (self.partners[index] as usize) < close => {}
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
            if self.punct(index) != "["
                || self.partners[index] == NO_PARTNER
                || self.partners[index] as usize != index + 2
                || self.raw[index..=index + 2].contains(&true)
            {
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
            if self.punct(open) != "(" || close == NO_PARTNER {
                continue;
            }
            let close = close as usize;
            if self.next(open) == Some(close) {
                continue;
            }
            // `((x))`: the inner pair, when its top level holds no comma.
            if let Some(inner) = self.next(open)
                && self.punct(inner) == "("
                && self.partners[inner] != NO_PARTNER
                && Some(self.partners[inner] as usize) == self.previous(close)
                && self.next(inner) != Some(self.partners[inner] as usize)
                && !self.holds(inner, self.partners[inner] as usize, ",")
                && !self.raw[inner]
                && !self.raw[self.partners[inner] as usize]
                && !self.commented[inner + 1]
            {
                self.unwrap(inner, self.partners[inner] as usize);
            }
            if self.raw[open] || self.raw[close] || self.commented[open + 1] || !self.group(open) {
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

    /// Units written around the program's function (`terse_program_nested`)
    /// read their globals in the scope where a root takes the program's name
    /// for it, which the [`NameGenerator`] never makes one of them.
    #[test]
    fn no_generated_name_is_a_global_the_units_read() {
        for global in TERSE_GLOBALS {
            assert!(
                crate::js_syntax::unavailable(global),
                "`{global}` could be a root's name"
            );
        }
    }

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
            let start = left[index].start as usize;
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

    /// The terse program of `pieces`, emitted text and raw `js*` text (`true`)
    /// in turn, without a runtime.
    fn terse_with_raw(pieces: &[(bool, &str)]) -> String {
        let mut source = String::new();
        let mut raw = Vec::new();
        for &(is_raw, text) in pieces {
            if is_raw {
                raw.push(source.len()..source.len() + text.len());
            }
            source.push_str(text);
        }
        terse_program(&source, &raw, None, None)
    }

    /// Raw text keeps its tokens, and the compiler's names avoid each of its
    /// identifiers, compiler-shaped ones too (light-cljs-294).
    #[test]
    fn raw_text_keeps_its_tokens_and_names_in_a_terse_program() {
        let output = terse_with_raw(&[
            (false, "let l1 = {b: 1};\nlet l2 = ("),
            (true, "(function (a) {\n  return (a)[\"b\"] + l2;\n})("),
            (false, "l1"),
            (true, ")"),
            (false, ");\nconsole.log(l2, (l1)[\"b\"]);\n"),
        ]);
        assert_eq!(
            output,
            "let c={b:1},d=(function(a){return(a)[\"b\"]+l2;})(c);console.log(d,c.b);\n"
        );
    }

    /// Raw text keeps its comments too: where one started a line it starts
    /// one, a line comment ends its line, and the parentheses of a group that
    /// starts after one stay, so that the comment's line break cannot end a
    /// `return`.
    #[test]
    fn raw_comments_are_kept_where_they_cannot_end_a_statement() {
        let output = terse_with_raw(&[
            (false, "let l1 = (() => {\n  return ("),
            (true, "// first\n  /*#__PURE__*/ f(1) /* a\n   b */"),
            (false, ");\n})();\nlet l2 = ("),
            (true, "l1 /* c */ / /* d */ 2"),
            (false, ");\n"),
            (true, "/** e */"),
            (false, ";\nconsole.log(l1 / l2);\n"),
        ]);
        assert_eq!(
            output,
            "let a=(()=>{return(// first\n\
             /*#__PURE__*/f(1)/* a\n   b */)})(),b=l1/* c *// /* d */2;\n\
             /** e */;console.log(a/b);\n"
        );
    }

    /// Raw text that the lexer may read otherwise than an engine keeps the
    /// hygienic layout: a regular expression after a block, which the lexer
    /// takes for a division, a name and a no-break space, which it takes for
    /// one name, an HTML-like comment, and a keyword as a property name before
    /// a `/`. A `/` after a group or a control head stays terse.
    #[test]
    fn raw_text_the_lexer_may_misread_keeps_the_hygienic_layout() {
        for raw in [
            "if (true) {} / x /.test('x'), ",
            "typeof a\u{a0}, ",
            "1 <!-- x\n, ",
            "({return: 6}).return / x / 1, ",
            "x-->0, ",
        ] {
            let output = terse_with_raw(&[
                (false, "let l1 = 1;\nconsole.log("),
                (true, raw),
                (false, "l1);\n"),
            ]);
            assert!(
                output.starts_with("let __light0_a = 1;"),
                "{raw:?}: {output}"
            );
        }
        let output = terse_with_raw(&[
            (false, "let l1 = 4;\nconsole.log(("),
            (true, "(() => { if (x) /a/.test(y); return ("),
            (false, "l1"),
            (true, " + 2) / 2; })()"),
            (false, "));\n"),
        ]);
        assert_eq!(
            output,
            "let a=4;console.log((()=>{if(x)/a/.test(y);return(a+2)/2;})());\n"
        );
    }

    /// A token that crosses between raw text and a hole leaves the program in
    /// its layout, with prefixed names, as the hygiene pass writes it.
    #[test]
    fn a_hole_inside_raw_strings_or_comments_keeps_the_hygienic_layout() {
        let output = terse_with_raw(&[
            (false, "let l1 = 1;\nconsole.log("),
            (true, "'"),
            (false, "l1"),
            (true, "'"),
            (false, ", l1);\n"),
        ]);
        assert_eq!(
            output,
            "let __light0_a = 1;\nconsole.log('__light0_a', __light0_a);\n"
        );
        let output = terse_with_raw(&[
            (false, "let l1 = 1;\n"),
            (true, "/* __light0_ "),
            (false, "l1"),
            (true, " */"),
            (false, "console.log(l1);\n"),
        ]);
        assert_eq!(
            output,
            "let __light1_a = 1;\n/* __light0_ __light1_a */console.log(__light1_a);\n"
        );
    }

    /// An emitted program whose functions declare locals in turn: siblings,
    /// closures over outer locals, a loop whose closures capture its
    /// variable, mutually recursive locals (`letfn`), a named function
    /// expression, and a raw `js*` local.
    const SCOPED: &str = "let g1 = 1;
let g2 = function (l1, l2) { return l1 + l2 + g1; };
let g3 = function (l3, l4) { return l5 => l3 + l4 + l5 + g2(g1, l5); };
let g4 = function (l6) {
  const l7 = [];
  for (let l8 = 0; l8 < l6; l8++) { l7.push(() => l8 + l6) }
  return l7;
};
let g5 = function self1(l9) {
  const l10 = l11 => l11 > 0 ? l12(l11 - 1) : l9;
  const l12 = l13 => l10(l13);
  return l10(l9) + self1.length;
};
let g6 = function (l14) { let l15 = l14 + 1; return [l15, g5(l15)]; };
console.log(g2(1, 2), g3(1, 2)(3), g4(2).length, g5(3), g6(1));
";

    /// light-cljs-419: locals get their names per scope, so sibling
    /// functions reuse the same short names, and a local avoids exactly the
    /// names that occur inside its scope and resolve outside it: the globals
    /// a function reads (`g1` in `g2`'s body) and the outer locals its inner
    /// functions read (`l3` in the arrow of `g3`). Globals take the table's
    /// names, which no longer compete with locals.
    #[test]
    fn locals_are_named_per_scope_without_capture() {
        let output = terse_program(SCOPED, &[], None, None);
        same_bindings(SCOPED, &output).unwrap();
        assert_eq!(
            output,
            "let a=1,b=function(b,c){return b+c+a},c=function(c,d){return e=>c+d+e+b(a,e)},\
             d=function(a){const b=[];for(let c=0;c<a;c++){b.push(()=>c+a)}\nreturn b};\
             let e=function a(b){const c=a=>a>0?d(a-1):b,d=a=>c(a);return c(b)+a.length};\
             let f=function(a){let b=a+1;return[b,e(b)]};\
             console.log(b(1,2),c(1,2)(3),d(2).length,e(3),f(1));\n"
        );
        // A local of a function with raw text takes none of its identifiers,
        // and one that raw text spells keeps the table's name.
        let output = terse_with_raw(&[
            (false, "let g1 = function (l1, l2) { return "),
            (true, "a + b + l2 + "),
            (false, "l1"),
            (true, ""),
            (false, "; };\nconsole.log(g1(1, 2));\n"),
        ]);
        assert_eq!(
            output,
            "let c=function(c,d){return a+b+l2+c};console.log(c(1,2));\n"
        );
    }

    /// light-cljs-424: an operand local takes the table's name, the same in
    /// every function whatever else it binds, and the locals named per scope
    /// avoid it where it occurs, an inner function's own included.
    #[test]
    fn operand_locals_take_one_name_in_every_function() {
        const OPERANDS: &str = "let g1 = function (l1) { let operand0; return operand0 = l1.x, operand0 ? operand0 : 1; };
let g2 = function (l2, l3, l4) { let operand0; return l4 + (operand0 = l2.x, operand0 ? operand0 : l3); };
let g3 = function (l5) {
  let operand0, operand1;
  return operand1 = function (l6) { let operand0; return operand0 = l6 + l5, operand0 ? operand0 : 2; }, operand0 = l5, operand1(operand0);
};
console.log(g1({}), g2({x: 2}, 3, 4), g3(5));
";
        let output = terse_program(OPERANDS, &[], None, None);
        same_bindings(OPERANDS, &output).unwrap();
        assert_eq!(
            output,
            "let a=function(a){let b;return b=a.x,b?b:1};let c=function(a,c,d){let b;return d+(b=a.x,b?b:c)};\
             let d=function(a){let b,e;return e=function(c){let b;return b=c+a,b?b:2},b=a,e(b)};\
             console.log(a({}),c({x:2},3,4),d(5));\n"
        );
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

    /// Units at module level (light-cljs-303) named by the one table of
    /// [`terse_texts`], beside a program text that reads some of their
    /// bindings and declares compiler names of its own: wrapped in the
    /// single file's factory, they are the readable slice renamed without
    /// capture, a program read has its binding's name, a binding that keeps
    /// its spelling keeps it, and no program name is a binding's.
    #[test]
    fn module_level_units_rename_without_capture() {
        let manifest = RuntimeManifest::embedded();
        let mut state = 0x9e37_79b9_u64;
        for density in [3, 10, 40, 100] {
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
                .take(4)
                .collect::<Vec<_>>();
            let program = returned
                .iter()
                .enumerate()
                .map(|(local, binding)| format!("const l{local} = rt${binding};\n"))
                .collect::<String>()
                + "let g0 = (a) => a;\n";
            let texts = [
                Text {
                    source: "",
                    raw: Vec::new(),
                    runtime: None,
                    units: Some(&kept),
                },
                Text {
                    source: &program,
                    raw: Vec::new(),
                    runtime: None,
                    units: None,
                },
            ];
            let (outputs, names) = terse_texts(&texts);
            let (units, bytes) = terse_module_units(&kept, |word| names[word].as_str());
            assert_eq!(
                bytes.iter().sum::<usize>(),
                units.len(),
                "density {density}"
            );
            let mut wrapped = String::from(TERSE_PROLOGUE);
            wrapped.push_str(&units);
            wrapped.push_str("return[");
            let read = returned
                .iter()
                .map(|binding| names[format!("rt${binding}").as_str()].as_str())
                .collect::<Vec<_>>();
            wrapped.push_str(&read.join(","));
            wrapped.push_str("]})()");
            let original = manifest.sliced_bindings_text(&kept, &returned);
            same_bindings(&original, &wrapped)
                .unwrap_or_else(|error| panic!("density {density}: {error}"));
            for (binding, name) in returned.iter().zip(&read) {
                assert_eq!(keeps_name(binding), name == binding, "{binding} is {name}");
                assert!(
                    [",", ";"]
                        .iter()
                        .any(|end| outputs[1].contains(&format!("={name}{end}"))),
                    "{}",
                    outputs[1]
                );
            }
            let bindings = names
                .iter()
                .filter(|(word, _)| word.starts_with("rt$"))
                .map(|(_, name)| name.as_str())
                .collect::<HashSet<_>>();
            for word in ["l0", "g0"] {
                assert!(!bindings.contains(names[word].as_str()), "{word}");
            }
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
                let token = &tokens[tokens.partition_point(|token| (token.start as usize) < at)];
                assert_eq!(token.start as usize, at);
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

    #[test]
    fn compiler_name_dispatch_matches_reference() {
        fn reference(word: &str) -> bool {
            const NUMBERED: [&str; 17] = [
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
                "receiver",
                "__lightUnitReify",
            ];
            let numbered = NUMBERED.iter().any(|head| {
                word.strip_prefix(head).is_some_and(|tail| {
                    !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit() || b == b'_')
                })
            });
            numbered
                || word
                    .strip_prefix('g')
                    .and_then(|tail| tail.strip_suffix('$'))
                    .is_some_and(|number| {
                        !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())
                    })
                || matches!(
                    word,
                    "rt" | "__lightSessionRuntime"
                        | "truth"
                        | "host"
                        | "pkg"
                        | "f"
                        | "text"
                        | "savedPrint"
                        | "savedNewline"
                        | "caught"
                        | "caseValue"
                        | "lazy$"
                )
                || word.starts_with("rt$")
                || word.starts_with("core$")
                || word.starts_with("nativeCore$")
                || word.starts_with("nativeCoreGetter$")
        }

        let stems = [
            "",
            "l",
            "g",
            "k",
            "c",
            "m",
            "a",
            "r",
            "f",
            "o",
            "t",
            "s",
            "n",
            "i",
            "h",
            "p",
            "_",
            "next",
            "saved",
            "temporary",
            "captures",
            "self",
            "recur",
            "installed",
            "operand",
            "receiver",
            "__lightUnitReify",
            "__lightSessionRuntime",
            "rt",
            "rt$",
            "core$",
            "nativeCore$",
            "nativeCoreGetter$",
            "truth",
            "host",
            "pkg",
            "text",
            "savedPrint",
            "savedNewline",
            "caught",
            "caseValue",
            "lazy$",
            "return",
            "const",
            "let",
            "function",
            "length",
            "prototype",
            "call",
            "apply",
            "Object",
            "Array",
            "x",
            "z",
            "é",
        ];
        let suffixes = [
            "", "0", "12", "_", "0_1", "_0", "$", "0$", "12$", "0_1$", "a", "0a", "x", "$x",
        ];
        for stem in stems {
            for suffix in suffixes {
                let candidate = format!("{stem}{suffix}");
                assert_eq!(
                    compiler_name(&candidate),
                    reference(&candidate),
                    "mismatch for {candidate:?}"
                );
            }
        }
    }

    #[test]
    fn constrained_scope_matches_reference_and_scales_linearly() {
        fn reference<'a, F: Fn(&str) -> bool>(
            order: &[usize],
            avoided: &HashSet<&str>,
            candidates: &mut Candidates<F>,
            own_of: impl Fn(usize) -> Option<&'a HashSet<&'a str>>,
            first_of: impl Fn(usize) -> u32,
        ) -> Vec<(usize, String)> {
            let mut taken: Vec<bool> = Vec::new();
            let mut chosen = Vec::with_capacity(order.len());
            for &local in order {
                let own = own_of(local);
                let mut at = 0;
                loop {
                    if taken.len() <= at {
                        taken.resize(at + 1, false);
                    }
                    let name = candidates.get(at);
                    if !taken[at]
                        && !avoided.contains(name)
                        && !own.is_some_and(|own| own.contains(name))
                    {
                        break;
                    }
                    at += 1;
                }
                taken[at] = true;
                chosen.push(at);
            }
            let mut batches: Vec<Vec<usize>> = Vec::new();
            for (position, &at) in chosen.iter().enumerate() {
                let length = candidates.get(at).len();
                if batches.len() <= length {
                    batches.resize_with(length + 1, Vec::new);
                }
                batches[length].push(position);
            }
            for batch in batches {
                let mut slots = batch.iter().map(|&p| chosen[p]).collect::<Vec<_>>();
                slots.sort_unstable();
                let mut positions = batch;
                positions.sort_by_key(|&p| first_of(order[p]));
                let mut used = vec![false; slots.len()];
                let mut assigned = Vec::with_capacity(positions.len());
                for &position in &positions {
                    let own = own_of(order[position]);
                    let found = (0..slots.len()).find(|&index| {
                        !used[index]
                            && !own.is_some_and(|own| own.contains(candidates.get(slots[index])))
                    });
                    let Some(index) = found else {
                        assigned.clear();
                        break;
                    };
                    used[index] = true;
                    assigned.push((position, slots[index]));
                }
                for (position, slot) in assigned {
                    chosen[position] = slot;
                }
            }
            order
                .iter()
                .zip(chosen)
                .map(|(&local, at)| (local, candidates.get(at).to_owned()))
                .collect()
        }

        // Exact equivalence on tricky mixed constraints, including batch reorder
        // and batch fallback when a constrained local runs out of eligible slots.
        let order: Vec<usize> = (0..120).collect();
        let first: Vec<u32> = (0..120).map(|i| ((i * 37) % 120) as u32).collect();
        let avoided: HashSet<&str> = ["a", "c", "aa"].into_iter().collect();
        let own_a: HashSet<&str> = ["b", "d"].into_iter().collect();
        let own_b: HashSet<&str> = ["b", "e", "ab"].into_iter().collect();
        let mut own_map: HashMap<usize, &HashSet<&str>> = HashMap::new();
        for local in [0, 1, 5, 10, 50, 51, 52] {
            own_map.insert(local, if local % 2 == 0 { &own_a } else { &own_b });
        }
        let mut c1 = Candidates::new(|_| false);
        let mut c2 = Candidates::new(|_| false);
        let actual = constrained_scope(
            &order,
            &avoided,
            &mut c1,
            |local| own_map.get(&local).copied(),
            |local| first[local],
        );
        let expected = reference(
            &order,
            &avoided,
            &mut c2,
            |local| own_map.get(&local).copied(),
            |local| first[local],
        );
        assert_eq!(actual, expected);

        // Large scope with slot 0 (`"a"`) in `avoided`: previously O(N^2).
        let n = 10_000usize;
        let big_order: Vec<usize> = (0..n).collect();
        let big_first: Vec<u32> = (0..n).map(|i| (n - i) as u32).collect();
        let big_avoided: HashSet<&str> = ["a", "b"].into_iter().collect();
        let big_own: HashSet<&str> = ["c", "d", "e"].into_iter().collect();
        let mut big_candidates = Candidates::new(|_| false);
        let assigned = constrained_scope(
            &big_order,
            &big_avoided,
            &mut big_candidates,
            |local| (local < 20).then_some(&big_own),
            |local| big_first[local],
        );
        assert_eq!(assigned.len(), n);
        let unique: HashSet<&str> = assigned.iter().map(|(_, name)| name.as_str()).collect();
        assert_eq!(unique.len(), n);
        assert!(!unique.contains("a") && !unique.contains("b"));
        for &(local, ref name) in &assigned {
            if local < 20 {
                assert!(!big_own.contains(name.as_str()), "local {local} got {name}");
            }
        }
    }

    #[test]
    fn raw_tokens_matches_full_scanned_names_on_edge_cases() {
        fn reference<'s>(
            source: &'s str,
            tokens: &[Token],
            partners: &[u32],
            raw: &[Range<usize>],
            runtime: Option<&Range<usize>>,
        ) -> Option<RawTokens<'s>> {
            let mut in_raw = vec![false; tokens.len()];
            if raw.is_empty() {
                return Some((in_raw, Vec::new()));
            }
            let mut ranges = raw.iter().peekable();
            for (index, token) in tokens.iter().enumerate() {
                while ranges
                    .next_if(|range| range.end <= token.start as usize)
                    .is_some()
                {}
                match ranges.peek() {
                    Some(range) if range.start <= token.start as usize => {
                        if token.end as usize > range.end {
                            return None;
                        }
                        in_raw[index] = true;
                    }
                    Some(range) if range.start < token.end as usize => return None,
                    _ => {}
                }
            }
            let non_ascii = |text: &str| text.bytes().filter(|byte| !byte.is_ascii()).count();
            let in_strings = tokens
                .iter()
                .zip(&in_raw)
                .filter(|(token, in_raw)| **in_raw && token.kind == Kind::Str)
                .map(|(token, _)| non_ascii(token.text(source)))
                .sum::<usize>();
            if raw
                .iter()
                .map(|range| non_ascii(&source[range.clone()]))
                .sum::<usize>()
                != in_strings
            {
                return None;
            }
            let adjacent = |index: usize, texts: &[&str]| {
                texts.iter().enumerate().all(|(offset, text)| {
                    tokens.get(index + offset).is_some_and(|token| {
                        token.kind == Kind::Punct
                            && token.text(source).starts_with(*text)
                            && (offset == 0 || tokens[index + offset - 1].end == token.start)
                    })
                })
            };
            for (index, token) in tokens.iter().enumerate() {
                if !(in_raw[index] || (index > 0 && in_raw[index - 1])) {
                    continue;
                }
                if in_raw[index]
                    && (adjacent(index, &["<", "!", "--"]) || adjacent(index, &["--", ">"]))
                {
                    return None;
                }
                let text = token.text(source);
                let slash = token.kind == Kind::Regex
                    || (token.kind == Kind::Punct && text.starts_with('/'));
                if slash
                    && slash_starts_regex(source, tokens, partners, index)
                        != Some(token.kind == Kind::Regex)
                {
                    return None;
                }
            }
            let mut skipped = raw.to_vec();
            skipped.extend(runtime.cloned());
            let scanned = scanned_names(source, &skipped);
            let named = tokens
                .iter()
                .zip(&in_raw)
                .filter(|(token, in_raw)| {
                    !**in_raw && token.kind == Kind::Name && compiler_name(token.text(source))
                })
                .map(|(token, _)| token.start as usize..token.end as usize);
            if !named.eq(scanned.into_iter().map(|(range, _)| range)) {
                return None;
            }
            let bytes = source.as_bytes();
            let in_raw_at = |at: usize| {
                raw.get(raw.partition_point(|range| range.end <= at))
                    .is_some_and(|range| range.start <= at)
            };
            let mut comments = Vec::new();
            let mut gap_start = 0;
            for index in 0..=tokens.len() {
                let gap_end = tokens
                    .get(index)
                    .map_or(source.len(), |token| token.start as usize);
                let mut newline = false;
                let mut at = gap_start;
                while at < gap_end {
                    let end = match (bytes[at], bytes.get(at + 1)) {
                        (b'\n' | b'\r', _) => {
                            newline = true;
                            at += 1;
                            continue;
                        }
                        (b'/', Some(b'/')) => source[at..gap_end]
                            .find(['\n', '\r'])
                            .map_or(gap_end, |length| at + length),
                        (b'/', Some(b'*')) => source[at + 2..gap_end]
                            .find("*/")
                            .map_or(gap_end, |length| at + length + 4),
                        _ => {
                            at += 1;
                            continue;
                        }
                    };
                    let text = &source[at..end];
                    if in_raw_at(at) {
                        comments.push((index, text, newline));
                    }
                    newline |= text.contains(['\n', '\r']);
                    at = end;
                }
                if let Some(token) = tokens.get(index) {
                    gap_start = token.end as usize;
                }
            }
            Some((in_raw, comments))
        }

        let cases: &[&[(bool, &str)]] = &[
            // Normal emitted JS + raw comment + trailing raw comment
            &[
                (false, "let l1 = \"l2\\\"l3\";\nlet l2 = 1e-5 + 0xff + "),
                (true, "/* c */\n// line\nfoo(1)"),
                (false, ";\n"),
                (true, "// eof"),
            ],
            // Hole inside raw line comment
            &[
                (false, "let l1 = 1;\n"),
                (true, "// note "),
                (false, "l1"),
                (false, "\nconsole.log(l1);\n"),
            ],
            // Hole inside raw block comment
            &[
                (false, "let l1 = 1;\n"),
                (true, "/* "),
                (false, "l1"),
                (true, " */"),
                (false, "console.log(l1);\n"),
            ],
            // Hole inside raw block comment with non-compiler word
            &[
                (false, "let l1 = 1;\n"),
                (true, "/* "),
                (false, "hello"),
                (true, " */"),
                (false, "console.log(l1);\n"),
            ],
            // Non-raw comment containing compiler name
            &[
                (false, "let l1 = 1; /* l1 */ "),
                (true, "foo"),
                (false, ";\n"),
            ],
            // Non-raw comment without compiler name
            &[
                (false, "let l1 = 1; /* note */ "),
                (true, "foo"),
                (false, ";\n"),
            ],
            // Non-raw single-quoted string with compiler name vs without vs containing quote
            &[(false, "let l1 = 'l1'; "), (true, "foo"), (false, ";\n")],
            &[(false, "let l1 = 'ok'; "), (true, "foo"), (false, ";\n")],
            &[
                (false, "let l1 = '\"'; let l2 = '\"'; "),
                (true, "foo"),
                (false, ";\n"),
            ],
            // Non-raw number edge cases (`0a0` contains compiler name `a0`, `1e$a` touches `$`)
            &[(false, "let l1 = 0a0; "), (true, "foo"), (false, ";\n")],
            &[(false, "let l1 = 1e$a; "), (true, "foo"), (false, ";\n")],
            // Non-raw non-ASCII identifier containing compiler name vs harmless
            &[(false, "let éa0 = 1; "), (true, "foo"), (false, ";\n")],
            &[(false, "let éfoo = 1; "), (true, "foo"), (false, ";\n")],
            // Non-raw regex containing compiler name vs harmless
            &[(false, "let l1 = /l1/; "), (true, "foo"), (false, ";\n")],
            &[(false, "let l1 = /abc/; "), (true, "foo"), (false, ";\n")],
        ];

        for pieces in cases {
            let mut source = String::new();
            let mut raw = Vec::new();
            for &(is_raw, text) in *pieces {
                if is_raw && !text.is_empty() {
                    raw.push(source.len()..source.len() + text.len());
                }
                source.push_str(text);
            }
            let tokens = lex_section(&source, 0..source.len(), false).unwrap();
            let p = partners(&source, &tokens);
            let actual = raw_tokens(&source, &tokens, &p, &raw, None);
            let expected = reference(&source, &tokens, &p, &raw, None);
            assert_eq!(actual, expected, "mismatch for source:\n{source}");
        }
    }

    #[test]
    fn terse_program_nested_without_marks_matches_with_marks() {
        let manifest = RuntimeManifest::embedded();
        let kept = manifest
            .units
            .iter()
            .map(|unit| unit.declares.contains(&"keyword") || unit.name == "keyword")
            .collect::<Vec<_>>();
        let (factory_text, _) = terse_slice(&kept, &["keyword"]);
        let prefix = "(() => {\nconst [rt$keyword] = ";
        let factory_start = prefix.len();
        let factory_end = factory_start + factory_text.len();
        let stmt_start = "(() => {\n".len();
        let stmt_end = factory_end + 1;
        let source = format!(
            "{prefix}{factory_text};\nconst k0 = rt$keyword(\"a\");\nconsole.log(k0);\n}})();\n"
        );
        let factory = factory_start..factory_end;
        let statement = stmt_start..stmt_end;

        let (no_marks, bytes_no_marks) = terse_program_nested(
            &source,
            std::slice::from_ref(&factory),
            statement.clone(),
            factory.clone(),
            &kept,
            &["keyword"],
            None,
        )
        .expect("nested terse succeeds");

        let mut marks = crate::source_map::Marks::default();
        marks.position(stmt_end + 1, 0, 10);
        let (with_marks, bytes_with_marks) = terse_program_nested(
            &source,
            std::slice::from_ref(&factory),
            statement,
            factory.clone(),
            &kept,
            &["keyword"],
            Some(&mut marks),
        )
        .expect("nested terse with marks succeeds");

        assert_eq!(no_marks, with_marks);
        assert_eq!(bytes_no_marks, bytes_with_marks);
        assert!(!marks.list.is_empty());
    }

    #[test]
    fn terse_texts_with_slots_matches_single_slot() {
        let s0 = "export const g0 = 1;\nexport function g1(l0) { return g0 + l0; }\n";
        let s1 = "import { g0, g1 } from './a.js';\nexport const g2 = g1(g0) + 2;\n";
        let s2 = "import { g2 } from './b.js';\nconsole.log(g2);\n";
        let make_texts = || {
            vec![
                Text {
                    source: s0,
                    raw: Vec::new(),
                    runtime: None,
                    units: None,
                },
                Text {
                    source: s1,
                    raw: Vec::new(),
                    runtime: None,
                    units: None,
                },
                Text {
                    source: s2,
                    raw: Vec::new(),
                    runtime: None,
                    units: None,
                },
            ]
        };
        let (out1, names1) = terse_texts_with_slots(&make_texts(), crate::scheduler::Slots::ONE);
        let (out4, names4) =
            terse_texts_with_slots(&make_texts(), crate::scheduler::Slots::new(4).unwrap());
        assert_eq!(out1, out4);
        assert_eq!(names1, names4);

        // Hygienic fallback path (unparseable raw hole).
        let raw_src = "export const g0 = \"/*raw*/\" + 1;\n";
        let raw_range = 19..24; // inside string literal -> triggers hygienic fallback
        let make_fallback = || {
            vec![
                Text {
                    source: raw_src,
                    raw: vec![raw_range.clone()],
                    runtime: None,
                    units: None,
                },
                Text {
                    source: s1,
                    raw: Vec::new(),
                    runtime: None,
                    units: None,
                },
            ]
        };
        let (fb1, fbn1) = terse_texts_with_slots(&make_fallback(), crate::scheduler::Slots::ONE);
        let (fb4, fbn4) =
            terse_texts_with_slots(&make_fallback(), crate::scheduler::Slots::new(4).unwrap());
        assert_eq!(fb1, fb4);
        assert_eq!(fbn1, fbn4);
    }

    #[test]
    fn terse_program_slices_matches_single_slot_and_canonicalizes_constants() {
        let manifest = RuntimeManifest::embedded();
        let kept = manifest
            .units
            .iter()
            .map(|unit| {
                unit.declares.contains(&"keyword")
                    || unit.declares.contains(&"Atom")
                    || unit.name == "keyword"
                    || unit.name == "Atom"
            })
            .collect::<Vec<_>>();
        let canon_k_names = vec!["k0".to_owned(), "k1".to_owned()];
        let header = "const k0 = rt$keyword(\"a\");\nconst k1 = rt$keyword(\"b\");\nvar g0 = 0;\n";
        let slice0 = "var g1 = function(l0) { var l1 = l0 + 1; return [k0_0, rt$Atom, l1, obj.p_alpha$LKEY]; };\n";
        let slice1 = "var g2 = g1(g0);\nconsole.log(g2, k1_0, k1_1, obj.p_alpha$LKEY);\n";
        let remap0 = [0usize];
        let remap1 = [0usize, 1usize];

        let run_case = |slots: crate::scheduler::Slots, can_nest: bool, mapped: bool| {
            let mut m0 = crate::source_map::Marks::default();
            m0.position(0, 0, 1);
            let mut m1 = crate::source_map::Marks::default();
            m1.position(0, 0, 2);
            terse_program_slices(
                vec![
                    TerseSlice {
                        source: header,
                        raw: &[],
                        marks: None,
                        k_prefix: None,
                        remap: &[],
                    },
                    TerseSlice {
                        source: slice0,
                        raw: &[],
                        marks: mapped.then_some(m0),
                        k_prefix: Some("k0_"),
                        remap: &remap0,
                    },
                    TerseSlice {
                        source: slice1,
                        raw: &[],
                        marks: mapped.then_some(m1),
                        k_prefix: Some("k1_"),
                        remap: &remap1,
                    },
                ],
                &canon_k_names,
                Some(ParallelRuntime {
                    kept: &kept,
                    bindings: &["keyword", "Atom"],
                    can_nest,
                }),
                "",
                true,
                mapped,
                true,
                slots,
            )
        };

        let out1 = run_case(crate::scheduler::Slots::ONE, true, false);
        let out4 = run_case(crate::scheduler::Slots::new(4).unwrap(), true, false);
        assert_eq!(out1.pieces, out4.pieces);
        assert_eq!(out1.total_len, out4.total_len);
        assert_eq!(out1.runtime_bytes, out4.runtime_bytes);
        assert_eq!(out1.unit_bytes, out4.unit_bytes);
        let joined: String = out1.pieces.concat();
        assert_eq!(joined.len(), out1.total_len);
        assert!(joined.starts_with("export default (()=>{"));
        assert!(joined.ends_with("})()})();\n"));
        // Canonicalized constants must never leave slice-local `k0_0`/`k1_0`/`k1_1`.
        assert!(!joined.contains("k0_0"));
        assert!(!joined.contains("k1_0"));
        assert!(!joined.contains("k1_1"));
        // Verify streaming RuntimeKey matches whole-text RuntimeKey.
        let key_whole = crate::runtime_key::RuntimeKey::release(
            [joined.as_str()],
            crate::OptimizationLevel::Advanced,
            None,
        )
        .shortened([joined.as_str()]);
        let key_pieces = crate::runtime_key::RuntimeKey::release_pieces(
            out1.pieces.iter().map(String::as_str),
            out1.total_len,
            crate::OptimizationLevel::Advanced,
            None,
        )
        .shortened_from_counts(out1.lkey_counts);
        assert_eq!(key_pieces.key(), key_whole.key());
        assert_eq!(key_pieces.short_names(), key_whole.short_names());
        assert_eq!(
            key_pieces.apply_pieces(out1.pieces, crate::scheduler::Slots::new(4).unwrap()),
            key_whole.apply(&joined)
        );

        // Mapped mode & non-nested factory fallback also match across slot counts.
        let mapped1 = run_case(crate::scheduler::Slots::ONE, true, true);
        let mapped4 = run_case(crate::scheduler::Slots::new(4).unwrap(), true, true);
        assert_eq!(mapped1.pieces, mapped4.pieces);
        assert!(!mapped1.marks.unwrap().list.is_empty());

        let factory1 = run_case(crate::scheduler::Slots::ONE, false, false);
        let factory4 = run_case(crate::scheduler::Slots::new(4).unwrap(), false, false);
        assert_eq!(factory1.pieces, factory4.pieces);
        let factory_joined: String = factory1.pieces.concat();
        assert!(factory_joined.starts_with("export default (()=>{const["));
        assert!(factory_joined.ends_with("})();\n"));

        // Hygienic fallback when a slice has raw text that `raw_tokens` cannot
        // follow (an emitted hole inside a raw `js*` string literal: `"~{}"`).
        let bad_raw = "var g1 = \"k0_0\" + k0_0;\n";
        let bad_ranges = [9..10, 14..15];
        let run_fallback = |slots: crate::scheduler::Slots| {
            let mut m = crate::source_map::Marks::default();
            m.position(0, 0, 5);
            terse_program_slices(
                vec![
                    TerseSlice {
                        source: header,
                        raw: &[],
                        marks: None,
                        k_prefix: None,
                        remap: &[],
                    },
                    TerseSlice {
                        source: bad_raw,
                        raw: &bad_ranges,
                        marks: Some(m),
                        k_prefix: Some("k0_"),
                        remap: &remap0,
                    },
                ],
                &canon_k_names,
                Some(ParallelRuntime {
                    kept: &kept,
                    bindings: &["keyword", "Atom"],
                    can_nest: true,
                }),
                "",
                true,
                true,
                true,
                slots,
            )
        };
        let fb1 = run_fallback(crate::scheduler::Slots::ONE);
        let fb4 = run_fallback(crate::scheduler::Slots::new(4).unwrap());
        assert_eq!(fb1.pieces, fb4.pieces);
        let fb_joined: String = fb1.pieces.concat();
        assert!(fb_joined.contains("__light0_"));
        assert!(!fb_joined.contains("k0_0"));
        assert!(!fb1.marks.unwrap().list.is_empty());
    }
}
