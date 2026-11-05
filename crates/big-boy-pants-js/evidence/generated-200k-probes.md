# Generated 200k inventory: reproducible probes

These bounded diagnostic inputs accompany [the target](generated-200k.md).
They are documentation, not a second build/comparison harness. The original
observations below describe the inventory revision; the later recheck has its
own revision and results. Run the temporary hooks only in a disposable checkout
and remove them afterward. They neither change the generated project
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

## Execution-support recheck

November 4, 2026, on `8509a79c9e1040f2c49af1ccfb3c3ec442daf4ca` plus the
seven additional cases in `goose_compiler_generated_assembly`. No production
code changed. The wrapper (`b41aeccc`), assembly (`d7442b06`), deferred map
(`988e557f`) and callable publication (`c8c8b4b7`) changes cover the verified
target execution gaps. **No further execution-support child is justified.**

Fresh actual-compiler probes reproduce the original generation and inspect all
741 emitted declaration factories, including their nested/dead functions:
**741 exports, zero rejected opcodes**. The 37-opcode and 19-form-operation
unions are unchanged from the original inventory. The sorted factory hash
manifest remains `0de90e6c1a6ada95c28c54780010d54d167bd208b9a66c86781b70edec2a937f`;
the original macro emission has not been replaced to make it execute. The
original first-worker probe now reports `Ok("success")` for both V8 and native
Goose on unchanged `workload.jokoum`, including the former line-7 failure.

The [baseline archive](generated-200k-baseline.edn.gz) was independently reread:
all 97 embedded file digests match; 391 of its 392 instrument-phase source
hashes match this tree, with only the extended compiler test differing.
The fresh full generated manifest matches the pinned manifest byte-for-byte.
Every qualified source-entry count matches V8 at Goose 1/2/4/8. Joining those
615 identities to the freshly generated `model.edn` gives:

| Family / `workload.fonam` representative | Full-build source entries | Bounded actual-compiler coverage |
| --- | ---: | --- |
| blend / kalvas | 835 | Destructured vector, nested syntax quote, separate equal-number occurrences |
| twice / dukdos | 425 | Auto-symbols, local binding, metadata |
| chain / graidamha | 1,211 | Empty/nonempty rest and returned recursive call; the full-build count includes recursive entries |
| argc / gaikmenfeik | 392 | Original `&form`, zero/one/several arguments |
| or-local / joktol | 387 | `&env :locals`, present false/nil versus absent, original symbol/metadata |
| sum-all / buvaiskum | 424 | Empty/one/several rest elements, deferred callback, nested forms and metadata |
| doubled / gakzoultrei-grekvavus | 412 | Vector destructuring and body splicing |
| defcalc / zalstok | 529 | Real demanded callable, zero/one/several body forms and nested quote |
| defproto / stipopi | 286 | Returned protocol form and method signature |
| defshape / marore-jaipla | 215 | Returned record form and fields |
| defdispatch / dougri | 110 | Returned multimethod form and auto-symbols |
| **Total** | **5,226** | **All eleven reached families** |

These are dynamic source entries, not the older 4,314 static sites. The
57 helper identities and 672 exports remain distinct from 529 Goose helper
callbacks: a callback requests code, not a fresh producer. The separate
concurrent-helper regression asserts one producer for eight consumers and
one-worker progress; the bounded callable regression asserts two producers
across nine demands, preserving aliases and mutations without replay.

### Coverage and ownership

