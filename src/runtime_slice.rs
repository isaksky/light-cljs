//! Advanced-mode runtime slicing (docs/advanced-optimization.md, Phase 1
//! steps 2 and 3) and the retention report.
//!
//! The emitter writes every runtime read through one helper that records a
//! [`RuntimeName`] together with the expression being emitted, so emission
//! itself reports the roots of the slice rather than a scan of the output
//! (debug builds only check the record against the output). Below
//! [`OptimizationLevel::Advanced`], and for the shared runtime of the REPL,
//! macro host and deferred units, the output embeds the whole runtime and
//! reads it through its exports (`rt.keyword`, `rt.core.add`).
//!
//! At `Advanced`, application output reads runtime bindings directly. A core
//! operation `rt.core.<name>` resolves through the core table
//! ([`RuntimeManifest::core_reference`]) to the binding holding it (`add`), a
//! member of one (`builtinTypes.PersistentVector`) or a cell
//! (`coreCells.printFn`). The output embeds only the units reachable from the
//! roots over the unit manifest ([`RuntimeManifest::reachable`]), in runtime
//! order; the slice returns the bindings the program reads as an array, which
//! the output destructures into `rt$<binding>` constants. A root keeps the unit
//! declaring its binding, so the `core` hub stays out of application slices.

use std::collections::HashSet;
use std::fmt;

use crate::analyzer::{ExprId, Program, Span};
use crate::application::OptimizationLevel;
use crate::runtime_manifest::{RuntimeManifest, UnitId};

/// A runtime name emitted code reads: a top-level runtime binding, optionally
/// a property (`member`) read from it, such as the runtime name of a
/// [`CoreFnId`](crate::CoreFnId). Below `Advanced` the binding is an export
/// read through the runtime object, `rt.<binding>[.<member>]`. At `Advanced`
/// the output declares each binding it reads as `rt$<binding>`
/// ([`RuntimeName::direct`]) and reads `rt$<binding>[.<member>]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RuntimeName {
    pub binding: &'static str,
    pub member: Option<&'static str>,
    /// Whether the output reads the binding directly, as `rt$<binding>`.
    pub direct: bool,
}

impl RuntimeName {
    /// `rt.<name>`.
    pub const fn export(name: &'static str) -> Self {
        Self {
            binding: name,
            member: None,
            direct: false,
        }
    }

    /// `rt.core.<member>`.
    pub const fn core(member: &'static str) -> Self {
        Self {
            binding: "core",
            member: Some(member),
            direct: false,
        }
    }

    /// `rt.compilerForms.<member>`.
    pub const fn compiler_forms(member: &'static str) -> Self {
        Self {
            binding: "compilerForms",
            member: Some(member),
            direct: false,
        }
    }

    /// `rt$<binding>[.<member>]`: the `Advanced` spelling, where `binding` is
    /// any runtime binding the slice keeps, exported or not.
    pub const fn direct(binding: &'static str, member: Option<&'static str>) -> Self {
        Self {
            binding,
            member,
            direct: true,
        }
    }
}

/// The emitted spelling, `rt.core.add` or `rt$add`.
impl fmt::Display for RuntimeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let separator = if self.direct { '$' } else { '.' };
        write!(f, "rt{separator}{}", self.binding)?;
        if let Some(member) = self.member {
            write!(f, ".{member}")?;
        }
        Ok(())
    }
}

/// The runtime names one emission reads, each with the site that first
/// demanded it, in first-demand order. A site is the innermost expression
/// being emitted, or `None` for the namespace registration emitted with the
/// runtime.
#[derive(Clone, Debug, Default)]
pub(crate) struct RuntimeRoots {
    names: Vec<(RuntimeName, Option<ExprId>)>,
    seen: HashSet<RuntimeName>,
}

impl RuntimeRoots {
    pub(crate) fn demand(&mut self, name: RuntimeName, site: Option<ExprId>) {
        if self.seen.insert(name) {
            self.names.push((name, site));
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// Whether `rt.<binding>[.<member>]` was demanded.
    #[cfg(any(debug_assertions, test))]
    pub(crate) fn contains(&self, binding: &str, member: Option<&str>) -> bool {
        self.names
            .iter()
            .any(|(name, _)| !name.direct && name.binding == binding && name.member == member)
    }

    /// Whether some `rt$<binding>[.<member>]` was demanded.
    #[cfg(any(debug_assertions, test))]
    pub(crate) fn contains_direct(&self, binding: &str) -> bool {
        self.names
            .iter()
            .any(|(name, _)| name.direct && name.binding == binding)
    }

    /// The bindings of the direct roots, each once, in first-demand order:
    /// what an `Advanced` runtime slice returns.
    pub(crate) fn direct_bindings(&self) -> Vec<&'static str> {
        let mut bindings: Vec<&'static str> = Vec::new();
        for (name, _) in &self.names {
            if name.direct && !bindings.contains(&name.binding) {
                bindings.push(name.binding);
            }
        }
        bindings
    }

    /// The unit declaring each root's binding, in root order.
    fn units(&self, manifest: &RuntimeManifest) -> Vec<UnitId> {
        self.names
            .iter()
            .map(|(name, _)| {
                manifest
                    .unit_of(name.binding)
                    .unwrap_or_else(|| panic!("{name} is not a runtime binding"))
            })
            .collect()
    }
}

/// How an application's output embeds the runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeEmbedding {
    /// No emitted code reads the runtime, so the output has none.
    None,
    /// The whole runtime, as every level below `Advanced` embeds it.
    Whole,
    /// Only the units reachable from the roots (`Advanced`).
    Slice,
}

