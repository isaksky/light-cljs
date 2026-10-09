//! Owned values and source/code lifetimes for the native interpreter.
//!
//! This module reads and evaluates a resolved primitive Clojure core. Public
//! handles root their dependencies; raw typed IDs do not. Runtime edges use
//! unrooted IDs, so language cycles do not become Rust reference-count cycles.
//! See `docs/interpreter.md` for the collection and reset contract.

mod arena;
mod binding_macros;
mod control_macros;
mod deferred;
mod iteration_macros;
mod lazy_core;
pub use lazy_core::lazy_definitions;
mod limits;
pub use deferred::{DeferredStatus, deferred_definitions};
pub use limits::{CancellationToken, ResourceError, ResourceLimit, ResourceLimits, ResourceUsage};
mod collection_core;
mod collections;
mod embedding;
mod equality;
mod evaluation;
mod reuse;
pub use embedding::{
    ContextOptions, Evaluation, EvaluationError, NamePolicy, NamespaceAlias, OptionsError,
};
pub use reuse::ReuseStats;
mod exceptions;
pub use exceptions::{ExceptionCategory, exception_definitions};
mod diagnostics;
mod loading;
mod macros;
pub use diagnostics::{CodeGenerationStats, DiagnosticLocation};
mod namespaces;
pub use loading::{
    FileSourceProvider, LoadError, LoadRequest, LoadState, Reload, SourceInput, SourceProvider,
};
pub use macros::{MacroExpansionStats, macro_definitions};
mod native;
mod state;
mod vars;
pub use namespaces::{NamespaceError, namespace_definitions};
pub use state::state_definitions;
pub use vars::{BindingGuard, VarError, var_definitions};
mod io;
mod numbers;
mod printing;
pub use io::{
    HostInput, HostOutput, OutputBuffer, Utf8Input, Utf8Output, edn_definitions, io_definitions,
};
mod sets;
mod walk;
pub use sets::set_definitions;
pub use walk::walk_definitions;
mod strings;
pub use printing::{PrintOptions, printing_definitions};
pub use strings::{string_core_definitions, string_definitions};
mod reader;
mod sequence_core;
pub use sequence_core::sequence_definitions;

pub use collection_core::collection_definitions;
pub use collections::{CollectionError, NumberError};
use collections::{EagerSeq, ListData, MapData, Sequence};
pub use evaluation::{
    AnalyzedForm, ErrorClass, EvalError, EvalErrorKind, EvalPhase, ExecutionPoll, InvokeError,
    PollPoint, SourceFrame, eval_string,
};
pub use native::{
    Arity, ForwardedError, NativeCallback, NativeDefinition, NativeEffects, NativeError,
    NativeErrorClass, NativeMetadata, NativeResult, builtin_definitions,
};
pub use numbers::ArithmeticError;
pub use reader::{
    EdnOptions, Form, FormId, FormKind, FormNode, FormReader, LowerErrorKind, Origin, ReadError,
    ReadErrorKind, ReaderLimits, SourceReader, UnsupportedSyntax,
};

use std::{
    cell::{OnceCell, RefCell},
    collections::HashMap,
    fmt,
    ops::Range,
    rc::{Rc, Weak},
    sync::atomic::{AtomicU64, Ordering},
};

use crate::parser::LineIndex;
use arena::{Arena, Key};

macro_rules! identity {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
        pub struct $name(Key);
    };
}
identity!(
    HeapId,
    "Owner- and generation-checked heap identity; does not keep a value alive."
);
identity!(
    SymbolId,
    "Interned name identity shared by symbols/keywords; does not root the name."
);
identity!(
    SourceId,
    "Owner- and generation-checked source identity; does not keep source alive."
);
identity!(
    NamespaceId,
    "Owner- and generation-checked namespace identity; not a root."
);
identity!(VarId, "Stable, owner-checked Var identity; not a root.");
identity!(
    UnitId,
    "Owner- and generation-checked executable unit identity; not a data value."
);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandleError {
    WrongOwner,
    StaleId,
    InvalidSpan,
}

impl fmt::Display for HandleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::WrongOwner => "handle belongs to another interpreter owner",
            Self::StaleId => "identity has been reclaimed",
            Self::InvalidSpan => "source span is not a valid UTF-8 byte range",
        })
    }
}
impl std::error::Error for HandleError {}

/// A name's exact UTF-16 components, without reader resolution or validation.
/// Names constructed by a host need not be valid source token spellings.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Name {
    namespace: Option<Rc<[u16]>>,
    name: Rc<[u16]>,
}

