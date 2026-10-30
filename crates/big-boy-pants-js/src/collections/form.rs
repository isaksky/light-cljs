//! Native Clojure form values for shared macro inputs and outputs
//! (light-cljs-466).
//!
//! Macros exchange *forms*: keywords, symbols, lists, vectors and maps with
//! ClojureScript value semantics. This module stores them as frozen,
//! persistent heap objects, so a form built once (by Rust, or by any task)
//! is read by every task through its identity: no JavaScript object is
//! reconstructed per task and no form is copied between tasks. It is a
//! separate kind from the SameValueZero vectors and maps of
//! [`crate::collections`], whose internal-map contract it leaves unchanged.
//!
//! # Values
//!
//! | Kind ([`FormKind`]) | Contents | Metadata |
//! |------|----------|----------|
//! | keyword | namespace (optional) and name, exact UTF-16 strings | no |
//! | symbol | namespace (optional) and name, exact UTF-16 strings | yes |
//! | list | elements in order (stored in a persistent vector trie) | yes |
//! | vector | elements in order | yes |
//! | map | structural entries in a hash trie, or positional literal entries | yes |
//! | set | structural members in a hash trie, or positional literal members | yes |
//! | lazy-seq | a once-realized cell: a thunk, then what it realized (light-cljs-479, `crate::collections::lazy`) | yes |
//! | cons | realized elements followed by an unrealized tail (light-cljs-479) | yes |
//! | reduced | opaque early-termination marker with a traced payload (light-cljs-507) | no |
//! | compiler-literal | explicit tag and a traced native payload | yes |
//!
//! Elements, keys, values and metadata may be any value: `nil`
//! (`null`/`undefined`), booleans, numbers, strings, other forms, and
//! occurrences ([`crate::collections`]): one value at one source
//! location. Children are held by identity, never copied, so an update
//! returns a new object whose untouched children (and trie nodes) are the
//! same ones as before. Metadata must be `nil` or a form map.
//!
//! Compiler literals are explicit immutable descriptors, created with
//! `formCompilerLiteral(tag, payload)` or [`Scope::new_compiler_literal`].
//! Tags are `js-array`, `js-object`, `regex`, `instant`, `uuid` and `queue`.
//! Arrays and queues require a form-vector payload, objects a form map,
//! regexes and UUIDs a string, and instants a string or Number. Payloads may
//! retain occurrences; construction validates their underlying form kind.
//! `formName` reads the tag and `formLiteralPayload` reads the payload.
//! A regex literal (also `formRegex(source, flags)`) is a RegExp in
//! JavaScript whose `lastIndex` stays 0 (`crate::regexp::shared`).
//! JS containers and regexes compare and hash by identity; UUIDs and instants
//! compare by value, and queues compare and hash as sequential collections.
//! Descriptors are otherwise opaque to collection
//! operations. Publication checks the payload and metadata, so wrapping a
//! mutable host value cannot make it shareable. GC traces both.
//! `cljRealizeAll` realizes lazy values inside the payload and metadata,
//! retaining every unchanged subtree and the whole descriptor when unchanged.
//! Arbitrary host arrays and objects are never inferred to be descriptors.
//!
//! # Names
//!
//! A keyword or symbol is constructed only when its spelling (`name`, or
//! `namespace/name`) splits back into the same namespace and name at its
//! first `/`, a lone `/` being an unqualified name: the split the compiler
//! reader applies to a token (`name` in light-cljs `src/compiler_reader.rs`).
//! So a namespace contains no `/`, an unqualified name contains no `/`
//! unless it is `/` itself, and namespace `""` with name `""` (spelled `/`)
//! is excluded. Everything the reader produces qualifies: `a`, `/`, `a/b`,
//! `clojure.core//`, auto-resolved and namespaced-map keys; so do host names
//! such as namespace `a` with name `b/c` or an empty namespace or name.
//! Construction fails with [`CollectionError::AmbiguousName`] otherwise.
//!
//! The reason is that the light-cljs runtime compares keywords and symbols
//! by their full spelling but hashes them by namespace and name
//! (`structuralEqualPair` against `hashNamed` in `src/js_runtime.js`), while
//! the compiler's `Datum` compares namespace and name. A rejected
//! name such as `(keyword nil "a/b")` is `=` to `(keyword "a" "b")` there
//! yet hashes differently (384366870 against 1482224565, light-cljs
//! `tests/fixtures/compiler-keyword-map-keys.tsv`), so its runtime maps keep
//! the two as distinct keys or merge them depending on the map's
//! representation. No single equality and hash pair reproduces that, and
//! equal keys must hash alike for structural lookup. On the supported names
//! a spelling has exactly one namespace/name pair, so the namespace/name
//! comparison below *is* the runtime's spelling comparison and the hash is
//! the runtime's.
//!
//! # Equality, hashing and identity
//!
//! [`equals`] and [`hash`] follow the light-cljs runtime's `=` and `hash`
//! (`src/js_runtime.js`, `structuralEqualPair` and `hash`) for the
//! supported values, and the hash is the same 32-bit number that runtime
//! computes:
//!
//! * `nil` equals `nil` (`null` and `undefined` alike); booleans by value;
//!   numbers by `===` (so `-0` equals `0` and `NaN` equals nothing);
//!   strings by their exact UTF-16 code units.
//! * Keywords equal keywords, and symbols symbols, with the same namespace
//!   (or both none) and the same name, each compared as exact code units.
//!   For the supported names (see "Names") this is the runtime's comparison
//!   of full spellings. A keyword never equals a symbol or a string.
//! * Lists and vectors are *sequential*: a list equals a vector with
//!   pairwise equal elements, and both hash alike (`hashOrderedColl`).
//! * Maps are equal with the same count and, for every entry of one, an
//!   equal key in the other mapping to an equal value; the hash is
//!   `hashUnorderedColl` of the entries.
//!   A sorted destination uses comparator-based key lookup. As in the legacy
//!   runtime, compare-equal keys such as `NaN` and `1` need not hash alike,
//!   and equality between sorted and hash maps can be directional. A
//!   `sorted-map-by` destination needs its comparator, which only the task
//!   runtime calls: structural equality asks the task runtime for that one
//!   map comparison ([`take_sorted_request`]) and the instruction runs again
//!   with its answer ([`install_sorted_answers`]); without a running task
//!   runtime it reports [`CollectionError::Unsupported`].
//! * Sets are equal with the same count and an equal member in the other
//!   for every member of one; the hash is `hashUnorderedColl` of the
//!   members. A set never equals a map, list or vector.
//! * Positional literals retain equal lowered keys/members in source order.
//!   Lookup uses the first equal key; hashing includes every stored position.
//!   Unique positional forms compare and hash like hashed forms. Duplicates
//!   retain the legacy anomalies: set equality can be directional, and maps
//!   with different values at equal keys can compare unequal to a copy.
//!   Equality cannot reject by cached hash, even for enclosing collections.
//! * Metadata never takes part in structural equality or hashing. Opaque
//!   compiler descriptors retain identity equality: attaching new metadata
//!   creates a fresh descriptor that shares the original payload.
//! * Reduced markers compare and hash by identity, independent of their
//!   payload. They are not sequences; `cljUnreduced` unwraps one marker.
//! * Occurrences are transparent: an occurrence equals (and hashes as) the
//!   value it wraps, so provenance never affects equality.
//! * The same object always equals itself (`identical?` first, as `=`
//!   does), even a list holding `NaN`.
//! * Anything else (BigInts, JavaScript symbols, other heap objects,
//!   including SameValueZero vectors and maps, plain objects and
//!   functions) compares by `===`, with a private hash consistent with it.
//!
//! **Map keys are structural**: a lookup, `assoc` or `dissoc` finds the
//! entry whose key *equals* the probe, so separately allocated equal
//! keywords, symbols, vectors, lists or maps (and an occurrence of the key)
//! retrieve the same entry. `assoc` of an equal key keeps the stored key
//! object. Hashes are cached in each form on first use; equality and
//! hashing walk with explicit heap stacks (no native recursion, including
//! across nested keys whose full hashes collide), read only frozen bodies,
//! take no lock beyond resolving each object, and never run JavaScript.
//! Equality decides each distinct pair of form objects once, so its work is
//! linear in the distinct pairs it compares and their children; a map
//! entry whose full hash `c` keys of the other map share tries up to `c`
//! candidates (stopping at the first equal key) instead of one. A lookup
//! compares the probe with each stored key of its full hash in turn.
//! User `IEquiv`/`IHash` implementations and protocols are outside this
//! contract and never enter engine bookkeeping.
//!
//! **Identity and provenance are separate from equality.** Two keywords
//! `:a` allocated separately are equal but are two objects; `===` and
//! [`Scope::same_object`] tell them apart. Equal scalars at different
//! source locations are two occurrences with two provenances. A form a
//! task returns unchanged is the same object Rust gave it, and a no-op
//! update (`assoc` of an identical value, `dissoc` of an absent key,
//! `with_meta` of the same metadata) returns its input object itself.
//! Record extension updates copy their map even for unchanged entries, as
//! the legacy runtime does; metadata copies and field updates share it.
//! Map seq entries have vector equality and hashing but exact 0/1 call
//! indices; their `assoc`, `conj` and `with_meta` produce ordinary vectors,
//! including when the element or metadata is unchanged.
//!
//! # Exact literals and provenance
//!
//! A form element holds a JavaScript value, so a reader literal that no
//! Number represents exactly (an integer beyond 2^53, a ratio, a `1.0`
//! spelled differently from `1`) has only its nearest Number here. The
//! host keeps the exact text: it wraps such a literal in an occurrence
//! whose [`Provenance`](crate::Provenance) names the source location, and
//! keeps its own table from that location to the literal (the compiler
//! already owns the source text). A macro that returns the literal
//! unchanged returns the same occurrence object, so the host reads its
//! provenance back and emits the exact literal. A macro that computes a
//! new number (or unwraps the occurrence and wraps nothing) produces a
//! plain Number with no provenance, which the host emits as that Number.
//!
//! # Sharing, rooting and publication
//!
//! Forms are persistent (`crate::value::share`): publishable when
//! everything they reach is. A form holding a plain object, an array, a
//! builder or any other task-owned value anywhere (an element, a key, a
//! value or metadata) is rejected by [`crate::Runtime::publish`] and by
//! every publication path (dependency results, atoms), with a path such
//! as `value[1]{:k}.meta` naming where. A form's `Trace` reports every
//! element, key, value and its metadata; it is reachable only while
//! rooted (a [`crate::Rooted`], a scope local, a task frame or another
//! reachable value), and reclaimed after that.
//!
//! # Embedder API ([`Scope`])
//!
//! Construction: [`Scope::new_keyword`], [`Scope::new_symbol`],
//! [`Scope::new_list`], [`Scope::new_form_vector`],
//! [`Scope::new_form_map`] (a later duplicate key wins), and
//! [`Scope::form_with_meta`]. Inspection: [`Scope::form_kind`],
//! [`Scope::form_name`], [`Scope::form_count`], [`Scope::form_nth`], [`Scope::form_items`],
//! [`Scope::form_entries`], [`Scope::form_get`], [`Scope::form_meta`],
//! [`Scope::form_value`] (an occurrence's value). Updates:
//! [`Scope::form_assoc`], [`Scope::form_dissoc`], [`Scope::form_conj`].
//! Comparison: [`Scope::form_equals`], [`Scope::form_hash`]. Errors are
//! [`CollectionError`]s (`WrongType`, `InvalidIndex`, `IndexOutOfBounds`,
//! `AmbiguousName`, `OutOfMemory`); nothing panics. JavaScript reaches the
//! same operations through the private `__rt.form*` intrinsics
//! ([`crate::vm::intrinsics`]).
//!
//! ```
//! use big_boy_pants_js::frontend::compile_function;
//! use big_boy_pants_js::{FormKind, JsString, Primitive, RunOutcome, Runtime};
//!
//! let rt = Runtime::new();
//! let s = |t: &str| JsString::from(t);
//! // Rust builds `^{:line 1} (my.ns/defthing [a] {[:k] "v"})`.
//! let form = rt.scope(|sc| {
//!     let sym = sc.new_symbol(Some(s("my.ns")), s("defthing")).unwrap();
//!     let a = sc.new_symbol(None, s("a")).unwrap();
//!     let params = sc.new_form_vector(&[a]).unwrap();
//!     let k = sc.new_keyword(None, s("k")).unwrap();
//!     let key = sc.new_form_vector(&[k]).unwrap();
//!     let v = sc.primitive(Primitive::String(s("v")));
//!     let body = sc.new_form_map(&[(key, v)]).unwrap();
//!     let line = sc.new_keyword(None, s("line")).unwrap();
//!     let one = sc.primitive(Primitive::Number(1.0));
//!     let meta = sc.new_form_map(&[(line, one)]).unwrap();
//!     let list = sc.new_list(&[sym, params, body]).unwrap();
//!     sc.persist(sc.form_with_meta(list, Some(meta)).unwrap())
//! });
//! rt.publish(&form).unwrap(); // shareable: every part is immutable
//!
//! // A JavaScript task reads it with private intrinsics: a separately
//! // allocated key `[:k]` (here a *list*, sequentially equal) finds the
//! // entry, and the form is returned by identity.
//! let read = compile_function(
//!     "function read(form) {
//!        const body = __rt.formNth(form, 2);
//!        const key = __rt.formList(__rt.formKeyword(null, 'k'));
//!        if (__rt.formGet(body, key, null) !== 'v') throw new Error('lookup');
//!        if (__rt.formName(__rt.formNth(form, 0)) !== 'defthing') throw new Error('name');
//!        return form;
//!      }",
//! ).unwrap();
//! let task = rt.spawn(&read, &[&form]).unwrap();
//! let RunOutcome::Complete(result) = task.run().unwrap() else { panic!() };
//! assert!(result.same_object(&form));
//! rt.scope(|sc| {
//!     let l = sc.local(&result).unwrap();
//!     assert_eq!(sc.form_kind(l), Some(FormKind::List));
//!     assert!(sc.form_meta(l).is_some());
//! });
//! ```
//!
//! # Structs (light-cljs-478)
//!
//! A *struct type* (`FormKind::Type`, `formKind` `"type"`) is the identity of
//! a ClojureScript `deftype` or `defrecord` without mutable fields: its
//! qualified name, its field names, whether it is a record and the
//! JavaScript member name of each field (`null`: the field names), made by
//! `__rt.structType(name, fieldNames, record, memberNames)`; each call is a
//! new identity. A struct is the same object in JavaScript, a frozen one:
//! its fields are its own enumerable, non-writable, non-configurable data
//! properties under their member names (the last field with a member name
//! wins, as a constructor's assignments do; a record also has `__meta`,
//! `__extmap` and `__hash` unless a field already took the name), it is
//! not extensible, its prototype is `Object.prototype`, and its other named
//! members come from the task's core-protocol resolver
//! (`crate::vm::property`, `Deferred::Form`).
//! A *struct* (`FormKind::Struct`; `formKind` `"record"` or `"struct"`) is
//! an instance: its type, its field values in order and, for a record, an
//! extension map with structural keys. Structs are immutable and
//! persistent: publishable when their fields and extension entries are,
//! traced through their type, fields and entries (publication path steps
//! `[i]` for fields, `{key}` for entries).
//!
//! A record follows the light-cljs runtime's record methods: an
//! unqualified keyword naming a field looks the field up, other keys the
//! extension map (`cljGet`, `cljContains`, `cljAssoc`, `cljDissoc` — of a
//! field: a form map of the other entries with the record's metadata —,
//! `cljConj`, `cljCount`, `cljSeq`); two records are equal when they have
//! the same type identity, equal fields and equal extension maps (including
//! the map identity shortcut for shared positional extensions); the
//! hash is the sum of each entry's key hash xor value hash, xor the
//! `stringHash` of the type name, cached like every form hash in an
//! atomic, idempotent slot. A deftype instance is equal only to itself and
//! hashes by identity; the leaf clj operations reject it. These structural
//! rules also govern collection keys, members and stored values, without
//! invoking user code. VM-level core operations on a struct may instead call
//! a task-local user protocol resolver (`crate::vm::native`); its callbacks
//! never enter this module's equality or hash functions. A record is never
//! structurally equal to a map.
//!
//! `__rt.cljDispatch(table, v)` is protocol dispatch: `table` is a
//! SameValueZero persistent map ([`crate::collections`]) from dispatch keys
//! to implementations; a struct's key is its type (identity), other
//! values' a kind string (`crate::collections::clj::dispatch_key`), and
//! `"_"` is the default. A JavaScript object without a kind gives
//! `undefined` for the caller to resolve. No user equality, hashing or
//! property access runs.
//!
//! # Supported boundary
//!
//! Supported: the kinds above, metadata, occurrences as elements and
//! keys, the operations listed, sequential list/vector equality, and the
//! ClojureScript core operations of [`crate::collections::clj`] (whose
//! `seq`, `rest` and `next` of a list or vector are lists sharing its trie,
//! light-cljs-502, and of a map, set or string copy into a new list). Not
//! supported (not claimed): keyword and symbol names whose spelling does
//! not split back (rejected, see "Names"), and a runtime keyword whose
//! stored spelling differs from its namespace and name (the runtime's
//! one-argument `(keyword "a/b/c")`); comparator calls for `sorted-map-by`
//! maps (their positional leaves edit entries at positions the task runtime
//! computes; see "sorted-map-by" below) and sorted sets, seq views of
//! maps, sets and strings, user hashing or equality
//! (`IHash`/`IEquiv`) of structs,
//! interned or cached keyword identity, characters as a separate kind (a
//! character is a one-unit string; the reader distinction, like exact
//! numeric spellings, travels with provenance), property access or
//! iteration protocols on forms from JavaScript, and conversion to or from
//! the compiler's `Datum` (light-cljs-462 owns that bridge). `conj` on a
//! list prepends in time linear in its length.

use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

use super::lazy::{self, ConsBody, LazyCell};
use super::map::PMap;
use super::{CollectionError, PVector, identical, index_from_value, object_name};
use crate::heap::budget::GrowthCharge;
use crate::heap::gc::{Trace, TraceEdge};
use crate::heap::{Heap, HeapObject, Local, ObjectId, Scope};
use crate::value::string::FixedText;
use crate::value::{JsString, Value};

/// The kinds of native form value. See the module docs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FormKind {
    Keyword,
    Symbol,
    List,
    Vector,
    Map,
    Set,
    /// An instance of a `deftype` or `defrecord` without mutable fields
    /// (light-cljs-478; "Structs" in the module docs).
    Struct,
    /// A struct type: the identity of a `deftype` or `defrecord`.
    Type,
    /// A lazy sequence: a once-realized cell (light-cljs-479;
    /// `crate::collections::lazy`).
    Lazy,
    /// A sequence cell: realized elements followed by a lazy tail
    /// (`crate::collections::lazy`, "Sequences").
    Cons,
    /// An immutable early-termination marker, with identity equality.
    Reduced,
    /// Explicit immutable compiler descriptor; never an arbitrary host object.
    CompilerLiteral,
}

