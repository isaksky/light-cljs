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
//! | map | entries, keys unique by value equality (a hash trie) | yes |
//! | set | members unique by value equality (a hash trie; light-cljs-477) | yes |
//!
//! Elements, keys, values and metadata may be any value: `nil`
//! (`null`/`undefined`), booleans, numbers, strings, other forms, and
//! occurrences ([`crate::collections`]): one value at one source
//! location. Children are held by identity, never copied, so an update
//! returns a new object whose untouched children (and trie nodes) are the
//! same ones as before. Metadata must be `nil` or a form map.
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
//! * Sets are equal with the same count and an equal member in the other
//!   for every member of one; the hash is `hashUnorderedColl` of the
//!   members. A set never equals a map, list or vector.
//! * Metadata never takes part: `with_meta` changes neither equality nor
//!   hash.
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
//! candidates (backtracking past unequal ones) instead of one. A lookup
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
//! qualified name, its field names and whether it is a record, made by
//! `__rt.structType(name, fieldNames, record)`; each call is a new identity.
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
//! the same type identity, equal fields and equal extension entries; the
//! hash is the sum of each entry's key hash xor value hash, xor the
//! `stringHash` of the type name, cached like every form hash in an
//! atomic, idempotent slot. A deftype instance is equal only to itself and
//! hashes by identity; the clj operations reject it. A record is never
//! equal to a map.
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
//! Supported: the six kinds above, metadata, occurrences as elements and
//! keys, the operations listed, sequential list/vector equality, and the
//! ClojureScript core operations of [`crate::collections::clj`] (whose
//! `rest` copies into a new list). Not
//! supported (not claimed): keyword and symbol names whose spelling does
//! not split back (rejected, see "Names"), and a runtime keyword whose
//! stored spelling differs from its namespace and name (the runtime's
//! one-argument `(keyword "a/b/c")`); sorted maps and sets, lazy sequences,
//! shared seq views (a list is read by index), user hashing or equality
//! (`IHash`/`IEquiv`) of structs,
//! interned or cached keyword identity, characters as a separate kind (a
//! character is a one-unit string; the reader distinction, like exact
//! numeric spellings, travels with provenance), property access or
//! iteration protocols on forms from JavaScript, and conversion to or from
//! the compiler's `Datum` (light-cljs-462 owns that bridge). `conj` on a
//! list prepends in time linear in its length.

use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

use super::map::PMap;
use super::{CollectionError, PVector, identical, index_from_value, object_name};
use crate::heap::gc::{Trace, TraceEdge};
use crate::heap::{Heap, HeapObject, Local, ObjectId, Scope};
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
}

impl FormKind {
    /// The kind's ClojureScript name: `keyword`, `symbol`, `list`, `vector`,
    /// `map`, `set`, `struct` or `type` (the result of the `formKind`
    /// intrinsic).
    pub fn name(self) -> &'static str {
        match self {
            FormKind::Keyword => "keyword",
            FormKind::Symbol => "symbol",
            FormKind::List => "list",
            FormKind::Vector => "vector",
            FormKind::Map => "map",
            FormKind::Set => "set",
            FormKind::Struct => "struct",
            FormKind::Type => "type",
        }
    }

    /// The name in diagnostics, distinct from the SameValueZero kinds.
    pub(crate) fn diagnostic_name(self) -> &'static str {
        match self {
            FormKind::Vector => "form vector",
            FormKind::Map => "form map",
            FormKind::Set => "form set",
            FormKind::Type => "struct type",
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
    /// Keys stored with the trie hash of their value hash ([`trie_hash`]),
    /// compared by [`equals`]. Never read with the SameValueZero API.
    Map(PMap),
    /// Members stored as keys that map to themselves (the stored member),
    /// hashed and compared like map keys.
    Set(PMap),
    /// A struct type ("Structs" in the module docs).
    Type(Arc<TypeInfo>),
    /// An instance of a struct type.
    Struct(StructBody),
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
    /// A record's further entries, structural keys as in a form map.
    pub(crate) ext: Option<PMap>,
}

