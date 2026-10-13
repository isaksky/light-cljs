//! Canonical collection character/string literals and exact source phase evidence.
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
            .as_chunks::<2>()
            .0
            .iter()
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
fn literal_char_string_sources_execute_in_fresh_and_reused_macro_hosts() {
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
                    std::slice::from_ref(&sources),
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
    assert_eq!(input.lines().count(), 30);
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
                let error = light_cljs::compile_namespace_with_macros(
                    "user",
                    std::slice::from_ref(&folder),
                    host,
                )
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
fn canonical_literal_constants_and_same_host_diagnostic_recovery() {
    let mut host = MacroHost::new();
    let controls: &[(&str, &str, &str)] = &[
        (
            r###"literal-char-string-original"###,
            r###"[(count {\a 1 "a" 2}) (get {\a 1 "a" 2} \a) (count #{\a "a"}) (contains? #{\a "a"} "a")]"###,
            r###"{"id":"literal-char-string-original","outcome":{"status":"returned","value":{"$vector":[2,1,2,true]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-char-string-map-order-lookup"###,
            r###"(let [m {\a 1 "a" 2}] [(count m) (get m \a) (get m "a") (contains? m \a) (contains? m "a") (vec (seq m)) (vec (keys m)) (vec (vals m))])"###,
            r###"{"id":"literal-char-string-map-order-lookup","outcome":{"status":"returned","value":{"$vector":[2,1,1,true,true,{"$vector":[{"$vector":["a",1]},{"$vector":["a",2]}]},{"$vector":["a","a"]},{"$vector":[1,2]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-char-string-map-reverse-order"###,
            r###"(let [m {"a" 2 \a 1}] [(count m) (get m \a) (get m "a") (vec (seq m))])"###,
            r###"{"id":"literal-char-string-map-reverse-order","outcome":{"status":"returned","value":{"$vector":[2,2,2,{"$vector":[{"$vector":["a",2]},{"$vector":["a",1]}]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-char-string-set-order-membership"###,
            r###"(let [s #{\a "a"}] [(count s) (contains? s \a) (contains? s "a") (get s \a) (get s "a") (vec (seq s))])"###,
            r###"{"id":"literal-char-string-set-order-membership","outcome":{"status":"returned","value":{"$vector":[2,true,true,"a","a",{"$vector":["a","a"]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-char-string-set-reverse-order"###,
            r###"(let [s #{"a" \a}] [(count s) (vec (seq s)) (contains? s "a")])"###,
            r###"{"id":"literal-char-string-set-reverse-order","outcome":{"status":"returned","value":{"$vector":[2,{"$vector":["a","a"]},true]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-char-string-quoted"###,
            r###"(let [m '{\a 1 "a" 2} s '#{\a "a"}] [(count m) (get m \a) (vec (seq m)) (count s) (vec (seq s))])"###,
            r###"{"id":"literal-char-string-quoted","outcome":{"status":"returned","value":{"$vector":[2,1,{"$vector":[{"$vector":["a",1]},{"$vector":["a",2]}]},2,{"$vector":["a","a"]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-char-string-nested-literals"###,
            r###"(let [x [{:m {\a 1 "a" 2} :s #{\a "a"}}] m (get (first x) :m) s (get (first x) :s)] [(count m) (get m \a) (vec (seq m)) (count s) (vec (seq s))])"###,
            r###"{"id":"literal-char-string-nested-literals","outcome":{"status":"returned","value":{"$vector":[2,1,{"$vector":[{"$vector":["a",1]},{"$vector":["a",2]}]},2,{"$vector":["a","a"]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-char-string-quoted-nested"###,
            r###"(let [x '({:m {\a 1 "a" 2} :s #{\a "a"}}) m (get (first x) :m) s (get (first x) :s)] [(count m) (get m \a) (count s) (vec (seq s))])"###,
            r###"{"id":"literal-char-string-quoted-nested","outcome":{"status":"returned","value":{"$vector":[2,1,2,{"$vector":["a","a"]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-char-string-unicode-spelling"###,
            r###"[(count {\u0061 1 "a" 2}) (get {\u0061 1 "a" 2} \a) (count #{\u0061 "a"}) (vec (seq #{\u0061 "a"}))]"###,
            r###"{"id":"literal-char-string-unicode-spelling","outcome":{"status":"returned","value":{"$vector":[2,1,2,{"$vector":["a","a"]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-char-string-named-spelling"###,
            r###"[(count {\newline 1 "\n" 2}) (get {\newline 1 "\n" 2} \u000a) (count #{\space " "}) (vec (seq #{\space " "}))]"###,
            r###"{"id":"literal-char-string-named-spelling","outcome":{"status":"returned","value":{"$vector":[2,1,2,{"$vector":[" "," "]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-char-string-map-effects"###,
            r###"(let [events (atom []) m {\a (do (swap! events conj :first) 11) "a" (do (swap! events conj :second) 22)} output (with-out-str (println (count m)) (println (get m \a)))] [(count m) (get m \a) (vec (seq m)) @events output])"###,
            r###"{"id":"literal-char-string-map-effects","outcome":{"status":"returned","value":{"$vector":[2,11,{"$vector":[{"$vector":["a",11]},{"$vector":["a",22]}]},{"$vector":[{"$keyword":{"namespace":null,"name":"first","spelling":"first"}},{"$keyword":{"namespace":null,"name":"second","spelling":"second"}}]},"2\n11\n"]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-char-string-runtime-constructors"###,
            r###"(let [m (hash-map \a 1 "a" 2) s (set [\a "a"])] [(count m) (get m \a) (count s) (vec (seq s))])"###,
            r###"{"id":"literal-char-string-runtime-constructors","outcome":{"status":"returned","value":{"$vector":[1,2,1,{"$vector":["a"]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-char-string-dynamic-map"###,
            r###"(let [a \a b "a" m {a 1 b 2}] [(count m) (get m a) (get m b) (vec (seq m))])"###,
            r###"{"id":"literal-char-string-dynamic-map","outcome":{"status":"returned","value":{"$vector":[1,2,2,{"$vector":[{"$vector":["a",2]}]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-char-string-dynamic-set-distinct"###,
            r###"(let [a \a b "b"] [(count #{a b}) (contains? #{a b} "a") (contains? #{a b} "b")])"###,
            r###"{"id":"literal-char-string-dynamic-set-distinct","outcome":{"status":"returned","value":{"$vector":[2,true,true]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-char-string-map-eight-pairs"###,
            r###"(let [m {\a 1 "a" 2 :b 3 :c 4 :d 5 :e 6 :f 7 :g 8}] [(count m) (get m \a) (get m "a") (vec (vals m))])"###,
            r###"{"id":"literal-char-string-map-eight-pairs","outcome":{"status":"returned","value":{"$vector":[8,1,1,{"$vector":[1,2,3,4,5,6,7,8]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-char-string-map-nine-pairs"###,
            r###"(let [m {\a 1 "a" 2 :b 3 :c 4 :d 5 :e 6 :f 7 :g 8 :h 9}] [(count m) (get m \a) (get m "a")])"###,
            r###"{"id":"literal-char-string-map-nine-pairs","outcome":{"status":"returned","value":{"$vector":[8,2,2]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-char-string-set-ten-scalars"###,
            r###"(let [s #{\a "a" :b :c :d :e :f :g :h :i}] [(count s) (contains? s \a) (contains? s :i)])"###,
            r###"{"id":"literal-char-string-set-ten-scalars","outcome":{"status":"returned","value":{"$vector":[10,true,true]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-char-string-vector-map-keys"###,
            r###"(let [m {[\a] 1 ["a"] 2}] [(count m) (get m [\a]) (vec (seq m))])"###,
            r###"{"id":"literal-char-string-vector-map-keys","outcome":{"status":"returned","value":{"$vector":[1,2,{"$vector":[{"$vector":[{"$vector":["a"]},2]}]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-char-string-quoted-vector-map-keys"###,
            r###"(let [m '{[\a] 1 ["a"] 2}] [(count m) (get m [\a]) (vec (seq m))])"###,
            r###"{"id":"literal-char-string-quoted-vector-map-keys","outcome":{"status":"returned","value":{"$vector":[2,1,{"$vector":[{"$vector":[{"$vector":["a"]},1]},{"$vector":[{"$vector":["a"]},2]}]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-char-string-vector-set-distinct"###,
            r###"(let [s #{[\a] ["b"]}] [(count s) (contains? s ["a"]) (contains? s [\b])])"###,
            r###"{"id":"literal-char-string-vector-set-distinct","outcome":{"status":"returned","value":{"$vector":[2,true,true]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-char-string-quoted-vector-set-members"###,
            r###"(let [s '#{[\a] ["a"]}] [(count s) (vec (seq s)) (contains? s ["a"])])"###,
            r###"{"id":"literal-char-string-quoted-vector-set-members","outcome":{"status":"returned","value":{"$vector":[2,{"$vector":[{"$vector":["a"]},{"$vector":["a"]}]},true]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-char-string-quoted-composite-map-keys"###,
            r###"(let [m '{{\a 1} 11 {"a" 1} 22} n '{#{\a} 33 #{"a"} 44}] [(count m) (get m {"a" 1}) (count n) (get n #{"a"})])"###,
            r###"{"id":"literal-char-string-quoted-composite-map-keys","outcome":{"status":"returned","value":{"$vector":[2,11,2,33]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-char-string-quoted-composite-set-members"###,
            r###"(let [s '#{{\a 1} {"a" 1}} t '#{#{\a} #{"a"}}] [(count s) (count t) (contains? s {"a" 1}) (contains? t #{"a"})])"###,
            r###"{"id":"literal-char-string-quoted-composite-set-members","outcome":{"status":"returned","value":{"$vector":[2,2,true,true]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-char-string-runtime-small-constructor-difference"###,
            r###"[(count (array-map \a 1 "a" 2)) (get (array-map \a 1 "a" 2) \a) (count (hash-set \a "a"))]"###,
            r###"{"id":"literal-char-string-runtime-small-constructor-difference","outcome":{"status":"returned","value":{"$vector":[2,1,2]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-char-string-dynamic-set-duplicate-difference"###,
            r###"(let [a \a b "a"] (try [(count #{a b})] (catch :default e [:duplicate])))"###,
            r###"{"id":"literal-char-string-dynamic-set-duplicate-difference","outcome":{"status":"returned","value":{"$vector":[{"$keyword":{"namespace":null,"name":"duplicate","spelling":"duplicate"}}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-char-string-vector-set-duplicate-difference"###,
            r###"(try [(count #{[\a] ["a"]})] (catch :default e [:duplicate]))"###,
            r###"{"id":"literal-char-string-vector-set-duplicate-difference","outcome":{"status":"returned","value":{"$vector":[{"$keyword":{"namespace":null,"name":"duplicate","spelling":"duplicate"}}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-char-string-quoted-inner-quote-set"###,
            r###"(let [s '#{\a "a" '\b \b}] [(count s) (contains? s \b) (contains? s '(quote \b))])"###,
            r###"{"id":"literal-char-string-quoted-inner-quote-set","outcome":{"status":"returned","value":{"$vector":[4,true,true]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-char-string-quoted-inner-quote-set-order-difference"###,
            r###"(let [s '#{\a "a" '\b \b}] [(count s) (contains? s \b) (contains? s '(quote \b)) (vec (seq s))])"###,
            r###"{"id":"literal-char-string-quoted-inner-quote-set-order-difference","outcome":{"status":"returned","value":{"$vector":[4,true,true,{"$vector":["a","b",{"$list":[{"$symbol":{"namespace":null,"name":"quote","spelling":"quote"}},"b"]},"a"]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-char-string-quoted-inner-quote-map"###,
            r###"(let [m '{\a 1 "a" 2 '\b 3 \b 4}] [(count m) (get m \b) (get m '(quote \b)) (vec (vals m))])"###,
            r###"{"id":"literal-char-string-quoted-inner-quote-map","outcome":{"status":"returned","value":{"$vector":[4,4,3,{"$vector":[1,2,3,4]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"literal-char-string-numeric-literals"###,
            r###"[(count {1 :int 1.0 :float}) (get {1 :int 1.0 :float} 1) (count #{1 1.0}) (vec (seq #{1 1.0}))]"###,
            r###"{"id":"literal-char-string-numeric-literals","outcome":{"status":"returned","value":{"$vector":[2,{"$keyword":{"namespace":null,"name":"int","spelling":"int"}},2,{"$vector":[1,1]}]}},"trace":[],"stdout":""}"###,
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
                "(() => {{const value={captured};return ({})(globalThis.lightRuntime)('{id}',()=>value);}})()",
                include_str!("support/sci_execution_observation.js")
            );
            let runtime = rquickjs::Runtime::new().unwrap();
            let context = rquickjs::Context::full(&runtime).unwrap();
            let raw = context.with(|ctx| ctx.eval::<String, _>(script.as_bytes()).unwrap());
            assert_eq!(
                canonical(&context, &raw),
                canonical(&context, expected),
                "{id}, macro={phase}"
            );
        }
    }
    let diagnostics: &[(&str, &str)] = &[
        (r###"{\a 1 \a 2}"###, r###"duplicate map key"###),
        (r###"{\a 1 \u0061 2}"###, r###"duplicate map key"###),
        (r###"{"a" 1 "a" 2}"###, r###"duplicate map key"###),
        (r###"#{\a \a}"###, r###"duplicate set member"###),
        (r###"#{\a \u0061}"###, r###"duplicate set member"###),
        (r###"#{"a" "a"}"###, r###"duplicate set member"###),
        (r###"{:x {\a 1 \u0061 2}}"###, r###"duplicate map key"###),
        (r###"[#{"a" "a"}]"###, r###"duplicate set member"###),
        (r###"{\a 1 "a"}"###, r###"even number"###),
        (r###"{\a 1 "a" 2"###, r###"expected"###),
        (r###"#{\a "a""###, r###"expected"###),
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
