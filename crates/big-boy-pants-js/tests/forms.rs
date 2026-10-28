//! Native Clojure form values shared by tasks (light-cljs-466,
//! `crate::collections::form`), through the public embedding API and
//! JavaScript source using the private `__rt.form*` intrinsics. Programs are
//! inline synthetic snippets, not light-cljs output; the hash reference in
//! `hashes_match_the_light_cljs_runtime` copies the light-cljs runtime's
//! hash functions (`src/js_runtime.js` in the light-cljs repository).

use std::collections::HashMap;
use std::thread;

use big_boy_pants_js::frontend::compile_function;
use big_boy_pants_js::{
    CollectionError, Completion, DependencyId, DependencyKey, FormKind, JsString, KeyName,
    PathStep, Primitive, Provenance, PublishError, ResultKind, Rooted, RunOutcome, Runtime,
    ValueView, VmError, VmErrorKind,
};

fn js(s: &str) -> JsString {
    JsString::from(s)
}

fn complete(outcome: RunOutcome) -> Rooted {
    match outcome {
        RunOutcome::Complete(v) => v,
        other => panic!("expected Complete, got {other:?}"),
    }
}

/// Run the JavaScript function declaration `src` with `args` to its end.
fn call(rt: &Runtime, src: &str, args: &[&Rooted]) -> Rooted {
    let code = compile_function(src).unwrap_or_else(|e| panic!("{src}: {e}"));
    complete(rt.spawn(&code, args).unwrap().run().unwrap())
}

fn failure(rt: &Runtime, src: &str) -> VmError {
    let code = compile_function(src).unwrap_or_else(|e| panic!("{src}: {e}"));
    match rt.spawn(&code, &[]).unwrap().run().unwrap() {
        RunOutcome::Failed(e) => e,
        other => panic!("{src}: expected Failed, got {other:?}"),
    }
}

#[test]
fn printer_traversal_uses_legacy_hash_paths_and_preserves_key_identity() {
    let rt = Runtime::new();
    let result = call(
        &rt,
        r#"function f(){
      const k=__rt.formVector(1), sameHash=__rt.formList(1);
      const map=__rt.formMap(33,'a',1,'b',32,'c',0,'d',null,'nil');
      const entries=__rt.formPrintItems(map), keys=[];
      for(let i=0;i<__rt.formCount(entries);i++)keys.push(__rt.formNth(__rt.formNth(entries,i),0));
      if(keys[0]!==null||keys[1]!==0||keys[2]!==32||keys[3]!==1||keys[4]!==33)throw new Error('map traversal');
      let set=__rt.formSet(33,1,32,0,null,100,101,102,103);
      for(let i=100;i<104;i++)set=__rt.formDisj(set,i);
      const members=__rt.formPrintItems(set);
      for(let i=0;i<5;i++)if(__rt.formNth(members,i)!==keys[i])throw new Error('set traversal');
      const collision=__rt.formPrintItems(__rt.formMap(k,1,sameHash,2));
      if(__rt.formCount(collision)!==1||__rt.formNth(__rt.formNth(collision,0),0)!==k)throw new Error('retained collision key');
      const positional=__rt.formLiteralMap(__rt.formVector(33,'a',1,'b',k,1,sameHash,2));
      const items=__rt.formPrintItems(positional);
      if(__rt.formCount(items)!==4||__rt.formNth(__rt.formNth(items,0),0)!==33||__rt.formNth(__rt.formNth(items,2),0)!==k)throw new Error('positional key identity');
      const small=__rt.formPrintItems(__rt.formSet(33,1,32,0,null));
      if(__rt.formNth(small,0)!==33||__rt.formNth(small,4)!==null)throw new Error('small set order');
      const array=__rt.formArrayMap(__rt.formVector(k,1,sameHash,2,33,3));
      const pairs=__rt.formPrintItems(array);
      if(__rt.formCount(pairs)!==2||__rt.formNth(__rt.formNth(pairs,0),0)!==k||__rt.formNth(__rt.formNth(pairs,0),1)!==2)throw new Error('array map duplicate');
      return true;
    }"#,
        &[],
    );
    assert!(matches!(result.view(), ValueView::Bool(true)));
    assert!(matches!(
        failure(&rt, "function f(){return __rt.formPrintItems(1);}").kind(),
        VmErrorKind::Collection(_)
    ));
}

#[test]
fn default_sorted_maps_keep_order_identity_and_metadata_across_tasks_and_gc() {
    let rt = Runtime::new();
    let made = call(
        &rt,
        r#"function make(){
        const key=__rt.formWithMeta(__rt.formVector(2),__rt.formMap('key',true));
        const value=__rt.formVector('kept'), meta=__rt.formMap('map',true);
        let m=__rt.formSortedMap(__rt.formVector(key,value,__rt.formVector(1),'first'));
        m=__rt.formWithMeta(m,meta);
        const same=__rt.formAssoc(m,__rt.formVector(2),__rt.formVector('kept'));
        if(same!==m)throw new Error('compare-equal persistent update');
        m=__rt.formAssoc(m,__rt.formVector(3),'last');
        if(__rt.cljIsEditable(m))throw new Error('sorted map is editable');
        if(__rt.formGet(m,__rt.formVector(2),null)!==value)throw new Error('vector lookup');
        const keys=__rt.formKeys(m);
        if(__rt.formNth(keys,1)!==key)throw new Error('retained key identity');
        if(__rt.formMeta(m)!==meta)throw new Error('metadata');
        return __rt.formVector(m,key,value,meta);
    }"#,
        &[],
    );
    let (map, weak) = rt.scope(|s| {
        let v = s.local(&made).unwrap();
        (
            s.persist(s.form_nth(v, 0).unwrap()),
            [1, 2, 3].map(|i| s.downgrade(s.form_nth(v, i).unwrap())),
        )
    });
    rt.publish(&map).unwrap();
    drop(made);
    rt.collect().unwrap();
    assert!(weak.iter().all(|w| w.is_live()));
    let result = call(
        &rt,
        r#"function read(m){
        let n=__rt.formSortedMap(__rt.formVector());
        for(let i=15;i>=0;i--)n=__rt.cljAssoc(n,i,i);
        n=__rt.cljDissoc(n,3);
        const entries=__rt.formPrintItems(n);
        if(__rt.formNth(__rt.formNth(entries,0),0)!==0||__rt.formNth(__rt.formNth(entries,14),0)!==15)throw new Error('wide sorted order');
        if(__rt.cljIsEditable(n)||__rt.formCount(n)!==15)throw new Error('promotion');
        const d=__rt.cljDissoc(m,__rt.formVector(1));
        const first=__rt.cljFirst(__rt.cljSeq(d));
        if(__rt.formNth(__rt.formNth(first,0),0)!==2||__rt.formMeta(d)!==__rt.formMeta(m))throw new Error('sorted removal');
        const plain=__rt.formMap(__rt.formVector(1),'first',__rt.formVector(2),__rt.formVector('kept'),__rt.formVector(3),'last');
        if(!__rt.formEquals(m,plain)||!__rt.formEquals(plain,m)||__rt.formHash(m)!==__rt.formHash(plain))throw new Error('map semantics');
        return true;
    }"#,
        &[&map],
    );
    assert_eq!(result.view(), ValueView::Bool(true));
    drop(map);
    rt.collect().unwrap();
    assert!(weak.iter().all(|w| !w.is_live()));
}

#[test]
fn default_sorted_maps_preserve_semantics_when_realized() {
    let rt = Runtime::new();
    let result = call(
        &rt,
        r#"function f(){
        const key=__rt.formWithMeta(__rt.formVector(2),__rt.formMap('key',true));
        const lazy=__rt.lazySeq(()=>__rt.formList('a','b'));
        const lazyMeta=__rt.lazySeq(()=>__rt.formList('meta'));
        const meta=__rt.formMap('tag',lazyMeta);
        const original=__rt.formWithMeta(__rt.formSortedMap(__rt.formVector(key,lazy,__rt.formVector(1),'first')),meta);
        const m=__rt.cljRealizeAll(original);
        if(m===original||__rt.formMeta(m)===meta)throw new Error('map and metadata must rebuild');
        if(!__rt.cljRealized(lazy)||!__rt.cljRealized(lazyMeta))throw new Error('lazy children unrealized');
        if(__rt.cljIsEditable(m))throw new Error('rebuilt sorted map is editable');
        const entries=__rt.formPrintItems(m);
        if(__rt.formNth(__rt.formNth(entries,0),1)!=='first'||__rt.formNth(__rt.formNth(entries,1),0)!==key)throw new Error('order and key identity');
        const v=__rt.cljGet(m,__rt.formVector(2),null), tag=__rt.cljGet(__rt.formMeta(m),'tag',null);
        if(__rt.formKind(v)!=='list'||__rt.formCount(v)!==2||__rt.formNth(v,1)!=='b'||__rt.formKind(tag)!=='list'||__rt.formNth(tag,0)!=='meta')throw new Error('realized contents');
        const updated=__rt.cljAssoc(m,__rt.formVector(0),'new');
        const removed=__rt.cljDissoc(updated,__rt.formVector(1));
        const keys=__rt.formKeys(removed);
        if(__rt.formNth(__rt.formNth(keys,0),0)!==0||__rt.formNth(keys,1)!==key||__rt.cljIsEditable(removed))throw new Error('sorted updates');
        if(__rt.formMeta(removed)!==__rt.formMeta(m)||__rt.formCount(m)!==2||__rt.formCount(original)!==2)throw new Error('persistent metadata');
        return true;
    }"#,
        &[],
    );
    assert_eq!(result.view(), ValueView::Bool(true));
}

fn at(line: u32, column: u32) -> Provenance {
    Provenance {
        source: 1,
        line,
        column,
    }
}

/// Rust builds `(defthing ^{:doc "d"} [x] {[:k 1] x, {:a [1 2]} "y"})`, with
/// a vector and a map as keys, and two JavaScript tasks, running at once on
/// two threads, read it (looking keys up with separately allocated equal
/// values, a list for the vector key) and return the form, its parts and
/// the stored key objects by identity.
#[test]
fn rust_forms_are_read_and_returned_by_identity_by_two_tasks() {
    let rt = Runtime::new();
    let parts = rt.scope(|s| {
        let kw = |n: &str| s.new_keyword(None, js(n)).unwrap();
        let num = |n: f64| s.primitive(Primitive::Number(n));
        let head = s.new_symbol(None, js("defthing")).unwrap();
        let x = s.new_symbol(None, js("x")).unwrap();
        let doc = s.primitive(Primitive::String(js("d")));
        let meta = s.new_form_map(&[(kw("doc"), doc)]).unwrap();
        let params = s.new_form_vector(&[x]).unwrap();
        let params = s.form_with_meta(params, Some(meta)).unwrap();
        let x_value = s.new_symbol(None, js("x")).unwrap();
        let vec_key = s.new_form_vector(&[kw("k"), num(1.0)]).unwrap();
        let pair = s.new_form_vector(&[num(1.0), num(2.0)]).unwrap();
        let map_key = s.new_form_map(&[(kw("a"), pair)]).unwrap();
        let y = s.primitive(Primitive::String(js("y")));
        let body = s.new_form_map(&[(vec_key, x_value), (map_key, y)]).unwrap();
        let form = s.new_list(&[head, params, body]).unwrap();
        [form, params, x_value, vec_key, map_key, meta].map(|l| s.persist(l))
    });
    let form = &parts[0];
    rt.publish(form).unwrap();
    let src = "function read(form) {
        const params = __rt.formNth(form, 1), body = __rt.formNth(form, 2);
        if (__rt.formKind(form) !== 'list' || __rt.formCount(body) !== 2) throw new Error('shape');
        const x = __rt.formGet(body, __rt.formList(__rt.formKeyword(null, 'k'), 1), null);
        const y = __rt.formGet(body, __rt.formMap(__rt.formKeyword(null, 'a'), __rt.formList(1, 2)), null);
        if (y !== 'y' || __rt.formName(x) !== 'x' || __rt.formNamespace(x) !== null) throw new Error('lookup');
        const keys = __rt.formKeys(body);
        let vecKey = null, mapKey = null;
        for (let i = 0; i < __rt.formCount(keys); i++) {
            const k = __rt.formNth(keys, i);
            if (__rt.formKind(k) === 'vector') vecKey = k; else mapKey = k;
        }
        return __rt.formVector(form, params, x, vecKey, mapKey, __rt.formMeta(params));
    }";
    let code = compile_function(src).unwrap();
    let results: Vec<Rooted> = thread::scope(|sc| {
        let running: Vec<_> = (0..2)
            .map(|_| {
                let task = rt.spawn(&code, &[form]).unwrap();
                sc.spawn(move || complete(task.run().unwrap()))
            })
            .collect();
        running.into_iter().map(|t| t.join().unwrap()).collect()
    });
    for result in &results {
        rt.scope(|s| {
            let items = s.form_items(s.local(result).unwrap()).unwrap();
            assert_eq!(items.len(), parts.len());
            for (i, (got, want)) in items.iter().zip(&parts).enumerate() {
                assert!(s.same_object(*got, s.local(want).unwrap()), "item {i}");
            }
        });
    }
    // Neither task changed what Rust holds.
    rt.scope(|s| {
        let form = s.local(form).unwrap();
        assert_eq!(s.form_count(form), Ok(3));
        assert_eq!(s.form_kind(form), Some(FormKind::List));
    });
}

