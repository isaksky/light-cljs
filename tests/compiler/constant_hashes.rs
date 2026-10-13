//! Cached collection hashes and the precomputed hashes of pooled constants
//! (light-cljs-219; docs/advanced-optimization.md, "Constant collection
//! literals", tier 3).
//!
//! The runtime keeps the hash of every persistent vector, map, set, list and
//! queue once computed. At `Advanced` each composite constant of the pool is
//! declared with the hash `src/value_hash.rs` computes for it,
//! `rt$hashed(rt$vectorFromArray([1, 2]), 1185709346)`, so its first hash is a
//! lookup too. The embedded hashes must be the runtime's own, bit for bit:
//! [`checks`] rewrites the output's `hashed` to compare the embedded hash with
//! the one the runtime computes instead of storing it, and runs the output in
//! Node and in QuickJS. That covers hand-picked edge cases, seeded random
//! scalars, every constant literal of the language and execution corpora and
//! the generated programs of light-cljs-217. The generator's literal reads
//! also check that `=` implies equal hashes across pooled and built values.
use super::generated::{namespace_program, seeds};
use super::generated_optimization::analyze;
use super::optimization::{execute, readable};
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use light_cljs::{
    ApplicationOptions, OptimizationLevel, Program, analyze as analyze_source, parse,
    prepare_application,
};
use std::io::Write as _;
use std::process::{Command, Stdio};
use std::time::Duration;

