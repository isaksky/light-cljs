//! Rapid type analysis for the runtime's builtin types at `Advanced`
//! (light-cljs-227; docs/advanced-optimization.md, "Phase 2 builtin types
//! extension points").
//!
//! A generic operation reaches protocol machinery only through
//! `protocolHooks`, which `installMethods` fills in, and each kind's branch
//! only through `kindHooks`, which the units building the kind fill in. A
//! program that builds a vector therefore keeps no map, set or protocol
//! machinery. These tests run programs at every level, require every level
//! to give what `None` gives, and check what the `Advanced` slice keeps:
//! - each kind's branch of the generic operations, with and without protocol
//!   implementations in the program (the hooks closed and open);
//! - the host-facing methods of builtin prototypes (`equiv`, `indexOf`,
//!   iteration, `call`) on the kinds a small program builds;
//! - seqs of host iterables, which the generic operations know natively;
//! - the collection corpora of the other suites, printed with `pr-str`.
//!
//! Since light-cljs-230 ("Phase 2 builtin types, second pass") core
//! dispatchers and protocol tests answer natively, vectors and maps answer
//! their host-facing methods from their own items, the printer reads options
//! only from its public entries, and `str`, `atom` and map literals keep no
//! printer, option parsing or hash map of their own. Since light-cljs-231
//! sets do the same, a small set literal builds an array set, and a
//! membership test of a small set literal compares the tested value with its
//! members in value position too.
use super::optimization::execute;
use light_cljs::runtime_manifest::RuntimeManifest;
use light_cljs::runtime_slice::UPSTREAM_MEMBERS;
use light_cljs::{
    ApplicationOptions, OptimizationLevel, PreparedApplication, Program, analyze, parse,
    prepare_application,
};

fn program(source: &str) -> Program {
    analyze(source, &parse(source).unwrap()).unwrap_or_else(|error| panic!("{source}\n{error}"))
}

fn advanced(source: &str) -> PreparedApplication {
    prepare_application(
        &program(source),
        ApplicationOptions::new(OptimizationLevel::Advanced),
    )
}

/// The program prints `expected` (its value, through `console.log`) at every
/// level.
fn every_level(source: &str, expected: &str) {
    let program = program(source);
    for level in OptimizationLevel::ALL {
        let js = prepare_application(&program, ApplicationOptions::new(level)).emit_js();
        assert_eq!(
            execute(&format!("console.log({js});")),
            format!("{expected}\n"),
            "at {level}: {source}"
        );
    }
}

#[test]
fn existing_sequences_dispatch_only_the_requested_protocol() {
    // Pinned core.cljs: first/rest dispatch ISeq directly. IndexedSeq and
    // NodeSeq implement INext independently of ISeq's rest and ISeqable's seq.
    let body = r#"(let [calls (atom [])
                       s (specify! (seq [10 20 30])
                           ISeqable (-seq [this] (swap! calls conj :seq) this))
                       a [(first s) (into [] (rest s)) (into [] (next s)) @calls]
                       s (specify! (seq [10 20 30])
                           ISeq (-first [_] 99) (-rest [_] (list 88)))
                       b [(first s) (into [] (rest s)) (into [] (next s))]
                       s (specify! (seq [10 20 30])
                           INext (-next [_] (list 77)))
                       m (into {} (map (fn [x] [x x]) (range 20)))
                       s0 (seq m)
                       s1 (specify! s0 ISeqable (-seq [this] (swap! calls conj :map-seq) this))]
                   [a b (into [] (next s))
                    [(count (rest s1)) (count (next s1)) @calls]])"#;
    let expected = "[[10 [20 30] [20 30] []] [99 [88] [20 30]] [77] [19 19 []]]";
    every_level(&format!("(pr-str {body})"), expected);
    // The same native sequence implementations are used inside source macros.
    every_level(
        &format!("(defmacro result [] (list 'quote {body})) (pr-str (result))"),
        expected,
    );
}

#[test]
fn an_extended_empty_list_answers_seq_where_walks_end() {
    // `seq` takes the empty list past its slot (light-cljs-1033), to the
    // operation, whose protocol lookup finds a program's `-seq`; an empty
    // list with metadata takes its slot, which `extend-type` replaced. Each
    // call runs the program's method once. The answers are printed with
    // `str`, as printing a collection walks it, and a walk's `next` takes
    // the `seq` of the empty list that ends it.
    let source = r#"(def calls (atom 0))
                    (extend-type EmptyList ISeqable (-seq [_] (swap! calls inc) (list :extended)))
                    (defn once [f] (let [before @calls s (f)] (str (first s) "/" (- @calls before))))
                    (str (once #(seq ())) " " (once #(seq (with-meta () {:m 1}))) " "
                         (once #(seq (rest (list 1)))) " " (once #(seq (rest (cons 1 nil)))))"#;
    every_level(source, ":extended/1 :extended/1 :extended/1 :extended/1");
}

#[test]
fn map_seq_types_resolve_and_dispatch_as_upstream() {
    // Upstream's seq of an array map is a PersistentArrayMapSeq, and of a
    // hash map a NodeSeq; libraries dispatch on both names (light-cljs-982).
    let definitions = r#"(defprotocol Tag (tag [x]))
                    (extend-type cljs.core/PersistentArrayMapSeq Tag (tag [_] :extended))
                    (defmulti kind type)
                    (defmethod kind cljs.core/PersistentArrayMapSeq [_] :array-map-seq)
                    (defmethod kind cljs.core/NodeSeq [_] :node-seq)
                    (defmethod kind :default [_] :other)"#;
    let body = r#"(let [small (seq {:a 1 :b 2})
                          large (seq (into {} (map (fn [x] [x x]) (range 10))))]
                      [(kind small) (kind (rest small)) (kind large) (kind (seq [1 2]))
                       (kind (seq #{1})) (instance? PersistentArrayMapSeq small)
                       (instance? IndexedSeq small) (= PersistentArrayMapSeq (type small))
                       (satisfies? Tag small) (tag small) (satisfies? Tag large)
                       (count small) (counted? small) (vec small) (vec (js/Array.from small))])"#;
    let expected = "[:array-map-seq :array-map-seq :node-seq :other :other true false true true :extended false 2 true [[:a 1] [:b 2]] [[:a 1] [:b 2]]]";
    every_level(&format!("{definitions} (pr-str {body})"), expected);
    // Macro code sees the same types.
    every_level(
        &format!(
            "{definitions} (defmacro result [] (list (quote quote) {body})) (pr-str (result))"
        ),
        expected,
    );
}

#[test]
fn internal_core_types_that_values_have_resolve_and_dispatch_as_upstream() {
    // Upstream names the seq of a hash map whose root is an array node
    // ArrayNodeSeq, a reverse seq RSeq, a sorted map's entries BlackNode and
    // RedNode, and its transients TransientVector, TransientArrayMap,
    // TransientHashMap and TransientHashSet; libraries extend protocols to
    // them (light-cljs-983). A sorted map's entry is a MapEntry in light-cljs,
    // so the node names resolve to that type.
    let definitions = r#"(defprotocol Tag (tag [x]))
                    (extend-protocol Tag
                      cljs.core/ArrayNodeSeq (tag [_] :array-node-seq)
                      cljs.core/NodeSeq (tag [_] :node-seq)
                      cljs.core/RSeq (tag [_] :rseq)
                      cljs.core/BlackNode (tag [_] :tree-entry)
                      cljs.core/RedNode (tag [_] :tree-entry)
                      cljs.core/TransientVector (tag [_] :transient-vector)
                      cljs.core/TransientArrayMap (tag [_] :transient-array-map)
                      cljs.core/TransientHashMap (tag [_] :transient-hash-map)
                      cljs.core/TransientHashSet (tag [_] :transient-hash-set))
                    (defmulti kind type)
                    (defmethod kind cljs.core/ArrayNodeSeq [_] :array-node-seq)
                    (defmethod kind cljs.core/RSeq [_] :rseq)
                    (defmethod kind :default [_] :other)"#;
    // Keys 0 to 19 fill 20 of a root's 32 slots, so upstream's root is an
    // array node; keys 0 to 9 leave it a bitmap node.
    let body = r#"(let [large (seq (into {} (map (fn [x] [x x]) (range 20))))
                          medium (seq (into {} (map (fn [x] [x x]) (range 10))))
                          reversed (rseq [1 2 3])]
                      [(tag large) (tag (rest large)) (tag medium) (kind large) (kind medium)
                       (instance? ArrayNodeSeq large) (= ArrayNodeSeq (type large))
                       (instance? NodeSeq large) (count large) (= (range 20) (sort (map key large)))
                       (tag reversed) (kind reversed) (tag (rseq (subvec [1 2 3] 1)))
                       (tag (rseq (seq (array 1 2)))) (instance? IndexedSeq reversed) (vec reversed)
                       (vec (js/Array.from reversed)) (count reversed) (tag (rest reversed))
                       (tag (first (sorted-map :a 1))) (key (first (sorted-map :a 1)))
                       (tag (transient [])) (tag (transient {}))
                       (tag (transient (zipmap (range 10) (range 10)))) (tag (transient #{}))
                       (satisfies? Tag (transient #{})) (satisfies? Tag (seq [1 2]))])"#;
    let expected = "[:array-node-seq :array-node-seq :node-seq :array-node-seq :other true true false 20 true \
                    :rseq :rseq :rseq :rseq false [3 2 1] [3 2 1] 3 :rseq :tree-entry :a \
                    :transient-vector :transient-array-map :transient-hash-map :transient-hash-set \
                    true false]";
    every_level(&format!("{definitions} (pr-str {body})"), expected);
    // Macro code sees the same types.
    every_level(
        &format!(
            "{definitions} (defmacro result [] (list (quote quote) {body})) (pr-str (result))"
        ),
        expected,
    );
}

#[test]
fn internal_core_types_that_no_value_has_accept_inert_extensions() {
    // Upstream code extends protocols to cljs.core types that no light-cljs
    // value has: lite mode's collections (light-cljs has no lite mode; this
    // is cljs.spec.alpha's `specize*` on SetLite), and the trie nodes,
    // iterators and helpers of upstream's own collections, which light-cljs
    // builds differently. Each name resolves to a type of its own, so the
    // extension compiles and never dispatches (light-cljs-983).
    let names = "ArrayList ArrayNode ArrayNodeIterator BitmapIndexedNode Box HashCollisionNode \
                 HashMapIter HashMapLite HashSetIter NeverEquiv ObjMap ObjMapIterator SetLite \
                 VectorLite VectorLiteIterator VectorNode";
    let qualified: Vec<String> = names
        .split_whitespace()
        .map(|name| format!("cljs.core/{name}"))
        .collect();
    let types = qualified.join(" ");
    let extensions: String = qualified
        .iter()
        .map(|name| format!("{name} (lite [_] :lite) "))
        .collect();
    let definitions = format!(
        r#"(defprotocol Specize (specize* [x] [x form]))
           (extend-protocol Specize
             cljs.core/SetLite
             (specize* ([s] :lite) ([s form] :lite))
             default
             (specize* ([s] :default) ([s form] :default)))
           (defprotocol Lite (lite [x]))
           (extend-protocol Lite {extensions})
           (defmulti kind type)
           (defmethod kind cljs.core/SetLite [_] :lite)
           (defmethod kind cljs.core/VectorLite [_] :lite)
           (defmethod kind :default [_] :other)"#
    );
    let body = format!(
        r#"(let [types [{types}]
                 values [#{{1}} (hash-set 1 2) (set (range 20)) [] {{}} (zipmap (range 20) (range 20))
                         (sorted-map :a 1) (iter {{:a 1}}) (iter #{{1}}) (transient #{{}})]]
             [(specize* #{{1}}) (specize* #{{1}} :form) (map kind values)
              (some (fn [v] (satisfies? Lite v)) values)
              (some (fn [t] (some (fn [v] (instance? t v)) values)) types)
              (count types) (count (distinct types)) (every? fn? types)
              (= cljs.core/SetLite cljs.core/PersistentHashSet)])"#
    );
    let expected = "[:default :default (:other :other :other :other :other :other :other :other :other :other) \
                    nil nil 16 16 true false]";
    every_level(&format!("{definitions} (pr-str {body})"), expected);
    every_level(
        &format!(
            "{definitions} (defmacro result [] (list (quote quote) {body})) (pr-str (result))"
        ),
        expected,
    );
}

#[test]
fn record_field_lookup_uses_keyword_identity_without_equivalence_dispatch() {
    // Upstream defrecord uses keyword case for lookup/contains and
    // keyword-identical? for assoc. An IEquiv override cannot name a field.
    let body = r#"(let [calls (atom 0)
                       key (specify! (keyword "y")
                             IEquiv (-equiv [_ _] (swap! calls inc) true))
                       unknown (specify! (keyword "missing")
                                 IEquiv (-equiv [_ _] (swap! calls inc) true))
                       impostor (reify IEquiv (-equiv [_ _] (swap! calls inc) true)
                                       IHash (-hash [_] 0))
                       record (->R 10 20)
                       associated (assoc record key 30)
                       extra (assoc record unknown 40)]
                   [(get record key :missing) (contains? record key)
                    (get record unknown :missing) (contains? record unknown)
                    (get record impostor :missing) (contains? record impostor)
                    [(.-x associated) (.-y associated)]
                    [(.-x extra) (.-y extra) (count extra)] @calls])"#;
    let expected = "[20 true :missing false :missing false [10 30] [10 20 3] 0]";
    every_level(&format!("(defrecord R [x y]) (pr-str {body})"), expected);
    every_level(
        &format!("(defrecord R [x y]) (defmacro result [] (list 'quote {body})) (pr-str (result))"),
        expected,
    );
}

#[test]
fn list_construction_keeps_intrinsic_count_while_public_count_remains_live() {
    // Pinned List.-conj increments its stored count; list construction never
    // asks ICounted for the size of its newly allocated tail. Public count
    // still observes a method installed on List.prototype.
    // Declare the extension so Advanced retains its public dispatch hooks.
    let body = r#"(do (extend-type List ICounted (-count [xs] (.-count xs)))
                     (let [prototype (.-prototype List)
                       slot (js* "'cljs$core$ICounted$_count$arity$1$LKEY'")
                       previous (js/Object.getOwnPropertyDescriptor prototype slot)
                       calls (atom 0)
                       value (atom nil)
                       observed (atom nil)]
                   (try
                     (js/Object.defineProperty prototype slot
                       #js {:configurable true :writable true
                            :value (fn [_] (swap! calls inc) 999)})
                     (reset! value (list 10 20 30))
                     (reset! observed (count @value))
                     (finally
                       (if previous
                         (js/Object.defineProperty prototype slot previous)
                         (js-delete prototype slot))))
                   [@calls @observed (count @value) (into [] @value)]))"#;
    let expected = "[1 999 3 [10 20 30]]";
    every_level(&format!("(pr-str {body})"), expected);
    every_level(
        &format!("(defmacro result [] (list 'quote {body})) (pr-str (result))"),
        expected,
    );
}

#[test]
fn selected_protocol_missing_method_does_not_fall_back_after_a_getter_mutation() {
    // Pinned core.cljs get commits to -lookup after its membership test.
    // A method getter can remove the marker; that cannot turn a missing method
    // into a missing key or suppress a method the getter actually returns.
    let body = r#"(let [run (fn [found]
                             (let [value (specify! (js-obj)
                                           ILookup
                                           (-lookup [_ _] :two)
                                           (-lookup [_ _ _] :three))]
                               (js/Object.defineProperty
                                 value (js* "'cljs$core$ILookup$_lookup$arity$3$LKEY'")
                                 #js {:get (fn []
                                             (js-delete value (js* "'cljs$core$ILookup$$LKEY'"))
                                             (if found (fn [_ _ _] :found) nil))})
                               (try (get value :key :fallback)
                                    (catch :default error (.-message error)))))]
                         [(run false) (run true)])"#;
    let expected = r#"["No protocol method ILookup/-lookup defined for this value" :found]"#;
    every_level(&format!("(pr-str {body})"), expected);
    every_level(
        &format!("(defmacro result [] (list 'quote {body})) (pr-str (result))"),
        expected,
    );
}

/// As above, for values that declare `ILookup` and `IAssociative` by their
/// mask bits, whose slots `get` and `contains?` read themselves (`get.own`,
/// `contains.own`, light-cljs-358): a getter that clears the bit and the
/// marker cannot turn a missing method into a missing key, with
/// `extend-type`'s lookup (`specify!`) and without it (a `deftype`).
#[test]
fn masked_lookup_getters_preserve_selected_dispatch() {
    let run = |make: &str| {
        format!(
            r#"(let [run (fn [found]
                     (let [value {make}]
                       (doseq [[slot marker] [[(js* "'cljs$core$ILookup$_lookup$arity$3$LKEY'") (js* "'cljs$core$ILookup$$LKEY'")]
                                              [(js* "'cljs$core$IAssociative$_contains_key_QMARK_$arity$2$LKEY'") (js* "'cljs$core$IAssociative$$LKEY'")]]]
                         (js/Object.defineProperty value slot
                           #js {{:get (fn []
                                       (set! (.-cljs$lang$protocol_mask$partition0$ value) 0)
                                       (js-delete value marker)
                                       (case found :fn (fn [& _] :found) :false false nil))}}))
                       (set! (.-cljs$lang$protocol_mask$partition0$ value) (bit-or 256 512))
                       [(try (get value :key :fallback) (catch :default error (.-message error)))
                        (do (set! (.-cljs$lang$protocol_mask$partition0$ value) (bit-or 256 512))
                            (try (contains? value :key) (catch :default error (.-message error))))]))]
               (pr-str [(run nil) (run :false) (run :fn)]))"#
        )
    };
    let missing = r#"["No protocol method ILookup/-lookup defined for this value" "No protocol method IAssociative/-contains-key? defined for this value"]"#;
    let expected = format!("[{missing} {missing} [:found :found]]");
    every_level(
        &run(
            "(specify! (js-obj) ILookup (-lookup [_ _] :two) (-lookup [_ _ _] :three) IAssociative (-contains-key? [_ _] true) (-assoc [v _ _] v))",
        ),
        &expected,
    );
    every_level(
        &format!(
            "(deftype T [] ILookup (-lookup [_ _] :two) (-lookup [_ _ _] :three) IAssociative (-contains-key? [_ _] true) (-assoc [v _ _] v)) {}",
            run("(T.)")
        ),
        &expected,
    );
    // A getter that answers undefined and clears the bit and the marker only
    // on its second read, after the slot-first call's read: the upstream-shaped
    // paths still finish the selected dispatch (light-cljs-1066's review).
    every_level(
        r#"(deftype U [] ILookup (-lookup [_ _] :two) (-lookup [_ _ _] :three) IAssociative (-contains-key? [_ _] true) (-assoc [v _ _] v))
           (let [value (U.)]
             (doseq [[slot marker] [[(js* "'cljs$core$ILookup$_lookup$arity$3$LKEY'") (js* "'cljs$core$ILookup$$LKEY'")]
                                    [(js* "'cljs$core$IAssociative$_contains_key_QMARK_$arity$2$LKEY'") (js* "'cljs$core$IAssociative$$LKEY'")]]]
               (let [reads (atom 0)]
                 (js/Object.defineProperty value slot
                   #js {:get (fn []
                               (when (= 2 (swap! reads inc))
                                 (set! (.-cljs$lang$protocol_mask$partition0$ value) 0)
                                 (js-delete value marker))
                               js/undefined)})))
             (set! (.-cljs$lang$protocol_mask$partition0$ value) (bit-or 256 512))
             (pr-str [(try (get value :key :fallback) (catch :default error (.-message error)))
                      (try (contains? value :key) (catch :default error (.-message error)))]))"#,
        missing,
    );
}

/// The runtime units an `Advanced` program embeds.
fn units(source: &str) -> Vec<&'static str> {
    advanced(source)
        .retention()
        .units
        .iter()
        .map(|unit| RuntimeManifest::embedded().unit(unit.unit).name)
        .collect()
}

/// The protocol machinery a program without protocol implementations drops.
const PROTOCOL_MACHINERY: [&str; 12] = [
    "installMethods",
    "builtin",
    "ownMethod",
    "ownProtocol",
    "extendedMethod",
    "satisfies",
    "implementsProtocol",
    "builtinCoreImplementation",
    "protocolHooks.builtin",
    "protocolHooks.ownMethod",
    "protocolHooks.ownSatisfies",
    "protocolHooks.ownImplements",
];

/// The machinery that only protocol implementations of the runtime's own or
/// host values need (`extend-type`, `specify!`), which a program whose
/// implementations all belong to its own types drops (light-cljs-246), with
/// the protocol tables its protocol tests read (light-cljs-251).
const EXTENSION_MACHINERY: [&str; 11] = [
    "extendedMethod",
    "externalMethod",
    "protocolHooks.extend",
    "protocolHooks.programMethod",
    "protocolHooks.external",
    "satisfies",
    "implementsProtocol",
    "builtinCoreImplementation",
    "coreProtocolNames",
    "protocolHooks.satisfies",
    "protocolHooks.implements",
];

fn assert_keeps_none(source: &str, absent: &[&str]) -> Vec<&'static str> {
    let units = units(source);
    for unit in absent {
        assert!(!units.contains(unit), "{source} keeps `{unit}`: {units:?}");
    }
    units
}

#[test]
fn a_vector_keeps_no_map_set_or_protocol_machinery() {
    let source = "(js/console.log [1 2 3])";
    let units = assert_keeps_none(
        source,
        &[
            "makeMap",
            "makeSet",
            "PersistentArrayMapValue",
            "PersistentHashMapValue",
            "PersistentHashSetValue",
            "treeMapConstructor",
            "hash",
            "makeMap.seq",
            "makeMap.equiv",
            "makeTransient.nth",
        ],
    );
    for unit in PROTOCOL_MACHINERY {
        assert!(!units.contains(&unit), "{unit}: {units:?}");
    }
    // Its host-facing methods read its items (light-cljs-230), so it keeps
    // no seq and none of the seq types a vector builds.
    for unit in [
        "seq",
        "first",
        "next",
        "count",
        "nth",
        "makeVector.seq",
        "IndexedSeqValue",
        "ChunkedSeqValue",
        "equivSeqs",
    ] {
        assert!(!units.contains(&unit), "{unit}: {units:?}");
    }
    for unit in ["makeVector.equiv", "itemIterator", "vectorIndexOf"] {
        assert!(units.contains(&unit), "{unit}: {units:?}");
    }
}

#[test]
fn a_keyword_keeps_no_collection() {
    let units = assert_keeps_none(
        "(js/console.log :a)",
        &[
            "makeVector",
            "makeMap",
            "makeSet",
            "view",
            "seq",
            "count",
            "hash",
            "emptyList",
        ],
    );
    for unit in PROTOCOL_MACHINERY {
        assert!(!units.contains(&unit), "{unit}: {units:?}");
    }
    assert!(units.contains(&"get") && units.contains(&"objectEquiv"));
}

#[test]
fn an_empty_map_keeps_only_its_own_map_type() {
    let units = assert_keeps_none(
        "(js/console.log {})",
        &[
            "PersistentHashMapValue",
            "treeMapConstructor",
            "PersistentHashMapValue.traversal",
            "PersistentHashMapValue.put",
            "hashNodeAssoc",
            "hash",
            "makeSet",
        ],
    );
    for unit in PROTOCOL_MACHINERY {
        assert!(!units.contains(&unit), "{unit}: {units:?}");
    }
    // It iterates its entries itself (light-cljs-230), and keeps no seq.
    for unit in ["seq", "makeMap.seq", "mapProjection", "es6Iterator"] {
        assert!(!units.contains(&unit), "{unit}: {units:?}");
    }
    assert!(units.contains(&"makeMap.lookup") && units.contains(&"mapIterator"));
}

#[test]
fn a_program_type_opens_the_protocol_hooks() {
    // `installMethods` keeps the hooks the generic operations it reaches read,
    // so they dispatch to program types: since light-cljs-246 a type's own
    // methods (`ownMethod`), since light-cljs-251 the protocols it declares
    // (`ownProtocol`), and the whole machinery only with `extend-type` or
    // `specify!`. `count` walks a seq only for the values that need it, which
    // a program type builds only where it implements `ISeqable`, so this one
    // keeps no seqs: 21,678 bytes before light-cljs-251's second attempt.
    let source = "(deftype T [] ICounted (-count [_] 7)) (count (T.))";
    let units = assert_keeps_none(
        source,
        &[
            "accumulatingCount",
            "count.seqs",
            "seq",
            "next",
            "ISeqable",
            "makeMap.seq",
            "protocolHooks.ownSatisfies",
            "protocolHooks.countProtocol",
            "countProtocol",
        ],
    );
    for unit in [
        "installMethods",
        "ownMethod",
        "ownProtocol",
        "protocolHooks.ownMethod",
        "protocolHooks.ownImplements",
    ] {
        assert!(units.contains(&unit), "{unit}: {units:?}");
    }
    for unit in EXTENSION_MACHINERY {
        assert!(!units.contains(&unit), "{unit}: {units:?}");
    }
    // 7,558 bytes before light-cljs-291 and 8,043 after (`kindedStore`);
    // 8,355 to 8,644 after light-cljs-990 (2026-11-14): the stores' kind
    // brand (`payloads`); about 8,770 after light-cljs-1009 (2026-11-15):
    // the vectors' keyed kind, their `count` slot and the slot-first wrapper
    // of `count`; 9,162 after light-cljs-1025 (2026-11-15): the runtime
    // key's registration (about 290 bytes) and the key in each keyed name
    // (4 bytes more than the placeholder); 9,413 after
    // light-cljs-1010 (2026-11-15): the view test (`viewOf`, `isView`,
    // `viewCells`) that `count` and `seqableValue` read; 10,000 after
    // light-cljs-1029 (2026-11-16): the tests of chunks, chunk buffers, lazy
    // seqs and chunked conses (`chunkOf`, `chunkBufferOf`, `lazyCells`,
    // `chunkedConsCells`) that they read where those were private stores;
    // 10,171 after light-cljs-1030 (2026-11-16): the cells' test (`cellOf`,
    // `seqCells`) where theirs was a private store; 10,322 after
    // light-cljs-1028: `ownMethod` and `ownDeclared` tell a runtime class's
    // value, whose mask bits declare what it implements natively, by its
    // keyed kind; 10,664 after light-cljs-1012 (2026-11-16): the maps' and
    // map transients' tests (`mapOf`, `maps`, `transientMapState`) where
    // theirs were private stores; 11,026 after light-cljs-1034 (2026-11-16):
    // `maps` and `mapOf` tell the array and hash maps' fields apart, and
    // `count` and `hash` read an array map's; 11,609 after light-cljs-1013
    // (2026-11-16): the tests of the sets, queues, reduced values, atoms and
    // the other values that became classes, where theirs were private
    // stores; 11,942 after light-cljs-1037 (2026-11-16): the record test
    // (`recordTypeOf`, `recordCells`) that `count`'s native test reads where
    // `maps.records` answered records as maps, and a type's keyed kind;
    // 12,016 after light-cljs-1046 (2026-11-17): the keyed kinds of the
    // stores' classes (`classKinds`, `kindTable`, `storeHolds`), by which an
    // inventory (`storesPredicate`) tests one store where it probed each;
    // 12,182 after light-cljs-1069 (2026-11-18): `installMethods` defines a
    // protocol entry's slot before the records only `Object/` and `IFn`
    // methods need, and `decorateType` takes a record's masks and kit.
    let size = advanced(source).emit_js().len();
    assert!(size < 12_200, "{size} bytes");
    every_level(source, "7");
    let extended = "(deftype T [] ICounted (-count [_] 7)) (extend-type T ISeqable (-seq [_] nil)) (count (T.))";
    let units = self::units(extended);
    for unit in [
        "extendedMethod",
        "protocolHooks.programMethod",
        "protocolHooks.external",
        "protocolHooks.implements",
        "protocolHooks.satisfies",
    ] {
        assert!(units.contains(&unit), "{unit}: {units:?}");
    }
    every_level(extended, "7");
}

/// light-cljs-246: a program type and a builtin vector or keyword keep only
/// what the program can use. The host-facing methods of the runtime's values
/// take the generic operations only where `extend-type` or `specify!` can
/// give those values protocol implementations, a keyword invoked on a record
/// reaches the record's own `-lookup`, and items of program types still
/// compare through their `IEquiv`. Everything runs at every level against
/// `None`, including the errors of a protocol declared without the method.
#[test]
fn program_types_keep_no_machinery_for_builtin_values() {
    let record_and_vector = "(defrecord Pt [x y])
         (loop [i 0 acc 0] (if (< i 10) (recur (inc i) (+ acc (:x (->Pt i 0)) (nth [1 2 3] (rem i 3)))) acc))";
    let record_and_keyword = "(defrecord Pt [x y]) (js/console.log (:x (->Pt 1 0)) (.toString :a))";
    for source in [record_and_vector, record_and_keyword] {
        let units = assert_keeps_none(
            source,
            &[
                "seq",
                "count",
                "nth",
                "satisfies",
                "implementsProtocol",
                "cons.equiv",
                "makeMap.equiv",
                "makeMap.lookup",
                "collectionEntryIndex",
                "builtinCoreImplementation",
                "coreProtocolNames",
                "count.host",
                "es6Iterator.host",
                "sequentialIndex.host",
            ],
        );
        for unit in EXTENSION_MACHINERY {
            assert!(!units.contains(&unit), "{source} keeps `{unit}`");
        }
        // 11,867 to 12,178 bytes for the record and vector after
        // light-cljs-990 (2026-11-14): the stores' kind brand (`payloads`);
        // 13,206 for the record and keyword after light-cljs-1011
        // (2026-11-15): keywords as classes and the keyword constant's hash;
        // about 13,600 for the record and vector after light-cljs-1026
        // (2026-11-16): the vector's store, metadata and hash as its own
        // keyed fields, each read by its keyed name; 14,032 for the record and
        // vector after light-cljs-1028 (2026-11-16): the vector's and the map
        // entry's prototypes declare their protocols (masks and the `IFind`
        // marker), so the ceiling moves to 15,000.
        assert!(advanced(source).emit_js().len() < 15_000, "{source}");
    }
    every_level(record_and_vector, "64");
    // The vector's host-facing methods with records about: items of a program
    // type compare through their `IEquiv`, also inside vectors.
    every_level(
        &format!(
            "{DRAIN}
             (defrecord Pt [x y])
             (deftype Box [v] IEquiv (-equiv [_ o] (and (instance? Box o) (= v (.-v o)))))
             (let [v [(->Pt 1 2) (Box. 3) 4]]
               (pr-str [(.indexOf v (->Pt 1 2)) (.indexOf v (Box. 3)) (.lastIndexOf v 4) (.indexOf v (->Pt 1 3))
                        (.equiv v [(->Pt 1 2) (Box. 3) 4]) (.equiv v [(->Pt 1 2) (Box. 9) 4]) (.equiv [1 2] '(1 2))
                        (count (drain (.call (aget v js/Symbol.iterator) v))) (= (->Pt 1 2) {{:x 1 :y 2}})
                        (.equiv {{:x 1 :y 2}} (->Pt 1 2)) (= [(->Pt 1 2)] [(->Pt 1 2)])]))"
        ),
        "[0 1 2 -1 true false true 4 false false true]",
    );
    // A keyword invoked on a record, by the program or the host, reads the
    // record's fields and its other keys.
    every_level(
        "(defrecord Pt [x y])
         (let [p (assoc (->Pt 1 2) :z 3) ks [:x :y :z :w]]
           (pr-str [(map :x [p (->Pt 5 6)]) (mapv #(% p) ks) (.call :y nil p) (.call :w nil p :none)
                    (get p :z) (:w p :none) (.call :x nil {:x 7})]))",
        "[(1 5) [1 2 3 nil] 2 :none 3 :none 7]",
    );
    // A protocol declared without the method fails as at every level, but
    // where the runtime implements it natively.
    every_level(
        "(deftype NoLookup [] ILookup)
         (deftype HalfLookup [] ILookup (-lookup [_ k] [:got k]))
         (deftype NoCount [] ICounted)
         (let [message (fn [f] (try (pr-str (f)) (catch :default e (.-message e))))]
           (pr-str [(message #(get (NoLookup.) :a)) (message #(get (HalfLookup.) :a))
                    (message #(get (HalfLookup.) :a :nf)) (message #(:a (HalfLookup.) :nf))
                    (message #(count (NoCount.))) (message #(meta (reify IMeta)))
                    (message #(meta (with-meta (reify) {:m 1})))]))",
        r#"["No protocol method ILookup/-lookup defined for this value" "[:got :a]" "No protocol method ILookup/-lookup defined for this value" "No protocol method ILookup/-lookup defined for this value" "No protocol method ICounted/-count defined for this value" "nil" "{:m 1}"]"#,
    );
}

/// light-cljs-246: with a program type but no `extend-type`, a value counts
/// as implementing a protocol only where it declares it, as `extendedMethod`
/// decides: a host object holding a slot-named function is no lookup, but a
/// function's `IMeta` slot is its metadata. A core dispatcher read as a value
/// may be extended by type through its table, so the program keeps the
/// lookup `extend-type` installs. A record passed to a map's borrowed method
/// is a receiver of another type: it may throw, but never answers from the
/// record's empty map payload.
#[test]
fn program_types_keep_the_dispatch_of_interop() {
    every_level(
        "(let [f (fn [] 1)]
           (set! (.-cljs$core$IMeta$_meta$arity$1 f) (fn [_] {:m 1}))
           (pr-str [(get (reify ILookup (-lookup [_ k] 1)) \"x\")
                    (get (let [o (js-obj)] (set! (.-cljs$core$ILookup$_lookup$arity$2 o) (fn [_ k] 7)) o) \"x\")
                    (meta f)]))",
        "[1 nil {:m 1}]",
    );
    let patched = "(let [r (reify ICounted (-count [_] 1))]
         (aset -count \"object\" (fn [x] 7))
         (array (-count r) (try (-count #js {}) (catch :default e (.-message e)))))";
    every_level(patched, "[ 1, 7 ]");
    assert!(units(patched).contains(&"protocolHooks.external"));
    every_level(
        "(aset -count \"object\" (fn [x] 7)) (try (-count #js {}) (catch :default e (.-message e)))",
        "7",
    );
    let borrowed = "(defrecord R [x])
         (pr-str (try (.-done (.next (.call (.-keys {:a 1}) (->R 1)))) (catch js/TypeError _ :unsupported)))";
    every_level("(defrecord R [x]) (.-done (.next (.keys {:a 1})))", "false");
    let printed = execute(&format!("console.log({});", advanced(borrowed).emit_js()));
    assert!(
        ["false\n", ":unsupported\n"].contains(&printed.as_str()),
        "{printed}"
    );
}

/// light-cljs-1056: with a program type but no `extend-type` or `specify!`,
/// the lookup (`ownMethod`) takes a runtime class's mask bits as native, so
/// `meta` of a vector, which its `conj` and `assoc` read, asks no slot. A
/// program that writes a core protocol's member by name onto a vector keeps
/// the lookup `extend-type` installs, which reads that method, as every
/// level answers it.
#[test]
fn runtime_meta_slot_overrides_without_markers() {
    let written = "(deftype T [] ICounted (-count [_] 7))
         (let [v [1]]
           (set! (.-cljs$core$IMeta$_meta$arity$1 v) (fn [_] :custom))
           (pr-str [(count (T.)) (meta v) (meta (conj v 2)) (meta (assoc v 0 5))]))";
    every_level(written, "[7 :custom :custom :custom]");
    assert!(units(written).contains(&"protocolHooks.programMethod"));
    let plain = "(deftype T [] ICounted (-count [_] 7))
         (let [v (with-meta [1] {:m 1})]
           (pr-str [(count (T.)) (meta v) (meta (conj v 2)) (meta (assoc v 0 5))]))";
    every_level(plain, "[7 {:m 1} {:m 1} {:m 1}]");
    let units = units(plain);
    assert!(units.contains(&"protocolHooks.ownMethod"), "{units:?}");
    assert!(!units.contains(&"protocolHooks.programMethod"), "{units:?}");
}

/// light-cljs-1070: in a program that extends types, `count` of a string or
/// an array, `meta` of the runtime's collections and seqs, and a predicate of
/// a value whose class declares the protocol by its mask bit or marker take
/// no protocol lookup: the readable output's `extendedMethod`, `kindIn`
/// (the kind test of the lookup's, the hooks' and the native tests' kind
/// step, `storeHolds`'s store test until light-cljs-1079; `runtimeKind`
/// until light-cljs-1075 read the kind in the lookup),
/// `coreProtocolImplementation` and `satisfies` count no call, nor `builtin`
/// for `deref` of an atom, as upstream's `count`,
/// `meta` and `satisfies?` read the mask bit, the marker or the class's
/// slot. `extend-type` and `specify!` keep their answers.
#[test]
fn extended_programs_answer_count_meta_and_predicates_without_the_lookup() {
    let source = r#"(deftype T [] ICounted (-count [_] 7))
        (extend-type T IMeta (-meta [_] :t))
        (let [m {:m 1} calls #(aget js/globalThis "__calls")
              colls [(with-meta [1 2] m) (with-meta {:a 1} m) (with-meta (list 1) m) (with-meta #{1} m)
                     (with-meta (conj cljs.core/PersistentQueue.EMPTY 1) m) (with-meta (zipmap (range 20) (range 20)) m)
                     (with-meta (subvec [1 2 3] 1) m) (first {:a 1}) () (with-meta (sorted-map :a 1) m)
                     (with-meta (sorted-set 1) m) (with-meta (into #{} (range 20)) m) [] {}]
              seqs [(seq [1 2]) (seq {:a 1}) (rest (list 1 2)) (cons 0 (list 1)) (range 3)]
              before (calls)
              results [(count "abc") (count (array 1 2)) (mapv meta colls) (mapv meta seqs)
                       [(vector? (colls 0))] (mapv map? (mapv colls [1 5 9 13])) (mapv set? (mapv colls [3 10 11]))
                       (mapv coll? colls) (mapv seq? seqs) (mapv sequential? seqs) (chunked-seq? (range 3))
                       [@(atom 1) @(atom nil)]]
              after (calls)]
          (pr-str [(count (T.)) (meta (T.)) (- after before) results]))"#;
    let expected = "[7 :t 0 [3 2 [{:m 1} {:m 1} {:m 1} {:m 1} {:m 1} {:m 1} {:m 1} nil nil {:m 1} {:m 1} {:m 1} nil nil] \
         [nil nil nil nil nil] [true] [true true true true] [true true true] \
         [true true true true true true true true true true true true true true] [true true true true true] \
         [true true true true true] true [1 nil]]]";
    let mut options = ApplicationOptions::new(OptimizationLevel::Advanced);
    options.pseudo_names = true;
    let mut js = prepare_application(&program(source), options).emit_js();
    for (head, count) in [
        (
            "function extendedMethod(value, key, arity, fast = false, directOnly = false) {",
            " globalThis.__calls++;",
        ),
        (
            "const kindIn = (kind, low, high, program) =>",
            " globalThis.__calls++ >= 0 &&",
        ),
        (
            "function coreProtocolImplementation(protocol, value) {",
            " globalThis.__calls++;",
        ),
        (
            "function satisfies(protocol, value) {",
            " globalThis.__calls++;",
        ),
    ] {
        assert!(js.contains(head), "{head}");
        js = js.replace(head, &format!("{head}{count}"));
    }
    assert_eq!(
        execute(&format!("globalThis.__calls = 0; console.log({js});")),
        format!("{expected}\n")
    );
    // Unpatched, every level counts nothing.
    every_level(
        &format!("(set! (.-__calls js/globalThis) 0) {source}"),
        expected,
    );
    let deref_source = r#"(deftype T [] ICounted (-count [_] 7))
        (extend-type T IMeta (-meta [_] :t))
        (let [a (atom 1) b (atom nil) calls #(aget js/globalThis "__calls") before (calls) results [@a @b @a] after (calls)]
          (pr-str [(count (T.)) (- after before) results]))"#;
    let mut options = ApplicationOptions::new(OptimizationLevel::Advanced);
    options.pseudo_names = true;
    let head = "function builtin(value, key, arity) {";
    let js = prepare_application(&program(deref_source), options).emit_js();
    assert!(js.contains(head), "{head}");
    let js = js.replace(head, &format!("{head} globalThis.__calls++;"));
    assert_eq!(
        execute(&format!("globalThis.__calls = 0; console.log({js});")),
        "[7 0 [1 nil 1]]\n"
    );
}

/// light-cljs-1070: the answers the lookup gave `count`, `meta` and the
/// predicates in a program that extends types, at every level: a program's
/// `IMeta` of a runtime class (`extend-type`) or of one value (`specify!`),
/// `ICounted` of an array by `specify!` (and of `string` by `extend-type`,
/// which `count` of a string does not ask, as upstream's), a marker protocol
/// that makes a map a vector, and the misses of each predicate.
#[test]
fn extended_programs_keep_count_meta_and_predicate_answers() {
    every_level(
        "(extend-type PersistentVector IMeta (-meta [_] :vm))
         (extend-type PersistentArrayMap IVector)
         (extend-type string ICounted (-count [_] -1))
         (extend-type object IMeta (-meta [_] :om))
         (deftype T [])
         (let [a (specify! (array 1 2 3) ICounted (-count [_] 9))
               l (specify! (list 1) IMeta (-meta [_] :lm))
               h (zipmap (range 20) (range 20))]
           (pr-str [(meta [1]) (meta (with-meta [1] {:v 1})) (meta (with-meta {:a 1} {:x 1})) (meta l) (meta (list 2))
                    (meta (with-meta (conj cljs.core/PersistentQueue.EMPTY 1) {:q 1})) (meta (seq [1 2])) (meta (js-obj))
                    (meta (T.)) (meta inc) (meta nil) (meta :k) (meta (transient []))
                    (count \"abc\") (count a) (count (array 1)) (count nil) (count [1 2]) (count {:a 1}) (count (list 1 2))
                    (vector? {:a 1}) (vector? h) (vector? [1]) (vector? nil) (vector? \"s\") (vector? (T.))
                    (map? [1]) (map? nil) (map? (T.)) (map? h) (set? []) (set? nil) (set? (array 1))
                    (sequential? {:a 1}) (sequential? [1]) (sequential? (T.)) (sequential? :k) (sequential? (lazy-seq [1]))
                    (coll? \"s\") (coll? (T.)) (seq? {:a 1}) (seq? nil) (seq? (list 1)) (fn? [1]) (fn? \"s\") (fn? inc)
                    (empty? (conj cljs.core/PersistentQueue.EMPTY 1)) (empty? nil) (empty? (lazy-seq nil))
                    (chunked-seq? (seq [1 2])) (chunked-seq? (list 1)) (chunked-seq? (range 3))]))",
        "[:vm :vm {:x 1} :lm nil {:q 1} nil :om :om nil nil nil nil \
         3 9 1 0 2 1 2 \
         true false true false false false \
         false false false true false false false \
         false true false false true \
         false false false false true false false true \
         false true true \
         false false true]",
    );
}

/// light-cljs-1070: `extend-type` of `js/Object` writes `IMeta` on
/// `Object.prototype`, which the classes' own `-meta` slot (`metaSlot`) would
/// shadow: `meta` of every runtime value answers it, as the lookup found it
/// through the prototype chain, but a class's or a value's own method; and a
/// sorted map's seq, whose stack conjoins its lists, still builds them.
#[test]
fn inherited_object_meta_extensions_keep_runtime_answers() {
    every_level(
        "(extend-type js/Object IMeta (-meta [_] :custom))
         (pr-str [(meta [1]) (meta (with-meta [1] {:v 1})) (meta (with-meta {:a 1} {:x 1})) (meta (list 1))
                  (meta (conj cljs.core/PersistentQueue.EMPTY 1)) (meta (seq [1 2])) (meta #{1})
                  (meta (specify! [2] IMeta (-meta [_] :own))) (meta (range 3)) (meta (lazy-seq [1]))
                  (vec (keys (into (sorted-map) [[1 1] [2 2] [3 3]]))) (meta (rseq (into (sorted-map) [[1 1] [2 2]])))])",
        "[:custom :custom :custom :custom :custom :custom :custom :own :custom :custom [1 2 3] :custom]",
    );
}

/// light-cljs-1075: `empty?` calls a counted value's `-count` slot, but a
/// runtime class's method borrowed onto a program type's value takes the
/// dispatcher, which passes over it and finds none, as before (the Sol
/// review's scenario); a program type's own method, the queue's and an
/// array's from `specify!` answer as before.
#[test]
fn empty_preserves_borrowed_builtin_count_slots() {
    every_level(
        "(deftype T [] ICounted (-count [_] 5))
         (let [t (T.)]
           (set! (.-cljs$core$ICounted$_count$arity$1 t) (.-cljs$core$ICounted$_count$arity$1 ()))
           (pr-str [(try (empty? t) (catch :default _ :error)) (empty? (T.)) (empty? (conj cljs.core/PersistentQueue.EMPTY 1))
                    (empty? cljs.core/PersistentQueue.EMPTY) (empty? (specify! (array 1) ICounted (-count [_] 0)))]))",
        "[:error false false true true]",
    );
}

/// light-cljs-1070: the answers master's lookup gave, at every level, where
/// the `satisfies` hook skips the native test of nil, a primitive or a
/// program type's value and reads a mask-less protocol (`Fn`) of a class
/// with masks as false, `counted?` reads a seq view's kind first, and
/// `deref` of an atom calls its own `-deref` or reads its state: host
/// tables `extend-type` gave `nil`, `string`, `number`, `boolean` and
/// `default` (`Fn` among them), program types with and without masks and a
/// record, and an atom's `-deref` from `specify!`, `extend-type` of `Atom`
/// and of `js/Object`.
#[test]
fn extended_programs_keep_host_predicate_and_deref_answers() {
    every_level(
        "(extend-type nil ISequential)
         (extend-type string IMap)
         (extend-type number ICollection)
         (extend-type default ISet)
         (extend-type boolean IVector)
         (deftype T [])
         (deftype U [] ISeqable (-seq [_] nil))
         (defrecord R [x])
         (pr-str [(sequential? nil) (sequential? \"s\") (map? \"s\") (map? 1) (coll? 1) (coll? \"s\") (set? nil) (set? 1) (set? \"s\")
                  (set? (T.)) (set? (U.)) (set? (->R 1)) (set? [1]) (set? (js-obj)) (vector? true) (vector? false) (vector? nil)
                  (map? (->R 1)) (coll? (->R 1)) (map? (T.)) (coll? (U.)) (sequential? (T.)) (seq? (U.)) (seqable? (U.))
                  (seqable? (->R 1)) (seq? (T.)) (seq? \"s\") (fn? [1]) (fn? {:a 1}) (fn? :k) (fn? (T.)) (fn? nil) (fn? \"s\")
                  (counted? (seq [1 2])) (counted? (seq (array 1 2))) (counted? (rseq [1 2])) (counted? (seq #{1}))
                  (counted? (seq {:a 1})) (counted? (lazy-seq [1])) (counted? (list 1)) (counted? (cons 1 nil))
                  (bounded-count 2 (seq (array 1 2 3))) (bounded-count 2 (lazy-seq [1 2 3])) (apply + 1 2 [3 4])
                  (apply str (seq \"abc\"))])",
        "[true false true false true false false true true true false false false true true true false true true false \
         false false false true true false false false false false false false false true true false false true false \
         true true 3 2 10 \"abc\"]",
    );
    every_level(
        "(extend-type default Fn)
         (deftype T [])
         (pr-str [(fn? [1]) (fn? {:a 1}) (fn? :k) (fn? 'k) (fn? (T.)) (fn? nil) (fn? \"s\") (fn? 1) (fn? (js-obj)) (fn? (atom 1))
                  (fn? (list 1)) (fn? #{1}) (fn? (seq [1])) (fn? (with-meta inc {:m 1})) (fn? inc) (ifn? [1]) (ifn? nil)])",
        "[false false false false true true true true true false false false false true true true true]",
    );
    every_level(
        "(deftype D [] IDeref (-deref [_] :d))
         (extend-type D IMeta (-meta [_] :dm))
         (let [a (atom 1) s (specify! (atom 2) IDeref (-deref [_] :s)) v (volatile! 3)]
           (pr-str [@a @s @v @(D.) @(delay 4) (deref (reduced 5)) (swap! a inc) @a (reset! s 7) @s
                    (deref (atom nil)) (deref (atom false))]))",
        "[1 :s 3 :d 4 5 2 2 7 :s nil false]",
    );
    every_level(
        "(extend-type Atom IDeref (-deref [_] :ext))
         (pr-str [@(atom 1) @(volatile! 2) @(specify! (atom 3) IDeref (-deref [_] :own)) (swap! (atom 1) inc)])",
        "[:ext 2 :own 2]",
    );
    every_level(
        "(extend-type js/Object IDeref (-deref [_] :obj))
         (pr-str [@(atom 1) @(volatile! 2) @(js-obj) @(specify! (atom 3) IDeref (-deref [_] :own)) @(delay 4)])",
        "[:obj :obj :obj :own :obj]",
    );
    // A runtime class's slot written onto an atom's `-deref` is not its
    // method, as the lookup (`keyMethod`) passes over it.
    every_level(
        "(deftype T [] ICounted (-count [_] 7))
         (extend-type T IMeta (-meta [_] :t))
         (let [a (atom 1) v (atom [1 2])]
           (set! (.-cljs$core$IDeref$_deref$arity$1 a) (.-cljs$core$ICounted$_count$arity$1 [1 2 3]))
           (set! (.-cljs$core$IDeref$_deref$arity$1 v) (.-cljs$core$IMeta$_meta$arity$1 [1]))
           (pr-str [(count (T.)) @a @v (swap! a inc) @a]))",
        "[7 1 [1 2] 2 2]",
    );
}

/// light-cljs-1060: without `extend-type` or `specify!`, and with no core
/// method written by name, nothing but a vector's or a set's own field can
/// answer its metadata, so `conj` and `assoc` of a vector, a subvector and a
/// map entry, and `conj` of a set, copy the field, as upstream's `-conj` and
/// `-assoc` do, in a program that implements protocols: the readable
/// output's `meta` and its lookup (`ownMethod`) count no call. Where
/// `specify!` or a method written by name gives one value its own `IMeta`,
/// they still read the metadata with `meta`, so the result holds what that
/// method answers, as every level gave before.
#[test]
fn vector_and_set_updates_copy_metadata_without_the_lookup() {
    let source = r#"(deftype T [] ICounted (-count [_] 7))
        (let [m {:m 1} v (with-meta [1 2] m) sub (with-meta (subvec [1 2 3] 1) m) e (first {:a 1})
              s (with-meta #{1} m) none (with-meta [] nil) calls #(aget js/globalThis "__calls") before (calls)
              results [(conj v 3) (assoc v 0 5) (assoc v 2 3) (conj sub 4) (assoc sub 0 5) (assoc sub 2 6) (conj e 3)
                       (assoc e 1 2) (conj s 2) (conj [1] 2) (assoc [1] 0 2) (conj #{} 1) (conj none 1)]
              after (calls)]
          (pr-str [(count (T.)) (- after before) results (mapv meta results)]))"#;
    let expected = "[7 0 [[1 2 3] [5 2] [1 2 3] [2 3 4] [5 3] [2 3 6] [:a 1 3] [:a 2] #{1 2} [1 2] [2] #{1} [1]] \
         [{:m 1} {:m 1} {:m 1} {:m 1} {:m 1} {:m 1} nil nil {:m 1} nil nil nil nil]]";
    every_level(
        &format!("(set! (.-__calls js/globalThis) 0) {source}"),
        expected,
    );
    let kept = units(source);
    assert!(kept.contains(&"protocolHooks.ownMethod"), "{kept:?}");
    assert!(!kept.contains(&"protocolHooks.extend"), "{kept:?}");
    let mut options = ApplicationOptions::new(OptimizationLevel::Advanced);
    options.pseudo_names = true;
    let mut js = prepare_application(&program(source), options).emit_js();
    for head in [
        "function meta(value) {",
        "function ownMethod(value, key, arity) {",
    ] {
        assert!(js.contains(head), "{head}");
        js = js.replace(head, &format!("{head} globalThis.__calls++;"));
    }
    assert_eq!(
        execute(&format!("globalThis.__calls = 0; console.log({js});")),
        format!("{expected}\n")
    );
    // One value's own `IMeta`, by `specify!` or by a method written by name,
    // answers the metadata its updates hold.
    every_level(
        "(deftype T [] ICounted (-count [_] 7))
         (let [v (specify! [1 2] IMeta (-meta [_] :v)) sub (specify! (subvec [1 2 3] 1) IMeta (-meta [_] :sub))
               e (specify! (first {:a 1}) IMeta (-meta [_] :e)) s (specify! #{1} IMeta (-meta [_] :s))]
           (pr-str [(count (T.)) (mapv meta [(conj v 3) (assoc v 0 5) (conj sub 4) (assoc sub 0 5) (conj e 3) (assoc e 0 2)
                                             (conj s 2) (conj [1] 2)])]))",
        "[7 [:v :v :sub :sub :e :e :s nil]]",
    );
    let written = "(deftype T [] ICounted (-count [_] 7))
         (let [sub (subvec [1 2 3] 1) e (first {:a 1}) s #{1}]
           (doseq [x [sub e s]] (set! (.-cljs$core$IMeta$_meta$arity$1 x) (fn [_] :written)))
           (pr-str [(count (T.)) (mapv meta [(conj sub 4) (assoc sub 0 5) (conj e 3) (assoc e 0 2) (conj s 2) (conj [1] 2)])]))";
    every_level(
        written,
        "[7 [:written :written :written :written :written nil]]",
    );
    assert!(units(written).contains(&"protocolHooks.extend"));
}

/// light-cljs-246: a program type implementing `ISequential`, `IMap` or
/// `ISet` compares with the runtime's collections through the generic
/// operations: `ISequential` keeps the protocol's unit, so structural
/// equality of sequential values, and maps and sets count and search the
/// other value through `protocolHooks`. `specify!` and `extend-type` of the
/// runtime's values send their host-facing methods through the generic
/// operations.
#[test]
fn program_types_reach_the_runtime_kinds_they_implement() {
    let sequential = "(deftype Two [a b] ISequential ISeqable (-seq [_] (list a b)))
         (reify ISequential)
         (pr-str [(= [1 2] (Two. 1 2)) (= '(1 2) (Two. 1 2)) (= [1 3] (Two. 1 2)) (= [1 2] (Two. 2 1))
                  (= (range 2) (Two. 0 1)) (.equiv [1 2] (Two. 1 2))])";
    every_level(sequential, "[true true false false true true]");
    // Only the program's types are sequential values besides vectors here.
    for source in [
        "(deftype Empty [] ISequential ISeqable (-seq [_] nil))
         (array (= [] (Empty.)) (.equiv [] (Empty.)) (= [1] (Empty.)))",
        "(array (= [] (reify ISequential ISeqable (-seq [_] nil))) (= [] (reify ISeqable (-seq [_] nil))))",
    ] {
        assert!(units(source).contains(&"cons.equiv"), "{source}");
        assert!(!units(source).contains(&"ListValue"), "{source}");
    }
    every_level(
        "(deftype Empty [] ISequential ISeqable (-seq [_] nil))
         (array (= [] (Empty.)) (.equiv [] (Empty.)) (= [1] (Empty.)))",
        "[ true, true, false ]",
    );
    every_level(
        "(array (= [] (reify ISequential ISeqable (-seq [_] nil))) (= [] (reify ISeqable (-seq [_] nil))))",
        "[ true, false ]",
    );
    every_level(
        "(deftype M [] IMap ICounted (-count [_] 1)
           ILookup (-lookup [this k] (-lookup this k nil)) (-lookup [_ k nf] (if (= k :a) 1 nf)))
         (deftype S [] ISet ICounted (-count [_] 2) ILookup (-lookup [_ k] k) (-lookup [_ k nf] (if (#{1 2} k) k nf))
           IAssociative (-contains-key? [_ k] (contains? #{1 2} k)))
         (pr-str [(= {:a 1} (M.)) (= {:a 2} (M.)) (= {:a 1 :b 2} (M.)) (= #{1 2} (S.)) (= #{1 3} (S.))
                  (.equiv {:a 1} (M.)) (.equiv #{2 1} (S.))])",
        "[true false false true false true true]",
    );
    every_level(
        &format!(
            "{DRAIN}
             (def v (specify! [1 2 3] ISeqable (-seq [_] (list 9 8)) ICounted (-count [_] 5)))
             (defrecord Pt [x])
             (pr-str [(drain (.call (aget v js/Symbol.iterator) v)) (try (.lastIndexOf v 1) (catch :default e (.-message e))) (.-cnt v) (:x (->Pt 1))])"
        ),
        "[[9 8 nil] \"Index out of bounds\" 5 1]",
    );
}

/// The protocol tests of light-cljs-251's scenario over every kind of value:
/// program types, a record, `reify`, the runtime's values and host values,
/// among them one declaring `ICounted` by its mask bit and one holding the
/// marker's name as a property, each a row of its label, one digit per
/// predicate (`counted?`, `seq?`, `seqable?`, `sequential?`, `coll?`, `map?`,
/// `vector?`, `set?`, `record?`, `associative?`, `reversible?`, `indexed?`,
/// `reduceable?`, `ifn?`, `fn?`, `list?`, `iterable?`, `cloneable?`,
/// `map-entry?` and `satisfies?` of `ICounted`, `ISeqable` and `ISeq`), and
/// its `count`, the `first` of its `seq` and `empty?`, or their errors.
fn protocol_test_rows(extensions: &str, extra: &str) -> String {
    format!(
        "(deftype T [] ICounted (-count [_] 7))
         (deftype S [] ISeq (-first [_] 1) (-rest [_] (list 2)) ISeqable (-seq [this] this) ISequential)
         (defrecord R [x])
         {extensions}
         (defn attempt [f x] (try (f x) (catch :default e (if (fn? x) :threw (.-message e)))))
         (apply str (interpose \"\\n\"
           (for [[label v] [[:T (T.)] [:S (S.)] [:R (->R 1)] [:reify-counted (reify ICounted (-count [_] 3))]
                            [:reify (reify)] [:vector [1 2]] [:list '(1 2)] [:map {{:a 1}}] [:set #{{1}}]
                            [:range (range 3)] [:lazy (map inc [1])] [:string \"ab\"] [:nil nil] [:number 5]
                            [:array #js [1 2]] [:object #js {{:a 1}}] [:fn (fn [] 1)]
                            [:masked (let [o (js-obj)] (set! (.-cljs$lang$protocol_mask$partition0$ o) 2) o)]
                            [:marker (let [o (js-obj)] (set! (.-cljs$core$ICounted$ o) true) o)] {extra}]]
             (pr-str [label
                      (apply str (map #(if (% v) 1 0)
                                      [counted? seq? seqable? sequential? coll? map? vector? set? record?
                                       associative? reversible? indexed? reduceable? ifn? fn? list? iterable?
                                       cloneable? map-entry? #(satisfies? ICounted %) #(satisfies? ISeqable %)
                                       #(satisfies? ISeq %)]))
                      (attempt count v) (attempt #(some-> (seq %) first) v) (attempt empty? v)]))))"
    )
}

/// light-cljs-251: the generic operations test core protocols by name, so
/// where no `extend-type` or `specify!` exists a value satisfies the
/// protocols it declares (`ownProtocol`), and otherwise what the runtime's
/// values satisfy natively, and a program type keeps none of the protocol
/// tables (`satisfies`, `implementsProtocol`, `coreProtocolNames`,
/// `builtinCoreImplementation`). Every level answers as `None` does, with and
/// without extensions, which keep the tables.
#[test]
fn protocol_tests_of_program_types_need_no_protocol_tables() {
    let closed = protocol_test_rows("", "");
    // `satisfies?` of a core protocol calls `satisfies` itself, which keeps
    // the tables, but the generic operations still test the program's own.
    let units = assert_keeps_none(
        &closed,
        &[
            "extendedMethod",
            "externalMethod",
            "protocolHooks.programMethod",
            "protocolHooks.satisfies",
            "protocolHooks.implements",
        ],
    );
    for unit in [
        "ownProtocol",
        "protocolHooks.ownSatisfies",
        "protocolHooks.ownImplements",
    ] {
        assert!(units.contains(&unit), "{unit}: {units:?}");
    }
    every_level(
        &closed,
        r#"[:T "1000000000000000000100" 7 "[object Object] is not ISeqable" false]
[:S "0111000000000000000011" 2 1 false]
[:R "1010110011000000110110" 1 [:x 1] false]
[:reify-counted "1000000000000000000100" 3 "[object Object] is not ISeqable" false]
[:reify "0000000000000000000000" "count not supported on this type" "[object Object] is not ISeqable" "[object Object] is not ISeqable"]
[:vector "1011101001111100110110" 2 1 false]
[:list "1111100000001001110111" 2 1 false]
[:map "1010110001001100110110" 1 [:a 1] false]
[:set "1010100100000100110110" 1 1 false]
[:range "1111100000011000110111" 3 0 false]
[:lazy "0111100000001000100011" 1 2 false]
[:string "0010000000000000000000" 2 "a" false]
[:nil "1010000000000000000100" 0 nil true]
[:number "0000000000000000000000" "count not supported on this type" "5 is not ISeqable" "5 is not ISeqable"]
[:array "0010000000000000000000" 2 1 false]
[:object "0000000000000000000000" "count not supported on this type" "[object Object] is not ISeqable" "[object Object] is not ISeqable"]
[:fn "0000000000000110000000" :threw :threw :threw]
[:masked "1000000000000000000100" "No protocol method ICounted/-count defined for this value" "[object Object] is not ISeqable" "No protocol method ICounted/-count defined for this value"]
[:marker "0000000000000000000000" "count not supported on this type" "[object Object] is not ISeqable" "[object Object] is not ISeqable"]"#,
    );
    // `extend-type` of a program type and of a host type, and `specify!` of a
    // vector, keep the tables, which find what they add.
    let extended = protocol_test_rows(
        "(extend-type T ISeqable (-seq [_] (list 7 8))) (extend-type number ICounted (-count [n] n))",
        "[:specified (specify! (vec #js [1 2]) ICounted (-count [_] 42))]",
    );
    let units = self::units(&extended);
    for unit in [
        "satisfies",
        "implementsProtocol",
        "protocolHooks.satisfies",
        "protocolHooks.implements",
    ] {
        assert!(units.contains(&unit), "{unit}: {units:?}");
    }
    every_level(
        &extended,
        r#"[:T "1010000000000000000110" 7 7 false]
[:S "0111000000000000000011" 2 1 false]
[:R "1010110011000000110110" 1 [:x 1] false]
[:reify-counted "1000000000000000000100" 3 "[object Object] is not ISeqable" false]
[:reify "0000000000000000000000" "count not supported on this type" "[object Object] is not ISeqable" "[object Object] is not ISeqable"]
[:vector "1011101001111100110110" 2 1 false]
[:list "1111100000001001110111" 2 1 false]
[:map "1010110001001100110110" 1 [:a 1] false]
[:set "1010100100000100110110" 1 1 false]
[:range "1111100000011000110111" 3 0 false]
[:lazy "0111100000001000100011" 1 2 false]
[:string "0010000000000000000000" 2 "a" false]
[:nil "1010000000000000000100" 0 nil true]
[:number "1000000000000000000100" 5 "5 is not ISeqable" false]
[:array "0010000000000000000000" 2 1 false]
[:object "0000000000000000000000" "count not supported on this type" "[object Object] is not ISeqable" "[object Object] is not ISeqable"]
[:fn "0000000000000110000000" :threw :threw :threw]
[:masked "1000000000000000000100" "No protocol method ICounted/-count defined for this value" "[object Object] is not ISeqable" "No protocol method ICounted/-count defined for this value"]
[:marker "0000000000000000000000" "count not supported on this type" "[object Object] is not ISeqable" "[object Object] is not ISeqable"]
[:specified "1011101001111100110110" 42 1 false]"#,
    );
    // Protocol tests alone of a program type or a record keep no seqs:
    // 12,375 and 12,576 bytes before; 6,127 and 5,847 before light-cljs-291,
    // 6,622 and 6,372 after (`privateStore` and `kindedStore`). The record
    // keeps its brand reader, so since light-cljs-346 it is a class with a
    // brand of its own plus the `ClassBrand` units: 6,549 to 7,018 bytes.
    // The build's key on every slot, marker and mask name (light-cljs-1009,
    // docs/value-layout.md) adds five bytes a name: the record's 7,500.
    // The tests of chunks, chunk buffers, lazy seqs and chunked conses that
    // `counted?` reads (light-cljs-1029), where those were private stores:
    // the program type's 7,897. The cells' test (`cellOf`), where theirs was
    // a private store (light-cljs-1030): 8,011. The maps' and map
    // transients' tests (light-cljs-1012): 8,387. Their fields told apart
    // (light-cljs-1034): 8,631. The tests of the sets, queues and the other
    // values that became classes (light-cljs-1013), where theirs were
    // private stores: 9,048.
    for source in [
        "(deftype T [] ICounted (-count [_] 7)) (counted? (T.))",
        "(defrecord R [x]) (map? (->R 1))",
    ] {
        assert_keeps_none(source, &EXTENSION_MACHINERY);
        let size = advanced(source).emit_js().len();
        assert!(size < 9_300, "{source}: {size} bytes");
    }
}

/// light-cljs-251: a core protocol read as a value may gain types as
/// `extend-type` of a host type gives them (`(aset ICounted "number"
/// true)`), which only the full protocol tests read, so the program keeps
/// them as for an escaped dispatcher; the operand of `satisfies?` or
/// `implements?` is only read. `ownProtocol` reads a value's mask bit before
/// its marker, as `implementsProtocol` does.
#[test]
fn core_protocol_tables_written_directly_reach_the_protocol_tests() {
    for (source, expected) in [
        (
            "(aset ICounted \"number\" true)
             (let [v (js/parseInt \"5\")] (pr-str [(counted? v) (satisfies? ICounted v)]))",
            "[true true]",
        ),
        (
            "(deftype T [] ICounted (-count [_] 7)) (aset ICounted \"number\" true)
             (let [v (js/parseInt \"5\")] (pr-str [(count (T.)) (counted? v) (satisfies? ICounted v)]))",
            "[7 true true]",
        ),
    ] {
        assert!(units(source).contains(&"protocolHooks.satisfies"), "{source}");
        every_level(source, expected);
    }
    let read = "(deftype T [] ICounted (-count [_] 7))
         (let [v (js/parseInt \"5\")]
           (pr-str [(count (T.)) (counted? v) (satisfies? ICounted v) (implements? ICounted v)]))";
    assert_keeps_none(
        read,
        &[
            "extendType",
            "protocolHooks.satisfies",
            "protocolHooks.implements",
        ],
    );
    every_level(read, "[7 false false false]");
    every_level(
        "(deftype T [] ICounted (-count [_] 7))
         (let [o (js-obj)]
           (set! (.-cljs$lang$protocol_mask$partition0$ o) 2)
           (js/Object.defineProperty o (js* \"'cljs$core$ICounted$$LKEY'\")
             (js-obj \"get\" (fn [] (throw (js/Error. \"marker read\")))))
           (pr-str [(count (T.)) (counted? o)]))",
        "[7 true]",
    );
}

/// light-cljs-251: `count` walks the seq of a value it cannot count directly
/// through `kindHooks.countSeq`, which the units building such values install
/// (`count.seqs`): cells without a count, lazy seqs, host iterators' seqs,
/// `iteration`, `Eduction`, a program type implementing `ISeqable` (which
/// keeps the protocol's unit), `extend-type` and `specify!`; a sorted map's
/// seq without a count through `kindHooks.countTreeSeq`. Each value in a
/// program of its own counts as at `None`, and `count` of anything else keeps
/// no seqs.
#[test]
fn count_walks_the_seqs_of_the_values_that_need_it() {
    for source in [
        "(count (.split \"a,b\" \",\"))",
        "(deftype T [] ICounted (-count [_] 7)) (count (T.))",
        "(defrecord R [x]) (count (->R 1))",
        "(count (reify ICounted (-count [_] 3)))",
    ] {
        assert_keeps_none(source, &["accumulatingCount", "count.seqs", "seq", "next"]);
    }
    // 8,948 bytes before; the brand test for a vector, a class since
    // light-cljs-234, is about 170 bytes more than a WeakMap, and the seq
    // stores' `privateStore`, since light-cljs-259, about 320 more than theirs,
    // and `kindedStore` for the maps and sets, since light-cljs-291, about 490
    // more (1,876 to 2,361 bytes), and the kind brand they share, since
    // light-cljs-990 (2026-11-14), about 220 more (2,288 to 2,510 bytes), and
    // the stores' metadata read by descriptor, since light-cljs-1024 (the
    // Pants engine has no `Object.hasOwn`), 2,575 to 2,600, and the runtime
    // key's registration, since light-cljs-1025 (2026-11-15), about 290 more
    // (2,894 bytes), and the view test (`viewOf`, `isView`, `viewCells`),
    // since light-cljs-1010 (2026-11-15), where the views' store was a
    // `privateStore` (3,141 bytes), and the tests of chunks, chunk buffers,
    // lazy seqs and chunked conses, since light-cljs-1029 (2026-11-16),
    // where theirs were (3,725 bytes), and the cells' test (`cellOf`,
    // `seqCells`), since light-cljs-1030 (2026-11-16), where theirs was a
    // `privateStore` (3,892 bytes), and the maps' and map transients' tests
    // (`mapOf`, `maps`, `transientMapState`), since light-cljs-1012
    // (2026-11-16), where theirs were private stores (4,273 bytes), and
    // since light-cljs-1034 (2026-11-16) `maps`, `mapOf` and `count` telling
    // the array and hash maps' fields apart (4,633 bytes), and since
    // light-cljs-1013 (2026-11-16) the tests of the sets and queues that
    // `count` and `seqableValue` read, where theirs were private stores
    // (5,218 bytes), and since light-cljs-1037 the records' kind test
    // (`recordCells`) that `countedValue` reads, where `maps.records`
    // answered records as maps, and since light-cljs-1046 (2026-11-17) the
    // keyed kinds of the stores' classes (`classKinds`, `kindTable`,
    // `storeHolds`), by which `seqableValue` tests one store where it probed
    // each in turn (5,270 to 5,622 bytes).
    let size = advanced("(count (.split \"a,b\" \",\"))").emit_js().len();
    assert!(size < 5_700, "{size} bytes");
    for (source, expected) in [
        ("(count (.split \"a,b\" \",\"))", "2"),
        ("(defrecord R [x]) (count (assoc (->R 1) :y 2))", "2"),
        ("(count (reify ICounted (-count [_] 3)))", "3"),
        ("(count (lazy-seq [1 2]))", "2"),
        ("(count (cons 1 [2 3]))", "3"),
        ("(count (range 0.5 3))", "3"),
        ("(count (keys {:a 1 :b 2}))", "2"),
        ("(count (re-seq #\"a\" \"aba\"))", "2"),
        (
            "(count (chunk-cons (chunk (doto (chunk-buffer 2) (chunk-append 1) (chunk-append 2))) nil))",
            "2",
        ),
        ("(count (es6-iterator-seq (.values #js [1 2 3])))", "3"),
        (
            "(count (iteration (fn [k] (when (< k 3) k)) :initk 0 :kf inc))",
            "3",
        ),
        ("(count (eduction (map inc) [1 2 3]))", "3"),
        ("(count (subseq (sorted-map 1 2 3 4 5 6) > 1))", "2"),
        // The seq's stack holds the root, whose right node the count walks
        // to, as upstream's (light-cljs-1035; an array seq had no stack).
        (
            "(let [s (seq (sorted-map 1 2 3 4))] (count (PersistentTreeMapSeq. nil (.-stack s) true -1 nil)))",
            "2",
        ),
        (
            "(deftype S [] ISeqable (-seq [_] (list 1 2 3))) (count (S.))",
            "3",
        ),
        ("(count (reify ISeqable (-seq [_] (list 1 2))))", "2"),
        (
            "(deftype T []) (extend-type T ISeqable (-seq [_] (list 1 2))) (count (T.))",
            "2",
        ),
        (
            "(extend-type js/Date ISeqable (-seq [_] (list 1))) (count (js/Date. 0))",
            "1",
        ),
        (
            "(count (specify! #js {} ISeqable (-seq [_] (list 1 2))))",
            "2",
        ),
    ] {
        every_level(source, expected);
    }
    let units = units(
        "(let [s (seq (sorted-map 1 2 3 4))] (count (PersistentTreeMapSeq. nil (.-stack s) true -1 nil)))",
    );
    assert!(
        units.contains(&"treeMapSeqConstructor.countSlot"),
        "{units:?}"
    );
    // Each core dispatcher and its `protocolHooks` install is a unit of its
    // own (light-cljs-254), so a walk keeps `countProtocol` without the
    // dispatchers the unit `seqProtocol` declared with it, nor the `get`,
    // `contains` and entry accessors they dispatch to.
    let walked = "(deftype S [] ISeqable (-seq [_] (list 1 2 3))) (count (S.))";
    let dropped = [
        "seqProtocol",
        "firstProtocol",
        "restProtocol",
        "nextProtocol",
        "lookupProtocol",
        "containsProtocol",
        "keyProtocol",
        "valProtocol",
        "protocolHooks.firstProtocol",
        "protocolHooks.nextProtocol",
        "protocolHooks.keyProtocol",
        "protocolHooks.valProtocol",
        "get",
        "lookup",
        "contains",
        "entryKey",
        "entryVal",
    ];
    for unit in dropped {
        assert!(
            RuntimeManifest::embedded().unit_named(unit).is_some(),
            "{unit}"
        );
    }
    let units = assert_keeps_none(walked, &dropped);
    for unit in [
        "accumulatingCount",
        "countProtocol",
        "protocolHooks.countProtocol",
    ] {
        assert!(units.contains(&unit), "{unit}: {units:?}");
    }
    // A host value the program builds can declare `ISeqable` (or any core
    // protocol) by a protocol mask, whose name keeps the protocol hooks and
    // the units of `PROTOCOL_ROOTS`, with or without a program type.
    for (source, expected) in [
        (
            "(deftype T [] ICounted (-count [_] 7))
             (let [o (js-obj)]
               (set! (.-cljs$lang$protocol_mask$partition0$ o) 8388608)
               (set! (.-cljs$core$ISeqable$_seq$arity$1 o) (fn [_] nil))
               (array (count (T.)) (count o)))",
            "[ 7, 0 ]",
        ),
        (
            "(let [o (js-obj)]
               (set! (.-cljs$lang$protocol_mask$partition0$ o) 8388608)
               (set! (.-cljs$core$ISeqable$_seq$arity$1 o) (fn [_] (list 1 2)))
               (count o))",
            "2",
        ),
        (
            "(deftype T [] ICounted (-count [_] 7)) (def n (count (T.)))
             (try (count (doto (js-obj) (-> .-cljs$lang$protocol_mask$partition0$ (set! 8388608))))
               (catch :default e (.-message e)))",
            "No protocol method ISeqable/-seq defined for this value",
        ),
        (
            "(try (count (doto (js-obj) (-> .-cljs$lang$protocol_mask$partition0$ (set! 2))))
               (catch :default e (.-message e)))",
            "No protocol method ICounted/-count defined for this value",
        ),
    ] {
        every_level(source, expected);
    }
    let masked =
        self::units("(count (doto (js-obj) (-> .-cljs$lang$protocol_mask$partition0$ (set! 2))))");
    for unit in ["installMethods", "ISeqable", "ISequential", "count.seqs"] {
        assert!(masked.contains(&unit), "{unit}: {masked:?}");
    }
    // A mask name the program computes is a string, which names no keyed
    // mask (docs/value-layout.md, section 1, light-cljs-1025): the object is
    // a host object to `count` at every level. (Before the key was real, the
    // computed name reached the mask, outside the contract.)
    let computed = "(deftype T [] ICounted (-count [_] 7)) (def n (count (T.)))
         (let [o (js-obj)]
           (aset o (.concat \"cljs$lang$protocol_mask$\" \"partition0$\") 8388608)
           (try (count o) (catch js/TypeError _ :unsupported) (catch :default e (.-message e))))";
    every_level(computed, "count not supported on this type");
}

/// light-cljs-251: `satisfies?` and `implements?` only read their protocol
/// operand, so it does not keep `extendType`, unless the program replaces
/// them: a replacement may write the protocol's table, which the full
/// protocol tests then read, as at `None`.
#[test]
fn replaced_protocol_predicates_keep_the_full_protocol_tests() {
    for source in [
        "(deftype T [] ICounted (-count [_] 7))
         (set! cljs.core/satisfies? (fn [p _] (aset p \"number\" true)))
         (satisfies? ICounted nil)
         (array (counted? (T.)) (counted? (js/parseInt \"5\")))",
        "(deftype T [] ICounted (-count [_] 7))
         (with-redefs [cljs.core/implements? (fn [p _] (aset p \"number\" true))]
           (implements? ICounted nil))
         (array (counted? (T.)) (counted? (js/parseInt \"5\")))",
    ] {
        assert!(
            units(source).contains(&"protocolHooks.satisfies"),
            "{source}"
        );
        every_level(source, "[ true, true ]");
    }
}

/// light-cljs-251 (light-cljs-254): the map hooks records need (`makeMap.seq`,
/// `.conj`, `.print`, the transient ones, `str.collections`, ...) wait for
/// `recordMethods`, which only a program with records keeps, instead of
/// `makeType`, which every program type keeps. A `deftype` with a seq keeps
/// none of them, and a record still seqs, prints, compares, converts and
/// carries metadata at every level.
#[test]
fn map_hooks_for_records_wait_for_records() {
    let deftype = "(deftype T [x]) (first (seq [(.-x (T. 1))]))";
    assert_keeps_none(deftype, &["recordMethods", "makeMap.seq", "mapEntry"]);
    every_level(deftype, "1");
    every_level(
        "(defrecord R [x])
         (pr-str [(seq (->R 1)) (->R 1) (= (->R 1) (->R 1)) (= (->R 1) {:x 1}) (conj (->R 1) [:y 2])
                  (into {} (->R 1)) (persistent! (conj! (transient {}) [:a 1])) (with-meta (->R 1) {:m 1})
                  (meta (with-meta (->R 1) {:m 1})) (count (->R 1)) (get (assoc (->R 1) :z 3) :z)])",
        "[([:x 1]) #cljs.user.R{:x 1} true false #cljs.user.R{:x 1, :y 2} {:x 1} {:a 1} #cljs.user.R{:x 1} {:m 1} 1 3]",
    );
}

#[test]
fn host_facing_methods_of_builtin_prototypes() {
    every_level(
        "(let [v [1 2 3 2]]
           (array (.indexOf v 2) (.lastIndexOf v 2) (.indexOf v 9) (.equiv v [1 2 3 2])
                  (.equiv v '(1 2 3 2)) (.equiv v [1 2]) (.call v nil 1) (.-length (js/Array.from v))
                  (.join (js/Array.from v) \"\")))",
        "[\n  1,    3,    -1,\n  true, true, false,\n  2,    4,    '1232'\n]",
    );
    every_level(
        "(let [v (vec (range 40))]
           (array (.indexOf v 33) (.join (.slice (js/Array.from v) 30 35) \",\")
                  (.equiv v (range 40)) (.equiv v (range 41))))",
        "[ 33, '30,31,32,33,34', true, false ]",
    );
    every_level(
        "(array (.equiv :a :a) (.equiv :a :b) (.call :a nil {:a 1}) (.call :b nil {:a 1} 5)
                (.call :a nil nil) (.call :a nil #js [1]) (.equiv 'x 'x) (.call 'x nil {'x 2}))",
        "[\n  true, false, 1,\n  5,    null,  null,\n  true, 2\n]",
    );
    every_level(
        "(let [m {:a 1 :b 2}]
           (array (.get m :a) (.get m :c 3) (.has m :b) (.has m :c) (.equiv m {:b 2 :a 1})
                  (.-length (js/Array.from (.keys m))) (.-length (js/Array.from (.values m)))
                  (.join (.map (js/Array.from (.entries m)) (fn [e] (aget e 1))) \"\")
                  (.call m nil :b) (.equiv m {:a 1})))",
        "[\n  1,     3,    true,\n  false, true, 0,\n  0,     '',   2,\n  false\n]",
    );
    every_level(
        "(let [s #{1 2}]
           (array (.has s 1) (.has s 3) (.equiv s #{2 1}) (.call s nil 2) (pr-str (.call s nil 3 :none))
                  (.-length (js/Array.from s))))",
        "[ true, false, true, 2, ':none', 2 ]",
    );
}

#[test]
fn each_kind_branch_of_the_generic_operations() {
    // seq, first, rest, next, count, nth and equality over every kind.
    every_level(
        "(let [kinds [[1 2 3] (vec (range 40)) '(1 2 3) (cons 0 [1 2]) (lazy-seq [1 2])
                      (range 3) (range 0.5 3) (seq (vec (range 40))) {:a 1} (into {} (map vector (range 20) (range 20)))
                      (sorted-map 2 :b 1 :a) #{1 2} (sorted-set 3 1 2) (repeat 3 :x) (take 3 (cycle [1 2]))
                      (take 3 (iterate inc 0)) (subvec [1 2 3 4] 1 3) (conj cljs.core.PersistentQueue.EMPTY 1 2)
                      (keys {:a 1 :b 2}) (vals {:a 1 :b 2}) (rseq [1 2 3]) (drop 1 (vec (range 40)))]]
           (array (pr-str (map first kinds)) (pr-str (map count kinds)) (pr-str (map (comp first rest) kinds))
                  (pr-str (map #(nth (seq %) 1 :none) kinds)) (pr-str (map #(= % (seq %)) kinds))
                  (pr-str (map #(= (hash %) (hash (into (empty %) %))) kinds))))",
        "[\n  '(1 0 1 0 1 0 0.5 0 [:a 1] [0 0] [1 :a] 1 1 :x 1 0 2 1 :a 1 3 1)',\n  '(3 40 3 3 2 3 3 40 1 20 2 2 3 3 3 3 2 2 2 2 3 39)',\n  '(2 1 2 1 2 1 1.5 1 nil [1 1] [2 :b] 2 2 :x 2 1 3 2 :b 2 2 2)',\n  '(2 1 2 1 2 1 1.5 1 :none [1 1] [2 :b] 2 2 :x 2 1 3 2 :b 2 2 2)',\n  '(true true true true true true true true false false false false false true true true true true true true true true)',\n  '(true true false false false false false false true true true true true true true false true true false false false false)'\n]",
    );
    // Equality across kinds and with host values.
    every_level(
        "(array (= [1 2] '(1 2)) (= [1 2] (lazy-seq [1 2])) (= (range 3) [0 1 2]) (= {:a 1} (hash-map :a 1))
                (= {:a 1} (sorted-map :a 1)) (= #{1} (sorted-set 1)) (= [1 2] #js [1 2]) (= :a :a) (= 'x 'x)
                (= [1 [2 {:a #{3}}]] [1 [2 {:a #{3}}]]) (= {:a 1} {:a 2}) (= #{1} #{2}) (= [] '()) (= {} []))",
        "[\n  true,  true,  true,\n  true,  true,  true,\n  false, true,  true,\n  true,  false, false,\n  true,  false\n]",
    );
}

#[test]
fn transients_chunks_and_entries_through_the_generic_operations() {
    every_level(
        "(let [tv (transient [1 2 3]) tm (transient {:a 1}) chunk (chunk-first (seq (vec (range 40))))]
           (array (nth tv 1) (count tv) (get tm :a) (count tm) (nth chunk 3) (count chunk)
                  (pr-str (key (first {:a 1}))) (val (first {:a 1})) (pr-str (keys {:a 1})) (pr-str (find {:a 1} :a))
                  (count (persistent! (reduce conj! (transient {}) (map vector (range 9) (range 9)))))))",
        "[ 2, 3, 1, 1, 3, 32, ':a', 1, '(:a)', '[:a 1]', 9 ]",
    );
    // A projection of entries that are not map entries fails as `-key` does.
    every_level(
        "(try (first (keys [[1 2]])) (catch :default e (.-message e)))",
        "No protocol method IMapEntry/-key defined for this value",
    );
}

/// Vectors are 32-way tries (light-cljs-234): 33,000 items take three levels
/// above the leaves, which `conj`, `pop`, `assoc` and their transient forms
/// grow and shrink, and a tail the public constructor received stays the
/// caller's array through `assoc` and a seq.
#[test]
fn vectors_are_tries_across_leaf_and_level_boundaries() {
    every_level(
        "(let [n 33000 v (reduce conj [] (range n)) t (persistent! (reduce conj! (transient []) (range n)))
               p (loop [v v] (if (> (count v) 31) (recur (pop v)) v))
               tp (loop [t (transient v)] (if (> (count t) 31) (recur (pop! t)) (persistent! t)))
               a (reduce (fn [v i] (assoc v i (- i))) v (range 0 n 997))
               ta (persistent! (reduce (fn [t i] (assoc! t i (- i))) (transient v) (range 0 n 997)))]
           (pr-str [(= v t (range n)) (count v) (nth v 32767) (nth v 32800) (peek v) (= p tp (range 31))
                    (= a ta) [(nth a 997) (nth a 998) (nth v 997)] (reduce + (rseq p))
                    (= (seq (vec (range 1100))) (range 1100)) (count (rest (seq v))) (first (drop 32790 v))
                    (= (range 32) (vec (chunk-first (seq v))))
                    (= v (PersistentVector. nil n (.-shift v) (.-root v) (.-tail v) nil))
                    (let [x (vec (range 40)) arr (.-tail x) y (PersistentVector. nil 40 (.-shift x) (.-root x) arr nil)
                          z (assoc y 0 :z)]
                      (dorun (seq z)) (aset arr 0 :w) [(nth y 32) (nth z 32) (nth z 0)])]))",
        "[true 33000 32767 32800 32999 true true [-997 998 997] 465 true 32999 32790 true true [:w :w :z]]",
    );
}

/// A set of more than 8 members is a trie (light-cljs-236): `conj`, `disj`,
/// lookup, a call, `into`, `set`, equality and hashing agree across array
/// nodes and collisions. From review: a transient of a set the public
/// constructor built of nil, whose member array a lookup gave a root, still
/// becomes a hash set past 8 members and looks up after persistence.
#[test]
fn hash_sets_are_tries() {
    every_level(
        "(let [n 2000 s (reduce conj #{} (range n)) t (into #{} (range n)) u (set (range n))
               d (reduce disj s (range 0 n 2))
               k (PersistentHashSet. nil nil nil) _ (get k :missing) kt (transient k)]
           (doseq [i (range 9)] (conj! kt i))
           (let [kp (persistent! kt)]
             (pr-str [(= s t u (set (range n))) (count s) (contains? s 1999) (get s 7) (s 8) (s n :none)
                      (= (hash s) (hash t)) (count d) (contains? d 2) (contains? d 3)
                      (= (conj d 2 \"Aa\" \"BB\") (into d [2 \"BB\" \"Aa\"])) (count kp) (get kt 0) (kp 8)])))",
        "[true 2000 true 7 8 :none true 1000 false true true 9 0 8]",
    );
}

/// A subvec is a view of its vector's trie (light-cljs-234): reading, reducing
/// and seqs across leaf boundaries, a subvec of a subvec, and `pop`, `assoc`
/// and `conj`, which leave the vector viewed as it was. The public Subvec
/// constructor takes the same view, and keeps its source's protocol `nth`.
/// `mapv`, `filterv` and `into` build through transients.
#[test]
fn subvecs_are_views_of_their_vectors() {
    every_level(
        "(let [v (vec (range 100)) s (subvec v 3 70) s2 (subvec s 10 50) c (Subvec. nil v 2 40)]
           (pr-str [(count s) (nth s 0) (nth s 66) (nth s 0.5) (get s 67 :nf) (peek s) (s 4)
                    (reduce + s2) (reduce + 0 s2) (reduce + (subvec v 5 5)) (reduce + 1 (subvec v 5 6))
                    (reduce (fn [a x] (if (> x 50) (reduced x) a)) 0 s)
                    (= s2 (range 13 53)) (= (hash s2) (hash (vec (range 13 53)))) (identical? v (.-v s2))
                    (count (pop s2)) (peek (pop s2)) (nth (assoc s2 5 :x) 5) (peek (conj s2 :end))
                    (first (drop 33 s)) (vec (rest (subvec v 60 66))) (first (rseq s2))
                    (reduce-kv (fn [m k x] (+ m k x)) 0 (subvec v 1 4)) (.-tail (pop (subvec v 0 40)))
                    (= v (range 100)) (count c) (nth c 37) (reduce + c) (pop (Subvec. nil [1 2 3] 1 3))
                    (mapv inc (subvec v 95)) (filterv odd? s2) (count (into (subvec v 0 2) (range 40)))
                    (mapv + [1 2] [10 20 30])]))",
        "[67 3 69 3 :nf 69 7 1300 1300 0 6 51 true true true 39 51 :x :end 36 [61 62 63 64 65] 52 9 \
         #js [32 33 34 35 36 37 38] true 38 39 779 [2] [96 97 98 99 100] \
         [13 15 17 19 21 23 25 27 29 31 33 35 37 39 41 43 45 47 49 51] 42 [11 22]]",
    );
}

/// A Subvec over a caller's array, through a public PersistentVector's tail or
/// as the source itself, reads that array's current items wherever it is read.
#[test]
fn subvecs_of_caller_arrays_read_them_live() {
    every_level(
        "(let [arr #js [1 2] x (PersistentVector. nil 2 5 nil arr nil) s (Subvec. nil x 0 2 nil)
               before (vec (js/Array.from s)) a (to-array (range 40)) t (Subvec. nil a 0 40 nil)]
           (aset arr 0 9) (aset a 0 :changed) (aset a 39 :end)
           (pr-str [before (vec (js/Array.from s)) (nth s 0) (nth t 0) (nth t 39) (first t) (last t)]))",
        "[[1 2] [9 2] 9 :changed :end :changed :end]",
    );
}

/// Where a program replaces a vector's seq, `reduce` and `rseq` of a vector
/// still walk that seq; a Subvec reads an `nth` source only when read; `mapv`
/// of one collection reduces it.
#[test]
fn vector_reductions_keep_protocol_implementations() {
    every_level(
        "(extend-type PersistentVector ISeqable (-seq [_] (list 10 20)))
         (let [v (vec (range 40))
               r (reify IIndexed (-nth [_ i] (if (= i 35) (throw (js/Error. \"read\")) i)) (-nth [_ i _] i))
               s (Subvec. nil r 0 40 nil)]
           (str (reduce + 0 v) \" \" (reduce + v) \" \" (pr-str (rseq v)) \" \" (reduce + 0 (subvec v 0 35))
                \" \" (nth s 3) \" \" (nth s 39) \" \"
                (let [m (mapv inc (reify IReduce (-reduce [_ f init] (f (f init 1) 2))))] (str (count m) (nth m 0) (nth m 1)))))",
        "30 30 (20 10) 595 3 39 223",
    );
}

/// A protocol method a program installs on a vector itself, through its mask
/// and slot, answers `conj`: the fast path of `conj` onto a vector skips the
/// dispatch only where no protocol implementation can exist (light-cljs-234).
#[test]
fn vector_fast_paths_honor_direct_protocol_slots() {
    every_level(
        "(let [v [1]]
           (set! (.-cljs$lang$protocol_mask$partition0$ v) 8)
           (set! (.-cljs$core$ICollection$_conj$arity$2 v) (fn [_ x] [:custom x]))
           (pr-str [(conj v 2) (conj [5] 6) (into [] (range 3))]))",
        "[[:custom 2] [5 6] [0 1 2]]",
    );
}

/// A vector whose own protocol slots a program sets to nil or false answers
/// every slot-first operation (`nth`, `count`, `peek`, `conj`, `assoc`,
/// `get`) by the operation's older path, whose lookups take either for no
/// method, as they did before slot-first dispatch (light-cljs-1009).
#[test]
fn vector_slot_first_preserves_missing_slot_fallbacks() {
    for missing in ["nil", "false"] {
        every_level(
            &format!(
                "(let [v [1 2]]
                   (set! (.-cljs$core$IIndexed$_nth$arity$2 v) {missing})
                   (set! (.-cljs$core$IIndexed$_nth$arity$3 v) {missing})
                   (set! (.-cljs$core$ICounted$_count$arity$1 v) {missing})
                   (set! (.-cljs$core$IStack$_peek$arity$1 v) {missing})
                   (set! (.-cljs$core$ICollection$_conj$arity$2 v) {missing})
                   (set! (.-cljs$core$IAssociative$_assoc$arity$3 v) {missing})
                   (set! (.-cljs$core$ILookup$_lookup$arity$2 v) {missing})
                   (set! (.-cljs$core$ILookup$_lookup$arity$3 v) {missing})
                   (pr-str [(nth v 0) (nth v 5 :none) (count v) (peek v) (conj v 3) (assoc v 0 :a)
                            (get v 1) (get v 7 :none)]))"
            ),
            "[1 :none 2 2 [1 2 3] [:a 2] 2 :none]",
        );
    }
    // Likewise a map entry, a subvector (of `subvec` and of its public
    // constructor) with these slots and `-seq`, and a transient vector with
    // `-nth`, `-count` and `-lookup`, also once it is persistent
    // (light-cljs-1028).
    for missing in ["nil", "false"] {
        every_level(
            &format!(
                "(let [answers (fn [v]
                                 (set! (.-cljs$core$IIndexed$_nth$arity$2 v) {missing})
                                 (set! (.-cljs$core$IIndexed$_nth$arity$3 v) {missing})
                                 (set! (.-cljs$core$ICounted$_count$arity$1 v) {missing})
                                 (set! (.-cljs$core$IStack$_peek$arity$1 v) {missing})
                                 (set! (.-cljs$core$ICollection$_conj$arity$2 v) {missing})
                                 (set! (.-cljs$core$IAssociative$_assoc$arity$3 v) {missing})
                                 (set! (.-cljs$core$ILookup$_lookup$arity$2 v) {missing})
                                 (set! (.-cljs$core$ILookup$_lookup$arity$3 v) {missing})
                                 (set! (.-cljs$core$ISeqable$_seq$arity$1 v) {missing})
                                 [(nth v 0) (nth v 5 :none) (count v) (peek v) (conj v 3) (assoc v 0 :a)
                                  (get v 1) (get v 7 :none) (seq v)])
                       t (transient [1 2])]
                   (set! (.-cljs$core$IIndexed$_nth$arity$2 t) {missing})
                   (set! (.-cljs$core$IIndexed$_nth$arity$3 t) {missing})
                   (set! (.-cljs$core$ICounted$_count$arity$1 t) {missing})
                   (set! (.-cljs$core$ILookup$_lookup$arity$2 t) {missing})
                   (set! (.-cljs$core$ILookup$_lookup$arity$3 t) {missing})
                   (let [editable [(nth t 0) (nth t 5 :none) (count t) (get t 1) (get t 7 :none)]]
                     (persistent! t)
                     (pr-str [(answers (first {{:k 1}})) (answers (subvec [1 2 3] 1))
                              (answers (Subvec. nil [1 2 3] 1 3 nil)) editable
                              (try (count t) (catch :default _ :persisted)) (nth t 5 :none)
                              (try (nth t 0) (catch :default _ :persisted)) (try (get t 0) (catch :default _ :persisted))])))"
            ),
            "[[:k :none 2 1 [:k 1 3] [:a 1] 1 :none (:k 1)] [2 :none 2 3 [2 3 3] [:a 3] 3 :none (2 3)] \
             [2 :none 2 3 [2 3 3] [:a 3] 3 :none (2 3)] [1 :none 2 2 :none] :persisted :none :persisted :persisted]",
        );
    }
    // Likewise a runtime class's prototype slots of the lookups (`-lookup`,
    // `-nth`, `-count`, `-peek`, `-contains-key?`) that a program sets to nil,
    // false or undefined: the class answers, by the lookups' own paths, which
    // such a program keeps (`slotWrites`, light-cljs-1066).
    for missing in ["nil", "false", "js/undefined"] {
        every_level(
            &format!(
                "(let [v [1 2] m {{:a 1}} s #{{:x}} q (conj cljs.core/PersistentQueue.EMPTY 7)]
                   (set! (.. PersistentVector -prototype -cljs$core$ILookup$_lookup$arity$2) {missing})
                   (set! (.. PersistentVector -prototype -cljs$core$ILookup$_lookup$arity$3) {missing})
                   (set! (.. PersistentVector -prototype -cljs$core$IIndexed$_nth$arity$2) {missing})
                   (set! (.. PersistentVector -prototype -cljs$core$IIndexed$_nth$arity$3) {missing})
                   (set! (.. PersistentVector -prototype -cljs$core$ICounted$_count$arity$1) {missing})
                   (set! (.. PersistentVector -prototype -cljs$core$IStack$_peek$arity$1) {missing})
                   (set! (.. PersistentVector -prototype -cljs$core$IAssociative$_contains_key_QMARK_$arity$2) {missing})
                   (set! (.. PersistentArrayMap -prototype -cljs$core$ILookup$_lookup$arity$2) {missing})
                   (set! (.. PersistentArrayMap -prototype -cljs$core$ICounted$_count$arity$1) {missing})
                   (set! (.. PersistentHashSet -prototype -cljs$core$IAssociative$_contains_key_QMARK_$arity$2) {missing})
                   (set! (.. PersistentHashSet -prototype -cljs$core$ILookup$_lookup$arity$3) {missing})
                   (set! (.. PersistentQueue -prototype -cljs$core$IStack$_peek$arity$1) {missing})
                   (set! (.. PersistentQueue -prototype -cljs$core$ICounted$_count$arity$1) {missing})
                   (pr-str [(get v 0) (get v 0 :missing) (get v 7) (get v 7 :none) (nth v 1) (nth v 5 :none)
                            (count v) (peek v) (contains? v 1) (contains? v 2) (get m :a) (count m)
                            (contains? s :x) (get s :x :none) (get s :y :none) (peek q) (count q)]))"
            ),
            "[1 1 nil :none 2 :none 2 2 true false 1 1 true :x :none 7 1]",
        );
    }
    // ... and a value's own slots set to nil or false over the methods
    // `extend-type` gave its class: the value answers by its class, not by
    // the extension its own slots hide.
    for missing in ["nil", "false"] {
        every_level(
            &format!(
                "(extend-type PersistentVector
                   ICounted (-count [_] 42)
                   IIndexed (-nth ([_ _] :ext) ([_ _ _] :ext))
                   ILookup (-lookup ([_ _] :ext) ([_ _ _] :ext))
                   IAssociative (-contains-key? [_ _] :ext)
                   IStack (-peek [_] :ext))
                 (let [v (vector 1 2) w (vector 3)]
                   (set! (.-cljs$core$ICounted$_count$arity$1 v) {missing})
                   (set! (.-cljs$core$IIndexed$_nth$arity$2 v) {missing})
                   (set! (.-cljs$core$IIndexed$_nth$arity$3 v) {missing})
                   (set! (.-cljs$core$ILookup$_lookup$arity$2 v) {missing})
                   (set! (.-cljs$core$ILookup$_lookup$arity$3 v) {missing})
                   (set! (.-cljs$core$IAssociative$_contains_key_QMARK_$arity$2 v) {missing})
                   (set! (.-cljs$core$IStack$_peek$arity$1 v) {missing})
                   (pr-str [(count v) (nth v 1) (nth v 5 :none) (get v 0) (get v 7 :none) (contains? v 1)
                            (peek v) (count w) (nth w 0) (get w 0) (contains? w 0) (peek w)]))"
            ),
            "[2 2 :none 1 :none true 2 42 :ext :ext :ext :ext]",
        );
    }
}

/// The seq walk's slots (`-seq`, `-first`, `-rest`, `-next`) that a program
/// sets to nil, false or undefined on a runtime class's prototype, or on its
/// value over a method `extend-type` gave the class, answer as master
/// f533efb1 did, by the operations' own paths, which such a program keeps
/// (`slotWrites`, light-cljs-1071): `seq`, `first`, `rest` and `next` of the
/// classes' values, and `first` and `rest` of host values and program types
/// whose seqs are such values, and which a seq without the slot's protocol
/// (a program type's `-seq` giving a vector) does not reach. The answers are
/// master's at every level, but where master threw for a program type whose
/// `-seq` gives a lazy seq in every program: its `first`, `rest` and `next`
/// answer as upstream's since light-cljs-1054, and throw as master's where
/// the lazy seq's slot they read is cleared.
#[test]
fn seq_walk_slots_a_program_clears_answer_as_master() {
    let program = r#"
        (let [b (chunk-buffer 2)
              _ (do (chunk-append b 1) (chunk-append b 2))
              values [[1 2 3] (subvec [1 2 3] 1) (first {:k 1}) {:a 1 :b 2} (zipmap (range 10) (range 10))
                      (sorted-map :a 1 :b 2) #{1 2} (sorted-set 1 2) (conj cljs.core/PersistentQueue.EMPTY 1 2)
                      (list 1 2 3) () (cons 1 (list 2 3)) (lazy-seq [1 2 3]) (range 3) (range 0.5 3)
                      (seq (array 1 2 3)) (seq [1 2 3]) (seq (vec (range 40))) (chunk-cons (chunk b) (list 3))
                      (seq (zipmap (range 10) (range 10))) (seq {:a 1 :b 2}) (rseq [1 2 3]) (iterate inc 0)
                      (repeat 3 :x) (cycle [1 2]) (array 1 2 3) "abc" (js/Set. #js [1 2 3])
                      (reify ISeqable (-seq [_] (list 1 2 3))) (reify ISeqable (-seq [_] [1 2 3]))
                      (reify ISeqable (-seq [_] (map inc [1 2 3])))]
              labels (array "vector" "subvec" "entry" "array-map" "hash-map" "sorted-map" "set" "sorted-set" "queue"
                            "list" "empty-list" "cons" "lazy-seq" "range" "float-range" "array-seq" "vector-seq"
                            "chunked-seq" "chunked-cons" "node-seq" "map-seq" "rseq" "iterate" "repeat" "cycle"
                            "array" "string" "iterable" "type" "vector-type" "lazy-type")
              attempt (fn [f] (try (pr-str (f)) (catch :default e "E")))]
          (doseq [slot [(.. PersistentVector -prototype) (.. Subvec -prototype) (.. MapEntry -prototype)
                        (.. PersistentArrayMap -prototype) (.. PersistentHashMap -prototype) (.. PersistentTreeMap -prototype)
                        (.. PersistentHashSet -prototype) (.. PersistentTreeSet -prototype) (.. PersistentQueue -prototype)
                        (.. List -prototype) (.. EmptyList -prototype) (.. Cons -prototype) (.. LazySeq -prototype)
                        (.. Range -prototype) (.. IndexedSeq -prototype) (.. ChunkedSeq -prototype) (.. ChunkedCons -prototype)
                        (.. NodeSeq -prototype) (.. PersistentArrayMapSeq -prototype) (.. RSeq -prototype)
                        (.. Iterate -prototype) (.. Repeat -prototype) (.. Cycle -prototype)]]
            SETS)
          (.join (into-array (map (fn [l v]
                                    (str l " " (attempt #(vec (take 4 (seq v)))) " " (attempt #(first v))
                                         " " (attempt #(vec (take 3 (rest v)))) " " (attempt #(vec (take 3 (next v))))
                                         " " (attempt #(empty? v)) " " (attempt #(counted? (seq v)))))
                                  labels values))
                 "\n"))"#;
    let seq = "(set! (.-cljs$core$ISeqable$_seq$arity$1 slot) MISSING)";
    let first = "(set! (.-cljs$core$ISeq$_first$arity$1 slot) MISSING)";
    let rest = "(set! (.-cljs$core$ISeq$_rest$arity$1 slot) MISSING)";
    let next = "(set! (.-cljs$core$INext$_next$arity$1 slot) MISSING)";
    let all = [seq, first, rest, next].join("\n");
    // Master answered alike for each slot alone and for all four.
    let common = concat!(
        "vector [1 2 3] 1 [2 3] [2 3] false true\n",
        "subvec [2 3] 2 [3] [3] false true\n",
        "entry [:k 1] :k [1] [1] false true\n",
        "array-map [[:a 1] [:b 2]] [:a 1] [[:b 2]] [[:b 2]] false true\n",
        "hash-map [[0 0] [1 1] [2 2] [3 3]] [0 0] [[1 1] [2 2] [3 3]] [[1 1] [2 2] [3 3]] false false\n",
        "sorted-map [[:a 1] [:b 2]] [:a 1] [[:b 2]] [[:b 2]] false true\n",
        "set [1 2] 1 [2] [2] false false\n",
        "sorted-set [1 2] 1 [2] [2] false false\n",
        "queue [1 2] 1 [2] [2] false false\n",
        "list [1 2 3] 1 [2 3] [2 3] false true\n",
        "empty-list [] nil [] [] true true\n",
        "cons [1 2 3] 1 [2 3] [2 3] false false\n",
        "lazy-seq [1 2 3] 1 [2 3] [2 3] false true\n",
        "range [0 1 2] 0 [1 2] [1 2] false true\n",
        "float-range [0.5 1.5 2.5] 0.5 [1.5 2.5] [1.5 2.5] false false\n",
        "array-seq [1 2 3] 1 [2 3] [2 3] false true\n",
        "vector-seq [1 2 3] 1 [2 3] [2 3] false true\n",
        "chunked-seq [0 1 2 3] 0 [1 2 3] [1 2 3] false true\n",
        "chunked-cons [1 2 3] 1 [2 3] [2 3] false false\n",
        "node-seq [[0 0] [1 1] [2 2] [3 3]] [0 0] [[1 1] [2 2] [3 3]] [[1 1] [2 2] [3 3]] false false\n",
        "map-seq [[:a 1] [:b 2]] [:a 1] [[:b 2]] [[:b 2]] false true\n",
        "rseq [3 2 1] 3 [2 1] [2 1] false false\n",
        "iterate [0 1 2 3] 0 [1 2 3] [1 2 3] false false\n",
        "repeat [:x :x :x] :x [:x :x] [:x :x] false false\n",
        "cycle [1 2 1 2] 1 [2 1 2] [2 1 2] false false\n",
        "array [1 2 3] 1 [2 3] [2 3] false true\n",
        "string [\"a\" \"b\" \"c\"] \"a\" [\"b\" \"c\"] [\"b\" \"c\"] false true\n",
        "iterable [1 2 3] 1 [2 3] [2 3] false false\n",
        "type [1 2 3] 1 [2 3] [2 3] false true\n",
        "vector-type [1 2 3] E E E false true\n",
    );
    // Master's lazy-type row is "[2 3 4] E E E false false" in each.
    for (sets, missing, lazy) in [
        (all.as_str(), "nil", "E E E"),
        (&all, "false", "E E E"),
        (&all, "js/undefined", "E E E"),
        (seq, "nil", "2 [3 4] [3 4]"),
        (first, "false", "E [3 4] [3 4]"),
        (rest, "js/undefined", "2 E E"),
        (next, "nil", "2 [3 4] [3 4]"),
    ] {
        every_level(
            &program
                .replace("SETS", &sets.replace("MISSING", missing))
                .replace("MISSING", missing),
            &format!("{common}lazy-type [2 3 4] {lazy} false false"),
        );
    }
    // A value's own slots set to nil or false over the methods `extend-type`
    // gave its class, as on master: a vector's or a set's `seq` answers by
    // its class, and its `first`, `rest` and `next` by the protocol lookup of
    // `ISeq` and `INext`, which its class declares only through the
    // extension, and which throws; a lazy seq's take the extension, as
    // another value of each class does.
    for missing in ["nil", "false"] {
        every_level(
            &format!(
                "(extend-type PersistentVector ISeqable (-seq [_] (cons :ext nil)) ISeq (-first [_] :ext) (-rest [_] (cons :ext nil)) INext (-next [_] (cons :ext nil)))
                 (extend-type PersistentHashSet ISeqable (-seq [_] (cons :ext nil)) ISeq (-first [_] :ext) (-rest [_] (cons :ext nil)) INext (-next [_] (cons :ext nil)))
                 (extend-type LazySeq ISeqable (-seq [_] (cons :ext nil)) ISeq (-first [_] :ext) (-rest [_] (cons :ext nil)) INext (-next [_] (cons :ext nil)))
                 (let [values (array (vector 1 2 3) (hash-set 1 2) (lazy-seq [1 2 3]))
                       others (array (vector 1 2 3) (hash-set 1 2) (lazy-seq [1 2 3]))
                       labels (array \"vector\" \"set\" \"lazy-seq\")
                       attempt (fn [f] (try (pr-str (f)) (catch :default e \"E\")))
                       out (array)]
                   (dotimes [i 3]
                     (let [v (aget values i)]
                       (set! (.-cljs$core$ISeqable$_seq$arity$1 v) {missing})
                       (set! (.-cljs$core$ISeq$_first$arity$1 v) {missing})
                       (set! (.-cljs$core$ISeq$_rest$arity$1 v) {missing})
                       (set! (.-cljs$core$INext$_next$arity$1 v) {missing})))
                   (dotimes [i 3]
                     (let [v (aget values i) w (aget others i)]
                       (.push out (str (aget labels i) \" \" (attempt #(seq v)) \" \" (attempt #(first v))
                                       \" \" (attempt #(rest v)) \" \" (attempt #(next v)) \" \" (attempt #(seq w))
                                       \" \" (attempt #(first w)) \" \" (attempt #(rest w)) \" \" (attempt #(next w))))))
                   (.join out \"\\n\"))"
            ),
            concat!(
                "vector (1 2 3) E E E (:ext) :ext (:ext) (:ext)\n",
                "set (1 2) E E E (:ext) :ext (:ext) (:ext)\n",
                "lazy-seq (:ext) :ext (:ext) (:ext) (:ext) :ext (:ext) (:ext)",
            ),
        );
    }
}

/// A vector reading a caller's array reads each item as it is printed or
/// iterated (light-cljs-234): printing within `*print-length*` stops before
/// an item that throws, and an iterator reads the tail as it is when it gets
/// there.
#[test]
fn vectors_of_caller_arrays_print_and_iterate_as_they_read() {
    every_level(
        "(let [a (to-array (range 40))
               _ (js/Object.defineProperty a 39 #js {:get (fn [] (throw (js/Error. \"read too far\")))})
               s (Subvec. nil a 0 40 nil)
               tail (to-array (range 32 40))
               x (PersistentVector. nil 40 5 (.-root (vec (range 40))) tail nil)
               it (.call (aget x js/Symbol.iterator) x)]
           (dotimes [_ 32] (.next it))
           (aset tail 0 :changed)
           (pr-str [(binding [*print-length* 1] (pr-str s)) (.-value (.next it)) (nth x 32)]))",
        "[\"[0 ...]\" :changed :changed]",
    );
}

#[test]
fn seqs_of_host_iterables() {
    // An ES6IteratorSeq: ISeqable and ISeq, not ISequential, equal only to
    // itself, reprobing a nil tail.
    every_level(
        "(let [s (seq (js/Set. #js [1 2 3])) t (seq (.values #js [4 5]))]
           (array (first s) (count s) (nth s 2) (seq? s) (seqable? s) (sequential? s) (counted? s)
                  (= s [1 2 3]) (= s s) (pr-str s) (pr-str (rest t)) (pr-str (next (rest t)))
                  (identical? s (seq s)) (pr-str (cons 0 s)) (reduce + s)))",
        "[\n  1,        3,\n  3,        true,\n  true,     false,\n  false,    false,\n  true,     '#js {}',\n  '#js {}', 'nil',\n  true,     '(0 1 2 3)',\n  6\n]",
    );
    // Small programs keep the native path: no protocol machinery.
    every_level(
        "(let [s (seq (js/Set. #js [1 2 3]))] (array (first s) (first (rest s)) (count s)))",
        "[ 1, 2, 3 ]",
    );
    for unit in PROTOCOL_MACHINERY {
        assert!(
            !units(
                "(let [s (seq (js/Set. #js [1 2 3]))] (array (first s) (first (rest s)) (count s)))"
            )
            .contains(&unit),
            "{unit}"
        );
    }
}

#[test]
fn protocol_extensions_of_builtin_and_host_types_reach_the_generic_operations() {
    every_level(
        "(extend-type PersistentVector ICounted (-count [_] 42))
         (extend-type object ISeqable (-seq [o] (list :obj)))
         (extend-type number IHash (-hash [n] 5))
         (array (count [1 2]) (pr-str (seq #js {})) (= (hash 1) (hash 2)) (count '(1 2)))",
        "[ 42, '(:obj)', false, 2 ]",
    );
    every_level(
        "(deftype Pair [a b] ISeqable (-seq [_] (list a b)) ICounted (-count [_] 2)
           IIndexed (-nth [_ i] (if (zero? i) a b)) (-nth [_ i d] (case i 0 a 1 b d)))
         (let [p (Pair. :x :y)] (array (pr-str (seq p)) (count p) (pr-str (nth p 1)) (pr-str (into [] p)) (= [:x :y] (vec p))))",
        "[ '(:x :y)', 2, ':y', '[:x :y]', true ]",
    );
    every_level(
        "(def s (seq (js/Set. #js [1 2])))
         (specify! s ICounted (-count [_] 10))
         (array (count s) (first s) (seq? s))",
        "[ 10, 1, true ]",
    );
}

#[test]
fn conj_assoc_and_with_meta_over_every_kind() {
    every_level(
        "(defrecord R [a])
         (let [kinds [[1] (subvec [1 2 3] 1) (cljs.core/Subvec. nil [1 2 3] 0 2 nil) {:a 1} (sorted-map :a 1) #{1}
                      (sorted-set 1) (cljs.core/PersistentHashSet. nil {1 nil} nil) '(1) (cons 1 nil) (lazy-seq [1])
                      (conj cljs.core.PersistentQueue.EMPTY 1) (range 2) (seq [1 2]) nil (->R 1)]
               item #(if (or (map? %) (record? %)) [:k 9] 5)
               metas (map #(pr-str (meta (with-meta % {:m 1}))) (remove nil? kinds))]
           (array (pr-str (map #(conj % (item %)) kinds)) (pr-str (map #(count (conj % (item %) (item %))) kinds))
                  (pr-str metas)
                  (pr-str (map #(meta (with-meta % {:m 2})) [(range 2.5) (repeat 2 :x) (iterate inc 0) (cycle [1])
                                                               (rest (seq (vec (range 40)))) (chunk-cons (chunk-first (seq (vec (range 40)))) nil)]))
                  (pr-str (assoc [1 2] 2 3)) (pr-str (assoc (cljs.core/Subvec. nil [1 2 3] 0 2 nil) 0 :x))
                  (pr-str (assoc (cljs.core/PersistentArrayMap. nil 8 (to-array (range 16)) nil) :new 1))
                  (pr-str (assoc (->R 1) :a 2 :b 3)) (pr-str (assoc nil :a 1)) (pr-str (into {} [[:a 1] {:b 2}]))
                  (try (conj {} [1]) (catch :default e (.-message e)))))",
        "[\n  '([1 5] [2 3 5] [1 2 5] {:a 1, :k 9} {:a 1, :k 9} #{1 5} #{1 5} #{1 5} (5 1) (5 1) (5 1) #queue [1 5] (5 0 1) (5 1 2) (5) #cljs.user.R{:a 1, :k 9})',\n  '(3 4 4 2 2 2 2 2 3 3 3 3 4 4 2 2)',\n  '(\"{:m 1}\" \"{:m 1}\" \"{:m 1}\" \"{:m 1}\" \"{:m 1}\" \"{:m 1}\" \"{:m 1}\" \"{:m 1}\" \"{:m 1}\" \"{:m 1}\" \"{:m 1}\" \"{:m 1}\" \"{:m 1}\" \"{:m 1}\" \"{:m 1}\")',\n  '({:m 2} {:m 2} {:m 2} {:m 2} {:m 2} {:m 2})',\n  '[1 2 3]',\n  '[:x 2]',\n  '{0 1, 2 3, 4 5, 6 7, 8 9, :new 1, 10 11, 12 13, 14 15}',\n  '#cljs.user.R{:a 2, :b 3}',\n  '{:a 1}',\n  '{:a 1, :b 2}',\n  'Index out of bounds'\n]",
    );
    every_level(
        "(let [f (with-meta (fn [& xs] (apply + xs)) {:m 1}) g (with-meta f {:m 2})]
           (array (f 1 2) (g 3 4) (pr-str (meta g)) (apply f 1 [2 3]) (apply + 1 2 [3 4 5])
                  (apply + (range 30)) (apply max (repeat 3 7)) (apply str (seq (js/Set. #js [\"a\" \"b\"])))))",
        "[ 3, 7, '{:m 2}', 6, 15, 435, 7, 'ab' ]",
    );
}

/// Drains a host iterator through `.next`, with the final `value`.
const DRAIN: &str = "(defn drain [it] (loop [acc []] (let [r (.next it)] (if (.-done r) (conj acc (.-value r)) (recur (conj acc (.-value r)))))))";

#[test]
fn vectors_answer_host_facing_methods_from_their_items() {
    // light-cljs-230: without protocol implementations a vector iterates,
    // counts, searches and compares its own items, as `seq` walks them,
    // including the tail its public constructor received and a subvec's
    // source.
    every_level(
        &format!(
            "{DRAIN}
             (let [pv (cljs.core/PersistentVector. nil 3 5 (js-obj) #js [1 2 3] nil)
                   odd (cljs.core/PersistentVector. nil 2 5 (js-obj) #js [7 8 9] nil)
                   none (cljs.core/PersistentVector. nil 0 5 (js-obj) #js [5] nil)
                   sv (subvec [1 2 3 4 5] 1 4) big (vec (range 40)) e (first {{:a 1}})
                   it (fn [v] (drain (.call (aget v js/Symbol.iterator) v)))]
               (pr-str [(it pv) (it odd) (it none) (it sv) (count (it big)) (it e) (it [])
                        (.indexOf pv 2) (.lastIndexOf odd 8) (.indexOf sv 4) (.lastIndexOf sv 2 -1)
                        (.indexOf e 1) (.indexOf big 33 -10) (.lastIndexOf [1 2 1] 1 js/undefined)
                        (.indexOf [1 2] 2 0.5) (.equiv pv [1 2 3]) (.equiv odd [7 8]) (.equiv odd odd)
                        (.equiv sv [2 3 4]) (.equiv e [:a 1]) (.equiv [] none) (= pv [1 2 3])
                        (= sv (subvec [0 2 3 4] 1)) (= [1 2] '(1 2)) (.-cnt pv) (.-cnt [1 2]) (.-cnt sv)]))"
        ),
        "[[1 2 3 nil] [7 8 9 nil] [nil] [2 3 4 nil] 41 [:a 1 nil] [nil] 1 1 2 0 1 33 2 1.5 true false true true true true true true true 3 2 nil]",
    );
}

#[test]
fn maps_answer_host_facing_methods_from_their_entries() {
    // Array, hash and sorted maps, a public constructor's array map and a
    // tree map, which takes the generic iteration.
    every_level(
        &format!(
            "{DRAIN}
             (let [it (fn [m] [(mapv vec (js/Array.from m)) (drain (.keys m)) (drain (.values m))
                               (mapv #(if (array? %) (vec %) %) (drain (.entries m)))])]
               (pr-str [(it {{:a 1 :b 2}}) (it (into {{}} (map vector (range 10) (range 10))))
                        (it (sorted-map 2 :b 1 :a)) (it (cljs.core/PersistentArrayMap. nil 2 #js [:x 1 :y 2] nil))
                        (it (cljs.core/PersistentTreeMap. compare nil 0 nil nil)) (it {{}})
                        (.equiv {{:a 1 :b 2}} {{:b 2 :a 1}}) (.-cnt {{:a 1}})
                        (.-cnt (cljs.core/PersistentArrayMap. nil 2 #js [:x 1 :y 2] nil))]))"
        ),
        "[[[[:a 1] [:b 2]] [:a :b nil] [1 2 nil] [[:a 1] [:b 2] nil]] [[[0 0] [1 1] [2 2] [3 3] [4 4] [5 5] [6 6] [7 7] [8 8] [9 9]] [0 1 2 3 4 5 6 7 8 9 nil] [0 1 2 3 4 5 6 7 8 9 nil] [[0 0] [1 1] [2 2] [3 3] [4 4] [5 5] [6 6] [7 7] [8 8] [9 9] nil]] [[[1 :a] [2 :b]] [1 2 nil] [:a :b nil] [[1 :a] [2 :b] nil]] [[[:x 1] [:y 2]] [:x :y nil] [1 2 nil] [[:x 1] [:y 2] nil]] [[] [nil] [nil] [nil]] [[] [nil] [nil] [nil]] true 1 2]",
    );
    // With protocol implementations the generic methods dispatch to them,
    // and they take receivers of another type.
    every_level(
        &format!(
            "{DRAIN}
             (extend-type PersistentVector ICounted (-count [_] 7))
             (extend-type PersistentArrayMap ISeqable (-seq [_] (list (cljs.core/MapEntry. :z 26 nil))))
             (let [v [1 2 3] m {{:a 1}}]
               (pr-str [(drain (.call (aget v js/Symbol.iterator) v)) (try (.lastIndexOf v 3) (catch :default e (.-message e)))
                        (.-cnt v) (.equiv v [1 2 3]) (mapv vec (js/Array.from m)) (drain (.keys m)) (.-cnt m)
                        (drain (.call (aget v js/Symbol.iterator) '(4 5))) (.call (.-indexOf v) '(4 5) 5)]))"
        ),
        "[[1 2 3 nil] \"Index out of bounds\" 7 true [[:z 26]] [:z nil] 1 [4 5 nil] 1]",
    );
}

#[test]
fn map_entries_keep_no_vector_type_where_no_vector_is_built() {
    // light-cljs-247: a map's host iteration yields map entries, vectors of a
    // key and a value. Where the program can build no vector, they have
    // their own members (`MapEntryType.lean`) and equality
    // (`mapEntry.equiv`), and each kind hook for vectors that the program's
    // operations read; they answer as at `None`. The program names `next`,
    // an upstream member, so it keeps the fields of the types it builds.
    let source = "(defn t [f] (try (f) (catch :default x (.-message x))))
         (defn drain [it] (let [a #js []] (loop [] (let [r (js/Object.values (.next it))] (.push a (aget r 0)) (if (aget r 1) a (recur))))))
         (let [m (if (js/Math.random) {:a 1 :b \"x\"} nil) es (js/Array.from m) e (aget es 0) f (aget (js/Array.from m) 0) g (aget es 1)]
           (.join (.map (array e (str e) (.toString g) (count e) (key e) (val g) (nth e 1) (get e 0) (get e 2 :nf) (e 1) (e 2 :nf) (.call e nil 0) (.apply e nil #js [1])
                               (.indexOf e 1) (.indexOf e :x) (.lastIndexOf e :a) (.lastIndexOf e 1 0) (.indexOf e 1 -1) (t #(.indexOf e :a 0.5)) (.lastIndexOf e :a -1) (t #(.lastIndexOf e :b 1.5))
                               (= e f) (= e g) (.equiv e f) (.equiv e g) (= (hash e) (hash f)) (vector? e) (map-entry? e) (seq e) (rest e)
                               (js/Array.from e) (drain (.entries m)) (drain (.keys m)) (drain (.values m))
                               (t #(.call e nil 5)) (t #(e)) (t #(nth e 5))
                               (identical? (type e) (type g)) (contains? e 1) (contains? e 2) (meta e) (find m :a) (= (find m :a) e))
                        #(pr-str %))
                  \" \"))";
    every_level(
        source,
        "[:a 1] \"[:a 1]\" \"[:b \\\"x\\\"]\" 2 :a \"x\" 1 :a :nf 1 :nf :a 1 1 -1 0 -1 1 \"Index out of bounds\" 0 -1 true false true false true true true (:a 1) (1) #js [:a 1] #js [#js [:a 1] #js [:b \"x\"] nil] #js [:a :b nil] #js [1 \"x\" nil] \"Index out of bounds\" \"Invalid arity: 0\" \"Index out of bounds\" true true false nil [:a 1] true",
    );
    let kept = units(source);
    for absent in [
        "makeVector",
        "PersistentVectorValue",
        "vectorPrototype",
        "MapEntryType.vector",
        "makeVector.equiv",
    ] {
        assert!(!kept.contains(&absent), "{absent}: {kept:?}");
    }
    for present in [
        "MapEntryType.lean",
        "mapEntry.equiv",
        "makeVector.seq",
        "makeVector.hash",
        "makeVector.print",
    ] {
        assert!(kept.contains(&present), "{present}: {kept:?}");
    }
    // Without a printer, the host's view of the entries the map's iteration
    // and `entries` give, and their members, as at `None`.
    let source = "(let [m (if (js/Math.random) {:k :a :n 2} nil) e (aget (js/Array.from m) 1) d (.next (.entries m))]
           (array (.indexOf e 2) (.lastIndexOf e :n) (.indexOf e :x) (.equiv e e) (e 1) (.call e nil 0 :nf)
                  (.-length (js/Array.from e)) (aget (js/Array.from e) 1) (.-length (.-value d)) (.-done d)
                  (.equiv (aget (js/Array.from m) 0) (aget (js/Array.from m) 0)) (.equiv e (aget (js/Array.from m) 0))))";
    let kept = units(source);
    assert!(
        !kept.contains(&"makeVector") && !kept.contains(&"printWriterImpl"),
        "{kept:?}"
    );
    every_level(
        &format!("(.join {source} \" \")"),
        "1 0 -1 true 2 :n 2 2 2 false true false",
    );
    // A map value alone keeps neither vectors nor sorted lookup.
    let kept = assert_keeps_none(
        "(js/console.log (if (js/Math.random) {:k :a} nil))",
        &[
            "makeVector",
            "vectorPrototype",
            "sortedPosition",
            "treeMapConstructor.entryIndex",
        ],
    );
    assert!(kept.contains(&"MapEntryType.lean"), "{kept:?}");
    // A program that builds a vector gives map entries the vectors' members.
    let kept = units("(js/console.log (pr-str (first {:a 1}) [1]))");
    assert!(kept.contains(&"MapEntryType.vector") && !kept.contains(&"MapEntryType.lean"));
    assert!(kept.contains(&"makeVector.equiv") && !kept.contains(&"mapEntry.equiv"));
}

#[test]
fn type_statics_named_in_raw_javascript_are_kept() {
    // Each read alone, without an empty literal that would keep the statics
    // anyway: one form that names `EMPTY` keeps the statics for every other.
    // light-cljs-247, from the Sol review: raw JavaScript that names a static
    // through string or identifier escapes keeps it. light-cljs-390, from the
    // Sol review: a line continuation (a backslash and a line terminator: LF,
    // CRLF, CR, U+2028 or U+2029) in a string or template contributes no
    // character, so `'EM\` and a newline and `PTY'` names `EMPTY`; and one
    // that ends a line comment does not join its last word to the name on
    // the next line.
    for read in [
        "~{}['\\\\x45MPTY'] === undefined",
        "~{}['\\\\u{45}MPTY'] === undefined",
        "~{}.\\\\u0045MPTY === undefined",
        "~{}['EM\\\\\nPTY'] === undefined",
        "~{}['EM\\\\\r\nPTY'] === undefined",
        "~{}['EM\\\\\rPTY'] === undefined",
        "~{}['EM\\\\\u{2028}PTY'] === undefined",
        "~{}[`EM\\\\\u{2029}PTY`] === undefined",
        "(({// prefix\\\\\nEMPTY: v}) => v === undefined)(~{})",
    ] {
        let source = format!("(js* \"{read}\" (type (if (js/Math.random) [1] nil)))");
        every_level(&source, "false");
        let kept = units(&format!("(js/console.log {source})"));
        for unit in ["typeStatics", "PersistentVectorValue.statics"] {
            assert!(kept.contains(&unit), "{read:?} {unit}: {kept:?}");
        }
    }
}

#[test]
fn a_map_value_keeps_no_public_constructor_statics_or_other_kinds_branches() {
    // light-cljs-247: builders instantiate the array map type, whose call is
    // `publicArrayMap` only where the program names `PersistentArrayMap`
    // (`typeHooks.arrayMap`); the types' statics wait for `typeStatics`,
    // which naming one or an empty literal keeps; and the branches of
    // `lookup` for sets, transients and sorted maps and of structural
    // equality for symbols are wrappers that their builders' effects install.
    let target = "(js/console.log (if (js/Math.random) {:k :a} nil))";
    assert_keeps_none(
        target,
        &[
            "publicArrayMap",
            "publicArrayMap.install",
            "constructorSlots",
            "typeStatics",
            "PersistentArrayMapValue.statics",
            "lookup.sets",
            "lookup.transients",
            "lookup.tree",
            "structuralEqualPair.symbols",
        ],
    );
    // 6,942 bytes before light-cljs-291 and 7,453 after: `privateStore` and
    // `kindedStore` for the maps' and transients' stores; 7,867 after
    // light-cljs-358: `get` behind its kinds' branches and the maps' branch;
    // 8,124 after light-cljs-373: the map entries' kind and `get` branch;
    // 8,137 to 8,449 after light-cljs-989 (2026-11-14): the array maps'
    // entry class and a keyword key's longer search; 8,779 after
    // light-cljs-990 (2026-11-14): the stores' kind brand (`payloads`);
    // 9,145 after light-cljs-1025 (2026-11-15): the runtime key's
    // registration and the key in each keyed name; 9,737 after
    // light-cljs-1011 (2026-11-15): keywords and symbols as classes
    // (`keywordOf`, `symbolOf`, the keyword cell, its kind and `toJSON`), a
    // keyword key's own search loops and each keyword constant's hash;
    // about 10,300 after light-cljs-1026 (2026-11-16): a map entry's store, metadata
    // and hash as its own keyed fields, and the class bases' `toJSON` leaving
    // out every keyed field (`unkeyedJSON`), so the ceiling moves to the next
    // 1,000; 11,455 after light-cljs-1028 (2026-11-16): a map entry's
    // prototype declares its protocols (masks and the `IFind` marker) and
    // holds its keyed `-lookup` slots, which `get`'s slot-first wrapper
    // (`get.slots`, `slotFirst`, `builtinSlots`) reads, so the ceiling moves
    // to 12,000; 12,976 after light-cljs-1012 (2026-11-16): the array map's
    // class and kind and its keyed `get`, `count` and `contains?` slots,
    // which its prototype's methods keep, with their slot-first wrappers;
    // 14,353 after light-cljs-1034 (2026-11-16): the array map's flat search
    // (`arrayIndex`), its pairs' readers and the kind hooks through which
    // `maps` still answers entries, so the ceiling moves to 15,000; 15,186
    // after light-cljs-1013 (2026-11-16): the tests of the sets, queues,
    // atoms and the other values that became classes (`setCellOf`, `sets`,
    // `queues`, `atoms` and the rest), which the stores' inventories read
    // where those were private stores, so the ceiling moves to 16,000;
    // 17,018 after light-cljs-1014 (2026-11-16): the array map's keyed
    // `-equiv` slot (`mapEquiv`, which looks each key up in the other map)
    // and the slot-first wrapper of `=` (`equalPair.slots`), which the map's
    // key search keeps, so the ceiling moves to 18,000.
    let output = advanced(target).emit_js();
    assert!(output.len() < 18_000, "{} bytes", output.len());
    // The public constructor, by name and through a quoted literal, which
    // keeps its positional array.
    let public = "(let [m (cljs.core/PersistentArrayMap. nil 2 #js [:a 1 :b 2] nil) q '{:x 1 \"y\" 2}]
           (pr-str [(get m :b) (count m) (.-cnt m) (vec (.-arr m)) m (= m {:a 1 :b 2}) (get q \"y\")
                    (instance? PersistentArrayMap {:a 1}) (identical? (type {:a 1}) PersistentArrayMap)
                    (instance? PersistentArrayMap q)]))";
    every_level(
        public,
        "[2 2 2 [:a 1 :b 2] {:a 1, :b 2} true 2 true true true]",
    );
    let kept = units(&format!("(js/console.log {public})"));
    assert!(kept.contains(&"publicArrayMap.install"), "{kept:?}");
    // Literal maps of keys that may lower to one value (a character and a string) keep their
    // positional array.
    let kept = units("(js/console.log (count (if (js/Math.random) {\\a 1 \"a\" 2} nil)))");
    assert!(kept.contains(&"publicArrayMap"), "{kept:?}");
    every_level(
        "(let [m (if (js/Math.random) {\\a 1 \"a\" 2} nil)] (pr-str [(count m) (get m \"a\") m]))",
        "[2 1 {\"a\" 1, \"a\" 2}]",
    );
    // Statics, named or read by empty literals.
    let statics = "(pr-str [{} [] #{} () (.-EMPTY (type {:a 1})) (.-HASHMAP_THRESHOLD (type {:a 1}))
                    (count (.-EMPTY (type [1]))) (cljs.core/PersistentHashMap.fromArray #js [:a 1] true)])";
    every_level(statics, "[{} [] #{} () {} 8 0 {:a 1}]");
    let kept = units("(js/console.log {} [])");
    for unit in [
        "typeStatics",
        "PersistentArrayMapValue.statics",
        "PersistentVectorValue.statics",
    ] {
        assert!(kept.contains(&unit), "{unit}: {kept:?}");
    }
    // Each static other than `EMPTY` waits for its own name (light-cljs-1077).
    let kept = units("(js/console.log (.-HASHMAP_THRESHOLD (type {:a 1})))");
    assert!(
        kept.contains(&"PersistentArrayMapValue.threshold")
            && !kept.contains(&"PersistentArrayMapValue.statics")
            && !kept.contains(&"PersistentHashMapValue.fromArrays"),
        "{kept:?}"
    );
    let empty = units("(js/console.log {} [])");
    assert!(
        !empty.contains(&"PersistentArrayMapValue.threshold")
            && !empty.contains(&"PersistentHashMapValue.fromArrays"),
        "{empty:?}"
    );
    // The other kinds' branches where their builders are kept.
    let branches = "(pr-str [(get #{:a} :a) (get (sorted-map :b 2 :a 1) :a) (get (transient {:a 1}) :a)
                    (get (sorted-set 3 1) 3) (get (transient #{:x}) :x) (:a (hash-map :a 5))
                    (get (hash-set 1 2) 2) (get (sorted-map :a 1) :z :nf) (contains? #{:a} :b)
                    (= 'a 'a) (= (symbol \"a\") 'a) (= 'a/b 'a) (= 'a :a) (.equiv 'x 'x) (= ['a] ['a])])";
    every_level(
        branches,
        "[:a 1 1 3 :x 5 2 :nf false true true false false true true]",
    );
    let kept = units(&format!("(js/console.log {branches})"));
    for unit in [
        "lookup.sets",
        "lookup.transients",
        "lookup.tree",
        // A symbol's comparison, its class's `-equiv` slot since light-cljs-1074.
        "SymbolValue.equivSlot",
    ] {
        assert!(kept.contains(&unit), "{unit}: {kept:?}");
    }
}

#[test]
fn lean_map_entries_answer_host_methods_through_protocol_extensions() {
    // light-cljs-247, from the Sol review: with `extend-type`, the members of
    // map entries where no vector can be built (`entryPrototype`) answer
    // through the generic operations (`hostHooks`), as `vectorPrototype`'s
    // do, so an extension of `MapEntry` is seen by host iteration and index
    // search as at `None`.
    let source = "(extend-type MapEntry
           ICounted (-count [_] 3)
           IIndexed (-nth ([_ i] (* 10 i)) ([_ i nf] (* 10 i))))
         (let [e (aget (js/Array.from (if (js/Math.random) {:a 1} nil)) 0)]
           (.join (array (.indexOf e 20) (.lastIndexOf e 0) (.lastIndexOf e 10 1) (count e) (nth e 2)
                         (.join (js/Array.from e) \",\") (.-length (js/Array.from e)))
                  \" \"))";
    let kept = units(source);
    assert!(
        kept.contains(&"MapEntryType.lean") && !kept.contains(&"makeVector"),
        "{kept:?}"
    );
    every_level(source, "2 0 1 3 20 :a,1 2");
}

#[test]
fn entry_only_and_transient_only_slices_keep_and_call_their_slots() {
    // light-cljs-1028: a program that builds map entries and no vector keeps
    // the keyed slots of map entries for the operations it keeps, each an
    // effect of `MapEntryType`, with those operations' slot-first wrappers,
    // and no vector's; they answer as the operations always have.
    let entries = "(let [e (first (if (js/Math.random) {:a 1} nil))]
           (.join (array (count e) (nth e 0) (nth e 1) (nth e 2 :nf) (get e 0) (get e 7 :nf) (peek e) (first (seq e))
                         (try (nth e 2) (catch :default _ :out)))
                  \" \"))";
    let kept = units(entries);
    for unit in [
        "MapEntryType.lean",
        "MapEntryType.type",
        "MapEntryType.count",
        "MapEntryType.nth",
        "MapEntryType.lookup",
        "MapEntryType.peek",
        "MapEntryType.seq",
        "count",
        "nth",
        "get",
        "peek",
        "seq",
    ] {
        assert!(kept.contains(&unit), "{unit}: {kept:?}");
    }
    for unit in [
        "makeVector",
        "PersistentVectorValue.count",
        "SubvecValue.count",
        "TransientVectorType.count",
        "MapEntryType.conjSlot",
        "MapEntryType.assocSlot",
    ] {
        assert!(!kept.contains(&unit), "{unit}: {kept:?}");
    }
    every_level(entries, "2 :a 1 :nf :a :nf 1 :a :out");
    // A program using transient vectors through `count`, `nth` and `get`
    // keeps their slots, and answers as before, also once the transient is
    // persistent (the slots throw as the operations did).
    let transients = "(let [t (conj! (transient []) 1 2 3) answers (array (count t) (nth t 0) (nth t 5 :nf) (get t 1) (get t :k :nf))]
           (persistent! t)
           (.join (.concat answers (array (try (count t) (catch :default _ :persisted)) (nth t 5 :nf)
                                          (try (get t 0) (catch :default _ :persisted))))
                  \" \"))";
    let kept = units(transients);
    for unit in [
        "TransientVectorType.kind",
        "TransientVectorType.count",
        "TransientVectorType.nth",
        "TransientVectorType.lookup",
        "count",
        "nth",
        "get",
    ] {
        assert!(kept.contains(&unit), "{unit}: {kept:?}");
    }
    assert!(!kept.contains(&"MapEntryType.count"), "{kept:?}");
    every_level(transients, "3 1 :nf 2 :nf :persisted :nf :persisted");
}

#[test]
fn keywords_and_symbols_from_public_constructors_compare_their_spelling() {
    // From the Sol review of light-cljs-247: `.equiv` of a keyword or a
    // symbol compares the spelling its public constructor received, NaN
    // too, without the identity shortcut of `=`.
    every_level(
        "(let [k (Keyword. nil \"k\" js/NaN nil) s (Symbol. nil \"s\" js/NaN nil nil)]
           (pr-str [(.equiv k k) (.equiv s s) (= k k) (= s s)]))",
        "[false false true true]",
    );
    // The Sol review of light-cljs-1011: an undefined spelling or hash is
    // kept as given; the value is still a keyword or a symbol, and an
    // undefined hash is computed on first use.
    every_level(
        "(let [k (Keyword. nil \"k\" js/undefined js/undefined) s (Symbol. nil \"s\" js/undefined js/undefined nil)
               h (.-_hash k) j (.-_hash s)]
           (pr-str [(keyword? k) (symbol? s) (name k) (name s) (undefined? (.-fqn k)) (undefined? (.-str s))
                    (undefined? h) (undefined? j) (= (hash k) (hash :k)) (= (hash s) (hash 's))
                    (= k (Keyword. nil \"j\" js/undefined nil)) (= k :k)]))",
        "[true true \"k\" \"s\" true true true true true true true false]",
    );
}

/// light-cljs-1038 (its Sol review): a sorted set counts its tree map with
/// `count`, as upstream's `(count tree-map)`, so a protocol implementation
/// of the map's, `specify!`'s or `extend-type`'s, answers for a set the
/// public constructor built of it; the set's own operations still read the
/// map's tree.
#[test]
fn public_tree_sets_honor_backing_map_count_overrides() {
    every_level(
        r#"(let [m (specify! (sorted-map 1 nil 3 nil) ICounted (-count [_] 7))
                 s (cljs.core/PersistentTreeSet. nil m nil)]
             (pr-str [(count s) (contains? s 3) (get s 2 :none) (seq s) (conj s 2) (count (sorted-set 1 2))]))"#,
        "[7 true :none (1 3) #{1 2 3} 2]",
    );
    every_level(
        r#"(extend-type PersistentTreeMap ICounted (-count [_] 9))
           (let [s (cljs.core/PersistentTreeSet. nil (sorted-map 1 nil) nil)]
             (pr-str [(count s) (count (sorted-set 1 2)) (disj s 1)]))"#,
        "[9 9 #{}]",
    );
}

#[test]
fn sets_answer_host_facing_methods_from_their_members() {
    // light-cljs-231: without protocol implementations a set iterates
    // (`keys`, `values`, `entries`) and searches (`has`) its own members in
    // the order `seq` walks them, compares its count without `count`, and
    // builds its `hash-map` field as a map of its kind: array, hash and
    // sorted sets, sets the public constructors built from maps of every
    // kind and from other values, and quoted literals.
    every_level(
        &[
            DRAIN,
            r#"
             (let [it (fn [s] [(vec (js/Array.from s)) (drain (.keys s)) (drain (.values s))
                               (mapv #(if (array? %) (vec %) %) (drain (.entries s)))
                               (.has s 1) (.has s 7)])
                   big (into #{} (range 10))
                   pub (cljs.core/PersistentHashSet. nil {1 nil :a nil} nil)
                   pubh (cljs.core/PersistentHashSet. nil (into {} (map vector (range 10) (range 10))) nil)
                   tree (cljs.core/PersistentTreeSet. nil (sorted-map 3 nil 1 nil) nil)
                   vsrc (cljs.core/PersistentHashSet. nil [1 2 3] nil)
                   sorted (cljs.core/PersistentHashSet. nil (sorted-map 1 nil 2 nil) nil)
                   none (cljs.core/PersistentHashSet. nil nil nil)]
               (pr-str [(it #{1 :a}) (it big) (it (sorted-set 3 1 2)) (it pub) (it pubh) (it tree) (it #{})
                        (it #{nil 1}) (it (disj big 3)) (it (conj #{1} 2)) (it '#{1 "a"})
                        (.equiv #{1 2} #{2 1}) (.equiv #{1 2} #{1}) (= #{1 2} pub) (= pub #{1 :a})
                        (= big (set (range 10))) (= vsrc #{}) (= #{} vsrc) (= tree #{1 3}) (= #{1 3} tree)
                        (count vsrc) (.-hash-map #{1 :a}) (.-hash-map big) (.-hash-map pub)
                        (.-hash-map (persistent! (transient pub))) (.call #{1 2} nil 2) (.call #{1 2} nil 3 :none)
                        (contains? sorted 1) (contains? sorted 3) (count sorted) (conj sorted 5) (disj sorted 1)
                        (contains? none 1) (conj none 1) (persistent! (conj! (transient none) 3))
                        (contains? (persistent! (conj! (transient none) 3)) 3)]))"#,
        ]
        .concat(),
        "[[[1 :a] [1 :a nil] [1 :a nil] [[1 1] [:a :a] nil] true false] \
         [[0 1 2 3 4 5 6 7 8 9] [0 1 2 3 4 5 6 7 8 9 nil] [0 1 2 3 4 5 6 7 8 9 nil] \
         [[0 0] [1 1] [2 2] [3 3] [4 4] [5 5] [6 6] [7 7] [8 8] [9 9] nil] true true] \
         [[1 2 3] [1 2 3 nil] [1 2 3 nil] [[1 1] [2 2] [3 3] nil] true false] \
         [[1 :a] [1 :a nil] [1 :a nil] [[1 1] [:a :a] nil] true false] \
         [[0 1 2 3 4 5 6 7 8 9] [0 1 2 3 4 5 6 7 8 9 nil] [0 1 2 3 4 5 6 7 8 9 nil] \
         [[0 0] [1 1] [2 2] [3 3] [4 4] [5 5] [6 6] [7 7] [8 8] [9 9] nil] true true] \
         [[1 3] [1 3 nil] [1 3 nil] [[1 1] [3 3] nil] true false] [[] [nil] [nil] [nil] false false] \
         [[nil 1] [nil 1 nil] [nil 1 nil] [[nil nil] [1 1] nil] true false] \
         [[0 1 2 4 5 6 7 8 9] [0 1 2 4 5 6 7 8 9 nil] [0 1 2 4 5 6 7 8 9 nil] \
         [[0 0] [1 1] [2 2] [4 4] [5 5] [6 6] [7 7] [8 8] [9 9] nil] true true] \
         [[1 2] [1 2 nil] [1 2 nil] [[1 1] [2 2] nil] true false] \
         [[1 \"a\"] [1 \"a\" nil] [1 \"a\" nil] [[1 1] [\"a\" \"a\"] nil] true false] \
         true false false true true true false true true 3 {1 nil, :a nil} \
         {0 nil, 1 nil, 2 nil, 3 nil, 4 nil, 5 nil, 6 nil, 7 nil, 8 nil, 9 nil} {1 nil, :a nil} \
         {1 nil, :a nil} 2 :none true false 2 #{1 2 5} #{2} false #{1} #{3} true]",
    );
    // With protocol implementations the generic methods dispatch to them,
    // and they take receivers of another type.
    every_level(
        &[
            DRAIN,
            r#"
             (extend-type PersistentHashSet ICounted (-count [_] 7))
             (extend-type PersistentTreeSet ISeqable (-seq [_] (list 9 8)))
             (let [s #{1 2} t (sorted-set 1 2)]
               (pr-str [(drain (.call (aget s js/Symbol.iterator) s)) (drain (.keys t)) (vec (js/Array.from t))
                        (mapv vec (drain (.entries s))) (.has s 1) (.has t 9) (= s (hash-set 1 2))
                        (= (hash-set 2 1) s) (= s (conj #{1} 2)) (count s) (drain (.call (.-keys s) '(4 5)))
                        (.call (.-has s) {:a 1} :a) (mapv vec (drain (.call (.-entries s) [6 7])))]))"#,
        ]
        .concat(),
        "[[1 2 nil] [9 8 nil] [9 8] [[1 1] [2 2] []] true false false false false 7 [4 5 nil] true \
         [[6 6] [7 7] []]]",
    );
}

/// A set literal's `has` and iteration through the host.
const SET_SOURCE: &str = "(js/console.log (.has #{1 2} 2) (.join (js/Array.from #{1 2}) \" \"))";

#[test]
fn a_set_keeps_no_seq_hash_map_or_hashing() {
    // light-cljs-231: it iterates and searches its members itself, compares
    // them without `count`, builds an array map for its `hash-map` field, and
    // its literal is an array set, which no promotion makes a hash set, so it
    // keeps neither seqs, nor a hash map, nor hashing.
    let kept = assert_keeps_none(
        SET_SOURCE,
        &[
            "seq",
            "first",
            "next",
            "count",
            "nth",
            "es6Iterator",
            "es6EntriesIterator",
            "makeSet.seq",
            "IndexedSeqValue",
            "mapFromArray",
            "PersistentHashMapValue",
            "hash",
            "hashNodeAssoc",
            "ensureHashRoot",
            "PersistentHashMapValue.traversal",
            "PersistentHashMapValue.entryIndex",
            "PersistentHashMapValue.put",
            "putMember",
            "setFromArray",
            "publicHashSet",
        ],
    );
    for unit in PROTOCOL_MACHINERY {
        assert!(!kept.contains(&unit), "{unit}: {kept:?}");
    }
    for unit in [
        "arraySetFromArray",
        "setIterator",
        "makeSet.equiv",
        "setCount",
    ] {
        assert!(kept.contains(&unit), "{unit}: {kept:?}");
    }
    every_level(SET_SOURCE, "true 1 2\nundefined");
    // A program naming the type keeps its public constructor, and a larger
    // literal keeps promotion and hashing.
    for source in [
        "(js/console.log (cljs.core/PersistentHashSet. nil {1 nil} nil))",
        "(js/console.log (instance? PersistentHashSet #{1}))",
        "(js/console.log #{1 2 3 4 5 6 7 8 9})",
    ] {
        let units = units(source);
        assert!(units.contains(&"hash"), "{source}: {units:?}");
    }
    assert!(!units("(js/console.log #{} (.-EMPTY (type #{})))").contains(&"publicHashSet"));
}

#[test]
fn array_maps_keep_no_hash_map_trie() {
    // light-cljs-235: a hash map is a trie whose brand and generic-operation
    // branches its type's effects install, so a program that only builds
    // array maps, through `array-map` too, keeps none of them.
    let source =
        "(js/console.log (.get (apply array-map [:a 1 :b 2]) :a) (count (array-map :a 1)))";
    assert_keeps_none(
        source,
        &[
            "mapFromArray",
            "PersistentHashMapValue",
            "HashMapCell",
            "hashTrie",
            "trieEntry",
            "get.trie",
            "copyEntries.trie",
            "hashNodeAssoc",
        ],
    );
    every_level(source, "1 1\nundefined");
    let units = units("(js/console.log (count (hash-map :a 1)))");
    for unit in ["hashTrie", "HashMapCell"] {
        assert!(units.contains(&unit), "{unit}: {units:?}");
    }
}

#[test]
fn promoted_transient_maps_look_up_without_a_persistent_hash_map() {
    // light-cljs-235, from review: `assoc!` promotes a transient array map
    // past 8 keys to a trie in a program that builds no persistent hash map,
    // whose lookup still finds the trie's hook.
    every_level(
        "(js/console.log (get (assoc! (transient (array-map)) 0 0 1 1 2 2 3 3 4 4 5 5 6 6 7 7 8 8) 8))",
        "8\nundefined",
    );
}

#[test]
fn applied_array_maps_accept_large_argument_sequences() {
    // light-cljs-235, from review: `apply array-map` hands its pairs on as
    // one array, not as spread arguments, whose number an engine limits.
    every_level(
        "(js/console.log (get (apply array-map (take 200000 (cycle [:a 1]))) :a))",
        "1\nundefined",
    );
}

#[test]
fn membership_in_value_position_keeps_no_set() {
    // light-cljs-231: `(contains? s x)`, `(s x)` and `(get s x)` on a set
    // literal of at most 8 keywords, strings or numbers compare the value
    // with its members wherever the result goes, so no set is built.
    let source = r#"
      (def ^:const modes #{:fast :safe})
      (defn kw [k] [(contains? #{:fast :safe :a/b} k) (#{:fast :safe :a/b} k) (get #{:fast :safe :a/b} k)
                    (contains? modes k) (modes k) (let [s #{:x}] (s k)) (contains? #{} k) (get #{:only} k)
                    (if (contains? #{:fast :safe} k) :in :out) (if (#{:fast} k) 1 2)
                    (and (contains? #{:fast :safe} k) :both)])
      (defn st [s] [(contains? #{"fast" "safe"} s) (#{"fast" "safe"} s) (get #{"fast" "safe"} s)
                    (if (contains? #{"fast" "safe"} s) :in :out) (contains? #{"x"} s)])
      (defn nm [n] [(contains? #{1 2 -1 1.5} n) (#{1 2 -1 1.5} n) (get #{0} n) (contains? #{##NaN} n)
                    (if (#{1 2} n) :in :out) (contains? #{1 "1"} n) (#{1 "1"} n)])
      (def calls (atom 0))
      (defn tick [x] (swap! calls inc) x)
      (set! (.-kw js/globalThis) kw)
      (set! (.-st js/globalThis) st)
      (set! (.-nm js/globalThis) nm)
      (pr-str [(map js/globalThis.kw [:fast :safe :slow :a/b :b "fast" 'fast nil (keyword "fast")
                                      (keyword "a" "b") {:fast 1} [:fast] :x :only])
               (map js/globalThis.st ["fast" "safe" "slow" :fast nil 1 "x"])
               (map js/globalThis.nm [1 2 -1 1.5 3 0 -0 ##NaN "1" nil :a])
               (contains? #{:a :b} (tick :a)) (#{1 2} (tick 2)) (get #{"a" "b"} (tick "c")) @calls
               (let [m (fn [x] (#{:a :b :c} (tick x)))] [(m :b) (m :d)]) @calls])"#;
    every_level(
        source,
        "[([true :fast :fast true :fast nil false nil :in 1 :both] \
         [true :safe :safe true :safe nil false nil :in 2 :both] [false nil nil false nil nil false nil :out 2 false] \
         [true :a/b :a/b false nil nil false nil :out 2 false] [false nil nil false nil nil false nil :out 2 false] \
         [false nil nil false nil nil false nil :out 2 false] [false nil nil false nil nil false nil :out 2 false] \
         [false nil nil false nil nil false nil :out 2 false] [true :fast :fast true :fast nil false nil :in 1 :both] \
         [true :a/b :a/b false nil nil false nil :out 2 false] [false nil nil false nil nil false nil :out 2 false] \
         [false nil nil false nil nil false nil :out 2 false] [false nil nil false nil :x false nil :out 2 false] \
         [false nil nil false nil nil false :only :out 2 false]) \
         ([true \"fast\" \"fast\" :in false] [true \"safe\" \"safe\" :in false] [false nil nil :out false] \
         [false nil nil :out false] [false nil nil :out false] [false nil nil :out false] [false nil nil :out true]) \
         ([true 1 nil false :in true 1] [true 2 nil false :in false nil] [true -1 nil false :out false nil] \
         [true 1.5 nil false :out false nil] [false nil nil false :out false nil] [false nil 0 false :out false nil] \
         [false nil 0 false :out false nil] [false nil nil false :out false nil] [false nil nil false :out true \"1\"] \
         [false nil nil false :out false nil] [false nil nil false :out false nil]) true 2 nil 3 [:b nil] 5]",
    );
    let units = assert_keeps_none(
        "(set! (.-f js/globalThis) (fn [k] [(contains? #{:fast :safe} k) (#{\"a\" \"b\"} k) (get #{1 2} k)]))",
        &[
            "makeSet",
            "PersistentHashSetValue",
            "contains",
            "get",
            "lookup",
        ],
    );
    assert!(units.contains(&"keywordFqn"), "{units:?}");
}

#[test]
fn small_programs_keep_only_what_they_use() {
    // A vector, a map literal, `str` of scalars, an atom without options and
    // the printer without options keep neither seqs nor hash maps nor the
    // protocol machinery (light-cljs-230).
    let absent = |source: &str, extra: &[&str]| {
        let units = assert_keeps_none(source, extra);
        for unit in PROTOCOL_MACHINERY {
            assert!(!units.contains(&unit), "{source} keeps `{unit}`");
        }
        units
    };
    absent(
        "(js/console.log {:a 1})",
        &["seq", "PersistentHashMapValue", "hash", "mapFromArray"],
    );
    absent(
        "(js/console.log (str (js/Date.now) :a))",
        &["print", "printWriterImpl", "makeVector"],
    );
    absent("(js/console.log (atom 1))", &["seqToMap", "into", "seq"]);
    absent(
        "(js/console.log (pr-str 1))",
        &["assoc", "PersistentHashMapValue", "printOptions.writer"],
    );
    absent(
        "(js/console.log (into [] (range 3)) (reduce + [1 2]) (seq? (list 1)) (coll? [1]))",
        &[
            "PersistentHashMapValue",
            "makeMap.persist",
            "makeSet.persist",
        ],
    );
}

/// The prototype members of the runtime's types other than their upstream
/// fields and methods: host-facing methods (and protocol slots, `cljs$...`).
const HOST_FACING_MEMBERS: [&str; 8] = [
    "apply",
    "call",
    "constructor",
    "equiv",
    "indexOf",
    "lastIndexOf",
    // A chunk's and a chunk buffer's JSON, which leaves out their keyed
    // fields (light-cljs-1029; the seqs' is on their shared base).
    "toJSON",
    "toString",
];

#[test]
fn upstream_members_are_every_other_member_of_the_runtime_types() {
    // At `None`, every prototype member of a runtime type with a `.fields`
    // effect is host-facing or one of `UPSTREAM_MEMBERS`, and each of those
    // is a member of one (light-cljs-238).
    let source = "(let [names (js/Set.)]
       (doseq [v [(cons 1 nil) (list 1 2) () :k 'sym (repeat 2 1) (cycle [1 2]) (iterate inc 0)
                  [1] (subvec [1 2 3] 1) (seq (vec (range 40))) (conj cljs.core/PersistentQueue.EMPTY 1)
                  {} (apply hash-map (range 20)) #{1} (seq [1 2]) (range 0.5 3) (range 3)
                  (sorted-map 1 2) (sorted-set 1) (seq (sorted-map 1 2)) (first (sorted-map 1 2)) (eduction (map inc) [1])
                  (delay 1) (lazy-seq nil) (seq (map inc (range 3))) (chunk-buffer 2)
                  (array-chunk #js [1]) (reduced 1) (seq (apply hash-map (range 20)))]
               n (js/Object.getOwnPropertyNames (js/Object.getPrototypeOf v))]
         (.add names n))
       (.join (.sort (js/Array.from names)) \" \"))";
    let js = prepare_application(
        &program(source),
        ApplicationOptions::new(OptimizationLevel::None),
    )
    .emit_js();
    let printed = execute(&format!("console.log({js});"));
    // Names with a `$` are the runtime's own: protocol slots (`cljs$...`)
    // and the getters of a cell's computed fields (`first$<key>`,
    // light-cljs-1030).
    let upstream: Vec<&str> = printed
        .split_whitespace()
        .filter(|name| !name.contains('$') && !HOST_FACING_MEMBERS.contains(name))
        .collect();
    assert_eq!(upstream, UPSTREAM_MEMBERS);
}

#[test]
fn upstream_fields_need_a_program_that_names_them() {
    // A program that names none keeps neither the fields nor `publicFields`,
    // but keeps the host-facing members of the seqs it builds.
    let unnamed = assert_keeps_none(
        "(js/console.log (count (map inc (range 3))))",
        &[
            "publicFields",
            "LazySeq.fields",
            "LazySeq.metaField",
            "ChunkedCons.fields",
            "IntegerRangeValue.fields",
        ],
    );
    assert!(unnamed.contains(&"LazySeq.members") && unnamed.contains(&"IntegerRangeValue.members"));
    // A program naming one as a property, method or string, or in `js*`
    // text, reads the fields as `None` does: the fields of the types that
    // define a member it names, each waiting for that member's flag, and
    // not their `meta` unless it names `meta` too (light-cljs-1077).
    for (source, expected, fields, flag) in [
        (
            "(let [c (cons 1 (list 2 3))] (array (.-first c) (count (.-rest c)) (.-__hash c)))",
            "[ 1, 2, null ]",
            "Cons.fields",
            "member_first",
        ),
        (
            "(let [s (lazy-seq [1 2])] (array (count (.sval s)) (count (.-s s))))",
            "[ 2, 2 ]",
            "LazySeq.fields",
            "member_sval",
        ),
        (
            "(aget (range 3) \"cnt\")",
            "3",
            "IntegerRangeValue.fields",
            "member_cnt",
        ),
        (
            "(js* \"~{}.first\" (cons 5 nil))",
            "5",
            "Cons.fields",
            "member_first",
        ),
    ] {
        every_level(source, expected);
        let units = units(source);
        assert!(
            units.contains(&flag)
                && units.contains(&fields)
                && !units.contains(&fields.replace(".fields", ".metaField").as_str()),
            "{source} keeps {units:?}"
        );
    }
    // `alter-meta!` reads and assigns `meta`, which is all it keeps.
    for (source, expected, meta) in [
        (
            "(let [l (with-meta (list 1 2) {:a 1})] (pr-str (alter-meta! l assoc :b 2)))",
            "{:a 1, :b 2}",
            "ListValue.metaField",
        ),
        (
            "(let [l (with-meta (lazy-seq [1]) {:a 1})] (pr-str (alter-meta! l assoc :b 2) (meta l)))",
            "{:a 1, :b 2} {:a 1, :b 2}",
            "LazySeq.metaField",
        ),
    ] {
        every_level(source, expected);
        let units = units(source);
        assert!(
            units.contains(&meta)
                && !units.contains(&meta.replace(".metaField", ".fields").as_str())
                && !units.iter().any(|unit| unit.starts_with("member_")),
            "{source} keeps {units:?}"
        );
    }
}

#[test]
fn a_program_naming_one_upstream_member_reads_it_on_every_runtime_type() {
    // Each member waits for its own flag (light-cljs-1077): a program that
    // names only that member finds it on every runtime value that has it at
    // `None`, which keeps every member.
    let values = "[(cons 1 nil) (list 1 2) () :k 'sym (repeat 2 1) (cycle [1 2]) (iterate inc 0)
                  [1] (subvec [1 2 3] 1) (seq (vec (range 40))) (conj cljs.core/PersistentQueue.EMPTY 1)
                  {} (apply hash-map (range 20)) #{1} (seq [1 2]) (range 0.5 3) (range 3)
                  (sorted-map 1 2) (sorted-set 1) (seq (sorted-map 1 2)) (first (sorted-map 1 2)) (eduction (map inc) [1])
                  (delay 1) (lazy-seq nil) (seq (map inc (range 3))) (chunk-buffer 2)
                  (array-chunk #js [1]) (reduced 1) (seq (apply hash-map (range 20)))]";
    let has = |name: &str| {
        format!("(apply str (map (fn [v] (if (js* \"~{{}} in ~{{}}\" {name:?} v) 1 0)) {values}))")
    };
    let all = format!(
        "(apply str (interpose \" \" [{}]))",
        UPSTREAM_MEMBERS
            .iter()
            .map(|&name| has(name))
            .collect::<Vec<_>>()
            .join(" ")
    );
    let js = prepare_application(
        &program(&all),
        ApplicationOptions::new(OptimizationLevel::None),
    )
    .emit_js();
    let each: Vec<String> = UPSTREAM_MEMBERS
        .iter()
        .map(|&name| {
            prepare_application(
                &program(&has(name)),
                ApplicationOptions::new(OptimizationLevel::Advanced),
            )
            .emit_js()
        })
        .collect();
    // Run both the `None` oracle and the combined `Advanced` outputs from
    // files: each is near or past macOS's argv/stack limit for `node --eval`.
    let dir = crate::temp_dir::TempDir::new("upstream-members");
    let oracle = dir.path().join("all.js");
    std::fs::write(&oracle, format!("console.log({js});")).unwrap();
    let expected = execute(&format!("require({:?});", oracle.display().to_string()));
    let script = dir.path().join("members.js");
    std::fs::write(
        &script,
        format!("console.log([{}].join(' '));", each.join(",\n")),
    )
    .unwrap();
    let printed = execute(&format!("require({:?});", script.display().to_string()));
    assert_eq!(printed, expected);
    // Every member is on some value here.
    assert!(
        !expected
            .split_whitespace()
            .any(|bits| bits.chars().all(|c| c == '0')),
        "{expected}"
    );
}

#[test]
fn named_generated_record_slots_remain_callable() {
    // A record's core protocol methods come from the runtime, each installed
    // where an operation dispatching it is kept (`recordMethods.lookup` where
    // `get` is). A program with records alone that names a slot as a string,
    // property or method, or in `js*` text, calls it as `None` does, so its
    // slice keeps the method (light-cljs-250).
    let record = "(defrecord R [x]) (def r (->R 1))";
    for (body, expected, method) in [
        // The pellet's failure scenario, whose `.call` passes the receiver
        // as `this` only: the method reads "missing" as its receiver argument
        // and looks up "nf" without a default.
        (
            "(array (.equiv [1] [1]) (.call (.-cljs$core$ILookup$_lookup$arity$3 r) r \"missing\" \"nf\"))",
            "[ true, undefined ]",
            "recordMethods.lookup",
        ),
        (
            "(.call (.-cljs$core$ILookup$_lookup$arity$3 r) r r \"missing\" \"nf\")",
            "nf",
            "recordMethods.lookup",
        ),
        (
            "(.cljs$core$ILookup$_lookup$arity$2 r r \"x\")",
            "null",
            "recordMethods.lookup",
        ),
        (
            "(js* \"~{}.cljs$core$ICounted$_count$arity$1$LKEY(~{})\" r r)",
            "1",
            "recordMethods.count",
        ),
        (
            "(js* \"typeof ~{}\" (.-cljs$core$IPrintWithWriter$_pr_writer$arity$3 r))",
            "function",
            "recordMethods.print",
        ),
        (
            "(.cljs$core$IHash$_hash$arity$1 r r)",
            "2087646226",
            "recordMethods.hash",
        ),
        (
            "(.cljs$core$IEquiv$_equiv$arity$2 r r (->R 1))",
            "true",
            "recordMethods.equiv",
        ),
        (
            "(.cljs$core$IMeta$_meta$arity$1 r r)",
            "null",
            "recordMethods.meta",
        ),
        (
            "(.-x (.cljs$core$IWithMeta$_with_meta$arity$2 r r nil))",
            "1",
            "recordMethods.withMeta",
        ),
        (
            "(.-x (.cljs$core$ICloneable$_clone$arity$1 r r))",
            "1",
            "recordMethods.clone",
        ),
        (
            "(.-x (.cljs$core$ICollection$_conj$arity$2 r r [:x 7]))",
            "7",
            "recordMethods.conj",
        ),
        (
            "(.cljs$core$IAssociative$_contains_key_QMARK_$arity$2 r r :x)",
            "true",
            "recordMethods.contains",
        ),
        (
            "(.-x (.cljs$core$IAssociative$_assoc$arity$3 r r :x 5))",
            "5",
            "recordMethods.assoc",
        ),
        (
            "(.-x (.cljs$core$IMap$_dissoc$arity$2 r r :y))",
            "1",
            "recordMethods.dissoc",
        ),
        (
            "(nil? (.cljs$core$ISeqable$_seq$arity$1 r r))",
            "false",
            "recordMethods.seq",
        ),
        (
            "(.hasNext (.cljs$core$IIterable$_iterator$arity$1 r r))",
            "true",
            "recordMethods.iterator",
        ),
        (
            "(.cljs$core$IKVReduce$_kv_reduce$arity$3 r r (fn [a _ v] (+ a v)) 10)",
            "11",
            "recordMethods.kvReduce",
        ),
    ] {
        let source = format!("{record} {body}");
        every_level(&source, expected);
        let units = units(&source);
        assert!(units.contains(&method), "{source} keeps {units:?}");
    }
    // One `js*` text naming two slots keeps both methods.
    let source = format!(
        "{record} {}",
        "(js* \"[~{}.cljs$core$IMeta$_meta$arity$1$LKEY(~{}), ~{}.cljs$core$ICounted$_count$arity$1$LKEY(~{})]\" r r r r)"
    );
    every_level(&source, "[ null, 1 ]");
    let kept = units(&source);
    assert!(
        kept.contains(&"recordMethods.meta") && kept.contains(&"recordMethods.count"),
        "{source} keeps {kept:?}"
    );
    // A program that names no slot keeps none of them.
    let units = units(&format!("{record} (.-x r)"));
    assert!(
        !units
            .iter()
            .any(|unit| unit.starts_with("recordMethods.") && *unit != "recordMethods.install"),
        "{units:?}"
    );
}

#[test]
fn printing_with_options_dispatchers_and_into() {
    every_level(
        "(let [alt (fn [x w opts] (if (number? x) (-write w (str \"#\" x)) ((:fallback-impl opts) x w opts)))
               parts #js [] w (StringBufferWriter. (js-obj \"append\" (fn [t] (.push parts t) nil)))]
           (pr-sequential-writer w (fn [x w o] (-write w (pr-str x))) \"<\" \" \" \">\" {:print-length 2 :more-marker \"~\"} [1 2 3])
           (-flush w)
           (pr-str [(str 1 :a \"b\" nil [1 2] {:a 1} #{2} '(3) (range 2))
                    (pr-str-with-opts [[1 {:a 2}] \"x\"] {:alt-impl alt})
                    (prn-str-with-opts [[1]] {:print-length 1})
                    (print-meta? {:meta true} (with-meta [1] {:m 1}))
                    (pr-str-with-opts [(with-meta [1] {:m 1})] {:meta true})
                    (.join parts \"\") (satisfies? IWriter w) (implements? IWriter w)
                    (pr-str (atom 1 :meta {:a 1})) (meta (atom 1 :meta {:a 1})) (meta (atom 1 {:meta {:b 2}}))
                    (pr-str (into [] (range 3)) (into {} [[:a 1]]) (into #{} [1]) (into () [1 2])
                            (into (sorted-map) {:b 1 :a 2}) (into (with-meta [] {:m 1}) [1]))
                    (reduce + [1 2 3]) (reduce + (js/Set. #js [1 2])) (seq? '(1)) (coll? [1])
                    (counted? nil) (empty? []) (-conj [1] 2) (-count nil) (-lookup {:a 1} :a)
                    (try (-count 1) (catch :default e (.-message e)))]))",
        "[\"1:ab[1 2]{:a 1}#{2}(3)(0 1)\" \"[#1 {:a #2}] x\" \"[1]\\n\" true \"^{:m 1} [1]\" \"<1 2 ~>\" true true \"#object[cljs.core.Atom {:val 1}]\" {:a 1} {:b 2} \"[0 1 2] {:a 1} #{1} (2 1) {:a 2, :b 1} [1]\" 6 3 true true true true [1 2] 0 1 \"No protocol method ICounted/-count defined for this value\"]",
    );
}

#[test]
fn metadata_through_the_generic_operations() {
    // The public `meta` field's map is printed with `pr-str`: Node's
    // `console.log` of a map lists its keyed fields since light-cljs-1012
    // (docs/value-layout.md, question 3), whose names hold the build's key.
    every_level(
        "(defn ^{:test (fn [] :ok)} f [] 1)
         (array (pr-str (meta (with-meta [1] {:a 1}))) (some? (:test (meta #'f))) (pr-str (.-meta (with-meta [1] {:a 2})))
                (map? (.-meta (with-meta [1] {:a 2}))) (pr-str (meta (conj (with-meta [1] {:b 1}) 2))) (nil? (.-meta [1])))",
        "[ '{:a 1}', true, '{:a 2}', true, '{:b 1}', true ]",
    );
}

/// Metadata and cached hashes are the values' own fields (light-cljs-1053),
/// given on demand: a set's `conj` gives no metadata it lacks, a copy keeps
/// a public constructor's hash, an undefined one too, and hashing a public
/// range caches its hash where its `__hash` field reads it, as the stores
/// did. From review.
#[test]
fn metadata_and_hash_fields_answer_as_the_stores_did() {
    every_level(
        "(let [m (PersistentArrayMap. nil 1 (array :a 1) js/undefined) r (IntegerRange. nil 0 3 1 3 nil) h (hash r)]
           (array (nil? (meta (conj #{1} 2))) (undefined? (meta (conj #{1} 2)))
                  (undefined? (.-__hash (with-meta m {:m 1}))) (undefined? (.-__hash (clone m)))
                  (= h (.-__hash r)) (pr-str (meta (conj (with-meta #{1} {:s 1}) 2)))))",
        "[ true, false, true, true, true, '{:s 1}' ]",
    );
}

/// `seq`, `first`, `rest`, `next` and `count` read the branches of a lazy
/// seq's or a cell's kind from the kind tables (light-cljs-237), and every
/// seq kind, including the values the branches leave to the operations
/// (empty lists, views, queues, public constructors' values), answers as
/// before. A program building lazy seqs or cells keeps the wrappers and
/// branches of the operations it keeps; one building neither, even where it
/// builds views, keeps none.
#[test]
fn seq_operations_read_the_kind_tables() {
    every_level(
        "(pr-str
          (mapv (fn [x] [(nil? (seq x)) (take 3 (seq x)) (first x) (take 3 (rest x)) (nil? (next x)) (count x)])
                [(list 1 2) (cons 0 (list 1)) () (list 1) (map inc [1 2]) (lazy-seq nil) (concat [1] [2])
                 (seq [1 2 3]) (seq (vec (range 40))) (nthrest (seq (vec (range 40))) 35) (rest [1 2])
                 (rest (seq [1])) (seq [nil false]) (seq \"ab\") (seq (array 1 2)) (rseq [1 2 3])
                 (range 3) (range 0 2 0.5) (repeat 2 :x) (take 2 (iterate inc 0)) (keys {:a 1 :b 2})
                 (vals {:a 1}) (seq {:a 1}) (seq #{1}) (seq (sorted-map :a 1 :b 2)) (subseq (sorted-set 1 2 3) > 1)
                 (into cljs.core/PersistentQueue.EMPTY [1 2]) (seq (into cljs.core/PersistentQueue.EMPTY [1 2]))
                 (re-seq #\"a\" \"aba\") (with-meta (list 1 2) {:m 1}) (with-meta (seq [1 2]) {:m 1})
                 (map inc (range 40)) (filter odd? (range 70)) (rest (map inc (range 40))) (lazy-seq (list nil))
                 (cons nil (lazy-seq nil)) (range 5 0 -2) (drop 33 (range 40))]))",
        "[[false (1 2) 1 (2) false 2] [false (0 1) 0 (1) false 2] [true () nil () true 0] \
         [false (1) 1 () true 1] [false (2 3) 2 (3) false 2] [true () nil () true 0] \
         [false (1 2) 1 (2) false 2] [false (1 2 3) 1 (2 3) false 3] [false (0 1 2) 0 (1 2 3) false 40] \
         [false (35 36 37) 35 (36 37 38) false 5] [false (2) 2 () true 1] [true () nil () true 0] \
         [false (nil false) nil (false) false 2] [false (\"a\" \"b\") \"a\" (\"b\") false 2] \
         [false (1 2) 1 (2) false 2] [false (3 2 1) 3 (2 1) false 3] [false (0 1 2) 0 (1 2) false 3] \
         [false (0 0.5 1) 0 (0.5 1 1.5) false 4] [false (:x :x) :x (:x) false 2] [false (0 1) 0 (1) false 2] \
         [false (:a :b) :a (:b) false 2] [false (1) 1 () true 1] [false ([:a 1]) [:a 1] () true 1] \
         [false (1) 1 () true 1] [false ([:a 1] [:b 2]) [:a 1] ([:b 2]) false 2] [false (2 3) 2 (3) false 2] \
         [false (1 2) 1 (2) false 2] [false (1 2) 1 (2) false 2] [false (\"a\" \"a\") \"a\" (\"a\") false 2] \
         [false (1 2) 1 (2) false 2] [false (1 2) 1 (2) false 2] \
         [false (1 2 3) 1 (2 3 4) false 40] [false (1 3 5) 1 (3 5 7) false 35] [false (2 3 4) 2 (3 4 5) false 39] \
         [false (nil) nil () true 1] [false (nil) nil () true 1] [false (5 3 1) 5 (3 1) false 3] \
         [false (33 34 35) 33 (34 35 36) false 7]]",
    );
    // No kind tables are left (light-cljs-1015): the slots and the
    // operations answer every kind.
    let kept = units("(js/console.log (first (map inc (list 1 2))))");
    assert!(
        !kept.iter().any(|unit| unit.ends_with(".branches")
            || unit.ends_with(".cells")
            || unit.ends_with(".views")
            || unit.ends_with(".viewKind")
            || *unit == "kindTables"),
        "{kept:?}"
    );
    let kept = units("(js/console.log (count [1 2]) (get {:a 1} :a) (first [3]))");
    assert!(
        !kept.iter().any(|unit| unit.ends_with(".branches")
            || unit.ends_with(".cells")
            || unit.ends_with(".views")
            || unit.ends_with(".viewKind")
            || *unit == "kindTables"),
        "{kept:?}"
    );
}

/// `first` of a view (an IndexedSeq or a ChunkedSeq) gives at every level
/// what the operation's path gives (a kind table branch read it from
/// light-cljs-237 until light-cljs-1015, and its slot since light-cljs-1010): of a public constructor's
/// IndexedSeq over a caller's array, which sees later writes, of strings,
/// subvectors, a vector's chunks and their ends, and of the views without a
/// kind (map, set and reversed seqs). Expected values are master's at `none`.
#[test]
fn views_read_their_first_from_the_kind_table() {
    every_level(
        "(let [a (array 1 2 3) s (IndexedSeq. a 1 nil) v (vec (range 70)) big (seq v)]
           (aset a 2 9)
           (pr-str [(first s) (first (prim-seq (array 4 5) 1)) (first (seq (subvec [1 2 3 4] 1))) (first (seq \"xy\"))
                    (first (rest (seq \"xy\"))) (first (nthrest big 69)) (first (nthrest big 32)) (first (rest (seq [nil 2])))
                    (first (seq [nil 2])) (first (IndexedSeq. (array 1 2) 2 nil)) (first (rseq [1 2])) (first (seq {:a 1}))
                    (first (seq #{7})) (first (with-meta (seq [3 4]) {:m 1})) (first (chunk-rest big)) (first (map inc [1]))
                    (first (seq (into-array [nil]))) (count s) (count (nthrest big 40))
                    (first (cljs.core/ChunkedSeq. v (.-tail v) 64 3 nil nil))]))",
        "[2 5 2 \"x\" \"y\" 69 32 2 nil nil 2 [:a 1] 7 3 32 2 nil 2 30 67]",
    );
}

/// The release runtime keeps none of the old dispatch layers
/// (light-cljs-1015, docs/value-layout.md, section 4): no payload class,
/// kind table, protocol wrapper or `publicMethod`, even where a program
/// defines a type, extends a runtime type and specifies a host object, which
/// keep the protocol lookup (`extendedMethod`); the operations answer as
/// before at every level. Since light-cljs-1046 the lookup and the
/// `satisfies` hooks test a runtime class's value by one store, its class's
/// (`ownedKind`, `ownedProtocolValue`; the lookup's through `runtimeKind`
/// until light-cljs-1075), so no payload store probe
/// (`payloadStore`, `kindStore`) is left either, with records and the operations of sets, queues and maps that have no
/// slot (`meta`, `empty`, `find`, `reduce`, printing).
#[test]
fn release_runtime_keeps_no_old_dispatch_layers() {
    let source = "(deftype T [a] ICounted (-count [_] a))
         (defrecord R [a])
         (extend-type PersistentVector IMeta (-meta [_] {:x 1}))
         (pr-str [(count (T. 3)) (meta [1]) (count (specify! #js {} ICounted (-count [_] 7))) (first (map inc [1 2]))
                  (seq {:a 1}) (rest #{1}) (next (list 1 2)) (get (sorted-map 1 2) 1) (seq (sorted-set 2 1)) (count #{1 2})
                  (:a (->R 4)) (count (->R 4)) (count (into cljs.core/PersistentQueue.EMPTY [1 2])) (seq (conj #{} 5))
                  (get #{:k} :k) (conj (with-meta #{1} {:m 1}) 2) (contains? [1 2] 1) (contains? (subvec [1 2 3] 1) 2)
                  (reduce + #{1 2}) (reduce + (into cljs.core/PersistentQueue.EMPTY [3 4])) (meta (with-meta {:a 1} {:m 2}))
                  (empty #{1}) (find {:a 1} :a) (satisfies? ICounted #{}) (implements? ISeq (cons 1 nil)) (counted? (list 1))
                  (seq? (cons 1 nil)) (count (cons 1 (cons 2 nil)))])";
    every_level(
        source,
        "[3 {:x 1} 7 2 ([:a 1]) () (2) 2 (1 2) 2 4 1 2 (5) :k #{1 2} true false 3 7 {:m 2} #{} [:a 1] true true true true 2]",
    );
    let kept = units(&format!("(js/console.log {source})"));
    for unit in [
        "payloads",
        "kindedStore",
        "behindProtocol",
        "kindTables",
        "kindOps",
        "publicMethod",
        "payloadStore",
        "payloadStores",
        "kindStore",
    ] {
        assert!(!kept.contains(&unit), "{unit}: {kept:?}");
    }
    for unit in ["extendedMethod", "ownedKind", "recordCells"] {
        assert!(kept.contains(&unit), "{unit}: {kept:?}");
    }
}

/// `extend-type` of a runtime type with branches or of `Object`, and
/// `specify!` of a runtime value with branches, empty the kind tables: the
/// operations then take their own paths, which find those implementations
/// where they did before light-cljs-237 (here where `rest` and `next` of a
/// list call `seq`; `first` of a value that is already a seq does not take
/// its seq, as upstream's does not). `extend-type default` and of a program
/// type leave the tables in use. Expected values are master's at `none`.
#[test]
fn extensions_reaching_the_kind_tables_empty_them() {
    for (source, expected) in [
        (
            "(extend-type List ISeqable (-seq [_] (list 7 8)))
             (let [v (list 1 2) c (cons 0 v)]
               (pr-str [(first v) (rest v) (next v) (count v) (first c) (count c) (count (map inc [1 2]))]))",
            "[1 (7 8) (7 8) 2 0 3 2]",
        ),
        (
            "(let [v (specify! (list 1 2) ISeqable (-seq [_] (list 9)))]
               (pr-str [(first v) (rest v) (count v) (first (list 3))]))",
            "[1 (2) 2 3]",
        ),
        (
            "(extend-type js/Object ISeqable (-seq [_] (list 5)))
             (str (first (list 1 2)) (first (rest (list 1 2 3))))",
            "12",
        ),
        (
            "(extend-type default ICounted (-count [_] 42))
             (pr-str [(count (cons 1 (list 2))) (count (list 1 2)) (count (map inc [1])) (count (seq [1 2]))])",
            "[2 2 1 2]",
        ),
        (
            "(extend-type LazySeq ICounted (-count [_] 99))
             (deftype T [] ISeqable (-seq [_] (list 4)))
             (pr-str [(count (map inc [1 2])) (count (cons 1 (map inc [1]))) (first (T.)) (count (T.)) (count (list 1))])",
            "[99 2 4 1 1]",
        ),
        (
            "(extend-type LazySeq ISeq (-first [_] :f) (-rest [_] ()))
             (let [s (map inc [1 2])] (pr-str [(first s) (rest s) (next (cons 0 s)) (first (list 1))]))",
            "[:f () (2 3) 1]",
        ),
    ] {
        every_level(source, expected);
    }
}

#[test]
fn seq_walk_fallbacks_keep_the_answers_of_specified_and_null_prototype_seqs() {
    // light-cljs-1054 (the Sol review): `first` and `rest` of a collection
    // read its seq by the seq's fields where `specify!` gave that seq its
    // own `ISeq`, as before; and a host seq with no prototype, which a
    // program's `-seq` gives, reaches the `object` table's methods.
    every_level(
        "(let [s (specify! (list 1 2) ISeq (-first [_] :custom) (-rest [_] ()))
               v (specify! (vector 0) ISeqable (-seq [_] s))]
           (pr-str [(first v) (rest v) (first s) (rest s)]))",
        "[1 (2) :custom ()]",
    );
    every_level(
        "(extend-type object ISeq (-first [_] :obj-first) (-rest [_] (list :obj-rest)))
         (deftype T [] ISeqable (-seq [_] (js/Object.create nil)))
         (pr-str [(first (T.)) (rest (T.))])",
        "[:obj-first (:obj-rest)]",
    );
}

/// The generic operations' protocol lookup on a value of the runtime's own
/// kinds skips the full protocol-membership test (light-cljs-988), so these
/// check that every implementation the test found still wins: `extend-type`
/// and `extend-protocol` of core sequence protocols to runtime types, the
/// internal type names (light-cljs-982 and 983), `specify!`, a method
/// JavaScript writes on a runtime type's prototype, which empties no kind
/// table, and the other core protocols the lookup serves (`IMeta`,
/// `IReduce`, `ICounted`, `ILookup`). `extend-type object` and `default`
/// still give the runtime's collections nothing. Expected values are
/// master's at `none` before the change.
#[test]
fn sequence_protocol_extensions_to_runtime_types_still_win() {
    // Each program prints its results through a JavaScript array, as the
    // extensions change how the runtime's collections print.
    for (source, expected) in [
        (
            "(extend-type PersistentVector ISeqable (-seq [_] (list :v)))
             (extend-protocol ISeq
               cljs.core/RSeq (-first [_] :rseq-first) (-rest [_] (list :rseq-rest))
               cljs.core/PersistentArrayMapSeq (-first [_] :amap-first) (-rest [_] ()))
             (extend-protocol INext cljs.core/RSeq (-next [_] (list :rseq-next)))
             (extend-type PersistentHashSet ISeqable (-seq [_] (list :set)))
             (let [out (array) p (fn [x] (.push out (pr-str x)))
                   r (rseq (subvec [1 2 3] 0)) a (seq {:a 1 :b 2})]
               (p (seq [1 2])) (p (first [1 2])) (p (rest [1 2])) (p (next [1 2])) (p (first (vec (range 40))))
               (p (first r)) (p (rest r)) (p (next r)) (p (first a)) (p (rest a)) (p (next a))
               (p (seq #{1})) (p (first #{1})) (p (next #{1})) (p (seq {:k 1})) (p (first (list 4 5)))
               (p (next (map inc (list 1 2))))
               (.join out \" \"))",
            "(:v) :v () nil :v :rseq-first (:rseq-rest) (:rseq-next) :amap-first () (:amap-first) (:set) :set nil (:amap-first) 4 (3)",
        ),
        (
            "(extend-type ArrayNodeSeq ISeq (-first [_] :node-first) (-rest [_] ()))
             (extend-type IndexedSeq ISeq (-first [_] :indexed-first))
             (extend-type MapEntry ISeqable (-seq [_] (list :entry)))
             (extend-type PersistentTreeMapSeq ISeq (-first [_] :tree-first) (-rest [_] (list :tree-rest)))
             (extend-protocol INext PersistentTreeMapSeq (-next [_] (list :tree-next)))
             (let [out (array) p (fn [x] (.push out (pr-str x)))
                   big (seq (into {} (map (fn [x] [x x]) (range 20)))) e (first {:a 1})
                   t (seq (sorted-map :a 1 :b 2))]
               (p (first big)) (p (rest big)) (p (first (seq (array 1 2)))) (p (seq e))
               (p (first e)) (p (rest e)) (p (next e)) (p (first [7]))
               (p (first t)) (p (rest t)) (p (next t)) (p (first (sorted-map :c 3))) (p (seq (sorted-set 1)))
               (.join out \" \"))",
            ":node-first () :indexed-first (:entry) :entry () nil 7 :tree-first (:tree-rest) (:tree-next) [:entry] (:indexed-first)",
        ),
        (
            // A sorted map seq's stack takes its list's protocols, as
            // upstream's `peek` and `next` of it (light-cljs-1035).
            "(let [out (array) p (fn [x] (.push out (pr-str x)))
                   m (sorted-map 1 :a 2 :b 3 :c) n (first m)
                   s (PersistentTreeMapSeq. nil (specify! (list n) IStack (-peek [_] :peeked)) true -1 nil)
                   s2 (PersistentTreeMapSeq. nil (specify! (list n n) INext (-next [_] nil)) true -1 nil)]
               (p (first s)) (p (next s2)) (p (vec m)) (p (vec (rseq m)))
               (.join out \" \"))",
            ":peeked nil [[1 :a] [2 :b] [3 :c]] [[3 :c] [2 :b] [1 :a]]",
        ),
        (
            "(extend-type object ISeqable (-seq [_] (list :object)))
             (extend-type default ISeq (-first [_] :default-first) (-rest [_] ()))
             (let [out (array) p (fn [x] (.push out (pr-str x)))
                   m (specify! {:a 1} ISeqable (-seq [_] (list :specified)))]
               (p (seq [1])) (p (first [1 2])) (p (rest #{3})) (p (next {:a 1})) (p (first (list 2)))
               (p (first (map inc [1]))) (p (seq m)) (p (first m)) (p (rest m)) (p (next m)) (p (seq {:b 2}))
               (p (first (js-obj)))
               (.join out \" \"))",
            "(1) 1 () nil 2 2 (:specified) :specified () nil ([:b 2]) :object",
        ),
        (
            "(deftype T [] ISeqable (-seq [_] (list :t)))
             (set! (.. PersistentQueue -prototype -cljs$core$ISeqable$) cljs.core/PROTOCOL_SENTINEL)
             (set! (.. PersistentQueue -prototype -cljs$core$ISeqable$_seq$arity$1) (fn [_] (list :queue)))
             (let [out (array) p (fn [x] (.push out (pr-str x)))
                   q (into cljs.core/PersistentQueue.EMPTY [1 2])]
               (p (seq q)) (p (first q)) (p (rest q)) (p (next q)) (p (first (T.))) (p (first [3]))
               (p (seq (conj q 3)))
               (.join out \" \"))",
            "(:queue) :queue () nil :t 3 (:queue)",
        ),
        (
            "(extend-type PersistentVector IMeta (-meta [_] {:extended true}))
             (extend-type List IReduce (-reduce ([_ f] :reduce-2) ([_ f init] [:reduce-3 init])))
             (extend-type PersistentTreeMap ICounted (-count [_] 99))
             (set! (.. PersistentHashMap -prototype -cljs$core$ILookup$) cljs.core/PROTOCOL_SENTINEL)
             (set! (.. PersistentHashMap -prototype -cljs$core$ILookup$_lookup$arity$2) (fn [_ k] [:hash-lookup k]))
             (let [out (array) p (fn [x] (.push out (pr-str x)))
                   hm (zipmap (range 20) (range 20))]
               (p (meta [1])) (p (meta (with-meta [1] {:m 1}))) (p (meta {:a 1})) (p (reduce + (list 1 2)))
               (p (reduce + 0 (list 1 2))) (p (reduce + (map inc [1 2]))) (p (count (sorted-map :a 1)))
               (p (count {:a 1})) (p (get hm 3)) (p (get {:a 1} :a)) (p (first (list 5)))
               (.join out \" \"))",
            "{:extended true} {:extended true} nil :reduce-2 [:reduce-3 0] 5 99 1 [:hash-lookup 3] 1 5",
        ),
        (
            // A method slot without the protocol's marker, written after the
            // lookup has answered for the type's values, and replaced.
            "(deftype T [])
             (extend-type T ICounted (-count [_] 1))
             (let [out (array) p (fn [x] (.push out (pr-str x)))
                   s #{1} big (into #{} (range 40))]
               (p (seq s)) (p (first s)) (p (count (T.)))
               (set! (.. PersistentHashSet -prototype -cljs$core$ISeqable$_seq$arity$1) (fn [_] (list :unmarked)))
               (p (seq s)) (p (first s)) (p (seq big)) (p (rest s))
               (set! (.. PersistentHashSet -prototype -cljs$core$ISeqable$_seq$arity$1) (fn [_] (list :replaced)))
               (p (seq s)) (p (next s)) (p (seq [1]))
               (.join out \" \"))",
            "(1) 1 1 (:unmarked) :unmarked (:unmarked) () (:replaced) nil (1)",
        ),
        (
            // A host value whose Proxy throws on any property but a string
            // one still reaches `extend-type object`: the lookup reads no
            // other property of it.
            "(extend-type object ISeqable (-seq [_] (list :ok)))
             (let [handler (js-obj \"get\" (fn [t k] (if (string? k) (aget t k) (throw (js/Error. \"not a string key\")))))
                   proxy (js/Proxy. (js-obj) handler)]
               (pr-str (seq proxy)))",
            "(:ok)",
        ),
    ] {
        every_level(source, expected);
    }
}

/// `reduce` and `reduce-kv` of a vector, a subvector, a map entry and the
/// maps call their `-reduce` and `-kv-reduce` slots before any protocol
/// lookup (light-cljs-1055), and keep every answer they had: a `-seq`, an
/// `IReduce` or an `IKVReduce` that `specify!` or `extend-type` gave the
/// value or its class, a public constructor's vector (whose seq walks the
/// whole tail it received) and array map (walked by its count), a reduction
/// `reduced` stops, and the errors. Each case runs in a thunk of its own, as
/// some throw. Expected values are master `d8a28b21`'s at every level. The
/// last two cases do the same for the `-reduce` slots of the hash and array
/// sets and the indexed seqs (light-cljs-1059), and the seqs beside them
/// that keep `reduce`'s path: a set's `-seq` and `-iterator`, a seq's `-seq`
/// and `IReduce`, public constructors' indexed seq, list and set, empty ones
/// and endless ones `reduced` stops; their expected values are master
/// `6a18e208`'s. Each `specify!` takes a value built at run time, as a
/// literal may be one constant that two of them would share.
#[test]
fn reduce_slots_keep_the_answers_of_extended_seqs_and_reductions() {
    for (source, expected) in [
        (
            r#"(let [out (array) p (fn [f] (.push out (try (pr-str (f)) (catch :default e (str "E:" (.-message e))))))
                   big (vec (range 40)) hm (zipmap (range 20) (range 20))
                   sv (specify! (vector 1 2 3) ISeqable (-seq [_] (list :sv)))
                   sbig (specify! (vec (range 40)) ISeqable (-seq [_] (list :sbig)))
                   rv (specify! (vector 1 2 3) IReduce (-reduce ([_ f] [:r2]) ([_ f i] [:r3 i])))
                   kvv (specify! (vector 1 2 3) IKVReduce (-kv-reduce [_ f i] [:kv i]))
                   sm (specify! (array-map :a 1) ISeqable (-seq [_] (list :sm)))
                   shm (specify! (zipmap (range 20) (range 20)) ISeqable (-seq [_] (list :shm)))
                   rm (specify! (array-map :a 1) IReduce (-reduce ([_ f] [:mr2]) ([_ f i] [:mr3 i])))
                   kvm (specify! (array-map :a 1) IKVReduce (-kv-reduce [_ f i] [:mkv i]))
                   ssub (specify! (subvec big 2 5) ISeqable (-seq [_] (list :ssub)))
                   sent (specify! (first (array-map :a 1)) ISeqable (-seq [_] (list :sent)))
                   pv (PersistentVector. nil 2 5 (.-EMPTY_NODE PersistentVector) (array 1 2 3 4) nil)
                   pam (PersistentArrayMap. nil 1 (array :a 1 :b 2) nil)
                   kvs (fn [a k v] (conj a k v))]
               (p #(reduce conj [] sv)) (p #(reduce conj sv)) (p #(reduce conj [] sbig)) (p #(reduce conj sbig)) (p #(reduce-kv kvs [] sv))
               (p #(reduce conj [] rv)) (p #(reduce conj rv)) (p #(reduce-kv conj [] kvv))
               (p #(reduce conj [] sm)) (p #(reduce conj sm)) (p #(reduce-kv kvs [] sm)) (p #(reduce conj [] shm))
               (p #(reduce conj [] rm)) (p #(reduce conj rm)) (p #(reduce-kv conj [] kvm)) (p #(reduce conj [] ssub)) (p #(reduce conj [] sent))
               (p #(reduce conj [] pv)) (p #(reduce + pv)) (p #(reduce conj [] (conj pv 9))) (p #(reduce-kv kvs [] pv))
               (p #(reduce conj [] pam)) (p #(reduce-kv kvs [] pam))
               (p #(reduce + big)) (p #(reduce + 1 (subvec big 30))) (p #(reduce conj [] (first {:a 1}))) (p #(reduce conj [] {:a 1 :b 2}))
               (p #(reduce + (map val hm))) (p #(reduce-kv (fn [a k v] (+ a k v)) 0 hm)) (p #(reduce-kv kvs [] (sorted-map 2 :b 1 :a)))
               (p #(reduce (fn [a x] (if (> x 5) (reduced [:stop a]) (+ a x))) 0 big))
               (p #(reduce-kv (fn [a k v] (if (= k 1) (reduced [:r a]) (+ a v))) 0 (sorted-map 0 5 1 6 2 7)))
               (p #(reduce (fn ([] :none) ([a b] a)) {}))
               (.join out " "))"#,
            "[:sv] :sv [:sbig] :sbig [0 1 1 2 2 3] [:r3 []] [:r2] [:kv []] [:sm] :sm [:a 1] [:shm] [:mr3 []] [:mr2] [:mkv []] [:ssub] [:sent] [1 2 3 4] 3 [1 2 3 4 9] [0 1 1 2] [[:a 1]] [:a 1 :b 2] 780 346 [:a 1] [[:a 1] [:b 2]] 190 380 [1 :a 2 :b] [:stop 15] [:r 5] :none",
        ),
        (
            // Printing a vector walks its seq, which the extension replaces:
            // the results print through `nth`.
            r#"(extend-type PersistentVector ISeqable (-seq [v] (list :v (count v))))
               (extend-type PersistentArrayMap ISeqable (-seq [m] (list :am (count m))))
               (extend-type PersistentHashMap ISeqable (-seq [m] (list :hm (count m))))
               (extend-type Subvec ISeqable (-seq [m] (list :sub (count m))))
               (let [out (array)
                     show (fn show [x] (if (vector? x) (str "[" (.join (let [a (array)] (dotimes [i (count x)] (.push a (show (nth x i)))) a) " ") "]") (pr-str x)))
                     p (fn [f] (.push out (try (show (f)) (catch :default e (str "E:" (.-message e))))))
                     big (vec (range 40)) hm (zipmap (range 20) (range 20)) kvs (fn [a k v] (conj a k v))]
                 (p #(reduce conj [] [1 2 3])) (p #(reduce conj [] big)) (p #(reduce conj [] (subvec big 2 5))) (p #(reduce conj [] {:a 1}))
                 (p #(reduce conj [] hm)) (p #(reduce-kv kvs [] [5 6])) (p #(reduce-kv kvs [] {:a 1})) (p #(reduce-kv (fn [a k v] (+ a v)) 0 hm))
                 (p #(reduce conj [] (first {:a 1}))) (p #(mapv identity [1 2]))
                 (.join out " "))"#,
            "[:v 3] [:v 40] [:sub 3] [:am 1] [:hm 20] [0 5 1 6] [:a 1] 190 E::am is not ISeqable [:v 2]",
        ),
        (
            r#"(extend-type PersistentVector IReduce (-reduce ([v f] [:vr2 (count v)]) ([v f i] [:vr3 i])) IKVReduce (-kv-reduce [v f i] [:vkv i]))
               (extend-type PersistentArrayMap IReduce (-reduce ([v f] [:amr2]) ([v f i] [:amr3 i])) IKVReduce (-kv-reduce [v f i] [:amkv i]))
               (extend-type PersistentHashMap IKVReduce (-kv-reduce [v f i] [:hmkv i]))
               (extend-type PersistentTreeMap IKVReduce (-kv-reduce [v f i] [:tmkv i]))
               (extend-type Subvec IReduce (-reduce ([v f] [:sr2]) ([v f i] [:sr3 i])))
               (extend-type MapEntry IReduce (-reduce [v f i] [:er3 i]))
               (let [out (array) p (fn [f] (.push out (try (pr-str (f)) (catch :default e (str "E:" (.-message e))))))
                     big (vec (range 40)) hm (zipmap (range 20) (range 20)) kvs (fn [a k v] (conj a k v))]
                 (p #(reduce conj [] [1 2 3])) (p #(reduce conj [1 2 3])) (p #(reduce conj [] big)) (p #(reduce conj [] (subvec big 2 5)))
                 (p #(reduce conj [] {:a 1})) (p #(count (reduce conj [] hm))) (p #(reduce-kv kvs [] [5 6])) (p #(reduce-kv kvs [] (subvec big 2 4)))
                 (p #(reduce-kv kvs [] {:a 1})) (p #(reduce-kv (fn [a k v] (+ a v)) 0 hm)) (p #(reduce-kv (fn [a k v] (+ a v)) 0 (sorted-map 1 2)))
                 (p #(reduce conj [] (first {:a 1}))) (p #(reduce conj (first {:a 1})))
                 (.join out " "))"#,
            "[:vr3 []] [:vr2 3] [:vr3 []] [:sr3 []] [:amr3 []] 20 [:vkv []] [0 2 1 3] [:amkv []] [:hmkv 0] [:tmkv 0] [:er3 []] E:conj not supported on this type",
        ),
        (
            // The sets' and indexed seqs' `-reduce` slots (light-cljs-1059),
            // beside the seqs that keep `reduce`'s path.
            r#"(let [out (array) p (fn [f] (.push out (try (pr-str (f)) (catch :default e (str "E:" (.-message e))))))
                   hs (set (range 20))
                   ss (specify! (hash-set 1 2 3) ISeqable (-seq [_] (list :ss)))
                   si (specify! (set [4 5 6]) IIterable (-iterator [_] (-iterator [:i1 :i2])))
                   sr (specify! (hash-set 7 8) IReduce (-reduce ([_ f] [:sr2]) ([_ f i] [:sr3 i])))
                   sl (specify! (list 1 2) ISeqable (-seq [_] (list :sl)))
                   rl (specify! (list 3 4) IReduce (-reduce ([_ f] [:lr2]) ([_ f i] [:lr3 i])))
                   slz (specify! (map inc [1 2]) ISeqable (-seq [_] (list :slz)))
                   sc (specify! (cons 0 (list 1)) ISeqable (-seq [_] (list :sc)))
                   sir (specify! (range 3) ISeqable (-seq [_] (list :sir)))
                   sis (specify! (seq [1 2 3]) ISeqable (-seq [_] (list :sis)))
                   scs (specify! (rest (vec (range 40))) ISeqable (-seq [_] (list :scs)))
                   pis (IndexedSeq. (array 1 2 3) 1 nil) pl (List. nil 1 nil 0 nil)
                   phs (PersistentHashSet. nil (array-map :a nil :b nil) nil)
                   upto (fn [a x] (if (> a 10) (reduced [:stop a]) (+ a x)))]
               (p #(reduce conj [] hs)) (p #(reduce + hs)) (p #(reduce conj [] ss)) (p #(reduce conj ss)) (p #(reduce conj [] si)) (p #(reduce conj si))
               (p #(reduce conj [] sr)) (p #(reduce conj sr)) (p #(reduce conj [] sl)) (p #(reduce conj sl)) (p #(reduce conj [] rl)) (p #(reduce conj rl))
               (p #(reduce conj [] slz)) (p #(reduce conj [] sc)) (p #(reduce conj [] sir)) (p #(reduce + sir)) (p #(reduce conj [] sis)) (p #(reduce conj [] scs))
               (p #(reduce conj [] pis)) (p #(reduce conj [] pl)) (p #(reduce conj pl)) (p #(reduce conj [] phs)) (p #(reduce conj #{1}))
               (p #(reduce conj [] #{})) (p #(reduce (fn ([] :none) ([a b] a)) #{})) (p #(reduce (fn ([] :none) ([a b] a)) (list)))
               (p #(reduce (fn ([] :none) ([a b] a)) (lazy-seq nil))) (p #(reduce (fn [a x] (if (> (count a) 1) (reduced [:stop a]) (conj a x))) [] hs))
               (p #(reduce upto 0 (iterate inc 0))) (p #(reduce upto (cycle [1 2]))) (p #(reduce upto (repeat 4))) (p #(reduce conj [] (repeat 3 :x)))
               (p #(reduce conj [] (re-seq #"a." "abacad"))) (p #(reduce conj (map inc (range 5)))) (p #(reduce + 1 (range 2 20 3)))
               (p #(reduce conj [] ((fn [& xs] xs) 1 2 3))) (p #(reduce conj [] (seq (map inc (vec (range 40)))))) (p #(reduce + (rest (vec (range 40)))))
               (.join out " "))"#,
            r#"[0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19] 190 [:ss] :ss [:i1 :i2] E:conj not supported on this type [:sr3 []] [:sr2] [:sl] :sl [:lr3 []] [:lr2] [:slz] [:sc] [0 1 2] 3 [:sis] [1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 32 33 34 35 36 37 38 39] [2 3] [1] 1 [:a :b] 1 [] :none :none :none [:stop [0 1]] [:stop 15] [:stop 12] [:stop 12] [:x :x :x] ["ab" "ac" "ad"] E:conj not supported on this type 58 [1 2 3] [1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 32 33 34 35 36 37 38 39 40] 780"#,
        ),
        (
            r#"(extend-type PersistentHashSet ISeqable (-seq [s] (list :hs (count s))))
               (extend-type List IReduce (-reduce ([l f] [:lr2 (count l)]) ([l f i] [:lr3 i])))
               (extend-type Cons ISeqable (-seq [c] (list :cons)))
               (extend-type IntegerRange IReduce (-reduce ([r f] [:ir2]) ([r f i] [:ir3 i])))
               (extend-type ChunkedCons IReduce (-reduce ([c f] [:cc2]) ([c f i] [:cc3 i])))
               (extend-type LazySeq IReduce (-reduce ([c f] [:lz2]) ([c f i] [:lz3 i])))
               (let [out (array) p (fn [f] (.push out (try (pr-str (f)) (catch :default e (str "E:" (.-message e))))))
                     v40 (let [a (array)] (dotimes [i 40] (.push a i)) (vec a))]
                 (p #(reduce conj [] #{1 2})) (p #(reduce conj #{1 2})) (p #(reduce conj [] (list 1 2))) (p #(reduce conj (list 1 2)))
                 (p #(reduce conj [] (cons 0 (list 1)))) (p #(reduce conj [] (range 4))) (p #(reduce conj (range 4))) (p #(reduce conj [] (seq [1 2 3])))
                 (p #(reduce conj [] (seq (map inc v40)))) (p #(reduce conj [] (map inc [1 2]))) (p #(reduce conj [] (repeat 2 :x)))
                 (p #(reduce conj [] (sorted-set 2 1))) (p #(into [] #{3})) (p #(reduce + (rest v40)))
                 (.join out " "))"#,
            "[:hs 2] E:conj not supported on this type [:lr3 []] [:lr2 2] [:cons] [:ir3 []] [:ir2] [1 2 3] [:cc3 []] [:lz3 []] [:x :x] [1 2] [:hs 1] 780",
        ),
        (
            // `reduce` of a set or a queue's seq walks an iterator that calls
            // the indexed seqs' `-first` and `-next`, which an extension
            // replaces.
            r#"(extend-type IndexedSeq ISeq (-first [_] 10) (-rest [_] ()))
               (let [out (array) p (fn [f] (.push out (try (pr-str (f)) (catch :default e (str "E:" (.-message e))))))
                     q (conj cljs.core/PersistentQueue.EMPTY 1 2 3)]
                 (p #(reduce + 0 (hash-set 1 2 3))) (p #(reduce + (hash-set 1 2 3))) (p #(reduce + 0 (seq q))) (p #(reduce + (seq q)))
                 (p #(reduce + 0 ((fn [& xs] xs) 1 2 3))) (p #(reduce + 0 (IndexedSeq. (array 1 2 3) 0 nil)))
                 (.join out " "))"#,
            "30 30 10 10 6 6",
        ),
        (
            // A set given one arity of `IReduce` takes the lookup for the
            // other, which it does not implement.
            r#"(extend-type PersistentHashSet IReduce (-reduce [_ f] :two))
               (let [out (array) p (fn [f] (.push out (try (pr-str (f)) (catch :default e (str "E:" (.-message e))))))
                     s3 (specify! (hash-set 4 5) IReduce (-reduce [_ f init] [:three init]))]
                 (p #(reduce + (hash-set 1 2))) (p #(reduce + 0 (hash-set 1 2))) (p #(reduce + s3)) (p #(reduce + 0 s3))
                 (.join out " "))"#,
            ":two E:No protocol method IReduce/-reduce defined for this value :two [:three 0]",
        ),
        (
            r#"(let [out (array) p (fn [f] (.push out (try (pr-str (f)) (catch :default e (str "E:" (.-message e))))))
                     s3 (specify! (hash-set 1 2) IReduce (-reduce [_ f init] [:three init]))
                     s2 (specify! (hash-set 3 4) IReduce (-reduce [_ f] :two))]
                 (p #(reduce + s3)) (p #(reduce + 0 s3)) (p #(reduce + s2)) (p #(reduce + 0 s2)) (p #(reduce + 0 (hash-set 5 6)))
                 (.join out " "))"#,
            "E:No protocol method IReduce/-reduce defined for this value [:three 0] :two E:No protocol method IReduce/-reduce defined for this value 11",
        ),
    ] {
        every_level(source, expected);
    }
    // In release output with `extend-type`, `reduce` and `reduce-kv` keep
    // the slots of the classes they reduce, and not their old paths
    // (light-cljs-1072).
    let kept = units(
        "(extend-type PersistentVector IMeta (-meta [_] nil))
         (pr-str [(reduce + [1 2]) (reduce + 0 (subvec [1 2 3] 1)) (reduce conj [] {:a 1}) (reduce conj [] (zipmap (range 9) (range 9)))
                  (reduce conj [] (first {:a 1})) (reduce-kv conj [] [1]) (reduce-kv conj [] {:a 1}) (reduce-kv conj [] (sorted-map 1 2))
                  (reduce + #{1 2}) (reduce + (seq [1 2]))])",
    );
    for unit in [
        "reduceOf",
        "reduceKvOf",
        "PersistentVectorValue.reduceSlot",
        "SubvecValue.reduceSlot",
        "PersistentArrayMapValue.reduceSlot",
        "PersistentHashMapValue.reduceSlot",
        "PersistentVectorValue.kvReduceSlot",
        "PersistentArrayMapValue.kvReduceSlot",
        "treeMapConstructor.kvReduceSlot",
        "PersistentHashSetValue.reduceSlot",
        "IndexedSeqValue.reduceSlot",
    ] {
        assert!(kept.contains(&unit), "{unit}: {kept:?}");
    }
    for unit in ["reducePath", "reduceKvPath", "reduce.protocol"] {
        assert!(!kept.contains(&unit), "{unit}: {kept:?}");
    }
}

/// In release output of a program with `extend-type`, `reduce` and
/// `reduce-kv` of a vector, a subvector, the maps and the public
/// constructors' vectors and array maps take no protocol lookup
/// (light-cljs-1055), nor `reduce` of a hash or array set and of an indexed
/// seq (rest arguments, a short vector's seq, a public constructor's over an
/// array or a string; light-cljs-1059): the readable output's lookups
/// (`extendedMethod`, `builtin`, `satisfies`) count their calls (the sets'
/// and indexed seqs' 14 on master `f8f1ed3f`). A list's, a lazy seq's
/// and a sorted set's `reduce`, which have no slot, still take them.
#[test]
fn reduce_of_vectors_and_maps_takes_no_protocol_lookup() {
    let source = r#"(extend-type PersistentVector IMeta (-meta [_] nil))
        (let [v40 (vec (range 40)) sub (subvec v40 2 7) hm (zipmap (range 20) (range 20)) sm (sorted-map 1 2 3 4)
              pv40 (PersistentVector. nil 40 5 (.-root v40) (.-tail v40) nil)
              pv3 (PersistentVector. nil 3 5 (.-EMPTY_NODE PersistentVector) (array 1 2 3) nil)
              pam (PersistentArrayMap. nil 2 (array :a 1 :b 2) nil) am (array-map :a 1 :b 2) l (list 1 2)
              hs (set (range 20)) as #{1 2 3} lz (doall (map inc [1 2 3])) ss (sorted-set 1 2)
              is (seq [1 2 3]) pis (IndexedSeq. (array 1 2 3) 1 nil) sis (IndexedSeq. "abc" 0 nil) rest-args ((fn [& xs] xs) 1 2 3)
              up (fn [a _] (inc a)) kv (fn [a _ _] (inc a)) out (array) lookups #(aget js/globalThis "__lookups")
              before (lookups)]
          (.push out (reduce up 0 [1 2 3]) (reduce up [1 2 3]) (reduce up 0 v40) (reduce up v40) (reduce up 0 sub) (reduce up sub)
                 (reduce up 0 pv40) (reduce up pv40) (reduce up 0 pv3) (reduce up pv3) (reduce up 0 am) (reduce up 0 hm) (reduce up 0 pam)
                 (reduce-kv kv 0 [1 2 3]) (reduce-kv kv 0 v40) (reduce-kv kv 0 sub) (reduce-kv kv 0 am) (reduce-kv kv 0 hm)
                 (reduce-kv kv 0 sm) (reduce-kv kv 0 pam))
          (let [after (lookups)]
            (.push out (- after before) (reduce up 0 hs) (reduce up hs) (reduce up 0 as) (reduce up 0 is) (reduce up is)
                   (reduce up 0 pis) (reduce up 0 sis) (reduce up 0 rest-args))
            (let [slotted (lookups)]
              (.push out (- slotted after) (reduce up 0 l) (reduce up 0 lz) (reduce up 0 ss) (pos? (- (lookups) slotted)))
              (.join out " "))))"#;
    let mut options = ApplicationOptions::new(OptimizationLevel::Advanced);
    options.pseudo_names = true;
    let mut js = prepare_application(&program(source), options).emit_js();
    for head in [
        "function extendedMethod(value, key, arity, fast = false, directOnly = false) {",
        "function builtin(value, key, arity) {",
        "function satisfies(protocol, value) {",
    ] {
        assert!(js.contains(head), "{head}");
        js = js.replace(head, &format!("{head} globalThis.__lookups++;"));
    }
    assert_eq!(
        execute(&format!("globalThis.__lookups = 0; console.log({js});")),
        "3 3 40 39 5 6 40 39 3 3 2 20 2 3 40 5 2 20 2 2 0 20 19 3 3 3 2 3 3 0 2 3 2 true\n"
    );
}

/// The key and value seqs call their map seq's `-first` and `-next` and the
/// entry's `-key` and `-val` slots (light-cljs-1064), so each program
/// extends one of them, or the maps' `-seq`, or the entries' `-nth`, which
/// `-key` and `-val` read, and prints what `keys` and `vals` give at every
/// level; a program with protocols and no `extend-type` (`installMethods`
/// alone), and the projections of seqs that are not maps', print theirs
/// too. The expected values are master `dbf91703`'s.
#[test]
fn key_and_value_seqs_keep_the_answers_of_extended_map_seqs_and_entries() {
    let walks = r#"(let [out (array) p (fn [f] (.push out (try (pr-str (f)) (catch :default e (str "E:" (re-find #"\S+/\S+" (.-message e)))))))
                        sized (fn [n] (zipmap (range n) (range n)))
                        am (array-map :a 1 :b 2 :c 3) m10 (sized 10) m20 (sized 20) sm (sorted-map 3 :c 1 :a 2 :b)]
                    (doseq [m [am m10 m20 sm]]
                      (p #(vec (take 3 (keys m)))) (p #(vec (take 3 (vals m)))) (p #(first (keys m))) (p #(last (keys m)))
                      (p #(last (vals m))) (p #(take 2 (rest (vals m)))) (p #(take 2 (next (keys m))))
                      (p #(reduce (fn [a x] (if (number? x) (+ a x) (inc a))) 0 (vals m))) (p #(count (into [] (keys m))))
                      (p #(count (keys m))) (p #(= (keys m) (map key m))) (p #(vec (take 2 (seq m)))) (p #(take 2 (rest (seq m)))))
                    (p #(vec (seq (sorted-set 3 1 2)))) (p #(keys nil)) (p #(vals {})) (p #(keys (seq {:a 1 :b 2})))
                    (p #(vals (list (first {:a 1}) (first {:b 2})))) (p #(keys [[1 2]])) (p #(vals (js/Map. (clj->js [[1 2]]))))
                    (.join out " "))"#;
    // Without an extension of these, or with one of `Atom`.
    let plain = "[:a :b :c] [1 2 3] :a :c 3 (2 3) (:b :c) 6 3 3 true [[:a 1] [:b 2]] ([:b 2] [:c 3]) [0 1 2] [0 1 2] 0 9 9 (1 2) (1 2) 45 10 10 true [[0 0] [1 1]] ([1 1] [2 2]) [0 1 2] [0 1 2] 0 19 19 (1 2) (1 2) 190 20 20 true [[0 0] [1 1]] ([1 1] [2 2]) [1 2 3] [:a :b :c] 1 3 :c (:b :c) (2 3) 3 3 3 true [[1 :a] [2 :b]] ([2 :b] [3 :c]) [1 2 3] nil nil (:a :b) (1 2) E:IMapEntry/-key E:IMapEntry/-val";
    for (extensions, expected) in [
        (
            "(defrecord R [a] ICounted (-count [_] 1)) (count (R. 1))",
            plain,
        ),
        ("(extend-type Atom ICounted (-count [_] 1))", plain),
        (
            "(extend-type PersistentArrayMapSeq ISeq (-first [_] (first {:x 1})) (-rest [_] ()))
             (extend-type NodeSeq INext (-next [_] nil))
             (extend-type ArrayNodeSeq ISeq (-first [_] (first {:y 2})))",
            "[:x :x :x] [1 1 1] :x :x 1 (1 1) (:x :x) 3 3 3 false [[:x 1]] () [0] [0] 0 0 0 () () 0 1 1 false [[0 0] [1 1]] ([1 1] [2 2]) [:y :y :y] [2 2 2] :y :y 2 (2 2) (:y :y) 40 20 20 true [[:y 2] [:y 2]] ([:y 2] [:y 2]) [1 2 3] [:a :b :c] 1 3 :c (:b :c) (2 3) 3 3 3 true [[1 :a] [2 :b]] ([2 :b] [3 :c]) [1 2 3] nil nil (:x :x) (1 2) E:IMapEntry/-key E:IMapEntry/-val",
        ),
        (
            "(extend-type MapEntry IMapEntry (-key [e] [:key (nth e 0)]) (-val [e] [:val (nth e 1)]))",
            "[[:key :a] [:key :b] [:key :c]] [[:val 1] [:val 2] [:val 3]] [:key :a] [:key :c] [:val 3] ([:val 2] [:val 3]) ([:key :b] [:key :c]) 3 3 3 true [[:a 1] [:b 2]] ([:b 2] [:c 3]) [[:key 0] [:key 1] [:key 2]] [[:val 0] [:val 1] [:val 2]] [:key 0] [:key 9] [:val 9] ([:val 1] [:val 2]) ([:key 1] [:key 2]) 10 10 10 true [[0 0] [1 1]] ([1 1] [2 2]) [[:key 0] [:key 1] [:key 2]] [[:val 0] [:val 1] [:val 2]] [:key 0] [:key 19] [:val 19] ([:val 1] [:val 2]) ([:key 1] [:key 2]) 20 20 20 true [[0 0] [1 1]] ([1 1] [2 2]) [[:key 1] [:key 2] [:key 3]] [[:val :a] [:val :b] [:val :c]] [:key 1] [:key 3] [:val :c] ([:val :b] [:val :c]) ([:key 2] [:key 3]) 3 3 3 true [[1 :a] [2 :b]] ([2 :b] [3 :c]) [1 2 3] nil nil ([:key :a] [:key :b]) ([:val 1] [:val 2]) E:IMapEntry/-key E:IMapEntry/-val",
        ),
        (
            "(extend-type MapEntry IIndexed (-nth ([e i] [:nth i]) ([e i nf] [:nth i nf])))",
            "[[:nth 0] [:nth 0] [:nth 0]] [[:nth 1] [:nth 1] [:nth 1]] [:nth 0] [:nth 0] [:nth 1] ([:nth 1] [:nth 1]) ([:nth 0] [:nth 0]) 3 3 3 true [[:a 1] [:b 2]] ([:b 2] [:c 3]) [[:nth 0] [:nth 0] [:nth 0]] [[:nth 1] [:nth 1] [:nth 1]] [:nth 0] [:nth 0] [:nth 1] ([:nth 1] [:nth 1]) ([:nth 0] [:nth 0]) 10 10 10 true [[0 0] [1 1]] ([1 1] [2 2]) [[:nth 0] [:nth 0] [:nth 0]] [[:nth 1] [:nth 1] [:nth 1]] [:nth 0] [:nth 0] [:nth 1] ([:nth 1] [:nth 1]) ([:nth 0] [:nth 0]) 20 20 20 true [[0 0] [1 1]] ([1 1] [2 2]) [[:nth 0] [:nth 0] [:nth 0]] [[:nth 1] [:nth 1] [:nth 1]] [:nth 0] [:nth 0] [:nth 1] ([:nth 1] [:nth 1]) ([:nth 0] [:nth 0]) 3 3 3 true [[1 :a] [2 :b]] ([2 :b] [3 :c]) [1 2 3] nil nil ([:nth 0] [:nth 0]) ([:nth 1] [:nth 1]) E:IMapEntry/-key E:IMapEntry/-val",
        ),
        (
            "(extend-type PersistentHashMap ISeqable (-seq [_] (list (first {:z 3}))))
             (extend-type PersistentArrayMap ISeqable (-seq [_] (list [1 2])))
             (extend-type PersistentTreeMapSeq INext (-next [_] nil))",
            "E:IMapEntry/-key E:IMapEntry/-val E:IMapEntry/-key E:IMapEntry/-key E:IMapEntry/-val () () E:IMapEntry/-val E:IMapEntry/-key 1 E:IMapEntry/-key [[1 2]] () E:IMapEntry/-key E:IMapEntry/-val E:IMapEntry/-key E:IMapEntry/-key E:IMapEntry/-val () () E:IMapEntry/-val E:IMapEntry/-key 1 E:IMapEntry/-key [[1 2]] () E:IMapEntry/-key E:IMapEntry/-val E:IMapEntry/-key E:IMapEntry/-key E:IMapEntry/-val () () E:IMapEntry/-val E:IMapEntry/-key 1 E:IMapEntry/-key [[1 2]] () [1] [:a] 1 1 :a () () 1 1 1 false [[1 :a] [2 :b]] ([2 :b] [3 :c]) [1 2 3] nil E:IMapEntry/-val E:IMapEntry/-key E:IMapEntry/-val E:IMapEntry/-key E:IMapEntry/-val",
        ),
    ] {
        every_level(&format!("{extensions} {walks}"), expected);
    }
}

/// The key and value seqs read their entries' seq's slots only where it has
/// a keyed kind, as the slot-first wrappers do (light-cljs-1064): a host seq
/// that `specify!` gave `ISeqable` and `ISeq`, with an `-next` member written
/// without `INext`, projects as on master `9b0028f0`, whose `next` ignored
/// that member, at every level.
#[test]
fn key_and_value_seqs_ignore_unmarked_next_slots_on_host_seqs() {
    every_level(
        r#"(let [s (js-obj)
                 p (fn [f] (try (pr-str (f)) (catch :default e (str "E:" (.-message e)))))]
             (specify! s ISeqable (-seq [this] this) ISeq (-first [_] (first {:a 1})) (-rest [_] ()))
             (set! (.-cljs$core$INext$_next$arity$1 s) (fn [_] (list (first {:b 2}))))
             (str [(p #(first (keys s))) (p #(next (keys s))) (p #(first (next (keys s)))) (p #(vec (keys s)))
                   (p #(vec (vals s))) (p #(next s)) (p #(rest (vals s))) (p #(count (keys s)))]))"#,
        r#"[":a" "nil" "nil" "[:a]" "[1]" "nil" "()" "1"]"#,
    );
}

/// In release output of a program with `extend-type`, a walk of a map's
/// keys or values, or of its seq, takes no protocol lookup per item
/// (light-cljs-1064): the map seqs (`PersistentArrayMapSeq`, `NodeSeq`,
/// `ArrayNodeSeq`) have `-first`, `-rest`, `-next` and `-seq` slots, the key
/// and value seqs' slots are on their prototype, and their steps call their
/// map seq's and entry's slots, where each step took 9 lookups on master
/// `dbf91703`. Each number is a walk's lookups (as
/// `reduce_of_vectors_and_maps_takes_no_protocol_lookup` counts them) over a
/// larger map less those over a smaller one of the same kind: an array map
/// of 8 and 2 keys, a hash map of 13 and 9 (a `NodeSeq`) and of 100 and 20
/// (an `ArrayNodeSeq`) and a sorted map of 10 and 2, whose seq's stack of
/// nodes, a list, peeks its first with no lookup where a program can extend
/// lists (it took one per node), and a sorted set's seq (its tree map's keys)
/// of 10 and 2 members. A walk's own steps (`keys`, `seq` of the map, the
/// `seq` of the empty list that ends it) take the same lookups whatever its
/// length.
#[test]
fn walks_of_map_keys_and_values_take_no_protocol_lookup_per_item() {
    let source = r#"(extend-type Atom ICounted (-count [_] 1))
        (let [lookups #(aget js/globalThis "__lookups")
              cost (fn [f] (let [before (lookups)] (f) (- (lookups) before)))
              walk (fn [s] (loop [q s a 0] (if q (recur (next q) (+ a (if (nil? (first q)) 0 1))) a)))
              walk-rest (fn [s] (loop [q s a 0] (if (seq q) (recur (rest q) (+ a (if (nil? (first q)) 0 1))) a)))
              sized (fn [n] (zipmap (range n) (range n)))
              pairs [[(array-map :a 1 :b 2) (apply array-map (range 16))] [(sized 9) (sized 13)] [(sized 20) (sized 100)]
                     [(sorted-map 1 2 3 4) (apply sorted-map (range 20))]]
              walks [#(walk (keys %)) #(walk (vals %)) #(walk-rest (keys %)) #(walk (seq %)) #(walk-rest (seq %))
                     #(reduce + (vals %)) #(reduce (fn [a k] (if (nil? k) a (inc a))) 0 (keys %)) #(count (keys %))]
              out (array)]
          (doseq [[small big] pairs w walks]
            (.push out (- (cost #(w big)) (cost #(w small)))))
          (let [small (sorted-set 1 3) big (apply sorted-set (range 10))]
            (.push out (- (cost #(walk (seq big))) (cost #(walk (seq small))))))
          (.join out " "))"#;
    let mut options = ApplicationOptions::new(OptimizationLevel::Advanced);
    options.pseudo_names = true;
    let mut js = prepare_application(&program(source), options).emit_js();
    for head in [
        "function extendedMethod(value, key, arity, fast = false, directOnly = false) {",
        "function builtin(value, key, arity) {",
        "function satisfies(protocol, value) {",
    ] {
        assert!(js.contains(head), "{head}");
        js = js.replace(head, &format!("{head} globalThis.__lookups++;"));
    }
    assert_eq!(
        execute(&format!("globalThis.__lookups = 0; console.log({js});")),
        format!("{}\n", ["0"; 4 * 8 + 1].join(" "))
    );
}

/// In release output of a program with `extend-type`, `into` of a vector,
/// a set or a map, `mapv`, `filterv` and `vec` of a seq, and `conj!` and
/// `assoc!` onto transients take no protocol lookup per item
/// (light-cljs-1065): the transient vectors, maps and sets have `-conj!`,
/// `-assoc!` (but the sets) and `-persistent!` slots, which `conj!`,
/// `assoc!`, `persistent!` and the reductions of `into` and `mapv` call
/// first, and a map transient's `conj!` of a map entry reads the entry's
/// `-key` and `-val` slots. On master `a397e0c9` each item took one lookup
/// (of `-conj!` or `-assoc!`), `into {}` of pairs two and of map entries
/// four. Each number is a call's lookups over 13 items less those over 3
/// (as `walks_of_map_keys_and_values_take_no_protocol_lookup_per_item`
/// counts them), for a map's keys, a lazy seq, a list, a vector's seq and a
/// vector, then for a map, its seq and a lazy seq of its entries.
#[test]
fn into_and_transient_updates_take_no_protocol_lookup_per_item() {
    let source = r#"(extend-type Atom ICounted (-count [_] 1))
        (let [lookups #(aget js/globalThis "__lookups")
              cost (fn [f] (let [before (lookups)] (f) (- (lookups) before)))
              sized (fn [n] (zipmap (range n) (range n)))
              per-item (fn [sources fs]
                         (for [f fs [small big] sources] (let [s (small) b (big)] (- (cost #(f b)) (cost #(f s))))))
              items [[#(keys (sized 3)) #(keys (sized 13))] [#(map inc (range 3)) #(map inc (range 13))]
                     [#(list 1 2 3) #(apply list (range 13))] [#(seq [1 2 3]) #(seq (vec (range 13)))] [#(vector 1 2 3) #(vec (range 13))]]
              entries [[#(sized 3) #(sized 13)] [#(seq (sized 3)) #(seq (sized 13))] [#(filter some? (sized 3)) #(filter some? (sized 13))]]]
          (.join (into-array
                   (concat
                     (per-item items [#(into [] %) #(into #{} %) #(into [] (map identity) %) #(into #{} (map identity) %) #(mapv identity %)
                                      #(filterv some? %) #(vec %) #(persistent! (reduce conj! (transient []) %))
                                      #(persistent! (reduce conj! (transient #{}) %)) #(into {} (map (fn [x] [x x])) %)
                                      #(persistent! (reduce (fn [t x] (assoc! t x x)) (transient {}) %))])
                     (per-item entries [#(into {} %) #(persistent! (reduce conj! (transient {}) %)) #(persistent! (conj! (transient {}) %))])))
                 " "))"#;
    let mut options = ApplicationOptions::new(OptimizationLevel::Advanced);
    options.pseudo_names = true;
    let mut js = prepare_application(&program(source), options).emit_js();
    for head in [
        "function extendedMethod(value, key, arity, fast = false, directOnly = false) {",
        "function builtin(value, key, arity) {",
        "function satisfies(protocol, value) {",
    ] {
        assert!(js.contains(head), "{head}");
        js = js.replace(head, &format!("{head} globalThis.__lookups++;"));
    }
    assert_eq!(
        execute(&format!("globalThis.__lookups = 0; console.log({js});")),
        format!("{}\n", ["0"; 11 * 5 + 3 * 3].join(" "))
    );
}

/// In release output of a program with `extend-type`, `into` of a small
/// vector, map or set, onto a vector, map or set with or without metadata,
/// with or without a transducer, takes no protocol lookup per call
/// (light-cljs-1067): `into` reads the target's `IEditableCollection` mask
/// bit and calls its `-as-transient` slot, and carries its metadata through
/// its `-meta` and `-with-meta` slots, as upstream's `into` calls
/// `-as-transient`, `meta` and `with-meta`, where on master `8903e693`
/// each call took the lookups of `-as-transient` and of `-with-meta` (twice).
/// Each number is one call's lookups, counted as
/// `into_and_transient_updates_take_no_protocol_lookup_per_item` counts them.
#[test]
fn into_of_small_collections_takes_no_protocol_lookup_per_call() {
    let source = r#"(extend-type Atom ICounted (-count [_] 1))
        (let [lookups #(aget js/globalThis "__lookups")
              cost (fn [f] (let [before (lookups) r (f)] [(- (lookups) before) r]))
              m {:m 1} v [1 2 3] am {:a 1 :b 2} s #{1 2} mv (with-meta [] m) mm (with-meta {} m) ms (with-meta #{} m)
              hm (zipmap (range 9) (range 9))
              calls [#(into [] v) #(into [0] v) #(into mv v) #(into [] s) #(into [] am)
                     #(into {} am) #(into {:z 0} am) #(into mm am) #(into {} [[1 2]]) #(count (into hm am))
                     #(into #{} v) #(into #{0} s) #(into ms s)
                     #(into [] (map inc) v) #(into ms (map inc) v) #(into {} (filter some?) am)]
              results (mapv cost calls)]
          (pr-str [(mapv first results) (mapv (comp meta second) results) (mapv second results)]))"#;
    let mut options = ApplicationOptions::new(OptimizationLevel::Advanced);
    options.pseudo_names = true;
    let mut js = prepare_application(&program(source), options).emit_js();
    for head in [
        "function extendedMethod(value, key, arity, fast = false, directOnly = false) {",
        "function builtin(value, key, arity) {",
        "function satisfies(protocol, value) {",
        "function coreProtocolImplementation(protocol, value) {",
    ] {
        assert!(js.contains(head), "{head}");
        js = js.replace(head, &format!("{head} globalThis.__lookups++;"));
    }
    let expected = "[[0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0] \
         [nil nil {:m 1} nil nil nil nil {:m 1} nil nil nil nil {:m 1} nil {:m 1} nil] \
         [[1 2 3] [0 1 2 3] [1 2 3] [1 2] [[:a 1] [:b 2]] {:a 1, :b 2} {:z 0, :a 1, :b 2} {:a 1, :b 2} {1 2} 11 \
         #{1 2 3} #{0 1 2} #{1 2} [2 3 4] #{2 3 4} {:a 1, :b 2}]]";
    assert_eq!(
        execute(&format!("globalThis.__lookups = 0; console.log({js});")),
        format!("{expected}\n")
    );
}

/// The editable collections' `-as-transient` and `-with-meta` slots, which
/// `transient`, `-with-meta` and `into` call before any lookup
/// (light-cljs-1067), keep master `8903e693`'s answers at every level: for
/// `extend-type` of the protocols on a vector and a hash set and of `IMeta`
/// on an array map, `specify!` of them on values, `extend-type` of
/// `js/Object`, whose methods the slots shadow, and members a program
/// writes by name on a class's prototype (nil, false or a method) or on a
/// value; for targets that are no editable collection (nil, a list, sorted
/// collections, a subvector, a map entry); and for programs with protocol
/// implementations and none.
#[test]
fn transient_and_with_meta_slots_keep_extended_answers() {
    let head = r#"(let [out (array) p (fn [f] (.push out (try (pr-str (f)) (catch :default e (str "E:" (.-message e))))))
                       hm (zipmap (range 9) (range 9))]"#;
    let common = r#"(p #(into nil [1 2])) (p #(into () [1 2])) (p #(into (sorted-set) [3 1])) (p #(into (sorted-map) {:b 1 :a 2}))
                    (p #(into (subvec [1 2 3] 1) [4])) (p #(into (first {:a 1}) [3])) (p #(meta (into (with-meta [] {:mv 1}) [1])))
                    (p #(meta (into (with-meta #{} {:ms 1}) (map inc) [1]))) (p #(meta (into (with-meta hm {:mh 1}) {:a 1})))
                    (p #(meta (-with-meta [1] {:x 1}))) (p #(meta (-with-meta (fn [] 1) {:f 1}))) (p #(-with-meta (transient []) nil))
                    (p #(-with-meta (atom 1) nil)) (p #(-with-meta nil nil)) (p #(count (into hm {:a 1})))
                    (.join out " "))"#;
    let programs = [
        (
            r#"(extend-type PersistentVector IEditableCollection (-as-transient [v] (conj! (transient #{}) [:ext (count v)])))
           (extend-type PersistentHashSet IWithMeta (-with-meta [s m] [:wm (count s) m]))
           (extend-type PersistentArrayMap IMeta (-meta [m] {:am (count m)}))"#,
            r#"(p #(into [] [1 2])) (p #(into [0] (map inc) [1 2])) (p #(persistent! (transient [1]))) (p #(into #{} [1 2]))
           (p #(into #{} (map inc) [1 2])) (p #(into {} {:a 1})) (p #(meta (into {} {:a 1}))) (p #(into {:b 2} (map identity) {:a 1}))
           (p #(meta (into {:b 2} (map identity) {:a 1}))) (p #(-with-meta #{1} {:x 1})) (p #(-with-meta {:a 1} {:x 1}))"#,
        ),
        (
            "",
            r#"(let [v (specify! [1 2] IEditableCollection (-as-transient [_] (transient [:sv])))
                 s (specify! #{1} IWithMeta (-with-meta [_ m] [:sw m]))
                 m (specify! {:a 1} IMeta (-meta [_] {:sm 1}))]
             (p #(into v [3])) (p #(into v (map inc) [3])) (p #(persistent! (transient v))) (p #(into s [2])) (p #(-with-meta s {:k 1}))
             (p #(into m {:b 2})) (p #(meta (into m {:b 2}))) (p #(meta (into m (map identity) {:b 2}))) (p #(into [] s)))"#,
        ),
        (
            r#"(extend-type js/Object IWithMeta (-with-meta [o m] [:obj m]))"#,
            r#"(p #(into [] [1])) (p #(into {} {:a 1})) (p #(into #{} (map inc) [1])) (p #(-with-meta [1] {:a 1}))
           (p #(-with-meta (list 1) {:a 1})) (p #(with-meta [1] {:a 1})) (p #(into (with-meta [] {:a 1}) [2]))"#,
        ),
        (
            r#"(extend-type js/Object IEditableCollection (-as-transient [o] :objt))"#,
            r#"(p #(transient [1])) (p #(transient {:a 1})) (p #(transient #{})) (p #(into [] [1])) (p #(into #{} []))
           (p #(into {} (map identity) {}))"#,
        ),
        (
            "",
            r#"(let [w [5]]
             (set! (.. PersistentVector -prototype -cljs$core$IEditableCollection$_as_transient$arity$1) nil)
             (set! (.. PersistentHashSet -prototype -cljs$core$IWithMeta$_with_meta$arity$2) false)
             (set! (.. PersistentHashMap -prototype -cljs$core$IEditableCollection$_as_transient$arity$1) (fn [_] (transient {:hm 0})))
             (set! (.-cljs$core$IWithMeta$_with_meta$arity$2 w) (fn [_ m] [:byname m]))
             (p #(into [] [1 2])) (p #(into (with-meta [] {:a 1}) [1])) (p #(into #{} [1])) (p #(meta (into (with-meta #{} {:s 1}) [1])))
             (p #(-with-meta w {:q 1})) (p #(into w [2])) (p #(persistent! (transient hm))) (p #(count (into hm {:a 1}))))"#,
        ),
        (
            "(deftype T [] ICounted (-count [_] 7))",
            r#"(p #(count (T.))) (p #(into [] [1 2])) (p #(into {} {:a 1})) (p #(into #{} (map inc) [1]))"#,
        ),
        (
            "",
            r#"(p #(into [] [1 2])) (p #(into {} {:a 1})) (p #(into #{} (map inc) [1]))"#,
        ),
    ];
    let expected = [
        r#"[:wm 3 nil] [:wm 3 nil] #{[:ext 1]} [:wm 2 nil] [:wm 2 nil] {:a 1} {:am 1} {:b 2, :a 1} {:am 2} [:wm 1 {:x 1}] {:a 1} (2 1) (2 1) #{1 3} {:a 2, :b 1} [2 3 4] [:a 1 3] nil nil {:mh 1} {:x 1} E:No protocol method IWithMeta.-with-meta defined for this value E:No protocol method IWithMeta.-with-meta defined for this value E:No protocol method IWithMeta.-with-meta defined for this value E:No protocol method IWithMeta.-with-meta defined for this value 10"#,
        r#"[:sv 3] [:sv 4] [:sv] #{1 2} [:sw {:k 1}] {:a 1, :b 2} {:sm 1} {:sm 1} [1] (2 1) (2 1) #{1 3} {:a 2, :b 1} [2 3 4] [:a 1 3] {:mv 1} {:ms 1} {:mh 1} {:x 1} E:No protocol method IWithMeta.-with-meta defined for this value E:No protocol method IWithMeta.-with-meta defined for this value E:No protocol method IWithMeta.-with-meta defined for this value E:No protocol method IWithMeta.-with-meta defined for this value 10"#,
        r#"[:obj nil] [:obj nil] [:obj nil] [:obj {:a 1}] [:obj {:a 1}] [:obj {:a 1}] [:obj nil] (2 1) (2 1) #{1 3} {:a 2, :b 1} [2 3 4] [:a 1 3] nil nil nil nil {:f 1} [:obj nil] [:obj nil] E:No protocol method IWithMeta.-with-meta defined for this value 2"#,
        r#":objt :objt :objt E:No protocol method ITransientCollection/-conj! defined for this value E:No protocol method ITransientCollection/-persistent! defined for this value E:No protocol method ITransientCollection/-persistent! defined for this value (2 1) E:No protocol method ITransientCollection/-conj! defined for this value E:No protocol method ITransientCollection/-conj! defined for this value E:No protocol method ITransientCollection/-conj! defined for this value E:No protocol method ITransientCollection/-conj! defined for this value E:No protocol method ITransientCollection/-conj! defined for this value E:No protocol method ITransientCollection/-conj! defined for this value E:No protocol method ITransientCollection/-conj! defined for this value E:No protocol method ITransientCollection/-conj! defined for this value {:x 1} E:No protocol method IWithMeta.-with-meta defined for this value E:No protocol method IWithMeta.-with-meta defined for this value E:No protocol method IWithMeta.-with-meta defined for this value E:No protocol method IWithMeta.-with-meta defined for this value E:No protocol method ITransientCollection/-conj! defined for this value"#,
        r#"[1 2] [1] #{1} {:s 1} [:byname {:q 1}] [5 2] {:hm 0} 2 (2 1) (2 1) #{1 3} {:a 2, :b 1} [2 3 4] [:a 1 3] {:mv 1} {:ms 1} {:mh 1} {:x 1} E:No protocol method IWithMeta.-with-meta defined for this value E:No protocol method IWithMeta.-with-meta defined for this value E:No protocol method IWithMeta.-with-meta defined for this value E:No protocol method IWithMeta.-with-meta defined for this value 2"#,
        r#"7 [1 2] {:a 1} #{2} (2 1) (2 1) #{1 3} {:a 2, :b 1} [2 3 4] [:a 1 3] {:mv 1} {:ms 1} {:mh 1} {:x 1} E:No protocol method IWithMeta.-with-meta defined for this value E:No protocol method IWithMeta.-with-meta defined for this value E:No protocol method IWithMeta.-with-meta defined for this value E:No protocol method IWithMeta.-with-meta defined for this value 10"#,
        r#"[1 2] {:a 1} #{2} (2 1) (2 1) #{1 3} {:a 2, :b 1} [2 3 4] [:a 1 3] {:mv 1} {:ms 1} {:mh 1} {:x 1} E:No protocol method IWithMeta.-with-meta defined for this value E:No protocol method IWithMeta.-with-meta defined for this value E:No protocol method IWithMeta.-with-meta defined for this value E:No protocol method IWithMeta.-with-meta defined for this value 10"#,
    ];
    for ((extensions, body), expected) in programs.iter().zip(expected) {
        every_level(&format!("{extensions} {head} {body} {common}"), expected);
    }
}

/// The `-with-meta` slot (`withMetaSlot`) reads a value's metadata once, as
/// `collectionWithMeta`'s path did (the Sol review of light-cljs-1067): for a
/// vector, an array and a hash map and a set that `specify!` gave an `IMeta`
/// whose answer changes after its first read, `-with-meta`, `with-meta`,
/// `vary-meta` and `into`, with and without a transducer, call it as often
/// and give what master `8903e693` gave at every level: each entry is the
/// reads, whether the result is the value itself and its metadata.
#[test]
fn with_meta_slots_read_extended_meta_once() {
    every_level(
        r#"(let [out (array)
                 probe (fn [make f]
                         (let [reads (atom 0) m {:x 1}
                               v (specify! (make) IMeta (-meta [_] (if (= 1 (swap! reads inc)) nil m)))
                               r (f v m)]
                           (.push out (pr-str [@reads (identical? v r) (meta r)]))))]
             (doseq [make [(fn [] [1]) (fn [] {:a 1}) (fn [] (zipmap (range 9) (range 9))) (fn [] #{1})]
                     f [(fn [v m] (-with-meta v m)) (fn [v m] (with-meta v m)) (fn [v m] (vary-meta v assoc :k 1))
                        (fn [v _] (into v [[2 3]])) (fn [v _] (into v (map identity) [[2 3]]))]]
               (probe make f))
             (.join out " "))"#,
        &["[1 false {:x 1}] [1 false {:x 1}] [2 false {:k 1}] [1 false nil] [1 false nil]"; 4]
            .join(" "),
    );
}

/// In release output of a program with `extend-type`, `into` onto a target
/// that is no editable collection (a sorted set and map, a list and `()`, a
/// queue, a subvector, a map entry, a program type that implements
/// `ICollection` and a record) takes no protocol lookup per item
/// (light-cljs-1076): it conjoins each item through the target's `-conj`
/// slot, as upstream's `into` calls `-conj`, where on master `86dc4aa0`
/// each item took the lookup of `-conj`'s dispatcher (`conjProtocol`). Each
/// number is a call's lookups over 13 items less those over 3, counted as
/// `into_and_transient_updates_take_no_protocol_lookup_per_item` counts them,
/// for a vector, a lazy seq and a list of items, then a vector and a seq of
/// pairs.
#[test]
fn into_onto_other_collections_takes_no_protocol_lookup_per_item() {
    let source = r#"(extend-type Atom ICounted (-count [_] 1))
        (defrecord R [a])
        (deftype T [xs] ICollection (-conj [_ x] (T. (conj xs x))))
        (let [lookups #(aget js/globalThis "__lookups")
              cost (fn [f] (let [before (lookups)] (f) (- (lookups) before)))
              per-item (fn [sources fs]
                         (for [f fs [small big] sources] (let [s (small) b (big)] (- (cost #(f b)) (cost #(f s))))))
              items [[#(vec (range 3)) #(vec (range 13))] [#(map inc (range 3)) #(map inc (range 13))]
                     [#(list 1 2 3) #(apply list (range 13))]]
              pairs [[#(mapv (fn [i] [i i]) (range 3)) #(mapv (fn [i] [i i]) (range 13))]
                     [#(seq (zipmap (range 3) (range 3))) #(seq (zipmap (range 13) (range 13)))]]]
          (.join (into-array
                   (concat
                     (per-item items [#(into (sorted-set) %) #(into (list 0) %) #(into () %) #(into cljs.core/PersistentQueue.EMPTY %)
                                      #(into (subvec [1 2 3] 1) %) #(into (first {:a 1}) %) #(into (T. []) %)])
                     (per-item pairs [#(into (sorted-map) %) #(into (sorted-map-by >) %) #(into (->R 1) %)])))
                 " "))"#;
    let mut options = ApplicationOptions::new(OptimizationLevel::Advanced);
    options.pseudo_names = true;
    let mut js = prepare_application(&program(source), options).emit_js();
    for head in [
        "function extendedMethod(value, key, arity, fast = false, directOnly = false) {",
        "function builtin(value, key, arity) {",
        "function satisfies(protocol, value) {",
        "function coreProtocolImplementation(protocol, value) {",
    ] {
        assert!(js.contains(head), "{head}");
        js = js.replace(head, &format!("{head} globalThis.__lookups++;"));
    }
    assert_eq!(
        execute(&format!("globalThis.__lookups = 0; console.log({js});")),
        format!("{}\n", ["0"; 7 * 3 + 3 * 2].join(" "))
    );
}

/// `into` onto a target that is no editable collection, which conjoins each
/// item through the target's `-conj` slot before any lookup
/// (light-cljs-1076), keeps master `86dc4aa0`'s answers at every level: for
/// `extend-type` of `ICollection` on a sorted map, a list, a queue, a
/// subvector and a lazy seq, `specify!` of it on values, `extend-type` of
/// `js/Object`, whose method the classes' slots shadow, of `object` and of
/// `default`, members a program writes by name on a class's prototype (nil,
/// false or a method) or on a value, and the slots of a program type, a list
/// and two vectors that a getter answers nil or false first, which `into`
/// and a map's `-conj` read as often as the dispatchers' lookups did (the
/// Sol review of light-cljs-1076); for nil, seqs without a slot, records,
/// metadata, and values that are no collection, whose error is the
/// dispatcher's; and for programs with protocol implementations and none. A
/// map's `-conj` of a vector or a map entry reads its items through their
/// `-nth` slots: each program also conjoins entries onto maps and sorted
/// maps, with `extend-type` of `IIndexed` on vectors and map entries and on
/// `js/Object`, `specify!` of it, members written by name, and program types
/// that implement `IVector` with and without `IIndexed`.
#[test]
fn into_onto_other_collections_keeps_extended_answers() {
    let head = r#"(let [out (array)
                       p (fn [f] (.push out (try (pr-str (f))
                                              (catch :default e (if (instance? js/TypeError e) "TypeError" (str "E:" (.-message e)))))))]"#;
    let common = r#"(p #(into nil [1 2])) (p #(into () [1 2])) (p #(into (list 0) [1 2])) (p #(into (sorted-set) [3 1]))
                    (p #(into (sorted-map) {:b 1 :a 2})) (p #(into (sorted-map-by >) [[1 :a] [2 :b]])) (p #(seq (into cljs.core/PersistentQueue.EMPTY [1 2])))
                    (p #(into (subvec [1 2 3] 1) [4])) (p #(into (first {:a 1}) [3])) (p #(into (map inc [1]) [5])) (p #(into (cons 1 nil) [2]))
                    (p #(into (seq [1 2]) [3])) (p #(into (range 2) [5])) (p #(into (seq {:a 1}) [3])) (p #(into (list 0) []))
                    (p #(meta (into (with-meta (sorted-map) {:m 1}) {:a 1}))) (p #(meta (into (with-meta (list 0) {:l 1}) [1])))
                    (p #(meta (into (with-meta (sorted-set) {:s 1}) [1]))) (p #(meta (into (with-meta cljs.core/PersistentQueue.EMPTY {:q 1}) [1])))
                    (p #(into 1 [2])) (p #(into (atom 1) [2])) (p #(into "a" [1])) (p #(into (js-obj) [1])) (p #(into (array) [1]))
                    (p #(into (sorted-map) [1])) (p #(into 1 []))
                    (p #(conj {} [:a 1])) (p #(into (sorted-map) [[:a 1] (first {:b 2})])) (p #(conj (sorted-map) {:c 3})) (p #(conj {} (list 1 2)))
                    (p #(conj (zipmap (range 9) (range 9)) [:h 1])) (p #(into (sorted-map) (seq {:d 4})))
                    (.join out " "))"#;
    let types = r#"(defrecord R [a])
                   (deftype T [xs] ICollection (-conj [_ x] (T. (conj xs x))) ICounted (-count [_] (count xs)))
                   (deftype U [])
                   (deftype V [] IVector IIndexed (-nth [_ i] (if (= i 0) :vk :vv)) (-nth [_ i _] i))
                   (deftype W [] IVector)"#;
    let typed = r#"(p #(into (->R 1) {:b 2})) (p #(into (->R 1) [[:a 5] [:c 6]])) (p #(count (into (T. []) [1 2]))) (p #(into (U.) [1]))
                   (p #(into (sorted-map) [(V.)])) (p #(conj {} (V.))) (p #(conj {} (W.))) (p #(into (sorted-map) [(W.)]))
                   (let [slot (js* "'cljs$core$ICollection$_conj$arity$2$LKEY'")
                         answer (fn [first-read]
                                  (let [value (T. []) reads (atom 0)]
                                    (js/Object.defineProperty value slot
                                      #js {:get (fn [] (if (= 1 (swap! reads inc)) first-read (fn [& _] :unexpected)))})
                                    (p #(into value [1]))
                                    (.push out @reads)))]
                     (answer nil) (answer false))
                   (let [getter (fn [value slot first-read f]
                                  (let [reads (atom 0)]
                                    (js/Object.defineProperty value slot
                                      #js {:get (fn [] (if (= 1 (swap! reads inc)) first-read (fn [& _] :unexpected)))})
                                    (p #(f value))
                                    (.push out @reads)))
                         conj-slot (js* "'cljs$core$ICollection$_conj$arity$2$LKEY'")
                         nth-slot (js* "'cljs$core$IIndexed$_nth$arity$2$LKEY'")]
                     (doseq [first-read [nil false]]
                       (getter (list (count out)) conj-slot first-read #(into % [2]))
                       (getter (vector :a (count out)) nth-slot first-read #(conj {} %))
                       (getter (vector :b (count out)) nth-slot first-read #(into (sorted-map) [%]))))"#;
    let programs = [
        (
            r#"(extend-type PersistentTreeMap ICollection (-conj [m x] [:tm (count m) x]))
               (extend-type List ICollection (-conj [l x] [:list (count l) x]))
               (extend-type PersistentQueue ICollection (-conj [q x] [:q (count q) x]))
               (extend-type Subvec ICollection (-conj [v x] [:sv (count v) x]))
               (extend-type LazySeq ICollection (-conj [s x] [:lz x]))"#,
            "",
        ),
        (
            "",
            r#"(let [sm (specify! (sorted-map :a 1) ICollection (-conj [_ x] [:ssm x]))
                    l (specify! (list 1) ICollection (-conj [_ x] [:sl x]))
                    q (specify! (conj cljs.core/PersistentQueue.EMPTY 1) ICollection (-conj [_ x] [:sq x]))
                    lz (specify! (map inc [1]) ICollection (-conj [_ x] [:slz x]))
                    o (specify! (js-obj) ICollection (-conj [_ x] [:so x]))]
                (p #(into sm [[:b 2]])) (p #(into l [2 3])) (p #(into q [2])) (p #(into lz [2])) (p #(into o [2])) (p #(conj l 2)))"#,
        ),
        (
            r#"(extend-type js/Object ICollection (-conj [o x] [:obj x]))"#,
            "",
        ),
        (
            r#"(extend-type object ICollection (-conj [o x] [:object x]))"#,
            "",
        ),
        (
            r#"(extend-type default ICollection (-conj [o x] [:default x]))"#,
            "",
        ),
        (
            "",
            r#"(set! (.. PersistentTreeMap -prototype -cljs$core$ICollection$_conj$arity$2) nil)
               (set! (.. PersistentTreeSet -prototype -cljs$core$ICollection$_conj$arity$2) false)
               (set! (.. PersistentQueue -prototype -cljs$core$ICollection$_conj$arity$2) (fn [_ x] [:byname x]))
               (let [l (list 1) e (list 2) s (subvec [1 2 3] 1)]
                 (set! (.-cljs$core$ICollection$_conj$arity$2 l) nil)
                 (set! (.-cljs$core$ICollection$_conj$arity$2 e) false)
                 (set! (.-cljs$core$ICollection$_conj$arity$2 s) (fn [_ x] [:byname-sv x]))
                 (p #(into l [2])) (p #(into e [3])) (p #(into s [4])) (p #(into (sorted-map) [[:a 1]])) (p #(into (sorted-set) [1])))"#,
        ),
        (
            r#"(extend-type PersistentVector IIndexed (-nth ([v i] [:nth i]) ([v i nf] [:nth3 i])))
               (extend-type MapEntry IIndexed (-nth ([e i] [:enth i]) ([e i nf] [:enth3 i])))"#,
            "",
        ),
        (
            "",
            r#"(let [v (specify! (vector :a 1) IIndexed (-nth ([_ i] [:s i]) ([_ i nf] [:s3 i])))
                    e (specify! (first {:k :v}) IIndexed (-nth ([_ i] [:se i]) ([_ i nf] [:se3 i])))]
                (p #(into (sorted-map) [v e])) (p #(conj {} v)) (p #(conj (sorted-map) e)) (p #(conj {} [:b 2])))"#,
        ),
        (
            r#"(extend-type js/Object IIndexed (-nth ([o i] [:obj i]) ([o i nf] [:obj3 i])))"#,
            "(p #(nth (js-obj) 0))",
        ),
        (
            "",
            r#"(set! (.. MapEntry -prototype -cljs$core$IIndexed$_nth$arity$2) false)
               (let [v (vector :a 1) w (vector :b 2)]
                 (set! (.-cljs$core$IIndexed$_nth$arity$2 v) nil)
                 (set! (.-cljs$core$IIndexed$_nth$arity$2 w) (fn [_ i] [:byname i]))
                 (p #(into (sorted-map) [v])) (p #(conj {} v)) (p #(conj (sorted-map) w)) (p #(conj {} w)))"#,
        ),
        (
            "(deftype X [] ICounted (-count [_] 7))",
            "(p #(count (X.)))",
        ),
    ];
    let mut sources = Vec::new();
    for (extensions, body) in programs {
        sources.push(format!(
            "{types} {extensions} {head} {body} {typed} {common}"
        ));
    }
    // A program without protocol implementations.
    sources.push(format!("{head} {common}"));
    let outputs: Vec<String> = sources
        .iter()
        .map(|source| {
            let program = program(source);
            let mut answers = OptimizationLevel::ALL.iter().map(|level| {
                execute(&format!(
                    "console.log({});",
                    prepare_application(&program, ApplicationOptions::new(*level)).emit_js()
                ))
            });
            let first = answers.next().unwrap();
            for (answer, level) in answers.zip(OptimizationLevel::ALL.iter().skip(1)) {
                assert_eq!(answer, first, "at {level}: {source}");
            }
            first.trim_end().to_string()
        })
        .collect();
    // `@` is the dispatcher's error for a value that is no collection.
    let none = "E:No protocol method ICollection/-conj defined for this value";
    let expected = [
        "#cljs.user.R{:a 1, :b 2} #cljs.user.R{:a 5, :c 6} 2 @ [:tm 0 #object[cljs.user.V \
         [object Object]]] {:vk :vv} E:No protocol method IIndexed/-nth defined for this value \
         [:tm 0 #object[cljs.user.W [object Object]]] @ 1 TypeError 1 :unexpected 2 {:unexpected \
         :unexpected} 3 [:tm 0 [:b 16]] 0 TypeError 1 TypeError 1 [:tm 0 [:b 22]] 0 [:list 1 2] \
         [:list 1 2] [:list 1 1 2] #{1 3} [:tm 0 [:b 1] [:a 2]] [:tm 0 [1 :a] [2 :b]] (:q 0 1 2) \
         [:sv 2 4] [:a 1 3] [:lz 5] [:list 1 2] (3 1 2) (5 0 1) (3 [:a 1]) (0) nil nil {:s 1} \
         nil @ @ @ @ @ [:tm 0 1] 1 {:a 1} [:tm 0 [:a 1] [:b 2]] [:tm 0 {:c 3}] E:Map conj \
         requires vector entries {0 0, 1 1, 2 2, 3 3, 4 4, 5 5, 6 6, 7 7, 8 8, :h 1} [:tm 0 [:d \
         4]]",
        "[:ssm [:b 2]] [:sl 2 3] [:sq 2] [:slz 2] [:so 2] [:sl 2] #cljs.user.R{:a 1, :b 2} \
         #cljs.user.R{:a 5, :c 6} 2 @ {:vk :vv} {:vk :vv} E:No protocol method IIndexed/-nth \
         defined for this value E:No protocol method IIndexed/-nth defined for this value @ 1 \
         TypeError 1 :unexpected 2 {:unexpected :unexpected} 3 {:unexpected :unexpected} 3 \
         TypeError 1 TypeError 1 TypeError 1 (2 1) (2 1) (2 1 0) #{1 3} {:a 2, :b 1} {2 :b, 1 \
         :a} (1 2) [2 3 4] [:a 1 3] (5 2) (2 1) (3 1 2) (5 0 1) (3 [:a 1]) (0) {:m 1} {:l 1} {:s \
         1} {:q 1} @ @ @ @ @ E:1 is not ISeqable 1 {:a 1} {:a 1, :b 2} {:c 3} E:Map conj \
         requires vector entries {0 0, 1 1, 2 2, 3 3, 4 4, 5 5, 6 6, 7 7, 8 8, :h 1} {:d 4}",
        "#cljs.user.R{:a 1, :b 2} #cljs.user.R{:a 5, :c 6} 2 [:obj 1] {:vk :vv} {:vk :vv} E:No \
         protocol method IIndexed/-nth defined for this value E:No protocol method IIndexed/-nth \
         defined for this value @ 1 TypeError 1 :unexpected 2 {:unexpected :unexpected} 3 \
         {:unexpected :unexpected} 3 TypeError 1 TypeError 1 TypeError 1 (2 1) (2 1) (2 1 0) #{1 \
         3} {:a 2, :b 1} {2 :b, 1 :a} (1 2) [2 3 4] [:a 1 3] [:obj 5] (2 1) [:obj 3] [:obj 5] \
         [:obj 3] (0) {:m 1} {:l 1} {:s 1} {:q 1} [:obj 2] [:obj 2] [:obj 1] [:obj 1] [:obj 1] \
         E:1 is not ISeqable 1 {:a 1} {:a 1, :b 2} {:c 3} E:Map conj requires vector entries {0 \
         0, 1 1, 2 2, 3 3, 4 4, 5 5, 6 6, 7 7, 8 8, :h 1} {:d 4}",
        "#cljs.user.R{:a 1, :b 2} #cljs.user.R{:a 5, :c 6} 2 [:object 1] {:vk :vv} {:vk :vv} \
         E:No protocol method IIndexed/-nth defined for this value E:No protocol method \
         IIndexed/-nth defined for this value [:object 1] 1 TypeError 1 :unexpected 2 \
         {:unexpected :unexpected} 3 {:unexpected :unexpected} 3 TypeError 1 TypeError 1 \
         TypeError 1 (2 1) (2 1) (2 1 0) #{1 3} {:a 2, :b 1} {2 :b, 1 :a} (1 2) [2 3 4] [:a 1 3] \
         (5 2) (2 1) (3 1 2) (5 0 1) (3 [:a 1]) (0) {:m 1} {:l 1} {:s 1} {:q 1} @ [:object 2] @ \
         [:object 1] @ E:1 is not ISeqable 1 {:a 1} {:a 1, :b 2} {:c 3} E:Map conj requires \
         vector entries {0 0, 1 1, 2 2, 3 3, 4 4, 5 5, 6 6, 7 7, 8 8, :h 1} {:d 4}",
        "#cljs.user.R{:a 1, :b 2} #cljs.user.R{:a 5, :c 6} 2 [:default 1] {:vk :vv} {:vk :vv} \
         E:No protocol method IIndexed/-nth defined for this value E:No protocol method \
         IIndexed/-nth defined for this value [:default 1] 1 TypeError 1 :unexpected 2 \
         {:unexpected :unexpected} 3 {:unexpected :unexpected} 3 TypeError 1 TypeError 1 \
         TypeError 1 (2 1) (2 1) (2 1 0) #{1 3} {:a 2, :b 1} {2 :b, 1 :a} (1 2) [2 3 4] [:a 1 3] \
         (5 2) (2 1) (3 1 2) (5 0 1) (3 [:a 1]) (0) {:m 1} {:l 1} {:s 1} {:q 1} [:default 2] \
         [:default 2] [:default 1] [:default 1] [:default 1] E:1 is not ISeqable 1 {:a 1} {:a 1, \
         :b 2} {:c 3} E:Map conj requires vector entries {0 0, 1 1, 2 2, 3 3, 4 4, 5 5, 6 6, 7 \
         7, 8 8, :h 1} {:d 4}",
        "(2 1) TypeError [:byname-sv 4] {:a 1} TypeError #cljs.user.R{:a 1, :b 2} \
         #cljs.user.R{:a 5, :c 6} 2 @ {:vk :vv} {:vk :vv} E:No protocol method IIndexed/-nth \
         defined for this value E:No protocol method IIndexed/-nth defined for this value @ 1 \
         TypeError 1 :unexpected 2 {:unexpected :unexpected} 3 {:unexpected :unexpected} 3 \
         TypeError 1 TypeError 1 TypeError 1 (2 1) (2 1) (2 1 0) TypeError {:a 2, :b 1} {2 :b, 1 \
         :a} (:byname 1 2) [2 3 4] [:a 1 3] (5 2) (2 1) (3 1 2) (5 0 1) (3 [:a 1]) (0) {:m 1} \
         {:l 1} TypeError nil @ @ @ @ @ E:1 is not ISeqable 1 {:a 1} {:a 1, :b 2} {:c 3} E:Map \
         conj requires vector entries {0 0, 1 1, 2 2, 3 3, 4 4, 5 5, 6 6, 7 7, 8 8, :h 1} {:d 4}",
        "#cljs.user.R{:a 1, [:enth 0] [:enth 1]} #cljs.user.R{:a 1, [:nth 0] [:nth 1]} 2 @ {:vk \
         :vv} {:vk :vv} E:No protocol method IIndexed/-nth defined for this value E:No protocol \
         method IIndexed/-nth defined for this value @ 1 TypeError 1 :unexpected 2 {:unexpected \
         :unexpected} 3 {:unexpected :unexpected} 3 TypeError 1 TypeError 1 TypeError 1 (2 1) (2 \
         1) (2 1 0) #{1 3} {[:enth 0] [:enth 1]} {[:nth 0] [:nth 1]} (1 2) [[:nth 1] [:nth 2] 4] \
         [:a 1 3] (5 2) (2 1) (3 1 2) (5 0 1) (3 [:a 1]) (0) {:m 1} {:l 1} {:s 1} {:q 1} @ @ @ @ \
         @ E:1 is not ISeqable 1 {[:nth 0] [:nth 1]} {[:enth 0] [:enth 1], [:nth 0] [:nth 1]} \
         {[:enth 0] [:enth 1]} E:Map conj requires vector entries {0 0, 1 1, 2 2, 3 3, 4 4, 5 5, \
         6 6, 7 7, 8 8, [:nth 0] [:nth 1]} {[:enth 0] [:enth 1]}",
        "{[:s 0] [:s 1], [:se 0] [:se 1]} {[:s 0] [:s 1]} {[:se 0] [:se 1]} {:b 2} \
         #cljs.user.R{:a 1, :b 2} #cljs.user.R{:a 5, :c 6} 2 @ {:vk :vv} {:vk :vv} E:No protocol \
         method IIndexed/-nth defined for this value E:No protocol method IIndexed/-nth defined \
         for this value @ 1 TypeError 1 :unexpected 2 {:unexpected :unexpected} 3 {:unexpected \
         :unexpected} 3 TypeError 1 TypeError 1 TypeError 1 (2 1) (2 1) (2 1 0) #{1 3} {:a 2, :b \
         1} {2 :b, 1 :a} (1 2) [2 3 4] [:a 1 3] (5 2) (2 1) (3 1 2) (5 0 1) (3 [:a 1]) (0) {:m \
         1} {:l 1} {:s 1} {:q 1} @ @ @ @ @ E:1 is not ISeqable 1 {:a 1} {:a 1, :b 2} {:c 3} \
         E:Map conj requires vector entries {0 0, 1 1, 2 2, 3 3, 4 4, 5 5, 6 6, 7 7, 8 8, :h 1} \
         {:d 4}",
        "[:obj 0] #cljs.user.R{:a 1, :b 2} #cljs.user.R{:a 5, :c 6} 2 @ {:vk :vv} {:vk :vv} \
         {[:obj 0] [:obj 1]} {[:obj 0] [:obj 1]} @ 1 TypeError 1 :unexpected 2 {:unexpected \
         :unexpected} 3 {:unexpected :unexpected} 3 TypeError 1 TypeError 1 TypeError 1 (2 1) (2 \
         1) (2 1 0) #{1 3} {:a 2, :b 1} {2 :b, 1 :a} (1 2) [2 3 4] [:a 1 3] (5 2) (2 1) (3 1 2) \
         (5 0 1) (3 [:a 1]) (0) {:m 1} {:l 1} {:s 1} {:q 1} @ @ @ @ @ E:1 is not ISeqable 1 {:a \
         1} {:a 1, :b 2} {:c 3} E:Map conj requires vector entries {0 0, 1 1, 2 2, 3 3, 4 4, 5 \
         5, 6 6, 7 7, 8 8, :h 1} {:d 4}",
        "{:a 1} {:a 1} {[:byname 0] [:byname 1]} {[:byname 0] [:byname 1]} #cljs.user.R{:a 1, :b \
         2} #cljs.user.R{:a 5, :c 6} 2 @ {:vk :vv} {:vk :vv} E:No protocol method IIndexed/-nth \
         defined for this value E:No protocol method IIndexed/-nth defined for this value @ 1 \
         TypeError 1 :unexpected 2 {:unexpected :unexpected} 3 {:unexpected :unexpected} 3 \
         TypeError 1 TypeError 1 TypeError 1 (2 1) (2 1) (2 1 0) #{1 3} TypeError {2 :b, 1 :a} \
         (1 2) [2 3 4] [:a 1 3] (5 2) (2 1) (3 1 2) (5 0 1) (3 [:a 1]) (0) TypeError {:l 1} {:s \
         1} {:q 1} @ @ @ @ @ E:1 is not ISeqable 1 {:a 1} TypeError TypeError E:Map conj \
         requires vector entries {0 0, 1 1, 2 2, 3 3, 4 4, 5 5, 6 6, 7 7, 8 8, :h 1} TypeError",
        "7 #cljs.user.R{:a 1, :b 2} #cljs.user.R{:a 5, :c 6} 2 @ {:vk :vv} {:vk :vv} E:No \
         protocol method IIndexed/-nth defined for this value E:No protocol method IIndexed/-nth \
         defined for this value @ 1 TypeError 1 :unexpected 2 {:unexpected :unexpected} 3 \
         {:unexpected :unexpected} 3 TypeError 1 TypeError 1 TypeError 1 (2 1) (2 1) (2 1 0) #{1 \
         3} {:a 2, :b 1} {2 :b, 1 :a} (1 2) [2 3 4] [:a 1 3] (5 2) (2 1) (3 1 2) (5 0 1) (3 [:a \
         1]) (0) {:m 1} {:l 1} {:s 1} {:q 1} @ @ @ @ @ E:1 is not ISeqable 1 {:a 1} {:a 1, :b 2} \
         {:c 3} E:Map conj requires vector entries {0 0, 1 1, 2 2, 3 3, 4 4, 5 5, 6 6, 7 7, 8 8, \
         :h 1} {:d 4}",
        "(2 1) (2 1) (2 1 0) #{1 3} {:a 2, :b 1} {2 :b, 1 :a} (1 2) [2 3 4] [:a 1 3] (5 2) (2 1) \
         (3 1 2) (5 0 1) (3 [:a 1]) (0) {:m 1} {:l 1} {:s 1} {:q 1} @ @ @ @ @ E:1 is not \
         ISeqable 1 {:a 1} {:a 1, :b 2} {:c 3} E:Map conj requires vector entries {0 0, 1 1, 2 \
         2, 3 3, 4 4, 5 5, 6 6, 7 7, 8 8, :h 1} {:d 4}",
    ]
    .map(|answers| answers.replace('@', none));
    assert_eq!(outputs, expected);
}

/// A map's `-conj` of an entry that `extend-type` of `nil` makes a vector
/// takes `-nth`'s dispatcher, which answers nil from `nil`'s table, as
/// before light-cljs-1076's `entryNth` (the second Sol review): onto an
/// array, a hash and a sorted map, by `conj`, `into` and `merge`.
#[test]
fn map_conj_preserves_nil_vector_extensions() {
    every_level(
        "(extend-type nil IVector IIndexed (-nth ([_ i] (if (= i 0) :k :v)) ([_ i nf] nf)))
         (let [p (fn [f] (try (pr-str (f)) (catch :default e (str \"E:\" (.-message e)))))]
           (pr-str [(p #(conj {} (list nil))) (p #(conj (zipmap (range 9) (range 9)) (list nil)))
                    (p #(conj (sorted-map) (list nil))) (p #(into (sorted-map) [(list nil)]))
                    (p #(merge {:a 1} (list nil))) (p #(conj {} nil))]))",
        r#"["{:k :v}" "{0 0, 1 1, 2 2, 3 3, 4 4, 5 5, 6 6, 7 7, 8 8, :k :v}" "{:k :v}" "{:k :v}" "{:a 1, :k :v}" "{}"]"#,
    );
}

/// A map's `-conj` of a value that `extend-type` makes a vector and whose
/// `-nth` member holds a runtime class's method (a map entry's,
/// `builtinSlots`) answers as `-nth`'s dispatcher, which passes over such a
/// method (the third Sol review of light-cljs-1076): an atom's error, and a
/// vector's own answer through the method its class lent another vector.
#[test]
fn map_conj_preserves_borrowed_builtin_nth_slots() {
    every_level(
        "(extend-type Atom IVector)
         (let [p (fn [f] (try (pr-str (f)) (catch :default e (str \"E:\" (.-message e)))))
               lend (fn [value] (set! (.-cljs$core$IIndexed$_nth$arity$2 value) (.-cljs$core$IIndexed$_nth$arity$2 (first {:k :v}))) value)]
           (pr-str [(p #(conj {} (lend (atom 1)))) (p #(conj (sorted-map) (lend (atom 1)))) (p #(conj {} (lend (vector :a 1))))
                    (p #(into (sorted-map) [(lend (vector :b 2))]))]))",
        r#"["E:No protocol method IIndexed/-nth defined for this value" "E:No protocol method IIndexed/-nth defined for this value" "{:a 1}" "{:b 2}"]"#,
    );
}

/// `into` onto a program type's value whose `-conj` member holds a runtime
/// class's method (a list's, `builtinSlots`) answers as `-conj`'s
/// dispatcher, which passes over such a method (the second Sol review of
/// light-cljs-1076, and the third for an atom, as `empty_preserves_borrowed_builtin_count_slots` for
/// `-count`): the dispatcher's error for a program type, `object`'s method
/// where a program extends `object`, and the class's path for a record and
/// for a runtime class's value that holds another class's method, as
/// before.
#[test]
fn into_preserves_borrowed_builtin_conj_slots() {
    let body = "(deftype T [] ICounted (-count [_] 0))
         (defrecord R [a])
         (let [p (fn [f] (try (pr-str (f)) (catch :default e (str \"E:\" (.-message e)))))
               lend (fn [value] (set! (.-cljs$core$ICollection$_conj$arity$2 value) (.-cljs$core$ICollection$_conj$arity$2 ())) value)]
           (pr-str [(p #(into (lend (T.)) [1])) (p #(into (lend (->R 1)) [[:b 2]])) (p #(into (lend (sorted-set 1)) [2]))
                    (p #(into (lend (T.)) [])) (p #(count (into (->R 1) [[:b 2]]))) (p #(into (lend (atom nil)) [1]))]))";
    every_level(
        body,
        r##"["E:No protocol method ICollection/-conj defined for this value" "([:b 2] [:a 1])" "(2 1)" "#object[cljs.user.T [object Object]]" "2" "E:No protocol method ICollection/-conj defined for this value"]"##,
    );
    every_level(
        &format!("(extend-type object ICollection (-conj [o x] [:object x])) {body}"),
        r##"["[:object 1]" "([:b 2] [:a 1])" "(2 1)" "#object[cljs.user.T [object Object]]" "2" "[:object 1]"]"##,
    );
}

/// The transients' `-conj!`, `-assoc!` and `-persistent!` slots, which
/// `conj!`, `assoc!`, `persistent!`, `into`, `mapv` and `filterv` call
/// before any lookup, and a map transient's `conj!` of a map entry, which
/// reads the entry's `-key` and `-val` slots (light-cljs-1065), keep master
/// `a397e0c9`'s answers at every level. Each program extends the transient
/// protocols of one transient type, of `default` or of `object`, or a map
/// entry's `IMapEntry` or `IIndexed`, and prints, for the transients it can
/// reach, what `into`, `mapv`, `filterv`, `vec`, `conj!`, `assoc!` and
/// `persistent!` give, their errors for a transient no longer editable and
/// for values that are no transients, and the answers of transients
/// `specify!` gave the protocols; a program with protocols and no
/// `extend-type` (`installMethods` alone) prints them all.
#[test]
fn transient_slots_keep_the_answers_of_extended_and_specified_transients() {
    let head = r#"(let [out (array) p (fn [f] (.push out (try (pr-str (f)) (catch :default e (str "E:" (.-message e)))))) big (zipmap (range 20) (range 20))]"#;
    let part = |name: &str| match name {
        "v" => {
            r#"(p #(into [] (range 3))) (p #(into [1] (list 2 3))) (p #(into [] (map inc) [1 2])) (p #(mapv inc [1 2 3])) (p #(filterv odd? (range 5)))
               (p #(vec (list 1 2))) (p #(persistent! (reduce conj! (transient []) [1 2]))) (p #(persistent! (conj! (transient [1]) 2 3)))
               (p #(persistent! (assoc! (transient [1 2]) 0 :x 2 :y))) (p #(persistent! (conj! (transient [])))) (p #(persistent! (conj!)))
               (p #(let [t (transient [])] (persistent! t) (conj! t 1))) (p #(let [t (transient [])] (persistent! t) (persistent! t)))
               (p #(assoc! (transient [1]) :k 2)) (p #(assoc! (transient [1]) 5 2)) (p #(persistent! (-conj! (transient [1]) 2)))
               (p #(conj! (specify! (transient []) ITransientCollection (-conj! [t x] [:sconj x])) 1))
               (p #(reduce conj! (specify! (transient []) ITransientCollection (-conj! [t x] [:sconj x])) [1 2]))
               (p #(let [s (specify! (transient [1]) ITransientCollection (-persistent! [t] :svp))] [(persistent! s) (count s)]))"#
        }
        "s" => {
            r#"(p #(into #{} (range 3))) (p #(into #{9} (keys {:a 1 :b 2}))) (p #(into #{} (map inc) [1 2])) (p #(persistent! (conj! (transient #{}) 1 2)))
               (p #(let [t (transient #{})] (persistent! t) (conj! t 1))) (p #(assoc! (transient #{}) 1 2)) (p #(-persistent! (transient #{1})))
               (p #(let [s (specify! (transient #{}) ITransientCollection (-conj! [t x] [:ssconj x]) (-persistent! [t] :ssp))] [(conj! s 1) (persistent! s)]))"#
        }
        "m" => {
            r#"(p #(into {} [[1 2] [3 4]])) (p #(into {:z 0} (seq {:a 1 :b 2}))) (p #(count (into {} big))) (p #(into {} (filter some?) {:a 1}))
               (p #(persistent! (conj! (transient {}) [:a 1] {:b 2 :c 3}))) (p #(persistent! (assoc! (transient {}) :a 1 :b 2)))
               (p #(let [m (persistent! (assoc! (transient big) 0 :x))] [(count m) (m 0)]))
               (p #(count (persistent! (reduce (fn [t i] (assoc! t i i)) (transient {}) (range 12))))) (p #(persistent! (assoc! (transient {}) :a 1 :b)))
               (p #(let [t (transient {})] (persistent! t) (assoc! t 1 2))) (p #(let [t (transient big)] (persistent! t) (conj! t [1 2])))
               (p #(let [t (transient big)] (persistent! t) (persistent! t))) (p #(persistent! (-assoc! (transient {}) 1 2)))
               (p #(let [s (specify! (transient {}) ITransientAssociative (-assoc! [t k v] [:sassoc k v]))] [(assoc! s 1 2) (conj! s [3 4])]))"#
        }
        _ => {
            r#"(p #(conj! nil 1)) (p #(conj! [1] 2)) (p #(assoc! {} 1 2)) (p #(persistent! [1])) (p #(persistent! nil))"#
        }
    };
    let no_conj = "E:No protocol method ITransientCollection/-conj! defined for this value";
    let no_assoc = "E:No protocol method ITransientAssociative/-assoc! defined for this value";
    let no_persistent =
        "E:No protocol method ITransientCollection/-persistent! defined for this value";
    let vectors = "[0 1 2] [1 2 3] [2 3] [2 3 4] [1 3] [1 2] [1 2] [1 2 3] [:x 2 :y] [] [] E:conj! after persistent! E:persistent! called twice E:TransientVector's key for assoc! must be a number. E:Index out of bounds [1 2] [:sconj 1]";
    let sets = "#{0 1 2} #{9 :a :b} #{2 3} #{1 2} E:conj! after persistent!";
    let maps = "{1 2, 3 4} {:z 0, :a 1, :b 2} 20 {:a 1} {:a 1, :b 2, :c 3} {:a 1, :b 2} [20 :x] 12 {:a 1, :b nil} E:assoc! after persistent! E:conj! after persistent! E:persistent! called twice {1 2} [[:sassoc 1 2] [:sassoc 3 4]]";
    for (extensions, parts, expected) in [
        (
            "(defrecord R [a] ICounted (-count [_] 1)) (count (R. 1))",
            &["v", "s", "m", "e"][..],
            format!(
                "{vectors} {no_conj} [:svp 1] {sets} {no_assoc} #{{1}} [[:ssconj 1] :ssp] {maps} {no_conj} {no_conj} {no_assoc} {no_persistent} {no_persistent}"
            ),
        ),
        (
            "(def log (atom [])) (extend-type TransientVector ITransientCollection (-conj! [t x] (swap! log conj x) t))",
            &["v"],
            format!(
                "[] [1] [] [] [] [] [] [1] [:x 2 :y] [] [] #object[cljs.core.TransientVector] E:persistent! called twice E:TransientVector's key for assoc! must be a number. E:Index out of bounds [1] [:sconj 1] {no_conj} [:svp 1]"
            ),
        ),
        (
            "(extend-type TransientVector ITransientCollection (-persistent! [t] [:vp (count t)]))",
            &["v"],
            format!(
                "[:vp 3] [:vp 3] [:vp 2] [:vp 3] [:vp 2] [:vp 2] [:vp 2] [:vp 3] [:vp 3] [:vp 0] [:vp 0] #object[cljs.core.TransientVector] [:vp 0] E:TransientVector's key for assoc! must be a number. E:Index out of bounds [:vp 2] [:sconj 1] {no_conj} [:svp 1]"
            ),
        ),
        (
            "(extend-type TransientVector ITransientAssociative (-assoc! [t k v] (conj! t [:va k v])))",
            &["v"],
            format!(
                "[0 1 2] [1 2 3] [2 3] [2 3 4] [1 3] [1 2] [1 2] [1 2 3] [1 2 [:va 0 :x] [:va 2 :y]] [] [] E:conj! after persistent! E:persistent! called twice #object[cljs.core.TransientVector] #object[cljs.core.TransientVector] [1 2] [:sconj 1] {no_conj} [:svp 1]"
            ),
        ),
        (
            "(extend-type TransientHashSet ITransientCollection (-conj! [t x] [:hsconj x]) (-persistent! [t] :hsp))",
            &["s"],
            format!("{no_conj} {no_conj} {no_conj} {no_conj} [:hsconj 1] {no_assoc} :hsp [[:ssconj 1] :ssp]"),
        ),
        (
            "(extend-type TransientArrayMap ITransientAssociative (-assoc! [t k v] [:amassoc k v]))",
            &["m"],
            format!(
                "{no_conj} {no_conj} {no_conj} {no_persistent} {no_conj} {no_assoc} [20 :x] {no_assoc} {no_assoc} [:amassoc 1 2] E:conj! after persistent! E:persistent! called twice {no_persistent} [[:sassoc 1 2] [:sassoc 3 4]]"
            ),
        ),
        (
            "(extend-type TransientArrayMap ITransientCollection (-conj! [t x] [:amconj x]))",
            &["m"],
            format!(
                "{no_conj} {no_conj} {no_conj} {no_persistent} {no_conj} {{:a 1, :b 2}} [20 :x] 12 {{:a 1, :b nil}} E:assoc! after persistent! E:conj! after persistent! E:persistent! called twice {{1 2}} [[:sassoc 1 2] [:amconj [3 4]]]"
            ),
        ),
        (
            "(extend-type TransientHashMap ITransientCollection (-persistent! [t] [:hmp (count t)]) (-conj! [t x] [:hmconj x]))",
            &["m"],
            format!(
                "{{1 2, 3 4}} {{:z 0, :a 1, :b 2}} {no_conj} {{:a 1}} {{:a 1, :b 2, :c 3}} {{:a 1, :b 2}} [2 :hmp] 2 {{:a 1, :b nil}} E:assoc! after persistent! [:hmconj [1 2]] [:hmp 20] {{1 2}} [[:sassoc 1 2] [:sassoc 3 4]]"
            ),
        ),
        (
            "(extend-type TransientHashMap ITransientAssociative (-assoc! [t k v] [:hmassoc k]))",
            &["m"],
            format!(
                "{{1 2, 3 4}} {{:z 0, :a 1, :b 2}} {no_conj} {{:a 1}} {{:a 1, :b 2, :c 3}} {{:a 1, :b 2}} {no_persistent} {no_assoc} {{:a 1, :b nil}} E:assoc! after persistent! E:conj! after persistent! E:persistent! called twice {{1 2}} [[:sassoc 1 2] [:sassoc 3 4]]"
            ),
        ),
        (
            "(extend-type MapEntry IMapEntry (-key [e] [:key (nth e 0)]) (-val [e] [:val (nth e 1)]))",
            &["m"],
            "{1 2, 3 4} {:z 0, [:key :a] [:val 1], [:key :b] [:val 2]} 20 {[:key :a] [:val 1]} {:a 1, [:key :b] [:val 2], [:key :c] [:val 3]} {:a 1, :b 2} [20 :x] 12 {:a 1, :b nil} E:assoc! after persistent! E:conj! after persistent! E:persistent! called twice {1 2} [[:sassoc 1 2] [:sassoc 3 4]]".to_owned(),
        ),
        (
            "(extend-type MapEntry IIndexed (-nth ([e i] [:nth i]) ([e i nf] [:nth i nf])))",
            &["m"],
            "{1 2, 3 4} {:z 0, [:nth 0] [:nth 1]} 1 {[:nth 0] [:nth 1]} {:a 1, [:nth 0] [:nth 1]} {:a 1, :b 2} [20 :x] 12 {:a 1, :b nil} E:assoc! after persistent! E:conj! after persistent! E:persistent! called twice {1 2} [[:sassoc 1 2] [:sassoc 3 4]]".to_owned(),
        ),
        (
            "(extend-type default ITransientCollection (-conj! [t x] :dconj) (-persistent! [t] :dp))
             (extend-type default ITransientAssociative (-assoc! [t k v] :dassoc))",
            &["v", "s", "m", "e"],
            format!(
                "{vectors} :dconj [:svp 1] {sets} :dassoc #{{1}} [[:ssconj 1] :ssp] {maps} :dconj :dconj :dassoc :dp :dp"
            ),
        ),
        (
            "(extend-type object ITransientCollection (-conj! [t x] :oconj) (-persistent! [t] :op))",
            &["v", "s", "m", "e"],
            format!(
                "{vectors} :oconj [:svp 1] {sets} {no_assoc} #{{1}} [[:ssconj 1] :ssp] {maps} {no_conj} :oconj {no_assoc} :op {no_persistent}"
            ),
        ),
    ] {
        let body = parts.iter().map(|name| part(name)).collect::<Vec<_>>().join(" ");
        every_level(
            &format!("{extensions} {head} {body} (.join out \" \"))"),
            &expected,
        );
    }
}

/// A map transient's `conj!` of a map entry calls the entry's `-key` before
/// it reads its `-val` (light-cljs-1065, the Sol review), as `keyProtocol`'s
/// call came before `valProtocol`'s lookup: a `-key` that `specify!`s the
/// entry another `-val` has that one called, at every level, as on master
/// `a397e0c9`.
#[test]
fn transient_map_conj_observes_value_method_changes_from_key() {
    every_level(
        "(let [e (specify! (first {:a 1}) IMapEntry (-key [e] (specify! e IMapEntry (-val [_] 99)) :a) (-val [_] 1))]
           (pr-str [(into {} [e]) (persistent! (conj! (transient {}) e))]))",
        r#"[{:a 99} {:a 99}]"#,
    );
}

/// light-cljs-998: once a program keeps `extend-type` or `specify!`, the
/// generic predicates and operations ask the `satisfies` and `implements`
/// hooks, which answer a value of the runtime's payload stores in one step.
/// Each program extends runtime types, host types, `object` or `default`, or
/// writes marked and unmarked slots and a mask bit from JavaScript, then
/// prints, for runtime, host and forged values and values `specify!` gave
/// protocols, the predicates of the `satisfies` hook (`vector?` to `empty?`),
/// those of the `implements` hook (`map-entry?`, `chunked-seq?`, `uuid?`),
/// and `count`, `nth` and `cons`, which also ask the hooks; then what a Proxy
/// was asked, and how many times. The expected values are what master
/// `e379d6c5`'s runtime prints, except the number of times: since
/// light-cljs-1009 a vector or transient vector test of a host value reads
/// its keyed kind (`vectorStore`, `isTransientCell`), where the private
/// brands read nothing, so the Proxy is asked the same names more often,
/// and more again since light-cljs-1010: a view test (`viewOf`) and the
/// slot-first wrappers of `first`, `rest`, `next` and `seq` read it too,
/// and since light-cljs-1011 a keyword or symbol test (`keywordOf`,
/// `symbolOf`), where the `keywords` and `symbols` stores read nothing,
/// and since light-cljs-1029 the tests of lazy seqs, chunked conses, chunks
/// and chunk buffers (`lazyOf` and the others), where their private stores
/// read nothing (about 100 reads more per program). light-cljs-1028 removed
/// the row forged from a vector's prototype (docs/value-layout.md, section
/// 5, item 3), which declares the vector's protocols. light-cljs-1034's
/// `count` reads the keyed kind once more for an array map's field, and
/// light-cljs-1035's once less: `maps` no longer reads it for a sorted map's
/// entries, which no map of the classes has. Since light-cljs-1037 a record
/// and a `reify` are told by their keyed kinds (`recordCells`, `reifyCells`),
/// where their brands read nothing, and `Advanced` output no longer keeps
/// the class brands' prototype walk, so each level asks the same number.
/// Light-cljs-1038's tests of sorted sets, vars, functions with metadata and
/// string writers (`treeSetOf`, `vars`, `metaFunctions`,
/// `stringBufferWriters`) read it where their stores read nothing.
/// Since light-cljs-1015 the Proxy is asked fewer times (842 and 809, where
/// 877 and 837): no kind table reads its symbol key (`kindOps`), and the
/// protocol lookup (`extendedMethod`) probes the payload stores only for a
/// value of a runtime class, which a host value is not. Since light-cljs-1046
/// less than half as many (340 and 345): the store inventories
/// (`ownedProtocolValue` of `satisfies`, `seqableValue` and the other native
/// tests) read a value's keyed kind once and test its class's store
/// (`kindTable`), where they asked every store in turn, each reading it.
/// Since light-cljs-1066 twice more below `Advanced`, and at `Advanced` in
/// the set that writes slots by name: `count` and `get` of the Proxy read
/// its keyed kind for the wrappers that answer a runtime class's value
/// whose prototype slot a program cleared (`slotWrites`). Since
/// light-cljs-1071 once or twice more again there: the seq walk's wrappers
/// read it too, for the Proxy's `seq` and `first` (or `rest`). Since
/// light-cljs-1070 the forged rows answer the predicates from the bits their
/// prototypes declare, and the sorted map of the set that writes keyed names,
/// whose mask the program writes by name, answers the written mask
/// (docs/value-layout.md, section 5, item 35); the Proxy is asked as often.
/// Since light-cljs-1075 `empty?` of the forged list, map and range calls
/// their class's `-count` slot, where it threw, and in the set that extends
/// host types, `object` and `default`, the forged list's and range's bits,
/// which declare `ICounted`, keep `object`'s `-count` from them, as from
/// their class, so `count` of a cons of one throws, where it gave 43 (item
/// 37); the Proxy is asked as often.
#[test]
fn protocol_hooks_answer_runtime_values_as_the_full_lookup() {
    let values = r#"
        (let [log (array)
              handler (js-obj "get" (fn [t k] (.push log (str "get:" (.toString k))) (aget t k))
                              "has" (fn [t k] (.push log (str "has:" (.toString k))) (js-in k t))
                              "getPrototypeOf" (fn [t] (.push log "proto") (js/Object.getPrototypeOf t)))
              proxy (js/Proxy. (js-obj) handler)
              forge (fn [v] (js/Object.create (js/Object.getPrototypeOf v)))
              e (first {:a 1})
              values [[1 2] e (list 1) () (lazy-seq [1]) (cons 1 nil) (range 3) (range 2.5) {:a 1}
                      (zipmap (range 20) (range 20)) (sorted-map :a 1) #{1} (into #{} (range 20)) (sorted-set 1)
                      (conj cljs.core/PersistentQueue.EMPTY 1) (transient []) (subvec [1 2 3] 1) (->R 1)
                      (seq [1 2]) (seq {:a 1}) (seq (sorted-map :a 1)) (rseq [1 2]) (seq (zipmap (range 20) (range 20)))
                      (iterate inc 0) (repeat 1) (cycle [1]) (reduced 1) (atom 1) :k 'sym "s" 1 nil (array 1)
                      (js-obj) inc (uuid "00000000-0000-0000-0000-000000000000") (T.) proxy
                      (forge (list 1)) (forge {:a 1}) (forge (range 3)) (forge (seq [1 2]))
                      cljs.core/PersistentQueue.EMPTY (specify! {:s 1} ISequential IList) (specify! [1] ICounted (-count [_] 5))]
              labels (quote [vector entry list empty-list lazy-seq cons range float-range array-map hash-map sorted-map set hash-set sorted-set queue transient-vector subvec record vector-seq map-seq sorted-map-seq rseq hash-map-seq iterate repeat cycle reduced atom keyword symbol string number null array object fn uuid type proxy forged-list forged-map forged-range forged-seq empty-queue specified-map specified-vector])
              preds [vector? seq? map? set? seqable? coll? sequential? counted? associative? list? sorted?
                     reversible? record? ifn? fn? indexed? reduceable? empty? map-entry? chunked-seq? uuid?]
              attempt (fn [f] (try (f) (catch :default _ :error)))
              row (fn [l v]
                    (str l " " (apply str (map #(case (attempt (fn [] (% v))) :error "e" true 1 0) preds))
                         ;; `count` of an infinite seq would not return.
                         (when-not (#{(quote iterate) (quote repeat) (quote cycle)} l)
                           (str " " (pr-str (attempt #(count v))) " " (pr-str (attempt #(nth v 0 :none)))
                                " " (pr-str (attempt #(count (cons 0 v))))))))]
          (str (.join (into-array (map row labels values)) "\n") "\n"
               (.join (into-array (sort (distinct (array-seq log)))) " ") "\n" (.-length log)))
    "#;
    for (extensions, expected, reads) in [
        (
            // Runtime types extended with protocols they lack, ones they
            // have natively, and a marker protocol; and `specify!`.
            "(extend-type PersistentVector ISeq (-first [_] :vf) (-rest [_] ()))
             (extend-protocol ICounted LazySeq (-count [_] 7) Range (-count [_] 8))
             (extend-type PersistentHashSet IList)
             (extend-type Range IMapEntry (-key [_] 1) (-val [_] 2))
             (extend-type PersistentTreeMap IVector (-assoc-n [_ _ _] nil))
             (extend-type Cons IChunkedSeq (-chunked-first [_] nil) (-chunked-rest [_] ()))
             (extend-type Keyword ISequential)
             (specify! cljs.core/PersistentQueue.EMPTY IMap (-dissoc [q _] q))",
            concat!(
                "vector 110011111001010110000 2 1 3\n",
                "entry 100011111001010110100 2 :a 3\n",
                "list 010011110100000010000 1 1 2\n",
                "empty-list 010011110100000011000 0 :none 1\n",
                "lazy-seq 010011110000000010000 7 :vf 2\n",
                "cons 010011110100000010000 1 1 2\n",
                "range 010011110000000110010 3 0 4\n",
                "float-range 010011110000000010110 8 0 9\n",
                "array-map 001011011000010010000 1 :error 2\n",
                "hash-map 001011011000010010000 20 :error 21\n",
                "sorted-map 101011011011010010000 1 :error 2\n",
                "set 000111010100010000000 1 :error 2\n",
                "hash-set 000111010100010000000 20 :error 21\n",
                "sorted-set 000111010011010000000 1 :error 2\n",
                "queue 010011110000000000000 1 1 2\n",
                "transient-vector 000000010000010101000 0 :none :error\n",
                "subvec 100011111001010110000 2 2 3\n",
                "record 001011011000100000000 1 :error 2\n",
                "vector-seq 010011110001000110000 2 1 3\n",
                "map-seq 010011110000000010000 1 [:a 1] 2\n",
                "sorted-map-seq 010011110000000010000 1 [:a 1] 2\n",
                "rseq 010011100000000010000 2 2 3\n",
                "hash-map-seq 010011100000000010000 20 [0 0] 21\n",
                "iterate 010011100000000010000\n",
                "repeat 010011100000000010000\n",
                "cycle 010011100000000010000\n",
                "reduced 00000000000000000e000 :error :error :error\n",
                "atom 00000000000000000e000 :error :error :error\n",
                "keyword 00000010000001000e000 :error :error :error\n",
                "symbol 00000000000001000e000 :error :error :error\n",
                "string 000010000000000000000 1 \"s\" 2\n",
                "number 00000000000000000e000 :error :error :error\n",
                "null 000010010000000001000 0 :none 1\n",
                "array 000010000000000000000 1 1 2\n",
                "object 00000000000000000e000 :error :error :error\n",
                "fn 00000000000001100e000 :error :error :error\n",
                "uuid 00000000000000000e001 :error :error :error\n",
                "type 00000000000000000e000 :error :error :error\n",
                "proxy 00000000000000000e000 :error :error :error\n",
                "forged-list 010011110100000010000 nil nil :error\n",
                "forged-map 001011011000010010000 nil :error :error\n",
                "forged-range 010011110000000110010 nil :none :error\n",
                "forged-seq 01001110000000000e000 :error :error :error\n",
                "empty-queue 010011110000000001000 0 :none 1\n",
                "specified-map 001011111100010010000 1 [:s 1] 2\n",
                "specified-vector 110011111001010110000 5 1 2\n",
                "get:Symbol(Symbol.iterator) get:Symbol(Symbol.toPrimitive) get:Symbol(Symbol.toStringTag) get:cljs$core$Fn$$LKEY get:cljs$core$IAssociative$$LKEY get:cljs$core$IChunkedSeq$$LKEY get:cljs$core$ICollection$$LKEY get:cljs$core$ICounted$$LKEY get:cljs$core$ICounted$_count$arity$1$LKEY get:cljs$core$IFn$$LKEY get:cljs$core$IIndexed$$LKEY get:cljs$core$IList$$LKEY get:cljs$core$IMap$$LKEY get:cljs$core$IMapEntry$$LKEY get:cljs$core$IRecord$$LKEY get:cljs$core$IReduce$$LKEY get:cljs$core$IReversible$$LKEY get:cljs$core$ISeq$$LKEY get:cljs$core$ISeqable$$LKEY get:cljs$core$ISequential$$LKEY get:cljs$core$ISet$$LKEY get:cljs$core$ISorted$$LKEY get:cljs$core$IUUID$$LKEY get:cljs$core$IVector$$LKEY get:cljs$lang$kind$$LKEY get:cljs$lang$protocol_mask$partition0$$LKEY get:cljs$lang$protocol_mask$partition1$$LKEY get:toString proto\n",
            ),
            [305, 305, 301],
        ),
        (
            // Host types, `object` and `default`, which a runtime value's
            // native implementation outranks and which its being the
            // runtime's own keeps out.
            "(extend-type object ICounted (-count [_] 42) ISeq (-first [_] :of) (-rest [_] ()))
             (extend-type default ISequential IList IVector (-assoc-n [_ _ _] nil))
             (extend-type number IMapEntry (-key [_] :k) (-val [_] :v))
             (extend-type string ICounted (-count [_] -1))
             (extend-type array IMap (-dissoc [a _] a))
             (extend-type function IChunkedSeq (-chunked-first [_] nil) (-chunked-rest [_] ()))",
            concat!(
                "vector 100011111001010110000 2 1 3\n",
                "entry 100011111001010110100 2 :a 3\n",
                "list 010011110100000010000 1 1 2\n",
                "empty-list 010011110100000011000 0 :none 1\n",
                "lazy-seq 010011100000000010000 1 1 2\n",
                "cons 010011110100000010000 1 1 2\n",
                "range 010011110000000110010 3 0 4\n",
                "float-range 010011100000000010010 3 0 4\n",
                "array-map 001011011000010010000 1 :error 2\n",
                "hash-map 001011011000010010000 20 :error 21\n",
                "sorted-map 001011011011010010000 1 :error 2\n",
                "set 000111010000010000000 1 :error 2\n",
                "hash-set 000111010000010000000 20 :error 21\n",
                "sorted-set 000111010011010000000 1 :error 2\n",
                "queue 010011110000000000000 1 1 2\n",
                "transient-vector 000000010000010101000 0 :none :error\n",
                "subvec 100011111001010110000 2 2 3\n",
                "record 001011011000100000000 1 :error 2\n",
                "vector-seq 010011110001000110000 2 1 3\n",
                "map-seq 010011110000000010000 1 [:a 1] 2\n",
                "sorted-map-seq 010011110000000010000 1 [:a 1] 2\n",
                "rseq 010011100000000010000 2 2 3\n",
                "hash-map-seq 010011100000000010000 20 [0 0] 21\n",
                "iterate 010011100000000010000\n",
                "repeat 010011100000000010000\n",
                "cycle 010011100000000010000\n",
                "reduced 00000000000000000e000 42 :error :error\n",
                "atom 00000000000000000e000 42 :error :error\n",
                "keyword 00000000000001000e000 42 :error :error\n",
                "symbol 00000000000001000e000 42 :error :error\n",
                "string 100010110100000000000 1 \"s\" 2\n",
                "number 10000010010000000e000 :error :error :error\n",
                "null 100010110100000001000 0 :none 1\n",
                "array 101010100100000000000 1 1 2\n",
                "object 110000110100000000000 42 :error :error\n",
                "fn 10000010010001100e000 :error :error :error\n",
                "uuid 110000110100000000001 42 :error :error\n",
                "type 110000110100000000000 42 :error :error\n",
                "proxy 110000110100000000000 42 :error :error\n",
                "forged-list 010011110100000010000 nil nil :error\n",
                "forged-map 001011011000010010000 nil :error :error\n",
                "forged-range 010011110000000110010 nil :none :error\n",
                "forged-seq 01001110000000000e000 :error :error :error\n",
                "empty-queue 010011110000000001000 0 :none 1\n",
                "specified-map 001011111100010010000 1 [:s 1] 2\n",
                "specified-vector 100011111001010110000 5 1 2\n",
                "get:Symbol(Symbol.iterator) get:Symbol(Symbol.toPrimitive) get:Symbol(Symbol.toStringTag) get:cljs$core$Fn$$LKEY get:cljs$core$IAssociative$$LKEY get:cljs$core$IChunkedSeq$$LKEY get:cljs$core$ICollection$$LKEY get:cljs$core$ICounted$$LKEY get:cljs$core$ICounted$_count$arity$1$LKEY get:cljs$core$IFn$$LKEY get:cljs$core$IIndexed$$LKEY get:cljs$core$IList$$LKEY get:cljs$core$IMap$$LKEY get:cljs$core$IMapEntry$$LKEY get:cljs$core$IRecord$$LKEY get:cljs$core$IReduce$$LKEY get:cljs$core$IReversible$$LKEY get:cljs$core$ISeq$$LKEY get:cljs$core$ISeqable$$LKEY get:cljs$core$ISequential$$LKEY get:cljs$core$ISet$$LKEY get:cljs$core$ISorted$$LKEY get:cljs$core$IUUID$$LKEY get:cljs$core$IVector$$LKEY get:cljs$lang$kind$$LKEY get:cljs$lang$protocol_mask$partition0$$LKEY get:cljs$lang$protocol_mask$partition1$$LKEY get:toString proto\n",
            ),
            [318, 318, 315],
        ),
        (
            // Slots JavaScript writes on runtime prototypes: a marker with its
            // method, a method without its marker, and a protocol mask bit
            // (ISeq's, partition 0 bit 6).
            "(extend-type T IReduce (-reduce ([_ f] :r) ([_ f init] init)))
             (set! (.. PersistentQueue -prototype -cljs$core$ISeqable$) cljs.core/PROTOCOL_SENTINEL)
             (set! (.. PersistentQueue -prototype -cljs$core$ISeqable$_seq$arity$1) (fn [_] (list :queue)))
             (set! (.. PersistentHashSet -prototype -cljs$core$ICounted$_count$arity$1) (fn [_] :unmarked))
             (set! (.. PersistentArrayMap -prototype -cljs$core$IMapEntry$) cljs.core/PROTOCOL_SENTINEL)
             (set! (.. PersistentTreeMap -prototype -cljs$lang$protocol_mask$partition0$) 64)
             (set! (.. List -prototype -cljs$core$IVector$) cljs.core/PROTOCOL_SENTINEL)",
            concat!(
                "vector 100011111001010110000 2 1 3\n",
                "entry 100011111001010110100 2 :a 3\n",
                "list 110011110100000010000 1 1 2\n",
                "empty-list 010011110100000011000 0 :none 1\n",
                "lazy-seq 010011100000000010000 1 1 2\n",
                "cons 110011110100000010000 1 1 2\n",
                "range 010011110000000110010 3 0 4\n",
                "float-range 010011100000000010010 3 0 4\n",
                "array-map 001011011000010010100 1 :error 2\n",
                "hash-map 001011011000010010000 20 :error 21\n",
                // A sorted map has a keyed kind since light-cljs-1012, so the
                // older lookups ignore the mask bits a program wrote on its
                // prototype (light-cljs-1028), and `nth` walks its seq: a
                // write to a runtime prototype (docs/value-layout.md, section
                // 5, item 23).
                "sorted-map 010010011010010010000 1 [:a 1] 2\n",
                "set 000111010000010000000 :unmarked :error 2\n",
                "hash-set 000111010000010000000 :unmarked :error 21\n",
                "sorted-set 000111010011010000000 1 :error 2\n",
                "queue 010011110000000000000 1 :queue 2\n",
                "transient-vector 000000010000010101000 0 :none :error\n",
                "subvec 100011111001010110000 2 2 3\n",
                "record 001011011000100000000 1 :error 2\n",
                "vector-seq 010011110001000110000 2 1 3\n",
                "map-seq 010011110000000010000 1 [:a 1] 2\n",
                "sorted-map-seq 010011110000000010000 1 [:a 1] 2\n",
                "rseq 010011100000000010000 2 2 3\n",
                "hash-map-seq 010011100000000010000 20 [0 0] 21\n",
                "iterate 010011100000000010000\n",
                "repeat 010011100000000010000\n",
                "cycle 010011100000000010000\n",
                "reduced 00000000000000000e000 :error :error :error\n",
                "atom 00000000000000000e000 :error :error :error\n",
                "keyword 00000000000001000e000 :error :error :error\n",
                "symbol 00000000000001000e000 :error :error :error\n",
                "string 000010000000000000000 1 \"s\" 2\n",
                "number 00000000000000000e000 :error :error :error\n",
                "null 000010010000000001000 0 :none 1\n",
                "array 000010000000000000000 1 1 2\n",
                "object 00000000000000000e000 :error :error :error\n",
                "fn 00000000000001100e000 :error :error :error\n",
                "uuid 00000000000000000e001 :error :error :error\n",
                "type 00000000000000001e000 :error :error :error\n",
                "proxy 00000000000000000e000 :error :error :error\n",
                "forged-list 110011110100000010000 nil nil :error\n",
                "forged-map 001011011000010010100 nil :error :error\n",
                "forged-range 010011110000000110010 nil :none :error\n",
                "forged-seq 01001110000000000e000 :error :error :error\n",
                "empty-queue 010011110000000001000 0 :queue 2\n",
                "specified-map 001011111100010010100 1 [:s 1] 2\n",
                "specified-vector 100011111001010110000 5 1 2\n",
                "get:Symbol(Symbol.iterator) get:Symbol(Symbol.toPrimitive) get:Symbol(Symbol.toStringTag) get:cljs$core$Fn$$LKEY get:cljs$core$IAssociative$$LKEY get:cljs$core$IChunkedSeq$$LKEY get:cljs$core$ICollection$$LKEY get:cljs$core$ICounted$$LKEY get:cljs$core$ICounted$_count$arity$1$LKEY get:cljs$core$IFn$$LKEY get:cljs$core$IIndexed$$LKEY get:cljs$core$IList$$LKEY get:cljs$core$IMap$$LKEY get:cljs$core$IMapEntry$$LKEY get:cljs$core$IRecord$$LKEY get:cljs$core$IReduce$$LKEY get:cljs$core$IReversible$$LKEY get:cljs$core$ISeq$$LKEY get:cljs$core$ISeqable$$LKEY get:cljs$core$ISequential$$LKEY get:cljs$core$ISet$$LKEY get:cljs$core$ISorted$$LKEY get:cljs$core$IUUID$$LKEY get:cljs$core$IVector$$LKEY get:cljs$lang$kind$$LKEY get:cljs$lang$protocol_mask$partition0$$LKEY get:cljs$lang$protocol_mask$partition1$$LKEY get:toString proto\n",
            ),
            [305, 305, 305],
        ),
    ] {
        let source = format!("(deftype T []) (defrecord R [a])\n{extensions}\n{values}");
        let program = program(&source);
        for (level, reads) in OptimizationLevel::ALL.into_iter().zip(reads) {
            let prepared = prepare_application(&program, ApplicationOptions::new(level));
            let js = prepared.emit_js();
            // The Proxy is asked keyed names: the key, normalised
            // (docs/value-layout.md, section 5, item 4), and `Advanced`'s
            // short names written whole, in the order the whole names sort.
            let printed = super::optimization::whole_names(
                &prepared,
                &js,
                &execute(&format!("console.log({js});")),
            );
            let printed = printed
                .lines()
                .map(|line| {
                    let mut reads = line.split(' ').collect::<Vec<_>>();
                    if line.starts_with("get:") {
                        reads.sort();
                    }
                    reads.join(" ") + "\n"
                })
                .collect::<String>();
            assert_eq!(
                printed,
                format!("{expected}{reads}\n"),
                "at {level}: {extensions}"
            );
        }
    }
}

/// The kind lookup of the protocol hooks and `extendedMethod` reads only its
/// own entries: indices a host gives `Array.prototype` after the runtime
/// loads change no answer for host values, transients or the runtime's
/// collections (the Sol review of light-cljs-998). Each row is computed
/// without and then with the pollution.
#[test]
fn protocol_lookups_ignore_inherited_kind_indices() {
    every_level(
        "(extend-type default IVector (-assoc-n [v _ _] v) IMapEntry (-key [_] :k) (-val [_] :v))
         (extend-type object ISeqable (-seq [_] (list :object)))
         (let [row (fn []
                     (pr-str [(vector? 7) (vector? (js-obj)) (vector? \"s\") (map-entry? 7) (seq (js-obj))
                              (vector? [1]) (vector? (transient [])) (map? {:a 1}) (seq? (list 1))
                              (seq [1 2]) (first (list 3)) (count (transient [1]))]))
               clean (row)
               proto (.-prototype js/Array)]
           (dotimes [i 16] (aset proto i 1))
           (let [polluted (try (row) (finally (dotimes [i 16] (js-delete proto i))))]
             (str clean \" \" (= clean polluted))))",
        "[true true true false (:object) true false true true (1 2) 3 1] true",
    );
}

/// A lazy seq's branches realize it without taking the seq of what it
/// realized, which takes an item of a host iterator, so a lazy seq over one
/// gives at every level what the operations' own paths give (the Sol review
/// of light-cljs-237: `first` gave 3 at `Advanced`).
#[test]
fn lazy_branches_preserve_host_iterator_demand() {
    every_level(
        "(let [it (fn [] (.values #js [1 2 3 4 5 6]))]
           (pr-str [(first (lazy-seq (it))) (into [] (rest (lazy-seq (it)))) (into [] (next (lazy-seq (it))))
                    (count (lazy-seq (it))) (into [] (seq (lazy-seq (it)))) (first (lazy-seq (lazy-seq (it))))
                    (into [] (next (cons 0 (lazy-seq (it)))))]))",
        "[2 [3 4 5 6] [3 4 5 6] 6 [1 2 3 4 5 6] 2 [1 2 3 4 5 6]]",
    );
}

/// The seqs over vectors and arrays and rest arguments answer from keyed
/// slots since light-cljs-1010, and an `extend-type` of one of those
/// protocols still answers as master `cf613766` did: a chunked seq's `next`
/// is the seq of its `rest`, which the program's `-rest` gives, an indexed
/// seq's `next` advances without its `-rest`, and a program's `-count` of
/// `IndexedSeq` counts rest arguments too.
#[test]
fn seq_slots_keep_the_answers_of_extended_protocols() {
    every_level(
        "(extend-type ChunkedSeq ISeq (-rest [_] (list :r)))
         (extend-type IndexedSeq ICounted (-count [_] 9))
         (let [s (seq (vec (range 40))) t (seq [1 2 3])]
           (pr-str [(first s) (rest s) (next s) (first (next t)) (count (rest t))
                    ((fn [& xs] (count xs)) 1 2) (seq? (rest t))]))",
        "[0 (:r) (:r) 2 9 9 true]",
    );
    every_level(
        "(let [s (seq (vec (range 40))) t (seq (to-array [1 2 3]))]
           (pr-str [(first s) (first (next s)) (count (rest s)) (first (drop 33 s)) (reduce + (next s))
                    (first (rest t)) (next (next (next t))) (count t) ((fn [a & xs] [a xs (count xs)]) 1 2 3)
                    (apply + 1 [2 3]) (vec (rest (seq \"abc\"))) (meta (with-meta (next s) {:m 1}))]))",
        "[0 1 39 33 780 2 nil 3 [1 (2 3) 2] 6 [\"b\" \"c\"] {:m 1}]",
    );
    // `clone` of a seq class value builds one of its class at its offset
    // with its metadata, and the public `ChunkedSeq` constructor keeps an
    // undefined metadata or hash as given once either is supplied (the Sol
    // review of light-cljs-1010); the answers are master `cf613766`'s.
    every_level(
        "(let [s (with-meta (rest (seq [1 2 3])) {:m 1}) c (clone s) r (rseq [1 2 3]) rc (clone (next r))
               v (vec (range 40))
               cs (ChunkedSeq. v (array 0 1) 0 0 js/undefined 47)
               ch (ChunkedSeq. v (array 0 1) 0 0 {:k 1} js/undefined)]
           (pr-str [(identical? s c) (= s c) (meta c) (count c) (= (type s) (type c))
                    (vec rc) (= (type r) (type rc)) (identical? (next r) rc)
                    (undefined? (meta cs)) (.-__hash cs) (undefined? (.-__hash ch)) (meta ch)]))",
        "[false true {:m 1} 2 true [2 1] true false true 47 true {:k 1}]",
    );
}

/// The lazy seqs, chunked conses, chunks and a vector's chunked seq answer
/// from keyed slots since light-cljs-1029, which the chunk operations
/// (`chunkFirst`, `chunkRest`, `chunkNext`, `chunkDropFirst`) call first, and an
/// `extend-type` or `specify!` of their protocols still answers as master
/// `f91e0e62` did, in the operations and in a lazy pipeline over them (the
/// Sol review of light-cljs-1029, finding 3).
#[test]
fn chunk_slots_keep_the_answers_of_extended_protocols() {
    every_level(
        "(extend-type ChunkedCons
           IChunkedSeq (-chunked-first [_] (array-chunk (array 10 20))) (-chunked-rest [_] (list :rest))
           IChunkedNext (-chunked-next [_] (list :next)))
         (let [cc (seq (map identity (vec (range 40))))]
           (pr-str [(chunked-seq? cc) (vec (chunk-first cc)) (chunk-rest cc) (chunk-next cc)
                    (vec (map identity cc)) (first cc) (vec (take 3 (filter some? cc)))]))",
        "[true [10 20] (:rest) (:next) [10 20 :rest] 0 [10 20 :rest]]",
    );
    every_level(
        "(extend-type ArrayChunk ICounted (-count [_] 2)
           IIndexed (-nth ([_ i] (* 100 i)) ([_ i nf] (* 10 i)))
           IChunk (-drop-first [_] (array-chunk (array :d))))
         (let [c (array-chunk (array 1 2 3)) cc (chunk-cons c nil)]
           (pr-str [(count c) (nth c 1) (nth c 5 :nf) (vec (-drop-first c)) (first cc) (first (rest cc))
                    (vec (take 5 (map identity (vec (range 40)))))]))",
        "[2 100 50 [:d] 0 0 [0 0 0 0 0]]",
    );
    every_level(
        "(let [sp (specify! (seq (map identity (vec (range 40))))
                    IChunkedSeq (-chunked-first [_] (array-chunk (array :s))) (-chunked-rest [_] ())
                    ISeq (-first [_] :sf) (-rest [_] ()))
               cc (seq (map identity (vec (range 40))))]
           (pr-str [(vec (chunk-first sp)) (chunk-rest sp) (first sp) (rest sp) (vec (map identity sp))
                    (first cc) (count (chunk-first cc))]))",
        "[[:s] () :sf () [:s] 0 32]",
    );
    every_level(
        "(extend-type ChunkedSeq IChunkedSeq (-chunked-first [_] (array-chunk (array :x :y))) (-chunked-rest [_] ())
           IChunkedNext (-chunked-next [_] (list :n)))
         (let [s (seq (vec (range 40)))]
           (pr-str [(vec (map identity (vec (range 40)))) (vec (chunk-first s)) (chunk-rest s) (chunk-next s)
                    (vec (filter keyword? (vec (range 40)))) (first s)]))",
        "[[:x :y] [:x :y] () (:n) [:x :y] 0]",
    );
    every_level(
        "(extend-type LazySeq ISeqable (-seq [_] (list :q)))
         (let [l (map inc [1 2 3])]
           (pr-str [(first l) (rest l) (next l) (seq l) (count (map inc [1 2]))]))",
        "[nil () nil (:q) 1]",
    );
}

/// An `ISeqable` marker and method that JavaScript writes on a runtime seq
/// give what the operations' own paths give where protocol implementations
/// exist, as the branches look for what those paths would find there
/// (light-cljs-358; they gave every value to the paths before): a value that
/// is already a seq is not asked for its seq, as upstream's is not. Expected
/// values are master's at `none`.
#[test]
fn direct_seq_slots_give_what_the_operations_paths_give() {
    every_level(
        "(deftype T [])
         (let [s (list 1 2 3) v (seq [4 5]) l (map inc [1 2])]
           (set! (.-cljs$core$ISeqable$ s) cljs.core/PROTOCOL_SENTINEL)
           (set! (.-cljs$core$ISeqable$_seq$arity$1 s) (fn [_] (list 9 8)))
           (set! (.-cljs$core$ISeqable$ v) cljs.core/PROTOCOL_SENTINEL)
           (set! (.-cljs$core$ISeqable$_seq$arity$1 v) (fn [_] (list 7)))
           (set! (.-cljs$core$ISeqable$ l) cljs.core/PROTOCOL_SENTINEL)
           (set! (.-cljs$core$ISeqable$_seq$arity$1 l) (fn [_] (list 6)))
           (pr-str [(first s) (rest s) (count s) (first v) (rest v) (count v) (first l) (rest l) (some? (T.))]))",
        "[1 (2 3) 3 4 (5) 2 nil () true]",
    );
}

/// Vectors, maps, sets and queues are kinds 4 to 7 of the kind tables
/// (light-cljs-358): `seq` and `count` read their branches, `nth` a vector's,
/// `get` a vector's and a map's, `conj` every collection's and `assoc` a
/// vector's and a map's. Every collection, including those the branches
/// leave to the operations (public constructors' values with slots,
/// subvectors, map entries, sorted collections), answers at every level as
/// master's runtime does at `none`, metadata included. A program calling
/// `get` on a map and `nth` on a vector keeps those branches. Sets' `get` and
/// map entries, kind 8 with branches of `seq`, `count` and `nth`, are
/// light-cljs-373's.
#[test]
fn collection_operations_read_the_kind_tables() {
    every_level(
        "(let [m {:a 1 :b 2} hm (zipmap (range 20) (range 20)) sm (sorted-map :b 2 :a 1) s #{1 2} ss (sorted-set 3 1)
               q (into cljs.core/PersistentQueue.EMPTY [1 2]) v [1 2 3] sv (subvec [1 2 3 4] 1 3) e (first {:k :v})
               big (vec (range 40)) wm (with-meta [1] {:m 1}) ws (with-meta #{1} {:m 2}) wq (with-meta q {:m 3})
               wmap (with-meta {:a 1} {:m 4}) pv (cljs.core/PersistentVector. nil 2 5 (.-EMPTY-NODE cljs.core/PersistentVector) (array 7 8) nil)
               pm (cljs.core/PersistentArrayMap. nil 1 (array :x 9) nil) ps (cljs.core/PersistentHashSet. nil (hash-map 4 nil) nil)
               pq (cljs.core/PersistentQueue. nil 1 (list 5) nil nil)]
           (pr-str [(mapv count [m hm sm s ss q v sv e big pv pm ps pq [] {} #{}])
                    (mapv seq [m sm s ss q v sv [] {} #{} pv pm ps pq])
                    (take 3 (seq hm)) (take 3 (seq big))
                    [(nth v 0) (nth v 5 :nf) (nth sv 1) (nth e 1) (nth v 1.5) (nth big 39) (nth pv 1) (nth v -1 :neg)
                     (try (nth v 9) (catch :default x :oob)) (try (nth e 2) (catch :default x :oob-e)) (nth e 2 :nf-e)]
                    [(get v 1) (get v 7 :nf) (get v -1) (get v 1.5) (get m :a) (get m :z :nf) (get hm 7) (get hm 99 :nf)
                     (get sm :a) (get sm :q :nf) (get sv 0) (get e 0) (get e 1 :x) (get s 1) (get ss 3) (get pv 0) (get pm :x) (get v nil) (get m nil)]
                    [(conj v 4) (conj m [:c 3]) (conj s 3) (conj s 1) (conj ss 2) (seq (conj q 3)) (conj sv 9) (conj e 1)
                     (conj sm [:c 3]) (conj m {:d 4}) (conj pv 9) (conj pm [:y 1]) (conj ps 5) (seq (conj pq 6)) (count (conj big 1))
                     (conj [] 1) (conj {} [1 2]) (conj #{} nil)]
                    [(assoc v 0 :x) (assoc v 3 :y) (assoc m :c 3) (count (assoc hm 99 1)) (assoc sv 0 :z) (assoc sm :c 3)
                     (assoc pv 0 :p) (assoc pm :x 1) (assoc e 0 :k2) (assoc {} :a 1) (assoc [] 0 1)
                     (try (assoc v 9 1) (catch :default x :oob)) (try (assoc v :k 1) (catch :default x (.-message x)))]
                    (mapv meta [(conj wm 2) (conj ws 3) (conj wq 4) (conj wmap [:b 2]) (assoc wm 0 1) (assoc wmap :c 1)])
                    [(contains? m :a) (contains? m :z) (contains? v 1) (contains? hm 3) (contains? s 2) (contains? sm :b)]]))",
        r#"[[2 20 2 2 2 2 3 2 2 40 2 1 1 1 0 0 0] [([:a 1] [:b 2]) ([:a 1] [:b 2]) (1 2) (1 3) (1 2) (1 2 3) (2 3) nil nil nil (7 8) ([:x 9]) (4) (5)] ([0 0] [1 1] [2 2]) (0 1 2) [1 :nf 3 :v 2 39 8 :neg :oob :oob-e :nf-e] [2 :nf nil 2 1 :nf 7 :nf 1 :nf 2 :k :v 1 3 7 9 nil nil] [[1 2 3 4] {:a 1, :b 2, :c 3} #{1 2 3} #{1 2} #{1 2 3} (1 2 3) [2 3 9] [:k :v 1] {:a 1, :b 2, :c 3} {:a 1, :b 2, :d 4} [7 8 9] {:x 9, :y 1} #{4 5} (5 6) 41 [1] {1 2} #{nil}] [[:x 2 3] [1 2 3 :y] {:a 1, :b 2, :c 3} 21 [:z 3] {:a 1, :b 2, :c 3} [:p 8] {:x 1} [:k2 :v] {:a 1} [1] :oob "Vector key must be a number"] [{:m 1} {:m 2} {:m 3} {:m 4} {:m 1} {:m 4}] [true false true true true true]]"#,
    );
    let kept = units(
        "(let [m (zipmap (range 9) (range 9)) v (vec (range 3)) q (into cljs.core/PersistentQueue.EMPTY [1])]
           (js/console.log (get m 1) (nth v 1) (count v) (conj v 1) (assoc m 1 2) (seq #{1}) (count q)))",
    );
    for unit in [
        "PersistentVectorValue.type",
        "PersistentHashMapValue.type",
        "get.trie",
    ] {
        assert!(kept.contains(&unit), "{unit}: {kept:?}");
    }
    // Sets' `get` and map entries, kind 8, read the tables too
    // (light-cljs-373): sets of every representation (arrays, a hash set's
    // trie, sorted, a public constructor's, with nil and with a member
    // equal to the key but not identical to it) and entries of an array map
    // and a hash map, each operation's indexes and keys out of range
    // included.
    every_level(
        "(let [m {:k :v} e (first m) he (first (zipmap (range 20) (range 20))) s #{1 2 nil} hs (set (range 20)) ss (sorted-set 3 1)
               ps (cljs.core/PersistentHashSet. nil (hash-map 4 nil) nil) ks #{[1 2]} hk (into #{[1 2]} (range 12)) ws (with-meta #{:a} {:m 1})]
           (pr-str [[(get s 1) (get s 3) (get s 3 :nf) (get s nil) (get s nil :nf) (get hs 7) (get hs 99) (get hs 99 :nf) (get hs nil :nf)
                     (get ss 1) (get ss 2 :nf) (get ps 4) (get ps 5 :nf) (identical? (get ks (list 1 2)) (first ks))
                     (identical? (get hk (list 1 2)) (first (filter vector? hk))) (get ws :a) (contains? s nil) (contains? hs 3) (s 2) (hs 5 :nf) (:a ws) (get #{} 1 :e)]
                    [(nth e 0) (nth e 1) (nth e 1 :nf) (nth e 2 :nf) (nth e -1 :nf) (nth e 0.5 :nf) (try (nth e 2) (catch :default x :oob))
                     (get e 0) (get e 1) (get e 2) (get e 2 :nf) (get e nil :nf) (get e 0.0) (get e :k :nf) (count e) (seq e) (nth he 1) (get he 0)
                     (key e) (val e) (e 0) (e 1 :x) (vector? e) (map-entry? e) (= e [:k :v])]]))",
        "[[1 nil :nf nil nil 7 nil :nf :nf 1 :nf 4 :nf true true :a true true 2 5 :a :e] \
         [:k :v :v :nf :nf :nf :oob :k :v nil :nf :nf :k :nf 2 (:k :v) 0 0 :k :v :k :v true true true]]",
    );
    // A program that builds map entries and no vector keeps the entries'
    // branches and none of the vectors'.
    let lean = "(let [e (first {:k :v}) f (first (seq (hash-map 1 2 3 4)))]
           (str (nth e 0) \" \" (nth e 1 :nf) \" \" (nth e 2 :nf) \" \" (get e 1) \" \" (get e 2 :nf) \" \" (count e) \" \" (pr-str (seq e)) \" \"
                (nth f 0) \" \" (count f) \" \" (try (nth e 5) (catch :default x \"oob\")) \" \" (get (hash-set :a (key e)) :k) \" \" (get (hash-set (val e)) :z :nf)))";
    every_level(lean, ":k :v :nf :v :nf 2 (:k :v) 1 2 oob :k :nf");
    let kept = units(lean);
    for unit in ["MapEntryType.lean", "MapEntryType.type"] {
        assert!(kept.contains(&unit), "{unit}: {kept:?}");
    }
    for unit in ["PersistentVectorValue", "nth.vectors", "get.vectors"] {
        assert!(!kept.contains(&unit), "{unit}: {kept:?}");
    }
    assert!(units("(get (set (range 20)) 3)").contains(&"get.members"));
    // `first`, `rest` and `next` of every collection kind read the tables
    // too (light-cljs-375): vectors of every size and representation (a
    // subvector, one a transient built, the public constructor's, with
    // metadata), map entries, maps, sets and queues, empty ones included,
    // and the types and metadata of the seqs they give.
    let seqs = "(let [v [1 2 3] v1 [7] v0 [] big (vec (range 40)) sv (subvec [1 2 3 4] 1 3) e (first {:k :v}) m {:a 1 :b 2}
               hm (zipmap (range 20) (range 20)) sm (sorted-map :b 2 :a 1) s #{1 2} ss (sorted-set 3 1) hs (set (range 20))
               q (into cljs.core/PersistentQueue.EMPTY [1 2]) q1 (conj cljs.core/PersistentQueue.EMPTY 5)
               wv (with-meta [1 2] {:m 1}) wq (with-meta q {:m 3})
               pv (cljs.core/PersistentVector. nil 2 5 (.-EMPTY-NODE cljs.core/PersistentVector) (array 7 8) nil)
               pq (cljs.core/PersistentQueue. nil 1 (list 5) nil nil) tv (persistent! (conj! (transient [1]) 2))
               sbig (subvec big 1 39) es (first (zipmap (range 20) (range 20)))]
           (pr-str [(mapv first [v v1 v0 big sv e m hm sm s ss hs q q1 wv pv pq tv sbig es {} #{} cljs.core/PersistentQueue.EMPTY])
                    (mapv rest [v v1 v0 sv e m sm s ss q q1 wv pv pq tv es {} #{} cljs.core/PersistentQueue.EMPTY])
                    (mapv next [v v1 v0 sv e m sm s ss q q1 wv pv pq tv es {} #{} cljs.core/PersistentQueue.EMPTY])
                    (take 3 (rest big)) (take 3 (next big)) (first (rest big)) (first (next (next big))) (count (rest big))
                    (count (next hm)) (first (rest hm)) (first (rest hs)) (count (next sbig)) (first (rest sbig))
                    (mapv meta [(rest v) (rest q) (next q) (rest wq) (next wq) (rest wv) (next wv)])
                    [(instance? cljs.core/IndexedSeq (rest v)) (instance? cljs.core/ChunkedSeq (rest big)) (instance? cljs.core/ChunkedSeq (next big))
                     (chunked-seq? (rest big)) (seq? (rest v)) (identical? (rest v0) ()) (seq? (next sv))]
                    (let [[a & r] v [b & r2] big [k vv & r3] e] [a r b (count r2) k vv r3])]))";
    every_level(
        seqs,
        "[[1 7 nil 0 2 :k [:a 1] [0 0] [:a 1] 1 1 0 1 5 1 7 5 1 1 0 nil nil nil] \
         [(2 3) () () (3) (:v) ([:b 2]) ([:b 2]) (2) (3) (2) () (2) (8) () (2) (0) () () ()] \
         [(2 3) nil nil (3) (:v) ([:b 2]) ([:b 2]) (2) (3) (2) nil (2) (8) nil (2) (0) nil nil nil] \
         (1 2 3) (1 2 3) 1 2 39 19 [1 1] 1 37 2 [nil nil nil nil nil nil nil] [true true true true true true true] \
         [1 (2 3) 0 39 :k :v nil]]",
    );
    // A subvector of, and an `assoc` onto, a vector the public constructor
    // built over a caller's array, which then shrinks, answer as their seqs
    // do (Sol review of light-cljs-375).
    every_level(
        "(let [a (array 7 8 9) sv (subvec (cljs.core/PersistentVector. nil 3 5 (.-EMPTY-NODE cljs.core/PersistentVector) a nil) 0 3)
               av (assoc (cljs.core/PersistentVector. nil 2 5 (.-EMPTY-NODE cljs.core/PersistentVector) a nil) 0 1)]
           (set! (.-length a) 0)
           (pr-str [(first sv) (identical? (rest sv) ()) (next sv) (first av) (rest av) (next av)]))",
        "[nil true nil 1 (8 9) (8 9)]",
    );
    let kept = units(seqs);
    assert!(
        !kept.iter().any(|unit| unit.ends_with(".branches")
            || unit.ends_with(".cells")
            || unit.ends_with(".views")
            || unit.ends_with(".viewKind")
            || *unit == "kindTables"),
        "{kept:?}"
    );
}

/// Where protocol implementations exist, the collections' branches make the
/// protocol lookups their operations make first, and the cells' and lazy
/// seqs' branches look for the implementations that their operations' paths
/// would find (light-cljs-358, where they gave every value to those paths
/// before): methods that JavaScript writes on runtime values are called as
/// often, and give what, they did. Records and program types keep their own
/// methods. Expected values are master's at `none`; at `advanced`, master
/// and this runtime alike ignore an `ILookup` that JavaScript gives a vector
/// or a map, which the second program therefore runs at `none` and
/// `prepare` only.
#[test]
fn collection_branches_find_what_javascript_gives_runtime_values() {
    let common = "(defrecord R [a])
         (deftype T [] ILookup (-lookup [_ k] [:t k]) (-lookup [_ k nf] [:t k nf]))
         (let [calls (atom [])
               note (fn [tag] (swap! calls conj tag))
               v [1 2 3] m {:a 1} s #{1} q (into cljs.core/PersistentQueue.EMPTY [1])
               lv (list 1 2) lz (lazy-seq lv) c (cons 0 (list 1 2)) cl (cons 0 (lazy-seq (list 5 6)))
               inner (list 3 4) lz2 (lazy-seq inner) lz3 (lazy-seq (list 7 8))]
           LOOKUPS
           (set! (.-cljs$core$ICollection$_conj$arity$2 s) (fn [_ x] (note :s-conj) [:sc x]))
           (set! (.-cljs$core$ICollection$_conj$arity$2 q) (fn [_ x] (note :q-conj) [:qc x]))
           (set! (.-cljs$core$IAssociative$_assoc$arity$3 m) (fn [_ k x] (note :m-assoc) [:ma k x]))
           (set! (.-cljs$core$ISeqable$ inner) cljs.core/PROTOCOL_SENTINEL)
           (set! (.-cljs$core$ISeqable$_seq$arity$1 inner) (fn [_] (note :inner-seq) (list 9)))
           (set! (.-cljs$core$ISeq$ c) cljs.core/PROTOCOL_SENTINEL)
           (set! (.-cljs$core$ISeq$_rest$arity$1 c) (fn [_] (note :c-rest) (list :r)))
           (set! (.-cljs$core$ISeqable$ lz3) cljs.core/PROTOCOL_SENTINEL)
           (set! (.-cljs$core$ISeqable$_seq$arity$1 lz3) (fn [_] (note :lz3-seq) (list :z)))
           (let [results [(get v 0) (get v 0 :nf) (nth v 0) (get m :a) (get m :a :nf) (conj v 4) (conj m [:b 2])
                          (conj s 2) (conj q 2) (assoc m :b 1) (assoc v 0 :x) (count v) (count m) (count s) (count q)
                          (first lz) (rest lz) (next lz) (count lz) (seq lz)
                          (first lz2) (rest lz2) (next lz2) (seq lz2) (count lz2)
                          (next c) (next cl) (first cl) (rest cl)
                          (first lz3) (rest lz3) (next lz3) (seq lz3)
                          (get (->R 1) :a) (get (T.) :k) (get (T.) :k :nf) (contains? (->R 1) :a) (conj (->R 1) [:b 2])
                          (assoc (->R 1) :a 2) (count (->R 1)) (seq (->R 1)) (get (assoc (->R 1) :z 3) :z)]]
             (pr-str [results @calls])))";
    let tail = "[:x 2 3] 3 1 1 1 1 (2) (2) 2 (1 2) 3 (4) (4) (9) 1 (:r) (5 6) 0 (5 6) nil () nil (:z) 1 \
         [:t :k] [:t :k :nf] true #cljs.user.R{:a 1, :b 2} #cljs.user.R{:a 2} 1 ([:a 1]) 3]";
    let calls = ":m-assoc :s-conj :q-conj :m-assoc :inner-seq :inner-seq :inner-seq :inner-seq :inner-seq \
         :c-rest :lz3-seq :lz3-seq :lz3-seq :lz3-seq]]";
    every_level(
        &common.replace("LOOKUPS", ""),
        &format!("[[1 1 1 1 1 [1 2 3 4] [:ma :b 2] [:sc 2] [:qc 2] [:ma :b 1] {tail} [{calls}"),
    );
    let lookups = "(set! (.-cljs$core$ILookup$ v) cljs.core/PROTOCOL_SENTINEL)
           (set! (.-cljs$core$ILookup$_lookup$arity$2 v) (fn [_ k] (note :v-lookup) [:vl k]))
           (set! (.-cljs$core$ILookup$_lookup$arity$3 v) (fn [_ k nf] (note :v-lookup3) [:vl k nf]))
           (set! (.-cljs$core$ILookup$ m) cljs.core/PROTOCOL_SENTINEL)
           (set! (.-cljs$core$ILookup$_lookup$arity$2 m) (fn [_ k] (note :m-lookup) [:ml k]))";
    let source = common.replace("LOOKUPS", lookups);
    let program = program(&source);
    for level in [OptimizationLevel::None, OptimizationLevel::Prepare] {
        let js = prepare_application(&program, ApplicationOptions::new(level)).emit_js();
        assert_eq!(
            execute(&format!("console.log({js});")),
            format!(
                "[[[:vl 0] [:vl 0 :nf] 1 [:ml :a] 1 [1 2 3 4] [:ma :b 2] [:sc 2] [:qc 2] [:ma :b 1] {tail} \
                 [:v-lookup :v-lookup3 :m-lookup {calls}\n"
            ),
            "at {level}"
        );
    }
    // The collections' `first`, `rest` and `next` (light-cljs-375) call a
    // `-seq` that JavaScript gives a vector, an entry, a map, a set, a queue
    // or a subvector once each, and the `ISeq` methods it gives a vector
    // where the operations find them; at `advanced`, master and this runtime
    // alike give `first` of the let-bound vectors their items.
    let seqs = "(deftype T [] ISeqable (-seq [_] (list :t)))
         (let [calls (atom [])
               note (fn [tag] (swap! calls conj tag))
               v [1 2 3] big (vec (range 40)) e (first {:k :v}) m {:a 1} s #{1} q (into cljs.core/PersistentQueue.EMPTY [1 2])
               sv (subvec [1 2 3 4] 1 3) wv [4 5] pv (cljs.core/PersistentVector. nil 2 5 (.-EMPTY-NODE cljs.core/PersistentVector) (array 7 8) nil)]
           (doseq [[x tag] [[v :v] [e :e] [m :m] [s :s] [q :q] [sv :sv]]]
             (set! (.-cljs$core$ISeqable$ x) cljs.core/PROTOCOL_SENTINEL)
             (set! (.-cljs$core$ISeqable$_seq$arity$1 x) (fn [_] (note tag) (list tag 2))))
           (set! (.-cljs$core$ISeq$ wv) cljs.core/PROTOCOL_SENTINEL)
           (set! (.-cljs$core$ISeq$_rest$arity$1 wv) (fn [_] (note :wv-rest) (list :r)))
           (set! (.-cljs$core$ISeq$_first$arity$1 wv) (fn [_] (note :wv-first) :f))
           (pr-str [[(first v) (rest v) (next v) (first e) (rest e) (next e) (first m) (rest m) (next m) (first s) (rest s) (next s)
                     (first q) (rest q) (next q) (first sv) (rest sv) (next sv)]
                    [(first wv) (rest wv) (next wv) (first big) (first (rest big)) (count (next big)) (first pv) (rest pv) (next pv) (first (T.))
                     (first [9]) (rest [9]) (next [9]) (next [8 9]) (first []) (next [])]
                    @calls]))";
    let seq_program = self::program(seqs);
    for level in [OptimizationLevel::None, OptimizationLevel::Prepare] {
        let js = prepare_application(&seq_program, ApplicationOptions::new(level)).emit_js();
        assert_eq!(
            execute(&format!("console.log({js});")),
            "[[:v (2) (2) :e (2) (2) :m (2) (2) :s (2) (2) :q (2) (2) :sv (2) (2)] \
             [:f (:r) (:r) 0 1 39 7 (8) (8) :t 9 () nil (9) nil nil] \
             [:v :v :v :e :e :e :m :m :m :s :s :s :q :q :q :sv :sv :sv :wv-first :wv-rest :wv-rest]]\n",
            "at {level}"
        );
    }
    let js = prepare_application(
        &seq_program,
        ApplicationOptions::new(OptimizationLevel::Advanced),
    )
    .emit_js();
    assert_eq!(
        execute(&format!("console.log({js});")),
        "[[1 (2) (2) :e (2) (2) :m (2) (2) :s (2) (2) :q (2) (2) :sv (2) (2)] \
         [4 (:r) (:r) 0 1 39 7 (8) (8) :t 9 () nil (9) nil nil] \
         [:v :v :e :e :e :m :m :m :s :s :s :q :q :q :sv :sv :sv :wv-rest :wv-rest]]\n"
    );
}

/// `extend-type` of a collection type and `specify!` of a collection empty
/// the kind tables, as for the seq kinds: the operations then find those
/// implementations, and the other collections answer as before. Expected
/// values are master's at `none`.
#[test]
fn extensions_reaching_the_collection_kinds_empty_the_tables() {
    every_level(
        "(extend-type PersistentVector
           ICounted (-count [_] 7)
           ILookup (-lookup ([_ k] [:l k]) ([_ k nf] [:l k nf]))
           IIndexed (-nth ([_ i] [:n i]) ([_ i nf] [:n i nf]))
           ICollection (-conj [_ x] [:c x])
           IAssociative (-assoc [_ k x] [:a k x]) (-contains-key? [_ k] :ck))
         (let [sm (specify! {:a 1} ILookup (-lookup ([_ k] [:sl k]) ([_ k nf] [:sl k nf])) ICounted (-count [_] 11))
               ss (specify! #{1} ICollection (-conj [_ x] [:sc x]))]
           (pr-str [(count [1]) (get [1] 0) (get [1] 0 :nf) (nth [1] 0) (nth [1] 0 :nf) (conj [1] 2) (assoc [1] 0 2)
                    (get sm :a) (count sm) (conj ss 2) (get {:b 2} :b) (count {:b 2}) (conj #{5} 2) (assoc {:b 2} :c 3)
                    (count (list 1 2)) (first (map inc [1])) (seq #{3}) (count (subvec [1 2 3] 1))]))",
        "[7 [:l 0] [:l 0 :nf] [:n 0] [:n 0 :nf] [:c 2] [:a 0 2] [:sl :a] 11 [:sc 2] 2 1 #{5 2} {:b 2, :c 3} 2 2 (3) 6]",
    );
}

#[test]
fn small_maps_keep_their_key_tests_entries_and_hashes() {
    // light-cljs-989 made array-map entries, their search and map entries
    // cheaper to build. What a program can observe stays: keys compare by
    // value (a keyword by namespace and name, though not identical), a key's
    // own `-equiv` runs once per entry it is compared with, in order, each
    // key hashes at most once however many versions share its entry, an
    // existing key is kept on `assoc`, the order of keys, `map-entry?` and
    // metadata on the entries a seq gives, and which of them are identical.
    every_level(
        "(deftype K [n calls]
           IEquiv (-equiv [_ o] (swap! calls conj [n (when (instance? K o) (.-n o))]) (and (instance? K o) (= n (.-n o))))
           IHash (-hash [_] (swap! calls conj [:hash n]) n))
         (let [kw (keyword \"a\" \"b\") other (Keyword. \"a\" \"b\" \"a/b\" nil)
               m (if (js/Math.random) {:x 1 kw 2 \"s\" 3 5 4 nil 5} nil)
               calls (atom [])
               k1 (K. 1 calls) k2 (K. 2 calls)
               km (array-map k1 :one :y 0 k2 :two)
               hashes (atom [])
               hm (array-map (K. 7 hashes) 1 :q 2)
               hm2 (assoc hm :r 3)
               h1 (hash hm) h2 (hash hm2)
               s (seq m) e (first s) big (zipmap (range 8) (range 8))]
           (pr-str [(identical? kw other) (= kw other) (get m other) (contains? m other) (get m :a/c :nf)
                    (count (assoc m other 9)) (identical? kw (key (find (assoc m other 9) other))) (get (assoc m other 9) kw)
                    (keys (assoc m :y 6)) (keys (dissoc m other)) (vals (assoc m \"s\" 7)) (get m nil) (get m 5)
                    (get km (K. 2 calls)) @calls (do (reset! calls []) (get km :y)) @calls
                    (find km (K. 9 calls)) (count @calls)
                    @hashes (= h1 (hash hm)) (= h2 (hash hm2))
                    (map-entry? e) (vector? e) (meta e) (meta (with-meta e {:m 1})) (= e [:x 1]) (hash e) (= (hash e) (hash [:x 1]))
                    (identical? e (first s)) (identical? e (first (seq m))) (identical? (find m :x) (find m :x))
                    (map map-entry? [(first (hash-map :a 1)) (first (sorted-map :a 1)) (find m :x) (first {:a 1})])
                    (meta (assoc (with-meta {:a 1} {:m 1}) :b 2)) (meta (dissoc (with-meta {:a 1 :b 2} {:m 2}) :a))
                    (keys (assoc big 8 8)) (keys (assoc big 3 :three)) (= (assoc big 8 8) (zipmap (range 9) (range 9)))
                    (= m (into {} (seq m))) (= (hash m) (hash (into {} (reverse (seq m)))))]))",
        "[false true 2 true :nf 5 true 9 (:x :a/b \"s\" 5 nil :y) (:x \"s\" 5 nil) (1 2 7 4 5) 5 4 \
         :two [[2 1] [2 nil] [2 1] [2 nil] [2 2]] 0 [] nil 3 \
         [[:hash 7] [:hash 7]] true true \
         true true nil {:m 1} true 1885214723 true \
         true false false (true true true true) \
         {:m 1} {:m 2} \
         (0 1 2 3 4 5 6 7 8) (0 1 2 3 4 5 6 7) true \
         true true]",
    );
}

#[test]
fn small_map_searches_read_a_keyword_key_and_bound_the_entries_as_before() {
    // light-cljs-989: a keyword key's search makes its `extend-type` lookup
    // once and compares each entry by identity first. Since light-cljs-1011
    // it reads the key's own `IEquiv` slot once per search, at the first
    // entry that is not the key, where a getter can watch or make it throw
    // (a getter on a keyed name is deliberate reflection,
    // docs/value-layout.md section 5); without a program's `-equiv` the
    // other entries compare by kind and `fqn`, with no read. The search
    // covers the entries there were when it started, as `findIndex` did,
    // though a key's `-equiv` adds one to a transient.
    every_level(
        "(let [k (keyword \"t989\" \"k\") slot (js* \"'cljs$core$IEquiv$_equiv$arity$2$LKEY'\") reads (atom 0) calls (atom [])
               watch (fn [f] (js/Object.defineProperty k slot #js {:configurable true :get f}))
               _ (watch (fn [] (swap! reads inc) (when (> @reads 1) (fn [_ o] (swap! calls conj o) false))))
               a [(get (array-map k 1) k) @reads (get {} k :nf) @reads (get (array-map :x 1 :y 2 :z 3) k :nf) @reads @calls]
               _ (watch (fn [] (throw (js/Error. \"read\"))))
               b [(get (array-map k 1) k) (try (get (array-map :x 1 k 2) k) (catch :default e (.-message e)))]
               t (transient (array-map :a 1))
               q (reify IEquiv (-equiv [_ o] (assoc! t :b 2) (= o :b)))]
           (js-delete k slot)
           (pr-str [a b (get t q :nf) (get (array-map :x 1 k 2) k)]))",
        "[[1 0 :nf 0 :nf 1 []] [1 \"read\"] :nf 2]",
    );
}

#[test]
fn keyword_searches_call_a_program_equiv_of_keywords() {
    // light-cljs-1040: the keyword searches of array maps (`arrayIndex`,
    // `entryIndex`, `arrayMapDissoc`) take the keyword class's own `-equiv`
    // slot for no program method (`keywordProgramEquiv`), but an
    // `extend-type` of `IEquiv` on `Keyword`, or a `specify!` of a key, still
    // decides which keys are equal, where the spelling would not: `get`,
    // `contains?`, `assoc`, `dissoc`, `find` and a transient's `assoc!`, at
    // every level and inside a macro, whose lookup runs at compile time.
    let extended = r#"(do (extend-type Keyword IEquiv
                            (-equiv [this other] (and (keyword? other) (identical? (name this) (name other)))))
                          (let [m (array-map (keyword "a" "x") 1 :y 2) k (keyword "b" "x")
                                t (transient (array-map (keyword "a" "x") 1 :y 2))]
                            [(get m k) (get m (keyword "b" "z") :nf) (contains? m k) (assoc m k 3) (dissoc m k)
                             (persistent! (assoc! t k 4)) (find m k) (= k :a/x)]))"#;
    let specified = r#"(let [k (specify! (keyword "s" "k") IEquiv (-equiv [_ o] (= o :hit))) m (array-map :a 1 :hit 2)]
                         [(get m k) (contains? m k) (dissoc m k) (assoc m k 3) (get (array-map :a 1) k :nf)])"#;
    for (body, expected) in [
        (
            extended,
            "[1 :nf true {:a/x 3, :y 2} {:y 2} {:a/x 4, :y 2} [:a/x 1] true]",
        ),
        (specified, "[2 true {:a 1} {:a 1, :hit 3} :nf]"),
    ] {
        every_level(&format!("(pr-str {body})"), expected);
        every_level(
            &format!("(defmacro result [] (list 'quote {body})) (pr-str (result))"),
            expected,
        );
    }
}

#[test]
fn hash_map_searches_call_a_program_equiv_of_keywords() {
    // light-cljs-1057: a hash map's trie compares a keyword key with a
    // leaf's by identity, then by kind and `fqn` (`hashKeyTest`), as
    // upstream's `key-test`, but a `specify!` of a key, or an `extend-type`
    // of `IEquiv` on `Keyword`, still decides which keys are equal, where
    // the spelling would not: `get`, `contains?`, `find`, `assoc`, `dissoc`
    // and a transient's lookup, `assoc!` and `dissoc!`, of a leaf the root
    // holds (10 keys), one below it (41 keys) and one in a collision node,
    // at every level and inside a macro, whose lookup runs at compile time.
    let specified = r#"(let [keys (fn [n] (mapcat (fn [i] [(keyword "n" (str "k" i)) i]) (range n)))
                             k (specify! (keyword "s" "k") IEquiv (-equiv [_ o] (= o :hit)) IHash (-hash [_] (hash :hit)))
                             no (specify! (keyword "n" "k3") IEquiv (-equiv [_ _] false))
                             probe (fn [m] [(get m k) (contains? m k) (find m k) (count (dissoc m k))
                                            (get (assoc m k 2) :hit) (count (assoc m k 2))
                                            (get m no :nf) (contains? m no) (count (dissoc m no)) (count (assoc m no 0))
                                            (get m (keyword "n" "k3")) (get (transient m) k)
                                            (count (persistent! (dissoc! (transient m) k)))
                                            (get (persistent! (assoc! (transient m) k 3)) :hit)])
                             small (apply hash-map :hit 1 (keys 9))
                             c1 (specify! (keyword "c" "1") IHash (-hash [_] 7))
                             c2 (specify! (keyword "c" "2") IHash (-hash [_] 7))
                             collided (assoc small c1 :c1 c2 :c2)
                             in-collision (specify! (keyword "s" "c") IHash (-hash [_] 7) IEquiv (-equiv [_ o] (= o (keyword "c" "2"))))
                             fresh (specify! (keyword "c" "1") IHash (-hash [_] 7))]
                         [(probe small) (probe (apply hash-map :hit 1 (keys 40)))
                          [(get collided in-collision) (get collided fresh) (count (dissoc collided in-collision))
                           (get (assoc collided fresh :x) c1) (get (transient collided) in-collision)]])"#;
    let extended = r#"(do (extend-type Keyword IEquiv
                            (-equiv [this other] (and (keyword? other) (= (str this) (str other)) (not= (name this) "stale"))))
                          (let [m (apply hash-map (keyword "a" "stale") 1 (mapcat (fn [i] [(keyword "n" (str "k" i)) i]) (range 40)))
                                stale (keyword "a" "stale")]
                            [(get m stale :nf) (contains? m stale) (count (assoc m stale 2)) (count (dissoc m stale))
                             (get m (keyword "n" "k5")) (get (transient m) stale :nf) (= (keyword "a" "x") (keyword "a" "x"))]))"#;
    for (body, expected) in [
        (
            specified,
            "[[1 true [:hit 1] 9 2 10 :nf false 10 11 3 1 9 3] \
             [1 true [:hit 1] 40 2 41 :nf false 41 42 3 1 40 3] [:c2 :c1 11 :x :c2]]",
        ),
        (extended, "[:nf false 42 41 5 :nf true]"),
    ] {
        every_level(&format!("(pr-str {body})"), expected);
        every_level(
            &format!("(defmacro result [] (list 'quote {body})) (pr-str (result))"),
            expected,
        );
    }
}

#[test]
fn keyword_searches_call_an_equiv_slot_a_program_wrote_by_name() {
    // light-cljs-1057 (Sol review): a program that writes a keyword's
    // `-equiv` slot by its member name, and has no `extend-type`, `specify!`
    // or `reify`, keeps no protocol lookup at `Advanced`; the keyword
    // searches of hash and array maps still call that slot, as `equalPair`'s
    // wrapper calls it. (The array maps' search had answered by the spelling
    // there since light-cljs-1040. `=` with a keyword constant, which
    // `Advanced` compiled to an identity and spelling test before
    // light-cljs-1061, is pinned in type_inference.rs,
    // `equality_with_a_keyword_constant_calls_a_program_equiv_at_every_level`.)
    every_level(
        r#"(let [k (keyword "k") m (hash-map :k (js/parseInt "1") :a 2 :b 3 :c 4 :d 5 :e 6 :f 7 :g 8 :h 9)
                 small (array-map :k (js/parseInt "1"))]
             (set! (.-cljs$core$IEquiv$_equiv$arity$2 k) (fn [_ _] false))
             (pr-str [(get m k "nf") (contains? m k) (count (dissoc m k)) (get small k "nf") (contains? small k)
                      (count (dissoc small k))]))"#,
        "[\"nf\" false 9 \"nf\" false 1]",
    );
}

#[test]
fn contains_of_an_array_map_searches_its_keys_where_its_lookup_is_its_own() {
    // light-cljs-1041: `contains?` of an array map searches its keys as `get`
    // does (`arrayIndex`) where the map's three-argument `-lookup` slot is
    // its class's own (`arrayMapContains`), and otherwise calls `get`, so
    // that a `specify!` or an `extend-type` of `ILookup` answers as before:
    // keys of every kind, and a keyword whose `specify!` of `IEquiv` decides
    // which key is equal at the first key that is not it, at every level and
    // inside a macro, whose lookup runs at compile time.
    let body = r#"(let [m (array-map :a 1 nil 2 "s" 3 4 5 [6] 7 (keyword "n" "k") 8 :v nil)
                        own [(contains? m :a) (contains? m (keyword "n" "k")) (contains? m :b) (contains? m nil)
                             (contains? m "s") (contains? m 4) (contains? m '(6)) (contains? m 9) (contains? m :v)
                             (contains? (array-map) :a)]
                        specified (specify! (array-map :a 1)
                                    ILookup (-lookup ([_ k] (= k :b)) ([_ k nf] (if (= k :b) 1 nf))))
                        k (specify! (keyword "s" "k") IEquiv (-equiv [_ o] (= o :hit)))
                        searched [(contains? (array-map :x 1 :hit 2) k) (contains? (array-map :hit 1) k)
                                  (contains? (array-map k 1) k) (contains? (array-map :x 1) k) (get (array-map :x 1 :hit 2) k)]]
                    [own (contains? specified :a) (contains? specified :b) searched
                     (do (extend-type PersistentArrayMap ILookup (-lookup ([_ k] nil) ([_ k nf] (if (= k :z) 0 nf))))
                         [(contains? (array-map :a 1) :a) (contains? (array-map :a 1) :z) (contains? {:a 1} :z)])])"#;
    let expected = "[[true true false true true true true false true false] false true \
                    [true true true false 2] [false true true]]";
    every_level(&format!("(pr-str {body})"), expected);
    every_level(
        &format!("(defmacro result [] (list 'quote {body})) (pr-str (result))"),
        expected,
    );
}

#[test]
fn array_map_searches_read_each_key_once_and_contains_reads_the_value() {
    // light-cljs-1041: `contains?` of an array map reads the value it found
    // and compares it with the `notFound` that `get` would pass, as before,
    // which a program's `-lookup` can keep; and a keyword whose `specify!`
    // of `IEquiv` decides the search compares the key `arrayIndex` read,
    // within the keys there were when it started, though a public
    // constructor's array has getters.
    let body = r#"(let [saved (atom nil)
                        r (reify ILookup (-lookup [_ k] nil) (-lookup [_ k nf] (reset! saved nf) nf))
                        _ (contains? r :x)
                        kept [(contains? (array-map :a @saved) :a) (contains? (array-map :a 1) :a)]
                        v #js [:a nil]
                        _ (js/Object.defineProperty v 1 #js {:get (fn [] (throw (js/Error. "value")))})
                        thrown (try (contains? (PersistentArrayMap. nil 1 v nil) :a) (catch :default e (.-message e)))
                        k (specify! (keyword "s" "k") IEquiv (-equiv [_ o] (= o :hit)))
                        reads (atom 0)
                        a #js [nil 7]
                        _ (js/Object.defineProperty a 0 #js {:get (fn [] (swap! reads inc) (if (= @reads 1) :hit :miss))})
                        b #js [nil 1]
                        _ (js/Object.defineProperty b 0 #js {:get (fn [] (.push b :hit 2) :x)})]
                    [kept thrown (get (PersistentArrayMap. nil 1 a nil) k :nf) @reads
                     (get (PersistentArrayMap. nil 1 b nil) k :nf) (alength b)])"#;
    let expected = "[[false true] \"value\" 7 1 :nf 4]";
    every_level(&format!("(pr-str {body})"), expected);
    every_level(
        &format!("(defmacro result [] (list 'quote {body})) (pr-str (result))"),
        expected,
    );
}

#[test]
fn keywords_and_symbols_compare_by_their_own_slot_over_an_object_extension() {
    // light-cljs-1040: a keyword's and a symbol's own `-equiv` slot outranks
    // an `extend-type` of `js/Object` with `IEquiv`, as upstream's `Keyword`
    // and `Symbol` own theirs (and as light-cljs-1014 made it for the other
    // classes). Before, `=`, `.equiv` and a vector's items called the
    // program's method, which answered true, so `dissoc` of one key emptied
    // an array map, a map contained every keyword, and `get` at `Advanced`
    // found the first key (docs/value-layout.md, light-cljs-1040's notes).
    every_level(
        "(do (extend-type js/Object IEquiv (-equiv [_ _] true))
             (pr-str [(= :a :b) (= :a :a) (= 'a 'b) (.equiv :a :b) (.equiv 'a 'b)
                      (get {:x 1 :y 2} :y) (get (array-map :x 1 :y 2) :y)
                      (dissoc (array-map :x 1 :y 2 :z 3) :y) (contains? {:x 1} :y) (= [:a] [:b])]))",
        "[false true false false false 2 2 {:x 1, :z 3} false false]",
    );
}

/// Each row of a fixture corpus (`id<TAB>source<TAB>...`) prints, with
/// `pr-str` or as an error message, what it prints at `None`.
fn corpus(fixture: &str, range: std::ops::Range<usize>) {
    let rows: Vec<(&str, &str)> = fixture
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let mut columns = line.split('\t');
            (columns.next().unwrap(), columns.next().unwrap())
        })
        .collect();
    for (id, source) in &rows[rows.len().min(range.start)..rows.len().min(range.end)] {
        let wrapped = format!(
            "(try (pr-str (do {source})) (catch :default e (str \"error: \" (.-message e))))"
        );
        let program = analyze(&wrapped, &parse(&wrapped).unwrap())
            .unwrap_or_else(|error| panic!("{id}: {error}"));
        let outputs = OptimizationLevel::ALL.map(|level| {
            let js = prepare_application(&program, ApplicationOptions::new(level)).emit_js();
            execute(&format!("console.log({js});"))
        });
        for (level, output) in OptimizationLevel::ALL.iter().zip(&outputs).skip(1) {
            assert_eq!(output, &outputs[0], "{id} at {level}");
        }
    }
}

const EQUALITY: &str = include_str!("../fixtures/compiler-equality.tsv");
const SEQUENCE_CONTRACTS: &str = include_str!("../fixtures/compiler-sequence-contracts.tsv");
const SEQUENCE_COMPLETION: &str = include_str!("../fixtures/compiler-sequence-completion.tsv");
const TRANSIENTS: &str = include_str!("../fixtures/compiler-transients.tsv");
const SORTED: &str = include_str!("../fixtures/compiler-sorted-collections.tsv");
const OBJECT_EQUIV: &str = include_str!("../fixtures/compiler_object_equiv.tsv");
const SEQUENCE_OBJECT_EQUIV: &str = include_str!("../fixtures/compiler_sequence_object_equiv.tsv");
const REDUCE_KV: &str = include_str!("../fixtures/compiler-reduce-kv-entries.tsv");
const HASH_BRANCHES: &str = include_str!("../fixtures/compiler-hash-branch-effects.tsv");
const FIND_EFFECTS: &str = include_str!("../fixtures/compiler-find-effects.tsv");
const PUBLIC_CONSTRUCTORS: &str = include_str!("../fixtures/compiler-public-constructors.tsv");
const CORE_HELPERS: &str = include_str!("../fixtures/compiler-core-helpers.tsv");
const LANGUAGE: &str = include_str!("../fixtures/compiler-language.tsv");

row_tests!(corpus;
    equality_corpus(EQUALITY, 0..100),
    sequence_contracts_corpus(SEQUENCE_CONTRACTS, 0..100),
    sequence_completion_corpus(SEQUENCE_COMPLETION, 0..100),
    transients_corpus(TRANSIENTS, 0..100),
    sorted_collections_corpus(SORTED, 0..100),
    object_equiv_corpus(OBJECT_EQUIV, 0..100),
    sequence_object_equiv_corpus(SEQUENCE_OBJECT_EQUIV, 0..100),
    reduce_kv_corpus(REDUCE_KV, 0..100),
    hash_branches_corpus_00(HASH_BRANCHES, 0..35),
    hash_branches_corpus_35(HASH_BRANCHES, 35..100),
    find_effects_corpus(FIND_EFFECTS, 0..100),
    public_constructors_corpus(PUBLIC_CONSTRUCTORS, 0..100),
    core_helpers_corpus(CORE_HELPERS, 0..100),
    language_corpus_000(LANGUAGE, 0..100),
    language_corpus_100(LANGUAGE, 100..200),
    language_corpus_200(LANGUAGE, 200..300),
    language_corpus_300(LANGUAGE, 300..400),
    language_corpus_400(LANGUAGE, 400..500),
    language_corpus_500(LANGUAGE, 500..600),
    language_corpus_600(LANGUAGE, 600..700),
    language_corpus_700(LANGUAGE, 700..800),
);

/// light-cljs-990: the runtime tests a value of its private-field stores by
/// one kind field (`payloads`) instead of probing each store in turn, and a
/// map entry carries that kind instead of a WeakSet entry. light-cljs-1009
/// rescoped the copies of a vector's and a map entry's own fields onto
/// their prototype (docs/value-layout.md, section 5, item 1): those classes
/// hold their store in an own keyed field, so such a copy, deliberate
/// reflection, is now a value of its kind. Every predicate
/// answers as before for the runtime's values, host values, and values forged
/// by `Object.create` of a runtime type's prototype or by copying a value's
/// own properties (a record's methods are its prototype's, so a value
/// inheriting them is one to `satisfies?`, as it was). The table is what
/// master `f0f5e81b`'s runtime prints. The Proxy is asked for nothing but
/// protocol mask and marker properties, `Symbol.iterator` and its prototype:
/// the kind tests read no property of a host value. light-cljs-1028 removed
/// the rows forged from a vector's, a map entry's and a transient vector's
/// prototypes (docs/value-layout.md, section 5, item 1): those prototypes
/// declare their protocols, so such a value, deliberate reflection,
/// type-tests as its kind. Since light-cljs-1070 the other classes declare
/// theirs, and the remaining forged rows type-test as their kinds too
/// (item 36).
#[test]
fn value_kind_predicates_answer_as_the_brand_stores_did() {
    let source = r#"
        (defrecord R [a])
        (let [log (array)
              handler (js-obj "get" (fn [t k] (.push log (str "get:" (.toString k))) (aget t k))
                              "has" (fn [t k] (.push log (str "has:" (.toString k))) (js-in k t))
                              "getPrototypeOf" (fn [t] (.push log "proto") (js/Object.getPrototypeOf t)))
              proxy (js/Proxy. (js-obj) handler)
              forge (fn [v] (js/Object.create (js/Object.getPrototypeOf v)))
              copy (fn [v] (js/Object.defineProperties (forge v) (js/Object.getOwnPropertyDescriptors v)))
              e (first {:a 1})
              values [[1 2] e (list 1) () (lazy-seq [1]) (cons 1 nil) (range 3) (range 2.5) {:a 1}
                      (zipmap (range 20) (range 20)) (sorted-map :a 1) #{1} (into #{} (range 20)) (sorted-set 1)
                      (conj cljs.core/PersistentQueue.EMPTY 1) (transient []) (transient {}) (subvec [1 2 3] 1)
                      (->R 1) (assoc (->R 1) :b 2) (seq [1 2]) (seq {:a 1}) (seq (sorted-map :a 1)) (rseq [1 2])
                      (seq (zipmap (range 20) (range 20))) (iterate inc 0) (repeat 1) (cycle [1]) (reduced 1)
                      (atom 1) (volatile! 1) :k 'sym "s" 1 nil (array 1) (js-obj) inc proxy
                      (forge {:a 1}) (copy {:a 1})
                      (forge (list 1)) (copy (list 1)) (forge #{1}) (forge (lazy-seq [1])) (forge (range 3))
                      (forge (seq [1 2])) (forge (->R 1))]
              labels (quote [vector entry list empty-list lazy-seq cons range float-range array-map hash-map sorted-map set hash-set sorted-set queue transient-vector transient-map subvec record record+ vector-seq map-seq sorted-map-seq rseq hash-map-seq iterate repeat cycle reduced atom volatile keyword symbol string number null array object fn proxy forged-map copied-map forged-list copied-list forged-set forged-lazy-seq forged-range forged-seq forged-record])
              preds [vector? seq? map? map-entry? set? seqable? coll? sequential? counted? associative? list?
                     sorted? reversible? record? ifn? indexed? reduced? chunked-seq? reduceable?
                     #(satisfies? ISeqable %) #(satisfies? ICollection %) #(satisfies? IMeta %)
                     #(satisfies? IWithMeta %) #(satisfies? IMapEntry %) #(satisfies? ILookup %)
                     #(satisfies? IPrintWithWriter %) #(satisfies? IEmptyableCollection %)
                     #(satisfies? IKVReduce %) #(satisfies? IEditableCollection %) #(satisfies? IHash %)
                     #(satisfies? ICounted %) #(satisfies? INext %) #(satisfies? ISeq %) #(satisfies? IStack %)]]
          (array (.join (into-array (map (fn [l v] (str l " " (apply str (map #(if (% v) 1 0) preds)))) labels values)) ", ")
           (.join log " ")))
    "#;
    let expected = concat!(
        "vector 1000011111001011001111101111111001\n",
        "entry 1001011111001011001111111110011001\n",
        "list 0100011110100000001111100110011111\n",
        "empty-list 0100011110100000001111100110011111\n",
        "lazy-seq 0100011100000000001111100110010110\n",
        "cons 0100011110100000001111100110011111\n",
        "range 0100011110000001011111100110011110\n",
        "float-range 0100011100000000011111100110010110\n",
        "array-map 0010011011000010001111101111111000\n",
        "hash-map 0010011011000010001111101111111000\n",
        "sorted-map 0010011011011010001111101111011000\n",
        "set 0000111010000010000111101110111000\n",
        "hash-set 0000111010000010000111101110111000\n",
        "sorted-set 0000111010011010000111101110011000\n",
        "queue 0100011110000000000111100110011111\n",
        "transient-vector 0000000010000011000000001000001000\n",
        "transient-map 0000000010000010000000001000001000\n",
        "subvec 1000011111001011001111101111011001\n",
        "record 0010011011000100000111101101011000\n",
        "record+ 0010011011000100000111101101011000\n",
        "vector-seq 0100011110001001001111100110011110\n",
        "map-seq 0100011110000000001111100110011110\n",
        "sorted-map-seq 0100011110000000001111100110011110\n",
        "rseq 0100011100000000001111100110010110\n",
        "hash-map-seq 0100011100000000001111100110010110\n",
        "iterate 0100011100000000001111100110000110\n",
        "repeat 0100011100000000001111100110010110\n",
        "cycle 0100011100000000001111100110000110\n",
        "reduced 0000000000000000100000000100010000\n",
        "atom 0000000000000000000001000100010000\n",
        "volatile 0000000000000000000000000100010000\n",
        "keyword 0000000000000010000000000100010000\n",
        "symbol 0000000000000010000001100100010000\n",
        "string 0000010000000000000000000000010000\n",
        "number 0000000000000000000000000000010000\n",
        "null 0000010010000000000000000000011000\n",
        "array 0000010000000000000000000000010000\n",
        "object 0000000000000000000000000000010000\n",
        "fn 0000000000000010000001000000010000\n",
        "proxy 0000000000000000000000000000010000\n",
        "forged-map 0010011011000010001111101111111000\n",
        // A map's own fields copied onto its prototype make one by deliberate
        // reflection (light-cljs-1012, docs/value-layout.md section 5).
        "copied-map 0010011011000010001111101111111000\n",
        "forged-list 0100011110100000001111100110011111\n",
        // A list's own fields copied onto its prototype make one by deliberate
        // reflection (light-cljs-1030, docs/value-layout.md section 5).
        "copied-list 0100011110100000001111100110011111\n",
        "forged-set 0000111010000010000111101110111000\n",
        "forged-lazy-seq 0100011100000000001111100110010110\n",
        "forged-range 0100011110000001011111100110011110\n",
        "forged-seq 0100011100000000000111100110010110\n",
        "forged-record 0010011011000100000111101101011000\n",
        "get:Symbol(Symbol.iterator) proto\n",
    );
    let program = program(source);
    for level in OptimizationLevel::ALL {
        let js = prepare_application(&program, ApplicationOptions::new(level)).emit_js();
        // The same answers where a host gave `Object.prototype` the stores'
        // kind metadata before the runtime loaded: only a store's own counts
        // (the Sol review of light-cljs-990).
        for pollution in [
            "",
            "for (const [k, v] of [['kindBits', -1], ['exact', true]]) \
             Object.defineProperty(Object.prototype, k, {value: v, writable: true, configurable: true});",
        ] {
            let script = format!(
                "{pollution} const r = {js}; console.log(r[0].split(', ').join('\\n')); \
                 console.log([...new Set(r[1].split(' ').filter(t => !t.startsWith('get:cljs$') && !t.startsWith('get:$')))].sort().join(' '));"
            );
            assert_eq!(execute(&script), expected, "at {level}: {pollution}");
        }
    }
}

/// The keyed slots of array, hash and sorted maps (light-cljs-1012) answer
/// every map of their class, a public constructor's included, through
/// the operations' slot-first wrappers and the protocol functions, with
/// the answers the operations gave before (the same program on master).
const MAP_SLOTS_SOURCE: &str = "(let [pub (cljs.core/PersistentArrayMap. nil 2 (array :a 1 :b 2) nil)
      big (cljs.core/PersistentArrayMap. nil 8 (array :a 1 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h 8) nil)
      h (hash-map :a 1 :b 2) s (sorted-map :b 2 :a 1) m (with-meta {:x 1} {:m 1})]
  [(-count pub) (-lookup pub :a) (-lookup pub :z :none) (-contains-key? pub :b) (-assoc pub :c 3) (-dissoc pub :a)
   (count (-assoc big :i 9)) (map? (-assoc big :i 9)) (-count h) (-dissoc h :a) (-lookup (-assoc h :c 3) :c) (-count (-conj h [:d 4]))
   (-lookup s :a) (-assoc s :c 3) (-count s) (count pub) (get pub :b) (contains? pub :z) (assoc pub :c 3) (dissoc pub :b)
   (conj s [:c 3]) (meta (assoc m :y 2)) (meta (dissoc m :x)) (dissoc {:a 1 :b 2 :c 3} :b) (dissoc {:a 1} :a) (dissoc {:a 1} :z)
   (get (dissoc (zipmap (range 20) (range 20)) 3) 4) (count (dissoc (zipmap (range 20) (range 20)) 3 4)) (contains? {nil 1} nil)
   (get (assoc {:a 1 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h 8} :i 9) :i) (persistent! (assoc! (transient {:a 1}) :b 2))])";
const MAP_SLOTS_EXPECTED: &str = "[2 1 :none true {:a 1, :b 2, :c 3} {:b 2} 9 true 2 {:b 2} 3 3 1 {:a 1, :b 2, :c 3} 2 2 2 false {:a 1, :b 2, :c 3} {:a 1} {:a 1, :b 2, :c 3} {:m 1} {:m 1} {:a 1, :c 3} {} {:a 1} 4 18 true 9 {:a 1, :b 2}]";

#[test]
fn map_slots_answer_every_map_of_their_class() {
    every_level(&format!("(pr-str {MAP_SLOTS_SOURCE})"), MAP_SLOTS_EXPECTED);
}

/// The classes' own `-equiv` and `-hash` (light-cljs-1014) give `=` and
/// `hash` today's answers: a vector against a list, a seq, a subvector or a
/// map entry, a map against a record and against maps of the other classes,
/// sets of either class, a vector of more than one leaf, a NaN item (`=` is
/// identity first, `.equiv` is not), and the hashes that equal values of
/// different classes share. The answers are those of the runtime before it
/// (5cd32c66), at every level.
const CLASS_EQUALITY_SOURCE: &str = r#"(do (defrecord R [a])
  (let [v (vec (range 40)) w (vec (range 40)) v2 (conj (vec (range 39)) :x)
        e (first {:a 1}) sv (subvec [0 1 2 3] 1 3)
        h (hash-map :a 1 :b 2) s (sorted-map :a 1 :b 2)
        nan [js/NaN]]
    (pr-str
      [(= [1 2] '(1 2)) (= '(1 2) [1 2]) (= [1 2] (seq [1 2])) (= [1 2] (map inc [0 1])) (= [1 2] (range 1 3))
       (= [:a 1] e) (= e [:a 1]) (= [:a 2] e) (= [1 2] sv) (= sv [1 2]) (= [1 2] [1 2 3]) (= [] '()) (= [] {})
       (= v w) (= v v2) (= v (seq w)) (= nan nan) (= nan [js/NaN]) (.equiv nan nan)
       (= {:a 1} (->R 1)) (= (->R 1) {:a 1}) (= {:a 1 :b 2} h) (= h {:a 1 :b 2}) (= s h) (= h s) (= {:a 1 :b 2} s)
       (= {:a 1} {:a 2}) (= {:a 1} {:b 1}) (= {:a nil} {:b nil}) (= {nil 1} {nil 1}) (= {[1] 2} {'(1) 2})
       (= #{1 2} (sorted-set 1 2)) (= (sorted-set 1 2) #{1 2}) (= #{1 2} #{1 3}) (= #{[1]} #{'(1)})
       (= '(1 2) '(1 2)) (= (list 1 2) (cons 1 (list 2))) (= (map inc [0 1]) [1 2]) (= (seq [1 2]) (list 1 2))
       (= (hash [1 2]) (hash '(1 2)) (hash (seq [1 2])) (hash (map inc [0 1])))
       (= (hash {:a 1}) (hash (hash-map :a 1)) (hash (sorted-map :a 1)))
       (= (hash #{1 2}) (hash (sorted-set 1 2))) (= (hash v) (hash (seq w))) (= (hash e) (hash [:a 1]))
       (hash [1 2 3]) (hash {:a 1}) (hash #{:a}) (hash :a) (hash 'a/b) (hash v) (hash '(1 2)) (hash (seq [1 2]))
       (hash 1.5) (hash "ab") (hash true)])))"#;

#[test]
fn classes_compare_and_hash_as_the_generic_walk_did() {
    every_level(
        CLASS_EQUALITY_SOURCE,
        "[true true true true true true true false true true false true false true false true true false false \
         false false true true true true true false false false true true true true false true true true true \
         true true true true true true -1710654795 -2144769406 1271612507 -2123407586 -1172211204 -833271242 \
         1185709346 1185709346 63551 1707819681 1231]",
    );
    // A program's `ISeqable` of vectors, `ILookup` of array maps and
    // `ICounted` of one vector, which empty the kind tables, are what `=`
    // and `hash` read afterwards, as before: the classes then compare and
    // hash through the generic walk, not by their fields.
    every_level(
        "(let [a [1 2] b [3 4] c (specify! [5 6] ICounted (-count [_] 3)) m {:a 1} n {:a 2}]
           (extend-type PersistentVector ISeqable (-seq [_] (list 1)))
           (extend-type PersistentArrayMap ILookup (-lookup ([_ k] 1) ([_ k nf] 1)))
           (js/JSON.stringify (array (= a b) (= (hash a) (hash b)) (= a c) (= c a) (= m n) (= n m)
                                     (= [1 2] '(1)) (= '(1) [7 8]))))",
        "[true,true,false,false,true,false,false,false]",
    );
    // `hash` of a primitive reads its prototype's `IHash` by name, which an
    // `extend-type` of `js/Number` or `js/String` writes; the `-equiv`
    // protocol function answers an identical collection holding NaN as `=`
    // does, and `.equiv` without the identity test, in the program and in a
    // macro (the compile-time lookup calls the classes' own slots).
    every_level(
        "(defmacro probe [] (let [v [js/NaN] m {:a js/NaN}] [(-equiv v v) (.equiv v v) (-equiv m m) (.equiv m m) (= v v)]))
         (let [nan [js/NaN] m {:a js/NaN} p (probe)]
           (extend-type js/Number IHash (-hash [_] 37))
           (extend-type js/String IHash (-hash [_] 38))
           (js/JSON.stringify (array (hash 2) (hash \"x\") (hash [2 \"x\"]) (-equiv nan nan) (.equiv nan nan)
                                     (-equiv m m) (.equiv m m) (nth p 0) (nth p 1) (nth p 2) (nth p 3) (nth p 4))))",
        "[37,38,-1156898992,true,false,true,false,true,false,true,false,true]",
    );
}

/// `object?` of a record whose field is named `constructor` and holds
/// `js/Object`: a record at `Advanced` holds no `ICollection` marker
/// (light-cljs-1069), so the collection test reads its mask bit
/// (`typeMaskDeclares`); a plain object stays one.
#[test]
fn object_predicate_excludes_mask_only_records() {
    every_level(
        "(defrecord R [constructor])
         (let [r (atom (->R js/Object))] (array (object? @r) (object? (js-obj \"a\" 1)) (coll? @r)))",
        "[ false, true, true ]",
    );
}

/// The updates shaped as upstream's (light-cljs-1068) keep the old paths'
/// answers where a program changes what they read: a vector's variadic
/// `assoc` and a set's variadic `disj` take every key at once and the
/// value's metadata once, so an `extend-type` of `IMeta` is asked as often
/// as before; `disj` and `pop` of a program type whose slot a getter
/// answers false finish from that read, as the old paths' one lookup did,
/// without reading the slot again; and the update slots a program clears by
/// name, on the value or its class's prototype, answer by the class
/// (`slotWrites`), lists, sets and maps included.
#[test]
fn update_fallbacks_keep_metadata_reads_and_selected_dispatch() {
    every_level(
        r#"(def reads (atom 0))
           (extend-type PersistentVector IMeta (-meta [_] (swap! reads inc) nil))
           (extend-type PersistentHashSet IMeta (-meta [_] (swap! reads inc) nil))
           (extend-type PersistentTreeSet IMeta (-meta [_] (swap! reads inc) nil))
           (let [counted (fn [f] (reset! reads 0) (let [result (f) n @reads] [(pr-str result) n]))]
             (pr-str [(counted #(assoc [0 0] 0 1 1 2)) (counted #(assoc [0 0] 0 1))
                      (counted #(disj (hash-set 1 2 3) 1 2)) (counted #(disj (sorted-set 1 2 3) 1 2))]))"#,
        r##"[["[1 2]" 1] ["[1 0]" 1] ["#{3}" 1] ["#{3}" 1]]"##,
    );
    every_level(
        r#"(deftype S [] ISet (-disjoin [_ _] :disjoined) IStack (-peek [_] nil) (-pop [_] :popped))
           (let [answer (fn [slot f]
                          (let [value (S.) reads (atom 0)]
                            (js/Object.defineProperty value slot
                              #js {:get (fn [] (if (= 1 (swap! reads inc)) false (fn [& _] :unexpected)))})
                            (try (f value) (catch :default error (.-message error)))))]
             (pr-str [(answer (js* "'cljs$core$ISet$_disjoin$arity$2$LKEY'") #(disj % :k))
                      (answer (js* "'cljs$core$IStack$_pop$arity$1$LKEY'") pop)
                      (disj (S.) :k) (pop (S.))]))"#,
        r#"["disj not supported on this type" "No protocol method IStack.-pop defined for this value" :disjoined :popped]"#,
    );
    for missing in ["nil", "false"] {
        every_level(
            &format!(
                "(let [v [1 2 3] s #{{1 2}} m {{:a 1 :b 2}} l (list 1 2)]
                   (set! (.-cljs$core$IStack$_pop$arity$1 v) {missing})
                   (set! (.-cljs$core$ISet$_disjoin$arity$2 s) {missing})
                   (set! (.-cljs$core$IMap$_dissoc$arity$2 m) {missing})
                   (set! (.-cljs$core$IStack$_pop$arity$1 l) {missing})
                   (set! (.-cljs$core$ICollection$_conj$arity$2 l) {missing})
                   (pr-str [(pop v) (disj s 1) (dissoc m :a) (pop l) (conj l 0) (disj s 1 2)
                            (conj nil 1) (assoc nil :k 1) (dissoc nil :k) (disj nil 1) (pop nil)]))"
            ),
            "[[1 2] #{2} {:b 2} (2) (0 1 2) #{} (1) {:k 1} nil nil nil]",
        );
    }
    // ... and cleared on the class's prototype.
    every_level(
        "(set! (.. PersistentVector -prototype -cljs$core$IStack$_pop$arity$1) nil)
         (set! (.. PersistentHashSet -prototype -cljs$core$ISet$_disjoin$arity$2) nil)
         (set! (.. PersistentArrayMap -prototype -cljs$core$IMap$_dissoc$arity$2) false)
         (set! (.. List -prototype -cljs$core$ICollection$_conj$arity$2) nil)
         (set! (.. List -prototype -cljs$core$IStack$_pop$arity$1) false)
         (pr-str [(pop [1 2 3]) (disj (hash-set 1 2) 1) (dissoc {:a 1 :b 2} :a) (conj (list 1 2) 0)
                  (pop (list 1 2)) (list? (conj (list 1 2) 0))])",
        "[[1 2] #{2} {:b 2} (0 1 2) (2) true]",
    );
}

/// `reduce` of a runtime value that `extend-type` or `specify!` gave an
/// `IIterable` iterator walks the iterator where the old path did: an atom,
/// a queue and a chunked cons, both arities; a list keeps the walk of its
/// seq, which the old path gave it (light-cljs-1072).
#[test]
fn reduce_honors_iterable_extensions_on_runtime_values() {
    every_level(
        r#"(extend-type Atom IIterable (-iterator [a] (-iterator [@a 10])))
           (let [p (fn [f] (try (pr-str (f)) (catch :default e (str "E:" (.-message e)))))
                 a1 (specify! (atom 0) IIterable (-iterator [_] (-iterator [1 2 3])))
                 q (specify! (conj cljs.core/PersistentQueue.EMPTY 1) IIterable (-iterator [_] (-iterator [7 8])))
                 l (specify! (list 1 2) IIterable (-iterator [_] (-iterator [100])))
                 cc (specify! (seq (map inc (range 40))) IIterable (-iterator [_] (-iterator [100])))]
             (pr-str [(p #(reduce + 0 a1)) (p #(reduce + a1)) (p #(reduce + (atom 5))) (p #(reduce + 1 (atom 5)))
                      (p #(reduce + q)) (p #(reduce + 0 q)) (p #(reduce + l)) (p #(reduce + 0 l)) (p #(reduce + cc))
                      (p #(reduce + (hash-set 1 2)))]))"#,
        r#"["6" "6" "15" "16" "15" "15" "3" "3" "100" "3"]"#,
    );
}

/// A reduction slot that a program clears by name, on a value or on its
/// class's prototype, still answers by the class, as the old paths did: a
/// program that writes a core protocol method's member keeps the
/// `slotWrites` effects of `reduceOf` and `reduceKvOf` (light-cljs-1072).
#[test]
fn reduction_fallbacks_keep_cleared_slots() {
    for missing in ["nil", "false"] {
        every_level(
            &format!(
                "(let [v [1 2 3] m {{:a 1 :b 2}} lz (map inc [1 2]) r (range 4) s (hash-set 1 2)]
                   (set! (.-cljs$core$IReduce$_reduce$arity$2 v) {missing})
                   (set! (.-cljs$core$IReduce$_reduce$arity$3 v) {missing})
                   (set! (.-cljs$core$IKVReduce$_kv_reduce$arity$3 v) {missing})
                   (set! (.-cljs$core$IReduce$_reduce$arity$3 m) {missing})
                   (set! (.-cljs$core$IKVReduce$_kv_reduce$arity$3 m) {missing})
                   (set! (.-cljs$core$IReduce$_reduce$arity$3 lz) {missing})
                   (set! (.-cljs$core$IReduce$_reduce$arity$2 r) {missing})
                   (set! (.-cljs$core$IReduce$_reduce$arity$3 s) {missing})
                   (pr-str [(reduce + v) (reduce + 10 v) (reduce-kv (fn [a k x] (+ a k x)) 0 v) (reduce (fn [a [_ x]] (+ a x)) 0 m)
                            (reduce-kv (fn [a _ x] (+ a x)) 0 m) (reduce + 0 lz) (reduce + r) (reduce + 0 s) (reduce + nil)
                            (reduce + 5 nil) (reduce-kv + 7 nil)]))"
            ),
            "[6 16 9 3 3 5 6 3 0 5 7]",
        );
    }
    // ... and cleared on the class's prototype, and on a value over a method
    // `extend-type` gave its class, which throws for the arity the set does
    // not implement, as before.
    let cleared = r#"(extend-type PersistentHashSet IReduce (-reduce ([_ f] :ext2) ([_ f i] :ext3)))
        (set! (.. PersistentVector -prototype -cljs$core$IReduce$_reduce$arity$3) nil)
        (set! (.. PersistentVector -prototype -cljs$core$IKVReduce$_kv_reduce$arity$3) false)
        (set! (.. PersistentArrayMap -prototype -cljs$core$IReduce$_reduce$arity$2) nil)
        (set! (.. LazySeq -prototype -cljs$core$IReduce$_reduce$arity$3) nil)
        (set! (.. IntegerRange -prototype -cljs$core$IReduce$_reduce$arity$3) false)
        (let [p (fn [f] (try (pr-str (f)) (catch :default e (str "E:" (.-message e)))))
              s (hash-set 1 2 3)]
          (set! (.-cljs$core$IReduce$_reduce$arity$3 s) nil)
          (pr-str [(p #(reduce + 0 [1 2 3])) (p #(reduce + [1 2 3])) (p #(reduce-kv (fn [a k x] (+ a k x)) 0 [1 2 3]))
                   (p #(reduce (fn [a e] (+ (if (number? a) a (val a)) (val e))) {:a 1 :b 2})) (p #(reduce + 0 (map inc [1 2])))
                   (p #(reduce + 0 (range 4))) (p #(reduce + (range 4))) (p #(reduce + 0 s)) (p #(reduce + s))
                   (p #(reduce + (hash-set 4)))]))"#;
    every_level(
        cleared,
        r#"["6" "6" "9" "3" "5" "6" "6" "E:No protocol method IReduce/-reduce defined for this value" ":ext2" ":ext2"]"#,
    );
    let kept = units(cleared);
    for unit in ["slotWrites.reduce", "slotWrites.reduceKv"] {
        assert!(kept.contains(&unit), "{unit}: {kept:?}");
    }
}

/// `=` and `hash` are upstream-shaped at `:advanced` (light-cljs-1074): the
/// runtime's seqs and queue without an `-equiv` or `-hash` slot of their
/// own compare and hash their items from `equalPairOf` and `hashOf`, by their
/// keyed kind; iterates, cycles, atoms and host values keep their identity;
/// UUIDs and vars have slots; `.equiv` of an identical collection compares
/// its contents. Master c09a181b's answers, at every level.
#[test]
fn equality_and_hash_fallbacks_keep_the_old_answers() {
    let source = r#"
(defrecord R [a])
(deftype T [x])
(def u (uuid "00000000-0000-0000-0000-000000000001"))
(let [i (iterate inc 1) c (cycle [1 2]) q (conj cljs.core/PersistentQueue.EMPTY 1 2) d (js/Date. 5)]
  (pr-str [(= (range 3) [0 1 2]) (= [0 1 2] (range 3)) (= (seq {1 2}) [[1 2]]) (= (rseq [1 2]) '(2 1)) (= (keys {1 2}) [1])
           (= (vals {1 2}) '(2)) (= (repeat 2 :a) [:a :a]) (= () []) (= () nil) (= (range 0.5 2) [0.5 1.5]) (= (re-seq #"a" "aa") ["a" "a"])
           (= q [1 2]) (= [1 2] q) (= q '(1 2)) (= i i) (= i (iterate inc 1)) (= c c) (= c (cycle [1 2])) (= (range 2) (->R 1))
           (= u (uuid "00000000-0000-0000-0000-000000000001")) (= u (uuid "00000000-0000-0000-0000-000000000002")) (= u "x")
           (= (->R 1) (->R 1)) (= (->R 1) {:a 1}) (= {:a 1} (->R 1)) (= (T. 1) (T. 1)) (= d (js/Date. 5)) (= d 5) (= (atom 1) (atom 1))
           (= (hash (range 3)) (hash [0 1 2])) (= (hash (seq {1 2})) (hash [[1 2]])) (= (hash q) (hash [1 2])) (= (hash (repeat 2 :a)) (hash [:a :a]))
           (hash ()) (hash u) (= (hash i) (hash i)) (= (hash (->R 1)) (hash (->R 1))) (hash d) (hash "s") (hash 1.5) (hash false) (hash nil)
           (let [v [##NaN] m {:a ##NaN} s #{##NaN} l (list ##NaN)] [(= v v) (.equiv v v) (.equiv m m) (.equiv s s) (.equiv l l) (.equiv c c) (.equiv q q)])
           (.equiv [1] '(1)) (.equiv :a :a) (.equiv {:a 1} {:a 1})
           (count (into #{} [[1] '(1) (range 1 2) (seq [1]) (subvec [0 1] 1) (lazy-seq [1])])) (get {(range 2) :r} [0 1])]))"#;
    every_level(
        source,
        "[true true true true true true true true false true true true true true true false true false false true false false true false false false true false false true true true true -2017569654 -1060979852 true true 5 1975881410 63551 1237 0 [true false false false false true true] true true true 1 :r]",
    );
    // In release output, `=` and `hash` keep their upstream-shaped paths and
    // not the old ones, which only the compile-time runtime, the shared-heap
    // profile and a program writing a slot by name keep.
    let kept = units(source);
    for unit in [
        "equalPair",
        "equalPairOf",
        "hash",
        "hashOf",
        "uuidValue.equiv",
        "uuidValue.hash",
    ] {
        assert!(kept.contains(&unit), "{unit}: {kept:?}");
    }
    for unit in [
        "equalPairPath",
        "hashPath",
        "structuralEqualPair",
        "hashStructural",
        "slotWrites.equalPair",
        "slotWrites.hash",
    ] {
        assert!(!kept.contains(&unit), "{unit}: {kept:?}");
    }
    // A slot cleared by name, on a class's prototype or on a value over a
    // method `extend-type` gave its class, takes the old path, which answers
    // the value by its class (the set's structural hash, not the
    // extension's 99), as before.
    let cleared = r#"
(extend-type PersistentHashSet IHash (-hash [_] 99))
(set! (.. PersistentVector -prototype -cljs$core$IEquiv$_equiv$arity$2) nil)
(set! (.. PersistentArrayMap -prototype -cljs$core$IHash$_hash$arity$1) false)
(set! (.. Keyword -prototype -cljs$core$IEquiv$_equiv$arity$2) nil)
(set! (.. List -prototype -cljs$core$IHash$_hash$arity$1) nil)
(let [p (fn [f] (try (pr-str (f)) (catch :default e (str "E:" (.-message e)))))
      s (hash-set 1 2) l (list 1 2) m {:a 1}]
  (set! (.-cljs$core$IHash$_hash$arity$1 s) nil)
  (set! (.-cljs$core$IEquiv$_equiv$arity$2 m) false)
  (pr-str [(p #(= [1 2] [1 2])) (p #(= [1 2] '(1 2))) (p #(= :a :a)) (p #(= :a (keyword "a"))) (p #(= m {:a 1})) (p #(= {:a 1} m))
           (p #(= (hash {:a 1}) (hash {:a 1}))) (p #(hash s)) (p #(hash (hash-set 3))) (p #(= (hash l) (hash [1 2]))) (p #(= (hash [1 2]) (hash '(1 2))))
           (p #(contains? #{[1 2]} '(1 2))) (p #(get {:a 1 [1] 2} [1]))]))"#;
    every_level(
        cleared,
        r#"["true" "true" "true" "true" "true" "true" "true" "-1751407317" "99" "true" "true" "true" "2"]"#,
    );
    let kept = units(cleared);
    for unit in ["slotWrites.equalPair", "slotWrites.hash"] {
        assert!(kept.contains(&unit), "{unit}: {kept:?}");
    }
    // From the Sol review of light-cljs-1074: `.equiv` of an identical queue,
    // repeat or reversed seq holding NaN compares its items, and an
    // `extend-type` of `js/Object` answers UUIDs and vars, which have no slot
    // of their own, as before.
    every_level(
        r#"(def v (var cljs.core/inc))
(let [q (conj cljs.core/PersistentQueue.EMPTY ##NaN) r (repeat 1 ##NaN) rs (rseq [##NaN 1]) e (list) a (uuid "00000000-0000-0000-0000-000000000001") b (uuid "00000000-0000-0000-0000-000000000002")
      before [(.equiv q q) (.equiv r r) (.equiv rs rs) (.equiv e e) (= a b) (= v v) (= v (var cljs.core/inc)) (= (hash v) (hash 'cljs.core/inc)) (= (hash a) (hash "00000000-0000-0000-0000-000000000001"))]]
  (extend-type js/Object IEquiv (-equiv [_ _] true) IHash (-hash [_] 37))
  (pr-str [before (= a b) (hash a) (= v (var cljs.core/dec)) (hash v) (= [1] [2]) (hash :k)]))"#,
        "[[false false false true false true true true true] true 37 true 37 true -2146297393]",
    );
    // From the second Sol review: `.equiv` of an identical map, set or
    // keyword whose slot a program cleared by name compares it structurally
    // (`slotWrites.objectEquiv`), and an `extend-type` of `object` or
    // `default` answers cycles, iterates, atoms and host objects, which have
    // no native `IEquiv` or `IHash`, as before.
    every_level(
        r#"(let [m {:a ##NaN} s #{##NaN} k (Keyword. nil "k" js/NaN nil) q (conj cljs.core/PersistentQueue.EMPTY ##NaN) m2 {:b ##NaN} s2 #{##NaN 1}]
  (set! (.-cljs$core$IEquiv$_equiv$arity$2 m) nil)
  (set! (.-cljs$core$IEquiv$_equiv$arity$2 s) false)
  (set! (.. PersistentHashSet -prototype -cljs$core$IEquiv$_equiv$arity$2) nil)
  (pr-str [(.equiv m m) (.equiv s s) (.equiv s2 s2) (.equiv k k) (.equiv q q) (= m m) (= s s) (.equiv m2 m2) (.equiv {:a 1} {:a 1})]))"#,
        "[false false false false false true true false true]",
    );
    every_level(
        r#"(extend-type object IEquiv (-equiv [_ _] true) IHash (-hash [_] 37))
(extend-type default IEquiv (-equiv [_ _] :d) IHash (-hash [_] 41))
(pr-str [(= (iterate inc 1) (iterate inc 1)) (hash (iterate inc 1)) (hash (cycle [1])) (= (cycle [1]) (cycle [2])) (= (atom 1) (atom 2)) (hash (atom 1))
         (= (range 2) [0 1]) (= (uuid "00000000-0000-0000-0000-000000000001") (uuid "00000000-0000-0000-0000-000000000002")) (hash (js/Date. 5)) (= (js-obj) (js-obj)) (hash (js-obj))])"#,
        "[true 37 37 true false 1 true true 5 true 37]",
    );
    // The seqs and the queue without a slot compare and hash their items
    // before any lookup (they implement `IEquiv` and `IHash` natively); an
    // `extend-type` of `object` or `default` still answers them as before.
    every_level(
        r#"(extend-type object IEquiv (-equiv [_ _] true) IHash (-hash [_] 37))
(extend-type default IEquiv (-equiv [_ _] true) IHash (-hash [_] 41))
(let [q (conj cljs.core/PersistentQueue.EMPTY 1)]
  (pr-str [(= (range 3) [9]) (= (range 0.5 2) [9]) (= (seq {1 2}) [9]) (= () [1]) (= (repeat 2 1) [3]) (= (keys {1 2}) [9]) (= (vals {1 2}) [9]) (= q [9])
           (= (rseq [1 2]) [9]) (= (re-seq #"a" "a") [9]) (= (seq (sorted-map 1 2)) [9]) (= (seq (hash-map 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18)) [9])
           (= (hash (range 3)) 37) (= (hash (seq {1 2})) 37) (= (hash ()) 37) (= (hash q) 37) (= (hash (repeat 2 1)) 37) (= (hash (keys {1 2})) 37) (= (hash (rseq [1 2])) 37)
           (= (hash (re-seq #"a" "a")) 37) (= (hash (range 0.5 2)) 37) (= (iterate inc 1) (iterate inc 1)) (hash (cycle [1]))]))"#,
        "[false false false false false true true true false true false false true true true true true true true true true true 37]",
    );
}

/// The upstream-shaped paths share their cold steps (light-cljs-1078): a
/// class's slot and the protocol lookup through `fallbackMethod`, the errors
/// through `unsupported`. Their nil answers, a host type's `extend-type`
/// methods, the seq walks and the errors are master `15fc27ba`'s at every
/// level, and an `Advanced` slice keeps none of the effects that put a
/// compiler session's own paths in front of them (`operationPaths.count`
/// and the others).
#[test]
fn fallbacks_share_their_cold_steps() {
    let source = r#"(deftype Box [x])
        (extend-type js/Date ICounted (-count [_] 42) ICollection (-conj [_ x] [:conj x]) IStack (-peek [_] :peek) (-pop [_] :pop))
        (let [p (fn [f] (try (pr-str (f)) (catch :default e (str "E:" (.-message e)))))
              d (js/Date. 0)]
          (pr-str [(p #(count nil)) (p #(conj nil 1)) (p #(assoc nil :a 1)) (p #(dissoc nil :a)) (p #(disj nil 1))
                   (p #(peek nil)) (p #(pop nil)) (p #(nth nil 0)) (p #(get nil :a)) (p #(reduce + nil)) (p #(reduce-kv + 7 nil))
                   (p #(count 5)) (p #(conj 5 1)) (p #(assoc 5 :a 1)) (p #(dissoc 5 :a)) (p #(disj 5 1)) (p #(peek 5)) (p #(pop 5))
                   (p #(nth 5 0)) (p #(first 5)) (p #(count (Box. 1)))
                   (p #(count d)) (p #(conj d 1)) (p #(peek d)) (p #(pop d))
                   (p #(conj (map inc [1]) 0)) (p #(nth (map inc [1 2 3]) 2)) (p #(reduce + (map inc [1 2])))]))"#;
    every_level(
        source,
        r#"["0" "(1)" "{:a 1}" "nil" "nil" "nil" "nil" "nil" "nil" "0" "7" "E:count not supported on this type" "E:conj not supported on this type" "E:assoc not supported on this type" "E:dissoc not supported on this type" "E:disj not supported on this type" "E:No protocol method IStack.-peek defined for this value" "E:No protocol method IStack.-pop defined for this value" "E:nth not supported on this type" "E:5 is not ISeqable" "E:count not supported on this type" "42" "[:conj 1]" ":peek" ":pop" "(0 2)" "4" "5"]"#,
    );
    let kept = units(source);
    for unit in ["fallbackMethod", "unsupported"] {
        assert!(kept.contains(&unit), "{unit}: {kept:?}");
    }
    assert!(
        !kept.iter().any(|unit| unit.starts_with("operationPaths.")),
        "{kept:?}"
    );
}

/// The shared steps read a host value's members as each path did (the Sol
/// review of light-cljs-1078): a Proxy over an array counts every property
/// read of each fallback, and `reduce` reads the keyed kind once. Master
/// `15fc27ba`'s counts at each level, where the slices differ.
#[test]
fn fallbacks_read_a_host_value_as_before() {
    let program = program(
        r#"(let [reads (atom 0)
               mk (fn [] (js/Proxy. #js [1 2] #js {:get (fn [t k r] (swap! reads inc) (js/Reflect.get t k r))}))
               c (fn [f] (reset! reads 0) (let [v (try (pr-str (f (mk))) (catch :default e "E"))] [v @reads]))]
           (pr-str [(c #(reduce + 0 %)) (c #(reduce + %)) (c #(reduce-kv (fn [a _ x] (+ a x)) 0 %)) (c count) (c #(nth % 1))
                    (c #(nth % 5 :nf)) (c #(get % 1)) (c #(conj % 3)) (c #(assoc % 0 9)) (c #(dissoc % 0)) (c #(disj % 1))
                    (c peek) (c pop) (c first) (c rest) (c next) (c seq) (c #(= % [1 2])) (c hash) (c #(contains? % 0))]))"#,
    );
    let printed: Vec<String> = OptimizationLevel::ALL
        .into_iter()
        .map(|level| {
            let js = prepare_application(&program, ApplicationOptions::new(level)).emit_js();
            execute(&format!("console.log({js});"))
        })
        .collect();
    let expected = [
        r#"[["3" 15] ["3" 15] ["E" 28] ["2" 6] ["2" 7] [":nf" 6] ["2" 7] ["E" 8] ["E" 4] ["E" 4] ["E" 4] ["E" 4] ["E" 4] ["1" 12] ["(2)" 15] ["(2)" 20] ["(1 2)" 13] ["false" 6] ["1" 14] ["true" 23]]"#,
        r#"[["3" 15] ["3" 15] ["E" 28] ["2" 6] ["2" 7] [":nf" 6] ["2" 7] ["E" 8] ["E" 4] ["E" 4] ["E" 4] ["E" 4] ["E" 4] ["1" 12] ["(2)" 15] ["(2)" 20] ["(1 2)" 13] ["false" 6] ["1" 14] ["true" 23]]"#,
        r#"[["3" 9] ["3" 9] ["E" 8] ["2" 3] ["2" 5] [":nf" 4] ["2" 5] ["E" 7] ["E" 3] ["E" 3] ["E" 2] ["E" 2] ["E" 2] ["1" 6] ["(2)" 9] ["(2)" 11] ["(1 2)" 10] ["false" 2] ["1" 2] ["true" 7]]"#,
    ];
    assert_eq!(printed, expected.map(|line| format!("{line}\n")));
}

/// light-cljs-1079: whether a value is one of the runtime's own
/// (`ownedKind`) reads its keyed kind alone, so a value made by
/// `Object.create` of the prototype of a class without masks (an atom, a
/// var, a function with metadata, a reduced value, a delay, a volatile)
/// answers `satisfies?`, `implements?` and `fn?` as its class's values do by
/// kind, and `extend-type` of `object` or `default` no longer reaches it
/// (docs/value-layout.md, section 5, item 38); a native test that reads its
/// fields (`IDeref`'s) still answers false. Asked first, such a value leaves
/// the genuine values' answers, which the hooks keep by kind
/// (`protocolMethodInfo`'s `kinds`), as master gave them at every level. The
/// store table that tested the class's store is gone from the slice.
#[test]
fn forged_values_of_classes_without_masks_answer_by_kind() {
    let source = r#"(extend-type object ISeqable (-seq [_] (list :o)))
        (extend-type default IMeta (-meta [_] :default-meta))
        (def x 1)
        (let [a (atom 1) v (var x) f (with-meta (fn [] 1) {:m 1}) r (reduced 1) d (delay 1) vo (volatile! 1)
              forge (fn [value] (js/Object.create (js/Object.getPrototypeOf value)))
              ask (fn [value] [(satisfies? IMeta value) (implements? IMeta value) (satisfies? Fn value) (implements? Fn value)
                               (satisfies? ISeqable value) (satisfies? IDeref value) (fn? value)])]
          (pr-str [(mapv ask (mapv forge [a v f r d vo])) (mapv ask [a v f r d vo]) (seq (js-obj)) (meta (js-obj))]))"#;
    every_level(
        source,
        "[[[true true false false false false false] [true true true true false false true] \
         [true true true true false false true] [false false false false false false false] \
         [false false false false false false false] [false false false false false false false]] \
         [[true true false false false true false] [true true true true false true true] \
         [true true true true false false true] [false false false false false true false] \
         [false false false false false true false] [false false false false false true false]] \
         (:o) :default-meta]",
    );
    let units = assert_keeps_none(
        source,
        &[
            "kindTable",
            "storeHolds",
            "storesPredicate",
            "ownedStores",
            "protocolStores",
            "vectorCells",
            "seqCells",
            "viewCells",
            "keywordCells",
            "transientCells",
        ],
    );
    for unit in [
        "ownedKind",
        "protocolHooks.satisfies",
        "builtinCoreImplementation",
    ] {
        assert!(units.contains(&unit), "{unit}: {units:?}");
    }
}