/// Separately allocated equal keys retrieve the same entry; keyword, symbol
/// and string spellings stay distinct; lists and vectors are sequentially
/// equal; updates keep identity rules. All from JavaScript.
#[test]
fn equal_separately_allocated_keys_retrieve_the_same_entry() {
    let rt = Runtime::new();
    let src = r#"function check() {
        const kw = (ns, n) => __rt.formKeyword(ns, n), sym = (ns, n) => __rt.formSymbol(ns, n);
        const vecKey = __rt.formVector(kw(null, 'a'), 1);
        const m = __rt.formMap(vecKey, 'v1',
            __rt.formMap(kw(null, 'x'), __rt.formList(1)), 'v2',
            sym('my.ns', 's'), 'v3',
            1, 'one',
            'str', 'v5',
            null, 'nil');
        const withLine = __rt.formWithMeta(sym('my.ns', 's'), __rt.formMap(kw(null, 'line'), 3));
        const nan = __rt.formList(NaN);
        const checks = [
            __rt.formEquals(kw(null, 'a'), kw(null, 'a')) && kw(null, 'a') !== kw(null, 'a'),
            !__rt.formEquals(kw(null, 'a'), sym(null, 'a')),
            !__rt.formEquals(kw('ns', 'a'), kw(null, 'a')),
            !__rt.formEquals(kw('', 'a'), kw(null, 'a')),
            !__rt.formEquals(kw(null, 'a'), 'a'),
            __rt.formEquals(__rt.formList(1, 2), __rt.formVector(1, 2)),
            __rt.formHash(__rt.formList(1, 2)) === __rt.formHash(__rt.formVector(1, 2)),
            !__rt.formEquals(__rt.formList(1, 2), __rt.formVector(2, 1)),
            __rt.formEquals(__rt.formMap(kw(null, 'a'), 1, kw(null, 'b'), 2), __rt.formMap(kw(null, 'b'), 2, kw(null, 'a'), 1)),
            !__rt.formEquals(__rt.formMap(kw(null, 'a'), 1), __rt.formMap(kw(null, 'a'), 2)),
            !__rt.formEquals(nan, __rt.formList(NaN)) && __rt.formEquals(nan, nan),
            __rt.formEquals(0, -0) && __rt.formEquals(null, undefined) && !__rt.formEquals(null, false),
            !__rt.formEquals(kw(null, '\ud800'), kw(null, '�')) && __rt.formName(kw(null, '\ud800')) === '\ud800',
            __rt.formGet(m, __rt.formList(kw(null, 'a'), 1), null) === 'v1',
            __rt.formGet(m, __rt.formMap(kw(null, 'x'), __rt.formVector(1)), null) === 'v2',
            __rt.formGet(m, withLine, null) === 'v3',
            __rt.formGet(m, 1.0, null) === 'one' && __rt.formGet(m, '1', 'none') === 'none',
            __rt.formGet(m, undefined, null) === 'nil',
            __rt.formGet(m, kw('my.ns', 's'), 'none') === 'none',
            __rt.formGet(m, sym(null, 's'), 'none') === 'none',
            __rt.formCount(__rt.formAssoc(m, __rt.formList(kw(null, 'a'), 1), 'w')) === 6,
            __rt.formNth(__rt.formKeys(__rt.formAssoc(__rt.formMap(vecKey, 1), __rt.formList(kw(null, 'a'), 1), 2)), 0) === vecKey,
            __rt.formAssoc(m, __rt.formList(kw(null, 'a'), 1), 'v1') === m,
            __rt.formCount(__rt.formDissoc(m, __rt.formList(kw(null, 'a'), 1))) === 5,
            __rt.formDissoc(m, kw(null, 'absent')) === m,
            __rt.formGet(__rt.formAssoc(__rt.formMap(), NaN, 1), NaN, 'none') === 'none',
            __rt.formCount(__rt.formMap(__rt.formVector(1), 'a', __rt.formList(1), 'b')) === 1,
            __rt.formGet(__rt.formMap(__rt.formVector(1), 'a', __rt.formList(1), 'b'), __rt.formVector(1), null) === 'b',
            __rt.formEquals(__rt.formConj(__rt.formList(2, 3), 1), __rt.formVector(1, 2, 3)),
            __rt.formEquals(__rt.formConj(__rt.formVector(1, 2), 3), __rt.formList(1, 2, 3)),
            __rt.formKind(__rt.formConj(__rt.formList(), 1)) === 'list',
            __rt.formGet(__rt.formVector(10, 20), 1, null) === 20 && __rt.formGet(__rt.formVector(10), 1.5, 'none') === 'none',
            __rt.formEquals(__rt.formAssoc(__rt.formVector(1, 2), 2, 3), __rt.formVector(1, 2, 3)),
            __rt.formMeta(kw(null, 'a')) === null && __rt.formKind(1) === null && __rt.formValue(1) === 1,
        ];
        return checks.indexOf(false);
    }"#;
    let result = call(&rt, src, &[]);
    assert_eq!(
        result.view(),
        ValueView::Number(-1.0),
        "first failing check"
    );
}

#[test]
fn positional_forms_share_equality_hash_and_first_match_update_contracts() {
    let rt = Runtime::new();
    let result = call(
        &rt,
        r#"function check() {
        const map = xs => __rt.formLiteralMap(xs), set = xs => __rt.formLiteralSet(xs);
        const m = map(__rt.formVector('b', 2, 'a', 1)), h = __rt.formMap('a', 1, 'b', 2);
        const s = set(__rt.formVector(2, 1)), t = __rt.formSet(1, 2);
        if (!__rt.formEquals(m, h) || !__rt.formEquals(h, m) || __rt.formHash(m) !== __rt.formHash(h) ||
            !__rt.formEquals(s, t) || !__rt.formEquals(t, s) || __rt.formHash(s) !== __rt.formHash(t)) throw 'hashed equality';
        // Positional forms also work as structural keys in hashed forms.
        if (__rt.formGet(__rt.formMap(m, s), h, null) !== s ||
            __rt.formGet(__rt.formMap(s, m), t, null) !== m) throw 'structural keys';
        const d = map(__rt.formVector('a', 1, 'a', 2, 'b', 3));
        if (__rt.formCount(d) !== 3 || __rt.formGet(d, 'a', null) !== 1 ||
            __rt.formAssoc(d, 'a', 1) !== d || __rt.formDissoc(d, 'missing') !== d) throw 'first lookup';
        const updated = __rt.formAssoc(d, 'a', 9);
        const entries = __rt.cljSeq(updated);
        if (__rt.formGet(updated, 'a', null) !== 9 || __rt.formNth(__rt.formNth(entries, 1), 1) !== 2) throw 'first assoc';
        // Value mismatch at the first equal key cannot fall through to the second.
        if (__rt.formEquals(map(__rt.formVector('a', 2, 'a', 1, 'b', 3)), d)) throw 'first equality';
        const removed = __rt.formDissoc(d, 'a');
        if (__rt.formCount(removed) !== 2 || !__rt.cljContains(removed, null) ||
            __rt.formGet(removed, 'b', null) !== 3) throw 'dissoc holes';
        const dup = set(__rt.formVector(1, 1, 2));
        if (__rt.formConj(dup, 1) !== dup || __rt.formDisj(dup, 7) !== dup ||
            __rt.formCount(__rt.formConj(dup, 3)) !== 4 ||
            __rt.formCount(__rt.formDisj(dup, 1)) !== 2) throw 'set updates';
        const firstMember = __rt.formWithMeta(__rt.formVector('a'), __rt.formMap('tag', 'first'));
        const lastMember = __rt.formWithMeta(__rt.formList('a'), __rt.formMap('tag', 'last'));
        const promoted = __rt.formConj(set(__rt.formVector(firstMember, lastMember, 2, 3, 4, 5, 6, 7)), 8);
        if (__rt.formCount(promoted) !== 8 || __rt.formGet(promoted, __rt.formList('a'), null) !== firstMember) throw 'promotion member identity';
        // Legacy duplicate sets have directional membership equality, including
        // after hashing both operands. Hash caches must not change that result.
        const left = set(__rt.formVector(1, 1)), right = set(__rt.formVector(1, 2));
        __rt.formHash(left); __rt.formHash(right);
        if (!__rt.formEquals(left, right) || __rt.formEquals(right, left)) throw 'duplicate equality';
        const nestedLeft = __rt.formVector(left), nestedRight = __rt.formVector(right);
        __rt.formHash(nestedLeft); __rt.formHash(nestedRight);
        if (!__rt.formEquals(nestedLeft, nestedRight)) throw 'nested duplicate equality';
        const positional = map(__rt.formVector('k', left)), hashed = __rt.formMap('k', right);
        if (!__rt.formEquals(positional, hashed) || !__rt.formEquals(hashed, positional)) throw 'map value direction';
        const lazy = __rt.lazySeq(() => __rt.formList(3, 4));
        const rebuilt = __rt.cljRealizeAll(map(__rt.formVector('a', lazy, 'a', 2)));
        if (__rt.formCount(rebuilt) !== 2 || __rt.formKind(__rt.formGet(rebuilt, 'a', null)) !== 'list' ||
            __rt.formNth(__rt.formNth(__rt.cljSeq(rebuilt), 1), 1) !== 2) throw 'realization keeps positions';
        const meta = map(__rt.formVector('tag', 'literal'));
        const tagged = __rt.formWithMeta(m, meta), taggedDup = __rt.formWithMeta(d, meta);
        return __rt.formEquals(m, tagged) && __rt.formHash(m) === __rt.formHash(tagged) &&
            __rt.formMeta(__rt.formAssoc(taggedDup, 'a', 3)) === meta &&
            __rt.formMeta(__rt.formDissoc(taggedDup, 'a')) === meta;
    }"#,
        &[],
    );
    assert_eq!(result.view(), ValueView::Bool(true));
}

#[test]
fn positional_directional_keys_use_query_first_equality() {
    let rt = Runtime::new();
    let result = call(
        &rt,
        r#"function check() {
        const a = __rt.formLiteralSet(__rt.formVector(0, 0));
        const b = __rt.formLiteralSet(__rt.formVector(0, 2147483647));
        if (__rt.formHash(a) !== __rt.formHash(b) || !__rt.formEquals(a, b) || __rt.formEquals(b, a)) throw 'fixture';
        const m = __rt.formMap(a, 'old');
        if (__rt.formGet(m, b, 'missing') !== 'missing' || __rt.formDissoc(m, b) !== m) throw 'query direction';
        const updated = __rt.formAssoc(m, b, 'new');
        if (__rt.formCount(updated) !== 2 || __rt.formGet(updated, a, null) !== 'old' ||
            __rt.formGet(updated, b, null) !== 'new') throw 'assoc direction';
        return true;
    }"#,
        &[],
    );
    assert_eq!(result.view(), ValueView::Bool(true));
}

#[test]
fn map_to_record_preserves_positional_extensions() {
    let rt = Runtime::new();
    let result = call(
        &rt,
        r#"function check() {
        const R = __rt.structType('example/R', __rt.formVector('x'), true);
        const x = __rt.formKeyword(null, 'x');
        const m = __rt.formLiteralMap(__rt.formVector('a', 1, x, 0, 'a', 2, 'z', 3));
        const r = __rt.recordFromMap(R, m), entries = __rt.cljSeq(r);
        if (__rt.cljCount(r) !== 4 || __rt.cljGet(r, 'a', null) !== 1 || __rt.cljGet(r, x, null) !== 0 ||
            __rt.formNth(__rt.formNth(entries, 1), 1) !== 1 ||
            __rt.formNth(__rt.formNth(entries, 2), 1) !== 2 ||
            __rt.formNth(__rt.formNth(entries, 3), 1) !== 3) throw 'extension positions';
        const duplicateField = __rt.formLiteralMap(__rt.formVector(x, 4, x, 5, 'z', 3));
        const duplicate = __rt.recordFromMap(R, duplicateField);
        if (__rt.cljGet(duplicate, x, null) !== 4 || __rt.cljCount(duplicate) !== 3 ||
            !__rt.cljContains(duplicate, null)) throw 'first field and dissoc holes';
        const meta = __rt.formMap('tag', true), copy = __rt.formWithMeta(r, meta);
        if (!__rt.formEquals(r, copy) || !__rt.formEquals(copy, r) ||
            __rt.formHash(r) !== __rt.formHash(copy)) throw 'record metadata identity';
        const ext = __rt.formLiteralMap(__rt.formVector('a', 1, 'a', 2));
        const direct = __rt.structNew(R, 0, null, ext);
        if (!__rt.formEquals(direct, __rt.structNew(R, 0, null, ext)) ||
            __rt.formEquals(direct, __rt.structNew(R, 0, null, __rt.formWithMeta(ext, meta)))) throw 'constructor extension identity';
        if (!__rt.formEquals(r, __rt.cljAssoc(__rt.cljAssoc(r, x, 9), x, 0)) ||
            __rt.formEquals(r, __rt.cljAssoc(r, 'a', 1)) ||
            __rt.formEquals(r, __rt.cljDissoc(r, 'missing'))) throw 'extension update copies';
        const updated = __rt.cljAssoc(r, 'a', 9), removed = __rt.cljDissoc(r, 'a');
        if (__rt.formEquals(r, updated) || __rt.formEquals(r, __rt.cljAssoc(updated, 'a', 1)) ||
            __rt.formEquals(r, __rt.cljDissoc(__rt.cljAssoc(r, 'new', 4), 'new')) ||
            !__rt.formEquals(updated, __rt.formWithMeta(updated, meta)) ||
            !__rt.formEquals(removed, __rt.formWithMeta(removed, meta))) throw 'updated extension identity';
        return true;
    }"#,
        &[],
    );
    assert_eq!(result.view(), ValueView::Bool(true));
}

#[test]
fn positional_equality_stops_at_the_first_full_hash_collision_match() {
    let rt = Runtime::new();
    let result = call(
        &rt,
        r#"function check() {
        const a = __rt.formLiteralSet(__rt.formVector(0, 0));
        const b = __rt.formLiteralSet(__rt.formVector(0, 2147483647));
        if (__rt.formHash(a) !== __rt.formHash(b) || !__rt.formEquals(a, b) || __rt.formEquals(b, a)) throw 'fixture';
        const h = __rt.formMap(a, 'first', b, 'second');
        const p = __rt.formLiteralMap(__rt.formVector(a, 'second', b, 'second'));
        const q = __rt.formMap(a, 'second', b, 'second');
        const match = __rt.formLiteralMap(__rt.formVector(a, 'first', b, 'second'));
        if (__rt.formCount(h) !== 2 || __rt.formGet(h, a, null) !== 'first' ||
            __rt.formGet(h, b, null) !== 'second') throw 'collision entries';
        return !__rt.formEquals(p, h) && !__rt.formEquals(h, p) &&
            !__rt.formEquals(q, h) && !__rt.formEquals(h, q) &&
            __rt.formEquals(match, h) && __rt.formEquals(h, match);
    }"#,
        &[],
    );
    assert_eq!(result.view(), ValueView::Bool(true));
}

