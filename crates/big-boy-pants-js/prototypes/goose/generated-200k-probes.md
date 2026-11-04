# Generated 200k inventory: reproducible probes

These bounded diagnostic inputs accompany [the target](generated-200k.md).
They are documentation, not a second build/comparison harness. Run them only in
a disposable checkout of the inventory revision; they temporarily add test-only
hooks and must be removed afterward. They neither change the generated project
nor bypass an unsupported execution operation. The inspector parses/lowerings
all functions and inventories rejected opcodes without admitting them to Goose.

Observed with Rust 1.99.1, Babashka 1.12.218, Node 26.12.1, Goose
`v2026.3-267-g369c7a2c` / runtime `3025e5810379`, clang 21.0.0 on macOS arm64.
Both native debug and optimized runners reproduce the same boundaries:

- All eleven real macro-body calls succeed in V8 and fail explicitly in Goose
  as listed in the target. `gaikmenfeik 1 2 3` returns 169 in V8; `joktol x 99`
  returns the original symbol with `:locals {x {}}`.
- All 741 native-emitted macro factories parse. Export outcomes: 285 success,
  399 `new_arguments`, 57 `rest`. Full frontend inspection finds only those two
  unsupported opcodes, even behind the first rejection.
- Native leaf probes: `contains`, `concat`, `concat-all`, `sequence`, `map-seq`,
  `fn-apply-null`, `callable`, `invoke` => `Fault.NativeBoundary`;
  `array-length` and `function-call` => `Fault.UnsupportedOperand`.
- The real first worker succeeds in V8. Goose rejects
  `workload.jokoum:7:1`, macro `workload.fonam/stipopi`, definition line 34:
  `4:5: SyntaxError: Goose export unsupported operation new_arguments in function l0`.
- Demanding the actual `favoukstun` helper directly reaches factory execution
  but callable publication fails `helper-Fault.None-Error.Unsupported`.
  The exporter hook captures the exact prepared canonical helper factory;
  it uses `rt.core.list`, `rt.symbol` and `.call`. Returning a callable is an
  investigation of the helper transport, not an allowed compiler expansion form.

Successful diagnostic test exits mean the probes ran and recorded outcomes;
**they do not mean unsupported cases passed correctness**. No full 200k build
or timing was attempted here. Generator tests passed 9 tests / 353 assertions;
full-target repeat generation matched every source/shim. No production Rust or
Goose changes remain. Existing broad-suite and strict-lint limitations were not
retested or represented as passes.

## Repeat in an isolated checkout

Generate `target/generated-200k` using the exact command in the target. Install
these two Rust blocks and the exporter observation hook with Babashka:

```sh
bb -e '
(require (quote [babashka.fs :as fs]) (quote [clojure.string :as s]))
(let [dir "crates/big-boy-pants-js/prototypes/goose/"
      blocks (map second (re-seq #"(?s)```rust\n(.*?)\n```" (slurp (str dir "generated-200k-probes.md"))))
      capture (str dir "capture.rs") exporter (str dir "export.rs") adapter "src/goose_evaluator.rs"]
  (assert (= 2 (count blocks)))
  (doseq [[path name] [[capture "capture"] [exporter "export"] [adapter "adapter"]]]
    (assert (not (fs/exists? (str "target/745-" name ".original"))))
    (fs/copy path (str "target/745-" name ".original")))
  (spit "target/probe.rs" (first blocks))
  (spit capture "\n#[cfg(feature = \"goose-macros\")]\n#[path = \"../../../../target/probe.rs\"]\nmod generated_745_probe;\n" :append true)
  (spit exporter (str "\n" (second blocks) "\n") :append true)
  (let [needle "fn export(&self, path: &Path, source: &str) -> Result<(), EvalError> {"
        source (slurp adapter)]
    (assert (s/includes? source needle))
    (spit adapter (s/replace source needle (str needle "\n        #[cfg(test)] crate::compiler_bootstrap::goose_capture::unit(source, true);")))))'
export CARGO_TARGET_DIR="$PWD/target" GOOSE_HOME="$PWD/target/goose"
export CARGO_BUILD_JOBS=3 LIGHT_CLJS_V8_CACHE=require
# Both native modes use the same current VM, not a patched admission path.
goose build crates/big-boy-pants-js/prototypes/goose/compiler_session.goose --debug -o target/745-runner-debug
goose build crates/big-boy-pants-js/prototypes/goose/compiler_session.goose -o target/745-runner-native
export GOOSE_COMPILER_RUNNER="$PWD/target/745-runner-debug"
cargo test --locked --features goose-macros --lib generated_745_inventory -- --include-ignored --nocapture --test-threads=1
cargo test --locked --features goose-macros --lib generated_745_ -- --include-ignored --nocapture --test-threads=1
export GOOSE_COMPILER_RUNNER="$PWD/target/745-runner-native"
cargo test --locked --features goose-macros --lib generated_745_ -- --include-ignored --nocapture --test-threads=1
```