| Layer | Concrete implementation / evidence | Finite boundary |
| --- | --- | --- |
| Lowering/export | `export.rs::supported` and `export_source`; fresh 741-factory probe | Rust frontend parses/lowers only; nested unsupported operations still reject |
| VM/wrappers | `source_vm.goose`; `goose_compiler_variadic_wrappers`, `arguments_across_helper_suspension`, `wrapper_conventions_and_boundaries` | Actual arguments, rest arrays, receiver, captures, numeric/control operations; no general JS prototype model |
| Native leaves/collections | `source_vm.goose::form_concat`, `form_contains`, `form_sequence`, `form_op`, `native_value`; `assembly_leaf_boundaries` | Finite forms/cursors and bounded temporary arrays; invalid lengths/types/arity fail explicitly |
| Deferred closures | `source_vm.goose` mapSeq continuation/memo; `mapseq_demand_and_throw`, `mapseq_helper_no_replay` | One finite rest-list source, full demand; partial/chunked/multiple-source lazy protocols are not reached |
| Callable helper/interop | `callable.goose::encode`/`install`, `compiler_session.goose`; generated zalstok and `callable_nested_demand_aliases_and_no_replay` | Captured canonical `rt.core.list`, `rt.symbol`, `.call`; immutable code/captures relocate, mutable publication rejects |
| Strings and forms | `compiler_bridge.rs`, `form_bridge.goose`, `snapshot.goose`; `generated_assembly`, `goose_compiler_pipeline` | Exact UTF-16, ordered children, scalar pools, metadata and provenance; generated symbols/keywords are forms, not arbitrary JS interop |
| Nested expansion/effects | `compilation_session.rs::compile_unit_in`, `prepare_goose`; `goose_compiler_concurrent_helper_once`, pipeline `reentrant` | Compiler resolves/compiles nested jobs; suspended Goose capacity is released; producer authority is outside snapshots |
| Explicit failures | `worker_failure_and_owned_diagnostic`, `helper_failure_keeps_instruction_context`, shared-write/root-bypass rejection tests | Native function/pc/emitted span plus macro call/definition context; tests separately show V8 accepts rejected Goose jobs |

The test names without prefixes above are `goose_compiler_*` in
`compiler_tests.rs`; concurrent-helper coverage is in `src/goose_evaluator.rs`,
and the actual compiler pipeline/complete comparator is in `capture.rs` and
`benchmark.rs`. Fresh `compiler.bb` passes **35 tests in native debug and
35 optimized**, plus readiness checks in both modes. The expanded assembly
test compares 26 calls covering all families at V8 and Goose 1/2/4/8 **after
host/session teardown**. Two additional repeated-number cases retain exact
Goose spans and the documented V8 value-based last-occurrence limitation;
they do not normalize away that discrepancy.

The execution-boundary audit follows `MacroHost::use_goose_source_macros` to
`compiler_bootstrap.rs::expand_source_javascript`: a present Goose evaluator
takes precedence, and all preparation/call errors propagate. `GooseSession`
exports source through the Rust frontend and sends it to the native process;
`GooseEvaluator::call` has no reference-engine retry. The helper callback's
`compile_unit_in` returns emitted JavaScript, including already-prepared code,
never a precomputed user result. Canonical helper captures are inspected by
the generated assembly test before the session directory is removed.

`expand_core_javascript` deliberately filters out the source-only evaluator;
`build_in` compiles/installs core and sequence bootstrap units in V8. This is
the declared retained compiler service, charged to every fresh baseline build.
The archived target trace has no later core-JavaScript expansion entries;
Rust handles its reached core shapes. Protocol/record/multimethod forms returned
by user macros are subsequently analyzed by light-cljs; application runtime
code executes in Node. Their application operations are not missing Goose
macro-VM requirements. Cached Goose installation rejects explicitly. Thus the
claim remains **Goose source-macro execution with V8 core/bootstrap**.

### Reproduce and limits

Run `compiler.bb` with the private target/GOOSE_HOME and cached-V8 environment
in the recipe below. For the fresh emitted inventory, use its temporary
blocks and run `generated_745_inventory`, then
`generated_745_complete_frontend_inventory` and `generated_745_real_worker`
with an optimized native runner. The historical test names are retained for
reproducibility. Check 741 `OK` rows and an empty final rejected-opcode set in
every `complete.tsv` row. Restore all temporary hooks afterward. The
first-worker probe prints outcomes; require both success lines, not merely a
successful test-process exit.

For the archived count reconciliation, decompress the EDN, verify each
`:files` text against its matching `:sha256` digest, then read
`macro-instrument/{v81,goose1,goose2,goose4,goose8}-0.macros.tsv`. Select
`expanded/workload.*` keys, compare their call-count maps exactly, and join
the suffix `[namespace name]` to `target/generated-200k/model.edn :macros`.
No timing or trace normalization is needed.

Full 200k builds are reused from the reviewed baseline, not rerun or newly
claimed here. Application byte equality does not prove every expansion's
provenance. Full-target form verification, scale/retention/failure budgets,
and clean five-round Goose measurements remain separate acceptance work.
The original and current source chats were retrieved by exact ID; original
message turns were paginated to their end, current milestone/authorization
turns read. This is not the final conversation audit.

