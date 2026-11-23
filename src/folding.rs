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
//!   it in the same way. A global without `^:const` that is so defined and
//!   never assigned (no `set!`, `binding`, `with-redefs` or second definition
//!   anywhere in the program) is folded the same way when its value is a
//!   scalar or symbol literal, a read of such a global, or a call without
//!   arguments of a function whose one method takes no parameters and returns
//!   one, and so is such a call in a later top-level form (light-cljs-984,
//!   [`constant_globals`]): `(defn ^boolean on? [] enabled?)` and
//!   `(def alias? enabled?)` fold where `enabled?` does.
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
//! become constants (fenced positions): emission builds maps with
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
use crate::analyzer::{
    CoreFnId, Expr, ExprId, ExprKind, GlobalId, LocalId, NamedLiteral, NamedLiteralId, Program,
    StringId,
};
use crate::compiler_data::CompilerLiteralKind;
use crate::core_operations::{Purity, Type};
use crate::emitter::ConstantClass;
use crate::inference::ProgramFacts;
use crate::program_fragment::visit_children;
use crate::scheduler::{self, Slots};
use std::collections::{HashMap, HashSet};

/// Minimum expression count before fanning out folding or lookup scans across
/// worker threads on `scheduler::map`.
const PARALLEL_EXPR_THRESHOLD: usize = 2048;

/// Minimum top-level item count before slicing top-level traversals.
const PARALLEL_FORM_THRESHOLD: usize = 16;

/// Target slices per CPU slot for LPT balancing on `scheduler::map`.
const SLICES_PER_SLOT: usize = 4;

/// Compact dense bitset (1 bit per ID, ~62 KiB for 500k expressions) so
/// parallel workers keep per-slice scratch tiny.
#[derive(Clone)]
pub(crate) struct BitSet {
    words: Vec<u64>,
}

impl BitSet {
    pub(crate) fn new(len: usize) -> Self {
        Self {
            words: vec![0u64; len.div_ceil(64)],
        }
    }

    pub(crate) fn resize(&mut self, len: usize) {
        let words = len.div_ceil(64);
        if words > self.words.len() {
            self.words.resize(words, 0);
        }
    }

    #[inline]
    pub(crate) fn get(&self, index: usize) -> bool {
        let word = index >> 6;
        word < self.words.len() && ((self.words[word] >> (index & 63)) & 1) != 0
    }

    /// Set bit `index` and return its previous value.
    #[inline]
    pub(crate) fn set(&mut self, index: usize) -> bool {
        let word = index >> 6;
        let mask = 1u64 << (index & 63);
        let prev = (self.words[word] & mask) != 0;
        self.words[word] |= mask;
        prev
    }

    #[inline]
    pub(crate) fn clear(&mut self, index: usize) {
        let word = index >> 6;
        if word < self.words.len() {
            self.words[word] &= !(1u64 << (index & 63));
        }
    }

    pub(crate) fn intersects(&self, other: &Self) -> bool {
        self.words
            .iter()
            .zip(&other.words)
            .any(|(&a, &b)| (a & b) != 0)
    }

    pub(crate) fn or_assign(&mut self, other: &Self) {
        if other.words.len() > self.words.len() {
            self.words.resize(other.words.len(), 0);
        }
        for (dst, &src) in self.words.iter_mut().zip(&other.words) {
            *dst |= src;
        }
    }
}

/// Partition `weights` into up to `target_slices` contiguous, weight-balanced
/// non-empty ranges covering `0..weights.len()`.
pub(crate) fn partition_slices(
    weights: &[u64],
    target_slices: usize,
) -> Vec<std::ops::Range<usize>> {
    let n = weights.len();
    if n == 0 {
        return Vec::new();
    }
    let slices = target_slices.clamp(1, n);
    if slices == 1 {
        return std::iter::once(0..n).collect();
    }
    let mut remaining_weight: u64 = weights.iter().sum();
    let mut ranges = Vec::with_capacity(slices);
    let mut start = 0usize;
    for remaining_slices in (1..=slices).rev() {
        if start >= n {
            break;
        }
        if remaining_slices == 1 {
            ranges.push(start..n);
            break;
        }
        let target = remaining_weight / remaining_slices as u64;
        let limit = n - (remaining_slices - 1);
        let mut acc = 0u64;
        let mut end = start;
        while end < limit {
            let w = weights[end];
            let next = acc + w;
            if end > start && next > target && next - target > target.saturating_sub(acc) {
                break;
            }
            acc = next;
            end += 1;
            if acc >= target && acc > 0 {
                break;
            }
        }
        if end == start {
            acc = weights[end];
            end += 1;
        }
        ranges.push(start..end);
        remaining_weight = remaining_weight.saturating_sub(acc);
        start = end;
    }
    ranges
}

/// Read-only view of expressions and strings: implemented directly by
/// [`Program`] and by the sparse-delta overlay [`FolderView`] so read-only
/// folding observes its own local rewrites without mutating [`Program`].
trait ExprSource {
    fn kind(&self, id: ExprId) -> &ExprKind;
    fn string(&self, id: StringId) -> &[u16];
    fn named_literal(&self, id: NamedLiteralId) -> &NamedLiteral;
}

impl ExprSource for Program {
    #[inline]
    fn kind(&self, id: ExprId) -> &ExprKind {
        &self.exprs[id.0].kind
    }

    #[inline]
    fn string(&self, id: StringId) -> &[u16] {
        &self.strings[id.0]
    }

