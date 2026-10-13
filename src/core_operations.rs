//! The core operation table: parameter and return types, purity and runtime
//! units for every [`CoreFnId`], read from `src/core_operations.tsv`.
//!
//! This is Phase 2 step 2 of docs/advanced-optimization.md ("tables over
//! inference"): passes that need to know what a core operation does read its
//! row instead of rediscovering it. `build.rs` turns the table's first column
//! into [`CoreFnId::ALL`] and an exhaustive `match`, so a variant without a
//! row does not compile; the rest is parsed once, on first use.
//!
//! # Types
//!
//! [`Type`] is a set of value classes, so the lattice order is set inclusion,
//! [`Type::union`] is the join and [`Type::intersection`] the meet. The
//! classes partition all values:
//!
//! | Class | Values |
//! |---|---|
//! | `nil` | `nil` (JavaScript `null` and `undefined`) |
//! | `boolean`, `number`, `string` | JavaScript primitives of that type |
//! | `keyword`, `symbol` | keywords and symbols |
//! | `fn` | JavaScript functions, including type constructors and protocols |
//! | `seq` | the runtime's sequences other than lists: cons cells, lazy, indexed, chunked, range and key/value sequences |
//! | `list` | the runtime's lists |
//! | `vector`, `map`, `set` | the runtime's persistent vectors (with subvectors and map entries), maps and sets, sorted ones included |
//! | `record` | instances of `defrecord` types |
//! | `object` | other values of runtime or user types: atoms, volatiles, delays, vars, UUIDs, transients, reduced values, queues, chunks, namespaces, metadata-carrying functions, `deftype` and `reify` instances |
//! | `host` | every other JavaScript value: arrays, plain objects, dates, regular expressions, errors, promises, bigints, JavaScript symbols |
//!
//! A user type that implements `ISeq` or `IMap` is still an `object`. `any` is
//! the union of all classes and `never` the empty type (an operation that
//! never returns normally). In the table, classes are joined with `|`, and
//! [`TYPE_ALIASES`] name three common unions: `scalar`, `coll` (the five
//! runtime collection classes) and `seqable`.
//!
//! # Rows
//!
//! Each row describes one [`Operation`]:
//!
//! - `value`: the type of the core value itself (`fn` for operations, types
//!   and protocols, `any` for dynamic vars, which a program may rebind).
//! - `params`: the call signatures, one bracketed arity each, with `&` before
//!   the type of every rest argument: `[] [number] [number & number]`. `-` means
//!   no call is described. The arities and their parameter types are the
//!   operation's *domain*.
//! - `returns`: the type of every value a call within the domain returns.
//! - `otherwise`: the type of every value any other call returns, including
//!   arity mismatches. `str` returns a string whatever it is given, although
//!   only scalars are inside its domain.
//! - `purity` and `calls` hold for calls within the domain. A call outside it
//!   may do anything: see [`Operation::call`].
//! - `units`: the runtime units (`src/runtime_manifest.rs`) whose code defines
//!   the value the shared runtime holds as `rt.core.<name>`, as the core table
//!   (`RuntimeManifest::core_reference`) resolves it: the unit declaring the
//!   binding that holds it (`add` for `+`, `keywordCore` for `keyword`, the
//!   constructor `PersistentVectorValue` for the type), the unit declaring the
//!   object it is a member of (`reader` for the reader), or `coreCells` for a
//!   cell (dynamic vars, `rand`). These are the slicing roots of the direct reference that
//!   `Advanced` output reads.
//!
//! [`Purity`] is the operation's own behavior: `pure` (no effect, never
//! throws, reads no mutable state, and returns the identical value for the
//! same arguments, so a call may be removed, reordered, merged or folded),
//! `allocates` (as `pure`, but a call may return a fresh object, so calls may
//! be removed but not merged), `throws` (may throw, otherwise `allocates`) and
//! `effect` (anything else: mutation, input and output, reading mutable state
//! such as atoms, arrays, host objects and dynamic vars, randomness and time).
//!
//! [`Calls`] says which code the arguments supply that the operation may run;
//! its effects come on top of the operation's purity. `-`: none (it stores
//! its arguments or tests their type). `dispatch`: the arguments' and their
//! elements' protocol implementations, lazy sequence and delay bodies, the
//! comparators of sorted collections, and atom watches and validators.
//! `invoke`: `dispatch`, and it calls its function arguments.
//!
//! # What a pass may assume
//!
//! The runtime's own protocol implementations for its own types are `pure`.
//! So a `dispatch` call whose arguments are *plain data* runs no other code:
//! values built only from scalars and from vectors, hash maps, hash sets and
//! lists of plain data, as literals build them (no lazy sequences, sorted
//! collections, functions, records, objects or host values). The table
//! describes the runtime's own definitions and relies on these program-wide
//! conditions, which a pass that uses it must check or accept:
//!
//! - The program does not `set!` the core operation (that replaces its value
//!   everywhere) and does not extend core protocols to the runtime's own types
//!   or to host types (`extend-type` on `PersistentVector`, `number`, `object`,
//!   `default`, ...), which would let `dispatch` reach program code on plain
//!   data.
//! - Protocol extensions, including `specify!`, are part of the program's
//!   definition rather than state: predicates such as `counted?` and
//!   `satisfies?` are `pure` although an extension evaluated later changes
//!   their answer.
//! - Identity hash codes of functions, atoms and host objects are assigned on
//!   first use and are not state, as with Closure's `goog.getUid`: removing or
//!   reordering a `hash` can change the iteration order of hash collections
//!   keyed by such values.
//! - Termination is not an effect: `(count (range))` is `pure`.
//! - Results that come from protocol implementations of argument elements
//!   have the type the protocol requires (`-compare` returns a number).
//!
//! An unknown row is safe: `-` parameters, `any` types, `effect` and `invoke`.
//! The `core_operations` suite in `tests/compiler/` checks that every unit the
//! table names exists and is exactly the one that defines each runtime value,
//! so moving runtime code forces the table to follow (see that suite for how
//! to rewrite the `units` column), and calls every operation with params on
//! sample arguments to check its types, purity and calls.

