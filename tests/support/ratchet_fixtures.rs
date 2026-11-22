//! The optimization fixture set (docs/advanced-optimization.md, Phase 0), shared by
//! the size ratchet in tests/compiler/size_ratchet.rs and the startup benchmark in
//! benches/compiler.rs (`--levels`), so every fixture pinned here is also measured,
//! except the programs of several namespaces in `NAMESPACE_FIXTURES`.
//!
//! Ceilings only move down: a change that shrinks output lowers them to the new size
//! rounded up to the next 100 bytes (next 1,000 from 10,000 bytes up) and records the
//! sizes in benchmarks/compiler.md. Raising a ceiling needs a reason in the commit
//! message. Add fixtures here when a later optimization needs a size or behavior
//! pinned; keep the five upstream rows.
//!
//! 2026-10-16: none/prepare ceilings include 2,000 additional bytes for the
//! compiler bridge's GC-owned provenance and bounded environment cache (1,344
//! bytes of shared runtime growth). Advanced slices discard this machinery;
//! their ceilings are unchanged. See benchmarks/penpot-investigation.md.
//!
//! Native sequence cursors (2026-10-16): eight advanced ceilings below increase
//! to the next 1,000 bytes for first/rest/next paths that avoid redispatching
//! unrelated protocols. This fixes override behavior and cuts the cold Penpot
//! render from 30.66 to 27.32 seconds; the representative QuickJS traversal
//! falls from 3,862 to 1,853 ms. The readable runtime grows by 767 bytes and
//! these slices exceed their previous ceilings by 11–362 bytes. Retain this
//! size/performance rationale in the eventual commit message.
//!
//! Environment descendants (2026-10-17): fifteen none/prepare ceilings move
//! to the next 1,000 bytes for bounded child reuse and canonical-snapshot
//! validation. The full runtime is embedded at these levels; the advanced
//! slices and ceilings are unchanged. The previous ceilings were exceeded by
//! 23–583 bytes. See the phase20 evidence in benchmarks/penpot-investigation.md;
//! retain this size/performance rationale in the eventual commit message.
//!
//! Intrinsic list construction (2026-10-17): eighteen none/prepare ceilings
//! move up 1,000 bytes for a direct list builder (390 readable bytes). Terse
//! slices using it grow by 99 bytes; only features exceeds its ceiling, by
//! 12 bytes, and moves to 71,000. This prevents a public ICounted override
//! from corrupting a new list's stored length and avoids redundant protocol
//! dispatch. The isolated main profile improves 166.78 -> 155.48 seconds.
//! Retain this size/performance rationale in the eventual commit message.

//! Fixed protocol metadata (2026-10-17): the readable runtime grows 1,051
//! bytes to cache private protocol ABI lookups while keeping membership and
//! method reads live. All 34 none/prepare ceilings move to the next 1,000
//! bytes; advanced sizes and ceilings are unchanged. The isolated main
//! profile improves 155.48 -> 148.13 seconds before frontier optimizations.
//! Retain this size/performance rationale in the eventual commit message.

//! Keyword record fields (2026-10-17): the readable runtime grows 493 bytes
//! for lookup/contains/assoc to use keyword identity instead of invoking a
//! key's IEquiv for every field. This fixes false field matches and reduces
//! the prototype Instaparse macro replay from 11.0 to 9.3 seconds. Twenty
//! none/prepare ceilings across eleven fixtures move to the next 1,000;
//! advanced ceilings stay unchanged. Retain this correctness/performance
//! rationale in the eventual commit message; see phase32 evidence.

//! Shared macro inputs (2026-10-17): 1,610 readable runtime bytes keep one
//! decoded call graph through environment import, then restore argument
//! provenance and validate snapshots without rebuilding those values. This
//! fixes &form/argument identity and avoids duplicate literal construction.
//! The prototype main improves 122.11 -> 116.98 seconds. All 34 none/prepare
//! ceiling pairs move to the next 1,000 bytes; advanced ceilings are unchanged.
//! Retain this correctness/performance rationale in the eventual commit message.

//! Seq kind tables (light-cljs-237, 2026-10-18): `seq`, `first`, `rest`,
//! `next` and `count` read a lazy seq's, a cell's or a view's branch from a
//! private table by the kind number its prototype holds. A program that
//! builds a lazy seq or a cell grows about 2.9 KB at advanced (the user
//! accepted about 2.8 KB for seq-pipeline going from 4.1x to 1.9x upstream;
//! it is 3.7x to 1.8x on this master); others move by at most 66 bytes.
//! Eleven advanced ceilings move to the next 1,000 bytes (closed-params by
//! 3 bytes) and every none/prepare ceiling by the 10,452 readable bytes.

//! Map entries, the array map's constructor and the types' statics
//! (light-cljs-247, 2026-10-18): the runtime that none and prepare embed
//! grows by the lean map entries, the public constructors' hooks, the statics'
//! condition and the wrappers of `lookup` and structural equality, with their
//! comments (5,425 bytes over both commits); those ceilings move to the next
//! 1,000. No advanced size grows; those that shrink by a step lower their
//! ceilings.

//! Private payload fields for the remaining per-operation kinds
//! (light-cljs-291, 2026-10-18): reduced markers, transients, queues and
//! iterator seqs use `privateStore`, and the maps and sets outside the hash
//! tries a `kindedStore` that holds payload and kind in two private fields of
//! one construction. An advanced program that tests values against these
//! stores keeps `privateStore` and `kindedStore` and grows 484-535 bytes
//! (`keyword-only` 2,875 to 3,396), for `event-maps` 6.2x and
//! `keyword-lookups` 2.2x as fast in Node; eighteen advanced ceilings move
//! to the new size rounded up (to 100 bytes below 10,000, else 1,000) and
//! every none/prepare ceiling by the 2,016 readable bytes.

//! WeakMap stores where the host states them (light-cljs-292, 2026-10-18):
//! `privateStore` and `kindedStore` return WeakMaps in the evaluator's
//! QuickJS, which makes its seq and per-operation workloads 10-35% faster.
//! The readable runtime grows 387 bytes; the nineteen none/prepare ceiling
//! pairs it passed move to the next 1,000. Advanced sizes shrink by up to
//! 61 bytes, as the stores share one test for a value that can hold a field;
//! the ceilings of atom-str, vector-printed, map-printed and defmulti move down.

//! Collection kinds (light-cljs-358, second part, 2026-10-19): vectors,
//! maps, sets and queues are kinds of the kind tables, which `seq`, `count`,
//! `nth`, `get`, `conj` and `assoc` read once per call. In Node at advanced,
//! `count` of the four collections is 3.3x as fast, `get` of a vector 5x, of
//! a hash map 1.7x, `nth` 2.9x, and with protocol implementations `conj`
//! onto a vector 4.7x. A program that keeps one of these operations and
//! builds a collection keeps its wrapper, the collections' branches and the
//! tables: 30 to 2,770 bytes at advanced over the first part (`map-only`
//! 7,313 to 7,879, `features` 72,736 to 75,501). The advanced ceilings
//! passed move to the new size rounded up (to 100 bytes below 10,000, else
//! 1,000), and every none/prepare ceiling, for the 10.1 KB of readable
//! runtime (branches, wrappers and their comments), to the next 1,000.

//! Per-type record equality and hash (light-cljs-267, 2026-10-19): at
//! advanced a record type of a program that compares or hashes values gets
//! its own `-equiv` and `-hash` (`record-keys` 7.7x to 2.07x upstream); no
//! fixture's advanced size moves. The readable runtime grows 283 bytes for
//! `recordEntriesHash`, which the shared record hash and the per-type one
//! share; the five none/prepare ceiling pairs it passed move to the next
//! 1,000.

//! Sets' `get` and map entry kinds (light-cljs-373, 2026-10-19): `get` of a
//! set and `seq`, `count`, `nth` and `get` of a map entry read the kind
//! tables (in Node at advanced `get` of a set 1.2x as fast, `count` of an
//! entry 2.6x, `get` of an entry 4.4x, destructuring an entry 1.3x). A
//! program building a map keeps the entries' kind (18 bytes) and the
//! branches of the operations it keeps: `map-only` 7,879 to 8,053,
//! `keyword-lookups` 34,714 to 35,170, `eight-records` 34,883 to 35,315,
//! `constant-keys` 47,532 to 48,297 and `features` 75,499 to 76,265 (which
//! also keep `get.sets`), and `closed-params` 14,970 to 15,027 (`get`'s
//! token for a method answering `missing`). Those advanced ceilings move to
//! the new size rounded up as before, and every none/prepare ceiling, for
//! the 3.1 KB of readable runtime, to the next 1,000.

//! The collections' `first`, `rest` and `next` and `extend-type`'s dispatch
//! cells (light-cljs-375, 2026-10-19): `first` and `rest` of vectors, map
//! entries, maps, sets and queues and `next` of vectors and entries read the
//! kind tables (in Node at advanced `first` of a vector 8.6x as fast, `next`
//! 4x, `rest` 1.5x, `first` of an entry 4.7x, of a map or a set 1.2x), and a
//! core protocol dispatcher lives in a cell its resolved keys hold. A
//! program that keeps `first`, `rest` or `next` and builds a collection
//! keeps their wrappers and the branches of the kinds it builds, and one
//! with a core protocol dispatcher keeps `dispatchCell` (54 bytes):
//! `println-hi` 4,795 to 4,853, `vector-printed` 10,992 to 11,052,
//! `keyword-lookups` 35,170 to 36,193, `eight-records` 35,315 to 36,720,
//! `constant-keys` 48,297 to 49,398, `event-maps` 33,665 to 34,637,
//! `mapv-count` 36,234 to 37,013, `callbacks` 43,845 to 44,663,
//! `protocol-params` 48,915 to 49,734, `features` 76,265 to 77,371 and
//! `features-direct` 75,873 to 76,979. Those advanced ceilings move to the
//! new size rounded up as before, and every none/prepare ceiling, for the
//! 5.8 KB of readable runtime, to the next 1,000 above the size.

