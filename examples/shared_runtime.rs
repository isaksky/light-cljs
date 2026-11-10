//! Print the shared-heap profile of the runtime exactly as the compiler
//! embeds it (`runtime_manifest::SHARED_RUNTIME`, assembled by build.rs from
//! src/js_runtime.js and src/js_shared_profile.js; docs/real-core-design.md).
//! The macro ledger's core-boot step
//! (crates/big-boy-pants-js/scripts/macro-ledger.bb) exports this text as a
//! Goose program image and boots it on the Goose VM.
//!
//! `cargo run --release --features goose-macros --example shared_runtime > shared.js`
use std::io::Write;

fn main() {
    let text = light_cljs::runtime_manifest::SHARED_RUNTIME;
    std::io::stdout()
        .write_all(text.as_bytes())
        .expect("write the shared-heap runtime to standard output");
}
