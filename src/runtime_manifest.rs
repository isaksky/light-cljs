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
//! //@unit <name>[ compile-time][ mutator][ effect[: <binding>, ...[ when: <binding>, ...][ unless: <binding>, ...]] | install: <binding>, ...][ requires: <binding>, ...]
//! //@ <binding>, ...                    continues the header (any list wraps)
//! //@exports
//! ```
//!
//! A header line is any line whose first non-blank characters are `//@`.
//! - `<name>` is unique. A unit that declares bindings is named after the
//!   first one, so for most units the name is the binding. An effect unit
//!   that declares nothing has a free name, by convention `<target>.<what>`
//!   (`add.variadic`, `ListValue.members`, `core.seal`); it never collides
//!   with a binding.
//! - `requires:` lists exactly the top-level runtime bindings the body
//!   references that other units declare, in any order (the update below
//!   writes them sorted case-insensitively). ECMAScript globals
//!   ([`ECMASCRIPT_GLOBALS`]) are never listed; host globals are not allowed
//!   at all, and the runtime reaches the host through `globalThis`.
//! - `compile-time` marks machinery of the compiler, the macro host and the
//!   REPL, which reach it through the shared runtime's exports: the compiler
//!   form bridge, declaration demand and transactions, the namespace registry,
//!   the macro reader context, `inspect` and the `eval` stub. Application
//!   code reaches a compile-time unit only through reflection (`*ns*`,
//!   `find-ns`, the reader, `eval`, macros defined in the program), and an
//!   `Advanced` program that does so registers its namespaces
//!   ([`crate::runtime_slice`]). Compile-time hooks that application paths
//!   call (`demandProtocol`, `patchDeclaration`) are application units that
//!   start as plain versions, which the compile-time units replace.
//! - `mutator` marks a unit declaring a function whose calls mutate their
//!   arguments or a registry (`builtinType`, `installMethods`, `variadicCore`,
//!   `coreMethod`). Calling one while loading is an effect.
//! - `effect` marks a unit that runs code with an effect while loading:
//!   top-level statements other than declarations, or a call of a `mutator`
//!   outside every function body. Bare `effect` means the code mutates state
//!   the unit itself declares (or registers it, as each `coreMethod`
//!   dispatcher does), so it travels with the unit. `effect: a, b` names the
//!   bindings of other units whose state it mutates: the unit belongs to
//!   those bindings and must be kept whenever any of their units is kept (see
//!   [`RuntimeManifest::reachable`]); this is how a builtin type's prototype
//!   setup (`ListValue.type`), members (`ListValue.members`) and static fields
//!   (`ListValue.statics`) travel with its constructor. A unit that declares
//!   nothing names at least one target.
//! - `effect: a when: b, c` is kept only where a target is kept and so is one
//!   of the `when:` bindings: rapid type analysis for the runtime's own
//!   methods. Each record method (`recordMethods.hash`) belongs to
//!   `recordMethods`, which only a program with records keeps, and is needed
//!   only where an operation that dispatches its protocol method is kept
//!   (`when: hash, hashProtocol`). The same holds for a generic operation's
//!   branch for one kind of value, which belongs to the units that build the
//!   kind and is needed where the operation is kept: `makeVector.seq effect:
//!   makeVector when: seq` fills in `kindHooks.seqVector`; and for the
//!   protocol machinery the generic operations reach through `protocolHooks`,
//!   which belongs to `installMethods`. The `runtime_manifest` suite checks
//!   that the `when:` list of a method unit names every unit whose body
//!   dispatches one of its methods' keys, that of a hook install every unit
//!   reading the hook, and that a kind hook's targets are every unit writing
//!   to its kind's store.
//!   A `when:` list may also name an effect unit, `when: makeMap.equiv`,
//!   which is kept only where that effect is: a hook that the body of an
//!   effect reads is needed where the effect is kept, not wherever one of
//!   its own `when:` units is.
//! - `effect: a [when: b] unless: c, d` is kept where it would be without
//!   `unless:` but none of the `unless:` bindings' units is kept once
//!   everything else is: the leaner form of something those units give in
//!   full, such as the members of map entries where no vector can be built
//!   (`MapEntryType.lean effect: MapEntryType unless: makeVector`). The
//!   shared runtime keeps both, so an `unless:` unit must be harmless where
//!   the full form is kept too: the full form follows it and replaces what
//!   it installs, or installs with `??=` what it would not replace.
//! - `install: a, b` is an effect that only adds properties to the targets'
//!   values for code that receives them, such as the variadic arity ABI of a
//!   core function (`add.variadic`), which `apply` reads. It is kept once a
//!   target's value escapes: when a kept unit requires the target, or the
//!   program uses it as a value rather than only calling it
//!   ([`RuntimeManifest::reached_by_uses`]). An install declares nothing.
//!
//! `coreMethod("<key>", ...)` registers a core protocol dispatcher in
//! `coreDispatchers`, where `extendType` looks it up by key to patch it. The
//! manifest lists every registration ([`RuntimeManifest::core_dispatcher`]),
//! so a program that extends a core protocol method keeps its dispatcher; any
//! use of `coreMethod` other than `const <binding> = coreMethod("<key>", ...)`
//! is a parse error.
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
//! effects: a unit with load-time effects (statements or `mutator` calls) that
//! is not marked `effect`, and an `effect` unit without them. The
//! `runtime_manifest` suite in `tests/compiler/` runs both on the
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
//! `build.rs` also writes every unit terse, with short names, in manifest
//! order, which terse `Advanced` output embeds instead of the bodies here
//! (`src/terse.rs`); the lexer and scope analysis both use are
//! `src/js_syntax.rs`.

use std::collections::HashMap;
use std::fmt;
use std::ops::Range;
use std::sync::OnceLock;

pub use crate::js_syntax::ECMASCRIPT_GLOBALS;
use crate::js_syntax::{Kind, Token, analyze, lex, lex_section};

/// `src/js_runtime.js` as written, including unit headers.
pub const ANNOTATED_RUNTIME: &str = include_str!("js_runtime.js");

/// The shared runtime factory as embedded in output: [`ANNOTATED_RUNTIME`]
/// without its header lines (written by `build.rs`).
pub const RUNTIME: &str = include_str!(concat!(env!("OUT_DIR"), "/js_runtime.js"));

/// [`RUNTIME`] as the compile-time evaluators run it: with hooks only the
/// compiler uses, which output never embeds (written by `build.rs`; see
/// `compiler_runtime` there).
pub const COMPILER_RUNTIME: &str =
    include_str!(concat!(env!("OUT_DIR"), "/js_compiler_runtime.js"));

/// Dense index of a unit in [`RuntimeManifest::units`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnitId(pub usize);

/// Code a unit runs at load time other than declaring bindings (top-level
/// statements, or calls of a [`Unit::mutator`]), and whose state it mutates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Effect<'a> {
    /// Bindings of other units the statements belong to. Empty when they
    /// mutate the unit's own declarations.
    pub targets: Vec<&'a str>,
    /// `install:` instead of `effect:`: the statements only add properties to
    /// the targets' values that code receiving those values may read (the
    /// variadic arity ABI of a core function), so they are needed where a
    /// target's value escapes, not where it is only called
    /// ([`RuntimeManifest::reached_by_uses`]).
    pub install: bool,
    /// `effect: a when: b, c`: bindings of which one must be kept as well,
    /// the operations that can call what the effect installs, or effect units
    /// (`makeMap.equiv`) whose bodies can. Empty for an effect needed wherever
    /// a target is.
    pub when: Vec<&'a str>,
    /// `effect: a unless: b, c`: bindings none of whose units may be kept
    /// once everything else is, the full form of what the effect gives
    /// leanly. Empty for an effect kept whatever else is.
    pub unless: Vec<&'a str>,
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
    /// Marked `compile-time`: machinery of the compiler, macro host and REPL
    /// that application code reaches only through reflection.
    pub compile_time: bool,
    /// Marked `mutator`: calling the function the unit declares mutates its
    /// arguments or a registry, so a unit that calls it at load time has an
    /// effect.
    pub mutator: bool,
    pub effect: Option<Effect<'a>>,
    /// Top-level bindings the body declares, in source order.
    pub declares: Vec<&'a str>,
    /// Free identifiers of the body: other units' bindings and globals,
    /// sorted and without duplicates. Excludes [`Unit::declares`].
    pub references: Vec<&'a str>,
    /// The [`Unit::references`] called directly (`name(...)`) by code run at load
    /// time, outside every function body, sorted and without duplicates.
    pub load_time_calls: Vec<&'a str>,
    /// The [`Unit::references`] the body assigns, `seq = behindProtocol(seq, ...)`:
    /// another unit's binding that this unit replaces (a wrapper effect),
    /// sorted and without duplicates.
    pub assigns: Vec<&'a str>,
    /// The top-level bindings, its own or other units', that a function of the
    /// body assigns, so that their values change after the runtime loads,
    /// sorted and without duplicates.
    pub call_time_assigns: Vec<&'a str>,
    /// The [`Unit::references`] whose list the body appends to at load time,
    /// `recordMethodTable.push(...)`: a table another unit may enumerate.
    pub load_time_appends: Vec<&'a str>,
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

