//! Resolve the executable core to an arena with stable lexical binding IDs.

use std::{borrow::Cow, collections::HashMap, fmt, ops::Range};

use crate::compiler_data::{CompilerLiteralKind, DatumView, Form, FormKind, Source};
use crate::macro_host::MacroHost;

use crate::{CollectionKind, NodeId, NodeKind, NumberKind, PrefixKind, SyntaxTree};

pub type Span = Range<usize>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ExprId(pub usize);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct LocalId(pub usize);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct StringId(pub usize);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct NamedLiteralId(pub usize);

/// Names share the owned UTF-16 table with strings, including generated names.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NamedLiteral {
    pub namespace: Option<StringId>,
    pub name: StringId,
}

/// The finite executable JavaScript core; independent of interpreter Vars.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum CoreFnId {
    ExecTapFn,
    NamespaceClass,
    CreateNs,
    FindNs,
    NsName,
    TheNs,
    AllNs,
    CurrentNs,
    Assertions,
    CompilerMacroVar,
    SystemTime,
    AddTap,
    RemoveTap,
    TapGreater,
    LoadFile,
    PrintMeta,
    PrintDup,
    FlushOnNewline,
    SetPrintFn,
    SetPrintErrFn,
    StringPrint,
    Flush,
    PrStrWithOpts,
    PrnStrWithOpts,
    PrSequentialWriter,
    PrSeqWriter,
    PrintMetaQ,
    EnableConsolePrint,
    PrintFnBodies,

    MultiFn,
    IMultiFn,
    MakeHierarchy,
    GetGlobalHierarchy,
    Isa,
    Derive,
    Underive,
    Parents,
    Ancestors,
    Descendants,
    Bases,
    Supers,
    RemoveAllMethods,
    ResetMultiFnProtocol,
    AddMethod,
    RemoveMethod,
    RemoveMethodProtocol,
    PreferMethod,
    PreferMethodProtocol,
    Methods,
    MethodsProtocol,
    GetMethod,
    GetMethodProtocol,
    Prefers,
    PrefersProtocol,
    DefaultDispatchVal,
    DefaultDispatchValProtocol,
    DispatchFn,
    DispatchFnProtocol,

    EdnReadString,
    EdnRead,
    ReaderReadString,
    ReaderRead,
    ReaderStringReader,
    ReaderReadLine,
    ReaderInputProvider,
    ReaderParseTimestampFields,
    ReaderParseTimestamp,
    ReaderInitialTagTable,
    ReaderRegisterTagParser,
    ReaderDeregisterTagParser,
    ReaderRegisterDefaultTagParser,
    ReaderDeregisterDefaultTagParser,
    ReaderCurrentInputValue,
    ReaderCurrentInput,
    ReaderWithInput,
    ReaderClose,
    ReadString,
    Read,
    ReadLine,

    Gensym,
    CommandLineArgs,
    PrintErrFn,
    ReplOne,
    ReplTwo,
    ReplThree,
    ReplError,
    SourceFile,
    Eval,
    Macroexpand,
    Macroexpand1,
    Keyword,
    Symbol,
    Vector,
    List,
    HashMap,
    ArrayMap,
    HashSet,
    Dissoc,
    Disj,
    Conj,
    Transient,
    PersistentBang,
    ConjBang,
    AssocBang,
    DissocBang,
    DisjBang,
    PopBang,
    IEditableCollection,
    ITransientCollection,
    ITransientAssociative,
    ITransientMap,
    ITransientVector,
    ITransientSet,
    AsTransientProtocol,
    PersistentBangProtocol,
    ConjBangProtocol,
    AssocBangProtocol,
    DissocBangProtocol,
    DisjoinBangProtocol,
    PopBangProtocol,
    AssocNBangProtocol,
    Assoc,
    Seq,
    SeqToMap,
    First,
    Rest,
    Next,
    Name,
    Namespace,
    Subs,
    Count,
    Nth,
    Get,
    GetIn,
    AssocIn,
    UpdateIn,
    SelectKeys,
    Merge,
    MergeWith,
    SomeVals,
    RequiredGet,
    MinKey,
    MaxKey,
    Reductions,
    Replace,
    Distinct,
    IsDistinct,
    Sort,
    SortBy,
    SomeFn,
    EveryPred,
    Memoize,
    Rand,
    RandInt,
    RandNth,
    ParseLong,
    ParseDouble,
    ParseUuid,
    ParseBoolean,
    IsJsSymbol,
    IsSpecialSymbol,
    ToArray2d,
    IntArray,
    LongArray,
    DoubleArray,
    ObjectArray,
    GroupBy,
    Frequencies,
    Trampoline,
    Comparator,
    UpdateVals,
    UpdateKeys,
    Zipmap,
    Contains,
    Find,
    IsCloneable,
    Every,
    Set,
    Equal,
    Reduce,
    Add,
    Subtract,
    Multiply,
    Divide,
    Inc,
    Dec,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    NumericEqual,
    Max,
    Min,
    Abs,
    Int,
    Long,
    Byte,
    Short,
    Float,
    Double,
    Char,
    Longs,
    Doubles,
    Floats,
    Ints,
    Shorts,
    Chars,
    Bytes,
    Booleans,
    UncheckedInt,
    UncheckedLong,
    UncheckedByte,
    UncheckedChar,
    UncheckedShort,
    UncheckedFloat,
    UncheckedDouble,
    UncheckedAdd,
    UncheckedAddInt,
    UncheckedSubtract,
    UncheckedSubtractInt,
    UncheckedMultiply,
    UncheckedMultiplyInt,
    UncheckedDivideInt,
    UncheckedInc,
    UncheckedIncInt,
    UncheckedDec,
    UncheckedDecInt,
    UncheckedNegate,
    UncheckedNegateInt,
    UncheckedRemainderInt,
    JsMod,
    Mod,
    Quot,
    Rem,
    BitXor,
    BitAnd,
    BitOr,
    BitAndNot,
    BitNot,
    BitClear,
    BitFlip,
    BitSet,
    BitTest,
    BitShiftLeft,
    BitShiftRight,
    BitShiftRightZeroFill,
    UnsignedBitShiftRight,
    BitCount,
    IsZero,
    IsPositive,
    IsNegative,
    IsEven,
    IsOdd,
    IsInteger,
    IsInt,
    IsPositiveInt,
    IsNegativeInt,
    IsNaturalInt,
    IsNumber,
    IsFloat,
    IsDouble,
    IsInfinite,
    IsNaN,
    Boolean,
    IsBoolean,
    IsTrue,
    IsFalse,
    IsNil,
    IsSome,
    IsUndefined,
    Not,
    NotEqual,
    Identity,
    IsAny,
    IsString,
    IsChar,
    IsKeyword,
    IsSymbol,
    IsIdent,
    IsSimpleIdent,
    IsQualifiedIdent,
    IsSimpleKeyword,
    IsQualifiedKeyword,
    IsSimpleSymbol,
    IsQualifiedSymbol,
    KeywordIdentical,
    SymbolIdentical,
    IsFn,
    IsIFn,
    IsArray,
    IsObject,
    IsColl,
    IsList,
    IsSeq,
    IsSeqable,
    IsSequential,
    IsVector,
    IsMap,
    IsSet,
    IsAssociative,
    IsIFind,
    IsCounted,
    IsIndexed,
    IsReduceable,
    IsReversible,
    IsSorted,
    IsRecord,
    IsMapEntry,
    IsChunkedSeq,
    IsEmpty,
    Completing,
    Transduce,
    EnsureReduced,
    HaltWhen,
    Dedupe,
    RandomSample,
    Reduced,
    IsReduced,
    Unreduced,
    Deref,
    Delay,
    DelayFactory,
    LazySeqFactory,
    IsDelay,
    Force,
    Realized,
    IPending,
    RealizedProtocol,
    Nthrest,
    Nthnext,
    Meta,
    WithMeta,
    SortedMap,
    SortedMapBy,
    SortedSet,
    SortedSetBy,
    Subseq,
    Rsubseq,
    MkBoundFn,
    VaryMeta,
    ResetMeta,
    AlterMeta,
    IMeta,
    IWithMeta,
    IMapEntry,
    IFind,
    ISorted,
    IChunkedSeq,
    KeyProtocol,
    ValProtocol,
    FindProtocol,
    SortedSeqProtocol,
    SortedSeqFromProtocol,
    EntryKeyProtocol,
    ComparatorProtocol,
    ChunkedFirstProtocol,
    ChunkedRestProtocol,
    InvokeProtocol,
    WriteAll,
    IsIterable,
    PersistentVector,
    SubvecType,
    PersistentArrayMap,
    PersistentHashMap,
    PersistentTreeMap,
    PersistentTreeSet,
    PersistentTreeMapSeq,
    PersistentHashSet,
    ListType,
    EmptyList,
    ConsType,
    IndexedSeq,
    LazySeq,
    PersistentQueue,
    KeywordType,
    SymbolType,
    ReducedType,
    UUID,
    MapEntry,
    IRecord,
    ICloneable,
    IPrintWithWriter,
    IWriter,
    Clone,
    PrWriter,
    Write,
    FlushWriter,
    Satisfies,
    Implements,
    ProtocolSentinel,
    FnMarker,
    IList,
    IFn,
    ISeqable,
    ISeq,
    INext,
    ICounted,
    IIndexed,
    ILookup,
    ISequential,
    IAssociative,
    IReduce,
    IKVReduce,
    IVector,
    IMap,
    ICollection,
    IEmptyableCollection,
    ISet,
    IStack,
    IReversible,
    IComparable,
    IIterable,
    ReduceProtocol,
    KvReduceProtocol,
    AssocProtocol,
    AssocNProtocol,
    DissocProtocol,
    ConjProtocol,
    EmptyProtocol,
    DisjoinProtocol,
    PeekProtocol,
    PopProtocol,
    RseqProtocol,
    CompareProtocol,
    Compare,
    IteratorProtocol,
    ReduceKv,
    Subvec,
    EquivMap,
    SeqIter,
    RangedIterator,
    IHash,
    IEquiv,
    HashProtocol,
    EquivProtocol,
    Type,
    SeqProtocol,
    FirstProtocol,
    RestProtocol,
    NextProtocol,
    CountProtocol,
    NthProtocol,
    LookupProtocol,
    ContainsProtocol,

    MetaProtocol,
    WithMetaProtocol,
    Empty,
    Peek,
    Pop,
    Rseq,
    Keys,
    Vals,
    Repeat,
    Cycle,
    Atom,
    Reset,
    Swap,
    ResetVals,
    SwapVals,
    CompareAndSet,
    SetValidator,
    GetValidator,
    AddWatch,
    RemoveWatch,
    NotifyWatches,
    AddWatchProtocol,
    RemoveWatchProtocol,
    DerefProtocol,
    ResetProtocol,
    SwapProtocol,
    Volatile,
    IsVolatile,
    VReset,
    VResetProtocol,
    AtomType,
    AtomFactory,
    VolatileType,
    VolatileFactory,
    IDeref,
    IAtom,
    IReset,
    ISwap,
    IVolatile,
    IWatchable,
    Update,

    PrStr,
    Str,
    Identical,
    IsUuid,
    IsInst,
    InstMs,
    Uuid,
    RePattern,
    ReFind,
    ReMatches,
    ReSeq,
    IsRegex,
    Vec,
    Concat,
    Sequence,
    Eduction,
    EductionType,
    Apply,
    ArraySeq,

    ApplyTo,
    Constantly,
    Complement,
    Comp,
    Partial,
    Juxt,
    Fnil,
    Cons,
    ListStar,
    Iterate,
    Take,
    Range,
    ToArray,
    Array,
    AGet,
    ASet,
    ALength,
    AClone,
    MakeArray,
    IntoArray,
    JsObj,
    JsKeys,
    JsDelete,
    JsIn,
    JsInvoke,
    UncheckedGet,
    UncheckedSet,
    CheckedAGet,
    CheckedASet,
    CheckedAGetStrict,
    CheckedASetStrict,
    Second,
    Ffirst,
    Nfirst,
    Fnext,
    Nnext,
    Reverse,
    Some,
    NotEvery,
    NotAny,
    NotEmpty,
    Run,
    MapSeq,
    Drop,
    Repeatedly,
    Replicate,
    MapIndexed,
    Mapcat,
    Filter,
    Remove,
    Keep,
    KeepIndexed,
    TakeWhile,
    DropWhile,
    TakeLast,
    DropLast,
    TakeNth,
    SplitAt,
    SplitWith,
    Partition,
    PartitionAll,
    PartitionBy,
    Interleave,
    Interpose,
    Dorun,
    Doall,
    BoundedCount,
    Last,
    Butlast,
    Mapv,
    Filterv,
    ChunkFirst,
    ChunkRest,
    ChunkNext,
    ChunkCons,
    ChunkedConsClass,
    ChunkBuffer,
    ChunkBufferClass,
    ChunkAppend,
    Chunk,
    ArrayChunk,
    IChunk,
    IChunkedNext,
    IDrop,
    DropProtocol,
    DropFirstProtocol,
    ChunkedNextProtocol,
    ChunkedSeq,

    Into,
    IsVar,
    Hash,
    HashOrderedColl,
    HashUnorderedColl,
    HashDouble,
    HashString,
    IntRotateLeft,
    Imul,
    INamed,
    NameProtocol,
    NamespaceProtocol,
    VarClass,
    Test,
    Print,
    PrintFn,
    UncheckedArrays,
    PrintNewline,
    PrintReadably,
    PrintLength,
    RandomUuid,
    IsUri,
    Cat,
    RangeClass,
    IntegerRangeClass,
    NodeSeqClass,
    TreeSeq,
    Flatten,
    Iter,
    Es6IteratorSeq,
    Es6Iterator,
    Demunge,
    MungeStr,
    Munge,
    EncodeClojureProtocol,
    EncodeKeyProtocol,
    EncodeJsProtocol,
    IEncodeClojure,
    IEncodeJs,
    KeyToJs,
    JsToClj,
    CljToJs,
    IsTaggedLiteral,
    TaggedLiteral,
    PrintlnStr,
    PrintStr,
    PrnStr,
    Newline,
    Pr,
    Prn,
    Println,
    PrintNamespaceMaps,
    PrintLevel,
    Iteration,
    JsIterable,
    Shuffle,
    PartitionVAll,
    PartitionV,
    SplitVAt,
    ExceptionInfo,
    ExInfo,
    ExData,
    ExMessage,
    ExCause,
    IsInstance,
    ThrowableToMap,
}

impl CoreFnId {
    pub(crate) fn resolve(name: &str) -> Option<Self> {
        Some(match name {
            "MultiFn" => Self::MultiFn,
            "IMultiFn" => Self::IMultiFn,
            "make-hierarchy" => Self::MakeHierarchy,
            "get-global-hierarchy" => Self::GetGlobalHierarchy,
            "isa?" => Self::Isa,
            "derive" => Self::Derive,
            "underive" => Self::Underive,
            "parents" => Self::Parents,
            "ancestors" => Self::Ancestors,
            "descendants" => Self::Descendants,
            "bases" => Self::Bases,
            "supers" => Self::Supers,
            "remove-all-methods" => Self::RemoveAllMethods,
            "-reset" => Self::ResetMultiFnProtocol,
            "-add-method" => Self::AddMethod,
            "remove-method" => Self::RemoveMethod,
            "-remove-method" => Self::RemoveMethodProtocol,
            "prefer-method" => Self::PreferMethod,
            "-prefer-method" => Self::PreferMethodProtocol,
            "methods" => Self::Methods,
            "-methods" => Self::MethodsProtocol,
            "get-method" => Self::GetMethod,
            "-get-method" => Self::GetMethodProtocol,
            "prefers" => Self::Prefers,
            "-prefers" => Self::PrefersProtocol,
            "default-dispatch-val" => Self::DefaultDispatchVal,
            "-default-dispatch-val" => Self::DefaultDispatchValProtocol,
            "dispatch-fn" => Self::DispatchFn,
            "-dispatch-fn" => Self::DispatchFnProtocol,
            "compiler-edn-read-string" => Self::EdnReadString,
            "compiler-edn-read" => Self::EdnRead,
            "compiler-reader-read-string" => Self::ReaderReadString,
            "compiler-reader-read" => Self::ReaderRead,
            "compiler-reader-string-reader" => Self::ReaderStringReader,
            "compiler-reader-read-line" => Self::ReaderReadLine,
            "compiler-reader-input-provider" => Self::ReaderInputProvider,
            "compiler-reader-parse-timestamp-fields" => Self::ReaderParseTimestampFields,
            "compiler-reader-parse-timestamp" => Self::ReaderParseTimestamp,
            "compiler-reader-initial-tag-table" => Self::ReaderInitialTagTable,
            "compiler-reader-register-tag-parser" => Self::ReaderRegisterTagParser,
            "compiler-reader-deregister-tag-parser" => Self::ReaderDeregisterTagParser,
            "compiler-reader-register-default-tag-parser" => Self::ReaderRegisterDefaultTagParser,
            "compiler-reader-deregister-default-tag-parser" => {
                Self::ReaderDeregisterDefaultTagParser
            }
            "*in*" => Self::ReaderCurrentInputValue,
            "compiler-reader-current-input" => Self::ReaderCurrentInput,
            "compiler-reader-with-input" => Self::ReaderWithInput,
            "compiler-reader-close" => Self::ReaderClose,
            "read-string" => Self::ReadString,
            "read" => Self::Read,
            "read-line" => Self::ReadLine,
            "*1" => Self::ReplOne,
            "*2" => Self::ReplTwo,
            "*3" => Self::ReplThree,
            "*e" => Self::ReplError,
            "*file*" => Self::SourceFile,

            "keyword" => Self::Keyword,
            "symbol" => Self::Symbol,
            "vector" => Self::Vector,
            "list" => Self::List,
            "hash-map" => Self::HashMap,
            "hash-set" => Self::HashSet,
            "dissoc" => Self::Dissoc,
            "disj" => Self::Disj,
            "conj" => Self::Conj,
            "transient" => Self::Transient,
            "persistent!" => Self::PersistentBang,
            "conj!" => Self::ConjBang,
            "assoc!" => Self::AssocBang,
            "dissoc!" => Self::DissocBang,
            "disj!" => Self::DisjBang,
            "pop!" => Self::PopBang,
            "IEditableCollection" => Self::IEditableCollection,
            "ITransientCollection" => Self::ITransientCollection,
            "ITransientAssociative" => Self::ITransientAssociative,
            "ITransientMap" => Self::ITransientMap,
            "ITransientVector" => Self::ITransientVector,
            "ITransientSet" => Self::ITransientSet,
            "-as-transient" => Self::AsTransientProtocol,
            "-persistent!" => Self::PersistentBangProtocol,
            "-conj!" => Self::ConjBangProtocol,
            "-assoc!" => Self::AssocBangProtocol,
            "-dissoc!" => Self::DissocBangProtocol,
            "-disjoin!" => Self::DisjoinBangProtocol,
            "-pop!" => Self::PopBangProtocol,
            "-assoc-n!" => Self::AssocNBangProtocol,
            "assoc" => Self::Assoc,
            "seq" => Self::Seq,
            "seq-to-map-for-destructuring" => Self::SeqToMap,
            "first" => Self::First,
            "rest" => Self::Rest,
            "next" => Self::Next,
            "name" => Self::Name,
            "namespace" => Self::Namespace,
            "subs" => Self::Subs,
            "count" => Self::Count,
            "nth" => Self::Nth,
            "get" => Self::Get,
            "get-in" => Self::GetIn,
            "assoc-in" => Self::AssocIn,
            "update-in" => Self::UpdateIn,
            "select-keys" => Self::SelectKeys,
            "merge" => Self::Merge,
            "merge-with" => Self::MergeWith,
            "some-vals" => Self::SomeVals,
            "req!" => Self::RequiredGet,
            "min-key" => Self::MinKey,
            "max-key" => Self::MaxKey,
            "reductions" => Self::Reductions,
            "replace" => Self::Replace,
            "distinct" => Self::Distinct,
            "distinct?" => Self::IsDistinct,
            "sort" => Self::Sort,
            "sort-by" => Self::SortBy,
            "some-fn" => Self::SomeFn,
            "every-pred" => Self::EveryPred,
            "memoize" => Self::Memoize,
            "rand" => Self::Rand,
            "rand-int" => Self::RandInt,
            "rand-nth" => Self::RandNth,
            "parse-long" => Self::ParseLong,
            "parse-double" => Self::ParseDouble,
            "parse-uuid" => Self::ParseUuid,
            "parse-boolean" => Self::ParseBoolean,
            "js-symbol?" => Self::IsJsSymbol,
            "special-symbol?" => Self::IsSpecialSymbol,
            "to-array-2d" => Self::ToArray2d,
            "int-array" => Self::IntArray,
            "long-array" => Self::LongArray,
            "double-array" => Self::DoubleArray,
            "object-array" => Self::ObjectArray,
            "group-by" => Self::GroupBy,
            "frequencies" => Self::Frequencies,
            "trampoline" => Self::Trampoline,
            "comparator" => Self::Comparator,
            "update-vals" => Self::UpdateVals,
            "update-keys" => Self::UpdateKeys,
            "zipmap" => Self::Zipmap,
            "contains?" => Self::Contains,
            "find" => Self::Find,
            "cloneable?" => Self::IsCloneable,
            "every?" => Self::Every,
            "set" => Self::Set,
            "=" => Self::Equal,
            "reduce" => Self::Reduce,
            "+" => Self::Add,
            "-" => Self::Subtract,
            "*" => Self::Multiply,
            "/" => Self::Divide,
            "inc" => Self::Inc,
            "dec" => Self::Dec,
            "<" => Self::Less,
            "<=" => Self::LessEqual,
            ">" => Self::Greater,
            ">=" => Self::GreaterEqual,
            "==" => Self::NumericEqual,
            "max" => Self::Max,
            "min" => Self::Min,
            "abs" => Self::Abs,
            "int" => Self::Int,
            "long" => Self::Long,
            "byte" => Self::Byte,
            "short" => Self::Short,
            "float" => Self::Float,
            "double" => Self::Double,
            "char" => Self::Char,
            "longs" => Self::Longs,
            "doubles" => Self::Doubles,
            "floats" => Self::Floats,
            "ints" => Self::Ints,
            "shorts" => Self::Shorts,
            "chars" => Self::Chars,
            "bytes" => Self::Bytes,
            "booleans" => Self::Booleans,
            "unchecked-int" => Self::UncheckedInt,
            "unchecked-long" => Self::UncheckedLong,
            "unchecked-byte" => Self::UncheckedByte,
            "unchecked-char" => Self::UncheckedChar,
            "unchecked-short" => Self::UncheckedShort,
            "unchecked-float" => Self::UncheckedFloat,
            "unchecked-double" => Self::UncheckedDouble,
            "unchecked-add" => Self::UncheckedAdd,
            "unchecked-add-int" => Self::UncheckedAddInt,
            "unchecked-subtract" => Self::UncheckedSubtract,
            "unchecked-subtract-int" => Self::UncheckedSubtractInt,
            "unchecked-multiply" => Self::UncheckedMultiply,
            "unchecked-multiply-int" => Self::UncheckedMultiplyInt,
            "unchecked-divide-int" => Self::UncheckedDivideInt,
            "unchecked-inc" => Self::UncheckedInc,
            "unchecked-inc-int" => Self::UncheckedIncInt,
            "unchecked-dec" => Self::UncheckedDec,
            "unchecked-dec-int" => Self::UncheckedDecInt,
            "unchecked-negate" => Self::UncheckedNegate,
            "unchecked-negate-int" => Self::UncheckedNegateInt,
            "unchecked-remainder-int" => Self::UncheckedRemainderInt,
            "js-mod" => Self::JsMod,
            "mod" => Self::Mod,
            "quot" => Self::Quot,
            "rem" => Self::Rem,
            "bit-xor" => Self::BitXor,
            "bit-and" => Self::BitAnd,
            "bit-or" => Self::BitOr,
            "bit-and-not" => Self::BitAndNot,
            "bit-not" => Self::BitNot,
            "bit-clear" => Self::BitClear,
            "bit-flip" => Self::BitFlip,
            "bit-set" => Self::BitSet,
            "bit-test" => Self::BitTest,
            "bit-shift-left" => Self::BitShiftLeft,
            "bit-shift-right" => Self::BitShiftRight,
            "bit-shift-right-zero-fill" => Self::BitShiftRightZeroFill,
            "unsigned-bit-shift-right" => Self::UnsignedBitShiftRight,
            "bit-count" => Self::BitCount,
            "zero?" => Self::IsZero,
            "pos?" => Self::IsPositive,
            "neg?" => Self::IsNegative,
            "even?" => Self::IsEven,
            "odd?" => Self::IsOdd,
            "integer?" => Self::IsInteger,
            "int?" => Self::IsInt,
            "pos-int?" => Self::IsPositiveInt,
            "neg-int?" => Self::IsNegativeInt,
            "nat-int?" => Self::IsNaturalInt,
            "number?" => Self::IsNumber,
            "float?" => Self::IsFloat,
            "double?" => Self::IsDouble,
            "infinite?" => Self::IsInfinite,
            "NaN?" => Self::IsNaN,
            "boolean" => Self::Boolean,
            "boolean?" => Self::IsBoolean,
            "true?" => Self::IsTrue,
            "false?" => Self::IsFalse,
            "nil?" => Self::IsNil,
            "some?" => Self::IsSome,
            "undefined?" => Self::IsUndefined,
            "not" => Self::Not,
            "not=" => Self::NotEqual,
            "identity" => Self::Identity,
            "any?" => Self::IsAny,
            "string?" => Self::IsString,
            "char?" => Self::IsChar,
            "keyword?" => Self::IsKeyword,
            "symbol?" => Self::IsSymbol,
            "ident?" => Self::IsIdent,
            "simple-ident?" => Self::IsSimpleIdent,
            "qualified-ident?" => Self::IsQualifiedIdent,
            "simple-keyword?" => Self::IsSimpleKeyword,
            "qualified-keyword?" => Self::IsQualifiedKeyword,
            "simple-symbol?" => Self::IsSimpleSymbol,
            "qualified-symbol?" => Self::IsQualifiedSymbol,
            "keyword-identical?" => Self::KeywordIdentical,
            "symbol-identical?" => Self::SymbolIdentical,
            "fn?" => Self::IsFn,
            "ifn?" => Self::IsIFn,
            "array?" => Self::IsArray,
            "object?" => Self::IsObject,
            "coll?" => Self::IsColl,
            "list?" => Self::IsList,
            "seq?" => Self::IsSeq,
            "seqable?" => Self::IsSeqable,
            "sequential?" => Self::IsSequential,
            "vector?" => Self::IsVector,
            "map?" => Self::IsMap,
            "set?" => Self::IsSet,
            "associative?" => Self::IsAssociative,
            "ifind?" => Self::IsIFind,
            "counted?" => Self::IsCounted,
            "indexed?" => Self::IsIndexed,
            "reduceable?" => Self::IsReduceable,
            "reversible?" => Self::IsReversible,
            "sorted?" => Self::IsSorted,
            "record?" => Self::IsRecord,
            "map-entry?" => Self::IsMapEntry,
            "chunked-seq?" => Self::IsChunkedSeq,
            "empty?" => Self::IsEmpty,
            "completing" => Self::Completing,
            "transduce" => Self::Transduce,
            "ensure-reduced" => Self::EnsureReduced,
            "halt-when" => Self::HaltWhen,
            "dedupe" => Self::Dedupe,
            "random-sample" => Self::RandomSample,
            "reduced" => Self::Reduced,
            "reduced?" => Self::IsReduced,
            "unreduced" => Self::Unreduced,
            "deref" => Self::Deref,
            "Delay" => Self::Delay,
            "->Delay" => Self::DelayFactory,
            "->LazySeq" => Self::LazySeqFactory,
            "delay?" => Self::IsDelay,
            "force" => Self::Force,
            "realized?" => Self::Realized,
            "IPending" => Self::IPending,
            "-realized?" => Self::RealizedProtocol,
            "nthrest" => Self::Nthrest,
            "nthnext" => Self::Nthnext,
            "meta" => Self::Meta,
            "with-meta" => Self::WithMeta,
            "sorted-map" => Self::SortedMap,
            "sorted-map-by" => Self::SortedMapBy,
            "sorted-set" => Self::SortedSet,
            "sorted-set-by" => Self::SortedSetBy,
            "subseq" => Self::Subseq,
            "rsubseq" => Self::Rsubseq,
            "mk-bound-fn" => Self::MkBoundFn,
            "vary-meta" => Self::VaryMeta,
            "reset-meta!" => Self::ResetMeta,
            "alter-meta!" => Self::AlterMeta,
            "IMeta" => Self::IMeta,
            "IWithMeta" => Self::IWithMeta,
            "IMapEntry" => Self::IMapEntry,
            "IFind" => Self::IFind,
            "ISorted" => Self::ISorted,
            "IChunkedSeq" => Self::IChunkedSeq,
            "-key" | "key" => Self::KeyProtocol,
            "-val" | "val" => Self::ValProtocol,
            "-find" => Self::FindProtocol,
            "-sorted-seq" => Self::SortedSeqProtocol,
            "-sorted-seq-from" => Self::SortedSeqFromProtocol,
            "-entry-key" => Self::EntryKeyProtocol,
            "-comparator" => Self::ComparatorProtocol,
            "-chunked-first" => Self::ChunkedFirstProtocol,
            "-chunked-rest" => Self::ChunkedRestProtocol,
            "-invoke" => Self::InvokeProtocol,
            "write-all" => Self::WriteAll,
            "iterable?" => Self::IsIterable,
            "PersistentVector" => Self::PersistentVector,
            "Subvec" => Self::SubvecType,
            "PersistentArrayMap" => Self::PersistentArrayMap,
            "PersistentHashMap" => Self::PersistentHashMap,
            "PersistentTreeMap" => Self::PersistentTreeMap,
            "PersistentTreeSet" => Self::PersistentTreeSet,
            "PersistentTreeMapSeq" => Self::PersistentTreeMapSeq,
            "PersistentHashSet" => Self::PersistentHashSet,
            "List" => Self::ListType,
            "EmptyList" => Self::EmptyList,
            "Cons" => Self::ConsType,
            "IndexedSeq" => Self::IndexedSeq,
            "LazySeq" => Self::LazySeq,
            "PersistentQueue" => Self::PersistentQueue,
            "Keyword" => Self::KeywordType,
            "Symbol" => Self::SymbolType,
            "Reduced" => Self::ReducedType,
            "UUID" => Self::UUID,
            "MapEntry" | "->MapEntry" => Self::MapEntry,
            "IRecord" => Self::IRecord,
            "ICloneable" => Self::ICloneable,
            "IPrintWithWriter" => Self::IPrintWithWriter,
            "IWriter" => Self::IWriter,
            "clone" => Self::Clone,
            "-clone" => Self::Clone,
            "-pr-writer" => Self::PrWriter,
            "-write" => Self::Write,
            "-flush" => Self::FlushWriter,
            "satisfies?" => Self::Satisfies,
            "implements?" => Self::Implements,
            "PROTOCOL_SENTINEL" => Self::ProtocolSentinel,
            "Fn" => Self::FnMarker,
            "IList" => Self::IList,
            "IFn" => Self::IFn,
            "ISeqable" => Self::ISeqable,
            "ISeq" => Self::ISeq,
            "INext" => Self::INext,
            "ICounted" => Self::ICounted,
            "IIndexed" => Self::IIndexed,
            "ILookup" => Self::ILookup,
            "ISequential" => Self::ISequential,
            "IAssociative" => Self::IAssociative,
            "IReduce" => Self::IReduce,
            "IKVReduce" => Self::IKVReduce,
            "IVector" => Self::IVector,
            "IMap" => Self::IMap,
            "ICollection" => Self::ICollection,
            "IEmptyableCollection" => Self::IEmptyableCollection,
            "ISet" => Self::ISet,
            "IStack" => Self::IStack,
            "IReversible" => Self::IReversible,
            "IComparable" => Self::IComparable,
            "IIterable" => Self::IIterable,
            "-reduce" => Self::ReduceProtocol,
            "-kv-reduce" => Self::KvReduceProtocol,
            "-assoc" => Self::AssocProtocol,
            "-assoc-n" => Self::AssocNProtocol,
            "-dissoc" => Self::DissocProtocol,
            "-conj" => Self::ConjProtocol,
            "-empty" => Self::EmptyProtocol,
            "-disjoin" => Self::DisjoinProtocol,
            "-peek" => Self::PeekProtocol,
            "-pop" => Self::PopProtocol,
            "-rseq" => Self::RseqProtocol,
            "-compare" => Self::CompareProtocol,
            "compare" => Self::Compare,
            "-iterator" => Self::IteratorProtocol,
            "reduce-kv" => Self::ReduceKv,
            "subvec" => Self::Subvec,
            "equiv-map" => Self::EquivMap,
            "SeqIter" => Self::SeqIter,
            "RangedIterator" => Self::RangedIterator,
            "IHash" => Self::IHash,
            "IEquiv" => Self::IEquiv,
            "-hash" => Self::HashProtocol,
            "-equiv" => Self::EquivProtocol,
            "type" => Self::Type,
            "-seq" => Self::SeqProtocol,
            "-first" => Self::FirstProtocol,
            "-rest" => Self::RestProtocol,
            "-next" => Self::NextProtocol,
            "-count" => Self::CountProtocol,
            "-nth" => Self::NthProtocol,
            "-lookup" => Self::LookupProtocol,
            "-contains-key?" => Self::ContainsProtocol,

            "-meta" => Self::MetaProtocol,
            "-with-meta" => Self::WithMetaProtocol,
            "empty" => Self::Empty,
            "peek" => Self::Peek,
            "pop" => Self::Pop,
            "rseq" => Self::Rseq,
            "keys" => Self::Keys,
            "vals" => Self::Vals,
            "repeat" => Self::Repeat,
            "cycle" => Self::Cycle,
            "atom" => Self::Atom,
            "reset!" => Self::Reset,
            "swap!" => Self::Swap,
            "reset-vals!" => Self::ResetVals,
            "swap-vals!" => Self::SwapVals,
            "compare-and-set!" => Self::CompareAndSet,
            "set-validator!" => Self::SetValidator,
            "get-validator" => Self::GetValidator,
            "add-watch" => Self::AddWatch,
            "remove-watch" => Self::RemoveWatch,
            "-notify-watches" => Self::NotifyWatches,
            "-add-watch" => Self::AddWatchProtocol,
            "-remove-watch" => Self::RemoveWatchProtocol,
            "-deref" => Self::DerefProtocol,
            "-reset!" => Self::ResetProtocol,
            "-swap!" => Self::SwapProtocol,
            "volatile!" => Self::Volatile,
            "volatile?" => Self::IsVolatile,
            "vreset!" => Self::VReset,
            "-vreset!" => Self::VResetProtocol,
            "Atom" => Self::AtomType,
            "->Atom" => Self::AtomFactory,
            "Volatile" => Self::VolatileType,
            "->Volatile" => Self::VolatileFactory,
            "IDeref" => Self::IDeref,
            "IAtom" => Self::IAtom,
            "IReset" => Self::IReset,
            "ISwap" => Self::ISwap,
            "IVolatile" => Self::IVolatile,
            "IWatchable" => Self::IWatchable,
            "update" => Self::Update,

            "pr-str" => Self::PrStr,
            "str" | "str_" => Self::Str,
            "identical?" => Self::Identical,
            "uuid?" => Self::IsUuid,
            "inst?" => Self::IsInst,
            "inst-ms" => Self::InstMs,
            "uuid" => Self::Uuid,
            "re-pattern" => Self::RePattern,
            "re-find" => Self::ReFind,
            "re-seq" => Self::ReSeq,
            "re-matches" => Self::ReMatches,
            "regexp?" => Self::IsRegex,
            "vec" => Self::Vec,
            "concat" => Self::Concat,
            "sequence" => Self::Sequence,
            "eduction" => Self::Eduction,
            "Eduction" => Self::EductionType,
            "apply" => Self::Apply,
            "array-seq" => Self::ArraySeq,

            "apply-to" => Self::ApplyTo,
            "constantly" => Self::Constantly,
            "complement" => Self::Complement,
            "comp" => Self::Comp,
            "partial" => Self::Partial,
            "juxt" => Self::Juxt,
            "fnil" => Self::Fnil,
            "cons" => Self::Cons,
            "list*" => Self::ListStar,
            "iterate" => Self::Iterate,
            "take" => Self::Take,
            "range" => Self::Range,
            "to-array" => Self::ToArray,
            "array" => Self::Array,
            "aget" => Self::AGet,
            "aset" => Self::ASet,
            "alength" => Self::ALength,
            "aclone" => Self::AClone,
            "make-array" => Self::MakeArray,
            "into-array" => Self::IntoArray,
            "js-obj" => Self::JsObj,
            "js-keys" => Self::JsKeys,
            "js-delete" => Self::JsDelete,
            "js-in" => Self::JsIn,
            "js-invoke" => Self::JsInvoke,
            "unchecked-get" => Self::UncheckedGet,
            "unchecked-set" => Self::UncheckedSet,
            "checked-aget" => Self::CheckedAGet,
            "checked-aset" => Self::CheckedASet,
            "checked-aget'" => Self::CheckedAGetStrict,
            "checked-aset'" => Self::CheckedASetStrict,

            "second" => Self::Second,
            "ffirst" => Self::Ffirst,
            "nfirst" => Self::Nfirst,
            "fnext" => Self::Fnext,
            "nnext" => Self::Nnext,
            "reverse" => Self::Reverse,
            "some" => Self::Some,
            "not-every?" => Self::NotEvery,
            "not-any?" => Self::NotAny,
            "not-empty" => Self::NotEmpty,
            "run!" => Self::Run,
            "map" => Self::MapSeq,
            "drop" => Self::Drop,
            "repeatedly" => Self::Repeatedly,
            "replicate" => Self::Replicate,
            "map-indexed" => Self::MapIndexed,
            "mapcat" => Self::Mapcat,
            "filter" => Self::Filter,
            "remove" => Self::Remove,
            "keep" => Self::Keep,
            "keep-indexed" => Self::KeepIndexed,
            "take-while" => Self::TakeWhile,
            "drop-while" => Self::DropWhile,
            "take-last" => Self::TakeLast,
            "drop-last" => Self::DropLast,
            "take-nth" => Self::TakeNth,
            "split-at" => Self::SplitAt,
            "split-with" => Self::SplitWith,
            "partition" => Self::Partition,
            "partition-all" => Self::PartitionAll,
            "partition-by" => Self::PartitionBy,
            "interleave" => Self::Interleave,
            "interpose" => Self::Interpose,
            "dorun" => Self::Dorun,
            "doall" => Self::Doall,
            "bounded-count" => Self::BoundedCount,
            "last" => Self::Last,
            "butlast" => Self::Butlast,
            "mapv" => Self::Mapv,
            "filterv" => Self::Filterv,
            "chunk-first" => Self::ChunkFirst,
            "chunk-rest" => Self::ChunkRest,
            "chunk-next" => Self::ChunkNext,
            "chunk-cons" => Self::ChunkCons,
            "ChunkedCons" | "->ChunkedCons" => Self::ChunkedConsClass,
            "ChunkBuffer" | "->ChunkBuffer" => Self::ChunkBufferClass,
            "chunk-buffer" => Self::ChunkBuffer,
            "chunk-append" => Self::ChunkAppend,
            "chunk" => Self::Chunk,
            "ArrayChunk" | "->ArrayChunk" | "array-chunk" => Self::ArrayChunk,
            "IChunk" => Self::IChunk,
            "IChunkedNext" => Self::IChunkedNext,
            "IDrop" => Self::IDrop,
            "-drop" => Self::DropProtocol,
            "-drop-first" => Self::DropFirstProtocol,
            "-chunked-next" => Self::ChunkedNextProtocol,
            "ChunkedSeq" => Self::ChunkedSeq,

            "into" => Self::Into,
            "var?" => Self::IsVar,
            "hash" => Self::Hash,
            "hash-ordered-coll" => Self::HashOrderedColl,
            "hash-unordered-coll" => Self::HashUnorderedColl,
            "hash-double" => Self::HashDouble,
            "hash-string" => Self::HashString,
            "int-rotate-left" => Self::IntRotateLeft,
            "imul" => Self::Imul,
            "INamed" => Self::INamed,
            "-name" => Self::NameProtocol,
            "-namespace" => Self::NamespaceProtocol,

            "Var" | "->Var" => Self::VarClass,
            "test" => Self::Test,
            "print" => Self::Print,
            "*print-fn*" => Self::PrintFn,
            "*unchecked-arrays*" => Self::UncheckedArrays,
            "*print-newline*" => Self::PrintNewline,
            "*print-readably*" => Self::PrintReadably,
            "*print-length*" => Self::PrintLength,
            "random-uuid" => Self::RandomUuid,
            "uri?" => Self::IsUri,
            "cat" => Self::Cat,
            "Range" => Self::RangeClass,
            "IntegerRange" => Self::IntegerRangeClass,
            "NodeSeq" => Self::NodeSeqClass,
            "tree-seq" => Self::TreeSeq,
            "flatten" => Self::Flatten,
            "iter" => Self::Iter,
            "es6-iterator-seq" => Self::Es6IteratorSeq,
            "es6-iterator" => Self::Es6Iterator,
            "demunge" => Self::Demunge,
            "munge-str" => Self::MungeStr,
            "munge" => Self::Munge,
            "-js->clj" => Self::EncodeClojureProtocol,
            "-key->js" => Self::EncodeKeyProtocol,
            "-clj->js" => Self::EncodeJsProtocol,
            "IEncodeClojure" => Self::IEncodeClojure,
            "IEncodeJS" => Self::IEncodeJs,
            "key->js" => Self::KeyToJs,
            "js->clj" => Self::JsToClj,
            "clj->js" => Self::CljToJs,
            "tagged-literal?" => Self::IsTaggedLiteral,
            "tagged-literal" => Self::TaggedLiteral,
            "println-str" => Self::PrintlnStr,
            "print-str" => Self::PrintStr,
            "prn-str" => Self::PrnStr,
            "newline" => Self::Newline,
            "pr" => Self::Pr,
            "prn" => Self::Prn,
            "println" => Self::Println,
            "*print-namespace-maps*" => Self::PrintNamespaceMaps,
            "*print-level*" => Self::PrintLevel,
            "iteration" => Self::Iteration,
            "js-iterable?" => Self::JsIterable,
            "shuffle" => Self::Shuffle,
            "partitionv-all" => Self::PartitionVAll,
            "partitionv" => Self::PartitionV,
            "splitv-at" => Self::SplitVAt,
            "ExceptionInfo" => Self::ExceptionInfo,
            "ex-info" => Self::ExInfo,
            "ex-data" => Self::ExData,
            "ex-message" => Self::ExMessage,
            "ex-cause" => Self::ExCause,
            "instance?" => Self::IsInstance,
            "Throwable->map" => Self::ThrowableToMap,
            "gensym" => Self::Gensym,
            "*command-line-args*" => Self::CommandLineArgs,
            "*print-err-fn*" => Self::PrintErrFn,
            "eval" => Self::Eval,
            "macroexpand" => Self::Macroexpand,
            "macroexpand-1" => Self::Macroexpand1,
            "Namespace" => Self::NamespaceClass,
            "create-ns" => Self::CreateNs,
            "find-ns" => Self::FindNs,
            "ns-name" => Self::NsName,
            "the-ns" => Self::TheNs,
            "all-ns" => Self::AllNs,
            "*ns*" => Self::CurrentNs,
            "*assert*" => Self::Assertions,
            "compiler-macro-var*" => Self::CompilerMacroVar,
            "*exec-tap-fn*" => Self::ExecTapFn,
            "system-time" => Self::SystemTime,
            "add-tap" => Self::AddTap,
            "remove-tap" => Self::RemoveTap,
            "tap>" => Self::TapGreater,
            "load-file" => Self::LoadFile,
            "*print-meta*" => Self::PrintMeta,
            "*print-dup*" => Self::PrintDup,
            "*flush-on-newline*" => Self::FlushOnNewline,
            "set-print-fn!" => Self::SetPrintFn,
            "set-print-err-fn!" => Self::SetPrintErrFn,
            "string-print" => Self::StringPrint,
            "flush" => Self::Flush,
            "pr-str-with-opts" => Self::PrStrWithOpts,
            "prn-str-with-opts" => Self::PrnStrWithOpts,
            "pr-sequential-writer" => Self::PrSequentialWriter,
            "pr-seq-writer" => Self::PrSeqWriter,
            "print-meta?" => Self::PrintMetaQ,
            "enable-console-print!" => Self::EnableConsolePrint,
            "*print-fn-bodies*" => Self::PrintFnBodies,
            "array-map" => Self::ArrayMap,
            _ => return None,
        })
    }

