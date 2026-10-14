//! The core operation table (src/core_operations.tsv, read by
//! src/core_operations.rs) against the analyzer and the runtime: every
//! `CoreFnId` has a row, every runtime unit it names exists and is the unit
//! that defines the operation's value in the runtime `core` object, and the
//! annotated types and purity hold on sample arguments.
//!
//! Moving runtime code between units (pointing a `core` hub entry at another
//! binding, turning effects into install functions, separating compile-time
//! machinery) changes which unit defines an operation, and the units check
//! fails. Rewrite the
//! `units` column with
//! `LIGHT_CLJS_UPDATE_CORE_OPERATIONS=1 cargo test --test compiler core_operations::`
//! and review the diff.

use std::collections::{BTreeMap, HashSet};

use light_cljs::CoreFnId;
use light_cljs::core_operations::{COLUMNS, Calls, Purity, TABLE, Type, operations};
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use light_cljs::runtime_manifest::{ANNOTATED_RUNTIME, CORE_CELLS, CoreReference, RuntimeManifest};

#[test]
fn every_core_operation_has_exactly_one_row() {
    let operations = operations();
    assert_eq!(operations.len(), CoreFnId::ALL.len());
    let mut seen = HashSet::new();
    for (id, operation) in CoreFnId::ALL.into_iter().zip(operations) {
        assert!(seen.insert(id), "{id:?} has two rows");
        assert_eq!(format!("{id:?}"), operation.variant);
        assert!(std::ptr::eq(id.operation(), operation));
    }
    let header = format!(";; {}", COLUMNS.join("\t"));
    assert!(
        TABLE.lines().any(|line| line == header),
        "the table documents its columns"
    );
}

#[test]
fn every_runtime_unit_in_the_table_exists_in_the_manifest() {
    let manifest = RuntimeManifest::embedded();
    let mut missing = Vec::new();
    for operation in operations() {
        let units = operation.runtime_units(manifest);
        for unit in &operation.units {
            if !units.iter().any(|&id| manifest.unit(id).name == *unit) {
                missing.push(format!(
                    "src/core_operations.tsv:{}: {} names unknown runtime unit `{unit}`",
                    operation.line, operation.variant
                ));
            }
        }
    }
    assert!(missing.is_empty(), "{}", missing.join("\n"));
}

/// For each key of the runtime's core table (`RuntimeManifest::core_reference`),
/// the unit that Advanced output keeps to read it directly: the unit declaring
/// the binding that holds the value (`add`, `keywordCore` for `keyword`, the
/// constructor `PersistentVectorValue` for the type `PersistentVector`), or
/// the binding it is a member of (`reader` for the reader), or `coreCells` for
/// a cell (dynamic Vars, `rand`).
fn core_value_units(manifest: &RuntimeManifest<'_>) -> BTreeMap<String, Vec<String>> {
    manifest
        .core_references()
        .map(|(key, reference)| {
            let binding = match reference {
                CoreReference::Binding(binding) | CoreReference::Member(binding, _) => binding,
                CoreReference::Cell(_) => CORE_CELLS,
            };
            let unit = manifest.unit_of(binding).unwrap();
            (key.to_owned(), vec![manifest.unit(unit).name.to_owned()])
        })
        .collect()
}

#[test]
fn runtime_units_are_the_units_that_define_each_core_value() {
    let owners = core_value_units(RuntimeManifest::embedded());
    let mut expected = BTreeMap::new();
    let mut errors = Vec::new();
    for (id, operation) in CoreFnId::ALL.into_iter().zip(operations()) {
        let key = id.runtime_key();
        let Some(units) = owners.get(key) else {
            errors.push(format!(
                "src/core_operations.tsv:{}: {} reads `core.{key}`, which the runtime does not define",
                operation.line, operation.variant
            ));
            continue;
        };
        let mut units = units.clone();
        units.sort();
        expected.insert(operation.variant, units.join(","));
        if operation.units != units {
            errors.push(format!(
                "src/core_operations.tsv:{}: {} (`core.{key}`) lists units {}, but the runtime defines it in {}",
                operation.line,
                operation.variant,
                if operation.units.is_empty() { "-".into() } else { operation.units.join(",") },
                units.join(",")
            ));
        }
    }
    if std::env::var_os("LIGHT_CLJS_UPDATE_CORE_OPERATIONS").is_some() {
        let mut updated = String::with_capacity(TABLE.len());
        for line in TABLE.split_inclusive('\n') {
            let row = line.trim_end_matches('\n');
            let mut fields = row.split('\t').collect::<Vec<_>>();
            if !row.starts_with(';')
                && fields.len() == COLUMNS.len()
                && let Some(units) = expected.get(fields[0])
            {
                let units = if units.is_empty() {
                    "-"
                } else {
                    units.as_str()
                };
                *fields.last_mut().unwrap() = units;
                updated.push_str(&fields.join("\t"));
                updated.push_str(&line[row.len()..]);
            } else {
                updated.push_str(line);
            }
        }
        if updated != TABLE {
            let path =
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/core_operations.tsv");
            std::fs::write(path, updated).unwrap();
        }
        return;
    }
    assert!(
        errors.is_empty(),
        "the core operation table is out of date with src/js_runtime.js:\n{}\n\nFix the units \
         column or run LIGHT_CLJS_UPDATE_CORE_OPERATIONS=1 cargo test --test compiler \
         core_operations:: and review the diff.",
        errors.join("\n")
    );
}