Changed Rust formatting and `git diff --check` pass. Strict Clippy retains the
unchanged engine diagnostics documented in the baseline; unrelated root format,
playground and full-suite limitations remain. No broad-suite success is claimed.

## Repeat in an isolated checkout

Generate `target/generated-200k` using the exact command in the target. Install
these two Rust blocks and the exporter observation hook with Babashka:

```sh
bb -e '
(require (quote [babashka.fs :as fs]) (quote [clojure.string :as s]))
(let [dir "crates/big-boy-pants-js/"
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
goose build crates/big-boy-pants-js/src/compiler_session.goose --debug -o target/745-runner-debug
goose build crates/big-boy-pants-js/src/compiler_session.goose -o target/745-runner-native
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
(let [dir "crates/big-boy-pants-js/"]
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

## Full-session safety and lifetime recheck

November 4, 2026, runtime source `730a4c70afc18b5505746826df56621e9d9e345e`.
The frozen 200k project completes with the existing limits. No production budget,
admission rule, or retention policy changed, and no additional safety fix is
justified by these probes. The new regression is
`goose_compiler_large_continuations_reclaim_across_batches`.

The existing comparison command ran one warmup and one diagnostic full build
at V8 one slot and Goose 1/2/4/8. Every run matched the complete 3,449,022-byte
V8 output, all five oracle answers, the strict waste check and every qualified
source-macro entry count. This is safety evidence, not a clean timing matrix.
Counter-only temporary native hooks below observe the full session, avoiding
the existing 4,096-row protocol prefix. The original native source was restored
before compiler runs; the only retained source change is the regression.

### Measured bounds

All eight Goose sessions had 5,226 macro jobs, 5,812 dispatches and resets,
615 prepared macros, **57 actual helper publications**, 529 suspensions,
and zero final waiters. Thus the 529 compiler callbacks are not 529 producers.
Each session copied 843,816,693 continuation bytes cumulatively; the largest
continuation was 3,084,085 bytes, including VM frames/locals/operands/writes/
handlers. The largest prepared checkpoint was 42,813 bytes. The base snapshot
was 7,746 bytes. These are logical serialized/table footprints, not allocator
or RSS measurements.

| Goose execution limit / compiler slots | Peak admitted chunks, warmup / repeat | Peak pending new requests, warmup / repeat | Peak charged retention, bytes | Full-session native peak RSS range, bytes | Repeat private table high-water sum, bytes |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 1 / 1 | 1 / 1 | 20,786,974 | 114,606,080–117,637,120 | 5,303,554 |
| 2 | 2 / 2 | 2 / 2 | 20,708,571 | 156,286,976–161,447,936 | 10,568,762 |
| 4 | 4 / 4 | 3 / 3 | 20,786,974 | 194,527,232–202,162,176 | 20,941,311 |
| 8 | 8 / 6 | 6 / 5 | 20,786,974 | 273,907,712–280,756,224 | 41,587,049 |

An admitted chunk can be queued, restoring/importing, executing or publishing;
this column does not assert simultaneous VM instructions. Every slot did work.
The compiler's separate DAG trace still has 13,477 deferred units, depth 11,
ready width median 28/p90 33/max 37 and macro-free width max 35. Those units
include analysis without a source-macro call and are not a Goose ready queue.
The single active waiter peak in every run reflects this target's demand pattern,
not the 128-waiter ceiling or serial admission of all source work.

A separate bounded interval probe on Goose8 records the wall-clock region from
immediately before the first `source::run` slice through return from the last
slice of each body chunk. It excludes queue wait, restore, argument/helper
import, output conversion and helper factory execution. Both full builds again
passed complete JS/oracle/waste checks and recorded all **5,755** body intervals
(5,226 initial + 529 resumed). Peak overlap was **4** in the warmup and **3** in
the repeat, not eight. Intervals include OS descheduling; this is overlapping VM
execution phases, not proof of simultaneous CPU instructions. The full target
exposes parallel source execution, but does not establish eight-way body
saturation. No missing admission mechanism was demonstrated. Clean scaling
results must keep configured workers and achieved overlap distinct.

One four-way witness, in the native monotonic clock's nanoseconds, is:

```edn
;; [job slot begin end]; all four cover t=4774183403246000
[[1372 7 4774183403246000 4774183403258000]
 [1371 4 4774183403239000 4774183403263000]
 [1368 2 4774183403240000 4774183403249000]
 [1370 5 4774183403229000 4774183403248000]]