impl Name {
    pub fn new(namespace: Option<&str>, name: &str) -> Self {
        Self {
            namespace: namespace.map(|s| s.encode_utf16().collect()),
            name: name.encode_utf16().collect(),
        }
    }

    pub fn from_utf16(namespace: Option<&[u16]>, name: &[u16]) -> Self {
        Self {
            namespace: namespace.map(Rc::from),
            name: Rc::from(name),
        }
    }

    pub fn namespace(&self) -> Option<&[u16]> {
        self.namespace.as_deref()
    }

    pub fn name(&self) -> &[u16] {
        &self.name
    }
}

/// Owned inspection data. Cloning this snapshot cannot retain runtime edges.
/// Numeric categories are distinct; doubles retain all their IEEE-754 bits.
#[derive(Clone, Debug, PartialEq)]
pub enum ValueView {
    Nil,
    Boolean(bool),
    Integer(i64),
    Double(f64),
    Ratio {
        numerator: i64,
        denominator: i64,
    },
    /// One UTF-16 code unit, including surrogate units supplied by a host.
    Character(u16),
    String(Rc<[u16]>),
    Symbol(Name),
    Keyword(Name),
    /// Collection inspection is shallow; use `elements`/`entries` for roots.
    List(usize),
    Vector(usize),
    Map(usize),
    Set(usize),
    NativeFunction(NativeMetadata),
    Namespace(Name),
    Var(Name),
    /// A declared Var without a root; truthy data, but not callable.
    Unbound(Name),
    Function,
    /// An early-reduction marker; deref/unreduced expose its retained value.
    Reduced,
    Exception(ExceptionCategory),
    /// Finite sequence/Cons view, including variadic rest and deferred key/value projections.
    Seq(usize),
    /// Deferred producer or a Cons whose tail may be deferred; inspection never runs code.
    LazySeq,
    Delay,
    Atom,
    Volatile,
    Input,
    Output,
}

/// Internal runtime values must never contain a public rooted handle or Owner.
#[derive(Clone, Copy, Debug)]
enum Value {
    Nil,
    Boolean(bool),
    Integer(i64),
    Double(f64),
    Ratio(i64, i64),
    Character(u16),
    Symbol(SymbolId),
    Keyword(SymbolId),
    Heap(HeapId),
    Native(&'static NativeDefinition),
    Namespace(NamespaceId),
    Var(VarId),
    Unbound(VarId),
}

impl Value {
    fn same_identity(self, other: Self) -> bool {
        match (self, other) {
            (Self::Nil, Self::Nil) => true,
            (Self::Boolean(a), Self::Boolean(b)) => a == b,
            (Self::Integer(a), Self::Integer(b)) => a == b,
            (Self::Double(a), Self::Double(b)) => a.to_bits() == b.to_bits(),
            (Self::Ratio(a, b), Self::Ratio(c, d)) => (a, b) == (c, d),
            (Self::Character(a), Self::Character(b)) => a == b,
            (Self::Symbol(a), Self::Symbol(b)) | (Self::Keyword(a), Self::Keyword(b)) => a == b,
            (Self::Heap(a), Self::Heap(b)) => a == b,
            (Self::Namespace(a), Self::Namespace(b)) => a == b,
            (Self::Var(a), Self::Var(b)) => a == b,
            (Self::Unbound(_), Self::Unbound(_)) => true,
            (Self::Native(a), Self::Native(b)) => std::ptr::eq(a, b),
            _ => false,
        }
    }
}

enum HeapValue {
    String(Rc<[u16]>),
    Closure(evaluation::Closure),
    HigherOrder(sequence_core::HigherOrder),
    Reduced(Value),
    Atom(state::Atom),
    Volatile(Value),
    Input(Rc<RefCell<io::InputState>>),
    Output(Rc<RefCell<Box<dyn HostOutput>>>),
    Deferred(deferred::Deferred),
    LazyCons {
        head: Value,
        tail: Value,
    },
    Exception(exceptions::Exception),
    ArgumentSeq(EagerSeq),
    List(ListData),
    Vector(Sequence),
    Map(MapData),
    Set(MapData),
    Annotated {
        value: Value,
        metadata: Value,
    },
    // Arbitrary test graphs exercise collector shapes independently of language
    // constructors. This variant and its mutation helpers are absent from production.
    #[cfg(test)]
    Edges(Vec<Target>),
}

#[derive(Clone, Copy)]
enum Target {
    Value(Value),
    Source(SourceId),
    Unit(UnitId),
}

struct Root(Target);

// Immediate values have no collector edges. Keeping them inline avoids an Rc
// allocation and dead weak entry for every arithmetic result and temporary nil.
#[derive(Clone)]
enum ValueRoot {
    Immediate(Value),
    Traced(Rc<Root>),
}

/// An owned value. Clones of collector-backed values share one root record.
#[derive(Clone)]
pub struct RootedValue {
    owner: Rc<Owner>,
    root: ValueRoot,
}

impl RootedValue {
    fn value(&self) -> Value {
        match &self.root {
            ValueRoot::Immediate(value) => *value,
            ValueRoot::Traced(root) => match root.0 {
                Target::Value(value) => value,
                _ => unreachable!(),
            },
        }
    }