/// What [`crate::emitter`] reports about the runtime of one emission.
pub(crate) struct EmittedRuntime {
    pub(crate) roots: RuntimeRoots,
    pub(crate) embedding: RuntimeEmbedding,
    /// Bytes of the embedded runtime factory expression.
    pub(crate) bytes: usize,
}

/// The `Advanced` runtime factory for `roots`, which are all direct: the units
/// they reach, returning the bindings they read as an array in
/// [`RuntimeRoots::direct_bindings`] order, which the output destructures.
pub(crate) fn sliced_runtime(roots: &RuntimeRoots) -> (String, Vec<&'static str>) {
    debug_assert!(
        roots.names.iter().all(|(name, _)| name.direct),
        "Advanced output reads every runtime name directly"
    );
    let manifest = RuntimeManifest::embedded();
    let kept = manifest.reachable(roots.units(manifest));
    let bindings = roots.direct_bindings();
    (manifest.sliced_bindings_text(&kept, &bindings), bindings)
}

/// What first demanded a runtime name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Demand {
    /// The innermost expression being emitted when the name was first read.
    Expression {
        expr: ExprId,
        /// The analyzed source the expression came from, in load order.
        source_index: usize,
        /// Byte span of the expression in that source.
        span: Span,
    },
    /// The namespace registration (`registerReaderNamespace`, `currentNs`)
    /// that accompanies an embedded runtime.
    NamespaceRegistration,
}

impl fmt::Display for Demand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Expression {
                source_index, span, ..
            } => write!(
                f,
                "source {source_index}, bytes {}..{}",
                span.start, span.end
            ),
            Self::NamespaceRegistration => f.write_str("namespace registration"),
        }
    }
}

/// A runtime name the emitted program reads and where it was first demanded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeRoot {
    pub name: RuntimeName,
    /// The unit declaring the name's binding.
    pub unit: UnitId,
    pub demand: Demand,
}

/// One embedded runtime unit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetainedUnit {
    pub unit: UnitId,
    pub name: &'static str,
    /// Bytes of the unit body, including its comments and blank lines.
    pub bytes: usize,
    /// Index in [`RetentionReport::roots`] of the first root whose closure
    /// reaches this unit. `None` for a unit embedded only because the whole
    /// runtime is.
    pub root: Option<usize>,
}

/// Which runtime units an application's output embeds and why
/// (`PreparedApplication::retention`, `--size-report`). Roots are listed in
/// first-demand order: the program body in emission order, then the
/// namespace registration. Each unit is attributed to the first root whose
/// closure reaches it, so a later root lists only the units it adds.
#[derive(Clone, Debug)]
pub struct RetentionReport {
    pub level: OptimizationLevel,
    pub embedding: RuntimeEmbedding,
    pub roots: Vec<RuntimeRoot>,
    /// Embedded units in runtime order.
    pub units: Vec<RetainedUnit>,
    /// Units in the whole runtime.
    pub total_units: usize,
    /// Bytes of the embedded runtime factory expression: the prologue, the
    /// unit bodies and the export section. Zero without a runtime.
    pub runtime_bytes: usize,
}

impl RetentionReport {
    pub(crate) fn new(
        program: &Program,
        level: OptimizationLevel,
        emitted: &EmittedRuntime,
    ) -> Self {
        let manifest = RuntimeManifest::embedded();
        let embedded = emitted.embedding != RuntimeEmbedding::None;
        let root_units = if embedded {
            emitted.roots.units(manifest)
        } else {
            Vec::new()
        };
        let first = manifest.reached_by(&root_units);
        let roots = emitted
            .roots
            .names
            .iter()
            .zip(root_units)
            .map(|(&(name, site), unit)| RuntimeRoot {
                name,
                unit,
                demand: site.map_or(Demand::NamespaceRegistration, |expr| {
                    let expr_data = &program.exprs[expr.0];
                    Demand::Expression {
                        expr,
                        source_index: expr_data.source_index,
                        span: expr_data.span.clone(),
                    }
                }),
            })
            .collect();
        let units = manifest
            .units
            .iter()
            .enumerate()
            .filter(|(index, _)| match emitted.embedding {
                RuntimeEmbedding::None => false,
                RuntimeEmbedding::Whole => true,
                RuntimeEmbedding::Slice => first[*index].is_some(),
            })
            .map(|(index, unit)| RetainedUnit {
                unit: UnitId(index),
                name: unit.name,
                bytes: unit.body_range.len(),
                root: first[index],
            })
            .collect();
        Self {
            level,
            embedding: emitted.embedding,
            roots,
            units,
            total_units: manifest.units.len(),
            runtime_bytes: emitted.bytes,
        }
    }