    // Core numeric macros reject malformed direct calls before JS execution.
    // Saved function values still use the ordinary permissive runtime ABI.
    fn accepts_direct_arity(self, arity: usize) -> bool {
        match self {
            Self::SystemTime | Self::AllNs | Self::Flush | Self::EnableConsolePrint => arity == 0,
            Self::FindNs | Self::NsName | Self::TheNs | Self::AddTap | Self::RemoveTap | Self::TapGreater | Self::LoadFile | Self::SetPrintFn | Self::SetPrintErrFn | Self::StringPrint => arity == 1,
            Self::CreateNs => matches!(arity, 1 | 2),
            Self::NamespaceClass | Self::CompilerMacroVar => arity == 2,
            Self::Gensym => arity <= 1,
            Self::Eval | Self::Macroexpand | Self::Macroexpand1 => arity == 1,
            Self::AGet => arity >= 2,
            Self::ASet => arity >= 3,
            Self::UncheckedGet | Self::JsDelete | Self::JsIn => arity == 2,
            Self::UncheckedSet => arity == 3,
            Self::ALength => arity == 1,
            Self::Subtract
            | Self::Divide
            | Self::Less
            | Self::LessEqual
            | Self::Greater
            | Self::GreaterEqual
            | Self::NumericEqual
            | Self::UncheckedSubtract
            | Self::UncheckedSubtractInt
            | Self::UncheckedDivideInt => arity >= 1,
            Self::BitXor | Self::BitAnd | Self::BitOr | Self::BitAndNot => arity >= 2,
            Self::Inc
            | Self::Dec
            | Self::Int
            | Self::Byte
            | Self::Short
            | Self::Float
            | Self::Double
            | Self::UncheckedByte
            | Self::UncheckedChar
            | Self::UncheckedShort
            | Self::UncheckedFloat
            | Self::UncheckedDouble
            | Self::UncheckedInc
            | Self::UncheckedIncInt
            | Self::UncheckedDec
            | Self::UncheckedDecInt
            | Self::UncheckedNegate
            | Self::UncheckedNegateInt
            | Self::BitNot
            | Self::IsZero
            | Self::IsPositive
            | Self::IsNegative
            | Self::IsNil
            | Self::IsSome
            | Self::IsUndefined
            | Self::IsNumber
            | Self::IsTrue
            | Self::IsFalse
            | Self::IsString => arity == 1,
            Self::JsMod
            | Self::UncheckedRemainderInt
            | Self::BitClear
            | Self::BitFlip
            | Self::BitSet
            | Self::BitTest
            | Self::BitShiftLeft
            | Self::BitShiftRight
            | Self::BitShiftRightZeroFill
            | Self::UnsignedBitShiftRight
            | Self::Identical
            | Self::IsInstance => arity == 2,
            _ => true,
        }
    }

