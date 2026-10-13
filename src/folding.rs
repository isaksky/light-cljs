//! Constant folding and dead-branch elimination over the prepared IR: the
//! folding half of Phase 2 step 8 of docs/advanced-optimization.md
//! (light-cljs-214), at `Advanced` only. [`crate::inlining`] is the other
//! half; `prepare_application` alternates the two with definition removal
//! (see "Phase 2 folding and inlining extension points" in the design
//! document).
//!
//! [`fold`] rewrites expressions in place, children first, so each rewrite sees
//! its operands folded:
//!
//! - A read of a global that is `^:const` (or defined by `goog-define`,
//!   [`crate::defines`]), defined once by a top-level `def` whose value is a
//!   [`Constant`] and never assigned, is that constant when the read is in a
//!   later top-level form, which runs after the definition. Upstream
//!   substitutes the value of a `^:const` Var at every reference analyzed after
//!   it in the same way.
//! - A `let` binding of a constant is substituted into its reads and dropped,
//!   and so is a binding nothing reads whose initializer has no effect. This is
//!   what folds `and` and `or`, whose expansions bind their operands.
//! - A call of a core operation on constants is its result when the core
//!   operation table ([`crate::core_operations`]) says the call is `pure` for
//!   those arguments and the program can neither replace the operation nor
//!   make it dispatch to program code ([`ProgramFacts::trusted`]):
//!   arithmetic, comparisons, `=`, predicates, `not` and `str` ([`evaluate`]).
//! - An `if` whose test is a constant is the branch the test selects, so the
//!   other branch, and the definitions only it reads, are gone (the
//!   definitions go when `prepare_application` next removes unread ones).
//!   `when`, `if-not`, `cond` and `and`/`or` in tests are `if`s.
//! - A `do` drops the forms before its last that have no effect, and one form
//!   left is that form; a `^boolean` hint on a constant is the constant.
//! - Lookups, counts and items of constant collections are their results
//!   (light-cljs-221; "Constant folding extension points" in the design
//!   document): a [`Constant::Collection`] is a literal the constant pool
//!   may hold, which [`Folder::collection`] also finds behind locals and
//!   `^:const` globals; [`evaluate`] gives its counts and predicates and
//!   [`select`] its items, which are copied to the call's place as the same
//!   pool entries. A map literal whose keys are constants answers its count
//!   and the lookups of absent keys whatever its values
//!   ([`Folder::keyed`]), and [`entry_lookups`] lists the lookups of present
//!   keys, which emission reads from the map's entries. Destructuring a
//!   constant folds through the `nth` and `get` its expansion calls, and
//!   [`Folder::sweep`] drops the bindings that leaves unread.
//!
//! # Soundness
//!
//! Each rewrite gives the value the expression has at run time, with the same
//! effects: a constant is the value of a literal, a read of a binding nothing
//! assigns, or a `pure` call of constants, which has no effect, cannot throw
//! and returns the same value every time. [`evaluate`] reproduces the
//! runtime's definitions of the operations in `src/js_runtime.js` with the
//! same IEEE double operations in the same order. A local that a closure,
//! a type or `js*` raw text may see is never substituted: the emitter names
//! captured locals and raw text may assign its holes. The root, namespace
//! entries and macro functions keep their expression identities, which
//! emission and preparation look up.
//!
//! Literals keep what emission makes of them. Map keys and set members never
//! become constants ([`fenced_positions`]): emission builds maps with
//! constant keys from key shapes and sets of scalars without a duplicate
//! check, which rely on the distinct keys of source literals. A literal the
//! program builds at each evaluation stays built when its items become
//! constants ([`built_literals`], [`keep_built`]), so it never becomes the
//! one pooled value every evaluation shares, and an item of it is never
//! copied ([`Folder::copyable`]).
//!
//! A fold against a collection reproduces the runtime's lookups
//! ([`same_key`], [`lookup`]) and needs core protocols intact, so that only
//! the runtime's implementations run. It never folds a call that throws (an
//! `nth` out of range without a default stays), never decides an equality
//! the runtime's hashing could decide otherwise, and drops only constants
//! and local reads.
use crate::analyzer::{CoreFnId, ExprId, ExprKind, LocalId, NamedLiteralId, Program, StringId};
use crate::core_operations::{Purity, Type};
use crate::emitter::{ConstantClass, constant_nodes};
use crate::inference::ProgramFacts;
use crate::program_fragment::{children, kind_children};
use std::collections::HashMap;

/// A value the folder knows at compile time.
#[derive(Clone, Debug, PartialEq)]
pub enum Constant {
    Nil,
    Bool(bool),
    Number(f64),
    /// UTF-16 code units, as `Program::strings` holds them.
    String(Vec<u16>),
    Keyword(NamedLiteralId),
    /// A quoted symbol without metadata, an interned table constant at
    /// `Advanced` (light-cljs-208).
    Symbol(NamedLiteralId),
    /// A constant collection (light-cljs-221): the literal expression that
    /// builds it, a collection literal of constants or reader metadata on one
    /// that the constant pool may hold (the emitter's `constant_nodes`). The
    /// folder resolves the locals and `^:const` globals bound to one
    /// ([`Folder::collection`]); [`Constant::of`] gives only scalars and
    /// symbols, which a `let` may substitute, while a collection stays where
    /// it is built.
    Collection(ExprId),
}

impl Constant {
    /// The scalar or symbol a literal expression evaluates to.
    pub fn of(program: &Program, id: ExprId) -> Option<Self> {
        Self::of_kind(program, &program.exprs[id.0].kind)
    }

    fn of_kind(program: &Program, kind: &ExprKind) -> Option<Self> {
        Some(match kind {
            ExprKind::Nil => Self::Nil,
            ExprKind::Bool(value) => Self::Bool(*value),
            ExprKind::Number(value) => Self::Number(*value),
            ExprKind::String(string) => Self::String(program.strings[string.0].clone()),
            ExprKind::Keyword(literal) => Self::Keyword(*literal),
            ExprKind::Symbol(literal) => Self::Symbol(*literal),
            _ => return None,
        })
    }

    /// The value of the item `id` of a constant collection: a scalar or
    /// symbol, or a nested collection, which is itself constant.
    fn item(program: &Program, id: ExprId) -> Option<Self> {
        Self::of(program, id).or_else(|| shape(program, id).map(|_| Self::Collection(id)))
    }

    /// Its class in the core operation table's lattice.
    pub fn ty(&self, program: &Program) -> Type {
        match self {
            Self::Nil => Type::NIL,
            Self::Bool(_) => Type::BOOLEAN,
            Self::Number(_) => Type::NUMBER,
            Self::String(_) => Type::STRING,
            Self::Keyword(_) => Type::KEYWORD,
            Self::Symbol(_) => Type::SYMBOL,
            Self::Collection(id) => match shape(program, *id) {
                Some((Shape::Vector, _)) => Type::VECTOR,
                Some((Shape::List, _)) => Type::LIST,
                Some((Shape::Map { .. }, _)) => Type::MAP,
                Some((Shape::Set { .. }, _)) => Type::SET,
                None => Type::ANY,
            },
        }
    }

    /// Whether Clojure counts it as true: anything but `nil` and `false`.
    pub fn truthy(&self) -> bool {
        !matches!(self, Self::Nil | Self::Bool(false))
    }

    /// The literal expression of this scalar or symbol, adding a string to
    /// `program`. A collection is copied by the folder instead.
    pub fn into_kind(self, program: &mut Program) -> ExprKind {
        match self {
            Self::Nil => ExprKind::Nil,
            Self::Bool(value) => ExprKind::Bool(value),
            Self::Number(value) => ExprKind::Number(value),
            Self::String(units) => {
                program.strings.push(units);
                ExprKind::String(StringId(program.strings.len() - 1))
            }
            Self::Keyword(literal) => ExprKind::Keyword(literal),
            Self::Symbol(literal) => ExprKind::Symbol(literal),
            Self::Collection(_) => unreachable!("evaluation returns scalars"),
        }
    }
}

/// How the runtime builds a constant collection, which decides its lookups
/// (src/js_runtime.js): `vectorFromArray`, `listFromArray`, `mapFromArray`
/// and `mapFromArrays` (equal keys merge, first key and last value) or the
/// positional `constantMapFromArray` (`LiteralMap`, whose lookups find the
/// first equal key), and `setFromArray` or the positional
/// `constantSetFromArray`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shape {
    Vector,
    List,
    Map { positional: bool },
    Set { positional: bool },
}

/// The shape and items (keys and values alternating for a map) of the
/// collection literal `id`, under any reader metadata.
pub(crate) fn shape(program: &Program, mut id: ExprId) -> Option<(Shape, &[ExprId])> {
    loop {
        return Some(match &program.exprs[id.0].kind {
            ExprKind::WithMeta { value, .. } => {
                id = *value;
                continue;
            }
            ExprKind::Vector(items) => (Shape::Vector, items),
            ExprKind::List(items) => (Shape::List, items),
            ExprKind::Map(items) => (Shape::Map { positional: false }, items),
            ExprKind::LiteralMap(items) => (Shape::Map { positional: true }, items),
            ExprKind::Set(items) => (Shape::Set { positional: false }, items),
            ExprKind::LiteralSet(items) => (Shape::Set { positional: true }, items),
            _ => return None,
        });
    }
}

/// The result of a fold that [`select`] computes.
#[derive(Clone, Debug, PartialEq)]
pub enum Folded {
    /// This scalar or symbol.
    Value(Constant),
    /// The item of a constant collection at this expression (a key, value,
    /// member or element), which the folder copies to the call's place.
    Item(ExprId),
    /// A list of these items of a constant list or vector, in order: `rest`
    /// and `next` of a list.
    List(Vec<ExprId>),
    /// The call's argument at this index: a not-found default, or the
    /// collection itself (`seq` of a list).
    Argument(usize),
    /// This value expression of a map literal that the call reads, whose
    /// evaluation moves to the call's place.
    Expression(ExprId),
}

/// Whether the runtime's `=` holds between `key` and the key or member `item`
/// of a constant collection, as its lookups compare them (`entryIndex`,
/// `equalPair`; a hash map hashes first, and keys that are `=` hash alike
/// here); `None` when this cannot tell: a collection key, `NaN`, `0` against
/// `-0` (`=` but hashed apart), and keywords or symbols spelled alike with
/// another namespace split (`=` compares spellings, hashing the parts).
fn same_key(program: &Program, key: &Constant, item: ExprId) -> Option<bool> {
    match Constant::of(program, item) {
        Some(item) => scalar_equal(program, key, &item),
        // A scalar is never `=` to a collection or `#uuid` while core
        // protocols are intact.
        None if !matches!(key, Constant::Collection(_)) => Some(false),
        None => None,
    }
}

/// `=` on two scalars or symbols, as [`same_key`] decides it.
fn scalar_equal(program: &Program, a: &Constant, b: &Constant) -> Option<bool> {
    use Constant::{Bool, Collection, Keyword, Nil, Number, String, Symbol};
    Some(match (a, b) {
        (Collection(_), _) | (_, Collection(_)) => return None,
        (Nil, Nil) => true,
        (Number(a), Number(b)) if a.is_nan() || b.is_nan() => return None,
        (Number(a), Number(b)) if *a == 0.0 && *b == 0.0 => {
            if a.is_sign_negative() != b.is_sign_negative() {
                return None;
            }
            true
        }
        (Number(a), Number(b)) => a == b,
        (String(a), String(b)) => a == b,
        (Bool(a), Bool(b)) => a == b,
        (Keyword(a), Keyword(b)) | (Symbol(a), Symbol(b)) => {
            let parts = |literal: NamedLiteralId| {
                let name = &program.named_literals[literal.0];
                (
                    name.namespace
                        .map(|namespace| &program.strings[namespace.0][..]),
                    &program.strings[name.name.0][..],
                )
            };
            if parts(*a) == parts(*b) {
                true
            } else if spelling(program, *a) == spelling(program, *b) {
                return None;
            } else {
                false
            }
        }
        _ => false,
    })
}