```

At successful close, charged retained state was exactly **17,773,294 bytes**:
17,155,744 bytes of prepared checkpoints plus 617,550 bytes of helper publications.
The largest live charge is 31.0% of 64 MiB; 672 Rust exported identities use
65.6% of the 1,024-definition cap. No consumed continuation remains in this sum.
The 1,024 request guard is a bounded coordinator queue, not a cumulative job cap;
its consumed prefix is cleared once drained. Work has eight entries, command
credits 128, events 32, and the coalesced wake/cancel queues one each. Credit
acquisition precedes command enqueue, so input saturation blocks the reader
instead of dropping work. At most the execution limit is dispatched; priority
resumes and helper production release/reuse capacity at one worker. The existing
poison protocol drains only work actually dequeued by the coordinator and waits
for racing claims, then queues one stop per surviving worker.

Keep the 64 MiB snapshot/coordinator budgets, 16 MiB form packets, VM limits
(100,000 nodes; 1,048,576 values/properties/UTF-16 units; 10 million work;
one million instructions; 256 frames; 65,536 stack entries), 128 active waiters
and 1,024 definitions/requests. Full-target success demonstrates their current
adequacy; this does not promise headroom for larger generated targets. The pinned
compiler configuration already permits 100 billion session polls and a one-hour
deadline. These compiler controls are separate from per-VM/per-packet bounds.

Retention remains compilation scoped: prepared definitions and immutable
publications live until close, inactive continuations are reclaimed after
dispatch, and each private worker clears logical tables while retaining its
allocator high water. All spawned native threads join: 3/5/9/17, with total
native threads 4/6/10/18 = coordinator + input + W execution + W join-only
supervisors. Rust additionally owns the CLI thread, two Goose pipe readers,
unit-pool coordinator/workers, parse-pool workers and V8 platform background
threads. Those roles are not counted in the native total; 450 generated worker
namespaces are not threads. Normal completion removes the session directory and
all request files; the same cleanup follows terminal failures.

Full-session native RSS is materially higher than the baseline's explicitly
prefix-only 14–33 MB observations. Logical retention and private table maxima
exclude temporary relocation/validation buffers, base state, queued copies,
allocator page retention/rounding and other runtime memory; they must not be
advertised as a 64 MiB process ceiling. Queued messages use owned copies; the
installed Goose runtime allocates/copies before queue admission and frees the
message after receive. Snapshot and input/output copies remain real costs.
Process-wide RSS and compiler/native memory must not be added as synchronized
peaks. Keep session teardown as the reclamation boundary; no per-job trimming,
collector or quota increase is warranted for this fixed compilation.

### Stress and failure coverage

The new one-worker test carries a 14,000-element scalar vector through **1,058 helper
suspensions**, twice this project's count, in two batches with one definition
and one producer. Its 3,399,414-byte continuation is slightly larger than the
largest target continuation. It validates every returned count/helper form,
constant live Rust handle count, one preparation/two exports, 2,117 dispatches
and resets, complete native joins, directory removal, and owned forms after
both evaluator and session drop. Separate counters measured 3,596,580,012 bytes
of cumulative continuations, peak charged retention 3,409,759 bytes, final charge
10,345 bytes (10,261 checkpoint + 84 publication), zero waiters and 76,218,368
bytes native peak RSS. This rejects cumulative-snapshot retention as an
explanation of the memory footprint; it is not a general allocator-leak proof.
The finite 100-million-poll/120-second test budget covers repeated transport
visits while remaining below the generated project's poll budget. No production
limit changes. Returning the entire large vector would consume an additional
conversion walk; this fixture tests continuation lifetime with a small result.

The existing 35 compiler tests passed with debug-native and optimized-native
runners; the new large-continuation test passed separately in both modes and
with the counter-only runner. Existing coverage includes:

- Delayed producer/idle wake and eight consumers sharing one producer, with
  one-worker progress; actual reentrant compiler analysis and captured writes
  remain unchanged across suspension.
- Inactive waiter reclamation, explicit rejection of the 129th active waiter,
  deterministic ready-batch drain, and alternating payload/private-state reset.
- Failed preparation/helper publication, malformed transport, contained abort
  before publication, and post-publication abort with queued/in-transit work;
  the latter observes native `CLOSED` before the host's kill fallback.
- Cancellation/deadline during ordinary execution, deferred callbacks and
  published callable invocation; fresh-session recovery after poison.
- Owner-scoped/stale IDs, fresh restore identities, durable metadata/spans/forms
  after teardown, and transactional malformed-snapshot rejection. Smoke tests
  also cover byte/work/graph limits, memo-range exclusivity and immutable aliases.

`smoke.bb` passed. Changed-file formatting and `git diff --check` passed.
Strict Clippy reproduced the same eight engine diagnostics; with those known
categories allowed it exposed only the existing namespace-publication
single-element loop, and with that category allowed the lib/tests audit passed.
Root formatting retains its unrelated baseline differences. No full-suite or
strict-lint success is claimed. The full-form/provenance gate and quiet
performance matrix remain separate acceptance work.

### Reproduce the counter-only probe

Use a disposable checkout, the documented private Cargo/Goose build environment,
and the unchanged generated target. Save this block as `target/safety748.bb`,
run it from the repository root, then run the existing comparison command with
`--runner target/safety748-runner --backend all --phase instrument --runs 1`.
The helper restores `compiler_session.goose` in `finally`; measurements are flat
`target/safety748-W-TIME.edn` files, written only at native close. `retained-peak`
is sampled after each charge, queue peak after command drain before dispatch,
waiter peak on insertion, and running peak after dispatch. RSS is native
`getrusage` high water observed throughout and at close. No admission or
execution behavior is replaced, and no protocol fields are added.

The diagnostic release compiler SHA-256 was
`85fe647503b4d4edf8a1e02d58b65a98dabf762b3075c3c01a69c755425e17e4`;
the counter-runner SHA-256 was
`ef8d949fa04b2279da8f306adcabb388ef1b78f8f3e311dde5bf5a1c8e085ce0`.
Compiler/native source remains the above base; the helper embeds its output
path, so a different checkout need not yield an identical runner binary.
Both source conversations were read by exact ID through their final pages;
this safety check does not replace the final conversation audit.

```clojure
(require '[clojure.string :as s] '[babashka.process :as p])
(def file "crates/big-boy-pants-js/src/compiler_session.goose")
(def original (slurp file))
(defn replace-once [text from to]
  (assert (= 1 (count (re-seq (re-pattern (java.util.regex.Pattern/quote from)) text))) from)
  (s/replace text from to))