#[test]
fn record_extensions_publish_and_trace_map_identity_and_metadata() {
    let rt = Runtime::new();
    let made = call(
        &rt,
        r#"function make() {
        const R = __rt.structType('example/R', __rt.formVector('x'), true);
        const meta = __rt.formMap('tag', __rt.formVector('kept'));
        const ext = __rt.formWithMeta(__rt.formLiteralMap(__rt.formVector('a', 1, 'a', 2)), meta);
        const r = __rt.structNew(R, 0, null, ext);
        return __rt.formVector(r, __rt.formWithMeta(r, __rt.formMap('copy', true)), ext, meta);
    }"#,
        &[],
    );
    let (records, weak) = rt.scope(|s| {
        let v = s.local(&made).unwrap();
        (
            [0, 1].map(|i| s.persist(s.form_nth(v, i).unwrap())),
            [2, 3].map(|i| s.downgrade(s.form_nth(v, i).unwrap())),
        )
    });
    for r in &records {
        rt.publish(r).unwrap();
    }
    drop(made);
    rt.collect().unwrap();
    assert!(weak.iter().all(|w| w.is_live()));
    let result = call(
        &rt,
        r#"function read(r, copy) {
        return __rt.formEquals(r, copy) && __rt.formEquals(copy, r) &&
            __rt.formHash(r) === __rt.formHash(copy) && __rt.cljGet(r, 'a', null) === 1;
    }"#,
        &[&records[0], &records[1]],
    );
    assert_eq!(result.view(), ValueView::Bool(true));
    drop(result);
    drop(records);
    rt.collect().unwrap();
    assert!(weak.iter().all(|w| !w.is_live()));

    // Metadata on the retained extension map is also part of the record's
    // reachable graph, even though record equality and hashing ignore it.
    let bad = call(
        &rt,
        r#"function make() {
        const R = __rt.structType('example/R', __rt.formVector('x'), true);
        const ext = __rt.formWithMeta(__rt.formLiteralMap(__rt.formVector('a', 1, 'a', 2)), __rt.formMap('hidden', {}));
        return __rt.structNew(R, 0, null, ext);
    }"#,
        &[],
    );
    assert!(matches!(
        rt.publish(&bad),
        Err(PublishError::NotShareable { .. })
    ));
}

#[test]
fn positional_equality_does_not_hash_lazy_keys() {
    let rt = Runtime::new();
    let result = call(
        &rt,
        r#"function check() {
        const key = __rt.lazySeq(() => { throw 'hashed lazy key'; });
        const m = __rt.formLiteralMap(__rt.formVector('a', 1, 'a', 1, key, 2));
        const s = __rt.formLiteralSet(__rt.formVector(1, 1, key));
        const meta = __rt.formMap('tag', true);
        const n = __rt.formWithMeta(m, meta), t = __rt.formWithMeta(s, meta);
        return __rt.formEquals(m, n) && __rt.formEquals(n, m) &&
            __rt.formEquals(s, t) && __rt.formEquals(t, s);
    }"#,
        &[],
    );
    assert_eq!(result.view(), ValueView::Bool(true));
}

#[test]
fn positional_equality_short_circuits_in_source_order() {
    let rt = Runtime::new();
    let result = call(
        &rt,
        r#"function check() {
        const late = __rt.lazySeq(() => { throw 'compared late entry'; });
        const m = __rt.formLiteralMap(__rt.formVector('a', 0, 'a', 0, 'late', late));
        const n = __rt.formLiteralMap(__rt.formVector('a', 1, 'a', 1, 'late', __rt.formVector(1)));
        const s = __rt.formLiteralSet(__rt.formVector(1, 1, late));
        const t = __rt.formLiteralSet(__rt.formVector(2, 2, __rt.formVector(1)));
        // An absent key must also short-circuit before its lazy value.
        const missing = __rt.formLiteralMap(__rt.formVector('missing', late));
        const other = __rt.formMap('other', __rt.formVector(1));
        return !__rt.formEquals(m, n) && !__rt.formEquals(n, m) &&
            !__rt.formEquals(s, t) && !__rt.formEquals(t, s) &&
            !__rt.formEquals(missing, other) && !__rt.formEquals(other, missing);
    }"#,
        &[],
    );
    assert_eq!(result.view(), ValueView::Bool(true));
}

#[test]
fn positional_forms_publish_and_trace_every_duplicate_and_metadata() {
    let rt = Runtime::new();
    let (root, weak) = rt.scope(|s| {
        let one = s.primitive(Primitive::Number(1.0));
        let first = s.new_occurrence(one, at(1, 1)).unwrap();
        let second = s.new_occurrence(one, at(1, 2)).unwrap();
        let payload = s.new_form_vector(&[second]).unwrap();
        let map = s
            .new_literal_map(&[(first, first), (second, payload)])
            .unwrap();
        let set = s.new_literal_set(&[first, second]).unwrap();
        let meta = s.new_literal_map(&[(first, payload)]).unwrap();
        let map = s.form_with_meta(map, Some(meta)).unwrap();
        let root = s.new_form_vector(&[map, set]).unwrap();
        (
            s.persist(root),
            [first, second, payload, meta].map(|v| s.downgrade(v)),
        )
    });
    rt.publish(&root).unwrap();
    rt.collect().unwrap();
    assert!(weak.iter().all(|w| w.is_live()));
    let result = call(
        &rt,
        r#"function read(root) {
        const m = __rt.formNth(root, 0), s = __rt.formNth(root, 1);
        return __rt.formCount(m) === 2 && __rt.formCount(s) === 2 &&
            __rt.formValue(__rt.formGet(m, 1, null)) === 1 &&
            __rt.formCount(__rt.formNth(__rt.formNth(__rt.cljSeq(m), 1), 1)) === 1;
    }"#,
        &[&root],
    );
    assert_eq!(result.view(), ValueView::Bool(true));
    drop(result);
    drop(root);
    rt.collect().unwrap();
    assert!(weak.iter().all(|w| !w.is_live()));

    for expression in [
        "__rt.formLiteralMap(__rt.formVector(1, 0, 1, {}))",
        "__rt.formLiteralSet(__rt.formVector(1, {}))",
        "__rt.formWithMeta(__rt.formLiteralSet(__rt.formVector(1, 1)), __rt.formLiteralMap(__rt.formVector(1, {})))",
    ] {
        let value = call(
            &rt,
            &format!("function make() {{ return {expression}; }}"),
            &[],
        );
        assert!(
            matches!(rt.publish(&value), Err(PublishError::NotShareable { .. })),
            "{expression}"
        );
    }
}

#[test]
fn positional_storage_obeys_the_aggregate_byte_limit() {
    use big_boy_pants_js::{HeapPolicy, VmLimits};
    let policy = |bytes| HeapPolicy {
        hard_limit_bytes: Some(bytes),
        ..Default::default()
    };
    let realm = Runtime::try_with_heap_policy(VmLimits::default(), policy(usize::MAX / 2))
        .unwrap()
        .heap_stats()
        .managed_bytes;
    let limit = realm + 256 * 1024;
    let rt = Runtime::try_with_heap_policy(VmLimits::default(), policy(limit)).unwrap();
    rt.scope(|s| {
        let one = s.primitive(Primitive::Number(1.0));
        // One rooted scalar repeated many times: the payload nodes, rather
        // than member objects, must be charged before they are allocated.
        assert_eq!(
            s.new_literal_set(&vec![one; 65_536]).err(),
            Some(CollectionError::HeapLimit)
        );
        assert_eq!(
            s.new_literal_map(&vec![(one, one); 65_536]).err(),
            Some(CollectionError::HeapLimit)
        );
    });
    assert!(rt.heap_stats().peak_managed_bytes <= limit);
}

#[test]
fn default_sorted_map_comparison_memoizes_shared_vector_pairs() {
    let rt = Runtime::new();
    let result = call(
        &rt,
        r#"function f() {
        let a = 0, b = 0;
        for (let i = 0; i < 30; i++) {
            a = __rt.formVector(a, a);
            b = __rt.formVector(b, b);
        }
        const map = __rt.formSortedMap(__rt.formVector(a, "first", b, "second"));
        if (__rt.formCount(map) !== 1 || __rt.formGet(map, b, null) !== "second") return false;
        // The original key survives replacement. A later unequal child
        // still decides order after the shared prefix has been memoized.
        if (__rt.formNth(__rt.formKeys(map), 0) !== a) return false;
        const c = __rt.formVector(a, 1), d = __rt.formVector(b, 2);
        const ordered = __rt.formSortedMap(__rt.formVector(d, "last", c, "first"));
        return __rt.formCount(ordered) === 2 && __rt.formGet(ordered, c, null) === "first";
    }"#,
        &[],
    );
    assert_eq!(result.view(), ValueView::Bool(true));
}

#[test]
fn structural_key_operations_propagate_sorted_comparison_errors() {
    let rt = Runtime::new();
    let result = call(
        &rt,
        r#"function f() {
        const stored = __rt.formSortedMap(__rt.formVector(1, "v"));
        const query = __rt.formLiteralMap(__rt.formVector("x", "v"));
        const hashStored = __rt.formSortedMap(__rt.formVector(0, "v"));
        const hashQuery = __rt.formLiteralMap(__rt.formVector("", "v"));
        if (__rt.formHash(hashStored) !== __rt.formHash(hashQuery)) throw new Error('equal hash precondition');
        const cases = [
            [__rt.formLiteralMap(__rt.formVector(stored, "stored")), query, "Cannot compare 1 to x"],
            [__rt.formMap(hashStored, "stored"), hashQuery, "Cannot compare 0 to "]];
        for (const item of cases) {
            const map = item[0], key = item[1], message = item[2];
            const operations = [() => __rt.formGet(map, key, "missing"),
                () => __rt.cljContains(map, key),
                () => __rt.formAssoc(map, key, "new"), () => __rt.formDissoc(map, key)];
            for (const op of operations) {
                try { op(); return false; }
                catch(e) { if (e.message !== message) return false; }
            }
        }
        return true;
    }"#,
        &[],
    );
    assert_eq!(result.view(), ValueView::Bool(true));
}

#[test]
fn sorted_comparison_map_snapshots_obey_heap_budget() {
    use big_boy_pants_js::{Budget, HeapPolicy, Host, HostCall, VmLimits};
    struct Marker(bool);
    impl Host for Marker {
        fn effect(&mut self, _: HostCall) -> Result<Primitive, String> {
            self.0 = true;
            Ok(Primitive::Null)
        }
    }
    for entries in [1, 64] {
        let policy = |bytes| HeapPolicy {
            hard_limit_bytes: Some(bytes),
            ..Default::default()
        };
        let realm = Runtime::try_with_heap_policy(VmLimits::default(), policy(usize::MAX / 2))
            .unwrap()
            .heap_stats()
            .managed_bytes;
        let limit = realm + (1 << 20);
        let rt = Runtime::try_with_heap_policy(VmLimits::default(), policy(limit)).unwrap();
        let key = rt.scope(|s| {
            let pairs: Vec<_> = (0..entries)
                .map(|i| {
                    let key = s.primitive(Primitive::Number(i as f64));
                    (key, key)
                })
                .collect();
            let map = s.new_form_map(&pairs).unwrap();
            s.persist(map)
        });
        let code = compile_function("function f(key) { const m = __rt.formSortedMap(__rt.formVector(1,0)); __rt.effect(7,0); try { __rt.formGet(m,key,null); } catch(e) { return e === 'heap byte limit exceeded'; } return null; }").unwrap();
        let task = rt.spawn(&code, &[&key]).unwrap();
        let mut marker = Marker(false);
        while !marker.0 {
            assert!(matches!(
                task.run_with(Budget::instructions(1), &mut marker).unwrap(),
                RunOutcome::Yielded
            ));
        }
        rt.collect().unwrap();
        // Pause after admission of the diagnostic traversal, then consume
        // its three bounded prefix steps (header, first operand, separator).
        let mut started = false;
        for _ in 0..100 {
            let before = rt.heap_stats().managed_bytes;
            assert!(matches!(
                task.run_with(Budget::instructions(1), &mut marker).unwrap(),
                RunOutcome::Yielded
            ));
            if rt.heap_stats().managed_bytes.saturating_sub(before) >= 2048 {
                started = true;
                break;
            }
        }
        assert!(started, "diagnostic traversal must be admitted");
        for _ in 0..3 {
            assert!(matches!(
                task.run_with(Budget::instructions(1), &mut marker).unwrap(),
                RunOutcome::Yielded
            ));
        }
        // Leave less than even a one-entry B-tree node needs, immediately
        // before the map operand starts its snapshot reservation.
        let before = rt.heap_stats().managed_bytes;
        let probe = rt.scope(|s| s.persist(s.try_string_units(&[0]).unwrap()));
        let overhead = rt.heap_stats().managed_bytes - before - 2;
        drop(probe);
        let units = (limit - rt.heap_stats().managed_bytes - 512 - overhead) / 2;
        let padding = rt.scope(|s| s.persist(s.try_string_units(&vec![0; units]).unwrap()));
        let occupied = rt.heap_stats().managed_bytes;
        assert!(matches!(
            task.run_with(Budget::instructions(1), &mut marker).unwrap(),
            RunOutcome::Yielded
        ));
        assert!(
            occupied - rt.heap_stats().managed_bytes >= 2048,
            "snapshot admission must unwind the diagnostic before scanning entries"
        );
        let outcome = loop {
            match task
                .run_with(Budget::instructions(97), &mut marker)
                .unwrap()
            {
                RunOutcome::Yielded => (),
                other => break other,
            }
        };
        let value = complete(outcome);
        assert_eq!(
            value.view(),
            ValueView::Bool(true),
            "snapshot denial must be catchable"
        );
        assert!(rt.heap_stats().peak_managed_bytes <= limit);
        drop((value, padding, key, task));
        rt.collect().unwrap();
        assert!(rt.heap_stats().managed_bytes < limit);
    }
}

#[test]
fn sorted_comparison_map_snapshot_cancellation_releases_storage() {
    use big_boy_pants_js::{Budget, HeapPolicy, Host, HostCall, VmLimits};
    struct NoHost;
    impl Host for NoHost {
        fn effect(&mut self, _: HostCall) -> Result<Primitive, String> {
            unreachable!()
        }
    }
    let limit = 16 << 20;
    let rt = Runtime::try_with_heap_policy(
        VmLimits::default(),
        HeapPolicy {
            hard_limit_bytes: Some(limit),
            ..Default::default()
        },
    )
    .unwrap();
    let entries = 128;
    let (key, weak) = rt.scope(|s| {
        let value = s.new_form_vector(&[]).unwrap();
        let pairs: Vec<_> = (0..entries)
            .map(|i| (s.primitive(Primitive::Number(i as f64)), value))
            .collect();
        let map = s.new_form_map(&pairs).unwrap();
        (s.persist(map), [s.downgrade(map), s.downgrade(value)])
    });
    let code = compile_function(
        "function f(key){return __rt.formGet(__rt.formSortedMap(__rt.formVector(1,0)),key,null);}",
    )
    .unwrap();
    let task = rt.spawn(&code, &[&key]).unwrap();
    drop(key);
    rt.collect().unwrap();
    let mut paused = false;
    for _ in 0..500 {
        let before = rt.heap_stats().managed_bytes;
        assert!(matches!(
            task.run_with(Budget::instructions(1), &mut NoHost).unwrap(),
            RunOutcome::Yielded
        ));
        let after = rt.heap_stats().managed_bytes;
        // The snapshot is admitted in one step, before any tree insertion.
        if after.saturating_sub(before) >= entries * 512 {
            for _ in 0..8 {
                assert!(matches!(
                    task.run_with(Budget::instructions(1), &mut NoHost).unwrap(),
                    RunOutcome::Yielded
                ));
            }
            paused = true;
            break;
        }
    }
    assert!(paused, "must pause with a partially scanned map snapshot");
    rt.collect().unwrap();
    assert!(weak.iter().all(|value| value.is_live()));
    let occupied = rt.heap_stats().managed_bytes;
    let _ = task.cancel();
    assert!(matches!(
        task.run_with(Budget::instructions(1), &mut NoHost).unwrap(),
        RunOutcome::Cancelled
    ));
    assert!(
        occupied - rt.heap_stats().managed_bytes >= entries * 512,
        "cancellation must immediately release snapshot storage"
    );
    rt.collect().unwrap();
    assert!(weak.iter().all(|value| !value.is_live()));
    assert!(rt.heap_stats().peak_managed_bytes <= limit);
}

