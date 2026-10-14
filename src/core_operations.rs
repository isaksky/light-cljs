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
//!   operation's *domain*. An arity may narrow `returns` for its calls
//!   (`[fn]:fn`, a transducer), and a `fn` parameter may carry a callback
//!   shape ([`Callback`], light-cljs-243): the argument lists of every call
//!   the operation makes of that argument, one parenthesized list each, of
//!   comma-separated [`Descriptor`]s. `map` is `[fn(e1+) seqable & seqable]`
//!   (an element of each collection), `reduce` `[fn()(e1|r0,e1) seqable]`
//!   (no arguments for an empty collection, else the accumulator and an
//!   element) and `swap!` `[object fn(s0,a2+) & any]`. A descriptor joins
//!   terms with `|`: a type, `aN` (argument `N`), `eN` (an element of it,
//!   [`crate::inference::Inferred::element`]), `sN` (the state of the atom
//!   argument `N`: a value it holds, light-cljs-252), `rN` (what the function
//!   argument `N` returns), and `aN+` or `eN+` (from argument `N` on, which
//!   alone in a call's last descriptor spreads into one argument each).
//!   A function argument without a shape escapes: the operation may store,
//!   return or call it otherwise.
//! - `returns`: the type of every value a call within the domain returns,
//!   optionally followed by the elements of the `seq`, `list`, `vector` and
//!   `set` values it returns as a descriptor in angle brackets: `filter` is
//!   `fn|seq|list<e1>` and `map` `fn|seq|list<r0>`.
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

/// One term of a [`Descriptor`], naming a value through the arguments of a
/// call of the operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Term {
    /// A value of this type (`number`, `any`, ...).
    Type(Type),
    /// The argument at this index (`a1`).
    Argument(usize),
    /// An element of the argument at this index (`e1`): one of the values a
    /// sequence over it yields.
    Element(usize),
    /// The argument at this index and every later one (`a1+`).
    Arguments(usize),
    /// An element of the argument at this index and of every later one
    /// (`e1+`).
    Elements(usize),
    /// The state of the atom that is the argument at this index (`s0`): one
    /// of the values it holds when the call runs. Inference knows what the
    /// atoms whose every use it sees hold (light-cljs-252); any other
    /// argument's state is `any`.
    State(usize),
    /// What the function argument at this index returns when the operation
    /// calls it as its [`Callback`] says (`r0`).
    Returns(usize),
}

/// A value described through a call's arguments: the join of its terms
/// (`a1|r0`). In a [`Callback`]'s call, a descriptor that is one `Arguments`
/// or `Elements` term comes last and spreads: it is one argument per call
/// argument from that index on. Anywhere else such a term is the join of
/// them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Descriptor {
    pub terms: Vec<Term>,
}

impl Descriptor {
    /// The term of a descriptor that spreads in a callback's call.
    pub fn spread(&self) -> Option<Term> {
        match self.terms[..] {
            [term @ (Term::Arguments(_) | Term::Elements(_))] => Some(term),
            _ => None,
        }
    }
}

/// How an operation calls one of its function arguments (a callback shape):
/// the argument lists of every call it makes of the function at `index`,
/// for calls within the arity's domain. Written after the `fn` parameter in
/// the table, one parenthesized list per call: `fn()(e1|r0,e1)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Callback {
    pub index: usize,
    pub calls: Vec<Vec<Descriptor>>,
}

/// One arity of a call signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arity {
    pub params: Vec<Type>,
    /// The type of every argument after `params`, for a variadic arity.
    pub rest: Option<Type>,
    /// The function parameters with a callback shape. The operation calls a
    /// function argument without one in ways the table does not describe
    /// (it may also return or store it), so that function escapes.
    pub callbacks: Vec<Callback>,
    /// The type of every value a call within this arity's domain returns,
    /// when narrower than the row's `returns` (written `[fn]:fn`).
    pub returns: Option<Type>,
}

impl Arity {
    /// The callback shape of the function argument at `index`.
    pub fn callback(&self, index: usize) -> Option<&Callback> {
        self.callbacks
            .iter()
            .find(|callback| callback.index == index)
    }

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
    /// For calls within the domain: the values the `seq`, `list`, `vector`
    /// and `set` collections a call returns hold, written after the
    /// `returns` type (`seq|list<e1>`). A term naming an argument the call
    /// does not pass is `any`.
    pub elements: Option<Descriptor>,
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

    /// What a call with `count` arguments within the domain returns: the
    /// arity's `returns`, else the row's.
    pub fn returns_for(&self, count: usize) -> Type {
        self.arity(count)
            .and_then(|arity| arity.returns)
            .unwrap_or(self.returns)
    }

