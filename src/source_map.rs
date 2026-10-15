//! Source Map v3 (ECMA-426) for `--compile` output.
//!
//! Emission records marks ([`Marks`]): byte offsets of the output where the
//! text of a source expression starts, written as the emitter writes, moved
//! as later steps insert text before them, and carried through the terse
//! rewrite ([`crate::terse`]). Text after a nested expression maps to the
//! expression around it again, so each output position maps to the innermost
//! expression whose text it is. Macro-expanded code carries the span of the
//! macro call, so it maps to the call site. The embedded runtime maps to
//! `src/js_runtime.js` under its own source name ([`RUNTIME_SOURCE_URL`]):
//! each line of a readable runtime to its line, and each source line of a
//! terse unit to where its text starts in the unit (`build.rs`).
//!
//! [`SourceMapping`] is the finished table of an emitted output, which
//! [`SourceMapping::to_json`] writes against the program's source files.
//! Columns, generated and original, count UTF-16 code units.

use std::collections::HashMap;

use crate::runtime_manifest::ANNOTATED_RUNTIME;

/// The source index that marks text of `src/js_runtime.js`, beside the
/// program's own source indices.
pub(crate) const RUNTIME: usize = usize::MAX;

/// The source index of text that maps to no source ([`Marks::unmapped`]).
const UNMAPPED: usize = usize::MAX - 1;

/// The `sources` entry of `src/js_runtime.js`, resolved against the map's
/// location as every other entry is.
pub const RUNTIME_SOURCE_URL: &str = "light-cljs/js_runtime.js";

/// One mark: the output position where the text of `origin` starts.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Mark {
    /// Byte offset in the output.
    pub(crate) output: usize,
    /// The source index ([`RUNTIME`] for the runtime) and byte offset there,
    /// or `None` for a mark that only names the token at `output` and keeps
    /// the position mapped before it.
    pub(crate) origin: Option<(usize, usize)>,
    /// Index into the names.
    pub(crate) name: Option<u32>,
}

/// The marks of an output being emitted, with the names they use.
#[derive(Clone, Debug, Default)]
pub(crate) struct Marks {
    pub(crate) list: Vec<Mark>,
    names: Vec<String>,
    numbers: HashMap<String, u32>,
}

impl Marks {
    /// Text from `output` on is the text of byte `byte` of source `source`.
    pub(crate) fn position(&mut self, output: usize, source: usize, byte: usize) {
        self.list.push(Mark {
            output,
            origin: Some((source, byte)),
            name: None,
        });
    }

    /// The token at `output` has the original name `name`; `origin` is its
    /// position, or `None` to keep the position mapped before it.
    pub(crate) fn named(&mut self, output: usize, origin: Option<(usize, usize)>, name: &str) {
        let number = match self.numbers.get(name) {
            Some(&number) => number,
            None => {
                let number = self.names.len() as u32;
                self.names.push(name.to_owned());
                self.numbers.insert(name.to_owned(), number);
                number
            }
        };
        self.list.push(Mark {
            output,
            origin,
            name: Some(number),
        });
    }

    /// Text from `output` on is the compiler's own, such as the host
    /// helpers, and maps to no source. Any other mark at `output` wins.
    pub(crate) fn unmapped(&mut self, output: usize) {
        let mark = Mark {
            output,
            origin: Some((UNMAPPED, 0)),
            name: None,
        };
        self.list.insert(0, mark);
    }

    /// Text was inserted (`delta` > 0) or removed at `from`: marks from
    /// `from` on move by `delta`.
    pub(crate) fn shift(&mut self, from: usize, delta: isize) {
        for mark in &mut self.list[..] {
            if mark.output >= from {
                mark.output = mark.output.wrapping_add_signed(delta);
            }
        }
    }

    /// Marks recorded since `start`, removed: the marks of text written into
    /// a buffer other than the output.
    pub(crate) fn split_off(&mut self, start: usize) -> Vec<Mark> {
        self.list.split_off(start)
    }