    pub(crate) fn runtime_name(self) -> &'static str {
        match self {
            Self::EdnReadString => "ednReadString",
            Self::EdnRead => "ednRead",
            Self::ReaderReadString => "readerReadString",
            Self::ReaderRead => "readerRead",
            Self::ReaderStringReader => "readerStringReader",
            Self::ReaderReadLine => "readerReadLine",
            Self::ReaderInputProvider => "readerInputProvider",
            Self::ReaderParseTimestampFields => "readerParseTimestampFields",
            Self::ReaderParseTimestamp => "readerParseTimestamp",
            Self::ReaderInitialTagTable => "readerInitialTagTable",
            Self::ReaderRegisterTagParser => "readerRegisterTagParser",
            Self::ReaderDeregisterTagParser => "readerDeregisterTagParser",
            Self::ReaderRegisterDefaultTagParser => "readerRegisterDefaultTagParser",
            Self::ReaderDeregisterDefaultTagParser => "readerDeregisterDefaultTagParser",
            Self::ReaderCurrentInputValue => "readerCurrentInputValue",
            Self::ReaderCurrentInput => "readerCurrentInput",
            Self::ReaderWithInput => "readerWithInput",
            Self::ReaderClose => "readerClose",
            Self::ReadString => "readString",
            Self::Read => "read",
            Self::ReadLine => "readLine",

            Self::Gensym => "gensym",
            Self::CommandLineArgs => "commandLineArgs",
            Self::NamespaceClass => "Namespace",
            Self::CreateNs => "createNs",
            Self::FindNs => "findNs",
            Self::NsName => "nsName",
            Self::TheNs => "theNs",
            Self::AllNs => "allNs",
            Self::CurrentNs => "currentNs",
            Self::Assertions => "assertions",
            Self::CompilerMacroVar => "compilerMacroVar",
            Self::ExecTapFn => "execTapFn",
            Self::SystemTime => "systemTime",
            Self::AddTap => "addTap",
            Self::RemoveTap => "removeTap",
            Self::TapGreater => "tapGreater",
            Self::LoadFile => "loadFile",
            Self::PrintMeta => "printMeta",
            Self::PrintDup => "printDup",
            Self::FlushOnNewline => "flushOnNewline",
            Self::SetPrintFn => "setPrintFn",
            Self::SetPrintErrFn => "setPrintErrFn",
            Self::StringPrint => "stringPrint",
            Self::Flush => "flush",
            Self::PrStrWithOpts => "prStrWithOpts",
            Self::PrnStrWithOpts => "prnStrWithOpts",
            Self::PrSequentialWriter => "prSequentialWriter",
            Self::PrSeqWriter => "prSeqWriter",
            Self::PrintMetaQ => "printMetaQ",
            Self::EnableConsolePrint => "enableConsolePrint",
            Self::PrintFnBodies => "printFnBodies",
            Self::PrintErrFn => "printErrFn",
            Self::ReplOne => "replOne",
            Self::ReplTwo => "replTwo",
            Self::ReplThree => "replThree",
            Self::ReplError => "replError",
            Self::SourceFile => "sourceFile",
            Self::Eval => "eval",
            Self::Macroexpand => "macroexpand",
            Self::Macroexpand1 => "macroexpand1",
            Self::MultiFn => "MultiFn",
            Self::IMultiFn => "IMultiFn",
            Self::MakeHierarchy => "makeHierarchy",
            Self::GetGlobalHierarchy => "getGlobalHierarchy",
            Self::Isa => "isa",
            Self::Derive => "derive",
            Self::Underive => "underive",
            Self::Parents => "parents",
            Self::Ancestors => "ancestors",
            Self::Descendants => "descendants",
            Self::Bases => "bases",
            Self::Supers => "supers",
            Self::RemoveAllMethods => "removeAllMethods",
            Self::ResetMultiFnProtocol => "removeAllMethods",
            Self::AddMethod => "addMethod",
            Self::RemoveMethod => "removeMethod",
            Self::RemoveMethodProtocol => "removeMethod",
            Self::PreferMethod => "preferMethod",
            Self::PreferMethodProtocol => "preferMethod",
            Self::Methods => "methods",
            Self::MethodsProtocol => "methods",
            Self::GetMethod => "getMethod",
            Self::GetMethodProtocol => "getMethod",
            Self::Prefers => "prefers",
            Self::PrefersProtocol => "prefers",
            Self::DefaultDispatchVal => "defaultDispatchVal",
            Self::DefaultDispatchValProtocol => "defaultDispatchVal",
            Self::DispatchFn => "dispatchFn",
            Self::DispatchFnProtocol => "dispatchFn",
            Self::Keyword => "keyword",
            Self::Symbol => "symbol",
            Self::Vector => "vector",
            Self::List => "list",
            Self::HashMap => "hashMap",
            Self::ArrayMap => "arrayMap",
            Self::HashSet => "hashSet",
            Self::Dissoc => "dissoc",
            Self::Disj => "disj",
            Self::Conj => "conj",
            Self::Transient => "transient",
            Self::PersistentBang => "persistentBang",
            Self::ConjBang => "conjBang",
            Self::AssocBang => "assocBang",
            Self::DissocBang => "dissocBang",
            Self::DisjBang => "disjBang",
            Self::PopBang => "popBang",
            Self::IEditableCollection => "IEditableCollection",
            Self::ITransientCollection => "ITransientCollection",
            Self::ITransientAssociative => "ITransientAssociative",
            Self::ITransientMap => "ITransientMap",
            Self::ITransientVector => "ITransientVector",
            Self::ITransientSet => "ITransientSet",
            Self::AsTransientProtocol => "asTransientProtocol",
            Self::PersistentBangProtocol => "persistentBangProtocol",
            Self::ConjBangProtocol => "conjBangProtocol",
            Self::AssocBangProtocol => "assocBangProtocol",
            Self::DissocBangProtocol => "dissocBangProtocol",
            Self::DisjoinBangProtocol => "disjoinBangProtocol",
            Self::PopBangProtocol => "popBangProtocol",
            Self::AssocNBangProtocol => "assocNBangProtocol",
            Self::Assoc => "assoc",
            Self::Seq => "seq",
            Self::SeqToMap => "seqToMap",
            Self::First => "first",
            Self::Rest => "rest",
            Self::Next => "next",
            Self::Name => "name",
            Self::Namespace => "namespace",
            Self::Subs => "subs",
            Self::Count => "count",
            Self::Nth => "nth",
            Self::Get => "get",
            Self::GetIn => "getIn",
            Self::AssocIn => "assocIn",
            Self::UpdateIn => "updateIn",
            Self::SelectKeys => "selectKeys",
            Self::Merge => "merge",
            Self::MergeWith => "mergeWith",
            Self::SomeVals => "someVals",
            Self::RequiredGet => "requiredGet",
            Self::MinKey => "minKey",
            Self::MaxKey => "maxKey",
            Self::Reductions => "reductions",
            Self::Replace => "replace",
            Self::Distinct => "distinct",
            Self::IsDistinct => "isDistinct",
            Self::Sort => "sort",
            Self::SortBy => "sortBy",
            Self::SomeFn => "someFn",
            Self::EveryPred => "everyPred",
            Self::Memoize => "memoize",
            Self::Rand => "rand",
            Self::RandInt => "randInt",
            Self::RandNth => "randNth",
            Self::ParseLong => "parseLong",
            Self::ParseDouble => "parseDouble",
            Self::ParseUuid => "parseUuid",
            Self::ParseBoolean => "parseBoolean",
            Self::IsJsSymbol => "isJsSymbol",
            Self::IsSpecialSymbol => "isSpecialSymbol",
            Self::ToArray2d => "toArray2d",
            Self::IntArray => "intArray",
            Self::LongArray => "longArray",
            Self::DoubleArray => "doubleArray",
            Self::ObjectArray => "objectArray",
            Self::GroupBy => "groupBy",
            Self::Frequencies => "frequencies",
            Self::Trampoline => "trampoline",
            Self::Comparator => "comparator",
            Self::UpdateVals => "updateVals",
            Self::UpdateKeys => "updateKeys",
            Self::Zipmap => "zipmap",
            Self::Contains => "contains",
            Self::Find => "find",
            Self::IsCloneable => "isCloneable",
            Self::Every => "every",
            Self::Set => "set",
            Self::Equal => "equal",
            Self::Reduce => "reduce",
            Self::Add => "add",
            Self::Subtract => "subtract",
            Self::Multiply => "multiply",
            Self::Divide => "divide",
            Self::Inc => "inc",
            Self::Dec => "dec",
            Self::Less => "less",
            Self::LessEqual => "lessEqual",
            Self::Greater => "greater",
            Self::GreaterEqual => "greaterEqual",
            Self::NumericEqual => "numericEqual",
            Self::Max => "max",
            Self::Min => "min",
            Self::Abs => "abs",
            Self::Int => "int",
            Self::Long => "long",
            Self::Byte => "byte",
            Self::Short => "short",
            Self::Float => "float",
            Self::Double => "double",
            Self::Char => "char",
            Self::Longs => "longs",
            Self::Doubles => "doubles",
            Self::Floats => "floats",
            Self::Ints => "ints",
            Self::Shorts => "shorts",
            Self::Chars => "chars",
            Self::Bytes => "bytes",
            Self::Booleans => "booleans",
            Self::UncheckedInt => "uncheckedInt",
            Self::UncheckedLong => "uncheckedLong",
            Self::UncheckedByte => "uncheckedByte",
            Self::UncheckedChar => "uncheckedChar",
            Self::UncheckedShort => "uncheckedShort",
            Self::UncheckedFloat => "uncheckedFloat",
            Self::UncheckedDouble => "uncheckedDouble",
            Self::UncheckedAdd => "uncheckedAdd",
            Self::UncheckedAddInt => "uncheckedAddInt",
            Self::UncheckedSubtract => "uncheckedSubtract",
            Self::UncheckedSubtractInt => "uncheckedSubtractInt",
            Self::UncheckedMultiply => "uncheckedMultiply",
            Self::UncheckedMultiplyInt => "uncheckedMultiplyInt",
            Self::UncheckedDivideInt => "uncheckedDivideInt",
            Self::UncheckedInc => "uncheckedInc",
            Self::UncheckedIncInt => "uncheckedIncInt",
            Self::UncheckedDec => "uncheckedDec",
            Self::UncheckedDecInt => "uncheckedDecInt",
            Self::UncheckedNegate => "uncheckedNegate",
            Self::UncheckedNegateInt => "uncheckedNegateInt",
            Self::UncheckedRemainderInt => "uncheckedRemainderInt",
            Self::JsMod => "jsMod",
            Self::Mod => "mod",
            Self::Quot => "quot",
            Self::Rem => "rem",
            Self::BitXor => "bitXor",
            Self::BitAnd => "bitAnd",
            Self::BitOr => "bitOr",
            Self::BitAndNot => "bitAndNot",
            Self::BitNot => "bitNot",
            Self::BitClear => "bitClear",
            Self::BitFlip => "bitFlip",
            Self::BitSet => "bitSet",
            Self::BitTest => "bitTest",
            Self::BitShiftLeft => "bitShiftLeft",
            Self::BitShiftRight => "bitShiftRight",
            Self::BitShiftRightZeroFill => "bitShiftRightZeroFill",
            Self::UnsignedBitShiftRight => "unsignedBitShiftRight",
            Self::BitCount => "bitCount",
            Self::IsZero => "isZero",
            Self::IsPositive => "isPositive",
            Self::IsNegative => "isNegative",
            Self::IsEven => "isEven",
            Self::IsOdd => "isOdd",
            Self::IsInteger => "isInteger",
            Self::IsInt => "isInt",
            Self::IsPositiveInt => "isPositiveInt",
            Self::IsNegativeInt => "isNegativeInt",
            Self::IsNaturalInt => "isNaturalInt",
            Self::IsNumber => "isNumber",
            Self::IsFloat => "isFloat",
            Self::IsDouble => "isDouble",
            Self::IsInfinite => "isInfinite",
            Self::IsNaN => "isNaN",
            Self::Boolean => "boolean",
            Self::IsBoolean => "isBoolean",
            Self::IsTrue => "isTrue",
            Self::IsFalse => "isFalse",
            Self::IsNil => "isNil",
            Self::IsSome => "isSome",
            Self::IsUndefined => "isUndefined",
            Self::Not => "not",
            Self::NotEqual => "notEqual",
            Self::Identity => "identity",
            Self::IsAny => "isAny",
            Self::IsString => "isString",
            Self::IsChar => "isChar",
            Self::IsKeyword => "isKeyword",
            Self::IsSymbol => "isSymbol",
            Self::IsIdent => "isIdent",
            Self::IsSimpleIdent => "isSimpleIdent",
            Self::IsQualifiedIdent => "isQualifiedIdent",
            Self::IsSimpleKeyword => "isSimpleKeyword",
            Self::IsQualifiedKeyword => "isQualifiedKeyword",
            Self::IsSimpleSymbol => "isSimpleSymbol",
            Self::IsQualifiedSymbol => "isQualifiedSymbol",
            Self::KeywordIdentical => "keywordIdentical",
            Self::SymbolIdentical => "symbolIdentical",
            Self::IsFn => "isFn",
            Self::IsIFn => "isIFn",
            Self::IsArray => "isArray",
            Self::IsObject => "isObject",
            Self::IsColl => "isColl",
            Self::IsList => "isList",
            Self::IsSeq => "isSeq",
            Self::IsSeqable => "isSeqable",
            Self::IsSequential => "isSequential",
            Self::IsVector => "isVector",
            Self::IsMap => "isMap",
            Self::IsSet => "isSet",
            Self::IsAssociative => "isAssociative",
            Self::IsIFind => "isIFind",
            Self::IsCounted => "isCounted",
            Self::IsIndexed => "isIndexed",
            Self::IsReduceable => "isReduceable",
            Self::IsReversible => "isReversible",
            Self::IsSorted => "isSorted",
            Self::IsRecord => "isRecord",
            Self::IsMapEntry => "isMapEntry",
            Self::IsChunkedSeq => "isChunkedSeq",
            Self::IsEmpty => "isEmpty",
            Self::Completing => "completing",
            Self::Transduce => "transduce",
            Self::EnsureReduced => "ensureReduced",
            Self::HaltWhen => "haltWhen",
            Self::Dedupe => "dedupe",
            Self::RandomSample => "randomSample",
            Self::Reduced => "reduced",
            Self::IsReduced => "isReduced",
            Self::Unreduced => "unreduced",
            Self::Deref => "deref",
            Self::Delay => "Delay",
            Self::DelayFactory => "Delay",
            Self::LazySeqFactory => "LazySeq",
            Self::IsDelay => "isDelay",
            Self::Force => "force",
            Self::Realized => "realized",
            Self::IPending => "IPending",
            Self::RealizedProtocol => "realized",
            Self::Nthrest => "nthrest",
            Self::Nthnext => "nthnext",
            Self::Meta => "meta",
            Self::WithMeta => "withMeta",
            Self::SortedMap => "sortedMap",
            Self::SortedMapBy => "sortedMapBy",
            Self::SortedSet => "sortedSet",
            Self::SortedSetBy => "sortedSetBy",
            Self::Subseq => "subseq",
            Self::Rsubseq => "rsubseq",
            Self::MkBoundFn => "mkBoundFn",
            Self::VaryMeta => "varyMeta",
            Self::ResetMeta => "resetMeta",
            Self::AlterMeta => "alterMeta",
            Self::IMeta => "IMeta",
            Self::IWithMeta => "IWithMeta",
            Self::IMapEntry => "IMapEntry",
            Self::IFind => "IFind",
            Self::ISorted => "ISorted",
            Self::IChunkedSeq => "IChunkedSeq",
            Self::KeyProtocol => "keyProtocol",
            Self::ValProtocol => "valProtocol",
            Self::FindProtocol => "findProtocol",
            Self::SortedSeqProtocol => "sortedSeqProtocol",
            Self::SortedSeqFromProtocol => "sortedSeqFromProtocol",
            Self::EntryKeyProtocol => "entryKeyProtocol",
            Self::ComparatorProtocol => "comparatorProtocol",
            Self::ChunkedFirstProtocol => "chunkedFirstProtocol",
            Self::ChunkedRestProtocol => "chunkedRestProtocol",
            Self::InvokeProtocol => "invokeProtocol",
            Self::WriteAll => "writeAll",
            Self::IsIterable => "isIterable",
            Self::PersistentVector => "PersistentVector",
            Self::SubvecType => "Subvec",
            Self::PersistentArrayMap => "PersistentArrayMap",
            Self::PersistentHashMap => "PersistentHashMap",
            Self::PersistentTreeMap => "PersistentTreeMap",
            Self::PersistentTreeSet => "PersistentTreeSet",
            Self::PersistentTreeMapSeq => "PersistentTreeMapSeq",
            Self::PersistentHashSet => "PersistentHashSet",
            Self::ListType => "List",
            Self::EmptyList => "EmptyList",
            Self::ConsType => "Cons",
            Self::IndexedSeq => "IndexedSeq",
            Self::LazySeq => "LazySeq",
            Self::PersistentQueue => "PersistentQueue",
            Self::KeywordType => "Keyword",
            Self::SymbolType => "Symbol",
            Self::ReducedType => "Reduced",
            Self::UUID => "UUID",
            Self::MapEntry => "MapEntry",
            Self::IRecord => "IRecord",
            Self::ICloneable => "ICloneable",
            Self::IPrintWithWriter => "IPrintWithWriter",
            Self::IWriter => "IWriter",
            Self::Clone => "clone",
            Self::PrWriter => "prWriter",
            Self::Write => "write",
            Self::FlushWriter => "flushWriter",
            Self::Satisfies => "satisfies",
            Self::Implements => "implements",
            Self::ProtocolSentinel => "protocolSentinel",
            Self::FnMarker => "Fn",
            Self::IList => "IList",
            Self::IFn => "IFn",
            Self::ISeqable => "ISeqable",
            Self::ISeq => "ISeq",
            Self::INext => "INext",
            Self::ICounted => "ICounted",
            Self::IIndexed => "IIndexed",
            Self::ILookup => "ILookup",
            Self::ISequential => "ISequential",
            Self::IAssociative => "IAssociative",
            Self::IReduce => "IReduce",
            Self::IKVReduce => "IKVReduce",
            Self::IVector => "IVector",
            Self::IMap => "IMap",
            Self::ICollection => "ICollection",
            Self::IEmptyableCollection => "IEmptyableCollection",
            Self::ISet => "ISet",
            Self::IStack => "IStack",
            Self::IReversible => "IReversible",
            Self::IComparable => "IComparable",
            Self::IIterable => "IIterable",
            Self::ReduceProtocol => "reduceProtocol",
            Self::KvReduceProtocol => "kvReduceProtocol",
            Self::AssocProtocol => "assocProtocol",
            Self::AssocNProtocol => "assocNProtocol",
            Self::DissocProtocol => "dissocProtocol",
            Self::ConjProtocol => "conjProtocol",
            Self::EmptyProtocol => "emptyProtocol",
            Self::DisjoinProtocol => "disjoinProtocol",
            Self::PeekProtocol => "peekProtocol",
            Self::PopProtocol => "popProtocol",
            Self::RseqProtocol => "rseqProtocol",
            Self::CompareProtocol => "compareProtocol",
            Self::Compare => "compare",
            Self::IteratorProtocol => "iteratorProtocol",
            Self::ReduceKv => "reduceKv",
            Self::Subvec => "subvec",
            Self::EquivMap => "equivMap",
            Self::SeqIter => "SeqIter",
            Self::RangedIterator => "RangedIterator",
            Self::IHash => "IHash",
            Self::IEquiv => "IEquiv",
            Self::HashProtocol => "hashProtocol",
            Self::EquivProtocol => "equivProtocol",
            Self::Type => "type",
            Self::SeqProtocol => "seqProtocol",
            Self::FirstProtocol => "firstProtocol",
            Self::RestProtocol => "restProtocol",
            Self::NextProtocol => "nextProtocol",
            Self::CountProtocol => "countProtocol",
            Self::NthProtocol => "nthProtocol",
            Self::LookupProtocol => "lookupProtocol",
            Self::ContainsProtocol => "containsProtocol",

            Self::MetaProtocol => "metaProtocol",
            Self::WithMetaProtocol => "withMetaProtocol",
            Self::Empty => "empty",
            Self::Peek => "peek",
            Self::Pop => "pop",
            Self::Rseq => "rseq",
            Self::Keys => "keys",
            Self::Vals => "vals",
            Self::Repeat => "repeat",
            Self::Cycle => "cycle",
            Self::Atom => "atom",
            Self::Reset => "reset",
            Self::Swap => "swap",
            Self::ResetVals => "resetVals",
            Self::SwapVals => "swapVals",
            Self::CompareAndSet => "compareAndSet",
            Self::SetValidator => "setValidator",
            Self::GetValidator => "getValidator",
            Self::AddWatch => "addWatch",
            Self::RemoveWatch => "removeWatch",
            Self::NotifyWatches => "notifyWatches",
            Self::AddWatchProtocol => "addWatchProtocol",
            Self::RemoveWatchProtocol => "removeWatchProtocol",
            Self::DerefProtocol => "derefProtocol",
            Self::ResetProtocol => "resetProtocol",
            Self::SwapProtocol => "swapProtocol",
            Self::Volatile => "volatile",
            Self::IsVolatile => "isVolatile",
            Self::VReset => "vreset",
            Self::VResetProtocol => "vresetProtocol",
            Self::AtomType => "Atom",
            Self::AtomFactory => "atomFactory",
            Self::VolatileType => "Volatile",
            Self::VolatileFactory => "volatileFactory",
            Self::IDeref => "IDeref",
            Self::IAtom => "IAtom",
            Self::IReset => "IReset",
            Self::ISwap => "ISwap",
            Self::IVolatile => "IVolatile",
            Self::IWatchable => "IWatchable",
            Self::Update => "update",

            Self::PrStr => "prStr",
            Self::Str => "str",
            Self::Identical => "identical",
            Self::IsUuid => "isUuid",
            Self::IsInst => "isInst",
            Self::InstMs => "instMs",
            Self::Uuid => "uuid",
            Self::RePattern => "rePattern",
            Self::ReFind => "reFind",
            Self::ReSeq => "reSeq",
            Self::ReMatches => "reMatches",
            Self::IsRegex => "isRegex",
            Self::Vec => "vec",
            Self::Concat => "concat",
            Self::Sequence => "sequence",
            Self::Eduction => "eduction",
            Self::EductionType => "Eduction",
            Self::Apply => "apply",
            Self::ArraySeq => "arraySeq",

            Self::ApplyTo => "applyTo",
            Self::Constantly => "constantly",
            Self::Complement => "complement",
            Self::Comp => "comp",
            Self::Partial => "partial",
            Self::Juxt => "juxt",
            Self::Fnil => "fnil",
            Self::Cons => "cons",
            Self::ListStar => "listStar",
            Self::Iterate => "iterate",
            Self::Take => "take",
            Self::Range => "range",
            Self::ToArray => "toArray",
            Self::Array => "array",
            Self::AGet => "aget",
            Self::ASet => "aset",
            Self::ALength => "alength",
            Self::AClone => "aclone",
            Self::MakeArray => "makeArray",
            Self::IntoArray => "intoArray",
            Self::JsObj => "jsObj",
            Self::JsKeys => "jsKeys",
            Self::JsDelete => "jsDelete",
            Self::JsIn => "jsIn",
            Self::JsInvoke => "jsInvoke",
            Self::UncheckedGet => "uncheckedGet",
            Self::UncheckedSet => "uncheckedSet",
            Self::CheckedAGet => "checkedAget",
            Self::CheckedASet => "checkedAset",
            Self::CheckedAGetStrict => "checkedAgetStrict",
            Self::CheckedASetStrict => "checkedAsetStrict",

            Self::Second => "second",
            Self::Ffirst => "ffirst",
            Self::Nfirst => "nfirst",
            Self::Fnext => "fnext",
            Self::Nnext => "nnext",
            Self::Reverse => "reverse",
            Self::Some => "some",
            Self::NotEvery => "notEvery",
            Self::NotAny => "notAny",
            Self::NotEmpty => "notEmpty",
            Self::Run => "run",
            Self::MapSeq => "mapSeq",
            Self::Drop => "drop",
            Self::Repeatedly => "repeatedly",
            Self::Replicate => "replicate",
            Self::MapIndexed => "mapIndexed",
            Self::Mapcat => "mapcat",
            Self::Filter => "filter",
            Self::Remove => "remove",
            Self::Keep => "keep",
            Self::KeepIndexed => "keepIndexed",
            Self::TakeWhile => "takeWhile",
            Self::DropWhile => "dropWhile",
            Self::TakeLast => "takeLast",
            Self::DropLast => "dropLast",
            Self::TakeNth => "takeNth",
            Self::SplitAt => "splitAt",
            Self::SplitWith => "splitWith",
            Self::Partition => "partition",
            Self::PartitionAll => "partitionAll",
            Self::PartitionBy => "partitionBy",
            Self::Interleave => "interleave",
            Self::Interpose => "interpose",
            Self::Dorun => "dorun",
            Self::Doall => "doall",
            Self::BoundedCount => "boundedCount",
            Self::Last => "last",
            Self::Butlast => "butlast",
            Self::Mapv => "mapv",
            Self::Filterv => "filterv",
            Self::ChunkFirst => "chunkFirst",
            Self::ChunkRest => "chunkRest",
            Self::ChunkNext => "chunkNext",
            Self::ChunkCons => "chunkCons",
            Self::ChunkedConsClass => "ChunkedCons",
            Self::ChunkBuffer => "chunkBuffer",
            Self::ChunkBufferClass => "ChunkBuffer",
            Self::ChunkAppend => "chunkAppend",
            Self::Chunk => "chunk",
            Self::ArrayChunk => "arrayChunk",
            Self::IChunk => "IChunk",
            Self::IChunkedNext => "IChunkedNext",
            Self::IDrop => "IDrop",
            Self::DropProtocol => "dropProtocol",
            Self::DropFirstProtocol => "dropFirstProtocol",
            Self::ChunkedNextProtocol => "chunkedNextProtocol",
            Self::ChunkedSeq => "ChunkedSeq",

            Self::Into => "into",
            Self::IsVar => "isVar",
            Self::Hash => "hash",
            Self::HashOrderedColl => "hashOrderedColl",
            Self::HashUnorderedColl => "hashUnorderedColl",
            Self::HashDouble => "hashDouble",
            Self::HashString => "stringHash",
            Self::IntRotateLeft => "rotateLeft",
            Self::Imul => "imul",
            Self::INamed => "INamed",
            Self::NameProtocol => "nameProtocol",
            Self::NamespaceProtocol => "namespaceProtocol",

            Self::VarClass => "Var",
            Self::Test => "test",
            Self::Print => "printCore",
            Self::PrintFn => "printFn",
            Self::UncheckedArrays => "uncheckedArrays",
            Self::PrintNewline => "printNewline",
            Self::PrintReadably => "printReadably",
            Self::PrintLength => "printLength",
            Self::RandomUuid => "randomUuid",
            Self::IsUri => "isUri",
            Self::Cat => "cat",
            Self::RangeClass => "Range",
            Self::IntegerRangeClass => "IntegerRange",
            Self::NodeSeqClass => "NodeSeq",
            Self::TreeSeq => "treeSeq",
            Self::Flatten => "flatten",
            Self::Iter => "iter",
            Self::Es6IteratorSeq => "es6IteratorSeq",
            Self::Es6Iterator => "es6Iterator",
            Self::Demunge => "demunge",
            Self::MungeStr => "mungeStr",
            Self::Munge => "munge",
            Self::EncodeClojureProtocol => "encodeClojureProtocol",
            Self::EncodeKeyProtocol => "encodeKeyProtocol",
            Self::EncodeJsProtocol => "encodeJsProtocol",
            Self::IEncodeClojure => "IEncodeClojure",
            Self::IEncodeJs => "IEncodeJS",
            Self::KeyToJs => "keyToJs",
            Self::JsToClj => "jsToClj",
            Self::CljToJs => "cljToJs",
            Self::IsTaggedLiteral => "isTaggedLiteral",
            Self::TaggedLiteral => "taggedLiteral",
            Self::PrintlnStr => "printlnStr",
            Self::PrintStr => "printStr",
            Self::PrnStr => "prnStr",
            Self::Newline => "newline",
            Self::Pr => "pr",
            Self::Prn => "prn",
            Self::Println => "println",
            Self::PrintNamespaceMaps => "printNamespaceMaps",
            Self::PrintLevel => "printLevel",
            Self::Iteration => "iteration",
            Self::JsIterable => "jsIterable",
            Self::Shuffle => "shuffle",
            Self::PartitionVAll => "partitionVAll",
            Self::PartitionV => "partitionV",
            Self::SplitVAt => "splitVAt",
            Self::ExceptionInfo => "ExceptionInfo",
            Self::ExInfo => "exInfo",
            Self::ExData => "exData",
            Self::ExMessage => "exMessage",
            Self::ExCause => "exCause",
            Self::IsInstance => "isInstance",
            Self::ThrowableToMap => "throwableToMap",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct GlobalId(pub usize);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct JsGlobalId(pub usize);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct RecurTargetId(pub usize);

#[derive(Clone, Debug, PartialEq)]
pub struct RecurTarget {
    pub params: Vec<LocalId>,
    pub protocol_impl: bool,
    pub receiver_from_this: bool,
    pub used: bool,
    pub argument_slots: Vec<Option<usize>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GlobalBinding {
    /// Active source generation is a type/record descriptor, not an alias.
    pub type_descriptor: bool,
    /// Existence probes may reserve storage before an actual definition.
    pub declared: bool,
    pub constant: bool,
    pub boolean_tag: bool,
    pub metadata: Option<ExprId>,
    pub namespace: String,
    pub name: String,
    pub span: Span,
    pub source_index: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JsGlobal {
    /// Validated ASCII JavaScript identifiers, never raw source interpolation.
    pub path: Vec<String>,
}

/// A namespace whose dependencies have already been placed earlier in the slice.
/// The caller validates its first `ns` form and the dependency graph.
pub struct NamespaceInput<'a> {
    pub name: &'a str,
    pub source: &'a str,
    pub syntax: &'a SyntaxTree,
    pub aliases: &'a [(String, String)],
    pub requires: &'a [String],
}

/// Both analysis (including native macro expansion) and emission track value use.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExprContext {
    Value,
    Discard,
    Return,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    Nil,
    Bool(bool),
    Number(f64),
    String(StringId),
    ReaderLiteral {
        kind: CompilerLiteralKind,
        items: Vec<ExprId>,
        /// Quoted #js data keeps its explicit compiler form brand on export.
        quoted: bool,
    },
    WithMeta {
        value: ExprId,
        metadata: ExprId,
    },
    Var {
        definition: bool,
        value: ExprId,
        symbol: NamedLiteralId,
        metadata: Option<ExprId>,
    },
    Keyword(NamedLiteralId),
    Symbol(NamedLiteralId),
    List(Vec<ExprId>),
    Vector(Vec<ExprId>),
    Map(Vec<ExprId>),
    Set(Vec<ExprId>),
    CoreFn(CoreFnId),
    Local(LocalId),
    Global(GlobalId),
    JsGlobal(JsGlobalId),
    ThisAs {
        binding: LocalId,
        body: ExprId,
    },
    JsArguments,
    JsTemplate {
        segments: Vec<String>,
        args: Vec<ExprId>,
    },
    JsIndex {
        receiver: ExprId,
        indices: Vec<ExprId>,
    },
    JsDelete {
        receiver: ExprId,
        key: ExprId,
    },
    QualifiedMethod {
        class: ExprId,
        method: Option<String>,
    },
    CopyArguments(ExprId),
    DefProtocol {
        global: GlobalId,
        methods: Vec<ProtocolMethod>,
        via_metadata: bool,
    },
    ProtocolPredicate {
        protocol: ExprId,
        value: ExprId,
        key: String,
        direct: bool,
    },
    ExtendType {
        instance: bool,
        return_target: bool,
        target: ExprId,
        methods: Vec<(MethodDispatch, FnMethod)>,
    },
    Reify {
        metadata: Option<ExprId>,
        captures: Vec<LocalId>,
        methods: Vec<(MethodDispatch, FnMethod)>,
    },
    DefType {
        record: bool,
        global: GlobalId,
        constructor: Option<GlobalId>,
        body: Option<ExprId>,
        masks: Option<ExprId>,
        map_constructor: Option<GlobalId>,
        fields: Vec<String>,
        methods: Vec<(MethodDispatch, FnMethod)>,
    },
    Construct {
        callee: ExprId,
        args: Vec<ExprId>,
    },
    Assert {
        test: ExprId,
        message: Option<ExprId>,
        form: StringId,
    },
    Def {
        global: GlobalId,
        init: Option<ExprId>,
        test: Option<ExprId>,
        once: bool,
    },
    SetGlobal {
        target: ExprId,
        value: ExprId,
    },
    Binding {
        bindings: Vec<(ExprId, ExprId)>,
        body: ExprId,
    },
    WithOutStr(ExprId),
    Exists(ExprId),
    /// Resolve checks binding identity in the compiler, value availability in applications.
    BindingExists(ExprId),
    BooleanHint(ExprId),
    Property {
        receiver: ExprId,
        property: String,
    },
    MethodCall {
        receiver: ExprId,
        method: String,
        args: Vec<ExprId>,
    },
    If {
        test: ExprId,
        yes: ExprId,
        no: ExprId,
    },
    Do(Vec<ExprId>),
    Let {
        bindings: Vec<(LocalId, ExprId)>,
        body: ExprId,
    },
    LetFn {
        bindings: Vec<(LocalId, ExprId)>,
        body: ExprId,
    },
    Loop {
        target: RecurTargetId,
        bindings: Vec<(LocalId, ExprId)>,
        body: ExprId,
    },
    Recur {
        target: RecurTargetId,
        args: Vec<ExprId>,
    },
    Throw(ExprId),
    Try {
        body: ExprId,
        catches: Vec<CatchClause>,
        finally: Option<ExprId>,
    },
    Case {
        strict: bool,
        keyword_tests: bool,
        test: ExprId,
        branches: Vec<(ExprId, ExprId)>,
        default: Option<ExprId>,
    },
    Fn {
        name: Option<LocalId>,
        methods: Vec<FnMethod>,
    },
    Call {
        callee: ExprId,
        args: Vec<ExprId>,
    },
    InstanceOf {
        class: ExprId,
        value: ExprId,
    },
}

/// Each overload owns a separate parameter scope and shares the function name.
#[derive(Clone, Debug, PartialEq)]
pub struct FnMethod {
    pub params: Vec<LocalId>,
    pub rest: Option<LocalId>,
    pub body: ExprId,
    pub target: RecurTargetId,
    pub captures: Vec<LocalId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CatchClause {
    pub class: Option<ExprId>,
    pub binding: LocalId,
    pub body: ExprId,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum MethodKind {
    Function,
    IFn,
    Protocol,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProtocolMethod {
    pub global: GlobalId,
    pub key: String,
    pub arities: Vec<usize>,
    pub variadic: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MethodDispatch {
    Protocol(String),
    Builtin(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
    pub context: ExprContext,
    pub source_index: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalBinding {
    /// Diagnostic spelling only. References and emitted names use LocalId.
    pub name: String,
    pub span: Span,
    pub source_index: usize,
}

/// Compilation demand is recorded separately from application reachability.
/// A generation can be analyzed for final output without ever being executed
/// by a macro. These owned facts contain no evaluator handles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompilationUse {
    pub name: String,
    pub generation: usize,
    pub analyses: usize,
    pub emissions: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeReaderNamespace {
    pub aliases: Vec<(String, String)>,
    pub refers: Vec<(String, String, String)>,
    pub excludes: Vec<String>,
    pub macros: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    /// Source namespaces present in this unit, including namespaces without Vars.
    pub namespaces: Vec<String>,
    /// Explicit source reader contexts; runtime reads never consult a loader.
    pub reader_contexts: HashMap<String, RuntimeReaderNamespace>,
    pub namespace_entries: HashMap<ExprId, String>,
    pub compilation_uses: Vec<CompilationUse>,
    pub exprs: Vec<Expr>,
    pub locals: Vec<LocalBinding>,
    pub recur_targets: Vec<RecurTarget>,
    pub protocols: HashMap<GlobalId, Vec<ProtocolMethod>>,
    pub record_constructors: std::collections::HashSet<GlobalId>,
    pub field_bindings: HashMap<LocalId, (RecurTargetId, bool)>,
    pub type_methods: std::collections::HashSet<RecurTargetId>,
    /// Runtime macro functions have two implicit parameters but public user arities.
    pub macro_functions: std::collections::HashSet<ExprId>,
    pub globals: Vec<GlobalBinding>,
    pub js_globals: Vec<JsGlobal>,
    /// Owned UTF-16 preserves even lone surrogate escapes.
    pub strings: Vec<Vec<u16>>,
    pub named_literals: Vec<NamedLiteral>,
    pub root: ExprId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AnalysisErrorKind {
    UnresolvedSymbol(String),
    Unsupported(String),
    InvalidForm(&'static str),
    InvalidBinding,
    DuplicateParameter(String),
    NumberOutOfRange,
    MacroExpansion(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnalysisError {
    pub kind: AnalysisErrorKind,
    pub span: Span,
}

/// Public source diagnostics are separate from fatal analysis errors. Callers
/// retain warnings emitted before a later error by supplying an output vector.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnalysisWarning {
    pub kind: &'static str,
    pub message: String,
    pub span: Span,
    pub source_index: usize,
}

/// Runtime validation for direct source aget/aset calls. Saved functions retain
/// their ordinary runtime behavior, as in upstream macro expansion.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CheckedArrays {
    #[default]
    Off,
    Warn,
    Error,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AnalysisOptions {
    /// CLJS permits undeclared Vars and conflicting function arities with a
    /// warning. Existing strict entry points retain their error contracts.
    pub warning_mode: bool,
    /// Preserve the existing opt-in static array diagnostics.
    pub checked_arrays: bool,
    /// :checked-arrays :warn/:error runtime validation (also enables diagnostics).
    pub array_checks: CheckedArrays,
}

impl fmt::Display for AnalysisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            AnalysisErrorKind::UnresolvedSymbol(name) => write!(f, "unresolved symbol: {name}"),
            AnalysisErrorKind::Unsupported(what) => write!(f, "unsupported construct: {what}"),
            AnalysisErrorKind::InvalidForm(message) => f.write_str(message),
            AnalysisErrorKind::InvalidBinding => {
                f.write_str("binding must be an unqualified symbol without '.' and cannot be '&'")
            }
            AnalysisErrorKind::DuplicateParameter(name) => write!(f, "duplicate parameter: {name}"),
            AnalysisErrorKind::MacroExpansion(message) => write!(f, "macro expansion: {message}"),
            AnalysisErrorKind::NumberOutOfRange => {
                f.write_str("number is not a valid ClojureScript constant")
            }
        }
    }
}

impl std::error::Error for AnalysisError {}

// JVM reader data identity precedes JavaScript lowering: integers and doubles
// remain distinct, while vectors/lists compare by contents and maps unordered.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum ReaderDatum {
    Nil,
    Bool(bool),
    Number(u8, String),
    ReaderLiteral(u8, String),
    String(Vec<u16>),
    Named(bool, Option<Vec<u16>>, Vec<u16>),
    Sequential(Vec<ReaderDatum>),
    Map(Vec<(ReaderDatum, ReaderDatum)>),
    Set(Vec<ReaderDatum>),
}

/// A scope holds IDs, while records live for the entire compilation unit.
#[derive(Default)]
struct Environment {
    scopes: Vec<Vec<LocalId>>,
}

#[derive(Clone, Default)]
pub(crate) struct NamespaceResolution {
    pub refers: Vec<(String, String, String)>,
    pub excludes: Vec<String>,
    pub js_refers: Vec<(String, String)>,
    pub cljc: bool,
    pub source_name: Option<String>,
}

struct Analyzer<'a> {
    source: &'a str,
    syntax: Cow<'a, SyntaxTree>,
    generated: Vec<Option<Form>>,
    discards: Vec<NodeId>,
    next_discard: usize,
    spellings: Vec<Option<String>>,
    numbers: Vec<Option<Option<f64>>>,
    local_kinds: HashMap<LocalId, (&'static str, Option<usize>)>,
    type_fields: Option<Vec<(String, bool)>>,
    raw_type: bool,
    macro_host: &'a mut MacroHost,
    engine: Option<&'a dyn crate::evaluator::MacroEvaluator>,
    deferred: Vec<(ExprId, usize)>,
    macro_depth: usize,
    analysis_depth: usize,
    macro_source: Option<Source>,
    program: Program,
    env: Environment,
    recur_target: Option<RecurTargetId>,
    globals: HashMap<(String, String), GlobalId>,
    namespace: &'a str,
    aliases: &'a [(String, String)],
    requires: &'a [String],
    source_index: usize,
    resolution: NamespaceResolution,
    options: AnalysisOptions,
    unchecked_arrays: bool,
    checked_array_exprs: std::collections::HashSet<ExprId>,
    warnings: &'a mut Vec<AnalysisWarning>,
    resolving_var: bool,
    definition_name: Option<String>,
    top_level: bool,
}

fn compiler_syntax<'a>(source: &str, syntax: &'a SyntaxTree) -> (Cow<'a, SyntaxTree>, Vec<NodeId>) {
    if !source.contains("#_")
        || !crate::Tokenizer::new(source).any(|token| token.kind == crate::TokenKind::Discard)
    {
        return (Cow::Borrowed(syntax), Vec::new());
    }
    // Only original source is parsed; semantic/native values are never reparsed.
    let mut parser = crate::Parser::new(source).retaining_discards();
    let mut roots = Vec::new();
    while let Some(root) = parser.read_form().expect("already validated source") {
        roots.push(root);
    }
    let mut discards = parser.discarded().to_vec();
    discards.sort_by_key(|id| parser.nodes()[id.0].span.start);
    let mut nodes = parser.into_nodes();
    // Namespace preparation may have replaced top-level require forms with nil.
    // Retaining discarded forms must preserve those prepared operations.
    let prepared_nil_spans = syntax.nodes.iter()
        .filter(|node| matches!(node.kind, NodeKind::Nil))
        .map(|node| node.span.clone())
        .collect::<std::collections::HashSet<_>>();
    for node in &mut nodes {
        if prepared_nil_spans.contains(&node.span) {
            node.kind = NodeKind::Nil;
        }
    }
    (Cow::Owned(SyntaxTree { nodes, roots }), discards)
}

fn empty_program() -> Program {
    Program {
        namespaces: Vec::new(),
        reader_contexts: HashMap::new(),
        namespace_entries: HashMap::new(),
        compilation_uses: Vec::new(),
        exprs: Vec::new(),
        locals: Vec::new(),
        recur_targets: Vec::new(),
        protocols: HashMap::new(),
        record_constructors: std::collections::HashSet::new(),
        field_bindings: HashMap::new(),
        type_methods: std::collections::HashSet::new(),
        macro_functions: std::collections::HashSet::new(),
        globals: Vec::new(),
        js_globals: Vec::new(),
        strings: Vec::new(),
        named_literals: Vec::new(),
        root: ExprId(0),
    }
}

/// Analyze a parser-produced tree against its exact original source buffer.
/// Standalone input has namespace `cljs.user`; filesystem loading is explicit.
pub fn analyze(source: &str, syntax: &SyntaxTree) -> Result<Program, AnalysisError> {
    analyze_with_macros(source, syntax, &mut MacroHost::new())
}

/// Analyze with an explicitly supplied macro host and import configuration.
pub fn analyze_with_macros(
    source: &str,
    syntax: &SyntaxTree,
    macro_host: &mut MacroHost,
) -> Result<Program, AnalysisError> {
    analyze_with_macro_warnings(
        source,
        syntax,
        macro_host,
        AnalysisOptions::default(),
        &mut Vec::new(),
    )
}

pub fn analyze_with_warnings(
    source: &str,
    syntax: &SyntaxTree,
    options: AnalysisOptions,
    warnings: &mut Vec<AnalysisWarning>,
) -> Result<Program, AnalysisError> {
    analyze_with_macro_warnings(source, syntax, &mut MacroHost::new(), options, warnings)
}

pub fn analyze_with_macro_warnings(
    source: &str,
    syntax: &SyntaxTree,
    macro_host: &mut MacroHost,
    options: AnalysisOptions,
    warnings: &mut Vec<AnalysisWarning>,
) -> Result<Program, AnalysisError> {
    analyze_input(source, syntax, macro_host, options, warnings, None, None, None, None)
}

/// Start a standalone compilation from an immutable compiler form. Generated
/// spellings and literal payloads enter the analyzer directly, without a reader.
pub fn analyze_compiler_form(form: &Form) -> Result<Program, AnalysisError> {
    let source = form.span().source().clone();
    let syntax = SyntaxTree {
        nodes: vec![],
        roots: vec![],
    };
    analyze_input(
        source.text(),
        &syntax,
        &mut MacroHost::new(),
        AnalysisOptions::default(),
        &mut Vec::new(),
        Some(form),
        None,
        None,
        None,
    )
}

pub(crate) fn analyze_deferred_form(
    form: &Form,
    host: &mut MacroHost,
    snapshot: &crate::compilation_session::DefinitionSnapshot,
    engine: Option<&dyn crate::evaluator::MacroEvaluator>,
) -> Result<(Program, Vec<AnalysisWarning>), AnalysisError> {
    let mut warnings = vec![];
    let evaluator = host.bootstrap.as_ref().map(|b| b.evaluator.clone());
    let engine = engine.or_else(|| {
        evaluator
            .as_ref()
            .map(|e| e.as_ref() as &dyn crate::evaluator::MacroEvaluator)
    });
    let program = analyze_input(
        form.span().source().text(),
        &SyntaxTree {
            nodes: vec![],
            roots: vec![],
        },
        host,
        snapshot.options,
        &mut warnings,
        Some(form),
        Some((
            &snapshot.namespace,
            &snapshot.aliases,
            &snapshot.requires,
            snapshot.resolution.clone(),
        )),
        Some(snapshot),
        engine,
    )?;
    Ok((program, warnings))
}

fn analyze_input(
    source: &str,
    syntax: &SyntaxTree,
    macro_host: &mut MacroHost,
    options: AnalysisOptions,
    warnings: &mut Vec<AnalysisWarning>,
    owned_form: Option<&Form>,
    definition_view: Option<(&str, &[(String, String)], &[String], NamespaceResolution)>,
    snapshot: Option<&crate::compilation_session::DefinitionSnapshot>,
    engine: Option<&dyn crate::evaluator::MacroEvaluator>,
) -> Result<Program, AnalysisError> {
    analyze_input_state(source, syntax, macro_host, options, warnings, owned_form, definition_view, snapshot, engine, None)
}

fn analyze_input_state(
    source: &str,
    syntax: &SyntaxTree,
    macro_host: &mut MacroHost,
    options: AnalysisOptions,
    warnings: &mut Vec<AnalysisWarning>,
    owned_form: Option<&Form>,
    definition_view: Option<(&str, &[(String, String)], &[String], NamespaceResolution)>,
    snapshot: Option<&crate::compilation_session::DefinitionSnapshot>,
    engine: Option<&dyn crate::evaluator::MacroEvaluator>,
    previous: Option<&Program>,
) -> Result<Program, AnalysisError> {
    if definition_view.is_none() && previous.is_none() {
        macro_host.begin_compilation();
    }
    let (syntax, discards) = if owned_form.is_some() {
        // A deferred semantic form has already selected branches and validated
        // source discards. Re-reading the whole source would restore inactive
        // CLJC forms and replay unrelated reader work inside this unit.
        (Cow::Borrowed(syntax), Vec::new())
    } else {
        compiler_syntax(source, syntax)
    };
    let mut roots = syntax.roots.clone();
    let (namespace, aliases, requires, resolution) =
        definition_view.unwrap_or(("cljs.user", &[], &[], NamespaceResolution::default()));
    let mut analyzer = Analyzer {
        source,
        syntax,
        discards,
        next_discard: 0,
        generated: Vec::new(),
        spellings: Vec::new(),
        numbers: Vec::new(),
        local_kinds: HashMap::new(),
        type_fields: None,
        raw_type: false,
        macro_host,
        engine,
        deferred: Vec::new(),
        macro_depth: 0,
        analysis_depth: 0,
        macro_source: None,
        program: empty_program(),
        env: Environment::default(),
        recur_target: None,
        globals: HashMap::new(),
        namespace,
        aliases,
        requires,
        source_index: 0,
        resolution,
        options,
        unchecked_arrays: snapshot.is_some_and(|s| s.unchecked_arrays),
        checked_array_exprs: Default::default(),
        warnings,
        resolving_var: false,
        definition_name: None,
        top_level: snapshot.is_none(),
    };
    if let Some(previous) = previous {
        analyzer.program = previous.clone();
        analyzer.globals = previous.globals.iter().enumerate().map(|(i,g)| ((g.namespace.clone(),g.name.clone()),GlobalId(i))).collect();
    }
    if let Some(snapshot) = snapshot {
        analyzer.program = snapshot.resolution_program.clone();
        analyzer.globals = analyzer.program.globals.iter().enumerate().map(|(i,g)| ((g.namespace.clone(),g.name.clone()),GlobalId(i))).collect();
        analyzer.source_index = snapshot.source_index;
        analyzer.definition_name = snapshot.definition_name.clone();
    }
    if !analyzer.program.namespaces.iter().any(|ns| ns == namespace) { analyzer.program.namespaces.push(namespace.into()); }
    analyzer.program.reader_contexts.insert(namespace.into(), RuntimeReaderNamespace {aliases: aliases.to_vec(), refers: analyzer.resolution.refers.clone(), excludes: analyzer.resolution.excludes.clone(), macros: Vec::new()});
    if let Some(form) = owned_form {
        roots = vec![analyzer.import_form(form)?];
    }
    analyzer.program.root = analyzer.body(&roots, 0..source.len(), ExprContext::Return)?;
    analyzer.program.reader_contexts.get_mut(namespace).unwrap().macros = analyzer.macro_host.source_macro_names(namespace);
    analyzer.read_discards_before(source.len())?;
    analyzer.drain_deferred()?;
    analyzer.collect_expression_warnings();
    if snapshot.is_none() && let Some(session) = analyzer.macro_host.session_handle() {
        analyzer.program.compilation_uses = session.compilation_uses();
    }
    Ok(analyzer.program)
}

/// Analyze validated namespaces in dependency-first order into one shared unit.
pub fn analyze_namespaces(
    inputs: &[NamespaceInput<'_>],
) -> Result<Program, (usize, AnalysisError)> {
    analyze_namespaces_with_macros(inputs, &mut MacroHost::new())
}

pub fn analyze_namespaces_with_macros(
    inputs: &[NamespaceInput<'_>],
    macro_host: &mut MacroHost,
) -> Result<Program, (usize, AnalysisError)> {
    analyze_namespaces_prepared(
        inputs,
        macro_host,
        |_, _| Ok(NamespaceResolution::default()),
    )
}

pub(crate) fn analyze_namespaces_prepared(
    inputs: &[NamespaceInput<'_>],
    macro_host: &mut MacroHost,
    prepare: impl FnMut(&mut MacroHost, usize) -> Result<NamespaceResolution, AnalysisError>,
) -> Result<Program, (usize, AnalysisError)> {
    analyze_namespaces_prepared_with_warnings(
        inputs,
        macro_host,
        AnalysisOptions::default(),
        &mut Vec::new(),
        prepare,
    )
}

pub(crate) fn analyze_namespaces_prepared_with_warnings(
    inputs: &[NamespaceInput<'_>],
    macro_host: &mut MacroHost,
    options: AnalysisOptions,
    warnings: &mut Vec<AnalysisWarning>,
    mut prepare: impl FnMut(&mut MacroHost, usize) -> Result<NamespaceResolution, AnalysisError>,
) -> Result<Program, (usize, AnalysisError)> {
    macro_host.begin_compilation();
    let mut program = empty_program();
    let mut globals = HashMap::new();
    let mut roots = Vec::new();
    for (source_index, input) in inputs.iter().enumerate() {
        let resolution =
            prepare(macro_host, source_index).map_err(|error| (source_index, error))?;
        let (mut syntax, mut discards) = compiler_syntax(input.source, input.syntax);
        if resolution.cljc {
            crate::compiler_reader::select_cljs(input.source, syntax.to_mut(), &mut discards)
                .map_err(|error| (source_index, error))?;
        }
        let source_roots = syntax.roots.clone();
        let mut analyzer = Analyzer {
            source: input.source,
            syntax,
            discards,
            next_discard: 0,
            generated: Vec::new(),
            spellings: Vec::new(),
            numbers: Vec::new(),
            local_kinds: HashMap::new(),
            type_fields: None,
            raw_type: false,
            macro_host,
            engine: None,
            deferred: Vec::new(),
            macro_depth: 0,
            analysis_depth: 0,
            macro_source: None,
            program,
            env: Environment::default(),
            recur_target: None,
            globals,
            namespace: input.name,
            aliases: input.aliases,
            requires: input.requires,
            source_index,
            resolution,
            options,
            unchecked_arrays: false,
            checked_array_exprs: Default::default(),
            warnings,
            resolving_var: false,
            definition_name: None,
            top_level: true,
        };
        for (local, namespace, original) in &analyzer.resolution.refers.clone() {
            let runtime = if matches!(namespace.as_str(), "cljs.core" | "clojure.core") {
                CoreFnId::resolve(original).is_some()
                    || crate::compiler_names::is_definition_macro(namespace, original)
            } else {
                analyzer
                    .globals
                    .get(&(namespace.clone(), original.clone()))
                    .is_some_and(|g| analyzer.program.globals[g.0].declared)
            };
            let mac = analyzer
                .macro_host
                .resolves_macro(input.name, local, true)
                .map_err(|message| {
                    (
                        source_index,
                        AnalysisError {
                            kind: AnalysisErrorKind::MacroExpansion(message),
                            span: input.syntax.nodes[input.syntax.roots[0].0].span.clone(),
                        },
                    )
                })?;
            if !runtime && !mac {
                return Err((
                    source_index,
                    AnalysisError {
                        kind: AnalysisErrorKind::UnresolvedSymbol(format!(
                            "{namespace}/{original} (:refer does not name a public Var or macro)"
                        )),
                        span: input.syntax.nodes[input.syntax.roots[0].0].span.clone(),
                    },
                ));
            }
        }
        let context = if source_index + 1 == inputs.len() {
            ExprContext::Return
        } else {
            ExprContext::Discard
        };
        let root = analyzer
            .body(&source_roots[1..], 0..input.source.len(), context)
            .map_err(|error| (source_index, error))?;
        analyzer
            .read_discards_before(input.source.len())
            .map_err(|e| (source_index, e))?;
        analyzer.program.reader_contexts.insert(input.name.into(), RuntimeReaderNamespace {aliases: input.aliases.to_vec(), refers: analyzer.resolution.refers.clone(), excludes: analyzer.resolution.excludes.clone(), macros: analyzer.macro_host.source_macro_names(input.name)});
        analyzer.program.namespace_entries.insert(root, input.name.into());
        roots.push(root);
        analyzer.drain_deferred().map_err(|e| (source_index, e))?;
        analyzer.collect_expression_warnings();
        if !analyzer.program.namespaces.iter().any(|ns| ns == input.name) { analyzer.program.namespaces.push(input.name.into()); }
        program = analyzer.program;
        globals = analyzer.globals;
    }
    program.compilation_uses = macro_host.session_handle().map_or_else(Vec::new, |s| s.compilation_uses());
    program.root = ExprId(program.exprs.len());
    program.exprs.push(Expr {
        kind: ExprKind::Do(roots),
        span: 0..0,
        context: ExprContext::Return,
        source_index: 0,
    });
    Ok(program)
}

const DIAGNOSTIC_NUMBER: u8 = 1;
const DIAGNOSTIC_STRING: u8 = 2;
const DIAGNOSTIC_BOOLEAN: u8 = 4;
const DIAGNOSTIC_OBJECT: u8 = 8;
const DIAGNOSTIC_ARRAY: u8 = 16;
const DIAGNOSTIC_NIL: u8 = 32;
const DIAGNOSTIC_ANY: u8 = 128;

fn diagnostic_type_name(kind: u8) -> String {
    if kind & DIAGNOSTIC_ANY != 0 || kind == 0 {
        return "any".into();
    }
    let names = [
        (DIAGNOSTIC_ARRAY, "array"),
        (DIAGNOSTIC_BOOLEAN, "boolean"),
        (DIAGNOSTIC_NIL, "clj-nil"),
        (DIAGNOSTIC_NUMBER, "number"),
        (DIAGNOSTIC_OBJECT, "object"),
        (DIAGNOSTIC_STRING, "string"),
    ]
    .into_iter()
    .filter_map(|(bit, name)| (kind & bit != 0).then_some(name))
    .collect::<Vec<_>>();
    if names.len() == 1 {
        names[0].into()
    } else {
        format!("#{{{}}}", names.join(" "))
    }
}

fn diagnostic_type(
    program: &Program,
    id: ExprId,
    locals: &HashMap<LocalId, u8>,
    depth: usize,
) -> u8 {
    if depth > 128 {
        return DIAGNOSTIC_ANY;
    }
    let infer = |id| diagnostic_type(program, id, locals, depth + 1);
    match &program.exprs[id.0].kind {
        ExprKind::Number(_) => DIAGNOSTIC_NUMBER,
        ExprKind::String(_) => DIAGNOSTIC_STRING,
        ExprKind::Bool(_) | ExprKind::BooleanHint(_) | ExprKind::InstanceOf { .. } => {
            DIAGNOSTIC_BOOLEAN
        }
        ExprKind::Nil => DIAGNOSTIC_NIL,
        ExprKind::Local(local) => *locals.get(local).unwrap_or(&DIAGNOSTIC_ANY),
        ExprKind::WithMeta { value, .. } => infer(*value),
        ExprKind::Let { body, .. }
        | ExprKind::LetFn { body, .. }
        | ExprKind::Loop { body, .. }
        | ExprKind::ThisAs { body, .. } => infer(*body),
        ExprKind::Do(items) => items.last().map(|&id| infer(id)).unwrap_or(DIAGNOSTIC_NIL),
        ExprKind::If { yes, no, .. } => infer(*yes) | infer(*no),
        ExprKind::Throw(_) | ExprKind::Recur { .. } => 0,
        ExprKind::Try { body, catches, .. } => catches
            .iter()
            .fold(infer(*body), |types, clause| types | infer(clause.body)),
        ExprKind::Call { callee, .. } => match program.exprs[callee.0].kind {
            ExprKind::CoreFn(
                CoreFnId::Add
                | CoreFnId::Subtract
                | CoreFnId::Multiply
                | CoreFnId::Divide
                | CoreFnId::Inc
                | CoreFnId::Dec
                | CoreFnId::Count,
            ) => DIAGNOSTIC_NUMBER,
            ExprKind::CoreFn(CoreFnId::Array | CoreFnId::IntoArray) => DIAGNOSTIC_ARRAY,
            ExprKind::CoreFn(CoreFnId::JsObj) => DIAGNOSTIC_OBJECT,
            _ => DIAGNOSTIC_ANY,
        },
        ExprKind::JsTemplate { segments, .. }
            if segments == &["({})"]
                || (segments.first().is_some_and(|value| value == "({")
                    && segments.last().is_some_and(|value| value == "})")) =>
        {
            DIAGNOSTIC_OBJECT
        }
        _ => DIAGNOSTIC_ANY,
    }
}

impl Analyzer<'_> {
    fn metadata_value(&self, metadata: ExprId, key: &str) -> Option<ExprId> {
        let ExprKind::Map(entries) = &self.program.exprs[metadata.0].kind else {
            return None;
        };
        entries.as_chunks::<2>().0.iter().find_map(|pair| {
            let ExprKind::Keyword(name) = self.program.exprs[pair[0].0].kind else {
                return None;
            };
            let literal = &self.program.named_literals[name.0];
            (literal.namespace.is_none()
                && String::from_utf16_lossy(&self.program.strings[literal.name.0]) == key)
                .then_some(pair[1])
        })
    }

    fn datum_text(&self, value: ExprId) -> String {
        match &self.program.exprs[value.0].kind {
            ExprKind::Symbol(name) | ExprKind::Keyword(name) => {
                let literal = &self.program.named_literals[name.0];
                let namespace = literal
                    .namespace
                    .map(|id| format!("{}/", String::from_utf16_lossy(&self.program.strings[id.0])))
                    .unwrap_or_default();
                let prefix = if matches!(self.program.exprs[value.0].kind, ExprKind::Keyword(_)) {
                    ":"
                } else {
                    ""
                };
                format!(
                    "{prefix}{namespace}{}",
                    String::from_utf16_lossy(&self.program.strings[literal.name.0])
                )
            }
            ExprKind::Vector(items) | ExprKind::List(items) => {
                let (start, end) =
                    if matches!(self.program.exprs[value.0].kind, ExprKind::Vector(_)) {
                        ('[', ']')
                    } else {
                        ('(', ')')
                    };
                format!(
                    "{start}{}{end}",
                    items
                        .iter()
                        .map(|&id| self.datum_text(id))
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            }
            ExprKind::WithMeta { value, .. } => self.datum_text(*value),
            ExprKind::Bool(value) => value.to_string(),
            ExprKind::Nil => "nil".into(),
            _ => "?".into(),
        }
    }

    fn check_private_var(&mut self, node: NodeId, global: GlobalId) {
        let binding = &self.program.globals[global.0];
        if !self.resolving_var
            && binding.namespace != self.namespace
            && binding
                .metadata
                .and_then(|m| self.metadata_value(m, "private"))
                .is_some_and(|id| matches!(self.program.exprs[id.0].kind, ExprKind::Bool(true)))
        {
            self.warn(
                node,
                "private-var-access",
                format!("var: {}/{} is not public", binding.namespace, binding.name),
            );
        }
    }

    fn collect_expression_warnings(&mut self) {
        let mut locals = HashMap::new();
        // Each local's type is a finite union. Iterate recurrence edges until
        // no union changes; unknown operands remain unknown, never guessed.
        for _ in 0..=self.program.locals.len() * 8 {
            let before = locals.clone();
            for expr in &self.program.exprs {
                match &expr.kind {
                    ExprKind::Let { bindings, .. } | ExprKind::Loop { bindings, .. } => {
                        for &(local, init) in bindings {
                            let kind = diagnostic_type(&self.program, init, &locals, 0);
                            *locals.entry(local).or_insert(0) |= kind;
                        }
                    }
                    ExprKind::Recur { target, args } => {
                        for (&local, &arg) in
                            self.program.recur_targets[target.0].params.iter().zip(args)
                        {
                            let kind = diagnostic_type(&self.program, arg, &locals, 0);
                            *locals.entry(local).or_insert(0) |= kind;
                        }
                    }
                    _ => {}
                }
            }
            if locals == before {
                break;
            }
        }
        for (expr_index, expr) in self.program.exprs.iter().enumerate() {
            if expr.source_index != self.source_index {
                continue;
            }
            let mut warning = None;
            let mut arithmetic = Vec::new();
            if let ExprKind::Call { callee, args } = &expr.kind {
                match self.program.exprs[callee.0].kind {
                    ExprKind::CoreFn(core)
                        if matches!(
                            core,
                            CoreFnId::Add
                                | CoreFnId::Subtract
                                | CoreFnId::Multiply
                                | CoreFnId::Divide
                        ) =>
                    {
                        let mut types = args
                            .iter()
                            .map(|&arg| diagnostic_type(&self.program, arg, &locals, 0))
                            .collect::<Vec<_>>();
                        if core == CoreFnId::Divide && args.len() == 1 {
                            types.insert(0, DIAGNOSTIC_NUMBER);
                        }
                        let operands = if types.len() <= 2 {
                            vec![types]
                        } else {
                            std::iter::once(types[..2].to_vec())
                                .chain(types[2..].iter().map(|&kind| vec![DIAGNOSTIC_NUMBER, kind]))
                                .collect()
                        };
                        for types in operands {
                            if types.iter().any(|kind| {
                                kind & (DIAGNOSTIC_ANY | DIAGNOSTIC_NUMBER | DIAGNOSTIC_NIL) == 0
                                    && *kind != 0
                            }) {
                                let operator = match core {
                                    CoreFnId::Add => "+",
                                    CoreFnId::Subtract => "-",
                                    CoreFnId::Multiply => "*",
                                    _ => "/",
                                };
                                arithmetic.push(("invalid-arithmetic", format!("cljs.core/{operator}, all arguments must be numbers, got [{}] instead", types.into_iter().map(diagnostic_type_name).collect::<Vec<_>>().join(" "))));
                            }
                        }
                    }
                    ExprKind::Global(global) => {
                        let mut arities = Vec::new();
                        for definition in &self.program.exprs {
                            if let ExprKind::DefType {
                                constructor: Some(constructor),
                                fields,
                                ..
                            } = &definition.kind
                                && *constructor == global
                            {
                                arities.push((fields.len(), false));
                            }
                            if let ExprKind::Def {
                                global: target,
                                init: Some(init),
                                ..
                            } = definition.kind
                                && target == global
                            {
                                let mut init = init;
                                while let ExprKind::WithMeta { value, .. } =
                                    self.program.exprs[init.0].kind
                                {
                                    init = value;
                                }
                                if let ExprKind::Fn { methods, .. } =
                                    &self.program.exprs[init.0].kind
                                {
                                    arities.extend(
                                        methods.iter().map(|m| (m.params.len(), m.rest.is_some())),
                                    );
                                }
                            }
                        }
                        if !arities.is_empty()
                            && !arities.iter().any(|&(fixed, variadic)| {
                                args.len() == fixed || (variadic && args.len() >= fixed)
                            })
                        {
                            let binding = &self.program.globals[global.0];
                            warning = Some((
                                "fn-arity",
                                format!(
                                    "Wrong number of args ({}) passed to {}/{}",
                                    args.len(),
                                    binding.namespace,
                                    binding.name
                                ),
                            ));
                        }
                    }
                    _ => {}
                }
            }
            if self.checked_array_exprs.contains(&ExprId(expr_index)) {
                let indexed = match &expr.kind {
                    ExprKind::JsIndex { receiver, indices } => {
                        Some((*receiver, indices.as_slice(), None))
                    }
                    ExprKind::SetGlobal { target, value } => {
                        match &self.program.exprs[target.0].kind {
                            ExprKind::JsIndex { receiver, indices } => {
                                Some((*receiver, indices.as_slice(), Some(*value)))
                            }
                            _ => None,
                        }
                    }
                    ExprKind::Call { callee, args } => match self.program.exprs[callee.0].kind {
                        ExprKind::CoreFn(CoreFnId::CheckedAGet | CoreFnId::CheckedAGetStrict) => {
                            Some((args[0], &args[1..], None))
                        }
                        ExprKind::CoreFn(CoreFnId::CheckedASet | CoreFnId::CheckedASetStrict) => {
                            Some((args[0], &args[1..args.len() - 1], args.last().copied()))
                        }
                        _ => None,
                    },
                    _ => None,
                };
                if let Some((receiver, indices, value)) = indexed {
                    // The assignment target is a child, not a separate aget.
                    let assignment_target = value.is_none() && self.program.exprs.iter().any(|parent| matches!(parent.kind, ExprKind::SetGlobal { target, .. } if std::ptr::eq(expr, &self.program.exprs[target.0])));
                    if !assignment_target {
                        let mut types = vec![diagnostic_type(&self.program, receiver, &locals, 0)];
                        types.extend(
                            indices
                                .iter()
                                .map(|&id| diagnostic_type(&self.program, id, &locals, 0)),
                        );
                        if (types[0] & (DIAGNOSTIC_ARRAY | DIAGNOSTIC_ANY) == 0)
                            || types[1..]
                                .iter()
                                .any(|t| t & (DIAGNOSTIC_NUMBER | DIAGNOSTIC_ANY) == 0)
                        {
                            let operator = if value.is_some() { "aset" } else { "aget" };
                            if let Some(value) = value {
                                types.push(diagnostic_type(&self.program, value, &locals, 0));
                            }
                            let required = if value.is_some() {
                                "an array, followed by numeric indices, followed by a value"
                            } else {
                                "an array followed by numeric indices"
                            };
                            let alternative = if value.is_some() {
                                "set"
                            } else if indices.len() > 1 {
                                "getValueByKeys"
                            } else {
                                "get"
                            };
                            warning = Some((
                                "invalid-array-access",
                                format!(
                                    "cljs.core/{operator}, arguments must be {required}, got [{}] instead (consider goog.object/{alternative} for object access)",
                                    types
                                        .into_iter()
                                        .map(diagnostic_type_name)
                                        .collect::<Vec<_>>()
                                        .join(" ")
                                ),
                            ));
                        }
                    }
                }
            }
            for (kind, message) in arithmetic.into_iter().chain(warning) {
                // Some intrinsic lowering shares an indexed child node. Emit
                // each source diagnostic once, retaining separate occurrences.
                if kind != "invalid-array-access"
                    || !self.warnings.iter().any(|w| {
                        w.source_index == expr.source_index
                            && w.span == expr.span
                            && w.kind == kind
                            && w.message == message
                    })
                {
                    self.warnings.push(AnalysisWarning {
                        kind,
                        message,
                        span: expr.span.clone(),
                        source_index: expr.source_index,
                    });
                }
            }
        }
        self.warnings
            .sort_by_key(|warning| (warning.source_index, warning.span.start));
    }

    fn warn(&mut self, node: NodeId, kind: &'static str, message: String) {
        self.warnings.push(AnalysisWarning {
            kind,
            message,
            span: self.syntax.nodes[node.0].span.clone(),
            source_index: self.source_index,
        });
    }
    fn push(&mut self, kind: ExprKind, span: Span, context: ExprContext) -> ExprId {
        let id = ExprId(self.program.exprs.len());
        if !self.unchecked_arrays
            && (self.options.checked_arrays || self.options.array_checks != CheckedArrays::Off)
            && matches!(kind, ExprKind::JsIndex { .. } | ExprKind::SetGlobal { .. })
        {
            self.checked_array_exprs.insert(id);
        }
        self.program.exprs.push(Expr {
            kind,
            span,
            context,
            source_index: self.source_index,
        });
        id
    }

    fn error(&self, node: NodeId, kind: AnalysisErrorKind) -> AnalysisError {
        AnalysisError {
            kind,
            span: self.syntax.nodes[node.0].span.clone(),
        }
    }

    fn spelling(&self, node: NodeId) -> &str {
        self.spellings
            .get(node.0)
            .and_then(Option::as_deref)
            .unwrap_or_else(|| &self.source[self.syntax.nodes[node.0].span.clone()])
    }

    fn lookup(&self, name: &str) -> Option<LocalId> {
        self.env
            .scopes
            .iter()
            .rev()
            .flat_map(|scope| scope.iter().rev())
            .copied()
            .find(|id| self.program.locals[id.0].name == name)
    }

    fn binding_name(&self, node: NodeId) -> Result<&str, AnalysisError> {
        self.validate_symbol(node)?;
        let name = self.spelling(node);
        if !matches!(self.syntax.nodes[node.0].kind, NodeKind::Symbol)
            || (name != "/" && name.contains('/'))
            || name.contains('.')
            || name == "&"
        {
            return Err(self.error(node, AnalysisErrorKind::InvalidBinding));
        }
        Ok(name)
    }

    fn bind(&mut self, node: NodeId) -> Result<LocalId, AnalysisError> {
        let name = self.binding_name(node)?.to_owned();
        let id = LocalId(self.program.locals.len());
        self.program.locals.push(LocalBinding {
            name,
            span: self.syntax.nodes[node.0].span.clone(),
            source_index: self.source_index,
        });
        self.env.scopes.last_mut().expect("binding scope").push(id);
        Ok(id)
    }

    fn form(&mut self, id: NodeId, context: ExprContext) -> Result<ExprId, AnalysisError> {
        if let Some(engine) = self.engine {
            engine.control().poll().map_err(|e| self.error(id, AnalysisErrorKind::MacroExpansion(e.to_string())))?;
        }
        if self.macro_depth > 0 && self.analysis_depth >= 128 {
            return Err(self.error(
                id,
                AnalysisErrorKind::MacroExpansion(
                    "generated runtime form nesting limit exceeded".into(),
                ),
            ));
        }
        self.analysis_depth += 1;
        let previous_top_level = self.top_level;
        let allows_declarations = match &self.syntax.nodes[id.0].kind {
            NodeKind::Collection {
                kind: CollectionKind::List,
                children,
            } => children.first().is_some_and(|id| {
                let name = self.spelling(*id);
                matches!(
                    name,
                    "do" | "def" | "defonce" | "declare" | "cljs.core/def" | "cljs.core/defonce" | "cljs.core/declare" | "defmacro" | "cljs.core/defmacro" | "clojure.core/defmacro"
                ) || matches!(name.rsplit('/').next(), Some("defprotocol" | "deftype" | "defrecord" | "extend-type" | "extend-protocol")) || (self.lookup(name).is_none()
                    && (self.macro_host.has_javascript_macro(self.namespace, name)
                        || self
                            .macro_host
                            .macro_name(
                                self.namespace,
                                name,
                                self.globals
                                    .get(&(self.namespace.into(), name.into()))
                                    .is_some_and(|g| self.program.globals[g.0].declared)
                                    || self
                                        .resolution
                                        .excludes
                                        .iter()
                                        .any(|excluded| excluded == name),
                            )
                            .is_some()))
            }),
            _ => false,
        };
        self.top_level &= allows_declarations;
        let result = self.form_inner(id, context);
        self.top_level = previous_top_level;
        self.analysis_depth -= 1;
        result
    }

    // Value-use context and legal recurrence are independent: a discarded loop
    // can recur, but a returning IIFE inside an argument cannot recur outward.
    fn non_tail_form(&mut self, id: NodeId, context: ExprContext) -> Result<ExprId, AnalysisError> {
        let previous = self.recur_target.take();
        let result = self.form(id, context);
        self.recur_target = previous;
        result
    }

    fn new_recur_target(&mut self, params: Vec<LocalId>, protocol_impl: bool) -> RecurTargetId {
        let id = RecurTargetId(self.program.recur_targets.len());
        self.program.recur_targets.push(RecurTarget {
            params,
            protocol_impl,
            receiver_from_this: protocol_impl,
            used: false,
            argument_slots: Vec::new(),
        });
        id
    }

    fn recur_form(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        let target = self
            .recur_target
            .ok_or_else(|| self.error(id, AnalysisErrorKind::InvalidForm("Can't recur here")))?;
        let frame = &self.program.recur_targets[target.0];
        let implicit_receiver = frame.protocol_impl && args.len() + 1 == frame.params.len();
        if args.len() + usize::from(implicit_receiver) != frame.params.len() {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm("recur argument count mismatch"),
            ));
        }
        let mut values = Vec::with_capacity(frame.params.len());
        if implicit_receiver {
            values.push(self.push(
                ExprKind::Nil,
                self.syntax.nodes[id.0].span.clone(),
                ExprContext::Value,
            ));
        }
        for &arg in args {
            values.push(self.non_tail_form(arg, ExprContext::Value)?);
        }
        self.program.recur_targets[target.0].used = true;
        Ok(self.push(
            ExprKind::Recur {
                target,
                args: values,
            },
            self.syntax.nodes[id.0].span.clone(),
            context,
        ))
    }

    fn form_inner(&mut self, id: NodeId, context: ExprContext) -> Result<ExprId, AnalysisError> {
        let node = self.syntax.nodes[id.0].clone();
        if self.generated.get(id.0).and_then(Option::as_ref).is_none() {
            self.read_discards_before(node.span.start)?;
        }
        if self.needs_reader(id) {
            let expanded = self.expand_reader(id)?;
            return self.form(expanded, context);
        }
        if let Some(form) = self.generated.get(id.0).and_then(Option::as_ref).cloned()
            && let DatumView::CompilerLiteral(kind) = form.data().inspect()
        {
            return self.reader_literal(&form, kind, context, false);
        }
        // Keep literal/collection lowering temporaries off every recursive list
        // frame, including in unoptimized builds on the default thread stack.
        if let NodeKind::Collection {
            kind: CollectionKind::List,
            children,
        } = &node.kind
        {
            let value = self.list(id, children, context)?;
            return if matches!(
                self.program.exprs[value.0].kind,
                ExprKind::Fn { .. } | ExprKind::Reify { .. }
            ) {
                self.attach_metadata(id, value, context, false)
            } else {
                Ok(value)
            };
        }
        self.value_form(id, &node, context)
    }

    fn value_form(
        &mut self,
        id: NodeId,
        node: &crate::parser::Node,
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        let kind = match &node.kind {
            NodeKind::Nil => ExprKind::Nil,
            NodeKind::Boolean(value) => ExprKind::Bool(*value),
            NodeKind::Number(kind) => {
                let value = self
                    .numbers
                    .get(id.0)
                    .copied()
                    .flatten()
                    .unwrap_or_else(|| number(self.spelling(id), *kind))
                    .ok_or_else(|| self.error(id, AnalysisErrorKind::NumberOutOfRange))?;
                ExprKind::Number(value)
            }
            NodeKind::Character(value) => {
                ExprKind::String(self.own_string(value.encode_utf16(&mut [0; 2]).to_vec()))
            }
            NodeKind::SymbolicValue(value) => ExprKind::Number(match value {
                crate::SymbolicValue::Infinity => f64::INFINITY,
                crate::SymbolicValue::NegativeInfinity => f64::NEG_INFINITY,
                crate::SymbolicValue::NaN => f64::NAN,
            }),
            NodeKind::String(value) => {
                let id = StringId(self.program.strings.len());
                self.program.strings.push(value.to_utf16(self.source));
                ExprKind::String(id)
            }
            NodeKind::Keyword { auto_resolve } => self.keyword(id, *auto_resolve)?,
            NodeKind::Collection {
                kind: CollectionKind::Vector,
                children,
            } => ExprKind::Vector(
                children
                    .iter()
                    .map(|&child| self.non_tail_form(child, ExprContext::Value))
                    .collect::<Result<_, _>>()?,
            ),
            NodeKind::Collection {
                kind: CollectionKind::Map,
                children,
            } => {
                self.check_map_keys(children)?;
                ExprKind::Map(
                    children
                        .iter()
                        .map(|&child| self.non_tail_form(child, ExprContext::Value))
                        .collect::<Result<_, _>>()?,
                )
            }
            NodeKind::Collection {
                kind: CollectionKind::Set,
                children,
            } => {
                self.check_set_members(children, false)?;
                ExprKind::Set(
                    children
                        .iter()
                        .map(|&child| self.non_tail_form(child, ExprContext::Value))
                        .collect::<Result<_, _>>()?,
                )
            }
            NodeKind::Symbol => {
                self.validate_symbol(id)?;
                let name = self.spelling(id).to_owned();
                let value = self.resolve(id, &name)?;
                let hinted = matches!(value, ExprKind::Global(g) if self.program.globals[g.0].boolean_tag)
                    || self.metadata_is(id, "tag", "boolean");
                if hinted {
                    let value = self.push(value, node.span.clone(), context);
                    ExprKind::BooleanHint(value)
                } else {
                    value
                }
            }
            NodeKind::Prefix {
                kind: PrefixKind::Quote,
                form,
            } => {
                return self.datum(*form, context);
            }
            other => {
                let description = match other {
                    NodeKind::Number(NumberKind::BigInteger) => "big integers",
                    NodeKind::Number(NumberKind::Decimal) => "decimal literals",
                    NodeKind::Number(NumberKind::Ratio) => "ratios",
                    NodeKind::Character(_) => "characters",
                    NodeKind::SymbolicValue(_) => "symbolic numbers",
                    NodeKind::Regex => "regular expressions",
                    NodeKind::Collection { .. } => {
                        "collection literals or anonymous reader functions"
                    }
                    NodeKind::Prefix { .. } => "reader prefixes (including quote)",
                    NodeKind::Metadata { .. } => "metadata",
                    NodeKind::Tagged { .. } => "tagged literals",
                    NodeKind::Conditional { .. } => "reader conditionals",
                    NodeKind::NamespacedMap { .. } => "namespaced maps",
                    NodeKind::Argument { .. } => "anonymous reader function arguments",
                    _ => unreachable!(),
                };
                return Err(self.error(id, AnalysisErrorKind::Unsupported(description.into())));
            }
        };
        let value = self.push(kind, node.span.clone(), context);
        if matches!(
            node.kind,
            NodeKind::Collection {
                kind: CollectionKind::Vector | CollectionKind::Map | CollectionKind::Set,
                ..
            }
        ) {
            return self.attach_metadata(id, value, context, false);
        }
        Ok(value)
    }

    fn list(
        &mut self,
        id: NodeId,
        children: &[NodeId],
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        let Some((&operator, args)) = children.split_first() else {
            return Ok(self.push(
                ExprKind::List(vec![]),
                self.syntax.nodes[id.0].span.clone(),
                context,
            ));
        };
        if matches!(self.syntax.nodes[operator.0].kind, NodeKind::Nil) {
            return Err(self.error(operator, AnalysisErrorKind::InvalidForm("cannot call nil")));
        }
        if matches!(self.syntax.nodes[operator.0].kind, NodeKind::Keyword { .. })
            && !(1..=2).contains(&args.len())
        {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm(
                    "literal keyword calls require one or two arguments",
                ),
            ));
        }
        // Binding forms must be recognized before their children are analyzed.
        if matches!(self.syntax.nodes[operator.0].kind, NodeKind::Symbol) {
            self.validate_symbol(operator)?;
            let operator_name = self.spelling(operator).to_owned();
            let imported_intrinsic = self
                .macro_host
                .definition_macro(self.namespace, &operator_name)
                .or_else(|| {
                    self.resolution
                        .refers
                        .iter()
                        .find(|(local, ns, name)| {
                            local == &operator_name
                                && crate::compiler_names::is_definition_macro(ns, name)
                        })
                        .map(|(_, _, name)| name.clone())
                });
            let intrinsic = match operator_name.split_once('/') {
                Some((qualifier, name))
                    if matches!(qualifier, "cljs.core" | "clojure.core")
                        || self
                            .aliases
                            .iter()
                            .any(|(alias, target)| alias == qualifier && target == "cljs.core") =>
                {
                    name
                }
                _ => imported_intrinsic.as_deref().unwrap_or(&operator_name),
            };
            let shadowed = !operator_name.contains('/')
                && (self
                    .macro_host
                    .has_source_value(self.namespace, &operator_name)
                    || self
                        .globals
                        .get(&(self.namespace.into(), operator_name.clone()))
                        .is_some_and(|g| self.program.globals[g.0].declared)
                    || (imported_intrinsic.is_none()
                        && self.resolution.excludes.contains(&operator_name))
                    || self.resolution.refers.iter().any(|(name, _, _)| {
                        name == &operator_name && imported_intrinsic.is_none()
                    }));
            if !shadowed && self.lookup(&operator_name).is_none() {
                if let Some(value) =
                    self.intrinsic_form(id, operator, args, context, intrinsic, &operator_name)?
                {
                    return Ok(value);
                }
            }
            if let Some(constructor) = operator_name.strip_suffix('.')
                && !constructor.is_empty()
                && !operator_name.rsplit('/').next().unwrap().starts_with('.')
            {
                let callee = self.resolve(operator, constructor)?;
                let callee = self.push(
                    callee,
                    self.syntax.nodes[operator.0].span.clone(),
                    ExprContext::Value,
                );
                let args = args
                    .iter()
                    .map(|&arg| self.non_tail_form(arg, ExprContext::Value))
                    .collect::<Result<_, _>>()?;
                let args = self.record_constructor_args(operator, callee, args);
                return Ok(self.push(
                    ExprKind::Construct { callee, args },
                    self.syntax.nodes[id.0].span.clone(),
                    context,
                ));
            }
            match operator_name.as_str() {
                "quote" => {
                    if args.len() != 1 {
                        return Err(self.error(
                            id,
                            AnalysisErrorKind::InvalidForm("quote requires one argument"),
                        ));
                    }
                    return self.datum(args[0], context);
                }
                "if" => return self.if_form(id, args, context),
                "do" => {
                    return self.body(
                        args,
                        self.syntax.nodes[id.0].span.clone(),
                        if context == ExprContext::Value && args.len() > 1 {
                            ExprContext::Return
                        } else {
                            context
                        },
                    );
                }
                "let*" => return self.let_form(id, args, context, false),
                "letfn*" => return self.letfn_form(id, args, context),
                "loop*" => return self.let_form(id, args, context, true),
                "recur" => return self.recur_form(id, args, context),
                "try" => return self.try_form(id, args, context),
                "throw" => return self.throw_form(id, args, context),
                "compiler-assert*"
                    if self.generated.get(id.0).and_then(Option::as_ref).is_some() =>
                {
                    return self.assert_form(id, args, context);
                }
                "case*" => return self.case_form(id, args, context, false),
                "compiler-case*" if self.generated.get(id.0).and_then(Option::as_ref).is_some() => {
                    return self.case_form(id, args, context, true);
                }
                "fn*" => return self.fn_form(id, args, context),
                "def" => return self.definition(id, args, context, false),
                "set!" => return self.set_form(id, args, context),
                "new" => return self.new_form(id, args, context),
                "." => return self.dot_form(id, args, context),
                "js*" => return self.js_template(id, args, context),
                "var" => return self.var_form(id, args, context),
                name if name.starts_with('.') && name != "." && name != ".." => {
                    return self.interop(id, name, args, context);
                }
                "deftype*" | "defrecord*" => {
                    return self.raw_type_form(id, args, context, operator_name == "defrecord*");
                }
                "ns" | "ns*" if self.analysis_depth > 1 => {
                    return Err(self.error(
                        operator,
                        AnalysisErrorKind::InvalidForm(
                            "Namespace declarations must appear at the top-level.",
                        ),
                    ));
                }
                "ns" | "ns*" | "&" => {
                    return Err(self.error(
                        operator,
                        AnalysisErrorKind::Unsupported(self.spelling(operator).into()),
                    ));
                }
                _ => {}
            }
        }
        self.call_form(id, operator, args, context)
    }

    // Only intrinsic calls retain this lowering frame. Primitive forms and
    // ordinary calls return from this probe before analyzing their children.
    fn intrinsic_form(
        &mut self,
        id: NodeId,
        operator: NodeId,
        args: &[NodeId],
        context: ExprContext,
        intrinsic: &str,
        operator_name: &str,
    ) -> Result<Option<ExprId>, AnalysisError> {
        match intrinsic {
            "defmacro" => return self.macro_declaration(id, context).map(Some),
            "delay" | "lazy-seq" => {
                return self
                    .deferred_form(id, args, context, intrinsic == "delay")
                    .map(Some);
            }
            "defonce" => return self.definition(id, args, context, true).map(Some),
            "declare" => return self.declare_form(id, args, context).map(Some),
            "binding" | "with-redefs" => return self.binding_form(id, args, context).map(Some),
            "resolve" => return self.resolve_form(id, args, context).map(Some),
            "require-macros" | "use-macros" => {
                return Err(self.error(operator, AnalysisErrorKind::Unsupported(
                    "legacy macro imports are retired; import macros and functions with ordinary :require from .cljs/.cljc sources".into(),
                )));
            }
            "require" => {
                return Err(self.error(
                    operator,
                    AnalysisErrorKind::InvalidForm(
                        "Calls to `require` must appear at the top-level.",
                    ),
                ));
            }
            "exists?" => return self.exists_form(id, args, context).map(Some),
            "with-out-str" => {
                let previous = self.recur_target.take();
                let body = self.body(
                    args,
                    self.syntax.nodes[id.0].span.clone(),
                    ExprContext::Discard,
                );
                self.recur_target = previous;
                let body = body?;
                return Ok(Some(self.push(
                    ExprKind::WithOutStr(body),
                    self.syntax.nodes[id.0].span.clone(),
                    context,
                )));
            }
            "this-as" => return self.this_as(id, args, context).map(Some),
            "js-arguments" if args.is_empty() => {
                return Ok(Some(self.push(
                    ExprKind::JsArguments,
                    self.syntax.nodes[id.0].span.clone(),
                    context,
                )));
            }
            "copy-arguments" if args.len() == 1 => {
                let dest = self.non_tail_form(args[0], ExprContext::Value)?;
                return Ok(Some(self.push(
                    ExprKind::CopyArguments(dest),
                    self.syntax.nodes[id.0].span.clone(),
                    context,
                )));
            }
            "reify" => return self.reify_form(id, args, context).map(Some),
            "satisfies?" | "implements?" => {
                return self
                    .protocol_predicate_form(id, args, context, intrinsic == "implements?")
                    .map(Some);
            }
            "defmulti" | "defmethod" => {
                if let Some(expanded) = self.expand_macro(id, operator, context)? {
                    let top_level = self.top_level;
                    self.top_level = false;
                    self.macro_depth += 1;
                    let result = if top_level && self.macro_host.defers_source_units() {
                        self.multimethod_declaration_stub(expanded, context, intrinsic == "defmulti")
                    } else {
                        self.form(expanded, context)
                    };
                    self.macro_depth -= 1;
                    self.top_level = top_level;
                    let expr = result?;
                    return self.catalog_bundle(id, expr).map(Some);
                }
                Ok(None)
            }
            "defprotocol" => {
                let expr = self.defprotocol_form(id, args, context)?;
                return self.catalog_bundle(id, expr).map(Some);
            }
            "extend-type" => {
                let expr = self.extend_type_form(id, args, context)?;
                return self.catalog_bundle(id, expr).map(Some);
            }
            "extend-protocol" => {
                let expr = self.extend_protocol_form(id, args, context)?;
                return self.catalog_bundle(id, expr).map(Some);
            }
            "specify!" | "specify" => {
                return self
                    .specify_form(id, args, context, operator_name == "specify")
                    .map(Some);
            }
            "deftype" | "defrecord" => {
                let expr = self.type_form(id, args, context, intrinsic == "defrecord")?;
                return self.catalog_bundle(id, expr).map(Some);
            }
            "assert" => return self.assert_form(id, args, context).map(Some),
            _ => return Ok(None),
        }
    }

    // Declaration snapshots and generated forms must not inflate the frame
    // retained by every ordinary list while its children are analyzed.
    fn macro_declaration(
        &mut self,
        id: NodeId,
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        if !self.top_level {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm("defmacro declarations must appear at top level"),
            ));
        }
        let form = self
            .compiler_read_form(id)
            .map_err(|e| self.error(id, AnalysisErrorKind::MacroExpansion(e.to_string())))?;
        if self.macro_host.bootstrap_seed {
            let lowered = crate::compiler_bootstrap::lower_declaration(&form)
                .map_err(|e| self.error(id, AnalysisErrorKind::MacroExpansion(e)))?
                .1;
            let node = self.import_form(&lowered)?;
            return self.form(node, context);
        }
        self.macro_host
            .register_javascript_macro(
                self.namespace,
                &form,
                self.definition_snapshot(),
                self.engine,
            )
            .map_err(|e| self.error(id, AnalysisErrorKind::MacroExpansion(e)))?;
        Ok(self.push(ExprKind::Nil, self.syntax.nodes[id.0].span.clone(), context))
    }

    // Keep invocation temporaries outside the recursive special-form dispatch.
    // In debug builds otherwise every nested let/if/body carries the stack frame
    // for macro expansion, interop argument lowering and ordinary invocation.
    fn call_form(
        &mut self,
        id: NodeId,
        operator: NodeId,
        args: &[NodeId],
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        // Macro lookup uses the current lexical environment and expression
        // context, before ordinary invocation analysis.
        if let Some(expanded) = self.expand_macro(id, operator, context)? {
            self.macro_depth += 1;
            let result = self.form(expanded, context);
            self.macro_depth -= 1;
            return result.map_err(|error| AnalysisError {
                kind: match error.kind {
                    AnalysisErrorKind::MacroExpansion(_) => error.kind,
                    _ => AnalysisErrorKind::MacroExpansion(format!(
                        "generated by {}: {}",
                        self.spelling(operator),
                        error
                    )),
                },
                span: error.span,
            });
        }
        let callee = self.non_tail_form(operator, ExprContext::Value)?;
        if matches!(self.syntax.nodes[operator.0].kind, NodeKind::Symbol)
            && let ExprKind::CoreFn(core) = self.program.exprs[callee.0].kind
            && !core.accepts_direct_arity(args.len())
        {
            return Err(self.error(
                id,
                AnalysisErrorKind::MacroExpansion(format!(
                    "invalid arity for core numeric/predicate macro: {} arguments",
                    args.len()
                )),
            ));
        }
        if let ExprKind::CoreFn(core) = self.program.exprs[callee.0].kind
            && matches!(
                core,
                CoreFnId::AGet
                    | CoreFnId::ASet
                    | CoreFnId::UncheckedGet
                    | CoreFnId::UncheckedSet
                    | CoreFnId::JsDelete
            )
        {
            let checks = if self.unchecked_arrays {
                CheckedArrays::Off
            } else {
                self.options.array_checks
            };
            let mut values = args
                .iter()
                .map(|&arg| self.non_tail_form(arg, ExprContext::Value))
                .collect::<Result<Vec<_>, _>>()?;
            if matches!(core, CoreFnId::AGet | CoreFnId::ASet) && checks != CheckedArrays::Off {
                let checked = match (core, checks) {
                    (CoreFnId::AGet, CheckedArrays::Warn) => CoreFnId::CheckedAGet,
                    (CoreFnId::AGet, _) => CoreFnId::CheckedAGetStrict,
                    (CoreFnId::ASet, CheckedArrays::Warn) => CoreFnId::CheckedASet,
                    _ => CoreFnId::CheckedASetStrict,
                };
                let callee = self.push(
                    ExprKind::CoreFn(checked),
                    self.syntax.nodes[operator.0].span.clone(),
                    ExprContext::Value,
                );
                let call = self.push(
                    ExprKind::Call { callee, args: values },
                    self.syntax.nodes[id.0].span.clone(),
                    context,
                );
                self.checked_array_exprs.insert(call);
                return Ok(call);
            }
            let receiver = values.remove(0);
            let kind = if core == CoreFnId::JsDelete {
                ExprKind::JsDelete {
                    receiver,
                    key: values[0],
                }
            } else {
                let value = matches!(core, CoreFnId::ASet | CoreFnId::UncheckedSet)
                    .then(|| values.pop().unwrap());
                let target = self.push(
                    ExprKind::JsIndex {
                        receiver,
                        indices: values,
                    },
                    self.syntax.nodes[id.0].span.clone(),
                    ExprContext::Value,
                );
                if let Some(value) = value {
                    ExprKind::SetGlobal { target, value }
                } else {
                    return Ok(self.push(
                        self.program.exprs[target.0].kind.clone(),
                        self.syntax.nodes[id.0].span.clone(),
                        context,
                    ));
                }
            };
            return Ok(self.push(kind, self.syntax.nodes[id.0].span.clone(), context));
        }
        let inline_instance = matches!(
            self.program.exprs[callee.0].kind,
            ExprKind::CoreFn(CoreFnId::IsInstance)
        ) && args.len() == 2
            && matches!(self.syntax.nodes[args[0].0].kind, NodeKind::Symbol);
        let args: Vec<_> = args
            .iter()
            .map(|&arg| self.non_tail_form(arg, ExprContext::Value))
            .collect::<Result<_, _>>()?;
        // CLJS's direct instance? macro reads a symbolic class after the value;
        // expression classes and saved function calls retain argument order.
        if inline_instance {
            return Ok(self.push(
                ExprKind::InstanceOf {
                    class: args[0],
                    value: args[1],
                },
                self.syntax.nodes[id.0].span.clone(),
                context,
            ));
        }
        Ok(self.push(
            ExprKind::Call { callee, args },
            self.syntax.nodes[id.0].span.clone(),
            context,
        ))
    }

    fn resolve(&mut self, id: NodeId, name: &str) -> Result<ExprKind, AnalysisError> {
        if name == "js/-Infinity" {
            return Ok(ExprKind::Number(f64::NEG_INFINITY));
        }
        if matches!(
            name,
            "cljs.core.PersistentQueue.EMPTY" | "cljs.core/PersistentQueue.EMPTY"
        ) {
            return Ok(ExprKind::ReaderLiteral {
                kind: CompilerLiteralKind::Queue,
                items: vec![],
                quoted: false,
            });
        }
        if let Some(local) = self.lookup(name) {
            return Ok(ExprKind::Local(local));
        }
        if self.macro_host.bootstrap_seed
            && self.macro_host.has_source_value(self.namespace, name)
            && !self
                .globals
                .get(&(self.namespace.into(), name.into()))
                .is_some_and(|g| self.program.globals[g.0].declared)
        {
            return Err(self.error(
                id,
                AnalysisErrorKind::Unsupported(format!(
                    "ordinary helper {}/{name} is not available during macro compilation",
                    self.namespace
                )),
            ));
        }
        if self.macro_host.has_javascript_macro(self.namespace, name) {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm(
                    "compile-time macro cannot be consumed as a runtime value; invoke it directly in source so the compiler expands it",
                ),
            ));
        }
        if let Some(name) = name.strip_prefix("cljs.core.")
            && let Some(function) = CoreFnId::resolve(name)
        {
            return Ok(ExprKind::CoreFn(function));
        }
        if let Some((_, original)) = self
            .resolution
            .js_refers
            .iter()
            .find(|(local, _)| local == name)
            .cloned()
            && !self
                .globals
                .get(&(self.namespace.into(), name.into()))
                .is_some_and(|g| self.program.globals[g.0].declared)
        {
            return self.resolve(id, &format!("js/{original}"));
        }
        if name == "cljs.core.ExceptionInfo" {
            return Ok(ExprKind::CoreFn(CoreFnId::ExceptionInfo));
        }
        if let Some(path) = name.strip_prefix("js/") {
            // Standard ClojureScript resolves js/x to a same-named lexical x.
            if let Some(local) = self.lookup(path) {
                self.warn(
                    id,
                    "js-shadowed-by-local",
                    format!("{name} is shadowed by a local"),
                );
                return Ok(ExprKind::Local(local));
            }
            let path = munge_js_path(path);
            if path.iter().any(|part| !js_identifier(part)) {
                return Err(self.error(
                    id,
                    AnalysisErrorKind::Unsupported(
                        "JavaScript paths require identifier components".into(),
                    ),
                ));
            }
            let global = self
                .program
                .js_globals
                .iter()
                .position(|global| global.path == path)
                .map(JsGlobalId)
                .unwrap_or_else(|| {
                    let id = JsGlobalId(self.program.js_globals.len());
                    self.program.js_globals.push(JsGlobal { path });
                    id
                });
            return Ok(ExprKind::JsGlobal(global));
        }
        if let Some((class, member)) = name.split_once('/')
            && class != "js"
            && (member == "new" || (member != ".." && member.starts_with('.')))
            && !self
                .aliases
                .iter()
                .any(|(alias, target)| alias == class || target == class)
            && !self.requires.iter().any(|ns| ns == class)
        {
            let class = self.resolve(id, class)?;
            let class = self.push(
                class,
                self.syntax.nodes[id.0].span.clone(),
                ExprContext::Value,
            );
            return Ok(ExprKind::QualifiedMethod {
                class,
                method: member.strip_prefix('.').map(str::to_owned),
            });
        }
        if let Some((root, fields)) = name.split_once('.')
            && (self.lookup(root).is_some()
                || self
                    .resolution
                    .js_refers
                    .iter()
                    .any(|(local, _)| local == root)
                || self
                    .globals
                    .get(&(self.namespace.into(), root.into()))
                    .is_some_and(|g| self.program.globals[g.0].declared))
        {
            let kind = self.resolve(id, root)?;
            return self.dotted_fields(id, kind, fields);
        }
        let core_path = name
            .strip_prefix("cljs.core.")
            .or_else(|| name.strip_prefix("cljs.core/"))
            .or_else(|| name.strip_prefix("clojure.core/"))
            .unwrap_or(name);
        if let Some((class, fields)) = core_path
            .split_once('.')
            .or_else(|| core_path.split_once('/'))
            && matches!(
                class,
                "PersistentVector" | "PersistentArrayMap" | "PersistentHashMap"
            )
        {
            let kind = if core_path == name {
                self.resolve(id, class)?
            } else {
                ExprKind::CoreFn(CoreFnId::resolve(class).unwrap())
            };
            return self.dotted_fields(id, kind, fields);
        }
        if let Some((class, field)) = name.split_once('/')
            && !matches!(class, "cljs.core" | "clojure.core")
            && !self
                .aliases
                .iter()
                .any(|(alias, target)| alias == class || target == class)
            && !self.requires.iter().any(|ns| ns == class)
            && (self.lookup(class).is_some()
                || self
                    .resolution
                    .js_refers
                    .iter()
                    .any(|(local, _)| local == class)
                || matches!(
                    class,
                    "String"
                        | "Object"
                        | "Array"
                        | "Number"
                        | "Boolean"
                        | "Date"
                        | "RegExp"
                        | "Error"
                        | "Function"
                ))
        {
            let kind = if self.lookup(class).is_some()
                || self
                    .resolution
                    .js_refers
                    .iter()
                    .any(|(local, _)| local == class)
            {
                self.resolve(id, class)?
            } else {
                self.resolve(id, &format!("js/{class}"))?
            };
            return self.dotted_fields(id, kind, field);
        }
        let qualified = (name != "/").then(|| name.split_once('/')).flatten();
        let (namespace, symbol) = if let Some((qualifier, symbol)) = qualified {
            let namespace = self
                .aliases
                .iter()
                .find(|(alias, _)| alias == qualifier)
                .map(|(_, namespace)| namespace.as_str())
                .unwrap_or(qualifier);
            if matches!(namespace, "cljs.core" | "clojure.core") {
                return CoreFnId::resolve(symbol)
                    .map(ExprKind::CoreFn)
                    .ok_or_else(|| {
                        self.error(id, AnalysisErrorKind::UnresolvedSymbol(name.into()))
                    });
            }
            if namespace != self.namespace
                && !self.requires.iter().any(|required| required == namespace)
            {
                if self.options.warning_mode {
                    let namespace = namespace.to_owned();
                    self.warn(id, "undeclared-ns", format!("No such namespace: {namespace}, could not locate {}.cljs, {}.cljc, or JavaScript source providing \"{namespace}\"", namespace.replace('.', "/").replace('-', "_"), namespace.replace('.', "/").replace('-', "_")));
                    self.warn(
                        id,
                        "undeclared-var",
                        format!("Use of undeclared Var {namespace}/{symbol}"),
                    );
                    return self.resolve(id, &format!("js/{namespace}.{symbol}"));
                }
                return Err(self.error(
                    id,
                    AnalysisErrorKind::UnresolvedSymbol(format!(
                        "{name} (unknown namespace or alias {qualifier})"
                    )),
                ));
            }
            (namespace, symbol)
        } else if let Some((_, namespace, original)) = self
            .resolution
            .refers
            .iter()
            .find(|(local, _, _)| local == name)
        {
            if !self
                .globals
                .get(&(self.namespace.into(), name.into()))
                .is_some_and(|g| self.program.globals[g.0].declared)
            {
                if matches!(namespace.as_str(), "cljs.core" | "clojure.core") {
                    return CoreFnId::resolve(original)
                        .map(ExprKind::CoreFn)
                        .ok_or_else(|| {
                            self.error(id, AnalysisErrorKind::UnresolvedSymbol(name.into()))
                        });
                }
                return self
                    .globals
                    .get(&(namespace.clone(), original.clone()))
                    .filter(|g| self.program.globals[g.0].declared)
                    .copied()
                    .map(ExprKind::Global)
                    .ok_or_else(|| {
                        self.error(id, AnalysisErrorKind::UnresolvedSymbol(name.into()))
                    });
            }
            (self.namespace, name)
        } else {
            (self.namespace, name)
        };
        let resolved = self
            .globals
            .get(&(namespace.into(), symbol.into()))
            .filter(|g| self.program.globals[g.0].declared)
            .copied()
            .map(ExprKind::Global)
            .or_else(|| {
                (qualified.is_none() && !self.resolution.excludes.iter().any(|n| n == name))
                    .then(|| CoreFnId::resolve(name))
                    .flatten()
                    .map(ExprKind::CoreFn)
            });
        if let Some(resolved) = resolved {
            if let ExprKind::Global(global) = resolved {
                self.check_private_var(id, global);
            }
            return Ok(resolved);
        }
        if qualified.is_none() && name.contains('.') {
            return self.resolve(id, &format!("js/{name}"));
        }
        let namespace = namespace.to_owned();
        let symbol = symbol.to_owned();
        self.warn(
            id,
            "undeclared-var",
            format!("Use of undeclared Var {namespace}/{symbol}"),
        );
        if self.options.warning_mode || self.type_fields.is_some() {
            let global = *self
                .globals
                .entry((namespace.clone(), symbol.clone()))
                .or_insert_with(|| {
                    let global = GlobalId(self.program.globals.len());
                    self.program.globals.push(GlobalBinding {
                        type_descriptor: false,
                        declared: false,
                        constant: false,
                        boolean_tag: false,
                        metadata: None,
                        namespace,
                        name: symbol,
                        span: self.syntax.nodes[id.0].span.clone(),
                        source_index: self.source_index,
                    });
                    global
                });
            Ok(ExprKind::Global(global))
        } else {
            Err(self.error(id, AnalysisErrorKind::UnresolvedSymbol(name.into())))
        }
    }

    fn keyword(&mut self, id: NodeId, auto_resolve: bool) -> Result<ExprKind, AnalysisError> {
        // Macro-produced names are semantic data. Their spans may cover an
        // entire invocation, so they must never be decoded from that source.
        let (namespace, name) = if let Some(form) =
            self.generated.get(id.0).and_then(Option::as_ref)
            && let DatumView::Keyword(name) = form.data().inspect()
        {
            (name.namespace().map(<[u16]>::to_vec), name.name().to_vec())
        } else {
            let raw = &self.spelling(id)[if auto_resolve { 2 } else { 1 }..];
            let (namespace, name) = if raw == "/" {
                (None, raw)
            } else {
                raw.split_once('/')
                    .map_or((None, raw), |(ns, name)| (Some(ns), name))
            };
            let namespace = if auto_resolve {
                match namespace {
                    None => Some(self.namespace.to_owned()),
                    Some(alias) => Some(
                        self.macro_host
                            .reader_aliases(self.namespace, self.aliases, self.requires)
                            .into_iter()
                            .rev()
                            .find(|(candidate, _)| candidate == alias)
                            .map(|(_, target)| target)
                            .ok_or_else(|| {
                                self.error(
                                    id,
                                    AnalysisErrorKind::UnresolvedSymbol(format!(
                                        "keyword namespace alias {alias}"
                                    )),
                                )
                            })?,
                    ),
                }
            } else {
                namespace.map(str::to_owned)
            };
            (
                namespace.map(|ns| ns.encode_utf16().collect()),
                name.encode_utf16().collect(),
            )
        };
        let namespace = namespace.map(|units| self.own_string(units));
        let name = self.own_string(name);
        let id = NamedLiteralId(self.program.named_literals.len());
        self.program
            .named_literals
            .push(NamedLiteral { namespace, name });
        Ok(ExprKind::Keyword(id))
    }

    fn own_string(&mut self, units: Vec<u16>) -> StringId {
        let id = StringId(self.program.strings.len());
        self.program.strings.push(units);
        id
    }

    fn validate_symbol(&self, id: NodeId) -> Result<(), AnalysisError> {
        if let Some(form) = self.generated.get(id.0).and_then(Option::as_ref)
            && let DatumView::Symbol(name) = form.data().inspect()
            && (String::from_utf16(name.name()).is_err()
                || name
                    .namespace()
                    .is_some_and(|ns| String::from_utf16(ns).is_err()))
        {
            return Err(self.error(
                id,
                AnalysisErrorKind::Unsupported(
                    "non-Unicode executable symbol generated by a macro".into(),
                ),
            ));
        }
        Ok(())
    }

    fn symbol_literal(&mut self, id: NodeId) -> ExprKind {
        let (namespace, name) = if let Some(form) =
            self.generated.get(id.0).and_then(Option::as_ref)
            && let DatumView::Symbol(name) = form.data().inspect()
        {
            (name.namespace().map(<[u16]>::to_vec), name.name().to_vec())
        } else {
            let raw = self.spelling(id);
            let (namespace, name) = if raw == "/" {
                (None, raw)
            } else {
                raw.split_once('/')
                    .map_or((None, raw), |(ns, name)| (Some(ns), name))
            };
            (
                namespace.map(|ns| ns.encode_utf16().collect()),
                name.encode_utf16().collect(),
            )
        };
        let namespace = namespace.map(|ns| self.own_string(ns));
        let name = self.own_string(name);
        let literal = NamedLiteralId(self.program.named_literals.len());
        self.program
            .named_literals
            .push(NamedLiteral { namespace, name });
        ExprKind::Symbol(literal)
    }

    fn check_map_keys(&mut self, children: &[NodeId]) -> Result<(), AnalysisError> {
        let mut seen = std::collections::BTreeSet::new();
        for &key in children.iter().step_by(2) {
            if let Some(data) = self.reader_datum(key)?
                && !seen.insert(data)
            {
                return Err(self.error(key, AnalysisErrorKind::InvalidForm("duplicate map key")));
            }
        }
        Ok(())
    }

    fn check_set_members(
        &mut self,
        children: &[NodeId],
        _quoted: bool,
    ) -> Result<(), AnalysisError> {
        let mut seen = std::collections::BTreeSet::new();
        for &member in children {
            // The reader rejects identical readable forms before evaluation,
            // including executable forms. Distinct operands retain runtime checks.
            if let Some(data) = self.reader_datum(member)?
                && !seen.insert(data)
            {
                return Err(self.error(
                    member,
                    AnalysisErrorKind::InvalidForm("duplicate set member"),
                ));
            }
        }
        Ok(())
    }

    // The compiler macro bridge preserves all source children, including equal
    // executable set operands. Validate readable duplicates here before
    // native set storage deduplicates the macro's ordinary data projection.
    fn check_compiler_sets(&mut self, root: NodeId) -> Result<(), AnalysisError> {
        let mut pending = vec![(root, false)];
        while let Some((id, quoted)) = pending.pop() {
            match self.syntax.nodes[id.0].kind.clone() {
                NodeKind::Collection { kind, children } => {
                    if kind == CollectionKind::Set {
                        self.check_set_members(&children, quoted)?;
                    }
                    let quotes = quoted
                        || (kind == CollectionKind::List
                            && children.len() == 2
                            && matches!(self.syntax.nodes[children[0].0].kind, NodeKind::Symbol)
                            && self.spelling(children[0]) == "quote");
                    pending.extend(children.into_iter().rev().map(|child| (child, quotes)));
                }
                NodeKind::Prefix { kind, form } => pending.push((
                    form,
                    quoted || matches!(kind, PrefixKind::Quote | PrefixKind::SyntaxQuote),
                )),
                NodeKind::Metadata { metadata, target } => {
                    pending.push((target, quoted));
                    pending.push((metadata, true));
                }
                NodeKind::Tagged { tag, form } => {
                    pending.push((form, quoted));
                    pending.push((tag, true));
                }
                NodeKind::Conditional { branches, .. } => pending.push((branches, quoted)),
                NodeKind::NamespacedMap { map, .. } => pending.push((map, quoted)),
                _ => {}
            }
        }
        Ok(())
    }

    // Read identity directly from owned source/Form nodes, without invoking the
    // native interpreter or reconstructing effectful children from map storage.
    fn reader_datum(&mut self, id: NodeId) -> Result<Option<ReaderDatum>, AnalysisError> {
        if self.macro_depth > 0 && self.analysis_depth >= 128 {
            return Err(self.error(
                id,
                AnalysisErrorKind::MacroExpansion(
                    "generated runtime form nesting limit exceeded".into(),
                ),
            ));
        }
        self.analysis_depth += 1;
        let result = self.reader_datum_inner(id);
        self.analysis_depth -= 1;
        result
    }

    fn reader_datum_inner(&mut self, id: NodeId) -> Result<Option<ReaderDatum>, AnalysisError> {
        let node = self.syntax.nodes[id.0].clone();
        if let Some(form) = self.generated.get(id.0).and_then(Option::as_ref)
            && let DatumView::CompilerLiteral(kind) = form.data().inspect()
        {
            let payload = form.data().compiler_literal_data().unwrap();
            return Ok(match (kind, payload.inspect()) {
                (CompilerLiteralKind::BigInteger, DatumView::String(text)) => {
                    Some(ReaderDatum::Number(0, String::from_utf16_lossy(&text)))
                }
                (CompilerLiteralKind::Uuid, DatumView::String(text)) => Some(
                    ReaderDatum::ReaderLiteral(0, String::from_utf16_lossy(&text)),
                ),
                (CompilerLiteralKind::Instant, DatumView::Double(millis)) => {
                    Some(ReaderDatum::ReaderLiteral(1, millis.to_string()))
                }
                _ => None,
            });
        }
        let data = match node.kind {
            NodeKind::Nil => ReaderDatum::Nil,
            NodeKind::Boolean(value) => ReaderDatum::Bool(value),
            NodeKind::Number(kind) => {
                if self.generated.get(id.0).and_then(Option::as_ref).is_none()
                    && let Some((category, spelling)) = crate::reader_literals::number_key(self.spelling(id), kind) {
                    return Ok(Some(ReaderDatum::Number(category, spelling)));
                }
                let Some(value) = self
                    .numbers
                    .get(id.0)
                    .copied()
                    .flatten()
                    .unwrap_or_else(|| number(self.spelling(id), kind))
                else {
                    return Ok(None);
                };
                ReaderDatum::Number(
                    u8::from(kind == NumberKind::Float),
                    if kind == NumberKind::Integer {
                        if let Some(form) = self.generated.get(id.0).and_then(Option::as_ref) {
                            match form.data().inspect() {
                                DatumView::Integer(n) => n.to_string(),
                                _ => value.to_string(),
                            }
                        } else {
                            crate::reader_literals::integer_key(self.spelling(id))
                        }
                    } else {
                        format!("{:x}", if value == 0.0 { 0 } else { value.to_bits() })
                    },
                )
            }
            NodeKind::String(value) => ReaderDatum::String(value.to_utf16(self.source)),
            NodeKind::Keyword { auto_resolve } => {
                let ExprKind::Keyword(name) = self.keyword(id, auto_resolve)? else {
                    unreachable!()
                };
                self.reader_named(true, name)
            }
            NodeKind::Symbol => {
                let ExprKind::Symbol(name) = self.symbol_literal(id) else {
                    unreachable!()
                };
                self.reader_named(false, name)
            }
            NodeKind::Prefix {
                kind: PrefixKind::Quote,
                form,
            } => {
                let Some(data) = self.reader_datum(form)? else {
                    return Ok(None);
                };
                ReaderDatum::Sequential(vec![
                    ReaderDatum::Named(false, None, "quote".encode_utf16().collect()),
                    data,
                ])
            }
            NodeKind::Collection {
                kind:
                    CollectionKind::List
                    | CollectionKind::Vector
                    | CollectionKind::Map
                    | CollectionKind::Set,
                ref children,
            } => {
                let mut values = Vec::with_capacity(children.len());
                for &child in children {
                    let Some(data) = self.reader_datum(child)? else {
                        return Ok(None);
                    };
                    values.push(data);
                }
                if matches!(
                    node.kind,
                    NodeKind::Collection {
                        kind: CollectionKind::Map,
                        ..
                    }
                ) {
                    let mut pairs = values
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|p| (p[0].clone(), p[1].clone()))
                        .collect::<Vec<_>>();
                    pairs.sort();
                    ReaderDatum::Map(pairs)
                } else if matches!(
                    node.kind,
                    NodeKind::Collection {
                        kind: CollectionKind::Set,
                        ..
                    }
                ) {
                    values.sort();
                    ReaderDatum::Set(values)
                } else {
                    ReaderDatum::Sequential(values)
                }
            }
            _ => return Ok(None),
        };
        Ok(Some(data))
    }

    fn reader_named(&self, keyword: bool, id: NamedLiteralId) -> ReaderDatum {
        let named = &self.program.named_literals[id.0];
        ReaderDatum::Named(
            keyword,
            named.namespace.map(|ns| self.program.strings[ns.0].clone()),
            self.program.strings[named.name.0].clone(),
        )
    }

    /// Quote traverses source/owned macro data directly, without resolution,
    /// macro lookup, printing, reparsing, or constructing an interpreter value.
    fn datum(&mut self, id: NodeId, context: ExprContext) -> Result<ExprId, AnalysisError> {
        if self.macro_depth > 0 && self.analysis_depth >= 128 {
            return Err(self.error(
                id,
                AnalysisErrorKind::MacroExpansion(
                    "generated runtime form nesting limit exceeded".into(),
                ),
            ));
        }
        self.analysis_depth += 1;
        let result = self.datum_inner(id, context);
        self.analysis_depth -= 1;
        result
    }

    fn datum_inner(&mut self, id: NodeId, context: ExprContext) -> Result<ExprId, AnalysisError> {
        let node = self.syntax.nodes[id.0].clone();
        if self.needs_reader(id) {
            let expanded = self.expand_reader(id)?;
            return self.datum(expanded, context);
        }
        if let Some(form) = self.generated.get(id.0).and_then(Option::as_ref).cloned()
            && let DatumView::CompilerLiteral(kind) = form.data().inspect()
        {
            let value = self.reader_literal(&form, kind, context, true)?;
            return self.attach_metadata(id, value, context, true);
        }
        let kind = match node.kind {
            NodeKind::Symbol => self.symbol_literal(id),
            NodeKind::Collection {
                kind:
                    CollectionKind::List
                    | CollectionKind::Vector
                    | CollectionKind::Map
                    | CollectionKind::Set,
                ref children,
            } => {
                if matches!(
                    node.kind,
                    NodeKind::Collection {
                        kind: CollectionKind::Map,
                        ..
                    }
                ) {
                    self.check_map_keys(children)?;
                } else if matches!(
                    node.kind,
                    NodeKind::Collection {
                        kind: CollectionKind::Set,
                        ..
                    }
                ) {
                    self.check_set_members(children, true)?;
                }
                let items = children
                    .iter()
                    .map(|&child| self.datum(child, ExprContext::Value))
                    .collect::<Result<_, _>>()?;
                if matches!(
                    node.kind,
                    NodeKind::Collection {
                        kind: CollectionKind::List,
                        ..
                    }
                ) {
                    ExprKind::List(items)
                } else if matches!(
                    node.kind,
                    NodeKind::Collection {
                        kind: CollectionKind::Map,
                        ..
                    }
                ) {
                    ExprKind::Map(items)
                } else if matches!(
                    node.kind,
                    NodeKind::Collection {
                        kind: CollectionKind::Set,
                        ..
                    }
                ) {
                    ExprKind::Set(items)
                } else {
                    ExprKind::Vector(items)
                }
            }
            NodeKind::Prefix {
                kind: PrefixKind::Quote,
                form,
            } => {
                let name = self.own_string("quote".encode_utf16().collect());
                let literal = NamedLiteralId(self.program.named_literals.len());
                self.program.named_literals.push(NamedLiteral {
                    namespace: None,
                    name,
                });
                let head = self.push(
                    ExprKind::Symbol(literal),
                    node.span.clone(),
                    ExprContext::Value,
                );
                let data = self.datum(form, ExprContext::Value)?;
                ExprKind::List(vec![head, data])
            }
            // Only scalar kinds can use ordinary analysis without resolving or
            // executing anything. Other reader facilities keep their errors.
            _ => return self.form(id, context),
        };
        let value = self.push(kind, node.span, context);
        self.attach_metadata(id, value, context, true)
    }

    fn metadata_entry(&self, node: NodeId, key: &str) -> Option<Form> {
        let metadata = self.generated.get(node.0)?.as_ref()?.metadata()?;
        metadata
            .children()
            .as_chunks::<2>()
            .0
            .iter()
            .find_map(|pair| {
                matches!(pair[0].data().inspect(), DatumView::Keyword(name)
                if name.namespace().map(|ns| format!("{}/", String::from_utf16_lossy(ns))).unwrap_or_default() + &String::from_utf16_lossy(name.name()) == key)
                .then(|| pair[1].clone())
            })
    }

    fn metadata_is(&self, node: NodeId, key: &str, expected: &str) -> bool {
        self.metadata_entry(node, key)
            .is_some_and(|v| match v.data().inspect() {
                DatumView::Boolean(value) => expected == if value { "true" } else { "false" },
                DatumView::Symbol(name) => {
                    name.namespace().is_none() && String::from_utf16_lossy(name.name()) == expected
                }
                _ => false,
            })
    }

    fn data_source(&mut self, text: &str) -> Result<ExprId, AnalysisError> {
        let source = Source::new("compiler-definition-metadata", text);
        let form = crate::compiler_reader::read_form(
            &source,
            &crate::compiler_reader::NamespaceView::new(self.namespace, &source),
        )
        .map_err(|e| AnalysisError {
            span: e.span.bytes(),
            kind: AnalysisErrorKind::MacroExpansion(e.to_string()),
        })?
        .expect("metadata form");
        let node = self.import_form(&form)?;
        self.datum(node, ExprContext::Value)
    }

    fn definition_snapshot(&self) -> crate::compilation_session::DefinitionSnapshot {
        crate::compilation_session::DefinitionSnapshot {
            namespace: self.namespace.into(),
            aliases: self.aliases.to_vec(),
            requires: self.requires.to_vec(),
            resolution: self.resolution.clone(),
            resolution_program: crate::program_fragment::resolution_snapshot(&self.program),
            options: self.options,
            unchecked_arrays: self.unchecked_arrays,
            source_index: self.source_index,
            definition_name: self.definition_name.clone(),
            extension_frontier: self.macro_host.session_handle().map_or(0, |s| s.extension_frontier()),
        }
    }

    fn drain_deferred(&mut self) -> Result<(), AnalysisError> {
        let Some(session) = self.macro_host.session_handle() else {
            return Ok(());
        };
        for (placeholder, unit) in std::mem::take(&mut self.deferred) {
            let (program, warnings) = session.analyze(unit, self.engine).map_err(|message| {
                session.diagnostic(unit).unwrap_or_else(|| AnalysisError {
                    span: self.program.exprs[placeholder.0].span.clone(),
                    kind: AnalysisErrorKind::MacroExpansion(message),
                })
            })?;
            let globals = self.program.globals.clone();
            let root = crate::program_fragment::append(&mut self.program, &program);
            // Snapshot metadata resolves references within the fragment, but must
            // not roll back a later source declaration's live metadata/flags.
            self.program.globals[..globals.len()].clone_from_slice(&globals);
            for (index, global) in self.program.globals.iter().enumerate() {
                self.globals.insert((global.namespace.clone(), global.name.clone()), GlobalId(index));
            }
            self.program.exprs[placeholder.0].kind = ExprKind::Do(vec![root]);
            self.warnings.extend(warnings);
        }
        Ok(())
    }
    fn definition(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        context: ExprContext,
        once: bool,
    ) -> Result<ExprId, AnalysisError> {
        if !(1..=3).contains(&args.len())
            || (once && args.len() != 2)
            || (args.len() == 3
                && !matches!(self.syntax.nodes[args[1].0].kind, NodeKind::String(_)))
        {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm(
                    "def requires a name, optional docstring and initializer",
                ),
            ));
        }
        if args.len() == 1 && context == ExprContext::Value {
            return Err(self.error(id, AnalysisErrorKind::Unsupported("uninitialized def in a value position (the reference emits invalid JavaScript)".into())));
        }
        let raw = self.spelling(args[0]);
        let name = if let Some((namespace, symbol)) = raw.split_once('/')
            && raw != "/"
        {
            self.validate_symbol(args[0])?;
            if namespace != self.namespace || symbol.is_empty() || symbol.contains(['/', '.']) {
                return Err(self.error(args[0], AnalysisErrorKind::InvalidBinding));
            }
            symbol.to_owned()
        } else {
            self.validate_symbol(args[0])?;
            if !matches!(self.syntax.nodes[args[0].0].kind, NodeKind::Symbol) {
                return Err(self.error(args[0], AnalysisErrorKind::InvalidBinding));
            }
            raw.to_owned()
        };
        let key = (self.namespace.to_owned(), name.clone());
        if name.len() > 1
            && name.starts_with('*')
            && name.ends_with('*')
            && !self.metadata_is(args[0], "dynamic", "true")
        {
            self.warn(args[0], "non-dynamic-earmuffed-var", format!("{name} not declared dynamic and thus is not dynamically rebindable, but its name suggests otherwise. Please either indicate ^:dynamic {name} or change the name"));
        }
        let global = if let Some(&global) = self.globals.get(&key) {
            if self.program.globals[global.0].constant {
                return Err(self.error(
                    args[0],
                    AnalysisErrorKind::InvalidForm("Can't redefine a constant"),
                ));
            }
            global
        } else {
            let global = GlobalId(self.program.globals.len());
            self.program.globals.push(GlobalBinding {
                type_descriptor: false,
                declared: true,
                constant: false,
                boolean_tag: false,
                metadata: None,
                namespace: self.namespace.into(),
                name,
                span: self.syntax.nodes[args[0].0].span.clone(),
                source_index: self.source_index,
            });
            self.globals.insert(key, global);
            global
        };
        self.program.globals[global.0].declared = true;
        self.program.globals[global.0].constant = self.metadata_is(args[0], "const", "true");
        self.program.globals[global.0].boolean_tag = self.metadata_is(args[0], "tag", "boolean");
        let test_form = self.metadata_entry(args[0], "test");
        let mut entries = Vec::new();
        if let Some(metadata) = self
            .generated
            .get(args[0].0)
            .and_then(Option::as_ref)
            .and_then(Form::metadata)
        {
            for pair in metadata.children().as_chunks::<2>().0 {
                let key = match pair[0].data().inspect() {
                    DatumView::Keyword(name) if name.namespace().is_none() => {
                        String::from_utf16_lossy(name.name())
                    }
                    _ => String::new(),
                };
                if key == "test" {
                    continue;
                }
                let key_node = self.import_form(&pair[0])?;
                entries.push(self.datum(key_node, ExprContext::Value)?);
                let value = if key == "arglists" && pair[1].children().len() == 2 && pair[1].children().first().is_some_and(|f|
                    matches!(f.data().inspect(), DatumView::Symbol(name) if name.namespace().is_none() && String::from_utf16_lossy(name.name()) == "quote")) {
                    pair[1].children()[1].clone()
                } else { pair[1].clone() };
                let value = self.import_form(&value)?;
                entries.push(self.datum(value, ExprContext::Value)?);
            }
        }
        if args.len() == 3 {
            entries.push(self.data_source(":doc")?);
            entries.push(self.datum(args[1], ExprContext::Value)?);
        }
        let (line, column) = crate::LineIndex::new(self.source)
            .line_column(self.syntax.nodes[id.0].span.start)
            .unwrap();
        for (key, value) in [(":line", line.to_string()), (":column", column.to_string())] {
            entries.push(self.data_source(key)?);
            entries.push(self.data_source(&value)?);
        }
        let metadata = self.push(
            ExprKind::Map(entries),
            self.syntax.nodes[id.0].span.clone(),
            ExprContext::Value,
        );
        if let Some(previous) = self.program.globals[global.0].metadata
            && self.metadata_value(previous, "declared").is_some()
            && let (Some(declared), Some(defined)) = (
                self.metadata_value(previous, "arglists"),
                self.metadata_value(metadata, "arglists"),
            )
        {
            let declared = self.datum_text(declared);
            let defined = self.datum_text(defined);
            if declared != defined {
                self.warn(
                    args[0],
                    "declared-arglists-mismatch",
                    format!(
                        "{}/{} declared arglists {declared} mismatch defined arglists {defined}",
                        self.namespace, self.program.globals[global.0].name
                    ),
                );
            }
        }
        self.program.globals[global.0].metadata = Some(metadata);
        self.macro_host
            .register_runtime_var(
                self.namespace,
                &self.program.globals[global.0].name,
                self.metadata_is(args[0], "private", "true"),
            )
            .map_err(|e| self.error(args[0], AnalysisErrorKind::MacroExpansion(e)))?;
        // Registration precedes initializer/test analysis, so both see the live global.
        let previous_definition = self
            .definition_name
            .replace(self.program.globals[global.0].name.clone());
        let initializer = args.get(if args.len() == 3 { 2 } else { 1 }).copied();
        if initializer.is_some() {
            self.program.globals[global.0].type_descriptor = false;
        }
        let top_level = self.top_level;
        self.top_level = false;
        let init = if top_level && self.macro_host.defers_source_units() {
            let form = initializer.map(|node| self.compiler_read_form(node)).transpose()
                .map_err(|e| self.error(id, AnalysisErrorKind::MacroExpansion(e.to_string())))?;
            let session = self.macro_host.session_handle().expect("compilation session");
            let unit = session.register(
                format!("{}/{}", self.namespace, self.program.globals[global.0].name),
                form, self.definition_snapshot(), self.macro_host.macro_analysis_frame(), once,
                {
                    let form = self.compiler_read_form(args[0]).map_err(|e| self.error(id, AnalysisErrorKind::MacroExpansion(e.to_string())))?;
                    let loc = crate::compiler_data::DiagnosticLocation::from_span(&form.span());
                    format!("{}:{}:{}", loc.source, loc.line, loc.column)
                },
            ).map_err(|e| self.error(id, AnalysisErrorKind::MacroExpansion(e)))?;
            Ok(unit.map(|unit| {
                let placeholder = self.push(ExprKind::Nil, self.syntax.nodes[initializer.unwrap().0].span.clone(), ExprContext::Value);
                self.deferred.push((placeholder, unit));
                placeholder
            }))
        } else {
            initializer.map(|init| self.non_tail_form(init, ExprContext::Value)).transpose()
        };
        self.top_level = top_level;
        self.definition_name = previous_definition;
        let init = init?;
        if self.metadata_is(args[0], "macro", "true")
            && let Some(init) = init
            && matches!(self.program.exprs[init.0].kind, ExprKind::Fn { .. })
        {
            self.program.macro_functions.insert(init);
        }
        let test = test_form
            .map(|f| {
                let node = self.import_form(&f)?;
                self.non_tail_form(node, ExprContext::Value)
            })
            .transpose()?;
        Ok(self.push(
            ExprKind::Def {
                global,
                init,
                test,
                once,
            },
            self.syntax.nodes[id.0].span.clone(),
            context,
        ))
    }

    fn declare_form(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        if !args.is_empty() && context == ExprContext::Value {
            return Err(self.error(id, AnalysisErrorKind::Unsupported("uninitialized declare in a value position (the reference emits invalid JavaScript)".into())));
        }
        let mut last = None;
        for &name in args {
            let text = self.spelling(name);
            let local_name = if let Some((namespace, member)) = text.split_once('/') {
                if namespace != self.namespace {
                    return Err(self.error(name, AnalysisErrorKind::InvalidBinding));
                }
                member.to_owned()
            } else {
                self.binding_name(name)?.to_owned()
            };
            let key = (self.namespace.to_owned(), local_name);
            // A declaration after a definition preserves previous analyzer metadata.
            if !self
                .globals
                .get(&key)
                .is_some_and(|g| self.program.globals[g.0].metadata.is_some())
            {
                self.definition(id, &[name], ExprContext::Discard, false)?;
                let global = self.globals[&key];
                let marker = self.data_source(":declared")?;
                let yes = self.data_source("true")?;
                let metadata = self.program.globals[global.0].metadata.unwrap();
                if let ExprKind::Map(entries) = &mut self.program.exprs[metadata.0].kind {
                    entries.extend([marker, yes]);
                }
            }
            last = Some(self.globals[&key]);
        }
        let kind = last.map_or(ExprKind::Nil, |global| ExprKind::Def {
            global,
            init: None,
            test: None,
            once: false,
        });
        Ok(self.push(kind, self.syntax.nodes[id.0].span.clone(), context))
    }

    fn var_form(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        if args.len() != 1 || !matches!(self.syntax.nodes[args[0].0].kind, NodeKind::Symbol) {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm("var requires one global symbol"),
            ));
        }
        let name = self.spelling(args[0]).to_owned();
        if let Some((namespace, local, metadata)) = self.macro_host.reflected_macro(self.namespace, &name) {
            let symbol = self.data_source(&format!("{namespace}/{local}"))?;
            let c = crate::compiler_data::DataBuilder;
            let metadata = metadata.unwrap_or_else(|| c.map(&[(c.keyword(crate::compiler_data::Name::new(None, "macro")), c.boolean(true))]).unwrap());
            let quoted = c.list(&[c.symbol(crate::compiler_data::Name::new(None, "quote")), metadata]).unwrap();
            let original = self.compiler_read_form(id).map_err(|e| self.error(id, AnalysisErrorKind::MacroExpansion(e.to_string())))?;
            let expanded = Form::from_expansion(quoted, &original, crate::compiler_data::Name::new(None, "var"));
            let node = self.import_form(&expanded)?;
            let metadata = self.non_tail_form(node, ExprContext::Value)?;
            let callee = self.push(ExprKind::CoreFn(CoreFnId::CompilerMacroVar), self.syntax.nodes[id.0].span.clone(), ExprContext::Value);
            return Ok(self.push(ExprKind::Call { callee, args: vec![symbol, metadata] }, self.syntax.nodes[id.0].span.clone(), context));
        }
        let locals = std::mem::take(&mut self.env);
        let previous_var = self.resolving_var;
        self.resolving_var = true;
        let resolved = self.resolve(args[0], &name);
        self.resolving_var = previous_var;
        self.env = locals;
        let resolved = resolved?;
        let (namespace, binding_name, metadata) = match &resolved {
            ExprKind::Global(global) => {
                let b = &self.program.globals[global.0];
                if let Some((namespace, name)) = b.name.rsplit_once('.') {
                    // A dotted definition writes a host path. Taking its Var
                    // resolves that host namespace/name, not the enclosing
                    // CLJS definition's spelling or source metadata.
                    let namespace = namespace.to_owned();
                    let name = name.to_owned();
                    // The public Var metadata expands its :test lookup into
                    // both a truth test and a property read. Both unresolved
                    // host-namespace references deliver analyzer warnings.
                    for _ in 0..2 {
                        if namespace != self.namespace && !self.requires.contains(&namespace) {
                            let path = namespace.replace('.', "/").replace('-', "_");
                            self.warn(args[0], "undeclared-ns", format!("No such namespace: {namespace}, could not locate {path}.cljs, {path}.cljc, or JavaScript source providing \"{namespace}\""));
                        }
                        if !self
                            .globals
                            .get(&(namespace.clone(), name.clone()))
                            .is_some_and(|g| self.program.globals[g.0].declared)
                        {
                            self.warn(
                                args[0],
                                "undeclared-var",
                                format!("Use of undeclared Var {namespace}/{name}"),
                            );
                        }
                    }
                    (namespace, name, None)
                } else {
                    (b.namespace.clone(), b.name.clone(), b.metadata)
                }
            }
            ExprKind::CoreFn(core) => {
                // Aliases can share a runtime operation while retaining their
                // own public Var name, documentation and source arity metadata.
                let requested = name.rsplit('/').next().unwrap();
                let entries = || {
                    include_str!("core_var_metadata.tsv")
                        .lines()
                        .filter_map(|row| {
                            let (n, meta) = row.split_once('\t')?;
                            (CoreFnId::resolve(n) == Some(*core)).then_some((n, meta))
                        })
                };
                let canonical = entries()
                    .find(|(n, _)| *n == requested)
                    .or_else(|| entries().next());
                let (name, metadata) = if let Some((n, metadata)) = canonical {
                    (n.to_owned(), Some(self.data_source(metadata)?))
                } else {
                    (name.rsplit('/').next().unwrap().to_owned(), None)
                };
                ("cljs.core".into(), name, metadata)
            }
            _ => {
                return Err(self.error(
                    args[0],
                    AnalysisErrorKind::InvalidForm("var requires a global binding"),
                ));
            }
        };
        let value = self.push(
            resolved,
            self.syntax.nodes[args[0].0].span.clone(),
            ExprContext::Value,
        );
        let namespace = self.own_string(namespace.encode_utf16().collect());
        let name = self.own_string(binding_name.encode_utf16().collect());
        let symbol = NamedLiteralId(self.program.named_literals.len());
        self.program.named_literals.push(NamedLiteral {
            namespace: Some(namespace),
            name,
        });
        Ok(self.push(
            ExprKind::Var {
                definition: true,
                value,
                symbol,
                metadata,
            },
            self.syntax.nodes[id.0].span.clone(),
            context,
        ))
    }

    fn resolve_form(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        let symbol = args
            .first()
            .and_then(|&arg| match &self.syntax.nodes[arg.0].kind {
                NodeKind::Prefix {
                    kind: PrefixKind::Quote,
                    form,
                } => Some(*form),
                NodeKind::Collection {
                    kind: CollectionKind::List,
                    children,
                } if children.len() == 2 && self.spelling(children[0]) == "quote" => {
                    Some(children[1])
                }
                _ => None,
            });
        let Some(symbol) = symbol
            .filter(|s| args.len() == 1 && matches!(self.syntax.nodes[s.0].kind, NodeKind::Symbol))
        else {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm("Argument to resolve must be a quoted symbol"),
            ));
        };
        let name = self.spelling(symbol).to_owned();
        if self.macro_host.reflected_macro(self.namespace, &name).is_some() { return self.var_form(id, &[symbol], context); }
        let resolved = self.existence_target(symbol)?;
        let value = if let ExprKind::Local(local) = resolved {
            let (line, column) = crate::LineIndex::new(self.source)
                .line_column(self.program.locals[local.0].span.start)
                .unwrap();
            let metadata = self.data_source(&format!("{{:ns nil :name {name} :doc nil :file nil :line {line} :column {column} :test nil :arglists ()}}"))?;
            let value = self.push(
                resolved,
                self.syntax.nodes[symbol.0].span.clone(),
                ExprContext::Value,
            );
            let name = self.own_string(name.encode_utf16().collect());
            let sym = NamedLiteralId(self.program.named_literals.len());
            self.program.named_literals.push(NamedLiteral {
                namespace: None,
                name,
            });
            self.push(
                ExprKind::Var {
                    definition: false,
                    value,
                    symbol: sym,
                    metadata: Some(metadata),
                },
                self.syntax.nodes[id.0].span.clone(),
                context,
            )
        } else if let ExprKind::Global(global) = resolved {
            let binding = self.program.globals[global.0].clone();
            let value = self.push(
                resolved,
                self.syntax.nodes[symbol.0].span.clone(),
                ExprContext::Value,
            );
            let namespace = self.own_string(binding.namespace.encode_utf16().collect());
            let name = self.own_string(binding.name.encode_utf16().collect());
            let sym = NamedLiteralId(self.program.named_literals.len());
            self.program.named_literals.push(NamedLiteral {
                namespace: Some(namespace),
                name,
            });
            self.push(
                ExprKind::Var {
                    definition: binding.metadata.is_some(),
                    value,
                    symbol: sym,
                    metadata: binding.metadata,
                },
                self.syntax.nodes[id.0].span.clone(),
                context,
            )
        } else {
            self.var_form(id, &[symbol], context)?
        };

        let ExprKind::Var { value: root, .. } = self.program.exprs[value.0].kind else {
            unreachable!()
        };
        let test = self.push(
            ExprKind::BindingExists(root),
            self.syntax.nodes[id.0].span.clone(),
            ExprContext::Value,
        );
        let no = self.push(ExprKind::Nil, self.syntax.nodes[id.0].span.clone(), context);
        Ok(self.push(
            ExprKind::If {
                test,
                yes: value,
                no,
            },
            self.syntax.nodes[id.0].span.clone(),
            context,
        ))
    }

    fn existence_target(&mut self, node: NodeId) -> Result<ExprKind, AnalysisError> {
        let name = self.spelling(node).to_owned();
        match self.resolve(node, &name) {
            Ok(value) => Ok(value),
            Err(AnalysisError {
                kind: AnalysisErrorKind::UnresolvedSymbol(_),
                ..
            }) => {
                let (namespace, name) = if let Some((ns, name)) = name.split_once('/') {
                    (
                        self.aliases
                            .iter()
                            .find(|(a, _)| a == ns)
                            .map(|(_, ns)| ns.as_str())
                            .unwrap_or(ns),
                        name,
                    )
                } else {
                    (self.namespace, name.as_str())
                };
                let key = (namespace.to_owned(), name.to_owned());
                let global = *self.globals.entry(key).or_insert_with(|| {
                    let g = GlobalId(self.program.globals.len());
                    self.program.globals.push(GlobalBinding {
                        type_descriptor: false,
                        declared: false,
                        constant: false,
                        boolean_tag: false,
                        metadata: None,
                        namespace: namespace.into(),
                        name: name.into(),
                        span: self.syntax.nodes[node.0].span.clone(),
                        source_index: self.source_index,
                    });
                    g
                });
                Ok(ExprKind::Global(global))
            }
            Err(error) => Err(error),
        }
    }

    fn exists_form(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        if args.len() != 1 {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm("exists? requires one argument"),
            ));
        }
        let symbolic = matches!(self.syntax.nodes[args[0].0].kind, NodeKind::Symbol);
        let value = if symbolic {
            let kind = self.existence_target(args[0])?;
            self.push(
                kind,
                self.syntax.nodes[args[0].0].span.clone(),
                ExprContext::Value,
            )
        } else {
            self.non_tail_form(args[0], ExprContext::Value)?
        };
        let kind = if symbolic {
            ExprKind::Exists(value)
        } else {
            let callee = self.push(
                ExprKind::CoreFn(CoreFnId::IsSome),
                self.syntax.nodes[id.0].span.clone(),
                ExprContext::Value,
            );
            ExprKind::Call {
                callee,
                args: vec![value],
            }
        };
        Ok(self.push(kind, self.syntax.nodes[id.0].span.clone(), context))
    }

    fn assignment_target(&mut self, node: NodeId) -> Result<ExprId, AnalysisError> {
        let target = self.non_tail_form(node, ExprContext::Value)?;
        let target = if let ExprKind::BooleanHint(inner) = self.program.exprs[target.0].kind {
            inner
        } else {
            target
        };
        match self.program.exprs[target.0].kind {
            ExprKind::Global(global) if self.program.globals[global.0].constant => {
                return Err(self.error(
                    node,
                    AnalysisErrorKind::InvalidForm("Can't set! a constant"),
                ));
            }
            ExprKind::Local(local)
                if self
                    .program
                    .field_bindings
                    .get(&local)
                    .is_some_and(|(_, mutable)| *mutable) => {}
            ExprKind::Global(_)
            | ExprKind::CoreFn(_)
            | ExprKind::JsGlobal(_)
            | ExprKind::Property { .. } => {}
            _ => {
                return Err(self.error(
                    node,
                    AnalysisErrorKind::InvalidForm("Can't set! local var or non-mutable field"),
                ));
            }
        }
        Ok(target)
    }

    fn set_form(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        if !(2..=3).contains(&args.len()) {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm(
                    "set! requires a target and value, or object, property and value",
                ),
            ));
        }
        if args.len() == 2
            && matches!(self.syntax.nodes[args[0].0].kind, NodeKind::Symbol)
            && self.spelling(args[0]) == "*unchecked-arrays*"
        {
            self.assignment_target(args[0])?;
            if let NodeKind::Boolean(value) = self.syntax.nodes[args[1].0].kind {
                self.unchecked_arrays = value;
            }
            // Non-REPL CLJS analyzes the RHS, then emits a no-op. In particular,
            // a computed boolean neither changes compiler state nor mutates the
            // runtime Var. Diagnostics and macro expansion still run on the RHS.
            self.non_tail_form(args[1], ExprContext::Value)?;
            return Ok(self.push(
                ExprKind::Nil,
                self.syntax.nodes[id.0].span.clone(),
                context,
            ));
        }
        let target = if args.len() == 3 {
            let target = self.dot_form(id, &args[..2], ExprContext::Value)?;
            if !matches!(self.program.exprs[target.0].kind, ExprKind::Property { .. }) {
                return Err(self.error(
                    args[1],
                    AnalysisErrorKind::InvalidForm("set! requires a property target"),
                ));
            }
            target
        } else {
            self.assignment_target(args[0])?
        };
        let value = self.non_tail_form(*args.last().unwrap(), ExprContext::Value)?;
        Ok(self.push(
            ExprKind::SetGlobal { target, value },
            self.syntax.nodes[id.0].span.clone(),
            context,
        ))
    }

    fn binding_form(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        let Some((&names, body)) = args.split_first() else {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm("binding requires a vector"),
            ));
        };
        let pairs = self.vector(names, "binding requires a vector")?.to_vec();
        if pairs.len() % 2 != 0 {
            return Err(self.error(
                names,
                AnalysisErrorKind::InvalidForm("binding requires an even number of forms"),
            ));
        }
        let previous = self.recur_target.take();
        let result = (|| {
            let mut bindings = Vec::new();
            for pair in pairs.as_chunks::<2>().0 {
                if !matches!(self.syntax.nodes[pair[0].0].kind, NodeKind::Symbol) {
                    return Err(self.error(
                        pair[0],
                        AnalysisErrorKind::InvalidForm("binding requires global symbols"),
                    ));
                }
                let target = self.assignment_target(pair[0])?;
                let init = self.non_tail_form(pair[1], ExprContext::Value)?;
                bindings.push((target, init));
            }
            let body = self.body(body, self.syntax.nodes[id.0].span.clone(), context)?;
            Ok(self.push(
                ExprKind::Binding { bindings, body },
                self.syntax.nodes[id.0].span.clone(),
                context,
            ))
        })();
        self.recur_target = previous;
        result
    }

    fn this_as(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        let Some((&name, body)) = args.split_first() else {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm("this-as requires a binding"),
            ));
        };
        let span = self.syntax.nodes[id.0].span.clone();
        self.env.scopes.push(Vec::new());
        let result = (|| {
            let local = self.bind(name)?;
            let body = self.body(
                body,
                span.clone(),
                if context == ExprContext::Value {
                    ExprContext::Return
                } else {
                    context
                },
            )?;
            Ok(self.push(
                ExprKind::ThisAs {
                    binding: local,
                    body,
                },
                span,
                context,
            ))
        })();
        self.env.scopes.pop();
        result
    }

    fn declare_global(&mut self, node: NodeId) -> Result<GlobalId, AnalysisError> {
        let name = self.binding_name(node)?.to_owned();
        self.macro_host
            .register_runtime_var(
                self.namespace,
                &name,
                self.metadata_is(node, "private", "true"),
            )
            .map_err(|e| self.error(node, AnalysisErrorKind::MacroExpansion(e)))?;
        let key = (self.namespace.to_owned(), name.clone());
        if let Some(&global) = self.globals.get(&key) {
            self.program.globals[global.0].declared = true;
            return Ok(global);
        }
        let global = GlobalId(self.program.globals.len());
        self.program.globals.push(GlobalBinding {
            type_descriptor: false,
            declared: true,
            constant: false,
            boolean_tag: false,
            metadata: None,
            namespace: self.namespace.into(),
            name,
            span: self.syntax.nodes[node.0].span.clone(),
            source_index: self.source_index,
        });
        self.globals.insert(key, global);
        Ok(global)
    }

    fn protocol_predicate_form(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        context: ExprContext,
        direct: bool,
    ) -> Result<ExprId, AnalysisError> {
        if args.len() != 2 || !matches!(self.syntax.nodes[args[0].0].kind, NodeKind::Symbol) {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm(
                    "protocol predicate requires a protocol symbol and value",
                ),
            ));
        }
        let spelling = self.spelling(args[0]).to_owned();
        let locals = std::mem::take(&mut self.env);
        let resolved = self.resolve(args[0], &spelling);
        self.env = locals;
        let protocol = self.non_tail_form(args[0], ExprContext::Value)?;
        let value = self.non_tail_form(args[1], ExprContext::Value)?;
        let kind = match resolved {
            Ok(ExprKind::CoreFn(_)) => {
                let callee = self.push(
                    ExprKind::CoreFn(if direct {
                        CoreFnId::Implements
                    } else {
                        CoreFnId::Satisfies
                    }),
                    self.syntax.nodes[id.0].span.clone(),
                    ExprContext::Value,
                );
                ExprKind::Call {
                    callee,
                    args: vec![protocol, value],
                }
            }
            resolved => {
                let key = if let Ok(ExprKind::Global(global)) = resolved {
                    let b = &self.program.globals[global.0];
                    format!("{}/{}", b.namespace, b.name)
                } else if let Some((ns, name)) = spelling.split_once('/') {
                    let ns = self
                        .aliases
                        .iter()
                        .find(|(alias, _)| alias == ns)
                        .map_or(ns, |(_, target)| target.as_str());
                    format!("{ns}/{name}")
                } else {
                    format!("{}/{spelling}", self.namespace)
                };
                ExprKind::ProtocolPredicate {
                    protocol,
                    value,
                    key,
                    direct,
                }
            }
        };
        Ok(self.push(kind, self.syntax.nodes[id.0].span.clone(), context))
    }

    fn protocol_metadata(&mut self, global: GlobalId, entries: Vec<ExprId>) {
        let metadata = self.program.globals[global.0].metadata.unwrap();
        let ExprKind::Map(existing) = &mut self.program.exprs[metadata.0].kind else {
            unreachable!()
        };
        existing.extend(entries);
    }

    // Source macro construction preserves the declaration's metadata, but its
    // dispatch and method bodies are expanded only when the bundle is analyzed.
    fn multimethod_declaration_stub(
        &mut self,
        expanded: NodeId,
        context: ExprContext,
        definition: bool,
    ) -> Result<ExprId, AnalysisError> {
        let NodeKind::Collection { children, .. } = self.syntax.nodes[expanded.0].kind.clone() else {
            return Err(self.error(expanded, AnalysisErrorKind::InvalidForm("invalid multimethod expansion")));
        };
        let target = children[1];
        if definition {
            let expr = self.definition(expanded, &[target], ExprContext::Discard, false)?;
            if let ExprKind::Def { once, .. } = &mut self.program.exprs[expr.0].kind {
                *once = true;
            }
            self.program.exprs[expr.0].context = context;
            Ok(expr)
        } else {
            let span = self.syntax.nodes[expanded.0].span.clone();
            let target = if matches!(self.syntax.nodes[target.0].kind, NodeKind::Symbol) {
                self.non_tail_form(target, ExprContext::Value)?
            } else {
                self.push(ExprKind::Nil, span.clone(), ExprContext::Value)
            };
            let callee = self.push(ExprKind::CoreFn(CoreFnId::AddMethod), span.clone(), ExprContext::Value);
            Ok(self.push(ExprKind::Call { callee, args: vec![target] }, span, context))
        }
    }

    /// Only supported source-level top-level declarations are replayable bundles.
    /// Nested declarations, reify and specify! keep ordinary expression semantics.
    fn catalog_bundle(&mut self, id: NodeId, expr: ExprId) -> Result<ExprId, AnalysisError> {
        if !self.top_level || !self.macro_host.defers_source_units() {
            return Ok(expr);
        }
        let Some(session) = self.macro_host.session_handle() else {
            return Ok(expr);
        };
        let mut products = Vec::new();
        let mut once = false;
        let mut extensions = Vec::new();
        let mut work = vec![expr];
        while let Some(value) = work.pop() {
            match &self.program.exprs[value.0].kind {
                ExprKind::Def { global, once: true, .. } => {
                    products.push(*global);
                    once = true;
                }
                ExprKind::Call { callee, args }
                    if matches!(self.program.exprs[callee.0].kind, ExprKind::CoreFn(CoreFnId::AddMethod)) =>
                {
                    let target = args.first().and_then(|target| {
                        if let ExprKind::Global(global) = self.program.exprs[target.0].kind {
                            let binding = &self.program.globals[global.0];
                            session.multifn_target(&format!("{}/{}", binding.namespace, binding.name))
                        } else {
                            None
                        }
                    });
                    // Unknown and computed targets follow the conservative
                    // null-selector policy used by computed extend-type.
                    extensions.push((
                        target.map_or_else(|| "multifn:*".into(), |name| format!("multifn:{name}")),
                        None,
                        format!("method@{}:{}", self.source_index, self.syntax.nodes[id.0].span.start),
                    ));
                }
                ExprKind::DefProtocol {
                    global, methods, ..
                } => {
                    products.push(*global);
                    products.extend(methods.iter().map(|m| m.global));
                }
                ExprKind::DefType {
                    global,
                    constructor,
                    map_constructor,
                    ..
                } => {
                    products.push(*global);
                    products.extend(*constructor);
                    products.extend(*map_constructor);
                }
                ExprKind::Do(forms) => work.extend(forms.iter().rev()),
                ExprKind::ExtendType {
                    instance,
                    target,
                    methods,
                    ..
                } => {
                    let selector = if *instance {
                        None
                    } else {
                        match &self.program.exprs[target.0].kind {
                            ExprKind::String(s) => {
                                Some(String::from_utf16_lossy(&self.program.strings[s.0]))
                            }
                            ExprKind::Global(g) if self.program.globals[g.0].type_descriptor => {
                                let b = &self.program.globals[g.0];
                                Some(format!("@{}/{}", b.namespace, b.name))
                            }
                            _ => None,
                        }
                    };
                    let identity = match &self.program.exprs[target.0].kind {
                        ExprKind::Global(g) => {
                            let b = &self.program.globals[g.0];
                            format!("{}/{}", b.namespace, b.name)
                        }
                        _ => selector.clone().unwrap_or_else(|| {
                            format!("target@{}", self.program.exprs[target.0].span.start)
                        }),
                    };
                    for (dispatch, method) in methods {
                        let key = match dispatch {
                            MethodDispatch::Protocol(k) | MethodDispatch::Builtin(k) => k,
                        };
                        let protocol = if method.params.is_empty() {
                            key.as_str()
                        } else {
                            key.rsplit_once('/').map_or(key.as_str(), |(p, _)| p)
                        };
                        let entry = (protocol.to_owned(), selector.clone(), identity.clone());
                        if !extensions.contains(&entry) {
                            extensions.push(entry);
                        }
                    }
                }
                _ => (),
            }
        }
        for g in &products {
            let b = &self.program.globals[g.0];
            self.macro_host
                .register_runtime_var(&b.namespace, &b.name, false)
                .map_err(|e| self.error(id, AnalysisErrorKind::MacroExpansion(e)))?;
        }
        let form = self
            .compiler_read_form(id)
            .map_err(|e| self.error(id, AnalysisErrorKind::MacroExpansion(e.to_string())))?;
        let loc = crate::compiler_data::DiagnosticLocation::from_span(&form.span());
        let unit = session
            .register_bundle(
                &self.program,
                &products,
                &extensions,
                once,
                form,
                self.definition_snapshot(),
                self.macro_host.macro_analysis_frame(),
                format!("{}:{}:{}", loc.source, loc.line, loc.column),
            )
            .map_err(|e| self.error(id, AnalysisErrorKind::MacroExpansion(e)))?;
        self.program.exprs[expr.0].kind = ExprKind::Nil;
        self.deferred.push((expr, unit));
        Ok(expr)
    }

    fn defprotocol_form(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        let Some((&name, specs)) = args.split_first() else {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm("defprotocol requires a name"),
            ));
        };
        let global = self.declare_global(name)?;
        self.program.globals[global.0].type_descriptor = false;
        self.definition(id, &[name], ExprContext::Discard, false)?;
        let protocol_name = format!("{}/{}", self.namespace, self.program.globals[global.0].name);
        let mut metadata = vec![
            self.data_source(":protocol-symbol")?,
            self.data_source("true")?,
        ];
        let mut methods = Vec::new();
        let mut sigs = Vec::new();
        let mut via_metadata = false;
        let mut specs = specs.iter();
        while let Some(&spec) = specs.next() {
            if matches!(self.syntax.nodes[spec.0].kind, NodeKind::String(_)) {
                metadata.extend([
                    self.data_source(":doc")?,
                    self.datum(spec, ExprContext::Value)?,
                ]);
                continue;
            }
            if matches!(self.syntax.nodes[spec.0].kind, NodeKind::Keyword { .. }) {
                let value = *specs.next().ok_or_else(|| {
                    self.error(
                        spec,
                        AnalysisErrorKind::InvalidForm("protocol option requires a value"),
                    )
                })?;
                if self.spelling(spec) == ":extend-via-metadata" {
                    via_metadata = !matches!(self.spelling(value), "nil" | "false");
                }
                metadata.extend([
                    self.datum(spec, ExprContext::Value)?,
                    self.datum(value, ExprContext::Value)?,
                ]);
                continue;
            }
            let NodeKind::Collection {
                kind: CollectionKind::List,
                children,
            } = self.syntax.nodes[spec.0].kind.clone()
            else {
                return Err(self.error(
                    spec,
                    AnalysisErrorKind::InvalidForm(
                        "protocol methods require a name and parameter vectors",
                    ),
                ));
            };
            let Some((&name, signatures)) = children.split_first() else {
                return Err(self.error(
                    spec,
                    AnalysisErrorKind::InvalidForm("empty protocol method"),
                ));
            };
            let method = self.declare_global(name)?;
            self.definition(spec, &[name], ExprContext::Discard, false)?;
            let mut arities = Vec::new();
            let mut variadic = None;
            let mut arglists = Vec::new();
            let mut doc = self.data_source("nil")?;
            for (i, &signature) in signatures.iter().enumerate() {
                if matches!(self.syntax.nodes[signature.0].kind, NodeKind::String(_))
                    && i + 1 == signatures.len()
                {
                    doc = self.datum(signature, ExprContext::Value)?;
                    continue;
                }
                let params =
                    self.vector(signature, "protocol signatures require parameter vectors")?;
                if let Some(index) = params.iter().position(|&p| self.spelling(p) == "&")
                    && (index == 0
                        || index + 2 != params.len()
                        || variadic.replace(index).is_some())
                {
                    return Err(self.error(
                        signature,
                        AnalysisErrorKind::InvalidForm("invalid variadic protocol signature"),
                    ));
                }
                if params.is_empty() {
                    return Err(self.error(
                        signature,
                        AnalysisErrorKind::InvalidForm("protocol methods require a receiver"),
                    ));
                }
                if arities.contains(&params.len()) {
                    return Err(self.error(
                        signature,
                        AnalysisErrorKind::InvalidForm("duplicate protocol method arity"),
                    ));
                }
                arities.push(params.len());
                arglists.push(self.datum(signature, ExprContext::Value)?);
            }
            if arities.is_empty() {
                return Err(self.error(
                    spec,
                    AnalysisErrorKind::InvalidForm("protocol method requires a signature"),
                ));
            }
            let span = self.syntax.nodes[spec.0].span.clone();
            let arglists = self.push(ExprKind::List(arglists), span.clone(), ExprContext::Value);
            let method_metadata = vec![
                self.data_source(":protocol")?,
                self.data_source(&protocol_name)?,
                self.data_source(":doc")?,
                doc,
                self.data_source(":arglists")?,
                arglists,
            ];
            self.protocol_metadata(method, method_metadata);
            let method_name = self.program.globals[method.0].name.clone();
            sigs.push(self.data_source(&format!(":{method_name}"))?);
            let entries = vec![
                self.data_source(":name")?,
                self.datum(name, ExprContext::Value)?,
                self.data_source(":arglists")?,
                arglists,
                self.data_source(":doc")?,
                doc,
            ];
            sigs.push(self.push(ExprKind::Map(entries), span, ExprContext::Value));
            methods.retain(|m: &ProtocolMethod| m.global != method);
            methods.push(ProtocolMethod {
                global: method,
                key: format!("{protocol_name}/{method_name}"),
                arities,
                variadic,
            });
        }
        metadata.push(self.data_source(":sigs")?);
        metadata.push(self.push(
            ExprKind::Map(sigs),
            self.syntax.nodes[id.0].span.clone(),
            ExprContext::Value,
        ));
        self.protocol_metadata(global, metadata);
        self.program.protocols.insert(global, methods.clone());
        Ok(self.push(
            ExprKind::DefProtocol {
                global,
                methods,
                via_metadata,
            },
            self.syntax.nodes[id.0].span.clone(),
            context,
        ))
    }

    fn extend_type_form(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        let Some((&target, methods)) = args.split_first() else {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm("extend-type requires a type"),
            ));
        };
        let target = if matches!(self.syntax.nodes[target.0].kind, NodeKind::Metadata { .. }) {
            self.expand_reader(target)?
        } else {
            target
        };
        if !matches!(
            self.syntax.nodes[target.0].kind,
            NodeKind::Symbol | NodeKind::Nil
        ) {
            return Err(self.error(
                target,
                AnalysisErrorKind::InvalidForm("extension target must be a type symbol or nil"),
            ));
        }
        let instance = self.metadata_entry(target, "extend").is_some_and(|v| matches!(v.data().inspect(), DatumView::Keyword(name) if name.name() == "instance".encode_utf16().collect::<Vec<_>>()));
        let name = self.spelling(target).to_owned();
        let native = !instance
            && matches!(
                name.as_str(),
                "nil"
                    | "default"
                    | "object"
                    | "string"
                    | "symbol"
                    | "bigint"
                    | "number"
                    | "boolean"
                    | "array"
                    | "function"
            );
        let target = if native {
            let native = match name.as_str() {
                "nil" => "null",
                "default" => "_",
                n => n,
            };
            let text = self.own_string(native.encode_utf16().collect());
            self.push(
                ExprKind::String(text),
                self.syntax.nodes[target.0].span.clone(),
                ExprContext::Value,
            )
        } else {
            self.non_tail_form(target, ExprContext::Value)?
        };
        let fields = self.type_fields.clone().unwrap_or_default();
        let methods = self.object_methods(id, methods, &fields, false, native)?;
        Ok(self.push(
            ExprKind::ExtendType {
                instance,
                return_target: false,
                target,
                methods,
            },
            self.syntax.nodes[id.0].span.clone(),
            context,
        ))
    }

    fn specify_form(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        context: ExprContext,
        copy: bool,
    ) -> Result<ExprId, AnalysisError> {
        let Some((&target, methods)) = args.split_first() else {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm("specify requires a value"),
            ));
        };
        let mut target = self.non_tail_form(target, ExprContext::Value)?;
        if copy {
            let callee = self.push(
                ExprKind::CoreFn(CoreFnId::Clone),
                self.syntax.nodes[id.0].span.clone(),
                ExprContext::Value,
            );
            target = self.push(
                ExprKind::Call {
                    callee,
                    args: vec![target],
                },
                self.syntax.nodes[id.0].span.clone(),
                ExprContext::Value,
            );
        }
        let methods = self.object_methods(id, methods, &[], false, false)?;
        Ok(self.push(
            ExprKind::ExtendType {
                instance: true,
                return_target: true,
                target,
                methods,
            },
            self.syntax.nodes[id.0].span.clone(),
            context,
        ))
    }

    fn extend_protocol_form(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        let Some((&protocol, rest)) = args.split_first() else {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm("extend-protocol requires a protocol"),
            ));
        };
        let mut specifications: Vec<Vec<NodeId>> = Vec::new();
        let mut i = 0;
        while i < rest.len() {
            let target = rest[i];
            i += 1;
            let mut args = vec![target, protocol];
            while i < rest.len()
                && matches!(
                    self.syntax.nodes[rest[i].0].kind,
                    NodeKind::Collection {
                        kind: CollectionKind::List,
                        ..
                    }
                )
            {
                args.push(rest[i]);
                i += 1;
            }
            if let Some(previous) = specifications
                .iter_mut()
                .find(|previous| self.spelling(previous[0]) == self.spelling(target))
            {
                *previous = args;
            } else {
                specifications.push(args);
            }
        }
        let forms = specifications
            .iter()
            .map(|args| self.extend_type_form(id, args, ExprContext::Discard))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(self.push(
            ExprKind::Do(forms),
            self.syntax.nodes[id.0].span.clone(),
            context,
        ))
    }

    fn assert_form(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        if !(1..=2).contains(&args.len()) {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm("assert requires a condition and optional message"),
            ));
        }
        if !self.macro_host.assertions_enabled() {
            return Ok(self.push(ExprKind::Nil, self.syntax.nodes[id.0].span.clone(), context));
        }
        // Print the actual semantic form, including generated pre/post conditions.
        let node = if self
            .generated
            .get(args[0].0)
            .and_then(Option::as_ref)
            .is_some()
        {
            args[0]
        } else {
            self.expand_reader(args[0])?
        };
        let data = self.generated[node.0].as_ref().unwrap().data();
        let text = crate::compiler_data::diagnostic_text(&data);
        let form = self.own_string(text.to_vec());
        let test = self.non_tail_form(node, ExprContext::Value)?;
        let message = args
            .get(1)
            .map(|&arg| self.non_tail_form(arg, ExprContext::Value))
            .transpose()?;
        Ok(self.push(
            ExprKind::Assert {
                test,
                message,
                form,
            },
            self.syntax.nodes[id.0].span.clone(),
            context,
        ))
    }

    /// Feed method parameters through the same pure fn transform as ordinary
    /// functions, then analyze with the original protocol/IFn recurrence ABI.
    fn source_method(
        &mut self,
        id: NodeId,
        _method: &[NodeId],
        kind: MethodKind,
        fields: &[(String, bool)],
    ) -> Result<FnMethod, AnalysisError> {
        let original = if self.generated.get(id.0).and_then(Option::as_ref).is_some() {
            id
        } else {
            self.expand_reader(id)?
        };
        let type_method = self
            .metadata_entry(original, "cljs.analyzer/type")
            .is_some();
        let original = self.generated[original.0].as_ref().unwrap().clone();
        let builder = crate::compiler_data::DataBuilder;
        let mut values = original
            .children()
            .iter()
            .map(Form::data)
            .collect::<Vec<_>>();
        if values
            .first()
            .is_some_and(|d| matches!(d.inspect(), DatumView::Symbol(_)))
        {
            values.remove(0);
        }
        if self.type_fields.is_some() && values.len() > 1 {
            values[1] = builder
                .list(&[
                    builder.symbol(crate::compiler_data::Name::new(None, "do")),
                    values[1].clone(),
                ])
                .unwrap();
        }
        values.insert(
            0,
            builder.symbol(crate::compiler_data::Name::new(Some("cljs.core"), "fn")),
        );
        let form = Form::from_expansion(
            builder.list(&values).unwrap(),
            &original,
            crate::compiler_data::Name::new(Some("cljs.core"), "fn"),
        );
        let call = self.import_form(&form)?;
        let NodeKind::Collection { children, .. } = self.syntax.nodes[call.0].kind.clone() else {
            unreachable!()
        };
        let expanded = self
            .expand_macro(call, children[0], ExprContext::Value)?
            .unwrap();
        let NodeKind::Collection { children, .. } = self.syntax.nodes[expanded.0].kind.clone()
        else {
            unreachable!()
        };
        let NodeKind::Collection { children, .. } = self.syntax.nodes[children[1].0].kind.clone()
        else {
            unreachable!()
        };
        let method = self.fn_method_with_fields(id, &children, kind, fields)?;
        if type_method {
            self.program.type_methods.insert(method.target);
        }
        Ok(method)
    }

    fn object_methods(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        fields: &[(String, bool)],
        strict: bool,
        native: bool,
    ) -> Result<Vec<(MethodDispatch, FnMethod)>, AnalysisError> {
        let mut protocol = None;
        let mut methods = Vec::new();
        for &arg in args {
            let arg = if matches!(self.syntax.nodes[arg.0].kind, NodeKind::Metadata { .. }) {
                self.expand_reader(arg)?
            } else {
                arg
            };
            if matches!(self.syntax.nodes[arg.0].kind, NodeKind::Symbol) {
                let name = self.spelling(arg).to_owned();
                let builtin = match name.split_once('/') {
                    Some((qualifier, local))
                        if matches!(qualifier, "cljs.core" | "clojure.core")
                            || self.aliases.iter().any(|(alias, target)| {
                                alias == qualifier && target == "cljs.core"
                            }) =>
                    {
                        local
                    }
                    _ => &name,
                };
                if matches!(
                    builtin,
                    "IEncodeJS"
                        | "IEncodeClojure"
                        | "IFn"
                        | "Fn"
                        | "IList"
                        | "IPending"
                        | "IDeref"
                        | "IAtom"
                        | "IReset"
                        | "ISwap"
                        | "IVolatile"
                        | "IWatchable"
                        | "IMeta"
                        | "IWithMeta"
                        | "IMapEntry"
                        | "IFind"
                        | "ISorted"
                        | "IChunkedSeq"
                        | "IChunk"
                        | "IChunkedNext"
                        | "IDrop"
                        | "IRecord"
                        | "ICloneable"
                        | "IPrintWithWriter"
                        | "IWriter"
                        | "ISeqable"
                        | "IIndexed"
                        | "ILookup"
                        | "ICounted"
                        | "ISeq"
                        | "INext"
                        | "ISequential"
                        | "IAssociative"
                        | "IReduce"
                        | "IKVReduce"
                        | "IVector"
                        | "IMap"
                        | "ICollection"
                        | "IEmptyableCollection"
                        | "ISet"
                        | "IStack"
                        | "IEditableCollection"
                        | "ITransientCollection"
                        | "ITransientAssociative"
                        | "ITransientMap"
                        | "ITransientVector"
                        | "ITransientSet"
                        | "IMultiFn"

                        | "IReversible"
                        | "IComparable"
                        | "IIterable"
                        | "IHash"
                        | "IEquiv"
                        | "INamed"
                        | "Object"
                ) {
                    protocol = Some(Err(builtin.to_owned()));
                } else {
                    let ExprKind::Global(global) = self.resolve(arg, &name)? else {
                        return Err(self.error(
                            arg,
                            AnalysisErrorKind::InvalidForm("method requires a declared protocol"),
                        ));
                    };
                    if !self.program.protocols.contains_key(&global) {
                        return Err(self.error(
                            arg,
                            AnalysisErrorKind::InvalidForm("method requires a declared protocol"),
                        ));
                    }
                    protocol = Some(Ok(global));
                }
                let key = match protocol.as_ref().unwrap() {
                    Ok(global) => {
                        let b = &self.program.globals[global.0];
                        format!("{}/{}", b.namespace, b.name)
                    }
                    Err(name) => name.clone(),
                };
                if !strict {
                    methods.retain(|(dispatch, _): &(MethodDispatch, FnMethod)| {
                        let existing = match dispatch {
                            MethodDispatch::Builtin(name) | MethodDispatch::Protocol(name) => name,
                        };
                        existing != &key && !existing.starts_with(&format!("{key}/"))
                    });
                }
                let body = self.push(
                    ExprKind::Bool(true),
                    self.syntax.nodes[arg.0].span.clone(),
                    ExprContext::Return,
                );
                let target = self.new_recur_target(Vec::new(), false);
                methods.push((
                    MethodDispatch::Builtin(key),
                    FnMethod {
                        params: Vec::new(),
                        rest: None,
                        body,
                        target,
                        captures: Vec::new(),
                    },
                ));
                continue;
            }
            if self.top_level && self.macro_host.defers_source_units() {
                continue;
            }
            let NodeKind::Collection {
                kind: CollectionKind::List,
                children,
            } = self.syntax.nodes[arg.0].kind.clone()
            else {
                return Err(
                    self.error(arg, AnalysisErrorKind::InvalidForm("methods must be lists"))
                );
            };
            let Some((&name, declaration)) = children.split_first() else {
                return Err(self.error(arg, AnalysisErrorKind::InvalidForm("empty method")));
            };
            if !matches!(self.syntax.nodes[name.0].kind, NodeKind::Symbol) {
                return Err(self.error(
                    name,
                    AnalysisErrorKind::InvalidForm("method name must be a symbol"),
                ));
            }
            if self.type_fields.is_some()
                && !self.raw_type
                && declaration.first().is_some_and(|node| {
                    matches!(
                        self.syntax.nodes[node.0].kind,
                        NodeKind::Collection {
                            kind: CollectionKind::List,
                            ..
                        }
                    )
                })
            {
                return Err(self.error(
                    arg,
                    AnalysisErrorKind::InvalidForm("type overloads must use separate method forms"),
                ));
            }
            let spelling = if self.spelling(name) == "/" {
                "/"
            } else {
                self.spelling(name).rsplit('/').next().unwrap()
            }
            .to_owned();
            let Some(protocol) = protocol.as_ref() else {
                return Err(self.error(
                    id,
                    AnalysisErrorKind::InvalidForm("a protocol must precede its methods"),
                ));
            };
            let (dispatch, kind, arities) = match protocol {
                Ok(protocol) => {
                    let method = self.program.protocols[protocol]
                        .iter()
                        .find(|m| self.program.globals[m.global.0].name == spelling)
                        .ok_or_else(|| {
                            self.error(
                                name,
                                AnalysisErrorKind::InvalidForm(
                                    "method is not declared by this protocol",
                                ),
                            )
                        })?;
                    (
                        MethodDispatch::Protocol(method.key.clone()),
                        MethodKind::Protocol,
                        method.arities.clone(),
                    )
                }
                Err(protocol) => {
                    let arities = match (protocol.as_str(), spelling.as_str()) {
                        ("IMultiFn", "-reset" | "-methods" | "-prefers" | "-default-dispatch-val" | "-dispatch-fn") => vec![1],
                        ("IMultiFn", "-remove-method" | "-get-method") => vec![2],
                        ("IMultiFn", "-add-method" | "-prefer-method") => vec![3],
                        ("IEncodeJS", "-clj->js" | "-key->js") => vec![1],
                        ("IEncodeClojure", "-js->clj") => vec![2],
                        ("IFn", "-invoke") => (1..=22).collect(),
                        ("IReduce", "-reduce") => vec![2, 3],
                        ("IEditableCollection", "-as-transient")
                        | ("ITransientCollection", "-persistent!")
                        | ("ITransientVector", "-pop!") => vec![1],
                        ("ITransientCollection", "-conj!")
                        | ("ITransientMap", "-dissoc!")
                        | ("ITransientSet", "-disjoin!") => vec![2],
                        ("ITransientAssociative", "-assoc!")
                        | ("ITransientVector", "-assoc-n!") => vec![3],
                        ("IMapEntry", "-key" | "-val")
                        | ("IChunkedSeq", "-chunked-first" | "-chunked-rest")
                        | ("IChunkedNext", "-chunked-next")
                        | ("IChunk", "-drop-first")
                        | ("ISorted", "-comparator") => vec![1],
                        ("IDrop", "-drop")
                        | ("IFind", "-find")
                        | ("ISorted", "-sorted-seq" | "-entry-key") => vec![2],
                        ("ISorted", "-sorted-seq-from") => vec![3],
                        ("IKVReduce", "-kv-reduce")
                        | ("IAssociative", "-assoc")
                        | ("IVector", "-assoc-n") => vec![3],
                        ("ICollection", "-conj")
                        | ("IMap", "-dissoc")
                        | ("ISet", "-disjoin")
                        | ("IComparable", "-compare") => vec![2],
                        ("ICloneable", "-clone")
                        | ("IEmptyableCollection", "-empty")
                        | ("IStack", "-peek" | "-pop")
                        | ("IReversible", "-rseq")
                        | ("IIterable", "-iterator")
                        | ("IHash", "-hash")
                        | ("INamed", "-name" | "-namespace")
                        | ("IPending", "-realized?")
                        | ("IDeref", "-deref")
                        | ("IMeta", "-meta")
                        | ("IWriter", "-flush")
                        | ("ISeqable", "-seq")
                        | ("ICounted", "-count")
                        | ("ISeq", "-first" | "-rest")
                        | ("INext", "-next") => vec![1],
                        ("IWriter", "-write")
                        | ("IEquiv", "-equiv")
                        | ("IAssociative", "-contains-key?")
                        | ("IReset", "-reset!")
                        | ("IVolatile", "-vreset!")
                        | ("IWatchable", "-remove-watch")
                        | ("IWithMeta", "-with-meta") => vec![2],
                        ("IWatchable", "-add-watch" | "-notify-watches")
                        | ("IPrintWithWriter", "-pr-writer") => vec![3],
                        ("ISwap", "-swap!") => vec![2, 3, 4, 5],
                        ("IIndexed", "-nth") | ("ILookup", "-lookup") => vec![2, 3],
                        ("Object", _) => (1..=22).collect(),
                        _ => {
                            return Err(self.error(
                                name,
                                AnalysisErrorKind::InvalidForm(
                                    "method is not declared by this protocol",
                                ),
                            ));
                        }
                    };
                    (
                        MethodDispatch::Builtin(format!("{protocol}/{spelling}")),
                        if protocol == "IFn" {
                            MethodKind::IFn
                        } else {
                            MethodKind::Protocol
                        },
                        arities,
                    )
                }
            };
            if !strict
                && !native
                && matches!(&dispatch, MethodDispatch::Builtin(name) if name == "IFn/-invoke")
                && declaration.first().is_some_and(|n| {
                    matches!(
                        self.syntax.nodes[n.0].kind,
                        NodeKind::Collection {
                            kind: CollectionKind::Vector,
                            ..
                        }
                    )
                })
            {
                return Err(self.error(
                    arg,
                    AnalysisErrorKind::InvalidForm("IFn extension signatures must be grouped"),
                ));
            }
            if native {
                methods.retain(|(key, _)| key != &dispatch);
            }
            let declarations = if declaration.first().is_some_and(|n| {
                matches!(
                    self.syntax.nodes[n.0].kind,
                    NodeKind::Collection {
                        kind: CollectionKind::List,
                        ..
                    }
                )
            }) {
                declaration
                    .iter()
                    .map(|&node| {
                        let NodeKind::Collection { children, .. } =
                            self.syntax.nodes[node.0].kind.clone()
                        else {
                            unreachable!()
                        };
                        (node, children)
                    })
                    .collect::<Vec<_>>()
            } else {
                vec![(arg, declaration.to_vec())]
            };
            for (arg, declaration) in declarations {
                let parsed = self.source_method(
                    arg,
                    &declaration,
                    if native || (!strict && !self.raw_type) {
                        MethodKind::Function
                    } else {
                        kind
                    },
                    fields,
                )?;
                if !native && !strict {
                    self.program.recur_targets[parsed.target.0].receiver_from_this =
                        kind != MethodKind::IFn;
                    if !self.raw_type
                        && matches!(&dispatch, MethodDispatch::Builtin(name) if name == "IFn/-invoke")
                        && self.program.recur_targets[parsed.target.0].used
                    {
                        return Err(self.error(arg, AnalysisErrorKind::InvalidForm("IFn extension recur arities differ between call and invoke adapters")));
                    }
                }
                let slot_arity = parsed.params.len() + if parsed.rest.is_some() { 2 } else { 0 };
                if parsed.params.is_empty()
                    || (strict
                        && !matches!(&dispatch, MethodDispatch::Builtin(name) if name.starts_with("Object/"))
                        && !arities.contains(&slot_arity))
                {
                    return Err(self.error(
                        arg,
                        AnalysisErrorKind::InvalidForm("protocol method arity mismatch"),
                    ));
                }
                if strict
                    && methods
                        .iter()
                        .any(|(key, method): &(MethodDispatch, FnMethod)| {
                            key == &dispatch
                                && method.params.len() + if method.rest.is_some() { 2 } else { 0 }
                                    == slot_arity
                        })
                {
                    return Err(self.error(
                        arg,
                        AnalysisErrorKind::InvalidForm("duplicate protocol method arity"),
                    ));
                }
                methods.push((dispatch.clone(), parsed));
            }
        }
        Ok(methods)
    }

    fn type_form(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        context: ExprContext,
        record: bool,
    ) -> Result<ExprId, AnalysisError> {
        if args.len() < 2 {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm("type requires a name and field vector"),
            ));
        }
        let global = self.declare_global(args[0])?;
        self.program.globals[global.0].type_descriptor = true;
        if record {
            self.program.record_constructors.insert(global);
        } else {
            self.program.record_constructors.remove(&global);
        }
        let fields = self.type_field_names(args[1])?;
        if fields
            .iter()
            .enumerate()
            .any(|(i, (name, _))| fields[..i].iter().any(|(previous, _)| previous == name))
        {
            return Err(self.error(
                args[1],
                AnalysisErrorKind::InvalidForm("duplicate type field"),
            ));
        }
        let name = self.program.globals[global.0].name.clone();
        let mut declare = |prefix: &str| {
            let name = format!("{prefix}{name}");
            let key = (self.namespace.to_owned(), name.clone());
            if let Some(&global) = self.globals.get(&key) {
                return global;
            }
            let global = GlobalId(self.program.globals.len());
            self.program.globals.push(GlobalBinding {
                type_descriptor: false,
                declared: true,
                constant: false,
                boolean_tag: false,
                metadata: None,
                namespace: self.namespace.into(),
                name,
                span: self.syntax.nodes[args[0].0].span.clone(),
                source_index: self.source_index,
            });
            self.globals.insert(key, global);
            global
        };
        let constructor = declare("->");
        let map_constructor = record.then(|| declare("map->"));
        for (factory, prefix, suffix) in
            std::iter::once((constructor, "Positional factory function for ", ".")).chain(
                map_constructor.map(|factory| {
                    (
                        factory,
                        "Factory function for ",
                        ", taking a map of keywords to field values.",
                    )
                }),
            )
        {
            let doc = format!("{prefix}{}/{name}{suffix}", self.namespace);
            let metadata = self.data_source(&format!(
                "{{:doc {doc:?} :arglists ([{}])}}",
                fields
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            ))?;
            self.program.globals[factory.0].metadata = Some(metadata);
        }
        let previous_env = std::mem::take(&mut self.env);
        let mut method_fields = fields.clone();
        if record {
            method_fields.extend([
                ("__meta".into(), false),
                ("__extmap".into(), false),
                ("__hash".into(), true),
            ]);
        }
        let previous_fields = self.type_fields.replace(method_fields.clone());
        let previous_raw = std::mem::replace(&mut self.raw_type, false);
        let methods = self.object_methods(id, &args[2..], &method_fields, true, false);
        self.raw_type = previous_raw;
        self.env = previous_env;
        self.type_fields = previous_fields;
        let methods = methods?;
        Ok(self.push(
            ExprKind::DefType {
                record,
                global,
                constructor: Some(constructor),
                body: None,
                masks: None,
                map_constructor,
                fields: fields.into_iter().map(|(name, _)| name).collect(),
                methods,
            },
            self.syntax.nodes[id.0].span.clone(),
            context,
        ))
    }

    fn type_field_names(&mut self, node: NodeId) -> Result<Vec<(String, bool)>, AnalysisError> {
        let node = if self
            .generated
            .get(node.0)
            .and_then(Option::as_ref)
            .is_some()
        {
            node
        } else {
            self.expand_reader(node)?
        };
        self.vector(node, "type fields must be a vector")?
            .iter()
            .map(|&field| {
                let name = self.binding_name(field)?.to_owned();
                let mutable = ["mutable", "unsynchronized-mutable", "volatile-mutable"]
                    .iter()
                    .any(|key| {
                        self.metadata_entry(field, key).is_some_and(|value| {
                            !matches!(
                                value.data().inspect(),
                                DatumView::Nil | DatumView::Boolean(false)
                            )
                        })
                    });
                Ok((name, mutable))
            })
            .collect()
    }

    fn raw_type_form(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        context: ExprContext,
        record: bool,
    ) -> Result<ExprId, AnalysisError> {
        if !(3..=4).contains(&args.len()) {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm(
                    "deftype* requires a name, fields, protocol masks and optional body",
                ),
            ));
        }
        let global = self.declare_global(args[0])?;
        self.program.globals[global.0].type_descriptor = true;
        let masks = self.datum(args[2], ExprContext::Value)?;
        if record {
            self.program.record_constructors.insert(global);
        } else {
            self.program.record_constructors.remove(&global);
        }
        let fields = self.type_field_names(args[1])?;
        if fields
            .iter()
            .enumerate()
            .any(|(i, (name, _))| fields[..i].iter().any(|(previous, _)| previous == name))
        {
            return Err(self.error(
                args[1],
                AnalysisErrorKind::InvalidForm("duplicate type field"),
            ));
        }
        let previous_env = std::mem::take(&mut self.env);
        let mut method_fields = fields.clone();
        if record {
            method_fields.extend([
                ("__meta".into(), false),
                ("__extmap".into(), false),
                ("__hash".into(), true),
            ]);
        }
        let previous_fields = self.type_fields.replace(method_fields.clone());
        let previous_raw = std::mem::replace(&mut self.raw_type, true);
        let body = self.body(
            &args[3..],
            self.syntax.nodes[id.0].span.clone(),
            ExprContext::Value,
        );
        self.raw_type = previous_raw;
        self.env = previous_env;
        self.type_fields = previous_fields;
        let body = body?;
        Ok(self.push(
            ExprKind::DefType {
                record,
                global,
                constructor: None,
                map_constructor: None,
                body: Some(body),
                masks: Some(masks),
                fields: fields.into_iter().map(|(name, _)| name).collect(),
                methods: Vec::new(),
            },
            self.syntax.nodes[id.0].span.clone(),
            context,
        ))
    }

    fn reify_form(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        // Upstream reify captures the visible lexical environment as fields of
        // an anonymous type, including locals unused by its methods.
        let mut captures = Vec::new();
        let mut names = std::collections::HashSet::new();
        for &local in self.env.scopes.iter().flatten().rev() {
            if names.insert(self.program.locals[local.0].name.clone()) {
                captures.push(local);
            }
        }
        captures.reverse();
        for &arg in args {
            if let NodeKind::Collection {
                kind: CollectionKind::List,
                children,
            } = &self.syntax.nodes[arg.0].kind
                && children.get(1).is_some_and(|node| {
                    matches!(
                        self.syntax.nodes[node.0].kind,
                        NodeKind::Collection {
                            kind: CollectionKind::List,
                            ..
                        }
                    )
                })
            {
                return Err(self.error(
                    arg,
                    AnalysisErrorKind::InvalidForm(
                        "reify overloads must use separate method forms",
                    ),
                ));
            }
        }
        let methods = self.object_methods(id, args, &[], true, false)?;
        Ok(self.push(
            ExprKind::Reify {
                metadata: None,
                captures,
                methods,
            },
            self.syntax.nodes[id.0].span.clone(),
            context,
        ))
    }

    fn dotted_fields(
        &mut self,
        id: NodeId,
        root: ExprKind,
        fields: &str,
    ) -> Result<ExprKind, AnalysisError> {
        let mut kind = root;
        for field in fields.split('.') {
            if field.is_empty() {
                return Err(self.error(
                    id,
                    AnalysisErrorKind::InvalidForm("empty JavaScript property"),
                ));
            }
            let receiver = self.push(
                kind,
                self.syntax.nodes[id.0].span.clone(),
                ExprContext::Value,
            );
            kind = ExprKind::Property {
                receiver,
                property: munge_js_member(field),
            };
        }
        Ok(kind)
    }

    fn record_constructor_args(
        &mut self,
        node: NodeId,
        callee: ExprId,
        mut args: Vec<ExprId>,
    ) -> Vec<ExprId> {
        if let ExprKind::Global(global) = self.program.exprs[callee.0].kind
            && self.program.record_constructors.contains(&global)
            && self.metadata_entry(node, "internal-ctor").is_none()
        {
            if let Some(fields) = self.program.exprs.iter().find_map(|expr| match &expr.kind {
                ExprKind::DefType {
                    global: target,
                    fields,
                    ..
                } if *target == global => Some(fields.len()),
                _ => None,
            }) && args.len() != fields
                && args.len() != fields + 2
            {
                self.warn(
                    node,
                    "fn-arity",
                    format!(
                        "Wrong number of args ({}) passed to {}",
                        args.len(),
                        self.program.globals[global.0].name
                    ),
                );
            }
            for _ in 0..3 {
                args.push(self.push(
                    ExprKind::Nil,
                    self.syntax.nodes[node.0].span.clone(),
                    ExprContext::Value,
                ));
            }
        }
        args
    }

    fn new_form(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        let Some((&callee, args)) = args.split_first() else {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm("new requires a constructor"),
            ));
        };
        let constructor_node = callee;
        let callee = self.non_tail_form(callee, ExprContext::Value)?;
        let args = args
            .iter()
            .map(|&arg| self.non_tail_form(arg, ExprContext::Value))
            .collect::<Result<_, _>>()?;
        let args = self.record_constructor_args(constructor_node, callee, args);
        Ok(self.push(
            ExprKind::Construct { callee, args },
            self.syntax.nodes[id.0].span.clone(),
            context,
        ))
    }

    fn dot_form(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        if args.len() < 2 {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm("dot requires a receiver and member"),
            ));
        }
        let mut operands = vec![args[0]];
        let member = match self.syntax.nodes[args[1].0].kind.clone() {
            NodeKind::Symbol => {
                operands.extend_from_slice(&args[2..]);
                args[1]
            }
            NodeKind::Collection {
                kind: CollectionKind::List,
                children,
            } if args.len() == 2 && !children.is_empty() => {
                operands.extend_from_slice(&children[1..]);
                children[0]
            }
            _ => {
                return Err(self.error(
                    args[1],
                    AnalysisErrorKind::InvalidForm(
                        "dot member must be a symbol or method call list",
                    ),
                ));
            }
        };
        if !matches!(self.syntax.nodes[member.0].kind, NodeKind::Symbol) {
            return Err(self.error(
                member,
                AnalysisErrorKind::InvalidForm("dot member must be a symbol"),
            ));
        }
        let member = format!(".{}", self.spelling(member).rsplit('/').next().unwrap());
        self.interop(id, &member, &operands, context)
    }

    fn js_template(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        let Some((&template, operands)) = args.split_first() else {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm("js* requires a string template"),
            ));
        };
        if !matches!(self.syntax.nodes[template.0].kind, NodeKind::String(_)) {
            return Err(self.error(
                template,
                AnalysisErrorKind::InvalidForm("js* requires a string template"),
            ));
        }
        let template_expr = self.non_tail_form(template, ExprContext::Value)?;
        let ExprKind::String(string) = self.program.exprs[template_expr.0].kind else {
            unreachable!()
        };
        let source = String::from_utf16(&self.program.strings[string.0]).map_err(|_| {
            self.error(
                template,
                AnalysisErrorKind::InvalidForm("js* template contains an unpaired surrogate"),
            )
        })?;
        let mut segments = Vec::new();
        let mut placeholders = Vec::new();
        let mut tail = source.as_str();
        while let Some(start) = tail.find("~{") {
            let end = tail[start + 2..].find('}').ok_or_else(|| {
                self.error(
                    template,
                    AnalysisErrorKind::InvalidForm("unterminated js* placeholder"),
                )
            })? + start
                + 2;
            segments.push(tail[..start].to_owned());
            placeholders.push(tail[start + 2..end].to_owned());
            tail = &tail[end + 1..];
        }
        segments.push(tail.to_owned());
        let args = if operands.is_empty() {
            let mut args = Vec::new();
            for name in placeholders {
                if name.is_empty() {
                    return Err(self.error(
                        template,
                        AnalysisErrorKind::InvalidForm(
                            "js* positional placeholder requires an operand",
                        ),
                    ));
                }
                let kind = self.resolve(template, &name)?;
                args.push(self.push(
                    kind,
                    self.syntax.nodes[template.0].span.clone(),
                    ExprContext::Value,
                ));
            }
            args
        } else {
            if placeholders.len() != operands.len() {
                return Err(self.error(
                    id,
                    AnalysisErrorKind::InvalidForm("js* placeholder count does not match operands"),
                ));
            }
            operands
                .iter()
                .map(|&arg| self.non_tail_form(arg, ExprContext::Value))
                .collect::<Result<_, _>>()?
        };
        Ok(self.push(
            ExprKind::JsTemplate { segments, args },
            self.syntax.nodes[id.0].span.clone(),
            context,
        ))
    }

    fn interop(
        &mut self,
        id: NodeId,
        operator: &str,
        args: &[NodeId],
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        let (property, field) = operator
            .strip_prefix(".-")
            .filter(|_| !operator.contains('/'))
            .map(|name| (name, true))
            .unwrap_or((&operator[1..], false));
        if property.is_empty() {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm("JavaScript member name must not be empty"),
            ));
        }
        let mut path = munge_js_path(property);
        let property = path.pop().expect("nonempty member");
        if path.iter().any(|part| !js_identifier(part)) || property.is_empty() {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm("JavaScript member name must not be empty"),
            ));
        }
        if args.is_empty() || (field && args.len() != 1) {
            return Err(self.error(id, AnalysisErrorKind::InvalidForm("property reads require one receiver; method calls require a receiver followed by arguments")));
        }
        if matches!(self.syntax.nodes[args[0].0].kind, NodeKind::Nil) {
            return Err(self.error(
                args[0],
                AnalysisErrorKind::InvalidForm(
                    "JavaScript member access cannot target literal nil",
                ),
            ));
        }
        let mut receiver = self.non_tail_form(args[0], ExprContext::Value)?;
        for property in path {
            receiver = self.push(
                ExprKind::Property { receiver, property },
                self.syntax.nodes[id.0].span.clone(),
                ExprContext::Value,
            );
        }
        let kind = if field {
            ExprKind::Property { receiver, property }
        } else {
            let args = args[1..]
                .iter()
                .map(|&arg| self.non_tail_form(arg, ExprContext::Value))
                .collect::<Result<_, _>>()?;
            ExprKind::MethodCall {
                receiver,
                method: property,
                args,
            }
        };
        Ok(self.push(kind, self.syntax.nodes[id.0].span.clone(), context))
    }

    fn expand_macro(
        &mut self,
        call: NodeId,
        operator: NodeId,
        context: ExprContext,
    ) -> Result<Option<NodeId>, AnalysisError> {
        if !matches!(self.syntax.nodes[operator.0].kind, NodeKind::Symbol) {
            return Ok(None);
        }
        let name = self.spelling(operator).to_owned();
        if self.lookup(&name).is_some() {
            return Ok(None);
        }
        let call_span = self.syntax.nodes[call.0].span.clone();
        let error = |message| AnalysisError {
            kind: AnalysisErrorKind::MacroExpansion(format!("while expanding {name}: {message}")),
            span: call_span.clone(),
        };
        let core_alias = if let Some((qualifier, member)) = name.split_once('/') {
            self.aliases
                .iter()
                .find(|(alias, target)| alias == qualifier && target == "cljs.core")
                .map(|_| format!("cljs.core/{member}"))
        } else {
            self.resolution
                .refers
                .iter()
                .find(|(local, namespace, _)| local == &name && namespace == "cljs.core")
                .map(|(_, _, member)| format!("cljs.core/{member}"))
        };
        let shadow_core = self.macro_host.has_source_value(self.namespace, &name)
            || self
                .globals
                .get(&(self.namespace.into(), name.clone()))
                .is_some_and(|g| self.program.globals[g.0].declared)
            || self.resolution.excludes.contains(&name)
            || self
                .resolution
                .refers
                .iter()
                .any(|(local, _, _)| local == &name);
        let target = core_alias.as_deref().unwrap_or(&name);
        self.macro_host.check_macro_policy(self.namespace, target, shadow_core).map_err(error)?;
        if self.macro_host.has_javascript_macro(self.namespace, target) {
            if self.macro_depth >= self.macro_host.limits.expansion_depth.min(64) {
                return Err(error(
                    "compiler macro expansion depth limit exceeded".into(),
                ));
            }
            let form = self
                .compiler_read_form(call)
                .map_err(|e| error(e.to_string()))?;
            let locals = self
                .env
                .scopes
                .iter()
                .flat_map(|s| s.iter())
                .map(|id| {
                    let (kind, argument) =
                        self.local_kinds.get(id).copied().unwrap_or(("let", None));
                    (self.program.locals[id.0].name.clone(), id.0, kind, argument)
                })
                .collect::<Vec<_>>();
            let location = crate::LineIndex::new(self.source)
                .line_column(self.syntax.nodes[call.0].span.start)
                .unwrap();
            let env = self
                .macro_host
                .compiler_environment(
                    self.namespace,
                    context,
                    &locals,
                    self.aliases,
                    self.requires,
                    location,
                )
                .map_err(&error)?;
            let env = self.array_environment(env).map_err(error)?;
            let result = self
                .macro_host
                .expand_source_javascript(self.namespace, target, &form, &env, self.engine)
                .map_err(error)?;
            return self.import_form(&result).map(Some);
        }
        let core_name = self
            .macro_host
            .macro_name(self.namespace, target, shadow_core)
            .filter(|(ns, _)| matches!(ns.as_str(), "cljs.core" | "clojure.core"))
            .map(|(_, name)| name);
        if let Some(core_name) = core_name.as_ref().filter(|name| {
            matches!(
                name.as_str(),
                "let"
                    | "fn"
                    | "loop"
                    | "letfn"
                    | "defn"
                    | "defn-"
                    | "assert"
                    | "ns-publics"
                    | "ns-interns"
                    | "ns-imports"
                    | "ns-refers"
            )
        }) {
            if self.macro_depth >= self.macro_host.limits.expansion_depth.min(64) {
                return Err(error(
                    "compiler macro expansion depth limit exceeded".into(),
                ));
            }
            self.check_compiler_sets(call)?;
            let form = self
                .compiler_read_form(call)
                .map_err(|e| error(e.to_string()))?;
            if let Some(expanded) = self
                .macro_host
                .expand_compiler_seed(
                    &form,
                    &crate::compiler_data::Name::new(Some("cljs.core"), core_name),
                )
                .map_err(error)?
            {
                return self.import_form(&expanded).map(Some);
            }
        }
        if let Some(core_name) = core_name {
            if self.macro_depth >= self.macro_host.limits.expansion_depth.min(64) {
                return Err(error(
                    "compiler macro expansion depth limit exceeded".into(),
                ));
            }
            self.check_compiler_sets(call)?;
            let form = self
                .compiler_read_form(call)
                .map_err(|e| error(e.to_string()))?;
            let locals = self
                .env
                .scopes
                .iter()
                .flat_map(|s| s.iter())
                .map(|id| {
                    let (kind, argument) =
                        self.local_kinds.get(id).copied().unwrap_or(("let", None));
                    (self.program.locals[id.0].name.clone(), id.0, kind, argument)
                })
                .collect::<Vec<_>>();
            let location = crate::LineIndex::new(self.source)
                .line_column(self.syntax.nodes[call.0].span.start)
                .unwrap();
            let environment = self
                .macro_host
                .compiler_environment(
                    self.namespace,
                    context,
                    &locals,
                    self.aliases,
                    self.requires,
                    location,
                )
                .map_err(&error)?;
            let environment = self.array_environment(environment).map_err(error)?;
            let expanded = self
                .macro_host
                .expand_core_javascript(&core_name, &form, &environment, self.engine)
                .map_err(error)?;
            return self.import_form(&expanded).map(Some);
        }
        Ok(None)
    }

    fn array_environment(
        &self,
        environment: crate::compiler_data::Datum,
    ) -> Result<crate::compiler_data::Datum, String> {
        let c = crate::compiler_data::DataBuilder;
        let environment = self.macro_host.reader_environment(environment, self.namespace,
            &self.resolution.refers, &self.resolution.excludes, &self.program.globals)?;
        c.assoc(
            &environment,
            &c.keyword(crate::compiler_data::Name::new(
                Some("light-cljs"),
                "unchecked-arrays",
            )),
            &c.boolean(self.unchecked_arrays),
        )
    }

    fn needs_reader(&self, root: NodeId) -> bool {
        if self
            .generated
            .get(root.0)
            .and_then(Option::as_ref)
            .is_some()
        {
            return false;
        }
        let span = &self.syntax.nodes[root.0].span;
        if self.discards[self.next_discard..]
            .iter()
            .any(|id| span.contains(&self.syntax.nodes[id.0].span.start))
        {
            return true;
        }
        let mut pending = vec![root];
        while let Some(id) = pending.pop() {
            match &self.syntax.nodes[id.0].kind {
                NodeKind::Regex
                | NodeKind::Tagged { .. }
                | NodeKind::Metadata { .. }
                | NodeKind::NamespacedMap { .. }
                | NodeKind::Collection {
                    kind: CollectionKind::AnonymousFn,
                    ..
                }
                | NodeKind::Prefix {
                    kind:
                        PrefixKind::SyntaxQuote
                        | PrefixKind::Deref
                        | PrefixKind::VarQuote
                        | PrefixKind::Unquote
                        | PrefixKind::UnquoteSplicing,
                    ..
                } => return true,
                NodeKind::Collection { children, .. } => pending.extend(children),
                NodeKind::Prefix { form, .. } => pending.push(*form),
                _ => (),
            }
        }
        false
    }

    fn read_discards_before(&mut self, end: usize) -> Result<(), AnalysisError> {
        while let Some(&id) = self.discards.get(self.next_discard) {
            if self.syntax.nodes[id.0].span.start >= end {
                break;
            }
            self.next_discard += 1;
            self.expand_reader(id)?;
        }
        Ok(())
    }

    fn reader_bindings(&self) -> Vec<(String, String, String)> {
        // Resolve compiler exception class/helper names against the JS core,
        // independently of the native class/Var surface, before
        // applying explicit excludes, refers and namespace definitions below.
        let mut bindings = [
            "ExceptionInfo",
            "ex-info",
            "ex-data",
            "ex-message",
            "ex-cause",
            "instance?",
            "Throwable->map",
        ]
        .into_iter()
        .map(|name| (name.into(), "cljs.core".into(), name.into()))
        .collect::<Vec<_>>();
        bindings.extend(
            self.resolution
                .excludes
                .iter()
                .map(|name| (name.clone(), self.namespace.into(), name.clone())),
        );
        bindings.extend(self.resolution.refers.clone());
        bindings.extend(self.macro_host.reader_refers(self.namespace));
        bindings.extend(
            self.program
                .globals
                .iter()
                .filter(|g| g.namespace == self.namespace)
                .map(|g| (g.name.clone(), g.namespace.clone(), g.name.clone())),
        );
        bindings.extend(self.macro_host.javascript_macros.keys().filter_map(|key| {
            let (namespace, name) = key.split_once('/')?;
            (namespace == self.namespace).then(|| (name.into(), namespace.into(), name.into()))
        }));
        bindings
    }

    fn compiler_read_form(&mut self, id: NodeId) -> Result<Form, crate::compiler_data::ReadError> {
        if let Some(form) = self.generated.get(id.0).and_then(Option::as_ref) {
            return Ok(form.clone());
        }
        let aliases = self
            .macro_host
            .reader_aliases(self.namespace, self.aliases, self.requires);
        let globals = self.reader_bindings();
        let source = self
            .macro_source
            .get_or_insert_with(|| Source::new(
                self.resolution.source_name.as_deref().unwrap_or(self.namespace),
                self.source,
            ));
        let mut view = crate::compiler_reader::NamespaceView::new(self.namespace, source);
        view.aliases = aliases
            .iter()
            .map(|(a, n)| {
                (
                    crate::compiler_data::Name::new(None, a),
                    crate::compiler_data::Name::new(None, n),
                )
            })
            .collect();
        view.refers = globals
            .iter()
            .map(|(a, n, v)| {
                (
                    crate::compiler_data::Name::new(None, a),
                    crate::compiler_data::Name::new(Some(n), v),
                )
            })
            .collect();
        view.excludes = self
            .resolution
            .excludes
            .iter()
            .map(|n| crate::compiler_data::Name::new(None, n))
            .collect();
        self.macro_host.register_macro_view(&mut view);
        let binding_kinds = self
            .program
            .exprs
            .iter()
            .filter_map(|expr| match &expr.kind {
                ExprKind::DefProtocol { global, .. } => {
                    Some((global.0, crate::compiler_reader::BindingKind::Protocol))
                }
                ExprKind::DefType { global, record, .. } => Some((
                    global.0,
                    if *record {
                        crate::compiler_reader::BindingKind::Record
                    } else {
                        crate::compiler_reader::BindingKind::Type
                    },
                )),
                _ => None,
            })
            .collect::<HashMap<_, _>>();
        for (index, global) in self
            .program
            .globals
            .iter()
            .enumerate()
            .filter(|(_, g)| g.declared)
        {
            let qualified = crate::compiler_data::Name::new(Some(&global.namespace), &global.name);
            let binding = crate::compiler_reader::ResolvedBinding {
                id: crate::compiler_reader::BindingId(index),
                kind: binding_kinds
                    .get(&index)
                    .copied()
                    .unwrap_or(crate::compiler_reader::BindingKind::Value),
                name: qualified.clone(),
            };
            view.bindings.insert(qualified.clone(), binding.clone());
            if global.namespace == self.namespace {
                view.bindings.insert(
                    crate::compiler_data::Name::new(None, &global.name),
                    binding.clone(),
                );
            }
            for (local, target) in &view.refers {
                if target == &qualified {
                    view.bindings.insert(local.clone(), binding.clone());
                }
            }
            view.declaration_frontier = view.declaration_frontier.max(index + 1);
        }
        self.macro_host
            .lower_form_in_view(source, &self.syntax, id, &view, &self.discards)
    }
    fn expand_reader(&mut self, id: NodeId) -> Result<NodeId, AnalysisError> {
        self.check_compiler_sets(id)?;
        let form = self.compiler_read_form(id).map_err(|e| AnalysisError {
            span: e.span.bytes(),
            kind: AnalysisErrorKind::MacroExpansion(format!("reader: {e}")),
        })?;
        while self.discards.get(self.next_discard).is_some_and(|discard| {
            self.syntax.nodes[discard.0].span.start < self.syntax.nodes[id.0].span.end
        }) {
            self.next_discard += 1;
        }
        self.import_form(&form)
    }

    fn attach_metadata(
        &mut self,
        id: NodeId,
        value: ExprId,
        context: ExprContext,
        quoted: bool,
    ) -> Result<ExprId, AnalysisError> {
        let metadata = self
            .generated
            .get(id.0)
            .and_then(Option::as_ref)
            .and_then(Form::metadata);
        if let Some(metadata) = metadata {
            let metadata = self.import_form(&metadata)?;
            // CLJS strips reader locations and its private analyzed marker from
            // emitted object metadata, including quoted constants. Definition
            // metadata is stored separately and is not filtered here.
            if let NodeKind::Collection {
                kind: CollectionKind::Map,
                children,
            } = self.syntax.nodes[metadata.0].kind.clone()
            {
                let children: Vec<_> = children
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .filter(|pair| {
                        let Some(form) = self.generated.get(pair[0].0).and_then(Option::as_ref)
                        else {
                            return true;
                        };
                        let DatumView::Keyword(name) = form.data().inspect() else {
                            return true;
                        };
                        let local = String::from_utf16_lossy(name.name());
                        let namespace = name.namespace().map(String::from_utf16_lossy);
                        !(namespace.is_none()
                            && matches!(
                                local.as_str(),
                                "file" | "line" | "column" | "end-column" | "end-line" | "source"
                            )
                            || namespace.as_deref() == Some("cljs.analyzer") && local == "analyzed")
                    })
                    .flatten()
                    .copied()
                    .collect();
                if children.is_empty() {
                    return Ok(value);
                }
                self.syntax.to_mut().nodes[metadata.0].kind = NodeKind::Collection {
                    kind: CollectionKind::Map,
                    children,
                };
            }
            let metadata = if !quoted {
                // Runtime object metadata is evaluated with the value.
                // In letfn this is initializer work, before entering the body.
                self.non_tail_form(metadata, ExprContext::Value)?
            } else {
                self.datum(metadata, ExprContext::Value)?
            };
            if let ExprKind::Reify {
                metadata: initial, ..
            } = &mut self.program.exprs[value.0].kind
            {
                *initial = Some(metadata);
                return Ok(value);
            }
            return Ok(self.push(
                ExprKind::WithMeta { value, metadata },
                self.syntax.nodes[id.0].span.clone(),
                context,
            ));
        }
        Ok(value)
    }

    fn reader_literal(
        &mut self,
        form: &Form,
        kind: CompilerLiteralKind,
        context: ExprContext,
        quoted: bool,
    ) -> Result<ExprId, AnalysisError> {
        let data = form
            .data()
            .compiler_literal_data()
            .expect("compiler literal payload");
        if kind == CompilerLiteralKind::BigInteger {
            let DatumView::String(text) = data.inspect() else {
                unreachable!("integer payload")
            };
            let value = String::from_utf16_lossy(&text)
                .parse::<f64>()
                .expect("validated integer");
            return Ok(self.push(ExprKind::Number(value), form.span().bytes(), context));
        }
        let payload = if let Some(payload) = form.node().compiler_payload.and_then(|id| form.at(id))
        {
            payload
        } else {
            Form::from_data(data, form.span(), form.node().origin.clone())
        };
        let payload = self.import_form(&payload)?;
        let mut items = Vec::new();
        if matches!(
            kind,
            CompilerLiteralKind::JsArray
                | CompilerLiteralKind::JsObject
                | CompilerLiteralKind::Queue
        ) {
            let NodeKind::Collection { children, .. } = self.syntax.nodes[payload.0].kind.clone()
            else {
                unreachable!()
            };
            for (index, child) in children.into_iter().enumerate() {
                if kind == CompilerLiteralKind::JsObject && index % 2 == 0 {
                    let string = match self.generated[child.0].as_ref().unwrap().data().inspect() {
                        DatumView::Keyword(name) => name.name().to_vec(),
                        DatumView::String(text) => text.to_vec(),
                        _ => unreachable!("validated JS key"),
                    };
                    let string = self.own_string(string);
                    items.push(self.push(
                        ExprKind::String(string),
                        form.span().bytes(),
                        ExprContext::Value,
                    ));
                } else {
                    items.push(if quoted {
                        self.datum(child, ExprContext::Value)?
                    } else {
                        self.non_tail_form(child, ExprContext::Value)?
                    });
                }
            }
        } else {
            items.push(self.datum(payload, ExprContext::Value)?);
        }
        Ok(self.push(
            ExprKind::ReaderLiteral {
                kind,
                items,
                quoted,
            },
            form.span().bytes(),
            context,
        ))
    }

    /// Append semantic data directly to a private syntax arena. Spellings are
    /// owned side-table data, never printed/reparsed or sliced from fake source.
    fn import_form(&mut self, form: &Form) -> Result<NodeId, AnalysisError> {
        let mut spelling = None;
        let mut numeric = None;
        let unsupported = |what| AnalysisError {
            kind: AnalysisErrorKind::Unsupported(what),
            span: form.span().bytes(),
        };
        let kind = match &form.node().kind {
            FormKind::Collection { kind, .. } => {
                let children = form
                    .children()
                    .iter()
                    .map(|child| self.import_form(child))
                    .collect::<Result<_, _>>()?;
                NodeKind::Collection {
                    kind: *kind,
                    children,
                }
            }
            FormKind::Scalar => match form.data().inspect() {
                DatumView::Nil => NodeKind::Nil,
                DatumView::Boolean(value) => NodeKind::Boolean(value),
                DatumView::Integer(value) => {
                    numeric = Some(Some(value as f64));
                    NodeKind::Number(NumberKind::Integer)
                }
                DatumView::Double(value) => {
                    numeric = Some(Some(value));
                    NodeKind::Number(NumberKind::Float)
                }
                DatumView::Keyword(_) => NodeKind::Keyword {
                    auto_resolve: false,
                },
                DatumView::Ratio {
                    numerator,
                    denominator,
                } => {
                    if denominator != 1 {
                        return Err(unsupported(
                            "non-integral ratios are not ClojureScript constants".into(),
                        ));
                    }
                    numeric = Some(Some(numerator as f64));
                    NodeKind::Number(NumberKind::Integer)
                }
                DatumView::CompilerLiteral(_) => NodeKind::Regex,
                DatumView::Character(value) => {
                    NodeKind::String(crate::StringValue::Decoded(vec![value]))
                }
                DatumView::String(value) => {
                    NodeKind::String(crate::StringValue::Decoded(value.to_vec()))
                }
                DatumView::Symbol(name) => {
                    // Invalid Unicode is legal quoted name data. Delay the
                    // executable-name check until analysis knows its context.
                    spelling = Some(
                        String::from_utf16(name.name())
                            .and_then(|local| match name.namespace() {
                                Some(ns) => String::from_utf16(ns)
                                    .map(|namespace| format!("{namespace}/{local}")),
                                None => Ok(local),
                            })
                            .unwrap_or_default(),
                    );
                    NodeKind::Symbol
                }
                other => {
                    return Err(unsupported(format!(
                        "runtime value generated by a macro: {other:?}"
                    )));
                }
            },
        };
        let id = NodeId(self.syntax.nodes.len());
        self.syntax.to_mut().nodes.push(crate::Node {
            kind,
            span: form.span().bytes(),
        });
        self.generated.resize(id.0, None);
        self.spellings.resize(id.0, None);
        self.numbers.resize(id.0, None);
        self.generated.push(Some(form.clone()));
        self.spellings.push(spelling);
        self.numbers.push(numeric);
        Ok(id)
    }

    fn body(
        &mut self,
        forms: &[NodeId],
        span: Span,
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        let mut exprs = Vec::with_capacity(forms.len());
        for (index, &form) in forms.iter().enumerate() {
            exprs.push(if index + 1 == forms.len() {
                self.form(form, context)?
            } else {
                self.non_tail_form(form, ExprContext::Discard)?
            });
            // Warning APIs report diagnostics before any later source failure.
            if self.options.warning_mode { self.drain_deferred()?; }
        }
        Ok(self.push(ExprKind::Do(exprs), span, context))
    }

    fn if_form(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        if !(2..=3).contains(&args.len()) {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm(
                    "if requires a test, a then branch, and an optional else branch",
                ),
            ));
        }
        let test = self.non_tail_form(args[0], ExprContext::Value)?;
        let yes = self.form(args[1], context)?;
        let no = if let Some(&no) = args.get(2) {
            self.form(no, context)?
        } else {
            self.push(ExprKind::Nil, self.syntax.nodes[id.0].span.clone(), context)
        };
        Ok(self.push(
            ExprKind::If { test, yes, no },
            self.syntax.nodes[id.0].span.clone(),
            context,
        ))
    }

    fn vector(&self, id: NodeId, message: &'static str) -> Result<&[NodeId], AnalysisError> {
        match &self.syntax.nodes[id.0].kind {
            NodeKind::Collection {
                kind: CollectionKind::Vector,
                children,
            } => Ok(children),
            _ => Err(self.error(id, AnalysisErrorKind::InvalidForm(message))),
        }
    }

    fn let_form(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        context: ExprContext,
        is_loop: bool,
    ) -> Result<ExprId, AnalysisError> {
        let Some((&bindings, body)) = args.split_first() else {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm("let* requires a binding vector"),
            ));
        };
        let pairs = self
            .vector(bindings, "let* requires a binding vector")?
            .to_vec();
        if !pairs.len().is_multiple_of(2) {
            return Err(self.error(
                bindings,
                AnalysisErrorKind::InvalidForm("let* requires an even number of binding forms"),
            ));
        }
        self.env.scopes.push(Vec::new());
        let result = (|| {
            let mut bindings = Vec::with_capacity(pairs.len() / 2);
            for pair in pairs.as_chunks::<2>().0 {
                self.binding_name(pair[0])?;
                // Own initializer sees outer bindings, but never its new binding.
                let init = self.non_tail_form(pair[1], ExprContext::Value)?;
                let local = self.bind(pair[0])?;
                bindings.push((local, init));
            }
            let span = self.syntax.nodes[id.0].span.clone();
            let target = is_loop.then(|| {
                self.new_recur_target(bindings.iter().map(|(id, _)| *id).collect(), false)
            });
            let previous = self.recur_target;
            if let Some(target) = target {
                self.recur_target = Some(target);
                for &(local, _) in &bindings {
                    self.local_kinds.insert(local, ("loop", None));
                }
            }
            let body = self.body(
                body,
                span.clone(),
                if context == ExprContext::Value {
                    ExprContext::Return
                } else {
                    context
                },
            );
            self.recur_target = previous;
            let body = body?;
            Ok(self.push(
                if let Some(target) = target {
                    ExprKind::Loop {
                        target,
                        bindings,
                        body,
                    }
                } else {
                    ExprKind::Let { bindings, body }
                },
                span,
                context,
            ))
        })();
        self.env.scopes.pop();
        result
    }

    fn letfn_form(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        let Some((&bindings, body)) = args.split_first() else {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm("letfn* requires a binding vector"),
            ));
        };
        let pairs = self
            .vector(bindings, "letfn* requires a binding vector")?
            .to_vec();
        if !pairs.len().is_multiple_of(2) {
            return Err(self.error(
                bindings,
                AnalysisErrorKind::InvalidForm("letfn* requires an even number of binding forms"),
            ));
        }
        self.env.scopes.push(Vec::new());
        let result = (|| {
            // Allocate the complete group before analyzing any initializer.
            // Like CLJS's name->initializer map, repeated names keep their last
            // declaration; no discarded initializer is analyzed or evaluated.
            let mut declarations: Vec<(LocalId, NodeId)> = Vec::new();
            for pair in pairs.as_chunks::<2>().0 {
                let name = self.binding_name(pair[0])?;
                if let Some((_, init)) = declarations
                    .iter_mut()
                    .find(|(local, _)| self.program.locals[local.0].name == name)
                {
                    *init = pair[1];
                } else {
                    let local = self.bind(pair[0])?;
                    self.local_kinds.insert(local, ("letfn", None));
                    declarations.push((local, pair[1]));
                }
            }
            let mut bindings = Vec::with_capacity(declarations.len());
            for (local, initializer) in declarations {
                let init = self.non_tail_form(initializer, ExprContext::Value)?;
                let mut function = init;
                while let ExprKind::WithMeta { value, .. } = self.program.exprs[function.0].kind {
                    function = value;
                }
                if !matches!(self.program.exprs[function.0].kind, ExprKind::Fn { .. }) {
                    return Err(self.error(
                        initializer,
                        AnalysisErrorKind::InvalidForm("letfn* initializers must be functions"),
                    ));
                }
                bindings.push((local, init));
            }
            let span = self.syntax.nodes[id.0].span.clone();
            let body = self.body(
                body,
                span.clone(),
                if context == ExprContext::Value {
                    ExprContext::Return
                } else {
                    context
                },
            )?;
            Ok(self.push(ExprKind::LetFn { bindings, body }, span, context))
        })();
        self.env.scopes.pop();
        result
    }

    fn throw_form(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        if args.len() != 1 {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm("throw requires one argument"),
            ));
        }
        let value = self.non_tail_form(args[0], ExprContext::Value)?;
        Ok(self.push(
            ExprKind::Throw(value),
            self.syntax.nodes[id.0].span.clone(),
            context,
        ))
    }

    fn try_form(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        let previous = self.recur_target.take();
        let result = (|| {
            let mut body_forms = Vec::new();
            let mut parsed_body = None;
            let mut catches = Vec::new();
            let mut finally = None;
            let body_context = if context == ExprContext::Value {
                ExprContext::Return
            } else {
                context
            };
            for &arg in args {
                let clause = match &self.syntax.nodes[arg.0].kind {
                    NodeKind::Collection {
                        kind: CollectionKind::List,
                        children,
                    } if !children.is_empty() => {
                        Some((self.spelling(children[0]).to_owned(), children.clone()))
                    }
                    _ => None,
                };
                if parsed_body.is_none()
                    && clause
                        .as_ref()
                        .is_some_and(|(name, _)| matches!(name.as_str(), "catch" | "finally"))
                {
                    parsed_body = Some(self.body(
                        &body_forms,
                        self.syntax.nodes[id.0].span.clone(),
                        body_context,
                    )?);
                }
                match clause.as_ref().map(|(name, _)| name.as_str()) {
                    Some("catch") => {
                        let items = &clause.as_ref().unwrap().1;
                        if items.len() < 3
                            || finally.is_some()
                            || catches.iter().any(|c: &CatchClause| c.class.is_none())
                        {
                            return Err(self.error(
                                arg,
                                AnalysisErrorKind::InvalidForm("invalid catch clause"),
                            ));
                        }
                        let default_catch = if let Some(form) =
                            self.generated.get(items[1].0).and_then(Option::as_ref)
                        {
                            matches!(form.data().inspect(), DatumView::Keyword(name)
                                if name.namespace().is_none() && name.name().iter().copied().eq("default".encode_utf16()))
                        } else {
                            self.spelling(items[1]) == ":default"
                        };
                        let class = if default_catch {
                            None
                        } else {
                            Some(self.non_tail_form(items[1], ExprContext::Value)?)
                        };
                        self.env.scopes.push(Vec::new());
                        let parsed = (|| {
                            let binding = self.bind(items[2])?;
                            let body = self.body(
                                &items[3..],
                                self.syntax.nodes[arg.0].span.clone(),
                                body_context,
                            )?;
                            Ok(CatchClause {
                                class,
                                binding,
                                body,
                            })
                        })();
                        self.env.scopes.pop();
                        catches.push(parsed?);
                    }
                    Some("finally") => {
                        if finally.is_some() {
                            return Err(self.error(
                                arg,
                                AnalysisErrorKind::InvalidForm("duplicate finally clause"),
                            ));
                        }
                        finally = Some(self.body(
                            &clause.as_ref().unwrap().1[1..],
                            self.syntax.nodes[arg.0].span.clone(),
                            ExprContext::Discard,
                        )?);
                    }
                    _ if catches.is_empty() && finally.is_none() => body_forms.push(arg),
                    _ => {
                        return Err(self.error(
                            arg,
                            AnalysisErrorKind::InvalidForm(
                                "try body must precede catch and finally clauses",
                            ),
                        ));
                    }
                }
            }
            let body = match parsed_body {
                Some(body) => body,
                None => self.body(
                    &body_forms,
                    self.syntax.nodes[id.0].span.clone(),
                    body_context,
                )?,
            };
            Ok(self.push(
                ExprKind::Try {
                    body,
                    catches,
                    finally,
                },
                self.syntax.nodes[id.0].span.clone(),
                context,
            ))
        })();
        self.recur_target = previous;
        result
    }

    fn case_form(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        context: ExprContext,
        compiler_macro: bool,
    ) -> Result<ExprId, AnalysisError> {
        if !(3..=4).contains(&args.len()) {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm(
                    "case* requires a selector, grouped tests, branches and optional default",
                ),
            ));
        }
        let raw = self
            .vector(args[1], "case* requires a test vector")?
            .to_vec();
        let (pairs, default_node) = {
            if !compiler_macro && !matches!(self.syntax.nodes[args[0].0].kind, NodeKind::Symbol) {
                return Err(self.error(
                    args[0],
                    AnalysisErrorKind::InvalidForm("case* must switch on a symbol"),
                ));
            }
            let thens = self
                .vector(args[2], "case* requires a branch vector")?
                .to_vec();
            if raw.len() != thens.len() {
                return Err(self.error(
                    id,
                    AnalysisErrorKind::InvalidForm("case* test and branch counts differ"),
                ));
            }
            let mut pairs = Vec::new();
            for (tests, then) in raw.into_iter().zip(thens) {
                for &test in self.vector(tests, "case* requires constant groups")? {
                    pairs.extend([test, then]);
                }
            }
            (pairs, args.get(3))
        };
        if !pairs.len().is_multiple_of(2) {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm("case* requires constant/branch pairs"),
            ));
        }
        let test = self.non_tail_form(args[0], ExprContext::Value)?;
        let keyword_tests =
            compiler_macro
                && !pairs.is_empty()
                && pairs.as_chunks::<2>().0.iter().all(|pair| {
                    matches!(self.syntax.nodes[pair[0].0].kind, NodeKind::Keyword { .. })
                });
        let mut constants = HashMap::new();
        let mut primitive_tests = true;
        for pair in pairs.as_chunks::<2>().0 {
            match self.syntax.nodes[pair[0].0].kind {
                NodeKind::Number(_)
                | NodeKind::String(_)
                | NodeKind::Character(_)
                | NodeKind::SymbolicValue(_) => {}
                NodeKind::Symbol => {
                    let name = self.spelling(pair[0]).to_owned();
                    if let Ok(kind @ ExprKind::Global(_)) = self.resolve(pair[0], &name) {
                        let value = self.push(
                            kind,
                            self.syntax.nodes[pair[0].0].span.clone(),
                            ExprContext::Value,
                        );
                        if self.case_constant(value) {
                            constants.insert(pair[0], value);
                        } else {
                            primitive_tests = false;
                        }
                    } else {
                        primitive_tests = false;
                    }
                }
                _ => primitive_tests = false,
            }
        }
        let mut branches = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        let mut analyzed_branches = HashMap::new();
        for pair in pairs.as_chunks::<2>().0 {
            if compiler_macro
                && let Some(data) = self.reader_datum(pair[0])?
                && !seen.insert(data)
            {
                return Err(self.error(
                    pair[0],
                    AnalysisErrorKind::InvalidForm("duplicate case test constant"),
                ));
            }
            let constant = if primitive_tests && let Some(&value) = constants.get(&pair[0]) {
                value
            } else if compiler_macro
                && matches!(self.syntax.nodes[pair[0].0].kind, NodeKind::Symbol)
            {
                self.datum(pair[0], ExprContext::Value)?
            } else {
                let value = self.non_tail_form(pair[0], ExprContext::Value)?;
                if !compiler_macro && !self.case_constant(value) {
                    return Err(self.error(
                        pair[0],
                        AnalysisErrorKind::InvalidForm(
                            "case* tests must be numbers, strings, or constants",
                        ),
                    ));
                }
                value
            };
            let branch = if let Some(&branch) = analyzed_branches.get(&pair[1]) {
                branch
            } else {
                let branch = self.form(pair[1], context)?;
                analyzed_branches.insert(pair[1], branch);
                branch
            };
            branches.push((constant, branch));
        }
        let default = default_node
            .map(|&arg| self.form(arg, context))
            .transpose()?;
        let default = if !compiler_macro && default.is_none() {
            Some(self.push(ExprKind::Nil, self.syntax.nodes[id.0].span.clone(), context))
        } else {
            default
        };
        Ok(self.push(
            ExprKind::Case {
                strict: !compiler_macro || primitive_tests,
                keyword_tests,
                test,
                branches,
                default,
            },
            self.syntax.nodes[id.0].span.clone(),
            context,
        ))
    }

    fn case_constant(&self, value: ExprId) -> bool {
        match self.program.exprs[value.0].kind {
            ExprKind::Number(_) | ExprKind::String(_) => true,
            ExprKind::Global(global) => {
                self.program.globals[global.0]
                    .metadata
                    .is_some_and(|metadata| {
                        let ExprKind::Map(ref pairs) = self.program.exprs[metadata.0].kind else {
                            return false;
                        };
                        pairs.as_chunks::<2>().0.iter().any(|&[key, value]| {
                            let ExprKind::Keyword(name) = self.program.exprs[key.0].kind else {
                                return false;
                            };
                            let name = &self.program.named_literals[name.0];
                            name.namespace.is_none()
                                && self.program.strings[name.name.0]
                                    == "const".encode_utf16().collect::<Vec<_>>()
                                && !matches!(
                                    self.program.exprs[value.0].kind,
                                    ExprKind::Nil | ExprKind::Bool(false)
                                )
                        })
                    })
            }
            _ => false,
        }
    }

    // Deferred bodies use an ordinary zero-argument function so captured locals,
    // recur targets and exception boundaries follow the existing closure path.
    fn deferred_form(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        context: ExprContext,
        delay: bool,
    ) -> Result<ExprId, AnalysisError> {
        let span = self.syntax.nodes[id.0].span.clone();
        let parameters = NodeId(self.syntax.nodes.len());
        self.syntax.to_mut().nodes.push(crate::Node {
            kind: NodeKind::Collection {
                kind: CollectionKind::Vector,
                children: vec![],
            },
            span: span.clone(),
        });
        let mut body = vec![parameters];
        body.extend_from_slice(args);
        let thunk = self.fn_form(id, &body, ExprContext::Value)?;
        let nil = self.push(ExprKind::Nil, span.clone(), ExprContext::Value);
        let callee = self.push(
            ExprKind::CoreFn(if delay {
                CoreFnId::Delay
            } else {
                CoreFnId::LazySeq
            }),
            span.clone(),
            ExprContext::Value,
        );
        Ok(self.push(
            ExprKind::Construct {
                callee,
                args: if delay {
                    vec![thunk, nil]
                } else {
                    vec![nil, thunk, nil, nil]
                },
            },
            span,
            context,
        ))
    }

    fn fn_form(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        let (name, args) = match args.split_first() {
            Some((&name, rest)) if matches!(self.syntax.nodes[name.0].kind, NodeKind::Symbol) => {
                (Some(name), rest)
            }
            _ => (None, args),
        };
        let methods = match args.first().map(|id| &self.syntax.nodes[id.0].kind) {
            Some(NodeKind::Collection {
                kind: CollectionKind::Vector,
                ..
            }) => vec![args.to_vec()],
            _ => {
                if args.is_empty() {
                    return Err(self.error(
                        id,
                        AnalysisErrorKind::InvalidForm("fn* requires a parameter vector"),
                    ));
                }
                args.iter()
                    .map(|&method| match &self.syntax.nodes[method.0].kind {
                        NodeKind::Collection {
                            kind: CollectionKind::List,
                            children,
                        } => {
                            if args.len() > 1 && children.is_empty() {
                                return Err(self.error(
                                    method,
                                    AnalysisErrorKind::InvalidForm(
                                        "fn* requires a parameter vector",
                                    ),
                                ));
                            }
                            Ok(children.clone())
                        }
                        _ => Err(self.error(
                            method,
                            AnalysisErrorKind::InvalidForm(
                                "fn* arities must be lists beginning with a parameter vector",
                            ),
                        )),
                    })
                    .collect::<Result<Vec<_>, _>>()?
            }
        };
        self.env.scopes.push(Vec::new());
        let result = (|| {
            let name = name.map(|name| self.bind(name)).transpose()?;
            if let Some(name) = name {
                self.local_kinds.insert(name, ("fn", None));
            }
            let methods = methods
                .iter()
                .map(|method| self.fn_method(id, method, MethodKind::Function))
                .collect::<Result<Vec<_>, _>>()?;
            let diagnostic_name = name
                .map(|name| self.program.locals[name.0].name.clone())
                .or_else(|| self.definition_name.clone())
                .unwrap_or_default();
            let mut fixed = std::collections::HashSet::new();
            let mut variadic = None;
            for method in &methods {
                if method.rest.is_some() {
                    if let Some(previous) = variadic.replace(method.params.len()) {
                        self.warn(
                            id,
                            "multiple-variadic-overloads",
                            format!("{diagnostic_name}: Can't have more than 1 variadic overload"),
                        );
                        if previous == method.params.len() {
                            self.warn(
                                id,
                                "overload-arity",
                                format!(
                                    "{diagnostic_name}: Can't have 2 overloads with same arity"
                                ),
                            );
                        }
                        if self.options.warning_mode {
                            continue;
                        }
                        return Err(self.error(
                            id,
                            AnalysisErrorKind::InvalidForm(
                                "fn* cannot have more than one variadic overload",
                            ),
                        ));
                    }
                } else if !fixed.insert(method.params.len()) {
                    self.warn(
                        id,
                        "overload-arity",
                        format!("{diagnostic_name}: Can't have 2 overloads with same arity"),
                    );
                    if self.options.warning_mode {
                        continue;
                    }
                    return Err(self.error(
                        id,
                        AnalysisErrorKind::InvalidForm(
                            "fn* cannot have two overloads with the same arity",
                        ),
                    ));
                }
            }
            if variadic.is_some_and(|max| fixed.iter().any(|&arity| arity > max)) {
                self.warn(id, "variadic-max-arity", format!("{diagnostic_name}: Can't have fixed arity function with more params than variadic function"));
                if !self.options.warning_mode {
                    return Err(self.error(
                        id,
                        AnalysisErrorKind::InvalidForm(
                            "fn* fixed arity cannot exceed the variadic fixed prefix",
                        ),
                    ));
                }
            }
            if self.generated.get(id.0).and_then(Option::as_ref).is_none()
                && variadic.is_some_and(|prefix| fixed.contains(&(prefix + 1)))
            {
                self.warn(
                    id,
                    "overload-arity",
                    format!("{diagnostic_name}: Can't have 2 overloads with same arity"),
                );
            }
            let span = self.syntax.nodes[id.0].span.clone();
            Ok(self.push(ExprKind::Fn { name, methods }, span, context))
        })();
        self.env.scopes.pop();
        result
    }

    fn fn_method(
        &mut self,
        id: NodeId,
        method: &[NodeId],
        method_kind: MethodKind,
    ) -> Result<FnMethod, AnalysisError> {
        self.fn_method_with_fields(id, method, method_kind, &[])
    }

    fn fn_method_with_fields(
        &mut self,
        id: NodeId,
        method: &[NodeId],
        method_kind: MethodKind,
        fields: &[(String, bool)],
    ) -> Result<FnMethod, AnalysisError> {
        let Some((&parameters, body)) = method.split_first() else {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm("fn* requires a parameter vector"),
            ));
        };
        let parameters = self
            .vector(parameters, "fn* requires a parameter vector")?
            .to_vec();
        let outer: std::collections::HashSet<_> =
            self.env.scopes.iter().flatten().copied().collect();
        let first_expr = self.program.exprs.len();
        self.env.scopes.push(Vec::new());
        let result = (|| {
            let mut field_locals = Vec::new();
            for (field, mutable) in fields {
                let local = LocalId(self.program.locals.len());
                self.program.locals.push(LocalBinding {
                    name: field.clone(),
                    span: self.syntax.nodes[id.0].span.clone(),
                    source_index: self.source_index,
                });
                self.env.scopes.last_mut().unwrap().push(local);
                self.local_kinds.insert(local, ("field", None));
                field_locals.push((local, *mutable));
            }
            let mut params = Vec::with_capacity(parameters.len());
            let mut rest = None;
            let mut parameters = parameters.into_iter();
            while let Some(parameter) = parameters.next() {
                if self.spelling(parameter) == "&" {
                    let parameter = parameters.next().ok_or_else(|| {
                        self.error(
                            parameter,
                            AnalysisErrorKind::InvalidForm("& requires one rest binding"),
                        )
                    })?;
                    if parameters.next().is_some() {
                        return Err(self.error(
                            parameter,
                            AnalysisErrorKind::InvalidForm("rest binding must be last"),
                        ));
                    }
                    let local = self.bind(parameter)?;
                    self.local_kinds.insert(local, ("arg", Some(params.len())));
                    rest = Some(local);
                    break;
                }
                // CLJS permits repeated parameter symbols: the last binding
                // wins. Unique IDs keep generated strict JavaScript legal.
                let local = self.bind(parameter)?;
                self.local_kinds.insert(local, ("arg", Some(params.len())));
                params.push(local);
            }
            let span = self.syntax.nodes[id.0].span.clone();
            // CLJS lowers IFn methods by dropping the receiver parameter and
            // binding its source name to this. The analyzer's protocol marker
            // remains on both emitted entries. Their accepted arities intersect
            // at exactly the non-receiver parameter count.
            let target = self.new_recur_target(
                params
                    .iter()
                    .skip(usize::from(method_kind == MethodKind::IFn))
                    .chain(rest.iter())
                    .copied()
                    .collect(),
                method_kind == MethodKind::Protocol,
            );
            let arguments: Vec<_> = params.iter().chain(rest.iter()).copied().collect();
            self.program.recur_targets[target.0].argument_slots = arguments
                .iter()
                .enumerate()
                .skip(usize::from(method_kind == MethodKind::IFn))
                .map(|(index, local)| {
                    (!arguments[index + 1..].iter().any(|later| {
                        self.program.locals[later.0].name == self.program.locals[local.0].name
                    }))
                    .then_some(index)
                })
                .collect();
            if self.type_fields.is_some() && !self.raw_type && method_kind != MethodKind::Function {
                self.program.type_methods.insert(target);
            }
            for &(local, mutable) in &field_locals {
                self.program.field_bindings.insert(local, (target, mutable));
            }
            let previous = self.recur_target.replace(target);
            let body = self.body(body, span, ExprContext::Return);
            self.recur_target = previous;
            let body = body?;
            let mut captures = Vec::new();
            for expr in &self.program.exprs[first_expr..] {
                if let ExprKind::Local(local) = expr.kind
                    && outer.contains(&local)
                    && !captures.contains(&local)
                {
                    captures.push(local);
                }
            }
            Ok(FnMethod {
                params,
                rest,
                body,
                target,
                captures,
            })
        })();
        self.env.scopes.pop();
        result
    }
}