    /// What a call with arguments of these types does. Within the domain it is
    /// the arity's or the row's `returns`, and the row's `purity` and `calls`;
    /// outside it the call returns `otherwise` and may do anything.
    pub fn call(&self, arguments: &[Type]) -> CallSummary {
        match self.arity(arguments.len()) {
            Some(arity) if arity.admits(arguments) => CallSummary {
                returns: arity.returns.unwrap_or(self.returns),
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
    let (returns, elements) = match returns.split_once('<') {
        Some((returns, elements)) => {
            let descriptor = elements
                .strip_suffix('>')
                .and_then(parse_descriptor)
                .ok_or_else(|| format!("{variant}: invalid element type `<{elements}`"))?;
            (returns, Some(descriptor))
        }
        None => (returns, None),
    };
    let operation = Operation {
        variant,
        name,
        value: ty("value", value)?,
        arities: parse_params(params).map_err(|message| format!("{variant}: {message}"))?,
        returns: ty("returns", returns)?,
        elements,
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
    if operation.arities.iter().flatten().any(|arity| {
        arity
            .returns
            .is_some_and(|returns| !operation.returns.contains(returns))
    }) {
        return Err(format!(
            "{variant}: an arity's returns type is not part of the row's `{}`",
            operation.returns
        ));
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
        let (returns, after) = match after.strip_prefix(':') {
            Some(after) => {
                let end = after.find(char::is_whitespace).unwrap_or(after.len());
                let returns = &after[..end];
                let ty = Type::parse(returns)
                    .ok_or_else(|| format!("invalid arity returns type `{returns}`"))?;
                (Some(ty), &after[end..])
            }
            None => (None, after),
        };
        rest_text = after.trim_start();
        let mut params = Vec::new();
        let mut rest = None;
        let mut callbacks = Vec::new();
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
            } else if let Some(shape) = word.strip_prefix("fn(") {
                let calls =
                    parse_calls(shape).ok_or_else(|| format!("invalid callback shape `{word}`"))?;
                callbacks.push(Callback {
                    index: params.len(),
                    calls,
                });
                params.push(Type::FN);
            } else {
                params.push(Type::parse(word).ok_or_else(|| format!("invalid type `{word}`"))?);
            }
        }
        // Every term names a parameter, and `rN` one with a callback shape.
        let names_parameter = |index: usize| index < params.len() || rest.is_some();
        let valid = |term: &Term| match *term {
            Term::Type(_) => true,
            Term::Argument(index)
            | Term::Element(index)
            | Term::Arguments(index)
            | Term::Elements(index)
            | Term::State(index) => names_parameter(index),
            Term::Returns(index) => callbacks.iter().any(|callback| callback.index == index),
        };
        if callbacks
            .iter()
            .flat_map(|callback| callback.calls.iter().flatten())
            .any(|descriptor| !descriptor.terms.iter().all(valid))
        {
            return Err(format!(
                "a callback shape names a missing parameter in `{text}`"
            ));
        }
        arities.push(Arity {
            params,
            rest,
            callbacks,
            returns,
        });
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

/// Parses the calls of a callback shape after its `fn(`: comma-separated
/// descriptors per call, each call closed by `)`, the next opened by `(`:
/// `e1)` or `)(e1|r0,e1)`. An `aN+` or `eN+` term is alone in the last
/// descriptor of its call, which it spreads.
fn parse_calls(text: &str) -> Option<Vec<Vec<Descriptor>>> {
    let mut calls = Vec::new();
    let mut rest = text;
    loop {
        let (body, after) = rest.split_once(')')?;
        let call = if body.is_empty() {
            Vec::new()
        } else {
            body.split(',')
                .map(parse_descriptor)
                .collect::<Option<Vec<_>>>()?
        };
        let from = |term: &Term| matches!(term, Term::Arguments(_) | Term::Elements(_));
        let misplaced = call.iter().enumerate().any(|(position, descriptor)| {
            descriptor.terms.iter().any(from)
                && (position + 1 != call.len() || descriptor.spread().is_none())
        });
        if misplaced {
            return None;
        }
        calls.push(call);
        if after.is_empty() {
            return Some(calls);
        }
        rest = after.strip_prefix('(')?;
    }
}

/// Parses a [`Descriptor`]: terms joined by `|`, each a type, `aN`, `eN`,
/// `aN+`, `eN+`, `sN` or `rN`.
fn parse_descriptor(text: &str) -> Option<Descriptor> {
    let terms = text
        .split('|')
        .map(|term| {
            let (term, from) = match term.strip_suffix('+') {
                Some(term) => (term, true),
                None => (term, false),
            };
            let index = |prefix: char| {
                term.strip_prefix(prefix)
                    .filter(|digits| !digits.is_empty())
                    .and_then(|digits| digits.parse::<usize>().ok())
            };
            match (index('a'), index('e'), index('r'), index('s')) {
                (Some(index), ..) if from => Some(Term::Arguments(index)),
                (Some(index), ..) => Some(Term::Argument(index)),
                (_, Some(index), ..) if from => Some(Term::Elements(index)),
                (_, Some(index), ..) => Some(Term::Element(index)),
                (_, _, Some(index), _) if !from => Some(Term::Returns(index)),
                (_, _, _, Some(index)) if !from => Some(Term::State(index)),
                _ if !from => Type::parse(term).map(Term::Type),
                _ => None,
            }
        })
        .collect::<Option<Vec<_>>>()?;
    Some(Descriptor { terms })
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
    #[test]
    fn callback_shapes_element_types_and_arity_returns_parse() {
        let reduce = CoreFnId::Reduce.operation();
        let two = reduce.arity(2).unwrap().callback(0).unwrap();
        assert_eq!(two.calls.len(), 2);
        assert!(two.calls[0].is_empty());
        assert_eq!(two.calls[1][0].terms, [Term::Element(1), Term::Returns(0)]);
        let swap = CoreFnId::Swap.operation().arity(3).unwrap();
        assert_eq!(
            swap.callback(1).unwrap().calls[0][0].terms,
            [Term::State(0)]
        );
        assert_eq!(
            swap.callback(1).unwrap().calls[0][1].spread(),
            Some(Term::Arguments(2))
        );
        assert!(swap.callback(0).is_none());
        let filter = CoreFnId::Filter.operation();
        assert_eq!(filter.returns_for(1), Type::FN);
        assert_eq!(filter.returns_for(2), Type::SEQ.union(Type::LIST));
        assert_eq!(
            filter.call(&[Type::FN, Type::VECTOR]).returns,
            Type::SEQ.union(Type::LIST)
        );
        assert_eq!(filter.elements.as_ref().unwrap().terms, [Term::Element(1)]);
        let conj = CoreFnId::Conj.operation().elements.as_ref().unwrap();
        assert_eq!(conj.terms, [Term::Element(0), Term::Arguments(1)]);
        assert_eq!(conj.spread(), None);
        // A transducer arity has no shape: its function escapes.
        assert!(
            CoreFnId::MapSeq
                .operation()
                .arity(1)
                .unwrap()
                .callbacks
                .is_empty()
        );
    }

    #[test]
    fn malformed_callback_shapes_are_rejected() {
        let message = |params: &str, returns: &str| {
            let row = format!("Run\trun!\tfn\t{params}\t{returns}\tany\tpure\tinvoke\trun");
            parse_table(Box::leak(row.into_boxed_str()))
                .unwrap_err()
                .into_iter()
                .map(|error| error.to_string())
                .collect::<Vec<_>>()
        };
        for (params, returns, expected) in [
            ("[fn(e1 seqable]", "nil", "invalid callback shape `fn(e1`"),
            (
                "[fn(e1)x seqable]",
                "nil",
                "invalid callback shape `fn(e1)x`",
            ),
            (
                "[fn(e1+,a0) seqable]",
                "nil",
                "invalid callback shape `fn(e1+,a0)`",
            ),
            (
                "[fn(e1|a2+) seqable]",
                "nil",
                "invalid callback shape `fn(e1|a2+)`",
            ),
            (
                "[fn(r1) seqable]",
                "nil",
                "a callback shape names a missing parameter in `[fn(r1) seqable]`",
            ),
            (
                "[fn(e2) seqable]",
                "nil",
                "a callback shape names a missing parameter in `[fn(e2) seqable]`",
            ),
            (
                "[fn(s1+) object]",
                "nil",
                "invalid callback shape `fn(s1+)`",
            ),
            (
                "[fn(s2) object]",
                "nil",
                "a callback shape names a missing parameter in `[fn(s2) object]`",
            ),
            (
                "[fn(e1) seqable]:number",
                "nil",
                "an arity's returns type is not part of the row's `nil`",
            ),
            ("[fn(e1) seqable]", "nil<e1", "invalid element type `<e1`"),
            ("[fn(e1) seqable]", "nil<r+>", "invalid element type `<r+>`"),
        ] {
            assert_eq!(
                message(params, returns),
                [format!("src/core_operations.tsv:1: Run: {expected}")],
                "{params} {returns}"
            );
        }
    }
}