    /// Bytes of the embedded unit bodies.
    pub fn unit_bytes(&self) -> usize {
        self.units.iter().map(|unit| unit.bytes).sum()
    }
}

fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// A summary line, a table of roots with the units and bytes each adds, and a
/// table of every embedded unit with its bytes and the root and source span
/// that first reached it.
impl fmt::Display for RetentionReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let embedding = match self.embedding {
            RuntimeEmbedding::None => {
                return writeln!(f, "{}: no runtime embedded", self.level);
            }
            RuntimeEmbedding::Whole => "whole runtime",
            RuntimeEmbedding::Slice => "runtime slice",
        };
        writeln!(
            f,
            "{}: {embedding}, {} of {} units, {} unit bytes, {} runtime bytes",
            self.level,
            self.units.len(),
            self.total_units,
            thousands(self.unit_bytes()),
            thousands(self.runtime_bytes)
        )?;
        let mut added = vec![(0, 0); self.roots.len()];
        for unit in &self.units {
            if let Some(root) = unit.root {
                added[root].0 += 1;
                added[root].1 += unit.bytes;
            }
        }
        let names = self
            .roots
            .iter()
            .map(|root| root.name.to_string())
            .collect::<Vec<_>>();
        let name_width = names.iter().map(String::len).max().unwrap_or(0).max(4);
        writeln!(f)?;
        writeln!(
            f,
            "{:<name_width$}  {:>5}  {:>9}  first demanded by",
            "root", "units", "bytes"
        )?;
        for ((root, name), (units, bytes)) in self.roots.iter().zip(&names).zip(&added) {
            writeln!(
                f,
                "{name:<name_width$}  {units:>5}  {:>9}  {}",
                thousands(*bytes),
                root.demand
            )?;
        }
        let unit_width = self
            .units
            .iter()
            .map(|unit| unit.name.len())
            .max()
            .unwrap_or(0)
            .max(4);
        writeln!(f)?;
        writeln!(
            f,
            "{:<unit_width$}  {:>7}  {:<name_width$}  first demanded by",
            "unit", "bytes", "root"
        )?;
        for unit in &self.units {
            let (root, demand) = match unit.root {
                Some(root) => (names[root].as_str(), self.roots[root].demand.to_string()),
                None => ("-", "whole runtime".to_owned()),
            };
            writeln!(
                f,
                "{:<unit_width$}  {:>7}  {root:<name_width$}  {demand}",
                unit.name,
                thousands(unit.bytes)
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_names_print_as_emitted_and_roots_keep_their_first_site() {
        assert_eq!(RuntimeName::export("keyword").to_string(), "rt.keyword");
        assert_eq!(RuntimeName::core("add").to_string(), "rt.core.add");
        assert_eq!(
            RuntimeName::compiler_forms("jsArray").to_string(),
            "rt.compilerForms.jsArray"
        );
        let mut roots = RuntimeRoots::default();
        roots.demand(RuntimeName::core("add"), Some(ExprId(3)));
        roots.demand(RuntimeName::export("keyword"), Some(ExprId(5)));
        roots.demand(RuntimeName::core("add"), Some(ExprId(7)));
        roots.demand(RuntimeName::export("symbol"), None);
        assert_eq!(
            roots.names,
            [
                (RuntimeName::core("add"), Some(ExprId(3))),
                (RuntimeName::export("keyword"), Some(ExprId(5))),
                (RuntimeName::export("symbol"), None),
            ]
        );
        assert!(roots.contains("core", Some("add")));
        assert!(!roots.contains("core", None));
        assert!(roots.direct_bindings().is_empty());

        let direct = RuntimeName::direct("coreCells", Some("printFn"));
        assert_eq!(direct.to_string(), "rt$coreCells.printFn");
        assert_eq!(RuntimeName::direct("add", None).to_string(), "rt$add");
        let mut roots = RuntimeRoots::default();
        roots.demand(direct, Some(ExprId(1)));
        roots.demand(RuntimeName::direct("add", None), None);
        roots.demand(RuntimeName::direct("coreCells", Some("printLength")), None);
        assert!(
            roots.contains_direct("coreCells") && !roots.contains("coreCells", Some("printFn"))
        );
        assert_eq!(roots.direct_bindings(), ["coreCells", "add"]);
        assert_eq!(thousands(334_504), "334,504");
        assert_eq!(thousands(999), "999");
    }
}