/// The spelling `ns/name` (or `name`) of a keyword or symbol literal, which
/// the runtime's `=` compares.
fn spelling(program: &Program, literal: NamedLiteralId) -> Vec<u16> {
    let name = &program.named_literals[literal.0];
    let mut units = Vec::new();
    if let Some(namespace) = name.namespace {
        units.extend_from_slice(&program.strings[namespace.0]);
        units.push(u16::from(b'/'));
    }
    units.extend_from_slice(&program.strings[name.name.0]);
    units
}

/// What `get` (and `lookup`) finds for `key` in the constant collection `id`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lookup {
    /// The value or member at this expression.
    Found(ExprId),
    Missing,
}

/// `get` of `key` in the constant collection `id`, or `None` when this cannot
/// tell. A vector finds a number index in range, truncated; a list finds
/// nothing. A map or set finds the value or member of the key `=` to `key`,
/// and needs every other key to be provably unequal to it: a merging builder
/// keeps one of several equal keys, and a positional one finds the first.
fn lookup(program: &Program, id: ExprId, key: &Constant) -> Option<Lookup> {
    let (shape, items) = shape(program, id)?;
    let (stride, positional) = match shape {
        Shape::Vector => {
            return Some(match key {
                Constant::Number(index) if *index >= 0.0 && *index < items.len() as f64 => {
                    Lookup::Found(items[index.trunc() as usize])
                }
                _ => Lookup::Missing,
            });
        }
        Shape::List => return Some(Lookup::Missing),
        Shape::Map { positional } => (2, positional),
        Shape::Set { positional } => (1, positional),
    };
    let mut found = None;
    for (index, item) in items.iter().step_by(stride).enumerate() {
        if same_key(program, key, *item)? {
            // A map finds the value after the key, a set the member.
            let result = items[index * stride + stride - 1];
            if positional {
                return Some(Lookup::Found(result));
            }
            if found.replace(result).is_some() {
                return None;
            }
        }
    }
    Some(found.map_or(Lookup::Missing, Lookup::Found))
}

/// The count of the constant collection `id`: its items, or its entries when
/// no two keys or members of a merging map or set may be `=`.
fn count(program: &Program, id: ExprId) -> Option<usize> {
    let (shape, items) = shape(program, id)?;
    match shape {
        Shape::Vector | Shape::List | Shape::Set { positional: true } => Some(items.len()),
        Shape::Map { positional: true } => Some(items.len() / 2),
        Shape::Map { positional: false } => {
            distinct(program, items.iter().step_by(2).copied()).then_some(items.len() / 2)
        }
        Shape::Set { positional: false } => {
            distinct(program, items.iter().copied()).then_some(items.len())
        }
    }
}

/// Whether no two of `keys` may be `=`: scalars and symbols that are
/// pairwise provably unequal ([`scalar_equal`]) and at most one collection.
/// Linear, with a set of canonical forms: zeros of both signs share one, and
/// keywords and symbols are their spellings, so `0` and `-0.0` or keywords
/// spelled alike count as possibly equal, and `NaN` makes no key distinct.
fn distinct(program: &Program, keys: impl Iterator<Item = ExprId>) -> bool {
    #[derive(Hash, PartialEq, Eq)]
    enum Canonical {
        Nil,
        Bool(bool),
        Number(u64),
        String(Vec<u16>),
        Named(bool, Vec<u16>),
    }
    let mut seen = std::collections::HashSet::new();
    let mut collections = 0;
    for key in keys {
        let canonical = match Constant::of(program, key) {
            Some(Constant::Nil) => Canonical::Nil,
            Some(Constant::Bool(value)) => Canonical::Bool(value),
            Some(Constant::Number(value)) if value.is_nan() => return false,
            Some(Constant::Number(0.0)) => Canonical::Number(0),
            Some(Constant::Number(value)) => Canonical::Number(value.to_bits()),
            Some(Constant::String(units)) => Canonical::String(units),
            Some(Constant::Keyword(literal)) => Canonical::Named(false, spelling(program, literal)),
            Some(Constant::Symbol(literal)) => Canonical::Named(true, spelling(program, literal)),
            Some(Constant::Collection(_)) | None => {
                collections += 1;
                if collections > 1 {
                    return false;
                }
                continue;
            }
        };
        if !seen.insert(canonical) {
            return false;
        }
    }
    true
}

/// The collection a constant is, if it is one.
fn collection_of(constant: &Constant) -> Option<ExprId> {
    match constant {
        Constant::Collection(id) => Some(*id),
        _ => None,
    }
}