(def patched
  (reduce (fn [text [from to]] (replace-once text from to)) original
    [["var pool: i64[>..] = [];" "var peak_retained: i64 = 0; var peak_waiting: i64 = 0; var peak_queue: i64 = 0; var peak_running: i64 = 0; var checkpoint_bytes: i64 = 0; var helper_bytes: i64 = 0; var max_checkpoint: i64 = 0; var max_saved: i64 = 0; var saved_bytes: i64 = 0; var suspensions: i64 = 0; var peak_rss: i64 = 0;\n    var pool: i64[>..] = [];" ]
     ["// Test mode waits for this bounded batch's real events," "peak_queue = max(peak_queue, queue.len - received);\n        // Test mode waits for this bounded batch's real events,"]
     ["retained += bytes; if event.metrics.len" "retained += bytes; peak_retained = max(peak_retained, retained); if event.kind == 1 { suspensions += 1; saved_bytes += bytes; max_saved = max(max_saved, bytes); } else if event.kind == 5 { helper_bytes += bytes; } if event.metrics.len"]
     ["print(\"NEED \", event.id" "peak_waiting = max(peak_waiting, waiting.len); print(\"NEED \", event.id"]
     ["retained += bytes; index = prepared.len;" "retained += bytes; checkpoint_bytes += bytes; max_checkpoint = max(max_checkpoint, bytes); peak_retained = max(peak_retained, retained); index = prepared.len;"]
     ["// Release inactive continuation owners and their budget after dispatch." "peak_running = max(peak_running, running.len); peak_rss = max(peak_rss, peak_memory());\n        // Release inactive continuation owners and their budget after dispatch."]
     ["assert(thread_wait(reader)); if tracing" (str "assert(write_file(str(\"" (System/getProperty "user.dir") "/target/safety748-\", limit, \"-\", clock_ns(), \".edn\"), str(\"{:workers \", limit, \" :prepared \", prepared.len, \" :producers \", produced.len, \" :base-bytes \", snapshot::footprint(base), \" :checkpoint-bytes \", checkpoint_bytes, \" :helper-bytes \", helper_bytes, \" :max-checkpoint \", max_checkpoint, \" :retained-final \", retained, \" :retained-peak \", peak_retained, \" :waiters-final \", waiting.len, \" :waiters-peak \", peak_waiting, \" :queue-peak \", peak_queue, \" :running-peak \", peak_running, \" :suspensions \", suspensions, \" :saved-bytes-total \", saved_bytes, \" :max-saved \", max_saved, \" :native-rss \", max(peak_rss, peak_memory()), \"}\\n\")));\n    assert(thread_wait(reader)); if tracing")]]))
