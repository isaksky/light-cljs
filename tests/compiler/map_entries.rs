//! Map entry interfaces, indexes, type changes and key/val traversal effects.

/// Complete sources in both compiler phases and both host lifecycles, each run in
/// QuickJS and Node (see `tests/support/phase_oracle.rs`).
mod phase_oracles {
    use crate::phase_oracle::{Case, run};

    #[test]
    fn map_entry_test_bodies() {
        run(&[
Case::expression("all-map-entry-tests", include_str!("../fixtures/phase-oracles/map-entries/all-map-entry-tests.cljs")).printing("[true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true]"),
Case::expression("map-entry-interface-tests", include_str!("../fixtures/phase-oracles/map-entries/map-entry-interface-tests.cljs")),
]);
    }

    #[test]
    fn entry_helpers() {
        run(&[
            Case::observed_during_expansion(
                "entry-type-changes",
                include_str!("../fixtures/phase-oracles/map-entries/entry-type-changes.cljs"),
                r#"{"id":"entry-type-changes","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"#,
            ),
            Case::observed_during_expansion(
                "entry-indexes",
                include_str!("../fixtures/phase-oracles/map-entries/entry-indexes.cljs"),
                r#"{"id":"entry-indexes","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"#,
            ),
            Case::observed_during_expansion(
                "keys-traversal-effects",
                include_str!("../fixtures/phase-oracles/map-entries/keys-traversal-effects.cljs"),
                r#"{"id":"keys-traversal-effects","outcome":{"status":"returned","value":true},"trace":["seq","made","next","rested","first","next"],"stdout":""}"#,
            ),
            Case::observed_during_expansion(
                "vals-traversal-effects",
                include_str!("../fixtures/phase-oracles/map-entries/vals-traversal-effects.cljs"),
                r#"{"id":"vals-traversal-effects","outcome":{"status":"returned","value":true},"trace":["seq","made","next","rested","first","next"],"stdout":""}"#,
            ),
        ]);
    }
}