#[test]
fn moving_an_operation_into_its_own_unit_moves_its_units() {
    let embedded = core_value_units(RuntimeManifest::embedded());
    assert_eq!(embedded["second"], ["second"]);
    assert_eq!(embedded["inc"], ["inc"]);
    assert_eq!(embedded["add"], ["add"]);
    assert_eq!(embedded["keyword"], ["keywordCore"]);
    assert_eq!(embedded["readString"], ["reader"]);
    assert_eq!(embedded["PersistentVector"], ["PersistentVectorValue"]);
    assert_eq!(embedded["printLength"], ["coreCells"]);
    assert_eq!(CoreFnId::Second.operation().units, ["second"]);
    // Pointing the hub's `second` at another unit's binding makes that unit
    // define it, so the table row above goes stale.
    let moved = ANNOTATED_RUNTIME.replacen(" chunk, second,\n", " chunk, second: fnext,\n", 1);
    assert_ne!(moved, ANNOTATED_RUNTIME);
    let moved = RuntimeManifest::parse(&moved)
        .unwrap()
        .with_computed_requires();
    let manifest = RuntimeManifest::parse(&moved).unwrap();
    manifest.validate().unwrap();
    let units = core_value_units(&manifest);
    let changed = units
        .iter()
        .filter(|(key, owners)| embedded.get(*key) != Some(owners))
        .map(|(key, owners)| format!("{key}: {}", owners.join(",")))
        .collect::<Vec<_>>();
    assert_eq!(changed, ["second: fnext"]);
}

/// Prints, for every runtime unit the table names, how many operations it
/// defines and what keeping it alone keeps, then the purity and calls counts.
/// Run with
/// `cargo test --test compiler core_operations::print_core_operation_units -- --ignored --nocapture`.
#[test]
#[ignore = "measurement; prints the operation units"]
fn print_core_operation_units() {
    let manifest = RuntimeManifest::embedded();
    let mut operations_per_unit = BTreeMap::<&str, usize>::new();
    let mut purity = BTreeMap::<String, usize>::new();
    let mut calls = BTreeMap::<String, usize>::new();
    for operation in operations() {
        for unit in &operation.units {
            *operations_per_unit.entry(unit).or_default() += 1;
        }
        *purity.entry(format!("{:?}", operation.purity)).or_default() += 1;
        *calls.entry(format!("{:?}", operation.calls)).or_default() += 1;
    }
    println!("unit\toperations\tbytes\treach_units\treach_bytes");
    for (name, count) in operations_per_unit {
        let unit = manifest.unit_named(name).unwrap();
        let reach = manifest.reachable([unit]);
        let (units, bytes) = manifest
            .units
            .iter()
            .zip(&reach)
            .filter(|(_, kept)| **kept)
            .fold((0, 0), |(units, bytes), (unit, _)| {
                (units + 1, bytes + unit.body_range.len())
            });
        let own = manifest.unit(unit).body_range.len();
        println!("{name}\t{count}\t{own}\t{units}\t{bytes}");
    }
    println!("\npurity: {purity:?}\ncalls: {calls:?}");
}

/// A JavaScript array of the class names of `ty`.
fn classes(ty: Type) -> String {
    let names = ty
        .classes()
        .map(|class| format!("{:?}", class.to_string()))
        .collect::<Vec<_>>();
    format!("[{}]", names.join(","))
}