(try
  (spit file patched)
  (p/shell "goose" "check" file)
  (p/shell "goose" "build" file "-o" "target/safety748-runner")
  (finally (spit file original)))
```

For the separate execution-overlap observation, save the following helper as
`target/overlap748.bb` and run it from the root. With the V8 reference above
present, use the same comparison command with `--backend goose --workers 8
--runner target/overlap748-runner --phase check --runs 1`. Both runs remain
diagnostic despite the harness phase name. Each worker records bounded private
flat spans, written only at close. The instrumentation adds clock reads and
private records, without new protocol events, locks, VM programs or admission
rules. The runner hash was
`e09b968ec9d300fec039e72c17dc70d8f9cc0359fd6a2ffbc848b404fc511679`.

```clojure
(require '[clojure.string :as s] '[babashka.process :as p])
(def file "crates/big-boy-pants-js/src/compiler_session.goose")
(def original (slurp file))
(defn replace-once [text from to]
  (assert (= 1 (count (re-seq (re-pattern (java.util.regex.Pattern/quote from)) text))) from)
  (s/replace text from to))
(def patched
  (reduce (fn [text [from to]] (replace-once text from to)) original
    [["var c = Capacity {}; var count:" "var spans: i64[>..] = []; var span: i64[2] = [0; 2]; var c = Capacity {}; var count:"]
     ["graph, bytes);\n        // Publish" "graph, bytes, span);\n        if span[0] > 0 && span[1] > span[0] { assert(spans.len < 8192 * 4); spans.append([work.id, work.mode, span[0], span[1]]); } span = [0; 2];\n        // Publish"]
     ["graph: bridge::Graph&, bytes: u8[>..]&) {" "graph: bridge::Graph&, bytes: u8[>..]&, span: i64[2]&) {"]
     ["let execution_started = clock_ns();" "let execution_started = clock_ns(); span[0] = execution_started;"]
     ["let outcome = source::run(job, 10000); if outcome == tiny::Outcome.Complete { break; }" "let outcome = source::run(job, 10000); span[1] = clock_ns(); if outcome == tiny::Outcome.Complete { break; }"]
     ["if tracing { var high: i64[4]" (str "var out: u8[>..] = []; format(out, \"[\"); for n in spans { format(out, n, \" \"); } format(out, \"]\\n\"); assert(write_file(str(\"" (System/getProperty "user.dir") "/target/overlap748-\", slot, \"-\", clock_ns(), \".edn\"), out));\n    if tracing { var high: i64[4]")]]))
(try
  (spit file patched)
  (p/shell "goose" "check" file)
  (p/shell "goose" "build" file "-o" "target/overlap748-runner")
  (finally (spit file original)))