    /// Read without borrowing the context or the input/parse buffer.
    pub fn inspect(&self) -> ValueView {
        let store = self.owner.store.borrow();
        match store.unannotated(self.value()) {
            Value::Nil => ValueView::Nil,
            Value::Boolean(v) => ValueView::Boolean(v),
            Value::Integer(v) => ValueView::Integer(v),
            Value::Double(v) => ValueView::Double(v),
            Value::Ratio(numerator, denominator) => ValueView::Ratio {
                numerator,
                denominator,
            },
            Value::Character(v) => ValueView::Character(v),
            Value::Native(definition) => ValueView::NativeFunction(definition.metadata),
            Value::Namespace(id) => ValueView::Namespace(
                store
                    .namespaces
                    .get(id.0)
                    .expect("rooted namespace")
                    .name
                    .clone(),
            ),
            Value::Var(id) => ValueView::Var(store.var_name(id)),
            Value::Unbound(id) => ValueView::Unbound(store.var_name(id)),
            Value::Symbol(id) => {
                ValueView::Symbol(store.names.get(id.0).expect("rooted name").clone())
            }
            Value::Keyword(id) => {
                ValueView::Keyword(store.names.get(id.0).expect("rooted name").clone())
            }
            Value::Heap(id) => match store.heap.get(id.0).expect("rooted heap value") {
                HeapValue::String(v) => ValueView::String(v.clone()),
                HeapValue::Closure(_) | HeapValue::HigherOrder(_) => ValueView::Function,
                HeapValue::Reduced(_) => ValueView::Reduced,
                HeapValue::Atom(_) => ValueView::Atom,
                HeapValue::Volatile(_) => ValueView::Volatile,
                HeapValue::Input(_) => ValueView::Input,
                HeapValue::Output(_) => ValueView::Output,
                HeapValue::Deferred(d) if d.delay => ValueView::Delay,
                HeapValue::Deferred(_) | HeapValue::LazyCons { .. } => ValueView::LazySeq,
                HeapValue::Exception(e) => ValueView::Exception(e.category),
                HeapValue::ArgumentSeq(v) => ValueView::Seq(v.len()),
                HeapValue::List(v) => {
                    if v.is_list() {
                        ValueView::List(v.len())
                    } else {
                        ValueView::Seq(v.len())
                    }
                }
                HeapValue::Vector(v) => ValueView::Vector(v.len()),
                HeapValue::Map(v) => ValueView::Map(v.len()),
                HeapValue::Set(v) => ValueView::Set(v.len()),
                HeapValue::Annotated { .. } => unreachable!("annotations were removed"),
                #[cfg(test)]
                HeapValue::Edges(_) => panic!("test graph is not language data"),
            },
        }
    }

    /// Representation identity: heap/name IDs include their owner and generation;
    /// immediate values compare their exact representation (including float bits).
    /// This is not a promise of the JVM's boxing-dependent `identical?` behavior.
    pub fn same_identity(&self, other: &Self) -> bool {
        self.value().same_identity(other.value())
    }

    /// Structural value equality, including across contexts. Categories stay distinct;
    /// doubles use IEEE equality (NaN differs from itself; signed zeros are equal).
    /// This total storage helper is intended for eager values. A failed deferred
    /// comparison returns false; use `Context::value_eq` to realize lazy producers.
    pub fn value_eq(&self, other: &Self) -> bool {
        equality::equal(
            &self.owner.store.borrow(),
            self.value(),
            &other.owner.store.borrow(),
            other.value(),
        )
    }

    /// A content hash consistent with `value_eq`, independent of metadata and
    /// owner. This is not the JVM's exact numeric `hash` result. This eager-value
    /// storage helper uses a shared fallback hash if projection fails; language
    /// callers must realize lazy keys first and use `try_value_hash` to preserve failures.
    pub fn value_hash(&self) -> u64 {
        equality::hash(&self.owner.store.borrow(), self.value())
    }

