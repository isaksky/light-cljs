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
//! member of one (`reader.readString`) or a cell (`coreCells.printFn`). The
//! output embeds only the units reachable from the roots over the unit
//! manifest ([`RuntimeManifest::reached_by_uses`]), in runtime order; the slice
//! returns the bindings the program reads as an array, which the output
//! destructures into `rt$<binding>` constants. A root keeps the unit declaring
//! its binding, so the `core` hub stays out of application slices. Each root
//! records how the output uses it ([`NameUse`]): a core operation that is only
//! the callee of plain calls (`rt$add(1, 2)`) does not escape, so the slice
//! leaves out its `install:` units (the variadic arity ABI), and an
//! `extend-type` of a core protocol method keeps the method's dispatcher,
//! which the runtime looks up by key. The output registers its namespaces
//! with the runtime only when the program reaches reflection
//! ([`RuntimeRoots::reach_reflection`]), so a slice keeps the `compile-time`
//! units of the manifest only for programs that use them.

use std::collections::HashMap;
use std::fmt;

use crate::analyzer::{ExprId, Program, Span};
use crate::application::OptimizationLevel;
use crate::runtime_manifest::{CORE_CELLS, RootUse, RuntimeManifest, UnitId};

/// The cells of [`CORE_CELLS`] that hold a namespace of the registry, which
/// only the namespace registration fills: `*ns*`, set when the runtime loads
/// and at each namespace entry. An `Advanced` program that reads, sets or
/// binds one reaches reflection.
pub const REGISTRY_CELLS: &[&str] = &["currentNs"];

/// The upstream fields and methods of the runtime's types, munged: `first` of
/// a `Cons`, `cnt` of a vector, `sval` of a `LazySeq`, ..., which the types'
/// `.fields` effects add. Upstream they are compiler-owned names, which
/// Closure renames and removes where nothing names them. An `Advanced` program
/// keeps them likewise only where reachable code names one of them as a
/// property, method or string or in `js*` text ([`crate::rta`]): its slice
/// then roots [`UPSTREAM_MEMBERS_UNIT`], whose `when:` the effects wait for.
/// The `builtin_types` suite checks the list against the runtime's prototypes.
pub const UPSTREAM_MEMBERS: &[&str] = &[
    "__hash",
    "_hash",
    "_meta",
    "_next",
    "add",
    "all",
    "arr",
    "ascending_QMARK_",
    "buf",
    "chunk",
    "chunk_next",
    "cnt",
    "coll",
    "comp",
    "count",
    "current",
    "currentval",
    "end",
    "f",
    "first",
    "fn",
    "forceChunk",
    "fqn",
    "front",
    "has_nil_QMARK_",
    "hash_map",
    "i",
    "meta",
    "more",
    "name",
    "next",
    "nil_val",
    "node",
    "ns",
    "off",
    "prev",
    "prev_seed",
    "rear",
    "rest",
    "root",
    "s",
    "seed",
    "shift",
    "stack",
    "start",
    "step",
    "str",
    "sval",
    "tail",
    "tree",
    "tree_map",
    "v",
    "val",
    "value",
    "vec",
    "xform",
];

/// The runtime unit that the `.fields` effects of the runtime's types wait
/// for, which an `Advanced` slice keeps only where the program names one of
/// [`UPSTREAM_MEMBERS`].
pub const UPSTREAM_MEMBERS_UNIT: &str = "upstreamMembers";

/// Core protocols whose implementations reach branches of the runtime's own
/// operations: a value of a type implementing one is handled like the values
/// of the runtime's kinds that implement it natively, so the conditional
/// effects installing those branches name the protocol's unit as a target
/// (`cons.equiv`, structural equality of sequential values, for
/// `ISequential`), besides `extendType` and `extendInstance`. An `Advanced`
/// slice keeps the unit where emitted code implements the protocol in a
/// `deftype`, `defrecord`, `reify` or extension, and the runtime's own types
/// built with `installMethods` are targets themselves. The
/// `runtime_manifest` suite checks the list against those targets.
pub const PROTOCOL_ROOTS: &[&str] = &["ISequential"];

