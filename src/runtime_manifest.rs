//! The runtime unit manifest: `src/js_runtime.js` partitioned into units with
//! declared dependencies, checked by a lexical scan.
//!
//! The runtime stays one source file and one closure. Header comments mark
//! where each unit starts; everything up to the next header is the unit's
//! body. A unit is one top-level declaration statement or a tightly coupled
//! group of adjacent statements (statements that share a source line always
//! share a unit). See docs/advanced-optimization.md, Phase 1.
//!
//! # File layout
//!
//! ```text
//! // Original bounded runtime ...       prologue: the factory comment,
//! (() => {                              `(() => {` and `"use strict";`
//!   "use strict";
//!
//!   //@unit keywords                    one header per unit, then its body
//!   const keywords = new WeakMap();
//!   ...
//!   //@exports                          the export section, last
//!   return Object.freeze({setDeclarationDemand, ..., hash});
//! })()
//! ```
//!
//! # Header syntax
//!
//! ```text
//! //@unit <name>[ effect[: <binding>, ...]][ requires: <binding>, ...]
//! //@ <binding>, ...                    continues the requires list
//! //@exports
//! ```
//!
//! A header line is any line whose first non-blank characters are `//@`.
//! - `<name>` is unique. A unit that declares bindings is named after the
//!   first one, so for most units the name is the binding. An effect unit
//!   that declares nothing has a free name, by convention `<target>.<what>`
//!   (`add.variadic`, `core.seal`); it never collides with a binding.
//! - `requires:` lists exactly the top-level runtime bindings the body
//!   references that other units declare, in any order (the update below
//!   writes them sorted case-insensitively). ECMAScript globals
//!   ([`ECMASCRIPT_GLOBALS`]) are never listed; host globals are not allowed
//!   at all, and the runtime reaches the host through `globalThis`.
//! - `effect` marks a unit with top-level statements other than declarations.
//!   Bare `effect` means the statements mutate state the unit itself
//!   declares, so they travel with it. `effect: a, b` names the bindings of
//!   other units whose state they mutate: the unit belongs to those bindings
//!   and must be kept whenever any of their units is kept (see
//!   [`RuntimeManifest::reachable`]). A unit that declares nothing names at
//!   least one target.
//!
//! # Validation
//!
//! [`RuntimeManifest::parse`] lexes each body (identifiers, punctuators,
//! strings, template literals, regular expression literals and comments; no
//! full parser) and resolves identifiers through function, block, class and
//! catch scopes, so a local that shadows a runtime binding is not a
//! dependency. [`RuntimeManifest::validate`] then requires every free
//! identifier of a unit to be its own binding, a listed require, or an
//! ECMAScript global, and rejects stale or unknown requires and misdeclared
//! effects. The `runtime_manifest` suite in `tests/compiler/` runs both on the
//! embedded runtime, so the runtime cannot regain hidden edges. After editing
//! the runtime, fix the headers by hand or rewrite every `requires:` list with
//! `LIGHT_CLJS_UPDATE_RUNTIME_MANIFEST=1 cargo test --test compiler runtime_manifest::`
//! and review the header diff.
//!
//! # Embedded text
//!
//! Header lines are build-time annotations. `build.rs` removes every header
//! line, so [`RUNTIME`] (the shared runtime embedded by the emitter, the
//! evaluator and the REPL) is byte-for-byte the runtime text without them,
//! and [`RuntimeManifest::runtime_text`] reproduces it from the units.
//! [`RuntimeManifest::annotated_text`] reproduces the annotated file itself.

use std::collections::HashMap;
use std::fmt;
use std::ops::Range;
use std::sync::OnceLock;

/// `src/js_runtime.js` as written, including unit headers.
pub const ANNOTATED_RUNTIME: &str = include_str!("js_runtime.js");

/// The shared runtime factory as embedded in output: [`ANNOTATED_RUNTIME`]
/// without its header lines (written by `build.rs`).
pub const RUNTIME: &str = include_str!(concat!(env!("OUT_DIR"), "/js_runtime.js"));

/// Global bindings of the ECMAScript specification that a unit may reference
/// without declaring them. Host globals (`console`, timers, ...) are absent.
pub const ECMASCRIPT_GLOBALS: &[&str] = &[
    "AggregateError",
    "Array",
    "ArrayBuffer",
    "Atomics",
    "BigInt",
    "BigInt64Array",
    "BigUint64Array",
    "Boolean",
    "DataView",
    "Date",
    "Error",
    "EvalError",
    "FinalizationRegistry",
    "Float16Array",
    "Float32Array",
    "Float64Array",
    "Function",
    "Infinity",
    "Int16Array",
    "Int32Array",
    "Int8Array",
    "Iterator",
    "JSON",
    "Map",
    "Math",
    "NaN",
    "Number",
    "Object",
    "Promise",
    "Proxy",
    "RangeError",
    "ReferenceError",
    "Reflect",
    "RegExp",
    "Set",
    "SharedArrayBuffer",
    "String",
    "Symbol",
    "SyntaxError",
    "TypeError",
    "URIError",
    "Uint16Array",
    "Uint32Array",
    "Uint8Array",
    "Uint8ClampedArray",
    "WeakMap",
    "WeakRef",
    "WeakSet",
    "decodeURI",
    "decodeURIComponent",
    "encodeURI",
    "encodeURIComponent",
    "eval",
    "globalThis",
    "isFinite",
    "isNaN",
    "parseFloat",
    "parseInt",
    "undefined",
];

/// Dense index of a unit in [`RuntimeManifest::units`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnitId(pub usize);

/// Top-level statements of a unit other than declarations, and whose state
/// they mutate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Effect<'a> {
    /// Bindings of other units the statements belong to. Empty when they
    /// mutate the unit's own declarations.
    pub targets: Vec<&'a str>,
}

#[derive(Clone, Debug)]
pub struct Unit<'a> {
    pub name: &'a str,
    /// The header lines, including their final newline.
    pub header_range: Range<usize>,
    /// The statements (with their comments and blank lines) up to the next header.
    pub body_range: Range<usize>,
    /// The bindings listed after `requires:`, in header order.
    pub requires: Vec<&'a str>,
    pub effect: Option<Effect<'a>>,
    /// Top-level bindings the body declares, in source order.
    pub declares: Vec<&'a str>,
    /// Free identifiers of the body: other units' bindings and globals,
    /// sorted and without duplicates. Excludes [`Unit::declares`].
    pub references: Vec<&'a str>,
    /// Whether some top-level statement is not a declaration.
    pub has_effect_statements: bool,
}

/// The `//@exports` section: the closure's return statement and its end.
#[derive(Clone, Debug)]
pub struct Exports<'a> {
    pub header_range: Range<usize>,
    pub body_range: Range<usize>,
    /// The shorthand properties of `return Object.freeze({...})`, in order.
    pub names: Vec<&'a str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestError {
    /// One-based line in the annotated source.
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "src/js_runtime.js:{}: {}", self.line, self.message)
    }
}

#[derive(Clone, Debug)]
pub struct RuntimeManifest<'a> {
    source: &'a str,
    /// Everything before the first header.
    pub prologue: Range<usize>,
    pub units: Vec<Unit<'a>>,
    pub exports: Exports<'a>,
    bindings: HashMap<&'a str, UnitId>,
    names: HashMap<&'a str, UnitId>,
    /// Per unit: the units declaring its required bindings, sorted.
    dependencies: Vec<Vec<UnitId>>,
    /// Per unit: the effect units targeting one of its bindings.
    attached: Vec<Vec<UnitId>>,
}

/// Whether `line` is a manifest header line. `build.rs` removes exactly these.
pub fn is_header_line(line: &str) -> bool {
    line.trim_start().starts_with("//@")
}

impl RuntimeManifest<'static> {
    /// The manifest of the embedded runtime, parsed once.
    ///
    /// # Panics
    /// When the embedded runtime does not parse; the `runtime_manifest` test
    /// suite rejects such a runtime.
    pub fn embedded() -> &'static RuntimeManifest<'static> {
        static MANIFEST: OnceLock<RuntimeManifest<'static>> = OnceLock::new();
        MANIFEST.get_or_init(|| {
            RuntimeManifest::parse(ANNOTATED_RUNTIME).unwrap_or_else(|errors| {
                panic!(
                    "invalid runtime manifest:\n{}",
                    errors
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            })
        })
    }
}

