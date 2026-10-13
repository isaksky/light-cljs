//! Key shapes of map and set literals (light-cljs-220; docs/advanced-optimization.md,
//! "Constant collection literals", tier 4).
//!
//! At `Advanced` a map literal whose keys are distinct constants and whose
//! values are not all constants reads its keys from a pooled shape,
//! `const k3 = rt$mapShape([k0, k1, k2], [<hashes>]);`, and its site passes only
//! the values, `rt$mapFromShape(k3, [k4, l3, l4])`: no key is looked up, compared
//! or hashed per evaluation. Past 8 entries the shape also holds the hash trie
//! `mapFromArray` builds (`rt$hashMapShape`, `rt$hashMapFromShape`). A set
//! literal whose first members are distinct scalar, keyword or symbol constants
//! adds its other members to a pooled shape of the first ones
//! (`rt$setShape`, `rt$setFromShape`), checking them as `literalSetFromArray`
//! does. Below `Advanced` nothing changes, and every observation of a shaped
//! literal (printing, order, equality, hashes, lookups, updates, transients,
//! duplicate errors) is the one a literal built at its site gives, which these
//! tests check at every level in Node and QuickJS, down to the runtime's
//! internal entries and trie nodes.
use super::constant_hashes::agrees;
use super::constant_pool::calls;
use super::generated::{SEEDS, namespace_program};
use super::generated_optimization::analyze as analyze_generated;
use super::optimization::execute;
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use light_cljs::{
    ApplicationOptions, OptimizationLevel, Program, analyze, parse, prepare_application,
};
use std::time::Duration;

fn program(source: &str) -> Program {
    analyze(source, &parse(source).unwrap()).unwrap_or_else(|error| panic!("{source}\n{error}"))
}

fn emit(program: &Program, level: OptimizationLevel) -> String {
    prepare_application(program, ApplicationOptions::new(level)).emit_js()
}

/// The value of the JavaScript string expression `expression` in a fresh
/// QuickJS evaluator.
fn quickjs(expression: &str) -> String {
    let evaluator = Evaluator::new(EvaluatorOptions {
        memory_limit: 1 << 30,
        poll_budget: u64::MAX,
        timeout: Duration::from_secs(300),
        ..EvaluatorOptions::default()
    })
    .unwrap();
    let value = evaluator
        .evaluate_js(expression)
        .unwrap_or_else(|error| panic!("{error:?}"));
    evaluator.string(value).unwrap()
}

/// What the program, whose value is a string, gives at every level, in Node
/// and in QuickJS: the same string, which is returned. `None` is the oracle.
fn every_level(program: &Program) -> String {
    let outputs = OptimizationLevel::ALL.map(|level| {
        let js = emit(program, level);
        let node = execute(&format!("console.log({js});"));
        let quickjs = quickjs(&js);
        assert_eq!(node, format!("{quickjs}\n"), "Node and QuickJS at {level}");
        quickjs
    });
    for (level, output) in OptimizationLevel::ALL.into_iter().zip(&outputs) {
        assert_eq!(output, &outputs[0], "{level} differs from none");
    }
    outputs[0].clone()
}

