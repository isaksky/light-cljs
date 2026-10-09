//! Compile this exact transport against baseline/current libraries and compare
//! stdout. Covers every supported/dispositioned corpus result, effect and error.
#[path = "../../tests/support/sci_protocol.rs"]
#[allow(dead_code)]
mod protocol;
#[path = "../../tests/support/sci_native.rs"]
#[allow(dead_code)]
mod native;
fn main() {
    let cases = protocol::cases(include_str!("../../tests/fixtures/sci-outcomes.tsv")).unwrap();
    for case in cases {
        let profile = if case.id.starts_with("sci-context-reader-tag-syntax-error-") {
            "reader-tags"
        } else {
            case.profile
        };
        let observation = native::run(case.source, profile).unwrap();
        println!("{}\t{:?}", case.id, observation);
    }
}