/// The unit declaring the mutable cells of `cljs.core`: dynamic Vars, host
/// hooks and the operations the runtime itself reads through a cell.
pub const CORE_CELLS: &str = "coreCells";
/// The unit declaring `core`, the map of every core operation by runtime name
/// that the shared runtime exports.
pub const CORE_HUB: &str = "core";

/// What `rt.core.<name>` holds, as a direct reference
/// ([`RuntimeManifest::core_reference`]). Advanced output reads core
/// operations this way instead of through the `core` hub.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CoreReference<'a> {
    /// A runtime binding holding the operation: `add`, or `keywordCore` for
    /// `keyword`.
    Binding(&'a str),
    /// A member of a runtime binding: `reader.readString`.
    Member(&'a str, &'a str),
    /// The cell [`CORE_CELLS`]`.<name>`, which the hub shares: reads and
    /// writes through either reach the runtime's own reads.
    Cell(&'a str),
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
    /// The core table, by runtime name; empty without a [`CORE_HUB`] unit.
    core: HashMap<&'a str, CoreReference<'a>>,
    /// The core protocol dispatchers, binding by protocol method key.
    dispatchers: HashMap<&'a str, &'a str>,
    /// Per unit: the units declaring its required bindings, sorted.
    dependencies: Vec<Vec<UnitId>>,
    /// Per unit: the `effect:` units targeting one of its bindings.
    attached: Vec<Vec<UnitId>>,
    /// Per unit: the `install:` units targeting one of its bindings.
    installs: Vec<Vec<UnitId>>,
    /// Per unit: the units declaring its `when:` bindings, or named by it,
    /// sorted.
    conditions: Vec<Vec<UnitId>>,
    /// Per unit: the `effect: ... when:` units naming one of its bindings
    /// under `when:`.
    conditional: Vec<Vec<UnitId>>,
    /// Per unit: the other units that share state through one of its
    /// bindings ([`RuntimeManifest::sharers`]), sorted.
    sharers: Vec<Vec<UnitId>>,
    /// Per unit: the units whose bindings it shares, the inverse of
    /// `sharers`, sorted.
    shared: Vec<Vec<UnitId>>,
    /// Per unit: the units declaring its `unless:` bindings, sorted.
    blockers: Vec<Vec<UnitId>>,
    /// The `effect: ... unless:` units, in source order.
    lean: Vec<UnitId>,
}

/// How a program uses a root of a runtime slice
/// ([`RuntimeManifest::reached_by_uses`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RootUse {
    /// The program holds the binding's value, which may escape to code that
    /// reads its installed properties.
    Value,
    /// The program only calls the binding directly, `rt$add(1, 2)`, so the
    /// `install:` units targeting it are not needed.
    Call,
}