/// The observations of a map `m` with the keys `ks`: printing, order, size,
/// hash, lookups, updates, transients, metadata and conversions. Every one is
/// the same at every level, so a shaped map behaves as a built one.
const PROBE: &str = r#"
(defn probe [m ks]
  [(pr-str m) (count m) (hash m) (pr-str (keys m)) (pr-str (vals m)) (pr-str (seq m))
   (mapv #(get m % :missing) ks) (pr-str (mapv #(find m %) ks)) (get m :absent :missing)
   (contains? m nil) (get m nil :missing) (pr-str (m (first ks)))
   (pr-str (reduce-kv (fn [acc k v] (conj acc k v)) [] m))
   (pr-str (dissoc m (first ks))) (pr-str (apply dissoc m (take 3 ks)))
   (pr-str (assoc m :new 1 (last ks) :replaced))
   (pr-str (persistent! (reduce dissoc! (transient m) (take-nth 2 ks))))
   (pr-str (persistent! (reduce dissoc! (transient m) (reverse ks))))
   (pr-str (persistent! (assoc! (transient m) :t 1 (first ks) :tt)))
   (pr-str (conj m [:c 3])) (pr-str (merge m {:z 1})) (pr-str (into [] m))
   (pr-str (meta (with-meta m {:m 1}))) (= m (with-meta m {:m 1}))
   (pr-str (select-keys m ks)) (= m (zipmap (keys m) (vals m))) (hash (into {} m))
   (pr-str (map meta (keys m)))])
(defn checked
  "The probe of shaped array map `a`, after checking that it is the probe of
  array map `b` built at its site from the same keys and values in the same
  order, and that the two are `=`."
  [a b ks]
  (let [same (and (= a b) (= b a) (= (probe a ks) (probe b ks))
                  (= (dissoc a (first ks)) (dissoc b (first ks)))
                  (= (persistent! (reduce dissoc! (transient a) (take-nth 2 ks)))
                     (persistent! (reduce dissoc! (transient b) (take-nth 2 ks)))))]
    (if same (probe a ks) (throw (js/Error. (pr-str [:differs (probe a ks) (probe b ks)]))))))
(defn compared
  "The probe of shaped hash map `a`, after checking that it is `=` to hash map
  `b` built at its site from the same keys and values, with the same hash,
  lookups and removals; their operand order may differ."
  [a b ks]
  (let [same (and (= a b) (= b a) (= (hash a) (hash b)) (= (count a) (count b))
                  (= (mapv #(get a % :missing) ks) (mapv #(get b % :missing) ks))
                  (= (apply dissoc a (take 3 ks)) (apply dissoc b (take 3 ks)))
                  (= (persistent! (reduce dissoc! (transient a) (take-nth 2 ks)))
                     (persistent! (reduce dissoc! (transient b) (take-nth 2 ks)))))]
    (if same (probe a ks) (throw (js/Error. (pr-str [:differs (probe a ks) (probe b ks)]))))))
"#;

/// `{0 x 1 y ... (n - 1) x}`, a hash map whose integer keys fill the first
/// trie node past 16 slots, so it becomes an array node.
fn wide(n: usize) -> String {
    let entries = (0..n)
        .map(|key| format!("{key} {}", if key % 2 == 0 { "x" } else { "y" }))
        .collect::<Vec<_>>()
        .join(" ");
    format!("{{{entries}}}")
}

/// Shaped maps of every kind with adversarial keys and values, and maps built
/// from the same keys, whose probes the program returns: array maps and hash
/// maps; nil keys and values, undefined values, one value at several keys;
/// keys that hash alike (`1` and `2147483648`, `0` and `2147483647`, `"Aa"`
/// and `"BB"`, `true` and `1231`) or share their low bits (`1` and `33`);
/// keywords, symbols, strings and numbers spelled alike; collection keys,
/// empty ones and one with metadata; nested shaped maps; and a 40-key map
/// whose first trie node becomes an array node. The first key of a built map
/// `b` goes behind `identity`. Array maps keep source order, so the program
/// compares their probes too; a hash map's operand order follows the reader,
/// which `identity` changes, so the program compares what does not depend on
/// order, and the levels compare its probes.
fn edge_cases() -> String {
    let wide40 = wide(40);
    format!(
        r#"{PROBE}
(defn e1 [x y] {{:type :click :x x :y y}})
(defn b1 [x y] {{(identity :type) :click :x x :y y}})
(defn e2 [x] {{nil x :a nil :b x 1 x 2147483648 (identity nil) :c js/undefined}})
(defn b2 [x] {{(identity nil) x :a nil :b x 1 x 2147483648 (identity nil) :c js/undefined}})
(defn e3 [x] {{:a x "a" x 'a x :ns/a x [1 2] x {{}} x #{{}} x ^:m [3] x}})
(defn b3 [x] {{(identity :a) x "a" x 'a x :ns/a x [1 2] x {{}} x #{{}} x ^:m [3] x}})
(defn e4 [x y] {{:a x :b y :c x :d y :e x :f y :g x :h y :i x}})
(defn b4 [x y] {{(identity :a) x :b y :c x :d y :e x :f y :g x :h y :i x}})
(defn e5 [x y] {{1 x 2147483648 y 33 nil 2147483680 x 0 y 2147483647 x "Aa" x "BB" (identity nil)
               true x 1231 y nil x :k y 'sym x [1 2] y {{}} x #{{}} y [] x 65 y -1 x 1.5 y}})
(defn b5 [x y] {{(identity 1) x 2147483648 y 33 nil 2147483680 x 0 y 2147483647 x "Aa" x "BB" (identity nil)
               true x 1231 y nil x :k y 'sym x [1 2] y {{}} x #{{}} y [] x 65 y -1 x 1.5 y}})
(defn e6 [x] {{:outer {{:inner x :k 1}} :v [x] :w {{:a x :b x :c x :d x :e x :f x :g x :h x :i x :j x}}}})
(defn e7 [x y] {wide40})
(defn e8 [x] {{:a x :b 2 :c [3] :d {{:e 4}}}})
;; The builders escape, so their calls are not inlined (light-cljs-214) and
;; each builds its map in its own body.
(js/Array.of e1 b1 e2 b2 e3 b3 e4 b4 e5 b5 e6 e7 e8)
(let [ks1 [:type :x :y] ks2 [nil :a :b 1 2147483648 :c] ks3 [:a "a" 'a :ns/a [1 2] {{}} #{{}} [3]]
      ks4 [:a :b :c :d :e :f :g :h :i]
      ks5 [1 2147483648 33 2147483680 0 2147483647 "Aa" "BB" true 1231 nil :k 'sym [1 2] {{}} #{{}} [] 65 -1 1.5]
      ks6 [:outer :v :w] ks7 (vec (range 40))]
  (pr-str [(checked (e1 1 2) (b1 1 2) ks1) (checked (e1 nil nil) (b1 nil nil) ks1)
           (checked (e2 7) (b2 7) ks2) (checked (e2 nil) (b2 nil) ks2) (checked (e3 :v) (b3 :v) ks3)
           (compared (e4 1 2) (b4 1 2) ks4) (compared (e4 [1] [1]) (b4 [1] [1]) ks4)
           (compared (e5 1 2) (b5 1 2) ks5) (compared (e5 nil nil) (b5 nil nil) ks5)
           (probe (e6 3) ks6) (probe (e7 1 2) ks7) (probe (e8 1) [:a :b :c :d])
           (= (e4 1 2) (e4 1 2)) (identical? (e1 1 2) (e1 1 2)) (identical? (e8 1) (e8 1))
           (= (e5 1 2) (into {{}} (e5 1 2))) (= (e5 1 2) (apply hash-map (mapcat identity (e5 1 2))))]))"#
    )
}

#[test]
fn shaped_maps_behave_as_maps_built_at_their_site() {
    let program = program(&edge_cases());
    let printed = every_level(&program);
    assert!(
        printed.starts_with("[[\"{:type :click, :x 1, :y 2}\" 3 "),
        "{printed}"
    );
    assert!(
        printed.ends_with(" true false false true true]"),
        "{printed}"
    );
    let advanced = emit(&program, OptimizationLevel::Advanced);
    assert!(advanced.contains("rt$mapFromShape(k"), "{advanced}");
    assert!(advanced.contains("rt$hashMapFromShape(k"));
    // The built maps' first key is not a constant.
    assert!(advanced.contains("rt$mapFromArray([rt$identity("));
}

#[test]
fn keys_the_pool_cannot_tell_apart_are_built_at_their_site() {
    // `{:a 1}` and `{:a 1.0}` are reader-distinct and `=`, so the builder
    // keeps one entry with the first key and the last value, and two map keys
    // with items might be `=`. (Reader-distinct numbers that are `=`, `1` and
    // `1.0`, make a positional literal map instead.) A program that extends a
    // core protocol to a host or runtime type may change `=` and `hash` on
    // keys, so it shapes nothing.
    let source = r#"
      (defn a [x y] {1 x 1.0 y}) (defn b [x y] {0 x -0.0 y :k x})
      (defn c [x y] {{:a 1} x {:a 1.0} y}) (defn d [x y] {{:a 1} x {:b 1} y})
      ;; Escaping, they are not inlined (light-cljs-214).
      (js/Array.of a b c d)
      (pr-str [(a 1 2) (b 1 2) (c 1 2) (d 1 2) (count (a 1 2)) (hash (b 1 2))])"#;
    let program = self::program(source);
    assert_eq!(
        every_level(&program),
        "[{1 1, 1 2} {0 1, -0.0 2, :k 1} {{:a 1} 2} {{:a 1} 1, {:b 1} 2} 2 1786157536]"
    );
    let advanced = emit(&program, OptimizationLevel::Advanced);
    assert!(!advanced.contains("FromShape(k"), "{advanced}");
    // Two sites, and the pooled map keys.
    assert_eq!(advanced.matches("rt$mapFromArray([k").count(), 4);
    assert_eq!(advanced.matches("rt$constantMapFromArray([").count(), 2);

    let extended = format!(
        "(extend-type number IHash (-hash [n] 42))
         (defn e [x] {{:type :click :x x}}) (defn w [x y] {})
         (pr-str [(hash (e 1)) (hash (w 1 2)) (w 1 2) (get (w 1 2) 30)])",
        wide(12)
    );
    let program = self::program(&extended);
    assert_eq!(
        every_level(&program),
        "[-587303405 -1224892369 {0 1, 1 2, 2 1, 3 2, 4 1, 5 2, 6 1, 7 2, 8 1, 9 2, 10 1, 11 2} nil]"
    );
    let advanced = emit(&program, OptimizationLevel::Advanced);
    assert!(!advanced.contains("Shape("), "{advanced}");
}

#[test]
fn set_literals_add_their_other_members_to_a_shape_of_their_first_members() {
    // The reader orders set members, so which members come first depends on
    // their forms: these literals start with constants (the advanced output is
    // checked below). Members equal at run time throw the error that names the
    // later one, also when it is a constant after a dynamic member, `nil` and
    // `undefined` are equal, as are `0` and `-0`, and a set past 8 members goes
    // on in the hash form its shape started.
    let source = r#"
      (defn s1 [x] #{nil x}) (defn s2 [x] #{0 x})
      (defn s3 [b] #{1 2 3 4 5 6 7 8 9 10 11 b}) (defn s4 [c] #{1 2 3 4 5 6 7 8 9 10 11 c})
      (defn s5 [d] #{:a :b :c d}) (defn s6 [a b c d e] #{1 2 3 4 a b c d e})
      (defn s7 [d] #{:a :b [d] d})
      (defn t [f & args]
        (try (let [s (apply f args)]
               [(pr-str s) (count s) (hash s) (pr-str (seq s)) (contains? s nil) (get s 8 :missing)
                (pr-str (disj s 1 2 :a)) (pr-str (conj s :z))
                (pr-str (persistent! (reduce disj! (transient s) (take-nth 2 (seq s)))))
                (= s (set (seq s))) (= s (apply hash-set (seq s)))])
             (catch :default e (.-message e))))
      (pr-str [(t s1 1) (t s1 nil) (t s1 js/undefined) (t s2 1) (t s2 -0) (t s2 0)
               (t s3 12) (t s3 8) (t s3 nil) (t s4 12) (t s4 11) (t s5 :d) (t s5 :a) (t s5 nil)
               (t s6 20 21 22 23 24) (t s6 20 21 1 22 23) (t s6 20 20 21 22 23) (t s6 nil nil 1 2 3)
               (t s7 1) (t s7 [1]) (identical? (s5 :d) (s5 :d))])"#;
    let program = self::program(source);
    let printed = every_level(&program);
    for error in [
        "\"Duplicate key: \"",
        "\"Duplicate key: 0\"",
        "\"Duplicate key: 8\"",
        "\"Duplicate key: 11\"",
        "\"Duplicate key: :a\"",
        "\"Duplicate key: 1\"",
        "\"Duplicate key: 20\"",
        "\"Duplicate key: 3\"",
    ] {
        assert!(printed.contains(error), "{error}: {printed}");
    }
    assert!(printed.ends_with(" false]"), "{printed}");
    // `s7` starts with `[d]` in reader order, so its site builds it.
    let advanced = emit(&program, OptimizationLevel::Advanced);
    assert_eq!(advanced.matches("rt$setFromShape(k").count(), 6);
    assert_eq!(advanced.matches("rt$literalSetFromArray([").count(), 1);
}

/// Runtime internals the structural test compares, exposed on `globalThis`
/// by [`exposed`].
const INTERNALS: &str = "maps, mapKinds, sets, setKinds, hashRoots, hash, keyword, symbol, \
     vectorFromArray, listFromArray, setFromArray, mapFromArray, mapShape, mapFromShape, \
     hashMapShape, hashMapFromShape, setShape, setFromShape, literalSetFromArray";

/// `None` output of a program whose runtime exposes [`INTERNALS`] as
/// `globalThis.lightInternals` when it loads.
fn exposed() -> String {
    let js = emit(&program("[]"), OptimizationLevel::None);
    let exports = "  return Object.freeze({setDeclarationDemand,";
    assert_eq!(js.matches(exports).count(), 1);
    js.replacen(
        exports,
        &format!("  globalThis.lightInternals = {{{INTERNALS}}};\n{exports}"),
        1,
    )
}

/// Builds random literals both ways from pairwise distinct keys, many of which
/// hash alike in full or in their low bits, compares the runtime's entries
/// and trie nodes, and returns a summary, or throws at the first difference.
const STRUCTURE: &str = r#"
const I = globalThis.lightInternals;
let state = 0x2545f491;
const random = n => {
  state ^= state << 13; state >>>= 0; state ^= state >>> 17; state ^= state << 5; state >>>= 0;
  return state % n;
};
const M = 2147483647;
const pool = [null, true, false, 1231, 1237, "Aa", "BB", "AaAa", "BBBB", "AaBB", "BBAa", "", "a",
  1.5, -2.5, I.keyword(null, "a"), I.keyword("n", "a"), I.symbol(null, "a"), I.symbol("n", "a"),
  I.vectorFromArray([1, 2]), I.listFromArray([3]), I.setFromArray([4]), I.mapFromArray([5, 6])];
for (let b = 0; b < 70; b++) pool.push(b, b + M, b + 2 * M, b + 224 + M);
const keysOf = count => {
  const keys = pool.slice();
  for (let i = keys.length - 1; i > 0; i--) { const j = random(i + 1); [keys[i], keys[j]] = [keys[j], keys[i]]; }
  return keys.slice(0, count);
};
const valueOf = () => [null, undefined, 0, "v", pool[random(pool.length)]][random(5)];
const fail = (what, ...detail) => { throw new Error(what + ": " + detail.map(String).join(" ")); };
const sameKeys = (x, y, what) => {
  const a = Object.keys(x).join(), b = Object.keys(y).join();
  if (a !== b) fail(what + " properties", a, b);
};
const sameEntries = (ex, ey) => {
  if (ex.length !== ey.length) fail("entry count", ex.length, ey.length);
  if (Object.isFrozen(ex) !== Object.isFrozen(ey)) fail("frozen entries");
  ex.forEach((x, i) => {
    const y = ey[i];
    sameKeys(x, y, "entry");
    if (!Object.isFrozen(x) || !Object.isFrozen(y)) fail("entry not frozen", i);
    if (x.key !== y.key || x.value !== y.value || x.hash !== y.hash) fail("entry", i, x.key, y.key, x.value, y.value, x.hash, y.hash);
  });
};
const sameNode = (x, y, ex, ey, path) => {
  if (x == null || y == null) { if (x !== y) fail("node", path, x, y); return; }
  sameKeys(x, y, "node " + path);
  if (x.kind !== y.kind) fail("kind", path, x.kind, y.kind);
  if (x.kind === "collision") {
    if (x.hash !== y.hash || x.entries.length !== y.entries.length) fail("collision", path);
    x.entries.forEach((e, i) => { if (ex.indexOf(e) !== ey.indexOf(y.entries[i]) || ex.indexOf(e) < 0) fail("collision entry", path, i); });
    return;
  }
  if (x.kind === "bitmap" && (x.capacity !== y.capacity || (x.edit === null) !== (y.edit === null))) fail("bitmap", path, x.capacity, y.capacity);
  if (x.slots.length !== y.slots.length) fail("slots", path, x.slots.length, y.slots.length);
  for (let i = 0; i < x.slots.length; i++) {
    if ((i in x.slots) !== (i in y.slots)) fail("hole", path, i);
    const s = x.slots[i], t = y.slots[i];
    if (!s || !t) { if (s !== t) fail("slot", path, i); continue; }
    sameKeys(s, t, "slot " + path + "/" + i);
    if (s.node) sameNode(s.node, t.node, ex, ey, path + "/" + i);
    else if (ex.indexOf(s.entry) !== ey.indexOf(t.entry) || ex.indexOf(s.entry) < 0) fail("leaf", path, i);
  }
};
const sameMap = (a, b) => {
  const ea = I.maps.get(a), eb = I.maps.get(b);
  if (I.mapKinds.get(a) !== I.mapKinds.get(b)) fail("map kind", I.mapKinds.get(a), I.mapKinds.get(b));
  if (Object.getPrototypeOf(a) !== Object.getPrototypeOf(b)) fail("map type");
  sameEntries(ea, eb);
  if (I.hashRoots.has(ea) !== I.hashRoots.has(eb)) fail("root presence");
  sameNode(I.hashRoots.get(ea), I.hashRoots.get(eb), ea, eb, "");
};
const sameSet = (a, b) => {
  const ea = I.sets.get(a), eb = I.sets.get(b);
  if (I.setKinds.get(a) !== I.setKinds.get(b)) fail("set kind");
  sameEntries(ea, eb);
  if (I.hashRoots.has(ea) !== I.hashRoots.has(eb)) fail("set root presence");
  sameNode(I.hashRoots.get(ea), I.hashRoots.get(eb), ea, eb, "");
};
const outcome = build => { try { return build(); } catch (error) { return error.message; } };
let maps = 0, hashMaps = 0, sets = 0, duplicates = 0;
for (let trial = 0; trial < 600; trial++) {
  const big = trial % 2 === 1, count = big ? 9 + random(110) : 1 + random(8);
  const keys = keysOf(count), values = keys.map(valueOf), hashes = keys.map(key => key == null ? 0 : I.hash(key));
  const built = I.mapFromArray(keys.flatMap((key, i) => [key, values[i]]), big ? "hash-map" : "array-map");
  const shape = big ? I.hashMapShape(keys.slice(), hashes.slice()) : I.mapShape(keys.slice(), hashes.slice());
  const fromShape = big ? I.hashMapFromShape : I.mapFromShape;
  sameMap(built, fromShape(shape, values));
  // A shape builds any number of maps, which share no entries or nodes.
  const other = fromShape(shape, values.map(valueOf));
  sameMap(built, fromShape(shape, values));
  if (I.maps.get(other)[0] === I.maps.get(built)[0] || (big && I.hashRoots.get(I.maps.get(other)) === I.hashRoots.get(I.maps.get(built)))) fail("shared");
  if (big) hashMaps++; else maps++;
}
for (let trial = 0; trial < 600; trial++) {
  const first = keysOf(1 + random(trial % 3 === 0 ? 20 : 8));
  const rest = Array.from({length: 1 + random(12)}, () => random(4) === 0 ? first[random(first.length)] : pool[random(pool.length)]);
  const built = outcome(() => I.literalSetFromArray(first.concat(rest)));
  const shape = I.setShape(first.slice());
  const shaped = outcome(() => I.setFromShape(shape, rest));
  if (typeof built === "string" || typeof shaped === "string") {
    if (built !== shaped) fail("set outcome", built, shaped);
    duplicates++;
  } else {
    sameSet(built, shaped);
    sameSet(built, I.setFromShape(shape, rest));
  }
  sets++;
}
return `${maps} array maps, ${hashMaps} hash maps, ${sets} sets, ${duplicates} duplicate errors`;
"#;

#[test]
fn shapes_build_the_entries_and_tries_the_builders_build() {
    // The strongest form of "behaves as built at its site": the runtime's
    // private entries (key, value, hash, frozen) and hash trie nodes (kind,
    // slots and holes, capacity, collision order) of a shaped map or set equal
    // those `mapFromArray` and `literalSetFromArray` build from the same keys
    // in the same order, so every later operation, transients included, sees
    // the same structure. Set trials repeat first members among the others,
    // and both builds must then throw the same error.
    let js = exposed();
    let expression = format!("(() => {{ const value = {js};\n{STRUCTURE} }})()");
    let summary = quickjs(&expression);
    assert_eq!(
        execute(&format!("console.log({expression});")),
        format!("{summary}\n")
    );
    assert!(
        summary.starts_with("300 array maps, 300 hash maps, 600 sets, "),
        "{summary}"
    );
    let duplicates: usize = summary
        .rsplit_once(", ")
        .and_then(|(_, last)| last.split(' ').next()?.parse().ok())
        .unwrap();
    assert!((100..500).contains(&duplicates), "{summary}");
}

#[test]
fn shape_key_hashes_are_the_runtime_hashes() {
    // Each shape embeds its keys' hashes from `src/value_hash.rs`; the
    // constant hash check of light-cljs-219 also compares those with the
    // runtime's, in Node and QuickJS (see `constant_hashes::agrees`).
    let advanced = emit(&program(&edge_cases()), OptimizationLevel::Advanced);
    let checked = agrees(&advanced, "edge cases");
    assert!(checked > 100, "{checked}");
}

#[test]
fn event_map_allocations_per_evaluation_are_pinned() {
    // The fixed allocation fixture: calls of the builders and of what they
    // call per key, over 1,000 evaluations of an event map, a 10-entry hash map
    // and a set whose first member is a constant. Below `Advanced` every
    // evaluation interns its keywords and puts each key, comparing it with
    // the keys before it; at `Advanced` the keys and the trie are built once,
    // with their shapes, and each evaluation only makes the entries and copies
    // the trie, and puts only the set's other member.
    let source = r#"
      (defn event [i] {:type :click :x i :y (inc i) :target "button"})
      (defn wide [i] {:a i :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h 8 :i 9 :j i})
      (defn tagged [i] #{nil i})
      (defn total [n]
        (loop [i 0 acc 0]
          (if (< i n)
            (recur (inc i) (+ acc (:x (event i)) (count (wide i)) (count (tagged i))))
            acc)))
      (total 1000)"#;
    let names = [
        "keyword",
        "mapFromArray",
        "mapFromShape",
        "hashMapFromShape",
        "literalSetFromArray",
        "setFromShape",
        "putEntry",
        "putMember",
        "entryIndex",
        "hashNodeAssoc",
    ];
    let program = program(source);
    let expected = [
        r#"{"keyword":16000,"mapFromArray":2000,"mapFromShape":0,"hashMapFromShape":0,"literalSetFromArray":1000,"setFromShape":0,"putEntry":16000,"putMember":2000,"entryIndex":7000,"hashNodeAssoc":16000}"#,
        r#"{"keyword":16000,"mapFromArray":2000,"mapFromShape":0,"hashMapFromShape":0,"literalSetFromArray":1000,"setFromShape":0,"putEntry":16000,"putMember":2000,"entryIndex":7000,"hashNodeAssoc":16000}"#,
        r#"{"keyword":15,"mapFromArray":0,"mapFromShape":1000,"hashMapFromShape":1000,"literalSetFromArray":1000,"setFromShape":1000,"putEntry":1001,"putMember":1001,"entryIndex":2001,"hashNodeAssoc":16}"#,
    ];
    for (level, expected) in OptimizationLevel::ALL.into_iter().zip(expected) {
        assert_eq!(
            calls(&emit(&program, level), &names),
            ("511500".into(), expected.into()),
            "{level}"
        );
    }
    // The shapes, with the keys' hashes, and the sites, which pass the values
    // and the set's other member in their operand order. Each builder is
    // called once, so its site is inlined into the loop (light-cljs-214), where
    // `:x` is read first.
    let advanced = emit(&program, OptimizationLevel::Advanced);
    let shaped: Vec<_> = advanced
        .lines()
        .map(str::trim)
        .filter(|line| line.contains("Shape(") && line.contains(" rt$"))
        .collect();
    assert_eq!(
        shaped,
        [
            "const k4 = rt$mapShape([k1, k0, k2, k3], [1174270348, 2099068185, -1757859776, 253001721]);",
            "const k16 = rt$hashMapShape([k6, k7, k8, k9, k10, k11, k12, k13, k14, k15], \
             [1381269198, 1738089905, -1763192079, -1397974765, 1109658740, 1482224470, \
             1972142424, -1597136552, -1386841315, -2123407586]);",
            "const k17 = rt$setShape([null]);",
            "const next1 = (l5 + (k0).call(null, rt$mapFromShape(k4, [k5, l4, (l4 + 1), \"button\"])) \
             + rt$count(rt$hashMapFromShape(k16, [5, 7, 3, l4, 8, 2, 4, 6, 9, l4])) \
             + rt$count(rt$setFromShape(k17, [l4])));",
        ]
    );
}

#[test]
fn generated_programs_build_shaped_maps_and_sets() {
    // The generator's shape reads (`Builder::shape_read` in
    // tests/support/generated.rs) compare each shaped literal with one built at
    // its site, and `generated_optimization` compares the levels; this pins
    // that the fixed seeds do build shaped maps of both kinds and shaped sets
    // at `Advanced`.
    let mut sites = [0; 3];
    for seed in SEEDS {
        let program = namespace_program(seed);
        let js = emit(
            &analyze_generated(&program, &program.sources(&|_| false, false)),
            OptimizationLevel::Advanced,
        );
        for (count, site) in sites.iter_mut().zip([
            "rt$mapFromShape(k",
            "rt$hashMapFromShape(k",
            "rt$setFromShape(k",
        ]) {
            *count += js.matches(site).count();
        }
    }
    assert!(sites.iter().all(|&count| count >= 3), "{sites:?}");
}
