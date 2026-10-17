// light-cljs builds the quota-aware V8 source patch instead of downloading the
// ABI-incompatible upstream archive. Upstream still owns toolchains and GN.
#[path = "upstream_build.rs"]
mod upstream;

fn main() {
    use std::{env, path::PathBuf, process::Command};
    for file in ["build.rs", "upstream_build.rs", "prepare.bb", "engine-budget.patch"] {
        println!("cargo:rerun-if-changed={file}");
    }
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    if env::var_os("DOCS_RS").is_none() && env::var_os("DENO_TRYBUILD").is_none() {
        if env::var_os("RUSTY_V8_ARCHIVE").is_some() {
            assert!(env::var_os("RUSTY_V8_SRC_BINDING_PATH").is_some(),
                "RUSTY_V8_ARCHIVE requires matching RUSTY_V8_SRC_BINDING_PATH from this patched engine");
            // Override an inherited V8_FROM_SOURCE for this explicit pair.
            unsafe { env::remove_var("V8_FROM_SOURCE"); }
        } else {
            let cache = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("source");
            let status = Command::new("bb").arg(manifest.join("prepare.bb")).arg(&cache)
                .status().expect("building the patched V8 engine requires babashka (bb)");
            assert!(status.success(), "preparing the pinned V8 sources failed");
            let root = cache.join("v8-152.2.0");
            env::set_current_dir(&root).expect("enter V8 sources");
            // This single-threaded build process launches upstream after setting
            // its source root. It does not modify Cargo's parent environment.
            unsafe {
                env::set_var("CARGO_MANIFEST_DIR", &root);
                env::set_var("V8_FROM_SOURCE", "1");
            }
        }
    }
    upstream::main();
}
