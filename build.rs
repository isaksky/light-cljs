//! Build-time inputs derived from source files:
//!
//! - The embedded runtime: `src/js_runtime.js` without its unit manifest header
//!   lines (lines whose first non-blank characters are `//@`; see
//!   `src/runtime_manifest.rs`). Output, the evaluator and the REPL embed the
//!   result, so the headers cost no emitted bytes.
//! - The terse runtime units (docs/advanced-optimization.md, Phase 3): each
//!   unit body without comments or whitespace and with its locals renamed to
//!   short names, which `Advanced` output embeds (see `src/terse.rs`). The
//!   lexer and scope analysis are `src/js_syntax.rs`, shared with the crate.
//! - The core operation index: `CoreFnId::ALL` and an exhaustive `match` from
//!   each `CoreFnId` to its row in `src/core_operations.tsv` (see
//!   `src/core_operations.rs`). A variant without a row fails to compile.

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    env,
    fmt::Write as _,
    fs,
    path::Path,
};

// The module allows the parts the crate uses and the build does not.
#[path = "src/js_syntax.rs"]
mod js_syntax;

use js_syntax::{Join, Kind, Shape, TerseWriter};

fn main() {
    let out = env::var_os("OUT_DIR").expect("OUT_DIR");
    let out = Path::new(&out);

    println!("cargo::rerun-if-changed=src/js_runtime.js");
    println!("cargo::rerun-if-changed=src/js_syntax.rs");
    let source = fs::read_to_string("src/js_runtime.js").expect("read src/js_runtime.js");
    let runtime = source
        .split_inclusive('\n')
        .filter(|line| !line.trim_start().starts_with("//@"))
        .collect::<String>();
    fs::write(out.join("js_runtime.js"), runtime).expect("write the embedded runtime");
    fs::write(out.join("terse_runtime.rs"), terse_runtime(&source))
        .expect("write the terse runtime");

    println!("cargo::rerun-if-changed=src/core_operations.tsv");
    let table =
        fs::read_to_string("src/core_operations.tsv").expect("read src/core_operations.tsv");
    fs::write(out.join("core_operations.rs"), core_operation_index(&table))
        .expect("write the core operation index");
}

/// The marker that opens a top-level binding's placeholder in a terse unit:
/// `\u{1}<binding index>\u{2}`, which the slice replaces with the binding's
/// name in that slice.
const OPEN: char = '\u{1}';
const CLOSE: char = '\u{2}';

