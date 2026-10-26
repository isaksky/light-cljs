use big_boy_pants_js::frontend::compile_script;
use big_boy_pants_js::{RunOutcome, Runtime, ValueView};

#[test]
fn get_traps_follow_ecmascript() {
    let code = compile_script("proxy-get", include_str!("fixtures/js/proxy_get.js")).unwrap();
    let rt = Runtime::new();
    let result = rt.spawn(&code, &[]).unwrap().run().unwrap();
    assert!(
        matches!(result, RunOutcome::Complete(ref v) if v.view() == ValueView::Bool(true)),
        "{result:?}"
    );
}