//! Locals named per scope (light-cljs-419, 2026-10-22): at advanced the
//! program's locals take short names per scope, so sibling functions repeat
//! them. `features` 77,053 to 76,997 and the features chunk `app.reports.js`
//! 5,040 to 4,995, whose ceilings move down to 77,000 and 5,000; no other
//! advanced size crosses its rounding.
//!
//! Value-position statements without wrappers (light-cljs-420, 2026-10-22):
//! only `eight-records` changes, 37,384 to 37,369, under the same ceiling;
//! the fixtures have few value-position `let`, `case` or `loop` forms.
//!
//! `cljs.core/pr-writer` (light-cljs-981, 2026-11-14): the readable runtime
//! grows 343 bytes for upstream's private printer entry that libraries call
//! from their own `-pr-writer` methods. Eight none/prepare ceilings that it
//! exceeds by 70–343 bytes (keyword-case and event-maps at both, closed-params
//! at both, pure-defs and loadable-site at prepare) move to the next 1,000;
//! advanced sizes and ceilings are unchanged.
//!
//! cljs.core's internal type names (light-cljs-983, 2026-11-14): the readable
//! runtime grows 1,625 bytes for the types `ArrayNodeSeq` and `RSeq`, the
//! hash map hook choosing `ArrayNodeSeq`, and 16 inert types upstream code
//! extends protocols to (lite mode's collections, trie nodes, iterators), each
//! in the core table. Every none and prepare ceiling it exceeds, by up to 1,409
//! bytes, moves to the next 1,000: all but those of `features` and
//! `features-direct`, which emitter savings landed alongside keep under theirs.
//! At advanced only `features`, whose hash map seq now keeps the hook and
//! `ArrayNodeSeq`, moves, to 78,000.
//!
//! Protocol lookups on the runtime's own values (light-cljs-988, 2026-11-14):
//! the readable runtime grows about 2,300 bytes for `publicMethod`'s shortcut, the
//! store test `payloadStore` and their comments. The shortcut makes release
//! output run 10–15% faster in benchmarks/runtime-vs-shadow.md. Every
//! none and prepare ceiling, all of which it exceeds, moves to the next
//! 1,000. Advanced sizes stay under their ceilings.
//!
//! The code reader apart from EDN data reading (light-cljs-985, 2026-11-14):
//! the readable runtime grows about 290 bytes for the `codeReader` unit that
//! gives `read-string` and `read` the core names, so that a program reading
//! only EDN data keeps neither them nor the namespace registry at advanced
//! (re-frame's TodoMVC: benchmarks/re-frame-vs-shadow.md). The five
//! none/prepare ceiling pairs it exceeds (keyword-case, event-maps,
//! multi-arity, defmulti, direct-site), by 33–189 bytes, move to the next
//! 1,000; advanced sizes and ceilings are unchanged.
//!
//! Small maps and map entries (light-cljs-989, 2026-11-14): an array map's
//! entries are instances of one class (`ArrayMapEntry`) instead of frozen
//! objects with a hash getter closure each; a keyword key's search reads its
//! own `IEquiv` slot per entry as before but looks up `extend-type`'s tables
//! once; and a map entry is made with its prototype. Release output takes
//! 8-29% less time per iteration (benchmarks/runtime-vs-shadow.md). A
//! program building array maps keeps the class and the longer search:
//! `map-only` 8,066 to 8,378, `set-only` 4,887 to 5,197 and
//! `features-direct` to 77,066, whose ceilings move to 8,400, 5,200 and
//! 78,000. The readable runtime grows about 1,500 bytes; every none and
//! prepare ceiling it exceeds, which with light-cljs-985 is all of them,
//! moves to the next 1,000 above its size.
//!
//! One kind brand for the runtime's stores (light-cljs-990, 2026-11-14): the
//! private-field stores share one brand class (`payloads`) whose kind field
//! names a value's store in one read, and a map entry is a vector of its own
//! kind instead of a WeakSet entry. Kind tests no longer probe each store
//! in turn, and building a map entry no longer writes a WeakSet;
//! benchmarks/runtime-vs-shadow.md has the measured effect. Every program
//! keeping a store keeps the brand class: advanced output grows by about
//! 220-390 bytes, and the fourteen advanced ceilings it exceeds move to the
//! next 100 (1,000 from 10,000 up), as does the features example's startup
//! chunk (6,363 bytes, to 6,400). The readable runtime grows about 2,400
//! bytes; every none and prepare ceiling, all of which it exceeds, moves to
//! the next 1,000 above its size.
//!
//! The protocol hooks' one step for the runtime's own values (light-cljs-998,
//! 2026-11-14): with `extend-type` or `specify!` kept, the `satisfies` and
//! `implements` hooks answer a value whose kind names a payload store from
//! its mask bit, marker and native test (`storeProtocol`, `kindStore`)
//! instead of the full test. The readable runtime grows about 1,550 bytes;
//! every none and prepare ceiling, all of which it exceeds, moves to the next
//! 1,000 above its size. Advanced sizes are unchanged: no fixture keeps the
//! hooks.
//!
//! The Pants build's runtime (light-cljs-1024, 2026-11-15): the stores' own
//! metadata are read by descriptor, as the Pants engine has no
//! `Object.hasOwn`, and an array map's search leaves the identity of keys
//! other than keywords, strings and numbers to `equalPair`. The readable
//! runtime grows up to 126 bytes, past the none and prepare ceilings of
//! `keyword-case` and `event-maps`, which move to the next 1,000. At
//! advanced `constant-keys` grows 30 bytes (49,991 to 50,021), past its
//! ceiling, which moves to 51,000.
//!
//! The build's key on every protocol slot, marker and mask name
//! (light-cljs-1009, 2026-11-15, docs/value-layout.md section 1): each name
//! grows by five bytes (`$LKEY`), and the readable runtime by about 300.
//! The three none and prepare ceilings it exceeds (keyword-case,
//! event-maps, closed-params, by 46-206 bytes) move to the next 1,000.
//! Advanced sizes grow by a few bytes and stay under their ceilings.
//!
//! Vectors, subvecs, map entries and vector transients as classes, rebased
//! onto the Pants build's runtime (light-cljs-1009 with light-cljs-1024,
//! 2026-11-15): the two changes together put the readable runtime past most
//! none and prepare ceilings, and the review fixes (the shared-heap
//! profile's slot registry and switch, the unkeyed `IFn` invoke slots in
//! `protocolSlot`, `inspect`'s stamp test) add 543 bytes more. Each
//! exceeded none and prepare ceiling moves to the next 1,000. At advanced,
//! the class layout grew `keyword-lookups`, `constant-keys`, `vector-only`
//! and `protocol-params` past their ceilings (by 3 to 90 bytes), and the
//! fixes grow the record programs by 56 bytes (`record-protocol`,
//! `pure-defs`, `three-records`) and shrink others by up to 77; each
//! exceeded advanced ceiling moves to the next 100 below 10,000 and the
//! next 1,000 above.
//!
//! Vectors, map entries, subvectors and transient vectors as class values
//! with slot-first dispatch (light-cljs-1009, 2026-11-15): each holds its
//! store in an own keyed field behind its prototype's keyed kind
//! (`vectorStore`, `VectorCell` and the other cells), the vector prototype
//! has six keyed slot methods, `nth`, `count`, `peek`, `conj`, `assoc` and
//! `get` call them first, and the compile-time runtime stamps each value
//! (`stamp`). The readable runtime grows about 11,000 bytes over master
//! `f533efb1`, so every none and prepare ceiling moves to the next 1,000
//! above its size. Advanced: `vector-only` 5,394 to 5,441 bytes, `mapv-count`
//! 37,295 to 39,561, `features-direct` 77,785 to 81,454; the three advanced
//! ceilings exceeded (`eight-records`, `map-only`, `features-direct`, by
//! 19-454 bytes: the store helpers every collection test now calls) and the
//! features example's shared chunk move to the next 100 (1,000 from 10,000
//! up).
//!
//! The per-build runtime key (light-cljs-1025, 2026-11-15, docs/value-layout.md
//! section 1): the output writer writes each build's eight-character key for
//! the placeholder, four bytes more on every keyed name, and every runtime
//! registers it in the page's registry as it loads (`runtimeKey`, about 290
//! terse bytes, which every advanced slice keeps, and about 1,100 readable
//! ones with its comment). Every none and prepare ceiling moves to the next
//! 1,000 above its size. At advanced the 19 ceilings exceeded (by 6-327
//! bytes; `loadable-site` 927 against 600, whose slice is smallest) move to
//! the next 100 below 10,000 and the next 1,000 above, as do the features
//! example's startup chunk (7,136 bytes) and `app.editor.js` (5,702).
//!
//! The seqs over arrays and strings, and rest arguments, as class values
//! with slot-first dispatch (light-cljs-1010, 2026-11-15): indexed, chunked
//! and map seqs hold their storage, offset and kind in own keyed fields
//! (`viewOf`, `IndexedSeqCell` and the other cells), indexed and chunked
//! seqs have keyed `first`, `rest`, `next` and `seq` slots (and an indexed
//! seq `count`), a vector a `-seq` slot, and `first`, `rest`, `next` and
//! `seq` call them first. The readable runtime grows about 9,000 bytes, so
//! every none and prepare ceiling moves to the next 1,000 above its size.
//! Advanced, against master `cf613766`: programs that walk seqs grow by the
//! slots and wrappers, `rest-fn` 11,079 to 14,003 bytes, `mapv-count` 39,619
//! to 43,238, `keyword-lookups` 39,028 to 42,846, `constant-keys` 53,037 to
//! 57,117, `event-maps` 35,420 to 39,449, `callbacks` 47,923 to 51,743 and
//! `protocol-params` 53,046 to 56,865; printing programs by 173 bytes (the
//! view test and the seqs' `toJSON`). Each exceeded advanced ceiling, and
//! the features example's chunks, moves to the next 100 below 10,000 and
//! the next 1,000 above. Speed comes first (pl group 1661); the slots and
//! wrappers stay until light-cljs-1015 removes the older layers. Rebased
//! onto light-cljs-1025 (master `95bd2acc`), whose key adds four bytes to
//! every keyed name, the ceilings exceeded again move by the same rule.
//!
//! Keywords and symbols as class values (light-cljs-1011, 2026-11-15): a
//! keyword holds its namespace, name, `fqn` and hash in own keyed fields
//! (`KeywordCell`, `keywordOf`), a symbol likewise (`SymbolCell`,
//! `symbolOf`), the `keywords` and `symbols` stores are deleted, an array
//! map's keyword search compares by identity and `fqn` in loops of its own,
//! and each keyword constant at advanced carries its compile-time hash
//! (`rt$keyword(null, "a", -2123407586)`, 10 to 13 bytes a keyword). The
//! readable runtime grows about 4,000 bytes, so every none and prepare
//! ceiling moves to the next 1,000 above its size. Advanced, against master
//! `f91e0e62`: `keyword-only` and `keyword-case` past 4,000 (4,479 and
//! 4,575 bytes, with the keyword's two `IFn` arity entries), `constant-lookups`
//! 4,920, `atom-str` 3,636, `vector-only` 6,067, `set-only` 6,159 and
//! `map-only` 9,895 (`keywordOf`, `symbolOf` and `namedOf` replace two
//! `WeakMap` declarations), and the keyword-heavy programs by their
//! constants' hashes (`keyword-lookups` 44,624, `constant-keys` 59,003,
//! `defmulti` 12,107, `features` 88,550). Each exceeded advanced ceiling,
//! and the features example's startup (7,869)
//! and shared chunk (71,709), moves to the next 100 below
//! 10,000 and the next 1,000 above; the features startup's slack over the
//! application without its features moves from 200 to 300 bytes (201 over:
//! its constants' hashes).
//!
//! Lazy seqs, chunked conses, chunks and chunk buffers as class values with
//! keyed slots (light-cljs-1029, 2026-11-16): each holds upstream's fields as
//! own keyed fields (`LazySeqCell` and the other cells, `lazyOf` and the
//! other tests), a lazy seq and a chunked cons have keyed `first`, `rest`,
//! `next` and `seq` slots, a chunked cons and a vector's chunked seq the chunk
//! protocols, and a chunk `count`, `nth` and `-drop-first`; the private stores
//! `lazyValues`, `chunks` and `chunkBuffers` are deleted. The readable
//! runtime grows about 11,000 bytes, so every none and prepare ceiling moves
//! to the next 1,000 above its size. Advanced, against master `f91e0e62`:
//! programs that keep lazy seqs grow 3-5 KB, mostly the 17 slots with their
//! long keyed names (`seq-pipeline` 37,748 bytes, `mapv-count` 48,444,
//! `keyword-lookups` 48,027, `constant-keys` 62,372, `event-maps` 44,974,
//! `callbacks` 57,067, `protocol-params` 62,215, `features` 91,466),
//! and programs that keep no lazy seq less, through the kind tests the
//! older layers call (`rest-fn` 15,603, `eight-records` 45,910,
//! `map-printed` 15,275, `atom-str` 3,738). Each exceeded advanced ceiling, and the features
//! example's chunks (`app.main.js` 7,580, the shared chunk 74,804), moves to
//! the next 100 below 10,000 and the next 1,000 above. Speed comes first
//! (pl group 1661): `lazy-walk` and `reduce-map-inc` reach shadow-cljs's
//! times. Rebased onto light-cljs-1011 (master `5a99032e`), the ceilings
//! exceeded again move by the same rule (`seq-pipeline` 37,781, `mapv-count`
//! 48,549, `keyword-lookups` 48,733, `constant-keys` 63,142, `features`
//! 92,705, the features startup 8,139 and shared chunk 75,498).
//!
//! Vectors, subvectors, map entries and transient vectors holding their data
//! as their own keyed fields (light-cljs-1026, 2026-11-16): a vector's
//! `meta`, `cnt`, `shift`, `root`, `tail` and `hash` (`VectorCell`), a
//! subvector's view fields, a transient's store and ownership, each read by
//! its keyed name instead of one keyed field holding a store object, and the
//! metadata and hash helpers that tell these classes (`ownMeta`, `setMeta`)
//! and the class bases' `toJSON` (`unkeyedJSON`). The readable runtime grows
//! about 9,200 bytes, so every none and prepare ceiling moves to the next
//! 1,000 above its size. Advanced, against master `ef5c9df2`: `vector-only`
//! 6,051 to 6,824 bytes, `vector-printed` 12,633 to 13,607, `map-only` 9,895
//! to 10,313, `mapv-count` 48,549 to 51,189, `seq-pipeline` 37,781 to 38,381,
//! `constant-keys` 63,142 to 66,159, `features` 92,705 to 96,306, the
//! features example's startup 8,139 to 8,211 and shared chunk 75,498 to
//! 78,976; programs without a vector move by under 100 bytes
//! (`keyword-only` 4,479 to 4,553). Each
//! exceeded advanced ceiling and the shared chunk move to the next 100 below
//! 10,000 and the next 1,000 above. Speed comes first (pl group 1661):
//! `vec-conj` and `vec-assoc` gain 27-32%.
//!
//! Lists, conses, ranges, repeats, iterates, cycles, key and value seqs and
//! `re-seq`'s conses as class values with keyed slots (light-cljs-1030,
//! 2026-11-16): each holds its data in own keyed fields (`ListCell` and the
//! other cells, `cellOf`), with getters of its prototype where a step
//! computes `first` or `rest`, has keyed `first`, `rest`, `next` and `seq`
//! slots (and a list, the empty list and an integer range `count`), and the
//! `cells` store is deleted. The readable runtime grows about 16,000 bytes,
//! so every none and prepare ceiling moves to the next 1,000 above its size.
//! Advanced, against master `ef5c9df2`: programs that keep the cells grow
//! 4-5 KB, mostly the 43 slots with their long keyed names and the cell
//! classes (`seq-pipeline` 42,513 bytes, `mapv-count` 53,201,
//! `keyword-lookups` 52,734, `constant-literals` 38,594, `constant-keys`
//! 68,266, `event-maps` 50,318, `callbacks` 61,492, `protocol-params`
//! 66,807, `features` 98,251), others less through the cells' test the
//! older layers call (`rest-fn` 16,668, `eight-records` 47,522, `atom-str`
//! 4,047). Each exceeded advanced ceiling, and the features example's
//! chunks (`app.main.js` 8,290, the shared chunk 80,031, `app.editor.js`
//! 6,736), moves to the next 100 below 10,000 and the next 1,000 above.
//! Speed comes first (pl group 1661). Rebased onto light-cljs-1026 (master
//! `518edac7`), the ceilings exceeded again move by the same rule
//! (`seq-pipeline` 43,113, `mapv-count` 55,843, `constant-keys` 71,286,
//! `features` 102,500, the features startup 8,362, the shared chunk 83,512
//! and `app.editor.js` 7,521, with the sorted map seq's slots).

//! The vector classes' protocol masks, markers and slots (light-cljs-1028,
//! 2026-11-16): the prototypes of vectors, subvectors, map entries and
//! transient vectors hold their keyed protocol masks and `IFind` marker, and
//! subvectors, map entries and transient vectors keyed slots of their own
//! (17 slot units), which the slot-first wrappers, now effects of those
//! classes too, read. The readable runtime grows about 11,600 bytes, so
//! every none and prepare ceiling moves to the next 1,000 above its size.
//! Advanced, against master `518edac7`: programs with vectors grow by the
//! masks and the marker (`vector-only` 6,824 to 7,089, `vector-printed`
//! 13,607 to 13,875); programs with maps and no vector keep the entries'
//! slots and the slot-first wrappers of the operations they keep (`map-only`
//! 10,313 to 11,372, `map-printed` 17,768); programs with both, both
//! (`mapv-count` 51,189 to 52,079, `constant-keys` 66,159 to 67,882,
//! `features` 96,306 to 99,368, `eight-records` 49,226, `event-maps`
//! 48,356, `keyword-lookups` 51,528, `callbacks` 60,372, `protocol-params`
//! 65,824, `closed-params` 19,150, `defmulti` 13,720). Each exceeded advanced
//! ceiling moves to the next 100 below 10,000 and the next 1,000 above, and
//! so do the features example's shared chunk (78,976 to 81,888) and its
//! editor chunk (6,053). Rebased onto light-cljs-1030 (master `bb6e8fad`),
//! the ceilings exceeded again move by the same rule (`vector-printed`
//! 14,055, `map-printed` 17,947, `features` 105,562, the shared chunk 86,349
//! and the editor chunk 7,677).

//! Array, hash and sorted maps and the map transients as classes
//! (light-cljs-1012, 2026-11-16): each holds its payload in an own keyed
//! field (`ArrayMapCell` and the others, kinds 28 to 32), the maps have keyed
//! `-lookup`, `-count`, `-assoc`, `-contains-key?`, `-conj` and (array and
//! hash maps) `-dissoc` slots, `dissoc` and `contains?` slot-first wrappers,
//! and the `maps` store becomes a reader of the field. The readable runtime
//! grows about 15,000 bytes, so every none and prepare ceiling moves to the
//! next 1,000 above its size. Advanced, against master `1bd59e34`: programs
//! with maps keep the slots, their helpers and the wrappers (`map-only`
//! 12,893, `map-printed` 19,471, `constant-keys` 76,472, `event-maps`
//! 54,662, `keyword-lookups` 57,206, `mapv-count` 57,638, `protocol-params`
//! 70,286, `defmulti` 15,243, `features` 110,364, `features-direct`
//! 109,974, `rest-fn` 17,231, `constant-lookups` 5,187, `keyword-case`
//! 4,842, `set-only` 6,720); programs without maps keep the maps' and map
//! transients' tests that `count` and the store inventories read
//! (`vector-only` 7,282, `keyword-only` 4,746, `atom-str` 4,276). Each
//! exceeded advanced ceiling, and the features example's chunks
//! (`app.main.js` 8,710, the shared chunk 90,100, `app.editor.js` 8,418),
//! moves to the next 100 below 10,000 and the next 1,000 above. Speed comes
//! first (pl group 1661): the gate's small-map operations gain 77-90%.