/// One form heap object: immutable once allocated, shared by `Arc` so a
/// read copies out one pointer and its cached hash is shared by readers.
#[derive(Debug)]
pub(crate) struct FormObject {
    body: FormBody,
    meta: Option<Value>,
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
            hash: OnceLock::new(),
        }
    }

    /// A copy with other metadata, keeping the cached hash.
    fn with_meta(&self, meta: Option<Value>) -> FormObject {
        let hash = OnceLock::new();
        if let Some(h) = self.hash.get() {
            let _ = hash.set(*h);
        }
        FormObject {
            body: self.body.clone(),
            meta,
            hash,
        }
    }

    /// This value with another body and the same metadata (an update).
    fn updated(&self, body: FormBody) -> FormObject {
        FormObject::new(body, self.meta.clone())
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
            FormBody::Type(_) => {}
            FormBody::Struct(s) => {
                if s.info.record {
                    s.fields.for_each(&mut f);
                    if let Some(ext) = &s.ext {
                        ext.for_each(|k, v| {
                            f(k);
                            f(v);
                        });
                    }
                }
            }
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
        }
        if let Some(meta) = &self.meta {
            meta.trace(visit);
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
    map: &PMap,
    key: Value,
    value: Value,
) -> Result<Option<PMap>, CollectionError> {
    let h = trie_hash(hash(heap, &key));
    map.try_assoc_with(heap, h, key, value, &|a, b| equals(heap, a, b))
}

fn map_get(heap: &Heap, map: &PMap, key: &Value) -> Option<Value> {
    let h = trie_hash(hash(heap, key));
    map.get_with(h, key, &|a, b| equals(heap, a, b))
}

// ---- Construction ----

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

pub(crate) fn new_vector(heap: &Heap, items: &[Value]) -> Result<Value, CollectionError> {
    let body = FormBody::Vector(PVector::try_from_values(heap, items)?);
    alloc(heap, FormObject::new(body, None), items)
}

/// A new map; a later duplicate (equal) key replaces an earlier one's
/// value, keeping the earlier key object.
pub(crate) fn new_map(heap: &Heap, entries: &[(Value, Value)]) -> Result<Value, CollectionError> {
    let mut map = PMap::default();
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
fn set_add(heap: &Heap, set: &PMap, member: Value) -> Result<Option<PMap>, CollectionError> {
    if map_get(heap, set, &member).is_some() {
        return Ok(None);
    }
    map_assoc(heap, set, member.clone(), member)
}

/// A new set of `items`; of equal members the first is kept.
pub(crate) fn new_set(heap: &Heap, items: &[Value]) -> Result<Value, CollectionError> {
    let mut set = PMap::default();
    for item in items {
        if let Some(updated) = set_add(heap, &set, item.clone())? {
            set = updated;
        }
    }
    alloc(heap, FormObject::new(FormBody::Set(set), None), items)
}

// ---- Structs (light-cljs-478) ----

/// A new struct type named `name` (a string) with the field names of the
/// form vector or list `fields` (strings), a record when `record` is true.
/// Each call makes a new identity.
pub(crate) fn new_type(
    heap: &Heap,
    name: &Value,
    fields: &Value,
    record: &Value,
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
    let info = TypeInfo {
        name,
        fields: names.into_boxed_slice(),
        record: matches!(record, Value::Bool(true)),
    };
    alloc(
        heap,
        FormObject::new(FormBody::Type(Arc::new(info)), None),
        &[],
    )
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
        let map = |i: usize| -> Result<Option<(Value, PMap)>, CollectionError> {
            let Some(v) = values.get(i) else {
                return Ok(None);
            };
            let v = unwrap(heap, v.clone());
            match form_object(heap, &v).as_deref().map(FormObject::body) {
                Some(FormBody::Map(m)) => Ok(Some((v, m.clone()))),
                None if matches!(v, Value::Null | Value::Undefined) => Ok(None),
                _ => Err(wrong(heap, "form map or nil", &v)),
            }
        };
        meta = map(n)?.map(|(v, _)| v);
        ext = map(n + 1)?.map(|(_, m)| m).filter(|m| m.len() > 0);
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
/// extension entries (in trie order).
pub(crate) fn record_entries(
    heap: &Heap,
    s: &StructBody,
) -> Result<Vec<(Value, Value)>, CollectionError> {
    let mut entries = Vec::with_capacity(s.info.fields.len());
    for (name, value) in s.info.fields.iter().zip(s.fields.to_vec()) {
        entries.push((field_keyword(heap, name)?, value));
    }
    if let Some(ext) = &s.ext {
        entries.extend(ext.to_vec());
    }
    Ok(entries)
}

/// A record's value for `key`: a field by its keyword, else an extension
/// entry by an equal key.
pub(crate) fn record_get(heap: &Heap, s: &StructBody, key: &Value) -> Option<Value> {
    match field_index(heap, &s.info, key) {
        Some(i) => s.fields.get(i),
        None => s.ext.as_ref().and_then(|ext| map_get(heap, ext, key)),
    }
}

/// A record like `f` (same type and metadata) with this body; its hash is
/// computed again.
fn record_like(
    heap: &Heap,
    f: &FormObject,
    s: &StructBody,
    fields: PVector,
    ext: Option<PMap>,
    parts: &[Value],
) -> Result<Value, CollectionError> {
    let ext = ext.filter(|e| e.len() > 0);
    let body = StructBody {
        ty: s.ty.clone(),
        info: s.info.clone(),
        fields,
        ext,
    };
    alloc(heap, f.updated(FormBody::Struct(body)), parts)
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
            let ext = s.ext.clone().unwrap_or_default();
            match map_assoc(heap, &ext, key, value)? {
                Some(ext) => record_like(heap, &f, &s, s.fields.clone(), Some(ext), &parts),
                None => Ok(record.clone()),
            }
        }
    }
}