use std::fmt;
use std::sync::OnceLock;

use crate::analyzer::CoreFnId;
use crate::runtime_manifest::{RuntimeManifest, UnitId};

include!(concat!(env!("OUT_DIR"), "/core_operations.rs"));

/// `src/core_operations.tsv` as written.
pub const TABLE: &str = include_str!("core_operations.tsv");

/// A set of value classes; see the module documentation.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Type(u16);

/// The class names in bit order, as written in the table.
const CLASS_NAMES: [&str; 15] = [
    "nil", "boolean", "number", "string", "keyword", "symbol", "fn", "seq", "vector", "map", "set",
    "list", "record", "object", "host",
];

impl Type {
    pub const NEVER: Self = Self(0);
    pub const NIL: Self = Self(1 << 0);
    pub const BOOLEAN: Self = Self(1 << 1);
    pub const NUMBER: Self = Self(1 << 2);
    pub const STRING: Self = Self(1 << 3);
    pub const KEYWORD: Self = Self(1 << 4);
    pub const SYMBOL: Self = Self(1 << 5);
    pub const FN: Self = Self(1 << 6);
    pub const SEQ: Self = Self(1 << 7);
    pub const VECTOR: Self = Self(1 << 8);
    pub const MAP: Self = Self(1 << 9);
    pub const SET: Self = Self(1 << 10);
    pub const LIST: Self = Self(1 << 11);
    pub const RECORD: Self = Self(1 << 12);
    pub const OBJECT: Self = Self(1 << 13);
    pub const HOST: Self = Self(1 << 14);
    pub const ANY: Self = Self((1 << 15) - 1);