    /// Stored-value equality; unrealized sequences return RequiresRealization.
    /// Use Context::value_eq when evaluation is allowed.
    pub fn try_value_eq(&self, other: &Self) -> Result<bool, CollectionError> {
        equality::try_equal(
            &self.owner.store.borrow(),
            self.value(),
            &other.owner.store.borrow(),
            other.value(),
        )
    }
    /// Semantic content hashing with explicit deferred sequence failures.
    pub fn try_value_hash(&self) -> Result<u64, CollectionError> {
        equality::try_hash(&self.owner.store.borrow(), self.value())
    }

    pub fn heap_id(&self) -> Option<HeapId> {
        match self.value() {
            Value::Heap(id) => Some(id),
            _ => None,
        }
    }

    /// Symbols and keywords with the same components share a name ID, but remain
    /// different value categories and are never equal to one another.
    pub fn var_id(&self) -> Option<VarId> {
        match self.value() {
            Value::Var(id) => Some(id),
            _ => None,
        }
    }

    pub fn namespace_id(&self) -> Option<NamespaceId> {
        match self.value() {
            Value::Namespace(id) => Some(id),
            _ => None,
        }
    }

    pub fn symbol_id(&self) -> Option<SymbolId> {
        match self.owner.store.borrow().unannotated(self.value()) {
            Value::Symbol(id) | Value::Keyword(id) => Some(id),
            _ => None,
        }
    }
}

impl fmt::Debug for RootedValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inspect().fmt(f)
    }
}

struct SourceFile {
    name: String,
    text: String,
    lines: OnceCell<LineIndex>,
    // Stream compaction retains original lines without retaining consumed text.
    line_offset: usize,
}

/// An owned source file. Text and byte positions survive Context reset/drop.
#[derive(Clone)]
pub struct Source {
    owner: Rc<Owner>,
    root: Rc<Root>,
    file: Rc<SourceFile>,
}

impl Source {
    pub fn id(&self) -> SourceId {
        let Target::Source(id) = self.root.0 else {
            unreachable!()
        };
        id
    }

    pub fn name(&self) -> &str {
        &self.file.name
    }
    pub fn text(&self) -> &str {
        &self.file.text
    }

    /// Lines and byte columns follow the existing parser's LineIndex contract.
    /// Compacted stream sources retain their original line numbers; byte spans
    /// remain relative to this source's retained text.
    pub fn line_column(&self, offset: usize) -> Option<(usize, usize)> {
        self.file
            .lines
            .get_or_init(|| LineIndex::new(self.text()))
            .line_column(offset)
            .and_then(|(line, column)| Some((line.checked_add(self.file.line_offset)?, column)))
    }

    pub fn span(&self, bytes: Range<usize>) -> Result<SourceSpan, HandleError> {
        if self.text().get(bytes.clone()).is_none() {
            return Err(HandleError::InvalidSpan);
        }
        Ok(SourceSpan {
            source: self.clone(),
            bytes,
        })
    }
}

impl fmt::Debug for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Source")
            .field("id", &self.id())
            .field("name", &self.name())
            .finish()
    }
}

/// Validated UTF-8 byte provenance, retaining its original source.
#[derive(Clone, Debug)]
pub struct SourceSpan {
    source: Source,
    bytes: Range<usize>,
}

impl SourceSpan {
    pub fn source(&self) -> &Source {
        &self.source
    }
    pub fn bytes(&self) -> Range<usize> {
        self.bytes.clone()
    }
    pub fn text(&self) -> &str {
        &self.source.text()[self.bytes.clone()]
    }
}

#[derive(Clone)]
struct Location {
    source: SourceId,
    bytes: Range<usize>,
    origin: Origin,
}

// Executable storage is intentionally separate from data Values. Runtime edges
// retain units through UnitId, including their constants and source side table.
struct CodeUnit {
    constants: Box<[Value]>,
    locations: Box<[Location]>,
    expressions: Box<[evaluation::Expr]>,
    functions: Box<[evaluation::FunctionCode]>,
    local_count: usize,
    vars: Box<[VarId]>,
    permissions: Box<[Name]>,
}

/// Owned executable constant/provenance storage, optionally with analyzed code.
/// A unit is not a reader form or a callable language value.
#[derive(Clone)]
pub struct ExecutableUnit {
    owner: Rc<Owner>,
    root: Rc<Root>,
    code: Rc<CodeUnit>,
}

