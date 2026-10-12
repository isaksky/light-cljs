//! Promoted set traversal uses retained trie routes without replaying key effects.
use light_cljs::MacroHost;
use std::io::Write;
use std::process::{Command, Stdio};

fn both(source: &str) {
    let mut host = MacroHost::new();
    for phase in [false, true] {
        let text = if phase {
            format!("(defmacro set-order [] (list 'quote {source})) (set-order)")
        } else {
            source.to_owned()
        };
        let js = light_cljs::compile_with_macros(&text, &mut host).unwrap();
        let rt = rquickjs::Runtime::new().unwrap();
        let ctx = rquickjs::Context::full(&rt).unwrap();
        ctx.with(|ctx| assert!(ctx.eval::<bool, _>(js.as_bytes()).unwrap(), "{text}"));
        let mut child = Command::new("node")
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
            .write_all(format!("console.log({js});").as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), "true");
    }
}

#[test]
fn promoted_set_order_and_immutable_versions() {
    both(
        r##"(let [s (set [7 3 6 1 5 0 4 2]) p (conj s 8) q (disj p 1 3 5 7) r (conj q nil 33)]
      (and (= [7 3 6 1 5 0 4 2] (vec s))
           (= [0 1 2 3 4 5 6 7 8] (vec p))
           (= [0 2 4 6 8] (vec q))
           (= [nil 0 33 2 4 6 8] (vec r))
           (= [0 1 2 3 4 5 6 7 8] (reduce conj [] p))
           (= "#{0 1 2 3 4 5 6 7 8}" (pr-str p))))"##,
    );
}

#[test]
fn traversal_retains_key_representatives_without_rehash_or_equality_effects() {
    both(
        r##"(let [events (atom [])
        keys (mapv (fn [x] (with-meta
          (reify IHash (-hash [_] (swap! events conj [:hash x]) (mod x 3))
                 IEquiv (-equiv [self other] (swap! events conj [:equiv x]) (identical? self other))) {:id x})) (range 12))
        s (set keys) _ (reset! events [])
        first-pass (mapv (fn [x] (:id (meta x))) s)
        second-pass (mapv (fn [x] (:id (meta x))) s)]
      (and (= [0 3 6 9 1 4 7 10 2 5 8 11] first-pass second-pass)
           (= [] @events)
           (every? true? (map identical? (mapv (fn [i] (nth keys i)) first-pass) (vec s)))))"##,
    );
}