    /// The join: values of either type.
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// The meet: values of both types.
    pub const fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    /// The values of `self` that are not values of `other`.
    pub const fn difference(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    /// Whether every value of `other` is a value of `self`.
    pub const fn contains(self, other: Self) -> bool {
        other.0 & !self.0 == 0
    }

    pub const fn is_never(self) -> bool {
        self.0 == 0
    }

    /// The single classes this type consists of, in bit order.
    pub fn classes(self) -> impl Iterator<Item = Self> {
        (0..CLASS_NAMES.len())
            .map(|bit| Self(1 << bit))
            .filter(move |class| self.contains(*class))
    }

    /// Parses `any`, `never`, or class names and [`TYPE_ALIASES`] joined by
    /// `|`, each class at most once.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "any" => return Some(Self::ANY),
            "never" => return Some(Self::NEVER),
            _ => {}
        }
        text.split('|').try_fold(Self::NEVER, |union, name| {
            let part = match CLASS_NAMES.iter().position(|class| *class == name) {
                Some(bit) => Self(1 << bit),
                None => TYPE_ALIASES.iter().find(|(alias, _)| *alias == name)?.1,
            };
            union
                .intersection(part)
                .is_never()
                .then_some(union.union(part))
        })
    }
}

/// Names for unions the table uses often. [`Type`]'s `Display` spells out
/// the classes instead.
pub const TYPE_ALIASES: [(&str, Type); 3] = [
    (
        "scalar",
        Type::NIL
            .union(Type::BOOLEAN)
            .union(Type::NUMBER)
            .union(Type::STRING)
            .union(Type::KEYWORD)
            .union(Type::SYMBOL),
    ),
    (
        "coll",
        Type::SEQ
            .union(Type::VECTOR)
            .union(Type::MAP)
            .union(Type::SET)
            .union(Type::LIST),
    ),
    (
        "seqable",
        Type::NIL
            .union(Type::STRING)
            .union(Type::SEQ)
            .union(Type::VECTOR)
            .union(Type::MAP)
            .union(Type::SET)
            .union(Type::LIST),
    ),
];

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if *self == Self::ANY {
            return f.write_str("any");
        }
        if self.is_never() {
            return f.write_str("never");
        }
        for (index, class) in self.classes().enumerate() {
            if index > 0 {
                f.write_str("|")?;
            }
            f.write_str(CLASS_NAMES[class.0.trailing_zeros() as usize])?;
        }
        Ok(())
    }
}

impl fmt::Debug for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Type({self})")
    }
}

/// What an operation does besides running code supplied through its
/// arguments; see the module documentation. Ordered from least to most
/// effectful, so `max` combines the purity of several calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Purity {
    Pure,
    Allocates,
    Throws,
    Effect,
}

/// Which code supplied through its arguments an operation may run; ordered
/// like [`Purity`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Calls {
    None,
    Dispatch,
    Invoke,
}

/// One arity of a call signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arity {
    pub params: Vec<Type>,
    /// The type of every argument after `params`, for a variadic arity.
    pub rest: Option<Type>,
}

impl Arity {
    /// Whether a call with `count` arguments uses this arity's shape.
    pub fn accepts_count(&self, count: usize) -> bool {
        if self.rest.is_some() {
            count >= self.params.len()
        } else {
            count == self.params.len()
        }
    }

    /// The parameter type of argument `index`.
    pub fn param(&self, index: usize) -> Option<Type> {
        self.params.get(index).copied().or(self.rest)
    }

    /// Whether arguments of these types are within this arity's domain.
    pub fn admits(&self, arguments: &[Type]) -> bool {
        self.accepts_count(arguments.len())
            && arguments
                .iter()
                .enumerate()
                .all(|(index, argument)| self.param(index).is_some_and(|p| p.contains(*argument)))
    }
}

/// What a call does, as far as the table knows; see [`Operation::call`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallSummary {
    pub returns: Type,
    pub purity: Purity,
    pub calls: Calls,
}

/// One row of the table; see the module documentation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Operation {
    pub variant: &'static str,
    /// The Clojure name the analyzer resolves to this operation.
    pub name: &'static str,
    pub value: Type,
    /// `None` when no call is described (`-`).
    pub arities: Option<Vec<Arity>>,
    pub returns: Type,
    pub otherwise: Type,
    pub purity: Purity,
    pub calls: Calls,
    pub units: Vec<&'static str>,
    /// One-based line in `src/core_operations.tsv`.
    pub line: usize,
}

