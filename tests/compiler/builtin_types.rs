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
                       slot "cljs$core$ICounted$_count$arity$1"
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
                                 value "cljs$core$ILookup$_lookup$arity$3"
                                 #js {:get (fn []
                                             (js-delete value "cljs$core$ILookup$")
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
    "publicMethod",
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
    "publicMethod",
    "externalMethod",
    "protocolHooks.extend",
    "protocolHooks.publicMethod",
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
    assert!(advanced(source).emit_js().len() < 8_000);
    every_level(source, "7");
    let extended = "(deftype T [] ICounted (-count [_] 7)) (extend-type T ISeqable (-seq [_] nil)) (count (T.))";
    let units = self::units(extended);
    for unit in [
        "publicMethod",
        "protocolHooks.publicMethod",
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
        assert!(advanced(source).emit_js().len() < 12_000, "{source}");
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
/// as implementing a protocol only where it declares it, as `publicMethod`
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
           (aset f \"cljs$core$IMeta$_meta$arity$1\" (fn [_] {:m 1}))
           (pr-str [(get (reify ILookup (-lookup [_ k] 1)) \"x\")
                    (get (js-obj \"cljs$core$ILookup$_lookup$arity$2\" (fn [_ k] 7)) \"x\")
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
         (try (.-done (.next (.call (.-keys {:a 1}) (->R 1)))) (catch js/TypeError _ :unsupported))";
    every_level("(defrecord R [x]) (.-done (.next (.keys {:a 1})))", "false");
    let printed = execute(&format!("console.log({});", advanced(borrowed).emit_js()));
    assert!(
        ["false\n", "KeywordValue {}\n"].contains(&printed.as_str()),
        "{printed}"
    );
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
                            [:masked (js-obj \"cljs$lang$protocol_mask$partition0$\" 2)]
                            [:marker (js-obj \"cljs$core$ICounted$\" true)] {extra}]]
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
            "publicMethod",
            "externalMethod",
            "protocolHooks.publicMethod",
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
    // 12,375 and 12,576 bytes before.
    for source in [
        "(deftype T [] ICounted (-count [_] 7)) (counted? (T.))",
        "(defrecord R [x]) (map? (->R 1))",
    ] {
        assert_keeps_none(source, &EXTENSION_MACHINERY);
        assert!(advanced(source).emit_js().len() < 6_500, "{source}");
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
         (let [o (js-obj \"cljs$lang$protocol_mask$partition0$\" 2)]
           (js/Object.defineProperty o \"cljs$core$ICounted$\"
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
    // stores' `privateStore`, since light-cljs-259, about 320 more than theirs.
    assert!(advanced("(count (.split \"a,b\" \",\"))").emit_js().len() < 1_900);
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
        (
            "(let [s (seq (sorted-map 1 2 3 4))] (count (PersistentTreeMapSeq. nil (.-stack s) true -1 nil)))",
            "1",
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
    assert!(units.contains(&"treeMapSeqConstructor.count"), "{units:?}");
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
             (let [o (js-obj \"cljs$lang$protocol_mask$partition0$\" 8388608
                             \"cljs$core$ISeqable$_seq$arity$1\" (fn [_] nil))]
               (array (count (T.)) (count o)))",
            "[ 7, 0 ]",
        ),
        (
            "(count (js-obj \"cljs$lang$protocol_mask$partition0$\" 8388608
                            \"cljs$core$ISeqable$_seq$arity$1\" (fn [_] (list 1 2))))",
            "2",
        ),
        (
            "(deftype T [] ICounted (-count [_] 7)) (def n (count (T.)))
             (try (count (js-obj \"cljs$lang$protocol_mask$partition0$\" 8388608))
               (catch :default e (.-message e)))",
            "No protocol method ISeqable/-seq defined for this value",
        ),
        (
            "(try (count (js-obj \"cljs$lang$protocol_mask$partition0$\" 2))
               (catch :default e (.-message e)))",
            "No protocol method ICounted/-count defined for this value",
        ),
    ] {
        every_level(source, expected);
    }
    let masked = self::units("(count (js-obj \"cljs$lang$protocol_mask$partition0$\" 2))");
    for unit in ["installMethods", "ISeqable", "ISequential", "count.seqs"] {
        assert!(masked.contains(&unit), "{unit}: {masked:?}");
    }
    // A mask whose name the program computes is outside the contract: where
    // nothing implements `ISeqable`, `count` throws a `TypeError` naming the
    // hook at `Advanced`.
    let computed = "(deftype T [] ICounted (-count [_] 7)) (def n (count (T.)))
         (let [o (js-obj)]
           (aset o (.concat \"cljs$lang$protocol_mask$\" \"partition0$\") 8388608)
           (try (count o) (catch js/TypeError _ :unsupported) (catch :default e (.-message e))))";
    let printed = execute(&format!("console.log({});", advanced(computed).emit_js()));
    assert_eq!(printed, "KeywordValue {}\n");
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
           (array (.has s 1) (.has s 3) (.equiv s #{2 1}) (.call s nil 2) (.call s nil 3 :none)
                  (.-length (js/Array.from s))))",
        "[ true, false, true, 2, KeywordValue {}, 2 ]",
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
                  (key (first {:a 1})) (val (first {:a 1})) (pr-str (keys {:a 1})) (pr-str (find {:a 1} :a))
                  (count (persistent! (reduce conj! (transient {}) (map vector (range 9) (range 9)))))))",
        "[ 2, 3, 1, 1, 3, 32, KeywordValue {}, 1, '(:a)', '[:a 1]', 9 ]",
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
           (aset v \"cljs$lang$protocol_mask$partition0$\" 8)
           (aset v \"cljs$core$ICollection$_conj$arity$2\" (fn [_ x] [:custom x]))
           (pr-str [(conj v 2) (conj [5] 6) (into [] (range 3))]))",
        "[[:custom 2] [5 6] [0 1 2]]",
    );
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
         (let [p (Pair. :x :y)] (array (pr-str (seq p)) (count p) (nth p 1) (pr-str (into [] p)) (= [:x :y] (vec p))))",
        "[ '(:x :y)', 2, KeywordValue {}, '[:x :y]', true ]",
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
            "PersistentHashMapValue.brand",
            "hashTrie",
            "trieEntry",
            "lookup.trie",
            "copyEntries.trie",
            "hashNodeAssoc",
        ],
    );
    every_level(source, "1 1\nundefined");
    let units = units("(js/console.log (count (hash-map :a 1)))");
    for unit in ["hashTrie", "PersistentHashMapValue.brand"] {
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
const HOST_FACING_MEMBERS: [&str; 7] = [
    "apply",
    "call",
    "constructor",
    "equiv",
    "indexOf",
    "lastIndexOf",
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
                  (sorted-map 1 2) (sorted-set 1) (seq (sorted-map 1 2)) (eduction (map inc) [1])
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
    let upstream: Vec<&str> = printed
        .split_whitespace()
        .filter(|name| !name.starts_with("cljs$") && !HOST_FACING_MEMBERS.contains(name))
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
            "upstreamMembers",
            "publicFields",
            "LazySeq.fields",
            "ChunkedCons.fields",
            "IntegerRangeValue.fields",
        ],
    );
    assert!(unnamed.contains(&"LazySeq.members") && unnamed.contains(&"IntegerRangeValue.members"));
    // A program naming one as a property, method or string, or in `js*`
    // text, reads the fields as `None` does.
    for (source, expected, fields) in [
        (
            "(let [c (cons 1 (list 2 3))] (array (.-first c) (count (.-rest c)) (.-__hash c)))",
            "[ 1, 2, null ]",
            "Cons.fields",
        ),
        (
            "(let [s (lazy-seq [1 2])] (array (count (.sval s)) (count (.-s s))))",
            "[ 2, 2 ]",
            "LazySeq.fields",
        ),
        ("(aget (range 3) \"cnt\")", "3", "IntegerRangeValue.fields"),
        ("(js* \"~{}.first\" (cons 5 nil))", "5", "Cons.fields"),
    ] {
        every_level(source, expected);
        let units = units(source);
        assert!(
            units.contains(&"upstreamMembers") && units.contains(&fields),
            "{source} keeps {units:?}"
        );
    }
    // `alter-meta!` reads and assigns `meta`.
    for (source, expected, fields) in [
        (
            "(let [l (with-meta (list 1 2) {:a 1})] (pr-str (alter-meta! l assoc :b 2)))",
            "{:a 1, :b 2}",
            "ListValue.fields",
        ),
        (
            "(let [l (with-meta (lazy-seq [1]) {:a 1})] (pr-str (alter-meta! l assoc :b 2) (meta l)))",
            "{:a 1, :b 2} {:a 1, :b 2}",
            "LazySeq.fields",
        ),
    ] {
        every_level(source, expected);
        let units = units(source);
        assert!(
            !units.contains(&"upstreamMembers") && units.contains(&fields),
            "{source} keeps {units:?}"
        );
    }
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
            "(array (.equiv [1] [1]) (.call (aget r \"cljs$core$ILookup$_lookup$arity$3\") r \"missing\" \"nf\"))",
            "[ true, undefined ]",
            "recordMethods.lookup",
        ),
        (
            "(.call (aget r \"cljs$core$ILookup$_lookup$arity$3\") r r \"missing\" \"nf\")",
            "nf",
            "recordMethods.lookup",
        ),
        (
            "(.cljs$core$ILookup$_lookup$arity$2 r r \"x\")",
            "null",
            "recordMethods.lookup",
        ),
        (
            "(js* \"~{}.cljs$core$ICounted$_count$arity$1(~{})\" r r)",
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
        "(js* \"[~{}.cljs$core$IMeta$_meta$arity$1(~{}), ~{}.cljs$core$ICounted$_count$arity$1(~{})]\" r r r r)"
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
    every_level(
        "(defn ^{:test (fn [] :ok)} f [] 1)
         (array (pr-str (meta (with-meta [1] {:a 1}))) (some? (:test (meta #'f))) (.-meta (with-meta [1] {:a 2}))
                (pr-str (meta (conj (with-meta [1] {:b 1}) 2))) (nil? (.-meta [1])))",
        "[ '{:a 1}', true, PersistentArrayMapValue {}, '{:b 1}', true ]",
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
    let kept = units("(js/console.log (first (map inc (list 1 2))))");
    for unit in [
        "first.branches",
        "first.cells",
        "LazySeq.first",
        "seq.branches",
        "seq.cells",
    ] {
        assert!(kept.contains(&unit), "{unit}: {kept:?}");
    }
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

/// Where the wrappers exist, `first` of a view (an IndexedSeq or a
/// ChunkedSeq, kind 3) reads the view's branch (light-cljs-237), which gives
/// at every level what the operation's path gives: of a public constructor's
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
    let kept = units("(js/console.log (first (map inc (seq [1 2]))))");
    for unit in [
        "first.views",
        "IndexedSeqValue.viewKind",
        "ChunkedSeqValue.viewKind",
    ] {
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

/// An `ISeqable` marker and method that JavaScript writes on a runtime seq
/// give what the operations' own paths give where protocol implementations
/// exist, as the branches leave every value to those paths there: a value
/// that is already a seq is not asked for its seq, as upstream's is not.
/// Expected values are master's at `none`.
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