Outputs under `target/745-inventory`: exact emitted factory JS, per-declaration
export result (`inventory.tsv`), full frontend opcode/form-op/rejected-opcode
sets (`complete.tsv`), and the demanded helper factory. Qualified source macro
identities and original source paths remain in the diagnostics. No output is
used as a pre-expanded compiler input. Probe imports use a small explicit
`&env`; the first-worker probe separately exercises the actual loader/analysis
path with the original project sources.

Restore the three files and remove only these owned temporary hooks/binaries:

```sh
bb -e '
(require (quote [babashka.fs :as fs]))
(let [dir "crates/big-boy-pants-js/prototypes/goose/"]
  (doseq [[path name] [[(str dir "capture.rs") "capture"] [(str dir "export.rs") "export"] ["src/goose_evaluator.rs" "adapter"]]]
    (fs/copy (str "target/745-" name ".original") path {:replace-existing true})
    (fs/delete (str "target/745-" name ".original")))
  (doseq [path ["target/probe.rs" "target/745-runner-debug" "target/745-runner-native"]]
    (fs/delete-if-exists path)))'
git diff --check
```

## Preserved compiler probe

```rust
use super::*;
use std::time::Instant;
fn read(text: &str) -> Form {
    let source = crate::compiler_data::Source::new("generated-probe.cljs", text);
    crate::compiler_reader::read_form(&source, &crate::compiler_reader::NamespaceView::new("workload.fonam", &source)).unwrap().unwrap()
}
fn host_for(source: &str, goose: bool) -> (MacroHost, Option<std::sync::Arc<crate::goose_evaluator::GooseSession>>) {
    let mut host = MacroHost::new();
    host.evaluator_options.memory_limit = 2 << 30;
    host.evaluator_options.poll_budget = 100_000_000_000;
    host.evaluator_options.timeout = std::time::Duration::from_secs(180);
    let session = goose.then(|| crate::goose_evaluator::GooseSession::start(&PathBuf::from(std::env::var_os("GOOSE_COMPILER_RUNNER").unwrap()), 1).unwrap());
    if let Some(s) = &session { host.use_goose_source_macros(s.clone()).unwrap(); }
    let ns = source.lines().next().unwrap().trim_start_matches("(ns ").trim_end_matches(")");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/generated-200k");
    crate::compiler::analyze_namespace_entry_recorded(ns, &[root.join("src"), root.join("macros")], &mut host, Default::default(), &mut Vec::new(), crate::compiler::EntryRecorders { slots: crate::scheduler::Slots::new(1).unwrap(), ..Default::default() }).unwrap();
    (host, session)
}
#[test]
#[ignore]
fn generated_745_inventory() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let generated = root.join("target/generated-200k");
    let out = root.join("target/745-inventory"); std::fs::create_dir_all(&out).unwrap();
    let mut paths = std::fs::read_dir(generated.join("macros/workload")).unwrap().map(|p| p.unwrap().path()).collect::<Vec<_>>(); paths.sort();
    let mut inventory = String::new();
    for path in paths {
        let source = std::fs::read_to_string(&path).unwrap();
        let (host, session) = host_for(&source, true);
        for (key, declaration) in host.tables.javascript_macros.iter() {
            if !key.starts_with("workload.") { continue; }
            let MacroFunction::Ready(_, Some(code)) = &declaration.function else { panic!("code missing") };
            let factory = format!("rt => rt.core.macroReaderFunction(({})(rt))", code.unit);
            let stem = key.replace('/', "-");
            std::fs::write(out.join(format!("{stem}.js")), &factory).unwrap();
            match big_boy_pants_js::frontend::goose::export_source(&stem, &factory) {
                Ok(e) => inventory.push_str(&format!("{key}\tOK\t{:?}\t{:?}\t{:?}\n", e.operations, e.form_operations, e.string_constants)),
                Err(e) => inventory.push_str(&format!("{key}\tERROR\t{e}\n")),
            }
        }
        session.unwrap().close();
    }
    std::fs::write(out.join("inventory.tsv"), inventory).unwrap();
}
#[test]
#[ignore]
fn generated_745_calls() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source = std::fs::read_to_string(root.join("target/generated-200k/macros/workload/fonam.cljc")).unwrap();
    for (name, args) in [("kalvas", "[2 3] 4"),("dukdos", "7"),("graidamha", "2 3 4"),("gaikmenfeik", "1 2 3"),("joktol", "x 99"),("buvaiskum", "1 2 3"),("gakzoultrei-grekvavus", "[x 7] x"),("zalstok", "sample [x] x"),("stipopi", "P (m [x])"),("marore-jaipla", "R [x]"),("dougri", "mm")] {
        for goose in [false, true] {
            let _names = crate::generated_names::enter_namespace(0);
            let (mut host, session) = host_for(&source, goose);
            let call = read(&format!("({name} {args})"));
            let env = read("{:ns {:name workload.fonam} :locals {x {}}}").data();
            let begun = Instant::now();
            let result = host.expand_source_javascript("workload.fonam", name, &call, &env, None);
            eprintln!("PROBE {name} goose={goose} elapsed={:?} result={:?}", begun.elapsed(), result.as_ref().map(|r|r.data()));
            if let Some(s) = session { eprintln!("STATS {:?}", s.stats()); s.close(); }
        }
    }
}
#[test]
#[ignore]
fn generated_745_complete_frontend_inventory() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/745-inventory");
    let mut paths = std::fs::read_dir(&root).unwrap().map(|p| p.unwrap().path()).filter(|p| p.file_name().unwrap().to_str().unwrap().starts_with("workload.") && p.extension().is_some_and(|e|e=="js")).collect::<Vec<_>>();paths.sort();
    let mut rows = String::new();
    for p in paths { rows.push_str(&format!("{}\t{}\n", p.file_stem().unwrap().to_str().unwrap(), big_boy_pants_js::frontend::goose::generated_745_inspect(&std::fs::read_to_string(&p).unwrap()))); }
    std::fs::write(root.join("complete.tsv"), rows).unwrap();
}
#[test]
#[ignore]
fn generated_745_real_worker() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/generated-200k");
    for goose in [false,true] {
        let mut host=MacroHost::new();host.evaluator_options.memory_limit=2<<30;host.evaluator_options.poll_budget=100_000_000_000;
        host.evaluator_options.timeout=std::time::Duration::from_secs(180);
        let session=goose.then(||crate::goose_evaluator::GooseSession::start(&PathBuf::from(std::env::var_os("GOOSE_COMPILER_RUNNER").unwrap()),1).unwrap());
        if let Some(s)=&session{host.use_goose_source_macros(s.clone()).unwrap();}
        let result=crate::compiler::analyze_namespace_entry_recorded("workload.jokoum",&[root.join("src"),root.join("macros")],&mut host,Default::default(),&mut Vec::new(),crate::compiler::EntryRecorders{slots:crate::scheduler::Slots::new(1).unwrap(),..Default::default()});
        eprintln!("REAL-WORKER goose={goose} {:?}",result.map(|_|"success"));if let Some(s)=session{s.close();}
    }
}
#[test]
#[ignore]
fn generated_745_leaves() {
    use crate::evaluator::MacroEvaluator;
    for (name, body) in [
        ("contains", "__rt.cljContains(__rt.formMap(__rt.formKeyword(null, 'locals'), null), __rt.formKeyword(null, 'locals'))"),
        ("concat", "__rt.cljConcat(__rt.formList(1), __rt.formList(2))"),
        ("concat-all", "__rt.cljConcatAll(__rt.formList(__rt.formList(1), __rt.formList(2)))"),
        ("array-length", "(() => {const a=[1,2,3];a.length=1;return a[0];})()"),
        ("sequence", "globalThis.__lightNativeRuntime.core.sequence(__rt.formList(1))"),
        ("map-seq", "globalThis.__lightNativeRuntime.core.mapSeq((x)=>x, __rt.formList(1))"),
        ("fn-apply-null", "__rt.fnApplyNull((x)=>x)"),
        ("function-call", "((x)=>x).call(null, 1)"),
        ("callable", "__rt.cljCallable((x)=>x)"),
        ("invoke", "__rt.cljInvoke((x)=>x, 1, null, 1)")
    ] {
        let session=crate::goose_evaluator::GooseSession::start(&PathBuf::from(std::env::var_os("GOOSE_COMPILER_RUNNER").unwrap()),1).unwrap();
        let e=crate::goose_evaluator::GooseEvaluator::new(session.clone(),&Default::default()).unwrap();
        let f=e.evaluate_unit(&format!("rt => (form, env) => {body}")).unwrap();
        let form=read("nil");let a=e.import_form(&form,Default::default()).unwrap();let b=e.import_data(&form.data(),Default::default()).unwrap();
        eprintln!("LEAF {name}: {:?}",e.call(f,&[a,b]));session.close();
    }
}
#[test]
#[ignore]
fn generated_745_helper_factory() {
    use crate::evaluator::MacroEvaluator;
    let root=PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source=std::fs::read_to_string(root.join("target/generated-200k/macros/workload/fonam.cljc")).unwrap();
    let (host,session)=host_for(&source,true); let e=host.engine.goose.as_ref().unwrap();
    host.session_handle().unwrap().prepare_goose(e).unwrap();
    UNITS.with_borrow_mut(|r| *r=Some(Default::default()));
    let f=e.evaluate_unit("rt => (form, env) => __rt.cljReadRoot(globalThis.__lightCompilerRoots, null, 'workload.fonam/favoukstun')").unwrap();
    let call=read("nil");let a=e.import_form(&call,Default::default()).unwrap();let b=e.import_data(&call.data(),Default::default()).unwrap();
    eprintln!("HELPER-ONLY {:?}",e.call(f,&[a,b]));
    for (i,(native,unit)) in UNITS.with_borrow_mut(|r|r.take().unwrap()).into_iter().enumerate(){
        if native{std::fs::write(root.join(format!("target/745-inventory/helper-{i}.js")),&unit).unwrap(); eprintln!("HELPER-FACTORY {}",big_boy_pants_js::frontend::goose::generated_745_inspect(&unit));}
    }
    session.unwrap().close();
}

```