```

Each file holds flat `[job mode begin end ...]` records. Group the eight files
from each completed session (their timestamps precede the next session), then
sort starts as +1 and ends as -1 by timestamp, with ends first on ties. The
maximum cumulative sum is the overlap. Require exactly 5,755 spans, modes
`{0 5226, 1 529}`, and a final sum of zero. Keep this instrumentation out of
clean timing binaries. Remove both temporary helpers, their runners and
measurement files after preserving any needed local evidence.

## Complete-project forms and teardown verification

November 4, 2026, on `b74740caa2928dfb57d6d452418ccbd8acf9613c` plus
this test-only verification change. No production Rust/Goose runtime, generator,
workload, budget or macro declaration changed. Fresh release/advanced CLI builds
at V8 one slot and Goose 1/2/4/8, one warmup and one check per cell, all matched
**3,449,022 bytes**, SHA-256
`18335556dd4672f2f63b901b68fcad3c447cf5676f457a08fda61f366cc9f3ec`,
all five frozen oracle outputs and strict waste checks. No normalization was used.
The comparison command checked the complete frozen source/shim/configuration
manifest before and after every build. This is correctness work, not the quiet
five-round performance matrix.

The new explicitly ignored `generated_200k_complete_forms` test analyzes the
entire unmodified `workload.main` through the actual namespace compiler. A
test-only cross-thread hook retains every generated source-macro call/result.
Each of V8 and Goose 1/2/4/8 records **5,226 expansions across 615 qualified
identities**, with identical per-identity count maps. Each Goose session has
5,226 jobs, 615 preparations and 672 exports. All jobs, programs and macro hosts
are destroyed and the native session closes **before** comparison. The test
requires native thread accounting 4/6/10/18 total with 3/5/9/17 spawned threads
joined, no process, no reader handles, no pending mailboxes and no session files.
The complete audit passes in both optimized-native and `--debug` native Goose
at all four worker limits, using the same optimized Rust test executable.
No Goose full-project debug budget was exceeded. An exploratory unoptimized
Rust run was stopped after roughly three minutes in V8 reference analysis,
before any Goose result; it is not counted as a pass.

Goose 1/2/4/8 results match one another strictly, including complete call/result
arenas, roots, ordered child IDs, scalar datum/UTF-16 pools, scalar flavours,
metadata, compiler payloads, source names/text/spans, node origins and expansion
history. The complete-form comparator now also checks every expansion's macro
name, call, definition span and ordered helper identities. Negative controls
reject changed integer/double flavour, metadata, definition provenance and
helper provenance. Source identity numbers remain process-local; the existing
comparator compares source names and complete UTF-8 text.

### Reference ambiguities and independent checks

The V8 comparison has exactly **82 span differences per Goose cell**, all
explicitly checked. Its scalar pools, types, values, structure, metadata, node
origins and expansion history otherwise match exactly. These differences are
reported individually by the test; they are not erased from Goose forms:

- **80 anonymous-argument aliases**, including inherited nodes in a later call's
  larger borrowed arena. The reader can reuse one Datum for a `#()` parameter
  and its body placeholders. V8's `retain` table keeps the last Form for that
  identity. The audit requires the exact original identity alias or independently
  parses the immutable source's anonymous function and last matching `%`, `%1`,
  `%2`, etc. No arbitrary symbol-span difference is permitted.
- **Two equal-integer occurrences** affected by V8's documented latest-by-value
  scalar provenance. The pinned exceptions are only the two span pairs below.
  Independent assertions follow the original macro declarations' output
  positions and check all five argument occurrences, including each equal value.

| Canonical source / call span | Original macro behavior | Goose occurrence / V8 occurrence |
| --- | --- | --- |
| `workload/pleistum.cljs`, `6865..6909` | `dreiplofail`: the initializer and body in `(… [gubrar 9] 9)` remain separate | `6904..6905` / `6907..6908` |
| `workload/storhouher.cljs`, `18455..18625` | `pleissilnim`: each `sum-all` argument remains in its own `(* 19 argument)` term | `18579..18582` / `18621..18624` |

The anonymous-parameter audit independently checks **43 reader-precise input
parameters**, **22 already-collapsed compiler-input parameters**, and **65
returned parameters** in each Goose cell. These count occurrences across macro
requests, not distinct functions. It handles numbered placeholders, unused
positional parameters and rest placeholders. Rust's pre-existing pure expansion
intentionally retains the last occurrence of a shared Datum, pinned by
`compiler_data::tests::pure_expansion_preserves_last_occurrence_within_its_input_subtree`.
An inherited collapse is accepted only if it already occurs identically in V8's
input and follows the source's last-placeholder rule. Every returned Goose
binder and ordered body occurrence must exactly preserve **its actual job
input**. A negative control proves that a new Goose collapse fails even if V8
would agree. The independent rooted check covers the graph sent to Goose;
complete arena comparisons also retain earlier unreachable compiler nodes.
The actual generated-assembly regression additionally pins parameter/body spans
`20..28` and `24..25` for `(zalstok sample [x] #(+ % 1))`, while recording V8's
`24..25`/`24..25` observation. The earlier equal-number representative checks
remain unchanged.