impl Operation {
    /// The arity a call with `count` arguments uses: the fixed arity of that
    /// length, else the variadic one.
    pub fn arity(&self, count: usize) -> Option<&Arity> {
        let arities = self.arities.as_deref()?;
        arities
            .iter()
            .find(|arity| arity.rest.is_none() && arity.params.len() == count)
            .or_else(|| arities.iter().find(|arity| arity.accepts_count(count)))
    }

    /// What a call with arguments of these types does. Within the domain it is
    /// the row's `returns`, `purity` and `calls`; outside it the call returns
    /// `otherwise` and may do anything.
    pub fn call(&self, arguments: &[Type]) -> CallSummary {
        match self.arity(arguments.len()) {
            Some(arity) if arity.admits(arguments) => CallSummary {
                returns: self.returns,
                purity: self.purity,
                calls: self.calls,
            },
            _ => CallSummary {
                returns: self.otherwise,
                purity: Purity::Effect,
                calls: Calls::Invoke,
            },
        }
    }

    /// The `units` column as units of `manifest`, skipping names it lacks
    /// (the `core_operations` suite keeps every name valid for the embedded
    /// runtime): the slicing roots of a direct reference to this operation.
    pub fn runtime_units(&self, manifest: &RuntimeManifest<'_>) -> Vec<UnitId> {
        self.units
            .iter()
            .filter_map(|unit| manifest.unit_named(unit))
            .collect()
    }
}

/// A malformed table row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableError {
    /// One-based line in the table.
    pub line: usize,
    pub message: String,
}

impl fmt::Display for TableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "src/core_operations.tsv:{}: {}", self.line, self.message)
    }
}

/// The columns of a row, in order.
pub const COLUMNS: [&str; 9] = [
    "variant",
    "name",
    "value",
    "params",
    "returns",
    "otherwise",
    "purity",
    "calls",
    "units",
];

/// Parses table text: `;` lines are comments, every other line is a row of
/// tab-separated [`COLUMNS`]. Rows are returned in order.
pub fn parse_table(text: &'static str) -> Result<Vec<Operation>, Vec<TableError>> {
    let mut operations = Vec::new();
    let mut errors = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        match parse_row(line, index + 1) {
            Ok(operation) => operations.push(operation),
            Err(message) => errors.push(TableError {
                line: index + 1,
                message,
            }),
        }
    }
    if errors.is_empty() {
        Ok(operations)
    } else {
        Err(errors)
    }
}

fn parse_row(row: &'static str, line: usize) -> Result<Operation, String> {
    let fields = row.split('\t').collect::<Vec<_>>();
    let [
        variant,
        name,
        value,
        params,
        returns,
        otherwise,
        purity,
        calls,
        units,
    ] = fields[..]
    else {
        return Err(format!(
            "expected {} tab-separated columns ({}), found {}",
            COLUMNS.len(),
            COLUMNS.join(", "),
            fields.len()
        ));
    };
    let ty = |column: &str, text: &str| {
        Type::parse(text).ok_or_else(|| format!("{variant}: invalid {column} type `{text}`"))
    };
    let operation = Operation {
        variant,
        name,
        value: ty("value", value)?,
        arities: parse_params(params).map_err(|message| format!("{variant}: {message}"))?,
        returns: ty("returns", returns)?,
        otherwise: ty("otherwise", otherwise)?,
        purity: match purity {
            "pure" => Purity::Pure,
            "allocates" => Purity::Allocates,
            "throws" => Purity::Throws,
            "effect" => Purity::Effect,
            _ => return Err(format!("{variant}: invalid purity `{purity}`")),
        },
        calls: match calls {
            "-" => Calls::None,
            "dispatch" => Calls::Dispatch,
            "invoke" => Calls::Invoke,
            _ => return Err(format!("{variant}: invalid calls `{calls}`")),
        },
        units: if units == "-" {
            Vec::new()
        } else {
            units.split(',').collect()
        },
        line,
    };
    if name.is_empty() {
        return Err(format!("{variant}: empty name"));
    }
    if operation.units.iter().any(|unit| unit.is_empty()) {
        return Err(format!("{variant}: invalid units `{units}`"));
    }
    if operation.arities.is_none()
        && (operation.returns != Type::ANY
            || operation.purity != Purity::Effect
            || operation.calls != Calls::Invoke)
    {
        return Err(format!(
            "{variant}: without `params`, write `any`, `effect` and `invoke` (the type of \
             every result goes in `otherwise`)"
        ));
    }
    Ok(operation)
}