pub(crate) fn number(text: &str, kind: NumberKind) -> Option<f64> {
    if kind == NumberKind::Float || kind == NumberKind::Decimal {
        return text.trim_end_matches('M').parse::<f64>().ok();
    }
    if kind == NumberKind::Ratio {
        let (n, d) = text.split_once('/')?;
        return crate::reader_literals::integral_ratio(n, d);
    }
    let text = if kind == NumberKind::BigInteger {
        text.strip_suffix('N')?
    } else {
        text
    };
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    let (radix, digits) = if let Some(hex) = unsigned
        .strip_prefix("0x")
        .or_else(|| unsigned.strip_prefix("0X"))
    {
        (16, hex)
    } else if let Some((base, digits)) = unsigned.split_once(['r', 'R']) {
        (base.parse().ok()?, digits)
    } else if unsigned.starts_with('0') {
        (8, unsigned)
    } else {
        (10, unsigned)
    };
    // Decimal parsing supplies correct IEEE rounding even above native integer widths.
    let magnitude = if radix == 10 {
        digits.parse::<f64>().ok()?
    } else if let Ok(n) = u128::from_str_radix(digits, radix) {
        n as f64
    } else {
        // Reader-owned macro arguments preserve exact integer text until the
        // form bridge converts it once. Application literals must round at that
        // same boundary, rather than accumulate rounding at every radix digit.
        crate::reader_literals::integer_key(text)
            .parse::<f64>()
            .ok()?
            .abs()
    };
    Some(if text.starts_with('-') && magnitude != 0.0 {
        -magnitude
    } else {
        magnitude
    })
}