//! Array maps' flat key, value arrays and hash maps' own fields
//! (light-cljs-1034, 2026-11-16): an array map holds `cnt` and `arr`, a hash
//! map `cnt`, `root`, `nil` and `lit`, with `arrayIndex`, the hash map slots'
//! own trie paths, `mapPairs`, `mapValueAt` and the kind hooks through which
//! `maps` still answers entries. The readable runtime grows about 10,000
//! bytes, so every none and prepare ceiling moves to the next 1,000 above
//! its size. Advanced, against master `6f5beb29`: programs with maps keep
//! the flat search and the field paths (`map-only` 14,270, `map-printed`
//! 21,365, `constant-keys` 79,187, `event-maps` 56,372, `keyword-lookups`
//! 58,701, `mapv-count` 58,229, `features` 113,208, `features-direct`
//! 112,818, `constant-lookups` 5,543, `keyword-case` 5,198, `set-only`
//! 7,130); programs without maps keep the longer `maps` and `mapOf` and the
//! array-map branches of `count` and `hash` (`vector-only` 7,519,
//! `keyword-only` 5,102, `atom-str` 4,513). Each exceeded advanced ceiling,
//! and the features example's chunks (`app.main.js` 9,067, the shared chunk
//! 92,494, `app.editor.js` 8,518), moves to the next 100 below 10,000 and the
//! next 1,000 above. Speed comes first (pl group 1661).

//! `seq` sends the empty list to the operation (light-cljs-1033,
//! 2026-11-16): its slot-first wrapper tests for the empty list, which it
//! and `noBranch` take as parameters. The readable runtime grows 473 bytes,
//! so each exceeded none and prepare ceiling moves to the next 1,000 above
//! its size. Advanced, against master `765a92e1`: programs that keep the
//! wrapper grow 17 bytes (`seq-pipeline` 43,724, `features` 110,381,
//! `features-direct` 109,991, `mapv-count` 57,655), under every ceiling.
//! Speed comes first (pl group 1661): lazy-walk and seq-walk-vec, timed as
//! a pair, recover the 10-23% they lost with light-cljs-1030.
//!
//! A symbol's metadata in its own keyed `meta` field (light-cljs-1031,
//! 2026-11-16): `SymbolCell` takes a fifth field and `ownMeta` answers
//! kind 21, so `meta` and `setMeta` read and write it. The readable runtime
//! grows 132 bytes, so each exceeded none and prepare ceiling moves to the
//! next 1,000 above its size (`event-maps` and `keyword-case`). Advanced,
//! against master `87e5db4b`: programs that keep `ownMeta` shrink 13 bytes,
//! its test of kinds 1 to 3 now one comparison (`mapv-count` 58,216,
//! `constant-keys` 79,225, `map-printed` 21,403, `vector-printed` 14,474),
//! `protocol-params` grows 12 (71,105), and `features` and
//! `features-direct` 28 (113,287 and 112,897), under every ceiling.

//! Map transients' own fields and the maps' field readers (light-cljs-1036,
//! 2026-11-16): a transient array map holds `editable` and `arr`, a
//! transient hash map `edit`, `cnt`, `root` and `nil`, each with its own
//! `assoc!`, `dissoc!`, lookup and `persistent!` paths; a public hash map
//! holds its count and nil entry as fields; `maps` no longer answers an
//! array or hash map's entries, and the older layers, the printer and
//! `mapIterator` read the fields. The readable runtime grows about 12,700
//! bytes, so every none and prepare ceiling moves to the next 1,000 above
//! its size. Advanced, against master `87e5db4b`: programs with maps keep
//! the transients' paths and the older layers' field branches (`map-only`
//! 14,613, `map-printed` 22,038, `constant-keys` 80,361, `event-maps`
//! 56,871, `keyword-lookups` 59,200, `eight-records` 52,117, `callbacks`
//! 66,174, `protocol-params` 72,063, `features` 116,130, `features-direct`
//! 115,740), and printing takes a map's flat pairs (`println-hi` 5,360);
//! programs without maps keep the shorter `maps` (`vector-only` 7,368,
//! `keyword-only` 5,005, `atom-str` 4,362, `set-only` 7,033). Each exceeded
//! advanced ceiling, and the features example's shared chunk (96,057),
//! moves to the next 100 below 10,000 and the next 1,000 above. Speed comes
//! first (pl group 1661).

//! The remaining runtime kinds as classes (light-cljs-1013, 2026-11-16):
//! hash and array sets, set transients, queues, reduced values, iterator
//! seqs, atoms, volatiles, delays, multimethods, UUIDs and eductions hold
//! their data in their own fields, told by their prototypes' keyed kinds,
//! where private stores and WeakMaps held them; `privateStore` is gone.
//! The readable runtime grows about 9,500 bytes (the classes, their tests
//! and comments), so every none and prepare ceiling moves to the next 1,000
//! above its size. Advanced, against master `31578fd9`: the stores'
//! inventories and the predicates now read each class's test, which a
//! WeakMap or a private store's one line was (`keyword-only` 5,433,
//! `vector-only` 7,774, `atom-str` 5,161, `set-only` 8,036, `map-only`
//! 15,103, `vector-printed` 15,018, `constant-keys` 81,738, `features`
//! 117,852). Each exceeded advanced ceiling, and the features example's
//! main chunk (9,617) and shared chunk (97,156), moves to the next 100
//! below 10,000 and the next 1,000 above. Speed comes first (pl group 1661).

//! Sorted maps as red-black trees of map entries (light-cljs-1035,
//! 2026-11-16): a sorted map holds `comp`, `tree` and `cnt`, its nodes are
//! map entries (`TreeNodeCell`), its `assoc`, `dissoc`, lookup, `find`,
//! `reduce-kv` and seqs are upstream's tree algorithms, and the generic
//! operations test its class (`isTreeMap`) where `maps` answered its entries.
//! The readable runtime grows about 17,000 bytes (none 656,581 for
//! `console-log-sum`, under 640,000 before), so every none and prepare
//! ceiling moves to the next 1,000 above its size. Advanced, against master
//! `4b52e192`: the features example, which builds sorted maps and sets,
//! passes its 118,000 ceiling by about 3,400 bytes, the tree's algorithms
//! (`features` 121,323, `features-direct` 120,933), and its editor chunk,
//! which holds the sorted collections, its 8,600 by 3,300 (11,941); each
//! moves to the next 1,000 above. Programs with other maps keep the class
//! tests (`map-only` 15,306, under its 16,000). Speed comes first (pl group
//! 1661).
//!
//! Records, program types and `reify` told by their keyed kinds
//! (light-cljs-1037, 2026-11-16): `decorateType` gives a type's prototype
//! its kind (64 to 66) and a record's its type; `typeFields`, `recordTypes`,
//! `reifyValues`, `recordMaps` and the record wrappers of `maps` and
//! `mapKinds` are deleted, and no constructor writes a brand (no
//! `TypeBrand`, `ClassBrand` or `defineClass` in release output). The
//! native tests that answered a record as a map read its kind
//! (`recordCells`), which every program whose `count`, `seq`, `get` or
//! metadata tests are kept now carries, records or not (`vector-only`
//! 7,813, `constant-lookups` 5,913, `constant-literals` 41,150,
//! `constant-keys` 82,214, `features-direct` 121,353); the record programs
//! keep `recordTypeOf` and the kinds but no brand (`record-protocol` 5,814,
//! `three-records` 5,809, `pure-defs` 5,714). Each exceeded advanced
//! ceiling moves to the next 100 below 10,000 and the next 1,000 above; the
//! readable runtime grows about 500 bytes, so every none and prepare
//! ceiling moves to the next 1,000 above its size (`keyword-case` 658,120).
//! Speed comes first (pl group 1661).