#[test]
fn sorted_comparison_diagnostics_obey_heap_budget() {
    use big_boy_pants_js::{HeapPolicy, VmLimits};
    let policy = |bytes| HeapPolicy {
        hard_limit_bytes: Some(bytes),
        ..Default::default()
    };
    let realm = Runtime::try_with_heap_policy(VmLimits::default(), policy(usize::MAX / 2))
        .unwrap()
        .heap_stats()
        .managed_bytes;
    let limit = realm + 512 * 1024;
    let rt = Runtime::try_with_heap_policy(VmLimits::default(), policy(limit)).unwrap();
    // Thirty tiny shared nodes would expand to about 4 GiB of text. The
    // error must fail admission while its source graph still fits easily.
    let key = rt.scope(|s| {
        let mut key = s.primitive(Primitive::Number(0.0));
        for _ in 0..30 {
            key = s.new_form_vector(&[key, key]).unwrap();
        }
        s.persist(key)
    });
    let code =
        compile_function("function f(key){return __rt.formSortedMap(__rt.formVector(1,0,key,0));}")
            .unwrap();
    match rt.spawn(&code, &[&key]).unwrap().run().unwrap() {
        RunOutcome::Failed(error) => assert_eq!(error.kind(), VmErrorKind::HeapLimit),
        other => panic!("expected comparison admission failure, got {other:?}"),
    }
    assert!(rt.heap_stats().peak_managed_bytes <= limit);
    drop(key);
    rt.collect().unwrap();
    assert!(rt.heap_stats().managed_bytes < limit);
}

#[test]
fn sorted_comparison_diagnostics_yield_and_keep_operands_rooted() {
    use big_boy_pants_js::{Budget, Host, HostCall};
    struct NoHost;
    impl Host for NoHost {
        fn effect(&mut self, _: HostCall) -> Result<Primitive, String> {
            panic!("formatting must not run host effects")
        }
    }
    let rt = Runtime::new();
    let key = rt.scope(|s| {
        let mut key = s.primitive(Primitive::Number(0.0));
        for _ in 0..10 {
            key = s.new_form_vector(&[key, key]).unwrap();
        }
        s.persist(key)
    });
    let code = compile_function("function f(key){try {__rt.formSortedMap(__rt.formVector(1,0,key,0));} catch(e){return e.message;}}").unwrap();
    let task = rt.spawn(&code, &[&key]).unwrap();
    drop(key);
    let mut steps = 0;
    let result = loop {
        match task.run_with(Budget::instructions(1), &mut NoHost).unwrap() {
            RunOutcome::Yielded => {
                steps += 1;
                if steps % 100 == 0 {
                    rt.collect().unwrap();
                }
                assert!(steps < 20_000, "comparison failed to finish");
            }
            RunOutcome::Complete(result) => break result,
            other => panic!("expected yielded diagnostic or completion, got {other:?}"),
        }
    };
    assert!(steps > 1_000, "formatting must span bounded VM steps");
    let mut expected = "0".to_string();
    for _ in 0..10 {
        expected = format!("[{expected} {expected}]");
    }
    assert_eq!(
        result.string(),
        Some(js(&format!("Cannot compare 1 to {expected}")))
    );
}

#[test]
fn sorted_comparison_host_diagnostics_remain_complete_and_accounted() {
    use big_boy_pants_js::{HeapPolicy, VmLimits};
    let rt = Runtime::try_with_heap_policy(
        VmLimits::default(),
        HeapPolicy {
            hard_limit_bytes: Some(16 << 20),
            ..Default::default()
        },
    )
    .unwrap();
    let text = "x".repeat(4096);
    let key = rt.scope(|s| {
        let key = s.primitive(Primitive::String(js(&text)));
        s.persist(key)
    });
    let code =
        compile_function("function f(key){return __rt.formSortedMap(__rt.formVector(1,0,key,0));}")
            .unwrap();
    let RunOutcome::Failed(error) = rt.spawn(&code, &[&key]).unwrap().run().unwrap() else {
        panic!("expected comparison failure");
    };
    assert_eq!(error.kind(), VmErrorKind::Error);
    assert_eq!(error.message(), format!("Cannot compare 1 to {text}"));
    let occupied = rt.heap_stats().managed_bytes;
    let copy = error.clone();
    assert_eq!(rt.heap_stats().managed_bytes, occupied);
    drop(error);
    assert_eq!(rt.heap_stats().managed_bytes, occupied);
    assert_eq!(copy.message(), format!("Cannot compare 1 to {text}"));
    drop(copy);
    assert!(rt.heap_stats().managed_bytes < occupied);
}

/// Metadata never changes equality or hashing, and does not change identity
/// rules: a new object, unless the metadata is the same object. Updates
/// keep metadata.
#[test]
fn metadata_does_not_change_equality_or_hash() {
    let rt = Runtime::new();
    rt.scope(|s| {
        let kw = |n: &str| s.new_keyword(None, js(n)).unwrap();
        let num = |n: f64| s.primitive(Primitive::Number(n));
        let meta = s.new_form_map(&[(kw("line"), num(7.0))]).unwrap();
        let other = s.new_form_map(&[(kw("line"), num(8.0))]).unwrap();
        let sym = s.new_symbol(Some(js("my.ns")), js("f")).unwrap();
        let list = s.new_list(&[sym, num(1.0)]).unwrap();
        let vec = s.new_form_vector(&[sym]).unwrap();
        let map = s.new_form_map(&[(kw("k"), vec)]).unwrap();
        for v in [sym, list, vec, map] {
            let a = s.form_with_meta(v, Some(meta)).unwrap();
            let b = s.form_with_meta(a, Some(other)).unwrap();
            assert!(!s.same_object(a, v) && !s.same_object(a, b));
            assert!(s.form_equals(a, v) && s.form_equals(a, b));
            assert_eq!(s.form_hash(a), s.form_hash(v));
            assert_eq!(s.form_hash(b), s.form_hash(v));
            // As map keys, the variants find each other's entries.
            let m = s.new_form_map(&[(a, num(1.0))]).unwrap();
            assert!(s.form_get(m, b).unwrap().is_some());
            assert!(s.form_get(m, v).unwrap().is_some());
            let same = s.form_with_meta(a, Some(meta)).unwrap();
            assert!(s.same_object(same, a), "same metadata: same object");
            let stripped = s.form_with_meta(a, None).unwrap();
            assert!(s.form_meta(stripped).is_none() && s.form_equals(stripped, a));
            assert!(s.same_object(s.form_meta(a).unwrap(), meta));
        }
        let vec = s.form_with_meta(vec, Some(meta)).unwrap();
        let updated = s.form_conj(vec, num(2.0)).unwrap();
        assert!(s.same_object(s.form_meta(updated).unwrap(), meta));
        let keyword = kw("k");
        assert_eq!(
            s.form_with_meta(keyword, Some(meta)).err(),
            Some(CollectionError::WrongType {
                expected: "symbol, list, form vector, form map, form set or record",
                found: "keyword"
            })
        );
        assert_eq!(
            s.form_with_meta(sym, Some(vec)).err(),
            Some(CollectionError::WrongType {
                expected: "form map or nil",
                found: "form vector"
            })
        );
    });
}

/// Equal scalars at different source locations are distinct occurrences:
/// equal values, different objects and provenances. A literal no Number
/// represents exactly survives an unchanged round trip through a task
/// because its occurrence (and so its provenance) is returned by identity;
/// a number the task computes has none.
#[test]
fn distinct_occurrences_and_exact_literals_survive_a_round_trip() {
    let rt = Runtime::new();
    // The host's own table: source location -> exact reader text.
    let mut exact: HashMap<(u32, u32), &str> = HashMap::new();
    let form = rt.scope(|s| {
        let mut occurrence = |text: &'static str, n: f64, line: u32| {
            exact.insert((line, 2), text);
            let value = s.primitive(Primitive::Number(n));
            s.new_occurrence(value, at(line, 2)).unwrap()
        };
        let big = occurrence("12345678901234567891", 12_345_678_901_234_567_891.0, 2);
        let one = occurrence("1", 1.0, 3);
        let one_point_zero = occurrence("1.0", 1.0, 4);
        let head = s.new_symbol(None, js("f")).unwrap();
        // Occurrences are transparent for equality, hashing and keys.
        assert!(s.form_equals(one, one_point_zero) && !s.same_object(one, one_point_zero));
        assert_ne!(s.provenance(one), s.provenance(one_point_zero));
        assert_eq!(
            s.form_hash(one),
            s.form_hash(s.primitive(Primitive::Number(1.0)))
        );
        let k = s.new_keyword(None, js("k")).unwrap();
        let k_occurrence = s.new_occurrence(k, at(9, 9)).unwrap();
        let v = s.primitive(Primitive::Bool(true));
        let m = s.new_form_map(&[(k_occurrence, v)]).unwrap();
        let k2 = s.new_keyword(None, js("k")).unwrap();
        assert!(s.form_get(m, k2).unwrap().is_some());
        assert!(s.same_object(s.form_value(k_occurrence), k));
        s.persist(s.new_list(&[head, big, one, one_point_zero]).unwrap())
    });
    rt.publish(&form).unwrap();
    let result = call(
        &rt,
        "function macro(form) {
            const big = __rt.formNth(form, 1), one = __rt.formNth(form, 2), onePointZero = __rt.formNth(form, 3);
            if (one === onePointZero || !__rt.formEquals(one, onePointZero)) throw new Error('occurrences');
            if (__rt.formKind(one) !== 'occurrence' || __rt.formValue(one) !== 1) throw new Error('value');
            return __rt.formList(__rt.formSymbol(null, 'g'), big, one, onePointZero, __rt.formValue(big) + 0);
        }",
        &[&form],
    );
    // The host emits each element: exact text by provenance, else the Number.
    let emitted: Vec<String> = rt.scope(|s| {
        let items = s.form_items(s.local(&result).unwrap()).unwrap();
        let input = s.form_items(s.local(&form).unwrap()).unwrap();
        for i in 1..4 {
            assert!(
                s.same_object(items[i], input[i]),
                "occurrence {i} by identity"
            );
        }
        items[1..]
            .iter()
            .map(|&item| match s.provenance(item) {
                Ok(p) => exact[&(p.line, p.column)].to_string(),
                Err(_) => match s.view(item) {
                    ValueView::Number(n) => format!("{n}"),
                    other => panic!("unexpected {other:?}"),
                },
            })
            .collect()
    });
    assert_eq!(
        emitted,
        ["12345678901234567891", "1", "1.0", "12345678901234567000"]
    );
}

/// A task suspended at `__rt.need` keeps a form it holds alive while the
/// host drops its own root and collects; the task resumes (on another
/// thread) and returns it by identity. Once the result and task are gone,
/// every part is reclaimed.
#[test]
fn collection_during_suspension_preserves_live_forms_and_released_roots_reclaim() {
    let rt = Runtime::new();
    let (root, weak) = rt.scope(|s| {
        let kw = s.new_keyword(Some(js("ns")), js("k")).unwrap();
        let sym = s.new_symbol(None, js("s")).unwrap();
        let key = s.new_form_vector(&[kw, sym]).unwrap();
        let value = s.new_list(&[sym]).unwrap();
        let map = s.new_form_map(&[(key, value)]).unwrap();
        let weak: Vec<_> = [map, key, kw, sym, value]
            .iter()
            .map(|l| s.downgrade(*l))
            .collect();
        (s.persist(map), weak)
    });
    let code = compile_function(
        "function hold(m) {
            const key = __rt.formVector(__rt.formKeyword('ns', 'k'), __rt.formSymbol(null, 's'));
            const before = __rt.formGet(m, key, null);
            const n = __rt.need(5, 3);
            if (__rt.formGet(m, key, null) !== before) throw new Error('changed');
            return __rt.formVector(m, before, n);
        }",
    )
    .unwrap();
    let task = rt.spawn(&code, &[&root]).unwrap();
    let key = DependencyKey::new(rt.world(), DependencyId::new(5), ResultKind::new(3));
    match task.run().unwrap() {
        RunOutcome::Waiting(k) => assert_eq!(k, key),
        other => panic!("expected Waiting, got {other:?}"),
    }
    drop(root);
    for _ in 0..2 {
        rt.request_collection().unwrap();
        assert!(
            weak.iter().all(|w| w.is_live()),
            "held by the suspended task"
        );
    }
    let mut requests = task.take_producer_requests();
    let request = requests.pop().unwrap();
    let five = rt.root(Primitive::Number(5.0));
    let mut runnable = rt
        .complete(request.key(), request.producer(), Completion::Ready(&five))
        .unwrap()
        .into_runnable();
    let resumed = runnable.pop().unwrap();
    drop(task);
    let result = thread::spawn(move || complete(resumed.run().unwrap()))
        .join()
        .unwrap();
    rt.request_collection().unwrap();
    rt.scope(|s| {
        let items = s.form_items(s.local(&result).unwrap()).unwrap();
        assert!(s.same_object(items[0], s.upgrade(&weak[0]).unwrap()));
        assert!(s.same_object(items[1], s.upgrade(&weak[4]).unwrap()));
        assert_eq!(s.view(items[2]), ValueView::Number(5.0));
    });
    drop(result);
    let stats = rt.request_collection().unwrap();
    assert!(stats.reclaimed >= weak.len());
    assert!(
        weak.iter().all(|w| !w.is_live()),
        "released forms are reclaimed"
    );
}

