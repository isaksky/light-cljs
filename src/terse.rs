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
    Declaration, Join, Kind, NO_SCOPE, NameGenerator, Resolution, Shape, TerseWriter, Token,
    closes_control_head, joined_declarations, lex_section, partners, redundant_semicolons,
    reserved, resolve,
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
    let numbered = |word: &str| {
        NUMBERED.iter().any(|head| {
            word.strip_prefix(head).is_some_and(|tail| {
                !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit() || b == b'_')
            })
        })
    };
    numbered(word)
        || word
            .strip_prefix('g')
            .and_then(|tail| tail.strip_suffix('$'))
            .is_some_and(|number| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
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
/// property, and that the caller admits.
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
        let declarations = &resolution.declarations;
        let mut named = declarations
            .iter()
            .map(|declaration| {
                declaration.scope != 0
                    && declaration.token.is_some()
                    && compiler_name(declaration.name)
                    && eligible(declaration.name)
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
            let word = tokens[index].text(source);
            if in_raw[index] || !compiler_name(word) {
                continue;
            }
            let local = occurrence
                .declaration
                .map_or(NO_LOCAL, |declaration| local_of[declaration]);
            let (key, target) = match local {
                NO_LOCAL => {
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

    fn get(&mut self, at: usize) -> &str {
        while self.names.len() <= at {
            let excluded = &self.excluded;
            let name = self
                .generator
                .find(|name| !excluded(name))
                .expect("names never run out");
            self.names.push(name);
        }
        &self.names[at]
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
    match rewritten(source, runtime.clone(), &raw) {
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
            debug_assert!(false, "emitted JavaScript does not lex:\n{source}");
            source.to_owned()
        }
    }
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
pub(crate) fn terse_texts<'s>(texts: &[Text<'s>]) -> (Vec<String>, HashMap<&'s str, String>) {
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
    let lexed = texts
        .iter()
        .zip(&raws)
        .map(|(text, raw)| Lexed::new(text.source, text.runtime.clone(), raw, eligible))
        .collect::<Option<Vec<_>>>();
    if let Some(lexed) = lexed {
        let mut words: Vec<&'s str> = Vec::new();
        let mut excluded = HashSet::new();
        for (text, lexed) in texts.iter().zip(&lexed) {
            if let Some(kept) = text.units {
                words.extend(unit_words(kept).map(|word| -> &'s str { word }));
            }
            lexed.words(&mut words, &mut excluded);
        }
        words.extend(kept_reads(module).map(|word| -> &'s str { word }));
        let names = constrained_short_names(
            words,
            |name| excluded.contains(name) || module_level(name),
            &avoid,
            fixed,
        );
        let mut candidates =
            Candidates::new(|name: &str| excluded.contains(name) || module_level(name));
        let locals = lexed
            .iter()
            .map(|text| text.locals.names(&names, &mut candidates))
            .collect::<Vec<_>>();
        let outputs = lexed
            .into_iter()
            .zip(&locals)
            .map(|(text, locals)| text.rewrite(&names, locals).0)
            .collect();
        return (outputs, names);
    }
    debug_assert!(
        raws.iter().any(|raw| !raw.is_empty()),
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
    let words = texts
        .iter()
        .zip(&skipped)
        .flat_map(|(text, skipped)| {
            text.units
                .into_iter()
                .flat_map(unit_words)
                .map(|word| -> &'s str { word })
                .chain(
                    scanned_names(text.source, skipped)
                        .into_iter()
                        .map(|(_, word)| word),
                )
        })
        .chain(kept_reads(module).map(|word| -> &'s str { word }))
        .collect::<Vec<_>>();
    let names = constrained_short_names(words, |_| false, &HashMap::new(), fixed)
        .into_iter()
        .map(|(word, name)| match fixed(word) {
            Some(_) => (word, name),
            None => (word, format!("{prefix}{name}")),
        })
        .collect::<HashMap<_, _>>();
    let outputs = texts
        .iter()
        .zip(&skipped)
        .map(|(text, skipped)| {
            rename_compiler_names(text.source, skipped, |word| names[word].clone()).0
        })
        .collect();
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

/// The program rewritten from its tokens, with the start of each written
/// token in `source` and in the output; `None` when it does not lex, or when
/// a token crosses a boundary of the `raw` ranges (sorted, disjoint `js*`
/// text) or the compiler names outside them are not the names
/// [`scanned_names`] finds there.
fn rewritten(
    source: &str,
    runtime: Option<Range<usize>>,
    raw: &[Range<usize>],
) -> Option<(String, Vec<(usize, usize)>)> {
    let lexed = Lexed::new(source, runtime, raw, |_| true)?;
    let mut words = Vec::new();
    let mut excluded = HashSet::new();
    lexed.words(&mut words, &mut excluded);
    let names = short_names(words, |name| excluded.contains(name));
    let locals = lexed
        .locals
        .names(&names, &mut Candidates::new(|name| excluded.contains(name)));
    Some(lexed.rewrite(&names, &locals))
}

/// The tokens of a text [`rewritten`] writes: which are raw `js*` text, the
/// comments of raw text it keeps, its locals named per scope ([`Locals`])
/// and the declarations that join.
struct Lexed<'s> {
    source: &'s str,
    runtime: Option<Range<usize>>,
    tokens: Vec<Token>,
    partners: Vec<usize>,
    in_raw: Vec<bool>,
    comments: Vec<(usize, &'s str, bool)>,
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
        let partners = partners(source, &tokens);
        let in_raw = raw_tokens(source, &tokens, &partners, raw, runtime.as_ref())?;
        let comments = raw_comments(source, &tokens, raw);
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

    /// The text rewritten with the short `names` of its compiler names and
    /// the names of its `locals` ([`Locals::names`]), with the start of each
    /// written token in the source and in the output.
    fn rewrite(
        self,
        names: &HashMap<&str, String>,
        locals: &[String],
    ) -> (String, Vec<(usize, usize)>) {
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
        rewrite_tokens(
            source, runtime, tokens, partners, in_raw, comments, joins, name,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn rewrite_tokens<'n>(
    source: &str,
    runtime: Option<Range<usize>>,
    tokens: Vec<Token>,
    partners: Vec<usize>,
    in_raw: Vec<bool>,
    comments: Vec<(usize, &str, bool)>,
    joins: Vec<Join>,
    name: impl Fn(usize, &str) -> &'n str,
) -> (String, Vec<(usize, usize)>) {
    let is_runtime = |token: &Token| runtime.as_ref().is_some_and(|r| r.start == token.start);
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
        ..
    } = rewrite;
    let mut writer = TerseWriter::default();
    let mut written = Vec::new();
    let mut comments = comments.into_iter().peekable();
    for (index, token) in tokens.iter().enumerate() {
        while let Some((_, text, newline)) = comments.next_if(|(at, ..)| *at == index) {
            writer.comment(text, newline);
        }
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
        } else if token.kind == Kind::Name && compiler_name(word) && !in_raw[index] {
            let shape = Shape {
                kind: Kind::Name,
                text: word,
            };
            writer.write(shape, name(index, word), newline);
        } else {
            let shape = Shape {
                kind: token.kind,
                text: word,
            };
            writer.write(shape, word, newline);
        }
        written.push((token.start, separated(&writer.out)));
    }
    for (_, text, newline) in comments {
        writer.comment(text, newline);
    }
    let mut output = writer.out;
    if !output.ends_with('\n') {
        output.push('\n');
    }
    (output, written)
}

/// The comments of raw `js*` text (sorted, disjoint `raw` ranges), which
/// terse output keeps: each with the index of the token it precedes
/// (`tokens.len()` after the last) and whether a line break precedes it
/// since the token before.
fn raw_comments<'s>(
    source: &'s str,
    tokens: &[Token],
    raw: &[Range<usize>],
) -> Vec<(usize, &'s str, bool)> {
    let mut comments = Vec::new();
    if raw.is_empty() {
        return comments;
    }
    let bytes = source.as_bytes();
    let in_raw = |at: usize| {
        raw.get(raw.partition_point(|range| range.end <= at))
            .is_some_and(|range| range.start <= at)
    };
    let mut gap_start = 0;
    for index in 0..=tokens.len() {
        // Between two tokens there is only whitespace and comments.
        let gap_end = tokens.get(index).map_or(source.len(), |token| token.start);
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
            if in_raw(at) {
                comments.push((index, text, newline));
            }
            newline |= text.contains(['\n', '\r']);
            at = end;
        }
        if let Some(token) = tokens.get(index) {
            gap_start = token.end;
        }
    }
    comments
}

/// Which `tokens` are raw `js*` text, inside one of the `raw` ranges (sorted
/// and disjoint, without the `runtime` factory); `None` when a token crosses
/// a boundary of one, or when the compiler names of the other tokens are
/// not those [`scanned_names`] finds outside the ranges, which the readable
/// output renames. Either happens only where raw text puts a hole inside
/// one of its strings, comments, templates or identifiers.
///
/// Also `None` where raw text may not lex as a JavaScript engine lexes it
/// (the lexer serves the runtime and emitted code): a byte outside ASCII
/// other than in a string literal (`a` and a no-break space would be one
/// name), an HTML-like comment (`<!--`, `-->`), and a `/` that the lexer
/// takes for a division or a regular expression where the tokens before it
/// do not tell which ([`slash_starts_regex`]).
fn raw_tokens(
    source: &str,
    tokens: &[Token],
    partners: &[usize],
    raw: &[Range<usize>],
    runtime: Option<&Range<usize>>,
) -> Option<Vec<bool>> {
    let mut in_raw = vec![false; tokens.len()];
    if raw.is_empty() {
        return Some(in_raw);
    }
    let mut ranges = raw.iter().peekable();
    for (index, token) in tokens.iter().enumerate() {
        while ranges.next_if(|range| range.end <= token.start).is_some() {}
        match ranges.peek() {
            Some(range) if range.start <= token.start => {
                if token.end > range.end {
                    return None;
                }
                in_raw[index] = true;
            }
            Some(range) if range.start < token.end => return None,
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
    for (index, token) in tokens.iter().enumerate() {
        let touches_raw = in_raw[index] || (index > 0 && in_raw[index - 1]);
        if !touches_raw {
            continue;
        }
        if in_raw[index] && (adjacent(index, &["<", "!", "--"]) || adjacent(index, &["--", ">"])) {
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
    let mut skipped = raw.to_vec();
    skipped.extend(runtime.cloned());
    let scanned = scanned_names(source, &skipped);
    let named = tokens
        .iter()
        .zip(&in_raw)
        .filter(|(token, in_raw)| {
            !**in_raw && token.kind == Kind::Name && compiler_name(token.text(source))
        })
        .map(|(token, _)| token.start..token.end);
    named
        .eq(scanned.into_iter().map(|(range, _)| range))
        .then_some(in_raw)
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
    partners: &[usize],
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
                    open != usize::MAX
                        && open > 0
                        && tokens[open - 1].kind == Kind::Name
                        && matches!(
                            tokens[open - 1].text(source),
                            "if" | "for" | "while" | "with"
                        )
                        && !member(open - 1),
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
            if self.punct(index) != "["
                || self.partners[index] != index + 2
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
            if self.punct(open) != "(" || close == usize::MAX || self.next(open) == Some(close) {
                continue;
            }
            // `((x))`: the inner pair, when its top level holds no comma.
            if let Some(inner) = self.next(open)
                && self.punct(inner) == "("
                && Some(self.partners[inner]) == self.previous(close)
                && self.next(inner) != Some(self.partners[inner])
                && !self.holds(inner, self.partners[inner], ",")
                && !self.raw[inner]
                && !self.raw[self.partners[inner]]
                && !self.commented[inner + 1]
            {
                self.unwrap(inner, self.partners[inner]);
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
