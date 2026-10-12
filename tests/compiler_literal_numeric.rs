//! Canonical collection numeric literals and exact source phase evidence.
use light_cljs::MacroHost;
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};
fn hex(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02x}")).collect()
}
fn unhex(s: &str) -> String {
    String::from_utf8(
        s.as_bytes()
            .chunks_exact(2)
            .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
            .collect(),
    )
    .unwrap()
}
fn observe(script: &str, artifact: &std::path::Path) -> [(String, String); 2] {
    let runtime = rquickjs::Runtime::new().unwrap();
    let context = rquickjs::Context::full(&runtime).unwrap();
    let quickjs = context.with(|ctx| match ctx.eval::<String, _>(script.as_bytes()) {
        Ok(value) => value,
        Err(error) => format!("HARNESS_ERROR:{error:?}: {:?}", ctx.catch()),
    });
    let mut child = Command::new(std::env::var("NODE").unwrap())
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(format!("console.log({script});").as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    fs::write(artifact.join("script.js"), script).unwrap();
    fs::write(artifact.join("quickjs.raw"), &quickjs).unwrap();
    fs::write(
        artifact.join("node.stdin"),
        format!("console.log({script});"),
    )
    .unwrap();
    fs::write(artifact.join("node.stdout"), &out.stdout).unwrap();
    fs::write(artifact.join("node.stderr"), &out.stderr).unwrap();
    fs::write(
        artifact.join("node.exit"),
        out.status.code().unwrap_or(-1).to_string(),
    )
    .unwrap();
    [
        ("QuickJS".into(), quickjs),
        (
            "Node".into(),
            if out.status.success() && out.stderr.is_empty() {
                String::from_utf8(out.stdout).unwrap().trim().into()
            } else {
                format!("HARNESS_ERROR:{}", String::from_utf8_lossy(&out.stderr))
            },
        ),
    ]
}
#[test]
#[ignore = "explicit original .cljc filename and actual phase audit"]
fn literal_numeric_sources_execute_in_fresh_and_reused_macro_hosts() {
    let root = PathBuf::from(std::env::var("LIGHT_SCI_CLJC_ACCEPTED_ROOT").unwrap());
    let input = fs::read_to_string(root.join("input.tsv")).unwrap();
    let mut out = fs::File::create(root.join("observations.tsv")).unwrap();
    let mut reused = MacroHost::new();
    let warmup = light_cljs::compile_with_macros(
        "(defmacro cljc-ready [] (+ 20 22)) (cljc-ready)",
        &mut reused,
    )
    .unwrap();
    fs::write(root.join("warmup.js"), &warmup).unwrap();
    for row in input.lines() {
        let fields: Vec<_> = row.split('\t').collect();
        assert_eq!(fields.len(), 2);
        let (id, original) = (fields[0], unhex(fields[1]));
        for reuse in [false, true] {
            for phase in [false, true] {
                let variant = format!(
                    "{}-{}",
                    if phase { "macro" } else { "application" },
                    if reuse { "reused" } else { "fresh" }
                );
                let source = if phase {
                    format!(
                        "(ns user)\n(defmacro upstream [] (list 'quote (do {original})))\n(defn run [observe] (upstream))\nrun\n"
                    )
                } else {
                    format!("(ns user)\n(defn run [observe] (do {original}))\nrun\n")
                };
                let sources = root.join(id).join(&variant);
                fs::create_dir_all(&sources).unwrap();
                fs::write(sources.join("user.cljc"), &source).unwrap();
                let mut fresh = MacroHost::new();
                match light_cljs::compile_namespace_with_macros(
                    "user",
                    &[sources.clone()],
                    if reuse { &mut reused } else { &mut fresh },
                ) {
                    Err(e) => {
                        fs::write(sources.join("rejection.txt"), e.to_string()).unwrap();
                        writeln!(
                            out,
                            "{id}\t{variant}\trejected\t{}\t{}",
                            hex(&source),
                            hex(&e.to_string())
                        )
                        .unwrap();
                    }
                    Ok(js) => {
                        let anchor = " = // Original bounded runtime";
                        assert_eq!(js.matches(anchor).count(), 1);
                        let captured = js.replacen(
                            anchor,
                            " = globalThis.lightRuntime = // Original bounded runtime",
                            1,
                        );
                        let script = format!(
                            "(() => {{const run={captured};return ({})(globalThis.lightRuntime)('{id}',observe=>run(observe));}})()",
                            include_str!("support/sci_execution_observation.js")
                        );
                        fs::write(sources.join("compiled.js"), &js).unwrap();
                        for (engine, raw) in observe(&script, &sources) {
                            writeln!(
                                out,
                                "{id}\t{variant}\t{engine}\t{}\t{}\t{}\t{}",
                                hex(&source),
                                hex(&js),
                                hex(&raw),
                                if reuse { hex(&warmup) } else { "-".into() }
                            )
                            .unwrap();
                        }
                    }
                }
            }
        }
    }
    assert_eq!(input.lines().count(), 42);
    out.flush().unwrap();
    let negatives = root.parent().unwrap().join("diagnostics");
    let input = fs::read_to_string(negatives.join("input.tsv")).unwrap();
    let mut output = fs::File::create(negatives.join("observations.tsv")).unwrap();
    for row in input.lines() {
        let fields: Vec<_> = row.split('\t').collect();
        let (id, original) = (fields[0], unhex(fields[1]));
        for reuse in [false, true] {
            for phase in [false, true] {
                let variant = format!(
                    "{}-{}",
                    if phase { "macro" } else { "application" },
                    if reuse { "reused" } else { "fresh" }
                );
                let source = if phase {
                    format!(
                        "(ns user)\n(defmacro upstream [] (list 'quote (do {original})))\n(defn run [observe] (upstream))\nrun\n"
                    )
                } else {
                    format!("(ns user)\n(defn run [observe] (do {original}))\nrun\n")
                };
                let folder = negatives.join(id).join(&variant);
                fs::create_dir_all(&folder).unwrap();
                fs::write(folder.join("user.cljc"), &source).unwrap();
                let mut fresh = MacroHost::new();
                let host = if reuse { &mut reused } else { &mut fresh };
                let error =
                    light_cljs::compile_namespace_with_macros("user", &[folder.clone()], host)
                        .unwrap_err()
                        .to_string();
                fs::write(folder.join("rejection.txt"), &error).unwrap();
                let recovery = "(defmacro literal-recovery [] (+ 20 22)) (literal-recovery)";
                fs::write(folder.join("recovery.cljs"), recovery).unwrap();
                let js = light_cljs::compile_with_macros(recovery, host).unwrap();
                fs::write(folder.join("recovery.js"), &js).unwrap();
                let script = format!("JSON.stringify({js})");
                let observed = observe(&script, &folder);
                for (engine, raw) in observed {
                    assert_eq!(raw, "42", "{id} {variant} {engine}: recovery");
                    writeln!(
                        output,
                        "{id}\t{variant}\t{engine}\t{}\t{}\t{}\t{}",
                        hex(&source),
                        hex(&error),
                        hex(&js),
                        hex(&raw)
                    )
                    .unwrap();
                }
            }
        }
    }
    output.flush().unwrap();
}

fn canonical(context: &rquickjs::Context, value: &str) -> String {
    let script = format!(
        "JSON.stringify((function sort(v) {{ if(Array.isArray(v)) return v.map(sort); if(v && typeof v==='object') {{ const out={{}}; Object.keys(v).sort().forEach(k=>out[k]=sort(v[k])); return out; }} return v; }})(JSON.parse({value:?})))"
    );
    context.with(|ctx| ctx.eval::<String, _>(script.as_bytes()).unwrap())
}

#[test]
fn canonical_numeric_literal_constants_and_same_host_diagnostic_recovery() {
    let mut host = MacroHost::new();
    // A demanded scalar macro result can omit the runtime entirely. The typed
    // observer still needs a real runtime to capture stdout and inspect values.
    let observer_runtime = light_cljs::compile("[]").unwrap().replacen(
        " = // Original bounded runtime",
        " = globalThis.lightRuntime = // Original bounded runtime",
        1,
    );
    let controls: &[(&str, &str, &str)] = &[
        (
            r###"literal-numeric-original"###,
            r###"[(count {1 :int 1.0 :float}) (get {1 :int 1.0 :float} 1) (count #{1 1.0}) (vec (seq #{1 1.0}))]"###,
            r###"{"id":"literal-numeric-original","outcome":{"status":"returned","value":{"$vector":[2,{"$keyword":{"namespace":null,"name":"int","spelling":"int"}},2,{"$vector":[1,1]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-map-order-lookup"###,
            r###"(let [m {1 :int 1.0 :float}] [(count m) (get m 1) (get m 1.0) (contains? m 1) (contains? m 1.0) (vec (seq m)) (vec (keys m)) (vec (vals m))])"###,
            r###"{"id":"literal-numeric-map-order-lookup","outcome":{"status":"returned","value":{"$vector":[2,{"$keyword":{"namespace":null,"name":"int","spelling":"int"}},{"$keyword":{"namespace":null,"name":"int","spelling":"int"}},true,true,{"$vector":[{"$vector":[1,{"$keyword":{"namespace":null,"name":"int","spelling":"int"}}]},{"$vector":[1,{"$keyword":{"namespace":null,"name":"float","spelling":"float"}}]}]},{"$vector":[1,1]},{"$vector":[{"$keyword":{"namespace":null,"name":"int","spelling":"int"}},{"$keyword":{"namespace":null,"name":"float","spelling":"float"}}]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-map-reverse-order"###,
            r###"(let [m {1.0 :float 1 :int}] [(count m) (get m 1) (get m 1.0) (vec (seq m))])"###,
            r###"{"id":"literal-numeric-map-reverse-order","outcome":{"status":"returned","value":{"$vector":[2,{"$keyword":{"namespace":null,"name":"float","spelling":"float"}},{"$keyword":{"namespace":null,"name":"float","spelling":"float"}},{"$vector":[{"$vector":[1,{"$keyword":{"namespace":null,"name":"float","spelling":"float"}}]},{"$vector":[1,{"$keyword":{"namespace":null,"name":"int","spelling":"int"}}]}]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-set-reverse-order"###,
            r###"[(count #{1.0 1}) (vec (seq #{1.0 1})) (get #{1.0 1} 1) (contains? #{1.0 1} 1)]"###,
            r###"{"id":"literal-numeric-set-reverse-order","outcome":{"status":"returned","value":{"$vector":[2,{"$vector":[1,1]},1,true]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-quoted"###,
            r###"(let [m '{1 :int 1.0 :float} s '#{1 1.0}] [(count m) (get m 1) (vec (seq m)) (count s) (vec (seq s))])"###,
            r###"{"id":"literal-numeric-quoted","outcome":{"status":"returned","value":{"$vector":[2,{"$keyword":{"namespace":null,"name":"int","spelling":"int"}},{"$vector":[{"$vector":[1,{"$keyword":{"namespace":null,"name":"int","spelling":"int"}}]},{"$vector":[1,{"$keyword":{"namespace":null,"name":"float","spelling":"float"}}]}]},2,{"$vector":[1,1]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-nested-literals"###,
            r###"(let [x [{:m {1 :int 1.0 :float} :s #{1 1.0}}] m (get (first x) :m) s (get (first x) :s)] [(count m) (get m 1) (count s) (vec (seq s))])"###,
            r###"{"id":"literal-numeric-nested-literals","outcome":{"status":"returned","value":{"$vector":[2,{"$keyword":{"namespace":null,"name":"int","spelling":"int"}},2,{"$vector":[1,1]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-quoted-nested"###,
            r###"(let [x '({:m {1 :int 1.0 :float} :s #{1 1.0}}) m (get (first x) :m) s (get (first x) :s)] [(count m) (get m 1) (count s) (vec (seq s))])"###,
            r###"{"id":"literal-numeric-quoted-nested","outcome":{"status":"returned","value":{"$vector":[2,{"$keyword":{"namespace":null,"name":"int","spelling":"int"}},2,{"$vector":[1,1]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-hex-octal"###,
            r###"[(count {0x1 :hex 1.0 :float}) (get {01 :octal 1e0 :float} 1) (count #{0x1 1.0}) (vec (seq #{01 1e0}))]"###,
            r###"{"id":"literal-numeric-hex-octal","outcome":{"status":"returned","value":{"$vector":[2,{"$keyword":{"namespace":null,"name":"octal","spelling":"octal"}},2,{"$vector":[1,1]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-bigint-ratio"###,
            r###"[(count {1N :big 1.0 :float}) (get {2/2 :ratio 1.0 :float} 1) (count #{1N 1.0}) (vec (seq #{2/2 1.0}))]"###,
            r###"{"id":"literal-numeric-bigint-ratio","outcome":{"status":"returned","value":{"$vector":[2,{"$keyword":{"namespace":null,"name":"ratio","spelling":"ratio"}},2,{"$vector":[1,1]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-decimal-categories"###,
            r###"(let [m {1 :int 1M :decimal 1.0 :float} s #{1 1M 1.0}] [(count m) (get m 1) (vec (seq m)) (count s) (vec (seq s))])"###,
            r###"{"id":"literal-numeric-decimal-categories","outcome":{"status":"returned","value":{"$vector":[3,{"$keyword":{"namespace":null,"name":"int","spelling":"int"}},{"$vector":[{"$vector":[1,{"$keyword":{"namespace":null,"name":"int","spelling":"int"}}]},{"$vector":[1,{"$keyword":{"namespace":null,"name":"decimal","spelling":"decimal"}}]},{"$vector":[1,{"$keyword":{"namespace":null,"name":"float","spelling":"float"}}]}]},3,{"$vector":[1,1,1]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-decimal-precision"###,
            r###"[(count {1.0000000000000001M :precise 1M :rounded}) (get {1.0000000000000001M :precise 1M :rounded} 1) (count #{1.0000000000000001M 1M}) (vec (seq #{1.0000000000000001M 1M}))]"###,
            r###"{"id":"literal-numeric-decimal-precision","outcome":{"status":"returned","value":{"$vector":[2,{"$keyword":{"namespace":null,"name":"precise","spelling":"precise"}},2,{"$vector":[1,1]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-rounding-integers"###,
            r###"[(count {9007199254740992 :first 9007199254740993 :second}) (get {9007199254740992 :first 9007199254740993 :second} 9007199254740992) (count #{9007199254740992 9007199254740993}) (vec (seq #{9007199254740992 9007199254740993}))]"###,
            r###"{"id":"literal-numeric-rounding-integers","outcome":{"status":"returned","value":{"$vector":[2,{"$keyword":{"namespace":null,"name":"first","spelling":"first"}},2,{"$vector":[9007199254740992,9007199254740992]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-wide-integers"###,
            r###"[(count {9223372036854775808N :first 9223372036854775809N :second}) (get {9223372036854775808N :first 9223372036854775809N :second} 9223372036854775808N) (count #{9223372036854775808N 9223372036854775809N})]"###,
            r###"{"id":"literal-numeric-wide-integers","outcome":{"status":"returned","value":{"$vector":[2,{"$keyword":{"namespace":null,"name":"first","spelling":"first"}},2]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-zero-integer-float"###,
            r###"[(count {0 :int -0.0 :float}) (get {0 :int -0.0 :float} 0) (count #{0 -0.0}) (vec (seq #{0 -0.0}))]"###,
            r###"{"id":"literal-numeric-zero-integer-float","outcome":{"status":"returned","value":{"$vector":[2,{"$keyword":{"namespace":null,"name":"int","spelling":"int"}},2,{"$vector":[0,{"$number":"-0"}]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-nonfinite"###,
            r###"(let [m {##Inf :pos ##-Inf :neg ##NaN :nan} s #{##Inf ##-Inf ##NaN}] [(count m) (get m ##Inf) (get m ##-Inf) (contains? m ##NaN) (count s) (contains? s ##Inf) (contains? s ##NaN) (vec (seq s))])"###,
            r###"{"id":"literal-numeric-nonfinite","outcome":{"status":"returned","value":{"$vector":[3,{"$keyword":{"namespace":null,"name":"pos","spelling":"pos"}},{"$keyword":{"namespace":null,"name":"neg","spelling":"neg"}},false,3,true,false,{"$vector":[{"$number":"+Infinity"},{"$number":"-Infinity"},{"$number":"NaN"}]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-repeated-nan"###,
            r###"[(count {##NaN :first ##NaN :second}) (count #{##NaN ##NaN}) (contains? #{##NaN ##NaN} ##NaN)]"###,
            r###"{"id":"literal-numeric-repeated-nan","outcome":{"status":"returned","value":{"$vector":[2,2,false]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-collision-and-nan"###,
            r###"(let [m {1 :int 1.0 :float ##NaN :nan} s #{1 1.0 ##NaN}] [(count m) (get m 1) (count s) (vec (seq s)) (contains? s ##NaN)])"###,
            r###"{"id":"literal-numeric-collision-and-nan","outcome":{"status":"returned","value":{"$vector":[3,{"$keyword":{"namespace":null,"name":"int","spelling":"int"}},3,{"$vector":[1,{"$number":"NaN"},1]},false]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-map-effects"###,
            r###"(let [events (atom []) m {1 (do (swap! events conj :first) 11) 1.0 (do (swap! events conj :second) 22)} output (with-out-str (println (count m)) (println (get m 1)))] [(count m) (get m 1) (vec (seq m)) @events output])"###,
            r###"{"id":"literal-numeric-map-effects","outcome":{"status":"returned","value":{"$vector":[2,11,{"$vector":[{"$vector":[1,11]},{"$vector":[1,22]}]},{"$vector":[{"$keyword":{"namespace":null,"name":"first","spelling":"first"}},{"$keyword":{"namespace":null,"name":"second","spelling":"second"}}]},"2\n11\n"]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-set-effects"###,
            r###"(let [events (atom []) s #{(do (swap! events conj :first) 1) (do (swap! events conj :second) 2)} output (with-out-str (println (count s)))] [(count s) @events output])"###,
            r###"{"id":"literal-numeric-set-effects","outcome":{"status":"returned","value":{"$vector":[2,{"$vector":[{"$keyword":{"namespace":null,"name":"first","spelling":"first"}},{"$keyword":{"namespace":null,"name":"second","spelling":"second"}}]},"2\n"]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-map-eight-pairs"###,
            r###"(let [m {1 :int 1.0 :float :b 3 :c 4 :d 5 :e 6 :f 7 :g 8}] [(count m) (get m 1) (get m 1.0) (vec (vals m))])"###,
            r###"{"id":"literal-numeric-map-eight-pairs","outcome":{"status":"returned","value":{"$vector":[8,{"$keyword":{"namespace":null,"name":"int","spelling":"int"}},{"$keyword":{"namespace":null,"name":"int","spelling":"int"}},{"$vector":[{"$keyword":{"namespace":null,"name":"int","spelling":"int"}},{"$keyword":{"namespace":null,"name":"float","spelling":"float"}},3,4,5,6,7,8]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-map-nine-pairs"###,
            r###"(let [m {1 :int 1.0 :float :b 3 :c 4 :d 5 :e 6 :f 7 :g 8 :h 9}] [(count m) (get m 1) (get m 1.0)])"###,
            r###"{"id":"literal-numeric-map-nine-pairs","outcome":{"status":"returned","value":{"$vector":[8,{"$keyword":{"namespace":null,"name":"int","spelling":"int"}},{"$keyword":{"namespace":null,"name":"int","spelling":"int"}}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-set-eight-scalars"###,
            r###"[(count #{1 1.0 :b :c :d :e :f :g}) (contains? #{1 1.0 :b :c :d :e :f :g} 1)]"###,
            r###"{"id":"literal-numeric-set-eight-scalars","outcome":{"status":"returned","value":{"$vector":[8,true]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-set-nine-scalars"###,
            r###"[(count #{1 1.0 :b :c :d :e :f :g :h}) (contains? #{1 1.0 :b :c :d :e :f :g :h} 1)]"###,
            r###"{"id":"literal-numeric-set-nine-scalars","outcome":{"status":"returned","value":{"$vector":[9,true]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-vector-map-keys"###,
            r###"(let [m {[1] :int [1.0] :float}] [(count m) (get m [1]) (vec (seq m))])"###,
            r###"{"id":"literal-numeric-vector-map-keys","outcome":{"status":"returned","value":{"$vector":[1,{"$keyword":{"namespace":null,"name":"float","spelling":"float"}},{"$vector":[{"$vector":[{"$vector":[1]},{"$keyword":{"namespace":null,"name":"float","spelling":"float"}}]}]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-quoted-vector-map-keys"###,
            r###"(let [m '{[1] :int [1.0] :float}] [(count m) (get m [1]) (vec (seq m))])"###,
            r###"{"id":"literal-numeric-quoted-vector-map-keys","outcome":{"status":"returned","value":{"$vector":[2,{"$keyword":{"namespace":null,"name":"int","spelling":"int"}},{"$vector":[{"$vector":[{"$vector":[1]},{"$keyword":{"namespace":null,"name":"int","spelling":"int"}}]},{"$vector":[{"$vector":[1]},{"$keyword":{"namespace":null,"name":"float","spelling":"float"}}]}]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-vector-set-members"###,
            r###"(try [(count #{[1] [1.0]})] (catch :default e [:duplicate]))"###,
            r###"{"id":"literal-numeric-vector-set-members","outcome":{"status":"returned","value":{"$vector":[{"$keyword":{"namespace":null,"name":"duplicate","spelling":"duplicate"}}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-quoted-vector-set-members"###,
            r###"(let [s '#{[1] [1.0]}] [(count s) (vec (seq s)) (contains? s [1])])"###,
            r###"{"id":"literal-numeric-quoted-vector-set-members","outcome":{"status":"returned","value":{"$vector":[2,{"$vector":[{"$vector":[1]},{"$vector":[1]}]},true]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-quoted-composite-map-keys"###,
            r###"(let [m '{{:x 1} :int {:x 1.0} :float} n '{#{1} :int #{1.0} :float}] [(count m) (get m {:x 1}) (count n) (get n #{1})])"###,
            r###"{"id":"literal-numeric-quoted-composite-map-keys","outcome":{"status":"returned","value":{"$vector":[2,{"$keyword":{"namespace":null,"name":"int","spelling":"int"}},2,{"$keyword":{"namespace":null,"name":"int","spelling":"int"}}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-quoted-composite-set-members"###,
            r###"(let [s '#{{:x 1} {:x 1.0}} t '#{#{1} #{1.0}}] [(count s) (count t) (contains? s {:x 1}) (contains? t #{1})])"###,
            r###"{"id":"literal-numeric-quoted-composite-set-members","outcome":{"status":"returned","value":{"$vector":[2,2,true,true]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-runtime-constructors"###,
            r###"(let [m (hash-map 1 :int 1.0 :float) s (set [1 1.0])] [(count m) (get m 1) (count s) (vec (seq s))])"###,
            r###"{"id":"literal-numeric-runtime-constructors","outcome":{"status":"returned","value":{"$vector":[1,{"$keyword":{"namespace":null,"name":"float","spelling":"float"}},1,{"$vector":[1]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-runtime-small-constructors"###,
            r###"[(count (array-map 1 :int 1.0 :float)) (get (array-map 1 :int 1.0 :float) 1) (count (hash-set 1 1.0))]"###,
            r###"{"id":"literal-numeric-runtime-small-constructors","outcome":{"status":"returned","value":{"$vector":[2,{"$keyword":{"namespace":null,"name":"int","spelling":"int"}},2]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-dynamic-map"###,
            r###"(let [a 1 b 1.0 m {a :int b :float}] [(count m) (get m a) (get m b) (vec (seq m))])"###,
            r###"{"id":"literal-numeric-dynamic-map","outcome":{"status":"returned","value":{"$vector":[1,{"$keyword":{"namespace":null,"name":"float","spelling":"float"}},{"$keyword":{"namespace":null,"name":"float","spelling":"float"}},{"$vector":[{"$vector":[1,{"$keyword":{"namespace":null,"name":"float","spelling":"float"}}]}]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-dynamic-set"###,
            r###"(let [a 1 b 1.0] (try [(count #{a b})] (catch :default e [:duplicate])))"###,
            r###"{"id":"literal-numeric-dynamic-set","outcome":{"status":"returned","value":{"$vector":[{"$keyword":{"namespace":null,"name":"duplicate","spelling":"duplicate"}}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-set-traversal-gap"###,
            r###"(let [s '#{1 1.0 '2 2}] [(count s) (contains? s 2) (contains? s '(quote 2)) (vec (seq s))])"###,
            r###"{"id":"literal-numeric-set-traversal-gap","outcome":{"status":"returned","value":{"$vector":[4,true,true,{"$vector":[1,{"$list":[{"$symbol":{"namespace":null,"name":"quote","spelling":"quote"}},2]},1,2]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-collision-and-nan-count"###,
            r###"(let [m {1 :int 1.0 :float ##NaN :nan} s #{1 1.0 ##NaN}] [(count m) (get m 1) (count s) (contains? s ##NaN)])"###,
            r###"{"id":"literal-numeric-collision-and-nan-count","outcome":{"status":"returned","value":{"$vector":[3,{"$keyword":{"namespace":null,"name":"int","spelling":"int"}},3,false]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-map-nine-reverse"###,
            r###"(let [m {1.0 :float 1 :int :b 3 :c 4 :d 5 :e 6 :f 7 :g 8 :h 9}] [(count m) (get m 1) (get m 1.0)])"###,
            r###"{"id":"literal-numeric-map-nine-reverse","outcome":{"status":"returned","value":{"$vector":[8,{"$keyword":{"namespace":null,"name":"int","spelling":"int"}},{"$keyword":{"namespace":null,"name":"int","spelling":"int"}}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-map-ten-categories"###,
            r###"(let [m {1 :int 1M :decimal 1.0 :float :b 3 :c 4 :d 5 :e 6 :f 7 :g 8 :h 9}] [(count m) (get m 1)])"###,
            r###"{"id":"literal-numeric-map-ten-categories","outcome":{"status":"returned","value":{"$vector":[8,{"$keyword":{"namespace":null,"name":"decimal","spelling":"decimal"}}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-map-nine-wide-integers"###,
            r###"(let [m {9007199254740992 :first 9007199254740993 :second :b 3 :c 4 :d 5 :e 6 :f 7 :g 8 :h 9}] [(count m) (get m 9007199254740992) (get m 9007199254740993)])"###,
            r###"{"id":"literal-numeric-map-nine-wide-integers","outcome":{"status":"returned","value":{"$vector":[8,{"$keyword":{"namespace":null,"name":"second","spelling":"second"}},{"$keyword":{"namespace":null,"name":"second","spelling":"second"}}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-map-nine-decimal-precision"###,
            r###"(let [m {1.0000000000000001M :precise 1M :rounded :b 3 :c 4 :d 5 :e 6 :f 7 :g 8 :h 9}] [(count m) (get m 1)])"###,
            r###"{"id":"literal-numeric-map-nine-decimal-precision","outcome":{"status":"returned","value":{"$vector":[8,{"$keyword":{"namespace":null,"name":"rounded","spelling":"rounded"}}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-map-nine-zero-categories"###,
            r###"(let [m {0 :int -0.0 :float :b 3 :c 4 :d 5 :e 6 :f 7 :g 8 :h 9}] [(count m) (get m 0) (get m -0.0)])"###,
            r###"{"id":"literal-numeric-map-nine-zero-categories","outcome":{"status":"returned","value":{"$vector":[8,{"$keyword":{"namespace":null,"name":"float","spelling":"float"}},{"$keyword":{"namespace":null,"name":"float","spelling":"float"}}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-metadata-wide-map"###,
            r###"(count ^:x {1000000000000000000000 1 1000000000000000000001 2})"###,
            r###"{"id":"literal-numeric-metadata-wide-map","outcome":{"status":"returned","value":2},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-numeric-metadata-wide-set"###,
            r###"(count ^:x #{1000000000000000000000 1000000000000000000001})"###,
            r###"{"id":"literal-numeric-metadata-wide-set","outcome":{"status":"returned","value":2},"trace":[],"stdout":""}"###,
        ),
    ];
    for (id, source, expected) in controls {
        for phase in [false, true] {
            let input = if phase {
                format!("(defmacro upstream [] (list 'quote (do {source}))) (upstream)")
            } else {
                source.to_string()
            };
            let js = light_cljs::compile_with_macros(&input, &mut host)
                .unwrap_or_else(|e| panic!("{id}, macro={phase}: {e}"));
            let captured = js.replacen(
                " = // Original bounded runtime",
                " = globalThis.lightRuntime = // Original bounded runtime",
                1,
            );
            let script = format!(
                "(() => {{const observerRuntime={observer_runtime};const value={captured};return ({})(globalThis.lightRuntime)('{id}',()=>value);}})()",
                include_str!("support/sci_execution_observation.js")
            );
            let runtime = rquickjs::Runtime::new().unwrap();
            let context = rquickjs::Context::full(&runtime).unwrap();
            let raw = context.with(|ctx| {
                ctx.eval::<String, _>(script.as_bytes())
                    .unwrap_or_else(|error| {
                        panic!("{id}, macro={phase}: {error:?}: {:?}", ctx.catch())
                    })
            });
            assert_eq!(
                canonical(&context, &raw),
                canonical(&context, expected),
                "{id}, macro={phase}"
            );
        }
    }
    let diagnostics: &[(&str, &str)] = &[
        (r###"{1 :first 0x1 :second}"###, r###"duplicate map key"###),
        (r###"#{1 0x1}"###, r###"duplicate set member"###),
        (r###"{1 :first 01 :second}"###, r###"duplicate map key"###),
        (r###"#{1 01}"###, r###"duplicate set member"###),
        (r###"{1 :first 1N :second}"###, r###"duplicate map key"###),
        (r###"#{1 1N}"###, r###"duplicate set member"###),
        (r###"{1 :first 2/2 :second}"###, r###"duplicate map key"###),
        (r###"#{1 2/2}"###, r###"duplicate set member"###),
        (
            r###"{1.0 :first 1e0 :second}"###,
            r###"duplicate map key"###,
        ),
        (r###"#{1.0 1e0}"###, r###"duplicate set member"###),
        (
            r###"{1M :first 1.0M :second}"###,
            r###"duplicate map key"###,
        ),
        (r###"#{1M 1.0M}"###, r###"duplicate set member"###),
        (
            r###"{-0.0 :first 0.0 :second}"###,
            r###"duplicate map key"###,
        ),
        (r###"#{-0.0 0.0}"###, r###"duplicate set member"###),
        (
            r###"{##Inf :first ##Inf :second}"###,
            r###"duplicate map key"###,
        ),
        (r###"#{##Inf ##Inf}"###, r###"duplicate set member"###),
        (
            r###"{9007199254740992.0 :first 9007199254740993.0 :second}"###,
            r###"duplicate map key"###,
        ),
        (
            r###"#{9007199254740992.0 9007199254740993.0}"###,
            r###"duplicate set member"###,
        ),
        (
            r###"{1/2 :first 2/4 :second}"###,
            r###"number is not a valid ClojureScript constant"###,
        ),
        (
            r###"#{1/2 2/4}"###,
            r###"number is not a valid ClojureScript constant"###,
        ),
        (
            r###"{:x {[1] :first [0x1] :second}}"###,
            r###"duplicate map key"###,
        ),
        (r###"'[#{1 1N}]"###, r###"duplicate set member"###),
        (r###"{1 :int 1.0}"###, r###"even number"###),
        (r###"{1 :int 1.0 :float"###, r###"expected"###),
        (r###"#{1 1.0"###, r###"expected"###),
        (
            r###"[(count {1/2 :ratio 0.5 :float}) (get {1/2 :ratio 0.5 :float} 0.5) (count #{1/2 0.5}) (vec (seq #{1/2 0.5}))]"###,
            r###"number is not a valid ClojureScript constant"###,
        ),
    ];
    for (source, diagnostic) in diagnostics {
        for phase in [false, true] {
            let input = if phase {
                format!("(defmacro upstream [] (list 'quote (do {source}))) (upstream)")
            } else {
                source.to_string()
            };
            let error = light_cljs::compile_with_macros(&input, &mut host)
                .unwrap_err()
                .to_string();
            assert!(
                error.contains(diagnostic)
                    || (*diagnostic == "number is not a valid ClojureScript constant"
                        && error.contains("NumericOverflow"))
                    || (phase
                        && *diagnostic == "duplicate map key"
                        && error.contains("DuplicateMapKey")),
                "{source}: {error}"
            );
            let js = light_cljs::compile_with_macros(
                "(defmacro literal-recovery [] (+ 20 22)) (literal-recovery)",
                &mut host,
            )
            .unwrap();
            let runtime = rquickjs::Runtime::new().unwrap();
            let context = rquickjs::Context::full(&runtime).unwrap();
            assert_eq!(
                context.with(|ctx| ctx.eval::<i32, _>(js.as_bytes()).unwrap()),
                42
            );
        }
    }
}