/// A form reaching task-owned mutation anywhere (an element, a map value,
/// a map key, metadata, or inside an occurrence) is rejected on publication
/// with the path to it. Immutable form wrappers never make it shareable.
#[test]
fn a_mutable_descendant_is_rejected_on_publication_with_a_path() {
    let rt = Runtime::new();
    let cases = [
        (
            "__rt.formList(__rt.formSymbol(null, 'do'), __rt.formMap(__rt.formKeyword(null, 'k'), __rt.formVector(1, {})))",
            "value[1]{:k}[1]",
            "object",
        ),
        (
            "__rt.formWithMeta(__rt.formSymbol('my.ns', 'f'), __rt.formMap(__rt.formSymbol('a', 'b'), [1]))",
            "value.meta{a/b}",
            "array",
        ),
        (
            "__rt.formMap(__rt.formVector({}), 1)",
            "value{<form vector>}.key[0]",
            "object",
        ),
        // A set's members, direct and nested, and its metadata
        // (light-cljs-477).
        ("__rt.formSet(1, {})", "value{<object>}.key", "object"),
        (
            "__rt.formList(__rt.formSet(__rt.formVector(2, [3])))",
            "value[0]{<form vector>}.key[1]",
            "array",
        ),
        (
            "__rt.formWithMeta(__rt.formSet(1), __rt.formMap(__rt.formKeyword(null, 'm'), {}))",
            "value.meta{:m}",
            "object",
        ),
    ];
    for (expr, path, found) in cases {
        let result = call(&rt, &format!("function make() {{ return {expr}; }}"), &[]);
        let err = rt.publish(&result).unwrap_err();
        let PublishError::NotShareable {
            path: p,
            found: f,
            owner,
        } = &err
        else {
            panic!("{expr}: {err:?}")
        };
        assert_eq!((p.to_string().as_str(), *f), (path, found), "{expr}");
        assert!(owner.is_some(), "{expr}");
        assert!(err.to_string().contains(path), "{err}");
    }
    // An occurrence (an immutable wrapper) around a task-owned array.
    let array = call(&rt, "function make() { return [1]; }", &[]);
    let wrapped = rt.scope(|s| {
        let occ = s
            .new_occurrence(s.local(&array).unwrap(), at(1, 1))
            .unwrap();
        let sym = s.new_symbol(None, js("quote")).unwrap();
        s.persist(s.new_list(&[sym, occ]).unwrap())
    });
    let err = rt.publish(&wrapped).unwrap_err();
    let PublishError::NotShareable { path, found, .. } = err else {
        panic!("{err:?}")
    };
    assert_eq!(
        path.steps(),
        [PathStep::Index(1), PathStep::OccurrenceValue]
    );
    assert_eq!(found, "array");
    // The same value cannot become a dependency result either.
    let key = DependencyKey::new(rt.world(), DependencyId::new(1), ResultKind::new(1));
    let reader = compile_function("function r() { return __rt.need(1, 1); }").unwrap();
    let task = rt.spawn(&reader, &[]).unwrap();
    assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(_)));
    let request = task.take_producer_requests().pop().unwrap();
    assert_eq!(request.key(), key);
    assert!(
        rt.complete(
            request.key(),
            request.producer(),
            Completion::Ready(&wrapped)
        )
        .is_err()
    );
    // Map keys are named by spelling in paths.
    assert_eq!(KeyName::Form(":k".into()).to_string(), ":k");
}

/// Why a keyword or symbol name must split back at its first `/`, from the
/// light-cljs runtime's own source run in this engine: it compares names by
/// full spelling but hashes namespace and name, so `(keyword nil "a/b")` is
/// `=` to `(keyword "a" "b")` with another hash (the values recorded in
/// light-cljs `tests/fixtures/compiler-keyword-map-keys.tsv`), and a hash
/// map keeps both; symbols likewise (`symbol-full-spelling` in
/// `tests/fixtures/execution.tsv`). Native construction rejects the name
/// with no single split, through Rust and JavaScript alike, and on every
/// supported name (the ones the reader produces and host names such as
/// `a`/`b/c` or an empty namespace) agrees with the runtime's `=` and
/// `hash`.
#[test]
fn names_whose_spelling_does_not_split_back_are_rejected() {
    let rt = Runtime::new();
    let src = format!(
        r#"function check() {{
        const c = ({}).core;
        const a = c.keyword(null, 'a/b'), b = c.keyword('a', 'b');
        const sa = c.symbol(null, 'a/b/c'), sb = c.symbol('a/b/c');
        const runtime = [c.equal(a, b), c.hash(a), c.hash(b), c.count(c.hashMap(a, 1, b, 2)),
            c.equal(sa, sb), c.hash(sa) !== c.hash(sb), c.namespace(sb), c.name(sb)];
        // Supported names: native `=` and `hash` are the runtime's.
        const names = [[null, 'a'], [null, '/'], ['clojure.core', '/'], ['a', 'b'], ['a', 'b/c'],
            ['ns', ''], ['', 'x'], ['', '/'], [null, 'é😀'], ['\ud800', 'x\udc00']];
        const made = [];
        for (const [ns, n] of names) {{
            made.push([c.keyword(ns, n), __rt.formKeyword(ns, n)], [c.symbol(ns, n), __rt.formSymbol(ns, n)]);
        }}
        const mismatches = [];
        for (let i = 0; i < made.length; i++) {{
            const [r, f] = made[i];
            if (c.hash(r) !== __rt.formHash(f) || c.namespace(r) !== __rt.formNamespace(f) || c.name(r) !== __rt.formName(f))
                mismatches.push('hash or name ' + i);
            for (let j = 0; j < made.length; j++) {{
                if (c.equal(r, made[j][0]) !== __rt.formEquals(f, made[j][1])) mismatches.push('= ' + i + ' ' + j);
            }}
        }}
        return [...runtime, made.length, mismatches.join(', ')].join(' ');
    }}"#,
        include_str!("../../../src/js_runtime.js")
    );
    let result = call(&rt, &src, &[]);
    assert_eq!(
        result.string().map(|s| s.to_string_lossy()).as_deref(),
        Some("true 384366870 1482224565 2 true true a b/c 20 ")
    );
    // The fixture's canonical keyword hashes the same natively.
    rt.scope(|s| {
        let b = s.new_keyword(Some(js("a")), js("b")).unwrap();
        assert_eq!(s.form_hash(b), 1_482_224_565);
    });

    let ambiguous = [
        (None, "a/b"),
        (None, "a/b/c"),
        (None, "/x"),
        (Some("a/b"), "c"),
        (Some("a/"), ""),
        (Some(""), ""),
    ];
    rt.scope(|s| {
        for (ns, n) in ambiguous {
            assert_eq!(
                s.new_keyword(ns.map(js), js(n)).err(),
                Some(CollectionError::AmbiguousName),
                "keyword {ns:?} {n:?}"
            );
            assert_eq!(
                s.new_symbol(ns.map(js), js(n)).err(),
                Some(CollectionError::AmbiguousName),
                "symbol {ns:?} {n:?}"
            );
        }
    });
    for (ns, n) in ambiguous {
        let ns = ns.map_or("null".to_string(), |ns| format!("'{ns}'"));
        for op in ["formKeyword", "formSymbol"] {
            let src = format!("function f() {{ return __rt.{op}({ns}, '{n}'); }}");
            let e = failure(&rt, &src);
            assert_eq!(
                e.kind(),
                VmErrorKind::Collection(CollectionError::AmbiguousName),
                "{src}"
            );
            assert!(e.to_string().contains("split back"), "{e}");
        }
    }
}

/// The form hash is the light-cljs runtime's `hash`: checked against the
/// runtime's own functions run in this engine, and against values computed
/// by the runtime under Node.
#[test]
fn hashes_match_the_light_cljs_runtime() {
    let rt = Runtime::new();
    let src = r#"function check() {
        // Copied from the light-cljs runtime (src/js_runtime.js): rotateLeft,
        // mixK1, mixH1, fmix, hashInt, stringHash, hashChars, hashNamed and
        // mixCollectionHash. hashDouble reads the same bytes through typed
        // arrays instead of a DataView.
        function rotateLeft(value, distance) { return (value << distance) | (value >>> (32 - distance)); }
        function mixK1(value) { return Math.imul(rotateLeft(Math.imul(value, 0xcc9e2d51), 15), 0x1b873593); }
        function mixH1(hash, value) { return (Math.imul(rotateLeft(hash ^ value, 13), 5) + 0xe6546b64) | 0; }
        function fmix(hash, length) {
            hash ^= length;
            hash = Math.imul(hash ^ (hash >>> 16), 0x85ebca6b);
            hash = Math.imul(hash ^ (hash >>> 13), 0xc2b2ae35);
            return hash ^ (hash >>> 16);
        }
        function hashInt(value) { return value === 0 ? 0 : fmix(mixH1(0, mixK1(value)), 4); }
        function stringHash(value) {
            if (value == null) return 0;
            let result = 0;
            for (let i = 0; i < value.length; i++) result = Math.imul(result, 31) + value.charCodeAt(i);
            return result;
        }
        function hashChars(value) {
            let h = 0, i = 1;
            for (; i < value.length; i += 2) h = mixH1(h, mixK1(value.charCodeAt(i - 1) | (value.charCodeAt(i) << 16)));
            if (value.length & 1) h ^= mixK1(value.charCodeAt(value.length - 1));
            return fmix(h, value.length * 2);
        }
        function hashNamed(value) {
            const seed = hashChars(value.name);
            return seed ^ (stringHash(value.namespace) + 0x9e3779b9 + (seed << 6) + (seed >> 2));
        }
        function hashDouble(value) {
            const b = new Uint8Array(new Float64Array([value]).buffer);
            return ((b[0] << 24) | (b[1] << 16) | (b[2] << 8) | b[3]) ^ ((b[4] << 24) | (b[5] << 16) | (b[6] << 8) | b[7]);
        }
        function mixCollectionHash(basis, count) { return fmix(mixH1(0, mixK1(basis)), count); }
        // The runtime's `hash`, reading forms through the intrinsics.
        function hash(value) {
            value = __rt.formValue(value);
            if (value == null) return 0;
            if (typeof value === 'boolean') return value ? 1231 : 1237;
            if (typeof value === 'number') {
                if (!Number.isFinite(value)) return value === Infinity ? 2146435072 : value === -Infinity ? -1048576 : 2146959360;
                return Number.isSafeInteger(value) ? (Math.floor(value) % 2147483647) | 0 : hashDouble(value);
            }
            if (typeof value === 'string') return hashInt(stringHash(value));
            const kind = __rt.formKind(value);
            if (kind === 'keyword' || kind === 'symbol') {
                const named = {namespace: __rt.formNamespace(value), name: __rt.formName(value)};
                return (hashNamed(named) + (kind === 'keyword' ? 0x9e3779b9 : 0)) | 0;
            }
            if (kind === 'map') {
                const keys = __rt.formKeys(value), n = __rt.formCount(keys);
                let result = 0;
                for (let i = 0; i < n; i++) {
                    const k = __rt.formNth(keys, i);
                    result = (result + mixCollectionHash((Math.imul(31, 31 + hash(k)) + hash(__rt.formGet(value, k, null))) | 0, 2)) | 0;
                }
                return mixCollectionHash(result, n);
            }
            let result = 1;
            const n = __rt.formCount(value);
            for (let i = 0; i < n; i++) result = (Math.imul(result, 31) + hash(__rt.formNth(value, i))) | 0;
            return mixCollectionHash(result, n);
        }
        const kw = (ns, n) => __rt.formKeyword(ns, n), sym = (ns, n) => __rt.formSymbol(ns, n);
        const values = [null, undefined, true, false, 0, -0, 1, -1, 7, 2147483647, 2147483648, -2147483649,
            9007199254740991, -9007199254740991, 9007199254740992, 1.5, -2.75, 1e300, 5e-324, NaN, Infinity, -Infinity,
            '', 'a', 'ab', 'abc', '\ud800x', 'κλειδί', 'a long string with an odd count',
            kw(null, 'a'), kw('my.ns', 'a'), kw('', 'b'), sym(null, 'a'), sym('clojure.core', 'defn'), sym(null, 'é😀'),
            __rt.formList(), __rt.formVector(), __rt.formMap(),
            __rt.formList(1, 2), __rt.formVector(1, 2), __rt.formList(sym(null, 'f'), __rt.formVector(sym(null, 'x')), 'body'),
            __rt.formMap(kw(null, 'a'), 1), __rt.formMap(__rt.formVector(kw(null, 'k'), 1), __rt.formMap(kw(null, 'n'), null), 'x', __rt.formList(true, false)),
            __rt.formWithMeta(__rt.formVector(1, 2), __rt.formMap(kw(null, 'line'), 3))];
        let mismatches = [];
        for (let i = 0; i < values.length; i++) {
            if (__rt.formHash(values[i]) !== hash(values[i])) mismatches.push(i);
        }
        return mismatches.length === 0 ? values.length : -1 - mismatches[0];
    }"#;
    assert_eq!(call(&rt, src, &[]).view(), ValueView::Number(44.0));
    // Computed by the light-cljs runtime's functions under Node.
    rt.scope(|s| {
        let num = |n: f64| s.primitive(Primitive::Number(n));
        let kw_a = s.new_keyword(None, js("a")).unwrap();
        let expected = [
            (kw_a, -2_123_407_586),
            (
                s.new_keyword(Some(js("my.ns")), js("a")).unwrap(),
                -1_674_339_691,
            ),
            (s.new_symbol(None, js("a")).unwrap(), -482_876_059),
            (
                s.new_symbol(Some(js("clojure.core")), js("defn")).unwrap(),
                -1_504_648_747,
            ),
            (s.primitive(Primitive::String(js("abc"))), 74_834_163),
            (
                s.primitive(Primitive::String(
                    JsString::from_code_units(vec![0xD800, u16::from(b'x')]).unwrap(),
                )),
                188_715_951,
            ),
            (num(1.5), 63_551),
            (num(9_007_199_254_740_992.0), 16_451),
            (num(-7.0), -7),
            (
                s.new_form_vector(&[num(1.0), num(2.0)]).unwrap(),
                1_185_709_346,
            ),
            (s.new_list(&[num(1.0), num(2.0)]).unwrap(), 1_185_709_346),
            (s.new_list(&[]).unwrap(), -2_017_569_654),
            (s.new_form_map(&[(kw_a, num(1.0))]).unwrap(), -2_144_769_406),
        ];
        for (i, (v, h)) in expected.into_iter().enumerate() {
            assert_eq!(s.form_hash(v), h, "value {i}");
        }
    });
}