fn js_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_alphabetic() || matches!(c, '_' | '$'))
        && chars.all(|c| c.is_alphanumeric() || matches!(c, '_' | '$'))
}

/// CLJS normalizes double dots before interpreting dots/slashes as host paths.
pub(crate) fn munge_js_path(name: &str) -> Vec<String> {
    name.replace("..", "_DOT__DOT_")
        .replace('/', ".")
        .split('.')
        .map(munge_js_member)
        .collect()
}

/// CLJS host-member munging does not suffix JavaScript reserved words.
pub(crate) fn munge_js_member(name: &str) -> String {
    let mut result = String::new();
    for c in name.chars() {
        result.push_str(match c {
            '-' => "_",
            ':' => "_COLON_",
            '+' => "_PLUS_",
            '>' => "_GT_",
            '<' => "_LT_",
            '=' => "_EQ_",
            '~' => "_TILDE_",
            '!' => "_BANG_",
            '@' => "_CIRCA_",
            '#' => "_SHARP_",
            '\'' => "_SINGLEQUOTE_",
            '"' => "_DOUBLEQUOTE_",
            '%' => "_PERCENT_",
            '^' => "_CARET_",
            '&' => "_AMPERSAND_",
            '*' => "_STAR_",
            '|' => "_BAR_",
            '{' => "_LBRACE_",
            '}' => "_RBRACE_",
            '[' => "_LBRACK_",
            ']' => "_RBRACK_",
            '/' => "$",
            '\\' => "_BSLASH_",
            '?' => "_QMARK_",
            _ => {
                result.push(c);
                continue;
            }
        });
    }
    result
}

/// Advance explicit interactive symbol state without restarting compilation roots.
pub(crate) fn analyze_interactive_form(
    form: &Form, host: &mut MacroHost, previous: Option<&Program>,
    namespace: &str, aliases: &[(String, String)], requires: &[String],
    resolution: NamespaceResolution, options: AnalysisOptions,
    warnings: &mut Vec<AnalysisWarning>,
) -> Result<Program, AnalysisError> {
    analyze_input_state(form.span().source().text(), &SyntaxTree { nodes: vec![], roots: vec![] },
        host, options, warnings, Some(form), Some((namespace, aliases, requires, resolution)),
        None, None, previous)
}