/// The result of a call of `core` that returns an item of a constant
/// collection, or a list of them, when the runtime's definition is
/// reproduced here (light-cljs-221): `get`, `get-in`, `nth`, `first`,
/// `second`, `last`, `peek`, `rest`, `next`, `seq` and `not-empty`. `args`
/// are the call's arguments, `None` for one that is not a constant, which
/// only a not-found default may be. A call
/// that would throw (`nth` out of range without a default, an index that is
/// not a number) or whose result this does not know is `None` and stays.
/// The caller has checked that the operation is trusted and that core
/// protocols are intact.
pub fn select(program: &Program, core: CoreFnId, args: &[Option<Constant>]) -> Option<Folded> {
    let found = |lookup: Lookup, default: Option<usize>| match (lookup, default) {
        (Lookup::Found(item), _) => Folded::Item(item),
        (Lookup::Missing, Some(index)) => Folded::Argument(index),
        (Lookup::Missing, None) => Folded::Value(Constant::Nil),
    };
    match (core, args) {
        (CoreFnId::Get, [Some(collection), Some(key), rest @ ..]) if rest.len() <= 1 => {
            let default = (!rest.is_empty()).then_some(2);
            Some(found(get(program, collection, key)?, default))
        }
        (CoreFnId::GetIn, [Some(collection), Some(path), rest @ ..]) if rest.len() <= 1 => {
            get_in(program, collection, path, !rest.is_empty())
        }
        (CoreFnId::Nth, [Some(collection), Some(Constant::Number(index)), rest @ ..])
            if rest.len() <= 1 =>
        {
            let id = collection_of(collection)?;
            let (shape, items) = shape(program, id)?;
            let index = *index;
            let item = match shape {
                // `vectorNth`: a number in range, truncated.
                Shape::Vector if index >= 0.0 && index < items.len() as f64 => {
                    Some(items[index.trunc() as usize])
                }
                // A list counts down to an exact zero.
                Shape::List
                    if index.fract() == 0.0 && index >= 0.0 && index < items.len() as f64 =>
                {
                    Some(items[index as usize])
                }
                Shape::Vector | Shape::List => None,
                Shape::Map { .. } | Shape::Set { .. } => return None,
            };
            match (item, rest.is_empty()) {
                (Some(item), _) => Some(Folded::Item(item)),
                // Out of range without a default throws, so the call stays.
                (None, true) => None,
                (None, false) => Some(Folded::Argument(2)),
            }
        }
        (
            CoreFnId::First | CoreFnId::Second | CoreFnId::Last | CoreFnId::Peek,
            [Some(collection)],
        ) => {
            let (shape, items) = shape(program, collection_of(collection)?)?;
            let item = match (core, shape) {
                (CoreFnId::First, Shape::Vector | Shape::List) => items.first(),
                (CoreFnId::Second, Shape::Vector | Shape::List) => items.get(1),
                // `peek` is the end a vector grows at and the head of a list.
                (CoreFnId::Last | CoreFnId::Peek, Shape::Vector)
                | (CoreFnId::Last, Shape::List) => items.last(),
                (CoreFnId::Peek, Shape::List) if !items.is_empty() => items.first(),
                _ => return None,
            };
            Some(item.map_or(Folded::Value(Constant::Nil), |item| Folded::Item(*item)))
        }
        (CoreFnId::Rest | CoreFnId::Next, [Some(collection)]) => {
            let (shape, items) = shape(program, collection_of(collection)?)?;
            match shape {
                // The tail of a list, or the empty list.
                Shape::List if items.len() > 1 => Some(Folded::List(items[1..].to_vec())),
                Shape::List | Shape::Vector if items.len() <= 1 => {
                    Some(if core == CoreFnId::Rest {
                        Folded::List(Vec::new())
                    } else {
                        Folded::Value(Constant::Nil)
                    })
                }
                // The rest of a longer vector is a seq over it.
                _ => None,
            }
        }
        (CoreFnId::Seq | CoreFnId::NotEmpty, [Some(collection)]) => {
            let (shape, items) = shape(program, collection_of(collection)?)?;
            if items.is_empty() {
                Some(Folded::Value(Constant::Nil))
            } else if core == CoreFnId::NotEmpty || shape == Shape::List {
                // `notEmpty` returns its argument, and the seq of a list is
                // the list.
                Some(Folded::Argument(0))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// `get` of `key` in `value`: a lookup in a constant collection, and nothing
/// in `nil`, a number, boolean, keyword or symbol (`lookup` finds nothing in
/// them). A string finds a character at a key that compares as an index in
/// range (`key > -1 && key < length`), which a keyword (`:k`) or `nil` never
/// does; other keys are left alone.
fn get(program: &Program, value: &Constant, key: &Constant) -> Option<Lookup> {
    match value {
        Constant::Collection(id) => lookup(program, *id, key),
        Constant::String(_) if !matches!(key, Constant::Nil | Constant::Keyword(_)) => None,
        _ => Some(Lookup::Missing),
    }
}

/// `get-in` of the constant `path` (a vector or list of keys) in `value`,
/// with a default when `defaulted`: each step is a `get`, which with a
/// default stops at the first missing key.
fn get_in(program: &Program, value: &Constant, path: &Constant, defaulted: bool) -> Option<Folded> {
    let keys = path_keys(program, path)?;
    if keys.is_empty() {
        return Some(Folded::Argument(0));
    }
    walk(program, value.clone(), None, keys, defaulted)
}

/// The keys of a constant `get-in` path: a vector or list.
fn path_keys<'p>(program: &'p Program, path: &Constant) -> Option<&'p [ExprId]> {
    match shape(program, collection_of(path)?)? {
        (Shape::Vector | Shape::List, keys) => Some(keys),
        _ => None,
    }
}

/// The rest of a `get-in` from `current`, the constant item at `item` (or
/// not an item), through `keys`.
fn walk(
    program: &Program,
    mut current: Constant,
    mut item: Option<ExprId>,
    keys: &[ExprId],
    defaulted: bool,
) -> Option<Folded> {
    for key in keys {
        let key = Constant::of(program, *key)?;
        match get(program, &current, &key)? {
            Lookup::Found(found) => {
                current = Constant::item(program, found)?;
                item = Some(found);
            }
            Lookup::Missing if defaulted => return Some(Folded::Argument(2)),
            Lookup::Missing => {
                current = Constant::Nil;
                item = None;
            }
        }
    }
    Some(item.map_or(Folded::Value(current), Folded::Item))
}

/// The result of calling `core` on `args` when the runtime's definition is
/// reproduced here; the caller has checked that the call is `pure` for these
/// arguments. `None` leaves the call alone.
pub fn evaluate(program: &Program, core: CoreFnId, args: &[Constant]) -> Option<Constant> {
    use Constant::{Bool, Number};
    let numbers = || -> Option<Vec<f64>> {
        args.iter()
            .map(|arg| match arg {
                Number(value) => Some(*value),
                _ => None,
            })
            .collect()
    };
    // `numericFold`: a left fold over two or more operands.
    let fold = |operation: fn(f64, f64) -> f64| -> Option<f64> {
        let values = numbers()?;
        let (first, rest) = values.split_first()?;
        Some(
            rest.iter()
                .fold(*first, |value, item| operation(value, *item)),
        )
    };
    // `comparison`: one operand is true, else every adjacent pair passes.
    let chain = |test: fn(f64, f64) -> bool| -> Option<Constant> {
        let values = numbers()?;
        Some(Bool(values.windows(2).all(|pair| test(pair[0], pair[1]))))
    };
    let one = || -> Option<f64> {
        match args {
            [Number(value)] => Some(*value),
            _ => None,
        }
    };
    let two = || -> Option<(f64, f64)> {
        match args {
            [Number(n), Number(d)] => Some((*n, *d)),
            _ => None,
        }
    };
    let fix = |x: f64| if x >= 0.0 { x.floor() } else { x.ceil() };
    let quot = |n: f64, d: f64| fix((n - n % d) / d);
    let extrema = |test: fn(f64, f64) -> bool| -> Option<Constant> {
        let values = numbers()?;
        let (first, rest) = values.split_first()?;
        Some(Number(rest.iter().fold(*first, |a, b| {
            if a.is_nan() {
                a
            } else if b.is_nan() || !test(a, *b) {
                *b
            } else {
                a
            }
        })))
    };
    Some(match core {
        CoreFnId::Add => Number(fold(|a, b| a + b).unwrap_or(0.0)),
        CoreFnId::Multiply if args.is_empty() => Number(1.0),
        CoreFnId::Multiply => Number(fold(|a, b| a * b)?),
        CoreFnId::Subtract if args.len() == 1 => Number(-one()?),
        CoreFnId::Subtract => Number(fold(|a, b| a - b)?),
        CoreFnId::Divide if args.len() == 1 => Number(1.0 / one()?),
        CoreFnId::Divide => Number(fold(|a, b| a / b)?),
        CoreFnId::Inc => Number(one()? + 1.0),
        CoreFnId::Dec => Number(one()? - 1.0),
        CoreFnId::Abs => Number(one()?.abs()),
        CoreFnId::Max => extrema(|a, b| a > b)?,
        CoreFnId::Min => extrema(|a, b| a < b)?,
        CoreFnId::Mod => {
            let (n, d) = two()?;
            Number(((n % d) + d) % d)
        }
        CoreFnId::Quot => {
            let (n, d) = two()?;
            Number(quot(n, d))
        }
        CoreFnId::Rem => {
            let (n, d) = two()?;
            Number(n - d * quot(n, d))
        }
        CoreFnId::Less => chain(|a, b| a < b)?,
        CoreFnId::LessEqual => chain(|a, b| a <= b)?,
        CoreFnId::Greater => chain(|a, b| a > b)?,
        CoreFnId::GreaterEqual => chain(|a, b| a >= b)?,
        // `numericEqual` is `equal` on numbers, which is `===`.
        CoreFnId::NumericEqual => chain(|a, b| a == b)?,
        CoreFnId::Equal => Bool(equal_all(program, args)?),
        CoreFnId::NotEqual => Bool(!equal_all(program, args)?),
        CoreFnId::Identical => match args {
            [a, b]
                if [a, b].iter().all(|arg| {
                    !matches!(
                        arg,
                        Constant::Keyword(_) | Constant::Symbol(_) | Constant::Collection(_)
                    )
                }) =>
            {
                Bool(equal(program, a, b)?)
            }
            _ => return None,
        },
        // Collections (light-cljs-221): the caller has checked that core
        // protocols are intact, so the runtime's own implementations answer.
        CoreFnId::Count => Number(match args {
            [Constant::Nil] => 0.0,
            [Constant::String(units)] => units.len() as f64,
            [Constant::Collection(id)] => count(program, *id)? as f64,
            _ => return None,
        }),
        // Every constant collection is counted, and has entries exactly when
        // it has items, however its keys merge.
        CoreFnId::IsEmpty => Bool(match args {
            [Constant::Nil] => true,
            [Constant::String(units)] => units.is_empty(),
            [Constant::Collection(id)] => shape(program, *id)?.1.is_empty(),
            _ => return None,
        }),
        CoreFnId::Contains => match args {
            [Constant::Collection(id), key] => match shape(program, *id)? {
                // `contains` on a vector: an integer index in range.
                (Shape::Vector, items) => Bool(matches!(key, Constant::Number(index)
                    if index.fract() == 0.0 && *index >= 0.0 && *index < items.len() as f64)),
                (Shape::List, _) => return None,
                _ => Bool(lookup(program, *id, key)? != Lookup::Missing),
            },
            _ => return None,
        },
        CoreFnId::IsSymbol => Bool(matches!(args.first()?, Constant::Symbol(_))),
        CoreFnId::IsColl
        | CoreFnId::IsList
        | CoreFnId::IsSeq
        | CoreFnId::IsSeqable
        | CoreFnId::IsSequential
        | CoreFnId::IsVector
        | CoreFnId::IsMap
        | CoreFnId::IsSet
        | CoreFnId::IsAssociative
        | CoreFnId::IsCounted => {
            let [Constant::Collection(id)] = args else {
                return None;
            };
            let (shape, _) = shape(program, *id)?;
            let (vector, list) = (shape == Shape::Vector, shape == Shape::List);
            let map = matches!(shape, Shape::Map { .. });
            Bool(match core {
                CoreFnId::IsColl | CoreFnId::IsSeqable | CoreFnId::IsCounted => true,
                // A list is its own seq.
                CoreFnId::IsList | CoreFnId::IsSeq => list,
                CoreFnId::IsSequential => vector || list,
                CoreFnId::IsVector => vector,
                CoreFnId::IsMap => map,
                CoreFnId::IsSet => matches!(shape, Shape::Set { .. }),
                _ => vector || map,
            })
        }
        CoreFnId::Not => Bool(!args.first()?.truthy()),
        CoreFnId::Boolean => Bool(args.first()?.truthy()),
        CoreFnId::IsNil => Bool(matches!(args.first()?, Constant::Nil)),
        CoreFnId::IsSome => Bool(!matches!(args.first()?, Constant::Nil)),
        CoreFnId::IsTrue => Bool(matches!(args.first()?, Constant::Bool(true))),
        CoreFnId::IsFalse => Bool(matches!(args.first()?, Constant::Bool(false))),
        CoreFnId::IsBoolean => Bool(matches!(args.first()?, Constant::Bool(_))),
        CoreFnId::IsNumber => Bool(matches!(args.first()?, Constant::Number(_))),
        CoreFnId::IsString => Bool(matches!(args.first()?, Constant::String(_))),
        CoreFnId::IsKeyword => Bool(matches!(args.first()?, Constant::Keyword(_))),
        CoreFnId::IsZero => Bool(matches!(args.first()?, Constant::Number(value) if *value == 0.0)),
        CoreFnId::IsPositive => Bool(one()? > 0.0),
        CoreFnId::IsNegative => Bool(one()? < 0.0),
        CoreFnId::Str => {
            let mut units = Vec::new();
            for arg in args {
                match arg {
                    Constant::Nil => (),
                    Constant::String(string) => units.extend_from_slice(string),
                    Constant::Bool(value) => units.extend(value.to_string().encode_utf16()),
                    Constant::Number(value) => units.extend(js_number(*value).encode_utf16()),
                    // A keyword's or symbol's text is its printed form, and a
                    // collection's its printing, which this does not
                    // reproduce.
                    Constant::Keyword(_) | Constant::Symbol(_) | Constant::Collection(_) => {
                        return None;
                    }
                }
            }
            Constant::String(units)
        }
        _ => return None,
    })
}

/// `equal` on scalars and symbols: `nil` equals only `nil`, keywords and
/// symbols compare by spelling and everything else with `===`; `None` for a
/// collection, which this does not compare.
fn equal(program: &Program, a: &Constant, b: &Constant) -> Option<bool> {
    Some(match (a, b) {
        (Constant::Collection(_), _) | (_, Constant::Collection(_)) => return None,
        (Constant::Keyword(a), Constant::Keyword(b))
        | (Constant::Symbol(a), Constant::Symbol(b)) => {
            let (a, b) = (&program.named_literals[a.0], &program.named_literals[b.0]);
            let spelling = |name: Option<StringId>| name.map(|name| &program.strings[name.0]);
            spelling(a.namespace) == spelling(b.namespace)
                && program.strings[a.name.0] == program.strings[b.name.0]
        }
        (Constant::Number(a), Constant::Number(b)) => a == b,
        (a, b) => a == b,
    })
}

/// `equal` over its arguments, pairwise.
fn equal_all(program: &Program, args: &[Constant]) -> Option<bool> {
    if args.is_empty() {
        return None;
    }
    // The runtime stops at the first unequal pair.
    for pair in args.windows(2) {
        if !equal(program, &pair[0], &pair[1])? {
            return Some(false);
        }
    }
    Some(true)
}

/// ECMAScript `Number::toString`, which `str` applies to numbers.
pub fn js_number(value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value == 0.0 {
        return "0".into();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if value < 0.0 {
        return format!("-{}", js_number(-value));
    }
    // Rust's shortest round-trip digits are the `s` of the specification.
    let scientific = format!("{value:e}");
    let (mantissa, exponent) = scientific.split_once('e').expect("exponent notation");
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exponent.parse::<i32>().expect("an integer exponent") + 1;
    if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let sign = if n > 0 { '+' } else { '-' };
        let exponent = (n - 1).abs();
        if k == 1 {
            format!("{digits}e{sign}{exponent}")
        } else {
            format!("{}.{}e{sign}{exponent}", &digits[..1], &digits[1..])
        }
    }
}

/// No top-level position: unreachable from the root.
pub(crate) const NOWHERE: u32 = u32::MAX;

/// Fold `program` in place and return how many expressions were rewritten.
pub fn fold(program: &mut Program, built: &mut Vec<bool>) -> usize {
    let mut top = Vec::new();
    flatten(program, program.root, &mut top);
    let facts = ProgramFacts::new(program, &top);
    let count = program.exprs.len();
    built.resize(count, false);

    // The first top-level form that reaches each expression.
    let mut position = vec![NOWHERE; count];
    for (index, &form) in top.iter().enumerate() {
        let mut pending = vec![form];
        while let Some(id) = pending.pop() {
            if position[id.0] == NOWHERE {
                position[id.0] = index as u32;
                pending.extend(children(program, id));
            }
        }
    }

    // Constant globals: defined by one `def` with a value that the root
    // reaches, a direct top-level form, and assigned or defined by nothing
    // else. A `declare` defines no value and does not count.
    let (globals, collections) = constant_globals(program, &top, &position);

    // Locals that are never substituted, and those whose value may change
    // after their binding (a type field, a `js*` hole, a `set!` target).
    let mut pinned = vec![false; program.locals.len()];
    let mut assignable = vec![false; program.locals.len()];
    for local in program.field_bindings.keys() {
        pinned[local.0] = true;
        assignable[local.0] = true;
    }
    for expr in &program.exprs {
        let methods: &[crate::analyzer::FnMethod] = match &expr.kind {
            ExprKind::SetGlobal { target, .. } => {
                if let ExprKind::Local(local) = program.exprs[target.0].kind {
                    pinned[local.0] = true;
                    assignable[local.0] = true;
                }
                continue;
            }
            ExprKind::Fn { methods, .. } => methods,
            ExprKind::Reify {
                captures, methods, ..
            } => {
                for local in captures {
                    pinned[local.0] = true;
                }
                for (_, method) in methods {
                    method.captures.iter().for_each(|l| pinned[l.0] = true);
                }
                continue;
            }
            ExprKind::DefType { methods, .. } | ExprKind::ExtendType { methods, .. } => {
                for (_, method) in methods {
                    method.captures.iter().for_each(|l| pinned[l.0] = true);
                }
                continue;
            }
            ExprKind::JsTemplate { args, .. } => {
                for arg in args {
                    if let ExprKind::Local(local) = program.exprs[arg.0].kind {
                        pinned[local.0] = true;
                        assignable[local.0] = true;
                    }
                }
                continue;
            }
            _ => continue,
        };
        for method in methods {
            method.captures.iter().for_each(|l| pinned[l.0] = true);
        }
    }

    // Map keys and set members stay as they are (a local they read keeps its
    // binding).
    let fenced = fenced_positions(program);
    for (index, expr) in program.exprs.iter().enumerate() {
        if let ExprKind::Local(local) = expr.kind
            && fenced[index]
        {
            pinned[local.0] = true;
        }
    }

    // How many expressions the root reaches each one from, and how often the
    // reachable code reads each local: earlier rounds leave the expressions
    // they replaced in the arena, whose reads no longer count.
    let mut parents = vec![0u8; count];
    let mut reached = vec![false; count];
    let reads = reachable_reads(program, Some(&mut parents), &mut reached);

    let mut kept = vec![false; count];
    kept[program.root.0] = true;
    for id in program.namespace_entries.keys() {
        kept[id.0] = true;
    }
    for id in &program.macro_functions {
        if id.0 < count {
            kept[id.0] = true;
        }
    }
    let replaced = facts.replaced_names();
    let locals = program.locals.len();
    let mut folder = Folder {
        program,
        facts,
        replaced,
        position,
        globals,
        collections,
        pinned,
        assignable,
        reads,
        kept,
        fenced,
        built,
        parents,
        discarded: vec![false; count],
        locals: vec![None; locals],
        truths: vec![false; locals],
        bound: vec![None; locals],
        classes: vec![ConstantClass::Unknown; count],
        visited: vec![false; count],
        folded: 0,
    };
    let root = folder.program.root;
    folder.visit(root);
    folder.sweep();
    folder.folded
}

/// How often the code the root reaches reads each local, by local. Fills
/// `reached` (by expression) and, when given, `parents`: how many reached
/// expressions each one is a child of.
fn reachable_reads(
    program: &Program,
    mut parents: Option<&mut Vec<u8>>,
    reached: &mut [bool],
) -> Vec<u32> {
    let mut reads = vec![0u32; program.locals.len()];
    reached[program.root.0] = true;
    let mut pending = vec![program.root];
    while let Some(id) = pending.pop() {
        if let ExprKind::Local(local) = program.exprs[id.0].kind {
            reads[local.0] = reads[local.0].saturating_add(1);
        }
        for child in children(program, id) {
            if let Some(parents) = parents.as_deref_mut() {
                parents[child.0] = parents[child.0].saturating_add(1);
            }
            if !std::mem::replace(&mut reached[child.0], true) {
                pending.push(child);
            }
        }
    }
    reads
}

/// Lookups of a constant key in a map literal, or in a local a `let` binds to
/// one (light-cljs-221; "Constant folding extension points" in
/// docs/advanced-optimization.md): by call, the index of the key among the
/// map literal's keys. `Advanced` emission reads the entry at that index,
/// `rt$maps.get(m)[i].value`, instead of calling `get`. The calls are `(:k m)`,
/// `(get m k)`, `(m k)` and their forms with a default that has no effect
/// (which a found key does not evaluate to). A local is bound by `let`
/// (never assigned: no type field, `js*` hole or `set!` target), possibly
/// through other locals, to a `Map` literal; the literal's keys are constants
/// (at most one of them a collection) that no two may be `=` ([`distinct`]), so the
/// literal's builder (`mapFromArray`, `mapFromArrays` or a key shape) makes
/// one entry per key, in order; the lookup key is `=` to exactly one of them
/// and provably unequal to the others ([`same_key`]). Needs core protocols
/// intact and `get` not replaced, so that the runtime's own `lookup` runs.
pub fn entry_lookups(program: &Program, facts: &ProgramFacts) -> HashMap<ExprId, usize> {
    let mut lookups = HashMap::new();
    if !facts.core_protocols_intact() || facts.replaced(CoreFnId::Get) {
        return lookups;
    }
    let mut reached = vec![false; program.exprs.len()];
    reachable_reads(program, None, &mut reached);
    let mut assignable = vec![false; program.locals.len()];
    for local in program.field_bindings.keys() {
        assignable[local.0] = true;
    }
    let mut bound = vec![None; program.locals.len()];
    for (index, expr) in program.exprs.iter().enumerate() {
        let targets: &[ExprId] = match &expr.kind {
            ExprKind::SetGlobal { target, .. } => std::slice::from_ref(target),
            ExprKind::JsTemplate { args, .. } => args,
            ExprKind::Let { bindings, .. } if reached[index] => {
                for (local, init) in bindings {
                    bound[local.0] = Some(*init);
                }
                continue;
            }
            _ => continue,
        };
        for target in targets {
            if let ExprKind::Local(local) = program.exprs[target.0].kind {
                assignable[local.0] = true;
            }
        }
    }
    // The items of the map literal a local is bound to.
    let map = |mut id: ExprId| -> Option<&[ExprId]> {
        for _ in 0..program.locals.len() + 1 {
            match &program.exprs[id.0].kind {
                ExprKind::Local(local) if !assignable[local.0] => id = bound[local.0]?,
                ExprKind::Map(items) => return Some(items),
                _ => return None,
            }
        }
        None
    };
    for (index, expr) in program.exprs.iter().enumerate() {
        let ExprKind::Call { callee, args } = &expr.kind else {
            continue;
        };
        if !reached[index] {
            continue;
        }
        let (map_value, key, rest) = match (&program.exprs[callee.0].kind, &args[..]) {
            (ExprKind::Keyword(literal), [map, rest @ ..]) => {
                (*map, Constant::Keyword(*literal), rest)
            }
            (ExprKind::Symbol(literal), [map, rest @ ..]) => {
                (*map, Constant::Symbol(*literal), rest)
            }
            (ExprKind::CoreFn(CoreFnId::Get), [map, key, rest @ ..]) => {
                let Some(key) = Constant::of(program, *key) else {
                    continue;
                };
                (*map, key, rest)
            }
            (ExprKind::Local(_), [key, rest @ ..]) => {
                let Some(key) = Constant::of(program, *key) else {
                    continue;
                };
                (*callee, key, rest)
            }
            _ => continue,
        };
        if rest.len() > 1
            || !rest
                .iter()
                .all(|arg| effect_free(program, &program.exprs[arg.0].kind))
        {
            continue;
        }
        let Some(items) = map(map_value) else {
            continue;
        };
        let keys = || items.iter().step_by(2).copied();
        // Every key a constant: a scalar, a symbol or a collection literal.
        if !keys().all(|key| Constant::item(program, key).is_some()) || !distinct(program, keys()) {
            continue;
        }
        let mut found = None;
        for (position, item) in keys().enumerate() {
            match same_key(program, &key, item) {
                Some(true) if found.is_none() => found = Some(position),
                Some(false) => (),
                _ => {
                    found = None;
                    break;
                }
            }
        }
        if let Some(position) = found {
            lookups.insert(ExprId(index), position);
        }
    }
    lookups
}

/// By global: the sole `def` with a value that defines it, when the root
/// reaches it (`position` is not [`NOWHERE`]) and nothing else assigns or
/// defines the global (`set!`, `binding`, `with-redefs`, a `js*` hole, a
/// type or protocol). A `declare` defines no value and does not count.
pub(crate) fn sole_definitions(program: &Program, position: &[u32]) -> Vec<Option<ExprId>> {
    let count = program.globals.len();
    let mut definitions: Vec<(u32, Option<ExprId>)> = vec![(0, None); count];
    let mut assigned = vec![false; count];
    for (index, expr) in program.exprs.iter().enumerate() {
        let targets: Vec<ExprId> = match &expr.kind {
            ExprKind::SetGlobal { target, .. } => vec![*target],
            ExprKind::Binding { bindings, .. } => {
                bindings.iter().map(|(target, _)| *target).collect()
            }
            // Raw text may assign its holes.
            ExprKind::JsTemplate { args, .. } => args.clone(),
            ExprKind::Def {
                global, init, once, ..
            } => {
                if position[index] != NOWHERE && (init.is_some() || *once) {
                    let definition = &mut definitions[global.0];
                    definition.0 += 1;
                    definition.1 = Some(ExprId(index));
                }
                continue;
            }
            ExprKind::Global(_) => continue,
            kind => {
                for global in crate::application::globals(kind) {
                    assigned[global.0] = true;
                }
                continue;
            }
        };
        for target in targets {
            if let ExprKind::Global(global) = program.exprs[target.0].kind {
                assigned[global.0] = true;
            }
        }
    }
    definitions
        .into_iter()
        .zip(assigned)
        .map(|(definition, assigned)| match definition {
            (1, definition) if !assigned => definition,
            _ => None,
        })
        .collect()
}

/// For each global, the top-level position of its definition and its value
/// when it is a `^:const` (or `goog-define`) global that [`fold`]
/// substitutes: its sole definition is a direct top-level `def` of a
/// [`Constant`]. Second, for each `^:const` global so defined by a collection
/// literal, the position and the literal, which [`fold`] folds lookups
/// against but never substitutes.
#[allow(clippy::type_complexity)]
fn constant_globals(
    program: &Program,
    top: &[ExprId],
    position: &[u32],
) -> (Vec<Option<(u32, ExprKind)>>, Vec<Option<(u32, ExprId)>>) {
    let definitions = sole_definitions(program, position);
    let mut globals = vec![None; program.globals.len()];
    let mut collections = vec![None; program.globals.len()];
    for (index, binding) in program.globals.iter().enumerate() {
        let Some(def) = definitions[index] else {
            continue;
        };
        let at = position[def.0];
        if !binding.constant || top.get(at as usize) != Some(&def) {
            continue;
        }
        let ExprKind::Def {
            init: Some(init),
            once: false,
            test: None,
            ..
        } = program.exprs[def.0].kind
        else {
            continue;
        };
        let literal = crate::application::unwrap_do(program, init);
        if Constant::of(program, literal).is_some() {
            globals[index] = Some((at, program.exprs[literal.0].kind.clone()));
        } else if shape(program, literal).is_some() {
            collections[index] = Some((at, literal));
        }
    }
    (globals, collections)
}

/// The direct top-level forms of the program.
fn flatten(program: &Program, id: ExprId, top: &mut Vec<ExprId>) {
    if let ExprKind::Do(items) = &program.exprs[id.0].kind {
        for item in items {
            flatten(program, *item, top);
        }
    } else {
        top.push(id);
    }
}

struct Folder<'p> {
    program: &'p mut Program,
    facts: ProgramFacts,
    /// The runtime names of the core operations the program may replace,
    /// sorted, as the emitter's `constant_nodes` takes them.
    replaced: Vec<&'static str>,
    /// By expression: the first top-level form that reaches it.
    position: Vec<u32>,
    /// By global: the top-level position of its sole definition and its
    /// constant value, for `^:const` globals.
    globals: Vec<Option<(u32, ExprKind)>>,
    /// By global: the top-level position of its sole definition and the
    /// collection literal it defines, for `^:const` globals.
    collections: Vec<Option<(u32, ExprId)>>,
    /// By local: captured by a closure or type, a type field, or a `js*` hole.
    pinned: Vec<bool>,
    /// By local: a type field, a `js*` hole or a `set!` target, whose value
    /// may change after its binding.
    assignable: Vec<bool>,
    /// By local: the expressions the root reaches that read it.
    reads: Vec<u32>,
    /// By expression: its identity matters (top-level `do` forms, namespace
    /// entries, macro functions), so it keeps its kind and no other
    /// expression takes it over.
    kept: Vec<bool>,
    /// By expression: a map key or set member, which never becomes a constant
    /// here ([`fenced_positions`]).
    fenced: Vec<bool>,
    /// By expression: a literal built at each evaluation ([`built_literals`]),
    /// a mark that moves with the literal when an expression takes it over.
    built: &'p mut Vec<bool>,
    /// By expression: how many expressions the root reaches it from.
    parents: Vec<u8>,
    /// By expression: a `do` form whose value is discarded, set by the `do`
    /// around it before it is visited.
    discarded: Vec<bool>,
    /// By local: the constant substituted for it.
    locals: Vec<Option<ExprKind>>,
    /// By local: bound by a `let` to a literal collection or `fn`, so true.
    truths: Vec<bool>,
    /// By local: the initializer a `let` binds it to and never changes, which
    /// [`Folder::collection`] follows to a constant collection.
    bound: Vec<Option<ExprId>>,
    /// By expression: the constant pool's classification
    /// ([`crate::emitter::constant_nodes`]), memoized.
    classes: Vec<ConstantClass>,
    visited: Vec<bool>,
    folded: usize,
}

impl Folder<'_> {
    fn visit(&mut self, id: ExprId) {
        if std::mem::replace(&mut self.visited[id.0], true) {
            return;
        }
        let kind = std::mem::replace(&mut self.program.exprs[id.0].kind, ExprKind::Nil);
        let kind = self.rewrite(id, kind);
        self.program.exprs[id.0].kind = kind;
    }

    /// `child` produces the value of `parent`, so it is discarded when
    /// `parent` is and nothing else reads `parent`.
    fn pass_discard(&mut self, parent: ExprId, child: ExprId) {
        if !self.visited[child.0] {
            self.discarded[child.0] = self.discarded[parent.0] && self.parents[parent.0] <= 1;
        }
    }

    /// The constant `id` evaluates to, once visited.
    fn constant(&self, id: ExprId) -> Option<Constant> {
        Constant::of(self.program, id)
    }

    /// The kind of `child`, for its parent to take over, unless the child's
    /// identity matters.
    fn adopt(&mut self, parent: ExprId, child: ExprId) -> Option<ExprKind> {
        if self.kept[child.0]
            || (self.fenced[parent.0]
                && constant_like(self.program, &self.program.exprs[child.0].kind))
        {
            return None;
        }
        self.built[parent.0] = self.built[child.0];
        self.folded += 1;
        Some(self.program.exprs[child.0].kind.clone())
    }

    fn rewrite(&mut self, id: ExprId, kind: ExprKind) -> ExprKind {
        match kind {
            ExprKind::Global(global) => match &self.globals[global.0] {
                Some((at, value))
                    if !self.fenced[id.0]
                        && self.position[id.0] != NOWHERE
                        && self.position[id.0] > *at =>
                {
                    self.folded += 1;
                    value.clone()
                }
                _ => kind,
            },
            ExprKind::Local(local) => match &self.locals[local.0] {
                Some(value) => {
                    self.folded += 1;
                    value.clone()
                }
                None => kind,
            },
            ExprKind::Let { bindings, body } => {
                // Substitution rewrites the reads while it visits the body, so
                // a body another parent already visited keeps its bindings.
                let shared = self.visited[body.0];
                let mut kept = Vec::with_capacity(bindings.len());
                for (local, init) in bindings {
                    self.visit(init);
                    // Lookups through the local may fold against the value it
                    // is bound to.
                    if !shared && !self.assignable[local.0] {
                        self.bound[local.0] = Some(init);
                    }
                    // Another local bound to a collection literal whose value
                    // cannot change: the aliases a destructuring binds, such
                    // as its `:as` local. Aliases of other values stay, since
                    // passes after this one read `and` and `or` from the
                    // local their expansions bind.
                    let alias = !self.pinned[local.0]
                        && !shared
                        && matches!(self.program.exprs[init.0].kind,
                            ExprKind::Local(other) if !self.assignable[other.0])
                        && (self.collection(init).is_some() || self.keyed(init).is_some());
                    let value = &self.program.exprs[init.0].kind;
                    if self.pinned[local.0] || shared {
                        kept.push((local, init));
                    } else if Constant::of_kind(self.program, value).is_some() || alias {
                        // A constant or such an alias is read in its place.
                        self.locals[local.0] = Some(value.clone());
                        self.folded += 1;
                    } else if self.reads[local.0] == 0 && effect_free(self.program, value) {
                        self.folded += 1;
                    } else {
                        // A literal collection or `fn` is true wherever the
                        // local is tested.
                        if literal_truth(self.program, value) == Some(true) {
                            self.truths[local.0] = true;
                        }
                        kept.push((local, init));
                    }
                }
                self.pass_discard(id, body);
                self.visit(body);
                if kept.is_empty()
                    && !scoped(self.program, body)
                    && let Some(kind) = self.adopt(id, body)
                {
                    return kind;
                }
                ExprKind::Let {
                    bindings: kept,
                    body,
                }
            }
            ExprKind::If { test, yes, no } => {
                self.visit(test);
                self.member_set(test);
                self.pass_discard(id, yes);
                self.pass_discard(id, no);
                let truth = match self.program.exprs[test.0].kind {
                    ExprKind::Local(local) if self.truths[local.0] => Some(true),
                    ref kind => literal_truth(self.program, kind),
                };
                if let Some(truth) = truth {
                    let branch = if truth { yes } else { no };
                    self.visit(branch);
                    if let Some(kind) = self.adopt(id, branch) {
                        return kind;
                    }
                }
                self.visit(yes);
                self.visit(no);
                ExprKind::If { test, yes, no }
            }
            ExprKind::BooleanHint(inner) => {
                self.visit(inner);
                if self.constant(inner).is_some()
                    && let Some(kind) = self.adopt(id, inner)
                {
                    return kind;
                }
                ExprKind::BooleanHint(inner)
            }
            ExprKind::Do(items) => {
                // Forms before the last are discarded, and so is the last one
                // of a discarded `do` that nothing else reads.
                let discarded = self.discarded[id.0] && self.parents[id.0] <= 1;
                let last = items.len().saturating_sub(1);
                for (index, item) in items.iter().enumerate() {
                    if !self.visited[item.0] {
                        self.discarded[item.0] = discarded || index != last;
                    }
                    self.visit(*item);
                }
                if self.kept[id.0] || items.is_empty() {
                    return ExprKind::Do(items);
                }
                let forms: Vec<ExprId> = items
                    .iter()
                    .enumerate()
                    .filter(|(index, item)| {
                        (*index == last && !discarded)
                            || self.kept[item.0]
                            || !effect_free(self.program, &self.program.exprs[item.0].kind)
                    })
                    .map(|(_, item)| *item)
                    .collect();
                self.folded += items.len() - forms.len();
                if forms.is_empty() {
                    return ExprKind::Nil;
                }
                if let [form] = forms[..]
                    && !scoped(self.program, form)
                    && let Some(kind) = self.adopt(id, form)
                {
                    return kind;
                }
                ExprKind::Do(forms)
            }
            ExprKind::Call { callee, args } => {
                self.visit(callee);
                for arg in &args {
                    self.visit(*arg);
                }
                if let ExprKind::CoreFn(core) = self.program.exprs[callee.0].kind {
                    if !self.fenced[id.0]
                        && let Some(value) = self.call(core, &args)
                    {
                        self.folded += 1;
                        return value.into_kind(self.program);
                    }
                    if !self.fenced[id.0]
                        && let Some(kind) = self.select(id, core, &args)
                    {
                        self.folded += 1;
                        return kind;
                    }
                    if let Some(args) = self.prefix(core, &args) {
                        self.folded += 1;
                        return ExprKind::Call { callee, args };
                    }
                } else if !self.fenced[id.0]
                    && let Some(kind) = self.invoke(id, callee, &args)
                {
                    self.folded += 1;
                    return kind;
                }
                ExprKind::Call { callee, args }
            }
            // These read a global's binding rather than its value.
            ExprKind::Var { .. } | ExprKind::Exists(_) | ExprKind::BindingExists(_) => kind,
            // Assignment targets are not reads.
            ExprKind::SetGlobal { target, value } => {
                if !matches!(
                    self.program.exprs[target.0].kind,
                    ExprKind::Global(_) | ExprKind::Local(_) | ExprKind::CoreFn(_)
                ) {
                    self.visit(target);
                }
                self.visit(value);
                ExprKind::SetGlobal { target, value }
            }
            ExprKind::Binding { bindings, body } => {
                for (_, init) in &bindings {
                    self.visit(*init);
                }
                self.visit(body);
                ExprKind::Binding { bindings, body }
            }
            // Case tests are the constants it dispatches on, as analyzed.
            ExprKind::Case {
                strict,
                keyword_tests,
                test,
                branches,
                default,
            } => {
                self.visit(test);
                for (_, branch) in &branches {
                    self.visit(*branch);
                }
                if let Some(default) = default {
                    self.visit(default);
                }
                ExprKind::Case {
                    strict,
                    keyword_tests,
                    test,
                    branches,
                    default,
                }
            }
            kind => {
                for child in kind_children(self.program, &kind) {
                    self.visit(child);
                }
                kind
            }
        }
    }

    /// The arguments of a call of `+`, `-`, `*` or `/` whose leading number
    /// literals are folded into one: the runtime folds its operands from the
    /// left (`numericFold`) and JavaScript's operators associate to the left,
    /// so `(+ 1 2 x)` is `(+ 3 x)` whatever `x` is.
    fn prefix(&mut self, core: CoreFnId, args: &[ExprId]) -> Option<Vec<ExprId>> {
        if !matches!(
            core,
            CoreFnId::Add | CoreFnId::Subtract | CoreFnId::Multiply | CoreFnId::Divide
        ) || self.facts.replaced(core)
        {
            return None;
        }
        let numbers: Vec<Constant> = args
            .iter()
            .map_while(|arg| match self.program.exprs[arg.0].kind {
                ExprKind::Number(value) => Some(Constant::Number(value)),
                _ => None,
            })
            .collect();
        if numbers.len() < 2 || numbers.len() == args.len() {
            return None;
        }
        let value = evaluate(self.program, core, &numbers)?;
        let kind = value.into_kind(self.program);
        let folded = self.push(args[0], kind);
        Some(
            std::iter::once(folded)
                .chain(args[numbers.len()..].iter().copied())
                .collect(),
        )
    }

    /// The value of a call of `core` on `args` when all are constants and the
    /// call is `pure`. A collection argument also needs core protocols
    /// intact: the predicates ask `satisfies?` of the runtime's types.
    fn call(&mut self, core: CoreFnId, args: &[ExprId]) -> Option<Constant> {
        let Some(constants) = args
            .iter()
            .map(|arg| self.value(*arg))
            .collect::<Option<Vec<_>>>()
        else {
            return self.keyed_call(core, args);
        };
        if constants
            .iter()
            .any(|constant| matches!(constant, Constant::Collection(_)))
            && !self.facts.core_protocols_intact()
        {
            return None;
        }
        let types: Vec<Type> = constants
            .iter()
            .map(|constant| constant.ty(self.program))
            .collect();
        let call = core.operation().call(&types);
        if call.purity != Purity::Pure || !self.facts.trusted(core, call.calls) {
            return None;
        }
        evaluate(self.program, core, &constants)
    }

    /// In an `if` test of membership in a set, `(s x)`, `(contains? s x)` or
    /// `(get s x)`, make a local or `^:const` global `s` bound to a constant
    /// set literal of at most 8 keywords that literal (a copy, which the pool
    /// declares as the same value), so that emission switches on the tested
    /// value's spelling instead of looking it up (the emitter's
    /// `membership`, which states the conditions checked here).
    fn member_set(&mut self, test: ExprId) {
        if !self.facts.core_protocols_intact() || self.facts.implements_equiv() {
            return;
        }
        let ExprKind::Call { callee, args } = &self.program.exprs[test.0].kind else {
            return;
        };
        let operand = match (&self.program.exprs[callee.0].kind, &args[..]) {
            (ExprKind::CoreFn(core @ (CoreFnId::Contains | CoreFnId::Get)), [set, _])
                if !self.facts.replaced(*core) =>
            {
                *set
            }
            (_, [_]) => *callee,
            _ => return,
        };
        if !matches!(
            self.program.exprs[operand.0].kind,
            ExprKind::Local(_) | ExprKind::Global(_)
        ) {
            return;
        }
        let Some(set) = self.collection(operand) else {
            return;
        };
        let (ExprKind::Set(members) | ExprKind::LiteralSet(members)) =
            &self.program.exprs[set.0].kind
        else {
            return;
        };
        if members.len() > 8
            || !members
                .iter()
                .all(|member| matches!(self.program.exprs[member.0].kind, ExprKind::Keyword(_)))
        {
            return;
        }
        if let Some(kind) = self.copy(set) {
            self.program.exprs[operand.0].kind = kind;
            self.folded += 1;
        }
    }

    /// The kind that replaces a call of `core` that [`select`] folds: an item
    /// of a constant collection, a list of them, or an argument. The call's
    /// row must not be `effect` (`nth` is `throws`, which [`select`] rules
    /// out for these arguments), the operation must be trusted and core
    /// protocols intact.
    fn select(&mut self, id: ExprId, core: CoreFnId, args: &[ExprId]) -> Option<ExprKind> {
        if !matches!(
            core,
            CoreFnId::Get
                | CoreFnId::GetIn
                | CoreFnId::Nth
                | CoreFnId::First
                | CoreFnId::Second
                | CoreFnId::Last
                | CoreFnId::Peek
                | CoreFnId::Rest
                | CoreFnId::Next
                | CoreFnId::Seq
                | CoreFnId::NotEmpty
        ) || !self.facts.core_protocols_intact()
        {
            return None;
        }
        let values: Vec<Option<Constant>> = args.iter().map(|arg| self.value(*arg)).collect();
        // `get` and `get-in` on a map bound to a local whose keys are
        // constants.
        let keyed = match (core, &values[..]) {
            (CoreFnId::Get | CoreFnId::GetIn, [None, Some(_), ..]) => Some(self.keyed(args[0])?),
            _ => None,
        };
        let types: Vec<Type> = values
            .iter()
            .enumerate()
            .map(|(index, value)| match value {
                Some(value) => value.ty(self.program),
                None if index == 0 && keyed.is_some() => Type::MAP,
                None => Type::ANY,
            })
            .collect();
        let call = core.operation().call(&types);
        if call.purity == Purity::Effect || !self.facts.trusted(core, call.calls) {
            return None;
        }
        let folded = match keyed {
            Some(map) => {
                let (key, rest) = match core {
                    CoreFnId::Get => (values[1].clone()?, Vec::new()),
                    _ => match path_keys(self.program, values[1].as_ref()?)? {
                        [] => return self.write(id, Folded::Argument(0), args, None),
                        [key, rest @ ..] => (Constant::of(self.program, *key)?, rest.to_vec()),
                    },
                };
                self.keyed_walk(args[0], map, &key, &rest, (args.len() == 3).then_some(2))?
            }
            None => select(self.program, core, &values)?,
        };
        // A default argument that is not a constant is the last one.
        let default = (args.len() > 2 && values.last()?.is_none()).then(|| args.len() - 1);
        self.write(id, folded, args, default)
    }

    /// The kind that replaces a call of a keyword or a constant collection:
    /// `(:k c)` and `(:k c default)` are `get`, `(m k)` and `(s k)` of a map
    /// or set are its lookup with the same arguments, and `(v i)` of a vector
    /// is its item `i` (a number in range; otherwise the call throws, and
    /// stays). A vector ignores its arguments after the first, which must
    /// have no effect then. Lists are not callable. A lookup of a key that a
    /// map bound to a local lacks ([`Folder::keyed`]) is `nil` or the default
    /// too.
    fn invoke(&mut self, id: ExprId, callee: ExprId, args: &[ExprId]) -> Option<ExprKind> {
        // Keywords and collections call the runtime's `get` and `lookup`.
        if !self.facts.core_protocols_intact() || self.facts.replaced(CoreFnId::Get) {
            return None;
        }
        let values: Vec<Option<Constant>> = args.iter().map(|arg| self.value(*arg)).collect();
        let defaulted = match values.len() {
            1 => false,
            2 => true,
            _ => return None,
        };
        // A default that is not a constant runs before any other result.
        let default = (defaulted && values[1].is_none()).then_some(1);
        let found = |lookup: Lookup| match lookup {
            Lookup::Found(item) => Folded::Item(item),
            Lookup::Missing if defaulted => Folded::Argument(1),
            Lookup::Missing => Folded::Value(Constant::Nil),
        };
        let named = match self.program.exprs[callee.0].kind {
            ExprKind::Keyword(literal) => Some(Constant::Keyword(literal)),
            ExprKind::Symbol(literal) => Some(Constant::Symbol(literal)),
            _ => None,
        };
        let folded = if let Some(named) = named {
            match &values[0] {
                Some(value) => found(get(self.program, value, &named)?),
                None => {
                    let map = self.keyed(args[0])?;
                    self.keyed_walk(args[0], map, &named, &[], defaulted.then_some(1))?
                }
            }
        } else {
            let key = values[0].as_ref()?;
            let Some(collection) = self.collection(callee) else {
                let map = self.keyed(callee)?;
                let folded =
                    self.keyed_walk(callee, map, &key.clone(), &[], defaulted.then_some(1))?;
                return self.write(id, folded, args, default);
            };
            match shape(self.program, collection)?.0 {
                Shape::Vector => match lookup(self.program, collection, key)? {
                    Lookup::Found(item) if matches!(key, Constant::Number(_)) => {
                        if defaulted
                            && !effect_free(self.program, &self.program.exprs[args[1].0].kind)
                        {
                            return None;
                        }
                        // The ignored argument has no effect, so drop it.
                        return self.write(id, Folded::Item(item), args, None);
                    }
                    _ => return None,
                },
                Shape::List => return None,
                Shape::Map { .. } | Shape::Set { .. } => {
                    found(lookup(self.program, collection, key)?)
                }
            }
        };
        self.write(id, folded, args, default)
    }

    /// The map literal that `id` is, or that the local `id` is bound to
    /// (through other locals), when its keys are constants no two of which
    /// may be `=` ([`distinct`]) and its values need not be: the keys alone
    /// decide its count, `contains?` and a lookup of a key it lacks. Reading
    /// a local has no effect, and the map was built where the local was
    /// bound; a literal is evaluated where it is, so a fold that drops it
    /// needs its values to have no effect ([`Folder::dropped`]).
    fn keyed(&self, mut id: ExprId) -> Option<ExprId> {
        for _ in 0..self.program.locals.len() + 1 {
            match &self.program.exprs[id.0].kind {
                ExprKind::Local(local) => id = self.bound[local.0]?,
                ExprKind::Map(items) => {
                    let keys = || items.iter().step_by(2).copied();
                    return (keys().all(|key| Constant::item(self.program, key).is_some())
                        && distinct(self.program, keys()))
                    .then_some(id);
                }
                _ => return None,
            }
        }
        None
    }

    /// `get` of `key` in the map literal `map` that [`Folder::keyed`] found
    /// for `receiver`, then `get` of the `rest` of a `get-in` path in what
    /// that gives: `nil` or the default when the map lacks the key, and the
    /// key's value when that is a constant the pool holds
    /// ([`Folder::copyable`]), which the map was built with and every
    /// evaluation of the value reads. A literal receiver that `get` alone
    /// reads is also its value expression when its other values have no
    /// effect (Closure folds an object literal's property read likewise).
    fn keyed_walk(
        &mut self,
        receiver: ExprId,
        map: ExprId,
        key: &Constant,
        rest: &[ExprId],
        default: Option<usize>,
    ) -> Option<Folded> {
        match lookup(self.program, map, key)? {
            Lookup::Found(value) if self.copyable(value) => {
                if !self.dropped(receiver, map, None) {
                    return None;
                }
                let current = Constant::item(self.program, value)?;
                walk(self.program, current, Some(value), rest, default.is_some())
            }
            Lookup::Found(value) if receiver == map && rest.is_empty() => self
                .dropped(receiver, map, Some(value))
                .then_some(Folded::Expression(value)),
            // A value that reads a local whose value cannot change is that
            // read, where every local from the receiver to the map is read
            // only in the function that binds it (none is captured), so the
            // read stays in the scope the map was built in.
            Lookup::Found(value)
                if rest.is_empty()
                    && matches!(self.program.exprs[value.0].kind,
                        ExprKind::Local(local) if !self.assignable[local.0])
                    && self.uncaptured(receiver) =>
            {
                Some(Folded::Expression(value))
            }
            Lookup::Found(_) => None,
            _ if !self.dropped(receiver, map, None) => None,
            Lookup::Missing => match default {
                Some(index) => Some(Folded::Argument(index)),
                None => walk(self.program, Constant::Nil, None, rest, false),
            },
        }
    }

    /// Whether no closure, type or `js*` hole reads the locals from
    /// `receiver` along the chain of locals [`Folder::keyed`] followed.
    fn uncaptured(&self, mut id: ExprId) -> bool {
        for _ in 0..self.program.locals.len() + 1 {
            let ExprKind::Local(local) = self.program.exprs[id.0].kind else {
                return true;
            };
            match self.bound[local.0] {
                Some(init) if !self.pinned[local.0] => id = init,
                _ => return false,
            }
        }
        false
    }

    /// Whether a fold may drop the evaluation of `receiver`, which
    /// [`Folder::keyed`] resolved to the map literal `map`, except for its
    /// value `kept`: a local read has no effect, and the literal itself none
    /// when its other values have none.
    fn dropped(&self, receiver: ExprId, map: ExprId, kept: Option<ExprId>) -> bool {
        if receiver != map {
            return true;
        }
        let ExprKind::Map(items) = &self.program.exprs[map.0].kind else {
            return false;
        };
        items
            .iter()
            .filter(|item| Some(**item) != kept)
            .all(|item| effect_free(self.program, &self.program.exprs[item.0].kind))
    }

    /// A `pure` call on a map bound to a local whose keys are constants
    /// ([`Folder::keyed`]), whatever its values, and constants: [`evaluate`]
    /// answers `count`, `empty?`, `contains?` and the type predicates from
    /// the literal's shape and keys alone (`(seq? m)` of a map destructuring).
    fn keyed_call(&mut self, core: CoreFnId, args: &[ExprId]) -> Option<Constant> {
        if !self.facts.core_protocols_intact() {
            return None;
        }
        let (receiver, rest) = args.split_first()?;
        let map = self.keyed(*receiver)?;
        if !self.dropped(*receiver, map, None) {
            return None;
        }
        let mut constants = vec![Constant::Collection(map)];
        for arg in rest {
            constants.push(self.value(*arg)?);
        }
        let types: Vec<Type> = constants
            .iter()
            .map(|constant| constant.ty(self.program))
            .collect();
        let call = core.operation().call(&types);
        if call.purity != Purity::Pure || !self.facts.trusted(core, call.calls) {
            return None;
        }
        evaluate(self.program, core, &constants)
    }

    /// The kind that replaces the call `id` of `args` with `folded`. Only the
    /// argument at `default` may be other than a constant, and has an effect
    /// only then: when the call's result is another value, it runs first,
    /// `(do default value)`. An item of a constant collection is copied
    /// ([`Folder::copy`]).
    fn write(
        &mut self,
        id: ExprId,
        folded: Folded,
        args: &[ExprId],
        default: Option<usize>,
    ) -> Option<ExprKind> {
        match folded {
            Folded::Argument(index) => return self.adopt(id, args[index]),
            // The default runs after the value, so it must have no effect.
            Folded::Expression(value) => {
                return match default {
                    Some(index)
                        if !effect_free(self.program, &self.program.exprs[args[index].0].kind) =>
                    {
                        None
                    }
                    _ => self.adopt(id, value),
                };
            }
            _ => (),
        }
        let kind = match folded {
            Folded::Value(value) => value.into_kind(self.program),
            Folded::Item(item) => self.copy(item)?,
            Folded::List(items) => {
                if !items.iter().all(|item| self.copyable(*item)) {
                    return None;
                }
                let items = items
                    .into_iter()
                    .map(|item| {
                        let kind = self.copy(item).expect("a copyable item");
                        self.push(item, kind)
                    })
                    .collect();
                ExprKind::List(items)
            }
            Folded::Argument(_) | Folded::Expression(_) => unreachable!("written above"),
        };
        match default {
            Some(index) if !effect_free(self.program, &self.program.exprs[args[index].0].kind) => {
                let value = self.push(id, kind);
                Some(ExprKind::Do(vec![args[index], value]))
            }
            _ => Some(kind),
        }
    }

    /// The constant `id` evaluates to: a scalar or symbol literal, or a
    /// constant collection ([`Folder::collection`]).
    fn value(&mut self, id: ExprId) -> Option<Constant> {
        Constant::of(self.program, id).or_else(|| self.collection(id).map(Constant::Collection))
    }

    /// The constant collection literal that `id` evaluates to: a literal the
    /// constant pool may hold, or one that a `let` binds a local to, or a
    /// `^:const` global read after its definition, through any chain of
    /// locals bound to such reads.
    fn collection(&mut self, mut id: ExprId) -> Option<ExprId> {
        // Locals bound to locals form chains as long as the nesting of `let`s.
        for _ in 0..self.program.locals.len() + 2 {
            match self.program.exprs[id.0].kind {
                ExprKind::Local(local) => id = self.bound[local.0]?,
                ExprKind::Global(global) => {
                    // Read in a later top-level form, after the definition.
                    let (at, literal) = self.collections[global.0]?;
                    let read = self.position.get(id.0).copied().unwrap_or(NOWHERE);
                    if read == NOWHERE || read <= at {
                        return None;
                    }
                    id = literal;
                }
                _ => {
                    shape(self.program, id)?;
                    return self.constant_class(id).then_some(id);
                }
            }
        }
        None
    }

    /// Whether the constant pool may hold `id` (the emitter's
    /// `constant_nodes`): a constant that cannot throw or change when built.
    fn constant_class(&mut self, id: ExprId) -> bool {
        self.classes
            .resize(self.program.exprs.len(), ConstantClass::Unknown);
        constant_nodes(self.program, &self.replaced, &mut self.classes, id).is_some()
    }

    /// Whether the item `id` of a constant collection may be copied to
    /// another place with the same value and identity: a scalar or symbol,
    /// or a collection of at most [`COPY_LIMIT`] nodes that no evaluation
    /// builds, so that the pool declares the copy as the same entry as the
    /// item (hash-consing on structure). A literal marked built (see
    /// [`built_literals`]) would be a fresh value at each place.
    fn copyable(&mut self, id: ExprId) -> bool {
        if Constant::of(self.program, id).is_some() {
            return true;
        }
        if !self.constant_class(id) {
            return false;
        }
        let mut pending = vec![id];
        let mut nodes = 0;
        while let Some(node) = pending.pop() {
            nodes += 1;
            if nodes > COPY_LIMIT || self.built.get(node.0).copied().unwrap_or(false) {
                return false;
            }
            pending.extend(children(self.program, node));
        }
        true
    }

    /// The kind of a copy of the constant item `id`, whose nested items are
    /// new expressions, when it is [`Folder::copyable`].
    fn copy(&mut self, id: ExprId) -> Option<ExprKind> {
        if !self.copyable(id) {
            return None;
        }
        let mut kind = self.program.exprs[id.0].kind.clone();
        match &mut kind {
            ExprKind::Vector(items)
            | ExprKind::List(items)
            | ExprKind::Map(items)
            | ExprKind::Set(items)
            | ExprKind::LiteralMap(items)
            | ExprKind::LiteralSet(items)
            | ExprKind::ReaderLiteral { items, .. } => {
                for item in items.iter_mut() {
                    let copy = self.copy(*item).expect("an item of a copyable constant");
                    *item = self.push(*item, copy);
                }
            }
            ExprKind::WithMeta { value, metadata } => {
                for item in [value, metadata] {
                    let copy = self.copy(*item).expect("an item of a copyable constant");
                    *item = self.push(*item, copy);
                }
            }
            _ => {}
        }
        Some(kind)
    }

    /// A new expression of `kind` where `like` is, already visited: the
    /// per-expression state grows with the arena.
    fn push(&mut self, like: ExprId, kind: ExprKind) -> ExprId {
        let mut expr = self.program.exprs[like.0].clone();
        expr.kind = kind;
        self.program.exprs.push(expr);
        let id = ExprId(self.program.exprs.len() - 1);
        let count = self.program.exprs.len();
        let position = self.position.get(like.0).copied().unwrap_or(NOWHERE);
        self.position.resize(count, position);
        self.kept.resize(count, false);
        self.fenced.resize(count, false);
        self.built.resize(count, false);
        self.parents.resize(count, 1);
        self.discarded.resize(count, false);
        self.visited.resize(count, true);
        id
    }

    /// Drop the bindings that nothing reads any more and whose initializers
    /// have no effect, which folding a lookup leaves behind: the local a
    /// destructuring binds its value to, and the locals bound to that one.
    /// A `let` left without bindings is its body. Repeats (at most
    /// [`SWEEPS`] times) while a binding goes, since an initializer may be
    /// the last read of an earlier binding.
    fn sweep(&mut self) {
        for _ in 0..SWEEPS {
            let mut reached = vec![false; self.program.exprs.len()];
            let reads = reachable_reads(self.program, None, &mut reached);
            let mut dropped = 0;
            for (index, reached) in reached.iter().enumerate() {
                let ExprKind::Let { bindings, body } = &self.program.exprs[index].kind else {
                    continue;
                };
                if !reached {
                    continue;
                }
                let body = *body;
                let kept: Vec<(LocalId, ExprId)> = bindings
                    .iter()
                    .filter(|(local, init)| {
                        reads[local.0] > 0
                            || self.pinned[local.0]
                            || !effect_free(self.program, &self.program.exprs[init.0].kind)
                    })
                    .copied()
                    .collect();
                if kept.len() == bindings.len() {
                    continue;
                }
                dropped += bindings.len() - kept.len();
                let id = ExprId(index);
                if kept.is_empty()
                    && !self.kept[index]
                    && !scoped(self.program, body)
                    && let Some(kind) = self.adopt(id, body)
                {
                    self.program.exprs[index].kind = kind;
                    continue;
                }
                self.program.exprs[index].kind = ExprKind::Let {
                    bindings: kept,
                    body,
                };
            }
            self.folded += dropped;
            if dropped == 0 {
                break;
            }
        }
    }
}