impl FormKind {
    /// The kind's ClojureScript name: `keyword`, `symbol`, `list`, `vector`,
    /// `map`, `set`, `struct` or `type` (the result of the `formKind`
    /// intrinsic).
    pub fn name(self) -> &'static str {
        self.fixed_name().as_str()
    }

    pub(crate) fn name_string(self) -> JsString {
        self.fixed_name().get()
    }

    fn fixed_name(self) -> &'static FixedText {
        static KEYWORD: FixedText = FixedText::new("keyword");
        static SYMBOL: FixedText = FixedText::new("symbol");
        static LIST: FixedText = FixedText::new("list");
        static VECTOR: FixedText = FixedText::new("vector");
        static MAP: FixedText = FixedText::new("map");
        static SET: FixedText = FixedText::new("set");
        static STRUCT: FixedText = FixedText::new("struct");
        static TYPE: FixedText = FixedText::new("type");
        static LAZY: FixedText = FixedText::new("lazy-seq");
        static CONS: FixedText = FixedText::new("cons");
        static LITERAL: FixedText = FixedText::new("compiler-literal");
        static REDUCED: FixedText = FixedText::new("reduced");
        match self {
            FormKind::Keyword => &KEYWORD,
            FormKind::Symbol => &SYMBOL,
            FormKind::List => &LIST,
            FormKind::Vector => &VECTOR,
            FormKind::Map => &MAP,
            FormKind::Set => &SET,
            FormKind::Struct => &STRUCT,
            FormKind::Type => &TYPE,
            FormKind::Lazy => &LAZY,
            FormKind::Cons => &CONS,
            FormKind::Reduced => &REDUCED,
            FormKind::CompilerLiteral => &LITERAL,
        }
    }

    /// Whether values of this kind are sequential (`=` to a list or vector
    /// with equal elements).
    pub(crate) fn sequential(self) -> bool {
        matches!(
            self,
            FormKind::List | FormKind::Vector | FormKind::Lazy | FormKind::Cons
        )
    }

    /// The name in diagnostics, distinct from the SameValueZero kinds.
    pub(crate) fn diagnostic_name(self) -> &'static str {
        match self {
            FormKind::Vector => "form vector",
            FormKind::Map => "form map",
            FormKind::Set => "form set",
            FormKind::Type => "struct type",
            FormKind::Lazy => "lazy sequence",
            other => other.name(),
        }
    }
}

/// A keyword's or symbol's namespace and name, as exact UTF-16 strings.
#[derive(Clone, Debug)]
pub struct FormName {
    /// The namespace, if the name is qualified.
    pub namespace: Option<JsString>,
    /// The name.
    pub name: JsString,
}

impl FormName {
    /// Whether the spelling splits back into this namespace and name at its
    /// first `/` (a lone `/` unqualified): the names a form may hold. See
    /// "Names" in the module docs.
    fn splits_back(&self) -> bool {
        const SLASH: u16 = b'/' as u16;
        let name = self.name.code_units();
        match &self.namespace {
            None => name == [SLASH] || !name.contains(&SLASH),
            Some(ns) => {
                let ns = ns.code_units();
                !ns.contains(&SLASH) && !(ns.is_empty() && name.is_empty())
            }
        }
    }

    /// Same namespace (or both none) and same name, by code units. For
    /// names that split back this is equality of full spellings.
    fn same(&self, other: &FormName) -> bool {
        self.name.code_units() == other.name.code_units()
            && match (&self.namespace, &other.namespace) {
                (None, None) => true,
                (Some(a), Some(b)) => a.code_units() == b.code_units(),
                _ => false,
            }
    }

    /// The reader spelling `ns/name` (lossy for lone surrogates), for
    /// diagnostics only.
    pub(crate) fn spelling(&self) -> String {
        match &self.namespace {
            Some(ns) => format!("{}/{}", ns.to_string_lossy(), self.name.to_string_lossy()),
            None => self.name.to_string_lossy(),
        }
    }
}

/// The contents of a form, without its metadata.
#[derive(Clone, Debug)]
pub(crate) enum FormBody {
    Keyword(FormName),
    Symbol(FormName),
    List(PVector),
    Vector(PVector),
    /// Structural keys, hashed or positional, compared by [`equals`].
    /// Never read with the SameValueZero API.
    Map(FormMap),
    /// Members stored as keys that map to themselves (the stored member),
    /// stored and compared like map keys.
    Set(FormMap),
    /// A struct type ("Structs" in the module docs).
    Type(Arc<TypeInfo>),
    /// An instance of a struct type.
    Struct(StructBody),
    /// A lazy sequence (light-cljs-479): the cell is shared by every form
    /// object made from it (`with-meta` keeps it).
    Lazy(Arc<LazyCell>),
    /// Realized elements followed by a lazy tail.
    Cons(ConsBody),
    /// `reduced`: opaque to sequence operations; its value is traced.
    Reduced(Value),
    CompilerLiteral {
        tag: JsString,
        payload: Value,
        // Comparable value without replacing the original payload occurrence.
        scalar: Option<Value>,
        /// A regex's RegExp state (`crate::regexp::shared`); `None` for
        /// every other tag.
        regex: Option<Arc<crate::regexp::shared::RegexCell>>,
    },
}

/// What a struct type is: its name, its field names in order and whether
/// it is a record. The type's identity is that of its form object.
#[derive(Debug)]
pub(crate) struct TypeInfo {
    /// The qualified name, `ns/Name`, as the record hash reads it.
    pub(crate) name: JsString,
    /// The field names as the source spells them (`my-field`): a record's
    /// keyword keys.
    pub(crate) fields: Box<[JsString]>,
    /// Each field's JavaScript member name (`my_field`), one per field.
    pub(crate) members: Box<[JsString]>,
    pub(crate) record: bool,
}

/// A struct instance: its type, its field values in the type's order, and a
/// record's extension map (absent when it has no entries).
#[derive(Clone, Debug)]
pub(crate) struct StructBody {
    /// The type form object: the instance's dispatch identity.
    pub(crate) ty: Value,
    pub(crate) info: Arc<TypeInfo>,
    pub(crate) fields: PVector,
    /// The extension map itself: metadata copies and field updates retain
    /// its identity, even when duplicate keys make distinct copies unequal.
    pub(crate) ext: Option<Value>,
}

impl StructBody {
    pub(crate) fn ext_map(&self, heap: &Heap) -> Option<FormMap> {
        self.ext
            .as_ref()
            .map(|ext| map_body(heap, ext).expect("record extension is a form map"))
    }
}

/// Structural map storage. Literal entries retain every source position,
/// including keys that become equal after reader values are lowered. Their
/// lookup and updates follow the legacy PersistentArrayMap constructor.
/// PVector charges its immutable nodes and traces every retained value.
#[derive(Clone, Debug)]
pub(crate) enum FormMap {
    Hashed(PMap),
    Positional(PVector),
}

impl Default for FormMap {
    fn default() -> Self {
        Self::Hashed(PMap::default())
    }
}

impl FormMap {
    pub(crate) fn positional(&self) -> bool {
        matches!(self, Self::Positional(_))
    }

    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Hashed(map) => map.len(),
            Self::Positional(pairs) => pairs.len() / 2,
        }
    }

    pub(crate) fn for_each(&self, mut f: impl FnMut(&Value, &Value)) {
        match self {
            Self::Hashed(map) => map.for_each(f),
            Self::Positional(pairs) => {
                for i in (0..pairs.len()).step_by(2) {
                    f(&pairs.get(i).unwrap(), &pairs.get(i + 1).unwrap());
                }
            }
        }
    }

    pub(crate) fn to_vec(&self) -> Vec<(Value, Value)> {
        let mut out = Vec::with_capacity(self.len());
        self.for_each(|k, v| out.push((k.clone(), v.clone())));
        out
    }
}

impl Trace for FormMap {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        match self {
            Self::Hashed(map) => map.trace(visit),
            Self::Positional(pairs) => pairs.trace(visit),
        }
    }
}

/// One form heap object: immutable once allocated, shared by `Arc` so a
/// read copies out one pointer and its cached hash is shared by readers.
#[derive(Debug)]
pub(crate) struct FormObject {
    body: FormBody,
    meta: Option<Value>,
    /// A map's seq entry has a vector body but requires exact 0/1 call
    /// indices. Vector-producing updates and `with-meta` clear this flag.
    map_entry: bool,
    /// A positional sequence view (IndexedSeq, map/set/string seq), rather
    /// than an IList. Some(true) is counted; Some(false) is uncounted.
    /// Its list body still supplies structural value semantics.
    sequence_view: Option<bool>,
    /// Runtime reader collections use legacy hash traversal for seq as well as
    /// printing. Keep their actual hash storage through every persistent update.
    reader_traversal: bool,
    /// A sorted map. Its positional storage is already sorted, never
    /// promotes, and is not an editable collection.
    sorted: bool,
    /// A `sorted-map-by` map's comparator function (an edge). Only the task
    /// runtime calls it: engine leaves that would compare its keys raise
    /// [`CollectionError::Unsupported`] instead of using the default order.
    comparator: Option<Value>,
    /// [`hash`] of this value, computed once (metadata-independent). An
    /// interior cache, not an edge or part of the value: readers of the
    /// frozen body may race to set it, and every writer stores the same
    /// number.
    hash: OnceLock<i32>,
}

impl FormObject {
    fn new(body: FormBody, meta: Option<Value>) -> FormObject {
        FormObject {
            body,
            meta,
            map_entry: false,
            sequence_view: None,
            reader_traversal: false,
            sorted: false,
            comparator: None,
            hash: OnceLock::new(),
        }
    }

    /// A copy with other metadata, keeping the cached hash. A map entry
    /// becomes an ordinary vector, as in the legacy runtime.
    fn with_meta(&self, meta: Option<Value>) -> FormObject {
        let hash = OnceLock::new();
        if let Some(h) = self.hash.get() {
            let _ = hash.set(*h);
        }
        FormObject {
            body: self.body.clone(),
            meta,
            map_entry: false,
            sequence_view: self.sequence_view,
            reader_traversal: self.reader_traversal,
            sorted: self.sorted,
            comparator: self.comparator.clone(),
            hash,
        }
    }

    /// This value with another body and the same metadata (an update).
    fn updated(&self, body: FormBody) -> FormObject {
        let mut object = FormObject::new(body, self.meta.clone());
        object.reader_traversal = self.reader_traversal;
        object.sorted = self.sorted;
        object.comparator = self.comparator.clone();
        object
    }

    pub(crate) fn is_sorted(&self) -> bool {
        self.sorted
    }

    /// A `sorted-map-by` map's comparator (`None` for every other value).
    pub(crate) fn comparator(&self) -> Option<&Value> {
        self.comparator.as_ref()
    }

    /// Engine leaves cannot call a custom comparator; the task runtime does.
    fn custom_order(&self, what: &'static str) -> Result<(), CollectionError> {
        match self.comparator {
            Some(_) => Err(CollectionError::Unsupported(what)),
            None => Ok(()),
        }
    }

    pub(crate) fn kind(&self) -> FormKind {
        match self.body {
            FormBody::Keyword(_) => FormKind::Keyword,
            FormBody::Symbol(_) => FormKind::Symbol,
            FormBody::List(_) => FormKind::List,
            FormBody::Vector(_) => FormKind::Vector,
            FormBody::Map(_) => FormKind::Map,
            FormBody::Set(_) => FormKind::Set,
            FormBody::Struct(_) => FormKind::Struct,
            FormBody::Type(_) => FormKind::Type,
            FormBody::Lazy(_) => FormKind::Lazy,
            FormBody::Cons(_) => FormKind::Cons,
            FormBody::Reduced(_) => FormKind::Reduced,
            FormBody::CompilerLiteral { .. } => FormKind::CompilerLiteral,
        }
    }

    /// A regex compiler literal's RegExp state (`crate::regexp::shared`).
    pub(crate) fn regex_cell(&self) -> Option<&Arc<crate::regexp::shared::RegexCell>> {
        match &self.body {
            FormBody::CompilerLiteral { regex, .. } => regex.as_ref(),
            _ => None,
        }
    }

    /// A lazy form's cell.
    pub(crate) fn lazy_cell(&self) -> Option<&Arc<LazyCell>> {
        match &self.body {
            FormBody::Lazy(cell) => Some(cell),
            _ => None,
        }
    }

    /// A record's body (`None` for every other value, deftypes included).
    pub(crate) fn record(&self) -> Option<&StructBody> {
        match &self.body {
            FormBody::Struct(s) if s.info.record => Some(s),
            _ => None,
        }
    }

    pub(crate) fn body(&self) -> &FormBody {
        &self.body
    }

    pub(crate) fn sequential(&self) -> bool {
        self.kind().sequential()
            || matches!(&self.body, FormBody::CompilerLiteral { tag, .. } if tag == "queue")
    }

    pub(crate) fn is_map_entry(&self) -> bool {
        self.map_entry
    }

    pub(crate) fn is_list(&self) -> bool {
        self.sequence_view.is_none()
            && (matches!(&self.body, FormBody::List(_))
                || matches!(&self.body, FormBody::Cons(c) if c.kind == lazy::ConsKind::Cons))
    }

    pub(crate) fn sequence_counted(&self) -> bool {
        self.sequence_view.unwrap_or_else(|| {
            matches!(&self.body, FormBody::List(_) | FormBody::Vector(_))
                || matches!(&self.body, FormBody::Cons(c) if c.kind == lazy::ConsKind::ChunkedSeq)
        })
    }

    pub(crate) fn meta(&self) -> Option<&Value> {
        self.meta.as_ref()
    }

    /// The child values that take part in equality and hashing (elements,
    /// or keys and values; not metadata).
    fn for_each_child(&self, mut f: impl FnMut(&Value)) {
        match &self.body {
            FormBody::Keyword(_) | FormBody::Symbol(_) => {}
            FormBody::List(items) | FormBody::Vector(items) => items.for_each(f),
            FormBody::Map(map) => map.for_each(|k, v| {
                f(k);
                f(v);
            }),
            FormBody::Set(set) => set.for_each(|k, _| f(k)),
            // A deftype's hash is its identity's; a record's reads its
            // field values and extension entries.
            FormBody::Type(_) | FormBody::Reduced(_) => {}
            FormBody::CompilerLiteral { tag, payload, .. } => {
                if tag == "queue" {
                    f(payload);
                }
            }
            FormBody::Struct(s) => {
                if s.info.record {
                    s.fields.for_each(&mut f);
                    if let Some(ext) = &s.ext {
                        f(ext);
                    }
                }
            }
            // Read through [`sequence_elements`], which needs the heap.
            FormBody::Lazy(_) | FormBody::Cons(_) => {}
        }
    }
}

impl Trace for FormObject {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        match &self.body {
            FormBody::Keyword(_) | FormBody::Symbol(_) | FormBody::Type(_) => {}
            FormBody::List(items) | FormBody::Vector(items) => items.trace(visit),
            FormBody::Map(map) | FormBody::Set(map) => map.trace(visit),
            FormBody::Struct(s) => {
                s.ty.trace(visit);
                s.fields.trace(visit);
                if let Some(ext) = &s.ext {
                    ext.trace(visit);
                }
            }
            FormBody::Lazy(cell) => cell.trace(visit),
            FormBody::Cons(c) => c.trace(visit),
            FormBody::Reduced(value) => value.trace(visit),
            FormBody::CompilerLiteral {
                payload, scalar, ..
            } => {
                payload.trace(visit);
                if let Some(scalar) = scalar {
                    scalar.trace(visit);
                }
            }
        }
        if let Some(meta) = &self.meta {
            meta.trace(visit);
        }
        if let Some(comparator) = &self.comparator {
            comparator.trace(visit);
        }
    }
}

/// What a value resolves to for form operations.
enum Resolved {
    Form(ObjectId, Arc<FormObject>),
    Occurrence(Value),
    Other,
}

fn read(heap: &Heap, v: &Value) -> Resolved {
    let Value::Object(id) = *v else {
        return Resolved::Other;
    };
    heap.read(id, |object| match object {
        HeapObject::Form(f) => Resolved::Form(id, f.clone()),
        HeapObject::Occurrence(o) => Resolved::Occurrence(o.value.clone()),
        _ => Resolved::Other,
    })
    .unwrap_or(Resolved::Other)
}

/// The form object `v` is (occurrences are not transparent here).
pub(crate) fn form_object(heap: &Heap, v: &Value) -> Option<Arc<FormObject>> {
    match read(heap, v) {
        Resolved::Form(_, f) => Some(f),
        _ => None,
    }
}

/// `v` with occurrences unwrapped, and its form object if it is one.
fn resolve(heap: &Heap, mut v: Value) -> (Value, Option<(ObjectId, Arc<FormObject>)>) {
    loop {
        match read(heap, &v) {
            Resolved::Form(id, f) => return (v, Some((id, f))),
            Resolved::Occurrence(inner) => v = inner,
            Resolved::Other => return (v, None),
        }
    }
}

/// The value an occurrence wraps (through nested occurrences), or `v`.
pub(crate) fn unwrap(heap: &Heap, v: Value) -> Value {
    let mut v = v;
    while let Resolved::Occurrence(inner) = read(heap, &v) {
        v = inner;
    }
    v
}

/// The diagnostic kind name of `v`.
fn found(heap: &Heap, v: &Value) -> &'static str {
    match *v {
        Value::Object(id) => heap.read(id, object_name).unwrap_or("reclaimed object"),
        ref other => other.type_name(),
    }
}

fn wrong(heap: &Heap, expected: &'static str, v: &Value) -> CollectionError {
    CollectionError::WrongType {
        expected,
        found: found(heap, v),
    }
}

/// Allocate a form. `parts` are the values it was derived from, for the
/// known-shareable bit (`Heap::alloc_persistent`).
fn alloc(heap: &Heap, object: FormObject, parts: &[Value]) -> Result<Value, CollectionError> {
    heap.alloc_persistent(HeapObject::Form(Arc::new(object)), Some(parts))
        .map(Value::Object)
        .ok_or(CollectionError::OutOfMemory)
}