    /// Re-add `marks` (from [`Marks::split_off`]) for their text placed at
    /// `at` in the output.
    pub(crate) fn place(&mut self, marks: &[Mark], at: usize) {
        self.list.extend(marks.iter().map(|mark| Mark {
            output: mark.output + at,
            ..*mark
        }));
    }

    /// Add the marks of `other`, an output placed at `at` in this one.
    pub(crate) fn absorb(&mut self, other: Marks, at: usize) {
        for mark in other.list {
            match mark.name {
                Some(name) => {
                    self.named(mark.output + at, mark.origin, &other.names[name as usize]);
                }
                None => self.list.push(Mark {
                    output: mark.output + at,
                    ..mark
                }),
            }
        }
    }

    /// Move every mark to the position `relocate` gives for it in rewritten
    /// output, dropping the marks it gives none.
    pub(crate) fn relocate(&mut self, relocate: impl Fn(usize) -> Option<usize>) {
        self.list.retain_mut(|mark| match relocate(mark.output) {
            Some(output) => {
                mark.output = output;
                true
            }
            None => false,
        });
    }

    /// Marks for text of `src/js_runtime.js` copied as is: `text`, written at
    /// output offset `at`, is the runtime source from byte `source` on. Each
    /// line maps to its first non-blank character.
    pub(crate) fn verbatim_runtime(&mut self, text: &str, at: usize, source: usize) {
        let mut offset = 0;
        for line in text.split_inclusive('\n') {
            let indent = line.len() - line.trim_start().len();
            if indent < line.len() {
                self.position(at + offset + indent, RUNTIME, source + offset + indent);
            }
            offset += line.len();
        }
    }

    /// Marks for the whole runtime as output embeds it,
    /// [`crate::runtime_manifest::RUNTIME`]`.trim()`, written at `at`: the
    /// annotated source without its header lines.
    pub(crate) fn whole_runtime(&mut self, at: usize) {
        let embedded = crate::runtime_manifest::RUNTIME;
        let skipped = embedded.len() - embedded.trim_start().len();
        let mut offset = 0usize;
        let mut source = 0;
        for line in ANNOTATED_RUNTIME.split_inclusive('\n') {
            let start = source;
            source += line.len();
            if crate::runtime_manifest::is_header_line(line) {
                continue;
            }
            let indent = line.len() - line.trim_start().len();
            if indent < line.len() && offset + indent >= skipped {
                self.position(at + offset + indent - skipped, RUNTIME, start + indent);
            }
            offset += line.len();
        }
    }

    /// Marks for a readable runtime slice
    /// ([`crate::runtime_manifest::RuntimeManifest::sliced_bindings_text`]),
    /// `text`, written at `at`: the prologue and the bodies of the units it
    /// keeps, in manifest order, each copied as is, then its `return`.
    pub(crate) fn readable_slice(&mut self, text: &str, at: usize) {
        let manifest = crate::runtime_manifest::RuntimeManifest::embedded();
        let sections = std::iter::once(manifest.prologue.clone())
            .chain(manifest.units.iter().map(|unit| unit.body_range.clone()));
        let mut offset = 0;
        for section in sections {
            let copied = &ANNOTATED_RUNTIME[section.clone()];
            // A unit the slice does not keep is not there; another unit's
            // body never starts with a kept one's.
            if !copied.is_empty() && text[offset..].starts_with(copied) {
                self.verbatim_runtime(copied, at + offset, section.start);
                offset += copied.len();
            }
        }
    }