/// Parses `-` or bracketed arities such as `[] [number] [number & number]`.
fn parse_params(text: &str) -> Result<Option<Vec<Arity>>, String> {
    if text == "-" {
        return Ok(None);
    }
    let mut arities = Vec::new();
    let mut rest_text = text.trim();
    while !rest_text.is_empty() {
        let inner = rest_text
            .strip_prefix('[')
            .and_then(|after| after.split_once(']'))
            .ok_or_else(|| format!("invalid params `{text}`"))?;
        let (body, after) = inner;
        rest_text = after.trim_start();
        let mut params = Vec::new();
        let mut rest = None;
        let mut words = body.split_whitespace();
        while let Some(word) = words.next() {
            if word == "&" {
                let ty = words
                    .next()
                    .ok_or_else(|| format!("missing rest type in `{text}`"))?;
                rest = Some(Type::parse(ty).ok_or_else(|| format!("invalid type `{ty}`"))?);
                if words.next().is_some() {
                    return Err(format!("the rest type must come last in `{text}`"));
                }
            } else {
                params.push(Type::parse(word).ok_or_else(|| format!("invalid type `{word}`"))?);
            }
        }
        arities.push(Arity { params, rest });
    }
    if arities.is_empty() {
        return Err("empty params; use `-`".into());
    }
    let variadic = arities.iter().filter(|arity| arity.rest.is_some()).count();
    let mut fixed = arities
        .iter()
        .filter(|arity| arity.rest.is_none())
        .map(|arity| arity.params.len())
        .collect::<Vec<_>>();
    fixed.sort_unstable();
    if fixed.windows(2).any(|pair| pair[0] == pair[1]) || variadic > 1 {
        return Err(format!("overlapping arities in `{text}`"));
    }
    if let Some(variadic) = arities.iter().find(|arity| arity.rest.is_some())
        && fixed.last().is_some_and(|&max| max > variadic.params.len())
    {
        return Err(format!(
            "a fixed arity is longer than the variadic one in `{text}`"
        ));
    }
    Ok(Some(arities))
}

impl CoreFnId {
    /// This operation's row of the core operation table.
    ///
    /// # Panics
    /// When the table does not parse; the `core_operations` test suite and
    /// the module's unit tests reject such a table.
    pub fn operation(self) -> &'static Operation {
        &operations()[self.table_row()]
    }

    /// The property of the runtime `core` object that holds this operation.
    pub fn runtime_key(self) -> &'static str {
        self.runtime_name()
    }
}