impl ExecutableUnit {
    pub fn id(&self) -> UnitId {
        let Target::Unit(id) = self.root.0 else {
            unreachable!()
        };
        id
    }

    pub fn constant_count(&self) -> usize {
        self.code.constants.len()
    }
    pub fn span_count(&self) -> usize {
        self.code.locations.len()
    }

    pub fn constant(&self, index: usize) -> Option<RootedValue> {
        self.code
            .constants
            .get(index)
            .map(|value| self.owner.value(*value))
    }

    pub fn span(&self, index: usize) -> Option<SourceSpan> {
        self.code.locations.get(index).map(|location| SourceSpan {
            source: self
                .owner
                .source(location.source)
                .expect("rooted unit source"),
            bytes: location.bytes.clone(),
        })
    }
}

impl fmt::Debug for ExecutableUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExecutableUnit")
            .field("id", &self.id())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MemoryStats {
    pub heap_values: usize,
    pub names: usize,
    pub sources: usize,
    pub executable_units: usize,
    pub namespaces: usize,
    pub vars: usize,
    /// Live collector root records; clones share a record. Immediate values and IDs do not count.
    pub roots: usize,
    /// Arena slots, including reusable vacancies. Backing capacity is retained.
    pub slots: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CollectionStats {
    pub before: MemoryStats,
    pub after: MemoryStats,
}

struct Store {
    accounting: Rc<limits::Accounting>,
    heap: Arena<HeapValue>,
    names: Arena<Name>,
    sources: Arena<Rc<SourceFile>>,
    units: Arena<Rc<CodeUnit>>,
    interned: HashMap<Name, SymbolId>,
    namespaces: Arena<namespaces::Namespace>,
    vars: Arena<namespaces::Var>,
    trace: Vec<Target>,
}

struct Owner {
    accounting: Rc<limits::Accounting>,
    store: RefCell<Store>,
    roots: RefCell<Vec<Weak<Root>>>,
}

impl Owner {
    fn new(accounting: Rc<limits::Accounting>) -> Self {
        static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);
        let id = loop {
            let id = NEXT_OWNER.load(Ordering::Relaxed);
            let next = id
                .checked_add(1)
                .expect("interpreter owner identities exhausted");
            if NEXT_OWNER
                .compare_exchange_weak(id, next, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                break id;
            }
        };
        Self {
            accounting: accounting.clone(),
            store: RefCell::new(Store {
                accounting,
                heap: Arena::new(id),
                names: Arena::new(id),
                sources: Arena::new(id),
                units: Arena::new(id),
                interned: HashMap::new(),
                namespaces: Arena::new(id),
                vars: Arena::new(id),
                trace: Vec::new(),
            }),
            roots: RefCell::new(Vec::new()),
        }
    }

    fn root(&self, target: Target) -> Rc<Root> {
        // One bounded record; entry points recheck the sticky failure before
        // exposing success. Diagnostic roots can still be retained on failure.
        let _ = self.accounting.charge(ResourceLimit::Allocation, 1);
        let root = Rc::new(Root(target));
        self.roots.borrow_mut().push(Rc::downgrade(&root));
        root
    }

    fn value(self: &Rc<Self>, value: Value) -> RootedValue {
        let root = match value {
            Value::Nil
            | Value::Boolean(_)
            | Value::Integer(_)
            | Value::Double(_)
            | Value::Ratio(_, _)
            | Value::Character(_)
            | Value::Native(_) => {
                // Preserve the semantic allocation budget independently of the
                // Rust representation; allocation optimizations are not a bypass.
                let _ = self.accounting.charge(ResourceLimit::Allocation, 1);
                ValueRoot::Immediate(value)
            }
            _ => ValueRoot::Traced(self.root(Target::Value(value))),
        };
        RootedValue {
            owner: self.clone(),
            root,
        }
    }

    fn checked_value(self: &Rc<Self>, value: Value) -> Result<RootedValue, CollectionError> {
        let value = self.value(value);
        self.accounting.check().map_err(CollectionError::Resource)?;
        Ok(value)
    }

    fn source(self: &Rc<Self>, id: SourceId) -> Result<Source, HandleError> {
        let file = self.store.borrow().sources.get(id.0)?.clone();
        Ok(Source {
            owner: self.clone(),
            root: self.root(Target::Source(id)),
            file,
        })
    }

    fn unit(self: &Rc<Self>, id: UnitId) -> Result<ExecutableUnit, HandleError> {
        let code = self.store.borrow().units.get(id.0)?.clone();
        Ok(ExecutableUnit {
            owner: self.clone(),
            root: self.root(Target::Unit(id)),
            code,
        })
    }

    fn stats(&self) -> MemoryStats {
        let store = self.store.borrow();
        MemoryStats {
            heap_values: store.heap.len(),
            names: store.names.len(),
            sources: store.sources.len(),
            executable_units: store.units.len(),
            namespaces: store.namespaces.len(),
            vars: store.vars.len(),
            roots: self
                .roots
                .borrow()
                .iter()
                .filter(|root| root.strong_count() != 0)
                .count(),
            slots: store.heap.slots()
                + store.names.slots()
                + store.sources.slots()
                + store.units.slots()
                + store.namespaces.slots()
                + store.vars.slots(),
        }
    }

    fn collect(&self) -> CollectionStats {
        let before = self.stats();
        let mut store = self.store.borrow_mut();
        let mut trace = std::mem::take(&mut store.trace);
        self.roots.borrow_mut().retain(|weak| {
            if let Some(root) = weak.upgrade() {
                trace.push(root.0);
                true
            } else {
                false
            }
        });
        while let Some(target) = trace.pop() {
            match target {
                Target::Value(Value::Heap(id)) => {
                    if let Some(value) = store.heap.mark(id.0) {
                        match value {
                            HeapValue::String(_) | HeapValue::Input(_) | HeapValue::Output(_) => {}
                            HeapValue::Exception(e) => {
                                trace.extend([e.message, e.data, e.cause].map(Target::Value));
                            }
                            HeapValue::Closure(closure) => {
                                trace.push(Target::Unit(closure.unit));
                                trace.extend(closure.captures.iter().copied().map(Target::Value));
                            }
                            HeapValue::HigherOrder(function) => {
                                trace.extend(function.captures.iter().copied().map(Target::Value));
                                trace.extend(function.tail.map(Target::Value));
                            }
                            HeapValue::Reduced(value) | HeapValue::Volatile(value) => {
                                trace.push(Target::Value(*value))
                            }
                            HeapValue::Atom(atom) => atom.trace(&mut trace),
                            HeapValue::Deferred(d) => d.trace(&mut trace),
                            HeapValue::LazyCons { head, tail } => {
                                trace.push(Target::Value(*head));
                                trace.push(Target::Value(*tail));
                            }
                            HeapValue::ArgumentSeq(values) => {
                                trace.extend(values.iter().copied().map(Target::Value));
                                trace.extend(values.sources().map(Target::Value));
                            }
                            HeapValue::List(ListData::Empty) => {}
                            HeapValue::List(ListData::Cons { head, tail, .. }) => {
                                trace.push(Target::Value(*head));
                                trace.push(Target::Value(*tail));
                            }
                            HeapValue::Vector(values) => values.trace(&mut trace),
                            HeapValue::Map(values) | HeapValue::Set(values) => {
                                values.trace(&mut trace);
                            }
                            HeapValue::Annotated { value, metadata } => {
                                trace.push(Target::Value(*value));
                                trace.push(Target::Value(*metadata));
                            }
                            #[cfg(test)]
                            HeapValue::Edges(edges) => trace.extend_from_slice(edges),
                        }
                    }
                }
                Target::Value(Value::Namespace(id)) => {
                    if let Some(ns) = store.namespaces.mark(id.0) {
                        trace.extend(
                            ns.aliases
                                .values()
                                .map(|id| Target::Value(Value::Namespace(*id))),
                        );
                        trace.extend(ns.mappings.iter().flat_map(|(symbol, var)| {
                            [
                                Target::Value(Value::Symbol(*symbol)),
                                Target::Value(Value::Var(*var)),
                            ]
                        }));
                    }
                }
                Target::Value(Value::Var(id) | Value::Unbound(id)) => {
                    if let Some(var) = store.vars.mark(id.0) {
                        trace.push(Target::Value(Value::Namespace(var.namespace)));
                        trace.push(Target::Value(Value::Symbol(var.name)));
                        trace.push(Target::Value(var.metadata));
                        trace.extend(var.root.map(Target::Value));
                        trace.extend(var.bindings.iter().map(|(_, v)| Target::Value(*v)));
                    }
                }
                Target::Value(Value::Symbol(id) | Value::Keyword(id)) => {
                    store.names.mark(id.0);
                }
                Target::Value(_) => {}
                Target::Source(id) => {
                    store.sources.mark(id.0);
                }
                Target::Unit(id) => {
                    if let Some(code) = store.units.mark(id.0) {
                        trace.extend(code.constants.iter().copied().map(Target::Value));
                        trace.extend(
                            code.vars
                                .iter()
                                .copied()
                                .map(|id| Target::Value(Value::Var(id))),
                        );
                        trace.extend(
                            code.locations
                                .iter()
                                .map(|location| Target::Source(location.source)),
                        );
                    }
                }
            }
        }
        store.heap.sweep();
        store.names.sweep();
        store.sources.sweep();
        store.units.sweep();
        store.namespaces.sweep();
        store.vars.sweep();
        let Store {
            names, interned, ..
        } = &mut *store;
        interned.retain(|_, id| names.get(id.0).is_ok());
        // `trace` is empty: scratch capacity survives, runtime references do not.
        store.trace = trace;
        drop(store);
        CollectionStats {
            before,
            after: self.stats(),
        }
    }
}