    /// The finished table of `code`, the output these marks are positions of.
    /// A mapping applies only on its generated line, so each line that
    /// starts inside mapped text also starts with the origin mapped there.
    pub(crate) fn finish(mut self, code: &str) -> SourceMapping {
        self.list.sort_by_key(|mark| mark.output);
        self.list
            .retain(|mark| mark.output <= code.len() && code.is_char_boundary(mark.output));
        let mut segments: Vec<Segment> = Vec::new();
        let mut cursor = Cursor::default();
        let mut origin: Option<(usize, usize)> = None;
        let mut lines = Vec::new();
        let mut index = 0;
        while index <= self.list.len() {
            let output = self.list.get(index).map_or(code.len(), |mark| mark.output);
            let (line, column) = cursor.advance(code, output, &mut lines);
            if let Some((source, byte)) = origin.filter(|(source, _)| *source != UNMAPPED) {
                segments.extend(lines.drain(..).map(|line| Segment {
                    line,
                    column: 0,
                    source,
                    byte,
                    name: None,
                }));
            }
            lines.clear();
            if index == self.list.len() {
                break;
            }
            // Of the marks at one position, the last recorded is the
            // innermost expression's.
            let mut name = None;
            while index < self.list.len() && self.list[index].output == output {
                let mark = self.list[index];
                origin = mark.origin.or(origin);
                name = mark.name.or(name);
                index += 1;
            }
            let Some((source, byte)) = origin else {
                continue;
            };
            let name = name.filter(|_| source != UNMAPPED);
            if name.is_none()
                && segments.last().is_some_and(|last| {
                    last.line == line && (last.source, last.byte) == (source, byte)
                })
            {
                continue;
            }
            if let Some(last) = segments.last_mut()
                && (last.line, last.column) == (line, column)
            {
                *last = Segment {
                    line,
                    column,
                    source,
                    byte,
                    name,
                };
                continue;
            }
            segments.push(Segment {
                line,
                column,
                source,
                byte,
                name,
            });
        }
        SourceMapping {
            segments,
            names: self.names,
        }
    }
}

/// A walk over generated code that counts lines and UTF-16 columns.
#[derive(Default)]
struct Cursor {
    at: usize,
    line: u32,
    column: u32,
    /// The previous character was `\r`, which with a `\n` ends one line.
    carriage: bool,
}

impl Cursor {
    /// The line and column of `to`, adding the lines that start before it
    /// to `lines`.
    fn advance(&mut self, code: &str, to: usize, lines: &mut Vec<u32>) -> (u32, u32) {
        for ch in code[self.at..to].chars() {
            match ch {
                '\n' if self.carriage => {}
                '\n' | '\r' | '\u{2028}' | '\u{2029}' => {
                    self.line += 1;
                    self.column = 0;
                    lines.push(self.line);
                }
                _ => self.column += ch.len_utf16() as u32,
            }
            self.carriage = ch == '\r';
        }
        self.at = to;
        (self.line, self.column)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Segment {
    line: u32,
    column: u32,
    source: usize,
    byte: usize,
    name: Option<u32>,
}

/// The source map of an emitted output, before it names its sources: see
/// [`crate::PreparedApplication::emit_js_mapped`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceMapping {
    segments: Vec<Segment>,
    names: Vec<String>,
}

/// A program source as a source map names it.
#[derive(Clone, Copy, Debug)]
pub struct SourceFile<'a> {
    /// The `sources` entry: a URL, usually relative to the map's location.
    pub url: &'a str,
    /// The source text the program was analyzed from.
    pub text: &'a str,
}