impl<'a> RuntimeManifest<'a> {
    /// Splits annotated runtime source into units and analyzes each body.
    /// Fails on malformed headers, bodies that do not lex or are not a
    /// sequence of complete statements, duplicate names or bindings, and a
    /// prologue or export section of another shape. Dependency declarations
    /// are checked separately by [`RuntimeManifest::validate`].
    pub fn parse(source: &'a str) -> Result<Self, Vec<ManifestError>> {
        let mut errors = Vec::new();
        let sections = split_sections(source, &mut errors);
        if !errors.is_empty() {
            return Err(errors);
        }
        let Some((prologue_end, sections)) = sections else {
            return Err(errors);
        };
        let line_of = |offset: usize| line_at(source, offset);

        // The prologue must stay the bare factory opening.
        match lex_section(source, 0..prologue_end, false) {
            Ok(tokens) => {
                let texts = tokens.iter().map(|t| t.text(source)).collect::<Vec<_>>();
                let shape = texts.len() == 7
                    && texts[..5] == ["(", "(", ")", "=>", "{"]
                    && tokens[5].kind == Kind::Str
                    && texts[6] == ";";
                if !shape {
                    errors.push(ManifestError {
                        line: 1,
                        message: "the prologue must be exactly `(() => { \"use strict\";` plus comments; put code in a unit".into(),
                    });
                }
            }
            Err(error) => errors.push(ManifestError {
                line: line_of(error.offset),
                message: error.message.into(),
            }),
        }

        let mut units = Vec::new();
        let mut exports = None;
        for section in sections {
            match section.header {
                Header::Exports => match parse_exports(source, section.body.clone()) {
                    Ok(names) => {
                        exports = Some(Exports {
                            header_range: section.header_range,
                            body_range: section.body,
                            names,
                        })
                    }
                    Err((offset, message)) => errors.push(ManifestError {
                        line: line_of(offset),
                        message,
                    }),
                },
                Header::Unit {
                    name,
                    effect,
                    requires,
                } => {
                    let facts = match lex(source, section.body.clone()) {
                        Ok(tokens) => analyze(source, &tokens),
                        Err(error) => Err((error.offset, error.message.to_string())),
                    };
                    match facts {
                        Ok(facts) => units.push(Unit {
                            name,
                            header_range: section.header_range,
                            body_range: section.body,
                            requires,
                            effect,
                            declares: facts.declares,
                            references: facts.references,
                            has_effect_statements: facts.has_effect_statements,
                        }),
                        Err((offset, message)) => errors.push(ManifestError {
                            line: line_of(offset),
                            message: format!("unit `{name}`: {message}"),
                        }),
                    }
                }
            }
        }

        let mut bindings = HashMap::new();
        let mut names = HashMap::new();
        for (index, unit) in units.iter().enumerate() {
            if names.insert(unit.name, index).is_some() {
                errors.push(ManifestError {
                    line: line_of(unit.header_range.start),
                    message: format!("duplicate unit name `{}`", unit.name),
                });
            }
            for &binding in &unit.declares {
                if let Some(previous) = bindings.insert(binding, UnitId(index)) {
                    errors.push(ManifestError {
                        line: line_of(unit.header_range.start),
                        message: format!(
                            "unit `{}` redeclares `{binding}`, already declared by unit `{}`",
                            unit.name, units[previous.0].name
                        ),
                    });
                }
            }
        }
        for unit in &units {
            if let Some(owner) = bindings.get(unit.name)
                && units[owner.0].name != unit.name
            {
                errors.push(ManifestError {
                    line: line_of(unit.header_range.start),
                    message: format!(
                        "unit name `{}` is a binding declared by unit `{}`",
                        unit.name, units[owner.0].name
                    ),
                });
            }
        }
        let Some(exports) = exports.filter(|_| errors.is_empty()) else {
            return Err(errors);
        };
        let mut dependencies = Vec::with_capacity(units.len());
        let mut attached = vec![Vec::new(); units.len()];
        for (index, unit) in units.iter().enumerate() {
            let mut required = unit
                .requires
                .iter()
                .filter_map(|binding| bindings.get(binding).copied())
                .collect::<Vec<_>>();
            required.sort_unstable();
            required.dedup();
            dependencies.push(required);
            for target in unit.effect.iter().flat_map(|effect| &effect.targets) {
                if let Some(owner) = bindings.get(target) {
                    attached[owner.0].push(UnitId(index));
                }
            }
        }
        let names = names
            .into_iter()
            .map(|(name, index)| (name, UnitId(index)))
            .collect();
        Ok(Self {
            source,
            prologue: 0..prologue_end,
            units,
            exports,
            bindings,
            names,
            dependencies,
            attached,
        })
    }