/// Single-threaded context. Fresh contexts own isolated stores; explicit forks
/// share a store and existing values. Host handles outlive the context.
/// Constructors only build data; they perform no evaluation or external effects.
pub struct Context {
    owner: Rc<Owner>,
    evaluation: evaluation::Scratch,
    analysis_scratch: evaluation::AnalysisScratch,
    reuse: reuse::AnalysisReuse,
    reuse_revision: Rc<std::cell::Cell<u64>>,
    active_calls: usize,
    active_expressions: usize,
    accounting: Rc<limits::Accounting>,
    namespaces: Rc<OnceCell<RefCell<namespaces::Registry>>>,
    readers: Rc<RefCell<reader::ReaderConfig>>,
    macros: macros::MacroState,
    loading: Rc<RefCell<loading::Loader>>,
    builtin_var_mutation: Rc<std::cell::Cell<bool>>,
    policy: Rc<RefCell<NamePolicy>>,
    var_remap: Rc<RefCell<HashMap<VarId, RootedValue>>>,
    #[cfg(test)]
    evaluation_trace: Vec<String>,
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

impl Context {
    pub fn new() -> Self {
        let accounting = Rc::new(limits::Accounting::default());
        Self {
            owner: Rc::new(Owner::new(accounting.clone())),
            evaluation: evaluation::Scratch::default(),
            analysis_scratch: evaluation::AnalysisScratch::default(),
            reuse: reuse::AnalysisReuse::default(),
            reuse_revision: Rc::new(std::cell::Cell::new(0)),
            active_calls: 0,
            active_expressions: 0,
            accounting,
            namespaces: Rc::new(OnceCell::new()),
            readers: Rc::new(RefCell::new(reader::ReaderConfig::default())),
            macros: macros::MacroState::default(),
            loading: Rc::new(RefCell::new(loading::Loader::default())),
            builtin_var_mutation: Rc::new(std::cell::Cell::new(false)),
            policy: Rc::new(RefCell::new(NamePolicy::default())),
            var_remap: Rc::new(RefCell::new(HashMap::new())),
            #[cfg(test)]
            evaluation_trace: Vec::new(),
        }
    }
    pub fn nil(&self) -> RootedValue {
        self.owner.value(Value::Nil)
    }
    pub fn boolean(&self, value: bool) -> RootedValue {
        self.owner.value(Value::Boolean(value))
    }
    pub fn integer(&self, value: i64) -> RootedValue {
        self.owner.value(Value::Integer(value))
    }
    pub fn double(&self, value: f64) -> RootedValue {
        self.owner.value(Value::Double(value))
    }
    pub fn character(&self, unit: u16) -> RootedValue {
        self.owner.value(Value::Character(unit))
    }