impl SourceMapping {
    /// The number of mapped positions.
    pub fn len(&self) -> usize {
        self.segments.len()
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// The program source index and byte offset that the output position at
    /// zero-based `line` and `column` (UTF-16 code units) maps to: that of
    /// the last segment of the line at or before the column. None where the
    /// position maps to no program source.
    pub fn original(&self, line: u32, column: u32) -> Option<(usize, usize)> {
        self.segments
            .iter()
            .rev()
            .find(|segment| segment.line == line && segment.column <= column)
            .filter(|segment| segment.source != UNMAPPED && segment.source != RUNTIME)
            .map(|segment| (segment.source, segment.byte))
    }

    /// The Source Map v3 JSON. `file` is the output's name; `sources` are the
    /// program's sources by source index (the order a namespace program
    /// loaded them in; one for a single source), followed by the runtime when
    /// the output embeds it. With `include_sources`, `sourcesContent` holds
    /// every source's text.
    pub fn to_json(
        &self,
        file: Option<&str>,
        sources: &[SourceFile<'_>],
        include_sources: bool,
    ) -> String {
        let runtime = self
            .segments
            .iter()
            .any(|segment| segment.source == RUNTIME);
        let mut positions: Vec<Option<Positions>> = Vec::new();
        positions.resize_with(sources.len() + 1, || None);
        let mut mappings = String::new();
        let mut line = 0;
        let mut previous = [0i64; 5];
        let mut first = true;
        for segment in &self.segments {
            let mut fields = vec![i64::from(segment.column)];
            // A segment of one field, the column, maps to no source.
            if segment.source != UNMAPPED {
                let (index, text) = match segment.source {
                    RUNTIME => (sources.len(), ANNOTATED_RUNTIME),
                    source if source < sources.len() => (source, sources[source].text),
                    _ => continue,
                };
                if segment.byte > text.len() {
                    continue;
                }
                let mut byte = segment.byte;
                while !text.is_char_boundary(byte) {
                    byte -= 1;
                }
                let (original_line, original_column) = positions[index]
                    .get_or_insert_with(|| Positions::new(text))
                    .at(byte);
                fields.extend([index as i64, original_line as i64, original_column as i64]);
                fields.extend(segment.name.map(i64::from));
            }
            while line < segment.line {
                mappings.push(';');
                line += 1;
                previous[0] = 0;
                first = true;
            }
            if !first {
                mappings.push(',');
            }
            first = false;
            for (slot, value) in fields.into_iter().enumerate() {
                vlq(&mut mappings, value - previous[slot]);
                previous[slot] = value;
            }
        }
        // Node 24 reads a name for a last segment without one, at the end of
        // the text; a line break after it, an empty line, keeps it unnamed.
        if !first {
            mappings.push(';');
        }
        let mut json = String::from("{\"version\":3");
        if let Some(file) = file {
            json.push_str(",\"file\":");
            json_string(&mut json, file);
        }
        let urls = sources
            .iter()
            .map(|source| source.url)
            .chain(runtime.then_some(RUNTIME_SOURCE_URL));
        json.push_str(",\"sources\":[");
        for (index, url) in urls.enumerate() {
            if index > 0 {
                json.push(',');
            }
            json_string(&mut json, url);
        }
        json.push(']');
        if include_sources {
            json.push_str(",\"sourcesContent\":[");
            let texts = sources
                .iter()
                .map(|source| source.text)
                .chain(runtime.then_some(ANNOTATED_RUNTIME));
            for (index, text) in texts.enumerate() {
                if index > 0 {
                    json.push(',');
                }
                json_string(&mut json, text);
            }
            json.push(']');
        }
        json.push_str(",\"names\":[");
        for (index, name) in self.names.iter().enumerate() {
            if index > 0 {
                json.push(',');
            }
            json_string(&mut json, name);
        }
        json.push_str("],\"mappings\":");
        json_string(&mut json, &mappings);
        json.push_str("}\n");
        json
    }
}

/// One output put together from texts, some of them with the mapping of
/// their own text: a dev build unit made of the units of a namespace's forms
/// ([`crate::dev_build::namespace_unit`]).
#[derive(Default)]
pub(crate) struct MappedText {
    pub(crate) text: String,
    pub(crate) mapping: SourceMapping,
    /// The end of `text`.
    cursor: Cursor,
}

impl MappedText {
    /// Append `text`, which maps to no source of its own.
    pub(crate) fn push_str(&mut self, text: &str) {
        self.text.push_str(text);
        self.cursor
            .advance(&self.text, self.text.len(), &mut Vec::new());
    }

    /// Append `text`, whose own mapping is `mapping` with its sources at
    /// `sources`: its source index `i` is this output's `sources[i]`.
    pub(crate) fn push_mapped(&mut self, text: &str, mapping: &SourceMapping, sources: &[usize]) {
        self.push_mapped_lines(text, mapping, sources, 0);
    }

    /// [`Self::push_mapped`] of `text`, the lines of a text mapped by
    /// `mapping` from the start of its line `first`: the segments of those
    /// lines, and of no other.
    pub(crate) fn push_mapped_lines(
        &mut self,
        text: &str,
        mapping: &SourceMapping,
        sources: &[usize],
        first: u32,
    ) {
        let (line, column) = (self.cursor.line, self.cursor.column);
        // The line after `text`'s last line terminator.
        let last = first
            + Cursor::default()
                .advance(text, text.len(), &mut Vec::new())
                .0;
        let names = self.mapping.names.len() as u32;
        self.mapping.segments.extend(
            mapping
                .segments
                .iter()
                .filter(|segment| (first..=last).contains(&segment.line))
                .map(|segment| Segment {
                    line: segment.line - first + line,
                    column: segment.column + if segment.line == first { column } else { 0 },
                    source: match segment.source {
                        RUNTIME | UNMAPPED => segment.source,
                        source => sources.get(source).copied().unwrap_or(UNMAPPED),
                    },
                    byte: segment.byte,
                    name: segment.name.map(|name| name + names),
                }),
        );
        self.mapping.names.extend(mapping.names.iter().cloned());
        self.push_str(text);
    }
}

/// The zero-based line and UTF-16 column of each byte offset of a source.
struct Positions {
    lines: crate::LineIndex,
    /// The UTF-16 code units before each byte offset; `None` for ASCII text,
    /// where they are the offset.
    units: Option<Vec<u32>>,
}

impl Positions {
    fn new(text: &str) -> Self {
        let units = (!text.is_ascii()).then(|| {
            let mut units = Vec::with_capacity(text.len() + 1);
            let mut count = 0;
            for ch in text.chars() {
                units.extend(std::iter::repeat_n(count, ch.len_utf8()));
                count += ch.len_utf16() as u32;
            }
            units.push(count);
            units
        });
        Self {
            lines: crate::LineIndex::new(text),
            units,
        }
    }

    /// The position of `byte`, a character boundary of the text.
    fn at(&self, byte: usize) -> (usize, usize) {
        let (line, column) = self
            .lines
            .line_column(byte)
            .expect("a position in the source");
        let start = byte + 1 - column;
        let column = match &self.units {
            Some(units) => (units[byte] - units[start]) as usize,
            None => byte - start,
        };
        (line - 1, column)
    }
}

/// `json` as a `data:` URL, for a `sourceMappingURL` comment that embeds
/// the map.
pub fn data_url(json: &str) -> String {
    const DIGITS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut url = String::from("data:application/json;charset=utf-8;base64,");
    for chunk in json.as_bytes().chunks(3) {
        let bits = chunk.iter().enumerate().fold(0u32, |bits, (index, &byte)| {
            bits | u32::from(byte) << (16 - 8 * index)
        });
        for index in 0..4 {
            if index <= chunk.len() {
                url.push(char::from(DIGITS[(bits >> (18 - 6 * index)) as usize & 63]));
            } else {
                url.push('=');
            }
        }
    }
    url
}

/// Append `value` as a Base64 VLQ.
fn vlq(out: &mut String, value: i64) {
    const DIGITS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut rest = if value < 0 {
        (value.unsigned_abs() << 1) | 1
    } else {
        (value as u64) << 1
    };
    loop {
        let mut digit = rest & 31;
        rest >>= 5;
        if rest > 0 {
            digit |= 32;
        }
        out.push(char::from(DIGITS[digit as usize]));
        if rest == 0 {
            break;
        }
    }
}

/// Append `text` as a JSON string.
pub(crate) fn json_string(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 || matches!(ch, '\u{2028}' | '\u{2029}') => {
                out.push_str(&format!("\\u{:04x}", ch as u32));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vlq_matches_the_specification_examples() {
        let mut out = String::new();
        for value in [0, 1, -1, 15, 16, -16, 1000] {
            vlq(&mut out, value);
            out.push(' ');
        }
        assert_eq!(out, "A C D e gB hB w+B ");
    }

    #[test]
    fn data_url_is_base64() {
        assert_eq!(
            data_url("{}\n"),
            "data:application/json;charset=utf-8;base64,e30K"
        );
        assert_eq!(
            data_url("ab"),
            "data:application/json;charset=utf-8;base64,YWI="
        );
    }

    /// Each line of a readable runtime, whole or sliced, maps to the same
    /// text of `src/js_runtime.js`, except a slice's own `return`.
    #[test]
    fn readable_runtime_lines_map_to_their_text() {
        let manifest = crate::runtime_manifest::RuntimeManifest::embedded();
        let kept = (0..manifest.units.len())
            .map(|unit| unit % 2 == 0)
            .collect::<Vec<_>>();
        let slice = manifest.sliced_bindings_text(&kept, &[]);
        let mut whole = Marks::default();
        whole.whole_runtime(0);
        let mut sliced = Marks::default();
        sliced.readable_slice(&slice, 0);
        for (text, marks, own) in [
            (crate::runtime_manifest::RUNTIME.trim(), whole, 0),
            (slice.as_str(), sliced, 2),
        ] {
            let line = |text: &str, at: usize| text[at..].split('\n').next().unwrap().to_owned();
            for mark in &marks.list {
                let (source, byte) = mark.origin.unwrap();
                assert_eq!(source, RUNTIME);
                assert_eq!(line(text, mark.output), line(ANNOTATED_RUNTIME, byte));
            }
            let lines = text.lines().filter(|line| !line.trim().is_empty()).count();
            assert_eq!(marks.list.len() + own, lines);
        }
    }

    /// A line that starts inside mapped text starts with its origin; text
    /// after an unmapped mark maps to nothing on later lines too. Original
    /// columns count a supplementary character as two UTF-16 units.
    #[test]
    fn lines_continue_their_origin() {
        let mut marks = Marks::default();
        let code = "f(\n  x\n);\ng\nh";
        marks.position(0, 0, 5);
        marks.unmapped(9);
        let json = marks.finish(code).to_json(
            None,
            &[SourceFile {
                url: "in.cljs",
                text: "\u{1f600} (f x)",
            }],
            false,
        );
        assert!(
            json.ends_with("\"mappings\":\"AAAG;AAAA;AAAA,E;\"}\n"),
            "{json}"
        );
    }

    #[test]
    fn marks_map_lines_and_utf16_columns() {
        let mut marks = Marks::default();
        let code = "a\u{e9}b\nc;\nd";
        marks.position(0, 0, 0);
        marks.position(3, 0, 4);
        // The same origin again on the line adds no segment.
        marks.position(4, 0, 4);
        marks.named(5, None, "x");
        marks.unmapped(6);
        marks.position(8, 0, 11);
        let mapping = marks.finish(code);
        let source = "(a b)\n  (\u{e9} d)";
        let json = mapping.to_json(
            Some("out.js"),
            &[SourceFile {
                url: "in.cljs",
                text: source,
            }],
            false,
        );
        // Line 0: column 0 -> 0:0; column 2 (after the two-byte character,
        // one UTF-16 unit) -> 0:4. Line 1: column 0 (a name, at the last
        // origin) -> 0:4; column 1 -> no source. Line 2: column 0 -> 1:4
        // (byte 11, UTF-16 column 4). The last line ends for Node.
        assert_eq!(
            json,
            "{\"version\":3,\"file\":\"out.js\",\"sources\":[\"in.cljs\"],\"names\":[\"x\"],\"mappings\":\"AAAA,EAAI;AAAAA,C;AACA;\"}\n"
        );
    }
}