/// The trie hash of a value hash: a bijective spread (splitmix64), so
/// keys collide in the trie exactly when their value hashes are equal.
fn trie_hash(hash: i32) -> u64 {
    let mut z = u64::from(hash as u32) ^ 0x666f_726d_0000_0000;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

fn map_assoc(
    heap: &Heap,
    map: &FormMap,
    key: Value,
    value: Value,
) -> Result<Option<FormMap>, CollectionError> {
    if let FormMap::Positional(pairs) = map {
        for i in (0..pairs.len()).step_by(2) {
            if try_equals(heap, &key, &pairs.get(i).unwrap())? {
                return if identical(&value, &pairs.get(i + 1).unwrap()) {
                    Ok(None)
                } else {
                    Ok(Some(FormMap::Positional(pairs.try_set(
                        heap,
                        i + 1,
                        value,
                    )?)))
                };
            }
        }
        if map.len() < 8 {
            return Ok(Some(FormMap::Positional(
                pairs.try_push(heap, key)?.try_push(heap, value)?,
            )));
        }
        // Appending to a full array map promotes its existing entries first,
        // merging equal keys with last-value wins, then adds the new entry.
        let mut promoted = FormMap::default();
        for (k, v) in map.to_vec().into_iter().chain([(key, value)]) {
            if let Some(updated) = map_assoc(heap, &promoted, k, v)? {
                promoted = updated;
            }
        }
        return Ok(Some(promoted));
    }
    let FormMap::Hashed(map) = map else {
        unreachable!()
    };
    let h = trie_hash(hash(heap, &key));
    map.try_assoc_with(heap, h, key, value, &|stored, query| {
        try_equals(heap, query, stored)
    })
    .map(|m| m.map(FormMap::Hashed))
}

fn map_get(heap: &Heap, map: &FormMap, key: &Value) -> Result<Option<Value>, CollectionError> {
    match map {
        FormMap::Hashed(map) => {
            let h = trie_hash(hash(heap, key));
            map.get_with(h, key, &|stored, query| try_equals(heap, query, stored))
        }
        FormMap::Positional(pairs) => {
            for i in (0..pairs.len()).step_by(2) {
                if try_equals(heap, key, &pairs.get(i).unwrap())? {
                    return Ok(pairs.get(i + 1));
                }
            }
            Ok(None)
        }
    }
}

fn map_dissoc(heap: &Heap, map: &FormMap, key: &Value) -> Result<Option<FormMap>, CollectionError> {
    match map {
        FormMap::Hashed(map) => map
            .try_dissoc_with(heap, trie_hash(hash(heap, key)), key, &|stored, query| {
                try_equals(heap, query, stored)
            })
            .map(|m| m.map(FormMap::Hashed)),
        FormMap::Positional(_) => {
            if map_get(heap, map, key)?.is_none() {
                return Ok(None);
            }
            let mut retained = Vec::new();
            for (k, v) in map.to_vec() {
                if !try_equals(heap, key, &k)? {
                    retained.extend([k, v]);
                }
            }
            // CLJS reserves space for exactly one removed entry but copies
            // every unequal key. Equal lowered keys therefore leave holes.
            retained.resize((map.len() - 1) * 2, Value::Undefined);
            Ok(Some(FormMap::Positional(PVector::try_from_values(
                heap, &retained,
            )?)))
        }
    }
}

fn compare_error(heap: &Heap, a: &Value, b: &Value) -> CollectionError {
    let named = |v| {
        matches!(
            form_object(heap, v).as_deref().map(FormObject::body),
            Some(FormBody::Keyword(_) | FormBody::Symbol(_))
        )
    };
    if !matches!(a, Value::Number(_)) && (named(a) || named(b)) {
        CollectionError::CannotCompare("Cannot compare different types".into())
    } else {
        CollectionError::CannotCompareValues(super::ComparisonOperands(a.clone(), b.clone()))
    }
}

fn compare_keys(heap: &Heap, a: &Value, b: &Value) -> Result<std::cmp::Ordering, CollectionError> {
    use std::cmp::Ordering;
    struct Vector {
        left: PVector,
        right: PVector,
        index: usize,
        pair: (ObjectId, ObjectId),
    }
    let mut pending: Vec<Vector> = Vec::new();
    let mut pending_charge = GrowthCharge::default();
    let mut completed = HashSet::new();
    let mut completed_charge = GrowthCharge::default();
    let mut next = (a.clone(), b.clone());
    loop {
        let (a, b) = next;
        let (a, b) = (unwrap(heap, a), unwrap(heap, b));
        let order = if identical(&a, &b) {
            Ordering::Equal
        } else {
            match (&a, &b) {
                (Value::Null | Value::Undefined, _) => Ordering::Less,
                (_, Value::Null | Value::Undefined) => Ordering::Greater,
                (Value::Number(a), Value::Number(b)) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
                (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
                (Value::String(a), Value::String(b)) => a.code_units().cmp(b.code_units()),
                _ => {
                    let (left, right) = (form_object(heap, &a), form_object(heap, &b));
                    match (
                        left.as_deref().map(FormObject::body),
                        right.as_deref().map(FormObject::body),
                    ) {
                        (Some(FormBody::Keyword(a)), Some(FormBody::Keyword(b)))
                        | (Some(FormBody::Symbol(a)), Some(FormBody::Symbol(b))) => a
                            .namespace
                            .as_ref()
                            .map(JsString::code_units)
                            .cmp(&b.namespace.as_ref().map(JsString::code_units))
                            .then_with(|| a.name.code_units().cmp(b.name.code_units())),
                        (Some(FormBody::Vector(left)), Some(FormBody::Vector(right))) => {
                            let order = left.len().cmp(&right.len());
                            if order == Ordering::Equal {
                                let (Value::Object(a), Value::Object(b)) = (&a, &b) else {
                                    unreachable!("vector identities");
                                };
                                let pair = (*a, *b);
                                if !completed.contains(&pair) {
                                    pending_charge
                                        .admit(heap, pending.len(), std::mem::size_of::<Vector>())
                                        .map_err(|_| CollectionError::HeapLimit)?;
                                    if pending.len() == pending.capacity() {
                                        pending
                                            .try_reserve_exact(pending.len().max(8))
                                            .map_err(|_| CollectionError::OutOfMemory)?;
                                    }
                                    pending.push(Vector {
                                        left: left.clone(),
                                        right: right.clone(),
                                        index: 0,
                                        pair,
                                    });
                                }
                            }
                            order
                        }
                        (Some(FormBody::Keyword(_) | FormBody::Symbol(_)), _)
                        | (_, Some(FormBody::Keyword(_) | FormBody::Symbol(_))) => {
                            return Err(compare_error(heap, &a, &b));
                        }
                        _ => return Err(compare_error(heap, &a, &b)),
                    }
                }
            }
        };
        if order != Ordering::Equal {
            return Ok(order);
        }
        loop {
            let Some(frame) = pending.last_mut() else {
                return Ok(Ordering::Equal);
            };
            if frame.index < frame.left.len() {
                next = (
                    frame.left.get(frame.index).unwrap(),
                    frame.right.get(frame.index).unwrap(),
                );
                frame.index += 1;
                break;
            }
            let pair = pending.pop().unwrap().pair;
            // Only completed equal pairs may be reused: an earlier child
            // must still determine order or raise its comparison failure.
            // Immutable vectors are acyclic, so a pair cannot be active twice.
            // Include hash-table control bytes and spare buckets in admission.
            completed_charge
                .admit(
                    heap,
                    completed.len(),
                    4 * std::mem::size_of::<(ObjectId, ObjectId)>() + 64,
                )
                .map_err(|_| CollectionError::HeapLimit)?;
            completed
                .try_reserve(1)
                .map_err(|_| CollectionError::OutOfMemory)?;
            completed.insert(pair);
        }
    }
}

fn sorted_position(
    heap: &Heap,
    pairs: &PVector,
    key: &Value,
) -> Result<(usize, bool), CollectionError> {
    let (mut lo, mut hi) = (0, pairs.len() / 2);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if compare_keys(heap, &pairs.get(mid * 2).unwrap(), key)?.is_lt() {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    Ok((
        lo * 2,
        lo * 2 < pairs.len() && compare_keys(heap, &pairs.get(lo * 2).unwrap(), key)?.is_eq(),
    ))
}

fn sorted_assoc(
    heap: &Heap,
    map: &FormMap,
    key: Value,
    value: Value,
) -> Result<Option<FormMap>, CollectionError> {
    let FormMap::Positional(pairs) = map else {
        unreachable!("sorted map storage")
    };
    let (i, exists) = sorted_position(heap, pairs, &key)?;
    if exists {
        return if try_equals(heap, &pairs.get(i + 1).unwrap(), &value)? {
            Ok(None)
        } else {
            Ok(Some(FormMap::Positional(pairs.try_set(
                heap,
                i + 1,
                value,
            )?)))
        };
    }
    let mut items = pairs.to_vec();
    items.splice(i..i, [key, value]);
    Ok(Some(FormMap::Positional(PVector::try_from_values(
        heap, &items,
    )?)))
}

/// A default-comparator sorted map. Odd trailing operands have nil values,
/// matching the legacy constructor; compare-equal keys retain the first key.
pub(crate) fn sorted_map_from_sequence(
    heap: &Heap,
    items: &Value,
) -> Result<Value, CollectionError> {
    let values = sequential(heap, items)?;
    let mut map = FormMap::Positional(PVector::default());
    for i in (0..values.len()).step_by(2) {
        if let Some(updated) = sorted_assoc(
            heap,
            &map,
            values.get(i).unwrap(),
            values.get(i + 1).unwrap_or(Value::Null),
        )? {
            map = updated;
        }
    }
    let mut object = FormObject::new(FormBody::Map(map), None);
    object.sorted = true;
    alloc(heap, object, std::slice::from_ref(items))
}

// ---- sorted-map-by ----
//
// A `sorted-map-by` map keeps its comparator as an edge and its entries in
// sorted positional storage. The task runtime orders keys by calling the
// comparator on its VM (so a call can demand a helper and park); these
// leaves only read or edit an entry at a position the runtime computed.
// Leaves that would compare its keys themselves report `Unsupported`.

const SORTED_BY_LEAF: &str = "native sorted-map-by key lookup outside the task runtime";
const SORTED_BY_EQUALITY: &str = "native nested equality with a sorted-map-by map";

/// The task runtime's answer to one structural comparison of a map (`left`)
/// with a `sorted-map-by` map (`right`), made by its comparator-calling `=`.
/// `equal` is `None` when that `=` threw an `Error` inside a set comparison:
/// as the legacy set `equiv` catches it, the innermost enclosing set
/// comparison is then unequal.
#[derive(Debug, Clone)]
pub(crate) struct SortedEquality {
    pub(crate) left: Value,
    pub(crate) right: Value,
    pub(crate) equal: Option<bool>,
}

impl Trace for SortedEquality {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        self.left.trace(visit);
        self.right.trace(visit);
    }
}

/// One form instruction's answers, retained across its executions (and the
/// task's suspensions) by the frame that obtained them. Their storage is
/// charged to the aggregate heap budget before it grows ([`Self::charge`]).
#[derive(Debug, Default)]
pub(crate) struct SortedAnswers {
    pub(crate) items: Vec<SortedEquality>,
    pub(crate) charge: GrowthCharge,
}

impl Trace for SortedAnswers {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        self.items.iter().for_each(|answer| answer.trace(visit));
    }
}

/// A comparison the task runtime must answer: the left map, the right
/// `sorted-map-by` map, and whether it is met inside a set comparison
/// (where an `Error` its `=` throws makes that set comparison unequal).
#[derive(Debug, Clone)]
pub(crate) struct SortedRequest {
    pub(crate) left: Value,
    pub(crate) right: Value,
    pub(crate) in_set: bool,
}

#[derive(Default)]
struct SortedEqualities {
    /// Answers for the running instruction, from its earlier executions, in
    /// the order it met the comparisons: each occurrence of a comparison
    /// takes its own answer, as each legacy `=` calls the comparator again.
    answers: SortedAnswers,
    /// The next answer to take.
    next: usize,
    /// The first comparison it met without an answer.
    request: Option<SortedRequest>,
}

thread_local! {
    // Like lazy realization requests: set and taken around one instruction
    // (`crate::vm::intrinsics`), which never parks or collects meanwhile.
    static SORTED_EQUALITIES: std::cell::RefCell<SortedEqualities> =
        std::cell::RefCell::new(SortedEqualities::default());
}

/// Make `answers` available to the instruction about to run (replacing any
/// left over).
pub(crate) fn install_sorted_answers(answers: SortedAnswers) {
    SORTED_EQUALITIES.with(|s| {
        let mut s = s.borrow_mut();
        s.answers = answers;
        s.next = 0;
    });
}

/// Take back the answers [`install_sorted_answers`] made available.
pub(crate) fn take_sorted_answers() -> SortedAnswers {
    SORTED_EQUALITIES.with(|s| std::mem::take(&mut s.borrow_mut().answers))
}

/// Take the recorded comparison (left map, right sorted-map-by map) that
/// the task runtime must answer before the instruction can run again.
pub(crate) fn take_sorted_request() -> Option<SortedRequest> {
    SORTED_EQUALITIES.with(|s| s.borrow_mut().request.take())
}

/// The answer for the next comparison met, which an execution meets in the
/// same order as the last (the operands are immutable values; a sequence
/// realized meanwhile only lets it go further).
fn sorted_answer(left: &Value, right: &Value) -> Option<Option<bool>> {
    SORTED_EQUALITIES.with(|s| {
        let mut s = s.borrow_mut();
        let answer = s.answers.items.get(s.next)?;
        if !(answer.left.same_object(left) && answer.right.same_object(right)) {
            // Not the comparison answered here: ask again from this one on.
            let next = s.next;
            s.answers.items.truncate(next);
            return None;
        }
        let equal = answer.equal;
        s.next += 1;
        Some(equal)
    })
}

/// The first unanswered comparison met wins.
fn request_sorted(left: Value, right: Value, in_set: bool) {
    SORTED_EQUALITIES.with(|s| {
        let mut s = s.borrow_mut();
        if s.request.is_none() {
            s.request = Some(SortedRequest {
                left,
                right,
                in_set,
            });
        }
    });
}

/// An empty `sorted-map-by` map ordered by `comparator`. The comparator is
/// not called here; the runtime reports a non-callable one when it first
/// compares two keys, as the legacy runtime does.
pub(crate) fn sorted_map_by(heap: &Heap, comparator: &Value) -> Result<Value, CollectionError> {
    let mut object = FormObject::new(FormBody::Map(FormMap::Positional(PVector::default())), None);
    object.sorted = true;
    object.comparator = Some(comparator.clone());
    alloc(heap, object, std::slice::from_ref(comparator))
}

/// A `sorted-map-by` map's comparator, or null for every other value.
pub(crate) fn sorted_comparator(heap: &Heap, v: &Value) -> Value {
    form_object(heap, v)
        .and_then(|f| f.comparator.clone())
        .unwrap_or(Value::Null)
}

fn sorted_by_pairs(
    heap: &Heap,
    map: &Value,
) -> Result<(Arc<FormObject>, PVector), CollectionError> {
    let f = form_object(heap, map)
        .filter(|f| f.comparator.is_some())
        .ok_or_else(|| wrong(heap, "sorted-map-by map", map))?;
    let FormBody::Map(FormMap::Positional(pairs)) = f.body() else {
        unreachable!("sorted map storage")
    };
    let pairs = pairs.clone();
    Ok((f, pairs))
}

/// An entry position `< bound` of a sorted-by map.
fn sorted_index(index: &Value, bound: usize) -> Result<usize, CollectionError> {
    let index = index_from_value(index)?;
    if index >= bound {
        return Err(CollectionError::IndexOutOfBounds {
            index,
            count: bound,
        });
    }
    Ok(index)
}

/// The key (`part` 0) or value (`part` 1) of a sorted-by map's entry `index`.
pub(crate) fn sorted_entry(
    heap: &Heap,
    map: &Value,
    index: &Value,
    part: &Value,
) -> Result<Value, CollectionError> {
    let (_, pairs) = sorted_by_pairs(heap, map)?;
    let i = sorted_index(index, pairs.len() / 2)?;
    let part = match part {
        Value::Number(n) if *n == 0.0 => 0,
        Value::Number(n) if *n == 1.0 => 1,
        other => return Err(wrong(heap, "entry part 0 or 1", other)),
    };
    Ok(pairs.get(i * 2 + part).unwrap())
}

/// A new entry inserted at position `index` (`<= count`), keeping the
/// comparator and metadata.
pub(crate) fn sorted_insert(
    heap: &Heap,
    map: &Value,
    index: &Value,
    key: &Value,
    value: &Value,
) -> Result<Value, CollectionError> {
    let (f, pairs) = sorted_by_pairs(heap, map)?;
    let i = sorted_index(index, pairs.len() / 2 + 1)?;
    let mut items = pairs.to_vec();
    items.splice(i * 2..i * 2, [key.clone(), value.clone()]);
    let body = FormBody::Map(FormMap::Positional(PVector::try_from_values(heap, &items)?));
    alloc(
        heap,
        f.updated(body),
        &[map.clone(), key.clone(), value.clone()],
    )
}

/// Entry `index` with another value; its key is retained.
pub(crate) fn sorted_replace(
    heap: &Heap,
    map: &Value,
    index: &Value,
    value: &Value,
) -> Result<Value, CollectionError> {
    let (f, pairs) = sorted_by_pairs(heap, map)?;
    let i = sorted_index(index, pairs.len() / 2)?;
    let body = FormBody::Map(FormMap::Positional(pairs.try_set(
        heap,
        i * 2 + 1,
        value.clone(),
    )?));
    alloc(heap, f.updated(body), &[map.clone(), value.clone()])
}

/// The map without entry `index`.
pub(crate) fn sorted_remove(
    heap: &Heap,
    map: &Value,
    index: &Value,
) -> Result<Value, CollectionError> {
    let (f, pairs) = sorted_by_pairs(heap, map)?;
    let i = sorted_index(index, pairs.len() / 2)?;
    let mut items = pairs.to_vec();
    items.drain(i * 2..i * 2 + 2);
    let body = FormBody::Map(FormMap::Positional(PVector::try_from_values(heap, &items)?));
    alloc(heap, f.updated(body), std::slice::from_ref(map))
}

/// A sorted-by map with the same comparator and order over `items`, the
/// alternating keys and values of `map`'s entries after realization.
pub(crate) fn sorted_rebuilt(
    heap: &Heap,
    map: &Value,
    items: &[Value],
) -> Result<Value, CollectionError> {
    let (f, _) = sorted_by_pairs(heap, map)?;
    let body = FormBody::Map(FormMap::Positional(PVector::try_from_values(heap, items)?));
    let mut parts = items.to_vec();
    parts.push(map.clone());
    alloc(heap, f.updated(body), &parts)
}

/// `compare` with the default sorted-map order: -1, 0 or 1, or the
/// default comparison's errors.
pub(crate) fn compare(heap: &Heap, a: &Value, b: &Value) -> Result<Value, CollectionError> {
    Ok(Value::Number(match compare_keys(heap, a, b)? {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => 0.0,
        std::cmp::Ordering::Greater => 1.0,
    }))
}

// ---- Construction ----

/// A fresh `reduced` marker. Allocation and its payload use the ordinary
/// persistent-form budget, tracing and publication rules.
pub(crate) fn new_reduced(heap: &Heap, value: Value) -> Result<Value, CollectionError> {
    let parts = [value.clone()];
    alloc(
        heap,
        FormObject::new(FormBody::Reduced(value), None),
        &parts,
    )
}

/// A new keyword or symbol (`kind` is one of the two); `AmbiguousName`
/// unless its spelling splits back into `namespace` and `name`.
pub(crate) fn new_named(
    heap: &Heap,
    kind: FormKind,
    namespace: Option<JsString>,
    name: JsString,
) -> Result<Value, CollectionError> {
    let namespace = namespace
        .map(|text| text.import(heap).ok_or(CollectionError::HeapLimit))
        .transpose()?;
    let name = name.import(heap).ok_or(CollectionError::HeapLimit)?;
    let name = FormName { namespace, name };
    if !name.splits_back() {
        return Err(CollectionError::AmbiguousName);
    }
    let body = match kind {
        FormKind::Keyword => FormBody::Keyword(name),
        _ => FormBody::Symbol(name),
    };
    alloc(heap, FormObject::new(body, None), &[])
}

/// [`new_named`] from JavaScript operands: a string or `null`/`undefined`
/// namespace and a string name.
pub(crate) fn new_named_from_values(
    heap: &Heap,
    kind: FormKind,
    namespace: Value,
    name: Value,
) -> Result<Value, CollectionError> {
    let namespace = match namespace {
        Value::Null | Value::Undefined => None,
        Value::String(s) => Some(s),
        other => return Err(wrong(heap, "string or nil namespace", &other)),
    };
    let Value::String(name) = name else {
        return Err(wrong(heap, "string name", &name));
    };
    new_named(heap, kind, namespace, name)
}

pub(crate) fn new_list(heap: &Heap, items: &[Value]) -> Result<Value, CollectionError> {
    let body = FormBody::List(PVector::try_from_values(heap, items)?);
    alloc(heap, FormObject::new(body, None), items)
}

/// A nonempty positional sequence, with list equality and hashing but no
/// IList marker. Empty sequence tails use the ordinary EmptyList instead.
pub(crate) fn new_sequence(
    heap: &Heap,
    items: &[Value],
    counted: bool,
) -> Result<Value, CollectionError> {
    let body = FormBody::List(PVector::try_from_values(heap, items)?);
    let mut object = FormObject::new(body, None);
    object.sequence_view = (!items.is_empty()).then_some(counted);
    alloc(heap, object, items)
}

/// A non-IList sequence over an existing positional trie or cons cell. No
/// elements are inspected or realized, and retained edges are unchanged.
pub(crate) fn sequence_view(heap: &Heap, value: &Value) -> Result<Value, CollectionError> {
    let source = unwrap(heap, value.clone());
    let Some(f) = form_object(heap, &source) else {
        return Err(wrong(heap, "positional sequence", &source));
    };
    let body = match f.body() {
        FormBody::List(items) | FormBody::Vector(items) => FormBody::List(items.clone()),
        FormBody::Cons(_) => f.body().clone(),
        _ => return Err(wrong(heap, "positional sequence", &source)),
    };
    let mut object = FormObject::new(body, None);
    object.sequence_view =
        (!matches!(&object.body, FormBody::List(items) if items.len() == 0)).then_some(false);
    alloc(heap, object, std::slice::from_ref(&source))
}

/// A list of `items`, a body taken from the list or vector `source` (or a
/// [`PVector::drop_first`] view of it) that shares its trie: the `seq`,
/// `rest` or `next` of `source`, allocated without copying an element.
/// `items`' elements are some of `source`'s, so `source` stands for them in
/// the new object's known-shareable bit.
pub(crate) fn list_sharing(
    heap: &Heap,
    items: PVector,
    source: &Value,
) -> Result<Value, CollectionError> {
    let sequence_view = (items.len() != 0)
        .then(|| {
            form_object(heap, source).and_then(|f| {
                f.sequence_view
                    .or_else(|| (f.kind() == FormKind::Vector).then_some(true))
            })
        })
        .flatten();
    let body = FormBody::List(items);
    let mut object = FormObject::new(body, None);
    object.sequence_view = sequence_view;
    alloc(heap, object, std::slice::from_ref(source))
}

pub(crate) fn new_vector(heap: &Heap, items: &[Value]) -> Result<Value, CollectionError> {
    let body = FormBody::Vector(PVector::try_from_values(heap, items)?);
    alloc(heap, FormObject::new(body, None), items)
}

/// A map's seq entry: vector equality, hashing and export, but exact
/// indexing when called (the legacy runtime's `MapEntryType`).
pub(crate) fn new_map_entry(
    heap: &Heap,
    key: &Value,
    value: &Value,
) -> Result<Value, CollectionError> {
    let items = [key.clone(), value.clone()];
    let body = FormBody::Vector(PVector::try_from_values(heap, &items)?);
    let mut object = FormObject::new(body, None);
    object.map_entry = true;
    alloc(heap, object, &items)
}

/// A new map; a later duplicate (equal) key replaces an earlier one's
/// value, keeping the earlier key object.
pub(crate) fn new_map(heap: &Heap, entries: &[(Value, Value)]) -> Result<Value, CollectionError> {
    let mut map = FormMap::default();
    for (key, value) in entries {
        if let Some(updated) = map_assoc(heap, &map, key.clone(), value.clone())? {
            map = updated;
        }
    }
    let parts: Vec<Value> = entries
        .iter()
        .flat_map(|(k, v)| [k.clone(), v.clone()])
        .collect();
    alloc(heap, FormObject::new(FormBody::Map(map), None), &parts)
}

/// `set` with `member` added, or `None` when an equal member is present
/// (the stored member is kept, as `conj` of a present member does).
fn set_add(heap: &Heap, set: &FormMap, member: Value) -> Result<Option<FormMap>, CollectionError> {
    if map_get(heap, set, &member)?.is_some() {
        return Ok(None);
    }
    if set.positional() && set.len() >= 8 {
        // Promotion keeps each first stored member, including its metadata
        // and occurrence identity. Map promotion's last value would instead
        // make lookup return a later equal member while seq kept the first.
        let mut promoted = FormMap::default();
        for item in set.to_vec().into_iter().map(|(k, _)| k).chain([member]) {
            if let Some(updated) = set_add(heap, &promoted, item)? {
                promoted = updated;
            }
        }
        return Ok(Some(promoted));
    }
    map_assoc(heap, set, member.clone(), member)
}

/// A new set of `items`; of equal members the first is kept. Up to eight
/// distinct members retain insertion order; the ninth promotes to a hash trie.
pub(crate) fn new_set(heap: &Heap, items: &[Value]) -> Result<Value, CollectionError> {
    let mut set = FormMap::Positional(PVector::default());
    for item in items {
        if let Some(updated) = set_add(heap, &set, item.clone())? {
            set = updated;
        }
    }
    alloc(heap, FormObject::new(FormBody::Set(set), None), items)
}

/// A literal map: every alternating key/value position is retained.
pub(crate) fn new_literal_map(heap: &Heap, pairs: &[Value]) -> Result<Value, CollectionError> {
    if !pairs.len().is_multiple_of(2) {
        return Err(CollectionError::MissingKey);
    }
    let body = FormBody::Map(FormMap::Positional(PVector::try_from_values(heap, pairs)?));
    alloc(heap, FormObject::new(body, None), pairs)
}

/// A literal set: every member position is retained, even when equal.
pub(crate) fn new_literal_set(heap: &Heap, items: &[Value]) -> Result<Value, CollectionError> {
    let pairs: Vec<Value> = items.iter().flat_map(|v| [v.clone(), v.clone()]).collect();
    let body = FormBody::Set(FormMap::Positional(PVector::try_from_values(heap, &pairs)?));
    alloc(heap, FormObject::new(body, None), items)
}

/// An array map of any size: equal keys retain the first key and last value,
/// without promoting. Subsequent `assoc` follows the ordinary size threshold.
pub(crate) fn array_map_from_sequence(
    heap: &Heap,
    items: &Value,
) -> Result<Value, CollectionError> {
    let values = sequential(heap, items)?.to_vec();
    if !values.len().is_multiple_of(2) {
        return Err(CollectionError::MissingKey);
    }
    let mut pairs = PVector::default();
    for [key, value] in values.as_chunks::<2>().0 {
        let mut index = None;
        for i in (0..pairs.len()).step_by(2) {
            if try_equals(heap, key, &pairs.get(i).unwrap())? {
                index = Some(i);
                break;
            }
        }
        pairs = if let Some(i) = index {
            pairs.try_set(heap, i + 1, value.clone())?
        } else {
            pairs
                .try_push(heap, key.clone())?
                .try_push(heap, value.clone())?
        };
    }
    alloc(
        heap,
        FormObject::new(FormBody::Map(FormMap::Positional(pairs)), None),
        &values,
    )
}

/// Literal constructors accepting a list/vector of operands. The runtime's
/// wide array adapters use the same path as inline native literal lowering.
pub(crate) fn literal_from_sequence(
    heap: &Heap,
    items: &Value,
    set: bool,
) -> Result<Value, CollectionError> {
    let values = sequential(heap, items)?;
    if set {
        new_literal_set(heap, &values.to_vec())
    } else {
        if !values.len().is_multiple_of(2) {
            return Err(CollectionError::MissingKey);
        }
        // Alternating map operands are already in a charged immutable trie.
        // Share it rather than copying a counted constructor's payload.
        alloc(
            heap,
            FormObject::new(FormBody::Map(FormMap::Positional(values)), None),
            std::slice::from_ref(items),
        )
    }
}

/// Construct only known explicit descriptors with a correctly shaped payload.
pub(crate) fn new_compiler_literal(
    heap: &Heap,
    tag: &Value,
    payload: &Value,
) -> Result<Value, CollectionError> {
    new_compiler_literal_with_zone(heap, tag, payload, || {
        crate::vm::date::zone(heap, &mut crate::host::NoHost)
    })
}

/// A regex compiler literal (`formRegex(source, flags)`, light-cljs-563)
/// whose payload is the legacy export text of the RegExp the caller has
/// already compiled from `source` and `flags`: `(?flags)source`, without the
/// `v` flag, which that text cannot spell. It matches with exactly `flags`,
/// which may not be global or sticky (`crate::regexp::shared`).
pub(crate) fn new_regex(
    heap: &Heap,
    source: &Value,
    flags: &Value,
) -> Result<Value, CollectionError> {
    let (Value::String(text), Value::String(letters)) = (source, flags) else {
        return Err(wrong(heap, "regex source and flags strings", source));
    };
    let (source, flags) = (text, letters);
    let compiled = crate::regexp::object::compile_accounted(heap, source, flags).map_err(|_| {
        wrong(
            heap,
            "valid regex source and flags",
            &Value::String(source.clone()),
        )
    })?;
    if compiled.flags.global || compiled.flags.sticky {
        return Err(wrong(
            heap,
            "regex flags without g or y",
            &Value::String(flags.clone()),
        ));
    }
    let payload = Value::String(regex_export_text(heap, &compiled)?);
    let cell = crate::regexp::shared::RegexCell::new(
        heap,
        source
            .clone()
            .import(heap)
            .ok_or(CollectionError::HeapLimit)?,
        flags
            .clone()
            .import(heap)
            .ok_or(CollectionError::HeapLimit)?,
        Some(compiled),
    )
    .ok_or(CollectionError::HeapLimit)?;
    regex_form(heap, payload, cell)
}

/// The legacy export text of a compiled RegExp: `(?flags)source`, with the
/// escaped source and the flags but `v`, which that text cannot spell.
fn regex_export_text(
    heap: &Heap,
    compiled: &crate::regexp::object::Compiled,
) -> Result<JsString, CollectionError> {
    let shown = crate::regexp::object::source(heap, &compiled.source)
        .map_err(|_| CollectionError::HeapLimit)?;
    let exported: String = crate::regexp::object::flags(compiled.flags)
        .to_string()
        .chars()
        .filter(|&c| c != 'v')
        .collect();
    if exported.is_empty() {
        return Ok(shown);
    }
    JsString::from(format!("(?{exported})").as_str())
        .concat_accounted(heap, &shown)
        .map_err(|_| CollectionError::HeapLimit)
}

/// Allocate a regex form around its RegExp state.
fn regex_form(
    heap: &Heap,
    payload: Value,
    cell: crate::regexp::shared::RegexCell,
) -> Result<Value, CollectionError> {
    let object = FormObject::new(
        FormBody::CompilerLiteral {
            tag: JsString::fixed("regex"),
            payload: payload.clone(),
            scalar: None,
            regex: Some(Arc::new(cell)),
        },
        None,
    );
    alloc(heap, object, std::slice::from_ref(&payload))
}

/// Resolve the runtime zone only when a textual instant needs local time.
/// Hostless construction selects and freezes the same UTC default as Date.
pub(crate) fn new_compiler_literal_with_zone(
    heap: &Heap,
    tag: &Value,
    payload: &Value,
    resolve_zone: impl FnOnce() -> crate::host::TimeZone,
) -> Result<Value, CollectionError> {
    let Value::String(tag) = tag else {
        return Err(wrong(heap, "compiler literal tag", tag));
    };
    let valid = if tag == "js-array" || tag == "queue" {
        kind(heap, &unwrap(heap, payload.clone())) == Some(FormKind::Vector)
    } else if tag == "js-object" {
        kind(heap, &unwrap(heap, payload.clone())) == Some(FormKind::Map)
    } else if tag == "regex" || tag == "uuid" {
        matches!(unwrap(heap, payload.clone()), Value::String(_))
    } else if tag == "instant" {
        matches!(
            unwrap(heap, payload.clone()),
            Value::String(_) | Value::Number(_)
        )
    } else {
        false
    };
    if !valid {
        return Err(wrong(heap, "valid compiler literal payload", payload));
    }
    let scalar = if tag == "uuid" {
        let Value::String(text) = unwrap(heap, payload.clone()) else {
            unreachable!()
        };
        let text = if text
            .code_units()
            .iter()
            .any(|unit| (65..=90).contains(unit))
        {
            let units: Vec<_> = text
                .code_units()
                .iter()
                .map(|&unit| {
                    if (65..=90).contains(&unit) {
                        unit + 32
                    } else {
                        unit
                    }
                })
                .collect();
            JsString::copy_units(heap, &units)
        } else {
            text.import(heap)
        }
        .ok_or(CollectionError::HeapLimit)?;
        Some(Value::String(text))
    } else if tag == "instant" {
        let time = match unwrap(heap, payload.clone()) {
            Value::Number(time) => crate::vm::date::time_clip(time),
            Value::String(text) => {
                let mut parser = crate::vm::date::Parser::new(heap, text)
                    .map_err(|_| CollectionError::HeapLimit)?;
                loop {
                    if let Some(result) = parser.advance(4096) {
                        break result.map_or(f64::NAN, |date| {
                            date.time(if date.is_local {
                                resolve_zone()
                            } else {
                                crate::host::TimeZone::UTC
                            })
                        });
                    }
                }
            }
            _ => unreachable!(),
        };
        Some(Value::Number(time))
    } else {
        None
    };
    if tag == "regex" {
        let Value::String(text) = unwrap(heap, payload.clone()) else {
            unreachable!()
        };
        let cell = crate::regexp::shared::RegexCell::from_payload(heap, &text)
            .ok_or(CollectionError::HeapLimit)?;
        return regex_form(heap, payload.clone(), cell);
    }
    let tag = tag.clone().import(heap).ok_or(CollectionError::HeapLimit)?;
    alloc(
        heap,
        FormObject::new(
            FormBody::CompilerLiteral {
                tag,
                payload: payload.clone(),
                scalar,
                regex: None,
            },
            None,
        ),
        std::slice::from_ref(payload),
    )
}

pub(crate) fn compiler_literal(
    heap: &Heap,
    value: &Value,
) -> Result<(JsString, Value), CollectionError> {
    match form_object(heap, value).as_deref().map(FormObject::body) {
        Some(FormBody::CompilerLiteral { tag, payload, .. }) => Ok((tag.clone(), payload.clone())),
        _ => Err(wrong(heap, "compiler literal", value)),
    }
}

// ---- Structs (light-cljs-478) ----

/// A new struct type named `name` (a string) with the field names of the
/// form vector or list `fields` (strings), a record when `record` is true,
/// and the fields' JavaScript member names `members` (strings, one per
/// field; `null`/`undefined` for the field names). Each call makes a new
/// identity.
pub(crate) fn new_type(
    heap: &Heap,
    name: &Value,
    fields: &Value,
    record: &Value,
    members: &Value,
) -> Result<Value, CollectionError> {
    let Value::String(name) = name else {
        return Err(wrong(heap, "string type name", name));
    };
    let name = name
        .clone()
        .import(heap)
        .ok_or(CollectionError::HeapLimit)?;
    let mut names = Vec::new();
    for field in sequential(heap, fields)?.to_vec() {
        let Value::String(field) = field else {
            return Err(wrong(heap, "string field name", &field));
        };
        names.push(field.import(heap).ok_or(CollectionError::HeapLimit)?);
    }
    let members = match unwrap(heap, members.clone()) {
        Value::Null | Value::Undefined => names.clone(),
        members => {
            let mut out = Vec::new();
            for member in sequential(heap, &members)?.to_vec() {
                let Value::String(member) = member else {
                    return Err(wrong(heap, "string member name", &member));
                };
                out.push(member.import(heap).ok_or(CollectionError::HeapLimit)?);
            }
            if out.len() != names.len() {
                return Err(wrong(heap, "one member name per field", &members));
            }
            out
        }
    };
    let info = TypeInfo {
        name,
        fields: names.into_boxed_slice(),
        members: members.into_boxed_slice(),
        record: matches!(record, Value::Bool(true)),
    };
    alloc(
        heap,
        FormObject::new(FormBody::Type(Arc::new(info)), None),
        &[],
    )
}

/// The JavaScript own property `name` of the struct `f`: a record's
/// `__meta`, `__extmap` or `__hash` slot (`null` when absent, as a fresh
/// legacy record's are; a legacy constructor assigns the slots after the
/// fields, so a slot overrides a field of the same member name), else the
/// value of the last field whose member name it is.
pub(crate) fn struct_own_property(f: &FormObject, name: &JsString) -> Option<Value> {
    let FormBody::Struct(s) = f.body() else {
        return None;
    };
    if s.info.record {
        if name == &JsString::from("__meta") {
            return Some(f.meta().cloned().unwrap_or(Value::Null));
        } else if name == &JsString::from("__extmap") {
            return Some(s.ext.clone().unwrap_or(Value::Null));
        } else if name == &JsString::from("__hash") {
            return Some(Value::Null);
        }
    }
    s.info
        .members
        .iter()
        .rposition(|member| member.code_units() == name.code_units())
        .map(|i| s.fields.get(i).unwrap_or(Value::Undefined))
}

/// The JavaScript own keys of the struct `f`, in a legacy constructor's
/// assignment order: each distinct member name at its first field's
/// position, then a record's three slots.
pub(crate) fn struct_own_keys(f: &FormObject) -> Vec<JsString> {
    let FormBody::Struct(s) = f.body() else {
        return Vec::new();
    };
    let mut keys: Vec<JsString> = Vec::new();
    for member in s.info.members.iter() {
        if !keys.iter().any(|k| k.code_units() == member.code_units()) {
            keys.push(member.clone());
        }
    }
    if s.info.record {
        for slot in ["__meta", "__extmap", "__hash"].map(JsString::from) {
            if !keys.iter().any(|k| k.code_units() == slot.code_units()) {
                keys.push(slot);
            }
        }
    }
    keys
}

/// The type of a struct type form.
fn type_info(heap: &Heap, ty: &Value) -> Result<Arc<TypeInfo>, CollectionError> {
    match form_object(heap, ty).as_deref().map(FormObject::body) {
        Some(FormBody::Type(info)) => Ok(info.clone()),
        _ => Err(wrong(heap, "struct type", ty)),
    }
}

/// A struct of type `ty` with these field values, in the type's order; a
/// missing value is `undefined`, as a JavaScript constructor's parameter
/// is, and extra values are ignored.
pub(crate) fn new_struct(
    heap: &Heap,
    ty: &Value,
    values: &[Value],
) -> Result<Value, CollectionError> {
    let info = type_info(heap, ty)?;
    let n = info.fields.len();
    let mut fields = values[..values.len().min(n)].to_vec();
    fields.resize(n, Value::Undefined);
    // A record constructor's further arguments are its metadata and
    // extension map (`(R. a b meta ext)`), each a form map or nil.
    let mut meta = None;
    let mut ext = None;
    if info.record {
        let map = |i: usize| -> Result<Option<Value>, CollectionError> {
            let Some(v) = values.get(i) else {
                return Ok(None);
            };
            let v = unwrap(heap, v.clone());
            match form_object(heap, &v).as_deref().map(FormObject::body) {
                Some(FormBody::Map(_)) => Ok(Some(v)),
                None if matches!(v, Value::Null | Value::Undefined) => Ok(None),
                _ => Err(wrong(heap, "form map or nil", &v)),
            }
        };
        meta = map(n)?;
        ext = map(n + 1)?.filter(|m| count(heap, m).expect("validated map") > 0);
    }
    let parts: Vec<Value> = std::iter::once(ty.clone())
        .chain(values.iter().take(n + 2).cloned())
        .collect();
    let body = StructBody {
        ty: ty.clone(),
        info,
        fields: PVector::try_from_values(heap, &fields)?,
        ext,
    };
    alloc(heap, FormObject::new(FormBody::Struct(body), meta), &parts)
}

/// The struct body of `v` (occurrences are not transparent here).
fn struct_body(heap: &Heap, v: &Value) -> Option<(Arc<FormObject>, StructBody)> {
    let f = form_object(heap, v)?;
    match f.body() {
        FormBody::Struct(s) => {
            let s = s.clone();
            Some((f, s))
        }
        _ => None,
    }
}

/// The field named `name` (a string) of struct `s`: a method's field read.
pub(crate) fn struct_field(heap: &Heap, s: &Value, name: &Value) -> Result<Value, CollectionError> {
    let Some((_, body)) = struct_body(heap, s) else {
        return Err(wrong(heap, "struct", s));
    };
    let Value::String(name) = name else {
        return Err(wrong(heap, "string field name", name));
    };
    body.info
        .fields
        .iter()
        .position(|field| field.code_units() == name.code_units())
        .and_then(|i| body.fields.get(i))
        .ok_or(CollectionError::MissingKey)
}

/// Whether `v` is a record (occurrences are not transparent here).
pub(crate) fn is_record(heap: &Heap, v: &Value) -> bool {
    form_object(heap, v).is_some_and(|f| f.record().is_some())
}

/// The type of a struct (through occurrences), or `nil`.
pub(crate) fn type_of(heap: &Heap, v: &Value) -> Value {
    match resolve(heap, v.clone()) {
        (_, Some((_, f))) => match f.body() {
            FormBody::Struct(s) => s.ty.clone(),
            _ => Value::Null,
        },
        _ => Value::Null,
    }
}

/// The index of the record field a key names: an unqualified keyword whose
/// name is the field's (the runtime's `recordKeywordFieldIndex`).
fn field_index(heap: &Heap, info: &TypeInfo, key: &Value) -> Option<usize> {
    let key = unwrap(heap, key.clone());
    let f = form_object(heap, &key)?;
    let FormBody::Keyword(n) = f.body() else {
        return None;
    };
    if n.namespace.is_some() {
        return None;
    }
    info.fields
        .iter()
        .position(|field| field.code_units() == n.name.code_units())
}

/// The field keyword `:name` of a record entry.
fn field_keyword(heap: &Heap, name: &JsString) -> Result<Value, CollectionError> {
    new_named(heap, FormKind::Keyword, None, name.clone())
}

/// A record's entries: its fields under their keywords, then its
/// extension entries (in the extension map's traversal order).
pub(crate) fn record_entries(
    heap: &Heap,
    s: &StructBody,
) -> Result<Vec<(Value, Value)>, CollectionError> {
    let mut entries = Vec::with_capacity(s.info.fields.len());
    for (name, value) in s.info.fields.iter().zip(s.fields.to_vec()) {
        entries.push((field_keyword(heap, name)?, value));
    }
    if let Some(ext) = s.ext_map(heap) {
        entries.extend(ext.to_vec());
    }
    Ok(entries)
}

/// A record's value for `key`: a field by its keyword, else an extension
/// entry by an equal key.
pub(crate) fn record_get(
    heap: &Heap,
    s: &StructBody,
    key: &Value,
) -> Result<Option<Value>, CollectionError> {
    match field_index(heap, &s.info, key) {
        Some(i) => Ok(s.fields.get(i)),
        None => match s.ext_map(heap) {
            Some(ext) => map_get(heap, &ext, key),
            None => Ok(None),
        },
    }
}

/// A record like `f` (same type and metadata) with this body; its hash is
/// computed again.
fn record_like(
    heap: &Heap,
    f: &FormObject,
    s: &StructBody,
    fields: PVector,
    ext: Option<Value>,
    parts: &[Value],
) -> Result<Value, CollectionError> {
    let body = StructBody {
        ty: s.ty.clone(),
        info: s.info.clone(),
        fields,
        ext,
    };
    alloc(heap, f.updated(FormBody::Struct(body)), parts)
}

/// Legacy record extension updates copy even an unchanged map. Its identity
/// matters when positional duplicate keys make distinct copies unequal.
fn copy_map(heap: &Heap, map: &Value) -> Result<Value, CollectionError> {
    let f = form_object(heap, map).ok_or_else(|| wrong(heap, "form map", map))?;
    alloc(
        heap,
        f.updated(FormBody::Map(map_body(heap, map)?)),
        std::slice::from_ref(map),
    )
}

/// `assoc` on a record: a field by its keyword (a new record with that
/// field), else an extension entry. An identical field value returns
/// `record` itself.
pub(crate) fn record_assoc(
    heap: &Heap,
    record: &Value,
    key: Value,
    value: Value,
) -> Result<Value, CollectionError> {
    let Some((f, s)) = struct_body(heap, record).filter(|(_, s)| s.info.record) else {
        return Err(wrong(heap, "record", record));
    };
    let parts = [record.clone(), key.clone(), value.clone()];
    match field_index(heap, &s.info, &key) {
        Some(i) => {
            if s.fields.get(i).is_some_and(|old| identical(&old, &value)) {
                return Ok(record.clone());
            }
            let fields = s.fields.try_set(heap, i, value)?;
            record_like(heap, &f, &s, fields, s.ext.clone(), &parts)
        }
        None => {
            let mut ext = match &s.ext {
                Some(ext) => assoc(heap, ext, key, value)?,
                None => new_literal_map(heap, &[key, value])?,
            };
            if s.ext.as_ref().is_some_and(|old| old.same_object(&ext)) {
                ext = copy_map(heap, &ext)?;
            }
            record_like(heap, &f, &s, s.fields.clone(), Some(ext), &parts)
        }
    }
}

/// `dissoc` on a record: a field key gives a form map of the other entries
/// with the record's metadata (no longer a record); an extension key a
/// record without it; other keys copy the extension map when present.
pub(crate) fn record_dissoc(
    heap: &Heap,
    record: &Value,
    key: &Value,
) -> Result<Value, CollectionError> {
    let Some((f, s)) = struct_body(heap, record).filter(|(_, s)| s.info.record) else {
        return Err(wrong(heap, "record", record));
    };
    if field_index(heap, &s.info, key).is_some() {
        let entries = record_entries(heap, &s)?;
        let map = new_map(heap, &entries)?;
        let map = dissoc(heap, &map, key)?;
        return match f.meta() {
            Some(meta) => with_meta(heap, &map, meta),
            None => Ok(map),
        };
    }
    let Some(ext) = &s.ext else {
        return Ok(record.clone());
    };
    let mut updated = dissoc(heap, ext, key)?;
    if ext.same_object(&updated) {
        updated = copy_map(heap, &updated)?;
    }
    let ext = (count(heap, &updated)? > 0).then_some(updated);
    record_like(
        heap,
        &f,
        &s,
        s.fields.clone(),
        ext,
        std::slice::from_ref(record),
    )
}

/// `map->R`: a record of type `ty` whose fields are `map`'s values for the
/// field keywords and whose extension entries are `map`'s other entries.
pub(crate) fn record_from_map(
    heap: &Heap,
    ty: &Value,
    map: &Value,
) -> Result<Value, CollectionError> {
    let info = type_info(heap, ty)?;
    let source = match resolve(heap, map.clone()) {
        (Value::Null | Value::Undefined, None) => new_map(heap, &[])?,
        (source, Some((_, f))) => match f.body() {
            FormBody::Map(_) => source,
            FormBody::Struct(s) if s.info.record => new_map(heap, &record_entries(heap, s)?)?,
            _ => return Err(wrong(heap, "form map or record", map)),
        },
        _ => return Err(wrong(heap, "form map or record", map)),
    };
    let mut fields = Vec::with_capacity(info.fields.len());
    let mut ext = source.clone();
    for name in &info.fields {
        let key = field_keyword(heap, name)?;
        fields.push(get(heap, &source, &key)?.unwrap_or(Value::Null));
        let updated = dissoc(heap, &ext, &key)?;
        ext = if ext.same_object(&updated) {
            copy_map(heap, &updated)?
        } else {
            updated
        };
    }
    let body = StructBody {
        ty: ty.clone(),
        info,
        fields: PVector::try_from_values(heap, &fields)?,
        ext: (count(heap, &ext)? > 0).then_some(ext),
    };
    alloc(
        heap,
        FormObject::new(FormBody::Struct(body), None),
        &[ty.clone(), map.clone()],
    )
}

// ---- Lazy sequences (light-cljs-479) ----

/// A new lazy sequence realized by calling `thunk`. Never known shareable
/// at allocation: the cell learns it is shared from a publication check
/// ([`crate::collections::lazy`], "Sharing").
pub(crate) fn new_lazy(heap: &Heap, thunk: Value) -> Result<Value, CollectionError> {
    let object = FormObject::new(FormBody::Lazy(Arc::new(LazyCell::new(thunk))), None);
    heap.alloc(HeapObject::Form(Arc::new(object)))
        .map(Value::Object)
        .ok_or(CollectionError::OutOfMemory)
}

/// A new lazy `concat` of the collections in the list `colls`, realized in
/// Rust one collection at a time (light-cljs-479).
pub(crate) fn new_lazy_concat(heap: &Heap, colls: Value) -> Result<Value, CollectionError> {
    let object = FormObject::new(FormBody::Lazy(Arc::new(LazyCell::concat(colls))), None);
    heap.alloc(HeapObject::Form(Arc::new(object)))
        .map(Value::Object)
        .ok_or(CollectionError::OutOfMemory)
}

/// A new sequence cell: `items[start..]` (nonempty) followed by `rest`
/// (`nil`, a list, or a lazy or cons sequence).
pub(crate) fn new_cons(
    heap: &Heap,
    items: PVector,
    start: usize,
    rest: Value,
    kind: lazy::ConsKind,
    range: Option<lazy::RangeCount>,
) -> Result<Value, CollectionError> {
    let body = ConsBody {
        items,
        start,
        rest,
        kind,
        range,
    };
    let mut parts = body.elements();
    parts.push(body.rest.clone());
    alloc(heap, FormObject::new(FormBody::Cons(body), None), &parts)
}

/// [`new_cons`] of elements and a tail that all belong to `source` (a
/// vector, or a cons sequence it continues): no element is copied, and the
/// new sequence takes its known-shareable bit from `source`, which reaches
/// everything it does.
pub(crate) fn cons_sharing(
    heap: &Heap,
    items: PVector,
    start: usize,
    rest: Value,
    kind: lazy::ConsKind,
    source: &Value,
) -> Result<Value, CollectionError> {
    let source_form = resolve(heap, source.clone()).1.map(|(_, f)| f);
    let range = source_form.as_ref().and_then(|f| match f.body() {
        FormBody::Cons(c) => c.range,
        _ => None,
    });
    let body = ConsBody {
        items,
        start,
        rest,
        kind,
        range,
    };
    let mut object = FormObject::new(FormBody::Cons(body), None);
    object.sequence_view = source_form.and_then(|f| f.sequence_view);
    alloc(heap, object, std::slice::from_ref(source))
}

/// Why a sequence could not be read through.
#[derive(Debug)]
pub(crate) enum SeqError {
    /// This lazy form must be realized first.
    Pending(Value),
    /// The realized sequence contains itself: it is infinite.
    Cycle,
}

/// `v` with occurrences unwrapped and realized lazy forms followed to what
/// they realized. `visited` holds the cells this walk followed already.
pub(crate) fn realized_head(
    heap: &Heap,
    v: Value,
    visited: &mut lazy::Visited,
) -> Result<Value, SeqError> {
    realized_head_with(heap, v, visited, false, false)
}

/// Follow lazy cells for sequence operations, retaining the terminal value's
/// occurrence. Returning an operand sequence must preserve its exact identity.
pub(crate) fn sequence_head(
    heap: &Heap,
    v: Value,
    visited: &mut lazy::Visited,
) -> Result<Value, SeqError> {
    realized_head_with(heap, v, visited, false, true)
}

/// Follow lazy cells for language protocol dispatch, retaining record
/// receivers that structural traversal sees as their built-in sequences.
pub(crate) fn protocol_head(
    heap: &Heap,
    v: Value,
    visited: &mut lazy::Visited,
) -> Result<Value, SeqError> {
    realized_head_with(heap, v, visited, true, false)
}

fn realized_head_with(
    heap: &Heap,
    v: Value,
    visited: &mut lazy::Visited,
    protocols: bool,
    occurrence: bool,
) -> Result<Value, SeqError> {
    let mut v = v;
    loop {
        let (u, f) = resolve(heap, v.clone());
        let Some(cell) = f.as_ref().and_then(|(_, f)| f.lazy_cell()) else {
            return Ok(if occurrence { v } else { u });
        };
        if !visited.enter(cell) {
            return Err(SeqError::Cycle);
        }
        let value = if protocols {
            cell.protocol_value()
        } else {
            cell.realized().cloned()
        };
        match value {
            Some(r) => v = r,
            None => return Err(SeqError::Pending(u)),
        }
    }
}

/// The elements of the sequence `v`, realized: `nil`, a list or vector, or
/// a lazy or cons sequence whose tails are those kinds (realization
/// canonicalizes the others to lists). `Ok(None)` when `v`, or a tail, is
/// something else.
pub(crate) fn sequence_elements(heap: &Heap, v: &Value) -> Result<Option<Vec<Value>>, SeqError> {
    let mut out = Vec::new();
    let mut visited = lazy::Visited::default();
    let mut cur = v.clone();
    loop {
        let head = realized_head(heap, cur, &mut visited)?;
        if matches!(head, Value::Null | Value::Undefined) {
            return Ok(Some(out));
        }
        let Some(f) = form_object(heap, &head) else {
            return Ok(None);
        };
        match &f.body {
            FormBody::List(items) | FormBody::Vector(items) => {
                out.extend(items.to_vec());
                return Ok(Some(out));
            }
            FormBody::Cons(c) => {
                out.extend(c.elements());
                cur = c.rest.clone();
            }
            _ => return Ok(None),
        }
    }
}

/// The length of a native sequence, without copying elements or following a
/// counted range's lazy tail. Pending cells use the ordinary realization path.
pub(crate) fn sequence_count(heap: &Heap, v: &Value) -> Result<Option<f64>, SeqError> {
    let mut count = 0.0;
    let mut visited = lazy::Visited::default();
    let mut cur = v.clone();
    loop {
        let head = realized_head(heap, cur, &mut visited)?;
        if matches!(head, Value::Null | Value::Undefined) {
            return Ok(Some(count));
        }
        let Some(f) = form_object(heap, &head) else {
            return Ok(None);
        };
        match &f.body {
            FormBody::List(items) | FormBody::Vector(items) => {
                return Ok(Some(count + items.len() as f64));
            }
            FormBody::Cons(c) => {
                if let Some(n) = c.count() {
                    return Ok(Some(count + n));
                }
                count += c.len() as f64;
                cur = c.rest.clone();
            }
            _ => return Ok(None),
        }
    }
}

/// The elements a lazy or cons form `v` (object `f`) hashes and compares
/// by; records a realization request when one is pending.
fn lazy_elements(heap: &Heap, v: &Value, f: &FormObject) -> Option<Vec<Value>> {
    if !matches!(f.body, FormBody::Lazy(_) | FormBody::Cons(_)) {
        return None;
    }
    lazy_elements_any(heap, v, f)
}

/// [`lazy_elements`] of any sequential form (a list's or vector's own).
fn lazy_elements_any(heap: &Heap, v: &Value, f: &FormObject) -> Option<Vec<Value>> {
    if let FormBody::List(items) | FormBody::Vector(items) = &f.body {
        return Some(items.to_vec());
    }
    match sequence_elements(heap, v) {
        Ok(items) => items,
        Err(SeqError::Pending(p)) => {
            lazy::request(p);
            None
        }
        Err(SeqError::Cycle) => None,
    }
}

// ---- Inspection ----

pub(crate) fn kind(heap: &Heap, v: &Value) -> Option<FormKind> {
    form_object(heap, v).map(|f| f.kind())
}

pub(crate) fn name(heap: &Heap, v: &Value) -> Result<FormName, CollectionError> {
    match form_object(heap, v).as_deref().map(FormObject::body) {
        Some(FormBody::Keyword(n) | FormBody::Symbol(n)) => Ok(n.clone()),
        Some(FormBody::CompilerLiteral { tag, .. }) => Ok(FormName {
            namespace: None,
            name: tag.clone(),
        }),
        // A struct type's qualified name, as one unqualified name.
        Some(FormBody::Type(info)) => Ok(FormName {
            namespace: None,
            name: info.name.clone(),
        }),
        _ => Err(wrong(heap, "keyword or symbol", v)),
    }
}

pub(crate) fn count(heap: &Heap, v: &Value) -> Result<usize, CollectionError> {
    match form_object(heap, v).as_deref().map(FormObject::body) {
        Some(FormBody::List(items) | FormBody::Vector(items)) => Ok(items.len()),
        Some(FormBody::Map(map) | FormBody::Set(map)) => Ok(map.len()),
        _ => Err(wrong(heap, "list, form vector, form map or form set", v)),
    }
}

fn sequential(heap: &Heap, v: &Value) -> Result<PVector, CollectionError> {
    match form_object(heap, v).as_deref().map(FormObject::body) {
        Some(FormBody::List(items) | FormBody::Vector(items)) => Ok(items.clone()),
        _ => Err(wrong(heap, "list or form vector", v)),
    }
}

fn map_body(heap: &Heap, v: &Value) -> Result<FormMap, CollectionError> {
    match form_object(heap, v).as_deref().map(FormObject::body) {
        Some(FormBody::Map(map)) => Ok(map.clone()),
        _ => Err(wrong(heap, "form map", v)),
    }
}

/// The element of a list or vector at `index`.
pub(crate) fn nth(heap: &Heap, coll: &Value, index: usize) -> Result<Value, CollectionError> {
    let items = sequential(heap, coll)?;
    items.get(index).ok_or(CollectionError::IndexOutOfBounds {
        index,
        count: items.len(),
    })
}

/// [`nth`] at an index value (the vector index contract).
pub(crate) fn nth_at(heap: &Heap, coll: &Value, index: &Value) -> Result<Value, CollectionError> {
    let items = sequential(heap, coll)?;
    let index = index_from_value(index)?;
    items.get(index).ok_or(CollectionError::IndexOutOfBounds {
        index,
        count: items.len(),
    })
}

/// The elements of a list or vector, or the members of a set: source order
/// for positional literals, deterministic trie order for ordinary hashed sets,
/// and legacy hash traversal for runtime reader sets.
pub(crate) fn items(heap: &Heap, coll: &Value) -> Result<Vec<Value>, CollectionError> {
    if let Some(f) = form_object(heap, coll)
        && let FormBody::Set(set) = f.body()
    {
        let mut entries = set.to_vec();
        if f.reader_traversal {
            legacy_order(heap, set, &mut entries);
        }
        return Ok(entries.into_iter().map(|(k, _)| k).collect());
    }
    match sequential(heap, coll) {
        Ok(items) => Ok(items.to_vec()),
        Err(_) => Err(wrong(heap, "list, form vector or form set", coll)),
    }
}

/// Map entries in source order for positional literals, deterministic trie
/// order for ordinary hashed maps, and legacy hash traversal for reader maps.
pub(crate) fn entries(heap: &Heap, map: &Value) -> Result<Vec<(Value, Value)>, CollectionError> {
    let body = map_body(heap, map)?;
    let mut entries = body.to_vec();
    if form_object(heap, map).is_some_and(|f| f.reader_traversal) {
        legacy_order(heap, &body, &mut entries);
    }
    Ok(entries)
}

/// Select reader traversal without converting a hash collection to an array
/// literal. Metadata and child identities stay intact; updates preserve the flag.
pub(crate) fn reader_collection(heap: &Heap, value: &Value) -> Result<Value, CollectionError> {
    let f = form_object(heap, value).ok_or_else(|| wrong(heap, "form map or set", value))?;
    if !matches!(f.body(), FormBody::Map(_) | FormBody::Set(_)) {
        return Err(wrong(heap, "form map or set", value));
    }
    if f.reader_traversal {
        return Ok(value.clone());
    }
    let mut object = f.with_meta(f.meta.clone());
    object.reader_traversal = true;
    alloc(heap, object, std::slice::from_ref(value))
}

fn legacy_order(heap: &Heap, map: &FormMap, entries: &mut [(Value, Value)]) {
    if !map.positional() {
        entries.sort_by_cached_key(|(key, _)| {
            let hash = hash(heap, key) as u32;
            let nil = matches!(unwrap(heap, key.clone()), Value::Null | Value::Undefined);
            // Five-bit trie paths compare from the low bits, not numerically.
            (
                !nil,
                std::array::from_fn::<_, 7, _>(|i| (hash >> (i * 5)) & 31),
            )
        });
    }
}

/// The native lookup trie spreads hashes for its own routing. Printing uses
/// ClojureScript's five-bit hash paths instead, with nil before the trie.
/// This does not change language lookup or ordinary native seq traversal.
pub(crate) fn print_items(heap: &Heap, value: &Value) -> Result<Value, CollectionError> {
    let wrong = || CollectionError::WrongType {
        expected: "form map, set or record",
        found: kind(heap, value).map_or(value.type_name(), FormKind::name),
    };
    let object = form_object(heap, value).ok_or_else(wrong)?;
    if let FormBody::Struct(s) = object.body()
        && s.info.record
    {
        let mut entries = Vec::with_capacity(s.fields.len());
        for (name, value) in s.info.fields.iter().zip(s.fields.to_vec()) {
            let key = new_named(heap, FormKind::Keyword, None, name.clone())?;
            entries.push(new_vector(heap, &[key, value])?);
        }
        if let Some(ext) = &s.ext {
            entries.extend(items(heap, &print_items(heap, ext)?)?);
        }
        return new_vector(heap, &entries);
    }
    let (map, set) = match object.body() {
        FormBody::Map(map) => (map, false),
        FormBody::Set(set) => (set, true),
        _ => return Err(wrong()),
    };
    let mut entries = map.to_vec();
    legacy_order(heap, map, &mut entries);
    let values = entries
        .into_iter()
        .map(|(key, value)| {
            if set {
                Ok(key)
            } else {
                new_vector(heap, &[key, value])
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    new_vector(heap, &values)
}

/// Lookup: a map entry by an equal key, or a vector element by index
/// (`None` when absent, out of bounds or not an index, as `get` does).
pub(crate) fn get(
    heap: &Heap,
    coll: &Value,
    key: &Value,
) -> Result<Option<Value>, CollectionError> {
    if let Some(f) = form_object(heap, coll)
        && f.sorted
        && let FormBody::Map(FormMap::Positional(pairs)) = f.body()
    {
        f.custom_order(SORTED_BY_LEAF)?;
        let (i, exists) = sorted_position(heap, pairs, key)?;
        return Ok(exists.then(|| pairs.get(i + 1).unwrap()));
    }
    match form_object(heap, coll).as_deref().map(FormObject::body) {
        Some(FormBody::Map(map) | FormBody::Set(map)) => map_get(heap, map, key),
        Some(FormBody::Vector(items)) => Ok(index_from_value(key).ok().and_then(|i| items.get(i))),
        _ => Err(wrong(heap, "form vector, form map or form set", coll)),
    }
}

/// A form's metadata (`None` for no metadata and for every other value).
pub(crate) fn meta(heap: &Heap, v: &Value) -> Option<Value> {
    form_object(heap, v).and_then(|f| f.meta.clone())
}

// ---- Updates ----

/// `assoc`: a map entry (by an equal key), or a vector index `<= count`
/// (`count` appends). Metadata is kept.
pub(crate) fn assoc(
    heap: &Heap,
    coll: &Value,
    key: Value,
    value: Value,
) -> Result<Value, CollectionError> {
    let Some(f) = form_object(heap, coll) else {
        return Err(wrong(heap, "form vector or form map", coll));
    };
    let parts = [coll.clone(), key.clone(), value.clone()];
    match &f.body {
        FormBody::Map(map) => match if f.sorted {
            f.custom_order(SORTED_BY_LEAF)?;
            sorted_assoc(heap, map, key, value)?
        } else {
            map_assoc(heap, map, key, value)?
        } {
            Some(updated) => alloc(heap, f.updated(FormBody::Map(updated)), &parts),
            None => Ok(coll.clone()),
        },
        FormBody::Vector(items) => {
            let index = index_from_value(&key)?;
            let count = items.len();
            let updated = if index == count {
                items.try_push(heap, value)?
            } else {
                let old = items
                    .get(index)
                    .ok_or(CollectionError::IndexOutOfBounds { index, count })?;
                if !f.is_map_entry() && identical(&old, &value) {
                    return Ok(coll.clone());
                }
                items.try_set(heap, index, value)?
            };
            alloc(heap, f.updated(FormBody::Vector(updated)), &parts)
        }
        _ => Err(wrong(heap, "form vector or form map", coll)),
    }
}

/// `dissoc` of a map entry by an equal key (absent: the map itself).
pub(crate) fn dissoc(heap: &Heap, map: &Value, key: &Value) -> Result<Value, CollectionError> {
    let Some(f) = form_object(heap, map) else {
        return Err(wrong(heap, "form map", map));
    };
    let FormBody::Map(body) = &f.body else {
        return Err(wrong(heap, "form map", map));
    };
    if f.sorted {
        f.custom_order(SORTED_BY_LEAF)?;
        let FormMap::Positional(pairs) = body else {
            unreachable!("sorted map storage")
        };
        let (i, exists) = sorted_position(heap, pairs, key)?;
        if !exists {
            return Ok(map.clone());
        }
        let mut items = pairs.to_vec();
        items.drain(i..i + 2);
        return alloc(
            heap,
            f.updated(FormBody::Map(FormMap::Positional(
                PVector::try_from_values(heap, &items)?,
            ))),
            std::slice::from_ref(map),
        );
    }
    match map_dissoc(heap, body, key)? {
        Some(updated) => alloc(
            heap,
            f.updated(FormBody::Map(updated)),
            std::slice::from_ref(map),
        ),
        None => Ok(map.clone()),
    }
}

/// `conj`: a vector appends, a list prepends (linear in its length), a set
/// adds a member (present: the set itself).
pub(crate) fn conj(heap: &Heap, coll: &Value, value: Value) -> Result<Value, CollectionError> {
    let expected = "list, form vector or form set";
    let Some(f) = form_object(heap, coll) else {
        return Err(wrong(heap, expected, coll));
    };
    let parts = [coll.clone(), value.clone()];
    let body = match &f.body {
        FormBody::Vector(items) => FormBody::Vector(items.try_push(heap, value)?),
        FormBody::List(items) => FormBody::List(items.try_prepend(heap, value)?),
        FormBody::Set(set) => match set_add(heap, set, value)? {
            Some(updated) => FormBody::Set(updated),
            None => return Ok(coll.clone()),
        },
        _ => return Err(wrong(heap, expected, coll)),
    };
    alloc(heap, f.updated(body), &parts)
}

/// `pop` of a form vector: the vector without its last element, keeping its
/// metadata. An empty vector is out of bounds.
pub(crate) fn vector_pop(heap: &Heap, coll: &Value) -> Result<Value, CollectionError> {
    let Some(f) = form_object(heap, coll) else {
        return Err(wrong(heap, "form vector", coll));
    };
    let FormBody::Vector(items) = &f.body else {
        return Err(wrong(heap, "form vector", coll));
    };
    let body = FormBody::Vector(items.try_pop(heap)?);
    alloc(heap, f.updated(body), std::slice::from_ref(coll))
}

/// `pop` of a queue literal: the queue without its first element, keeping
/// its metadata. An empty queue is itself. The vector payload may be wrapped
/// in occurrences (an imported queue); the popped payload is a bare vector.
pub(crate) fn queue_pop(heap: &Heap, coll: &Value) -> Result<Value, CollectionError> {
    let expected = "queue literal";
    let Some(f) = form_object(heap, coll) else {
        return Err(wrong(heap, expected, coll));
    };
    let FormBody::CompilerLiteral {
        tag,
        payload,
        scalar,
        ..
    } = &f.body
    else {
        return Err(wrong(heap, expected, coll));
    };
    // Imported and compiler-literal payloads keep their occurrence wrapper.
    let vector = unwrap(heap, payload.clone());
    let Some(FormBody::Vector(items)) = form_object(heap, &vector).map(|p| p.body.clone()) else {
        return Err(wrong(heap, expected, coll));
    };
    if items.len() == 0 {
        return Ok(coll.clone());
    }
    let rest = PVector::try_from_values(heap, &items.to_vec()[1..])?;
    let rest = alloc(
        heap,
        FormObject::new(FormBody::Vector(rest), None),
        std::slice::from_ref(payload),
    )?;
    let body = FormBody::CompilerLiteral {
        tag: tag.clone(),
        payload: rest.clone(),
        scalar: scalar.clone(),
        regex: None,
    };
    alloc(heap, f.updated(body), &[coll.clone(), rest])
}

/// `disj` of a set member by an equal value (absent: the set itself).
pub(crate) fn disj(heap: &Heap, set: &Value, member: &Value) -> Result<Value, CollectionError> {
    let Some(f) = form_object(heap, set) else {
        return Err(wrong(heap, "form set", set));
    };
    let FormBody::Set(body) = &f.body else {
        return Err(wrong(heap, "form set", set));
    };
    match map_dissoc(heap, body, member)? {
        Some(updated) => alloc(
            heap,
            f.updated(FormBody::Set(updated)),
            std::slice::from_ref(set),
        ),
        None => Ok(set.clone()),
    }
}

/// `with-meta`: `v` (a symbol, list, vector or map) with metadata `meta`
/// (`nil` or a form map). The same metadata object returns `v` itself.
pub(crate) fn with_meta(heap: &Heap, v: &Value, meta: &Value) -> Result<Value, CollectionError> {
    let expected = "symbol, list, form vector, form map, form set or record";
    let f = match form_object(heap, v) {
        Some(f)
            if !matches!(
                f.kind(),
                FormKind::Keyword | FormKind::Type | FormKind::Reduced
            ) && (f.kind() != FormKind::Struct || f.record().is_some()) =>
        {
            f
        }
        _ => return Err(wrong(heap, expected, v)),
    };
    let meta = match meta {
        Value::Null | Value::Undefined => None,
        m if kind(heap, m) == Some(FormKind::Map) => Some(m.clone()),
        other => return Err(wrong(heap, "form map or nil", other)),
    };
    let same = match (&f.meta, &meta) {
        (None, None) => true,
        (Some(a), Some(b)) => a.same_object(b),
        _ => false,
    };
    if same && !f.is_map_entry() {
        return Ok(v.clone());
    }
    let parts: Vec<Value> = std::iter::once(v.clone()).chain(meta.clone()).collect();
    alloc(heap, f.with_meta(meta), &parts)
}

/// A new copy of the collection `v` holding `meta`: a form map, or an
/// explicit `nil` (`null` or `undefined`), which `with_meta` never stores.
/// Their distinction remains observable through metadata and identity.
/// A realized lazy sequence keeps it (light-cljs-479): the light-cljs runtime's `LazySeq`
/// has metadata, `nil` when none was given, which the compiler bridge
/// exports, so the native bridge exports it too.
pub(crate) fn with_exported_meta(
    heap: &Heap,
    v: &Value,
    meta: Value,
) -> Result<Value, CollectionError> {
    let Some(f) = form_object(heap, v) else {
        return Err(wrong(heap, "list", v));
    };
    match &meta {
        Value::Null | Value::Undefined => {}
        m if kind(heap, m) == Some(FormKind::Map) => {}
        other => return Err(wrong(heap, "form map or nil", other)),
    }
    alloc(heap, f.with_meta(Some(meta.clone())), &[v.clone(), meta])
}

// ---- Equality ----

/// Equality of two non-form values (occurrences already unwrapped).
fn scalar_equals(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null | Value::Undefined, Value::Null | Value::Undefined) => true,
        _ => a.strict_equals(b),
    }
}

/// A position in a sequence for [`equals`]: the elements of the segment
/// being read (from `index`), then `tail`.
struct SeqCursor {
    items: Option<PVector>,
    index: usize,
    tail: Value,
    /// The form whose elements `items` are.
    source: Option<ObjectId>,
}

/// Where a [`SeqCursor`] is: its segment's form and index.
type SeqState = (Option<ObjectId>, usize);

/// What [`seq_split`] found.
enum Split {
    Empty,
    Item(Value, SeqCursor),
    /// A realization is pending (requested), or the value is not a
    /// sequence or contains itself.
    Stop,
}

impl SeqCursor {
    fn new(v: Value) -> SeqCursor {
        SeqCursor {
            items: None,
            index: 0,
            tail: v,
            source: None,
        }
    }

    fn state(&self) -> SeqState {
        (self.source, self.index)
    }
}

/// The next element of a sequence and the position after it, reading one
/// segment (a list's or vector's elements, a cons sequence's) at a time and
/// realized lazy sequences transparently. An unrealized one is requested.
fn seq_split(heap: &Heap, mut c: SeqCursor) -> Split {
    loop {
        if let Some(items) = &c.items
            && let Some(x) = items.get(c.index)
        {
            c.index += 1;
            return Split::Item(x, c);
        }
        let tail = std::mem::replace(&mut c.tail, Value::Null);
        let head = match realized_head(heap, tail, &mut lazy::Visited::default()) {
            Ok(head) => head,
            Err(SeqError::Pending(p)) => {
                lazy::request(p);
                return Split::Stop;
            }
            Err(SeqError::Cycle) => return Split::Stop,
        };
        if matches!(head, Value::Null | Value::Undefined) {
            return Split::Empty;
        }
        let Value::Object(id) = head else {
            return Split::Stop;
        };
        let Some(f) = form_object(heap, &head) else {
            return Split::Stop;
        };
        c = match &f.body {
            FormBody::List(items) | FormBody::Vector(items) => SeqCursor {
                items: Some(items.clone()),
                index: 0,
                tail: Value::Null,
                source: Some(id),
            },
            FormBody::Cons(cons) => SeqCursor {
                items: Some(cons.items.clone()),
                index: cons.start,
                tail: cons.rest.clone(),
                source: Some(id),
            },
            FormBody::CompilerLiteral { tag, payload, .. } if tag == "queue" => {
                c.tail = payload.clone();
                continue;
            }
            _ => return Split::Stop,
        };
    }
}

/// Value equality. See the module docs.
///
/// An explicit-stack goal machine: no recursion on the native stack, so
/// depth is bounded only by memory. A pair of forms expands into an
/// *all-of* goal over its children: element pairs, or a first-match lookup
/// for each map entry or set member, in traversal order. Hashed lookups
/// consider only the stored full hash's candidates. Positional lookups
/// share the destination's persistent operand vector and advance an index,
/// without hashing the query or copying the candidate entries.
///
/// The result of every expanded pair of form identities is recorded,
/// equal or unequal, and reused when the pair is met again (persistent
/// values are acyclic, so a pair never recurs inside its own expansion).
/// Each distinct pair is therefore expanded at most once: the work is
/// linear in the distinct object pairs compared plus their children, where
/// an entry whose full hash `c` keys of the other map share contributes up
/// to `c` candidate key/value pairs instead of one.
pub(crate) fn equals(heap: &Heap, a: &Value, b: &Value) -> bool {
    // The embedding API is boolean; language equality and key operations use
    // the fallible variant so an incompatible sorted lookup raises an Error.
    // No task runtime answers a sorted-map-by comparison made here.
    let equal = try_equals(heap, a, b);
    let _ = take_sorted_request();
    equal.unwrap_or(false)
}

/// Equality for language operations, preserving sorted lookup failures.
pub(crate) fn try_equals(heap: &Heap, a: &Value, b: &Value) -> Result<bool, CollectionError> {
    /// One thing to establish inside an all-of goal.
    enum Step {
        Pair(Value, Value),
        /// Lookup stops at the first equal key, even when its value differs.
        /// A source trie already knows the query's hash; a positional source
        /// computes it only when this step runs and the destination needs it.
        Lookup(Value, Option<Value>, bool, FormMap, Option<u64>, bool),
        /// The rest of two sequences, compared element by element so that
        /// an unequal element ends the comparison before a tail is
        /// realized (light-cljs-479).
        Seq(SeqCursor, SeqCursor, Positions),
    }
    /// The positions one sequence comparison has passed: met again, the
    /// sequences cycle and are equal so far. Per comparison, so a failed
    /// alternative leaves nothing behind.
    type Positions = std::rc::Rc<std::cell::RefCell<HashSet<(SeqState, SeqState)>>>;
    enum Candidates {
        Positional(PVector, usize),
        Hashed(std::vec::IntoIter<(Value, Value)>),
    }
    impl Candidates {
        fn next(&mut self) -> Option<(Value, Value)> {
            match self {
                Self::Positional(pairs, index) => {
                    let key = pairs.get(*index)?;
                    let value = pairs.get(*index + 1).unwrap();
                    *index += 2;
                    Some((key, value))
                }
                Self::Hashed(entries) => entries.next(),
            }
        }
    }
    enum Frame {
        /// Every step must hold; `memo` is the form pair it decides.
        /// `answered` counts the task runtime's answers taken before it: a
        /// pair decided with one is not reused, since each legacy `=` with
        /// a sorted-map-by map calls its comparator again. `set` marks a
        /// set comparison, the boundary of a failed answer.
        All {
            memo: Option<(ObjectId, ObjectId)>,
            pending: Vec<Step>,
            answered: usize,
            set: bool,
        },
        Lookup {
            key: Value,
            value: Option<Value>,
            candidates: Candidates,
            found: Option<Value>,
            checking_value: bool,
            reverse_value: bool,
        },
    }
    /// What comparing one pair yields.
    enum Outcome {
        Decided(bool),
        Expand((ObjectId, ObjectId), Vec<Step>),
        /// A set comparison: an `Error` its lookups meet makes it unequal.
        ExpandSet((ObjectId, ObjectId), Vec<Step>),
        /// Decided by the task runtime's answer for a `sorted-map-by` map
        /// (`None`: its `=` threw an `Error` inside a set comparison).
        Answered(Option<bool>),
        /// A map compared with a `sorted-map-by` map the task runtime has
        /// not compared yet.
        Request(Value, Value),
    }
    let map_steps =
        |p: &FormMap, q: &FormMap, values: bool, source_sorted: bool, target_sorted: bool| {
            let mut steps = Vec::with_capacity(p.len());
            let mut entry = |h, k: &Value, v: &Value| {
                steps.push(Step::Lookup(
                    k.clone(),
                    values.then(|| v.clone()),
                    source_sorted || !p.positional(),
                    q.clone(),
                    h,
                    target_sorted,
                ));
            };
            match p {
                FormMap::Hashed(map) => map.for_each_hashed(|h, k, v| entry(Some(h), k, v)),
                FormMap::Positional(_) => p.for_each(|k, v| entry(None, k, v)),
            }
            // All-of goals pop last-first; lookup and value comparison must
            // short-circuit in the source's traversal order.
            steps.reverse();
            steps
        };
    let mut equal: HashSet<(ObjectId, ObjectId)> = HashSet::new();
    let mut unequal: HashSet<(ObjectId, ObjectId)> = HashSet::new();
    // Pairs being expanded: met again inside their own expansion (only
    // through a lazy sequence that contains itself), they are taken as
    // equal, so the comparison ends.
    let mut active: HashSet<(ObjectId, ObjectId)> = HashSet::new();
    let compare = |a: Value,
                   b: Value,
                   equal: &HashSet<_>,
                   unequal: &HashSet<_>,
                   active: &HashSet<_>| {
        // `identical?` first: also an occurrence of `NaN` equals itself.
        if a.same_object(&b) {
            return Outcome::Decided(true);
        }
        let ((a, fa), (b, fb)) = (resolve(heap, a), resolve(heap, b));
        let ((ia, x), (ib, y)) = match (fa, fb) {
            (Some(x), Some(y)) => (x, y),
            (None, None) => return Outcome::Decided(scalar_equals(&a, &b)),
            _ => return Outcome::Decided(false),
        };
        if ia == ib || equal.contains(&(ia, ib)) || active.contains(&(ia, ib)) {
            return Outcome::Decided(true);
        }
        if unequal.contains(&(ia, ib)) {
            return Outcome::Decided(false);
        }
        // Positional duplicate sets retain the legacy directional membership
        // comparison. It can succeed despite different hashes, also when
        // nested, so cached hashes cannot reject an equality comparison.
        match (&x.body, &y.body) {
            (
                FormBody::CompilerLiteral {
                    tag: m,
                    scalar: Some(p),
                    ..
                },
                FormBody::CompilerLiteral {
                    tag: n,
                    scalar: Some(q),
                    ..
                },
            ) if (m == "uuid" || m == "instant") && m == n => {
                Outcome::Expand((ia, ib), vec![Step::Pair(p.clone(), q.clone())])
            }
            (FormBody::CompilerLiteral { tag, .. }, _)
            | (_, FormBody::CompilerLiteral { tag, .. })
                if tag == "queue" && x.sequential() && y.sequential() =>
            {
                Outcome::Expand(
                    (ia, ib),
                    vec![Step::Seq(
                        SeqCursor::new(a),
                        SeqCursor::new(b),
                        Positions::default(),
                    )],
                )
            }
            (FormBody::Keyword(m), FormBody::Keyword(n))
            | (FormBody::Symbol(m), FormBody::Symbol(n)) => Outcome::Decided(m.same(n)),
            // A lazy or cons sequence: its realized elements, sequentially
            // (light-cljs-479). An unrealized one is a realization request
            // and, until realized, equal only to itself.
            (FormBody::Lazy(_) | FormBody::Cons(_), _)
            | (_, FormBody::Lazy(_) | FormBody::Cons(_))
                if x.sequential() && y.sequential() =>
            {
                Outcome::Expand(
                    (ia, ib),
                    vec![Step::Seq(
                        SeqCursor::new(a),
                        SeqCursor::new(b),
                        Positions::default(),
                    )],
                )
            }
            (FormBody::List(p) | FormBody::Vector(p), FormBody::List(q) | FormBody::Vector(q)) => {
                if p.len() != q.len() {
                    return Outcome::Decided(false);
                }
                let steps = p
                    .to_vec()
                    .into_iter()
                    .zip(q.to_vec())
                    .rev()
                    .map(|(a, b)| Step::Pair(a, b))
                    .collect();
                Outcome::Expand((ia, ib), steps)
            }
            (FormBody::Set(p), FormBody::Set(q)) => {
                if p.len() != q.len() {
                    return Outcome::Decided(false);
                }
                Outcome::ExpandSet((ia, ib), map_steps(p, q, false, false, false))
            }
            (FormBody::Map(p), FormBody::Map(q)) => {
                if p.len() != q.len() {
                    return Outcome::Decided(false);
                }
                // A sorted-map-by destination: the task runtime's `=`, which
                // calls its comparator, decides this pair (light-cljs-561).
                if y.comparator.is_some() {
                    return match sorted_answer(&a, &b) {
                        Some(equal) => Outcome::Answered(equal),
                        None => Outcome::Request(a, b),
                    };
                }
                Outcome::Expand((ia, ib), map_steps(p, q, true, x.sorted, y.sorted))
            }
            // Records of the same type with equal fields and extension
            // entries; a deftype only to itself (decided above).
            (FormBody::Struct(p), FormBody::Struct(q)) => {
                if !(p.info.record && q.info.record && p.ty.same_object(&q.ty)) {
                    return Outcome::Decided(false);
                }
                let mut steps = vec![Step::Pair(
                    p.ext.clone().unwrap_or(Value::Null),
                    q.ext.clone().unwrap_or(Value::Null),
                )];
                steps.extend(
                    p.fields
                        .to_vec()
                        .into_iter()
                        .zip(q.fields.to_vec())
                        .rev()
                        .map(|(a, b)| Step::Pair(a, b)),
                );
                Outcome::Expand((ia, ib), steps)
            }
            _ => Outcome::Decided(false),
        }
    };
    let mut answered = 0;
    let mut frames = vec![Frame::All {
        memo: None,
        pending: vec![Step::Pair(a.clone(), b.clone())],
        answered,
        set: false,
    }];
    // The result of the frame popped last, delivered to the one below it.
    let mut returned: Option<bool> = None;
    while let Some(frame) = frames.last_mut() {
        // A failed answer: unwind to the innermost set comparison.
        let mut failed_answer = false;
        match frame {
            Frame::All {
                memo,
                pending,
                answered: before,
                ..
            } => {
                let failed = returned.take() == Some(false);
                let step = if failed { None } else { pending.pop() };
                match step {
                    // Done: every step held, or one failed.
                    None => {
                        let result = !failed;
                        if let Some(pair) = *memo {
                            active.remove(&pair);
                            if answered == *before {
                                if result { &mut equal } else { &mut unequal }.insert(pair);
                            }
                        }
                        frames.pop();
                        returned = Some(result);
                    }
                    Some(Step::Pair(a, b)) => match compare(a, b, &equal, &unequal, &active) {
                        Outcome::Decided(true) => {}
                        Outcome::Decided(false) => returned = Some(false),
                        Outcome::Answered(Some(equal)) => {
                            answered += 1;
                            if !equal {
                                returned = Some(false);
                            }
                        }
                        Outcome::Answered(None) => {
                            answered += 1;
                            failed_answer = true;
                        }
                        Outcome::Request(a, b) => {
                            let in_set = frames
                                .iter()
                                .any(|f| matches!(f, Frame::All { set: true, .. }));
                            request_sorted(a, b, in_set);
                            return Err(CollectionError::Unsupported(SORTED_BY_EQUALITY));
                        }
                        Outcome::Expand(pair, steps) => {
                            active.insert(pair);
                            frames.push(Frame::All {
                                memo: Some(pair),
                                pending: steps,
                                answered,
                                set: false,
                            });
                        }
                        Outcome::ExpandSet(pair, steps) => {
                            active.insert(pair);
                            frames.push(Frame::All {
                                memo: Some(pair),
                                pending: steps,
                                answered,
                                set: true,
                            });
                        }
                    },
                    Some(Step::Lookup(key, value, reverse_value, target, h, sorted)) => {
                        if sorted {
                            let FormMap::Positional(pairs) = target else {
                                unreachable!("sorted map storage")
                            };
                            let (i, exists) = sorted_position(heap, &pairs, &key)?;
                            if !exists {
                                returned = Some(false);
                            } else if let Some(value) = value {
                                let other = pairs.get(i + 1).unwrap();
                                pending.push(if reverse_value {
                                    Step::Pair(other, value)
                                } else {
                                    Step::Pair(value, other)
                                });
                            }
                            continue;
                        }
                        let candidates = match target {
                            FormMap::Positional(pairs) => Candidates::Positional(pairs, 0),
                            FormMap::Hashed(map) => Candidates::Hashed(
                                map.entries_with_hash(
                                    h.unwrap_or_else(|| trie_hash(hash(heap, &key))),
                                )
                                .into_iter(),
                            ),
                        };
                        frames.push(Frame::Lookup {
                            key,
                            value,
                            candidates,
                            found: None,
                            checking_value: false,
                            reverse_value,
                        })
                    }
                    Some(Step::Seq(p, q, seen)) => match (seq_split(heap, p), seq_split(heap, q)) {
                        (Split::Empty, Split::Empty) => {}
                        (Split::Item(x, p), Split::Item(y, q)) => {
                            // The same positions again: a cycle, equal so far.
                            if seen.borrow_mut().insert((p.state(), q.state())) {
                                pending.push(Step::Seq(p, q, seen));
                            }
                            pending.push(Step::Pair(x, y));
                        }
                        _ => returned = Some(false),
                    },
                }
            }
            Frame::Lookup {
                key,
                value,
                candidates,
                found,
                checking_value,
                reverse_value,
            } => {
                if *checking_value {
                    frames.pop();
                    // The value comparison's result is delivered unchanged.
                } else if returned.take() == Some(true) {
                    if let Some(value) = value {
                        let other = found.take().unwrap();
                        let value = value.clone();
                        let pair = if *reverse_value {
                            Step::Pair(other, value)
                        } else {
                            Step::Pair(value, other)
                        };
                        *checking_value = true;
                        frames.push(Frame::All {
                            memo: None,
                            pending: vec![pair],
                            answered,
                            set: false,
                        });
                    } else {
                        frames.pop();
                        returned = Some(true);
                    }
                } else if let Some((other, candidate_value)) = candidates.next() {
                    *found = Some(candidate_value);
                    let key = key.clone();
                    frames.push(Frame::All {
                        memo: None,
                        pending: vec![Step::Pair(key, other)],
                        answered,
                        set: false,
                    });
                } else {
                    frames.pop();
                    returned = Some(false);
                }
            }
        }
        if failed_answer {
            // The set comparison (memoized by no one, as an answer was
            // taken inside it) is unequal; the frames above it end with it.
            loop {
                match frames.pop() {
                    Some(Frame::All { memo, set, .. }) => {
                        if let Some(pair) = memo {
                            active.remove(&pair);
                        }
                        if set {
                            break;
                        }
                    }
                    Some(Frame::Lookup { .. }) => {}
                    None => return Err(CollectionError::Unsupported(SORTED_BY_EQUALITY)),
                }
            }
            returned = Some(false);
        }
    }
    Ok(returned.unwrap_or(true))
}

// ---- Hashing (the light-cljs runtime's Murmur3 profile) ----

fn mix_k1(v: i32) -> i32 {
    v.wrapping_mul(0xcc9e_2d51_u32 as i32)
        .rotate_left(15)
        .wrapping_mul(0x1b87_3593)
}

fn mix_h1(h: i32, v: i32) -> i32 {
    (h ^ v)
        .rotate_left(13)
        .wrapping_mul(5)
        .wrapping_add(0xe654_6b64_u32 as i32)
}

fn fmix(h: i32, length: i32) -> i32 {
    let mut h = h ^ length;
    h = (h ^ ((h as u32) >> 16) as i32).wrapping_mul(0x85eb_ca6b_u32 as i32);
    h = (h ^ ((h as u32) >> 13) as i32).wrapping_mul(0xc2b2_ae35_u32 as i32);
    h ^ ((h as u32) >> 16) as i32
}

fn hash_int(v: i32) -> i32 {
    if v == 0 {
        0
    } else {
        fmix(mix_h1(0, mix_k1(v)), 4)
    }
}

/// `stringHash`: Java's `31 * h + c`, wrapping (equal to the runtime's
/// result after the ToInt32 every use of it applies).
fn string_hash(units: &[u16]) -> i32 {
    units
        .iter()
        .fold(0i32, |h, &c| h.wrapping_mul(31).wrapping_add(i32::from(c)))
}

fn hash_chars(units: &[u16]) -> i32 {
    let (pairs, rest) = units.as_chunks::<2>();
    let mut h = pairs.iter().fold(0, |h, &[a, b]| {
        mix_h1(h, mix_k1((u32::from(a) | (u32::from(b) << 16)) as i32))
    });
    if let [last] = rest {
        h ^= mix_k1(i32::from(*last));
    }
    fmix(h, (units.len() as i32).wrapping_mul(2))
}

fn hash_named(name: &FormName) -> i32 {
    let seed = hash_chars(name.name.code_units());
    let ns = name
        .namespace
        .as_ref()
        .map_or(0, |n| string_hash(n.code_units()));
    seed ^ ns
        .wrapping_add(0x9e37_79b9_u32 as i32)
        .wrapping_add(seed << 6)
        .wrapping_add(seed >> 2)
}

fn mix_collection_hash(basis: i32, count: usize) -> i32 {
    fmix(mix_h1(0, mix_k1(basis)), count as i32)
}

fn number_hash(n: f64) -> i32 {
    const SAFE: f64 = 9_007_199_254_740_991.0;
    if n.is_nan() {
        2_146_959_360
    } else if n == f64::INFINITY {
        2_146_435_072
    } else if n == f64::NEG_INFINITY {
        -1_048_576
    } else if n.fract() == 0.0 && n.abs() <= SAFE {
        // In (-2^31 + 1, 2^31 - 1): exact as i32; `-0` hashes as `0`.
        (n % 2_147_483_647.0) as i32
    } else {
        // Little-endian Float64 bytes read as two big-endian int32s.
        let b = n.to_le_bytes();
        i32::from_be_bytes([b[0], b[1], b[2], b[3]]) ^ i32::from_be_bytes([b[4], b[5], b[6], b[7]])
    }
}

/// Hash of a non-form value (occurrences already unwrapped).
fn scalar_hash(v: &Value) -> i32 {
    match v {
        Value::Null | Value::Undefined => 0,
        Value::Bool(true) => 1231,
        Value::Bool(false) => 1237,
        Value::Number(n) => number_hash(*n),
        Value::String(s) => hash_int(string_hash(s.code_units())),
        // Outside the supported values: a private hash consistent with `===`.
        other => {
            let h = super::map::key_hash(other);
            (h ^ (h >> 32)) as u32 as i32
        }
    }
}

/// Hash of a child whose form (if any) already has its hash cached.
fn child_hash(heap: &Heap, v: &Value) -> i32 {
    match resolve(heap, v.clone()) {
        (u, Some((id, f))) => match f.hash.get().copied().or_else(|| identity_hash(&f, id)) {
            Some(h) => h,
            // Past a pending realization or a cycle the hash is a
            // placeholder: do not walk further.
            None if lazy::has_request() => 0,
            None => form_hash(heap, &u, &f),
        },
        (v, None) => scalar_hash(&v),
    }
}

/// The hash of one form `v` (object `f`) from its children's (all cached
/// by now, unless a realization is pending).
fn shallow_hash(heap: &Heap, v: &Value, f: &FormObject) -> i32 {
    match &f.body {
        // A lazy or cons sequence hashes as a list of its realized elements
        // (light-cljs-479); while a realization is pending the result is a
        // placeholder that is not cached.
        FormBody::Lazy(_) | FormBody::Cons(_) => match lazy_elements(heap, v, f) {
            Some(items) => {
                let mut h = 1i32;
                for item in &items {
                    h = h.wrapping_mul(31).wrapping_add(child_hash(heap, item));
                }
                mix_collection_hash(h, items.len())
            }
            None => 0,
        },
        FormBody::Keyword(n) => hash_named(n).wrapping_add(0x9e37_79b9_u32 as i32),
        FormBody::Symbol(n) => hash_named(n),
        FormBody::List(items) | FormBody::Vector(items) => {
            let mut h = 1i32;
            items.for_each(|v| h = h.wrapping_mul(31).wrapping_add(child_hash(heap, v)));
            mix_collection_hash(h, items.len())
        }
        FormBody::Map(map) => {
            let mut sum = 0i32;
            map.for_each(|k, v| {
                let entry = 31i32
                    .wrapping_mul(31i32.wrapping_add(child_hash(heap, k)))
                    .wrapping_add(child_hash(heap, v));
                sum = sum.wrapping_add(mix_collection_hash(entry, 2));
            });
            mix_collection_hash(sum, map.len())
        }
        // `hashUnorderedColl` of the members.
        FormBody::Set(set) => {
            let mut sum = 0i32;
            set.for_each(|k, _| sum = sum.wrapping_add(child_hash(heap, k)));
            mix_collection_hash(sum, set.len())
        }
        // A record: each entry's key hash xor its value hash, summed, xor
        // the `stringHash` of the type's name (the runtime's record
        // `IHash`). Field keys are unqualified keywords of the field names.
        FormBody::Struct(s) if s.info.record => {
            let mut sum = 0i32;
            for (name, value) in s.info.fields.iter().zip(s.fields.to_vec()) {
                let key = hash_named(&FormName {
                    namespace: None,
                    name: name.clone(),
                })
                .wrapping_add(0x9e37_79b9_u32 as i32);
                sum = sum.wrapping_add(key ^ child_hash(heap, &value));
            }
            if let Some(ext) = s.ext_map(heap) {
                ext.for_each(|k, v| {
                    sum = sum.wrapping_add(child_hash(heap, k) ^ child_hash(heap, v))
                });
            }
            sum ^ string_hash(s.info.name.code_units())
        }
        FormBody::CompilerLiteral { tag, payload, .. } if tag == "queue" => {
            child_hash(heap, payload)
        }
        FormBody::CompilerLiteral {
            tag,
            scalar: Some(Value::Number(time)),
            ..
        } if tag == "instant" => crate::vm::dispatch::to_int32(*time),
        FormBody::CompilerLiteral {
            tag,
            scalar: Some(value),
            ..
        } if tag == "uuid" => scalar_hash(value),
        // Identity-hashed: set where the object identity is known
        // ([`identity_hash`]); never reached through it.
        FormBody::Struct(_)
        | FormBody::Type(_)
        | FormBody::Reduced(_)
        | FormBody::CompilerLiteral { .. } => 0,
    }
}

/// The hash of a deftype instance or a struct type, whose `=` is identity:
/// a private hash of the object's identity, as for other heap objects.
fn identity_hash(f: &FormObject, id: ObjectId) -> Option<i32> {
    match &f.body {
        FormBody::Struct(s) if s.info.record => None,
        FormBody::CompilerLiteral { tag, .. }
            if tag == "uuid" || tag == "instant" || tag == "queue" =>
        {
            None
        }
        FormBody::Struct(_)
        | FormBody::Type(_)
        | FormBody::Reduced(_)
        | FormBody::CompilerLiteral { .. } => {
            Some(*f.hash.get_or_init(|| scalar_hash(&Value::Object(id))))
        }
        _ => None,
    }
}

/// The cached hash of `root`, computing uncached descendants first with an
/// explicit stack (children before parents), so depth needs no recursion.
fn form_hash(heap: &Heap, root_value: &Value, root: &Arc<FormObject>) -> i32 {
    if let Some(h) = root.hash.get() {
        return *h;
    }
    let mut stack = vec![(root_value.clone(), root.clone(), false)];
    // The root's entry is popped last.
    let mut last = 0;
    // Forms whose children are being hashed: one met again is inside
    // itself, which only a realized lazy sequence can make (light-cljs-479).
    let mut active: HashSet<usize> = HashSet::new();
    let key = |f: &Arc<FormObject>| Arc::as_ptr(f) as usize;
    while let Some((v, f, children_done)) = stack.pop() {
        if let Some(h) = f.hash.get() {
            last = *h;
            continue;
        }
        if children_done {
            active.remove(&key(&f));
            last = shallow_hash(heap, &v, &f);
            // A hash computed past a pending realization is a placeholder.
            if !lazy::has_request() {
                let _ = f.hash.set(last);
            }
            continue;
        }
        stack.push((v.clone(), f.clone(), true));
        active.insert(key(&f));
        let mut push = |c: &Value| {
            if let (u, Some((id, child))) = resolve(heap, c.clone())
                && child.hash.get().is_none()
                && identity_hash(&child, id).is_none()
            {
                if active.contains(&key(&child)) {
                    lazy::note_cycle();
                } else {
                    stack.push((u, child, false));
                }
            }
        };
        match lazy_elements(heap, &v, &f) {
            Some(items) => items.iter().for_each(&mut push),
            None => f.for_each_child(&mut push),
        }
    }
    root.hash.get().copied().unwrap_or(last)
}

/// The value hash: the light-cljs runtime's `hash` for supported values.
pub(crate) fn hash(heap: &Heap, v: &Value) -> i32 {
    match resolve(heap, v.clone()) {
        (u, Some((id, f))) => identity_hash(&f, id).unwrap_or_else(|| form_hash(heap, &u, &f)),
        (v, None) => scalar_hash(&v),
    }
}

/// Run a structural operation of the embedder API: one that hashes or
/// compares keys or members. An unrealized lazy sequence among them has no
/// hash yet (realizing it runs JavaScript, which only a task does), so the
/// operation fails instead of storing or looking up a placeholder.
fn realized_keys<T>(op: impl FnOnce() -> Result<T, CollectionError>) -> Result<T, CollectionError> {
    let _ = lazy::take_request();
    let _ = lazy::take_cycle();
    let result = op();
    let pending = lazy::take_request().is_some();
    if lazy::take_cycle() || pending {
        return Err(CollectionError::WrongType {
            expected: "realized sequence",
            found: "unrealized or self-containing lazy sequence",
        });
    }
    result
}

/// The embedder API. See the module docs.
impl<'s> Scope<'s> {
    /// A new explicit compiler literal. The payload stays rooted and traced.
    /// Local date-time text uses the runtime's selected zone; this hostless
    /// operation selects and freezes UTC if no zone has been selected yet.
    pub fn new_compiler_literal(
        &self,
        tag: JsString,
        payload: Local<'s>,
    ) -> Result<Local<'s>, CollectionError> {
        self.result(new_compiler_literal(
            self.heap(),
            &Value::String(tag),
            &self.local_value(payload),
        ))
    }

    /// The legacy export text (`(?flags)source`, without `v`) of a host
    /// RegExp object, such as a global regex a macro returns
    /// (light-cljs-563); `None` for any other value. Reads only the RegExp's
    /// internal slots, never JavaScript.
    pub fn regexp_export_text(
        &self,
        value: Local<'s>,
    ) -> Result<Option<JsString>, CollectionError> {
        let Value::Object(id) = self.local_value(value) else {
            return Ok(None);
        };
        let compiled = self.heap().read(id, |o| match o {
            HeapObject::Plain(o) => o.regexp().cloned(),
            _ => None,
        });
        match compiled.flatten() {
            Some(compiled) => regex_export_text(self.heap(), &compiled).map(Some),
            None => Ok(None),
        }
    }

    /// The descriptor tag and payload, without invoking JavaScript or user hooks.
    pub fn form_compiler_literal(
        &self,
        value: Local<'s>,
    ) -> Result<(JsString, Local<'s>), CollectionError> {
        let (tag, payload) = compiler_literal(self.heap(), &self.local_value(value))?;
        Ok((tag, self.result(Ok(payload))?))
    }

    /// A new keyword `:namespace/name` (exact code units);
    /// [`CollectionError::AmbiguousName`] unless the spelling splits back
    /// into `namespace` and `name` (module docs, "Names").
    pub fn new_keyword(
        &self,
        namespace: Option<JsString>,
        name: JsString,
    ) -> Result<Local<'s>, CollectionError> {
        self.result(new_named(self.heap(), FormKind::Keyword, namespace, name))
    }

    /// A new symbol `namespace/name` without metadata (exact code units);
    /// [`CollectionError::AmbiguousName`] as for [`Scope::new_keyword`].
    pub fn new_symbol(
        &self,
        namespace: Option<JsString>,
        name: JsString,
    ) -> Result<Local<'s>, CollectionError> {
        self.result(new_named(self.heap(), FormKind::Symbol, namespace, name))
    }

    /// A new list of `items`, in order.
    pub fn new_list(&self, items: &[Local<'s>]) -> Result<Local<'s>, CollectionError> {
        self.result(new_list(self.heap(), &self.values(items)))
    }

    /// A new form vector of `items`, in order.
    pub fn new_form_vector(&self, items: &[Local<'s>]) -> Result<Local<'s>, CollectionError> {
        self.result(new_vector(self.heap(), &self.values(items)))
    }

    /// A new form map of `entries` with structural keys; a later equal key
    /// replaces an earlier one's value.
    pub fn new_form_map(
        &self,
        entries: &[(Local<'s>, Local<'s>)],
    ) -> Result<Local<'s>, CollectionError> {
        let entries: Vec<(Value, Value)> = entries
            .iter()
            .map(|(k, v)| (self.local_value(*k), self.local_value(*v)))
            .collect();
        self.result(realized_keys(|| new_map(self.heap(), &entries)))
    }

    /// A new form set of `items` with structural members; of equal members
    /// the first is kept. Embedder structural keys must already be realized.
    pub fn new_form_set(&self, items: &[Local<'s>]) -> Result<Local<'s>, CollectionError> {
        self.result(realized_keys(|| {
            let values = self.values(items);
            // Small sets no longer hash while inserting. Preserve the
            // embedding API's rejection of unrealized structural keys.
            for value in &values {
                hash(self.heap(), value);
            }
            new_set(self.heap(), &values)
        }))
    }

    /// A positional literal map, preserving equal lowered keys and source
    /// order. Lookup returns the first matching value.
    pub fn new_literal_map(
        &self,
        entries: &[(Local<'s>, Local<'s>)],
    ) -> Result<Local<'s>, CollectionError> {
        let pairs: Vec<Value> = entries
            .iter()
            .flat_map(|(k, v)| [self.local_value(*k), self.local_value(*v)])
            .collect();
        self.result(new_literal_map(self.heap(), &pairs))
    }

    /// A positional literal set, preserving every member and source order.
    pub fn new_literal_set(&self, items: &[Local<'s>]) -> Result<Local<'s>, CollectionError> {
        self.result(new_literal_set(self.heap(), &self.values(items)))
    }

    /// A new set without the member equal to `member`.
    pub fn form_disj(
        &self,
        set: Local<'s>,
        member: Local<'s>,
    ) -> Result<Local<'s>, CollectionError> {
        let (s, m) = (self.local_value(set), self.local_value(member));
        self.result(realized_keys(|| disj(self.heap(), &s, &m)))
    }

    /// The form kind of a value, or `None` (occurrences included).
    pub fn form_kind(&self, v: Local<'s>) -> Option<FormKind> {
        kind(self.heap(), &self.local_value(v))
    }

    /// The namespace and name of a keyword or symbol.
    pub fn form_name(&self, v: Local<'s>) -> Result<FormName, CollectionError> {
        name(self.heap(), &self.local_value(v))
    }

    /// Element count of a list or vector, or entry count of a map.
    pub fn form_count(&self, coll: Local<'s>) -> Result<usize, CollectionError> {
        count(self.heap(), &self.local_value(coll))
    }

    /// The element of a list or vector at `index`.
    pub fn form_nth(&self, coll: Local<'s>, index: usize) -> Result<Local<'s>, CollectionError> {
        self.result(nth(self.heap(), &self.local_value(coll), index))
    }

    /// The elements of a list or vector, or a set's members (source order
    /// for positional literals, deterministic trie order for hashed sets).
    pub fn form_items(&self, coll: Local<'s>) -> Result<Vec<Local<'s>>, CollectionError> {
        Ok(self.new_locals(items(self.heap(), &self.local_value(coll))?))
    }

    /// Map entries in source order for positional literals, deterministic
    /// trie order for hashed maps.
    pub fn form_entries(
        &self,
        map: Local<'s>,
    ) -> Result<Vec<(Local<'s>, Local<'s>)>, CollectionError> {
        let pairs = entries(self.heap(), &self.local_value(map))?;
        let locals = self.new_locals(pairs.into_iter().flat_map(|(k, v)| [k, v]));
        Ok(locals.chunks(2).map(|kv| (kv[0], kv[1])).collect())
    }

    /// Map/record entries (two-element vectors), or set members, in the
    /// legacy printer and compiler-form export order. Lookup and ordinary
    /// native seq traversal keep their existing deterministic trie order.
    pub fn form_print_items(&self, coll: Local<'s>) -> Result<Vec<Local<'s>>, CollectionError> {
        let ordered = print_items(self.heap(), &self.local_value(coll))?;
        Ok(self.new_locals(items(self.heap(), &ordered)?))
    }

    /// A map value by an equal key, or a vector element by index; `None`
    /// when absent.
    pub fn form_get(
        &self,
        coll: Local<'s>,
        key: Local<'s>,
    ) -> Result<Option<Local<'s>>, CollectionError> {
        let found =
            realized_keys(|| get(self.heap(), &self.local_value(coll), &self.local_value(key)))?;
        Ok(found.map(|v| self.new_local(v)))
    }

    /// A new map with `key` set, or a new vector with index `key` set
    /// (`count` appends). Metadata is kept.
    pub fn form_assoc(
        &self,
        coll: Local<'s>,
        key: Local<'s>,
        value: Local<'s>,
    ) -> Result<Local<'s>, CollectionError> {
        let (c, k, v) = (
            self.local_value(coll),
            self.local_value(key),
            self.local_value(value),
        );
        self.result(realized_keys(|| assoc(self.heap(), &c, k, v)))
    }

    /// A new map without the entry for an equal key.
    pub fn form_dissoc(
        &self,
        map: Local<'s>,
        key: Local<'s>,
    ) -> Result<Local<'s>, CollectionError> {
        let (m, k) = (self.local_value(map), self.local_value(key));
        self.result(realized_keys(|| dissoc(self.heap(), &m, &k)))
    }

    /// `conj`: append to a vector, prepend to a list.
    pub fn form_conj(
        &self,
        coll: Local<'s>,
        value: Local<'s>,
    ) -> Result<Local<'s>, CollectionError> {
        let (c, v) = (self.local_value(coll), self.local_value(value));
        self.result(realized_keys(|| conj(self.heap(), &c, v)))
    }

    /// Value equality (metadata, provenance and identity ignored). An
    /// unrealized lazy sequence is equal only to itself (realizing one runs
    /// JavaScript, which only a task does).
    pub fn form_equals(&self, a: Local<'s>, b: Local<'s>) -> bool {
        equals(self.heap(), &self.local_value(a), &self.local_value(b))
    }

    /// The value hash, consistent with [`Scope::form_equals`].
    pub fn form_hash(&self, v: Local<'s>) -> i32 {
        hash(self.heap(), &self.local_value(v))
    }

    /// A form's metadata map, if it has one.
    pub fn form_meta(&self, v: Local<'s>) -> Option<Local<'s>> {
        meta(self.heap(), &self.local_value(v)).map(|m| self.new_local(m))
    }

    /// `v` with metadata `meta` (`None`: without). `v` itself when its
    /// metadata is already that object.
    pub fn form_with_meta(
        &self,
        v: Local<'s>,
        meta: Option<Local<'s>>,
    ) -> Result<Local<'s>, CollectionError> {
        let m = meta.map_or(Value::Null, |m| self.local_value(m));
        self.result(with_meta(self.heap(), &self.local_value(v), &m))
    }

    /// The value an occurrence wraps (through nested occurrences), or `v`.
    pub fn form_value(&self, v: Local<'s>) -> Local<'s> {
        self.new_local(unwrap(self.heap(), self.local_value(v)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Count requested host bytes only during one comparison, on its thread.
    // Persistent payloads are built before measurement, so copying their
    // candidate entries would be visible here despite bypassing Heap's budget.
    mod allocations {
        use std::alloc::{GlobalAlloc, Layout, System};
        use std::cell::Cell;

        thread_local! {
            static BYTES: Cell<Option<usize>> = const { Cell::new(None) };
        }

        struct Counting;

        #[global_allocator]
        static GLOBAL: Counting = Counting;

        // SAFETY: allocation and deallocation forward unchanged to System.
        // The const thread-local counter never allocates or runs destructors.
        unsafe impl GlobalAlloc for Counting {
            unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
                // SAFETY: the caller supplies a valid allocation layout.
                let ptr = unsafe { System.alloc(layout) };
                if !ptr.is_null() {
                    let _ = BYTES.try_with(|bytes| {
                        if let Some(n) = bytes.get() {
                            bytes.set(Some(n + layout.size()));
                        }
                    });
                }
                ptr
            }

            unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
                // SAFETY: ptr and layout came from this allocator's System.
                unsafe { System.dealloc(ptr, layout) };
            }
        }

        pub fn during(f: impl FnOnce() -> bool) -> (bool, usize) {
            BYTES.with(|bytes| bytes.set(Some(0)));
            let result = f();
            let bytes = BYTES.with(|bytes| bytes.replace(None).unwrap());
            (result, bytes)
        }
    }

    fn n(x: f64) -> Value {
        Value::Number(x)
    }

    fn s(t: &str) -> Value {
        Value::String(JsString::from(t))
    }

    #[test]
    fn deep_forms_hash_and_compare_without_recursion() {
        let heap = Arc::new(Heap::default());
        let _scope = Scope::new(&heap);
        let (mut a, mut b) = (n(1.0), n(1.0));
        for i in 0..100_000 {
            a = if i % 2 == 0 {
                new_list(&heap, &[a]).unwrap()
            } else {
                new_vector(&heap, &[a]).unwrap()
            };
            b = new_vector(&heap, &[b]).unwrap();
        }
        assert_eq!(hash(&heap, &a), hash(&heap, &b));
        assert!(equals(&heap, &a, &b));
        let c = new_list(&heap, &[n(2.0)]).unwrap();
        let c = (0..99_999).fold(c, |c, _| new_list(&heap, &[c]).unwrap());
        assert!(!equals(&heap, &a, &c));
    }

    #[test]
    fn sorted_comparison_traversal_storage_is_admitted_and_released() {
        let heap = Arc::new(Heap::with_test_byte_headroom(1 << 20, false));
        let _scope = Scope::new(&heap);
        let mut left = Vec::new();
        let mut right = Vec::new();
        for i in 0..512 {
            left.push(new_vector(&heap, &[n(i as f64)]).unwrap());
            right.push(new_vector(&heap, &[n(i as f64)]).unwrap());
        }
        let a = new_vector(&heap, &left).unwrap();
        let b = new_vector(&heap, &right).unwrap();
        let budget = heap.byte_budget();
        let occupied = budget.used();
        assert!(compare_keys(&heap, &a, &b).unwrap().is_eq());
        assert_eq!(
            budget.used(),
            occupied,
            "completed traversal releases its charge"
        );
        // The smaller allowance rejects traversal frames; the larger admits
        // frames but rejects the growing table of completed singleton pairs.
        for headroom in [1, 8192] {
            budget.set_limit(occupied + headroom);
            assert!(matches!(
                compare_keys(&heap, &a, &b),
                Err(CollectionError::HeapLimit)
            ));
            assert_eq!(
                budget.used(),
                occupied,
                "failed traversal releases its charge"
            );
        }
    }

    #[test]
    fn shared_subtrees_compare_in_time_linear_in_distinct_pairs() {
        let heap = Arc::new(Heap::default());
        let _scope = Scope::new(&heap);
        // 60 levels of [x x]: 2^60 paths, 60 distinct vectors per side.
        let (mut a, mut b) = (n(1.0), n(1.0));
        for _ in 0..60 {
            a = new_vector(&heap, &[a.clone(), a]).unwrap();
            b = new_list(&heap, &[b.clone(), b]).unwrap();
        }
        assert!(equals(&heap, &a, &b));
        assert_eq!(hash(&heap, &a), hash(&heap, &b));
    }

    #[test]
    fn positional_equality_auxiliary_storage_is_linear() {
        let heap = Arc::new(Heap::default());
        let _scope = Scope::new(&heap);
        let meta = new_map(&heap, &[(s("tag"), n(1.0))]).unwrap();
        for set in [false, true] {
            let measure = |len| {
                // Equal duplicates match the first candidate: the comparison
                // itself is linear too, independently of positional scanning.
                let value = if set {
                    new_literal_set(&heap, &vec![n(0.0); len]).unwrap()
                } else {
                    new_literal_map(&heap, &vec![n(0.0); 2 * len]).unwrap()
                };
                let copy = with_meta(&heap, &value, &meta).unwrap();
                assert!(!value.same_object(&copy));
                let (equal, bytes) = allocations::during(|| equals(&heap, &value, &copy));
                assert!(equal);
                bytes
            };
            let small = measure(256);
            let medium = measure(1024);
            // A 4x input increase stays near 4x host allocation, rather than
            // the 16x increase from per-query copies of all candidates.
            assert!(medium <= 5 * small, "set={set}: {small} -> {medium}");
            let large = measure(10_000);
            assert!(large <= 1024 * 10_000, "set={set}: {large} host bytes");
        }
    }

    #[test]
    fn keys_with_equal_full_hashes_share_a_collision_node() {
        let heap = Arc::new(Heap::default());
        let _scope = Scope::new(&heap);
        // "Aa" and "BB" have the same Java string hash, so the same value
        // hash and the same trie hash.
        assert_eq!(hash(&heap, &s("Aa")), hash(&heap, &s("BB")));
        let m = new_map(&heap, &[(s("Aa"), n(1.0)), (s("BB"), n(2.0))]).unwrap();
        assert_eq!(count(&heap, &m), Ok(2));
        assert_eq!(get(&heap, &m, &s("Aa")), Ok(Some(n(1.0))));
        assert_eq!(get(&heap, &m, &s("BB")), Ok(Some(n(2.0))));
        let reversed = new_map(&heap, &[(s("BB"), n(2.0)), (s("Aa"), n(1.0))]).unwrap();
        assert!(equals(&heap, &m, &reversed));
        let swapped = new_map(&heap, &[(s("BB"), n(1.0)), (s("Aa"), n(2.0))]).unwrap();
        assert!(!equals(&heap, &m, &swapped));
        let other = new_map(&heap, &[(s("Aa"), n(1.0)), (s("C#"), n(2.0))]).unwrap();
        assert!(!equals(&heap, &m, &other));
        let without = dissoc(&heap, &m, &s("Aa")).unwrap();
        assert_eq!(get(&heap, &without, &s("Aa")), Ok(None));
        assert_eq!(get(&heap, &without, &s("BB")), Ok(Some(n(2.0))));
    }

    /// `a_0 = "Aa"`, `b_0 = "BB"` (equal full hashes, unequal), and
    /// `a_(i+1) = {a_i 1, b_i 2}`, `b_(i+1) = {a_i 2, b_i 1}`: unequal maps
    /// whose hashes are equal again, so every level of a comparison is a
    /// choice between two colliding candidate keys, and the levels share
    /// their children (2^depth paths). With `swap`, the bottom strings trade
    /// places, which makes `a` structurally the other side's `b`.
    fn colliding_levels(heap: &Heap, depth: usize, swap: bool) -> (Value, Value) {
        let (mut a, mut b) = if swap {
            (s("BB"), s("Aa"))
        } else {
            (s("Aa"), s("BB"))
        };
        for _ in 0..depth {
            let next_a = new_map(heap, &[(a.clone(), n(1.0)), (b.clone(), n(2.0))]).unwrap();
            let next_b = new_map(heap, &[(a, n(2.0)), (b, n(1.0))]).unwrap();
            (a, b) = (next_a, next_b);
        }
        (a, b)
    }

    #[test]
    fn nested_colliding_keys_compare_and_look_up_on_a_small_stack() {
        // A small native stack: any recursion per level would overflow it.
        let run = std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(|| {
                // Inserting each level compares its two colliding keys (a walk
                // to the bottom), so building is quadratic in the depth.
                const DEPTH: usize = 1_000;
                let heap = Arc::new(Heap::default());
                let _scope = Scope::new(&heap);
                let (a, b) = colliding_levels(&heap, DEPTH, false);
                let (a2, b2) = colliding_levels(&heap, DEPTH, false);
                let (swapped, _) = colliding_levels(&heap, DEPTH, true);
                assert_eq!(hash(&heap, &a), hash(&heap, &b));
                assert_eq!(hash(&heap, &a), hash(&heap, &a2));
                assert_eq!(hash(&heap, &a), hash(&heap, &swapped));
                // Separately allocated copies are equal; the colliding
                // twins are not, found only at the bottom of the chain.
                assert!(equals(&heap, &a, &a2) && equals(&heap, &b, &b2));
                assert!(!equals(&heap, &a, &b) && !equals(&heap, &a2, &b));
                assert!(equals(&heap, &swapped, &b) && !equals(&heap, &swapped, &a));
                // One varied value (its hash differs too) is unequal.
                let varied = new_map(&heap, &[(a2.clone(), n(1.0)), (b2.clone(), n(3.0))]).unwrap();
                let level = new_map(&heap, &[(a.clone(), n(1.0)), (b.clone(), n(2.0))]).unwrap();
                assert!(!equals(&heap, &level, &varied));
                // Maps keyed by the colliding twins: equal probes find
                // their own entries.
                let m = new_map(&heap, &[(a.clone(), s("a")), (b.clone(), s("b"))]).unwrap();
                assert_eq!(count(&heap, &m), Ok(2));
                assert_eq!(get(&heap, &m, &a2), Ok(Some(s("a"))));
                assert_eq!(get(&heap, &m, &b2), Ok(Some(s("b"))));
                assert_eq!(get(&heap, &m, &swapped), Ok(Some(s("b"))));
                let without = dissoc(&heap, &m, &b2).unwrap();
                assert_eq!(get(&heap, &without, &b), Ok(None));
                assert_eq!(get(&heap, &without, &a2), Ok(Some(s("a"))));
                // Both maps hold the twins (stored order may differ).
                let copy = new_map(&heap, &[(b2, s("b")), (a2, s("a"))]).unwrap();
                assert!(equals(&heap, &m, &copy));
                let other = new_map(&heap, &[(a, s("b")), (b, s("a"))]).unwrap();
                assert!(!equals(&heap, &m, &other));
            })
            .unwrap();
        run.join()
            .expect("no stack overflow or failure on the small stack");
    }

    #[test]
    fn hashes_are_cached_per_object_and_kept_by_with_meta() {
        let heap = Arc::new(Heap::default());
        let _scope = Scope::new(&heap);
        let v = new_vector(&heap, &[n(1.0), s("x")]).unwrap();
        let f = form_object(&heap, &v).unwrap();
        assert!(f.hash.get().is_none());
        let h = hash(&heap, &v);
        assert_eq!(f.hash.get(), Some(&h));
        let k = new_named(&heap, FormKind::Keyword, None, JsString::from("m")).unwrap();
        let meta = new_map(&heap, &[(k, n(1.0))]).unwrap();
        let w = with_meta(&heap, &v, &meta).unwrap();
        assert_eq!(form_object(&heap, &w).unwrap().hash.get(), Some(&h));
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;
    use crate::collections::{Provenance, new_occurrence};

    #[test]
    fn the_same_object_equals_itself_and_nan_equals_nothing_else() {
        let heap = Arc::new(Heap::default());
        let _scope = Scope::new(&heap);
        let nan = Value::Number(f64::NAN);
        let occ = new_occurrence(&heap, nan.clone(), Provenance::default()).unwrap();
        let other = new_occurrence(&heap, nan.clone(), Provenance::default()).unwrap();
        assert!(equals(&heap, &occ, &occ));
        assert!(!equals(&heap, &occ, &other));
        assert!(!equals(&heap, &nan, &nan));
        let list = new_list(&heap, &[nan]).unwrap();
        assert!(equals(&heap, &list, &list));
    }
}