/// The core protocol methods the runtime gives every record, each with the
/// arities of its slots (receiver included) and a unit of the `when:` list
/// of the effect installing it: `recordMethods.lookup`, kept where `get` is.
/// The effect waits for an operation that dispatches the method, but a
/// program can also call the method through its slot,
/// `cljs$core$ILookup$_lookup$arity$3`, by naming it as a string, property
/// or method, or in `js*` text ([`crate::rta`]). An `Advanced` slice of a
/// program with records then roots the unit, the one of the list that adds
/// least: mostly one the effect requires anyway. The `runtime_manifest`
/// suite checks the table against those effects.
pub const RECORD_METHODS: &[(&str, &[usize], &str)] = &[
    ("ICloneable/-clone", &[1], "clone"),
    ("IHash/-hash", &[1], "hash"),
    ("IEquiv/-equiv", &[2], "equalPair"),
    ("IMeta/-meta", &[1], "metaSupported"),
    ("IWithMeta/-with-meta", &[2], "withMetaSupported"),
    ("ILookup/-lookup", &[2, 3], "get"),
    ("ICounted/-count", &[1], "count"),
    ("ICollection/-conj", &[2], "conj"),
    ("IAssociative/-contains-key?", &[2], "contains"),
    ("IAssociative/-assoc", &[3], "assoc"),
    ("IMap/-dissoc", &[2], "dissoc"),
    ("ISeqable/-seq", &[1], "seq"),
    ("IIterable/-iterator", &[1], "iterator"),
    ("IPrintWithWriter/-pr-writer", &[3], "printWriterImpl"),
    ("IKVReduce/-kv-reduce", &[3], "reduceKv"),
];

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

/// How emitted code uses a runtime name it records.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NameUse {
    /// Read as a value, which may escape to code that reads its installed
    /// properties, so its `install:` units are kept.
    Value,
    /// Read only as the callee of plain calls, `rt$add(1, 2)` (Advanced), so
    /// its `install:` units (the variadic arity ABI) are not needed.
    Call,
    /// Not read: kept because the runtime finds it by key. An `extend-type` of
    /// a core protocol method keeps its dispatcher, which `extendType` looks up
    /// in `coreDispatchers` and patches (Advanced).
    Keep,
}

impl NameUse {
    /// The use of a name recorded twice: a name both called and kept is read
    /// and escapes, as a value is.
    fn join(self, other: Self) -> Self {
        if self == other { self } else { Self::Value }
    }
}

/// How the slice treats a root's unit: only a binding the output calls and
/// never reads otherwise is a call; a member read uses its binding as a value.
fn root_use(name: &RuntimeName, used: NameUse) -> RootUse {
    if used == NameUse::Call && name.member.is_none() {
        RootUse::Call
    } else {
        RootUse::Value
    }
}

/// The runtime names one emission reads, each with the site that first
/// demanded it, in first-demand order, and how the output uses it. A site is
/// the innermost expression being emitted, or `None` for the namespace
/// registration emitted with the runtime.
#[derive(Clone, Debug, Default)]
pub(crate) struct RuntimeRoots {
    names: Vec<(RuntimeName, Option<ExprId>, NameUse)>,
    seen: HashMap<RuntimeName, usize>,
}

impl RuntimeRoots {
    /// Records a read of `name` as a value.
    pub(crate) fn demand(&mut self, name: RuntimeName, site: Option<ExprId>) {
        self.record(name, site, NameUse::Value);
    }

    pub(crate) fn record(&mut self, name: RuntimeName, site: Option<ExprId>, used: NameUse) {
        match self.seen.get(&name) {
            Some(&index) => self.names[index].2 = self.names[index].2.join(used),
            None => {
                self.seen.insert(name, self.names.len());
                self.names.push((name, site, used));
            }
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
            .any(|(name, ..)| !name.direct && name.binding == binding && name.member == member)
    }

    /// Whether the output reads some `rt$<binding>[.<member>]`.
    #[cfg(any(debug_assertions, test))]
    pub(crate) fn contains_direct(&self, binding: &str) -> bool {
        self.names
            .iter()
            .any(|(name, _, used)| name.direct && name.binding == binding && *used != NameUse::Keep)
    }

    /// The bindings of the direct roots the output reads, each once, in
    /// first-demand order: what an `Advanced` runtime slice returns.
    pub(crate) fn direct_bindings(&self) -> Vec<&'static str> {
        let mut bindings: Vec<&'static str> = Vec::new();
        for (name, _, used) in &self.names {
            if name.direct && *used != NameUse::Keep && !bindings.contains(&name.binding) {
                bindings.push(name.binding);
            }
        }
        bindings
    }

