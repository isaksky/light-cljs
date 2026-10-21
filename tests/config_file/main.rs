//! Tests of `light-cljs.edn`, the configuration file
//! (docs/build-configuration.md): one module per concern, linked into a single
//! test binary (`--test config_file`). Library-level tests pass an environment
//! lookup map; they never set process environment variables.

#[path = "../support/temp_dir.rs"]
#[allow(dead_code)]
mod temp_dir;

mod reader;