/// The most nodes of a constant collection item that a fold copies to the
/// call's place (rule 4 of docs/dos-and-donts.md). The pool declares the
/// copy as the entry the item already is, so emitted code does not grow.
const COPY_LIMIT: usize = 256;

/// The most passes [`Folder::sweep`] makes over the program.
const SWEEPS: usize = 4;

/// Whether `id` depends on the function scope emission gives it: outside
/// nested functions it reads `arguments`, holds `js*` raw text (which may read
/// `this` or `arguments`) or is a definition with `:test` metadata. A `let` or
/// `do` in value position is emitted as a function call, so such contents
/// keep the form around them (the emitter's `sequence_keeps_scope` asks the
/// same).
pub(crate) fn scoped(program: &Program, id: ExprId) -> bool {
    let mut pending = vec![id];
    while let Some(id) = pending.pop() {
        match program.exprs[id.0].kind {
            ExprKind::JsArguments
            | ExprKind::CopyArguments(_)
            | ExprKind::JsTemplate { .. }
            | ExprKind::Def { test: Some(_), .. } => return true,
            ExprKind::Fn { .. } => (),
            _ => pending.extend(children(program, id)),
        }
    }
    false
}

/// By expression: a key of a map literal or a member of a set literal. These
/// never become constants here: the emitter builds a map whose keys are all
/// constants from a key shape and a set whose members are all scalars without
/// the duplicate check of members computed at run time, which assume the
/// distinct keys analysis checks in source literals.
pub(crate) fn fenced_positions(program: &Program) -> Vec<bool> {
    let mut fenced = vec![false; program.exprs.len()];
    for expr in &program.exprs {
        match &expr.kind {
            ExprKind::Map(items) | ExprKind::LiteralMap(items) => {
                for key in items.iter().step_by(2) {
                    fenced[key.0] = true;
                }
            }
            ExprKind::Set(items) | ExprKind::LiteralSet(items) => {
                for member in items {
                    fenced[member.0] = true;
                }
            }
            _ => (),
        }
    }
    fenced
}