//! Sorted sets, vars, functions with metadata and string writers as classes
//! (light-cljs-1038, 2026-11-16): a sorted set holds its tree map, a var its
//! `val` and `sym`, a function with metadata its `afn` and a string writer
//! its `sb`, told by their prototypes' keyed kinds, where `sets.store`,
//! `setComparators`, `vars`, `metaFunctions` and `stringBufferWriters` held
//! them; `payloads` and `kindedStore` are deleted. The readable runtime
//! grows about 4,000 bytes (none 662,402 for `console-log-sum`), so every
//! none and prepare ceiling moves to the next 1,000 above its size.
//! Advanced, against master `c88f22ae`: the store inventories no longer read
//! a private-field kind, so most programs shrink about 450 bytes (`set-only`
//! 8,093 to 7,797, `keyword-only` 5,472 to 5,025, `map-only` 15,459 to
//! 15,057, `vector-only` 7,813 to 7,366); a string writer's class and kind
//! test outweigh its WeakSet (`println-hi` 5,360 to 5,537), and the features
//! example, whose sorted sets now hold the red-black tree map, grows about
//! 1,100 bytes (`features-direct` 121,353 to 122,470, `features` 122,860;
//! its editor chunk 12,435, the shared chunk 98,209). Each exceeded advanced
//! ceiling, and the shared chunk's, moves to the next 100 below 10,000 and
//! the next 1,000 above. Speed comes first (pl group 1661).
//!
//! The protocol dispatcher's receiver test as upstream's `x != null`
//! (light-cljs-1022, 2026-11-16): each method's own dispatcher tests
//! `r0 != null && r0[m1] != null` where it read `r0?.[m1] != null`, 7 bytes
//! more at `Advanced` per slot test (one per arity). Against master
//! `5cd32c66`: `eight-records` 52,245 to 52,252, and a two-method protocol
//! 316 to 330 bytes (`each_two_method_protocol_costs_at_most_its_budget`);
//! no ceiling moves. Workload 200k grows 2,940 bytes (420 slot tests), 130
//! gzipped (against `c88f22ae`).
//!
//! Bit operations as operators (light-cljs-1020, 2026-11-15): at advanced a
//! call of `bit-and`, `bit-or`, `bit-xor`, `bit-and-not`, `bit-not`, a shift,
//! `bit-clear`, `bit-flip`, `bit-set` or `bit-test` is the JavaScript operator
//! its runtime definition uses, and a program that only calls them keeps none
//! of their units. No fixture calls one, so every size and ceiling is
//! unchanged.
//!
//! Array calls as literals (light-cljs-1021, 2026-11-15): at advanced a call
//! of `array` is the JavaScript array literal of its arguments. No fixture
//! calls `array`, so every size and ceiling is unchanged.
//!
//! The classes' own `-equiv` and `-hash` (light-cljs-1014, 2026-11-16): the
//! keyed `-equiv` and `-hash` slots of vectors, map entries, subvectors,
//! maps, sets, keywords, symbols and the common seqs (`classEquiv`,
//! `mapEquiv`, `vectorHash`), the slot-first wrappers of `=` and `hash`
//! (`equalPair.slots`, `hash.slots`) and `hash`'s split
//! (`hashStructural`), kept wherever `=` is (a vector's host `equiv` keeps
//! it) or `hash` is, for the classes a program builds. The readable runtime
//! grows about 11,000 bytes, so every none and prepare ceiling moves to the
//! next 1,000 above its size. Advanced, against master `5cd32c66`: about
//! 1,000 to 2,000 bytes per program (`vector-printed` 15,062 to 16,075,
//! `map-printed` 22,905 to 24,424, `vector-only` 7,366 to 8,368, `map-only`
//! 15,057 to 17,029, `set-only` 7,797 to 9,252), about 4,000 for the
//! features example (`features-direct` 122,470 to 126,593, `features`
//! 126,983; the shared chunk 98,209 to 101,798, startup 9,752). Each
//! exceeded advanced ceiling, and the features chunks', moves to the next
//! 100 below 10,000 and the next 1,000 above. Speed comes first (pl group
//! 1661).
//!
//! Call-site entries of the slot-first operations (light-cljs-1016,
//! 2026-11-16): at advanced, where only values with a keyed kind can hold a
//! core protocol slot, `count`, `nth`, `get` (and `(:k m)`), `first`,
//! `rest`, `next`, `seq`, `peek`, `conj`, `assoc`, `dissoc` and `contains?`
//! call `countSite` and the others, one small unit each that the program
//! keeps with its operation. The readable runtime grows about 4,500 bytes
//! (none 679,318 for `console-log-sum`), so every none and prepare ceiling
//! moves to the next 1,000 above its size. Advanced, against master
//! `1bd3ac3d`: 154 to 328 bytes where a program calls them in its functions
//! (`mapv-count` 61,129 to 61,283, `callbacks` 68,143 to 68,297,
//! `keyword-lookups` 62,286 to 62,454, `constant-keys` 86,319 to 86,647), 939
//! for the features example (`features-direct` 126,593 to 127,532,
//! `features` 126,983 to 127,922; the editor chunk 12,674 to 12,828, the
//! reports chunk 4,578 to 5,371, the shared chunk unchanged at 101,798);
//! the other fixtures do not change. `features-direct`'s ceiling moves to
//! 128,000 and the reports chunk's to 6,000. Speed comes first (pl group
//! 1661).
//!
//! Keywords and symbols compare by their own `-equiv` slots (light-cljs-1040,
//! 2026-11-16): the slots (`keywordEquiv`, `symbolEquiv`), the keyword
//! searches' read of a program's method (`keywordProgramEquiv`), and `=`'s
//! slot-first wrapper wherever a program builds keywords and keeps `=` (a
//! keyword's host `equiv` keeps it). The readable runtime grows about 2,100
//! bytes, so every none and prepare ceiling moves to the next 1,000 above its
//! size (none 681,377 for `console-log-sum`). Advanced, against master
//! `15b858bf`: about 500 bytes for a program of keywords alone, which now
//! keeps the wrapper (`keyword-only` 5,090 to 5,599, `keyword-case` 5,186 to
//! 5,695, `constant-lookups` 5,531 to 6,040), 240 to 400 for one with maps
//! (`map-printed` 24,423 to 24,665, `set-only` 9,251 to 9,572, `event-maps`
//! 59,878 to 60,190, `protocol-params` 74,224 to 74,622); 511 for the
//! features example (`features-direct` 127,532 to 128,043, `features` 127,922
//! to 128,433; the startup chunk 9,953, the shared chunk 101,798 to 102,114).
//! Each exceeded advanced ceiling, and the features chunks', moves to the
//! next 100 below 10,000 and the next 1,000 above. Speed comes first (pl
//! group 1661).
//!
//! The old dispatch layers removed (light-cljs-1015, 2026-11-16): no kind
//! tables, branches or protocol wrappers, and the protocol lookup of
//! `extend-type` programs is `extendedMethod`. The readable runtime is
//! about 22,000 bytes smaller (none 659,486 for `console-log-sum`, where
//! 681,377 before light-cljs-1040's note; every none and prepare ceiling
//! keeps its value). Advanced, against master `0b473fdb` (the command line's
//! output): 75 to 5,640 bytes smaller where a program keeps the collections
//! (`map-printed` 24,685 to 23,955, `keyword-lookups` 62,785 to 58,586,
//! `constant-keys` 87,087 to 81,447, `callbacks` 68,515 to 64,662,
//! `protocol-params` 74,642 to 70,790); `rest-fn` 735 larger (19,447 to
//! 20,182, its slice taking other units); one byte
//! more for `keyword-case`, `keyword-only` and `constant-lookups`.
//! `rest-fn`'s ceiling moves to 21,000; no other ceiling moves.
//!
//! One store test for the runtime's values (light-cljs-1046, 2026-11-17):
//! the protocol lookup (`extendedMethod`), the `satisfies` hooks and the
//! store inventories (`storesPredicate`, `ownedProtocolValue`) read a
//! value's keyed kind once and test its class's store (`kindTable`, each
//! store naming its classes' kinds in `classKinds`) where they probed the
//! stores in turn (`payloadStore`), and the hash sets, queues and vectors
//! have the `-count`, `-seq`, `-conj`, `-lookup` and `-contains-key?` slots
//! they lacked. The readable runtime grows about 6,000 bytes (none 666,777
//! for `add`, where 660,822), under every none and prepare ceiling. At
//! advanced a program keeping an inventory keeps the stores' kinds and the
//! table, about 350 bytes (`keyword-case` 5,738, past 5,700, its ceiling
//! moving to 5,800).
//!
//! An array map's keyword search kept small, and `contains?` of an array map
//! without `get`'s layers (light-cljs-1041, 2026-11-17): `arrayIndexOf`,
//! `keywordLookupEquiv`, `arrayMapGet` and `arrayMapContains`, split off
//! or added so that V8 inlines a shared `-lookup` call's array map target
//! (docs/value-layout.md). The readable runtime grows 2,468 bytes (none
//! 669,245 for `add`), under every none and prepare ceiling. Advanced,
//! against master `e0168930`: about 290 bytes where a program keeps array
//! maps (`map-printed` 24,598 to 24,889, `keyword-lookups` 59,298 to
//! 59,590, `map-only` 16,888 to 17,176, `defmulti` 19,344 to 19,635), 350
//! for `constant-keys` (82,977 to 83,327) and the features example
//! (`features` 124,058 to 124,408), 62 for `set-only`. No ceiling moves.
//!
//! The side stores in the values' fields (light-cljs-1053, 2026-11-17):
//! metadata, cached hashes, a public constructor's slots, a subvector's
//! source and a vector's root token are the values' own keyed fields, where
//! `metadataValues`, `constructorHashes`, `publicCollectionSlots`,
//! `subvecSources`, `vectorRoots` and `canonicalVectorRoots` held them.
//! Advanced, against master `d8a28b21` (the command line's output): smaller
//! where a program keeps vectors, maps or sets without seqs (`vector-printed`
//! 16,455 to 16,344, `vector-only` 8,348 to 8,278, `set-only` 9,596 to
//! 9,527, `map-only` 17,196 to 17,145, `closed-params` 19,563 to 19,490,
//! `defmulti` 19,655 to 19,603); larger where it keeps seqs, metadata or
//! hashing, whose keyed field reads are longer than the minified store calls
//! they replace and whose units (`setMeta`, `metaFields`, `extraFields`,
//! `heldHash`, `holdHash`) are new (`rest-fn` 20,572 to 20,834,
//! `eight-records` 51,628 to 51,928, `seq-pipeline` 44,902 to 45,028,
//! `constant-literals` 40,725 to 40,851, `callbacks` 65,114 to 65,243). The
//! ceilings of `vector-only` (8,300) and `closed-params` (20,000) move down;
//! no ceiling moves up.
//!
//! Vectors and maps reduce by their own slots (light-cljs-1055,
//! 2026-11-17): the `-reduce` slots of vectors, subvectors and array and
//! hash maps, the `-kv-reduce` slots of vectors, subvectors and the three
//! maps, and the slot-first wrappers of `reduce` and `reduce-kv`. The
//! readable runtime grows about 9,800 bytes (none 678,661 for `add`, where
//! 668,899), under every none and prepare ceiling. Advanced, against master
//! `51f38606` (the command line's output): 1,158 to 1,908 bytes where a
//! program reduces vectors or maps (`keyword-lookups` 59,725 to 61,633,
//! `eight-records` 51,748 to 52,906, `event-maps` 57,493 to 58,752,
//! `mapv-count` 58,781 to 59,940, `callbacks` 65,243 to 66,403,
//! `protocol-params` 71,406 to 72,567), mostly the slots' long keyed names;
//! every other fixture is unchanged. No ceiling moves.
//!
//! Short keyed names at advanced (light-cljs-1048, 2026-11-17): the output
//! writer writes every keyed name `$`, the key and a code of one or two
//! letters, the most used first, and the protocol slots' and markers' codes
//! in a table `protocolSlot` reads (docs/value-layout.md, section 1, "Short
//! names at `:advanced`"). None and prepare keep whole names; the readable
//! runtime grows about 1,000 bytes (`protocolSlotCodes` and its comment,
//! and the mask and field names once built by concatenation written whole),
//! under every none and prepare ceiling (`console-log-sum` 678,821 to
//! 679,823). Advanced, against master `4fbc0e65` (the command line's
//! output): 359 to 7,483 bytes smaller where a program keeps the
//! collections (`vector-printed` 16,344 to 15,549, `map-printed` 24,883 to
//! 23,313, `constant-keys` 83,362 to 75,879, `mapv-count` 59,940 to 54,436,
//! `protocol-params` 72,567 to 67,629); `record-protocol` and
//! `three-records` 20 bytes larger (5,832 to 5,852, 5,827 to 5,847), whose
//! few keyed names save less than the table's lookup costs. Every advanced
//! ceiling, and the features example's chunks (`app.main.js` 9,079, the
//! shared chunk 92,527, `app.editor.js` 11,595, `app.reports.js` 5,002),
//! moves to the next 100 above its size below 10,000 and the next 1,000
//! above.
//!
//! `ownMethod` tests a runtime class's kind before its mask bit again
//! (light-cljs-1056, 2026-11-17), and a program that writes a core
//! protocol method's member by name keeps `extendType`: the readable runtime
//! grows 536 bytes (`console-log-sum` 679,823 to 680,359), the comments.
//! Advanced, against master `1df0a071`: 20 bytes more where a program keeps
//! `ownMethod` (`eight-records` 49,829 to 49,849, `closed-params` 18,779 to
//! 18,799, `callbacks` 61,606 to 61,626, `protocol-params` 67,629 to
//! 67,649); every other fixture is unchanged (none writes a core member by
//! name). No ceiling moves.
//!
//! A hash map's keyword keys compared by their spelling (light-cljs-1057,
//! 2026-11-17): the trie's key test (`hashKeyTest`) reads a keyword key's
//! program `IEquiv` method through `keywordProgramEquiv`, which a program
//! keeping hash maps now keeps too, and which reads the key's slot without
//! the protocol lookup. The readable runtime grows 1,909 bytes (none
//! 682,108 for `add` on master `1ffb4d00`, where 680,199), mostly comments;
//! with light-cljs-1056's 536 it passes 28 fixtures' none and prepare
//! ceilings, which move to the next 1,000 above the larger size.
//! Advanced, against master `1df0a071`: 267 bytes where a program keeps
//! hash maps (`constant-keys` 75,879 to 76,146, `features` 116,385 to
//! 116,652, `features-direct` 115,995 to 116,262), 7 smaller where it keeps
//! the keyword searches of array maps (`map-printed`, `keyword-lookups`,
//! `event-maps`, `map-only`, `set-only`, `defmulti`); every other fixture
//! is unchanged. The ceilings of `constant-keys` (77,000) and
//! `features-direct` (117,000) move to the next 1,000 above.
//!
//! Declarations where they are defined (light-cljs-1051, 2026-11-17): at
//! advanced a single file declares each top-level global where it defines
//! it, `var g3 = ...;`, rather than in a `let` list up front
//! (docs/advanced-optimization.md, "Declarations where they are defined").
//! Against master `1df0a071`, up to 62 bytes smaller raw where a program
//! defines several globals (`eight-records` 49,829 to 49,767, `features`
//! 116,385 to 116,353, `callbacks` 61,606 to 61,590), 0 to 9 elsewhere. No
//! ceiling crosses its rounding, so none moves; the re-frame apps save 0.6%
//! to 1.2% of their gzipped bundles. None and prepare are unchanged, and so
//! are the features example's chunks, a chunked build.
//!
//! `conj` and `assoc` of a vector, a subvector and a map entry, and `conj`
//! of a set, copy the metadata field without `meta`'s lookup where no
//! `extend-type` or `specify!` is kept (`conjMeta`, light-cljs-1060): the
//! readable runtime is 2 bytes smaller (`console-log-sum` 682,268 to
//! 682,266). Advanced, against master `c49a646a`: `constant-keys` 76,146 to
//! 76,248, which now keeps `conjMeta` (102 bytes) for its vectors' `assoc`
//! and its sets' `conj` slots, and `mapv-count` 54,436 to 54,435; every
//! other fixture is unchanged. No ceiling moves.
//!
//! A program protocol's slots named from the text (light-cljs-1058,
//! 2026-11-18): at advanced each `protocolMethod` names its slots and a
//! method entry that does not name its dispatcher names its slot, so the
//! slot-code table `protocolSlot` reads holds only the core protocols' slots
//! and the markers (docs/value-layout.md, section 1, "Short names at
//! `:advanced`"). The readable runtime grows 640 bytes, the code that takes
//! the names and its comments (`console-log-sum` 682,908 at none), past 11
//! fixtures' none and prepare ceilings, which move to the next 1,000 above
//! the larger size. Advanced, against master `8c21c1ae` (the command
//! line's output): 53 to 128 bytes larger where a program keeps the
//! protocol machinery, the names and the code that takes them, which
//! outweigh the few slots a small program takes out of the table
//! (`record-protocol` 5,852 to 5,907, `three-records` 5,847 to 5,900,
//! `pure-defs` 5,744 to 5,838, `eight-records` 49,787 to 49,915,
//! `closed-params` 18,791 to 18,885, `callbacks` 61,610 to 61,704);
//! `protocol-params` 67,641 to 67,625; every other fixture is unchanged.
//! The re-frame apps' bundles shrink 61 to 122 bytes after gzip
//! (benchmarks/re-frame-vs-shadow.md). The advanced ceilings of
//! `record-protocol` (6,000) and `pure-defs` (5,900) move to the next 100.
//!
//! Keyword constants without their hash (light-cljs-1050, 2026-11-17): at
//! advanced a keyword constant is `rt$keyword(null, "a")` again, and its
//! first `hash` computes its hash and caches it in its field, as below
//! advanced; the runtime adds nothing to compute it. The readable runtime is
//! 5 bytes smaller (`console-log-sum` 682,268 to 682,263). Advanced, against
//! master `8c21c1ae` (the library's output): 9 to 161 bytes smaller where a
//! program has keyword constants (`keyword-case` 5,373 to 5,320,
//! `keyword-only` 5,283 to 5,253, `constant-lookups` 5,706 to 5,697,
//! `keyword-lookups` 55,688 to 55,612, `event-maps` 53,343 to 53,259,
//! `constant-keys` 76,142 to 76,067, `defmulti` 18,243 to 18,189,
//! `features` 116,620 to 116,459, `features-direct` 116,230 to 116,069),
//! and the features example's startup (9,079 to 9,039) and shared chunk
//! (92,794 to 92,673). By the rounding rule (the next 100 above a size below
//! 10,000, the next 1,000 above) only `constant-lookups`' ceiling moves
//! down, to 5,700; the others already sit at their rounding. The features
//! startup is 236 bytes over the application without its features, so its
//! slack stays 300.
//!
//! Sets and indexed seqs reduce by their own slots (light-cljs-1059,
//! 2026-11-17): the `-reduce` slots of hash and array sets and of indexed
//! seqs and the walk they share (`itemsReduce`). The readable runtime grows
//! 4,108 bytes (`console-log-sum` 682,901 to 687,009 at none, on master
//! `a33f6fef`), mostly the slots' comments and the measurements of the
//! classes left out, past every none and prepare ceiling, which moves
//! to the next 1,000 above the larger size. Advanced, against `a33f6fef`'s
//! runtime: 653 to 1,197 bytes more where a program reduces a set or an
//! indexed seq (`event-maps` 53,259 to 53,913, `keyword-lookups` 55,612 to
//! 56,265, `mapv-count` 54,435 to 55,089, `callbacks` 61,676 to 62,371,
//! `eight-records` 49,897 to 50,592, `protocol-params` 67,607 to 68,303,
//! `constant-literals` 37,138 to 38,212, `seq-pipeline` 41,221 to 42,296,
//! `features` 116,459 to 117,656, `features-direct` 116,069 to 117,266),
//! and the features example's shared chunk 92,673 to 93,871; every other
//! fixture is unchanged. The nine exceeded advanced ceilings and the shared
//! chunk's move to the next 1,000 above (benchmarks/runtime-vs-shadow.md,
//! light-cljs-1059, has the re-frame apps' gzip growth, 0.07 to 0.20%).
//!
//! A runtime class only extended keeps its prototype alone (light-cljs-1052,
//! 2026-11-18): at advanced, an `extend-type` or `extend-protocol` of a
//! runtime class reads it as `NameUse::Extend`, and where nothing else keeps
//! the class whole the slice keeps its prototype and none of its slots
//! (`RuntimeManifest::reached_by_uses`, "prototype only"). No fixture
//! extended a runtime class, so every size is unchanged and no ceiling
//! moves. The new fixture `extended-classes` pins it: cljs.spec's
//! `specize*` pattern, which extends the hash set it builds and the sorted
//! set it never builds, is 72,416 bytes at advanced against 109,065 at
//! master `a33f6fef` and `dbf91703` (the command line's output); its ceilings
//! are the next 1,000 above, 73,000, and 689,000 at none and prepare
//! (688,175, after light-cljs-1059's comments).
//!
//! Another copy's ex-info by its keyed kind (light-cljs-1044, 2026-11-18):
//! `ExceptionInfo`'s prototype holds kind 48, which `ex-data`, `ex-cause`,
//! `Throwable->map` and the printer test where they tested `instanceof`.
//! The readable runtime grows 424 bytes (`console-log-sum` 687,009 to
//! 687,433 at none, on master `a123bd44`), past fifteen fixtures' none or
//! prepare ceilings, which move to the next 1,000 above the larger size.
//! Advanced is unchanged in every fixture, none of which makes an ex-info;
//! benchmarks/runtime-vs-shadow.md, light-cljs-1044, has the re-frame
//! apps' bundles (+28 bytes raw, -39 and +64 bytes gzip).
//!
//! Map seqs and key seqs step through slots (light-cljs-1064, 2026-11-18):
//! the `-first`, `-rest`, `-next`, `-seq` slots of the three map seqs and an
//! array map seq's `-count`, the first, seq and count methods they share
//! with indexed seqs (`viewFirstSlot`, `viewSeqSlot`, `viewCountSlot`), the
//! key and value seqs' getters calling those slots, and a sorted map seq's
//! list stack peeked and pushed without lookups where a program can extend
//! lists. The readable runtime grows 5,045 bytes (`console-log-sum` 687,433
//! to 692,478 at none, on master `5bb838cd`), mostly comments, past every
//! none and prepare ceiling, which moves to the next 1,000 above the larger
//! size. Advanced, against `5bb838cd` (the command line's output): 357 to
//! 361 bytes more where a program keeps a map seq (`event-maps` 53,913 to
//! 54,271, `keyword-lookups` 56,265 to 56,623, `eight-records` 50,836 to
//! 51,193, `callbacks` 62,371 to 62,730, `protocol-params` 68,303 to
//! 68,664), 576 where it also keeps the key seqs (`constant-keys` 76,169 to
//! 76,745), 1,064 for `features` (117,656 to 118,720) and
//! `features-direct`, whose shared chunk is 93,871 to 94,445 and editor
//! chunk, which keeps the sorted maps' seqs, 11,597 to 12,096, and 24 where
//! a program keeps only indexed seqs, whose three methods are now named
//! (`seq-pipeline`, `mapv-count`, `extended-classes` and others). The
//! exceeded advanced ceilings (`event-maps`, `features`,
//! `features-direct`) and the two chunks' move to the next 1,000 above
//! (benchmarks/runtime-vs-shadow.md, light-cljs-1064, has the re-frame
//! apps' gzip growth).
//!
//! Transients take `conj!`, `assoc!` and `persistent!` from slots
//! (light-cljs-1065, 2026-11-18): the `-conj!`, `-assoc!` and
//! `-persistent!` slots of the transient vectors, maps and sets, the
//! slot-first `conjBangOwn`, `assocBangOwn` and `persistentBang`, and a map
//! transient's `conj!` of a map entry through its slots (`assocEntry`). The
//! readable runtime grows 5,465 bytes (`console-log-sum` 692,478 to 697,943
//! at none, on master `a397e0c9`), past every none and prepare ceiling,
//! which moves to the next 1,000 above the larger size. Advanced, against
//! `a397e0c9`'s runtime: 507 bytes more where a program conjoins onto a
//! transient vector (`mapv-count` 55,113 to 55,620) and 1,031 where it also
//! keeps the map and set transients (`features` 118,720 to 119,751 and
//! `features-direct` 118,330 to 119,361, whose shared chunk is 94,445 to
//! 95,476); every other fixture is unchanged. The two exceeded advanced
//! ceilings and the shared chunk's move to the next 1,000 above
//! (benchmarks/runtime-vs-shadow.md, light-cljs-1065, has the re-frame
//! apps' gzip growth, 0.02 to 0.21%).
//!
//! The seq walk shaped as upstream's (light-cljs-1054, 2026-11-18): `seq`,
//! `first`, `rest` and `next` call a value's slot, then upstream's path for
//! nil and host values (`seqOf` and the others), without the slot-first
//! wrappers and the slots' `noBranch` guards; the maps, the sorted set, a
//! reversed seq and an iterator seq gained the slots their old paths gave.
//! The old paths stay for the compile-time runtime and the shared-heap
//! profile (`seqPath`, `firstPath`, `restPath`, `nextPath`), so the
//! readable runtime grows 6,211 bytes (`console-log-sum` 692,478 to 698,689
//! at none, on master `a397e0c9`), past every none and prepare ceiling,
//! which moves to the next 1,000 above the larger size. Advanced, against
//! `a397e0c9`: 1,460 to 2,503 bytes smaller where a program walks seqs
//! (`seq-pipeline` 42,320 to 40,072, `rest-fn` 19,239 to 17,779,
//! `callbacks` 62,730 to 60,229, `protocol-params` 68,664 to 66,161,
//! `features` 118,720 to 116,584), unchanged elsewhere; those advanced
//! ceilings move down (benchmarks/runtime-vs-shadow.md, light-cljs-1054,
//! has the re-frame apps' bundles); the shared chunk of `features-direct`
//! (92,312 bytes on `a397e0c9`, 93,347 with light-cljs-1065's transients) moves
//! to 94,000; on master `4faa3383` every none and prepare size is 6,211
//! bytes larger than light-cljs-1065's (`console-log-sum` 704,154).
//!
//! Cheaper type and keyword setup at load (light-cljs-1017, 2026-11-18):
//! `installMethods` keeps only the `Object/` methods' entries and reads its
//! entries by index, `decorateType` finds a marker's mask bit in
//! `protocolMasks`, the record markers' entries are one shared unit
//! (`recordMarkers`), `mungeMember` replaces by a pattern, `keyword` finds a
//! literal without a namespace by its name (one that starts with a slash
//! takes another), and a host type's `extend-type` implementation calls
//! its method without a rest array. The readable runtime grows 2,150 bytes
//! (`console-log-sum` 704,154 to 706,304 at none, on master `9c9479b3`),
//! past every none and prepare ceiling, which moves to the next 1,000 above
//! the larger size. Advanced: 21 bytes more where a program builds keyword
//! constants (`keyword-only` 5,253 to 5,274), and 213 to 265 bytes more
//! where a program defines records (`record-protocol` 5,907 to 6,172,
//! `pure-defs` 5,838 to 6,103, `three-records` 5,900 to 6,165,
//! `closed-params` 18,845 to 19,061, `eight-records` 49,205 to 49,421,
//! `extended-classes` 70,633 to 70,846), for the indexed loops and
//! `protocolMasks`, which a record program now keeps.
//! The four exceeded advanced ceilings move to the next 100 (1,000 from
//! 10,000) above.
//! The lookups shaped as upstream's (light-cljs-1066, 2026-11-18): `count`,
//! `nth`, `get`, `contains?` and `peek` call a value's slot, then upstream's
//! path (`countOf` and the others), without their slot-first wrappers and
//! their slots' `noBranch` guards; the views, the sorted map seq and set,
//! chunk buffers, transients, integer ranges, lists and queues gained the
//! slots their old paths gave. The old paths stay for the compile-time
//! runtime and the shared-heap profile, so the readable runtime grows about
//! 12,000 bytes (`console-log-sum` 704,154 to 716,438 at none), past every
//! none and prepare ceiling, which moves to the next 1,000 above the larger
//! size. Advanced: each ceiling moves to the size rounded up (to 100 below
//! 10,000 bytes, else to 1,000) where that is lower (`seq-pipeline` 38,954,
//! `rest-fn` 16,595, `callbacks` 57,919, `protocol-params` 63,887,
//! `eight-records` 46,800, `extended-classes` 68,812, `features` 116,863);
//! `constant-lookups` grows past its ceiling, 5,720 bytes, as its `get` of a
//! value of unknown kind keeps `get`, `get2`, `get3` and `getOf` where the
//! wrappers kept the old `get`: its ceiling moves up to 5,800. The wrappers
//! that answer a runtime class's value whose prototype slot a program cleared
//! (`slotWrites`, kept at advanced only where the program writes a core
//! protocol method's member by name) add about 2,400 bytes to the readable
//! runtime (`console-log-sum` 720,976 at none on master `2af3dfcb`, with
//! light-cljs-1017), and every none and prepare ceiling moves to the next
//! 1,000 above again; no advanced size moves for them. On `2af3dfcb` the
//! advanced ceilings this lowered that light-cljs-1017's record programs
//! exceed move to the size rounded up as above (`eight-records` 47,016,
//! `callbacks` 58,135, `protocol-params` 64,103, `extended-classes` 69,025),
//! all below master's (benchmarks/runtime-vs-shadow.md, light-cljs-1066,
//! has the re-frame apps' bundles).
//!
//! Types, protocols and hierarchies defined at load as cheaply as upstream's
//! (light-cljs-1069, 2026-11-18): `installMethods` defines a protocol entry's
//! slot before the records only `Object/` and `IFn/-invoke` methods need,
//! a record type at `:advanced` takes its 15 markers as their masks
//! (`recordMarkerMasks`) and its kit as `defineType`'s last argument, a
//! program protocol's dispatcher is written as the function itself with its
//! slots named (`protocolMiss`), and `derive` conjoins. The readable runtime
//! grows about 4,200 bytes (`console-log-sum` 725,134 at none, on master
//! `c786677b`), past every none and prepare ceiling, which moves to the
//! next 1,000 above the larger size. Advanced: record programs grow 555
//! bytes (`record-protocol` 6,172 to 6,727, `pure-defs` 6,103 to 6,658,
//! `three-records` 6,165 to 6,720; `eight-records` 47,707,
//! `extended-classes` 69,492 and `callbacks` 58,697 stay under theirs) for
//! the one loop, the record masks and the direct dispatchers, which spell
//! each slot twice as upstream's do. The three exceeded advanced ceilings
//! move to the next 100 above.
//!
//! The updates shaped as upstream's (light-cljs-1068, 2026-11-18): `conj`,
//! `assoc`, `dissoc`, `disj` and `pop` call a value's slot, then upstream's
//! path (`conjOf` and the others), without their slot-first wrappers and
//! their slots' `noBranch` guards; the lists, sets, vectors, a map entry and
//! the queue gained the `-conj`, `-disjoin` and `-pop` slots their old paths
//! gave. The old paths stay for the compile-time runtime and the shared-heap
//! profile, so the readable runtime grows about 12,900 bytes
//! (`console-log-sum` 738,044 at none, on master `f9d52a2c`), past every
//! none and prepare ceiling, which moves to the next 1,000 above the larger
//! size. Advanced: each ceiling moves to the size rounded up as above where
//! that is lower (`constant-keys` 72,706, `mapv-count` 51,921, `features`
//! 115,751, `features-direct` 115,361, both from 117,000); no advanced size
//! grows against master. A vector's variadic `assoc` keeps the old path's
//! one metadata read (`kindHooks.assocVector`, about 450 bytes where a
//! program keeps `assoc` and vectors). The features example's chunks are
//! about 1,050 bytes smaller together: its shared chunk 93,836 to 92,179
//! (ceiling 93,000), while `app.reports.js` grows 4,830 to 5,458 (ceiling
//! 5,500), as the hash map's statics (`EMPTY`, `fromArray`), which its `#{}`
//! keeps, are placed in it rather than in the shared chunk.
//!
//! The seq walk's slots a program clears answering as master's
//! (light-cljs-1071, 2026-11-19): four more `slotWrites` wrappers, of
//! `seqOf`, `firstOf`, `restOf` and `nextOf`, kept at advanced only where
//! the program writes a core protocol method's member by name. The readable
//! runtime grows 2,271 bytes (`console-log-sum` 740,315 at none, on master
//! `76061c5c`), past every none and prepare ceiling, which moves to the next
//! 1,000 above. No advanced size moves.
//!
//! The reductions shaped as upstream's (light-cljs-1072, 2026-11-19): `reduce`
//! and `reduce-kv` call a value's slot, then upstream's path (`reduceOf`,
//! `reduceKvOf`), without their slot-first wrappers and their slots'
//! `noBranch` guards; a vector's chunked seq, the integer range, repeat,
//! iterate, cycle, an array chunk, a lazy seq and a key or value seq gained
//! the `-reduce` slots their old path's walks gave. The old paths stay for
//! the compile-time runtime and the shared-heap profile, so the readable
//! runtime grows about 9,600 bytes (`console-log-sum` 740,315 to 749,904 at
//! none, on master `4e76d385`),
//! past every none and prepare ceiling, which moves to the next 1,000 above
//! the larger size. Advanced: the upstream-shaped dispatch (`reduce2`,
//! `reduce3`, `reduceOf`, the slot factory `reduceSlot`) costs about 110 to
//! 210 bytes in programs that keep the recognisers for their other
//! operations anyway: `mapv-count` 51,926 on master `76061c5c` to 52,069,
//! past its 52,000 ceiling, which moves to 53,000, and the features example
//! built as one module 117,689 to 117,902 (its ceiling of 116,000 holds,
//! 115,996). `eight-records` moves to 47,000. The re-frame apps' whole
//! bundles shrink 135 to 355 bytes after gzip (benchmarks/runtime-vs-shadow.md,
//! light-cljs-1072).
//!
//! Count, meta and the predicates without the extended lookup (light-cljs-1070,
//! 2026-11-19): `meta` calls a `-meta` slot (`metaSlot`) that 29 collection and
//! seq classes share, `count` of a string or an array reads its length first,
//! and 30 runtime classes declare their protocols by keyed masks
//! (`classMasks`, one call each), which the `satisfies` hooks read first. The
//! readable runtime grows about 12,850 bytes (`console-log-sum` 740,315 to
//! 753,162 at none on master `4e76d385`, 762,751 on master `c09a181b`), past
//! every none and prepare ceiling, which moves to the next 1,000 above.
//! Advanced: a program that keeps collections grows 300 to 2,600 bytes, the
//! masks and slots of the classes it keeps (on `4e76d385`: `mapv-count`
//! 51,921 to 53,035, `constant-keys` 72,706 to 74,358, `features` 115,751 to
//! 117,675); the exceeded advanced ceilings move to the next 100 above. The
//! features example's shared chunk grows to 93,965 (ceiling 93,000 to
//! 94,000). Its review's fix (`metaSlot` answers a method on
//! `Object.prototype` first) and its second attempt (`deref` of an atom
//! without the lookup, the `satisfies` hook's steps for nil, primitives and
//! program types, `hostSatisfies`) grow the readable runtime to 765,178
//! (`console-log-sum` at none), so every none and prepare ceiling moves up
//! (to the next 1,000 above a variant 2,000 bytes larger that the review
//! dropped); at advanced a program that extends types grows about 250 bytes
//! (`extended-classes` 71,656, ceiling 71,400 to 71,700) and one that keeps
//! collections 100 to 400 (`mapv-count` 53,285, `features` 118,051), each
//! exceeded ceiling moving to the next 100 above; the shared chunk is
//! 94,066 (ceiling 94,100).
//!
//! `=` and `hash` shaped as upstream's (light-cljs-1074, 2026-11-19): each
//! calls a value's keyed slot, then upstream's path (`equalPairOf`,
//! `hashOf`), without the slot-first wrappers; the classes' own comparisons
//! and hashes call their class's directly, where they asked the old path's
//! recognisers (`structuralEqualPair`, `hashStructural`) to tell the class
//! again.
//! The old paths stay for the compile-time runtime and the shared-heap
//! profile, so the readable runtime grows 9,708 bytes (`console-log-sum`
//! 749,904 to 759,612 at none, on master `c09a181b`), past every none and
//! prepare ceiling, which moves to the next 1,000 above the larger size.
//! Advanced, measured by the command line: most programs that compare or
//! hash drop the recognisers, 30 to 1,170 bytes (`keyword-case` 5,364 to
//! 4,192, `keyword-only` 5,297 to 4,125, `vector-only` 7,724 to 6,769,
//! `constant-lookups` 5,720 to 4,643, `closed-params` 18,883 to 17,968,
//! `mapv-count` 52,069 to 51,926); two grow, `map-printed` 23,458 to 23,499
//! and `atom-str` 4,679 to 4,693 (their ceilings hold): `.equiv` of an
//! identical seq or queue without a slot compares its items in
//! `objectEquiv`, which every program with a seq or a map keeps (the Sol
//! review's finding). Each ceiling moves to the size rounded up where that
//! is lower: `keyword-case` and `keyword-only` 4,200, `vector-only` 6,800,
//! `constant-lookups` 4,700, `set-only` 8,900, `closed-params` 18,000 and
//! `mapv-count` 52,000.
//! Rebased onto light-cljs-1070 (`8903e693`): the readable runtime is
//! 774,886 bytes (`console-log-sum` at none), every none and prepare ceiling
//! moving to the next 1,000 above; no advanced size passes its ceiling, and
//! the ceilings move to the size rounded up where that is lower:
//! `keyword-case` and `keyword-only` 4,300, `vector-only` 6,800,
//! `constant-lookups` 4,800, `constant-literals` 35,000, `seq-pipeline`
//! 39,000, `event-maps` 52,000, `protocol-params` 66,000 and `features`
//! 118,000. The features example's shared chunk grows 94,066 to 94,176
//! (ceiling 94,100 to 94,200): it keeps `=` and `hash` with the classes the
//! two features share, whose upstream-shaped fallbacks (`equalPairOf`,
//! `hashOf`) are larger than the recognisers it dropped there.
//!
//! `into` without a lookup per call (light-cljs-1067, 2026-11-20): the
//! vector, array map, hash map and hash set classes hold `-as-transient` and
//! `-with-meta` slots (`asTransientSlot`, `withMetaSlot`, one assignment unit
//! each per class), `transient` and `-with-meta` call them first, and `into`
//! is one function per arity with a mask-bit test of its target. The
//! readable runtime grows 5,482 bytes (`console-log-sum` at none 765,178 to
//! 770,660 on `8903e693`; 780,368 rebased onto light-cljs-1074's
//! `b86a231e`), past every none and prepare ceiling, which moves to the next
//! 1,000 above. Advanced: a program that keeps `into`, `mapv` or `vec` grows
//! about 700 to 1,000 bytes (on `8903e693`: `mapv-count` 53,285 to 53,985,
//! of which `into` +146, `-with-meta`'s slot-first entry +148, `transient`
//! +91, the two slots 188 and the vector's two slot units 94; `features`
//! 118,051 to 119,042), each exceeded advanced ceiling moving to the next
//! 100 above (`mapv-count` 53,900, `features` 119,000, `features-direct`
//! 118,600); others move by 0 to 33 bytes. The features example's shared
//! chunk, which holds the two features' `into` and `-with-meta`, grows 94,176
//! to 95,167 on `b86a231e` (ceiling 94,200 to 95,200). Its review's fix
//! (`withMetaSlot` reads the value's `meta` once, `copyWithMeta`'s
//! `compared`) adds 194 bytes readable (`console-log-sum` 780,562 on
//! `b86a231e`; `defmulti`, `direct-site`, `event-maps`, `keyword-case` and
//! `multi-arity` pass their none and prepare ceilings, which move to the next
//! 1,000 above) and 11 at advanced (`mapv-count` 53,852, `features` 118,978,
//! the shared chunk 95,178).
//!
//! The predicates and the remaining lookups (light-cljs-1075, 2026-11-20):
//! `coll?`, `seq?`, `sequential?`, `vector?`, `map?`, `set?`, `empty?`,
//! `chunked-seq?` and `implementsSeq` read their protocol's mask bit first,
//! `empty?` calls `-count` (`countSlot`), `deref` a program type's `-deref`,
//! and the lookups gain a few steps with their comments. The readable runtime
//! grows 6,422 bytes (`console-log-sum` at none 780,562 to 786,984 on
//! `86dc4aa0`), past every none and prepare ceiling, which moves to the next
//! 1,000 above. Advanced: a program that keeps a front grows 121 to 395 bytes
//! (`event-maps` 51,914 to 52,038, `atom-str` 4,770 to 4,891, `mapv-count`
//! 53,852 to 53,975, `callbacks` 59,127 to 59,497, `protocol-params` 65,888
//! to 66,283, `features` 118,978 to 119,211, `features-direct` 118,588 to
//! 118,821; `eight-records` 47,506 and `closed-params` 18,202, where
//! `empty?` takes about 130) and `extended-classes`, which keeps the lookup,
//! 566 (71,485 to 72,051), each exceeded ceiling moving to the next 100 above.
//! The features example's shared chunk grows 95,178 to 95,369 (ceiling
//! 95,400) and `app.reports.js` passes 5,500 (5,505, ceiling 5,600).
//!
//! `into` onto other collections through `-conj` (light-cljs-1076,
//! 2026-11-20): `into` without a transducer conjoins each item onto a target
//! that is no editable collection through its `-conj` slot (`conjOwn`, with
//! `conjRead` for a program type's slot and one that holds no method), and a
//! map's `-conj` reads an entry's items through its `-nth` slot (`entryNth`,
//! with `entryNthRead`, and their `slotWrites` wrappers, which pass over a
//! runtime class's method lent by name to an atom). The readable runtime
//! grows 5,388 bytes (`console-log-sum` at none 786,984 to 792,372 on light-cljs-1075's
//! `d87d84a1`), past every none and prepare ceiling, which moves to the next
//! 1,000 above. Advanced: a program that keeps `into` grows by `conjOwn` and
//! `conjRead`, 375 bytes (`mapv-count` 53,975 to 54,350, ceiling 54,400),
//! and one that also conjoins onto a map with protocol machinery by
//! `entryNth` and `entryNthRead`, 823 in all (`features` 119,211 to 120,034,
//! `features-direct` 118,821 to 119,644, ceilings 120,100 and 119,700); no
//! other advanced size passes its ceiling. The features example's shared
//! chunk, which holds the two features' `into` and map updates, grows 95,369
//! to 96,195 (ceiling 95,400 to 96,200).
//!
//! The fallbacks' shared steps (light-cljs-1078): the upstream-shaped paths
//! (`assocOf`, `nthOf` and the others) read a runtime class's slot and the
//! protocol lookup through one function (`fallbackMethod`) and throw through
//! another (`unsupported`), and the operations' own paths that a compiler
//! session sets are put in front of them by sixteen effects kept only with
//! `setDeclarationDemand` (`operationPaths.count` and the others), where
//! each path tested them itself. The readable runtime grows 922 bytes
//! (`console-log-sum` at none 792,372 to 793,294 on master `15fc27ba`), past
//! 36 none and 36 prepare ceilings, which move to the next 1,000 above.
//! Advanced: no fixture grows; the 20 that keep several fallbacks shrink by
//! 71 to 1,062 bytes (`features` 120,034 to 118,972), and 12 advanced
//! ceilings move down to their sizes rounded up.
//!
//! The store tests by kind (light-cljs-1079): whether a value is one of
//! the runtime's own (`ownedKind`), the inventories that were store tests
//! (`seqableValue`, `collectionValue`, `metaValue`, `withMetaValue`,
//! `fnValue`) and `copyWithMeta`'s probes (`withMetaKeeps`,
//! `withMetaCopies`) test the keyed kind against fixed bits (`kindIn`),
//! where a table of stores by kind (`kindTable`, `storeHolds`,
//! `storesPredicate`, `ownedStores`) and the `*Cells` lists asked the
//! class's store; six native tests read their class's mask bit
//! (`maskNative`); the stores lose their `classKinds`. The readable runtime
//! grows 1,332 bytes, its comments (`console-log-sum` at none 793,294 to
//! 794,626 on master `3f9c2446`), past 34 none and 35 prepare ceilings,
//! which move to the next 1,000 above. Advanced: no fixture grows; the 20
//! that keep a store test shrink by 16 to 2,513 bytes (`vector-printed`
//! 15,815 to 13,302 and `map-printed` 23,839 to 22,171, whose printing kept
//! every store through `ownedProtocolValue`; `extended-classes` 71,534 to
//! 70,328; `features` 118,972 to 118,319), and their advanced ceilings move
//! down to their sizes rounded up to the next 100. The features example's
//! shared chunk shrinks 96,195 to 94,735 (ceiling 96,200 to 95,000).
//!
//! Each upstream member by its own name (light-cljs-1077): the types'
//! `.fields` effects wait for the flags of the members they define
//! (`member_cnt` and the others, `member_unit`), where they waited for one
//! flag that naming any member kept, and each type's `meta` field is its
//! own effect (`.metaField`), which `reset-meta!` keeps too; the hash map's
//! `fromArray` and `fromArrays` and the array map's `HASHMAP_THRESHOLD` wait
//! for their own names, where they waited for `typeStatics` with `EMPTY`.
//! The readable runtime grows 2,854 bytes, the 62 flags, the 20 `.metaField`
//! headers and their comments (`console-log-sum` at none 794,626 to
//! 797,480 on master `4af741fb`), past all 36 none and prepare ceilings,
//! which move to the next 1,000 above. Advanced: no fixture moves, as none
//! names a member; the re-frame apps, which name a few, shrink
//! (benchmarks/re-frame-vs-shadow.md, light-cljs-1077).
//!
//! The gate review's fixes (light-cljs-1023): `str` at `Advanced` joins its
//! operands' texts with `+` (`rt$strOne` for an operand that may run code),
//! a vector literal of at most 32 items is `rt$vectorOfTail([...])`, and
//! `group-by` gathers in a transient map. The readable runtime grows 1,111
//! bytes, the two units and their comments (`console-log-sum` at none
//! 797,480 to 798,591 on master `ed53a283`), past all 36 none and prepare
//! ceilings, which move to the next 1,000 above. Advanced: nine fixtures
//! grow by 10 to 149 bytes (`str` sites written out, `strOne` and
//! `vectorOfTail` beside `str` and `vectorFromArray`)
//! (`protocol-params` 65,011 to 65,160, past its ceiling, which moves to
//! 65,200; `features` 117,758 to 117,839; `eight-records`, `callbacks`,
//! `constant-keys` and `extended-classes` 63 to 68; `keyword-lookups` 22,
//! `atom-str` 10); the others do not move.
//!
//! Small map literals as one array (light-cljs-1080): at `Advanced` a map
//! literal of at most 8 distinct constant keys is
//! `rt$arrayMapOfLiteral([k0, v0, k1, v1, ...])`, an array map of the one
//! array, where it was a pooled key shape (`rt$mapShape`) and
//! `rt$mapFromShape(k<n>, [values])`. The readable runtime grows 354 bytes,
//! the unit and its comment (`console-log-sum` at none 798,591 to 798,945 on
//! master `cb4c3d94`), past eleven none and prepare ceiling pairs, which
//! move to the next 1,000 above (`debug-logging`, `constant-lookups`,
//! `set-only`, `seq-pipeline`, `pure-defs`, `closed-params`, `mapv-count`,
//! `extended-classes`, `loadable-site`, `rest-fn`, `local-arities`).
//! Advanced: three fixtures shrink, as their shapes and their keys' hashes
//! leave the pool (`event-maps` 50,978 to 50,790, `keyword-lookups` 52,788
//! to 52,631, `constant-keys` 73,198 to 73,042), and their ceilings move
//! down to the next 100; the others do not move.
//!
//! Multimethod calls (light-cljs-1083): the lean tables cache every plain
//! dispatch value's answer, past 16 entries by its code (`leanCode`,
//! serialized once at the root so nested vectors stay linear), and
//! find it by identity first (`getMethod`, `leanMethod`, `leanPlain`, which
//! keeps `symbolOf`), `MultiFn`'s `call` takes a step per count of 0 to 3
//! arguments (`multiCall0` to `multiCall3`, `multiApply`, `multiTarget`),
//! and where the tables are in atoms a call's `-get-method` takes
//! two-argument steps (`getMethodProtocol`). The readable runtime grows
//! 5,846 bytes, the units and their comments (`console-log-sum` at none
//! 798,945 to 804,791 on master `8a35dbd2`), past all 36 none and prepare
//! ceilings, which move to the next 1,000 above. Advanced: `defmulti` grows
//! about 1,500 bytes, the steps above (19,782, past its ceiling, which
//! moves to 19,800); no other fixture keeps a multimethod, and none moves.
//!
//! Gap-closing runtime, terse, and `derefSite` optimizations (`t3/gap-benchmark`
//! merged with `master` `321b3b41c`): adds inline `hostExt` fast paths for
//! `extend-type`-active predicates and protocol dispatch, `Atom.deref` + slim
//! `deref` + `derefSite`, fast printing (`writeText`, `stringEscapes`,
//! `kindHooks.byKind`), keyed `builtinSlots` flag, fast small-vector
//! construction, and unparenthesizing unambiguous function expressions in
//! `terse.rs` (`app.main.js` lowers 9,100 -> 7,800 and `app.reports.js`
//! 5,600 -> 5,100; `console-log-sum` at none is 818,088). Ceilings move to
//! the next rounding step above the merged measurements.
//!
//! Array-map and hash-map index seqs and getter-free key/value seqs
//! (light-cljs-1081): an array map's `seq` builds `ArrayMapIndexCell` and a
//! hash map's `seq` builds `NodeSeqCell`/`ArrayNodeSeqCell` over its cached
//! traversal array, each with keyed `-first`, `-rest`, `-next`, `-seq`,
//! `-count`, and `-reduce` slots (and `-drop` for array-map seqs), lazily
//! building and sharing realized `MapEntry` instances in a prototype-free map
//! on `root` only when an entry is demanded, while `KeySeqCell` uses plain
//! methods behind slim slot wrappers and reads unextended array-map and
//! hash-map keys/values directly without building entries; `ownEntryItems`
//! checks unextended entries and `seqReduceFrom` handles mid-reduction
//! protocol-mutation fallback. The readable runtime grows 22,002 bytes
//! (`console-log-sum` at none 818,088 to 840,090 on master `ba3efd525`), past
//! all 36 none and prepare ceilings, which move to the next 1,000 above.
//! Advanced: moving `makeMap.seq` to `when: seqPath` shrinks the three
//! record-only fixtures by 939 to 2,124 bytes (`eight-records` 46,109 ->
//! ceiling 46,200, `callbacks` 58,876 -> 58,900, `protocol-params` 67,861 ->
//! 67,900); the nine seq-walking fixtures that keep `ArrayMapIndexCell`,
//! `NodeSeqCell`/`ArrayNodeSeqCell`, `KeySeqCell`, or `seqReduceFrom` grow
//! (`seq-pipeline` 39,794, `rest-fn` 16,801, `keyword-lookups` 55,319,
//! `constant-keys` 75,956, `event-maps` 53,400, `mapv-count` 54,900,
//! `extended-classes` 74,144, `features` 127,917, `features-direct` 127,527),
//! and the features chunks move (`app.main.js` 7,802 -> 7,900,
//! `app.editor+app.reports.js` 100,594 -> 100,600, `app.editor.js` 16,464 ->
//! 16,500), each moving to the next 100 above.