/// Every row in [`CoreFnId::ALL`] order, parsed once.
pub fn operations() -> &'static [Operation] {
    static OPERATIONS: OnceLock<Vec<Operation>> = OnceLock::new();
    OPERATIONS.get_or_init(|| {
        let operations = parse_table(TABLE).unwrap_or_else(|errors| {
            panic!(
                "invalid core operation table:\n{}",
                errors
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        });
        // build.rs indexes the same rows, so the counts always agree.
        assert_eq!(operations.len(), CoreFnId::ALL.len());
        operations
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_parses_and_names_its_variant() {
        let operations = operations();
        for (row, (&id, operation)) in CoreFnId::ALL.iter().zip(operations).enumerate() {
            assert_eq!(id.table_row(), row);
            assert_eq!(format!("{id:?}"), operation.variant, "row {row}");
            assert_eq!(
                CoreFnId::resolve(operation.name),
                Some(id),
                "{}: `{}` does not resolve to {}",
                operation.line,
                operation.name,
                operation.variant
            );
            assert!(std::ptr::eq(id.operation(), operation));
        }
    }

    #[test]
    fn types_form_a_lattice_and_round_trip() {
        let seqish = Type::parse("seq|list|nil").unwrap();
        assert_eq!(seqish, Type::NIL.union(Type::SEQ).union(Type::LIST));
        assert_eq!(seqish.to_string(), "nil|seq|list");
        assert!(Type::ANY.contains(seqish) && seqish.contains(Type::LIST));
        assert!(!seqish.contains(Type::VECTOR));
        assert_eq!(seqish.intersection(Type::LIST.union(Type::MAP)), Type::LIST);
        assert_eq!(
            seqish.difference(Type::NIL.union(Type::MAP)),
            Type::SEQ.union(Type::LIST)
        );
        assert_eq!(Type::ANY.difference(Type::ANY), Type::NEVER);
        assert_eq!(Type::parse("any"), Some(Type::ANY));
        assert_eq!(Type::parse("never"), Some(Type::NEVER));
        assert_eq!(Type::parse(&Type::ANY.to_string()), Some(Type::ANY));
        assert_eq!(Type::ANY.classes().count(), CLASS_NAMES.len());
        for name in CLASS_NAMES {
            assert_eq!(Type::parse(name).unwrap().to_string(), name);
        }
        assert_eq!(
            Type::parse("scalar|fn|coll").unwrap().to_string(),
            "nil|boolean|number|string|keyword|symbol|fn|seq|vector|map|set|list"
        );
        assert_eq!(Type::parse("seqable"), Type::parse("nil|string|coll"));
        for invalid in [
            "",
            "nil|",
            "vec",
            "nil|nil",
            "any|nil",
            "seqable|nil",
            "coll|list",
        ] {
            assert_eq!(Type::parse(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn calls_outside_the_domain_are_unknown() {
        let (add, str) = (CoreFnId::Add.operation(), CoreFnId::Str.operation());
        let number = add.call(&[Type::NUMBER, Type::NUMBER, Type::NUMBER]);
        assert_eq!(
            (number.returns, number.purity),
            (Type::NUMBER, Purity::Pure)
        );
        let mixed = add.call(&[Type::NUMBER, Type::STRING]);
        assert_eq!(mixed.purity, Purity::Effect);
        assert_eq!(mixed.returns, add.otherwise);
        assert_eq!(str.call(&[Type::VECTOR]).returns, Type::STRING);
        assert_eq!(str.call(&[Type::VECTOR]).purity, Purity::Effect);
        assert_eq!(str.call(&[Type::STRING, Type::NUMBER]).purity, Purity::Pure);
    }

    #[test]
    fn malformed_rows_are_rejected_with_their_line() {
        let messages = |text: &'static str| {
            parse_table(text)
                .unwrap_err()
                .into_iter()
                .map(|error| error.to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            messages(
                ";; comment\nAdd\t+\tfn\t[number]\tnumber\tany\tpure\t-\tadd\nInc\tinc\tfn\t[num]\tnumber\tany\tpure\t-\tnumeric"
            ),
            ["src/core_operations.tsv:3: Inc: invalid type `num`"]
        );
        assert_eq!(
            messages("Add\t+\tfn\t[] [number]"),
            [
                "src/core_operations.tsv:1: expected 9 tab-separated columns (variant, name, value, params, returns, otherwise, purity, calls, units), found 4"
            ]
        );
        assert_eq!(
            messages("Add\t+\tfn\t[number] [any]\tnumber\tany\tpure\t-\tadd"),
            ["src/core_operations.tsv:1: Add: overlapping arities in `[number] [any]`"]
        );
        assert_eq!(
            messages("Add\t+\tfn\t[a b c] [& number]\tnumber\tany\tpure\t-\tadd")[0],
            "src/core_operations.tsv:1: Add: invalid type `a`"
        );
        assert_eq!(
            messages("Add\t+\tfn\t[number number] [& number]\tnumber\tany\tpure\t-\tadd"),
            [
                "src/core_operations.tsv:1: Add: a fixed arity is longer than the variadic one in `[number number] [& number]`"
            ]
        );
        assert_eq!(
            messages("Add\t+\tfn\t-\tany\tnumber\tpure\t-\tadd"),
            [
                "src/core_operations.tsv:1: Add: without `params`, write `any`, `effect` and `invoke` (the type of every result goes in `otherwise`)"
            ]
        );
    }
}
