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
//! keeping hash maps now keeps too. The readable runtime grows 1,647 bytes
//! (none 681,310 for `add`, where 679,663), under every none and prepare
//! ceiling. Advanced, against master `1df0a071`: 279 bytes where a program
//! keeps hash maps (`constant-keys` 75,879 to 76,158, `features` 116,385 to
//! 116,664, `features-direct` 115,995 to 116,274); every other fixture is
//! unchanged. The ceilings of `constant-keys` (77,000) and `features-direct`
//! (117,000) move to the next 1,000 above.

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
        ceilings: [682_000, 682_000, 200],
        removed: [0, 0, 0],
        printed: "3\nundefined\n",
    },
    Fixture {
        name: "println-hi",
        source: r#"(enable-console-print!) (println "hi")"#,
        ceilings: [682_000, 682_000, 5_600],
        removed: [0, 0, 0],
        printed: "hi\nnull\n",
    },
    Fixture {
        name: "vector-printed",
        source: "(enable-console-print!) (println [1 2 3])",
        ceilings: [682_000, 682_000, 16_000],
        removed: [0, 0, 0],
        printed: "[1 2 3]\nnull\n",
    },
    Fixture {
        name: "map-printed",
        source: "(enable-console-print!) (println {})",
        ceilings: [682_000, 682_000, 24_000],
        removed: [0, 0, 0],
        printed: "{}\nnull\n",
    },
    Fixture {
        name: "seq-pipeline",
        source: "(js/console.log (->> (map inc (range 10)) (filter even?) (partition 2) \
                 (drop 1) (mapcat identity) into-array))",
        ceilings: [682_000, 682_000, 42_000],
        removed: [0, 0, 0],
        printed: "[ 6, 8 ]\nundefined\n",
    },
    Fixture {
        name: "add",
        source: "(+ 1 2)",
        ceilings: [682_000, 682_000, 100],
        removed: [0, 0, 0],
        printed: "3\n",
    },
    Fixture {
        name: "rest-fn",
        source: "(fn [& xs] xs)",
        ceilings: [682_000, 682_000, 20_000],
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
        ceilings: [683_000, 683_000, 5_400],
        removed: [0, 0, 0],
        printed: "321\n",
    },
    Fixture {
        name: "record-protocol",
        source: "(defprotocol Area (area [shape]))
                 (defrecord Rect [w h] Area (area [_] (* w h)))
                 (area (->Rect 3 4))",
        ceilings: [683_000, 683_000, 5_900],
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
        ceilings: [683_000, 683_000, 5_800],
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
        ceilings: [683_000, 683_000, 56_000],
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
        ceilings: [683_000, 683_000, 38_000],
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
        ceilings: [683_000, 683_000, 5_800],
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
        ceilings: [685_000, 685_000, 5_900],
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
        ceilings: [693_000, 693_000, 50_000],
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
        ceilings: [683_000, 683_000, 77_000],
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
        ceilings: [683_000, 683_000, 54_000],
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
        ceilings: [682_000, 682_000, 7_800],
        removed: [0, 0, 0],
        printed: "1 2 3\nundefined\n",
    },
    Fixture {
        name: "keyword-only",
        source: "(js/console.log (.toString :a))",
        ceilings: [682_000, 682_000, 5_300],
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
        ceilings: [682_000, 682_000, 16_000],
        removed: [0, 0, 0],
        printed: "1\nundefined\n",
    },
    Fixture {
        name: "atom-str",
        source: "(js/console.log (str @(atom 1) 2))",
        ceilings: [682_000, 682_000, 4_700],
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
        ceilings: [682_000, 682_000, 9_000],
        removed: [0, 0, 0],
        printed: "true 1 3 2\nundefined\n",
    },
    // Since light-cljs-238 the generic seq operations call protocol
    // implementations only where the program has some, and keep a branch for
    // vectors only where it builds them.
    Fixture {
        name: "mapv-count",
        source: "(js/console.log (count (mapv inc [1 2 3])))",
        ceilings: [682_000, 682_000, 55_000],
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
        ceilings: [683_000, 683_000, 100],
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
        ceilings: [685_000, 685_000, 400],
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
        ceilings: [685_000, 685_000, 19_000],
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
        ceilings: [683_000, 683_000, 100],
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
        ceilings: [685_000, 685_000, 500],
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
        ceilings: [684_000, 684_000, 62_000],
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
        ceilings: [685_000, 685_000, 68_000],
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
        ceilings: [683_000, 683_000, 19_000],
        removed: [0, 0, 0],
        printed: "1\nundefined\n",
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
    ("app.main.js", 9_100),
    ("app.editor+app.reports.js", 93_000),
    ("app.editor.js", 12_000),
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
        ceilings: [701_000, 701_000, 117_000],
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
        ceilings: [700_000, 700_000, 117_000],
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
        ceilings: [683_000, 683_000, 1_000],
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
        ceilings: [683_000, 683_000, 300],
        removed: [0, 0, 0],
        printed: "null\nhello lazy\n",
    },
];