/// By expression: the literals that the program builds at each evaluation
/// (collection literals and `with-meta` with an item that is not a
/// constant). Folding and inlining carry the mark where they move or copy a
/// literal, and [`keep_built`] keeps every marked literal built.
pub fn built_literals(program: &Program) -> Vec<bool> {
    program
        .exprs
        .iter()
        .map(|expr| pooled_kind(&expr.kind) && !constant_like(program, &expr.kind))
        .collect()
}

/// Keep each literal [`built_literals`] marked built at its site where
/// folding or inlining made all its items constants, which the emitter would
/// otherwise declare once in the constant pool, so that evaluations that
/// built distinct values shared one: one item (a map's first value, so that
/// its constant keys keep their shape) becomes a one-form `do`, which
/// emission writes as the item itself but does not pool.
pub fn keep_built(program: &mut Program, built: &[bool]) {
    for index in 0..program.exprs.len() {
        if !built.get(index).copied().unwrap_or(false)
            || !constant_like(program, &program.exprs[index].kind)
        {
            continue;
        }
        // A map keeps its constant keys, and so its key shape, and wraps its
        // first value.
        let slot = match &program.exprs[index].kind {
            ExprKind::Map(items) | ExprKind::LiteralMap(items) => (items.len() > 1).then_some(1),
            ExprKind::Vector(items)
            | ExprKind::List(items)
            | ExprKind::Set(items)
            | ExprKind::LiteralSet(items) => (!items.is_empty()).then_some(0),
            ExprKind::WithMeta { .. } => Some(0),
            _ => None,
        };
        let Some(slot) = slot else {
            continue;
        };
        let item = match &program.exprs[index].kind {
            ExprKind::WithMeta { value, .. } => *value,
            ExprKind::Vector(items)
            | ExprKind::List(items)
            | ExprKind::Map(items)
            | ExprKind::Set(items)
            | ExprKind::LiteralMap(items)
            | ExprKind::LiteralSet(items) => items[slot],
            _ => unreachable!("a literal"),
        };
        let mut expr = program.exprs[item.0].clone();
        expr.kind = ExprKind::Do(vec![item]);
        program.exprs.push(expr);
        let wrapped = ExprId(program.exprs.len() - 1);
        match &mut program.exprs[index].kind {
            ExprKind::Vector(items)
            | ExprKind::List(items)
            | ExprKind::Map(items)
            | ExprKind::Set(items)
            | ExprKind::LiteralMap(items)
            | ExprKind::LiteralSet(items) => items[slot] = wrapped,
            ExprKind::WithMeta { value, .. } => *value = wrapped,
            _ => unreachable!("a literal"),
        }
    }
}