    pub fn string(&self, units: impl Into<Rc<[u16]>>) -> RootedValue {
        // Conversion may execute host code; finish it before borrowing storage.
        let units = units.into();
        // Infallible host data construction is cooperative: latch exhaustion
        // for the surrounding invocation, which must never return success.
        let _ = self.charge_allocation(units.len().saturating_add(1) as u64);
        let id = HeapId(
            self.owner
                .store
                .borrow_mut()
                .heap
                .insert(HeapValue::String(units)),
        );
        self.owner.value(Value::Heap(id))
    }

    fn intern(&self, name: Name) -> SymbolId {
        let mut store = self.owner.store.borrow_mut();
        if let Some(id) = store.interned.get(&name) {
            return *id;
        }
        let units = name
            .name()
            .len()
            .saturating_add(name.namespace().map_or(0, <[u16]>::len))
            .saturating_add(1);
        let _ = self.charge_allocation(units as u64);
        let id = SymbolId(store.names.insert(name.clone()));
        store.interned.insert(name, id);
        id
    }

    pub fn symbol(&self, name: Name) -> RootedValue {
        self.owner.value(Value::Symbol(self.intern(name)))
    }

    pub fn keyword(&self, name: Name) -> RootedValue {
        self.owner.value(Value::Keyword(self.intern(name)))
    }