## Read-only frontend inspector

```rust
pub fn generated_745_inspect(source: &str) -> String {
    fn walk(e: &Executable, all: &mut BTreeSet<String>, forms: &mut BTreeSet<String>, missing: &mut BTreeSet<String>) {
        let s = e.to_spec();
        for child in &s.functions { walk(child, all, forms, missing); }
        let mut i = 0;
        while i < s.code.len() {
            let op = Opcode::from_byte(s.code[i]).unwrap();
            all.insert(op.name().to_owned());
            if !supported(op) { missing.insert(op.name().to_owned()); }
            let n = op.operand().size();
            if op == Opcode::FormOp {
                let arg = u16::from_le_bytes([s.code[i+1],s.code[i+2]]);
                forms.insert(format!("{:?}",crate::vm::instr::FormOp::from_u16(arg).unwrap()));
            }
            i += 1+n;
        }
    }
    let e = compile_script("generated-745", source).unwrap();
    let (mut all, mut forms, mut missing) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    walk(&e, &mut all, &mut forms, &mut missing);
    format!("{:?}\t{:?}\t{:?}", all, forms, missing)
}

```

## Static source counts

Save this block as `target/745-source-inventory.bb` and run
`bb target/745-source-inventory.bb`. It reads all canonical nonmacro/nonshim
files, resolves namespace aliases and counts actual source AST heads, without
claiming the counts are dynamic executions.

