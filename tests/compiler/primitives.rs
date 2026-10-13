//! Numeric coercions, rounding, bit and unchecked arithmetic, truthiness and
//! the upstream primitives tests.

/// Complete sources in both compiler phases and both host lifecycles, each run in
/// QuickJS and Node (see `tests/support/phase_oracle.rs`).
mod phase_oracles {
    use crate::phase_oracle::{Case, run};

    #[test]
    fn numeric_operations() {
        run(&[
            Case::observed(
                "numeric-rounding",
                include_str!("../fixtures/phase-oracles/primitives/numeric-rounding.cljs"),
                r#"{"id":"numeric-rounding","outcome":{"status":"returned","value":{"$vector":[-2147483648,1,4294967297,4294967297,{"$number":"-0"},0,{"$number":"+Infinity"},0,{"$number":"NaN"},300,65537,"3",false]}},"trace":[],"stdout":""}"#,
            ),
            Case::observed(
                "numeric-extrema",
                include_str!("../fixtures/phase-oracles/primitives/numeric-extrema.cljs"),
                r#"{"id":"numeric-extrema","outcome":{"status":"returned","value":{"$vector":[{"$number":"-0"},0,{"$number":"-0"},0,{"$number":"NaN"},{"$number":"NaN"},"x",0,{"$number":"+Infinity"}]}},"trace":[],"stdout":""}"#,
            ),
            Case::observed(
                "numeric-bit-width",
                include_str!("../fixtures/phase-oracles/primitives/numeric-bit-width.cljs"),
                r#"{"id":"numeric-bit-width","outcome":{"status":"returned","value":{"$vector":[-2147483648,true,1,-2147483648,-1073741824,2147483647,4294967295,32,0,-1,3]}},"trace":[],"stdout":""}"#,
            ),
            Case::observed(
                "numeric-unchecked",
                include_str!("../fixtures/phase-oracles/primitives/numeric-unchecked.cljs"),
                r#"{"id":"numeric-unchecked","outcome":{"status":"returned","value":{"$vector":[0,2147483648,1,-2147483649,24,4294967296,1.5,2147483648,2147483648,-2147483649,-2147483649,{"$number":"-0"},2147483648,3]}},"trace":[],"stdout":""}"#,
            ),
            Case::observed(
                "numeric-unchecked-identity",
                include_str!(
                    "../fixtures/phase-oracles/primitives/numeric-unchecked-identity.cljs"
                ),
                r#"{"id":"numeric-unchecked-identity","outcome":{"status":"returned","value":{"$vector":[300,65537,65,"x",false,false,false]}},"trace":[],"stdout":""}"#,
            ),
            Case::observed(
                "numeric-truth",
                include_str!("../fixtures/phase-oracles/primitives/numeric-truth.cljs"),
                r#"{"id":"numeric-truth","outcome":{"status":"returned","value":{"$vector":[true,true,true,true,false,false,false,false,false,true,true,true,true,false,true,true,false,false,true]}},"trace":[],"stdout":""}"#,
            ),
            Case::observed(
                "numeric-array-coercions",
                include_str!("../fixtures/phase-oracles/primitives/numeric-array-coercions.cljs"),
                r#"{"id":"numeric-array-coercions","outcome":{"status":"returned","value":{"$vector":[1,2,3,4,5,6,7,8]}},"trace":[],"stdout":""}"#,
            ),
        ]);
    }

    #[test]
    fn primitives_test_bodies() {
        run(&[
            Case::expression(
                "misc",
                include_str!("../fixtures/phase-oracles/primitives/misc.cljs"),
            ),
            Case::expression(
                "char-496",
                include_str!("../fixtures/phase-oracles/primitives/char-496.cljs"),
            ),
            Case::expression(
                "arithmetic",
                include_str!("../fixtures/phase-oracles/primitives/arithmetic.cljs"),
            ),
        ]);
    }
}