    /// Recover a root only while the raw ID is still live in this owner.
    pub fn heap_value(&self, id: HeapId) -> Result<RootedValue, HandleError> {
        self.owner.store.borrow().heap.get(id.0)?;
        Ok(self.owner.value(Value::Heap(id)))
    }

    pub fn symbol_value(&self, id: SymbolId) -> Result<RootedValue, HandleError> {
        self.owner.store.borrow().names.get(id.0)?;
        Ok(self.owner.value(Value::Symbol(id)))
    }

    pub fn keyword_value(&self, id: SymbolId) -> Result<RootedValue, HandleError> {
        self.owner.store.borrow().names.get(id.0)?;
        Ok(self.owner.value(Value::Keyword(id)))
    }

    pub fn source(&self, name: impl Into<String>, text: impl Into<String>) -> Source {
        self.source_at_line(name.into(), text.into(), 0)
    }

    fn source_at_line(&self, name: String, text: String, line_offset: usize) -> Source {
        let file = Rc::new(SourceFile {
            name,
            text,
            lines: OnceCell::new(),
            line_offset,
        });
        let id = SourceId(self.owner.store.borrow_mut().sources.insert(file));
        self.owner.source(id).expect("new source")
    }

    pub fn source_by_id(&self, id: SourceId) -> Result<Source, HandleError> {
        self.owner.source(id)
    }

    /// Freeze an owned constant/provenance pool, rejecting foreign/reset handles
    /// before allocating anything. It contains no parser nodes or source borrows.
    pub fn executable_unit(
        &self,
        constants: &[RootedValue],
        spans: &[SourceSpan],
    ) -> Result<ExecutableUnit, HandleError> {
        if constants.iter().any(|v| !Rc::ptr_eq(&v.owner, &self.owner))
            || spans
                .iter()
                .any(|s| !Rc::ptr_eq(&s.source.owner, &self.owner))
        {
            return Err(HandleError::WrongOwner);
        }
        let code = Rc::new(CodeUnit {
            constants: constants.iter().map(RootedValue::value).collect(),
            locations: spans
                .iter()
                .map(|span| Location {
                    source: span.source.id(),
                    bytes: span.bytes.clone(),
                    origin: Origin::Source,
                })
                .collect(),
            expressions: Box::default(),
            functions: Box::default(),
            local_count: 0,
            vars: Box::default(),
            permissions: Box::default(),
        });
        let id = UnitId(self.owner.store.borrow_mut().units.insert(code));
        self.owner.unit(id)
    }

    pub fn unit_by_id(&self, id: UnitId) -> Result<ExecutableUnit, HandleError> {
        self.owner.unit(id)
    }

    pub fn memory_stats(&self) -> MemoryStats {
        self.owner.stats()
    }

    /// Collect at an explicit host boundary. No automatic collection occurs
    /// during allocation, and no runtime borrow is held across host callbacks.
    pub fn collect_garbage(&mut self) -> CollectionStats {
        self.prune_analysis_reuse();
        self.owner.collect()
    }

    /// Start a fresh owner, retaining name policy and resource configuration.
    /// Old host handles remain
    /// usable but cannot be inserted into the new owner. Old IDs never alias it.
    pub fn reset(&mut self) {
        self.evaluation.clear();
        self.analysis_scratch = evaluation::AnalysisScratch::default();
        self.reuse = reuse::AnalysisReuse::default();
        self.reuse_revision = Rc::new(std::cell::Cell::new(0));
        self.namespaces = Rc::new(OnceCell::new());
        self.readers = Rc::new(RefCell::new(reader::ReaderConfig::default()));
        self.loading = Rc::new(RefCell::new(loading::Loader::default()));
        self.builtin_var_mutation = Rc::new(std::cell::Cell::new(false));
        let policy = self.policy.borrow().clone();
        self.policy = Rc::new(RefCell::new(policy));
        self.var_remap = Rc::new(RefCell::new(HashMap::new()));
        let form_depth = self.macros.form_depth;
        self.macros = macros::MacroState::default();
        self.macros.form_depth = form_depth;
        self.set_execution_poll(None);
        self.owner.collect();
        self.owner = Rc::new(Owner::new(self.accounting.clone()));
    }
}

#[cfg(test)]
mod tests;