### Reproduce and evidence reuse

From a fresh isolated checkout, set the private target/Goose home, build-job cap
and required V8 cache as in the baseline recipe, then run:

```sh
cargo build --release --locked --features goose-macros --bin light-cljs
bb benchmarks/generated-project/generate.bb target/generated-200k --seed 42 --lines 200k
goose build crates/big-boy-pants-js/src/compiler_session.goose -o target/generated-200k-runner
bb benchmarks/generated-project/compare.bb target/generated-200k --light target/release/light-cljs --backend all --runner target/generated-200k-runner --phase check --runs 1
GOOSE_COMPILER_RUNNER="$PWD/target/generated-200k-runner" cargo test --release --locked --features goose-macros --lib generated_200k_complete_forms -- --include-ignored --nocapture --test-threads=1
goose build crates/big-boy-pants-js/src/compiler_session.goose --debug -o target/generated-200k-debug-runner
GOOSE_COMPILER_RUNNER="$PWD/target/generated-200k-debug-runner" cargo test --release --locked --features goose-macros --lib generated_200k_complete_forms -- --include-ignored --nocapture --test-threads=1
bb crates/big-boy-pants-js/scripts/compiler.bb
cargo test --locked --features goose-macros --lib complete_form_comparison -- --test-threads=1
cargo test --locked --features goose-macros --lib generated_reference_ambiguity_requires_original_alias -- --test-threads=1
cargo test --locked --features goose-macros --lib pure_expansion_preserves_last_occurrence_within_its_input_subtree -- --test-threads=1
GOOSE_EXPANDED_FOCUS=heavier GOOSE_EXPANDED_SAMPLES="$PWD/target/expanded-check-native.edn" bb crates/big-boy-pants-js/scripts/benchmark.bb --expanded-check
GOOSE_EXPANDED_FOCUS=heavier GOOSE_BENCH_DEBUG=1 GOOSE_EXPANDED_SAMPLES="$PWD/target/expanded-check-debug.edn" bb crates/big-boy-pants-js/scripts/benchmark.bb --expanded-check
```

Current optimized native runner hash is unchanged from the baseline:
`92140b1a31ba1fe42beadc3caae59fb6ea0916e4a3204e5caff10ef7146688e0`.
The current release CLI hash is
`b39a3644a301108844ce516eea56319cc0d675f528e64b9f6a043aac76609515`.
Toolchains remain Goose `v2026.3-267-g369c7a2c` / runtime `3025e5810379`,
Rust 1.99.1, Node 26.12.1 and Babashka 1.12.218. The research checkout remains
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`.

All 97 baseline archive file digests were reverified. Of 392 archived execution
input hashes, 389 still match; only the three test/capture/comparator files
changed. This justifies reusing the reviewed 741-factory identity/no-fallback
boundary audit and full-session safety counters above. The unchanged 128-waiter,
1,024-definition/request and 64 MiB retention quotas remain sufficient; the
measured ~20.8 MB peak charge and up-to-281 MB native RSS are different scopes.
No quota was raised, and no eight-way CPU-saturation claim is made.

Fresh focused compiler coverage is 36 tests per native mode, plus the extended
generated-assembly test in both modes. It includes repeated fresh sessions,
owned handles/forms, helper progress/no replay, malformed packets, bridge limits,
cancellation/deadlines, poison/queued cleanup and large-continuation reclamation.
The heavier seed-24301/15-namespace/120-call/8,192-read control passes V8 and
Goose 1/2/4/8 with both optimized and debug runners, including its complete output
and independent oracle. Unchanged VM/snapshot/45-row/QuickJS layers are reused;
this change does not modify those execution paths.

Changed Rust formatting and `git diff --check` pass. Fresh strict Clippy stops on
the same eight engine diagnostics; allowing those known categories exposes only
the existing namespace-publication single-element loop, and allowing that
category passes the lib/tests audit. Root formatting reports 24 unchanged files.
The known broad CLI playground/lint failures and lack of a current successful
full-suite run remain limitations, not passes. The boundary remains **Goose
source-macro execution with retained V8 core/bootstrap**, as charged and described
in the baseline; cached Goose installation remains explicitly unsupported.