/// Sets (light-cljs-477) hash as the light-cljs runtime does, compare and
/// look members up structurally, and publish; the `__rt.clj*` core
/// operations pun `nil`, read through occurrences, and raise catchable
/// errors for unsupported kinds.
#[test]
fn sets_and_clojure_core_operations_follow_the_runtime() {
    let rt = Runtime::new();
    rt.scope(|s| {
        let num = |n: f64| s.primitive(Primitive::Number(n));
        let kw_a = s.new_keyword(None, js("a")).unwrap();
        let x = s.primitive(Primitive::String(js("x")));
        let pair = s.new_form_vector(&[num(1.0), num(2.0)]).unwrap();
        let inner = s.new_form_set(&[num(1.0)]).unwrap();
        // Computed by the light-cljs runtime's `hash` of `hash-set` under Node.
        for (members, h) in [
            (vec![], -15_128_758),
            (vec![num(1.0), num(2.0)], -1_751_407_317),
            (vec![kw_a, x, pair], 375_108_620),
            (vec![inner], -1_056_510_838),
        ] {
            let set = s.new_form_set(&members).unwrap();
            assert_eq!(s.form_hash(set), h);
            assert_eq!(s.form_kind(set), Some(FormKind::Set));
        }
        let a = s.new_form_set(&[num(1.0), pair, num(1.0)]).unwrap();
        let equal_pair = s.new_list(&[num(1.0), num(2.0)]).unwrap();
        let b = s.new_form_set(&[equal_pair, num(1.0)]).unwrap();
        assert_eq!(s.form_count(a), Ok(2));
        assert!(s.form_equals(a, b));
        assert!(s.same_object(s.form_get(a, equal_pair).unwrap().unwrap(), pair));
        let fewer = s.form_disj(a, equal_pair).unwrap();
        assert!(!s.form_equals(fewer, b));
        assert_eq!(s.form_items(fewer).unwrap().len(), 1);
        let shared = s.persist(a);
        rt.publish(&shared).unwrap();
    });
    let src = r#"function check() {
        const kw = n => __rt.formKeyword(null, n);
        const s = __rt.formSet(1, kw('a'), __rt.formVector(1));
        const checks = [
            __rt.formKind(s) === 'set',
            __rt.cljCount(s) === 3,
            __rt.cljContains(s, __rt.formList(1)),
            !__rt.cljContains(s, 2),
            __rt.cljCount(__rt.cljDisj(s, kw('a'))) === 2,
            __rt.cljDisj(s, 9) === s,
            __rt.cljConj(s, 1) === s,
            __rt.cljCount(__rt.cljSeq(s)) === 3,
            __rt.formKind(__rt.cljSeq(s)) === 'list',
            __rt.cljCount(null) === 0,
            __rt.cljFirst(null) === null,
            __rt.cljSeq(__rt.formVector()) === null,
            __rt.cljGet(null, 1, 'd') === 'd',
            __rt.cljGet(__rt.formMap(kw('k'), 1), kw('k'), null) === 1,
            __rt.cljGet(__rt.formVector(5, 6), 1, null) === 6,
            __rt.formKind(__rt.cljConj(null, 1)) === 'list',
            __rt.cljGet(__rt.cljAssoc(null, kw('k'), 2), kw('k'), null) === 2,
            __rt.cljDissoc(null, 1) === null,
            __rt.cljContains(__rt.formVector(1), 0) && !__rt.cljContains(__rt.formVector(1), 1),
            __rt.cljFirst(__rt.cljFirst(__rt.formMap(kw('k'), 1))) !== null,
            __rt.cljFirst('ab') === 'a' && __rt.cljCount('ab') === 2,
            __rt.cljNthOr(__rt.formList(1), 4, 'd') === 'd',
            __rt.cljTruth(0) && __rt.cljTruth('') && !__rt.cljTruth(null) && !__rt.cljTruth(false),
            __rt.cljCount(__rt.cljRest(__rt.formVector(1, 2, 3))) === 2,
            __rt.cljNext(__rt.formList(1)) === null,
            __rt.cljCount(__rt.cljCons(0, __rt.formVector(1))) === 2,
            __rt.cljMeta(__rt.cljWithMeta(__rt.formSet(), __rt.formMap(kw('m'), 1))) !== null,
        ];
        let message = null;
        try { __rt.cljSeq(1); } catch (e) { message = String(e); }
        let range = null;
        try { __rt.cljNth(__rt.formVector(), 0); } catch (e) { range = String(e); }
        const failed = checks.indexOf(false);
        if (failed >= 0) return 'check ' + failed;
        return message + ' | ' + range;
    }"#;
    let result = call(&rt, src, &[]);
    assert_eq!(
        result.string().map(|s| s.to_string_lossy()).as_deref(),
        Some(
            "TypeError: unsupported operation: native macro runtime seq on number | \
             RangeError: index out of bounds"
        )
    );
}

#[test]
fn scalar_predicates_and_callable_collections_preserve_occurrences() {
    let rt = Runtime::new();
    let [zero, no] = rt.scope(|s| {
        let zero = s.primitive(Primitive::Number(-0.0));
        let no = s.primitive(Primitive::Bool(false));
        [zero, no].map(|v| {
            let occurrence = s.new_occurrence(v, at(1, 1)).unwrap();
            s.persist(occurrence)
        })
    });
    let result = call(
        &rt,
        r#"function check(zero, no) {
        if (!__rt.cljIsZero(zero) || !__rt.cljIsZero(0) || __rt.cljIsZero('0') ||
            __rt.cljIsZero(null) || __rt.cljIsZero(false) || __rt.cljIsZero(NaN) ||
            !__rt.cljIsFalse(no) || __rt.cljIsFalse(0) || __rt.cljIsFalse(null)) throw new Error('predicates');
        const k = __rt.formKeyword(null, 'x'), sym = __rt.formSymbol('n', 'x');
        const m = __rt.formMap(k, no, sym, zero), v = __rt.formVector(no, zero);
        if (__rt.cljInvoke(k, m, null, 1) !== no || __rt.cljInvoke(sym, m, null, 1) !== zero ||
            __rt.cljInvoke(m, sym, null, 1) !== zero || __rt.cljInvoke(v, 0, null, 1) !== no ||
            __rt.cljInvoke(v, 0.5, null, 1) !== no || __rt.cljInvoke(v, 1.5, null, 1) !== zero ||
            __rt.cljInvoke(v, zero, null, 1) !== no ||
            __rt.cljInvoke(__rt.formSet(no), false, null, 1) !== no ||
            __rt.cljInvoke(m, 'absent', zero, 2) !== zero) throw new Error('lookup identity');
        for (const index of [-0.5, 2, NaN, Infinity, -Infinity]) {
            let message;
            try { __rt.cljInvoke(v, index, 'ignored', 2); } catch (e) { message = e.message; }
            if (message !== 'Index out of bounds') throw new Error('numeric bounds');
        }
        // Fractional truncation belongs only to the legacy callable ABI.
        if (__rt.cljGet(v, 0.5, 'missing') !== 'missing' ||
            __rt.cljNthOr(v, 0.5, 'missing') !== 'missing') throw new Error('index contract');
        let keyError, boundsError, callableError;
        try { __rt.cljInvoke(v, '0', null, 1); } catch (e) { keyError = e.message; }
        try { __rt.cljInvoke(v, 2, 'ignored', 2); } catch (e) { boundsError = e.message; }
        try { __rt.cljInvoke(null, 0, null, 1); } catch (e) { callableError = e instanceof TypeError; }
        return keyError === 'Key must be integer' && boundsError === 'Index out of bounds' && callableError;
    }"#,
        &[&zero, &no],
    );
    assert_eq!(result.view(), ValueView::Bool(true));
}

#[test]
fn editable_collection_test_preserves_map_entry_and_occurrence_distinctions() {
    let rt = Runtime::new();
    let entry = call(
        &rt,
        "function entry() { return __rt.cljFirst(__rt.cljSeq(__rt.formMap('a', 1))); }",
        &[],
    );
    let occurrence = rt.scope(|s| {
        let entry = s.local(&entry).unwrap();
        let occurrence = s.new_occurrence(entry, at(3, 4)).unwrap();
        s.persist(occurrence)
    });
    let result = call(
        &rt,
        r#"function probe(entry, occurrence) {
        const normalized = __rt.cljWithMeta(entry, null);
        const lazy = __rt.lazySeq(() => { throw new Error('must not realize'); });
        return !__rt.cljIsEditable(entry) && !__rt.cljIsEditable(occurrence) &&
            __rt.cljIsEditable(normalized) && __rt.cljIsEditable(__rt.formVector()) &&
            __rt.cljIsEditable(__rt.formMap()) && __rt.cljIsEditable(__rt.formSet()) &&
            !__rt.cljIsEditable(__rt.formList()) && !__rt.cljIsEditable(null) &&
            !__rt.cljIsEditable(lazy);
    }"#,
        &[&entry, &occurrence],
    );
    assert_eq!(result.view(), ValueView::Bool(true));
}

#[test]
fn reduced_markers_preserve_payload_identity_and_trace_it_through_collection() {
    let rt = Runtime::new();
    let (payload, weak) = rt.scope(|s| {
        let value = s.primitive(Primitive::Number(42.0));
        let occurrence = s.new_occurrence(value, at(2, 3)).unwrap();
        let payload = s.new_form_vector(&[occurrence]).unwrap();
        (
            s.persist(payload),
            [s.downgrade(payload), s.downgrade(occurrence)],
        )
    });
    let marker = call(
        &rt,
        "function wrap(x) { return __rt.cljReduced(x); }",
        &[&payload],
    );
    rt.publish(&marker).unwrap();
    drop(payload);
    rt.collect().unwrap();
    assert!(weak.iter().all(|w| w.is_live()));
    let result = call(
        &rt,
        r#"function check(marker) {
        const payload = __rt.cljUnreduced(marker), nested = __rt.cljReduced(marker);
        if (!__rt.cljIsReduced(marker) || __rt.cljIsReduced(payload) ||
            __rt.cljUnreduced(nested) !== marker || __rt.cljUnreduced(payload) !== payload ||
            __rt.formEquals(marker, __rt.cljReduced(payload)) || !__rt.formEquals(marker, marker) ||
            __rt.formKind(marker) !== 'reduced') throw new Error('marker semantics');
        const keyed = __rt.formMap(marker, payload);
        if (__rt.formGet(keyed, marker, null) !== payload) throw new Error('identity key');
        let unsupported = false;
        try { __rt.cljSeq(marker); } catch (e) { unsupported = e instanceof TypeError; }
        if (!unsupported) throw new Error('opaque marker');
        return payload;
    }"#,
        &[&marker],
    );
    rt.scope(|s| {
        assert!(s.same_object(s.local(&result).unwrap(), s.upgrade(&weak[0]).unwrap()));
        let occurrence = s.form_nth(s.local(&result).unwrap(), 0).unwrap();
        assert!(s.same_object(occurrence, s.upgrade(&weak[1]).unwrap()));
    });
    drop(result);
    drop(marker);
    rt.collect().unwrap();
    assert!(weak.iter().all(|w| !w.is_live()));
}

#[test]
fn reduced_markers_do_not_publish_task_owned_payloads() {
    let rt = Runtime::new();
    for expression in ["{}", "[]", "(() => { let x = 0; return () => ++x; })()"] {
        let marker = call(
            &rt,
            &format!("function make() {{ return __rt.cljReduced({expression}); }}"),
            &[],
        );
        let PublishError::NotShareable { path, .. } = rt.publish(&marker).unwrap_err() else {
            panic!("published {expression}");
        };
        assert_eq!(path.steps()[0], PathStep::NativeEdge(0));
    }
}

#[test]
fn map_entry_calls_require_exact_indices_and_preserve_stored_occurrences() {
    let rt = Runtime::new();
    let [zero, no] = rt.scope(|s| {
        [Primitive::Number(-0.0), Primitive::Bool(false)].map(|value| {
            let value = s.primitive(value);
            let occurrence = s.new_occurrence(value, at(1, 1)).unwrap();
            s.persist(occurrence)
        })
    });
    let result = call(
        &rt,
        r#"function check(zero, no) {
        const entry = __rt.cljFirst(__rt.formMap(zero, no));
        if (__rt.cljInvoke(entry, zero, null, 1) !== zero ||
            __rt.cljInvoke(entry, 1, null, 1) !== no ||
            __rt.cljInvoke(entry, 1, zero, 2) !== no) throw new Error('entry identity');
        for (const index of [0.5, 1.5, -0.5, -1, 2, NaN, Infinity, '0', null, undefined, false]) {
            let message;
            try { __rt.cljInvoke(entry, index, null, 1); } catch (e) { message = e.message; }
            if (message !== 'Index out of bounds') throw new Error('entry bounds');
            for (const fallback of [zero, no, null, undefined]) {
                if (__rt.cljInvoke(entry, index, fallback, 2) !== fallback) throw new Error('fallback identity');
            }
        }
        for (const count of [0, 3, 4]) {
            let message;
            try { __rt.cljInvoke(entry, 0, zero, count); } catch (e) { message = e.message; }
            if (message !== 'Invalid arity: ' + count) throw new Error('entry arity');
        }
        // Entry and vector keys still compare and hash alike.
        const vector = __rt.formVector(zero, no);
        const map = __rt.formMap(entry, no);
        if (__rt.cljGet(map, vector, null) !== no ||
            __rt.cljInvoke(vector, 0.5, null, 1) !== zero ||
            __rt.cljInvoke(__rt.cljWithMeta(entry, null), 1.5, null, 1) !== no) throw new Error('vector semantics');
        return __rt.formVector(__rt.cljInvoke(entry, zero, null, 1),
                               __rt.cljInvoke(entry, 1, null, 1),
                               __rt.cljInvoke(entry, 0.5, zero, 2),
                               __rt.cljInvoke(entry, '0', no, 2));
    }"#,
        &[&zero, &no],
    );
    rt.scope(|s| {
        let items = s.form_items(s.local(&result).unwrap()).unwrap();
        assert_eq!(items.len(), 4);
        for (got, want) in items.iter().zip([&zero, &no, &zero, &no]) {
            assert!(s.same_object(*got, s.local(want).unwrap()));
        }
    });
}