/// Whether a literal of this kind is one the emitter may pool.
fn pooled_kind(kind: &ExprKind) -> bool {
    matches!(
        kind,
        ExprKind::Vector(_)
            | ExprKind::List(_)
            | ExprKind::Map(_)
            | ExprKind::Set(_)
            | ExprKind::LiteralMap(_)
            | ExprKind::LiteralSet(_)
            | ExprKind::WithMeta { .. }
    )
}

/// Whether an expression of this kind may be a constant the emitter pools:
/// scalars, quoted symbols, and collection literals of such expressions. A
/// superset of the emitter's own classification.
pub(crate) fn constant_like(program: &Program, kind: &ExprKind) -> bool {
    match kind {
        ExprKind::Nil
        | ExprKind::Bool(_)
        | ExprKind::Number(_)
        | ExprKind::String(_)
        | ExprKind::Keyword(_)
        | ExprKind::Symbol(_) => true,
        ExprKind::Vector(items)
        | ExprKind::List(items)
        | ExprKind::Map(items)
        | ExprKind::Set(items)
        | ExprKind::LiteralMap(items)
        | ExprKind::LiteralSet(items)
        | ExprKind::ReaderLiteral { items, .. } => items
            .iter()
            .all(|item| constant_like(program, &program.exprs[item.0].kind)),
        ExprKind::WithMeta { value, metadata } => [value, metadata]
            .iter()
            .all(|item| constant_like(program, &program.exprs[item.0].kind)),
        _ => false,
    }
}