/// `dissoc` on a record: a field key gives a form map of the other entries
/// with the record's metadata (no longer a record); an extension key a
/// record without it; any other key `record` itself.
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
    let h = trie_hash(hash(heap, key));
    match ext.try_dissoc_with(heap, h, key, &|a, b| equals(heap, a, b))? {
        Some(ext) => record_like(
            heap,
            &f,
            &s,
            s.fields.clone(),
            Some(ext),
            std::slice::from_ref(record),
        ),
        None => Ok(record.clone()),
    }
}

/// `map->R`: a record of type `ty` whose fields are `map`'s values for the
/// field keywords and whose extension entries are `map`'s other entries.
pub(crate) fn record_from_map(
    heap: &Heap,
    ty: &Value,
    map: &Value,
) -> Result<Value, CollectionError> {
    let info = type_info(heap, ty)?;
    let entries = match resolve(heap, map.clone()) {
        (_, None) if matches!(unwrap(heap, map.clone()), Value::Null | Value::Undefined) => {
            Vec::new()
        }
        (_, Some((_, f))) => match f.body() {
            FormBody::Map(m) => m.to_vec(),
            FormBody::Struct(s) if s.info.record => record_entries(heap, s)?,
            _ => return Err(wrong(heap, "form map or record", map)),
        },
        _ => return Err(wrong(heap, "form map or record", map)),
    };
    let mut fields = vec![Value::Null; info.fields.len()];
    let mut ext = PMap::default();
    for (k, v) in entries {
        match field_index(heap, &info, &k) {
            Some(i) => fields[i] = v,
            None => {
                if let Some(updated) = map_assoc(heap, &ext, k, v)? {
                    ext = updated;
                }
            }
        }
    }
    let body = StructBody {
        ty: ty.clone(),
        info,
        fields: PVector::try_from_values(heap, &fields)?,
        ext: (ext.len() > 0).then_some(ext),
    };
    alloc(
        heap,
        FormObject::new(FormBody::Struct(body), None),
        &[ty.clone(), map.clone()],
    )
}

// ---- Inspection ----

pub(crate) fn kind(heap: &Heap, v: &Value) -> Option<FormKind> {
    form_object(heap, v).map(|f| f.kind())
}