    pub fn source(&self) -> &'a str {
        self.source
    }

    pub fn unit(&self, id: UnitId) -> &Unit<'a> {
        &self.units[id.0]
    }

    /// The unit declaring a top-level runtime binding.
    pub fn unit_of(&self, binding: &str) -> Option<UnitId> {
        self.bindings.get(binding).copied()
    }

    /// The unit with this header name.
    pub fn unit_named(&self, name: &str) -> Option<UnitId> {
        self.names.get(name).copied()
    }

    /// Units declaring the bindings `unit` requires, sorted, without duplicates.
    pub fn dependencies(&self, unit: UnitId) -> &[UnitId] {
        &self.dependencies[unit.0]
    }

    /// Effect units that target one of `unit`'s bindings, in source order.
    pub fn attached_effects(&self, unit: UnitId) -> &[UnitId] {
        &self.attached[unit.0]
    }

    /// Units declaring the exported bindings, the roots of the full runtime.
    pub fn export_roots(&self) -> Vec<UnitId> {
        let mut roots = self
            .exports
            .names
            .iter()
            .filter_map(|name| self.unit_of(name))
            .collect::<Vec<_>>();
        roots.sort_unstable();
        roots.dedup();
        roots
    }

    /// The units a program needs when it uses `roots`, indexed by [`UnitId`]:
    /// the closure under `requires`, where keeping a unit also keeps every
    /// effect unit that targets one of its bindings.
    pub fn reachable(&self, roots: impl IntoIterator<Item = UnitId>) -> Vec<bool> {
        let mut kept = vec![false; self.units.len()];
        let mut stack = roots.into_iter().collect::<Vec<_>>();
        while let Some(unit) = stack.pop() {
            if std::mem::replace(&mut kept[unit.0], true) {
                continue;
            }
            stack.extend_from_slice(&self.dependencies[unit.0]);
            stack.extend_from_slice(&self.attached[unit.0]);
        }
        kept
    }

    /// Checks every unit's declared requires and effect against its body.
    pub fn validate(&self) -> Result<(), Vec<ManifestError>> {
        let mut errors = Vec::new();
        for (index, unit) in self.units.iter().enumerate() {
            let id = UnitId(index);
            let mut fail = |message: String| {
                errors.push(ManifestError {
                    line: line_at(self.source, unit.header_range.start),
                    message: format!("unit `{}`: {message}", unit.name),
                })
            };
            if let Some(&first) = unit.declares.first()
                && unit.name != first
            {
                fail(format!(
                    "declares bindings, so it must be named `{first}`, its first"
                ));
            }
            for &reference in &unit.references {
                match self.unit_of(reference) {
                    Some(owner) if !unit.requires.contains(&reference) => fail(format!(
                        "references `{reference}` (declared by unit `{}`) but does not require it",
                        self.units[owner.0].name
                    )),
                    Some(_) => {}
                    None if ECMASCRIPT_GLOBALS.binary_search(&reference).is_ok() => {}
                    None => fail(format!(
                        "references `{reference}`, which is neither a runtime binding nor an ECMAScript global"
                    )),
                }
            }
            for (position, &required) in unit.requires.iter().enumerate() {
                if unit.requires[..position].contains(&required) {
                    fail(format!("requires `{required}` twice"));
                } else if self.unit_of(required) == Some(id) {
                    fail(format!("requires its own binding `{required}`"));
                } else if self.unit_of(required).is_none() {
                    fail(format!("requires `{required}`, which no unit declares"));
                } else if unit.references.binary_search(&required).is_err() {
                    fail(format!("requires `{required}` but never references it"));
                }
            }
            match &unit.effect {
                None if unit.has_effect_statements => fail(
                    "has top-level statements other than declarations but is not marked `effect`"
                        .into(),
                ),
                Some(_) if !unit.has_effect_statements => {
                    fail("is marked `effect` but only declares bindings".into())
                }
                Some(effect) => {
                    if effect.targets.is_empty() && unit.declares.is_empty() {
                        fail(
                            "declares nothing, so its `effect` must name the bindings it mutates"
                                .into(),
                        );
                    }
                    for (position, &target) in effect.targets.iter().enumerate() {
                        if effect.targets[..position].contains(&target) {
                            fail(format!("names effect target `{target}` twice"));
                        }
                        match self.unit_of(target) {
                            None => {
                                fail(format!("effect target `{target}` is not a runtime binding"))
                            }
                            Some(owner) if owner == id => fail(format!(
                                "effect target `{target}` is its own binding; write bare `effect`"
                            )),
                            Some(_) => {}
                        }
                    }
                }
                None => {}
            }
        }
        for &name in &self.exports.names {
            if self.unit_of(name).is_none() {
                errors.push(ManifestError {
                    line: line_at(self.source, self.exports.body_range.start),
                    message: format!("exports `{name}`, which no unit declares"),
                });
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// The runtime bindings a unit references that other units declare: what
    /// its `requires:` list must contain.
    pub fn computed_requires(&self, unit: UnitId) -> Vec<&'a str> {
        let mut required = self.units[unit.0]
            .references
            .iter()
            .copied()
            .filter(|reference| self.unit_of(reference).is_some_and(|owner| owner != unit))
            .collect::<Vec<_>>();
        required.sort_by(|a, b| {
            a.to_ascii_lowercase()
                .cmp(&b.to_ascii_lowercase())
                .then(a.cmp(b))
        });
        required
    }

    /// The annotated source rebuilt from its parts; equals the parsed source.
    pub fn annotated_text(&self) -> String {
        let mut text = String::with_capacity(self.source.len());
        text.push_str(&self.source[self.prologue.clone()]);
        for unit in &self.units {
            text.push_str(&self.source[unit.header_range.clone()]);
            text.push_str(&self.source[unit.body_range.clone()]);
        }
        text.push_str(&self.source[self.exports.header_range.clone()]);
        text.push_str(&self.source[self.exports.body_range.clone()]);
        text
    }

    /// The full runtime factory without headers: the prologue, every unit
    /// body in order, and the export section. For the embedded manifest this
    /// equals [`RUNTIME`], the text the shared path embeds.
    pub fn runtime_text(&self) -> String {
        let mut text = String::with_capacity(self.source.len());
        text.push_str(&self.source[self.prologue.clone()]);
        for unit in &self.units {
            text.push_str(&self.source[unit.body_range.clone()]);
        }
        text.push_str(&self.source[self.exports.body_range.clone()]);
        text
    }

    /// The annotated source with every `requires:` list replaced by
    /// [`RuntimeManifest::computed_requires`], wrapped to 100 columns.
    /// Names, effects and bodies are kept.
    pub fn with_computed_requires(&self) -> String {
        let mut text = String::with_capacity(self.source.len());
        text.push_str(&self.source[self.prologue.clone()]);
        for (index, unit) in self.units.iter().enumerate() {
            let header = &self.source[unit.header_range.clone()];
            let indent = &header[..header.len() - header.trim_start().len()];
            let mut words = vec![format!("//@unit {}", unit.name)];
            match &unit.effect {
                Some(effect) if effect.targets.is_empty() => words.push("effect".into()),
                Some(effect) => words.push(format!("effect: {}", effect.targets.join(", "))),
                None => {}
            }
            let required = self.computed_requires(UnitId(index));
            let mut line = format!("{indent}{}", words.join(" "));
            if !required.is_empty() {
                line.push_str(" requires:");
                for (position, binding) in required.iter().enumerate() {
                    let item = if position + 1 < required.len() {
                        format!("{binding},")
                    } else {
                        binding.to_string()
                    };
                    if line.len() + 1 + item.len() > 100 && !line.trim_end().ends_with(':') {
                        text.push_str(&line);
                        text.push('\n');
                        line = format!("{indent}//@");
                    }
                    line.push(' ');
                    line.push_str(&item);
                }
            }
            text.push_str(&line);
            text.push('\n');
            text.push_str(&self.source[unit.body_range.clone()]);
        }
        text.push_str(&self.source[self.exports.header_range.clone()]);
        text.push_str(&self.source[self.exports.body_range.clone()]);
        text
    }
}

fn line_at(source: &str, offset: usize) -> usize {
    source.as_bytes()[..offset.min(source.len())]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

// ---------------------------------------------------------------------------
// Headers

enum Header<'a> {
    Unit {
        name: &'a str,
        effect: Option<Effect<'a>>,
        requires: Vec<&'a str>,
    },
    Exports,
}

struct Section<'a> {
    header: Header<'a>,
    header_range: Range<usize>,
    body: Range<usize>,
}

/// Returns the prologue end and the sections in order, or records errors.
fn split_sections<'a>(
    source: &'a str,
    errors: &mut Vec<ManifestError>,
) -> Option<(usize, Vec<Section<'a>>)> {
    // A header's lines: where they start and end, and the text after `//@unit`.
    struct Pending<'a> {
        start: usize,
        end: usize,
        line: usize,
        exports: bool,
        first: &'a str,
        continued: Vec<&'a str>,
    }
    let mut pending: Vec<Pending<'a>> = Vec::new();
    let mut offset = 0;
    let mut previous_was_header = false;
    for (index, line) in source.split_inclusive('\n').enumerate() {
        let start = offset;
        offset += line.len();
        if !is_header_line(line) {
            previous_was_header = false;
            continue;
        }
        let text = line.trim();
        let rest = &text[3..];
        let line_number = index + 1;
        if let Some(remainder) = rest.strip_prefix("unit")
            && remainder.starts_with(|c: char| c.is_whitespace())
        {
            pending.push(Pending {
                start,
                end: offset,
                line: line_number,
                exports: false,
                first: remainder,
                continued: Vec::new(),
            });
        } else if rest == "exports" {
            pending.push(Pending {
                start,
                end: offset,
                line: line_number,
                exports: true,
                first: "",
                continued: Vec::new(),
            });
        } else if rest.is_empty() || rest.starts_with(|c: char| c.is_whitespace()) {
            match pending.last_mut() {
                Some(header) if previous_was_header && !header.exports => {
                    header.continued.push(rest);
                    header.end = offset;
                }
                _ => errors.push(ManifestError {
                    line: line_number,
                    message: "a `//@` continuation line must directly follow a `//@unit` header"
                        .into(),
                }),
            }
        } else {
            errors.push(ManifestError {
                line: line_number,
                message: format!("unknown manifest header `{text}`; expected `//@unit`, `//@exports` or a `//@ ` continuation"),
            });
        }
        previous_was_header = true;
    }
    let Some(first) = pending.first() else {
        errors.push(ManifestError {
            line: 1,
            message: "the runtime has no `//@unit` headers".into(),
        });
        return None;
    };
    let prologue_end = first.start;
    let mut sections = Vec::new();
    for (index, header) in pending.iter().enumerate() {
        let body_end = pending
            .get(index + 1)
            .map_or(source.len(), |next| next.start);
        let parsed = if header.exports {
            if index + 1 != pending.len() {
                errors.push(ManifestError {
                    line: header.line,
                    message: "`//@exports` must be the last header".into(),
                });
            }
            Ok(Header::Exports)
        } else {
            parse_unit_header(header.first, &header.continued)
        };
        match parsed {
            Ok(parsed) => sections.push(Section {
                header: parsed,
                header_range: header.start..header.end,
                body: header.end..body_end,
            }),
            Err(message) => errors.push(ManifestError {
                line: header.line,
                message,
            }),
        }
    }
    if !pending.last().is_some_and(|header| header.exports) {
        errors.push(ManifestError {
            line: line_at(source, source.len()),
            message: "the runtime must end with an `//@exports` section".into(),
        });
    }
    Some((prologue_end, sections))
}