/// Rust source for `TERSE_PROLOGUE`, `TERSE_BINDINGS`, `TERSE_KEPT` and
/// `TERSE_UNITS`: the runtime's prologue and every unit body (in manifest
/// order) written by `js_syntax::TerseWriter`, with locals renamed
/// (`Resolution::local_names`) and each top-level binding, declared or read,
/// as a placeholder; per unit, for each binding it names, the local names
/// visible at some occurrence, which that binding's name must avoid; and the
/// names that keep their spelling (`shows_name`). The manifest itself
/// (`src/runtime_manifest.rs`) parses the annotated source at run time; the
/// unit tests of `src/terse.rs` check that these units match it and rename
/// without capture.
fn terse_runtime(source: &str) -> String {
    assert!(
        !source.contains([OPEN, CLOSE]),
        "src/js_runtime.js must not contain U+0001 or U+0002"
    );
    // Section ranges: the prologue, then one per `//@unit` header; the
    // export section is not embedded by slices.
    let mut prologue_end = None;
    let mut units: Vec<std::ops::Range<usize>> = Vec::new();
    let mut offset = 0;
    let mut in_unit = false;
    for line in source.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let header = line.trim_start();
        if !header.starts_with("//@") {
            if in_unit {
                units.last_mut().unwrap().end = offset;
            }
            continue;
        }
        prologue_end.get_or_insert(start);
        if header.starts_with("//@unit") {
            units.push(offset..offset);
            in_unit = true;
        } else if header.trim_end() == "//@exports" {
            in_unit = false;
        } else if in_unit {
            // A continuation line of the header.
            units.last_mut().unwrap().start = offset;
            units.last_mut().unwrap().end = offset;
        }
    }
    let prologue_end = prologue_end.expect("the runtime has unit headers");
    let line_starts = std::iter::once(0)
        .chain(source.match_indices('\n').map(|(at, _)| at + 1))
        .collect::<Vec<_>>();
    let prologue = js_syntax::lex_section(source, 0..prologue_end, false)
        .unwrap_or_else(|error| panic!("runtime prologue: {}", error.message));
    let prologue = js_syntax::terse(source, &prologue);

    let parsed = units
        .iter()
        .map(|range| {
            let tokens = js_syntax::lex(source, range.clone()).unwrap_or_else(|error| {
                panic!("src/js_runtime.js byte {}: {}", error.offset, error.message)
            });
            let resolution =
                js_syntax::resolve(source, &tokens).unwrap_or_else(|(offset, message)| {
                    panic!("src/js_runtime.js byte {offset}: {message}")
                });
            (tokens, resolution)
        })
        .collect::<Vec<_>>();
    let bindings = parsed
        .iter()
        .flat_map(|(_, resolution)| {
            resolution
                .declarations
                .iter()
                .filter(|declaration| declaration.scope == 0)
                .map(|declaration| declaration.name)
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let index = bindings
        .iter()
        .enumerate()
        .map(|(index, &name)| (name, index))
        .collect::<HashMap<_, _>>();
    // Names that keep their spelling (`shows_name`), top-level or local.
    let kept = parsed
        .iter()
        .flat_map(|(tokens, resolution)| {
            resolution
                .declarations
                .iter()
                .filter(|declaration| shows_name(source, tokens, declaration))
                .map(|declaration| declaration.name)
        })
        .collect::<BTreeSet<_>>();

    let mut code =
        String::from("// Generated by build.rs from src/js_runtime.js; see src/terse.rs.\n\n");
    writeln!(
        code,
        "pub(crate) const TERSE_PROLOGUE: &str = {prologue:?};"
    )
    .unwrap();
    writeln!(
        code,
        "pub(crate) static TERSE_BINDINGS: [&str; {}] = {bindings:?};",
        bindings.len()
    )
    .unwrap();
    writeln!(
        code,
        "pub(crate) static TERSE_KEPT: [&str; {}] = {:?};",
        kept.len(),
        kept.iter().collect::<Vec<_>>()
    )
    .unwrap();
    writeln!(
        code,
        "pub(crate) static TERSE_UNITS: [TerseUnit; {}] = [",
        parsed.len()
    )
    .unwrap();
    for (tokens, resolution) in &parsed {
        let names = resolution.local_names(
            |declaration| shows_name(source, tokens, declaration),
            |name| js_syntax::unavailable(name) || kept.contains(name),
        );
        let partners = js_syntax::partners(source, tokens);
        let redundant = js_syntax::redundant_semicolons(source, tokens, &partners);
        let joins = js_syntax::joined_declarations(source, tokens, &resolution.statements);
        let mut writer = TerseWriter::default();
        let mut avoid: Vec<(usize, HashSet<String>)> = Vec::new();
        let mut lines = LineMarks::default();
        for (position, token) in tokens.iter().enumerate() {
            lines.token(&writer.out, &line_starts, token.start);
            match joins[position] {
                _ if redundant[position] => continue,
                Join::Drop => continue,
                Join::Comma => {
                    let comma = Shape {
                        kind: Kind::Punct,
                        text: ",",
                    };
                    writer.write(comma, ",", token.newline_before);
                    continue;
                }
                Join::Keep => {}
            }
            let text = token.text(source);
            let Some(occurrence) = resolution.occurrences[position] else {
                writer.token(source, token);
                continue;
            };
            let declaration = occurrence.declaration.map(|d| &resolution.declarations[d]);
            let binding = match declaration {
                Some(declaration) if declaration.scope == 0 && kept.contains(text) => None,
                Some(declaration) if declaration.scope == 0 => Some(index[text]),
                Some(_) => None,
                None => index.get(text).copied().filter(|_| !kept.contains(text)),
            };
            let written = match (binding, occurrence.declaration) {
                (Some(binding), _) => {
                    let visible = resolution.visible_locals(occurrence.scope, &names);
                    match avoid.iter_mut().find(|(b, _)| *b == binding) {
                        Some((_, set)) => set.extend(visible.into_iter().map(str::to_owned)),
                        None => {
                            avoid.push((binding, visible.into_iter().map(str::to_owned).collect()))
                        }
                    }
                    format!("{OPEN}{binding}{CLOSE}")
                }
                (None, Some(declaration)) => names[declaration]
                    .clone()
                    .unwrap_or_else(|| text.to_owned()),
                (None, None) => text.to_owned(),
            };
            let shape = Shape {
                kind: Kind::Name,
                text,
            };
            if occurrence.shorthand && written != text {
                // `{a}` becomes `{a:b}`.
                writer.write(shape, text, token.newline_before);
                writer.write(
                    Shape {
                        kind: Kind::Punct,
                        text: ":",
                    },
                    ":",
                    false,
                );
                writer.write(shape, &written, false);
            } else {
                writer.write(shape, &written, token.newline_before);
            }
        }
        lines.settle(&writer.out);
        // The next unit of a slice starts a statement.
        writer.end_statements();
        // A unit that starts with a `let` or `const` statement continues one
        // that the unit before it in a slice ends with.
        let keyword = |token: usize| {
            let text = tokens[token].text(source);
            matches!(text, "let" | "const").then_some(text)
        };
        let opens = resolution
            .statements
            .iter()
            .find(|(start, _)| *start == 0)
            .and_then(|(start, _)| keyword(*start));
        let closes = resolution
            .statements
            .iter()
            .find(|(_, end)| *end + 1 == tokens.len())
            .and_then(|(start, _)| keyword(*start));
        avoid.sort_by_key(|(binding, _)| *binding);
        let avoid = avoid
            .into_iter()
            .filter(|(_, names)| !names.is_empty())
            .map(|(binding, names)| {
                let mut names = names.into_iter().collect::<Vec<_>>();
                names.sort();
                format!("({binding}, {:?})", names.join(" "))
            })
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(
            code,
            "    TerseUnit {{ text: {:?}, avoid: &[{avoid}], opens: {opens:?}, closes: {closes:?}, lines: {:?} }},",
            writer.out, lines.encoded
        )
        .unwrap();
    }
    code.push_str("];\n");
    code
}

/// The source lines of a terse unit for its source map
/// (`TerseUnit::lines` in `src/terse.rs`): where in the unit's text the
/// first token of each source line starts, and that token's byte offset in
/// `src/js_runtime.js`, as pairs of Base64 VLQ deltas from the pair before.
#[derive(Default)]
struct LineMarks {
    encoded: String,
    line: Option<usize>,
    /// The first token of the current line, not yet written: the length of
    /// the text before it and its source offset.
    pending: Option<(usize, usize)>,
    previous: (usize, usize),
}

impl LineMarks {
    /// Note a token at byte `start` of the source, about to be written
    /// after `out`, the unit's text so far.
    fn token(&mut self, out: &str, line_starts: &[usize], start: usize) {
        self.settle(out);
        let line = line_starts.partition_point(|&at| at <= start);
        if self.line != Some(line) {
            self.line = Some(line);
            self.pending = Some((out.len(), start));
        }
    }

    /// Record the pending token once the writer has written it (a dropped
    /// token writes nothing and stays pending).
    fn settle(&mut self, out: &str) {
        let Some((before, start)) = self.pending.filter(|(before, _)| out.len() > *before) else {
            return;
        };
        self.pending = None;
        // The writer may separate the token by one space or line break.
        let at = before + usize::from(matches!(out.as_bytes()[before], b' ' | b'\n'));
        for (value, previous) in [(at, self.previous.0), (start, self.previous.1)] {
            vlq(&mut self.encoded, value as i64 - previous as i64);
        }
        self.previous = (at, start);
    }
}

/// Append `value` as a Base64 VLQ, as source maps write numbers.
fn vlq(out: &mut String, value: i64) {
    const DIGITS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut rest = (value.unsigned_abs() << 1) | u64::from(value < 0);
    loop {
        let digit = rest & 31;
        rest >>= 5;
        out.push(char::from(
            DIGITS[(digit | if rest > 0 { 32 } else { 0 }) as usize],
        ));
        if rest == 0 {
            break;
        }
    }
}

/// Whether a declaration keeps its name in terse units: a constructor, whose
/// `name` the host shows (Node's `console.log` prints an instance as
/// `KeywordValue {}`) and the printer writes (`#object[...]`). That is a
/// function or class declaration with an uppercase initial (`KeywordValue`,
/// `Atom`), or a local one bound to a function or class expression (the
/// `Type` of `makeType`). Protocol markers (`const IEquiv = function () {}`)
/// have no instances and are renamed.
fn shows_name(
    source: &str,
    tokens: &[js_syntax::Token],
    declaration: &js_syntax::Declaration,
) -> bool {
    let Some(token) = declaration.token else {
        return false;
    };
    let text = |index: usize| tokens.get(index).map(|token| token.text(source));
    declaration
        .name
        .starts_with(|c: char| c.is_ascii_uppercase())
        && (matches!(
            token.checked_sub(1).and_then(text),
            Some("function" | "class")
        ) || (declaration.scope != 0
            && text(token + 1) == Some("=")
            && matches!(text(token + 2), Some("function" | "class"))))
}

/// Rust source for `CoreFnId::ALL` and `CoreFnId::table_row`, in row order.
/// Only the first column (the variant) is read here; `src/core_operations.rs`
/// parses the rest.
fn core_operation_index(table: &str) -> String {
    let mut variants = Vec::new();
    let mut seen = HashSet::new();
    for (index, line) in table.lines().enumerate() {
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        let variant = line.split('\t').next().unwrap_or_default();
        let valid = variant.starts_with(|c: char| c.is_ascii_uppercase())
            && variant.chars().all(|c| c.is_ascii_alphanumeric());
        assert!(
            valid,
            "src/core_operations.tsv:{}: `{variant}` is not a CoreFnId variant name",
            index + 1
        );
        assert!(
            seen.insert(variant),
            "src/core_operations.tsv:{}: duplicate row for `{variant}`",
            index + 1
        );
        variants.push(variant);
    }
    let mut code = String::from(
        "// Generated by build.rs from src/core_operations.tsv. A `CoreFnId` variant\n\
         // missing from the table is a non-exhaustive match error below: add its row.\n\n\
         impl crate::analyzer::CoreFnId {\n",
    );
    writeln!(
        code,
        "    /// Every core operation, in the row order of `src/core_operations.tsv`.\n    \
         pub const ALL: [Self; {}] = [",
        variants.len()
    )
    .unwrap();
    for variant in &variants {
        writeln!(code, "        Self::{variant},").unwrap();
    }
    code.push_str(
        "    ];\n\n    /// Index of this operation's row among the table rows.\n    \
         pub(crate) const fn table_row(self) -> usize {\n        match self {\n",
    );
    for (row, variant) in variants.iter().enumerate() {
        writeln!(code, "            Self::{variant} => {row},").unwrap();
    }
    code.push_str("        }\n    }\n}\n");
    code
}