```clojure
(require '[babashka.fs :as fs] '[clojure.edn :as edn] '[clojure.string :as str] '[edamame.core :as reader])
(def root "target/generated-200k")
(def manifest (edn/read-string (slurp (str root "/manifest.edn"))))
(def model (edn/read-string (slurp (str root "/model.edn"))))
(def counts (atom {}))
(def core-heads (atom {}))
(doseq [{:keys [path role]} (:files manifest) :when (not (#{:shim :macro} role))]
  (let [source (slurp (str root "/" path))
        ns-form (reader/parse-string source {:all true})
        aliases (into {} (for [clause (rest ns-form) :when (and (seq? clause) (= :require (first clause))) [lib _ alias] (rest clause)] [alias lib]))
        current (second ns-form)
        forms (reader/parse-string-all source {:all true :auto-resolve #(if (= :current %) current (aliases %))})]
    (doseq [form (tree-seq coll? seq forms) :when (and (seq? form) (symbol? (first form)))]
      (let [head (first form) namespace (namespace head) nm (name head)
            owner (if namespace (str (get aliases (symbol namespace) namespace)) (str current))
            macro (get (:macros model) [owner nm])]
        (when macro (swap! counts update (:kind macro) (fnil inc 0)))
        (when (contains? '#{defprotocol defrecord deftype defmulti defmethod extend-protocol extend-type} head)
          (swap! core-heads update head (fnil inc 0)))))))
(prn {:source-call-sites (into (sorted-map) @counts) :source-structural-heads (into (sorted-map) @core-heads)})

```