/// Whether an `if` on an expression of this kind takes its first branch,
/// when evaluating it has no effect and the answer is known: the truth of a
/// constant, and `true` for quoted symbols, `fn` and collection literals
/// without effects, which are never `nil` or `false`.
fn literal_truth(program: &Program, kind: &ExprKind) -> Option<bool> {
    if let Some(constant) = Constant::of_kind(program, kind) {
        return Some(constant.truthy());
    }
    match kind {
        ExprKind::Symbol(_)
        | ExprKind::Fn { .. }
        | ExprKind::Vector(_)
        | ExprKind::List(_)
        | ExprKind::Map(_)
        | ExprKind::Set(_)
        | ExprKind::LiteralMap(_)
        | ExprKind::LiteralSet(_) => effect_free(program, kind).then_some(true),
        _ => None,
    }
}

/// Whether evaluating an expression of this kind has no effect and cannot
/// throw: literals, reads of locals, globals and core values, `fn`, and
/// vector and list literals of such expressions, and map and set literals
/// whose keys and members are scalar literals (analysis rejects equal literal
/// keys, which would throw) and whose values are such expressions.
pub(crate) fn effect_free(program: &Program, kind: &ExprKind) -> bool {
    let scalar = |id: &ExprId| {
        matches!(
            program.exprs[id.0].kind,
            ExprKind::Nil
                | ExprKind::Bool(_)
                | ExprKind::Number(_)
                | ExprKind::String(_)
                | ExprKind::Keyword(_)
                | ExprKind::Symbol(_)
        )
    };
    let free = |id: &ExprId| effect_free(program, &program.exprs[id.0].kind);
    match kind {
        ExprKind::Nil
        | ExprKind::Bool(_)
        | ExprKind::Number(_)
        | ExprKind::String(_)
        | ExprKind::Keyword(_)
        | ExprKind::Symbol(_)
        | ExprKind::Local(_)
        | ExprKind::Global(_)
        | ExprKind::CoreFn(_)
        | ExprKind::Fn { .. } => true,
        ExprKind::Vector(items) | ExprKind::List(items) => items.iter().all(free),
        ExprKind::Map(items) | ExprKind::LiteralMap(items) => items
            .as_chunks::<2>()
            .0
            .iter()
            .all(|[key, value]| scalar(key) && free(value)),
        ExprKind::Set(items) | ExprKind::LiteralSet(items) => items.iter().all(scalar),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Constant, Folded, Lookup, count, flatten, js_number, lookup, same_key, select, shape,
    };
    use crate::analyzer::{CoreFnId, ExprId, ExprKind, Program};

    /// The program of `source` and its top-level forms.
    fn forms(source: &str) -> (Program, Vec<ExprId>) {
        let program = crate::analyze(source, &crate::parse(source).unwrap()).unwrap();
        let mut top = Vec::new();
        flatten(&program, program.root, &mut top);
        (program, top)
    }

    /// The scalar a form evaluates to, for a lookup key.
    fn key(program: &Program, id: ExprId) -> Constant {
        Constant::of(program, id).unwrap_or(Constant::Collection(id))
    }

    #[test]
    fn keys_compare_as_the_runtime_compares_them() {
        // Each pair: a lookup key, the key of a map, and what `=` gives.
        let (program, top) = forms(
            "[0 -0.0] [##NaN ##NaN] [1 1.0] [\"a\" :a] [:a :a] [:n/a :n/a] [:n/a :a] ['a :a] \
             ['a 'a] [nil nil] [nil false] [true 1] [:a [:a]] [[1] [1]]",
        );
        let expected = [
            None,
            None,
            Some(true),
            Some(false),
            Some(true),
            Some(true),
            Some(false),
            Some(false),
            Some(true),
            Some(true),
            Some(false),
            Some(false),
            Some(false),
            None,
        ];
        for (form, expected) in top.iter().zip(expected) {
            let (_, items) = shape(&program, *form).unwrap();
            let lookup_key = key(&program, items[0]);
            assert_eq!(
                same_key(&program, &lookup_key, items[1]),
                expected,
                "{:?}",
                program.exprs[form.0].kind
            );
        }
    }

    #[test]
    fn lookups_and_counts_follow_the_builders() {
        let (program, top) = forms(
            "{:a 1 :b 2} {1 :one 1.0 :onef} {[1] :v '(1) :w} {0 :zero 1 :one} #{1 2} '#{1 1.0} [5 6]",
        );
        let value = |id: ExprId| program.exprs[id.0].kind.clone();
        let b = Constant::Keyword(
            match program.exprs[shape(&program, top[0]).unwrap().1[2].0].kind {
                ExprKind::Keyword(literal) => literal,
                _ => unreachable!(),
            },
        );
        let Some(Lookup::Found(two)) = lookup(&program, top[0], &b) else {
            panic!("`:b` is found");
        };
        assert_eq!(value(two), ExprKind::Number(2.0));
        // A positional map finds the first key that is `=`.
        let Some(Lookup::Found(one)) = lookup(&program, top[1], &Constant::Number(1.0)) else {
            panic!("`1` is found");
        };
        assert!(matches!(value(one), ExprKind::Keyword(_)));
        assert_eq!(count(&program, top[1]), Some(2));
        // Collection keys may merge: a scalar is missing, a collection
        // undecided, and the count unknown.
        assert_eq!(lookup(&program, top[2], &b), Some(Lookup::Missing));
        assert_eq!(
            lookup(&program, top[2], &Constant::Collection(top[6])),
            None
        );
        assert_eq!(count(&program, top[2]), None);
        assert_eq!(count(&program, top[3]), Some(2));
        assert_eq!(count(&program, top[4]), Some(2));
        assert_eq!(count(&program, top[5]), Some(2));
        // Vectors truncate number indexes; other keys miss.
        let vector = Constant::Collection(top[6]);
        let item = |index: f64| lookup(&program, top[6], &Constant::Number(index));
        assert!(matches!(item(1.9), Some(Lookup::Found(_))));
        assert_eq!(item(-0.5), Some(Lookup::Missing));
        assert_eq!(item(f64::NAN), Some(Lookup::Missing));
        assert_eq!(lookup(&program, top[6], &b), Some(Lookup::Missing));
        // `nth` out of range throws without a default, so it does not fold.
        let nth = |index: f64, default: bool| {
            let mut args = vec![Some(vector.clone()), Some(Constant::Number(index))];
            if default {
                args.push(None);
            }
            select(&program, CoreFnId::Nth, &args)
        };
        assert!(matches!(nth(0.0, false), Some(Folded::Item(_))));
        assert_eq!(nth(2.0, false), None);
        assert_eq!(nth(2.0, true), Some(Folded::Argument(2)));
        assert_eq!(
            select(
                &program,
                CoreFnId::Nth,
                &[Some(vector.clone()), Some(Constant::Nil)]
            ),
            None
        );
    }

    #[test]
    fn numbers_print_as_javascript_prints_them() {
        for (value, text) in [
            (0.0, "0"),
            (-0.0, "0"),
            (1.0, "1"),
            (-1.5, "-1.5"),
            (100.0, "100"),
            (0.1, "0.1"),
            (0.1 + 0.2, "0.30000000000000004"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (123456789012345680000.0, "123456789012345680000"),
            (1.5e300, "1.5e+300"),
            (0.000001, "0.000001"),
            (1e-7, "1e-7"),
            (1.25e-7, "1.25e-7"),
            (9007199254740992.0, "9007199254740992"),
            (f64::MAX, "1.7976931348623157e+308"),
            (5e-324, "5e-324"),
            (f64::NAN, "NaN"),
            (f64::INFINITY, "Infinity"),
            (f64::NEG_INFINITY, "-Infinity"),
        ] {
            assert_eq!(js_number(value), text, "{value:e}");
        }
    }
}