pub struct Fixture {
    pub name: &'static str,
    pub source: &'static str,
    /// Byte ceilings in `OptimizationLevel::ALL` order: none, prepare, advanced.
    pub ceilings: [usize; 3],
    /// `ApplicationStats::removed_definitions` in the same order.
    pub removed: [usize; 3],
    /// Program output followed by the final value, as Node's `console.log` shows
    /// them, except that a function value prints as `#function`.
    pub printed: &'static str,
}

/// The first five rows are the Targets table of the design document, mirroring
/// upstream's `build_api_tests.clj` size ratchets (`trivial/core*.cljs`).
pub const FIXTURES: &[Fixture] = &[
    Fixture {
        name: "console-log-sum",
        source: "(js/console.log (+ 1 2))",
        ceilings: [841_000, 841_000, 200],
        removed: [0, 0, 0],
        printed: "3\nundefined\n",
    },
    Fixture {
        name: "println-hi",
        source: r#"(enable-console-print!) (println "hi")"#,
        ceilings: [840_000, 840_000, 6_200],
        removed: [0, 0, 0],
        printed: "hi\nnull\n",
    },
    Fixture {
        name: "vector-printed",
        source: "(enable-console-print!) (println [1 2 3])",
        ceilings: [841_000, 841_000, 14_300],
        removed: [0, 0, 0],
        printed: "[1 2 3]\nnull\n",
    },
    Fixture {
        name: "map-printed",
        source: "(enable-console-print!) (println {})",
        ceilings: [841_000, 841_000, 23_000],
        removed: [0, 0, 0],
        printed: "{}\nnull\n",
    },
    Fixture {
        name: "seq-pipeline",
        source: "(js/console.log (->> (map inc (range 10)) (filter even?) (partition 2) \
                 (drop 1) (mapcat identity) into-array))",
        ceilings: [841_000, 841_000, 39_800],
        removed: [0, 0, 0],
        printed: "[ 6, 8 ]\nundefined\n",
    },
    Fixture {
        name: "add",
        source: "(+ 1 2)",
        ceilings: [840_000, 840_000, 100],
        removed: [0, 0, 0],
        printed: "3\n",
    },
    Fixture {
        name: "rest-fn",
        source: "(fn [& xs] xs)",
        ceilings: [841_000, 841_000, 16_900],
        removed: [0, 0, 0],
        printed: "#function\n",
    },
    // light-cljs-1018 raised the whole-runtime ceilings by 1,000 for the
    // per-key protocol marker value and identity-hash table, 151 bytes past
    // 682,000.
    Fixture {
        name: "keyword-case",
        source: "(defn kind [k] (case k :a 1 :b 2 3))
                 (+ (kind :a) (* 10 (kind :b)) (* 100 (kind :c)))",
        ceilings: [841_000, 841_000, 4_300],
        removed: [0, 0, 0],
        printed: "321\n",
    },
    Fixture {
        name: "record-protocol",
        source: "(defprotocol Area (area [shape]))
                 (defrecord Rect [w h] Area (area [_] (* w h)))
                 (area (->Rect 3 4))",
        ceilings: [841_000, 841_000, 7_100],
        removed: [0, 0, 0],
        printed: "12\n",
    },
    // Unused definitions with pure initializers: `Prepare` removes the literal,
    // `Advanced` also the core calls, `let` and record construction.
    Fixture {
        name: "pure-defs",
        source: r#"(defrecord Point [x y])
                 (def label (str "n" (+ 1 2)))
                 (def table (hash-map :a 1 :b (* 2 3)))
                 (def origin (->Point 0 0))
                 (def size (count [1 2 3]))
                 (def lookup (let [k :a] (get {:a 1} k)))
                 (def literal [1 2 3])
                 (:x (->Point 1 2))"#,
        ceilings: [842_000, 842_000, 7_000],
        removed: [0, 1, 6],
        printed: "1\n",
    },
    // Keyword literals at every site: interned at every level, and one table
    // constant each at `Advanced` (light-cljs-208).
    Fixture {
        name: "keyword-lookups",
        source: "(defn score [m] (+ (get m :a 0) (get m :b 0) (:c m 0) (get m :ns/d 0)))
                 (defn kind [k] (case k :x 1 :y 2 0))
                 (reduce + (map score [{:a 1 :b 2} {:c 3 :ns/d 4} {:a 5 :c (kind :y)}]))",
        ceilings: [842_000, 842_000, 55_400],
        removed: [0, 0, 0],
        printed: "17\n",
    },
    // Constant collection literals (light-cljs-218): the empty vector reads
    // the runtime's singleton at every level, and at `Advanced` the constant
    // parts of `row` and the `get-in` path are built once, from the pool.
    // `row`, called once, is inlined into its caller (light-cljs-214), where
    // the lookups of its constant values and their counts fold, so no row is
    // built (light-cljs-221).
    Fixture {
        name: "constant-literals",
        source: "(defn row [i] {:id i :tags #{:a :b} :pos [1 2] :empty []
                                :meta {:kind :row :dims [1 2 3]}})
                 (reduce + (map (fn [i] (let [r (row i)]
                                          (+ (count (:pos r)) (count (get-in r [:meta :dims])))))
                                (range 10)))",
        ceilings: [841_000, 841_000, 35_700],
        removed: [0, 0, 1],
        printed: "50\n",
    },
    // Lookups, counts and destructuring of constants (light-cljs-221): at
    // `Advanced` a `^:const` map's destructured keys, a constant vector's
    // items and the counts are their values, and the membership test is a
    // `switch` on the keyword's spelling, so no collection is left.
    Fixture {
        name: "constant-lookups",
        source: "(def ^:const CONFIG {:width 640 :height 480 :title \"demo\" :modes #{:fast :safe}})
                 (defn allowed? [mode] (if (#{:fast :safe} mode) 1 0))
                 (let [{:keys [width height title]} CONFIG
                       [x y] [3 4]]
                   (+ (* width height) (count title) x y (count (:modes CONFIG))
                      (allowed? (keyword (.toLowerCase \"FAST\")))))",
        ceilings: [842_000, 842_000, 4_800],
        removed: [0, 0, 1],
        printed: "307214\n",
    },
    // Three records implement one protocol and only one is constructed: at
    // `Advanced` rapid type analysis (light-cljs-211) keeps that type and the
    // one method called, which the call reads from the receiver, and no
    // dispatcher, protocol or record method.
    Fixture {
        name: "three-records",
        source: "(defprotocol Shape (area [s]) (perimeter [s]))
                 (defrecord Square [side] Shape (area [_] (* side side)) (perimeter [_] (* 4 side)))
                 (defrecord Circle [r] Shape (area [_] (* 3 r r)) (perimeter [_] (* 6 r)))
                 (defrecord Tri [a b c] Shape (area [_] (* a b)) (perimeter [_] (+ a b c)))
                 (area (->Square 3))",
        ceilings: [843_000, 843_000, 7_100],
        removed: [0, 0, 0],
        printed: "9\n",
    },
    // Eight records and eight deftypes (light-cljs-264), each constructed,
    // its method called through the protocol's dispatcher and its field read
    // by a function that gets all sixteen, so that every type survives. What
    // each type adds is tested in `size_ratchet`, which builds the same
    // source with fewer types.
    Fixture {
        name: "eight-records",
        source: "(defprotocol Shape (area [s]))\n\
                 (defrecord R1 [a b] Shape (area [_] (+ a b)))\n\
                 (defrecord R2 [a b] Shape (area [_] (- a b)))\n\
                 (defrecord R3 [a b] Shape (area [_] (* a b)))\n\
                 (defrecord R4 [a b] Shape (area [_] (+ b a)))\n\
                 (defrecord R5 [a b] Shape (area [_] (- b a)))\n\
                 (defrecord R6 [a b] Shape (area [_] (* b a)))\n\
                 (defrecord R7 [a b] Shape (area [_] (+ a a)))\n\
                 (defrecord R8 [a b] Shape (area [_] (* b b)))\n\
                 (deftype T1 [a b] Shape (area [_] (* b b)))\n\
                 (deftype T2 [a b] Shape (area [_] (+ a a)))\n\
                 (deftype T3 [a b] Shape (area [_] (* b a)))\n\
                 (deftype T4 [a b] Shape (area [_] (- b a)))\n\
                 (deftype T5 [a b] Shape (area [_] (+ b a)))\n\
                 (deftype T6 [a b] Shape (area [_] (* a b)))\n\
                 (deftype T7 [a b] Shape (area [_] (- a b)))\n\
                 (deftype T8 [a b] Shape (area [_] (+ a b)))\n\
                 (reduce (fn [t s] (+ t (area s) (.-a s))) 0 \
                 [(->R1 1 2) (->R2 2 3) (->R3 3 4) (->R4 4 5) (->R5 5 6) (->R6 6 7) (->R7 7 8) \
                 (->R8 8 9) (T1. 9 10) (T2. 10 11) (T3. 11 12) (T4. 12 13) (T5. 13 14) \
                 (T6. 14 15) (T7. 15 16) (T8. 16 17)])",
        ceilings: [851_000, 851_000, 46_200],
        removed: [0, 0, 0],
        printed: "819\n",
    },
    // Constant collections as keys of a hash map (light-cljs-219): the
    // runtime caches each key's hash, and at `Advanced` the pooled keys carry
    // their hashes from the compiler. light-cljs-1018 raised the advanced
    // ceiling by 1,000: the identity-hash table one per runtime key on the
    // page (`identityHashes`) took it 71 bytes past 87,000.
    Fixture {
        name: "constant-keys",
        source: "(def table (zipmap (concat (map (fn [i] {:id i :kind :row}) (range 12))
                                        (map (fn [i] [:pos i (inc i)]) (range 12))
                                        (map (fn [i] #{i :tag}) (range 12)))
                                (range 36)))
                 (+ (get table {:id 3 :kind :row}) (get table [:pos 5 6]) (get table #{7 :tag})
                    (get table {:id 11 :kind :row} 0))",
        ceilings: [842_000, 842_000, 76_000],
        removed: [0, 0, 0],
        printed: "62\n",
    },
    // Event maps, a map literal with constant keys and dynamic values
    // (light-cljs-220): at `Advanced` its keys and their hashes are a pooled
    // shape, and each evaluation passes only the values. `event`, called
    // once, is inlined into its caller (light-cljs-214). light-cljs-1018
    // raised the whole-runtime ceilings by 1,000 for the per-key protocol
    // marker value and identity-hash table, 153 bytes past 682,000.
    Fixture {
        name: "event-maps",
        source: "(defn event [i] {:type :click :x i :y (- 100 i) :button 0 :target \"submit\"})
                 (reduce + (map (fn [i] (let [e (event i)] (+ (:x e) (:y e) (count e)))) (range 10)))",
        ceilings: [841_000, 841_000, 53_400],
        removed: [0, 0, 1],
        printed: "1050\n",
    },
    // A program that only builds a vector or a keyword keeps that type, what
    // its host-facing methods (`equiv`, `indexOf`, iteration, `call`) need
    // for the kinds it builds, and no map, set or protocol machinery
    // (light-cljs-227).
    Fixture {
        name: "vector-only",
        source: r#"(js/console.log (.join (js/Array.from [1 2 3]) " "))"#,
        ceilings: [841_000, 841_000, 7_000],
        removed: [0, 0, 0],
        printed: "1 2 3\nundefined\n",
    },
    Fixture {
        name: "keyword-only",
        source: "(js/console.log (.toString :a))",
        ceilings: [841_000, 841_000, 4_300],
        removed: [0, 0, 0],
        printed: ":a\nundefined\n",
    },
    // Since light-cljs-230 a vector or map answers its host-facing methods
    // from its own items, so neither keeps a seq; a map literal keeps no hash
    // map, `str` of scalars no printer and `atom` without options no option
    // parsing.
    Fixture {
        name: "map-only",
        source: "(js/console.log (.get {:a 1} :a))",
        ceilings: [841_000, 841_000, 16_000],
        removed: [0, 0, 0],
        printed: "1\nundefined\n",
    },
    Fixture {
        name: "atom-str",
        source: "(js/console.log (str @(atom 1) 2))",
        ceilings: [841_000, 841_000, 5_200],
        removed: [0, 0, 0],
        printed: "12\nundefined\n",
    },
    // Since light-cljs-231 a set answers its host-facing methods from its
    // members and a set literal of at most 8 constants builds an array set,
    // so a set keeps no seq, hash map or hashing. Since light-cljs-302 its
    // `lookup` tests for a hash trie in the map and set branches (95 bytes)
    // even where no hash trie is built, as the tries' hooks need.
    Fixture {
        name: "set-only",
        source: r#"(js/console.log (.has #{1 2 3} 2) (.join (js/Array.from #{1 2 3}) " "))"#,
        ceilings: [841_000, 841_000, 9_100],
        removed: [0, 0, 0],
        printed: "true 1 3 2\nundefined\n",
    },
    // Since light-cljs-238 the generic seq operations call protocol
    // implementations only where the program has some, and keep a branch for
    // vectors only where it builds them.
    Fixture {
        name: "mapv-count",
        source: "(js/console.log (count (mapv inc [1 2 3])))",
        ceilings: [841_000, 841_000, 54_900],
        removed: [0, 0, 0],
        printed: "3\nundefined\n",
    },
    // Retained raw JavaScript is an escape below `Advanced` only. There,
    // `add` is also inlined and folded (light-cljs-214).
    Fixture {
        name: "raw-js",
        source: r#"(def unused "unused")
                 (defn add ([x] x) ([x y] (+ x y)))
                 (js* "void 0")
                 (add 1 2)"#,
        ceilings: [841_000, 841_000, 100],
        removed: [0, 0, 2],
        printed: "3\n",
    },
    // Functions with several methods that are only called: at `Advanced` each
    // call names the method it runs and no dispatcher is defined
    // (light-cljs-212). Since light-cljs-214 `clamp` and `area` are inlined
    // at their calls and folded, and only `fib`, a `recur` target, is left.
    // light-cljs-1018 raised the whole-runtime ceilings by 1,000: the protocol
    // marker value became one per runtime key (`protocolSentinel`), 111 bytes
    // past 684,000 at none and prepare; advanced did not move.
    Fixture {
        name: "multi-arity",
        source: "(defn clamp ([x] (clamp x 0 10)) ([x lo hi] (max lo (min hi x))))
                 (defn area ([w] (area w w)) ([w h] (* w h)))
                 (defn fib ([n] (fib n 0 1)) ([n a b] (if (zero? n) a (recur (dec n) b (+ a b)))))
                 (+ (clamp 15) (area 3) (area 2 5) (fib 10))",
        ceilings: [843_000, 843_000, 400],
        removed: [0, 0, 2],
        printed: "84\n",
    },
    // Closed functions whose parameters take the types of their calls'
    // arguments (light-cljs-229): at `Advanced` record field reads, `nth` on a
    // vector, `=` on strings and keywords and `count` of a string lower inside
    // them. Since light-cljs-214 `at`, `width` and `weight` are inlined into
    // `score`, where the arguments' own types apply. light-cljs-1018 raised
    // the whole-runtime ceilings by 1,000 for the per-key protocol marker
    // value and identity-hash table, 15 bytes past 684,000.
    Fixture {
        name: "closed-params",
        source: "(defrecord Pt [x y])
                 (defn dist2 [p q] (let [dx (- (:x p) (:x q)) dy (- (:y p) (:y q))] (+ (* dx dx) (* dy dy))))
                 (defn at [v i] (nth v i))
                 (defn width [s] (if (= s \"\") 0 (count s)))
                 (defn weight [k] (if (= k :heavy) 10 1))
                 (defn score [acc v i s k]
                   (+ acc (dist2 (->Pt i 0) (->Pt 0 2)) (at v (rem i 3)) (width s) (weight k)))
                 (loop [i 0 acc 0]
                   (if (< i 10)
                     (recur (inc i) (score acc [1 2 3] i (if (even? i) \"\" \"word\") (if (odd? i) :heavy :light)))
                     acc))",
        ceilings: [843_000, 843_000, 18_600],
        removed: [0, 0, 3],
        printed: "419\n",
    },
    // A debug-only logging path behind a `goog-define` (light-cljs-214): at
    // `Advanced` the define's value folds every `when DEBUG` away, the logging
    // function becomes empty and is inlined at its calls, and nothing of the
    // path (its strings, `pr-str` and the printer) is left, so the output is
    // the multiplication alone.
    Fixture {
        name: "debug-logging",
        source: r#"(goog-define DEBUG false)
                 (defn debug-log [& parts] (when DEBUG (js/console.log "[debug]" (pr-str (vec parts)))))
                 (defn area [w h] (debug-log "area" w h {:w w :h h}) (* w h))
                 (when DEBUG (debug-log "startup" {:debug true}))
                 (area (.-length "abc") 4)"#,
        ceilings: [842_000, 842_000, 100],
        removed: [0, 0, 3],
        printed: "12\n",
    },
    // Local and named functions with several methods that are only called: at
    // `Advanced` their methods are entries with no dispatcher, those that
    // capture nothing in the constant table (light-cljs-228).
    Fixture {
        name: "local-arities",
        source: "(defn score [n lo hi]
                   (let [clamp (fn clamp ([x] (clamp x lo hi)) ([x l h] (if (< x l) l (if (> x h) h x))))
                         area (fn area ([w] (area w w)) ([w h] (* w h)))]
                     (letfn [(walk ([i] (walk i 0))
                                   ([i acc] (if (< i n) (recur (inc i) (+ acc (clamp i) (area i 2))) acc)))]
                       (+ (walk -3) (area 3)))))
                 (score 12 0 10)",
        ceilings: [844_000, 844_000, 500],
        removed: [0, 0, 0],
        printed: "194\n",
    },
    // Functions passed to `filter`, `reduce`, `map` and `swap!`, whose
    // parameters take what those operations pass them, as their callback
    // shapes in src/core_operations.tsv say (light-cljs-243): at `Advanced`
    // `heavy?`, `add-cost` (with `cost` inlined) and the `map` callback read
    // the record's fields, so no `:qty`, `:price` or `:label` keyword is
    // built, and the `swap!` callback compares its keyword argument directly.
    Fixture {
        name: "callbacks",
        source: "(defrecord Item [label qty price])
                 (defn cost [item] (* (:qty item) (:price item)))
                 (defn heavy? [item] (> (:qty item) 2))
                 (defn add-cost [total item] (+ total (cost item)))
                 (def counter (atom 0))
                 (let [items [(->Item \"a\" 1 2) (->Item \"bb\" 3 4) (->Item \"ccc\" 5 1)]]
                   (swap! counter (fn [n k] (if (= k :all) (+ n (count items)) n)) :all)
                   (+ (reduce add-cost 0 (filter heavy? items))
                      (reduce + (map (fn [item] (count (:label item))) items))
                      @counter))",
        ceilings: [842_000, 842_000, 58_900],
        removed: [0, 0, 1],
        printed: "26\n",
    },
    // Protocol methods whose parameters take what the protocol's calls pass
    // them (light-cljs-253): at `Advanced` both types' methods read the
    // record's fields, so no `:qty`, `:price` or `:label` keyword is built,
    // and `tag` tests its boolean flag directly.
    Fixture {
        name: "protocol-params",
        source: "(defrecord Item [label qty price])
                 (defprotocol Pricing (cost [p item]) (tag [p item flag]))
                 (deftype Plain [] Pricing
                   (cost [_ item] (* (:qty item) (:price item)))
                   (tag [_ item flag] (if flag (:label item) \"-\")))
                 (deftype Sale [pct] Pricing
                   (cost [_ item] (* (:qty item) (:price item) (- 1 pct)))
                   (tag [_ item flag] (if flag (str (:label item) \"%\") \"-\")))
                 (let [items [(->Item \"a\" 1 2) (->Item \"bb\" 3 4) (->Item \"ccc\" 5 1)]
                       plain (Plain.)
                       sale (Sale. 0.5)]
                   (str (reduce + (map (fn [item] (+ (cost plain item) (cost sale item))) items))
                        (apply str (map (fn [item]
                                          (str (tag plain item true)
                                               (tag sale item (> (:qty item) 2))))
                                        items))))",
        ceilings: [844_000, 844_000, 67_900],
        removed: [0, 0, 0],
        printed: "28.5a-bbbb%cccccc%\n",
    },
    // light-cljs-239: a multimethod with keyword dispatch values and no
    // hierarchy keeps its tables as arrays, walks no hierarchy and calls no
    // `IMultiFn` dispatcher; its map argument keeps what a map literal keeps.
    // light-cljs-1018 raised the whole-runtime ceilings by 1,000 for the
    // per-key protocol marker value, 68 bytes past 682,000; advanced grew 19
    // bytes, the marker value written as `Symbol.for(...)`.
    Fixture {
        name: "defmulti",
        source: "(defmulti m :k) (defmethod m :a [_] 1) (js/console.log (m {:k :a}))",
        ceilings: [841_000, 841_000, 19_900],
        removed: [0, 0, 0],
        printed: "1\nundefined\n",
    },
    // light-cljs-1052: cljs.spec's `specize*` pattern, a protocol extended to
    // a runtime class the program builds (the hash set) and to one it never
    // builds (the sorted set), whose slots and tree walks the slice leaves
    // out, keeping its prototype for `extendType`.
    Fixture {
        name: "extended-classes",
        source: "(defprotocol Specize (specize* [x]))
                 (extend-protocol Specize
                   cljs.core/PersistentHashSet (specize* [s] [:set (count s)])
                   cljs.core/PersistentTreeSet (specize* [s] [:sorted (count s)])
                   default (specize* [x] :other))
                 (js/console.log (pr-str (map specize* [#{1 2} [3] (seq #{4}) (= #{5} #{5})])))",
        ceilings: [842_000, 842_000, 74_200],
        removed: [0, 0, 0],
        printed: "([:set 2] :other :other :other)\nundefined\n",
    },
];

/// A program of several namespaces, analyzed from `entry` over the in-memory
/// source root `files` (light-cljs-272). The startup benchmark measures only
/// [`FIXTURES`], whose programs are one source text.
pub struct NamespaceFixture {
    pub name: &'static str,
    pub entry: &'static str,
    /// Each namespace's source by its path under the source root.
    pub files: &'static [(&'static str, &'static str)],
    /// As [`Fixture::ceilings`].
    pub ceilings: [usize; 3],
    /// As [`Fixture::removed`].
    pub removed: [usize; 3],
    /// As [`Fixture::printed`], followed by what the program logs once its
    /// promise reactions have run.
    pub printed: &'static str,
}

/// The source of the loadable target of `loadable-site` and the function
/// `direct-site` calls directly.
const GREET: (&str, &str) = (
    "app/greet.cljs",
    "(ns app.greet) (defn greet [who] (.concat \"hello \" who))",
);

/// The namespaces of examples/features its entry `app.main` loads.
const FEATURES_SHARED: (&str, &str) = (
    "app/shared.cljs",
    include_str!("../../examples/features/app/shared.cljs"),
);
const FEATURES_EDITOR: (&str, &str) = (
    "app/editor.cljs",
    include_str!("../../examples/features/app/editor.cljs"),
);
const FEATURES_REPORTS: (&str, &str) = (
    "app/reports.cljs",
    include_str!("../../examples/features/app/reports.cljs"),
);

/// Byte ceilings of each file of the `features` fixture's chunked build at
/// `Advanced` (`PreparedApplication::emit_chunks`, the files `--out-dir`
/// writes; light-cljs-280), in plan order: startup, the chunk both features
/// share, then each feature's. They move as [`Fixture::ceilings`] do: the
/// sizes on master `030e618` (6,243, 62,034, 5,268 and 5,087 bytes) rounded
/// up to the next 100 bytes, or 1,000 from 10,000 up. Startup holds 795
/// bytes of runtime units the size report attributes to the features' roots
/// and the 612-byte chunk loader beside what the application without its
/// features holds (`size_ratchet::FEATURES_STARTUP_SLACK`).
pub const FEATURES_CHUNK_CEILINGS: &[(&str, usize)] = &[
    ("app.main.js", 7_900),
    ("app.editor+app.reports.js", 100_600),
    ("app.editor.js", 16_500),
    ("app.reports.js", 5_100),
];

/// The code-splitting example and the same example calling its features
/// directly (`features-direct`), and a loadable at its smallest beside the same
/// program reading its target directly: the difference between the last two
/// is the cost of one `light.lazy/loadable` site with the `Loadable` type and
/// `light.lazy/load` (docs/code-splitting.md, "Phase 1").
pub const NAMESPACE_FIXTURES: &[NamespaceFixture] = &[
    NamespaceFixture {
        name: "features",
        entry: "app.main",
        files: &[
            (
                "app/main.cljs",
                include_str!("../../examples/features/app/main.cljs"),
            ),
            FEATURES_SHARED,
            FEATURES_EDITOR,
            FEATURES_REPORTS,
        ],
        ceilings: [859_000, 859_000, 128_000],
        removed: [0, 0, 21],
        printed: "menu: Editor | Reports\n#function\n",
    },
    NamespaceFixture {
        name: "features-direct",
        entry: "app.main",
        files: &[
            (
                "app/main.cljs",
                "(ns app.main (:require [app.shared :as shared] [app.editor] [app.reports]))
                 (js/console.log \"menu:\" (shared/label :editor) \"|\" (shared/label :reports))
                 (def editor app.editor/start)
                 (def reports app.reports/start)
                 (defn start []
                   (-> (js/Promise.resolve editor)
                       (.then (fn [start-editor] (start-editor) (js/Promise.resolve reports)))
                       (.then (fn [start-reports] (start-reports) nil))))
                 start",
            ),
            FEATURES_SHARED,
            FEATURES_EDITOR,
            FEATURES_REPORTS,
        ],
        ceilings: [859_000, 859_000, 127_600],
        removed: [0, 0, 20],
        printed: "menu: Editor | Reports\n#function\n",
    },
    NamespaceFixture {
        name: "loadable-site",
        entry: "app.main",
        files: &[
            (
                "app/main.cljs",
                "(ns app.main (:require [light.lazy :as lazy]))
                 (def greeting (lazy/loadable app.greet/greet))
                 (.then (lazy/load greeting) (fn [greet] (js/console.log (greet \"lazy\"))))
                 nil",
            ),
            GREET,
        ],
        ceilings: [842_000, 842_000, 1_000],
        removed: [0, 1, 2],
        printed: "null\nhello lazy\n",
    },
    // light-cljs-1018 raised the whole-runtime ceilings by 1,000 for the
    // per-key protocol marker value, 45 bytes past 682,000.
    NamespaceFixture {
        name: "direct-site",
        entry: "app.main",
        files: &[
            (
                "app/main.cljs",
                "(ns app.main (:require [app.greet :as greet]))
                 (def greeting greet/greet)
                 (.then (js/Promise.resolve greeting) (fn [greet] (js/console.log (greet \"lazy\"))))
                 nil",
            ),
            GREET,
        ],
        ceilings: [841_000, 841_000, 300],
        removed: [0, 0, 0],
        printed: "null\nhello lazy\n",
    },
];
