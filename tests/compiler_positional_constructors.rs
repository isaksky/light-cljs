//! Direct positional constructors and exact source phase evidence.
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
fn positional_constructors_sources_execute_in_fresh_and_reused_macro_hosts() {
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
    assert_eq!(input.lines().count(), 22);
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
fn canonical_positional_constructors_and_same_host_diagnostic_recovery() {
    let mut host = MacroHost::new();
    let controls: &[(&str, &str, &str)] = &[
        (
            r###"positional-constructor-original"###,
            r###"[(count (array-map \a 1 "a" 2)) (get (array-map \a 1 "a" 2) \a) (count (hash-set \a "a"))]"###,
            r###"{"id":"positional-constructor-original","outcome":{"status":"returned","value":{"$vector":[2,1,2]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"positional-constructor-direct-order"###,
            r###"(let [m (array-map \a 1 "a" 2) s (hash-set \a "a")] [(count m) (get m "a") (contains? m \a) (vec (seq m)) (vec (keys m)) (vec (vals m)) (count s) (get s "a") (contains? s \a) (vec (seq s))])"###,
            r###"{"id":"positional-constructor-direct-order","outcome":{"status":"returned","value":{"$vector":[2,1,true,{"$vector":[{"$vector":["a",1]},{"$vector":["a",2]}]},{"$vector":["a","a"]},{"$vector":[1,2]},2,"a",true,{"$vector":["a","a"]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"positional-constructor-reverse"###,
            r###"(let [m (array-map "a" 2 \a 1) s (hash-set "a" \a)] [(count m) (get m \a) (vec (seq m)) (count s) (vec (seq s))])"###,
            r###"{"id":"positional-constructor-reverse","outcome":{"status":"returned","value":{"$vector":[2,2,{"$vector":[{"$vector":["a",2]},{"$vector":["a",1]}]},2,{"$vector":["a","a"]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"positional-constructor-numeric"###,
            r###"(let [m (array-map 1 :int 1.0 :float) s (hash-set 1 1.0)] [(count m) (get m 1) (vec (seq m)) (count s) (vec (seq s))])"###,
            r###"{"id":"positional-constructor-numeric","outcome":{"status":"returned","value":{"$vector":[2,{"$keyword":{"namespace":null,"name":"int","spelling":"int"}},{"$vector":[{"$vector":[1,{"$keyword":{"namespace":null,"name":"int","spelling":"int"}}]},{"$vector":[1,{"$keyword":{"namespace":null,"name":"float","spelling":"float"}}]}]},2,{"$vector":[1,1]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"positional-constructor-equal"###,
            r###"[(vec (seq (array-map "a" 1 "a" 2))) (vec (seq (hash-set "a" "a"))) (count (array-map 1 :one 0x1 :two)) (count (hash-set 1 0x1))]"###,
            r###"{"id":"positional-constructor-equal","outcome":{"status":"returned","value":{"$vector":[{"$vector":[{"$vector":["a",2]}]},{"$vector":["a"]},1,1]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"positional-constructor-saved"###,
            r###"(let [a array-map h hash-set m (a \a 1 "a" 2) s (h \a "a")] [(count m) (get m \a) (vec (seq m)) (count s) (vec (seq s))])"###,
            r###"{"id":"positional-constructor-saved","outcome":{"status":"returned","value":{"$vector":[1,2,{"$vector":[{"$vector":["a",2]}]},1,{"$vector":["a"]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"positional-constructor-apply"###,
            r###"(let [m (apply array-map [\a 1 "a" 2]) s (apply hash-set [\a "a"])] [(count m) (get m \a) (vec (seq m)) (count s) (vec (seq s))])"###,
            r###"{"id":"positional-constructor-apply","outcome":{"status":"returned","value":{"$vector":[1,2,{"$vector":[{"$vector":["a",2]}]},1,{"$vector":["a"]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"positional-constructor-dynamic"###,
            r###"(let [a \a b "a" m (array-map a 1 b 2) s (hash-set a b)] [(count m) (get m a) (vec (seq m)) (count s) (vec (seq s))])"###,
            r###"{"id":"positional-constructor-dynamic","outcome":{"status":"returned","value":{"$vector":[1,2,{"$vector":[{"$vector":["a",2]}]},1,{"$vector":["a"]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"positional-constructor-zero-one"###,
            r###"[(count (array-map)) (count (hash-set)) (count (apply array-map [])) (count (apply hash-set [])) (vec (seq (array-map :x 1))) (vec (seq (hash-set :x)))]"###,
            r###"{"id":"positional-constructor-zero-one","outcome":{"status":"returned","value":{"$vector":[0,0,0,0,{"$vector":[{"$vector":[{"$keyword":{"namespace":null,"name":"x","spelling":"x"}},1]}]},{"$vector":[{"$keyword":{"namespace":null,"name":"x","spelling":"x"}}]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"positional-constructor-eight"###,
            r###"(let [m (array-map \a 1 "a" 2 :b 3 :c 4 :d 5 :e 6 :f 7 :g 8) s (hash-set \a "a" :b :c :d :e :f :g)] [(count m) (get m \a) (vec (vals m)) (count s) (vec (seq s))])"###,
            r###"{"id":"positional-constructor-eight","outcome":{"status":"returned","value":{"$vector":[8,1,{"$vector":[1,2,3,4,5,6,7,8]},8,{"$vector":["a","a",{"$keyword":{"namespace":null,"name":"b","spelling":"b"}},{"$keyword":{"namespace":null,"name":"c","spelling":"c"}},{"$keyword":{"namespace":null,"name":"d","spelling":"d"}},{"$keyword":{"namespace":null,"name":"e","spelling":"e"}},{"$keyword":{"namespace":null,"name":"f","spelling":"f"}},{"$keyword":{"namespace":null,"name":"g","spelling":"g"}}]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"positional-constructor-nine"###,
            r###"(let [m (array-map \a 1 "a" 2 :b 3 :c 4 :d 5 :e 6 :f 7 :g 8 :h 9) s (hash-set \a "a" :b :c :d :e :f :g :h)] [(count m) (get m \a) (vec (vals m)) (count s) (contains? s \a) (contains? s :h)])"###,
            r###"{"id":"positional-constructor-nine","outcome":{"status":"returned","value":{"$vector":[9,1,{"$vector":[1,2,3,4,5,6,7,8,9]},8,true,true]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"positional-constructor-metadata-hash"###,
            r###"(let [m (with-meta (array-map \a 1 "a" 2) {:tag :map}) s (with-meta (hash-set \a "a") {:tag :set})] [(meta m) (meta s) (count m) (count s) (= (hash m) (hash (array-map \a 1 "a" 2))) (= (hash s) (hash (hash-set \a "a"))) (= (hash m) (hash (with-meta m {:other true}))) (= (hash s) (hash (with-meta s {:other true})))])"###,
            r###"{"id":"positional-constructor-metadata-hash","outcome":{"status":"returned","value":{"$vector":[{"$map":[[{"$keyword":{"namespace":null,"name":"tag","spelling":"tag"}},{"$keyword":{"namespace":null,"name":"map","spelling":"map"}}]],"count":1},{"$map":[[{"$keyword":{"namespace":null,"name":"tag","spelling":"tag"}},{"$keyword":{"namespace":null,"name":"set","spelling":"set"}}]],"count":1},2,2,true,true,true,true]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"positional-constructor-map-updates"###,
            r###"(let [m (with-meta (array-map \a 1 "a" 2) {:tag :map}) a (assoc m "a" 3) d (dissoc m "a") c (conj m ["a" 4])] [(count a) (get a \a) (vec (seq a)) (count d) (mapv (fn [e] [(undefined? (key e)) (undefined? (val e))]) (seq d)) (count c) (get c \a) (vec (seq c)) (meta a) (meta d) (meta c) (count m) (get m \a)])"###,
            r###"{"id":"positional-constructor-map-updates","outcome":{"status":"returned","value":{"$vector":[2,3,{"$vector":[{"$vector":["a",3]},{"$vector":["a",2]}]},1,{"$vector":[{"$vector":[true,true]}]},2,4,{"$vector":[{"$vector":["a",4]},{"$vector":["a",2]}]},{"$map":[[{"$keyword":{"namespace":null,"name":"tag","spelling":"tag"}},{"$keyword":{"namespace":null,"name":"map","spelling":"map"}}]],"count":1},{"$map":[[{"$keyword":{"namespace":null,"name":"tag","spelling":"tag"}},{"$keyword":{"namespace":null,"name":"map","spelling":"map"}}]],"count":1},{"$map":[[{"$keyword":{"namespace":null,"name":"tag","spelling":"tag"}},{"$keyword":{"namespace":null,"name":"map","spelling":"map"}}]],"count":1},2,1]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"positional-constructor-set-updates"###,
            r###"(let [s (with-meta (hash-set \a "a") {:tag :set}) c (conj s "a") d (disj s "a")] [(count c) (vec (seq c)) (count d) (contains? d "a") (mapv undefined? (seq d)) (meta c) (meta d) (count s)])"###,
            r###"{"id":"positional-constructor-set-updates","outcome":{"status":"returned","value":{"$vector":[2,{"$vector":["a","a"]},1,false,{"$vector":[true]},{"$map":[[{"$keyword":{"namespace":null,"name":"tag","spelling":"tag"}},{"$keyword":{"namespace":null,"name":"set","spelling":"set"}}]],"count":1},{"$map":[[{"$keyword":{"namespace":null,"name":"tag","spelling":"tag"}},{"$keyword":{"namespace":null,"name":"set","spelling":"set"}}]],"count":1},2]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"positional-constructor-promotion"###,
            r###"(let [m (array-map \a 1 "a" 2 :b 3 :c 4 :d 5 :e 6 :f 7 :g 8) s (hash-set \a "a" :b :c :d :e :f :g) a (assoc m :h 9) c (conj s :h)] [(count a) (get a "a") (get a :h) (count c) (contains? c "a") (contains? c :h) (count m) (count s)])"###,
            r###"{"id":"positional-constructor-promotion","outcome":{"status":"returned","value":{"$vector":[8,2,9,8,true,true,8,8]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"positional-constructor-unicode"###,
            r###"[(count (array-map \u0061 1 "a" 2)) (get (array-map \newline 1 "\n" 2) \u000a) (count (hash-set \space " "))]"###,
            r###"{"id":"positional-constructor-unicode","outcome":{"status":"returned","value":{"$vector":[2,1,2]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"positional-constructor-quoted"###,
            r###"(let [m (array-map '\a 1 "a" 2) s (hash-set '\a "a")] [(count m) (get m "a") (count s) (contains? s "a")])"###,
            r###"{"id":"positional-constructor-quoted","outcome":{"status":"returned","value":{"$vector":[2,1,2,true]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"positional-constructor-hash-map-set-dedup"###,
            r###"(let [m (hash-map \a 1 "a" 2) s (set [\a "a"]) n (set (list \a "a"))] [(count m) (get m \a) (count s) (count n) (vec (seq s))])"###,
            r###"{"id":"positional-constructor-hash-map-set-dedup","outcome":{"status":"returned","value":{"$vector":[1,2,1,1,{"$vector":["a"]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"positional-constructor-operand-effects"###,
            r###"(let [events (atom []) m (array-map \a (do (swap! events conj :first) 11) "a" (do (swap! events conj :second) 22)) s (hash-set (do (swap! events conj :third) \a) (do (swap! events conj :fourth) "a")) output (with-out-str (println (count m)) (println (get m \a)))] [(count m) (get m \a) (count s) @events output])"###,
            r###"{"id":"positional-constructor-operand-effects","outcome":{"status":"returned","value":{"$vector":[2,11,1,{"$vector":[{"$keyword":{"namespace":null,"name":"first","spelling":"first"}},{"$keyword":{"namespace":null,"name":"second","spelling":"second"}},{"$keyword":{"namespace":null,"name":"third","spelling":"third"}},{"$keyword":{"namespace":null,"name":"fourth","spelling":"fourth"}}]},"2\n11\n"]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"positional-constructor-equality-effects"###,
            r###"(let [events (atom []) k (reify IEquiv (-equiv [this other] (swap! events conj :equiv) (identical? this other)) IHash (-hash [this] (swap! events conj :hash) 7)) m (array-map k 1 k 2) s (hash-set k k)] [(count m) (get m k) (count s) (contains? s k) @events])"###,
            r###"{"id":"positional-constructor-equality-effects","outcome":{"status":"returned","value":{"$vector":[1,2,1,true,{"$vector":[]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"positional-constructor-odd-recovery"###,
            r###"(let [a array-map events (atom []) result (try (a :a (do (swap! events conj :value) 1) :b) (catch :default e (.-message e)))] [result @events (count (a :ok 42))])"###,
            r###"{"id":"positional-constructor-odd-recovery","outcome":{"status":"returned","value":{"$vector":["No value supplied for key: :b",{"$vector":[{"$keyword":{"namespace":null,"name":"value","spelling":"value"}}]},1]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"positional-constructor-saved-boundary"###,
            r###"(let [a array-map h hash-set m (a \a 1 "a" 2 :b 3 :c 4 :d 5 :e 6 :f 7 :g 8 :h 9) s (apply h [\a "a" :b :c :d :e :f :g :h])] [(count m) (get m \a) (count s) (contains? s :h)])"###,
            r###"{"id":"positional-constructor-saved-boundary","outcome":{"status":"returned","value":{"$vector":[8,2,8,true]}},"trace":[],"stdout":""}"###,
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
    let diagnostics: &[(&str, &str)] = &[(
        r###"(array-map :a {1 1 0x1 2})"###,
        r###"duplicate map key"###,
    )];
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