## Investigation build identities

These identify the actual diagnostic run, not portable binary expectations.
The temporary Rust test binary includes exactly the hooks/blocks above; the
Goose VM/runner sources are unchanged from the inventory revision.

| Input/output | SHA-256 |
| --- | --- |
| temporary probe.rs | `8c5d484cfb943e619f5e823ee2f68dc9ce3d9493245d749870d9c379cafc8086` |
| Rust lib-test binary | `f0db18d93494d60d50236574e741be504a2c76247ec18b6b813bd824ff4b01d1` |
| native debug runner | `4d11a308bdd18c8970ecba1d03e4e5252be64ae63d9ef558a73e99258f3fd280` |
| native optimized runner | `7c6d8ab7a22f1cf2c0b358ebebb15c2cce45553e7e754b845f85a172f0d88ae8` |
| prepared favoukstun factory | `0b21bff7f17bc339e126331858616ce92855a144e468907422e7c6895f38d3a4` |
| 741-factory sorted hash manifest | `0de90e6c1a6ada95c28c54780010d54d167bd208b9a66c86781b70edec2a937f` |

The last digest hashes UTF-8 lines `FILENAME<TAB>FILE_SHA256<LF>`, sorted by
filename, for the 741 `workload.*.js` declaration factories. Helpers are separate.
Final diagnostic runs: six tests completed in each native mode, with the
intentional failure signatures above. No performance inference uses their
elapsed times.

## Variadic wrapper regression

After the wrapper implementation, repeating the recipe above on the unchanged
seed42/200k sources exports all **741/741** factories successfully. This includes
the nested `function(_, ...args)` adapter. Check this explicitly after the
inventory command:

```sh
bb -e '(require (quote [clojure.string :as s])) (let [rows (s/split-lines (slurp "target/745-inventory/inventory.tsv"))] (assert (= 741 (count rows))) (assert (every? #(s/includes? % "\tOK\t") rows)))'
```

The actual `workload.fonam/gaikmenfeik 1 2 3` returns `169` in V8 and both
native Goose modes. The permanent `goose_compiler_variadic_wrappers` test also
executes its original declaration for zero, one and three arguments and checks
`[166 167 169]`, identical emitted application JS, three real Goose jobs and
one preparation. Argument/rest/call receiver cases, every serial budget cut,
resource rejection, private preparation snapshots and helper suspension are
covered by `serial_tests.goose` and the compiler wrapper tests.

The real first worker still succeeds in V8. Both native Goose modes now reach
`Fault.NativeBoundary` at `workload.jokoum:7:1`, macro
`workload.fonam/stipopi`, definition `fonam.cljc:34:1`, function 8, pc 55,
emitted span `562:563`, writes 5. Subtracting the ten-byte `rt => null` base
source locates factory span `552:553`, the `cljConcatAll(...)` call. This is the
separately scoped collection operation, after wrapper execution. It is not a
successful first-worker or full-target build, and no timing claim follows.