fn is_identifier(word: &str) -> bool {
    let mut chars = word.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

fn parse_unit_header<'a>(first: &'a str, continued: &[&'a str]) -> Result<Header<'a>, String> {
    let mut rest = first.trim();
    let name_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let name = &rest[..name_end];
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '$' | '.' | '-'))
    {
        return Err(format!("invalid unit name `{name}`"));
    }
    rest = rest[name_end..].trim_start();
    let mut effect = None;
    if let Some(after) = rest.strip_prefix("effect")
        && (after.is_empty() || after.starts_with(|c: char| c.is_whitespace() || c == ':'))
    {
        let mut targets = Vec::new();
        rest = after.trim_start();
        if let Some(after) = rest.strip_prefix(':') {
            rest = after;
            loop {
                rest = rest.trim_start();
                if rest.starts_with("requires:") {
                    break;
                }
                let end = rest
                    .find(|c: char| c == ',' || c.is_whitespace())
                    .unwrap_or(rest.len());
                let target = &rest[..end];
                if !is_identifier(target) {
                    return Err(format!("unit `{name}`: invalid effect target `{target}`"));
                }
                targets.push(target);
                rest = rest[end..].trim_start();
                match rest.strip_prefix(',') {
                    Some(after) => rest = after,
                    None => break,
                }
            }
            if targets.is_empty() {
                return Err(format!("unit `{name}`: `effect:` names no binding"));
            }
        }
        effect = Some(Effect { targets });
    }
    let mut requires = Vec::new();
    if let Some(after) = rest.strip_prefix("requires:") {
        for item in std::iter::once(after)
            .chain(continued.iter().copied())
            .flat_map(|line| line.split(','))
        {
            let item = item.trim();
            if item.is_empty() {
                continue;
            }
            if !is_identifier(item) {
                return Err(format!("unit `{name}`: invalid required binding `{item}`"));
            }
            requires.push(item);
        }
        rest = "";
    } else if !continued.is_empty() {
        return Err(format!(
            "unit `{name}`: continuation lines continue a `requires:` list"
        ));
    }
    if !rest.is_empty() {
        return Err(format!(
            "unit `{name}`: unexpected `{rest}`; expected `effect` or `requires:`"
        ));
    }
    Ok(Header::Unit {
        name,
        effect,
        requires,
    })
}

/// Checks the export section's shape and returns the exported names.
fn parse_exports(source: &str, range: Range<usize>) -> Result<Vec<&str>, (usize, String)> {
    let tokens = lex_section(source, range.clone(), false)
        .map_err(|error| (error.offset, error.message.to_string()))?;
    let texts = tokens.iter().map(|t| t.text(source)).collect::<Vec<_>>();
    let shape_error = || {
        (
            range.start,
            "the export section must be `return Object.freeze({name, ...});` followed by `})()`"
                .to_string(),
        )
    };
    let head = ["return", "Object", ".", "freeze", "(", "{"];
    let tail = ["}", ")", ";", "}", ")", "(", ")"];
    if texts.len() < head.len() + tail.len()
        || texts[..head.len()] != head
        || texts[texts.len() - tail.len()..] != tail
    {
        return Err(shape_error());
    }
    let mut names = Vec::new();
    let list = &tokens[head.len()..tokens.len() - tail.len()];
    for (index, token) in list.iter().enumerate() {
        let text = token.text(source);
        let expected_name = index % 2 == 0;
        if expected_name && token.kind == Kind::Name && is_identifier(text) {
            names.push(text);
        } else if expected_name || text != "," {
            return Err(shape_error());
        }
    }
    Ok(names)
}

// ---------------------------------------------------------------------------
// Lexer

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Name,
    Private,
    Punct,
    Number,
    Str,
    Template,
    TemplateHead,
    TemplateMiddle,
    TemplateTail,
    Regex,
}

#[derive(Clone, Copy, Debug)]
struct Token {
    kind: Kind,
    start: usize,
    end: usize,
    newline_before: bool,
}

impl Token {
    fn text<'a>(&self, source: &'a str) -> &'a str {
        &source[self.start..self.end]
    }
}

struct LexError {
    offset: usize,
    message: &'static str,
}

const PUNCTUATORS: &[&str] = &[
    ">>>=", "...", "===", "!==", "**=", "<<=", ">>=", ">>>", "&&=", "||=", "??=", "=>", "==", "!=",
    "<=", ">=", "&&", "||", "??", "?.", "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=",
    "**", "<<", ">>", "{", "}", "(", ")", "[", "]", ";", ",", "<", ">", "+", "-", "*", "/", "%",
    "&", "|", "^", "!", "~", "?", ":", "=", ".", "@",
];

fn identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte == b'$' || byte >= 0x80
}

fn identifier_part(byte: u8) -> bool {
    identifier_start(byte) || byte.is_ascii_digit()
}

/// Whether a `/` after `previous` starts a regular expression literal.
/// `closed_control` tells whether the last `)` closed an `if`, `while`,
/// `for` or `with` head, after which a statement (and so a regex) starts.
fn regex_allowed(source: &str, previous: Option<&Token>, closed_control: bool) -> bool {
    let Some(previous) = previous else {
        return true;
    };
    if previous.kind == Kind::Punct && previous.text(source) == ")" {
        return closed_control;
    }
    match previous.kind {
        Kind::Name => matches!(
            previous.text(source),
            "return"
                | "typeof"
                | "instanceof"
                | "in"
                | "of"
                | "new"
                | "delete"
                | "void"
                | "throw"
                | "case"
                | "do"
                | "else"
                | "yield"
                | "await"
        ),
        Kind::Punct => !matches!(previous.text(source), ")" | "]" | "}" | "++" | "--"),
        Kind::TemplateHead | Kind::TemplateMiddle => true,
        _ => false,
    }
}

/// Tokens of a unit body, which must close every bracket it opens.
fn lex(source: &str, range: Range<usize>) -> Result<Vec<Token>, LexError> {
    lex_section(source, range, true)
}

/// Tokens of `range`. Unbalanced sections (the prologue opens the factory
/// closure and the export section closes it) skip the brace checks.
fn lex_section(source: &str, range: Range<usize>, balanced: bool) -> Result<Vec<Token>, LexError> {
    let bytes = source.as_bytes();
    let end = range.end;
    let at = |i: usize| if i < end { bytes[i] } else { 0 };
    let mut i = range.start;
    let mut tokens: Vec<Token> = Vec::new();
    // One entry per open `{`: true when it is a template substitution.
    let mut braces: Vec<bool> = Vec::new();
    // One entry per open `(`: true when it opens a control statement head.
    let mut parens: Vec<bool> = Vec::new();
    let mut closed_control = false;
    let mut newline = false;
    // Scans template characters after a backtick or a substitution's `}`.
    let template = |mut i: usize| -> Result<(usize, bool), LexError> {
        loop {
            match at(i) {
                0 if i >= end => {
                    return Err(LexError {
                        offset: i,
                        message: "unterminated template literal",
                    });
                }
                b'\\' => i += 2,
                b'`' => return Ok((i + 1, true)),
                b'$' if at(i + 1) == b'{' => return Ok((i + 2, false)),
                _ => i += 1,
            }
        }
    };
    while i < end {
        let byte = bytes[i];
        match byte {
            b'\n' | b'\r' => {
                newline = true;
                i += 1;
                continue;
            }
            b' ' | b'\t' | 0x0b | 0x0c => {
                i += 1;
                continue;
            }
            b'/' if at(i + 1) == b'/' => {
                while i < end && !matches!(bytes[i], b'\n' | b'\r') {
                    i += 1;
                }
                continue;
            }
            b'/' if at(i + 1) == b'*' => {
                let Some(close) = source[i + 2..end].find("*/") else {
                    return Err(LexError {
                        offset: i,
                        message: "unterminated block comment",
                    });
                };
                newline |= source[i..i + 2 + close].contains(['\n', '\r']);
                i += close + 4;
                continue;
            }
            _ => {}
        }
        let start = i;
        let kind = match byte {
            b'`' => {
                let (next, ended) = template(i + 1)?;
                i = next;
                if ended {
                    Kind::Template
                } else {
                    braces.push(true);
                    Kind::TemplateHead
                }
            }
            b'}' if braces.last() == Some(&true) => {
                braces.pop();
                let (next, ended) = template(i + 1)?;
                i = next;
                if ended {
                    Kind::TemplateTail
                } else {
                    braces.push(true);
                    Kind::TemplateMiddle
                }
            }
            b'"' | b'\'' => {
                i += 1;
                loop {
                    match at(i) {
                        b'\\' if at(i + 1) == b'\r' && at(i + 2) == b'\n' => i += 3,
                        b'\\' => i += 2,
                        b'\n' | b'\r' | 0 => {
                            return Err(LexError {
                                offset: start,
                                message: "unterminated string literal",
                            });
                        }
                        quote if quote == byte => {
                            i += 1;
                            break;
                        }
                        _ => i += 1,
                    }
                }
                Kind::Str
            }
            b'0'..=b'9' | b'.' if byte != b'.' || at(i + 1).is_ascii_digit() => {
                let radix =
                    byte == b'0' && matches!(at(i + 1), b'x' | b'X' | b'o' | b'O' | b'b' | b'B');
                i += 1;
                // Digits, separators, a fraction, an exponent and its sign.
                while at(i).is_ascii_alphanumeric()
                    || matches!(at(i), b'_' | b'.')
                    || (matches!(at(i), b'+' | b'-') && !radix && matches!(at(i - 1), b'e' | b'E'))
                {
                    i += 1;
                }
                Kind::Number
            }
            b'#' if identifier_start(at(i + 1)) => {
                i += 1;
                while identifier_part(at(i)) {
                    i += 1;
                }
                Kind::Private
            }
            _ if identifier_start(byte) => {
                while identifier_part(at(i)) {
                    i += 1;
                }
                Kind::Name
            }
            b'/' if regex_allowed(source, tokens.last(), closed_control) => {
                i += 1;
                let mut class = false;
                loop {
                    match at(i) {
                        b'\n' | b'\r' | 0 => {
                            return Err(LexError {
                                offset: start,
                                message: "unterminated regular expression literal",
                            });
                        }
                        b'\\' => i += 2,
                        b'[' => {
                            class = true;
                            i += 1;
                        }
                        b']' => {
                            class = false;
                            i += 1;
                        }
                        b'/' if !class => {
                            i += 1;
                            break;
                        }
                        _ => i += 1,
                    }
                }
                while identifier_part(at(i)) {
                    i += 1;
                }
                Kind::Regex
            }
            _ => {
                let rest = &source[i..end];
                let Some(punctuator) = PUNCTUATORS.iter().find(|p| {
                    rest.starts_with(**p) && !(**p == "?." && at(i + 2).is_ascii_digit())
                }) else {
                    return Err(LexError {
                        offset: i,
                        message: "unexpected character",
                    });
                };
                i += punctuator.len();
                match *punctuator {
                    "(" => parens.push(tokens.last().is_some_and(|t| {
                        t.kind == Kind::Name
                            && matches!(t.text(source), "if" | "while" | "for" | "with")
                    })),
                    ")" => closed_control = parens.pop().unwrap_or(false),
                    "{" => braces.push(false),
                    "}" if braces.pop().is_none() && balanced => {
                        return Err(LexError {
                            offset: start,
                            message: "unbalanced `}`",
                        });
                    }
                    _ => {}
                }
                Kind::Punct
            }
        };
        if i > end {
            return Err(LexError {
                offset: start,
                message: "token runs past the end of its unit",
            });
        }
        tokens.push(Token {
            kind,
            start,
            end: i,
            newline_before: newline,
        });
        newline = false;
    }
    if braces.contains(&true) || (balanced && !braces.is_empty()) {
        return Err(LexError {
            offset: end,
            message: "unclosed `{` or template literal",
        });
    }
    Ok(tokens)
}