/// The function whose calls register core protocol dispatchers by key
/// ([`RuntimeManifest::core_dispatcher`]).
pub const CORE_METHOD: &str = "coreMethod";

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
                    compile_time,
                    mutator,
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
                            compile_time,
                            mutator,
                            effect,
                            declares: facts.declares,
                            references: facts.references,
                            load_time_calls: facts.load_time_calls,
                            assigns: facts.assigns,
                            call_time_assigns: facts.call_time_assigns,
                            load_time_appends: facts.load_time_appends,
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
        let core = match parse_core_table(source, &units, &bindings) {
            Ok(core) => core,
            Err((offset, message)) => {
                errors.push(ManifestError {
                    line: line_of(offset),
                    message,
                });
                HashMap::new()
            }
        };
        let dispatchers = match parse_dispatchers(source, &units) {
            Ok(dispatchers) => dispatchers,
            Err(found) => {
                errors.extend(found.into_iter().map(|(offset, message)| ManifestError {
                    line: line_of(offset),
                    message,
                }));
                HashMap::new()
            }
        };
        let Some(exports) = exports.filter(|_| errors.is_empty()) else {
            return Err(errors);
        };
        let mut dependencies = Vec::with_capacity(units.len());
        let mut attached = vec![Vec::new(); units.len()];
        let mut installs = vec![Vec::new(); units.len()];
        let mut conditions = vec![Vec::new(); units.len()];
        let mut conditional = vec![Vec::new(); units.len()];
        let mut blockers = vec![Vec::new(); units.len()];
        for (index, unit) in units.iter().enumerate() {
            let mut required = unit
                .requires
                .iter()
                .filter_map(|binding| bindings.get(binding).copied())
                .collect::<Vec<_>>();
            required.sort_unstable();
            required.dedup();
            dependencies.push(required);
            if let Some(effect) = &unit.effect {
                for target in &effect.targets {
                    if let Some(owner) = bindings.get(target) {
                        let list = if effect.install {
                            &mut installs
                        } else {
                            &mut attached
                        };
                        list[owner.0].push(UnitId(index));
                    }
                }
                for binding in &effect.when {
                    let owner = bindings
                        .get(binding)
                        .copied()
                        .or_else(|| names.get(binding).map(|&unit| UnitId(unit)));
                    if let Some(owner) = owner {
                        conditions[index].push(owner);
                        conditional[owner.0].push(UnitId(index));
                    }
                }
                conditions[index].sort_unstable();
                conditions[index].dedup();
                blockers[index] = effect
                    .unless
                    .iter()
                    .filter_map(|binding| bindings.get(binding).copied())
                    .collect();
                blockers[index].sort_unstable();
                blockers[index].dedup();
            }
        }
        // A binding another unit assigns is shared with that unit, and one a
        // function assigns after loading with every unit that reads it. So is
        // a list another unit appends to when it loads: a reader that
        // enumerates the list (`recordMethods`, building a record type's
        // methods when the type is defined) sees only what is appended
        // before, so an append in a later chunk would come too late.
        let mut sharers = vec![Vec::new(); units.len()];
        let mut shared = vec![Vec::new(); units.len()];
        let mut share = |owner: UnitId, unit: usize| {
            if owner.0 != unit {
                sharers[owner.0].push(UnitId(unit));
                shared[unit].push(owner);
            }
        };
        for (index, unit) in units.iter().enumerate() {
            for binding in unit.assigns.iter().chain(&unit.load_time_appends) {
                if let Some(&owner) = bindings.get(binding) {
                    share(owner, index);
                }
            }
        }
        let volatile = units
            .iter()
            .flat_map(|unit| unit.call_time_assigns.iter().copied())
            .filter(|binding| bindings.contains_key(binding))
            .collect::<std::collections::HashSet<_>>();
        for (index, unit) in units.iter().enumerate() {
            for binding in &unit.references {
                if volatile.contains(binding) {
                    share(bindings[binding], index);
                }
            }
        }
        for list in sharers.iter_mut().chain(shared.iter_mut()) {
            list.sort_unstable();
            list.dedup();
        }
        let names = names
            .into_iter()
            .map(|(name, index)| (name, UnitId(index)))
            .collect();
        let lean = (0..units.len())
            .filter(|&index| !blockers[index].is_empty())
            .map(UnitId)
            .collect();
        Ok(Self {
            source,
            prologue: 0..prologue_end,
            units,
            exports,
            bindings,
            names,
            core,
            dispatchers,
            dependencies,
            attached,
            installs,
            conditions,
            conditional,
            sharers,
            shared,
            blockers,
            lean,
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

    /// What the shared runtime's `rt.core.<name>` holds, for a runtime name of
    /// a [`CoreFnId`](crate::CoreFnId) or another key of the [`CORE_HUB`].
    pub fn core_reference(&self, name: &str) -> Option<CoreReference<'a>> {
        self.core.get(name).copied()
    }

    /// Every key of the [`CORE_HUB`] with its reference, unordered.
    pub fn core_references(&self) -> impl Iterator<Item = (&'a str, CoreReference<'a>)> + '_ {
        self.core
            .iter()
            .map(|(&name, &reference)| (name, reference))
    }

    /// Units declaring the bindings `unit` requires, sorted, without duplicates.
    pub fn dependencies(&self, unit: UnitId) -> &[UnitId] {
        &self.dependencies[unit.0]
    }

    /// `effect:` units that target one of `unit`'s bindings, in source order.
    pub fn attached_effects(&self, unit: UnitId) -> &[UnitId] {
        &self.attached[unit.0]
    }

    /// `install:` units that target one of `unit`'s bindings, in source order.
    pub fn attached_installs(&self, unit: UnitId) -> &[UnitId] {
        &self.installs[unit.0]
    }

    /// The units declaring the `when:` bindings of an `effect: ... when:`
    /// unit, sorted; empty for any other unit.
    pub fn conditions(&self, unit: UnitId) -> &[UnitId] {
        &self.conditions[unit.0]
    }

    /// The units that share mutable state through one of `unit`'s bindings,
    /// sorted: each unit that assigns one (a wrapper effect, `seq =
    /// behindProtocol(seq, ...)`, or a function that bumps a counter), and,
    /// for a binding some function assigns after the runtime loads, each unit
    /// that reads it. One closure holds them all in a single runtime; a
    /// chunked build keeps them in `unit`'s chunk
    /// ([`RuntimeManifest::signatures`]), since a chunk's runtime receives
    /// other chunks' bindings as values, which an assignment cannot reach.
    pub fn sharers(&self, unit: UnitId) -> &[UnitId] {
        &self.sharers[unit.0]
    }

    /// The units declaring the `unless:` bindings of an `effect: ... unless:`
    /// unit, sorted; empty for any other unit.
    pub fn blockers(&self, unit: UnitId) -> &[UnitId] {
        &self.blockers[unit.0]
    }

    /// The binding of the core protocol dispatcher registered for a protocol
    /// method key such as `"ICounted/-count"`: the `<binding>` of
    /// `const <binding> = coreMethod("<key>", ...)`. `extendType` patches the
    /// dispatcher it finds in `coreDispatchers` by key, so a program extending
    /// a core protocol keeps the dispatcher's unit.
    pub fn core_dispatcher(&self, key: &str) -> Option<&'a str> {
        self.dispatchers.get(key).copied()
    }

    /// Whether `binding` is a core protocol dispatcher
    /// ([`RuntimeManifest::core_dispatcher`]).
    pub fn is_core_dispatcher(&self, binding: &str) -> bool {
        self.dispatchers
            .values()
            .any(|&dispatcher| dispatcher == binding)
    }

    /// Whether `binding` is a core protocol (`ICounted`), one of those
    /// `coreProtocolNames` names. Its object is the protocol's type table,
    /// which `extend-type` of a host type writes (`ICounted.number = true`).
    pub fn is_core_protocol(&self, binding: &str) -> bool {
        self.unit_named("coreProtocolNames")
            .is_some_and(|unit| self.units[unit.0].requires.contains(&binding))
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

    /// The units a program needs when it holds the values of `roots`, indexed
    /// by [`UnitId`]: the closure under `requires`, where keeping a unit also
    /// keeps every `effect:` and `install:` unit that targets one of its
    /// bindings. [`RuntimeManifest::reached_by_uses`] distinguishes roots the
    /// program only calls.
    pub fn reachable(&self, roots: impl IntoIterator<Item = UnitId>) -> Vec<bool> {
        let roots = roots
            .into_iter()
            .map(|unit| (unit, RootUse::Value))
            .collect::<Vec<_>>();
        self.reached_by_uses(&roots)
            .into_iter()
            .map(|first| first.is_some())
            .collect()
    }

    /// [`RuntimeManifest::reachable`] with attribution, indexed by [`UnitId`]:
    /// the position in `roots` of the first root whose closure contains the
    /// unit, or `None` for a unit no root keeps. The kept units are the same.
    pub fn reached_by(&self, roots: &[UnitId]) -> Vec<Option<usize>> {
        let roots = roots
            .iter()
            .map(|&unit| (unit, RootUse::Value))
            .collect::<Vec<_>>();
        self.reached_by_uses(&roots)
    }

    /// The units a program needs, with attribution as in
    /// [`RuntimeManifest::reached_by`], when it uses each root as given. A
    /// unit's `effect:` units are kept with it, those with `when:` once one of
    /// their `when:` units is kept too. Its `install:` units are kept once its
    /// value escapes: when it is a [`RootUse::Value`] root or another kept unit
    /// requires it, since the runtime may pass a binding it references
    /// anywhere. A [`RootUse::Call`] root alone keeps none. An effect with
    /// `unless:` is kept last, where it would be kept without `unless:` and
    /// none of its `unless:` units is kept, and is attributed as a target.
    pub fn reached_by_uses(&self, roots: &[(UnitId, RootUse)]) -> Vec<Option<usize>> {
        let mut first = vec![None; self.units.len()];
        let mut escaped = vec![false; self.units.len()];
        let mut stack = Vec::new();
        for (index, &(root, root_use)) in roots.iter().enumerate() {
            // A unit an earlier root reached has its whole closure attributed.
            stack.push((root, root_use == RootUse::Value));
            self.close(&mut stack, index, &mut first, &mut escaped);
        }
        loop {
            let kept = |unit: &UnitId| first[unit.0].is_some();
            let ready = self
                .lean
                .iter()
                .filter(|&&effect| !kept(&effect) && self.lean_ready(effect, kept))
                .filter_map(|&effect| {
                    let targets = &self.units[effect.0].effect.as_ref().unwrap().targets;
                    targets
                        .iter()
                        .filter_map(|target| first[self.unit_of(target)?.0])
                        .min()
                        .map(|index| (effect, index))
                })
                .collect::<Vec<_>>();
            if ready.is_empty() {
                return first;
            }
            for (effect, index) in ready {
                // One kept before it may have kept what it waits to be without.
                if self.blockers[effect.0]
                    .iter()
                    .any(|unit| first[unit.0].is_some())
                {
                    continue;
                }
                stack.push((effect, true));
                self.close(&mut stack, index, &mut first, &mut escaped);
            }
        }
    }

    /// Whether an `effect: ... unless:` unit may be kept once everything else
    /// is: one of its `when:` units (if any) is kept and none of its `unless:`
    /// units.
    fn lean_ready(&self, effect: UnitId, kept: impl Fn(&UnitId) -> bool) -> bool {
        let conditions = &self.conditions[effect.0];
        (conditions.is_empty() || conditions.iter().any(&kept))
            && !self.blockers[effect.0].iter().any(&kept)
    }

    /// Keeps the closure of the units on `stack` for [`Self::reached_by_uses`],
    /// attributing new units to the root at `index`.
    fn close(
        &self,
        stack: &mut Vec<(UnitId, bool)>,
        index: usize,
        first: &mut [Option<usize>],
        escaped: &mut [bool],
    ) {
        while let Some((unit, escapes)) = stack.pop() {
            if escapes && !std::mem::replace(&mut escaped[unit.0], true) {
                stack.extend(self.installs[unit.0].iter().map(|&install| (install, true)));
            }
            if first[unit.0].is_some() {
                continue;
            }
            first[unit.0] = Some(index);
            stack.extend(self.dependencies[unit.0].iter().map(|&unit| (unit, true)));
            let kept = |unit: &UnitId| first[unit.0].is_some();
            // An effect with `when:` waits for one of its conditions; one with
            // `unless:` for everything else.
            stack.extend(
                self.attached[unit.0]
                    .iter()
                    .filter(|effect| {
                        let conditions = &self.conditions[effect.0];
                        self.blockers[effect.0].is_empty()
                            && (conditions.is_empty() || conditions.iter().any(kept))
                    })
                    .map(|&unit| (unit, true)),
            );
            // ... and is kept once a condition is, if a target is.
            for &effect in &self.conditional[unit.0] {
                let targets = &self.units[effect.0].effect.as_ref().unwrap().targets;
                if self.blockers[effect.0].is_empty()
                    && targets
                        .iter()
                        .any(|target| self.unit_of(target).is_some_and(|owner| kept(&owner)))
                {
                    stack.push((effect, true));
                }
            }
        }
    }

    /// The roots that need each unit when a program's demands are split among
    /// roots (docs/code-splitting.md, "Phase 3", runtime units), indexed by
    /// [`UnitId`]. Each demand is a unit, its use, and the roots whose code
    /// makes it, a bitset of `words` words; a unit's roots are its
    /// *signature*, empty for a unit no demand keeps. The roots of a unit are
    /// those whose demands alone [`RuntimeManifest::reached_by_uses`] keeps it
    /// for, so a single root's demands keep exactly the units with its bit,
    /// and all demands together the units with any: requires, `effect:`
    /// units and installs follow as there, an install with the roots whose
    /// code holds its target's value. One more rule makes the slices of
    /// several roots add up to the slice of all of them: an `effect: ...
    /// when:` unit is needed by every root that needs a target once a root
    /// that needs none of its targets needs a condition, since the values a
    /// root builds can reach code that another root loads (`makeVector.seq`
    /// for a feature that seqs a vector another feature built). And a kept
    /// unit that shares state through another unit's binding
    /// ([`RuntimeManifest::sharers`]) has that unit's roots, so that both are
    /// in one chunk. Neither rule keeps a unit no single root keeps alone or
    /// with the others.
    pub fn signatures(&self, demands: &[(UnitId, RootUse, &[u64])], words: usize) -> Vec<Vec<u64>> {
        /// The roots that need each unit and that hold its value, and the
        /// units whose roots grew since they were last followed.
        struct Walk {
            needed: Vec<Vec<u64>>,
            escaped: Vec<Vec<u64>>,
            queued: Vec<bool>,
            queue: std::collections::VecDeque<UnitId>,
        }
        fn add(into: &mut [u64], from: &[u64]) -> bool {
            let mut changed = false;
            for (word, more) in into.iter_mut().zip(from) {
                changed |= *more & !*word != 0;
                *word |= *more;
            }
            changed
        }
        impl Walk {
            /// Add `roots` to those that need `unit`, and to those that hold
            /// its value when `value`.
            fn grow(&mut self, unit: UnitId, roots: &[u64], value: bool) {
                let mut changed = add(&mut self.needed[unit.0], roots);
                if value {
                    changed |= add(&mut self.escaped[unit.0], roots);
                }
                if changed && !std::mem::replace(&mut self.queued[unit.0], true) {
                    self.queue.push_back(unit);
                }
            }
        }
        let count = self.units.len();
        let mut walk = Walk {
            needed: vec![vec![0u64; words]; count],
            escaped: vec![vec![0u64; words]; count],
            queued: vec![false; count],
            queue: std::collections::VecDeque::new(),
        };
        for &(unit, root_use, roots) in demands {
            debug_assert_eq!(roots.len(), words);
            walk.grow(unit, roots, root_use == RootUse::Value);
        }
        // An `effect: ... when:` unit's roots given its targets' and
        // conditions' (see above).
        let conditional = |needed: &[Vec<u64>], effect: UnitId| {
            let mut targets = vec![0u64; words];
            for target in &self.units[effect.0].effect.as_ref().unwrap().targets {
                if let Some(owner) = self.unit_of(target) {
                    add(&mut targets, &needed[owner.0]);
                }
            }
            let mut conditions = vec![0u64; words];
            for condition in &self.conditions[effect.0] {
                add(&mut conditions, &needed[condition.0]);
            }
            let elsewhere = conditions
                .iter()
                .zip(&targets)
                .any(|(condition, target)| condition & !target != 0);
            targets
                .iter()
                .zip(&conditions)
                .map(|(target, condition)| {
                    if elsewhere {
                        *target
                    } else {
                        target & condition
                    }
                })
                .collect::<Vec<_>>()
        };
        loop {
            while let Some(unit) = walk.queue.pop_front() {
                walk.queued[unit.0] = false;
                let roots = walk.needed[unit.0].clone();
                let values = walk.escaped[unit.0].clone();
                for &dependency in &self.dependencies[unit.0] {
                    walk.grow(dependency, &roots, true);
                }
                for &install in &self.installs[unit.0] {
                    walk.grow(install, &values, true);
                }
                for &effect in &self.attached[unit.0] {
                    if !self.blockers[effect.0].is_empty() {
                        continue;
                    }
                    let roots = if self.conditions[effect.0].is_empty() {
                        roots.clone()
                    } else {
                        conditional(&walk.needed, effect)
                    };
                    walk.grow(effect, &roots, true);
                }
                for &effect in &self.conditional[unit.0] {
                    if !self.blockers[effect.0].is_empty() {
                        continue;
                    }
                    let roots = conditional(&walk.needed, effect);
                    walk.grow(effect, &roots, true);
                }
                // A kept unit sharing state through another's binding has its
                // roots ([`RuntimeManifest::sharers`]).
                for &sharer in &self.sharers[unit.0] {
                    if walk.needed[sharer.0].iter().any(|&word| word != 0) {
                        walk.grow(sharer, &roots, true);
                    }
                }
                for &owner in &self.shared[unit.0] {
                    let roots = walk.needed[owner.0].clone();
                    walk.grow(unit, &roots, true);
                }
            }
            // An `effect: ... unless:` unit waits for everything else and is kept
            // only where no root needs one of its `unless:` units, with the roots
            // it would have without `unless:`.
            let needed =
                |walk: &Walk, unit: &UnitId| walk.needed[unit.0].iter().any(|&word| word != 0);
            for &effect in &self.lean {
                if !self.lean_ready(effect, |unit| needed(&walk, unit)) {
                    continue;
                }
                let roots = if self.conditions[effect.0].is_empty() {
                    let mut targets = vec![0u64; words];
                    for target in &self.units[effect.0].effect.as_ref().unwrap().targets {
                        if let Some(owner) = self.unit_of(target) {
                            add(&mut targets, &walk.needed[owner.0]);
                        }
                    }
                    targets
                } else {
                    conditional(&walk.needed, effect)
                };
                walk.grow(effect, &roots, true);
            }
            if walk.queue.is_empty() {
                return walk.needed;
            }
        }
    }

    /// A runtime factory with only the `kept` units (indexed by [`UnitId`]):
    /// the prologue, the kept bodies in `UnitId` order, so initialization order
    /// is the full runtime's, and an export section that freezes `exports` in
    /// the given order. `kept` must be closed under
    /// [`RuntimeManifest::reachable`] and every export must be a kept binding.
    /// Keeping every unit and exporting [`Exports::names`] gives
    /// [`RuntimeManifest::runtime_text`].
    pub fn sliced_text(&self, kept: &[bool], exports: &[&str]) -> String {
        self.slice_with_return(kept, exports, "Object.freeze({", "})")
    }

    /// [`RuntimeManifest::sliced_text`] for direct reads (Advanced output):
    /// the factory returns `bindings`, which may be any kept bindings, as an
    /// array in the given order, `return [add, symbol];`, for the output to
    /// destructure.
    pub fn sliced_bindings_text(&self, kept: &[bool], bindings: &[&str]) -> String {
        self.slice_with_return(kept, bindings, "[", "]")
    }

    /// The runtime of one chunk of a chunked build (docs/code-splitting.md,
    /// "What a chunk file contains"): a function of the bindings its `kept`
    /// units require that other chunks declare, `parameters`, which returns
    /// `returned` as an array, `((keywords, hash) => { ... return [compare];
    /// })`, for the chunk to call with its imports of them. The bodies are as
    /// written, in `UnitId` order; a chunk is a module, so the function needs
    /// no `"use strict"`. Without `returned` it returns nothing.
    pub fn chunk_factory_text(
        &self,
        kept: &[bool],
        parameters: &[&str],
        returned: &[&str],
    ) -> String {
        debug_assert_eq!(kept.len(), self.units.len());
        let mut text = format!("(({}) => {{\n", parameters.join(", "));
        for (unit, _) in self.units.iter().zip(kept).filter(|(_, kept)| **kept) {
            text.push_str(&self.source[unit.body_range.clone()]);
        }
        if !returned.is_empty() {
            text.push_str("  return [");
            text.push_str(&returned.join(", "));
            text.push_str("];\n");
        }
        text.push_str("})");
        text
    }

    fn slice_with_return(&self, kept: &[bool], names: &[&str], open: &str, close: &str) -> String {
        debug_assert_eq!(kept.len(), self.units.len());
        debug_assert!(
            names
                .iter()
                .all(|name| self.unit_of(name).is_some_and(|unit| kept[unit.0])),
            "every binding a runtime slice returns must be kept: {names:?}"
        );
        let mut text = String::new();
        text.push_str(&self.source[self.prologue.clone()]);
        for (unit, _) in self.units.iter().zip(kept).filter(|(_, kept)| **kept) {
            text.push_str(&self.source[unit.body_range.clone()]);
        }
        text.push_str("  return ");
        text.push_str(open);
        text.push_str(&names.join(", "));
        text.push_str(close);
        text.push_str(";\n})()\n");
        text
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
            if unit.mutator && unit.declares.is_empty() {
                fail("is marked `mutator` but declares no function".into());
            }
            // Mutators the unit calls while loading.
            let mutators = unit
                .load_time_calls
                .iter()
                .filter(|reference| {
                    self.unit_of(reference)
                        .is_some_and(|owner| owner != id && self.units[owner.0].mutator)
                })
                .collect::<Vec<_>>();
            match &unit.effect {
                None if unit.has_effect_statements => fail(
                    "has top-level statements other than declarations but is not marked `effect`"
                        .into(),
                ),
                None if !mutators.is_empty() => fail(format!(
                    "calls mutator `{}` while loading but is not marked `effect`",
                    mutators[0]
                )),
                Some(_) if !unit.has_effect_statements && mutators.is_empty() => fail(
                    "is marked `effect` but only declares bindings and calls no mutator".into(),
                ),
                Some(effect) if effect.install && !unit.declares.is_empty() => {
                    fail("is marked `install:` but declares bindings".into())
                }
                Some(effect) => {
                    if effect.targets.is_empty() && unit.declares.is_empty() {
                        fail(
                            "declares nothing, so its `effect` must name the bindings it mutates"
                                .into(),
                        );
                    }
                    for (position, &binding) in effect.when.iter().enumerate() {
                        if effect.when[..position].contains(&binding) {
                            fail(format!("names `when:` binding `{binding}` twice"));
                        } else if self.unit_of(binding).is_none()
                            && self.unit_named(binding).is_none_or(|unit| {
                                self.units[unit.0]
                                    .effect
                                    .as_ref()
                                    .is_none_or(|effect| effect.targets.is_empty())
                            })
                        {
                            fail(format!(
                                "`when:` binding `{binding}` is neither a runtime binding nor an effect unit"
                            ));
                        }
                    }
                    // What the unit keeps once it is kept itself.
                    let closure = (!effect.unless.is_empty()).then(|| self.reachable([id]));
                    for (position, &binding) in effect.unless.iter().enumerate() {
                        let owner = self.unit_of(binding);
                        if effect.unless[..position].contains(&binding) {
                            fail(format!("names `unless:` binding `{binding}` twice"));
                        } else if self.unit_of(binding).is_none() {
                            fail(format!(
                                "`unless:` binding `{binding}` is not a runtime binding"
                            ));
                        } else if unit.requires.contains(&binding) {
                            fail(format!(
                                "requires its `unless:` binding `{binding}`, so it is never kept"
                            ));
                        } else if owner.is_some_and(|owner| closure.as_ref().unwrap()[owner.0]) {
                            fail(format!(
                                "keeps its `unless:` binding `{binding}` through what it requires"
                            ));
                        }
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
            if unit.compile_time {
                words.push("compile-time".into());
            }
            if unit.mutator {
                words.push("mutator".into());
            }
            // A keyword, then its list's items with their commas.
            let mut list = |keyword: &str, items: &[&str]| {
                words.push(keyword.into());
                for (position, item) in items.iter().enumerate() {
                    words.push(if position + 1 < items.len() {
                        format!("{item},")
                    } else {
                        item.to_string()
                    });
                }
            };
            match &unit.effect {
                Some(effect) if effect.targets.is_empty() => list("effect", &[]),
                Some(effect) => {
                    list(
                        if effect.install {
                            "install:"
                        } else {
                            "effect:"
                        },
                        &effect.targets,
                    );
                    if !effect.when.is_empty() {
                        list("when:", &effect.when);
                    }
                    if !effect.unless.is_empty() {
                        list("unless:", &effect.unless);
                    }
                }
                None => {}
            }
            let required = self.computed_requires(UnitId(index));
            if !required.is_empty() {
                list("requires:", &required);
            }
            // Wrap before a word that would pass column 100, but never right
            // after a keyword.
            let mut line = format!("{indent}{}", words[0]);
            for word in &words[1..] {
                if line.len() + 1 + word.len() > 100 && !line.trim_end().ends_with(':') {
                    text.push_str(&line);
                    text.push('\n');
                    line = format!("{indent}//@");
                }
                line.push(' ');
                line.push_str(word);
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
        compile_time: bool,
        mutator: bool,
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

/// The words of a header that end its lists.
const HEADER_KEYWORDS: [&str; 5] = ["effect:", "install:", "when:", "unless:", "requires:"];

/// A comma-separated header list: items up to one without a trailing comma,
/// across continuation lines.
fn header_list<'a>(
    words: &mut std::iter::Peekable<impl Iterator<Item = &'a str>>,
    name: &str,
    what: &str,
    units: bool,
) -> Result<Vec<&'a str>, String> {
    let mut items = Vec::new();
    while let Some(word) = words.next_if(|word| !HEADER_KEYWORDS.contains(word)) {
        for item in word.split(',').filter(|item| !item.is_empty()) {
            // A `when:` list may name an effect unit (`makeMap.equiv`).
            if !(is_identifier(item) || units && item.split('.').all(is_identifier)) {
                return Err(format!("unit `{name}`: invalid {what} `{item}`"));
            }
            items.push(item);
        }
        if !word.ends_with(',') {
            break;
        }
    }
    Ok(items)
}

fn parse_unit_header<'a>(first: &'a str, continued: &[&'a str]) -> Result<Header<'a>, String> {
    // Continuation lines continue the header, so that any list can wrap.
    let mut words = std::iter::once(first)
        .chain(continued.iter().copied())
        .flat_map(str::split_whitespace)
        .peekable();
    let name = words.next().unwrap_or_default();
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '$' | '.' | '-'))
    {
        return Err(format!("invalid unit name `{name}`"));
    }
    let compile_time = words.next_if_eq(&"compile-time").is_some();
    let mutator = words.next_if_eq(&"mutator").is_some();
    let mut effect = None;
    if words.next_if_eq(&"effect").is_some() {
        effect = Some(Effect {
            targets: Vec::new(),
            install: false,
            when: Vec::new(),
            unless: Vec::new(),
        });
    } else if let Some(keyword) = words.next_if(|word| matches!(*word, "effect:" | "install:")) {
        let install = keyword == "install:";
        let keyword = &keyword[..keyword.len() - 1];
        let targets = header_list(&mut words, name, &format!("{keyword} target"), false)?;
        if targets.is_empty() {
            return Err(format!("unit `{name}`: `{keyword}:` names no binding"));
        }
        effect = Some(Effect {
            targets,
            install,
            when: Vec::new(),
            unless: Vec::new(),
        });
    }
    for keyword in ["when:", "unless:"] {
        if words.next_if_eq(&keyword).is_none() {
            continue;
        }
        match &mut effect {
            Some(effect) if !effect.install && !effect.targets.is_empty() => {
                // A `when:` list may name effect units; an `unless:` list only
                // bindings.
                let when = keyword == "when:";
                let list = header_list(&mut words, name, &format!("`{keyword}` binding"), when)?;
                if when {
                    effect.when = list;
                } else {
                    effect.unless = list;
                }
            }
            _ => {
                return Err(format!(
                    "unit `{name}`: `{keyword}` needs `effect:` with a target"
                ));
            }
        }
    }
    let mut requires = Vec::new();
    if words.next_if_eq(&"requires:").is_some() {
        requires = header_list(&mut words, name, "required binding", false)?;
    }
    let rest = words.collect::<Vec<_>>();
    if !rest.is_empty() {
        return Err(format!(
            "unit `{name}`: unexpected `{}`; expected `compile-time`, `mutator`, `effect`, `install:` or `requires:`",
            rest.join(" ")
        ));
    }
    Ok(Header::Unit {
        name,
        compile_time,
        mutator,
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

/// The core table: every key of the [`CORE_CELLS`] literal as a cell, and the
/// entries of the [`CORE_HUB`], `const core = Object.assign(coreCells, {...});`,
/// whose values are a binding or `binding.member` (shorthand for a binding of
/// the key's name). Empty when neither unit exists.
fn parse_core_table<'a>(
    source: &'a str,
    units: &[Unit<'a>],
    bindings: &HashMap<&'a str, UnitId>,
) -> Result<HashMap<&'a str, CoreReference<'a>>, (usize, String)> {
    let find = |name: &str| units.iter().find(|unit| unit.name == name);
    let (cells, hub) = match (find(CORE_CELLS), find(CORE_HUB)) {
        (None, None) => return Ok(HashMap::new()),
        (Some(cells), Some(hub)) => (cells, hub),
        (Some(unit), None) | (None, Some(unit)) => {
            return Err((
                unit.header_range.start,
                format!("units `{CORE_CELLS}` and `{CORE_HUB}` come together"),
            ));
        }
    };
    let tokens = |unit: &Unit<'a>| {
        lex(source, unit.body_range.clone())
            .map_err(|error| (error.offset, error.message.to_string()))
    };
    let mut table = HashMap::new();
    let mut insert = |name: &'a str, reference, offset| match table.insert(name, reference) {
        Some(_) => Err((offset, format!("the core table lists `{name}` twice"))),
        None => Ok(()),
    };

    let cell_tokens = tokens(cells)?;
    let mut at = expect_core_head(
        source,
        &cell_tokens,
        &["const", CORE_CELLS, "=", "{"],
        cells.body_range.start,
    )?;
    loop {
        let Some(key) = cell_tokens.get(at).filter(|token| token.kind == Kind::Name) else {
            return Err((
                cells.body_range.start,
                format!("`{CORE_CELLS}` must be a flat object literal"),
            ));
        };
        let name = key.text(source);
        insert(name, CoreReference::Cell(name), key.start)?;
        at += 1;
        if cell_tokens
            .get(at)
            .is_some_and(|token| token.text(source) == ":")
        {
            // Skip the initial value, an expression up to a top-level `,` or `}`.
            let mut depth = 0usize;
            at += 1;
            while let Some(token) = cell_tokens.get(at) {
                match token.text(source) {
                    "(" | "[" | "{" => depth += 1,
                    ")" | "]" => depth = depth.saturating_sub(1),
                    "}" if depth > 0 => depth -= 1,
                    "," | "}" if depth == 0 => break,
                    _ => {}
                }
                at += 1;
            }
        }
        match cell_tokens.get(at).map(|token| token.text(source)) {
            Some(",") => at += 1,
            Some("}") => break,
            _ => {
                return Err((
                    key.start,
                    format!("`{CORE_CELLS}` must be a flat object literal"),
                ));
            }
        }
    }
    if cell_tokens[at..]
        .iter()
        .map(|token| token.text(source))
        .ne(["}", ";"])
    {
        return Err((
            cells.body_range.start,
            format!("`{CORE_CELLS}` must be a flat object literal"),
        ));
    }

    let hub_tokens = tokens(hub)?;
    let mut at = expect_core_head(
        source,
        &hub_tokens,
        &[
            "const", CORE_HUB, "=", "Object", ".", "assign", "(", CORE_CELLS, ",", "{",
        ],
        hub.body_range.start,
    )?;
    let shape = || {
        format!(
            "`{CORE_HUB}` must be `Object.assign({CORE_CELLS}, {{name, name: binding, name: binding.member, ...}})`"
        )
    };
    let text = |at: usize| hub_tokens.get(at).map(|token| token.text(source));
    let name_at = |at: usize| hub_tokens.get(at).filter(|token| token.kind == Kind::Name);
    loop {
        let Some(key) = name_at(at) else {
            return Err((hub.body_range.start, shape()));
        };
        let name = key.text(source);
        at += 1;
        let reference = if text(at) == Some(":") {
            let Some(binding) = name_at(at + 1) else {
                return Err((key.start, shape()));
            };
            at += 2;
            if text(at) == Some(".") {
                let Some(member) = name_at(at + 1) else {
                    return Err((key.start, shape()));
                };
                at += 2;
                CoreReference::Member(binding.text(source), member.text(source))
            } else {
                CoreReference::Binding(binding.text(source))
            }
        } else {
            CoreReference::Binding(name)
        };
        let (CoreReference::Binding(binding) | CoreReference::Member(binding, _)) = reference
        else {
            unreachable!("hub entries are bindings or members")
        };
        if !bindings.contains_key(binding) {
            return Err((
                key.start,
                format!("the core table reads `{binding}` for `{name}`, which no unit declares"),
            ));
        }
        insert(name, reference, key.start)?;
        match text(at) {
            Some(",") => at += 1,
            Some("}") => break,
            _ => return Err((key.start, shape())),
        }
    }
    if hub_tokens[at..]
        .iter()
        .map(|token| token.text(source))
        .ne(["}", ")", ";"])
    {
        return Err((hub.body_range.start, shape()));
    }
    Ok(table)
}

/// The core protocol dispatchers: for every `const <binding> = coreMethod(
/// "<key>", ...)` declarator, the key it registers in `coreDispatchers` and
/// the binding that holds it. Any other mention of [`CORE_METHOD`] outside its
/// own unit is an error, so the table lists every registration.
fn parse_dispatchers<'a>(
    source: &'a str,
    units: &[Unit<'a>],
) -> Result<HashMap<&'a str, &'a str>, Vec<(usize, String)>> {
    let mut table = HashMap::new();
    let mut errors = Vec::new();
    for unit in units {
        if unit.references.binary_search(&CORE_METHOD).is_err() {
            continue;
        }
        let tokens = match lex(source, unit.body_range.clone()) {
            Ok(tokens) => tokens,
            Err(error) => {
                errors.push((error.offset, error.message.to_string()));
                continue;
            }
        };
        let text = |index: Option<usize>| {
            index
                .and_then(|index| tokens.get(index))
                .map(|token| token.text(source))
        };
        for (index, token) in tokens.iter().enumerate() {
            if token.kind != Kind::Name || token.text(source) != CORE_METHOD {
                continue;
            }
            let binding = index
                .checked_sub(2)
                .map(|at| &tokens[at])
                .filter(|token| token.kind == Kind::Name);
            let key = tokens
                .get(index + 2)
                .filter(|token| token.kind == Kind::Str)
                .map(|token| &source[token.start + 1..token.end - 1]);
            let declarator = matches!(text(index.checked_sub(3)), Some("const" | ","))
                && text(index.checked_sub(1)) == Some("=")
                && text(Some(index + 1)) == Some("(");
            match (binding, key) {
                (Some(binding), Some(key)) if declarator && !key.contains('\\') => {
                    if table.insert(key, binding.text(source)).is_some() {
                        errors.push((
                            token.start,
                            format!("`{CORE_METHOD}` registers a dispatcher for `{key}` twice"),
                        ));
                    }
                }
                _ => errors.push((
                    token.start,
                    format!(
                        "unit `{}`: `{CORE_METHOD}` registers a core protocol dispatcher by key; \
                         call it only as `const <binding> = {CORE_METHOD}(\"<key>\", ...)` so the \
                         manifest lists the dispatcher",
                        unit.name
                    ),
                )),
            }
        }
    }
    if errors.is_empty() {
        Ok(table)
    } else {
        Err(errors)
    }
}

/// The index after `head` at the start of a core table unit's tokens.
fn expect_core_head(
    source: &str,
    tokens: &[Token],
    head: &[&str],
    start: usize,
) -> Result<usize, (usize, String)> {
    if tokens.len() > head.len()
        && tokens[..head.len()]
            .iter()
            .map(|token| token.text(source))
            .eq(head.iter().copied())
    {
        Ok(head.len())
    } else {
        Err((
            start,
            format!("expected `{}` at the start of the unit", head.join(" ")),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    const INSTALLS: &str = "// Original bounded runtime\n(() => {\n  \"use strict\";\n\n  //@unit f\n  function f() { return 1; }\n  //@unit install mutator\n  function install(fn) { fn.extra = true; return fn; }\n  //@unit f.extra install: f requires: f, install\n  install(f);\n  //@unit g requires: f\n  const g = () => f();\n  //@unit h effect requires: install\n  const h = install(() => 2);\n  //@exports\n  return Object.freeze({f, g, h});\n})()\n";

    #[test]
    fn compile_time_units_parse_and_round_trip_with_their_other_flags() {
        let source = INSTALLS
            .replace(
                "//@unit install mutator",
                "//@unit install compile-time mutator",
            )
            .replace("//@unit g requires", "//@unit g compile-time requires");
        let manifest = RuntimeManifest::parse(&source).unwrap();
        manifest.validate().unwrap();
        assert_eq!(manifest.with_computed_requires(), source);
        let unit = |name| &manifest.units[manifest.unit_named(name).unwrap().0];
        assert!(unit("install").compile_time && unit("install").mutator);
        assert!(unit("g").compile_time && !unit("f").compile_time);
        let error = RuntimeManifest::parse(
            &INSTALLS.replace("//@unit g requires", "//@unit g compile requires"),
        )
        .unwrap_err();
        assert!(
            error[0]
                .message
                .contains("expected `compile-time`, `mutator`"),
            "{error:?}"
        );
    }

    #[test]
    fn installs_follow_escaping_values_and_mutator_calls_are_effects() {
        let manifest = RuntimeManifest::parse(INSTALLS).unwrap();
        manifest.validate().unwrap();
        assert_eq!(manifest.with_computed_requires(), INSTALLS);
        let unit = |name| manifest.unit_named(name).unwrap();
        assert!(manifest.units[unit("install").0].mutator);
        assert_eq!(manifest.units[unit("h").0].load_time_calls, ["install"]);
        assert!(manifest.units[unit("g").0].load_time_calls.is_empty());
        assert_eq!(manifest.attached_installs(unit("f")), [unit("f.extra")]);
        assert!(manifest.attached_effects(unit("f")).is_empty());
        let kept = |roots: &[(UnitId, RootUse)]| {
            manifest
                .reached_by_uses(roots)
                .iter()
                .map(Option::is_some)
                .collect::<Vec<_>>()
        };
        // Called only, `f` keeps no install; held as a value, or required by
        // a kept unit that may pass it on, it does.
        assert_eq!(
            kept(&[(unit("f"), RootUse::Call)]),
            [true, false, false, false, false]
        );
        assert_eq!(
            kept(&[(unit("f"), RootUse::Value)]),
            [true, true, true, false, false]
        );
        assert_eq!(
            manifest.reached_by_uses(&[(unit("f"), RootUse::Call), (unit("g"), RootUse::Call)]),
            [Some(0), Some(1), Some(1), Some(1), None]
        );
        assert_eq!(
            manifest.reachable([unit("f")]),
            kept(&[(unit("f"), RootUse::Value)])
        );

        let messages = |source: &str| {
            RuntimeManifest::parse(source)
                .unwrap()
                .validate()
                .unwrap_err()
                .into_iter()
                .map(|e| e.message)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            messages(&INSTALLS.replace("//@unit h effect requires", "//@unit h requires")),
            ["unit `h`: calls mutator `install` while loading but is not marked `effect`"]
        );
        assert_eq!(
            messages(&INSTALLS.replace("//@unit g requires: f", "//@unit g effect requires: f")),
            ["unit `g`: is marked `effect` but only declares bindings and calls no mutator"]
        );
        // Without `mutator`, the call that initializes `h` is no effect.
        assert_eq!(
            messages(&INSTALLS.replace("//@unit install mutator", "//@unit install")),
            ["unit `h`: is marked `effect` but only declares bindings and calls no mutator"]
        );
        // An install names the bindings whose values it extends.
        assert!(
            RuntimeManifest::parse(&INSTALLS.replace("install: f requires", "install: requires"))
                .is_err()
        );
    }

    /// `table` is filled for `user` by `table.entry`, which only `op` can use.
    const CONDITIONAL: &str = "// Original bounded runtime\n(() => {\n  \"use strict\";\n\n  //@unit table\n  const table = [];\n  //@unit user requires: table\n  function user() { return table; }\n  //@unit op\n  function op() { return 1; }\n  //@unit table.entry effect: user when: op requires: op, table\n  table.push(op);\n  //@exports\n  return Object.freeze({user, op});\n})()\n";

    #[test]
    fn conditional_effects_parse_round_trip_and_wait_for_a_condition() {
        let manifest = RuntimeManifest::parse(CONDITIONAL).unwrap();
        manifest.validate().unwrap();
        assert_eq!(manifest.with_computed_requires(), CONDITIONAL);
        let unit = |name| manifest.unit_named(name).unwrap();
        let entry = unit("table.entry");
        assert_eq!(
            manifest.units[entry.0].effect.as_ref().unwrap().when,
            ["op"]
        );
        assert_eq!(manifest.conditions(entry), [unit("op")]);
        assert!(manifest.conditions(unit("user")).is_empty());
        // The target alone, or the condition alone, keeps no entry; both do,
        // whichever comes first.
        assert!(!manifest.reachable([unit("user")])[entry.0]);
        assert!(!manifest.reachable([unit("op")])[entry.0]);
        for roots in [[unit("user"), unit("op")], [unit("op"), unit("user")]] {
            assert!(manifest.reachable(roots)[entry.0]);
        }
        assert_eq!(
            manifest.reached_by(&[unit("op"), unit("user")])[entry.0],
            Some(1)
        );

        let error = |source: &str| match RuntimeManifest::parse(source) {
            Err(errors) => errors[0].message.clone(),
            Ok(manifest) => manifest.validate().unwrap_err()[0].message.clone(),
        };
        assert_eq!(
            error(&CONDITIONAL.replace("when: op", "when: missing")),
            "unit `table.entry`: `when:` binding `missing` is neither a runtime binding nor an effect unit"
        );
        // A `when:` list may name an effect unit, which must be kept as well.
        let chained = CONDITIONAL.replace(
            "  //@exports",
            "  //@unit table.more effect: user when: table.entry requires: table\n  table.push(null);\n  //@exports",
        );
        let manifest = RuntimeManifest::parse(&chained).unwrap();
        manifest.validate().unwrap();
        let unit = |name| manifest.unit_named(name).unwrap();
        let more = unit("table.more");
        assert_eq!(manifest.conditions(more), [unit("table.entry")]);
        assert!(!manifest.reachable([unit("user")])[more.0]);
        assert!(manifest.reachable([unit("op"), unit("user")])[more.0]);
        assert_eq!(
            error(&chained.replace("when: table.entry", "when: op.table")),
            "unit `table.more`: `when:` binding `op.table` is neither a runtime binding nor an effect unit"
        );
        assert_eq!(
            error(&CONDITIONAL.replace("effect: user when: op", "effect when: op")),
            "unit `table.entry`: `when:` needs `effect:` with a target"
        );
        assert_eq!(
            error(&CONDITIONAL.replace("when: op", "when: op, op")),
            "unit `table.entry`: names `when:` binding `op` twice"
        );
    }

    /// Signatures over roots: each root's units are those its own demands
    /// keep, installs follow the roots that hold the target's value, and a
    /// conditional effect whose condition another root needs goes with its
    /// target.
    #[test]
    fn signatures_split_the_slice_among_roots() {
        let manifest = RuntimeManifest::parse(INSTALLS).unwrap();
        let unit = |name| manifest.unit_named(name).unwrap();
        let [first, second, both] = [[1u64], [2], [3]];
        // `f` is called by root 0 and held by `g`, which root 1 calls, so its
        // install is root 1's.
        let demands = [
            (unit("f"), RootUse::Call, &first[..]),
            (unit("g"), RootUse::Call, &second[..]),
        ];
        let signatures = manifest.signatures(&demands, 1);
        let roots = |name| signatures[unit(name).0][0];
        assert_eq!(
            ["f", "install", "f.extra", "g", "h"].map(roots),
            [3, 2, 2, 2, 0]
        );
        for (bit, demand) in demands.iter().enumerate() {
            let kept = manifest.reached_by_uses(&[(demand.0, demand.1)]);
            for (unit, kept) in kept.iter().enumerate() {
                assert_eq!(kept.is_some(), signatures[unit][0] & (1 << bit) != 0);
            }
        }

        // An entry that sets a property, which no reader enumerates, so that
        // it may run after its readers loaded.
        let keyed = CONDITIONAL.replace("table.push(op);", "table.op = op;");
        let manifest = RuntimeManifest::parse(&keyed).unwrap();
        let unit = |name| manifest.unit_named(name).unwrap();
        let roots = |demands: &[(UnitId, RootUse, &[u64])]| {
            let signatures = manifest.signatures(demands, 1);
            ["table", "user", "op", "table.entry"].map(|name| signatures[unit(name).0][0])
        };
        // A root that needs the target and the condition keeps the entry.
        assert_eq!(
            roots(&[
                (unit("user"), RootUse::Value, &both[..]),
                (unit("op"), RootUse::Value, &second[..]),
            ]),
            [3, 3, 2, 2]
        );
        // Root 1 needs `op` but no `user`, whose values root 0 may pass it:
        // the entry goes with `user`, and takes `op` along.
        assert_eq!(
            roots(&[
                (unit("user"), RootUse::Value, &first[..]),
                (unit("op"), RootUse::Value, &second[..]),
            ]),
            [1, 1, 3, 1]
        );
        assert_eq!(
            roots(&[(unit("op"), RootUse::Value, &second[..])]),
            [0, 0, 2, 0]
        );

        // An entry appending to `table` shares its roots once kept: a reader
        // may enumerate the list before a later chunk loads (`recordMethods`
        // builds a record type's methods when the type is defined), so the
        // entry, and `op` with it, goes with `table`.
        let manifest = RuntimeManifest::parse(CONDITIONAL).unwrap();
        let unit = |name| manifest.unit_named(name).unwrap();
        assert_eq!(manifest.sharers(unit("table")), [unit("table.entry")]);
        let signatures = manifest.signatures(
            &[
                (unit("user"), RootUse::Value, &both[..]),
                (unit("op"), RootUse::Value, &second[..]),
            ],
            1,
        );
        assert_eq!(
            ["table", "user", "op", "table.entry"].map(|name| signatures[unit(name).0][0]),
            [3, 3, 3, 3]
        );

        // A wrapper assigns `user`, so it shares `user`'s roots once kept:
        // chunks receive other chunks' bindings as values, which it cannot
        // assign.
        let shared = CONDITIONAL.replace(
            "  //@exports",
            "  //@unit user.wrap effect: table when: op requires: user\n  user = (wrapped => () => wrapped())(user);\n  //@exports",
        );
        let manifest = RuntimeManifest::parse(&shared).unwrap();
        manifest.validate().unwrap();
        let unit = |name| manifest.unit_named(name).unwrap();
        let wrap = unit("user.wrap");
        assert_eq!(manifest.sharers(unit("user")), [wrap]);
        let wrapped =
            |demands: &[(UnitId, RootUse, &[u64])]| manifest.signatures(demands, 1)[wrap.0][0];
        // Root 1 alone needs the wrapper; it goes with `user`, which both need.
        assert_eq!(
            wrapped(&[
                (unit("user"), RootUse::Value, &both[..]),
                (unit("op"), RootUse::Value, &second[..]),
            ]),
            3
        );
        // Without `op` nothing keeps it.
        assert_eq!(wrapped(&[(unit("user"), RootUse::Value, &both[..])]), 0);
    }

    /// `table` gets `fallback` for `user` unless `op` is kept, whose own entry
    /// follows it.
    const LEAN: &str = "// Original bounded runtime\n(() => {\n  \"use strict\";\n\n  //@unit table\n  const table = [];\n  //@unit user requires: table\n  function user() { return table; }\n  //@unit op\n  function op() { return 1; }\n  //@unit other requires: op\n  function other() { return op; }\n  //@unit fallback\n  function fallback() { return 0; }\n  //@unit table.lean effect: user unless: op requires: fallback, table\n  table.push(fallback);\n  //@unit table.full effect: user when: op requires: op, table\n  table.push(op);\n  //@exports\n  return Object.freeze({user, op, other});\n})()\n";

    #[test]
    fn effects_with_unless_wait_for_everything_else() {
        let manifest = RuntimeManifest::parse(LEAN).unwrap();
        manifest.validate().unwrap();
        assert_eq!(manifest.with_computed_requires(), LEAN);
        let unit = |name| manifest.unit_named(name).unwrap();
        let lean = unit("table.lean");
        assert_eq!(
            manifest.units[lean.0].effect.as_ref().unwrap().unless,
            ["op"]
        );
        assert_eq!(manifest.blockers(lean), [unit("op")]);
        // The target keeps the lean entry and its requires, attributed as the
        // target is ...
        let kept = manifest.reached_by(&[unit("op"), unit("user")]);
        assert!(kept[lean.0].is_none() && kept[unit("table.full").0] == Some(1));
        let kept = manifest.reached_by(&[unit("other"), unit("user")]);
        assert!(kept[lean.0].is_none(), "`other` keeps `op`");
        let kept = manifest.reached_by(&[unit("table"), unit("user")]);
        assert_eq!(kept[lean.0], Some(1));
        assert_eq!(kept[unit("fallback").0], Some(1));
        assert!(kept[unit("op").0].is_none());
        // ... and nothing without it.
        assert!(!manifest.reachable([unit("op")])[lean.0]);
        // Among roots, it goes with its target where no root keeps `op`, and
        // nowhere where one does: there the full entry goes with the target.
        let [first, second, both] = [[1u64], [2], [3]];
        let roots = |demands: &[(UnitId, RootUse, &[u64])]| {
            let signatures = manifest.signatures(demands, 1);
            ["user", "table.lean", "fallback", "table.full", "op"]
                .map(|name| signatures[unit(name).0][0])
        };
        assert_eq!(
            roots(&[
                (unit("user"), RootUse::Value, &first[..]),
                (unit("table"), RootUse::Value, &second[..]),
            ]),
            [1, 3, 3, 0, 0]
        );
        assert_eq!(
            roots(&[
                (unit("user"), RootUse::Value, &first[..]),
                (unit("other"), RootUse::Value, &second[..]),
            ]),
            [1, 0, 0, 1, 3]
        );
        assert_eq!(
            roots(&[(unit("user"), RootUse::Value, &both[..])]),
            [3, 3, 3, 0, 0]
        );

        let error = |source: &str| match RuntimeManifest::parse(source) {
            Err(errors) => errors[0].message.clone(),
            Ok(manifest) => manifest.validate().unwrap_err()[0].message.clone(),
        };
        assert_eq!(
            error(&LEAN.replace("unless: op", "unless: missing")),
            "unit `table.lean`: `unless:` binding `missing` is not a runtime binding"
        );
        assert_eq!(
            error(&LEAN.replace("effect: user unless: op", "effect unless: op")),
            "unit `table.lean`: `unless:` needs `effect:` with a target"
        );
        assert_eq!(
            error(&LEAN.replace(
                "unless: op requires: fallback, table\n  table.push(fallback);",
                "unless: op requires: fallback, op, table\n  table.push(fallback, op);"
            )),
            "unit `table.lean`: requires its `unless:` binding `op`, so it is never kept"
        );
        assert_eq!(
            error(&LEAN.replace(
                "unless: op requires: fallback, table\n  table.push(fallback);",
                "unless: op requires: fallback, other, table\n  table.push(fallback, other);"
            )),
            "unit `table.lean`: keeps its `unless:` binding `op` through what it requires"
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