pub(crate) fn name(heap: &Heap, v: &Value) -> Result<FormName, CollectionError> {
    match form_object(heap, v).as_deref().map(FormObject::body) {
        Some(FormBody::Keyword(n) | FormBody::Symbol(n)) => Ok(n.clone()),
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

fn map_body(heap: &Heap, v: &Value) -> Result<PMap, CollectionError> {
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

/// The elements of a list or vector, in order, or the members of a set in
/// unspecified (deterministic) trie order.
pub(crate) fn items(heap: &Heap, coll: &Value) -> Result<Vec<Value>, CollectionError> {
    if let Some(FormBody::Set(set)) = form_object(heap, coll).as_deref().map(FormObject::body) {
        let mut members = Vec::with_capacity(set.len());
        set.for_each(|k, _| members.push(k.clone()));
        return Ok(members);
    }
    match sequential(heap, coll) {
        Ok(items) => Ok(items.to_vec()),
        Err(_) => Err(wrong(heap, "list, form vector or form set", coll)),
    }
}

/// The entries of a map, in unspecified (deterministic) trie order.
pub(crate) fn entries(heap: &Heap, map: &Value) -> Result<Vec<(Value, Value)>, CollectionError> {
    Ok(map_body(heap, map)?.to_vec())
}

/// Lookup: a map entry by an equal key, or a vector element by index
/// (`None` when absent, out of bounds or not an index, as `get` does).
pub(crate) fn get(
    heap: &Heap,
    coll: &Value,
    key: &Value,
) -> Result<Option<Value>, CollectionError> {
    match form_object(heap, coll).as_deref().map(FormObject::body) {
        Some(FormBody::Map(map) | FormBody::Set(map)) => Ok(map_get(heap, map, key)),
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
        FormBody::Map(map) => match map_assoc(heap, map, key, value)? {
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
                if identical(&old, &value) {
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
    let h = trie_hash(hash(heap, key));
    match body.try_dissoc_with(heap, h, key, &|a, b| equals(heap, a, b))? {
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

/// `disj` of a set member by an equal value (absent: the set itself).
pub(crate) fn disj(heap: &Heap, set: &Value, member: &Value) -> Result<Value, CollectionError> {
    let Some(f) = form_object(heap, set) else {
        return Err(wrong(heap, "form set", set));
    };
    let FormBody::Set(body) = &f.body else {
        return Err(wrong(heap, "form set", set));
    };
    let h = trie_hash(hash(heap, member));
    match body.try_dissoc_with(heap, h, member, &|a, b| equals(heap, a, b))? {
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
            if !matches!(f.kind(), FormKind::Keyword | FormKind::Type)
                && (f.kind() != FormKind::Struct || f.record().is_some()) =>
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
    if same {
        return Ok(v.clone());
    }
    let parts: Vec<Value> = std::iter::once(v.clone()).chain(meta.clone()).collect();
    alloc(heap, f.with_meta(meta), &parts)
}

// ---- Equality ----

/// Equality of two non-form values (occurrences already unwrapped).
fn scalar_equals(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null | Value::Undefined, Value::Null | Value::Undefined) => true,
        _ => a.strict_equals(b),
    }
}

/// Value equality. See the module docs.
///
/// An explicit-stack goal machine: no recursion on the native stack, so
/// depth is bounded only by memory. A pair of forms expands into an
/// *all-of* goal over its children: element pairs, or for each entry of
/// one map the entries of the other with the same stored full hash. One
/// such candidate is the only possible match (the other map's keys are
/// pairwise unequal and equal keys hash alike), so its key and value pairs
/// join the all-of goal; several candidates (distinct keys sharing a full
/// hash) become a *one-of* goal that tries each candidate's key and value
/// pairs in turn, backtracking past a failed candidate.
///
/// The result of every expanded pair of form identities is recorded,
/// equal or unequal, and reused when the pair is met again (persistent
/// values are acyclic, so a pair never recurs inside its own expansion).
/// Each distinct pair is therefore expanded at most once: the work is
/// linear in the distinct object pairs compared plus their children, where
/// an entry whose full hash `c` keys of the other map share contributes up
/// to `c` candidate key/value pairs instead of one.
pub(crate) fn equals(heap: &Heap, a: &Value, b: &Value) -> bool {
    /// One thing to establish inside an all-of goal.
    enum Step {
        Pair(Value, Value),
        /// Candidate (key pair, value pair) alternatives for one entry.
        OneOf(Vec<[(Value, Value); 2]>),
    }
    enum Frame {
        /// Every step must hold; `memo` is the form pair it decides.
        All {
            memo: Option<(ObjectId, ObjectId)>,
            pending: Vec<Step>,
        },
        /// One alternative must hold.
        Any(Vec<[(Value, Value); 2]>),
    }
    /// What comparing one pair yields.
    enum Outcome {
        Decided(bool),
        /// The form pair is unequal, found after reading its entries.
        Refuted((ObjectId, ObjectId)),
        Expand((ObjectId, ObjectId), Vec<Step>),
    }
    let mut equal: HashSet<(ObjectId, ObjectId)> = HashSet::new();
    let mut unequal: HashSet<(ObjectId, ObjectId)> = HashSet::new();
    let compare = |a: Value, b: Value, equal: &HashSet<_>, unequal: &HashSet<_>| {
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
        if ia == ib || equal.contains(&(ia, ib)) {
            return Outcome::Decided(true);
        }
        if unequal.contains(&(ia, ib)) {
            return Outcome::Decided(false);
        }
        if let (Some(h), Some(k)) = (x.hash.get(), y.hash.get())
            && h != k
        {
            return Outcome::Decided(false);
        }
        match (&x.body, &y.body) {
            (FormBody::Keyword(m), FormBody::Keyword(n))
            | (FormBody::Symbol(m), FormBody::Symbol(n)) => Outcome::Decided(m.same(n)),
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
                let mut members = Vec::with_capacity(p.len());
                p.for_each_hashed(|h, k, _| members.push((h, k.clone())));
                let mut steps = Vec::with_capacity(members.len());
                for (h, k) in members {
                    let mut candidates = q.entries_with_hash(h);
                    match candidates.len() {
                        0 => return Outcome::Refuted((ia, ib)),
                        1 => steps.push(Step::Pair(k, candidates.pop().unwrap().0)),
                        // A member pair with a trivially equal "value" pair.
                        _ => steps.push(Step::OneOf(
                            candidates
                                .into_iter()
                                .rev()
                                .map(|(qk, _)| [(k.clone(), qk), (Value::Null, Value::Null)])
                                .collect(),
                        )),
                    }
                }
                Outcome::Expand((ia, ib), steps)
            }
            (FormBody::Map(p), FormBody::Map(q)) => {
                if p.len() != q.len() {
                    return Outcome::Decided(false);
                }
                let mut entries = Vec::with_capacity(p.len());
                p.for_each_hashed(|h, k, v| entries.push((h, k.clone(), v.clone())));
                let mut steps = Vec::with_capacity(2 * entries.len());
                for (h, k, v) in entries {
                    let mut candidates = q.entries_with_hash(h);
                    match candidates.len() {
                        0 => return Outcome::Refuted((ia, ib)),
                        1 => {
                            let (qk, qv) = candidates.pop().unwrap();
                            // Popped last-first: the (cheaper) value first.
                            steps.push(Step::Pair(k, qk));
                            steps.push(Step::Pair(v, qv));
                        }
                        _ => steps.push(Step::OneOf(
                            candidates
                                .into_iter()
                                .rev()
                                .map(|(qk, qv)| [(k.clone(), qk), (v.clone(), qv)])
                                .collect(),
                        )),
                    }
                }
                Outcome::Expand((ia, ib), steps)
            }
            // Records of the same type with equal fields and extension
            // entries; a deftype only to itself (decided above).
            (FormBody::Struct(p), FormBody::Struct(q)) => {
                if !(p.info.record && q.info.record && p.ty.same_object(&q.ty)) {
                    return Outcome::Decided(false);
                }
                let empty = PMap::default();
                let (pe, qe) = (
                    p.ext.as_ref().unwrap_or(&empty),
                    q.ext.as_ref().unwrap_or(&empty),
                );
                if pe.len() != qe.len() {
                    return Outcome::Decided(false);
                }
                let mut steps: Vec<Step> = p
                    .fields
                    .to_vec()
                    .into_iter()
                    .zip(q.fields.to_vec())
                    .rev()
                    .map(|(a, b)| Step::Pair(a, b))
                    .collect();
                let mut entries = Vec::with_capacity(pe.len());
                pe.for_each_hashed(|h, k, v| entries.push((h, k.clone(), v.clone())));
                for (h, k, v) in entries {
                    let mut candidates = qe.entries_with_hash(h);
                    match candidates.len() {
                        0 => return Outcome::Refuted((ia, ib)),
                        1 => {
                            let (qk, qv) = candidates.pop().unwrap();
                            steps.push(Step::Pair(k, qk));
                            steps.push(Step::Pair(v, qv));
                        }
                        _ => steps.push(Step::OneOf(
                            candidates
                                .into_iter()
                                .rev()
                                .map(|(qk, qv)| [(k.clone(), qk), (v.clone(), qv)])
                                .collect(),
                        )),
                    }
                }
                Outcome::Expand((ia, ib), steps)
            }
            _ => Outcome::Decided(false),
        }
    };
    let mut frames = vec![Frame::All {
        memo: None,
        pending: vec![Step::Pair(a.clone(), b.clone())],
    }];
    // The result of the frame popped last, delivered to the one below it.
    let mut returned: Option<bool> = None;
    while let Some(frame) = frames.last_mut() {
        match frame {
            Frame::All { memo, pending } => {
                let failed = returned.take() == Some(false);
                let step = if failed { None } else { pending.pop() };
                match step {
                    // Done: every step held, or one failed.
                    None => {
                        let result = !failed;
                        if let Some(pair) = *memo {
                            if result { &mut equal } else { &mut unequal }.insert(pair);
                        }
                        frames.pop();
                        returned = Some(result);
                    }
                    Some(Step::Pair(a, b)) => match compare(a, b, &equal, &unequal) {
                        Outcome::Decided(true) => {}
                        Outcome::Decided(false) => returned = Some(false),
                        Outcome::Refuted(pair) => {
                            unequal.insert(pair);
                            returned = Some(false);
                        }
                        Outcome::Expand(pair, steps) => frames.push(Frame::All {
                            memo: Some(pair),
                            pending: steps,
                        }),
                    },
                    Some(Step::OneOf(alternatives)) => frames.push(Frame::Any(alternatives)),
                }
            }
            Frame::Any(alternatives) => match returned.take() {
                Some(true) => {
                    frames.pop();
                    returned = Some(true);
                }
                _ => match alternatives.pop() {
                    Some([key, value]) => frames.push(Frame::All {
                        memo: None,
                        pending: vec![Step::Pair(key.0, key.1), Step::Pair(value.0, value.1)],
                    }),
                    None => {
                        frames.pop();
                        returned = Some(false);
                    }
                },
            },
        }
    }
    returned.unwrap_or(true)
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
        (_, Some((id, f))) => match f.hash.get().copied().or_else(|| identity_hash(&f, id)) {
            Some(h) => h,
            None => form_hash(heap, &f),
        },
        (v, None) => scalar_hash(&v),
    }
}

/// The hash of one form from its children's (all cached by now).
fn shallow_hash(heap: &Heap, f: &FormObject) -> i32 {
    match &f.body {
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
            if let Some(ext) = &s.ext {
                ext.for_each(|k, v| {
                    sum = sum.wrapping_add(child_hash(heap, k) ^ child_hash(heap, v))
                });
            }
            sum ^ string_hash(s.info.name.code_units())
        }
        // Identity-hashed: set where the object identity is known
        // ([`identity_hash`]); never reached through it.
        FormBody::Struct(_) | FormBody::Type(_) => 0,
    }
}

/// The hash of a deftype instance or a struct type, whose `=` is identity:
/// a private hash of the object's identity, as for other heap objects.
fn identity_hash(f: &FormObject, id: ObjectId) -> Option<i32> {
    match &f.body {
        FormBody::Struct(s) if s.info.record => None,
        FormBody::Struct(_) | FormBody::Type(_) => {
            Some(*f.hash.get_or_init(|| scalar_hash(&Value::Object(id))))
        }
        _ => None,
    }
}

/// The cached hash of `root`, computing uncached descendants first with an
/// explicit stack (children before parents), so depth needs no recursion.
fn form_hash(heap: &Heap, root: &Arc<FormObject>) -> i32 {
    if let Some(h) = root.hash.get() {
        return *h;
    }
    let mut stack = vec![(root.clone(), false)];
    while let Some((f, children_done)) = stack.pop() {
        if f.hash.get().is_some() {
            continue;
        }
        if children_done {
            let h = shallow_hash(heap, &f);
            let _ = f.hash.set(h);
            continue;
        }
        stack.push((f.clone(), true));
        f.for_each_child(|c| {
            if let (_, Some((id, child))) = resolve(heap, c.clone())
                && child.hash.get().is_none()
                && identity_hash(&child, id).is_none()
            {
                stack.push((child, false));
            }
        });
    }
    *root.hash.get().expect("hash computed above")
}

/// The value hash: the light-cljs runtime's `hash` for supported values.
pub(crate) fn hash(heap: &Heap, v: &Value) -> i32 {
    match resolve(heap, v.clone()) {
        (_, Some((id, f))) => identity_hash(&f, id).unwrap_or_else(|| form_hash(heap, &f)),
        (v, None) => scalar_hash(&v),
    }
}

/// The embedder API. See the module docs.
impl<'s> Scope<'s> {
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
        self.result(new_map(self.heap(), &entries))
    }

    /// A new form set of `items` with structural members; of equal members
    /// the first is kept.
    pub fn new_form_set(&self, items: &[Local<'s>]) -> Result<Local<'s>, CollectionError> {
        self.result(new_set(self.heap(), &self.values(items)))
    }

    /// A new set without the member equal to `member`.
    pub fn form_disj(
        &self,
        set: Local<'s>,
        member: Local<'s>,
    ) -> Result<Local<'s>, CollectionError> {
        let (s, m) = (self.local_value(set), self.local_value(member));
        self.result(disj(self.heap(), &s, &m))
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

    /// The elements of a list or vector, in order, or a set's members in
    /// unspecified (deterministic) order.
    pub fn form_items(&self, coll: Local<'s>) -> Result<Vec<Local<'s>>, CollectionError> {
        Ok(self.new_locals(items(self.heap(), &self.local_value(coll))?))
    }

    /// The entries of a map, in unspecified (deterministic) order.
    pub fn form_entries(
        &self,
        map: Local<'s>,
    ) -> Result<Vec<(Local<'s>, Local<'s>)>, CollectionError> {
        let pairs = entries(self.heap(), &self.local_value(map))?;
        let locals = self.new_locals(pairs.into_iter().flat_map(|(k, v)| [k, v]));
        Ok(locals.chunks(2).map(|kv| (kv[0], kv[1])).collect())
    }

    /// A map value by an equal key, or a vector element by index; `None`
    /// when absent.
    pub fn form_get(
        &self,
        coll: Local<'s>,
        key: Local<'s>,
    ) -> Result<Option<Local<'s>>, CollectionError> {
        let found = get(self.heap(), &self.local_value(coll), &self.local_value(key))?;
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
        self.result(assoc(self.heap(), &c, k, v))
    }

    /// A new map without the entry for an equal key.
    pub fn form_dissoc(
        &self,
        map: Local<'s>,
        key: Local<'s>,
    ) -> Result<Local<'s>, CollectionError> {
        let (m, k) = (self.local_value(map), self.local_value(key));
        self.result(dissoc(self.heap(), &m, &k))
    }

    /// `conj`: append to a vector, prepend to a list.
    pub fn form_conj(
        &self,
        coll: Local<'s>,
        value: Local<'s>,
    ) -> Result<Local<'s>, CollectionError> {
        let (c, v) = (self.local_value(coll), self.local_value(value));
        self.result(conj(self.heap(), &c, v))
    }

    /// Value equality (metadata, provenance and identity ignored).
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