// ---------------------------------------------------------------------------
// Scope analysis

struct Facts<'a> {
    declares: Vec<&'a str>,
    references: Vec<&'a str>,
    has_effect_statements: bool,
}

struct Scope<'a> {
    parent: usize,
    /// Function scopes receive `var` declarations; scope 0 is the unit's top level.
    function: bool,
    names: Vec<&'a str>,
}

const NO_SCOPE: usize = usize::MAX;

/// Reserved words and literals that are never identifier references.
fn reserved(word: &str) -> bool {
    matches!(
        word,
        "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "debugger"
            | "default"
            | "delete"
            | "do"
            | "else"
            | "enum"
            | "export"
            | "extends"
            | "false"
            | "finally"
            | "for"
            | "function"
            | "if"
            | "import"
            | "in"
            | "instanceof"
            | "let"
            | "new"
            | "null"
            | "return"
            | "static"
            | "super"
            | "switch"
            | "this"
            | "throw"
            | "true"
            | "try"
            | "typeof"
            | "var"
            | "void"
            | "while"
            | "with"
            | "yield"
            | "await"
            | "implements"
            | "interface"
            | "package"
            | "private"
            | "protected"
            | "public"
    )
}

/// Words that cannot continue an expression; meeting one ends it (after ASI).
fn ends_expression(word: &str) -> bool {
    matches!(
        word,
        "var"
            | "let"
            | "const"
            | "if"
            | "for"
            | "while"
            | "do"
            | "return"
            | "throw"
            | "try"
            | "switch"
            | "break"
            | "continue"
            | "debugger"
            | "case"
            | "default"
            | "else"
            | "catch"
            | "finally"
            | "with"
            | "import"
            | "export"
    )
}

struct Parser<'t, 'a> {
    source: &'a str,
    tokens: &'t [Token],
    /// For each bracket token, the index of its partner.
    partners: Vec<usize>,
    pos: usize,
    scopes: Vec<Scope<'a>>,
    references: Vec<(&'a str, usize)>,
    error: Option<(usize, String)>,
}

#[derive(Clone, Copy)]
struct Stops {
    comma: bool,
    /// The left-hand side of a `for` head ends at `of` or `in`.
    for_head: bool,
}

const SEQUENCE: Stops = Stops {
    comma: false,
    for_head: false,
};
const ASSIGNMENT: Stops = Stops {
    comma: true,
    for_head: false,
};

fn analyze<'a>(source: &'a str, tokens: &[Token]) -> Result<Facts<'a>, (usize, String)> {
    let mut partners = vec![usize::MAX; tokens.len()];
    let mut open = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if token.kind != Kind::Punct {
            continue;
        }
        match token.text(source) {
            "(" | "[" | "{" => open.push(index),
            close @ (")" | "]" | "}") => {
                let expected = match close {
                    ")" => "(",
                    "]" => "[",
                    _ => "{",
                };
                match open.pop() {
                    Some(opener) if tokens[opener].text(source) == expected => {
                        partners[opener] = index;
                        partners[index] = opener;
                    }
                    _ => return Err((token.start, format!("unbalanced `{close}`"))),
                }
            }
            _ => {}
        }
    }
    if let Some(&opener) = open.last() {
        return Err((
            tokens[opener].start,
            format!("unclosed `{}`", tokens[opener].text(source)),
        ));
    }
    let mut parser = Parser {
        source,
        tokens,
        partners,
        pos: 0,
        scopes: vec![Scope {
            parent: NO_SCOPE,
            function: true,
            names: Vec::new(),
        }],
        references: Vec::new(),
        error: None,
    };
    let mut has_effect_statements = false;
    while parser.pos < tokens.len() && parser.error.is_none() {
        let declaration = match parser.word() {
            Some("var" | "let" | "const" | "function" | "class") => true,
            Some("async") => parser.word_at(parser.pos + 1) == Some("function"),
            _ => parser.punct() == Some(";"),
        };
        has_effect_statements |= !declaration;
        parser.statement_progressing(0);
    }
    if let Some(error) = parser.error {
        return Err(error);
    }
    let declares = parser.scopes[0].names.clone();
    let mut references = Vec::new();
    for &(name, scope) in &parser.references {
        let mut current = scope;
        let local = loop {
            if current == 0 {
                break parser.scopes[0].names.contains(&name);
            }
            if parser.scopes[current].names.contains(&name) {
                break true;
            }
            current = parser.scopes[current].parent;
        };
        if !local {
            references.push(name);
        }
    }
    references.sort_unstable();
    references.dedup();
    Ok(Facts {
        declares,
        references,
        has_effect_statements,
    })
}