/// Form operations reject other values with a `Collection` error naming the
/// expected and found kinds; the SameValueZero collections keep their own
/// identity-keyed contract and are not forms.
#[test]
fn operand_kinds_are_checked_and_internal_maps_are_unchanged() {
    let rt = Runtime::new();
    let wrong =
        |expected, found| VmErrorKind::Collection(CollectionError::WrongType { expected, found });
    for (src, kind) in [
        (
            "__rt.formCount(1)",
            wrong("list, form vector, form map or form set", "number"),
        ),
        (
            "__rt.formCount(__rt.vector(1))",
            wrong("list, form vector, form map or form set", "vector"),
        ),
        (
            "__rt.formGet(__rt.formList(1), 0, null)",
            wrong("form vector, form map or form set", "list"),
        ),
        ("__rt.formKeyword(null, 1)", wrong("string name", "number")),
        (
            "__rt.formSymbol(1, 'a')",
            wrong("string or nil namespace", "number"),
        ),
        (
            "__rt.formName(__rt.formList())",
            wrong("keyword or symbol", "list"),
        ),
        (
            "__rt.formDissoc(__rt.formVector(), 0)",
            wrong("form map", "form vector"),
        ),
        (
            "__rt.formConj(__rt.formMap(), 1)",
            wrong("list, form vector or form set", "form map"),
        ),
        (
            "__rt.formNth(__rt.formVector(1), 3)",
            VmErrorKind::Collection(CollectionError::IndexOutOfBounds { index: 3, count: 1 }),
        ),
        (
            "__rt.formAssoc(__rt.formVector(1), -1, 0)",
            VmErrorKind::Collection(CollectionError::InvalidIndex),
        ),
    ] {
        let e = failure(&rt, &format!("function f() {{ return {src}; }}"));
        assert_eq!(e.kind(), kind, "{src}");
    }
    let src = "function f() {
        const m = __rt.map(__rt.vector(1), 'internal');
        const f = __rt.formMap(__rt.formVector(1), 'form');
        return __rt.getOr(m, __rt.vector(1), 'by identity') === 'by identity'
            && __rt.formGet(f, __rt.formVector(1), null) === 'form'
            && __rt.formKind(m) === null && __rt.formKind(__rt.vector(1)) === null
            && __rt.formEquals(__rt.vector(1), __rt.vector(1)) === false;
    }";
    assert_eq!(call(&rt, src, &[]).view(), ValueView::Bool(true));
    let err = compile_function("function f() { return __rt.formKeyword('a'); }").unwrap_err();
    assert!(err.to_string().contains("takes 2 arguments"), "{err}");
}

#[test]
fn paused_task_global_snapshot_roots_forms_without_running_accessors() {
    let rt = Runtime::new();
    let executable = big_boy_pants_js::frontend::compile_script(
        "snapshot",
        r#"
        globalThis.result = __rt.formVector(__rt.formKeyword(null, 'native'), 42);
        Object.defineProperty(globalThis, 'accessor', {get() { throw new Error('must not run'); }});
        __rt.need(91, 1);
    "#,
    )
    .unwrap();
    let task = rt.spawn(&executable, &[]).unwrap();
    {
        let _guard = task.try_begin_run().unwrap();
        assert!(matches!(
            task.global_value("result"),
            Err(big_boy_pants_js::RunError::AlreadyRunning)
        ));
    }
    assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(_)));
    assert!(task.global_value("accessor").unwrap().is_none());
    assert!(task.global_value("missing").unwrap().is_none());
    let result = task.global_value("result").unwrap().unwrap();
    drop(task);
    rt.collect().unwrap();
    rt.scope(|s| {
        let result = s.local(&result).unwrap();
        assert_eq!(s.form_kind(result), Some(FormKind::Vector));
        assert_eq!(
            s.view(s.form_nth(result, 1).unwrap()),
            ValueView::Number(42.0)
        );
    });
}

#[test]
fn opaque_object_identity_is_heap_and_generation_scoped_without_rooting() {
    use std::collections::HashSet;
    let rt = Runtime::new();
    let mut identities = HashSet::new();
    let (old, weak) = rt.scope(|s| {
        assert!(
            s.object_identity(s.primitive(Primitive::Number(1.0)))
                .is_none()
        );
        let value = s.new_form_vector(&[]).unwrap();
        let identity = s.object_identity(value).unwrap();
        assert_eq!(identity, s.object_identity(value).unwrap());
        let other = s.new_form_vector(&[]).unwrap();
        assert_ne!(identity, s.object_identity(other).unwrap());
        identities.insert(identity.clone());
        (identity, s.downgrade(value))
    });
    rt.collect().unwrap();
    assert!(
        weak.upgrade().is_err(),
        "identity tokens are not collector roots"
    );
    rt.scope(|s| {
        let value = s.new_form_vector(&[]).unwrap();
        assert!(!identities.contains(&s.object_identity(value).unwrap()));
    });
    drop(weak);
    drop(rt);
    let other_runtime = Runtime::new();
    other_runtime.scope(|s| {
        let value = s.new_form_vector(&[]).unwrap();
        assert_ne!(old, s.object_identity(value).unwrap());
    });
}

#[test]
fn protocol_lookup_requires_declared_types_and_keeps_host_resolution_outside_the_leaf() {
    let rt = Runtime::new();
    let result = call(
        &rt,
        r#"function f() {
          const R = __rt.structType('ns/R', __rt.formVector(), true);
          const T = __rt.structType('ns/T', __rt.formVector(), false);
          const sameName = __rt.structType('ns/T', __rt.formVector(), false);
          const r = __rt.structNew(R), t = __rt.structNew(T);
          let declared = __rt.assoc(__rt.assoc(__rt.map(), R, true), T, true);
          let table = __rt.assoc(__rt.assoc(__rt.map(), R, 'record'), 'object', 'object');
          table = __rt.assoc(__rt.assoc(table, '_', 'default'), 'number', 'number');
          const check = (ok, what) => { if (!ok) throw new Error(what); };
          const lookup = (table, declared, x) => __rt.cljProtocolLookup(__rt.vector(1, 10, table, declared), 1, 10, x);
          const stale = __rt.vector(1, 10, null, null);
          check(__rt.cljProtocolLookup(null, 1, 10, r) === undefined, 'cold table');
          check(__rt.cljProtocolLookup(stale, 2, 10, r) === undefined, 'version before lookup');
          check(__rt.cljProtocolLookup(stale, 1, 9, r) === undefined, 'frontier before lookup');
          check(lookup(table, declared, r) === 'record', 'exact type');
          check(lookup(table, declared, t) === 'object', 'struct object before default');
          check(lookup(table, declared, 2) === 'number', 'primitive exact');
          check(lookup(table, declared, 's') === 'default', 'primitive ignores object');
          check(lookup(table, declared, __rt.structNew(sameName)) === undefined, 'type identity');
          check(lookup(table, __rt.map(), r) === undefined, 'declaration before exact entry');
          const host = { get __lightNativeType() { throw new Error('host getter'); } };
          check(lookup(table, declared, host) === undefined, 'host slow path');
          check(lookup(table, declared, Promise.resolve(1)) === undefined, 'Promise slow path');
          table = __rt.dissoc(table, 'object');
          check(lookup(table, declared, t) === 'default', 'struct default');
          table = __rt.dissoc(table, '_');
          check(lookup(table, declared, t) === null, 'missing struct');
          check(lookup(table, declared, null) === null, 'missing primitive');
          check(lookup(__rt.assoc(table, 'null', 'nil'), declared, undefined) === 'nil', 'nullish key');
          return true;
        }"#,
        &[],
    );
    assert!(matches!(result.view(), ValueView::Bool(true)));
}

/// Records, deftype instances and protocol dispatch tables
/// (light-cljs-478): struct operations from JavaScript, publication of a
/// struct with immutable fields (and rejection of one holding a JavaScript
/// object, with a path), and the GC edges of its fields.
#[test]
fn structs_follow_record_semantics_dispatch_by_type_identity_and_publish() {
    let rt = Runtime::new();
    let made = call(
        &rt,
        "function make() {
           const R = __rt.structType('my.ns/R', __rt.formVector('a', 'b'), true);
           const T = __rt.structType('my.ns/T', __rt.formVector('x'), false);
           const r = __rt.structNew(R, 1, __rt.formVector(2));
           const a = __rt.formKeyword(null, 'a'), c = __rt.formKeyword(null, 'c');
           const check = (ok, what) => { if (!ok) throw new Error(what); };
           check(__rt.formKind(r) === 'record' && __rt.formKind(__rt.structNew(T, 1)) === 'struct', 'kind');
           check(__rt.formKind(R) === 'type' && __rt.structTypeOf(r) === R, 'type');
           check(__rt.cljGet(r, a, null) === 1 && __rt.structField(r, 'a') === 1, 'field');
           const r2 = __rt.cljAssoc(r, c, 3);
           check(__rt.cljCount(r2) === 3 && __rt.cljGet(r2, c, null) === 3, 'extension');
           check(__rt.formKind(__rt.cljDissoc(r2, c)) === 'record', 'dissoc extension');
           check(__rt.formKind(__rt.cljDissoc(r, a)) === 'map', 'dissoc field');
           check(__rt.formEquals(r, __rt.cljDissoc(r2, c)), 'equal');
           check(__rt.formHash(r) === __rt.formHash(__rt.structNew(R, 1, __rt.formVector(2))), 'hash');
           check(!__rt.formEquals(__rt.structNew(T, 1), __rt.structNew(T, 1)), 'deftype identity');
           check(__rt.cljAssoc(r, a, 1) === r, 'unchanged assoc');
           const m = __rt.recordFromMap(R, __rt.formMap(a, 5, c, 6));
           check(__rt.cljGet(m, a, null) === 5 && __rt.cljGet(m, c, null) === 6, 'map->R');
           let table = __rt.map();
           table = __rt.assoc(table, R, 'R impl');
           table = __rt.assoc(table, 'string', 'string impl');
           table = __rt.assoc(table, '_', 'default impl');
           check(__rt.cljDispatch(table, r) === 'R impl' && __rt.cljDispatch(table, 'x') === 'string impl', 'dispatch');
           check(__rt.cljDispatch(table, __rt.structNew(T, 1)) === 'default impl', 'default');
           check(__rt.cljDispatch(table, {}) === undefined, 'object');
           return __rt.formVector(r, __rt.structNew(T, {}),
             __rt.cljAssoc(r, c, {}), __rt.structNew(R, 1, 2, __rt.formMap(c, {}), null),
             __rt.cljAssoc(r, c, __rt.formVector('kept')));
         }",
        &[],
    );
    let [record, holder, extended, annotated, kept] = rt.scope(|s| {
        let v = s.local(&made).unwrap();
        std::array::from_fn(|i| s.persist(s.form_nth(v, i).unwrap()))
    });
    // Extension entries and metadata are walked too.
    for (value, expected) in [(&extended, "value{:c}"), (&annotated, "value.meta{:c}")] {
        let PublishError::NotShareable { path, .. } = rt.publish(value).unwrap_err() else {
            panic!("published a record holding an object");
        };
        assert_eq!(path.to_string(), expected);
    }
    rt.publish(&record).unwrap();
    let PublishError::NotShareable { path, .. } = rt.publish(&holder).unwrap_err() else {
        panic!("published a struct holding an object");
    };
    assert_eq!(path.to_string(), "value[0]");
    drop(made);
    rt.collect().unwrap();
    // The record's fields survive collection while it is rooted.
    let field = call(
        &rt,
        "function read(r, k) { return __rt.formNth(__rt.cljGet(r, __rt.formKeyword(null, 'b'), null), 0) + __rt.formNth(__rt.cljGet(k, __rt.formKeyword(null, 'c'), null), 0); }",
        &[&record, &kept],
    );
    let read = rt.scope(|s| s.view(s.local(&field).unwrap()));
    assert!(matches!(read, ValueView::String), "{read:?}");
    rt.scope(|s| {
        assert_eq!(
            s.string(s.local(&field).unwrap())
                .unwrap()
                .to_string_lossy(),
            "2kept"
        )
    });
}

#[test]
fn core_protocol_callbacks_resume_across_collection_without_entering_key_comparison() {
    let rt = Runtime::new();
    let code = compile_function(r#"function f() {
      const T=__rt.structType('T',__rt.formVector('n'),false);
      const a=__rt.structNew(T,7), b=__rt.structNew(T,7);
      let calls=0;
      __rt.cljCoreResolver((op,x)=> {
        if(op==='formEquals') { __rt.need(90,1); return (x,y)=>{calls++; __rt.need(91,1); return __rt.structField(x,'n')===__rt.structField(y,'n');}; }
        if(op==='formHash') return x=>{calls++; return 123;};
        if(op==='cljGet') return (x,k,d)=>{calls++; __rt.need(92,1); throw new Error('lookup');};
        return null;
      });
      if(!__rt.formEquals(a,b)||calls!==1)throw new Error('equiv');
      if(!__rt.formEquals(a,a)||calls!==1)throw new Error('identity shortcut');
      if(__rt.formHash(a)!==123||calls!==2)throw new Error('hash');
      // All language collection key/member operations still use frozen form
      // comparison. Even equal user values retain separate identity keys.
      let m=__rt.formMap(a,1,b,2), s=__rt.formSet(a,b);
      if(__rt.cljCount(m)!==2||__rt.cljCount(s)!==2)throw new Error('structural keys');
      m=__rt.cljAssoc(m,a,3); s=__rt.cljDisj(s,b);
      if(__rt.cljGet(m,b,null)!==2||!__rt.cljContains(s,a)||__rt.cljContains(s,b))throw new Error('lookup keys');
      if(__rt.cljCount(__rt.cljDissoc(m,b))!==1||calls!==2)throw new Error('key callbacks');
      if(__rt.formEquals(__rt.formVector(a),__rt.formVector(b)))throw new Error('stored values');
      __rt.formHash(__rt.formVector(a));
      if(calls!==2)throw new Error('nested callbacks');
      try{__rt.cljGet(a,0,null);throw new Error('not thrown');}
      catch(e){if(e.message!=='lookup'||calls!==3)throw e;}
      // A resolver's missing entry falls back to record operations without
      // redispatch; that path can itself realize a lazy sequence.
      const R=__rt.structType('R',__rt.formVector('x'),true);
      if(__rt.cljCount(__rt.structNew(R,1))!==1)throw new Error('fallback');
      const r1=__rt.structNew(R,7), r2=__rt.structNew(R,7);
      if(__rt.cljCount(__rt.formMap(r1,1,r2,2))!==1||__rt.cljCount(__rt.formSet(r1,r2))!==1||calls!==3)throw new Error('record keys');
      return calls;
    }"#).unwrap();
    let mut task = rt.spawn(&code, &[]).unwrap();
    let mut outcome = task.run().unwrap();
    for _ in 0..3 {
        assert!(matches!(outcome, RunOutcome::Waiting(_)), "{outcome:?}");
        rt.collect().unwrap();
        let request = task.take_producer_requests().pop().unwrap();
        let ready = rt.root(Primitive::Null);
        let resumed = rt
            .complete(request.key(), request.producer(), Completion::Ready(&ready))
            .unwrap()
            .into_runnable()
            .pop()
            .unwrap();
        (task, outcome) = thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || {
                let outcome = resumed.run().unwrap();
                (resumed, outcome)
            })
            .unwrap()
            .join()
            .unwrap();
    }
    assert_eq!(complete(outcome).view(), ValueView::Number(3.0));
}