fn program(source: &str) -> Program {
    analyze_source(source, &parse(source).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n{error}"))
}

fn emit(program: &Program, level: OptimizationLevel) -> String {
    prepare_application(program, ApplicationOptions::new(level)).emit_js()
}

/// The readable output (`pseudo_names`), whose text the checks read and
/// whose runtime they patch.
fn emit_readable(program: &Program, level: OptimizationLevel) -> String {
    prepare_application(program, readable(level)).emit_js()
}

/// The program prints `expected` (its value, through `console.log`) at every
/// level.
fn every_level(source: &str, expected: &str) {
    let program = program(source);
    for level in OptimizationLevel::ALL {
        assert_eq!(
            execute(&format!("console.log({});", emit(&program, level))),
            format!("{expected}\n"),
            "at {level}: {source}"
        );
    }
}

/// Run a Node script given on stdin, which, unlike `--eval`, has no size limit.
fn node(script: &str) -> String {
    let mut child = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("constant hash tests require Node");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
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
    evaluator
        .string(evaluator.evaluate_js(expression).unwrap())
        .unwrap()
}

/// What the rewritten `hashed` saw: how many embedded hashes it checked and
/// the ones that differ from the runtime's, as `embedded != runtime`.
#[derive(Debug, PartialEq, Eq)]
struct Checks {
    checked: usize,
    mismatches: Vec<String>,
}

/// The check that replaces the body of the runtime's `hashed`: it hashes the
/// value with the runtime's `hash`, which caches that hash, and records a
/// mismatch, so containing constants are checked against runtime hashes of
/// their parts. The output must embed the `hash` unit.
const CHECK: &str = "const actual = hash(value); globalThis.lightHashed.checked++; \
     if (actual !== cached) globalThis.lightHashed.mismatches.push(cached + \" != \" + actual); \
     return value; ";

/// The check added to the runtime's map key shape constructors
/// (light-cljs-220), which receive the keys and their embedded hashes: each
/// key's runtime hash is compared with its embedded one.
const SHAPE_CHECK: &str = "keys.forEach((key, i) => { const actual = hash(key); \
     globalThis.lightHashed.checked++; if (actual !== hashes[i]) \
     globalThis.lightHashed.mismatches.push(hashes[i] + \" != \" + actual + \" (a shape key)\"); }); ";

/// The map key shape constructors whose embedded hashes [`SHAPE_CHECK`] checks.
const SHAPES: [&str; 2] = [
    "  function mapShape(keys, hashes) {",
    "  function hashMapShape(keys, hashes) {",
];

/// The number of key hashes the map key shapes of `js` embed: the last array
/// of each shape's declaration, which holds only integers.
fn shape_hashes(js: &str) -> usize {
    js.lines()
        .filter(|line| line.contains("rt$mapShape([") || line.contains("rt$hashMapShape(["))
        .map(|line| line[line.rfind('[').unwrap()..].split(", ").count())
        .sum()
}

/// Run `Advanced` output `js` with its `hashed` rewritten by [`CHECK`] and its
/// map key shape constructors by [`SHAPE_CHECK`], in Node and in QuickJS, and
/// return what each saw. Whatever the program does after building its
/// constants (it may throw) is ignored.
fn checks(js: &str) -> [Checks; 2] {
    assert_eq!(
        js.matches("  function hash(value) {").count(),
        1,
        "the output embeds `hash`"
    );
    let mut js = js.to_owned();
    if js.contains("rt$hashed(") {
        let header = "  function hashed(value, cached) { ";
        assert_eq!(js.matches(header).count(), 1, "the output embeds `hashed`");
        js = js.replacen(header, &format!("{header}{CHECK}"), 1);
    }
    for header in SHAPES {
        assert!(js.matches(header).count() <= 1, "{header}");
        js = js.replacen(header, &format!("{header} {SHAPE_CHECK}"), 1);
    }
    let expression = format!(
        "(() => {{\n globalThis.lightHashed = {{checked: 0, mismatches: []}};\n \
         try {{ {js} }} catch (error) {{}}\n \
         const {{checked, mismatches}} = globalThis.lightHashed;\n \
         return checked + \"\\n\" + mismatches.join(\"\\n\");\n}})()"
    );
    let parse = |output: &str| {
        let mut lines = output.trim_end().lines();
        Checks {
            checked: lines.next().unwrap().parse().unwrap(),
            mismatches: lines.map(str::to_owned).collect(),
        }
    };
    [
        parse(&node(&format!("console.log({expression});"))),
        parse(&quickjs(&expression)),
    ]
}

/// Check every embedded hash of `Advanced` output `js` in both engines and
/// return how many there are: each constant declared with one is built, and a
/// mismatch names its declaration. Since light-cljs-220 the key hashes of map
/// key shapes count too.
pub(crate) fn agrees(js: &str, context: &str) -> usize {
    let embedded = js.matches("rt$hashed(").count() + shape_hashes(js);
    if embedded == 0 {
        return 0;
    }
    for (engine, checks) in ["Node", "QuickJS"].into_iter().zip(checks(js)) {
        let declarations: Vec<_> = checks
            .mismatches
            .iter()
            .map(|mismatch| {
                let embedded = mismatch.split(' ').next().unwrap();
                let suffix = format!(", {embedded});");
                let line = js
                    .lines()
                    .find(|line| line.starts_with("  ") && line.ends_with(&suffix))
                    .unwrap_or("?");
                format!("{mismatch}: {line}")
            })
            .collect();
        assert!(
            declarations.is_empty(),
            "{context}: {engine} computes other hashes:\n{}",
            declarations.join("\n")
        );
        assert_eq!(
            checks.checked, embedded,
            "{context}: {engine} built {} of {embedded} hashed constants",
            checks.checked
        );
    }
    embedded
}

/// The program `[cljs.core/hash <items>]`, whose value keeps the runtime's
/// `hash` and the constants among `items`.
fn kept(items: &str) -> Program {
    program(&format!("[cljs.core/hash\n{items}]"))
}

#[test]
fn edge_case_constants_embed_the_runtime_hash() {
    // Each scalar sits in a vector of its own, so its hash is checked alone;
    // the collections nest, carry metadata, merge equal keys or keep
    // reader-distinct ones, and one is a lazy cell.
    let big = (0..1100)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    let items = format!(
        r##"[[-0.0] [0] [##NaN] [##Inf] [##-Inf] [1.5] [0.1] [-1.5e-10] [5e-324] [1.7976931348623157e308]
   [9007199254740991] [-9007199254740991] [9007199254740992] [9007199254740993] [1e21]
   [2147483647] [2147483648] [-2147483648] [-2147483649] [4294967295] [4294967296]
   [12345678901234] [-7] [1e-7]
   [""] ["a"] ["ab"] ["abc"] ["\uD83D\uDE00"] ["😀x"] ["\uD800"] ["\uDC00x"] ["é"] ["日本語"]
   ["a\u0000b"] [\a] [\newline]
   [:a] [:a/b] [:ns.x/name-y] [:é/λ] ['a] ['a/b] ['ns.x/y] [nil] [true] [false]
   [[]] [{{}}] [#{{}}] [()] ['()] [[[]]] [{{}} #{{}} [] ()] [[] ()] ['([] ())]
   {{1 :a 1.0 :b}} #{{1 1.0}} {{[1] :a '(1) :b}} {{[] :a '() :b}} {{0 :a -0.0 :b}} #{{0 -0.0}}
   (array-map) (hash-set) (array-map 1 2 1.0 3) (hash-set "a" \a)
   {{:a 1 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h 8 :i 9}} #{{1 2 3 4 5 6 7 8 9 10}}
   {{nil 1 false 2 true 3 "" 4 0 5 :k 6 'sym 7 [] 8 {{}} 9 #{{}} 10}}
   ^{{:m 1}} [1 2] ^:x {{:a 1}} ^:y #{{:s}} '^:z sym [^:w [1] ^:v ^:u [2]]
   '(1 (2 3) [4] {{:a 5}} #{{6}}) '{{a [1 2] b (3 4)}} '#{{a b c}} '{{a 1 b 2}}
   #uuid "00000000-0000-0000-0000-00000000000A" [#uuid "00000000-0000-0000-0000-00000000000b"]
   {{#uuid "00000000-0000-0000-0000-00000000000c" 1}}
   {{:a {{:b {{:c [1 2 {{:d #{{:e "f"}}}}]}}}}}} [[[[[[1]]]]]]
   [[7 8 9] {big}]]"##
    );
    let js = emit_readable(&kept(&items), OptimizationLevel::Advanced);
    // The lazy cell is hashed too.
    assert!(js.contains("??= rt$hashed(rt$vectorFromArray([k"));
    let checked = agrees(&js, "edge cases");
    assert!(checked > 100, "{checked}");
}

/// A deterministic xorshift generator for the random scalars.
struct Random(u64);
impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// A source literal that reads as `value`, a finite number.
fn number_literal(value: f64) -> String {
    if value == 0.0 && value.is_sign_negative() {
        "-0.0".into()
    } else {
        format!("{value:e}")
    }
}

/// A source string literal for UTF-16 code units, escaping all but
/// printable ASCII.
fn string_literal(units: &[u16]) -> String {
    let mut out = String::from("\"");
    for &unit in units {
        match unit {
            0x20..=0x7e if unit != u16::from(b'"') && unit != u16::from(b'\\') => {
                out.push(char::from(unit as u8))
            }
            _ => out.push_str(&format!("\\u{unit:04X}")),
        }
    }
    out.push('"');
    out
}

#[test]
fn random_scalars_embed_the_runtime_hash() {
    // Numbers from random bit patterns, integers of every magnitude and short
    // decimals; strings of ASCII, BMP, surrogate pairs and lone surrogates;
    // keywords and symbols with and without namespaces.
    let mut random = Random(0x219_5eed);
    let mut literals = Vec::new();
    for _ in 0..300 {
        let value = f64::from_bits(random.next());
        if value.is_finite() {
            literals.push(number_literal(value));
        }
    }
    for bits in 0..64 {
        let magnitude = (random.next() >> (63 - bits)) as f64;
        let sign = if random.below(2) == 0 { 1.0 } else { -1.0 };
        literals.push(number_literal(sign * magnitude));
        literals.push(number_literal(
            sign * (random.below(100_000) as f64) / 1000.0,
        ));
    }
    for _ in 0..200 {
        let units: Vec<u16> = (0..random.below(9))
            .flat_map(|_| match random.below(6) {
                0 | 1 => vec![0x20 + random.below(0x5f) as u16],
                2 => vec![0xa0 + random.below(0xd700) as u16],
                3 => vec![0xd800 + random.below(0x400) as u16],
                4 => vec![0xdc00 + random.below(0x400) as u16],
                _ => {
                    let code = 0x10000 + random.below(0x10_0000) as u32;
                    let mut pair = [0; 2];
                    char::from_u32(code)
                        .unwrap()
                        .encode_utf16(&mut pair)
                        .to_vec()
                }
            })
            .collect();
        literals.push(string_literal(&units));
    }
    const LETTERS: [&str; 12] = [
        "a", "b", "z", "x1", "-", "*", "?", "é", "λ", "日", "ab-c", "q.r",
    ];
    let name = |random: &mut Random| {
        (0..1 + random.below(3))
            .map(|_| LETTERS[random.below(LETTERS.len() as u64) as usize])
            .collect::<String>()
    };
    for _ in 0..150 {
        let mut spelling = format!("n{}", name(&mut random));
        if random.below(2) == 0 {
            spelling = format!("n{}/{spelling}", name(&mut random));
        }
        literals.push(if random.below(2) == 0 {
            format!(":{spelling}")
        } else {
            format!("'{spelling}")
        });
    }
    let items = literals
        .iter()
        .map(|literal| format!("[{literal}]"))
        .collect::<Vec<_>>()
        .join(" ");
    let checked = agrees(
        &emit_readable(&kept(&items), OptimizationLevel::Advanced),
        "random scalars",
    );
    assert!(checked > 700, "{checked}");
}

/// The sources of the corpora's rows: compiler-language.tsv and the
/// execution corpus.
fn corpus() -> Vec<String> {
    let rows = |text: &'static str| {
        text.lines()
            .filter(|line| !line.starts_with('#') && !line.is_empty())
            .map(|line| line.split('\t').nth(1).unwrap())
    };
    let hex = |text: &str| {
        let (pairs, _) = text.as_bytes().as_chunks::<2>();
        String::from_utf8(
            pairs
                .iter()
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect(),
        )
        .unwrap()
    };
    rows(include_str!("../fixtures/compiler-language.tsv"))
        .chain(rows(include_str!("../fixtures/execution.tsv")))
        .chain(rows(include_str!("../fixtures/execution-outcomes.tsv")))
        .map(str::to_owned)
        .chain(rows(include_str!("../fixtures/execution-literals.tsv")).map(hex))
        .collect()
}

/// `rows` as one program, each row the body of a function nothing calls, so
/// that its constants are built and nothing else runs.
fn functions(rows: &[&str]) -> String {
    let mut source = String::from("[cljs.core/hash");
    for row in rows {
        source.push_str("\n (fn [] ");
        source.push_str(row);
        source.push(')');
    }
    source.push(']');
    source
}

/// Check the embedded hashes of `rows` as one program. A chunk that does not
/// compile, or that extends a core protocol beyond its own top-level types
/// (which turns precomputation off), is split; such a row alone is skipped.
fn rows_agree(rows: &[&str]) -> usize {
    let source = functions(rows);
    let prepared = parse(&source)
        .ok()
        .and_then(|syntax| analyze_source(&source, &syntax).ok())
        .map(|program| {
            let prepared = prepare_application(&program, readable(OptimizationLevel::Advanced));
            let intact = prepared
                .inference()
                .is_some_and(|inference| inference.facts().core_protocols_intact());
            (prepared, intact)
        });
    match prepared {
        Some((prepared, true)) => agrees(
            &prepared.emit_js(),
            &format!("corpus rows\n{}", rows.join("\n")),
        ),
        _ if rows.len() == 1 => 0,
        _ => {
            let (left, right) = rows.split_at(rows.len() / 2);
            rows_agree(left) + rows_agree(right)
        }
    }
}

#[test]
fn corpus_constants_embed_the_runtime_hash() {
    // Rows that do not compile as a function body (namespaces, macros, reader
    // errors) are left out, and so are rows that define or extend types in
    // it (see `rows_agree`).
    let corpus = corpus();
    let rows: Vec<&str> = corpus
        .iter()
        .map(String::as_str)
        .filter(|row| {
            let source = functions(&[row]);
            parse(&source).is_ok_and(|syntax| analyze_source(&source, &syntax).is_ok())
        })
        .collect();
    assert!(
        rows.len() > 1500,
        "{} of {} rows compile",
        rows.len(),
        corpus.len()
    );
    let checked: usize = rows.chunks(150).map(rows_agree).sum();
    eprintln!(
        "{checked} embedded hashes in {} of {} corpus rows",
        rows.len(),
        corpus.len()
    );
    assert!(checked > 1000, "{checked}");
}

#[test]
fn generated_program_constants_embed_the_runtime_hash() {
    // The generator's literals are repeated, nested, carry metadata and are
    // partially constant; each seed's entry namespace also keeps `hash`.
    let mut checked = 0;
    for seed in seeds() {
        let program = namespace_program(seed);
        let entry = format!("{}.cljs", program.entry.replace('.', "/"));
        let mut sources = program.sources(&|_| false, false);
        let (_, text) = sources.iter_mut().find(|(path, _)| *path == entry).unwrap();
        text.push_str("\n(set! (.-lightHash js/globalThis) cljs.core/hash)\n");
        let analyzed = analyze(&program, &sources);
        let js = emit_readable(&analyzed, OptimizationLevel::Advanced);
        checked += agrees(&js, &format!("seed {seed}"));
    }
    eprintln!("{checked} embedded hashes in generated programs");
    assert!(checked > 20, "{checked}");
}

#[test]
fn equal_values_hash_alike_whether_pooled_or_built() {
    // `=` implies equal hashes across a pooled constant and a value built at
    // run time, whatever the level; metadata does not take part.
    every_level(
        r#"
      (defn pairs []
        [[[1 2] (vector 1 2)] ['(1 2) (list 1 2)] [[1 2] (list 1 2)] [{:a 1} (hash-map :a 1)]
         [#{1 2} (hash-set 1 2)] [[-0.0] (vector 0)] [[1.0] (vector 1)] [{[1] :a} {(list 1) :a}]
         [^{:m 1} [1] (vector 1)] [{:a [1 #{2}]} (assoc {} :a (conj [1] (hash-set 2)))]
         [["😀" "\uD800"] (vector (str "😀") (str "\uD800"))] [[:a/b 'c/d] (vector (keyword "a" "b") (symbol "c" "d"))]
         [#uuid "00000000-0000-0000-0000-00000000000A" (uuid "00000000-0000-0000-0000-00000000000a")]
         [{:a 1 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h 8 :i 9} (zipmap [:a :b :c :d :e :f :g :h :i] (range 1 10))]
         [[[] {} #{} ()] (vector (vector) (hash-map) (hash-set) (list))]])
      (pr-str (map (fn [[a b]] [(= a b) (= (hash a) (hash b))]) (pairs)))"#,
        "([true true] [true true] [true true] [true true] [true true] [true true] [true true] \
         [true true] [true true] [true true] [true true] [true true] [true true] [true true] \
         [true true])",
    );
}

#[test]
fn cached_hashes_never_go_stale() {
    // A cached hash belongs to an immutable value: updates, transients and
    // `with-meta` make new values, which hash their own contents; the old
    // value keeps its hash. Positional constructors keep a supplied hash.
    every_level(
        r#"
      (defn v [] [1 2]) (defn m [] {:a 1}) (defn s [] #{:x}) (defn q [] (conj cljs.core/PersistentQueue.EMPTY 1))
      (let [v (v) m (m) s (s) q (q) hv (hash v) hm (hash m) hs (hash s) hq (hash q)
            t (transient v) _ (conj! t 3) pv (persistent! t)
            tm (transient m) _ (assoc! tm :b 2) pm (persistent! tm)
            ts (transient s) _ (conj! ts :y) ps (persistent! ts)]
        (pr-str [(= hv (hash v) (hash [1 2])) (= hm (hash m) (hash {:a 1})) (= hs (hash s) (hash #{:x}))
                 (= hq (hash q) (hash (conj cljs.core/PersistentQueue.EMPTY 1)))
                 (= (hash pv) (hash [1 2 3])) (= (hash pm) (hash {:a 1 :b 2})) (= (hash ps) (hash #{:x :y}))
                 (= (hash (conj v 3)) (hash [1 2 3])) (= (hash (assoc m :a 2)) (hash {:a 2}))
                 (= (hash (disj s :x)) (hash #{})) (= (hash (pop q)) (hash cljs.core/PersistentQueue.EMPTY))
                 (= hv (hash (with-meta v {:m 1})) (hash (vary-meta v assoc :n 2)))
                 (= (hash (assoc v 0 5)) (hash [5 2])) (= (hash (subvec v 1)) (hash [2]))
                 (= (hash (seq v)) hv) (= (hash (rseq v)) (hash [2 1]))
                 (= 7 (hash (PersistentVector. nil 1 5 nil #js [1] 7)))
                 v m s (vec q) pv pm (count ps)]))"#,
        "[true true true true true true true true true true true true true true true true true [1 2] {:a 1} #{:x} [1] [1 2 3] {:a 1, :b 2} 2]",
    );
    // A JavaScript array's sequence reads the array, so its hash follows it.
    every_level(
        "(let [a #js [1 2] xs (seq a) before (hash xs)] (aset a 0 5) \
         (pr-str [(= before (hash [1 2])) (= (hash xs) (hash [5 2]))]))",
        "[true true]",
    );
}

#[test]
fn hashes_are_embedded_only_at_advanced_with_intact_core_protocols() {
    let source = "(defn f [] [1 {:a #{2}} '(3) ^:m [4]]) (pr-str [(f) (hash (f))])";
    let program = program(source);
    for level in [OptimizationLevel::None, OptimizationLevel::Prepare] {
        assert!(!emit(&program, level).contains("rt.hashed("), "{level}");
    }
    let advanced = emit_readable(&program, OptimizationLevel::Advanced);
    let declarations: Vec<_> = advanced
        .lines()
        .filter(|line| {
            line.strip_prefix("  const k")
                .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
        })
        .collect();
    assert_eq!(
        declarations,
        [
            "  const k0 = rt$keyword(null, \"a\");",
            "  const k1 = rt$hashed(rt$arraySetFromArray([2]), -2073014043);",
            "  const k2 = rt$hashed(rt$arrayMapFromArray([k0, k1]), -689938007);",
            "  const k3 = rt$hashed(rt$listFromArray([3]), -1025653387);",
            "  const k4 = rt$hashed(rt$vectorFromArray([4]), 385458271);",
            "  const k5 = rt$keyword(null, \"m\");",
            "  const k6 = rt$hashed(rt$arrayMapFromArray([k5, true]), 1882657429);",
            "  const k7 = rt$withMeta(k4, k6);",
            "  const k8 = rt$hashed(rt$vectorFromArray([1, k2, k3, k7]), 318764438);",
        ]
    );
    // The hash of the pooled vector is the one every level computes.
    let expected = execute(&format!(
        "console.log({});",
        emit(&program, OptimizationLevel::None)
    ));
    assert!(expected.ends_with(" 318764438]\n"), "{expected}");
    assert_eq!(execute(&format!("console.log({advanced});")), expected);
    assert_eq!(
        execute(&format!(
            "console.log({});",
            emit(&program, OptimizationLevel::Advanced)
        )),
        expected
    );
    // Extending a core protocol to a host type may change the hash of any
    // value, so such a program embeds none.
    let extended = emit_readable(
        &self::program(&format!(
            "(extend-type number IHash (-hash [n] 42)) {source}"
        )),
        OptimizationLevel::Advanced,
    );
    assert!(extended.contains("rt$vectorFromArray([1, k"));
    assert!(!extended.contains("rt$hashed("));
}

#[test]
fn a_pooled_constant_shows_its_hash_field_before_its_first_hash() {
    // The advanced-mode contract: the `__hash` field of a collection holds its
    // hash once computed, as upstream's does; a pooled constant's is set
    // before its first hash.
    let source = "(defn v [] [1 2]) (let [x (v) before (.-__hash x)] (hash x) (pr-str [before (.-__hash x)]))";
    let program = program(source);
    let printed = |level| execute(&format!("console.log({});", emit(&program, level)));
    assert_eq!(printed(OptimizationLevel::None), "[nil 1185709346]\n");
    assert_eq!(printed(OptimizationLevel::Prepare), "[nil 1185709346]\n");
    assert_eq!(
        printed(OptimizationLevel::Advanced),
        "[1185709346 1185709346]\n"
    );
}