impl<'a> Parser<'_, 'a> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn word_at(&self, index: usize) -> Option<&'a str> {
        self.tokens
            .get(index)
            .filter(|t| t.kind == Kind::Name)
            .map(|t| t.text(self.source))
    }

    fn word(&self) -> Option<&'a str> {
        self.word_at(self.pos)
    }

    fn punct_at(&self, index: usize) -> Option<&'a str> {
        self.tokens
            .get(index)
            .filter(|t| t.kind == Kind::Punct)
            .map(|t| t.text(self.source))
    }

    fn punct(&self) -> Option<&'a str> {
        self.punct_at(self.pos)
    }

    fn at_punct(&self, text: &str) -> bool {
        self.punct() == Some(text)
    }

    fn at_word(&self, text: &str) -> bool {
        self.word() == Some(text)
    }

    fn fail(&mut self, message: impl Into<String>) {
        if self.error.is_none() {
            let offset = self.peek().or(self.tokens.last()).map_or(0, |t| t.start);
            self.error = Some((offset, message.into()));
        }
        self.pos = self.tokens.len();
    }

    fn expect(&mut self, text: &str) {
        if self.at_punct(text) {
            self.pos += 1;
        } else {
            let found = self.peek().map_or("the end of the unit".to_string(), |t| {
                format!("`{}`", t.text(self.source))
            });
            self.fail(format!("expected `{text}`, found {found}"));
        }
    }

    fn eat(&mut self, text: &str) -> bool {
        let found = self.at_punct(text);
        self.pos += usize::from(found);
        found
    }

    fn scope(&mut self, parent: usize, function: bool) -> usize {
        self.scopes.push(Scope {
            parent,
            function,
            names: Vec::new(),
        });
        self.scopes.len() - 1
    }

    fn declare(&mut self, scope: usize, name: &'a str) {
        if !self.scopes[scope].names.contains(&name) {
            self.scopes[scope].names.push(name);
        }
    }

    fn function_scope(&self, mut scope: usize) -> usize {
        while !self.scopes[scope].function {
            scope = self.scopes[scope].parent;
        }
        scope
    }

    fn reference(&mut self, name: &'a str, scope: usize) {
        self.references.push((name, scope));
    }

    /// The single-token property key at the cursor, if any.
    fn key_start(&self, index: usize) -> bool {
        self.tokens.get(index).is_some_and(|t| {
            matches!(
                t.kind,
                Kind::Name | Kind::Str | Kind::Number | Kind::Private
            ) || (t.kind == Kind::Punct && matches!(t.text(self.source), "[" | "*"))
        })
    }

    /// Whether the `(` at `index` opens arrow function parameters.
    fn arrow_parameters_at(&self, index: usize) -> bool {
        self.punct_at(index) == Some("(") && self.punct_at(self.partners[index] + 1) == Some("=>")
    }

    fn statements_until_brace(&mut self, scope: usize) {
        while self.pos < self.tokens.len() && !self.at_punct("}") {
            self.statement_progressing(scope);
        }
        self.expect("}");
    }

    fn statement_progressing(&mut self, scope: usize) {
        let start = self.pos;
        self.statement(scope);
        if self.pos == start {
            let text = self.peek().map_or("", |t| t.text(self.source)).to_string();
            self.fail(format!("unexpected `{text}`"));
        }
    }

    fn statement(&mut self, scope: usize) {
        if self.at_punct("{") {
            self.pos += 1;
            let block = self.scope(scope, false);
            self.statements_until_brace(block);
            return;
        }
        if self.eat(";") {
            return;
        }
        let Some(word) = self.word() else {
            self.expression(scope, SEQUENCE);
            self.eat(";");
            return;
        };
        match word {
            "var" | "let" | "const" => {
                self.pos += 1;
                let target = if word == "var" {
                    self.function_scope(scope)
                } else {
                    scope
                };
                self.declarations(target, scope, false);
                self.eat(";");
            }
            "function" => self.function_declaration(scope),
            "async" if self.word_at(self.pos + 1) == Some("function") => {
                self.pos += 1;
                self.function_declaration(scope);
            }
            "class" => {
                self.pos += 1;
                match self.word() {
                    Some(name) if !reserved(name) => {
                        self.pos += 1;
                        self.declare(scope, name);
                        self.class_tail(scope, Some(name));
                    }
                    _ => self.fail("expected a class name"),
                }
            }
            "if" => {
                self.pos += 1;
                self.parenthesized(scope);
                self.statement_progressing(scope);
                if self.at_word("else") {
                    self.pos += 1;
                    self.statement_progressing(scope);
                }
            }
            "for" => self.for_statement(scope),
            "while" => {
                self.pos += 1;
                self.parenthesized(scope);
                self.statement_progressing(scope);
            }
            "do" => {
                self.pos += 1;
                self.statement_progressing(scope);
                if !self.at_word("while") {
                    return self.fail("expected `while` after a `do` body");
                }
                self.pos += 1;
                self.parenthesized(scope);
                self.eat(";");
            }
            "switch" => {
                self.pos += 1;
                self.parenthesized(scope);
                self.expect("{");
                let block = self.scope(scope, false);
                while self.pos < self.tokens.len() && !self.at_punct("}") {
                    if self.at_word("case") {
                        self.pos += 1;
                        self.expression(block, SEQUENCE);
                        self.expect(":");
                    } else if self.at_word("default") {
                        self.pos += 1;
                        self.expect(":");
                    } else {
                        self.statement_progressing(block);
                    }
                }
                self.expect("}");
            }
            "try" => {
                self.pos += 1;
                self.block(scope);
                if self.at_word("catch") {
                    self.pos += 1;
                    let catch = self.scope(scope, false);
                    if self.eat("(") {
                        self.binding(catch, catch);
                        self.expect(")");
                    }
                    self.block(catch);
                }
                if self.at_word("finally") {
                    self.pos += 1;
                    self.block(scope);
                }
            }
            "return" | "throw" => {
                self.pos += 1;
                let ends = self.peek().is_none_or(|t| {
                    (word == "return" && t.newline_before)
                        || matches!(self.punct(), Some(";" | "}"))
                });
                if !ends {
                    self.expression(scope, SEQUENCE);
                }
                self.eat(";");
            }
            "break" | "continue" => {
                self.pos += 1;
                if self
                    .peek()
                    .is_some_and(|t| t.kind == Kind::Name && !t.newline_before)
                {
                    self.pos += 1;
                }
                self.eat(";");
            }
            "debugger" => {
                self.pos += 1;
                self.eat(";");
            }
            label if !reserved(label) && self.punct_at(self.pos + 1) == Some(":") => {
                self.pos += 2;
                self.statement_progressing(scope);
            }
            _ => {
                self.expression(scope, SEQUENCE);
                self.eat(";");
            }
        }
    }

    fn block(&mut self, scope: usize) {
        self.expect("{");
        let block = self.scope(scope, false);
        self.statements_until_brace(block);
    }

    fn parenthesized(&mut self, scope: usize) {
        self.expect("(");
        self.expression(scope, SEQUENCE);
        self.expect(")");
    }

    fn for_statement(&mut self, scope: usize) {
        self.pos += 1;
        if self.at_word("await") {
            self.pos += 1;
        }
        self.expect("(");
        let head = self.scope(scope, false);
        match self.word() {
            Some(word @ ("var" | "let" | "const")) => {
                self.pos += 1;
                let target = if word == "var" {
                    self.function_scope(head)
                } else {
                    head
                };
                self.declarations(target, head, true);
            }
            _ if self.at_punct(";") => {}
            _ => self.expression(
                head,
                Stops {
                    comma: false,
                    for_head: true,
                },
            ),
        }
        if self.at_word("of") || self.at_word("in") {
            self.pos += 1;
            self.expression(head, SEQUENCE);
        } else {
            self.expect(";");
            if !self.at_punct(";") {
                self.expression(head, SEQUENCE);
            }
            self.expect(";");
            if !self.at_punct(")") {
                self.expression(head, SEQUENCE);
            }
        }
        self.expect(")");
        self.statement_progressing(head);
    }

    /// Declarators after `var`/`let`/`const`: names go to `target`,
    /// initializers and defaults resolve in `scope`.
    fn declarations(&mut self, target: usize, scope: usize, for_head: bool) {
        loop {
            self.binding(target, scope);
            if self.eat("=") {
                self.expression(scope, ASSIGNMENT);
            }
            if for_head && (self.at_word("of") || self.at_word("in")) {
                return;
            }
            if !self.eat(",") {
                return;
            }
        }
    }

    /// A binding identifier or destructuring pattern.
    fn binding(&mut self, target: usize, scope: usize) {
        match self.peek().map(|t| (t.kind, t.text(self.source))) {
            Some((Kind::Name, name)) if !reserved(name) => {
                self.pos += 1;
                self.declare(target, name);
            }
            Some((Kind::Punct, "[")) => {
                self.pos += 1;
                while self.pos < self.tokens.len() && !self.at_punct("]") {
                    if self.eat(",") {
                        continue;
                    }
                    self.eat("...");
                    self.binding(target, scope);
                    if self.eat("=") {
                        self.expression(scope, ASSIGNMENT);
                    }
                    if !self.at_punct("]") {
                        self.expect(",");
                    }
                }
                self.expect("]");
            }
            Some((Kind::Punct, "{")) => {
                self.pos += 1;
                while self.pos < self.tokens.len() && !self.at_punct("}") {
                    if self.eat("...") {
                        self.binding(target, scope);
                    } else if self.eat("[") {
                        self.expression(scope, SEQUENCE);
                        self.expect("]");
                        self.expect(":");
                        self.binding(target, scope);
                    } else {
                        let Some(key) = self.peek().copied() else {
                            break;
                        };
                        if !matches!(key.kind, Kind::Name | Kind::Str | Kind::Number) {
                            return self.fail("expected a property name in an object pattern");
                        }
                        self.pos += 1;
                        if self.eat(":") {
                            self.binding(target, scope);
                        } else if key.kind == Kind::Name && !reserved(key.text(self.source)) {
                            self.declare(target, key.text(self.source));
                        } else {
                            return self.fail("expected `:` after a non-identifier pattern key");
                        }
                    }
                    if self.eat("=") {
                        self.expression(scope, ASSIGNMENT);
                    }
                    if !self.at_punct("}") {
                        self.expect(",");
                    }
                }
                self.expect("}");
            }
            _ => self.fail("expected a binding name or pattern"),
        }
    }

    fn function_declaration(&mut self, scope: usize) {
        self.pos += 1; // function
        self.eat("*");
        match self.word() {
            Some(name) if !reserved(name) => {
                self.pos += 1;
                self.declare(scope, name);
                self.function_rest(scope, None, true);
            }
            _ => self.fail("expected a function name"),
        }
    }

    /// Parameters and body, from the `(`. `own_name` binds a named function
    /// expression inside itself.
    fn function_rest(&mut self, outer: usize, own_name: Option<&'a str>, has_arguments: bool) {
        let function = self.scope(outer, true);
        if let Some(name) = own_name {
            self.declare(function, name);
        }
        if has_arguments {
            self.declare(function, "arguments");
        }
        self.parameters(function);
        self.expect("{");
        self.statements_until_brace(function);
    }

    fn parameters(&mut self, function: usize) {
        self.expect("(");
        while self.pos < self.tokens.len() && !self.at_punct(")") {
            self.eat("...");
            self.binding(function, function);
            if self.eat("=") {
                self.expression(function, ASSIGNMENT);
            }
            if !self.at_punct(")") {
                self.expect(",");
            }
        }
        self.expect(")");
    }

    /// An arrow function from its parameters (a name or a parenthesized list).
    fn arrow(&mut self, outer: usize) {
        let function = self.scope(outer, true);
        if self.at_punct("(") {
            self.parameters(function);
        } else {
            self.binding(function, function);
        }
        self.expect("=>");
        if self.eat("{") {
            self.statements_until_brace(function);
        } else {
            self.expression(function, ASSIGNMENT);
        }
    }

    fn class_tail(&mut self, scope: usize, name: Option<&'a str>) {
        let class = self.scope(scope, false);
        if let Some(name) = name {
            self.declare(class, name);
        }
        if self.at_word("extends") {
            self.pos += 1;
            // A left-hand-side expression; its end is the class body's `{`.
            while self.pos < self.tokens.len() && !self.at_punct("{") {
                if self.at_punct("(") || self.at_punct("[") {
                    let close = self.partners[self.pos];
                    self.pos += 1;
                    self.expression(scope, SEQUENCE);
                    if self.pos != close {
                        return self.fail("malformed class heritage");
                    }
                    self.pos += 1;
                } else if self.at_punct(".") || self.at_punct("?.") {
                    self.pos += 2;
                } else if let Some(word) = self.word().filter(|word| !reserved(word)) {
                    self.reference(word, scope);
                    self.pos += 1;
                } else {
                    self.pos += 1;
                }
            }
        }
        self.expect("{");
        while self.pos < self.tokens.len() && !self.at_punct("}") {
            if self.eat(";") {
                continue;
            }
            if self.at_word("static") && self.punct_at(self.pos + 1) == Some("{") {
                self.pos += 2;
                let block = self.scope(class, true);
                self.statements_until_brace(block);
                continue;
            }
            self.member_key(class);
            if self.at_punct("(") {
                self.function_rest(class, None, true);
            } else {
                if self.eat("=") {
                    let initializer = self.scope(class, true);
                    self.expression(initializer, ASSIGNMENT);
                }
                if !self.at_punct("}") {
                    self.expect(";");
                }
            }
        }
        self.expect("}");
    }

    /// Modifiers and the key of an object or class member. Returns the key
    /// when it is a plain identifier (a shorthand candidate).
    fn member_key(&mut self, scope: usize) -> Option<&'a str> {
        let mut modified = false;
        loop {
            match self.peek().map(|t| (t.kind, t.text(self.source))) {
                Some((Kind::Name, "get" | "set" | "async" | "static"))
                    if self.key_start(self.pos + 1) =>
                {
                    self.pos += 1;
                    modified = true;
                }
                Some((Kind::Punct, "*")) => {
                    self.pos += 1;
                    modified = true;
                }
                _ => break,
            }
        }
        match self.peek().copied() {
            Some(token) if token.kind == Kind::Punct && token.text(self.source) == "[" => {
                self.pos += 1;
                self.expression(scope, SEQUENCE);
                self.expect("]");
                None
            }
            Some(token)
                if matches!(
                    token.kind,
                    Kind::Name | Kind::Str | Kind::Number | Kind::Private
                ) =>
            {
                self.pos += 1;
                (token.kind == Kind::Name && !modified).then(|| token.text(self.source))
            }
            _ => {
                self.fail("expected a property name");
                None
            }
        }
    }

    fn object_literal(&mut self, scope: usize) {
        self.pos += 1; // {
        while self.pos < self.tokens.len() && !self.at_punct("}") {
            if self.eat("...") {
                self.expression(scope, ASSIGNMENT);
            } else {
                let shorthand = self.member_key(scope);
                if self.at_punct("(") {
                    self.function_rest(scope, None, true);
                } else if self.eat(":") {
                    self.expression(scope, ASSIGNMENT);
                } else {
                    match shorthand {
                        Some(name) if !reserved(name) => self.reference(name, scope),
                        _ => return self.fail("expected `:` or `(` after a property name"),
                    }
                    if self.eat("=") {
                        self.expression(scope, ASSIGNMENT);
                    }
                }
            }
            if !self.at_punct("}") {
                self.expect(",");
            }
        }
        self.expect("}");
    }

    /// Whether the token at `index` ends an operand, so that an operand on
    /// the next line starts a new statement (automatic semicolon insertion).
    fn ends_operand(&self, index: usize) -> bool {
        let token = &self.tokens[index];
        match token.kind {
            Kind::Name => {
                let word = token.text(self.source);
                !reserved(word) || matches!(word, "this" | "super" | "null" | "true" | "false")
            }
            Kind::Punct => matches!(token.text(self.source), ")" | "]" | "}"),
            Kind::TemplateHead | Kind::TemplateMiddle => false,
            _ => true,
        }
    }

    /// Scans an expression, recording references and nested scopes. Stops
    /// before a closing bracket, `;`, a `:` that closes no `?`, a statement
    /// keyword or an operand on a new line after an operand (automatic
    /// semicolon insertion) and, with `stops.comma`, `,`.
    fn expression(&mut self, scope: usize, stops: Stops) {
        let mut conditionals = 0usize;
        let start = self.pos;
        while let Some(token) = self.peek().copied() {
            let text = token.text(self.source);
            let starts_operand = match token.kind {
                Kind::Name => !matches!(text, "in" | "instanceof"),
                Kind::Punct | Kind::TemplateMiddle | Kind::TemplateTail => false,
                _ => true,
            };
            if token.newline_before
                && starts_operand
                && self.pos > start
                && self.ends_operand(self.pos - 1)
            {
                return;
            }
            match token.kind {
                Kind::Punct => match text {
                    ")" | "]" | "}" | ";" => return,
                    "," if stops.comma => return,
                    ":" if conditionals == 0 => return,
                    ":" => {
                        conditionals -= 1;
                        self.pos += 1;
                    }
                    "?" => {
                        conditionals += 1;
                        self.pos += 1;
                    }
                    "." | "?." => {
                        self.pos += 1;
                        if self
                            .peek()
                            .is_some_and(|t| matches!(t.kind, Kind::Name | Kind::Private))
                        {
                            self.pos += 1;
                        }
                    }
                    "(" if self.arrow_parameters_at(self.pos) => self.arrow(scope),
                    "(" | "[" => {
                        let close = self.partners[self.pos];
                        self.pos += 1;
                        self.expression(scope, SEQUENCE);
                        if self.pos != close {
                            return self.fail(format!(
                                "unexpected `{}`",
                                self.peek().map_or("", |t| t.text(self.source))
                            ));
                        }
                        self.pos += 1;
                    }
                    "{" => self.object_literal(scope),
                    _ => self.pos += 1,
                },
                Kind::TemplateHead => {
                    self.pos += 1;
                    loop {
                        self.expression(scope, SEQUENCE);
                        match self.peek().map(|t| t.kind) {
                            Some(Kind::TemplateMiddle) => self.pos += 1,
                            Some(Kind::TemplateTail) => {
                                self.pos += 1;
                                break;
                            }
                            _ => return self.fail("malformed template literal substitution"),
                        }
                    }
                }
                Kind::TemplateMiddle | Kind::TemplateTail => return,
                Kind::Name => match text {
                    "function" => {
                        self.pos += 1;
                        self.eat("*");
                        let own = self.word().filter(|word| !reserved(word));
                        self.pos += usize::from(own.is_some());
                        self.function_rest(scope, own, true);
                    }
                    "async"
                        if self.word_at(self.pos + 1) == Some("function")
                            && !self.tokens[self.pos + 1].newline_before =>
                    {
                        self.pos += 1;
                    }
                    "async"
                        if self.arrow_parameters_at(self.pos + 1)
                            || (self.word_at(self.pos + 1).is_some()
                                && self.punct_at(self.pos + 2) == Some("=>")) =>
                    {
                        self.pos += 1;
                        self.arrow(scope);
                    }
                    "class" => {
                        self.pos += 1;
                        let own = self
                            .word()
                            .filter(|word| !reserved(word) && *word != "extends");
                        self.pos += usize::from(own.is_some());
                        self.class_tail(scope, own);
                    }
                    "of" | "in" if stops.for_head => return,
                    word if ends_expression(word) => return,
                    word if reserved(word) => self.pos += 1,
                    _ if self.punct_at(self.pos + 1) == Some("=>") => self.arrow(scope),
                    word => {
                        self.reference(word, scope);
                        self.pos += 1;
                    }
                },
                _ => self.pos += 1,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(body: &str) -> (Vec<&str>, Vec<&str>, bool) {
        let tokens =
            lex(body, 0..body.len()).unwrap_or_else(|e| panic!("{} at {}", e.message, e.offset));
        let facts = analyze(body, &tokens)
            .unwrap_or_else(|(offset, message)| panic!("{message} at {offset}"));
        (
            facts.declares,
            facts.references,
            facts.has_effect_statements,
        )
    }

    #[test]
    fn locals_shadow_runtime_bindings_through_every_scope_kind() {
        let (declares, references, effect) = facts(
            "function f(a, {b, c: [d = e], ...g} = h, ...i) {\n\
               const j = k; let [l, , m] = n; var o;\n\
               for (const [p, q] of r) s(p, q);\n\
               for (let t = 0; t < u; t++) {}\n\
               for (v in w) {}\n\
               try { x(); } catch ({message}) { y(message); }\n\
               if (z) { function inner() {} inner(); }\n\
               return a + b + c + d + g + i + j + l + m + o + arguments.length;\n\
             }",
        );
        assert_eq!(declares, ["f"]);
        // `c:` in the parameter pattern is a key, so the body's `c` is free.
        assert_eq!(
            references,
            [
                "c", "e", "h", "k", "n", "r", "s", "u", "v", "w", "x", "y", "z"
            ]
        );
        assert!(!effect);
    }

    #[test]
    fn properties_keys_and_labels_are_not_references() {
        let (_, references, _) = facts(
            "const o = {a: b, c, [d]: e, f() { return this.g; }, get h() { return i?.j; }, 'k': l, async m() {}, *n() {}, ...p};\n\
             outer: for (;;) { break outer; }\n\
             class Q extends R.S { static T = U; #v = 1; w(x) { return x.#v + super.y; } static { z; } }",
        );
        assert_eq!(
            references,
            ["R", "U", "b", "c", "d", "e", "i", "l", "p", "z"]
        );
    }

    #[test]
    fn arrows_regexes_and_templates_scope_and_lex_correctly() {
        let (declares, references, effect) = facts(
            "const re = /[/{]\\/(a)/g, div = a / b / c;\n\
             const f = (x, y = z) => x + y, g = w => ({w, v}), h = async q => q, k = async (r) => { return r; };\n\
             const t = `a${m + `b${n}`}c${ {o}.o }`;\n\
             if (re) /x/.test(p);\n\
             run(u => u, typeof s === 'string' ? 1 : 2, x => x ? y : z2);",
        );
        assert_eq!(declares, ["re", "div", "f", "g", "h", "k", "t"]);
        assert_eq!(
            references,
            [
                "a", "b", "c", "m", "n", "o", "p", "run", "s", "v", "y", "z", "z2"
            ]
        );
        assert!(effect);
    }

    #[test]
    fn statements_without_semicolons_still_split_at_declarations() {
        let (declares, references, _) =
            facts("const a = b\nconst c = a\nfunction d() { return e }\n");
        assert_eq!(declares, ["a", "c", "d"]);
        assert_eq!(references, ["b", "e"]);
    }

    #[test]
    fn malformed_bodies_are_rejected() {
        for body in [
            "function f() {",
            "const a = );",
            "const s = 'open",
            "const r = /x",
            "a ? b : c : d;",
        ] {
            let parsed = lex(body, 0..body.len())
                .map_err(|e| (e.offset, e.message.to_string()))
                .and_then(|tokens| analyze(body, &tokens).map(|_| ()));
            assert!(parsed.is_err(), "{body}");
        }
    }

    const SAMPLE: &str = "// Original bounded runtime\n(() => {\n  \"use strict\";\n\n  //@unit a requires: b\n  const a = () => b;\n  //@unit b\n  function b() { return 1; }\n  //@unit b.extra effect: b requires: b\n  b.extra = true;\n\n  //@unit c effect requires:\n  //@   a\n  const c = {};\n  c.a = a;\n  //@exports\n  return Object.freeze({a, c});\n})()\n";

    #[test]
    fn manifests_round_trip_validate_and_reach_effects_through_targets() {
        let manifest = RuntimeManifest::parse(SAMPLE).unwrap();
        assert_eq!(manifest.annotated_text(), SAMPLE);
        assert_eq!(
            manifest.runtime_text(),
            SAMPLE
                .split_inclusive('\n')
                .filter(|line| !is_header_line(line))
                .collect::<String>()
        );
        manifest.validate().unwrap();
        assert_eq!(
            manifest.units.iter().map(|u| u.name).collect::<Vec<_>>(),
            ["a", "b", "b.extra", "c"]
        );
        assert_eq!(manifest.units[3].requires, ["a"]);
        assert_eq!(manifest.exports.names, ["a", "c"]);
        let b = manifest.unit_of("b").unwrap();
        assert_eq!(manifest.reachable([b]), [false, true, true, false]);
        assert_eq!(
            manifest.reachable(manifest.export_roots()),
            [true, true, true, true]
        );
        assert_eq!(
            manifest.with_computed_requires().replace(
                "  //@unit c effect requires: a\n",
                "  //@unit c effect requires:\n  //@   a\n"
            ),
            SAMPLE
        );
    }

    #[test]
    fn validation_reports_missing_stale_unknown_and_effect_errors() {
        let broken = SAMPLE
            .replace("//@unit a requires: b", "//@unit a requires: c")
            .replace(
                "//@unit b.extra effect: b requires: b",
                "//@unit b.extra requires: b",
            )
            .replace("b.extra = true;", "b.extra = missing;");
        let manifest = RuntimeManifest::parse(&broken).unwrap();
        let messages = manifest
            .validate()
            .unwrap_err()
            .into_iter()
            .map(|e| e.message)
            .collect::<Vec<_>>();
        assert_eq!(
            messages,
            [
                "unit `a`: references `b` (declared by unit `b`) but does not require it",
                "unit `a`: requires `c` but never references it",
                "unit `b.extra`: references `missing`, which is neither a runtime binding nor an ECMAScript global",
                "unit `b.extra`: has top-level statements other than declarations but is not marked `effect`",
            ]
        );
        let duplicate = SAMPLE.replace("//@unit b.extra", "//@unit a");
        assert!(RuntimeManifest::parse(&duplicate).is_err());
        let stray = SAMPLE.replace("  \"use strict\";\n", "  \"use strict\";\n  let hidden;\n");
        assert!(RuntimeManifest::parse(&stray).is_err());
    }
}