#[test]
fn lazy_struct_sequence_callbacks_survive_collection_and_small_stack_resumption() {
    let code = compile_function(
        r#"function f(record) {
      const T=__rt.structType('T',__rt.formVector('n'),record);
      let calls=0;
      __rt.cljCoreResolver((op,x,lazy)=>{
        __rt.need(90,1);
        if(op==='cljSeq')return lazy?()=>__rt.cljSeq(x):()=>{
          calls++; __rt.need(92,1); return __rt.formList(7,8);
        };
        if(op==='cljFirst')return ()=>__rt.cljFirst(__rt.cljSeq(x));
        return null;
      });
      const s=__rt.lazySeq(()=>{__rt.need(91,1);return __rt.structNew(T,3);});
      if(__rt.cljFirst(s)!==7)throw new Error('lazy first');
      __rt.need(93,1);
      if(__rt.cljCount(__rt.cljSeq(s))!==2)throw new Error('lazy seq');
      if(record){
        const entries=__rt.formList(__rt.formVector(__rt.formKeyword(null,'n'),3));
        if(!__rt.formEquals(s,entries)||__rt.formHash(s)!==__rt.formHash(entries))throw new Error('structural record seq');
        if(__rt.cljGet(__rt.formMap(s,11),entries,null)!==11)throw new Error('structural lazy key');
      }
      return calls;
    }"#,
    )
    .unwrap();
    for record in [false, true] {
        let rt = Runtime::new();
        let flag = rt.root(Primitive::Bool(record));
        let mut task = rt.spawn(&code, &[&flag]).unwrap();
        let mut outcome = task.run().unwrap();
        for dependency in [91, 90, 92, 93] {
            assert!(matches!(outcome, RunOutcome::Waiting(_)), "{outcome:?}");
            rt.collect().unwrap();
            let request = task.take_producer_requests().pop().unwrap();
            assert_eq!(request.key().dependency(), DependencyId::new(dependency));
            let ready = rt.root(Primitive::Null);
            let resumed = rt
                .complete(request.key(), request.producer(), Completion::Ready(&ready))
                .unwrap()
                .into_runnable()
                .pop()
                .unwrap();
            (task, outcome) = thread::Builder::new()
                .stack_size(64 * 1024)
                .spawn(move || {
                    let outcome = resumed.run().unwrap();
                    (resumed, outcome)
                })
                .unwrap()
                .join()
                .unwrap();
        }
        assert_eq!(complete(outcome).view(), ValueView::Number(2.0));
    }
}

#[test]
fn realized_lazy_records_publish_the_receiver_retained_for_protocol_dispatch() {
    let rt = Runtime::new();
    let lazy = call(
        &rt,
        r#"function f(){
      const R=__rt.structType('R',__rt.formVector('n'),true);
      const r=__rt.formWithMeta(__rt.structNew(R,3),__rt.formMap('hidden',{}));
      const s=__rt.lazySeq(()=>r);
      __rt.cljSeq(s);
      return s;
    }"#,
        &[],
    );
    let PublishError::NotShareable { path, found, .. } = rt.publish(&lazy).unwrap_err() else {
        panic!("published a lazy record retaining task-owned metadata");
    };
    assert!(path.to_string().contains(".realized.meta"), "{path}");
    assert_eq!(found, "object");
}

#[test]
fn shared_lazy_records_check_retained_receivers_during_realization() {
    let make = compile_function(
        r#"function make(bad) {
      return __rt.lazySeq(()=>{
        __rt.need(94,1);
        const R=__rt.structType('R',__rt.formVector('n'),true);
        const hidden=bad?{}:__rt.formVector(8);
        return __rt.formWithMeta(__rt.structNew(R,3),__rt.formMap('hidden',hidden));
      });
    }"#,
    )
    .unwrap();
    let read = compile_function(
        r#"function read(s) {
      __rt.cljCoreResolver((op,x,lazy)=>{
        if(!lazy||__rt.formKind(x)!=='record')throw new Error('lost record receiver');
        if(op==='cljSeq')return ()=>{
          const hidden=__rt.formGet(__rt.formMeta(x),'hidden',null);
          if(__rt.formNth(hidden,0)!==8)throw new Error('lost receiver metadata');
          return __rt.formList(7,8);
        };
        return null;
      });
      try { return __rt.cljFirst(__rt.cljSeq(s)); } catch(e) { return String(e); }
    }"#,
    )
    .unwrap();
    for bad in [false, true] {
        let rt = Runtime::new();
        let flag = rt.root(Primitive::Bool(bad));
        let lazy = complete(rt.spawn(&make, &[&flag]).unwrap().run().unwrap());
        // Publish before the thunk constructs its record. Its metadata is
        // absent from the cached structural sequence, but retained for dispatch.
        rt.publish(&lazy).unwrap();
        let producer = rt.spawn(&read, &[&lazy]).unwrap();
        assert!(matches!(producer.run().unwrap(), RunOutcome::Waiting(_)));
        let waiter = rt.spawn(&read, &[&lazy]).unwrap();
        assert!(matches!(waiter.run().unwrap(), RunOutcome::Waiting(_)));
        rt.collect().unwrap();
        let request = producer.take_producer_requests().pop().unwrap();
        assert_eq!(request.key().dependency(), DependencyId::new(94));
        let ready = rt.root(Primitive::Null);
        let resumed = rt
            .complete(request.key(), request.producer(), Completion::Ready(&ready))
            .unwrap()
            .into_runnable()
            .pop()
            .unwrap();
        let produced = complete(resumed.run().unwrap());
        let woken = rt.take_lazy_wakeups();
        assert_eq!(woken.len(), 1);
        assert_eq!(woken[0].id(), waiter.id());
        rt.collect().unwrap();
        let observed = complete(waiter.run().unwrap());
        for result in [produced, observed] {
            if bad {
                let message = result.string().unwrap().to_string_lossy();
                assert!(
                    message.starts_with(
                        "TypeError: a shared lazy sequence realized a value it cannot share:"
                    ),
                    "{message}"
                );
                assert!(message.contains(".meta"), "{message}");
            } else {
                assert_eq!(result.view(), ValueView::Number(7.0));
            }
        }
    }
}

#[test]
fn missing_core_protocol_resolves_once_when_record_comparison_realizes_fields() {
    let rt = Runtime::new();
    let code = compile_function(
        r#"function f() {
      const R=__rt.structType('R',__rt.formVector('x'),true);
      const a=__rt.structNew(R,__rt.lazySeq(()=>{__rt.need(92,1);return __rt.formList(7);}));
      const b=__rt.structNew(R,__rt.formList(7));
      let resolutions=0;
      __rt.cljCoreResolver(()=>{resolutions++;return null;});
      if(!__rt.formEquals(a,b))throw new Error('record equality');
      return resolutions;
    }"#,
    )
    .unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(_)));
    rt.collect().unwrap();
    let request = task.take_producer_requests().pop().unwrap();
    let ready = rt.root(Primitive::Null);
    let resumed = rt
        .complete(request.key(), request.producer(), Completion::Ready(&ready))
        .unwrap()
        .into_runnable()
        .pop()
        .unwrap();
    let result = thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(move || complete(resumed.run().unwrap()))
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(result.view(), ValueView::Number(1.0));
}

/// The `seq`, `rest` and `next` of a list or vector share its trie
/// (light-cljs-502) and publish by their own elements: a task-owned object
/// in the hidden prefix does not stop publication, and one among the
/// remaining elements is reported at its position in the tail, past the
/// first trie leaf too.
#[test]
fn shared_seq_views_publish_only_their_own_elements() {
    let rt = Runtime::new();
    let made = call(
        &rt,
        "function make() {
           let head = __rt.formVector({});
           let tail = __rt.formVector();
           for (let i = 0; i < 40; i++) {
             head = __rt.cljConj(head, i);
             tail = __rt.cljConj(tail, i === 35 ? {} : i);
           }
           for (let i = 0; i < 33; i++) tail = __rt.cljNext(tail);
           return __rt.formVector(
             __rt.cljRest(head),
             __rt.cljNext(__rt.cljSeq(head)),
             __rt.cljRest(__rt.formList({}, 1)),
             tail);
         }",
        &[],
    );
    let [rest, next, list_rest, tail] = rt.scope(|s| {
        let v = s.local(&made).unwrap();
        std::array::from_fn(|i| s.persist(s.form_nth(v, i).unwrap()))
    });
    for view in [&rest, &next, &list_rest] {
        rt.publish(view).unwrap();
    }
    rt.scope(|s| {
        let tail = s.local(&tail).unwrap();
        assert_eq!(s.form_kind(tail), Some(FormKind::List));
        assert_eq!(s.form_count(tail), Ok(7));
    });
    let PublishError::NotShareable { path, found, .. } = rt.publish(&tail).unwrap_err() else {
        panic!("published a tail holding an object");
    };
    assert_eq!((path.to_string().as_str(), found), ("value[2]", "object"));
}

#[test]
fn compiler_literal_value_equality_hashing_and_queue_keys_survive_collection() {
    let rt = Runtime::new();
    let values = call(
        &rt,
        r#"function make() {
      const literal=(tag,payload)=>__rt.formCompilerLiteral(tag,payload);
      return __rt.formVector(
        literal('instant','2024-01-02T03:04:05.000Z'),literal('instant',1704164645000),literal('instant',0),
        literal('uuid','123E4567-E89B-12D3-A456-426614174000'),
        literal('uuid','123e4567-e89b-12d3-a456-426614174000'),literal('uuid','other'),
        literal('queue',__rt.formVector(1,__rt.formList(2))),
        literal('queue',__rt.formVector(1,__rt.formVector(2))),
        literal('queue',__rt.formVector(1,3)));
    }"#,
        &[],
    );
    rt.publish(&values).unwrap();
    rt.collect().unwrap();
    let result = call(
        &rt,
        r#"function check(values) {
      const eq=(a,b)=>__rt.formEquals(a,b),hash=x=>__rt.formHash(x);
      if(__rt.formLiteralPayload(__rt.formNth(values,0))!=='2024-01-02T03:04:05.000Z')throw new Error('instant payload identity');
      if(__rt.formLiteralPayload(__rt.formNth(values,3))!=='123E4567-E89B-12D3-A456-426614174000')throw new Error('UUID payload identity');
      const clipped=__rt.formCompilerLiteral('instant',0.9),zero=__rt.formCompilerLiteral('instant',0);
      if(!eq(clipped,zero)||hash(clipped)!==hash(zero))throw new Error('instant TimeClip');
      let map=__rt.formMap();
      for(let i=0;i<20;i++)map=__rt.formAssoc(map,i,i);
      for(let i=0;i<9;i+=3){
        const a=__rt.formNth(values,i),b=__rt.formNth(values,i+1),c=__rt.formNth(values,i+2);
        if(!eq(a,b)||eq(a,c)||eq(c,a)||hash(a)!==hash(b))throw new Error('value equality/hash');
        map=__rt.formAssoc(map,a,i);
        if(__rt.formGet(map,b,null)!==i)throw new Error('hashed literal key');
        const meta=__rt.formWithMeta(a,__rt.formMap('tag',true));
        if(!eq(meta,b)||hash(meta)!==hash(b))throw new Error('metadata');
      }
      const queue=__rt.formNth(values,6),vector=__rt.formVector(1,__rt.formVector(2));
      const list=__rt.formList(1,__rt.formList(2));
      const lazy=__rt.lazySeq(()=>list);
      for(const sequence of [vector,list,lazy]){
        if(!eq(queue,sequence)||!eq(sequence,queue)||hash(queue)!==hash(sequence))throw new Error('sequential queue');
        if(__rt.formGet(map,sequence,null)!==6)throw new Error('sequential queue key');
      }
      if(eq(queue,__rt.formVector(1))||eq(__rt.formVector(1),queue))throw new Error('queue length');
      for(const tag of ['js-array','js-object','regex']){
        const payload=tag==='js-array'?__rt.formVector():tag==='js-object'?__rt.formMap():'a';
        const a=__rt.formCompilerLiteral(tag,payload),b=__rt.formCompilerLiteral(tag,payload);
        if(eq(a,b)||!eq(a,a)||hash(a)!==hash(a))throw new Error('identity literal');
      }
      return true;
    }"#,
        &[&values],
    );
    assert!(matches!(result.view(), ValueView::Bool(true)));
}

#[test]
fn compiler_literals_trace_publish_and_realize_only_explicit_payloads() {
    let rt = Runtime::new();
    let literal = call(
        &rt,
        r#"function make() {
      const leaf=__rt.formVector('\ud800', 1);
      const lazy=__rt.lazySeq(() => __rt.formList(leaf));
      const payload=__rt.formVector(lazy);
      const literal=__rt.formCompilerLiteral('js-array',payload);
      const meta=__rt.formMap(__rt.formKeyword(null,'tag'),leaf);
      return __rt.formWithMeta(literal,meta);
    }"#,
        &[],
    );
    rt.publish(&literal).unwrap();
    let weak = rt.scope(|s| {
        let (_, payload) = s.form_compiler_literal(s.local(&literal).unwrap()).unwrap();
        s.downgrade(payload)
    });
    rt.collect().unwrap();
    assert!(weak.is_live());
    let result = call(
        &rt,
        r#"function check(x) {
      if(__rt.formKind(x)!=='compiler-literal'||__rt.formName(x)!=='js-array')throw new Error('kind');
      const result=__rt.cljRealizeAll(x), payload=__rt.formLiteralPayload(result);
      if(__rt.formKind(__rt.formNth(payload,0))!=='list')throw new Error('realization');
      if(__rt.formMeta(result)!==__rt.formMeta(x))throw new Error('metadata identity');
      if(__rt.formEquals(x,__rt.formCompilerLiteral('js-array',__rt.formLiteralPayload(x))))throw new Error('identity');
      return result;
    }"#,
        &[&literal],
    );
    rt.publish(&result).unwrap();
    rt.scope(|s| {
        let (_, payload) = s.form_compiler_literal(s.local(&result).unwrap()).unwrap();
        let list = s.form_nth(payload, 0).unwrap();
        let leaf = s.form_nth(list, 0).unwrap();
        assert_eq!(
            s.string(s.form_nth(leaf, 0).unwrap()).unwrap().code_units(),
            &[0xd800]
        );
    });
    drop(literal);
    drop(result);
    rt.collect().unwrap();
    assert!(!weak.is_live());
    for expression in ["{}", "[]", "Object.freeze({})"] {
        let result = call(
            &rt,
            &format!(
                "function f() {{ return __rt.formCompilerLiteral('js-array',__rt.formVector({expression})); }}"
            ),
            &[],
        );
        let PublishError::NotShareable { path, .. } = rt.publish(&result).unwrap_err() else {
            panic!("published {expression}");
        };
        assert_eq!(path.steps()[0], PathStep::NativeEdge(0));
    }
    for (tag, payload) in [("js-array", "[]"), ("js-object", "{}"), ("unknown", "null")] {
        let error = failure(
            &rt,
            &format!("function f() {{ return __rt.formCompilerLiteral('{tag}',{payload}); }}"),
        );
        assert!(error.to_string().contains("valid compiler literal payload"));
    }
}