/// Calls every operation that is not `effect` on sample arguments of every
/// class its parameters admit, in the canonical runtime, and reports each
/// call that contradicts its row: a result outside its arity's `returns` (or
/// `otherwise` for calls outside the domain), a `pure` or `allocates` call that throws,
/// a call that invokes a function argument without `invoke`, or one that
/// realizes a lazy sequence argument with `-` calls. Function arguments are
/// traps that throw when called, except for `invoke` rows; lazy sequence
/// arguments of `-` rows throw when realized. Records have no samples.
#[test]
fn signatures_hold_on_sample_arguments() {
    let mut rows = Vec::new();
    for (id, operation) in CoreFnId::ALL.into_iter().zip(operations()) {
        let arities = match &operation.arities {
            Some(arities) => arities
                .iter()
                .map(|arity| {
                    let params = arity.params.iter().map(|p| classes(*p)).collect::<Vec<_>>();
                    let rest = arity.rest.map_or("null".into(), classes);
                    let returns = classes(arity.returns.unwrap_or(operation.returns));
                    format!(
                        "{{params: [{}], rest: {rest}, returns: {returns}}}",
                        params.join(",")
                    )
                })
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        };
        rows.push(format!(
            "{{variant: {:?}, key: {:?}, value: {}, arities: [{}], returns: {}, otherwise: {}, \
             pure: {}, throws: {}, effect: {}, calls: {:?}}}",
            operation.variant,
            id.runtime_key(),
            classes(operation.value),
            arities.join(","),
            classes(operation.returns),
            classes(operation.otherwise),
            operation.purity == Purity::Pure,
            operation.purity >= Purity::Throws,
            operation.purity == Purity::Effect,
            match operation.calls {
                Calls::None => "-",
                Calls::Dispatch => "dispatch",
                Calls::Invoke => "invoke",
            },
        ));
    }
    let script = format!(
        r#"(() => {{
  const c = rt.core;
  // The samples contain no user types, so the collection predicates identify
  // runtime collections; other runtime values are instances of a core type or
  // implement a core protocol directly, which host arrays and objects do not.
  const constructors = Object.values(c).filter(f => typeof f === "function" && f.prototype !== null && typeof f.prototype === "object");
  const protocols = [c.IFn, c.IMeta, c.IDeref, c.ICounted, c.ISeqable, c.IHash, c.IEquiv, c.IPrintWithWriter];
  const owned = v => constructors.some(f => v instanceof f) || protocols.some(p => c.implements(p, v));
  const classify = v => {{
    if (v == null) return "nil";
    const t = typeof v;
    if (t === "boolean" || t === "number" || t === "string") return t;
    if (t !== "object" && t !== "function") return "host";
    if (c.isKeyword(v)) return "keyword";
    if (c.isSymbol(v)) return "symbol";
    if (c.isRecord(v)) return "record";
    if (c.isList(v)) return "list";
    if (c.isSeq(v)) return "seq";
    if (c.isVector(v)) return "vector";
    if (c.isMap(v)) return "map";
    if (c.isSet(v)) return "set";
    if (t === "function") return "fn";
    return owned(v) ? "object" : "host";
  }};
  // Function arguments of rows without `invoke` throw INVOKED when called.
  // Code that arguments carry (lazy sequence bodies and the comparators of
  // sorted collections) throws RAN in rows with `-` calls; in other rows it
  // runs, and an exception it raises becomes ARGUMENT_THREW, which the row's
  // purity does not have to admit.
  const INVOKED = "invoked a function argument", RAN = "ran argument code", ARGUMENT_THREW = "argument code threw";
  let calls = "invoke";
  const argumentCode = () => {{ if (calls === "-") throw new Error(RAN); }};
  const comparator = (a, b) => {{
    argumentCode();
    try {{ return c.compare(a, b); }} catch {{ throw new Error(ARGUMENT_THREW); }}
  }};
  const kw = name => rt.keyword(null, name), vec = (...xs) => rt.vectorFromArray(xs);
  const lastArgument = (...xs) => xs.length ? xs[xs.length - 1] : null;
  const sorted = () => {{
    const saved = calls;
    calls = "invoke";
    try {{ return [c.sortedMapBy(comparator, 1, 2, 3, 4), c.sortedSetBy(comparator, 1, 2)]; }}
    finally {{ calls = saved; }}
  }};
  // Fresh samples, since realizing a lazy sequence caches its elements.
  const samples = ([sortedMap, sortedSet] = sorted()) => ({{
    nil: [null],
    boolean: [true, false],
    number: [0, 1, 2, -1, 1.5, NaN, Infinity],
    string: ["", "ab", "a/b"],
    keyword: [kw("a"), rt.keyword("x", "b")],
    symbol: [rt.symbol(null, "s"), rt.symbol("x", "t")],
    fn: calls === "invoke" ? [lastArgument, () => true] : [() => {{ throw new Error(INVOKED); }}],
    seq: [c.seq(vec(1, 2)), c.cons(0, c.seq(vec(1))), c.range(3),
      c.mapSeq(x => {{ argumentCode(); return x; }}, vec(1, 2))],
    vector: [vec(), vec(1, 2), vec(vec(kw("k"), 1)), vec(rt.mapFromArray([kw("a"), vec()]))],
    map: [rt.mapFromArray([]), rt.mapFromArray([kw("a"), 1, kw("b"), vec(2)]), sortedMap],
    set: [rt.setFromArray([]), rt.setFromArray([1, kw("a")]), sortedSet],
    list: [rt.listFromArray([]), rt.listFromArray([1, 2])],
    record: [],
    object: [c.atom(1), c.reduced(2), c.uuid("00000000-0000-4000-8000-000000000000")],
    host: [[], [1, 2], {{}}],
  }});
  const LIMIT = 400;
  // Every argument list from the product of the choices, or LIMIT of them
  // picked by a fixed pseudo-random sequence when there are more.
  const tuples = choices => {{
    const total = choices.reduce((n, xs) => n * xs.length, 1);
    const pick = index => {{
      let rest = index;
      return choices.map(xs => {{ const x = xs[rest % xs.length]; rest = Math.floor(rest / xs.length); return x; }});
    }};
    if (total <= LIMIT) return Array.from({{length: total}}, (_, index) => pick(index));
    let seed = 2463534242;
    return Array.from({{length: LIMIT}}, () => {{
      seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5; seed >>>= 0;
      return pick(seed % total);
    }});
  }};
  const shapes = arity => arity.rest ? [0, 1, 2].map(n => [...arity.params, ...Array(n).fill(arity.rest)]) : [arity.params];
  const show = v => {{ try {{ return c.prStr(v); }} catch {{ return "<" + classify(v) + ">"; }} }};
  const violations = [];
  for (const row of [{rows}]) {{
    const report = (args, message) => violations.push(row.variant + "(" + args.map(show).join(" ") + "): " + message);
    const f = c[row.key];
    if (!row.value.includes(classify(f))) report([], "value is " + classify(f));
    let reported = 0;
    const check = (args, returns, inDomain) => {{
      if (reported > 3) return;
      try {{
        const result = f(...args);
        const k = classify(result);
        if (!returns.includes(k)) {{ reported++; report(args, "returned " + k + " " + show(result)); }}
        else if (inDomain && row.pure && !Object.is(result, f(...args))) {{ reported++; report(args, "returned a fresh " + k); }}
      }} catch (error) {{
        const message = error?.message ?? String(error);
        if (!inDomain || row.effect || message === ARGUMENT_THREW || (row.throws && message !== INVOKED && message !== RAN)) return;
        reported++;
        report(args, message === INVOKED || message === RAN ? message : "threw " + message);
      }}
    }};
    // Effects may do anything, so only their result types are checked.
    calls = row.effect ? "invoke" : row.calls;
    for (const arity of row.arities)
      for (const shape of shapes(arity))
        for (const args of tuples(shape.map(classes => {{ const pool = samples(); return classes.flatMap(k => pool[k]); }})))
          check(args, arity.returns, true);
    calls = "invoke";
    if (row.arities.length && row.otherwise.length < 15) {{
      const everything = Object.values(samples()).flat();
      for (let n = 0; n <= 3; n++)
        for (const args of tuples(Array(n).fill(everything)))
          if (!row.arities.some(a => (a.rest ? n >= a.params.length : n === a.params.length) &&
              args.every((x, i) => (a.params[i] ?? a.rest).includes(classify(x)))))
            check(args, row.otherwise, false);
    }}
  }}
  return violations.join("\n");
}})()"#,
        rows = rows.join(",\n  ")
    );
    let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
    let report = evaluator.evaluate_js(&script).unwrap();
    let violations = evaluator.string(report).unwrap();
    assert!(
        violations.is_empty(),
        "src/core_operations.tsv disagrees with the runtime:\n{violations}"
    );
}