    /// Whether an `Advanced` program whose output reads these roots reaches
    /// reflection, so that the output registers its namespaces: its slice keeps
    /// a compile-time unit (the namespace registry, the reader, the macro reader
    /// context, the compiler form bridge, `eval`), or it reads, sets or binds a
    /// [`REGISTRY_CELLS`] cell (`*ns*`). Otherwise nothing in the output can
    /// observe the registration.
    pub(crate) fn reach_reflection(&self) -> bool {
        let manifest = RuntimeManifest::embedded();
        self.names.iter().any(|(name, ..)| {
            name.binding == CORE_CELLS
                && name
                    .member
                    .is_some_and(|member| REGISTRY_CELLS.contains(&member))
        }) || manifest
            .reached_by_uses(&self.units(manifest))
            .iter()
            .zip(&manifest.units)
            .any(|(first, unit)| first.is_some() && unit.compile_time)
    }

    /// The unit declaring each root's binding with its use, in root order. A
    /// member read uses the binding as a value. A core cell whose initial
    /// value is a unit of its own, `coreCells.<cell>` (an effect that only the
    /// runtime's readers of the cell otherwise keep), roots that unit, which
    /// requires the cells.
    fn units(&self, manifest: &RuntimeManifest) -> Vec<(UnitId, RootUse)> {
        self.names
            .iter()
            .map(|(name, _, used)| {
                let cell = match name.member {
                    Some(member) if name.binding == CORE_CELLS => {
                        manifest.unit_named(&format!("{CORE_CELLS}.{member}"))
                    }
                    _ => None,
                };
                let unit = cell
                    .or_else(|| manifest.unit_of(name.binding))
                    .unwrap_or_else(|| panic!("{name} is not a runtime binding"));
                (unit, root_use(name, *used))
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
    /// Per unit, by `UnitId`, its bytes in a terse slice
    /// ([`crate::terse::terse_slice`]); `None` for readable text.
    pub(crate) unit_bytes: Option<Vec<usize>>,
    pub(crate) constants: ConstantPool,
}

/// The constant table of `Advanced` output (docs/advanced-optimization.md,
/// "Constant collection literals"): the keywords, quoted symbols and constant
/// collections the emitted code reads, each declared once after the runtime.
/// Empty below `Advanced`, which has no table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConstantPool {
    /// Declared entries, `k<n>`.
    pub entries: usize,
    /// Entries that are collection literals, reader metadata on them or
    /// `#uuid` literals.
    pub composites: usize,
    /// Entries that are methods of local functions hoisted out of their scope
    /// (light-cljs-228). The rest are keywords and quoted symbols.
    pub functions: usize,
    /// Entries built on first read rather than while the output loads.
    pub lazy: usize,
    /// Bytes of the declarations.
    pub bytes: usize,
}

/// The `Advanced` runtime factory for `roots`, which are all direct: the units
/// they reach, returning the bindings they read as an array in
/// [`RuntimeRoots::direct_bindings`] order, which the output destructures.
/// `terse` writes the units as `build.rs` strips and renames them
/// ([`crate::terse::terse_slice`]) rather than as written, and also returns
/// each unit's bytes.
pub(crate) fn sliced_runtime(
    roots: &RuntimeRoots,
    terse: bool,
) -> (String, Vec<&'static str>, Option<Vec<usize>>) {
    debug_assert!(
        roots.names.iter().all(|(name, ..)| name.direct),
        "Advanced output reads every runtime name directly"
    );
    let manifest = RuntimeManifest::embedded();
    let kept = manifest
        .reached_by_uses(&roots.units(manifest))
        .iter()
        .map(Option::is_some)
        .collect::<Vec<_>>();
    let bindings = roots.direct_bindings();
    if terse {
        let (text, bytes) = crate::terse::terse_slice(&kept, &bindings);
        (text, bindings, Some(bytes))
    } else {
        (
            manifest.sliced_bindings_text(&kept, &bindings),
            bindings,
            None,
        )
    }
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
    /// that accompanies an embedded runtime: always below `Advanced`, and at
    /// `Advanced` when the program reaches reflection.
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
    /// The unit declaring the name's binding, or for a core cell with an
    /// initial value of its own, that unit (`coreCells.rand`).
    pub unit: UnitId,
    pub demand: Demand,
    /// How the output uses the name, over all its reads.
    pub used: NameUse,
}

impl RuntimeRoot {
    /// The use of the root's unit for [`RuntimeManifest::reached_by_uses`]: a
    /// call for a binding only called, else a value (a member read uses its
    /// binding as a value).
    pub fn root_use(&self) -> RootUse {
        root_use(&self.name, self.used)
    }

    /// Whether the output reads the name: every root but a dispatcher kept for
    /// an `extend-type`.
    pub fn read(&self) -> bool {
        self.used != NameUse::Keep
    }

    /// The root column of the report: the emitted spelling, with `(...)` for a
    /// name only called and in parentheses for a name kept but not read.
    fn label(&self) -> String {
        match self.used {
            NameUse::Value => self.name.to_string(),
            NameUse::Call => format!("{}(...)", self.name),
            NameUse::Keep => format!("({})", self.name),
        }
    }
}

/// One embedded runtime unit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetainedUnit {
    pub unit: UnitId,
    pub name: &'static str,
    /// Bytes of the unit as the output embeds it: its body, including its
    /// comments and blank lines, or at `Advanced` its terse text (with the
    /// names of the slice) unless the output is readable (`pseudo_names`).
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
    /// The constant table (`Advanced`).
    pub constants: ConstantPool,
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
        let first = manifest.reached_by_uses(&root_units);
        let roots = emitted
            .roots
            .names
            .iter()
            .zip(root_units)
            .map(|(&(name, site, used), (unit, _))| RuntimeRoot {
                name,
                unit,
                used,
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
                bytes: emitted
                    .unit_bytes
                    .as_ref()
                    .map_or(unit.body_range.len(), |bytes| bytes[index]),
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
            constants: emitted.constants,
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
        let pool = &self.constants;
        if pool.entries > 0 {
            let functions = if pool.functions > 0 {
                format!(", {} functions", thousands(pool.functions))
            } else {
                String::new()
            };
            writeln!(
                f,
                "constant pool: {} entries ({} composite, {} lazy{functions}), {} bytes",
                thousands(pool.entries),
                thousands(pool.composites),
                thousands(pool.lazy),
                thousands(pool.bytes)
            )?;
        }
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
            .map(RuntimeRoot::label)
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
                (RuntimeName::core("add"), Some(ExprId(3)), NameUse::Value),
                (
                    RuntimeName::export("keyword"),
                    Some(ExprId(5)),
                    NameUse::Value
                ),
                (RuntimeName::export("symbol"), None, NameUse::Value),
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

        // A name only called stays a call; read as a value or both called and
        // kept, it escapes. A name only kept is not returned by the slice.
        let mut roots = RuntimeRoots::default();
        let name = |binding| RuntimeName::direct(binding, None);
        roots.record(name("add"), Some(ExprId(1)), NameUse::Call);
        roots.record(name("add"), Some(ExprId(2)), NameUse::Call);
        roots.record(name("inc"), Some(ExprId(3)), NameUse::Call);
        roots.demand(name("inc"), Some(ExprId(4)));
        roots.record(name("countProtocol"), Some(ExprId(5)), NameUse::Keep);
        roots.record(name("seqProtocol"), Some(ExprId(6)), NameUse::Keep);
        roots.record(name("seqProtocol"), Some(ExprId(7)), NameUse::Call);
        assert_eq!(
            roots
                .names
                .iter()
                .map(|(name, site, used)| (name.binding, *site, *used))
                .collect::<Vec<_>>(),
            [
                ("add", Some(ExprId(1)), NameUse::Call),
                ("inc", Some(ExprId(3)), NameUse::Value),
                ("countProtocol", Some(ExprId(5)), NameUse::Keep),
                ("seqProtocol", Some(ExprId(6)), NameUse::Value),
            ]
        );
        assert_eq!(roots.direct_bindings(), ["add", "inc", "seqProtocol"]);
        assert!(!roots.contains_direct("countProtocol"));
        assert_eq!(thousands(334_504), "334,504");
        assert_eq!(thousands(999), "999");
    }
}