    #[inline]
    fn named_literal(&self, id: NamedLiteralId) -> &NamedLiteral {
        &self.named_literals[id.0]
    }
}

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
        Self::of_view(program, id)
    }

    fn of_view<S: ExprSource>(src: &S, id: ExprId) -> Option<Self> {
        Self::of_kind_view(src, src.kind(id))
    }

    fn of_kind_view<S: ExprSource>(src: &S, kind: &ExprKind) -> Option<Self> {
        Some(match kind {
            ExprKind::Nil => Self::Nil,
            ExprKind::Bool(value) => Self::Bool(*value),
            ExprKind::Number(value) => Self::Number(*value),
            ExprKind::String(string) => Self::String(src.string(*string).to_vec()),
            ExprKind::Keyword(literal) => Self::Keyword(*literal),
            ExprKind::Symbol(literal) => Self::Symbol(*literal),
            _ => return None,
        })
    }

    /// The value of the item `id` of a constant collection: a scalar or
    /// symbol, or a nested collection, which is itself constant.
    fn item_view<S: ExprSource>(src: &S, id: ExprId) -> Option<Self> {
        Self::of_view(src, id).or_else(|| shape_view(src, id).map(|_| Self::Collection(id)))
    }

    /// Its class in the core operation table's lattice.
    pub fn ty(&self, program: &Program) -> Type {
        self.ty_view(program)
    }

    fn ty_view<S: ExprSource>(&self, src: &S) -> Type {
        match self {
            Self::Nil => Type::NIL,
            Self::Bool(_) => Type::BOOLEAN,
            Self::Number(_) => Type::NUMBER,
            Self::String(_) => Type::STRING,
            Self::Keyword(_) => Type::KEYWORD,
            Self::Symbol(_) => Type::SYMBOL,
            Self::Collection(id) => match shape_view(src, *id) {
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

    /// Whether JavaScript counts it as true exactly when Clojure does: not
    /// for `0`, `-0`, `NaN` and the empty string, which JavaScript counts
    /// as false.
    pub fn truth_is_js(&self) -> bool {
        match self {
            Self::Number(value) => *value != 0.0 && !value.is_nan(),
            Self::String(units) => !units.is_empty(),
            _ => true,
        }
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
pub(crate) fn shape(program: &Program, id: ExprId) -> Option<(Shape, &[ExprId])> {
    shape_view(program, id)
}

fn shape_view<S: ExprSource>(src: &S, mut id: ExprId) -> Option<(Shape, &[ExprId])> {
    loop {
        return Some(match src.kind(id) {
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
#[cfg(test)]
fn same_key(program: &Program, key: &Constant, item: ExprId) -> Option<bool> {
    same_key_view(program, key, item)
}

fn same_key_view<S: ExprSource>(src: &S, key: &Constant, item: ExprId) -> Option<bool> {
    match Constant::of_view(src, item) {
        Some(item) => scalar_equal_view(src, key, &item),
        // A scalar is never `=` to a collection or `#uuid` while core
        // protocols are intact.
        None if !matches!(key, Constant::Collection(_)) => Some(false),
        None => None,
    }
}

/// `=` on two scalars or symbols, as [`same_key_view`] decides it.
fn scalar_equal_view<S: ExprSource>(src: &S, a: &Constant, b: &Constant) -> Option<bool> {
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
                let name = src.named_literal(literal);
                (
                    name.namespace.map(|namespace| src.string(namespace)),
                    src.string(name.name),
                )
            };
            if parts(*a) == parts(*b) {
                true
            } else if spelling_view(src, *a) == spelling_view(src, *b) {
                return None;
            } else {
                false
            }
        }
        _ => false,
    })
}

/// How a membership test compares a value with the members of a set literal
/// (the emitter's `membership`, [`Folder::member_set`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Members {
    /// Keywords, which a value is `=` to exactly when it is a keyword of the
    /// same spelling.
    Keywords,
    /// Strings and numbers, which a value is `=` to exactly when it is `===`
    /// to them: the array set's `entryIndex` compares a string or number key
    /// with `===` and any other key with `equalPair`, which finds no other
    /// value `=` to a string or number.
    Primitives,
}

/// The [`Members`] of a set literal whose membership tests the emitter writes
/// as comparisons: at most 8 members (an array set, which compares the value
/// with each member without hashing it), all keywords or all strings and
/// numbers. The caller checks that core protocols are intact and that no
/// program type implements `IEquiv`.
pub(crate) fn members(program: &Program, members: &[ExprId]) -> Option<Members> {
    members_view(program, members)
}

fn members_view<S: ExprSource>(src: &S, members: &[ExprId]) -> Option<Members> {
    if members.len() > 8 {
        return None;
    }
    if members
        .iter()
        .all(|member| matches!(src.kind(*member), ExprKind::Keyword(_)))
    {
        Some(Members::Keywords)
    } else if members
        .iter()
        .all(|member| matches!(src.kind(*member), ExprKind::Number(_) | ExprKind::String(_)))
    {
        Some(Members::Primitives)
    } else {
        None
    }
}

/// The spelling `ns/name` (or `name`) of a keyword or symbol literal, which
/// the runtime's `=` compares.
pub(crate) fn spelling(program: &Program, literal: NamedLiteralId) -> Vec<u16> {
    spelling_view(program, literal)
}

fn spelling_view<S: ExprSource>(src: &S, literal: NamedLiteralId) -> Vec<u16> {
    let name = src.named_literal(literal);
    let mut units = Vec::new();
    if let Some(namespace) = name.namespace {
        units.extend_from_slice(src.string(namespace));
        units.push(u16::from(b'/'));
    }
    units.extend_from_slice(src.string(name.name));
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
#[cfg(test)]
fn lookup(program: &Program, id: ExprId, key: &Constant) -> Option<Lookup> {
    lookup_view(program, id, key)
}

fn lookup_view<S: ExprSource>(src: &S, id: ExprId, key: &Constant) -> Option<Lookup> {
    let (shape, items) = shape_view(src, id)?;
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
        if same_key_view(src, key, *item)? {
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
#[cfg(test)]
fn count(program: &Program, id: ExprId) -> Option<usize> {
    count_view(program, id)
}

fn count_view<S: ExprSource>(src: &S, id: ExprId) -> Option<usize> {
    let (shape, items) = shape_view(src, id)?;
    match shape {
        Shape::Vector | Shape::List | Shape::Set { positional: true } => Some(items.len()),
        Shape::Map { positional: true } => Some(items.len() / 2),
        Shape::Map { positional: false } => {
            distinct_view(src, items.iter().step_by(2).copied()).then_some(items.len() / 2)
        }
        Shape::Set { positional: false } => {
            distinct_view(src, items.iter().copied()).then_some(items.len())
        }
    }
}

/// Whether no two of `keys` may be `=`: scalars and symbols that are
/// pairwise provably unequal ([`scalar_equal_view`]) and at most one collection.
/// Linear, with a set of canonical forms: zeros of both signs share one, and
/// keywords and symbols are their spellings, so `0` and `-0.0` or keywords
/// spelled alike count as possibly equal, and `NaN` makes no key distinct.
fn distinct_view<S: ExprSource>(src: &S, keys: impl Iterator<Item = ExprId>) -> bool {
    #[derive(Hash, PartialEq, Eq)]
    enum Canonical {
        Nil,
        Bool(bool),
        Number(u64),
        String(Vec<u16>),
        Named(bool, Vec<u16>),
    }
    let mut seen = HashSet::new();
    let mut collections = 0;
    for key in keys {
        let canonical = match Constant::of_view(src, key) {
            Some(Constant::Nil) => Canonical::Nil,
            Some(Constant::Bool(value)) => Canonical::Bool(value),
            Some(Constant::Number(value)) if value.is_nan() => return false,
            Some(Constant::Number(0.0)) => Canonical::Number(0),
            Some(Constant::Number(value)) => Canonical::Number(value.to_bits()),
            Some(Constant::String(units)) => Canonical::String(units),
            Some(Constant::Keyword(literal)) => {
                Canonical::Named(false, spelling_view(src, literal))
            }
            Some(Constant::Symbol(literal)) => Canonical::Named(true, spelling_view(src, literal)),
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
    select_view(program, core, args)
}

fn select_view<S: ExprSource>(
    src: &S,
    core: CoreFnId,
    args: &[Option<Constant>],
) -> Option<Folded> {
    let found = |lookup: Lookup, default: Option<usize>| match (lookup, default) {
        (Lookup::Found(item), _) => Folded::Item(item),
        (Lookup::Missing, Some(index)) => Folded::Argument(index),
        (Lookup::Missing, None) => Folded::Value(Constant::Nil),
    };
    match (core, args) {
        (CoreFnId::Get, [Some(collection), Some(key), rest @ ..]) if rest.len() <= 1 => {
            let default = (!rest.is_empty()).then_some(2);
            Some(found(get_view(src, collection, key)?, default))
        }
        (CoreFnId::GetIn, [Some(collection), Some(path), rest @ ..]) if rest.len() <= 1 => {
            get_in_view(src, collection, path, !rest.is_empty())
        }
        (CoreFnId::Nth, [Some(collection), Some(Constant::Number(index)), rest @ ..])
            if rest.len() <= 1 =>
        {
            let id = collection_of(collection)?;
            let (shape, items) = shape_view(src, id)?;
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
            let (shape, items) = shape_view(src, collection_of(collection)?)?;
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
            let (shape, items) = shape_view(src, collection_of(collection)?)?;
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
            let (shape, items) = shape_view(src, collection_of(collection)?)?;
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
fn get_view<S: ExprSource>(src: &S, value: &Constant, key: &Constant) -> Option<Lookup> {
    match value {
        Constant::Collection(id) => lookup_view(src, *id, key),
        Constant::String(_) if !matches!(key, Constant::Nil | Constant::Keyword(_)) => None,
        _ => Some(Lookup::Missing),
    }
}

/// `get-in` of the constant `path` (a vector or list of keys) in `value`,
/// with a default when `defaulted`: each step is a `get`, which with a
/// default stops at the first missing key.
fn get_in_view<S: ExprSource>(
    src: &S,
    value: &Constant,
    path: &Constant,
    defaulted: bool,
) -> Option<Folded> {
    let keys = path_keys_view(src, path)?;
    if keys.is_empty() {
        return Some(Folded::Argument(0));
    }
    walk_view(src, value.clone(), None, keys, defaulted)
}

/// The keys of a constant `get-in` path: a vector or list.
fn path_keys_view<'p, S: ExprSource>(src: &'p S, path: &Constant) -> Option<&'p [ExprId]> {
    match shape_view(src, collection_of(path)?)? {
        (Shape::Vector | Shape::List, keys) => Some(keys),
        _ => None,
    }
}

/// The rest of a `get-in` from `current`, the constant item at `item` (or
/// not an item), through `keys`.
fn walk_view<S: ExprSource>(
    src: &S,
    mut current: Constant,
    mut item: Option<ExprId>,
    keys: &[ExprId],
    defaulted: bool,
) -> Option<Folded> {
    for key in keys {
        let key = Constant::of_view(src, *key)?;
        match get_view(src, &current, &key)? {
            Lookup::Found(found) => {
                current = Constant::item_view(src, found)?;
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
    evaluate_view(program, core, args)
}

fn evaluate_view<S: ExprSource>(src: &S, core: CoreFnId, args: &[Constant]) -> Option<Constant> {
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
        CoreFnId::Equal => Bool(equal_all_view(src, args)?),
        CoreFnId::NotEqual => Bool(!equal_all_view(src, args)?),
        CoreFnId::Identical => match args {
            [a, b]
                if [a, b].iter().all(|arg| {
                    !matches!(
                        arg,
                        Constant::Keyword(_) | Constant::Symbol(_) | Constant::Collection(_)
                    )
                }) =>
            {
                Bool(equal_view(src, a, b)?)
            }
            _ => return None,
        },
        // Collections (light-cljs-221): the caller has checked that core
        // protocols are intact, so the runtime's own implementations answer.
        CoreFnId::Count => Number(match args {
            [Constant::Nil] => 0.0,
            [Constant::String(units)] => units.len() as f64,
            [Constant::Collection(id)] => count_view(src, *id)? as f64,
            _ => return None,
        }),
        // Every constant collection is counted, and has entries exactly when
        // it has items, however its keys merge.
        CoreFnId::IsEmpty => Bool(match args {
            [Constant::Nil] => true,
            [Constant::String(units)] => units.is_empty(),
            [Constant::Collection(id)] => shape_view(src, *id)?.1.is_empty(),
            _ => return None,
        }),
        CoreFnId::Contains => match args {
            [Constant::Collection(id), key] => match shape_view(src, *id)? {
                // `contains` on a vector: an integer index in range.
                (Shape::Vector, items) => Bool(matches!(key, Constant::Number(index)
                    if index.fract() == 0.0 && *index >= 0.0 && *index < items.len() as f64)),
                (Shape::List, _) => return None,
                _ => Bool(lookup_view(src, *id, key)? != Lookup::Missing),
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
            let (shape, _) = shape_view(src, *id)?;
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
fn equal_view<S: ExprSource>(src: &S, a: &Constant, b: &Constant) -> Option<bool> {
    Some(match (a, b) {
        (Constant::Collection(_), _) | (_, Constant::Collection(_)) => return None,
        (Constant::Keyword(a), Constant::Keyword(b))
        | (Constant::Symbol(a), Constant::Symbol(b)) => {
            let (a, b) = (src.named_literal(*a), src.named_literal(*b));
            let spelling = |name: Option<StringId>| name.map(|name| src.string(name));
            spelling(a.namespace) == spelling(b.namespace)
                && src.string(a.name) == src.string(b.name)
        }
        (Constant::Number(a), Constant::Number(b)) => a == b,
        (a, b) => a == b,
    })
}

/// `equal` over its arguments, pairwise.
fn equal_all_view<S: ExprSource>(src: &S, args: &[Constant]) -> Option<bool> {
    if args.is_empty() {
        return None;
    }
    // The runtime stops at the first unequal pair.
    for pair in args.windows(2) {
        if !equal_view(src, &pair[0], &pair[1])? {
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
    let facts = ProgramFacts::new_light(program, &top);
    fold_with_facts_with_slots(program, built, &facts, Slots::ONE)
}

/// Fold `program` in place using precomputed [`ProgramFacts`], returning how
/// many expressions were rewritten.
pub fn fold_with_facts(
    program: &mut Program,
    built: &mut Vec<bool>,
    facts: &ProgramFacts,
) -> usize {
    fold_with_facts_with_slots(program, built, facts, Slots::ONE)
}

/// A top-level child of `program.root` (or `program.root` itself if not a `Do`),
/// paired with the contiguous range of flattened `top` forms it contains.
#[derive(Clone, Copy)]
struct RootItem {
    id: ExprId,
    top_start: usize,
    top_end: usize,
}

fn collect_root_items(program: &Program) -> (Vec<ExprId>, Vec<RootItem>, bool) {
    let mut top = Vec::new();
    if let ExprKind::Do(items) = &program.exprs[program.root.0].kind {
        let mut root_items = Vec::with_capacity(items.len());
        for &item in items {
            let start = top.len();
            flatten(program, item, &mut top);
            root_items.push(RootItem {
                id: item,
                top_start: start,
                top_end: top.len(),
            });
        }
        (top, root_items, true)
    } else {
        flatten(program, program.root, &mut top);
        let root_items = vec![RootItem {
            id: program.root,
            top_start: 0,
            top_end: top.len(),
        }];
        (top, root_items, false)
    }
}

struct ReachScanSlice {
    reached: BitSet,
    positions: Vec<(u32, u32)>,
    multi_parents: Vec<(u32, u8)>,
    local_reads: Vec<u32>,
    item_weights: Vec<u64>,
    inter_item_shared: bool,
}

fn scan_reach_slice(
    program: &Program,
    top: &[ExprId],
    root_items: &[RootItem],
    range: std::ops::Range<usize>,
) -> ReachScanSlice {
    let count = program.exprs.len();
    let mut reached = BitSet::new(count);
    let mut in_item = BitSet::new(count);
    let mut item_nodes: Vec<u32> = Vec::new();
    let mut positions: Vec<(u32, u32)> = Vec::new();
    let mut parent_overflow: HashMap<u32, u8> = HashMap::new();
    let mut local_reads: Vec<u32> = Vec::new();
    let mut item_weights = Vec::with_capacity(range.len());
    let mut inter_item_shared = false;
    let mut pending: Vec<(ExprId, u32)> = Vec::new();

    for item in &root_items[range] {
        let mut weight = 0u64;
        // Walk intermediate `Do` spine (if `item.id` is a nested `Do`) to count
        // parents and mark reached without assigning `position` to spine `Do`s.
        if item.top_end - item.top_start != 1 || top[item.top_start] != item.id {
            let mut spine = vec![item.id];
            while let Some(d) = spine.pop() {
                let d32 = d.0 as u32;
                if in_item.get(d.0) {
                    let p = parent_overflow.entry(d32).or_insert(1);
                    *p = p.saturating_add(1);
                    continue;
                }
                if reached.get(d.0) {
                    inter_item_shared = true;
                    let p = parent_overflow.entry(d32).or_insert(1);
                    *p = p.saturating_add(1);
                    continue;
                }
                if let ExprKind::Do(children) = &program.exprs[d.0].kind {
                    reached.set(d.0);
                    in_item.set(d.0);
                    item_nodes.push(d32);
                    weight += 1;
                    for &c in children {
                        if matches!(program.exprs[c.0].kind, ExprKind::Do(_)) {
                            spine.push(c);
                        }
                    }
                }
            }
        }
        for top_idx in item.top_start..item.top_end {
            let form = top[top_idx];
            let form32 = form.0 as u32;
            if in_item.get(form.0) {
                let p = parent_overflow.entry(form32).or_insert(1);
                *p = p.saturating_add(1);
                continue;
            }
            if reached.get(form.0) {
                inter_item_shared = true;
                let p = parent_overflow.entry(form32).or_insert(1);
                *p = p.saturating_add(1);
                continue;
            }
            reached.set(form.0);
            in_item.set(form.0);
            item_nodes.push(form32);
            positions.push((form32, top_idx as u32));
            pending.push((form, top_idx as u32));
            while let Some((id, pos)) = pending.pop() {
                weight += 1;
                if let ExprKind::Local(local) = program.exprs[id.0].kind {
                    local_reads.push(local.0 as u32);
                }
                visit_children(&program.exprs[id.0].kind, |child| {
                    let c32 = child.0 as u32;
                    if in_item.get(child.0) {
                        let p = parent_overflow.entry(c32).or_insert(1);
                        *p = p.saturating_add(1);
                    } else if reached.get(child.0) {
                        inter_item_shared = true;
                        let p = parent_overflow.entry(c32).or_insert(1);
                        *p = p.saturating_add(1);
                    } else {
                        reached.set(child.0);
                        in_item.set(child.0);
                        item_nodes.push(c32);
                        positions.push((c32, pos));
                        pending.push((child, pos));
                    }
                });
            }
        }
        for node in item_nodes.drain(..) {
            in_item.clear(node as usize);
        }
        item_weights.push(weight.max(1));
    }

    ReachScanSlice {
        reached,
        positions,
        multi_parents: parent_overflow.into_iter().collect(),
        local_reads,
        item_weights,
        inter_item_shared,
    }
}

struct FoldReachData {
    position: Vec<u32>,
    parents: Vec<u8>,
    reads: Vec<u32>,
    item_weights: Vec<u64>,
    can_parallel_fold: bool,
}

fn scan_fold_reachability_serial(
    program: &Program,
    top: &[ExprId],
    root_items_len: usize,
) -> FoldReachData {
    let count = program.exprs.len();
    let mut position = vec![NOWHERE; count];
    let mut parents = vec![0u8; count];
    let mut reads = vec![0u32; program.locals.len()];
    let mut reached = BitSet::new(count);

    // Single fused reachability pass: mark intermediate `Do` spine from
    // `program.root`, then DFS each flattened `top` form once to fill
    // `position`, `parents`, and `reads`.
    if let ExprKind::Do(_) = &program.exprs[program.root.0].kind {
        let mut spine = vec![program.root];
        reached.set(program.root.0);
        while let Some(d) = spine.pop() {
            if let ExprKind::Do(children) = &program.exprs[d.0].kind {
                for &c in children {
                    parents[c.0] = parents[c.0].saturating_add(1);
                    if matches!(program.exprs[c.0].kind, ExprKind::Do(_)) && !reached.set(c.0) {
                        spine.push(c);
                    }
                }
            }
        }
    }

    let mut pending = Vec::new();
    for (index, &form) in top.iter().enumerate() {
        let pos = index as u32;
        if position[form.0] == NOWHERE {
            position[form.0] = pos;
            if !reached.set(form.0) {
                pending.push(form);
                while let Some(id) = pending.pop() {
                    if let ExprKind::Local(local) = program.exprs[id.0].kind {
                        reads[local.0] = reads[local.0].saturating_add(1);
                    }
                    visit_children(&program.exprs[id.0].kind, |child| {
                        parents[child.0] = parents[child.0].saturating_add(1);
                        if position[child.0] == NOWHERE {
                            position[child.0] = pos;
                        }
                        if !reached.set(child.0) {
                            pending.push(child);
                        }
                    });
                }
            }
        }
    }

    FoldReachData {
        position,
        parents,
        reads,
        item_weights: vec![1; root_items_len],
        can_parallel_fold: false,
    }
}

fn scan_fold_reachability(
    program: &Program,
    top: &[ExprId],
    root_items: &[RootItem],
    root_is_do: bool,
    slots: Slots,
) -> FoldReachData {
    let count = program.exprs.len();
    let use_parallel = root_is_do
        && slots.get() > 1
        && count >= PARALLEL_EXPR_THRESHOLD
        && root_items.len() >= PARALLEL_FORM_THRESHOLD;
    if !use_parallel {
        return scan_fold_reachability_serial(program, top, root_items.len());
    }

    let target_slices = (slots.get() * SLICES_PER_SLOT).min(root_items.len());
    let est_weights: Vec<u64> = root_items
        .iter()
        .map(|item| (item.top_end - item.top_start).max(1) as u64)
        .collect();
    let ranges = partition_slices(&est_weights, target_slices);
    let jobs: Vec<(u64, std::ops::Range<usize>)> = ranges
        .into_iter()
        .map(|r| {
            let w: u64 = est_weights[r.clone()].iter().sum();
            (w, r)
        })
        .collect();
    let slices = scheduler::map(slots, jobs, |range| {
        scan_reach_slice(program, top, root_items, range)
    });

    let mut global_reached = BitSet::new(count);
    global_reached.set(program.root.0);
    let mut any_overlap = false;
    for slice in &slices {
        if slice.inter_item_shared || global_reached.intersects(&slice.reached) {
            any_overlap = true;
            break;
        }
        global_reached.or_assign(&slice.reached);
    }
    if any_overlap {
        drop(slices);
        return scan_fold_reachability_serial(program, top, root_items.len());
    }
    drop(global_reached);

    let mut position = vec![NOWHERE; count];
    let mut parents = vec![0u8; count];
    let mut reads = vec![0u32; program.locals.len()];
    let mut item_weights = Vec::with_capacity(root_items.len());
    for slice in slices {
        for (id, pos) in slice.positions {
            let idx = id as usize;
            position[idx] = pos;
            parents[idx] = 1;
        }
        for (id, p) in slice.multi_parents {
            parents[id as usize] = p;
        }
        for local in slice.local_reads {
            let l = local as usize;
            reads[l] = reads[l].saturating_add(1);
        }
        item_weights.extend(slice.item_weights);
    }
    // Root's direct children have parent count at least 1 (including nested Do spines).
    for item in root_items {
        if parents[item.id.0] == 0 {
            parents[item.id.0] = 1;
        }
    }
    FoldReachData {
        position,
        parents,
        reads,
        item_weights,
        can_parallel_fold: true,
    }
}

/// Fused single-pass arena scan for `fold_with_facts_with_slots`: computes
/// `sole_definitions`, `pinned`, `assignable`, and `fenced` in one pass over
/// `program.exprs`, parallelized over chunks when `slots > Slots::ONE`.
struct FoldArenaChunk {
    defs: Vec<(GlobalId, ExprId)>,
    assigned_globals: Vec<GlobalId>,
    pinned_locals: Vec<LocalId>,
    assignable_locals: Vec<LocalId>,
    fenced_exprs: Vec<ExprId>,
}

fn scan_fold_arena_chunk(
    program: &Program,
    position: &[u32],
    range: std::ops::Range<usize>,
) -> FoldArenaChunk {
    let mut defs = Vec::new();
    let mut assigned_globals = Vec::new();
    let mut pinned_locals = Vec::new();
    let mut assignable_locals = Vec::new();
    let mut fenced_exprs = Vec::new();

    let mark_target_global = |mut target: ExprId, out: &mut Vec<GlobalId>| {
        while let ExprKind::BooleanHint(inner) = program.exprs[target.0].kind {
            target = inner;
        }
        if let ExprKind::Global(global) = program.exprs[target.0].kind {
            out.push(global);
        }
    };

    for index in range {
        let kind = &program.exprs[index].kind;
        match kind {
            ExprKind::SetGlobal { target, .. } => {
                mark_target_global(*target, &mut assigned_globals);
                if let ExprKind::Local(local) = program.exprs[target.0].kind {
                    pinned_locals.push(local);
                    assignable_locals.push(local);
                }
            }
            ExprKind::Binding { bindings, .. } => {
                for &(target, _) in bindings {
                    mark_target_global(target, &mut assigned_globals);
                }
            }
            ExprKind::JsTemplate { args, .. } => {
                for &arg in args {
                    mark_target_global(arg, &mut assigned_globals);
                    if let ExprKind::Local(local) = program.exprs[arg.0].kind {
                        pinned_locals.push(local);
                        assignable_locals.push(local);
                    }
                }
            }
            ExprKind::Def {
                global, init, once, ..
            } => {
                if position[index] != NOWHERE && (init.is_some() || *once) {
                    defs.push((*global, ExprId(index)));
                }
            }
            ExprKind::Global(_) => (),
            ExprKind::Fn { methods, .. } => {
                for method in methods {
                    pinned_locals.extend_from_slice(&method.captures);
                }
            }
            ExprKind::Reify {
                captures, methods, ..
            } => {
                pinned_locals.extend_from_slice(captures);
                for (_, method) in methods {
                    pinned_locals.extend_from_slice(&method.captures);
                }
            }
            ExprKind::DefType { methods, .. } | ExprKind::ExtendType { methods, .. } => {
                crate::application::visit_globals(kind, |global| {
                    assigned_globals.push(global);
                });
                for (_, method) in methods {
                    pinned_locals.extend_from_slice(&method.captures);
                }
            }
            ExprKind::Map(items) | ExprKind::LiteralMap(items) => {
                for &key in items.iter().step_by(2) {
                    fenced_exprs.push(key);
                    if let ExprKind::Local(local) = program.exprs[key.0].kind {
                        pinned_locals.push(local);
                    }
                }
            }
            ExprKind::Set(items) | ExprKind::LiteralSet(items) => {
                for &member in items {
                    fenced_exprs.push(member);
                    if let ExprKind::Local(local) = program.exprs[member.0].kind {
                        pinned_locals.push(local);
                    }
                }
            }
            _ => {
                crate::application::visit_globals(kind, |global| {
                    assigned_globals.push(global);
                });
            }
        }
    }

    FoldArenaChunk {
        defs,
        assigned_globals,
        pinned_locals,
        assignable_locals,
        fenced_exprs,
    }
}

struct FoldArenaFacts {
    definitions: Vec<Option<ExprId>>,
    pinned: BitSet,
    assignable: BitSet,
    fenced: BitSet,
}

fn scan_fold_arena(program: &Program, position: &[u32], slots: Slots) -> FoldArenaFacts {
    let count = program.exprs.len();
    let globals = program.globals.len();
    let locals = program.locals.len();

    let mut def_state: Vec<(u32, Option<ExprId>)> = vec![(0, None); globals];
    let mut assigned = BitSet::new(globals);
    let mut pinned = BitSet::new(locals);
    let mut assignable = BitSet::new(locals);
    let mut fenced = BitSet::new(count);

    for local in program.field_bindings.keys() {
        pinned.set(local.0);
        assignable.set(local.0);
    }

    if slots.get() > 1 && count >= PARALLEL_EXPR_THRESHOLD {
        let num_chunks = (slots.get() * SLICES_PER_SLOT).min(count);
        let chunk_size = count.div_ceil(num_chunks);
        let jobs: Vec<(u64, std::ops::Range<usize>)> = (0..count)
            .step_by(chunk_size)
            .map(|start| {
                let end = (start + chunk_size).min(count);
                ((end - start) as u64, start..end)
            })
            .collect();
        let chunks = scheduler::map(slots, jobs, |range| {
            scan_fold_arena_chunk(program, position, range)
        });
        for chunk in chunks {
            for (global, expr_id) in chunk.defs {
                let entry = &mut def_state[global.0];
                entry.0 += 1;
                entry.1 = Some(expr_id);
            }
            for global in chunk.assigned_globals {
                assigned.set(global.0);
            }
            for local in chunk.pinned_locals {
                pinned.set(local.0);
            }
            for local in chunk.assignable_locals {
                assignable.set(local.0);
            }
            for expr_id in chunk.fenced_exprs {
                fenced.set(expr_id.0);
            }
        }
    } else {
        let mark_target_global = |mut target: ExprId, assigned: &mut BitSet| {
            while let ExprKind::BooleanHint(inner) = program.exprs[target.0].kind {
                target = inner;
            }
            if let ExprKind::Global(global) = program.exprs[target.0].kind {
                assigned.set(global.0);
            }
        };
        for (index, expr) in program.exprs.iter().enumerate() {
            let kind = &expr.kind;
            match kind {
                ExprKind::SetGlobal { target, .. } => {
                    mark_target_global(*target, &mut assigned);
                    if let ExprKind::Local(local) = program.exprs[target.0].kind {
                        pinned.set(local.0);
                        assignable.set(local.0);
                    }
                }
                ExprKind::Binding { bindings, .. } => {
                    for &(target, _) in bindings {
                        mark_target_global(target, &mut assigned);
                    }
                }
                ExprKind::JsTemplate { args, .. } => {
                    for &arg in args {
                        mark_target_global(arg, &mut assigned);
                        if let ExprKind::Local(local) = program.exprs[arg.0].kind {
                            pinned.set(local.0);
                            assignable.set(local.0);
                        }
                    }
                }
                ExprKind::Def {
                    global, init, once, ..
                } => {
                    if position[index] != NOWHERE && (init.is_some() || *once) {
                        let entry = &mut def_state[global.0];
                        entry.0 += 1;
                        entry.1 = Some(ExprId(index));
                    }
                }
                ExprKind::Global(_) => (),
                ExprKind::Fn { methods, .. } => {
                    for method in methods {
                        for local in &method.captures {
                            pinned.set(local.0);
                        }
                    }
                }
                ExprKind::Reify {
                    captures, methods, ..
                } => {
                    for local in captures {
                        pinned.set(local.0);
                    }
                    for (_, method) in methods {
                        for local in &method.captures {
                            pinned.set(local.0);
                        }
                    }
                }
                ExprKind::DefType { methods, .. } | ExprKind::ExtendType { methods, .. } => {
                    crate::application::visit_globals(kind, |global| {
                        assigned.set(global.0);
                    });
                    for (_, method) in methods {
                        for local in &method.captures {
                            pinned.set(local.0);
                        }
                    }
                }
                ExprKind::Map(items) | ExprKind::LiteralMap(items) => {
                    for &key in items.iter().step_by(2) {
                        fenced.set(key.0);
                        if let ExprKind::Local(local) = program.exprs[key.0].kind {
                            pinned.set(local.0);
                        }
                    }
                }
                ExprKind::Set(items) | ExprKind::LiteralSet(items) => {
                    for &member in items {
                        fenced.set(member.0);
                        if let ExprKind::Local(local) = program.exprs[member.0].kind {
                            pinned.set(local.0);
                        }
                    }
                }
                _ => {
                    crate::application::visit_globals(kind, |global| {
                        assigned.set(global.0);
                    });
                }
            }
        }
    }

    let definitions = def_state
        .into_iter()
        .enumerate()
        .map(|(idx, def)| match def {
            (1, definition) if !assigned.get(idx) => definition,
            _ => None,
        })
        .collect();

    FoldArenaFacts {
        definitions,
        pinned,
        assignable,
        fenced,
    }
}

/// Fused single-pass `sole_definitions` + `fenced_positions` over
/// `program.exprs` for [`crate::inlining`], parallelized via `scheduler::map`.
pub(crate) fn sole_definitions_and_fenced_with_slots(
    program: &Program,
    reached: &BitSet,
    slots: Slots,
) -> (Vec<Option<ExprId>>, BitSet) {
    let count = program.exprs.len();
    let globals = program.globals.len();

    let scan_chunk = |range: std::ops::Range<usize>| {
        let mut defs: Vec<(GlobalId, ExprId)> = Vec::new();
        let mut assigned_globals: Vec<GlobalId> = Vec::new();
        let mut fenced_exprs: Vec<ExprId> = Vec::new();
        let mark_target = |mut target: ExprId, out: &mut Vec<GlobalId>| {
            while let ExprKind::BooleanHint(inner) = program.exprs[target.0].kind {
                target = inner;
            }
            if let ExprKind::Global(global) = program.exprs[target.0].kind {
                out.push(global);
            }
        };
        for index in range {
            let kind = &program.exprs[index].kind;
            match kind {
                ExprKind::SetGlobal { target, .. } => mark_target(*target, &mut assigned_globals),
                ExprKind::Binding { bindings, .. } => {
                    for &(target, _) in bindings {
                        mark_target(target, &mut assigned_globals);
                    }
                }
                ExprKind::JsTemplate { args, .. } => {
                    for &arg in args {
                        mark_target(arg, &mut assigned_globals);
                    }
                }
                ExprKind::Def {
                    global, init, once, ..
                } => {
                    if reached.get(index) && (init.is_some() || *once) {
                        defs.push((*global, ExprId(index)));
                    }
                }
                ExprKind::Global(_) => (),
                ExprKind::Map(items) | ExprKind::LiteralMap(items) => {
                    for &key in items.iter().step_by(2) {
                        fenced_exprs.push(key);
                    }
                }
                ExprKind::Set(items) | ExprKind::LiteralSet(items) => {
                    fenced_exprs.extend_from_slice(items);
                }
                _ => {
                    crate::application::visit_globals(kind, |global| {
                        assigned_globals.push(global);
                    });
                }
            }
        }
        (defs, assigned_globals, fenced_exprs)
    };

    let mut def_state: Vec<(u32, Option<ExprId>)> = vec![(0, None); globals];
    let mut assigned = BitSet::new(globals);
    let mut fenced = BitSet::new(count);

    if slots.get() > 1 && count >= PARALLEL_EXPR_THRESHOLD {
        let num_chunks = (slots.get() * SLICES_PER_SLOT).min(count);
        let chunk_size = count.div_ceil(num_chunks);
        let jobs: Vec<(u64, std::ops::Range<usize>)> = (0..count)
            .step_by(chunk_size)
            .map(|start| {
                let end = (start + chunk_size).min(count);
                ((end - start) as u64, start..end)
            })
            .collect();
        for (defs, assigned_globals, fenced_exprs) in scheduler::map(slots, jobs, scan_chunk) {
            for (global, expr_id) in defs {
                let entry = &mut def_state[global.0];
                entry.0 += 1;
                entry.1 = Some(expr_id);
            }
            for global in assigned_globals {
                assigned.set(global.0);
            }
            for expr_id in fenced_exprs {
                fenced.set(expr_id.0);
            }
        }
    } else {
        let mark_target = |mut target: ExprId, assigned: &mut BitSet| {
            while let ExprKind::BooleanHint(inner) = program.exprs[target.0].kind {
                target = inner;
            }
            if let ExprKind::Global(global) = program.exprs[target.0].kind {
                assigned.set(global.0);
            }
        };
        for (index, expr) in program.exprs.iter().enumerate() {
            let kind = &expr.kind;
            match kind {
                ExprKind::SetGlobal { target, .. } => mark_target(*target, &mut assigned),
                ExprKind::Binding { bindings, .. } => {
                    for &(target, _) in bindings {
                        mark_target(target, &mut assigned);
                    }
                }
                ExprKind::JsTemplate { args, .. } => {
                    for &arg in args {
                        mark_target(arg, &mut assigned);
                    }
                }
                ExprKind::Def {
                    global, init, once, ..
                } => {
                    if reached.get(index) && (init.is_some() || *once) {
                        let entry = &mut def_state[global.0];
                        entry.0 += 1;
                        entry.1 = Some(ExprId(index));
                    }
                }
                ExprKind::Global(_) => (),
                ExprKind::Map(items) | ExprKind::LiteralMap(items) => {
                    for &key in items.iter().step_by(2) {
                        fenced.set(key.0);
                    }
                }
                ExprKind::Set(items) | ExprKind::LiteralSet(items) => {
                    for &member in items {
                        fenced.set(member.0);
                    }
                }
                _ => {
                    crate::application::visit_globals(kind, |global| {
                        assigned.set(global.0);
                    });
                }
            }
        }
    }

    let definitions = def_state
        .into_iter()
        .enumerate()
        .map(|(idx, def)| match def {
            (1, definition) if !assigned.get(idx) => definition,
            _ => None,
        })
        .collect();
    (definitions, fenced)
}

/// Whether `root` is a pure literal tree (scalars, symbols, collections,
/// `with-meta`, reader literals) with no expression that [`Folder`] could
/// rewrite or fold. Used to skip unnecessary serial pre-folding of already
/// constant `^:const` collection definitions before parallel fan-out.
fn is_pure_literal_tree(program: &Program, root: ExprId) -> bool {
    let mut pending = vec![root];
    while let Some(id) = pending.pop() {
        match &program.exprs[id.0].kind {
            ExprKind::Nil
            | ExprKind::Bool(_)
            | ExprKind::Number(_)
            | ExprKind::String(_)
            | ExprKind::Keyword(_)
            | ExprKind::Symbol(_) => (),
            ExprKind::Vector(items)
            | ExprKind::List(items)
            | ExprKind::Map(items)
            | ExprKind::LiteralMap(items)
            | ExprKind::Set(items)
            | ExprKind::LiteralSet(items)
            | ExprKind::ReaderLiteral { items, .. } => pending.extend_from_slice(items),
            ExprKind::WithMeta { value, metadata } => {
                pending.push(*value);
                pending.push(*metadata);
            }
            _ => return false,
        }
    }
    true
}

/// Fold `program` in place using precomputed [`ProgramFacts`] and `slots` CPU
/// work slots (`--jobs N`), returning how many expressions were rewritten.
pub fn fold_with_facts_with_slots(
    program: &mut Program,
    built: &mut Vec<bool>,
    facts: &ProgramFacts,
    slots: Slots,
) -> usize {
    let (top, root_items, root_is_do) = collect_root_items(program);
    let count = program.exprs.len();
    built.resize(count, false);

    let FoldReachData {
        mut position,
        mut parents,
        reads,
        item_weights,
        can_parallel_fold,
    } = scan_fold_reachability(program, &top, &root_items, root_is_do, slots);

    let FoldArenaFacts {
        definitions,
        pinned,
        assignable,
        mut fenced,
    } = scan_fold_arena(program, &position, slots);

    // Constant globals: defined by one `def` with a value that the root
    // reaches, a direct top-level form, and assigned or defined by nothing
    // else, and what each one makes known (light-cljs-984). A `declare`
    // defines no value and does not count.
    let ConstantGlobals {
        values: globals,
        collections,
        returns,
    } = constant_globals_from_defs(program, &top, &position, definitions);

    let mut kept = BitSet::new(count);
    kept.set(program.root.0);
    for id in program.namespace_entries.keys() {
        kept.set(id.0);
    }
    for id in &program.macro_functions {
        if id.0 < count {
            kept.set(id.0);
        }
    }
    let replaced = facts.replaced_names();

    let mut total_folded = 0usize;
    let mut saw_shared_unread_let = false;

    if can_parallel_fold {
        // Pre-fold any `^:const` collection definitions serially in top-level
        // order so every collection literal in `collections` is frozen and
        // read-only before parallel fan-out.
        let mut const_coll_positions: Vec<u32> = collections
            .iter()
            .filter_map(|entry| entry.map(|(at, _)| at))
            .collect();
        const_coll_positions.sort_unstable();
        const_coll_positions.dedup();
        let mut pre_folded_defs: HashSet<ExprId> = HashSet::new();
        for at in const_coll_positions {
            let def_id = top[at as usize];
            if let ExprKind::Def {
                init: Some(init),
                test: None,
                ..
            } = program.exprs[def_id.0].kind
                && is_pure_literal_tree(program, init)
            {
                continue;
            }
            let discarded = (at as usize) + 1 != top.len();
            let shared = FoldContext {
                program: &*program,
                facts,
                replaced: &replaced,
                position: &position,
                globals: &globals,
                collections: &collections,
                returns: &returns,
                pinned: &pinned,
                assignable: &assignable,
                reads: &reads,
                kept: &kept,
                fenced: &fenced,
                built: built.as_slice(),
                parents: &parents,
                pre_folded: &pre_folded_defs,
            };
            let mut folder = Folder::new(&shared);
            folder.visit(def_id, discarded);
            let delta = folder.into_delta();
            total_folded += delta.folded;
            saw_shared_unread_let |= delta.saw_shared_unread_let;
            let added = delta.new_exprs.len();
            apply_fold_delta(
                program,
                built,
                delta,
                shared.program.exprs.len(),
                0,
                shared.program.strings.len(),
                0,
            );
            if added > 0 {
                let new_len = program.exprs.len();
                position.resize(new_len, at);
                parents.resize(new_len, 1);
                kept.resize(new_len);
                fenced.resize(new_len);
            }
            pre_folded_defs.insert(def_id);
        }

        let base_exprs = program.exprs.len();
        let base_strings = program.strings.len();
        let target_slices = (slots.get() * SLICES_PER_SLOT).min(root_items.len());
        let ranges = partition_slices(&item_weights, target_slices);
        let jobs: Vec<(u64, std::ops::Range<usize>)> = ranges
            .into_iter()
            .map(|r| {
                let w: u64 = item_weights[r.clone()].iter().sum();
                (w, r)
            })
            .collect();
        let last_root_item = root_items.len().saturating_sub(1);

        let deltas: Vec<FoldDelta> = {
            let shared = FoldContext {
                program: &*program,
                facts,
                replaced: &replaced,
                position: &position,
                globals: &globals,
                collections: &collections,
                returns: &returns,
                pinned: &pinned,
                assignable: &assignable,
                reads: &reads,
                kept: &kept,
                fenced: &fenced,
                built: built.as_slice(),
                parents: &parents,
                pre_folded: &pre_folded_defs,
            };
            scheduler::map(slots, jobs, |range| {
                let mut folder = Folder::new(&shared);
                for index in range {
                    folder.visit(root_items[index].id, index != last_root_item);
                }
                folder.into_delta()
            })
        };

        let mut expr_offset = 0usize;
        let mut string_offset = 0usize;
        for delta in deltas {
            total_folded += delta.folded;
            saw_shared_unread_let |= delta.saw_shared_unread_let;
            let added_exprs = delta.new_exprs.len();
            let added_strings = delta.new_strings.len();
            apply_fold_delta(
                program,
                built,
                delta,
                base_exprs,
                expr_offset,
                base_strings,
                string_offset,
            );
            expr_offset += added_exprs;
            string_offset += added_strings;
        }
        let final_len = program.exprs.len();
        kept.resize(final_len);
        fenced.resize(final_len);
    } else if root_is_do && !root_items.is_empty() {
        // Serial path over a `Do` root: flush deltas every small batch of
        // top-level items while preserving a single `visited` BitSet so peak
        // delta memory stays tiny and `delta_map` stays cache-hot.
        const FLUSH_BATCH: usize = 64;
        let last_root_item = root_items.len().saturating_sub(1);
        let empty_pre = HashSet::new();
        let mut visited = BitSet::new(program.exprs.len());
        visited.set(program.root.0);
        for batch_start in (0..root_items.len()).step_by(FLUSH_BATCH) {
            let batch_end = (batch_start + FLUSH_BATCH).min(root_items.len());
            let base_exprs = program.exprs.len();
            let base_strings = program.strings.len();
            visited.resize(base_exprs);
            let (delta, returned_visited) = {
                let shared = FoldContext {
                    program: &*program,
                    facts,
                    replaced: &replaced,
                    position: &position,
                    globals: &globals,
                    collections: &collections,
                    returns: &returns,
                    pinned: &pinned,
                    assignable: &assignable,
                    reads: &reads,
                    kept: &kept,
                    fenced: &fenced,
                    built: built.as_slice(),
                    parents: &parents,
                    pre_folded: &empty_pre,
                };
                let mut folder = Folder::with_visited(&shared, visited);
                for index in batch_start..batch_end {
                    folder.visit(root_items[index].id, index != last_root_item);
                }
                folder.into_delta_and_visited()
            };
            visited = returned_visited;
            total_folded += delta.folded;
            saw_shared_unread_let |= delta.saw_shared_unread_let;
            let added = delta.new_exprs.len();
            apply_fold_delta(program, built, delta, base_exprs, 0, base_strings, 0);
            if added > 0 {
                let new_len = program.exprs.len();
                position.resize(new_len, NOWHERE);
                parents.resize(new_len, 1);
                kept.resize(new_len);
                fenced.resize(new_len);
            }
        }
    } else {
        let base_exprs = program.exprs.len();
        let base_strings = program.strings.len();
        let empty_pre = HashSet::new();
        let delta = {
            let shared = FoldContext {
                program: &*program,
                facts,
                replaced: &replaced,
                position: &position,
                globals: &globals,
                collections: &collections,
                returns: &returns,
                pinned: &pinned,
                assignable: &assignable,
                reads: &reads,
                kept: &kept,
                fenced: &fenced,
                built: built.as_slice(),
                parents: &parents,
                pre_folded: &empty_pre,
            };
            let mut folder = Folder::new(&shared);
            folder.visit(program.root, false);
            folder.into_delta()
        };
        total_folded += delta.folded;
        saw_shared_unread_let |= delta.saw_shared_unread_let;
        apply_fold_delta(program, built, delta, base_exprs, 0, base_strings, 0);
        let final_len = program.exprs.len();
        kept.resize(final_len);
        fenced.resize(final_len);
    }

    if total_folded > 0 || saw_shared_unread_let {
        total_folded += sweep_with_slots(program, built, &pinned, &kept, &fenced, slots);
    }
    total_folded
}

/// How often the code the root reaches reads each local, by local. Fills
/// `reached` (by expression) and, when given, `parents`: how many reached
/// expressions each one is a child of, and `lets`: reached `ExprKind::Let`
/// indices.
fn reachable_reads_bits(
    program: &Program,
    mut parents: Option<&mut Vec<u8>>,
    reached: &mut BitSet,
    mut lets: Option<&mut Vec<usize>>,
) -> Vec<u32> {
    let mut reads = vec![0u32; program.locals.len()];
    reached.set(program.root.0);
    let mut pending = vec![program.root];
    while let Some(id) = pending.pop() {
        match program.exprs[id.0].kind {
            ExprKind::Local(local) => reads[local.0] = reads[local.0].saturating_add(1),
            ExprKind::Let { .. } => {
                if let Some(lets) = lets.as_deref_mut() {
                    lets.push(id.0);
                }
            }
            _ => (),
        }
        visit_children(&program.exprs[id.0].kind, |child| {
            if let Some(parents) = parents.as_deref_mut() {
                parents[child.0] = parents[child.0].saturating_add(1);
            }
            if !reached.set(child.0) {
                pending.push(child);
            }
        });
    }
    reads
}

/// Parallel reachability bitset from `program.root` via `scheduler::map`
/// (without allocating or counting local reads).
fn reachable_bitset_with_slots(program: &Program, slots: Slots) -> BitSet {
    let count = program.exprs.len();
    if let ExprKind::Do(items) = &program.exprs[program.root.0].kind
        && slots.get() > 1
        && count >= PARALLEL_EXPR_THRESHOLD
        && items.len() >= PARALLEL_FORM_THRESHOLD
    {
        let target_slices = (slots.get() * SLICES_PER_SLOT).min(items.len());
        let chunk_size = items.len().div_ceil(target_slices);
        let jobs: Vec<(u64, std::ops::Range<usize>)> = (0..items.len())
            .step_by(chunk_size)
            .map(|start| {
                let end = (start + chunk_size).min(items.len());
                ((end - start) as u64, start..end)
            })
            .collect();
        let bitsets = scheduler::map(slots, jobs, |range| {
            let mut local = BitSet::new(count);
            let mut pending = Vec::new();
            for &item in &items[range] {
                if !local.set(item.0) {
                    pending.push(item);
                }
                while let Some(id) = pending.pop() {
                    visit_children(&program.exprs[id.0].kind, |child| {
                        if !local.set(child.0) {
                            pending.push(child);
                        }
                    });
                }
            }
            local
        });
        let mut reached = BitSet::new(count);
        reached.set(program.root.0);
        for bs in bitsets {
            reached.or_assign(&bs);
        }
        return reached;
    }

    let mut reached = BitSet::new(count);
    reached.set(program.root.0);
    let mut pending = vec![program.root];
    while let Some(id) = pending.pop() {
        visit_children(&program.exprs[id.0].kind, |child| {
            if !reached.set(child.0) {
                pending.push(child);
            }
        });
    }
    reached
}

/// Parallel reachability + local read counts + reached `Let` indices for
/// [`sweep_with_slots`].
fn reachable_reads_and_lets_with_slots(program: &Program, slots: Slots) -> (Vec<u32>, Vec<usize>) {
    let count = program.exprs.len();
    if let ExprKind::Do(items) = &program.exprs[program.root.0].kind
        && slots.get() > 1
        && count >= PARALLEL_EXPR_THRESHOLD
        && items.len() >= PARALLEL_FORM_THRESHOLD
    {
        let target_slices = (slots.get() * SLICES_PER_SLOT).min(items.len());
        let chunk_size = items.len().div_ceil(target_slices);
        let jobs: Vec<(u64, std::ops::Range<usize>)> = (0..items.len())
            .step_by(chunk_size)
            .map(|start| {
                let end = (start + chunk_size).min(items.len());
                ((end - start) as u64, start..end)
            })
            .collect();
        let slices = scheduler::map(slots, jobs, |range| {
            let mut reached = BitSet::new(count);
            let mut local_reads = Vec::new();
            let mut lets = Vec::new();
            let mut pending = Vec::new();
            for &item in &items[range] {
                if !reached.set(item.0) {
                    pending.push(item);
                }
                while let Some(id) = pending.pop() {
                    match program.exprs[id.0].kind {
                        ExprKind::Local(local) => local_reads.push(local),
                        ExprKind::Let { .. } => lets.push(id.0),
                        _ => (),
                    }
                    visit_children(&program.exprs[id.0].kind, |child| {
                        if !reached.set(child.0) {
                            pending.push(child);
                        }
                    });
                }
            }
            (reached, local_reads, lets)
        });

        let mut global_reached = BitSet::new(count);
        global_reached.set(program.root.0);
        let mut overlaps = false;
        for (reached, _, _) in &slices {
            if global_reached.intersects(reached) {
                overlaps = true;
                break;
            }
            global_reached.or_assign(reached);
        }
        if !overlaps {
            let mut reads = vec![0u32; program.locals.len()];
            let mut lets = Vec::new();
            for (_, local_reads, slice_lets) in slices {
                for local in local_reads {
                    reads[local.0] = reads[local.0].saturating_add(1);
                }
                lets.extend(slice_lets);
            }
            return (reads, lets);
        }
        drop(slices);
    }

    let mut reached = BitSet::new(count);
    let mut lets = Vec::new();
    let reads = reachable_reads_bits(program, None, &mut reached, Some(&mut lets));
    (reads, lets)
}

/// Drop the bindings that nothing reads any more and whose initializers
/// have no effect, which folding a lookup leaves behind: the local a
/// destructuring binds its value to, and the locals bound to that one.
/// A `let` left without bindings is its body. Repeats (at most
/// [`SWEEPS`] times) while a binding goes, since an initializer may be
/// the last read of an earlier binding.
fn sweep_with_slots(
    program: &mut Program,
    built: &mut [bool],
    pinned: &BitSet,
    kept_ids: &BitSet,
    fenced: &BitSet,
    slots: Slots,
) -> usize {
    let mut total_folded = 0;
    for _ in 0..SWEEPS {
        let (reads, mut lets) = reachable_reads_and_lets_with_slots(program, slots);
        lets.sort_unstable();
        let mut dropped = 0;
        for index in lets {
            let ExprKind::Let { bindings, body } = &program.exprs[index].kind else {
                continue;
            };
            let body = *body;
            let mut kept: Option<Vec<(LocalId, ExprId)>> = None;
            for (b_idx, &(local, init)) in bindings.iter().enumerate() {
                let keep_binding = reads[local.0] > 0
                    || pinned.get(local.0)
                    || !effect_free(program, &program.exprs[init.0].kind);
                if keep_binding {
                    if let Some(ref mut k) = kept {
                        k.push((local, init));
                    }
                } else if kept.is_none() {
                    let mut k = Vec::with_capacity(bindings.len() - 1);
                    k.extend_from_slice(&bindings[..b_idx]);
                    kept = Some(k);
                }
            }
            let Some(kept) = kept else {
                continue;
            };
            dropped += bindings.len() - kept.len();
            if kept.is_empty()
                && !kept_ids.get(index)
                && !scoped(program, body)
                && !kept_ids.get(body.0)
                && !(fenced.get(index) && constant_like(program, &program.exprs[body.0].kind))
            {
                if index < built.len() {
                    built[index] = built.get(body.0).copied().unwrap_or(false);
                }
                program.exprs[index].kind = program.exprs[body.0].kind.clone();
                total_folded += 1;
                continue;
            }
            program.exprs[index].kind = ExprKind::Let {
                bindings: kept,
                body,
            };
        }
        total_folded += dropped;
        if dropped == 0 {
            break;
        }
    }
    total_folded
}

/// Lookups of a constant key in a map literal, or in a local a `let` binds to
/// one (light-cljs-221; "Constant folding extension points" in
/// docs/advanced-optimization.md): by call, the index of the key among the
/// map literal's keys. `Advanced` emission reads the entry at that index,
/// `rt$mapValueAt(m, i)`, instead of calling `get`. The calls are `(:k m)`,
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
    entry_lookups_with_slots(program, facts, Slots::ONE)
}

/// [`entry_lookups`] running its reachability, binding and lookup scans on
/// `slots` CPU work slots (`--jobs N`).
pub fn entry_lookups_with_slots(
    program: &Program,
    facts: &ProgramFacts,
    slots: Slots,
) -> HashMap<ExprId, usize> {
    let mut lookups = HashMap::new();
    if !facts.core_protocols_intact() || facts.replaced(CoreFnId::Get) {
        return lookups;
    }
    let count = program.exprs.len();
    let locals_len = program.locals.len();
    let reached = reachable_bitset_with_slots(program, slots);

    let mut assignable = BitSet::new(locals_len);
    for local in program.field_bindings.keys() {
        assignable.set(local.0);
    }
    let mut bound: Vec<u32> = vec![u32::MAX; locals_len];

    if slots.get() > 1 && count >= PARALLEL_EXPR_THRESHOLD {
        let scan_bindings = |range: std::ops::Range<usize>| {
            let mut assignable_locals = Vec::new();
            let mut bound_entries: Vec<(u32, u32)> = Vec::new();
            for index in range {
                let targets: &[ExprId] = match &program.exprs[index].kind {
                    ExprKind::SetGlobal { target, .. } => std::slice::from_ref(target),
                    ExprKind::JsTemplate { args, .. } => args,
                    ExprKind::Let { bindings, .. } if reached.get(index) => {
                        for &(local, init) in bindings {
                            if matches!(
                                program.exprs[init.0].kind,
                                ExprKind::Local(_) | ExprKind::Map(_)
                            ) {
                                bound_entries.push((local.0 as u32, init.0 as u32));
                            }
                        }
                        continue;
                    }
                    _ => continue,
                };
                for target in targets {
                    if let ExprKind::Local(local) = program.exprs[target.0].kind {
                        assignable_locals.push(local.0 as u32);
                    }
                }
            }
            (assignable_locals, bound_entries)
        };
        let num_chunks = (slots.get() * SLICES_PER_SLOT).min(count);
        let chunk_size = count.div_ceil(num_chunks);
        let jobs: Vec<(u64, std::ops::Range<usize>)> = (0..count)
            .step_by(chunk_size)
            .map(|start| {
                let end = (start + chunk_size).min(count);
                ((end - start) as u64, start..end)
            })
            .collect();
        for (assignable_locals, bound_entries) in scheduler::map(slots, jobs, scan_bindings) {
            for local in assignable_locals {
                assignable.set(local as usize);
            }
            for (local, init) in bound_entries {
                bound[local as usize] = init;
            }
        }
    } else {
        for (index, expr) in program.exprs.iter().enumerate() {
            let targets: &[ExprId] = match &expr.kind {
                ExprKind::SetGlobal { target, .. } => std::slice::from_ref(target),
                ExprKind::JsTemplate { args, .. } => args,
                ExprKind::Let { bindings, .. } if reached.get(index) => {
                    for &(local, init) in bindings {
                        if matches!(
                            program.exprs[init.0].kind,
                            ExprKind::Local(_) | ExprKind::Map(_)
                        ) {
                            bound[local.0] = init.0 as u32;
                        }
                    }
                    continue;
                }
                _ => continue,
            };
            for target in targets {
                if let ExprKind::Local(local) = program.exprs[target.0].kind {
                    assignable.set(local.0);
                }
            }
        }
    }

    let scan_calls = |range: std::ops::Range<usize>| -> Vec<(ExprId, usize)> {
        let mut found_lookups = Vec::new();
        let map = |mut id: ExprId| -> Option<&[ExprId]> {
            for _ in 0..locals_len + 1 {
                match &program.exprs[id.0].kind {
                    ExprKind::Local(local) if !assignable.get(local.0) => {
                        let next = bound[local.0];
                        if next == u32::MAX {
                            return None;
                        }
                        id = ExprId(next as usize);
                    }
                    ExprKind::Map(items) => return Some(items),
                    _ => return None,
                }
            }
            None
        };
        for index in range {
            let ExprKind::Call { callee, args } = &program.exprs[index].kind else {
                continue;
            };
            if !reached.get(index) {
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
            if !keys().all(|key| Constant::item_view(program, key).is_some())
                || !distinct_view(program, keys())
            {
                continue;
            }
            let mut found = None;
            for (position, item) in keys().enumerate() {
                match same_key_view(program, &key, item) {
                    Some(true) if found.is_none() => found = Some(position),
                    Some(false) => (),
                    _ => {
                        found = None;
                        break;
                    }
                }
            }
            if let Some(position) = found {
                found_lookups.push((ExprId(index), position));
            }
        }
        found_lookups
    };

    if slots.get() > 1 && count >= PARALLEL_EXPR_THRESHOLD {
        let num_chunks = (slots.get() * SLICES_PER_SLOT).min(count);
        let chunk_size = count.div_ceil(num_chunks);
        let jobs: Vec<(u64, std::ops::Range<usize>)> = (0..count)
            .step_by(chunk_size)
            .map(|start| {
                let end = (start + chunk_size).min(count);
                ((end - start) as u64, start..end)
            })
            .collect();
        for chunk in scheduler::map(slots, jobs, scan_calls) {
            lookups.extend(chunk);
        }
    } else {
        lookups.extend(scan_calls(0..count));
    }
    lookups
}

/// The longest string, in UTF-16 code units, that a definition which is not
/// `^:const` gives every read of it: a longer one stays one definition read
/// by name, so that substitution never copies long text into many places.
/// A `^:const` string has no limit (upstream substitutes it at every
/// reference).
const COPIED_STRING_LIMIT: usize = 16;

/// The globals [`fold`] knows the value of (light-cljs-214, light-cljs-984).
struct ConstantGlobals {
    /// By global: the top-level position of its sole definition and the
    /// literal of the scalar or symbol every later read is.
    values: Vec<Option<(u32, ExprKind)>>,
    /// By global: the top-level position of its sole definition and the
    /// collection literal it defines, for `^:const` globals, which [`fold`]
    /// folds lookups against but never substitutes.
    collections: Vec<Option<(u32, ExprId)>>,
    /// By global: the top-level position of its sole definition, a function
    /// of one method without parameters whose body is known, and the literal
    /// every later call of it without arguments is.
    returns: Vec<Option<(u32, ExprKind)>>,
}

/// The globals whose sole definition (nothing else defines or assigns them,
/// by `set!`, `binding`, `with-redefs`, a `js*` hole, a type or a protocol)
/// is a direct top-level `def` (not `defonce`), with what that definition
/// makes known (light-cljs-984). Definitions are taken in top-level order,
/// so each reads only what earlier ones made known:
///
/// - a value: a `^:const` (or `goog-define`) global defined by a
///   [`Constant`], or any global defined by a scalar or symbol literal (a
///   string of at most [`COPIED_STRING_LIMIT`] code units unless
///   `^:const`), by a read of a global with a value defined before it, or by
///   a call without arguments of a global with a return defined before it,
///   through `do` and `^boolean` hints;
/// - a return: a global defined by a `fn` of one method without parameters
///   (not `^:async`) whose body is such a literal, read or call; the body
///   runs when the function is called, after its definition;
/// - a collection: a `^:const` global defined by a collection literal.
///
/// A global tagged `^boolean` whose value or return would be other than
/// `true`, `false` or `nil` gets none: tests trust the tag and read
/// JavaScript truth, which substituting the value would change. A
/// `^:const` literal keeps its value whatever the tag, as before.
fn constant_globals_from_defs(
    program: &Program,
    top: &[ExprId],
    position: &[u32],
    definitions: Vec<Option<ExprId>>,
) -> ConstantGlobals {
    let count = program.globals.len();
    let mut known = ConstantGlobals {
        values: vec![None; count],
        collections: vec![None; count],
        returns: vec![None; count],
    };
    let mut order: Vec<(u32, usize, ExprId)> = Vec::new();
    for (index, definition) in definitions.into_iter().enumerate() {
        let Some(def) = definition else {
            continue;
        };
        let at = position[def.0];
        if top.get(at as usize) != Some(&def) {
            continue;
        }
        if let ExprKind::Def {
            init: Some(init),
            once: false,
            test: None,
            ..
        } = program.exprs[def.0].kind
        {
            order.push((at, index, init));
        }
    }
    order.sort_unstable_by_key(|&(at, index, _)| (at, index));
    for (at, index, init) in order {
        let binding = &program.globals[index];
        let init = crate::application::unwrap_do(program, init);
        let fits = |value: &ExprKind| {
            (!binding.boolean_tag || matches!(value, ExprKind::Bool(_) | ExprKind::Nil))
                && (binding.constant
                    || !matches!(value, ExprKind::String(string)
                        if program.strings[string.0].len() > COPIED_STRING_LIMIT))
        };
        if binding.constant && Constant::of(program, init).is_some() {
            known.values[index] = Some((at, program.exprs[init.0].kind.clone()));
        } else if let Some(value) = known_value(program, &known, init, at) {
            if fits(&value) {
                known.values[index] = Some((at, value));
            }
        } else if binding.constant && shape(program, init).is_some() {
            known.collections[index] = Some((at, init));
        } else if let ExprKind::Fn { methods, .. } = &program.exprs[init.0].kind
            && let [method] = &methods[..]
            && method.params.is_empty()
            && method.rest.is_none()
            && !method.asynchronous
            && let Some(value) = known_value(program, &known, method.body, at)
            && fits(&value)
        {
            known.returns[index] = Some((at, value));
        }
    }
    known
}

/// The literal of the scalar or symbol that `id` evaluates to when it runs
/// after top-level position `at`: a literal, or, through `do` and `^boolean`
/// hints, a read of a global with a value or a call without arguments of a
/// global with a return, defined before `at`. Through a hint, only a value
/// whose JavaScript truth is its Clojure truth.
fn known_value(
    program: &Program,
    known: &ConstantGlobals,
    id: ExprId,
    at: u32,
) -> Option<ExprKind> {
    let id = crate::application::unwrap_do(program, id);
    let found = |entry: &Option<(u32, ExprKind)>| match entry {
        Some((defined, value)) if *defined < at => Some(value.clone()),
        _ => None,
    };
    match &program.exprs[id.0].kind {
        // A hinted value whose JavaScript truth is not its Clojure truth
        // stays unknown: a function returning it has its calls tested by
        // JavaScript truth once inlined, as upstream infers its return tag.
        ExprKind::BooleanHint(inner) => known_value(program, known, *inner, at).filter(|value| {
            Constant::of_kind_view(program, value).is_some_and(|c| c.truth_is_js())
        }),
        ExprKind::Global(global) => found(&known.values[global.0]),
        ExprKind::Call { callee, args } if args.is_empty() => {
            found(&known.returns[called_global_view(program, *callee)?.0])
        }
        kind => Constant::of_kind_view(program, kind).map(|_| kind.clone()),
    }
}

/// The global a callee reads, through the `^boolean` hint a tagged
/// function's calls carry.
fn called_global_view<S: ExprSource>(src: &S, mut callee: ExprId) -> Option<GlobalId> {
    loop {
        match src.kind(callee) {
            ExprKind::BooleanHint(inner) => callee = *inner,
            ExprKind::Global(global) => return Some(*global),
            _ => return None,
        }
    }
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

/// Sparse delta returned by a read-only [`Folder`] traversal (serial or per
/// parallel slice). Applied sequentially to `&mut Program` in slice order
/// without cloning [`Program`].
struct FoldDelta {
    replacements: Vec<(ExprId, ExprKind)>,
    new_exprs: Vec<Expr>,
    new_strings: Vec<Vec<u16>>,
    built_updates: Vec<(ExprId, bool)>,
    folded: usize,
    saw_shared_unread_let: bool,
}

/// Rebase slice-local `ExprId >= base_exprs` and `StringId >= base_strings`
/// inside `kind` when applying parallel slice deltas in order.
fn rebase_expr_kind(
    kind: &mut ExprKind,
    base_exprs: usize,
    expr_offset: usize,
    base_strings: usize,
    string_offset: usize,
) {
    let re = |id: &mut ExprId| {
        if id.0 >= base_exprs {
            id.0 += expr_offset;
        }
    };
    let rs = |id: &mut StringId| {
        if id.0 >= base_strings {
            id.0 += string_offset;
        }
    };
    use ExprKind::*;
    match kind {
        Nil
        | Bool(_)
        | Number(_)
        | Keyword(_)
        | Symbol(_)
        | CoreFn(_)
        | Local(_)
        | Global(_)
        | JsGlobal(_)
        | JsArguments
        | LoadableType
        | DefProtocol { .. } => {}
        String(sid) => rs(sid),
        NamespaceView { namespace, value } => {
            rs(namespace);
            re(value);
        }
        Assert {
            test,
            message,
            form,
        } => {
            re(test);
            if let Some(m) = message {
                re(m);
            }
            rs(form);
        }
        Loadable { class: value, .. }
        | DefinitionTest { value, .. }
        | ThisAs { body: value, .. }
        | QualifiedMethod { class: value, .. }
        | Property {
            receiver: value, ..
        }
        | CopyArguments(value)
        | WithOutStr(value)
        | Exists(value)
        | BindingExists(value)
        | BooleanHint(value)
        | Throw(value)
        | Await(value) => re(value),
        ReaderLiteral { items, .. }
        | List(items)
        | Vector(items)
        | Map(items)
        | Set(items)
        | LiteralMap(items)
        | LiteralSet(items)
        | Do(items)
        | JsTemplate { args: items, .. }
        | Recur { args: items, .. } => {
            for item in items {
                re(item);
            }
        }
        WithMeta {
            value: a,
            metadata: b,
        }
        | JsDelete {
            receiver: a,
            key: b,
        }
        | ProtocolPredicate {
            protocol: a,
            value: b,
            ..
        }
        | SetGlobal {
            target: a,
            value: b,
        }
        | InstanceOf { class: a, value: b } => {
            re(a);
            re(b);
        }
        Var {
            value, metadata, ..
        } => {
            re(value);
            if let Some(m) = metadata {
                re(m);
            }
        }
        JsIndex {
            receiver,
            indices: args,
        }
        | MethodCall { receiver, args, .. }
        | Construct {
            callee: receiver,
            args,
        }
        | Call {
            callee: receiver,
            args,
        } => {
            re(receiver);
            for arg in args {
                re(arg);
            }
        }
        ExtendType {
            target, methods, ..
        } => {
            re(target);
            for (_, method) in methods {
                re(&mut method.body);
            }
        }
        Reify {
            metadata, methods, ..
        } => {
            if let Some(m) = metadata {
                re(m);
            }
            for (_, method) in methods {
                re(&mut method.body);
            }
        }
        DefType {
            layout, methods, ..
        } => {
            if let Some(b) = &mut layout.body {
                re(b);
            }
            if let Some(m) = &mut layout.masks {
                re(m);
            }
            for (_, method) in methods {
                re(&mut method.body);
            }
        }
        Def { init, test, .. } => {
            if let Some(i) = init {
                re(i);
            }
            if let Some(t) = test {
                re(t);
            }
        }
        Binding { bindings, body } => {
            for (target, value) in bindings {
                re(target);
                re(value);
            }
            re(body);
        }
        If { test, yes, no } => {
            re(test);
            re(yes);
            re(no);
        }
        Let { bindings, body } | LetFn { bindings, body } | Loop { bindings, body, .. } => {
            for (_, value) in bindings {
                re(value);
            }
            re(body);
        }
        Try {
            body,
            catches,
            finally,
        } => {
            re(body);
            for catch in catches {
                if let Some(c) = &mut catch.class {
                    re(c);
                }
                re(&mut catch.body);
            }
            if let Some(f) = finally {
                re(f);
            }
        }
        Case {
            test,
            branches,
            default,
            ..
        } => {
            re(test);
            for (t, v) in branches {
                re(t);
                re(v);
            }
            if let Some(d) = default {
                re(d);
            }
        }
        Fn { methods, .. } => {
            for method in methods {
                re(&mut method.body);
            }
        }
    }
}

fn apply_fold_delta(
    program: &mut Program,
    built: &mut Vec<bool>,
    mut delta: FoldDelta,
    base_exprs: usize,
    expr_offset: usize,
    base_strings: usize,
    string_offset: usize,
) {
    let needs_rebase = (expr_offset > 0 && !delta.new_exprs.is_empty())
        || (string_offset > 0 && !delta.new_strings.is_empty());
    for s in delta.new_strings {
        program.strings.push(s);
    }
    for mut expr in delta.new_exprs {
        if needs_rebase {
            rebase_expr_kind(
                &mut expr.kind,
                base_exprs,
                expr_offset,
                base_strings,
                string_offset,
            );
        }
        program.exprs.push(expr);
    }
    built.resize(program.exprs.len(), false);
    for (id, mut kind) in delta.replacements.drain(..) {
        if needs_rebase {
            rebase_expr_kind(
                &mut kind,
                base_exprs,
                expr_offset,
                base_strings,
                string_offset,
            );
        }
        program.exprs[id.0].kind = kind;
    }
    for (id, val) in delta.built_updates {
        if id.0 < built.len() {
            built[id.0] = val;
        }
    }
}

/// Shared read-only tables across all parallel [`Folder`] slices.
struct FoldContext<'p> {
    program: &'p Program,
    facts: &'p ProgramFacts,
    replaced: &'p [&'static str],
    position: &'p [u32],
    globals: &'p [Option<(u32, ExprKind)>],
    collections: &'p [Option<(u32, ExprId)>],
    returns: &'p [Option<(u32, ExprKind)>],
    pinned: &'p BitSet,
    assignable: &'p BitSet,
    reads: &'p [u32],
    kept: &'p BitSet,
    fenced: &'p BitSet,
    built: &'p [bool],
    parents: &'p [u8],
    pre_folded: &'p HashSet<ExprId>,
}

/// Read-only overlay combining immutable `&Program` with a slice's sparse
/// replacements, newly created expressions, and newly interned strings.
struct FolderView<'a> {
    program: &'a Program,
    base_exprs: usize,
    base_strings: usize,
    modified_bits: &'a BitSet,
    delta_map: &'a HashMap<ExprId, usize>,
    replacements: &'a [(ExprId, ExprKind)],
    new_exprs: &'a [Expr],
    new_strings: &'a [Vec<u16>],
}

impl ExprSource for FolderView<'_> {
    #[inline]
    fn kind(&self, id: ExprId) -> &ExprKind {
        if id.0 >= self.base_exprs {
            &self.new_exprs[id.0 - self.base_exprs].kind
        } else if self.modified_bits.get(id.0) {
            &self.replacements[self.delta_map[&id]].1
        } else {
            &self.program.exprs[id.0].kind
        }
    }

    #[inline]
    fn string(&self, id: StringId) -> &[u16] {
        if id.0 >= self.base_strings {
            &self.new_strings[id.0 - self.base_strings]
        } else {
            &self.program.strings[id.0]
        }
    }

    #[inline]
    fn named_literal(&self, id: NamedLiteralId) -> &NamedLiteral {
        &self.program.named_literals[id.0]
    }
}

/// Constant-pool eligibility classification matching
/// [`crate::emitter::constant_nodes`], over any [`ExprSource`] with a sparse
/// memoization map so parallel workers avoid allocating dense tables.
fn constant_nodes_view<S: ExprSource>(
    src: &S,
    replaced: &[&str],
    classes: &mut HashMap<ExprId, ConstantClass>,
    id: ExprId,
) -> Option<u32> {
    if let Some(&class) = classes.get(&id) {
        return match class {
            ConstantClass::Constant(nodes) => Some(nodes),
            ConstantClass::Variable => None,
            ConstantClass::Unknown => unreachable!(),
        };
    }
    let fixed = |name: &str| replaced.binary_search(&name).is_err();
    let scalar_constant = |kind: &ExprKind| {
        matches!(
            kind,
            ExprKind::Nil
                | ExprKind::Bool(_)
                | ExprKind::Number(_)
                | ExprKind::String(_)
                | ExprKind::Keyword(_)
                | ExprKind::Symbol(_)
        )
    };
    let kind = src.kind(id);
    let nodes = if scalar_constant(kind) {
        Some(1)
    } else {
        match kind {
            ExprKind::Vector(items) if items.is_empty() => fixed("PersistentVector").then_some(1),
            ExprKind::Map(items) if items.is_empty() => fixed("PersistentArrayMap").then_some(1),
            ExprKind::Set(items) if items.is_empty() => fixed("PersistentHashSet").then_some(1),
            ExprKind::List(items) if items.is_empty() => fixed("List").then_some(1),
            ExprKind::Vector(items)
            | ExprKind::List(items)
            | ExprKind::Map(items)
            | ExprKind::LiteralMap(items)
            | ExprKind::LiteralSet(items) => items.iter().try_fold(1u32, |nodes, &item| {
                Some(nodes.saturating_add(constant_nodes_view(src, replaced, classes, item)?))
            }),
            ExprKind::Set(items) => {
                if items.iter().all(|item| scalar_constant(src.kind(*item))) {
                    items.iter().try_fold(1u32, |nodes, &item| {
                        Some(
                            nodes
                                .saturating_add(constant_nodes_view(src, replaced, classes, item)?),
                        )
                    })
                } else {
                    None
                }
            }
            ExprKind::WithMeta { value, metadata } => {
                let value_kind = src.kind(*value);
                if !fixed("withMeta")
                    || matches!(
                        value_kind,
                        ExprKind::ReaderLiteral {
                            kind: CompilerLiteralKind::JsArray | CompilerLiteralKind::JsObject,
                            quoted: true,
                            ..
                        }
                    )
                    || !matches!(
                        value_kind,
                        ExprKind::Vector(_)
                            | ExprKind::List(_)
                            | ExprKind::Map(_)
                            | ExprKind::Set(_)
                            | ExprKind::LiteralMap(_)
                            | ExprKind::LiteralSet(_)
                            | ExprKind::Symbol(_)
                            | ExprKind::WithMeta { .. }
                    )
                {
                    None
                } else {
                    let v = constant_nodes_view(src, replaced, classes, *value)?;
                    let m = constant_nodes_view(src, replaced, classes, *metadata)?;
                    Some(1u32.saturating_add(v).saturating_add(m))
                }
            }
            ExprKind::ReaderLiteral {
                kind: CompilerLiteralKind::Uuid,
                items,
                ..
            } => {
                if !fixed("uuid") {
                    None
                } else {
                    items.iter().try_fold(1u32, |nodes, &item| {
                        Some(
                            nodes
                                .saturating_add(constant_nodes_view(src, replaced, classes, item)?),
                        )
                    })
                }
            }
            _ => None,
        }
    };
    classes.insert(
        id,
        nodes.map_or(ConstantClass::Variable, ConstantClass::Constant),
    );
    nodes
}

struct Folder<'p> {
    ctx: &'p FoldContext<'p>,
    base_exprs: usize,
    base_strings: usize,
    visited: BitSet,
    modified_bits: BitSet,
    delta_map: HashMap<ExprId, usize>,
    replacements: Vec<(ExprId, ExprKind)>,
    new_exprs: Vec<Expr>,
    new_built: Vec<bool>,
    new_positions: Vec<u32>,
    new_strings: Vec<Vec<u16>>,
    built_overrides: HashMap<ExprId, bool>,
    /// Lexically scoped constant/alias substitutions for active `Let` bindings.
    locals: Vec<(LocalId, ExprKind)>,
    /// Lexically scoped locals bound to truthy literals (`fn` / collections).
    truths: Vec<LocalId>,
    /// Lexically scoped locals bound to collections or chains of such locals.
    bound: Vec<(LocalId, ExprId)>,
    /// Sparse constant-pool classification memoization.
    classes: HashMap<ExprId, ConstantClass>,
    folded: usize,
    /// Whether `rewrite` deferred dropping an unread non-pinned `let` binding
    /// (shared body or skipped subtree), so `sweep` must still run even if
    /// `folded == 0`.
    saw_shared_unread_let: bool,
}

impl<'p> Folder<'p> {
    fn new(ctx: &'p FoldContext<'p>) -> Self {
        let count = ctx.program.exprs.len();
        Self::with_visited(ctx, BitSet::new(count))
    }

    fn with_visited(ctx: &'p FoldContext<'p>, visited: BitSet) -> Self {
        let count = ctx.program.exprs.len();
        Self {
            ctx,
            base_exprs: count,
            base_strings: ctx.program.strings.len(),
            visited,
            modified_bits: BitSet::new(count),
            delta_map: HashMap::new(),
            replacements: Vec::new(),
            new_exprs: Vec::new(),
            new_built: Vec::new(),
            new_positions: Vec::new(),
            new_strings: Vec::new(),
            built_overrides: HashMap::new(),
            locals: Vec::new(),
            truths: Vec::new(),
            bound: Vec::new(),
            classes: HashMap::new(),
            folded: 0,
            saw_shared_unread_let: false,
        }
    }

    fn into_delta(self) -> FoldDelta {
        FoldDelta {
            replacements: self.replacements,
            new_exprs: self.new_exprs,
            new_strings: self.new_strings,
            built_updates: self.built_overrides.into_iter().collect(),
            folded: self.folded,
            saw_shared_unread_let: self.saw_shared_unread_let,
        }
    }

    fn into_delta_and_visited(self) -> (FoldDelta, BitSet) {
        let visited = self.visited;
        let delta = FoldDelta {
            replacements: self.replacements,
            new_exprs: self.new_exprs,
            new_strings: self.new_strings,
            built_updates: self.built_overrides.into_iter().collect(),
            folded: self.folded,
            saw_shared_unread_let: self.saw_shared_unread_let,
        };
        (delta, visited)
    }

    #[inline]
    fn view(&self) -> FolderView<'_> {
        FolderView {
            program: self.ctx.program,
            base_exprs: self.base_exprs,
            base_strings: self.base_strings,
            modified_bits: &self.modified_bits,
            delta_map: &self.delta_map,
            replacements: &self.replacements,
            new_exprs: &self.new_exprs,
            new_strings: &self.new_strings,
        }
    }

    #[inline]
    fn kind(&self, id: ExprId) -> &ExprKind {
        if id.0 >= self.base_exprs {
            &self.new_exprs[id.0 - self.base_exprs].kind
        } else if self.modified_bits.get(id.0) {
            &self.replacements[self.delta_map[&id]].1
        } else {
            &self.ctx.program.exprs[id.0].kind
        }
    }

    #[inline]
    fn position_of(&self, id: ExprId) -> u32 {
        if id.0 >= self.base_exprs {
            self.new_positions[id.0 - self.base_exprs]
        } else {
            self.ctx.position.get(id.0).copied().unwrap_or(NOWHERE)
        }
    }

    #[inline]
    fn built_of(&self, id: ExprId) -> bool {
        if id.0 >= self.base_exprs {
            self.new_built[id.0 - self.base_exprs]
        } else if let Some(&val) = self.built_overrides.get(&id) {
            val
        } else {
            self.ctx.built.get(id.0).copied().unwrap_or(false)
        }
    }

    #[inline]
    fn set_built(&mut self, id: ExprId, val: bool) {
        if id.0 >= self.base_exprs {
            self.new_built[id.0 - self.base_exprs] = val;
        } else if self.built_of(id) != val {
            self.built_overrides.insert(id, val);
        }
    }

    #[inline]
    fn set_kind(&mut self, id: ExprId, kind: ExprKind) {
        self.classes.remove(&id);
        if id.0 >= self.base_exprs {
            self.new_exprs[id.0 - self.base_exprs].kind = kind;
        } else if self.modified_bits.get(id.0) {
            let slot = self.delta_map[&id];
            self.replacements[slot].1 = kind;
        } else {
            let slot = self.replacements.len();
            self.replacements.push((id, kind));
            self.delta_map.insert(id, slot);
            self.modified_bits.set(id.0);
        }
    }

    fn constant_to_kind(&mut self, constant: Constant) -> ExprKind {
        match constant {
            Constant::Nil => ExprKind::Nil,
            Constant::Bool(value) => ExprKind::Bool(value),
            Constant::Number(value) => ExprKind::Number(value),
            Constant::String(units) => {
                let id = StringId(self.base_strings + self.new_strings.len());
                self.new_strings.push(units);
                ExprKind::String(id)
            }
            Constant::Keyword(literal) => ExprKind::Keyword(literal),
            Constant::Symbol(literal) => ExprKind::Symbol(literal),
            Constant::Collection(_) => unreachable!("evaluation returns scalars"),
        }
    }

    #[inline]
    fn local_value(&self, local: LocalId) -> Option<&ExprKind> {
        if self.locals.is_empty() {
            return None;
        }
        self.locals
            .iter()
            .rev()
            .find(|(l, _)| *l == local)
            .map(|(_, kind)| kind)
    }

    #[inline]
    fn bound_init(&self, local: LocalId) -> Option<ExprId> {
        if self.bound.is_empty() {
            return None;
        }
        self.bound
            .iter()
            .rev()
            .find(|(l, _)| *l == local)
            .map(|(_, init)| *init)
    }

    #[inline]
    fn is_truthy_local(&self, local: LocalId) -> bool {
        !self.truths.is_empty() && self.truths.contains(&local)
    }

    fn is_bound_candidate(&self, init: ExprId) -> bool {
        let view = self.view();
        match view.kind(init) {
            ExprKind::Local(other) => self.bound_init(*other).is_some(),
            ExprKind::Global(global) => self.ctx.collections[global.0].is_some(),
            _ => shape_view(&view, init).is_some(),
        }
    }

    /// Check skipped subtrees (e.g., `Case` branch test operands) for any
    /// unread non-pinned effect-free `let` binding when nothing has folded yet,
    /// so `sweep` still runs when needed.
    fn check_skipped_unread_let(&mut self, root: ExprId) {
        if self.folded > 0 || self.saw_shared_unread_let {
            return;
        }
        let mut pending = vec![root];
        let view = self.view();
        while let Some(id) = pending.pop() {
            if id.0 < self.base_exprs && self.visited.get(id.0) {
                continue;
            }
            let kind = view.kind(id);
            if let ExprKind::Let { bindings, .. } = kind
                && bindings.iter().any(|(local, init)| {
                    self.ctx.reads[local.0] == 0
                        && !self.ctx.pinned.get(local.0)
                        && effect_free_view(&view, view.kind(*init))
                })
            {
                self.saw_shared_unread_let = true;
                return;
            }
            visit_children(kind, |child| pending.push(child));
        }
    }

    fn visit(&mut self, id: ExprId, discarded: bool) {
        if id.0 >= self.base_exprs || self.visited.set(id.0) {
            return;
        }
        if !self.ctx.pre_folded.is_empty() && self.ctx.pre_folded.contains(&id) {
            return;
        }
        // Read directly from immutable `&'p Program` with zero `mem::replace`
        // churn on untouched nodes.
        let kind: &'p ExprKind = &self.ctx.program.exprs[id.0].kind;
        match kind {
            ExprKind::Global(global) => {
                if let Some((at, value)) = &self.ctx.globals[global.0]
                    && !self.ctx.fenced.get(id.0)
                    && self.ctx.position[id.0] != NOWHERE
                    && self.ctx.position[id.0] > *at
                {
                    self.folded += 1;
                    self.set_kind(id, value.clone());
                }
            }
            ExprKind::Local(local) => {
                if let Some(value) = self.local_value(*local) {
                    let value = value.clone();
                    self.folded += 1;
                    self.set_kind(id, value);
                }
            }
            ExprKind::Let { bindings, body } => {
                let body = *body;
                // Substitution rewrites the reads while it visits the body, so
                // a body another parent already visited keeps its bindings.
                let shared = body.0 < self.base_exprs && self.visited.get(body.0);
                let saved_locals = self.locals.len();
                let saved_truths = self.truths.len();
                let saved_bound = self.bound.len();
                let mut kept: Option<Vec<(LocalId, ExprId)>> = None;
                for (b_idx, &(local, init)) in bindings.iter().enumerate() {
                    self.visit(init, false);
                    // Lookups through the local may fold against the value it
                    // is bound to.
                    if !shared && !self.ctx.assignable.get(local.0) && self.is_bound_candidate(init)
                    {
                        self.bound.push((local, init));
                    }
                    // Another local bound to a collection literal whose value
                    // cannot change: the aliases a destructuring binds, such
                    // as its `:as` local. Aliases of other values stay, since
                    // passes after this one read `and` and `or` from the
                    // local their expansions bind.
                    let alias = !self.ctx.pinned.get(local.0)
                        && !shared
                        && matches!(self.kind(init), ExprKind::Local(other) if !self.ctx.assignable.get(other.0))
                        && (self.collection(init).is_some() || self.keyed(init).is_some());
                    let mut keep_binding = false;
                    if self.ctx.pinned.get(local.0) || shared {
                        if shared
                            && !self.ctx.pinned.get(local.0)
                            && self.ctx.reads[local.0] == 0
                            && effect_free_view(&self.view(), self.kind(init))
                        {
                            self.saw_shared_unread_let = true;
                        }
                        keep_binding = true;
                    } else if Constant::of_kind_view(&self.view(), self.kind(init)).is_some()
                        || alias
                    {
                        // A constant or such an alias is read in its place.
                        let val = self.kind(init).clone();
                        self.locals.push((local, val));
                        self.folded += 1;
                    } else if self.ctx.reads[local.0] == 0
                        && effect_free_view(&self.view(), self.kind(init))
                    {
                        self.folded += 1;
                    } else {
                        // A literal collection or `fn` is true wherever the
                        // local is tested.
                        if literal_truth_view(&self.view(), self.kind(init)) == Some(true) {
                            self.truths.push(local);
                        }
                        keep_binding = true;
                    }
                    if keep_binding {
                        if let Some(ref mut k) = kept {
                            k.push((local, init));
                        }
                    } else if kept.is_none() {
                        let mut k = Vec::with_capacity(bindings.len() - 1);
                        k.extend_from_slice(&bindings[..b_idx]);
                        kept = Some(k);
                    }
                }
                let body_discarded = discarded && self.ctx.parents[id.0] <= 1;
                self.visit(body, body_discarded);
                self.locals.truncate(saved_locals);
                self.truths.truncate(saved_truths);
                self.bound.truncate(saved_bound);
                let kept_empty = match &kept {
                    Some(k) => k.is_empty(),
                    None => bindings.is_empty(),
                };
                if kept_empty
                    && !scoped_view(&self.view(), body)
                    && let Some(new_kind) = self.adopt(id, body)
                {
                    self.set_kind(id, new_kind);
                    return;
                }
                if let Some(kept) = kept {
                    self.set_kind(
                        id,
                        ExprKind::Let {
                            bindings: kept,
                            body,
                        },
                    );
                }
            }
            ExprKind::If { test, yes, no } => {
                let (test, yes, no) = (*test, *yes, *no);
                self.visit(test, false);
                let branch_discarded = discarded && self.ctx.parents[id.0] <= 1;
                let truth = match self.kind(test) {
                    ExprKind::Local(local) if self.is_truthy_local(*local) => Some(true),
                    test_kind => literal_truth_view(&self.view(), test_kind),
                };
                if let Some(truth) = truth {
                    let branch = if truth { yes } else { no };
                    self.visit(branch, branch_discarded);
                    if let Some(new_kind) = self.adopt(id, branch) {
                        self.set_kind(id, new_kind);
                        return;
                    }
                }
                self.visit(yes, branch_discarded);
                self.visit(no, branch_discarded);
            }
            ExprKind::BooleanHint(inner) => {
                let inner = *inner;
                self.visit(inner, false);
                // A hinted test reads JavaScript truth, so the hint stays on
                // a constant whose JavaScript truth is not its Clojure truth
                // (`0`, `NaN`, `""`).
                if self
                    .constant(inner)
                    .is_some_and(|value| value.truth_is_js())
                    && let Some(new_kind) = self.adopt(id, inner)
                {
                    self.set_kind(id, new_kind);
                }
            }
            ExprKind::Do(items) => {
                // Forms before the last are discarded, and so is the last one
                // of a discarded `do` that nothing else reads.
                let do_discarded = discarded && self.ctx.parents[id.0] <= 1;
                let last = items.len().saturating_sub(1);
                for (index, &item) in items.iter().enumerate() {
                    self.visit(item, do_discarded || index != last);
                }
                if self.ctx.kept.get(id.0) || items.is_empty() {
                    return;
                }
                let mut forms: Option<Vec<ExprId>> = None;
                for (index, &item) in items.iter().enumerate() {
                    let keep_item = (index == last && !do_discarded)
                        || self.ctx.kept.get(item.0)
                        || !effect_free_view(&self.view(), self.kind(item));
                    if keep_item {
                        if let Some(ref mut f) = forms {
                            f.push(item);
                        }
                    } else if forms.is_none() {
                        let mut f = Vec::with_capacity(items.len() - 1);
                        f.extend_from_slice(&items[..index]);
                        forms = Some(f);
                    }
                }
                let kept_slice = forms.as_deref().unwrap_or(items);
                self.folded += items.len() - kept_slice.len();
                if kept_slice.is_empty() {
                    self.set_kind(id, ExprKind::Nil);
                    return;
                }
                if let [form] = kept_slice
                    && !scoped_view(&self.view(), *form)
                    && let Some(new_kind) = self.adopt(id, *form)
                {
                    self.set_kind(id, new_kind);
                    return;
                }
                if let Some(forms) = forms {
                    self.set_kind(id, ExprKind::Do(forms));
                }
            }
            ExprKind::Call { callee, args } => {
                let callee = *callee;
                self.visit(callee, false);
                for &arg in args {
                    self.visit(arg, false);
                }
                if let ExprKind::CoreFn(core) = *self.kind(callee) {
                    if !self.ctx.fenced.get(id.0)
                        && let Some(value) = self.call(core, args)
                    {
                        self.folded += 1;
                        let new_kind = self.constant_to_kind(value);
                        self.set_kind(id, new_kind);
                        return;
                    }
                    if !self.ctx.fenced.get(id.0)
                        && let Some(new_kind) = self.select(id, core, args)
                    {
                        self.folded += 1;
                        self.set_kind(id, new_kind);
                        return;
                    }
                    if let Some(new_args) = self.prefix(core, args) {
                        self.folded += 1;
                        self.set_kind(
                            id,
                            ExprKind::Call {
                                callee,
                                args: new_args,
                            },
                        );
                        return;
                    }
                } else if args.is_empty()
                    && !self.ctx.fenced.get(id.0)
                    && !self.ctx.kept.get(id.0)
                    && let Some(global) = called_global_view(&self.view(), callee)
                    && let Some((at, value)) = &self.ctx.returns[global.0]
                    && self.ctx.position[id.0] != NOWHERE
                    && self.ctx.position[id.0] > *at
                {
                    // A function that returns a constant, called after its
                    // definition, is that constant.
                    self.folded += 1;
                    self.set_kind(id, value.clone());
                    return;
                } else if !self.ctx.fenced.get(id.0)
                    && let Some(new_kind) = self.invoke(id, callee, args)
                {
                    self.folded += 1;
                    self.set_kind(id, new_kind);
                    return;
                }
                self.member_set(callee, args);
            }
            // These read a global's binding rather than its value.
            ExprKind::Var {
                value, metadata, ..
            } => {
                self.check_skipped_unread_let(*value);
                if let Some(m) = metadata {
                    self.check_skipped_unread_let(*m);
                }
            }
            ExprKind::Exists(inner) | ExprKind::BindingExists(inner) => {
                self.check_skipped_unread_let(*inner);
            }
            // Assignment targets are not reads.
            ExprKind::SetGlobal { target, value } => {
                let (target, value) = (*target, *value);
                if !matches!(
                    self.kind(target),
                    ExprKind::Global(_) | ExprKind::Local(_) | ExprKind::CoreFn(_)
                ) {
                    self.visit(target, false);
                }
                self.visit(value, false);
            }
            ExprKind::Binding { bindings, body } => {
                for &(target, init) in bindings {
                    self.check_skipped_unread_let(target);
                    self.visit(init, false);
                }
                self.visit(*body, false);
            }
            // Case tests are the constants it dispatches on, as analyzed.
            ExprKind::Case {
                test,
                branches,
                default,
                ..
            } => {
                self.visit(*test, false);
                for &(branch_test, branch) in branches {
                    self.check_skipped_unread_let(branch_test);
                    self.visit(branch, false);
                }
                if let Some(default) = default {
                    self.visit(*default, false);
                }
            }
            _ => {
                visit_children(kind, |child| self.visit(child, false));
            }
        }
    }

    /// The constant `id` evaluates to, once visited.
    fn constant(&self, id: ExprId) -> Option<Constant> {
        Constant::of_view(&self.view(), id)
    }

    /// The kind of `child`, for its parent to take over, unless the child's
    /// identity matters.
    fn adopt(&mut self, parent: ExprId, child: ExprId) -> Option<ExprKind> {
        if (child.0 < self.base_exprs && self.ctx.kept.get(child.0))
            || (parent.0 < self.base_exprs
                && self.ctx.fenced.get(parent.0)
                && constant_like_view(&self.view(), self.kind(child)))
        {
            return None;
        }
        let child_built = self.built_of(child);
        self.set_built(parent, child_built);
        self.folded += 1;
        Some(self.kind(child).clone())
    }

    /// The arguments of a call of `+`, `-`, `*` or `/` whose leading number
    /// literals are folded into one: the runtime folds its operands from the
    /// left (`numericFold`) and JavaScript's operators associate to the left,
    /// so `(+ 1 2 x)` is `(+ 3 x)` whatever `x` is.
    fn prefix(&mut self, core: CoreFnId, args: &[ExprId]) -> Option<Vec<ExprId>> {
        if !matches!(
            core,
            CoreFnId::Add | CoreFnId::Subtract | CoreFnId::Multiply | CoreFnId::Divide
        ) || self.ctx.facts.replaced(core)
        {
            return None;
        }
        let numbers: Vec<Constant> = args
            .iter()
            .map_while(|arg| match *self.kind(*arg) {
                ExprKind::Number(value) => Some(Constant::Number(value)),
                _ => None,
            })
            .collect();
        if numbers.len() < 2 || numbers.len() == args.len() {
            return None;
        }
        let value = evaluate_view(&self.view(), core, &numbers)?;
        let kind = self.constant_to_kind(value);
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
            && !self.ctx.facts.core_protocols_intact()
        {
            return None;
        }
        let view = self.view();
        let types: Vec<Type> = constants
            .iter()
            .map(|constant| constant.ty_view(&view))
            .collect();
        let call = core.operation().call(&types);
        if call.purity != Purity::Pure
            || !self.ctx.facts.trusted(core, call.calls)
            || self.equiv_written(core)
        {
            return None;
        }
        evaluate_view(&view, core, &constants)
    }

    /// Whether `core` is `=` or `not=` in a program that writes `IEquiv`'s
    /// method by name, which may have given a constant, a keyword literal
    /// too, a method that `=` calls at every level (light-cljs-1061): no
    /// fold reads it.
    fn equiv_written(&self, core: CoreFnId) -> bool {
        matches!(core, CoreFnId::Equal | CoreFnId::NotEqual) && self.ctx.facts.equiv_by_name()
    }

    /// In a test of membership in a set, a call `(s x)`, `(contains? s x)` or
    /// `(get s x)`, make a local or `^:const` global `s` bound to a constant
    /// set literal whose [`members`] emission compares that literal (a copy,
    /// which the pool declares as the same value), so that emission compares
    /// the tested value with the members instead of looking it up (the
    /// emitter's `membership`, which states the conditions checked here), in
    /// an `if` test and in value position alike (light-cljs-231).
    fn member_set(&mut self, callee: ExprId, args: &[ExprId]) {
        if !self.ctx.facts.core_protocols_intact() || self.ctx.facts.implements_equiv() {
            return;
        }
        let operand = match (self.kind(callee), args) {
            (ExprKind::CoreFn(core @ (CoreFnId::Contains | CoreFnId::Get)), [set, _])
                if !self.ctx.facts.replaced(*core) =>
            {
                *set
            }
            (_, [_]) => callee,
            _ => return,
        };
        if !matches!(self.kind(operand), ExprKind::Local(_) | ExprKind::Global(_)) {
            return;
        }
        let Some(set) = self.collection(operand) else {
            return;
        };
        let is_comparable = match self.kind(set) {
            ExprKind::Set(members) | ExprKind::LiteralSet(members) => {
                members_view(&self.view(), members).is_some()
            }
            _ => false,
        };
        if !is_comparable {
            return;
        }
        if let Some(kind) = self.copy(set) {
            self.set_kind(operand, kind);
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
        ) || !self.ctx.facts.core_protocols_intact()
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
        let types: Vec<Type> = {
            let view = self.view();
            values
                .iter()
                .enumerate()
                .map(|(index, value)| match value {
                    Some(value) => value.ty_view(&view),
                    None if index == 0 && keyed.is_some() => Type::MAP,
                    None => Type::ANY,
                })
                .collect()
        };
        let call = core.operation().call(&types);
        if call.purity == Purity::Effect || !self.ctx.facts.trusted(core, call.calls) {
            return None;
        }
        let folded = match keyed {
            Some(map) => {
                let (key, rest) = match core {
                    CoreFnId::Get => (values[1].clone()?, Vec::new()),
                    _ => {
                        let view = self.view();
                        match path_keys_view(&view, values[1].as_ref()?)? {
                            [] => return self.write(id, Folded::Argument(0), args, None),
                            [key, rest @ ..] => (Constant::of_view(&view, *key)?, rest.to_vec()),
                        }
                    }
                };
                self.keyed_walk(args[0], map, &key, &rest, (args.len() == 3).then_some(2))?
            }
            None => select_view(&self.view(), core, &values)?,
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
        if !self.ctx.facts.core_protocols_intact() || self.ctx.facts.replaced(CoreFnId::Get) {
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
        let named = match *self.kind(callee) {
            ExprKind::Keyword(literal) => Some(Constant::Keyword(literal)),
            ExprKind::Symbol(literal) => Some(Constant::Symbol(literal)),
            _ => None,
        };
        let folded = if let Some(named) = named {
            match &values[0] {
                Some(value) => found(get_view(&self.view(), value, &named)?),
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
            let view = self.view();
            match shape_view(&view, collection)?.0 {
                Shape::Vector => match lookup_view(&view, collection, key)? {
                    Lookup::Found(item) if matches!(key, Constant::Number(_)) => {
                        if defaulted && !effect_free_view(&view, view.kind(args[1])) {
                            return None;
                        }
                        // The ignored argument has no effect, so drop it.
                        return self.write(id, Folded::Item(item), args, None);
                    }
                    _ => return None,
                },
                Shape::List => return None,
                Shape::Map { .. } | Shape::Set { .. } => {
                    found(lookup_view(&view, collection, key)?)
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
        let view = self.view();
        for _ in 0..self.bound.len() + 2 {
            match view.kind(id) {
                ExprKind::Local(local) => id = self.bound_init(*local)?,
                ExprKind::Map(items) => {
                    let keys = || items.iter().step_by(2).copied();
                    return (keys().all(|key| Constant::item_view(&view, key).is_some())
                        && distinct_view(&view, keys()))
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
        match lookup_view(&self.view(), map, key)? {
            Lookup::Found(value) if self.copyable(value) => {
                if !self.dropped(receiver, map, None) {
                    return None;
                }
                let view = self.view();
                let current = Constant::item_view(&view, value)?;
                walk_view(&view, current, Some(value), rest, default.is_some())
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
                    && matches!(self.kind(value),
                        ExprKind::Local(local) if !self.ctx.assignable.get(local.0))
                    && self.uncaptured(receiver) =>
            {
                Some(Folded::Expression(value))
            }
            Lookup::Found(_) => None,
            _ if !self.dropped(receiver, map, None) => None,
            Lookup::Missing => match default {
                Some(index) => Some(Folded::Argument(index)),
                None => walk_view(&self.view(), Constant::Nil, None, rest, false),
            },
        }
    }

    /// Whether no closure, type or `js*` hole reads the locals from
    /// `receiver` along the chain of locals [`Folder::keyed`] followed.
    fn uncaptured(&self, mut id: ExprId) -> bool {
        for _ in 0..self.bound.len() + 2 {
            let ExprKind::Local(local) = *self.kind(id) else {
                return true;
            };
            match self.bound_init(local) {
                Some(init) if !self.ctx.pinned.get(local.0) => id = init,
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
        let view = self.view();
        let ExprKind::Map(items) = view.kind(map) else {
            return false;
        };
        items
            .iter()
            .filter(|item| Some(**item) != kept)
            .all(|item| effect_free_view(&view, view.kind(*item)))
    }

    /// A `pure` call on a map bound to a local whose keys are constants
    /// ([`Folder::keyed`]), whatever its values, and constants: [`evaluate`]
    /// answers `count`, `empty?`, `contains?` and the type predicates from
    /// the literal's shape and keys alone (`(seq? m)` of a map destructuring).
    fn keyed_call(&mut self, core: CoreFnId, args: &[ExprId]) -> Option<Constant> {
        if !self.ctx.facts.core_protocols_intact() {
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
        let view = self.view();
        let types: Vec<Type> = constants
            .iter()
            .map(|constant| constant.ty_view(&view))
            .collect();
        let call = core.operation().call(&types);
        if call.purity != Purity::Pure
            || !self.ctx.facts.trusted(core, call.calls)
            || self.equiv_written(core)
        {
            return None;
        }
        evaluate_view(&view, core, &constants)
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
                    Some(index) if !effect_free_view(&self.view(), self.kind(args[index])) => None,
                    _ => self.adopt(id, value),
                };
            }
            _ => (),
        }
        let kind = match folded {
            Folded::Value(value) => self.constant_to_kind(value),
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
            Some(index) if !effect_free_view(&self.view(), self.kind(args[index])) => {
                let value = self.push(id, kind);
                Some(ExprKind::Do(vec![args[index], value]))
            }
            _ => Some(kind),
        }
    }

    /// The constant `id` evaluates to: a scalar or symbol literal, or a
    /// constant collection ([`Folder::collection`]).
    fn value(&mut self, id: ExprId) -> Option<Constant> {
        Constant::of_view(&self.view(), id)
            .or_else(|| self.collection(id).map(Constant::Collection))
    }

    /// The constant collection literal that `id` evaluates to: a literal the
    /// constant pool may hold, or one that a `let` binds a local to, or a
    /// `^:const` global read after its definition, through any chain of
    /// locals bound to such reads.
    fn collection(&mut self, mut id: ExprId) -> Option<ExprId> {
        // Locals bound to locals form chains as long as the nesting of `let`s.
        for _ in 0..self.bound.len() + 3 {
            match *self.kind(id) {
                ExprKind::Local(local) => id = self.bound_init(local)?,
                ExprKind::Global(global) => {
                    // Read in a later top-level form, after the definition.
                    let (at, literal) = self.ctx.collections[global.0]?;
                    let read = self.position_of(id);
                    if read == NOWHERE || read <= at {
                        return None;
                    }
                    id = literal;
                }
                _ => {
                    shape_view(&self.view(), id)?;
                    return self.constant_class(id).then_some(id);
                }
            }
        }
        None
    }

    /// Whether the constant pool may hold `id` (the emitter's
    /// `constant_nodes`): a constant that cannot throw or change when built.
    fn constant_class(&mut self, id: ExprId) -> bool {
        let view = FolderView {
            program: self.ctx.program,
            base_exprs: self.base_exprs,
            base_strings: self.base_strings,
            modified_bits: &self.modified_bits,
            delta_map: &self.delta_map,
            replacements: &self.replacements,
            new_exprs: &self.new_exprs,
            new_strings: &self.new_strings,
        };
        constant_nodes_view(&view, self.ctx.replaced, &mut self.classes, id).is_some()
    }

    /// Whether the item `id` of a constant collection may be copied to
    /// another place with the same value and identity: a scalar or symbol,
    /// or a collection of at most [`COPY_LIMIT`] nodes that no evaluation
    /// builds, so that the pool declares the copy as the same entry as the
    /// item (hash-consing on structure). A literal marked built (see
    /// [`built_literals`]) would be a fresh value at each place.
    fn copyable(&mut self, id: ExprId) -> bool {
        if Constant::of_view(&self.view(), id).is_some() {
            return true;
        }
        if !self.constant_class(id) {
            return false;
        }
        let mut pending = vec![id];
        let mut nodes = 0;
        while let Some(node) = pending.pop() {
            nodes += 1;
            if nodes > COPY_LIMIT || self.built_of(node) {
                return false;
            }
            visit_children(self.kind(node), |child| pending.push(child));
        }
        true
    }

    /// The kind of a copy of the constant item `id`, whose nested items are
    /// new expressions, when it is [`Folder::copyable`].
    fn copy(&mut self, id: ExprId) -> Option<ExprKind> {
        if !self.copyable(id) {
            return None;
        }
        let mut kind = self.kind(id).clone();
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

    /// A new expression of `kind` where `like` is, already visited: stored
    /// in the slice's local `new_exprs` buffer and rebased on merge.
    fn push(&mut self, like: ExprId, kind: ExprKind) -> ExprId {
        let template = if like.0 >= self.base_exprs {
            &self.new_exprs[like.0 - self.base_exprs]
        } else {
            &self.ctx.program.exprs[like.0]
        };
        let expr = Expr {
            kind,
            span: template.span.clone(),
            context: template.context,
            source_index: template.source_index,
        };
        let pos = self.position_of(like);
        let id = ExprId(self.base_exprs + self.new_exprs.len());
        self.new_exprs.push(expr);
        self.new_built.push(false);
        self.new_positions.push(pos);
        id
    }
}

/// The most nodes of a constant collection item that a fold copies to the
/// call's place (rule 4 of docs/dos-and-donts.md). The pool declares the
/// copy as the entry the item already is, so emitted code does not grow.
const COPY_LIMIT: usize = 256;

/// The most passes [`sweep_with_slots`] makes over the program.
const SWEEPS: usize = 4;

/// Whether `id` depends on the function scope emission gives it: outside
/// nested functions it reads `arguments`, holds `js*` raw text (which may read
/// `this` or `arguments`) or is a definition with `:test` metadata. A `let` or
/// `do` in value position is emitted as a function call, so such contents
/// keep the form around them (the emitter's `sequence_keeps_scope` asks the
/// same).
pub(crate) fn scoped(program: &Program, id: ExprId) -> bool {
    scoped_view(program, id)
}

fn scoped_view<S: ExprSource>(src: &S, id: ExprId) -> bool {
    let mut pending = vec![id];
    while let Some(id) = pending.pop() {
        match src.kind(id) {
            ExprKind::JsArguments
            | ExprKind::CopyArguments(_)
            | ExprKind::JsTemplate { .. }
            | ExprKind::Def { test: Some(_), .. } => return true,
            ExprKind::Fn { .. } => (),
            kind => visit_children(kind, |child| pending.push(child)),
        }
    }
    false
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
    constant_like_view(program, kind)
}

fn constant_like_view<S: ExprSource>(src: &S, kind: &ExprKind) -> bool {
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
            .all(|item| constant_like_view(src, src.kind(*item))),
        ExprKind::WithMeta { value, metadata } => [value, metadata]
            .iter()
            .all(|item| constant_like_view(src, src.kind(**item))),
        _ => false,
    }
}

/// Whether an `if` on an expression of this kind takes its first branch,
/// when evaluating it has no effect and the answer is known: the truth of a
/// constant, and `true` for quoted symbols, `fn` and collection literals
/// without effects, which are never `nil` or `false`.
fn literal_truth_view<S: ExprSource>(src: &S, kind: &ExprKind) -> Option<bool> {
    if let Some(constant) = Constant::of_kind_view(src, kind) {
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
        | ExprKind::LiteralSet(_) => effect_free_view(src, kind).then_some(true),
        _ => None,
    }
}

/// Whether evaluating an expression of this kind has no effect and cannot
/// throw: literals, reads of locals, globals and core values, `fn`, and
/// vector and list literals of such expressions, and map and set literals
/// whose keys and members are scalar literals (analysis rejects equal literal
/// keys, which would throw) and whose values are such expressions.
pub(crate) fn effect_free(program: &Program, kind: &ExprKind) -> bool {
    effect_free_view(program, kind)
}

fn effect_free_view<S: ExprSource>(src: &S, kind: &ExprKind) -> bool {
    let scalar = |id: &ExprId| {
        matches!(
            src.kind(*id),
            ExprKind::Nil
                | ExprKind::Bool(_)
                | ExprKind::Number(_)
                | ExprKind::String(_)
                | ExprKind::Keyword(_)
                | ExprKind::Symbol(_)
        )
    };
    let free = |id: &ExprId| effect_free_view(src, src.kind(*id));
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
        Constant, Folded, Lookup, built_literals, count, flatten, fold_with_facts, js_number,
        lookup, same_key, select, shape,
    };
    use crate::analyzer::{CoreFnId, ExprId, ExprKind, Program};
    use crate::inference::ProgramFacts;

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

    #[test]
    fn sweep_runs_when_shared_let_defers_unread_binding() {
        // Build a program where a `Let` has an unread non-pinned effect-free
        // binding (an array literal, not a Constant) whose `body` ExprId is
        // shared and already visited earlier, so `Folder::rewrite` defers
        // dropping the binding (`shared == true`) with `folded == 0`.
        let source = "(defn f [x] (let [unused [x]] (js/console.log x))) (f 1)";
        let mut program = crate::analyze(source, &crate::parse(source).unwrap()).unwrap();
        let let_idx = program
            .exprs
            .iter()
            .position(|e| matches!(e.kind, ExprKind::Let { .. }))
            .expect("let expr");
        let ExprKind::Let { bindings, body } = program.exprs[let_idx].kind.clone() else {
            unreachable!();
        };
        assert_eq!(bindings.len(), 1);
        // Replace the `fn` body with `(do body (let [unused [x]] body))` so
        // `body` is visited before the `let`, making `shared == true` on `let`.
        let fn_idx = program
            .exprs
            .iter()
            .position(|e| matches!(e.kind, ExprKind::Fn { .. }))
            .expect("fn expr");
        let do_expr = crate::analyzer::Expr {
            kind: ExprKind::Do(vec![body, ExprId(let_idx)]),
            span: program.exprs[let_idx].span.clone(),
            source_index: program.exprs[let_idx].source_index,
            context: program.exprs[let_idx].context,
        };
        let do_id = ExprId(program.exprs.len());
        program.exprs.push(do_expr);
        if let ExprKind::Fn { methods, .. } = &mut program.exprs[fn_idx].kind {
            methods[0].body = do_id;
        }

        let mut built = built_literals(&program);
        let mut top = Vec::new();
        flatten(&program, program.root, &mut top);
        let facts = ProgramFacts::new_light(&program, &top);
        let folded = fold_with_facts(&mut program, &mut built, &facts);
        assert!(folded >= 1, "expected sweep to drop shared unread binding");
        assert!(
            !matches!(program.exprs[let_idx].kind, ExprKind::Let { ref bindings, .. } if !bindings.is_empty()),
            "unread binding should be dropped by sweep: {:?}",
            program.exprs[let_idx].kind
        );
    }

    #[test]
    fn sweep_drops_unread_let_in_case_operand() {
        let source = "(case [1] [(let [unused [42]] 1)] 7 0)";
        let mut program = crate::analyze(source, &crate::parse(source).unwrap()).unwrap();
        let let_idx = program
            .exprs
            .iter()
            .position(|e| matches!(e.kind, ExprKind::Let { .. }))
            .expect("let expr in case operand");
        let mut built = built_literals(&program);
        let mut top = Vec::new();
        flatten(&program, program.root, &mut top);
        let facts = ProgramFacts::new_light(&program, &top);
        let folded = fold_with_facts(&mut program, &mut built, &facts);
        assert!(folded >= 1, "expected sweep to drop unread binding in case");
        assert!(
            !matches!(program.exprs[let_idx].kind, ExprKind::Let { ref bindings, .. } if !bindings.is_empty()),
            "unread binding in case operand should be dropped: {:?}",
            program.exprs[let_idx].kind
        );
    }

    #[test]
    fn parallel_fold_and_entry_lookups_match_serial() {
        use super::{entry_lookups_with_slots, fold_with_facts_with_slots};
        use crate::scheduler::Slots;

        // Build a program well above PARALLEL_EXPR_THRESHOLD (2048) and
        // PARALLEL_FORM_THRESHOLD (16) that exercises:
        // - a non-literal `^:const` collection pre-folded before fan-out
        // - per-slice string creation (`str`) and new-expr creation (`+` prefix and collection copy)
        // - `sweep_with_slots` dropping unread destructuring bindings
        // - `entry_lookups_with_slots` over map literals and local aliases
        let mut source =
            String::from("(def ^:const BASE {:a (+ 10 20) :b (str \"hi-\" 7) :v [1 (+ 2 3)]})\n");
        for i in 0..180 {
            source.push_str(&format!(
                "(defn f{i} [x] \
                   (let [m {{:k1 (inc x) :k2 (+ {i} 1)}} \
                         unused [x] \
                         s (str \"p\" {i} \"q\") \
                         p (+ 1 2 x) \
                         v (:v BASE) \
                         alias m] \
                     [(get alias :k1) (:k2 m) (:a BASE) (:b BASE) (nth v 1) p s]))\n"
            ));
        }
        source.push_str("(pr-str [(f0 5) (f90 6) (f179 7)])\n");

        let base = crate::analyze(&source, &crate::parse(&source).unwrap()).unwrap();
        assert!(
            base.exprs.len() >= 2048,
            "program must exceed parallel threshold: {}",
            base.exprs.len()
        );

        let mut p1 = base.clone();
        let mut built1 = built_literals(&p1);
        let mut top1 = Vec::new();
        flatten(&p1, p1.root, &mut top1);
        let facts1 = ProgramFacts::new_light(&p1, &top1);
        let folded1 = fold_with_facts_with_slots(&mut p1, &mut built1, &facts1, Slots::ONE);
        let lookups1 = entry_lookups_with_slots(&p1, &facts1, Slots::ONE);

        for n in [2, 4, 8] {
            let slots = Slots::new(n).unwrap();
            let mut pn = base.clone();
            let mut builtn = built_literals(&pn);
            let mut topn = Vec::new();
            flatten(&pn, pn.root, &mut topn);
            let factsn = ProgramFacts::new_light(&pn, &topn);
            let foldedn = fold_with_facts_with_slots(&mut pn, &mut builtn, &factsn, slots);
            let lookupsn = entry_lookups_with_slots(&pn, &factsn, slots);

            assert_eq!(foldedn, folded1, "folded count mismatch at slots={n}");
            assert_eq!(lookupsn, lookups1, "entry_lookups mismatch at slots={n}");
            assert!(
                folded1 > 180 * 3,
                "expected folds across all slices: {folded1}"
            );
            assert!(
                lookups1.len() >= 180,
                "expected entry lookups: {}",
                lookups1.len()
            );
        }
    }

    #[test]
    fn parallel_fold_falls_back_cleanly_on_cross_slice_shared_expr() {
        use super::fold_with_facts_with_slots;
        use crate::scheduler::Slots;

        let mut source = String::new();
        for i in 0..160 {
            source.push_str(&format!(
                "(defn g{i} [x] (let [a (+ 1 2) b (+ {i} 3)] (+ a b x)))\n"
            ));
        }
        source.push_str("(g0 1)\n");
        let mut base = crate::analyze(&source, &crate::parse(&source).unwrap()).unwrap();
        assert!(base.exprs.len() >= 2048);

        // Share an expression from an early top-level form inside a late top-level form.
        if let ExprKind::Do(ref items) = base.exprs[base.root.0].kind {
            let shared_id = items[0];
            let mut new_items = items.clone();
            new_items.push(shared_id);
            base.exprs[base.root.0].kind = ExprKind::Do(new_items);
        }

        let mut p1 = base.clone();
        let mut b1 = built_literals(&p1);
        let mut t1 = Vec::new();
        flatten(&p1, p1.root, &mut t1);
        let f1 = ProgramFacts::new_light(&p1, &t1);
        let folded1 = fold_with_facts_with_slots(&mut p1, &mut b1, &f1, Slots::ONE);

        let mut p4 = base.clone();
        let mut b4 = built_literals(&p4);
        let mut t4 = Vec::new();
        flatten(&p4, p4.root, &mut t4);
        let f4 = ProgramFacts::new_light(&p4, &t4);
        let folded4 = fold_with_facts_with_slots(&mut p4, &mut b4, &f4, Slots::new(4).unwrap());
        assert_eq!(folded4, folded1);
    }
}
